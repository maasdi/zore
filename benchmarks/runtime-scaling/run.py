#!/usr/bin/env python3
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

from workload import source

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'benchmarks/performance'))
from compile import frame_layouts


def hook_ir(source):
    for name in ['probe', 'quiesce']:
        pattern = rf'^define (?:internal |private )?void @"main\.{name}"\([^\n]*\) \{{.*?^\}}'
        replacement = (f'define internal void @"main.{name}"(i64 %n) {{\nentry:\n'
                       f'  call void @zore_profile_{name}(i64 %n)\n  ret void\n}}')
        source, count = re.subn(pattern, lambda _match: replacement, source, flags=re.M | re.S)
        if count != 1:
            raise RuntimeError(f'expected one {name} definition, found {count}')
        source += f'\ndeclare void @zore_profile_{name}(i64)\n'
    return source


def command(arguments, **options):
    return subprocess.run(list(map(str, arguments)), capture_output=True, text=True,
                          check=True, timeout=300, cwd=options.pop('cwd', ROOT), **options).stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_runtime(directory, runtime):
    library = directory / 'libzore_runtime.rlib'
    command([os.environ.get('ZORE_RUSTC', 'rustc'), '--edition=2024', '--crate-name=zore_runtime',
             '--crate-type=rlib', '--cfg=zore_test_hooks', '-Copt-level=2', '-Cpanic=abort',
             runtime / 'lib.rs', '-o', library])
    return library


def build(directory, compiler, mode, payload, library):
    directory.mkdir()
    workload = directory / 'workload.ore'
    workload.write_text(source(mode, payload))
    captured = directory / 'captured.ll'
    cc = os.environ.get('ZORE_CC', 'clang')
    wrapper = HERE / ('cc.cmd' if os.name == 'nt' else 'cc.py')
    environment = {**os.environ, 'ZORE_CC': str(wrapper), 'ZORE_SCALING_CC': cc,
                   'ZORE_SCALING_PYTHON': sys.executable,
                   'ZORE_SCALING_IR': str(captured), 'ZORE_CACHE_DIR': str(directory / 'cache')}
    command([compiler, 'build', workload], env=environment, cwd=directory)
    ir = captured.read_text()
    layouts = frame_layouts(ir, directory)
    instrumented = directory / 'instrumented.ll'
    instrumented.write_text(hook_ir(ir))
    obj = directory / 'workload.o'
    command([cc, '-Wno-override-module', '-O2', '-c', instrumented, '-o', obj])
    entry = directory / 'entry.rs'
    entry.write_text('''unsafe extern "C" { fn zore_entry(); }
#[unsafe(no_mangle)]
extern "C" fn zore_profile_probe(n: i64) {
    if n > 0 { zore_runtime::scheduler::test_support::wait_population(n.try_into().unwrap(), KIND); }
    println!("{}", zore_runtime::scheduler::test_support::snapshot());
}
#[unsafe(no_mangle)]
extern "C" fn zore_profile_quiesce(n: i64) {
    zore_runtime::scheduler::test_support::wait_idle(n.try_into().unwrap());
}
fn main() {
    zore_runtime::scheduler::test_support::install_pool(2);
    unsafe { zore_entry(); }
    zore_runtime::finish();
}
'''.replace('KIND', str(1 if mode == 'timers' else 2 if mode.startswith('socket') else 0)))
    executable = directory / ('profiled.exe' if os.name == 'nt' else 'profiled')
    command([os.environ.get('ZORE_RUSTC', 'rustc'), '--edition=2024', '-Copt-level=2', '-Cpanic=abort', '--extern',
             f'zore_runtime={library}', f'-Clink-arg={obj}', entry, '-o', executable])
    return executable, {'workload_sha256': digest(workload), 'captured_ir_sha256': digest(captured),
                        'executable_sha256': digest(executable), 'frames': layouts,
                        'requested_live_payload_bytes': payload * 8, 'scheduler_capacity': 2,
                        'helper_capacity': 512, 'socket_read_max_bytes': 1,
                        'socket_deadline_seconds': 5 if mode == 'socket-deadline' else 120}


def resource_sample(process):
    memory = process.memory_info()
    return {'rss_bytes': memory.rss, 'threads': process.num_threads(),
            'handles' if os.name == 'nt' else 'fds': process.num_handles() if os.name == 'nt' else process.num_fds()}


