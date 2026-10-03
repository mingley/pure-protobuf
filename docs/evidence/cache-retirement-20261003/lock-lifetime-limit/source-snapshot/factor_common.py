"""Work-only GN03 factor orchestration; never replaces the frozen measurement API."""
from __future__ import annotations

import hashlib
from contextlib import contextmanager
import errno
import importlib.util
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import tarfile
import threading
import time
from datetime import datetime, timezone

GIB = 1 << 30
CAP = FLOOR = 2 * GIB
FACTORS = {"bdef": ("base", "default"), "cdef": ("cand", "default"),
           "bshr": ("base", "shared"), "cshr": ("cand", "shared")}
SOURCES = {"base": "4b2384e728bce17601d56e43a887c354d66b9024",
           "cand": "5fe226b015a2c2bc667db976390b8337db6e9a2e"}
PHASES = ("generation", "generation_unchanged", "check_clean",
          "check_incremental", "build_release")
ALL_PHASES = ("consumer_lock", *PHASES, "release_smoke")
HARNESS_SHA = "cf4cb09d684ca8a976a00ae9ee2ad6ffd05d00326981bb4b71145cda6232cbcd"
DRIVER_SHA = "79e5383d967161094303f04f1f3925d14d74812e6a0c15147057c78d24bad796"
EXECUTION_COMPONENTS = ("coordinate-gn03-factors.py", "factor_common.py", "audit-gn03-factors.py",
                        "retirement_adapter.py", "reviewed-retirement-primitives.py", "lock-contender.py", "prepare-factor-inputs.py")
ENV_KEYS = ("PATH", "CARGO_HOME", "RUSTUP_HOME", "RUSTC", "RUSTDOC", "PROTOC",
            "CARGO_NET_OFFLINE", "CARGO_BUILD_JOBS", "CARGO_TERM_COLOR", "CARGO_TARGET_DIR",
            "CARGO_INCREMENTAL", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER", "CC", "CXX", "AR", "LD", "LD_LIBRARY_PATH", "LC_ALL")
UNSUPPORTED_ENV = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS",
                   "CARGO_BUILD_RUSTFLAGS", "CARGO_BUILD_RUSTDOCFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
                   "RUSTC_BOOTSTRAP", "CARGO_BUILD_TARGET", "RUSTC_LINKER", "CC", "CXX", "AR", "LD",
                   "TARGET_CC", "TARGET_CXX", "TARGET_AR", "HOST_CC", "HOST_CXX", "HOST_AR")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def utc():
    return datetime.now(timezone.utc).isoformat()


def digest_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha(path, resources=None):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for part in iter(lambda: f.read(1 << 20), b""):
            h.update(part)
            if resources:
                resources.check("hashing_provenance_input")
    return h.hexdigest()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    temporary.replace(path)


def load_json(path):
    return json.loads(Path(path).read_text())


def component_pins():
    return {name: sha(Path(__file__).with_name(name)) for name in EXECUTION_COMPONENTS}


def ordinary_file(path):
    p = Path(path)
    require(p.is_file() and not p.is_symlink(), "not an ordinary file: " + str(p))
    return p


def safe_relative(value):
    p = Path(value)
    require(not p.is_absolute() and value and all(x not in ("", ".", "..") for x in p.parts),
            "unsafe relative path: " + value)
    return p


def source_pins(root, expected, resources=None):
    root = Path(root)
    require(root.is_dir() and not root.is_symlink(), "source root missing or symlink")
    actual = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file() or p.is_symlink()}
    require(actual == set(expected), "source file set changed: " + str(root))
    for name, value in expected.items():
        require(sha(ordinary_file(root / safe_relative(name)), resources) == value, "source pin changed: " + name)
    return {"root": str(root), "files": len(expected), "inventory_sha256": digest_bytes(
        json.dumps(expected, sort_keys=True, separators=(",", ":")).encode())}


