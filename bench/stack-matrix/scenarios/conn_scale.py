"""SB-12 connection-scale runner: idle-conn RSS, active-stream RSS, handshake rate, storm rate.

Peer-agnostic harness: opens real TCP (+TLS) + HTTP/2 connections with the
stdlib only (raw sockets, ``ssl``, hand-encoded HPACK), so every SB-11
server peer (native, tonic-pbrs, tonic-prost, go, cpp) is measured the
same way. Server RSS is sampled externally per process, never mixed with
client memory.

Accept contract (SB-12):
  (1) fd/sysctl limits are recorded per run; a run that hits an OS limit
      is status ``invalid``, never a loss (``fail``).
  (2) memory per connection/stream is derived from RSS deltas with >= 3
      repeats; the runner refuses memory kinds with repeats < 3.

Statuses: pass | invalid (OS limit / saturation) | unsupported (peer
cannot do this kind, e.g. tonic TLS) | not_run (optional peer unrunnable)
| fail (real failure). Exit code is nonzero only when a cell failed.

Usage:
  python3 conn_scale.py --scenario conn-scale-idle --out-dir DIR
      [--server-peers native,go] [--smoke] [--repeats N] [-v]
  python3 conn_scale.py --self-test
"""

from __future__ import annotations

import argparse
import concurrent.futures
import errno
import json
import os
import resource
import shutil
import socket
import ssl
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

SCEN_DIR = Path(__file__).resolve().parent
STACK_DIR = SCEN_DIR.parent
REPO_ROOT = STACK_DIR.parents[1]
sys.path.insert(0, str(STACK_DIR))
import cells as cells_mod  # noqa: E402
import peers as peers_mod  # noqa: E402
import peertls  # noqa: E402
import pin as pin_mod  # noqa: E402
import run as matrix_run  # noqa: E402

bench_matrix = matrix_run.bench_matrix

SCHEMA = "sb12-conn-scale/1"

# HTTP/2 client connection preface: magic + empty SETTINGS frame.
H2_MAGIC = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"
H2_EMPTY_SETTINGS = bytes([0, 0, 0, 4, 0, 0, 0, 0, 0])
H2_SETTINGS_ACK = bytes([0, 0, 0, 4, 1, 0, 0, 0, 0])
# Held-open bidi method for the active-stream kind (standard interop case,
# implemented by every SB-11 server peer).
BIDI_PATH = "/grpc.testing.TestService/FullDuplexCall"

# Errnos that prove an OS resource ceiling was hit (accept 1). ENOMEM is
# included: under fd/port pressure the kernel reports it for socket setup.
OS_LIMIT_ERRNOS = {
    errno.EMFILE: "EMFILE: process fd limit",
    errno.ENFILE: "ENFILE: system fd limit",
    errno.ENOBUFS: "ENOBUFS: kernel buffer/port space",
    errno.ENOMEM: "ENOMEM: kernel memory for socket setup",
}

# Server process lives this long at most per ladder point (clean shutdown).
SERVER_TIMEOUT_SECS = 1800.0


class OsLimitHit(Exception):
    """An OS fd/port/backlog ceiling was hit: cell is invalid, not failed."""

    def __init__(self, kind: str, detail: str):
        super().__init__(detail)
        self.kind = kind
        self.detail = detail


class PeerFailure(Exception):
    """The peer misbehaved (died, refused with cause): cell failed."""


# ---------------------------------------------------------------------------
# OS limits (accept 1)
# ---------------------------------------------------------------------------

def _read_sysctl_file(path: str) -> Optional[str]:
    try:
        with open(path, "r", encoding="utf-8") as f:
            return f.read().strip()
    except OSError:
        return None


def _sysctl(keys: List[str]) -> Dict[str, Optional[str]]:
    exe = shutil.which("sysctl")
    if exe is None:
        return {k: None for k in keys}
    try:
        out = subprocess.run(
            [exe, "-n"] + keys, capture_output=True, text=True, timeout=10
        )
    except (OSError, subprocess.SubprocessError):
        return {k: None for k in keys}
    lines = out.stdout.splitlines()
    return {k: (lines[i].strip() if i < len(lines) else None) for i, k in enumerate(keys)}


def collect_os_limits() -> Dict[str, Any]:
    """Record fd/sysctl ceilings; every SB-12 report embeds this (accept 1)."""
    soft_no, hard_no = resource.getrlimit(resource.RLIMIT_NOFILE)
    try:
        soft_np, hard_np = resource.getrlimit(resource.RLIMIT_NPROC)
    except (ValueError, OSError, resource.error):
        soft_np, hard_np = None, None
    record: Dict[str, Any] = {
        "platform": sys.platform,
        "rlimit_nofile_soft": soft_no,
        "rlimit_nofile_hard": hard_no,
        "rlimit_nproc_soft": soft_np,
        "rlimit_nproc_hard": hard_np,
    }
    if sys.platform.startswith("linux"):
        record.update(
            {
                "fs_file_max": _read_sysctl_file("/proc/sys/fs/file-max"),
                "fs_nr_open": _read_sysctl_file("/proc/sys/fs/nr_open"),
                "fs_file_nr": _read_sysctl_file("/proc/sys/fs/file-nr"),
                "net_core_somaxconn": _read_sysctl_file("/proc/sys/net/core/somaxconn"),
                "net_ipv4_ip_local_port_range": _read_sysctl_file(
                    "/proc/sys/net/ipv4/ip_local_port_range"
                ),
                "net_ipv4_tcp_max_syn_backlog": _read_sysctl_file(
                    "/proc/sys/net/ipv4/tcp_max_syn_backlog"
                ),
                "net_core_netdev_max_backlog": _read_sysctl_file(
                    "/proc/sys/net/core/netdev_max_backlog"
                ),
                "net_ipv4_tcp_tw_reuse": _read_sysctl_file(
                    "/proc/sys/net/ipv4/tcp_tw_reuse"
                ),
            }
        )
    elif sys.platform == "darwin":
        vals = _sysctl(
            [
                "kern.maxfiles",
                "kern.maxfilesperproc",
                "kern.ipc.somaxconn",
                "kern.ipc.maxsockbuf",
                "net.inet.ip.portrange.first",
                "net.inet.ip.portrange.last",
            ]
        )
        record.update(
            {
                "kern_maxfiles": vals["kern.maxfiles"],
                "kern_maxfilesperproc": vals["kern.maxfilesperproc"],
                "kern_ipc_somaxconn": vals["kern.ipc.somaxconn"],
                "kern_ipc_maxsockbuf": vals["kern.ipc.maxsockbuf"],
                "portrange_first": vals["net.inet.ip.portrange.first"],
                "portrange_last": vals["net.inet.ip.portrange.last"],
            }
        )
    else:
        record["note"] = "sysctl inventory implemented for linux/darwin only"
    return record


