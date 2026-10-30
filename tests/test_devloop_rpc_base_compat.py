"""The perf compatibility patch touches only the recognized fixture."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/devloop-rpc-base-compat.py"
spec = importlib.util.spec_from_file_location("rpc_base_compat", SCRIPT)
compat = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compat)


class BaseFixtureCompat(unittest.TestCase):
    def test_recognized_and_unknown_source(self):
        original = b'''impl Inputs {\nSpecimen::Maps(n) => Self::Maps(Pair::prepare(\n                maps(n),\nself.native_bytes, self.bytes\n'''
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / compat.FILE
            target.parent.mkdir(parents=True)
            target.write_bytes(original)
            record = root / "record.json"
            head = root / "head"
            head_source = head / compat.FILE
            head_source.parent.mkdir(parents=True)
            head_source.write_text(compat.HELPER + "impl Inputs {\nSpecimen::Maps(n) => Self::Maps(Pair::prepare(\n                rpc_maps(n),\n")
            args = [str(SCRIPT), str(root), str(record), "--head", str(head)]
            with patch.object(compat, "LEGACY_SHA256", hashlib.sha256(original).hexdigest()), patch.object(sys, "argv", args), contextlib.redirect_stdout(io.StringIO()):
                compat.main()
                result = json.loads(record.read_text())
                self.assertTrue(result["applied"])
                self.assertEqual(result["before_sha256"], hashlib.sha256(original).hexdigest())
                patched = target.read_text()
                self.assertIn("fn rpc_maps(entries: usize)", patched)
                self.assertIn("Specimen::Maps(n) => Self::Maps(Pair::prepare(\n                rpc_maps(n),", patched)
                self.assertIn("self.native_bytes, self.bytes", patched)
                self.assertEqual(result["after_sha256"], hashlib.sha256(target.read_bytes()).hexdigest())
                compat.main()
                self.assertFalse(json.loads(record.read_text())["applied"])
                self.assertEqual(patched, target.read_text())
                unsupported = original + b"// different source version\n"
                target.write_bytes(unsupported)
                compat.main()
                self.assertFalse(json.loads(record.read_text())["applied"])
                self.assertEqual(unsupported, target.read_bytes())
                target.write_bytes(original)
                head_source.write_text("a different fixture")
                with self.assertRaisesRegex(ValueError, "head RPC fixture differs"):
                    compat.main()
                self.assertEqual(original, target.read_bytes())


if __name__ == "__main__":
    unittest.main()
