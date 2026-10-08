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


if __name__ == "__main__":
    unittest.main()

class CaptureGuards(unittest.TestCase):
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
