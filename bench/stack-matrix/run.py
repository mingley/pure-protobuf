"""SB-11 stage runner: pin endpoints, step offered load, rate against the SLO.

Server cells hold the generator fixed (rpc-bench `load`, open-loop
Poisson) and vary the server peer; client cells hold the reference
server fixed and vary the client peer at a matched offered rate.
Reference soak clients (go/cpp) cannot run the open-loop workload, so
their client cells delegate to the existing soak path and are labeled
`interop-soak`, never SLO-rated.
"""

from __future__ import annotations

import importlib.util
import json
import math
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

STACK_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(STACK_DIR))
import cells as cells_mod
import frozen as frozen_mod
import peers as peers_mod
import peertls
import pin as pin_mod
import slo as slo_mod
from peers import fairness as fairness_mod

if set(peers_mod.OPTIONAL_PEER_IDS) != set(cells_mod.OPTIONAL_PEERS):
    raise RuntimeError(
        "SB-18 registry drift: peers.OPTIONAL_PEER_IDS != cells.OPTIONAL_PEERS"
    )

REPO_ROOT = STACK_DIR.parents[1]
MATRIX_SCRIPT = REPO_ROOT / "scripts" / "rpc-bench-matrix.py"


def load_matrix():
    spec = importlib.util.spec_from_file_location("rpc_bench_matrix", MATRIX_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


bench_matrix = load_matrix()

TONIC_PBRS = "tonic-pbrs"
TONIC_PROST = "tonic-prost"

STAGE_PARAMS = {
    # Wiring proof: short runs, narrow search, loose ceiling.
    "smoke": {
        "warmup_s": 1.0,
        "duration_s": 2.0,
        "seed": 0x5EED_2026,
        "slo_p99_s": 0.050,
        "start_rate": 500.0,
        "max_rate": 8000.0,
        "growth": 2.0,
        "max_steps": 6,
        "matched_rate": 1000.0,
    },
    # Contract-shaped defaults; dedicated pinned hosts still required
    # for claim-grade numbers (§8), which this harness cannot provide.
    "primary": {
        "warmup_s": 15.0,
        "duration_s": 60.0,
        "seed": 0x5EED_2026,
        "slo_p99_s": 0.010,
        "start_rate": 1000.0,
        "max_rate": 200000.0,
        "growth": 2.0,
        "max_steps": 12,
        "matched_rate": 20000.0,
    },
}

# Server CPU fraction (of verified effective capacity) above which a client cell has no
# verified server headroom and is invalid, not rated.
HEADROOM_MAX_SERVER_FRACTION = 0.80


def server_command(
    registry,
    peer: str,
    host: str,
    port: int,
    timeout_secs: float,
    tls_spec: Optional[peertls.ServerTlsSpec],
    compression: str = "identity",
) -> Tuple[List[str], str, str]:
    """Build the server argv for `peer`. Returns (cmd, binary, codec)."""
    if peer in ("native", "tonic-pbrs", "tonic-prost", "tonic"):
        binary = str(registry.resolve_native_binary(None))
        cmd = [
            binary,
            "load-server",
            f"--transport={'native' if peer == 'native' else 'tonic'}",
            f"--codec={'prost' if peer in ('tonic', TONIC_PROST) else 'pbrs'}",
            f"--compression={compression}",
            f"--host={host}",
            f"--port={port}",
            f"--timeout-secs={int(timeout_secs)}",
        ]
        if tls_spec is not None:
            cmd.extend(tls_spec.server_args)
        return cmd, binary, "prost" if peer in ("tonic", TONIC_PROST) else "pbrs"
    if peer == "go":
        go_server = registry.resolve_go_peer("server")
        if not go_server:
            raise FileNotFoundError("Go server peer (go-interop-server) not found or could not be built.")
        cmd = [str(go_server), f"-port={port}"]
        cmd.extend(tls_spec.server_args if tls_spec else ["-use_tls=false"])
        return cmd, str(go_server), "protobuf-go"
    if peer == "cpp":
        cpp_server = registry.resolve_cpp_peer("server")
        if not cpp_server:
            raise FileNotFoundError(
                "C++ server peer (interop_server) not found. Set GRPC_INTEROP_CPP_SERVER "
                "or build via scripts/grpc-interop-cpp.sh."
            )
        cmd = [str(cpp_server), f"--port={port}"]
        cmd.extend(tls_spec.server_args if tls_spec else ["--use_tls=false"])
        return cmd, str(cpp_server), "protobuf-cpp"
    if peers_mod.is_optional(peer):
        # Raises peers_mod.PeerNotRunnable when the peer cannot run; the
        # caller maps that to a not_run cell, never a failure.
        return peers_mod.server_command(
            peer, host, port, timeout_secs, tls_spec, registry.repo_root
        )
    raise ValueError(f"unsupported server peer: {peer}")


def load_command(
    native_bin: str,
    client_peer: str,
    addr: str,
    cell: cells_mod.Cell,
    rate: float,
    duration_s: float,
    seed: int,
    out_path: Path,
    tls_spec: Optional[peertls.ServerTlsSpec],
) -> List[str]:
    """Build the open-loop `load` argv for one probe."""
    req_bytes, resp_bytes = cells_mod.PAYLOADS[cell.payload]
    transport = "tonic" if client_peer in ("tonic", TONIC_PBRS, TONIC_PROST) else "native"
    cmd = [
        native_bin,
        "load",
        f"--server_addr={addr}",
        f"--transport={transport}",
        f"--shape={cell.shape}",
        f"--compression={cell.compression}",
        f"--req-bytes={req_bytes}",
        f"--resp-bytes={resp_bytes}",
        "--distribution=poisson",
        f"--rate={rate:g}",
        f"--seed={seed}",
        f"--duration-secs={duration_s:g}",
        f"--output={out_path}",
    ]
    if client_peer == TONIC_PROST:
        cmd.append("--codec=prost")
    if cell.shape in cells_mod.STREAM_MSGS:
        cmd.append(f"--stream-msgs={cells_mod.STREAM_MSGS[cell.shape]}")
    if tls_spec is not None:
        cmd.append(f"--tls-ca={tls_spec.ca_file}")
        cmd.append(f"--tls-server-name={tls_spec.server_name}")
    return cmd


def parse_metrics(out_path: Path) -> Dict[str, Any]:
    try:
        with open(out_path, "r", encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        raise ValueError(f"load wrote no valid metrics at {out_path}: {e}")


def validate_load_accounting(metrics: Dict[str, Any]) -> Dict[str, Any]:
    """Reconcile terminal load counters and both measured latency histograms."""
    required = ("offered_rpcs", "dispatched_rpcs", "successful_rpcs", "failed_rpcs",
                "timeouts", "queue_overflows", "duration_nanos")
    for key in required:
        if type(metrics.get(key)) is not int or metrics[key] < 0:
            raise ValueError(f"missing/invalid independent load counter {key}")
    offered, dispatched = metrics["offered_rpcs"], metrics["dispatched_rpcs"]
    if offered != dispatched + metrics["queue_overflows"]:
        raise ValueError("load offered != dispatched + rejected")
    if dispatched != metrics["successful_rpcs"] + metrics["failed_rpcs"]:
        raise ValueError("load dispatched != successful + failed (including terminal unfinished)")
    if metrics["timeouts"] > metrics["failed_rpcs"] or metrics["duration_nanos"] <= 0:
        raise ValueError("load timeout count or measurement duration is inconsistent")
    errors = metrics.get("status_errors")
    if not isinstance(errors, dict) or any(type(count) is not int or count < 0 for count in errors.values()):
        raise ValueError("load omitted independent failure/unfinished status counts")
    unfinished = errors.get("UNFINISHED", 0)
    if unfinished > metrics["failed_rpcs"]:
        raise ValueError("load unfinished exceeds failed calls")
    for key in ("service_latency_nanos", "e2e_latency_nanos"):
        histogram = (metrics.get(key) or {}).get("histogram", {})
        buckets = histogram.get("buckets")
        if (histogram.get("total_count") != dispatched or not isinstance(buckets, list)
                or any(not isinstance(bucket, dict) or type(bucket.get("count")) is not int
                       or bucket["count"] < 0 for bucket in buckets)
                or sum(bucket["count"] for bucket in buckets) != dispatched):
            raise ValueError(f"load {key} does not reconcile with terminal dispatched calls")
    return {"verified": True, "window_kind": "arrival_window_with_terminal_drain",
            "offered": offered, "dispatched": dispatched, "completed": dispatched - unfinished,
            "rejected": metrics["queue_overflows"], "timed_out": metrics["timeouts"],
            "unfinished": unfinished, "latency_samples": dispatched,
            "unfinished_latency_policy": "terminal drain samples; any unfinished invalidates the SLO"}


def to_step(rate: float, metrics: Dict[str, Any], gen_saturated: bool) -> slo_mod.StepResult:
    validate_load_accounting(metrics)
    e2e = metrics.get("e2e_latency_nanos") or {}
    qps = metrics.get("throughput_qps")
    if qps is None:
        dur_s = (metrics.get("duration_nanos") or 0) / 1e9
        qps = (metrics.get("successful_rpcs") or 0) / dur_s if dur_s > 0 else 0.0
    return slo_mod.StepResult(
        offered_rate=rate,
        offered_calls=int(metrics.get("offered_rpcs") or 0),
        successful_calls=int(metrics.get("successful_rpcs") or 0),
        failed_calls=int(metrics.get("failed_rpcs") or 0),
        timed_out_calls=int(metrics.get("timeouts") or 0),
        rejected_calls=int(metrics.get("queue_overflows") or 0),
        success_qps=float(qps),
        p99_s=(e2e.get("p99_nanos") or 0) / 1e9 if e2e else float("inf"),
        p50_s=(e2e.get("p50_nanos") or 0) / 1e9 if e2e else float("inf"),
        gen_saturated=gen_saturated,
    )


class CellRunner:
    """Spawns one server per cell and probes it at stepped offered load."""

    def __init__(
        self,
        registry,
        native_bin: str,
        log_dir: Path,
        params: Dict[str, Any],
        verbose: bool = False,
    ):
        self.registry = registry
        self.native_bin = native_bin
        self.log_dir = log_dir
        self.params = params
        self.verbose = verbose
        self.log_dir.mkdir(parents=True, exist_ok=True)

    def _spawn(
        self, cmd: List[str], cpus: int, offset: int, name: str, cell_id: str
    ) -> Tuple[subprocess.Popen, Dict[str, Any], Path, Path]:
        pinned, pin_state = pin_mod.wrap(cmd, cpus, offset)
        stdout_path = self.log_dir / f"{cell_id}-{name}.stdout"
        stderr_path = self.log_dir / f"{cell_id}-{name}.stderr"
        if self.verbose:
            print(f"[{cell_id}] spawn {name}: {' '.join(pinned)}")
        proc = subprocess.Popen(
            pinned,
            stdout=open(stdout_path, "w", encoding="utf-8"),
            stderr=open(stderr_path, "w", encoding="utf-8"),
            text=True,
        )
        bench_matrix._ACTIVE_PROCESSES.add(proc)
        pin_state = dict(pin_state)
        pin_state["actual"] = pin_mod.actual_affinity(proc.pid)
        return proc, pin_state, stdout_path, stderr_path

    def _terminate(self, proc: subprocess.Popen) -> None:
        bench_matrix._ACTIVE_PROCESSES.discard(proc)
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3.0)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5.0)

    def probe(
        self,
        cell: cells_mod.Cell,
        server_proc: subprocess.Popen,
        addr: str,
        tls_spec: Optional[peertls.ServerTlsSpec],
        rate: float,
        tag: str,
    ) -> Tuple[slo_mod.StepResult, Dict[str, Any]]:
        """One measured probe at `rate`; returns (step, resources)."""
        metrics_path = self.log_dir / f"{cell.id}-{tag}.json"
        cmd = load_command(
            self.native_bin,
            cell.client_peer,
            addr,
            cell,
            rate,
            self.params["duration_s"],
            self.params["seed"],
            metrics_path,
            tls_spec,
        )
        monitor = bench_matrix.ProcessResourceMonitor(
            server_pid=server_proc.pid, poll_interval_s=0.05
        )
        client_proc, client_pin, _, _ = self._spawn(
            cmd, cell.cpus, cell.cpus, f"load-{tag}", cell.id
        )
        monitor.set_client_pid(client_proc.pid)
        monitor.start()
        wall_start = time.monotonic()
        timeout = self.params["duration_s"] + self.params["warmup_s"] + 60.0
        try:
            client_proc.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            client_proc.kill()
            client_proc.communicate()
            raise TimeoutError(f"load probe at {rate:g} qps exceeded {timeout:.0f}s")
        finally:
            bench_matrix._ACTIVE_PROCESSES.discard(client_proc)
            client_res, server_res, sat_check = monitor.stop()
        wall_s = max(0.001, time.monotonic() - wall_start)
        if client_proc.returncode != 0:
            raise RuntimeError(
                f"load probe at {rate:g} qps exited {client_proc.returncode}; "
                f"see {cell.id}-load-{tag}.stderr"
            )
        if not client_res.get("supported") or not server_res.get("supported"):
            raise RuntimeError(
                f"load probe at {rate:g} qps has unsupported resource sampling "
                f"(client={client_res.get('method')}, server={server_res.get('method')})"
            )
        metrics = parse_metrics(metrics_path)
        lag = (metrics.get("scheduling_lag_nanos") or {}).get("p50")
        p50 = (metrics.get("e2e_latency_nanos") or {}).get("p50_nanos")
        lag_verified = lag is not None and p50 is not None and p50 > 0
        lag_saturated = not lag_verified or lag > 0.10 * p50
        sat_check["schedule_lag_verified"] = lag_verified
        sat_check["schedule_lag_saturated"] = lag_saturated
        # This is a contract validity check even when CPU sampling has spare capacity.
        step = to_step(rate, metrics, bool(sat_check.get("saturated")) or lag_saturated)
        resources = {
            "accounting": validate_load_accounting(metrics),
            "client": client_res,
            "server": server_res,
            "saturation": sat_check,
            "client_pin": client_pin,
            "wall_s": round(wall_s, 3),
            "metrics_file": str(metrics_path.name),
        }
        return step, resources

    def warmup(self, cell, addr, tls_spec, rate):
        path = self.log_dir / f"{cell.id}-warmup.json"
        cmd = load_command(self.native_bin, cell.client_peer, addr, cell, rate,
                           self.params["warmup_s"], self.params["seed"], path, tls_spec)
        proc, _, _, _ = self._spawn(cmd, cell.cpus, cell.cpus, "warmup", cell.id)
        try:
            proc.communicate(timeout=self.params["warmup_s"] + 60.0)
            if proc.returncode != 0:
                raise RuntimeError(f"warmup exited {proc.returncode}")
            metrics = parse_metrics(path)
            if metrics.get("successful_rpcs", 0) <= 0 or metrics.get("failed_rpcs", 0):
                raise RuntimeError("warmup failed or completed no calls")
            return {"accounting": validate_load_accounting(metrics),
                    "metrics_file": path.name, "duration_s": self.params["warmup_s"],
                    "successful_rpcs": metrics["successful_rpcs"]}
        finally:
            self._terminate(proc)

    def run_server_cell(
        self, cell: cells_mod.Cell, tls_spec: Optional[peertls.ServerTlsSpec]
    ) -> Dict[str, Any]:
        """SLO search for a server cell; one server, stepped load, warmup first."""
        host = "127.0.0.1"
        port = bench_matrix.find_free_port()
        budget = (
            self.params["warmup_s"]
            + self.params["duration_s"] * self.params["max_steps"]
            + 120.0
        )
        cmd, binary, codec = server_command(
            self.registry, cell.server_peer, host, port, budget, tls_spec, cell.compression
        )
        server_proc, server_pin, _, _ = self._spawn(cmd, cell.cpus, 0, "server", cell.id)
        report: Dict[str, Any] = {
            **cells_mod.as_dict(cell),
            "status": "fail",
            "server_binary": binary,
            "server_codec": codec,
            "server_pin": server_pin,
        }
        try:
            _, addr = bench_matrix.wait_for_server_readiness(
                server_proc, host=host, port=port, timeout_secs=30.0
            )
            report["addr"] = addr
            # Warmup at the search start rate; discarded.
            report["warmup"] = self.warmup(cell, addr, tls_spec, self.params["start_rate"])

            per_step_resources: List[Dict[str, Any]] = []

            def probe_fn(rate: float) -> slo_mod.StepResult:
                tag = f"step{len(per_step_resources)}-{rate:g}qps".replace(".", "p")
                step, resources = self.probe(cell, server_proc, addr, tls_spec, rate, tag)
                per_step_resources.append(resources)
                return step

            result = slo_mod.find_sustained_qps(
                probe_fn,
                start_rate=self.params["start_rate"],
                max_rate=self.params["max_rate"],
                growth=self.params["growth"],
                slo_p99_s=self.params["slo_p99_s"],
                max_steps=self.params["max_steps"],
            )
            report["slo"] = result.as_dict()
            report["probe_resources"] = per_step_resources
            if result.sustained_step < 0:
                report["status"] = "invalid"
                report["reason"] = f"no SLO-valid step: {result.ceiling_reason}"
                return report
            best = per_step_resources[result.sustained_step]
            step = result.steps[result.sustained_step]
            report["sustained"] = summarize(step, best, cell.cpus)
            report["status"] = "pass"
            return report
        finally:
            self._terminate(server_proc)

    def run_client_cell(
        self, cell: cells_mod.Cell, tls_spec: Optional[peertls.ServerTlsSpec]
    ) -> Dict[str, Any]:
        """Matched-rate probe for a client cell with server-headroom check."""
        if cell.workload() == "interop-soak":
            return self.run_soak_client_cell(cell)
        host = "127.0.0.1"
        port = bench_matrix.find_free_port()
        budget = self.params["warmup_s"] + self.params["duration_s"] + 120.0
        # The fixed reference server matches the cell's TLS mode so the
        # client under test exercises its TLS path on TLS cells.
        cmd, binary, codec = server_command(
            self.registry, cell.server_peer, host, port, budget, tls_spec, cell.compression
        )
        server_proc, server_pin, _, _ = self._spawn(cmd, cell.cpus, 0, "server", cell.id)
        report: Dict[str, Any] = {
            **cells_mod.as_dict(cell),
            "status": "fail",
            "server_binary": binary,
            "server_codec": codec,
            "server_pin": server_pin,
        }
        try:
            _, addr = bench_matrix.wait_for_server_readiness(
                server_proc, host=host, port=port, timeout_secs=30.0
            )
            report["addr"] = addr
            rate = self.params["matched_rate"]
            report["warmup"] = self.warmup(cell, addr, tls_spec, rate)
            step, resources = self.probe(cell, server_proc, addr, tls_spec, rate, "matched")
            report["probe_resources"] = [resources]
            step = slo_mod.check_step(step, self.params["slo_p99_s"])
            report["probe"] = step.as_dict()
            if not step.valid:
                report["status"] = "invalid"
                report["reason"] = f"matched probe invalid: {step.invalid_reason}"
                return report
            headroom = check_headroom(
                resources["server"],
                resources["wall_s"],
                cell.cpus,
                bool(server_pin.get("supported")),
            )
            report["headroom"] = headroom
            if not headroom["ok"]:
                report["status"] = "invalid"
                report["reason"] = f"no verified server headroom: {headroom['reason']}"
                return report
            report["sustained"] = summarize(step, resources, cell.cpus)
            report["status"] = "pass"
            return report
        finally:
            self._terminate(server_proc)

    def run_soak_client_cell(self, cell: cells_mod.Cell) -> Dict[str, Any]:
        """Delegate soak-driven reference clients to the matrix runner."""
        ok, data, summary = bench_matrix.run_single_benchmark(
            self.registry,
            cell.server_peer,
            cell.client_peer,
            "unary",
            "127.0.0.1",
            0,
            True,
            120.0,
            self.verbose,
        )
        report: Dict[str, Any] = {**cells_mod.as_dict(cell)}
        report["soak_summary"] = summary[-2000:]
        if not ok or not data:
            report["status"] = "fail"
            report["reason"] = (summary or "soak cell failed")[-500:]
            return report
        runs = data.get("runs", [])
        total_ok = sum(r.get("metrics", {}).get("successful_rpcs", 0) for r in runs)
        client_cpu = sum(
            (r.get("metrics", {}).get("client_resources", {}) or {}).get("user_cpu_seconds", 0.0)
            + (r.get("metrics", {}).get("client_resources", {}) or {}).get("system_cpu_seconds", 0.0)
            for r in runs
        )
        report["status"] = "pass"
        report["sustained"] = {
            "successful_rpcs": total_ok,
            "client_cpu_seconds": round(client_cpu, 6),
            "client_cpu_per_rpc": round(client_cpu / total_ok, 9) if total_ok else None,
            "note": "soak workload: closed-loop interop loops, not offered-load rated",
        }
        return report


