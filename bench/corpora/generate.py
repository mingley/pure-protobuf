#!/usr/bin/env python3
"""Generate deterministic seeded payloads for the bench/corpora/ corpora.

Builds a FileDescriptorSet per corpus with the pinned protoc, populates
the configured root messages with a seeded RNG, and writes length-tiered
binary payloads. Every payload is reproducible from (corpus, message,
tier, seed): `generate.py --verify` regenerates everything and compares
SHA-256 hashes against manifest.json.

Usage:
    python3 bench/corpora/generate.py            # generate all tiers
    python3 bench/corpora/generate.py otlp       # one corpus
    python3 bench/corpora/generate.py --verify   # determinism check only

Requires: python3 + protobuf runtime, protoc ($PROTOC or the repo's
pinned build or PATH). Only tiny/typical payloads are checked in;
large/huge are regenerated on demand (see README.md).
"""
import hashlib
import json
import os
import random
import string
import subprocess
import sys

from google.protobuf import descriptor_pb2
from google.protobuf import descriptor_pool
from google.protobuf import message_factory
from google.protobuf.message import Message

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(HERE, "manifest.json")

# Tier -> (min_bytes_inclusive, max_bytes_exclusive).
TIERS = {
    "tiny": (0, 64),
    "typical": (64, 4096),
    "large": (64 * 1024, 1024 * 1024),
    "huge": (8 * 1024 * 1024, 64 * 1024 * 1024),
}

# corpus -> [(short name, full message name)]
MESSAGES = {
    "otlp": [
        ("spans", "opentelemetry.proto.trace.v1.ResourceSpans"),
        ("metrics", "opentelemetry.proto.metrics.v1.ResourceMetrics"),
        ("logs", "opentelemetry.proto.logs.v1.ResourceLogs"),
    ],
    "xds": [
        ("cluster", "envoy.config.cluster.v3.Cluster"),
        ("cla", "envoy.config.endpoint.v3.ClusterLoadAssignment"),
        ("route", "envoy.config.route.v3.RouteConfiguration"),
    ],
    "googleapis": [
        ("status", "google.rpc.Status"),
    ],
    "grpc-testing": [
        ("request", "grpc.testing.SimpleRequest"),
        ("response", "grpc.testing.SimpleResponse"),
    ],
    "google-messages": [
        ("gm1_proto2", "benchmarks.proto2.GoogleMessage1"),
        ("gm1_proto3", "benchmarks.proto3.GoogleMessage1"),
        ("gm2", "benchmarks.proto2.GoogleMessage2"),
    ],
}

# corpus -> full name of the message packed into google.protobuf.Any fields.
ANY_CONTENT = {
    "xds": "google.protobuf.Struct",
    "googleapis": "google.rpc.ErrorInfo",
}

ALPHA = string.ascii_letters + string.digits + " "


def protoc():
    for cand in [os.environ.get("PROTOC"),
                 os.path.join(ROOT, "target", "pinned-protoc-build", "protoc"),
                 "protoc"]:
        if cand and (os.sep not in cand or os.path.exists(cand)):
            return cand
    return "protoc"


def protoc_version():
    out = subprocess.run([protoc(), "--version"], check=True,
                         capture_output=True, text=True)
    return out.stdout.strip()


def build_pool(corpus, roots):
    protos = os.path.join(HERE, corpus, "protos")
    fds_path = os.path.join("/tmp", f"sb05-{corpus}.fds")
    subprocess.run([protoc(), "-I", protos, "--descriptor_set_out", fds_path,
                    "--include_imports", *roots],
                   check=True, capture_output=True, text=True)
    fset = descriptor_pb2.FileDescriptorSet()
    fset.ParseFromString(open(fds_path, "rb").read())
    pool = descriptor_pool.DescriptorPool()
    for f in fset.file:
        pool.Add(f)
    return pool


