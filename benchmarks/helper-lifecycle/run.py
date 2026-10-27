import argparse
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import queue
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import time


ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def command(arguments, **options):
    try:
        return subprocess.run(list(map(str, arguments)), cwd=options.pop('cwd', ROOT),
                              capture_output=True, text=True, check=True,
                              timeout=300, **options).stdout
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f'{error}\n{error.stderr}') from error


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runtime_sources(revision, directory):
    archived = subprocess.run(['git', 'archive', '--format=tar', revision, 'runtime/src'],
                              cwd=ROOT, capture_output=True, check=True, timeout=30).stdout
    directory.mkdir()
    manifest = {}
    with tarfile.open(fileobj=io.BytesIO(archived)) as archive:
        for member in archive:
            path = Path(member.name)
            if not member.isfile() or path.parts[:2] != ('runtime', 'src'):
                continue
            if len(path.parts) != 3 or path.suffix != '.rs':
                continue
            contents = archive.extractfile(member).read()
            (directory / path.name).write_bytes(contents)
            manifest[path.as_posix()] = hashlib.sha256(contents).hexdigest()
    if 'runtime/src/lib.rs' not in manifest:
        raise RuntimeError('captured revision has no runtime library')
    return manifest


def build(directory, runtime):
    rustc = os.environ.get('ZORE_RUSTC', 'rustc')
    linker_arguments = []
    if os.name == 'nt' and os.environ.get('VCToolsInstallDir'):
        linker = Path(os.environ['VCToolsInstallDir']) / 'bin/Hostx64/x64/link.exe'
        if not linker.is_file():
            raise RuntimeError(f'native MSVC linker is absent: {linker}')
        linker_arguments.append(f'-Clinker={linker}')
    library = directory / 'libzore_runtime.rlib'
    command([rustc, '--edition=2024', '--crate-name=zore_runtime', '--crate-type=rlib',
             '--cfg=zore_test_hooks', '-Dwarnings', '-Copt-level=2', '-Cpanic=abort',
             runtime / 'lib.rs', '-o', library])
    executable = directory / ('probe.exe' if os.name == 'nt' else 'probe')
    command([rustc, '--edition=2024', '-Dwarnings', '-Copt-level=2', '-Cpanic=abort',
             *linker_arguments, '--extern', f'zore_runtime={library}', HERE / 'probe.rs', '-o', executable])
    return executable, {'runtime_library_sha256': digest(library),
                        'executable_sha256': digest(executable), 'probe_sha256': digest(HERE / 'probe.rs'),
                        'linker_arguments': linker_arguments}


def read_lines(stream, output):
    try:
        for line in stream:
            output.put(line.rstrip('\r\n'))
    finally:
        output.put(None)


def snapshot(output, timeout):
    try:
        line = output.get(timeout=timeout)
    except queue.Empty as error:
        raise TimeoutError('helper probe did not respond') from error
    if line is None:
        raise RuntimeError('helper probe exited during protocol')
    return json.loads(line)


def validate(counters, quiet=False):
    threads = counters['helper_threads']
    active = counters['helper_active_jobs']
    idle = counters['idle_helpers']
    if not (0 <= active <= threads <= 512 and 0 <= idle <= threads and active + idle <= threads):
        raise RuntimeError(f'inconsistent helper counts: {counters}')
    if counters['helper_starts'] - counters['helper_retirements'] != threads:
        raise RuntimeError(f'inconsistent helper lifecycle: {counters}')
    retained = ['tasks', 'queued_tasks', 'blocked_workers', 'socket_handles',
                'reactor_registrations', 'live_timers', 'timer_threads', 'readiness_threads',
                'allocated_bytes']
    if any(counters[key] for key in retained):
        raise RuntimeError(f'unexpected workload resource: {counters}')
    if quiet and (active or counters['queued_helpers']):
        raise RuntimeError(f'helper jobs have not finished: {counters}')


def resource_sample(process):
    return {'rss_bytes': process.memory_info().rss, 'threads': process.num_threads(),
            'handles' if os.name == 'nt' else 'fds': process.num_handles() if os.name == 'nt' else process.num_fds()}


