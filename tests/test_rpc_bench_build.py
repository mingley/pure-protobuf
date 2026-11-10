"""Exercise source-pinned RPC benchmark builds and their feature records."""
import importlib.util
import json
from pathlib import Path
import subprocess
import shutil
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "rpc_bench_build", Path(__file__).resolve().parents[1] / "scripts/build-rpc-bench.py")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)

INSTRUMENTED_COMMAND = ["cargo", "build", "--locked", "--release", "--manifest-path",
                        "rpc-bench/Cargo.toml", "--features", "allocation-counts", "--message-format=json"]
PLAIN_COMMAND = ["cargo", "build", "--locked", "--release", "--manifest-path",
                 "rpc-bench/Cargo.toml", "--no-default-features", "--message-format=json"]


class BuildRecordTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / "rpc-bench"
        self.binary.write_bytes(b"frozen executable")
        self.executable = self.root / "cargo-output"
        self.executable.write_bytes(self.binary.read_bytes())
        self.entry = {"reason": "compiler-artifact", "executable": str(self.executable),
                      "manifest_path": str(BUILD.ROOT / "rpc-bench/Cargo.toml"),
                      "target": {"name": "rpc-bench", "kind": ["bin"]}}
        (self.root / "build.stderr.log").write_text("Finished release\n")
        self.pin = {"commit": "a" * 40, "tree": "b" * 40,
                    "locks": {"Cargo.lock": "c" * 64, "rpc-bench/Cargo.lock": "d" * 64}}
        self.record = {"schema": "pbrs.rpc-bench-build.v2", "source": self.pin,
                       "exit_code": 0, "build_executable": str(self.executable),
                       "binary_sha256": BUILD.digest(self.binary),
                       "tools": {"rustc": "rustc 1.88", "cargo": "cargo 1.88"}}
        self.path = self.root / "build.json"
        self.set_mode(True)

    def write_entry(self):
        (self.root / "build.stdout.jsonl").write_text(json.dumps(self.entry) + "\n")
        self.record["files"] = {name: BUILD.digest(self.root / name)
                                for name in ("build.stdout.jsonl", "build.stderr.log")}

    def set_mode(self, enabled):
        features = ["allocation-counts", "default"] if enabled else []
        self.entry["features"] = features
        self.record.update(command=(INSTRUMENTED_COMMAND if enabled else PLAIN_COMMAND).copy(), allocation_counts=enabled,
                           artifact_features=features.copy())
        self.write_entry()

    def validate(self):
        self.path.write_text(json.dumps(self.record))
        with patch.object(BUILD, "source_pin", return_value=self.pin):
            return BUILD.validate_record(self.path, self.binary)

    def test_matching_instrumented_and_plain_records(self):
        for enabled in (True, False):
            with self.subTest(enabled=enabled):
                self.set_mode(enabled)
                self.assertEqual(self.validate(), self.record)
                self.assertIs(BUILD.validate_feature_record(self.record, self.path), enabled)

    def test_legacy_instrumented_features_remain_supported(self):
        self.record["schema"] = "pbrs.rpc-bench-build.v1"
        del self.record["allocation_counts"]
        del self.record["artifact_features"]
        self.assertTrue(BUILD.validate_feature_record(self.record, self.path))

    def test_new_builder_rejects_a_downgraded_record(self):
        self.record["schema"] = "pbrs.rpc-bench-build.v1"
        del self.record["allocation_counts"]
        del self.record["artifact_features"]
        with self.assertRaisesRegex(ValueError, "requires a v2 build record"):
            self.validate()

    def test_legacy_schema_cannot_describe_plain_or_new_metadata(self):
        self.record["schema"] = "pbrs.rpc-bench-build.v1"
        with self.assertRaisesRegex(ValueError, "invalid legacy build feature metadata"):
            BUILD.validate_feature_record(self.record, self.path)
        self.set_mode(False)
        self.record["schema"] = "pbrs.rpc-bench-build.v1"
        del self.record["allocation_counts"]
        del self.record["artifact_features"]
        with self.assertRaisesRegex(ValueError, "invalid legacy build feature metadata"):
            BUILD.validate_feature_record(self.record, self.path)

    def test_new_records_reject_extra_package_features(self):
        for enabled, features in ((False, ["unexpected"]),
                                  (True, ["allocation-counts", "default", "unexpected"])):
            with self.subTest(enabled=enabled):
                self.set_mode(enabled)
                self.entry["features"] = features
                self.record["artifact_features"] = features.copy()
                self.write_entry()
                with self.assertRaisesRegex(ValueError, "artifact features disagree"):
                    self.validate()

    def test_instrumented_default_feature_is_optional(self):
        self.entry["features"] = ["allocation-counts"]
        self.record["artifact_features"] = ["allocation-counts"]
        self.write_entry()
        self.assertEqual(self.validate(), self.record)

    def test_legacy_record_preserves_pinned_manifest_default_features(self):
        self.record["schema"] = "pbrs.rpc-bench-build.v1"
        del self.record["allocation_counts"]
        del self.record["artifact_features"]
        self.entry["features"] = ["allocation-counts", "default", "older-default"]
        self.write_entry()
        self.assertTrue(BUILD.validate_feature_record(self.record, self.path))

    def test_plain_record_requires_explicit_feature_metadata(self):
        self.set_mode(False)
        del self.record["allocation_counts"]
        del self.record["artifact_features"]
        with self.assertRaisesRegex(ValueError, "incomplete build feature metadata"):
            self.validate()

    def test_cargo_features_must_match_the_exact_command(self):
        for enabled in (True, False):
            with self.subTest(enabled=enabled):
                self.set_mode(enabled)
                self.entry["features"] = [] if enabled else ["allocation-counts"]
                self.write_entry()
                with self.assertRaisesRegex(ValueError, "artifact features disagree"):
                    self.validate()

    def test_feature_metadata_cannot_contradict_or_partially_describe_artifact(self):
        for change in ({"allocation_counts": False}, {"allocation_counts": 1},
                       {"artifact_features": ["default"]}):
            with self.subTest(change=change):
                self.set_mode(True)
                self.record.update(change)
                with self.assertRaisesRegex(ValueError, "feature metadata"):
                    self.validate()
        for missing in ("allocation_counts", "artifact_features"):
            with self.subTest(missing=missing):
                self.set_mode(True)
                del self.record[missing]
                with self.assertRaisesRegex(ValueError, "incomplete build feature metadata"):
                    self.validate()

    def test_missing_invalid_or_duplicate_artifact_features(self):
        for features in (None, "allocation-counts", ["allocation-counts", "allocation-counts"], [1]):
            with self.subTest(features=features):
                self.set_mode(True)
                self.entry["features"] = features
                self.write_entry()
                with self.assertRaisesRegex(ValueError, "valid artifact features"):
                    self.validate()

    def test_binary_source_logs_and_command_remain_pinned(self):
        self.binary.write_bytes(b"different executable")
        with self.assertRaisesRegex(ValueError, "binary differs"):
            self.validate()
        self.binary.write_bytes(self.executable.read_bytes())
        self.record["source"] = {**self.pin, "commit": "e" * 40}
        with self.assertRaisesRegex(ValueError, "build source differs"):
            self.validate()
        self.record["source"] = self.pin
        (self.root / "build.stderr.log").write_text("edited\n")
        with self.assertRaisesRegex(ValueError, "build log changed"):
            self.validate()
        self.write_entry()
        for key, value in (("exit_code", 1), ("command", ["cargo", "test"]), ("tools", {})):
            with self.subTest(key=key), patch.dict(self.record, {key: value}), self.assertRaises(ValueError):
                self.validate()

    def test_missing_or_ambiguous_cargo_executable(self):
        for messages in ([], [self.entry, self.entry]):
            with self.subTest(messages=messages), self.assertRaises(ValueError):
                BUILD.artifact(messages)

    def test_builder_selects_and_records_both_modes(self):
        for enabled in (True, False):
            with self.subTest(enabled=enabled):
                output = self.root / ("instrumented" if enabled else "plain")
                features = ["allocation-counts", "default"] if enabled else []
                entry = {**self.entry, "features": features}

                def run(command, *, cwd, stdout, stderr, check):
                    self.assertEqual(command, INSTRUMENTED_COMMAND if enabled else PLAIN_COMMAND)
                    self.assertEqual(cwd, BUILD.ROOT)
                    self.assertFalse(check)
                    stdout.write(json.dumps(entry) + "\n")
                    stderr.write("Finished release\n")
                    return SimpleNamespace(returncode=0)

                argv = ["build-rpc-bench.py", "--source", self.pin["commit"], "--output", str(output)]
                if not enabled:
                    argv.append("--no-allocation-counts")
                with patch("sys.argv", argv), patch.object(BUILD, "source_pin", return_value=self.pin), \
                        patch.object(BUILD.subprocess, "run", side_effect=run), \
                        patch.object(BUILD.subprocess, "check_output", return_value="tool 1.0"):
                    self.assertEqual(BUILD.main(), 0)
                record = json.loads((output / "build.json").read_text())
                self.assertEqual(record["schema"], "pbrs.rpc-bench-build.v2")
                self.assertIs(record["allocation_counts"], enabled)
                self.assertEqual(record["artifact_features"], features)
                self.assertEqual(record["command"], INSTRUMENTED_COMMAND if enabled else PLAIN_COMMAND)
                self.assertEqual((output / "rpc-bench").read_bytes(), self.executable.read_bytes())

    def test_builder_rejects_unexpected_compiled_features(self):
        output = self.root / "bad-build"

        def run(command, *, cwd, stdout, stderr, check):
            stdout.write(json.dumps({**self.entry, "features": ["allocation-counts"]}) + "\n")
            stderr.write("Finished release\n")
            return SimpleNamespace(returncode=0)

        argv = ["build-rpc-bench.py", "--source", self.pin["commit"], "--output", str(output),
                "--no-allocation-counts"]
        with patch("sys.argv", argv), patch.object(BUILD, "source_pin", return_value=self.pin), \
                patch.object(BUILD.subprocess, "run", side_effect=run), \
                self.assertRaisesRegex(ValueError, "artifact features disagree"):
            BUILD.main()
        self.assertTrue((output / "build.stdout.jsonl").exists())
        self.assertFalse((output / "build.json").exists())

    def test_dirty_output_fails_before_cargo_but_ignored_and_external_outputs_work(self):
        repo = self.root / "checkout"
        repo.mkdir()
        (repo / "rpc-bench").mkdir()
        (repo / "Cargo.lock").write_text("root lock\n")
        (repo / "rpc-bench/Cargo.lock").write_text("benchmark lock\n")
        (repo / ".gitignore").write_text("/target/\n")
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Test",
                        "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false",
                        "commit", "-qm", "fixture"], check=True)
        commit = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
        real_run = subprocess.run
        real_check_output = subprocess.check_output

        for name, output, should_pass in (
                ("unignored", repo / "work/build", False),
                ("ignored", repo / "target/build", True),
                ("external", self.root / "external-build", True)):
            with self.subTest(output=name):
                # Remove the untracked failed attempt before checking the other paths.
                if name == "ignored":
                    import shutil
                    shutil.rmtree(repo / "work")
                cargo_calls = []

                def run(command, *args, **kwargs):
                    if command[0] != "cargo":
                        return real_run(command, *args, **kwargs)
                    cargo_calls.append(command)
                    entry = {**self.entry, "manifest_path": str(repo / "rpc-bench/Cargo.toml"),
                             "features": ["allocation-counts", "default"]}
                    kwargs["stdout"].write(json.dumps(entry) + "\n")
                    kwargs["stderr"].write("Finished release\n")
                    return SimpleNamespace(returncode=0)

                def check_output(command, *args, **kwargs):
                    if command[0] in ("cargo", "rustc"):
                        return "tool 1.0"
                    return real_check_output(command, *args, **kwargs)

                argv = ["build-rpc-bench.py", "--source", commit, "--output", str(output)]
                with patch("sys.argv", argv), patch.object(BUILD, "ROOT", repo), \
                        patch.object(BUILD.subprocess, "run", side_effect=run), \
                        patch.object(BUILD.subprocess, "check_output", side_effect=check_output):
                    if should_pass:
                        self.assertEqual(BUILD.main(), 0)
                        self.assertEqual(len(cargo_calls), 1)
                    else:
                        with self.assertRaisesRegex(ValueError, "ignored or external output"):
                            BUILD.main()
                        self.assertEqual(cargo_calls, [])
                        self.assertTrue((output / "build.stdout.jsonl").exists())