def rand_scalar(rng, field):
    t = field.type
    if t == field.TYPE_BOOL:
        return rng.random() < 0.5
    if t == field.TYPE_FLOAT:
        return rng.uniform(-1e3, 1e3)
    if t == field.TYPE_DOUBLE:
        return rng.uniform(-1e6, 1e6)
    if t in (field.TYPE_INT32, field.TYPE_SINT32, field.TYPE_SFIXED32):
        return rng.randint(-100000, 100000)
    if t in (field.TYPE_INT64, field.TYPE_SINT64, field.TYPE_SFIXED64):
        return rng.randint(-1000000000, 1000000000)
    if t in (field.TYPE_UINT32, field.TYPE_FIXED32):
        return rng.randint(0, 200000)
    if t in (field.TYPE_UINT64, field.TYPE_FIXED64):
        return rng.randint(0, 2000000000)
    if t == field.TYPE_ENUM:
        return rng.choice(list(field.enum_type.values_by_number))
    raise AssertionError(f"not a scalar: {t}")


def rand_text(rng, n):
    return "".join(rng.choice(ALPHA) for _ in range(max(n, 1)))


def is_message(field):
    return field.type in (field.TYPE_MESSAGE, field.TYPE_GROUP)


def is_map(field):
    return (is_message(field) and field.message_type is not None
            and field.message_type.GetOptions().map_entry)


def is_any(field):
    return (field.type == field.TYPE_MESSAGE
            and field.message_type.full_name == "google.protobuf.Any")


class Filler:
    """Seeded descriptor-driven message population."""

    def __init__(self, pool, any_msg, max_depth, fill_prob, max_items, max_bytes):
        self.pool = pool
        self.any_msg = any_msg
        self.max_depth = max_depth
        self.fill_prob = fill_prob
        self.max_items = max_items
        self.max_bytes = max_bytes

    def fill_message(self, msg, rng, depth):
        desc = msg.DESCRIPTOR
        # Real oneof members (synthetic proto3-optional oneofs count as
        # plain optional fields).
        oneofs = {}
        for f in desc.fields:
            if f.containing_oneof is not None and not f.has_presence:
                oneofs.setdefault(f.containing_oneof.name, []).append(f)
        in_real_oneof = {f.name for fs in oneofs.values() for f in fs}
        for f in sorted(desc.fields, key=lambda f: f.number):
            if f.name in in_real_oneof:
                continue
            if f.is_required or rng.random() < self.fill_prob:
                self.fill_field(msg, f, rng, depth)
        for _, fs in sorted(oneofs.items()):
            if rng.random() < self.fill_prob:
                self.fill_field(msg, rng.choice(sorted(fs, key=lambda f: f.number)),
                                rng, depth)

    def fill_field(self, msg, f, rng, depth):
        if is_map(f):
            m = getattr(msg, f.name)
            for _ in range(rng.randint(0, 2)):
                v = self.rand_value(rng, f, depth)
                if isinstance(v, Message):
                    m[self.rand_key(rng, f)].CopyFrom(v)
                else:
                    m[self.rand_key(rng, f)] = v
            return
        if f.is_repeated:
            n = 1 + int(rng.random() * self.max_items)
            rep = getattr(msg, f.name)
            for _ in range(n):
                self.append_one(rep, f, rng, depth)
            return
        if is_message(f):
            if depth >= self.max_depth and not f.is_required:
                return
            if is_any(f):
                content = message_factory.GetMessageClass(
                    self.pool.FindMessageTypeByName(self.any_msg))()
                self.fill_message(content, rng, depth + 1)
                # Pack manually: Any.Pack() does not serialize
                # deterministically (map order inside content varies).
                packed = getattr(msg, f.name)
                packed.type_url = ("type.googleapis.com/"
                                   + content.DESCRIPTOR.full_name)
                packed.value = content.SerializeToString(deterministic=True)
                return
            self.fill_message(getattr(msg, f.name), rng, depth + 1)
            return
        if f.type == f.TYPE_STRING:
            setattr(msg, f.name, rand_text(rng, 1 + int(rng.random() * self.max_bytes)))
        elif f.type == f.TYPE_BYTES:
            setattr(msg, f.name, rng.randbytes(1 + int(rng.random() * self.max_bytes)))
        else:
            setattr(msg, f.name, rand_scalar(rng, f))

    def append_one(self, rep, f, rng, depth):
        if is_message(f):
            self.fill_message(rep.add(), rng, depth + 1)
        elif f.type == f.TYPE_STRING:
            rep.append(rand_text(rng, 1 + int(rng.random() * self.max_bytes)))
        elif f.type == f.TYPE_BYTES:
            rep.append(rng.randbytes(1 + int(rng.random() * self.max_bytes)))
        else:
            rep.append(rand_scalar(rng, f))

    def rand_key(self, rng, f):
        kf = f.message_type.fields_by_name["key"]
        if kf.type == kf.TYPE_STRING:
            return rand_text(rng, 1 + int(rng.random() * 12))
        if kf.type == kf.TYPE_BOOL:
            return rng.random() < 0.5
        return rng.randint(0, 1000000)

    def rand_value(self, rng, f, depth):
        vf = f.message_type.fields_by_name["value"]
        if is_message(vf):
            sub = message_factory.GetMessageClass(vf.message_type)()
            self.fill_message(sub, rng, depth + 1)
            return sub
        if vf.type == vf.TYPE_STRING:
            return rand_text(rng, 1 + int(rng.random() * self.max_bytes))
        if vf.type == vf.TYPE_BYTES:
            return rng.randbytes(1 + int(rng.random() * self.max_bytes))
        return rand_scalar(rng, vf)


