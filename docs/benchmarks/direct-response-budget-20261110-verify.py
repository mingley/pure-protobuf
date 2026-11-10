#!/usr/bin/env python3
"""Independently recompute the published summaries from the retained run data."""
import ast
import json
import math
import re
from pathlib import Path
import statistics

ROOT = Path("evidence")

def read_json(name):
    paths = list(ROOT.rglob(name))
    assert len(paths) == 1, (name, paths)
    return json.loads(paths[0].read_text())

def records(log, prefix):
    found = []
    for line in (ROOT / log).read_text(errors="replace").splitlines():
        marker = prefix + " "
        if marker not in line:
            continue
        value = line.split(marker, 1)[1].strip()
        try:
            found.append(json.loads(value))
        except json.JSONDecodeError:
            pass
    return found

def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)

def verify_summary(rows, summaries, key_fields, variants, field):
    expected_groups = {}
    for row in rows:
        key = tuple(row[k] for k in key_fields)
        expected_groups.setdefault(key, []).append(row)
        samples = row["samples"]
        assert set(samples) == set(variants)
        n = samples[variants[1]][field]
        d = samples[variants[0]][field]
        assert n > 0 and d > 0
        near(row["ratio"], n / d)
        assert set(row["controls"]) == set(variants)
        assert all(math.isfinite(x) and x > 0 for x in row["controls"].values())
    assert len(summaries) == len(expected_groups), (key_fields, len(summaries), len(expected_groups))
    output = []
    for summary in summaries:
        key = tuple(summary[k] for k in key_fields)
        group = expected_groups.pop(key)
        ratios = [r["ratio"] for r in group]
        near(summary["median"], statistics.median(ratios))
        near(summary["range"][0], min(ratios))
        near(summary["range"][1], max(ratios))
        assert summary["slower"] == sum(x > 1 for x in ratios)
        stated_controls = summary.get("controls", summary.get("control_ranges"))
        assert set(stated_controls) == set(variants)
        for variant in variants:
            controls = [r["controls"][variant] for r in group]
            near(stated_controls[variant][0], min(controls))
            near(stated_controls[variant][1], max(controls))
        assert sorted(r["round"] for r in group) == list(range(len(group)))
        for row in group:
            assert row["order"] == list(variants if row["round"] % 2 == 0 else variants[::-1])
        output.append(summary)
    assert not expected_groups
    return output

client = read_json("shared-debt-cpu.json")
client_rows = client["rows"]
assert len(client_rows) == 80
for row in client_rows:
    for variant, sample in row["samples"].items():
        call = sample["client"]
        peer = sample["peer"]
        assert call["count"] == 10000 and call["warmup"] == 64
        assert call["mode"] == peer["mode"] == row["mode"]
        assert call["concurrency"] == row["concurrency"]
        paired = row["mode"] == "paired"
        assert call["hedges"] == (10000 if paired else 0)
        assert peer["streams"] == (2 if paired else 1) * 10064
        assert peer["responses"] == 10064
        assert peer["cancelled"] == (10064 if paired else 0)
        assert peer["pending"] == 0
        assert call["client_cpu_ns"] > 0 and sample["peer_cpu_ns"] > 0

# Pull the measured CPU field into a top-level map for the generic checker.
client_checked = []
for row in client_rows:
    row = dict(row)
    row["samples"] = {k: {"cpu": v["client"]["client_cpu_ns"]} for k, v in row["samples"].items()}
    client_checked.append(row)
client_summary = verify_summary(
    client_checked, records("client.log", "DEBT_CPU_SUMMARY_V1"),
    ("mode", "setting", "concurrency"), ("orig", "reserve"), "cpu"
)
client_alloc = read_json("shared-debt-alloc.json")["rows"]
assert len(client_alloc) == 20
client_log_alloc = records("client.log", "DEBT_ALLOCATION_V1")
assert client_alloc == client_log_alloc
for row in client_alloc:
    for key in ("allocation_calls", "reallocation_calls", "requested_bytes"):
        near(row["delta_per_rpc"][key], (row["large"][key] - row["small"][key]) / 512)

server = read_json("native-server-cpu.json")
server_rows = server["rows"]
assert len(server_rows) == 40
for row in server_rows:
    for sample in row["samples"].values():
        assert sample["successes"] == 6000 and sample["failures"] == 0
        assert sample["cpu_ns"] > 0 and sample["server_cpu_ns"] > 0
