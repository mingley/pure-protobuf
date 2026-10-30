#!/usr/bin/env python3
"""Verify archived inputs and build records; never extract or run a benchmark."""

import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tomllib


HERE = Path(__file__).resolve().parent
MEASUREMENT = "work/all-benchmarks/measurement/"
SYMLINKS = {
    f"rpc-bench/proto/{name}.proto"
    for name in (
        "benchmark_service", "control", "payloads", "services", "stats",
        "worker_service",
    )
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def relative(path):
    # Absolute build-time paths are retained as evidence. Map only the known
    # workspace prefix, never open arbitrary paths named in an archive.
    prefix = "/workspace/pure-protobuf/"
    assert path.startswith(prefix), path
    return path.removeprefix(prefix)


def load_archive():
    manifest = json.loads((HERE / "manifest.json").read_text())
    archive = (HERE / "raw.tar.gz").read_bytes()
    assert sha(archive) == manifest["archive_sha256"], "archive digest"
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        members = tar.getmembers()
        assert len(members) == len(manifest["files"]), "member count"
        assert all(m.isfile() for m in members), "ordinary archive members only"
        assert all(
            not PurePosixPath(m.name).is_absolute()
            and ".." not in PurePosixPath(m.name).parts
            for m in members
        ), "relative archive paths only"
        contents = {m.name: tar.extractfile(m).read() for m in members}
    assert len(contents) == len(members), "duplicate archive member"
    assert set(contents) == set(manifest["files"]), "archive inventory"
    for path, data in contents.items():
        entry = manifest["files"][path]
        assert sha(data) == entry["sha256"] and len(data) == entry["bytes"], path
    return manifest, contents


def reconstruct_sources(manifest, contents):
    def recorded(name):
        return json.loads(contents[MEASUREMENT + name + "-source.json"])["files"]

    base = recorded("round30-map-baseline-source")
    command = ["git", "archive", "--format=tar", manifest["baseline_commit"], "--"]
    repository = subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"], cwd=HERE, text=True
    ).strip()
    data = subprocess.check_output(command + sorted(base), cwd=repository)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as tar:
        members = tar.getmembers()
        symlinks = {m.name for m in members if m.issym()}
        assert symlinks == SYMLINKS, "unreviewed snapshot symlink exception"
        # freeze.py wrote `git show commit:path` bytes into ordinary files,
        # including link-target text. This reproduces that snapshot, not a
        # filesystem checkout in which those six paths would be symlinks.
        source = {
            m.name: tar.extractfile(m).read() if m.isfile() else m.linkname.encode()
            for m in members if m.isfile() or m.issym()
        }
    assert set(source) == set(base), "baseline inventory"
    assert all(sha(source[p]) == h for p, h in base.items()), "baseline bytes"
    sources = {"round30-map-baseline-source": base}
    overlays = (
        ("round30-map-pure-source", "src/map.rs", "work/round30-map/map.rs"),
        ("round30-map-slicemask-source", "src/map.rs", "work/round30-map/slice-mask-map.rs"),
        ("round30-reserve-source", "src/codegen/parse.rs",
         MEASUREMENT + "round30-reserve-source/src/codegen/parse.rs"),
    )
    for name, path, overlay in overlays:
        expected = recorded(name)
        assert set(expected) == set(base), name
        assert {p for p in base if expected[p] != base[p]} == {path}, name
        assert sha(contents[overlay]) == expected[path], overlay
        sources[name] = expected

    # All local code belongs to core plus the two small harnesses. The core
    # build's conformance input is vendored here, not a protobuf source tree.
    assert "vendor/google/conformance_fds.bin" in source
    assert not any(p.startswith("third_party/protobuf/") for p in source)
    reviewed = [data for path, data in source.items()
                if path.startswith("src/") or path == "build.rs"]
    assert not any(b"rpc-bench" in data for data in reviewed)
    return sources


