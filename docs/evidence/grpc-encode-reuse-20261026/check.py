#!/usr/bin/env python3
"""Verify this experiment capsule without extracting archive members."""
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
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError("expected a regular local file: " + str(path))


def check(directory):
    manifest_path = directory / "manifest.json"
    regular(manifest_path)
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("qualified") is not False or manifest.get("production_ready") is not False:
        raise ValueError("unexpected qualification claim")
    name = manifest["archive"]["name"]
    if not safe_name(name) or len(PurePosixPath(name).parts) != 1:
        raise ValueError("unsafe archive name")
    archive = directory / name
    regular(archive)
    if archive.stat().st_size != manifest["archive"]["bytes"] or digest(archive) != manifest["archive"]["sha256"]:
        raise ValueError("archive size or checksum mismatch")
    expected = {row["path"]: row for row in manifest["files"]}
    if len(expected) != len(manifest["files"]) or not all(map(safe_name, expected)):
        raise ValueError("duplicate or unsafe manifest path")
    publications = manifest["publication_files"]
    if len(publications) != 3 or {row["path"] for row in publications} != {"README.md", "results.json", "check.py"}:
        raise ValueError("incomplete publication inventory")
    for row in publications:
        local = directory / row["path"]
        regular(local)
        if local.stat().st_size != row["bytes"] or digest(local) != row["sha256"]:
            raise ValueError("publication checksum mismatch: " + row["path"])
    seen, total = set(), 0
    with tarfile.open(archive, "r:gz") as capsule:
        for member in capsule:
            if not member.isfile() or not safe_name(member.name) or member.name in seen or member.name not in expected:
                raise ValueError("unexpected member: " + member.name)
            row = expected[member.name]
            if member.size != row["bytes"]:
                raise ValueError("member size mismatch: " + member.name)
            stream = capsule.extractfile(member)
            if stream is None:
                raise ValueError("missing member content")
            value = hashlib.sha256()
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                value.update(chunk)
            if value.hexdigest() != row["sha256"]:
                raise ValueError("member checksum mismatch: " + member.name)
            seen.add(member.name)
            total += member.size
    if seen != set(expected) or total != manifest["logical_bytes"]:
        raise ValueError("incomplete archive inventory")
    print(f"verified {len(seen)} files, {total} bytes; experiment only, no production qualification")


if __name__ == "__main__":
    check(Path(__file__).resolve().parent)
