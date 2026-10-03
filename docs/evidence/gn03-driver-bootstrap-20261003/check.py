#!/usr/bin/env python3
"""Read-only bootstrap/source/graph auditor; never launches a native tool."""
import argparse
import hashlib
import json
from pathlib import Path
import stat
import tarfile
import tomllib

HERE = Path(__file__).resolve().parent
PAIR = {"base": "4b2384e728bce17601d56e43a887c354d66b9024",
        "cand": "5fe226b015a2c2bc667db976390b8337db6e9a2e"}
ORIGINAL = "26fe775fd5158cce97a34ce8b04ea396b9ba96298b5021c6dc786cd70b494750"
ACCEPTED = "125a1a43354730a599c4faa1c56e0f3a302ed20953efce249c0c53368347c0f2"
LIMIT = 2147483648


def require(value, message):
    if not value:
        raise ValueError(message)


def digest_bytes(data):
    return hashlib.sha256(data).hexdigest()


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for data in iter(lambda: stream.read(1 << 20), b""):
            value.update(data)
    return value.hexdigest()


def safe(name):
    value = Path(name)
    require(not value.is_absolute() and ".." not in value.parts, "unsafe member: " + name)
    return name


def capsule(index):
    path = HERE / "capsule.tar.gz"
    require(digest(path) == index["capsule_sha256"], "capsule hash differs")
    values = {}
    with tarfile.open(path, "r|gz") as archive:
        for member in archive:
            name = safe(member.name)
            require(member.isfile() and name not in values, "unexpected/duplicate capsule member")
            expected = index["members"][name]
            require(member.size == expected["bytes"] and member.mode == expected["mode"], "capsule size/mode differs: " + name)
            data = archive.extractfile(member).read()
            require(digest_bytes(data) == expected["sha256"], "capsule content differs: " + name)
            values[name] = data
    require(set(values) == set(index["members"]), "capsule member set differs")
    return values


def load(values, name):
    return json.loads(values[safe(name)])


def registry(data):
    return sorted((p["name"], p["version"], p["source"], p["checksum"])
                  for p in tomllib.loads(data.decode())["package"] if "source" in p)


def verify_metadata(values, prepared):
    graphs = []
    summary = load(values, "driver-graph-amendment-001/summary.json")
    require(summary["status"] == "pass" and summary["sources"] == PAIR, "graph summary identity differs")
    for variant in PAIR:
        prefix = "driver-graph-amendment-001/drivers/" + variant + "/"
        seed = registry(values[prefix + "raw-seed.Cargo.lock"])
        accepted = registry(values[prefix + "accepted.Cargo.lock"])
        require(len(seed) == 14 and seed == accepted, "registry tuples drifted")
        require(values[prefix + "Cargo.lock"] == values[prefix + "accepted.Cargo.lock"], "accepted lock changed")
        local = [p for p in tomllib.loads(values[prefix + "Cargo.lock"].decode())["package"] if "source" not in p]
        require({(p["name"], p["version"]) for p in local} == {("pbrs", "0.2.0"), ("cg19-generator", "0.0.0")}, "unexpected local lock node")
        records = []
        metadata = []
        for phase in ("normalize", "locked"):
            path = "driver-graph-amendment-001/captures/" + variant + "-" + phase + "/"
            record = load(values, path + "record.json")
            require(record["exit"] == 0 and record["source_tool_before"] == record["source_tool_after"], "metadata source/tool/exit invalid")
            require(record["source_tool_unchanged"], "metadata source/tool drift")
            argv = record["argv"]
            require(argv[1] == "metadata" and "--offline" in argv and ("--locked" in argv) == (phase == "locked"), "unexpected graph operation")
            for name in ("stdout", "stderr"):
                require(digest_bytes(values[path + name]) == record["raw_sha256"][name], "metadata raw hash differs")
            if phase == "locked":
                require(values[path + "before.Cargo.lock"] == values[path + "after.Cargo.lock"] == values[prefix + "Cargo.lock"], "locked graph changed lock")
            require(all(s["global_free"] >= LIMIT for s in record["samples"]), "metadata reserve failed")
            records.append(record)
            metadata.append(load(values, path + "stdout"))
        require(metadata[0]["resolve"] == metadata[1]["resolve"], "normalized/locked full graph differs")
        ids = {p["id"] for p in metadata[1]["packages"]}
        require({n["id"] for n in metadata[1]["resolve"]["nodes"]} <= ids, "unknown resolved package")
        root = prepared["roots"][variant]
        driver = prepared["drivers"][variant]["root"]
        require(records[1]["cwd"] == driver, "wrong metadata driver root")
        manifest = tomllib.loads(values[prefix + "Cargo.toml"].decode())
        require(manifest["dependencies"]["pbrs"] == {"path": root}, "driver features/path changed")
        normalized = json.dumps(metadata[1]["resolve"], sort_keys=True).replace(root, "SOURCE_ROOT").replace(driver, "DRIVER_ROOT")
        graphs.append(json.loads(normalized))
    require(graphs[0] == graphs[1] == summary["normalized_graph"], "paired nodes/edges/dep_kinds/features differ")


