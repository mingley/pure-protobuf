#!/usr/bin/env python3
"""Compare registered interop cases against target upstream inventories.

Standalone standard-library Python 3 tool (zero external dependencies) that:
1. Validates schema compliance of tests/interop/cases.json.
2. Compares registered cases against a target inventory or list of upstream case names.
3. Detects newly added upstream cases as 'uncovered' (unclassified).
4. Flags missing (removed) or renamed cases between registry and upstream.
5. Exits 0 if all expected cases match, or 1 if drift / unclassified cases are detected.

Usage:
  python3 scripts/check-upstream-cases.py
  python3 scripts/check-upstream-cases.py --cases tests/interop/cases.json
  python3 scripts/check-upstream-cases.py --upstream path/to/target_inventory.json
  python3 scripts/check-upstream-cases.py --upstream-cases empty_unary,large_unary
  python3 scripts/check-upstream-cases.py --format markdown --output drift-summary.md
  python3 scripts/check-upstream-cases.py --schema-only
"""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
import difflib
import json
import os
from pathlib import Path
import re
import sys
from typing import Any, Dict, List, Optional, Set, Tuple, Union

# Exit codes
EXIT_SUCCESS = 0
EXIT_DRIFT_DETECTED = 1
EXIT_USAGE_ERROR = 2

# Allowed schema enums
VALID_DISPOSITIONS = {
    "passed",
    "failed",
    "not_run",
    "unsupported",
    "blocked_external",
    "not_applicable",
}

VALID_PEER_DIRECTIONS = {
    "both",
    "client_to_server",
    "server_to_client",
    "runner_to_binary",
    "driver_to_worker",
    "internal",
}

VALID_TRANSPORTS = {
    "http2_cleartext",
    "http2_tls",
    "http2_mtls",
    "alts",
    "pipe",
    "none",
}

VALID_PROFILES = {
    "core",
    "native",
    "tonic",
    "full",
    "extended",
}

# Authoritative list of 69 pinned upstream cases defined in tests/interop/cases.json
DEFAULT_PINNED_CASES = [
    # standard_interop (16)
    "empty_unary",
    "large_unary",
    "client_streaming",
    "server_streaming",
    "ping_pong",
    "empty_stream",
    "cancel_after_begin",
    "cancel_after_first_response",
    "timeout_on_sleeping_server",
    "custom_metadata",
    "status_code_and_message",
    "special_status_message",
    "unimplemented_method",
    "unimplemented_service",
    "pick_first_unary",
    "cacheable_unary",
    # compression_interop (4)
    "client_compressed_unary",
    "server_compressed_unary",
    "client_compressed_streaming",
    "server_compressed_streaming",
    # http2_negative (8)
    "rst_after_header",
    "rst_after_data",
    "rst_during_data",
    "goaway",
    "ping",
    "max_streams",
    "data_frame_padding",
    "no_df_padding_sanity_test",
    # server_probe (2)
    "server_tls_probe",
    "server_framing_probe",
    # connection_backoff (1)
    "connection_backoff",
    # soak (2)
    "rpc_soak",
    "channel_soak",
    # scaling (1)
    "max_concurrent_streams_connection_scaling",
    # auth (7)
    "compute_engine_creds",
    "jwt_token_creds",
    "oauth2_auth_token",
    "per_rpc_creds",
    "google_default_credentials",
    "compute_engine_channel_credentials",
    "alts_credentials",
    # orca (2)
    "orca_per_rpc",
    "orca_oob",
    # xds_lb (12)
    "round_robin",
    "backends_restart",
    "circuit_breaking",
    "outlier_detection",
    "xds_ping_pong",
    "traffic_splitting",
    "path_matching",
    "header_matching",
    "fault_injection",
    "timeout",
    "metadata_exchange",
    "app_net_security",
    # protobuf_conformance (6)
    "protobuf_binary_conformance",
    "protobuf_json_conformance",
    "protobuf_text_conformance",
    "protobuf_enforce_recommended",
    "rust_shared_application_tests",
    "upb_kernel_internals",
    # performance (8)
    "worker_service_run_server",
    "worker_service_run_client",
    "worker_service_core_count",
    "worker_service_quit_worker",
    "benchmark_service_unary",
    "benchmark_service_streaming",
    "benchmark_service_streaming_from_client",
    "benchmark_service_streaming_from_server",
]


