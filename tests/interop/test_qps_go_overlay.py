"""Fail-closed benchmark-only Go overlay preparation and provenance tests."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/qps-go-overlay.py"
spec = importlib.util.spec_from_file_location("qps_go_overlay", SCRIPT)
overlay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(overlay)


class GoOverlayTest(unittest.TestCase):
    def test_source_drift_fails_before_creating_an_overlay(self):
        with tempfile.TemporaryDirectory() as directory:
            module = Path(directory) / "module"
            source = module / "benchmark/worker/benchmark_client.go"
            source.parent.mkdir(parents=True)
            source.write_text("different upstream source")
            output = Path(directory) / "output"
            with self.assertRaisesRegex(ValueError, "source hash drift"):
                overlay.prepare(module, output)
            self.assertFalse(output.exists())

    def test_overlay_keeps_upstream_source_and_checks_every_source_and_binary(self):
        # A small structural fixture tests preparation without fetching a module.
        # The actual module's SHA is checked separately by the build and evidence.
        source_text = ("type benchmarkClient struct {\n}\n"
                       "\tswitch config.RpcType {\n}\n"
                       "func (bc *benchmarkClient) getStats(reset bool) *testpb.ClientStats {\n}\n")
        with tempfile.TemporaryDirectory() as directory:
            module = Path(directory) / "module"
            source = module / "benchmark/worker/benchmark_client.go"
            source.parent.mkdir(parents=True)
            source.write_text(source_text)
            (module / "transport.go").write_text("unchanged transport source")
            output = Path(directory) / "output"
            with patch.object(overlay, "CLIENT_SHA256", hashlib.sha256(source.read_bytes()).hexdigest()), \
                 patch.object(overlay, "MODULE_TREE_SHA256", overlay.tree_digest(module)):
                manifest = overlay.prepare(module, output)
                self.assertEqual(source.read_text(), source_text)
                self.assertEqual(manifest["qualification"], "benchmark harness overlay")
                self.assertFalse(manifest["claim_eligible"])
                replacements = json.loads((output / "overlay.json").read_text())["Replace"]
                self.assertTrue(all(str(output) in source for source in replacements))
                binary = Path(directory) / "worker"
                binary.write_bytes(b"pinned harness worker")
                manifest["binary_sha256"] = overlay.digest(binary)
                (output / "worker.mod").write_text("fixture pinned dependencies")
                (output / "worker.sum").write_text("fixture locked checksums")
                manifest["dependencies_mod_sha256"] = overlay.digest(output / "worker.mod")
                manifest["dependencies_sum_sha256"] = overlay.digest(output / "worker.sum")
                (output / "manifest.json").write_text(json.dumps(manifest))
                overlay.verify(output, binary)
                for path in (binary, output / "benchmark_client.go",
                             output / "grpc-go-source/transport.go", output / "overlay.json", output / "worker.mod"):
                    original = path.read_bytes()
                    path.write_bytes(original + b" drift")
                    with self.subTest(path=path), self.assertRaisesRegex(ValueError, "provenance drift"):
                        overlay.verify(output, binary)
                    path.write_bytes(original)


if __name__ == "__main__":
    unittest.main()
