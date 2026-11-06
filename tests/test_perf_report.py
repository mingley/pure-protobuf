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
MISSING = object()
FRESH_CELLS = (
    "codec.pbrs.fresh_encode", "codec.prost.fresh_encode", "codec.v4.fresh_encode",
)
FRESH_PROFILE = "independent-warmup-v1"


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


def fresh_report(sha, cell_id, profile=MISSING):
    result = report(sha)
    cell = result["cells"][0]
    cell["id"] = cell_id
    cell["codec"] = {"pbrs": "pbrs", "prost": "prost", "v4": "v4-upb"}[cell_id.split(".")[1]]
    if profile is not MISSING:
        cell["fresh_encode_profile"] = profile
    return result


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


class FreshEncodeProfileTests(unittest.TestCase):
    def test_known_profiles_qualify_with_explicit_lifecycle(self):
        for name in FRESH_CELLS:
            with self.subTest(cell=name):
                base = fresh_report(BASE, name, FRESH_PROFILE)
                head = fresh_report(HEAD, name, FRESH_PROFILE)
                result = PERF.validate_pair(base, head, BASE, HEAD)
                self.assertTrue(result["qualified_for_noise"], result["errors"])
                self.assertEqual(result["fresh_encode_lifecycles"], {name: FRESH_PROFILE})
                self.assertEqual(set(result["eligible_metrics"][name]), set(PERF.METRICS))
                self.assertIn(FRESH_PROFILE, PERF.summary(result, base, head))

    def test_missing_and_null_historical_profiles_remain_explicitly_legacy(self):
        for name in FRESH_CELLS:
            for before in (MISSING, None):
                for after in (MISSING, None):
                    with self.subTest(cell=name, before=before, after=after):
                        base = fresh_report(BASE, name, before)
                        head = fresh_report(HEAD, name, after)
                        result = PERF.validate_pair(base, head, BASE, HEAD)
                        self.assertTrue(result["qualified_for_noise"], result["errors"])
                        self.assertEqual(result["fresh_encode_lifecycles"], {name: "legacy-unreported"})
                        self.assertIn("do not establish independent fresh-object costs",
                                      PERF.summary(result, base, head))

    def test_mixed_lifecycles_in_either_direction_clear_all_metrics(self):
        for name in FRESH_CELLS:
            for missing in (MISSING, None):
                for before, after in ((missing, FRESH_PROFILE), (FRESH_PROFILE, missing)):
                    with self.subTest(cell=name, before=before, after=after):
                        base = fresh_report(BASE, name, before)
                        head = fresh_report(HEAD, name, after)
                        # An unrelated valid common row must not keep any part
                        # of an incompatible run eligible for noise qualification.
                        base["cells"].extend(report(BASE)["cells"])
                        head["cells"].extend(report(HEAD)["cells"])
                        result = PERF.validate_pair(base, head, BASE, HEAD)
                        self.assertFalse(result["qualified_for_noise"])
                        self.assertEqual(result["eligible_metrics"], {})
                        self.assertEqual(result["fresh_encode_lifecycles"][name], "incompatible")
                        self.assertCountEqual(result["common_cells"], [name, "codec.test"])
                        self.assertTrue(any("fresh encode profiles differ" in e for e in result["errors"]))

    def test_equal_unknown_or_malformed_profiles_never_qualify(self):
        for name in FRESH_CELLS:
            for profile in ("future-v2", "", False, 0, [], {}):
                with self.subTest(cell=name, profile=profile):
                    result = PERF.validate_pair(fresh_report(BASE, name, profile),
                                                fresh_report(HEAD, name, profile), BASE, HEAD)
                    self.assertFalse(result["qualified_for_noise"])
                    self.assertEqual(result["eligible_metrics"], {})
                    self.assertTrue(any("unsupported fresh encode profile" in e for e in result["errors"]))

    def test_lifecycle_metadata_is_rejected_on_unrelated_cells(self):
        for name in ("codec.pbrs.cached_encode", "rpc.native.unary",
                     "codec.adoption.pbrs.entities.n10.fresh_encode"):
            base, head = report(BASE), report(HEAD)
            for item in (base, head):
                item["cells"][0]["id"] = name
            self.assertTrue(PERF.validate_pair(base, head, BASE, HEAD)["qualified_for_noise"])
            for item in (base, head):
                item["cells"][0]["fresh_encode_profile"] = FRESH_PROFILE
            result = PERF.validate_pair(base, head, BASE, HEAD)
            self.assertFalse(result["qualified_for_noise"])
            self.assertEqual(result["eligible_metrics"], {})
            self.assertTrue(any("unexpected fresh encode profile" in e for e in result["errors"]))

    def test_unknown_profile_on_new_cell_rejects_the_entire_pair(self):
        base, head = report(BASE), report(HEAD)
        name = FRESH_CELLS[0]
        head["cells"].extend(fresh_report(HEAD, name, "future-v2")["cells"])
        result = PERF.validate_pair(base, head, BASE, HEAD)
        self.assertFalse(result["qualified_for_noise"])
        self.assertEqual(result["eligible_metrics"], {})
        self.assertEqual(result["new_cells"], [name])

    def test_cli_mixed_lifecycles_retains_an_unqualified_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = root / "base.json"
            head = root / "head.json"
            base.write_text(json.dumps(fresh_report(BASE, FRESH_CELLS[0])))
            head.write_text(json.dumps(fresh_report(HEAD, FRESH_CELLS[0], FRESH_PROFILE)))
            output, summary = root / "validation.json", root / "summary.md"
            run = subprocess.run([
                sys.executable, str(SCRIPT), "verify", "--base", str(base), "--head", str(head),
                "--base-sha", BASE, "--head-sha", HEAD, "--out", str(output), "--summary", str(summary),
            ], capture_output=True, text=True)
            self.assertEqual(run.returncode, 1, run.stderr)
            result = json.loads(output.read_text())
            self.assertFalse(result["qualified_for_noise"])
            self.assertEqual(result["eligible_metrics"], {})
            self.assertIn("fresh encode profiles differ", summary.read_text())
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