def verify_sources(values, prepared, index):
    origin = load(values, "independent-audit/git-source.json")
    require(origin["source_pair"] == PAIR and origin["status"] == "pass", "Git origin audit differs")
    for variant in PAIR:
        pins = prepared["source_sha256"][variant]
        modes = prepared["source_git_modes"][variant]
        require(len(pins) == 1027 and set(pins) == set(modes) == set(origin["entries"][variant]), "source projection set differs")
        prefix = "prepared-inputs/sources/" + variant + "/"
        actual = {name[len(prefix):] for name in values if name.startswith(prefix)}
        require(actual == set(pins), "source projection payload incomplete")
        for name, expected in pins.items():
            data = values[prefix + name]
            require(digest_bytes(data) == expected, "source SHA differs: " + name)
            entry = origin["entries"][variant][name]
            require(entry["mode"] == modes[name] and entry["sha256"] == expected, "Git source map differs")
            require(index["members"][prefix + name]["mode"] == (0o555 if entry["mode"] == "100755" else 0o444), "projected read-only source mode differs")
            object_sha = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
            require(object_sha == entry["oid"], "retained source differs from recorded Git blob")
        driver = prepared["drivers"][variant]
        prefix = "driver-graph-amendment-001/drivers/" + variant + "/"
        for name, field in (("Cargo.toml", "manifest_sha256"), ("Cargo.lock", "lock_sha256"), ("src/main.rs", "source_sha256")):
            require(digest_bytes(values[prefix + name]) == driver[field], "driver input hash differs")
        require(values[prefix + "src/main.rs"] == values["prepared-inputs/sources/" + variant + "/bench/codegen/generator.rs"], "driver API replaced")
        old = "prepared-inputs/drivers/" + variant + "/"
        require(values[old + "Cargo.toml"] == values[prefix + "Cargo.toml"] and values[old + "src/main.rs"] == values[prefix + "src/main.rs"], "graph amendment changed manifest/driver")
        require(values[old + "Cargo.lock"] == values[prefix + "raw-seed.Cargo.lock"], "original seeded lock changed")
    for name, row in prepared["inputs"].items():
        data = values["prepared-inputs/inputs/" + name]
        require(len(data) == row["bytes"] and digest_bytes(data) == row["sha256"], "frozen input changed: " + name)


