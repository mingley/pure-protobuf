#!/usr/bin/env python3
"""Check scoped ordered-map parity and retain explicit mixed-generation limits."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot():
    names = subprocess.check_output([
        "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
        "--", "src", "Cargo.toml", "Cargo.lock", "build.rs", "tests/proof/ordered_maps",
    ], cwd=ROOT).decode().split("\0")
    return {name: sha(ROOT / name) for name in sorted(set(names))
            if name and (ROOT / name).is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    env = os.environ.copy()
    env.setdefault("CARGO_BUILD_JOBS", "1")
    env.setdefault("CARGO_INCREMENTAL", "0")
    env.setdefault("CARGO_PROFILE_DEV_DEBUG", "0")
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / "target"))
    command = ["cargo", "run", "--locked", "--offline", "--manifest-path", str(HERE / "Cargo.toml"),
               "--message-format=json-render-diagnostics"]
    result = subprocess.run(command, env=env, cwd=ROOT, text=True, capture_output=True)
    (args.output / "stdout").write_text(result.stdout)
    (args.output / "stderr").write_text(result.stderr)
    output = result.stdout
    default_roots = [Path(line.removeprefix("DEFAULT_ROOT=")) for line in output.splitlines()
                     if line.startswith("DEFAULT_ROOT=")]
    baseline = json.loads((HERE / "baseline-default.json").read_text())
    baseline_report = HERE / "baseline-parent-json.json"
    baseline_red = json.loads(baseline_report.read_text())
    baseline_red_valid = (
        sha(baseline_report) == baseline["parent_json_baseline"]["reproduction_report_sha256"]
        and len(baseline_red["results"]) == 2
        and all(case["process_exit"] == 0 for case in baseline_red["results"])
        and "json_result=Err(ParseError)" in baseline_red["results"][1]["stdout"]
        and "text_result=Ok(" in baseline_red["results"][1]["stdout"]
        and any(name.endswith("/libpbrs.rlib") and value == baseline["runtime_sha256"]
                for name, value in baseline_red["inputs"].items())
        and any(name.endswith("/ordered.rs") and value == baseline["generated_sha256"]["ordered.rs"]
                for name, value in baseline_red["inputs"].items())
    )
    golden = bool(default_roots) and all(
        sha(default_roots[-1] / filename) == expected
        for filename, expected in baseline["generated_sha256"].items()
    )
    frozen = json.loads((HERE / "old_parent/manifest.json").read_text())
    frozen_valid = all(sha(HERE / name) == expected
                       for name, expected in frozen["input_output_sha256"].items())
    after = snapshot()
    artifacts = []
    for line in output.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("executable"):
            path = Path(message["executable"])
            artifacts.append({"path": str(path), "sha256": sha(path), "features": message["features"]})
    report = {
        "command": command, "process_exit": result.returncode,
        "source_inputs_unchanged": before == after, "source_sha256": before,
        "runtime_groups": output.count("PASS ordered-case "),
        "selector_controls": output.count("PASS selector "),
        "legacy_cache_controls": output.count("PASS ordered-legacy "),
        "parent_json_controls": output.count("PASS ordered-parent-json "),
        "baseline_parent_json_red_preserved": baseline_red_valid,
        "default_output_matches_frozen_05a3": golden,
        "frozen_old_parent_valid": frozen_valid,
        "compiled_artifacts": artifacts,
        "limits": [
            "Old or differently configured lazy ancestors can forward original unsorted duplicate map wire until mutation.",
            "Map default fields keep pbrs explicit encoding; equal ordered values do not imply byte equality with Prost.",
            "Current Parent JSON positives require canonical descriptor resolution; the historical 05a3 failure is preserved separately.",
            "Ordered queries currently convert to owned keys and string lookups may allocate.",
            "These finite generated consumers do not establish exhaustive parity or any timing/ranking result.",
        ],
    }
    report["harness_passed"] = (
        result.returncode == 0 and before == after and report["runtime_groups"] == 24
        and report["selector_controls"] == 18 and report["legacy_cache_controls"] == 1
        and report["parent_json_controls"] == 4 and baseline_red_valid and golden and frozen_valid
    )
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key != "source_sha256"}, indent=2))
    return 0 if report["harness_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