def verify_builds(contents, sources):
    records = sorted(p for p in contents if p.endswith("-build.json")
                     and p.startswith(MEASUREMENT))
    assert len(records) == 11, "three map and eight repeated-codec builds"
    binaries = {}
    for path in records:
        record = json.loads(contents[path])
        directory = str(PurePosixPath(path).parent) + "/"
        source = PurePosixPath(record["source"]).name
        assert record["source_files"] == sources[source], path
        assert record["exit_code"] == 0, path
        assert record["source_unchanged"] and record["harness_unchanged"], path
        harness = str(PurePosixPath(relative(record["manifest"])).parent) + "/"
        for name, digest in record["harness_files"].items():
            assert sha(contents[harness + name]) == digest, harness + name
        assert sha(contents[harness + "Cargo.lock"]) == record["cargo_lock_sha256"], path
        for name, digest in record["logs"].items():
            assert sha(contents[directory + name]) == digest, directory + name
        messages = [json.loads(line) for line in contents[
            path.removesuffix("-build.json") + ".jsonl"
        ].splitlines()]
        artifacts = [m for m in messages if m.get("reason") == "compiler-artifact"]
        executable = [m for m in artifacts if m.get("executable")]
        assert len(executable) == 1 and executable[0]["fresh"] is False, path
        assert executable[0]["executable"] == record["build_executable"], path
        assert executable[0]["features"] == record["features"], path
        local_manifests = {m["manifest_path"] for m in artifacts
                           if m["package_id"].startswith("path+")}
        assert local_manifests == {
            record["manifest"], record["source"] + "/Cargo.toml"
        }, (path, local_manifests)
        assert "rpc-bench" not in contents[harness + "Cargo.toml"].decode(), path
        assert "rpc-bench" not in contents.get(harness + "build.rs", b"").decode(), path
        lock = tomllib.loads(contents[harness + "Cargo.lock"].decode())
        assert all("checksum" in p for p in lock["package"]
                   if p.get("source", "").startswith("registry+")), path
        aggregate = json.loads(contents[directory + "source.json"])
        key = PurePosixPath(record["binary"]).name
        assert aggregate["binaries"][key]["sha256"] == record["binary_sha256"], path
        assert aggregate["binaries"][key]["build_record"] == PurePosixPath(path).name, path
        binaries[relative(record["binary"])] = record["binary_sha256"]
    return binaries


def verify_screen(contents, binaries):
    total = 0
    for profile in ("lazy", "owned"):
        directory = f"work/round30-map-screen/reserve-{profile}-results/"
        run = json.loads(contents[directory + "manifest.json"])
        rows = json.loads(contents[directory + "summary.json"])
        assert run["qualified"] is False and run["state"] == "complete"
        assert run["passed"] is True and run["pairs"] == 5 and run["samples"] == 1
        assert len(run["cases"]) == 11 and len(rows) == 22
        for side in ("before", "after"):
            for mode in ("shipping", "allocations"):
                path = MEASUREMENT + f"round30-reserve-{side}-{profile}/codec-{mode}"
                assert run["binary_sha256"][side + "-" + mode] == binaries[path]
        total += len(rows)
    assert total == 44


def verify_proofs(contents, binaries):
    prefix = "work/round30-reserve-scan/"
    proof = json.loads(contents[prefix + "manifest.json"])
    for path, entry in proof["candidate"].items():
        archive_path = (MEASUREMENT + "round30-reserve-source/" + path
                        if path == "src/codegen/parse.rs"
                        else prefix + "source/" + path)
        assert sha(contents[archive_path]) == entry["after_sha256"], archive_path
    log = proof["proof_log"]
    assert sha(contents[prefix + log["path"]]) == log["sha256"]
    audit = json.loads(contents["work/round30-evidence/input-audit.json"])
    assert audit["retained_executables_verified_before_packaging"] == binaries
    assert set(audit["symlink_exceptions"]) == SYMLINKS
    scalar = prefix + "source-plus-scalars/src/codegen/parse.rs"
    assert sha(contents[scalar]) == audit["scalar_extension_sha256"]


def main():
    manifest, contents = load_archive()
    sources = reconstruct_sources(manifest, contents)
    binaries = verify_builds(contents, sources)
    verify_screen(contents, binaries)
    verify_proofs(contents, binaries)
    print(f"Verified {len(contents)} archived files, 609 recorded baseline inputs, "
          "six unused symlink exceptions, three overlays, 11 builds and 44 diagnostic rows")


if __name__ == "__main__":
    main()
