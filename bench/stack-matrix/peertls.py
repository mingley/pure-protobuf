"""SB-11 TLS material: which certs each peer serves and what verifies them.

All cells verify; there is no skip-verify path. Reference peers serve
their own upstream test credentials (`-use_tls` / `--use_tls`), and the
native generator verifies with the matching test CA plus the documented
server-name override. A peer whose material cannot be resolved fails its
TLS cells with a reason instead of running plaintext on a TLS cell.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional


@dataclass(frozen=True)
class ServerTlsSpec:
    """How to start a TLS server for a peer, and how to verify it."""

    peer: str
    # Extra server argv (appended after the base args template).
    server_args: List[str]
    # CA file the native generator verifies against.
    ca_file: str
    # Server name the native generator verifies.
    server_name: str
    # Where the material comes from (for the report).
    provenance: str


def _go_testdata(repo_root: Path) -> Optional[Path]:
    """Locate the pinned grpc-go module's testdata directory."""
    try:
        out = subprocess.run(
            ["go", "env", "GOMODCACHE"],
            capture_output=True,
            text=True,
            timeout=15,
            cwd=repo_root / "tests" / "interop" / "go",
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if out.returncode != 0:
        return None
    modcache = Path(out.stdout.strip())
    # Version comes from the pinned go.mod require line.
    version = None
    try:
        for line in (repo_root / "tests" / "interop" / "go" / "go.mod").read_text().splitlines():
            line = line.strip()
            if line.startswith("require google.golang.org/grpc "):
                version = line.split()[-1]
                break
    except OSError:
        return None
    if version is None:
        return None
    testdata = modcache / "google.golang.org" / f"grpc@{version}" / "testdata"
    if not (testdata / "ca.pem").is_file():
        return None
    return testdata


def server_spec(peer: str, repo_root: Path) -> Optional[ServerTlsSpec]:
    """Resolve the TLS server spec for `peer`, or None when unresolvable."""
    if peer in ("native", "tonic-pbrs", "tonic-prost", "tonic"):
        cert = repo_root / "pbrs-grpc" / "tests" / "tls_data" / "server.crt"
        key = repo_root / "pbrs-grpc" / "tests" / "tls_data" / "server.key"
        ca = repo_root / "pbrs-grpc" / "tests" / "tls_data" / "ca.crt"
        if not (cert.is_file() and key.is_file() and ca.is_file()):
            return None
        return ServerTlsSpec(
            peer=peer,
            server_args=[f"--tls-cert={cert}", f"--tls-key={key}"],
            ca_file=str(ca),
            server_name="localhost",
            provenance="pbrs-grpc/tests/tls_data (test-only, loopback)",
        )
    if peer == "go":
        testdata = _go_testdata(repo_root)
        if testdata is None:
            return None
        return ServerTlsSpec(
            peer="go",
            server_args=["-use_tls"],
            ca_file=str(testdata / "ca.pem"),
            # server1.pem SANs: *.test.google.fr, *.test.youtube.com.
            server_name="foo.test.google.fr",
            provenance=f"grpc-go module testdata ({testdata})",
        )
    if peer == "cpp":
        creds = repo_root / "third_party" / "grpc" / "src" / "core" / "tsi" / "test_creds"
        ca = creds / "ca.pem"
        if not ca.is_file():
            return None
        return ServerTlsSpec(
            peer="cpp",
            server_args=["--use_tls=true"],
            ca_file=str(ca),
            # test_credentials_provider.cc pins foo.test.google.fr.
            server_name="foo.test.google.fr",
            provenance=f"grpc test_creds ({creds})",
        )
    return None


def spec_dict(spec: ServerTlsSpec) -> Dict[str, str]:
    return {
        "peer": spec.peer,
        "server_args": " ".join(spec.server_args),
        "ca_file": spec.ca_file,
        "server_name": spec.server_name,
        "provenance": spec.provenance,
    }
