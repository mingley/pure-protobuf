#!/usr/bin/env python3
"""Validate dev-loop CI evidence; an uploaded error report is not a measurement."""

import argparse
import json
import math
from pathlib import Path
import re
import sys

SHA = re.compile(r"[0-9a-f]{40}\Z")
METRICS = ("instructions", "allocs", "alloc_bytes", "syscalls", "locks", "wall_ns")
REQUIRED_METRICS = ("allocs", "alloc_bytes", "wall_ns")
FRESH_ENCODE_PROFILE = "independent-warmup-v1"
FRESH_ENCODE_CELLS = frozenset((
    "codec.pbrs.fresh_encode", "codec.prost.fresh_encode", "codec.v4.fresh_encode",
))


def revisions(event_name, event, default_head):
    if event_name == "pull_request":
        pair = (event["pull_request"]["base"]["sha"], event["pull_request"]["head"]["sha"])
    elif event_name == "push":
        pair = (event["before"], event["after"])
    elif event_name == "workflow_dispatch":
        inputs = event.get("inputs", {})
        pair = (inputs.get("base_sha", ""), inputs.get("head_sha") or default_head)
    else:
        raise ValueError(f"unsupported event: {event_name}")
    for name, value in zip(("base", "head"), pair):
        if not isinstance(value, str) or not SHA.fullmatch(value) or value == "0" * 40:
            raise ValueError(f"{name} must be a nonzero full 40-character commit SHA")
    return pair


def load_report(path):
    try:
        report = json.loads(Path(path).read_text())
        if not isinstance(report, dict):
            raise ValueError("report must be an object")
        return report
    except (OSError, ValueError) as error:
        return {"error": f"cannot read report: {error}"}


def measured(metric):
    if not isinstance(metric, dict) or metric.get("status") != "measured":
        return None
    data = metric.get("data", {})
    value = data.get("value") if isinstance(data, dict) else None
    if (isinstance(value, bool) or not isinstance(value, (int, float))
            or not math.isfinite(value) or value < 0
            or not isinstance(data.get("unit"), str) or not data["unit"].strip()):
        raise ValueError("measured metric needs a finite nonnegative number and unit")
    return value


def validate_report(report, expected_sha, label):
    errors = []
    cells = {}
    if report.get("error"):
        errors.append(f"{label}: {report['error']}")
    if report.get("schema") != "devloop/1":
        errors.append(f"{label}: missing or unsupported schema")
    if report.get("devloop_commit") != expected_sha:
        errors.append(f"{label}: source SHA differs from the requested revision")
    host = report.get("host", {})
    if not isinstance(host, dict):
        host = {}
    for key in ("os", "arch", "cpu", "rustc"):
        value = host.get(key)
        if not isinstance(value, str) or not value.strip() or value == "unknown":
            errors.append(f"{label}: missing host {key}")
    for key in ("perf", "strace", "valgrind"):
        if not isinstance(host.get(key), bool):
            errors.append(f"{label}: missing tool availability for {key}")
    rows = report.get("cells")
    if not isinstance(rows, list) or not rows:
        errors.append(f"{label}: no measured cells")
        return cells, errors
    for cell in rows:
        if (not isinstance(cell, dict) or not isinstance(cell.get("id"), str)
                or not cell["id"].strip()):
            errors.append(f"{label}: invalid cell")
            continue
        name = cell["id"]
        if name in cells:
            errors.append(f"{label}: duplicate cell {name}")
        cells[name] = cell
        profile = cell.get("fresh_encode_profile")
        if profile is not None:
            if name not in FRESH_ENCODE_CELLS:
                errors.append(f"{label}/{name}: unexpected fresh encode profile")
            elif not isinstance(profile, str) or profile != FRESH_ENCODE_PROFILE:
                errors.append(f"{label}/{name}: unsupported fresh encode profile {profile!r}")
        for key in ("kind", "codec"):
            if not isinstance(cell.get(key), str) or not cell[key].strip():
                errors.append(f"{label}/{name}: missing {key}")
        for key in ("iters", "repeats"):
            value = cell.get(key)
            if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
                errors.append(f"{label}/{name}: invalid {key}")
        fingerprint = cell.get("input_wire_fingerprint")
        if fingerprint is not None and not isinstance(fingerprint, str):
            errors.append(f"{label}/{name}: invalid input_wire_fingerprint")
        if (name.startswith(("codec.adoption.", "rpc.adoption.", "rpc.tonic_transport."))
                and (not isinstance(fingerprint, str) or not fingerprint.strip())):
            errors.append(f"{label}/{name}: input_wire_fingerprint missing or blank")
        length = cell.get("input_wire_bytes")
        if (length is not None and (isinstance(length, bool) or not isinstance(length, int)
                                    or not 0 <= length <= 2**64 - 1)):
            errors.append(f"{label}/{name}: invalid input_wire_bytes")
        for key in METRICS:
            metric = cell.get(key)
            try:
                value = measured(metric)
                if value is None:
                    data = metric.get("data", {}) if isinstance(metric, dict) else {}
                    reason = data.get("reason") if isinstance(data, dict) else None
                    if (not isinstance(metric, dict) or metric.get("status") != "not_run"
                            or not isinstance(reason, str) or not reason.strip()):
                        raise ValueError("missing measured value or nonempty not_run reason")
                    if key in REQUIRED_METRICS:
                        raise ValueError("required measurement is not_run")
            except ValueError as error:
                errors.append(f"{label}/{name}/{key}: {error}")
    return cells, errors


