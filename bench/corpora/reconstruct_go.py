#!/usr/bin/env python3
"""Reconstruct google_message1/2 .proto schemas from protobuf-go (SB-05).

The historical benchmark datasets live only as generated Go code (plus
embedded FileDescriptorProtos) in protocolbuffers/protobuf-go; the
original .proto sources were never published. This script downloads the
generated files at the pinned commit, extracts each FileDescriptorProto,
emits an equivalent .proto file, and verifies wire-equivalence by
recompiling with the pinned protoc and comparing normalized descriptors.

Usage:
    python3 bench/corpora/reconstruct_go.py          # reconstruct + verify
    python3 bench/corpora/reconstruct_go.py --verify # re-download, compare
"""
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.request

from google.protobuf import descriptor_pb2

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(HERE, "manifest.json")

REPO = "protocolbuffers/protobuf-go"
SHA = "cdd4c5f7406e82462949c7a65defa9f3029c162d"
TAG = "v1.36.12"
LICENSE = "BSD-3-Clause"

# (output proto path, upstream .pb.go path, upstream source comment)
SOURCES = [
    ("benchmarks/message1_proto2.proto",
     "internal/testprotos/benchmarks/datasets/google_message1/proto2/benchmark_message1_proto2.pb.go",
     "datasets/google_message1/proto2/benchmark_message1_proto2.proto"),
    ("benchmarks/message1_proto3.proto",
     "internal/testprotos/benchmarks/datasets/google_message1/proto3/benchmark_message1_proto3.pb.go",
     "datasets/google_message1/proto3/benchmark_message1_proto3.proto"),
    ("benchmarks/message2.proto",
     "internal/testprotos/benchmarks/datasets/google_message2/benchmark_message2.pb.go",
     "datasets/google_message2/benchmark_message2.proto"),
]

TYPE_NAMES = {
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_DOUBLE: "double",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_FLOAT: "float",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_INT64: "int64",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_UINT64: "uint64",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_INT32: "int32",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_FIXED64: "fixed64",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_FIXED32: "fixed32",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_BOOL: "bool",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_STRING: "string",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_BYTES: "bytes",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_UINT32: "uint32",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_SFIXED32: "sfixed32",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_SFIXED64: "sfixed64",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_SINT32: "sint32",
    descriptor_pb2.FieldDescriptorProto.Type.TYPE_SINT64: "sint64",
}