@dataclass
class RenameMatch:
    """Detected rename between an old registered case and a new upstream case."""

    old_case: str
    new_case: str
    similarity: float
    reason: str

    def to_dict(self) -> Dict[str, Any]:
        return {
            "old_case": self.old_case,
            "new_case": self.new_case,
            "similarity": round(self.similarity, 3),
            "reason": self.reason,
        }


@dataclass
class DriftReport:
    """Result of comparing registered cases against upstream inventory."""

    timestamp: str
    cases_path: str
    target_source: str
    registered_count: int
    target_count: int
    matched: List[str] = field(default_factory=list)
    uncovered: List[str] = field(default_factory=list)  # Added upstream, unclassified in registry
    missing: List[str] = field(default_factory=list)    # Present in registry, missing upstream
    renamed: List[RenameMatch] = field(default_factory=list)
    schema_errors: List[str] = field(default_factory=list)

    @property
    def has_drift(self) -> bool:
        return bool(self.uncovered or self.missing or self.renamed or self.schema_errors)

    @property
    def exit_code(self) -> int:
        return EXIT_DRIFT_DETECTED if self.has_drift else EXIT_SUCCESS

    def to_dict(self) -> Dict[str, Any]:
        return {
            "timestamp": self.timestamp,
            "cases_path": self.cases_path,
            "target_source": self.target_source,
            "has_drift": self.has_drift,
            "exit_code": self.exit_code,
            "summary": {
                "registered_cases": self.registered_count,
                "target_cases": self.target_count,
                "matched_count": len(self.matched),
                "uncovered_count": len(self.uncovered),
                "missing_count": len(self.missing),
                "renamed_count": len(self.renamed),
                "schema_errors_count": len(self.schema_errors),
            },
            "matched": self.matched,
            "uncovered": self.uncovered,
            "missing": self.missing,
            "renamed": [r.to_dict() for r in self.renamed],
            "schema_errors": self.schema_errors,
        }

    def format_text(self, verbose: bool = False) -> str:
        lines: List[str] = []
        lines.append("=" * 72)
        lines.append("UPSTREAM INTEROPERABILITY CASE DRIFT REPORT")
        lines.append("=" * 72)
        lines.append(f"Registry:      {self.cases_path}")
        lines.append(f"Target Source: {self.target_source}")
        lines.append(f"Timestamp:     {self.timestamp}")
        lines.append("-" * 72)
        lines.append(f"Registered Cases:  {self.registered_count:4d}")
        lines.append(f"Target Inventory:  {self.target_count:4d}")
        lines.append(f"Matched Cases:     {len(self.matched):4d}")
        lines.append(f"Uncovered (New):   {len(self.uncovered):4d}")
        lines.append(f"Missing (Removed): {len(self.missing):4d}")
        lines.append(f"Renamed:           {len(self.renamed):4d}")
        lines.append(f"Schema Errors:     {len(self.schema_errors):4d}")
        lines.append("-" * 72)

        if self.schema_errors:
            lines.append("SCHEMA ERRORS:")
            for err in self.schema_errors:
                lines.append(f"  [!] {err}")
            lines.append("-" * 72)

        if self.uncovered:
            lines.append("UNCOVERED NEW UPSTREAM CASES (Require Classification in cases.json):")
            for c in self.uncovered:
                lines.append(f"  [+] {c}")
            lines.append("-" * 72)

        if self.missing:
            lines.append("MISSING CASES (Present in registry, missing in target inventory):")
            for c in self.missing:
                lines.append(f"  [-] {c}")
            lines.append("-" * 72)

        if self.renamed:
            lines.append("RENAMED CASES (Candidate renames requiring reconciliation):")
            for r in self.renamed:
                lines.append(f"  [~] '{r.old_case}' -> '{r.new_case}' (similarity: {r.similarity:.2f}, reason: {r.reason})")
            lines.append("-" * 72)

        if verbose and self.matched:
            lines.append("MATCHED CASES:")
            for c in self.matched:
                lines.append(f"  [=] {c}")
            lines.append("-" * 72)

        if self.has_drift:
            lines.append("VERDICT: FAILED - Upstream drift or schema errors detected (exit code 1)")
        else:
            lines.append("VERDICT: PASSED - All expected cases match target inventory (exit code 0)")
        lines.append("=" * 72)
        return "\n".join(lines)

    def format_markdown(self, verbose: bool = False) -> str:
        lines: List[str] = []
        status_badge = "FAILED" if self.has_drift else "PASSED"
        lines.append(f"### Upstream Case Drift Report: {status_badge}\n")
        lines.append(f"- **Registry**: `{self.cases_path}`")
        lines.append(f"- **Target Source**: `{self.target_source}`")
        lines.append(f"- **Timestamp**: `{self.timestamp}`\n")

        lines.append("| Metric | Count | Status |")
        lines.append("|---|---|---|")
        lines.append(f"| **Registered Cases** | {self.registered_count} | - |")
        lines.append(f"| **Target Inventory** | {self.target_count} | - |")
        lines.append(f"| **Matched Cases** | {len(self.matched)} | {'Pass' if len(self.matched) > 0 else 'Warn'} |")
        lines.append(f"| **Uncovered (New)** | {len(self.uncovered)} | {'FAIL' if self.uncovered else 'Clean'} |")
        lines.append(f"| **Missing (Removed)** | {len(self.missing)} | {'FAIL' if self.missing else 'Clean'} |")
        lines.append(f"| **Renamed Cases** | {len(self.renamed)} | {'FAIL' if self.renamed else 'Clean'} |")
        lines.append(f"| **Schema Errors** | {len(self.schema_errors)} | {'FAIL' if self.schema_errors else 'Clean'} |\n")

        if self.schema_errors:
            lines.append("#### Schema Errors\n")
            for err in self.schema_errors:
                lines.append(f"- :x: {err}")
            lines.append("")

        if self.uncovered:
            lines.append("#### Uncovered Upstream Cases (Require Classification)\n")
            for c in self.uncovered:
                lines.append(f"- :new: `{c}`")
            lines.append("")

        if self.missing:
            lines.append("#### Missing Cases (Absent from Target Inventory)\n")
            for c in self.missing:
                lines.append(f"- :warning: `{c}`")
            lines.append("")

        if self.renamed:
            lines.append("#### Renamed Cases (Require Reconciliation)\n")
            for r in self.renamed:
                lines.append(f"- :arrows_counterclockwise: `{r.old_case}` &rarr; `{r.new_case}` (similarity: {r.similarity:.2f}, reason: {r.reason})")
            lines.append("")

        if verbose and self.matched:
            lines.append("<details><summary>Matched Cases (expand)</summary>\n")
            for c in self.matched:
                lines.append(f"- `{c}`")
            lines.append("\n</details>\n")

        return "\n".join(lines)


