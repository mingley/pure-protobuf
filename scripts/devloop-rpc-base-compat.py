#!/usr/bin/env python3
"""Apply the documented RPC fixture repair to the exact legacy base harness.

Only a disposable CI base worktree is changed; runtime source is untouched.
Unknown source versions are left as-is and must pass ordinary qualification.
"""
import argparse
import hashlib
import json
from pathlib import Path

LEGACY_SHA256 = "145d8bfea4a2451fa504b2d59199e4afe3e6da19e08572169ef42aeeac67f7d9"
FILE = Path("bench/devloop/adoption/src/rpc.rs")
HELPER = '''// RPC-only fixture compatibility: avoid default map entries whose encoded
// lengths differ between the two codecs. Keep the common wire and byte gate.
fn rpc_maps(entries: usize) -> prost_types::MapHeavy {
    let mut specimen = maps(entries);
    *specimen.counts.get_mut("count-0000").unwrap() = -1;
    let zero_key_value = specimen.entities.remove(&0).unwrap();
    assert!(specimen.entities.insert(4, zero_key_value).is_none());
    specimen
}

'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("record", type=Path)
    parser.add_argument("--head", type=Path, required=True)
    args = parser.parse_args()
    file = args.base / FILE
    original = file.read_bytes()
    before = hashlib.sha256(original).hexdigest()
    head_bytes = (args.head / FILE).read_bytes()
    head_sha256 = hashlib.sha256(head_bytes).hexdigest()
    applied = before == LEGACY_SHA256
    if applied:
        # Do not patch the base if the candidate has changed this fixture.
        # That would make the two workloads different while hiding the change.
        head = head_bytes.decode("utf-8")
        helper_body = HELPER[HELPER.index("fn rpc_maps("):]
        call = "Specimen::Maps(n) => Self::Maps(Pair::prepare(\n                rpc_maps(n),"
        if head.count(helper_body) != 1 or head.count(call) != 1:
            raise ValueError("head RPC fixture differs from the pinned compatibility overlay")
        source = original.decode("utf-8")
        insertion = "impl Inputs {\n"
        old = "Specimen::Maps(n) => Self::Maps(Pair::prepare(\n                maps(n),"
        assert source.count(insertion) == source.count(old) == 1
        source = source.replace(insertion, HELPER + insertion, 1)
        source = source.replace(old, old.replace("maps(n),", "rpc_maps(n),"), 1)
        file.write_text(source)
    after = hashlib.sha256(file.read_bytes()).hexdigest()
    record = {"path": str(FILE), "before_sha256": before, "after_sha256": after,
              "head_sha256": head_sha256,
              "applied": applied, "scope": "RPC map fixture only; base runtime unchanged",
              "unknown_versions": "left unmodified; normal qualification remains mandatory"}
    args.record.parent.mkdir(parents=True, exist_ok=True)
    args.record.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