def grow_targets(msg, depth=0):
    """BFS for appendable fields: repeated/map/singular bytes/string."""
    found = []
    if depth > 3:
        return found
    for f in sorted(msg.DESCRIPTOR.fields, key=lambda f: f.number):
        if f.is_repeated or is_map(f):
            found.append((msg, f))
        elif f.type in (f.TYPE_BYTES, f.TYPE_STRING) and not f.is_repeated:
            found.append((msg, f))
        elif is_message(f) and not is_any(f) and msg.HasField(f.name):
            found.extend(grow_targets(getattr(msg, f.name), depth + 1))
    return found


def grow_step(msg, field, filler, rng, chunk):
    if is_map(field):
        m = getattr(msg, field.name)
        v = filler.rand_value(rng, field, 0)
        if isinstance(v, Message):
            m[filler.rand_key(rng, field)].CopyFrom(v)
        else:
            m[filler.rand_key(rng, field)] = v
        return
    if field.is_repeated:
        rep = getattr(msg, field.name)
        if is_message(field):
            filler.fill_message(rep.add(), rng, 1)
        elif field.type == field.TYPE_STRING:
            rep.append(rand_text(rng, chunk))
        elif field.type == field.TYPE_BYTES:
            rep.append(rng.randbytes(chunk))
        else:
            for _ in range(max(chunk // 4, 1)):
                rep.append(rand_scalar(rng, field))
        return
    if field.type == field.TYPE_STRING:
        cur = getattr(msg, field.name)
        setattr(msg, field.name, cur + rand_text(rng, chunk))
    else:
        cur = getattr(msg, field.name)
        setattr(msg, field.name, cur + rng.randbytes(chunk))


def ser_len(msg):
    return len(msg.SerializeToString(deterministic=True))


def build_payload(pool, full_name, any_msg, tier, seed):
    cls = message_factory.GetMessageClass(pool.FindMessageTypeByName(full_name))
    lo, hi = TIERS[tier]
    rng = random.Random(seed)
    if tier == "tiny":
        msg = cls()
        Filler(pool, any_msg, 1, 0.0, 1, 4).fill_message(msg, rng, 0)
        # Force low-numbered singular fields until tiny is non-trivial
        # (a first field stuck at its implicit default serializes empty).
        if ser_len(msg) == 0:
            for f in sorted(msg.DESCRIPTOR.fields, key=lambda f: f.number):
                if f.is_repeated or is_map(f):
                    continue
                Filler(pool, any_msg, 1, 1.0, 1, 4).fill_field(msg, f, rng, 0)
                if ser_len(msg) > 0:
                    break
        return msg
    depth = 3 if tier == "typical" else 4
    # Densest skeleton that still fits under the band ceiling; growth
    # below then lifts small skeletons to the band floor.
    msg = None
    for prob in (1.0, 0.75, 0.5, 0.25):
        attempt = cls()
        filler = Filler(pool, any_msg, depth, prob, 3, 32)
        filler.fill_message(attempt, random.Random(seed), 0)
        if ser_len(attempt) < hi:
            msg = attempt
            break
    if msg is None:
        raise SystemExit(f"skeleton for {full_name} exceeds {tier} band at any density")
    filler = Filler(pool, any_msg, depth, 1.0, 3, 32)
    targets = grow_targets(msg)
    if not targets and ser_len(msg) < lo:
        raise SystemExit(f"no growable field in {full_name} for {tier} band")
    chunk = max((hi - lo) // 32, 16)
    i = 0
    while ser_len(msg) < lo:
        m, f = targets[i % len(targets)]
        grow_step(m, f, filler, rng, chunk)
        i += 1
        if i > 100000:
            raise SystemExit(f"growth did not converge for {full_name} {tier}")
    if ser_len(msg) >= hi:
        raise SystemExit(f"overshot {tier} band for {full_name}")
    return msg


def seed_for(corpus, short, tier):
    h = hashlib.sha256(f"sb05/{corpus}/{short}/{tier}".encode()).digest()
    return int.from_bytes(h[:8], "little")


def main(args):
    verify_only = "--verify" in args
    names = [a for a in args if not a.startswith("--")] or sorted(MESSAGES)
    manifest = json.load(open(MANIFEST))
    manifest.setdefault("config", {})["protoc"] = protoc_version()
    manifest["config"]["tiers"] = {k: list(v) for k, v in TIERS.items()}
    failed = 0
    for corpus in names:
        if corpus not in MESSAGES:
            raise SystemExit(f"unknown corpus: {corpus}")
        roots = manifest["corpora"][corpus]["roots"]
        pool = build_pool(corpus, roots)
        any_msg = ANY_CONTENT.get(corpus, "google.protobuf.Struct")
        entry = manifest["corpora"][corpus].setdefault("payloads", {})
        for short, full in MESSAGES[corpus]:
            for tier in TIERS:
                seed = seed_for(corpus, short, tier)
                msg = build_payload(pool, full, any_msg, tier, seed)
                blob = msg.SerializeToString(deterministic=True)
                lo, hi = TIERS[tier]
                if not (lo <= len(blob) < hi):
                    print(f"  {corpus}/{short}/{tier}: size {len(blob)} "
                          f"outside [{lo}, {hi})")
                    failed += 1
                    continue
                digest = hashlib.sha256(blob).hexdigest()
                rel = f"payloads/{corpus}/{short}/{tier}.bin"
                if verify_only:
                    want = entry.get(f"{short}/{tier}", {})
                    if want.get("sha256") != digest or want.get("size") != len(blob):
                        print(f"  MISMATCH {rel}: manifest={want} "
                              f"got size={len(blob)} sha={digest[:16]}")
                        failed += 1
                    continue
                dest = os.path.join(HERE, rel)
                os.makedirs(os.path.dirname(dest), exist_ok=True)
                open(dest, "wb").write(blob)
                entry[f"{short}/{tier}"] = {
                    "message": full, "seed": seed,
                    "size": len(blob), "sha256": digest,
                }
                print(f"  {rel}: {len(blob)} bytes", flush=True)
    if failed:
        raise SystemExit(f"{failed} payload failures")
    if not verify_only:
        json.dump(manifest, open(MANIFEST, "w"), indent=1, sort_keys=True)
        print(f"manifest: {MANIFEST}")
    else:
        print("all payloads reproduce recorded hashes")


if __name__ == "__main__":
    main(sys.argv[1:])