def summarize(
    step: slo_mod.StepResult, resources: Dict[str, Any], cpus: int
) -> Dict[str, Any]:
    server = resources["server"]
    client = resources["client"]
    n = step.successful_calls or 0
    server_cpu = (server.get("user_cpu_seconds") or 0.0) + (
        server.get("system_cpu_seconds") or 0.0
    )
    client_cpu = (client.get("user_cpu_seconds") or 0.0) + (
        client.get("system_cpu_seconds") or 0.0
    )
    return {
        "success_qps": round(step.success_qps, 1),
        "p50_s": step.p50_s,
        "p99_s": step.p99_s,
        "server_cpu_per_rpc": round(server_cpu / n, 9) if n else None,
        "client_cpu_per_rpc": round(client_cpu / n, 9) if n else None,
        "server_peak_rss_mib": server.get("peak_rss_mib"),
        "client_peak_rss_mib": client.get("peak_rss_mib"),
        "server_cpu_seconds": round(server_cpu, 6),
        "client_cpu_seconds": round(client_cpu, 6),
        "cpus": cpus,
    }


def check_headroom(
    server_res: Dict[str, Any], wall_s: float, cpus: int, pinned: bool
) -> Dict[str, Any]:
    cpu_s = (server_res.get("user_cpu_seconds") or 0.0) + (
        server_res.get("system_cpu_seconds") or 0.0
    )
    avg_pct = (cpu_s / wall_s) * 100.0 if wall_s > 0 else float("inf")
    capacity = server_res.get("cpu_capacity", {})
    effective = capacity.get("effective_cpu_capacity")
    verified = bool(capacity.get("verified") is True and isinstance(effective, (int, float))
                    and not isinstance(effective, bool) and math.isfinite(effective)
                    and effective > 0 and capacity.get("affinity_cpu_count") == cpus)
    # A configured pin alone cannot reveal a lower fractional cgroup quota.
    budget = 100.0 * min(cpus, effective) if verified else None
    delta_verified = server_res.get("cpu_delta_verified") is True
    ok = bool(pinned and verified and delta_verified and server_res.get("supported") and wall_s > 0
              and avg_pct < HEADROOM_MAX_SERVER_FRACTION * budget)
    return {
        "ok": ok,
        "server_avg_cpu_pct": round(avg_pct, 1),
        "server_cpu_seconds": round(cpu_s, 6),
        "wall_s": wall_s,
        "budget_cpu_pct": budget,
        "effective_cpu_capacity": effective if verified else None,
        "capacity_verified": verified,
        "cpu_delta_verified": delta_verified,
        "pinned": pinned,
        "reason": ""
        if ok
        else ("resource sampling, actual affinity or effective CPU capacity unverified"
              if not pinned or not server_res.get("supported") or not verified or not delta_verified
              else f"server at {avg_pct:.1f}% of {budget:.0f}% budget (>{HEADROOM_MAX_SERVER_FRACTION:.0%})"),
    }


