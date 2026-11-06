#!/usr/bin/env python3
"""Apply the documented RPC fixture repair to the exact legacy base harness.

Only a disposable CI base worktree is changed; production source is untouched.
The input overlay repairs benchmark identity/length reporting and its guards,
not request values, serialization order or measured RPC loops.
Unknown source versions are left as-is and must pass ordinary qualification.
"""
import argparse
import hashlib
import json
import re
import subprocess
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


# Review each immutable baseline and all five reporting sources together.
LEGACY_INPUT_SOURCES = {'before': {'bench/devloop/adoption/src/rpc.rs': '145d8bfea4a2451fa504b2d59199e4afe3e6da19e08572169ef42aeeac67f7d9',
            'bench/devloop/adoption/src/workloads.rs': '0557880e16913782724ac38831a7fd460cab150da5ade69ef016f581422207cf',
            'bench/devloop/src/adoption_rpc.rs': 'c9497a30742be52c073ce957eb8e4bfa9c2963d354eebceb7fca66cec176e943',
            'bench/devloop/src/main.rs': '257587f1e76de6d8d7c90751a3c75edc76e2ef4df9478d45aaf511171028da21',
            'bench/devloop/src/tonic_transport_rpc.rs': 'b75fb3f8dbb687e2833b7a7f9d099c9e9959ef1da9f571edf27cca6452317dc0'},
 'after': {'bench/devloop/adoption/src/rpc.rs': 'd3ff5cd88766be923daaeb5e8a4e5751c4a6cfa2fe59e96d1beed01231526e8f',
           'bench/devloop/adoption/src/workloads.rs': '140d209d8616b7206e4950c5b52b03da52abbf1c6c4d3e5c517ee946ff08e1c0',
           'bench/devloop/src/adoption_rpc.rs': '420f6ca3b27d12461cf1ce0603eec740f23b959761bc9d3485e5be1c31fcba88',
           'bench/devloop/src/main.rs': '143ea65d81a0b56dd6fe680c523086d6631a6ec501a4f7c9226b13e686ac4a5b',
           'bench/devloop/src/tonic_transport_rpc.rs': '2d64deecb82119f159c6d562fec271b632ab06280e4beb46acba6066ef82b523'}}
CURRENT_INPUT_SOURCES = {'before': {'bench/devloop/adoption/src/rpc.rs': '57809fa5c92d8719c12a22cbd7f668cc22f97f5b1d02348cd77266e01c079daf',
            'bench/devloop/adoption/src/workloads.rs': '0557880e16913782724ac38831a7fd460cab150da5ade69ef016f581422207cf',
            'bench/devloop/src/adoption_rpc.rs': '69171bf38aa8456c59cd9de3f32d05e4c348e8f1befaf34cf45991023e9484d0',
            'bench/devloop/src/main.rs': '257587f1e76de6d8d7c90751a3c75edc76e2ef4df9478d45aaf511171028da21',
            'bench/devloop/src/tonic_transport_rpc.rs': '945cd231c193dc285ec9dd8d9dd191dbb7ac5d365e220e47eb1df49f0cd49e7f'},
 'after': {'bench/devloop/adoption/src/rpc.rs': '7796c60905c5d0986f8bf0337ba9818b92218ee304dab0c977cbf4868e626b8c',
           'bench/devloop/adoption/src/workloads.rs': '140d209d8616b7206e4950c5b52b03da52abbf1c6c4d3e5c517ee946ff08e1c0',
           'bench/devloop/src/adoption_rpc.rs': 'c2f001fdf8e7204b42f4d2e4fe1dbf43b339ec80fd531fa108cbaad6a260736a',
           'bench/devloop/src/main.rs': '143ea65d81a0b56dd6fe680c523086d6631a6ec501a4f7c9226b13e686ac4a5b',
           'bench/devloop/src/tonic_transport_rpc.rs': '9df799c5a0f43869cf9a3a5bf4b3528e560178c714609c9b339995d047aa8b88'}}
KNOWN_INPUT_BASES = {
    "15e738499f0dc937b0e54fd4325271b75ba7e5f4": LEGACY_INPUT_SOURCES,
    "26389ccf77bf9342fe6e605f1d797853acd6c39a": CURRENT_INPUT_SOURCES,
    "4a3e2d9f82edb8f0a94ae0f9f6a9bc500537607d": CURRENT_INPUT_SOURCES,
    "a35701582fb9e6964c855386b0d9c991c50345ba": CURRENT_INPUT_SOURCES,
}
HEAD_RUNTIME_PREFIXES = {'bench/devloop/adoption/src/rpc.rs': '51908d997a073906a530b9f22ba1fe3efbd3c4c87fd0472aebc543d980c5f67a',
 'bench/devloop/adoption/src/workloads.rs': 'ef8ca0fbbaec80044cf96ea4ca8a9285cf781cf3413702f70099d84b4fc53730',
 'bench/devloop/src/adoption_rpc.rs': 'b7e085e8b86ba56cb6e189412149ff2ec07ac438d04be493ee9ed295825e0ad6',
 'bench/devloop/src/main.rs': '3c83d8d08b7d85e1ddec7515f6347269cc776816bca83bcdb93b3719eb0cb08a',
 'bench/devloop/src/tonic_transport_rpc.rs': 'fba35f3a4bd81b3e0c28ba1a2369b36482fdcdc9838b263e368414852188915c'}
