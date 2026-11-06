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
METHODS = ["btree_map"]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run_bytes_controls(manifest, run):
    profiles = []
    for profile, binary in [
        ("selected", "pbrs_bytes"), ("owned", "pbrs_bytes_owned"), ("safe", "pbrs_bytes_safe"),
    ]:
        result, control = run(f"bytes-{profile}", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", binary])
        control.update(profile=profile, passed=result.returncode == 0,
                       cases_passed=result.stdout.count(b"PASS bytes-case "),
                       selector_cases_passed=result.stdout.count(b"PASS bytes-selector "))
        profiles.append(control)
    controls = []
    for name, binary, marker in [
        ("default", "pbrs_bytes_default_control", b"PASS bytes-default "),
        ("old-parent", "pbrs_bytes_old_parent", b"PASS bytes-old-parent "),
        ("lightweight", "pbrs_bytes_lightweight", b"PASS bytes-lightweight "),
    ]:
        result, control = run(f"bytes-{name}", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", binary])
        control.update(name=name, passed=result.returncode == 0, cases_passed=result.stdout.count(marker))
        if name == "old-parent":
            control["duplicate_remove_difference_reproduced"] = result.stdout.count(b"KNOWN-GAP bytes-map-remove ") == 1
            control["old_source_scope"] = "frozen6f generated parent and current unconfigured parent, both linked to the current runtime"
        controls.append(control)
    result, lightweight_gap = run("bytes-lightweight-gap", ["cargo", "check", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", "pbrs_bytes_lightweight_gap"])
    lightweight_gap.update(
        reproduced=result.returncode != 0 and result.stderr.count(b"error[E0599]") == 4
        and all(f"`{method}`".encode() in result.stderr for method in ["to_json", "from_json", "to_text", "from_text"]),
        scope="existing complex-schema typed-codec subset when reflection fallback is disabled; not a Prost JSON comparison",
    )
    prefixes = []
    for prefix in ["default", "relative", "absolute", "local"]:
        result, control = run(f"bytes-prefix-{prefix}", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", f"pbrs_bytes_prefix_{prefix}"])
        control.update(prefix=prefix, passed=result.returncode == 0, cases_passed=result.stdout.count(b"PASS bytes-prefix "))
        prefixes.append(control)
    frozen = json.loads((HERE / "old_bytes_parent/manifest.json").read_text())
    return {
        "profiles": profiles, "controls": controls, "prefixes": prefixes, "lightweight_gap": lightweight_gap,
        "frozen_old_source_valid": all(sha(HERE / path) == expected for path, expected in frozen["input_output_sha256"].items()),
        "scope": "selected Bytes-backed storage, sharing, codecs, source selection and explicit old/mixed-binding limits",
    }


def bytes_controls_passed(control):
    return (
        len(control["profiles"]) == 3
        and all(case["passed"] and case["cases_passed"] == 13 for case in control["profiles"])
        and [case["selector_cases_passed"] for case in control["profiles"]] == [21, 0, 0]
        and len(control["controls"]) == 3
        and all(case["passed"] and case["cases_passed"] == 1 for case in control["controls"])
        and next(case for case in control["controls"] if case["name"] == "old-parent")["duplicate_remove_difference_reproduced"]
        and control["lightweight_gap"]["reproduced"] and control["frozen_old_source_valid"]
        and len(control["prefixes"]) == 4
        and all(case["passed"] and case["cases_passed"] == 1 for case in control["prefixes"])
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--embedded-target", default="thumbv7em-none-eabi")
    parser.add_argument("--only", choices=["all", "bytes"], default="all", help="run the focused bytes controls without the embedded-target and unrelated suites")
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists():
        parser.error("choose a new output directory to preserve earlier evidence")
    output.mkdir(parents=True)
    env = dict(os.environ, CARGO_BUILD_JOBS="1", CARGO_INCREMENTAL="0")
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / "target"))
    selected_protoc = shutil.which(env.get("PROTOC", "protoc"), path=env.get("PATH"))
    if selected_protoc is None:
        parser.error("protoc must be available via PROTOC or PATH")
    env["PROTOC"] = selected_protoc
    manifest = HERE / "Cargo.toml"
    source_paths = sorted(
        path for path in HERE.rglob("*")
        if path.is_file()
        and not any(part in (".git", "target", "__pycache__") for part in path.relative_to(HERE).parts)
    )
    source_paths += sorted((ROOT / "src").rglob("*.rs"))
    source_paths += [ROOT / path for path in ["Cargo.toml", "build.rs"]]
    before_hashes = {str(path.relative_to(ROOT)): sha(path) for path in source_paths}

    def run(name, command):
        result = subprocess.run(command, cwd=HERE, env=env, capture_output=True)
        (output / f"{name}.stdout").write_bytes(result.stdout)
        (output / f"{name}.stderr").write_bytes(result.stderr)
        return result, {"command": command, "exit_code": result.returncode}

    if args.only == "bytes":
        controls = run_bytes_controls(manifest, run)
        after_hashes = {str(path.relative_to(ROOT)): sha(path) for path in source_paths}
        report = {
            "schema_version": 1,
            "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            "rustc": subprocess.check_output(["rustc", "--version"], env=env, text=True).strip(),
            "protoc": subprocess.check_output([selected_protoc, "--version"], env=env, text=True).strip(),
            "protoc_sha256": sha(Path(selected_protoc)), "bytes_fields": controls,
            "source_sha256": after_hashes,
            "changed_source_files": [name for name in before_hashes if before_hashes[name] != after_hashes[name]],
            "limitations": [
                "No timing or ranking conclusion follows from these functional controls.",
                "Older or differently configured eager parents can copy selected child payloads.",
                "Duplicate-key removal exposes an earlier pbrs map entry; Prost removes the logical key.",
                "With reflection disabled, complex schemas outside the typed subset omit JSON/text methods.",
            ],
        }
        report["harness_passed"] = bytes_controls_passed(controls) and not report["changed_source_files"]
        (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"harness_passed": report["harness_passed"], "bytes_cases": sum(case["cases_passed"] for case in controls["profiles"]), "bytes_selector_cases": sum(case["selector_cases_passed"] for case in controls["profiles"]), "controls": len(controls["controls"]), "lightweight_gap": controls["lightweight_gap"]["reproduced"]}))
        return 0 if report["harness_passed"] else 1

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
    result, options = run("configured-options", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", "pbrs_configured_options"])
    options["passed"] = result.returncode == 0
    options["name_cases_passed"] = result.stdout.count(b"PASS configured-name ")
    options["any_cases_passed"] = result.stdout.count(b"PASS configured-any ")
    options["any_url_cases_passed"] = result.stdout.count(b"PASS configured-any-url ")
    options["any_malformed_cases_passed"] = result.stdout.count(b"PASS configured-any-malformed ")
    options["enum_cases_passed"] = result.stdout.count(b"PASS configured-enum ")
    options["scope"] = "same-schema name and type URL selection, retained enum spelling and cross-codec wire values"
    options["semantic_controls_match"] = options["passed"] and all(
        options[key] == expected for key, expected in {
            "name_cases_passed": 8,
            "any_cases_passed": 4,
            "any_url_cases_passed": 12,
            "any_malformed_cases_passed": 1,
            "enum_cases_passed": 4,
        }.items()
    )
    result, attributes = run("attribute-selectors", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", "pbrs_attribute_selectors"])
    attributes["passed"] = result.returncode == 0
    attributes["cases_passed"] = result.stdout.count(b"PASS attribute-selector ")
    attributes["scope"] = "same-schema field attribute selector placement against prost-build"
    collisions = []
    for profile, binary in [
        ("lazy", "pbrs_type_collisions"),
        ("owned", "pbrs_owned_type_collisions"),
        ("external", "pbrs_external_type_collisions"),
    ]:
        result, collision = run(f"type-collisions-{profile}", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", binary])
        collision.update({
            "profile": profile,
            "passed": result.returncode == 0,
            "cases_passed": result.stdout.count(b"PASS type-collision "),
            "scope": "flattened identifiers, message View/Mut names, scoped imported aliases, synthetic map entries and explicit renamed external parents",
        })
        collisions.append(collision)
    runtime_symbols = []
    for profile, binary in [
        ("lazy", "pbrs_runtime_symbols"),
        ("owned", "pbrs_owned_runtime_symbols"),
    ]:
        result, symbols = run(f"runtime-symbols-{profile}", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", binary])
        symbols.update({
            "profile": profile,
            "passed": result.returncode == 0,
            "cases_passed": result.stdout.count(b"PASS runtime-symbol-"),
            "scope": "literal Default/Map/Enum schema names, typed view and mutator macros, and cross-file wire values against Prost",
        })
        runtime_symbols.append(symbols)
    boxed_fields = []
    for profile, binary in [
        ("selected", "pbrs_boxed"),
        ("default", "pbrs_boxed_default"),
        ("owned", "pbrs_boxed_owned"),
    ]:
        result, boxed = run(f"boxed-fields-{profile}", ["cargo", "run", "--locked", "--offline", "--manifest-path", str(manifest), "--bin", binary])
        boxed.update({
            "profile": profile,
            "passed": result.returncode == 0,
            "cases_passed": result.stdout.count(b"PASS boxed-case "),
            "selector_cases_passed": result.stdout.count(b"PASS boxed-selector "),
            "scope": "selected eager boxed singular messages, same-schema wire values, sharing and validation; message-selector source placement against Prost",
        })
        boxed_fields.append(boxed)
    bytes_fields = run_bytes_controls(manifest, run)
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
        "configured_options": options,
        "attribute_selectors": attributes,
        "type_collisions": collisions,
        "runtime_symbols": runtime_symbols,
        "boxed_fields": boxed_fields,
        "bytes_fields": bytes_fields,
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
            "Boxed selectors apply to singular message/group fields; Prost's scalar boxing output is not a supported compiled control.",
            "Selected bytes can copy through old/mixed eager parents; duplicate map removal and reflection-off complex codecs retain explicit counterexamples.",
        ],
    }
    report["harness_passed"] = (
        consumer["passed"] and consumer["cases_passed"] == 9
        and options["semantic_controls_match"]
        and attributes["passed"] and attributes["cases_passed"] == 13
        and all(case["passed"] and case["cases_passed"] == 4 for case in collisions)
        and all(case["passed"] and case["cases_passed"] == 4 for case in runtime_symbols)
        and all(case["passed"] and case["cases_passed"] == 8 for case in boxed_fields)
        and [case["selector_cases_passed"] for case in boxed_fields] == [18, 0, 0]
        and bytes_controls_passed(bytes_fields)
        and all(probe["gap_reproduced"] for probe in probes)
        and no_std["functional_gap_reproduced"]
        and not report["changed_source_files"]
    )
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"harness_passed": report["harness_passed"], "consumer_cases": consumer["cases_passed"], "name_cases": options["name_cases_passed"], "any_cases": options["any_cases_passed"], "enum_cases": options["enum_cases_passed"], "attribute_cases": attributes["cases_passed"], "type_collision_cases": sum(case["cases_passed"] for case in collisions), "runtime_symbol_cases": sum(case["cases_passed"] for case in runtime_symbols), "boxed_cases": sum(case["cases_passed"] for case in boxed_fields), "boxed_selector_cases": sum(case["selector_cases_passed"] for case in boxed_fields), "bytes_cases": sum(case["cases_passed"] for case in bytes_fields["profiles"]), "bytes_selector_cases": sum(case["selector_cases_passed"] for case in bytes_fields["profiles"]), "configuration_gaps": sum(p["gap_reproduced"] for p in probes), "alloc_only_gap": no_std["functional_gap_reproduced"]}))
    return 0 if report["harness_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