def validate_cases_schema(data: Dict[str, Any]) -> List[str]:
    """Validate tests/interop/cases.json structure and completeness.

    Returns a list of error strings. Empty list indicates full compliance.
    """
    errors: List[str] = []

    # 1. Top-level required keys
    required_top = [
        "version",
        "updated_at",
        "title",
        "description",
        "upstream_pins",
        "disposition_definitions",
        "suites",
        "summary",
        "cases",
    ]
    for key in required_top:
        if key not in data:
            errors.append(f"Missing top-level key: '{key}'")

    if not isinstance(data.get("version"), str) or not data.get("version"):
        errors.append("Field 'version' must be a non-empty string")

    # 2. Upstream pins
    pins = data.get("upstream_pins")
    if not isinstance(pins, dict):
        errors.append("Field 'upstream_pins' must be an object")
    else:
        for pin_name in ("protobuf", "grpc", "grpc_go"):
            if pin_name not in pins:
                errors.append(f"upstream_pins missing required pin '{pin_name}'")
            elif not isinstance(pins[pin_name], dict) or "commit" not in pins[pin_name]:
                errors.append(f"upstream_pins['{pin_name}'] missing required 'commit' hash")

    # 3. Dispositions
    disp_defs = data.get("disposition_definitions")
    if not isinstance(disp_defs, dict):
        errors.append("Field 'disposition_definitions' must be an object")
    else:
        missing_disps = VALID_DISPOSITIONS - set(disp_defs.keys())
        if missing_disps:
            errors.append(f"disposition_definitions missing required states: {sorted(missing_disps)}")
        extra_disps = set(disp_defs.keys()) - VALID_DISPOSITIONS
        if extra_disps:
            errors.append(f"disposition_definitions contains unknown states: {sorted(extra_disps)}")

    # 4. Suites
    suites = data.get("suites")
    if not isinstance(suites, dict) or not suites:
        errors.append("Field 'suites' must be a non-empty object mapping suite IDs to descriptions")

    # 5. Cases list
    cases = data.get("cases")
    if not isinstance(cases, list) or not cases:
        errors.append("Field 'cases' must be a non-empty list of case objects")
        return errors

    seen_cases: Set[str] = set()
    actual_disp_counts: Dict[str, int] = {}
    actual_suite_counts: Dict[str, int] = {}

    for idx, c in enumerate(cases):
        if not isinstance(c, dict):
            errors.append(f"Case at index {idx} is not an object")
            continue

        case_id = c.get("case")
        if not case_id or not isinstance(case_id, str):
            errors.append(f"Case at index {idx} has missing or non-string 'case' identifier")
            continue

        if not re.match(r"^[a-zA-Z0-9_]+$", case_id):
            errors.append(f"Case '{case_id}' has invalid identifier syntax (alphanumeric and underscores only)")

        if case_id in seen_cases:
            errors.append(f"Duplicate case identifier: '{case_id}'")
        seen_cases.add(case_id)

        # Name
        if not c.get("name") or not isinstance(c.get("name"), str):
            errors.append(f"Case '{case_id}' missing required 'name' string")

        # Suite
        suite = c.get("suite")
        if isinstance(suites, dict):
            if not suite or suite not in suites:
                errors.append(f"Case '{case_id}' references undefined suite: '{suite}'")
            else:
                actual_suite_counts[suite] = actual_suite_counts.get(suite, 0) + 1

        # Procedure source
        if not c.get("procedure_source") or not isinstance(c.get("procedure_source"), str):
            errors.append(f"Case '{case_id}' missing required 'procedure_source'")

        # Peer direction
        direction = c.get("peer_direction")
        if direction not in VALID_PEER_DIRECTIONS:
            errors.append(f"Case '{case_id}' has invalid peer_direction '{direction}' (must be one of {sorted(VALID_PEER_DIRECTIONS)})")

        # Transport
        transport = c.get("transport")
        if transport not in VALID_TRANSPORTS:
            errors.append(f"Case '{case_id}' has invalid transport '{transport}' (must be one of {sorted(VALID_TRANSPORTS)})")

        # Profile
        profile = c.get("profile")
        if profile not in VALID_PROFILES:
            errors.append(f"Case '{case_id}' has invalid profile '{profile}' (must be one of {sorted(VALID_PROFILES)})")

        # Owner
        if not c.get("owner") or not isinstance(c.get("owner"), str):
            errors.append(f"Case '{case_id}' missing required 'owner' task ID")

        # Disposition
        disp = c.get("disposition")
        if disp not in VALID_DISPOSITIONS:
            errors.append(f"Case '{case_id}' has invalid disposition '{disp}' (must be one of {sorted(VALID_DISPOSITIONS)})")
        else:
            actual_disp_counts[disp] = actual_disp_counts.get(disp, 0) + 1

            # Justification requirement
            justification = c.get("justification")
            if disp != "passed":
                if not justification or not isinstance(justification, str) or not justification.strip():
                    errors.append(f"Case '{case_id}' with disposition '{disp}' requires a non-empty 'justification'")

        # Present coverage
        coverage = c.get("present_coverage")
        if not isinstance(coverage, dict):
            errors.append(f"Case '{case_id}' missing required 'present_coverage' object")
        else:
            cov_status = coverage.get("status")
            scope = coverage.get("evidence_scope")
            if scope not in (None, "original_procedure", "local_adapter"):
                errors.append(f"Case '{case_id}' has invalid evidence_scope '{scope}'")
            if cov_status not in VALID_DISPOSITIONS:
                errors.append(f"Case '{case_id}' has invalid coverage status '{cov_status}'")
            if cov_status != disp and not (
                cov_status == "passed"
                and disp in ("not_run", "failed")
                and scope == "local_adapter"
            ):
                errors.append(f"Case '{case_id}' coverage status '{cov_status}' does not match disposition '{disp}'")
            if scope == "local_adapter" and cov_status == disp:
                errors.append(f"Case '{case_id}' marks matching coverage as a local_adapter")
            if cov_status == "passed" and not coverage.get("evidence_file"):
                errors.append(f"Case '{case_id}' has passing coverage without an evidence file")
            if scope == "local_adapter" and not coverage.get("passing_directions"):
                errors.append(f"Case '{case_id}' local_adapter has no passing directions")
            if "passing_directions" not in coverage or not isinstance(coverage["passing_directions"], list):
                errors.append(f"Case '{case_id}' present_coverage missing 'passing_directions' list")

    # 6. Summary check
    summary = data.get("summary")
    if isinstance(summary, dict):
        total_cases = summary.get("total_cases")
        if total_cases != len(cases):
            errors.append(f"Summary total_cases ({total_cases}) != actual cases count ({len(cases)})")

        summary_disps = summary.get("by_disposition")
        if isinstance(summary_disps, dict):
            for d in VALID_DISPOSITIONS:
                expected_cnt = summary_disps.get(d, 0)
                actual_cnt = actual_disp_counts.get(d, 0)
                if expected_cnt != actual_cnt:
                    errors.append(f"Summary by_disposition['{d}'] ({expected_cnt}) != actual count ({actual_cnt})")

        summary_suites = summary.get("by_suite")
        if isinstance(summary_suites, dict) and isinstance(suites, dict):
            for s in suites:
                expected_cnt = summary_suites.get(s, 0)
                actual_cnt = actual_suite_counts.get(s, 0)
                if expected_cnt != actual_cnt:
                    errors.append(f"Summary by_suite['{s}'] ({expected_cnt}) != actual count ({actual_cnt})")

    return errors