def ephemeral_port_budget(limits: Dict[str, Any]) -> Optional[int]:
    """Usable loopback ephemeral-port count, or None when unknown."""
    if sys.platform.startswith("linux"):
        raw = limits.get("net_ipv4_ip_local_port_range") or ""
        try:
            lo, hi = raw.split()
            return max(0, int(hi) - int(lo))
        except ValueError:
            return None
    if sys.platform == "darwin":
        try:
            return max(
                0,
                int(limits["portrange_last"]) - int(limits["portrange_first"]),
            )
        except (TypeError, ValueError, KeyError):
            return None
    return None


def preflight_budget(target_conns: int, limits: Dict[str, Any]) -> Optional[str]:
    """Return the invalid reason when the target provably exceeds OS budget.

    Both opener and server need ~1 fd per connection out of the same soft
    rlimit, plus headroom for stdio/threads; loopback ports bound any
    single-source ladder point. Hitting this check marks the cell invalid
    without spawning anything (accept 1).
    """
    soft_no = limits.get("rlimit_nofile_soft")
    if isinstance(soft_no, int) and soft_no > 0:
        need = 2 * target_conns + 256
        if need > soft_no:
            return (
                f"os-limit preflight: {target_conns} conns need ~{need} fds "
                f"(opener + server) but RLIMIT_NOFILE soft={soft_no}; raise "
                f"with ulimit -n (claim hosts: >= 200k for the 100k point)"
            )
    ports = ephemeral_port_budget(limits)
    if ports is not None and target_conns > ports:
        return (
            f"os-limit preflight: {target_conns} conns exceed the loopback "
            f"ephemeral-port budget ~{ports}; widen ip_local_port_range / "
            f"portrange (claim hosts) or lower the ladder"
        )
    return None


def classify_socket_error(exc: OSError, server_alive: bool) -> Optional[OsLimitHit]:
    """Map an OSError to OsLimitHit when it names an OS ceiling, else None."""
    if exc.errno in OS_LIMIT_ERRNOS:
        return OsLimitHit(
            OS_LIMIT_ERRNOS[exc.errno].split(":")[0],
            f"{OS_LIMIT_ERRNOS[exc.errno]} during connect/preface: {exc}",
        )
    if exc.errno == errno.ECONNREFUSED and server_alive:
        # Live server yet refused: listen/accept backlog is full (storm or
        # fd exhaustion server-side). Not a peer loss.
        return OsLimitHit(
            "ECONNREFUSED-live-server",
            f"server alive but refused connections (backlog/accept-queue full): {exc}",
        )
    return None


# ---------------------------------------------------------------------------
# RSS sampling (accept 2: deltas, never absolute RSS)
# ---------------------------------------------------------------------------

