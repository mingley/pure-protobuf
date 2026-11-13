#!/usr/bin/env python3
"""Build concrete generated Cortex messages and retain dependency-feature evidence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sources():
    return {str(path): digest(path) for path in [
        ROOT / "Cargo.toml", ROOT / "build.rs", *sorted((ROOT / "src").rglob("*.rs")),
        *sorted(HERE.rglob("*.rs")), *sorted(HERE.rglob("*.proto")),
        *sorted(HERE.rglob("Cargo.toml")), *sorted(HERE.rglob("Cargo.lock")),
        *sorted(HERE.rglob("*.py")),
    ]}


def artifact_rows(stdout, target):
    rows = []
    for line in stdout.read_text().splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("reason") != "compiler-artifact" or "lib" not in row["target"]["kind"]:
            continue
        files = [path for path in row["filenames"] if path.endswith((".rlib", ".rmeta"))]
        if files:
            rows.append({"package_id": row["package_id"], "name": row["target"]["name"],
                         "context": "target" if any(target in Path(path).parts for path in files) else "host",
                         "features": row["features"], "files": files})
    return rows


def validate_single_test_transcript(output, expected_name):
    """Require the named test to run once, with no skipped or extra tests."""
    tests = re.findall(r"^test ([^\r\n]+?) \.\.\. ([^\r\n]+)$", output, re.MULTILINE)
    summaries = re.findall(
        r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
        r"(\d+) measured; (\d+) filtered out;[^\r\n]*$", output, re.MULTILINE,
    )
    if tests != [(expected_name, "ok")] or summaries != [("ok", "1", "0", "0", "0", "0")]:
        raise ValueError(f"expected exactly one passing {expected_name} with no skipped tests; "
                         f"observed tests={tests}, summaries={summaries}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target", default="thumbv7em-none-eabi")
    parser.add_argument("--target-dir", type=Path, help="Dedicated team build directory, included in the resource cap")
    parser.add_argument("--baseline-plugin", type=Path, help="Frozen ordinary-output generator for cross-version byte comparison")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    target_dir = args.target_dir.resolve() if args.target_dir else out / "target"
    target_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update({"CARGO_TARGET_DIR": str(target_dir), "CARGO_BUILD_JOBS": "1",
                "CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0",
                "CARGO_PROFILE_TEST_DEBUG": "0"})
    if args.baseline_plugin:
        env["ALLOC_PROOF_BASELINE_PLUGIN"] = str(args.baseline_plugin.resolve())
    before = sources()
    if args.baseline_plugin:
        before[str(args.baseline_plugin.resolve())] = digest(args.baseline_plugin)
    (out / "inputs-before.json").write_text(json.dumps(before, indent=2) + "\n")
    records = []
    peak = 0
    minimum = shutil.disk_usage(out).free

    def run(label, command, expect_success=True):
        nonlocal peak, minimum
        stdout, stderr = out / (label + ".jsonl"), out / (label + ".stderr")
        with stdout.open("w") as output, stderr.open("w") as error:
            process = subprocess.Popen([str(value) for value in command], cwd=ROOT, env=env,
                                       stdout=output, stderr=error, start_new_session=True)
            stopped = None
            while process.poll() is None:
                size = 0
                for path in target_dir.rglob("*"):
                    try:
                        if path.is_file():
                            size += path.stat().st_size
                    except FileNotFoundError:
                        pass
                free = shutil.disk_usage(out).free
                peak, minimum = max(peak, size), min(minimum, free)
                if size > 440 * 1024 ** 2 or free < 1024 ** 3:
                    stopped = "440 MiB target cap (500 MiB team budget) or 1 GiB free-space floor"
                    os.killpg(process.pid, signal.SIGTERM)
                    break
                time.sleep(0.2)
            code = process.wait()
        records.append({"label": label, "command": [str(value) for value in command], "exit_code": code,
                        "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr), "resource_stop": stopped})
        (out / "commands.json").write_text(json.dumps(records, indent=2) + "\n")
        generated = out / (label + "-generated")
        out_dirs = sorted({
            *target_dir.glob("**/build/pbrs-alloc-runtime-proof-*/out"),
            *(source for source in target_dir.glob("**/build/pbrs-alloc-runtime-proof/*/out")
              if (source.parent / "run").is_dir()),
        })
        for index, source in enumerate(out_dirs):
            shutil.copytree(source, generated / str(index), dirs_exist_ok=True)
        diagnostics = []
        for line in stdout.read_text().splitlines():
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue
            if message.get("reason") == "compiler-message":
                diagnostics.append(message["message"].get("rendered", ""))
        (out / (label + ".diagnostics.txt")).write_text("".join(diagnostics))

        if stopped:
            raise RuntimeError(stopped)
        if expect_success and code:
            raise RuntimeError(label + " failed; see " + str(stderr))
        return code, stdout, stderr

    manifest = HERE / "Cargo.toml"
    common = ["--locked", "--offline", "-j1", "--message-format=json"]
    _, embedded, _ = run("cortex", ["cargo", "build", "--manifest-path", manifest,
                                  "--target", args.target, *common])
    features = artifact_rows(embedded, args.target)
    for name in ["pbrs", "bytes", "simdutf8", "prost"]:
        selected = [row for row in features if row["name"] == name and row["context"] == "target"]
        if not selected or any("std" in row["features"] or "default" in row["features"] for row in selected):
            raise RuntimeError("target dependency is missing or enabled std/default: " + name)
    host_codegen = [row for row in features if row["name"] == "pbrs" and row["context"] == "host"]
    if not any({"std", "codegen", "reflect"} <= set(row["features"]) for row in host_codegen):
        raise RuntimeError("receipt lacks the separate std host generator")
    _, host_output, _ = run("host-execution", ["cargo", "test", "--manifest-path", manifest,
                                              "--features", "std", "--lib", *common,
                                              "--", "--test-threads=1", "--color", "never"])
    host_test = "tests::all_native_fields_against_generated_prost"
    validate_single_test_transcript(host_output.read_text(), host_test)
    code, _, error = run("unsupported-pure-host", ["cargo", "check", "--manifest-path", manifest, *common], False)
    if code == 0 or "alloc-only without std currently supports only bare-metal targets" not in (out / "unsupported-pure-host.diagnostics.txt").read_text():
        raise RuntimeError("pure host alloc-only did not fail with its profile diagnostic")
    _, legacy, _ = run("legacy-no-default", ["cargo", "test", "--manifest-path", HERE / "legacy_std/Cargo.toml",
                                            "--lib", *common, "--", "--test-threads=1", "--color", "never"])
    legacy_test = "tests::existing_standard_library_apis"
    validate_single_test_transcript(legacy.read_text(), legacy_test)
    legacy_features = artifact_rows(legacy, args.target)
    for name in ["bytes", "simdutf8"]:
        if not any(row["name"] == name and "std" in row["features"] for row in legacy_features):
            raise RuntimeError("legacy no-default dependency lost std integration or dispatch: " + name)
    after = sources()
    if args.baseline_plugin:
        after[str(args.baseline_plugin.resolve())] = digest(args.baseline_plugin)
    report = {"target": args.target, "commands": records, "cortex_artifacts": features,
              "legacy_artifacts": legacy_features, "inputs_sha256": before, "inputs_unchanged": before == after,
              "baseline_plugin": str(args.baseline_plugin.resolve()) if args.baseline_plugin else None,
              "peak_team_target_bytes": peak, "minimum_free_bytes": minimum,
              "required_tests": {"host-execution": host_test, "legacy-no-default": legacy_test},
              "scope": "Concrete generated native and Prost target compilation; explicit std-backed host execution. No hardware execution or performance result."}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if before != after:
        raise RuntimeError("selected source inputs changed")
    print(json.dumps({"passed": True, "target": args.target, "commands": len(records),
                      "baseline_output_comparison": bool(args.baseline_plugin)}))


if __name__ == "__main__":
    main()