def load_registered_cases(cases_path: Union[str, Path]) -> Tuple[Dict[str, Any], List[str]]:
    """Load and parse cases.json.

    Returns (cases_by_name, schema_errors).
    """
    path = Path(cases_path)
    if not path.exists():
        raise FileNotFoundError(f"Case registry not found: {path}")

    with path.open("r", encoding="utf-8") as f:
        data = json.load(f)

    schema_errors = validate_cases_schema(data)
    cases_list = data.get("cases", [])
    cases_by_name = {c["case"]: c for c in cases_list if isinstance(c, dict) and "case" in c}
    return cases_by_name, schema_errors


def load_target_inventory(source: Optional[Union[str, Path]] = None) -> Tuple[Set[str], str]:
    """Load target upstream case names from a file, string, or default inventory.

    Returns (case_names_set, source_description).
    """
    if source is None or source == "" or source == "default":
        return set(DEFAULT_PINNED_CASES), "builtin:pinned_v35.1_cases"

    # Direct comma-separated list
    if isinstance(source, str) and ("," in source or not Path(source).exists()):
        parts = [p.strip() for p in source.split(",") if p.strip()]
        if parts:
            return set(parts), "cli:--upstream-cases"

    path = Path(source)
    if not path.exists():
        raise FileNotFoundError(f"Target upstream inventory not found: {path}")

    content = path.read_text(encoding="utf-8").strip()

    # Try parsing as JSON
    try:
        data = json.loads(content)
        cases: Set[str] = set()

        if isinstance(data, list):
            for item in data:
                if isinstance(item, str):
                    cases.add(item.strip())
                elif isinstance(item, dict) and "case" in item:
                    cases.add(str(item["case"]).strip())
        elif isinstance(data, dict):
            if "cases" in data and isinstance(data["cases"], list):
                for item in data["cases"]:
                    if isinstance(item, str):
                        cases.add(item.strip())
                    elif isinstance(item, dict) and "case" in item:
                        cases.add(str(item["case"]).strip())
            elif "suites" in data and isinstance(data["suites"], dict):
                for suite_cases in data["suites"].values():
                    if isinstance(suite_cases, list):
                        cases.update(str(c).strip() for c in suite_cases if c)
            else:
                # Dict mapping case names to metadata
                cases.update(str(k).strip() for k in data.keys() if not k.startswith("$"))

        if cases:
            return cases, str(path)
    except json.JSONDecodeError:
        pass

    # Fallback to plain text lines
    cases = set()
    for line in content.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        # Extract first token
        parts = line.split()
        if parts:
            cases.add(parts[0].strip(","))

    return cases, str(path)


