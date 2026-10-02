#!/usr/bin/env python3
"""Replay retained inventory equality without building or measuring anything."""
import json
import pathlib
import tarfile

ROOT = pathlib.Path(__file__).resolve().parents[3]
ARCHIVE = pathlib.Path(__file__).with_name("raw-artifacts.tar.gz")


def compare_retained(before, after, path=""):
    if isinstance(before, dict):
        assert isinstance(after, dict), path
        for key, value in before.items():
            assert key in after, path + "." + key
            compare_retained(value, after[key], path + "." + key)
    elif isinstance(before, list):
        assert isinstance(after, list) and len(before) == len(after), path
        for i, (left, right) in enumerate(zip(before, after)):
            compare_retained(left, right, path + f"[{i}]")
    else:
        assert before == after, (path, before, after)


with tarfile.open(ARCHIVE) as archive:
    for name, expected in [("inventory", 64), ("codec-inventory", 512), ("rpc-inventory", 64)]:
        with archive.extractfile(name + ".json") as stream:
            current = json.load(stream)
        before = json.loads((ROOT / "bench/devloop/adoption/evidence" / (name + ".json")).read_text())
        compare_retained(before, current)
        count = len(current.get("cells", current.get("cases", []))) if isinstance(current, dict) else len(current)
        assert count == expected, (name, count)
        print(f"{name}: {count} records retain every committed oracle field")
