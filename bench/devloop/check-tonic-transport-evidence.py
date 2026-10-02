#!/usr/bin/env python3
"""Independently reconstruct SB-32 from raw children, counters and provenance.

Exit success means the archive is internally consistent. Replay failures,
unavailable metrics and every measured loss remain explicit in the summary;
an integrity audit does not turn them into performance qualification.
"""

import argparse
import gzip
import hashlib
import json
import math
from pathlib import Path
import re
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PREFIX = "rpc.tonic_transport."
PROFILES = ("reference_reference", "native_client_reference_server",
            "reference_client_native_server", "native_native")
SHAPES = ("unary", "server_stream")
PRIMARY = ("instructions", "allocs", "alloc_bytes", "syscalls", "locks")
SYSCALL_SCOPE = "strace -c -f N16-only whole-process counts divided by 16, including preparation, network oracle, 100-operation warmup and teardown; not hot-only or differential syscall cost; futex includes wakes/errors, not lock acquisitions"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git_file(commit, path):
    return subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=ROOT)


def integer(value):
    assert re.fullmatch(r"[0-9]+", value), f"invalid counter {value!r}"
    return int(value)


def instruction_counter(stderr, tool):
    if tool == "valgrind":
        matches = re.findall(r"I   refs:\s*([^\s]+)", stderr)
        if not matches:
            return None
        if len(matches) != 1 or re.fullmatch(r"[0-9]+", matches[0].replace(",", "")) is None:
            return None
        return int(matches[0].replace(",", ""))
    matches = [line.split(",")[0].strip() for line in stderr.splitlines()
               if len(line.split(",")) >= 3 and line.split(",")[2].strip().split(":")[0] == "instructions"]
    if not matches or any(re.fullmatch(r"[0-9]+", v) is None for v in matches):
        return None
    if len(matches) != 1:
        return None
    return int(matches[0])


def syscall_counter(stderr):
    total, futex = [], 0
    for line in stderr.splitlines():
        fields = line.split()
        if not fields or fields[-1] not in ("total", "futex", "futex_waitv"):
            continue
        if len(fields) not in (5, 6) or re.fullmatch(r"[0-9]+", fields[3]) is None:
            return None
        calls = int(fields[3])
        if fields[-1] == "total":
            total.append(calls)
        else:
            futex += calls
    if len(total) != 1 or futex > total[0]:
        return None
    return total[0], futex


def marked(stderr, name):
    lines = [line[len(name):] for line in stderr.splitlines() if line.startswith(name)]
    assert len(lines) == 1, f"expected one {name}"
    return json.loads(lines[0])


def checked_process(directory, relative, expected=None):
    path = directory / relative
    record = json.loads((path / "process.json").read_bytes())
    stdout, stderr = (path / "stdout").read_bytes(), (path / "stderr").read_bytes()
    assert sha(stdout) == record["stdout_sha256"] and sha(stderr) == record["stderr_sha256"]
    if expected is not None:
        assert {k: v for k, v in expected.items() if k != "raw_tool_records"} == record
    for artifact in record.get("callgrind_files", []):
        compressed = (path / artifact["path"]).read_bytes()
        assert sha(compressed) == artifact["sha256"]
        content = gzip.decompress(compressed)
        assert sha(content) == artifact["uncompressed_sha256"]
        # The collector asks for only Ir. Validate the actual tool's summary
        # file against its stderr counter, rather than trusting either alone.
        if record["exit_code"] == 0:
            text = content.decode()
            assert re.findall(r"^events: (.*)$", text, re.M) == ["Ir"]
            summaries = re.findall(r"^summary: ([0-9]+)$", text, re.M)
            totals = re.findall(r"^totals: ([0-9]+)$", text, re.M)
            count = instruction_counter(stderr.decode(errors="replace"), "valgrind")
            assert len(summaries) == 1 and len(totals) == 1
            assert int(summaries[0]) == int(totals[0])
            if count is not None:
                assert int(summaries[0]) == count
    return record, stdout, stderr.decode(errors="replace")


