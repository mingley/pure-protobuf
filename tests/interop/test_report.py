#!/usr/bin/env python3
"""Unit tests for scripts/interop-report.py.

Covers:
  - Valid result processing and report generation
  - Rejecting missing and duplicate matrix rows
  - Rejecting wrong peer and procedure source pins
  - Rejecting empty results output
  - Detecting skipped required cases
  - Detecting retries that hide first failures
  - Aggregation failing closed on test failure, missing required results, or self-test substitution
  - CLI commands and exit codes (0 = pass, 1 = test fail, 2 = validation fail, 3 = usage/IO fail)
"""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

# Ensure scripts/interop-report.py is loaded cleanly
REPO_ROOT = Path(__file__).resolve().parent.parent.parent
SCRIPT_PATH = REPO_ROOT / "scripts" / "interop-report.py"
CASES_PATH = REPO_ROOT / "tests" / "interop" / "cases.json"

spec = importlib.util.spec_from_file_location("interop_report", SCRIPT_PATH)
interop_report = importlib.util.module_from_spec(spec)
sys.modules["interop_report"] = interop_report
spec.loader.exec_module(interop_report)

from interop_report import (
    EXIT_SUCCESS,
    EXIT_TEST_FAILURE,
    EXIT_USAGE_ERROR,
    EXIT_VALIDATION_ERROR,
    AggregatedReport,
    CasesRegistry,
    DuplicateMatrixRowError,
    EmptyResultsError,
    ExecutionStatus,
    MissingCaseError,
    MissingMatrixRowError,
    PeerSubstitutionError,
    ReportAggregator,
    ReportValidator,
    ResultWriter,
    RetryFailureError,
    TestAttempt,
    TestResult,
    ValidationReport,
    WrongPinError,
    WrongSourcePinError,
    load_results_file,
)


class BaseReportTest(unittest.TestCase):
    """Base fixture providing access to the authoritative cases.json registry."""

    @classmethod
    def setUpClass(cls):
        cls.registry = CasesRegistry.load(CASES_PATH)
        cls.grpc_go_pin = cls.registry.upstream_pins["grpc_go"]["commit"]
        cls.protobuf_pin = cls.registry.upstream_pins["protobuf"]["commit"]
        cls.grpc_pin = cls.registry.upstream_pins["grpc"]["commit"]

    def make_valid_case_result(
        self,
        case: str = "empty_unary",
        peer: str = "grpc-go",
        direction: str = "kernel_client_to_go_server",
        transport: str = "http2_cleartext",
        status: str = "passed",
        duration_ms: float = 15.0,
        attempt_count: int = 1,
        peer_pin: str | None = None,
        procedure_source: str | None = None,
        attempts: list[TestAttempt] | None = None,
    ) -> TestResult:
        case_def = self.registry.cases_by_name[case]
        return TestResult(
            case=case,
            status=status,
            duration_ms=duration_ms,
            peer=peer,
            direction=direction,
            transport=transport,
            stdout_log=f"logs/{case}_{direction}.stdout",
            stderr_log=f"logs/{case}_{direction}.stderr",
            attempt_count=attempt_count,
            attempts=attempts or [],
            exit_code=0 if status == "passed" else 1,
            suite=case_def.get("suite"),
            profile=case_def.get("profile"),
            peer_pin=peer_pin or (self.grpc_go_pin if "go" in peer else None),
            procedure_source=procedure_source or case_def.get("procedure_source"),
        )


