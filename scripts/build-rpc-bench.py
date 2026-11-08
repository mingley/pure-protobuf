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


def build_command(allocation_counts):
    command = ["cargo", "build", "--locked", "--release", "--manifest-path",
               "rpc-bench/Cargo.toml"]
    if allocation_counts:
        command += ["--features", "allocation-counts"]
    else:
        command += ["--no-default-features"]
    return [*command, "--message-format=json"]


def artifact_message(messages, source_checkout=None):
    root = source_checkout or ROOT
    matches = [entry for entry in messages
               if entry.get("reason") == "compiler-artifact"
               and entry.get("manifest_path") == str(root / "rpc-bench/Cargo.toml")
               and entry.get("target", {}).get("name") == "rpc-bench"
               and entry.get("target", {}).get("kind") == ["bin"]
               and entry.get("executable")]
    if len(matches) != 1:
        raise ValueError("Cargo must report exactly one RPC benchmark executable")
    return matches[0]


def artifact_features(entry):
    features = entry.get("features")
    if (not isinstance(features, list)
            or any(type(feature) is not str or not feature for feature in features)
            or len(set(features)) != len(features)):
        raise ValueError("Cargo executable is missing valid artifact features")
    return features


def artifact(messages):
    return Path(artifact_message(messages)["executable"])


def check_artifact_features(features, allocation_counts, *, legacy=False):
    enabled = set(features)
    if legacy:
        # Older commands enabled defaults from their own pinned manifest.
        valid = allocation_counts and "allocation-counts" in enabled
    elif allocation_counts:
        valid = enabled in ({"allocation-counts"}, {"allocation-counts", "default"})
    else:
        valid = not enabled
    if not valid:
        raise ValueError("Cargo artifact features disagree with the build command or mode")


def validate_feature_record(record, path, source_checkout=None):
    """Check command, Cargo features, and recorded mode, including legacy records."""
    command = record.get("command")
    if command == build_command(True):
        allocation_counts = True
    elif command == build_command(False):
        allocation_counts = False
    else:
        raise ValueError("unrecognized RPC benchmark build command")
    messages = [json.loads(line) for line in (path.parent / "build.stdout.jsonl").read_text().splitlines()]
    features = artifact_features(artifact_message(messages, source_checkout))
    has_mode = "allocation_counts" in record
    has_features = "artifact_features" in record
    schema = record.get("schema")
    if schema == "pbrs.rpc-bench-build.v1":
        if has_mode or has_features or not allocation_counts:
            raise ValueError("invalid legacy build feature metadata")
        check_artifact_features(features, allocation_counts, legacy=True)
    elif schema == "pbrs.rpc-bench-build.v2":
        if not has_mode or not has_features:
            raise ValueError("incomplete build feature metadata")
        check_artifact_features(features, allocation_counts)
        if (type(record["allocation_counts"]) is not bool
                or record["allocation_counts"] != allocation_counts
                or record["artifact_features"] != features):
            raise ValueError("build feature metadata disagrees with the Cargo artifact")
    else:
        raise ValueError("unrecognized RPC benchmark build record schema")
    return allocation_counts


def validate_record(path, binary):
    record = json.loads(path.read_text())
    if record.get("schema") != "pbrs.rpc-bench-build.v2":
        raise ValueError("this builder requires a v2 build record")
    if record.get("source") != source_pin():
        raise ValueError("build source differs from the clean current source")
    if type(record.get("exit_code")) is not int or record["exit_code"] != 0:
        raise ValueError("missing successful release build")
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
    validate_feature_record(record, path)
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--no-allocation-counts", action="store_true",
                        help="disable the benchmark package's allocation-counts Cargo feature")
    args = parser.parse_args()
    pin = source_pin()
    if not re.fullmatch(r"[0-9a-f]{40}", args.source) or pin["commit"] != args.source:
        parser.error("--source must be the full current commit")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    allocation_counts = not args.no_allocation_counts
    command = build_command(allocation_counts)
    with (args.output / "build.stdout.jsonl").open("w") as stdout, \
            (args.output / "build.stderr.log").open("w") as stderr:
        try:
            before_build = source_pin()
        except ValueError as error:
            raise ValueError("build output or source is dirty; use an ignored or external output directory") from error
        if before_build != pin:
            raise ValueError("source changed before build; logs retained")
        result = subprocess.run(command, cwd=ROOT, stdout=stdout, stderr=stderr, check=False)
    if result.returncode != 0:
        raise ValueError(f"build failed: exit {result.returncode}; logs retained")
    if source_pin() != pin:
        raise ValueError("source changed during build; logs retained")
    messages = [json.loads(line) for line in (args.output / "build.stdout.jsonl").read_text().splitlines()]
    entry = artifact_message(messages)
    features = artifact_features(entry)
    check_artifact_features(features, allocation_counts)
    executable = Path(entry["executable"])
    binary = args.output / "rpc-bench"
    shutil.copy2(executable, binary)
    record = {"schema": "pbrs.rpc-bench-build.v2", "source": pin, "command": command,
              "allocation_counts": allocation_counts, "artifact_features": features,
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
