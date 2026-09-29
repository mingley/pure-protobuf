"""SB-18 fairness metadata: SB-01 socket/window/TLS settings on every cell.

SB-01 pins the apples-to-apples transport settings (16 MiB HTTP/2
windows, 1 MiB frames, 256 streams, no adaptive window, 16 KiB header
list, TCP_NODELAY, TLS 1.3 AES-128-GCM). Every stack-matrix cell
records what each endpoint ran under, with per-field provenance, so a
checker can verify matched settings from result metadata alone:

- ``configured``: the harness or peer defaults set this value.
- ``observed``: read back at runtime (e.g. getsockopt).
- ``scripted``: the SB-18 harness contract a future optional-peer
  build must implement; the peer did not run in this report.
- ``upstream-default``: reference-peer defaults, recorded, not matched.
- ``negotiated``: TLS 1.3 handshake outcome, suite unrecorded.
"""

from __future__ import annotations

from typing import Any, Dict, List, Tuple

SPEC_NAME = "SB-01"

# pbrs-grpc/src/config.rs + MATCHING_CONFIGURATION in scripts/rpc-bench-matrix.py.
SB01_SPEC: Dict[str, Any] = {
    "tcp_nodelay": True,
    "stream_window": 16 * 1024 * 1024,
    "conn_window": 16 * 1024 * 1024,
    "frame_size": 1024 * 1024,
    "max_streams": 256,
    "adaptive_window": False,
    "header_list": 16 * 1024,
    "tls_cipher": "TLS_AES_128_GCM_SHA256",
    "connections": 1,
    "concurrency": 1,
}

# Socket/window fields the matcher compares. TLS cipher suites negotiate
# at handshake time and are reported separately (see tls_field).
MATCHED_FIELDS = (
    "tcp_nodelay",
    "stream_window",
    "conn_window",
    "frame_size",
    "max_streams",
    "adaptive_window",
    "header_list",
)


def _field(value: Any, provenance: str) -> Dict[str, Any]:
    return {"value": value, "provenance": provenance}


# Required (SB-11) peers: what each endpoint actually runs under.
# "tonic" is a legacy alias: server side is the tonic-prost binary,
# client side is the tonic-pbrs transport.
REQUIRED_PROVENANCE: Dict[str, Dict[str, str]] = {
    "native": {
        "kind": "configured",
        "note": (
            "pbrs-grpc defaults equal the SB-01 spec; tcp::tune sets "
            "TCP_NODELAY on every socket"
        ),
    },
    "tonic-pbrs": {
        "kind": "configured+observed",
        "note": (
            "fair_tonic_server/endpoint set SB-01 windows/frames/streams; "
            "NodelayIncoming getsockopt-verifies accepted server sockets"
        ),
    },
    "tonic-prost": {
        "kind": "upstream-default",
        "note": (
            "tonic-interop uses Server::builder() defaults; SB-01 matching "
            "covers rpc-bench transports only"
        ),
    },
    "go": {
        "kind": "upstream-default",
        "note": "upstream grpc-go interop server/client defaults; reference peer, unmatched",
    },
    "cpp": {
        "kind": "upstream-default",
        "note": "upstream grpc-c++ interop server/client defaults; reference peer, unmatched",
    },
}


def canonical_peer(peer: str, role: str) -> str:
    """Resolve the legacy ``tonic`` alias to the codec it executes."""
    if peer == "tonic":
        return "tonic-prost" if role == "server" else "tonic-pbrs"
    return peer


def tls_field(peer: str, tls_active: bool, scripted: bool) -> Dict[str, Any]:
    """TLS-cipher metadata for one endpoint."""
    if not tls_active:
        return _field(None, "n/a (plaintext cell)")
    if scripted:
        return _field(
            SB01_SPEC["tls_cipher"],
            "scripted (harness contract pins the suite to TLS_AES_128_GCM_SHA256)",
        )
    return _field(None, "negotiated (TLS 1.3 handshake; suite unrecorded)")


