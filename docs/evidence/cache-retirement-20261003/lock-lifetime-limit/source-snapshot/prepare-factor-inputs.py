"""Source-only preparation; no compiler or native tool identity execution."""
from pathlib import Path
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib

from factor_common import (DRIVER_SHA, SOURCES, digest_bytes, load_harness, load_json,
                           ordinary_file, require, safe_relative, sha, source_pins, utc, write_json)


def lock_subset(lock_text, package_name):
    """Retain the exact locked transitive pbrs package records, not latest registry versions."""
    parsed = tomllib.loads(lock_text)
    require(parsed["version"] == 4, "unexpected source lock version")
    entries = parsed["package"]
    todo, selected = [next(p for p in entries if p["name"] == "pbrs")], {}
    def identity(p):
        return (p["name"], p["version"], p.get("source", ""))
    while todo:
        p = todo.pop()
        if identity(p) in selected:
            continue
        selected[identity(p)] = p
        for dep in p.get("dependencies", []):
            parts = dep.split()
            candidates = [x for x in entries if x["name"] == parts[0]
                          and (len(parts) == 1 or x["version"] == parts[1])]
            require(len(candidates) == 1, "ambiguous/missing locked dependency: " + dep)
            todo.append(candidates[0])
    blocks = re.split(r"(?m)^\[\[package\]\]\n", lock_text)[1:]
    records = {}
    for block in blocks:
        item = tomllib.loads("[[package]]\n" + block)["package"][0]
        if identity(item) in selected:
            records[identity(item)] = "[[package]]\n" + block.rstrip() + "\n"
    require(set(records) == set(selected), "lock records omitted")
    driver = '[[package]]\nname = ' + json.dumps(package_name) + '\nversion = "0.0.0"\ndependencies = [\n "pbrs",\n]\n'
    text = '# Source-only subset of pinned shipping Cargo.lock; BUILD compatibility remains unqualified.\nversion = 4\n\n'
    all_records = {**records, (package_name, "0.0.0", ""): driver}
    text += "\n".join(all_records[k] for k in sorted(all_records))
    registry = sorted({(p["name"], p["version"], p["source"], p["checksum"])
                       for p in selected.values() if "source" in p})
    return text, registry