def validate_pair(base, head, base_sha, head_sha):
    old, errors = validate_report(base, base_sha, "base")
    new, head_errors = validate_report(head, head_sha, "head")
    errors.extend(head_errors)
    if base.get("host") != head.get("host"):
        errors.append("host, compiler or tool availability differs between base and head")
    common = sorted(old.keys() & new.keys())
    if not common:
        errors.append("no common measured cells")
    eligible = {}
    fresh_encode_lifecycles = {}
    for name in common:
        before, after = old[name], new[name]
        if name in FRESH_ENCODE_CELLS:
            before_profile = before.get("fresh_encode_profile")
            after_profile = after.get("fresh_encode_profile")
            if before_profile is None and after_profile is None:
                fresh_encode_lifecycles[name] = "legacy-unreported"
            elif before_profile == after_profile == FRESH_ENCODE_PROFILE:
                fresh_encode_lifecycles[name] = FRESH_ENCODE_PROFILE
            else:
                fresh_encode_lifecycles[name] = "incompatible"
            if before_profile != after_profile:
                errors.append(f"{name}: fresh encode profiles differ between base and head")
        for key in ("kind", "codec", "iters", "repeats"):
            if before.get(key) != after.get(key):
                errors.append(f"{name}: {key} differs between base and head")
        # Qualification requires actual input evidence for adoption cells.
        # The local comparator still permits absent identities on legacy rows.
        first_fingerprint = before.get("input_wire_fingerprint")
        second_fingerprint = after.get("input_wire_fingerprint")
        if name.startswith(("codec.adoption.", "rpc.adoption.", "rpc.tonic_transport.")):
            if (not isinstance(first_fingerprint, str) or not first_fingerprint.strip()
                    or not isinstance(second_fingerprint, str) or not second_fingerprint.strip()):
                errors.append(f"{name}: input_wire_fingerprint missing or blank")
            elif first_fingerprint != second_fingerprint:
                errors.append(f"{name}: input_wire_fingerprint differs")
        if name.startswith(("rpc.adoption.", "rpc.tonic_transport.")):
            first_length = before.get("input_wire_bytes")
            second_length = after.get("input_wire_bytes")
            if first_length is None or second_length is None:
                errors.append(f"{name}: input_wire_bytes missing")
            elif first_length != second_length:
                errors.append(f"{name}: input_wire_bytes differs")
        metrics = []
        for key in METRICS:
            try:
                if measured(before.get(key)) is None or measured(after.get(key)) is None:
                    continue
            except ValueError:
                continue  # Already reported with the source label.
            if before[key]["data"]["unit"] != after[key]["data"]["unit"]:
                errors.append(f"{name}/{key}: units differ")
                continue
            method = before.get("instruction_method")
            if key == "instructions" and (
                not isinstance(method, str) or not method.strip() or method == "not_run"
                or method != after.get("instruction_method")
            ):
                errors.append(f"{name}: instruction methods differ or are missing")
                continue
            metrics.append(key)
        eligible[name] = metrics
    return {
        "schema": "devloop-ci/1",
        "base_sha": base_sha,
        "head_sha": head_sha,
        "qualified_for_noise": not errors,
        "errors": errors,
        "common_cells": common,
        "new_cells": sorted(new.keys() - old.keys()),
        "removed_cells": sorted(old.keys() - new.keys()),
        "fresh_encode_lifecycles": fresh_encode_lifecycles,
        "eligible_metrics": eligible if not errors else {},
    }


