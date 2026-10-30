#!/usr/bin/env python3
"""Verify capsule digests and source bindings without extracting or running it."""

import hashlib
from fnmatch import fnmatchcase
import io
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
PREFIX = "work/readiness-qa-20261030/"
ROOT_AT_RUN = PurePosixPath("/workspace/pure-protobuf")
BASE_TEST = ["cargo", "test", "--workspace", "--all-features", "--locked"]
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_capsule():
    manifest = json.loads((HERE / "manifest.json").read_text())
    assert manifest["schema"] == "pbrs.transport-readiness-capsule.v1"
    assert manifest["qualified_production_profile"] is False
    # All archive paths are data. No member is extracted or executed.
    data = (HERE / "raw.tar.gz").read_bytes()
    assert sha(data) == manifest["archive_sha256"], "archive digest"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        members = archive.getmembers()
        assert len(members) == len(manifest["files"]), "member count"
        assert all(m.isfile() and m.size < 8 * 1024 * 1024 for m in members)
        assert all(not PurePosixPath(m.name).is_absolute()
                   and ".." not in PurePosixPath(m.name).parts for m in members)
        contents = {m.name: archive.extractfile(m).read() for m in members}
    assert len(contents) == len(members), "duplicate member"
    assert set(contents) == set(manifest["files"]), "inventory"
    for path, data in contents.items():
        entry = manifest["files"][path]
        assert len(data) == entry["bytes"] and sha(data) == entry["sha256"], path
    return manifest, contents


def reconstruct_final_source(manifest, contents):
    record = json.loads(contents["source-inputs.json"])
    assert record["base_commit"] == manifest["base_commit"]
    inputs = record["inputs_sha256"]
    repository = subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"], cwd=HERE, text=True
    ).strip()
    command = ["git", "archive", "--format=tar", manifest["base_commit"], "--"]
    data = subprocess.check_output(command + sorted(inputs), cwd=repository)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as archive:
        source = {m.name: archive.extractfile(m).read()
                  for m in archive.getmembers() if m.isfile()}
    for path, overlay in record["overlays"].items():
        assert path in inputs and overlay["sha256"] == inputs[path], path
        assert overlay["archive_path"] == "source-overlay/" + path, path
        if overlay["base_sha256"] is None:
            assert path not in source, path
        else:
            assert sha(source[path]) == overlay["base_sha256"], path
        data = contents[overlay["archive_path"]]
        assert sha(data) == overlay["sha256"], path
        source[path] = data
    assert set(source) == set(inputs), "reconstructed source inventory"
    assert all(sha(source[p]) == digest for p, digest in inputs.items()), "source hashes"
    return inputs


def verify_final_labels(manifest, contents, inputs):
    labels = manifest["final_labels"]
    assert labels and len(labels) == len(set(labels))
    totals = []
    for label in labels:
        assert label and all(c.isalnum() or c in "-_" for c in label), label
        prefix = "work/readiness-qa-20261030/" + label
        report = json.loads(contents[prefix + ".json"])
        log = contents[prefix + ".log"]
        assert report["base_commit"] == manifest["base_commit"], label
        if label == "partitions-final":
            assert report["schema"] == "pbrs.workspace-partitions-observation.v1", label
            assert report["observation_only"] is True and report["runner_status"] == "passed", label
            assert report["wrapper_exit_status"] == "not recorded after environment handover", label
            assert "exit_code" not in report and "elapsed_seconds" not in report, label
            assert report["coverage_sha256"] == sha(contents[PREFIX + "partitions-coverage.json"]), label
        else:
            assert report["exit_code"] == 0, label
        assert report["source_inputs_unchanged"] is True, label
        assert report["log_sha256"] == sha(log), label
        assert report["command"] and report["rustc"], label
        assert report["inputs_sha256"] and all(
            inputs[path] == digest for path, digest in report["inputs_sha256"].items()
        ), label
        results = re.findall(rb"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", log)
        assert all(int(failed) == 0 for _, failed, _ in results), label
        if "test" in report["command"]:
            assert results and sum(int(passed) for passed, _, _ in results) > 0, label
        totals.append((label, sum(int(passed) for passed, _, _ in results)))
    return totals


