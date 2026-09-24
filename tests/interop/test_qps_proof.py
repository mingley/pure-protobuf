"""QPS driver preparation and result validation without a live peer."""

import importlib.util
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "qps-proof.py"
ROOT = SCRIPT.parents[1]
RUNNER = ROOT / "scripts" / "grpc-qps-interop.sh"
spec = importlib.util.spec_from_file_location("qps_proof", SCRIPT)
qps_proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qps_proof)


class QpsProofTest(unittest.TestCase):
    def test_runner_dry_run_uses_shared_cargo_target_and_caps_jobs(self):
        with tempfile.TemporaryDirectory() as directory:
            command = [
                "bash", str(RUNNER), "--dry-run", "--mode=native_pair",
                "--scenario=protobuf_unary_ping_pong_empty", f"--log-dir={directory}",
            ]
            env = os.environ.copy()
            env.pop("CARGO_TARGET_DIR", None)
            env.pop("CARGO_BUILD_JOBS", None)
            default = subprocess.run(
                command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=10, check=True
            )
            self.assertIn(f"Native Worker:      {ROOT}/target/release/rpc-bench worker", default.stdout)
            self.assertIn(f"Cargo Target:       {ROOT}/target", default.stdout)
            self.assertIn("Cargo Build Jobs:   2", default.stdout)

            env["CARGO_TARGET_DIR"] = "target/integration-consumers"
            env["CARGO_BUILD_JOBS"] = "16"
            capped = subprocess.run(
                command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=10, check=True
            )
            self.assertIn(
                f"Native Worker:      {ROOT}/target/integration-consumers/release/rpc-bench worker",
                capped.stdout,
            )
            self.assertIn("Cargo Build Jobs:   2", capped.stdout)
            self.assertIn("Capping CARGO_BUILD_JOBS=16 to 2", capped.stderr)

            env["CARGO_BUILD_JOBS"] = "0"
            invalid = subprocess.run(
                command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=10
            )
            self.assertNotEqual(invalid.returncode, 0)
            self.assertIn("CARGO_BUILD_JOBS must be a positive integer", invalid.stderr)

    def test_runner_incremental_build_does_not_trust_an_existing_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            target = temporary / "cache"
            worker = target / "release" / "rpc-bench"
            worker.parent.mkdir(parents=True)
            worker.write_text("#!/bin/sh\nexit 0\n")
            worker.chmod(0o755)
            bin_dir = temporary / "bin"
            bin_dir.mkdir()
            capture = temporary / "cargo-args"
            cargo = bin_dir / "cargo"
            cargo.write_text(
                '#!/bin/sh\nprintf "%s\\n" "$CARGO_TARGET_DIR" "$CARGO_BUILD_JOBS" "$@" > "$CARGO_CAPTURE"\n'
            )
            cargo.chmod(0o755)
            env = os.environ.copy()
            env.update({
                "PATH": f"{bin_dir}:{env['PATH']}",
                "CARGO_TARGET_DIR": str(target),
                "CARGO_BUILD_JOBS": "8",
                "CARGO_CAPTURE": str(capture),
                "SKIP_BUILD": "0",
            })
            result = subprocess.run(
                [
                    "bash", str(RUNNER), "--mode=native_pair",
                    "--scenario=protobuf_unary_ping_pong_empty",
                    f"--log-dir={temporary / 'logs'}",
                    f"--driver={temporary / 'missing-qps-driver'}",
                ],
                cwd=ROOT, env=env, capture_output=True, text=True, timeout=10
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("QPS driver binary missing", result.stderr)
            self.assertEqual(capture.read_text().splitlines()[:2], [str(target), "2"])
            self.assertIn("--locked", capture.read_text())
            self.assertIn("--release", capture.read_text())

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
