#!/usr/bin/env python3
"""Verify retained aligned generated files and their lexical source partition."""
import hashlib
import json
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
found = list(root.rglob("source-breakdown.json"))
assert len(found) == 1, found
root = found[0].parent
report = json.loads(found[0].read_text())
assert report == json.loads(Path(sys.argv[2]).read_text())
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def manifest(root):
    rows = [(str(path.relative_to(root)), path.stat().st_size, sha(path))
            for path in sorted(root.rglob("*")) if path.is_file()]
    return {"files": len(rows), "bytes": sum(r[1] for r in rows),
            "sha256": hashlib.sha256(json.dumps(rows).encode()).hexdigest()}
assert report["source"] == "5fe70efdc798031badefd1a3847d8866dde352f5"
assert report["reference_run"] == 42770907500
names = [f"part_{i:02d}.proto" for i in range(20)]
digest = hashlib.sha256(json.dumps([(name, sha(root / "corpus" / name)) for name in names]).encode()).hexdigest()
assert digest == report["corpus"]["sha256"] == "73ff55edceede7079014c22e1adee623e2fe9baead31b5c1e34e57281edc9580"
expected = {
    "pbrs-lean": (21, 12925901, "de51947430a8769f539989b1911cf0af55d32ecb2266be5b0638a0036491ae03"),
    "prost-aligned": (1, 412622, "046d211af74344974c17eb5c106fa0f700b30559b9a4ba3278a421f089cd00b8"),
}
for variant, (files, size, digest) in expected.items():
    actual = manifest(root / variant)
    assert actual == report["outputs"][variant]["manifest"] == {"files": files, "bytes": size, "sha256": digest}
    fds = sha(root / (variant + ".fds"))
    assert fds == report["outputs"][variant]["fds_sha256"] == "cb2d308fd5c8e7769964f8656da25f32d0635ff4281e9a0f2e558367596e1fa0"

method_re = re.compile(r"^    (?:#\[inline(?:\([^]]*\))?\]\s*)?(?:pub\s+)?(?:const\s+)?fn\s+(\w+)")
struct_re = re.compile(r"^pub struct Message\d{4}\s*\{")
bins, methods, structures, total = {}, {}, 0, 0
for path in sorted((root / "pbrs-lean").glob("*.rs")):
    active = "other"
    for line in path.read_text().splitlines(keepends=True):
        length = len(line.encode())
        total += length
        structures += bool(struct_re.match(line))
        if line and not line[0].isspace():
            active = "other"
        match = method_re.match(line)
        if match:
            name = match.group(1)
            methods[name] = methods.get(name, 0) + 1
            if name.startswith(("merge_", "parse_")):
                active = "merge_parse"
            elif name.startswith("validate_") or name == "check_required":
                active = "validation"
            elif name in ("compute_size", "write_to"):
                active = "size_encode"
            elif name.startswith(("set_", "clear_", "has_")) or name.startswith("field_"):
                active = "accessors"
            elif name == "new":
                active = "constructors"
            else:
                active = "other_methods"
        label = "doc_comments" if line.lstrip().startswith("///") else (
                "typed_message_macro_calls" if "impl_typed_message!(" in line else active)
        bins[label] = bins.get(label, 0) + length
assert total == 12925901 == sum(bins.values())
assert structures == report["message_structures"] == 1000
assert bins == report["line_partition_bytes"] and methods == report["method_counts"]
print("CODEGEN_SOURCE_VERIFIED files=22 corpus=20 messages=1000 total_bytes=12925901")