def source_modes(root, expected):
    require(set(expected) and all(v in ("100644", "100755") for v in expected.values()), "unsupported shipping source modes")
    for name, mode in expected.items():
        require(stat.S_IMODE(ordinary_file(Path(root) / safe_relative(name)).stat().st_mode)
                == (0o555 if mode == "100755" else 0o444), "read-only shipping source mode changed: " + name)
    return expected


def load_harness(root):
    path = Path(root) / "bench/codegen/run.py"
    require(sha(path) == HARNESS_SHA, "frozen harness changed")
    require(sha(path.with_name("generator.rs")) == DRIVER_SHA, "frozen driver changed")
    spec = importlib.util.spec_from_file_location("gn03_frozen_measurement", path)
    h = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(h)
    require(h.ROOT == Path(root), "frozen helper ROOT changed")
    return h


def effective_env(env):
    # Compiler inputs only: do not copy unrelated credential values into evidence.
    names = set(ENV_KEYS) | set(UNSUPPORTED_ENV) | {k for k in env if k.startswith(("SB09_PBRS_", "CARGO_PROFILE_", "CARGO_TARGET_", "CC_", "CXX_", "AR_", "LD_"))}
    return {k: env.get(k) for k in sorted(names)}


def clean_env(tools, profile):
    require(profile in ("default", "shared"), "unknown existing profile")
    env = os.environ.copy()
    cleared = sorted(k for k in env if k.startswith(("SB09_PBRS_", "PURE_PROTOBUF_")))
    for k in cleared:
        del env[k]
    require(not any(env.get(k) for k in UNSUPPORTED_ENV), "unexpected compiler flags/wrapper/target/tool override")
    require(not any(v for k, v in env.items() if k.startswith(("CARGO_TARGET_", "CC_", "CXX_", "AR_", "LD_"))
                    and k != "CARGO_TARGET_DIR"), "unexpected target/compiler-specific override")
    require(not any(k.startswith("CARGO_PROFILE_") for k in env), "unexpected Cargo profile override")
    env.pop("CARGO_INCREMENTAL", None)
    env.pop("CARGO_TARGET_DIR", None)
    env.update({"CARGO_NET_OFFLINE": "true", "CARGO_BUILD_JOBS": "1", "CARGO_TERM_COLOR": "never",
                "LC_ALL": "C", "RUSTC": tools["rustc"]["path"], "PROTOC": tools["protoc"]["path"]})
    if "rustdoc" in tools:
        env["RUSTDOC"] = tools["rustdoc"]["path"]
    if profile == "shared":
        env["SB09_PBRS_SHARED_DESCRIPTOR_SET"] = "1"
    return env, cleared


@contextmanager
def clean_parent_codegen_env():
    """Original prepare_corpus reads runtime profile from this isolated parent's environment."""
    saved = {k: v for k, v in os.environ.items() if k.startswith(("SB09_PBRS_", "PURE_PROTOBUF_"))}
    for key in saved:
        del os.environ[key]
    try:
        yield
    finally:
        for key in list(os.environ):
            if key.startswith(("SB09_PBRS_", "PURE_PROTOBUF_")):
                del os.environ[key]
        os.environ.update(saved)


def verify_tools(tools):
    require(all(k in tools for k in ("cargo", "rustc", "protoc", "time", "ps", "python")),
            "tool envelope is incomplete")
    for name, info in tools.items():
        require(isinstance(info, dict) and info.get("path") and info.get("sha256"), "bad tool pin: " + name)
        require(sha(info["path"]) == info["sha256"], "tool bytes changed: " + name)
        require(str(Path(info["path"]).resolve()) == info["resolved_path"], "tool target changed: " + name)
    require(tools["time"]["path"] == "/usr/bin/time", "original time path changed")
    return tools


