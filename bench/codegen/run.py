#!/usr/bin/env python3
"""Unqualified CG-19 codegen/consumer cost diagnostic; no network or new dependencies."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORPORA = {"small": (6, 2), "100": (100, 5), "1000": (1000, 20)}
DEFAULT_SEED = 190019
REFERENCE_REVISION = "35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03"
REFERENCE_PROTOC_SHA256 = "e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940"
REFERENCE_RUNTIME_VERSION = "4.35.1-release"
REFERENCE_RUNTIME_CHECKSUM = "a169648cc34d6f327fea8919ca63f38261fb26405fde8879745dc0a483db328e"
REFERENCE_MACROS_CHECKSUM = "fddf7218148e92862511304cfde3ef07ce109e1f03e830601833b9f0cb901a75"
REFERENCE_RUST_OPT = "experimental-codegen=enabled,kernel=upb"
FIELD_TYPES = (
    "string",
    "bytes",
    "uint64",
    "int32",
    "bool",
    "repeated uint32",
    "repeated string",
    "map<string, uint32>",
)


class BenchmarkError(Exception):
    """An incomplete or invalid measurement, never a successful comparison."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def relative(path: Path, run_dir: Path) -> str:
    return path.relative_to(run_dir).as_posix()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def render_proto(seed: int, messages: int, files: int, index: int) -> str:
    if messages % files or not 0 <= index < files:
        raise ValueError("invalid corpus distribution")
    per_file = messages // files
    lines = ['syntax = "proto3";', "package bench.cg19;"]
    if index:
        lines.append(f'import "part_{index - 1:02d}.proto";')
    lines.append("")
    for offset in range(per_file):
        number = index * per_file + offset
        lines.append(f"message Message{number:04d} {{")
        for field in range(1, 5):
            key = f"cg19-v1:{seed}:{messages}:{number}:{field}".encode("ascii")
            kind = FIELD_TYPES[hashlib.sha256(key).digest()[0] % len(FIELD_TYPES)]
            lines.append(f"  {kind} field_{field} = {field};")
        if index and offset == 0:
            lines.append(f"  Message{number - per_file:04d} previous = 5;")
        lines.extend(("}", ""))
    return "\n".join(lines)


