"""Counter validity and N/2N workload matching for the diagnostic ledger."""
import hashlib
import importlib.util
import json
import tempfile
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("ledger", Path(__file__).resolve().parents[1] / "scripts/dominance-ledger.py")
LEDGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LEDGER)


class DifferentialTests(unittest.TestCase):
    def test_syscalls_require_all_thread_counts_and_consistent_totals(self):
        def row(count):
            return {"syscall_totals": {"client": {"scope": "traced_endpoint_lifetime",
                "method": "linux_strace_f_count", "includes_exited_threads": True,
                "calls": count, "errors": 1, "syscall_counts": {"futex": {"calls": count, "errors": 1}}}}}
        self.assertEqual(LEDGER.differential(row(30), row(70), "client", "syscalls", 8), 5)
        for change in [{"calls": 71}, {"calls": True}, {"errors": 2},
                       {"scope": "leader_thread"}, {"includes_exited_threads": False},
                       {"syscall_counts": {}}, {"syscall_counts": {"futex": {"calls": 70, "errors": 71}}}]:
            invalid = row(70)
            invalid["syscall_totals"]["client"].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                LEDGER.differential(row(30), invalid, "client", "syscalls", 8)

    def test_context_switch_totals_preserve_scope_and_reject_bad_sums(self):
        def row(voluntary, involuntary):
            return {"context_switch_totals": {"client": {
                "scope": "process_lifetime", "method": "linux_getrusage_self",
                "includes_exited_threads": True,
                "counts": {"voluntary": voluntary, "involuntary": involuntary},
                "total": voluntary + involuntary}}}
        first, second = row(30, 5), row(60, 7)
        self.assertEqual(LEDGER.differential(first, second, "client", "context_switches", 8), 4)
        second["context_switch_totals"]["client"]["total"] = 1
        with self.assertRaises(ValueError):
            LEDGER.differential(first, second, "client", "context_switches", 8)
        for change in [{"scope": "leader_thread"}, {"includes_exited_threads": False}]:
            second = row(60, 7)
            second["context_switch_totals"]["client"].update(change)
            with self.assertRaises(ValueError):
                LEDGER.differential(first, second, "client", "context_switches", 8)

    def test_positive_endpoint_differential(self):
        before = {"allocation_totals": {"server": {"allocations": 100}}}
        after = {"allocation_totals": {"server": {"allocations": 180}}}
        self.assertEqual(LEDGER.differential(before, after, "server", "allocations", 8), 10)

    def test_missing_and_nonpositive_differences_stay_unmeasured(self):
        self.assertIsNone(LEDGER.differential({}, {}, "client", "instructions", 8))
        before = {"instruction_totals": {"client": 100}}
        for total in [99, 100]:
            self.assertIsNone(LEDGER.differential(before, {"instruction_totals": {"client": total}}, "client", "instructions", 8))

    def test_invalid_counters_fail(self):
        for invalid in [False, "100", -1, 100.0]:
            with self.assertRaises(ValueError):
                LEDGER.differential({"instruction_totals": {"client": invalid}},
                                    {"instruction_totals": {"client": 200}}, "client", "instructions", 8)

    def test_workload_identity_distinguishes_repeat_and_concurrency(self):
        cell = {"pair": ["native", "prost", "tonic", "prost"], "shape": "bidi_pipelined", "payload_bytes": 1024,
                "tls": True, "compression": "gzip", "connections": 1, "in_flight": 16, "repeat": 0}
        for update in [{"repeat": 1}, {"connections": 64}, {"shape": "bidi"}, {"tls": False}]:
            self.assertNotEqual(LEDGER.key(cell), LEDGER.key({**cell, **update}))


