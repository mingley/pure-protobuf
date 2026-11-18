#!/usr/bin/env python3
"""Audit and replay the original packed boolean binaries without rebuilding."""

import difflib
import hashlib
import json
import pathlib
import re
import statistics
import subprocess
import sys

input_dir, output_dir = map(pathlib.Path, sys.argv[1:])
cases = (
    "boolc.4096.wirecoldget", "boolo.4096.wirecoldget",
    "boolc.8.wirecoldget", "boolo.8.wirecoldget",
    "boolc.4096.wirewarmget", "boolo.4096.wirewarmget",
    "boolc.4096.wirecold", "fx32.1024.warm",
)
assert hashlib.sha256((input_dir / "results.json").read_bytes()).hexdigest() == "5cc5e6fd79c8d42477fa9535312dc37b38422f3b70751a1bc5817723e0100503"
variants = ("a", "b", "c")

def ratios(label, path):
    report = json.loads(path.read_text())
    summaries = {}
    for case, details in report["cases"].items():
        blocks = details["blocks"]
        values = [block["ba"] / block["ca"] for block in blocks]
        def median_ns(name):
            return statistics.median(row["cpu_ns"] / row["n"]
                                     for block in blocks
                                     for key, row in zip(block["order"], block["rows"])
                                     if key == name)
        summary = {
            "ba": details["ba"], "ca": details["ca"],
            "bc": {"median": statistics.median(values), "range": [min(values), max(values)],
                   "slower": sum(v > 1 for v in values), "pairs": values},
            "cpu_ns_op": {name: median_ns(name) for name in variants},
            "min_process_cpu_ns": details["min_process_cpu_ns"],
        }
        summaries[case] = summary
        if case in cases:
            print(label, case, json.dumps(summary, separators=(",", ":")), flush=True)
    (output_dir / (label + "-paired.json")).write_text(json.dumps(summaries, indent=2) + "\n")

ratios("original", input_dir / "results.json")

symbols = {}
instructions = {}
body_header = re.compile(r"^([0-9a-f]+) <(.+)>:$")
instruction = re.compile(r"^\s*([0-9a-f]+):\s+(.*)$")

def interested(name):
    return ("bool_component" in name or "observe_values" in name
            or "bulk_decode_varints" in name or "validate_bulk_16" in name
            or ("Bools" in name and "packed" in name)
            or ("Packed<C>" in name and "as_slice" in name))

def normalize(body):
    normalized = []
    for line in body:
        match = instruction.match(line)
        if not match:
            continue
        insn = match.group(2)
        # Retain the mnemonic, operands and named branch target. Displacement
        # and raw addresses can change as symbols move in the executable.
        insn = re.sub(r"#\s*[0-9a-f]+\s+(?=<)", "# ", insn)
        insn = re.sub(r"\b[0-9a-f]+\s+(?=<)", "", insn)
        insn = re.sub(r"0x[0-9a-f]+\(%rip\)", "REL(%rip)", insn)
        insn = re.sub(r"\+0x[0-9a-f]+(?=>)", "+OFFSET", insn)
        normalized.append(insn)
    return normalized

for variant in variants:
    binary = input_dir / "bin" / variant
    raw_symbols = subprocess.check_output(
        ["nm", "-n", "-S", "-C", "--defined-only", binary], text=True)
    raw_asm = subprocess.check_output(
        ["objdump", "-d", "-C", "--no-show-raw-insn", binary], text=True)
    (output_dir / (variant + "-symbols.txt")).write_text(raw_symbols)
    (output_dir / (variant + "-assembly.txt")).write_text(raw_asm)
    symbols[variant] = {}
    for line in raw_symbols.splitlines():
        parts = line.split(maxsplit=3)
        if len(parts) == 4 and interested(parts[3]):
            start, size, _, name = parts
            symbols[variant].setdefault(name, []).append((start, size))
    selected = {}
    in_body = None
    for line in raw_asm.splitlines():
        match = body_header.match(line)
        if match:
            name = match.group(2)
            in_body = [] if interested(name) else None
            if in_body is not None:
                selected.setdefault(name, []).append(in_body)
        elif in_body is not None:
            in_body.append(line)
    instructions[variant] = {k: [normalize(body) for body in bodies]
                             for k, bodies in selected.items()}

def signature(bodies):
    # Separate duplicate demangled monomorphization names by hashed body.
    return sorted(hashlib.sha256("\n".join(body).encode()).hexdigest()[:16]
                  for body in bodies)

layouts = {}
for name in sorted(set().union(*(symbols[v].keys() for v in variants))):
    ids = {v: signature(instructions[v].get(name, [])) for v in variants}
    layout = {v: [{"address": address, "size": int(size, 16), "mod_64": int(address, 16) % 64}
                  for address, size in symbols[v].get(name, [])] for v in variants}
    layouts[name] = {"symbols": layout, "normalized_instructions": ids}
    print("LAYOUT", name, json.dumps(layouts[name], separators=(",", ":")), flush=True)
    if len(instructions["a"].get(name, [])) == len(instructions["b"].get(name, [])) == 1:
        a = instructions["a"][name][0]
        b = instructions["b"][name][0]
        if a != b:
            diff = "\n".join(difflib.unified_diff(a, b, n=2, fromfile="a", tofile="b"))
            with (output_dir / "normalized-diffs.txt").open("a") as out:
                out.write(name + "\n" + diff + "\n")
(output_dir / "layout.json").write_text(json.dumps(layouts, indent=2) + "\n")

driver = (input_dir / "source" / "compare.py").read_text()
marker = "allowed = sorted(os.sched_getaffinity(0))"
assert driver.count(marker) == 1
driver = driver.replace(marker, "CASES = {k: CASES[k] for k in " + repr(cases) + "}\n" + marker)
replay_script = output_dir / "compare-selected.py"
replay_script.write_text(driver)
replay_results = output_dir / "replay-results.json"
subprocess.run([sys.executable, replay_script, input_dir / "bin", replay_results], check=True)
ratios("replay", replay_results)
