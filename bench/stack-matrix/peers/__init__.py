"""SB-18 optional peers for the cross-stack matrix.

Registers grpc-java (Netty), grpc-dotnet (Kestrel), volo-grpc,
connect-rust, Google's grpc crate, and the Vert.x/Quarkus stacks from
grpc_bench. Every peer is pinned (see ``PEERS`` and
``rpc-bench/peers/<id>.json``) and scripted (build command + server
argv with SB-01-matched settings). A peer that cannot run reports
``not_run`` with a reason and never blocks required peers.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from . import fairness


class PeerNotRunnable(RuntimeError):
    """Raised when an optional peer cannot run a cell (→ not_run)."""


# Peer id -> registry record. ``harness`` is the in-tree interop project
# (a followup outside the SB-18 write scope); ``toolchains`` are PATH
# executables, each entry a list of alternatives.
PEERS: Dict[str, Dict[str, Any]] = {
    "grpc-java": {
        "pin": {"artifact": "io.grpc:grpc-netty", "version": "1.84.0"},
        "codec": "protobuf-java",
        "harness": "tests/interop/grpc-java",
        "toolchains": [["java"], ["mvn", "gradle"]],
        "build": "cd tests/interop/grpc-java && ./mvnw -q package -Dgrpc.version=1.84.0",
        "binary_candidates": ["tests/interop/grpc-java/target/grpc-java-interop-server.jar"],
        "run": ["java", "-jar", "{binary}", "--port={port}"],
        "transport_notes": (
            "NettyServerBuilder: flowControlWindow 16 MiB, "
            "maxConcurrentCallsPerConnection 256, TCP_NODELAY on; "
            "TLS pins TLS_AES_128_GCM_SHA256"
        ),
    },
    "grpc-dotnet": {
        "pin": {"artifact": "Grpc.AspNetCore", "version": "2.84.0", "tfm": "net9.0"},
        "codec": "protobuf-csharp",
        "harness": "tests/interop/grpc-dotnet",
        "toolchains": [["dotnet"]],
        "build": (
            "cd tests/interop/grpc-dotnet && "
            "dotnet publish -c Release -p:GrpcAspNetCoreVersion=2.84.0 "
            "-o ../../../target/interop-dotnet"
        ),
        "binary_candidates": ["target/interop-dotnet/GrpcDotnetInterop.dll"],
        "run": ["dotnet", "{binary}", "--port={port}"],
        "transport_notes": (
            "Kestrel: Http2 InitialConnection/StreamWindowSize 16 MiB, "
            "MaxStreamsPerConnection 256, NoDelay on; TLS CipherSuitesPolicy "
            "pins TLS_AES_128_GCM_SHA256"
        ),
    },
    "volo-grpc": {
        "pin": {"artifact": "volo-grpc", "version": "0.12.2"},
        "codec": "prost",
        "harness": "tests/interop/volo",
        "toolchains": [["cargo"], ["protoc"]],
        "build": "cargo build --locked --release --manifest-path tests/interop/volo/Cargo.toml --target-dir target",
        "binary_candidates": ["target/release/volo-interop-server"],
        "run": ["{binary}", "server", "--port={port}"],
        "transport_notes": (
            "volo-grpc server builder: 16 MiB stream/connection windows, "
            "1 MiB frames, 256 streams, TCP_NODELAY on, no adaptive window"
        ),
    },
    "connect-rust": {
        "pin": {"artifact": "connectrpc", "version": "0.9.1"},
        "codec": "prost",
        "harness": "tests/interop/connect",
        "toolchains": [["cargo"], ["protoc"]],
        "build": "cargo build --locked --release --manifest-path tests/interop/connect/Cargo.toml --target-dir target",
        "binary_candidates": ["target/release/connect-interop-server"],
        "run": ["{binary}", "server", "--port={port}"],
        "transport_notes": (
            "connectrpc grpc-over-h2 server on hyper: 16 MiB windows, "
            "1 MiB frames, 256 streams, TCP_NODELAY on"
        ),
    },
    "google-grpc": {
        "pin": {"artifact": "grpc", "version": "0.9.0"},
        "codec": "protobuf",
        "harness": "tests/interop/google-grpc",
        "toolchains": [["cargo"], ["protoc"]],
        "build": "cargo build --locked --release --manifest-path tests/interop/google-grpc/Cargo.toml --target-dir target",
        "binary_candidates": ["target/release/google-grpc-interop-server"],
        "run": ["{binary}", "server", "--port={port}"],
        "usability_gate": (
            "pinned grpc 0.9.0 declares `pub(crate) mod server` (verified "
            "from crate source 2026-09-29); no usable server until an "
            "upstream release opens it"
        ),
        "transport_notes": "blocked on the upstream usability gate above",
    },
    "vertx": {
        "pin": {
            "artifact": "io.vertx:vertx-grpc-server",
            "version": "4.4.4",
            "grpc": "1.58.0",
            "reference": "grpc_bench@48b6b95/java_vertx_grpc_bench",
        },
        "codec": "protobuf-java",
        "harness": "tests/interop/vertx",
        "toolchains": [["java"], ["gradle"]],
        "build": "cd tests/interop/vertx && ./gradlew -q installDist",
        "binary_candidates": [
            "tests/interop/vertx/build/install/vertx-interop/bin/vertx-interop"
        ],
        "run": ["{binary}", "--port={port}"],
        "transport_notes": (
            "vert.x HttpServerOptions: initialWindowSize 16 MiB, "
            "maxConcurrentStreams 256, TCP_NODELAY on"
        ),
    },
    "quarkus": {
        "pin": {
            "artifact": "io.quarkus:quarkus-grpc",
            "version": "3.1.2.Final",
            "reference": "grpc_bench@48b6b95/java_quarkus_bench",
        },
        "codec": "protobuf-java",
        "harness": "tests/interop/quarkus",
        "toolchains": [["java"], ["mvn"]],
        "build": "cd tests/interop/quarkus && ./mvnw -q package -Dquarkus.platform.version=3.1.2.Final",
        "binary_candidates": [
            "tests/interop/quarkus/target/quarkus-app/quarkus-run.jar"
        ],
        "run": ["java", "-jar", "{binary}", "--port={port}"],
        "transport_notes": (
            "quarkus-grpc over vert.x HTTP/2: 16 MiB windows, "
            "256 max streams, TCP_NODELAY on"
        ),
    },
}

OPTIONAL_PEER_IDS = sorted(PEERS)


def is_optional(peer: str) -> bool:
    return peer in PEERS


def pin(peer: str) -> Dict[str, Any]:
    return dict(PEERS[peer]["pin"])


def manifest(peer: str, repo_root: Path) -> Dict[str, Any]:
    """Load the rpc-bench manifest for an optional peer."""
    path = repo_root / "rpc-bench" / "peers" / f"{peer}.json"
    if not path.is_file():
        raise FileNotFoundError(f"optional peer {peer} has no manifest at {path}")
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def _toolchain_status(alternatives: List[List[str]]) -> Dict[str, Any]:
    found: Dict[str, Optional[str]] = {}
    missing: List[str] = []
    for group in alternatives:
        hit = next((t for t in group if shutil.which(t)), None)
        for tool in group:
            found[tool] = shutil.which(tool)
        if hit is None:
            missing.append("/".join(group))
    return {"found": found, "missing": missing}


def preflight_peer(peer: str, repo_root: Path) -> Dict[str, Any]:
    """Preflight one optional peer; never raises for a missing peer.

    Returns a status record with ``status`` ready|not_run, a ``reason``
    when not runnable, the ``pin``, and toolchain/harness evidence.
    """
    record = PEERS[peer]
    tools = _toolchain_status(record["toolchains"])
    harness = repo_root / record["harness"]
    binary = next(
        (
            str((repo_root / cand).resolve())
            for cand in record["binary_candidates"]
            if (repo_root / cand).is_file()
        ),
        None,
    )
    base: Dict[str, Any] = {
        "peer": peer,
        "pin": pin(peer),
        "harness": str(harness),
        "harness_present": harness.is_dir(),
        "toolchain": tools,
        "build": record["build"],
    }
    gate = record.get("usability_gate")
    if gate:
        return {**base, "status": "not_run", "reason": f"upstream-incapable: {gate}"}
    if not harness.is_dir():
        detail = (
            f"harness-missing: {record['harness']}/ is not in the tree "
            f"(followup outside the SB-18 write scope); build with `{record['build']}`"
        )
        if tools["missing"]:
            detail += f"; also missing toolchain: {', '.join(tools['missing'])}"
        return {**base, "status": "not_run", "reason": detail}
    if tools["missing"]:
        return {
            **base,
            "status": "not_run",
            "reason": f"toolchain-missing: {', '.join(tools['missing'])} not on PATH",
        }
    if binary is None:
        return {
            **base,
            "status": "not_run",
            "reason": f"unbuilt: harness present but no binary; run `{record['build']}`",
        }
    return {**base, "status": "ready", "reason": "", "binary": binary}


def _expand_template(
    template: List[str],
    binary: str,
    host: str,
    port: int,
    timeout_secs: float,
    tls_spec: Optional[Any],
) -> List[str]:
    tls_cert = tls_key = ""
    if tls_spec is not None:
        for arg in tls_spec.server_args:
            if arg.startswith("--tls-cert="):
                tls_cert = arg.split("=", 1)[1]
            elif arg.startswith("--tls-key="):
                tls_key = arg.split("=", 1)[1]
    return [
        piece.format(
            binary=binary,
            host=host,
            port=port,
            timeout_secs=int(timeout_secs),
            tls_cert=tls_cert,
            tls_key=tls_key,
        )
        for piece in template
    ]


def server_command(
    peer: str,
    host: str,
    port: int,
    timeout_secs: float,
    tls_spec: Optional[Any],
    repo_root: Optional[Path] = None,
) -> Tuple[List[str], str, str]:
    """Build the server argv for an optional peer.

    Raises PeerNotRunnable when the peer cannot run; the harness maps
    that to a not_run cell, never a failure.
    """
    record = PEERS[peer]
    root = repo_root or Path(__file__).resolve().parents[3]
    status = preflight_peer(peer, root)
    if status["status"] != "ready":
        raise PeerNotRunnable(status["reason"])
    binary = status["binary"]
    template = list(record["run"])
    if tls_spec is not None:
        template.extend(["--tls-cert={tls_cert}", "--tls-key={tls_key}"])
    cmd = _expand_template(template, binary, host, port, timeout_secs, tls_spec)
    return cmd, binary, record["codec"]


def tls_spec(peer: str, repo_root: Path) -> Optional[Any]:
    """Scripted TLS spec for a runnable optional peer (peertls.ServerTlsSpec).

    Optional harnesses serve the in-tree PEM test PKI so the native
    generator verifies them exactly like the native peer (ca.crt +
    ``localhost``). Returns None while the peer is unrunnable or its
    server is upstream-incapable.
    """
    import peertls

    record = PEERS[peer]
    if record.get("usability_gate"):
        return None
    if preflight_peer(peer, repo_root)["status"] != "ready":
        return None
    cert = repo_root / "pbrs-grpc" / "tests" / "tls_data" / "server.crt"
    key = repo_root / "pbrs-grpc" / "tests" / "tls_data" / "server.key"
    ca = repo_root / "pbrs-grpc" / "tests" / "tls_data" / "ca.crt"
    if not (cert.is_file() and key.is_file() and ca.is_file()):
        return None
    return peertls.ServerTlsSpec(
        peer=peer,
        server_args=[f"--tls-cert={cert}", f"--tls-key={key}"],
        ca_file=str(ca),
        server_name="localhost",
        provenance=f"in-tree PEM test PKI (SB-18 harness contract for {peer})",
    )


def versions(tool: str) -> Optional[str]:
    """Best-effort installed version string for a toolchain executable."""
    exe = shutil.which(tool)
    if exe is None:
        return None
    for args in ([exe, "--version"], [exe, "-version"], [exe, "--Version"]):
        try:
            out = subprocess.run(args, capture_output=True, text=True, timeout=10)
        except (OSError, subprocess.SubprocessError):
            continue
        text = (out.stdout + out.stderr).strip().splitlines()
        if text and (out.returncode == 0 or text[0]):
            return text[0][:200]
    return "(present, version unknown)"
