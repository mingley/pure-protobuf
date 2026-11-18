#!/usr/bin/env python3
import json
import statistics
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
cases = ("hooked", "observed")
rounds = (("before", "base"), ("after", "candidate"), ("repeat", "base"))
allocated_rpcs = 1024
allocation_raw = []


def execute(command, log):
    result = subprocess.run(command, capture_output=True, text=True, timeout=90)
    log.with_suffix(".stdout").write_text(result.stdout)
    log.with_suffix(".stderr").write_text(result.stderr)
    if result.returncode != 0:
        raise RuntimeError(f"{command!r}: {result.returncode}\n{result.stdout}\n{result.stderr}")
    return json.loads(result.stdout)


for case in cases:
    for order, source in rounds:
        binary = root / f"{source}-alloc"
        data = execute([str(binary), case, str(allocated_rpcs)], root / f"alloc-{case}-{order}")
        assert data["case"] == case and data["rpcs"] == allocated_rpcs
        allocation_raw.append({"order": order, "source": source, **data})
        print("alloc", order, data, flush=True)

(root / "allocations.json").write_text(json.dumps(allocation_raw, indent=2) + "\n")


failures = []
summary = {}
for case in cases:
    rows = {x["order"]: x for x in allocation_raw if x["case"] == case}
    allocs = {name: row["allocations"] / allocated_rpcs for name, row in rows.items()}
    sizes = {name: row["bytes"] / allocated_rpcs for name, row in rows.items()}
    alloc_delta = allocs["after"] - statistics.mean([allocs["before"], allocs["repeat"]])
    summary[case] = {
        "allocations_per_rpc": allocs,
        "requested_bytes_per_rpc": sizes,
        "allocations_delta": alloc_delta,
    }
    if not rows["before"]["allocations"] == rows["after"]["allocations"] == rows["repeat"]["allocations"]:
        failures.append(f"{case} allocation requests differed across runs")
    if not rows["before"]["bytes"] == rows["after"]["bytes"] == rows["repeat"]["bytes"]:
        failures.append(f"{case} allocation bytes differed across runs")

(root / "summary.json").write_text(json.dumps({"cases": summary, "failures": failures}, indent=2) + "\n")
print(json.dumps(summary, indent=2), flush=True)
if failures:
    raise SystemExit("\n".join(failures))
