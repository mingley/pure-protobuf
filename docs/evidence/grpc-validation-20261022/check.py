#!/usr/bin/env python3
"""Verify the retained gRPC evidence without extracting the archive."""
import hashlib
import json
from pathlib import Path
import tarfile


def check(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    archive = directory / manifest["archive"]["name"]
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != manifest["archive"]["sha256"]:
        raise ValueError("archive checksum mismatch")
    expected = {entry["path"]: entry for entry in manifest["files"]}
    if len(expected) != len(manifest["files"]):
        raise ValueError("duplicate manifest path")
    seen = set()
    total = 0
    with tarfile.open(archive, "r:gz") as capsule:
        for member in capsule:
            if not member.isfile() or member.name in seen or member.name not in expected:
                raise ValueError(f"unexpected archive member: {member.name}")
            entry = expected[member.name]
            if member.size != entry["bytes"]:
                raise ValueError(f"size mismatch: {member.name}")
            stream = capsule.extractfile(member)
            if stream is None:
                raise ValueError(f"missing content: {member.name}")
            digest = hashlib.sha256()
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
            if digest.hexdigest() != entry["sha256"]:
                raise ValueError(f"file checksum mismatch: {member.name}")
            total += member.size
            seen.add(member.name)
    if seen != set(expected):
        raise ValueError("archive inventory is incomplete")
    if total != manifest["logical_bytes"]:
        raise ValueError("logical byte total mismatch")
    print(f"verified {len(seen)} files, {total} bytes; no production qualification")


if __name__ == "__main__":
    check(Path(__file__).resolve().parent)
