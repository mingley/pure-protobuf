#!/usr/bin/env python3
"""Recompute the retained lean-codegen instruction profile."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

SOURCE = "5fe70efdc798031badefd1a3847d8866dde352f5"
PROFILE_SHA256 = "af9c639fb61ffbc5ba804741901d01a4505c01c6071808bae67faa2e03ec563c"
VERIFIER_BLOB = "0296ebb5befa1ebc3800ad7c8d640a4466a00d28"
REQUEST_SHA256 = "9d9828815a721377e5cbac6d96b810e08819d1c17e14cd245af78a37407d6134"
DESCRIPTOR_SHA256 = "cb2d308fd5c8e7769964f8656da25f32d0635ff4281e9a0f2e558367596e1fa0"
TOTAL = 666229445


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def blob_sha(path):
    data = path.read_bytes()
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def parent_edges(report):
    header = re.compile(r"^\s*[\d,]+\s+\(\s*[\d.]+%\)\s+\*\s+(.*)$")
    edge = re.compile(r"^\s*([\d,]+)\s+\(\s*[\d.]+%\)\s+>\s+(.*?)\s+\(([\d,]+)x\)(?:\s+\[.*\])?\s*$")
    needles = (
        ("generate_from_code_generator_request::{closure#31}", "codegen::messages::emit_message", 303495994, 3000),
        ("codegen::messages::emit_message", "codegen::parse::emit_codec", 175051493, 3000),
        ("generate_from_code_generator_request::{closure#31}", "codegen::descriptors::FileMatcher>::matches", 48423578, 60000),
    )
    found = {pair[:2]: [] for pair in needles}
    current = ""
    for line in report.splitlines():
        h = header.match(line)
        if h:
            current = h.group(1)
            continue
        m = edge.match(line)
        if not m:
            continue
        for parent, child, _, _ in needles:
            if parent in current and child in m.group(2):
                found[(parent, child)].append((int(m.group(1).replace(",", "")),
                                                int(m.group(3).replace(",", ""))))
    rows = []
    for parent, child, expected_ir, expected_calls in needles:
        matches = found[(parent, child)]
        assert matches == [(expected_ir, expected_calls)], (parent, child, matches)
        rows.append({"caller": parent, "callee": child,
                     "guest_instructions": expected_ir, "calls": expected_calls})
    return rows


def collect(bundle):
    archive = bundle / "profile.zip"
    verifier = bundle / "verify.py"
    assert sha256(archive) == PROFILE_SHA256, "original profile ZIP changed"
    assert blob_sha(verifier) == VERIFIER_BLOB, "original verifier changed"
    with tempfile.TemporaryDirectory(prefix="pbrs-codegen-profile-") as temp:
        temp = Path(temp)
        root = temp / "evidence" / "profile"
        root.mkdir(parents=True)
        with zipfile.ZipFile(archive) as z:
            z.extractall(root)
        # The source-pinned original verifier expects evidence/profile relative
        # to its working directory and checks inputs, outputs and raw timings.
        subprocess.run([sys.executable, str(verifier.resolve())], cwd=temp, check=True)
        profile = json.loads((root / "results/profile.json").read_text())
        assert profile["source"] == SOURCE
        assert profile["request_sha256"] == REQUEST_SHA256
        env = (root / "environment.txt").read_text()
        assert "rustc 1.99.1 (58cf8f9c3 2026-10-13)" in env
        assert "valgrind-3.22.0" in env
        assert "AMD EPYC 9V74 80-Core Processor" in env
        trace = root / "results/callgrind.out"
        content = trace.read_text()
        assert re.search(r"^events:\s+Ir\s*$", content, re.M)
        m = re.search(r"^summary:\s+(\d+)\s*$", content, re.M)
        assert m and int(m.group(1)) == TOTAL
        tool = shutil.which("callgrind_annotate")
        assert tool, "callgrind_annotate is required"
        report = subprocess.run(
            [tool, "--show=Ir", "--inclusive=yes", "--threshold=99", "--tree=calling",
             "--auto=no", str(trace)], text=True, capture_output=True, timeout=120, check=True
        ).stdout
        edges = parent_edges(report)
        ratios = [r["ratio"] for r in profile["native_cpu_rows"]]
        controls = {v: [min(r["controls"][v]["ratio"] for r in profile["native_cpu_rows"]),
                        max(r["controls"][v]["ratio"] for r in profile["native_cpu_rows"])]
                    for v in ("base", "debug")}
        assert len(ratios) == 5
        assert math.isclose(profile["callgrind_over_native_bench1_median_wall"],
                            49.53687512638048, rel_tol=1e-12)
        return {
            "schema": "codegen-instruction-profile-20261110/1",
            "source": SOURCE,
            "input": {"proto_files": 20, "messages": 1000, "outputs_per_variant": 21,
                      "output_bytes_per_variant": 12925901, "request_sha256": REQUEST_SHA256,
                      "descriptor_sha256": DESCRIPTOR_SHA256},
            "environment": {"cpu": "AMD EPYC 9V74 80-Core Processor",
                            "rustc": "1.99.1", "valgrind": "3.22.0",
                            "workers": 1, "lto": "thin", "codegen_units": 1},
            "trace": {"event": "Ir", "guest_instructions": TOTAL,
                      "warmup_generations": 2, "measured_generations": 1,
                      "inclusive_direct_edges": edges},
            "native_control_ranges": controls,
            "profile": profile,
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path, help="extracted verified artifact directory")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--write", type=Path)
    group.add_argument("--check", type=Path)
    args = parser.parse_args()
    record = collect(args.bundle.resolve())
    if args.write:
        args.write.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
    else:
        assert json.loads(args.check.read_text()) == record, "retained data differs"
    print("CODEGEN_INSTRUCTION_DATA_OK edges=3 cpu_blocks=5 outputs=42")


if __name__ == "__main__":
    main()