def endpoint(
    peer: str,
    role: str,
    tls_active: bool,
    *,
    scripted: bool = False,
) -> Dict[str, Any]:
    """Fairness block for one endpoint (``role``: server|client)."""
    if scripted:
        fields = {
            name: _field(SB01_SPEC[name], "scripted (SB-18 harness contract)")
            for name in MATCHED_FIELDS
        }
        fields["tls_cipher"] = tls_field(peer, tls_active, True)
        return {
            "peer": peer,
            "role": role,
            "spec": SPEC_NAME,
            "fields": fields,
            "matches_spec": True,
            "note": (
                f"optional peer {peer} did not run; values are the scripted "
                "SB-01 contract its harness must implement"
            ),
        }
    canon = canonical_peer(peer, role)
    prov = REQUIRED_PROVENANCE.get(canon)
    if prov is None:
        fields = {name: _field(None, "unknown peer") for name in MATCHED_FIELDS}
        fields["tls_cipher"] = tls_field(peer, tls_active, False)
        return {
            "peer": peer,
            "role": role,
            "spec": SPEC_NAME,
            "fields": fields,
            "matches_spec": False,
            "note": f"no provenance record for peer {peer}",
        }
    if prov["kind"] == "upstream-default":
        fields = {
            name: _field(None, f"upstream-default (unverified; {prov['note']})")
            for name in MATCHED_FIELDS
        }
        fields["tls_cipher"] = tls_field(peer, tls_active, False)
        return {
            "peer": peer,
            "role": role,
            "spec": SPEC_NAME,
            "fields": fields,
            "matches_spec": False,
            "note": prov["note"],
        }
    nodelay_prov = prov["kind"]
    if canon == "tonic-pbrs" and role == "server":
        nodelay_prov = "observed (getsockopt per accepted socket)"
    fields = {
        name: _field(
            SB01_SPEC[name],
            nodelay_prov if name == "tcp_nodelay" else prov["kind"],
        )
        for name in MATCHED_FIELDS
    }
    fields["tls_cipher"] = tls_field(peer, tls_active, False)
    return {
        "peer": peer,
        "role": role,
        "spec": SPEC_NAME,
        "fields": fields,
        "matches_spec": True,
        "note": prov["note"],
    }


def for_cell(
    server_peer: str,
    client_peer: str,
    tls_active: bool,
    *,
    is_optional=None,
) -> Dict[str, Any]:
    """Fairness block for a cell, keyed by endpoint role."""
    opt = is_optional or (lambda p: False)
    return {
        "spec": SPEC_NAME,
        "server": endpoint(server_peer, "server", tls_active, scripted=opt(server_peer)),
        "client": endpoint(client_peer, "client", tls_active, scripted=opt(client_peer)),
    }


def verify_report(report: Dict[str, Any]) -> Tuple[bool, List[Dict[str, Any]]]:
    """Verify SB-18 fairness from report metadata.

    Hard checks (fail): the report carries the SB-01 spec, every cell
    carries a fairness block, every optional-peer endpoint matches the
    spec, and every optional peer entry is pinned with a status.
    Required-peer mismatches (upstream-default reference servers) are
    reported as info: their sources sit outside the SB-18 write scope.
    """
    findings: List[Dict[str, Any]] = []

    def add(severity: str, check: str, detail: str) -> None:
        findings.append({"severity": severity, "check": check, "detail": detail})

    spec = report.get("fairness_spec")
    if spec != SB01_SPEC:
        add("fail", "spec", "report fairness_spec missing or differs from SB-01")
    optional = report.get("optional_peers", {})
    for peer, entry in sorted(optional.items()):
        if not isinstance(entry, dict) or not entry.get("pin"):
            add("fail", "pinned", f"optional peer {peer} has no recorded pin")
        elif entry.get("status") not in ("ready", "not_run"):
            add("fail", "status", f"optional peer {peer} has no ready/not_run status")
        elif entry.get("status") == "not_run" and not entry.get("reason"):
            add("fail", "reason", f"optional peer {peer} is not_run without a reason")
    cells = report.get("cells", [])
    for cell in cells:
        cid = cell.get("id", "?")
        fair = cell.get("fairness")
        if not isinstance(fair, dict) or "server" not in fair or "client" not in fair:
            add("fail", "metadata", f"cell {cid} carries no fairness block")
            continue
        for role in ("server", "client"):
            ep = fair[role]
            fields = ep.get("fields", {})
            is_scripted = any(
                str(f.get("provenance", "")).startswith("scripted")
                for f in fields.values()
                if isinstance(f, dict)
            )
            if is_scripted and not ep.get("matches_spec"):
                add(
                    "fail",
                    "optional-match",
                    f"cell {cid} {role} {ep.get('peer')}: scripted settings diverge from SB-01",
                )
            elif not is_scripted and not ep.get("matches_spec"):
                add(
                    "info",
                    "required-unmatched",
                    f"cell {cid} {role} {ep.get('peer')}: {ep.get('note', '')}",
                )
        if cell.get("tls") and cell.get("status") in ("pass", "fail"):
            for role in ("server", "client"):
                tls = fair[role].get("fields", {}).get("tls_cipher", {})
                if tls.get("value") is None and tls.get("provenance", "").startswith(
                    "negotiated"
                ):
                    add(
                        "info",
                        "tls-suite",
                        f"cell {cid} {role}: TLS 1.3 suite negotiated, unrecorded; "
                        "scrape from handshake/keylog in a followup",
                    )
    ok = not any(f["severity"] == "fail" for f in findings)
    return ok, findings
