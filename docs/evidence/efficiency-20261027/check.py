#!/usr/bin/env python3
"""Check the retained result and every archive member without extraction."""

import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def safe_name(name: str) -> bool:
    path = PurePosixPath(name)
    return (
        bool(name)
        and not path.is_absolute()
        and str(path) == name
        and not any(part in (".", "..") for part in path.parts)
        and "\\" not in name
        and ":" not in name
    )


root = Path(__file__).resolve().parent
manifest = json.loads((root / "manifest.json").read_text())
require(manifest["schema"] == "pbrs.evidence-manifest.v1", "Unknown manifest schema")
require(digest((root / "results.json").read_bytes()) == manifest["results_sha256"], "Results checksum mismatch")
require(digest((root / "raw.tar.gz").read_bytes()) == manifest["archive_sha256"], "Archive checksum mismatch")
expected = manifest["files"]
require(all(map(safe_name, expected)), "Unsafe manifest path")
seen = set()
total = 0
with tarfile.open(root / "raw.tar.gz", "r:gz") as archive:
    for member in archive:
        require(member.isfile() and safe_name(member.name), "Unsafe archive member")
        require(member.name not in seen and member.name in expected, "Unexpected archive member")
        require(member.size == expected[member.name]["bytes"], "Member size mismatch")
        seen.add(member.name)
        file = archive.extractfile(member)
        require(file is not None, "Missing member content")
        value = hashlib.sha256()
        first = True
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            if first:
                require(not chunk.startswith(b"\x7fELF"), "Executable in archive")
                first = False
            value.update(chunk)
        require(value.hexdigest() == expected[member.name]["sha256"], "Member checksum mismatch")
        total += member.size
require(seen == set(expected), "Incomplete archive inventory")
print(f"Verified {len(seen)} files and {total} logical bytes")
