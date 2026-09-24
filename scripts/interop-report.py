#!/usr/bin/env python3
"""Machine-readable proof, validation, and aggregation for gRPC and Protobuf interop testing.

Standard-library-only Python 3 tool (no pip dependencies) that reads, writes,
validates, and aggregates test execution results against tests/interop/cases.json.

Verification states supported:
  - passed: Verified passing in official test scripts, peer passes, or conformance harness.
  - failed: Test execution failed, exited non-zero, or violated protocol assertion.
  - not_run: Active upstream case scheduled for execution, not yet run.
  - unsupported: Protocol feature, RPC pattern, or service not yet implemented in target profile.
  - blocked_external: Blocked by external infrastructure/credentials (e.g. cloud IAM, metadata server).
  - not_applicable: Explicitly excluded upstream test with approved, documented technical justification.
"""

from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from enum import Enum
import json
import os
from pathlib import Path
import sys
from typing import Any, Dict, List, Optional, Set, Tuple, Union

# Exit codes
EXIT_SUCCESS = 0
EXIT_TEST_FAILURE = 1
EXIT_VALIDATION_ERROR = 2
EXIT_USAGE_ERROR = 3


class ExecutionStatus(str, Enum):
    """Allowed verification and execution states."""

    PASSED = "passed"
    FAILED = "failed"
    NOT_RUN = "not_run"
    UNSUPPORTED = "unsupported"
    BLOCKED_EXTERNAL = "blocked_external"
    NOT_APPLICABLE = "not_applicable"

    @classmethod
    def normalize(cls, val: str) -> "ExecutionStatus":
        """Normalize case-insensitive strings and common aliases."""
        if not isinstance(val, str):
            raise ValueError(f"Execution status must be a string, got {type(val).__name__}")
        v = val.strip().lower()
        if v in ("passed", "pass", "ok", "success"):
            return cls.PASSED
        if v in ("failed", "failure", "fail", "error"):
            return cls.FAILED
        if v in ("not_run", "not-run", "notrun", "skip", "skipped"):
            return cls.NOT_RUN
        if v in ("unsupported",):
            return cls.UNSUPPORTED
        if v in ("blocked_external", "external_blocked", "blocked", "blocked-external"):
            return cls.BLOCKED_EXTERNAL
        if v in ("not_applicable", "na", "n/a", "not-applicable"):
            return cls.NOT_APPLICABLE
        raise ValueError(
            f"Invalid execution status: '{val}'. Must be one of: "
            f"{', '.join(s.value for s in cls)}"
        )


# Custom exceptions
class ReportError(Exception):
    """Base exception for interop report operations."""

    exit_code: int = EXIT_VALIDATION_ERROR


class EmptyResultsError(ReportError):
    """Raised when the results collection is empty."""

    pass


class DuplicateMatrixRowError(ReportError):
    """Raised when duplicate matrix rows are detected."""

    pass


class MissingMatrixRowError(ReportError):
    """Raised when required matrix directions/rows are missing."""

    pass


class MissingCaseError(ReportError):
    """Raised when a required case is missing."""

    pass


class WrongPinError(ReportError):
    """Raised when a peer pin does not match pinned upstream commit."""

    pass


class WrongSourcePinError(ReportError):
    """Raised when procedure source does not match cases.json."""

    pass


class RetryFailureError(ReportError):
    """Raised when retries hide first failures during qualification."""

    pass


class PeerSubstitutionError(ReportError):
    """Raised when a self-test is substituted for an independent peer."""

    pass


class InvalidRegistryError(ReportError):
    """Raised when cases.json registry fails schema validation."""

    pass


