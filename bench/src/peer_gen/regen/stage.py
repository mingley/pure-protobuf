#!/usr/bin/env python3
"""Stage SB-08 checked-in peer gencode into bench/src/peer_gen/.

Copies /tmp/sb08out/* (see regen/src/main.rs) into the worktree with
mechanical path rewrites for renamed dependencies and prepends SB-08
provenance headers. Rerunnable (idempotent).

Usage: python3 stage.py /path/to/worktree
"""
import hashlib
import pathlib
import sys

WORKTREE = pathlib.Path(sys.argv[1])
OUT = pathlib.Path("/tmp/sb08out")
GEN = WORKTREE / "bench/src/peer_gen"

PERSON_SHA = "13ec16099b636bb46467e28199347c79ba26e72223462c1c3060e28d6c1620c5"
TAT_SHA = "c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873"
PIN = "protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03"

HEADER = """\
// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: {generator}
// source: {source} (sha256 {sha})
// rewrite: {rewrite}
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
"""


def emit(src: pathlib.Path, dst: pathlib.Path, generator: str, source: str,
         sha: str, rewrite: str, subs: list[tuple[str, str]]):
    text = src.read_text()
    for old, new in subs:
        if old not in text and dst.suffix == ".rs" and "generated.rs" not in dst.name:
            print(f"WARN: {old!r} not found in {src.name}")
        text = text.replace(old, new)
    head = HEADER.format(generator=generator, source=source, sha=sha, rewrite=rewrite)
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(head + text)
    print(f"staged {dst.relative_to(WORKTREE)} ({len(text.splitlines())} lines)")


def main():
    person = "proto/person.proto"
    tat = "third_party/protobuf/src/google/protobuf/test_messages_proto3.proto"

    # prost 0.13 person (no rewrite: bench `prost` IS 0.13)
    emit(OUT / "prost13_person/example.rs", GEN / "prost13_person.rs",
         "prost-build 0.13.5 + protoc 36.2", person, PERSON_SHA, "none",
         [])
    # prost 0.14 person + TAT (generated with native prost_path/\
    # prost_types_path pointing at the renamed deps; no rewrite)
    emit(OUT / "prost14_person/example.rs", GEN / "prost14_person.rs",
         "prost-build 0.14.4 + protoc 36.2"
         " (prost_path=::prost14, prost_types_path=::prost_types14)",
         person, PERSON_SHA, "none", [])
    emit(OUT / "prost14_tat/protobuf_test_messages.proto3.rs", GEN / "prost14_tat.rs",
         "prost-build 0.14.4 + protoc 36.2"
         " (prost_path=::prost14, prost_types_path=::prost_types14)",
         tat + f" [{PIN}]", TAT_SHA, "none", [])
    # quick-protobuf person (strip cross-module glob; single-module output)
    qp = (OUT / "qp_person/person.rs").read_text()
    assert "use super::*;" in qp
    qp = qp.replace("use super::*;\n", "")
    qp_lines = [ln for ln in qp.splitlines(keepends=True) if not ln.startswith("#![")]
    qp = "".join(qp_lines)
    head = HEADER.format(generator="pb-rs 0.10.0 (single_module)", source=person,
                         sha=PERSON_SHA,
                         rewrite="dropped `use super::*;` (single-module output needs no parent imports); "
                                 "moved #![allow] inner attrs to the peer_gen wrapper module")
    d = GEN / "qp_person.rs"
    d.write_text(head + qp)
    print(f"staged {d.relative_to(WORKTREE)}")
    # buffa 0.9.2 person + TAT (renamed deps buffa092 / buffa_types092)
    for name in ["person.rs", "person.__view.rs", "person.__lazy_view.rs",
                 "example.mod.rs"]:
        emit(OUT / "buffa092_person" / name, GEN / "buffa092_person" / name,
             "buffa-build 0.9.2 (lazy_views=true, json=false, text=false)",
             person, PERSON_SHA,
             "s/::buffa::/::buffa092::/g, s/::buffa_types::/::buffa_types092::/g",
             [("::buffa_types::", "::buffa_types092::"), ("::buffa::", "::buffa092::")])
    for name in ["google.protobuf.test_messages_proto3.rs",
                 "google.protobuf.test_messages_proto3.__view.rs",
                 "google.protobuf.test_messages_proto3.__lazy_view.rs",
                 "google.protobuf.test_messages_proto3.__oneof.rs",
                 "google.protobuf.test_messages_proto3.__view_oneof.rs",
                 "protobuf_test_messages.proto3.mod.rs"]:
        emit(OUT / "buffa092_tat" / name, GEN / "buffa092_tat" / name,
             "buffa-build 0.9.2 (lazy_views=true, json=false, text=false)",
             tat + f" [{PIN}]", TAT_SHA,
             "s/::buffa::/::buffa092::/g, s/::buffa_types::/::buffa_types092::/g",
             [("::buffa_types::", "::buffa_types092::"), ("::buffa::", "::buffa092::")])
    # google-protobuf 0.36.2 person + TAT (bench `protobuf` IS google-protobuf;
    # gencode `::protobuf::` resolves with no rewrite)
    for name in ["generated.rs", "person.u.pb.rs"]:
        emit(OUT / "gpb36_person" / name, GEN / "gpb36_person" / name,
             "protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)",
             person, PERSON_SHA, "none", [])
    for p in sorted((OUT / "gpb36_tat/google/protobuf").glob("*")):
        emit(p, GEN / "gpb36_tat" / p.name,
             "protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)",
             tat + f" [{PIN}] (+ google/protobuf WKTs)", TAT_SHA, "none", [])

    # sanity: no leftover absolute refs to renamed crates
    bad = []
    for f in GEN.rglob("*.rs"):
        t = f.read_text()
        needles = ["::buffa::", "::buffa_types::", "::prost::", "::prost_types::"]
        if f.name == "prost13_person.rs":
            needles.remove("::prost::")  # bench `prost` IS 0.13: no rewrite there
        for needle in needles:
            if needle in t and "rewrite:" not in t.split(needle, 1)[0][-200:]:
                # allow the header line itself to mention them
                lines = [ln for ln in t.splitlines()
                         if needle in ln and not ln.startswith("//")]
                if lines:
                    bad.append((str(f), needle, lines[0][:100]))
    if bad:
        for f, n, ln in bad[:10]:
            print(f"LEFTOVER {n} in {f}: {ln}")
        sys.exit(1)
    print("rewrite check clean")


main()
