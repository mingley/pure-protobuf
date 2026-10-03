"""Independent retained-byte/coverage/factor auditor; success never means GN03 adoption."""
from pathlib import Path
import argparse
import json
import math
import statistics
import sys
import tarfile
import tomllib

from factor_common import (ALL_PHASES, CAP, FACTORS, FLOOR, HARNESS_SHA, PHASES, SOURCES, UNSUPPORTED_ENV,
                           digest_bytes, inventory, load_json, ordinary_file, require, safe_relative,
                           sha, source_modes, source_pins, write_json)


def metric_values(cell):
    result = {"output.rust_bytes": cell["output"]["rust_bytes"],
              "release_binary.size_bytes": cell["release_binary"]["size_bytes"]}
    for phase in PHASES:
        for key in ("elapsed_ns", "peak_rss_bytes"):
            value = cell["phases"][phase][key]
            require(isinstance(value, int) and not isinstance(value, bool) and value > 0, "missing/invalid original metric")
            result[phase + "." + key] = value
    require(all(isinstance(v, int) and not isinstance(v, bool) and v > 0 for v in result.values()), "invalid source/ELF size")
    return result


def verify_preservation(run, cache, release_binary, verify_archive=False):
    path = Path(run) / "preservation/preservation.json"
    if not path.exists():
        require(Path(cache).is_dir(), "missing cache and missing preservation proof")
        return {"status": "awaiting_verified_retirement"}
    p = load_json(path)
    require(p["status"] == "verified_retired" and p["cache"] == str(cache) and not Path(cache).exists(), "cache retirement not complete")
    root = path.parent
    require(sha(root / "all-files.json") == p["all_files_sha256"] and sha(root / "payloads.json") == p["payloads_sha256"], "preservation manifest changed")
    all_files = load_json(root / "all-files.json"); payloads = load_json(root / "payloads.json")
    require(p["inventory_files"] == len(all_files) and p["retained_payloads"] == len(payloads), "retention counts differ")
    require(p["hash_only_cache_files"] == 0 and p["mode_ownership_content_link_topology_pass"] is True,
            "full topology preservation qualification missing")
    manifest_path = root / "manifest.json"
    require(sha(manifest_path) == p["manifest_sha256"], "full topology manifest changed")
    manifest = load_json(manifest_path)
    require(all(row["disposition"] == "archived" for row in manifest.values()), "full cache member omitted")
    for name, row in manifest.items():
        if row["kind"] == "file":
            require(all_files.get(name) == {"bytes": row["identity"]["st_size"], "sha256": row["sha256"]}, "topology/content manifests differ")
    require(all(all_files.get(k) == v for k, v in payloads.items()), "payload is not original cache inventory")
    for name, info in all_files.items():
        if any(x in Path(name).parts for x in ("deps", ".fingerprint", "build")):
            require(payloads.get(name) == info, "required full deps/fingerprint/build payload omitted")
    member = Path(release_binary["path"]).relative_to(cache).as_posix()
    require(payloads.get(member) == {"bytes": release_binary["size_bytes"], "sha256": release_binary["sha256"]}, "actual measured ELF missing from retained payload")
    if verify_archive:
        from retirement_adapter import primitives
        import os
        archive = root / "payloads.tar.gz"
        class ReadResources:
            def check(self, _phase, write_bytes=0, force=False):
                v = os.statvfs("/workspace"); free = v.f_bavail * v.f_frsize
                require(free >= FLOOR + write_bytes, "archive audit reserve below2GiBfloor")
                return free
        verification = primitives().verify_archive(archive, manifest, ReadResources())
        require(verification["archive_sha256"] == p["archive_sha256"] and verification["verified_members"] == p["archive_members"],
                "archive content/mode/link/member verification differs")
    return {"status": "verified_retired", "archive_members_reverified": verify_archive}