def read_proc(pid):
    try:
        raw = Path(f"/proc/{pid}/stat").read_text()
    except OSError as exc:
        if exc.errno in (errno.ENOENT, errno.ESRCH):
            return None
        raise
    fields = raw.rsplit(")", 1)[1].split()
    return {"pid": int(pid), "state": fields[0], "ppid": int(fields[1]),
            "pgid": int(fields[2]), "sid": int(fields[3]), "start_ticks": int(fields[19])}


def live_group(owner):
    require(owner.get("anchor") == "returned_Popen_new_session" and
            owner.get("anchor_stat") == {k: owner[k] for k in ("pid", "pgid", "sid", "start_ticks")},
            "refusing unanchored process group")
    leader = read_proc(owner["pid"])
    if leader is not None:
        require(leader["start_ticks"] == owner["start_ticks"] and leader["pgid"] == owner["pgid"]
                and leader["sid"] == owner["sid"], "owned PID/group identity changed")
    rows = []
    for p in Path("/proc").iterdir():
        if p.name.isdigit():
            row = read_proc(int(p.name))
            if row and row["state"] != "Z" and row["pgid"] == owner["pgid"]:
                require(row["sid"] == owner["sid"] and row["start_ticks"] >= owner["start_ticks"],
                        "group contains unqualified ownership")
                rows.append(row)
    return rows


class ObservedProcess:
    def __init__(self, proc, owner, observer):
        self._proc, self._owner, self._observer = proc, owner, observer

    def __getattr__(self, name):
        return getattr(self._proc, name)

    def wait(self, *args, **kwargs):
        code = self._proc.wait(*args, **kwargs)
        self._observer.finished(self._owner, code)
        return code


class ProcessObserver:
    """A module-local facade: stdlib subprocess and its global Popen stay untouched."""
    def __init__(self, sidecar):
        self.sidecar = Path(sidecar)
        self.owners = []
        self.processes = []
        self.lock = threading.RLock()
        self.phase = "unlabeled"
        self.blocked = threading.Event()
        self.save()

    def __getattr__(self, name):
        return getattr(subprocess, name)

    def save(self):
        with self.lock:
            write_json(self.sidecar, {"schema": "gn03-process-observer/1", "owners": self.owners,
                                    "scope": "observed actual phase sessions; accessible /proc only"})

    def Popen(self, *args, **kwargs):
        require(not self.blocked.is_set(), "refusing new phase after guard/cancellation")
        require(kwargs.get("start_new_session") is True, "phase lost new-session ownership")
        proc = subprocess.Popen(*args, **kwargs)
        try:
            row = read_proc(proc.pid)
            require(row and row["pgid"] == proc.pid and row["sid"] == proc.pid,
                    "new session could not be verified")
            output = getattr(kwargs.get("stdout"), "name", None)
            phase = self.phase if not output else Path(str(output)).name.removesuffix(".stdout.log").replace("-", "_")
            owner = {**row, "phase": phase, "started_utc": utc(), "argv": args[0],
                     "stdout_path": str(output), "stderr_path": str(getattr(kwargs.get("stderr"), "name", None)),
                     "cwd": str(kwargs.get("cwd")), "environment": effective_env(kwargs.get("env", {})),
                     "exit_code": None}
            owner["anchor"] = "returned_Popen_new_session"
            owner["anchor_stat"] = {k: row[k] for k in ("pid", "pgid", "sid", "start_ticks")}
            with self.lock:
                self.owners.append(owner)
                self.processes.append((proc, owner))
                self.save()
        except BaseException:
            # This exact Popen is ours, even if group registration failed. Do not signal an unknown group.
            proc.kill()
            proc.wait()
            raise
        return ObservedProcess(proc, owner, self)

    def finished(self, owner, code):
        with self.lock:
            if owner["exit_code"] is None:
                owner.update(exit_code=code, finished_utc=utc())
                self.save()

    def cleanup(self, grace=1.0):
        with self.lock:
            owners = list(self.owners)
        for owner in owners:
            if live_group(owner):
                try:
                    os.killpg(owner["pgid"], signal.SIGTERM)
                except ProcessLookupError:
                    pass
        end = time.monotonic() + grace
        while time.monotonic() < end:
            if not any(live_group(o) for o in owners):
                break
            time.sleep(0.02)
        for owner in owners:
            if live_group(owner):
                try:
                    os.killpg(owner["pgid"], signal.SIGKILL)
                except ProcessLookupError:
                    pass
        end = time.monotonic() + grace
        while any(live_group(o) for o in owners) and time.monotonic() < end:
            time.sleep(0.02)
        require(not any(live_group(o) for o in owners), "owned phase group survived cleanup")
        for proc, owner in self.processes:
            self.finished(owner, proc.wait(timeout=grace))
        self.save()


