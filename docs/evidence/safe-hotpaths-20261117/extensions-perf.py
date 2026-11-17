import json
import os
import resource
import re
import statistics
import subprocess
import sys

base, candidate, output = sys.argv[1:]
available = sorted(os.sched_getaffinity(0))
if not available:
    raise RuntimeError("no allowed CPU")
os.sched_setaffinity(0, {available[0]})
if sorted(os.sched_getaffinity(0)) != [available[0]]:
    raise RuntimeError("affinity mismatch")
result = {"base_source": "56bc4e53ae7369a18d1ca01fae81d350afb61cf4",
          "candidate_source": "13109f6ee3539b1def96e2464f79bd4163801694", "iterations": 5000000,
          "cpu_affinity": sorted(os.sched_getaffinity(0)),
          "scope": "release-mode ignored unit probe with identical test-only overlay; component, not RPC benchmark",
          "runs": {}}
for pair, programs in (("aa", {"base": base, "candidate": base}),
                       ("candidate", {"base": base, "candidate": candidate})):
    result["runs"][pair] = {}
    for case in ("mtls", "tls", "tcp", "control"):
        blocks = []
        for block in range(4):
            order = (["base", "candidate", "candidate", "base"] if block % 2 == 0
                     else ["candidate", "base", "base", "candidate"])
            samples = {"base": [], "candidate": []}
            for name in order:
                env = dict(os.environ, EXT_MODE=case, EXT_ITERATIONS="5000000")
                before = resource.getrusage(resource.RUSAGE_CHILDREN)
                proc = subprocess.run([programs[name], "--exact", "request::tests::extension_read_probe",
                                       "--ignored", "--nocapture"], env=env, check=True,
                                      capture_output=True, text=True)
                after = resource.getrusage(resource.RUSAGE_CHILDREN)
                matches = re.findall(r"EXT_PROBE (mtls|tls|tcp|control) (\d+) (\d+)", proc.stdout)
                if len(matches) != 1:
                    raise RuntimeError(proc.stdout + proc.stderr)
                got_case, iterations, wall_ns = matches[0]
                if got_case != case or int(iterations) != 5000000:
                    raise RuntimeError(matches)
                samples[name].append({"cpu": after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime,
                                      "probe_ns": int(wall_ns)})
            cpu = sum(x["cpu"] for x in samples["candidate"]) / sum(x["cpu"] for x in samples["base"])
            wall = sum(x["probe_ns"] for x in samples["candidate"]) / sum(x["probe_ns"] for x in samples["base"])
            blocks.append({"order": order, "samples": samples, "cpu_ratio": cpu, "probe_ratio": wall,
                           "base_control": samples["base"][1]["cpu"] / samples["base"][0]["cpu"],
                           "candidate_control": samples["candidate"][1]["cpu"] / samples["candidate"][0]["cpu"]})
        result["runs"][pair][case] = blocks
        print(pair, case, "cpu", statistics.median(b["cpu_ratio"] for b in blocks),
              "probe", statistics.median(b["probe_ratio"] for b in blocks),
              "base_control", [min(b["base_control"] for b in blocks), max(b["base_control"] for b in blocks)],
              "candidate_control", [min(b["candidate_control"] for b in blocks), max(b["candidate_control"] for b in blocks)], flush=True)
with open(output, "w", encoding="utf-8") as handle:
    json.dump(result, handle, indent=2)
