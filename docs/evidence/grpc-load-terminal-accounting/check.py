#!/usr/bin/env python3
"""Verify retained endpoint diagnostics without executing archived binaries."""

import hashlib
import importlib.util
import itertools
import json
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parent
VALIDATOR = Path(__file__).resolve().parents[3] / "scripts/current-h2-soak.py"


def main():
    index = json.loads((ROOT / "index.json").read_text())
    archive = ROOT / "records.tar.gz"
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == index["archive_sha256"]
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        assert len(members) == len(index["files"])
        data = {}
        for member in members:
            assert member.isfile() and member.name in index["files"]
            assert not Path(member.name).is_absolute() and ".." not in Path(member.name).parts
            raw = tar.extractfile(member).read()
            assert hashlib.sha256(raw).hexdigest() == index["files"][member.name]
            data[member.name] = raw
        assert len(data) == len(index["files"])
    before = json.loads(data["before/reproduction.json"])
    after = json.loads(data["after/reproduction.json"])
    for name, pin in [("before-load-fix", before), ("after-load-fix", after)]:
        assert hashlib.sha256(data[name]).hexdigest() == pin["binary_sha256"]
    for pin, code, numeric_code, panic in [(before, 0, 101, True), (after, 1, 2, False)]:
        assert len(pin["records"]) == 6
        for record in pin["records"][:3]:
            assert record["exit_code"] == code and record["failed"] > 0 and record["successful"] == 0
        for record in pin["records"][3:]:
            assert record["exit_code"] == numeric_code and record["panicked"] is panic
    report = json.loads(data["matrix/report.json"])
    assert report["passed"] and report["binary_unchanged"]
    assert report["binary_sha256"] == after["binary_sha256"]
    assert report["qualification"]["qualified"] is False and report["source_verified"] is False
    pairs = [("native", "pbrs", "native", "pbrs"), ("native", "pbrs", "tonic", "prost"),
             ("tonic", "prost", "native", "pbrs"), ("tonic", "prost", "tonic", "prost")]
    expected = set(itertools.product(pairs, ["unary", "server_stream", "client_stream", "bidi"],
                                     [0, 1024, 65536, 1048576], [False, True], ["identity", "gzip"]))
    observed = set()
    successes = 0
    assert len(report["runs"]) == len(report["cells"]) == len(expected)
    for planned, entry in zip(report["cells"], report["runs"]):
        run = json.loads(data["matrix/" + entry["path"]])
        assert entry["passed"] and run["passed"] and run["client_exit_code"] == 0
        assert run["server_exit_before_teardown"] is None and run["cell"] == planned
        cell = run["cell"]
        key = (tuple(cell["pair"]), cell["shape"], cell["payload_bytes"], cell["tls"], cell["compression"])
        assert key not in observed
        observed.add(key)
        metrics = run["metrics"]
        n = metrics["successful_rpcs"]
        assert type(n) is int and n > 0
        assert all(metrics[name] == n for name in ["offered_rpcs", "dispatched_rpcs", "completed_rpcs"])
        assert all(metrics[name] == 0 for name in ["failed_rpcs", "timeouts", "queue_overflows", "unstarted_rpcs", "unfinished_rpcs"])
        assert metrics["status_errors"] == {}
        path = "matrix/" + str(Path(entry["path"]).parent) + "/metrics.json"
        assert json.loads(data[path]) == metrics
        samples = run["resource_samples"]
        assert samples and samples[-1]["client"]["state"] == "Z"
        ticks = report["clock_ticks_per_second"]
        end = samples[-1]["client"]
        assert run["client_lifetime_cpu_seconds"] == (end["user_ticks"] + end["system_ticks"]) / ticks
        start, end = run["server_cpu_before"], samples[-1]["server"]
        assert run["server_cpu_during_client_seconds"] == sum(end[k] - start[k] for k in ["user_ticks", "system_ticks"]) / ticks
        successes += n
    assert observed == expected
    resource = json.loads(data["resource/report.json"])
    validator = VALIDATOR
    assert validator.read_bytes() == data["source/current/scripts/current-h2-soak.py"]
    spec = importlib.util.spec_from_file_location("current_h2_soak", validator)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.validate_report(resource) == []
    assert resource["smoke"]["status"] == "passed" and resource["exit_code"] == 0
    assert resource["qualification"]["qualified"] is False
    assert resource["qualification"]["soak_24h"]["status"] == "not_run"
    assert resource["source"]["dirty"] is False and resource["source"]["commit"] == index["resource_source"]
    assert resource["duration_requested_seconds"] == 30 and resource["duration_actual_seconds"] >= 30
    assert resource["process_limits"] == resource["process_limits_requested"]
    for limit in resource["process_limits"].values():
        assert 0 < limit["soft"] <= limit["hard"]
    assert resource["events"][0]["phase"] == "baseline"
    drains = [event for event in resource["events"] if event["phase"] == "drain"]
    assert drains
    for event in drains:
        assert all(event[key] == 0 for key in ["server_allocated_bytes", "client_allocated_bytes", "server_byte_tokens", "client_byte_tokens", "observed_streaming_calls_active"])
    print(f"Verified {len(members)} artifact hashes, {len(observed)} endpoint cells, {successes} successful RPCs, and {len(drains)} recovery cycles.")


if __name__ == "__main__":
    main()
