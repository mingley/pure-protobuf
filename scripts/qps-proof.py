#!/usr/bin/env python3
"""Prepare one official QPS scenario and validate the driver's actual output."""

import argparse
import hashlib
import json
import math
import random
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


def execution_plan(scenarios: list[str], directions: list[str], repeats: int, seed: int) -> dict:
    """Independent seeded shuffles for each repeat and each peer comparison."""
    if repeats < 1 or not scenarios or not directions:
        raise ValueError("execution plan requires scenarios, directions and positive repeats")
    if len(set(scenarios)) != len(scenarios) or len(set(directions)) != len(directions):
        raise ValueError("execution plan contains duplicate scenarios or directions")
    runs = []
    for repeat in range(1, repeats + 1):
        order = list(scenarios)
        random.Random(f"{seed}:scenario:{repeat}").shuffle(order)
        for scenario in order:
            peers = list(directions)
            random.Random(f"{seed}:peer:{repeat}:{scenario}").shuffle(peers)
            runs.extend({"repeat": repeat, "scenario": scenario, "direction": peer} for peer in peers)
    return {"schema_version": 1, "seed": seed, "repeats": repeats, "runs": runs}


def validate_accounting(window: object) -> None:
    if not isinstance(window, dict) or window.get("schema_version") != 1:
        raise ValueError("missing supported independent worker accounting")
    if window.get("window_kind") != "completion_mark" or window.get("drain_seconds") != 0:
        raise ValueError("unsupported accounting window/drain semantics")
    fields = ("offered", "dispatched", "completed", "successful", "failed", "rejected",
              "timed_out", "unfinished", "incoming_in_flight", "carried_in_completed")
    for field in fields:
        value = window.get(field)
        if type(value) is not int or value < 0:
            raise ValueError(f"invalid accounting counter {field}")
    seconds = window.get("window_seconds")
    if type(seconds) not in (int, float) or not math.isfinite(seconds) or seconds <= 0:
        raise ValueError("invalid accounting window duration")
    if window["offered"] != window["dispatched"] + window["rejected"]:
        raise ValueError("offered != dispatched + rejected")
    if window["incoming_in_flight"] + window["dispatched"] != window["completed"] + window["unfinished"]:
        raise ValueError("incoming + dispatched != completed + unfinished")
    if window["completed"] != window["successful"] + window["failed"]:
        raise ValueError("completed != successful + failed")
    if window["timed_out"] > window["failed"]:
        raise ValueError("timed_out exceeds failed")
    if window["carried_in_completed"] > min(window["incoming_in_flight"], window["completed"]):
        raise ValueError("carried-in completions exceed their source")
    if window["completed"] - window["carried_in_completed"] > window["dispatched"]:
        raise ValueError("new completions exceed this window's dispatches")
    for key in ("service_latency_nanos", "scheduled_latency_nanos"):
        histogram = window.get(key, {})
        buckets = histogram.get("bucket")
        if (histogram.get("count") != window["completed"] or not isinstance(buckets, list)
                or not buckets or any(type(bucket) is not int or bucket < 0 for bucket in buckets)
                or sum(buckets) != window["completed"]):
            raise ValueError(f"{key} histogram does not reconcile with completed")
        for field in ("sum", "min_seen", "max_seen"):
            value = histogram.get(field)
            if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                raise ValueError(f"invalid {key}.{field}")
    if window["scheduled_latency_nanos"]["sum"] < window["service_latency_nanos"]["sum"]:
        raise ValueError("scheduled-send latency is shorter than service latency")


def accounting_from_log(path: Path) -> list[dict]:
    prefix = "QPS_ACCOUNTING "
    windows = [json.loads(line[len(prefix):]) for line in path.read_text().splitlines() if line.startswith(prefix)]
    validate_windows(windows)
    return windows


def validate_windows(windows: list[dict]) -> None:
    for window in windows:
        validate_accounting(window)
        if type(window.get("window_epoch")) is not int or window["window_epoch"] < 0 or type(window.get("reset")) is not bool:
            raise ValueError("missing accounting mark epoch/reset")
    for previous, current in zip(windows, windows[1:]):
        if previous.get("reset"):
            if (current.get("window_epoch") != previous.get("window_epoch", -1) + 1
                    or current["incoming_in_flight"] != previous["unfinished"]):
                raise ValueError("worker accounting reset windows do not join")
        elif (current["window_epoch"] != previous["window_epoch"]
              or current["window_seconds"] < previous["window_seconds"]
              or any(current[key] < previous[key] for key in ("offered", "dispatched", "completed", "rejected"))):
            raise ValueError("nonreset worker marks must remain cumulative")