def cell_not_run_reason(
    cell: cells_mod.Cell, optional_status: Dict[str, Any]
) -> Optional[str]:
    """Reason an optional-peered cell cannot run, or None when runnable."""
    peer = cell.peer_under_test
    if not peers_mod.is_optional(peer):
        return None
    if cell.role == "client":
        return (
            f"optional peer {peer} has no open-loop client driver; "
            "client cells need native/tonic/go/cpp drivers"
        )
    entry = optional_status.get(peer)
    if entry is None:
        return f"optional peer {peer} was not preflighted"
    if entry.get("status") != "ready":
        return str(entry.get("reason") or f"optional peer {peer} is not runnable")
    return None


def preflight(
    registry, server_peers: List[str], client_peers: List[str]
) -> Tuple[str, Dict[str, Any], Dict[str, Any]]:
    """Resolve peer binaries; fail fast naming a missing *required* peer.

    Returns (problem, resolved, optional_status). Optional peers are
    always preflighted for the report but never fail the stage.
    """
    resolved: Dict[str, Any] = {}
    missing: List[str] = []
    optional_status: Dict[str, Any] = {}
    try:
        native = str(registry.resolve_native_binary(None))
        resolved["native"] = native
    except Exception as e:
        for peer in peers_mod.OPTIONAL_PEER_IDS:
            optional_status[peer] = peers_mod.preflight_peer(peer, registry.repo_root)
        return f"native rpc-bench binary unresolvable: {e}", resolved, optional_status
    for peer in sorted(set(server_peers) | set(client_peers)):
        if peer in ("native", "tonic-pbrs", "tonic-prost", "tonic"):
            resolved[peer] = native
        elif peer == "go":
            server = registry.resolve_go_peer("server")
            client = registry.resolve_go_peer("client")
            if server and client:
                resolved["go-server"] = str(server)
                resolved["go-client"] = str(client)
            else:
                missing.append("go (go-interop-server/client)")
        elif peer == "cpp":
            server = registry.resolve_cpp_peer("server")
            client = registry.resolve_cpp_peer("client")
            if server and client:
                resolved["cpp-server"] = str(server)
                resolved["cpp-client"] = str(client)
            else:
                missing.append(
                    "cpp (interop_server/client; set GRPC_INTEROP_CPP_SERVER/CLIENT "
                    "or build via scripts/grpc-interop-cpp.sh)"
                )
        elif peers_mod.is_optional(peer):
            entry = peers_mod.preflight_peer(peer, registry.repo_root)
            optional_status[peer] = entry
            if entry["status"] == "ready":
                resolved[peer] = entry["binary"]
        else:
            missing.append(peer)
    # Every optional peer lands in the report (pinned + ready/not_run)
    # whether or not the stage requested it.
    for peer in peers_mod.OPTIONAL_PEER_IDS:
        if peer not in optional_status:
            optional_status[peer] = peers_mod.preflight_peer(peer, registry.repo_root)
    if missing:
        return f"missing required peers: {'; '.join(missing)}", resolved, optional_status
    return "", resolved, optional_status