def network(row, oracle):
    for field in ("id", "request_bytes", "native_request_bytes", "response_bytes_per_message",
                  "request_read_checksum", "response_read_checksum", "responses",
                  "input_wire_fingerprint", "message_caps", "accepted_socket_observer"):
        assert row[field] == oracle[field], f"network oracle: {row['id']} {field}"
    assert row["accepted_tcp_nodelay"] and row["full_decoded_equality"] and row["equal_wire_work"]
    assert row["accepted_local_addr"].startswith("127.0.0.1:") and row["accepted_remote_addr"].startswith("127.0.0.1:")


def value(row, metric):
    result = row[metric]
    assert result["status"] in ("measured", "not_run")
    if result["status"] == "not_run":
        assert result["data"]["reason"]
        return None
    result = result["data"]["value"]
    assert isinstance(result, (int, float)) and math.isfinite(result) and result >= 0
    return result


def metric(row, name, samples):
    expected = statistics.median(samples) if len(samples) == 3 else None
    actual = value(row, name)
    assert actual == expected, f"raw reconstruction: {row['id']} {name}: {actual} != {expected}"


def reconstruct(directory, report_info, report, oracle, selected_tool, tools, binary):
    raw = {}
    for relative, checksum in sorted(report_info["raw_tool_records"].items()):
        assert sha((directory / relative).read_bytes()) == checksum
        process, _, stderr = checked_process(directory, str(Path(relative).parent))
        argv = process["collector_argv"]
        tool = process["tool"]
        assert process["argv"][0] == tools[tool]
        effective = list(process["argv"][1:])
        if tool == "valgrind":
            assert effective[2].startswith("--callgrind-out-file=")
            effective[2] = "--callgrind-out-file=/tmp/devloop-callgrind.%p"
        assert effective == argv[1:], "effective tool/child arguments differ from collector"
        if tool == "strace":
            assert argv[:3] == ["strace", "-c", "-f"]
            assert argv[3] == binary
            child_args = argv[4:]
        elif tool == "valgrind":
            assert argv[:4] == ["valgrind", "--tool=callgrind", "--cache-sim=no", "--callgrind-out-file=/tmp/devloop-callgrind.%p"]
            assert argv[4] == binary
            child_args = argv[5:]
            if instruction_counter(stderr, tool) is not None:
                assert len(process["callgrind_files"]) == 1, "Callgrind summary artifact missing"
        else:
            assert tool == "perf" and argv[:6] == ["perf", "stat", "-x,", "-e", "instructions", "--"]
            assert argv[6] == binary
            child_args = argv[7:]
        assert child_args[:1] == ["run-cell"]
        cell = child_args[1]
        assert cell in oracle
        count = integer(child_args[child_args.index("--iters") + 1])
        assert count in (16, 32)
        if tool != "strace":
            assert child_args == ["run-cell", cell, "--iters", str(count), "--prepare-iters", "32"]
        else:
            assert child_args == ["run-cell", cell, "--iters", "16"]
        raw.setdefault(cell, {}).setdefault(tool, []).append((process, stderr, count))
    assert set(raw) == {r["id"] for r in report["cells"]}
    for row in report["cells"]:
        cell = row["id"]
        assert row["kind"] == "rpc" and row["codec"] == cell.removeprefix(PREFIX).split(".")[0]
        assert row["iters"] == 16 and row["repeats"] == 3
        assert row["input_wire_fingerprint"] == oracle[cell]["input_wire_fingerprint"]
        assert set(raw[cell]) == ({selected_tool, "strace"} if "strace" in tools else {selected_tool})
        counts = {16: [], 32: []}
        base_children = []
        for process, stderr, requested in raw[cell][selected_tool]:
            assert process["exit_code"] == 0 and not process["interrupted"]
            child, qualified = marked(stderr, "__CHILD__ "), marked(stderr, "__QUALIFICATION__ ")
            assert child["cell"] == cell and child["iters"] == requested, "child N/2N marker differs from actual argv"
            assert child["input_wire_fingerprint"] == oracle[cell]["input_wire_fingerprint"]
            network(qualified, oracle[cell])
            counts[child["iters"]].append(instruction_counter(stderr, selected_tool))
            if child["iters"] == 16:
                base_children.append(child)
        assert len(base_children) == len(counts[16]) == 3
        assert len(counts[32]) == sum(c is not None for c in counts[16])
        deltas, next_double = [], iter(counts[32])
        for first in counts[16]:
            if first is None:
                continue
            second = next(next_double)
            if second is not None:
                assert second >= first, "instruction delta underflow"
                deltas.append((second - first) / 16)
        metric(row, "instructions", deltas)
        expected_method = f"differential_{'callgrind' if selected_tool == 'valgrind' else 'perf'}_2n_minus_n" if len(deltas) == 3 else "not_run"
        assert row["instruction_method"] == expected_method
        for name in ("allocs", "alloc_bytes", "wall_ns"):
            metric(row, name, [c[name] / 16 for c in base_children])
        if row.get("copy_counts") is not None:
            for name, count in row["copy_counts"].items():
                assert count == statistics.median([c["copy_counts"][name] / 16 for c in base_children])
        calls, locks = [], []
        traces = raw[cell].get("strace", [])
        assert len(traces) == (3 if "strace" in tools else 0), f"missing strace raw: {cell}"
        for process, stderr, requested in traces:
            summary = syscall_counter(stderr) if process["exit_code"] == 0 else None
            if summary is not None:
                child, qualified = marked(stderr, "__CHILD__ "), marked(stderr, "__QUALIFICATION__ ")
                assert child["cell"] == cell and child["iters"] == requested == 16
                assert child["input_wire_fingerprint"] == oracle[cell]["input_wire_fingerprint"]
                network(qualified, oracle[cell])
                calls.append(summary[0] / 16)
                locks.append(summary[1] / 16)
        metric(row, "syscalls", calls)
        metric(row, "locks", locks)


