"""SB-11 unit tests: cell expansion, SLO search, pinning, TLS specs, builders."""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

REPO_ROOT = Path(__file__).resolve().parents[2]
STACK_DIR = REPO_ROOT / "bench" / "stack-matrix"


def load_module(name):
    spec = importlib.util.spec_from_file_location(name, STACK_DIR / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


cells = load_module("cells")
slo = load_module("slo")
pin = load_module("pin")
peertls = load_module("peertls")
run = load_module("run")


class CellExpansionTest(unittest.TestCase):
    def test_smoke_covers_every_required_peer(self):
        got = cells.expand(stage="smoke")
        servers = {c.server_peer for c in got if c.role == "server" and not c.tls}
        clients = {c.client_peer for c in got if c.role == "client"}
        self.assertEqual(servers, set(cells.REQUIRED_SERVER_PEERS))
        self.assertEqual(clients, set(cells.REQUIRED_CLIENT_PEERS))
        # Plus exactly one native TLS server cell.
        tls_cells = [c for c in got if c.tls]
        self.assertEqual(len(tls_cells), 1)
        self.assertEqual(tls_cells[0].server_peer, "native")

    def test_smoke_cell_ids_unique(self):
        got = cells.expand(stage="smoke")
        ids = [c.id for c in got]
        self.assertEqual(len(ids), len(set(ids)))

    def test_primary_has_full_axes(self):
        got = cells.expand(stage="primary")
        server_cells = [c for c in got if c.role == "server" and c.cpus == 1]
        shapes = {(c.shape, c.payload, c.tls) for c in server_cells}
        want = {
            (s, p, t)
            for s in cells.SHAPES
            for p in cells.PAYLOADS
            for t in (False, True)
        }
        self.assertTrue(want <= shapes)
        scaled = {(c.cpus) for c in got if c.role == "server"}
        self.assertEqual(scaled, {1, 2, 4})

    def test_unknown_stage_rejected(self):
        with self.assertRaises(ValueError):
            cells.expand(stage="prod")


class CellValidationTest(unittest.TestCase):
    def mk(self, **kw):
        base = dict(
            role="server",
            server_peer="native",
            client_peer="native",
            shape="unary",
            payload="empty",
            tls=False,
            cpus=1,
        )
        base.update(kw)
        return cells.Cell(**base)

    def test_tonic_prost_client_supported(self):
        self.assertIsNone(cells.validate(self.mk(role="client", client_peer="tonic-prost")))

    def test_tonic_tls_supported(self):
        self.assertIsNone(cells.validate(self.mk(tls=True, server_peer="tonic-pbrs")))
        self.assertIsNone(cells.validate(self.mk(tls=True, server_peer="tonic-prost")))
        self.assertIsNone(
            cells.validate(self.mk(role="client", tls=True, client_peer="tonic-pbrs"))
        )

    def test_soak_tls_rejected_but_plaintext_soak_ok(self):
        self.assertIsNotNone(
            cells.validate(self.mk(role="client", client_peer="go", tls=True))
        )
        self.assertIsNone(cells.validate(self.mk(role="client", client_peer="go")))
        self.assertIsNone(cells.validate(self.mk(role="client", client_peer="cpp")))

    def test_native_go_cpp_tls_ok(self):
        for peer in ("native", "go", "cpp"):
            self.assertIsNone(cells.validate(self.mk(tls=True, server_peer=peer)))

    def test_soak_workload_labeled(self):
        self.assertEqual(
            self.mk(role="client", client_peer="go").workload(), "interop-soak"
        )
        self.assertEqual(self.mk().workload(), "open-loop-load")


class SloSearchTest(unittest.TestCase):
    def probe_factory(self, ceiling_rate, p99_at):
        """Valid below ceiling_rate; p99 blows past above it."""

        def probe(rate):
            saturate = rate > ceiling_rate * 4
            return slo.StepResult(
                offered_rate=rate,
                offered_calls=1000,
                successful_calls=1000,
                success_qps=rate * 0.99,
                p99_s=p99_at(rate),
                gen_saturated=saturate,
            )

        return probe

    def test_finds_highest_valid_step(self):
        result = slo.find_sustained_qps(
            self.probe_factory(4000.0, lambda r: 0.002 if r <= 4000 else 0.020),
            start_rate=1000.0,
            max_rate=16000.0,
            growth=2.0,
            slo_p99_s=0.010,
            max_steps=8,
        )
        self.assertEqual(result.sustained_step, 2)  # 1000, 2000, 4000 valid
        self.assertAlmostEqual(result.sustained_qps, 4000.0 * 0.99)
        self.assertEqual(len(result.steps), 4)  # 8000 kept as ceiling witness
        self.assertIn("8000", result.ceiling_reason)
        self.assertIn("p99", result.ceiling_reason)

    def test_zero_when_first_step_invalid(self):
        result = slo.find_sustained_qps(
            self.probe_factory(100.0, lambda r: 0.020),
            start_rate=1000.0,
            max_rate=16000.0,
            slo_p99_s=0.010,
        )
        self.assertEqual(result.sustained_qps, 0.0)
        self.assertEqual(result.sustained_step, -1)
        self.assertTrue(result.ceiling_reason)

    def test_errors_invalidate_before_slo(self):
        def probe(rate):
            return slo.StepResult(
                offered_rate=rate,
                offered_calls=100,
                successful_calls=90,
                failed_calls=10,
                success_qps=90.0,
                p99_s=0.001,
            )

        result = slo.find_sustained_qps(probe, start_rate=100.0, max_rate=100.0)
        self.assertEqual(result.sustained_step, -1)
        self.assertIn("failed=10", result.ceiling_reason)

    def test_unaccounted_calls_invalidate(self):
        def probe(rate):
            return slo.StepResult(
                offered_rate=rate,
                offered_calls=100,
                successful_calls=50,
                success_qps=50.0,
                p99_s=0.001,
            )

        result = slo.find_sustained_qps(probe, start_rate=100.0, max_rate=100.0)
        self.assertIn("unaccounted calls", result.ceiling_reason)

    def test_rejections_and_saturation_invalidate(self):
        rej = slo.check_step(
            slo.StepResult(
                offered_rate=1.0,
                offered_calls=100,
                successful_calls=90,
                rejected_calls=10,
                p99_s=0.001,
            ),
            0.010,
        )
        self.assertFalse(rej.valid)
        self.assertIn("rejected", rej.invalid_reason)
        sat = slo.check_step(
            slo.StepResult(
                offered_rate=1.0,
                offered_calls=100,
                successful_calls=100,
                p99_s=0.001,
                gen_saturated=True,
            ),
            0.010,
        )
        self.assertFalse(sat.valid)
        self.assertIn("saturated", sat.invalid_reason)

    def test_bad_search_params_rejected(self):
        with self.assertRaises(ValueError):
            slo.find_sustained_qps(lambda r: slo.StepResult(r), start_rate=0, max_rate=1)


class PinTest(unittest.TestCase):
    def test_wrap_prefixes_taskset_on_linux(self):
        with (
            patch.object(pin.sys, "platform", "linux"),
            patch.object(pin.shutil, "which", return_value="/usr/bin/taskset"),
            patch.object(pin.os, "cpu_count", return_value=8),
        ):
            cmd, state = pin.wrap(["server", "--x"], 2, offset=4)
        self.assertTrue(state["supported"])
        self.assertEqual(cmd[:3], ["/usr/bin/taskset", "-c", "4-5"])
        self.assertEqual(cmd[3:], ["server", "--x"])

    def test_over_subscription_refuses_to_clamp(self):
        with (
            patch.object(pin.sys, "platform", "linux"),
            patch.object(pin.shutil, "which", return_value="/usr/bin/taskset"),
            patch.object(pin.os, "cpu_count", return_value=2),
        ):
            cmd, state = pin.wrap(["server"], 4)
        self.assertFalse(state["supported"])
        self.assertIn("exceeds", state["reason"])
        self.assertEqual(cmd, ["server"])

    def test_non_linux_records_unsupported(self):
        with patch.object(pin.sys, "platform", "darwin"):
            cmd, state = pin.wrap(["server"], 1)
        self.assertFalse(state["supported"])
        self.assertIn("darwin", state["reason"])
        self.assertEqual(cmd, ["server"])

    def test_zero_cpus_rejected(self):
        with self.assertRaises(ValueError):
            pin.describe(0)


class PeerTlsTest(unittest.TestCase):
    def test_native_spec_from_in_tree_testdata(self):
        spec = peertls.server_spec("native", REPO_ROOT)
        self.assertIsNotNone(spec)
        self.assertTrue(Path(spec.ca_file).is_file())
        self.assertEqual(spec.server_name, "localhost")
        self.assertTrue(any(a.startswith("--tls-cert=") for a in spec.server_args))

    def test_unknown_peer_unresolvable(self):
        self.assertIsNotNone(peertls.server_spec("tonic-pbrs", REPO_ROOT))
        self.assertIsNone(peertls.server_spec("nope", REPO_ROOT))

    def test_go_spec_when_module_cache_present(self):
        spec = peertls.server_spec("go", REPO_ROOT)
        if spec is None:
            self.skipTest("grpc-go module cache unavailable")
        self.assertEqual(spec.server_args, ["-use_tls"])
        self.assertEqual(spec.server_name, "foo.test.google.fr")

    def test_cpp_spec_when_testdata_present(self):
        spec = peertls.server_spec("cpp", REPO_ROOT)
        if spec is None:
            self.skipTest("grpc test_creds unavailable")
        self.assertEqual(spec.server_args, ["--use_tls=true"])
        self.assertEqual(spec.server_name, "foo.test.google.fr")


class CommandBuilderTest(unittest.TestCase):
    def test_load_command_open_loop_args(self):
        cell = cells.Cell("server", "cpp", "native", "server_stream", "1kib", False, 2)
        cmd = run.load_command(
            "/bin/rpc-bench", "native", "127.0.0.1:9", cell, 2500.0, 3.0, 7, Path("/tmp/m.json"), None
        )
        self.assertEqual(cmd[0:2], ["/bin/rpc-bench", "load"])
        for want in (
            "--transport=native",
            "--shape=server_stream",
            "--req-bytes=1024",
            "--resp-bytes=1024",
            "--distribution=poisson",
            "--rate=2500",
            "--seed=7",
            "--duration-secs=3",
            "--stream-msgs=2000",
        ):
            self.assertIn(want, cmd)
        self.assertFalse(any(a.startswith("--tls-") for a in cmd))

    def test_load_command_tls_and_tonic(self):
        spec = peertls.server_spec("native", REPO_ROOT)
        cell = cells.Cell("server", "native", "native", "unary", "empty", True, 1)
        cmd = run.load_command(
            "/bin/rpc-bench", "native", "127.0.0.1:9", cell, 100.0, 1.0, 1, Path("/tmp/m.json"), spec
        )
        self.assertIn(f"--tls-ca={spec.ca_file}", cmd)
        self.assertIn("--tls-server-name=localhost", cmd)
        tonic_cell = cells.Cell("client", "native", "tonic-pbrs", "unary", "empty", False, 1)
        tonic_cmd = run.load_command(
            "/bin/rpc-bench", "tonic-pbrs", "127.0.0.1:9", tonic_cell, 100.0, 1.0, 1, Path("/tmp/m.json"), None
        )
        self.assertIn("--transport=tonic", tonic_cmd)

    def test_to_step_reads_metrics(self):
        metrics = {
            "offered_rpcs": 200,
            "dispatched_rpcs": 200,
            "successful_rpcs": 200,
            "failed_rpcs": 0,
            "timeouts": 0,
            "queue_overflows": 0,
            "status_errors": {},
            "throughput_qps": 199.5,
            "duration_nanos": 1_000_000_000,
            "e2e_latency_nanos": {"p50_nanos": 1_000_000, "p99_nanos": 5_000_000,
                "histogram": {"total_count": 200, "buckets": [{"count": 200}]}},
            "service_latency_nanos": {"histogram": {"total_count": 200, "buckets": [{"count": 200}]}},
        }
        step = run.to_step(200.0, metrics, False)
        self.assertEqual(step.offered_calls, 200)
        self.assertAlmostEqual(step.success_qps, 199.5)
        self.assertAlmostEqual(step.p99_s, 0.005)
        checked = slo.check_step(step, 0.010)
        self.assertTrue(checked.valid)

    def test_load_accounting_rejects_missing_counts_and_lost_latency_samples(self):
        import copy
        metrics = {"offered_rpcs": 12, "dispatched_rpcs": 10, "successful_rpcs": 8,
                   "failed_rpcs": 2, "timeouts": 1, "queue_overflows": 2,
                   "duration_nanos": 1_000_000_000, "status_errors": {"UNFINISHED": 1},
                   "service_latency_nanos": {"histogram": {"total_count": 10, "buckets": [{"count": 10}]}},
                   "e2e_latency_nanos": {"histogram": {"total_count": 10, "buckets": [{"count": 10}]}}}
        oracle = run.validate_load_accounting(metrics)
        self.assertEqual((oracle["completed"], oracle["unfinished"]), (9, 1))
        for key, value in (("dispatched_rpcs", None), ("offered_rpcs", 13),
                           ("successful_rpcs", 9), ("timeouts", 3), ("queue_overflows", False)):
            damaged = copy.deepcopy(metrics)
            damaged[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                run.validate_load_accounting(damaged)
        damaged = copy.deepcopy(metrics)
        damaged["e2e_latency_nanos"]["histogram"]["buckets"][0]["count"] = 9
        with self.assertRaisesRegex(ValueError, "does not reconcile"):
            run.validate_load_accounting(damaged)

    def test_summarize_cpu_per_rpc(self):
        step = slo.StepResult(offered_rate=1.0, successful_calls=1000, success_qps=999.0,
                              p50_s=0.001, p99_s=0.002)
        resources = {
            "server": {"user_cpu_seconds": 1.5, "system_cpu_seconds": 0.5,
                       "peak_rss_mib": 12.0},
            "client": {"user_cpu_seconds": 0.8, "system_cpu_seconds": 0.2,
                       "peak_rss_mib": 9.0},
        }
        out = run.summarize(step, resources, 2)
        self.assertAlmostEqual(out["server_cpu_per_rpc"], 0.002)
        self.assertAlmostEqual(out["client_cpu_per_rpc"], 0.001)
        self.assertEqual(out["server_peak_rss_mib"], 12.0)

    def test_headroom_budget(self):
        capacity = {"verified": True, "effective_cpu_capacity": 1.0, "affinity_cpu_count": 1}
        window = {"verified": True, "start_monotonic_s": 1.0, "end_monotonic_s": 3.0,
                  "duration_seconds": 2.0}
        ok = run.check_headroom({"supported": True, "user_cpu_seconds": 1.0, "system_cpu_seconds": 0.0,
                                 "cpu_capacity": capacity, "cpu_delta_verified": True,
                                 "cpu_counter_window": window}, 2.0, 1, True)
        self.assertTrue(ok["ok"])  # 50% of one core
        bad = run.check_headroom({"supported": True, "user_cpu_seconds": 1.9, "system_cpu_seconds": 0.0,
                                  "cpu_capacity": capacity, "cpu_delta_verified": True,
                                  "cpu_counter_window": window}, 2.0, 1, True)
        self.assertFalse(bad["ok"])  # 95% of one core
        self.assertIn("95.0%", bad["reason"])

    def test_unpinned_or_missing_samples_do_not_prove_headroom(self):
        self.assertFalse(run.check_headroom({"supported": True}, 2.0, 4, False)["ok"])
        self.assertFalse(run.check_headroom({}, 2.0, 4, True)["ok"])

    def test_fractional_quota_caps_requested_four_cpu_headroom_budget(self):
        resources = {"supported": True, "user_cpu_seconds": 0.9, "system_cpu_seconds": 0.0, "cpu_delta_verified": True,
                     "cpu_counter_window": {"verified": True, "start_monotonic_s": 1.0,
                                            "end_monotonic_s": 3.0, "duration_seconds": 2.0},
                     "cpu_capacity": {"verified": True, "effective_cpu_capacity": 0.5,
                                      "affinity_cpu_count": 4}}
        out = run.check_headroom(resources, 2.0, 4, True)
        self.assertFalse(out["ok"])
        self.assertEqual(out["budget_cpu_pct"], 50.0)

    def test_server_headroom_uses_endpoint_counter_window_not_probe_timer(self):
        resources = {"supported": True, "user_cpu_seconds": 1.0, "system_cpu_seconds": 0.0,
                     "cpu_delta_verified": True,
                     "cpu_counter_window": {"verified": True, "start_monotonic_s": 1.0,
                                            "end_monotonic_s": 3.0, "duration_seconds": 2.0},
                     "cpu_capacity": {"verified": True, "effective_cpu_capacity": 1.0,
                                      "affinity_cpu_count": 1}}
        out = run.check_headroom(resources, 10.0, 1, True)
        self.assertEqual(out["server_avg_cpu_pct"], 50.0)
        self.assertEqual(out["cpu_counter_window"], resources["cpu_counter_window"])
        self.assertEqual(out["wall_s"], 10.0)  # diagnostic, not the CPU denominator

    def test_missing_or_invalid_counter_window_cannot_certify_server_headroom(self):
        resources = {"supported": True, "user_cpu_seconds": 0.0, "cpu_delta_verified": True,
                     "cpu_capacity": {"verified": True, "effective_cpu_capacity": 1.0,
                                      "affinity_cpu_count": 1}}
        for window in (None, {"verified": True, "duration_seconds": 0},
                       {"verified": True, "start_monotonic_s": 3.0,
                        "end_monotonic_s": 1.0, "duration_seconds": 2.0}):
            with self.subTest(window=window):
                resources["cpu_counter_window"] = window
                out = run.check_headroom(resources, 10.0, 1, True)
                self.assertFalse(out["ok"])
                self.assertIsNone(out["server_avg_cpu_pct"])

    def test_legacy_resource_records_without_capacity_cannot_prove_headroom(self):
        out = run.check_headroom({"supported": True, "user_cpu_seconds": 0.1}, 2.0, 4, True)
        self.assertFalse(out["ok"])
        self.assertIsNone(out["budget_cpu_pct"])
        self.assertIn("unverified", out["reason"])

    def test_actual_affinity_must_match_the_declared_pin(self):
        resources = {"supported": True, "user_cpu_seconds": 0.1,
                     "cpu_capacity": {"verified": True, "effective_cpu_capacity": 4.0,
                                      "affinity_cpu_count": 4}}
        self.assertFalse(run.check_headroom(resources, 2.0, 1, True)["ok"])

    def test_timeout_is_a_failed_call_not_an_extra_completion(self):
        step = slo.StepResult(offered_rate=1, offered_calls=10, successful_calls=8,
                              failed_calls=2, timed_out_calls=2, p99_s=0.001)
        self.assertIn("errors:", slo.check_step(step, 0.01).invalid_reason)


class FrozenScenarioTest(unittest.TestCase):
    path = STACK_DIR / "scenarios" / "grpc-bench-echo.json"

    def test_all_repeats_preserve_frozen_pair_order(self):
        definition = json.loads(self.path.read_text())
        loaded = run.frozen_mod.load(self.path, run.STAGE_PARAMS["primary"])
        self.assertEqual(len(loaded["plan"]), 5 * 19)
        for rep, expected in definition["frozen_order_per_repeat"].items():
            self.assertEqual([c.id for r, c in loaded["plan"] if r == rep], expected)
        self.assertEqual(loaded["params"], definition["params"])

    def test_filter_preserves_relative_order_in_every_repeat(self):
        chosen = ["server-native-unary-1kib-plain-1cpu", "server-tonic-pbrs-unary-1kib-plain-1cpu"]
        loaded = run.frozen_mod.load(self.path, run.STAGE_PARAMS["primary"], chosen)
        self.assertEqual(len(loaded["plan"]), 10)
        self.assertTrue(loaded["filtered"])
        self.assertEqual({tuple(order) for order in loaded["order"].values()},
                         {tuple(chosen), tuple(reversed(chosen))})

    def test_bad_order_and_params_fail_before_launch(self):
        definition = json.loads(self.path.read_text())
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            definition["frozen_order_per_repeat"]["rep2"].pop()
            path.write_text(json.dumps(definition))
            with self.assertRaisesRegex(ValueError, "every cell"):
                run.frozen_mod.load(path, run.STAGE_PARAMS["primary"])
        with self.assertRaisesRegex(ValueError, "unknown"):
            run.frozen_mod.load(self.path, run.STAGE_PARAMS["primary"], ["typo"])

    def test_claim_blocks_before_resolving_or_building_peers(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(run, "preflight") as resolve:
            code, report = run.run_stage("primary", Path(tmp), scenario_path=self.path, claim=True)
        self.assertEqual(code, 1)
        self.assertFalse(report["matrix_complete"])
        self.assertFalse(report["claim_eligible"])
        self.assertTrue(report["claim_blockers"])
        resolve.assert_not_called()

    def test_configured_values_cannot_pass_claim_fairness(self):
        cell = cells.Cell("server", "native", "native", "unary", "empty", False, 1)
        report = {"fairness_spec": run.fairness_mod.SB01_SPEC, "matrix_complete": True,
                  "qualification": "dedicated-network", "cells": [{**cells.as_dict(cell),
                    "status": "pass", "fairness": run.fairness_mod.for_cell("native", "native", False)}]}
        self.assertTrue(run.fairness_mod.verify_report(report)[0])
        ok, findings = run.fairness_mod.verify_report(report, claim=True)
        self.assertFalse(ok)
        self.assertTrue(any(f["check"] == "effective-setting" for f in findings))

    def test_claim_coverage_does_not_trust_complete_boolean(self):
        loaded = run.frozen_mod.load(self.path, run.STAGE_PARAMS["primary"])
        report = {"scenario": loaded, "matrix_complete": True,
                  "cells": [{"repeat": rep, "id": cell.id} for rep, cell in loaded["plan"]]}
        _, findings = run.fairness_mod.verify_report(report, claim=True)
        self.assertFalse(any(f["check"] == "coverage" for f in findings))
        report["cells"].pop()
        _, findings = run.fairness_mod.verify_report(report, claim=True)
        self.assertTrue(any(f["check"] == "coverage" for f in findings))


if __name__ == "__main__":
    unittest.main()
