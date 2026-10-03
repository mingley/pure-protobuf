"""Tiny source-recorded POSIX close reproduction and OFD+flock lifetime oracle."""
import argparse
import fcntl
import os
from pathlib import Path
import subprocess
import sys
import tarfile

from factor_common import require, sha, utc, write_json
from retirement_adapter import hold_cargo_lock, ofd_abi


def run_fixture(output):
    out = Path(output); require(not out.exists(), "refusing existing fixture output")
    out.mkdir(parents=True)
    here = Path(__file__).parent
    inputs = [Path(__file__), here / "lock-contender.py", here / "retirement_adapter.py", here / "factor_common.py"]
    before = {str(p): sha(p) for p in inputs}
    headers = [Path("/usr/include/asm-generic/fcntl.h"), Path("/usr/include/x86_64-linux-gnu/bits/fcntl-linux.h")]
    uapi = {}
    for header in headers:
        text = header.read_text()
        import re
        require(re.search(r"#\s*define\s+F_OFD_SETLK\s+37\b", text), "local UAPI command source disagrees")
        uapi[str(header)] = {"sha256": sha(header), "source_definition": "F_OFD_SETLK 37"}
    result = {"schema": "linux-lock-lifetime-fixture/1", "started_utc": utc(), "status": "running",
              "source_before": before, "local_UAPI_sources": uapi, "ofd_abi": ofd_abi(), "observations": []}
    lock = out / ".cargo-lock"; lock.write_bytes(b"tiny-lock-fixture\n")
    def observe(kind, expected, stage):
        argv = [sys.executable, "-B", str(here / "lock-contender.py"), "--path", str(lock), "--kind", kind, "--expect", expected]
        child = subprocess.run(argv, capture_output=True, timeout=3)
        row = {"utc": utc(), "stage": stage, "argv": argv, "exit_code": child.returncode,
               "stdout": child.stdout.decode(), "stderr": child.stderr.decode()}
        result["observations"].append(row)
        require(child.returncode == 0, "independent contender disagrees: " + repr(row))
    try:
        fd = os.open(lock, os.O_RDWR)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            observe("posix", "blocked", "classic_posix_before_other_fd_close")
            sha(lock)  # Opening/closing this OTHER fd releases this process's classic POSIX inode locks.
            observe("posix", "acquired", "classic_posix_after_hash_fd_close")
            observe("ofd", "acquired", "classic_posix_after_hash_fd_close")
            observe("flock", "blocked", "classic_flock_survives_hash_fd_close")
        finally:
            os.close(fd)
        fd = os.open(lock, os.O_RDWR)
        try:
            hold_cargo_lock(fd)
            sha(lock)
            with tarfile.open(out / "tiny.tar.gz", "w:gz") as archive:
                archive.add(lock, arcname=".cargo-lock")
            with tarfile.open(out / "tiny.tar.gz", "r:gz") as archive:
                require(archive.extractfile(".cargo-lock").read() == lock.read_bytes(), "tiny archive content mismatch")
            for kind in ("posix", "flock", "ofd"):
                observe(kind, "blocked", "ofd_flock_after_hash_and_archive_fd_closes")
        finally:
            os.close(fd)
        for kind in ("posix", "flock", "ofd"):
            observe(kind, "acquired", "ofd_flock_after_anchor_fd_close")
        after = {str(p): sha(p) for p in inputs}; require(before == after, "fixture source changed")
        result.update(status="passed", source_after=after, finished_utc=utc(), observations_count=len(result["observations"]))
        return result
    except BaseException as exc:
        result.update(status="failed", error=str(exc), finished_utc=utc()); raise
    finally:
        write_json(out / "record.json", result)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__); p.add_argument("--out", required=True)
    print(run_fixture(p.parse_args().out)["status"])
