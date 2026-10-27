#!/usr/bin/env python3
"""Check scoped RPC CPU evidence against the numerical benchmark contract.

Input is pbrs.rpc-cpu-evidence.v1, a frozen manifest of named cells, peer
pins, hosts and SHA-256-addressed paired raw JSON runs. The fixture in
tests/test_performance_claim.py documents its fields. Scope is deliberately
limited to C8 (client CPU/RPC) or D4 (server CPU/RPC), not every performance
category. Required peers come from bench/scoreboard/categories.json.

This preflight recomputes paired bootstrap intervals; supplied intervals and
winner labels have no authority. Exit 0 means every numerical/source check
passed *and* the 20% CPU, 5% p99 and 10% RSS bounds passed for every named
cell/architecture/required peer. Exit 1 retains invalid evidence, losses and
uncertainty. Synthetic fixtures cannot pass the evidence gate.

A successful preflight is necessary evidence, never certification: a JSON
checker cannot establish honest measurements, host isolation, equivalence
of application work, interoperability, sustained memory safety or absence of
benchmark detection. External qualification requirements remain explicit.
"""

import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import random
import re
import statistics
import sys

ROOT = Path(__file__).resolve().parents[1]
SHA = re.compile(r"[0-9a-f]{40}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")
ARCHES = ("x86_64", "aarch64")
SIDES = {"C8": "client", "D4": "server"}
EXTERNAL = [
    "independent review of source/binary provenance and recorded host controls",
    "identical handler, codec-tier, validation, payload and resource-budget work",
    "all applicable conformance, interop, hostile-peer, fuzz and dependency gates",
    "one-hour RSS soak with zero growth; overload/recovery and holdout validation",
    "cross-peer combinations, real-network RTT and 1/2/4/8-core qualification",
    "other categories and competitors before any broader performance ranking",
]


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path):
    return json.loads(path.read_text(), object_pairs_hook=unique_object,
                      parse_constant=lambda value: (_ for _ in ()).throw(
                          ValueError(f"non-finite JSON constant: {value}")))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def number(value, name, minimum=0):
    require(type(value) in (int, float) and math.isfinite(value) and value >= minimum,
            f"invalid {name}")
    return value


def count(value, name):
    require(type(value) is int and value >= 0, f"invalid {name}")
    return value


def nonempty(value, name):
    require(isinstance(value, str) and value.strip() and value != "unknown", f"missing {name}")
    return value


def artifact(base, ref):
    require(isinstance(ref, dict) and set(ref) == {"path", "sha256"}, "invalid raw artifact reference")
    relative = ref["path"]
    require(isinstance(relative, str) and relative and not Path(relative).is_absolute(), "raw path must be relative")
    path = (base / relative).resolve()
    require(path.is_relative_to(base.resolve()), "raw artifact escapes campaign directory")
    require(isinstance(ref["sha256"], str) and DIGEST.fullmatch(ref["sha256"]), "invalid raw artifact SHA-256")
    require(hashlib.sha256(path.read_bytes()).hexdigest() == ref["sha256"], "raw artifact hash mismatch")
    return path, load_json(path)


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[math.ceil(len(ordered) * fraction) - 1]


def paired_interval(ratios):
    """Mean paired ratio and percentile bootstrap CI, fixed 10,000 resamples."""
    generator = random.Random(0x50524253)
    n = len(ratios)
    draws = sorted(statistics.fmean(ratios[generator.randrange(n)] for _ in range(n))
                   for _ in range(10_000))
    return {"estimate": statistics.fmean(ratios), "low": draws[249], "high": draws[9749],
            "confidence": 0.95, "method": "paired-mean-ratio-percentile-bootstrap",
            "resamples": 10_000, "pairs": n}


def expected_order(seed, cell, arch, peer, repeat):
    order = ["pbrs-grpc", peer]
    random.Random(f"{seed}:{cell}:{arch}:{peer}:{repeat}").shuffle(order)
    return order


