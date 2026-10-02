"""Regression tests for mixed-peer benchmark accounting."""

import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "rpc-bench-matrix.py"
spec = importlib.util.spec_from_file_location("rpc_bench_matrix", SCRIPT)
bench_matrix = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = bench_matrix
spec.loader.exec_module(bench_matrix)


def soak_output(successes=3, total=3, failures=0, samples=(0, 1, 2)):
    return (
        f"soak test successes: {successes} / {total} iterations. Total failures: {failures}\n"
        f"Latencies in milliseconds: Count: {total} Min: 0 Max: 2 Avg: 1\n"
        + "".join(
            f"soak iteration: {index} elapsed_ms: {latency}\n"
            for index, latency in enumerate(samples)
        )
    )


def cpp_soak_output(total=3, failures=0, samples=(0, 1, 2)):
    return (
        f"(server_uri: 127.0.0.1) soak test ran: {total} iterations. total_failures: "
        f"{failures} is within max_failures_threshold: 0.\n"
        + "".join(
            f"soak iteration: {index} elapsed_ms: {latency} peer: test succeeded\n"
            for index, latency in enumerate(samples)
        )
    )


class SoakAccountingTest(unittest.TestCase):
    def parse(self, output, expected_iterations=3):
        return bench_matrix.parse_soak_output(
            output, duration_s=0.1, req_size=0, resp_size=0, expected_iterations=expected_iterations
        )

    def test_all_iterations_have_real_latency_samples(self):
        run = self.parse(soak_output())
        self.assertEqual(run["metrics"]["successful_rpcs"], 3)
        self.assertEqual(run["latency"]["p50_nanos"], 1_000_000)
        self.assertEqual(run["latency"]["p99_nanos"], 2_000_000)
        self.assertEqual(run["latency"]["sample_count"], 3)

    def test_cpp_peer_success_has_different_official_summary_format(self):
        run = self.parse(cpp_soak_output())
        self.assertEqual(run["metrics"]["successful_rpcs"], 3)
        self.assertEqual(run["latency"]["raw_samples_ms"], [0.0, 1.0, 2.0])

    def test_missing_latency_samples_are_not_estimated(self):
        with self.assertRaisesRegex(ValueError, "latency samples"):
            self.parse(soak_output(samples=(0, 1)))

    def test_reported_failures_do_not_count_as_a_successful_cell(self):
        with self.assertRaisesRegex(ValueError, "failures"):
            self.parse(soak_output(successes=2, total=3, failures=1))

    def test_requested_iterations_must_match_completed_iterations(self):
        with self.assertRaisesRegex(ValueError, "requested"):
            self.parse(soak_output(), expected_iterations=4)

    def test_missing_summary_does_not_produce_an_empty_success(self):
        with self.assertRaisesRegex(ValueError, "summary"):
            self.parse("soak iteration: 0 elapsed_ms: 1")


class EndpointResourcesTest(unittest.TestCase):
    def test_sequential_client_processes_contribute_both_cpu_samples(self):
        first = {
            "user_cpu_seconds": 0.12,
            "system_cpu_seconds": 0.02,
            "peak_rss_bytes": 8000,
            "thread_count": 2,
            "supported": True,
            "method": "rusage-child-delta",
        }
        second = {
            "user_cpu_seconds": 0.04,
            "system_cpu_seconds": 0.01,
            "peak_rss_bytes": 12000,
            "thread_count": 3,
            "supported": True,
            "method": "rusage-child-delta",
        }
        total = bench_matrix.aggregate_endpoint_resources([first, second])
        self.assertAlmostEqual(total["user_cpu_seconds"], 0.16)
        self.assertAlmostEqual(total["system_cpu_seconds"], 0.03)
        self.assertEqual(total["peak_rss_bytes"], 12000)
        self.assertEqual(total["thread_count"], 3)

    def test_unmeasured_endpoint_cannot_be_reported_as_supported(self):
        with self.assertRaisesRegex(ValueError, "resource sampling unavailable"):
            bench_matrix.aggregate_endpoint_resources(
                [{"supported": False, "method": "unsupported"}]
            )

    def test_quick_and_soak_runs_cannot_prove_server_ceiling(self):
        runs = [{"metrics": {"duration_nanos": 60_000_000_000}}]
        resources = {"supported": True}
        headroom = {"saturated": False}
        self.assertIn(
            "quick smoke",
            bench_matrix.server_ceiling_exclusion(True, "rpc-bench", headroom, resources, resources, runs),
        )
        self.assertIn(
            "different workload",
            bench_matrix.server_ceiling_exclusion(False, "interop-soak", headroom, resources, resources, runs),
        )

    def test_short_run_and_saturation_are_not_valid_ceiling_evidence(self):
        resources = {"supported": True}
        self.assertIn(
            "60 seconds",
            bench_matrix.server_ceiling_exclusion(
                False, "rpc-bench", {"saturated": False}, resources, resources,
                [{"metrics": {"duration_nanos": 50_000_000_000}}],
            ),
        )
        self.assertIn(
            "saturated",
            bench_matrix.server_ceiling_exclusion(
                False, "rpc-bench", {"saturated": True}, resources, resources,
                [{"metrics": {"duration_nanos": 60_000_000_000}}],
            ),
        )