@dataclass
class TestAttempt:
    """Individual attempt record for an execution of a test case."""

    attempt: int
    status: str
    duration_ms: float = 0.0
    exit_code: Optional[int] = None
    stdout_log: Optional[str] = None
    stderr_log: Optional[str] = None
    error_message: Optional[str] = None

    def __post_init__(self):
        self.status = ExecutionStatus.normalize(self.status).value

    def to_dict(self) -> Dict[str, Any]:
        return {
            "attempt": self.attempt,
            "status": self.status,
            "duration_ms": self.duration_ms,
            "exit_code": self.exit_code,
            "stdout_log": self.stdout_log,
            "stderr_log": self.stderr_log,
            "error_message": self.error_message,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "TestAttempt":
        return cls(
            attempt=int(data.get("attempt", 1)),
            status=str(data.get("status", "passed")),
            duration_ms=float(data.get("duration_ms", 0.0)),
            exit_code=data.get("exit_code"),
            stdout_log=data.get("stdout_log"),
            stderr_log=data.get("stderr_log"),
            error_message=data.get("error_message"),
        )


@dataclass
class TestResult:
    """Execution result for a single case under a specific matrix coordinate."""

    case: str
    status: str
    duration_ms: float
    peer: str
    direction: str
    transport: str
    stdout_log: Optional[str] = None
    stderr_log: Optional[str] = None
    attempt_count: int = 1
    attempts: List[TestAttempt] = field(default_factory=list)
    exit_code: Optional[int] = None
    suite: Optional[str] = None
    profile: Optional[str] = None
    peer_pin: Optional[str] = None
    procedure_source: Optional[str] = None
    notes: Optional[str] = None
    error_message: Optional[str] = None
    first_attempt_status: Optional[str] = None
    is_flaky: bool = False

    def __post_init__(self):
        self.status = ExecutionStatus.normalize(self.status).value
        if self.first_attempt_status:
            self.first_attempt_status = ExecutionStatus.normalize(self.first_attempt_status).value

        # Populate attempts if empty and attempt_count is specified
        if not self.attempts and self.attempt_count > 0:
            if self.first_attempt_status and self.attempt_count > 1:
                # We know first attempt had distinct status
                self.attempts = [
                    TestAttempt(
                        attempt=1,
                        status=self.first_attempt_status,
                        duration_ms=self.duration_ms / self.attempt_count,
                        exit_code=1 if self.first_attempt_status == ExecutionStatus.FAILED.value else 0,
                    ),
                    TestAttempt(
                        attempt=self.attempt_count,
                        status=self.status,
                        duration_ms=self.duration_ms / self.attempt_count,
                        exit_code=self.exit_code or (0 if self.status == ExecutionStatus.PASSED.value else 1),
                        stdout_log=self.stdout_log,
                        stderr_log=self.stderr_log,
                    ),
                ]
            else:
                self.attempts = [
                    TestAttempt(
                        attempt=i + 1,
                        status=self.status,
                        duration_ms=self.duration_ms / max(1, self.attempt_count),
                        exit_code=self.exit_code,
                        stdout_log=self.stdout_log if i + 1 == self.attempt_count else None,
                        stderr_log=self.stderr_log if i + 1 == self.attempt_count else None,
                    )
                    for i in range(self.attempt_count)
                ]

        # Sync attempt count
        if self.attempts and len(self.attempts) != self.attempt_count:
            self.attempt_count = len(self.attempts)

        # Detect flaky/retry hiding failure
        if self.attempts:
            if not self.first_attempt_status:
                self.first_attempt_status = self.attempts[0].status
            prior_failed = any(
                a.status == ExecutionStatus.FAILED.value or (a.exit_code is not None and a.exit_code != 0)
                for a in self.attempts[:-1]
            )
            if prior_failed and self.status == ExecutionStatus.PASSED.value:
                self.is_flaky = True
        elif self.first_attempt_status == ExecutionStatus.FAILED.value and self.status == ExecutionStatus.PASSED.value:
            self.is_flaky = True
        elif self.attempt_count > 1 and self.status == ExecutionStatus.PASSED.value:
            # Multi-attempt pass without explicit attempt breakdown is considered potentially flaky
            self.is_flaky = True

        if self.exit_code is None:
            self.exit_code = 0 if self.status == ExecutionStatus.PASSED.value else 1

    @property
    def matrix_key(self) -> Tuple[str, str, str, str]:
        """Matrix coordinate tuple: (case, peer, direction, transport)."""
        return (self.case, self.peer, self.direction, self.transport)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "case": self.case,
            "status": self.status,
            "duration_ms": self.duration_ms,
            "peer": self.peer,
            "direction": self.direction,
            "transport": self.transport,
            "stdout_log": self.stdout_log,
            "stderr_log": self.stderr_log,
            "attempt_count": self.attempt_count,
            "attempts": [a.to_dict() for a in self.attempts],
            "exit_code": self.exit_code,
            "suite": self.suite,
            "profile": self.profile,
            "peer_pin": self.peer_pin,
            "procedure_source": self.procedure_source,
            "notes": self.notes,
            "error_message": self.error_message,
            "first_attempt_status": self.first_attempt_status,
            "is_flaky": self.is_flaky,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "TestResult":
        for req in ("case", "status", "duration_ms", "peer", "direction", "transport"):
            if req not in data:
                raise ValueError(f"Missing required field in test result: '{req}'")
        attempts = [TestAttempt.from_dict(a) for a in data.get("attempts", [])]
        return cls(
            case=str(data["case"]),
            status=str(data["status"]),
            duration_ms=float(data["duration_ms"]),
            peer=str(data["peer"]),
            direction=str(data["direction"]),
            transport=str(data["transport"]),
            stdout_log=data.get("stdout_log"),
            stderr_log=data.get("stderr_log"),
            attempt_count=int(data.get("attempt_count", max(1, len(attempts)))),
            attempts=attempts,
            exit_code=data.get("exit_code"),
            suite=data.get("suite"),
            profile=data.get("profile"),
            peer_pin=data.get("peer_pin"),
            procedure_source=data.get("procedure_source"),
            notes=data.get("notes"),
            error_message=data.get("error_message"),
            first_attempt_status=data.get("first_attempt_status"),
            is_flaky=bool(data.get("is_flaky", False)),
        )


class ResultWriter:
    """Writer and manager for machine-readable test execution results."""

    def __init__(self, output_path: Optional[Union[str, Path]] = None):
        self.output_path = Path(output_path) if output_path else None
        self.results: List[TestResult] = []
        self.metadata: Dict[str, Any] = {}
        if self.output_path and self.output_path.exists():
            self.load(self.output_path)

    def load(self, path: Union[str, Path]) -> None:
        p = Path(path)
        with p.open("r", encoding="utf-8") as f:
            data = json.load(f)
        if isinstance(data, list):
            self.results = [TestResult.from_dict(d) for d in data]
        elif isinstance(data, dict):
            self.metadata = {k: v for k, v in data.items() if k != "results"}
            self.results = [TestResult.from_dict(d) for d in data.get("results", [])]
        else:
            raise ValueError(f"Unexpected JSON structure in {path}")

    def add_result(self, result: Union[TestResult, Dict[str, Any]]) -> None:
        res = TestResult.from_dict(result) if isinstance(result, dict) else result
        for idx, existing in enumerate(self.results):
            if existing.matrix_key == res.matrix_key:
                self.results[idx] = res
                return
        self.results.append(res)

    def write(self, path: Optional[Union[str, Path]] = None) -> Path:
        target = Path(path) if path else self.output_path
        if not target:
            raise ValueError("No output path specified for ResultWriter.write()")
        target.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            **self.metadata,
            "updated_at": datetime.now(timezone.utc).isoformat(),
            "results": [r.to_dict() for r in self.results],
        }
        with target.open("w", encoding="utf-8") as f:
            json.dump(payload, f, indent=2)
        return target


class CasesRegistry:
    """Wrapper and validator for tests/interop/cases.json."""

    def __init__(self, registry_data: Dict[str, Any]):
        self.raw = registry_data
        self.version = registry_data.get("version", "unknown")
        self.title = registry_data.get("title", "")
        self.upstream_pins = registry_data.get("upstream_pins", {})
        self.suites = registry_data.get("suites", {})
        self.disposition_definitions = registry_data.get("disposition_definitions", {})
        self.summary = registry_data.get("summary", {})
        self.cases_list: List[Dict[str, Any]] = registry_data.get("cases", [])
        self.cases_by_name: Dict[str, Dict[str, Any]] = {
            c["case"]: c for c in self.cases_list if "case" in c
        }

    @classmethod
    def load(cls, path: Union[str, Path]) -> "CasesRegistry":
        p = Path(path)
        if not p.exists():
            raise FileNotFoundError(f"cases.json registry not found at: {path}")
        with p.open("r", encoding="utf-8") as f:
            data = json.load(f)
        return cls(data)

    def validate_registry_schema(self) -> List[str]:
        """Validates the schema and completeness of cases.json."""
        errors = []
        expected_statuses = {status.value for status in ExecutionStatus}
        if set(self.disposition_definitions) != expected_statuses:
            errors.append("Registry disposition definitions do not match the six execution statuses")
        if not self.version:
            errors.append("Registry missing 'version'")
        if not self.upstream_pins:
            errors.append("Registry missing 'upstream_pins'")
        else:
            for pin_key in ("protobuf", "grpc", "grpc_go"):
                if pin_key not in self.upstream_pins:
                    errors.append(f"upstream_pins missing required pin '{pin_key}'")
                elif "commit" not in self.upstream_pins[pin_key]:
                    errors.append(f"upstream_pins['{pin_key}'] missing commit hash")
        if not self.suites:
            errors.append("Registry missing 'suites'")
        if not self.cases_list:
            errors.append("Registry contains no 'cases'")

        seen_cases: Set[str] = set()
        for idx, c in enumerate(self.cases_list):
            case_id = c.get("case")
            if not case_id:
                errors.append(f"Case at index {idx} missing 'case' identifier")
                continue
            if case_id in seen_cases:
                errors.append(f"Duplicate case identifier in registry: '{case_id}'")
            seen_cases.add(case_id)

            suite = c.get("suite")
            if not suite or suite not in self.suites:
                errors.append(f"Case '{case_id}' references undefined suite: '{suite}'")

            disp = c.get("disposition")
            if not disp or disp not in self.disposition_definitions:
                errors.append(f"Case '{case_id}' has invalid disposition: '{disp}'")

            if disp != ExecutionStatus.PASSED.value:
                if not c.get("justification"):
                    errors.append(f"Case '{case_id}' with disposition '{disp}' requires a justification")
            coverage = c.get("present_coverage")
            if not isinstance(coverage, dict):
                errors.append(f"Case '{case_id}' has no present_coverage object")
            elif coverage.get("status") not in expected_statuses:
                errors.append(f"Case '{case_id}' has invalid coverage status: '{coverage.get('status')}'")
            elif coverage["status"] == ExecutionStatus.PASSED.value and not coverage.get("evidence_file"):
                errors.append(f"Case '{case_id}' has passing coverage without an evidence file")

        if self.summary:
            expected_total = self.summary.get("total_cases")
            if expected_total is not None and expected_total != len(self.cases_list):
                errors.append(
                    f"Summary total_cases ({expected_total}) != actual cases count ({len(self.cases_list)})"
                )
            for summary_key, case_key in (
                ("by_disposition", "disposition"),
                ("by_suite", "suite"),
            ):
                recorded = self.summary.get(summary_key)
                actual = dict(Counter(c.get(case_key) for c in self.cases_list))
                if recorded != actual:
                    errors.append(
                        f"Summary {summary_key} ({recorded}) != actual case counts ({actual})"
                    )
        return errors


