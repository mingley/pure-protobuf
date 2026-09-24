#!/usr/bin/env python3
"""Unit tests for scripts/check-upstream-cases.py.

Covers:
  - Exact schema compliance of tests/interop/cases.json
  - Schema error detection (missing keys, invalid dispositions, unverified justifications,
    summary count mismatches, duplicate cases)
  - Target inventory ingestion (JSON cases list, string list, suites map, plain text file, CLI)
  - Comparison logic:
      - Exact match (0 additions, 0 deletions, 0 renames, exit code 0)
      - Detecting newly added upstream cases as 'uncovered' (exit code 1)
      - Detecting removed cases as 'missing' (exit code 1)
      - Detecting renames via similarity, normalized naming, and prefix/suffix matching
      - Complex mixed drift (additions, deletions, renames, and matches)
  - CLI execution and report formatting (text, json, markdown, file output)

Run with:
  python3 -m unittest tests/interop/test_upstream_cases.py
"""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any, Dict, List
import unittest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
SCRIPT_PATH = REPO_ROOT / "scripts" / "check-upstream-cases.py"
CASES_PATH = REPO_ROOT / "tests" / "interop" / "cases.json"

# Dynamically import scripts/check-upstream-cases.py
spec = importlib.util.spec_from_file_location("check_upstream_cases", SCRIPT_PATH)
check_upstream_cases = importlib.util.module_from_spec(spec)
sys.modules["check_upstream_cases"] = check_upstream_cases
spec.loader.exec_module(check_upstream_cases)

from check_upstream_cases import (
    DEFAULT_PINNED_CASES,
    EXIT_DRIFT_DETECTED,
    EXIT_SUCCESS,
    EXIT_USAGE_ERROR,
    DriftReport,
    RenameMatch,
    check_upstream_drift,
    detect_renames,
    load_registered_cases,
    load_target_inventory,
    validate_cases_schema,
)


class TestCasesJsonSchema(unittest.TestCase):
    """Verify authoritative schema compliance and schema validator behavior."""

    def setUp(self):
        with CASES_PATH.open("r", encoding="utf-8") as f:
            self.raw_data = json.load(f)

    def test_authoritative_cases_json_conforms_to_schema(self):
        """Authoritative tests/interop/cases.json must pass schema validation with 0 errors."""
        errors = validate_cases_schema(self.raw_data)
        self.assertEqual(errors, [], f"cases.json has schema errors: {errors}")
        self.assertEqual(len(self.raw_data["cases"]), 69)
        self.assertEqual(self.raw_data["summary"]["total_cases"], 69)

    def test_schema_missing_required_top_level_keys(self):
        """Top-level keys must all be present."""
        for key in ["version", "upstream_pins", "disposition_definitions", "suites", "summary", "cases"]:
            mutated = dict(self.raw_data)
            del mutated[key]
            errors = validate_cases_schema(mutated)
            self.assertTrue(any(key in err for err in errors), f"Expected missing '{key}' error, got: {errors}")

    def test_schema_missing_upstream_pins(self):
        """upstream_pins must contain protobuf, grpc, and grpc_go with commit hashes."""
        mutated = dict(self.raw_data)
        mutated["upstream_pins"] = {"protobuf": {"commit": "abc"}}
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("grpc" in err for err in errors))
        self.assertTrue(any("grpc_go" in err for err in errors))

    def test_schema_invalid_disposition(self):
        """Cases with invalid dispositions must fail validation."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["cases"][0]["disposition"] = "unknown_disposition"
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("unknown_disposition" in err for err in errors))

    def test_schema_missing_justification_for_nonpassing_case(self):
        """Every nonpassing original procedure needs an explicit explanation."""
        for disposition in ("failed", "not_run", "unsupported", "blocked_external"):
            with self.subTest(disposition=disposition):
                mutated = json.loads(json.dumps(self.raw_data))
                target = next(c for c in mutated["cases"] if c["disposition"] == disposition)
                target["justification"] = None
                errors = validate_cases_schema(mutated)
                self.assertTrue(any(f"Case '{target['case']}'" in err and "justification" in err for err in errors))

    def test_schema_duplicate_case_identifier(self):
        """Duplicate case identifiers must be flagged as schema errors."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["cases"].append(dict(mutated["cases"][0]))
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("Duplicate case identifier" in err for err in errors))

    def test_schema_invalid_identifier_syntax(self):
        """Case identifiers must contain alphanumeric characters and underscores only."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["cases"][0]["case"] = "invalid-name-with-dashes!"
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("invalid identifier syntax" in err for err in errors))

    def test_schema_undefined_suite(self):
        """Cases referencing undefined suites must trigger errors."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["cases"][0]["suite"] = "nonexistent_suite"
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("nonexistent_suite" in err for err in errors))

    def test_schema_invalid_peer_direction_or_transport(self):
        """Invalid peer_direction and transport enums must be caught."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["cases"][0]["peer_direction"] = "invalid_direction"
        mutated["cases"][0]["transport"] = "invalid_transport"
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("invalid peer_direction" in err for err in errors))
        self.assertTrue(any("invalid transport" in err for err in errors))

    def test_schema_summary_mismatch(self):
        """Mismatched summary counts must be detected."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["summary"]["total_cases"] = 999
        mutated["summary"]["by_disposition"]["passed"] = 999
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("total_cases" in err for err in errors))
        self.assertTrue(any("by_disposition['passed']" in err for err in errors))

    def test_schema_present_coverage_mismatch(self):
        """Unscoped coverage cannot replace an original procedure's disposition."""
        mutated = json.loads(json.dumps(self.raw_data))
        mutated["cases"][0]["present_coverage"]["status"] = "unsupported"  # was passed
        errors = validate_cases_schema(mutated)
        self.assertTrue(any("coverage status 'unsupported' does not match" in err for err in errors))

    def test_schema_local_only_pass_requires_explicit_scope_and_evidence(self):
        for mutation, expected in (
            (lambda coverage: coverage.pop("evidence_scope"), "does not match disposition"),
            (lambda coverage: coverage.update(evidence_scope="unknown"), "invalid evidence_scope"),
            (lambda coverage: coverage.update(evidence_file=None), "without an evidence file"),
            (lambda coverage: coverage.update(passing_directions=[]), "no passing directions"),
        ):
            mutated = json.loads(json.dumps(self.raw_data))
            local = next(c for c in mutated["cases"] if c["case"] == "rst_after_header")
            mutation(local["present_coverage"])
            errors = validate_cases_schema(mutated)
            self.assertTrue(any(expected in error for error in errors), errors)


