#!/usr/bin/env python3
"""Compare exact output, paired direct-request CPU, and separate allocation counts."""
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shutil
import statistics
import subprocess
import sys

data, binaries, evidence = map(Path, sys.argv[1:4])
workers = int(sys.argv[4])
variants = ("base", "candidate")
native = {v: binaries / v for v in variants}
counting = {v: binaries / (v + "-alloc") for v in variants}
cases = json.loads((data / "cases.json").read_text())
random.Random(20261118).shuffle(cases)
cpus = sorted(os.sched_getaffinity(0))

def topology(cpu):
    root = Path("/sys/devices/system/cpu") / f"cpu{cpu}" / "topology"
    return tuple(int((root / attr).read_text()) for attr in ("physical_package_id", "core_id"))

separate = {}
for cpu in cpus:
    separate.setdefault(topology(cpu), cpu)
assert len(separate) >= workers, (cpus, separate, workers)
selected = list(separate.values())[:workers]
os.sched_setaffinity(0, selected)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def run(binary, request, iters, mode, dest=None):
    cmd = [str(binary), str(request), str(iters), mode]
    if dest is not None:
        cmd.append(str(dest))
    if mode == "bench":
        cmd = ["/usr/bin/time", "-f", "RSS_KIB=%M", *cmd]
    task = subprocess.run(cmd, text=True, capture_output=True, timeout=180, check=False)
    assert task.returncode == 0, (cmd, task.returncode, task.stdout, task.stderr)
    assert task.stdout.startswith(("PROBE_V1 " if mode == "bench" else "OUTPUT_V1 ")), task.stdout
    row = {key: int(value) for key, value in re.findall(r"(\w+)=(\d+)", task.stdout)}
    assert row["parallelism"] == workers, (selected, row)
    if mode == "bench":
        row["rss_kib"] = int(re.search(r"RSS_KIB=(\d+)", task.stderr).group(1))
        assert row["iters"] == iters and row["cpu_ns"] > 0, row
    return row

def dump(binary, request, dest):
    dest.mkdir(parents=True)
    stats = run(binary, request, 1, "out", dest)
    result = []
    for index in range(stats["files"]):
        key = f"{index:06}"
        path = (dest / (key + ".name")).read_text()
        payload = (dest / (key + ".rs")).read_bytes()
        assert not Path(path).is_absolute() and ".." not in Path(path).parts, path
        result.append((path, payload))
    assert len(result) >= 2 and len(result) * 2 == len(list(dest.iterdir()))
    assert len({entry[0] for entry in result}) == len(result)
    assert sum(len(entry[1]) for entry in result) == stats["bytes"]
    shutil.rmtree(dest)
    return result

def collect(paths, request, nominal):
    assert paths == nominal, str(request)
    return [{"name": path, "bytes": len(src), "sha256": sha(src)} for path, src in paths]

def save_consumer(case, sources):
    root = data / case["name"]
    consumer = data.parent / "consumers" / case["name"]
    generated = consumer / "generated"
    for path, source in sources:
        target = generated / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(source)
    assert (generated / "mod.rs").is_file()
    source_path = consumer / "src" / "main.rs"
    source_path.parent.mkdir()
    shutil.copyfile(root / "consumer-main.rs", source_path)
    shutil.copyfile(root / "consumer-Cargo.toml", consumer / "Cargo.toml")

def summary_case(name, rows, allocations, files):
    ratios = [row["cpu_ratio"] for row in rows]
    wall = [row["wall_ratio"] for row in rows]
    def values(v, key):
        return [r[key] for r in allocations[v]]
    return {
        "case": name, "workers": workers, "cpu_median": statistics.median(ratios),
        "cpu_range": [min(ratios), max(ratios)],
        "slower_blocks": sum(ratio > 1 for ratio in ratios),
        "wall_median": statistics.median(wall),
        "wall_range": [min(wall), max(wall)],
        "cpu_controls": {v: [min(row["controls"][v] for row in rows),
                             max(row["controls"][v] for row in rows)] for v in variants},
        "alloc_calls": {v: statistics.median(values(v, "alloc_calls")) for v in variants},
        "requested_bytes": {v: statistics.median(values(v, "requested_bytes")) for v in variants},
        "sample_rss_kib": {v: [min(s["rss_kib"] for row in rows for s in row["samples"][v]),
                             max(s["rss_kib"] for row in rows for s in row["samples"][v])]
                           for v in variants},
        "output_files": len(files), "output_bytes": sum(f["bytes"] for f in files),
    }

phase = evidence / f"workers-{workers}"
phase.mkdir(parents=True)
for case in cases:
    name = case["name"]
    root = data / name
    results = {"case": case, "workers": workers, "cpu_ids": selected,
               "topology": {cpu: topology(cpu) for cpu in selected}}
    nominal = dump(native["base"], root / "request", phase / "check")
    if len(nominal) < len(case["protos"]) + 1:
        raise AssertionError((name, len(nominal), len(case["protos"])))
    outputs = {}
    for request_name, digest in case["requests"].items():
        request = root / request_name
        assert sha(request.read_bytes()) == digest
        outputs[request_name] = {}
        for variant, binary in native.items():
            paths = dump(binary, request, phase / "check")
            outputs[request_name][variant] = collect(paths, request, nominal)
    results["outputs"] = outputs
    files = outputs["request"]["base"]
    if workers == 2:
        first = json.loads((evidence / "workers-1" / (name + ".json")).read_text())
        assert first["outputs"] == outputs, name
    if workers == 1 and case["consumer"]:
        save_consumer(case, nominal)
    if workers == 1 and name == "collision-default":
        for i, (path, source) in enumerate(nominal):
            (phase / f"collision-{i:02d}.name").write_text(path)
            (phase / f"collision-{i:02d}.rs").write_bytes(source)
    if case["timed"]:
        request = root / "request"
        pilot = run(native["base"], request, 1, "bench")
        count = max(1, min(10000, (170_000_000 + pilot["cpu_ns"] - 1) // pilot["cpu_ns"]))
        rows = []
        first_order = random.Random(name + str(workers)).choice(variants)
        for block in range(8):
            first = variants[(variants.index(first_order) + block) % 2]
            second = variants[1 - variants.index(first)]
            order = [first, second, second, first]
            samples = {v: [] for v in variants}
            for variant in order:
                samples[variant].append(run(native[variant], request, count, "bench"))
            def ratio(key):
                return sum(s[key] for s in samples["candidate"]) / sum(s[key] for s in samples["base"])
            rows.append({
                "block": block, "order": order, "samples": samples,
                "cpu_ratio": ratio("cpu_ns"), "wall_ratio": ratio("wall_ns"),
                "controls": {v: samples[v][1]["cpu_ns"] / samples[v][0]["cpu_ns"] for v in variants},
            })
        allocations = {v: [run(counting[v], request, 1, "bench") for _ in range(3)]
                       for v in variants}
        summary = summary_case(name, rows, allocations, files)
        results.update({"pilot": pilot, "timing": rows, "allocation": allocations, "summary": summary})
        print("FIELD_NAME_RESULT " + json.dumps(summary, sort_keys=True), flush=True)
    else:
        print(f"FIELD_NAME_IDENTITY {name} workers={workers} files={len(files)}", flush=True)
    (phase / (name + ".json")).write_text(json.dumps(results, indent=2, sort_keys=True) + "\n")
