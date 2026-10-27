"""Summarize shared-consumer Cargo output and validate executed suite identities."""

import re
import sys
from pathlib import Path


ANSI = re.compile(r"\x1b\[[0-9;]*m")
RUNNING = re.compile(r"\bRunning\s+(.+)$")
INTEGRATION_SOURCE = re.compile(r"tests/([a-zA-Z0-9_]+)\.rs")
RESULT = re.compile(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed")


def parse_results(output: str) -> dict[str, tuple[str, int, int]]:
    crates = {}
    current = None
    for line in ANSI.sub("", output).splitlines():
        run = RUNNING.search(line)
        if run:
            if current is not None:
                raise ValueError(f"missing test result for {current}")
            source, separator, executable = run.group(1).partition(" (")
            if not separator or not executable.endswith(")") or executable == ")":
                raise ValueError(f"malformed Cargo Running event: {run.group(1)}")
            if source.startswith("unittests "):
                continue
            integration = INTEGRATION_SOURCE.fullmatch(source)
            if integration is None:
                raise ValueError(f"unrecognized integration test path: {source}")
            current = integration.group(1)
        result = RESULT.search(line)
        if result and current is not None:
            if current in crates:
                raise ValueError(f"duplicate test result for {current}")
            crates[current] = (
                result.group(1),
                int(result.group(2)),
                int(result.group(3)),
            )
            current = None
    if current is not None:
        raise ValueError(f"missing test result for {current}")
    return crates


def main(path: Path, require_inventory: bool = False) -> None:
    crates = parse_results(path.read_text(encoding="utf-8"))
    if require_inventory:
        if __package__:
            from .shared_consumer_inventory import validate_results
        else:
            from shared_consumer_inventory import validate_results
        validate_results(crates)
    for name, (status, passed, failed) in sorted(crates.items()):
        mark = "+" if status == "ok" and failed == 0 else "!"
        print(f"  {mark} {name:<35} {status:>6} ({passed} passed, {failed} failed)")
    print(f"__CRATES_COUNT__={len(crates)}")
    print(f"__TOTAL_PASSED__={sum(entry[1] for entry in crates.values())}")
    print(f"__TOTAL_FAILED__={sum(entry[2] for entry in crates.values())}")


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[2] != "--require-inventory"):
        sys.exit("usage: shared_consumer_report.py <cargo-test-log> [--require-inventory]")
    try:
        main(Path(sys.argv[1]), require_inventory=len(sys.argv) == 3)
    except (OSError, ValueError) as exc:
        sys.exit(f"shared consumer report: {exc}")
