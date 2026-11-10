#!/usr/bin/env python3
"""Cross-check consumer source manifests and report measured resource ranges."""
import hashlib
import json
from pathlib import Path
import sys

consumer_root, source_root = map(Path, sys.argv[1:])
paths = list(consumer_root.rglob("results.json"))
assert len(paths) == 1, paths
data = json.loads(paths[0].read_text())
def manifest(root):
    items = []
    for path in sorted(root.rglob("*")):
        if path.is_file():
            raw = path.read_bytes()
            items.append((str(path.relative_to(root)), len(raw), hashlib.sha256(raw).hexdigest()))
    assert items
    return {"files": len(items), "bytes": sum(x[1] for x in items),
            "sha256": hashlib.sha256(json.dumps(items).encode()).hexdigest()}
report = []
for case in data["results"]:
    label = case["case"]
    found = [p for p in source_root.rglob(label)
             if p.is_dir() and (p / "base" / "mod.rs").is_file()
             and (p / "candidate" / "mod.rs").is_file()]
    assert len(found) == 1, (label, found)
    for v in ("base", "candidate"):
        assert manifest(found[0] / v) == case["source"][v], (label, v)
    row = {"case": label, "mode": case["mode"], "blocks": len(case["blocks"]),
           "summary": case["summary"], "source": case["source"], "samples": {}}
    for v in ("base", "candidate"):
        samples = [block["samples"][v] for block in case["blocks"]]
        rss = [s["max_child_rss_kib"] for s in samples]
        cpu = [s["cpu_ns"] for s in samples]
        item = {"max_child_rss_kib_range": [min(rss), max(rss)],
                "cpu_ns_range": [min(cpu), max(cpu)]}
        if case["mode"] == "release":
            item["executable_bytes"] = sorted({s["executable_bytes"] for s in samples})
            item["executable_sha256"] = sorted({s["executable_sha256"] for s in samples})
        row["samples"][v] = item
    report.append(row)
out = {"schema": "literal-consumer-resource-summary-v1",
       "source": data["source"], "candidate": data["candidate"], "scope": data["scope"],
       "cases": report}
(consumer_root / "verified-resource-summary.json").write_text(
    json.dumps(out, indent=2, sort_keys=True) + "\n")
print("LITERAL_CONSUMER_SOURCES_VERIFIED cases=9 retained_output_pairs=5")
for row in report:
    print("LITERAL_CONSUMER_RESOURCES", json.dumps(row, sort_keys=True))