def run_stage(
    stage: str,
    out_dir: Path,
    repo_root: Path = REPO_ROOT,
    server_peers: Optional[List[str]] = None,
    client_peers: Optional[List[str]] = None,
    verbose: bool = False,
    include_optional: bool = False,
    scenario_path: Optional[Path] = None,
    cell_filter: Optional[List[str]] = None,
    scenario_smoke: bool = False,
    claim: bool = False,
) -> Tuple[int, Dict[str, Any]]:
    """Run one stage; return (exit_code, report)."""
    scenario = frozen_mod.load(scenario_path, STAGE_PARAMS["primary"], cell_filter) if scenario_path else None
    params = dict(scenario["params"] if scenario else STAGE_PARAMS[stage])
    if scenario_smoke:
        if not scenario:
            raise ValueError("scenario-smoke requires a frozen scenario")
        params.update(STAGE_PARAMS["smoke"])
        params["seed"] = scenario["params"]["seed"]
    out_dir.mkdir(parents=True, exist_ok=True)
    log_dir = out_dir / "logs"
    registry = bench_matrix.PeerRegistry(repo_root)
    server_peers = server_peers if server_peers is not None else cells_mod.REQUIRED_SERVER_PEERS
    client_peers = client_peers if client_peers is not None else cells_mod.REQUIRED_CLIENT_PEERS
    plan = scenario["plan"] if scenario else [("rep1", cell) for cell in cells_mod.expand(
        stage=stage, server_peers=server_peers, client_peers=client_peers,
        include_optional=include_optional)]
    if scenario:
        server_peers = sorted({cell.server_peer for _, cell in plan})
        client_peers = sorted({cell.client_peer for _, cell in plan})

    report: Dict[str, Any] = {
        "schema": "stack-matrix/1",
        "stage": stage,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "params": params,
        "cpu_constraints": bench_matrix.collect_cpu_constraints(),
        "pinning": pin_mod.describe(1),
        "fairness_spec": fairness_mod.SB01_SPEC,
        "qualification": "diagnostic-loopback",
        "claim_eligible": False,
        "scenario": {k: v for k, v in scenario.items() if k != "plan"} if scenario else None,
        "scenario_smoke": scenario_smoke,
        "execution_order": [{"repeat": rep, "id": cell.id} for rep, cell in plan],
    }

    if claim:
        report["matrix_complete"] = False
        report["fatal"] = (
            "claim preflight blocked: this runner uses loopback; effective peer settings, "
            "dedicated host topology, generator/reference headroom and aligned endpoint "
            "measurement windows are not independently verified. Use diagnostic mode "
            "to collect wiring evidence; no defaults or scripted settings certify equality."
        )
        report["claim_blockers"] = fairness_mod.claim_preflight([cell for _, cell in plan])
        write_report(out_dir, report)
        print(f"[STACK-MATRIX] {report['fatal']}")
        return 1, report

    problem, resolved, optional_status = preflight(registry, server_peers, client_peers)
    report["peers"] = resolved
    report["optional_peers"] = optional_status
    if problem:
        report["matrix_complete"] = False
        report["fatal"] = problem
        write_report(out_dir, report)
        print(f"[STACK-MATRIX] {problem}")
        return 1, report

    def fairness_for(cell: cells_mod.Cell, tls_active: bool) -> Dict[str, Any]:
        return fairness_mod.for_cell(
            cell.server_peer,
            cell.client_peer,
            tls_active,
            is_optional=peers_mod.is_optional,
        )

    runner = CellRunner(registry, resolved["native"], log_dir, params, verbose)
    tls_cache: Dict[str, Optional[peertls.ServerTlsSpec]] = {}
    cell_reports: List[Dict[str, Any]] = []
    failures = 0
    for repeat, cell in plan:
        runner.log_dir = log_dir / repeat
        runner.log_dir.mkdir(parents=True, exist_ok=True)
        not_run = cell_not_run_reason(cell, optional_status)
        if not_run:
            cell_reports.append(
                {
                    **cells_mod.as_dict(cell),
                    "status": "not_run",
                    "repeat": repeat,
                    "reason": not_run,
                    "fairness": fairness_for(cell, False),
                }
            )
            print(f"[STACK-MATRIX] {cell.id}: not_run ({not_run[:120]})")
            continue
        unsupported = cells_mod.validate(cell)
        if unsupported:
            cell_reports.append(
                {
                    **cells_mod.as_dict(cell),
                    "status": "unsupported",
                    "repeat": repeat,
                    "reason": unsupported,
                    "fairness": fairness_for(cell, False),
                }
            )
            continue
        tls_spec = None
        if cell.tls:
            if cell.server_peer not in tls_cache:
                if peers_mod.is_optional(cell.server_peer):
                    tls_cache[cell.server_peer] = peers_mod.tls_spec(
                        cell.server_peer, repo_root
                    )
                else:
                    tls_cache[cell.server_peer] = peertls.server_spec(
                        cell.server_peer, repo_root
                    )
            tls_spec = tls_cache[cell.server_peer]
            if tls_spec is None:
                cell_reports.append(
                    {
                        **cells_mod.as_dict(cell),
                        "status": "unsupported",
                        "repeat": repeat,
                        "reason": f"TLS material unresolvable for {cell.server_peer}",
                        "fairness": fairness_for(cell, False),
                    }
                )
                continue
        try:
            if cell.role == "server":
                cell_report = runner.run_server_cell(cell, tls_spec)
            else:
                cell_report = runner.run_client_cell(cell, tls_spec)
        except peers_mod.PeerNotRunnable as e:
            cell_report = {
                **cells_mod.as_dict(cell),
                "status": "not_run",
                "reason": str(e),
                "fairness": fairness_for(cell, False),
            }
        except Exception as e:  # noqa: BLE001 - a dead cell must not kill the matrix
            cell_report = {
                **cells_mod.as_dict(cell),
                "status": "fail",
                "reason": f"{type(e).__name__}: {e}"[-500:],
            }
        cell_report.setdefault(
            "fairness", fairness_for(cell, tls_spec is not None)
        )
        cell_report["repeat"] = repeat
        if tls_spec is not None:
            cell_report["tls"] = peertls.spec_dict(tls_spec)
        if cell_report["status"] == "fail":
            failures += 1
        cell_reports.append(cell_report)
        print(
            f"[STACK-MATRIX] {cell.id}: {cell_report['status']}"
            + (
                f" sustained={cell_report['sustained']['success_qps']}qps"
                if cell_report.get("sustained", {}).get("success_qps") is not None
                else ""
            )
            + (f" ({cell_report['reason']})" if cell_report.get("reason") else "")
        )

    report["cells"] = cell_reports
    required_reports = [c for c in cell_reports if not cells_mod.is_optional(c["server_peer"])
                        and not cells_mod.is_optional(c["client_peer"])]
    report["selection_complete"] = bool(required_reports) and all(c["status"] == "pass" for c in required_reports)
    report["matrix_complete"] = report["selection_complete"] and not (scenario and scenario["filtered"])
    if failures:
        report["fatal"] = f"{failures} cell(s) failed"
    write_report(out_dir, report)
    return (0 if failures == 0 else 1), report


