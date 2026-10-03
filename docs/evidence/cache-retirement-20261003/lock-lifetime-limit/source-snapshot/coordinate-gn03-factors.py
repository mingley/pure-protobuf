"""Execute one root-leased frozen GN03 factor trial, bootstrap, or verified retirement."""
from pathlib import Path
import argparse
import json
import os
import shutil
import signal
import stat
import sys
import tomllib

from factor_common import *


def registry_tuples(lock):
    d = tomllib.loads(Path(lock).read_text())
    return sorted([p["name"], p["version"], p["source"], p["checksum"]]
                  for p in d["package"] if "source" in p)


def check_prepared(prepared_path, resources=None):
    p = load_json(prepared_path)
    seal = load_json(Path(prepared_path).with_name("preparation-seal.json"))
    require(sha(prepared_path, resources) == seal["prepared_sha256"], "prepared input seal changed")
    require(p["source_pair"] == SOURCES and p["consumer_runtime_source"] == SOURCES["base"], "source pair/runtime changed")
    for variant in SOURCES:
        require(len(p["source_sha256"][variant]) == 1027, "shipping source input count changed")
        source_pins(p["roots"][variant], p["source_sha256"][variant], resources)
        require(set(p["source_git_modes"][variant]) == set(p["source_sha256"][variant]), "shipping source modes set changed")
        source_modes(p["roots"][variant], p["source_git_modes"][variant])
        driver = p["drivers"][variant]; root = Path(driver["root"])
        for name, key in (("Cargo.toml", "manifest_sha256"), ("Cargo.lock", "lock_sha256"), ("src/main.rs", "source_sha256")):
            require(sha(root / name, resources) == driver[key], "driver input changed: " + name)
    for name, pin in p["inputs"].items():
        require(sha(pin["path"], resources) == pin["sha256"] and Path(pin["path"]).stat().st_size == pin["bytes"], "seeded input changed: " + name)
    require(sha(p["ordinary_report"]["path"], resources) == p["ordinary_report"]["sha256"], "ordinary report changed")
    return p


def schedule_for(stage, case):
    schedule = load_json(Path(__file__).with_name("schedule-proposal.json"))
    if stage == "screen":
        rows = next(x for x in schedule["screen"] if x["case"] == case)
        return [{"case": case, "cell": factor, "repeat": 0} for factor in rows["sealed_order"]]
    require(stage == "repeated", "unknown measurement stage")
    return [x for x in schedule["full"] if x["case"] == case]


def verify_lease(args, prepared):
    require(args.execute and args.root_lease, "execution requires an explicit root lease")
    lease = load_json(args.root_lease)
    require(lease.get("schema") == "gn03-factor-root-lease/1" and lease.get("authorized_by") == "/root"
            and lease.get("status") == "granted", "no granted root lease")
    require(lease["operation"] == args.operation and lease["campaign"] == str(Path(args.campaign).absolute()), "lease operation/campaign mismatch")
    require(lease["prepared_sha256"] == sha(args.prepared), "lease prepared input pin mismatch")
    require(lease["coordinator_sha256"] == sha(__file__) and lease["common_sha256"] == sha(Path(__file__).with_name("factor_common.py")), "lease runner pin mismatch")
    require(lease["components_sha256"] == component_pins(), "lease executable helper inputs changed")
    require(lease["schedule_sha256"] == sha(Path(__file__).with_name("schedule-proposal.json")), "lease schedule mismatch")
    require(lease["source_pair"] == SOURCES and lease["consumer_runtime_source"] == SOURCES["base"], "lease source scope changed")
    require(lease["jobs"] == 1 and lease["cache_cap_bytes"] == CAP and lease["free_floor_bytes"] == FLOOR,
            "original jobs/resource controls changed")
    if args.operation == "bootstrap":
        require(lease["version"] == args.version and lease["kind"] == "ordinary_bootstrap", "bootstrap lease mismatch")
    elif args.operation == "measure":
        require(lease["kind"] == "exclusive_quiet_LTO" and lease["case"] == args.case and
                lease["stage"] == args.stage and lease["trial_index"] == args.trial_index, "quiet trial lease mismatch")
        require(lease["repeats"] == (1 if args.stage == "screen" else 5) and lease["seed"] == 190019
                and lease["timeout_seconds"] == 900 and lease["rss_sample_ms"] == 100, "frozen workload controls changed")
        require(args.driver_campaign and lease["driver_campaign"] == str(Path(args.driver_campaign).absolute()),
                "immutable driver BUILD namespace not sealed by root")
    else:
        require(lease["kind"] == "preservation_after_quiet_release", "retirement is outside timed quiet phases")
        require(lease["trial_record"] == str(Path(args.trial_record).absolute()), "retirement trial mismatch")
    return lease


