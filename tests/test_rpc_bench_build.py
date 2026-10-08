"""Reject mismatched executable, source and build records."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "rpc_bench_build", Path(__file__).resolve().parents[1] / "scripts/build-rpc-bench.py")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)


class BuildRecordTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / "rpc-bench"
        self.binary.write_bytes(b"frozen executable")
        executable = str(self.root / "cargo-output")
        entry = {"reason": "compiler-artifact", "executable": executable,
                 "manifest_path": str(BUILD.ROOT / "rpc-bench/Cargo.toml"),
                 "target": {"name": "rpc-bench", "kind": ["bin"]}}
        (self.root / "build.stdout.jsonl").write_text(json.dumps(entry) + "\n")
        (self.root / "build.stderr.log").write_text("Finished release\n")
        self.pin = {"commit": "a" * 40, "tree": "b" * 40,
                    "locks": {"Cargo.lock": "c" * 64, "rpc-bench/Cargo.lock": "d" * 64}}
        self.record = {"schema": "pbrs.rpc-bench-build.v1", "source": self.pin,
                       "command": ["cargo", "build", "--locked", "--release", "--manifest-path",
                                   "rpc-bench/Cargo.toml", "--features", "allocation-counts", "--message-format=json"],
                       "exit_code": 0, "build_executable": executable,
                       "binary_sha256": BUILD.digest(self.binary),
                       "tools": {"rustc": "rustc 1.88", "cargo": "cargo 1.88"},
                       "files": {name: BUILD.digest(self.root / name)
                                 for name in ("build.stdout.jsonl", "build.stderr.log")}}
        self.path = self.root / "build.json"

    def validate(self):
        self.path.write_text(json.dumps(self.record))
        with patch.object(BUILD, "source_pin", return_value=self.pin):
            return BUILD.validate_record(self.path, self.binary)

    def test_matching_record(self):
        self.assertEqual(self.validate(), self.record)

    def test_binary_replacement(self):
        self.binary.write_bytes(b"different executable")
        with self.assertRaises(ValueError):
            self.validate()

    def test_source_mismatch(self):
        self.record["source"] = {**self.pin, "commit": "e" * 40}
        with self.assertRaises(ValueError):
            self.validate()

    def test_failed_or_different_build(self):
        for key, value in [("exit_code", 1), ("command", ["cargo", "test"]), ("tools", {})]:
            with self.subTest(key=key), patch.dict(self.record, {key: value}), self.assertRaises(ValueError):
                self.validate()

    def test_modified_build_log(self):
        (self.root / "build.stderr.log").write_text("edited\n")
        with self.assertRaises(ValueError):
            self.validate()

    def test_missing_artifact(self):
        with self.assertRaises(ValueError):
            BUILD.artifact([])


if __name__ == "__main__":
    unittest.main()
