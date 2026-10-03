#!/usr/bin/env python3
"""Guarded reclamation of the completed excluded GN03 bootstrap cache only."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import tarfile
import time


ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "target/base"
RESOLVED = CACHE.resolve()
EXPECTED = Path("/workspace/scratch/work/gn11/target/base")
CAMPAIGN = ROOT / "target/codegen-bench/gn03-small-pair-20261002-234019"
OUT = ROOT / "docs/evidence/gn-03-bootstrap-reclaim-artifacts"
assert RESOLVED == EXPECTED and RESOLVED.is_dir()
assert not OUT.exists(), "reclamation evidence already exists"
OUT.mkdir()


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024**2), b""):
            result.update(block)
    return result.hexdigest()


def write(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2) + "\n")


def inside(value, cwd=None):
    value = value.removesuffix(" (deleted)")
    if "\n" in value:
        return False
    candidate = value.rsplit("=", 1)[-1]
    if not candidate.startswith(("/", "./", "../", "target/")):
        return False
    path = Path(candidate)
    if not path.is_absolute():
        if cwd is None:
            return False
        path = cwd / path
    try:
        path.resolve().relative_to(RESOLVED)
        return True
    except (ValueError, OSError):
        return False


def process_guard():
    own = {os.getpid()}
    pid = os.getpid()
    while pid > 1:
        try:
            fields = (Path("/proc") / str(pid) / "stat").read_text().rsplit(")", 1)[1].split()
        except FileNotFoundError:
            break
        pid = int(fields[1])
        own.add(pid)
    matches = []
    compiler_rows = []
    denied = []
    inspected = 0
    for proc in Path("/proc").glob("[0-9]*"):
        try:
            fields = (proc / "stat").read_text().rsplit(")", 1)[1].split()
            if fields[0] == "Z" or int(proc.name) in own:
                continue
            comm = (proc / "comm").read_text().strip()
            row = {"pid": int(proc.name), "comm": comm, "starttime_ticks": int(fields[19])}
            compiler = comm.startswith(("cargo", "rustc", "rustdoc", "clippy", "miri", "sccache", "rust-analyzer"))
            hits = []
            inaccessible = []
            cwd = None
            for name in ["exe", "cwd"]:
                try:
                    value = os.readlink(proc / name)
                    if name == "cwd":
                        cwd = Path(value)
                    if inside(value):
                        hits.append(name)
                except PermissionError:
                    inaccessible.append(name)
            argv = (proc / "cmdline").read_bytes().split(b"\0")
            if any(inside(value.decode(errors="replace"), cwd) for value in argv if value):
                hits.append("argv")
            try:
                environ = (proc / "environ").read_bytes().split(b"\0")
                if any(value.startswith(b"CARGO_TARGET_DIR=") and inside(value.decode(errors="replace"), cwd) for value in environ):
                    hits.append("CARGO_TARGET_DIR")
            except PermissionError:
                inaccessible.append("environ")
            try:
                for fd in (proc / "fd").iterdir():
                    try:
                        if inside(os.readlink(fd)):
                            hits.append("fd")
                            break
                    except (FileNotFoundError, PermissionError):
                        continue
            except PermissionError:
                inaccessible.append("fd")
            try:
                if any(inside(line.split(maxsplit=5)[-1], cwd) for line in (proc / "maps").read_text().splitlines() if len(line.split(maxsplit=5)) == 6):
                    hits.append("maps")
            except PermissionError:
                inaccessible.append("maps")
            if compiler:
                compiler_rows.append(row)
                assert not inaccessible, f"cannot inspect compiler process: {row} {inaccessible}"
            if inaccessible:
                row["inaccessible"] = inaccessible
                denied.append(row)
                assert comm in ["dockerd", "containerd"], f"unrecognized inaccessible process: {row}"
            if hits:
                row["cache_ownership_matches"] = sorted(set(hits))
                matches.append(row)
            inspected += 1
        except (FileNotFoundError, ProcessLookupError):
            continue
    result = {"utc": time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()), "inspected_live_processes": inspected,
              "excluded_own_ancestry": sorted(own), "compiler_processes": compiler_rows,
              "cache_ownership_matches": matches, "known_system_daemon_inspection_limits": denied}
    assert not matches, f"cache still owned by a process: {matches}"
    return result


def inventory():
    result = {}
    unique = {}
    directories = []
    for path in sorted(RESOLVED.rglob("*")):
        info = path.lstat()
        name = path.relative_to(RESOLVED).as_posix()
        row = {"bytes": info.st_size, "allocated_bytes": info.st_blocks * 512, "mtime_ns": info.st_mtime_ns,
               "mode": stat.S_IMODE(info.st_mode), "device": info.st_dev, "inode": info.st_ino, "nlink": info.st_nlink}
        if path.is_symlink():
            row.update({"kind": "symlink", "target": os.readlink(path)})
        elif path.is_file():
            row.update({"kind": "file", "sha256": digest(path)})
        elif path.is_dir():
            directories.append(row)
            continue
        else:
            raise RuntimeError(f"unexpected cache object: {path}")
        result[name] = row
        unique[(info.st_dev, info.st_ino)] = row
    accounting = {"non_directory_paths": len(result), "directories": len(directories),
                  "path_sum_logical_bytes": sum(row["bytes"] for row in result.values()),
                  "path_sum_allocated_bytes": sum(row["allocated_bytes"] for row in result.values()),
                  "unique_inode_logical_bytes": sum(row["bytes"] for row in unique.values()),
                  "unique_inode_allocated_bytes": sum(row["allocated_bytes"] for row in unique.values()),
                  "directory_allocated_bytes": sum(row["allocated_bytes"] for row in directories),
                  "multiple_link_paths": sum(row["nlink"] > 1 for row in result.values())}
    return result, accounting


write("process-guard-before.json", process_guard())
before, accounting = inventory()
write("cache-file-sha256.json", before)
bootstrap = json.loads((CAMPAIGN / "bootstrap.json").read_text())
assert digest(CAMPAIGN / bootstrap["binary"]) == bootstrap["binary_sha256"] == "718aef4131813c1c6fc449e33202588fc6bf48cc330bd28cfd33a96e056f524b"
assert digest(CAMPAIGN / "driver/Cargo.lock") == bootstrap["driver_lock_sha256"]
assert all(digest(ROOT / name) == pin for name, pin in bootstrap["source_sha256"].items())
controls = {"copied_generator": {"path": str(CAMPAIGN / bootstrap["binary"]), "sha256": bootstrap["binary_sha256"]},
            "source_sha256": bootstrap["source_sha256"], "driver_lock_sha256": bootstrap["driver_lock_sha256"],
            "bootstrap_record_sha256": digest(CAMPAIGN / "bootstrap.json"),
            "small_pair_sha256": digest(CAMPAIGN / "pair-small.json"),
            "small_quiet_note_sha256": digest(CAMPAIGN / "quiet-window-notes.json"),
            "100_pair_sha256": digest(CAMPAIGN / "pair-100.json"),
            "1000_failed_pair_sha256": digest(CAMPAIGN / "pair-1000.json"),
            "actual_tool_binaries": json.loads((CAMPAIGN / "pair-small.json").read_text())["actual_tool_binaries"]}
write("unchanged-controls-before.json", controls)
retained = [name for name, row in before.items() if row["kind"] == "file" and ("/.fingerprint/" in "/" + name or name.endswith(".d") or name in [".rustc_info.json", "CACHEDIR.TAG"])]
with tarfile.open(OUT / "fingerprint-and-dependency-contents.tar.gz", "w:gz", compresslevel=6) as archive:
    for name in retained:
        archive.add(RESOLVED / name, arcname=name, recursive=False)
write("retained-content-sha256.json", {name: before[name] for name in retained})
write("process-guard-immediately-before.json", process_guard())
for name, row in before.items():
    info = (RESOLVED / name).lstat()
    assert (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns) == (row["device"], row["inode"], row["bytes"], row["mtime_ns"]), f"cache changed during archival: {name}"
free_before = shutil.disk_usage(ROOT).free
shutil.rmtree(RESOLVED)
RESOLVED.mkdir()
free_after = shutil.disk_usage(ROOT).free
assert CACHE.resolve() == EXPECTED and not list(RESOLVED.iterdir())
assert digest(CAMPAIGN / bootstrap["binary"]) == controls["copied_generator"]["sha256"]
for name, key in [("bootstrap.json", "bootstrap_record_sha256"), ("pair-small.json", "small_pair_sha256"),
                  ("quiet-window-notes.json", "small_quiet_note_sha256"), ("pair-100.json", "100_pair_sha256"), ("pair-1000.json", "1000_failed_pair_sha256")]:
    assert digest(CAMPAIGN / name) == controls[key]
write("reclamation.json", {"status": "completed excluded bootstrap cache reclaimed; original evidence unchanged",
                          "literal_cache_path": str(CACHE), "resolved_cache_path": str(RESOLVED),
                          "accounting_before": accounting, "retained_original_content_files": len(retained),
                          "global_free_before_bytes": free_before, "global_free_after_bytes": free_after,
                          "observed_global_free_delta_bytes": free_after - free_before,
                          "physical_accounting_limit": "unique-inode allocated bytes count cache storage before deletion; global free delta is observed filesystem state and can include concurrent unrelated activity",
                          "remaining_cache_entries": len(list(RESOLVED.iterdir())), "retry": "not_run pending new root quiet lease"})
shutil.copy2(__file__, OUT / "reclaim-gn03-bootstrap.py")
published = {path.name: {"bytes": path.stat().st_size, "sha256": digest(path)} for path in sorted(OUT.iterdir()) if path.is_file()}
write("artifact-sha256.json", published)
print(json.dumps(json.loads((OUT / "reclamation.json").read_text()), indent=2))