def project_source(repository, source, expected, output):
    output = Path(output)
    require(not output.exists(), "refusing existing source projection")
    require(len(expected) == 1027, "shipping source projection must have1027inputs")
    tree = subprocess.run(["git", "ls-tree", "-rz", "--full-tree", source, "--", *sorted(expected)],
                          cwd=repository, capture_output=True, timeout=30)
    require(tree.returncode == 0 and not tree.stderr, "Git source modes failed")
    modes = {}
    for entry in tree.stdout.split(b"\0"):
        if not entry:
            continue
        metadata, name = entry.split(b"\t", 1); mode, kind, _object = metadata.split()
        require(kind == b"blob" and mode in (b"100644", b"100755"), "unsupported shipping source mode")
        modes[name.decode()] = mode.decode()
    require(set(modes) == set(expected), "Git source mode projection set changed")
    output.mkdir(parents=True)
    request = "".join(source + ":" + name + "\n" for name in sorted(expected)).encode()
    proc = subprocess.Popen(["git", "cat-file", "--batch"], cwd=repository,
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    # communicate avoids pipe deadlock while obtaining only the authorized narrow objects.
    data, errors = proc.communicate(request, timeout=30)
    require(proc.returncode == 0 and not errors, "Git source projection failed")
    offset = 0
    for name in sorted(expected):
        end = data.index(b"\n", offset)
        header = data[offset:end].split()
        require(len(header) == 3 and header[1] == b"blob", "non-blob shipping source input")
        size = int(header[2]); start = end + 1
        payload = data[start:start + size]
        require(len(payload) == size and data[start + size:start + size + 1] == b"\n", "truncated source blob")
        require(digest_bytes(payload) == expected[name], "shipping blob SHA mismatch: " + name)
        path = output / safe_relative(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
        path.chmod(0o555 if modes[name] == "100755" else 0o444)
        offset = start + size + 1
    require(offset == len(data), "extra source blob response bytes")
    source_pins(output, expected)
    for directory in sorted((p for p in output.rglob("*") if p.is_dir()), reverse=True):
        directory.chmod(0o555)
    output.chmod(0o555)
    return modes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--legacy-inputs", required=True)
    parser.add_argument("--ordinary-report", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    here = Path(__file__).parent
    cfg = load_json(args.config)
    require(sha(args.config) == "b1c271e1dc4f2519fe06f99db71f201f65e8ecc07345e35308ba12ce400ea445",
            "reviewed ordinary source config changed")
    out = Path(args.out).absolute()
    require(not out.exists(), "refusing existing preparation output")
    out.mkdir(parents=True)
    prepared = {"schema": "gn03-factor-prepared-inputs/1", "status": "source_only_no_BUILD",
                "created_utc": utc(), "source_pair": SOURCES, "roots": {}, "source_sha256": {},
                "ordinary_config_sha256": sha(args.config), "source_git_modes": {}, "drivers": {}, "consumer_locks": {},
                "tools": None, "bootstrap_status": "not_run", "performance_status": "not_run"}
    for variant, ordinary_name in (("base", "baseline"), ("cand", "candidate")):
        source = cfg["source"][ordinary_name]
        require(source["commit"] == SOURCES[variant], "shipping source commit changed")
        root = out / "sources" / variant
        prepared["source_git_modes"][variant] = project_source(cfg["repository"], source["commit"], source["sha256"], root)
        prepared["roots"][variant] = str(root)
        prepared["source_sha256"][variant] = source["sha256"]
        h = load_harness(root)
        driver = out / "drivers" / variant
        (driver / "src").mkdir(parents=True)
        (driver / "src/main.rs").write_bytes((root / "bench/codegen/generator.rs").read_bytes())
        manifest = h.manifest("cg19-generator")
        (driver / "Cargo.toml").write_text(manifest)
        text, registry = lock_subset((root / "Cargo.lock").read_text(), "cg19-generator")
        (driver / "Cargo.lock").write_text(text)
        prepared["drivers"][variant] = {"root": str(driver), "manifest_sha256": sha(driver / "Cargo.toml"),
            "lock_sha256": sha(driver / "Cargo.lock"), "source_sha256": sha(driver / "src/main.rs"),
            "normalized_manifest": manifest.replace(str(root), "SOURCE_ROOT"),
            "registry_tuples": registry, "BUILD_status": "not_run",
            "lock_scope": "verbatim shipping transitive pbrs records plus driver; no Cargo validation yet"}
    require(prepared["drivers"]["base"]["lock_sha256"] == prepared["drivers"]["cand"]["lock_sha256"],
            "matched driver locks differ")
    require(prepared["drivers"]["base"]["normalized_manifest"] == prepared["drivers"]["cand"]["normalized_manifest"],
            "matched driver manifests differ beyond source root")
    base_root = Path(prepared["roots"]["base"])
    base_h = load_harness(base_root)
    prepared["consumer_main_sha256"] = {}
    for case in ("small", "100", "1000"):
        prepared["consumer_main_sha256"][case] = {str(marker): digest_bytes(base_h.render_consumer(base_h.CORPORA[case][0], marker).encode())
                                                    for marker in (0, 1)}
        text, registry = lock_subset((base_root / "Cargo.lock").read_text(), "cg19-consumer-" + case)
        path = out / "consumer-locks" / (case + ".lock")
        path.parent.mkdir(exist_ok=True); path.write_text(text)
        prepared["consumer_locks"][case] = {"path": str(path), "sha256": sha(path), "registry_tuples": registry,
                                           "scope": "expected registry graph only; original helper still generates its lock"}
    input_pins = load_json(here / "historical-request-proto-pins.json")
    prepared["inputs"] = {}
    for name, pin in input_pins.items():
        old = Path(args.legacy_inputs) / safe_relative(name)
        require(old.stat().st_size == pin["bytes"] and sha(ordinary_file(old)) == pin["sha256"], "archived input changed")
        new = out / "inputs" / safe_relative(name)
        new.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(old, new); new.chmod(0o444)
        require(sha(new) == pin["sha256"], "input copy changed")
        prepared["inputs"][name] = {**pin, "path": str(new)}
    report = load_json(args.ordinary_report)
    require(report["status"] == "passed", "ordinary correctness report is not passed")
    prepared["ordinary_report"] = {"path": str(Path(args.ordinary_report).absolute()), "sha256": sha(args.ordinary_report),
                                   "scope": "existing qualified ordinary evidence; not rerun here"}
    prepared["plugins"] = {}
    for variant, old in (("base", "baseline"), ("cand", "candidate")):
        stage = report["stages"]["stable-" + old]
        require(stage["status"] == "passed" and stage["source"]["commit"] == SOURCES[variant], "ordinary plugin source gate failed")
        g = stage["generator"]
        require(g["source_commit"] == SOURCES[variant] and sha(g["path"]) == g["sha256"], "ordinary plugin identity changed")
        prepared["plugins"][variant] = {**g, "scope": "reused immutable ordinary stdin plugin; no cg19 API substitution"}
    require(report["default30"]["status"] == "passed", "ordinary exact default30 oracle failed")
    labels = {"bdef": "baseline-default", "cdef": "candidate-default",
              "bshr": "baseline-shared", "cshr": "candidate-shared"}
    prepared["output_pins"] = {}
    for case in ("small", "100", "1000"):
        oracle = report["default30"]["cases"][case]
        require(oracle["status"] == "passed", "ordinary request oracle incomplete")
        prepared["output_pins"][case] = {factor: oracle["files"][label] for factor, label in labels.items()}
        require(oracle["files"]["baseline-default"] == oracle["files"]["archived-default"]
                == oracle["files"]["candidate-default"], "default output control differs")
        fds = prepared["inputs"][f"work/source-volume/{case}/fixture.fds"]
        require(oracle["fds_sha256"] == fds["sha256"], "canonical FDS pin differs")
    prepared["consumer_runtime_source"] = SOURCES["base"]
    prepared["consumer_ROOT"] = prepared["roots"]["base"]
    prepared["input_member_count"] = len(prepared["inputs"])
    prepared["driver_registry_tuple_count"] = len(prepared["drivers"]["base"]["registry_tuples"])
    write_json(out / "prepared.json", prepared)
    write_json(out / "preparation-seal.json", {"prepared_sha256": sha(out / "prepared.json"),
        "source_only": True, "source_files_per_variant": 1027, "input_files": len(input_pins),
        "no_compiler_native_probes_or_runtime": True, "observed_utc": utc()})
    print(json.dumps({"prepared": str(out / "prepared.json"), "registry_tuples": prepared["driver_registry_tuple_count"],
                      "source_files_per_variant": 1027, "input_files": len(input_pins), "status": prepared["status"]}))


if __name__ == "__main__":
    require(sys.dont_write_bytecode, "Run with python -B to keep source projections byte-exact")
    main()