class TestTargetInventoryLoading(unittest.TestCase):
    """Verify target inventory parsing across JSON, text, and CLI sources."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.work_dir = Path(self.temp_dir.name)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_load_default_pinned_inventory(self):
        """Default target inventory matches the 69 pinned cases."""
        cases, source = load_target_inventory(None)
        self.assertEqual(len(cases), 69)
        self.assertEqual(cases, set(DEFAULT_PINNED_CASES))
        self.assertIn("empty_unary", cases)
        self.assertIn("builtin:", source)

    def test_load_from_json_cases_list(self):
        """JSON with 'cases' list containing dicts."""
        path = self.work_dir / "target.json"
        path.write_text(json.dumps({"cases": [{"case": "case_a"}, {"case": "case_b"}]}), encoding="utf-8")
        cases, source = load_target_inventory(path)
        self.assertEqual(cases, {"case_a", "case_b"})
        self.assertEqual(source, str(path))

    def test_load_from_json_string_list(self):
        """JSON array of case name strings."""
        path = self.work_dir / "target.json"
        path.write_text(json.dumps(["foo_case", "bar_case"]), encoding="utf-8")
        cases, source = load_target_inventory(path)
        self.assertEqual(cases, {"foo_case", "bar_case"})

    def test_load_from_json_suites_dict(self):
        """JSON dict with 'suites' mapping to arrays."""
        path = self.work_dir / "target.json"
        path.write_text(json.dumps({"suites": {"s1": ["c1", "c2"], "s2": ["c3"]}}), encoding="utf-8")
        cases, source = load_target_inventory(path)
        self.assertEqual(cases, {"c1", "c2", "c3"})

    def test_load_from_text_file(self):
        """Plain text file with comments, whitespace, and blank lines."""
        path = self.work_dir / "target.txt"
        path.write_text(
            "# Pinned suite cases\n"
            "empty_unary\n"
            "  large_unary  \n"
            "\n"
            "# Another comment\n"
            "client_streaming\n",
            encoding="utf-8",
        )
        cases, source = load_target_inventory(path)
        self.assertEqual(cases, {"empty_unary", "large_unary", "client_streaming"})

    def test_load_from_comma_separated_string(self):
        """CLI string with comma-separated values."""
        cases, source = load_target_inventory("case_1, case_2,case_3")
        self.assertEqual(cases, {"case_1", "case_2", "case_3"})
        self.assertIn("cli:", source)

    def test_load_nonexistent_file_raises(self):
        """Nonexistent path raises FileNotFoundError."""
        with self.assertRaises(FileNotFoundError):
            load_target_inventory(self.work_dir / "does_not_exist.json")


class TestComparisonLogic(unittest.TestCase):
    """Verify drift detection: additions, deletions, renames, and exact matching."""

    def setUp(self):
        self.registered_cases = {
            "empty_unary": {"case": "empty_unary", "suite": "standard_interop", "disposition": "passed"},
            "large_unary": {"case": "large_unary", "suite": "standard_interop", "disposition": "passed"},
            "client_streaming": {"case": "client_streaming", "suite": "standard_interop", "disposition": "passed"},
            "rst_after_header": {"case": "rst_after_header", "suite": "http2_negative", "disposition": "not_run"},
            "ping_pong": {"case": "ping_pong", "suite": "standard_interop", "disposition": "passed"},
        }

    def test_exact_match_no_drift(self):
        """Exact match reports 0 uncovered, 0 missing, 0 renamed, and exit code 0."""
        target_names = set(self.registered_cases.keys())
        report = check_upstream_drift(self.registered_cases, target_names)

        self.assertFalse(report.has_drift)
        self.assertEqual(report.exit_code, EXIT_SUCCESS)
        self.assertEqual(len(report.matched), 5)
        self.assertEqual(report.uncovered, [])
        self.assertEqual(report.missing, [])
        self.assertEqual(report.renamed, [])

    def test_detect_additions_as_uncovered(self):
        """New upstream cases are flagged as uncovered and exit with code 1."""
        target_names = set(self.registered_cases.keys()) | {"newly_added_rpc", "another_upstream_test"}
        report = check_upstream_drift(self.registered_cases, target_names)

        self.assertTrue(report.has_drift)
        self.assertEqual(report.exit_code, EXIT_DRIFT_DETECTED)
        self.assertEqual(sorted(report.uncovered), ["another_upstream_test", "newly_added_rpc"])
        self.assertEqual(report.missing, [])
        self.assertEqual(report.renamed, [])
        self.assertEqual(len(report.matched), 5)

    def test_detect_deletions_as_missing(self):
        """Cases absent from target upstream are flagged as missing and exit with code 1."""
        # Target only has 3 of the 5 registered cases
        target_names = {"empty_unary", "large_unary", "ping_pong"}
        report = check_upstream_drift(self.registered_cases, target_names)

        self.assertTrue(report.has_drift)
        self.assertEqual(report.exit_code, EXIT_DRIFT_DETECTED)
        self.assertEqual(sorted(report.missing), ["client_streaming", "rst_after_header"])
        self.assertEqual(report.uncovered, [])
        self.assertEqual(report.renamed, [])
        self.assertEqual(len(report.matched), 3)

    def test_detect_renames_by_similarity(self):
        """Cases renamed with high similarity are identified as renames, not plain additions/deletions."""
        # rst_after_header -> rst_after_headers
        # empty_unary -> empty_unary_call
        target_names = {
            "empty_unary_call",
            "large_unary",
            "client_streaming",
            "rst_after_headers",
            "ping_pong",
        }
        report = check_upstream_drift(self.registered_cases, target_names)

        self.assertTrue(report.has_drift)
        self.assertEqual(report.exit_code, EXIT_DRIFT_DETECTED)
        self.assertEqual(report.uncovered, [])
        self.assertEqual(report.missing, [])
        self.assertEqual(len(report.renamed), 2)

        renamed_map = {r.old_case: r.new_case for r in report.renamed}
        self.assertEqual(renamed_map["rst_after_header"], "rst_after_headers")
        self.assertEqual(renamed_map["empty_unary"], "empty_unary_call")
        self.assertEqual(len(report.matched), 3)

    def test_detect_renames_by_normalized_case(self):
        """camelCase vs snake_case renames match with 1.0 similarity."""
        missing = {"empty_unary"}
        added = {"emptyUnary"}
        renames, used_old, used_new = detect_renames(missing, added)

        self.assertEqual(len(renames), 1)
        self.assertEqual(renames[0].old_case, "empty_unary")
        self.assertEqual(renames[0].new_case, "emptyUnary")
        self.assertEqual(renames[0].similarity, 1.0)
        self.assertEqual(renames[0].reason, "normalized_case_match")
        self.assertEqual(used_old, {"empty_unary"})
        self.assertEqual(used_new, {"emptyUnary"})

    def test_detect_renames_by_prefix_suffix(self):
        """Prefix/suffix additions like test_ or _test are recognized as renames."""
        missing = {"client_streaming"}
        added = {"test_client_streaming"}
        renames, used_old, used_new = detect_renames(missing, added)

        self.assertEqual(len(renames), 1)
        self.assertEqual(renames[0].old_case, "client_streaming")
        self.assertEqual(renames[0].new_case, "test_client_streaming")
        self.assertGreaterEqual(renames[0].similarity, 0.90)
        self.assertEqual(renames[0].reason, "prefix_suffix_match")

    def test_detect_complex_drift(self):
        """Simultaneous match, addition, deletion, and rename are cleanly segregated."""
        # Starting with 5 cases:
        # matched: large_unary, ping_pong
        # renamed: empty_unary -> empty_unary_call
        # missing: client_streaming (completely removed)
        # uncovered: brand_new_feature_case (completely new)
        target_names = {
            "large_unary",
            "ping_pong",
            "empty_unary_call",
            "rst_after_header",
            "brand_new_feature_case",
        }
        report = check_upstream_drift(self.registered_cases, target_names)

        self.assertTrue(report.has_drift)
        self.assertEqual(report.exit_code, EXIT_DRIFT_DETECTED)
        self.assertEqual(report.uncovered, ["brand_new_feature_case"])
        self.assertEqual(report.missing, ["client_streaming"])
        self.assertEqual(len(report.renamed), 1)
        self.assertEqual(report.renamed[0].old_case, "empty_unary")
        self.assertEqual(report.renamed[0].new_case, "empty_unary_call")
        self.assertEqual(sorted(report.matched), ["large_unary", "ping_pong", "rst_after_header"])


class TestCliExecution(unittest.TestCase):
    """Verify CLI flags, exit codes, and output serialization."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.work_dir = Path(self.temp_dir.name)

    def tearDown(self):
        self.temp_dir.cleanup()

    def run_script(self, args: List[str]) -> subprocess.CompletedProcess:
        cmd = [sys.executable, str(SCRIPT_PATH)] + args
        return subprocess.run(cmd, cwd=str(REPO_ROOT), capture_output=True, text=True)

    def test_cli_default_run_passes(self):
        """Running scripts/check-upstream-cases.py on authoritative cases.json succeeds with exit code 0."""
        res = self.run_script([])
        self.assertEqual(res.returncode, EXIT_SUCCESS, f"Failed: {res.stdout}\n{res.stderr}")
        self.assertIn("VERDICT: PASSED", res.stdout)
        self.assertIn("Matched Cases:       69", res.stdout)
        self.assertIn("Uncovered (New):      0", res.stdout)

    def test_cli_schema_only_flag(self):
        """--schema-only succeeds and validates schema."""
        res = self.run_script(["--schema-only"])
        self.assertEqual(res.returncode, EXIT_SUCCESS, f"Failed: {res.stdout}\n{res.stderr}")
        self.assertIn("Schema validation PASSED", res.stdout)

    def test_cli_uncovered_drift_fails(self):
        """Passing an inventory with an unclassified case exits with 1 and reports uncovered."""
        target_file = self.work_dir / "target.txt"
        target_file.write_text(
            "\n".join(DEFAULT_PINNED_CASES + ["synthetic_new_upstream_case"]),
            encoding="utf-8",
        )

        res = self.run_script(["--upstream", str(target_file)])
        self.assertEqual(res.returncode, EXIT_DRIFT_DETECTED)
        self.assertIn("UNCOVERED NEW UPSTREAM CASES", res.stdout)
        self.assertIn("[+] synthetic_new_upstream_case", res.stdout)
        self.assertIn("VERDICT: FAILED", res.stdout)

    def test_cli_renamed_drift_fails(self):
        """Passing an inventory with a renamed case exits with 1 and reports renamed."""
        modified_cases = [c if c != "empty_unary" else "empty_unary_call" for c in DEFAULT_PINNED_CASES]
        target_file = self.work_dir / "target.json"
        target_file.write_text(json.dumps(modified_cases), encoding="utf-8")

        res = self.run_script(["--upstream", str(target_file)])
        self.assertEqual(res.returncode, EXIT_DRIFT_DETECTED)
        self.assertIn("RENAMED CASES", res.stdout)
        self.assertIn("'empty_unary' -> 'empty_unary_call'", res.stdout)

    def test_cli_json_format(self):
        """--format json produces parseable machine-readable JSON."""
        res = self.run_script(["--format", "json"])
        self.assertEqual(res.returncode, EXIT_SUCCESS)
        data = json.loads(res.stdout)
        self.assertIn("has_drift", data)
        self.assertFalse(data["has_drift"])
        self.assertEqual(data["exit_code"], 0)
        self.assertEqual(data["summary"]["matched_count"], 69)
        self.assertEqual(data["summary"]["uncovered_count"], 0)

    def test_cli_markdown_format(self):
        """--format markdown produces markdown tables."""
        res = self.run_script(["--format", "markdown"])
        self.assertEqual(res.returncode, EXIT_SUCCESS)
        self.assertIn("### Upstream Case Drift Report: PASSED", res.stdout)
        self.assertIn("| **Registered Cases** | 69 |", res.stdout)

    def test_cli_output_to_file(self):
        """--output writes the report to a destination file."""
        out_file = self.work_dir / "output-report.json"
        res = self.run_script(["--format", "json", "--output", str(out_file)])
        self.assertEqual(res.returncode, EXIT_SUCCESS)
        self.assertTrue(out_file.exists())
        data = json.loads(out_file.read_text(encoding="utf-8"))
        self.assertEqual(data["summary"]["matched_count"], 69)


if __name__ == "__main__":
    unittest.main()
