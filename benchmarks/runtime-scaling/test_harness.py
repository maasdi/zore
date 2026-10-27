import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

from workload import source
from run import ROOT, frame_layouts, hook_ir, validate_idle_runtime


class HarnessTests(unittest.TestCase):
    def test_idle_windows_timer_service_is_bounded_and_mode_specific(self):
        counters = {
            'tasks': 0,
            'queued_tasks': 0,
            'blocked_workers': 0,
            'queued_helpers': 0,
            'socket_handles': 0,
            'reactor_registrations': 0,
            'live_timers': 0,
            'timer_threads': 1,
            'readiness_threads': 0,
        }
        validate_idle_runtime('timers', counters, True)
        validate_idle_runtime('timers', {**counters, 'timer_threads': 0}, False)
        with self.assertRaisesRegex(RuntimeError, 'unexpected idle timer threads'):
            validate_idle_runtime('tasks', counters, True)
        with self.assertRaisesRegex(RuntimeError, 'unexpected idle timer threads'):
            validate_idle_runtime('timers', {**counters, 'timer_threads': 2}, True)

    def test_idle_windows_readiness_service_is_bounded_and_mode_specific(self):
        counters = dict.fromkeys(['tasks', 'queued_tasks', 'blocked_workers', 'queued_helpers',
                                  'socket_handles', 'reactor_registrations', 'live_timers',
                                  'timer_threads', 'readiness_threads'], 0)
        for mode in ['sockets', 'socket-deadline']:
            validate_idle_runtime(mode, {**counters, 'readiness_threads': 1}, True)
            validate_idle_runtime(mode, counters, False)
            for count in [0, 2]:
                with self.assertRaisesRegex(RuntimeError, 'unexpected idle readiness threads'):
                    validate_idle_runtime(mode, {**counters, 'readiness_threads': count}, True)
        with self.assertRaisesRegex(RuntimeError, 'unexpected idle readiness threads'):
            validate_idle_runtime('tasks', {**counters, 'readiness_threads': 1}, True)

    def test_payload_is_read_after_suspension_in_every_variant(self):
        for mode in ['tasks', 'timers', 'sockets', 'socket-deadline']:
            text = source(mode, 4096)
            self.assertIn('payload.push(i)', text)
            self.assertLess(text.index('ready.send(true)'), text.index('payload.len() != 4096'))
            self.assertIn('payload[payload.len() - 1] != 4095', text)
            self.assertIn('quiesce(0)', text)
            self.assertIn('missing start', text)

    def test_socket_deadline_and_success_have_distinct_result_oracles(self):
        self.assertIn('err != error("net.Read: timed out") || n != 0', source('socket-deadline', 0))
        self.assertIn('SetReadDeadline(time.Now() + 30 * time.Second)', source('socket-deadline', 0))
        self.assertIn('SetReadDeadline(time.Now() + 17 * time.Second)', source('socket-deadline', 0, 17))
        self.assertIn('err != nil || n != 1 || data[0] != 120', source('sockets', 0))
        self.assertIn('3600 * time.Second', source('timers', 0))
        self.assertIn('cancel()', source('timers', 0))

    def test_instrumentation_rejects_missing_or_duplicate_probe_definitions(self):
        for linkage in ['', 'internal ', 'private ']:
            ir = '\n'.join(f'define {linkage}void @"main.{name}"(i64 %n) {{\n  ret void\n}}' for name in ['probe', 'quiesce'])
            self.assertIn('call void @zore_profile_probe(i64 %n)', hook_ir(ir))
        with self.assertRaises(RuntimeError):
            hook_ir('')
        with self.assertRaises(RuntimeError):
            hook_ir(ir + '\n' + ir)

    def test_generated_programs_pass_real_frontend(self):
        suffix = '.exe' if os.name == 'nt' else ''
        compiler = Path(os.environ.get('ZORE_SCALING_COMPILER', ROOT / f'target/release/zore{suffix}'))
        if not compiler.is_file():
            self.skipTest('set ZORE_SCALING_COMPILER or build release compiler')
        with tempfile.TemporaryDirectory() as temporary:
            for mode in ['tasks', 'timers', 'sockets', 'socket-deadline']:
                directory = Path(temporary) / mode
                directory.mkdir()
                path = directory / 'main.ore'
                path.write_text(source(mode, 16))
                result = subprocess.run([str(compiler), 'check', str(path)], capture_output=True,
                                        text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_invalid_configuration_is_rejected_before_sampling_or_building(self):
        for arguments in [['--timeout', 'nan'], ['--repetitions', '0'], ['--populations', '0'], ['--payloads', '-1'], ['--socket-deadline-seconds', '0']]:
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('run.py')),
                                     '--compiler', 'unused', '--compiler-revision', 'unused',
                                     '--output', 'unused', *arguments], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 2, result.stderr)

    def test_compiler_revision_is_optional(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('run.py')),
                                 '--compiler', 'unused', '--output', 'unused', '--repetitions', '0'],
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('must be positive', result.stderr)
        self.assertNotIn('required', result.stderr)

    def test_runtime_is_built_once_and_sources_use_posix_keys(self):
        import json
        import run
        builds = []
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'report.json'
            fake_compiler = Path(temporary) / 'zore'
            fake_compiler.write_bytes(b'compiler')

            def fake_build(directory, compiler, mode, payload, library, socket_deadline_seconds):
                builds.append((mode, payload, library, socket_deadline_seconds))
                return Path('profiled'), {}

            fake_psutil = types.SimpleNamespace(__version__='0', virtual_memory=lambda: types.SimpleNamespace(total=1))
            arguments = ['run.py', '--compiler', str(fake_compiler), '--output', str(output),
                         '--modes', 'tasks', 'timers', '--payloads', '0', '4096', '--populations', '1',
                         '--repetitions', '1']
            with mock.patch.dict(sys.modules, {'psutil': fake_psutil}), \
                    mock.patch.object(sys, 'argv', arguments), \
                    mock.patch.object(run, 'command', return_value='revision\n'), \
                    mock.patch.object(run, 'build_runtime', return_value=Path('lib.rlib')) as build_runtime, \
                    mock.patch.object(run, 'build', side_effect=fake_build), \
                    mock.patch.object(run, 'measure', return_value={'status': 'ok'}):
                self.assertEqual(run.main(), 0)
            report = json.loads(output.read_text())
        self.assertEqual(build_runtime.call_count, 1)
        self.assertEqual(len(builds), 4)
        self.assertEqual({library for _, _, library, _ in builds}, {Path('lib.rlib')})
        self.assertEqual({deadline for _, _, _, deadline in builds}, {30})
        self.assertTrue(all('\\' not in key for key in report['runtime_sources']))
        self.assertIn('runtime/src/lib.rs', report['runtime_sources'])
        self.assertEqual(report['compiler_revision'], report['source_revision'])

    def test_frame_layouts_reject_ir_without_async_frames(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(RuntimeError):
                frame_layouts('', Path(temporary))

    def test_clang_reports_padded_frame_size_and_constructor_identity(self):
        if shutil.which(os.environ.get('ZORE_CC', 'clang')) is None:
            self.skipTest('clang is required for frame layouts')
        ir = '''%"main.AsyncFrame.17" = type { [3 x i8], i64, ptr }
define private ptr @"main.wait$async$new"() {
  %frame = alloca %"main.AsyncFrame.17"
  ret ptr null
}
'''
        with tempfile.TemporaryDirectory() as temporary:
            frames = frame_layouts(ir, Path(temporary))
        self.assertEqual(frames[0]['bytes'], 24)
        self.assertEqual(frames[0]['constructors'], ['@"main.wait$async$new"'])

    def test_runner_does_not_load_the_performance_harness(self):
        performance = (ROOT / 'benchmarks/performance').resolve()
        self.assertNotIn('compile', sys.modules)
        self.assertNotIn('common', sys.modules)
        self.assertFalse([entry for entry in sys.path if entry and Path(entry).resolve() == performance])