class TestCasesRegistry(BaseReportTest):
    """Verify internal validation of tests/interop/cases.json."""

    def test_authoritative_registry_schema_valid(self):
        errs = self.registry.validate_registry_schema()
        self.assertEqual(errs, [], f"Authoritative cases.json has schema errors: {errs}")
        self.assertEqual(len(self.registry.cases_list), 69)
        self.assertIn("standard_interop", self.registry.suites)
        data = json.loads(CASES_PATH.read_text(encoding="utf-8"))
        schema_path = CASES_PATH.parent / data["$schema"]
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        self.assertEqual(
            set(schema["$defs"]["status"]["enum"]),
            {status.value for status in ExecutionStatus},
        )

    def test_original_upstream_proofs_are_not_replaced_by_local_adapters(self):
        for case in self.registry.cases_list:
            if case["suite"] == "http2_negative":
                self.assertEqual(case["disposition"], "not_run", case["case"])
                self.assertEqual(case["present_coverage"]["status"], "passed")
                self.assertEqual(case["present_coverage"]["evidence_scope"], "local_adapter")
            elif case["suite"] == "server_probe":
                self.assertEqual(case["disposition"], "failed", case["case"])
                self.assertEqual(case["present_coverage"]["status"], "passed")
                self.assertEqual(case["present_coverage"]["evidence_scope"], "local_adapter")
            elif case["suite"] == "soak":
                self.assertEqual(case["disposition"], "not_run", case["case"])
                self.assertEqual(case["present_coverage"]["status"], "passed")
                self.assertEqual(case["present_coverage"]["evidence_scope"], "local_adapter")

    def test_registry_summary_rejects_stale_disposition_and_suite_counts(self):
        for summary_key, item in (("by_disposition", "passed"), ("by_suite", "soak")):
            with self.subTest(summary_key=summary_key):
                data = json.loads(CASES_PATH.read_text(encoding="utf-8"))
                data["summary"][summary_key][item] += 1
                errors = CasesRegistry(data).validate_registry_schema()
                self.assertTrue(any(summary_key in error for error in errors), errors)

    def test_registry_rejects_unproven_pass_and_invalid_coverage(self):
        for status, evidence in (("passed", None), ("unknown", "missing.py")):
            with self.subTest(status=status):
                data = json.loads(CASES_PATH.read_text(encoding="utf-8"))
                data["cases"][0]["present_coverage"]["status"] = status
                data["cases"][0]["present_coverage"]["evidence_file"] = evidence
                errors = CasesRegistry(data).validate_registry_schema()
                self.assertTrue(any("coverage" in error for error in errors), errors)

    def test_registry_rejects_local_pass_without_explicit_scope(self):
        data = json.loads(CASES_PATH.read_text(encoding="utf-8"))
        local = next(c for c in data["cases"] if c["case"] == "server_tls_probe")
        del local["present_coverage"]["evidence_scope"]
        errors = CasesRegistry(data).validate_registry_schema()
        self.assertTrue(any("does not match disposition" in error for error in errors), errors)


class TestValidResultProcessing(BaseReportTest):
    """Verify end-to-end processing of valid test execution results."""

    def test_valid_results_processing_and_report_generation(self):
        results = [
            self.make_valid_case_result("empty_unary", "grpc-go", "kernel_client_to_go_server"),
            self.make_valid_case_result("large_unary", "grpc-go", "kernel_client_to_go_server"),
            self.make_valid_case_result("client_streaming", "grpc-go", "kernel_client_to_go_server"),
        ]

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate(results=results, strict_retries=True)

        self.assertTrue(report.overall_passed)
        self.assertEqual(report.overall_status, "passed")
        self.assertEqual(report.failure_reasons, [])
        self.assertEqual(report.summary.passed, 3)
        self.assertEqual(report.summary.failed, 0)
        self.assertEqual(report.summary.total_evaluated, 3)

        report_dict = report.to_dict()
        self.assertEqual(report_dict["report_version"], "1.0.0")
        self.assertIn("upstream_pins", report_dict)
        self.assertIn("grpc_go", report_dict["upstream_pins"])
        self.assertIn("standard_interop", report_dict["by_suite"])

        with tempfile.TemporaryDirectory() as tmp_dir:
            out_path = Path(tmp_dir) / "report.json"
            report.write_json(out_path)
            self.assertTrue(out_path.exists())
            with out_path.open("r", encoding="utf-8") as f:
                saved_json = json.load(f)
            self.assertEqual(saved_json["overall_status"], "passed")
            self.assertEqual(len(saved_json["results"]), 3)
            self.assertEqual(saved_json["results"][0]["exit_code"], 0)
            self.assertIn(".stdout", saved_json["results"][0]["stdout_log"])

        md = report.to_markdown()
        self.assertIn("# gRPC and Protobuf Interoperability Report", md)
        self.assertIn("PASSED", md)
        self.assertIn("empty_unary", md)
        self.assertIn("[stdout]", md)

        term = report.to_terminal(use_color=False)
        self.assertIn("=== gRPC and Protobuf Interoperability Report ===", term)
        self.assertIn("PASSED", term)


