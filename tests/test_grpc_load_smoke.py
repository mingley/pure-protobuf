"""Reject incomplete or unsuccessful endpoint measurements."""

import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location(
    "grpc_load_smoke", Path(__file__).resolve().parents[1] / "scripts/grpc-load-smoke.py")
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class AccountingTests(unittest.TestCase):
    def setUp(self):
        self.metrics = {"offered_rpcs": 7, "dispatched_rpcs": 7, "completed_rpcs": 7,
                        "successful_rpcs": 7, "failed_rpcs": 0, "timeouts": 0,
                        "queue_overflows": 0, "unstarted_rpcs": 0, "unfinished_rpcs": 0,
                        "status_errors": {}}

    def test_complete_success(self):
        SMOKE.validate_metrics(self.metrics)

    def test_legacy_report_without_terminal_counts_is_rejected(self):
        for name in ["completed_rpcs", "unstarted_rpcs", "unfinished_rpcs"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                SMOKE.validate_metrics({key: value for key, value in self.metrics.items() if key != name})

    def test_empty_or_partial_success_is_rejected(self):
        for count in [0, 6]:
            with self.subTest(count=count), self.assertRaises(ValueError):
                SMOKE.validate_metrics({**self.metrics, "successful_rpcs": count})

    def test_every_failure_category_is_rejected(self):
        for name in ["failed_rpcs", "timeouts", "queue_overflows", "unstarted_rpcs", "unfinished_rpcs"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                SMOKE.validate_metrics({**self.metrics, name: 1})

    def test_invalid_types_or_negative_counts_are_rejected(self):
        for count in [None, False, -1, "7", 7.0]:
            with self.subTest(count=count), self.assertRaises(ValueError):
                SMOKE.validate_metrics({**self.metrics, "completed_rpcs": count})

    def test_error_breakdown_cannot_be_hidden_by_success_counts(self):
        with self.assertRaises(ValueError):
            SMOKE.validate_metrics({**self.metrics, "status_errors": {"UNAVAILABLE": 1}})


class CaptureGuards(unittest.TestCase):
    def test_strace_counts_include_error_calls_and_require_balanced_totals(self):
        import tempfile
        valid = "% time seconds usecs/call calls errors syscall\n------ ----------- ----------- --------- --------- ----------------\n60.00 0.003000 10 30 2 futex\n40.00 0.002000 10 20 read\n100.00 0.005000 10 50 2 total\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "strace"
            path.write_text(valid)
            result = SMOKE.strace_syscalls(path)
            self.assertEqual(result["calls"], 50)
            self.assertEqual(result["errors"], 2)
            self.assertTrue(result["includes_exited_threads"])
            for bad in ["", valid.replace("50 2 total", "51 2 total"),
                        valid.replace("50 2 total", "50 3 total"),
                        valid + "100.00 0.005000 10 50 2 total\n",
                        valid + "60.00 0.003000 10 30 2 futex\n",
                        valid.replace("30 2 futex", "-30 2 futex"),
                        valid.replace("30 2 futex", "30 31 futex"),
                        valid.replace("60.00", "nan"),
                        valid.replace("0.003000", "inf"),
                        valid.replace("50 2 total", "0 0 total")]:
                with self.subTest(summary=bad), self.assertRaises(ValueError):
                    path.write_text(bad)
                    SMOKE.strace_syscalls(path)

    def test_context_switch_snapshot_requires_all_thread_process_totals(self):
        import json
        import tempfile
        valid = {"scope": "process_lifetime", "method": "linux_getrusage_self",
                 "includes_exited_threads": True, "counts": {"voluntary": 20, "involuntary": 3}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stdout"
            def write(record):
                path.write_text("CONTEXT_SWITCHES " + json.dumps(record) + "\n")
            write(valid)
            self.assertEqual(SMOKE.context_switch_record(path)["total"], 23)
            for change in [{"scope": "leader_thread"}, {"scope": "process_since_exec"}, {"method": "proc_status"},
                           {"includes_exited_threads": False}, {"counts": None},
                           {"counts": {"voluntary": -1, "involuntary": 3}},
                           {"counts": {"voluntary": True, "involuntary": 3}},
                           {"counts": {"voluntary": 2 ** 64, "involuntary": 3}}]:
                with self.subTest(change=change), self.assertRaises(ValueError):
                    write({**valid, **change})
                    SMOKE.context_switch_record(path)
            write(valid)
            path.write_text(path.read_text() * 2)
            with self.assertRaises(ValueError):
                SMOKE.context_switch_record(path)
            for invalid in [None, [], 0, "invalid"]:
                with self.subTest(record=invalid), self.assertRaises(ValueError):
                    write(invalid)
                    SMOKE.context_switch_record(path)
            path.write_text("")
            with self.assertRaises(ValueError):
                SMOKE.context_switch_record(path)

    def test_allocator_snapshot_is_mandatory_and_unambiguous(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stdout"
            path.write_text("READY port=1\n")
            with self.assertRaises(ValueError):
                SMOKE.allocation_record(path)
            valid = 'ALLOCATIONS {"scope":"process_since_main","includes_reallocations":true,"allocations":9,"requested_bytes":100}\n'
            path.write_text(valid)
            self.assertEqual(SMOKE.allocation_record(path)["allocations"], 9)
            path.write_text(valid + valid)
            with self.assertRaises(ValueError):
                SMOKE.allocation_record(path)

    def test_callgrind_requires_a_single_positive_instruction_summary(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "callgrind"
            path.write_text("events: Dr Ir\nsummary: 99 120\n")
            self.assertEqual(SMOKE.callgrind_instructions(path), 120)
            for bad in ["", "events: Dr\nsummary: 9\n", "events: Ir\nsummary: 0\n",
                        "events: Ir\nsummary: 4\nsummary: 5\n"]:
                path.write_text(bad)
                with self.assertRaises(ValueError):
                    SMOKE.callgrind_instructions(path)


if __name__ == "__main__":
    unittest.main()
