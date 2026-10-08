"""Counter validity and N/2N workload matching for the diagnostic ledger."""
import importlib.util
import json
import tempfile
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("ledger", Path(__file__).resolve().parents[1] / "scripts/dominance-ledger.py")
LEDGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LEDGER)


class DifferentialTests(unittest.TestCase):
    def test_context_switch_totals_preserve_scope_and_reject_bad_sums(self):
        def row(voluntary, involuntary):
            return {"context_switch_totals": {"client": {
                "scope": "process_lifetime", "method": "linux_getrusage_self",
                "includes_exited_threads": True,
                "counts": {"voluntary": voluntary, "involuntary": involuntary},
                "total": voluntary + involuntary}}}
        first, second = row(30, 5), row(60, 7)
        self.assertEqual(LEDGER.differential(first, second, "client", "context_switches", 8), 4)
        second["context_switch_totals"]["client"]["total"] = 1
        with self.assertRaises(ValueError):
            LEDGER.differential(first, second, "client", "context_switches", 8)
        for change in [{"scope": "leader_thread"}, {"includes_exited_threads": False}]:
            second = row(60, 7)
            second["context_switch_totals"]["client"].update(change)
            with self.assertRaises(ValueError):
                LEDGER.differential(first, second, "client", "context_switches", 8)

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


class FailedCaptureTests(unittest.TestCase):
    def capture(self, path, count, failed=False):
        path.mkdir()
        cells = [{"pair": list(pair), "shape": "bidi", "payload_bytes": 1024,
                  "tls": False, "compression": "identity", "connections": 1,
                  "in_flight": 1, "repeat": 0}
                 for pair in [LEDGER.SIDES["client"][1], LEDGER.REFERENCE]]
        runs = []
        for index, cell in enumerate(cells):
            invalid = failed and index == 0
            row = {"cell": cell, "passed": not invalid,
                   "metrics": {"successful_rpcs": count - int(invalid)},
                   "instruction_totals": {"client": 1000 + count * (10 if index == 0 else 20)}}
            if invalid:
                row["error"] = "one RPC timed out"
            name = f"run-{index}.json"
            (path / name).write_text(json.dumps(row))
            runs.append({"path": name, "passed": not invalid})
        (path / "report.json").write_text(json.dumps({
            "schema": "pbrs.load-smoke.v2", "passed": not failed, "binary_unchanged": True,
            "source_unchanged": True, "source_verified": True, "dirty": False,
            "binary_sha256": "a" * 64, "head": "b" * 40, "rpc_count": count,
            "callgrind": "valgrind", "allocation_counts": True, "host": {},
            "cpu_affinity": [0], "cells": cells, "runs": runs}))

    def test_invalid_pair_cannot_turn_into_a_win_in_partial_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16, failed=True)
            with self.assertRaises(ValueError):
                LEDGER.compare(small, large)
            result = LEDGER.compare(small, large, allow_failed=True)
            self.assertFalse(result["qualified"])
            self.assertEqual(len(result["rows"]), len(LEDGER.REQUIRED_METRICS))
            for row in result["rows"]:
                self.assertEqual(row["disposition"], "failed_capture")
                self.assertIsNone(row["ratio"])
                self.assertEqual(len(row["failed_captures"]), 1)

    def test_partial_mode_still_rejects_source_drift(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8, failed=True)
            manifest = json.loads((path / "report.json").read_text())
            manifest["source_unchanged"] = False
            (path / "report.json").write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                LEDGER.load(path, allow_failed=True)


if __name__ == "__main__":
    unittest.main()
