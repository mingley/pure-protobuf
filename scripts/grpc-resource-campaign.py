#!/usr/bin/env python3
"""Frozen Linux resource campaign with TLS, gzip, mixed payloads and fault recovery."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import shutil
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
PHASES = ["profile", "warmup", "slow_reader", "overload", "cancelled", "deadline", "recovered", "fault", "drain"]
GAUGES = ["rss_bytes", "rss_hwm_bytes", "file_descriptors", "os_threads", "tokio_alive_tasks",
          "observed_streaming_calls_active", "observed_streaming_calls_peak", "observed_streaming_calls_started",
          "observed_streaming_calls_ended", "server_allocated_bytes", "client_allocated_bytes",
          "server_byte_peak", "client_byte_peak", "server_byte_tokens", "client_byte_tokens",
          "server_token_peak", "client_token_peak"]
LIMIT_NAMES = {"address_space": resource.RLIMIT_AS, "file_descriptors": resource.RLIMIT_NOFILE,
               "same_uid_processes": resource.RLIMIT_NPROC, "cpu_seconds": resource.RLIMIT_CPU}
SETTINGS = {"transport": "tcp_loopback_plaintext_and_tls", "compression": "identity_and_gzip", "runtime_workers": 2,
            "max_connections": 4, "max_h2_streams_per_connection": 8, "max_active_rpcs": 2,
            "max_message_bytes": 2097152, "per_stream_send_buffer_bytes": 16384,
            "client_byte_budget_bytes": 8388608, "server_byte_budget_bytes": 8388608,
            "client_stream_receive_window_bytes": 1024, "client_connection_receive_window_bytes": 4096,
            "client_outbound_stream_queue_messages": 1, "server_response_queue_messages": 4,
            "server_deadline_ms": 3000, "drain_grace_ms": 150,
            "slow_reader_stabilization_limit_ms": 1000, "slow_reader_observation_ms": 30, "slow_reader_responses": 128,
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


def executable_drift(executable, launch_sha256):
    try:
        actual = hashlib.sha256(executable.read_bytes()).hexdigest()
    except OSError:
        return ["executable disappeared during execution"]
    return [] if actual == launch_sha256 else ["executable changed during execution"]


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


def soak_disposition(requested, actual, exit_code, failures=()):
    if type(requested) not in (int, float) or not math.isfinite(requested) or requested < 86400:
        return "not_run"
    if (type(actual) not in (int, float) or not math.isfinite(actual)
            or actual < requested or exit_code != 0 or failures):
        return "failed"
    return "completed"


def validate_report(report):
    errors = []
    if report.get("schema") != "pbrs.resource-campaign.v3":
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
            or requested < 30 or requested > 86400 or actual < requested):
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
    soak_status = report.get("qualification", {}).get("soak_24h", {}).get("status")
    expected = soak_disposition(requested, actual, report.get("exit_code"),
                                report.get("smoke", {}).get("failures", []))
    if soak_status != expected:
        errors.append("24-hour disposition does not match elapsed duration and execution outcome")
    if report.get("exit_code") != 0:
        errors.append("resource test child failed")
    if report.get("smoke", {}).get("status") == "failed" or report.get("smoke", {}).get("failures"):
        errors.append("retained smoke has unresolved failures")
    events = report.get("events", [])
    profiles = [event for event in events if event.get("phase") == "profile"]
    if {(p.get("tls"), p.get("gzip")) for p in profiles} != {(False, False), (False, True), (True, False), (True, True)}:
        errors.append("missing plaintext/TLS and identity/gzip profile coverage")
    faults = [event for event in events if event.get("phase") == "fault"]
    if any(event.get("recovery_probe") != "warmed_independent_connection"
           or event.get("recovery_code") != "OK" or event.get("probe_timeout_ms") != 300 for event in faults):
        errors.append("missing successful independent fault recovery probe")
    if {e.get("fault") for e in faults} != {"RstStream(Cancel)", "Goaway", "TcpReset"}:
        errors.append("missing RST_STREAM, GOAWAY or TCP reset coverage")
    if not events or events[0].get("phase") != "baseline" or events[0].get("cycle") != 0:
        return errors + ["missing baseline phase accounting"]
    baseline = events[0]
    sequence = [(event.get("cycle"), event.get("phase")) for event in events[1:]]
    cycles = max((cycle for cycle, _ in sequence if type(cycle) is int), default=0)
    if cycles == 0 or sequence != [(cycle, phase) for cycle in range(1, cycles + 1) for phase in PHASES]:
        errors.append("incomplete, duplicated or reordered cycle phases")
    for event in events:
        cycle = event.get("cycle")
        if event.get("phase") == "profile":
            if (type(cycle) is not int or cycle < 1 or type(event.get("tls")) is not bool or type(event.get("gzip")) is not bool
                    or event["tls"] != ((cycle - 1) % 4 >= 2) or event["gzip"] != (cycle % 2 == 0)
                    or event.get("mixed_payload_bytes") != [0, 1024, 65536, 1048576]
                    or event.get("byte_budget") != 8388608 or event.get("message_limit") != 2097152
                    or event.get("server_deadline_ms") != 3000):
                errors.append("profile differs from frozen schedule or limits")
            continue
        if event.get("phase") == "fault":
            if (type(cycle) is not int or cycle < 1 or event.get("transport") != "plaintext_tcp"
                    or event.get("fault") != ["RstStream(Cancel)", "Goaway", "TcpReset"][(cycle - 1) % 3]):
                errors.append("fault differs from frozen schedule")
            continue
        if any(type(event.get(key)) is not int or event[key] < 0 for key in GAUGES):
            errors.append("missing or invalid resource gauge")
            continue
        if not event["rss_bytes"] or event["rss_hwm_bytes"] < event["rss_bytes"]:
            errors.append("invalid process RSS accounting")
        if event["observed_streaming_calls_peak"] > SETTINGS["max_active_rpcs"]:
            errors.append("observed streaming-call peak exceeds frozen limit")
        if event.get("phase") == "slow_reader":
            if type(event.get("stall_wait_ms")) is not int or not 30 <= event["stall_wait_ms"] <= 1050:
                errors.append("missing or out-of-bounds stall observation")
            first, second = event.get("producer_progress_before_hold"), event.get("producer_progress_after_hold")
            if (type(first) is not int or type(second) is not int or first != second
                    or first <= 0 or second >= SETTINGS["slow_reader_responses"]
                    or event.get("producer_done") is not False):
                errors.append("missing or invalid independent slow-producer progress")
        if event.get("phase") in ("overload", "cancelled", "deadline", "recovered", "drain"):
            if event.get("producer_sent_messages") != SETTINGS["slow_reader_responses"] or event.get("producer_done") is not True:
                errors.append("response producer did not finish exact delivery")
        for side in ("client", "server"):
            if event[f"{side}_byte_peak"] > SETTINGS[f"{side}_byte_budget_bytes"]:
                errors.append("accounted byte peak exceeds tracker budget")
        if event.get("phase") in ("cancelled", "deadline", "recovered", "drain"):
            idle = ["observed_streaming_calls_active", "server_allocated_bytes", "client_allocated_bytes",
                    "server_byte_tokens", "client_byte_tokens"]
            if any(event[key] != 0 for key in idle):
                errors.append("post-fault permits or observed streaming calls failed to recover")
            if event["observed_streaming_calls_started"] != event["observed_streaming_calls_ended"]:
                errors.append("observed streaming-call start/end accounting incomplete")
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
    samples = report.get("process_samples", [])
    times = [sample.get("elapsed_seconds") for sample in samples]
    if (not times or any(type(t) not in (int, float) or not math.isfinite(t) or t < 0 for t in times)
            or times != sorted(times) or times[0] > 2
            or (type(requested) in (int, float) and times[-1] < requested - 2)
            or any(b - a > 3 for a, b in zip(times, times[1:]))):
        errors.append("independent samples do not cover the requested duration")
    return sorted(set(errors))


def run(args):
    if platform.system() != "Linux":
        raise ValueError("Linux /proc accounting is required")
    if not math.isfinite(args.duration) or args.duration < 30 or args.duration > 86400:
        raise ValueError("duration must be finite and between 30 and 86400 seconds")
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
    build_executable = test_executable(messages)
    executable = output / "resource-test"
    shutil.copy2(build_executable, executable)
    test_command = [str(executable), "--exact", "current_h2_resource_campaign", "--ignored", "--nocapture", "--test-threads=1"]
    events_path = output / "events.jsonl"
    env = os.environ.copy()
    env.update(PBRS_CURRENT_H2_EVENTS=str(events_path), PBRS_CURRENT_H2_SECONDS=str(math.ceil(args.duration)),
               PBRS_CURRENT_H2_SEED=str(args.seed))
    samples, failures = [], []
    launch_sha256 = hashlib.sha256(executable.read_bytes()).hexdigest()
    start = time.monotonic()
    with (output / "test.stdout.log").open("w") as stdout, (output / "test.stderr.log").open("w") as stderr:
        child = subprocess.Popen(test_command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr,
                                 preexec_fn=lambda: apply_limits(limits))
        (output / "progress.json").write_text(json.dumps({"state": "running", "pid": child.pid,
            "source": source, "started_unix_seconds": time.time(), "duration_requested_seconds": args.duration,
            "binary_sha256": launch_sha256, "qualified": False}, indent=2) + "\n")
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
            time.sleep(1)
        exit_code = child.wait()
    failures.extend(executable_drift(executable, launch_sha256))
    events = []
    if events_path.exists():
        for line in events_path.open():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError as error:
                failures.append(f"invalid raw event: {error}")
    report = {"schema": "pbrs.resource-campaign.v3", "source": source,
              "host": dict(platform.uname()._asdict()), "seed": args.seed,
              "tools": {"rustc": command(["rustc", "-Vv"]), "cargo": command(["cargo", "-V"]),
                        "python": sys.version},
              "binary": {"path": str(executable), "build_path": str(build_executable), "sha256": launch_sha256},
              "commands": {"build": build_command, "test": test_command},
              "settings": SETTINGS, "process_limits": effective, "process_limits_requested": limits,
              "duration_requested_seconds": args.duration, "duration_actual_seconds": time.monotonic() - start,
              "events": events, "process_samples": samples, "exit_code": exit_code,
              "qualification": {"qualified": False, "tier": "shared-host-resource-campaign",
                  "soak_24h": {"status": soak_disposition(args.duration, time.monotonic() - start, exit_code, failures),
                               "reason": "resource campaign duration; overall QG-06 acceptance remains separate"},
                  "not_run": ["independent-process resource attribution", "allocator high-water", "kernel socket memory",
                              "TLS RST_STREAM/GOAWAY frame injection", "dedicated-host latency/goodput qualification",
                              "all feature and release gates"]}}

    errors = failures + validate_report(report)
    try:
        if freeze_source(args.source) != source:
            errors.append("source changed during execution")
    except ValueError as error:
        errors.append(str(error))
    report["qualification"]["soak_24h"]["status"] = soak_disposition(
        args.duration, report["duration_actual_seconds"], exit_code, errors)
    report["smoke"] = {"status": "failed" if errors else "passed", "failures": sorted(set(errors))}
    (output / "report.json").write_text(json.dumps(report, separators=(",", ":")) + "\n")
    (output / "progress.json").write_text(json.dumps({"state": report["smoke"]["status"],
        "report": str(output / "report.json"), "cycles": max((e.get("cycle", 0) for e in events), default=0),
        "duration_actual_seconds": report["duration_actual_seconds"], "qualified": False}, indent=2) + "\n")
    print(json.dumps({"report": str(output / "report.json"), "smoke": report["smoke"], "qualified": False}))
    return 1 if errors else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", help="required full clean HEAD pin")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--duration", type=float, default=86400)
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
        print(f"grpc-resource-campaign: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