def pin(value, name):
    require(isinstance(value, dict), f"missing {name} pin")
    for key in ("version", "allocator"):
        nonempty(value.get(key), f"{name}.{key}")
    require(isinstance(value.get("source"), str) and SHA.fullmatch(value["source"])
            and value["source"] != "0" * 40, f"invalid {name} source commit")
    builds = value.get("builds")
    require(isinstance(builds, dict) and set(builds) == set(ARCHES), f"{name} needs distinct architecture build pins")
    for arch, build in builds.items():
        require(isinstance(build, dict), f"missing {name}/{arch} build")
        nonempty(build.get("toolchain"), f"{name}/{arch} toolchain")
        digest = build.get("binary_sha256")
        require(isinstance(digest, str) and DIGEST.fullmatch(digest) and digest != "0" * 64,
                f"invalid {name}/{arch} binary SHA-256")
        profile = build.get("profile", {})
        kind = profile.get("kind") if isinstance(profile, dict) else None
        expected_kind = {"pbrs-grpc": "rust", "tonic": "rust", "grpc-go": "go", "grpc-cpp": "cpp"}[name]
        require(kind == expected_kind, f"{name}/{arch} compiler profile differs from peer implementation")
        if kind in ("rust", "cpp"):
            require(type(profile.get("opt_level")) is int and profile["opt_level"] == 3
                    and profile.get("lto") in ("thin", "fat"), f"{name}/{arch} needs release level 3 and LTO")
            if kind == "rust":
                require(type(profile.get("codegen_units")) is int and profile["codegen_units"] == 1,
                        f"{name}/{arch} needs one Rust codegen unit")
        elif kind == "go":
            require(profile.get("optimization") == "default" and profile.get("race") is False,
                    f"{name}/{arch} needs optimized Go without race instrumentation")
        else:
            raise ValueError(f"unsupported {name}/{arch} compiler profile")
    require(builds[ARCHES[0]]["binary_sha256"] != builds[ARCHES[1]]["binary_sha256"],
            f"{name} reuses one native executable digest across architectures")


def host(value):
    require(isinstance(value, dict), "invalid host snapshot")
    require(value.get("os") == "linux" and value.get("architecture") in ARCHES, "unsupported host platform")
    for key in ("cpu", "kernel", "hyperthread_topology", "interrupt_affinity", "sysctl", "ring_buffers"):
        nonempty(value.get(key), f"host.{key}")
    for key in ("dedicated", "isolated_cores", "turbo_disabled"):
        require(value.get(key) is True, f"host control not established: {key}")
    require(value.get("governor") == "performance", "host governor must be performance")
    number(value.get("nic_gbps"), "host NIC speed", 10)
    require(type(value.get("mtu")) is int and value["mtu"] > 0, "invalid host MTU")
    require(isinstance(value.get("cores"), list) and value["cores"] and
            all(type(core) is int and core >= 0 for core in value["cores"]) and
            len(set(value["cores"])) == len(value["cores"]), "invalid pinned cores")


def same_json(first, second):
    """JSON identity including primitive types (True must not equal 1)."""
    if type(first) is not type(second):
        return False
    if isinstance(first, dict):
        return first.keys() == second.keys() and all(same_json(first[key], second[key]) for key in first)
    if isinstance(first, list):
        return len(first) == len(second) and all(same_json(a, b) for a, b in zip(first, second))
    return first == second