def tools_for(args, lease):
    require(args.tools and sha(args.tools) == lease["tool_envelope_sha256"], "qualified tool envelope not sealed by root")
    record = load_json(args.tools)
    require(record["status"] == "qualified" and record.get("closure_limits") is not None, "tool closure not qualified/disclosed")
    verify_tools(record["tools"])
    require(str(Path(sys.executable).resolve()) == record["tools"]["python"]["resolved_path"], "actual Python selection changed")
    return record


def setup_path(env, run_dir, tools):
    path = Path(run_dir) / "bin"
    path.mkdir()
    (path / "protoc").symlink_to(tools["protoc"]["path"])
    env["PATH"] = str(path) + os.pathsep + env.get("PATH", "")
    require(shutil.which("ps", path=env["PATH"]) == tools["ps"]["path"], "actual frozen ps selection changed")
    require(str((path / "protoc").resolve()) == tools["protoc"]["resolved_path"], "build-script protoc selection changed")


def no_other_cache(prepared, campaign, permitted=None):
    candidates = [Path(v["root"]) / "target/bootstrap" for v in prepared["drivers"].values()]
    root = Path(campaign)
    if root.exists():
        candidates += list(root.glob("cells/*/*/r*/cases/*/target-r*"))
    existing = [str(p) for p in candidates if p.exists() and (permitted is None or p != permitted)]
    require(not existing, "single active cache violated: " + repr(existing))


def ready_disk(archive_allowance):
    require(archive_allowance >= 0, "negative preservation allowance")
    v = os.statvfs("/workspace")
    free = v.f_bavail * v.f_frsize
    require(free >= CAP + FLOOR + archive_allowance,
            "insufficient free space for original cache cap/reserve plus approved preservation allowance")
    return free


def observe_phases(h, observer):
    # Scope is just this freshly imported harness module, never the global stdlib module.
    require(h.subprocess is sys.modules["subprocess"], "unexpected existing subprocess wrapper")
    h.subprocess = observer


def generated_oracle(prepared, case, factor, generated):
    actual = inventory(generated)
    expected = prepared["output_pins"][case][factor]
    require(actual == expected, "cg19 generated bytes differ from exact request/default30 oracle")
    fds = Path(prepared["inputs"][f"work/source-volume/{case}/fixture.fds"]["path"]).read_bytes()
    import re
    prefix = b"pub const FILE_DESCRIPTOR_SET: &[u8] = "
    arrays = strings = 0
    for name in actual:
        data = (Path(generated) / name).read_bytes()
        for match in re.finditer(re.escape(prefix), data):
            tail = data[match.end():]
            if tail.startswith(b"&["):
                end = tail.index(b"];")
                literal = tail[2:end]
                require(re.fullmatch(rb"(?:\s|0x[0-9a-f]{2},)*", literal), "invalid raw array owner")
                payload = bytes(int(x, 16) for x in re.findall(rb"0x([0-9a-f]{2}),", literal)); arrays += 1
            elif tail.startswith(b'b"'):
                end = tail.index(b'";', 2)
                literal = tail[2:end]
                require(re.fullmatch(rb"(?:\\x[0-9a-f]{2})*", literal), "invalid raw byte-string owner")
                payload = bytes(int(x, 16) for x in re.findall(rb"\\x([0-9a-f]{2})", literal)); strings += 1
            else:
                continue  # Alias has no raw embedded bytes.
            require(payload == fds, "generated owner changed canonical FDS bytes")
    files = {"small": 2, "100": 5, "1000": 20}[case]
    expected_counts = (files, 0) if factor.endswith("def") else ((1, 0) if factor == "bshr" else (0, 1))
    require((arrays, strings) == expected_counts, "raw owner count changed")
    return {"status": "passed", "files": actual, "array_owners": arrays, "byte_string_owners": strings,
            "canonical_fds_sha256": digest_bytes(fds), "ordinary_report_sha256": prepared["ordinary_report"]["sha256"]}