def validate_measurement(result: dict, kind: str, scenario: dict, windows: list[dict], claims: bool) -> dict:
    """Reconcile accounting independently of the driver's rounded QPS summary.

    A completion mark has no drain: incoming and outgoing calls are explicit.
    This verifies evidence, not host isolation or an E1/E2 performance claim.
    """
    validate_result(result, kind)
    config = scenario.get("client_config", scenario.get("clientConfig", {}))
    load = config.get("load_params", config.get("loadParams", {}))
    offered_rate = load.get("poisson", {}).get("offered_load", load.get("poisson", {}).get("offeredLoad"))
    summary = result if kind == "cpp" else result["summary"]
    qps = summary["qps"]
    seconds = scenario.get("benchmark_seconds", scenario.get("benchmarkSeconds"))
    if offered_rate is not None:
        if type(offered_rate) not in (int, float) or not math.isfinite(offered_rate) or offered_rate <= 0:
            raise ValueError("invalid configured offered rate")
        if type(seconds) not in (int, float) or seconds <= 0:
            raise ValueError("missing configured measurement duration")
        expected = offered_rate * seconds
        capacity = config.get("client_channels", 1) * config.get("outstanding_rpcs_per_channel", 1)
        allowance = 6 * math.sqrt(expected) + capacity + expected * 0.01
        if qps * seconds > expected + allowance:
            raise ValueError("reported QPS exceeds configured Poisson offered load; independent accounting required")
    if claims and offered_rate is None:
        raise ValueError("claim evidence requires open-loop offered load")
    if not windows:
        if claims:
            raise ValueError("claim evidence lacks independent offered/rejected/unfinished and scheduled-send measurements")
        return {"accounting_verified": False, "claim_eligible": False, "reason": "independent worker accounting unavailable"}
    validate_windows(windows)
    window = windows[-1]
    if claims and (len(windows) < 2 or window.get("window_epoch", 0) < 1):
        raise ValueError("claim evidence lacks a warmup/reset boundary")
    elapsed = window["window_seconds"]
    if seconds and abs(elapsed - seconds) > max(0.25, seconds * 0.05):
        raise ValueError("worker accounting duration differs from scenario measurement window")
    if not math.isclose(qps, window["completed"] / elapsed, rel_tol=0.001, abs_tol=0.1):
        raise ValueError("driver QPS does not reconcile with worker completed/window_seconds")
    if kind == "go":
        if result["latencies"]["count"] != window["completed"]:
            raise ValueError("driver histogram does not match independent completion count")
        if result["latencies"]["bucket"] != window["service_latency_nanos"]["bucket"]:
            raise ValueError("driver service histogram differs from the independent histogram")
        stats = result["clientStats"]
        if len(stats) != 1 or not math.isclose(float(stats[0].get("timeElapsed", 0)), elapsed, rel_tol=1e-8):
            raise ValueError("driver and worker accounting do not describe the same mark window")
        errors = stats[0].get("requestResults", [])
        if not isinstance(errors, list):
            raise ValueError("driver omitted request-result accounting")
        counts = []
        for error in errors:
            count = error.get("count")
            if isinstance(count, str) and count.isdecimal():
                count = int(count)
            if type(count) is not int or count < 0 or not error.get("statusCode"):
                raise ValueError("driver returned an invalid error count")
            counts.append(count)
        if sum(counts) != window["failed"] + window["rejected"]:
            raise ValueError("driver errors do not reconcile with failed + rejected")
    if offered_rate:
        expected = offered_rate * elapsed
        if abs(window["offered"] - expected) > 6 * math.sqrt(expected) + max(100, expected * 0.01):
            raise ValueError("observed offered count does not match configured Poisson load")
    return {"accounting_verified": True, "claim_eligible": False,
            "claim_scope": "accounting preflight only; full benchmark contract still required",
            "successful_qps": window["successful"] / elapsed,
            "completion_qps": window["completed"] / elapsed,
            "measurement": window}


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
    validate.add_argument("--scenario", type=Path)
    validate.add_argument("--worker-log", type=Path)
    validate.add_argument("--proof-output", type=Path)
    validate.add_argument("--claims", action="store_true")
    plan = commands.add_parser("plan")
    plan.add_argument("--scenario", action="append", required=True)
    plan.add_argument("--direction", action="append", required=True)
    plan.add_argument("--repeats", type=int, default=1)
    plan.add_argument("--seed", type=int, default=210021)
    plan.add_argument("--output", type=Path, required=True)
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
            if args.scenario:
                scenario = json.loads(args.scenario.read_text())["scenarios"][0]
                windows = accounting_from_log(args.worker_log) if args.worker_log else []
                proof = validate_measurement(result, args.kind, scenario, windows, args.claims)
                if args.proof_output:
                    args.proof_output.write_text(json.dumps(proof, indent=2) + "\n")
            elif args.claims:
                raise ValueError("claim checking requires a prepared scenario")
        elif args.command == "plan":
            args.output.write_text(json.dumps(execution_plan(args.scenario, args.direction, args.repeats, args.seed), indent=2) + "\n")
        else:
            print(fingerprint(args.binary))
    except (OSError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