def summary(result, base, head):
    lines = ["## Dev-loop evidence", "",
             f"Base `{result['base_sha']}`; head `{result['head_sha']}`.", ""]
    if result["qualified_for_noise"]:
        lines.append("Valid base/head measurements; performance comparisons remain advisory.")
    else:
        lines.append("Incomplete or invalid evidence. This run must not count toward SB-20.")
        lines.extend(f"- {error}" for error in result["errors"])
    lines += ["", f"Common cells: {len(result['common_cells'])}; "
              f"new: {len(result['new_cells'])}; removed: {len(result['removed_cells'])}.",
              "Missing counters stay not_run and are excluded from eligible_metrics."]
    lifecycles = result.get("fresh_encode_lifecycles", {}).values()
    if "legacy-unreported" in lifecycles:
        lines.append("Legacy fresh-encode cells have no lifecycle profile and may reuse warmed "
                     "objects; they do not establish independent fresh-object costs.")
    if FRESH_ENCODE_PROFILE in lifecycles:
        lines.append(f"Fresh-encode profile `{FRESH_ENCODE_PROFILE}` uses independent warmup objects.")
    if result["qualified_for_noise"]:
        old = {cell["id"]: cell for cell in base["cells"]}
        new = {cell["id"]: cell for cell in head["cells"]}
        lines += ["", "| Cell | Instructions base → head | Allocations base → head | Allocation bytes base → head | Reported unit |",
                  "|---|---|---|---|---|"]
        for name in result["common_cells"]:
            values = []
            for metric in ("instructions", "allocs", "alloc_bytes"):
                pair = [measured(report[name].get(metric)) for report in (old, new)]
                values.append(" → ".join(
                    "not_run" if value is None else
                    (str(value) if metric == "alloc_bytes" else f"{value:.3f}")
                    for value in pair
                ))
            unit = old[name]["alloc_bytes"]["data"]["unit"]
            unit = unit.replace("\\", "\\\\").replace("|", "\\|").replace("\r", "\\r").replace("\n", "\\n")
            lines.append(f"| {name} | {values[0]} | {values[1]} | {values[2]} | {unit} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    resolve = commands.add_parser("revisions")
    resolve.add_argument("--event-name", required=True)
    resolve.add_argument("--event", required=True)
    resolve.add_argument("--default-head", required=True)
    verify = commands.add_parser("verify")
    for option in ("base", "head", "base-sha", "head-sha", "out", "summary"):
        verify.add_argument("--" + option, required=True)
    args = parser.parse_args()
    if args.command == "revisions":
        try:
            base, head = revisions(args.event_name, json.loads(Path(args.event).read_text()), args.default_head)
        except (OSError, ValueError, KeyError, TypeError) as error:
            parser.error(str(error))
        print(f"base_sha={base}\nhead_sha={head}")
        return 0
    for value in (args.base_sha, args.head_sha):
        if not SHA.fullmatch(value) or value == "0" * 40:
            parser.error("expected full nonzero commit SHAs")
    base, head = load_report(args.base), load_report(args.head)
    result = validate_pair(base, head, args.base_sha, args.head_sha)
    Path(args.out).write_text(json.dumps(result, indent=2, allow_nan=False) + "\n")
    Path(args.summary).write_text(summary(result, base, head))
    print("valid measurements" if result["qualified_for_noise"] else "invalid measurements")
    return 0 if result["qualified_for_noise"] else 1


if __name__ == "__main__":
    sys.exit(main())
