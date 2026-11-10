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
import signal
import subprocess
import time
import types

ROOT = Path(__file__).resolve().parents[1]
PROFILES = [("native", "pbrs"), ("native", "prost"), ("tonic", "pbrs"), ("tonic", "prost")]
PAIRS = [(*client, *server) for client, server in itertools.product(PROFILES, repeat=2)]
SHAPES = ["unary", "server_stream", "client_stream", "bidi", "bidi_pipelined"]


def file_matches(path, expected_sha256):
    try:
        return hashlib.sha256(path.read_bytes()).hexdigest() == expected_sha256
    except OSError:
        return False


def git_patch(checkout):
    """Capture the tracked worktree's net diff from HEAD, including binary files."""
    return subprocess.check_output(
        ["git", "diff", "--binary", "--no-color", "--no-ext-diff", "--no-textconv", "HEAD", "--"],
        cwd=checkout,
    )


def validate_verified_output_location(output, source, cell_count, *, callgrind, strace):
    """Check known capture files will not dirty a verified source checkout."""
    try:
        relative = output.relative_to(source.resolve())
    except ValueError:
        return
    files = ["source.patch", "harness.patch", "harness.py", "build-verifier.py",
             "manifest.json", "report.json"]
    cell_files = ["server.stdout", "server.stderr", "client.stdout", "client.stderr",
                  "metrics.json", "run.json"]
    if callgrind:
        cell_files += ["server.callgrind", "client.callgrind"]
    if strace:
        cell_files += ["server.strace", "client.strace"]
    paths = [os.fsencode((relative / name).as_posix()) for name in files]
    paths += [os.fsencode((relative / f"cell-{index:03d}" / name).as_posix())
              for index in range(cell_count) for name in cell_files]
    result = subprocess.run(
        ["git", "check-ignore", "--no-index", "-z", "--stdin"],
        cwd=source,
        input=b"\0".join(paths) + b"\0",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode not in (0, 1):
        raise ValueError("could not check whether capture outputs are ignored by Git")
    ignored = set(result.stdout.split(b"\0"))
    if not set(paths).issubset(ignored):
        raise ValueError("verified capture outputs inside the measured source must be Git-ignored")


def verified_build(path, binary, source_checkout=None, *, require_allocation_counts=True,
                   verifier_sha256=None):
    source = source_checkout or ROOT
    helper_path = ROOT / "scripts/build-rpc-bench.py"
    helper_bytes = helper_path.read_bytes()
    helper_hash = hashlib.sha256(helper_bytes).hexdigest()
    if verifier_sha256 is None:
        verifier_sha256 = helper_hash
    if helper_hash != verifier_sha256:
        raise ValueError("driver build verifier changed before validation")

    # Execute the exact hashed bytes, not an import cache. The source checkout
    # may carry an older validator which does not inspect Cargo features.
    driver = types.ModuleType("rpc_bench_build_driver")
    driver.__file__ = str(helper_path)
    exec(compile(helper_bytes, str(helper_path), "exec"), driver.__dict__)
    # A historical source helper may predate index-visibility validation.
    ensure_index_visible = getattr(driver, "ensure_visible_index", None)
    if not callable(ensure_index_visible):
        raise ValueError("driver build verifier does not support index visibility checks")
    ensure_index_visible(source)
    source_helper_path = source / "scripts/build-rpc-bench.py"
    source_helper_bytes = source_helper_path.read_bytes()
    source_helper_hash = hashlib.sha256(source_helper_bytes).hexdigest()
    module = types.ModuleType("rpc_bench_build")
    module.__file__ = str(source_helper_path)
    exec(compile(source_helper_bytes, str(source_helper_path), "exec"), module.__dict__)
    record = module.validate_record(path, binary)
    instrumented = driver.validate_feature_record(record, path, source)
    if not file_matches(helper_path, verifier_sha256):
        raise ValueError("driver build verifier changed during validation")
    if not file_matches(source_helper_path, source_helper_hash):
        raise ValueError("source build verifier changed during validation")
    if require_allocation_counts and not instrumented:
        raise ValueError("allocator snapshots require a build with allocation-counts")
    return record


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


def strace_syscalls(path):
    rows = {}
    total = None
    for line in path.read_text().splitlines():
        fields = line.split()
        if not fields or fields[0] == "%" or set(fields[0]) == {"-"}:
            continue
        if len(fields) not in (5, 6):
            raise ValueError("invalid strace summary row")
        percent, seconds = map(float, fields[:2])
        micros, calls = map(int, fields[2:4])
        errors = int(fields[4]) if len(fields) == 6 else 0
        name = fields[-1]
        if (not math.isfinite(percent) or not 0 <= percent <= 100
                or not math.isfinite(seconds) or seconds < 0 or micros < 0
                or not 0 <= errors <= calls < 2 ** 64):
            raise ValueError("invalid strace summary counters")
        record = {"calls": calls, "errors": errors}
        if name == "total":
            if total is not None:
                raise ValueError("duplicate strace total")
            total = record
        elif name in rows:
            raise ValueError("duplicate strace syscall row")
        else:
            rows[name] = record
    if (not rows or total is None or total["calls"] <= 0
            or any(sum(row[key] for row in rows.values()) != total[key] for key in ("calls", "errors"))):
        raise ValueError("missing or inconsistent strace summary")
    return {"scope": "traced_endpoint_lifetime", "method": "linux_strace_f_count",
            "includes_exited_threads": True, **total, "syscall_counts": rows}


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
        data = (getattr(args, "source_checkout", None) or ROOT) / "pbrs-grpc/tests/tls_data"
        server_cmd += [f"--tls-cert={data / 'server.crt'}", f"--tls-key={data / 'server.key'}"]
        client_cmd += [f"--tls-ca={data / 'ca.crt'}", "--tls-server-name=localhost"]
    if getattr(args, "rpc_count", None) is not None:
        client_cmd.append(f"--rpc-count={args.rpc_count}")
    if getattr(args, "callgrind", None) is not None:
        prefix = [str(args.callgrind), "--tool=callgrind", "--quiet"]
        server_cmd = [*prefix, f"--callgrind-out-file={directory / 'server.callgrind'}", *server_cmd]
        client_cmd = [*prefix, f"--callgrind-out-file={directory / 'client.callgrind'}", *client_cmd]
    if getattr(args, "strace", None) is not None:
        # -D keeps Popen.pid as the actual endpoint, so counters and SIGUSR1
        # still address it. -f includes worker threads even after they exit.
        prefix = [str(args.strace), "-D", "-f", "-c", "-o"]
        server_cmd = [*prefix, str(directory / "server.strace"), *server_cmd]
        client_cmd = [*prefix, str(directory / "client.strace"), *client_cmd]
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
        if getattr(args, "strace", None) is not None and report.get("passed"):
            deadline = time.monotonic() + 3
            while True:
                try:
                    report["syscall_totals"] = {side: strace_syscalls(directory / f"{side}.strace")
                                                for side in ("client", "server")}
                    break
                except (OSError, ValueError) as error:
                    if time.monotonic() >= deadline:
                        report.update(passed=False, error=str(error))
                        break
                    time.sleep(0.01)
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
    parser.add_argument("--strace", type=Path, help="strace executable; counts syscalls in both endpoints")
    parser.add_argument("--build-record", type=Path, help="source-pinned build.json from build-rpc-bench.py")
    parser.add_argument("--source-checkout", type=Path,
                        help="clean frozen benchmark checkout for --build-record; driver may be newer")
    args = parser.parse_args()
    if platform.system() != "Linux" or not math.isfinite(args.duration) or not 0.01 <= args.duration <= 60:
        parser.error("Linux and a finite 0.01..60 second diagnostic duration are required")
    if args.context_switches and not args.allocation_counts:
        parser.error("context-switch snapshots require --allocation-counts")
    if args.strace is not None and args.callgrind is not None:
        parser.error("strace and Callgrind require separate captures")
    if args.source_checkout is not None and args.build_record is None:
        parser.error("a frozen source checkout requires its build record")
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
    if args.strace is not None:
        args.strace = args.strace.resolve(strict=True)
    if args.source_checkout is not None:
        args.source_checkout = args.source_checkout.resolve(strict=True)
    args.binary = args.binary.resolve(strict=True)
    helper_path = ROOT / "scripts/build-rpc-bench.py"
    helper_bytes = helper_path.read_bytes() if args.build_record else None
    helper_hash = hashlib.sha256(helper_bytes).hexdigest() if helper_bytes is not None else None
    try:
        build = (verified_build(args.build_record.resolve(strict=True), args.binary, args.source_checkout,
                                require_allocation_counts=args.allocation_counts, verifier_sha256=helper_hash)
                 if args.build_record else None)
    except ValueError as error:
        parser.error(str(error))
    cells = [{"pair": list(pair), "shape": shape, "payload_bytes": size,
              "tls": tls, "compression": compression, "connections": level[0], "in_flight": level[1],
              "repeat": repeat}
             for pair, shape, size, tls, compression, level, repeat in itertools.product(
                 PAIRS, SHAPES, payloads, [False, True], ["identity", "gzip"], levels, range(args.repeats))]
    random.Random(args.seed).shuffle(cells)
    args.output = args.output.resolve()
    source_checkout = args.source_checkout or ROOT
    if args.build_record:
        try:
            validate_verified_output_location(
                args.output, source_checkout, len(cells), callgrind=args.callgrind, strace=args.strace)
        except ValueError as error:
            parser.error(str(error))
    args.output.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    harness_bytes = Path(__file__).read_bytes()
    source_patch = git_patch(source_checkout)
    harness_patch = source_patch if source_checkout.resolve() == ROOT.resolve() else git_patch(ROOT)
    report = {"schema": "pbrs.load-smoke.v2", "binary_sha256": digest,
              "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.source_checkout or ROOT, text=True).strip(),
              "dirty": bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=args.source_checkout or ROOT)),
              "harness_script_sha256": hashlib.sha256(harness_bytes).hexdigest(),
              "harness_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=ROOT)),
              "source_verified": build is not None, "build_record": build,
              "source_patch_sha256": hashlib.sha256(source_patch).hexdigest(),
              "harness_patch_sha256": hashlib.sha256(harness_patch).hexdigest(),
              "patch_scope": "Git net tracked worktree diff from HEAD at capture start; subject to index flags and config; untracked files, index staging and dirty submodule contents omitted",
              "build_verifier_sha256": helper_hash,
              "build_verifier_file": "build-verifier.py" if helper_bytes is not None else None,
              "host": platform.uname()._asdict(), "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
              "cpu_affinity": sorted(os.sched_getaffinity(0)), "seed": args.seed,
              "duration_per_cell_seconds": args.duration, "cells": cells,
              "rpc_count": args.rpc_count, "allocation_counts": args.allocation_counts,
              "context_switches": args.context_switches,
              "configured_policy": {"gzip_compression_level": 6, "tls_version": "1.3",
                                    "tls_cipher": "TLS_AES_128_GCM_SHA256", "tls_alpn": "h2"},
              "callgrind": str(args.callgrind) if args.callgrind else None,
              "strace": str(args.strace) if args.strace else None,
              "strace_version": subprocess.check_output([str(args.strace), "--version"], text=True).splitlines()[0] if args.strace else None,
              "qualification": {"qualified": False,
                  "limits": ["prebuilt binary digest is pinned; source-to-binary mapping must be checked against build records",
                             "without a build record, source_unchanged does not validate source drift",
                             "Git status and diff follow index flags and configuration and do not attest all checkout bytes",
                             "package artifact features record Cargo resolution, not custom Rust flags, wrappers or dependency feature unification",
                             "shared host, no verified CPU headroom or quota proof",
                             "repeats and concurrency are explicit; no claim-grade statistics",
                             "client CPU includes startup and handshake; server includes connection setup and cleanup",
                             "RSS is sampled; optional endpoint counters include setup; context switches are OS scheduling events, not task wakeups; task wakeups are unmeasured",
                             "optional strace includes all endpoint threads until exit; ptrace changes timing and scheduling; tracer CPU is excluded",
                             "zero-filled payload bodies; not a read-all adoption corpus",
                             "saturation, cold/idle lifecycles, read-all corpora and production soak are not run"]},
              "runs": []}
    (args.output / "source.patch").write_bytes(source_patch)
    (args.output / "harness.patch").write_bytes(harness_patch)
    (args.output / "harness.py").write_bytes(harness_bytes)
    if helper_bytes is not None:
        (args.output / "build-verifier.py").write_bytes(helper_bytes)
    (args.output / "manifest.json").write_text(json.dumps(report, indent=2) + "\n")
    for index, cell in enumerate(cells):
        run = run_cell(args, cell, args.output / f"cell-{index:03d}")
        report["runs"].append({"path": f"cell-{index:03d}/run.json", "passed": run["passed"]})
        print(f"{index + 1}/{len(cells)}: {'passed' if run['passed'] else run.get('error')}", flush=True)
    report["binary_unchanged"] = digest == hashlib.sha256(args.binary.read_bytes()).hexdigest()
    report["source_unchanged"] = True
    if args.build_record:
        try:
            verified_build(args.build_record.resolve(strict=True), args.binary, args.source_checkout,
                           require_allocation_counts=args.allocation_counts, verifier_sha256=helper_hash)
        except (OSError, ValueError) as error:
            report["source_unchanged"] = False
            report["source_error"] = str(error)
    report["harness_script_unchanged"] = (
        file_matches(Path(__file__), report["harness_script_sha256"])
        and file_matches(args.output / "harness.py", report["harness_script_sha256"])
    )
    report["build_verifier_unchanged"] = (helper_hash is None
        or (file_matches(helper_path, helper_hash)
            and file_matches(args.output / "build-verifier.py", helper_hash)))
    report["patches_unchanged"] = (
        file_matches(args.output / "source.patch", report["source_patch_sha256"])
        and file_matches(args.output / "harness.patch", report["harness_patch_sha256"])
    )
    report["passed"] = (report["binary_unchanged"] and report["source_unchanged"]
                        and report["harness_script_unchanged"] and report["build_verifier_unchanged"]
                        and report["patches_unchanged"] and all(run["passed"] for run in report["runs"]))
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
