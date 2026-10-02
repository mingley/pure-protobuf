#!/usr/bin/env python3
"""Capture SB-32 with the unchanged devloop collectors and complete raw tools.

Run only in a coordinated measurement window with a source-pinned release
binary. The wrapper mode is private: symlinks named perf/valgrind/strace retain
every raw result before forwarding it to the existing collector.
"""

import argparse
import datetime
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
PREFIX = "rpc.tonic_transport."
PROFILES = ("reference_reference", "native_client_reference_server",
            "reference_client_native_server", "native_native")
SHAPES = ("unary", "server_stream")
SOURCE_ROOTS = ("src", "proto", "pbrs-grpc", "protobuf-tonic", "prost_tat", "v4_tat", "Cargo.toml", "Cargo.lock",
                "build.rs", "bench/devloop/src", "bench/devloop/proto",
                "bench/devloop/build.rs", "bench/devloop/Cargo.toml",
                "bench/devloop/Cargo.lock", "bench/devloop/adoption/src",
                "bench/devloop/adoption/proto", "bench/devloop/adoption/build.rs",
                "bench/devloop/adoption/Cargo.toml", "bench/devloop/adoption/Cargo.lock",
                "bench/devloop/measure-tonic-transport.py",
                "bench/devloop/check-tonic-transport-evidence.py",
                "bench/devloop/test_tonic_transport_collectors.py")
