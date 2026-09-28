#!/usr/bin/env python3
"""Prepare one official QPS scenario and validate the driver's actual output."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import sys


def fingerprint(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as binary:
        for chunk in iter(lambda: binary.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def prepare_scenario(source: Path, name: str, warmup: int, duration: int) -> dict:
    if warmup < 0 or duration < 0:
        raise ValueError("warmup and duration must be nonnegative")
    data = json.loads(source.read_text(encoding="utf-8"))
    scenarios = data.get("scenarios") if isinstance(data, dict) else None
    if not isinstance(scenarios, list):
        raise ValueError("official scenarios input needs a scenarios array")
    matches = [item for item in scenarios if isinstance(item, dict) and item.get("name") == name]
    if len(matches) != 1:
        raise ValueError(f"expected exactly one official scenario named {name}, found {len(matches)}")
    scenario = dict(matches[0])
    if warmup:
        scenario["warmup_seconds"] = warmup
    if duration:
        scenario["benchmark_seconds"] = duration
    return {"scenarios": [scenario]}


def validate_result(result: object, kind: str) -> None:
    if kind not in {"cpp", "go"}:
        raise ValueError(f"unsupported QPS driver kind: {kind}")
    if not isinstance(result, dict):
        raise ValueError("QPS driver result must be a JSON object")
    summary = result if kind == "cpp" else result.get("summary")
    qps = summary.get("qps") if isinstance(summary, dict) else None
    if isinstance(qps, bool) or not isinstance(qps, (int, float)) or not math.isfinite(qps) or qps <= 0:
        raise ValueError("driver returned missing, non-finite or zero QPS")
    if kind == "cpp":
        if set(result) != {"qps"}:
            raise ValueError("upstream C++ driver output is QPS-only, not a raw ScenarioResult")
        return

    if not isinstance(result.get("scenario"), dict) or not result["scenario"]:
        raise ValueError("integrated driver omitted the ScenarioResult scenario")
    for key in ("latency50", "latency99"):
        latency = summary.get(key)
        if isinstance(latency, bool) or not isinstance(latency, (int, float)) or not math.isfinite(latency) or latency <= 0:
            raise ValueError(f"integrated driver omitted a measured {key}")
    for stats_key, success_key in (("clientStats", "clientSuccess"), ("serverStats", "serverSuccess")):
        stats = result.get(stats_key)
        successes = result.get(success_key)
        if (
            not isinstance(stats, list)
            or not stats
            or not isinstance(successes, list)
            or len(stats) != len(successes)
            or not all(value is True for value in successes)
        ):
            raise ValueError(f"worker result omitted successful {stats_key} / {success_key}")
    histogram = result.get("latencies")
    if not isinstance(histogram, dict):
        raise ValueError("latency histogram missing")
    count = histogram.get("count")
    buckets = histogram.get("bucket")
    if (
        isinstance(count, bool)
        or not isinstance(count, int)
        or count <= 0
        or not isinstance(buckets, list)
        or not buckets
        or any(isinstance(bucket, bool) or not isinstance(bucket, int) or bucket < 0 for bucket in buckets)
        or sum(buckets) != count
    ):
        raise ValueError("latency histogram does not account for every observation")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("source", type=Path)
    prepare.add_argument("name")
    prepare.add_argument("warmup", type=int)
    prepare.add_argument("duration", type=int)
    prepare.add_argument("output", type=Path)
    validate = commands.add_parser("validate")
    validate.add_argument("result", type=Path)
    validate.add_argument("kind", choices=("cpp", "go"))
    driver = commands.add_parser("fingerprint")
    driver.add_argument("binary", type=Path)
    args = parser.parse_args()

    try:
        if args.command == "prepare":
            prepared = prepare_scenario(args.source, args.name, args.warmup, args.duration)
            args.output.write_text(json.dumps(prepared, indent=2) + "\n", encoding="utf-8")
        elif args.command == "validate":
            result = json.loads(args.result.read_text(encoding="utf-8"))
            validate_result(result, args.kind)
        else:
            print(fingerprint(args.binary))
    except (OSError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
