import json, os, resource, statistics, subprocess, sys
from pathlib import Path
binaries = {"base": sys.argv[1], "candidate": sys.argv[2]}
count_binaries = {"base": sys.argv[3], "candidate": sys.argv[4]}
output = Path(sys.argv[5])
try:
    os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
    affinity = sorted(os.sched_getaffinity(0))
except (AttributeError, OSError):
    affinity = None
cases = [
    ("direct", "valid"),
    ("metadata", "valid"),
    ("direct", "missing"),
    ("metadata", "missing"),
    ("direct", "invalid"),
    ("metadata", "invalid"),
    ("direct", "future"),
    ("metadata", "state"),
]
results = []
def run(binary, carrier, scenario, n):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    completed = subprocess.run([binary, carrier, scenario, str(n)], text=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    report = json.loads(completed.stdout)
    assert report["iterations"] == n
    expected = n if scenario in ("valid", "future", "state") else 0
    assert report["checksum"] == expected, (carrier, scenario, report)
    return report, (after.ru_utime + after.ru_stime) - (before.ru_utime + before.ru_stime)
for carrier, scenario in cases:
    counts = {}
    for name, binary in count_binaries.items():
        report, _ = run(binary, carrier, scenario, 20000)
        counts[name] = report
    times = {"base": [], "candidate": []}
    for block in range(10):
        order = ("base", "candidate") if block % 2 == 0 else ("candidate", "base")
        for name in order:
            _, cpu = run(binaries[name], carrier, scenario, 1000000)
            times[name].append(cpu)
    base_median = statistics.median(times["base"])
    new_median = statistics.median(times["candidate"])
    results.append({"carrier": carrier, "scenario": scenario, "counts": counts,
                    "cpu_seconds": times, "candidate_over_base_median": new_median / base_median})
report = {"affinity": affinity, "blocks": 10, "iterations": 1000000,
          "allocation_iterations": 20000, "results": results}
output.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, sort_keys=True))
