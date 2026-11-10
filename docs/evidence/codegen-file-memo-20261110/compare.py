#!/usr/bin/env python3
"""Exact output and balanced CPU/allocation screens for file memo candidates."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
import tempfile

base, borrowed, contains, alloc_base, alloc_borrowed, alloc_contains, corpus, result = map(Path, sys.argv[1:])
normal = {"base": base, "borrowed": borrowed, "contains": contains}
counting = {"base": alloc_base, "borrowed": alloc_borrowed, "contains": alloc_contains}
variants = tuple(normal)
cases = json.loads((corpus / "corpus.json").read_text())
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
assert len(cores) >= workers, (workers, cores)
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
    assert values["parallelism"] == workers, (values, workers)
    if mode == "bench":
        assert values["iters"] == iters and values["cpu_ns"] > 0
    return values

def output_manifest(directory):
    names = sorted(directory.glob("*.name"))
    rs = sorted(directory.glob("*.rs"))
    assert len(names) == len(rs)
    entries = []
    for i, (name, src) in enumerate(zip(names, rs)):
        assert name.stem == src.stem == f"{i:06}"
        name_bytes, data = name.read_bytes(), src.read_bytes()
        entries.append({"name": name_bytes.decode("utf-8"), "bytes": len(data), "sha256": sha(data)})
    return entries

rows, validation, allocs, summaries = [], [], [], []
for case in cases:
    outcomes = {v: execute(normal[v], case, 1, "result") for v in variants}
    assert len(set(outcomes.values())) == 1, (case["case"], outcomes)
    ok = outcomes["base"] == "RESULT_OK_V1"
    if case["expected_error"]:
        assert not ok, (case["case"], outcomes)
    elif case["case"] != "duplicate-descriptor":
        assert ok, (case["case"], outcomes)
    if not ok:
        validation.append({"case": case, "result": outcomes["base"]})
        print("MEMO_ERROR_V1 " + json.dumps(validation[-1], sort_keys=True), flush=True)
        continue
    directories = {v: result / ("dump-" + v) for v in variants}
    for path in directories.values():
        path.mkdir()
    counters = {v: execute(normal[v], case, 1, "out", directories[v]) for v in variants}
    manifests = {v: output_manifest(directories[v]) for v in variants}
    assert len({json.dumps(m, sort_keys=True) for m in manifests.values()}) == 1
    assert all(c["files"] == len(manifests["base"]) for c in counters.values())
    for idx in range(len(manifests["base"])):
        prefix = f"{idx:06}"
        for ext in (".name", ".rs"):
            reference = (directories["base"] / (prefix + ext)).read_bytes()
            for variant in ("borrowed", "contains"):
                assert reference == (directories[variant] / (prefix + ext)).read_bytes(), (case["case"], prefix, ext, variant)
    if case["case"] == "services-lean":
        all_source = b"".join(p.read_bytes() for p in directories["base"].glob("*.rs"))
        assert b"EchoClient" in all_source, "service stub not emitted"
        assert b"::external::External" in all_source, "extern mapping not exercised"
    validation.append({"case": case, "result": "ok", "outputs": manifests["base"]})
    print("MEMO_OUTPUT_V1 " + json.dumps({"case": case["case"],
          "files": len(manifests["base"]), "bytes": sum(x["bytes"] for x in manifests["base"]),
          "manifest_sha256": sha(json.dumps(manifests["base"], sort_keys=True).encode())}), flush=True)
    for path in directories.values():
        shutil.rmtree(path)
    if not case["performance"]:
        continue
    pilot = execute(base, case, 1)
    iters = max(1, min(30, (120_000_000 + pilot["cpu_ns"] - 1) // pilot["cpu_ns"]))
    for v in variants:
        sample = execute(counting[v], case, 2)
        assert sample["alloc_calls"] > 0 and sample["requested_bytes"] > 0
        allocs.append({"case": case["case"], "variant": v, "sample": sample})
    group = []
    for block in range(6):
        order = list(variants[block % 3:] + variants[:block % 3])
        if block >= 3:
            order.reverse()
        samples = {v: execute(normal[v], case, iters) for v in order}
        controls = {}
        for v in order:
            a, b = execute(normal[v], case, iters), execute(normal[v], case, iters)
            controls[v] = {"a": a, "b": b, "ratio": b["cpu_ns"] / a["cpu_ns"]}
        ratios = {v: samples[v]["cpu_ns"] / samples["base"]["cpu_ns"] for v in ("borrowed", "contains")}
        row = {"case": case["case"], "block": block, "order": order, "iterations": iters,
               "samples": samples, "controls": controls, "ratios": ratios}
        rows.append(row)
        group.append(row)
        print("MEMO_PAIR_V1 " + json.dumps(row, sort_keys=True), flush=True)
    summary = {"case": case["case"]}
    for v in ("borrowed", "contains"):
        vals = [x["ratios"][v] for x in group]
        summary[v] = {"median": statistics.median(vals), "range": [min(vals), max(vals)],
                      "slower": sum(x > 1 for x in vals)}
    summary["controls"] = {v: [min(x["controls"][v]["ratio"] for x in group),
                               max(x["controls"][v]["ratio"] for x in group)] for v in variants}
    summaries.append(summary)
    print("MEMO_SUMMARY_V1 " + json.dumps(summary, sort_keys=True), flush=True)

data = {"scope": "shared runner; guest-distinct pinned cores; direct CodeGeneratorRequest; CPU excludes preparation and warmup; allocation samples separate; not a Prost comparison",
        "source": {"base": "5fe70efdc798031badefd1a3847d8866dde352f5",
                   "borrowed": "9717d306baf1e0391153d8157e5c31990f4a3c92",
                   "contains": "5bbdac8f28fea40bd6330067f3b205eaa63db728"},
        "parallelism": workers, "cpus": selected, "topology": {str(c): topology(c) for c in selected},
        "binary_sha256": {v: sha(p.read_bytes()) for v, p in normal.items()},
        "allocation_binary_sha256": {v: sha(p.read_bytes()) for v, p in counting.items()},
        "validation": validation, "rows": rows, "summaries": summaries, "allocations": allocs}
(result / "results.json").write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
print("MEMO_COMPLETE_V1 workers=" + str(workers), flush=True)