def suite_results(data, known, docs=False, allow_interrupted=False):
    """Parse retained Cargo output independently of the archived runner."""
    active, results, interrupted = None, {}, []
    for line in data.decode().splitlines():
        line = ANSI.sub("", line)
        match = re.match(r"^\s+Doc-tests (\S+)\s*$", line) if docs else re.match(r"^\s+Running .+ \((.+)\)\s*$", line)
        if match:
            if active is not None:
                interrupted.append(active)
            value = match.group(1)
            active = value if docs else str(ROOT_AT_RUN / value)
            assert active in known, ("unknown executed suite", active)
            continue
        match = re.match(r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;", line)
        if match:
            assert active is not None, "unassociated suite result"
            status, passed, failed, ignored, measured, filtered = match.groups()
            row = {"status": status, "passed": int(passed), "failed": int(failed),
                   "ignored": int(ignored), "measured": int(measured), "filtered": int(filtered)}
            assert status == "ok" and int(failed) == int(measured) == int(filtered) == 0, (active, row)
            assert active not in results, ("duplicate suite", active)
            results[active] = row
            active = None
    if active is not None:
        interrupted.append(active)
    assert allow_interrupted or not interrupted, interrupted
    return results, interrupted


def artifact_records(data, members):
    rows = {}
    for line in data.decode().splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (record.get("reason") != "compiler-artifact" or record.get("package_id") not in members
                or not record.get("executable") or not record.get("profile", {}).get("test")):
            continue
        name = record["executable"]
        row = {k: record[k] for k in ("package_id", "target", "profile", "features", "fresh")}
        row["executable"] = name
        assert name not in rows or rows[name] == row, ("conflicting Cargo artifact", name)
        rows[name] = row
    return rows


def verify_partitions(contents, inputs):
    """Check target union, feature graph, commands and counts without running code."""
    report = json.loads(contents[PREFIX + "partitions-coverage.json"])
    assert report["schema"] == "pbrs.workspace-partitions.v1" and report["status"] == "passed"
    assert report["inputs_sha256"] == inputs
    metadata = json.loads(contents[PREFIX + "partitions-metadata.json"])
    members = set(metadata["workspace_members"])
    inventory = report["inventory"]
    assert inventory and all(row["target"]["kind"] in (["lib"], ["bin"], ["test"]) for row in inventory.values())
    inventory_records = artifact_records(contents[PREFIX + "partitions-artifacts.log"], members)
    assert set(inventory_records) == set(inventory)
    for name, row in inventory.items():
        assert {k: value for k, value in row.items() if k != "binary_sha256"} == inventory_records[name], name
        assert re.fullmatch(r"[0-9a-f]{64}", row["binary_sha256"]), name
    prior_name = report["prior_report"]
    assert prior_name == "native-retry-final.json"
    prior = json.loads(contents[PREFIX + prior_name])
    prior_log = "native-retry-final.log"
    assert sha(contents[PREFIX + prior_name]) == report["prior_report_sha256"]
    assert sha(contents[PREFIX + prior_log]) == report["prior_log_sha256"] == prior["log_sha256"]
    assert prior["inputs_sha256"] == inputs and prior["source_inputs_unchanged"] is True
    assert prior["exit_code"] == 99 and prior["command"][:2] == BASE_TEST[:2]
    assert sorted(prior["command"][2:]) == sorted(BASE_TEST[2:])
    assert prior["rustc"] == report["rustc"]
    results, interrupted = suite_results(contents[PREFIX + prior_log], inventory, allow_interrupted=True)
    assert interrupted == report["prior_interrupted"]
    coverage = {name: dict(row, origin=prior_log, binary_sha256=inventory[name]["binary_sha256"])
                for name, row in results.items() if inventory[name]["fresh"] is True}
    assert len(coverage) == report["reused_targets"]
    missing = {name: row for name, row in inventory.items() if name not in coverage}
    groups = []
    if any(row["target"]["kind"] == ["lib"] for row in missing.values()):
        groups.append(["--lib"])
    if any(row["target"]["kind"] == ["bin"] for row in missing.values()):
        groups.append(["--bins"])
    groups += [["--test", name] for name in sorted({row["target"]["name"] for row in missing.values()
                                                     if row["target"]["kind"] == ["test"]})]
    assert groups == report["planned_groups"]
    observed_groups, completed_names, doctests = [], set(), None
    for number, command in enumerate(report["commands"]):
        log_name = command["log"]
        assert log_name not in completed_names and PurePosixPath(log_name).name == log_name
        completed_names.add(log_name)
        assert command == json.loads(contents[PREFIX + log_name.removesuffix(".log") + ".json"])
        assert command["exit_code"] == 0 and command["source_inputs_unchanged"] is True
        data = contents[PREFIX + log_name]
        assert sha(data) == command["log_sha256"]
        args = command["command"]
        if args == BASE_TEST + ["--no-run", "--message-format=json"]:
            assert number == 0 and log_name == "partitions-artifacts.log"
            continue
        if args == ["cargo", "clean", "--target-dir", str(ROOT_AT_RUN / "target/integration-consumers")]:
            assert number > 0 and report["commands"][number - 1]["command"][:len(BASE_TEST)] == BASE_TEST
            assert report["commands"][number - 1]["command"][-1] == "--message-format=json"
            continue
        if args == BASE_TEST + ["--doc"]:
            assert number == len(report["commands"]) - 1 and doctests is None
            names = {target["name"] for package in metadata["packages"] if package["id"] in members
                     for target in package["targets"] if target["kind"] == ["lib"] and target.get("doctest")}
            doctests, unfinished = suite_results(data, names, docs=True)
            assert names and set(doctests) == names and not unfinished
            continue
        assert args[:len(BASE_TEST)] == BASE_TEST and args[-1] == "--message-format=json"
        group = args[len(BASE_TEST):-1]
        assert len(observed_groups) < len(groups) and group == groups[len(observed_groups)]
        observed_groups.append(group)
        expected = {name for name, row in inventory.items()
                    if (group == ["--lib"] and row["target"]["kind"] == ["lib"])
                    or (group == ["--bins"] and row["target"]["kind"] == ["bin"])
                    or (group[0] == "--test" and row["target"]["kind"] == ["test"] and row["target"]["name"] == group[1])}
        assert expected
        actual, unfinished = suite_results(data, inventory)
        assert set(actual) == expected and not unfinished
        artifacts = artifact_records(data, members)
        assert expected <= artifacts.keys()
        for name, row in actual.items():
            for field in ("package_id", "target", "profile", "features"):
                assert artifacts[name][field] == inventory[name][field], (name, field)
            if name in coverage:
                assert {key: coverage[name][key] for key in row} == row, name
            coverage[name] = dict(row, origin=log_name, binary_sha256=inventory[name]["binary_sha256"])
    assert observed_groups == groups and doctests is not None
    assert set(coverage) == set(inventory) and coverage == report["coverage"]
    assert doctests == report["doctests"]
    totals = {"native_targets": len(coverage),
              "native_passed": sum(row["passed"] for row in coverage.values()),
              "native_ignored": sum(row["ignored"] for row in coverage.values()),
              "doctest_passed": sum(row["passed"] for row in doctests.values()),
              "doctest_ignored": sum(row["ignored"] for row in doctests.values())}
    assert all(report[key] == value for key, value in totals.items())
    observation = json.loads(contents[PREFIX + "partitions-final.json"])
    assert observation["inputs_sha256"] == inputs and observation["rustc"] == report["rustc"]
    return totals


def verify_history(contents):
    for kind in ("queue", "rejected", "provenance"):
        prefix = "work/ready-" + kind
        report = json.loads(contents[prefix + "-report.json"])
        assert sha(contents[prefix + ".patch"]) == report["patch_sha256"], kind
    interrupted = json.loads(contents["work/ready-provenance-5-interrupted.json"])
    assert interrupted["claim"] == "incomplete, not passed"
    broad = json.loads(contents["work/ready-provenance-5-streams.json"])
    assert broad["exit"] != 0
    lint = json.loads(contents["work/ready-test-lint-report.json"])
    assert sha(contents["work/ready-test-lint.patch"]) == lint["patch_sha256"]
    for path, data in contents.items():
        if not path.endswith(".json") or path == "source-inputs.json":
            continue
        record = json.loads(data)
        if isinstance(record, dict) and "log_sha256" in record:
            log = ("work/review30/miri-lib-runtime.log"
                   if path == "work/review30/miri-lib-runtime-result.json"
                   else path.removesuffix(".json") + ".log")
            assert sha(contents[log]) == record["log_sha256"], path


def verify_cold_cache_records(contents):
    cases = {
        "adapters_build_cold_from_descriptor_sets_without_protoc":
            "pbrs-onboarding-adapters-noprotoc-*/build",
        "packed_core_and_both_adapters_build_cold_without_protoc":
            "pbrs-onboarding-cold-packed-*/cold-build",
    }
    prefix = "/workspace/pure-protobuf/target/"
    paths = [p for p in contents if p.endswith(".cold-cache.json")]
    assert paths, "retain the completed cold-cache guard record"
    for path in paths:
        records = json.loads(contents[path])
        log = contents[path.removesuffix(".cold-cache.json") + ".log"].decode()
        assert isinstance(records, list), path
        for record in records:
            case = record["finished_case"]
            assert case in cases and f"test {case} ... ok" in log, (path, case)
            command = record["command"]
            assert len(command) == 4 and command[:3] == ["cargo", "clean", "--target-dir"], path
            assert command[3].startswith(prefix), path
            target = command[3].removeprefix(prefix)
            assert len(PurePosixPath(target).parts) == 2 and ".." not in PurePosixPath(target).parts, path
            assert fnmatchcase(target, cases[case]) and record["exit_code"] == 0, path


def main():
    manifest, contents = read_capsule()
    inputs = reconstruct_final_source(manifest, contents)
    totals = verify_final_labels(manifest, contents, inputs)
    partition_totals = verify_partitions(contents, inputs)
    verify_history(contents)
    verify_cold_cache_records(contents)
    print(f"Verified {len(contents)} archive members, {len(inputs)} final source inputs "
          f"and {len(totals)} final check labels; interrupted broad Miri remains incomplete.")
    for label, passed in totals:
        if label == "partitions-final":
            print(f"  {label}: recovered partition coverage {partition_totals}; wrapper exit not recorded")
        else:
            print(f"  {label}: exit 0; {passed} Rust tests reported passed")


if __name__ == "__main__":
    main()