class TestMatrixAndPinRejection(BaseReportTest):
    """Verify rejecting empty output, duplicate rows, missing matrix rows, and wrong pins."""

    def test_local_pass_cannot_replace_unqualified_original(self):
        validator = ReportValidator(self.registry)
        for case in ("ping", "server_tls_probe", "rpc_soak"):
            with self.subTest(case=case):
                result = self.make_valid_case_result(
                    case, peer="local-peer", direction="client_to_server"
                )
                errors = validator.validate_results([result]).errors
                self.assertTrue(
                    any("cannot be reported passed" in error for error in errors),
                    errors,
                )

    def test_spec_adapter_reports_pass_only_with_explicit_unqualified_scope(self):
        validator = ReportValidator(self.registry)
        for suite, case, peer in (
            ("http2_negative", "ping", "local-http2-peer"),
            ("http2_negative", "ping", "external-http2-peer"),
            ("server_probe", "server_tls_probe", "local-native-server"),
        ):
            with self.subTest(suite=suite, peer=peer):
                result = self.make_valid_case_result(
                    case, peer=peer, direction="client_to_server"
                )
                kwargs = {"suite": suite, "profile": "native", "spec_adapter": True}
                self.assertTrue(validator.validate_results([result], **kwargs).is_valid)
                report = ReportAggregator(self.registry).aggregate([result], **kwargs)
                self.assertTrue(report.overall_passed)
                self.assertEqual(report.to_dict()["evidence_scope"], "spec_derived_adapter")
                self.assertEqual(report.to_dict()["qualification"]["qualified"], False)
                self.assertIn("NOT QUALIFIED", report.to_terminal(use_color=False))

                result.peer = "pbrs-grpc"
                self.assertFalse(validator.validate_results([result], **kwargs).is_valid)
                result.peer = peer
                self.assertFalse(
                    validator.validate_results(
                        [result], profile="native", spec_adapter=True
                    ).is_valid
                )
                self.assertFalse(
                    validator.validate_results(
                        [result], require_all_cases=True, **kwargs
                    ).is_valid
                )

    def test_full_profile_fails_closed_on_upstream_blockers(self):
        result = self.make_valid_case_result()
        validator = ReportValidator(self.registry)
        profile = validator.validate_results([result], profile="native")
        self.assertFalse(profile.is_valid)
        self.assertIn("server_tls_probe", profile.missing_cases)
        self.assertIn("rpc_soak", profile.missing_cases)
        self.assertTrue(
            any("Unqualified required case 'server_tls_probe'" in e for e in profile.errors)
        )
        report = ReportAggregator(self.registry).aggregate([result], profile="native")
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.by_profile["native"].status, "failed")

        narrow = validator.validate_results(
            [result], suite="standard_interop", profile="native"
        )
        self.assertFalse(narrow.is_valid)
        self.assertFalse(
            any("server_tls_probe" in e for e in narrow.errors),
            "suite-specific CI validation must not accidentally become a full-profile gate",
        )

    def test_scoped_suite_cannot_pass_from_an_unrelated_result(self):
        result = self.make_valid_case_result()
        validator = ReportValidator(self.registry)
        scoped = validator.validate_results(
            [result], suite="server_probe", profile="native"
        )
        self.assertFalse(scoped.is_valid)
        self.assertTrue(
            any("No passing results for selected suite/profile" in e for e in scoped.errors)
        )
        report = ReportAggregator(self.registry).aggregate(
            [result], suite="server_probe", profile="native"
        )
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.by_suite["server_probe"].status, "failed")

    def test_reject_empty_output(self):
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([])
        self.assertFalse(val_report.is_valid)
        self.assertTrue(any("Empty output" in err for err in val_report.errors))

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([])
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")
        self.assertTrue(any("Empty" in reason for reason in report.failure_reasons))

    def test_reject_duplicate_matrix_rows(self):
        res1 = self.make_valid_case_result("empty_unary", "grpc-go", "kernel_client_to_go_server")
        res2 = self.make_valid_case_result("empty_unary", "grpc-go", "kernel_client_to_go_server")

        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res1, res2])
        self.assertFalse(val_report.is_valid)
        self.assertEqual(len(val_report.duplicate_rows), 1)
        self.assertTrue(any("Duplicate matrix row" in err for err in val_report.errors))

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([res1, res2])
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")

    def test_reject_missing_matrix_rows(self):
        # empty_unary requires kernel_client_to_kernel_server, kernel_client_to_go_server, and go_client_to_kernel_server
        res = self.make_valid_case_result("empty_unary", "grpc-go", "kernel_client_to_go_server")

        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res], require_matrix=True)
        self.assertFalse(val_report.is_valid)
        self.assertTrue(len(val_report.missing_matrix_rows) > 0)
        self.assertTrue(any("Missing required matrix row" in err for err in val_report.errors))

    def test_require_only_the_cpp_peer_directions(self):
        directions = {"kernel_client_to_cpp_server", "cpp_client_to_kernel_server"}
        cases = [
            c["case"]
            for c in self.registry.cases_list
            if c["suite"] == "compression_interop" and c["disposition"] == "passed"
        ]
        results = [
            self.make_valid_case_result(case, "grpc-cpp", direction, peer_pin=self.grpc_pin)
            for case in cases
            for direction in sorted(directions)
        ]
        validator = ReportValidator(self.registry)
        kwargs = {
            "suite": "compression_interop",
            "profile": "native",
            "require_matrix": True,
            "required_directions": directions,
        }
        self.assertTrue(validator.validate_results(results, **kwargs).is_valid)

        missing = validator.validate_results(results[1:], **kwargs)
        self.assertFalse(missing.is_valid)
        self.assertIn(f"{cases[0]}:{sorted(directions)[0]}", missing.missing_matrix_rows)

        results[0].status = "unsupported"
        unsupported = validator.validate_results(results, **kwargs)
        self.assertFalse(unsupported.is_valid)
        self.assertIn(f"{cases[0]}:{sorted(directions)[0]}", unsupported.missing_matrix_rows)

    def test_reject_unknown_required_direction(self):
        result = self.make_valid_case_result("empty_unary")
        report = ReportValidator(self.registry).validate_results(
            [result], require_matrix=True, required_directions={"not_an_official_direction"}
        )
        self.assertFalse(report.is_valid)
        self.assertTrue(any("Unknown required direction" in err for err in report.errors))

    def test_reject_wrong_peer_pin(self):
        res = self.make_valid_case_result(
            "empty_unary",
            peer="grpc-go",
            peer_pin="0000000000000000000000000000000000000000",
        )
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res])
        self.assertFalse(val_report.is_valid)
        self.assertTrue(len(val_report.wrong_pins) > 0)
        self.assertTrue(any("Wrong peer pin" in err for err in val_report.errors))

    def test_reject_wrong_procedure_source_pin(self):
        res = self.make_valid_case_result(
            "empty_unary",
            procedure_source="grpc/grpc@fakecommit1234:doc/invalid.md",
        )
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res])
        self.assertFalse(val_report.is_valid)
        self.assertTrue(len(val_report.wrong_pins) > 0)
        self.assertTrue(any("Wrong procedure source pin" in err for err in val_report.errors))

    def test_reject_wrong_metadata_peer_pins(self):
        res = self.make_valid_case_result("empty_unary")
        validator = ReportValidator(self.registry)
        metadata = {"peer_pins": {"grpc_go": "bad_sha_xyz"}}
        val_report = validator.validate_results([res], metadata=metadata)
        self.assertFalse(val_report.is_valid)
        self.assertTrue(any("Top-level peer pin mismatch" in err for err in val_report.errors))


    def test_reject_unknown_case(self):
        res = TestResult(
            case="non_existent_case",
            status="passed",
            duration_ms=10.0,
            peer="pbrs-grpc",
            direction="kernel_client_to_kernel_server",
            transport="http2_cleartext",
        )
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res])
        self.assertFalse(val_report.is_valid)
        self.assertTrue(any("unknown case" in err for err in val_report.errors))


