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

add("small-default", seeded(6, 2), default, consumer=("run", bench.render_consumer(6, 61)))
large = seeded(1000, 20)
add("large-default", large, default)
add("large-lean", large, lean, consumer=("check", bench.render_consumer(1000, 1001)))
otlp = {}
for name in bench.realistic_generation_names("otlp"):
    repo, revision, path, expected = bench.VENDOR_FILES[name]
    data = subprocess.check_output(["curl", "--fail", "--silent", "--show-error", "--location",
                                    "--max-time", "60", "--retry", "2",
                                    f"{bench.VENDOR_BASE}/{repo}/{revision}/{path}"])
    assert sha(data) == expected, (name, expected, sha(data))
    otlp[name] = data
add("otlp-default", otlp, default,
    consumer=("run", bench.render_consumer_entries("otlp", 31, "pbrs")))
add("otlp-lean", otlp, lean)

many = {}
for index in range(128):
    path = f"file_{index:03}.proto"
    many[path] = (f'syntax = "proto3"; package naming.many; message Row{index:03} '
                  '{ int32 id = 1; string routing_label = 2; }\n').encode()
add("many-lean", many, lean)

collision = b'''syntax = "proto2";
package naming.collisions;
message Child {
  optional string value = 1;
}
message Parent {
  optional string clone = 1;
  optional string clone_field = 2 [json_name = "originalCloneField"];
  optional int32 self = 3;
  optional int32 self_ = 4 [json_name = "trailingSelf"];
  optional Child child = 5;
  optional string child_view = 6;
  optional string type = 7;
}
'''
collision_consumer = '''include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));
use pbrs::{Parse as _, Serialize as _};

fn main() {
    let mut msg = naming::collisions::Parent::new();
    msg.set_clone_field("renamed clone");
    msg.set_clone_field_2("literal clone field");
    msg.set_self(13);
    msg.set_self_(17);
    msg.child_mut().set_value("child value");
    msg.set_child_view_6("literal child view");
    msg.set_type("keyword");
    let wire = msg.serialize().unwrap();
    let parsed = naming::collisions::Parent::parse(&wire).unwrap();
    assert_eq!(parsed.clone_field(), "renamed clone");
    assert_eq!(parsed.clone_field_2(), "literal clone field");
    assert_eq!(parsed.self_(), 13);
    assert_eq!(parsed.self__4(), 17);
    assert_eq!(parsed.child_view().value(), "child value");
    assert_eq!(parsed.child_view_6(), "literal child view");
    assert_eq!(parsed.r#type(), "keyword");
    println!("collision round trip passed");
}
'''
add("collision-default", {"nested/collisions.proto": collision}, default, False,
    consumer=("run", collision_consumer))
(out / "cases.json").write_text(json.dumps(cases, indent=2, sort_keys=True) + "\n")