@dataclass
class ValidationReport:
    """Outcome of validating execution results against cases.json."""

    is_valid: bool
    errors: List[str] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)
    missing_cases: List[str] = field(default_factory=list)
    duplicate_rows: List[Tuple[str, str, str, str]] = field(default_factory=list)
    wrong_pins: List[str] = field(default_factory=list)
    flaky_cases: List[str] = field(default_factory=list)
    missing_matrix_rows: List[str] = field(default_factory=list)
    peer_violations: List[str] = field(default_factory=list)


class ReportValidator:
    """Validates execution results against cases.json registry."""

    def __init__(self, registry: Union[CasesRegistry, str, Path, Dict[str, Any]]):
        if isinstance(registry, CasesRegistry):
            self.registry = registry
        elif isinstance(registry, dict):
            self.registry = CasesRegistry(registry)
        else:
            self.registry = CasesRegistry.load(registry)

    def validate_results(
        self,
        results: List[TestResult],
        suite: Optional[str] = None,
        profile: Optional[str] = None,
        require_all_cases: bool = False,
        require_matrix: bool = False,
        required_directions: Optional[Set[str]] = None,
        require_peers: bool = False,
        spec_adapter: bool = False,
        strict_retries: bool = True,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> ValidationReport:
        """Validate test execution results against cases.json."""
        errors: List[str] = []
        warnings: List[str] = []
        missing_cases: List[str] = []
        duplicate_rows: List[Tuple[str, str, str, str]] = []
        wrong_pins: List[str] = []
        flaky_cases: List[str] = []
        missing_matrix_rows: List[str] = []
        peer_violations: List[str] = []
        spec_peers = {
            "http2_negative": {"local-http2-peer", "external-http2-peer"},
            "server_probe": {"local-native-server"},
        }
        if spec_adapter and (
            suite not in spec_peers or profile != "native" or require_all_cases
        ):
            errors.append(
                "Spec-derived adapter mode requires a scoped native HTTP/2 suite without --require-all"
            )

        # 1. Empty results check
        if not results:
            msg = "Empty output: no test execution results provided."
            errors.append(msg)
            return ValidationReport(
                is_valid=False,
                errors=errors,
                warnings=warnings,
                missing_cases=missing_cases,
                duplicate_rows=duplicate_rows,
                wrong_pins=wrong_pins,
                flaky_cases=flaky_cases,
                missing_matrix_rows=missing_matrix_rows,
                peer_violations=peer_violations,
            )

        # 2. Duplicate matrix row check
        seen_matrix: Set[Tuple[str, str, str, str]] = set()
        for r in results:
            key = r.matrix_key
            if key in seen_matrix:
                msg = f"Duplicate matrix row: case '{r.case}', peer '{r.peer}', direction '{r.direction}', transport '{r.transport}'"
                errors.append(msg)
                duplicate_rows.append(key)
            seen_matrix.add(key)

        # 3. Check individual results against registry definitions and pins
        results_by_case: Dict[str, List[TestResult]] = {}
        for r in results:
            results_by_case.setdefault(r.case, []).append(r)
            case_def = self.registry.cases_by_name.get(r.case)
            if not case_def:
                errors.append(f"Result references unknown case '{r.case}' not found in cases.json")
                continue
            is_adapter_result = (
                spec_adapter
                and case_def.get("suite") == suite
                and r.peer in spec_peers.get(suite, set())
            )
            if spec_adapter and not is_adapter_result:
                errors.append(
                    f"Spec-derived adapter result for '{r.case}' must use the selected suite and peer from {sorted(spec_peers.get(suite, set()))}"
                )
            if case_def.get("disposition") not in (
                ExecutionStatus.PASSED.value,
                ExecutionStatus.NOT_APPLICABLE.value,
            ) and r.status == ExecutionStatus.PASSED.value and not is_adapter_result:
                errors.append(
                    f"Case '{r.case}' cannot be reported passed while the registry disposition is '{case_def['disposition']}'"
                )

            # Populate suite and profile from case_def if not present
            if not r.suite:
                r.suite = case_def.get("suite")
            if not r.profile:
                r.profile = case_def.get("profile")

            # Check procedure source pin
            if r.procedure_source:
                expected_src = case_def.get("procedure_source")
                if expected_src and r.procedure_source != expected_src:
                    msg = (
                        f"Wrong procedure source pin for case '{r.case}': "
                        f"expected '{expected_src}', got '{r.procedure_source}'"
                    )
                    errors.append(msg)
                    wrong_pins.append(msg)

            # Check peer pin
            if r.peer_pin:
                expected_commit = None
                p_lower = r.peer.lower()
                if "grpc-go" in p_lower or "grpc_go" in p_lower or "go" in p_lower:
                    expected_commit = self.registry.upstream_pins.get("grpc_go", {}).get("commit")
                elif "protobuf" in p_lower:
                    expected_commit = self.registry.upstream_pins.get("protobuf", {}).get("commit")
                elif "grpc" in p_lower and "kernel" not in p_lower and "pbrs" not in p_lower:
                    expected_commit = self.registry.upstream_pins.get("grpc", {}).get("commit")

                if expected_commit and r.peer_pin != expected_commit:
                    msg = (
                        f"Wrong peer pin for peer '{r.peer}' on case '{r.case}': "
                        f"expected commit '{expected_commit}', got '{r.peer_pin}'"
                    )
                    errors.append(msg)
                    wrong_pins.append(msg)

            # Check retries hiding first failures
            if r.is_flaky:
                flaky_cases.append(r.case)
                msg = (
                    f"Retry hid first failure on case '{r.case}' (direction '{r.direction}', peer '{r.peer}'): "
                    f"passed on attempt {r.attempt_count} after prior attempt failure."
                )
                if strict_retries:
                    errors.append(f"{msg} Flaky results cannot satisfy qualification gate.")
                else:
                    warnings.append(
                        f"Case '{r.case}' retried (attempt {r.attempt_count}) and passed after prior attempt failure."
                    )

        # 4. Check top-level metadata peer_pins
        if metadata and "peer_pins" in metadata and isinstance(metadata["peer_pins"], dict):
            for p_name, pin_val in metadata["peer_pins"].items():
                expected_commit = None
                pn_lower = p_name.lower()
                if pn_lower in ("grpc_go", "grpc-go", "go"):
                    expected_commit = self.registry.upstream_pins.get("grpc_go", {}).get("commit")
                elif pn_lower == "protobuf":
                    expected_commit = self.registry.upstream_pins.get("protobuf", {}).get("commit")
                elif pn_lower == "grpc":
                    expected_commit = self.registry.upstream_pins.get("grpc", {}).get("commit")

                if expected_commit and pin_val != expected_commit:
                    msg = f"Top-level peer pin mismatch for '{p_name}': expected '{expected_commit}', got '{pin_val}'"
                    errors.append(msg)
                    wrong_pins.append(msg)

        # A profile without a suite names the entire shipping profile, not a
        # partial smoke. The standard-interop CI selects its suite explicitly.
        enforce_all_cases = require_all_cases or (profile is not None and suite is None)

        # 5. Determine expected cases to evaluate
        expected_cases: List[Dict[str, Any]] = []
        for c in self.registry.cases_list:
            if suite and c.get("suite") != suite:
                continue
            if profile and c.get("profile") != profile:
                continue
            if enforce_all_cases or (suite or profile) or c.get("disposition") == ExecutionStatus.PASSED.value:
                expected_cases.append(c)
        if suite or profile:
            selected_names = {case["case"] for case in expected_cases}
            if not any(
                r.case in selected_names and r.status == ExecutionStatus.PASSED.value
                for r in results
            ):
                errors.append(
                    f"No passing results for selected suite/profile: suite={suite}, profile={profile}"
                )

        if required_directions is not None:
            if not require_matrix:
                errors.append("Required directions need --require-matrix.")
            if not required_directions:
                errors.append("At least one required direction must be specified.")
            applicable = {
                direction
                for case in expected_cases
                for direction in case.get("present_coverage", {}).get("passing_directions", [])
            }
            for direction in sorted(required_directions - applicable):
                errors.append(f"Unknown required direction for the selected suite/profile: '{direction}'")

        # 6. Check missing and skipped cases
        for c in expected_cases:
            c_name = c["case"]
            c_disp = c["disposition"]
            case_results = results_by_case.get(c_name, [])

            if enforce_all_cases and c_disp not in (
                ExecutionStatus.PASSED.value,
                ExecutionStatus.NOT_APPLICABLE.value,
            ):
                errors.append(
                    f"Unqualified required case '{c_name}': registry disposition is '{c_disp}'"
                )

            if not case_results:
                if enforce_all_cases or (c_disp == ExecutionStatus.PASSED.value and (suite or profile)):
                    msg = f"Missing required case in results: '{c_name}' (expected disposition: '{c_disp}')"
                    errors.append(msg)
                    missing_cases.append(c_name)
                continue

            for cr in case_results:
                if c_disp != ExecutionStatus.PASSED.value:
                    continue
                if required_directions is not None and cr.direction not in required_directions:
                    continue
                if cr.status == ExecutionStatus.FAILED.value:
                    errors.append(f"Required case '{c_name}' failed execution: {cr.error_message or 'non-zero exit'}")
                elif cr.status == ExecutionStatus.NOT_RUN.value:
                    errors.append(
                        f"Skipped required case: case '{c_name}' was not run (status: not_run)"
                    )
                elif (suite or profile or enforce_all_cases) and cr.status != ExecutionStatus.PASSED.value:
                    errors.append(
                        f"Required case '{c_name}' did not pass (status: {cr.status})"
                    )

            # 7. Check matrix directions and self-test substitution
            if require_matrix or require_peers:
                expected_dirs = c.get("present_coverage", {}).get("passing_directions", [])
                if required_directions is not None:
                    expected_dirs = [direction for direction in expected_dirs if direction in required_directions]
                tested_dirs = {cr.direction for cr in case_results if cr.status == ExecutionStatus.PASSED.value}

                for ed in expected_dirs:
                    if ed not in tested_dirs:
                        msg = f"Missing required matrix row: case '{c_name}' missing direction '{ed}'"
                        errors.append(msg)
                        missing_matrix_rows.append(f"{c_name}:{ed}")

                # Self-test substitution check
                if c.get("peer_direction") == "both":
                    has_self = any(cr.direction == "kernel_client_to_kernel_server" for cr in case_results)
                    has_peer = any(
                        cr.direction in ("kernel_client_to_go_server", "go_client_to_kernel_server")
                        or ("go" in cr.peer.lower() or "peer" in cr.peer.lower())
                        for cr in case_results
                    )
                    if has_self and not has_peer:
                        msg = (
                            f"Self-test substitution violation: case '{c_name}' was only executed as self-test "
                            f"('kernel_client_to_kernel_server'); independent peer execution is required and "
                            f"cannot be substituted."
                        )
                        errors.append(msg)
                        peer_violations.append(msg)

        is_valid = len(errors) == 0
        return ValidationReport(
            is_valid=is_valid,
            errors=errors,
            warnings=warnings,
            missing_cases=missing_cases,
            duplicate_rows=duplicate_rows,
            wrong_pins=wrong_pins,
            flaky_cases=flaky_cases,
            missing_matrix_rows=missing_matrix_rows,
            peer_violations=peer_violations,
        )


@dataclass
class SuiteSummary:
    """Aggregate statistics for a single test suite."""

    suite_id: str
    suite_name: str
    total_cases: int = 0
    passed: int = 0
    failed: int = 0
    not_run: int = 0
    unsupported: int = 0
    blocked_external: int = 0
    not_applicable: int = 0
    flaky: int = 0
    status: str = "passed"

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


@dataclass
class ProfileSummary:
    """Aggregate statistics for a single qualification profile."""

    profile_id: str
    total_cases: int = 0
    passed: int = 0
    failed: int = 0
    not_run: int = 0
    unsupported: int = 0
    blocked_external: int = 0
    not_applicable: int = 0
    flaky: int = 0
    status: str = "passed"

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


@dataclass
class RunSummary:
    """Aggregate summary for an entire test run."""

    total_evaluated: int = 0
    passed: int = 0
    failed: int = 0
    not_run: int = 0
    unsupported: int = 0
    blocked_external: int = 0
    not_applicable: int = 0
    flaky_or_retried: int = 0
    total_duration_ms: float = 0.0

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


class AggregatedReport:
    """Machine-readable aggregate interop report."""

    def __init__(
        self,
        overall_status: str,
        overall_passed: bool,
        failure_reasons: List[str],
        summary: RunSummary,
        by_suite: Dict[str, SuiteSummary],
        by_profile: Dict[str, ProfileSummary],
        results: List[TestResult],
        upstream_pins: Dict[str, Any],
        target_suite: Optional[str] = None,
        target_profile: Optional[str] = None,
        generated_at: Optional[str] = None,
        spec_adapter: bool = False,
    ):
        self.report_version = "1.0.0"
        self.generated_at = generated_at or datetime.now(timezone.utc).isoformat()
        self.overall_status = overall_status
        self.overall_passed = overall_passed
        self.failure_reasons = failure_reasons
        self.summary = summary
        self.by_suite = by_suite
        self.by_profile = by_profile
        self.results = results
        self.upstream_pins = upstream_pins
        self.target_suite = target_suite
        self.target_profile = target_profile
        self.spec_adapter = spec_adapter

    def to_dict(self) -> Dict[str, Any]:
        result = {
            "report_version": self.report_version,
            "generated_at": self.generated_at,
            "overall_status": self.overall_status,
            "overall_passed": self.overall_passed,
            "evidence_scope": "spec_derived_adapter" if self.spec_adapter else "registry",
            "failure_reasons": self.failure_reasons,
            "target_suite": self.target_suite,
            "target_profile": self.target_profile,
            "upstream_pins": self.upstream_pins,
            "summary": self.summary.to_dict(),
            "by_suite": {k: v.to_dict() for k, v in self.by_suite.items()},
            "by_profile": {k: v.to_dict() for k, v in self.by_profile.items()},
            "results": [r.to_dict() for r in self.results],
        }
        if self.spec_adapter:
            result["qualification"] = {
                "qualified": False,
                "reason": "Spec-derived adapters do not qualify original upstream procedures",
            }
        return result

    def to_json(self, indent: int = 2) -> str:
        return json.dumps(self.to_dict(), indent=indent)

    def write_json(self, path: Union[str, Path]) -> Path:
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        with p.open("w", encoding="utf-8") as f:
            f.write(self.to_json(indent=2))
        return p

    def to_markdown(self) -> str:
        """Render a full markdown report with summary tables and case links."""
        lines = []
        lines.append("# gRPC and Protobuf Interoperability Report\n")
        lines.append(f"**Overall Status**: **{self.overall_status.upper()}**")
        lines.append(f"**Generated**: `{self.generated_at}`")
        if self.target_suite:
            lines.append(f"**Target Suite**: `{self.target_suite}`")
        if self.target_profile:
            lines.append(f"**Target Profile**: `{self.target_profile}`")
        if self.spec_adapter:
            lines.append("**Evidence Scope**: spec-derived adapter; original upstream qualification: NOT QUALIFIED")

        lines.append("\n### Upstream Pinned Versions")
        for k, pin in self.upstream_pins.items():
            repo = pin.get("repository", k)
            commit = pin.get("commit", "unknown")
            ver = pin.get("version", "")
            ver_str = f" ({ver})" if ver else ""
            lines.append(f"- **{k}**: `{repo}@{commit}`{ver_str}")

        lines.append("\n## Overall Summary\n")
        lines.append("| Metric | Count |")
        lines.append("|---|---|")
        lines.append(f"| Total Evaluated | {self.summary.total_evaluated} |")
        lines.append(f"| Passed | {self.summary.passed} |")
        lines.append(f"| Failed | {self.summary.failed} |")
        lines.append(f"| Not Run | {self.summary.not_run} |")
        lines.append(f"| Unsupported | {self.summary.unsupported} |")
        lines.append(f"| Blocked External | {self.summary.blocked_external} |")
        lines.append(f"| Not Applicable | {self.summary.not_applicable} |")
        lines.append(f"| Retried / Flaky | {self.summary.flaky_or_retried} |")
        lines.append(f"| Total Duration | {self.summary.total_duration_ms:.2f} ms |")

        if self.failure_reasons:
            lines.append("\n## Failure Reasons\n")
            for reason in self.failure_reasons:
                lines.append(f"- ❌ {reason}")

        if self.by_suite:
            lines.append("\n## Suite Breakdown\n")
            lines.append("| Suite | Total | Passed | Failed | Not Run | Unsupported | Blocked | N/A | Flaky | Status |")
            lines.append("|---|---|---|---|---|---|---|---|---|---|")
            for s_id, s in self.by_suite.items():
                s_status = f"**{s.status.upper()}**"
                lines.append(
                    f"| `{s_id}` | {s.total_cases} | {s.passed} | {s.failed} | {s.not_run} | "
                    f"{s.unsupported} | {s.blocked_external} | {s.not_applicable} | {s.flaky} | {s_status} |"
                )

        if self.by_profile:
            lines.append("\n## Profile Breakdown\n")
            lines.append("| Profile | Total | Passed | Failed | Not Run | Unsupported | Blocked | N/A | Status |")
            lines.append("|---|---|---|---|---|---|---|---|---|")
            for p_id, p in self.by_profile.items():
                p_status = f"**{p.status.upper()}**"
                lines.append(
                    f"| `{p_id}` | {p.total_cases} | {p.passed} | {p.failed} | {p.not_run} | "
                    f"{p.unsupported} | {p.blocked_external} | {p.not_applicable} | {p_status} |"
                )

        if self.results:
            lines.append("\n## Execution Matrix Details\n")
            lines.append("| Case | Suite | Peer | Direction | Transport | Status | Attempts | Duration | Exit | Logs |")
            lines.append("|---|---|---|---|---|---|---|---|---|---|")
            for r in self.results:
                log_parts = []
                if r.stdout_log:
                    log_parts.append(f"[stdout]({r.stdout_log})")
                if r.stderr_log:
                    log_parts.append(f"[stderr]({r.stderr_log})")
                logs_str = ", ".join(log_parts) if log_parts else "-"
                status_str = r.status.upper()
                if r.is_flaky:
                    status_str += " (FLAKY)"
                exit_str = str(r.exit_code) if r.exit_code is not None else "-"
                lines.append(
                    f"| `{r.case}` | `{r.suite or '-'}` | `{r.peer}` | `{r.direction}` | "
                    f"`{r.transport}` | {status_str} | {r.attempt_count} | {r.duration_ms:.1f}ms | {exit_str} | {logs_str} |"
                )

        return "\n".join(lines)

    def to_terminal(self, use_color: bool = True) -> str:
        """Render a formatted summary table for console output."""
        GREEN = "\033[32m" if use_color else ""
        RED = "\033[31m" if use_color else ""
        YELLOW = "\033[33m" if use_color else ""
        BOLD = "\033[1m" if use_color else ""
        RESET = "\033[0m" if use_color else ""

        status_color = GREEN if self.overall_passed else RED
        lines = []
        lines.append(f"{BOLD}=== gRPC and Protobuf Interoperability Report ==={RESET}")
        lines.append(f"Overall Status: {status_color}{BOLD}{self.overall_status.upper()}{RESET}")
        lines.append(f"Generated:      {self.generated_at}")
        if self.target_suite:
            lines.append(f"Target Suite:   {self.target_suite}")
        if self.target_profile:
            lines.append(f"Target Profile: {self.target_profile}")
        if self.spec_adapter:
            lines.append("Evidence Scope: spec-derived adapter; original upstream qualification: NOT QUALIFIED")

        lines.append(f"\n{BOLD}Summary Metrics:{RESET}")
        metrics = [
            ("Total Evaluated", self.summary.total_evaluated),
            ("Passed", f"{GREEN}{self.summary.passed}{RESET}"),
            ("Failed", f"{RED}{self.summary.failed}{RESET}" if self.summary.failed else "0"),
            ("Not Run", f"{YELLOW}{self.summary.not_run}{RESET}" if self.summary.not_run else "0"),
            ("Unsupported", self.summary.unsupported),
            ("Blocked External", self.summary.blocked_external),
            ("Not Applicable", self.summary.not_applicable),
            ("Retried / Flaky", f"{YELLOW}{self.summary.flaky_or_retried}{RESET}" if self.summary.flaky_or_retried else "0"),
            ("Total Duration", f"{self.summary.total_duration_ms:.2f} ms"),
        ]
        for name, val in metrics:
            lines.append(f"  {name:<18}: {val}")

        if self.failure_reasons:
            lines.append(f"\n{RED}{BOLD}Failure Reasons:{RESET}")
            for reason in self.failure_reasons:
                lines.append(f"  {RED}✖{RESET} {reason}")

        if self.by_suite:
            lines.append(f"\n{BOLD}Suite Breakdown:{RESET}")
            header = f"  {'Suite':<22} {'Total':<6} {'Pass':<6} {'Fail':<6} {'Skip':<6} {'Status':<8}"
            lines.append(header)
            lines.append("  " + "-" * (len(header) - 2))
            for s_id, s in self.by_suite.items():
                col = GREEN if s.status == "passed" else RED
                lines.append(
                    f"  {s_id:<22} {s.total_cases:<6} {s.passed:<6} {s.failed:<6} {s.not_run:<6} {col}{s.status.upper():<8}{RESET}"
                )

        if self.results:
            lines.append(f"\n{BOLD}Matrix Execution Details:{RESET}")
            header = f"  {'Case':<28} {'Peer':<12} {'Direction':<30} {'Status':<10} {'Attempts':<8} {'Duration':<10}"
            lines.append(header)
            lines.append("  " + "-" * (len(header) - 2))
            for r in self.results:
                stat_col = GREEN if r.status == ExecutionStatus.PASSED.value else (YELLOW if r.status in (ExecutionStatus.NOT_RUN.value, ExecutionStatus.UNSUPPORTED.value) else RED)
                st_text = r.status.upper()
                if r.is_flaky:
                    st_text += "*"
                lines.append(
                    f"  {r.case:<28} {r.peer:<12} {r.direction:<30} {stat_col}{st_text:<10}{RESET} {r.attempt_count:<8} {r.duration_ms:.1f}ms"
                )

        return "\n".join(lines)


class ReportAggregator:
    """Aggregates test results, validates against cases.json, and generates reports."""

    def __init__(self, registry: Union[CasesRegistry, str, Path, Dict[str, Any]]):
        if isinstance(registry, CasesRegistry):
            self.registry = registry
        elif isinstance(registry, dict):
            self.registry = CasesRegistry(registry)
        else:
            self.registry = CasesRegistry.load(registry)
        self.validator = ReportValidator(self.registry)

    def aggregate(
        self,
        results: List[TestResult],
        suite: Optional[str] = None,
        profile: Optional[str] = None,
        require_all_cases: bool = False,
        require_matrix: bool = False,
        required_directions: Optional[Set[str]] = None,
        require_peers: bool = False,
        spec_adapter: bool = False,
        strict_retries: bool = True,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> AggregatedReport:
        """Run validation and aggregate metrics into an AggregatedReport."""
        val_report = self.validator.validate_results(
            results=results,
            suite=suite,
            profile=profile,
            require_all_cases=require_all_cases,
            require_matrix=require_matrix,
            required_directions=required_directions,
            require_peers=require_peers,
            spec_adapter=spec_adapter,
            strict_retries=strict_retries,
            metadata=metadata,
        )

        failure_reasons: List[str] = list(val_report.errors)

        # Count metrics
        passed = sum(1 for r in results if r.status == ExecutionStatus.PASSED.value)
        failed = sum(1 for r in results if r.status == ExecutionStatus.FAILED.value)
        not_run = sum(1 for r in results if r.status == ExecutionStatus.NOT_RUN.value)
        unsupported = sum(1 for r in results if r.status == ExecutionStatus.UNSUPPORTED.value)
        blocked = sum(1 for r in results if r.status == ExecutionStatus.BLOCKED_EXTERNAL.value)
        not_applicable = sum(1 for r in results if r.status == ExecutionStatus.NOT_APPLICABLE.value)
        flaky = sum(1 for r in results if r.is_flaky)
        total_duration = sum(r.duration_ms for r in results)

        summary = RunSummary(
            total_evaluated=len(results),
            passed=passed,
            failed=failed,
            not_run=not_run,
            unsupported=unsupported,
            blocked_external=blocked,
            not_applicable=not_applicable,
            flaky_or_retried=flaky,
            total_duration_ms=total_duration,
        )

        # Fail closed check
        if failed > 0:
            failure_reasons.append(f"{failed} test case(s) reported status 'failed'")
        if not_run > 0 and (require_all_cases or suite or profile):
            failure_reasons.append(f"{not_run} required case(s) were not run ('not_run')")
        if len(results) == 0:
            failure_reasons.append("Empty results: no tests executed")

        # Aggregate by suite
        by_suite: Dict[str, SuiteSummary] = {}
        for r in results:
            s_id = r.suite or "unknown"
            if s_id not in by_suite:
                s_name = self.registry.suites.get(s_id, s_id)
                by_suite[s_id] = SuiteSummary(suite_id=s_id, suite_name=s_name)
            s_sum = by_suite[s_id]
            s_sum.total_cases += 1
            if r.status == ExecutionStatus.PASSED.value:
                s_sum.passed += 1
            elif r.status == ExecutionStatus.FAILED.value:
                s_sum.failed += 1
                s_sum.status = "failed"
            elif r.status == ExecutionStatus.NOT_RUN.value:
                s_sum.not_run += 1
                if require_all_cases:
                    s_sum.status = "failed"
            elif r.status == ExecutionStatus.UNSUPPORTED.value:
                s_sum.unsupported += 1
            elif r.status == ExecutionStatus.BLOCKED_EXTERNAL.value:
                s_sum.blocked_external += 1
            elif r.status == ExecutionStatus.NOT_APPLICABLE.value:
                s_sum.not_applicable += 1
            if r.is_flaky:
                s_sum.flaky += 1
                if strict_retries:
                    s_sum.status = "failed"

        # Aggregate by profile
        by_profile: Dict[str, ProfileSummary] = {}
        for r in results:
            p_id = r.profile or "unknown"
            if p_id not in by_profile:
                by_profile[p_id] = ProfileSummary(profile_id=p_id)
            p_sum = by_profile[p_id]
            p_sum.total_cases += 1
            if r.status == ExecutionStatus.PASSED.value:
                p_sum.passed += 1
            elif r.status == ExecutionStatus.FAILED.value:
                p_sum.failed += 1
                p_sum.status = "failed"
            elif r.status == ExecutionStatus.NOT_RUN.value:
                p_sum.not_run += 1
                if require_all_cases:
                    p_sum.status = "failed"
            elif r.status == ExecutionStatus.UNSUPPORTED.value:
                p_sum.unsupported += 1
            elif r.status == ExecutionStatus.BLOCKED_EXTERNAL.value:
                p_sum.blocked_external += 1
            elif r.status == ExecutionStatus.NOT_APPLICABLE.value:
                p_sum.not_applicable += 1
            if r.is_flaky:
                p_sum.flaky += 1
                if strict_retries:
                    p_sum.status = "failed"

        # Deduplicate failure reasons while preserving order
        dedup_reasons: List[str] = []
        for reason in failure_reasons:
            if reason not in dedup_reasons:
                dedup_reasons.append(reason)

        overall_passed = len(dedup_reasons) == 0
        overall_status = "passed" if overall_passed else "failed"
        if not overall_passed:
            if suite is not None:
                by_suite.setdefault(
                    suite, SuiteSummary(suite_id=suite, suite_name=self.registry.suites.get(suite, suite))
                ).status = "failed"
            if profile is not None:
                by_profile.setdefault(
                    profile, ProfileSummary(profile_id=profile)
                ).status = "failed"

        return AggregatedReport(
            overall_status=overall_status,
            overall_passed=overall_passed,
            failure_reasons=dedup_reasons,
            summary=summary,
            by_suite=by_suite,
            by_profile=by_profile,
            results=results,
            upstream_pins=self.registry.upstream_pins,
            target_suite=suite,
            target_profile=profile,
            spec_adapter=spec_adapter,
        )


def load_results_file(source: Union[str, Path]) -> Tuple[List[TestResult], Dict[str, Any]]:
    """Load test execution results from a file path or '-' for stdin."""
    if source == "-":
        content = sys.stdin.read()
        if not content.strip():
            return [], {}
        data = json.loads(content)
    else:
        p = Path(source)
        if not p.exists():
            raise FileNotFoundError(f"Results file not found: {source}")
        with p.open("r", encoding="utf-8") as f:
            content = f.read()
            if not content.strip():
                return [], {}
            data = json.loads(content)

    metadata: Dict[str, Any] = {}
    if isinstance(data, list):
        results = [TestResult.from_dict(d) for d in data]
    elif isinstance(data, dict):
        metadata = {k: v for k, v in data.items() if k != "results"}
        raw_results = data.get("results", [])
        results = [TestResult.from_dict(d) for d in raw_results]
    else:
        raise ValueError(f"Unexpected JSON root type in {source}: {type(data).__name__}")

    return results, metadata


def default_cases_path() -> Path:
    """Resolve the default tests/interop/cases.json path relative to the repo root."""
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent
    return repo_root / "tests" / "interop" / "cases.json"


def parse_required_directions(value: str) -> Set[str]:
    directions = value.split(",")
    if any(not direction.strip() for direction in directions):
        raise argparse.ArgumentTypeError("Specify one or more comma-separated non-empty directions.")
    return {direction.strip() for direction in directions}


def build_parser() -> argparse.ArgumentParser:
    """Build the CLI argument parser."""
    parser = argparse.ArgumentParser(
        description="Standard-library result writer, validator, and aggregator for gRPC interop testing."
    )
    parser.add_argument(
        "--cases",
        type=str,
        default=str(default_cases_path()),
        help="Path to tests/interop/cases.json registry (default: tests/interop/cases.json)",
    )
    parser.add_argument(
        "--results",
        type=str,
        help="Path to input results JSON file (or '-' for stdin)",
    )
    parser.add_argument(
        "--output",
        "-o",
        type=str,
        help="Path to write machine-readable report.json",
    )
    parser.add_argument(
        "--format",
        choices=["terminal", "markdown", "json"],
        default="terminal",
        help="Console output format: terminal (default), markdown, or json",
    )
    parser.add_argument(
        "--suite",
        type=str,
        help="Filter and enforce requirements on a specific test suite (e.g. standard_interop)",
    )
    parser.add_argument(
        "--profile",
        type=str,
        help="Filter and enforce requirements on a specific qualification profile (e.g. native)",
    )
    parser.add_argument(
        "--require-all",
        action="store_true",
        help="Fail if any case in cases.json (filtered by suite/profile) is missing from results",
    )
    parser.add_argument(
        "--require-matrix",
        action="store_true",
        help="Fail if any matrix direction specified in present_coverage is missing",
    )
    parser.add_argument(
        "--required-directions",
        type=parse_required_directions,
        help="With --require-matrix, require only these comma-separated peer directions",
    )
    parser.add_argument(
        "--require-peers",
        action="store_true",
        help="Fail if independent peer execution is missing or substituted with self-test",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        default=True,
        help="Fail if retries hide first failures or any validation errors occur (default: True)",
    )
    parser.add_argument(
        "--no-strict",
        action="store_false",
        dest="strict",
        help="Permit flaky retries with a warning instead of failing closed",
    )
    parser.add_argument(
        "--no-color",
        action="store_true",
        help="Disable ANSI colors in terminal output",
    )

    subparsers = parser.add_subparsers(dest="subcommand", help="Optional subcommands")

    # Subcommand: validate
    sub_val = subparsers.add_parser("validate", help="Validate results against cases.json")
    sub_val.add_argument("--results", type=str, required=True, help="Path to results file (or '-' for stdin)")
    sub_val.add_argument("--cases", type=str, default=str(default_cases_path()), help="Path to cases.json")
    sub_val.add_argument("--suite", type=str, help="Suite filter")
    sub_val.add_argument("--profile", type=str, help="Profile filter")
    sub_val.add_argument("--require-all", action="store_true", default=argparse.SUPPRESS, help="Require every selected case and a qualifying registry disposition")
    sub_val.add_argument("--require-matrix", action="store_true", help="Enforce all matrix directions")
    sub_val.add_argument("--spec-adapter", action="store_true", help="Validate only a scoped native HTTP/2 spec-derived adapter, never upstream qualification")
    sub_val.add_argument("--required-directions", type=parse_required_directions, help="Comma-separated peer directions to require")
    sub_val.add_argument("--require-peers", action="store_true", help="Disallow self-test substitution")
    sub_val.add_argument("--strict", action="store_true", default=True, help="Enforce strict retry checking")

    # Subcommand: aggregate
    sub_agg = subparsers.add_parser("aggregate", help="Aggregate results and generate report.json")
    sub_agg.add_argument("--results", type=str, required=True, help="Path to results file (or '-' for stdin)")
    sub_agg.add_argument("--cases", type=str, default=str(default_cases_path()), help="Path to cases.json")
    sub_agg.add_argument("--output", "-o", type=str, help="Output path for report.json")
    sub_agg.add_argument("--format", choices=["terminal", "markdown", "json"], default="terminal")
    sub_agg.add_argument("--suite", type=str, help="Suite filter")
    sub_agg.add_argument("--profile", type=str, help="Profile filter")
    sub_agg.add_argument("--require-all", action="store_true", default=argparse.SUPPRESS, help="Require every selected case and a qualifying registry disposition")
    sub_agg.add_argument("--require-matrix", action="store_true", help="Enforce all matrix directions")
    sub_agg.add_argument("--spec-adapter", action="store_true", help="Report only a scoped native HTTP/2 spec-derived adapter, never upstream qualification")
    sub_agg.add_argument("--required-directions", type=parse_required_directions, help="Comma-separated peer directions to require")
    sub_agg.add_argument("--require-peers", action="store_true", help="Disallow self-test substitution")
    sub_agg.add_argument("--strict", action="store_true", default=True, help="Enforce strict retry checking")
    sub_agg.add_argument("--no-color", action="store_true", help="Disable ANSI color")

    # Subcommand: record
    sub_rec = subparsers.add_parser("record", help="Record a single test case result into a results file")
    sub_rec.add_argument("--output", "-o", type=str, required=True, help="Results file path to create/update")
    sub_rec.add_argument("--case", type=str, required=True, help="Case identifier (e.g. empty_unary)")
    sub_rec.add_argument(
        "--status",
        type=str,
        required=True,
        choices=["passed", "failed", "not_run", "unsupported", "blocked_external", "not_applicable"],
        help="Execution status",
    )
    sub_rec.add_argument("--duration-ms", type=float, required=True, help="Duration in milliseconds")
    sub_rec.add_argument("--peer", type=str, required=True, help="Peer identifier (e.g. grpc-go)")
    sub_rec.add_argument("--direction", type=str, required=True, help="RPC direction")
    sub_rec.add_argument("--transport", type=str, required=True, help="Transport (e.g. http2_cleartext)")
    sub_rec.add_argument("--stdout-log", type=str, help="Path to captured stdout log")
    sub_rec.add_argument("--stderr-log", type=str, help="Path to captured stderr log")
    sub_rec.add_argument("--exit-code", type=int, help="Command exit code")
    sub_rec.add_argument("--attempt-count", type=int, default=1, help="Attempt count (default: 1)")
    sub_rec.add_argument("--first-attempt-status", type=str, help="Status of the first attempt (for retries)")
    sub_rec.add_argument("--suite", type=str, help="Suite name")
    sub_rec.add_argument("--profile", type=str, help="Profile name")
    sub_rec.add_argument("--peer-pin", type=str, help="Peer commit pin")
    sub_rec.add_argument("--procedure-source", type=str, help="Procedure source string")
    sub_rec.add_argument("--notes", type=str, help="Execution notes")

    return parser


def handle_record(args: argparse.Namespace) -> int:
    """Handle recording a single test case result."""
    writer = ResultWriter(args.output)
    res = TestResult(
        case=args.case,
        status=args.status,
        duration_ms=args.duration_ms,
        peer=args.peer,
        direction=args.direction,
        transport=args.transport,
        stdout_log=args.stdout_log,
        stderr_log=args.stderr_log,
        exit_code=args.exit_code,
        attempt_count=args.attempt_count,
        first_attempt_status=args.first_attempt_status,
        suite=args.suite,
        profile=args.profile,
        peer_pin=args.peer_pin,
        procedure_source=args.procedure_source,
        notes=args.notes,
    )
    writer.add_result(res)
    writer.write()
    print(f"Recorded result for case '{args.case}' -> {args.output}")
    return EXIT_SUCCESS


def handle_validate(args: argparse.Namespace) -> int:
    """Handle validating test results against cases.json."""
    cases_path = Path(args.cases)
    if not cases_path.exists():
        print(f"Error: cases.json not found at {args.cases}", file=sys.stderr)
        return EXIT_USAGE_ERROR

    try:
        results, metadata = load_results_file(args.results)
    except FileNotFoundError as e:
        print(f"Error: {e}", file=sys.stderr)
        return EXIT_USAGE_ERROR
    except Exception as e:
        print(f"Error parsing results file: {e}", file=sys.stderr)
        return EXIT_VALIDATION_ERROR

    registry = CasesRegistry.load(cases_path)
    schema_errs = registry.validate_registry_schema()
    if schema_errs:
        for err in schema_errs:
            print(f"Registry schema error: {err}", file=sys.stderr)
        return EXIT_VALIDATION_ERROR

    validator = ReportValidator(registry)
    val_report = validator.validate_results(
        results=results,
        suite=args.suite,
        profile=args.profile,
        require_all_cases=getattr(args, "require_all", False),
        require_matrix=getattr(args, "require_matrix", False),
        required_directions=getattr(args, "required_directions", None),
        require_peers=getattr(args, "require_peers", False),
        spec_adapter=getattr(args, "spec_adapter", False),
        strict_retries=getattr(args, "strict", True),
        metadata=metadata,
    )

    if not val_report.is_valid:
        print("Validation FAILED:", file=sys.stderr)
        for err in val_report.errors:
            print(f"  ❌ {err}", file=sys.stderr)
        return EXIT_VALIDATION_ERROR

    # Check if any tests actually failed
    any_failed = any(r.status == ExecutionStatus.FAILED.value for r in results)
    if any_failed:
        print("Validation succeeded, but one or more tests failed execution.", file=sys.stderr)
        return EXIT_TEST_FAILURE

    if getattr(args, "spec_adapter", False):
        print("Spec-derived adapter validation PASSED; original upstream qualification remains open.")
    else:
        print("Validation PASSED: All matrix rows, pins, and test cases conform to specification.")
    return EXIT_SUCCESS


def handle_aggregate(args: argparse.Namespace) -> int:
    """Handle aggregating test results, outputting report, and determining exit code."""
    cases_path = Path(args.cases)
    if not cases_path.exists():
        print(f"Error: cases.json not found at {args.cases}", file=sys.stderr)
        return EXIT_USAGE_ERROR

    try:
        results, metadata = load_results_file(args.results)
    except FileNotFoundError as e:
        print(f"Error: {e}", file=sys.stderr)
        return EXIT_USAGE_ERROR
    except Exception as e:
        print(f"Error reading results: {e}", file=sys.stderr)
        return EXIT_VALIDATION_ERROR

    registry = CasesRegistry.load(cases_path)
    schema_errs = registry.validate_registry_schema()
    if schema_errs:
        for err in schema_errs:
            print(f"Registry schema error: {err}", file=sys.stderr)
        return EXIT_VALIDATION_ERROR
    aggregator = ReportAggregator(registry)
    report = aggregator.aggregate(
        results=results,
        suite=args.suite,
        profile=args.profile,
        require_all_cases=getattr(args, "require_all", False),
        require_matrix=getattr(args, "require_matrix", False),
        required_directions=getattr(args, "required_directions", None),
        require_peers=getattr(args, "require_peers", False),
        spec_adapter=getattr(args, "spec_adapter", False),
        strict_retries=getattr(args, "strict", True),
        metadata=metadata,
    )

    if args.output:
        report.write_json(args.output)

    fmt = getattr(args, "format", "terminal")
    no_color = getattr(args, "no_color", False)
    use_color = not no_color and sys.stdout.isatty()

    if fmt == "markdown":
        print(report.to_markdown())
    elif fmt == "json":
        print(report.to_json())
    else:
        print(report.to_terminal(use_color=use_color))

    if not report.overall_passed:
        # Distinguish validation error (matrix/pin/retries) from pure test failures
        has_validation_err = any(
            "Duplicate" in r
            or "Missing" in r
            or "pin" in r.lower()
            or "substitution" in r.lower()
            or "Empty" in r
            or "Retry hid" in r
            or "Unqualified" in r
            or "cannot be reported passed" in r
            or "No passing results" in r
            or "Spec-derived adapter" in r
            for r in report.failure_reasons
        )
        if has_validation_err:
            return EXIT_VALIDATION_ERROR
        return EXIT_TEST_FAILURE

    return EXIT_SUCCESS


def main(argv: Optional[List[str]] = None) -> int:
    """CLI entrypoint."""
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.subcommand == "record":
        return handle_record(args)
    elif args.subcommand == "validate":
        return handle_validate(args)
    elif args.subcommand == "aggregate":
        return handle_aggregate(args)
    else:
        # Default behavior when no subcommand is specified
        if args.results:
            return handle_aggregate(args)
        parser.print_help(sys.stderr)
        return EXIT_USAGE_ERROR


if __name__ == "__main__":
    sys.exit(main())
