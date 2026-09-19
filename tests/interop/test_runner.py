#!/usr/bin/env python3
"""Unit and integration tests for scripts/grpc-interop.sh test runner.

Exercises mock / fake peer behaviors:
  - Server startup failure (abrupt exit on launch)
  - Occupied port detection (cannot bind address already in use)
  - Case-level timeout / hang (bounded deadline kills hung client/server)
  - Nonzero exit from client (assertion failure or error status)
  - Successful case execution (valid reporting, log retention, exit code 0)
  - Process cleanup and leak prevention (tracked child PIDs terminated cleanly)
  - Unique log directory generation (no overwriting between runs)

Run with:
  python3 -m unittest tests/interop/test_runner.py
"""

from __future__ import annotations

import json
import os
from pathlib import Path
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


if __name__ == "__main__":
    unittest.main()