def collect_raw(run_dir, report):
    raw = {}
    for cell in report["cells"]:
        for phase in cell["phases"].values():
            for key in ("stdout_log", "stderr_log"):
                if key in phase:
                    p = Path(run_dir) / phase[key]
                    require(p.is_file(), "raw phase output absent")
                    raw[phase[key]] = {"bytes": p.stat().st_size, "sha256": sha(p)}
    return raw


def bootstrap(args, p, lease, tool_record):
    root = Path(p["drivers"][args.version]["root"])
    target = root / "target/bootstrap"
    no_other_cache(p, args.campaign)
    ready_disk(lease["preservation_allowance_bytes"])
    run = Path(args.campaign) / "bootstrap" / args.version
    run.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(args.tools, run / "tool-envelope.json")
    shutil.copyfile(args.root_lease, run / "root-lease.json")
    observer = ProcessObserver(run / "processes.json")
    h = load_harness(p["roots"][args.version]); observe_phases(h, observer)
    env, cleared = clean_env(tool_record["tools"], "default")
    setup_path(env, run, tool_record["tools"])
    argv = [tool_record["tools"]["cargo"]["path"], "build", "--offline", "--locked", "--manifest-path",
            str(root / "Cargo.toml"), "--target-dir", str(target), "--bin", "cg19-generator"]
    report = {"cells": [], "setup": {}}
    record = {"schema": "gn03-factor-BUILD/1", "status": "running", "source": SOURCES[args.version],
        "version": args.version, "source_scope": "source-matched cg19 debug driver; excluded consumer setup",
        "prepared_sha256": sha(args.prepared), "tools_sha256": sha(args.tools), "lease_sha256": sha(args.root_lease),
        "argv": argv, "environment": effective_env(env), "cleared_codegen_env_keys": cleared,
        "started_utc": utc(), "target": str(target), "run_dir": str(run),
        "closure_limits": tool_record["closure_limits"]}
    record["components_sha256"] = component_pins()
    path = run / "BUILD.json"; write_json(path, record)
    try:
        with ResourceGuard(target, "/workspace", observer, run / "resource.json"):
            h.phase(report, report["setup"], "driver_build", argv, Path(p["roots"][args.version]), env,
                    run, 900, 100, run / "logs/driver-build")
        require(all(not live_group(owner) for owner in observer.owners), "completed bootstrap left a live owned child group")
        check_prepared(args.prepared); verify_tools(tool_record["tools"])
        binary = target / "debug/cg19-generator"
        require(binary.read_bytes()[:4] == b"\x7fELF", "selected cg19 artifact is not ELF")
        retained = Path(args.campaign) / "bin" / ("cg19-generator-" + args.version)
        retained.parent.mkdir(exist_ok=True); require(not retained.exists(), "driver artifact already exists")
        shutil.copyfile(binary, retained); retained.chmod(0o555)
        require(sha(binary) == sha(retained), "actual selected cg19 copy differs")
        record.update(status="passed", artifact={"path": str(retained), "sha256": sha(retained), "bytes": retained.stat().st_size},
                      phase=report["setup"]["driver_build"], manifest_sha256=sha(root / "Cargo.toml"), lock_sha256=sha(root / "Cargo.lock"))
    except BaseException as exc:
        record.update(status="failed", error=str(exc)); observer.blocked.set(); observer.cleanup()
        raise
    finally:
        record["finished_utc"] = utc(); write_json(path, record)
    return path


