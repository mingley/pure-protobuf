#!/usr/bin/env python3
"""Work-only GN03 paired screen: unchanged SB09 cells, archived before reclamation."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import threading
import time


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("gn03_sb09", ROOT / "bench/codegen/run.py")
h = importlib.util.module_from_spec(spec)
spec.loader.exec_module(h)
CACHE_LIMIT = 2 * 1024**3
FREE_MINIMUM = 2 * 1024**3
ORDER = {"small": ("default", "shared"), "100": ("shared", "default"), "1000": ("default", "shared")}


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


def allocated_bytes(paths):
    seen = set()
    total = 0
    for path in paths:
        if not path.exists():
            continue
        for parent, _, names in os.walk(path.resolve()):
            for name in names:
                try:
                    st = (Path(parent) / name).stat()
                except FileNotFoundError:
                    continue
                key = (st.st_dev, st.st_ino)
                if key not in seen:
                    seen.add(key)
                    total += st.st_blocks * 512
    return total


def stop_own_timed_commands():
    # timed_command launches /usr/bin/time in its own new process group.
    # Only direct children of THIS coordinator with their own group qualify.
    for path in Path("/proc").glob("[0-9]*/stat"):
        try:
            rest = path.read_text().rsplit(")", 1)[1].split()
            pid = int(path.parent.name)
            if int(rest[1]) == os.getpid() and int(rest[2]) == pid:
                os.killpg(pid, signal.SIGTERM)
        except (FileNotFoundError, ProcessLookupError):
            continue


def bootstrap_once(campaign, bootstrap, base_env):
    record = campaign / "bootstrap.json"
    if record.exists():
        saved = json.loads(record.read_text())
        binary = campaign / saved["binary"]
        if saved["source_sha256"] != h.source_hashes() or saved["binary_sha256"] != h.sha256(binary):
            raise RuntimeError("common bootstrap source/artifact drift")
        return binary, saved
    details = h.provenance(campaign, 1, 100)
    report = {
        "schema_version": "cg19/1", "status": "pending", "environment": details,
        "setup": {}, "cells": [], "qualification": {"qualified": False, "reasons": ["bootstrap_only"]},
    }
    driver = campaign / "driver"
    h.write_text(driver / "src/main.rs", (ROOT / "bench/codegen/generator.rs").read_text())
    h.write_text(driver / "Cargo.toml", h.manifest("cg19-generator"))
    env = {
        **base_env, "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never",
        "CARGO_BUILD_JOBS": "1", "CARGO_TARGET_DIR": str(bootstrap), "LC_ALL": "C",
        "RUSTC": details["tools"]["rustc"]["executable"],
        "PROTOC": details["tools"]["protoc"]["executable"],
    }
    for name, command in [
        ("driver_lock", [details["tools"]["cargo"]["executable"], "generate-lockfile", "--offline",
                         "--manifest-path", str(driver / "Cargo.toml")]),
        ("driver_build", [details["tools"]["cargo"]["executable"], "build", "--offline", "--locked",
                          "--manifest-path", str(driver / "Cargo.toml"), "--target-dir", str(bootstrap),
                          "--bin", "cg19-generator"]),
    ]:
        h.phase(report, report["setup"], name, command, ROOT, env, campaign, 900, 100,
                campaign / "logs/setup" / name)
    binary, digest = h.copy_generator(bootstrap / "debug/cg19-generator", campaign)
    saved = {
        "source_sha256": details["repository"]["source_sha256"],
        "binary": h.relative(binary, campaign), "binary_sha256": digest,
        "driver_lock_sha256": h.sha256(driver / "Cargo.lock"),
        "bootstrap_excluded_from_all_cell_measurements": True,
        "resolved_target": str(bootstrap.resolve()),
    }
    if saved["source_sha256"] != h.source_hashes():
        raise RuntimeError("source changed during common bootstrap")
    report["status"] = "unqualified"
    h.write_report(report, campaign)
    write_json(record, saved)
    return binary, saved


def measure_one(out, case, profile, binary, bootstrap_record):
    out.mkdir(parents=True)
    details = h.provenance(out, 1, 100)
    selected_profile = h.pbrs_profile(dict(os.environ))
    report = {
        "schema_version": "cg19/1", "status": "pending", "started_at_utc": h.utc_now(),
        "seed": h.DEFAULT_SEED, "cases_requested": [case], "environment": details,
        "setup": {
            "generator_binary_path": os.path.relpath(binary, out),
            "generator_binary_sha256": bootstrap_record["binary_sha256"],
            "common_bootstrap": "../bootstrap.json",
        }, "cells": [], "generators": ["pbrs"], "stub_generators": ["pbrs-native"], "repeats": 1,
        "pbrs_profiles": {"pbrs": selected_profile}, "errors": [],
        "reference": {"status": "missing", "reason": "GN03 local default/shared pair; no independent reference"},
        "comparison": {"status": "not_run", "losing_cells": None},
        "qualification": {
            "qualified": False,
            "reasons": ["no_equivalent_reference_peer", "single_run_diagnostic", "partial_corpus_matrix"]
                       + (["nondefault_pbrs_profile_diagnostic"] if profile == "shared" else []),
        },
    }
    base_env = {
        **os.environ, "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never",
        "CARGO_BUILD_JOBS": "1", "LC_ALL": "C",
        "RUSTC": details["tools"]["rustc"]["executable"],
        "PROTOC": details["tools"]["protoc"]["executable"],
    }
    try:
        names, corpus = h.prepare_corpus(out / "cases" / case, case, h.DEFAULT_SEED, ("pbrs",))
        h.measure_pbrs_cell(report, case, names, corpus, 0, out / "cases" / case, binary,
                            details["tools"]["cargo"]["executable"], details["tools"]["protoc"]["executable"],
                            base_env, out, 900, 100, None, False)
        if details["repository"]["source_sha256"] != h.source_hashes():
            raise RuntimeError("source changed during measured cell")
        if h.sha256(binary) != bootstrap_record["binary_sha256"]:
            raise RuntimeError("common generator changed during measured cell")
        h.compute_matrix(report)
        report["status"] = "unqualified"
        return 0
    except Exception as exc:
        report["status"] = "error"
        report["errors"].append(str(exc))
        report["qualification"]["reasons"].append("incomplete_measurement")
        return 1
    finally:
        report["finished_at_utc"] = h.utc_now()
        h.write_report(report, out)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--case", choices=list(ORDER), required=True)
parser.add_argument("--campaign", default="gn03-paired-screen")
parser.add_argument("--lease", help="root's declared quiet lease, retained in campaign provenance")
parser.add_argument("--execute", action="store_true")
args = parser.parse_args()
if Path(args.campaign).name != args.campaign or args.campaign in (".", ".."):
    parser.error("campaign must be one directory name")
plan = {
    "case": args.case, "profiles": list(ORDER[args.case]), "jobs": 1, "repeats": 1,
    "seed": h.DEFAULT_SEED, "generators": ["pbrs"],
    "release": "original opt-level3/thin-LTO/codegen-units1; same target after check",
    "cold_targets": "distinct initially nonexistent per-profile targets; never bootstrap",
    "qualification": "local single-pair diagnostic; no thresholds or workload changes",
}
if not args.execute:
    print(json.dumps(plan, indent=2))
    raise SystemExit(0)
if not args.lease:
    parser.error("execute requires the granted root quiet lease to be recorded")

campaign = ROOT / "target/codegen-bench" / args.campaign
result_path = campaign / f"pair-{args.case}.json"
if result_path.exists():
    parser.error(f"pair evidence already exists: {result_path}")
bootstrap = ROOT / "target/integration-consumers"
base = ROOT / "target/base"
if not base.is_dir():
    raise RuntimeError(f"missing approved reused base target: {base}")
if bootstrap.exists() or bootstrap.is_symlink():
    if bootstrap.resolve() != base.resolve():
        raise RuntimeError("unexpected bootstrap target; do not overwrite another cache")
else:
    bootstrap.symlink_to(base, target_is_directory=True)

result = {
    **plan, "lease": args.lease, "source_commit": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True,
    ).strip(), "source_sha256": h.source_hashes(),
    "coordinator_sha256": h.sha256(Path(__file__)),
    "bootstrap_resolved": str(bootstrap.resolve()),
    "original_environment": {
        key: value for key, value in os.environ.items()
        if key.startswith(("CARGO_", "RUST", "SB09_", "PURE_PROTOBUF_"))
    }, "gnu_time_sha256": h.sha256(Path("/usr/bin/time")),
    "actual_tool_binaries": {
        name: {"path": str(path), "sha256": h.sha256(path)}
        for name, path in {
            **{
                name: Path(subprocess.check_output(["rustup", "which", name], text=True).strip())
                for name in ("cargo", "rustc")
            },
            "python": Path(sys.executable).resolve(),
            "protoc": Path(shutil.which("protoc")).resolve(),
        }.items()
    },
    "protobuf_source": {
        "revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT / "third_party/protobuf", text=True,
        ).strip(),
        "tracked_changes": subprocess.check_output(
            ["git", "status", "--porcelain", "--untracked-files=no"],
            cwd=ROOT / "third_party/protobuf", text=True,
        ).splitlines(),
    },
    "resource_sampling": "every5sec filesystem-allocated bytes + global free; owned timed groups terminated on budget breach",
    "started_at_utc": h.utc_now(), "runs": [], "status": "pending",
}
write_json(result_path, result)
original_env = os.environ.copy()
try:
    common_env = {
        key: value for key, value in original_env.items()
        if not key.startswith(("SB09_PBRS_", "PURE_PROTOBUF_"))
    }
    common_env["CARGO_BUILD_JOBS"] = "1"
    os.environ.clear()
    os.environ.update(common_env)
    binary, bootstrap_record = bootstrap_once(campaign, bootstrap, common_env)
    result["bootstrap"] = bootstrap_record
    write_json(result_path, result)
    for profile in ORDER[args.case]:
        # Equal-length directory suffixes avoid a path-length bias in embedded locations.
        out = campaign / f"{args.case}-{'sharing' if profile == 'shared' else 'default'}"
        if out.exists():
            raise RuntimeError(f"refusing existing run: {out}")
        os.environ.clear()
        os.environ.update(original_env)
        for key in list(os.environ):
            if key.startswith(("SB09_PBRS_", "PURE_PROTOBUF_")):
                del os.environ[key]
        os.environ["CARGO_BUILD_JOBS"] = "1"
        if profile == "shared":
            os.environ["SB09_PBRS_SHARED_DESCRIPTOR_SET"] = "true"
        telemetry = {"samples": [], "failures": []}
        stopped = threading.Event()

        def check_resources():
            sample = {
                "utc": h.utc_now(),
                "owned_cache_bytes": allocated_bytes([base, out / "cases"]),
                "global_free_bytes": shutil.disk_usage(ROOT).free,
            }
            telemetry["samples"].append(sample)
            if sample["owned_cache_bytes"] > CACHE_LIMIT or sample["global_free_bytes"] < FREE_MINIMUM:
                telemetry["failures"].append(sample)
                return False
            return True

        if not check_resources():
            raise RuntimeError(f"resource bounds already unavailable: {telemetry}")

        def monitor():
            while not stopped.wait(5):
                if not check_resources():
                    stop_own_timed_commands()
                    return

        watcher = threading.Thread(target=monitor, daemon=True)
        watcher.start()
        print(f"START {args.case}/{profile} {h.utc_now()}", flush=True)
        try:
            code = measure_one(out, args.case, profile, binary, bootstrap_record)
        finally:
            stopped.set()
            watcher.join()
            check_resources()
            write_json(out / "resource-telemetry.json", telemetry)
        report = json.loads((out / "summary.json").read_text())
        run = {"profile": profile, "out": h.relative(out, campaign), "exit_code": code,
               "report_sha256": h.sha256(out / "summary.json"), "resource_failures": telemetry["failures"]}
        result["runs"].append(run)
        # Preserve the actual executable before reclaiming only this completed target.
        for cell in report["cells"]:
            if "release_binary" in cell:
                binary = out / cell["release_binary"]["path"]
                preserved = out / "linked" / binary.name
                preserved.parent.mkdir()
                shutil.copy2(binary, preserved)
                if h.sha256(preserved) != cell["release_binary"]["sha256"]:
                    raise RuntimeError("linked binary changed during archival")
                run["preserved_binary"] = {
                    "path": h.relative(preserved, campaign),
                    "sha256": h.sha256(preserved), "bytes": preserved.stat().st_size,
                }
            target = out / cell["target_dir"]
            if target.exists():
                fingerprints = sorted(target.glob("*/.fingerprint/*/*"))
                write_json(out / "fingerprint-sha256.json", {
                    h.relative(path, target): h.sha256(path) for path in fingerprints if path.is_file()
                })
                shutil.rmtree(target)
        write_json(result_path, result)
        print(f"FINISH {args.case}/{profile}: exit {code}; cache reclaimed {h.utc_now()}", flush=True)
        if code or telemetry["failures"]:
            raise RuntimeError("incomplete/failed screen retained; stop before next profile")
    reports = [json.loads((campaign / run["out"] / "summary.json").read_text()) for run in result["runs"]]
    cells = {run["profile"]: report["cells"][0] for run, report in zip(result["runs"], reports)}
    if cells["default"]["corpus"] != cells["shared"]["corpus"]:
        raise RuntimeError("paired input corpus drift")
    if cells["default"]["consumer_lock_sha256"] != cells["shared"]["consumer_lock_sha256"]:
        raise RuntimeError("paired consumer lock drift")
    if reports[0]["environment"]["repository"]["source_sha256"] != reports[1]["environment"]["repository"]["source_sha256"]:
        raise RuntimeError("paired source drift")
    if reports[0]["setup"]["generator_binary_sha256"] != reports[1]["setup"]["generator_binary_sha256"]:
        raise RuntimeError("paired generator executable drift")
    for tool in ("cargo", "rustc", "protoc"):
        if reports[0]["environment"]["tools"][tool] != reports[1]["environment"]["tools"][tool]:
            raise RuntimeError(f"paired tool drift: {tool}")
    manifests = [(campaign / run["out"] / "cases" / args.case / "consumer/Cargo.toml") for run in result["runs"]]
    if manifests[0].read_bytes() != manifests[1].read_bytes():
        raise RuntimeError("paired consumer manifest drift")
    if h.source_hashes() != result["source_sha256"]:
        raise RuntimeError("coordinator source drift")
    result["metrics"] = {profile: h.matrix_metrics(cell) for profile, cell in cells.items()}
    result["status"] = "complete-local-diagnostic"
except Exception as exc:
    result["status"] = "incomplete"
    result["error"] = str(exc)
    raise
finally:
    result["finished_at_utc"] = h.utc_now()
    write_json(result_path, result)
    os.environ.clear()
    os.environ.update(original_env)
