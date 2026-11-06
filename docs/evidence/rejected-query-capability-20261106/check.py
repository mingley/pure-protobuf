#!/usr/bin/env python3
"""Verify retained observations and source reconstruction without running them."""
from pathlib import Path
import hashlib
import io
import json
import random
import statistics
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent

def sha(data):
    return hashlib.sha256(data).hexdigest()

manifest = json.loads((HERE / "manifest.json").read_text())
assert manifest["selected"] is False
raw = (HERE / "raw.tar.gz").read_bytes()
assert sha(raw) == manifest["archive_sha256"]
with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
    members = archive.getmembers()
    assert len(members) == len({member.name for member in members})
    assert all(member.isfile() for member in members)
    assert {member.name for member in members} == manifest["files"].keys()
    files = {member.name: archive.extractfile(member).read() for member in members}
for name, record in manifest["files"].items():
    assert len(files[name]) == record["bytes"] and sha(files[name]) == record["sha256"], name

def data(name):
    return json.loads(files[name])

def lines(name):
    return [json.loads(line) for line in files[name].splitlines()]

# Reconstruct every selected sparse-worktree input from the named Git base.
baseline = data("baseline-cpu-build.json")
paths = sorted(baseline["source"])
git_root = None
for directory in [HERE, Path.cwd()]:
    if not directory.is_dir():
        continue
    process = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=directory,
                             capture_output=True, text=True)
    if process.returncode == 0:
        git_root = process.stdout.strip()
        break
assert git_root is not None, "Run in a checkout containing the named Git base"
request = "".join(f"{manifest['base_commit']}:{path}\n" for path in paths).encode()
packed = subprocess.check_output(["git", "cat-file", "--batch"], input=request, cwd=git_root)
committed = {}
offset = 0
for path in paths:
    end = packed.index(b"\n", offset)
    header = packed[offset:end].split()
    assert len(header) == 3 and header[1] == b"blob", path
    length = int(header[2])
    offset = end + 1
    committed[path] = packed[offset:offset + length]
    offset += length + 1
assert offset == len(packed)

stages = ["", "prototype1-self-comparator/", "prototype2-byte-comparator/"]
for prefix in stages:
    for variant in ["baseline", "candidate"]:
        for mode in ["cpu", "alloc"]:
            name = f"{prefix}{variant}-{mode}-build.json"
            if name not in files:
                continue
            build = data(name)
            assert build["exit_code"] == 0 and build["guard"] is None
            assert build["target_bytes"] < 500 * 1024 ** 2 and build["free_bytes"] > 1024 ** 3
            assert build["variant"] == variant and build["mode"] == mode
            assert build["executable_sha256"] == build["retained_sha256"]
            assert build["source"].keys() == baseline["source"].keys()
            reconstructed = dict(committed)
            for relative in ["Cargo.toml", "Cargo.lock"]:
                reconstructed[relative] = files[f"{variant}/{relative}"]
            if variant == "candidate":
                for relative in ["src/map.rs", "src/lazy.rs"]:
                    reconstructed[relative] = files[f"{prefix}candidate/{relative}"]
            assert {path: sha(value) for path, value in reconstructed.items()} == build["source"], name
            assert build["harness_sha256"] == sha(files[f"{variant}-harness/src/main.rs"])
            assert build["lock_sha256"] == sha(files[f"{variant}-harness/Cargo.lock"])
            assert build["rustc"] == baseline["rustc"]
            messages = [json.loads(line) for line in files[name[:-5] + ".jsonl"].splitlines()
                        if line.startswith(b"{")]
            assert messages[-1] == {"reason": "build-finished", "success": True}
            targets = [m for m in messages if m.get("reason") == "compiler-artifact" and m.get("executable")]
            assert len(targets) == 1
            assert targets[0]["profile"]["opt_level"] == "3"
            assert targets[0]["profile"]["debuginfo"] == 0

assert files["baseline-harness/src/main.rs"] == files["candidate-harness/src/main.rs"]
assert files["baseline-harness/Cargo.lock"] == files["candidate-harness/Cargo.lock"]
assert files["baseline/Cargo.toml"] == files["candidate/Cargo.toml"]
assert files["baseline/Cargo.lock"] == files["candidate/Cargo.lock"]
candidate = data("candidate-cpu-build.json")
assert {path for path in paths if candidate["source"][path] != baseline["source"][path]} == {
    "src/map.rs", "src/lazy.rs"
}

unit = data("candidate-unit.json")
assert unit["exit_code"] == 0 and unit["guard"] is None
assert b"17 passed; 0 failed" in files["candidate-unit.log"]
assert b"Finished" in files["candidate-clippy.log"] and b"error:" not in files["candidate-clippy.log"]

plan = data("focused-plan.json")
order = data("focused-order.json")
cpu = lines("focused-raw.jsonl")
summary = data("focused-summary.json")
assert len(plan["cases"]) == 6 and plan["pairs_per_case"] == 9 and plan["iterations"] == 3000000
assert len(cpu) == summary["successful_processes"] == 108
assert len(summary["rows"]) == 6
assert len(order) == 54
rng = random.Random(plan["seed"])
expected_order = []
for pair in range(9):
    shuffled = list(range(6))
    rng.shuffle(shuffled)
    for case in shuffled:
        variants = ["baseline", "candidate"]
        rng.shuffle(variants)
        expected_order.append({"pair": pair, "case_no": case, "variants": variants})
