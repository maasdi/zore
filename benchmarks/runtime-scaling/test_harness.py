import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from workload import source
from run import ROOT, hook_ir


class HarnessTests(unittest.TestCase):
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
        for arguments in [['--timeout', 'nan'], ['--repetitions', '0'], ['--populations', '0'], ['--payloads', '-1']]:
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('run.py')),
                                     '--compiler', 'unused', '--compiler-revision', 'unused',
                                     '--output', 'unused', *arguments], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 2, result.stderr)
