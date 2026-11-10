#!/usr/bin/env python3
"""Extend the recorded codegen requests with interleaved and import controls."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

original, out = map(Path, sys.argv[1:])
shutil.copytree(original, out)
manifest = json.loads((out / "corpus.json").read_text())
perf = {"balanced20-default", "balanced20-lean", "many128-lean",
        "many512-lean", "alias128-lean", "services-lean", "interleaved-lean"}
for item in manifest:
    item["request"] = str(out / Path(item["request"]).name)
    assert hashlib.sha256(Path(item["request"]).read_bytes()).hexdigest() == item["request_sha256"]
    item["performance"] = item["case"] in perf

def varint(value):
    data = bytearray()
    while value >= 128:
        data.append((value & 127) | 128)
        value >>= 7
    data.append(value)
    return bytes(data)

def field(number, value):
    return varint((number << 3) | 2) + varint(len(value)) + value

def descriptor_blobs(data):
    pos = 0
    result = []
    def read():
        nonlocal pos
        value = 0
        for shift in range(0, 70, 7):
            b = data[pos]
            pos += 1
            value |= (b & 127) << shift
            if b < 128:
                return value
        raise ValueError("bad descriptor")
    while pos < len(data):
        assert read() == 10
        size = read()
        end = pos + size
        assert end <= len(data)
        result.append(data[pos:end])
        pos = end
    return result

def make_descriptor(root, targets, includes=()):
    fd = root / "descriptor.bin"
    subprocess.run(["protoc", f"--proto_path={root}",
                    *(f"--proto_path={p}" for p in includes),
                    "--include_imports", f"--descriptor_set_out={fd}", *targets],
                   cwd=root, check=True)
    return descriptor_blobs(fd.read_bytes())

def add(case, targets, blobs, params, expected_error=False):
    request = b"".join(field(1, name.encode()) for name in targets)
    request += field(2, params.encode())
    request += b"".join(field(15, blob) for blob in blobs)
    path = out / (case + ".request")
    path.write_bytes(request)
    manifest.append({"case": case, "request": str(path),
                     "request_sha256": hashlib.sha256(request).hexdigest(),
                     "targets": len(targets), "descriptor_files": len(blobs),
                     "performance": case in perf, "expected_error": expected_error})

lean = "no_reflect=true,emit_json=false,emit_text=false"
root = out / "interleaved"
root.mkdir()
targets = []
for file_index in range(20):
    name = f"part_{file_index:02d}.proto"
    targets.append(name)
    lines = ['syntax = "proto3";', "package cache.interleaved;"]
    for offset in range(50):
        number = offset * 20 + file_index
        lines.append(f"message Message{number:04d} {{ int32 id = 1; string text = 2; repeated int64 nums = 3; map<string,string> meta = 4; }}")
    (root / name).write_text("\n".join(lines) + "\n")
add("interleaved-lean", targets, make_descriptor(root, targets), lean)

root = out / "public-import"
root.mkdir()
(root / "shared.proto").write_text('syntax = "proto3"; package cache.publics; message Shared { string value = 1; }\n')
(root / "a.proto").write_text('syntax = "proto3"; package cache.publics; import public "shared.proto"; message Alpha { Shared value = 1; }\n')
(root / "b.proto").write_text('syntax = "proto3"; package cache.publics; import "a.proto"; message Beta { Alpha a = 1; Shared b = 2; }\n')
targets = ["a.proto", "b.proto"]
descs = make_descriptor(root, targets)
add("public-transitive", targets, descs, lean)
add("public-emit-deps", targets, descs, lean + ",emit_deps=true")
add("public-owned", ["a.proto", "b.proto", "shared.proto"], descs, lean)

root = out / "wkt"
root.mkdir()
(root / "a.proto").write_text('syntax = "proto3"; package cache.wkt; import "google/protobuf/any.proto"; message Alpha { google.protobuf.Any value = 1; }\n')
(root / "b.proto").write_text('syntax = "proto3"; package cache.wkt; import "a.proto"; message Beta { Alpha a = 1; }\n')
targets = ["a.proto", "b.proto"]
descs = make_descriptor(root, targets, ["/usr/include"])
add("wkt-shared", targets, descs, lean)

assert len(manifest) == 18 and {r["case"] for r in manifest if r["performance"]} == perf
(out / "corpus.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
print("TARGET_CACHE_CORPUS_V1 " + json.dumps(manifest, sort_keys=True), flush=True)
