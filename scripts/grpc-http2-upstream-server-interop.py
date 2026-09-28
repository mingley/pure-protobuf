#!/usr/bin/env python3
"""Run the pinned upstream Go HTTP/2 probes against the native server."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import time

from upstream_http2_probe_report import GRPC_SOURCE_SHA, summarize


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "third_party/grpc"
GO_BINARY = ROOT / "target/interop-go/http2interop.test"
NATIVE_BINARY = ROOT / "target/debug/pbrs-grpc-interop-server"
CREDENTIALS = SOURCE / "src/core/tsi/test_creds"
TLS_NAME = "foo.test.google.fr"


def command(args: list[str], cwd: Path = ROOT, env: dict | None = None, timeout: int = 120):
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


def require_clean_source() -> str:
    got = command(["git", "-C", str(SOURCE), "rev-parse", "HEAD"])
    if got.returncode != 0 or got.stdout.strip() != GRPC_SOURCE_SHA:
        raise RuntimeError("pinned grpc/grpc source is absent or at the wrong revision")
    status = command([
        "git", "-C", str(SOURCE), "status", "--porcelain=v1",
        "--untracked-files=all", "--ignored=matching", "--",
        "tools/http2_interop", "src/core/tsi/test_creds",
    ])
    if status.returncode != 0:
        raise RuntimeError("could not inspect pinned grpc/grpc Go probes or test credentials")
    if status.stdout:
        raise RuntimeError(
            "pinned grpc/grpc Go probes or test credentials are modified, untracked, or ignored"
        )
    return got.stdout.strip()


def go_toolchain() -> tuple[str, dict]:
    line = next(
        (line for line in (ROOT / "tests/interop/go/go.mod").read_text().splitlines()
         if line.startswith("go ")),
        None,
    )
    if line is None:
        raise RuntimeError("pinned interop Go version is missing")
    parts = line.split()
    if len(parts) != 2:
        raise RuntimeError("malformed pinned interop Go version")
    version = command(["go", "version"])
    if version.returncode != 0 or f"go{parts[1]}" not in version.stdout.split():
        raise RuntimeError(f"expected Go {parts[1]}, got {version.stdout.strip()}")
    env = os.environ.copy()
    env.update({"GO111MODULE": "off", "GOMAXPROCS": "2"})
    return version.stdout.strip(), env


def prepare_binaries(skip_rust: bool, skip_go: bool, go_version: str, go_env: dict):
    if not skip_rust:
        env = os.environ.copy()
        env["CARGO_BUILD_JOBS"] = "2"
        env["CARGO_TARGET_DIR"] = str(ROOT / "target")
        built = command([
            "cargo", "build", "--offline", "--locked", "-p", "pbrs-grpc",
            "--bin", "pbrs-grpc-interop-server",
        ], env=env, timeout=300)
        if built.returncode != 0:
            raise RuntimeError(f"native server build failed:\n{built.stderr[-3000:]}")
    if not NATIVE_BINARY.is_file():
        raise RuntimeError(
            "native interop server is missing; build it before using --skip-rust-build"
        )

    stamp = GO_BINARY.with_suffix(".pin")
    cached = stamp.read_text(encoding="utf-8").splitlines() if stamp.is_file() else []
    reusable = (
        GO_BINARY.is_file()
        and len(cached) == 3
        and cached[:2] == [GRPC_SOURCE_SHA, go_version]
        and cached[2] == digest(GO_BINARY)
    )
    if not reusable:
        if skip_go:
            raise RuntimeError("pinned upstream Go probe binary is missing or unverified")
        print("Building the pinned upstream Go probe binary", file=sys.stderr)
        GO_BINARY.parent.mkdir(parents=True, exist_ok=True)
        built = command([
            "go", "test", "-c", "-o", str(GO_BINARY), "./tools/http2_interop",
        ], cwd=SOURCE, env=go_env, timeout=180)
        if built.returncode != 0:
            raise RuntimeError(f"upstream Go probe build failed:\n{built.stderr[-3000:]}")
        if not os.access(GO_BINARY, os.X_OK):
            raise RuntimeError("upstream Go build did not produce an executable probe")
        stamp.write_text(
            f"{GRPC_SOURCE_SHA}\n{go_version}\n{digest(GO_BINARY)}\n",
            encoding="utf-8",
        )


def digest(path: Path) -> str:
    hashed = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hashed.update(chunk)
    return hashed.hexdigest()


def artifact_ref(path: Path) -> str:
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


def free_port() -> int:
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        return reservation.getsockname()[1]


def wait_for_server(server: subprocess.Popen, port: int):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if server.poll() is not None:
            raise RuntimeError(f"native server exited during startup: {server.returncode}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError(f"native server did not listen on {port} within five seconds")


def verify_tls(port: int):
    context = ssl.create_default_context(cafile=str(CREDENTIALS / "ca.pem"))
    # This pinned 2020 test certificate lacks AKI; Go verifies its chain and
    # name, but Python 3.14's extra strict-extension check rejects it.
    context.verify_flags &= ~ssl.VERIFY_X509_STRICT
    if context.verify_mode != ssl.CERT_REQUIRED or not context.check_hostname:
        raise RuntimeError("upstream TLS test identity verification must remain enabled")
    context.set_alpn_protocols(["h2"])
    with socket.create_connection(("127.0.0.1", port), timeout=2) as raw:
        with context.wrap_socket(raw, server_hostname=TLS_NAME) as tls:
            if tls.selected_alpn_protocol() != "h2":
                raise RuntimeError("verified test identity did not negotiate h2")


def run_mode(mode: str, log_dir: Path, go_version: str, go_env: dict, dirty: bool,
             skip_rust: bool, native_revision: str) -> dict:
    port = free_port()
    server_cmd = [str(NATIVE_BINARY), "--port", str(port)]
    if mode == "tls":
        server_cmd.extend([
            "--use_tls=true",
            "--tls_cert_file", str(CREDENTIALS / "server1.pem"),
            "--tls_key_file", str(CREDENTIALS / "server1.key"),
        ])
    server_log = log_dir / f"{mode}-server.log"
    raw_log = log_dir / f"{mode}-go.log"
    with server_log.open("w", encoding="utf-8") as server_output:
        server = subprocess.Popen(
            server_cmd, cwd=ROOT, stdout=server_output, stderr=subprocess.STDOUT,
        )
        try:
            wait_for_server(server, port)
            if mode == "tls":
                verify_tls(port)
            flags = [
                "-test.v", "-test.timeout=90s", "-server_host=127.0.0.1",
                f"-server_port={port}", f"-test_case={mode}",
                f"-use_tls={'true' if mode == 'tls' else 'false'}",
            ]
            if mode == "tls":
                flags.extend(["-use_test_ca=true", f"-server_host_override={TLS_NAME}"])
            go = command([str(GO_BINARY), *flags], cwd=SOURCE, env=go_env, timeout=100)
            raw_log.write_text(
                go.stdout + "\n--- STDERR ---\n" + go.stderr, encoding="utf-8",
            )
            report = summarize(raw_log.read_text(encoding="utf-8"),
                               mode, go.returncode, GRPC_SOURCE_SHA)
            report.update({
                "go_version": go_version,
                "go_binary_sha256": digest(GO_BINARY),
                "native_binary_sha256": digest(NATIVE_BINARY),
                "native_git_sha": native_revision,
                "native_dirty": dirty,
                "raw_log": artifact_ref(raw_log),
                "server_log": artifact_ref(server_log),
            })
            if dirty or skip_rust:
                report["qualified"] = False
                report["failures"].append(
                    "native source dirty or cached binary provenance unverified"
                )
            (log_dir / f"{mode}-summary.json").write_text(
                json.dumps(report, indent=2) + "\n", encoding="utf-8",
            )
            return report
        finally:
            if server.poll() is None:
                server.terminate()
            try:
                server.wait(timeout=3)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=3)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log-dir", type=Path)
    parser.add_argument("--skip-rust-build", action="store_true")
    parser.add_argument("--skip-go-build", action="store_true")
    args = parser.parse_args()
    log_dir = (args.log_dir or ROOT / "target/interop-logs" / (
        f"upstream_http2_{time.strftime('%Y%m%d_%H%M%S', time.gmtime())}_{os.getpid()}"
    )).resolve()
    try:
        log_dir.mkdir(parents=True, exist_ok=False)
        require_clean_source()
        go_version, go_env = go_toolchain()
        prepare_binaries(args.skip_rust_build, args.skip_go_build, go_version, go_env)
        dirty_check = command(["git", "diff", "--quiet", "HEAD", "--", "src", "pbrs-grpc"])
        if dirty_check.returncode not in (0, 1):
            raise RuntimeError("could not inspect native source provenance")
        dirty = dirty_check.returncode == 1
        native_revision = command(["git", "rev-parse", "HEAD"])
        if native_revision.returncode != 0 or len(native_revision.stdout.strip()) != 40:
            raise RuntimeError("native source revision is unavailable")
        reports = [
            run_mode(
                mode, log_dir, go_version, go_env, dirty,
                args.skip_rust_build, native_revision.stdout.strip(),
            )
            for mode in ("framing", "tls")
        ]
        qualified = all(report["qualified"] for report in reports)
        aggregate = {
            "schema_version": 1,
            "source_sha": GRPC_SOURCE_SHA,
            "go_version": go_version,
            "profiles": {report["mode"]: report for report in reports},
            "qualified": qualified,
        }
        (log_dir / "summary.json").write_text(
            json.dumps(aggregate, indent=2) + "\n", encoding="utf-8",
        )
        print(f"upstream Go server probes {'passed' if qualified else 'FAILED'}: {log_dir}")
        return 0 if qualified else 1
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as exc:
        print(f"upstream Go server proof incomplete: {exc}; logs: {log_dir}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