def measure(args, p, lease, tool_record):
    rows = schedule_for(args.stage, args.case)
    require(0 <= args.trial_index < len(rows), "trial index outside sealed schedule")
    trial = rows[args.trial_index]; factor = trial["cell"]; rep = trial["repeat"]
    version, profile = FACTORS[factor]
    require(lease["factor"] == factor and lease["repeat"] == rep, "lease trial factor/repeat mismatch")
    no_other_cache(p, args.campaign)
    for index, prior in enumerate(rows):
        previous = Path(args.campaign) / "cells" / args.case / prior["cell"] / f"r{prior['repeat']}"
        if index < args.trial_index:
            require(load_json(previous / "RUN.json")["status"] == "passed_local_unqualified"
                    and load_json(previous / "preservation/preservation.json")["status"] == "verified_retired",
                    "earlier sealed trial incomplete/unretired")
        else:
            require(not previous.exists(), "later/current trial already exists; refusing favorable retake or reorder")
    ready_disk(lease["preservation_allowance_bytes"])
    run = Path(args.campaign) / "cells" / args.case / factor / f"r{rep}"
    run.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(args.tools, run / "tool-envelope.json")
    shutil.copyfile(args.root_lease, run / "root-lease.json")
    h = load_harness(p["consumer_ROOT"])
    build_path = Path(args.driver_campaign) / "bootstrap" / version / "BUILD.json"
    build = load_json(build_path)
    require(build["status"] == "passed" and build["source"] == SOURCES[version]
            and build["prepared_sha256"] == sha(args.prepared) and build["tools_sha256"] == sha(args.tools),
            "matched driver BUILD missing/incorrect")
    shutil.copyfile(build_path, run / "driver-BUILD.json")
    binary = Path(build["artifact"]["path"])
    require(sha(binary) == build["artifact"]["sha256"], "immutable driver changed")
    observer = ProcessObserver(run / "processes.json"); observe_phases(h, observer)
    env, cleared = clean_env(tool_record["tools"], profile); setup_path(env, run, tool_record["tools"])
    report = {"schema_version": "cg19/1", "status": "pending", "setup": {}, "cells": [],
              "repeats": 1 if args.stage == "screen" else 5,
              "qualification": {"qualified": False, "reasons": ["historical_factor_pair_local_diagnostic"]}}
    case_dir = run / "cases" / args.case
    target = case_dir / f"target-r{rep}"
    record = {"schema": "gn03-factor-RUN/1", "status": "running", **trial, "factor": factor,
        "stage": args.stage, "trial_index": args.trial_index, "source_pair": SOURCES,
        "generator_source": SOURCES[version], "consumer_runtime_source": SOURCES["base"],
        "profile": profile, "prepared_sha256": sha(args.prepared), "tools_sha256": sha(args.tools),
        "lease_sha256": sha(args.root_lease), "driver_BUILD_sha256": sha(build_path),
        "driver_artifact": build["artifact"], "harness_sha256": HARNESS_SHA,
        "run_dir": str(run), "target": str(target), "schedule_sha256": sha(Path(__file__).with_name("schedule-proposal.json")),
        "environment": effective_env(env), "cleared_codegen_env_keys": cleared, "started_utc": utc(),
        "jobs": 1, "seed": 190019, "timeout_seconds": 900, "rss_sample_ms": 100,
        "cold_target_initially_absent": not target.exists(), "qualified": False}
    record["components_sha256"] = component_pins()
    path = run / "RUN.json"; write_json(path, record)
    try:
        with clean_parent_codegen_env():
            names, corpus = h.prepare_corpus(case_dir, args.case, 190019, ("pbrs",))
        require(sha(case_dir / "consumer/src/main.rs") == p["consumer_main_sha256"][args.case]["0"], "original marker0 consumer changed")
        for name in names:
            pin = p["inputs"][f"work/source-volume/{args.case}/proto/{name}"]
            require(sha(case_dir / "consumer/proto" / name) == pin["sha256"], "frozen seeded proto differs")
        # Reject an offline regenerated lock drift before allowing its later measurement phases.
        original_finished = observer.finished
        def finished(owner, code):
            original_finished(owner, code)
            if owner["phase"] == "consumer_lock" and code == 0:
                require(registry_tuples(case_dir / "consumer/Cargo.lock") == p["consumer_locks"][args.case]["registry_tuples"],
                        "generated consumer registry tuples differ from sealed shipping pins")
        observer.finished = finished
        with ResourceGuard(target, "/workspace", observer, run / "resource.json"):
            h.measure_pbrs_cell(report, args.case, names, corpus, rep, case_dir, binary,
                                tool_record["tools"]["cargo"]["path"], tool_record["tools"]["protoc"]["path"],
                                env, run, 900, 100, None, False)
        require(all(not live_group(owner) for owner in observer.owners), "completed phase left a live owned child group")
        check_prepared(args.prepared); verify_tools(tool_record["tools"])
        require(sha(binary) == build["artifact"]["sha256"], "driver changed during cell")
        require(sha(case_dir / "consumer/src/main.rs") == p["consumer_main_sha256"][args.case]["1"], "original marker1 consumer changed")
        oracle = generated_oracle(p, args.case, factor, case_dir / "consumer/generated")
        write_json(run / "generated-oracle.json", oracle)
        require(len(report["cells"]) == 1, "helper cell count changed")
        h.compute_matrix(report); report["status"] = "unqualified"; h.write_report(report, run)
        record.update(status="passed_local_unqualified", raw_outputs=collect_raw(run, report),
            summary_sha256=sha(run / "summary.json"), oracle_sha256=sha(run / "generated-oracle.json"),
            processes_sha256=sha(run / "processes.json"), resource_sha256=sha(run / "resource.json"),
            metrics=h.matrix_metrics(report["cells"][0]), consumer_lock_sha256=sha(case_dir / "consumer/Cargo.lock"))
    except BaseException as exc:
        record.update(status="failed", error=str(exc)); observer.blocked.set(); observer.cleanup()
        raise
    finally:
        record["finished_utc"] = utc(); write_json(path, record)
    return path


