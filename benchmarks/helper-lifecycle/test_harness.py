import io
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import types
import unittest
from unittest import mock

from run import HERE, ROOT, build, measure, read_lines, runtime_sources, snapshot, validate


def counters(**changes):
    value = dict.fromkeys(['helper_threads', 'helper_active_jobs', 'idle_helpers', 'queued_helpers',
                          'helper_starts', 'helper_retirements', 'helper_empty_wakes', 'tasks',
                          'queued_tasks', 'blocked_workers', 'socket_handles', 'reactor_registrations',
                          'live_timers', 'timer_threads', 'readiness_threads', 'allocated_bytes'], 0)
    value.update(changes)
    return value


class HarnessTests(unittest.TestCase):
    def test_idle_workers_are_observations_and_active_jobs_are_separate(self):
        validate(counters(helper_threads=4, idle_helpers=4, helper_starts=4), True)
        validate(counters(helper_threads=4, helper_active_jobs=2, helper_starts=4), False)
        with self.assertRaisesRegex(RuntimeError, 'inconsistent helper counts'):
            validate(counters(helper_threads=2, idle_helpers=2, helper_active_jobs=1, helper_starts=2))
        with self.assertRaisesRegex(RuntimeError, 'inconsistent helper counts'):
            validate(counters(helper_threads=2, helper_active_jobs=-1, helper_starts=2))
        with self.assertRaisesRegex(RuntimeError, 'inconsistent helper lifecycle'):
            validate(counters(helper_threads=1, helper_starts=2))
        with self.assertRaisesRegex(RuntimeError, 'jobs have not finished'):
            validate(counters(queued_helpers=1), True)
        with self.assertRaisesRegex(RuntimeError, 'unexpected workload resource'):
            validate(counters(readiness_threads=1), True)

    def test_protocol_rejects_eof_malformed_output_and_missing_responses(self):
        for value, error in [(None, RuntimeError), ('not-json', ValueError)]:
            output = queue.Queue()
            output.put(value)
            with self.assertRaises(error):
                snapshot(output, 0.1)
        with self.assertRaises(TimeoutError):
            snapshot(queue.Queue(), 0.001)
        output = queue.Queue()
        output.put(json.dumps(counters()))
        self.assertEqual(snapshot(output, 0.1), counters())

    def test_archived_sources_are_flat_regular_rust_files_with_canonical_keys(self):
        data = io.BytesIO()
        with tarfile.open(fileobj=data, mode='w') as archive:
            for name in ['runtime/src/lib.rs', 'runtime/src/nested/ignored.rs', '../outside.rs']:
                member = tarfile.TarInfo(name)
                member.size = 4
                archive.addfile(member, io.BytesIO(b'text'))
            link = tarfile.TarInfo('runtime/src/linked.rs')
            link.type = tarfile.SYMTYPE
            link.linkname = '../outside.rs'
            archive.addfile(link)
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / 'runtime'
            with mock.patch('run.subprocess.run', return_value=types.SimpleNamespace(stdout=data.getvalue())):
                manifest = runtime_sources('revision', destination)
            self.assertEqual(set(manifest), {'runtime/src/lib.rs'})
            self.assertEqual((destination / 'lib.rs').read_bytes(), b'text')
            self.assertEqual([path.name for path in destination.iterdir()], ['lib.rs'])

    def test_invalid_configuration_fails_before_native_builds(self):
        for arguments in [['--cycles', '0'], ['--timeout', 'nan'], ['--populations', '0'],
                          ['--checkpoints', '1', '1'], ['--sample-seconds', 'inf'],
                          ['--control-cycles', '10'], ['--control-wake-seconds', '60']]:
            with tempfile.TemporaryDirectory() as temporary:
                report = Path(temporary) / 'unused.json'
                result = subprocess.run([sys.executable, str(HERE / 'run.py'),
                                         '--baseline-revision', 'unused', '--output', str(report),
                                         *arguments], capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertFalse(report.exists())

    def test_sampler_errors_preserve_prior_checkpoints_and_reap_the_child(self):
        class SamplerError(Exception):
            pass

        class BrokenPipeInput(io.StringIO):
            def close(self):
                if not self.closed:
                    super().close()
                    raise BrokenPipeError('cleanup pipe failed')

        for fail_at_creation, fail_at_close in [(False, False), (True, False), (False, True)]:
            child = mock.Mock()
            child.pid = 1
            child.returncode = None
            child.stdin = BrokenPipeInput() if fail_at_close else io.StringIO()
            child.stdout = io.StringIO(json.dumps(counters()) + '\n' + json.dumps(counters()) + '\n')
            child.stderr = io.StringIO()
            child.poll.side_effect = lambda: child.returncode
            child.kill.side_effect = lambda: setattr(child, 'returncode', -9)
            child.wait.return_value = -9
            sampler = types.SimpleNamespace(Error=SamplerError, Process=mock.Mock())
            if fail_at_creation:
                sampler.Process.side_effect = SamplerError('sampler failed at creation')
            samples = [{'rss_bytes': 1, 'threads': 1}, SamplerError('sampler failed during sample')]
            with mock.patch.dict(sys.modules, {'psutil': sampler}), \
                    mock.patch('run.subprocess.Popen', return_value=child), \
                    mock.patch('run.resource_sample', side_effect=samples):
                result = measure(Path('unused'), 2, 1, [1], 0, 5, 0.01)
            self.assertEqual(result['status'], 'failed')
            self.assertIn('sampler failed', result['failure'])
            self.assertEqual(len(result['checkpoints']), 0 if fail_at_creation else 1)
            child.kill.assert_called_once()
            child.wait.assert_called_once()
            self.assertTrue(child.stdin.closed and child.stdout.closed and child.stderr.closed)

    @unittest.skipUnless(shutil.which('rustfmt'), 'rustfmt is required')
    def test_rust_probe_is_formatted(self):
        subprocess.run(['rustfmt', '--check', '--edition', '2024', str(HERE / 'probe.rs')],
                       cwd=ROOT, capture_output=True, text=True, check=True, timeout=30)

    @unittest.skipUnless(shutil.which(os.environ.get('ZORE_RUSTC', 'rustc'))
                         and (os.name != 'nt' or shutil.which('link.exe')),
                         'a native Rust linker is required')
    def test_real_probe_runs_repeated_bursts_and_exits_cleanly(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable, _ = build(Path(temporary), ROOT / 'runtime/src')
            child = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True)
            output = queue.Queue()
            reader = threading.Thread(target=read_lines, args=(child.stdout, output), daemon=True)
            reader.start()
            try:
                validate(snapshot(output, 10), True)
                for _ in range(2):
                    child.stdin.write('start 2\n')
                    child.stdin.flush()
                    active = snapshot(output, 10)
                    validate(active)
                    self.assertEqual((active['helper_active_jobs'], active['queued_helpers']), (2, 0))
                    child.stdin.write('release\n')
                    child.stdin.flush()
                    validate(snapshot(output, 10), True)
                child.stdin.write('nudge\n')
                child.stdin.flush()
                validate(snapshot(output, 10), True)
                child.stdin.write('stop\n')
                child.stdin.flush()
                child.stdin.close()
                child.wait(timeout=10)
                self.assertEqual(child.returncode, 0)
                self.assertEqual(child.stderr.read(), '')
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)
                if not child.stdin.closed:
                    child.stdin.close()
                child.stdout.close()
                child.stderr.close()
                reader.join(timeout=5)


if __name__ == '__main__':
    unittest.main()
