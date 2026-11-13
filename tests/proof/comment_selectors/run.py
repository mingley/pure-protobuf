#!/usr/bin/env python3
"""Compile and execute pinned-Prost source-comment selector consumers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CASES = {"none", "global", "package", "message", "field", "suffix", "nested", "enumeration", "enum-value", "imported", "nonmatch", "boundary", "reset", "false-selected", "old-global"}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inputs():
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "build.rs", *sorted((ROOT / "src").rglob("*.rs"))]
    files += [path for path in HERE.rglob("*") if path.is_file()]
    return {str(path): sha(path) for path in files}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--baseline-plugin", type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    target = args.target_dir.resolve() if args.target_dir else out / "target"
    target.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update({"CARGO_TARGET_DIR": str(target), "CARGO_BUILD_JOBS": "1", "CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0"})
    if args.baseline_plugin:
        env["COMMENT_BASELINE_PLUGIN"] = str(args.baseline_plugin.resolve())
    before = inputs()
    if args.baseline_plugin:
        before[str(args.baseline_plugin.resolve())] = sha(args.baseline_plugin)
    records = []

    def run(label, command):
        stdout, stderr = out / (label + ".stdout"), out / (label + ".stderr")
        with stdout.open("w") as output, stderr.open("w") as error:
            process = subprocess.Popen([str(arg) for arg in command], cwd=ROOT, env=env, stdout=output, stderr=error, start_new_session=True)
            stop = None
            while process.poll() is None:
                size = 0
                for path in target.rglob("*"):
                    try:
                        if path.is_file():
                            size += path.stat().st_size
                    except FileNotFoundError:
                        pass
                if size > 350 * 1024 ** 2 or shutil.disk_usage(out).free < 1024 ** 3:
                    stop = "350 MiB target cap / 1 GiB filesystem floor"
                    os.killpg(process.pid, signal.SIGTERM)
                    break
                time.sleep(0.2)
            code = process.wait()
        records.append({"label": label, "command": [str(arg) for arg in command], "exit_code": code, "resource_stop": stop, "stdout_sha256": sha(stdout), "stderr_sha256": sha(stderr)})
        (out / "commands.json").write_text(json.dumps(records, indent=2) + "\n")
        if code or stop:
            raise RuntimeError(label + " failed: " + str(stderr))
        return stdout

    (out / "inputs-before.json").write_text(json.dumps(before, indent=2) + "\n")
    artifacts = run("build", ["cargo", "build", "--locked", "--offline", "--manifest-path", HERE / "Cargo.toml", "--bins", "-j1", "--message-format=json"])
    binaries = {}
    for line in artifacts.read_text().splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("reason") == "compiler-artifact" and row["target"]["name"] in CASES and row.get("executable"):
            name = row["target"]["name"]
            if name in binaries:
                raise RuntimeError("duplicate consumer artifact: " + name)
            binaries[name] = Path(row["executable"])
    if set(binaries) != CASES:
        raise RuntimeError("missing compiled consumers")
    for name, binary in sorted(binaries.items()):
        stdout = run(name, [binary])
        if stdout.read_text().splitlines() != ["PASS " + name]:
            raise RuntimeError("missing, extra or nonpassing consumer result: " + name)
    generated = out / "generated"
    out_dirs = sorted({
        *target.glob("debug/build/pbrs-comment-selector-proof-*/out"),
        *(path for path in target.glob("debug/build/pbrs-comment-selector-proof/*/out")
          if (path.parent / "run").is_dir()),
    })
    for index, path in enumerate(out_dirs):
        shutil.copytree(path, generated / str(index))
    after = inputs()
    if args.baseline_plugin:
        after[str(args.baseline_plugin.resolve())] = sha(args.baseline_plugin)
    report = {"inputs_sha256": before, "inputs_unchanged": before == after, "cases": sorted(CASES), "commands": records, "baseline_plugin": str(args.baseline_plugin.resolve()) if args.baseline_plugin else None, "scope": "Source-comment selector agreement, compiled native/Prost wire consumers, builder/plugin/FDS identity and configuration isolation. RPC comment routing is source-only. No performance claim."}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if before != after:
        raise RuntimeError("selected source changed")
    print(json.dumps({"passed": True, "consumers": len(binaries), "inputs_unchanged": True}))


if __name__ == "__main__":
    main()
