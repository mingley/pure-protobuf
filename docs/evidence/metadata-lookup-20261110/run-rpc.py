import json, os, resource, statistics, subprocess, sys

base, candidate, out = sys.argv[1:]
available = sorted(os.sched_getaffinity(0))
if not available:
    raise RuntimeError("empty CPU affinity")
os.sched_setaffinity(0, {available[0]})
actual = sorted(os.sched_getaffinity(0))
if actual != [available[0]]:
    raise RuntimeError(f"unable to pin benchmark runner: {actual}")

result = {
    "runs": {}, "warmup": 2000, "iterations": 20000, "blocks": 8,
    "available_cpu_count": len(available), "affinity": actual,
    "order": "ABBA on even blocks, BAAB on odd blocks",
}
for reads in (0, 1, 8):
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
            p = subprocess.run([program, str(reads), "20000"], text=True,
                               capture_output=True, check=True)
            after = resource.getrusage(resource.RUSAGE_CHILDREN)
            cpu = ((after.ru_utime + after.ru_stime)
                   - (before.ru_utime + before.ru_stime))
            wall_ns = int(p.stdout.strip())
            if cpu <= 0 or wall_ns <= 0:
                raise RuntimeError("non-positive measurement")
            samples[name].append({"cpu_s": cpu, "measured_wall_ns": wall_ns})
        a, b = samples["base"], samples["candidate"]
        blocks.append({
            "order": order, "samples": samples,
            "cpu_ratio": (b[0]["cpu_s"] + b[1]["cpu_s"]) / (a[0]["cpu_s"] + a[1]["cpu_s"]),
            "cpu_base_control": a[1]["cpu_s"] / a[0]["cpu_s"],
            "cpu_candidate_control": b[1]["cpu_s"] / b[0]["cpu_s"],
            "wall_ratio": ((b[0]["measured_wall_ns"] + b[1]["measured_wall_ns"]) /
                           (a[0]["measured_wall_ns"] + a[1]["measured_wall_ns"])),
            "wall_base_control": a[1]["measured_wall_ns"] / a[0]["measured_wall_ns"],
            "wall_candidate_control": b[1]["measured_wall_ns"] / b[0]["measured_wall_ns"],
        })
    result["runs"][str(reads)] = blocks
    print("reads", reads, "affinity", actual, flush=True)
    for key in ("cpu_ratio", "cpu_base_control", "cpu_candidate_control",
                "wall_ratio", "wall_base_control", "wall_candidate_control"):
        vals = [row[key] for row in blocks]
        print(key, "median", round(statistics.median(vals), 4),
              "range", [round(min(vals), 4), round(max(vals), 4)], flush=True)
with open(out, "w", encoding="utf8") as f:
    json.dump(result, f, indent=2)
