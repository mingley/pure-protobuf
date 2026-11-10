#!/usr/bin/env python3
"""Create fixed requests for file-memo correctness and scalability."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

repo = Path.cwd()
out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=False)
spec = importlib.util.spec_from_file_location("codegen_bench", repo / "bench/codegen/run.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

def varint(value):
    data = bytearray()
    while value >= 128:
        data.append((value & 127) | 128)
        value >>= 7
    data.append(value)
    return bytes(data)

def field(number, value):
    return varint((number << 3) | 2) + varint(len(value)) + value

def read_varint(data, pos):
    value = 0
    for shift in range(0, 70, 7):
        b = data[pos]
        pos += 1
        value |= (b & 127) << shift
        if b < 128:
            return value, pos
    raise ValueError("bad descriptor")

def blobs(path):
    data, pos, result = path.read_bytes(), 0, []
    while pos < len(data):
        tag, pos = read_varint(data, pos)
        assert tag == 10
        length, pos = read_varint(data, pos)
        end = pos + length
        assert end <= len(data)
        result.append(data[pos:end])
        pos = end
    return result

def build(name, shape, n=0):
    root = out / name
    root.mkdir()
    files = []
    if shape == "balanced":
        for i in range(20):
            filename = f"part_{i:02d}.proto"
            (root / filename).write_text(module.render_proto(190019, 1000, 20, i))
            files.append(filename)
    elif shape == "many":
        for i in range(n):
            filename = f"dir_{i:04d}/f_{i:04d}.proto" if name.startswith("alias") else f"f_{i:04d}.proto"
            path = root / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f'syntax = "proto3"; package memo; message Message{i:04d} {{ int32 id = 1; string text = 2; }}\n')
            files.append(filename)
    elif shape in ("collision", "ambiguous"):
        defs = ([("common.proto", "root", "Root"), ("nested/common.proto", "nested", "Nested"),
                 ("third.proto", "third", "Third")] if shape == "collision"
                else [("a/common.proto", "aa", "A"), ("b/common.proto", "bb", "B")])
        for filename, pkg, msg in defs:
            path = root / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f'syntax = "proto3"; package memo.{pkg}; message {msg} {{ int32 id = 1; }}\n')
            files.append(filename)
    elif shape == "service":
        (root / "external.proto").write_text(
            'syntax = "proto3"; package memo.dep; message External { string value = 1; }\n')
        (root / "api.proto").write_text(
            'syntax = "proto3"; package memo.api; import "external.proto"; '
            'message Request { memo.dep.External item = 1; } message Reply { string text = 1; } '
            'service Echo { rpc Call(Request) returns (Reply); }\n')
        (root / "second.proto").write_text(
            'syntax = "proto3"; package memo.api; import "api.proto"; '
            'message Second { Request req = 1; } service More { rpc Call(Second) returns (Reply); }\n')
        files = ["api.proto", "second.proto"]
    else:
        raise AssertionError(shape)
    descriptor = root / "descriptor.bin"
    subprocess.run(["protoc", f"--proto_path={root}", "--include_imports",
                    f"--descriptor_set_out={descriptor}", *files], cwd=root, check=True)
    return files, blobs(descriptor)

lean = "no_reflect=true,emit_json=false,emit_text=false"
specs = [
    ("balanced20", "balanced", 0, "default", "", True),
    ("balanced20", "balanced", 0, "lean", lean, True),
    ("balanced20", "balanced", 0, "shared", "shared_descriptor_set=true", False),
    ("many128", "many", 128, "lean", lean, True),
    ("many512", "many", 512, "lean", lean, True),
    ("alias128", "many", 128, "lean", lean, True),
    ("alias512", "many", 512, "lean", lean, False),
    ("collision", "collision", 0, "lean", lean, False),
    ("ambiguous", "ambiguous", 0, "error", lean, False),
    ("services", "service", 0, "lean", lean + ",stubs=kernel,extern_path=.memo.dep.External=::external::External", True),
]
cache, manifest = {}, []
for name, shape, n, profile, parameter, perf in specs:
    if name not in cache:
        cache[name] = build(name, shape, n)
    files, descriptors = cache[name]
    targets = [Path(f).name for f in files] if name.startswith("alias") else files[:]
    if name == "ambiguous":
        targets = ["common.proto"]
    request = b"".join(field(1, x.encode()) for x in targets)
    if parameter:
        request += field(2, parameter.encode())
    request += b"".join(field(15, d) for d in descriptors)
    dest = out / f"{name}-{profile}.request"
    dest.write_bytes(request)
    manifest.append({"case": name + "-" + profile, "request": str(dest),
                     "request_sha256": hashlib.sha256(request).hexdigest(),
                     "targets": len(targets), "descriptor_files": len(descriptors),
                     "performance": perf, "expected_error": name == "ambiguous"})
# Synthetic all-files target, missing target, and repeated descriptor as controls.
files, descriptors = cache["collision"]
for name, targets, ds in (("synthetic-all", ["generated.proto"], descriptors),
                           ("unknown", ["missing.proto"], descriptors),
                           ("duplicate-descriptor", files, descriptors + [descriptors[0]])):
    request = b"".join(field(1, x.encode()) for x in targets) + field(2, lean.encode())
    request += b"".join(field(15, d) for d in ds)
    dest = out / f"{name}.request"
    dest.write_bytes(request)
    manifest.append({"case": name, "request": str(dest),
                     "request_sha256": hashlib.sha256(request).hexdigest(),
                     "targets": len(targets), "descriptor_files": len(ds),
                     "performance": False, "expected_error": name == "unknown"})
(out / "corpus.json").write_text(json.dumps(manifest, indent=2) + "\n")
print("MEMO_CORPUS_V1 " + json.dumps(manifest, sort_keys=True), flush=True)
