#!/usr/bin/env python3
"""Validate retained RX-09 run-cell pairs and summarize dev-loop counters."""

import hashlib
import json
from pathlib import Path
import re
import statistics
import sys

CELLS = (
    "rpc.pbrs.unary",
    "rpc.tonic.unary",
    "rpc.pbrs.server_stream",
    "rpc.tonic.server_stream",
)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_run(directory, cell, repeat, iters):
    stem = f"{cell}.repeat{repeat}.n{iters}"
    stderr_path = directory / "raw" / f"{stem}.stderr"
    stdout_path = directory / "raw" / f"{stem}.stdout"
    profile_path = directory / "callgrind" / f"{stem}.out"
    if stdout_path.read_bytes():
        raise ValueError(f"{stem}: unexpected child stdout")
    stderr = stderr_path.read_text()
    children = [line.removeprefix("__CHILD__ ") for line in stderr.splitlines()
                if line.startswith("__CHILD__ ")]
    if len(children) != 1:
        raise ValueError(f"{stem}: expected exactly one child result")
    child = json.loads(children[0])
    if child["cell"] != cell or child["iters"] != iters:
        raise ValueError(f"{stem}: mismatched child identity or count")
    if any(type(child[name]) is not int or child[name] < 0
           for name in ("allocs", "alloc_bytes")):
        raise ValueError(f"{stem}: invalid allocation counters")
    match = re.search(r"I\s+refs:\s*([\d,]+)", stderr)
    if match is None:
        raise ValueError(f"{stem}: missing callgrind instruction total")
    instructions = int(match.group(1).replace(",", ""))
    summary = re.search(r"^summary:\s*(\d+)\s*$", profile_path.read_text(), re.MULTILINE)
    if summary is None or int(summary.group(1)) != instructions:
        raise ValueError(f"{stem}: profile/summary instruction mismatch")
    return {
        "child": child,
        "whole_process_instructions": instructions,
        "stderr": str(stderr_path.relative_to(directory)),
        "stderr_sha256": sha256(stderr_path),
        "stdout": str(stdout_path.relative_to(directory)),
        "stdout_sha256": sha256(stdout_path),
        "callgrind": str(profile_path.relative_to(directory)),
        "callgrind_sha256": sha256(profile_path),
    }


def summarize(directory):
    expected = {f"{cell}.repeat{repeat}.n{iters}"
                for cell in CELLS for repeat in range(1, 4) for iters in (200, 400)}
    for folder, extension in (("raw", "stderr"), ("raw", "stdout"), ("callgrind", "out")):
        actual = {path.name.removesuffix(f".{extension}")
                  for path in (directory / folder).glob(f"*.{extension}")}
        if actual != expected:
            raise ValueError(f"{directory}/{folder}: incomplete or unexpected {extension} files")
    provenance_path = directory / "provenance.txt"
    provenance = provenance_path.read_text()
    source_sha = re.search(r"^source_sha=([0-9a-f]{40})$", provenance, re.MULTILINE).group(1)
    if "finished_utc=" not in provenance:
        raise ValueError(f"{directory}: capture did not finish")
    result = {"source_sha": source_sha, "provenance": provenance,
              "provenance_sha256": sha256(provenance_path), "cells": {}}
    for cell in CELLS:
        repeats = []
        for repeat in range(1, 4):
            first = read_run(directory, cell, repeat, 200)
            second = read_run(directory, cell, repeat, 400)
            difference = second["whole_process_instructions"] - first["whole_process_instructions"]
            if difference <= 0:
                raise ValueError(f"{cell} repeat {repeat}: nonpositive differential")
            repeats.append({
                "repeat": repeat, "n": first, "two_n": second,
                "instructions_per_rpc": difference / 200,
                "allocations_per_rpc": first["child"]["allocs"] / 200,
                "allocated_bytes_per_rpc": first["child"]["alloc_bytes"] / 200,
            })
        metrics = {}
        for name in ("instructions_per_rpc", "allocations_per_rpc", "allocated_bytes_per_rpc"):
            samples = [entry[name] for entry in repeats]
            metrics[name] = {"median": statistics.median(samples), "min": min(samples),
                             "max": max(samples), "samples": samples}
        result["cells"][cell] = {"metrics": metrics, "repeats": repeats}
    return result


def compare(a, b):
    return {key: (b[key]["median"] / a[key]["median"] - 1) * 100 for key in a}


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: summarize.py BASELINE_DIRECTORY CURRENT_DIRECTORY")
    baseline, current = (summarize(Path(arg)) for arg in sys.argv[1:])
    output = {
        "schema": "rx09-callgrind/1", "measurement_class": "dev_loop_diagnostic",
        "instruction_method": "differential_callgrind_2n_minus_n",
        "iterations": 200, "differential_iterations": 400, "prepare_iterations": 400,
        "repeats": 3, "wall_time_claim": False, "production_attribution_claim": False,
        "equal_work_cross_stack_comparison": False,
        "syscalls": {"status": "not_run", "reason": "strace absent from image"},
        "endpoint_cpu_and_retained_bytes": {
            "status": "not_run", "reason": "shared-runtime dev-loop counters only"},
        "baseline": baseline, "current": current,
        "current_change_from_baseline_percent": {
            cell: compare(baseline["cells"][cell]["metrics"], current["cells"][cell]["metrics"])
            for cell in CELLS
        },
        "current_pbrs_relative_to_tonic_percent": {
            shape: compare(current["cells"][f"rpc.tonic.{shape}"]["metrics"],
                           current["cells"][f"rpc.pbrs.{shape}"]["metrics"])
            for shape in ("unary", "server_stream")
        },
    }
    print(json.dumps(output, indent=2))
