#!/usr/bin/env python3
"""Deterministic unit and integration tests for official gRPC RPC and channel soak adapters.

Covers acceptance criteria for IO-10:
  - Short deterministic tests detect omitted iterations, exceeded latency budgets, and failed calls.
  - Channel soak actually creates and drops channels on each iteration.
  - Completion accounting tracks requested, completed, omitted, succeeded, and failed iterations.
  - Thread count and concurrency are enforced and recorded.
  - Long-running outputs preserve successes, all failure classes, resources, and configured budgets.
  - Shorter local smoke runs are classified as 'smoke' and not mislabeled as full qualification soak.

Run with:
  python3 -m unittest tests/interop/test_soak.py
"""

from __future__ import annotations

import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import unittest
from typing import Dict, Optional

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
# Honor CARGO_TARGET_DIR so isolated/lane builds resolve their own binaries;
# defaults to the in-tree target dir when unset (CI behavior unchanged).
_TARGET_DIR = Path(os.environ.get("CARGO_TARGET_DIR", str(REPO_ROOT / "target")))
CLIENT_BIN = _TARGET_DIR / "debug" / "pbrs-grpc-interop-client"
SERVER_BIN = _TARGET_DIR / "debug" / "pbrs-grpc-interop-server"


def pick_unused_port() -> int:
    """Find an unused TCP port on loopback."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def parse_soak_summary(output: str) -> Dict[str, str]:
    """Extract key-value pairs from the [soak_summary] line in process output."""
    for line in output.splitlines():
        if "[soak_summary]" in line:
            idx = line.index("[soak_summary]") + len("[soak_summary]")
            tokens = line[idx:].strip().split()
            parsed: Dict[str, str] = {}
            for token in tokens:
                if "=" in token:
                    k, v = token.split("=", 1)
                    parsed[k] = v
            return parsed
    return {}


class ServerGuard:
    """Manage lifecycle of spawned interop server."""

    def __init__(self, port: int):
        self.port = port
        self.proc = subprocess.Popen(
            [str(SERVER_BIN), f"--port={port}"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self._wait_ready()

    def _wait_ready(self, timeout_sec: float = 10.0) -> None:
        start = time.monotonic()
        while time.monotonic() - start < timeout_sec:
            if self.proc.poll() is not None:
                out = self.proc.stdout.read() if self.proc.stdout else ""
                err = self.proc.stderr.read() if self.proc.stderr else ""
                raise RuntimeError(f"Server exited prematurely with code {self.proc.returncode}:\n{out}\n{err}")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.2):
                    return
            except (socket.error, OSError):
                time.sleep(0.05)
        self.terminate()
        raise TimeoutError(f"Server failed to become ready on port {self.port} within {timeout_sec}s")

    def terminate(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        if self.proc.stdout:
            self.proc.stdout.close()
        if self.proc.stderr:
            self.proc.stderr.close()


class TestSoakCliFlags(unittest.TestCase):
    """Test CLI flag parsing, defaults, validation, and rejection in pbrs-grpc-interop-client."""

    @classmethod
    def setUpClass(cls):
        # Ensure binaries are built
        if not CLIENT_BIN.exists():
            subprocess.run(["cargo", "build", "--bin", "pbrs-grpc-interop-client", "-p", "pbrs-grpc"], check=True)

    def run_client(self, args: list[str]) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(CLIENT_BIN)] + args,
            capture_output=True,
            text=True,
        )

    def test_unknown_flag_rejected(self):
        res = self.run_client(["--unknown_soak_option=123"])
        self.assertEqual(res.returncode, 1)
        self.assertIn("unknown flag", res.stderr)

    def test_missing_flag_values_rejected(self):
        for flag in ["--soak_iterations", "--max_failures", "--per_rpc_timeout_ms", "--overall_timeout_seconds", "--soak_num_threads"]:
            res = self.run_client([flag])
            self.assertEqual(res.returncode, 1, f"Expected {flag} without value to fail")
            self.assertIn("missing value for flag", res.stderr)

    def test_invalid_iterations_rejected(self):
        # Non-numeric
        res = self.run_client(["--soak_iterations=invalid"])
        self.assertEqual(res.returncode, 1)
        self.assertIn("invalid value", res.stderr)

        # Zero iterations
        res = self.run_client(["--soak_iterations=0"])
        self.assertEqual(res.returncode, 1)
        self.assertIn("must be at least 1", res.stderr)

        # Negative iterations
        res = self.run_client(["--soak_iterations=-5"])
        self.assertEqual(res.returncode, 1)

    def test_invalid_threads_rejected(self):
        res = self.run_client(["--soak_num_threads=0"])
        self.assertEqual(res.returncode, 1)
        self.assertIn("must be at least 1", res.stderr)

        res = self.run_client(["--soak_num_threads=invalid"])
        self.assertEqual(res.returncode, 1)

    def test_indivisible_threads_rejected(self):
        res = self.run_client([
            "--server_port=10000",
            "--test_case=rpc_soak",
            "--soak_iterations=10",
            "--soak_num_threads=3",
        ])
        self.assertEqual(res.returncode, 1)
        self.assertIn("divisible by soak_num_threads", res.stderr)

    def test_qualification_soak_cannot_be_mislabeled(self):
        # A run with fewer than 1000 iterations cannot be labeled qualification soak
        res = self.run_client([
            "--qualification",
            "--soak_iterations=10",
        ])
        self.assertEqual(res.returncode, 1)
        self.assertIn("cannot be labeled qualification soak", res.stderr)
        self.assertIn("shorter runs are local smoke tests", res.stderr)


class TestSoakExecution(unittest.TestCase):
    """Deterministic end-to-end execution tests against reference interop server."""

    server_guard: Optional[ServerGuard] = None
    server_port: int = 0

    @classmethod
    def setUpClass(cls):
        if not CLIENT_BIN.exists() or not SERVER_BIN.exists():
            subprocess.run(["cargo", "build", "--bins", "-p", "pbrs-grpc"], check=True)
        cls.server_port = pick_unused_port()
        cls.server_guard = ServerGuard(cls.server_port)

    @classmethod
    def tearDownClass(cls):
        if cls.server_guard is not None:
            cls.server_guard.terminate()

    def run_client(self, args: list[str]) -> subprocess.CompletedProcess[str]:
        cmd = [
            str(CLIENT_BIN),
            f"--server_port={self.server_port}",
        ] + args
        return subprocess.run(cmd, capture_output=True, text=True)

    def test_rpc_soak_happy_path_accounting_and_resources(self):
        """Test rpc_soak happy path: verifies successes, single channel reuse, threads, and smoke run_type."""
        res = self.run_client([
            "--test_case=rpc_soak",
            "--soak_iterations=4",
            "--soak_num_threads=2",
            "--per_rpc_timeout_ms=5000",
            "--overall_timeout_seconds=10",
        ])
        self.assertEqual(res.returncode, 0, f"Client failed:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}")
        self.assertIn("Passed", res.stdout)

        # Verify per-iteration log output matching official regex pattern
        self.assertIn("soak iteration:", res.stderr)
        self.assertIn("succeeded", res.stderr)
        self.assertIn("thread_id: 0", res.stderr)
        self.assertIn("thread_id: 1", res.stderr)

        summary = parse_soak_summary(res.stdout)
        self.assertTrue(summary, f"Could not find [soak_summary] in:\n{res.stdout}")
        self.assertEqual(summary.get("test_case"), "rpc_soak")
        self.assertEqual(summary.get("run_type"), "smoke", "Short run must be labeled smoke, not qualification_soak")
        self.assertEqual(summary.get("iterations_requested"), "4")
        self.assertEqual(summary.get("iterations_completed"), "4")
        self.assertEqual(summary.get("iterations_succeeded"), "4")
        self.assertEqual(summary.get("iterations_omitted"), "0")
        self.assertEqual(summary.get("total_failures"), "0")
        self.assertEqual(summary.get("thread_count"), "2")
        self.assertEqual(summary.get("channels_created"), "1", "rpc_soak reuses a single channel")
        self.assertEqual(summary.get("channels_dropped"), "1")
        self.assertEqual(summary.get("non_ok_status"), "0")
        self.assertEqual(summary.get("latency_budget_exceeded"), "0")
        self.assertEqual(summary.get("payload_mismatch"), "0")
        self.assertEqual(summary.get("channel_error"), "0")
        self.assertEqual(summary.get("omitted_iterations"), "0")

    def test_channel_soak_creates_and_drops_channels(self):
        """Test channel_soak actually creates and drops channels on each iteration."""
        iters = 4
        res = self.run_client([
            "--test_case=channel_soak",
            f"--soak_iterations={iters}",
            "--soak_num_threads=1",
            "--per_rpc_timeout_ms=5000",
            "--overall_timeout_seconds=10",
        ])
        self.assertEqual(res.returncode, 0, f"Client failed:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}")
        self.assertIn("Passed", res.stdout)

        summary = parse_soak_summary(res.stdout)
        self.assertTrue(summary, f"Could not find [soak_summary] in:\n{res.stdout}")
        self.assertEqual(summary.get("test_case"), "channel_soak")
        self.assertEqual(summary.get("run_type"), "smoke")
        self.assertEqual(summary.get("iterations_requested"), str(iters))
        self.assertEqual(summary.get("iterations_completed"), str(iters))
        self.assertEqual(summary.get("iterations_succeeded"), str(iters))
        self.assertEqual(summary.get("total_failures"), "0")

        # Acceptance criteria check: channel_soak actually creates/drops channels per iteration
        self.assertEqual(
            summary.get("channels_created"),
            str(iters),
            "channel_soak must create a distinct channel for each iteration",
        )
        self.assertEqual(
            summary.get("channels_dropped"),
            str(iters),
            "channel_soak must destroy/drop the channel after each iteration",
        )

    def test_detect_exceeded_latency_budget(self):
        """Test detection of iterations exceeding the configured latency budget."""
        # per_rpc_timeout_ms = 0 ensures any measured latency >0ms exceeds budget
        res = self.run_client([
            "--test_case=channel_soak",
            "--soak_iterations=4",
            "--per_rpc_timeout_ms=0",
            "--max_failures=0",
            "--overall_timeout_seconds=10",
        ])
        combined = res.stdout + "\n" + res.stderr
        # Must fail when failures exceed max_failures=0
        self.assertEqual(res.returncode, 1, f"Expected failure due to latency budget exceed:\n{combined}")
        self.assertIn("latency_budget_exceeded", combined)

        # When failure budget covers it (--max_failures=4), the test passes but preserves all failure classes
        res_lenient = self.run_client([
            "--test_case=channel_soak",
            "--soak_iterations=4",
            "--per_rpc_timeout_ms=0",
            "--max_failures=4",
            "--overall_timeout_seconds=10",
        ])
        self.assertEqual(res_lenient.returncode, 0, f"Expected success with failure budget:\n{res_lenient.stderr}")
        summary = parse_soak_summary(res_lenient.stdout)
        self.assertEqual(summary.get("total_failures"), "4")
        self.assertEqual(summary.get("latency_budget_exceeded"), "4")
        self.assertEqual(summary.get("iterations_succeeded"), "0")

    def test_detect_failed_calls_to_unavailable_endpoint(self):
        """Test detection and accounting of failed calls when server is down."""
        dead_port = pick_unused_port()
        res = subprocess.run(
            [
                str(CLIENT_BIN),
                f"--server_port={dead_port}",
                "--test_case=rpc_soak",
                "--soak_iterations=3",
                "--max_failures=0",
                "--overall_timeout_seconds=5",
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 1)
        combined = res.stdout + "\n" + res.stderr
        self.assertTrue(
            "non_ok_status" in combined or "channel_error" in combined,
            f"Expected failure breakdown to record connection/status failure:\n{combined}",
        )

    def test_detect_omitted_iterations_on_deadline(self):
        """Test detection of omitted iterations when overall deadline expires before completing requested iterations."""
        # Request 1000 iterations with 100ms sleep between RPCs and a 1s deadline
        res = self.run_client([
            "--test_case=rpc_soak",
            "--soak_iterations=1000",
            "--overall_timeout_seconds=1",
            "--soak_min_time_ms_between_rpcs=100",
        ])
        self.assertEqual(res.returncode, 1, "Expected timeout failure")
        combined = res.stdout + "\n" + res.stderr
        self.assertIn("omitted", combined)

        summary = parse_soak_summary(combined)
        if summary:
            omitted = int(summary.get("iterations_omitted", "0"))
            completed = int(summary.get("iterations_completed", "0"))
            self.assertGreater(omitted, 0, "Expected omitted iterations to be greater than 0")
            self.assertLess(completed, 1000, "Expected completed iterations to be less than 1000")
            self.assertEqual(omitted + completed, 1000)

    def test_smoke_is_not_mislabeled_as_qualification_soak(self):
        """Verify that short local smoke tests report 'smoke' run_type and never 'qualification_soak'."""
        res = self.run_client([
            "--test_case=rpc_soak",
            "--soak_iterations=5",
        ])
        self.assertEqual(res.returncode, 0)
        summary = parse_soak_summary(res.stdout)
        self.assertEqual(summary.get("run_type"), "smoke")
        self.assertNotEqual(summary.get("run_type"), "qualification_soak")

        # Explicit --smoke flag preserves smoke label
        res_smoke = self.run_client([
            "--test_case=rpc_soak",
            "--smoke",
            "--soak_iterations=5",
        ])
        self.assertEqual(res_smoke.returncode, 0)
        summary_smoke = parse_soak_summary(res_smoke.stdout)
        self.assertEqual(summary_smoke.get("run_type"), "smoke")


if __name__ == "__main__":
    unittest.main()