def allocated_bytes(root):
    root = Path(root)
    if not root.exists():
        return 0
    require(not root.is_symlink(), "cache root is a symlink")
    seen, total = set(), 0
    for p in [root, *root.rglob("*")]:
        try:
            s = p.lstat()
        except FileNotFoundError:
            continue  # Atomic Cargo rename/delete during a sampled observation.
        if (s.st_dev, s.st_ino) not in seen:
            seen.add((s.st_dev, s.st_ino))
            total += s.st_blocks * 512
    return total


class ResourceGuard:
    def __init__(self, cache, filesystem, observer, record, interval=5.0, sample=None):
        self.cache, self.filesystem = Path(cache), Path(filesystem)
        self.observer, self.record = observer, Path(record)
        self.interval, self.sample_override = interval, sample
        self.samples, self.failures = [], []
        self.stop = threading.Event()
        self.thread = None
        self.lock = threading.RLock()

    def check(self):
        if self.sample_override:
            cache, free = self.sample_override()
        else:
            cache = allocated_bytes(self.cache)
            v = os.statvfs(self.filesystem)
            free = v.f_bavail * v.f_frsize
        sample = {"utc": utc(), "allocated_cache_bytes": cache, "global_free_bytes": free}
        self.samples.append(sample)
        with self.lock:
            write_json(self.record, {"samples": self.samples, "failures": self.failures,
                "cap_bytes": CAP, "floor_bytes": FLOOR, "sample_interval_seconds": self.interval,
                "scope": "sampled st_blocks unique-inode paths and available filesystem reserve"})
        require(cache <= CAP, "owned cache exceeded 2GiB cap")
        require(free >= FLOOR, "global free below 2GiB floor")

    def __enter__(self):
        self.check()
        def watch():
            while not self.stop.wait(self.interval):
                try:
                    self.check()
                except BaseException as exc:
                    self.failures.append(str(exc))
                    self.observer.blocked.set()
                    try:
                        self.observer.cleanup()
                    except BaseException as cleanup:
                        self.failures.append("cleanup: " + str(cleanup))
                    self.stop.set()
                    with self.lock:
                        write_json(self.record, {"samples": self.samples, "failures": self.failures,
                            "cap_bytes": CAP, "floor_bytes": FLOOR, "sample_interval_seconds": self.interval})
        self.thread = threading.Thread(target=watch, daemon=True)
        self.thread.start()
        return self

    def assert_ok(self):
        require(not self.failures, "resource guard failed: " + repr(self.failures))
        self.check()

    def __exit__(self, kind, value, trace):
        self.stop.set()
        self.thread.join()
        if kind:
            self.observer.cleanup()
        if not kind:
            self.assert_ok()


def inventory(root):
    root = Path(root)
    require(root.is_dir() and not root.is_symlink(), "inventory root missing/symlink")
    result = {}
    for p in sorted(root.rglob("*")):
        require(not p.is_symlink(), "payload symlink: " + str(p))
        if p.is_file():
            s = p.stat()
            result[p.relative_to(root).as_posix()] = {"bytes": s.st_size, "sha256": sha(p)}
    return result
