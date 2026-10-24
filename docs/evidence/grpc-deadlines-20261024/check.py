#!/usr/bin/env python3
"""Verify this evidence capsule without extracting it."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile


def safe_name(name):
    path = PurePosixPath(name)
    return (bool(name) and not path.is_absolute() and str(path) == name
            and not any(part in (".", "..") for part in path.parts)
            and "\\" not in name and ":" not in name)


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for data in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(data)
    return result.hexdigest()


def check(directory):
    manifest_path = directory / "manifest.json"
    if manifest_path.is_symlink() or not manifest_path.is_file():
        raise ValueError("manifest must be a regular local file")
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("qualified") is not False:
        raise ValueError("unexpected qualification claim")
    archive_name = manifest["archive"]["name"]
    if not safe_name(archive_name) or len(PurePosixPath(archive_name).parts) != 1:
        raise ValueError("unsafe archive name")
    archive = directory / archive_name
    if archive.is_symlink() or not archive.is_file():
        raise ValueError("archive must be a regular local file")
    if archive.stat().st_size != manifest["archive"]["bytes"] or digest(archive) != manifest["archive"]["sha256"]:
        raise ValueError("archive size or checksum mismatch")
    expected = {row["path"]: row for row in manifest["files"]}
    if len(expected) != len(manifest["files"]) or not all(map(safe_name, expected)):
        raise ValueError("duplicate or unsafe manifest path")
    seen, total = set(), 0
    with tarfile.open(archive, "r:gz") as capsule:
        for member in capsule:
            if not member.isfile() or not safe_name(member.name) or member.name in seen or member.name not in expected:
                raise ValueError("unexpected member: " + member.name)
            row = expected[member.name]
            if row["bytes"] != member.size:
                raise ValueError("size mismatch: " + member.name)
            stream = capsule.extractfile(member)
            if stream is None:
                raise ValueError("missing content: " + member.name)
            h = hashlib.sha256()
            for data in iter(lambda: stream.read(1024 * 1024), b""):
                h.update(data)
            if h.hexdigest() != row["sha256"]:
                raise ValueError("checksum mismatch: " + member.name)
            seen.add(member.name)
            total += member.size
    if seen != set(expected) or total != manifest["logical_bytes"]:
        raise ValueError("incomplete archive inventory")
    print(f"verified {len(seen)} files, {total} bytes; no production qualification")


if __name__ == "__main__":
    check(Path(__file__).resolve().parent)