def render_consumer(messages: int, marker: int, reference: bool = False) -> str:
    runtime = "protobuf" if reference else "pbrs"
    generated = "generated" if reference else "bench::cg19"
    lines = [
        ('#[path = "../generated/generated.rs"] mod generated;'
         if reference else
         'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));'),
        "",
        f"fn roundtrip<T: {runtime}::Parse + {runtime}::Serialize>(msg: T) -> usize {{",
        '    let wire = msg.serialize().expect("serialize generated message");',
        '    let parsed = T::parse(std::hint::black_box(&wire)).expect("parse generated message");',
        '    std::hint::black_box(parsed.serialize().expect("serialize parsed message")).len()',
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip({generated}::Message{number:04d}::new());"
        for number in range(messages)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def manifest(name: str, reference: bool = False) -> str:
    dependency = (
        f'protobuf = "={REFERENCE_RUNTIME_VERSION}"'
        if reference else f"pbrs = {{ path = {json.dumps(str(ROOT))} }}"
    )
    return (
        f'[package]\nname = "{name}"\nversion = "0.0.0"\nedition = "2024"\n'
        'publish = false\n\n[workspace]\n\n[dependencies]\n'
        f"{dependency}\n\n"
        '[profile.release]\nopt-level = 3\nlto = "thin"\ncodegen-units = 1\n'
    )


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def prepare_corpus(case_dir: Path, case: str, seed: int) -> tuple[list[str], dict]:
    messages, files = CORPORA[case]
    consumer = case_dir / "consumer"
    names = []
    inputs = []
    digest = hashlib.sha256()
    for index in range(files):
        name = f"part_{index:02d}.proto"
        path = consumer / "proto" / name
        write_text(path, render_proto(seed, messages, files, index))
        digest.update(f"{name}\n".encode("ascii"))
        digest.update(path.read_bytes())
        names.append(name)
        inputs.append(
            {"path": relative(path, case_dir), "sha256": sha256(path), "bytes": path.stat().st_size}
        )
    write_text(consumer / "Cargo.toml", manifest(f"cg19-consumer-{case}"))
    write_text(consumer / "src" / "main.rs", render_consumer(messages, 0))
    return names, {
        "messages": messages, "proto_file_count": files, "sha256": digest.hexdigest(), "inputs": inputs
    }


def snapshot_generated(out: Path, entrypoint: str = "mod.rs") -> dict[str, tuple[int, str, int]]:
    if not (out / entrypoint).is_file():
        raise BenchmarkError(f"generation omitted {out / entrypoint}")
    files = sorted(out.rglob("*.rs"))
    if len(files) < 2:
        raise BenchmarkError(f"generation produced fewer than two Rust files in {out}")
    return {
        path.relative_to(out).as_posix(): (path.stat().st_mtime_ns, sha256(path), path.stat().st_size)
        for path in files
    }


def assert_unchanged(before: dict, after: dict) -> None:
    if before.keys() != after.keys():
        raise BenchmarkError(
            f"unchanged generation changed the output file set: "
            f"removed={sorted(before.keys() - after.keys())}, "
            f"added={sorted(after.keys() - before.keys())}"
        )
    for path in sorted(before):
        if before[path] != after[path]:
            raise BenchmarkError(
                f"unchanged generation altered {path}: "
                f"(mtime_ns, sha256, bytes) {before[path]} -> {after[path]}"
            )


def assert_same_bytes(before: dict, after: dict) -> int:
    if before.keys() != after.keys():
        raise BenchmarkError("reference unchanged generation changed the output file set")
    changed = 0
    for path in sorted(before):
        if before[path][1:] != after[path][1:]:
            raise BenchmarkError(f"reference unchanged generation changed bytes in {path}")
        changed += before[path][0] != after[path][0]
    return changed


def tree_digest(snapshot: dict[str, tuple[int, str, int]]) -> str:
    entries = [(path, details[1]) for path, details in sorted(snapshot.items())]
    return hashlib.sha256(json.dumps(entries, separators=(",", ":")).encode("utf-8")).hexdigest()


def parse_time_rss(stderr: str, system: str) -> int:
    if system == "Darwin":
        match = re.search(r"(?m)^\s*(\d+)\s+maximum resident set size\b", stderr)
        scale = 1
    elif system == "Linux":
        match = re.search(r"(?m)^\s*Maximum resident set size \(kbytes\):\s*(\d+)\s*$", stderr)
        scale = 1024
    else:
        raise BenchmarkError(f"RSS measurement unsupported on {system}")
    if match is None:
        raise BenchmarkError(f"missing {system} maximum resident set size in time log")
    return int(match.group(1)) * scale


def tree_rss(ps_output: str, root_pid: int) -> int:
    children: dict[int, list[int]] = {}
    sizes: dict[int, int] = {}
    for row in ps_output.splitlines():
        columns = row.split()
        if len(columns) != 3:
            raise BenchmarkError(f"invalid ps RSS row: {row!r}")
        pid, parent, rss_kib = map(int, columns)
        children.setdefault(parent, []).append(pid)
        sizes[pid] = rss_kib * 1024
    if root_pid not in sizes:
        return 0  # The timed process finished between sampling and ps.
    pending = [root_pid]
    seen = set()
    total = 0
    while pending:
        pid = pending.pop()
        if pid in seen:
            continue
        seen.add(pid)
        total += sizes[pid]
        pending.extend(children.get(pid, ()))
    return total


def _terminate_own_group(proc: subprocess.Popen) -> None:
    # Every measured command starts in a new session; this group belongs only to it.
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()


def log_paths(stem: Path, run_dir: Path) -> tuple[Path, Path, dict]:
    stdout = stem.with_name(stem.name + ".stdout.log")
    stderr = stem.with_name(stem.name + ".stderr.log")
    return stdout, stderr, {
        "stdout_log": relative(stdout, run_dir),
        "stderr_log": relative(stderr, run_dir),
    }


def timed_command(
    command: list[str],
    cwd: Path,
    env: dict[str, str],
    stem: Path,
    run_dir: Path,
    timeout: int,
    sample_ms: int,
) -> dict:
    system = platform.system()
    time_flag = {"Darwin": "-l", "Linux": "-v"}.get(system)
    if time_flag is None or not Path("/usr/bin/time").is_file():
        raise BenchmarkError(f"requires /usr/bin/time on Linux or macOS (found {system})")
    ps = shutil.which("ps")
    if ps is None:
        raise BenchmarkError("requires ps to sample cargo/rustc child RSS")
    stdout, stderr, paths = log_paths(stem, run_dir)
    stem.parent.mkdir(parents=True, exist_ok=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        started = time.perf_counter_ns()
        try:
            proc = subprocess.Popen(
                ["/usr/bin/time", time_flag, *command],
                cwd=cwd,
                env=env,
                stdout=out,
                stderr=err,
                start_new_session=True,
            )
        except OSError as exc:
            raise BenchmarkError(f"cannot start {command[0]}: {exc}; logs: {paths}") from exc
        stop = threading.Event()
        samples: list[int] = []
        sampling_errors: list[str] = []

        def sample() -> None:
            while not stop.is_set():
                try:
                    result = subprocess.run(
                        [ps, "-A", "-o", "pid=,ppid=,rss="],
                        capture_output=True,
                        text=True,
                        check=True,
                        timeout=10,
                        env={**os.environ, "LC_ALL": "C"},
                    )
                    rss = tree_rss(result.stdout, proc.pid)
                    if rss:
                        samples.append(rss)
                except (
                    OSError,
                    ValueError,
                    BenchmarkError,
                    subprocess.CalledProcessError,
                    subprocess.TimeoutExpired,
                ) as exc:
                    sampling_errors.append(f"process-tree RSS sampling failed: {exc}")
                    return
                stop.wait(sample_ms / 1000)

        sampler = threading.Thread(target=sample, daemon=True)
        sampler.start()
        try:
            exit_code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            _terminate_own_group(proc)
            raise BenchmarkError(f"{command[0]} exceeded {timeout}s; logs: {paths}") from exc
        finally:
            stop.set()
            sampler.join()
        elapsed_ns = time.perf_counter_ns() - started
    if sampling_errors:
        raise BenchmarkError(f"{sampling_errors[0]}; logs: {paths}")
    direct_peak = parse_time_rss(stderr.read_text(encoding="utf-8", errors="replace"), system)
    sampled_peak = max(samples) if samples else None
    return {
        "command": command,
        "exit_code": exit_code,
        "elapsed_ns": elapsed_ns,
        "peak_rss_bytes": max(direct_peak, sampled_peak or 0),
        "direct_command_peak_rss_bytes": direct_peak,
        "process_tree_sample_peak_rss_bytes": sampled_peak,
        "process_tree_samples": len(samples),
        "rss_peak_is_lower_bound": True,
        **paths,
    }


def plain_command(
    command: list[str], cwd: Path, env: dict[str, str], stem: Path,
    run_dir: Path, timeout: int,
) -> dict:
    stdout, stderr, paths = log_paths(stem, run_dir)
    stem.parent.mkdir(parents=True, exist_ok=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        try:
            proc = subprocess.Popen(
                command, cwd=cwd, env=env, stdout=out, stderr=err, start_new_session=True,
            )
        except OSError as exc:
            raise BenchmarkError(f"cannot start {command[0]}: {exc}; logs: {paths}") from exc
        try:
            exit_code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            _terminate_own_group(proc)
            raise BenchmarkError(f"{command[0]} exceeded {timeout}s; logs: {paths}") from exc
    return {
        "command": command, "cwd": str(cwd), "exit_code": exit_code,
        "timeout_seconds": timeout, **paths,
    }


def write_report(report: dict, run_dir: Path) -> None:
    path = run_dir / "summary.json"
    temporary = run_dir / ".summary.json.tmp"
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def phase(
    report: dict,
    target: dict,
    name: str,
    command: list[str],
    cwd: Path,
    env: dict[str, str],
    run_dir: Path,
    timeout: int,
    sample_ms: int,
    stem: Path,
    *,
    raw_output: bool = False,
) -> dict:
    _out, _err, paths = log_paths(stem, run_dir)
    target[name] = {"status": "running", "command": command, **paths}
    write_report(report, run_dir)
    try:
        measurement = (
            plain_command(command, cwd, env, stem, run_dir, timeout)
            if raw_output else
            timed_command(command, cwd, env, stem, run_dir, timeout, sample_ms)
        )
    except BenchmarkError as exc:
        target[name].update({"status": "error", "error": str(exc)})
        write_report(report, run_dir)
        raise
    target[name] = {**measurement, "status": "passed" if measurement["exit_code"] == 0 else "failed"}
    write_report(report, run_dir)
    if measurement["exit_code"] != 0:
        raise BenchmarkError(f"{name} exited {measurement['exit_code']}; see {paths['stderr_log']}")
    return measurement


def tool_version(executable: str, flags: list[str], name: str, run_dir: Path) -> dict:
    resolved = shutil.which(executable)
    if resolved is None:
        raise BenchmarkError(f"required tool not found: {executable}")
    path = Path(os.path.abspath(resolved))
    stdout, stderr, logs = log_paths(run_dir / "logs" / "environment" / name, run_dir)
    stdout.parent.mkdir(parents=True, exist_ok=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        try:
            result = subprocess.run([str(path), *flags], stdout=out, stderr=err, timeout=30)
        except subprocess.TimeoutExpired as exc:
            raise BenchmarkError(f"{name} --version timed out; logs: {logs}") from exc
    if result.returncode:
        raise BenchmarkError(f"{name} --version exited {result.returncode}; logs: {logs}")
    return {
        "executable": str(path),
        "resolved_target": str(path.resolve()) if path.is_symlink() else None,
        "executable_sha256": sha256(path),
        "version": stdout.read_text(encoding="utf-8").strip(),
        **logs,
    }


def assert_consumer_built(measurement: dict, package: str, verb: str, run_dir: Path) -> None:
    log = (run_dir / measurement["stderr_log"]).read_text(encoding="utf-8", errors="replace")
    if not re.search(rf"(?m)^\s*{verb} {re.escape(package)} v0\.0\.0\b", log):
        raise BenchmarkError(
            f"{verb.lower()} was a no-op or built the wrong consumer; see "
            f"{measurement['stderr_log']}"
        )


def reference_pin(protoc: dict) -> dict:
    if protoc["version"] != "libprotoc 35.1" or protoc["executable_sha256"] != REFERENCE_PROTOC_SHA256:
        raise BenchmarkError(
            "reference protoc must be the pinned libprotoc 35.1 binary "
            f"({REFERENCE_PROTOC_SHA256}); got {protoc['version']!r}, "
            f"{protoc['executable_sha256']}"
        )
    checked = ROOT / "tonic-bench" / "checked_v4" / "manifest.json"
    pins = json.loads(checked.read_text(encoding="utf-8"))
    expected = {
        "protobuf_source_revision": REFERENCE_REVISION,
        "protoc_version": "libprotoc 35.1",
        "protoc_sha256": REFERENCE_PROTOC_SHA256,
        "runtime_version": REFERENCE_RUNTIME_VERSION,
    }
    if any(pins.get(key) != value for key, value in expected.items()):
        raise BenchmarkError(f"reference pins disagree with {checked}")
    if (ROOT / "vendor" / "google" / "PIN").read_text().strip() != "v35.1":
        raise BenchmarkError("vendor/google/PIN is not v35.1")
    if (ROOT / "vendor" / "google" / "SHA").read_text().strip() != REFERENCE_REVISION:
        raise BenchmarkError("vendor/google/SHA disagrees with pinned reference revision")
    checkout = ROOT / "third_party" / "protobuf"
    head = subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True, timeout=30
    ).strip()
    if head != REFERENCE_REVISION:
        raise BenchmarkError(f"reference source checkout at {head}, expected {REFERENCE_REVISION}")
    dirty = subprocess.check_output(
        ["git", "-C", str(checkout), "status", "--porcelain", "--untracked-files=no"],
        text=True, timeout=30,
    )
    if dirty:
        raise BenchmarkError(f"reference source checkout has tracked changes: {checkout}")
    return {
        "source_revision": head,
        "source_checkout": str(checkout),
        "checked_pin_manifest_sha256": sha256(checked),
        "generator_options": REFERENCE_RUST_OPT,
    }


def reference_lock(path: Path) -> dict:
    try:
        import tomllib
    except ModuleNotFoundError as exc:
        raise BenchmarkError("reference lock verification requires Python 3.11+") from exc
    packages = tomllib.loads(path.read_text(encoding="utf-8")).get("package")
    if not isinstance(packages, list):
        raise BenchmarkError(f"reference lockfile lacks packages: {path}")
    if any(package.get("name") == "pbrs" for package in packages):
        raise BenchmarkError(f"reference consumer unexpectedly depends on pbrs: {path}")
    result = {"lock_sha256": sha256(path), "packages": {}}
    for name, checksum in (
        ("protobuf", REFERENCE_RUNTIME_CHECKSUM),
        ("protobuf-macros", REFERENCE_MACROS_CHECKSUM),
    ):
        matches = [package for package in packages if package.get("name") == name]
        if len(matches) != 1 or any(
            matches[0].get(key) != value for key, value in (
                ("version", REFERENCE_RUNTIME_VERSION),
                ("source", "registry+https://github.com/rust-lang/crates.io-index"),
                ("checksum", checksum),
            )
        ):
            raise BenchmarkError(f"reference {name} version/source/checksum is not pinned in {path}")
        result["packages"][name] = {
            "version": REFERENCE_RUNTIME_VERSION, "source": matches[0]["source"], "checksum": checksum,
        }
    return result


def source_hashes(reference: bool = False) -> dict[str, str]:
    paths = [
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "build.rs",
        ROOT / "proto" / "person.proto",
        ROOT / "vendor" / "google" / "conformance_fds.bin",
        ROOT / "scripts" / "codegen-bench.sh",
        Path(__file__),
        Path(__file__).with_name("generator.rs"),
        *(ROOT / "src").rglob("*.rs"),
    ]
    if reference:
        paths.extend([
            ROOT / "tonic-bench" / "Cargo.lock",
            ROOT / "tonic-bench" / "checked_v4" / "manifest.json",
            ROOT / "vendor" / "google" / "PIN",
            ROOT / "vendor" / "google" / "SHA",
        ])
    return {
        path.relative_to(ROOT).as_posix(): sha256(path)
        for path in sorted(paths)
    }


def provenance(run_dir: Path, jobs: int, sample_ms: int, reference_protoc: Path | None = None) -> dict:
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, text=True
    ).splitlines()
    cargo = tool_version(os.environ.get("CARGO", "cargo"), ["--version", "--verbose"], "cargo", run_dir)
    rustc = tool_version(os.environ.get("RUSTC", "rustc"), ["--version", "--verbose"], "rustc", run_dir)
    protoc = tool_version(
        str(reference_protoc) if reference_protoc else os.environ.get("PROTOC", "protoc"),
        ["--version"], "protoc", run_dir,
    )
    reference = None
    tools = {"cargo": cargo, "rustc": rustc, "protoc": protoc}
    if reference_protoc is not None:
        overrides = sorted(
            key for key in os.environ if key.startswith("CC_") or key in ("TARGET_CC", "HOST_CC")
        )
        if overrides:
            raise BenchmarkError(f"reference C compiler overrides are unsupported: {overrides}")
        reference = reference_pin(protoc)
        tools["cc"] = tool_version(os.environ.get("CC", "cc"), ["--version"], "cc", run_dir)
    return {
        "repository": {
            "head": head,
            "tracked_changes": dirty,
            "source_sha256": source_hashes(reference_protoc is not None),
            "source_scope": (
                "src/**/*.rs, build.rs, proto/person.proto, vendored conformance FDS, "
                "Cargo.toml/Cargo.lock, harness, generator and wrapper"
                + (", vendored Google pin, checked generator pin and tonic-bench/Cargo.lock"
                   if reference_protoc is not None else "")
            ),
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "cpu_count": os.cpu_count(),
            "python": sys.version.split()[0],
        },
        "tools": tools,
        **({"reference_source": reference} if reference is not None else {}),
        "cache": {
            "cargo_home": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
            "inherited_target_dir": os.environ.get("CARGO_TARGET_DIR"),
            "inherited_cargo_incremental": os.environ.get("CARGO_INCREMENTAL"),
            "targets": "shared integration-consumers bootstrap; fresh isolated per-corpus targets",
            "registry": "existing local Cargo registry; warmth not controlled",
            "cargo_offline": True,
            "cargo_locked_after_offline_lockfile": True,
            "build_jobs": jobs,
            "check_incremental": True,
            "release_incremental": False,
            "rustflags": os.environ.get("RUSTFLAGS"),
            "rustc_wrapper": os.environ.get("RUSTC_WRAPPER"),
            "rustc_workspace_wrapper": os.environ.get("RUSTC_WORKSPACE_WRAPPER"),
            "sccache_dir": os.environ.get("SCCACHE_DIR"),
            "rustup_toolchain": os.environ.get("RUSTUP_TOOLCHAIN"),
            **({"c_compiler_env": {
                key: os.environ.get(key) for key in (
                    "CC", "CFLAGS", "CPPFLAGS", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET",
                    "CRATE_CC_NO_DEFAULTS", "CARGO_ENCODED_RUSTFLAGS",
                )
            }, "cargo_profile_overrides": {
                key: value for key, value in os.environ.items() if key.startswith("CARGO_PROFILE_")
            }} if reference_protoc is not None else {}),
        },
        "measurement": {
            "rss": "max of OS time direct-command peak and sampled process-tree RSS sum",
            "rss_sample_ms": sample_ms,
            "rss_peaks_are_lower_bounds": True,
            "generation_includes": "protoc descriptor compilation + Config::compile_protos Rust emission",
            "check_clean": "fresh per-corpus target; local registry/compiler wrapper may be warm",
            "check_incremental": "same target after source marker changes; consumer rechecked",
            "release": "same target, distinct release profile: opt-level=3, thin LTO, codegen-units=1",
        },
    }


def copy_generator(shared: Path, run_dir: Path) -> tuple[Path, str]:
    if not shared.is_file():
        raise BenchmarkError(f"bootstrap did not produce {shared}")
    before = sha256(shared)
    generator = run_dir / "bin" / "cg19-generator"
    generator.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(shared, generator)
    copied = sha256(generator)
    if copied != before or sha256(shared) != before:
        raise BenchmarkError(f"shared generator changed during copy to {generator}")
    if not os.access(generator, os.X_OK):
        raise BenchmarkError(f"copied generator is not executable: {generator}")
    return generator, copied


def release_smoke(
    report: dict, phases: dict, binary: Path, consumer: Path, env: dict[str, str],
    run_dir: Path, timeout: int, sample_ms: int, logs: Path,
) -> None:
    phase(
        report, phases, "release_smoke", [str(binary)], consumer, env, run_dir,
        min(timeout, 15), sample_ms, logs / "release-smoke", raw_output=True,
    )
    smoke = phases["release_smoke"]
    stdout_path = run_dir / smoke["stdout_log"]
    stderr_path = run_dir / smoke["stderr_log"]
    stdout = stdout_path.read_bytes()
    stderr = stderr_path.read_bytes()
    smoke.update({
        "expected_stdout": "1\n",
        "expected_stderr": "",
        "stdout_sha256": sha256(stdout_path),
        "stderr_sha256": sha256(stderr_path),
        "output_verified": stdout == b"1\n" and stderr == b"",
    })
    if not smoke["output_verified"]:
        smoke["status"] = "failed"
        smoke["error"] = (
            f"release smoke expected stdout b'1\\n' and empty stderr, "
            f"got {len(stdout)} stdout bytes and {len(stderr)} stderr bytes; "
            f"see {smoke['stdout_log']} and {smoke['stderr_log']}"
        )
        write_report(report, run_dir)
        raise BenchmarkError(smoke["error"])
    write_report(report, run_dir)


def measure_consumer_build(
    report: dict, cell: dict, case: str, consumer: Path, target_dir: Path, package: str,
    reference: bool, cargo: str, check_env: dict[str, str], run_dir: Path,
    timeout: int, sample_ms: int, logs: Path,
) -> None:
    if target_dir.exists():
        raise BenchmarkError(f"clean check target already exists: {target_dir}")
    cargo_args = [
        "--offline", "--locked", "--manifest-path", str(consumer / "Cargo.toml"),
        "--target-dir", str(target_dir), "--bin", package,
    ]
    checked = phase(
        report, cell["phases"], "check_clean", [cargo, "check", *cargo_args],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "check-clean",
    )
    assert_consumer_built(checked, package, "Checking", run_dir)
    main_rs = consumer / "src" / "main.rs"
    old_mtime = main_rs.stat().st_mtime_ns
    time.sleep(max(0, (old_mtime + 1_100_000_000 - time.time_ns()) / 1_000_000_000))
    write_text(main_rs, render_consumer(CORPORA[case][0], 1, reference))
    if main_rs.stat().st_mtime_ns <= old_mtime:
        raise BenchmarkError(f"incremental source mtime did not advance: {main_rs}")
    incremental = phase(
        report, cell["phases"], "check_incremental", [cargo, "check", *cargo_args],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "check-incremental",
    )
    assert_consumer_built(incremental, package, "Checking", run_dir)
    release = phase(
        report, cell["phases"], "build_release",
        [cargo, "build", "--release", *cargo_args],
        ROOT, {**check_env, "CARGO_INCREMENTAL": "0"},
        run_dir, timeout, sample_ms, logs / "build-release",
    )
    assert_consumer_built(release, package, "Compiling", run_dir)
    binary = target_dir / "release" / package
    if not binary.is_file() or not binary.stat().st_size:
        raise BenchmarkError(f"release build omitted a nonempty binary at {binary}")
    cell["release_binary"] = {
        "path": relative(binary, run_dir),
        "size_bytes": binary.stat().st_size,
        "sha256": sha256(binary),
    }
    write_report(report, run_dir)
    release_smoke(
        report, cell["phases"], binary, consumer, check_env, run_dir, timeout, sample_ms, logs,
    )


def measure_reference(
    report: dict, cell: dict, case: str, names: list[str], case_dir: Path, cargo: str,
    protoc: str, base_env: dict[str, str], run_dir: Path, timeout: int, sample_ms: int,
) -> dict:
    consumer = case_dir / "reference" / "consumer"
    generated = consumer / "generated"
    generated.mkdir(parents=True)
    package = f"cg19-reference-{case}"
    write_text(consumer / "Cargo.toml", manifest(package, reference=True))
    write_text(consumer / "src" / "main.rs", render_consumer(CORPORA[case][0], 0, reference=True))
    target_dir = case_dir / "reference" / "target"
    reference = {
        "corpus_sha256": cell["corpus"]["sha256"],
        "proto_inputs": [item["path"] for item in cell["corpus"]["inputs"]],
        "target_dir": relative(target_dir, run_dir),
        "phases": {},
    }
    cell["reference"] = reference
    write_report(report, run_dir)
    check_env = {**base_env, "CARGO_TARGET_DIR": str(target_dir), "CARGO_INCREMENTAL": "1"}
    logs = run_dir / "logs" / case / "reference"
    phase(
        report, reference["phases"], "consumer_lock",
        [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "consumer-lock",
    )
    report["reference"]["runtime"] = reference_lock(consumer / "Cargo.lock")
    reference["consumer_lock_sha256"] = report["reference"]["runtime"]["lock_sha256"]
    write_report(report, run_dir)
    generation = [
        protoc, f"--proto_path={case_dir / 'consumer' / 'proto'}",
        f"--rust_out={generated}", f"--rust_opt={REFERENCE_RUST_OPT}", *names,
    ]
    phase(
        report, reference["phases"], "generation", generation, ROOT, base_env,
        run_dir, timeout, sample_ms, logs / "generation",
    )
    before = snapshot_generated(generated, "generated.rs")
    expected = {"generated.rs", *(name.removesuffix(".proto") + ".u.pb.rs" for name in names)}
    if before.keys() != expected:
        raise BenchmarkError(
            f"{case}: unexpected reference Rust outputs: "
            f"missing={sorted(expected - before.keys())}, extra={sorted(before.keys() - expected)}"
        )
    phase(
        report, reference["phases"], "generation_unchanged", generation, ROOT, base_env,
        run_dir, timeout, sample_ms, logs / "generation-unchanged",
    )
    rewritten = assert_same_bytes(before, snapshot_generated(generated, "generated.rs"))
    reference["output"] = {
        "rust_file_count": len(before),
        "rust_bytes": sum(item[2] for item in before.values()),
        "rust_tree_sha256": tree_digest(before),
        "unchanged_generation_bytes_verified": True,
        "unchanged_generation_mtimes_preserved": rewritten == 0,
        "unchanged_generation_rewritten_files": rewritten,
    }
    write_report(report, run_dir)
    measure_consumer_build(
        report, reference, case, consumer, target_dir, package, True, cargo, check_env,
        run_dir, timeout, sample_ms, logs,
    )
    return reference


def compare_cell(report: dict, cell: dict, run_dir: Path) -> None:
    reference = cell["reference"]
    if cell["corpus"]["sha256"] != reference["corpus_sha256"]:
        raise BenchmarkError("paired consumers did not use the same corpus")
    pairs = {
        "output.rust_bytes": (cell["output"]["rust_bytes"], reference["output"]["rust_bytes"]),
        "release_binary.size_bytes": (
            cell["release_binary"]["size_bytes"], reference["release_binary"]["size_bytes"],
        ),
    }
    for phase_name in ("generation", "generation_unchanged", "check_clean",
                       "check_incremental", "build_release"):
        for metric in ("elapsed_ns", "peak_rss_bytes"):
            pairs[f"{phase_name}.{metric}"] = (
                cell["phases"][phase_name][metric],
                reference["phases"][phase_name][metric],
            )
    rows = []
    for metric, (pbrs, pinned) in pairs.items():
        if (not isinstance(pbrs, int) or isinstance(pbrs, bool) or pbrs < 0
                or not isinstance(pinned, int) or isinstance(pinned, bool) or pinned < 0):
            raise BenchmarkError(f"invalid paired measurement for {cell['case']}: {metric}")
        rows.append({
            "case": cell["case"], "metric": metric, "pbrs": pbrs, "reference": pinned,
            "pbrs_loses": pbrs > pinned,
            "rss_peak_is_lower_bound": metric.endswith("peak_rss_bytes"),
        })
    if report["comparison"]["losing_cells"] is None:
        report["comparison"]["losing_cells"] = []
    report["comparison"]["metrics"].extend(rows)
    report["comparison"]["losing_cells"].extend(row for row in rows if row["pbrs_loses"])
    report["comparison"]["status"] = "partial"
    write_report(report, run_dir)


def run_cases(
    report: dict, run_dir: Path, cases: list[str], seed: int, jobs: int, timeout: int,
    sample_ms: int, reference_protoc: Path | None = None,
) -> None:
    details = provenance(run_dir, jobs, sample_ms, reference_protoc)
    report["environment"] = details
    if reference_protoc is not None:
        report["reference"].update({
            "status": "ready",
            "generator": "protobuf v35.1 built-in --rust_out (kernel=upb)",
            "revision": REFERENCE_REVISION,
            "protoc": details["tools"]["protoc"],
            "c_compiler": details["tools"]["cc"],
            "source": details["reference_source"],
            "runtime": None,
            "reason": "independent pinned-host and repeated paired qualification not performed",
        })
    write_report(report, run_dir)
    cargo = details["tools"]["cargo"]["executable"]
    protoc = details["tools"]["protoc"]["executable"]
    base_env = os.environ.copy()
    cleared = sorted(key for key in base_env if key.startswith("PURE_PROTOBUF_"))
    for key in cleared:
        del base_env[key]
    details["cache"]["cleared_codegen_env_keys"] = cleared
    protoc_link = run_dir / "bin" / "protoc"
    protoc_link.parent.mkdir()
    protoc_link.symlink_to(protoc)
    details["tools"]["core_build_script_protoc"] = relative(protoc_link, run_dir)
    base_env.update(
        {
            "CARGO_NET_OFFLINE": "true",
            "CARGO_TERM_COLOR": "never",
            "CARGO_BUILD_JOBS": str(jobs),
            "RUSTC": details["tools"]["rustc"]["executable"],
            "PROTOC": protoc,
            "PATH": str(protoc_link.parent) + os.pathsep + base_env.get("PATH", ""),
            "LC_ALL": "C",
        }
    )
    if reference_protoc is not None:
        base_env["CC"] = details["tools"]["cc"]["executable"]
        details["cache"]["paired_cold_targets"] = (
            "separate initially nonexistent cases/<case>/target and cases/<case>/reference/target; "
            "serial Cargo jobs=2; shared registry and compiler-wrapper caches are not cleared"
        )
        details["measurement"]["reference_generation_includes"] = (
            "same proto inputs parsed by pinned protoc and emitted by built-in Rust --rust_out"
        )
        details["measurement"]["reference_check_clean"] = (
            "fresh target including protobuf 4.35.1-release and its C/upb build; "
            "no pbrs or ABI shim; local registry/compiler-wrapper caches may be warm"
        )
    write_report(report, run_dir)

    driver = run_dir / "driver"
    (driver / "src").mkdir(parents=True)
    shutil.copyfile(Path(__file__).with_name("generator.rs"), driver / "src" / "main.rs")
    write_text(driver / "Cargo.toml", manifest("cg19-generator"))
    boot_target = ROOT / "target" / "integration-consumers"
    boot_jobs = min(jobs, 2)
    details["cache"].update({
        "bootstrap_target_dir": str(boot_target),
        "bootstrap_build_jobs": boot_jobs,
        "bootstrap_cargo_incremental": base_env.get("CARGO_INCREMENTAL"),
        "bootstrap_shared": True,
        "bootstrap_included_in_measurements": False,
        "corpus_targets": "fresh cases/<case>/target; never use bootstrap target",
    })
    write_report(report, run_dir)
    boot_env = {**base_env, "CARGO_TARGET_DIR": str(boot_target), "CARGO_BUILD_JOBS": str(boot_jobs)}
    boot_log = run_dir / "logs" / "setup"
    phase(
        report, report["setup"], "driver_lock",
        [cargo, "generate-lockfile", "--offline", "--manifest-path", str(driver / "Cargo.toml")],
        ROOT, boot_env, run_dir, timeout, sample_ms, boot_log / "driver-lock",
    )
    report["setup"]["driver_lock_sha256"] = sha256(driver / "Cargo.lock")
    write_report(report, run_dir)
    phase(
        report, report["setup"], "driver_build",
        [
            cargo, "build", "--offline", "--locked", "--manifest-path", str(driver / "Cargo.toml"),
            "--target-dir", str(boot_target), "--bin", "cg19-generator",
        ],
        ROOT, boot_env, run_dir, timeout, sample_ms, boot_log / "driver-build",
    )
    shared_generator = boot_target / "debug" / "cg19-generator"
    generator, digest = copy_generator(shared_generator, run_dir)
    report["setup"]["shared_generator_binary_path"] = str(shared_generator)
    report["setup"]["generator_binary_path"] = relative(generator, run_dir)
    report["setup"]["generator_binary_sha256"] = digest
    write_report(report, run_dir)

    for case in cases:
        case_dir = run_dir / "cases" / case
        names, corpus = prepare_corpus(case_dir, case, seed)
        consumer = case_dir / "consumer"
        generated = consumer / "generated"
        target_dir = case_dir / "target"
        package = f"cg19-consumer-{case}"
        cell = {
            "case": case,
            "corpus": corpus,
            "target_dir": relative(target_dir, run_dir),
            "phases": {},
        }
        report["cells"].append(cell)
        write_report(report, run_dir)
        check_env = {**base_env, "CARGO_TARGET_DIR": str(target_dir), "CARGO_INCREMENTAL": "1"}
        logs = run_dir / "logs" / case
        phase(
            report, cell["phases"], "consumer_lock",
            [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
            ROOT, check_env, run_dir, timeout, sample_ms, logs / "consumer-lock",
        )
        cell["consumer_lock_sha256"] = sha256(consumer / "Cargo.lock")
        generation = [
            str(generator), str(consumer / "proto"), str(generated), protoc, *names
        ]
        phase(
            report, cell["phases"], "generation", generation, ROOT, base_env,
            run_dir, timeout, sample_ms, logs / "generation",
        )
        before = snapshot_generated(generated)
        expected_files = {"mod.rs", *(name.removesuffix(".proto") + ".rs" for name in names)}
        missing = expected_files - before.keys()
        if missing:
            raise BenchmarkError(f"{case}: missing generated Rust outputs: {sorted(missing)}")
        phase(
            report, cell["phases"], "generation_unchanged", generation, ROOT, base_env,
            run_dir, timeout, sample_ms, logs / "generation-unchanged",
        )
        assert_unchanged(before, snapshot_generated(generated))
        cell["output"] = {
            "rust_file_count": len(before),
            "rust_bytes": sum(item[2] for item in before.values()),
            "rust_tree_sha256": tree_digest(before),
            "unchanged_generation_mtimes_preserved": True,
            "unchanged_generation_verified_files": len(before),
        }
        write_report(report, run_dir)
        measure_consumer_build(
            report, cell, case, consumer, target_dir, package, False, cargo, check_env,
            run_dir, timeout, sample_ms, logs,
        )
        if reference_protoc is not None:
            measure_reference(
                report, cell, case, names, case_dir, cargo, protoc, base_env,
                run_dir, timeout, sample_ms,
            )
            compare_cell(report, cell, run_dir)

    before = details["repository"]["source_sha256"]
    after = source_hashes(reference_protoc is not None)
    if before != after:
        changed = sorted(path for path in before.keys() | after.keys() if before.get(path) != after.get(path))
        raise BenchmarkError(f"source changed during measurement: {changed}")
    if reference_protoc is not None:
        if sha256(Path(protoc)) != REFERENCE_PROTOC_SHA256:
            raise BenchmarkError("pinned reference protoc changed during measurement")
        report["reference"]["status"] = "measured"
        report["comparison"]["status"] = "diagnostic"
        write_report(report, run_dir)


def positive_int(value: str) -> int:
    number = int(value)
    if not 1 <= number <= 3600:
        raise argparse.ArgumentTypeError("must be between 1 and 3600")
    return number


def build_jobs(value: str) -> int:
    try:
        number = int(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("must be an integer between 1 and 8") from exc
    if not 1 <= number <= 8:
        raise argparse.ArgumentTypeError("must be between 1 and 8")
    return number


def seed_arg(value: str) -> int:
    number = int(value)
    if not 0 <= number <= 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("seed must fit in 32 bits")
    return number


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", choices=["all", *CORPORA], default="all")
    parser.add_argument("--seed", type=seed_arg, default=DEFAULT_SEED)
    parser.add_argument("--out", type=Path, help="new directory under target/codegen-bench")
    inherited_jobs = os.environ.get("CARGO_BUILD_JOBS")
    try:
        default_jobs = build_jobs(inherited_jobs) if inherited_jobs is not None else 2
    except argparse.ArgumentTypeError as exc:
        parser.error(f"invalid CARGO_BUILD_JOBS: {exc}")
    parser.add_argument("--jobs", type=build_jobs, default=default_jobs)
    parser.add_argument("--timeout-seconds", type=positive_int, default=900)
    parser.add_argument("--rss-sample-ms", type=positive_int, default=100)
    parser.add_argument(
        "--reference-protoc", type=Path,
        help="opt into the SHA-pinned v35.1 upb peer for one explicit "
             "--case small|100|1000 --seed 190019 --jobs 2",
    )
    parser.add_argument(
        "--require-qualified", action="store_true",
        help="fail fast: even a local pinned-reference diagnostic lacks independent qualification",
    )
    args = parser.parse_args(argv)
    if args.reference_protoc is not None:
        if args.case == "all" or args.seed != DEFAULT_SEED or args.jobs != 2:
            parser.error(
                "--reference-protoc requires one explicit --case small|100|1000 "
                "--seed 190019 --jobs 2"
            )
        args.reference_protoc = Path(os.path.abspath(args.reference_protoc))
    allowed = (ROOT / "target" / "codegen-bench").resolve()
    run_dir = (
        args.out if args.out is not None
        else allowed / f"{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{os.getpid()}"
    ).resolve()
    if not run_dir.is_relative_to(allowed) or run_dir == allowed:
        parser.error("--out must be a new directory under target/codegen-bench")
    if run_dir.exists():
        parser.error(f"output already exists: {run_dir} (refusing to overwrite evidence)")
    run_dir.mkdir(parents=True)
    cases = list(CORPORA) if args.case == "all" else [args.case]
    report = {
        "schema_version": "cg19/1",
        "status": "pending",
        "started_at_utc": utc_now(),
        "seed": args.seed,
        "cases_requested": cases,
        "environment": None,
        "setup": {},
        "cells": [],
        "reference": {
            "status": "missing",
            "generator": None,
            "revision": None,
            "reason": "No equivalent pinned generator and consumer with matched reflection/API work is wired.",
        } if args.reference_protoc is None else {
            "status": "requested",
            "generator": "protobuf v35.1 built-in --rust_out (kernel=upb)",
            "revision": REFERENCE_REVISION,
            "requested_protoc": str(args.reference_protoc),
            "reason": "Pinned binary, source checkout and runtime lock have not been verified yet.",
        },
        "comparison": {"status": "not_run", "losing_cells": None}
        if args.reference_protoc is None else
        {"status": "not_run", "metrics": [], "losing_cells": None},
        "qualification": {
            "qualified": False,
            "reasons": (
                ["no_equivalent_reference_peer", "single_run_diagnostic"]
                if args.reference_protoc is None else
                ["single_run_diagnostic", "no_independent_pinned_host_qualification",
                 "no_paired_replicates_or_uncertainty"]
            ),
        },
        "errors": [],
    }
    if len(cases) != len(CORPORA):
        report["qualification"]["reasons"].append("partial_corpus_matrix")
    write_report(report, run_dir)
    if args.require_qualified:
        report["status"] = "unqualified"
        report["finished_at_utc"] = utc_now()
        write_report(report, run_dir)
        print(f"UNQUALIFIED: independent paired-host evidence is missing; {run_dir / 'summary.json'}",
              file=sys.stderr)
        return 2
    try:
        run_cases(
            report, run_dir, cases, args.seed, args.jobs, args.timeout_seconds, args.rss_sample_ms,
            args.reference_protoc,
        )
    except (BenchmarkError, OSError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired, ValueError) as exc:
        report["status"] = "error"
        report["errors"].append(str(exc))
        if args.reference_protoc is not None and report["reference"]["status"] != "measured":
            report["reference"]["status"] = "incomplete"
            report["reference"]["reason"] = str(exc)
        report["qualification"]["reasons"].append("incomplete_measurement")
        report["finished_at_utc"] = utc_now()
        write_report(report, run_dir)
        print(f"CG-19 ERROR: {exc}; {run_dir / 'summary.json'}", file=sys.stderr)
        return 1
    report["status"] = "unqualified"
    report["finished_at_utc"] = utc_now()
    write_report(report, run_dir)
    if args.reference_protoc is None:
        print(f"UNQUALIFIED (no equivalent reference): {run_dir / 'summary.json'}")
    else:
        print(f"UNQUALIFIED (local pinned-reference diagnostic only): {run_dir / 'summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
