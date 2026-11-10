#!/usr/bin/env python3
"""Snapshot and compare the generated adoption Rust sources and descriptors."""

import difflib
import hashlib
from pathlib import Path
import sys

def digest(data):
    return hashlib.sha256(data).hexdigest()

def collect(target):
    outputs = sorted(target.glob("release/build/pbrs-adoption-corpus-*/out"))
    if not outputs:
        raise ValueError(f"no generated adoption corpus under {target}")
    inventories = []
    for output in outputs:
        files = {}
        for path in sorted(output.rglob("*")):
            if path.is_file() and path.suffix in (".rs", ".fds"):
                name = path.relative_to(output).as_posix()
                data = path.read_bytes()
                # Cargo build output paths are embedded in this include list.
                if name == "option_modules.rs":
                    data = data.replace(str(output).encode(), b"<BUILD_OUTPUT>")
                files[name] = data
        if "pbrs/adoption.rs" not in files or "prost/adoption.rs" not in files:
            raise ValueError(f"missing generated corpus in {output}")
        inventories.append(files)
    first = inventories[0]
    if any(other != first for other in inventories[1:]):
        raise ValueError(f"feature variants generated different outputs in {target}")
    return first

def snapshot(target, output):
    files = collect(target)
    for name, data in files.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    print(f"saved {len(files)} generated Rust files and descriptors from {target}")

def load_snapshot(folder):
    files = {
        path.relative_to(folder).as_posix(): path.read_bytes()
        for path in folder.rglob("*")
        if path.is_file() and path.suffix in (".rs", ".fds")
    }
    if "pbrs/adoption.rs" not in files or "prost/adoption.rs" not in files:
        raise ValueError(f"incomplete generated snapshot: {folder}")
    return files

QUERY_OUTPUTS = ("pbrs/adoption.rs", "pbrs/sparse.rs")
START = b"// BEGIN QUERY COLD A/B\n"
END = b"// END QUERY COLD A/B\n"


def block(data, name):
    if data.count(START) != 1 or data.count(END) != 1:
        raise ValueError(f"missing or duplicate Query marker: {name}")
    before, tail = data.split(START)
    query, after = tail.split(END)
    return before, query, after


def boundaries(baseline, candidate):
    if set(baseline) != set(candidate):
        raise ValueError("generated file inventory changed")
    expected_descriptors = {"native-core.fds", "native-options.fds"}
    if not expected_descriptors.issubset(baseline):
        raise ValueError("required descriptors missing")
    changed = {name for name in baseline if baseline[name] != candidate[name]}
    if changed != set(QUERY_OUTPUTS):
        raise ValueError(f"unexpected changed output inventory: {sorted(changed)}")
    queries = {}
    for name in QUERY_OUTPUTS:
        before_a, query_a, after_a = block(baseline[name], name)
        before_b, query_b, after_b = block(candidate[name], name)
        if before_a != before_b or after_a != after_b:
            raise ValueError(f"generated code changed outside Query markers: {name}")
        if query_a == query_b or b"struct QueryCold {" not in query_a or b"struct QueryCold {" not in query_b:
            raise ValueError(f"expected Query cold-vector change missing: {name}")
        structs = [query.split(b"struct QueryCold {", 1)[1].split(b"}", 1)[0]
                   for query in (query_a, query_b)]
        for field in (b"children:", b"predicates:", b"subqueries:", b"traversals:"):
            lines = [[line for line in struct.splitlines() if line.strip().startswith(field)]
                     for struct in structs]
            if any(len(found) != 1 for found in lines) or b"Repeated<" not in lines[0][0] or b"Vec<" not in lines[1][0]:
                raise ValueError(f"unexpected QueryCold storage for {name} {field!r}: {lines!r}")
        queries[name] = (query_a, query_b)
    if queries[QUERY_OUTPUTS[0]] != queries[QUERY_OUTPUTS[1]]:
        raise ValueError("Query copies in package outputs differ")
    for name in baseline:
        if name not in QUERY_OUTPUTS:
            if baseline[name] != candidate[name]:
                raise ValueError(f"generated source or descriptor changed: {name}")
            if START in baseline[name] or END in baseline[name]:
                raise ValueError(f"unexpected Query marker: {name}")
    return queries[QUERY_OUTPUTS[0]]


def negative_controls(baseline, candidate):
    def rejected(label, mutation):
        damaged = dict(candidate)
        mutation(damaged)
        try:
            boundaries(baseline, damaged)
        except ValueError:
            return
        raise ValueError(f"negative control was accepted: {label}")

    for name in QUERY_OUTPUTS:
        rejected(f"outside-{name}", lambda d, name=name: d.__setitem__(name, b"// outside\n" + d[name]))
    rejected("unexpected-file", lambda d: d.__setitem__("unexpected.rs", b"unexpected\n"))
    rejected("changed-query-copy", lambda d: d.__setitem__(
        QUERY_OUTPUTS[1], d[QUERY_OUTPUTS[1]].replace(END, b"// altered Query\n" + END, 1)))
    rejected("changed-descriptor", lambda d: d.__setitem__(
        "native-core.fds", d["native-core.fds"] + b"\x00"))
    print("negative controls rejected changes outside each marker, an extra file, "
          "a mismatched Query copy and a changed descriptor")


def compare(snapshots, artifact):
    artifact.mkdir(parents=True, exist_ok=True)
    base_cpu, base_alloc, cand_cpu, cand_alloc = map(load_snapshot, snapshots)
    for label, cpu, alloc in (("baseline", base_cpu, base_alloc),
                              ("candidate", cand_cpu, cand_alloc)):
        if cpu != alloc:
            names = sorted(name for name in set(cpu) | set(alloc)
                           if cpu.get(name) != alloc.get(name))
            raise ValueError(f"{label} allocation and counters-off outputs differ: {names}")
    query_a, query_b = boundaries(base_cpu, cand_cpu)
    negative_controls(base_cpu, cand_cpu)
    for name in QUERY_OUTPUTS:
        baseline, candidate = base_cpu[name], cand_cpu[name]
        stem = Path(name).stem
        (artifact / f"baseline-{stem}.rs").write_bytes(baseline)
        (artifact / f"candidate-{stem}.rs").write_bytes(candidate)
        diff = difflib.unified_diff(
            baseline.decode().splitlines(keepends=True),
            candidate.decode().splitlines(keepends=True),
            fromfile=f"baseline/{name}", tofile=f"candidate/{name}",
        )
        (artifact / f"{stem}.diff").write_text("".join(diff))
    lines = [
        "only the explicitly listed package outputs change, within their Query markers",
        "both copies of Query are byte-identical within each arm",
        "all bytes outside Query and all other generated files, including descriptors, match",
        "allocation and counters-off builds produce the same generated outputs per arm",
        f"baseline Query bytes sha256 {digest(query_a)}",
        f"candidate Query bytes sha256 {digest(query_b)}",
        *[f"{digest(base_cpu[name])}  {name}" for name in sorted(base_cpu)],
    ]
    (artifact / "generated-verification.log").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))

if len(sys.argv) == 4 and sys.argv[1] == "snapshot":
    snapshot(Path(sys.argv[2]), Path(sys.argv[3]))
elif len(sys.argv) == 7 and sys.argv[1] == "compare":
    compare(list(map(Path, sys.argv[2:6])), Path(sys.argv[6]))
else:
    raise SystemExit(
        "usage: verify_generated.py snapshot TARGET OUT | compare BASE_CPU BASE_ALLOC CAND_CPU CAND_ALLOC OUT"
    )