def fetch(path):
    url = f"https://raw.githubusercontent.com/{REPO}/{SHA}/{path}"
    req = urllib.request.Request(url, headers={"User-Agent": "pure-protobuf-corpora"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def extract_descriptor(go_src):
    m = re.search(r"var file_\w+_rawDesc = \[\]byte\{\n(.*?)\n\}", go_src, re.S)
    raw = bytes(int(x, 16) for x in re.findall(r"0x([0-9a-fA-F]{2})", m.group(1)))
    fd = descriptor_pb2.FileDescriptorProto()
    fd.ParseFromString(raw)
    return fd


def c_quote(s):
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif 32 <= o < 127:
            out.append(ch)
        elif o < 256:
            out.append("\\%03o" % o)
        else:
            out.append("\\u%04x" % o)
    out.append('"')
    return "".join(out)


def default_text(fl):
    d = fl.default_value
    if fl.type in (descriptor_pb2.FieldDescriptorProto.Type.TYPE_STRING,
                   descriptor_pb2.FieldDescriptorProto.Type.TYPE_BYTES):
        return c_quote(d)
    return d


def rel_name(type_name, package):
    assert type_name.startswith(".")
    prefix = "." + package + "."
    assert type_name.startswith(prefix), type_name
    return type_name[len(prefix):]


def emit_field(fl, package, indent, is_proto3):
    T = descriptor_pb2.FieldDescriptorProto.Type
    L = descriptor_pb2.FieldDescriptorProto.Label
    opts = []
    if fl.HasField("default_value"):
        opts.append(f"default = {default_text(fl)}")
    if fl.options.HasField("packed"):
        opts.append(f"packed = {'true' if fl.options.packed else 'false'}")
    opt_str = f" [{', '.join(opts)}]" if opts else ""
    if is_proto3:
        label = "repeated " if fl.label == L.LABEL_REPEATED else ""
    elif fl.label == L.LABEL_REQUIRED:
        label = "required "
    elif fl.label == L.LABEL_REPEATED:
        label = "repeated "
    else:
        label = "optional "
    if fl.type in (T.TYPE_MESSAGE, T.TYPE_ENUM):
        typ = rel_name(fl.type_name, package)
    else:
        typ = TYPE_NAMES[fl.type]
    return f"{indent}{label}{typ} {fl.name} = {fl.number}{opt_str};"


def emit_message(msg, package, indent, is_proto3, lines):
    T = descriptor_pb2.FieldDescriptorProto.Type
    L = descriptor_pb2.FieldDescriptorProto.Label
    group_bodies = {n.name: n for n in msg.nested_type}
    lines.append(f"{indent}message {msg.name} {{")
    sub = indent + "  "
    for fl in msg.field:
        if fl.type == T.TYPE_GROUP:
            gname = rel_name(fl.type_name, package).split(".")[-1]
            body = group_bodies.pop(gname)
            glabel = {L.LABEL_REQUIRED: "required ", L.LABEL_REPEATED: "repeated "}.get(
                fl.label, "optional ")
            lines.append(f"{sub}{glabel}group {gname} = {fl.number} {{")
            for gfl in body.field:
                lines.append(emit_field(gfl, package, sub + "  ", is_proto3))
            lines.append(f"{sub}}}")
        else:
            lines.append(emit_field(fl, package, sub, is_proto3))
    for name, body in group_bodies.items():
        emit_message(body, package, sub, is_proto3, lines)
    for en in msg.enum_type:
        emit_enum(en, sub, lines)
    lines.append(f"{indent}}}")


def emit_enum(en, indent, lines):
    lines.append(f"{indent}enum {en.name} {{")
    for v in en.value:
        lines.append(f"{indent}  {v.name} = {v.number};")
    lines.append(f"{indent}}}")


HEADER = """\
// Reconstructed from the FileDescriptorProto embedded in
// https://github.com/%s/blob/%s/%s
// (upstream source comment: %s; upstream license: %s).
// The original .proto was never published; this file is verified
// wire-equivalent (see reconstruct_go.py --verify and manifest.json).
"""


def emit(fd, upstream_path, upstream_source):
    is_proto3 = fd.syntax == "proto3"
    assert not fd.extension, "file-level extensions not supported"
    assert not fd.service, "services not supported"
    lines = [HEADER % (REPO, SHA, upstream_path, upstream_source, LICENSE)]
    lines.append(f'syntax = "{fd.syntax or "proto2"}";')
    lines.append(f"package {fd.package};")
    for msg in fd.message_type:
        lines.append("")
        emit_message(msg, fd.package, "", is_proto3, lines)
    for en in fd.enum_type:
        lines.append("")
        emit_enum(en, "", lines)
    return "\n".join(lines) + "\n"


def normalize_field(fl):
    d = {
        "name": fl.name, "number": fl.number, "label": fl.label,
        "type": fl.type, "type_name": fl.type_name,
        "default": fl.default_value if fl.HasField("default_value") else None,
        "json_name": fl.json_name,
        "oneof": fl.oneof_index if fl.HasField("oneof_index") else None,
        "packed": fl.options.packed if fl.options.HasField("packed") else None,
    }
    return d


def normalize_msg(m):
    return {
        "name": m.name,
        "fields": [normalize_field(f) for f in m.field],
        "nested": [normalize_msg(n) for n in m.nested_type],
        "enums": [normalize_enum(e) for e in m.enum_type],
        "oneofs": [o.name for o in m.oneof_decl],
    }


def normalize_enum(e):
    return {"name": e.name,
            "values": [(v.name, v.number) for v in e.value]}


def normalize(fd):
    return {
        "syntax": fd.syntax or "proto2",
        "package": fd.package,
        "messages": [normalize_msg(m) for m in fd.message_type],
        "enums": [normalize_enum(e) for e in fd.enum_type],
    }


def protoc():
    for cand in [os.environ.get("PROTOC"),
                 os.path.join(ROOT, "target", "pinned-protoc-build", "protoc"),
                 "protoc"]:
        if cand and os.path.exists(cand):
            return cand
    return "protoc"


def verify(out_path, want_fd):
    protos = os.path.join(HERE, "google-messages", "protos")
    fds = os.path.join("/tmp", "sb05-gm-verify.fds")
    subprocess.run([protoc(), "-I", protos, "--descriptor_set_out", fds,
                    out_path], check=True, capture_output=True, text=True)
    fset = descriptor_pb2.FileDescriptorSet()
    fset.ParseFromString(open(fds, "rb").read())
    got = next(f for f in fset.file if f.name == out_path)
    a, b = normalize(want_fd), normalize(got)
    if a != b:
        import difflib
        ja = json.dumps(a, indent=1, sort_keys=True).splitlines()
        jb = json.dumps(b, indent=1, sort_keys=True).splitlines()
        sys.stderr.write("\n".join(difflib.unified_diff(ja, jb, "upstream", "rebuilt")) + "\n")
        raise SystemExit(f"descriptor mismatch for {out_path}")
    return True


def main(args):
    verify_only = "--verify" in args
    manifest = json.load(open(MANIFEST)) if os.path.exists(MANIFEST) else {"corpora": {}}
    entry = manifest.setdefault("corpora", {}).setdefault("google-messages", {})
    entry["classification"] = "primary"
    entry["upstream"] = [{"repo": REPO, "commit": SHA, "tag": TAG, "license": LICENSE}]
    files = {}
    for out_path, upstream_path, upstream_source in SOURCES:
        data = fetch(upstream_path)
        go_hash = hashlib.sha256(data).hexdigest()
        fd = extract_descriptor(data.decode("utf-8"))
        text = emit(fd, upstream_path, upstream_source)
        dest = os.path.join(HERE, "google-messages", "protos", out_path)
        if verify_only:
            old = open(dest).read()
            if old != text:
                raise SystemExit(f"reconstruction drifted for {out_path}")
            print(f"  {out_path}: reconstruction stable")
        else:
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            open(dest, "w").write(text)
            print(f"  wrote {dest} ({len(text)} bytes)")
        verify(out_path, fd)
        print(f"  {out_path}: descriptor-equivalent")
        files[out_path] = {
            "sha256": hashlib.sha256(text.encode()).hexdigest(),
            "upstream_pb_go": upstream_path,
            "upstream_pb_go_sha256": go_hash,
        }
    entry["roots"] = [s[0] for s in SOURCES]
    entry["files"] = files
    if not verify_only:
        json.dump(manifest, open(MANIFEST, "w"), indent=1, sort_keys=True)
        print(f"manifest: {MANIFEST}")


if __name__ == "__main__":
    main(sys.argv[1:])
