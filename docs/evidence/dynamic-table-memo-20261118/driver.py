#!/usr/bin/env python3
"""Pair valid dynamic parse/drop workloads and retain every raw sample."""
import argparse
import json
import os
from pathlib import Path
import resource
import statistics
import subprocess

CASES = (
    "pooled_empty", "pooled_leaf", "poolless_leaf",
    "recursive", "repeated_child", "wide_memo",
)


def process_ns():
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return round(1e9 * (usage.ru_utime + usage.ru_stime))


def run(binary, schema, case, n):
    start = process_ns()
    proc = subprocess.run(
        [str(binary), case, str(n), str(schema)],
        text=True, capture_output=True, check=True,
    )
    cpu = process_ns() - start
    raw = json.loads(proc.stdout)
    assert raw["case"] == case and raw["n"] == n and cpu > 0
    return dict(raw, process_cpu_ns=cpu)


def identity(row):
    return row["wire_len"], row["wire_hash"]


def ratio(block, metric, left, right):
    return sum(row[metric] for row in block if row["binary"] == left) / sum(
        row[metric] for row in block if row["binary"] == right
    )


def summary(values):
    return dict(median=statistics.median(values), min=min(values), max=max(values))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("schema", type=Path)
    parser.add_argument("binaries", type=Path)
    parser.add_argument("report", type=Path)
    config = parser.parse_args()
    affinity = sorted(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {affinity[0]})
    paths = {label: config.binaries / label for label in ("a", "b", "a-count", "b-count")}
    data = {}
    for case in CASES:
        measured = {}
        calibration = run(paths["a"], config.schema, case, 50000)
        n = min(2_000_000, max(5000, round(160_000_000 * 50000 / calibration["wall_ns"])))
        blocks = []
        for order in ("abba", "baab", "abba", "baab", "abba", "baab"):
            block = [
                dict(run(paths[label], config.schema, case, n), binary=label)
                for label in order
            ]
            assert all(identity(row) == identity(calibration) for row in block)
            assert all(row["alloc_events"] == row["requested_bytes"] == 0 for row in block)
            blocks.append(block)
        for label in ("a-count", "b-count"):
            pairs = []
            for _ in range(3):
                first = run(paths[label], config.schema, case, 64)
                second = run(paths[label], config.schema, case, 128)
                assert identity(first) == identity(second) == identity(calibration)
                pairs.append([first, second])
            deltas = [
                tuple((b[key] - a[key]) / 64 for key in ("alloc_events", "requested_bytes"))
                for a, b in pairs
            ]
            assert deltas[0] == deltas[1] == deltas[2], (case, label, deltas)
            measured[label] = dict(pairs=pairs, per_parse=deltas[0])
        cell = dict(calibration=calibration, n=n, blocks=blocks, allocations=measured)
        for metric in ("wall_ns", "process_cpu_ns"):
            cell[metric] = dict(
                b_over_a=summary([ratio(block, metric, "b", "a") for block in blocks]),
                a_over_a=summary([
                    [row[metric] for row in block if row["binary"] == "a"][1]
                    / [row[metric] for row in block if row["binary"] == "a"][0]
                    for block in blocks
                ]),
            )
        data[case] = cell
        print(case, json.dumps({
            "allocations": {k: v["per_parse"] for k, v in measured.items()},
            "wall_ns": cell["wall_ns"], "process_cpu_ns": cell["process_cpu_ns"],
        }), flush=True)
    assert data["pooled_empty"]["allocations"]["a-count"]["per_parse"][0] == 1
    assert data["pooled_empty"]["allocations"]["b-count"]["per_parse"][0] == 0
    report = dict(allowed_cpus=affinity, selected_cpu=affinity[0], cases=data)
    config.report.write_text(json.dumps(report, separators=(",", ":")) + "\n")
    print("DYNAMIC_MEMO_JSON=" + config.report.read_text().strip(), flush=True)


if __name__ == "__main__":
    main()
