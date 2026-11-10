#!/usr/bin/env python3
"""Longer balanced file-memo CPU screen with per-variant output manifests."""
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

base, candidate, alloc_base, alloc_candidate, corpus, result = map(Path, sys.argv[1:])
normal = {"base": base, "candidate": candidate}
counting = {"base": alloc_base, "candidate": alloc_candidate}
variants = ("base", "candidate")
cases = json.loads((corpus / "corpus.json").read_text())
perf = {"balanced20-default", "balanced20-lean", "many128-lean",
        "many512-lean", "alias128-lean", "services-lean", "interleaved-lean"}
assert {c["case"] for c in cases if c["performance"]} == perf
random.Random(20261110).shuffle(cases)
available = sorted(os.sched_getaffinity(0))
workers = int(os.environ["PBRS_CODEGEN_CORES"])
assert workers in (1, 2)
def topology(cpu):
    root = Path("/sys/devices/system/cpu") / ("cpu" + str(cpu)) / "topology"
    return (int((root / "physical_package_id").read_text()),
            int((root / "core_id").read_text()))
cores = {}
for cpu in available:
    cores.setdefault(topology(cpu), []).append(cpu)
assert len(cores) >= workers
selected = [group[0] for group in list(cores.values())[:workers]]
os.sched_setaffinity(0, set(selected))
result.mkdir(parents=True, exist_ok=False)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def execute(binary, case, iters, mode="bench", directory=None):
    args = [str(binary), case["request"], str(iters), mode]
    if directory is not None:
        args.append(str(directory))
    proc = subprocess.run(args, capture_output=True, text=True, timeout=300)
    assert proc.returncode == 0, (args, proc.returncode, proc.stdout[-2000:], proc.stderr[-4000:])
    line = proc.stdout.strip()
    if mode == "result":
        assert line.startswith(("RESULT_OK_V1", "RESULT_ERR_V1 ")), line
        return line
    prefix = "OUTPUT_V1 " if mode == "out" else "PROBE_V1 "
    assert line.startswith(prefix), line
    values = {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", line)}
    assert values["parallelism"] == workers
    if mode == "bench":
        assert values["iters"] == iters and values["cpu_ns"] > 0
    return values

def output_manifest(directory):
    names, sources = sorted(directory.glob("*.name")), sorted(directory.glob("*.rs"))
    assert len(names) == len(sources)
    entries = []
    for i, (name, source) in enumerate(zip(names, sources)):
        assert name.stem == source.stem == f"{i:06}"
        data = source.read_bytes()
        entries.append({"name": name.read_bytes().decode("utf-8"),
                        "bytes": len(data), "sha256": sha(data)})
    return entries

rows, validation, allocations, summaries = [], [], [], []
for case in cases:
    outcomes = {v: execute(normal[v], case, 1, "result") for v in variants}
    assert len(set(outcomes.values())) == 1, (case["case"], outcomes)
    ok = outcomes["base"] == "RESULT_OK_V1"
    if case["expected_error"]:
        assert not ok, (case["case"], outcomes)
    elif case["case"] != "duplicate-descriptor":
        assert ok, (case["case"], outcomes)
    if not ok:
        validation.append({"case": case, "result": outcomes})
        continue
    paths = {v: result / ("dump-" + case["case"] + "-" + v) for v in variants}
    for path in paths.values():
        path.mkdir()
    counters = {v: execute(normal[v], case, 1, "out", paths[v]) for v in variants}
    manifests = {v: output_manifest(paths[v]) for v in variants}
    assert manifests["base"] == manifests["candidate"]
    for v in variants:
        assert counters[v]["files"] == len(manifests[v])
    for i in range(len(manifests["base"])):
        for ext in (".name", ".rs"):
            file = f"{i:06}" + ext
            assert (paths["base"] / file).read_bytes() == (paths["candidate"] / file).read_bytes()
    if case["case"] == "services-lean":
        content = b"".join(p.read_bytes() for p in paths["base"].glob("*.rs"))
        assert b"EchoClient" in content and b"::external::External" in content
    validation.append({"case": case, "result": outcomes, "variant_outputs": manifests})
    print("MEMO_REPEAT_OUTPUT_V1 " + json.dumps({"case": case["case"],
          "files": len(manifests["base"]), "bytes": sum(x["bytes"] for x in manifests["base"]),
          "manifest_sha256": sha(json.dumps(manifests["base"], sort_keys=True).encode())}), flush=True)
    retain = workers == 1 and case["case"] in {"balanced20-lean", "interleaved-lean",
                                                   "services-lean", "public-transitive",
                                                   "public-emit-deps", "public-owned", "wkt-shared"}
    if not retain:
        for path in paths.values():
            shutil.rmtree(path)
    if case["case"] not in perf:
        continue
    pilot = execute(base, case, 1)
    iterations = max(1, min(20000, (160_000_000 + pilot["cpu_ns"] - 1) // pilot["cpu_ns"]))
    for repeat in range(3):
        for variant in variants:
            a = execute(counting[variant], case, 2)
            assert a["alloc_calls"] > 0 and a["requested_bytes"] > 0
            allocations.append({"case": case["case"], "variant": variant, "repeat": repeat, "sample": a})
    group = []
    for block in range(6):
        order = variants if block % 2 == 0 else tuple(reversed(variants))
        samples = {v: execute(normal[v], case, iterations) for v in order}
        controls = {}
        for v in order:
            a = execute(normal[v], case, iterations)
            b = execute(normal[v], case, iterations)
            controls[v] = {"a": a, "b": b, "ratio": b["cpu_ns"] / a["cpu_ns"]}
        row = {"case": case["case"], "block": block, "order": order, "iterations": iterations,
               "samples": samples, "controls": controls,
               "ratio": samples["candidate"]["cpu_ns"] / samples["base"]["cpu_ns"]}
        rows.append(row)
        group.append(row)
        print("MEMO_REPEAT_PAIR_V1 " + json.dumps(row, sort_keys=True), flush=True)
    ratios = [r["ratio"] for r in group]
    summary = {"case": case["case"], "iterations": iterations,
               "median": statistics.median(ratios), "range": [min(ratios), max(ratios)],
               "slower": sum(v > 1 for v in ratios),
               "controls": {v: [min(r["controls"][v]["ratio"] for r in group),
                                  max(r["controls"][v]["ratio"] for r in group)] for v in variants},
               "sample_cpu_ns": {v: [min(r["samples"][v]["cpu_ns"] for r in group),
                                       max(r["samples"][v]["cpu_ns"] for r in group)] for v in variants}}
    summaries.append(summary)
    print("MEMO_REPEAT_SUMMARY_V1 " + json.dumps(summary, sort_keys=True), flush=True)
data = {"scope": "shared runner; guest-distinct pinned cores; direct CodeGeneratorRequest; process CPU excludes preparation and warmup; allocation samples separate; not a Prost comparison; selected one-worker output bytes retained",
        "source": {"base": "5fe70efdc798031badefd1a3847d8866dde352f5",
                   "candidate": "540e12e1a71d6ebdad86bb55b36ab89711451455"},
        "parallelism": workers, "cpus": selected, "topology": {str(c): topology(c) for c in selected},
        "binary_sha256": {v: sha(p.read_bytes()) for v, p in normal.items()},
        "allocation_binary_sha256": {v: sha(p.read_bytes()) for v, p in counting.items()},
        "validation": validation, "rows": rows, "summaries": summaries, "allocations": allocations}
(result / "results.json").write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
print("MEMO_REPEAT_COMPLETE_V1 workers=" + str(workers), flush=True)
