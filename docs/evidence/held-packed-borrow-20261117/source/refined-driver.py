#!/usr/bin/env python3
import json, math, os, pathlib, statistics, subprocess, sys

a, b, c, ac, bc, cc, output = sys.argv[1:]
cases = {
    "fx32.3.cold":(30000,200), "fx32.3.warm":(1000000,200),
    "fx32.3.warmcopy":(1000000,200), "fx32.64.warm":(1000000,200),
    "fx32.1024.coldcopy":(10000,100), "fx32.1024.warm":(1000000,200),
    "fx32.1024.warmcopy":(100000,100), "fx32.16384.coldcopy":(1000,20),
    "fx64.64.warm":(1000000,200), "f64.64.warm":(1000000,200),
    "fx32.64.wire":(1000000,200), "fx32.1.empty":(1000000,200),
    "varint.3.cold":(30000,200), "varint.3.warm":(1000000,200),
    "custom.3.cold":(30000,200), "custom.3.warm":(1000000,200),
}
allowed = sorted(os.sched_getaffinity(0))
os.sched_setaffinity(0, {allowed[0]})
identities = {}

def run(binary, case, n):
    p = subprocess.run([binary, case, str(n)], check=True, text=True,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180)
    row = json.loads(p.stdout.strip().splitlines()[-1])
    assert row["case"] == case and row["n"] == n and row["cpu_ns"] > 0
    found = (row["wire_len"], row["checksum"] / n)
    if case not in identities: identities[case] = found
    assert found == identities[case], (case, found, identities[case])
    return row

def summarize(vals):
    return {"median":statistics.median(vals), "range":[min(vals),max(vals)],
            "slower":sum(x>1 for x in vals)}

out = {"affinity_cpu": allowed[0], "wire_identity": None, "cases": {}}
binaries = {"a":a, "b":b, "c":c}
count_binaries = {"a":ac, "b":bc, "c":cc}
for case, (initial, an) in cases.items():
    calibration = run(a, case, initial)
    # Calibrate only from baseline, before paired measurements. Every process
    # targets at least 150ms of thread CPU; record any that still falls short.
    n = min(1_000_000_000, max(initial, math.ceil(initial * 180_000_000 / calibration["cpu_ns"])))
    blocks = []
    for i in range(6):
        names = ["a","b","c"]
        names = names[i%3:] + names[:i%3]
        order = names + list(reversed(names))
        vals = [(name, run(binaries[name], case, n)) for name in order]
        totals = {key:sum(row["cpu_ns"] for name,row in vals if name==key) for key in binaries}
        controls = {key:[row["cpu_ns"] for name,row in vals if name==key] for key in binaries}
        blocks.append({"order":"".join(name for name,_ in vals),
                       "rows":[row for _,row in vals],
                       "ba":totals["b"]/totals["a"],"ca":totals["c"]/totals["a"],
                       "cb":totals["c"]/totals["b"],
                       "aa":controls["a"][1]/controls["a"][0],
                       "bb":controls["b"][1]/controls["b"][0],
                       "cc":controls["c"][1]/controls["c"][0]})
    allocations = []
    for i in range(3):
        names = ["a","b","c"]
        names = names[i%3:] + names[:i%3]
        specs = [(key+"_n",count_binaries[key],an) for key in names]
        specs += [(key+"_2n",count_binaries[key],2*an) for key in reversed(names)]
        vals = {label:run(binary,case,loops) for label,binary,loops in specs}
        diff = {key:{field:(vals[key+"_2n"][field]-vals[key+"_n"][field])/an
                     for field in ("allocs","reallocs","bytes")} for key in binaries}
        allocations.append({"rows":vals,"per_op":diff})
    cpus = [row["cpu_ns"] for block in blocks for row in block["rows"]]
    result = {"n":n,"allocation_n":an,"calibration":calibration,"blocks":blocks,
              "allocations":allocations,"ba":summarize([x["ba"] for x in blocks]),
              "ca":summarize([x["ca"] for x in blocks]),
              "cb":summarize([x["cb"] for x in blocks]),
              "controls":{x:summarize([k[x] for k in blocks]) for x in ("aa","bb","cc")},
              "min_process_cpu_ns":min(cpus)}
    out["cases"][case] = result
    print(case,"n",n,"b/a",result["ba"],"c/a",result["ca"],"c/b",result["cb"],
          "alloc",allocations[0]["per_op"],"min_cpu_ms",round(min(cpus)/1e6,3),flush=True)
out["wire_identity"] = identities
pathlib.Path(output).write_text(json.dumps(out,indent=2)+"\n")

print("PACKED_RAW_JSON=" + json.dumps(out, separators=(",", ":")), flush=True)