@unittest.skipUnless(shutil.which("git"), "requires Git")
class SourcePinIndexTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "checkout"
        self.root.mkdir()
        (self.root / "rpc-bench").mkdir()
        (self.root / "excluded").mkdir()
        (self.root / "Cargo.lock").write_text("root lock\n")
        (self.root / "rpc-bench/Cargo.lock").write_text("benchmark lock\n")
        (self.root / "tracked.txt").write_text("original\n")
        (self.root / "excluded/hidden.txt").write_text("original\n")
        # Paths with embedded newlines exercise the NUL-delimited index parser.
        (self.root / "odd\nname.txt").write_text("original\n")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Benchmark Test",
                 "-c", "user.email=benchmark-test@example.invalid",
                 "-c", "commit.gpgsign=false",
                 "-c", f"core.hooksPath={self.root / '.git' / 'empty-hooks'}", "commit", "-qm", "fixture")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root)

    def source_pin(self):
        with patch.object(BUILD, "ROOT", self.root):
            return BUILD.source_pin()

    def test_clean_index_and_odd_filenames_are_accepted(self):
        pin = self.source_pin()
        self.assertEqual(pin["commit"], self.git("rev-parse", "HEAD").decode().strip())
        self.assertEqual(pin["tree"], self.git("rev-parse", "HEAD^{tree}").decode().strip())
        self.assertEqual(set(pin["locks"]), {"Cargo.lock", "rpc-bench/Cargo.lock"})

    def test_untracked_and_normal_tracked_changes_remain_rejected(self):
        (self.root / "untracked.txt").write_text("new\n")
        with self.assertRaisesRegex(ValueError, "must be clean"):
            self.source_pin()
        (self.root / "untracked.txt").unlink()
        (self.root / "tracked.txt").write_text("changed\n")
        with self.assertRaisesRegex(ValueError, "must be clean"):
            self.source_pin()

    def test_assume_unchanged_cannot_hide_tracked_edits(self):
        self.git("update-index", "--assume-unchanged", "--", "tracked.txt")
        (self.root / "tracked.txt").write_text("changed but hidden\n")
        # The former status-only check accepted this real Git state.
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")
        with self.assertRaisesRegex(ValueError, "assume-unchanged or skip-worktree"):
            self.source_pin()
        self.git("update-index", "--no-assume-unchanged", "--", "tracked.txt")
        with self.assertRaisesRegex(ValueError, "must be clean"):
            self.source_pin()

    def test_skip_worktree_cannot_hide_tracked_edits(self):
        self.git("update-index", "--skip-worktree", "--", "tracked.txt")
        (self.root / "tracked.txt").write_text("changed but hidden\n")
        # The former status-only check accepted this real Git state.
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")
        with self.assertRaisesRegex(ValueError, "assume-unchanged or skip-worktree"):
            self.source_pin()
        self.git("update-index", "--no-skip-worktree", "--", "tracked.txt")
        with self.assertRaisesRegex(ValueError, "must be clean"):
            self.source_pin()

    def test_clean_flagged_index_is_rejected_before_later_drift(self):
        for flag, reset in (("--assume-unchanged", "--no-assume-unchanged"),
                            ("--skip-worktree", "--no-skip-worktree")):
            with self.subTest(flag=flag):
                self.git("update-index", flag, "--", "tracked.txt")
                with self.assertRaisesRegex(ValueError, "hidden or unmerged"):
                    self.source_pin()
                self.git("update-index", reset, "--", "tracked.txt")
                self.source_pin()

    def test_sparse_checkout_with_skipped_paths_is_rejected(self):
        self.git("sparse-checkout", "init", "--cone")
        self.git("sparse-checkout", "set", "rpc-bench")
        self.assertFalse((self.root / "excluded/hidden.txt").exists())
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")
        with self.assertRaisesRegex(ValueError, "skip-worktree"):
            self.source_pin()


    def test_sparse_index_with_skipped_paths_is_rejected(self):
        try:
            self.git("sparse-checkout", "init", "--cone", "--sparse-index")
        except subprocess.CalledProcessError:
            self.skipTest("Git lacks sparse-index support")
        self.git("sparse-checkout", "set", "rpc-bench")
        self.assertEqual(self.git("config", "--get", "index.sparse").strip(), b"true")
        self.assertIn(b"S excluded/\0", self.git("ls-files", "--sparse", "-v", "-z"))
        self.assertFalse((self.root / "excluded/hidden.txt").exists())
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")
        with self.assertRaisesRegex(ValueError, "skip-worktree"):
            self.source_pin()


if __name__ == "__main__":
    unittest.main()
