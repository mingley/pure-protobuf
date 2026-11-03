"""The perf compatibility patch touches only the recognized fixture."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import subprocess
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


class PreparedInputCompat(unittest.TestCase):
    def fixture(self, directory, commit):
        base = directory / "base"
        head = directory / "head"
        subprocess.run(["git", "init", "--quiet", str(base)], check=True, stderr=subprocess.PIPE)
        git_common = Path(subprocess.check_output(
            ["git", "rev-parse", "--git-common-dir"], cwd=ROOT, text=True).strip())
        objects = (ROOT / git_common / "objects").resolve()
        (base / ".git/objects/info/alternates").write_text(str(objects) + "\n")
        subprocess.run(["git", "-C", str(base), "update-ref", "HEAD", commit], check=True)
        for path in compat.HEAD_RUNTIME_PREFIXES:
            original = subprocess.check_output(["git", "show", commit + ":" + path], cwd=ROOT)
            destination = base / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(original)
            candidate = head / path
            candidate.parent.mkdir(parents=True, exist_ok=True)
            candidate.write_bytes((ROOT / path).read_bytes())
        # Only the five benchmark reporting files exist in this Git fixture.
        # No complete baseline checkout or runtime build is claimed here.
        canary = base / "src/runtime-canary.rs"
        canary.parent.mkdir(parents=True)
        canary.write_bytes(b"production source must stay unchanged\n")
        return base, head, directory / "record.json", canary

    def run_overlay(self, base, head, record, commit):
        argv = [str(SCRIPT), str(base), str(record), "--head", str(head), "--base-sha", commit]
        with patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            compat.main()
        return json.loads(record.read_text())

    def snapshot(self, base):
        return {path: (base / path).read_bytes() for path in compat.HEAD_RUNTIME_PREFIXES}

    def test_all_immutable_sources_are_audited_and_idempotent(self):
        for commit, expected in compat.KNOWN_INPUT_BASES.items():
            with self.subTest(commit=commit), tempfile.TemporaryDirectory() as temp:
                base, head, record, canary = self.fixture(Path(temp), commit)
                unchanged = canary.read_bytes()
                result = self.run_overlay(base, head, record, commit)
                self.assertTrue(result["applied"])
                reporting = result["input_reporting"]
                self.assertTrue(reporting["applied"])
                self.assertEqual(reporting["base_commit"], commit)
                self.assertEqual({row["path"]: row["before_sha256"] for row in reporting["files"]}, expected["before"])
                self.assertEqual({row["path"]: row["after_sha256"] for row in reporting["files"]}, expected["after"])
                self.assertEqual(compat.source_hashes(base, compat.HEAD_RUNTIME_PREFIXES), expected["after"])
                self.assertEqual(reporting["patch_sha256"], compat.digest(compat.INPUT_PATCH.read_bytes()))
                self.assertEqual(canary.read_bytes(), unchanged)
                after = self.snapshot(base)
                again = self.run_overlay(base, head, record, commit)
                self.assertFalse(again["applied"])
                self.assertEqual(again["input_reporting"]["reason"], "already applied")
                self.assertEqual(self.snapshot(base), after)

    def test_wrong_commit_or_modified_base_fails_before_mutation(self):
        commit = "a35701582fb9e6964c855386b0d9c991c50345ba"
        with tempfile.TemporaryDirectory() as temp:
            base, head, record, _ = self.fixture(Path(temp), commit)
            before = self.snapshot(base)
            with self.assertRaisesRegex(ValueError, "base commit differs"):
                self.run_overlay(base, head, record, "26389ccf77bf9342fe6e605f1d797853acd6c39a")
            self.assertEqual(self.snapshot(base), before)
            target = base / "bench/devloop/src/main.rs"
            target.write_bytes(target.read_bytes() + b"// unexpected base edit\n")
            modified = self.snapshot(base)
            with self.assertRaisesRegex(ValueError, "pinned snapshot"):
                self.run_overlay(base, head, record, commit)
            self.assertEqual(self.snapshot(base), modified)
            self.assertFalse(record.exists())

    def test_changed_candidate_or_out_of_scope_patch_fails_before_mutation(self):
        commit = "a35701582fb9e6964c855386b0d9c991c50345ba"
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            base, head, record, _ = self.fixture(directory, commit)
            before = self.snapshot(base)
            candidate = head / compat.FILE
            old = candidate.read_bytes()
            candidate.write_bytes(old.replace(b"pub input_wire_fingerprint", b"pub wrong_input_wire_fingerprint", 1))
            with self.assertRaisesRegex(ValueError, "head input reporting differs"):
                self.run_overlay(base, head, record, commit)
            self.assertEqual(self.snapshot(base), before)
            candidate.write_bytes(old)
            altered_patch = directory / "unexpected.patch"
            altered_patch.write_bytes(compat.INPUT_PATCH.read_bytes().replace(
                b"bench/devloop/src/main.rs", b"src/production.rs"))
            with patch.object(compat, "INPUT_PATCH", altered_patch):
                with self.assertRaisesRegex(ValueError, "unexpected file"):
                    self.run_overlay(base, head, record, commit)
            self.assertEqual(self.snapshot(base), before)
            self.assertFalse(record.exists())

    def test_unknown_commit_with_identical_harness_is_left_unchanged(self):
        commit = "a35701582fb9e6964c855386b0d9c991c50345ba"
        with tempfile.TemporaryDirectory() as temp:
            base, head, record, _ = self.fixture(Path(temp), commit)
            tree = subprocess.check_output(["git", "rev-parse", commit + "^{tree}"], cwd=ROOT, text=True).strip()
            # An explicitly fictional local commit exercises immutable-identity
            # selection without changing or signing the real repository.
            body = (f"tree {tree}\nparent {commit}\n"
                    "author Fixture Test <fixture@example.invalid> 1 +0000\n"
                    "committer Fixture Test <fixture@example.invalid> 1 +0000\n\nunknown fixture\n")
            unknown = subprocess.check_output(
                ["git", "-C", str(base), "hash-object", "-t", "commit", "--stdin", "-w"],
                input=body, text=True).strip()
            subprocess.run(["git", "-C", str(base), "update-ref", "HEAD", unknown], check=True)
            self.assertNotIn(unknown, compat.KNOWN_INPUT_BASES)
            before = self.snapshot(base)
            result = self.run_overlay(base, head, record, unknown)
            self.assertFalse(result["applied"])
            self.assertEqual(result["input_reporting"]["reason"], "unknown base commit; normal qualification remains mandatory")
            self.assertEqual(self.snapshot(base), before)


if __name__ == "__main__":
    unittest.main()
