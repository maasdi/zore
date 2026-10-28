import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from common import ROOT, run_process


PARALLEL_PREPARATION = '''    let (result, runtime) = std::thread::scope(|scope| {
        let runtime = scope.spawn(|| runtime_library(&rustc, dir.path(), native_adapters));
        let result = compilation.wait_with_output();
        (
            result,
            runtime.join().expect("runtime build worker panicked"),
        )
    });'''
SERIAL_PREPARATION = '''    let result = compilation.wait_with_output();
    let runtime = runtime_library(&rustc, dir.path(), native_adapters);'''
WORKLOADS = ['hello', 'json', 'rest', 'rest_sample', 'dependency', 'suspension']


def serial_source(source):
    if source.count(PARALLEL_PREPARATION) != 1:
        raise ValueError('the scheduling control must replace exactly one known preparation block')
    return source.replace(PARALLEL_PREPARATION, SERIAL_PREPARATION)


def source_manifest(directory):
    listed = subprocess.run(['git', 'ls-files', '-z'], cwd=directory, capture_output=True,
                            text=True, check=True, timeout=30).stdout
    return {name: hashlib.sha256((directory / name).read_bytes()).hexdigest()
            for name in listed.split('\0') if name}


def comparison_evidence(report, repetitions):
    rows = report['compile']
    if len(rows) != len(WORKLOADS) * repetitions * 2:
        raise ValueError('paired capture has an incomplete measurement matrix')
    evidence = {}
    for workload in WORKLOADS:
        selected = [row for row in rows if row['workload'] == workload]
        expected = [(variant, repetition) for repetition in range(1, repetitions + 1)
                    for variant in (['baseline', 'candidate'] if repetition % 2 else ['candidate', 'baseline'])]
        if [(row['variant'], row['repetition']) for row in selected] != expected:
            raise ValueError('paired capture did not preserve alternating variant order')
        hashes = set()
        object_hashes = set()
        intervals = []
        for row in selected:
            for kind in ['warm_profile', 'cold_profile']:
                profile = row[kind]
                hashes.add(profile['ir_sha256'])
                object_hashes.add(profile['object_sha256'])
                if profile['ir_bytes'] != row['stages']['native_ir_bytes']:
                    raise ValueError('stage probe and captured native IR sizes differ')
                objects = [item for item in profile['commands']
                           if item['tool'] == 'clang' and item['role'] == 'object']
                links = [item for item in profile['commands']
                         if item['tool'] == 'link' and item['role'] == 'link']
                if len(objects) != 1 or len(links) != 1:
                    raise ValueError('capture must record one object compilation and native link')
                if any(item['exit_code'] for item in profile['commands']):
                    raise ValueError('capture contains a failed native tool command')
                if kind == 'cold_profile':
                    runtimes = [item for item in profile['commands']
                                if item['tool'] == 'rustc' and item['role'] == 'runtime']
                    if len(runtimes) != 1:
                        raise ValueError('cold capture must record one runtime compilation')
                    obj, runtime = objects[0], runtimes[0]
                    overlap = max(0, min(obj['started'] + obj['seconds'],
                                         runtime['started'] + runtime['seconds'])
                                  - max(obj['started'], runtime['started']))
                    intervals.append({'variant': row['variant'], 'repetition': row['repetition'],
                                      'object_runtime_overlap_seconds': overlap})
        if len(hashes) != 1:
            raise ValueError('serial and overlapping controls received different LLVM inputs')
        evidence[workload] = {'native_ir_sha256': next(iter(hashes)),
                              'object_sha256_values': sorted(object_hashes),
                              'cold_intervals': intervals}
    return evidence


