#!/usr/bin/env python3
"""Prepare pinned direct generator requests and downstream consumer recipes."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

out = Path(sys.argv[1])
out.mkdir(parents=True)
spec = importlib.util.spec_from_file_location("bench_codegen", "bench/codegen/run.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

def varint(value):
    data = bytearray()
    while value >= 128:
        data.append((value & 127) | 128)
        value >>= 7
    return bytes(data) + bytes([value])

def field(tag, data):
    return varint((tag << 3) | 2) + varint(len(data)) + data

def descriptor_files(data):
    pos = 0
    def read():
        nonlocal pos
        value = 0
        for shift in range(0, 70, 7):
            b = data[pos]
            pos += 1
            value |= (b & 127) << shift
            if b < 128:
                return value
        raise ValueError("invalid descriptor framing")
    while pos < len(data):
        assert read() == 10
        size = read()
        end = pos + size
        assert end <= len(data)
        yield data[pos:end]
        pos = end

def sha(data):
    return hashlib.sha256(data).hexdigest()

lean = "stubs=none,no_reflect=true,emit_json=false,emit_text=false"
default = "stubs=none"
cases = []

def add(name, inputs, params, timed=True, consumer=None):
    root = out / name
    root.mkdir()
    for path, data in inputs.items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    names = sorted(inputs)
    fds = root / "descriptor.bin"
    subprocess.run([os.environ["PROTOC"], "-I", str(root), "--include_imports",
                    f"--descriptor_set_out={fds}", *names], cwd=root, check=True)
    blobs = list(descriptor_files(fds.read_bytes()))
    assert len(blobs) == len(names)
    for label, paths, descs in (("request", names, blobs),
                                ("permuted", reversed(names), reversed(blobs))):
        request = (b"".join(field(1, path.encode()) for path in paths)
                   + field(2, params.encode())
                   + b"".join(field(15, descriptor) for descriptor in descs))
        (root / label).write_bytes(request)
    if consumer:
        command, source = consumer
        os.environ["SB09_PBRS_RUNTIME_PROFILE"] = "minimal" if params == lean else "json-text"
        (root / "consumer-Cargo.toml").write_text(bench.manifest(f"field-name-consumer-{name}"))
        (root / "consumer-main.rs").write_text(source)
    cases.append({
        "name": name, "params": params, "timed": timed, "consumer": consumer[0] if consumer else None,
        "protos": {name: sha(data) for name, data in inputs.items()},
        "descriptor_sha256": sha(fds.read_bytes()),
        "requests": {label: sha((root / label).read_bytes()) for label in ("request", "permuted")},
    })

def seeded(n, files):
    return {f"part_{index:02d}.proto": bench.render_proto(bench.DEFAULT_SEED, n, files, index).encode()
            for index in range(files)}

add("small-default", seeded(6, 2), default)
add("large-lean", seeded(1000, 20), lean)
otlp = {}
for name in bench.realistic_generation_names("otlp"):
    repo, revision, path, expected = bench.VENDOR_FILES[name]
    data = subprocess.check_output(["curl", "--fail", "--silent", "--show-error", "--location",
                                    "--max-time", "60", "--retry", "2",
                                    f"{bench.VENDOR_BASE}/{repo}/{revision}/{path}"])
    assert sha(data) == expected, (name, expected, sha(data))
    otlp[name] = data
add("otlp-default", otlp, default)

plain = b'''syntax = "proto3";
package naming.bind;
message Leaf {
  bytes data = 1;
}
message Plain {
  map<string, string> labels = 1;
}
'''
selected = b'''syntax = "proto3";
package naming.bind;
import "plain.proto";
message Holder {
  map<string, Leaf> clone = 1;
  map<string, Leaf> clone_field = 2 [json_name = "literalCloneField"];
  map<string, bytes> blobs = 3;
  Leaf child = 4;
  bytes payload = 5;
  string payload_shared = 6;
}
'''
consumer = '''include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));
use pbrs::{Parse as _, Serialize as _};
use pbrs::rt::Bytes;

fn main() {
    use naming::bind::{Holder, Leaf};
    let mut holder = Holder::new();
    let mut first = Leaf::new();
    first.set_data(Bytes::from_static(b"first"));
    let mut second = Leaf::new();
    second.set_data(Bytes::from_static(b"second"));
    holder.clone_field_mut().insert("one", first);
    holder.clone_field_2_mut().insert("two", second);
    holder.blobs_mut().insert("blob", Bytes::from_static(b"selected map value"));
    holder.child_mut().set_data(Bytes::from_static(b"boxed child"));
    holder.set_payload(Bytes::from_static(b"selected bytes"));
    holder.set_payload_shared("overlapping name");
    let wire = holder.serialize().unwrap();
    let parsed = Holder::parse(&wire).unwrap();
    assert_eq!(parsed.clone_field().get("one").unwrap().data(), b"first");
    assert_eq!(parsed.clone_field_2().get("two").unwrap().data(), b"second");
    assert!(parsed.blobs().get("blob").is_some());
    assert_eq!(parsed.child().data(), b"boxed child");
    assert_eq!(parsed.payload_shared_2().as_bytes(), b"selected bytes");
    assert_eq!(parsed.payload_shared(), "overlapping name");
    println!("map and storage selectors round trip passed");
}
'''
proto = {"plain.proto": plain, "selected.proto": selected}
opts = default + (
    ",boxed=.naming.bind.Holder.child"
    ",btree_map=.naming.bind.Holder.clone_field"
    ",bytes=.naming.bind.Holder.payload"
    ",bytes=.naming.bind.Holder.value"
    ",bytes=.naming.bind.Leaf.data"
)
add("map-default", proto, default, False)
add("map-selectors", proto, opts, consumer=("run", consumer))
add("map-owned-selectors", proto, opts + ",owned_message_fields=true", False)
(out / "cases.json").write_text(json.dumps(cases, indent=2, sort_keys=True) + "\n")
