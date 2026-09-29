"""Validate and expand frozen SB-21 scenarios before launching any process."""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path

import cells


def load(path: Path, defaults: dict, cell_filter: list[str] | None = None) -> dict:
    raw = path.read_bytes()
    scenario = json.loads(raw)
    params = scenario.get("params", {})
    if set(params) != set(defaults):
        raise ValueError("scenario params must specify exactly the runner parameters")
    for key, value in params.items():
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
            raise ValueError(f"invalid scenario parameter {key}: {value!r}")
        if value < 0 or (key != "seed" and value == 0):
            raise ValueError(f"scenario parameter {key} must be positive")
    if params["growth"] <= 1 or params["max_rate"] < params["start_rate"]:
        raise ValueError("scenario needs growth > 1 and max_rate >= start_rate")
    for key in ("seed", "max_steps"):
        if not isinstance(params[key], int):
            raise ValueError(f"scenario {key} must be an integer")
    repeats = scenario.get("repeats")
    if type(repeats) is not int or not 1 <= repeats <= 100:
        raise ValueError("scenario repeats must be an integer in 1..100")
    known_peers = set(cells.REQUIRED_SERVER_PEERS) | cells.OPTIONAL_PEERS | {"tonic"}
    by_id = {}
    for definition in scenario.get("cells", []):
        values = dict(definition)
        cid = values.pop("id")
        cell = cells.Cell(**values)
        if cell.id != cid or cid in by_id:
            raise ValueError(f"scenario cell id mismatched or duplicated: {cid}")
        if cell.role not in ("server", "client") or cell.shape not in cells.SHAPES:
            raise ValueError(f"invalid role or shape for {cid}")
        if cell.server_peer not in known_peers or cell.client_peer not in known_peers:
            raise ValueError(f"unknown peer for {cid}")
        if cell.payload not in cells.PAYLOADS or type(cell.tls) is not bool:
            raise ValueError(f"invalid payload or TLS mode for {cid}")
        if cell.compression not in ("identity", "gzip"):
            raise ValueError(f"invalid compression for {cid}")
        if type(cell.cpus) is not int or not 1 <= cell.cpus <= 1024:
            raise ValueError(f"invalid CPU budget for {cid}")
        by_id[cid] = cell
    if not by_id:
        raise ValueError("scenario cells cannot be empty")
    orders = scenario.get("frozen_order_per_repeat", {})
    if set(orders) != {f"rep{r}" for r in range(1, repeats + 1)}:
        raise ValueError("scenario must provide one frozen order for every repeat")
    for repeat, order in orders.items():
        if len(order) != len(by_id) or set(order) != set(by_id):
            raise ValueError(f"{repeat} must contain every cell exactly once")
    selected = set(cell_filter) if cell_filter is not None else set(by_id)
    if not selected or not selected <= set(by_id):
        raise ValueError(f"empty or unknown scenario cell filter: {sorted(selected - set(by_id))}")
    return {
        "name": scenario["scenario"],
        "path": str(path),
        "sha256": hashlib.sha256(raw).hexdigest(),
        "params": params,
        "repeats": repeats,
        "filtered": selected != set(by_id),
        "order": {rep: [cid for cid in order if cid in selected] for rep, order in orders.items()},
        "plan": [(rep, by_id[cid]) for rep in (f"rep{r}" for r in range(1, repeats + 1))
                 for cid in orders[rep] if cid in selected],
    }
