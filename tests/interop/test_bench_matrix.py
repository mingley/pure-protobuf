"""Regression tests for mixed-peer benchmark accounting."""

import importlib.util
import json
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