def measure(executable, population, cycles, checkpoints, sample_seconds, timeout,
            reuse_gap, empty_wake_seconds=0, wake_interval=0.5):
    import psutil

    started = time.monotonic()
    child = None
    process = None
    output = queue.Queue()
    reader = None
    errors = []
    error_reader = None
    result = {'population': population, 'cycles': cycles, 'empty_wake_seconds': empty_wake_seconds,
              'checkpoints': [], 'notifications': [], 'status': 'failed'}

    def response():
        remaining = timeout - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError('helper measurement exceeded its deadline')
        return snapshot(output, remaining)

    def send(value):
        child.stdin.write(value + '\n')
        child.stdin.flush()

    def record(phase, counters, cycle, idle_origin=None, requested_idle=None):
        quiet = phase not in ['active', 'reuse_active']
        validate(counters, quiet)
        if not quiet and (counters['helper_active_jobs'] != min(population, 512)
                          or counters['queued_helpers'] != max(0, population - 512)):
            raise RuntimeError(f'helper population did not settle: {counters}')
        now = time.monotonic()
        samples = []
        sample_deadline = now + sample_seconds
        while True:
            samples.append({'elapsed_seconds': time.monotonic() - started, **resource_sample(process)})
            if time.monotonic() >= sample_deadline:
                break
            time.sleep(0.05)
        result['checkpoints'].append({
            'phase': phase, 'cycle': cycle, 'runtime': counters,
            'runtime_snapshot_elapsed_seconds': now - started,
            'requested_idle_seconds': requested_idle,
            'observed_idle_seconds': None if idle_origin is None else now - idle_origin,
            'raw_samples': samples,
        })

    try:
        child = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True, bufsize=1,
                                 env={**os.environ, 'ZORE_CHECK_LEAKS': '1'})
        process = psutil.Process(child.pid)
        reader = threading.Thread(target=read_lines, args=(child.stdout, output), daemon=True)
        reader.start()
        error_reader = threading.Thread(target=lambda: errors.append(child.stderr.read()), daemon=True)
        error_reader.start()
        record('initial', response(), 0)
        for cycle in range(1, cycles + 1):
            send('snapshot')
            record('baseline', response(), cycle)
            send(f'start {population}')
            record('active', response(), cycle)
            send('release')
            record('completed', response(), cycle)
            time.sleep(reuse_gap)
            send(f'start {population}')
            record('reuse_active', response(), cycle)
            send('release')
            completed = response()
            idle_origin = time.monotonic()
            record('reuse_completed', completed, cycle)
            notifications = []
            if empty_wake_seconds:
                count = math.floor(empty_wake_seconds / wake_interval)
                notifications = [index * wake_interval for index in range(1, count + 1)]
            events = [(offset, 'snapshot') for offset in checkpoints]
            events += [(offset, 'nudge') for offset in notifications]
            for offset, event in sorted(events):
                delay = idle_origin + offset - time.monotonic()
                if delay > 0:
                    time.sleep(delay)
                send(event)
                counters = response()
                if event == 'snapshot':
                    record('idle', counters, cycle, idle_origin, offset)
                else:
                    validate(counters, True)
                    result['notifications'].append({'cycle': cycle,
                                                    'elapsed_idle_seconds': time.monotonic() - idle_origin,
                                                    'runtime': counters})
        send('stop')
        child.stdin.close()
        child.wait(timeout=max(1, timeout - (time.monotonic() - started)))
        if child.returncode:
            raise RuntimeError(f'helper probe exited with {child.returncode}')
        result['status'] = 'ok'
    except (OSError, RuntimeError, TimeoutError, ValueError, KeyError, subprocess.TimeoutExpired, psutil.Error) as error:
        result['failure'] = str(error)
    finally:
        result['terminated_by_harness'] = child is not None and child.poll() is None
        if child is not None:
            if result['terminated_by_harness']:
                child.kill()
            child.wait(timeout=5)
            for thread in [reader, error_reader]:
                if thread is not None and thread.ident is not None:
                    thread.join(timeout=5)
            if error_reader is None:
                errors.append(child.stderr.read())
            for stream in [child.stdin, child.stdout, child.stderr]:
                try:
                    stream.close()
                except OSError:
                    pass
        result['exit_code'] = None if child is None else child.returncode
        result['stderr'] = ''.join(errors)
        if result['status'] == 'ok' and result['stderr']:
            result['status'] = 'failed'
            result['failure'] = 'unexpected helper probe stderr'
        result['elapsed_seconds'] = time.monotonic() - started
    return result