def detect_renames(
    missing_cases: Set[str],
    added_cases: Set[str],
    similarity_threshold: float = 0.70,
) -> Tuple[List[RenameMatch], Set[str], Set[str]]:
    """Detect candidate renames between missing registered cases and added upstream cases.

    Returns (renamed_list, matched_missing_set, matched_added_set).
    """
    candidates: List[RenameMatch] = []

    for old_case in missing_cases:
        for new_case in added_cases:
            # 1. Normalized match (e.g. empty_unary vs emptyUnary vs empty-unary)
            norm_old = re.sub(r"[^a-z0-9]", "", old_case.lower())
            norm_new = re.sub(r"[^a-z0-9]", "", new_case.lower())
            if norm_old == norm_new and norm_old:
                candidates.append(RenameMatch(old_case, new_case, 1.0, "normalized_case_match"))
                continue

            # 2. Common prefix/suffix stripping (e.g. test_empty_unary vs empty_unary)
            strip_old = re.sub(r"^(test_|case_)|(_call|_test)$", "", old_case.lower())
            strip_new = re.sub(r"^(test_|case_)|(_call|_test)$", "", new_case.lower())
            if strip_old == strip_new and strip_old:
                candidates.append(RenameMatch(old_case, new_case, 0.95, "prefix_suffix_match"))
                continue

            # 3. String sequence similarity
            ratio = difflib.SequenceMatcher(None, old_case.lower(), new_case.lower()).ratio()
            if ratio >= similarity_threshold:
                candidates.append(RenameMatch(old_case, new_case, round(ratio, 3), "name_similarity"))

    # Sort greedily by similarity descending, then length difference ascending
    candidates.sort(key=lambda c: (c.similarity, -abs(len(c.old_case) - len(c.new_case))), reverse=True)

    renamed: List[RenameMatch] = []
    used_old: Set[str] = set()
    used_new: Set[str] = set()

    for c in candidates:
        if c.old_case not in used_old and c.new_case not in used_new:
            used_old.add(c.old_case)
            used_new.add(c.new_case)
            renamed.append(c)

    return renamed, used_old, used_new


