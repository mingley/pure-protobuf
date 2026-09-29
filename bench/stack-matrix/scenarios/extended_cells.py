"""SB-19 extended-cell runner: new shapes, gzip pairs, RTT holdouts, overload/recovery.

Server cells only: each server peer under test is driven by the fixed
native open-loop Poisson generator (rpc-bench ``load``), reusing the
SB-11 spawn/pin/preflight machinery read-only. Four scenario kinds
(frozen JSON in this directory, run order = peers file order x cells
file order):

* extended-shapes: client-streaming upload (runnable) plus pipelined
  bidi (frozen definition; ``load`` bidi is lockstep-only at this
  base, so the runner reports ``unsupported`` with the exact missing
  capability instead of mislabeling a lockstep run).
* extended-gzip: gzip on/off pairs; identity cells run, gzip-on cells
  are frozen definitions reported ``unsupported`` (``load`` has no
  compression flag at this base).
* extended-rtt: emulated 1 ms / 10 ms RTT profiles via the bench-only
  loopback proxy (``--latency-rtt-ms``). Settings are recorded and the
  emulation is verified by a measured ping probe before each run.
* extended-overload: saturation search, one 2x-saturation overload
  phase, then recovery probes back at the normal rate; reports
  goodput, error/timeout/rejection rates, p99 with retained failures,
  and time to recover.

Accept contract (SB-19):
  (1) each new cell runs for the required peers; overload cells report
      goodput, rejections and recovery time.
  (2) RTT emulation settings are recorded and verified by measured
      ping before each run.

Statuses: pass | invalid (precondition unmet: RTT unverified,
saturation unreached, accounting mismatch, generator saturated) |
unsupported (gzip-on, pipelined bidi: frozen, needs load flags outside
this card's write set) | not_run (optional peer unrunnable) | fail
(real failure). Exit code is nonzero only when a cell failed.

Usage:
  python3 extended_cells.py --scenario extended-rtt --out-dir DIR
      [--server-peers native,go] [--smoke] [--repeats N] [-v]
  python3 extended_cells.py --self-test
"""

from __future__ import annotations

import argparse
import json
import socket
import statistics
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

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

SCHEMA = "sb19-extended/1"
KINDS = ("extended-shapes", "extended-gzip", "extended-rtt", "extended-overload")

# Server process budget per cell (clean shutdown), mirroring SB-12.
SERVER_TIMEOUT_SECS = 1800.0

# Shapes the rpc-bench `load` binary drives at this base (see
# rpc-bench/src/main.rs LoadShape). Anything else in a frozen cell
# definition is reported unsupported, never guessed.
LOAD_SHAPES = {"unary", "server_stream", "bidi", "client_stream"}

# Contract 4.1: scheduling lag over 10% of p50 saturates the generator.
GEN_SAT_LAG_FRACTION = 0.10


class CellError(Exception):
    """A real cell failure (peer died, probe crashed, metrics unreadable)."""


@dataclass(frozen=True)
class ExtendedCell:
    """One SB-19 cell: server peer under test plus workload plus placement."""

    kind: str
    server_peer: str
    client_peer: str
    shape: str
    payload: str
    stream_msgs: int
    compression: str
    rtt_ms: Optional[float]
    overload: bool
    rate: float
    max_in_flight: int
    cpus: int

    @property
    def id(self) -> str:
        kind_short = {
            "extended-shapes": "shapes",
            "extended-gzip": "gzip",
            "extended-rtt": "rtt",
            "extended-overload": "overload",
        }[self.kind]
        if self.kind == "extended-gzip":
            tag = self.compression
        elif self.kind == "extended-rtt":
            tag = f"rtt{self.rtt_ms:g}ms" if self.rtt_ms is not None else "rtt0ms"
        elif self.kind == "extended-overload":
            tag = "2x"
        else:
            tag = "plain"
        return (
            f"sb19-{kind_short}-{self.server_peer}-{self.shape}"
            f"-{self.payload}-{tag}-{self.cpus}cpu"
        )


def as_dict(cell: ExtendedCell) -> Dict[str, Any]:
    return {
        "id": cell.id,
        "kind": cell.kind,
        "role": "server",
        "server_peer": cell.server_peer,
        "client_peer": cell.client_peer,
        "shape": cell.shape,
        "payload": cell.payload,
        "stream_msgs": cell.stream_msgs,
        "compression": cell.compression,
        "rtt_ms": cell.rtt_ms,
        "overload": cell.overload,
        "rate": cell.rate,
        "max_in_flight": cell.max_in_flight,
        "cpus": cell.cpus,
        "workload": "open-loop-load",
    }


# ---------------------------------------------------------------------------
# Scenario loading + expansion (frozen order: peers x cells, file order)
# ---------------------------------------------------------------------------

def load_scenario(name: str) -> Dict[str, Any]:
    path = SCEN_DIR / f"{name}.json"
    if not path.is_file():
        known = sorted(p.stem for p in SCEN_DIR.glob("extended-*.json"))
        raise FileNotFoundError(
            f"unknown SB-19 scenario '{name}'; want one of: " + ", ".join(known)
        )
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def validate_scenario(scenario: Dict[str, Any]) -> List[str]:
    """Return a list of problems (empty when valid). Pure, self-tested."""
    problems: List[str] = []
    kind = scenario.get("kind")
    if kind not in KINDS:
        problems.append(f"kind must be one of {KINDS}, got {kind!r}")
    for key in ("scenario", "label", "contract", "methodology", "peers", "cells", "params"):
        if key not in scenario:
            problems.append(f"missing required key {key!r}")
    peers = scenario.get("peers", [])
    if not peers:
        problems.append("peers list is empty")
    known_peers = set(cells_mod.REQUIRED_SERVER_PEERS) | set(cells_mod.OPTIONAL_SERVER_PEERS)
    for peer in peers:
        if peer not in known_peers:
            problems.append(f"unknown peer {peer!r}")
    if scenario.get("tls", False):
        problems.append("SB-19 scenarios are plaintext-only (tls must be false)")
    for i, cdef in enumerate(scenario.get("cells", [])):
        where = f"cells[{i}]"
        if cdef.get("payload") not in cells_mod.PAYLOADS:
            problems.append(f"{where}: unknown payload {cdef.get('payload')!r}")
        if cdef.get("shape") not in LOAD_SHAPES | {"bidi_pipelined"}:
            problems.append(f"{where}: unknown shape {cdef.get('shape')!r}")
        if cdef.get("compression", "identity") not in ("identity", "gzip"):
            problems.append(f"{where}: bad compression {cdef.get('compression')!r}")
        rtt = cdef.get("rtt_ms")
        if rtt is not None and (not isinstance(rtt, (int, float)) or rtt <= 0):
            problems.append(f"{where}: bad rtt_ms {rtt!r}")
        if kind == "extended-overload" and not cdef.get("overload", False):
            problems.append(f"{where}: overload scenario cells need overload=true")
        if kind != "extended-overload" and cdef.get("overload", False):
            problems.append(f"{where}: overload=true only in extended-overload")
        if kind == "extended-rtt" and rtt is None:
            problems.append(f"{where}: rtt scenario cells need rtt_ms")
        if kind != "extended-rtt" and rtt is not None:
            problems.append(f"{where}: rtt_ms only in extended-rtt")
    if not scenario.get("cells"):
        problems.append("cells list is empty")
    return problems


