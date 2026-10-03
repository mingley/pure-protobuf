"""GN-specific adapter around immutable root-reviewed RX cache retirement primitives."""
from pathlib import Path
import fcntl
import ctypes
import importlib.util
import json
import os
import stat
import subprocess
import sys
import time

from factor_common import CAP, FLOOR, allocated_bytes, require, sha, utc, write_json

PRIMITIVES_SHA = "e825d1b734154455d2293bf8f6d388e704b029fbca36f377aa890ebe0671667e"
MARGIN = 1 << 24


class Flock(ctypes.Structure):
    _fields_ = [("l_type", ctypes.c_short), ("l_whence", ctypes.c_short),
                ("l_start", ctypes.c_longlong), ("l_len", ctypes.c_longlong), ("l_pid", ctypes.c_int)]


def ofd_abi():
    # Linux UAPI asm-generic/fcntl.h: F_OFD_SETLK=37. Some Python builds omit
    # this exported constant even on an OFD-capable kernel. Support only the
    # verified Linux x86_64 flock layout; the syscall still fails closed.
    require(sys.platform == "linux" and os.uname().machine == "x86_64", "unsupported OFD lock ABI")
    require(ctypes.sizeof(ctypes.c_void_p) == 8 and ctypes.sizeof(Flock) == 32
            and [getattr(Flock, field).offset for field in ("l_type", "l_whence", "l_start", "l_len", "l_pid")]
            == [0, 2, 8, 16, 24], "unexpected Linux x86_64 struct flock layout")
    command = getattr(fcntl, "F_OFD_SETLK", 37)
    require(command == 37, "unexpected Linux OFD command")
    return {"command": command, "constant_source": "python_fcntl" if hasattr(fcntl, "F_OFD_SETLK") else "Linux UAPI asm-generic/fcntl.h F_OFD_SETLK=37",
            "platform": sys.platform, "machine": os.uname().machine, "flock_size": ctypes.sizeof(Flock),
            "flock_offsets": [0, 2, 8, 16, 24]}


def hold_cargo_lock(fd):
    # OFD fcntl ownership survives unrelated hash/archive descriptors closing on the same inode.
    abi = ofd_abi(); value = Flock(fcntl.F_WRLCK, os.SEEK_SET, 0, 0, 0)
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    fcntl.fcntl(fd, abi["command"], bytes(value))
    return abi


