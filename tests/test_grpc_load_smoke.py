"""Reject incomplete or unsuccessful endpoint measurements."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "grpc_load_smoke", Path(__file__).resolve().parents[1] / "scripts/grpc-load-smoke.py")
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class AccountingTests(unittest.TestCase):
    def setUp(self):
        self.metrics = {"offered_rpcs": 7, "dispatched_rpcs": 7, "completed_rpcs": 7,
                        "successful_rpcs": 7, "failed_rpcs": 0, "timeouts": 0,
                        "queue_overflows": 0, "unstarted_rpcs": 0, "unfinished_rpcs": 0,
                        "status_errors": {}}

    def test_complete_success(self):
        SMOKE.validate_metrics(self.metrics)

    def test_legacy_report_without_terminal_counts_is_rejected(self):
        for name in ["completed_rpcs", "unstarted_rpcs", "unfinished_rpcs"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                SMOKE.validate_metrics({key: value for key, value in self.metrics.items() if key != name})

    def test_empty_or_partial_success_is_rejected(self):
        for count in [0, 6]:
            with self.subTest(count=count), self.assertRaises(ValueError):
                SMOKE.validate_metrics({**self.metrics, "successful_rpcs": count})

    def test_every_failure_category_is_rejected(self):
        for name in ["failed_rpcs", "timeouts", "queue_overflows", "unstarted_rpcs", "unfinished_rpcs"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                SMOKE.validate_metrics({**self.metrics, name: 1})

    def test_invalid_types_or_negative_counts_are_rejected(self):
        for count in [None, False, -1, "7", 7.0]:
            with self.subTest(count=count), self.assertRaises(ValueError):
                SMOKE.validate_metrics({**self.metrics, "completed_rpcs": count})

    def test_error_breakdown_cannot_be_hidden_by_success_counts(self):
        with self.assertRaises(ValueError):
            SMOKE.validate_metrics({**self.metrics, "status_errors": {"UNAVAILABLE": 1}})


class CaptureGuards(unittest.TestCase):
    def test_strace_counts_include_error_calls_and_require_balanced_totals(self):
        import tempfile
        valid = "% time seconds usecs/call calls errors syscall\n------ ----------- ----------- --------- --------- ----------------\n60.00 0.003000 10 30 2 futex\n40.00 0.002000 10 20 read\n100.00 0.005000 10 50 2 total\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "strace"
            path.write_text(valid)
            result = SMOKE.strace_syscalls(path)
            self.assertEqual(result["calls"], 50)
            self.assertEqual(result["errors"], 2)
            self.assertTrue(result["includes_exited_threads"])
            for bad in ["", valid.replace("50 2 total", "51 2 total"),
                        valid.replace("50 2 total", "50 3 total"),
                        valid + "100.00 0.005000 10 50 2 total\n",
                        valid + "60.00 0.003000 10 30 2 futex\n",
                        valid.replace("30 2 futex", "-30 2 futex"),
                        valid.replace("30 2 futex", "30 31 futex"),
                        valid.replace("60.00", "nan"),
                        valid.replace("0.003000", "inf"),
                        valid.replace("50 2 total", "0 0 total")]:
                with self.subTest(summary=bad), self.assertRaises(ValueError):
                    path.write_text(bad)
                    SMOKE.strace_syscalls(path)

    def test_context_switch_snapshot_requires_all_thread_process_totals(self):
        import json
        import tempfile
        valid = {"scope": "process_lifetime", "method": "linux_getrusage_self",
                 "includes_exited_threads": True, "counts": {"voluntary": 20, "involuntary": 3}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stdout"
            def write(record):
                path.write_text("CONTEXT_SWITCHES " + json.dumps(record) + "\n")
            write(valid)
            self.assertEqual(SMOKE.context_switch_record(path)["total"], 23)
            for change in [{"scope": "leader_thread"}, {"scope": "process_since_exec"}, {"method": "proc_status"},
                           {"includes_exited_threads": False}, {"counts": None},
                           {"counts": {"voluntary": -1, "involuntary": 3}},
                           {"counts": {"voluntary": True, "involuntary": 3}},
                           {"counts": {"voluntary": 2 ** 64, "involuntary": 3}}]:
                with self.subTest(change=change), self.assertRaises(ValueError):
                    write({**valid, **change})
                    SMOKE.context_switch_record(path)
            write(valid)
            path.write_text(path.read_text() * 2)
            with self.assertRaises(ValueError):
                SMOKE.context_switch_record(path)
            for invalid in [None, [], 0, "invalid"]:
                with self.subTest(record=invalid), self.assertRaises(ValueError):
                    write(invalid)
                    SMOKE.context_switch_record(path)
            path.write_text("")
            with self.assertRaises(ValueError):
                SMOKE.context_switch_record(path)

    def test_allocator_snapshot_is_mandatory_and_unambiguous(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "stdout"
            path.write_text("READY port=1\n")
            with self.assertRaises(ValueError):
                SMOKE.allocation_record(path)
            valid = 'ALLOCATIONS {"scope":"process_since_main","includes_reallocations":true,"allocations":9,"requested_bytes":100}\n'
            path.write_text(valid)
            self.assertEqual(SMOKE.allocation_record(path)["allocations"], 9)
            path.write_text(valid + valid)
            with self.assertRaises(ValueError):
                SMOKE.allocation_record(path)

    def test_callgrind_requires_a_single_positive_instruction_summary(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "callgrind"
            path.write_text("events: Dr Ir\nsummary: 99 120\n")
            self.assertEqual(SMOKE.callgrind_instructions(path), 120)
            for bad in ["", "events: Dr\nsummary: 9\n", "events: Ir\nsummary: 0\n",
                        "events: Ir\nsummary: 4\nsummary: 5\n"]:
                path.write_text(bad)
                with self.assertRaises(ValueError):
                    SMOKE.callgrind_instructions(path)


@unittest.skipUnless(shutil.which("git"), "requires Git")
class BuildModeGuards(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.source = self.root / "checkout"
        (self.source / "scripts").mkdir(parents=True)
        subprocess.check_output(["git", "init", "-q", str(self.source)])
        # Model a frozen verifier using the legacy validate_record interface.
        # It deliberately leaves feature inspection to the current driver.
        (self.source / "scripts/build-rpc-bench.py").write_text(
            "import json\n"
            "def validate_record(path, binary):\n"
            "    return json.loads(path.read_text())\n")
        self.binary = self.root / "rpc-bench"
        self.binary.write_bytes(b"binary")
        self.record_path = self.root / "build.json"
        self.entry = {"reason": "compiler-artifact", "executable": str(self.binary),
                      "manifest_path": str(self.source / "rpc-bench/Cargo.toml"),
                      "target": {"name": "rpc-bench", "kind": ["bin"]}}

    def write_record(self, instrumented, legacy=False):
        prefix = ["cargo", "build", "--locked", "--release", "--manifest-path",
                  "rpc-bench/Cargo.toml"]
        flags = ["--features", "allocation-counts"] if instrumented else ["--no-default-features"]
        features = ["allocation-counts", "default"] if instrumented else []
        self.entry["features"] = features
        (self.root / "build.stdout.jsonl").write_text(json.dumps(self.entry) + "\n")
        record = {"schema": "pbrs.rpc-bench-build.v1" if legacy else "pbrs.rpc-bench-build.v2",
                  "command": [*prefix, *flags, "--message-format=json"]}
        if not legacy:
            record.update(allocation_counts=instrumented, artifact_features=features.copy())
        self.record_path.write_text(json.dumps(record))
        return record

    def test_legacy_source_checkout_remains_usable_and_features_are_checked(self):
        (self.source / "scripts/build-rpc-bench.py").write_text(
            "import json\n"
            "def validate_record(path, binary):\n"
            "    record = json.loads(path.read_text())\n"
            "    if record.get('schema') != 'pbrs.rpc-bench-build.v1':\n"
            "        raise ValueError('expected legacy record')\n"
            "    return record\n")
        record = self.write_record(True, legacy=True)
        self.assertEqual(SMOKE.verified_build(self.record_path, self.binary, self.source), record)
        self.entry["features"] = ["default"]
        (self.root / "build.stdout.jsonl").write_text(json.dumps(self.entry) + "\n")
        with self.assertRaisesRegex(ValueError, "artifact features disagree"):
            SMOKE.verified_build(self.record_path, self.binary, self.source)

    def test_current_source_verifier_rejects_a_downgraded_record(self):
        self.write_record(True, legacy=True)
        (self.source / "scripts/build-rpc-bench.py").write_bytes(
            (SMOKE.ROOT / "scripts/build-rpc-bench.py").read_bytes())
        with self.assertRaisesRegex(ValueError, "requires a v2 build record"):
            SMOKE.verified_build(self.record_path, self.binary, self.source)

    def test_plain_build_requires_an_explicit_opt_out_of_counter_requirement(self):
        record = self.write_record(False)
        with self.assertRaisesRegex(ValueError, "allocator snapshots require"):
            SMOKE.verified_build(self.record_path, self.binary, self.source)
        self.assertEqual(SMOKE.verified_build(self.record_path, self.binary, self.source,
                                             require_allocation_counts=False), record)

    def driver_checkout(self):
        driver = self.root / "driver"
        (driver / "scripts").mkdir(parents=True)
        helper = driver / "scripts/build-rpc-bench.py"
        helper.write_bytes((SMOKE.ROOT / "scripts/build-rpc-bench.py").read_bytes())
        return driver, helper

    def test_driver_helper_without_index_guard_is_rejected(self):
        self.write_record(True, legacy=True)
        driver, helper = self.driver_checkout()
        helper.write_text("def validate_feature_record(*args): return True\n")
        with patch.object(SMOKE, "ROOT", driver), \
                self.assertRaisesRegex(ValueError, "does not support index visibility"):
            SMOKE.verified_build(self.record_path, self.binary, self.source)

    def test_verifier_mutation_during_validation_is_rejected(self):
        self.write_record(True, legacy=True)
        driver, helper = self.driver_checkout()
        expected = hashlib.sha256(helper.read_bytes()).hexdigest()
        (self.source / "scripts/build-rpc-bench.py").write_text(
            "import json\nfrom pathlib import Path\n"
            "def validate_record(path, binary):\n"
            f"    helper = Path({str(helper)!r})\n"
            "    helper.write_bytes(helper.read_bytes() + b'\\n# mutated\\n')\n"
            "    return json.loads(path.read_text())\n")
        with patch.object(SMOKE, "ROOT", driver), \
                self.assertRaisesRegex(ValueError, "driver build verifier changed during validation"):
            SMOKE.verified_build(self.record_path, self.binary, self.source, verifier_sha256=expected)

    def test_source_checkout_verifier_mutation_during_validation_is_rejected(self):
        self.write_record(True, legacy=True)
        driver, _ = self.driver_checkout()
        (self.source / "scripts/build-rpc-bench.py").write_text(
            "import json\nfrom pathlib import Path\n"
            "def validate_record(path, binary):\n"
            "    helper = Path(__file__)\n"
            "    helper.write_bytes(helper.read_bytes() + b'\\n# mutated\\n')\n"
            "    return json.loads(path.read_text())\n")
        with patch.object(SMOKE, "ROOT", driver), \
                self.assertRaisesRegex(ValueError, "source build verifier changed during validation"):
            SMOKE.verified_build(self.record_path, self.binary, self.source)

    def test_verifier_is_retained_and_mutation_fails_a_capture(self):
        self.write_record(True, legacy=True)
        driver, helper = self.driver_checkout()
        original = helper.read_bytes()
        expected = hashlib.sha256(original).hexdigest()

        for mutation in ("none", "live", "retained"):
            with self.subTest(mutation=mutation):
                helper.write_bytes(original)
                output = self.root / mutation
                argv = ["grpc-load-smoke.py", "--binary", str(self.binary), "--output", str(output),
                        "--build-record", str(self.record_path), "--source-checkout", str(self.source),
                        "--allocation-counts", "--payloads=0"]

                def run_cell(args, cell, directory):
                    directory.mkdir()
                    if mutation == "live":
                        helper.write_bytes(original + b"\n# mutated\n")
                    elif mutation == "retained":
                        (output / "build-verifier.py").write_bytes(original + b"\n# mutated\n")
                    return {"passed": True}

                def git_output(command, **kwargs):
                    if not kwargs.get("text"):
                        return b""
                    return "" if "status" in command else "a" * 40

                with patch("sys.argv", argv), patch.object(SMOKE, "ROOT", driver), \
                        patch.object(SMOKE.platform, "system", return_value="Linux"), \
                        patch.object(SMOKE.subprocess, "check_output", side_effect=git_output), \
                        patch.object(SMOKE, "PAIRS", [("native", "pbrs", "tonic", "prost")]), \
                        patch.object(SMOKE, "SHAPES", ["unary"]), \
                        patch.object(SMOKE, "run_cell", side_effect=run_cell):
                    self.assertEqual(SMOKE.main(), 0 if mutation == "none" else 1)
                report = json.loads((output / "report.json").read_text())
                self.assertEqual(report["build_verifier_sha256"], expected)
                saved = (output / report["build_verifier_file"]).read_bytes()
                self.assertEqual(saved == original, mutation != "retained")
                self.assertIs(report["build_verifier_unchanged"], mutation == "none")
                self.assertIs(report["passed"], mutation == "none")
                if mutation == "live":
                    self.assertIn("driver build verifier changed", report["source_error"])

    def test_plain_build_rejects_allocator_request_before_endpoint_startup(self):
        self.write_record(False)
        output = self.root / "results"
        argv = ["grpc-load-smoke.py", "--binary", str(self.binary), "--output", str(output),
                "--build-record", str(self.record_path), "--source-checkout", str(self.source),
                "--allocation-counts"]
        with patch("sys.argv", argv), patch.object(SMOKE.platform, "system", return_value="Linux"), \
                patch.object(SMOKE, "run_cell") as run_cell, patch("sys.stderr", new_callable=io.StringIO) as errors:
            with self.assertRaises(SystemExit):
                SMOKE.main()
        self.assertIn("allocator snapshots require", errors.getvalue())
        run_cell.assert_not_called()
        self.assertFalse(output.exists())


@unittest.skipUnless(shutil.which("git") and hasattr(os, "sched_getaffinity"),
                     "requires Git and Linux CPU affinity")
class CaptureProvenanceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.source = self.root / "source"
        self.driver = self.root / "driver"
        for checkout in (self.source, self.driver):
            (checkout / "scripts").mkdir(parents=True)
            (checkout / "tracked.txt").write_text("base\n")
            (checkout / "tracked.bin").write_bytes(b"\x00base\xff")
            (checkout / ".gitignore").write_text(
                "ignored/\n!ignored/capture/report.json\n.pbrs-load-smoke-output-probe\n"
                "partial/*\n!partial/open/\n!partial/open/report.json\n"
                "file-only/*\nreinclude/*\n!reinclude/report.json\n"
                "subfile/*\n!subfile/cell-000/\nsubfile/cell-000/*\n"
                "!subfile/cell-000/run.json\n"
                "traces/*\n!traces/cell-000/\ntraces/cell-000/*\n"
                "!traces/cell-000/server.strace\n"
                "calls/*\n!calls/cell-000/\ncalls/cell-000/*\n"
                "!calls/cell-000/server.callgrind\n")
        (self.source / "rpc-bench").mkdir()
        (self.source / "rpc-bench/Cargo.toml").write_text("# test manifest\n")
        (self.source / "scripts/build-rpc-bench.py").write_text(
            "import json\n"
            "def validate_record(path, binary):\n"
            "    return json.loads(path.read_text())\n")
        (self.driver / "scripts/build-rpc-bench.py").write_bytes(
            (SMOKE.ROOT / "scripts/build-rpc-bench.py").read_bytes())
        for checkout in (self.source, self.driver):
            self.git(checkout, "init", "-q")
            self.git(checkout, "add", ".")
            self.git(checkout, "-c", "user.name=Benchmark Test",
                     "-c", "user.email=benchmark-test@example.invalid", "-c", "commit.gpgsign=false",
                     "-c", f"core.hooksPath={checkout / '.git' / 'empty-hooks'}", "commit", "-qm", "initial")

        self.binary = self.root / "rpc-bench"
        self.binary.write_bytes(b"frozen binary")
        self.record = self.root / "build.json"
        self.record.write_text(json.dumps({
            "schema": "pbrs.rpc-bench-build.v1",
            "command": ["cargo", "build", "--locked", "--release", "--manifest-path",
                        "rpc-bench/Cargo.toml", "--features", "allocation-counts",
                        "--message-format=json"],
        }))
        entry = {"reason": "compiler-artifact", "executable": str(self.binary),
                 "manifest_path": str(self.source / "rpc-bench/Cargo.toml"),
                 "target": {"name": "rpc-bench", "kind": ["bin"]},
                 "features": ["allocation-counts", "default"]}
        (self.root / "build.stdout.jsonl").write_text(json.dumps(entry) + "\n")

    @staticmethod
    def git(checkout, *args):
        return subprocess.check_output(["git", *args], cwd=checkout)

    def capture(self, output, mutate_patch=None):
        argv = ["grpc-load-smoke.py", "--binary", str(self.binary), "--output", str(output),
                "--build-record", str(self.record), "--source-checkout", str(self.source),
                "--allocation-counts", "--payloads=0"]

        def run_cell(args, cell, directory):
            directory.mkdir()
            if mutate_patch:
                (output / mutate_patch).write_bytes(b"changed after capture\n")
            return {"passed": True}

        with patch("sys.argv", argv), patch.object(SMOKE, "ROOT", self.driver), \
                patch.object(SMOKE.platform, "system", return_value="Linux"), \
                patch.object(SMOKE, "PAIRS", [("native", "pbrs", "tonic", "prost")]), \
                patch.object(SMOKE, "SHAPES", ["unary"]), \
                patch.object(SMOKE, "run_cell", side_effect=run_cell), \
                patch("sys.stdout", new_callable=io.StringIO):
            return SMOKE.main()

    def test_selected_source_and_dirty_driver_keep_distinct_patches(self):
        tracked = self.driver / "tracked.txt"
        tracked.write_text("staged\n")
        self.git(self.driver, "add", "tracked.txt")
        tracked.write_text("working tree\n")
        (self.driver / "untracked.txt").write_text("not in a Git patch\n")
        self.git(self.driver, "config", "status.showUntrackedFiles", "no")
        self.git(self.driver, "config", "color.ui", "always")
        output = self.root / "capture"
        self.assertEqual(self.capture(output), 0)

        source_patch = (output / "source.patch").read_bytes()
        driver_patch = (output / "harness.patch").read_bytes()
        self.assertEqual(source_patch, b"")
        self.assertIn(b"+working tree", driver_patch)
        self.assertNotIn(b"untracked.txt", driver_patch)
        self.assertNotIn(b"\x1b", driver_patch)
        report = json.loads((output / "report.json").read_text())
        self.assertFalse(report["dirty"])
        self.assertTrue(report["harness_dirty"])
        self.assertTrue(report["source_verified"])
        self.assertTrue(report["patches_unchanged"])
        self.assertEqual(report["source_patch_sha256"], hashlib.sha256(source_patch).hexdigest())
        self.assertEqual(report["harness_patch_sha256"], hashlib.sha256(driver_patch).hexdigest())
        self.assertIn("untracked files", report["patch_scope"])

    def test_untracked_driver_files_are_reported_even_if_git_hides_them(self):
        self.git(self.driver, "config", "status.showUntrackedFiles", "no")
        (self.driver / "untracked.txt").write_text("not captured\n")
        output = self.root / "untracked-capture"
        self.assertEqual(self.capture(output), 0)
        report = json.loads((output / "report.json").read_text())
        self.assertTrue(report["harness_dirty"])
        self.assertEqual((output / "harness.patch").read_bytes(), b"")

    def test_patch_scope_does_not_claim_separate_index_state(self):
        tracked = self.driver / "tracked.txt"
        tracked.write_text("staged only\n")
        self.git(self.driver, "add", "tracked.txt")
        tracked.write_text("base\n")
        output = self.root / "staging-capture"
        self.assertEqual(self.capture(output), 0)
        report = json.loads((output / "report.json").read_text())
        self.assertTrue(report["harness_dirty"])
        self.assertEqual((output / "harness.patch").read_bytes(), b"")
        self.assertIn("index staging", report["patch_scope"])

    def test_git_patch_contains_net_tracked_worktree_and_binary_changes_only(self):
        tracked = self.source / "tracked.txt"
        tracked.write_text("staged\n")
        self.git(self.source, "add", "tracked.txt")
        tracked.write_text("working tree\n")
        (self.source / "tracked.bin").write_bytes(b"\x00changed\xff")
        (self.source / "untracked.txt").write_text("excluded\n")
        result = SMOKE.git_patch(self.source)
        self.assertIn(b"+working tree", result)
        self.assertIn(b"GIT binary patch", result)
        self.assertNotIn(b"untracked.txt", result)

    def test_unignored_output_in_verified_source_fails_before_cells(self):
        output = self.source / "capture"
        with patch("sys.stderr", new_callable=io.StringIO) as errors:
            with self.assertRaises(SystemExit) as result:
                self.capture(output)
        self.assertEqual(result.exception.code, 2)
        self.assertIn("inside the measured source must be Git-ignored", errors.getvalue())
        self.assertFalse(output.exists())

    def test_nested_unignored_or_file_only_rules_fail_before_cells(self):
        for relative in ("partial/open/capture", "reinclude", "subfile"):
            with self.subTest(relative=relative):
                output = self.source / relative
                with patch("sys.stderr", new_callable=io.StringIO) as errors:
                    with self.assertRaises(SystemExit) as result:
                        self.capture(output)
                self.assertEqual(result.exception.code, 2)
                self.assertIn("inside the measured source must be Git-ignored", errors.getvalue())
                self.assertFalse(output.exists())

    def test_file_only_ignore_rules_can_cover_all_capture_files(self):
        output = self.source / "file-only"
        self.assertEqual(self.capture(output), 0)
        self.assertEqual(self.git(self.source, "status", "--porcelain"), b"")

    def test_optional_tracer_files_are_checked(self):
        output = (self.source / "traces").resolve()
        SMOKE.validate_verified_output_location(output, self.source, 1, callgrind=None, strace=None)
        with self.assertRaisesRegex(ValueError, "must be Git-ignored"):
            SMOKE.validate_verified_output_location(output, self.source, 1,
                                                    callgrind=None, strace=True)

    def test_optional_callgrind_files_are_checked(self):
        output = (self.source / "calls").resolve()
        SMOKE.validate_verified_output_location(output, self.source, 1, callgrind=None, strace=None)
        with self.assertRaisesRegex(ValueError, "must be Git-ignored"):
            SMOKE.validate_verified_output_location(output, self.source, 1,
                                                    callgrind=True, strace=None)

    def test_ignored_output_in_verified_source_is_allowed(self):
        output = self.source / "ignored" / "capture"
        self.assertEqual(self.capture(output), 0)
        report = json.loads((output / "report.json").read_text())
        self.assertFalse(report["dirty"])
        self.assertTrue(report["source_unchanged"])
        self.assertEqual(self.git(self.source, "status", "--porcelain"), b"")

    def test_tampered_retained_driver_fails_capture(self):
        output = self.root / "driver-capture"
        self.assertEqual(self.capture(output, mutate_patch="harness.py"), 1)
        report = json.loads((output / "report.json").read_text())
        self.assertFalse(report["harness_script_unchanged"])
        self.assertTrue(report["patches_unchanged"])
        self.assertFalse(report["passed"])

    def test_tampered_retained_patch_fails_capture(self):
        for filename in ("source.patch", "harness.patch"):
            with self.subTest(filename=filename):
                output = self.root / (filename + "-capture")
                self.assertEqual(self.capture(output, mutate_patch=filename), 1)
                report = json.loads((output / "report.json").read_text())
                self.assertFalse(report["patches_unchanged"])
                self.assertFalse(report["passed"])


    def test_legacy_source_verifier_cannot_hide_index_flags(self):
        tracked = self.source / "tracked.txt"
        for flag, reset in (("--assume-unchanged", "--no-assume-unchanged"),
                            ("--skip-worktree", "--no-skip-worktree")):
            with self.subTest(flag=flag):
                self.git(self.source, "update-index", flag, "--", "tracked.txt")
                tracked.write_text("changed but hidden\n")
                # The legacy source verifier sees this as clean and lacks
                # index-state validation; the newer driver must reject it.
                self.assertEqual(
                    self.git(self.source, "status", "--porcelain", "--untracked-files=all"),
                    b"",
                )
                output = self.root / ("legacy-hidden-" + flag[2:])
                with patch("sys.stderr", new_callable=io.StringIO) as errors:
                    with self.assertRaises(SystemExit) as result:
                        self.capture(output)
                self.assertEqual(result.exception.code, 2)
                self.assertIn("assume-unchanged or skip-worktree", errors.getvalue())
                self.assertFalse(output.exists())
                tracked.write_text("base\n")
                self.git(self.source, "update-index", reset, "--", "tracked.txt")
                self.assertEqual(
                    self.git(self.source, "status", "--porcelain", "--untracked-files=all"),
                    b"",
                )


if __name__ == "__main__":
    unittest.main()