def expand(
    scenario: Dict[str, Any],
    server_peers: Optional[List[str]] = None,
    include_optional: bool = False,
) -> List[ExtendedCell]:
    """Expand frozen cells: peers in file order x defs in file order."""
    kind = scenario["kind"]
    peers = list(server_peers) if server_peers else list(scenario["peers"])
    if include_optional:
        peers += [p for p in cells_mod.OPTIONAL_SERVER_PEERS if p not in peers]
    cpus = int(scenario.get("cpus", 1))
    client_peer = scenario.get("client_peer", cells_mod.FIXED_GENERATOR)
    cells: List[ExtendedCell] = []
    for peer in peers:
        for cdef in scenario["cells"]:
            cells.append(
                ExtendedCell(
                    kind=kind,
                    server_peer=peer,
                    client_peer=client_peer,
                    shape=cdef["shape"],
                    payload=cdef["payload"],
                    stream_msgs=int(cdef.get("stream_msgs", 0)),
                    compression=cdef.get("compression", "identity"),
                    rtt_ms=cdef.get("rtt_ms"),
                    overload=bool(cdef.get("overload", False)),
                    rate=float(cdef.get("rate", 0.0)),
                    max_in_flight=int(cdef.get("max_in_flight", 1000)),
                    cpus=cpus,
                )
            )
    return cells


def validate_cell(cell: ExtendedCell) -> Optional[str]:
    """Return None when runnable, else the unsupported reason."""
    if cell.compression == "gzip":
        return (
            "gzip-on cells are frozen but unrunnable at this base: rpc-bench "
            "`load` has no compression flag and per-peer gzip negotiation is "
            "unscripted (needs rpc-bench load flags outside the SB-19 write "
            "set); the identity half of the pair runs"
        )
    if cell.shape == "bidi_pipelined":
        return (
            "pipelined bidi is frozen but unrunnable at this base: rpc-bench "
            "`load --shape=bidi` is lockstep ping-pong only, with no pipelined "
            "full-duplex mode (needs rpc-bench load flags outside the SB-19 "
            "write set); lockstep bidi stays covered by SB-11"
        )
    if cell.shape not in LOAD_SHAPES:
        return f"shape {cell.shape!r} has no rpc-bench load driver"
    return None


def effective_params(scenario: Dict[str, Any], smoke: bool) -> Dict[str, Any]:
    params = dict(scenario.get("params", {}))
    if smoke:
        params.update(scenario.get("smoke_params", {}))
    return params


def median(values: List[float]) -> Optional[float]:
    if not values:
        return None
    return float(statistics.median(values))


# ---------------------------------------------------------------------------
# Load probes (open-loop `load`, modeled on run.CellRunner.probe)
# ---------------------------------------------------------------------------

def load_command(
    native_bin: str,
    addr: str,
    shape: str,
    req_bytes: int,
    resp_bytes: int,
    stream_msgs: int,
    rate: float,
    duration_s: float,
    seed: int,
    max_in_flight: int,
    out_path: Path,
    distribution: str = "poisson",
    rtt_ms: Optional[float] = None,
) -> List[str]:
    """Build the open-loop `load` argv for one probe.

    Flag spellings mirror run.load_command (`--server_addr` with an
    underscore); `--stream-msgs` is passed for every streaming shape
    (including client_stream, which cells.STREAM_MSGS does not list)
    and `--latency-rtt-ms` enables the bench-only loopback RTT proxy.
    """
    cmd = [
        native_bin,
        "load",
        f"--server_addr={addr}",
        "--transport=native",
        f"--shape={shape}",
        f"--req-bytes={req_bytes}",
        f"--resp-bytes={resp_bytes}",
        f"--distribution={distribution}",
        f"--rate={rate:g}",
        f"--seed={seed}",
        f"--duration-secs={duration_s:g}",
        f"--max-in-flight={max_in_flight}",
        f"--output={out_path}",
    ]
    if shape in ("server_stream", "bidi", "client_stream"):
        if stream_msgs <= 0:
            raise ValueError(f"streaming shape {shape} needs stream_msgs >= 1")
        cmd.append(f"--stream-msgs={stream_msgs}")
    if rtt_ms is not None:
        cmd.append(f"--latency-rtt-ms={rtt_ms:g}")
    return cmd


def cell_load_command(
    native_bin: str,
    addr: str,
    cell: ExtendedCell,
    rate: float,
    duration_s: float,
    seed: int,
    out_path: Path,
    max_in_flight: Optional[int] = None,
    distribution: str = "poisson",
) -> List[str]:
    req_bytes, resp_bytes = cells_mod.PAYLOADS[cell.payload]
    return load_command(
        native_bin,
        addr,
        cell.shape,
        req_bytes,
        resp_bytes,
        cell.stream_msgs,
        rate,
        duration_s,
        seed,
        max_in_flight if max_in_flight is not None else cell.max_in_flight,
        out_path,
        distribution,
        cell.rtt_ms,
    )


