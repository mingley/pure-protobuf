"""QPS driver preparation and result validation without a live peer."""

import importlib.util
import copy
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
    def test_reference_rate_is_aggregate_and_legacy_per_slot_multiplier_is_visible(self):
        scenario = {"client_config": {"client_channels": 2, "outstanding_rpcs_per_channel": 32,
                    "load_params": {"poisson": {"offered_load": 5000}}}}
        profile = qps_proof.arrival_profile(scenario)
        self.assertEqual(profile["aggregate_offered_qps"], 5000)
        self.assertEqual(profile["slot_limit"], 64)
        self.assertEqual(profile["unmodified_go_uncorrected_aggregate_qps"], 320000)
        self.assertEqual(profile["unmodified_go_configured_per_slot_qps"], 78.125)
        camel = {"clientConfig": {"clientChannels": 2, "outstandingRpcsPerChannel": 32,
                 "loadParams": {"poisson": {"offeredLoad": 5000}}}}
        self.assertEqual(qps_proof.arrival_profile(camel), profile)

    def test_effective_scenario_must_include_actual_overrides_and_aggregate_slots(self):
        scenario = {"name": "smoke", "warmup_seconds": 1, "benchmark_seconds": 2,
                    "client_config": {"client_channels": 1, "outstanding_rpcs_per_channel": 64,
                    "rpc_type": "UNARY", "load_params": {"poisson": {"offered_load": 5000}}}}
        result = {"scenario": copy.deepcopy(scenario)}
        qps_proof.validate_effective_scenario(result, scenario)
        for key, value in (("benchmark_seconds", 60), ("warmup_seconds", 15)):
            damaged = copy.deepcopy(result)
            damaged["scenario"][key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "effective scenario"):
                qps_proof.validate_effective_scenario(damaged, scenario)
        for key, value in (("outstanding_rpcs_per_channel", 1), ("client_channels", 2)):
            damaged = copy.deepcopy(result)
            damaged["scenario"]["client_config"][key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "effective client config"):
                qps_proof.validate_effective_scenario(damaged, scenario)

    @staticmethod
    def accounting_window():
        return {
            "schema_version": 1, "window_kind": "completion_mark", "window_epoch": 1,
            "reset": False, "window_seconds": 1.0, "drain_seconds": 0.0,
            "offered": 100, "dispatched": 90, "completed": 89, "successful": 87,
            "failed": 2, "rejected": 10, "timed_out": 1,
            "incoming_in_flight": 2, "carried_in_completed": 2, "unfinished": 3,
            "service_latency_nanos": {"count": 89, "bucket": [89], "sum": 890.0, "min_seen": 10, "max_seen": 10},
            "scheduled_latency_nanos": {"count": 89, "bucket": [89], "sum": 1780.0, "min_seen": 20, "max_seen": 20},
        }

    def test_each_repeat_independently_randomizes_scenario_and_peer_order(self):
        scenarios = ["empty", "1k", "64k"]
        directions = ["native_pair", "native_client_to_ref_server", "ref_client_to_native_server"]
        plan = qps_proof.execution_plan(scenarios, directions, 5, 210021)
        self.assertEqual(plan, qps_proof.execution_plan(scenarios, directions, 5, 210021))
        self.assertNotEqual(plan["runs"], qps_proof.execution_plan(scenarios, directions, 5, 42)["runs"])
        peer_orders = []
        for repeat in range(1, 6):
            runs = [run for run in plan["runs"] if run["repeat"] == repeat]
            self.assertEqual(len(runs), len(scenarios) * len(directions))
            self.assertEqual({(run["scenario"], run["direction"]) for run in runs},
                             {(scenario, direction) for scenario in scenarios for direction in directions})
            peer_orders.append(tuple(run["direction"] for run in runs if run["scenario"] == "empty"))
        self.assertGreater(len(set(peer_orders)), 1)
        with self.assertRaises(ValueError):
            qps_proof.execution_plan(scenarios, directions, 0, 42)

    def test_independent_accounting_conserves_carry_in_and_every_outcome(self):
        valid = self.accounting_window()
        qps_proof.validate_accounting(valid)
        mutations = [
            ("offered", 99), ("completed", 88), ("successful", 88),
            ("timed_out", 3), ("unfinished", 2), ("incoming_in_flight", 0),
            ("carried_in_completed", 3), ("window_seconds", float("nan")),
            ("dispatched", True), ("rejected", -1),
        ]
        for field, value in mutations:
            with self.subTest(field=field), self.assertRaises(ValueError):
                qps_proof.validate_accounting({**valid, field: value})
        for key in ("service_latency_nanos", "scheduled_latency_nanos"):
            damaged = copy.deepcopy(valid)
            damaged[key]["bucket"] = [88]
            with self.assertRaisesRegex(ValueError, "histogram"):
                qps_proof.validate_accounting(damaged)
        damaged = copy.deepcopy(valid)
        damaged["scheduled_latency_nanos"]["sum"] = 1
        with self.assertRaisesRegex(ValueError, "shorter than service"):
            qps_proof.validate_accounting(damaged)

    def test_claim_preflight_fails_closed_without_accounting_or_open_load(self):
        scenario = {"benchmark_seconds": 1, "client_config": {"load_params": {"poisson": {"offered_load": 100}}}}
        diagnostic = qps_proof.validate_measurement({"qps": 89}, "cpp", scenario, [], False)
        self.assertFalse(diagnostic["accounting_verified"])
        with self.assertRaisesRegex(ValueError, "lacks independent"):
            qps_proof.validate_measurement({"qps": 89}, "cpp", scenario, [], True)
        with self.assertRaisesRegex(ValueError, "open-loop"):
            qps_proof.validate_measurement({"qps": 89}, "cpp", {"benchmark_seconds": 1}, [], True)
        measured = self.accounting_window()
        warmup = {**measured, "reset": True, "window_epoch": 0, "offered": 99, "dispatched": 89, "unfinished": 2}
        proof = qps_proof.validate_measurement({"qps": 89}, "cpp", scenario, [warmup, measured], True)
        self.assertTrue(proof["accounting_verified"])
        self.assertFalse(proof["claim_eligible"])
        self.assertEqual(proof["successful_qps"], 87)
        with self.assertRaisesRegex(ValueError, "does not reconcile"):
            qps_proof.validate_measurement({"qps": 100}, "cpp", scenario, [warmup, measured], True)

    def test_old_go_poisson_outlier_is_rejected_without_guessing_its_cause(self):
        source = ROOT / "docs/evidence/qps-sb10/run5-go-c2n"
        scenario = json.loads((source / "protobuf_unary_poisson_5000qps.scenario.json").read_text())["scenarios"][0]
        result = json.loads((source / "protobuf_unary_poisson_5000qps-ref_client_to_native_server-driver-metrics.json").read_text())
        with self.assertRaisesRegex(ValueError, "exceeds configured Poisson offered load"):
            qps_proof.validate_measurement(result, "cpp", scenario, [], False)

    def test_worker_log_rejects_nonjoining_reset_windows(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "client.log"
            first = self.accounting_window()
            first.update({"reset": True, "window_epoch": 0})
            second = self.accounting_window()
            log.write_text("QPS_ACCOUNTING " + json.dumps(first) + "\nQPS_ACCOUNTING " + json.dumps(second) + "\n")
            with self.assertRaisesRegex(ValueError, "do not join"):
                qps_proof.accounting_from_log(log)

    def test_saved_native_smokes_reconcile_and_go_smoke_fails_closed(self):
        evidence = json.loads((ROOT / "docs/evidence/sb21-accounting-smoke.json").read_text())
        for run in evidence["native_runs"]:
            proof = qps_proof.validate_measurement(run["result"], "go", evidence["scenario"], run["worker_windows"], True)
            self.assertTrue(proof["accounting_verified"])
            self.assertFalse(proof["claim_eligible"])
            measured = proof["measurement"]
            self.assertGreater(measured["incoming_in_flight"], 0)
            self.assertGreater(measured["unfinished"], 0)
            self.assertGreater(measured["scheduled_latency_nanos"]["sum"], measured["service_latency_nanos"]["sum"])
        with self.assertRaisesRegex(ValueError, "exceeds configured Poisson offered load"):
            qps_proof.validate_measurement(evidence["go_rejection"]["result"], "go", evidence["scenario"], [], True)

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