class TestRetriesAndSkippedCases(BaseReportTest):
    """Verify detecting skipped required cases and retries that hide first failures."""

    def test_detect_skipped_required_cases(self):
        res = self.make_valid_case_result("empty_unary", status="not_run")
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res])
        self.assertFalse(val_report.is_valid)
        self.assertTrue(any("Skipped required case" in err for err in val_report.errors))

    def test_detect_retries_that_hide_first_failures_strict(self):
        attempts = [
            TestAttempt(attempt=1, status="failed", duration_ms=20.0, exit_code=1, stderr_log="logs/att1.err"),
            TestAttempt(attempt=2, status="passed", duration_ms=25.0, exit_code=0, stdout_log="logs/att2.out"),
        ]
        res = self.make_valid_case_result(
            case="cancel_after_begin",
            status="passed",
            attempt_count=2,
            attempts=attempts,
        )
        self.assertTrue(res.is_flaky)

        # In strict qualification mode, hiding a first failure is an error
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res], strict_retries=True)
        self.assertFalse(val_report.is_valid)
        self.assertIn("cancel_after_begin", val_report.flaky_cases)
        self.assertTrue(any("Retry hid first failure" in err for err in val_report.errors))

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([res], strict_retries=True)
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")

    def test_retries_visible_in_non_strict_mode(self):
        attempts = [
            TestAttempt(attempt=1, status="failed", duration_ms=20.0, exit_code=1),
            TestAttempt(attempt=2, status="passed", duration_ms=25.0, exit_code=0),
        ]
        res = self.make_valid_case_result(
            case="cancel_after_begin",
            status="passed",
            attempt_count=2,
            attempts=attempts,
        )
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([res], strict_retries=False)
        self.assertTrue(val_report.is_valid)
        self.assertIn("cancel_after_begin", val_report.flaky_cases)
        self.assertTrue(any("retried" in w for w in val_report.warnings))

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([res], strict_retries=False)
        self.assertTrue(report.overall_passed)
        self.assertEqual(report.summary.flaky_or_retried, 1)