def sample_rss_bytes(pid: int) -> Optional[int]:
    """Current RSS of `pid` in bytes; None when unreadable."""
    if sys.platform.startswith("linux"):
        try:
            with open(f"/proc/{pid}/status", "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        return int(line.split()[1]) * 1024
        except (OSError, ValueError, IndexError):
            return None
        return None
    ps = shutil.which("ps")
    if ps is None:
        return None
    try:
        out = subprocess.run(
            [ps, "-o", "rss=", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    try:
        return int(out.stdout.strip().split()[0]) * 1024
    except (ValueError, IndexError):
        return None


def sample_rss_median(pid: int, samples: int, interval_s: float) -> Optional[int]:
    vals = []
    for _ in range(samples):
        v = sample_rss_bytes(pid)
        if v is not None:
            vals.append(v)
        time.sleep(interval_s)
    if not vals:
        return None
    vals.sort()
    return vals[len(vals) // 2]


def median(values: List[float]) -> Optional[float]:
    if not values:
        return None
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return float(ordered[mid])
    return (ordered[mid - 1] + ordered[mid]) / 2.0


# ---------------------------------------------------------------------------
# HTTP/2 + HPACK primitives (peer-agnostic, stdlib only)
# ---------------------------------------------------------------------------

def hpack_literal(name: bytes, value: bytes) -> bytes:
    """One literal-without-indexing field (new name); names/values < 127 B."""
    assert len(name) < 127 and len(value) < 127, "sb12 headers are short"
    out = bytearray([0x00, len(name)])
    out += name
    out += bytes([len(value)])
    out += value
    return bytes(out)


def h2_frame(frame_type: int, flags: int, stream_id: int, payload: bytes) -> bytes:
    header = len(payload).to_bytes(3, "big") + bytes(
        [frame_type, flags]
    ) + (stream_id & 0x7FFFFFFF).to_bytes(4, "big")
    return header + payload


def grpc_bidi_headers_block(authority: str) -> bytes:
    """HPACK block for a held-open FullDuplexCall (no END_STREAM)."""
    fields = [
        (b":method", b"POST"),
        (b":scheme", b"http"),
        (b":path", BIDI_PATH.encode()),
        (b":authority", authority.encode()),
        (b"content-type", b"application/grpc"),
        (b"te", b"trailers"),
        (b"user-agent", b"sb12-conn-scale/1"),
    ]
    return b"".join(hpack_literal(k, v) for k, v in fields)


def read_server_settings(sock: socket.socket, timeout_s: float) -> bool:
    """Consume frames until the server's SETTINGS (type 4, stream 0, no ACK).

    Returns True on establishment; False on timeout/close. Sends the ACK.
    """
    sock.settimeout(timeout_s)
    buf = b""
    deadline = time.monotonic() + timeout_s
    try:
        while time.monotonic() < deadline:
            try:
                chunk = sock.recv(65536)
            except socket.timeout:
                return False
            if not chunk:
                return False
            buf += chunk
            while len(buf) >= 9:
                length = int.from_bytes(buf[0:3], "big")
                ftype, flags = buf[3], buf[4]
                stream = int.from_bytes(buf[5:9], "big") & 0x7FFFFFFF
                if len(buf) < 9 + length:
                    break
                buf = buf[9 + length:]
                if ftype == 4 and stream == 0 and not (flags & 0x1):
                    try:
                        sock.sendall(H2_SETTINGS_ACK)
                    except OSError:
                        return False
                    return True
    except OSError:
        return False
    return False


def count_rst_frames(sock: socket.socket, want_ids: set) -> int:
    """Non-blocking drain counting RST_STREAM frames for our stream ids."""
    try:
        sock.setblocking(False)
        try:
            data = sock.recv(1 << 20)
        except (BlockingIOError, OSError):
            return 0
    finally:
        try:
            sock.setblocking(True)
        except OSError:
            pass
    if not data:
        return 0
    n = 0
    off = 0
    while off + 9 <= len(data):
        length = int.from_bytes(data[off:off + 3], "big")
        ftype = data[off + 3]
        stream = int.from_bytes(data[off + 5:off + 9], "big") & 0x7FFFFFFF
        if ftype == 3 and stream in want_ids:
            n += 1
        off += 9 + length
    return n


def open_idle_connection(
    host: str,
    port: int,
    tls_context: Optional[ssl.SSLContext],
    server_hostname: Optional[str],
    timeout_s: float,
) -> socket.socket:
    """TCP (+TLS) connect + HTTP/2 preface; returns the held-open socket."""
    raw = socket.create_connection((host, port), timeout=timeout_s)
    try:
        sock: socket.socket = raw
        if tls_context is not None:
            sock = tls_context.wrap_socket(raw, server_hostname=server_hostname)
            sock.settimeout(timeout_s)
            sock.do_handshake()
        sock.sendall(H2_MAGIC + H2_EMPTY_SETTINGS)
        if not read_server_settings(sock, timeout_s):
            raise ConnectionError("no server SETTINGS within timeout")
        return sock
    except BaseException:
        try:
            raw.close()
        except OSError:
            pass
        raise


@dataclass
class OpenOutcome:
    sockets: List[Any] = field(default_factory=list)
    established: int = 0
    timeouts: int = 0
    os_limit: Optional[OsLimitHit] = None
    other_errors: int = 0
    first_error: str = ""


def open_many(
    host: str,
    port: int,
    count: int,
    tls_context: Optional[ssl.SSLContext],
    server_hostname: Optional[str],
    timeout_s: float,
    workers: int,
    server_alive: Callable[[], bool],
) -> OpenOutcome:
    """Open `count` idle connections with a thread pool; classify failures."""
    outcome = OpenOutcome()
    lock = threading.Lock()

    def one(_: int) -> None:
        if outcome.os_limit is not None:
            return
        try:
            sock = open_idle_connection(
                host, port, tls_context, server_hostname, timeout_s
            )
        except OsLimitHit:
            raise
        except OSError as e:
            hit = classify_socket_error(e, server_alive())
            with lock:
                if hit is not None and outcome.os_limit is None:
                    outcome.os_limit = hit
                elif hit is None:
                    outcome.other_errors += 1
                    if not outcome.first_error:
                        outcome.first_error = f"{type(e).__name__}: {e}"
            return
        except Exception as e:  # noqa: BLE001 - timeouts etc. count, never raise
            with lock:
                outcome.timeouts += 1
                if not outcome.first_error:
                    outcome.first_error = f"{type(e).__name__}: {e}"
            return
        with lock:
            outcome.sockets.append(sock)
            outcome.established += 1

    # An OSError inside a worker is classified above; OsLimitHit raised
    # there would escape the pool, so workers convert it via classify too.
    def guarded(i: int) -> None:
        try:
            one(i)
        except OsLimitHit as hit:
            with lock:
                if outcome.os_limit is None:
                    outcome.os_limit = hit

    with concurrent.futures.ThreadPoolExecutor(
        max_workers=max(1, workers), thread_name_prefix="sb12-open"
    ) as pool:
        list(pool.map(guarded, range(count)))
    return outcome


def close_all(sockets: List[Any]) -> None:
    for sock in sockets:
        try:
            # Deliberately no RST storm: plain close lets the server reclaim
            # connection + stream state via FIN handling.
            sock.close()
        except OSError:
            pass


# ---------------------------------------------------------------------------
# Server lifecycle (reuses SB-11 argv/pinning/readiness; read-only reuse)
# ---------------------------------------------------------------------------

@dataclass
class LiveServer:
    proc: subprocess.Popen
    addr: str
    host: str
    port: int
    binary: str
    codec: str
    pin_state: Dict[str, Any]
    stdout_path: Path
    stderr_path: Path

    def alive(self) -> bool:
        return self.proc.poll() is None


def spawn_server(
    registry: Any,
    peer: str,
    tls_spec: Optional[peertls.ServerTlsSpec],
    cpus: int,
    log_dir: Path,
    tag: str,
    verbose: bool = False,
) -> LiveServer:
    host = "127.0.0.1"
    port = bench_matrix.find_free_port()
    cmd, binary, codec = matrix_run.server_command(
        registry, peer, host, port, SERVER_TIMEOUT_SECS, tls_spec
    )
    pinned, pin_state = pin_mod.wrap(cmd, cpus, 0)
    stdout_path = log_dir / f"{tag}.stdout"
    stderr_path = log_dir / f"{tag}.stderr"
    if verbose:
        print(f"[SB-12] spawn {tag}: {' '.join(pinned)}")
    proc = subprocess.Popen(
        pinned,
        stdout=open(stdout_path, "w", encoding="utf-8"),
        stderr=open(stderr_path, "w", encoding="utf-8"),
        text=True,
    )
    bench_matrix._ACTIVE_PROCESSES.add(proc)
    pin_state = dict(pin_state)
    pin_state["actual"] = pin_mod.actual_affinity(proc.pid)
    try:
        _, addr = bench_matrix.wait_for_server_readiness(
            proc, host=host, port=port, timeout_secs=30.0
        )
    except BaseException:
        terminate(proc)
        raise
    return LiveServer(
        proc=proc,
        addr=addr,
        host=host,
        port=port,
        binary=binary,
        codec=codec,
        pin_state=pin_state,
        stdout_path=stdout_path,
        stderr_path=stderr_path,
    )


def terminate(proc: subprocess.Popen) -> None:
    bench_matrix._ACTIVE_PROCESSES.discard(proc)
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=10.0)


def server_stderr_tail(server: LiveServer, limit: int = 4000) -> str:
    try:
        text = server.stderr_path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    return text[-limit:]


def server_hit_fd_limit(server: LiveServer) -> Optional[str]:
    tail = server_stderr_tail(server).lower()
    for needle in ("too many open files", "emfile", "enfile", "no file descriptors"):
        if needle in tail:
            return f"server stderr reports fd exhaustion ({needle})"
    return None


# ---------------------------------------------------------------------------
# Measurement loops (one fresh server per ladder point; repeats share it)
# ---------------------------------------------------------------------------

def rss_or_fail(server: LiveServer, samples: int, interval_s: float) -> int:
    rss = sample_rss_median(server.proc.pid, samples, interval_s)
    if rss is None:
        raise PeerFailure(
            f"RSS unreadable for {server.binary} pid={server.proc.pid} "
            f"(alive={server.alive()}); cannot derive a delta"
        )
    return rss


def check_server(
    server: LiveServer, cell: Dict[str, Any], os_limits: Dict[str, Any]
) -> Optional[Dict[str, Any]]:
    """Return an invalid/fail cell patch when the server is unusable."""
    if server.alive():
        return None
    fd_note = server_hit_fd_limit(server)
    if fd_note is not None:
        return {
            "status": "invalid",
            "reason": f"os-limit: {fd_note}",
            "os_limit_hit": {"kind": "server-fd-exhaustion", "detail": fd_note},
        }
    return {
        "status": "fail",
        "reason": (
            f"server exited rc={server.proc.returncode}; "
            f"stderr: {server_stderr_tail(server, 500)}"
        ),
    }


def warmup_cycle(
    server: LiveServer,
    conns: int,
    streams_per_conn: int,
    settle_s: float,
    open_timeout_s: float,
    workers: int,
) -> Dict[str, Any]:
    """One discarded open/settle/close round to absorb first-touch growth.

    Allocator page retention and one-time runtime init otherwise land in
    repeat 1 (observed: ~12 MB on the first 256 conns, ~0 after). The
    warmup replays the full point shape (connections plus held streams),
    so all measured repeats are steady-state.
    """
    if conns <= 0:
        return {"conns": 0, "established": 0, "streams": 0}
    outcome = open_many(
        server.host, server.port, conns, None, None,
        open_timeout_s, workers, server.alive,
    )
    streams = 0
    if streams_per_conn > 0 and outcome.sockets:
        block = grpc_bidi_headers_block(server.addr)
        for sock in outcome.sockets:
            try:
                sock.settimeout(open_timeout_s)
                for k in range(streams_per_conn):
                    sock.sendall(h2_frame(1, 0x4, 2 * k + 1, block))
                    streams += 1
            except OSError:
                pass
    time.sleep(settle_s)
    close_all(outcome.sockets)
    time.sleep(settle_s)
    return {
        "conns": conns,
        "established": outcome.established,
        "streams": streams,
    }


def run_idle_point(
    server: LiveServer,
    target_conns: int,
    repeats: int,
    settle_s: float,
    samples: int,
    interval_s: float,
    open_timeout_s: float,
    workers: int,
    os_limits: Dict[str, Any],
    warmup_conns: Optional[int] = None,
    warmup_settle_s: float = 1.0,
    verbose: bool = False,
) -> Dict[str, Any]:
    """Idle connections: per-repeat baseline->loaded RSS deltas (accept 2)."""
    rep_rows: List[Dict[str, Any]] = []
    baselines: List[int] = []
    warmup = warmup_cycle(
        server,
        target_conns if warmup_conns is None else min(target_conns, warmup_conns),
        0,
        warmup_settle_s,
        open_timeout_s,
        workers,
    )
    for rep in range(repeats):
        time.sleep(settle_s)
        if not server.alive():
            return {"patch": check_server(server, {}, os_limits), "repeats": rep_rows}
        baseline = rss_or_fail(server, samples, interval_s)
        baselines.append(baseline)
        outcome = open_many(
            server.host, server.port, target_conns, None, None,
            open_timeout_s, workers, server.alive,
        )
        try:
            if outcome.os_limit is not None:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": f"os-limit: {outcome.os_limit.detail}",
                        "os_limit_hit": {
                            "kind": outcome.os_limit.kind,
                            "detail": outcome.os_limit.detail,
                            "established": outcome.established,
                            "target": target_conns,
                        },
                    },
                    "repeats": rep_rows,
                }
            fd_note = server_hit_fd_limit(server)
            if fd_note is not None:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": f"os-limit: {fd_note}",
                        "os_limit_hit": {"kind": "server-fd-exhaustion", "detail": fd_note},
                    },
                    "repeats": rep_rows,
                }
            if not server.alive():
                return {
                    "patch": check_server(server, {}, os_limits),
                    "repeats": rep_rows,
                }
            frac = outcome.established / target_conns if target_conns else 0.0
            if frac < 0.99:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": (
                            f"established {outcome.established}/{target_conns} "
                            f"({frac:.1%}; timeouts={outcome.timeouts} "
                            f"errors={outcome.other_errors} first={outcome.first_error[:160]}); "
                            "server-side saturation, not a peer loss"
                        ),
                        "os_limit_hit": None,
                        "established_fraction": round(frac, 4),
                    },
                    "repeats": rep_rows,
                }
            time.sleep(settle_s)
            loaded = rss_or_fail(server, samples, interval_s)
            delta = max(0, loaded - baseline)
            rep_rows.append(
                {
                    "rep": rep + 1,
                    "target_conns": target_conns,
                    "established": outcome.established,
                    "rss_baseline_bytes": baseline,
                    "rss_loaded_bytes": loaded,
                    "rss_delta_bytes": delta,
                    "bytes_per_connection": delta / outcome.established,
                }
            )
            if verbose:
                print(
                    f"[SB-12] idle rep {rep + 1}: delta={delta}B "
                    f"per-conn={delta / outcome.established:.0f}B"
                )
        finally:
            close_all(outcome.sockets)
            outcome.sockets.clear()
            time.sleep(min(settle_s, 2.0))
    per_conn = [r["bytes_per_connection"] for r in rep_rows]
    drift = baselines[-1] - baselines[0] if len(baselines) > 1 else 0
    return {
        "patch": {"status": "pass", "reason": ""},
        "repeats": rep_rows,
        "warmup": warmup,
        "summary": {
            "median_bytes_per_connection": median(per_conn),
            "min_bytes_per_connection": min(per_conn) if per_conn else None,
            "max_bytes_per_connection": max(per_conn) if per_conn else None,
            "repeats": len(rep_rows),
            "baseline_drift_bytes": drift,
        },
    }


