#!/usr/bin/env python3
"""Unit and integration tests for scripts/grpc-interop.sh test runner.

Exercises mock / fake peer behaviors:
  - Server startup failure (abrupt exit on launch)
  - Occupied port detection (cannot bind address already in use)
  - Case-level timeout / hang (bounded deadline kills hung client/server)
  - Nonzero exit from client (assertion failure or error status)
  - Successful case execution (valid reporting, log retention, exit code 0)
  - Process cleanup and leak prevention (tracked child PIDs terminated cleanly)
  - Flaky first attempt (retry pass stays visible and fails the gate)
  - Interruption (SIGINT retains logs/results/report and exits nonzero)
  - Concurrent runs (distinct dynamic ports, no shared or deleted files)

Run with:
  python3 -m unittest tests.interop.test_runner
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time
import unittest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
SCRIPT_PATH = REPO_ROOT / "scripts" / "grpc-interop.sh"
REPORT_SCRIPT = REPO_ROOT / "scripts" / "interop-report.py"
CASES_JSON = REPO_ROOT / "tests" / "interop" / "cases.json"


class InteropRunnerTest(unittest.TestCase):
    """Test suite exercising scripts/grpc-interop.sh with mock peer behaviors."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.work_dir = Path(self.temp_dir.name)
        self.log_dir = self.work_dir / "interop-logs"

    def tearDown(self):
        self.temp_dir.cleanup()

    def create_executable_script(self, name: str, content: str) -> Path:
        """Create an executable script inside the temporary directory."""
        path = self.work_dir / name
        path.write_text(content, encoding="utf-8")
        path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IRUSR)
        return path

    def create_mock_server(self, extra_code: str = "") -> Path:
        """Create a mock server that binds to the given --port and accepts connections."""
        content = f"""#!/usr/bin/env python3
import sys, socket, time

{extra_code}

port = 10000
for i, arg in enumerate(sys.argv):
    if arg in ("--port", "-port") and i + 1 < len(sys.argv):
        port = int(sys.argv[i + 1])
    elif arg.startswith("--port="):
        port = int(arg.split("=")[1])
    elif arg.startswith("-port="):
        port = int(arg.split("=")[1])

s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", port))
s.listen(5)
print(f"Mock server listening on 127.0.0.1:{{port}}", flush=True)

while True:
    try:
        conn, _ = s.accept()
        conn.close()
    except Exception:
        break
"""
        return self.create_executable_script("mock_server.py", content)

    def create_mock_client(self, exit_code: int = 0, message: str = "OK", delay: float = 0.0) -> Path:
        """Create a mock client that prints a message, optionally sleeps, and exits with code."""
        content = f"""#!/usr/bin/env python3
import sys, time

if {delay} > 0:
    time.sleep({delay})

if {exit_code} == 0:
    print("{message}", flush=True)
else:
    sys.stderr.write("{message}\\n")
sys.exit({exit_code})
"""
        return self.create_executable_script("mock_client.py", content)

    def run_interop_script(
        self,
        args: list[str] | None = None,
        env_overrides: dict[str, str] | None = None,
        timeout: float = 30.0,
    ) -> subprocess.CompletedProcess:
        """Run scripts/grpc-interop.sh with controlled environment and arguments."""
        cmd = [str(SCRIPT_PATH)] + (args if args is not None else ["--self-only"])
        env = os.environ.copy()
        env["SKIP_BUILD"] = "1"
        env["GRPC_INTEROP_SKIP_BUILD"] = "1"
        env["GRPC_INTEROP_LOG_DIR"] = str(self.log_dir)
        env["GRPC_INTEROP_CASES"] = "empty_unary"
        if env_overrides:
            env.update(env_overrides)

        return subprocess.run(
            cmd,
            cwd=str(REPO_ROOT),
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
        )

    def test_mock_server_startup_failure(self):
        """A server that crashes on startup must produce retained logs and fail closed."""
        failing_server = self.create_executable_script(
            "failing_server.py",
            """#!/usr/bin/env python3
import sys
sys.stderr.write("FATAL: server initialization crashed with signal 11\\n")
sys.exit(1)
""",
        )

        proc = self.run_interop_script(
            env_overrides={"GRPC_INTEROP_KERNEL_SERVER": str(failing_server)}
        )

        self.assertNotEqual(proc.returncode, 0, "Script must fail closed on server startup failure")

        # Retained logs must exist
        self.assertTrue(self.log_dir.exists(), "Log directory must exist")
        server_log = self.log_dir / "server-kernel.log"
        self.assertTrue(server_log.exists(), "server-kernel.log must be retained")
        self.assertIn("FATAL: server initialization crashed", server_log.read_text())

        # Case log must exist and record failure
        case_log = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt1.log"
        self.assertTrue(case_log.exists(), "Per-case attempt log must be retained on startup failure")

        # Results and report must record failure
        results_path = self.log_dir / "results.json"
        self.assertTrue(results_path.exists(), "results.json must exist")
        with results_path.open() as f:
            results_data = json.load(f)
        self.assertTrue(any(r["status"] == "failed" for r in results_data.get("results", [])))

        report_path = self.log_dir / "report.json"
        self.assertTrue(report_path.exists(), "report.json must be generated")
        with report_path.open() as f:
            report_data = json.load(f)
        self.assertEqual(report_data.get("overall_status"), "failed")
        self.assertFalse(report_data.get("overall_passed"))

    def test_occupied_port_detection(self):
        """Starting a server on an already-occupied port must fail cleanly with retained logs."""
        sock = socket.socket()
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("127.0.0.1", 0))
        sock.listen(1)
        occupied_port = sock.getsockname()[1]

        try:
            mock_server = self.create_mock_server()
            proc = self.run_interop_script(
                env_overrides={
                    "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                    "GRPC_INTEROP_PORT": str(occupied_port),
                }
            )

            self.assertNotEqual(proc.returncode, 0, "Script must fail on occupied port")
            self.assertIn(f"port {occupied_port} is already occupied", proc.stderr + proc.stdout)

            # Retained logs must exist
            self.assertTrue(self.log_dir.exists(), "Log directory must exist")
            server_log = self.log_dir / "server-kernel.log"
            self.assertTrue(server_log.exists(), "server-kernel.log must exist")

            results_path = self.log_dir / "results.json"
            self.assertTrue(results_path.exists(), "results.json must exist")
            report_path = self.log_dir / "report.json"
            self.assertTrue(report_path.exists(), "report.json must exist")
            with report_path.open() as f:
                report_data = json.load(f)
            self.assertEqual(report_data.get("overall_status"), "failed")
        finally:
            sock.close()

    def test_case_timeout_hang(self):
        """A hanging client must be terminated by the deadline without hanging CI forever."""
        mock_server = self.create_mock_server()
        # Mock client that hangs for 60 seconds
        hanging_client = self.create_mock_client(exit_code=0, message="Never reached", delay=60.0)

        t_start = time.time()
        proc = self.run_interop_script(
            env_overrides={
                "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                "GRPC_INTEROP_KERNEL_CLIENT": str(hanging_client),
                "GRPC_INTEROP_CASE_TIMEOUT": "1",
                "GRPC_INTEROP_MAX_ATTEMPTS": "1",
            },
            timeout=10.0,
        )
        elapsed = time.time() - t_start

        # Must finish in bounded time (well under 10 seconds)
        self.assertLess(elapsed, 8.0, f"Hanging client must be bounded by timeout (took {elapsed:.2f}s)")
        self.assertNotEqual(proc.returncode, 0, "Script must fail on case timeout")

        # Check retained attempt log
        case_log = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt1.log"
        self.assertTrue(case_log.exists(), "Attempt log must be retained for timed-out case")
        self.assertIn("timed out after 1", case_log.read_text())

        # Check results and report
        results_path = self.log_dir / "results.json"
        self.assertTrue(results_path.exists())
        with results_path.open() as f:
            results_data = json.load(f)
        case_res = next(r for r in results_data["results"] if r["case"] == "empty_unary")
        self.assertEqual(case_res["status"], "failed")
        self.assertEqual(case_res["exit_code"], 124)

        report_path = self.log_dir / "report.json"
        self.assertTrue(report_path.exists())
        with report_path.open() as f:
            report_data = json.load(f)
        self.assertEqual(report_data.get("overall_status"), "failed")

    def test_client_nonzero_exit(self):
        """A client that exits with non-zero must retain stderr/stdout and fail the report."""
        mock_server = self.create_mock_server()
        failing_client = self.create_mock_client(
            exit_code=2,
            message="AssertionError: expected status OK but got INTERNAL",
        )

        proc = self.run_interop_script(
            env_overrides={
                "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                "GRPC_INTEROP_KERNEL_CLIENT": str(failing_client),
                "GRPC_INTEROP_MAX_ATTEMPTS": "1",
            }
        )

        self.assertNotEqual(proc.returncode, 0, "Script must fail on non-zero exit")

        # Check retained log
        case_log = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt1.log"
        self.assertTrue(case_log.exists(), "Attempt log must be retained")
        self.assertIn("AssertionError: expected status OK but got INTERNAL", case_log.read_text())

        # Check report
        results_path = self.log_dir / "results.json"
        with results_path.open() as f:
            results_data = json.load(f)
        case_res = next(r for r in results_data["results"] if r["case"] == "empty_unary")
        self.assertEqual(case_res["status"], "failed")
        self.assertEqual(case_res["exit_code"], 2)

        report_path = self.log_dir / "report.json"
        with report_path.open() as f:
            report_data = json.load(f)
        self.assertEqual(report_data.get("overall_status"), "failed")

    def test_successful_mock_execution(self):
        """A successful mock execution must create structured logs and report status passed."""
        mock_server = self.create_mock_server()
        mock_client = self.create_mock_client(exit_code=0, message="SUCCESSFUL_MOCK_RUN")

        proc = self.run_interop_script(
            env_overrides={
                "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                "GRPC_INTEROP_KERNEL_CLIENT": str(mock_client),
            }
        )

        self.assertEqual(proc.returncode, 0, f"Script failed with output:\n{proc.stdout}\n{proc.stderr}")

        case_log = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt1.log"
        self.assertTrue(case_log.exists())
        self.assertIn("SUCCESSFUL_MOCK_RUN", case_log.read_text())

        results_path = self.log_dir / "results.json"
        with results_path.open() as f:
            results_data = json.load(f)
        case_res = next(r for r in results_data["results"] if r["case"] == "empty_unary")
        self.assertEqual(case_res["status"], "passed")
        self.assertEqual(case_res["exit_code"], 0)

        report_path = self.log_dir / "report.json"
        with report_path.open() as f:
            report_data = json.load(f)
        self.assertEqual(report_data.get("overall_status"), "passed")
        self.assertTrue(report_data.get("overall_passed"))

    def test_child_pids_cleaned_up_on_exit(self):
        """Background server process must be terminated when interop script exits."""
        pid_file = self.work_dir / "server.pid"
        extra_server_code = f"""
import os
with open('{pid_file}', 'w') as f:
    f.write(str(os.getpid()))
"""
        mock_server = self.create_mock_server(extra_code=extra_server_code)
        mock_client = self.create_mock_client(exit_code=0, message="DONE")

        proc = self.run_interop_script(
            env_overrides={
                "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                "GRPC_INTEROP_KERNEL_CLIENT": str(mock_client),
            }
        )
        self.assertEqual(proc.returncode, 0)
        self.assertTrue(pid_file.exists(), "PID file must have been written by mock server")
        server_pid = int(pid_file.read_text().strip())

        # Check whether server_pid is still alive
        is_alive = False
        try:
            os.kill(server_pid, 0)
            is_alive = True
        except OSError:
            is_alive = False

        self.assertFalse(is_alive, f"Server process {server_pid} was not cleaned up on exit!")

    def test_flaky_first_attempt_visible_and_fails_gate(self):
        """A retry pass after a first-attempt failure must stay visible and fail the gate."""
        state_file = self.work_dir / "flaky_state.txt"
        flaky_client = self.create_executable_script(
            "flaky_client.py",
            f"""#!/usr/bin/env python3
import sys
from pathlib import Path
state = Path("{state_file}")
n = int(state.read_text()) if state.exists() else 0
state.write_text(str(n + 1))
if n == 0:
    sys.stderr.write("FLAKY: transient failure on first attempt\\n")
    sys.exit(1)
print("FLAKY: recovered on attempt 2", flush=True)
""",
        )
        mock_server = self.create_mock_server()

        proc = self.run_interop_script(
            env_overrides={
                "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                "GRPC_INTEROP_KERNEL_CLIENT": str(flaky_client),
                "GRPC_INTEROP_MAX_ATTEMPTS": "2",
            },
            timeout=60.0,
        )

        self.assertNotEqual(proc.returncode, 0, "A flaky pass must not satisfy the required gate")

        # Both attempt logs must be retained, showing the first failure.
        att1 = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt1.log"
        att2 = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt2.log"
        self.assertTrue(att1.exists(), "First-attempt log must be retained")
        self.assertTrue(att2.exists(), "Second-attempt log must be retained")
        self.assertIn("FLAKY: transient failure", att1.read_text())

        # The recorded result must expose the retry, not hide it.
        with (self.log_dir / "results.json").open() as f:
            results_data = json.load(f)
        case_res = next(r for r in results_data["results"] if r["case"] == "empty_unary")
        self.assertEqual(case_res["attempt_count"], 2)
        self.assertEqual(case_res["first_attempt_status"], "failed")
        self.assertTrue(case_res["is_flaky"])

        # The aggregated report must fail closed on the hidden first failure.
        with (self.log_dir / "report.json").open() as f:
            report_data = json.load(f)
        self.assertEqual(report_data.get("overall_status"), "failed")
        self.assertFalse(report_data.get("overall_passed"))
        self.assertTrue(
            any("Retry hid" in r or "flaky" in r.lower() for r in report_data.get("failure_reasons", [])),
            f"Report must cite the retried failure: {report_data.get('failure_reasons')}",
        )

    def test_interruption_retains_report_and_cleans_up(self):
        """SIGINT mid-run must retain logs/results/report, stop owned servers, exit nonzero."""
        pid_file = self.work_dir / "server.pid"
        extra_server_code = f"""
import os
with open('{pid_file}', 'w') as f:
    f.write(str(os.getpid()))
"""
        mock_server = self.create_mock_server(extra_code=extra_server_code)
        hanging_client = self.create_mock_client(exit_code=0, message="Never reached", delay=25.0)

        cmd = [str(SCRIPT_PATH), "--self-only"]
        env = os.environ.copy()
        env.update(
            {
                "SKIP_BUILD": "1",
                "GRPC_INTEROP_SKIP_BUILD": "1",
                "GRPC_INTEROP_LOG_DIR": str(self.log_dir),
                "GRPC_INTEROP_CASES": "empty_unary",
                "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                "GRPC_INTEROP_KERNEL_CLIENT": str(hanging_client),
                "GRPC_INTEROP_MAX_ATTEMPTS": "1",
            }
        )
        proc = subprocess.Popen(
            cmd,
            cwd=str(REPO_ROOT),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
        try:
            # Wait until the case attempt is in flight (log created, no record yet).
            attempt_log = self.log_dir / "pbrs-grpc-kernel_client_to_kernel_server-empty_unary-attempt1.log"
            deadline = time.time() + 15.0
            while time.time() < deadline:
                if attempt_log.exists():
                    break
                if proc.poll() is not None:
                    out, err = proc.communicate()
                    self.fail(f"Script exited early with code {proc.returncode}:\n{out}\n{err}")
                time.sleep(0.05)
            self.assertTrue(attempt_log.exists(), "Timed out waiting for in-flight case attempt")
            time.sleep(0.3)

            os.killpg(proc.pid, signal.SIGINT)
            try:
                proc.communicate(timeout=30.0)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.communicate(timeout=10.0)
                self.fail("Interop script did not exit within 30s of SIGINT")
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()

        self.assertNotEqual(proc.returncode, 0, "Interrupted run must exit nonzero")

        # Every failure must have a retained report.
        self.assertTrue((self.log_dir / "server-kernel.log").exists(), "server-kernel.log must be retained")
        self.assertTrue(attempt_log.exists(), "In-flight attempt log must be retained")
        results_path = self.log_dir / "results.json"
        self.assertTrue(results_path.exists(), "results.json must exist after interruption")
        report_path = self.log_dir / "report.json"
        self.assertTrue(report_path.exists(), "report.json must be retained after interruption")
        with report_path.open() as f:
            report_data = json.load(f)
        self.assertEqual(report_data.get("overall_status"), "failed")
        self.assertFalse(report_data.get("overall_passed"))

        # The owned server must not leak.
        self.assertTrue(pid_file.exists(), "PID file must have been written by mock server")
        server_pid = int(pid_file.read_text().strip())
        with self.assertRaises(OSError, msg=f"Server process {server_pid} leaked after SIGINT"):
            os.kill(server_pid, 0)

    def test_concurrent_runs_do_not_share_ports_or_logs(self):
        """Two concurrent runs must bind distinct ports and keep each other's files intact."""
        mock_server = self.create_mock_server()
        mock_client = self.create_mock_client(exit_code=0, message="CONCURRENT_OK")

        def launch(tag: str) -> tuple[subprocess.Popen, Path]:
            run_log_dir = self.work_dir / f"logs-{tag}"
            cmd = [str(SCRIPT_PATH), "--self-only", "--log-dir", str(run_log_dir)]
            env = os.environ.copy()
            env.update(
                {
                    "SKIP_BUILD": "1",
                    "GRPC_INTEROP_SKIP_BUILD": "1",
                    "GRPC_INTEROP_CASES": "empty_unary",
                    "GRPC_INTEROP_KERNEL_SERVER": str(mock_server),
                    "GRPC_INTEROP_KERNEL_CLIENT": str(mock_client),
                }
            )
            # NOTE: no GRPC_INTEROP_PORT override; each run must pick a free dynamic port.
            handle = subprocess.Popen(
                cmd,
                cwd=str(REPO_ROOT),
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            return handle, run_log_dir

        proc_a, dir_a = launch("a")
        proc_b, dir_b = launch("b")
        out_a, err_a = proc_a.communicate(timeout=90.0)
        out_b, err_b = proc_b.communicate(timeout=90.0)

        self.assertEqual(proc_a.returncode, 0, f"Concurrent run A failed:\n{out_a}\n{err_a}")
        self.assertEqual(proc_b.returncode, 0, f"Concurrent run B failed:\n{out_b}\n{err_b}")

        # Neither run may delete or corrupt the other's evidence.
        ports = []
        for run_dir in (dir_a, dir_b):
            results_path = run_dir / "results.json"
            report_path = run_dir / "report.json"
            self.assertTrue(results_path.exists(), f"{run_dir}: results.json must exist")
            self.assertTrue(report_path.exists(), f"{run_dir}: report.json must exist")
            with report_path.open() as f:
                report_data = json.load(f)
            self.assertEqual(report_data.get("overall_status"), "passed")
            with results_path.open() as f:
                results_data = json.load(f)
            self.assertTrue(
                any(r["case"] == "empty_unary" and r["status"] == "passed" for r in results_data["results"]),
                f"{run_dir}: own passing record must be intact",
            )
            match = re.search(
                r"listening on 127\.0\.0\.1:(\d+)",
                (run_dir / "server-kernel.log").read_text(),
            )
            self.assertIsNotNone(match, f"{run_dir}: server port must be logged")
            ports.append(match.group(1))

        self.assertNotEqual(ports[0], ports[1], "Concurrent runs must bind distinct dynamic ports")


if __name__ == "__main__":
    unittest.main()
