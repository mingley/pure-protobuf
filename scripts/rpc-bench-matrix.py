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

        # Capture server initial baseline immediately upon monitor creation
        self.server_initial = sample_process(self.server_pid)
        if self.server_initial:
            self.server_peak_rss = self.server_initial.rss_bytes
            self.server_max_threads = self.server_initial.thread_count
            self._server_last_snap = self.server_initial

    def set_client_pid(self, client_pid: int) -> None:
        self.client_pid = client_pid
        self.client_initial = sample_process(client_pid)
        if self.client_initial:
            self.client_peak_rss = self.client_initial.rss_bytes
            self.client_max_threads = self.client_initial.thread_count
            self._client_last_snap = self.client_initial

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

    def stop(self) -> Tuple[Dict, Dict, Dict]:
        self._stop_event.set()
        if self._worker_thread and self._worker_thread.is_alive():
            self._worker_thread.join(timeout=1.0)
        self.end_time = time.monotonic()

        # Capture final snapshots
        if self.client_pid:
            final_c = sample_process(self.client_pid)
            if final_c:
                self.client_final = final_c
                self.client_peak_rss = max(self.client_peak_rss, final_c.rss_bytes)
                self.client_max_threads = max(self.client_max_threads, final_c.thread_count)

        if self.server_pid:
            final_s = sample_process(self.server_pid)
            if final_s:
                self.server_final = final_s
                self.server_peak_rss = max(self.server_peak_rss, final_s.rss_bytes)
                self.server_max_threads = max(self.server_max_threads, final_s.thread_count)

        duration_s = max(0.001, self.end_time - self.start_time)

        # 1. Compute client metrics
        c_init_user = self.client_initial.user_s if self.client_initial else 0.0
        c_init_sys = self.client_initial.sys_s if self.client_initial else 0.0
        c_final_user = self.client_final.user_s if self.client_final else (self._client_last_snap.user_s if self._client_last_snap else c_init_user)
        c_final_sys = self.client_final.sys_s if self.client_final else (self._client_last_snap.sys_s if self._client_last_snap else c_init_sys)

        client_user_s = max(0.0, c_final_user - c_init_user)
        client_sys_s = max(0.0, c_final_sys - c_init_sys)
        client_total_s = client_user_s + client_sys_s
        client_peak_rss = max(self.client_peak_rss, 1024)
        client_threads = max(self.client_max_threads, 1)

        # 2. Compute server metrics
        s_init_user = self.server_initial.user_s if self.server_initial else 0.0
        s_init_sys = self.server_initial.sys_s if self.server_initial else 0.0
        s_final_user = self.server_final.user_s if self.server_final else (self._server_last_snap.user_s if self._server_last_snap else s_init_user)
        s_final_sys = self.server_final.sys_s if self.server_final else (self._server_last_snap.sys_s if self._server_last_snap else s_init_sys)

        server_user_s = max(0.0, s_final_user - s_init_user)
        server_sys_s = max(0.0, s_final_sys - s_init_sys)
        server_total_s = server_user_s + server_sys_s
        server_peak_rss = max(self.server_peak_rss, 1024)
        server_threads = max(self.server_max_threads, 1)

        # 3. Client saturation verification
        # Utilization relative to available duration & client thread count
        overall_utilization_pct = (client_total_s / duration_s) * 100.0
        if self.client_cpu_samples:
            avg_cpu_pct = sum(self.client_cpu_samples) / len(self.client_cpu_samples)
            peak_cpu_pct = max(self.client_cpu_samples)
        else:
            avg_cpu_pct = overall_utilization_pct
            peak_cpu_pct = overall_utilization_pct

        # Spare capacity: 100% minus the fraction of client core capacity utilized
        spare_capacity_pct = max(0.0, 100.0 - (avg_cpu_pct / float(client_threads)))
        is_saturated = (spare_capacity_pct < (100.0 - self.saturation_threshold_pct)) or (
            avg_cpu_pct >= self.saturation_threshold_pct * float(client_threads)
        )

        saturation_status = "FAIL (SATURATED)" if is_saturated else "PASS"
        if is_saturated:
            saturation_message = (
                f"Client load generator reached {avg_cpu_pct:.1f}% CPU utilization "
                f"(spare capacity: {spare_capacity_pct:.1f}%). Load generator saturated its capacity; "
                f"measured throughput may reflect client generation limits rather than true server ceiling."
            )
        else:
            saturation_message = (
                f"Client load generator maintained {spare_capacity_pct:.1f}% spare CPU capacity "
                f"(avg CPU: {avg_cpu_pct:.1f}% across {client_threads} threads). "
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
            "cpu_seconds_per_rpc": None,
            "method": server_method,
            "supported": server_supported,
        }

        saturation_dict = {
            "saturated": is_saturated,
            "avg_cpu_pct": round(avg_cpu_pct, 1),
            "peak_cpu_pct": round(peak_cpu_pct, 1),
            "spare_capacity_pct": round(spare_capacity_pct, 1),
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


MATCHING_CONFIGURATION = {
    "tcp_nodelay": True,
    "tls_cipher": "TLS_AES_128_GCM_SHA256",
    "connections": 1,
    "concurrency": 1,
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
        repo_root / "rpc-bench" / "target" / "release" / "rpc-bench",
        repo_root / "target" / "release" / "rpc-bench",
        repo_root / "rpc-bench" / "target" / "debug" / "rpc-bench",
        repo_root / "target" / "debug" / "rpc-bench",
    ]
    for c in candidates:
        if c.is_file() and os.access(c, os.X_OK):
            return c.resolve()

    raise FileNotFoundError(
        "rpc-bench binary not found. Build it with: cargo build --manifest-path rpc-bench/Cargo.toml"
    )


def collect_cpu_constraints() -> Dict[str, Any]:
    """Record the effective core quota/affinity the matrix runs under.

    Mirrors the Rust `CpuConstraints` record: unknown values stay `None`
    (never measured zeros) and `source` names the detection path explicitly.
    """
    info: Dict[str, Any] = {
        "effective_cpu_count": None,
        "affinity_cpus": None,
        "affinity_count": None,
        "cgroup_quota_millicpus": None,
        "source": "unsupported",
    }

    try:
        if hasattr(os, "process_cpu_count"):
            info["effective_cpu_count"] = os.process_cpu_count()
        else:
            info["effective_cpu_count"] = os.cpu_count()
        info["source"] = "os-cpu-count"
    except Exception:
        pass

    if hasattr(os, "sched_affinity"):
        try:
            cpus = sorted(os.sched_affinity(0))
            info["affinity_cpus"] = ",".join(str(c) for c in cpus)
            info["affinity_count"] = len(cpus)
            info["source"] = "linux-sched-affinity"
        except Exception:
            pass

    try:
        with open("/sys/fs/cgroup/cpu.max", "r", encoding="utf-8") as f:
            parts = f.read().split()
        if len(parts) == 2 and parts[0] != "max":
            quota, period = int(parts[0]), int(parts[1])
            if period > 0:
                info["cgroup_quota_millicpus"] = (quota * 1000) // period
    except Exception:
        try:
            with open("/sys/fs/cgroup/cpu/cpu.cfs_quota_us", "r", encoding="utf-8") as f:
                quota = int(f.read().strip())
            with open("/sys/fs/cgroup/cpu/cpu.cfs_period_us", "r", encoding="utf-8") as f:
                period = int(f.read().strip())
            if quota >= 0 and period > 0:
                info["cgroup_quota_millicpus"] = (quota * 1000) // period
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
                "avg_cpu_pct": max(s["avg_cpu_pct"] for s in saturation_samples),
                "peak_cpu_pct": max(s["peak_cpu_pct"] for s in saturation_samples),
                "spare_capacity_pct": min(s["spare_capacity_pct"] for s in saturation_samples),
                "status": "FAIL (SATURATED)" if any(s["saturated"] for s in saturation_samples) else "PASS",
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

        res_summary = (
            f"[ENDPOINTS] Client ({client_peer}): codec={client_codec}, workload={client_workload}, binary={client_binary}\n"
            f"[ENDPOINTS] Server ({server_peer}): codec={server_codec}, binary={server_binary}\n"
            f"[RESOURCES] Client ({client_peer}): user={client_res['user_cpu_seconds']:.4f}s, sys={client_res['system_cpu_seconds']:.4f}s, "
            f"peak_rss={client_res['peak_rss_mib']:.1f} MiB, threads={client_res['thread_count']}\n"
            f"[RESOURCES] Server ({server_peer}): user={server_res['user_cpu_seconds']:.4f}s, sys={server_res['system_cpu_seconds']:.4f}s, "
            f"peak_rss={server_res['peak_rss_mib']:.1f} MiB, threads={server_res['thread_count']}\n"
            f"[SATURATION] Load generator: avg_cpu={sat_check['avg_cpu_pct']:.1f}%, "
            f"spare_capacity={sat_check['spare_capacity_pct']:.1f}% -> {sat_check['status']}"
        )
        if sat_check["saturated"]:
            res_summary += (
                "\n[WARNING] Load generator saturated: server ceiling NOT valid for "
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
    server_groups: Dict[Tuple[str, str], List[Dict]] = {}
    for r in records:
        key = (r["client_peer"], r["shape_id"])
        server_groups.setdefault(key, []).append(r)

    srv_headers = [
        "Server Peer", "Shape", "Throughput", "p50 Latency", "p99 Latency",
        "Server CPU", "CPU/RPC", "Peak RSS", "Server Eff", "vs Native Delta"
    ]

    for (c_peer, shape_id), group in server_groups.items():
        shape_disp = group[0]["shape_name"]
        title = f"SERVER EFFICIENCY COMPARISON (Fixed Load Generator: client={c_peer}, shape={shape_disp})"
        base = next((r for r in group if r["server_peer"] == "native"), None)

        rows = []
        group_results = []
        for r in group:
            s_peer = r["server_peer"]
            if s_peer == "native":
                delta_str = "Baseline"
            elif base:
                p50_diff = ((r["p50_us"] - base["p50_us"]) / base["p50_us"] * 100.0) if base["p50_us"] > 0 else 0.0
                cpu_diff = ((r["server_cpu_per_rpc_us"] - base["server_cpu_per_rpc_us"]) / base["server_cpu_per_rpc_us"] * 100.0) if base["server_cpu_per_rpc_us"] > 0 else 0.0
                delta_str = f"{'+' if p50_diff >= 0 else ''}{p50_diff:.1f}% p50, {'+' if cpu_diff >= 0 else ''}{cpu_diff:.1f}% CPU"
            else:
                delta_str = "N/A"

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
                "throughput_qps": r["qps"],
                "p50_us": r["p50_us"],
                "p99_us": r["p99_us"],
                "server_cpu_seconds": r["server_total_s"],
                "server_cpu_per_rpc_us": r["server_cpu_per_rpc_us"],
                "server_peak_rss_mib": r["server_rss_mib"],
                "server_efficiency_rpc_per_cpu_sec": r["server_eff"],
                "delta_vs_native": delta_str,
            })

        output_text_parts.append(format_table(title, srv_headers, rows))
        tables_json["server_efficiency"].append({
            "fixed_client": c_peer,
            "shape": shape_id,
            "results": group_results,
        })

    # 2. Client Efficiency Table (Fixed Server)
    client_groups: Dict[Tuple[str, str], List[Dict]] = {}
    for r in records:
        key = (r["server_peer"], r["shape_id"])
        client_groups.setdefault(key, []).append(r)

    cli_headers = [
        "Client Peer", "Shape", "Throughput", "p50 Latency", "p99 Latency",
        "Client CPU", "CPU/RPC", "Peak RSS", "Client Eff", "vs Native Delta"
    ]

    for (s_peer, shape_id), group in client_groups.items():
        shape_disp = group[0]["shape_name"]
        title = f"CLIENT EFFICIENCY COMPARISON (Fixed Server: server={s_peer}, shape={shape_disp})"
        base = next((r for r in group if r["client_peer"] == "native"), None)

        rows = []
        group_results = []
        for r in group:
            c_peer = r["client_peer"]
            if c_peer == "native":
                delta_str = "Baseline"
            elif base:
                p50_diff = ((r["p50_us"] - base["p50_us"]) / base["p50_us"] * 100.0) if base["p50_us"] > 0 else 0.0
                cpu_diff = ((r["client_cpu_per_rpc_us"] - base["client_cpu_per_rpc_us"]) / base["client_cpu_per_rpc_us"] * 100.0) if base["client_cpu_per_rpc_us"] > 0 else 0.0
                delta_str = f"{'+' if p50_diff >= 0 else ''}{p50_diff:.1f}% p50, {'+' if cpu_diff >= 0 else ''}{cpu_diff:.1f}% CPU"
            else:
                delta_str = "N/A"

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
                "throughput_qps": r["qps"],
                "p50_us": r["p50_us"],
                "p99_us": r["p99_us"],
                "client_cpu_seconds": r["client_total_s"],
                "client_cpu_per_rpc_us": r["client_cpu_per_rpc_us"],
                "client_peak_rss_mib": r["client_rss_mib"],
                "client_efficiency_rpc_per_cpu_sec": r["client_eff"],
                "delta_vs_native": delta_str,
            })

        output_text_parts.append(format_table(title, cli_headers, rows))
        tables_json["client_efficiency"].append({
            "fixed_server": s_peer,
            "shape": shape_id,
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
