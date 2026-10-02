#!/usr/bin/env python3
"""Orchestrator for separating benchmark client and server processes.

Complies with docs/benchmark-contract.md and pure-protobuf task BM-04:
- Spawns server in an independent OS process with dynamic or explicit port.
- Waits for stdout readiness reporting (`READY port=<PORT> addr=<ADDR>`).
- Spawns client in an independent OS process pointing to the server endpoint.
- Captures benchmark metrics into BenchmarkReport.
- Ensures clean SIGTERM/SIGINT teardown of all child processes on exit or failure.
- Reports failures and exits non-zero if child processes crash.
"""

import argparse
import atexit
import ctypes
import json
import math
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import threading
import time
from typing import Any, Dict, List, Optional, Set, Tuple

try:
    import resource
except ImportError:
    resource = None

# Track all active subprocesses to guarantee cleanup on exit or signal
_ACTIVE_PROCESSES: Set[subprocess.Popen] = set()

# ---------------------------------------------------------------------------
# Platform-specific process resource inspection (macOS & Linux)
# ---------------------------------------------------------------------------

class _MachTimebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]

class _ProcTaskInfo(ctypes.Structure):
    _fields_ = [
        ("pti_virtual_size", ctypes.c_uint64),
        ("pti_resident_size", ctypes.c_uint64),
        ("pti_total_user", ctypes.c_uint64),
        ("pti_total_system", ctypes.c_uint64),
        ("pti_threads_user", ctypes.c_uint64),
        ("pti_threads_system", ctypes.c_uint64),
        ("pti_policy", ctypes.c_int32),
        ("pti_faults", ctypes.c_int32),
        ("pti_pageins", ctypes.c_int32),
        ("pti_cow_faults", ctypes.c_int32),
        ("pti_messages_sent", ctypes.c_int32),
        ("pti_messages_received", ctypes.c_int32),
        ("pti_syscalls_mach", ctypes.c_int32),
        ("pti_syscalls_unix", ctypes.c_int32),
        ("pti_csw", ctypes.c_int32),
        ("pti_threadnum", ctypes.c_int32),
        ("pti_numrunning", ctypes.c_int32),
        ("pti_priority", ctypes.c_int32),
    ]

_HAS_MACOS_LIBPROC = False
_mach_tb: Optional[Tuple[int, int]] = None
_libproc = None

if sys.platform == "darwin":
    try:
        _libc = ctypes.CDLL(None)
        _libproc_candidate = ctypes.CDLL("/usr/lib/libproc.dylib")
        _tb_inst = _MachTimebase()
        if _libc.mach_timebase_info(ctypes.byref(_tb_inst)) == 0:
            _mach_tb = (_tb_inst.numer, _tb_inst.denom)
            _libproc = _libproc_candidate
            _HAS_MACOS_LIBPROC = True
    except Exception:
        pass


class ProcessSnapshot:
    """Instantaneous snapshot of a process's resource usage."""

    def __init__(
        self,
        user_s: float,
        sys_s: float,
        rss_bytes: int,
        thread_count: int,
        timestamp: float,
        method: str,
    ):
        self.user_s = user_s
        self.sys_s = sys_s
        self.rss_bytes = rss_bytes
        self.thread_count = thread_count
        self.timestamp = timestamp
        self.method = method

    @property
    def total_cpu_s(self) -> float:
        return self.user_s + self.sys_s