def run_stream_point(
    server: LiveServer,
    conns: int,
    streams_per_conn: int,
    repeats: int,
    settle_s: float,
    samples: int,
    interval_s: float,
    open_timeout_s: float,
    workers: int,
    os_limits: Dict[str, Any],
    warmup_conns: Optional[int] = None,
    warmup_settle_s: float = 1.0,
    verbose: bool = False,
) -> Dict[str, Any]:
    """Active streams: idle->streams RSS delta over held-open bidi HEADERS."""
    total_streams = conns * streams_per_conn
    rep_rows: List[Dict[str, Any]] = []
    warmup = warmup_cycle(
        server,
        conns if warmup_conns is None else min(conns, warmup_conns),
        streams_per_conn,
        warmup_settle_s,
        open_timeout_s,
        workers,
    )
    for rep in range(repeats):
        time.sleep(settle_s)
        if not server.alive():
            return {"patch": check_server(server, {}, os_limits), "repeats": rep_rows}
        baseline = rss_or_fail(server, samples, interval_s)
        outcome = open_many(
            server.host, server.port, conns, None, None,
            open_timeout_s, workers, server.alive,
        )
        try:
            if outcome.os_limit is not None:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": f"os-limit: {outcome.os_limit.detail}",
                        "os_limit_hit": {
                            "kind": outcome.os_limit.kind,
                            "detail": outcome.os_limit.detail,
                        },
                    },
                    "repeats": rep_rows,
                }
            if not server.alive():
                return {
                    "patch": check_server(server, {}, os_limits),
                    "repeats": rep_rows,
                }
            if outcome.established < conns:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": (
                            f"stream base: established {outcome.established}/{conns} "
                            "connections; server-side saturation, not a peer loss"
                        ),
                        "os_limit_hit": None,
                    },
                    "repeats": rep_rows,
                }
            time.sleep(settle_s)
            rss_idle = rss_or_fail(server, samples, interval_s)
            # Held-open bidi streams: HEADERS without END_STREAM, no DATA.
            block = grpc_bidi_headers_block(server.addr)
            opened = 0
            send_errors = 0
            id_sets: List[set] = []
            for sock in outcome.sockets:
                ids = set()
                try:
                    sock.settimeout(open_timeout_s)
                    for k in range(streams_per_conn):
                        sid = 2 * k + 1
                        sock.sendall(h2_frame(1, 0x4, sid, block))
                        ids.add(sid)
                        opened += 1
                except OSError as e:
                    send_errors += 1
                    hit = classify_socket_error(e, server.alive())
                    if hit is not None:
                        return {
                            "patch": {
                                "status": "invalid",
                                "reason": f"os-limit: {hit.detail}",
                                "os_limit_hit": {"kind": hit.kind, "detail": hit.detail},
                            },
                            "repeats": rep_rows,
                        }
                id_sets.append(ids)
            time.sleep(settle_s)
            rst = sum(
                count_rst_frames(sock, ids)
                for sock, ids in zip(outcome.sockets, id_sets)
            )
            if rst > 0.01 * opened:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": (
                            f"peer reset {rst}/{opened} held streams (>1%); its "
                            "per-connection stream cap is below the scenario demand "
                            f"({streams_per_conn}/conn); tune the ladder, not a loss"
                        ),
                        "os_limit_hit": None,
                        "rst_count": rst,
                    },
                    "repeats": rep_rows,
                }
            rss_streams = rss_or_fail(server, samples, interval_s)
            held = opened - rst
            stream_delta = max(0, rss_streams - rss_idle)
            rep_rows.append(
                {
                    "rep": rep + 1,
                    "conns": conns,
                    "streams_per_conn": streams_per_conn,
                    "streams_opened": opened,
                    "streams_reset": rst,
                    "streams_held": held,
                    "rss_baseline_bytes": baseline,
                    "rss_idle_bytes": rss_idle,
                    "rss_streams_bytes": rss_streams,
                    "stream_delta_bytes": stream_delta,
                    "bytes_per_stream": stream_delta / held if held else None,
                    "bytes_per_connection_idle_xcheck": (
                        max(0, rss_idle - baseline) / outcome.established
                    ),
                }
            )
            if verbose:
                print(
                    f"[SB-12] stream rep {rep + 1}: held={held} "
                    f"per-stream={stream_delta / held if held else 0:.0f}B"
                )
        finally:
            close_all(outcome.sockets)
            outcome.sockets.clear()
            time.sleep(min(settle_s, 2.0))
    per_stream = [r["bytes_per_stream"] for r in rep_rows if r["bytes_per_stream"]]
    return {
        "patch": {"status": "pass", "reason": ""},
        "repeats": rep_rows,
        "warmup": warmup,
        "summary": {
            "total_streams": total_streams,
            "median_bytes_per_stream": median(per_stream),
            "min_bytes_per_stream": min(per_stream) if per_stream else None,
            "max_bytes_per_stream": max(per_stream) if per_stream else None,
            "repeats": len(rep_rows),
        },
    }