def retire(args, p, lease):
    from retirement_adapter import retire_full_cache
    record_path = Path(args.trial_record); record = load_json(record_path)
    cache = Path(record["target"])
    require(cache.is_dir() and not cache.is_symlink(), "completed owned cache absent/symlink")
    campaign = Path(args.campaign).absolute()
    allowed = [Path(d["root"]) / "target/bootstrap" for d in p["drivers"].values()]
    allowed += list(campaign.glob("cells/*/*/r*/cases/*/target-r*"))
    require(cache in allowed and record_path.parent == Path(record["run_dir"]), "unowned retirement scope")
    require(record["status"] in ("passed", "passed_local_unqualified", "failed"), "cache still running")
    process_file = record_path.parent / "processes.json"
    def owners_idle():
        for owner in load_json(process_file)["owners"]:
            require(not live_group(owner), "recorded phase group is still live")
    no_other_cache(p, args.campaign, cache)
    preservation = retire_full_cache(cache, record_path.parent / "preservation", owners_idle,
                                    lambda resources: check_prepared(args.prepared, resources),
                                    {"root_lease_sha256": sha(args.root_lease)})
    return record_path.parent / "preservation/preservation.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepared", required=True)
    parser.add_argument("--campaign", required=True)
    parser.add_argument("--operation", choices=("bootstrap", "measure", "retire"), required=True)
    parser.add_argument("--tools")
    parser.add_argument("--root-lease")
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--version", choices=tuple(SOURCES))
    parser.add_argument("--driver-campaign")
    parser.add_argument("--case", choices=("small", "100", "1000"))
    parser.add_argument("--stage", choices=("screen", "repeated"))
    parser.add_argument("--trial-index", type=int)
    parser.add_argument("--trial-record")
    args = parser.parse_args()
    p = check_prepared(args.prepared)
    if not args.execute:
        print(json.dumps({"status": "source_ready_execution_not_run", "prepared_sha256": sha(args.prepared),
                          "operation": args.operation, "numeric_gates": "not_run"})); return 0
    lease = verify_lease(args, p)
    def cancelled(sig, _frame):
        raise KeyboardInterrupt("root-leased coordinator interrupted by signal" + str(sig))
    signal.signal(signal.SIGTERM, cancelled)
    signal.signal(signal.SIGINT, cancelled)
    tools = tools_for(args, lease)
    if args.operation == "retire":
        path = retire(args, p, lease)
    else:
        path = bootstrap(args, p, lease, tools) if args.operation == "bootstrap" else measure(args, p, lease, tools)
    verify_tools(tools["tools"]); verify_lease(args, p)
    print(json.dumps({"record": str(path), "operation": args.operation, "performance_qualified": False})); return 0


if __name__ == "__main__":
    try:
        require(sys.dont_write_bytecode, "Run with python -B to preserve source projection bytes")
        raise SystemExit(main())
    except (ValueError, OSError, KeyboardInterrupt) as exc:
        print("GN03 factor execution refused/failed: " + str(exc), file=sys.stderr)
        raise SystemExit(1)
