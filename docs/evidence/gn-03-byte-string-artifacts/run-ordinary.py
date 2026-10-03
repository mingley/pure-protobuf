#!/usr/bin/env python3
"""Work-only GN03 ordinary gates. Default is read-only preflight, never Cargo.

Execution requires a separately supplied root lease record. This is not a
performance coordinator. Frozen old campaigns and shipping worktrees are read
only; every compiler writes into a new, runner-owned source snapshot.
"""
import argparse
import datetime as dt
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import time
import traceback

HERE = Path(__file__).resolve().parent
CONFIG = HERE / "sealed-config.json"
GIB = 1024 ** 3
TEST = "codegen::reflection::tests::"
HEX_TEST = TEST + "fds_hex_block_roundtrips_bytes"
BYTE_TEST = TEST + "shared_byte_string_matches_a_compiled_literal_for_every_byte"
OWNER = b"pub const FILE_DESCRIPTOR_SET: &[u8] = "
HELPER = "__pbrs_shared_descriptors.rs"
COMPILERS = {"cargo", "rustc", "cargo-clippy", "clippy-driver", "cargo-miri", "miri"}
UNSUPPORTED_ENV = ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP"]


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def sha(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        while chunk := f.read(1024 * 1024):
            h.update(chunk)
    return h.hexdigest()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def require(condition, reason):
    if not condition:
        raise RuntimeError(reason)


def under(path, roots):
    return any(path == str(root) or path.startswith(str(root) + "/") for root in roots)


def processes(roots=()):
    """Visible /proc evidence only. Zombies are distinct from live compiler tasks."""
    active, owners, zombies, denied = [], [], [], {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit() or int(entry.name) == os.getpid():
            continue
        try:
            status = (entry / "status").read_text()
            state = re.search(r"^State:\s+(\w)", status, re.M).group(1)
            name = (entry / "comm").read_text().strip()
            cmdline = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except (OSError, AttributeError):
            denied["status_or_cmdline"] = denied.get("status_or_cmdline", 0) + 1
            continue
        if state == "Z":
            if name in COMPILERS:
                zombies.append({"pid": int(entry.name), "name": name, "state": state})
            continue
        row = {"pid": int(entry.name), "name": name, "state": state}
        if name in COMPILERS:
            active.append({**row, "cmdline": cmdline})
        links = []
        for field in ["exe", "cwd"]:
            try:
                links.append((field, os.readlink(entry / field)))
            except OSError:
                denied[field] = denied.get(field, 0) + 1
        if roots:
            try:
                for fd in (entry / "fd").iterdir():
                    try:
                        links.append(("fd", os.readlink(fd)))
                    except OSError:
                        denied["fd_link"] = denied.get("fd_link", 0) + 1
            except OSError:
                denied["fd_directory"] = denied.get("fd_directory", 0) + 1
            matches = [{"field": field, "path": value} for field, value in links if under(value.removesuffix(" (deleted)"), roots)]
            # Cargo/rustc argv also names targets and output paths.
            if name in COMPILERS and any(str(root) in cmdline for root in roots):
                matches.append({"field": "cmdline", "path": cmdline})
            if matches:
                owners.append({**row, "matches": matches})
    return {"observed_at_utc": utc(), "active_compilers": active, "visible_cache_owners": owners,
            "defunct_compilers": zombies, "visibility_denials": denied,
            "scope_limit": "accessible status/cmdline/exe/cwd/fd links only; mappings, environments and inaccessible process internals are not proved"}


def group_members(group):
    members = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            tail = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            if int(tail[2]) == group and int(tail[3]) == group:
                members.append({"pid": int(entry.name), "state": tail[0]})
        except (OSError, ValueError, IndexError):
            # Compiler process paths routinely disappear between /proc reads.
            continue
    return members


def terminate_group(proc):
    """Only the session/process group created by this Popen, including orphans."""
    before = group_members(proc.pid)
    sent = []
    def send(sig):
        try:
            os.killpg(proc.pid, sig)
            sent.append(signal.Signals(sig).name)
        except ProcessLookupError:
            pass
    if proc.poll() is None or any(p["state"] != "Z" for p in before):
        send(signal.SIGTERM)
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        send(signal.SIGKILL)
        proc.wait(timeout=5)
    deadline = time.monotonic() + 5
    while any(p["state"] != "Z" for p in group_members(proc.pid)) and time.monotonic() < deadline:
        time.sleep(0.1)
    after = group_members(proc.pid)
    if any(p["state"] != "Z" for p in after):
        send(signal.SIGKILL)
        time.sleep(0.1)
        after = group_members(proc.pid)
    return {"owned_session_and_group": proc.pid, "before": before, "signals_sent": sent, "after": after,
            "visible_live_members_remaining": [p for p in after if p["state"] != "Z"],
            "scope_limit": "accessible /proc stat group/session identity; missing paths treated as exited; defunct rows are not live tasks"}


def accounting(roots, filesystem):
    seen, allocated, apparent, paths = set(), 0, 0, 0
    for root in roots:
        if not root.exists():
            continue
        for base, dirs, files in os.walk(root, followlinks=False):
            for path in [Path(base), *[Path(base) / n for n in files], *[Path(base) / n for n in dirs if (Path(base) / n).is_symlink()]]:
                try:
                    s = path.lstat()
                except FileNotFoundError:
                    continue
                paths += 1
                apparent += s.st_size
                key = (s.st_dev, s.st_ino)
                if key not in seen:
                    allocated += s.st_blocks * 512
                    seen.add(key)
    v = os.statvfs(filesystem)
    return {"observed_at_utc": utc(), "owned_target_roots": [str(p) for p in roots],
            "allocated_unique_inode_bytes": allocated, "apparent_path_sum_bytes": apparent,
            "path_count": paths, "global_available_bytes": v.f_bavail * v.f_frsize,
            "accounting_limit": "st_blocks allocated paths, de-duplicated by device/inode; not exclusive overlay backing-store attribution"}


def verify_preflight(cfg):
    require(not {k: os.environ[k] for k in UNSUPPORTED_ENV if os.environ.get(k)}, "unsupported inherited compiler flags/wrappers; do not silently override")
    profile_keys = {k: v for k, v in os.environ.items() if k.startswith("CARGO_PROFILE_")}
    require(all(k in ["CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"] and v == "0" for k, v in profile_keys.items()), "unsupported inherited Cargo profile override")
    require(sha(cfg["plan_path"]) == cfg["plan_sha256"], "frozen qualification plan changed")
    require(sha(cfg["baseline_overlay_path"]) == cfg["baseline_overlay_sha256"], "baseline test overlay changed")
    for tool in cfg["tools"].values():
        for path, expected in tool["sha256"].items():
            require(sha(path) == expected, "tool pin changed: " + path)
    for name, expected in cfg["legacy_evidence"]["sha256"].items():
        require(sha(Path(cfg["legacy_evidence"]["root"]) / name) == expected, "legacy artifact changed: " + name)
    repo = cfg["repository"]
    for variant, source in cfg["source"].items():
        for name, expected in source["sha256"].items():
            blob = subprocess.check_output(["git", "show", source["commit"] + ":" + name], cwd=repo)
            require(hashlib.sha256(blob).hexdigest() == expected, f"{variant} source pin changed: {name}")
    # Candidate shipping worktree must also still be the reviewed pin.
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    require(head == cfg["source"]["candidate"]["commit"], "candidate worktree HEAD changed")
    plan = json.loads(Path(cfg["plan_path"]).read_text())
    for name, expected in plan["candidate_source_sha256"].items():
        require(sha(Path(repo) / name) == expected, "candidate working source changed: " + name)
    require(sha(Path(repo) / "tests/codegen_shared_descriptors.rs") == plan["candidate_test_source_sha256"], "candidate consumer oracle changed")
    require(not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=repo), "tracked candidate changes present")


def safe_name(name):
    p = Path(name)
    require(not p.is_absolute() and ".." not in p.parts, "unsafe retained filename: " + name)
    return p


def snapshot(cfg, variant, root):
    root.mkdir(parents=True, exist_ok=False)
    source = cfg["source"][variant]
    capsule = subprocess.check_output(["git", "archive", "--format=tar", source["commit"], "--", *cfg["source_paths"]], cwd=cfg["repository"])
    with tarfile.open(fileobj=io.BytesIO(capsule)) as t:
        for member in t.getmembers():
            safe_name(member.name)
            require(member.isfile() or member.isdir(), "source capsule symlink/special entry refused")
        t.extractall(root, filter="data")
    for name, expected in source["sha256"].items():
        require(sha(root / name) == expected, "snapshot source mismatch: " + name)
    if variant == "baseline":
        shutil.copyfile(cfg["baseline_overlay_path"], root / "tests/codegen_shared_descriptors.rs")
    return {"commit": source["commit"], "capsule_sha256": hashlib.sha256(capsule).hexdigest(),
            "tracked_source_files": len(source["sha256"]), "test_overlay_sha256": sha(root / "tests/codegen_shared_descriptors.rs")}


def verify_snapshot(cfg, variant, root):
    for name, expected in cfg["source"][variant]["sha256"].items():
        if variant == "baseline" and name == "tests/codegen_shared_descriptors.rs":
            expected = cfg["baseline_overlay_sha256"]
        require(sha(root / name) == expected, "compiled source/overlay changed: " + name)


WRAPPER = '''#!/usr/bin/env python3
import datetime,json,os,sys
from pathlib import Path
args=sys.argv[1:]
if args and args[0] in ["build","test","run","clippy","check"]:
    separator=args.index("--") if "--" in args else len(args)
    cargo_args,child_args=args[:separator],args[separator:]
    if "--offline" not in cargo_args: cargo_args.append("--offline")
    cargo_args += ["--jobs","1"]
    args=cargo_args+child_args
os.environ["CARGO_BUILD_JOBS"]="1"
row={"at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"cwd":str(Path.cwd()),"argv":[os.environ["GN03_ACTUAL_CARGO"],*args],"environment":{k:os.environ.get(k) for k in ["CARGO_BUILD_JOBS","CARGO_TARGET_DIR","RUSTC","RUSTDOC","RUSTUP_TOOLCHAIN","CARGO_INCREMENTAL"]}}
with open(os.environ["GN03_CARGO_INVOCATIONS"],"a") as f: f.write(json.dumps(row)+"\\n")
os.execv(os.environ["GN03_ACTUAL_CARGO"],[os.environ["GN03_ACTUAL_CARGO"],*args])
'''


class Run:
    def __init__(self, cfg, campaign, lease):
        self.cfg, self.campaign = cfg, campaign
        self.roots = [campaign / "snapshots" / (s["tool"] + "-" + s["variant"]) / "target" for s in cfg["stages"]]
        self.report = {"schema": "gn03-byte-string-ordinary/1", "scope": "ordinary correctness qualification only; no performance measurements", "start_utc": utc(),
                       "lease": lease, "sealed_config_sha256": sha(CONFIG), "runner_sha256": sha(__file__), "status": "running", "stages": {}, "default30": {"status": "not_run"}}
        self.generators = {}
        self.save()

    def save(self):
        write_json(self.campaign / "report.json", self.report)

    def resource(self):
        return accounting(self.roots, self.campaign)

    def check_resource(self, row):
        guard = self.cfg["guard"]
        require(row["allocated_unique_inode_bytes"] <= guard["owned_allocated_cache_limit_bytes"], "owned allocated target cap crossed")
        require(row["global_available_bytes"] >= guard["global_free_reserve_bytes"], "global free reserve crossed")

    def command(self, name, argv, cwd, env, stdin=None):
        directory = self.campaign / "phases" / name
        directory.mkdir(parents=True, exist_ok=False)
        rec = {"status": "running", "command": argv, "cwd": str(cwd), "start_utc": utc(), "environment": {k: env.get(k) for k in ["PATH", "RUSTC", "RUSTDOC", "RUSTUP_TOOLCHAIN", "CARGO_HOME", "CARGO_INCREMENTAL", "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"]}}
        if stdin:
            rec["stdin"] = {"path": str(stdin), "sha256": sha(stdin)}
        write_json(directory / "command.json", rec)
        initial = self.resource()
        start, reason, proc, caught = time.monotonic(), None, None, None
        summary = {"samples": 0, "maximum_owned_allocated_bytes": 0, "minimum_global_available_bytes": None, "guard_failures": []}
        def sample(telemetry):
            row = self.resource()
            row.update({"elapsed_seconds": time.monotonic() - start,
                        "owned_cache_within_cap": row["allocated_unique_inode_bytes"] <= self.cfg["guard"]["owned_allocated_cache_limit_bytes"],
                        "global_free_reserve_present": row["global_available_bytes"] >= self.cfg["guard"]["global_free_reserve_bytes"],
                        "within_phase_timeout": time.monotonic() - start <= self.cfg["guard"]["phase_timeout_seconds"]})
            summary["samples"] += 1
            summary["maximum_owned_allocated_bytes"] = max(summary["maximum_owned_allocated_bytes"], row["allocated_unique_inode_bytes"])
            summary["minimum_global_available_bytes"] = row["global_available_bytes"] if summary["minimum_global_available_bytes"] is None else min(summary["minimum_global_available_bytes"], row["global_available_bytes"])
            if not all(row[k] for k in ["owned_cache_within_cap", "global_free_reserve_present", "within_phase_timeout"]):
                summary["guard_failures"].append(row)
            telemetry.write(json.dumps(row) + "\n"); telemetry.flush()
            self.check_resource(row)
            require(row["within_phase_timeout"], "ordinary phase timeout")
        with (directory / "stdout.log").open("wb") as out, (directory / "stderr.log").open("wb") as err, (directory / "resource.jsonl").open("w") as telemetry:
            inp = Path(stdin).open("rb") if stdin else subprocess.DEVNULL
            try:
                sample(telemetry)
                proc = subprocess.Popen(argv, cwd=cwd, env=env, stdin=inp, stdout=out, stderr=err, start_new_session=True)
                rec.update({"pid": proc.pid, "process_group": proc.pid})
                write_json(directory / "command.json", rec)
                while True:
                    sample(telemetry)
                    if proc.poll() is not None:
                        break
                    time.sleep(self.cfg["guard"]["sample_seconds"])
            except BaseException as exc:
                caught = exc
                reason = str(exc) or type(exc).__name__
            finally:
                if proc is not None:
                    try:
                        cleanup = terminate_group(proc)
                        rec["owned_child_cleanup"] = cleanup
                        if cleanup["visible_live_members_remaining"]:
                            caught = caught or RuntimeError("owned child group has visible live members after cleanup")
                            reason = str(caught)
                    except BaseException as exc:
                        rec["owned_child_cleanup"] = {"status": "failed", "group": proc.pid, "error": str(exc), "cache_retirement_must_refuse_live_owners": True}
                        caught = caught or exc
                        reason = str(caught) or type(caught).__name__
                    rec["exit_code"] = proc.returncode
                else:
                    rec["exit_code"] = None
                if stdin:
                    inp.close()
                rec.update({"end_utc": utc(), "status": "guard_failed" if summary["guard_failures"] else "helper_failed" if caught else "passed" if proc.returncode == 0 else "failed", "guard_reason": reason,
                            "initial_resource": initial, "sample_summary": summary, "sampling_limit": "observed samples only; between-sample peaks and continuous bounds are not certified"})
                if caught:
                    (directory / "helper.traceback.log").write_text("".join(traceback.format_exception(caught)))
                write_json(directory / "command.json", rec)
        write_json(directory / "command.json", rec)
        print(name, rec["status"], rec["exit_code"], rec["end_utc"], flush=True)
        if caught:
            raise caught
        require(proc.returncode == 0, f"{name}: exit {proc.returncode}")
        return directory

    def stage(self, stage):
        tool, variant = stage["tool"], stage["variant"]
        key = tool + "-" + variant
        root = self.campaign / "snapshots" / key
        info = self.report["stages"][key] = {"status": "preparing", "source": snapshot(self.cfg, variant, root), "gates": {n: {"status": "not_run"} for n in ["tool_identity", "hex_unit", "byte_unit", "consumer_suite", "shipping_clippy", "consumer_clippy", "harness_contracts", "compiled_generator", "archive_and_retirement"]}}
        self.save()
        native = Path(self.cfg["tools"][tool]["root"])
        wrapper = self.campaign / "wrappers" / key
        wrapper.mkdir(parents=True)
        (wrapper / "cargo").write_text(WRAPPER); (wrapper / "cargo").chmod(0o700)
        target_name = "gn03-byte-string-msrv" if tool == "msrv185" else "gn03-byte-string-correctness"
        target = root / "target" / target_name
        env = os.environ.copy()
        for name in UNSUPPORTED_ENV:
            env.pop(name, None)
        env.update({"PATH": str(wrapper) + ":" + str(native / "bin") + ":" + env["PATH"], "RUSTC": str(native / "bin/rustc"), "RUSTDOC": str(native / "bin/rustdoc"),
                    "RUSTUP_TOOLCHAIN": native.name, "CARGO_HOME": self.cfg["cargo_home"], "RUSTUP_HOME": self.cfg["rustup_home"], "CARGO_BUILD_JOBS": "1", "CARGO_INCREMENTAL": "0",
                    "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0", "CARGO_TARGET_DIR": str(target), "GN03_ACTUAL_CARGO": str(native / "bin/cargo"),
                    "GN03_CARGO_INVOCATIONS": str(self.campaign / "phases" / (key + "-cargo-invocations.jsonl"))})
        info["environment_controls"] = {"set": {name: {"inherited": os.environ.get(name), "applied": env[name]} for name in env if env[name] != os.environ.get(name)},
                                         "removed_empty_or_unset": {name: os.environ.get(name) for name in UNSUPPORTED_ENV},
                                         "policy": "nonempty unsupported compiler/profile overrides refuse preflight; explicit common dev/test debug0 and incremental0 match both source variants, not a release/performance profile"}
        cargo = str(wrapper / "cargo")
        common = ["--locked", "--offline", "-p", "pbrs", "--no-default-features", "--features", "codegen", "--target-dir", str(target)]
        def gate(name, argv, expected=None):
            verify_snapshot(self.cfg, variant, root)
            info["gates"][name] = {"status": "running", "argv": argv}; self.save()
            directory = self.command(key + "-" + name, argv, root, env)
            if expected:
                text = (directory / "stdout.log").read_text()
                require(re.search(r"test result: ok\. " + str(expected) + r" passed; 0 failed;", text), f"{name}: expected nonzero test count {expected} absent")
            verify_snapshot(self.cfg, variant, root)
            info["gates"][name] = {"status": "passed", "phase": str(directory), "expected_tests": expected}; self.save()
            return directory
        error = None
        try:
            directory = gate("tool_identity", [str(native / "bin/rustc"), "--version", "--verbose"])
            version = (directory / "stdout.log").read_text()
            require(version.startswith(self.cfg["tools"][tool]["expected_rustc_version_prefix"]), "actual native rustc version does not match stage")
            self.command(key + "-cargo-version", [str(native / "bin/cargo"), "--version", "--verbose"], root, env)
            if tool in ["stable", "strict188"]:
                self.command(key + "-clippy-driver-version", [str(native / "bin/clippy-driver"), "--version"], root, env)
            gate("hex_unit", [cargo, "test", *common, "--lib", HEX_TEST, "--", "--exact", "--test-threads=1"], 1)
            if variant == "candidate":
                gate("byte_unit", [cargo, "test", *common, "--lib", BYTE_TEST, "--", "--exact", "--test-threads=1"], 1)
            else:
                info["gates"]["byte_unit"] = {"status": "not_applicable", "reason": "baseline contains no byte-string renderer; old hex unit includes every byte and empty input"}
            gate("consumer_suite", [cargo, "test", *common, "--test", "codegen_shared_descriptors", "--", "--test-threads=1"], self.cfg["expected_integration_tests"])
            # Retained expected-red is checked by the seven-test suite, not relabelled.
            fixtures = root / "target/gn03-tests"
            for name in self.cfg["strict_consumer_names"]:
                require((fixtures / name / "compile-status.txt").read_text().strip() == "exit status: 0", "missing successful actual consumer: " + name)
                require((fixtures / name / "Cargo.lock").is_file(), "consumer lock not retained: " + name)
            failure = (fixtures / self.cfg["consumer_expected_failure"] / "compile-status.txt").read_text().strip()
            require(failure != "exit status: 0", "intentional missing-helper failure unexpectedly succeeded")
            info["intentional_consumer_red"] = {"profile": self.cfg["consumer_expected_failure"], "actual_status": failure, "expected_missing_helper": True, "raw_logs_retained": True}
            info["consumer_locks"] = {name: sha(fixtures / name / "Cargo.lock") for name in self.cfg["strict_consumer_names"]}
            info["consumer_main_sources"] = {name: sha(fixtures / name / "src/main.rs") for name in self.cfg["strict_consumer_names"]}
            info["consumer_normalized_manifests"] = {name: hashlib.sha256((fixtures / name / "Cargo.toml").read_bytes().replace(str(root).encode(), b"<PINNED_RUNTIME_ROOT>")).hexdigest() for name in self.cfg["strict_consumer_names"]}
            if variant == "candidate":
                baseline = self.report["stages"][tool + "-baseline"]
                for attribute in ["consumer_locks", "consumer_main_sources", "consumer_normalized_manifests"]:
                    require(info[attribute] == baseline[attribute], "matched consumer fixtures differ: " + attribute)
                info["matched_consumer_fixture_equality"] = "locks, independent oracle source, and manifests after only absolute runtime-root normalization equal baseline"
            if tool in ["stable", "strict188"]:
                gate("shipping_clippy", [cargo, "clippy", *common, "--lib", "--bin", "protoc-gen-pbrs", "--test", "codegen_shared_descriptors", "--", "-D", "warnings"])
                info["gates"]["consumer_clippy"] = {"status": "running", "profiles": {name: {"status": "not_run"} for name in self.cfg["strict_consumer_names"]}}; self.save()
                consumer_env = {**env, "CARGO_TARGET_DIR": str(root / "target/integration-consumers")}
                for name in self.cfg["strict_consumer_names"]:
                    directory = self.command(key + "-clippy-" + name, [cargo, "clippy", "--locked", "--offline", "--manifest-path", str(fixtures / name / "Cargo.toml"), "--all-targets", "--", "-D", "warnings"], root, consumer_env)
                    info["gates"]["consumer_clippy"]["profiles"][name] = {"status": "passed", "phase": str(directory)}; self.save()
                info["gates"]["consumer_clippy"]["status"] = "passed"
            else:
                for name in ["shipping_clippy", "consumer_clippy"]:
                    info["gates"][name] = {"status": "not_applicable", "reason": "native1.85 semantic/MSRV stage; strict native1.88 has its own gates"}
            if tool == "stable":
                gate("compiled_generator", [cargo, "build", *common, "--bin", "protoc-gen-pbrs"])
                binary = self.campaign / "generators" / (variant + "-protoc-gen-pbrs")
                binary.parent.mkdir(exist_ok=True); shutil.copyfile(target / "debug/protoc-gen-pbrs", binary); binary.chmod(0o500)
                self.generators[variant] = binary
                info["generator"] = {"path": str(binary), "bytes": binary.stat().st_size, "sha256": sha(binary), "source_commit": self.cfg["source"][variant]["commit"], "tool": tool, "build_profile": "debug ordinary bootstrap, excluded from any future performance timing"}
            else:
                info["gates"]["compiled_generator"] = {"status": "not_applicable", "reason": "source-matched ordinary copied plugins built in stable stages"}
            if tool == "stable" and variant == "candidate":
                directory = gate("harness_contracts", [sys.executable, "-B", "-m", "unittest", "discover", "-s", "bench/codegen", "-p", "test_*.py"])
                require(re.search(r"Ran [1-9][0-9]* tests?", (directory / "stderr.log").read_text()), "harness suite executed zero tests")
            else:
                info["gates"]["harness_contracts"] = {"status": "not_applicable", "reason": "unchanged harness checked once in candidate stable stage"}
            info["status"] = "passed"
        except BaseException as exc:
            error = exc; info["status"] = "failed"; info["error"] = str(exc)
            for gate_info in info["gates"].values():
                if gate_info["status"] == "running":
                    gate_info["status"] = "failed"
                elif gate_info["status"] == "not_run":
                    gate_info["reason"] = "earlier gate/guard stopped this stage"
            raise
        finally:
            self.save()
            try:
                info["gates"]["archive_and_retirement"] = self.preserve_retire(root, key, target_name)
            except BaseException as exc:
                info["gates"]["archive_and_retirement"] = {"status": "failed", "cache_retained": True, "error": str(exc)}
                self.save()
                if error is None:
                    raise
            self.save()

    def preserve_retire(self, root, key, target_name):
        target = root / "target"
        allowed = {target_name, "integration-consumers", "gn03-tests"}
        require(set(p.name for p in target.iterdir()) <= allowed if target.exists() else True, "unreviewed target child; cache reclamation refused")
        scopes = [target / name for name in sorted(allowed) if (target / name).exists()]
        before = processes(scopes)
        require(not before["visible_cache_owners"] and not before["active_compilers"], "live cache owner/compiler; reclamation refused")
        directory = self.campaign / "retained" / key
        directory.mkdir(parents=True, exist_ok=False)
        write_json(directory / "process-before.json", before)
        self.check_resource(self.resource())
        inventory, payload = {}, []
        for scope in scopes:
            for path in sorted(scope.rglob("*")):
                if not path.is_file():
                    continue
                require(not path.is_symlink(), "target symlink payload refused")
                name = str(path.relative_to(root)); s = path.stat()
                inventory[name] = {"bytes": s.st_size, "allocated_bytes": s.st_blocks * 512, "sha256": sha(path), "mode": stat.S_IMODE(s.st_mode)}
                with path.open("rb") as f:
                    elf = f.read(4) == b"\x7fELF"
                if "gn03-tests" in path.parts or ".fingerprint" in path.parts or elf:
                    payload.append(path)
        write_json(directory / "all-target-files.json", inventory)
        raw = directory / "fixtures-fingerprints-executables.tar.gz"
        with tarfile.open(raw, "w:gz") as t:
            for path in payload:
                self.check_resource(self.resource())
                t.add(path, arcname=str(path.relative_to(root)), recursive=False)
        retained = {}
        with tarfile.open(raw) as t:
            for member in t.getmembers():
                content = t.extractfile(member).read()
                observed = {"bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()}
                require(observed == {k: inventory[member.name][k] for k in ["bytes", "sha256"]}, "retained archive member changed")
                retained[member.name] = observed
        write_json(directory / "retained-members.json", retained)
        after = processes(scopes); write_json(directory / "process-after.json", after)
        require(not after["visible_cache_owners"] and not after["active_compilers"], "cache owner appeared before retirement")
        verify_snapshot(self.cfg, key.split("-", 1)[1], root)
        accounting_before = self.resource()
        self.check_resource(accounting_before)
        # Verify every file still matches the completed cache inventory before
        # deleting any reviewed scope. Normal compiler races happen during
        # sampling, not after child-group completion and ownership checks.
        current = {str(path.relative_to(root)): path for scope in scopes for path in scope.rglob("*") if path.is_file()}
        require(set(current) == set(inventory), "completed cache path set changed before retirement")
        for name, path in current.items():
            require(sha(path) == inventory[name]["sha256"], "completed cache changed before retirement: " + name)
        for scope in scopes:
            shutil.rmtree(scope)
        accounting_after = self.resource()
        result = {"status": "passed", "retired_scopes": [str(p) for p in scopes], "archive": str(raw), "archive_sha256": sha(raw), "all_target_file_count": len(inventory), "retained_payload_count": len(retained),
                  "fingerprints": "all fingerprint file contents retained; other non-ELF compiler cache files are hash inventory only", "before": accounting_before, "after": accounting_after, "retirement_utc": utc(),
                  "process_scope_limit": before["scope_limit"]}
        write_json(directory / "retirement.json", result)
        return result

    def legacy_inputs(self):
        evidence = Path(self.cfg["legacy_evidence"]["root"])
        manifest = json.loads((evidence / "raw-member-sha256.json").read_text())
        root = self.campaign / "legacy-inputs"
        root.mkdir(exist_ok=False)
        selected = {"work/baseline-protoc-gen-pbrs"}
        for case in self.cfg["default_expected_files"]:
            prefix = "work/source-volume/" + case + "/"
            selected |= {name for name in manifest if name.startswith(prefix) and (name.endswith(("before-request.bin", "shared-final-request.bin", "fixture.fds")) or "/before/" in name or "/shared-final/" in name or "/proto/" in name)}
        with tarfile.open(evidence / "raw-artifacts.tar.gz") as t:
            for name in sorted(selected):
                content = t.extractfile(name).read()
                require(hashlib.sha256(content).hexdigest() == manifest[name]["sha256"] and len(content) == manifest[name]["bytes"], "legacy archive input changed: " + name)
                path = root / safe_name(name); path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(content)
        plugin = root / "work/baseline-protoc-gen-pbrs"; plugin.chmod(0o500)
        require(sha(plugin) == json.loads((evidence / "provenance.json").read_text())["baseline_plugin_sha256"], "archived baseline generator identity mismatch")
        return root, plugin

    def default30(self):
        inputs, historical = self.legacy_inputs()
        result = self.report["default30"] = {"status": "running", "cases": {}, "archived_plugin": {"path": str(historical), "sha256": sha(historical)}, "scope": "30 actual seeded default outputs, not308 archive members; generation observations only"}; self.save()
        expected_volume = json.loads((Path(self.cfg["legacy_evidence"]["root"]) / "volume-before.json").read_text())
        env = os.environ.copy(); env["CARGO_BUILD_JOBS"] = "1"
        for case, count in self.cfg["default_expected_files"].items():
            source = inputs / "work/source-volume" / case
            outputs = {}
            for label, binary, request in [("archived-default", historical, "before-request.bin"), ("baseline-default", self.generators["baseline"], "before-request.bin"), ("candidate-default", self.generators["candidate"], "before-request.bin"), ("baseline-shared", self.generators["baseline"], "shared-final-request.bin"), ("candidate-shared", self.generators["candidate"], "shared-final-request.bin")]:
                if label.startswith(("baseline", "candidate")):
                    variant = label.split("-", 1)[0]
                    require(sha(binary) == self.report["stages"]["stable-" + variant]["generator"]["sha256"], "copied compiled generator changed")
                directory = self.command("source30-" + case + "-" + label, [str(binary)], self.campaign, env, source / request)
                generated = parse_response((directory / "stdout.log").read_bytes())
                outputs[label] = generated
                for name, content in generated.items():
                    path = directory / "generated" / safe_name(name); path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(content)
            archived = {name: (source / "before" / name).read_bytes() for name in expected_volume["cases"][case]["files"]}
            for label in ["archived-default", "baseline-default", "candidate-default"]:
                require(len(outputs[label]) == count and outputs[label] == archived, f"default byte/filename equality failed: {case}/{label}")
                for name, content in outputs[label].items():
                    require(hashlib.sha256(content).hexdigest() == expected_volume["cases"][case]["files"][name]["sha256"], "legacy default hash equality failed")
            baseline, candidate = outputs["baseline-shared"], outputs["candidate-shared"]
            require(set(baseline) == set(candidate) and len(candidate) == count + 1, "shared output filename/count differs")
            for name in baseline:
                if name != HELPER:
                    require(baseline[name] == candidate[name], "shared nonhelper source differs: " + name)
            base_body, base_fds = owner_parts(baseline[HELPER], "array")
            cand_body, cand_fds = owner_parts(candidate[HELPER], "byte-string")
            blobs = [blob for n, wire, blob in fields((source / "fixture.fds").read_bytes()) if n == 1 and wire == 2]
            expected_fds = b"".join(field(1, blob) for blob in sorted(blobs, key=lambda blob: next(v for n, w, v in fields(blob) if n == 1 and w == 2)))
            require(base_body == cand_body and base_fds == cand_fds == expected_fds, "helper pool body or independent canonical descriptor bytes differ")
            owner_counts = {}
            for label, output in outputs.items():
                counts = {"array": sum(v.count(OWNER + b"&[") for v in output.values()), "byte-string": sum(v.count(OWNER + b'b"') for v in output.values())}
                require(counts == {"array": 0 if label == "candidate-shared" else 1 if label == "baseline-shared" else count - 1, "byte-string": int(label == "candidate-shared")}, "raw owner count differs (aliases excluded)")
                owner_counts[label] = counts
            result["cases"][case] = {"status": "passed", "default_files": count, "fds_sha256": hashlib.sha256(expected_fds).hexdigest(), "owner_counts": owner_counts,
                                    "files": {label: {name: {"bytes": len(value), "sha256": hashlib.sha256(value).hexdigest()} for name, value in output.items()} for label, output in outputs.items()}}; self.save()
        result["status"] = "passed"; result["default_generated_file_count"] = sum(self.cfg["default_expected_files"].values()); self.save()


def fields(blob):
    pos = 0
    def integer():
        nonlocal pos
        value, shift = 0, 0
        while True:
            require(pos < len(blob) and shift < 70, "malformed protobuf varint")
            byte = blob[pos]; pos += 1; value |= (byte & 127) << shift
            if byte < 128:
                return value
            shift += 7
    while pos < len(blob):
        key = integer(); n, wire = key >> 3, key & 7
        if wire == 2:
            size = integer(); require(pos + size <= len(blob), "malformed protobuf length")
            value = blob[pos:pos + size]; pos += size
        elif wire == 0:
            value = integer()
        elif wire in [1, 5]:
            size = 8 if wire == 1 else 4; require(pos + size <= len(blob), "malformed fixed protobuf field")
            value = blob[pos:pos + size]; pos += size
        else:
            raise RuntimeError("unsupported protobuf wire kind")
        yield n, wire, value


def varint(value):
    out = bytearray()
    while value >= 128:
        out.append((value & 127) | 128); value >>= 7
    out.append(value)
    return bytes(out)


def field(n, value):
    return varint((n << 3) | 2) + varint(len(value)) + value


def parse_response(blob):
    files = {}
    for n, wire, value in fields(blob):
        if n == 1 and wire == 2:
            require(not value, "plugin response error: " + value.decode(errors="replace"))
        if n == 15 and wire == 2:
            parts = {nn: vv for nn, ww, vv in fields(value) if ww == 2}
            require(1 in parts and 15 in parts, "incomplete plugin output file")
            name = parts[1].decode(); safe_name(name)
            require(name not in files, "duplicate plugin output filename")
            files[name] = parts[15]
    require(files, "empty plugin output")
    return files


def owner_parts(source, kind):
    start = source.index(OWNER); value_start = start + len(OWNER)
    if kind == "array":
        require(source[value_start:].startswith(b"&["), "baseline shared owner is not an array")
        end = source.index(b"];\n\n", value_start)
        literal = source[value_start + 2:end]
        require(re.fullmatch(rb"(?:\s|0x[0-9a-f]{2},)*", literal), "invalid array representation")
        payload = bytes(int(b, 16) for b in re.findall(rb"0x([0-9a-f]{2}),", literal)); end += 4
    else:
        require(source[value_start:].startswith(b'b"'), "candidate shared owner is not a byte string")
        end = source.index(b'";\n\n', value_start + 2)
        literal = source[value_start + 2:end]
        require(re.fullmatch(rb"(?:\\x[0-9a-f]{2})*", literal), "invalid byte-string representation")
        payload = bytes(int(b, 16) for b in re.findall(rb"\\x([0-9a-f]{2})", literal)); end += 4
    return source[:value_start] + b"<SAME_FDS_BYTES>;\n\n" + source[end:], payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--campaign", type=Path)
    parser.add_argument("--lease-file", type=Path)
    parser.add_argument("--prepare-record", type=Path)
    args = parser.parse_args()
    cfg = json.loads(CONFIG.read_text())
    verify_preflight(cfg)
    preflight = {"at_utc": utc(), "status": "prepared_only", "sealed_config_sha256": sha(CONFIG), "runner_sha256": sha(__file__), "source_pins": {v: s["commit"] for v, s in cfg["source"].items()},
                 "source_and_artifact_guards": "passed read-only", "tools_version_launches": "not_run", "processes": processes(), "resource": accounting([], HERE), "all_qualification_gates": "not_run"}
    if args.prepare_record:
        require(not args.prepare_record.exists(), "preparation record already exists")
        write_json(args.prepare_record, preflight)
    if not args.execute:
        print(json.dumps(preflight, indent=2)); return 0
    require(args.campaign and args.lease_file, "execution requires a new campaign and explicit root lease file")
    require(args.campaign.resolve().parent == Path(cfg["campaign_parent"]).resolve(), "campaign must be a direct new child of the reviewed work-only namespace")
    require(re.fullmatch(r"[A-Za-z0-9_.-]+", args.campaign.name), "campaign leaf name refused")
    require(not args.campaign.exists(), "campaign namespace already exists; never overwrite/retry in place")
    lease = json.loads(args.lease_file.read_text())
    require(lease.get("scope") == "GN03 byte-string ordinary qualification" and lease.get("owner") == "root", "ordinary root lease record required")
    require(lease.get("sealed_config_sha256") == sha(CONFIG) and lease.get("runner_sha256") == sha(__file__), "root lease does not match reviewed runner/config")
    require(lease.get("baseline_commit") == cfg["source"]["baseline"]["commit"] and lease.get("candidate_commit") == cfg["source"]["candidate"]["commit"], "root lease source pins differ")
    require(isinstance(lease.get("grant_utc"), str) and dt.datetime.fromisoformat(lease["grant_utc"].replace("Z", "+00:00")).utcoffset() == dt.timedelta(0), "literal UTC lease timestamp required")
    require(not preflight["processes"]["active_compilers"], "other visible compiler active; ordinary lease preflight refused")
    require(preflight["resource"]["global_available_bytes"] >= cfg["guard"]["global_free_reserve_bytes"], "preflight global free reserve absent")
    args.campaign.mkdir(parents=True, exist_ok=False)
    write_json(args.campaign / "preflight.json", preflight)
    shutil.copyfile(args.lease_file, args.campaign / "root-lease.json")
    shutil.copyfile(CONFIG, args.campaign / "sealed-config.json"); shutil.copyfile(__file__, args.campaign / "run-ordinary.py")
    shutil.copyfile(cfg["plan_path"], args.campaign / "qualification-plan.json")
    runner = Run(cfg, args.campaign.resolve(), lease)
    def interrupted(signum, frame):
        raise KeyboardInterrupt("outer runner received " + signal.Signals(signum).name)
    signal.signal(signal.SIGTERM, interrupted)
    try:
        for stage in cfg["stages"]:
            runner.stage(stage)
            if stage == {"tool": "stable", "variant": "candidate"}:
                runner.default30()
        runner.report["status"] = "passed"
    except BaseException as exc:
        runner.report["status"] = "failed"; runner.report["error"] = str(exc)
        (runner.campaign / "failure.traceback.log").write_text(traceback.format_exc())
        if runner.report["default30"]["status"] == "running":
            runner.report["default30"]["status"] = "failed"
    finally:
        for stage in cfg["stages"]:
            key = stage["tool"] + "-" + stage["variant"]
            runner.report["stages"].setdefault(key, {"status": "not_run", "reason": "earlier stage/guard stopped campaign"})
        runner.report["end_utc"] = utc(); runner.report["final_resource"] = runner.resource(); runner.save()
        summaries = [json.loads(path.read_text()).get("sample_summary", {}) for path in (runner.campaign / "phases").glob("*/command.json")]
        minima = [s["minimum_global_available_bytes"] for s in summaries if s.get("minimum_global_available_bytes") is not None]
        runner.report["resource_sample_summary"] = {"samples": sum(s.get("samples", 0) for s in summaries),
            "maximum_owned_allocated_bytes": max([s.get("maximum_owned_allocated_bytes", 0) for s in summaries], default=0),
            "minimum_global_available_bytes": min(minima, default=None), "guard_failure_sample_count": sum(len(s.get("guard_failures", [])) for s in summaries),
            "scope": "raw per-phase sampled ledger only; no continuous cap or between-sample maximum certification"}
        runner.save()
    print(json.dumps({"status": runner.report["status"], "campaign": str(runner.campaign), "end_utc": runner.report["end_utc"]}), flush=True)
    return int(runner.report["status"] != "passed")


if __name__ == "__main__":
    sys.exit(main())