class TestFailClosedAggregation(BaseReportTest):
    """Verify aggregation fails closed on test failure, missing results, or self-test substitution."""

    def test_aggregation_fails_closed_on_test_failure(self):
        res = self.make_valid_case_result("empty_unary", status="failed")
        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([res])
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")
        self.assertTrue(any("reported status 'failed'" in r for r in report.failure_reasons))

    def test_aggregation_fails_closed_on_missing_required_case(self):
        # Suite standard_interop has 16 cases; providing only 1 must fail when suite is target
        res = self.make_valid_case_result("empty_unary")
        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([res], suite="standard_interop")
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")
        self.assertTrue(any("Missing required case" in r for r in report.failure_reasons))

    def test_never_substitute_self_test_for_independent_peer(self):
        # Case large_unary has peer_direction: "both"
        # Providing only self-test (kernel_client_to_kernel_server) must fail when require_peers=True
        self_res = self.make_valid_case_result(
            case="large_unary",
            peer="pbrs-grpc",
            direction="kernel_client_to_kernel_server",
        )
        validator = ReportValidator(self.registry)
        val_report = validator.validate_results([self_res], require_peers=True)
        self.assertFalse(val_report.is_valid)
        self.assertTrue(any("Self-test substitution violation" in err for err in val_report.errors))

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate([self_res], require_peers=True)
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")


