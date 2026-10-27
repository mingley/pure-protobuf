#!/usr/bin/env python3
"""Run generated-consumer controls and explicit compile-fail API probes.

This is a scoped parity audit, not a proof of full compiler equivalence.
The successful reference generation exercises the requested configurations;
the negative probes establish migration/configuration gaps, not wire failures.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
METHODS = [
    "btree_map", "bytes", "boxed", "retain_enum_prefix",
    "enable_type_names", "type_name_domain",
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--embedded-target", default="thumbv7em-none-eabi")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_BUILD_JOBS="1", CARGO_INCREMENTAL="0")
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / "target"))
    selected_protoc = shutil.which(env.get("PROTOC", "protoc"), path=env.get("PATH"))
    if selected_protoc is None:
        parser.error("protoc must be available via PROTOC or PATH")
    env["PROTOC"] = selected_protoc
    manifest = HERE / "Cargo.toml"
    source_paths = sorted(path for path in HERE.rglob("*") if path.is_file() and ".git" not in path.parts)
    source_paths += sorted((ROOT / "src").rglob("*.rs"))
    source_paths += [ROOT / path for path in ["Cargo.toml", "build.rs"]]
    before_hashes = {str(path.relative_to(ROOT)): sha(path) for path in source_paths}

    def run(name, command):
        result = subprocess.run(command, cwd=HERE, env=env, capture_output=True)
        (output / f"{name}.stdout").write_bytes(result.stdout)
        (output / f"{name}.stderr").write_bytes(result.stderr)
        return result, {"command": command, "exit_code": result.returncode}

    result, consumer = run("consumer", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", "pbrs-codegen-parity-proof"])
    consumer["passed"] = result.returncode == 0
    consumer["cases_passed"] = result.stdout.count(b"PASS imported-")
    generated = {}
    for line in result.stdout.decode().splitlines():
        if line.startswith("GENERATED_ROOT="):
            generated_root = Path(line.removeprefix("GENERATED_ROOT="))
            for path in sorted(generated_root.rglob("*.rs")):
                relative = path.relative_to(generated_root)
                destination = output / "generated" / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, destination)
                generated[str(relative)] = sha(path)
    probes = []
    for method in METHODS:
        result, probe = run(method, ["cargo", "check", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", f"pbrs_{method}"])
        probe.update({
            "method": method,
            "gap_reproduced": result.returncode != 0
            and b"error[E0599]" in result.stderr
            and f"no method named `{method}`".encode() in result.stderr,
            "scope": "build configuration / Rust API migration; not wire semantics",
        })
        probes.append(probe)

    no_std = {"target": args.embedded_target}
    for peer in ["prost", "pbrs"]:
        result, control = run(f"no-std-{peer}", ["cargo", "check", "--locked", "--offline", "--manifest-path", str(HERE / "no_std" / peer / "Cargo.toml"), "--target", args.embedded_target])
        control["compiled"] = result.returncode == 0
        control["std_unavailable"] = b"can't find crate for `std`" in result.stderr and b"error[E0463]" in result.stderr
        no_std[peer] = control
    no_std["functional_gap_reproduced"] = no_std["prost"]["compiled"] and not no_std["pbrs"]["compiled"] and no_std["pbrs"]["std_unavailable"]

    after_hashes = {str(path.relative_to(ROOT)): sha(path) for path in source_paths}
    report = {
        "schema_version": 1,
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "rustc": subprocess.check_output(["rustc", "--version"], env=env, text=True).strip(),
        "protoc": subprocess.check_output([selected_protoc, "--version"], env=env, text=True).strip(),
        "protoc_sha256": sha(Path(selected_protoc)),
        "consumer": consumer,
        "configuration_probes": probes,
        "alloc_only_target": no_std,
        "source_sha256": after_hashes,
        "changed_source_files": [name for name in before_hashes if before_hashes[name] != after_hashes[name]],
        "generated_sha256": generated,
        "verdict": "limited generated wire equivalence; alloc-only deployment and compiler API parity have counterexamples",
        "limitations": [
            "Nine generated roundtrip cases are not an exhaustive schema or malformed-input corpus.",
            "Wire normalization uses prost value equality; map serialization order is not compared.",
            "Configuration probes compare the public build APIs and selected generated representations.",
            "pbrs uses its own map, byte storage and naming APIs; these failures do not prove a wire-format gap.",
            "The alloc-only target control proves compilation availability, not execution on embedded hardware.",
            "No timing or ranking conclusion is drawn from build execution.",
        ],
    }
    report["harness_passed"] = consumer["passed"] and consumer["cases_passed"] == 9 and all(p["gap_reproduced"] for p in probes) and no_std["functional_gap_reproduced"] and not report["changed_source_files"]
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"harness_passed": report["harness_passed"], "consumer_cases": consumer["cases_passed"], "configuration_gaps": sum(p["gap_reproduced"] for p in probes), "alloc_only_gap": no_std["functional_gap_reproduced"]}))
    return 0 if report["harness_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
