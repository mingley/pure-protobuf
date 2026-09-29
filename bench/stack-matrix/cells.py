"""SB-11 cell expansion: who is under test, with what workload, pinned where.

A cell isolates one endpoint: server cells hold the load generator fixed
(rpc-bench `load`, open-loop) and vary the server peer; client cells hold
the reference server fixed and vary the client peer. Every cell names its
workload methodology so an open-loop cell is never compared as equivalent
to a soak-driven reference cell.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional

# Required peers: a peer that cannot run fails the stage, never not_run.
REQUIRED_SERVER_PEERS = ["native", "tonic-pbrs", "tonic-prost", "go", "cpp"]
REQUIRED_CLIENT_PEERS = ["native", "tonic-pbrs", "go", "cpp"]

# SB-18 optional peers: pinned and scripted in bench/stack-matrix/peers/
# and rpc-bench/peers/<id>.json. One that cannot run reports not_run
# with a reason and never blocks required peers. Must match
# peers.OPTIONAL_PEER_IDS (run.py asserts this at import).
OPTIONAL_SERVER_PEERS = [
    "grpc-java",
    "grpc-dotnet",
    "volo-grpc",
    "connect-rust",
    "google-grpc",
    "vertx",
    "quarkus",
]
# No open-loop client driver exists for any optional transport yet, so
# optional client cells are always not_run; server cells use the native
# generator. Stays empty until a driver is scripted.
OPTIONAL_CLIENT_PEERS: List[str] = []

OPTIONAL_PEERS = frozenset(OPTIONAL_SERVER_PEERS + OPTIONAL_CLIENT_PEERS)


def is_optional(peer: str) -> bool:
    return peer in OPTIONAL_PEERS

# payload -> (request_bytes, response_bytes); streaming cells reuse
# response_bytes as the per-message size.
PAYLOADS = {
    "empty": (0, 0),
    "1kib": (1024, 1024),
    "64kib": (65536, 65536),
}

SHAPES = ["unary", "server_stream", "bidi"]

# Messages per streaming RPC (contract primary workloads).
STREAM_MSGS = {"server_stream": 2000, "bidi": 256}

# CPU counts for the server/core-scaling axis (§3.6). The harness pins with
# taskset on Linux; hosts without pinning record that explicitly.
CORE_COUNTS = [1, 2, 4]

# Fixed generator for server cells, fixed server for client cells.
FIXED_GENERATOR = "native"
FIXED_SERVER = "native"

# tonic-prost has no load generator (tonic-interop only runs named interop
# cases), so it can only sit on the server side of a cell.
NO_GENERATOR = {"tonic-prost"}

# rpc-bench has no TLS for the tonic transport (tonic 0.14 TLS pulls a C
# crypto provider, conflicting with the pure-Rust dependency policy), so
# any TLS cell touching tonic-pbrs/tonic-prost is unsupported with reason.
NO_TLS_TRANSPORTS = {"tonic", "tonic-pbrs", "tonic-prost"}

# Reference soak clients drive fixed closed-loop interop loops, not the
# open-loop `load` workload; their cells are labeled, never SLO-rated.
SOAK_CLIENTS = {"go", "cpp"}


@dataclass(frozen=True)
class Cell:
    """One matrix cell: a fixed pairing plus workload plus placement."""

    role: str  # "server" (server under test) or "client" (client under test)
    server_peer: str
    client_peer: str
    shape: str
    payload: str
    tls: bool
    cpus: int

    @property
    def peer_under_test(self) -> str:
        return self.server_peer if self.role == "server" else self.client_peer

    @property
    def id(self) -> str:
        tls = "tls" if self.tls else "plain"
        return (
            f"{self.role}-{self.peer_under_test}-{self.shape}-{self.payload}"
            f"-{tls}-{self.cpus}cpu"
        )

    def workload(self) -> str:
        """Methodology label: open-loop `load` or reference soak."""
        if self.client_peer in SOAK_CLIENTS:
            return "interop-soak"
        return "open-loop-load"


def validate(cell: Cell) -> Optional[str]:
    """Return None when runnable, else the unsupported reason."""
    if cell.client_peer in NO_GENERATOR:
        return (
            f"client {cell.client_peer} has no load generator; "
            "tonic-prost is server-only"
        )
    if cell.tls and (
        cell.server_peer in NO_TLS_TRANSPORTS or cell.client_peer in NO_TLS_TRANSPORTS
    ):
        return (
            "tonic transport has no TLS support in rpc-bench "
            "(C crypto provider conflict); TLS cells need native/go/cpp endpoints"
        )
    if cell.tls and cell.client_peer in SOAK_CLIENTS:
        return (
            f"soak client {cell.client_peer} runs plaintext-only interop loops; "
            "TLS client cells use the native open-loop generator"
        )
    if cell.role == "client" and cell.client_peer in SOAK_CLIENTS:
        # Runnable but not offered-load rated; callers check workload().
        return None
    return None


def expand(
    *,
    stage: str,
    server_peers: Optional[List[str]] = None,
    client_peers: Optional[List[str]] = None,
    include_optional: bool = False,
) -> List[Cell]:
    """Expand the matrix for a stage.

    smoke: one unary/empty/plaintext cell per required server peer (fixed
      native generator) plus one per rpc-bench client peer (fixed native
      server), all at 1 CPU, plus one native TLS server cell to prove the
      TLS path. Wiring proof, not numbers.
    primary: every required peer x payload x shape x TLS at 1 CPU, plus
      the 2/4-CPU core-scaling axis on unary/1kib/plaintext.
    include_optional adds one cell per SB-18 optional peer over the same
      shapes (default off; explicit peer lists already pass through).
    """
    server_peers = server_peers if server_peers is not None else REQUIRED_SERVER_PEERS
    client_peers = client_peers if client_peers is not None else REQUIRED_CLIENT_PEERS
    if include_optional:
        server_peers = list(server_peers) + [
            p for p in OPTIONAL_SERVER_PEERS if p not in server_peers
        ]
        client_peers = list(client_peers) + [
            p for p in OPTIONAL_CLIENT_PEERS if p not in client_peers
        ]
    cells: List[Cell] = []
    if stage == "smoke":
        for peer in server_peers:
            cells.append(
                Cell(
                    role="server",
                    server_peer=peer,
                    client_peer=FIXED_GENERATOR,
                    shape="unary",
                    payload="empty",
                    tls=False,
                    cpus=1,
                )
            )
        for peer in client_peers:
            if peer in NO_GENERATOR:
                continue
            cells.append(
                Cell(
                    role="client",
                    server_peer=FIXED_SERVER,
                    client_peer=peer,
                    shape="unary",
                    payload="empty",
                    tls=False,
                    cpus=1,
                )
            )
        cells.append(
            Cell(
                role="server",
                server_peer="native",
                client_peer=FIXED_GENERATOR,
                shape="unary",
                payload="empty",
                tls=True,
                cpus=1,
            )
        )
        return cells
    if stage == "primary":
        for peer in server_peers:
            for shape in SHAPES:
                for payload in PAYLOADS:
                    for tls in (False, True):
                        cells.append(
                            Cell(
                                role="server",
                                server_peer=peer,
                                client_peer=FIXED_GENERATOR,
                                shape=shape,
                                payload=payload,
                                tls=tls,
                                cpus=1,
                            )
                        )
            for cpus in (2, 4):
                cells.append(
                    Cell(
                        role="server",
                        server_peer=peer,
                        client_peer=FIXED_GENERATOR,
                        shape="unary",
                        payload="1kib",
                        tls=False,
                        cpus=cpus,
                    )
                )
        for peer in client_peers:
            if peer in NO_GENERATOR:
                continue
            for payload in PAYLOADS:
                for tls in (False, True):
                    cells.append(
                        Cell(
                            role="client",
                            server_peer=FIXED_SERVER,
                            client_peer=peer,
                            shape="unary",
                            payload=payload,
                            tls=tls,
                            cpus=1,
                        )
                    )
        return cells
    raise ValueError(f"unknown stack-matrix stage '{stage}'")


def as_dict(cell: Cell) -> Dict[str, Any]:
    return {
        "id": cell.id,
        "role": cell.role,
        "server_peer": cell.server_peer,
        "client_peer": cell.client_peer,
        "shape": cell.shape,
        "payload": cell.payload,
        "tls": cell.tls,
        "cpus": cell.cpus,
        "workload": cell.workload(),
    }
