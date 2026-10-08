#!/usr/bin/env python3
"""Build and freeze a source-pinned RPC benchmark executable."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_pin():
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()
    if git("status", "--porcelain", "--untracked-files=all"):
        raise ValueError("benchmark source must be clean, including untracked files")
    return {"commit": git("rev-parse", "HEAD"), "tree": git("rev-parse", "HEAD^{tree}"),
            "locks": {name: digest(ROOT / name) for name in ("Cargo.lock", "rpc-bench/Cargo.lock")}}


def artifact(messages):
    matches = [entry["executable"] for entry in messages
               if entry.get("reason") == "compiler-artifact"
               and entry.get("manifest_path") == str(ROOT / "rpc-bench/Cargo.toml")
               and entry.get("target", {}).get("name") == "rpc-bench"
               and entry.get("target", {}).get("kind") == ["bin"]
               and entry.get("executable")]
    if len(matches) != 1:
        raise ValueError("Cargo must report exactly one RPC benchmark executable")
    return Path(matches[0])


def validate_record(path, binary):
    record = json.loads(path.read_text())
    if record.get("schema") != "pbrs.rpc-bench-build.v1" or record.get("source") != source_pin():
        raise ValueError("build source differs from the clean current source")
    expected = ["cargo", "build", "--locked", "--release", "--manifest-path",
                "rpc-bench/Cargo.toml", "--features", "allocation-counts", "--message-format=json"]
    if record.get("command") != expected or record.get("exit_code") != 0:
        raise ValueError("missing successful release build with allocator counters")
    if record.get("binary_sha256") != digest(binary):
        raise ValueError("binary differs from frozen build")
    if any(not record.get("tools", {}).get(tool) for tool in ("rustc", "cargo")):
        raise ValueError("missing build tool versions")
    for name in ("build.stdout.jsonl", "build.stderr.log"):
        if record.get("files", {}).get(name) != digest(path.parent / name):
            raise ValueError("build log changed")
    messages = [json.loads(line) for line in (path.parent / "build.stdout.jsonl").read_text().splitlines()]
    if str(artifact(messages)) != record.get("build_executable"):
        raise ValueError("record does not identify the Cargo executable")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    pin = source_pin()
    if not re.fullmatch(r"[0-9a-f]{40}", args.source) or pin["commit"] != args.source:
        parser.error("--source must be the full current commit")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    command = ["cargo", "build", "--locked", "--release", "--manifest-path",
               "rpc-bench/Cargo.toml", "--features", "allocation-counts", "--message-format=json"]
    with (args.output / "build.stdout.jsonl").open("w") as stdout, \
            (args.output / "build.stderr.log").open("w") as stderr:
        result = subprocess.run(command, cwd=ROOT, stdout=stdout, stderr=stderr, check=False)
    if result.returncode != 0:
        raise ValueError(f"build failed: exit {result.returncode}; logs retained")
    if source_pin() != pin:
        raise ValueError("source changed during build; logs retained")
    messages = [json.loads(line) for line in (args.output / "build.stdout.jsonl").read_text().splitlines()]
    executable = artifact(messages)
    binary = args.output / "rpc-bench"
    shutil.copy2(executable, binary)
    record = {"schema": "pbrs.rpc-bench-build.v1", "source": pin, "command": command,
              "exit_code": result.returncode, "build_executable": str(executable),
              "binary_sha256": digest(binary),
              "tools": {tool: subprocess.check_output([tool, "--version"], text=True).strip()
                        for tool in ("rustc", "cargo")},
              "files": {name: digest(args.output / name)
                        for name in ("build.stdout.jsonl", "build.stderr.log")}}
    path = args.output / "build.json"
    path.write_text(json.dumps(record, indent=2) + "\n")
    validate_record(path, binary)
    print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
