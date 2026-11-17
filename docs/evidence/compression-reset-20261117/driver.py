import hashlib
import json
import os
from pathlib import Path
import resource
import statistics
import subprocess
import sys

base, candidate, destination = sys.argv[1:]
available = sorted(os.sched_getaffinity(0))
if not available:
    raise RuntimeError("no allowed CPU")
os.sched_setaffinity(0, {available[0]})
if sorted(os.sched_getaffinity(0)) != [available[0]]:
    raise RuntimeError("affinity not applied")
cases = [f"{coding}-{kind}" for coding in ("gzip", "deflate") for kind in (
    "empty", "1k-noise", "64k-noise", "1m-zero", "roundtrip-1k-noise", "encode-control-1k-noise")]
result = {"scope": "shared-host whole-process component CPU; not RPC or qualification",
          "base": "e0d6d09230c4e53934be6aa8aa928612bb40733d",
          "candidate": "2f27343e23770571c3ab40a15038c6dcb2ed5217", "affinity": [available[0]],
          "available_cpus": available, "blocks": 4, "runs": {}}

def run(binary, case, n):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    child = subprocess.run([binary, case, str(n)], capture_output=True, text=True, check=True)
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime
    if cpu <= 0:
        raise RuntimeError("invalid CPU sample")
    row = json.loads(child.stdout)
    if row["iterations"] != n:
        raise RuntimeError("wrong iteration count")
    return {"cpu": cpu, "output": row}

for case in cases:
    n = 32
    while True:
        calibration = run(base, case, n)
        if calibration["cpu"] >= 0.25:
            break
        n *= 2
        if n > 8_388_608:
            raise RuntimeError("calibration did not converge")
    blocks = []
    for block in range(4):
        order = ["base", "candidate", "candidate", "base"] if block % 2 == 0 else ["candidate", "base", "base", "candidate"]
        samples = {"base": [], "candidate": []}
        for name in order:
            sample = run(base if name == "base" else candidate, case, n)
            if sample["output"] != calibration["output"]:
                raise RuntimeError("workload/output mismatch")
            samples[name].append(sample)
        bs = [x["cpu"] for x in samples["base"]]
        cs = [x["cpu"] for x in samples["candidate"]]
        blocks.append({"order": order, "samples": samples, "ratio": sum(cs)/sum(bs),
                       "base_control": bs[1]/bs[0], "candidate_control": cs[1]/cs[0]})
    result["runs"][case] = {"iterations": n, "calibration": calibration, "blocks": blocks}
    print(case, "median", statistics.median(x["ratio"] for x in blocks),
          "range", [min(x["ratio"] for x in blocks), max(x["ratio"] for x in blocks)],
          "base_controls", [min(x["base_control"] for x in blocks), max(x["base_control"] for x in blocks)],
          "candidate_controls", [min(x["candidate_control"] for x in blocks), max(x["candidate_control"] for x in blocks)],
          flush=True)
Path(destination).write_text(json.dumps(result, indent=2) + "\n")