class FailedCaptureTests(unittest.TestCase):
    def capture(self, path, count, failed=False):
        path.mkdir()
        cells = [{"pair": list(pair), "shape": "bidi", "payload_bytes": 1024,
                  "tls": False, "compression": "identity", "connections": 1,
                  "in_flight": 1, "repeat": 0}
                 for pair in [LEDGER.SIDES["client"][1], LEDGER.REFERENCE]]
        runs = []
        for index, cell in enumerate(cells):
            invalid = failed and index == 0
            row = {"cell": cell, "passed": not invalid,
                   "metrics": {"successful_rpcs": count - int(invalid)},
                   "instruction_totals": {"client": 1000 + count * (10 if index == 0 else 20)}}
            if invalid:
                row["error"] = "one RPC timed out"
            name = f"run-{index}.json"
            (path / name).write_text(json.dumps(row))
            runs.append({"path": name, "passed": not invalid})
        (path / "report.json").write_text(json.dumps({
            "schema": "pbrs.load-smoke.v2", "passed": not failed, "binary_unchanged": True,
            "source_unchanged": True, "source_verified": True, "dirty": False,
            "binary_sha256": "a" * 64, "head": "b" * 40, "rpc_count": count,
            "callgrind": "valgrind", "allocation_counts": True, "host": {},
            "cpu_affinity": [0], "cells": cells, "runs": runs}))

    def attach_verifier(self, path, contents=b"verified helper\n"):
        (path / "build-verifier.py").write_bytes(contents)
        report = json.loads((path / "report.json").read_text())
        report.update(build_record={"schema": "pbrs.rpc-bench-build.v2"},
                      build_verifier_sha256=hashlib.sha256(contents).hexdigest(),
                      build_verifier_file="build-verifier.py", build_verifier_unchanged=True)
        (path / "report.json").write_text(json.dumps(report))
        return report

    def attach_capture_patches(self, path, source=b"", harness=b"", driver=b"saved driver\n"):
        contents = {"source.patch": source, "harness.patch": harness, "harness.py": driver}
        for name, data in contents.items():
            (path / name).write_bytes(data)
        report = json.loads((path / "report.json").read_text())
        report.update(source_patch_sha256=hashlib.sha256(source).hexdigest(),
                      harness_patch_sha256=hashlib.sha256(harness).hexdigest(),
                      harness_script_sha256=hashlib.sha256(driver).hexdigest(),
                      harness_script_unchanged=True, patches_unchanged=True,
                      patch_scope="net tracked worktree diff; untracked files omitted",
                      harness_head="c" * 40, harness_dirty=False)
        (path / "report.json").write_text(json.dumps(report))
        return report

    def test_new_capture_retained_files_are_verified_on_readback(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8, failed=True)
            original = self.attach_capture_patches(path, b"source", b"harness")
            self.assertEqual(LEDGER.load(path, allow_failed=True)[0]["patch_scope"],
                             original["patch_scope"])
            for filename in ("source.patch", "harness.patch", "harness.py"):
                with self.subTest(filename=filename):
                    saved = path / filename
                    contents = saved.read_bytes()
                    saved.write_bytes(contents + b" changed")
                    with self.assertRaisesRegex(ValueError, "retained capture differs"):
                        LEDGER.load(path, allow_failed=True)
                    saved.unlink()
                    with self.assertRaisesRegex(ValueError, "missing or unreadable"):
                        LEDGER.load(path, allow_failed=True)
                    saved.symlink_to(path / "report.json")
                    with self.assertRaisesRegex(ValueError, "retained capture differs"):
                        LEDGER.load(path, allow_failed=True)
                    saved.unlink()
                    saved.write_bytes(contents)

    def test_new_capture_rejects_partial_or_invalid_patch_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8)
            original = self.attach_capture_patches(path)
            for field in ("source_patch_sha256", "harness_patch_sha256", "patch_scope",
                          "patches_unchanged", "harness_script_sha256", "harness_head", "harness_dirty"):
                with self.subTest(field=field):
                    changed = dict(original)
                    del changed[field]
                    (path / "report.json").write_text(json.dumps(changed))
                    with self.assertRaises(ValueError):
                        LEDGER.load(path, allow_failed=True)
            for change in ({"patches_unchanged": False}, {"patch_scope": ""},
                           {"source_patch_sha256": "A" * 64},
                           {"harness_patch_sha256": None}, {"harness_head": ""},
                           {"harness_head": "x" * 40}, {"harness_head": "c" * 41},
                           {"harness_dirty": None}, {"harness_dirty": 0}):
                with self.subTest(change=change):
                    (path / "report.json").write_text(json.dumps({**original, **change}))
                    with self.assertRaises(ValueError):
                        LEDGER.load(path, allow_failed=True)

    def test_new_paired_captures_require_matching_retained_patches(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16)
            self.attach_capture_patches(small)
            self.attach_capture_patches(large)
            self.assertFalse(LEDGER.compare(small, large)["qualified"])
            for name in ("source", "harness"):
                with self.subTest(name=name):
                    kwargs = {name: b"different"}
                    self.attach_capture_patches(large, **kwargs)
                    with self.assertRaisesRegex(ValueError, name + "_patch_sha256"):
                        LEDGER.compare(small, large)
                    self.attach_capture_patches(large)

    def test_new_paired_captures_require_matching_driver_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16)
            self.attach_capture_patches(small)
            original = self.attach_capture_patches(large)
            for field, value in (("harness_head", "d" * 40), ("harness_dirty", True)):
                with self.subTest(field=field):
                    (large / "report.json").write_text(json.dumps({**original, field: value}))
                    with self.assertRaisesRegex(ValueError, field):
                        LEDGER.compare(small, large)

    def test_new_capture_accepts_git_sha256_driver_head(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8)
            report = self.attach_capture_patches(path)
            report["harness_head"] = "d" * 64
            (path / "report.json").write_text(json.dumps(report))
            self.assertEqual(LEDGER.load(path)[0]["harness_head"], "d" * 64)

    def test_paired_captures_reject_asymmetric_patch_evidence(self):
        for patched_side in ("small", "large"):
            with self.subTest(patched_side=patched_side), tempfile.TemporaryDirectory() as directory:
                small, large = Path(directory) / "small", Path(directory) / "large"
                self.capture(small, 8)
                self.capture(large, 16)
                new_path, other_path = (small, large) if patched_side == "small" else (large, small)
                new = self.attach_capture_patches(new_path)
                other = json.loads((other_path / "report.json").read_text())
                other.update(harness_script_sha256=new["harness_script_sha256"],
                             harness_script_unchanged=True)
                (other_path / "report.json").write_text(json.dumps(other))
                with self.assertRaisesRegex(ValueError, "patch evidence presence mismatch"):
                    LEDGER.compare(small, large)

    def test_new_paired_captures_require_matching_patch_scope(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16)
            self.attach_capture_patches(small)
            report = self.attach_capture_patches(large)
            report["patch_scope"] = "a different nonempty patch scope"
            (large / "report.json").write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, "patch_scope"):
                LEDGER.compare(small, large)

    def test_legacy_reports_without_verifier_metadata_remain_compatible(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16)
            self.assertFalse(LEDGER.compare(small, large)["qualified"])

    def test_v2_build_cannot_omit_all_verifier_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8)
            report = json.loads((path / "report.json").read_text())
            report["build_record"] = {"schema": "pbrs.rpc-bench-build.v2"}
            (path / "report.json").write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, "verifier"):
                LEDGER.load(path, allow_failed=True)

    def test_verifier_provenance_is_required_even_in_failed_capture_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8, failed=True)
            original = self.attach_verifier(path)
            self.assertEqual(LEDGER.load(path, allow_failed=True)[0]["build_verifier_sha256"],
                             original["build_verifier_sha256"])
            for change in ({"build_verifier_unchanged": False}, {"build_verifier_unchanged": None},
                           {"build_verifier_sha256": None}, {"build_verifier_sha256": "a" * 63},
                           {"build_verifier_sha256": "A" * 64}, {"build_verifier_file": "../outside"}):
                with self.subTest(change=change):
                    (path / "report.json").write_text(json.dumps({**original, **change}))
                    with self.assertRaisesRegex(ValueError, "verifier"):
                        LEDGER.load(path, allow_failed=True)
            for missing in ("build_verifier_sha256", "build_verifier_file", "build_verifier_unchanged"):
                with self.subTest(missing=missing):
                    partial = dict(original)
                    del partial[missing]
                    (path / "report.json").write_text(json.dumps(partial))
                    with self.assertRaisesRegex(ValueError, "verifier"):
                        LEDGER.load(path, allow_failed=True)
            (path / "report.json").write_text(json.dumps(original))
            (path / "build-verifier.py").write_bytes(b"mutated helper\n")
            with self.assertRaisesRegex(ValueError, "retained build verifier differs"):
                LEDGER.load(path, allow_failed=True)
            (path / "build-verifier.py").unlink()
            with self.assertRaisesRegex(ValueError, "retained build verifier is missing"):
                LEDGER.load(path, allow_failed=True)

    def test_paired_captures_require_matching_retained_verifiers(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16)
            self.attach_verifier(small)
            self.attach_verifier(large)
            self.assertFalse(LEDGER.compare(small, large)["qualified"])
            self.attach_verifier(large, b"different but valid helper\n")
            with self.assertRaisesRegex(ValueError, "build_verifier_sha256"):
                LEDGER.compare(small, large)
            report = json.loads((small / "report.json").read_text())
            for field in ("build_verifier_sha256", "build_verifier_file", "build_verifier_unchanged"):
                del report[field]
            report["build_record"] = {"schema": "pbrs.rpc-bench-build.v1"}
            (small / "report.json").write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, "build_verifier_sha256"):
                LEDGER.compare(small, large)

    def test_invalid_pair_cannot_turn_into_a_win_in_partial_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16, failed=True)
            with self.assertRaises(ValueError):
                LEDGER.compare(small, large)
            result = LEDGER.compare(small, large, allow_failed=True)
            self.assertFalse(result["qualified"])
            self.assertEqual(len(result["rows"]), len(LEDGER.REQUIRED_METRICS))
            for row in result["rows"]:
                self.assertEqual(row["disposition"], "failed_capture")
                self.assertIsNone(row["ratio"])
                self.assertEqual(len(row["failed_captures"]), 1)

    def test_partial_mode_still_rejects_source_drift(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8, failed=True)
            manifest = json.loads((path / "report.json").read_text())
            manifest["source_unchanged"] = False
            (path / "report.json").write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                LEDGER.load(path, allow_failed=True)

    def test_matched_counts_reject_different_tracers_or_driver_scripts(self):
        with tempfile.TemporaryDirectory() as directory:
            small, large = Path(directory) / "small", Path(directory) / "large"
            self.capture(small, 8)
            self.capture(large, 16)
            original = json.loads((large / "report.json").read_text())
            for field in ["strace", "strace_version", "harness_script_sha256"]:
                (large / "report.json").write_text(json.dumps({**original, field: "different"}))
                with self.subTest(field=field), self.assertRaises(ValueError):
                    LEDGER.compare(small, large)

    def test_driver_drift_cannot_be_accepted_as_a_partial_failed_cell(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture"
            self.capture(path, 8)
            original = json.loads((path / "report.json").read_text())
            for checked in [None, False]:
                (path / "report.json").write_text(json.dumps({**original,
                    "harness_script_sha256": "a" * 64, "harness_script_unchanged": checked}))
                with self.subTest(checked=checked), self.assertRaises(ValueError):
                    LEDGER.load(path, allow_failed=True)


if __name__ == "__main__":
    unittest.main()
