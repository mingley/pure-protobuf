"""A successful client result cannot replace missing local HTTP/2 peer evidence."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/grpc-http2-interop.sh"


@unittest.skipUnless(os.name == "posix", "HTTP/2 shell adapter requires Unix")
class Http2PeerProofTest(unittest.TestCase):
    def test_fake_success_without_peer_frames_is_recorded_as_failure(self):
        with tempfile.TemporaryDirectory(prefix="pbrs-http2-peer-proof-") as directory:
            fake_client = Path(directory) / "client"
            fake_client.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            fake_client.chmod(0o755)
            logs = Path(directory) / "logs"
            env = os.environ.copy()
            env["GRPC_HTTP2_CLIENT"] = str(fake_client)
            run = subprocess.run(
                [
                    str(SCRIPT),
                    "--skip-build",
                    "--cases=ping",
                    f"--log-dir={logs}",
                ],
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
            )
            rows = json.loads((logs / "results.json").read_text(encoding="utf-8"))["results"]
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["case"], "ping")
            self.assertEqual(rows[0]["exit_code"], 0)
            self.assertEqual(rows[0]["status"], "failed")
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("local peer never completed", run.stdout)


if __name__ == "__main__":
    unittest.main()
