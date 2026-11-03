#!/usr/bin/env python3
"""Check generated zero defaults on i686, rejecting empty or skipped controls."""

import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
TARGET = "i686-unknown-linux-gnu"
GROUP = "wire::unknown_group_structure_tests::"
LIBRARY_TESTS = (
    "lazy::tests::generated_zeroed_string_and_bytes_fields_are_empty",
    "map::tests::empty_eq_and_zeroed",
    "packed::tests::empty_eq_and_zeroed",
    "repeated::tests::empty_eq_and_zeroed",
    *(GROUP + name for name in (
        "empty_generated_messages_match_default_and_parse",
        "malformed_unknown_groups_reject_direct_parse",
        "malformed_unknown_groups_reject_lazy_parse",
        "malformed_unknown_groups_reject_validator",
        "malformed_unknown_map_entry_groups_reject_direct_and_lazy",
        "malformed_unknown_message_set_groups_reject_both_item_forms",
        "unknown_groups_keep_total_recursion_budget",
        "unknown_length_delimited_payloads_remain_opaque",
        "valid_unknown_groups_roundtrip_direct_lazy_and_validator",
        "wrong_wire_known_field_group_rejects_mismatched_end",
    )),
)
RUNTIME_TESTS = ("tat_default_is_empty_and_zeroed", "zeroed_primitives_are_valid_defaults")


def validate_inventory(output, required, filters=None):
    names = [line.removesuffix(": test") for line in output.splitlines()
             if line.endswith(": test")]
    missing = set(required) - set(names)
    if missing or len(names) != len(set(names)):
        raise ValueError("missing or duplicated zero-default controls: " + ", ".join(sorted(missing)))
    if filters is not None:
        uncovered = [name for name in required if not any(pattern in name for pattern in filters)]
        if uncovered:
            raise ValueError("zero-default controls are outside executed filters: " + ", ".join(uncovered))
    return names


def validate_result(output, total, selected):
    summaries = re.findall(
        r"^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; "
        r"(\d+) measured; (\d+) filtered out;", output, re.MULTILINE)
    expected = (selected, 0, 0, 0, total - selected)
    if (selected <= 0 or len(summaries) != 1
            or tuple(map(int, summaries[0])) != expected):
        raise ValueError(f"zero-default result differs from selected inventory: {summaries}, expected {expected}")


def run(command):
    print("Running: " + shlex.join(command), flush=True)
    process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True)
    lines = []
    for line in process.stdout:
        sys.stdout.write(line)
        sys.stdout.flush()
        lines.append(line)
    if process.wait():
        raise subprocess.CalledProcessError(process.returncode, command)
    return "".join(lines)


def main():
    if "-Zmiri-strict-provenance" not in shlex.split(os.environ.get("MIRIFLAGS", "")):
        raise ValueError("MIRIFLAGS must enable -Zmiri-strict-provenance")
    base = ["cargo", "miri", "test", "--locked", "-p", "pbrs", "--target", TARGET]
    passed = 0
    for selection, required, filters in (
        (["--lib"], LIBRARY_TESTS, ("zeroed", GROUP)),
        (["--test", "runtime"], RUNTIME_TESTS, ("zeroed",)),
    ):
        command = base + selection
        inventory = validate_inventory(
            run(command + ["--", "--list", "--color", "never"]), required, filters)
        for pattern in filters:
            selected = sum(pattern in name for name in inventory)
            output = run(command + [pattern, "--", "--test-threads=1", "--color", "never"])
            validate_result(output, len(inventory), selected)
            passed += selected
    print(f"i686 strict Miri zero-default controls: {passed} passed", flush=True)


if __name__ == "__main__":
    main()