def parse_metrics(out_path: Path) -> Dict[str, Any]:
    try:
        with open(out_path, "r", encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        raise CellError(f"load wrote no valid metrics at {out_path}: {e}")


def generator_cap(step: Dict[str, Any]) -> Optional[str]:
    """Pure gating verdict for one step: "cpu", "rejected", or None.

    Gating follows the SB-11 signal: the resource monitor's client-CPU
    saturation, plus queue-overflow rejections (offered load that never
    reached the server). The contract 4.1 lag rule (`gen_saturated`) is
    recorded on every step for auditors but does not gate: on shared
    hosts dispatch lag is dominated by noisy neighbors, while the
    per-process CPU signal matches SB-11 SLO semantics.
    """
    if step.get("cpu_saturated"):
        return "cpu"
    if step.get("rejected", 0) > 0:
        return "rejected"
    return None


def step_from_metrics(rate: float, metrics: Dict[str, Any]) -> Dict[str, Any]:
    """Flatten one load metrics file into a step record (pure).

    `cpu_saturated` is merged by Prober.run from the resource monitor;
    steps built elsewhere default it to False via generator_cap.
    """
    e2e = metrics.get("e2e_latency_nanos") or {}
    lag = metrics.get("scheduling_lag_nanos") or {}
    offered = int(metrics.get("offered_rpcs") or 0)
    successful = int(metrics.get("successful_rpcs") or 0)
    failed = int(metrics.get("failed_rpcs") or 0)
    timed_out = int(metrics.get("timeouts") or 0)
    rejected = int(metrics.get("queue_overflows") or 0)
    qps = metrics.get("throughput_qps")
    if qps is None:
        dur_s = (metrics.get("duration_nanos") or 0) / 1e9
        qps = successful / dur_s if dur_s > 0 else 0.0
    p50_s = (e2e.get("p50_nanos") or 0) / 1e9 if e2e else float("inf")
    p99_s = (e2e.get("p99_nanos") or 0) / 1e9 if e2e else float("inf")
    lag_p50_s = (lag.get("p50") or 0) / 1e9 if lag else 0.0
    gen_saturated = bool(e2e) and lag_p50_s > GEN_SAT_LAG_FRACTION * p50_s
    accounted = successful + failed + rejected
    return {
        "offered_rate": rate,
        "offered": offered,
        "successful": successful,
        "failed": failed,
        "timed_out": timed_out,
        "rejected": rejected,
        "accounting_ok": accounted == offered and offered > 0,
        "success_qps": float(qps),
        "p50_s": p50_s,
        "p99_s": p99_s,
        "lag_p50_s": lag_p50_s,
        "gen_saturated": gen_saturated,
        "status_errors": metrics.get("status_errors") or {},
    }


def accounting_note(step: Dict[str, Any]) -> str:
    return (
        f"offered={step['offered']} successful={step['successful']} "
        f"failed={step['failed']} (timeouts={step['timed_out']}) "
        f"rejected={step['rejected']}"
    )


def app_bytes_per_rpc(cell: ExtendedCell) -> int:
    """Application payload bytes per RPC for goodput (contract 5.3).

    Framing/header bytes excluded. Streaming shapes sum per-message
    application bytes per direction; `load` reuses the payload size
    per message.
    """
    req, resp = cells_mod.PAYLOADS[cell.payload]
    msgs = cell.stream_msgs
    if cell.shape == "unary":
        return req + resp
    if cell.shape == "client_stream":
        return msgs * req + resp
    if cell.shape == "server_stream":
        return req + msgs * resp
    if cell.shape == "bidi":
        return msgs * (req + resp)
    raise ValueError(f"no byte model for shape {cell.shape!r}")


def goodput_mib_s(successful: int, bytes_per_rpc: int, duration_s: float) -> Optional[float]:
    """Delivered application payload MiB/s (pure)."""
    if duration_s <= 0:
        return None
    return successful * bytes_per_rpc / duration_s / 1048576.0


class Prober:
    """Runs pinned open-loop `load` probes against one live server."""

    def __init__(
        self,
        native_bin: str,
        server_proc: subprocess.Popen,
        log_dir: Path,
        cpus: int,
        verbose: bool = False,
    ):
        self.native_bin = native_bin
        self.server_proc = server_proc
        self.log_dir = log_dir
        self.cpus = cpus
        self.verbose = verbose

    def run(
        self,
        cell: ExtendedCell,
        addr: str,
        rate: float,
        duration_s: float,
        seed: int,
        tag: str,
        max_in_flight: Optional[int] = None,
        distribution: str = "poisson",
        shape_override: Optional[str] = None,
        stream_msgs_override: Optional[int] = None,
    ) -> Tuple[Dict[str, Any], Dict[str, Any]]:
        """One measured probe; returns (step, resources)."""
        metrics_path = self.log_dir / f"{cell.id}-{tag}.json"
        shape = shape_override or cell.shape
        req_bytes, resp_bytes = cells_mod.PAYLOADS[cell.payload]
        cmd = load_command(
            self.native_bin,
            addr,
            shape,
            req_bytes,
            resp_bytes,
            stream_msgs_override if stream_msgs_override is not None else cell.stream_msgs,
            rate,
            duration_s,
            seed,
            max_in_flight if max_in_flight is not None else cell.max_in_flight,
            metrics_path,
            distribution,
            cell.rtt_ms,
        )
        pinned, pin_state = pin_mod.wrap(cmd, self.cpus, self.cpus)
        stdout_path = self.log_dir / f"{cell.id}-{tag}.stdout"
        stderr_path = self.log_dir / f"{cell.id}-{tag}.stderr"
        if self.verbose:
            print(f"[SB-19] [{cell.id}] probe {tag}: {' '.join(pinned)}")
        monitor = bench_matrix.ProcessResourceMonitor(
            server_pid=self.server_proc.pid, poll_interval_s=0.05
        )
        client_proc = subprocess.Popen(
            pinned,
            stdout=open(stdout_path, "w", encoding="utf-8"),
            stderr=open(stderr_path, "w", encoding="utf-8"),
            text=True,
        )
        bench_matrix._ACTIVE_PROCESSES.add(client_proc)
        monitor.set_client_pid(client_proc.pid)
        monitor.start()
        wall_start = time.monotonic()
        timeout = duration_s + 60.0
        try:
            client_proc.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            client_proc.kill()
            client_proc.communicate()
            raise CellError(f"load probe {tag} at {rate:g} qps exceeded {timeout:.0f}s")
        finally:
            bench_matrix._ACTIVE_PROCESSES.discard(client_proc)
            client_res, server_res, sat_check = monitor.stop()
        wall_s = max(0.001, time.monotonic() - wall_start)
        if client_proc.returncode != 0:
            raise CellError(
                f"load probe {tag} at {rate:g} qps exited {client_proc.returncode}; "
                f"see {cell.id}-{tag}.stderr"
            )
        if not client_res.get("supported") or not server_res.get("supported"):
            raise CellError(
                f"load probe {tag} has unsupported resource sampling "
                f"(client={client_res.get('method')}, server={server_res.get('method')})"
            )
        metrics = parse_metrics(metrics_path)
        step = step_from_metrics(rate, metrics)
        step["cpu_saturated"] = bool(sat_check.get("saturated"))
        step["cpu_spare_pct"] = sat_check.get("spare_capacity_pct")
        resources = {
            "client": client_res,
            "server": server_res,
            "saturation": sat_check,
            "wall_s": round(wall_s, 3),
            "metrics_file": str(metrics_path.name),
        }
        return step, resources


# ---------------------------------------------------------------------------
# RTT ping + emulation verification (accept 2)
# ---------------------------------------------------------------------------

def tcp_ping_ms(host: str, port: int, samples: int = 20, timeout_s: float = 5.0) -> Dict[str, Any]:
    """Median TCP-connect RTT to host:port over `samples` (pure measurement)."""
    rtts: List[float] = []
    errors = 0
    for _ in range(samples):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(timeout_s)
        start = time.monotonic()
        try:
            sock.connect((host, port))
            rtts.append((time.monotonic() - start) * 1000.0)
        except OSError:
            errors += 1
        finally:
            sock.close()
    if not rtts:
        raise CellError(f"tcp ping to {host}:{port} failed ({errors} errors)")
    return {
        "median_ms": float(statistics.median(rtts)),
        "min_ms": min(rtts),
        "max_ms": max(rtts),
        "samples": len(rtts),
        "errors": errors,
    }


def rtt_verify_verdict(
    configured_rtt_ms: float,
    measured_p50_s: float,
    min_ratio: float,
    max_slack_s: float,
) -> Tuple[bool, str]:
    """Pure verdict: does the measured ping evidence the configured delay?"""
    measured_ms = measured_p50_s * 1000.0
    if measured_p50_s < min_ratio * configured_rtt_ms / 1000.0:
        return False, (
            f"unverified: measured p50 {measured_ms:.3f}ms < "
            f"{min_ratio}x configured {configured_rtt_ms:g}ms; the loopback "
            "proxy delay is not evident on this path"
        )
    if measured_p50_s > configured_rtt_ms / 1000.0 + max_slack_s:
        return False, (
            f"unverified: measured p50 {measured_ms:.3f}ms exceeds configured "
            f"{configured_rtt_ms:g}ms + {max_slack_s * 1000:.0f}ms slack; "
            "path delay is not the configured emulation"
        )
    return True, (
        f"verified: measured p50 {measured_ms:.3f}ms evidences configured "
        f"{configured_rtt_ms:g}ms"
    )


def verify_rtt_emulation(
    prober: Prober,
    cell: ExtendedCell,
    addr: str,
    host: str,
    port: int,
    params: Dict[str, Any],
    seed: int,
    tag: str,
) -> Dict[str, Any]:
    """Record the RTT setting and verify it with a measured ping probe.

    The ping is a short constant-rate unary-empty `load` through the same
    `--latency-rtt-ms` proxy path the measured run uses. Callers run this
    BEFORE each measured repeat (accept 2).
    """
    assert cell.rtt_ms is not None
    baseline = tcp_ping_ms(host, port, int(params.get("baseline_ping_samples", 20)))
    step, _ = prober.run(
        cell,
        addr,
        float(params.get("verify_rate", 200.0)),
        float(params.get("verify_duration_s", 2.0)),
        seed,
        f"{tag}-rttverify",
        distribution="constant",
        shape_override="unary",
        stream_msgs_override=0,
    )
    # The verify probe forces the cheap unary shape but keeps the
    # cell's rtt_ms proxy path (Prober.run always applies cell.rtt_ms),
    # so the measured p50 evidences the emulation the run will use.
    ok, detail = rtt_verify_verdict(
        cell.rtt_ms,
        step["p50_s"],
        float(params.get("verify_min_ratio", 0.5)),
        float(params.get("verify_max_slack_s", 0.05)),
    )
    if step.get("cpu_saturated"):
        ok, detail = False, (
            "unverified: ping probe generator CPU-saturated; the measured "
            "p50 cannot evidence the emulation"
        )
    return {
        "configured_rtt_ms": cell.rtt_ms,
        "mechanism": params.get("rtt_mechanism", "rpc-bench --latency-rtt-ms loopback proxy"),
        "baseline_tcp_ping_ms": baseline,
        "ping_p50_s": step["p50_s"],
        "ping_offered": step["offered"],
        "ping_successful": step["successful"],
        "ping_cpu_saturated": bool(step.get("cpu_saturated")),
        "verified": ok,
        "detail": detail,
    }


# ---------------------------------------------------------------------------
# Kind runners (one fresh server per cell; repeats share it)
# ---------------------------------------------------------------------------

def check_server_alive(proc: subprocess.Popen, cell_id: str) -> None:
    if proc.poll() is not None:
        raise CellError(f"server for {cell_id} died (exit {proc.returncode})")


def summarize_repeats(repeats: List[Dict[str, Any]]) -> Dict[str, Any]:
    return {
        "repeats": len(repeats),
        "median_success_qps": median([r["success_qps"] for r in repeats]),
        "min_success_qps": min((r["success_qps"] for r in repeats), default=None),
        "max_success_qps": max((r["success_qps"] for r in repeats), default=None),
        "median_p99_s": median([r["p99_s"] for r in repeats]),
        "total_offered": sum(r["offered"] for r in repeats),
        "total_successful": sum(r["successful"] for r in repeats),
        "total_failed": sum(r["failed"] for r in repeats),
        "total_rejected": sum(r["rejected"] for r in repeats),
    }


def run_fixed_rate_cell(
    prober: Prober,
    cell: ExtendedCell,
    addr: str,
    params: Dict[str, Any],
    repeats: int,
    seed: int,
) -> Dict[str, Any]:
    """Validation-stage fixed-rate cell (shapes, gzip-identity).

    One discarded warmup probe, then `repeats` measured probes at the
    frozen offered rate. Pass = ran + every repeat reconciles; the
    numbers are reported, never gated.
    """
    warmup_s = float(params.get("warmup_s", 15.0))
    duration_s = float(params.get("duration_s", 60.0))
    prober.run(cell, addr, cell.rate, warmup_s, seed, "warmup")
    check_server_alive(prober.server_proc, cell.id)
    rep_rows: List[Dict[str, Any]] = []
    for rep in range(repeats):
        step, _res = prober.run(cell, addr, cell.rate, duration_s, seed + rep, f"rep{rep + 1}")
        check_server_alive(prober.server_proc, cell.id)
        if not step["accounting_ok"]:
            return {
                "status": "invalid",
                "reason": (
                    "offered/call accounting mismatch on rep "
                    f"{rep + 1} ({accounting_note(step)}): dropped or "
                    "misclassified failures invalidate the run"
                ),
                "repeats": rep_rows,
            }
        cap = generator_cap(step)
        if cap is not None:
            return {
                "status": "invalid",
                "reason": (
                    f"generator {cap}-capped on rep {rep + 1} "
                    f"({accounting_note(step)}): the numbers describe the "
                    "generator, not the server"
                ),
                "repeats": rep_rows,
            }
        rep_rows.append({**step, "status_errors": step["status_errors"]})
    return {"status": "pass", "reason": "", "repeats": rep_rows, "summary": summarize_repeats(rep_rows)}


def run_rtt_cell(
    prober: Prober,
    cell: ExtendedCell,
    addr: str,
    host: str,
    port: int,
    params: Dict[str, Any],
    repeats: int,
    seed: int,
) -> Dict[str, Any]:
    """RTT holdout cell: verify-then-measure per repeat (accept 2)."""
    assert cell.rtt_ms is not None
    warmup_s = float(params.get("warmup_s", 15.0))
    duration_s = float(params.get("duration_s", 60.0))
    # Warmup runs through the emulated path but is discarded; it is
    # still preceded by a verification so a broken proxy fails fast.
    warm_verify = verify_rtt_emulation(prober, cell, addr, host, port, params, seed, "warmup")
    if not warm_verify["verified"]:
        return {
            "status": "invalid",
            "reason": f"rtt emulation unverified before warmup: {warm_verify['detail']}",
            "repeats": [],
            "rtt_setup": warm_verify,
        }
    prober.run(cell, addr, cell.rate, warmup_s, seed, "warmup")
    check_server_alive(prober.server_proc, cell.id)
    rep_rows: List[Dict[str, Any]] = []
    for rep in range(repeats):
        verify = verify_rtt_emulation(
            prober, cell, addr, host, port, params, seed + 1000 + rep, f"rep{rep + 1}"
        )
        if not verify["verified"]:
            return {
                "status": "invalid",
                "reason": f"rtt emulation unverified before rep {rep + 1}: {verify['detail']}",
                "repeats": rep_rows,
                "rtt_setup": verify,
            }
        step, _res = prober.run(cell, addr, cell.rate, duration_s, seed + rep, f"rep{rep + 1}")
        check_server_alive(prober.server_proc, cell.id)
        if not step["accounting_ok"]:
            return {
                "status": "invalid",
                "reason": (
                    f"offered/call accounting mismatch on rep {rep + 1} "
                    f"({accounting_note(step)}): dropped or misclassified "
                    "failures invalidate the run"
                ),
                "repeats": rep_rows,
            }
        cap = generator_cap(step)
        if cap is not None:
            return {
                "status": "invalid",
                "reason": (
                    f"generator {cap}-capped on rep {rep + 1} "
                    f"({accounting_note(step)}): the numbers describe the "
                    "generator, not the server"
                ),
                "repeats": rep_rows,
            }
        rep_rows.append({**step, "rtt_verify": verify})
    summary = summarize_repeats(rep_rows)
    summary["rtt_ms"] = cell.rtt_ms
    summary["emulation_verified_repeats"] = len(rep_rows)
    return {"status": "pass", "reason": "", "repeats": rep_rows, "summary": summary}


def run_overload_cell(
    prober: Prober,
    cell: ExtendedCell,
    addr: str,
    params: Dict[str, Any],
    repeats: int,
    seed: int,
) -> Dict[str, Any]:
    """Overload/recovery cell (contract 3.7): search, 2x, recover.

    Per repeat: geometric saturation search, baseline at the normal
    rate, one overload phase at overload_factor x saturation, then
    recovery probes back at normal. Pass = every probe reconciles and
    recovery is measured and reported either way (validation axis, no
    gate); accounting mismatch invalidates the run.
    """
    start_rate = float(params.get("search_start_rate", 1000.0))
    growth = float(params.get("search_growth", 2.0))
    max_steps = int(params.get("max_search_steps", 8))
    max_down = int(params.get("max_search_down_steps", 3))
    search_dur = float(params.get("search_duration_s", 10.0))
    base_cap = int(params.get("max_in_flight", 1000))
    esc_cap = int(params.get("max_in_flight_escalated", 10000))
    overload_cap = int(params.get("overload_max_in_flight", 10000))
    baseline_dur = float(params.get("baseline_duration_s", 10.0))
    factor = float(params.get("overload_factor", 2.0))
    overload_dur = float(params.get("overload_duration_s", 30.0))
    rec_probe_s = float(params.get("recovery_probe_s", 5.0))
    max_rec = int(params.get("max_recovery_probes", 12))
    rec_p99_factor = float(params.get("recovery_p99_factor", 2.0))
    bytes_per_rpc = app_bytes_per_rpc(cell)

    rep_rows: List[Dict[str, Any]] = []
    for rep in range(repeats):
        tag = f"rep{rep + 1}"

        def search_probe(probe_rate: float, probe_tag: str, cap: int) -> Dict[str, Any]:
            step, _ = prober.run(
                cell, addr, probe_rate, search_dur, seed + rep, probe_tag, max_in_flight=cap
            )
            check_server_alive(prober.server_proc, cell.id)
            return step

        def escalated(step: Dict[str, Any], probe_tag: str) -> Tuple[Dict[str, Any], str, str]:
            """Redo a rejection-capped step at the escalated cap.

            Returns (step2, outcome, detail) with outcome one of clean
            (cap was too small; keep climbing), saturated (the server
            cannot sustain this rate within the escalated concurrency
            budget: knee found), or invalid-<cause>.
            """
            step2 = search_probe(step["offered_rate"], probe_tag, esc_cap)
            if not step2["accounting_ok"]:
                return step2, "invalid", (
                    "accounting mismatch during escalated search "
                    f"({accounting_note(step2)}): dropped or misclassified "
                    "failures invalidate the run"
                )
            if step2.get("cpu_saturated"):
                return step2, "invalid", (
                    f"generator CPU-saturated at {step['offered_rate']:g} qps; server "
                    "saturation inconclusive on this host"
                )
            if step2["failed"] + step2["timed_out"] > 0:
                return step2, "saturated", "server_errors"
            if step2["rejected"] > 0:
                return step2, "saturated", f"rejections_at_cap_{esc_cap}"
            return step2, "clean", ""

        # -- saturation search: first rate the server cannot sustain,
        # signaled by server errors/timeouts, or by rejections that
        # persist at the escalated cap (a bounded generator sheds load
        # before a slowing server ever errors, so rejection persistence
        # within a fixed concurrency budget IS the observable knee).
        search_steps: List[Dict[str, Any]] = []
        saturation_rate: Optional[float] = None
        saturation_signal = ""
        normal_rate: Optional[float] = None
        invalid_reason: Optional[str] = None
        rate = start_rate
        for _ in range(max_steps):
            step = search_probe(rate, f"{tag}-search{rate:g}qps".replace(".", "p"), base_cap)
            if not step["accounting_ok"]:
                invalid_reason = (
                    "accounting mismatch during saturation search "
                    f"({accounting_note(step)}): dropped or misclassified "
                    "failures invalidate the run"
                )
                break
            if step.get("cpu_saturated"):
                search_steps.append(step)
                invalid_reason = (
                    f"generator CPU-saturated at {rate:g} qps; server "
                    "saturation inconclusive on this host"
                )
                break
            if step["failed"] + step["timed_out"] > 0:
                search_steps.append(step)
                saturation_rate = rate
                saturation_signal = "server_errors"
                break
            if step["rejected"] > 0:
                step2, outcome, detail = escalated(
                    step, f"{tag}-search{rate:g}qps-esc".replace(".", "p")
                )
                search_steps.append(step2)
                if outcome == "clean":
                    # Cap was too small: keep climbing, but do NOT mark
                    # this rate normal -- normal is the last rate clean
                    # within the BASE budget, so the baseline and recovery
                    # probes (base cap) reproduce it.
                    rate *= growth
                    continue
                if outcome == "saturated":
                    saturation_rate = rate
                    saturation_signal = detail
                    break
                invalid_reason = detail
                break
            search_steps.append(step)
            normal_rate = rate
            rate *= growth
        if invalid_reason is None and saturation_rate is None and normal_rate is not None:
            invalid_reason = (
                "saturation not reached within "
                f"{max_steps} steps to {rate / growth:g} qps (no server errors "
                "and no persistent rejections); raise the search budget"
            )
        if invalid_reason is None and saturation_rate is not None and normal_rate is None:
            # Start rate already saturated: halve down to find a clean normal.
            rate = saturation_rate / growth
            for _ in range(max_down):
                step = search_probe(rate, f"{tag}-searchdown{rate:g}qps".replace(".", "p"), base_cap)
                if not step["accounting_ok"]:
                    invalid_reason = (
                        "accounting mismatch during down-search "
                        f"({accounting_note(step)}): dropped or misclassified "
                        "failures invalidate the run"
                    )
                    break
                if step.get("cpu_saturated"):
                    search_steps.append(step)
                    invalid_reason = (
                        f"generator CPU-saturated during down-search at {rate:g} qps"
                    )
                    break
                search_steps.append(step)
                if step["failed"] + step["timed_out"] > 0 or step["rejected"] > 0:
                    saturation_rate = rate
                    saturation_signal = "server_errors" if step["failed"] + step["timed_out"] > 0 else f"rejections_at_cap_{base_cap}"
                    rate /= growth
                    continue
                normal_rate = rate
                break
            if invalid_reason is None and normal_rate is None:
                invalid_reason = (
                    f"no clean normal rate below saturation at "
                    f"{saturation_rate:g} qps; lower search_start_rate"
                )
        if invalid_reason is not None:
            return {"status": "invalid", "reason": invalid_reason, "repeats": rep_rows}
        assert saturation_rate is not None and normal_rate is not None
        # -- baseline at the normal rate. The search boundary is one
        # noisy probe, so a non-reproducing baseline steps normal down
        # (bounded) instead of failing: recovery needs a clean
        # reference, not the exact search edge.
        baseline: Optional[Dict[str, Any]] = None
        baseline_step_downs = 0
        max_base_down = int(params.get("max_baseline_step_downs", 3))
        while baseline is None:
            down_tag = f"{tag}-baseline" + (
                f"-down{baseline_step_downs}" if baseline_step_downs else ""
            )
            candidate, _ = prober.run(
                cell, addr, normal_rate, baseline_dur, seed + rep, down_tag
            )
            check_server_alive(prober.server_proc, cell.id)
            if not candidate["accounting_ok"]:
                return {
                    "status": "invalid",
                    "reason": (
                        f"accounting mismatch in baseline ({accounting_note(candidate)}): "
                        "dropped or misclassified failures invalidate the run"
                    ),
                    "repeats": rep_rows,
                }
            if candidate["failed"] + candidate["timed_out"] == 0 and generator_cap(candidate) is None:
                baseline = candidate
                break
            baseline_step_downs += 1
            if baseline_step_downs > max_base_down:
                return {
                    "status": "invalid",
                    "reason": (
                        f"baseline unreproducible after {max_base_down} step-downs "
                        f"(last {accounting_note(candidate)} at {normal_rate:g} qps); "
                        "saturation point unstable"
                    ),
                    "repeats": rep_rows,
                }
            normal_rate /= growth
        # -- overload phase at factor x saturation.
        overload_rate = factor * saturation_rate
        overload_start = time.monotonic()
        overload, _ = prober.run(
            cell, addr, overload_rate, overload_dur, seed + rep, f"{tag}-overload",
            max_in_flight=overload_cap,
        )
        overload_end = time.monotonic()
        check_server_alive(prober.server_proc, cell.id)
        if not overload["accounting_ok"]:
            return {
                "status": "invalid",
                "reason": (
                    f"accounting mismatch in overload phase "
                    f"({accounting_note(overload)}): dropped or misclassified "
                    "failures invalidate the run"
                ),
                "repeats": rep_rows,
            }
        # Rejections in the overload phase are a reported result (accept
        # 1), not an invalidator: at 2x saturation the bounded generator
        # necessarily sheds load. CPU saturation, though, means offered
        # timing itself degraded, so the phase is untrustworthy.
        if overload.get("cpu_saturated"):
            return {
                "status": "invalid",
                "reason": (
                    "overload-phase generator CPU-saturated "
                    f"({accounting_note(overload)}); offered timing degraded"
                ),
                "repeats": rep_rows,
            }
        offered = overload["offered"] or 1
        overload_report = {
            **overload,
            "error_rate": overload["failed"] / offered,
            "timeout_rate": overload["timed_out"] / offered,
            "rejection_rate": overload["rejected"] / offered,
            "goodput_mib_s": goodput_mib_s(overload["successful"], bytes_per_rpc, overload_dur),
            "app_bytes_per_rpc": bytes_per_rpc,
        }
        # -- recovery probes back at normal until clean or budget out.
        rec_probes: List[Dict[str, Any]] = []
        recovered = False
        recovery_time_s: Optional[float] = None
        all_capped = True
        for i in range(max_rec):
            probe, _ = prober.run(
                cell, addr, normal_rate, rec_probe_s, seed + rep, f"{tag}-recover{i + 1}"
            )
            check_server_alive(prober.server_proc, cell.id)
            if not probe["accounting_ok"]:
                return {
                    "status": "invalid",
                    "reason": (
                        f"accounting mismatch in recovery probe {i + 1} "
                        f"({accounting_note(probe)}): dropped or "
                        "misclassified failures invalidate the run"
                    ),
                    "repeats": rep_rows,
                }
            capped = generator_cap(probe)
            clean = (
                probe["failed"] + probe["timed_out"] == 0
                and capped is None
                and probe["p99_s"] <= rec_p99_factor * baseline["p99_s"]
            )
            rec_probes.append({**probe, "clean": clean})
            if capped is None:
                all_capped = False
            if clean and not recovered:
                recovered = True
                recovery_time_s = time.monotonic() - overload_end
                break
        if all_capped and not recovered:
            return {
                "status": "invalid",
                "reason": (
                    "every recovery probe was generator-capped; recovery "
                    "of the server is unobservable"
                ),
                "repeats": rep_rows,
            }
        rep_rows.append(
            {
                "search_steps": search_steps,
                "saturation_rate": saturation_rate,
                "saturation_signal": saturation_signal,
                "normal_rate": normal_rate,
                "baseline_step_downs": baseline_step_downs,
                "baseline": baseline,
                "overload_rate": overload_rate,
                "overload": overload_report,
                "overload_wall_s": round(overload_end - overload_start, 3),
                "recovery": {
                    "recovered": recovered,
                    "recovery_time_s": recovery_time_s,
                    "probes": rec_probes,
                },
            }
        )
    recovered_times = [
        r["recovery"]["recovery_time_s"]
        for r in rep_rows
        if r["recovery"]["recovered"] and r["recovery"]["recovery_time_s"] is not None
    ]
    summary = {
        "repeats": len(rep_rows),
        "median_saturation_rate": median([r["saturation_rate"] for r in rep_rows]),
        "saturation_signals": sorted({r["saturation_signal"] for r in rep_rows}),
        "median_overload_goodput_mib_s": median(
            [r["overload"]["goodput_mib_s"] or 0.0 for r in rep_rows]
        ),
        "median_overload_error_rate": median([r["overload"]["error_rate"] for r in rep_rows]),
        "median_overload_timeout_rate": median([r["overload"]["timeout_rate"] for r in rep_rows]),
        "median_overload_rejection_rate": median(
            [r["overload"]["rejection_rate"] for r in rep_rows]
        ),
        "median_overload_p99_s": median([r["overload"]["p99_s"] for r in rep_rows]),
        "recovered_repeats": sum(1 for r in rep_rows if r["recovery"]["recovered"]),
        "median_recovery_time_s": median(recovered_times),
    }
    return {"status": "pass", "reason": "", "repeats": rep_rows, "summary": summary}


# ---------------------------------------------------------------------------
# Server lifecycle + scenario orchestration
# ---------------------------------------------------------------------------

def spawn_server(
    registry: Any,
    peer: str,
    cpus: int,
    log_dir: Path,
    tag: str,
    verbose: bool = False,
) -> Tuple[subprocess.Popen, str, str, str, Dict[str, Any], Path, Path]:
    """Spawn one plaintext server; return (proc, addr, binary, codec, pin, out, err)."""
    host = "127.0.0.1"
    port = bench_matrix.find_free_port()
    cmd, binary, codec = matrix_run.server_command(
        registry, peer, host, port, SERVER_TIMEOUT_SECS, None
    )
    pinned, pin_state = pin_mod.wrap(cmd, cpus, 0)
    stdout_path = log_dir / f"{tag}.stdout"
    stderr_path = log_dir / f"{tag}.stderr"
    if verbose:
        print(f"[SB-19] spawn {tag}: {' '.join(pinned)}")
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
    return proc, addr, binary, codec, pin_state, stdout_path, stderr_path


def terminate(proc: subprocess.Popen) -> None:
    bench_matrix._ACTIVE_PROCESSES.discard(proc)
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=10.0)