def check_upstream_drift(
    registered_cases: Dict[str, Any],
    target_case_names: Set[str],
    cases_path: str = "tests/interop/cases.json",
    target_source: str = "builtin",
    schema_errors: Optional[List[str]] = None,
    similarity_threshold: float = 0.70,
) -> DriftReport:
    """Compare registered cases against target upstream cases and produce DriftReport."""
    registered_names = set(registered_cases.keys())
    matched_names = registered_names & target_case_names

    potential_missing = registered_names - target_case_names
    potential_uncovered = target_case_names - registered_names

    renamed_matches, used_missing, used_uncovered = detect_renames(
        potential_missing,
        potential_uncovered,
        similarity_threshold=similarity_threshold,
    )

    final_missing = sorted(list(potential_missing - used_missing))
    final_uncovered = sorted(list(potential_uncovered - used_uncovered))
    final_matched = sorted(list(matched_names))

    return DriftReport(
        timestamp=datetime.now(timezone.utc).isoformat(),
        cases_path=cases_path,
        target_source=target_source,
        registered_count=len(registered_names),
        target_count=len(target_case_names),
        matched=final_matched,
        uncovered=final_uncovered,
        missing=final_missing,
        renamed=renamed_matches,
        schema_errors=schema_errors or [],
    )


def resolve_default_cases_path() -> Path:
    """Resolve path to tests/interop/cases.json from cwd or script location."""
    candidates = [
        Path.cwd() / "tests" / "interop" / "cases.json",
        Path(__file__).resolve().parent.parent / "tests" / "interop" / "cases.json",
        Path("tests/interop/cases.json"),
    ]
    for c in candidates:
        if c.exists():
            return c.resolve()
    return candidates[0]


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Check upstream gRPC and Protobuf interop cases for drift against tests/interop/cases.json",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--cases",
        type=str,
        default=None,
        help="Path to authoritative cases.json registry (default: tests/interop/cases.json)",
    )
    parser.add_argument(
        "--upstream",
        type=str,
        default=None,
        help="Path to newer upstream inventory (JSON or text file). If omitted, uses pinned inventory.",
    )
    parser.add_argument(
        "--upstream-cases",
        type=str,
        default=None,
        help="Comma-separated list of target upstream case names to compare against.",
    )
    parser.add_argument(
        "--schema-only",
        action="store_true",
        help="Only validate cases.json schema compliance without drift comparison.",
    )
    parser.add_argument(
        "--format",
        choices=["text", "json", "markdown"],
        default="text",
        help="Output report formatting.",
    )
    parser.add_argument(
        "--output",
        type=str,
        default=None,
        help="Write report output to specified file.",
    )
    parser.add_argument(
        "--similarity-threshold",
        type=float,
        default=0.70,
        help="Minimum similarity score [0.0 - 1.0] to classify a missing case as renamed.",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Include full list of matched cases in output.",
    )

    args = parser.parse_args(argv)

    cases_path = Path(args.cases) if args.cases else resolve_default_cases_path()

    try:
        registered_cases, schema_errors = load_registered_cases(cases_path)
    except Exception as e:
        sys.stderr.write(f"Error loading registry '{cases_path}': {e}\n")
        return EXIT_USAGE_ERROR

    if args.schema_only:
        if schema_errors:
            sys.stderr.write(f"Schema validation FAILED with {len(schema_errors)} error(s):\n")
            for err in schema_errors:
                sys.stderr.write(f"  - {err}\n")
            return EXIT_DRIFT_DETECTED
        print(f"Schema validation PASSED for '{cases_path}' ({len(registered_cases)} cases).")
        return EXIT_SUCCESS

    # Load target inventory
    target_source_arg = args.upstream_cases if args.upstream_cases else args.upstream
    try:
        target_cases, target_source_desc = load_target_inventory(target_source_arg)
    except Exception as e:
        sys.stderr.write(f"Error loading target inventory '{target_source_arg}': {e}\n")
        return EXIT_USAGE_ERROR

    report = check_upstream_drift(
        registered_cases=registered_cases,
        target_case_names=target_cases,
        cases_path=str(cases_path),
        target_source=target_source_desc,
        schema_errors=schema_errors,
        similarity_threshold=args.similarity_threshold,
    )

    if args.format == "json":
        output_text = json.dumps(report.to_dict(), indent=2)
    elif args.format == "markdown":
        output_text = report.format_markdown(verbose=args.verbose)
    else:
        output_text = report.format_text(verbose=args.verbose)

    if args.output:
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(output_text, encoding="utf-8")
    else:
        print(output_text)

    return report.exit_code


if __name__ == "__main__":
    sys.exit(main())