INPUT_PATCH = Path(__file__).with_name("devloop-rpc-input-overlay.patch")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_hashes(base, paths):
    return {path: digest((base / path).read_bytes()) for path in paths}


def current_commit(base):
    return subprocess.check_output(
        ["git", "-C", str(base), "rev-parse", "HEAD"], text=True
    ).strip()


def input_plan(base, head, commit):
    if commit is None:
        return {"apply": False, "reason": "no immutable base supplied"}
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or current_commit(base) != commit:
        raise ValueError("base commit differs from --base-sha")
    expected = KNOWN_INPUT_BASES.get(commit)
    if expected is None:
        return {"apply": False, "reason": "unknown base commit; normal qualification remains mandatory"}
    actual = source_hashes(base, expected["before"])
    if actual not in (expected["before"], expected["after"]):
        raise ValueError("base input reporting sources differ from the pinned snapshot")
    for path, digest_expected in HEAD_RUNTIME_PREFIXES.items():
        data = (head / path).read_bytes()
        prefix = data.split(b"\n#[cfg(test)]\nmod ", 1)[0]
        if digest(prefix) != digest_expected:
            raise ValueError("head input reporting differs from the reviewed overlay: " + path)
    return {"apply": actual == expected["before"],
            "reason": "recognized immutable reporting sources" if actual == expected["before"] else "already applied",
            "before": actual, "expected_after": expected["after"],
            "head": source_hashes(head, expected["before"])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("record", type=Path)
    parser.add_argument("--head", type=Path, required=True)
    parser.add_argument("--base-sha", help="full immutable base commit for input-reporting compatibility")
    args = parser.parse_args()
    file = args.base / FILE
    original = file.read_bytes()
    before = digest(original)
    head_bytes = (args.head / FILE).read_bytes()
    head_sha256 = digest(head_bytes)
    fixture_applied = before == LEGACY_SHA256
    if fixture_applied:
        # Keep the previous fixture guard. A changed candidate fixture cannot
        # silently be made comparable by modifying only the baseline.
        head = head_bytes.decode("utf-8")
        helper_body = HELPER[HELPER.index("fn rpc_maps("):]
        call = "Specimen::Maps(n) => Self::Maps(Pair::prepare(\n                rpc_maps(n),"
        if head.count(helper_body) != 1 or head.count(call) != 1:
            raise ValueError("head RPC fixture differs from the pinned compatibility overlay")
    # Validate every candidate source before any mutation. git apply is atomic
    # across the reviewed patch; an unknown base is never partially patched.
    plan = input_plan(args.base, args.head, args.base_sha)
    reporting = {"applied": plan["apply"], "reason": plan["reason"],
                 "base_commit": args.base_sha, "files": [],
                 "scope": "prepared benchmark input identity/length reporting and guards only; measured loops unchanged"}
    if plan["apply"]:
        patch = INPUT_PATCH.resolve()
        stats = subprocess.check_output(["git", "apply", "--numstat", str(patch)], text=True)
        paths = [line.split("\t", 2)[2] for line in stats.splitlines()]
        if set(paths) != set(HEAD_RUNTIME_PREFIXES) or len(paths) != len(set(paths)):
            raise ValueError("input reporting patch changes an unexpected file")
        subprocess.run(["git", "apply", "--check", str(patch)], cwd=args.base, check=True)
        subprocess.run(["git", "apply", str(patch)], cwd=args.base, check=True)
        reporting["patch_sha256"] = digest(patch.read_bytes())
    if fixture_applied:
        source = file.read_text()
        insertion = "impl Inputs {\n"
        old = "Specimen::Maps(n) => Self::Maps(Pair::prepare(\n                maps(n),"
        assert source.count(insertion) == source.count(old) == 1
        source = source.replace(insertion, HELPER + insertion, 1)
        source = source.replace(old, old.replace("maps(n),", "rpc_maps(n),"), 1)
        file.write_text(source)
    if plan.get("before") is not None:
        after = source_hashes(args.base, plan["before"])
        if after != plan["expected_after"]:
            raise ValueError("input reporting overlay produced unexpected source bytes")
        reporting["files"] = [
            {"path": path, "before_sha256": value, "after_sha256": after[path],
             "head_sha256": plan["head"][path]}
            for path, value in plan["before"].items()
        ]
    after = digest(file.read_bytes())
    record = {"path": str(FILE), "before_sha256": before, "after_sha256": after,
              "head_sha256": head_sha256,
              "applied": fixture_applied or reporting["applied"],
              "fixture_applied": fixture_applied, "input_reporting": reporting,
              "scope": "benchmark RPC fixture and prepared-input reporting only; production source unchanged",
              "unknown_versions": "left unmodified; normal qualification remains mandatory"}
    args.record.parent.mkdir(parents=True, exist_ok=True)
    args.record.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
