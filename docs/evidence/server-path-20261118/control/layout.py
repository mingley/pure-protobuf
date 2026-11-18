#!/usr/bin/env python3
import collections
import json
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
types = {
    "inbound": r"server::rpc::InboundRpc$",
    "rpc": r"server::rpc::Rpc$",
    "prepared": r"(?:server::drain::)?Prepared<",
    "unary": r"Rpc::unary[<(]",
    "client_streaming": r"Rpc::client_streaming[<(]",
    "server_streaming": r"Rpc::server_streaming[<(]",
    "bidi": r"Rpc::bidi_streaming[<(]",
    "run_unary": r"Rpc::run_unary_request[<(]",
    "run_streaming": r"Rpc::run_streaming_request[<(]",
    "read_unary": r"InboundRpc::read_unary[<(]",
    "read_streaming": r"InboundRpc::read_streaming[<(]",
}
layout = {}
for variant in ("base", "candidate"):
    values = {name: collections.Counter() for name in types}
    matched = []
    for filename in (f"{variant}-layout.txt",):
        for line in (root / filename).open():
            match = re.match(r"print-type-size type: (.*?)\s*:\s*(\d+) bytes, alignment: (\d+) bytes", line)
            if match is None:
                continue
            name, size, alignment = match.groups()
            name = name.strip("\x60")
            for kind, pattern in types.items():
                if re.search(pattern, name):
                    values[kind][f"{size}/{alignment}"] += 1
                    matched.append(line)
    layout[variant] = values
    (root / f"{variant}-matched-layout.txt").write_text("".join(matched))

print(json.dumps(layout, indent=2))
for kind in ("inbound", "rpc"):
    assert layout["base"][kind] and layout["candidate"][kind], f"missing {kind}"
    assert layout["base"][kind] == layout["candidate"][kind], f"changed {kind} layout"
for kind in types:
    if kind in ("inbound", "rpc"):
        continue
    before = sorted(size for entry, count in layout["base"][kind].items()
                    for size in [int(entry.split("/")[0])] * count)
    after = sorted(size for entry, count in layout["candidate"][kind].items()
                   for size in [int(entry.split("/")[0])] * count)
    assert len(before) == len(after), f"{kind} specialization counts changed"
    assert all(candidate <= baseline for baseline, candidate in zip(before, after)), f"{kind} grew"
