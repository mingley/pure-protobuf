#!/usr/bin/env python3
"""Frozen, finite-limit Linux current-h2 smoke; never production qualification."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
PHASES = ["warmup", "slow_reader", "overload", "cancelled", "deadline", "recovered", "drain"]
GAUGES = ["rss_bytes", "rss_hwm_bytes", "file_descriptors", "os_threads", "tokio_alive_tasks",
          "admitted_calls_active", "admitted_calls_peak", "admitted_calls_started",
          "admitted_calls_ended", "server_allocated_bytes", "client_allocated_bytes",
          "server_byte_peak", "client_byte_peak", "server_byte_tokens", "client_byte_tokens",
          "server_token_peak", "client_token_peak"]
LIMIT_NAMES = {"address_space": resource.RLIMIT_AS, "file_descriptors": resource.RLIMIT_NOFILE,
               "same_uid_processes": resource.RLIMIT_NPROC, "cpu_seconds": resource.RLIMIT_CPU}
SETTINGS = {"transport": "tcp_loopback_plaintext", "compression": "none", "runtime_workers": 2,
            "max_connections": 4, "max_h2_streams_per_connection": 8, "max_active_rpcs": 2,
            "max_message_bytes": 65536, "per_stream_send_buffer_bytes": 16384,
            "client_byte_budget_bytes": 262144, "server_byte_budget_bytes": 262144,
            "client_stream_receive_window_bytes": 1024, "client_connection_receive_window_bytes": 4096,
            "server_deadline_ms": 300, "drain_grace_ms": 150,
            "slow_reader_hold_ms": 60, "slow_reader_responses": 16,
            "slow_reader_response_bytes": 2048,
            "recovery_rss_tolerance_bytes": 32 * 1024 * 1024,
            "recovery_fd_tolerance": 1, "recovery_tokio_task_tolerance": 2}


def command(args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def freeze_source(expected):
    if not expected or not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("a full --source commit pin is required")
    actual = command(["git", "rev-parse", "HEAD"])
    if actual != expected:
        raise ValueError("source HEAD differs from --source")
    if command(["git", "status", "--porcelain", "--untracked-files=all"]):
        raise ValueError("source must be clean, including untracked files")
    return {"commit": actual, "tree": command(["git", "rev-parse", "HEAD^{tree}"]),
            "dirty": False, "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()}


def finite_limits(duration, memory, files, processes):
    values = {"address_space": memory, "file_descriptors": files,
              "same_uid_processes": processes, "cpu_seconds": math.ceil(duration) + 30}
    for key, value in values.items():
        if type(value) is not int or value <= 0 or value == resource.RLIM_INFINITY:
            raise ValueError(f"{key} must have a positive finite process limit")
        _, available = resource.getrlimit(LIMIT_NAMES[key])
        if available != resource.RLIM_INFINITY and value > available:
            raise ValueError(f"{key} exceeds inherited hard limit")
    return {key: {"soft": value, "hard": value} for key, value in values.items()}


def apply_limits(limits):
    # This function runs only in the child, after compilation. RLIMIT_NPROC
    # counts all processes/threads under this uid; it is not a Tokio task cap.
    for key, number in LIMIT_NAMES.items():
        entry = limits[key]
        resource.setrlimit(number, (entry["soft"], entry["hard"]))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def test_executable(messages):
    manifest = str(ROOT / "pbrs-grpc/Cargo.toml")
    artifacts = [entry["executable"] for entry in messages
                 if entry.get("reason") == "compiler-artifact"
                 and entry.get("manifest_path") == manifest
                 and entry.get("target", {}).get("name") == "resource_qualification"
                 and entry.get("target", {}).get("kind") == ["test"]
                 and entry.get("executable")]
    if len(artifacts) != 1:
        raise ValueError("Cargo must report exactly one current resource_qualification test executable")
    return Path(artifacts[0])


def observe_process(pid, elapsed):
    status = (Path("/proc") / str(pid) / "status").read_text()
    memory = {}
    for line in status.splitlines():
        if line.startswith(("VmRSS:", "VmHWM:", "VmSize:")):
            key, value, units = line.split()
            if units != "kB":
                raise ValueError("unexpected Linux memory unit")
            memory[key[:-1]] = int(value) * 1024
    if set(memory) != {"VmRSS", "VmHWM", "VmSize"}:
        raise ValueError("process exited or required Linux memory counters are missing")
    return {"elapsed_seconds": elapsed, "pid": pid, "memory_bytes": memory,
            "os_threads": len(list((Path("/proc") / str(pid) / "task").iterdir())),
            "file_descriptors": len(list((Path("/proc") / str(pid) / "fd").iterdir()))}


def validate_report(report):
    errors = []
    if report.get("schema") != "pbrs.current-h2-smoke.v1":
        errors.append("unknown evidence schema")
    source = report.get("source", {})
    if not re.fullmatch(r"[0-9a-f]{40}", str(source.get("commit", ""))) or source.get("dirty") is not False:
        errors.append("missing or dirty source pin")
    if not re.fullmatch(r"[0-9a-f]{40}", str(source.get("tree", ""))):
        errors.append("missing source tree pin")
    if not re.fullmatch(r"[0-9a-f]{64}", str(source.get("cargo_lock_sha256", ""))):
        errors.append("missing Cargo lockfile pin")
    if not re.fullmatch(r"[0-9a-f]{64}", str(report.get("binary", {}).get("sha256", ""))):
        errors.append("missing executable pin")
    if any(not report.get("tools", {}).get(tool) for tool in ("rustc", "cargo", "python")):
        errors.append("incomplete tool pins")
    if any(not report.get("commands", {}).get(stage) for stage in ("build", "test")):
        errors.append("missing exact execution commands")
    requested, actual = report.get("duration_requested_seconds"), report.get("duration_actual_seconds")
    if (type(requested) not in (int, float) or type(actual) not in (int, float)
            or not math.isfinite(requested) or not math.isfinite(actual)
            or requested < 1 or requested > 86400 or actual < requested):
        errors.append("missing or incomplete requested duration")
    if report.get("settings") != SETTINGS:
        errors.append("settings differ from frozen scenario")
    for key in LIMIT_NAMES:
        limit = report.get("process_limits", {}).get(key, {})
        soft, hard = limit.get("soft"), limit.get("hard")
        if (type(soft) is not int or type(hard) is not int
                or soft <= 0 or hard <= 0 or soft > hard):
            errors.append(f"nonfinite or invalid {key} process limit")
    if report.get("process_limits_requested") != report.get("process_limits"):
        errors.append("effective process limits differ from frozen requested limits")
    if report.get("qualification", {}).get("qualified") is not False:
        errors.append("a short diagnostic cannot be accepted as production qualification")
    if report.get("qualification", {}).get("soak_24h", {}).get("status") != "not_run":
        errors.append("24-hour qualification disposition must remain not_run")
    if report.get("exit_code") != 0:
        errors.append("resource test child failed")
    if report.get("smoke", {}).get("status") == "failed" or report.get("smoke", {}).get("failures"):
        errors.append("retained smoke has unresolved failures")
    events = report.get("events", [])
    if not events or events[0].get("phase") != "baseline" or events[0].get("cycle") != 0:
        return errors + ["missing baseline phase accounting"]
    baseline = events[0]
    sequence = [(event.get("cycle"), event.get("phase")) for event in events[1:]]
    cycles = max((cycle for cycle, _ in sequence if type(cycle) is int), default=0)
    if cycles == 0 or sequence != [(cycle, phase) for cycle in range(1, cycles + 1) for phase in PHASES]:
        errors.append("incomplete, duplicated or reordered cycle phases")
    for event in events:
        if any(type(event.get(key)) is not int or event[key] < 0 for key in GAUGES):
            errors.append("missing or invalid resource gauge")
            continue
        if not event["rss_bytes"] or event["rss_hwm_bytes"] < event["rss_bytes"]:
            errors.append("invalid process RSS accounting")
        if event["admitted_calls_peak"] > SETTINGS["max_active_rpcs"]:
            errors.append("admitted-call peak exceeds frozen limit")
        for side in ("client", "server"):
            if event[f"{side}_byte_peak"] > SETTINGS[f"{side}_byte_budget_bytes"]:
                errors.append("accounted byte peak exceeds tracker budget")
        if event.get("phase") in ("cancelled", "deadline", "recovered", "drain"):
            idle = ["admitted_calls_active", "server_allocated_bytes", "client_allocated_bytes",
                    "server_byte_tokens", "client_byte_tokens"]
            if any(event[key] != 0 for key in idle):
                errors.append("post-fault permits or admitted calls failed to recover")
            if event["admitted_calls_started"] != event["admitted_calls_ended"]:
                errors.append("admitted-call start/end accounting incomplete")
        if event.get("phase") == "drain" and all(type(baseline.get(key)) is int for key in GAUGES):
            if event["rss_bytes"] > baseline["rss_bytes"] + SETTINGS["recovery_rss_tolerance_bytes"]:
                errors.append("post-drain RSS exceeds predeclared tolerance")
            if event["file_descriptors"] > baseline["file_descriptors"] + SETTINGS["recovery_fd_tolerance"]:
                errors.append("post-drain descriptors exceed predeclared tolerance")
            if event["tokio_alive_tasks"] > baseline["tokio_alive_tasks"] + SETTINGS["recovery_tokio_task_tolerance"]:
                errors.append("post-drain Tokio tasks exceed predeclared tolerance")
    if not report.get("process_samples"):
        errors.append("missing independent process sampling")
    for sample in report.get("process_samples", []):
        memory = sample.get("memory_bytes", {})
        if (any(type(memory.get(key)) is not int or memory[key] < 0
                for key in ("VmRSS", "VmHWM", "VmSize"))
                or any(type(sample.get(key)) is not int or sample[key] < 0
                       for key in ("os_threads", "file_descriptors"))):
            errors.append("incomplete independent process sample")
    return sorted(set(errors))


def run(args):
    if platform.system() != "Linux":
        raise ValueError("Linux /proc accounting is required")
    if not math.isfinite(args.duration) or args.duration < 1 or args.duration > 86400:
        raise ValueError("duration must be finite and between 1 and 86400 seconds")
    if args.seed < 0 or args.seed > 2**64 - 1:
        raise ValueError("seed must fit an unsigned 64-bit integer")
    source = freeze_source(args.source)
    limits = finite_limits(args.duration, args.memory_bytes, args.max_fds, args.max_uid_processes)
    output = args.output.resolve()
    if output.exists():
        raise ValueError("output directory must be new to preserve earlier failures")
    # Artifacts must live outside tracked/untracked source or in an ignored directory.
    output.mkdir(parents=True)
    build_command = ["cargo", "test", "--locked", "-p", "pbrs-grpc", "--test",
                     "resource_qualification", "--no-run", "--message-format=json"]
    with (output / "build.stdout.jsonl").open("w") as stdout, (output / "build.stderr.log").open("w") as stderr:
        subprocess.run(build_command, cwd=ROOT, stdout=stdout, stderr=stderr, check=True)
    source_after = freeze_source(args.source)
    if source_after != source:
        raise ValueError("source changed during build")
    messages = [json.loads(line) for line in (output / "build.stdout.jsonl").read_text().splitlines()]
    executable = test_executable(messages)
    test_command = [str(executable), "--exact", "current_h2_resource_smoke", "--ignored", "--nocapture", "--test-threads=1"]
    events_path = output / "events.jsonl"
    env = os.environ.copy()
    env.update(PBRS_CURRENT_H2_EVENTS=str(events_path), PBRS_CURRENT_H2_SECONDS=str(math.ceil(args.duration)),
               PBRS_CURRENT_H2_SEED=str(args.seed))
    samples, failures = [], []
    start = time.monotonic()
    with (output / "test.stdout.log").open("w") as stdout, (output / "test.stderr.log").open("w") as stderr:
        child = subprocess.Popen(test_command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr,
                                 preexec_fn=lambda: apply_limits(limits))
        try:
            effective = {key: dict(zip(("soft", "hard"), resource.prlimit(child.pid, number)))
                         for key, number in LIMIT_NAMES.items()}
        except ProcessLookupError:
            effective = {}
            failures.append("child exited before effective process limits were observed")
        while child.poll() is None:
            try:
                samples.append(observe_process(child.pid, time.monotonic() - start))
            except (OSError, ValueError) as error:
                # A child exit may race its final sample; a live sampling error is retained.
                if child.poll() is None:
                    failures.append(str(error))
            if time.monotonic() - start > args.duration + 15:
                child.kill()
                failures.append("child exceeded frozen wall-time watchdog")
                break
            time.sleep(0.05)
        exit_code = child.wait()
    events = []
    if events_path.exists():
        for line in events_path.read_text().splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError as error:
                failures.append(f"invalid raw event: {error}")
    report = {"schema": "pbrs.current-h2-smoke.v1", "source": source,
              "host": dict(platform.uname()._asdict()), "seed": args.seed,
              "tools": {"rustc": command(["rustc", "-Vv"]), "cargo": command(["cargo", "-V"]),
                        "python": sys.version},
              "binary": {"path": str(executable), "sha256": hashlib.sha256(executable.read_bytes()).hexdigest()},
              "commands": {"build": build_command, "test": test_command},
              "settings": SETTINGS, "process_limits": effective, "process_limits_requested": limits,
              "duration_requested_seconds": args.duration, "duration_actual_seconds": time.monotonic() - start,
              "events": events, "process_samples": samples, "exit_code": exit_code,
              "qualification": {"qualified": False, "tier": "shared-host-diagnostic",
                  "soak_24h": {"status": "not_run", "reason": "this instrumented smoke is not the complete QG-06 campaign"},
                  "not_run": ["TLS/compression profiles", "injected RST_STREAM/GOAWAY", "independent peer",
                              "mixed 1 MiB messages", "allocator high-water", "kernel socket memory",
                              "dedicated-host latency/goodput qualification", "24-hour acceptance"]}}
    errors = failures + validate_report(report)
    try:
        if freeze_source(args.source) != source:
            errors.append("source changed during execution")
    except ValueError as error:
        errors.append(str(error))
    report["smoke"] = {"status": "failed" if errors else "passed", "failures": sorted(set(errors))}
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(output / "report.json"), "smoke": report["smoke"], "qualified": False}))
    return 1 if errors else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", help="required full clean HEAD pin")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--duration", type=float, default=30)
    parser.add_argument("--seed", type=int, default=20261002)
    parser.add_argument("--memory-bytes", type=int, default=1024 * 1024 * 1024)
    parser.add_argument("--max-fds", type=int, default=128)
    parser.add_argument("--max-uid-processes", type=int, default=4096)
    parser.add_argument("--validate", type=Path, help="validate a retained report without executing it")
    args = parser.parse_args()
    if args.validate:
        errors = validate_report(json.loads(args.validate.read_text()))
        print(json.dumps({"failures": errors, "qualified": False}))
        return 1 if errors else 0
    if args.output is None:
        parser.error("--output is required for execution")
    try:
        return run(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"current-h2-smoke: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
