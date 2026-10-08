"""Counter validity and N/2N workload matching for the diagnostic ledger."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("ledger", Path(__file__).resolve().parents[1] / "scripts/dominance-ledger.py")
LEDGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LEDGER)


class DifferentialTests(unittest.TestCase):
    def test_positive_endpoint_differential(self):
        before = {"allocation_totals": {"server": {"allocations": 100}}}
        after = {"allocation_totals": {"server": {"allocations": 180}}}
        self.assertEqual(LEDGER.differential(before, after, "server", "allocations", 8), 10)

    def test_missing_and_nonpositive_differences_stay_unmeasured(self):
        self.assertIsNone(LEDGER.differential({}, {}, "client", "instructions", 8))
        before = {"instruction_totals": {"client": 100}}
        for total in [99, 100]:
            self.assertIsNone(LEDGER.differential(before, {"instruction_totals": {"client": total}}, "client", "instructions", 8))

    def test_invalid_counters_fail(self):
        for invalid in [False, "100", -1, 100.0]:
            with self.assertRaises(ValueError):
                LEDGER.differential({"instruction_totals": {"client": invalid}},
                                    {"instruction_totals": {"client": 200}}, "client", "instructions", 8)

    def test_workload_identity_distinguishes_repeat_and_concurrency(self):
        cell = {"pair": ["native", "prost", "tonic", "prost"], "shape": "bidi_pipelined", "payload_bytes": 1024,
                "tls": True, "compression": "gzip", "connections": 1, "in_flight": 16, "repeat": 0}
        for update in [{"repeat": 1}, {"connections": 64}, {"shape": "bidi"}, {"tls": False}]:
            self.assertNotEqual(LEDGER.key(cell), LEDGER.key({**cell, **update}))


if __name__ == "__main__":
    unittest.main()