def main():
    parser = argparse.ArgumentParser(description='Capture isolated Windows native build scheduling variants')
    parser.add_argument('--source-revision', required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--host-description', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=3)
    args = parser.parse_args()
    if os.name != 'nt' or args.repetitions < 1:
        parser.error('a Windows x64 MSVC host and positive repetitions are required')
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    report = {'status': 'failed', 'commands': [], 'variants': {},
              'host': json.loads(args.host_description.read_text(encoding='utf-8-sig'))}
    output = directory / 'capture.json'

    def save():
        output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')

    def command(arguments, cwd=ROOT, environment=None, timeout=900, record=False):
        invocation = list(map(str, arguments))
        result = run_process(invocation, cwd, environment, timeout)
        if record or result.returncode:
            report['commands'].append({'command': invocation, 'cwd': str(cwd),
                                       'exit_code': result.returncode,
                                       'stdout': result.stdout, 'stderr': result.stderr})
            save()
        result.check_returncode()
        return result.stdout.strip()

    save()
    try:
        revision = command(['git', 'rev-parse', '--verify', '--end-of-options',
                            f'{args.source_revision}^{{commit}}'])
        report['source_revision'] = revision
        report['harness_revision'] = command(['git', 'rev-parse', 'HEAD'])
        report['harness_dirty'] = bool(command(['git', 'status', '--porcelain']))
        linker = Path(os.environ['VCToolsInstallDir']) / 'bin/Hostx64/x64/link.exe'
        if not linker.is_file():
            raise ValueError(f'native MSVC linker is absent: {linker}')
        rustc = command(['rustup', 'which', 'rustc'])
        clang = shutil.which(os.environ['ZORE_CC'])
        if not clang:
            raise ValueError('the selected clang executable is absent')
        environment = {**os.environ, 'ZORE_BENCH_LINK': str(linker),
                       'ZORE_RUSTC': rustc, 'ZORE_CC': clang,
                       'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER': str(linker)}
        environment.pop('CARGO_TARGET_DIR', None)
        environment.pop('RUSTFLAGS', None)
        report['toolchain'] = {'rustc': command([rustc, '-vV']),
                               'rustc_path': rustc,
                               'rustc_sha256': hashlib.sha256(Path(rustc).read_bytes()).hexdigest(),
                               'clang': command([clang, '--version']),
                               'clang_path': clang,
                               'clang_sha256': hashlib.sha256(Path(clang).read_bytes()).hexdigest(),
                               'linker_path': str(linker),
                               'linker_sha256': hashlib.sha256(linker.read_bytes()).hexdigest()}
        if 'host: x86_64-pc-windows-msvc' not in report['toolchain']['rustc']:
            raise ValueError('capture requires the x64 MSVC Rust host')
        for variant in ['candidate', 'baseline']:
            checkout = directory / f'{variant}-source'
            command(['git', 'worktree', 'add', '--detach', checkout, revision])
            if variant == 'baseline':
                source = checkout / 'compiler/src/driver/build.rs'
                source.write_text(serial_source(source.read_text(encoding='utf-8')),
                                  encoding='utf-8', newline='\n')
                changed = command(['git', 'diff', '--name-only'], cwd=checkout)
                if changed != 'compiler/src/driver/build.rs':
                    raise ValueError('serial control changed unexpected tracked files')
                patch = command(['git', 'diff', '--no-ext-diff', '--binary'], cwd=checkout)
                (directory / 'serial-control.patch').write_text(patch + '\n', encoding='utf-8', newline='\n')
                command(['git', 'add', 'compiler/src/driver/build.rs'], cwd=checkout)
                command(['git', '-c', 'user.name=Zore benchmark', '-c', 'user.email=benchmark@localhost',
                         'commit', '-m', 'Isolate serial runtime preparation for measurement'], cwd=checkout)
            variant_revision = command(['git', 'rev-parse', 'HEAD'], cwd=checkout)
            report['variants'][variant] = {'revision': variant_revision,
                                           'source_manifest': source_manifest(checkout)}
            print(f'Building {variant} {variant_revision}', flush=True)
            command(['cargo', 'build', '--release', '--locked', '--bin', 'zore',
                     '--example', 'compile_profile'], cwd=checkout, environment=environment,
                    timeout=1200, record=True)
            if command(['git', 'status', '--porcelain'], cwd=checkout):
                raise ValueError('compiler source tree changed during its build')
        manifests = {key: value['source_manifest'] for key, value in report['variants'].items()}
        changed = [name for name in manifests['candidate']
                   if manifests['candidate'][name] != manifests['baseline'].get(name)]
        if changed != ['compiler/src/driver/build.rs'] or manifests['candidate'].keys() != manifests['baseline'].keys():
            raise ValueError('paired source manifests differ beyond the scheduling control')
        initial = directory / 'initial.json'
        paired = directory / 'paired.json'
        shared = [sys.executable, ROOT / 'benchmarks/performance/run.py', '--suites', 'compile',
                  '--compiler', directory / 'candidate-source/target/release/zore.exe',
                  '--compiler-revision', revision,
                  '--stage-profiler', directory / 'candidate-source/target/release/examples/compile_profile.exe',
                  '--compile-workloads', *WORKLOADS,
                  '--compile-lock-file', directory / 'capture.lock']
        print('Capturing current-main initial profile', flush=True)
        command([*shared, '--repetitions', '1', '--work-directory', directory / 'initial-work',
                 '--output', initial], environment=environment, timeout=1800, record=True)
        print('Capturing alternating serial and overlapping variants', flush=True)
        command([*shared, '--repetitions', args.repetitions,
                 '--baseline-compiler', directory / 'baseline-source/target/release/zore.exe',
                 '--baseline-revision', report['variants']['baseline']['revision'],
                 '--baseline-stage-profiler', directory / 'baseline-source/target/release/examples/compile_profile.exe',
                 '--work-directory', directory / 'paired-work', '--output', paired],
                environment=environment, timeout=3600, record=True)
        initial_report = json.loads(initial.read_text(encoding='utf-8'))
        if len(initial_report['compile']) != len(WORKLOADS):
            raise ValueError('initial current-main profile is incomplete')
        report['comparison_evidence'] = comparison_evidence(
            json.loads(paired.read_text(encoding='utf-8')), args.repetitions)
        report['status'] = 'ok'
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report['failure'] = str(error)
        if isinstance(error, subprocess.TimeoutExpired):
            report['timeout_command'] = list(map(str, error.cmd))
            report['timeout_stdout'] = error.stdout
            report['timeout_stderr'] = error.stderr
        print(f'Windows build capture failed: {error}', file=sys.stderr)
    finally:
        save()
    return int(report['status'] != 'ok')


if __name__ == '__main__':
    raise SystemExit(main())
