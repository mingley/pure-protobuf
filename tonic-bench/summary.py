"""Summarize the per-fixture medians in a saved tonic-bench codec survey."""

import argparse
import json
import math
from pathlib import Path


OPERATIONS = {
    "first_enc": "First encode",
    "enc": "Repeated encode",
    "dec": "Owned decode",
    "touch": "Full read",
    "construct": "Construction",
}
PEERS = {"prost": "Prost", "v4": "upb"}


def read_report(text):
    raw = text.lstrip()
    if not raw.startswith("{"):
        lines = text.splitlines()
        headings = [i for i, line in enumerate(lines) if line.startswith("## Raw paired samples")]
        if len(headings) != 1:
            raise ValueError("expected one raw sample block in the survey")
        raw = "\n".join(lines[headings[0] + 1 :])
    report = json.loads(raw)
    if not isinstance(report, dict) or report.get("schema") != "tonic-raw/1":
        raise ValueError("expected a tonic-raw/1 report")
    return report


def sample_median(row, column):
    values = row.get(column)
    if (
        not isinstance(values, list)
        or not values
        or len(values) != row.get("samples")
        or any(type(v) not in (int, float) or not math.isfinite(v) or v <= 0 for v in values)
    ):
        raise ValueError(f"{row['name']} has invalid samples in {column}")
    # The Rust harness uses the upper middle sample for an even count.
    return sorted(values)[len(values) // 2]


def summarize(report, losses=False):
    rows = report.get("rows")
    if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
        raise ValueError("expected a list of measured rows")
    mutation_columns = ("pbrs_mutated_enc", "prost_mutated_enc", "v4_mutated_enc")
    fixtures = [
        row for row in rows
        if not (row.get("detail") and all(column in row for column in mutation_columns))
    ]
    names = [row.get("name") for row in fixtures]
    if not names or not all(isinstance(name, str) and name for name in names):
        raise ValueError("expected named codec fixtures")
    if len(names) != len(set(names)):
        raise ValueError("duplicate codec fixture name")

    output_mode = report.get("pbrs_prost_encode_output", "unrecorded")
    instrumented = report.get("allocation_counts")
    if instrumented is not None and type(instrumented) is not bool:
        raise ValueError("invalid allocation_counts flag")
    counters = {True: "on", False: "off", None: "unrecorded"}[instrumented]
    lines = [
        f"{len(fixtures)} codec fixtures. pbrs and Prost encode output {output_mode}. "
        f"upb encode output owned. Allocation counters {counters}.",
        "",
        "| Operation | Peer | pbrs faster | Tied | Peer faster |",
        "| --- | --- | ---: | ---: | ---: |",
    ]
    slower = []
    for metric, operation in OPERATIONS.items():
        for peer, display in PEERS.items():
            faster = tied = behind = 0
            for row in fixtures:
                ours = sample_median(row, f"pbrs_{metric}")
                theirs = sample_median(row, f"{peer}_{metric}")
                if ours < theirs:
                    faster += 1
                elif ours == theirs:
                    tied += 1
                else:
                    behind += 1
                    if losses:
                        slower.append(
                            f"| {operation} | {display} | {row['name']} | {ours:.3f} | {theirs:.3f} |"
                        )
            lines.append(f"| {operation} | {display} | {faster} | {tied} | {behind} |")
    if slower:
        lines.extend([
            "",
            "| Operation | Peer | Slower pbrs fixture | pbrs ns | Peer ns |",
            "| --- | --- | --- | ---: | ---: |",
            *slower,
        ])
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="saved survey stdout or tonic-raw/1 JSON")
    parser.add_argument("--losses", action="store_true", help="list every slower pbrs median")
    args = parser.parse_args()
    try:
        report = read_report(args.report.read_text(encoding="utf-8"))
        print(summarize(report, args.losses))
    except (OSError, ValueError) as error:
        parser.exit(1, f"{parser.prog}: {error}\n")


if __name__ == "__main__":
    main()
