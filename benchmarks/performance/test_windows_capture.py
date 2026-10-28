import copy
import ctypes
import os
from pathlib import Path
import subprocess
import sys
import unittest

from common import ROOT, run_process
from windows_capture import PARALLEL_PREPARATION, SERIAL_PREPARATION, WORKLOADS, comparison_evidence, serial_source


def paired_report():
    rows = []
    for workload in WORKLOADS:
        for repetition in range(1, 4):
            order = ['baseline', 'candidate'] if repetition % 2 else ['candidate', 'baseline']
            for variant in order:
                commands = [{'tool': 'clang', 'role': 'object', 'started': 1,
                             'seconds': 2, 'exit_code': 0},
                            {'tool': 'link', 'role': 'link', 'started': 5,
                             'seconds': 1, 'exit_code': 0}]
                profile = {'ir_sha256': workload, 'object_sha256': workload,
                           'ir_bytes': 10, 'commands': commands}
                cold = copy.deepcopy(profile)
                cold['commands'].append({'tool': 'rustc', 'role': 'runtime',
                                         'started': 3 if variant == 'baseline' else 2,
                                         'seconds': 2, 'exit_code': 0})
                rows.append({'variant': variant, 'workload': workload, 'repetition': repetition,
                             'warm_profile': profile, 'cold_profile': cold,
                             'stages': {'native_ir_bytes': 10}})
    return {'compile': rows}


class WindowsCaptureTests(unittest.TestCase):
    def test_timed_out_command_retains_output_and_terminates_its_child_tree(self):
        source = "import subprocess,sys,time; child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); print(child.pid,flush=True); print('capture diagnostic',file=sys.stderr,flush=True); time.sleep(60)"
        with self.assertRaises(subprocess.TimeoutExpired) as raised:
            run_process([sys.executable, '-c', source], timeout=2)
        self.assertIn('capture diagnostic', raised.exception.stderr)
        pid = int(raised.exception.stdout.strip())
        if os.name == 'nt':
            kernel = ctypes.WinDLL('kernel32', use_last_error=True)
            kernel.OpenProcess.restype = ctypes.c_void_p
            kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
            kernel.CloseHandle.argtypes = [ctypes.c_void_p]
            handle = kernel.OpenProcess(0x100000, False, pid)
            if handle:
                try:
                    self.assertEqual(kernel.WaitForSingleObject(handle, 5000), 0)
                finally:
                    kernel.CloseHandle(handle)
        elif sys.platform == 'linux':
            state = Path(f'/proc/{pid}/stat')
            try:
                status = state.read_text()
            except (FileNotFoundError, ProcessLookupError):
                return
            self.assertEqual(status.split(') ')[1].split()[0], 'Z')

    def test_serial_control_replaces_only_the_known_current_scheduling_block(self):
        source = (ROOT / 'compiler/src/driver/build.rs').read_text(encoding='utf-8')
        serial = serial_source(source)
        self.assertEqual(serial.replace(SERIAL_PREPARATION, PARALLEL_PREPARATION), source)
        for invalid in ['', source + source, source.replace('runtime build worker panicked', 'changed')]:
            with self.assertRaisesRegex(ValueError, 'exactly one'):
                serial_source(invalid)

    def test_complete_alternating_capture_retains_overlap_without_performance_thresholds(self):
        evidence = comparison_evidence(paired_report(), 3)
        self.assertEqual(set(evidence), set(WORKLOADS))
        for workload in evidence.values():
            self.assertEqual([item['object_runtime_overlap_seconds'] for item in workload['cold_intervals']],
                             [0, 1, 1, 0, 0, 1])

    def test_capture_rejects_incomplete_order_input_drift_and_missing_native_link(self):
        for fault in ['missing', 'order', 'ir', 'probe', 'link', 'tool_failure']:
            report = paired_report()
            row = report['compile'][0]
            if fault == 'missing':
                report['compile'].pop()
            elif fault == 'order':
                row['variant'] = 'candidate'
            elif fault == 'ir':
                row['warm_profile']['ir_sha256'] = 'unexpected'
            elif fault == 'probe':
                row['stages']['native_ir_bytes'] = 11
            elif fault == 'link':
                row['warm_profile']['commands'].pop()
            else:
                row['warm_profile']['commands'][0]['exit_code'] = 1
            with self.subTest(fault=fault), self.assertRaises(ValueError):
                comparison_evidence(report, 3)


if __name__ == '__main__':
    unittest.main()