def sample_process(pid: int) -> Optional[ProcessSnapshot]:
    """Capture resource metrics for a process by PID using native OS APIs."""
    now = time.monotonic()

    # 1. macOS native Mach task_info via libproc
    if _HAS_MACOS_LIBPROC and _libproc is not None and _mach_tb is not None:
        info = _ProcTaskInfo()
        ret = _libproc.proc_pidinfo(pid, 4, 0, ctypes.byref(info), ctypes.sizeof(info))
        if ret == ctypes.sizeof(info):
            numer, denom = _mach_tb
            user_s = (info.pti_total_user * numer // denom) / 1e9
            sys_s = (info.pti_total_system * numer // denom) / 1e9
            return ProcessSnapshot(
                user_s=user_s,
                sys_s=sys_s,
                rss_bytes=int(info.pti_resident_size),
                thread_count=int(info.pti_threadnum),
                timestamp=now,
                method="macos-mach",
            )

    # 2. Linux /proc/{pid}/stat and /proc/{pid}/status
    if sys.platform.startswith("linux"):
        try:
            stat_path = f"/proc/{pid}/stat"
            with open(stat_path, "r", encoding="utf-8") as f:
                content = f.read()
            rparen = content.rfind(")")
            if rparen != -1:
                fields = content[rparen + 2 :].split()
                utime_ticks = float(fields[11])
                stime_ticks = float(fields[12])
                num_threads = int(fields[17])
                rss_pages = int(fields[21])
                clk_tck = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else 100.0
                page_size = os.sysconf("SC_PAGE_SIZE") if hasattr(os, "sysconf") else 4096
                rss_bytes = rss_pages * page_size

                try:
                    with open(f"/proc/{pid}/status", "r", encoding="utf-8") as f_status:
                        for line in f_status:
                            if line.startswith("VmRSS:"):
                                parts = line.split()
                                if len(parts) >= 2:
                                    rss_bytes = int(parts[1]) * 1024
                            elif line.startswith("Threads:"):
                                parts = line.split()
                                if len(parts) >= 2:
                                    num_threads = int(parts[1])
                except Exception:
                    pass

                return ProcessSnapshot(
                    user_s=utime_ticks / clk_tck,
                    sys_s=stime_ticks / clk_tck,
                    rss_bytes=rss_bytes,
                    thread_count=num_threads,
                    timestamp=now,
                    method="linux-procfs",
                )
        except Exception:
            pass

    # 3. Best-effort POSIX ps fallback
    try:
        out = subprocess.check_output(
            ["ps", "-o", "rss,cputime", "-p", str(pid)],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip().splitlines()
        if len(out) >= 2:
            parts = out[1].split()
            rss_kb = int(parts[0]) if len(parts) >= 1 else 0
            cpu_parts = parts[1].split(":") if len(parts) >= 2 else []
            total_s = 0.0
            if len(cpu_parts) == 3:
                total_s = float(cpu_parts[0]) * 3600 + float(cpu_parts[1]) * 60 + float(cpu_parts[2])
            elif len(cpu_parts) == 2:
                total_s = float(cpu_parts[0]) * 60 + float(cpu_parts[1])
            return ProcessSnapshot(
                user_s=total_s * 0.75,
                sys_s=total_s * 0.25,
                rss_bytes=rss_kb * 1024,
                thread_count=1,
                timestamp=now,
                method="ps-fallback",
            )
    except Exception:
        pass

    return None


def collect_process_cpu_capacity(pid: int, proc_root: Path = Path("/proc")) -> Dict[str, Any]:
    """Prove a PID's CPU budget; unknown mappings never imply headroom.

    Affinity is the union of the live tasks' enforced masks. A genuine kernel
    hierarchy root must be established before visible quotas can certify the
    full ancestor minimum. Namespace roots can hide stricter parent quotas.
    """
    proof: Dict[str, Any] = {
        "pid": pid,
        "verified": False, "effective_cpu_capacity": None,
        "affinity_cpus": None, "affinity_cpu_count": None,
        "quota_cpu_capacity": None, "quota_samples": [],
        "visible_cpu_capacity": None,
        "method": "unsupported",
        "scope": "enforced affinity and proven genuine-root CPU quota hierarchy",
        "reason": "unsupported platform",
    }
    if sys.platform != "linux" or not hasattr(os, "sched_getaffinity"):
        return proof
    proof["method"] = "linux-task-affinity+genuine-hierarchy-quota-minimum"
    try:
        proc = proc_root / str(pid)
        # Reading another mount/cgroup namespace through our filesystem would
        # silently associate its membership with unrelated quota files.
        for ns in ("mnt", "cgroup"):
            if os.readlink(proc / "ns" / ns) != os.readlink(proc_root / "self" / "ns" / ns):
                raise ValueError(f"per-PID {ns} namespace differs from sampler")
        tasks = list((proc / "task").iterdir())
        if not tasks:
            raise ValueError("no live tasks for affinity proof")
        membership_text = (proc / "cgroup").read_text(encoding="utf-8")
        for task in tasks:
            if (task / "cgroup").read_text(encoding="utf-8") != membership_text:
                raise ValueError("live tasks have unequal CPU cgroup memberships")
        cpus = set().union(*(os.sched_getaffinity(int(task.name)) for task in tasks))
        if not cpus:
            raise ValueError("empty enforced task affinity")
        proof["affinity_cpus"] = sorted(cpus)
        proof["affinity_cpu_count"] = len(cpus)

        memberships = []
        for line in membership_text.splitlines():
            hierarchy, controllers, membership = line.split(":", 2)
            if hierarchy == "0" and controllers == "":
                memberships.append(("cgroup2", membership))
            elif "cpu" in controllers.split(","):
                memberships.append(("cgroup", membership))
        candidates = []
        for line in (proc / "mountinfo").read_text(encoding="utf-8").splitlines():
            before, after = line.split(" - ", 1)
            fields, fs = before.split(), after.split()
            if len(fields) < 6 or len(fs) < 3:
                raise ValueError("malformed cgroup mount information")
            for version, membership in memberships:
                if fs[0] != version or (version == "cgroup" and "cpu" not in fs[2].split(",")):
                    continue
                decode = lambda value: re.sub(r"\\([0-7]{3})", lambda m: chr(int(m[1], 8)), value)
                candidates.append((version, membership, decode(fields[3]), decode(fields[4]), fields[2]))
        if len(candidates) != 1:
            raise ValueError("CPU cgroup membership/mount mapping is missing or ambiguous")
        version, membership, mount_root, mount, mount_device = candidates[0]
        proof.update(cgroup_version=version, cgroup_membership=membership,
                     cgroup_mount_root=mount_root, cgroup_mount=mount)
        if mount_root != "/":
            # This exposed file is retained only as an unmapped observation.
            try:
                proof["unmapped_mount_root_cpu_max_raw"] = (Path(mount) / "cpu.max").read_text(encoding="utf-8").strip()
            except OSError:
                pass
            raise ValueError("cgroup mount root hides or ambiguously maps ancestors")
        if not membership.startswith("/") or any(p in (".", "..") for p in membership.split("/")):
            raise ValueError("invalid absolute CPU cgroup membership")
        mount_path = Path(mount)
        if not mount_path.is_absolute():
            raise ValueError("non-absolute CPU cgroup mount")
        major, minor = (int(part) for part in mount_device.split(":"))
        if mount_path.stat().st_dev != os.makedev(major, minor):
            raise ValueError("quota mount path does not match the per-PID cgroup filesystem device")
        root_identity = {"verified": False, "method": "kernel root-only interface markers",
                         "reason": "genuine cgroup hierarchy root not established"}
        proof["hierarchy_root"] = root_identity
        try:
            entries = set(os.listdir(mount_path))
            if version == "cgroup2":
                # Linux CFTYPE_NOT_ON_ROOT: these core files and cpu.max are
                # absent only on the genuine root, not a namespace subgroup.
                nonroot_files = sorted(entries.intersection({"cgroup.type", "cgroup.events", "cpu.max"}))
                controllers = (mount_path / "cgroup.controllers").read_text(encoding="utf-8").split()
                for name in ("cgroup.procs", "cgroup.subtree_control", "cpu.stat"):
                    with (mount_path / name).open(encoding="utf-8") as interface:
                        interface.read(64)
                root_identity.update(nonroot_interfaces_present=nonroot_files,
                                     cpu_controller_available="cpu" in controllers)
                root_identity["verified"] = not nonroot_files and "cpu" in controllers
            else:
                # Linux CFTYPE_ONLY_ON_ROOT for both v1 core interfaces.
                for name in ("release_agent", "cgroup.sane_behavior"):
                    with (mount_path / name).open(encoding="utf-8") as interface:
                        interface.read(64)
                root_identity["verified"] = True
            if root_identity["verified"]:
                root_identity["reason"] = ""
        except OSError as exc:
            root_identity["reason"] = f"root interface proof unavailable: {exc}"
        group = mount_path / membership.lstrip("/")
        quotas = []
        while True:
            if version == "cgroup2":
                quota_file = group / "cpu.max"
                try:
                    raw = quota_file.read_text(encoding="utf-8").strip()
                    quota_text, period_text = raw.split()
                    period = int(period_text)
                    quota = None if quota_text == "max" else int(quota_text)
                except FileNotFoundError:
                    if group != mount_path or not root_identity["verified"]:
                        raise ValueError(f"CPU quota interface unavailable without genuine-root proof: {quota_file}")
                    # CFTYPE_NOT_ON_ROOT: genuine v2 root is unthrottled.
                    raw, quota, period = None, None, 1
            else:
                quota_file = group / "cpu.cfs_quota_us"
                raw_quota = quota_file.read_text(encoding="utf-8").strip()
                raw_period = (group / "cpu.cfs_period_us").read_text(encoding="utf-8").strip()
                raw = f"{raw_quota} {raw_period}"
                quota_value, period = int(raw_quota), int(raw_period)
                quota = None if quota_value == -1 else quota_value
            if period <= 0 or (quota is not None and quota <= 0):
                raise ValueError(f"invalid CPU quota/period at {quota_file}")
            capacity = quota / period if quota is not None else None
            if capacity is not None:
                if not math.isfinite(capacity) or capacity <= 0:
                    raise ValueError("invalid finite CPU quota capacity")
                quotas.append(capacity)
            reading = {"path": str(quota_file), "raw": raw, "cpu_capacity": capacity}
            if raw is None:
                reading["interpretation"] = "genuine v2 root: cpu.max absent by CFTYPE_NOT_ON_ROOT"
            proof["quota_samples"].append(reading)
            if group == mount_path:
                break
            group = group.parent
        quota_capacity = min(quotas) if quotas else None
        proof["quota_cpu_capacity"] = quota_capacity
        proof["visible_cpu_capacity"] = min(len(cpus), quota_capacity) if quotas else float(len(cpus))
        if not root_identity["verified"]:
            raise ValueError(root_identity["reason"])
        proof["effective_cpu_capacity"] = proof["visible_cpu_capacity"]
        proof["verified"], proof["reason"] = True, ""
    except (OSError, ValueError, OverflowError) as exc:
        proof["reason"] = str(exc)
    return proof


def finite_cpu_delta(initial: float, final: float) -> float:
    """Retain bounded diagnostic counters; verification is a separate guard."""
    delta = final - initial
    return max(0.0, delta) if math.isfinite(delta) else 0.0


def cpu_counter_window(initial, final) -> Dict[str, Any]:
    """Prove the interval belonging to a cumulative endpoint CPU delta."""
    def value(snapshot, name):
        number = getattr(snapshot, name, None)
        return number if isinstance(number, (int, float)) and math.isfinite(number) else None

    start = value(initial, "timestamp")
    end = value(final, "timestamp")
    duration = end - start if start is not None and end is not None else None
    if duration is not None and not math.isfinite(duration):
        duration = None
    counters = {f"{endpoint}_{name}_seconds": value(snapshot, attribute)
                for endpoint, snapshot in (("initial", initial), ("final", final))
                for name, attribute in (("user", "user_s"), ("system", "sys_s"))}
    reason = ""
    if initial is None or final is None:
        reason = "initial or final endpoint CPU snapshot missing"
    elif duration is None or duration <= 0:
        reason = "endpoint CPU snapshot timestamps are nonfinite or nonmonotonic"
    elif any(c is None or c < 0 for c in counters.values()):
        reason = "endpoint CPU counters are nonfinite or negative"
    elif (final.user_s < initial.user_s or final.sys_s < initial.sys_s
          or not math.isfinite(final.total_cpu_s - initial.total_cpu_s)):
        reason = "endpoint CPU counters are nonmonotonic or overflowed"
    return {"verified": not reason, "start_monotonic_s": start,
            "end_monotonic_s": end, "duration_seconds": duration,
            **counters, "reason": reason}


class ProcessResourceMonitor:
    """Monitors, polls, and attributes CPU and memory to client and server processes."""

    def __init__(
        self,
        server_pid: int,
        client_pid: Optional[int] = None,
        poll_interval_s: float = 0.05,
        saturation_threshold_pct: float = 95.0,
    ):
        self.server_pid = server_pid
        self.client_pid = client_pid
        self.poll_interval_s = poll_interval_s
        self.saturation_threshold_pct = saturation_threshold_pct

        self._stop_event = threading.Event()
        self._worker_thread: Optional[threading.Thread] = None

        self.start_time: float = 0.0
        self.end_time: float = 0.0

        self.server_initial: Optional[ProcessSnapshot] = None
        self.server_final: Optional[ProcessSnapshot] = None
        self.server_peak_rss: int = 0
        self.server_max_threads: int = 1
        self.server_cpu_samples: List[float] = []
        self._server_last_snap: Optional[ProcessSnapshot] = None

        self.client_initial: Optional[ProcessSnapshot] = None
        self.client_final: Optional[ProcessSnapshot] = None
        self.client_peak_rss: int = 0
        self.client_max_threads: int = 1
        self.client_cpu_samples: List[float] = []
        self._client_last_snap: Optional[ProcessSnapshot] = None
        self._capacity_samples: Dict[str, List[Dict[str, Any]]] = {"client": [], "server": []}
        self._counter_errors: Dict[str, List[str]] = {"client": [], "server": []}

        # Capture server initial baseline immediately upon monitor creation
        self.server_initial = sample_process(self.server_pid)
        self._observe_capacity("server", self.server_pid)
        if self.server_initial:
            self.server_peak_rss = self.server_initial.rss_bytes
            self.server_max_threads = self.server_initial.thread_count
            self._server_last_snap = self.server_initial

    def set_client_pid(self, client_pid: int) -> None:
        self.client_pid = client_pid
        self.client_initial = sample_process(client_pid)
        self._observe_capacity("client", client_pid)
        if self.client_initial:
            self.client_peak_rss = self.client_initial.rss_bytes
            self.client_max_threads = self.client_initial.thread_count
            self._client_last_snap = self.client_initial

    def _observe_capacity(self, endpoint: str, pid: int) -> None:
        proof = collect_process_cpu_capacity(pid)
        proof["observed_at_monotonic_s"] = time.monotonic()
        self._capacity_samples[endpoint].append(proof)

    def _capacity_proof(self, endpoint: str) -> Dict[str, Any]:
        samples = self._capacity_samples[endpoint]
        if not samples:
            return {"verified": False, "effective_cpu_capacity": None,
                    "reason": "CPU capacity was not sampled", "sample_count": 0}
        proof = dict(samples[-1])
        proof["sample_count"] = len(samples)
        observations: Dict[str, Dict[str, Any]] = {}
        for index, sample in enumerate(samples):
            values = {key: value for key, value in sample.items() if key != "observed_at_monotonic_s"}
            key = json.dumps(values, sort_keys=True)
            if key not in observations:
                observations[key] = {"first_sample_index": index, "last_sample_index": index,
                                     "first_monotonic_s": sample.get("observed_at_monotonic_s"),
                                     "last_monotonic_s": sample.get("observed_at_monotonic_s"),
                                     "proof": values}
            observations[key]["last_sample_index"] = index
            observations[key]["last_monotonic_s"] = sample.get("observed_at_monotonic_s")
        proof["capacity_observations"] = list(observations.values())
        unknown = next((s for s in samples if not s["verified"]), None)
        signature_keys = ("affinity_cpus", "quota_samples", "cgroup_membership", "cgroup_mount",
                          "cgroup_mount_root", "effective_cpu_capacity")
        signatures = [json.dumps({key: s.get(key) for key in signature_keys}, sort_keys=True) for s in samples]
        proof["stable_at_samples"] = len(set(signatures)) == 1
        if unknown or not proof["stable_at_samples"]:
            proof["verified"] = False
            proof["effective_cpu_capacity"] = None
            proof["reason"] = unknown["reason"] if unknown else "CPU capacity changed during sampling window"
        return proof

    def start(self) -> None:
        self.start_time = time.monotonic()
        self._stop_event.clear()
        self._worker_thread = threading.Thread(
            target=self._poll_loop,
            name="rpc-bench-resource-monitor",
            daemon=True,
        )
        self._worker_thread.start()

    def _poll_loop(self) -> None:
        while not self._stop_event.is_set():
            # Sample client
            if self.client_pid:
                snap = sample_process(self.client_pid)
                if snap:
                    self._check_counter_transition("client", self._client_last_snap, snap)
                    self._observe_capacity("client", self.client_pid)
                    self.client_peak_rss = max(self.client_peak_rss, snap.rss_bytes)
                    self.client_max_threads = max(self.client_max_threads, snap.thread_count)
                    if self._client_last_snap:
                        dt = snap.timestamp - self._client_last_snap.timestamp
                        dcpu = snap.total_cpu_s - self._client_last_snap.total_cpu_s
                        if dt > 0.001:
                            pct = max(0.0, (dcpu / dt) * 100.0)
                            self.client_cpu_samples.append(pct)
                    self._client_last_snap = snap

            # Sample server
            if self.server_pid:
                snap = sample_process(self.server_pid)
                if snap:
                    self._check_counter_transition("server", self._server_last_snap, snap)
                    self._observe_capacity("server", self.server_pid)
                    self.server_peak_rss = max(self.server_peak_rss, snap.rss_bytes)
                    self.server_max_threads = max(self.server_max_threads, snap.thread_count)
                    if self._server_last_snap:
                        dt = snap.timestamp - self._server_last_snap.timestamp
                        dcpu = snap.total_cpu_s - self._server_last_snap.total_cpu_s
                        if dt > 0.001:
                            pct = max(0.0, (dcpu / dt) * 100.0)
                            self.server_cpu_samples.append(pct)
                    self._server_last_snap = snap

            self._stop_event.wait(self.poll_interval_s)

    def _check_counter_transition(self, endpoint: str, initial, final) -> None:
        if initial is not None:
            window = cpu_counter_window(initial, final)
            if not window["verified"]:
                self._counter_errors[endpoint].append(window["reason"])

    def stop(self) -> Tuple[Dict, Dict, Dict]:
        self._stop_event.set()
        if self._worker_thread and self._worker_thread.is_alive():
            self._worker_thread.join(timeout=1.0)
        self.end_time = time.monotonic()

        # Capture final snapshots
        if self.client_pid:
            final_c = sample_process(self.client_pid)
            if final_c:
                self._check_counter_transition("client", self._client_last_snap, final_c)
                self._observe_capacity("client", self.client_pid)
                self.client_final = final_c
                self.client_peak_rss = max(self.client_peak_rss, final_c.rss_bytes)
                self.client_max_threads = max(self.client_max_threads, final_c.thread_count)

        if self.server_pid:
            final_s = sample_process(self.server_pid)
            if final_s:
                self._check_counter_transition("server", self._server_last_snap, final_s)
                self._observe_capacity("server", self.server_pid)
                self.server_final = final_s
                self.server_peak_rss = max(self.server_peak_rss, final_s.rss_bytes)
                self.server_max_threads = max(self.server_max_threads, final_s.thread_count)

        # 1. Compute client metrics
        c_init_user = self.client_initial.user_s if self.client_initial else 0.0
        c_init_sys = self.client_initial.sys_s if self.client_initial else 0.0
        c_final_user = self.client_final.user_s if self.client_final else (self._client_last_snap.user_s if self._client_last_snap else c_init_user)
        c_final_sys = self.client_final.sys_s if self.client_final else (self._client_last_snap.sys_s if self._client_last_snap else c_init_sys)

        client_user_s = finite_cpu_delta(c_init_user, c_final_user)
        client_sys_s = finite_cpu_delta(c_init_sys, c_final_sys)
        client_total_s = client_user_s + client_sys_s
        client_peak_rss = max(self.client_peak_rss, 1024)
        client_threads = max(self.client_max_threads, 1)

        # 2. Compute server metrics
        s_init_user = self.server_initial.user_s if self.server_initial else 0.0
        s_init_sys = self.server_initial.sys_s if self.server_initial else 0.0
        s_final_user = self.server_final.user_s if self.server_final else (self._server_last_snap.user_s if self._server_last_snap else s_init_user)
        s_final_sys = self.server_final.sys_s if self.server_final else (self._server_last_snap.sys_s if self._server_last_snap else s_init_sys)

        server_user_s = finite_cpu_delta(s_init_user, s_final_user)
        server_sys_s = finite_cpu_delta(s_init_sys, s_final_sys)
        server_total_s = server_user_s + server_sys_s
        server_peak_rss = max(self.server_peak_rss, 1024)
        server_threads = max(self.server_max_threads, 1)

        # 3. Client saturation verification
        # Raw CPU percentage is core-seconds per wall-second. Thread count is
        # diagnostic only: extra threads cannot create extra CPU capacity.
        client_capacity = self._capacity_proof("client")
        server_capacity = self._capacity_proof("server")
        client_window = cpu_counter_window(self.client_initial, self.client_final)
        server_window = cpu_counter_window(self.server_initial, self.server_final)
        for endpoint, window in (("client", client_window), ("server", server_window)):
            window["observed_transition_errors"] = self._counter_errors[endpoint]
            if self._counter_errors[endpoint]:
                window.update(verified=False, reason=self._counter_errors[endpoint][0])
        client_delta_verified = client_window["verified"]
        server_delta_verified = server_window["verified"]
        # Weight every CPU second by the same endpoint's actual counter window.
        # Equal weighting of sampled percentages can hide a long busy interval.
        avg_cpu_pct = (client_total_s / client_window["duration_seconds"] * 100.0
                       if client_delta_verified else None)
        finite_samples = [s for s in self.client_cpu_samples if math.isfinite(s)]
        sample_mean_pct = sum(finite_samples) / len(finite_samples) if finite_samples else None
        peak_cpu_pct = max(finite_samples) if finite_samples else avg_cpu_pct
        headroom_verified = bool(client_capacity["verified"] and client_delta_verified)
        effective_capacity = client_capacity.get("effective_cpu_capacity")
        utilization_pct = avg_cpu_pct / effective_capacity if headroom_verified else None
        spare_capacity_pct = max(0.0, 100.0 - utilization_pct) if headroom_verified else None
        # Unknown budgets conservatively block every existing saturation gate.
        is_saturated = not headroom_verified or utilization_pct >= self.saturation_threshold_pct
        saturation_status = "FAIL (UNVERIFIED)" if not headroom_verified else ("FAIL (SATURATED)" if is_saturated else "PASS")
        if not headroom_verified:
            saturation_message = f"Client CPU headroom unverified: {client_capacity.get('reason') or client_window['reason']}"
        elif is_saturated:
            saturation_message = (
                f"Client load generator reached {avg_cpu_pct:.1f}% CPU utilization "
                f"(spare capacity: {spare_capacity_pct:.1f}%). Load generator saturated its capacity; "
                f"measured throughput may reflect client generation limits rather than true server ceiling."
            )
        else:
            saturation_message = (
                f"Client load generator maintained {spare_capacity_pct:.1f}% spare CPU capacity "
                f"(avg CPU: {avg_cpu_pct:.1f}%, effective capacity: {effective_capacity:g} CPUs). "
                "Headroom alone does not establish a server ceiling."
            )

        # Explicit platform support: when no snapshot was ever captured for an
        # endpoint, its counters are unsupported (never measured zeros).
        client_supported = (self.client_initial is not None) or (self._client_last_snap is not None)
        server_supported = (self.server_initial is not None) or (self._server_last_snap is not None)
        client_method = self._client_last_snap.method if self._client_last_snap else "unsupported"
        server_method = self._server_last_snap.method if self._server_last_snap else "unsupported"

        client_dict = {
            "user_cpu_seconds": round(client_user_s, 6),
            "system_cpu_seconds": round(client_sys_s, 6),
            "peak_rss_bytes": client_peak_rss,
            "peak_rss_mib": round(client_peak_rss / (1024.0 * 1024.0), 3),
            "current_rss_bytes": self.client_final.rss_bytes if self.client_final else client_peak_rss,
            "current_rss_mib": round((self.client_final.rss_bytes if self.client_final else client_peak_rss) / (1024.0 * 1024.0), 3),
            "user_cpu_nanos": int(client_user_s * 1e9),
            "system_cpu_nanos": int(client_sys_s * 1e9),
            "thread_count": client_threads,
            "cpu_capacity": client_capacity,
            "cpu_delta_verified": client_delta_verified,
            "cpu_counter_window": client_window,
            "cpu_seconds_per_rpc": None,
            "method": client_method,
            "supported": client_supported,
        }

        server_dict = {
            "user_cpu_seconds": round(server_user_s, 6),
            "system_cpu_seconds": round(server_sys_s, 6),
            "peak_rss_bytes": server_peak_rss,
            "peak_rss_mib": round(server_peak_rss / (1024.0 * 1024.0), 3),
            "current_rss_bytes": self.server_final.rss_bytes if self.server_final else server_peak_rss,
            "current_rss_mib": round((self.server_final.rss_bytes if self.server_final else server_peak_rss) / (1024.0 * 1024.0), 3),
            "user_cpu_nanos": int(server_user_s * 1e9),
            "system_cpu_nanos": int(server_sys_s * 1e9),
            "thread_count": server_threads,
            "cpu_capacity": server_capacity,
            "cpu_delta_verified": server_delta_verified,
            "cpu_counter_window": server_window,
            "cpu_seconds_per_rpc": None,
            "method": server_method,
            "supported": server_supported,
        }

        saturation_dict = {
            "saturated": is_saturated,
            "avg_cpu_pct": round(avg_cpu_pct, 1) if avg_cpu_pct is not None else None,
            "sample_mean_cpu_pct": round(sample_mean_pct, 1) if sample_mean_pct is not None else None,
            "peak_cpu_pct": round(peak_cpu_pct, 1) if peak_cpu_pct is not None else None,
            "cpu_utilization_method": "cumulative_endpoint_delta_over_snapshot_interval",
            "monitor_wall_seconds": self.end_time - self.start_time,
            "spare_capacity_pct": round(spare_capacity_pct, 1) if spare_capacity_pct is not None else None,
            "capacity_utilization_pct": round(utilization_pct, 1) if utilization_pct is not None else None,
            "effective_cpu_capacity": effective_capacity,
            "headroom_verified": headroom_verified,
            "status": saturation_status,
            "message": saturation_message,
        }

        return client_dict, server_dict, saturation_dict


def _cleanup_processes() -> None:
    """Terminate all active child processes with SIGTERM, then SIGKILL if needed."""
    for proc in list(_ACTIVE_PROCESSES):
        if proc.poll() is None:
            try:
                proc.terminate()
                try:
                    proc.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=1.0)
            except OSError:
                pass
        _ACTIVE_PROCESSES.discard(proc)


def _signal_handler(signum: int, _frame) -> None:
    """Handle termination signals by cleaning up child processes and exiting."""
    _cleanup_processes()
    sys.exit(128 + signum)


atexit.register(_cleanup_processes)
signal.signal(signal.SIGINT, _signal_handler)
signal.signal(signal.SIGTERM, _signal_handler)


MATCHING_CONFIGURATION = {
    "tcp_nodelay": True,
    "tls_cipher": "TLS_AES_128_GCM_SHA256",
    "connections": 1,
    "concurrency": 1,
    # BM-07: full matched-dimension set per contract accept (2). Dimensions
    # the harness cannot pin on official peers stay explicit peer defaults
    # (see rpc-bench/peers/*.json); they are recorded, never assumed equal.
    "compression": "none",
    "cores": "matched pinned cores per stage; effective quota recorded in cpu_constraints",
    "http2_windows": "16 MiB stream/connection windows on native and tonic-pbrs; go/cpp peer defaults",
    "message_limits": "4 MiB worker body cap; receiving-peer protobuf overhead counted",
    "deadlines": "5 s worker call deadline; soak per-iteration ceiling 5000 ms",
    "handler_work": "identical TestService/BenchmarkService procedures and validation on every peer",
    "payload_sizes": {
        "empty_unary": {"request_bytes": 0, "response_bytes": 0},
        "large_unary": {"request_bytes": 271828, "response_bytes": 314159},
        "stream": {"message_bytes": 1024},
        "ping_pong": {"message_bytes": 0, "round_trips": 256},
        "upload": {"message_bytes": 1024},
    },
}


def find_free_port() -> int:
    """Find an available ephemeral TCP port."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def find_binary(repo_root: Path, override_path: Optional[str] = None) -> Path:
    """Find the rpc-bench binary."""
    if override_path:
        p = Path(override_path)
        if p.is_file() and os.access(p, os.X_OK):
            return p.resolve()
        raise FileNotFoundError(f"Specified binary not found or not executable: {override_path}")

    candidates = [
        repo_root / "target" / "release" / "rpc-bench",
        repo_root / "target" / "debug" / "rpc-bench",
        repo_root / "rpc-bench" / "target" / "release" / "rpc-bench",
        repo_root / "rpc-bench" / "target" / "debug" / "rpc-bench",
    ]
    for c in candidates:
        if c.is_file() and os.access(c, os.X_OK):
            return c.resolve()

    raise FileNotFoundError(
        "rpc-bench binary not found. Build it with: CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target cargo build --locked --manifest-path rpc-bench/Cargo.toml"
    )

def bounded_cargo_env() -> Dict[str, str]:
    """Keep subprocess builds on the caller's cache without multiplying rustc jobs."""
    requested = os.environ.get("CARGO_BUILD_JOBS", "2")
    if not requested.isdecimal() or int(requested) < 1:
        raise ValueError("CARGO_BUILD_JOBS must be a positive integer")
    return {**os.environ, "CARGO_BUILD_JOBS": str(min(int(requested), 2))}


def collect_cpu_constraints() -> Dict[str, Any]:
    """Record the effective core quota/affinity the matrix runs under.

    Mirrors the Rust `CpuConstraints` record: unknown values stay `None`
    (never measured zeros) and `source` names the detection path explicitly.
    """
    capacity = collect_process_cpu_capacity(os.getpid())
    info: Dict[str, Any] = {
        "effective_cpu_count": None,
        "affinity_cpus": ",".join(str(c) for c in capacity["affinity_cpus"]) if capacity["affinity_cpus"] else None,
        "affinity_count": capacity["affinity_cpu_count"],
        "cgroup_quota_millicpus": int(capacity["quota_cpu_capacity"] * 1000) if capacity["quota_cpu_capacity"] is not None else None,
        "effective_cpu_capacity": capacity["effective_cpu_capacity"],
        "capacity_verified": capacity["verified"],
        "capacity_proof": capacity,
        "source": capacity["method"],
    }

    try:
        if hasattr(os, "process_cpu_count"):
            info["effective_cpu_count"] = os.process_cpu_count()
        else:
            info["effective_cpu_count"] = os.cpu_count()
    except Exception:
        pass

    return info


class PeerRegistry:
    """Registry and launcher metadata loader for benchmark peers."""

    def __init__(self, repo_root: Path, peer_dir: Optional[Path] = None):
        self.repo_root = repo_root
        self.peer_dir = peer_dir or (repo_root / "rpc-bench" / "peers")
        self.peers: Dict[str, Dict[str, Any]] = {}
        self.tonic_build_error = ""
        self._load_peers()

    def _load_peers(self) -> None:
        if not self.peer_dir.is_dir():
            raise FileNotFoundError(f"Benchmark peer directory missing: {self.peer_dir}")
        for f in sorted(self.peer_dir.glob("*.json")):
            try:
                with open(f, "r", encoding="utf-8") as fp:
                    data = json.load(fp)
            except (OSError, json.JSONDecodeError) as e:
                raise ValueError(f"Invalid benchmark peer manifest {f}: {e}") from e
            if not isinstance(data, dict) or not isinstance(data.get("id"), str):
                raise ValueError(f"Benchmark peer manifest {f} requires a string id")
            if data["id"] in self.peers:
                raise ValueError(f"Duplicate benchmark peer id {data['id']} in {f}")
            self.peers[data["id"]] = data

    def get_peer(self, name: str) -> Optional[Dict[str, Any]]:
        return self.peers.get(name)

    def resolve_native_binary(self, override_path: Optional[str] = None) -> Path:
        return find_binary(self.repo_root, override_path)

    def resolve_tonic_binary(self, override_path: Optional[str] = None) -> Optional[Path]:
        if override_path:
            p = Path(override_path)
            if p.is_file() and os.access(p, os.X_OK):
                return p.resolve()

        candidates = [
            self.repo_root / "target" / "release" / "tonic-interop",
            self.repo_root / "target" / "debug" / "tonic-interop",
            self.repo_root / "tests" / "interop" / "tonic" / "target" / "release" / "tonic-interop",
            self.repo_root / "tests" / "interop" / "tonic" / "target" / "debug" / "tonic-interop",
        ]
        for c in candidates:
            if c.is_file() and os.access(c, os.X_OK):
                return c.resolve()

        # Attempt build if Cargo manifest is present
        manifest = self.repo_root / "tests" / "interop" / "tonic" / "Cargo.toml"
        if manifest.is_file():
            try:
                subprocess.run(
                    [
                        "cargo",
                        "build",
                        "--locked",
                        "--release",
                        "--manifest-path",
                        str(manifest),
                        "--target-dir",
                        str(self.repo_root / "target"),
                    ],
                    capture_output=True,
                    text=True,
                    check=True,
                    env=bounded_cargo_env(),
                )
                cand = self.repo_root / "target" / "release" / "tonic-interop"
                if cand.is_file() and os.access(cand, os.X_OK):
                    return cand.resolve()
                self.tonic_build_error = f"Build succeeded but tonic-interop missing at {cand}"
            except (OSError, subprocess.CalledProcessError) as e:
                stderr = e.stderr.strip() if isinstance(e, subprocess.CalledProcessError) and e.stderr else str(e)
                self.tonic_build_error = f"tonic-interop build failed: {stderr[-1200:]}"
        else:
            self.tonic_build_error = f"tonic-interop manifest missing: {manifest}"
        return None

    def resolve_go_peer(self, role: str) -> Optional[Path]:
        bin_dir = self.repo_root / "target" / "interop-go"
        bin_name = f"go-interop-{role}"
        target_bin = bin_dir / bin_name
        if target_bin.is_file() and os.access(target_bin, os.X_OK):
            return target_bin.resolve()

        alt_bin = self.repo_root / "tests" / "interop" / "go" / bin_name
        if alt_bin.is_file() and os.access(alt_bin, os.X_OK):
            return alt_bin.resolve()

        go_dir = self.repo_root / "tests" / "interop" / "go"
        if not (go_dir / "go.mod").is_file():
            return None

        try:
            bin_dir.mkdir(parents=True, exist_ok=True)
            pkg = f"google.golang.org/grpc/interop/{role}"
            subprocess.run(
                ["go", "build", "-mod=readonly", "-o", str(target_bin), pkg],
                cwd=str(go_dir),
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=True,
            )
            if target_bin.is_file() and os.access(target_bin, os.X_OK):
                return target_bin.resolve()
        except Exception:
            pass
        return None

    def resolve_cpp_peer(self, role: str) -> Optional[Path]:
        env_key = f"GRPC_INTEROP_CPP_{role.upper()}"
        env_val = os.environ.get(env_key)
        if env_val:
            p = Path(env_val)
            if p.is_file() and os.access(p, os.X_OK):
                return p.resolve()

        bin_name = f"interop_{role}"
        candidates = [
            self.repo_root / "target" / "interop-cpp" / bin_name,
            self.repo_root / "target" / "interop-cpp-build" / bin_name,
            self.repo_root / "third_party" / "grpc" / "cmake" / "build" / bin_name,
        ]
        for c in candidates:
            if c.is_file() and os.access(c, os.X_OK):
                return c.resolve()
        return None


def wait_for_server_readiness(
    proc: subprocess.Popen, host: str, port: int, timeout_secs: float = 15.0
) -> Tuple[int, str]:
    """Read server stdout or probe TCP connection until server is ready.

    Returns (bound_port, bound_addr).
    """
    ready_pattern = re.compile(r"^READY\s+port=(\d+)\s+addr=([^\s]+)")
    deadline = time.monotonic() + timeout_secs
    bound_port = port
    bound_addr = f"{host}:{port}" if port > 0 else ""

    if proc.stdout:
        try:
            os.set_blocking(proc.stdout.fileno(), False)
        except Exception:
            pass

    while time.monotonic() < deadline:
        if proc.poll() is not None:
            stderr_out = ""
            if proc.stderr:
                try:
                    stderr_out = proc.stderr.read()
                except Exception:
                    pass
            raise RuntimeError(
                f"Server process terminated prematurely with exit code {proc.returncode}. Stderr: {stderr_out.strip()}"
            )

        if bound_port > 0:
            try:
                with socket.create_connection((host, bound_port), timeout=0.05):
                    return bound_port, f"{host}:{bound_port}"
            except OSError:
                pass

        if proc.stdout:
            try:
                line = proc.stdout.readline()
                if line:
                    line_str = line.strip()
                    match = ready_pattern.match(line_str)
                    if match:
                        bound_port = int(match.group(1))
                        bound_addr = match.group(2)
                        return bound_port, bound_addr
            except Exception:
                pass

        time.sleep(0.02)

    raise TimeoutError(f"Server failed to become ready within {timeout_secs} seconds")


def parse_soak_output(
    stderr: str, duration_s: float, req_size: int, resp_size: int, expected_iterations: int
) -> Dict[str, Any]:
    """Parse soak test output from Go / C++ interop clients into BenchmarkRun metrics."""
    succ_m = re.search(
        r"soak test successes:\s+(\d+)\s+/\s+(\d+)\s+iterations\.\s+Total failures:\s+(\d+)",
        stderr,
    )
    if succ_m:
        successes = int(succ_m.group(1))
        total = int(succ_m.group(2))
        failures = int(succ_m.group(3))
    else:
        cpp_m = re.search(
            r"soak test ran:\s+(\d+)\s+iterations\.\s+total_failures:\s+(\d+)"
            r"\s+is within max_failures_threshold:\s+(\d+)",
            stderr,
        )
        if not cpp_m:
            raise ValueError("soak summary missing from reference client output")
        total = int(cpp_m.group(1))
        failures = int(cpp_m.group(2))
        successes = total - failures
    if total != expected_iterations:
        raise ValueError(f"requested {expected_iterations} iterations but peer reported {total}")
    if failures or successes != total:
        raise ValueError(f"soak failures or omissions: {successes}/{total} succeeded, {failures} failures")
    if duration_s <= 0:
        raise ValueError("soak duration must be positive")

    lat_m = re.search(
        r"Latencies in milliseconds:\s+Count:\s+(\d+)\s+Min:\s+([\d.]+)\s+Max:\s+([\d.]+)\s+Avg:\s+([\d.]+)",
        stderr,
    )
    if lat_m and int(lat_m.group(1)) != total:
        raise ValueError(f"latency summary count {lat_m.group(1)} does not match {total} iterations")
    samples = [
        (int(m.group(1)), float(m.group(2)))
        for m in re.finditer(r"soak iteration:\s+(\d+)\s+elapsed_ms:\s+([\d.]+)", stderr)
    ]
    if len(samples) != total or len({index for index, _ in samples}) != total:
        raise ValueError(f"expected {total} distinct latency samples, got {len(samples)}")
    samples_ms = sorted(latency for _, latency in samples)
    p50_ns = int(samples_ms[min(int(total * 0.5), total - 1)] * 1e6)
    p99_ns = int(samples_ms[min(int(total * 0.99), total - 1)] * 1e6)

    dur_nanos = int(duration_s * 1e9)
    throughput_qps = successes / duration_s

    shape_id = "unary_empty_plaintext" if req_size == 0 and resp_size == 0 else "unary_large_plaintext"
    shape_name = (
        "Unary Empty Payload (Plaintext)"
        if req_size == 0 and resp_size == 0
        else "Unary Large Payload (Plaintext)"
    )

    return {
        "scenario": {
            "id": shape_id,
            "name": shape_name,
            "category": "primary",
            "rpc_type": "unary",
            "request_size_bytes": req_size,
            "response_size_bytes": resp_size,
        },
        "metrics": {
            "total_calls": total,
            "successful_rpcs": successes,
            "failed_rpcs": failures,
            "duration_nanos": dur_nanos,
            "throughput_qps": round(throughput_qps, 2),
        },
        "latency": {
            "p50_nanos": p50_ns,
            "p99_nanos": p99_ns,
            "sample_count": total,
            "measurement_resolution_ms": 1,
            "raw_samples_ms": samples_ms,
        },
    }


def aggregate_endpoint_resources(samples: List[Dict[str, Any]]) -> Dict[str, Any]:
    if not samples or any(not sample.get("supported") for sample in samples):
        raise ValueError("endpoint resource sampling unavailable for one or more processes")
    combined = dict(samples[-1])
    combined["user_cpu_seconds"] = round(sum(s["user_cpu_seconds"] for s in samples), 6)
    combined["system_cpu_seconds"] = round(sum(s["system_cpu_seconds"] for s in samples), 6)
    combined["user_cpu_nanos"] = int(combined["user_cpu_seconds"] * 1e9)
    combined["system_cpu_nanos"] = int(combined["system_cpu_seconds"] * 1e9)
    combined["peak_rss_bytes"] = max(s["peak_rss_bytes"] for s in samples)
    combined["peak_rss_mib"] = round(combined["peak_rss_bytes"] / (1024.0 * 1024.0), 3)
    combined["thread_count"] = max(s["thread_count"] for s in samples)
    capacities = [s.get("cpu_capacity", {}) for s in samples]
    capacity = dict(capacities[-1])
    if (not all(c.get("verified") is True for c in capacities)
            or len({c.get("effective_cpu_capacity") for c in capacities}) != 1):
        capacity.update(verified=False, effective_cpu_capacity=None,
                        reason="one or more sequential endpoint CPU budgets unverified or unequal")
    capacity["process_count"] = len(samples)
    combined["cpu_capacity"] = capacity
    combined["cpu_delta_verified"] = all(s.get("cpu_delta_verified") is True for s in samples)
    # These are different processes and intervals. Never associate summed CPU
    # counters with only the last process's snapshot timestamps.
    combined.pop("cpu_counter_window", None)
    combined["cpu_counter_windows"] = [s.get("cpu_counter_window") for s in samples]
    combined["method"] = f"sum-of-{len(samples)}-{samples[-1]['method']}"
    return combined


def server_ceiling_exclusion(
    quick: bool,
    client_workload: str,
    saturation: Dict[str, Any],
    client_resources: Dict[str, Any],
    server_resources: Dict[str, Any],
    runs: List[Dict[str, Any]],
) -> Optional[str]:
    if quick:
        return "quick smoke runs cannot establish a server ceiling"
    if client_workload != "rpc-bench":
        return "reference interop-soak clients execute a different workload"
    if not client_resources.get("supported") or not server_resources.get("supported"):
        return "separate client and server resource measurements are unavailable"
    if saturation["saturated"]:
        return "load generator saturated"
    if not runs or any(run.get("metrics", {}).get("duration_nanos", 0) < 60_000_000_000 for run in runs):
        return "each scenario needs at least 60 seconds measured after warmup"
    if (saturation.get("headroom_verified") is not True
            or any(r.get("cpu_capacity", {}).get("verified") is not True
                   or r.get("cpu_delta_verified") is not True
                   for r in (client_resources, server_resources))):
        return "endpoint CPU capacity/headroom proof is unavailable"
    return None


def run_single_benchmark(
    peer_registry: PeerRegistry,
    server_peer: str,
    client_peer: str,
    shape: str,
    host: str,
    port: int,
    quick: bool,
    timeout_secs: float,
    verbose: bool,
    binary_override: Optional[str] = None,
) -> Tuple[bool, Optional[Dict], str]:
    """Run one server peer process and one client peer process pair."""
    native_bin = peer_registry.resolve_native_binary(binary_override)

    # 1. Resolve Server Command
    assigned_port = port if port > 0 else find_free_port()
    server_codec = PEER_CODECS.get(server_peer, "unknown")
    server_binary = ""
    if server_peer == "native":
        server_cmd = [
            str(native_bin),
            "server",
            "--transport=native",
            f"--host={host}",
            f"--port={assigned_port}",
            f"--timeout-secs={int(timeout_secs + 30)}",
        ]
        server_binary = str(native_bin)
    elif server_peer == TONIC_PBRS:
        server_cmd = [
            str(native_bin),
            "server",
            "--transport=tonic",
            f"--host={host}",
            f"--port={assigned_port}",
            f"--timeout-secs={int(timeout_secs + 30)}",
        ]
        server_binary = str(native_bin)
    elif server_peer in ("tonic", TONIC_PROST):
        tonic_server = peer_registry.resolve_tonic_binary()
        if not tonic_server:
            return False, None, (
                "tonic-prost server peer (tonic-interop) not found and could not be built. "
                "Refusing to substitute the pbrs codec; choose tonic-pbrs for a same-codec cell. "
                f"{peer_registry.tonic_build_error}"
            )
        server_cmd = [str(tonic_server), "server", f"--port={assigned_port}"]
        server_binary = str(tonic_server)
        server_codec = "prost"
    elif server_peer == "go":
        go_server = peer_registry.resolve_go_peer("server")
        if not go_server:
            return False, None, "Go server peer (go-interop-server) not found or could not be built."
        server_cmd = [str(go_server), f"-port={assigned_port}", "-use_tls=false"]
        server_binary = str(go_server)
    elif server_peer == "cpp":
        cpp_server = peer_registry.resolve_cpp_peer("server")
        if not cpp_server:
            return False, None, "C++ server peer (interop_server) not found. Set GRPC_INTEROP_CPP_SERVER or build via scripts/grpc-interop-cpp.sh."
        server_cmd = [str(cpp_server), f"--port={assigned_port}", "--use_tls=false"]
        server_binary = str(cpp_server)
    else:
        return False, None, f"Unsupported server peer: {server_peer}"

    if verbose:
        print(f"Spawning server ({server_peer}): {' '.join(server_cmd)}")

    server_proc = subprocess.Popen(
        server_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    _ACTIVE_PROCESSES.add(server_proc)

    report_dir = peer_registry.repo_root / "target" / "rpc-bench-logs"
    report_dir.mkdir(parents=True, exist_ok=True)
    temp_report_path = report_dir / f"rpc-bench-run-{os.getpid()}-{time.time_ns()}.json"
    preserve_report = False

    try:
        try:
            bound_port, bound_addr = wait_for_server_readiness(
                server_proc, host=host, port=assigned_port, timeout_secs=15.0
            )
        except (RuntimeError, TimeoutError) as e:
            return False, None, f"Server ({server_peer}) startup failed: {e}"

        if verbose:
            print(f"Server ({server_peer}) ready on {bound_addr} (PID {server_proc.pid})")

        resource_monitor = ProcessResourceMonitor(
            server_pid=server_proc.pid,
            poll_interval_s=0.02 if quick else 0.05,
        )

        client_stdout = ""
        client_stderr = ""
        report_data = None

        # 2. Execute Client Peer
        client_codec = PEER_CODECS.get(client_peer, "unknown")
        client_binary = ""
        client_workload = "rpc-bench"
        if client_peer in ("native", "tonic", TONIC_PBRS):
            # rpc-bench transports are native or tonic; the tonic transport
            # encodes with the pbrs codec (same-codec transport comparison).
            transport_flag = "tonic" if client_peer in ("tonic", TONIC_PBRS) else "native"
            if client_peer == "tonic":
                client_codec = "pbrs"
            client_cmd = [
                str(native_bin),
                "client",
                f"--server_addr={bound_addr}",
                f"--transport={transport_flag}",
                f"--shape={shape}",
                f"--output={temp_report_path}",
            ]
            client_binary = str(native_bin)
            if quick:
                client_cmd.append("--quick")

            if verbose:
                print(f"Spawning client ({client_peer}): {' '.join(client_cmd)}")

            client_proc = subprocess.Popen(
                client_cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            _ACTIVE_PROCESSES.add(client_proc)

            resource_monitor.set_client_pid(client_proc.pid)
            resource_monitor.start()

            try:
                client_stdout, client_stderr = client_proc.communicate(timeout=timeout_secs)
            except subprocess.TimeoutExpired:
                client_proc.kill()
                client_stdout, client_stderr = client_proc.communicate()
                return False, None, f"Client ({client_peer}) exceeded {timeout_secs}s: {client_stderr}"
            finally:
                _ACTIVE_PROCESSES.discard(client_proc)
                client_res, server_res, sat_check = resource_monitor.stop()

            if client_proc.returncode != 0:
                err_msg = (
                    f"Client ({client_peer}) failed (code {client_proc.returncode}):\n"
                    f"STDOUT:\n{client_stdout}\n"
                    f"STDERR:\n{client_stderr}"
                )
                return False, None, err_msg

            if not temp_report_path.is_file():
                return False, None, f"Client ({client_peer}) exited successfully without a BenchmarkReport at {temp_report_path}"
            try:
                with open(temp_report_path, "r", encoding="utf-8") as f:
                    report_data = json.load(f)
            except (OSError, json.JSONDecodeError) as e:
                preserve_report = True
                return False, None, f"Client ({client_peer}) wrote an invalid BenchmarkReport at {temp_report_path}: {e}"

        elif client_peer == TONIC_PROST:
            # No prost load generator exists (tonic-interop only runs named
            # interop cases, not the rpc-bench workload). Fail the cell
            # explicitly instead of silently substituting the pbrs codec.
            return False, None, (
                "tonic-prost client unsupported: no prost load generator; "
                "use client tonic-pbrs for the same-codec transport comparison "
                "or tonic-prost as server for end-to-end reference."
            )
        elif client_peer in ("go", "cpp"):
            client_bin = (
                peer_registry.resolve_go_peer("client")
                if client_peer == "go"
                else peer_registry.resolve_cpp_peer("client")
            )
            if not client_bin:
                peer_name = "Go" if client_peer == "go" else "C++"
                return False, None, f"{peer_name} client peer not found."
            if resource is None:
                return False, None, "POSIX child CPU accounting is unavailable for reference client processes."
            client_binary = str(client_bin)
            # Go/C++ reference clients drive fixed rpc_soak interop loops,
            # not the rpc-bench open-loop workload: recorded per run so
            # cross-client cells are never compared as equivalent.
            client_workload = "interop-soak"

            soak_runs = []
            empty_iters = 20 if quick else 200
            large_iters = 5 if quick else 50
            cases_to_run = [
                ("empty_unary", 0, 0, empty_iters),
                ("large_unary", 271828, 314159, large_iters),
            ]
            client_samples: List[Dict[str, Any]] = []
            server_samples: List[Dict[str, Any]] = []
            saturation_samples: List[Dict[str, Any]] = []
            log_dir = (
                peer_registry.repo_root
                / "target"
                / "rpc-bench-logs"
                / f"{time.time_ns()}-{os.getpid()}-{server_peer}-{client_peer}"
            )
            log_dir.mkdir(parents=True, exist_ok=False)
            for c_name, req_sz, resp_sz, iters in cases_to_run:
                flag_pfx = "-" if client_peer == "go" else "--"
                c_args = [
                    str(client_bin),
                    f"{flag_pfx}server_host={host}",
                    f"{flag_pfx}server_port={bound_port}",
                    f"{flag_pfx}use_tls=false",
                    f"{flag_pfx}test_case=rpc_soak",
                    f"{flag_pfx}soak_iterations={iters}",
                    f"{flag_pfx}soak_min_time_ms_between_rpcs=0",
                    f"{flag_pfx}soak_request_size={req_sz}",
                    f"{flag_pfx}soak_response_size={resp_sz}",
                ]
                if client_peer == "cpp":
                    c_args.extend([
                        f"--soak_max_failures=0",
                        f"--soak_overall_timeout_seconds={max(1, int(timeout_secs) - 5)}",
                        "--soak_per_iteration_max_acceptable_latency_ms=5000",
                    ])
                if verbose:
                    print(f"Spawning client ({client_peer}) {c_name}: {' '.join(c_args)}")

                usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
                t_start = time.monotonic()
                sub_proc = subprocess.Popen(
                    c_args,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
                _ACTIVE_PROCESSES.add(sub_proc)
                case_monitor = ProcessResourceMonitor(
                    server_pid=server_proc.pid, client_pid=sub_proc.pid, poll_interval_s=0.005
                )
                case_monitor.start()
                timed_out = False
                try:
                    sub_out, sub_err = sub_proc.communicate(timeout=timeout_secs)
                except subprocess.TimeoutExpired:
                    timed_out = True
                    sub_proc.kill()
                    sub_out, sub_err = sub_proc.communicate()
                finally:
                    _ACTIVE_PROCESSES.discard(sub_proc)
                    case_client_res, case_server_res, case_sat = case_monitor.stop()
                usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
                dur_s = time.monotonic() - t_start

                stdout_path = log_dir / f"{c_name}.stdout"
                stderr_path = log_dir / f"{c_name}.stderr"
                stdout_path.write_text(sub_out, encoding="utf-8")
                stderr_path.write_text(sub_err, encoding="utf-8")
                client_stdout += f"[{client_peer} {c_name}] {sub_out}\n"
                client_stderr += f"[{client_peer} {c_name}] {sub_err}\n"

                if timed_out:
                    return False, None, f"Client ({client_peer}) {c_name} exceeded {timeout_secs}s; logs: {stderr_path}"
                if sub_proc.returncode != 0:
                    return False, None, f"Client ({client_peer}) {c_name} failed (exit {sub_proc.returncode}); logs: {stderr_path}"

                try:
                    run_entry = parse_soak_output(sub_err, dur_s, req_sz, resp_sz, iters)
                except ValueError as e:
                    return False, None, f"Client ({client_peer}) {c_name} reported invalid soak results: {e}; logs: {stderr_path}"
                if not case_client_res["supported"] or not case_server_res["supported"]:
                    return False, None, f"Client ({client_peer}) {c_name} resource sampling unavailable; logs: {stderr_path}"

                case_client_res["user_cpu_seconds"] = round(
                    max(0.0, usage_after.ru_utime - usage_before.ru_utime), 6
                )
                case_client_res["system_cpu_seconds"] = round(
                    max(0.0, usage_after.ru_stime - usage_before.ru_stime), 6
                )
                case_client_res["user_cpu_nanos"] = int(case_client_res["user_cpu_seconds"] * 1e9)
                case_client_res["system_cpu_nanos"] = int(case_client_res["system_cpu_seconds"] * 1e9)
                case_client_res["method"] = "posix-child-rusage+sampled-rss"
                case_client_res["current_rss_bytes"] = None
                case_client_res["current_rss_mib"] = None
                client_samples.append(case_client_res)
                server_samples.append(case_server_res)
                saturation_samples.append(case_sat)
                run_entry["metrics"]["client_resources"] = case_client_res
                run_entry["metrics"]["server_resources"] = case_server_res
                run_entry["raw_logs"] = {
                    "stdout": str(stdout_path.relative_to(peer_registry.repo_root)),
                    "stderr": str(stderr_path.relative_to(peer_registry.repo_root)),
                }
                soak_runs.append(run_entry)
                p50_val = run_entry["latency"]["p50_nanos"]
                p99_val = run_entry["latency"]["p99_nanos"]
                client_stdout += f"{c_name} {client_peer}_p50={p50_val} {client_peer}_p99={p99_val}\n"

            client_res = aggregate_endpoint_resources(client_samples)
            server_res = aggregate_endpoint_resources(server_samples)
            sat_check = {
                "saturated": any(s["saturated"] for s in saturation_samples),
                "avg_cpu_pct": max(s["avg_cpu_pct"] for s in saturation_samples)
                    if all(s["avg_cpu_pct"] is not None for s in saturation_samples) else None,
                "peak_cpu_pct": max((s["peak_cpu_pct"] for s in saturation_samples
                                     if s["peak_cpu_pct"] is not None), default=None),
                "endpoint_saturation_checks": saturation_samples,
                "spare_capacity_pct": min(s["spare_capacity_pct"] for s in saturation_samples)
                    if all(s.get("headroom_verified") is True for s in saturation_samples) else None,
                "headroom_verified": all(s.get("headroom_verified") is True for s in saturation_samples),
                "status": "FAIL (UNVERIFIED)" if any(s.get("headroom_verified") is not True for s in saturation_samples)
                    else ("FAIL (SATURATED)" if any(s["saturated"] for s in saturation_samples) else "PASS"),
                "message": "Conservative worst-case across independently sampled reference client processes.",
            }

            report_data = {
                "schema_version": "1.0.0",
                "report_id": f"{client_peer}-{server_peer}-{int(time.time()*1000)}",
                "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "runs": soak_runs,
            }
        else:
            return False, None, f"Unsupported client peer: {client_peer}"

        if not client_res["supported"] or not server_res["supported"]:
            return False, None, f"Client ({client_peer}) or server ({server_peer}) resource sampling unavailable"
        if not isinstance(report_data, dict) or not isinstance(report_data.get("runs"), list) or not report_data["runs"]:
            preserve_report = temp_report_path.is_file()
            return False, None, f"Client ({client_peer}) produced no completed BenchmarkReport runs; raw report: {temp_report_path}"

        # 3. Enrich Report Data
        runs = report_data["runs"]
        for run in runs:
            metrics = run.get("metrics")
            if not isinstance(metrics, dict) or metrics.get("successful_rpcs", 0) <= 0 or metrics.get("failed_rpcs", 0):
                preserve_report = temp_report_path.is_file()
                return False, None, f"Client ({client_peer}) reported missing successes or failed calls; raw report: {temp_report_path}"
        if report_data and isinstance(report_data, dict):
            for run in runs:
                run["client_peer"] = client_peer
                run["server_peer"] = server_peer
                run["matching_configuration"] = MATCHING_CONFIGURATION
                # Resolved endpoint configuration: every crossed direction
                # records the actual codec, workload methodology, and binary
                # so same-codec transport cells are never confused with
                # end-to-end or soak-driven reference cells.
                run["client_codec"] = client_codec
                run["server_codec"] = server_codec
                run["client_workload"] = client_workload
                run["client_binary"] = client_binary
                run["server_binary"] = server_binary
                # BM-07: every crossed direction records its comparison
                # class and endpoint configuration differences, so
                # same-codec transport cells are never averaged with or
                # ranked against end-to-end reference cells.
                run["comparison_type"] = classify_comparison(
                    client_peer, server_peer, client_codec, server_codec, client_workload
                )
                run["configuration_gaps"] = configuration_gaps(
                    client_codec, server_codec, client_workload
                )
                metrics = run.setdefault("metrics", {})
                successful = metrics.get("successful_rpcs", 0)

                c_res = dict(metrics.get("client_resources", client_res))
                s_res = dict(metrics.get("server_resources", server_res))

                if successful > 0:
                    c_res["cpu_seconds_per_rpc"] = round(
                        (c_res["user_cpu_seconds"] + c_res["system_cpu_seconds"]) / successful, 9
                    )
                    s_res["cpu_seconds_per_rpc"] = round(
                        (s_res["user_cpu_seconds"] + s_res["system_cpu_seconds"]) / successful, 9
                    )

                metrics["client_cpu_seconds"] = round(
                    c_res["user_cpu_seconds"] + c_res["system_cpu_seconds"], 6
                )
                metrics["server_cpu_seconds"] = round(
                    s_res["user_cpu_seconds"] + s_res["system_cpu_seconds"], 6
                )
                metrics["client_resources"] = c_res
                metrics["server_resources"] = s_res

            report_data["client_resources"] = client_res
            report_data["server_resources"] = server_res
            report_data["client_saturation_check"] = sat_check
            exclusion = server_ceiling_exclusion(
                quick, client_workload, sat_check, client_res, server_res, runs
            )
            report_data["server_ceiling_valid"] = exclusion is None
            report_data["server_ceiling_reason"] = exclusion

        spare_label = f"{sat_check['spare_capacity_pct']:.1f}%" if sat_check["spare_capacity_pct"] is not None else "unverified"
        avg_label = f"{sat_check['avg_cpu_pct']:.1f}%" if sat_check["avg_cpu_pct"] is not None else "unverified"
        res_summary = (
            f"[ENDPOINTS] Client ({client_peer}): codec={client_codec}, workload={client_workload}, binary={client_binary}\n"
            f"[ENDPOINTS] Server ({server_peer}): codec={server_codec}, binary={server_binary}\n"
            f"[RESOURCES] Client ({client_peer}): user={client_res['user_cpu_seconds']:.4f}s, sys={client_res['system_cpu_seconds']:.4f}s, "
            f"peak_rss={client_res['peak_rss_mib']:.1f} MiB, threads={client_res['thread_count']}\n"
            f"[RESOURCES] Server ({server_peer}): user={server_res['user_cpu_seconds']:.4f}s, sys={server_res['system_cpu_seconds']:.4f}s, "
            f"peak_rss={server_res['peak_rss_mib']:.1f} MiB, threads={server_res['thread_count']}\n"
            f"[SATURATION] Load generator: avg_cpu={avg_label}, "
            f"spare_capacity={spare_label} -> {sat_check['status']}"
        )
        if sat_check["saturated"]:
            res_summary += (
                "\n[WARNING] Load generator headroom failed: server ceiling NOT valid for "
                f"server={server_peer}, client={client_peer} (see client_saturation_check)."
            )
        summary = f"{client_stdout.strip()}\n{res_summary}"
        return True, report_data, summary

    finally:
        if server_proc.poll() is None:
            server_proc.terminate()
            try:
                server_proc.wait(timeout=3.0)
            except subprocess.TimeoutExpired:
                server_proc.kill()
                server_proc.wait(timeout=1.0)
        _ACTIVE_PROCESSES.discard(server_proc)

        if server_proc.stdout:
            server_proc.stdout.close()
        if server_proc.stderr:
            server_proc.stderr.close()

        if not preserve_report and temp_report_path.is_file():
            try:
                temp_report_path.unlink()
            except OSError as e:
                print(f"Warning: could not remove temporary benchmark report {temp_report_path}: {e}", file=sys.stderr)


def format_table(title: str, headers: List[str], rows: List[List[str]]) -> str:
    """Format ASCII table with aligned columns and borders."""
    col_widths = [len(h) for h in headers]
    for row in rows:
        for i, val in enumerate(row):
            if i < len(col_widths):
                col_widths[i] = max(col_widths[i], len(str(val)))
            else:
                col_widths.append(len(str(val)))

    border_line = "+" + "+".join("-" * (w + 2) for w in col_widths) + "+"
    header_line = "| " + " | ".join(h.ljust(col_widths[i]) for i, h in enumerate(headers)) + " |"

    lines = [
        f"\n{'=' * len(border_line)}",
        f" {title}",
        f"{'=' * len(border_line)}",
        border_line,
        header_line,
        border_line,
    ]
    for row in rows:
        r_line = "| " + " | ".join(str(row[i]).ljust(col_widths[i]) for i in range(len(headers))) + " |"
        lines.append(r_line)
    lines.append(border_line)
    return "\n".join(lines)


def generate_comparison_tables(all_runs: List[Dict]) -> Tuple[Dict[str, Any], str]:
    """Generate isolation comparison tables:
    1. Server Efficiency (Fixed Load Generator)
    2. Client Efficiency (Fixed Server)

    Each axis splits into same-codec-transport and end-to-end tables.
    A numeric vs-native delta is emitted only when the row shares the
    baseline's load-generator workload; otherwise the row is listed as
    not comparable with its reason, never ranked as a win or loss.
    """
    records = []
    for run in all_runs:
        s_peer = run.get("server_peer", "unknown")
        c_peer = run.get("client_peer", run.get("transport", "unknown"))
        sc = run.get("scenario", {})
        shape_id = sc.get("id", "unknown")
        shape_name = sc.get("name", shape_id)
        metrics = run.get("metrics", {})
        latency = run.get("latency", {}) or {}

        qps = metrics.get("throughput_qps") or 0.0
        p50_us = (latency.get("p50_nanos") or 0) / 1000.0
        p99_us = (latency.get("p99_nanos") or 0) / 1000.0

        c_res = metrics.get("client_resources") or {}
        s_res = metrics.get("server_resources") or {}

        c_user = c_res.get("user_cpu_seconds", 0.0)
        c_sys = c_res.get("system_cpu_seconds", 0.0)
        c_tot = c_user + c_sys
        c_rss = c_res.get("peak_rss_mib", 0.0)
        c_cpu_per_rpc = c_res.get("cpu_seconds_per_rpc", 0.0)
        c_cpu_per_rpc_us = (c_cpu_per_rpc * 1e6) if c_cpu_per_rpc else 0.0
        c_eff = (metrics.get("successful_rpcs", 0) / c_tot) if c_tot > 0.0001 else 0.0

        s_user = s_res.get("user_cpu_seconds", 0.0)
        s_sys = s_res.get("system_cpu_seconds", 0.0)
        s_tot = s_user + s_sys
        s_rss = s_res.get("peak_rss_mib", 0.0)
        s_cpu_per_rpc = s_res.get("cpu_seconds_per_rpc", 0.0)
        s_cpu_per_rpc_us = (s_cpu_per_rpc * 1e6) if s_cpu_per_rpc else 0.0
        s_eff = (metrics.get("successful_rpcs", 0) / s_tot) if s_tot > 0.0001 else 0.0

        records.append({
            "server_peer": s_peer,
            "client_peer": c_peer,
            "shape_id": shape_id,
            "shape_name": shape_name,
            "comparison_type": run.get("comparison_type") or END_TO_END,
            "client_workload": run.get("client_workload") or "unknown",
            "configuration_gaps": run.get("configuration_gaps") or [],
            "qps": qps,
            "p50_us": p50_us,
            "p99_us": p99_us,
            "client_total_s": c_tot,
            "client_cpu_per_rpc_us": c_cpu_per_rpc_us,
            "client_rss_mib": c_rss,
            "client_eff": c_eff,
            "server_total_s": s_tot,
            "server_cpu_per_rpc_us": s_cpu_per_rpc_us,
            "server_rss_mib": s_rss,
            "server_eff": s_eff,
        })

    tables_json: Dict[str, Any] = {
        "server_efficiency": [],
        "client_efficiency": [],
    }
    output_text_parts = []

    # 1. Server Efficiency Table (Fixed Load Generator / Client)
    # BM-07: groups split by comparison type so same-codec transport
    # cells never share a table with end-to-end reference cells.
    server_groups: Dict[Tuple[str, str, str], List[Dict]] = {}
    for r in records:
        key = (r["client_peer"], r["shape_id"], r["comparison_type"])
        server_groups.setdefault(key, []).append(r)

    srv_headers = [
        "Server Peer", "Shape", "Throughput", "p50 Latency", "p99 Latency",
        "Server CPU", "CPU/RPC", "Peak RSS", "Server Eff", "vs Native Delta"
    ]

    for (c_peer, shape_id, comparison), group in server_groups.items():
        shape_disp = group[0]["shape_name"]
        title = (
            f"SERVER EFFICIENCY COMPARISON [{comparison}] "
            f"(Fixed Load Generator: client={c_peer}, shape={shape_disp})"
        )
        base = next((r for r in group if r["server_peer"] == "native"), None)
        if base is None:
            # End-to-end groups exclude the native pair (classified
            # same-codec); pull it in as the fixed-generator baseline so
            # reference servers still compare against native, not each other.
            base = next(
                (
                    r
                    for r in records
                    if r["client_peer"] == c_peer
                    and r["server_peer"] == "native"
                    and r["shape_id"] == shape_id
                ),
                None,
            )
        display_group = list(group)
        if base is not None and all(
            r["server_peer"] != base["server_peer"] or r["client_peer"] != base["client_peer"]
            for r in display_group
        ):
            display_group.insert(0, base)

        rows = []
        group_results = []
        for r in display_group:
            s_peer = r["server_peer"]
            comparable = base is not None and _workloads_match(
                r["client_workload"], base["client_workload"]
            )
            incomparable_reason = ""
            if r is base or (s_peer == "native" and r["client_peer"] == c_peer):
                delta_str = "Baseline"
                comparable = True
            elif base is None:
                delta_str = "N/A (no native baseline: incomplete matrix)"
            elif not comparable:
                incomparable_reason = (
                    f"load generator differs: {r['client_workload']} vs {base['client_workload']}"
                )
                delta_str = f"not comparable ({incomparable_reason})"
            else:
                p50_diff = ((r["p50_us"] - base["p50_us"]) / base["p50_us"] * 100.0) if base["p50_us"] > 0 else 0.0
                cpu_diff = ((r["server_cpu_per_rpc_us"] - base["server_cpu_per_rpc_us"]) / base["server_cpu_per_rpc_us"] * 100.0) if base["server_cpu_per_rpc_us"] > 0 else 0.0
                delta_str = f"{'+' if p50_diff >= 0 else ''}{p50_diff:.1f}% p50, {'+' if cpu_diff >= 0 else ''}{cpu_diff:.1f}% CPU"

            row = [
                s_peer,
                shape_id,
                f"{r['qps']:,.0f} QPS" if r['qps'] > 0 else "N/A",
                f"{r['p50_us']:.1f} µs" if r['p50_us'] > 0 else "N/A",
                f"{r['p99_us']:.1f} µs" if r['p99_us'] > 0 else "N/A",
                f"{r['server_total_s']:.4f} s",
                f"{r['server_cpu_per_rpc_us']:.2f} µs" if r['server_cpu_per_rpc_us'] > 0 else "N/A",
                f"{r['server_rss_mib']:.1f} MiB",
                f"{r['server_eff']:,.0f} RPC/s-CPU" if r['server_eff'] > 0 else "N/A",
                delta_str,
            ]
            rows.append(row)
            group_results.append({
                "server_peer": s_peer,
                "shape": shape_id,
                "comparison_type": r["comparison_type"],
                "client_workload": r["client_workload"],
                "configuration_gaps": r["configuration_gaps"],
                "throughput_qps": r["qps"],
                "p50_us": r["p50_us"],
                "p99_us": r["p99_us"],
                "server_cpu_seconds": r["server_total_s"],
                "server_cpu_per_rpc_us": r["server_cpu_per_rpc_us"],
                "server_peak_rss_mib": r["server_rss_mib"],
                "server_efficiency_rpc_per_cpu_sec": r["server_eff"],
                "comparable": comparable,
                "incomparability_reason": incomparable_reason,
                "delta_vs_native": delta_str,
            })

        output_text_parts.append(format_table(title, srv_headers, rows))
        tables_json["server_efficiency"].append({
            "fixed_client": c_peer,
            "shape": shape_id,
            "comparison_type": comparison,
            "baseline_pair": (
                {"client_peer": base["client_peer"], "server_peer": base["server_peer"]}
                if base is not None
                else None
            ),
            "results": group_results,
        })

    # 2. Client Efficiency Table (Fixed Server)
    # BM-07: same comparison-type split as the server tables. Reference
    # soak clients drive a different workload than the rpc-bench load
    # generator, so they are listed but never ranked against it.
    client_groups: Dict[Tuple[str, str, str], List[Dict]] = {}
    for r in records:
        key = (r["server_peer"], r["shape_id"], r["comparison_type"])
        client_groups.setdefault(key, []).append(r)

    cli_headers = [
        "Client Peer", "Shape", "Throughput", "p50 Latency", "p99 Latency",
        "Client CPU", "CPU/RPC", "Peak RSS", "Client Eff", "vs Native Delta"
    ]

    for (s_peer, shape_id, comparison), group in client_groups.items():
        shape_disp = group[0]["shape_name"]
        title = (
            f"CLIENT EFFICIENCY COMPARISON [{comparison}] "
            f"(Fixed Server: server={s_peer}, shape={shape_disp})"
        )
        base = next((r for r in group if r["client_peer"] == "native"), None)
        if base is None:
            # End-to-end groups exclude the native pair (classified
            # same-codec); pull it in as the fixed-server baseline so the
            # table names what reference clients are held against.
            base = next(
                (
                    r
                    for r in records
                    if r["server_peer"] == s_peer
                    and r["client_peer"] == "native"
                    and r["shape_id"] == shape_id
                ),
                None,
            )
        display_group = list(group)
        if base is not None and all(
            r["client_peer"] != base["client_peer"] or r["server_peer"] != base["server_peer"]
            for r in display_group
        ):
            display_group.insert(0, base)

        rows = []
        group_results = []
        for r in display_group:
            c_peer = r["client_peer"]
            comparable = base is not None and _workloads_match(
                r["client_workload"], base["client_workload"]
            )
            incomparable_reason = ""
            if r is base or (c_peer == "native" and r["server_peer"] == s_peer):
                delta_str = "Baseline"
                comparable = True
            elif base is None:
                delta_str = "N/A (no native baseline: incomplete matrix)"
            elif not comparable:
                incomparable_reason = (
                    f"load generator differs: {r['client_workload']} vs {base['client_workload']}"
                )
                delta_str = f"not comparable ({incomparable_reason})"
            else:
                p50_diff = ((r["p50_us"] - base["p50_us"]) / base["p50_us"] * 100.0) if base["p50_us"] > 0 else 0.0
                cpu_diff = ((r["client_cpu_per_rpc_us"] - base["client_cpu_per_rpc_us"]) / base["client_cpu_per_rpc_us"] * 100.0) if base["client_cpu_per_rpc_us"] > 0 else 0.0
                delta_str = f"{'+' if p50_diff >= 0 else ''}{p50_diff:.1f}% p50, {'+' if cpu_diff >= 0 else ''}{cpu_diff:.1f}% CPU"

            row = [
                c_peer,
                shape_id,
                f"{r['qps']:,.0f} QPS" if r['qps'] > 0 else "N/A",
                f"{r['p50_us']:.1f} µs" if r['p50_us'] > 0 else "N/A",
                f"{r['p99_us']:.1f} µs" if r['p99_us'] > 0 else "N/A",
                f"{r['client_total_s']:.4f} s",
                f"{r['client_cpu_per_rpc_us']:.2f} µs" if r['client_cpu_per_rpc_us'] > 0 else "N/A",
                f"{r['client_rss_mib']:.1f} MiB",
                f"{r['client_eff']:,.0f} RPC/s-CPU" if r['client_eff'] > 0 else "N/A",
                delta_str,
            ]
            rows.append(row)
            group_results.append({
                "client_peer": c_peer,
                "shape": shape_id,
                "comparison_type": r["comparison_type"],
                "client_workload": r["client_workload"],
                "configuration_gaps": r["configuration_gaps"],
                "throughput_qps": r["qps"],
                "p50_us": r["p50_us"],
                "p99_us": r["p99_us"],
                "client_cpu_seconds": r["client_total_s"],
                "client_cpu_per_rpc_us": r["client_cpu_per_rpc_us"],
                "client_peak_rss_mib": r["client_rss_mib"],
                "client_efficiency_rpc_per_cpu_sec": r["client_eff"],
                "comparable": comparable,
                "incomparability_reason": incomparable_reason,
                "delta_vs_native": delta_str,
            })

        output_text_parts.append(format_table(title, cli_headers, rows))
        tables_json["client_efficiency"].append({
            "fixed_server": s_peer,
            "shape": shape_id,
            "comparison_type": comparison,
            "baseline_pair": (
                {"client_peer": base["client_peer"], "server_peer": base["server_peer"]}
                if base is not None
                else None
            ),
            "results": group_results,
        })

    return tables_json, "\n".join(output_text_parts)


TONIC_PBRS = "tonic-pbrs"
TONIC_PROST = "tonic-prost"

# Codec each matrix peer role actually executes. "tonic" is a legacy alias
# whose server codec depends on binary availability (recorded per run);
# explicit flavors are required for apples-to-apples codec claims.
PEER_CODECS: Dict[str, str] = {
    "native": "pbrs",
    "tonic": "mixed (server: prost, client: pbrs)",
    TONIC_PBRS: "pbrs",
    TONIC_PROST: "prost",
    "go": "google.golang.org/protobuf",
    "cpp": "google::protobuf (upb/C++)",
}

SAME_CODEC_TRANSPORT = "same-codec-transport"
END_TO_END = "end-to-end"

# Peer roles that execute the pbrs codec under the rpc-bench load
# generator. Only cells with both endpoints in this set isolate the
# transport; every other crossed direction is an end-to-end comparison.
SAME_CODEC_PEERS = frozenset({"native", TONIC_PBRS})


def classify_comparison(
    client_peer: str,
    server_peer: str,
    client_codec: str,
    server_codec: str,
    client_workload: str,
) -> str:
    """Classify a crossed cell as transport-only or end-to-end.

    A same-codec transport cell needs the pbrs codec on both endpoints AND
    the shared rpc-bench load generator. Anything else (prost/go/cpp codec,
    interop-soak workload, legacy alias) is an end-to-end comparison.
    """
    if (
        client_workload == "rpc-bench"
        and client_peer in SAME_CODEC_PEERS
        and server_peer in SAME_CODEC_PEERS
        and client_codec == "pbrs"
        and server_codec == "pbrs"
    ):
        return SAME_CODEC_TRANSPORT
    return END_TO_END


def configuration_gaps(
    client_codec: str, server_codec: str, client_workload: str
) -> List[str]:
    """Name the configuration differences between the two endpoints.

    An empty list means both endpoints ran the identical codec under the
    identical load generator. A non-empty list is recorded on the run and
    disqualifies numeric cross-cell deltas against a different workload.
    """
    gaps = []
    if client_codec != server_codec:
        gaps.append(f"codec differs: client={client_codec} server={server_codec}")
    if client_workload != "rpc-bench":
        gaps.append(f"load generator differs: {client_workload} vs rpc-bench open-loop")
    return gaps


def _workloads_match(row_workload: str, base_workload: str) -> bool:
    """Two cells share a load generator only when both workloads are known equal."""
    return (
        bool(row_workload)
        and row_workload != "unknown"
        and row_workload == base_workload
    )


def normalize_peer(name: str) -> str:
    """Normalize a peer role spelling (underscores accepted, legacy kept)."""
    p = name.strip().lower().replace("_", "-")
    return p


def parse_peers(arg_val: Optional[str], default_peers: List[str], role: str) -> List[str]:
    """Parse comma-separated peer list."""
    if role not in ("client", "server"):
        raise ValueError(f"Unknown benchmark peer role: {role}")
    valid = ("native", "tonic", TONIC_PBRS, TONIC_PROST, "go", "cpp")
    all_peers = ["native", TONIC_PBRS, "go", "cpp"]
    if role == "server":
        all_peers.insert(2, TONIC_PROST)
    if not arg_val:
        return default_peers
    val = arg_val.strip().lower()
    if val in ("all", "both"):
        return all_peers if val == "all" else ["native", "tonic"]
    result = []
    for item in val.split(","):
        p = normalize_peer(item)
        if p in valid:
            if p not in result:
                result.append(p)
        elif p == "both":
            for b in ("native", "tonic"):
                if b not in result:
                    result.append(b)
        elif p == "all":
            for b in all_peers:
                if b not in result:
                    result.append(b)
        else:
            raise ValueError(
                f"Unknown peer: '{item}'. Valid peers: native, tonic, "
                f"{TONIC_PBRS}, {TONIC_PROST}, go, cpp"
            )
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run multi-process gRPC benchmarks separating client and server OS processes with pinned peers."
    )
    parser.add_argument(
        "--binary",
        type=str,
        default=None,
        help="Path to rpc-bench binary (auto-detected if omitted).",
    )
    parser.add_argument(
        "--quick",
        action="store_true",
        help="Run fast abbreviated benchmark with reduced iterations.",
    )
    parser.add_argument(
        "--server-peer",
        "--server",
        dest="server_peer",
        type=str,
        default=None,
        help="Server peer(s): native, tonic (legacy alias), tonic-pbrs (same-codec), tonic-prost (end-to-end), go, cpp, or comma-separated list (default: native in quick mode, native/tonic in standard mode).",
    )
    parser.add_argument(
        "--client-peer",
        "--client",
        dest="client_peer",
        type=str,
        default=None,
        help="Client peer(s): native, tonic (legacy alias, pbrs codec), tonic-pbrs (same-codec), go, cpp, or comma-separated list (tonic-prost has no load generator and fails the cell explicitly).",
    )
    parser.add_argument(
        "--shape",
        choices=["unary", "stream", "ping_pong", "upload", "qps", "all"],
        default=None,
        help="Call shape to benchmark (default: unary in quick mode, all in standard mode).",
    )
    parser.add_argument(
        "--host",
        type=str,
        default="127.0.0.1",
        help="Host address for server to bind and client to connect (default: 127.0.0.1).",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=0,
        help="Port for server to bind (default: 0 for dynamic random OS port).",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=60.0,
        help="Timeout in seconds for readiness and execution (default: 60.0).",
    )
    parser.add_argument(
        "--output",
        "-o",
        type=str,
        default=None,
        help="Save aggregated benchmark report JSON to this file path.",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Enable verbose output of commands and PIDs.",
    )
    parser.add_argument(
        "--peer-dir",
        type=str,
        default=None,
        help="Path to peer configuration directory (default: rpc-bench/peers).",
    )

    args = parser.parse_args()
    repo_root = Path(__file__).resolve().parent.parent

    peer_dir = Path(args.peer_dir) if args.peer_dir else None
    peer_registry = PeerRegistry(repo_root, peer_dir)

    quick = args.quick
    shape = args.shape or ("unary" if quick else "all")

    # Determine peer combinations
    if args.server_peer or args.client_peer:
        servers = parse_peers(args.server_peer, ["native"], role="server")
        clients = parse_peers(args.client_peer, ["native"], role="client")
        pairs = []
        for s in servers:
            for c in clients:
                pairs.append((s, c))
    else:
        if quick:
            pairs = [("native", "native")]
        else:
            pairs = [("native", "native"), ("tonic", "tonic")]

    print("=" * 80)
    print("  RPC-BENCH APPLES-TO-APPLES MATRIX HARNESS")
    print("=" * 80)
    print(f"  TCP_NODELAY:         {MATCHING_CONFIGURATION['tcp_nodelay']}")
    print(f"  TLS Cipher:          {MATCHING_CONFIGURATION['tls_cipher']}")
    print(f"  Connections:         {MATCHING_CONFIGURATION['connections']}")
    print(f"  Concurrency:         {MATCHING_CONFIGURATION['concurrency']}")
    print(f"  Compression:         {MATCHING_CONFIGURATION['compression']}")
    print(f"  Cores:               {MATCHING_CONFIGURATION['cores']}")
    print(f"  HTTP/2 Windows:      {MATCHING_CONFIGURATION['http2_windows']}")
    print(f"  Message Limits:      {MATCHING_CONFIGURATION['message_limits']}")
    print(f"  Deadlines:           {MATCHING_CONFIGURATION['deadlines']}")
    print(f"  Handler Work:        {MATCHING_CONFIGURATION['handler_work']}")
    print(f"  Empty Unary Payload: {MATCHING_CONFIGURATION['payload_sizes']['empty_unary']['request_bytes']}B req / {MATCHING_CONFIGURATION['payload_sizes']['empty_unary']['response_bytes']}B resp")
    print(f"  Large Unary Payload: {MATCHING_CONFIGURATION['payload_sizes']['large_unary']['request_bytes']}B req / {MATCHING_CONFIGURATION['payload_sizes']['large_unary']['response_bytes']}B resp")
    print(f"  Streaming Payload:   {MATCHING_CONFIGURATION['payload_sizes']['stream']['message_bytes']}B / message")
    print(f"  Ping-Pong:           {MATCHING_CONFIGURATION['payload_sizes']['ping_pong']['round_trips']} round-trips")
    print(f"  Upload Payload:      {MATCHING_CONFIGURATION['payload_sizes']['upload']['message_bytes']}B / message")
    print(f"  Evaluated Pairs:     {len(pairs)} ({', '.join(f'{s}->{c}' for s, c in pairs)})")
    cpu_constraints = collect_cpu_constraints()
    print(f"  Effective CPUs:      {cpu_constraints['effective_cpu_count']} (source: {cpu_constraints['source']})")
    if cpu_constraints["affinity_count"] is not None:
        print(f"  CPU Affinity:        {cpu_constraints['affinity_count']} CPUs ({cpu_constraints['affinity_cpus']})")
    if cpu_constraints["cgroup_quota_millicpus"] is not None:
        print(f"  CGroup CPU Quota:    {cpu_constraints['cgroup_quota_millicpus'] / 1000.0:.2f} CPUs")
    print("=" * 80)

    all_runs: List[Dict] = []
    pair_saturation_checks: List[Dict] = []
    completed_pairs: List[Tuple[str, str]] = []
    failed_pairs: List[Dict[str, str]] = []
    failed = False

    for s_peer, c_peer in pairs:
        print(f"\n--- Running: server={s_peer}, client={c_peer}, shape={shape} ---")
        ok, report_json, summary = run_single_benchmark(
            peer_registry=peer_registry,
            server_peer=s_peer,
            client_peer=c_peer,
            shape=shape,
            host=args.host,
            port=args.port,
            quick=quick,
            timeout_secs=args.timeout,
            verbose=args.verbose,
            binary_override=args.binary,
        )

        if not ok:
            print(f"[FAIL] server={s_peer}, client={c_peer} failed:\n{summary}", file=sys.stderr)
            failed_pairs.append({
                "server_peer": s_peer,
                "client_peer": c_peer,
                "reason": summary.splitlines()[0] if summary else "unknown",
            })
            failed = True
            break
        completed_pairs.append((s_peer, c_peer))

        print(summary)
        print(f"[PASS] server={s_peer}, client={c_peer}")

        if report_json and "runs" in report_json:
            all_runs.extend(report_json["runs"])
            pair_saturation_checks.append({
                "server_peer": s_peer,
                "client_peer": c_peer,
                "server_ceiling_valid": report_json.get("server_ceiling_valid"),
                "server_ceiling_reason": report_json.get("server_ceiling_reason"),
                "client_saturation_check": report_json.get("client_saturation_check"),
            })

    # A missing peer yields an incomplete matrix, never a partial win: name
    # every completed, failed, and skipped pair explicitly.
    attempted = {(s, c) for s, c in completed_pairs} | {
        (f["server_peer"], f["client_peer"]) for f in failed_pairs
    }
    skipped_pairs = [
        {"server_peer": s, "client_peer": c}
        for s, c in pairs
        if (s, c) not in attempted
    ]
    if failed:
        missing_str = ", ".join(
            f"{f['server_peer']}->{f['client_peer']}" for f in failed_pairs
        ) + "".join(f", {s['server_peer']}->{s['client_peer']} (skipped)" for s in skipped_pairs)
        print(
            f"\n[INCOMPLETE MATRIX] {len(completed_pairs)}/{len(pairs)} pair(s) completed; "
            f"missing: {missing_str}. Partial tables below are NOT a comparison win.",
        )

    # Output comparison tables
    tables_json, tables_text = generate_comparison_tables(all_runs)
    if tables_text:
        print(tables_text)

    if args.output and (all_runs or failed):
        first_run = all_runs[0] if all_runs else {}
        aggregated_report = {
            "schema_version": "1.0.0",
            "report_id": f"matrix-report-{int(time.time() * 1000)}",
            "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "git_commit": first_run.get("git_commit", "unknown"),
            "host_info": first_run.get("host_info", {}),
            "matching_configuration": MATCHING_CONFIGURATION,
            "matrix_complete": not failed,
            "completed_pairs": [
                {"server_peer": s, "client_peer": c} for s, c in completed_pairs
            ],
            "failed_pairs": failed_pairs,
            "skipped_pairs": skipped_pairs,
            "cpu_constraints": cpu_constraints,
            "pair_saturation_checks": pair_saturation_checks,
            "server_ceiling_valid": (
                all(p.get("server_ceiling_valid") for p in pair_saturation_checks)
                if pair_saturation_checks
                else None
            ),
            "runs": all_runs,
            "comparison_tables": tables_json,
            "client_resources": first_run.get("metrics", {}).get("client_resources"),
            "server_resources": first_run.get("metrics", {}).get("server_resources"),
        }
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(aggregated_report, f, indent=2)
        print(f"\nSaved aggregated BenchmarkReport with {len(all_runs)} runs to {out_path}")

    invalid_ceilings = [p for p in pair_saturation_checks if not p.get("server_ceiling_valid")]
    if invalid_ceilings:
        pairs_str = ", ".join(
            f"{p['server_peer']}->{p['client_peer']}: {p['server_ceiling_reason']}"
            for p in invalid_ceilings
        )
        print(
            f"\n[NOT QUALIFIED] Server ceiling not established for pair(s): {pairs_str}. "
            "These results are diagnostic, not leadership evidence.",
            file=sys.stderr,
        )

    if failed:
        print("\nRPC-Bench matrix execution encountered failures.", file=sys.stderr)
        return 1

    print(f"\nRPC-Bench matrix completed successfully ({len(pairs)} pair(s) passed).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