class CpuCapacityTest(unittest.TestCase):
    def proof(self, capacity=1.0, verified=True):
        return {"verified": verified, "effective_cpu_capacity": capacity if verified else None,
                "affinity_cpus": [0], "affinity_cpu_count": 1,
                "quota_cpu_capacity": capacity, "quota_samples": [],
                "reason": "" if verified else "cgroup mapping unavailable"}

    def monitor(self, proof, threads=8, cpu_seconds=0.96):
        snap = bench_matrix.ProcessSnapshot
        snapshots = [snap(0, 0, 1024, threads, 1, "fixture"),
                     snap(0, 0, 1024, threads, 1, "fixture"),
                     snap(cpu_seconds, 0, 1024, threads, 2, "fixture"),
                     snap(0, 0, 1024, threads, 2, "fixture")]
        with (patch.object(bench_matrix, "sample_process", side_effect=snapshots),
              patch.object(bench_matrix, "collect_process_cpu_capacity", return_value=proof, create=True),
              patch.object(bench_matrix.time, "monotonic", return_value=2)):
            monitor = bench_matrix.ProcessResourceMonitor(111)
            monitor.set_client_pid(222)
            monitor.start_time = 1
            return monitor.stop()

    def test_eight_threads_pinned_to_one_cpu_do_not_create_eight_cpu_budget(self):
        client, _, saturation = self.monitor(self.proof())
        self.assertTrue(saturation["saturated"])
        self.assertEqual(saturation["spare_capacity_pct"], 4.0)
        self.assertEqual(client["cpu_capacity"]["effective_cpu_capacity"], 1)
        self.assertEqual(client["thread_count"], 8)

    def test_fractional_quota_is_the_utilization_denominator(self):
        _, _, saturation = self.monitor(self.proof(0.5), cpu_seconds=0.48)
        self.assertTrue(saturation["saturated"])
        self.assertEqual(saturation["spare_capacity_pct"], 4.0)

    def test_unknown_capacity_fails_headroom_even_at_zero_cpu(self):
        _, _, saturation = self.monitor(self.proof(verified=False), cpu_seconds=0)
        self.assertTrue(saturation["saturated"])
        self.assertFalse(saturation["headroom_verified"])
        self.assertIsNone(saturation["spare_capacity_pct"])
        self.assertEqual(saturation["status"], "FAIL (UNVERIFIED)")

    def test_initial_cpu_snapshot_alone_cannot_prove_unused_capacity(self):
        snap = bench_matrix.ProcessSnapshot
        with (patch.object(bench_matrix, "sample_process",
                           side_effect=[snap(0, 0, 1024, 2, 1, "fixture"),
                                        snap(0, 0, 1024, 2, 1, "fixture"), None, None]),
              patch.object(bench_matrix, "collect_process_cpu_capacity", return_value=self.proof()),
              patch.object(bench_matrix.time, "monotonic", return_value=2)):
            monitor = bench_matrix.ProcessResourceMonitor(111)
            monitor.set_client_pid(222)
            monitor.start_time = 1
            client, _, saturation = monitor.stop()
        self.assertFalse(client["cpu_delta_verified"])
        self.assertFalse(saturation["headroom_verified"])
        self.assertIsNone(saturation["spare_capacity_pct"])

    def test_capacity_change_during_sampling_fails_closed(self):
        snap = bench_matrix.ProcessSnapshot
        snapshots = [snap(0, 0, 1024, 2, 1, "fixture"),
                     snap(0, 0, 1024, 2, 1, "fixture"),
                     snap(0.1, 0, 1024, 2, 2, "fixture"),
                     snap(0, 0, 1024, 2, 2, "fixture")]
        with (patch.object(bench_matrix, "sample_process", side_effect=snapshots),
              patch.object(bench_matrix, "collect_process_cpu_capacity",
                           side_effect=[self.proof(), self.proof(), self.proof(0.5), self.proof()], create=True),
              patch.object(bench_matrix.time, "monotonic", return_value=2)):
            monitor = bench_matrix.ProcessResourceMonitor(111)
            monitor.set_client_pid(222)
            monitor.start_time = 1
            client, _, saturation = monitor.stop()
        self.assertFalse(saturation["headroom_verified"])
        self.assertIn("changed", client["cpu_capacity"]["reason"])
        self.assertEqual(len(client["cpu_capacity"]["capacity_observations"]), 2)

    def fixture(self, version="v2", root="/", membership="/parent/child", quota="50000 100000"):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        base = Path(temp.name)
        proc = base / "proc"
        hierarchy = base / "cgroup"
        cg = hierarchy / "parent"
        (cg / "child").mkdir(parents=True)
        for name in ("self", "123"):
            ns = proc / name / "ns"
            ns.mkdir(parents=True)
            (ns / "mnt").symlink_to("mnt:[1]")
            (ns / "cgroup").symlink_to("cgroup:[2]")
        (proc / "123" / "task" / "123").mkdir(parents=True)
        if version == "v2":
            (proc / "123" / "cgroup").write_text(f"0::{membership}\n")
            device = f"{os.major(hierarchy.stat().st_dev)}:{os.minor(hierarchy.stat().st_dev)}"
            (proc / "123" / "mountinfo").write_text(f"1 0 {device} {root} {hierarchy} rw - cgroup2 cgroup rw\n")
            # Source-informed genuine v2 root: cpu.max/type/events are
            # CFTYPE_NOT_ON_ROOT; readable core and CPU interfaces remain.
            for name, value in (("cgroup.controllers", "cpu io memory"), ("cgroup.procs", ""),
                                ("cgroup.subtree_control", "cpu"), ("cpu.stat", "usage_usec 0")):
                (hierarchy / name).write_text(value)
            (cg / "cpu.max").write_text("200000 100000")
            (cg / "child" / "cpu.max").write_text(quota)
        else:
            (proc / "123" / "cgroup").write_text(f"2:cpu,cpuacct:{membership}\n")
            device = f"{os.major(hierarchy.stat().st_dev)}:{os.minor(hierarchy.stat().st_dev)}"
            (proc / "123" / "mountinfo").write_text(f"1 0 {device} {root} {hierarchy} rw - cgroup cgroup rw,cpu,cpuacct\n")
            (hierarchy / "release_agent").write_text("")
            (hierarchy / "cgroup.sane_behavior").write_text("0")
            for path, value in ((hierarchy, "-1"), (cg, "200000"), (cg / "child", quota)):
                (path / "cpu.cfs_quota_us").write_text(value)
                (path / "cpu.cfs_period_us").write_text("100000")
        (proc / "123" / "task" / "123" / "cgroup").write_text((proc / "123" / "cgroup").read_text())
        return proc, cg

    def collect(self, proc, cpus=(0, 1, 2, 3)):
        with (patch.object(bench_matrix.sys, "platform", "linux"),
              patch.object(bench_matrix.os, "sched_getaffinity", return_value=set(cpus))):
            return bench_matrix.collect_process_cpu_capacity(123, proc_root=proc)

    def test_v2_effective_budget_uses_fractional_child_and_ancestor_minimum(self):
        proc, cg = self.fixture()
        proof = self.collect(proc)
        self.assertTrue(proof["verified"])
        self.assertEqual(proof["effective_cpu_capacity"], 0.5)
        self.assertEqual(len(proof["quota_samples"]), 3)
        self.assertTrue(proof["hierarchy_root"]["verified"])
        self.assertIsNone(proof["quota_samples"][-1]["raw"])
        (cg / "child" / "cpu.max").write_text("max 100000")
        proof = self.collect(proc)
        self.assertEqual(proof["effective_cpu_capacity"], 2)
        self.assertEqual(self.collect(proc, cpus=(0,))["effective_cpu_capacity"], 1)

    def test_v1_unlimited_child_still_obeys_ancestor_quota(self):
        proc, _ = self.fixture(version="v1", quota="-1")
        proof = self.collect(proc)
        self.assertTrue(proof["verified"])
        self.assertEqual(proof["effective_cpu_capacity"], 2)

    def test_enforced_affinity_includes_live_worker_threads(self):
        proc, _ = self.fixture(quota="max 100000")
        (proc / "123" / "task" / "124").mkdir()
        (proc / "123" / "task" / "124" / "cgroup").write_text((proc / "123" / "cgroup").read_text())
        with patch.object(bench_matrix.os, "sched_getaffinity", side_effect=lambda tid: {tid - 123}):
            proof = bench_matrix.collect_process_cpu_capacity(123, proc_root=proc)
        self.assertTrue(proof["verified"])
        self.assertEqual(proof["affinity_cpus"], [0, 1])
        self.assertEqual(proof["effective_cpu_capacity"], 2)

    def test_unlimited_visible_hierarchy_uses_affinity_and_never_thread_count(self):
        proc, cg = self.fixture(quota="max 100000")
        (cg / "cpu.max").write_text("max 100000")
        proof = self.collect(proc, cpus=(0, 1))
        self.assertEqual(proof["effective_cpu_capacity"], 2)
        self.assertIsNone(proof["quota_cpu_capacity"])

    def test_hidden_mount_root_and_missing_quota_are_unverified(self):
        proc, cg = self.fixture(root="/..")
        proof = self.collect(proc)
        self.assertFalse(proof["verified"])
        self.assertIsNone(proof["effective_cpu_capacity"])
        mountinfo = proc / "123" / "mountinfo"
        mountinfo.write_text(mountinfo.read_text().replace("/..", "/"))
        (cg / "cpu.max").unlink()
        self.assertFalse(self.collect(proc)["verified"])

    def test_namespace_root_cannot_certify_capacity_with_hidden_ancestors(self):
        proc, cg = self.fixture(quota="max 100000")
        hierarchy = cg.parent
        (hierarchy / "cpu.max").write_text("400000 100000")
        (hierarchy / "cgroup.type").write_text("domain")
        (hierarchy / "cgroup.events").write_text("populated 1\nfrozen 0")
        proof = self.collect(proc)
        self.assertFalse(proof["verified"])
        self.assertEqual(proof["visible_cpu_capacity"], 2)
        self.assertIsNone(proof["effective_cpu_capacity"])
        self.assertFalse(proof["hierarchy_root"]["verified"])

    def test_missing_root_markers_or_controller_remain_unverified(self):
        proc, cg = self.fixture()
        (cg.parent / "cgroup.controllers").write_text("io memory")
        self.assertFalse(self.collect(proc)["verified"])
        (cg.parent / "cgroup.controllers").write_text("cpu")
        (cg.parent / "cpu.stat").unlink()
        self.assertFalse(self.collect(proc)["verified"])

    def test_unequal_mounted_device_cannot_prove_root_identity(self):
        proc, _ = self.fixture()
        mountinfo = proc / "123" / "mountinfo"
        fields = mountinfo.read_text().split()
        major, minor = map(int, fields[2].split(":"))
        fields[2] = f"{major}:{minor + 1}"
        mountinfo.write_text(" ".join(fields))
        self.assertFalse(self.collect(proc)["verified"])

    def test_namespace_mismatch_and_invalid_quota_fail_closed(self):
        proc, cg = self.fixture()
        (proc / "123" / "ns" / "mnt").unlink()
        (proc / "123" / "ns" / "mnt").symlink_to("mnt:[3]")
        self.assertFalse(self.collect(proc)["verified"])
        (proc / "123" / "ns" / "mnt").unlink()
        (proc / "123" / "ns" / "mnt").symlink_to("mnt:[1]")
        for invalid in ("0 100000", "50000 0", "garbage", "-1 100000"):
            (cg / "child" / "cpu.max").write_text(invalid)
            self.assertFalse(self.collect(proc)["verified"], invalid)

    def test_ambiguous_multiple_cpu_mounts_fail_closed(self):
        proc, _ = self.fixture()
        mountinfo = proc / "123" / "mountinfo"
        mountinfo.write_text(mountinfo.read_text() * 2)
        self.assertFalse(self.collect(proc)["verified"])

    def test_threads_in_unequal_cgroups_cannot_share_a_capacity_proof(self):
        proc, _ = self.fixture()
        task = proc / "123" / "task" / "124"
        task.mkdir()
        (task / "cgroup").write_text("0::/other\n")
        proof = self.collect(proc)
        self.assertFalse(proof["verified"])
        self.assertIn("unequal", proof["reason"])

    def test_long_run_legacy_resources_still_cannot_certify_server_ceiling(self):
        reason = bench_matrix.server_ceiling_exclusion(
            False, "rpc-bench", {"saturated": False}, {"supported": True}, {"supported": True},
            [{"metrics": {"duration_nanos": 60_000_000_000}}])
        self.assertIn("capacity/headroom proof", reason)