def run_handshake_point(
    server: LiveServer,
    tls_spec: peertls.ServerTlsSpec,
    handshakes_per_rep: int,
    repeats: int,
    workers: int,
    cpus: int,
    verbose: bool = False,
) -> Dict[str, Any]:
    """TLS handshakes/sec (total + per core) with a verifying client."""
    # Minimal client context on purpose: create_default_context() enables
    # VERIFY_X509_STRICT, which rejects the in-tree test CA (CA:TRUE but
    # no key-usage extension; rustls accepts it, as do the SB-11 cells).
    # CERT_REQUIRED + check_hostname against the peer CA is still full
    # chain + name verification -- there is no skip-verify path.
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ctx.load_verify_locations(cafile=tls_spec.ca_file)
    ctx.verify_mode = ssl.CERT_REQUIRED
    ctx.check_hostname = True
    rep_rows: List[Dict[str, Any]] = []
    for rep in range(repeats):
        if not server.alive():
            return {"patch": check_server(server, {}, {}), "repeats": rep_rows}
        lat: List[float] = []
        lock = threading.Lock()
        stop = threading.Event()
        outcome_err: List[Any] = []
        attempted = 0

        def worker(n: int) -> None:
            nonlocal attempted
            for _ in range(n):
                if stop.is_set():
                    return
                start = time.monotonic()
                try:
                    raw = socket.create_connection(
                        (server.host, server.port), timeout=10.0
                    )
                except OSError as e:
                    with lock:
                        attempted += 1
                        outcome_err.append(e)
                    return
                try:
                    with ctx.wrap_socket(
                        raw, server_hostname=tls_spec.server_name
                    ) as tls:
                        tls.settimeout(10.0)
                        tls.do_handshake()
                except OSError as e:
                    with lock:
                        attempted += 1
                        outcome_err.append(e)
                    try:
                        raw.close()
                    except OSError:
                        pass
                    return
                with lock:
                    attempted += 1
                    lat.append(time.monotonic() - start)

        shares = [handshakes_per_rep // workers] * workers
        for i in range(handshakes_per_rep % workers):
            shares[i] += 1
        wall_start = time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(
            max_workers=workers, thread_name_prefix="sb12-hs"
        ) as pool:
            list(pool.map(worker, shares))
        wall_s = max(1e-6, time.monotonic() - wall_start)
        if outcome_err:
            first = outcome_err[0]
            if isinstance(first, OSError):
                hit = classify_socket_error(first, server.alive())
                if hit is not None:
                    return {
                        "patch": {
                            "status": "invalid",
                            "reason": f"os-limit: {hit.detail}",
                            "os_limit_hit": {"kind": hit.kind, "detail": hit.detail},
                        },
                        "repeats": rep_rows,
                    }
            return {
                "patch": {
                    "status": "fail",
                    "reason": (
                        f"handshake errors: {len(outcome_err)}/{attempted} "
                        f"attempted failed first={first!r:.200}"
                    ),
                },
                "repeats": rep_rows,
            }
        rate = len(lat) / wall_s
        ordered = sorted(lat)
        rep_rows.append(
            {
                "rep": rep + 1,
                "handshakes": len(lat),
                "wall_s": round(wall_s, 3),
                "handshakes_per_s": rate,
                "handshakes_per_s_per_core": rate / cpus,
                "latency_p50_s": ordered[len(ordered) // 2],
                "latency_p99_s": ordered[min(len(ordered) - 1, int(0.99 * len(ordered)))],
            }
        )
        if verbose:
            print(f"[SB-12] handshake rep {rep + 1}: {rate:.0f}/s ({rate / cpus:.0f}/core)")
    rates = [r["handshakes_per_s"] for r in rep_rows]
    return {
        "patch": {"status": "pass", "reason": ""},
        "repeats": rep_rows,
        "summary": {
            "median_handshakes_per_s": median(rates),
            "median_handshakes_per_s_per_core": (
                median(rates) / cpus if median(rates) else None
            ),
            "repeats": len(rep_rows),
            "pinned_cpus": cpus,
        },
    }


def run_storm_point(
    server: LiveServer,
    duration_s: float,
    repeats: int,
    workers: int,
    verbose: bool = False,
) -> Dict[str, Any]:
    """Connection-storm accept rate: established preface-roundtrips/sec."""
    rep_rows: List[Dict[str, Any]] = []
    for rep in range(repeats):
        if not server.alive():
            return {"patch": check_server(server, {}, {}), "repeats": rep_rows}
        deadline = time.monotonic() + duration_s
        established = 0
        refused = 0
        timeouts = 0
        os_hit: List[OsLimitHit] = []
        lock = threading.Lock()

        def worker() -> None:
            nonlocal established, refused, timeouts
            # Bounded rotating hold: holding every storm connection would
            # burn rate x duration fds (millions at claim scale). Each
            # counted connection completed the full preface roundtrip, so
            # the server did the accept+processing work; the standing pool
            # keeps the storm realistic without exhausting the opener.
            from collections import deque

            held: Any = deque(maxlen=64)
            try:
                while time.monotonic() < deadline and not os_hit:
                    try:
                        sock = open_idle_connection(
                            server.host, server.port, None, None, 2.0
                        )
                    except OSError as e:
                        hit = classify_socket_error(e, server.alive())
                        with lock:
                            if hit is not None and not os_hit:
                                os_hit.append(hit)
                            elif e.errno == errno.ECONNREFUSED:
                                refused += 1
                            else:
                                timeouts += 1
                        if hit is not None:
                            return
                        continue
                    except Exception:  # noqa: BLE001 - preface timeouts count
                        with lock:
                            timeouts += 1
                        continue
                    if len(held) == held.maxlen:
                        oldest = held.popleft()
                        try:
                            oldest.close()
                        except OSError:
                            pass
                    held.append(sock)
                    with lock:
                        established += 1
            finally:
                close_all(list(held))

        wall_start = time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(
            max_workers=workers, thread_name_prefix="sb12-storm"
        ) as pool:
            list(pool.map(lambda _: worker(), range(workers)))
        wall_s = max(1e-6, time.monotonic() - wall_start)
        if os_hit:
            return {
                "patch": {
                    "status": "invalid",
                    "reason": f"os-limit: {os_hit[0].detail}",
                    "os_limit_hit": {"kind": os_hit[0].kind, "detail": os_hit[0].detail},
                },
                "repeats": rep_rows,
            }
        if not server.alive():
            fd_note = server_hit_fd_limit(server)
            if fd_note is not None:
                return {
                    "patch": {
                        "status": "invalid",
                        "reason": f"os-limit: {fd_note}",
                        "os_limit_hit": {"kind": "server-fd-exhaustion", "detail": fd_note},
                    },
                    "repeats": rep_rows,
                }
            return {"patch": check_server(server, {}, {}), "repeats": rep_rows}
        rate = established / wall_s
        rep_rows.append(
            {
                "rep": rep + 1,
                "established": established,
                "refused": refused,
                "timeouts": timeouts,
                "wall_s": round(wall_s, 3),
                "accepts_per_s": rate,
            }
        )
        if verbose:
            print(f"[SB-12] storm rep {rep + 1}: {rate:.0f}/s established")
        time.sleep(1.0)
    rates = [r["accepts_per_s"] for r in rep_rows]
    return {
        "patch": {"status": "pass", "reason": ""},
        "repeats": rep_rows,
        "summary": {
            "median_accepts_per_s": median(rates),
            "min_accepts_per_s": min(rates) if rates else None,
            "max_accepts_per_s": max(rates) if rates else None,
            "repeats": len(rep_rows),
        },
    }


# ---------------------------------------------------------------------------
# Scenario loading + orchestration
# ---------------------------------------------------------------------------

MEMORY_KINDS = ("idle-conn", "active-stream")

TLS_UNSUPPORTED_PEERS = set(cells_mod.NO_TLS_TRANSPORTS)


def load_scenario(name: str) -> Dict[str, Any]:
    path = SCEN_DIR / f"{name}.json"
    if not path.is_file():
        raise FileNotFoundError(
            f"unknown SB-12 scenario '{name}'; want one of: "
            + ", ".join(sorted(p.stem for p in SCEN_DIR.glob("conn-scale-*.json")))
        )
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def effective_params(scenario: Dict[str, Any], smoke: bool) -> Dict[str, Any]:
    params = dict(scenario.get("params", {}))
    if smoke:
        params.update(scenario.get("smoke_params", {}))
    return params


def run_scenario(
    scenario_name: str,
    out_dir: Path,
    server_peers: Optional[List[str]] = None,
    smoke: bool = False,
    repeats_override: Optional[int] = None,
    include_optional: bool = False,
    verbose: bool = False,
) -> Tuple[int, Dict[str, Any]]:
    scenario = load_scenario(scenario_name)
    kind = scenario["kind"]
    params = effective_params(scenario, smoke)
    repeats = repeats_override or scenario.get("repeats", 3)
    if kind in MEMORY_KINDS and repeats < 3:
        raise ValueError(
            f"SB-12 accept (2): {kind} needs >= 3 repeats, got {repeats}"
        )
    peers = list(server_peers) if server_peers else list(scenario["peers"])
    if include_optional:
        peers += [p for p in cells_mod.OPTIONAL_SERVER_PEERS if p not in peers]
    tls_mode = bool(scenario.get("tls", False))
    cpus = int(scenario.get("cpus", 1))

    out_dir.mkdir(parents=True, exist_ok=True)
    log_dir = out_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    registry = bench_matrix.PeerRegistry(REPO_ROOT)
    os_limits = collect_os_limits()

    report: Dict[str, Any] = {
        "schema": SCHEMA,
        "scenario": scenario_name,
        "kind": kind,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "smoke": smoke,
        "params": params,
        "repeats": repeats,
        "cpus": cpus,
        "tls": tls_mode,
        "os_limits": os_limits,
        "cpu_constraints": bench_matrix.collect_cpu_constraints(),
        "pinning": pin_mod.describe(cpus),
    }

    problem, resolved, optional_status = matrix_run.preflight(registry, peers, [])
    report["peers"] = resolved
    report["optional_peers"] = optional_status
    if problem:
        report["matrix_complete"] = False
        report["fatal"] = problem
        write_report(out_dir, report)
        print(f"[SB-12] {problem}")
        return 1, report

    tls_cache: Dict[str, Optional[peertls.ServerTlsSpec]] = {}
    cells_out: List[Dict[str, Any]] = []
    failures = 0
    for peer in peers:
        for point in ladder_points(kind, params):
            cell_id = cell_name(kind, peer, point, tls_mode, cpus)
            base: Dict[str, Any] = {
                "id": cell_id,
                "scenario": scenario_name,
                "kind": kind,
                "peer": peer,
                "point": point,
                "cpus": cpus,
                "tls": tls_mode,
            }
            not_run = optional_not_run(peer, optional_status)
            if not_run:
                cells_out.append({**base, "status": "not_run", "reason": not_run})
                print(f"[SB-12] {cell_id}: not_run ({not_run[:120]})")
                continue
            if tls_mode and peer in TLS_UNSUPPORTED_PEERS:
                reason = (
                    f"tonic transport has no TLS support in rpc-bench "
                    f"(peer {peer}); TLS cells need native/go/cpp endpoints"
                )
                cells_out.append({**base, "status": "unsupported", "reason": reason})
                continue
            tls_spec = None
            if tls_mode:
                if peer not in tls_cache:
                    if peers_mod.is_optional(peer):
                        tls_cache[peer] = peers_mod.tls_spec(peer, REPO_ROOT)
                    else:
                        tls_cache[peer] = peertls.server_spec(peer, REPO_ROOT)
                tls_spec = tls_cache[peer]
                if tls_spec is None:
                    cells_out.append(
                        {
                            **base,
                            "status": "unsupported",
                            "reason": f"TLS material unresolvable for {peer}",
                        }
                    )
                    continue
            budget_note = preflight_point_budget(kind, point, params, os_limits)
            if budget_note is not None:
                cells_out.append(
                    {
                        **base,
                        "status": "invalid",
                        "reason": f"os-limit: {budget_note}",
                        "os_limit_hit": {"kind": "preflight", "detail": budget_note},
                        "os_limits": os_limits,
                    }
                )
                print(f"[SB-12] {cell_id}: invalid (preflight: {budget_note[:120]})")
                continue
            try:
                cell_report = run_point(
                    registry, kind, peer, point, params, repeats, cpus,
                    tls_spec, log_dir, cell_id, os_limits, verbose,
                )
            except peers_mod.PeerNotRunnable as e:
                cell_report = {"status": "not_run", "reason": str(e)}
            except (PeerFailure, ValueError) as e:
                cell_report = {"status": "fail", "reason": f"{type(e).__name__}: {e}"[-500:]}
            except Exception as e:  # noqa: BLE001 - a dead cell must not kill the matrix
                cell_report = {"status": "fail", "reason": f"{type(e).__name__}: {e}"[-500:]}
            full = {**base, **cell_report}
            full["os_limits"] = os_limits
            if tls_spec is not None:
                full["tls_spec"] = peertls.spec_dict(tls_spec)
            if full["status"] == "fail":
                failures += 1
            cells_out.append(full)
            one_line = f"[SB-12] {cell_id}: {full['status']}"
            if full.get("summary"):
                one_line += f" {json.dumps(full['summary'])[:220]}"
            if full.get("reason"):
                one_line += f" ({full['reason'][:140]})"
            print(one_line)

    report["cells"] = cells_out
    report["matrix_complete"] = failures == 0
    if failures:
        report["fatal"] = f"{failures} cell(s) failed"
    write_report(out_dir, report)
    # invalid/unsupported/not_run are not losses: only fail exits nonzero.
    return (0 if failures == 0 else 1), report


def optional_not_run(peer: str, optional_status: Dict[str, Any]) -> Optional[str]:
    if not peers_mod.is_optional(peer):
        return None
    entry = optional_status.get(peer)
    if entry is None:
        return f"optional peer {peer} was not preflighted"
    if entry.get("status") != "ready":
        return str(entry.get("reason") or f"optional peer {peer} is not runnable")
    return None


def ladder_points(kind: str, params: Dict[str, Any]) -> List[Dict[str, Any]]:
    if kind == "idle-conn":
        return [{"conns": n} for n in params["ladder_conns"]]
    if kind == "active-stream":
        return [
            {"conns": c, "streams_per_conn": s}
            for c, s in params["ladder_conns_streams"]
        ]
    if kind == "tls-handshake":
        return [{"handshakes_per_rep": params["handshakes_per_rep"]}]
    if kind == "storm":
        return [{"duration_s": params["duration_s"]}]
    raise ValueError(f"unknown SB-12 kind '{kind}'")


def cell_name(
    kind: str, peer: str, point: Dict[str, Any], tls: bool, cpus: int
) -> str:
    tls_tag = "tls" if tls else "plain"
    if kind == "idle-conn":
        n = point["conns"]
        scale = f"{n // 1000}k" if n % 1000 == 0 else str(n)
        return f"sb12-idle-{peer}-{scale}conn-{tls_tag}-{cpus}cpu"
    if kind == "active-stream":
        total = point["conns"] * point["streams_per_conn"]
        scale = f"{total // 1000}k" if total % 1000 == 0 else str(total)
        return f"sb12-stream-{peer}-{scale}stream-{tls_tag}-{cpus}cpu"
    if kind == "tls-handshake":
        return f"sb12-tlshs-{peer}-percore-{tls_tag}-{cpus}cpu"
    if kind == "storm":
        return f"sb12-storm-{peer}-burst-{tls_tag}-{cpus}cpu"
    raise ValueError(f"unknown SB-12 kind '{kind}'")


def preflight_point_budget(
    kind: str, point: Dict[str, Any], params: Dict[str, Any], os_limits: Dict[str, Any]
) -> Optional[str]:
    if kind == "idle-conn":
        return preflight_budget(point["conns"], os_limits)
    if kind == "active-stream":
        # Streams ride the base connections; budget the connection count.
        return preflight_budget(point["conns"], os_limits)
    if kind == "tls-handshake":
        # Sequential-ish churn: bound live sockets by workers, ports by
        # TIME_WAIT (worst case one port per handshake in the window).
        workers = params.get("workers", 16)
        note = preflight_budget(workers * 2, os_limits)
        if note is not None:
            return note
        ports = ephemeral_port_budget(os_limits)
        total = point["handshakes_per_rep"]
        if ports is not None and total > 4 * ports:
            return (
                f"os-limit preflight: {total} handshakes/rep churn vs ~{ports} "
                "ephemeral ports (connect-side TIME_WAIT); lower "
                "handshakes_per_rep or widen the port range"
            )
        return None
    if kind == "storm":
        workers = params.get("workers", 32)
        return preflight_budget(workers * 4, os_limits)
    return None


def run_point(
    registry: Any,
    kind: str,
    peer: str,
    point: Dict[str, Any],
    params: Dict[str, Any],
    repeats: int,
    cpus: int,
    tls_spec: Optional[peertls.ServerTlsSpec],
    log_dir: Path,
    cell_id: str,
    os_limits: Dict[str, Any],
    verbose: bool,
) -> Dict[str, Any]:
    server = spawn_server(registry, peer, tls_spec, cpus, log_dir, cell_id, verbose)
    cell: Dict[str, Any] = {
        "server_binary": server.binary,
        "server_codec": server.codec,
        "server_pin": server.pin_state,
        "addr": server.addr,
        "logs": {
            "stdout": server.stdout_path.name,
            "stderr": server.stderr_path.name,
        },
    }
    try:
        if kind == "idle-conn":
            out = run_idle_point(
                server, point["conns"], repeats,
                params.get("settle_s", 3.0), params.get("rss_samples", 5),
                params.get("rss_interval_s", 0.2),
                params.get("open_timeout_s", 10.0),
                params.get("open_workers", 32), os_limits,
                params.get("warmup_conns"),
                params.get("warmup_settle_s", 1.0), verbose,
            )
        elif kind == "active-stream":
            out = run_stream_point(
                server, point["conns"], point["streams_per_conn"], repeats,
                params.get("settle_s", 3.0), params.get("rss_samples", 5),
                params.get("rss_interval_s", 0.2),
                params.get("open_timeout_s", 10.0),
                params.get("open_workers", 32), os_limits,
                params.get("warmup_conns"),
                params.get("warmup_settle_s", 1.0), verbose,
            )
        elif kind == "tls-handshake":
            assert tls_spec is not None
            out = run_handshake_point(
                server, tls_spec, point["handshakes_per_rep"], repeats,
                params.get("workers", 16), cpus, verbose,
            )
        elif kind == "storm":
            out = run_storm_point(
                server, point["duration_s"], repeats,
                params.get("workers", 32), verbose,
            )
        else:
            raise ValueError(f"unknown SB-12 kind '{kind}'")
        cell.update(out["patch"])
        cell["repeats"] = out.get("repeats", [])
        if "summary" in out:
            cell["summary"] = out["summary"]
        for extra in ("os_limit_hit", "established_fraction", "rst_count", "warmup"):
            if extra in out:
                cell[extra] = out[extra]
        return cell
    finally:
        terminate(server.proc)


def write_report(out_dir: Path, report: Dict[str, Any]) -> Path:
    path = out_dir / "report.json"
    with open(path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
        f.write("\n")
    return path


# ---------------------------------------------------------------------------
# Self-test (no peer binaries needed; exercises framing + invalid marking)
# ---------------------------------------------------------------------------

def self_test() -> int:
    failures = []

    def check(name: str, cond: bool, detail: str = "") -> None:
        print(f"[self-test] {'PASS' if cond else 'FAIL'} {name} {detail}")
        if not cond:
            failures.append(name)

    # HPACK literal shape: 0x00, name-len, name, val-len, value.
    enc = hpack_literal(b":method", b"POST")
    check("hpack-literal", enc == b"\x00\x07:method\x04POST", enc.hex())
    # Frame header: len(24) type flags stream(31).
    fr = h2_frame(1, 0x4, 1, b"ab")
    check(
        "h2-frame",
        fr == b"\x00\x00\x02\x01\x04\x00\x00\x00\x01ab",
        fr.hex(),
    )
    block = grpc_bidi_headers_block("127.0.0.1:1")
    check("bidi-block", BIDI_PATH.encode() in block and len(block) < 256)
    # OS limits record is non-empty and names the fd ceiling.
    limits = collect_os_limits()
    check(
        "os-limits",
        isinstance(limits.get("rlimit_nofile_soft"), int)
        and limits["rlimit_nofile_soft"] > 0,
        f"nofile_soft={limits.get('rlimit_nofile_soft')}",
    )
    # RSS sampler reads our own process.
    rss = sample_rss_bytes(os.getpid())
    check("rss-self", isinstance(rss, int) and rss > 0, f"rss={rss}")
    # Preflight trips deterministically above the fd budget.
    fake = dict(limits, rlimit_nofile_soft=1024)
    note = preflight_budget(100_000, fake)
    check("preflight-fd-invalid", note is not None and "ulimit" in note, (note or "")[:80])
    note_ok = preflight_budget(10, dict(limits, rlimit_nofile_soft=100000))
    ports = ephemeral_port_budget(limits)
    if ports is not None and ports < 10:
        check("preflight-small", note_ok is not None, "tiny port range trips first")
    else:
        check("preflight-small", note_ok is None, (note_ok or "")[:80])
    # Errno classification: EMFILE -> invalid; refused+live -> invalid.
    hit = classify_socket_error(OSError(errno.EMFILE, "x"), True)
    check("classify-emfile", isinstance(hit, OsLimitHit))
    hit2 = classify_socket_error(OSError(errno.ECONNREFUSED, "x"), True)
    check("classify-refused-live", isinstance(hit2, OsLimitHit))
    no_hit = classify_socket_error(OSError(errno.ECONNREFUSED, "x"), False)
    check("classify-refused-dead", no_hit is None)
    # Roundtrip: preface against a stub h2 server over loopback.
    stub_ok = stub_preface_roundtrip()
    check("stub-preface-roundtrip", stub_ok)
    print(f"[self-test] {len(failures)} failure(s)")
    return 1 if failures else 0


def stub_preface_roundtrip() -> bool:
    """Serve one stub SETTINGS; open_idle_connection must establish."""
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(10.0)
    port = listener.getsockname()[1]
    errors: List[str] = []

    def serve() -> None:
        try:
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(10.0)
                # Read magic + SETTINGS, reply with server SETTINGS.
                want = len(H2_MAGIC) + 9
                buf = b""
                while len(buf) < want:
                    chunk = conn.recv(want - len(buf))
                    if not chunk:
                        errors.append("eof")
                        return
                    buf += chunk
                if not buf.startswith(H2_MAGIC):
                    errors.append("bad-magic")
                    return
                conn.sendall(H2_EMPTY_SETTINGS)
                # Expect the ACK, then hold briefly.
                ack = conn.recv(9)
                if ack != H2_SETTINGS_ACK:
                    errors.append(f"bad-ack:{ack.hex()}")
                    return
                time.sleep(0.3)
        except Exception as e:  # noqa: BLE001 - self-test evidence
            errors.append(f"{type(e).__name__}:{e}")
        finally:
            listener.close()

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    try:
        sock = open_idle_connection("127.0.0.1", port, None, None, 5.0)
    except Exception as e:  # noqa: BLE001 - self-test evidence
        errors.append(f"open:{type(e).__name__}:{e}")
        thread.join(timeout=5.0)
        print(f"[self-test] stub errors: {errors}")
        return False
    # Held-open stream HEADERS must serialize without error.
    try:
        block = grpc_bidi_headers_block(f"127.0.0.1:{port}")
        sock.sendall(h2_frame(1, 0x4, 1, block))
        sock.close()
    except OSError as e:
        errors.append(f"send:{e}")
    thread.join(timeout=5.0)
    if errors:
        print(f"[self-test] stub errors: {errors}")
    return not errors


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(
        description="SB-12 connection-scale scenarios (idle RSS, stream RSS, TLS rate, storm rate)."
    )
    parser.add_argument("--scenario", default=None)
    parser.add_argument("--out-dir", default="target/stack-matrix/sb12")
    parser.add_argument("--server-peers", default=None)
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--repeats", type=int, default=None)
    parser.add_argument("--include-optional", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verbose", "-v", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.scenario:
        parser.error("--scenario is required (e.g. conn-scale-idle)")
    code, _ = run_scenario(
        args.scenario,
        Path(args.out_dir),
        server_peers=args.server_peers.split(",") if args.server_peers else None,
        smoke=args.smoke,
        repeats_override=args.repeats,
        include_optional=args.include_optional,
        verbose=args.verbose,
    )
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