def verify_builds(values, prepared, index):
    for attempt, reason in (("driver-bootstrap", b"no matching package named `bytes`"),
                            ("driver-bootstrap-002", b"--locked was passed")):
        path = "campaigns/" + attempt + "/bootstrap/base/"
        record = load(values, path + "BUILD.json")
        report = load(values, path + "summary.json")
        require(record["status"] == "failed" and record["prepared_sha256"] == ORIGINAL, "first failure relabeled")
        require(report["setup"]["driver_build"]["exit_code"] == 101, "first failure exit differs")
        require(reason in values[path + "logs/driver-build.stderr.log"], "first native failure missing")
    for variant in PAIR:
        path = "campaigns/driver-bootstrap-003/bootstrap/" + variant + "/"
        record = load(values, path + "BUILD.json")
        require(record["status"] == "passed" and record["source"] == PAIR[variant] and record["prepared_sha256"] == ACCEPTED, "BUILD source/status invalid")
        phase = record["phase"]
        require(phase["status"] == "passed" and phase["exit_code"] == 0 and phase["command"] == record["argv"], "native BUILD operation failed")
        require(load(values, path + "summary.json")["setup"]["driver_build"] == phase, "raw BUILD phase differs")
        owners = load(values, path + "processes.json")["owners"]
        require(len(owners) == 1 and owners[0]["exit_code"] == 0 and owners[0]["phase"] == "driver_build", "actual owned process result differs")
        require(owners[0]["argv"] == ["/usr/bin/time", "-v", *record["argv"]] and owners[0]["environment"] == record["environment"], "actual wrapper/argument/environment differs")
        argv = record["argv"]
        require(argv[1:4] == ["build", "--offline", "--locked"] and argv[-2:] == ["--bin", "cg19-generator"] and "--release" not in argv, "BUILD flags changed")
        require("--message-format=json" not in argv, "artifact JSON provenance was not captured")
        env = record["environment"]
        require(env["CARGO_BUILD_JOBS"] == "1" and env["CARGO_NET_OFFLINE"] == "true", "jobs/offline changed")
        require(env["CARGO_HOME"] == "/workspace/pure-protobuf/work/toolchain/cargo" and env["RUSTUP_HOME"] == "/workspace/pure-protobuf/work/toolchain/rustup", "actual homes differ")
        for name, value in env.items():
            if name.startswith("CARGO_PROFILE_") or name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CC", "CXX", "AR", "LD"):
                require(value in (None, ""), "compiler/profile override: " + name)
        envelope = load(values, path + "tool-envelope.json")
        require(digest_bytes(values[path + "tool-envelope.json"]) == record["tools_sha256"], "tool envelope hash differs")
        require(envelope["config_context_before"] == envelope["config_context_after"] and envelope["sysroot_context_before"] == envelope["sysroot_context_after"], "prepared tool context drift")
        require("1.99.0" in envelope["tools"]["cargo"]["version_raw"] and "1.99.0" in envelope["tools"]["rustc"]["version_raw"], "wrong actual compiler")
        require(envelope["tools"]["protoc"]["sha256"] == "ba5165ada96fc34d1295b2056ab8ea99756f7896a0ef0449d07f721595702b28", "protoc pin differs")
        for name, expected in record["components_sha256"].items():
            require(digest_bytes(values[name]) == expected, "executed helper differs: " + name)
        resources = load(values, path + "resource.json")
        require(resources["cap_bytes"] == resources["floor_bytes"] == LIMIT and not resources["failures"], "BUILD resource guard failed")
        require(all(s["global_free_bytes"] >= LIMIT and s["allocated_cache_bytes"] <= LIMIT for s in resources["samples"]), "BUILD resource samples failed")
        artifact = record["artifact"]
        declared = index["external_payloads"]["campaigns/driver-bootstrap-003/bin/cg19-generator-" + variant]
        require(declared["sha256"] == artifact["sha256"] and declared["bytes"] == artifact["bytes"], "retained ELF pin differs")
        preservation = load(values, path + "preservation/preservation.json")
        require(preservation["status"] == "verified_retired" and preservation["retired"] and preservation["hash_only_cache_files"] == 0, "cache preservation incomplete")
        manifest = load(values, path + "preservation/manifest.json")
        require(digest_bytes(values[path + "preservation/manifest.json"]) == preservation["manifest_sha256"], "cache manifest pin differs")
        require(len(manifest) == 603 and sum(r["kind"] == "file" for r in manifest.values()) == 549, "cache scope differs")
        require(all(r["disposition"] == "archived" for r in manifest.values()), "hash-only cache payload")
        external = index["external_payloads"][path + "preservation/payloads.tar.gz"]
        require(external["sha256"] == preservation["archive_sha256"], "archive index differs from recorded verification")
        for name, field in (("all-files.json", "all_files_sha256"), ("payloads.json", "payloads_sha256")):
            require(digest_bytes(values[path + "preservation/" + name]) == preservation[field], "payload inventory pin differs")
        removed = load(values, path + "preservation/removed-paths.json")
        require(len(removed) == len(set(removed)) == len(manifest) and set(removed) == set(manifest), "retired path set differs")
        require(load(values, path + "preservation/fresh-live-verification.json")["complete_path_inode_mode_hashes_pass"], "fresh live verification missing")
        for name in ("users-before-locks.json", "users-after-locks.json", "users-immediately-before-retirement.json"):
            require(not load(values, path + "preservation/" + name)["matches"], "visible cache user retained")
        samples = [json.loads(line) for line in values[path + "preservation/preservation-resources.jsonl"].splitlines()]
        require(samples and all(s["cap_bytes"] == s["floor_bytes"] == LIMIT and s["global_free_bytes"] >= LIMIT and s["allocated_cache_bytes"] <= LIMIT for s in samples), "preservation resource guard failed")
        require(manifest["debug/cg19-generator"]["sha256"] == artifact["sha256"], "retained ELF differs from full cache")
        require(load(values, path + "preservation/source-before.json") == load(values, path + "preservation/source-after.json"), "retirement source drift")
        locks = load(values, path + "preservation/held-locks.json")["locks"]
        require(len(locks) == 1 and locks[0]["ofd_abi"]["command"] == 37 and "fcntl_OFD" in locks[0], "held OFD scope missing")
        for name in ("contenders-after-archive.json", "contenders-immediately-before-retirement.json"):
            rows = load(values, path + "preservation/" + name)
            require(len(rows) == 3 and all(r["exit_code"] == 0 and json.loads(r["stdout"])["observed"] == "blocked" for r in rows), "lock lifetime observation failed")


