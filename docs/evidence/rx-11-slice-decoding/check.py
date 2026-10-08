#!/usr/bin/env python3
"""Check the retained RX-11 decoder measurements without executing binaries."""

import hashlib
import json
from pathlib import Path
import re
import statistics
import tarfile


def main():
    root = Path(__file__).resolve().parent
    index = json.loads((root / "index.json").read_text())
    archive = root / "records.tar.gz"
    if hashlib.sha256(archive.read_bytes()).hexdigest() != index["archive_sha256"]:
        raise ValueError("archive checksum mismatch")
    with tarfile.open(archive, "r:gz") as tar:
        members = {m.name: m for m in tar.getmembers() if m.isfile()}
        if set(members) != set(index["members_sha256"]):
            raise ValueError("archive member set differs")
        data = {}
        for name, member in members.items():
            with tar.extractfile(member) as source:
                payload = source.read()
            if hashlib.sha256(payload).hexdigest() != index["members_sha256"][name]:
                raise ValueError(f"{name}: checksum mismatch")
            data[name] = payload
    report = json.loads(data["paired/report.json"])
    if len(report["cells"]) != 16 or report["repeats"] != 3:
        raise ValueError("expected 16 cells and three repeats")
    for cell in report["cells"]:
        for side in ["baseline", "candidate"]:
            if len(cell["samples"][side]) != 3:
                raise ValueError("missing repeat")
            for sample in cell["samples"][side]:
                stem = f"paired/{cell['cell']}.{sample['repeat']}.{side}."
                raw = [json.loads(data[f"{stem}{m}N.stdout"]) for m in [1, 2]]
                for multiplier, row in zip([1, 2], raw):
                    command = json.loads(data[f"{stem}{multiplier}N.command.json"])
                    if command["exit"] != 0:
                        raise ValueError("failed child")
                    if row["iters"] != multiplier * cell["n"]:
                        raise ValueError("iteration mismatch")
                    if row["input_fingerprint"] != cell["input_fingerprint"]:
                        raise ValueError("input mismatch")
                for metric in ["allocs", "alloc_bytes"]:
                    if raw[1][metric] != 2 * raw[0][metric]:
                        raise ValueError("allocation window mismatch")
                    if sample[f"{metric}_per_op"] != raw[0][metric] / cell["n"]:
                        raise ValueError("allocation report mismatch")
                counts = []
                for multiplier in [1, 2]:
                    graph = data[f"{stem}{multiplier}N.callgrind"].decode()
                    match = re.search(r"^summary: (\d+)\s*$", graph, re.M)
                    if match is None:
                        raise ValueError("missing instruction summary")
                    counts.append(int(match[1]))
                if sample["instructions_per_op"] != (counts[1] - counts[0]) / cell["n"]:
                    raise ValueError("instruction report mismatch")
            for metric, median in cell["medians"][side].items():
                if median != statistics.median(s[metric] for s in cell["samples"][side]):
                    raise ValueError("median mismatch")
        before, after = [cell["medians"][side] for side in ["baseline", "candidate"]]
        if before["allocs_per_op"] - after["allocs_per_op"] != 1:
            raise ValueError("allocation delta mismatch")
        if before["alloc_bytes_per_op"] - after["alloc_bytes_per_op"] != 32768:
            raise ValueError("byte delta mismatch")
        if after["instructions_per_op"] >= before["instructions_per_op"]:
            raise ValueError("instruction regression")
    for side, name in [("baseline", "baseline-frozen"), ("candidate", "candidate")]:
        if hashlib.sha256(data[name]).hexdigest() != report["binary_sha256"][side]:
            raise ValueError("binary checksum mismatch")
    print(f"RX-11: {len(members)} artifact hashes and 16 paired decoder cells verified")


if __name__ == "__main__":
    main()
