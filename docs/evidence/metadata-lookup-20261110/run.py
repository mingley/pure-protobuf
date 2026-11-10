import json, os, resource, statistics, subprocess, sys

base, candidate, out = sys.argv[1:]
cases = [
    "lower-hit", "upper-hit", "lower-miss", "upper-miss",
    "long-lower-hit", "long-upper-hit", "long-lower-miss", "long-upper-miss",
    "binary-hit", "binary-upper-hit", "binary-long-hit", "binary-long-upper-hit",
    "all-control", "reserved-control",
]
try:
    os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
except (AttributeError, OSError, ValueError):
    pass

result = {"runs": {}, "count_iterations": 10000, "timed_iterations": 2000000, "blocks": 10}
for case in cases:
    counts = {}
    for name, program in (("base", base), ("candidate", candidate)):
        text = subprocess.check_output([program, case, "10000", "count"], text=True)
        allocs, size = map(int, text.strip().split())
        counts[name] = {"allocations": allocs, "bytes": size}
    samples = {"base": [], "candidate": []}
    for block in range(10):
        order = [("base", base), ("candidate", candidate)]
        if block % 2:
            order.reverse()
        for name, program in order:
            before = resource.getrusage(resource.RUSAGE_CHILDREN)
            subprocess.run([program, case, "2000000"], stdout=subprocess.DEVNULL, check=True)
            after = resource.getrusage(resource.RUSAGE_CHILDREN)
            elapsed = ((after.ru_utime + after.ru_stime)
                       - (before.ru_utime + before.ru_stime))
            if elapsed <= 0:
                raise RuntimeError("process CPU duration was not positive")
            samples[name].append(elapsed)
    ratios = [a / b for a, b in zip(samples["candidate"], samples["base"])]
    result["runs"][case] = {"allocations": counts, "cpu_seconds": samples,
                            "ratios": ratios}
    print(case, "allocs", counts,
          "ratio", round(statistics.median(ratios), 4),
          "range", [round(min(ratios), 4), round(max(ratios), 4)],
          flush=True)
with open(out, "w", encoding="utf8") as f:
    json.dump(result, f, indent=2)
