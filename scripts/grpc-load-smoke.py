#!/usr/bin/env python3
"""Linux separate-process RPC diagnostics; never a performance qualification."""

import argparse
import hashlib
import importlib.util
import itertools
import json
import math
import os
from pathlib import Path
import platform
import random
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILES = [("native", "pbrs"), ("native", "prost"), ("tonic", "pbrs"), ("tonic", "prost")]
PAIRS = [(*client, *server) for client, server in itertools.product(PROFILES, repeat=2)]
SHAPES = ["unary", "server_stream", "client_stream", "bidi", "bidi_pipelined"]


def verified_build(path, binary):
    spec = importlib.util.spec_from_file_location("rpc_bench_build", ROOT / "scripts/build-rpc-bench.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.validate_record(path, binary)


def validate_metrics(metrics):
    names = ["offered_rpcs", "dispatched_rpcs", "completed_rpcs", "successful_rpcs",
             "failed_rpcs", "timeouts", "queue_overflows", "unstarted_rpcs", "unfinished_rpcs"]
    if any(type(metrics.get(name)) is not int or metrics[name] < 0 for name in names):
        raise ValueError("missing or invalid terminal RPC accounting")
    offered = metrics["offered_rpcs"]
    if offered == 0 or any(metrics[name] != offered for name in names[1:4]):
        raise ValueError("not every offered RPC completed successfully")
    if any(metrics[name] != 0 for name in names[4:]) or metrics.get("status_errors") != {}:
        raise ValueError("failed, rejected or incomplete RPCs")


def counters(pid):
    raw = (Path("/proc") / str(pid) / "stat").read_text()
    fields = raw[raw.rfind(")") + 2:].split()
    status = (Path("/proc") / str(pid) / "status").read_text()
    rss = next((int(line.split()[1]) * 1024 for line in status.splitlines()
                if line.startswith("VmRSS:")), None)
    return {"monotonic_ns": time.monotonic_ns(), "pid": pid,
            "user_ticks": int(fields[11]), "system_ticks": int(fields[12]),
            "rss_bytes": rss, "state": fields[0]}


def cpu_seconds(before, after):
    delta = sum(after[key] - before[key] for key in ("user_ticks", "system_ticks"))
    if delta < 0:
        raise ValueError("process CPU counters moved backwards")
    return delta / os.sysconf("SC_CLK_TCK")


def allocation_record(path):
    records = [json.loads(line[len("ALLOCATIONS "):]) for line in path.read_text().splitlines()
               if line.startswith("ALLOCATIONS ")]
    if len(records) != 1:
        raise ValueError("exactly one allocator snapshot is required per endpoint")
    record = records[0]
    if (record.get("scope") != "process_since_main" or record.get("includes_reallocations") is not True
            or any(type(record.get(key)) is not int or record[key] < 0 for key in ("allocations", "requested_bytes"))):
        raise ValueError("invalid allocator snapshot")
    return record


def callgrind_instructions(path):
    events, totals = None, []
    for line in path.read_text().splitlines():
        if line.startswith("events:"):
            events = line.split()[1:]
        elif line.startswith("summary:"):
            totals.append([int(value) for value in line.split()[1:]])
    if events is None or "Ir" not in events or len(totals) != 1 or len(totals[0]) != len(events):
        raise ValueError("missing or ambiguous Callgrind instruction summary")
    instructions = totals[0][events.index("Ir")]
    if instructions <= 0:
        raise ValueError("nonpositive Callgrind instructions")
    return instructions


def context_switch_record(path):
    records = [json.loads(line[len("CONTEXT_SWITCHES "):])
               for line in path.read_text().splitlines()
               if line.startswith("CONTEXT_SWITCHES ")]
    if len(records) != 1:
        raise ValueError("exactly one context-switch snapshot is required per endpoint")
    record = records[0]
    if not isinstance(record, dict):
        raise ValueError("invalid process context-switch snapshot")
    counts = record.get("counts")
    if (record.get("scope") != "process_lifetime"
            or record.get("method") != "linux_getrusage_self"
            or record.get("includes_exited_threads") is not True
            or not isinstance(counts, dict)
            or any(type(counts.get(name)) is not int or not 0 <= counts[name] < 2 ** 64
                   for name in ("voluntary", "involuntary"))):
        raise ValueError("invalid or unsupported process context-switch snapshot")
    return {**record, "total": counts["voluntary"] + counts["involuntary"]}


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)