def endpoint(row, peer, pins, hosts, cell, arch, side):
    require(isinstance(row, dict), "missing endpoint run")
    require(row.get("peer") == peer and same_json(row.get("pin"), {key: pins[peer][key] for key in ("source", "version", "allocator")}
            | {"build": pins[peer]["builds"][arch]}), "mixed peer source/binary/toolchain pin")
    require(same_json(row.get("settings"), cell["settings"]), "mixed or unrecorded effective endpoint settings")
    require(row.get("payload_sha256") == cell["payload_sha256"] and
            row.get("handler_contract_sha256") == cell["handler_contract_sha256"], "payload/handler work differs")
    require(row.get("allocation_counters") is False, "CPU measurement uses allocation instrumentation")
    pair = row.get("hosts")
    require(isinstance(pair, dict) and set(pair) == {"client", "server"} and pair["client"] != pair["server"],
            "client/server must use separate recorded hosts")
    require(all(name in hosts and hosts[name]["architecture"] == arch for name in pair.values()), "mixed host architecture")
    require(len(hosts[pair["client"]]["cores"]) == len(hosts[pair["server"]]["cores"]), "endpoint CPU budgets differ")
    number(row.get("warmup_seconds"), "warmup seconds", 15)
    require(count(row.get("warmup_rpcs"), "warmup RPCs") >= 10_000, "insufficient warmup RPCs")
    seconds = number(row.get("duration_seconds"), "measurement seconds", 60)
    names = ("offered", "dispatched", "completed", "successful", "failed", "rejected", "timed_out", "unfinished")
    counters = {name: count(row.get(name), name) for name in names}
    require(counters["offered"] == counters["dispatched"] + counters["rejected"], "offered-call conservation failed")
    require(counters["dispatched"] == counters["completed"] + counters["unfinished"], "completion conservation failed")
    require(counters["completed"] == counters["successful"] + counters["failed"], "success/error conservation failed")
    require(counters["completed"] >= 10_000, "p99 requires at least 10,000 samples per endpoint/run")
    require(all(counters[name] == 0 for name in ("failed", "rejected", "timed_out", "unfinished")),
            "errored/incomplete primary CPU cell; failures cannot be discarded")
    require(row.get("latency_origin") == "scheduled_dispatch" and row.get("arrival") in ("constant", "poisson"),
            "open-loop scheduled-send latencies required")
    expected = cell["settings"]["offered_qps"] * seconds
    require(math.isfinite(expected), "non-finite offered-call expectation")
    require(abs(counters["offered"] - expected) <= 6 * math.sqrt(expected) + .01 * expected,
            "offered calls differ from frozen open-loop rate")
    latencies, lags = row.get("latencies_ns"), row.get("scheduling_lag_ns")
    require(isinstance(latencies, list) and len(latencies) == counters["completed"], "latency sample count differs from completed calls")
    require(isinstance(lags, list) and len(lags) == counters["dispatched"], "scheduling-lag samples missing")
    for name, values in (("latency", latencies), ("scheduling lag", lags)):
        require(all(type(value) is int and value >= 0 for value in values), f"invalid exact-nanosecond {name} samples")
    require(percentile(latencies, .5) > 0, "zero p50 makes latency comparisons undefined")
    require(max(lags) <= .1 * percentile(latencies, .5), "load generator scheduling lag exceeds 10% of p50")
    cpu = row.get("cpu_seconds", {})
    for name in ("client", "server"):
        number(cpu.get(name), f"{name} CPU seconds", 1e-15)
        require(cpu[name] <= seconds * len(hosts[pair[name]]["cores"]), "CPU time exceeds pinned-core budget")
    rss_by_side = row.get("rss_bytes", {})
    for endpoint_side in ("client", "server"):
        rss = rss_by_side.get(endpoint_side, {})
        for name in ("steady", "peak"):
            require(count(rss.get(name), f"{endpoint_side} {name} RSS") > 0, "missing endpoint RSS")
        require(rss["peak"] >= rss["steady"], "peak RSS is below steady RSS")
    rss = rss_by_side[side]
    return {"cpu": cpu[side] / counters["successful"], "p99": percentile(latencies, .99),
            "steady_rss": rss["steady"], "peak_rss": rss["peak"]}