def verify_payloads(values, index, root):
    results = {}
    for variant in PAIR:
        prefix = "campaigns/driver-bootstrap-003/bootstrap/" + variant + "/preservation/"
        manifest = load(values, prefix + "manifest.json")
        path = root / (prefix + "payloads.tar.gz")
        expected = index["external_payloads"][prefix + "payloads.tar.gz"]
        require(path.stat().st_size == expected["bytes"] and digest(path) == expected["sha256"], "cache archive hash differs")
        seen = {}
        with tarfile.open(path, "r|gz") as archive:
            for member in archive:
                name = "." if member.name == "cache" else member.name.removeprefix("cache/")
                require(member.name == "cache" or member.name.startswith("cache/"), "cache member prefix differs")
                safe(name)
                require(name in manifest and name not in seen, "unexpected cache member")
                row = manifest[name]; identity = row["identity"]
                require(member.mode == stat.S_IMODE(identity["st_mode"]) and member.uid == identity["st_uid"] and member.gid == identity["st_gid"], "cache mode/ownership differs")
                if row["kind"] == "directory":
                    require(member.isdir(), "cache directory type differs")
                elif row["kind"] == "symlink":
                    require(member.issym() and member.linkname == row["link_target"], "cache symlink differs")
                elif member.islnk():
                    target = member.linkname.removeprefix("cache/")
                    require(target in seen and target in row["hardlink_aliases"], "hardlink topology differs")
                    require(manifest[target]["sha256"] == row["sha256"] and
                            (manifest[target]["identity"]["st_dev"], manifest[target]["identity"]["st_ino"]) == (identity["st_dev"], identity["st_ino"]), "hardlink content/identity differs")
                else:
                    require(member.isfile() and member.size == identity["st_size"], "cache regular type/size differs")
                    require(not any(alias in seen for alias in row["hardlink_aliases"]), "hardlink alias became independent payload")
                    stream = archive.extractfile(member); value = hashlib.sha256(); count = 0
                    for chunk in iter(lambda: stream.read(1 << 20), b""):
                        value.update(chunk); count += len(chunk)
                    require(count == member.size and value.hexdigest() == row["sha256"], "cache payload content differs")
                seen[name] = True
        require(set(seen) == set(manifest), "cache member set differs")
        results[variant] = {"members": len(seen), "regular_paths": 549, "archive_sha256": expected["sha256"]}
    for name, row in index["external_payloads"].items():
        if name.startswith("campaigns/driver-bootstrap-003/bin/"):
            path = root / name
            require(path.stat().st_size == row["bytes"] and digest(path) == row["sha256"], "immutable retained driver differs")
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--payload-root", type=Path, help="original local proposal root; verify both full cache archives and retained ELFs")
    args = parser.parse_args()
    index = json.loads((HERE / "index.json").read_text())
    values = capsule(index)
    require(digest_bytes(values["prepared-inputs/prepared.json"]) == ORIGINAL, "original preparation relabeled")
    prepared = load(values, "driver-graph-amendment-001/prepared.json")
    require(digest_bytes(values["driver-graph-amendment-001/prepared.json"]) == ACCEPTED and prepared["source_pair"] == PAIR, "accepted preparation differs")
    verify_sources(values, prepared, index)
    verify_metadata(values, prepared)
    verify_builds(values, prepared, index)
    payloads = verify_payloads(values, index, args.payload_root) if args.payload_root else "not_read; indexed local payloads only"
    print(json.dumps({"status": "pass", "capsule_members": len(values), "source_files_per_variant": 1027,
                      "metadata_operations": 4, "registry_tuples": 14, "BUILD_passes": 2,
                      "retained_native_failures": 2, "full_payload_readback": payloads,
                      "release_screens_repeats_performance": "NOT_RUN"}, indent=2))


if __name__ == "__main__":
    main()
