"""Fail-closed summary for the pinned upstream Go HTTP/2 server probes."""

import argparse
import json
from pathlib import Path
import sys


GRPC_SOURCE_SHA = "d1487957db6658bc532b72871775148229836627"
CASES = {
    "framing": (
        "TestSoonClientShortSettings",
        "TestSoonShortPreface",
        "TestSoonUnknownFrameType",
        "TestSoonClientPrefaceWithStreamId",
        "TestSoonSmallMaxFrameSize",
        "TestSoonAllSettingsFramesAcked",
    ),
    "tls": (
        "TestSoonTLSApplicationProtocol",
        "TestSoonTLSMaxVersion",
        "TestSoonTLSBadCipherSuites",
    ),
}


def summarize(output: str, mode: str, exit_code: int, source_sha: str) -> dict:
    if mode not in CASES or source_sha != GRPC_SOURCE_SHA:
        raise ValueError("unsupported upstream probe mode or grpc/grpc source SHA")
    reports = [
        json.loads(line)
        for line in output.splitlines()
        if line.startswith('{"cases":')
    ]
    if len(reports) != 1 or not isinstance(reports[0].get("cases"), list):
        raise ValueError("expected exactly one upstream Go TestMain case report")

    by_name = {}
    for row in reports[0]["cases"]:
        if not isinstance(row, dict) or not isinstance(row.get("name"), str):
            raise ValueError("malformed upstream Go case row")
        name = row["name"]
        if name in by_name:
            raise ValueError(f"duplicate upstream Go case: {name}")
        passed = row.get("passed")
        skipped = row.get("skipped", False)
        fatal = row.get("fatal", False)
        if any(not isinstance(flag, bool) for flag in (passed, skipped, fatal)):
            raise ValueError(f"malformed status flags for {name}")
        if passed and (skipped or fatal):
            raise ValueError(f"contradictory upstream Go status for {name}")
        by_name[name] = row
    expected = set(CASES["framing"]) | set(CASES["tls"])
    if set(by_name) != expected:
        raise ValueError(
            f"upstream Go case drift: missing={sorted(expected - set(by_name))}, "
            f"unexpected={sorted(set(by_name) - expected)}"
        )

    failures = []
    rows = []
    for name in sorted(expected):
        row = by_name[name]
        selected = name in CASES[mode]
        if selected:
            status = "not_run" if row.get("skipped", False) else (
                "passed" if row["passed"] else "failed"
            )
            if status != "passed":
                failures.append(f"{name}: {status}")
        else:
            status = "skipped" if row.get("skipped", False) else "unexpected_execution"
            if status != "skipped":
                failures.append(f"{name}: ran outside {mode} profile")
        rows.append({"name": name, "status": status})
    if exit_code != 0:
        failures.append(f"upstream Go process exited {exit_code}")
    return {
        "schema_version": 1,
        "suite": "upstream_http2_server",
        "mode": mode,
        "source_sha": source_sha,
        "process_exit_code": exit_code,
        "cases": rows,
        "qualified": not failures,
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(CASES), required=True)
    parser.add_argument("--go-exit", type=int, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--go-version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = summarize(
            args.log.read_text(encoding="utf-8"),
            args.mode,
            args.go_exit,
            args.source_sha,
        )
        report["go_version"] = args.go_version
        report["raw_log"] = str(args.log)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    except (OSError, ValueError) as exc:
        print(f"upstream Go HTTP/2 proof incomplete: {exc}", file=sys.stderr)
        return 2
    if not report["qualified"]:
        print("upstream Go HTTP/2 probe failed: " + "; ".join(report["failures"]), file=sys.stderr)
        return 1
    print(f"upstream Go {args.mode}: {len(CASES[args.mode])} required probes passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