def verify(path, expected_source):
    report = {"schema": "pbrs.rpc-cpu-preflight.v1", "qualified": False,
              "scope": "named RPC CPU cells only", "evidence_checks_passed": False,
              "numerical_checks_passed": False, "errors": [], "comparisons": [],
              "external_requirements": EXTERNAL}
    try:
        data = load_json(path)
        report["input_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        require(isinstance(data, dict), "campaign must be an object")
        require(data.get("schema") == "pbrs.rpc-cpu-evidence.v1", "unsupported evidence schema; summary diagnostics are not paired raw evidence")
        require(data.get("scope") == "named-rpc-cpu-cells", "only explicit named-cell CPU claims supported")
        require(data.get("synthetic") in (True, False) and type(data["synthetic"]) is bool, "synthetic/real origin must be explicit")
        report["synthetic"] = data["synthetic"]
        require(data.get("source") == expected_source and SHA.fullmatch(expected_source)
                and expected_source != "0" * 40, "candidate source differs from expected full commit")
        report["source"] = expected_source
        category = data.get("category")
        require(category in SIDES, "only C8/client or D4/server CPU categories are supported")
        report["category"] = category
        register = load_json(ROOT / "bench/scoreboard/categories.json")
        peers = next(row["comparators"] for row in register["categories"] if row["id"] == category)
        report["required_peers"] = peers
        pins = data.get("pins")
        require(isinstance(pins, dict) and set(pins) == {"pbrs-grpc", *peers}, "missing required peer pins or undeclared peers")
        for name, value in pins.items():
            pin(value, name)
        require(pins["pbrs-grpc"]["source"] == expected_source, "candidate pin differs from expected source")
        hosts = data.get("hosts")
        require(isinstance(hosts, dict) and hosts, "missing raw host-control snapshots")
        snapshots = {}
        for name, ref in hosts.items():
            _, snapshots[name] = artifact(path.parent, ref)
            host(snapshots[name])
        seed = data.get("random_seed")
        require(type(seed) is int and 0 <= seed < 2 ** 64, "missing frozen randomization seed")
        cells = data.get("cells")
        require(isinstance(cells, list) and cells, "no frozen named cells")
        cell_map = {}
        for cell in cells:
            require(isinstance(cell, dict), "invalid frozen cell")
            name = nonempty(cell.get("id"), "cell id")
            require(name not in cell_map, "duplicate frozen cell")
            require(isinstance(cell.get("settings"), dict), "missing frozen cell settings")
            number(cell["settings"].get("offered_qps"), "frozen offered QPS", 1)
            for key in ("connections", "concurrency", "stream_window", "connection_window", "max_frame_size", "max_message_size"):
                require(count(cell["settings"].get(key), f"frozen setting {key}") > 0, f"nonpositive frozen {key}")
            require(16_384 <= cell["settings"]["max_frame_size"] <= 16_777_215, "invalid HTTP/2 frame setting")
            require(max(cell["settings"][key] for key in ("stream_window", "connection_window")) <= 2 ** 31 - 1, "invalid HTTP/2 window setting")
            for key in ("compression", "tls_cipher", "rpc_shape"):
                nonempty(cell["settings"].get(key), f"frozen setting {key}")
            require(cell["settings"].get("tcp_nodelay") is True, "TCP_NODELAY must be matched and enabled")
            for key in ("payload_sha256", "handler_contract_sha256"):
                require(isinstance(cell.get(key), str) and DIGEST.fullmatch(cell[key]), f"missing {key}")
            cell_map[name] = cell
        runs = data.get("runs")
        require(isinstance(runs, list) and runs, "no paired raw runs")
        groups = {}
        seen_files = set()
        for ref in runs:
            raw_path, row = artifact(path.parent, ref)
            require(raw_path not in seen_files, "duplicate raw artifact used as an independent repeat")
            seen_files.add(raw_path)
            require(isinstance(row, dict), "invalid raw pair")
            identity = (row.get("cell"), row.get("architecture"), row.get("reference"))
            name, arch, peer = identity
            require(name in cell_map and arch in ARCHES and peer in peers, "undeclared cell/architecture/reference")
            repeat = row.get("repeat")
            require(type(repeat) is int and repeat >= 1, "invalid paired repeat")
            group = groups.setdefault(identity, {})
            require(repeat not in group, "duplicate cell/architecture/peer/repeat")
            require(row.get("order") == expected_order(seed, name, arch, peer, repeat), "execution order differs from frozen seeded randomization")
            values = [endpoint(row.get(key), who, pins, snapshots, cell_map[name], arch, SIDES[category])
                      for key, who in (("candidate", "pbrs-grpc"), ("reference_run", peer))]
            require(row["candidate"]["hosts"] == row["reference_run"]["hosts"], "paired runs use different host budgets")
            group[repeat] = {metric: values[0][metric] / values[1][metric] for metric in values[0]}
            require(all(math.isfinite(ratio) and ratio > 0 for ratio in group[repeat].values()), "invalid endpoint metric ratio")
        expected = set(itertools.product(cell_map, ARCHES, peers))
        missing = sorted(expected - groups.keys())
        require(not missing, f"missing required cell/architecture/peer comparisons: {missing}")
        for identity, values in sorted(groups.items()):
            require(len(values) >= 5 and set(values) == set(range(1, len(values) + 1)), "at least five independent consecutive paired repeats required")
            name, arch, peer = identity
            orders = {tuple(expected_order(seed, name, arch, peer, repeat)) for repeat in values}
            require(len(orders) == 2, "randomized paired order must include both orientations")
            intervals = {metric: paired_interval([values[index][metric] for index in sorted(values)])
                         for metric in ("cpu", "p99", "steady_rss", "peak_rss")}
            bounds = {"cpu": .8, "p99": 1.05, "steady_rss": 1.1, "peak_rss": 1.1}
            losses = [metric for metric, limit in bounds.items() if intervals[metric]["low"] > limit]
            unresolved = [metric for metric, limit in bounds.items()
                          if intervals[metric]["low"] <= limit < intervals[metric]["high"]]
            report["comparisons"].append({"cell": name, "architecture": arch, "reference": peer,
                                           "candidate_to_reference": intervals,
                                           "verdict": "fails_numerical_bounds" if losses else "uncertain" if unresolved else "passes_numerical_bounds",
                                           "failed_bounds": losses, "uncertain_bounds": unresolved})
        report["numerical_checks_passed"] = all(row["verdict"] == "passes_numerical_bounds" for row in report["comparisons"])
        if data["synthetic"]:
            report["errors"].append("synthetic fixture validates checker behavior; it is not measurement evidence")
        report["evidence_checks_passed"] = report["numerical_checks_passed"] and not report["errors"]
    except (OSError, ValueError, KeyError, TypeError, StopIteration, OverflowError, ZeroDivisionError) as error:
        report["errors"].append(str(error))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("campaign", type=Path)
    parser.add_argument("--expected-source", required=True, help="full expected candidate commit SHA")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = verify(args.campaign.resolve(), args.expected_source)
    payload = json.dumps(report, indent=2, allow_nan=False) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload)
    print(payload, end="")
    return 0 if report["evidence_checks_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