def primitives():
    path = Path(__file__).with_name("reviewed-retirement-primitives.py")
    require(sha(path) == PRIMITIVES_SHA, "reviewed retirement primitive bytes changed")
    spec = importlib.util.spec_from_file_location("gn03_reviewed_retirement", path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


class Resources:
    def __init__(self, cache, output):
        self.cache, self.path = Path(cache), Path(output) / "preservation-resources.jsonl"
        self.last, self.samples = 0.0, []

    def check(self, phase, write_bytes=0, force=False):
        v = os.statvfs("/workspace"); free = v.f_bavail * v.f_frsize
        require(free >= FLOOR + write_bytes + (MARGIN if write_bytes else 0), "preservation would breach2GiBfree floor")
        now = time.monotonic()
        if force or now - self.last >= 5.0:
            total = allocated_bytes(self.cache)
            require(total <= CAP, "preservation owned cache exceeded2GiBcap")
            row = {"utc": utc(), "phase": phase, "global_free_bytes": free, "allocated_cache_bytes": total,
                   "cap_bytes": CAP, "floor_bytes": FLOOR,
                   "scope": "single owned namespace, unique inode st_blocks; available filesystem reserve"}
            data = (json.dumps(row) + "\n").encode()
            require(free >= FLOOR + len(data) + MARGIN, "preservation telemetry write would breach floor")
            with self.path.open("ab") as stream:
                stream.write(data)
            self.samples.append(row); self.last = now
        return free

    def write(self, path, value):
        data = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        self.check("evidence_before_write", len(data)); Path(path).write_bytes(data); self.check("evidence_after_write")


def visible_users(cache):
    """Record all accessible witnesses and denials; held locks/owned release add separate protection."""
    r = primitives(); ancestors = r.ancestors(); matches, denials = [], []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit() or int(proc.name) in ancestors:
            continue
        values = {}
        for name in ("stat", "cmdline", "cwd", "exe", "maps", "fd"):
            try:
                if name == "fd":
                    for fd in (proc / name).iterdir():
                        try:
                            values["fd:" + fd.name] = os.readlink(fd)
                        except (FileNotFoundError, ProcessLookupError):
                            pass
                        except PermissionError as exc:
                            denials.append({"pid": int(proc.name), "field": "fd:" + fd.name, "error": str(exc)})
                elif name in ("cwd", "exe"):
                    values[name] = os.readlink(proc / name)
                elif name == "cmdline":
                    values[name] = (proc / name).read_bytes().replace(b"\0", b" ").decode(errors="replace")
                else:
                    values[name] = (proc / name).read_text()
            except (FileNotFoundError, ProcessLookupError):
                pass
            except PermissionError as exc:
                denials.append({"pid": int(proc.name), "field": name, "error": str(exc)})
        fields = values.get("stat", "").rsplit(")", 1)
        if len(fields) == 2 and fields[1].split()[0] == "Z":
            continue
        chosen = [k for k, v in values.items() if str(cache) in v]
        if chosen:
            matches.append({"pid": int(proc.name), "fields": chosen})
    result = {"utc": utc(), "matches": matches, "visibility_denials": denials,
              "scope": "point-in-time accessible proc witnesses; denials retained; root-owned namespace release and held Cargo locks are separate protection"}
    require(not matches, "visible owned-cache users: " + repr(matches))
    return result


def verify_lock_contenders(cache, lock_records, resources, phase):
    """Independent exec observations supplement the continuously held anchor descriptors."""
    r = primitives(); result = []
    script = Path(__file__).with_name("lock-contender.py")
    for lock in lock_records:
        path = Path(cache) / lock["path"]
        require(r.identity(path.lstat()) == lock["identity"], "held Cargo lock path changed")
        for kind in ("posix", "flock", "ofd"):
            resources.check(phase)
            argv = [sys.executable, "-B", str(script), "--path", str(path), "--kind", kind, "--expect", "blocked"]
            child = subprocess.run(argv, capture_output=True, timeout=3, start_new_session=True)
            row = {"utc": utc(), "phase": phase, "argv": argv, "exit_code": child.returncode,
                   "stdout": child.stdout.decode(errors="replace"), "stderr": child.stderr.decode(errors="replace"),
                   "contender_sha256": sha(script, resources)}
            result.append(row)
            require(child.returncode == 0, "held Cargo lock contender acquired/failed: " + repr(row))
            observation = json.loads(row["stdout"])
            require(observation["inode"] == lock["identity"]["st_ino"] and observation["device"] == lock["identity"]["st_dev"],
                    "Cargo contender inspected another inode")
        require(r.identity(path.lstat()) == lock["identity"], "Cargo lock changed during contenders")
    return result


def retire_full_cache(cache, output, owners_idle, source_check, extra_proof=None):
    r = primitives(); cache, out = Path(cache), Path(output)
    require(cache.is_dir() and not cache.is_symlink(), "owned cache missing/symlink")
    require(not out.exists() and not out.is_relative_to(cache), "unsafe or reused preservation output")
    out.mkdir(parents=True)
    resources = Resources(cache, out); resources.check("initial", force=True)
    result = {"status": "preserving_not_retired", "cache": str(cache), "started_utc": utc(),
              "primitive_sha256": PRIMITIVES_SHA, "retired": False}
    locks = []
    try:
        initial = r.scan(cache)
        resources.write(out / "stat-inventory.json", initial)
        owners_idle(); resources.write(out / "users-before-locks.json", visible_users(cache))
        lock_records = []
        for name, row in initial.items():
            if Path(name).name == ".cargo-lock":
                fd = os.open(cache / name, os.O_RDWR | os.O_NOFOLLOW); locks.append(fd)
                require(r.identity(os.fstat(fd)) == row["identity"], "Cargo lock inode changed")
                abi = hold_cargo_lock(fd)
                lock_records.append({"path": name, "identity": row["identity"], "flock": "exclusive held", "fcntl_OFD": "exclusive held through hashing/archive/unlink", "ofd_abi": abi})
        resources.write(out / "held-locks.json", {"locks": lock_records})
        require(r.scan(cache) == initial, "cache changed acquiring Cargo locks")
        owners_idle(); resources.write(out / "users-after-locks.json", visible_users(cache))
        before_source = source_check(resources)
        resources.write(out / "source-before.json", before_source)
        manifest = r.hash_manifest("gn03_full", cache, initial, resources)
        require(all(row["disposition"] == "archived" for row in manifest.values()), "full cache payload omitted")
        resources.write(out / "manifest.json", manifest)
        plain = {name: {"bytes": row["identity"]["st_size"], "sha256": row["sha256"]}
                 for name, row in manifest.items() if row["kind"] == "file"}
        resources.write(out / "all-files.json", plain); resources.write(out / "payloads.json", plain)
        archive = out / "payloads.tar.gz"
        r.add_archive(cache, manifest, archive, resources)
        verified = r.verify_archive(archive, manifest, resources)
        resources.write(out / "archive-verification.json", verified)
        resources.write(out / "contenders-after-archive.json", verify_lock_contenders(cache, lock_records, resources, "after_archive_verification"))
        r.verify_live(cache, manifest, resources)
        after_source = source_check(resources)
        require(before_source == after_source, "source/prepared inputs changed during preservation")
        resources.write(out / "source-after.json", after_source)
        owners_idle(); resources.write(out / "users-immediately-before-retirement.json", visible_users(cache))
        resources.write(out / "fresh-live-verification.json", r.verify_live(cache, manifest, resources))
        resources.check("immediately_before_retirement", force=True)
        resources.write(out / "contenders-immediately-before-retirement.json", verify_lock_contenders(cache, lock_records, resources, "immediately_before_retirement"))
        removed = r.remove_qualified(cache, manifest)  # Held locks + nofollow directory fds and qualified entry inodes.
        resources.write(out / "removed-paths.json", removed)
        require(not cache.exists() and not cache.is_symlink(), "owned root survived retirement")
        require(r.file_sha(archive, resources) == verified["archive_sha256"], "archive changed after retirement")
        result.update(status="verified_retired", retired=True, retired_utc=utc(),
            cache_identity={"device": manifest["."]["identity"]["st_dev"], "inode": manifest["."]["identity"]["st_ino"]},
            archive=str(archive), archive_sha256=verified["archive_sha256"],
            all_files_sha256=r.file_sha(out / "all-files.json", resources), payloads_sha256=r.file_sha(out / "payloads.json", resources),
            manifest_sha256=r.file_sha(out / "manifest.json", resources), inventory_files=len(plain), retained_payloads=len(plain),
            hash_only_cache_files=0, mode_ownership_content_link_topology_pass=True,
            archive_members=verified["verified_members"], held_lock_count=len(locks),
            process_visibility_limit="accessible proc plus recorded denials; no global process-absence claim")
        resources.check("retirement_complete", force=True)
        if extra_proof:
            result.update(extra_proof)
        resources.write(out / "preservation.json", result)
        return result
    except BaseException as exc:
        result.update(status="failed_or_incomplete", error=str(exc), cache_retained=cache.exists())
        # A guard failure must still leave a small truthful failure record; do not start another archive.
        try:
            resources.write(out / "preservation-failure.json", result)
        except ValueError:
            # Parent's already-open raw stderr remains the failure envelope when reserve forbids an evidence write.
            import sys
            print(json.dumps(result), file=sys.stderr)
        raise
    finally:
        for fd in locks:
            os.close(fd)
