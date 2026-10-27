"""Raw CPU-claim preflight must retain losses and reject unverifiable summaries.

Every record built below is SYNTHETIC. Passing numerical test data is never
real measurement evidence and must never make `qualified` true.
"""

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/verify-performance-claim.py"
SPEC = importlib.util.spec_from_file_location("performance_claim", SCRIPT)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)
SOURCE = "a" * 40


def write_artifact(directory, name, value):
    path = directory / name
    path.write_text(json.dumps(value, separators=(",", ":")))
    return {"path": name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def synthetic_fixture(directory):
    """A named D4 cell, two architectures and five randomized tonic pairs."""
    pins = {}
    for index, peer in enumerate(("pbrs-grpc", "tonic")):
        pins[peer] = {"source": SOURCE if index == 0 else "b" * 40,
                      "version": "synthetic-test-version", "allocator": "system", "builds": {}}
        for arch in CHECK.ARCHES:
            pins[peer]["builds"][arch] = {
                "binary_sha256": hashlib.sha256(f"SYNTHETIC {peer} {arch}".encode()).hexdigest(),
                "toolchain": "synthetic Rust compiler", "profile": {
                    "kind": "rust", "opt_level": 3, "codegen_units": 1, "lto": "thin"}}
    hosts = {}
    for arch in CHECK.ARCHES:
        for side in ("client", "server"):
            name = f"{arch}-{side}"
            hosts[name] = write_artifact(directory, f"{name}.json", {
                "os": "linux", "architecture": arch, "cpu": "SYNTHETIC CPU",
                "kernel": "SYNTHETIC kernel", "hyperthread_topology": "SYNTHETIC no siblings",
                "interrupt_affinity": "SYNTHETIC core 0", "sysctl": "SYNTHETIC recorded values",
                "ring_buffers": "SYNTHETIC recorded rings", "dedicated": True,
                "isolated_cores": True, "turbo_disabled": True, "governor": "performance",
                "nic_gbps": 10, "mtu": 1500, "cores": [1]})
    cell = {"id": "SYNTHETIC-unary", "settings": {
        "offered_qps": 200, "connections": 1, "concurrency": 1,
        "stream_window": 16 << 20, "connection_window": 16 << 20,
        "max_frame_size": 1 << 20, "max_message_size": 4 << 20,
        "compression": "identity", "tls_cipher": "none", "rpc_shape": "unary", "tcp_nodelay": True},
        "payload_sha256": "c" * 64, "handler_contract_sha256": "d" * 64}
    data = {"schema": "pbrs.rpc-cpu-evidence.v1", "scope": "named-rpc-cpu-cells",
            "source": SOURCE, "category": "D4", "synthetic": True,
            "pins": pins, "hosts": hosts, "random_seed": 27, "cells": [cell], "runs": []}
    for arch in CHECK.ARCHES:
        for repeat in range(1, 6):
            pair = {"cell": cell["id"], "architecture": arch, "reference": "tonic", "repeat": repeat,
                    "order": CHECK.expected_order(27, cell["id"], arch, "tonic", repeat)}
            for name, peer in (("candidate", "pbrs-grpc"), ("reference_run", "tonic")):
                pin = {key: pins[peer][key] for key in ("source", "version", "allocator")}
                pin["build"] = pins[peer]["builds"][arch]
                pair[name] = {
                    "peer": peer, "pin": pin, "settings": copy.deepcopy(cell["settings"]),
                    "payload_sha256": cell["payload_sha256"],
                    "handler_contract_sha256": cell["handler_contract_sha256"],
                    "allocation_counters": False,
                    "hosts": {side: f"{arch}-{side}" for side in ("client", "server")},
                    "warmup_seconds": 15, "warmup_rpcs": 10_000, "duration_seconds": 60,
                    "offered": 12_000, "dispatched": 12_000, "completed": 12_000, "successful": 12_000,
                    "failed": 0, "rejected": 0, "timed_out": 0, "unfinished": 0,
                    "arrival": "constant", "latency_origin": "scheduled_dispatch",
                    "latencies_ns": [1000] * 12_000, "scheduling_lag_ns": [1] * 12_000,
                    "cpu_seconds": {"client": 1.0, "server": 0.7 if name == "candidate" else 1.0},
                    "rss_bytes": {side: {"steady": 1_000_000, "peak": 1_100_000}
                                  for side in ("client", "server")}}
            data["runs"].append(write_artifact(directory, f"{arch}-{repeat}.json", pair))
    path = directory / "campaign.json"
    path.write_text(json.dumps(data))
    return path, data


class PerformanceClaimTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.path, self.data = synthetic_fixture(self.directory)

    def verify(self):
        self.path.write_text(json.dumps(self.data))
        return CHECK.verify(self.path, SOURCE)

    def mutate_run(self, change, index=0):
        ref = self.data["runs"][index]
        row = json.loads((self.directory / ref["path"]).read_text())
        change(row)
        self.data["runs"][index] = write_artifact(self.directory, ref["path"], row)

    def assert_invalid(self, phrase):
        report = self.verify()
        self.assertFalse(report["evidence_checks_passed"])
        self.assertFalse(report["qualified"])
        self.assertTrue(any(phrase in error for error in report["errors"]), report)

    def test_synthetic_fixture_passes_numerical_bounds_but_never_evidence(self):
        report = self.verify()
        self.assertTrue(report["numerical_checks_passed"], report)
        self.assertEqual(len(report["comparisons"]), 2)
        self.assertFalse(report["evidence_checks_passed"])
        self.assertFalse(report["qualified"])
        self.assertIn("synthetic", report["errors"][0])
        for comparison in report["comparisons"]:
            interval = comparison["candidate_to_reference"]["cpu"]
            self.assertAlmostEqual(interval["estimate"], .7)
            self.assertEqual(interval["resamples"], 10_000)
            self.assertEqual(interval["confidence"], .95)

    def test_source_pin_and_required_comparators_cannot_be_selected_away(self):
        self.data["source"] = "b" * 40
        self.assert_invalid("expected full commit")
        self.data["source"] = SOURCE
        self.data["category"] = "C8"
        self.assert_invalid("required peer pins")
        self.data["category"] = "D4"
        self.data["pins"]["pbrs-grpc"]["builds"]["aarch64"] = copy.deepcopy(
            self.data["pins"]["pbrs-grpc"]["builds"]["x86_64"])
        self.assert_invalid("across architectures")

    def test_hash_path_and_duplicate_artifact_rejection(self):
        original = copy.deepcopy(self.data)
        self.data["runs"][0]["sha256"] = "0" * 64
        self.assert_invalid("hash mismatch")
        self.data = copy.deepcopy(original)
        self.data["runs"][0]["path"] = "../outside.json"
        self.assert_invalid("escapes campaign")
        self.data = copy.deepcopy(original)
        self.data["runs"][0]["path"] = str(self.directory / original["runs"][0]["path"])
        self.assert_invalid("must be relative")
        self.data = original
        self.data["runs"].append(copy.deepcopy(self.data["runs"][0]))
        self.assert_invalid("duplicate raw artifact")

    def test_missing_architecture_short_repeats_and_wrong_order_fail(self):
        original = copy.deepcopy(self.data)
        self.data["runs"] = self.data["runs"][:5]
        self.assert_invalid("missing required cell/architecture")
        self.data = copy.deepcopy(original)
        self.data["runs"].pop()
        self.assert_invalid("at least five")
        self.data = original
        self.mutate_run(lambda row: row["order"].reverse())
        self.assert_invalid("frozen seeded randomization")

    def test_unmatched_settings_and_drifted_sources_fail(self):
        self.mutate_run(lambda row: row["candidate"]["settings"].update(concurrency=2))
        self.assert_invalid("effective endpoint settings")
        self.path, self.data = synthetic_fixture(self.directory)
        self.mutate_run(lambda row: row["candidate"]["pin"].update(source="f" * 40))
        self.assert_invalid("mixed peer source")

    def test_unobserved_host_controls_and_loopback_fail(self):
        ref = self.data["hosts"]["x86_64-client"]
        path = self.directory / ref["path"]
        snapshot = json.loads(path.read_text())
        snapshot["dedicated"] = False
        self.data["hosts"]["x86_64-client"] = write_artifact(self.directory, ref["path"], snapshot)
        self.assert_invalid("dedicated")
        self.path, self.data = synthetic_fixture(self.directory)
        self.mutate_run(lambda row: row["candidate"]["hosts"].update(server="x86_64-client"))
        self.assert_invalid("separate recorded hosts")

    def test_failed_incomplete_short_and_coordinated_omission_runs_fail(self):
        for field, value, message in (
                ("failed", 1, "success/error conservation"),
                ("unfinished", 1, "completion conservation"),
                ("duration_seconds", 59, "measurement seconds"),
                ("warmup_seconds", 14, "warmup seconds"),
                ("warmup_rpcs", 9999, "warmup RPCs"),
                ("latency_origin", "actual_dispatch", "scheduled-send latencies"),
                ("allocation_counters", True, "allocation instrumentation")):
            with self.subTest(field=field):
                self.path, self.data = synthetic_fixture(self.directory)
                self.mutate_run(lambda row: row["candidate"].update({field: value}))
                self.assert_invalid(message)
        self.path, self.data = synthetic_fixture(self.directory)
        self.mutate_run(lambda row: row["candidate"]["latencies_ns"].pop())
        self.assert_invalid("sample count")
        self.path, self.data = synthetic_fixture(self.directory)
        self.mutate_run(lambda row: row["candidate"]["scheduling_lag_ns"].__setitem__(0, 101))
        self.assert_invalid("scheduling lag exceeds")

    def test_bool_metrics_zero_latency_and_missing_side_rss_fail(self):
        for change, message in (
                (lambda row: row["candidate"].update(completed=True), "invalid completed"),
                (lambda row: row["candidate"]["settings"].update(concurrency=True), "effective endpoint settings"),
                (lambda row: row["candidate"]["pin"]["build"]["profile"].update(codegen_units=True), "mixed peer source"),
                (lambda row: row["candidate"]["cpu_seconds"].update(server=True), "invalid server CPU"),
                (lambda row: row["candidate"]["latencies_ns"].__setitem__(0, True), "nanosecond latency"),
                (lambda row: row["candidate"].update(latencies_ns=[0] * 12000), "zero p50"),
                (lambda row: row["candidate"].update(rss_bytes={"steady": 1, "peak": 1}), "RSS")):
            with self.subTest(message=message):
                self.path, self.data = synthetic_fixture(self.directory)
                self.mutate_run(change)
                self.assert_invalid(message)

    def test_supplied_winner_and_intervals_cannot_hide_raw_cpu_loss(self):
        self.data["claimed_winner"] = "pbrs-grpc"
        self.data["confidence_intervals"] = {"cpu": [.1, .2]}
        for index in range(len(self.data["runs"])):
            self.mutate_run(lambda row: row["candidate"]["cpu_seconds"].update(server=1.2), index)
        report = self.verify()
        self.assertFalse(report["numerical_checks_passed"])
        self.assertTrue(all(row["verdict"] == "fails_numerical_bounds" for row in report["comparisons"]))
        self.assertTrue(all("cpu" in row["failed_bounds"] for row in report["comparisons"]))
        self.assertAlmostEqual(report["comparisons"][0]["candidate_to_reference"]["cpu"]["estimate"], 1.2)

    def test_tail_and_memory_losses_survive_recomputation(self):
        for index in range(len(self.data["runs"])):
            def regress(row):
                row["candidate"]["latencies_ns"] = [1200] * 12_000
                row["candidate"]["rss_bytes"]["server"] = {"steady": 2_000_000, "peak": 3_000_000}
            self.mutate_run(regress, index)
        report = self.verify()
        self.assertFalse(report["numerical_checks_passed"])
        for row in report["comparisons"]:
            self.assertEqual(set(row["failed_bounds"]), {"p99", "steady_rss", "peak_rss"})

    def test_bootstrap_uncertainty_is_not_a_win(self):
        interval = CHECK.paired_interval([.5, .6, .7, .9, 1.1])
        self.assertLess(interval["low"], .8)
        self.assertGreater(interval["high"], .8)

    def test_duplicate_json_keys_nan_and_old_diagnostics_are_rejected(self):
        for text, phrase in (("{\"schema\":1,\"schema\":2}", "duplicate JSON key"),
                             ("{\"number\":NaN}", "non-finite JSON constant")):
            self.path.write_text(text)
            result = CHECK.verify(self.path, SOURCE)
            self.assertFalse(result["evidence_checks_passed"])
            self.assertIn(phrase, result["errors"][0])
        old = SCRIPT.parents[1] / "docs/evidence/efficiency-20261027/results.json"
        result = CHECK.verify(old, SOURCE)
        self.assertFalse(result["evidence_checks_passed"])
        self.assertIn("summary diagnostics", result["errors"][0])

    def test_cli_rejects_synthetic_campaign_with_machine_readable_output(self):
        result = subprocess.run([sys.executable, str(SCRIPT), str(self.path), "--expected-source", SOURCE],
                                text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertFalse(json.loads(result.stdout)["evidence_checks_passed"])


if __name__ == "__main__":
    unittest.main()