def reader(stream, output):
    try:
        for line in stream:
            output.put(line.rstrip('\r\n'))
    finally:
        output.put(None)


def measure(executable, mode, population, timeout, sample_seconds, idle_seconds, cycles=2):
    import psutil
    started = time.monotonic()
    child = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True, bufsize=1,
                             env={**os.environ, 'ZORE_CHECK_LEAKS': '1'})
    process = psutil.Process(child.pid)
    output = queue.Queue()
    stdout_reader = threading.Thread(target=reader, args=(child.stdout, output), daemon=True)
    stdout_reader.start()
    errors = []
    stderr_reader = threading.Thread(target=lambda: errors.append(child.stderr.read()), daemon=True)
    stderr_reader.start()
    sockets = []
    result = {'population': population, 'mode': mode, 'checkpoints': [], 'status': 'failed'}

    def send(value):
        value = value.zfill(8) if value.isdigit() else value.ljust(8, '_')
        child.stdin.write(value + '\n')
        child.stdin.flush()

    def line():
        remaining = timeout - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError('measurement deadline exceeded')
        try:
            value = output.get(timeout=remaining)
        except queue.Empty as error:
            raise TimeoutError('child protocol timed out') from error
        if value is None:
            raise RuntimeError(f'child exited during protocol: {child.poll()}')
        return value

    def checkpoint(phase, counters):
        snapshot_elapsed = time.monotonic() - started
        samples = []
        deadline = time.monotonic() + sample_seconds
        while True:
            samples.append({'elapsed_seconds': time.monotonic() - started, **resource_sample(process)})
            if time.monotonic() >= deadline:
                break
            time.sleep(0.05)
        result['checkpoints'].append({'phase': phase, 'cycle': cycle, 'runtime_snapshot_elapsed_seconds': snapshot_elapsed, 'runtime': json.loads(counters), 'raw_samples': samples})

    try:
        for cycle in range(1, cycles + 1):
            send(str(population))
            checkpoint('baseline', line())
            send('start')
            value = line()
            if mode.startswith('socket'):
                if not value.startswith('LISTEN '):
                    raise RuntimeError(f'expected listener: {value}')
                host, port = value[7:].split(':')
                for _ in range(population):
                    peer = socket.create_connection((host, int(port)), timeout=5)
                    sockets.append(peer)
                value = line()
            checkpoint('active', value)
            send('release')
            if mode == 'sockets':
                for peer in sockets:
                    peer.sendall(b'x')
            checkpoint('completed', line())
            for peer in sockets:
                peer.close()
            sockets.clear()
            time.sleep(idle_seconds)
            send('snapshot')
            checkpoint('after_idle_cleanup', line())
            counters = result['checkpoints'][-1]['runtime']
            if any(counters[key] for key in ['tasks', 'queued_tasks', 'blocked_workers', 'queued_helpers', 'socket_handles', 'reactor_registrations', 'live_timers', 'timer_threads']):
                raise RuntimeError(f'nonquiescent runtime: {counters}')
            if cycle < cycles:
                send('next')
        send('stop')
        child.stdin.close()
        child.wait(timeout=max(1, timeout - (time.monotonic() - started)))
        if child.returncode:
            raise RuntimeError(f'child exit {child.returncode}')
        result['status'] = 'ok'
    except (OSError, RuntimeError, TimeoutError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        result['failure'] = str(error)
        result['exit_code_at_failure'] = child.poll()
    finally:
        for peer in sockets:
            peer.close()
        result['terminated_by_harness'] = child.poll() is None
        if result['terminated_by_harness']:
            child.kill()
        child.wait(timeout=5)
        result['exit_code'] = child.returncode
        stdout_reader.join(timeout=5)
        stderr_reader.join(timeout=5)
        if not child.stdin.closed:
            child.stdin.close()
        child.stdout.close()
        child.stderr.close()
        result['stderr'] = ''.join(errors)
        if result['status'] == 'ok' and result['stderr']:
            result['status'] = 'failed'
            result['failure'] = 'unexpected child stderr'
        result['elapsed_seconds'] = time.monotonic() - started
    return result


def main():
    parser = argparse.ArgumentParser(description='Opt-in cross-host runtime population measurements')
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--compiler-revision', help='label recorded in the report (default: git rev-parse HEAD)')
    parser.add_argument('--populations', type=int, nargs='+', default=[32, 128, 512, 640])
    parser.add_argument('--payloads', type=int, nargs='+', default=[0, 4096])
    parser.add_argument('--modes', nargs='+', choices=['tasks', 'timers', 'sockets', 'socket-deadline'], default=['tasks', 'timers', 'sockets', 'socket-deadline'])
    parser.add_argument('--cycles', type=int, default=2)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--sample-seconds', type=float, default=0.3)
    parser.add_argument('--idle-seconds', type=float, default=6)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source_revision = command(['git', 'rev-parse', 'HEAD']).strip()
    args.compiler_revision = args.compiler_revision or source_revision
    if min(args.populations) < 1 or min(args.payloads) < 0 or args.repetitions < 1 or args.cycles < 1 or any(not math.isfinite(x) or x <= 0 for x in [args.timeout, args.sample_seconds, args.idle_seconds]):
        parser.error('populations/repetitions and finite durations must be positive; payloads nonnegative')
    import psutil
    limits = {'production_helper_threads': 512, 'total_memory_bytes': psutil.virtual_memory().total}
    if os.name != 'nt':
        import resource
        limits['open_files_soft_hard'] = resource.getrlimit(resource.RLIMIT_NOFILE)
        limits['processes_soft_hard'] = resource.getrlimit(resource.RLIMIT_NPROC)
    report = {'schema': 1, 'host_limits': limits, 'source_revision': source_revision,
              'source_dirty': bool(command(['git', 'status', '--porcelain']).strip()),
              'compiler_revision': args.compiler_revision, 'compiler_sha256': digest(args.compiler),
              'host': {'platform': platform.platform(), 'machine': platform.machine(), 'processor': platform.processor(),
                       'cpu_count': os.cpu_count(), 'python': sys.version, 'psutil': psutil.__version__},
              'rustc': command([os.environ.get('ZORE_RUSTC', 'rustc'), '-vV']),
              'clang': command([os.environ.get('ZORE_CC', 'clang'), '--version']),
              'runtime_sources': {str(path.relative_to(ROOT)): digest(path) for path in sorted((ROOT / 'runtime/src').glob('*.rs'))},
              'harness_sources': {path.name: digest(path) for path in sorted(HERE.glob('*')) if path.is_file()},
              'configuration': {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()},
              'builds': [], 'measurements': []}
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.output.write_text(json.dumps(report, indent=2) + '\n')

    save()
    with tempfile.TemporaryDirectory(prefix='zore-scaling-') as temporary:
        runtime = Path(temporary) / 'runtime'
        shutil.copytree(ROOT / 'runtime/src', runtime)
        report['runtime_sources'] = {(Path('runtime/src') / path.name).as_posix(): digest(path) for path in sorted(runtime.glob('*.rs'))}
        save()
        try:
            library = build_runtime(Path(temporary), runtime)
        except (OSError, subprocess.SubprocessError) as error:
            for mode in args.modes:
                for payload in args.payloads:
                    report['builds'].append({'mode': mode, 'payload': payload, 'status': 'failed', 'error': str(error), 'stderr': getattr(error, 'stderr', '')})
            save()
            return 1
        for mode in args.modes:
            for payload in args.payloads:
                try:
                    executable, identity = build(Path(temporary) / f'{mode}-{payload}', args.compiler.resolve(), mode, payload, library)
                except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                    report['builds'].append({'mode': mode, 'payload': payload, 'status': 'failed', 'error': str(error), 'stderr': getattr(error, 'stderr', '')})
                    save()
                    continue
                report['builds'].append({'mode': mode, 'payload': payload, 'status': 'ok', **identity})
                for repetition in range(1, args.repetitions + 1):
                    for population in args.populations:
                        print(f'{mode} payload={payload} n={population} repetition={repetition}', flush=True)
                        measurement = measure(executable, mode, population, args.timeout, args.sample_seconds, args.idle_seconds, args.cycles)
                        report['measurements'].append({'payload': payload, 'repetition': repetition, **measurement})
                        save()
    return int(any(item['status'] != 'ok' for item in report['builds'] + report['measurements']))


if __name__ == '__main__':
    raise SystemExit(main())