def audit(directory):
    record = json.loads((directory / "measurement.json").read_bytes())
    assert record["schema"] == "pbrs-sb32-measurement/1"
    assert record["status"] == "captured; independent audit pending"
    assert (record["iters"], record["double_iters"], record["common_prepare_iters"], record["repeats"], record["warmup"], record["runtime_workers"], record["report_parallelism"]) == (16, 32, 32, 3, 100, 2, 1)
    assert record["syscall_scope"] == SYSCALL_SCOPE
    for path, checksum in record["source_sha256"].items():
        assert sha(git_file(record["source_commit"], path)) == checksum, f"source pin: {path}"
    build_bytes = (directory / "release-build.json").read_bytes()
    assert sha(build_bytes) == record["build_record_sha256"]
    build = json.loads(build_bytes)
    assert build["binary_sha256"] == record["binary_sha256"] and build["profile"] == "release"
    assert build["build_argv"] and "features" in build and build["tools"]
    for path, checksum in record["source_sha256"].items():
        if not path.endswith(".py"):
            assert build["source_sha256"][path] == checksum
    for path, checksum in record["schema_sha256"].items():
        assert build["schema_sha256"][path] == checksum
        assert sha((directory / "schemas" / Path(path).name).read_bytes()) == checksum
    assert set(record["schema_sha256"]) == {"third_party/protobuf/src/google/protobuf/any.proto", "third_party/protobuf/src/google/protobuf/descriptor.proto"}
    binary = Path(record["binary"])
    binary_present = binary.is_file()
    if binary_present:
        assert sha(binary.read_bytes()) == record["binary_sha256"], "immutable release binary changed"
    inventory_bytes = git_file(record["source_commit"], record["inventory_path"])
    assert sha(inventory_bytes) == record["inventory_sha256"]
    inventory = json.loads(inventory_bytes)
    frozen = json.loads(git_file(record["source_commit"], "bench/devloop/adoption/evidence/rpc-inventory.json"))
    specimens = {r["specimen"]: r["qualification"] for r in frozen}
    assert len(specimens) == 64
    expected = {f"{PREFIX}{p}.{s}.{k}" for p in PROFILES for s in specimens for k in SHAPES}
    blocked = {f"{PREFIX}{p}.{s}.{k}" for p in PROFILES for s, q in specimens.items() if not q["equal_wire_work"] for k in SHAPES}
    assert len(expected) == 512 and len(blocked) == 24
    assert set(inventory["registered_ids"]) == expected and set(inventory["blocked_ids"]) == blocked
    oracle = {r["id"]: r for r in inventory["preflight_rows"]}
    assert len(oracle) == len(inventory["preflight_rows"]) == 488 and set(oracle) == expected - blocked
    for cell, row in oracle.items():
        specimen = cell.removeprefix(PREFIX).split(".", 1)[1].rsplit(".", 1)[0]
        original = specimens[specimen]
        for field in ("request_bytes", "native_request_bytes", "response_bytes_per_message", "request_read_checksum", "response_read_checksum"):
            assert row[field] == original[field]
        assert row["responses"] == (4 if cell.endswith(".server_stream") else 1)
    assert set(record["cells"]) == expected
    raw_tool_paths = set()
    for relative, process in record["processes"].items():
        checked_process(directory, relative, process)
        for path, checksum in process.get("raw_tool_records", {}).items():
            assert sha((directory / path).read_bytes()) == checksum
            checked_process(directory, str(Path(path).parent))
            raw_tool_paths.add(path)
    actual_tool_paths = {str(p.relative_to(directory)) for p in (directory / "raw/collector").glob("*/tools/*/process.json")}
    assert raw_tool_paths == actual_tool_paths, "unindexed or lost raw tool invocation"
    assert set(record["tool_pins"]) == set(record["tools"])
    for tool, pin in record["tool_pins"].items():
        version, _, _ = checked_process(directory, pin["version_process"])
        assert version["argv"] == [record["tools"][tool], "--version"]
        executable = Path(record["tools"][tool])
        if executable.is_file():
            assert sha(executable.read_bytes()) == pin["executable_sha256"]
    _, registered_stdout, _ = checked_process(directory, "raw/registry")
    assert {line.split()[0] for line in registered_stdout.decode().splitlines() if line.startswith(PREFIX)} == expected
    old_ids = {f"rpc.adoption.{p}.{s}.{k}" for p in ("native_pbrs", "native_prost", "tonic_pbrs", "tonic_prost") for s in specimens for k in SHAPES}
    assert {line.split()[0] for line in registered_stdout.decode().splitlines() if line.startswith("rpc.adoption.")} == old_ids
    for cell, state in record["cells"].items():
        process, _, stderr = checked_process(directory, state["preflight_process"])
        assert process["argv"][1:] == ["run-cell", cell, "--iters", "1", "--prepare-iters", "32", "--warmup", "0"]
        if cell in blocked:
            assert state["status"] == state["measurement_status"] == "not_run" and process["exit_code"] != 0 and "RPC encoded bytes must agree" in stderr
        else:
            assert state["status"] == "qualified" and state["measurement_status"] == "captured" and process["exit_code"] == 0
            network(marked(stderr, "__QUALIFICATION__ "), oracle[cell])
            network(state["network"], oracle[cell])
            child = marked(stderr, "__CHILD__ ")
            assert child["cell"] == cell and child["iters"] == 1
            assert child["input_wire_fingerprint"] == oracle[cell]["input_wire_fingerprint"]
    selected_tool = "perf" if "perf" in record["tools"] else "valgrind"
    assert selected_tool in record["tools"]
    reports = {}
    expected_reports = {f"baseline-replay-{r}-{k}" for r in (1, 2) for k in SHAPES}
    expected_reports |= {f"matrix-{p}-{k}" for p in PROFILES for k in SHAPES}
    assert set(record["reports"]) == expected_reports
    for name, info in record["reports"].items():
        content = (directory / info["path"]).read_bytes()
        assert sha(content) == info["sha256"]
        report = json.loads(content)
        assert report["schema"] == "devloop/1"
        report_commit = subprocess.check_output(["git", "rev-parse", report["devloop_commit"]], cwd=ROOT, text=True).strip()
        for path, checksum in record["source_sha256"].items():
            assert sha(git_file(report_commit, path)) == checksum
        profile, shape = name.removeprefix("matrix-").split("-") if name.startswith("matrix-") else ("reference_reference", name.rsplit("-", 1)[1])
        selected = {i for i in expected - blocked if i.startswith(f"{PREFIX}{profile}.") and i.endswith(f".{shape}")}
        assert len(report["cells"]) == 61 and {r["id"] for r in report["cells"]} == selected
        parent = record["processes"][info["collector_process"]]
        assert parent["exit_code"] == 0 and parent["raw_tool_records"] == info["raw_tool_records"]
        argv = parent["argv"]
        assert argv[:3] == [record["binary"], "run", "--cells"]
        assert set(argv[3].split(",")) == selected and len(argv[3].split(",")) == 61
        assert argv[4:9] == ["--iters", "16", "--repeats", "3", "--out"]
        assert len(argv) == 10 and Path(argv[9]).name == info["path"]
        reconstruct(directory, info, report, oracle, selected_tool, record["tools"], record["binary"])
        reports[name] = {r["id"]: r for r in report["cells"]}
    replay_deltas, replay_failures = [], []
    for shape in SHAPES:
        a, b = reports[f"baseline-replay-1-{shape}"], reports[f"baseline-replay-2-{shape}"]
        for cell in a:
            for name in PRIMARY:
                first, second = value(a[cell], name), value(b[cell], name)
                if first is None or second is None:
                    replay_deltas.append({"id": cell, "metric": name, "status": "not_run"})
                    continue
                # Match the original RPC comparison in BOTH directions,
                # including its zero-baseline skip and strict >2% ceiling.
                forward = (second - first) / first if first else None
                reverse = (first - second) / second if second else None
                failed = (forward is not None and forward > .02) or (reverse is not None and reverse > .02)
                row = {"id": cell, "metric": name, "forward_delta": forward, "reverse_delta": reverse, "original_rpc_limit": .02, "failed": failed}
                replay_deltas.append(row)
                if failed:
                    replay_failures.append(row)
    comparisons = []
    for shape in SHAPES:
        baseline = reports[f"matrix-reference_reference-{shape}"]
        for profile in PROFILES[1:]:
            for cell, candidate in reports[f"matrix-{profile}-{shape}"].items():
                reference = cell.replace(f"{PREFIX}{profile}.", f"{PREFIX}reference_reference.", 1)
                ratios = {}
                for name in (*PRIMARY, "wall_ns"):
                    current, base = value(candidate, name), value(baseline[reference], name)
                    ratios[name] = {"ratio": current / base, "loss": current > base} if current is not None and base is not None and base > 0 else {"status": "not_run", "reason": "ratio requires two available positive metrics"}
                comparisons.append({"id": cell, "reference_id": reference, "ratios": ratios})
    assert len(comparisons) == 366
    matrix_rows = [row for name, rows in reports.items() if name.startswith("matrix-") for row in rows.values()]
    assert len(matrix_rows) == 488
    return {"schema": "pbrs-sb32-audit/1", "integrity": "passed", "registered": 512, "network_qualified": 488, "blocked": 24,
            "binary_rechecked": binary_present, "source_commit": record["source_commit"], "binary_sha256": record["binary_sha256"],
            "original_baseline_replay_gate": "failed" if replay_failures else ("incomplete" if any(r.get("status") == "not_run" for r in replay_deltas) else "passed"),
            "baseline_replay_deltas": replay_deltas, "baseline_replay_failures": replay_failures,
            "metric_available_cells": {metric: sum(value(row, metric) is not None for row in matrix_rows) for metric in PRIMARY},
            "comparisons": comparisons, "syscall_scope": record["syscall_scope"],
            "claim": "raw evidence integrity only; replay failures/unavailable metrics and all losses remain blocking or diagnostic under original policy"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    summary = audit(args.directory.resolve())
    if args.out:
        assert not args.out.exists(), "do not overwrite an earlier audit"
        args.out.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"SB-32 integrity passed: 512 states, 488 cell archives verified, 24 blocked, 366 comparisons; instruction availability {summary['metric_available_cells']['instructions']}/488; original replay gate {summary['original_baseline_replay_gate']}")


if __name__ == "__main__":
    main()