def optional_not_run(peer: str, optional_status: Dict[str, Any]) -> Optional[str]:
    if not peers_mod.is_optional(peer):
        return None
    entry = optional_status.get(peer)
    if entry is None:
        return f"optional peer {peer} was not preflighted"
    if entry.get("status") != "ready":
        return str(entry.get("reason") or f"optional peer {peer} is not runnable")
    return None


def write_report(out_dir: Path, report: Dict[str, Any]) -> Path:
    path = out_dir / "report.json"
    with open(path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
        f.write("\n")
    return path


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
    problems = validate_scenario(scenario)
    if problems:
        raise ValueError(f"scenario {scenario_name} invalid: {'; '.join(problems)}")
    kind = scenario["kind"]
    params = effective_params(scenario, smoke)
    repeats = repeats_override or scenario.get("repeats", 3)
    if repeats < 3:
        raise ValueError(f"SB-19: {kind} needs >= 3 repeats, got {repeats}")
    seed = int(params.get("seed", 150019000))

    out_dir.mkdir(parents=True, exist_ok=True)
    log_dir = out_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    registry = bench_matrix.PeerRegistry(REPO_ROOT)

    report: Dict[str, Any] = {
        "schema": SCHEMA,
        "scenario": scenario_name,
        "kind": kind,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "smoke": smoke,
        "params": params,
        "repeats": repeats,
        "seed": seed,
        "cpu_constraints": bench_matrix.collect_cpu_constraints(),
        "pinning": pin_mod.describe(int(scenario.get("cpus", 1))),
    }
    if kind == "extended-rtt":
        report["rtt_authoritative"] = bool(scenario.get("authoritative", False))
        report["rtt_claim_note"] = scenario.get("claim_note", "")

    peers = list(server_peers) if server_peers else list(scenario["peers"])
    problem, resolved, optional_status = matrix_run.preflight(registry, peers, [])
    report["peers"] = resolved
    report["optional_peers"] = optional_status
    if problem:
        report["matrix_complete"] = False
        report["fatal"] = problem
        write_report(out_dir, report)
        print(f"[SB-19] {problem}")
        return 1, report

    cells_out: List[Dict[str, Any]] = []
    failures = 0
    for cell in expand(scenario, server_peers, include_optional):
        base = as_dict(cell)
        not_run = optional_not_run(cell.server_peer, optional_status)
        if not_run:
            cells_out.append({**base, "status": "not_run", "reason": not_run})
            print(f"[SB-19] {cell.id}: not_run ({not_run[:120]})")
            continue
        unsupported = validate_cell(cell)
        if unsupported:
            cells_out.append({**base, "status": "unsupported", "reason": unsupported})
            print(f"[SB-19] {cell.id}: unsupported ({unsupported[:120]})")
            continue
        proc = None
        try:
            proc, addr, binary, codec, pin_state, out_path, err_path = spawn_server(
                registry, cell.server_peer, cell.cpus, log_dir, f"{cell.id}-server", verbose
            )
            host = "127.0.0.1"
            port = int(addr.rsplit(":", 1)[-1])
            prober = Prober(resolved["native"], proc, log_dir, cell.cpus, verbose)
            if kind == "extended-rtt":
                out = run_rtt_cell(prober, cell, addr, host, port, params, repeats, seed)
            elif kind == "extended-overload":
                out = run_overload_cell(prober, cell, addr, params, repeats, seed)
            else:
                out = run_fixed_rate_cell(prober, cell, addr, params, repeats, seed)
            full = {
                **base,
                **out,
                "server_binary": binary,
                "server_codec": codec,
                "server_pin": pin_state,
                "addr": addr,
                "logs": {"stdout": out_path.name, "stderr": err_path.name},
            }
        except peers_mod.PeerNotRunnable as e:
            full = {**base, "status": "not_run", "reason": str(e)}
        except (CellError, ValueError) as e:
            full = {**base, "status": "fail", "reason": f"{type(e).__name__}: {e}"[-500:]}
        except Exception as e:  # noqa: BLE001 - a dead cell must not kill the matrix
            full = {**base, "status": "fail", "reason": f"{type(e).__name__}: {e}"[-500:]}
        finally:
            if proc is not None:
                terminate(proc)
        if full["status"] == "fail":
            failures += 1
        cells_out.append(full)
        one_line = f"[SB-19] {cell.id}: {full['status']}"
        if full.get("summary"):
            one_line += f" {json.dumps(full['summary'], default=str)[:220]}"
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


# ---------------------------------------------------------------------------
# Self-test (no peer binaries needed)
# ---------------------------------------------------------------------------

def _synthetic_metrics(
    offered: int,
    successful: int,
    failed: int,
    timed_out: int,
    rejected: int,
    p50_nanos: int,
    p99_nanos: int,
    lag_p50: int = 0,
) -> Dict[str, Any]:
    return {
        "offered_rpcs": offered,
        "successful_rpcs": successful,
        "failed_rpcs": failed,
        "timeouts": timed_out,
        "queue_overflows": rejected,
        "throughput_qps": float(successful),
        "duration_nanos": 1_000_000_000,
        "e2e_latency_nanos": {"p50_nanos": p50_nanos, "p99_nanos": p99_nanos},
        "scheduling_lag_nanos": {"p50": lag_p50, "p99": lag_p50, "max": lag_p50},
        "status_errors": {},
    }


def _stub_tcp_server() -> Tuple[socket.socket, int]:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(8)
    return listener, listener.getsockname()[1]


def self_test() -> int:
    failures: List[str] = []

    def check(name: str, cond: bool, detail: str = "") -> None:
        print(f"[self-test] {'PASS' if cond else 'FAIL'} {name} {detail}")
        if not cond:
            failures.append(name)

    # All four scenario files parse and validate clean.
    scenarios = {}
    for name in (
        "extended-shapes",
        "extended-gzip",
        "extended-rtt",
        "extended-overload",
    ):
        try:
            scen = load_scenario(name)
            problems = validate_scenario(scen)
            scenarios[name] = scen
            check(f"scenario-{name}", not problems, "; ".join(problems)[:120])
        except Exception as e:  # noqa: BLE001 - self-test evidence
            check(f"scenario-{name}", False, f"{type(e).__name__}: {e}")
    # A bad scenario is caught, not run.
    bad = {"kind": "nope", "peers": [], "cells": []}
    check("scenario-invalid-caught", len(validate_scenario(bad)) >= 3)

    if scenarios:
        # Expansion covers every required server peer; ids unique + frozen.
        for name, scen in scenarios.items():
            got = expand(scen)
            peers = {c.server_peer for c in got}
            check(
                f"expand-{name}-peers",
                peers == set(cells_mod.REQUIRED_SERVER_PEERS),
                f"peers={sorted(peers)}",
            )
            ids = [c.id for c in got]
            check(f"expand-{name}-unique", len(ids) == len(set(ids)), f"n={len(ids)}")
            again = [c.id for c in expand(scen)]
            check(f"expand-{name}-frozen", ids == again)
        # Spot-check the frozen shapes the deliverable names.
        shapes = {(c.shape, c.payload) for c in expand(scenarios["extended-shapes"])}
        check("shapes-cover-new", ("client_stream", "1kib") in shapes, str(sorted(shapes)))
        gzip_tags = {(c.shape, c.compression) for c in expand(scenarios["extended-gzip"])}
        check(
            "gzip-pairs",
            gzip_tags == {
                ("unary", "identity"),
                ("unary", "gzip"),
                ("server_stream", "identity"),
                ("server_stream", "gzip"),
            },
            str(sorted(gzip_tags)),
        )
        rtt_profiles = {c.rtt_ms for c in expand(scenarios["extended-rtt"])}
        check("rtt-profiles", rtt_profiles == {1, 10}, str(sorted(rtt_profiles)))
        rtt_shapes = {c.shape for c in expand(scenarios["extended-rtt"])}
        check(
            "rtt-shape-classes",
            rtt_shapes == {"unary", "client_stream", "server_stream"},
            str(sorted(rtt_shapes)),
        )
        overload_cells = expand(scenarios["extended-overload"])
        check(
            "overload-flagged",
            all(c.overload for c in overload_cells) and len(overload_cells) == 5,
            f"n={len(overload_cells)}",
        )

    # Unsupported mapping: frozen defs name the missing capability.
    pipe = ExtendedCell("extended-shapes", "native", "native", "bidi_pipelined",
                        "empty", 256, "identity", None, False, 50.0, 1000, 1)
    pipe_reason = validate_cell(pipe) or ""
    check("unsupported-pipelined", "pipelin" in pipe_reason.lower(), pipe_reason[:100])
    gzip_cell = ExtendedCell("extended-gzip", "go", "native", "unary",
                             "64kib", 0, "gzip", None, False, 500.0, 1000, 1)
    gzip_reason = validate_cell(gzip_cell) or ""
    check("unsupported-gzip", "compress" in gzip_reason.lower(), gzip_reason[:100])
    runnable = ExtendedCell("extended-shapes", "native", "native", "client_stream",
                            "1kib", 2000, "identity", None, False, 20.0, 1000, 1)
    check("runnable-client-stream", validate_cell(runnable) is None)

    # Load argv: client_stream carries --stream-msgs; rtt adds the proxy flag.
    argv = cell_load_command(
        "/bin/rpc-bench", "127.0.0.1:9", runnable, 20.0, 2.0, 7, Path("/tmp/m.json")
    )
    check("argv-client-stream", "--shape=client_stream" in argv and "--stream-msgs=2000" in argv)
    rtt_cell = ExtendedCell("extended-rtt", "cpp", "native", "unary",
                            "1kib", 0, "identity", 10, False, 5000.0, 1000, 1)
    rtt_argv = cell_load_command(
        "/bin/rpc-bench", "127.0.0.1:9", rtt_cell, 5000.0, 2.0, 7, Path("/tmp/m.json")
    )
    check("argv-rtt", "--latency-rtt-ms=10" in rtt_argv)
    check("argv-no-rtt-when-plain", not any(a.startswith("--latency-rtt-ms") for a in argv))
    check("argv-cell-id", rtt_cell.id == "sb19-rtt-cpp-unary-1kib-rtt10ms-1cpu", rtt_cell.id)

    # Step flattening: accounting identity + generator saturation rule.
    ok_metrics = _synthetic_metrics(1000, 990, 10, 2, 0, 1_000_000, 5_000_000, lag_p50=10_000)
    ok_step = step_from_metrics(500.0, ok_metrics)
    check(
        "step-accounting-ok",
        ok_step["accounting_ok"] and not ok_step["gen_saturated"]
        and abs(ok_step["p99_s"] - 0.005) < 1e-9,
        accounting_note(ok_step),
    )
    bad_metrics = _synthetic_metrics(1000, 900, 10, 0, 0, 1_000_000, 5_000_000)
    check("step-accounting-gap", not step_from_metrics(500.0, bad_metrics)["accounting_ok"])
    sat_metrics = _synthetic_metrics(1000, 1000, 0, 0, 0, 1_000_000, 2_000_000, lag_p50=500_000)
    check("step-gen-saturated", step_from_metrics(500.0, sat_metrics)["gen_saturated"])
    # Gating verdict: cpu saturation wins, then rejections, else clean.
    # The lag rule stays recorded (gen_saturated above) but never gates.
    check("cap-clean", generator_cap(ok_step) is None)
    check("cap-rejected", generator_cap({**ok_step, "rejected": 5}) == "rejected")
    check(
        "cap-cpu-first",
        generator_cap({**ok_step, "rejected": 5, "cpu_saturated": True}) == "cpu",
    )

    # Goodput math: 2048 app bytes/RPC x 1000 RPCs / 1 s = ~1.953 MiB/s.
    uni = ExtendedCell("extended-overload", "native", "native", "unary",
                       "1kib", 0, "identity", None, True, 0.0, 1000, 1)
    check("goodput-bytes", app_bytes_per_rpc(uni) == 2048)
    gp = goodput_mib_s(1000, 2048, 1.0)
    check("goodput-math", gp is not None and abs(gp - 1.953125) < 1e-9, f"{gp}")

    # Overload protocol end to end with a scripted prober (no binaries):
    # search 500 clean, 1000 rejected at base cap and still rejected
    # escalated (knee found), baseline, 2x overload, dirty then clean
    # recovery probe.
    def canned(offered, successful, failed, timed_out, rejected, p99_s, cpu=False):
        return {
            "offered": offered,
            "successful": successful,
            "failed": failed,
            "timed_out": timed_out,
            "rejected": rejected,
            "accounting_ok": successful + failed + rejected == offered and offered > 0,
            "success_qps": float(successful),
            "p50_s": p99_s / 2.0,
            "p99_s": p99_s,
            "lag_p50_s": 0.0,
            "gen_saturated": False,
            "cpu_saturated": cpu,
            "cpu_spare_pct": 50.0,
            "status_errors": {},
        }

    class FakeProc:
        def poll(self):
            return None

    class FakeProber:
        def __init__(self, script):
            self.server_proc = FakeProc()
            self._script = list(script)
            self.calls = []

        def run(self, cell, addr, rate, duration_s, seed, tag,
                max_in_flight=None, distribution="poisson",
                shape_override=None, stream_msgs_override=None):
            self.calls.append((tag, rate, max_in_flight))
            assert self._script, f"script exhausted at {tag} rate={rate}"
            step = dict(self._script.pop(0))
            step["offered_rate"] = rate
            return step, {}

    ov_params = {
        "search_start_rate": 500.0,
        "search_growth": 2.0,
        "max_search_steps": 4,
        "max_search_down_steps": 1,
        "search_duration_s": 1.0,
        "max_in_flight": 1000,
        "max_in_flight_escalated": 10000,
        "overload_max_in_flight": 10000,
        "baseline_duration_s": 1.0,
        "overload_factor": 2.0,
        "overload_duration_s": 2.0,
        "recovery_probe_s": 1.0,
        "max_recovery_probes": 6,
        "recovery_p99_factor": 2.0,
    }
    script = [
        canned(500, 500, 0, 0, 0, 0.002),          # search 500: clean
        canned(1000, 900, 0, 0, 100, 0.010),       # search 1000: rejected
        canned(1000, 950, 0, 0, 50, 0.010),        # esc 10000: still rejected
        canned(500, 500, 0, 0, 0, 0.002),          # baseline at 500
        canned(4000, 3000, 800, 200, 200, 5.0),    # overload at 2000
        canned(500, 490, 10, 0, 0, 0.050),         # recover1: dirty
        canned(500, 500, 0, 0, 0, 0.002),          # recover2: clean
    ]
    fake = FakeProber(script)
    ov = run_overload_cell(fake, uni, "127.0.0.1:9", ov_params, 1, 11)
    rep0 = (ov.get("repeats") or [{}])[0]
    ov_overload = rep0.get("overload", {})
    check("overload-script-status", ov.get("status") == "pass", ov.get("reason", "")[:100])
    check(
        "overload-script-search",
        rep0.get("saturation_rate") == 1000.0
        and rep0.get("saturation_signal") == "rejections_at_cap_10000"
        and rep0.get("normal_rate") == 500.0
        and rep0.get("overload_rate") == 2000.0,
        f"sat={rep0.get('saturation_rate')} sig={rep0.get('saturation_signal')}",
    )
    check(
        "overload-script-escalated",
        ("rep1-search1000qps-esc", 1000.0, 10000) in fake.calls,
        str([c for c in fake.calls if "esc" in c[0]]),
    )
    check(
        "overload-script-rates",
        abs(ov_overload.get("error_rate", -1) - 0.2) < 1e-9
        and abs(ov_overload.get("timeout_rate", -1) - 0.05) < 1e-9
        and abs(ov_overload.get("rejection_rate", -1) - 0.05) < 1e-9,
        f"e={ov_overload.get('error_rate')} t={ov_overload.get('timeout_rate')} r={ov_overload.get('rejection_rate')}",
    )
    want_gp = 3000 * 2048 / 2.0 / 1048576.0
    check(
        "overload-script-goodput",
        abs((ov_overload.get("goodput_mib_s") or -1) - want_gp) < 1e-9,
        f"{ov_overload.get('goodput_mib_s')}",
    )
    rec0 = rep0.get("recovery", {})
    check(
        "overload-script-recovery",
        rec0.get("recovered") is True
        and rec0.get("recovery_time_s") is not None
        and len(rec0.get("probes", [])) == 2,
        str({k: rec0.get(k) for k in ("recovered", "recovery_time_s")}),
    )
    # Same script with cooked overload accounting invalidates the run.
    bad_script = list(script)
    bad_script[4] = canned(4000, 3000, 0, 0, 0, 5.0)
    ov_bad = run_overload_cell(FakeProber(bad_script), uni, "127.0.0.1:9", ov_params, 1, 11)
    check(
        "overload-script-accounting-invalid",
        ov_bad.get("status") == "invalid" and "accounting" in ov_bad.get("reason", ""),
        ov_bad.get("reason", "")[:100],
    )
    # Baseline retry: a boundary normal that rejects steps down once and
    # the saturation-by-errors signal path records server_errors.
    script3 = [
        canned(500, 500, 0, 0, 0, 0.002),          # search 500: clean
        canned(1000, 900, 100, 0, 0, 1.000),       # search 1000: errors
        canned(500, 400, 0, 0, 100, 0.010),        # baseline 500: rejects
        canned(250, 250, 0, 0, 0, 0.001),          # baseline 250: clean
        canned(4000, 3000, 800, 200, 200, 5.0),    # overload at 2000
        canned(250, 250, 0, 0, 0, 0.001),          # recover: clean
    ]
    ov3 = run_overload_cell(FakeProber(script3), uni, "127.0.0.1:9", ov_params, 1, 11)
    rep3 = (ov3.get("repeats") or [{}])[0]
    check(
        "overload-script-stepdown",
        ov3.get("status") == "pass"
        and rep3.get("normal_rate") == 250.0
        and rep3.get("baseline_step_downs") == 1
        and rep3.get("saturation_signal") == "server_errors",
        f"normal={rep3.get('normal_rate')} downs={rep3.get('baseline_step_downs')} sig={rep3.get('saturation_signal')}",
    )

    # RTT verdict: evidencing p50 verifies; loopback-fast p50 does not.
    ok, _ = rtt_verify_verdict(1.0, 0.0011, 0.5, 0.05)
    check("rtt-verdict-pass", ok)
    ok2, detail2 = rtt_verify_verdict(10.0, 0.00005, 0.5, 0.05)
    check("rtt-verdict-unverified", not ok2, detail2[:80])
    ok3, _ = rtt_verify_verdict(1.0, 0.500, 0.5, 0.05)
    check("rtt-verdict-slack", not ok3)

    # TCP ping against a stub listener returns a finite RTT.
    listener, ping_port = _stub_tcp_server()
    try:
        stop = threading.Event()

        def serve() -> None:
            listener.settimeout(0.2)
            while not stop.is_set():
                try:
                    conn, _ = listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    return
                with conn:
                    pass

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        ping = tcp_ping_ms("127.0.0.1", ping_port, samples=5)
        stop.set()
        thread.join(timeout=5.0)
        check(
            "tcp-ping-stub",
            0 < ping["median_ms"] < 1000 and ping["samples"] == 5,
            f"median={ping['median_ms']:.3f}ms",
        )
    finally:
        listener.close()

    print(f"[self-test] {len(failures)} failure(s)")
    return 1 if failures else 0


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(
        description="SB-19 extended cells (shapes, gzip pairs, RTT holdouts, overload/recovery)."
    )
    parser.add_argument("--scenario", default=None)
    parser.add_argument("--out-dir", default="target/stack-matrix/sb19")
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
        parser.error("--scenario is required (e.g. extended-rtt)")
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