def main():
    parser = argparse.ArgumentParser(description='Paired helper-pool lifecycle measurements')
    parser.add_argument('--baseline-revision', required=True)
    parser.add_argument('--after-revision', default='HEAD')
    parser.add_argument('--populations', nargs='+', type=int, default=[32, 512, 640])
    parser.add_argument('--repetitions', type=int, default=2)
    parser.add_argument('--cycles', type=int, default=4)
    parser.add_argument('--checkpoints', nargs='+', type=float, default=[1, 6, 15, 30, 60])
    parser.add_argument('--sample-seconds', type=float, default=0.2)
    parser.add_argument('--timeout', type=float, default=360)
    parser.add_argument('--reuse-gap', type=float, default=0.05)
    parser.add_argument('--control-cycles', type=int, default=2)
    parser.add_argument('--control-wake-seconds', type=float, default=10)
    parser.add_argument('--wake-interval', type=float, default=0.5)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    durations = [*args.checkpoints, args.sample_seconds, args.timeout, args.reuse_gap,
                 args.control_wake_seconds, args.wake_interval]
    if (min(args.populations) < 1 or min(args.repetitions, args.cycles, args.control_cycles) < 1
            or any(not math.isfinite(value) or value <= 0 for value in durations)
            or len(set(args.checkpoints)) != len(args.checkpoints)):
        parser.error('populations, counts and finite durations must be positive; checkpoints must be distinct')
    args.checkpoints.sort()
    minimum_timeout = max(args.cycles, args.control_cycles) * args.checkpoints[-1]
    if args.timeout <= minimum_timeout or args.control_wake_seconds >= args.checkpoints[-1]:
        parser.error('timeout must exceed cycle idle waits and controls must finish before the last checkpoint')
    import psutil

    revisions = {name: command(['git', 'rev-parse', '--verify', '--end-of-options', f'{revision}^{{commit}}']).strip()
                 for name, revision in [('before', args.baseline_revision), ('after', args.after_revision)]}
    report = {
        'schema': 1, 'benchmark': 'synthetic-helper-pool-lifecycle',
        'source_revision': command(['git', 'rev-parse', 'HEAD']).strip(),
        'source_dirty': bool(command(['git', 'status', '--porcelain']).strip()),
        'host': {'platform': platform.platform(), 'machine': platform.machine(),
                 'processor': platform.processor(), 'cpu_count': os.cpu_count(),
                 'python': sys.version, 'psutil': psutil.__version__},
        'rustc': command([os.environ.get('ZORE_RUSTC', 'rustc'), '-vV']),
        'configuration': {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()},
        'helper_capacity': 512, 'idle_policy_seconds': 5,
        'harness_sources': {path.name: digest(path) for path in sorted(HERE.glob('*')) if path.is_file()},
        'variants': {}, 'measurements': [],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')

    save()
    with tempfile.TemporaryDirectory(prefix='zore-helper-lifecycle-') as temporary:
        executables = {}
        for name, revision in revisions.items():
            directory = Path(temporary) / name
            directory.mkdir()
            variant = {'source_revision': revision, 'status': 'failed'}
            report['variants'][name] = variant
            try:
                variant['runtime_sources'] = runtime_sources(revision, directory / 'runtime')
                executable, identity = build(directory, directory / 'runtime')
                variant.update(identity, status='ok')
                executables[name] = executable
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                stderr = getattr(error, 'stderr', '')
                if isinstance(stderr, bytes):
                    stderr = stderr.decode('utf-8', errors='replace')
                variant.update(error=str(error), stderr=stderr)
            save()
        if len(executables) != 2:
            return 1
        cases = [(population, repetition, 'burst', args.cycles, 0)
                 for population in args.populations for repetition in range(1, args.repetitions + 1)]
        cases.append((args.populations[0], 1, 'empty-wakes', args.control_cycles, args.control_wake_seconds))
        for index, (population, repetition, workload, cycles, wakes) in enumerate(cases):
            order = ['before', 'after'] if index % 2 == 0 else ['after', 'before']
            for name in order:
                print(f'{name} {workload} n={population} repetition={repetition}', flush=True)
                measurement = measure(executables[name], population, cycles, args.checkpoints,
                                      args.sample_seconds, args.timeout, args.reuse_gap,
                                      wakes, args.wake_interval)
                report['measurements'].append({'variant': name, 'workload': workload,
                                               'repetition': repetition, **measurement})
                save()
    return int(any(measurement['status'] != 'ok' for measurement in report['measurements']))


if __name__ == '__main__':
    raise SystemExit(main())
