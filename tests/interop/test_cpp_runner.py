"""The pinned C++ reference runner must not download unchecked archives."""

import os
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/grpc-interop-cpp.sh"


class CppPeerAcquisitionTest(unittest.TestCase):
    def test_unverified_binary_download_override_is_rejected(self):
        env = os.environ.copy()
        env["GRPC_INTEROP_CPP_DOWNLOAD_URL"] = "https://example.invalid/unverified.tar.gz"
        proc = subprocess.run(
            ["bash", str(RUNNER), "--skip-build", "--cases=empty_unary"],
            cwd=ROOT, env=env, capture_output=True, text=True, timeout=15,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("unverified", proc.stderr)


if __name__ == "__main__":
    unittest.main()
