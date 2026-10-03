#!/usr/bin/env python3
"""Verify archived GN03 pair inputs/artifacts without executing compiler output."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import tomllib


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--directory", type=Path, default=Path(__file__).resolve().parent)
args = parser.parse_args()
root = args.directory
manifest = json.loads((root / "members-sha256.json").read_text())
with tarfile.open(root / "raw.tar.gz", "r:gz") as archive:
    items = archive.getmembers()
    actual = {item.name: item for item in items}
    assert len(items) == len(actual), "duplicate archive member"
    assert actual.keys() == manifest.keys(), "archive/member manifest coverage differs"
    for name, pin in manifest.items():
        assert actual[name].isfile(), f"unexpected non-file member: {name}"
        assert not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts
        data = archive.extractfile(actual[name]).read()
        assert len(data) == pin["bytes"], f"size mismatch: {name}"
        assert hashlib.sha256(data).hexdigest() == pin["sha256"], f"hash mismatch: {name}"

    def data(name):
        return archive.extractfile(actual[name]).read()

    def record(name):
        return json.loads(data(name))

    pair_names = [name for name in actual if re.fullmatch(r"pair-(small|100|1000)\.json", name)]
    assert len(pair_names) == 1, "expected one paired capture"
    pair = record(pair_names[0])
    assert pair["status"] == "complete-local-diagnostic"
    assert pair["jobs"] == 1 and pair["repeats"] == 1 and pair["seed"] == 190019
    assert len(pair["runs"]) == 2 and {run["profile"] for run in pair["runs"]} == {"default", "shared"}
    bootstrap = pair["bootstrap"]
    assert bootstrap["bootstrap_excluded_from_all_cell_measurements"] is True
    assert manifest[bootstrap["binary"]]["sha256"] == bootstrap["binary_sha256"]
    assert manifest["driver/Cargo.lock"]["sha256"] == bootstrap["driver_lock_sha256"]
    assert manifest["coordinator/sb09-paired-screen.py"]["sha256"] == pair["coordinator_sha256"]
    for name, digest in pair["source_sha256"].items():
        assert manifest["source-capsule/" + name]["sha256"] == digest, f"source capsule mismatch: {name}"

    assert pair["case"] == "1000" and pair["profiles"] == ["default", "shared"]
    assert pair["source_commit"] == "795193f69e4adac3d9fc980f385d5b113f0b9aa5"
    assert len(pair["source_sha256"]) == 69
    assert bootstrap["binary_sha256"] == "718aef4131813c1c6fc449e33202588fc6bf48cc330bd28cfd33a96e056f524b"
    assert bootstrap["driver_lock_sha256"] == "f6f3382c52829e93543a6bfae35c61bfcda46e23200944df25e1968a22e14d63"
    assert pair["coordinator_sha256"] == "9146637996b827afd224de26ddf10cf0727f134c758b7ca28ae5bb17e98fb084"
    prep = record("retry-preparation.json")
    assert prep["source_worktree_commit"] == pair["source_commit"]
    assert prep["source_sha256"] == pair["source_sha256"]
    assert prep["current_main_commit"] == "4b2384e728bce17601d56e43a887c354d66b9024"
    assert set(prep["differences_against_current_main"]) == {"Cargo.lock", "src/codegen/config.rs", "src/text.rs"}
    for name, comparison in prep["differences_against_current_main"].items():
        assert comparison["historical_sha256"] == pair["source_sha256"][name]
        assert comparison["main_sha256"] != comparison["historical_sha256"]
    for name, pin in prep["prepared_artifact_sha256"].items():
        assert manifest[name] == pin, f"prepared artifact drift: {name}"
    assert prep["jobs"] == 1 and prep["phase_timeout_seconds"] == 900
    assert prep["guards"]["owned_cache_limit_bytes"] == prep["guards"]["global_free_minimum_bytes"] == 2 * 1024**3
    assert prep["latest_readiness_refresh"]["owned_bootstrap_allocated_bytes"] == 0
    assert prep["latest_readiness_refresh"]["case_and_pair_paths_absent"] is True
    assert pair["gnu_time_sha256"] == prep["gnu_time_sha256"]
    assert pair["protobuf_source"]["revision"] == prep["protobuf_source_revision"]
    assert pair["protobuf_source"]["tracked_changes"] == []
    for name, tool in pair["actual_tool_binaries"].items():
        assert tool["sha256"] == prep["actual_tools"][name]["sha256"]
    launch = record("coordinator/launch-1000-retry.json")
    exit_record = record("coordinator/exit-1000-retry.json")
    quiet = record("coordinator/quiet-window-1000-retry.json")
    assert launch["root_lease_grant_utc"] == "2026-10-03 03:42:18 UTC"
    assert launch["argv"][-1] == "--execute" and launch["argv"][-2] == pair["lease"]
    assert launch["environment_controls"]["CARGO_BUILD_JOBS"] == "1"
    assert exit_record["coordinator_exit_code"] == 0
    assert launch["actual_outer_launch_utc"] < pair["started_at_utc"] < pair["finished_at_utc"] < exit_record["actual_outer_exit_utc"]
    assert quiet["agent_stop_observation_utc"] > exit_record["actual_outer_exit_utc"]
    assert quiet["quiet_exceptions_reported_during_retry"] == []
    stdout = data("coordinator/" + launch["stdout_log"]).decode().splitlines()
    assert len(stdout) == 4
    assert stdout[0].startswith("START 1000/default ")
    assert stdout[1].startswith("FINISH 1000/default: exit 0; cache reclaimed ")
    assert stdout[2].startswith("START 1000/shared ")
    assert stdout[3].startswith("FINISH 1000/shared: exit 0; cache reclaimed ")
    assert data("coordinator/" + launch["stderr_log"]) == b""
    visibility = record("coordinator/prelaunch-process-visibility.json")
    assert visibility["visible_active_compilers"] == []
    assert len(visibility["zombie_compilers"]) == 10
    assert all(item["state"] == "Z" and item["argv"] == "" for item in visibility["zombie_compilers"])
    assert visibility["permission_limitations"] and visibility["initial_guard_refusal"]

    reports = []
    consumer_manifests = []
    results = {}
    for run in pair["runs"]:
        prefix = run["out"] + "/"
        report = record(prefix + "summary.json")
        reports.append(report)
        assert manifest[prefix + "summary.json"]["sha256"] == run["report_sha256"]
        assert run["exit_code"] == 0 and run["resource_failures"] == []
        assert report["status"] == "unqualified" and report["qualification"]["qualified"] is False
        assert "single_run_diagnostic" in report["qualification"]["reasons"]
        assert report["setup"]["generator_binary_sha256"] == bootstrap["binary_sha256"]
        assert report["environment"]["repository"]["source_sha256"] == pair["source_sha256"]
        assert len(report["cells"]) == 1
        cell = report["cells"][0]
        assert cell["case"] == pair["case"] and cell["generator"] == "pbrs" and cell["repeat"] == 0
        consumer = prefix + "cases/" + pair["case"] + "/consumer/"
        consumer_manifests.append(data(consumer + "Cargo.toml"))
        release = tomllib.loads(consumer_manifests[-1].decode())["profile"]["release"]
        assert release["opt-level"] == 3 and release["lto"] == "thin" and release["codegen-units"] == 1
        assert manifest[consumer + "Cargo.lock"]["sha256"] == cell["consumer_lock_sha256"]
        for input_pin in cell["corpus"]["inputs"]:
            name = prefix + "cases/" + pair["case"] + "/" + input_pin["path"]
            assert manifest[name]["sha256"] == input_pin["sha256"] and manifest[name]["bytes"] == input_pin["bytes"]
        generated = [name for name in actual if name.startswith(consumer + "generated/") and name.endswith(".rs")]
        assert len(generated) == cell["output"]["rust_file_count"]
        assert sum(manifest[name]["bytes"] for name in generated) == cell["output"]["rust_bytes"]
        assert cell["output"]["unchanged_generation_mtimes_preserved"] is True
        helper = cell["shared_descriptor_helper"]
        assert helper["active"] is (run["profile"] == "shared")
        if helper["active"]:
            assert manifest[consumer + "generated/" + helper["path"]]["sha256"] == helper["sha256"]
        preserved = run["preserved_binary"]
        assert manifest[preserved["path"]]["sha256"] == preserved["sha256"] == cell["release_binary"]["sha256"]
        assert manifest[preserved["path"]]["bytes"] == preserved["bytes"] == cell["release_binary"]["size_bytes"]
        assert data(preserved["path"]).startswith(b"\x7fELF"), "preserved release artifact is not an ELF"
        assert not any(name.startswith(prefix + cell["target_dir"] + "/") for name in actual)
        fingerprints = record(prefix + "fingerprint-sha256.json")
        assert len(fingerprints) == 162, "missing retained fingerprint inventory entries"
        assert all(re.fullmatch(r"[0-9a-f]{64}", digest) for digest in fingerprints.values())
        telemetry = record(prefix + "resource-telemetry.json")
        assert telemetry["samples"] and telemetry["failures"] == []
        assert all(sample["owned_cache_bytes"] <= 2 * 1024**3 and sample["global_free_bytes"] >= 2 * 1024**3 for sample in telemetry["samples"])
        phases = cell["phases"]
        assert set(phases) == {"consumer_lock", "generation", "generation_unchanged", "check_clean", "check_incremental", "build_release", "release_smoke"}
        for name, phase in phases.items():
            assert phase["status"] == "passed" and phase["exit_code"] == 0
            stdout = data(prefix + phase["stdout_log"])
            stderr = data(prefix + phase["stderr_log"])
            if name == "release_smoke":
                assert phase["output_verified"] is True and stdout == b"1\n" and stderr == b""
            else:
                assert b"Exit status: 0" in stderr
                raw_peak = int(re.search(rb"Maximum resident set size \(kbytes\): (\d+)", stderr).group(1)) * 1024
                assert raw_peak == phase["direct_command_peak_rss_bytes"]
                assert phase["peak_rss_bytes"] >= raw_peak
        metrics = {
            "output.rust_bytes": cell["output"]["rust_bytes"],
            "release_binary.size_bytes": preserved["bytes"],
        }
        for name in ("generation", "generation_unchanged", "check_clean", "check_incremental", "build_release"):
            for metric in ("elapsed_ns", "peak_rss_bytes"):
                metrics[f"{name}.{metric}"] = phases[name][metric]
        assert pair["metrics"][run["profile"]] == metrics, "pair/report metric drift"
        results[run["profile"]] = {
            "source_bytes": cell["output"]["rust_bytes"],
            "cold_check_seconds": phases["check_clean"]["elapsed_ns"] / 1e9,
            "release_seconds": phases["build_release"]["elapsed_ns"] / 1e9,
            "linked_binary_bytes": preserved["bytes"],
        }
    a, b = [report["cells"][0] for report in reports]
    assert a["corpus"] == b["corpus"] and a["consumer_lock_sha256"] == b["consumer_lock_sha256"]
    assert consumer_manifests[0] == consumer_manifests[1]
    assert reports[0]["environment"]["tools"] == reports[1]["environment"]["tools"]
    assert [run["preserved_binary"]["bytes"] for run in pair["runs"]] == [8896328, 8896328]
    assert pair["runs"][0]["preserved_binary"]["sha256"] != pair["runs"][1]["preserved_binary"]["sha256"]
    assert pair["metrics"]["shared"]["generation_unchanged.elapsed_ns"] > pair["metrics"]["default"]["generation_unchanged.elapsed_ns"]
    print(json.dumps({"verified_members": len(manifest), "case": pair["case"], "status": "unqualified historical-source diagnostic", "metrics": results, "binary_size_result": "equal", "unchanged_generation_result": "shared slower", "fingerprints": "inventories only; original file contents not retained"}, indent=2))
