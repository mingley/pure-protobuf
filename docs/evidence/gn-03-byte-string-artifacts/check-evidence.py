#!/usr/bin/env python3
"""Verify the additive GN03 ordinary proof without extracting or compiling."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import tarfile

ROOT = Path(__file__).resolve().parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(value, reason):
    if not value:
        raise AssertionError(reason)


def main():
    artifacts = json.loads((ROOT / "artifact-sha256.json").read_text())
    for name, record in artifacts.items():
        data = (ROOT / name).read_bytes()
        require(record == {"bytes": len(data), "sha256": digest(data)}, "published artifact changed: " + name)
    spec = importlib.util.spec_from_file_location("ordinary_source_helpers", ROOT / "run-ordinary.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    expected = json.loads((ROOT / "raw-member-sha256.json").read_text())
    report = json.loads((ROOT / "qualification.json").read_text())
    provenance = json.loads((ROOT / "provenance.json").read_text())
    cache_audit = json.loads((ROOT / "hash-only-cache-audit.json").read_text())
    with tarfile.open(ROOT / "raw-artifacts.tar.gz") as archive:
        actual = {member.name: member for member in archive.getmembers()}
        require(set(actual) == set(expected), "raw archive membership differs")
        for name, member in actual.items():
            require(member.isfile(), "unexpected special archive member")
            data = archive.extractfile(member).read()
            require(expected[name] == {"bytes": len(data), "sha256": digest(data)}, "raw member changed: " + name)

        def read(name):
            return archive.extractfile(actual[name]).read()

        def jread(name):
            return json.loads(read(name))

        require(jread("campaign/report.json") == report, "published/archived reports differ")
        config = jread("campaign/sealed-config.json")
        require(digest(read("campaign/sealed-config.json")) == report["sealed_config_sha256"], "sealed config differs")
        require(digest(read("campaign/run-ordinary.py")) == report["runner_sha256"], "executed runner differs")
        require((ROOT / "run-ordinary.py").read_bytes() == read("campaign/run-ordinary.py"), "published runner differs")
        require(jread("campaign/root-lease.json") == report["lease"], "literal root lease differs")
        require(digest(read("campaign/qualification-plan.json")) == config["plan_sha256"], "frozen plan differs")
        require(digest(read("preparation/baseline-codegen_shared_descriptors.rs")) == config["baseline_overlay_sha256"], "baseline test overlay differs")
        require(report["status"] == "passed" and report["default30"]["status"] == "passed", "ordinary gates not all passed")
        require(set(report["stages"]) == {s["tool"] + "-" + s["variant"] for s in config["stages"]}, "stage set differs")
        units, suites, strict_shipping, strict_profiles, negatives = 0, 0, 0, 0, 0
        for stage in config["stages"]:
            tool, variant = stage["tool"], stage["variant"]
            key = tool + "-" + variant
            stage_report = report["stages"][key]
            require(stage_report["status"] == "passed", "stage failed: " + key)
            for name, expected_sha in config["source"][variant]["sha256"].items():
                if variant == "baseline" and name == "tests/codegen_shared_descriptors.rs":
                    expected_sha = config["baseline_overlay_sha256"]
                require(digest(read("campaign/snapshots/" + key + "/" + name)) == expected_sha, "compiled snapshot changed: " + key + "/" + name)
            version = read("campaign/phases/" + key + "-tool_identity/stdout.log").decode()
            require(version.startswith(config["tools"][tool]["expected_rustc_version_prefix"]), "native rustc identity differs")
            for unit in ["hex_unit"] + (["byte_unit"] if variant == "candidate" else []):
                log = read("campaign/phases/" + key + "-" + unit + "/stdout.log").decode()
                require(re.search(r"test result: ok\. 1 passed; 0 failed;", log), "unit did not run exactly once")
                require(stage_report["gates"][unit]["status"] == "passed", "unit gate not passed")
                units += 1
            log = read("campaign/phases/" + key + "-consumer_suite/stdout.log").decode()
            require(re.search(r"test result: ok\. 7 passed; 0 failed;", log), "seven consumer tests did not pass")
            suites += 1
            if tool != "msrv185":
                require(stage_report["gates"]["shipping_clippy"]["status"] == "passed", "shipping strict gate absent")
                strict_shipping += 1
                profiles = stage_report["gates"]["consumer_clippy"]["profiles"]
                require(set(profiles) == set(config["strict_consumer_names"]) and all(v["status"] == "passed" for v in profiles.values()), "seven strict consumer profiles differ")
                strict_profiles += len(profiles)
            retirement = stage_report["gates"]["archive_and_retirement"]
            require(retirement["status"] == "passed", "completed cache was not preserved/retired")
            prefix = "campaign/retained/" + key + "/"
            inventory = jread(prefix + "all-target-files.json")
            members = jread(prefix + "retained-members.json")
            nested = read(prefix + "fixtures-fingerprints-executables.tar.gz")
            require(digest(nested) == retirement["archive_sha256"], "nested retained archive changed")
            with tarfile.open(fileobj=io.BytesIO(nested)) as retained:
                require(set(retained.getnames()) == set(members), "nested archive membership differs")
                elf_paths, fingerprint_paths, fixture_paths, unexpected = [], [], [], []
                for member in retained.getmembers():
                    data = retained.extractfile(member).read()
                    observed = {"bytes": len(data), "sha256": digest(data)}
                    require(observed == members[member.name] == {k: inventory[member.name][k] for k in ["bytes", "sha256"]}, "nested fingerprint/ELF/fixture changed")
                    if data.startswith(b"\x7fELF"):
                        elf_paths.append(member.name)
                    elif "/.fingerprint/" in member.name:
                        fingerprint_paths.append(member.name)
                    elif member.name.startswith("target/gn03-tests/"):
                        fixture_paths.append(member.name)
                    else:
                        unexpected.append(member.name)
                for name in config["strict_consumer_names"]:
                    require(retained.extractfile("target/gn03-tests/" + name + "/compile-status.txt").read().strip() == b"exit status: 0", "actual compiled consumer missing")
                    require(retained.extractfile("target/gn03-tests/" + name + "/Cargo.lock") is not None, "actual consumer lock missing")
                missing = "target/gn03-tests/" + config["consumer_expected_failure"]
                require(retained.extractfile(missing + "/compile-status.txt").read().strip() == b"exit status: 101", "expected missing-helper red not retained")
                require(b"__pbrs_shared_descriptors" in retained.extractfile(missing + "/compile.stderr.log").read(), "negative compiler diagnostic differs")
                negatives += 1
            hash_only = sorted(set(inventory) - set(members))
            require(not unexpected, "unclassified retained payload")
            require(not any("/.fingerprint/" in n or n.startswith("target/gn03-tests/") for n in hash_only), "required fingerprint/fixture payload is hash-only")
            suffix_counts = {}
            for name in hash_only:
                suffix = Path(name).suffix or "(no suffix)"
                suffix_counts[suffix] = suffix_counts.get(suffix, 0) + 1
            observed_audit = {"all_cache_file_count": len(inventory), "retained_payload_count": len(members),
                "retained_elf_count": len(elf_paths), "retained_elf_paths": sorted(elf_paths),
                "retained_fingerprint_count": len(fingerprint_paths), "retained_fingerprint_paths": sorted(fingerprint_paths),
                "retained_fixture_count": len(fixture_paths), "retained_fixture_paths": sorted(fixture_paths),
                "hash_only_file_count": len(hash_only), "hash_only_paths": hash_only,
                "hash_only_suffix_counts": dict(sorted(suffix_counts.items())),
                "hash_only_execute_mode_paths": [n for n in hash_only if inventory[n]["mode"] & 0o111],
                "unexpected_retained_payloads": unexpected}
            require(observed_audit == cache_audit["stages"][key], "explicit payload versus hash-only path/count audit differs")
            require(len(inventory) == retirement["all_target_file_count"] and len(members) == retirement["retained_payload_count"], "retirement counts differ")
            require(all(not v["visible_cache_owners"] and not v["active_compilers"] for v in [jread(prefix + "process-before.json"), jread(prefix + "process-after.json")]), "visible owner at retirement")
            if variant == "candidate":
                baseline = report["stages"][tool + "-baseline"]
                for attribute in ["consumer_locks", "consumer_main_sources", "consumer_normalized_manifests"]:
                    require(stage_report[attribute] == baseline[attribute], "matched consumer fixture differs")
        require((units, suites, strict_shipping, strict_profiles, negatives) == (9, 6, 4, 28, 6), "actual gate counts differ")

        samples = []
        phases = [name for name in actual if name.startswith("campaign/phases/") and name.endswith("/command.json")]
        for name in phases:
            command = jread(name)
            require(command["status"] == "passed" and command["exit_code"] == 0, "unexpected actual failed child")
            require(not command["owned_child_cleanup"]["visible_live_members_remaining"], "live owned child after gate")
            resource_name = name.removesuffix("command.json") + "resource.jsonl"
            rows = [json.loads(row) for row in read(resource_name).splitlines()]
            require(all(row["owned_cache_within_cap"] and row["global_free_reserve_present"] and row["within_phase_timeout"] for row in rows), "guard failure sample present")
            require(all(row["allocated_unique_inode_bytes"] <= 2147483648 and row["global_available_bytes"] >= 2147483648 for row in rows), "resource guard numeric bounds differ")
            summary = command["sample_summary"]
            require(summary["samples"] == len(rows) and summary["maximum_owned_allocated_bytes"] == max(r["allocated_unique_inode_bytes"] for r in rows) and summary["minimum_global_available_bytes"] == min(r["global_available_bytes"] for r in rows) and not summary["guard_failures"], "phase sample summary differs")
            samples.extend(rows)
        expected_summary = {"samples": len(samples), "maximum_owned_allocated_bytes": max(r["allocated_unique_inode_bytes"] for r in samples), "minimum_global_available_bytes": min(r["global_available_bytes"] for r in samples), "guard_failure_sample_count": 0, "scope": "raw per-phase sampled ledger only; no continuous cap or between-sample maximum certification"}
        require(report["resource_sample_summary"] == expected_summary, "campaign sample summary differs")

        defaults = report["default30"]
        require(defaults["default_generated_file_count"] == 30, "default corpus is not30 files")
        for case, count in config["default_expected_files"].items():
            result = defaults["cases"][case]
            require(result["default_files"] == count and result["status"] == "passed", "corpus result differs")
            outputs = {}
            for label in result["files"]:
                response = read("campaign/phases/source30-" + case + "-" + label + "/stdout.log")
                outputs[label] = helper.parse_response(response)
                observed = {n: {"bytes": len(v), "sha256": digest(v)} for n, v in outputs[label].items()}
                require(observed == result["files"][label], "generated source inventory differs")
                for name, value in outputs[label].items():
                    require(read("campaign/phases/source30-" + case + "-" + label + "/generated/" + name) == value, "response/emitted generated bytes differ")
            require(outputs["archived-default"] == outputs["baseline-default"] == outputs["candidate-default"], "three-way default byte equality differs")
            for name, value in outputs["archived-default"].items():
                require(read("campaign/legacy-inputs/work/source-volume/" + case + "/before/" + name) == value, "legacy golden default differs")
            base, candidate = outputs["baseline-shared"], outputs["candidate-shared"]
            require(set(base) == set(candidate) and len(base) == count + 1, "shared file set differs")
            for name in base:
                if name != helper.HELPER:
                    require(base[name] == candidate[name], "shared application source differs")
            base_body, base_bytes = helper.owner_parts(base[helper.HELPER], "array")
            cand_body, cand_bytes = helper.owner_parts(candidate[helper.HELPER], "byte-string")
            fds = read("campaign/legacy-inputs/work/source-volume/" + case + "/fixture.fds")
            blobs = [value for n, wire, value in helper.fields(fds) if n == 1 and wire == 2]
            canonical = b"".join(helper.field(1, blob) for blob in sorted(blobs, key=lambda blob: next(v for n, w, v in helper.fields(blob) if n == 1 and w == 2)))
            require(base_body == cand_body and base_bytes == cand_bytes == canonical and digest(canonical) == result["fds_sha256"], "pool body or independent canonical descriptor bytes differ")
            for label, output in outputs.items():
                owners = {"array": sum(v.count(helper.OWNER + b"&[") for v in output.values()), "byte-string": sum(v.count(helper.OWNER + b'b"') for v in output.values())}
                require(owners == result["owner_counts"][label], "raw owner counters differ")
                require(owners == {"array": 0 if label == "candidate-shared" else 1 if label == "baseline-shared" else count - 1, "byte-string": int(label == "candidate-shared")}, "owner predicate counts aliases")
        for variant in ["baseline", "candidate"]:
            generator = report["stages"]["stable-" + variant]["generator"]
            require(digest(read("campaign/generators/" + variant + "-protoc-gen-pbrs")) == generator["sha256"], "genuine copied plugin changed")
            require(generator["source_commit"] == config["source"][variant]["commit"], "matched plugin source point differs")
        harness = read("campaign/phases/stable-candidate-harness_contracts/stderr.log").decode()
        require("Ran 66 tests" in harness and "OK (skipped=1)" in harness, "harness test/skip count differs")
        require(provenance["performance_status"] == "not_run" and provenance["parent_GN03_acceptance"] == "open", "scope claims exceed ordinary proof")
    print(json.dumps({"status": "passed", "raw_members": len(expected), "unit_tests": units, "seven_test_suites": suites, "actual_successful_consumers": 42, "expected_missing_helper_reds": negatives, "strict_shipping": strict_shipping, "strict_consumer_profiles": strict_profiles, "default_outputs": 30, "resource_samples": len(samples)}, sort_keys=True))


if __name__ == "__main__":
    main()
