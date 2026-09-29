"""Performance CI must distinguish measurements from successful uploads."""

import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/perf-report.py"
SPEC = importlib.util.spec_from_file_location("perf_report", SCRIPT)
PERF = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PERF)
BASE, HEAD = "a" * 40, "b" * 40


def report(sha):
    metric = {"status": "measured", "data": {"value": 1.0, "unit": "per op"}}
    cell = {
        "id": "codec.test", "kind": "codec", "codec": "pbrs",
        "iters": 200, "repeats": 3, "instruction_method": "perf_differential",
        **{key: copy.deepcopy(metric) for key in PERF.METRICS},
    }
    return {
        "schema": "devloop/1", "devloop_commit": sha, "cells": [cell],
        "host": {"os": "linux", "arch": "x86_64", "cpu": "test CPU",
                 "rustc": "rustc 1.98.1", "perf": True, "strace": False,
                 "valgrind": False},
    }


class EvidenceTests(unittest.TestCase):
    def verify(self, base, head):
        return PERF.validate_pair(base, head, BASE, HEAD)

    def test_valid_measured_pair_can_count_toward_noise(self):
        result = self.verify(report(BASE), report(HEAD))
        self.assertTrue(result["qualified_for_noise"])
        self.assertIn("allocs", result["eligible_metrics"]["codec.test"])

    def test_empty_error_and_stale_revision_reports_never_qualify(self):
        for change in ({"cells": []}, {"error": "build failed"}, {"devloop_commit": BASE}):
            with self.subTest(change=change):
                head = report(HEAD)
                head.update(change)
                result = self.verify(report(BASE), head)
                self.assertFalse(result["qualified_for_noise"])
                self.assertEqual(result["eligible_metrics"], {})

    def test_duplicate_cells_and_different_sample_windows_fail(self):
        for field, value in (("iters", 100), ("repeats", 1), ("kind", "rpc")):
            with self.subTest(field=field):
                head = report(HEAD)
                head["cells"][0][field] = value
                self.assertFalse(self.verify(report(BASE), head)["qualified_for_noise"])
        head = report(HEAD)
        head["cells"].append(copy.deepcopy(head["cells"][0]))
        self.assertFalse(self.verify(report(BASE), head)["qualified_for_noise"])

    def test_missing_optional_counters_remain_not_run(self):
        base, head = report(BASE), report(HEAD)
        for item in (base, head):
            item["cells"][0]["instructions"] = {
                "status": "not_run", "data": {"reason": "perf unavailable"}
            }
        result = self.verify(base, head)
        self.assertTrue(result["qualified_for_noise"])
        self.assertNotIn("instructions", result["eligible_metrics"]["codec.test"])
        self.assertIn("not_run", PERF.summary(result, base, head))

    def test_invalid_numbers_units_and_instruction_methods_fail(self):
        for value in (float("nan"), float("inf"), -1, True, "1"):
            with self.subTest(value=value):
                head = report(HEAD)
                head["cells"][0]["allocs"]["data"]["value"] = value
                self.assertFalse(self.verify(report(BASE), head)["qualified_for_noise"])
        for change in ("unit", "method"):
            head = report(HEAD)
            if change == "unit":
                head["cells"][0]["allocs"]["data"]["unit"] = "different"
            else:
                head["cells"][0]["instruction_method"] = "whole_process_legacy"
            self.assertFalse(self.verify(report(BASE), head)["qualified_for_noise"])

    def test_missing_required_measurement_and_host_mismatch_fail(self):
        head = report(HEAD)
        head["cells"][0]["allocs"] = {"status": "not_run", "data": {"reason": "failed"}}
        self.assertFalse(self.verify(report(BASE), head)["qualified_for_noise"])
        head = report(HEAD)
        head["host"]["rustc"] = "different compiler"
        self.assertFalse(self.verify(report(BASE), head)["qualified_for_noise"])

    def test_added_cells_are_visible_and_not_noise_samples(self):
        head = report(HEAD)
        added = copy.deepcopy(head["cells"][0])
        added["id"] = "codec.new"
        head["cells"].append(added)
        result = self.verify(report(BASE), head)
        self.assertTrue(result["qualified_for_noise"])
        self.assertEqual(result["new_cells"], ["codec.new"])
        self.assertNotIn("codec.new", result["eligible_metrics"])

    def test_cli_missing_report_retains_explicit_invalid_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            head = root / "head.json"
            head.write_text(json.dumps(report(HEAD)))
            out, summary = root / "validation.json", root / "summary.md"
            run = subprocess.run([
                sys.executable, str(SCRIPT), "verify", "--base", str(root / "missing.json"),
                "--head", str(head), "--base-sha", BASE, "--head-sha", HEAD,
                "--out", str(out), "--summary", str(summary),
            ], capture_output=True, text=True)
            self.assertEqual(run.returncode, 1, run.stderr)
            self.assertFalse(json.loads(out.read_text())["qualified_for_noise"])
            self.assertIn("must not count", summary.read_text())


class RevisionTests(unittest.TestCase):
    def test_push_pr_and_explicit_dispatch_pin_the_correct_pair(self):
        events = [
            ("push", {"before": BASE, "after": HEAD}),
            ("pull_request", {"pull_request": {"base": {"sha": BASE}, "head": {"sha": HEAD}}}),
            ("workflow_dispatch", {"inputs": {"base_sha": BASE, "head_sha": HEAD}}),
            ("workflow_dispatch", {"inputs": {"base_sha": BASE}}),
        ]
        for name, event in events:
            with self.subTest(event=name):
                self.assertEqual(PERF.revisions(name, event, HEAD), (BASE, HEAD))

    def test_branch_names_zero_sha_and_shell_text_are_rejected(self):
        for invalid in ("main", "0" * 40, HEAD[:12], "$(touch /tmp/bad)", "--upload-pack=bad"):
            with self.subTest(invalid=invalid):
                with self.assertRaises(ValueError):
                    PERF.revisions("workflow_dispatch", {"inputs": {"base_sha": invalid}}, HEAD)


if __name__ == "__main__":
    unittest.main()
