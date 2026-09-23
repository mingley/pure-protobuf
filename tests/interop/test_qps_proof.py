"""QPS driver preparation and result validation without a live peer."""

import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "qps-proof.py"
spec = importlib.util.spec_from_file_location("qps_proof", SCRIPT)
qps_proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qps_proof)


class QpsProofTest(unittest.TestCase):
    def test_driver_digest_is_stable_and_content_based(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "driver"
            binary.write_bytes(b"official driver artifact")
            self.assertEqual(qps_proof.fingerprint(binary), hashlib.sha256(binary.read_bytes()).hexdigest())

    def test_single_scenario_and_overrides_preserve_other_config(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scenarios.json"
            path.write_text(json.dumps({"scenarios": [
                {"name": "first", "benchmark_seconds": 30, "client_config": {"rpc_type": "UNARY"}},
                {"name": "second", "benchmark_seconds": 40},
            ]}))
            prepared = qps_proof.prepare_scenario(path, "first", 1, 2)
            self.assertEqual(prepared["scenarios"], [{
                "name": "first", "warmup_seconds": 1, "benchmark_seconds": 2,
                "client_config": {"rpc_type": "UNARY"},
            }])
            self.assertEqual(json.loads(path.read_text())["scenarios"][0]["benchmark_seconds"], 30)
            with self.assertRaisesRegex(ValueError, "exactly one"):
                qps_proof.prepare_scenario(path, "missing", 1, 2)

    def test_cpp_qps_only_output_does_not_claim_raw_histograms(self):
        qps_proof.validate_result({"qps": 14373.1}, "cpp")
        with self.assertRaisesRegex(ValueError, "QPS-only"):
            qps_proof.validate_result({"qps": 12, "summary": {"qps": 12}}, "cpp")
        for invalid in (0, float("nan"), "fast"):
            with self.subTest(invalid=invalid), self.assertRaisesRegex(ValueError, "zero QPS|non-finite or zero QPS"):
                qps_proof.validate_result({"qps": invalid}, "cpp")

    def test_go_raw_result_requires_complete_successful_histogram(self):
        valid = {
            "scenario": {"name": "unary"},
            "summary": {"qps": 10.0, "latency50": 1000, "latency99": 3000},
            "clientStats": [{}],
            "serverStats": [{}],
            "clientSuccess": [True],
            "serverSuccess": [True],
            "latencies": {"count": 2, "bucket": [1, 1]},
        }
        qps_proof.validate_result(valid, "go")
        for damaged in (
            {**valid, "clientSuccess": [False]},
            {**valid, "summary": {"qps": 10.0, "latency50": 1000}},
            {**valid, "latencies": {"count": 3, "bucket": [1, 1]}},
            {**valid, "serverStats": []},
        ):
            with self.subTest(damaged=damaged), self.assertRaises(ValueError):
                qps_proof.validate_result(damaged, "go")


if __name__ == "__main__":
    unittest.main()
