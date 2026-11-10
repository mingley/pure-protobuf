import json, os, resource, statistics, subprocess, sys

base, candidate, out = sys.argv[1:]
cases = [
    "lower-hit", "upper-hit", "lower-miss", "upper-miss",
    "long-lower-hit", "long-upper-hit", "long-lower-miss", "long-upper-miss",
    "binary-hit", "binary-upper-hit", "binary-long-hit", "binary-long-upper-hit",
    "all-control", "reserved-control",
]
available = sorted(os.sched_getaffinity(0))
if not available:
    raise RuntimeError("empty CPU affinity")
os.sched_setaffinity(0, {available[0]})
actual = sorted(os.sched_getaffinity(0))
if actual != [available[0]]:
    raise RuntimeError(f"unable to pin benchmark runner: {actual}")

result = {
    "runs": {}, "timed_iterations": 2000000, "blocks": 8,
    "available_cpu_count": len(available), "affinity": actual,
    "order": "ABBA on even blocks, BAAB on odd blocks",
}
for case in cases:
    blocks = []
    for block in range(8):
        order = (
            ["base", "candidate", "candidate", "base"]
            if block % 2 == 0
            else ["candidate", "base", "base", "candidate"]
        )
        samples = {"base": [], "candidate": []}
        for name in order:
            program = base if name == "base" else candidate
            before = resource.getrusage(resource.RUSAGE_CHILDREN)
            p = subprocess.run([program, case, "2000000"], text=True,
                               capture_output=True, check=True)
            after = resource.getrusage(resource.RUSAGE_CHILDREN)
            cpu = ((after.ru_utime + after.ru_stime)
                   - (before.ru_utime + before.ru_stime))
            if cpu <= 0 or p.stdout.strip() != "done":
                raise RuntimeError("bad timing or benchmark response")
            samples[name].append(cpu)
        a, b = samples["base"], samples["candidate"]
        blocks.append({
            "order": order, "samples": samples,
            "ratio": sum(b) / sum(a), "base_control": a[1] / a[0],
            "candidate_control": b[1] / b[0],
        })
    result["runs"][case] = blocks
    print("case", case, "affinity", actual, flush=True)
    for key in ("ratio", "base_control", "candidate_control"):
        vals = [row[key] for row in blocks]
        print(key, "median", round(statistics.median(vals), 4),
              "range", [round(min(vals), 4), round(max(vals), 4)], flush=True)
with open(out, "w", encoding="utf8") as f:
    json.dump(result, f, indent=2)