server_summary = verify_summary(
    server_rows, records("server.log", "NATIVE_SERVER_CPU_V1"),
    ("setting", "shape", "concurrency"), ("orig", "reserve"), "server_cpu_ns"
)
server_alloc = read_json("native-server-alloc.json")["rows"]
assert len(server_alloc) == 40
assert server_alloc == records("server.log", "NATIVE_SERVER_ALLOC_V1")
for row in server_alloc:
    a, b, c = row["snapshots"]
    for key in ("allocations", "requested_bytes"):
        near(row["incremental_per_rpc"][key], ((c[key] - b[key]) - (b[key] - a[key])) / 1000)
server_diag = records("server.log", "NATIVE_SERVER_DIAGNOSTIC_V1")
assert len(server_diag) == 4
for row in server_diag:
    assert row["seen"] == (row["shape"] == "client_stream")
assert {(x["setting"], x["shape"]) for x in server_diag} == {
    (s, sh) for s in ("default", "explicit") for sh in ("unary", "client_stream")
}

paired = read_json("paired-native-server-cpu.json")
paired_rows = paired["rows"]
assert len(paired_rows) == 24
for row in paired_rows:
    for sample in row["samples"].values():
        call = sample["client"]
        peer = sample["peer"]
        assert call["count"] == 6000 and call["warmup"] == 64
        assert call["concurrency"] == row["concurrency"] and call["mode"] == "paired"
        assert call["hedges"] == 6000
        assert peer == {"mode": "paired", "streams": 2 * 6064, "responses": 6064,
                        "cancelled": 6064, "pending": 0}
        assert call["client_cpu_ns"] > 0 and sample["peer_cpu_ns"] > 0
paired_summary = verify_summary(
    paired_rows, records("paired.log", "PAIR_NATIVE_CPU_SUMMARY_V1"),
    ("setting", "concurrency"), ("orig", "candidate"), "peer_cpu_ns"
)
paired_log = (ROOT / "paired.log").read_text(errors="replace")
assert re.search(r"PAIR_NATIVE_DISABLED_WITNESS_REJECTED_V1\s*$", paired_log, re.MULTILINE)
witnesses = {}
for line in paired_log.splitlines():
    match = re.search(r"PAIR_NATIVE_WITNESS_V1 (default|explicit) (orig|candidate) (\{.*\})\s*$", line)
    if match:
        witnesses[(match.group(1), match.group(2))] = ast.literal_eval(match.group(3))
assert len(witnesses) == 4
for setting in ("default", "explicit"):
    for variant in ("orig", "candidate"):
        assert witnesses[(setting, variant)] == {
            "mode": "paired", "streams": 72, "responses": 36,
            "cancelled": 36, "pending": 0,
        }

# The runners retain same-binary control ratios, but not their underlying
# numerator and denominator samples. Verify their derived ranges, not the
# missing source measurements.
out = {
    "sources": {
        "baseline": "41d94325b8aa48b10b31fb6709c1a41dbc44a72a",
        "candidate": "f1925f2a5a2e3fc759d02f2e0a1f754250c2f9c2",
        "shipped": "0685d7f6cbc4e0ac9cdfb020bd085a8b7eb5b2be",
    },
    "client": {"placement": client["placement"], "summaries": client_summary,
               "allocation_rows": len(client_alloc)},
    "native_server": {"summaries": server_summary, "diagnostics": server_diag,
                      "allocation_rows": len(server_alloc)},
    "paired_native_server": {"placement": paired["placement"], "summaries": paired_summary},
    "control_limit": "Per-block control ratios are retained, but their two underlying CPU samples are not.",
    "scope": "Shared-runner screens; no tonic comparison; no universal performance conclusion.",
}
(ROOT / "verified-summary.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
bundle = {
    "schema": "pbrs-direct-budget-evidence-v1",
    "summary": out,
    "client_cpu": client,
    "client_allocations": client_alloc,
    "native_server_cpu": server,
    "native_server_allocations": server_alloc,
    "paired_native_server_cpu": paired,
    "artifact_sha256": {
        "client": "431a9b28a270e83ff07d5c91a3f20840124fc5b47e6c1a5850e8c1355b516cce",
        "native_server": "741f3bb72c1265215c17389f93e5770e1b2c908959d989ac1d2cb4cfc8f53aec",
        "paired_native_server": "18ea73ae29d6010744804e0195808b0a0eb9d4e53a314e8b75041c67d848a618",
    },
}
(ROOT / "direct-budget-20261110.json").write_text(json.dumps(bundle, indent=2, sort_keys=True) + "\n")
print("VERIFY_FULL_BUDGET_OK client=80 client_alloc=20 server=40 server_alloc=40 paired=24")
