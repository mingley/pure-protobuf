#!/usr/bin/env python3
"""Verify the archived GN03 source qualification without building anything."""

import hashlib
import json
from pathlib import Path
import tarfile


ROOT = Path(__file__).resolve().parent


def read_json(name):
    return json.loads((ROOT / name).read_text())


def require(condition, message):
    if not condition:
        raise ValueError(message)


manifest = read_json("raw-member-sha256.json")
records = {
    label: read_json(filename)
    for label, filename in [
        ("before", "volume-before.json"),
        ("default-final", "volume-default-final.json"),
        ("shared-final", "volume-shared-final.json"),
        ("shared-repeat", "volume-shared-repeat.json"),
    ]
}
provenance = read_json("provenance.json")
for name, expected in read_json("artifact-sha256.json").items():
    require(Path(name).name == name, f"Unexpected artifact path: {name}")
    content = (ROOT / name).read_bytes()
    require(
        {"bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()} == expected,
        f"Published artifact mismatch: {name}",
    )
generated = {}
for label, record in records.items():
    for case, data in record["cases"].items():
        for name, metrics in data["files"].items():
            generated[f"work/source-volume/{case}/{label}/{name}"] = metrics

seen = set()
with tarfile.open(ROOT / "raw-artifacts.tar.gz", "r|gz") as archive:
    for member in archive:
        require(member.isfile(), f"Unexpected non-file member: {member.name}")
        require(member.name in manifest, f"Unregistered member: {member.name}")
        require(member.name not in seen, f"Duplicate member: {member.name}")
        seen.add(member.name)
        stream = archive.extractfile(member)
        require(stream is not None, f"Unreadable member: {member.name}")
        digest = hashlib.sha256()
        size = 0
        content = bytearray() if member.name in generated else None
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
            if content is not None:
                content.extend(chunk)
        actual = {"bytes": size, "sha256": digest.hexdigest()}
        require(actual == manifest[member.name], f"Hash/size mismatch: {member.name}")
        if content is not None:
            metrics = generated[member.name]
            require(actual == {k: metrics[k] for k in actual}, member.name)
            require(len(content.splitlines()) == metrics["lines"], member.name)
            require(
                content.count(b"pub const FILE_DESCRIPTOR_SET: &[u8] = &[")
                == metrics["raw_descriptor_literals"],
                member.name,
            )
require(seen == set(manifest), "Archive is missing registered members")
require(set(generated) <= seen, "Archive is missing generated outputs")

for label, record in records.items():
    require(
        record["harness_sha256"]
        == provenance["current_sources"]["bench/codegen/run.py"],
        f"Harness mismatch: {label}",
    )
    expected_plugin = (
        provenance["baseline_plugin_sha256"]
        if label == "before"
        else provenance["final_plugin_sha256"]
    )
    require(record["plugin_sha256"] == expected_plugin, f"Plugin mismatch: {label}")
require(
    manifest["work/baseline-protoc-gen-pbrs"]["sha256"]
    == provenance["baseline_plugin_sha256"],
    "Archived baseline plugin mismatch",
)
for phase, key in [("before", "baseline_sources"), ("final", "current_sources")]:
    for name in provenance["changed_production_sources"]:
        require(
            manifest[f"source/{phase}/{name}"]["sha256"] == provenance[key][name],
            f"Source capsule mismatch: {phase}/{name}",
        )

before = records["before"]["cases"]
default = records["default-final"]["cases"]
shared = records["shared-final"]["cases"]
repeat = records["shared-repeat"]["cases"]
require(set(before) == set(default) == set(shared) == set(repeat), "Case mismatch")
for case in before:
    require(before[case]["files"] == default[case]["files"], f"Default drift: {case}")
    require(shared[case]["files"] == repeat[case]["files"], f"Repeat drift: {case}")
    require(
        len({records[label]["cases"][case]["fds_sha256"] for label in records}) == 1,
        f"Descriptor input drift: {case}",
    )
    for label, cases in [("before", before), ("default", default), ("shared", shared), ("repeat", repeat)]:
        data = cases[case]
        for metric in ["bytes", "lines"]:
            require(
                sum(item[metric] for item in data["files"].values()) == data[f"total_{metric}"],
                f"Total mismatch: {label}/{case}/{metric}",
            )
    require(
        sum(item["raw_descriptor_literals"] for item in shared[case]["files"].values()) == 1,
        f"Shared metadata owner mismatch: {case}",
    )
    old, new = before[case]["total_bytes"], shared[case]["total_bytes"]
    print(f"{case}: default exact; shared repeat exact; {old} -> {new} source bytes ({100 * (old - new) / old:.2f}% reduction)")
print(f"Verified {len(seen)} archived files; no compilation or timing performed.")