SCHEMAS = ("third_party/protobuf/src/google/protobuf/any.proto",
           "third_party/protobuf/src/google/protobuf/descriptor.proto")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def capture(command, directory, environment=None):
    """Persist bytes and process disposition before a caller can parse output."""
    directory.mkdir(parents=True, exist_ok=False)
    started = utc()
    interrupted = False
    spawn_error = None
    try:
        process = subprocess.Popen(command, cwd=ROOT, env=environment,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            stdout, stderr = process.communicate()
        except KeyboardInterrupt:
            interrupted = True
            process.terminate()
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                stdout, stderr = process.communicate()
        exit_code = process.returncode
    except OSError as error:
        spawn_error = {"type": type(error).__name__, "message": str(error), "errno": error.errno}
        stdout, stderr, exit_code = b"", b"", None
    (directory / "stdout").write_bytes(stdout)
    (directory / "stderr").write_bytes(stderr)
    record = {"argv": command, "cwd": str(ROOT), "exit_code": exit_code, "spawn_error": spawn_error,
              "started_utc": started, "finished_utc": utc(), "interrupted": interrupted,
              "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr)}
    write(directory / "process.json", record)
    return record, stdout, stderr


def tool_wrapper(name):
    real = json.loads(os.environ["PBRS_SB32_REAL_TOOLS"])[name]
    raw = Path(os.environ["PBRS_SB32_TOOL_RAW"]) / f"{time.time_ns()}-{os.getpid()}-{name}"
    arguments = sys.argv[1:]
    effective = list(arguments)
    is_callgrind = name == "valgrind" and "--tool=callgrind" in arguments
    if is_callgrind:
        # Only the artifact destination changes. Counter events, child argv,
        # runtime, warmup and preparation remain the existing collector's.
        effective = [f"--callgrind-out-file={raw}/callgrind.%p"
                     if a.startswith("--callgrind-out-file=") else a for a in effective]
    record, stdout, stderr = capture([real, *effective], raw)
    record["collector_argv"] = [name, *arguments]
    record["tool"] = name
    record["callgrind_files"] = []
    for path in sorted(raw.glob("callgrind.*")):
        content = path.read_bytes()
        compressed = gzip.compress(content, mtime=0)
        archive = path.with_suffix(path.suffix + ".gz")
        archive.write_bytes(compressed)
        record["callgrind_files"].append({"path": archive.name,
                                         "sha256": digest(compressed),
                                         "uncompressed_sha256": digest(content)})
        path.unlink()
    write(raw / "process.json", record)
    # Parent parsing cannot start until both raw streams and summary artifacts
    # above exist. The wrapper itself is outside the child measurement window.
    sys.stdout.buffer.write(stdout)
    sys.stderr.buffer.write(stderr)
    if record["spawn_error"] is not None:
        sys.stderr.write(json.dumps(record["spawn_error"]) + "\n")
        return 127
    return record["exit_code"] if record["exit_code"] >= 0 else 128 - record["exit_code"]


def git(*arguments):
    return subprocess.check_output(["git", *arguments], cwd=ROOT)


def source_hashes():
    paths = git("ls-files", "--", *SOURCE_ROOTS).decode().splitlines()
    return {path: digest((ROOT / path).read_bytes()) for path in paths}


def marker(stderr, name):
    rows = [json.loads(line[len(name):]) for line in stderr.decode().splitlines()
            if line.startswith(name)]
    assert len(rows) == 1, f"expected one {name} record"
    return rows[0]


def validate_network(row, expected):
    for field in ("id", "request_bytes", "native_request_bytes", "response_bytes_per_message",
                  "request_read_checksum", "response_read_checksum", "responses",
                  "input_wire_fingerprint", "message_caps", "accepted_socket_observer"):
        assert row[field] == expected[field], f"network oracle: {row['id']} {field}"
    assert row["accepted_tcp_nodelay"] and row["full_decoded_equality"] and row["equal_wire_work"]
    assert row["accepted_local_addr"].startswith("127.0.0.1:")
    assert row["accepted_remote_addr"].startswith("127.0.0.1:")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--build-record", type=Path, required=True,
                        help="shared release provenance: binary_sha256 and compiled source_sha256")
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--iters", type=int, choices=(16,), default=16)
    parser.add_argument("--repeats", type=int, choices=(3,), default=3)
    parser.add_argument("--jobs", type=int, choices=(1,), default=1)
    parser.add_argument("--qualify-only", action="store_true")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    binary, out = args.binary.resolve(), args.out.resolve()
    inventory_path = args.inventory.resolve()
    inventory = json.loads(inventory_path.read_bytes())
    ids = inventory["registered_ids"]
    oracle = {r["id"]: r for r in inventory["preflight_rows"]}
    blocked = set(inventory["blocked_ids"])
    assert len(ids) == len(set(ids)) == 512 and len(oracle) == 488 and len(blocked) == 24
    assert set(ids) == oracle.keys() | blocked
    assert all(".maps." in i for i in blocked)
    frozen_specimens = json.loads((ROOT / "bench/devloop/adoption/evidence/rpc-inventory.json").read_bytes())
    old_ids = {f"rpc.adoption.{p}.{r['specimen']}.{s}" for p in
               ("native_pbrs", "native_prost", "tonic_pbrs", "tonic_prost")
               for r in frozen_specimens for s in SHAPES}
    assert len(old_ids) == 512
    commit = git("rev-parse", "HEAD").decode().strip()
    sources = source_hashes()
    schema_bytes = {path: (ROOT / path).read_bytes() for path in SCHEMAS}
    schemas = {path: digest(data) for path, data in schema_bytes.items()}
    for path, checksum in sources.items():
        assert digest(git("show", f"{commit}:{path}")) == checksum, f"uncommitted source: {path}"
    assert digest(git("show", f"{commit}:{inventory_path.relative_to(ROOT)}")) == digest(inventory_path.read_bytes())
    build_bytes = args.build_record.resolve().read_bytes()
    build = json.loads(build_bytes)
    assert build["binary_sha256"] == digest(binary.read_bytes())
    assert build["profile"] == "release"
    assert build["build_argv"] and "features" in build and build["tools"]
    for path, checksum in sources.items():
        if not path.endswith(".py"):
            assert build["source_sha256"][path] == checksum, f"release build source: {path}"
    for path, checksum in schemas.items():
        assert build["schema_sha256"][path] == checksum, f"release build schema: {path}"
    tools = {name: str(Path(path).resolve()) for name in ("perf", "valgrind", "strace")
             if (path := shutil.which(name)) is not None}
    # Preserve existing perf-first/Callgrind-fallback tool selection. Never
    # hide an available but failing tool to turn a missing metric into a win.
    if args.resume:
        record = json.loads((out / "measurement.json").read_text())
        assert record["source_sha256"] == sources and record["binary_sha256"] == digest(binary.read_bytes())
        assert record["inventory_sha256"] == digest(inventory_path.read_bytes())
        assert record["tools"] == tools
        assert record["build_record_sha256"] == digest(build_bytes)
        assert record["schema_sha256"] == schemas
        assert record["source_commit"] == commit, "resume the exact pinned source checkout"
    else:
        out.mkdir(parents=True, exist_ok=False)
        (out / "release-build.json").write_bytes(build_bytes)
        (out / "schemas").mkdir()
        for path, data in schema_bytes.items():
            (out / "schemas" / Path(path).name).write_bytes(data)
        record = {"schema": "pbrs-sb32-measurement/1", "status": "qualifying",
                  "tier": "instrumented dev-loop diagnostic; no performance-leadership claim",
                  "source_commit": commit, "source_sha256": sources,
                  "binary": str(binary), "binary_sha256": digest(binary.read_bytes()),
                  "build_record_sha256": digest(build_bytes),
                  "schema_sha256": schemas,
                  "inventory_path": str(inventory_path.relative_to(ROOT)),
                  "inventory_sha256": digest(inventory_path.read_bytes()),
                  "tools": tools, "tool_pins": {}, "processes": {}, "cells": {}, "reports": {},
                  "iters": 16, "double_iters": 32, "common_prepare_iters": 32,
                  "repeats": 3, "warmup": 100, "runtime_workers": 2, "report_parallelism": 1,
                  "syscall_scope": "strace -c -f N16-only whole-process counts divided by 16, including preparation, network oracle, 100-operation warmup and teardown; not hot-only or differential syscall cost; futex includes wakes/errors, not lock acquisitions",
                  "limits": ["24 frozen map equal-wire blocks", "instrumented shared-host wall time is secondary", "copy hooks do not measure every prost copy"],
                  "started_utc": utc()}
        write(out / "measurement.json", record)
    wrappers = out / "tool-wrappers"
    wrappers.mkdir(exist_ok=True)
    for name in tools:
        link = wrappers / name
        if not link.exists():
            link.symlink_to(Path(__file__).resolve())
    environment = dict(os.environ, LC_ALL="C")
    environment["PATH"] = str(wrappers) + os.pathsep + os.environ.get("PATH", "")
    environment["PBRS_SB32_REAL_TOOLS"] = json.dumps(tools)
    for name, path in tools.items():
        if name not in record["tool_pins"]:
            relative = f"raw/tool-version-{name}"
            result, _, _ = capture([path, "--version"], out / relative, environment)
            record["processes"][relative] = result
            record["tool_pins"][name] = {"executable_sha256": digest(Path(path).read_bytes()), "version_process": relative}
            write(out / "measurement.json", record)
    if "raw/registry" not in record["processes"]:
        result, stdout, _ = capture([str(binary), "list"], out / "raw/registry", environment)
        record["processes"]["raw/registry"] = result
        write(out / "measurement.json", record)
        assert result["exit_code"] == 0
        assert {line.split()[0] for line in stdout.decode().splitlines() if line.startswith(PREFIX)} == set(ids)
        assert {line.split()[0] for line in stdout.decode().splitlines() if line.startswith("rpc.adoption.")} == old_ids
    for cell in ids:
        if cell in record["cells"]:
            continue
        relative = f"raw/preflight/{cell}"
        result, _, stderr = capture([str(binary), "run-cell", cell, "--iters", "1",
                                    "--prepare-iters", "32", "--warmup", "0"], out / relative, environment)
        record["processes"][relative] = result
        write(out / "measurement.json", record)
        if cell in blocked:
            assert result["exit_code"] != 0 and b"RPC encoded bytes must agree" in stderr
            record["cells"][cell] = {"status": "not_run", "measurement_status": "not_run", "reason": inventory["blocked_reason"], "preflight_process": relative}
        else:
            assert result["exit_code"] == 0
            network, child = marker(stderr, "__QUALIFICATION__ "), marker(stderr, "__CHILD__ ")
            validate_network(network, oracle[cell])
            assert child["cell"] == cell and child["iters"] == 1
            assert child["input_wire_fingerprint"] == network["input_wire_fingerprint"]
            record["cells"][cell] = {"status": "qualified", "measurement_status": "not_run", "network": network, "preflight_process": relative}
        write(out / "measurement.json", record)
    record["status"] = "qualified"
    write(out / "measurement.json", record)
    if args.qualify_only:
        return
    assert "perf" in tools or "valgrind" in tools, "no differential instruction tool; numeric qualification remains blocked"
    # Two independent unchanged reference replays, then every matrix group.
    batches = [(f"baseline-replay-{r}-{shape}", "reference_reference", shape)
               for r in (1, 2) for shape in SHAPES]
    batches += [(f"matrix-{profile}-{shape}", profile, shape) for profile in PROFILES for shape in SHAPES]
    record["status"] = "measuring"
    for name, profile, shape in batches:
        if name in record["reports"]:
            continue
        assert source_hashes() == sources and digest(binary.read_bytes()) == record["binary_sha256"]
        assert {p: digest((ROOT / p).read_bytes()) for p in SCHEMAS} == schemas
        selected = [i for i in ids if i.startswith(f"{PREFIX}{profile}.") and i.endswith(f".{shape}") and i not in blocked]
        assert len(selected) == 61
        relative = f"raw/collector/{name}-{time.time_ns()}"
        tool_raw = out / relative / "tools"
        environment["PBRS_SB32_TOOL_RAW"] = str(tool_raw)
        report_path = out / f"{name}.json"
        result, _, _ = capture([str(binary), "run", "--cells", ",".join(selected),
                               "--iters", "16", "--repeats", "3", "--out", str(report_path)],
                              out / relative, environment)
        result["raw_tool_records"] = {str(p.relative_to(out)): digest(p.read_bytes())
                                      for p in sorted(tool_raw.glob("*/process.json"))}
        record["processes"][relative] = result
        write(out / "measurement.json", record)
        assert result["exit_code"] == 0, f"collector failed; full raw retained: {relative}"
        report = json.loads(report_path.read_bytes())
        assert {r["id"] for r in report["cells"]} == set(selected) and len(report["cells"]) == 61
        record["reports"][name] = {"path": report_path.name, "sha256": digest(report_path.read_bytes()),
                                   "collector_process": relative,
                                   "raw_tool_records": result["raw_tool_records"]}
        if name.startswith("matrix-"):
            for cell in selected:
                record["cells"][cell]["measurement_status"] = "captured"
        write(out / "measurement.json", record)
        print(f"Captured {name}: 61 cells, all raw counters retained", flush=True)
    assert source_hashes() == sources and digest(binary.read_bytes()) == record["binary_sha256"]
    assert {p: digest((ROOT / p).read_bytes()) for p in SCHEMAS} == schemas
    record["status"] = "captured; independent audit pending"
    record["finished_utc"] = utc()
    write(out / "measurement.json", record)
    print("All 512 states retained: 488 captured, 24 map blocks. Run the independent audit.")


if __name__ == "__main__":
    invoked = Path(sys.argv[0]).name
    if invoked in ("perf", "valgrind", "strace") and "PBRS_SB32_REAL_TOOLS" in os.environ:
        sys.exit(tool_wrapper(invoked))
    main()
