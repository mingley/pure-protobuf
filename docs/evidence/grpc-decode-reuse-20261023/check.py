#!/usr/bin/env python3
"""Verify retained gRPC evidence without extracting the archive."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile


def safe_name(name):
    path = PurePosixPath(name)
    return (bool(name) and not path.is_absolute() and str(path) == name
            and not any(part in (".", "..") for part in path.parts)
            and "\\" not in name and ":" not in name)


def digest_file(path):
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


def check(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    if manifest.get("qualified") is not False:
        raise ValueError("unexpected qualification claim")
    archive_name = manifest["archive"]["name"]
    if not safe_name(archive_name) or len(PurePosixPath(archive_name).parts) != 1:
        raise ValueError("unsafe archive name")
    archive = directory / archive_name
    if archive.stat().st_size != manifest["archive"]["bytes"]:
        raise ValueError("archive size mismatch")
    if digest_file(archive) != manifest["archive"]["sha256"]:
        raise ValueError("archive checksum mismatch")
    expected = {entry["path"]: entry for entry in manifest["files"]}
    if len(expected) != len(manifest["files"]) or not all(map(safe_name, expected)):
        raise ValueError("duplicate or unsafe manifest path")
    seen, total = set(), 0
    with tarfile.open(archive, "r:gz") as capsule:
        for member in capsule:
            if (not member.isfile() or not safe_name(member.name)
                    or member.name in seen or member.name not in expected):
                raise ValueError(f"unexpected archive member: {member.name}")
            entry = expected[member.name]
            if member.size != entry["bytes"]:
                raise ValueError(f"size mismatch: {member.name}")
            stream = capsule.extractfile(member)
            if stream is None:
                raise ValueError(f"missing content: {member.name}")
            checksum = hashlib.sha256()
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                checksum.update(block)
            if checksum.hexdigest() != entry["sha256"]:
                raise ValueError(f"file checksum mismatch: {member.name}")
            total += member.size
            seen.add(member.name)
    if seen != set(expected) or total != manifest["logical_bytes"]:
        raise ValueError("archive inventory is incomplete")
    print(f"verified {len(seen)} files, {total} bytes; no production qualification")


if __name__ == "__main__":
    check(Path(__file__).resolve().parent)