assert order == expected_order
observed = {case: [] for case in range(6)}
for index, step in enumerate(order):
    pair = {}
    for j, variant in enumerate(step["variants"]):
        record = cpu[index * 2 + j]
        case = plan["cases"][step["case_no"]]
        assert record["variant"] == variant and record["case"] == case
        assert record["pair"] == step["pair"] and record["case_no"] == step["case_no"]
        assert record["exit_code"] == 0 and not record["stderr"]
        assert Path(record["command"][0]).name == variant + "-cpu-bin"
        assert record["command"][1:] == [*map(str, case), str(plan["iterations"])]
        result = json.loads(record["stdout"])
        assert result["schema"] == "pbrs.borrowed-map-lookup-screen.v1"
        assert [result[key] for key in ["kind", "size", "distribution", "key_length", "pattern", "result"]] == case
        assert result["semantic_preflight"] and not result["allocation_instrumentation"]
        assert result["allocation_calls"] is result["requested_bytes"] is None
        assert result["iterations"] == plan["iterations"]
        assert result["cpu_ns"] > 0 and result["wall_ns"] > 0
        pair[variant] = result
        assert plan["binary_sha256"][variant] == data(variant + "-cpu-build.json")["retained_sha256"]
    a, b = pair["baseline"], pair["candidate"]
    assert a["checksum"] == b["checksum"] and a["query_fingerprint"] == b["query_fingerprint"]
    observed[step["case_no"]].append({
        "pair": step["pair"], "baseline_cpu_ns": a["cpu_ns"],
        "candidate_cpu_ns": b["cpu_ns"], "ratio": b["cpu_ns"] / a["cpu_ns"],
        "checksum": a["checksum"], "query_fingerprint": a["query_fingerprint"],
    })
for case, row in enumerate(summary["rows"]):
    assert row["case_no"] == case and row["case"] == plan["cases"][case]
    assert row["pairs"] == observed[case]
    ratios = [pair["ratio"] for pair in observed[case]]
    assert row["median_ratio"] == statistics.median(ratios)
    assert row["min_ratio"] == min(ratios) and row["max_ratio"] == max(ratios)
    assert row["pairs_above_one"] == sum(value > 1 for value in ratios)

for prefix in ["", "prototype1-self-comparator/"]:
    captures = lines(prefix + "allocation-observations.jsonl")
    rows = data(prefix + "allocation-summary.json")["rows"]
    assert len(rows) == 76 and len(captures) == 152
    assert len({tuple(row["case"]) for row in rows}) == 76
    for index, row in enumerate(rows):
        pair = {}
        for record in captures[index * 2:index * 2 + 2]:
            assert record["exit_code"] == 0 and not record["stderr"]
            assert record["case"] == row["case"]
            assert Path(record["command"][0]).name == record["variant"] + "-alloc-bin"
            assert record["command"][1:] == [*map(str, row["case"]), "4096"]
            result = json.loads(record["stdout"])
            assert result["schema"] == "pbrs.borrowed-map-lookup-screen.v1"
            assert [result[key] for key in ["kind", "size", "distribution", "key_length", "pattern", "result"]] == row["case"]
            assert result["allocation_instrumentation"] and result["semantic_preflight"]
            assert result["iterations"] == 4096
            assert result["cpu_ns"] is result["wall_ns"] is None
            pair[record["variant"]] = result
        assert pair.keys() == {"baseline", "candidate"}
        for variant in pair:
            result = pair[variant]
            assert row[variant + "_calls"] == result["allocation_calls"]
            assert row[variant + "_requested"] == result["requested_bytes"]
            assert row["checksum"] == result["checksum"]
            assert row["query_fingerprint"] == result["query_fingerprint"]
        kind, size, _, length, _, _ = row["case"]
        expected_calls = 4096 if kind in ["proto", "lazy"] and size >= 16 and length == 96 else 0
        assert row["baseline_calls"] == expected_calls
        assert row["baseline_requested"] == expected_calls * 96
        assert row["candidate_calls"] == row["candidate_requested"] == 0

for kind in ["proto", "lazy", "int"]:
    current = files[f"candidate-{kind}-lookup.asm"]
    assert current == files[f"prototype1-self-comparator/candidate-{kind}-lookup.asm"]
    assert current == files[f"prototype2-byte-comparator/candidate-{kind}-lookup.asm"]
print(json.dumps({"passed": True, "archived_files": len(files), "selected_source_inputs": len(paths),
    "checkout_root": git_root, "base_commit": manifest["base_commit"],
    "cpu_processes": len(cpu), "final_allocation_processes": 152,
    "preliminary_allocation_processes": 152,
    "scope": "Retained arithmetic and selected-source reconstruction; no executable replay or host qualification."}))
