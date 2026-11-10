#!/usr/bin/env python3
"""Separate call-count witnesses and invalidation mutant control."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

base, candidate, mutant, trace_base, trace_candidate, corpus, out = map(Path, sys.argv[1:])
cpu = min(os.sched_getaffinity(0))
os.sched_setaffinity(0, {cpu})
out.mkdir(parents=True, exist_ok=False)
items = {x["case"]: x for x in json.loads((corpus / "corpus.json").read_text())}

def generate(binary, case, suffix):
    folder = out / (case + "-" + suffix)
    folder.mkdir()
    result = subprocess.run([str(binary), items[case]["request"], "1", "out", str(folder)],
                            text=True, capture_output=True, timeout=180)
    assert result.returncode == 0, (case, suffix, result.stdout[-2000:], result.stderr[-4000:])
    assert "parallelism=1" in result.stdout
    values = re.findall(r"TARGET_MATCH_COUNT (\d+)", result.stderr)
    names, sources = sorted(folder.glob("*.name")), sorted(folder.glob("*.rs"))
    assert len(names) == len(sources) and names
    outputs = {}
    for i, (name, source) in enumerate(zip(names, sources)):
        assert name.stem == source.stem == f"{i:06}"
        name_text = name.read_text()
        data = source.read_bytes()
        outputs[name_text] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
    return {"folder": str(folder), "counts": list(map(int, values)), "outputs": outputs}

results = {}
for case in ("balanced20-lean", "interleaved-lean"):
    a = generate(base, case, "base")
    b = generate(candidate, case, "candidate")
    c = generate(trace_base, case, "trace-base")
    d = generate(trace_candidate, case, "trace-candidate")
    assert a["outputs"] == b["outputs"] == c["outputs"] == d["outputs"]
    assert not a["counts"] and not b["counts"]
    assert len(c["counts"]) == len(d["counts"]) == 1
    if case == "balanced20-lean":
        assert c["counts"] == [20000] and d["counts"] == [400], (c["counts"], d["counts"])
    else:
        assert c["counts"] == [20000] and d["counts"] == [20000], (c["counts"], d["counts"])
    results[case] = {"base_match_calls": c["counts"][0], "candidate_match_calls": d["counts"][0],
                     "outputs": a["outputs"]}
    print("TARGET_CACHE_WITNESS_V1 " + json.dumps({"case": case,
          "base": c["counts"][0], "candidate": d["counts"][0]}, sort_keys=True), flush=True)

bad = generate(mutant, "interleaved-lean", "mutant")
ref = results["interleaved-lean"]["outputs"]
changed = sorted(name for name in set(ref) | set(bad["outputs"])
                 if ref.get(name) != bad["outputs"].get(name))
assert changed, "invalidation-disabled mutant unexpectedly matched"
results["mutant_changed_names"] = changed
print("TARGET_CACHE_MUTANT_REJECTED changed_files=" + str(len(changed)), flush=True)
(out / "witness.json").write_text(json.dumps(results, indent=2, sort_keys=True) + "\n")
