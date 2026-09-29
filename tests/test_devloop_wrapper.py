"""Keep build failures and regressed measurements recoverable in CI."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/devloop.sh"


class WrapperTests(unittest.TestCase):
    def test_build_failure_writes_error_report_in_new_output_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = root / "bin"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text("#!/bin/sh\nif [ \"$1\" = fetch ]; then exit 0; fi\necho 'error: synthetic build failure' >&2\nexit 1\n")
            cargo.chmod(0o755)
            out = root / "artifacts" / "head.json"
            env = {
                **os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                "PBRS_DEVLOOP_ROOT": str(root), "PBRS_DEVLOOP_COMMIT": "a" * 40,
            }
            run = subprocess.run(["bash", str(SCRIPT), "--out", str(out)], env=env,
                                 capture_output=True, text=True)
            self.assertEqual(run.returncode, 1)
            report = json.loads(out.read_text())
            self.assertEqual(report["devloop_commit"], "a" * 40)
            self.assertEqual(report["cells"], [])
            self.assertIn("synthetic build failure", report["error"])

    def test_compare_regression_preserves_current_report_and_exit_code(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = root / "bin"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text("#!/bin/sh\nexit 0\n")
            cargo.chmod(0o755)
            target = root / "custom-target"
            (target / "release").mkdir(parents=True)
            binary = target / "release" / "devloop"
            binary.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = compare ]; then exit 1; fi\n"
                "while [ $# -gt 0 ]; do\n"
                "  if [ \"$1\" = --out ]; then printf '%s\\n' '{\"measurement\":42}' > \"$2\"; exit 0; fi\n"
                "  shift\n"
                "done\nexit 2\n"
            )
            binary.chmod(0o755)
            out = root / "head.json"
            env = {
                **os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                "PBRS_DEVLOOP_ROOT": str(root), "CARGO_TARGET_DIR": str(target),
            }
            run = subprocess.run([
                "bash", str(SCRIPT), "--baseline", str(root / "base.json"), "--out", str(out)
            ], env=env, capture_output=True, text=True)
            self.assertEqual(run.returncode, 1, run.stderr)
            self.assertEqual(json.loads(out.read_text()), {"measurement": 42})


if __name__ == "__main__":
    unittest.main()