class BenchmarkReportTest(unittest.TestCase):
    def test_successful_client_exit_without_report_fails_the_cell(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fake = Path(temp_dir) / "fake-peer"
            fake.write_text(
                "#!/usr/bin/env python3\n"
                "import socket, sys, time\n"
                "if sys.argv[1] == 'server':\n"
                "    port = int(next(arg.split('=', 1)[1] for arg in sys.argv if arg.startswith('--port=')))\n"
                "    sock = socket.socket()\n"
                "    sock.bind(('127.0.0.1', port))\n"
                "    sock.listen(1)\n"
                "    print(f'READY port={port} addr=127.0.0.1:{port}', flush=True)\n"
                "    time.sleep(30)\n"
            )
            fake.chmod(0o755)
            ok, report, reason = bench_matrix.run_single_benchmark(
                peer_registry=bench_matrix.PeerRegistry(REPO_ROOT),
                server_peer="native",
                client_peer="native",
                shape="unary",
                host="127.0.0.1",
                port=0,
                quick=True,
                timeout_secs=5,
                verbose=False,
                binary_override=str(fake),
            )
            self.assertFalse(ok)
            self.assertIsNone(report)
            self.assertIn("BenchmarkReport", reason)


class PeerManifestTest(unittest.TestCase):
    def test_shared_cache_binary_precedes_legacy_standalone_release(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            legacy = root / "rpc-bench" / "target" / "release" / "rpc-bench"
            shared_debug = root / "target" / "debug" / "rpc-bench"
            shared_release = root / "target" / "release" / "rpc-bench"
            for path in (legacy, shared_debug):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"stub")
                path.chmod(0o755)
            self.assertEqual(bench_matrix.find_binary(root), shared_debug.resolve())
            shared_release.parent.mkdir(parents=True, exist_ok=True)
            shared_release.write_bytes(b"stub")
            shared_release.chmod(0o755)
            self.assertEqual(bench_matrix.find_binary(root), shared_release.resolve())

    def test_matrix_tonic_build_caps_cargo_jobs(self):
        with patch.dict(os.environ, {"CARGO_BUILD_JOBS": "16"}):
            self.assertEqual(bench_matrix.bounded_cargo_env()["CARGO_BUILD_JOBS"], "2")
        with patch.dict(os.environ, {"CARGO_BUILD_JOBS": "1"}):
            self.assertEqual(bench_matrix.bounded_cargo_env()["CARGO_BUILD_JOBS"], "1")
        with patch.dict(os.environ, {"CARGO_BUILD_JOBS": "0"}):
            with self.assertRaisesRegex(ValueError, "positive integer"):
                bench_matrix.bounded_cargo_env()

    def test_invalid_peer_manifest_is_not_silently_skipped(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            peer_dir = Path(temp_dir) / "peers"
            peer_dir.mkdir()
            (peer_dir / "tonic.json").write_text("{invalid json")
            with self.assertRaisesRegex(ValueError, "tonic.json"):
                bench_matrix.PeerRegistry(REPO_ROOT, peer_dir)

    def test_tonic_peer_versions_match_checked_lockfile(self):
        peer = json.loads((REPO_ROOT / "rpc-bench/peers/tonic.json").read_text())
        locked = tomllib.loads((REPO_ROOT / "tests/interop/tonic/Cargo.lock").read_text())
        versions = {p["name"]: p["version"] for p in locked["package"]}
        self.assertEqual(peer["upstream_pin"]["version"], f"v{versions['tonic']}")
        self.assertEqual(peer["upstream_pin"]["prost_version"], f"v{versions['prost']}")

    def test_all_excludes_known_unsupported_client_role(self):
        self.assertNotIn("tonic-prost", bench_matrix.parse_peers("all", ["native"], role="client"))
        self.assertIn("tonic-prost", bench_matrix.parse_peers("all", ["native"], role="server"))

    def test_legacy_tonic_server_never_substitutes_pbrs_when_prost_is_missing(self):
        registry = bench_matrix.PeerRegistry(REPO_ROOT)
        registry.tonic_build_error = "missing pinned peer"
        with patch.object(registry, "resolve_tonic_binary", return_value=None):
            ok, report, reason = bench_matrix.run_single_benchmark(
                registry, "tonic", "native", "unary", "127.0.0.1", 0,
                True, 5, False, sys.executable,
            )
        self.assertFalse(ok)
        self.assertIsNone(report)
        self.assertIn("Refusing to substitute", reason)
        self.assertIn("missing pinned peer", reason)


if __name__ == "__main__":
    unittest.main()