class TestResultWriter(BaseReportTest):
    """Verify ResultWriter persistence and round-tripping."""

    def test_write_and_read_results(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            file_path = Path(tmp_dir) / "results.json"
            writer = ResultWriter(file_path)

            res1 = self.make_valid_case_result("empty_unary", "grpc-go", "kernel_client_to_go_server")
            res2 = self.make_valid_case_result("large_unary", "grpc-go", "kernel_client_to_go_server")
            writer.add_result(res1)
            writer.add_result(res2)
            writer.write()

            loaded_results, metadata = load_results_file(file_path)
            self.assertEqual(len(loaded_results), 2)
            self.assertEqual(loaded_results[0].case, "empty_unary")
            self.assertEqual(loaded_results[1].case, "large_unary")

            # Overwrite an existing matrix coordinate
            res1_updated = self.make_valid_case_result(
                "empty_unary",
                "grpc-go",
                "kernel_client_to_go_server",
                duration_ms=42.0,
            )
            writer.add_result(res1_updated)
            writer.write()

            reloaded, _ = load_results_file(file_path)
            self.assertEqual(len(reloaded), 2)
            self.assertEqual(reloaded[0].duration_ms, 42.0)


class TestCLIIntegration(BaseReportTest):
    """Verify CLI subcommands, arguments, and exit codes."""

    def run_cli(self, args: list[str]) -> subprocess.CompletedProcess:
        cmd = [sys.executable, str(SCRIPT_PATH)] + args
        return subprocess.run(cmd, capture_output=True, text=True)

    def test_cli_record_and_aggregate_success(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            results_path = Path(tmp_dir) / "results.json"
            report_path = Path(tmp_dir) / "report.json"

            # Record a case
            rec_proc = self.run_cli([
                "record",
                "--output", str(results_path),
                "--case", "empty_unary",
                "--status", "passed",
                "--duration-ms", "12.5",
                "--peer", "grpc-go",
                "--direction", "kernel_client_to_go_server",
                "--transport", "http2_cleartext",
                "--stdout-log", "logs/stdout.log",
                "--stderr-log", "logs/stderr.log",
                "--exit-code", "0",
            ])
            self.assertEqual(rec_proc.returncode, EXIT_SUCCESS)
            self.assertTrue(results_path.exists())

            # Validate
            val_proc = self.run_cli([
                "validate",
                "--cases", str(CASES_PATH),
                "--results", str(results_path),
            ])
            self.assertEqual(val_proc.returncode, EXIT_SUCCESS)

            # Aggregate
            agg_proc = self.run_cli([
                "aggregate",
                "--cases", str(CASES_PATH),
                "--results", str(results_path),
                "--output", str(report_path),
                "--format", "markdown",
            ])
            self.assertEqual(agg_proc.returncode, EXIT_SUCCESS)
            self.assertTrue(report_path.exists())
            self.assertIn("# gRPC and Protobuf Interoperability Report", agg_proc.stdout)

    def test_cli_exit_codes(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            # 1. Missing input file -> EXIT_USAGE_ERROR (3)
            proc = self.run_cli(["aggregate", "--results", "/non/existent/results.json"])
            self.assertEqual(proc.returncode, EXIT_USAGE_ERROR)

            # 2. Test failure -> EXIT_TEST_FAILURE (1)
            fail_results = Path(tmp_dir) / "fail_results.json"
            writer = ResultWriter(fail_results)
            writer.add_result(self.make_valid_case_result("empty_unary", status="failed"))
            writer.write()

            proc = self.run_cli(["aggregate", "--cases", str(CASES_PATH), "--results", str(fail_results)])
            self.assertEqual(proc.returncode, EXIT_TEST_FAILURE)

            # 3. Validation / matrix error -> EXIT_VALIDATION_ERROR (2)
            dup_results = Path(tmp_dir) / "dup_results.json"
            writer = ResultWriter(dup_results)
            writer.add_result(self.make_valid_case_result("empty_unary"))
            # Manually append duplicate matrix row
            writer.results.append(self.make_valid_case_result("empty_unary"))
            writer.write()

            proc = self.run_cli(["validate", "--cases", str(CASES_PATH), "--results", str(dup_results)])
            self.assertEqual(proc.returncode, EXIT_VALIDATION_ERROR)

    def test_cli_profile_and_require_all_reject_missing_upstream_proof(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            results_path = Path(tmp_dir) / "results.json"
            writer = ResultWriter(results_path)
            writer.add_result(self.make_valid_case_result())
            writer.write()
            for args in (
                ["validate", "--profile", "native"],
                ["aggregate", "--profile", "native"],
                ["--require-all", "validate", "--suite", "server_probe"],
                ["validate", "--require-all", "--suite", "server_probe"],
            ):
                with self.subTest(args=args):
                    proc = self.run_cli(args + ["--cases", str(CASES_PATH), "--results", str(results_path)])
                    self.assertEqual(proc.returncode, EXIT_VALIDATION_ERROR, proc.stderr)
                    self.assertIn("server_tls_probe", proc.stdout + proc.stderr)

    def test_cli_spec_adapter_cannot_be_reused_as_upstream_pass(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            results_path = Path(tmp_dir) / "results.json"
            writer = ResultWriter(results_path)
            writer.add_result(
                self.make_valid_case_result(
                    "ping", peer="local-http2-peer", direction="client_to_server"
                )
            )
            writer.write()
            base = [
                "aggregate", "--cases", str(CASES_PATH), "--results", str(results_path),
                "--suite", "http2_negative", "--profile", "native", "--format", "json",
            ]
            local = self.run_cli(base + ["--spec-adapter"])
            self.assertEqual(local.returncode, EXIT_SUCCESS, local.stderr)
            self.assertFalse(json.loads(local.stdout)["qualification"]["qualified"])

            original = self.run_cli(base)
            self.assertEqual(original.returncode, EXIT_VALIDATION_ERROR)
            self.assertIn("cannot be reported passed", original.stdout)

    def test_cli_aggregate_rejects_stale_registry_summary(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            results_path = Path(tmp_dir) / "results.json"
            registry_path = Path(tmp_dir) / "cases.json"
            writer = ResultWriter(results_path)
            writer.add_result(self.make_valid_case_result())
            writer.write()
            data = json.loads(CASES_PATH.read_text(encoding="utf-8"))
            data["summary"]["by_disposition"]["passed"] += 1
            registry_path.write_text(json.dumps(data), encoding="utf-8")
            proc = self.run_cli([
                "aggregate", "--cases", str(registry_path),
                "--results", str(results_path),
            ])
            self.assertEqual(proc.returncode, EXIT_VALIDATION_ERROR)
            self.assertIn("Summary by_disposition", proc.stderr)

    def test_cli_stdin_input_and_json_format(self):
        res = self.make_valid_case_result("empty_unary")
        input_json = json.dumps({"results": [res.to_dict()]})

        cmd = [sys.executable, str(SCRIPT_PATH), "--cases", str(CASES_PATH), "--results", "-", "--format", "json"]
        proc = subprocess.run(cmd, input=input_json, capture_output=True, text=True)
        self.assertEqual(proc.returncode, EXIT_SUCCESS)
        parsed = json.loads(proc.stdout)
        self.assertEqual(parsed["overall_status"], "passed")
        self.assertEqual(parsed["summary"]["passed"], 1)


class TestVerificationStatuses(BaseReportTest):
    """Verify distinct pass, failure, not-run, unsupported, external-blocked, and not-applicable states."""

    def test_all_six_dispositions_tracked_distinctly(self):
        statuses = [
            ("passed", ExecutionStatus.PASSED),
            ("failed", ExecutionStatus.FAILED),
            ("not_run", ExecutionStatus.NOT_RUN),
            ("unsupported", ExecutionStatus.UNSUPPORTED),
            ("blocked_external", ExecutionStatus.BLOCKED_EXTERNAL),
            ("not_applicable", ExecutionStatus.NOT_APPLICABLE),
        ]
        for s_str, expected_enum in statuses:
            self.assertEqual(ExecutionStatus.normalize(s_str), expected_enum)

        # Build a heterogeneous result set across different cases in cases.json
        results = [
            self.make_valid_case_result("empty_unary", status="passed"),
            self.make_valid_case_result("large_unary", status="failed"),
            self.make_valid_case_result("client_streaming", status="not_run"),
            self.make_valid_case_result("alts_credentials", status="unsupported"),
            self.make_valid_case_result("compute_engine_creds", status="blocked_external"),
            self.make_valid_case_result("upb_kernel_internals", status="not_applicable"),
        ]

        aggregator = ReportAggregator(self.registry)
        report = aggregator.aggregate(results, strict_retries=False)

        # Should fail closed because failed > 0
        self.assertFalse(report.overall_passed)
        self.assertEqual(report.overall_status, "failed")
        self.assertEqual(report.summary.passed, 1)
        self.assertEqual(report.summary.failed, 1)
        self.assertEqual(report.summary.not_run, 1)
        self.assertEqual(report.summary.unsupported, 1)
        self.assertEqual(report.summary.blocked_external, 1)
        self.assertEqual(report.summary.not_applicable, 1)


if __name__ == "__main__":
    unittest.main()