def run_cell(args, cell, directory):
    directory.mkdir()
    client_transport, client_codec, server_transport, server_codec = cell["pair"]
    server_cmd = [str(args.binary), "load-server", "--port=0",
                  f"--transport={server_transport}", f"--codec={server_codec}",
                  f"--compression={cell['compression']}"]
    client_cmd = [str(args.binary), "load", f"--transport={client_transport}",
                  f"--codec={client_codec}", f"--shape={cell['shape']}",
                  f"--req-bytes={cell['payload_bytes']}", f"--resp-bytes={cell['payload_bytes']}",
                  "--stream-msgs=4", f"--connections={cell.get('connections', 1)}",
                  f"--max-in-flight={cell.get('in_flight', 1)}",
                  f"--duration-secs={args.duration}", f"--compression={cell['compression']}",
                  f"--output={directory / 'metrics.json'}"]
    if cell["tls"]:
        data = ROOT / "pbrs-grpc/tests/tls_data"
        server_cmd += [f"--tls-cert={data / 'server.crt'}", f"--tls-key={data / 'server.key'}"]
        client_cmd += [f"--tls-ca={data / 'ca.crt'}", "--tls-server-name=localhost"]
    if getattr(args, "rpc_count", None) is not None:
        client_cmd.append(f"--rpc-count={args.rpc_count}")
    if getattr(args, "callgrind", None) is not None:
        prefix = [str(args.callgrind), "--tool=callgrind", "--quiet"]
        server_cmd = [*prefix, f"--callgrind-out-file={directory / 'server.callgrind'}", *server_cmd]
        client_cmd = [*prefix, f"--callgrind-out-file={directory / 'client.callgrind'}", *client_cmd]
    env = {**os.environ, "TOKIO_WORKER_THREADS": "2"}
    server = client = None
    report = {"cell": cell, "commands": {"server": server_cmd, "client": client_cmd},
              "environment": {"TOKIO_WORKER_THREADS": "2"}, "passed": False}
    try:
        with (directory / "server.stdout").open("w") as server_out, \
                (directory / "server.stderr").open("w") as server_err, \
                (directory / "client.stdout").open("w") as client_out, \
                (directory / "client.stderr").open("w") as client_err:
            server = subprocess.Popen(server_cmd, stdout=server_out, stderr=server_err, env=env)
            # Files retain startup output without risking a full pipe.
            deadline = time.monotonic() + 10
            address = None
            while address is None and time.monotonic() < deadline:
                if server.poll() is not None:
                    raise RuntimeError("server exited before READY")
                for line in (directory / "server.stdout").read_text().splitlines():
                    if line.startswith("READY "):
                        address = next(word.split("=", 1)[1] for word in line.split()
                                       if word.startswith("addr="))
                time.sleep(0.01)
            if address is None:
                raise TimeoutError("server readiness timeout")
            client_cmd.append(f"--server_addr={address}")
            server_before = counters(server.pid)
            client = subprocess.Popen(client_cmd, stdout=client_out, stderr=client_err, env=env)
            samples = []
            deadline = time.monotonic() + args.duration + 15
            while True:
                # WNOWAIT keeps the terminal child CPU counters readable before reaping.
                exited = os.waitid(os.P_PID, client.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                samples.append({"client": counters(client.pid), "server": counters(server.pid)})
                if exited is not None:
                    break
                if time.monotonic() > deadline:
                    raise TimeoutError("load child exceeded bounded wall time")
                time.sleep(0.01)
            server_after, client_after = samples[-1]["server"], samples[-1]["client"]
            report["client_exit_code"] = client.wait(timeout=2)
            report["server_exit_before_teardown"] = server.poll()
            report["resource_samples"] = samples
            report["server_cpu_before"] = server_before
            report["client_lifetime_cpu_seconds"] = (
                client_after["user_ticks"] + client_after["system_ticks"]
            ) / os.sysconf("SC_CLK_TCK")
            report["server_cpu_during_client_seconds"] = cpu_seconds(server_before, server_after)
            metrics = json.loads((directory / "metrics.json").read_text())
            report["metrics"] = metrics
            validate_metrics(metrics)
            if report["client_exit_code"] != 0 or report["server_exit_before_teardown"] is not None:
                raise RuntimeError("benchmark endpoint failed")
            if getattr(args, "rpc_count", None) is not None and metrics["successful_rpcs"] != args.rpc_count:
                raise ValueError("fixed RPC count was not completed")
            if getattr(args, "allocation_counts", False):
                os.kill(server.pid, signal.SIGUSR1)
                deadline = time.monotonic() + 5
                marker = "CONTEXT_SWITCHES " if getattr(args, "context_switches", False) else "ALLOCATIONS "
                while marker not in (directory / "server.stdout").read_text():
                    if server.poll() is not None or time.monotonic() > deadline:
                        raise TimeoutError("server allocator snapshot missing")
                    time.sleep(0.01)
                report["allocation_totals"] = {side: allocation_record(directory / f"{side}.stdout")
                                               for side in ("client", "server")}
            if getattr(args, "context_switches", False):
                report["context_switch_totals"] = {
                    side: context_switch_record(directory / f"{side}.stdout")
                    for side in ("client", "server")}
            report["passed"] = True
    except (OSError, ValueError, RuntimeError, TimeoutError, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
    finally:
        stop(client)
        stop(server)
        report["server_teardown_exit_code"] = server.returncode if server else None
        if getattr(args, "callgrind", None) is not None and report.get("passed"):
            try:
                report["instruction_totals"] = {side: callgrind_instructions(directory / f"{side}.callgrind")
                                                 for side in ("client", "server")}
            except (OSError, ValueError) as error:
                report.update(passed=False, error=str(error))
        (directory / "run.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--duration", type=float, default=1.0)
    parser.add_argument("--payloads", default="0,1024,65536,1048576")
    parser.add_argument("--seed", type=int, default=11011)
    parser.add_argument("--load-levels", default="1:1", help="connections:total-in-flight pairs")
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("--rpc-count", type=int)
    parser.add_argument("--allocation-counts", action="store_true", help="requires allocation-counts Cargo feature")
    parser.add_argument("--context-switches", action="store_true",
                        help="capture all-thread Linux process totals; requires --allocation-counts")
    parser.add_argument("--callgrind", type=Path, help="Valgrind executable; instruments both endpoints")
    parser.add_argument("--build-record", type=Path, help="source-pinned build.json from build-rpc-bench.py")
    args = parser.parse_args()
    if platform.system() != "Linux" or not math.isfinite(args.duration) or not 0.01 <= args.duration <= 60:
        parser.error("Linux and a finite 0.01..60 second diagnostic duration are required")
    if args.context_switches and not args.allocation_counts:
        parser.error("context-switch snapshots require --allocation-counts")
    try:
        payloads = [int(value) for value in args.payloads.split(",")]
    except ValueError:
        parser.error("payloads must be comma-separated integer byte sizes")
    if not payloads or len(set(payloads)) != len(payloads) or any(value < 0 or value > 1048576 for value in payloads):
        parser.error("payloads must be unique body byte sizes in 0..1048576")
    try:
        levels = [tuple(int(value) for value in level.split(":")) for level in args.load_levels.split(",")]
    except ValueError:
        parser.error("load levels must be connections:in-flight integer pairs")
    if (not levels or len(set(levels)) != len(levels)
            or any(len(level) != 2 or not 1 <= level[0] <= 64 or not 1 <= level[1] <= 1024 for level in levels)):
        parser.error("load levels require 1..64 connections and 1..1024 in-flight RPCs")
    if not 1 <= args.repeats <= 10 or (args.rpc_count is not None and not 1 <= args.rpc_count <= 1000000):
        parser.error("repeats must be 1..10 and RPC count 1..1000000")
    if args.callgrind is not None:
        args.callgrind = args.callgrind.resolve(strict=True)
    args.binary = args.binary.resolve(strict=True)
    build = verified_build(args.build_record.resolve(strict=True), args.binary) if args.build_record else None
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    cells = [{"pair": list(pair), "shape": shape, "payload_bytes": size,
              "tls": tls, "compression": compression, "connections": level[0], "in_flight": level[1],
              "repeat": repeat}
             for pair, shape, size, tls, compression, level, repeat in itertools.product(
                 PAIRS, SHAPES, payloads, [False, True], ["identity", "gzip"], levels, range(args.repeats))]
    random.Random(args.seed).shuffle(cells)
    report = {"schema": "pbrs.load-smoke.v2", "binary_sha256": digest,
              "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
              "source_verified": build is not None, "build_record": build,
              "host": platform.uname()._asdict(), "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
              "cpu_affinity": sorted(os.sched_getaffinity(0)), "seed": args.seed,
              "duration_per_cell_seconds": args.duration, "cells": cells,
              "rpc_count": args.rpc_count, "allocation_counts": args.allocation_counts,
              "context_switches": args.context_switches,
              "configured_policy": {"gzip_compression_level": 6, "tls_version": "1.3",
                                    "tls_cipher": "TLS_AES_128_GCM_SHA256", "tls_alpn": "h2"},
              "callgrind": str(args.callgrind) if args.callgrind else None,
              "qualification": {"qualified": False,
                  "limits": ["prebuilt binary digest is pinned; source-to-binary mapping must be checked against build records",
                             "shared host, no verified CPU headroom or quota proof",
                             "repeats and concurrency are explicit; no claim-grade statistics",
                             "client CPU includes startup and handshake; server includes connection setup and cleanup",
                             "RSS is sampled; optional endpoint counters include setup; context switches are OS scheduling events, not task wakeups; wakes/syscalls are unmeasured",
                             "zero-filled payload bodies; not a read-all adoption corpus",
                             "saturation, cold/idle lifecycles, read-all corpora and production soak are not run"]},
              "runs": []}
    (args.output / "source.patch").write_bytes(subprocess.check_output(["git", "diff", "HEAD"], cwd=ROOT))
    (args.output / "manifest.json").write_text(json.dumps(report, indent=2) + "\n")
    for index, cell in enumerate(cells):
        run = run_cell(args, cell, args.output / f"cell-{index:03d}")
        report["runs"].append({"path": f"cell-{index:03d}/run.json", "passed": run["passed"]})
        print(f"{index + 1}/{len(cells)}: {'passed' if run['passed'] else run.get('error')}", flush=True)
    report["binary_unchanged"] = digest == hashlib.sha256(args.binary.read_bytes()).hexdigest()
    report["source_unchanged"] = True
    if args.build_record:
        try:
            verified_build(args.build_record.resolve(strict=True), args.binary)
        except (OSError, ValueError) as error:
            report["source_unchanged"] = False
            report["source_error"] = str(error)
    report["passed"] = (report["binary_unchanged"] and report["source_unchanged"]
                        and all(run["passed"] for run in report["runs"]))
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