def write_report(out_dir: Path, report: Dict[str, Any]) -> Path:
    path = out_dir / "report.json"
    with open(path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
        f.write("\n")
    return path


def main(argv: List[str]) -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Cross-stack client/server matrix harness (SB-11).")
    parser.add_argument("--stage", default="smoke", choices=list(STAGE_PARAMS))
    parser.add_argument("--out-dir", default="target/stack-matrix")
    parser.add_argument("--server-peers", default=None)
    parser.add_argument("--client-peers", default=None)
    parser.add_argument("--verbose", "-v", action="store_true")
    parser.add_argument("--scenario", type=Path, help="frozen scenario JSON path")
    parser.add_argument("--cells", help="comma-separated frozen cell ids, preserving repeat order")
    parser.add_argument("--scenario-smoke", action="store_true", help="short diagnostic params, retaining every frozen repeat")
    parser.add_argument("--claim", action="store_true", help="fail closed unless all claim preconditions are verified")
    parser.add_argument(
        "--include-optional",
        action="store_true",
        help="expand SB-18 optional peers into the matrix (not_run with reason when unrunnable)",
    )
    parser.add_argument(
        "--verify-fairness",
        default=None,
        metavar="REPORT",
        help="verify SB-01 fairness from a report's metadata and exit (no stage run)",
    )
    args = parser.parse_args(argv)
    if args.verify_fairness:
        with open(args.verify_fairness, "r", encoding="utf-8") as f:
            loaded = json.load(f)
        ok, findings = fairness_mod.verify_report(loaded, claim=args.claim)
        for item in findings:
            print(f"[FAIRNESS:{item['severity']}] {item['check']}: {item['detail']}")
        print(f"[FAIRNESS] {'PASS' if ok else 'FAIL'} ({len(findings)} findings)")
        return 0 if ok else 1
    if (args.cells or args.scenario_smoke) and not args.scenario:
        parser.error("--cells and --scenario-smoke require --scenario")
    if args.scenario and (args.server_peers or args.client_peers or args.include_optional):
        parser.error("a frozen scenario selects peers; use --cells to narrow it")
    try:
        code, _ = run_stage(
            args.stage,
            Path(args.out_dir),
            server_peers=args.server_peers.split(",") if args.server_peers else None,
            client_peers=args.client_peers.split(",") if args.client_peers else None,
            verbose=args.verbose,
            include_optional=args.include_optional,
            scenario_path=args.scenario,
            cell_filter=args.cells.split(",") if args.cells else None,
            scenario_smoke=args.scenario_smoke,
            claim=args.claim,
        )
    except (ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
