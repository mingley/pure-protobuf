#!/usr/bin/env python3
"""Retain raw isolated decode measurements; never infer RPC performance."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import re
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--valgrind", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("repeats must be positive")
    args.out.mkdir(parents=True, exist_ok=False)
    binaries = {"baseline": args.baseline.resolve()}
    if args.candidate:
        binaries["candidate"] = args.candidate.resolve()
    hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest()
              for name, path in binaries.items()}
    rng = random.Random(11011)
    cells = []
    shapes = [("empty", 0), ("text", 1024), ("random", 1024), ("zeros", 1024),
              ("text", 65536), ("random", 65536), ("zeros", 1048576),
              ("random", 1048576)]
    for codec in ["gzip", "deflate"]:
        for shape, size in shapes:
            cell = f"decode.{codec}.{shape}.{size}"
            n = 8 if size >= 1048576 else 32 if size >= 65536 else 200
            samples = {name: [] for name in binaries}
            fingerprint = None
            for repeat in range(args.repeats):
                order = list(binaries)
                rng.shuffle(order)
                for name in order:
                    raw = []
                    instructions = []
                    for multiplier in [1, 2] if args.valgrind else [1]:
                        stem = args.out / f"{cell}.{repeat}.{name}.{multiplier}N"
                        cmd = [str(binaries[name]), codec, shape, str(size),
                               str(n * multiplier), "4"]
                        if args.valgrind:
                            cmd = [str(args.valgrind.resolve()), "--tool=callgrind",
                                   f"--callgrind-out-file={stem}.callgrind", *cmd]
                        result = subprocess.run(cmd, capture_output=True, text=True,
                                                timeout=120, check=False)
                        stem.with_suffix(stem.suffix + ".stdout").write_text(result.stdout)
                        stem.with_suffix(stem.suffix + ".stderr").write_text(result.stderr)
                        stem.with_suffix(stem.suffix + ".command.json").write_text(
                            json.dumps({"argv": cmd, "exit": result.returncode}) + "\n")
                        result.check_returncode()
                        row = json.loads(result.stdout)
                        if fingerprint is None:
                            fingerprint = row["input_fingerprint"]
                        if row["input_fingerprint"] != fingerprint:
                            raise ValueError(f"{cell}: input fingerprint differs")
                        if row["iters"] != n * multiplier:
                            raise ValueError(f"{cell}: iteration count differs")
                        raw.append(row)
                        if args.valgrind:
                            graph = Path(f"{stem}.callgrind").read_text()
                            match = re.search(r"^summary: (\d+)\s*$", graph, re.M)
                            if not match:
                                raise ValueError(f"{stem}: missing instruction summary")
                            instructions.append(int(match[1]))
                    first = raw[0]
                    sample = {"repeat": repeat, "order": order,
                              "allocs_per_op": first["allocs"] / n,
                              "alloc_bytes_per_op": first["alloc_bytes"] / n,
                              "wall_ns_per_op": first["wall_ns"] / n,
                              "instructions_per_op": None}
                    if len(raw) == 2:
                        for metric in ["allocs", "alloc_bytes"]:
                            if raw[1][metric] != 2 * first[metric]:
                                raise ValueError(f"{cell}: nondeterministic {metric}")
                        delta = instructions[1] - instructions[0]
                        if delta <= 0:
                            raise ValueError(f"{cell}: nonpositive instruction delta")
                        sample["instructions_per_op"] = delta / n
                    samples[name].append(sample)
            medians = {name: {metric: statistics.median(s[metric] for s in rows)
                              for metric in rows[0] if metric.endswith("_per_op")
                              and rows[0][metric] is not None}
                       for name, rows in samples.items()}
            row = {"cell": cell, "n": n, "input_fingerprint": fingerprint,
                   "samples": samples, "medians": medians}
            if args.candidate:
                row["candidate_change_percent"] = {
                    metric: 100 * (medians["candidate"][metric] / value - 1)
                    for metric, value in medians["baseline"].items() if value > 0}
            cells.append(row)
            print(f"{cell}: {row.get('candidate_change_percent', medians)}", flush=True)
    for name, path in binaries.items():
        if hashlib.sha256(path.read_bytes()).hexdigest() != hashes[name]:
            raise ValueError(f"{name}: binary changed during measurement")
    report = {"schema": "compression-devloop/1", "tier": "dev-loop",
              "host": {"os": platform.system(), "arch": platform.machine(),
                       "platform": platform.platform(), "cpu_count": os.cpu_count()},
              "binary_sha256": hashes, "seed": 11011, "repeats": args.repeats,
              "instruction_method": "callgrind (2N-N)/N" if args.valgrind else "not_run",
              "wall_time": "secondary; instrumented when using callgrind",
              "cells": cells}
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
