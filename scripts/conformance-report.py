#!/usr/bin/env python3
"""Validate both suites in every pinned protobuf conformance runner pass."""

import argparse
import json
from pathlib import Path
import re
import sys


PIN = "v35.1"
EDITION = "2023"
BINARY_JSON_CASES = 5631
TEXT_CASES = 909
SUMMARY = re.compile(
    r"CONFORMANCE SUITE (PASSED|FAILED):\s*(\d+) successes,\s*(\d+) skipped,"
    r"\s*(\d+) expected failures,\s*(\d+) unexpected failures"
)
RUNS = (
    ("required_run_1", "required_1/runner.log"),
    ("required_run_2", "required_2/runner.log"),
    ("recommended", "recommended/runner.log"),
)


def parse_log(content: str) -> dict:
    # The pinned runner prints binary/JSON first and text second, without labels.
    matches = SUMMARY.findall(content)
    run = {
        "status": "failed",
        "successes": 0,
        "text_successes": 0,
        "skipped": 0,
        "text_skipped": 0,
        "expected_failures": 0,
        "text_expected_failures": 0,
        "unexpected_failures": 0,
        "text_unexpected_failures": 0,
    }
    if len(matches) != 2:
        run["error"] = f"expected 2 suite summaries (binary/JSON and text), found {len(matches)}"
        return run

    for match, prefix, expected in zip(
        matches, ("", "text_"), (BINARY_JSON_CASES, TEXT_CASES)
    ):
        status, successes, skipped, expected_failures, unexpected = match
        run[f"{prefix}successes"] = int(successes)
        run[f"{prefix}skipped"] = int(skipped)
        run[f"{prefix}expected_failures"] = int(expected_failures)
        run[f"{prefix}unexpected_failures"] = int(unexpected)
        if status != "PASSED" or int(successes) != expected or any(
            int(n) for n in (skipped, expected_failures, unexpected)
        ):
            run["error"] = (
                f"{prefix or 'binary_json_'}suite: {status}, {successes}/{expected} successes,"
                f" {skipped} skipped, {expected_failures} expected failures,"
                f" {unexpected} unexpected failures"
            )
    if "error" not in run:
        run["status"] = "passed"
    return run


def build_summary(
    out_dir: Path,
    pin: str,
    sha: str,
    commit: str,
    timestamp: str,
    edition: str,
    dirty: bool,
) -> dict:
    errors = []
    if pin != PIN or edition != EDITION:
        errors.append(
            f"unreviewed conformance contract: expected {PIN} / Edition {EDITION},"
            f" got {pin} / Edition {edition}"
        )
    runs = {}
    for name, filename in RUNS:
        path = out_dir / filename
        if not path.is_file():
            runs[name] = {"status": "not_run", "error": f"missing runner log: {filename}"}
        else:
            runs[name] = parse_log(path.read_text(encoding="utf-8", errors="replace"))
        if runs[name]["status"] != "passed":
            errors.append(f"{name}: {runs[name]['error']}")

    counts = {
        "required_run_1": runs["required_run_1"].get("successes", 0),
        "required_run_2": runs["required_run_2"].get("successes", 0),
        "recommended": runs["recommended"].get("successes", 0),
        "required_text_run_1": runs["required_run_1"].get("text_successes", 0),
        "required_text_run_2": runs["required_run_2"].get("text_successes", 0),
        "recommended_text": runs["recommended"].get("text_successes", 0),
        "total_failures": sum(
            run.get("unexpected_failures", 0) + run.get("text_unexpected_failures", 0)
            for run in runs.values()
        ),
    }
    return {
        "runner_pin": pin,
        "runner_sha": sha,
        "git_commit": commit,
        "dirty_source": dirty,
        "timestamp": timestamp,
        "maximum_edition": edition,
        "test_counts": counts,
        "runs": runs,
        "validation_errors": errors,
        "overall_status": "passed" if not errors else "failed",
        "overall_passed": not errors,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out_dir", type=Path)
    parser.add_argument("pin")
    parser.add_argument("sha")
    parser.add_argument("commit")
    parser.add_argument("timestamp")
    parser.add_argument("edition")
    parser.add_argument("dirty", choices=("0", "1"))
    args = parser.parse_args()
    summary = build_summary(
        args.out_dir, args.pin, args.sha, args.commit, args.timestamp,
        args.edition, args.dirty == "1",
    )
    output = args.out_dir / "summary.json"
    output.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"Conformance summary written to {output}")
    if summary["validation_errors"]:
        for error in summary["validation_errors"]:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
