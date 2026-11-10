#!/usr/bin/env python3
"""Verify paired TLS portability RPC timing and allocation controls."""
import json
import math
from pathlib import Path
import statistics
import sys

root = Path(sys.argv[1])
paths = list(root.rglob("results.json"))
assert len(paths) == 1, paths
data = json.loads(paths[0].read_text())
assert data["schema"] == "tls-portability-rpc-screen/1"
assert data["baseline"] == "f73b85d067ea861fe36f23536153e4c5add3137f"
assert data["candidate"] == "171388257f15d261a1667799f40a38edc3bbeffe"
assert len(data["cpus"]) == 2 and len({tuple(data["topology"][str(c)]) for c in data["cpus"]}) == 2
def near(a,b):
    assert math.isclose(float(a),float(b),rel_tol=1e-12,abs_tol=1e-12),(a,b)
def sample(s,mode,width,count,witness=False,allocation=False):
    assert s["schema"] == "tls-rpc-control/1"
    assert (s["mode"],s["width"],s["requests"]) == (mode,width,count)
    assert s["warmup"] == 256 and s["cpu_ns"] > 0 and s["wall_ns"] > 0
    assert s["response_validated"] is True and s["tls_verified"] == (mode=="tls")
    assert s["scheme_witness"] is witness and s["allocation_instrumented"] is allocation
    if allocation:
        assert s["allocations"] > 0 and s["requested_bytes"] > 0
    else:
        assert s["allocations"] == s["requested_bytes"] == 0
assert len(data["witnesses"]) == 4
assert {(x["variant"],x["result"]["mode"]) for x in data["witnesses"]} == {
    (v,m) for v in ("base","candidate") for m in ("plain","tls")}
for x in data["witnesses"]:
    sample(x["result"],x["result"]["mode"],8,64,witness=True)
assert len(data["rows"]) == 32 and len(data["summaries"]) == 4
assert len(data["allocations"]) == 8
wanted={(m,w) for m in ("plain","tls") for w in (1,32)}
assert {(s["mode"],s["width"]) for s in data["summaries"]} == wanted
for summary in data["summaries"]:
    mode,width=summary["mode"],summary["width"]
    group=[r for r in data["rows"] if (r["mode"],r["width"]) == (mode,width)]
    assert len(group)==8 and sorted(r["block"] for r in group)==list(range(8))
    ratios=[]
    for row in group:
        n=row["count"]
        assert n==summary["requests"] and 2048<=n<=50000
        assert row["order"] == (["base","candidate"] if row["block"]%2==0 else ["candidate","base"])
        sample(row["pilot"],mode,width,512)
        for v in ("base","candidate"):
            sample(row["samples"][v],mode,width,n)
            ctl=row["controls"][v]
            sample(ctl["a"],mode,width,n)
            sample(ctl["b"],mode,width,n)
            near(ctl["ratio"],ctl["b"]["cpu_ns"]/ctl["a"]["cpu_ns"])
        near(row["ratio"],row["samples"]["candidate"]["cpu_ns"]/row["samples"]["base"]["cpu_ns"])
        ratios.append(row["ratio"])
    near(summary["median"],statistics.median(ratios))
    near(summary["range"][0],min(ratios)); near(summary["range"][1],max(ratios))
    assert summary["slower"]==sum(x>1 for x in ratios)
    for v in ("base","candidate"):
        ratios_ctl=[r["controls"][v]["ratio"] for r in group]
        near(summary["control_ranges"][v][0],min(ratios_ctl))
        near(summary["control_ranges"][v][1],max(ratios_ctl))
    allocations=[r for r in data["allocations"] if (r["mode"],r["width"])==(mode,width)]
    assert len(allocations)==2 and {r["variant"] for r in allocations}=={"base","candidate"}
    for x in allocations:
        sample(x["low"],mode,width,1024,allocation=True)
        sample(x["high"],mode,width,2048,allocation=True)
        near(x["delta_calls_per_rpc"],(x["high"]["allocations"]-x["low"]["allocations"])/1024)
        near(x["delta_requested_bytes_per_rpc"],(x["high"]["requested_bytes"]-x["low"]["requested_bytes"])/1024)
        assert x["delta_calls_per_rpc"]>0 and x["delta_requested_bytes_per_rpc"]>0
def one(name):
    found=list(root.rglob(name))
    assert len(found)==1,(name,found)
    return found[0]
assert one("tree-base.txt").read_bytes()==one("tree-candidate.txt").read_bytes()
lines=one("binary-sha256.txt").read_text().splitlines()
recorded={line.split()[1].rsplit("/",1)[-1]:line.split()[0] for line in lines}
for key,suffix in (("binaries",""),("allocation_binaries","-alloc"),("witness_binaries","-witness")):
    for v in ("base","candidate"):
        assert data[key][v]==recorded["tls-rpc-"+v+suffix]
(root/"verified-tls-rpc.json").write_text(json.dumps(data,indent=2,sort_keys=True)+"\n")
print("TLS_RPC_VERIFIED cases=4 pairs=32 allocation_rows=8 witnesses=4")
for s in data["summaries"]:
    alloc=[x for x in data["allocations"] if (x["mode"],x["width"])==(s["mode"],s["width"])]
    print("TLS_RPC_VERIFIED_RESULT",json.dumps({"summary":s,"allocations":alloc},sort_keys=True))