def audit_run(run, prepared, expected, prepared_sha, verify_archive=False):
    run = Path(run); record = load_json(run / "RUN.json")
    factor = expected["cell"]; version, profile = FACTORS[factor]
    require(record["schema"] == "gn03-factor-RUN/1" and record["status"] == "passed_local_unqualified"
            and record["qualified"] is False, "failed/incomplete trial or qualification override")
    require(record["factor"] == factor and record["repeat"] == expected["repeat"] and record["case"] == expected["case"], "trial coverage identity changed")
    require(record["source_pair"] == SOURCES and record["generator_source"] == SOURCES[version]
            and record["consumer_runtime_source"] == SOURCES["base"], "trial source scope changed")
    require(record["prepared_sha256"] == prepared_sha and record["harness_sha256"] == HARNESS_SHA, "source/harness input closure changed")
    require(record["profile"] == profile and record["jobs"] == 1 and record["seed"] == 190019
            and record["timeout_seconds"] == 900 and record["rss_sample_ms"] == 100
            and record["cold_target_initially_absent"] is True, "original workload/resource settings changed")
    require(record["run_dir"] == str(run), "run path identity changed")
    require(sha(run / "tool-envelope.json") == record["tools_sha256"] and
            sha(run / "driver-BUILD.json") == record["driver_BUILD_sha256"] and
            sha(run / "root-lease.json") == record["lease_sha256"], "tool/BUILD/root lease sidecar changed")
    tool_record = load_json(run / "tool-envelope.json")
    require(tool_record["status"] == "qualified" and tool_record.get("closure_limits") is not None,
            "tool provenance closure qualification/limits missing")
    tools = tool_record["tools"]
    build = load_json(run / "driver-BUILD.json")
    require(build["status"] == "passed" and build["source"] == SOURCES[version]
            and build["artifact"] == record["driver_artifact"] and build["prepared_sha256"] == prepared_sha
            and build["tools_sha256"] == record["tools_sha256"], "matched source/ELF/tool BUILD missing")
    require(sha(build["artifact"]["path"]) == build["artifact"]["sha256"], "immutable cg19 driver changed")
    for filename, key in (("summary.json", "summary_sha256"), ("generated-oracle.json", "oracle_sha256"),
                          ("processes.json", "processes_sha256"), ("resource.json", "resource_sha256")):
        require(sha(ordinary_file(run / filename)) == record[key], "trial sidecar changed: " + filename)
    summary = load_json(run / "summary.json")
    require(summary["status"] == "unqualified" and summary["qualification"]["qualified"] is False
            and len(summary["cells"]) == 1, "original report failed/changed qualification")
    cell = summary["cells"][0]
    require(cell["case"] == expected["case"] and cell["repeat"] == expected["repeat"] and cell["generator"] == "pbrs", "original cell metadata changed")
    resolved = cell["profile"]["resolved"]
    require(resolved == {"emit_reflection": True, "emit_json": True, "emit_text": True,
                        "shared_descriptor_set": profile == "shared", "runtime_profile": "default", "stubs": "none"}, "existing profile knobs changed")
    require(set(cell["phases"]) == set(ALL_PHASES), "original seven phase coverage changed")
    cache = run / "cases" / expected["case"] / f"target-r{expected['repeat']}"
    require(record["target"] == str(cache), "cold target path changed")
    consumer = cache.parent / "consumer"
    cargo = tools["cargo"]["path"]; protoc = tools["protoc"]["path"]
    args = ["--offline", "--locked", "--manifest-path", str(consumer / "Cargo.toml"),
            "--target-dir", str(cache), "--bin", "cg19-consumer-" + expected["case"]]
    names = [f"part_{i:02d}.proto" for i in range({"small": 2, "100": 5, "1000": 20}[expected["case"]])]
    generation = [record["driver_artifact"]["path"], str(consumer / "proto"), str(consumer / "generated"), protoc, *names]
    expected_argv = {"consumer_lock": [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
                     "generation": generation, "generation_unchanged": generation,
                     "check_clean": [cargo, "check", *args], "check_incremental": [cargo, "check", *args],
                     "build_release": [cargo, "build", "--release", *args],
                     "release_smoke": [str(cache / "release" / ("cg19-consumer-" + expected["case"]))]}
    for name, phase in cell["phases"].items():
        require(phase["command"] == expected_argv[name], "frozen actual phase argv changed: " + name)
    for name in names:
        pin = prepared["inputs"][f"work/source-volume/{expected['case']}/proto/{name}"]
        require(sha(consumer / "proto" / name) == pin["sha256"], "actual schema input differs from frozen archive")
    require(sha(consumer / "src/main.rs") == prepared["consumer_main_sha256"][expected["case"]]["1"], "consumer default-value/marker workload changed")
    manifest = tomllib.loads((consumer / "Cargo.toml").read_text())
    require(set(manifest) == {"package", "workspace", "dependencies", "profile"}
            and manifest["dependencies"] == {"pbrs": {"path": prepared["consumer_ROOT"]}}
            and manifest["profile"] == {"release": {"opt-level": 3, "lto": "thin", "codegen-units": 1}},
            "fixed consumer/runtime/original release profile changed")
    raw_names = set()
    for name, phase in cell["phases"].items():
        require(phase["status"] == "passed" and phase["exit_code"] == 0, "actual phase is not passed: " + name)
        for key in ("stdout_log", "stderr_log"):
            path = phase[key]; safe_relative(path); raw_names.add(path)
    require(set(record["raw_outputs"]) == raw_names, "raw log coverage changed")
    for name, info in record["raw_outputs"].items():
        path = ordinary_file(run / safe_relative(name))
        require(path.stat().st_size == info["bytes"] and sha(path) == info["sha256"], "raw log bytes changed")
    smoke = cell["phases"]["release_smoke"]
    require((run / smoke["stdout_log"]).read_bytes() == b"1\n" and (run / smoke["stderr_log"]).read_bytes() == b""
            and smoke["output_verified"] is True, "actual smoke output changed")
    require(cell["output"]["unchanged_generation_mtimes_preserved"] is True, "unchanged generation rewrote outputs")
    oracle = load_json(run / "generated-oracle.json")
    require(oracle["status"] == "passed" and oracle["files"] == prepared["output_pins"][expected["case"]][factor], "output differs from frozen exact-request oracle")
    generated = run / "cases" / expected["case"] / "consumer/generated"
    require(inventory(generated) == oracle["files"], "retained generated bytes changed")
    require(cell["output"]["rust_bytes"] == sum(x["bytes"] for x in oracle["files"].values()), "source metric differs from retained bytes")
    fds = prepared["inputs"][f"work/source-volume/{expected['case']}/fixture.fds"]
    require(oracle["canonical_fds_sha256"] == fds["sha256"] and sha(fds["path"]) == fds["sha256"], "canonical FDS changed")
    original = metric_values(cell)
    require(original == record["metrics"], "derived metrics differ from original report")
    owner_rows = load_json(run / "processes.json")["owners"]
    require(len(owner_rows) == 7 and {x["phase"] for x in owner_rows} == set(ALL_PHASES), "actual child session coverage changed")
    for owner in owner_rows:
        phase = cell["phases"][owner["phase"]]
        argv = phase["command"] if owner["phase"] == "release_smoke" else ["/usr/bin/time", "-v", *phase["command"]]
        require(owner["argv"] == argv and owner["exit_code"] == phase["exit_code"] and owner["pgid"] == owner["pid"]
                and owner["sid"] == owner["pid"] and owner.get("finished_utc"), "actual phase argv/group/exit changed")
        env = owner["environment"]
        require(env["CARGO_BUILD_JOBS"] == "1" and env["CARGO_NET_OFFLINE"] == "true", "actual child jobs/offline changed")
        require(env.get("SB09_PBRS_SHARED_DESCRIPTOR_SET") == ("1" if profile == "shared" else None), "actual child profile changed")
        require(env["RUSTC"] == tools["rustc"]["path"] and env["PROTOC"] == protoc, "actual compiler/protoc selection changed")
        require(not any(env.get(k) for k in UNSUPPORTED_ENV)
                and not any(k.startswith("CARGO_PROFILE_") for k in env), "actual compiler/profile override added")
        require(not any(v for k, v in env.items() if k.startswith(("CARGO_TARGET_", "CC_", "CXX_", "AR_", "LD_"))
                        and k != "CARGO_TARGET_DIR"), "actual target/compiler-specific override added")
        require(not any(v is not None for k, v in env.items() if k.startswith("SB09_PBRS_") and k != "SB09_PBRS_SHARED_DESCRIPTOR_SET"),
                "actual common emission/profile mask changed")
        require(owner["cwd"] == (str(consumer) if owner["phase"] == "release_smoke" else prepared["consumer_ROOT"]), "actual phase cwd changed")
        if owner["phase"] in ("check_clean", "check_incremental", "build_release"):
            require(env["CARGO_INCREMENTAL"] == ("0" if owner["phase"] == "build_release" else "1"), "actual incremental setting changed")
    resource = load_json(run / "resource.json")
    require(not resource["failures"] and resource["samples"] and resource["cap_bytes"] == CAP and resource["floor_bytes"] == FLOOR,
            "resource guard failed/missing")
    require(all(s["allocated_cache_bytes"] <= CAP and s["global_free_bytes"] >= FLOOR for s in resource["samples"]), "resource bounds exceeded")
    binary = cell["release_binary"]
    absolute_binary = {**binary, "path": str(run / safe_relative(binary["path"]))}
    retention = verify_preservation(run, cache, absolute_binary, verify_archive)
    if cache.exists():
        path = ordinary_file(absolute_binary["path"])
        require(path.stat().st_size == binary["size_bytes"] and sha(path) == binary["sha256"], "actual selected release ELF bytes changed")
        require(path.read_bytes()[:4] == b"\x7fELF", "selected release artifact is not ELF")
    return {"case": expected["case"], "factor": factor, "repeat": expected["repeat"], "metrics": original,
            "retention": retention, "started_utc": record["started_utc"], "finished_utc": record["finished_utc"]}


def derive_contrasts(rows):
    grouped = {factor: [] for factor in FACTORS}
    for row in rows:
        grouped[row["factor"]].append(row)
    require(all(grouped.values()), "factor coverage incomplete")
    medians = {factor: {m: statistics.median(r["metrics"][m] for r in values)
                       for m in values[0]["metrics"]} for factor, values in grouped.items()}
    result = []
    for after, before, label in (("cdef", "bdef", "candidate_at_default"), ("cshr", "bshr", "candidate_at_shared"),
                                 ("bshr", "bdef", "sharing_at_baseline"), ("cshr", "cdef", "sharing_at_candidate")):
        for metric in medians[before]:
            a, b = medians[after][metric], medians[before][metric]
            result.append({"contrast": label, "metric": metric, "before_median": b, "after_median": a,
                           "relative_delta": a / b - 1, "loses": a > b, "equal": a == b})
    interaction = {m: (medians["cshr"][m] / medians["bshr"][m]) /
                      (medians["cdef"][m] / medians["bdef"][m]) - 1 for m in medians["bdef"]}
    return {"medians": medians, "contrasts": result, "interaction_descriptive_only": interaction}


def audit_campaign(prepared_path, campaign, case, stage, verify_archive=False):
    p = load_json(prepared_path); prepared_sha = sha(prepared_path)
    require(load_json(Path(prepared_path).with_name("preparation-seal.json"))["prepared_sha256"] == prepared_sha, "prepared outer seal changed")
    require(p["source_pair"] == SOURCES and p["consumer_runtime_source"] == SOURCES["base"], "historical source pair changed")
    for version in SOURCES:
        require(len(p["source_sha256"][version]) == 1027, "source input projection count changed")
        source_pins(p["roots"][version], p["source_sha256"][version])
        require(set(p["source_git_modes"][version]) == set(p["source_sha256"][version]), "shipping source modes set changed")
        source_modes(p["roots"][version], p["source_git_modes"][version])
    schedule = load_json(Path(__file__).with_name("schedule-proposal.json"))
    if stage == "screen":
        block = next(x for x in schedule["screen"] if x["case"] == case)
        expected = [{"case": case, "cell": factor, "repeat": 0} for factor in block["sealed_order"]]
    else:
        require(stage == "repeated", "unknown qualification stage")
        expected = [x for x in schedule["full"] if x["case"] == case]
    trials = list(Path(campaign).glob(f"cells/{case}/*/r*/RUN.json"))
    expected_paths = {Path(campaign) / "cells" / case / e["cell"] / f"r{e['repeat']}" / "RUN.json" for e in expected}
    require(set(trials) == expected_paths, "duplicate/missing/extra factor/repeat coverage")
    rows = [audit_run(path.parent, p, e, prepared_sha, verify_archive) for e in expected
            for path in [Path(campaign) / "cells" / case / e["cell"] / f"r{e['repeat']}" / "RUN.json"]]
    require(all(rows[i]["finished_utc"] <= rows[i + 1]["started_utc"] for i in range(len(rows) - 1)), "sealed trial order/serial execution changed")
    require(all(r["retention"]["status"] == "verified_retired" for r in rows), "full campaign contains unretired cache")
    return {"schema": "gn03-factor-audit/1", "status": "complete_local_unqualified", "qualified": False,
            "case": case, "stage": stage, "trial_count": len(rows), "source_pair": SOURCES,
            "consumer_runtime_source": SOURCES["base"], "raw_rows": rows, **derive_contrasts(rows),
            "adoption": "not_claimed; original size/check fall, parent controls and independent-host qualification remain separate"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepared", required=True); parser.add_argument("--campaign", required=True)
    parser.add_argument("--case", choices=("small", "100", "1000"), required=True)
    parser.add_argument("--stage", choices=("screen", "repeated"), required=True)
    parser.add_argument("--verify-archives", action="store_true")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    try:
        result = audit_campaign(args.prepared, Path(args.campaign).absolute(), args.case, args.stage, args.verify_archives)
    except (ValueError, OSError, KeyError, TypeError) as exc:
        write_json(args.out, {"status": "failed_or_incomplete", "qualified": False, "error": str(exc)})
        print("GN03 factor audit failed: " + str(exc), file=sys.stderr); return 1
    write_json(args.out, result)
    print(json.dumps({"status": result["status"], "trial_count": result["trial_count"], "qualified": False})); return 0


if __name__ == "__main__":
    if not sys.dont_write_bytecode:
        print("Run with python -B", file=sys.stderr); raise SystemExit(1)
    raise SystemExit(main())
