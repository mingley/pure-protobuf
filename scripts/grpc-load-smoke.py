#!/usr/bin/env python3
"""Linux separate-process RPC diagnostics; never a performance qualification."""

import argparse
import hashlib
import itertools
import json
import math
import os
from pathlib import Path
import platform
import random
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
PAIRS = [("native", "pbrs", "native", "pbrs"),
         ("native", "pbrs", "tonic", "prost"),
         ("tonic", "prost", "native", "pbrs"),
         ("tonic", "prost", "tonic", "prost")]
SHAPES = ["unary", "server_stream", "client_stream", "bidi"]


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
                  "--stream-msgs=4", "--connections=1", "--max-in-flight=1",
                  f"--duration-secs={args.duration}", f"--compression={cell['compression']}",
                  f"--output={directory / 'metrics.json'}"]
    if cell["tls"]:
        data = ROOT / "pbrs-grpc/tests/tls_data"
        server_cmd += [f"--tls-cert={data / 'server.crt'}", f"--tls-key={data / 'server.key'}"]
        client_cmd += [f"--tls-ca={data / 'ca.crt'}", "--tls-server-name=localhost"]
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
            report["passed"] = True
    except (OSError, ValueError, RuntimeError, TimeoutError, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
    finally:
        stop(client)
        stop(server)
        report["server_teardown_exit_code"] = server.returncode if server else None
        (directory / "run.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--duration", type=float, default=1.0)
    parser.add_argument("--payloads", default="0,1024,65536,1048576")
    parser.add_argument("--seed", type=int, default=11011)
    args = parser.parse_args()
    if platform.system() != "Linux" or not math.isfinite(args.duration) or not 0.01 <= args.duration <= 60:
        parser.error("Linux and a finite 0.01..60 second diagnostic duration are required")
    try:
        payloads = [int(value) for value in args.payloads.split(",")]
    except ValueError:
        parser.error("payloads must be comma-separated integer byte sizes")
    if not payloads or len(set(payloads)) != len(payloads) or any(value < 0 or value > 1048576 for value in payloads):
        parser.error("payloads must be unique body byte sizes in 0..1048576")
    args.binary = args.binary.resolve(strict=True)
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    cells = [{"pair": list(pair), "shape": shape, "payload_bytes": size,
              "tls": tls, "compression": compression}
             for pair, shape, size, tls, compression in itertools.product(
                 PAIRS, SHAPES, payloads, [False, True], ["identity", "gzip"])]
    random.Random(args.seed).shuffle(cells)
    report = {"schema": "pbrs.load-smoke.v1", "binary_sha256": digest,
              "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
              "source_verified": False,
              "host": platform.uname()._asdict(), "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
              "cpu_affinity": sorted(os.sched_getaffinity(0)), "seed": args.seed,
              "duration_per_cell_seconds": args.duration, "cells": cells,
              "qualification": {"qualified": False,
                  "limits": ["prebuilt binary digest is pinned; source-to-binary mapping must be checked against build records",
                             "shared host, no verified CPU headroom or quota proof",
                             "one repeat, one connection, one concurrent RPC",
                             "client CPU includes startup and handshake; server includes connection setup and cleanup",
                             "RSS is sampled; instructions, allocations, wakes and syscall costs are not measured",
                             "zero-filled payload bodies; not a read-all adoption corpus",
                             "native prost, many connections, sustained load and production soak are not run"]},
              "runs": []}
    (args.output / "source.patch").write_bytes(subprocess.check_output(["git", "diff", "HEAD"], cwd=ROOT))
    (args.output / "manifest.json").write_text(json.dumps(report, indent=2) + "\n")
    for index, cell in enumerate(cells):
        run = run_cell(args, cell, args.output / f"cell-{index:03d}")
        report["runs"].append({"path": f"cell-{index:03d}/run.json", "passed": run["passed"]})
        print(f"{index + 1}/{len(cells)}: {'passed' if run['passed'] else run.get('error')}", flush=True)
    report["binary_unchanged"] = digest == hashlib.sha256(args.binary.read_bytes()).hexdigest()
    report["passed"] = report["binary_unchanged"] and all(run["passed"] for run in report["runs"])
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
