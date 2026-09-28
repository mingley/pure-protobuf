#!/usr/bin/env python3
"""Diff docs/grfc.md against grpc/proposal (GF-08).

Standalone standard-library Python 3 tool (zero external dependencies) that:
1. Lists merged A/G proposals in grpc/proposal at a pinned or latest commit.
2. Lists open pull requests whose titles carry an A/G proposal number.
3. Parses the gRFC/status tables in docs/grfc.md.
4. Flags new merged proposals missing from the doc (drift), doc entries with
   no merged upstream file (stale or renamed), and upcoming numbered PRs.
5. Exits 0 when the doc covers every merged proposal, or 1 on drift.

Usage:
  python3 scripts/grfc-drift.py
  python3 scripts/grfc-drift.py --ref <sha> --format markdown --output drift.md
  python3 scripts/grfc-drift.py --self-test

Network goes through the public GitHub API only (contents + pulls reads);
GITHUB_TOKEN is honored when set to raise the rate limit. The scheduled CI
job uploads the report artifact and opens nothing externally.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

EXIT_SUCCESS = 0
EXIT_DRIFT_DETECTED = 1
EXIT_USAGE_ERROR = 2

API_ROOT = "https://api.github.com/repos/grpc/proposal"
# Merged proposal files look like A42-xds-ring-hash.md / G1-true-binary-metadata.md.
PROPOSAL_FILE_RE = re.compile(r"^([AG])(\d+)-.+\.md$")
# Table rows: | A42/A76 | title | status | notes |
TABLE_ROW_RE = re.compile(r"^\|\s*([^|]+?)\s*\|")
PROPOSAL_CELL_RE = re.compile(r"\b([AG])(\d+)\b")

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DOC = ROOT / "docs" / "grfc.md"


@dataclass
class DriftReport:
    ref: str
    sha: str
    checked_at: str
    merged_upstream: List[str] = field(default_factory=list)
    tracked: List[str] = field(default_factory=list)
    new_merged: List[str] = field(default_factory=list)
    stale_entries: List[str] = field(default_factory=list)
    upcoming_prs: List[Tuple[int, str, List[str]]] = field(default_factory=list)

    @property
    def has_drift(self) -> bool:
        return bool(self.new_merged or self.stale_entries)


def api_get(path: str, params: Optional[Dict[str, str]] = None) -> object:
    url = API_ROOT + path
    if params:
        url += "?" + urllib.parse.urlencode(params)
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "pure-protobuf-grfc-drift",
    }
    token = os.environ.get("GITHUB_TOKEN", "").strip()
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        raise SystemExit(f"GitHub API {path} failed: HTTP {exc.code}") from exc
    except urllib.error.URLError as exc:
        raise SystemExit(f"GitHub API {path} failed: {exc.reason}") from exc


def default_branch() -> str:
    payload = api_get("")
    if not isinstance(payload, dict) or "default_branch" not in payload:
        raise SystemExit("could not read proposal default branch")
    return str(payload["default_branch"])


def resolve_sha(ref: str) -> str:
    payload = api_get(f"/commits/{urllib.parse.quote(ref, safe='')}")
    if not isinstance(payload, dict) or "sha" not in payload:
        raise SystemExit(f"could not resolve proposal ref {ref!r}")
    return str(payload["sha"])


def list_merged(ref: str) -> Dict[str, str]:
    """Merged proposal number -> filename at `ref` (A/G files only)."""
    payload = api_get("/contents", {"ref": ref})
    if not isinstance(payload, list):
        raise SystemExit("unexpected /contents response")
    merged: Dict[str, str] = {}
    for entry in payload:
        if not isinstance(entry, dict) or entry.get("type") != "file":
            continue
        name = str(entry.get("name", ""))
        match = PROPOSAL_FILE_RE.match(name)
        if match:
            merged[f"{match.group(1)}{match.group(2)}"] = name
    return merged


def list_open_prs() -> List[Tuple[int, str]]:
    """(number, title) for every open PR, paginated."""
    prs: List[Tuple[int, str]] = []
    page = 1
    while True:
        payload = api_get("/pulls", {"state": "open", "per_page": "100", "page": str(page)})
        if not isinstance(payload, list) or not payload:
            break
        for pr in payload:
            if isinstance(pr, dict) and "number" in pr and "title" in pr:
                prs.append((int(pr["number"]), str(pr["title"])))
        if len(payload) < 100:
            break
        page += 1
    return prs


def parse_doc(path: Path) -> Dict[str, str]:
    """Proposal number -> status from the doc's gRFC tables."""
    tracked: Dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 3 or cells[0].lower() in ("grfc", "---"):
            continue
        if not line.startswith("|"):
            continue
        numbers = PROPOSAL_CELL_RE.findall(cells[0])
        if not numbers:
            continue
        status = cells[2]
        for kind, number in numbers:
            tracked[f"{kind}{number}"] = status
    if not tracked:
        raise SystemExit(f"no gRFC rows parsed from {path}")
    return tracked


def render_text(report: DriftReport, merged_files: Dict[str, str]) -> str:
    lines = [
        f"gRFC drift vs grpc/proposal@{report.sha[:12]} (ref {report.ref})",
        f"merged upstream: {len(report.merged_upstream)}, tracked: {len(report.tracked)}",
        "",
    ]
    if report.new_merged:
        lines.append(f"NEW MERGED ({len(report.new_merged)}):")
        for number in report.new_merged:
            lines.append(f"  {number} {merged_files.get(number, '')}")
    else:
        lines.append("NEW MERGED: none")
    if report.stale_entries:
        lines.append(f"STALE ENTRIES ({len(report.stale_entries)}):")
        for number in report.stale_entries:
            lines.append(f"  {number} (no merged upstream file)")
    else:
        lines.append("STALE ENTRIES: none")
    if report.upcoming_prs:
        lines.append(f"UPCOMING PRS ({len(report.upcoming_prs)}):")
        for number, title, tags in report.upcoming_prs:
            lines.append(f"  #{number} [{','.join(tags)}] {title}")
    else:
        lines.append("UPCOMING PRS: none")
    return "\n".join(lines) + "\n"


def render_markdown(report: DriftReport, merged_files: Dict[str, str]) -> str:
    lines = [
        "# gRFC drift report",
        "",
        f"- Upstream: `grpc/proposal@{report.sha}` (ref `{report.ref}`)",
        f"- Checked at: {report.checked_at}",
        f"- Merged upstream: {len(report.merged_upstream)}; tracked in doc: {len(report.tracked)}",
        "",
        "## New merged proposals",
        "",
    ]
    if report.new_merged:
        for number in report.new_merged:
            lines.append(f"- **{number}** `{merged_files.get(number, '')}`")
    else:
        lines.append("None.")
    lines += ["", "## Stale doc entries", ""]
    if report.stale_entries:
        for number in report.stale_entries:
            lines.append(f"- **{number}** (no merged upstream file; renamed or never merged)")
    else:
        lines.append("None.")
    lines += ["", "## Upcoming numbered PRs", ""]
    if report.upcoming_prs:
        for number, title, tags in report.upcoming_prs:
            lines.append(f"- #{number} [{','.join(tags)}] {title}")
    else:
        lines.append("None.")
    lines.append("")
    return "\n".join(lines)


def sort_key(number: str) -> Tuple[str, int]:
    return (number[:1], int(number[1:]))


def build_report(
    ref: str,
    sha: str,
    merged: Dict[str, str],
    tracked: Dict[str, str],
    prs: List[Tuple[int, str]],
) -> DriftReport:
    merged_numbers = set(merged)
    tracked_numbers = set(tracked)
    new_merged = sorted(merged_numbers - tracked_numbers, key=sort_key)
    stale = sorted(tracked_numbers - merged_numbers, key=sort_key)
    upcoming: List[Tuple[int, str, List[str]]] = []
    for number, title in prs:
        tags = sorted({f"{k}{n}" for k, n in PROPOSAL_CELL_RE.findall(title)}, key=sort_key)
        tags = [tag for tag in tags if tag not in tracked_numbers]
        if tags:
            upcoming.append((number, title, tags))
    upcoming.sort()
    return DriftReport(
        ref=ref,
        sha=sha,
        checked_at=datetime.now(timezone.utc).isoformat(timespec="seconds"),
        merged_upstream=sorted(merged_numbers, key=sort_key),
        tracked=sorted(tracked_numbers, key=sort_key),
        new_merged=new_merged,
        stale_entries=stale,
        upcoming_prs=upcoming,
    )


def self_test() -> int:
    """Offline fixture check: parsing, diffing, and rendering."""
    import tempfile

    doc = (
        "# gRFC Coverage\n\n"
        "| gRFC | Title | Status | Notes |\n"
        "|---|---|---|---|\n"
        "| A6 | retries | partial | x |\n"
        "| A42/A76 | ring | shipped | y |\n"
        "| G1 | binmeta | planned | z |\n"
        "| A999 | gone | planned | stale |\n"
        "\n"
        "Prose mentioning A6 must not double-count.\n"
    )
    with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False) as handle:
        handle.write(doc)
        fixture = Path(handle.name)
    try:
        tracked = parse_doc(fixture)
    finally:
        fixture.unlink()
    assert tracked == {
        "A6": "partial",
        "A42": "shipped",
        "A76": "shipped",
        "G1": "planned",
        "A999": "planned",
    }, tracked
    merged = {"A6": "A6-x.md", "A42": "A42-x.md", "A76": "A76-x.md", "G1": "G1-x.md", "A50": "A50-x.md"}
    prs = [(123, "A50: outlier detection"), (124, "Fix a typo"), (125, "G9: future thing")]
    report = build_report("main", "abc123", merged, tracked, prs)
    assert report.new_merged == ["A50"], report.new_merged
    assert report.stale_entries == ["A999"], report.stale_entries
    assert [(n, t) for n, _, t in report.upcoming_prs] == [
        (123, ["A50"]),
        (125, ["G9"]),
    ], report.upcoming_prs
    assert report.has_drift
    text = render_text(report, merged)
    assert "A50 A50-x.md" in text and "A999" in text
    markdown = render_markdown(report, merged)
    assert markdown.startswith("# gRFC drift report") and "**A50**" in markdown
    clean = build_report("main", "abc123", {k: v for k, v in merged.items() if k != "A50"},
                         {k: v for k, v in tracked.items() if k != "A999"}, [(124, "Fix a typo")])
    assert not clean.has_drift
    print("grfc-drift self-test: OK (parse, diff, render)")
    return EXIT_SUCCESS


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Diff docs/grfc.md against grpc/proposal.")
    parser.add_argument("--ref", default=None,
                        help="proposal commit, branch, or tag (default: upstream default branch)")
    parser.add_argument("--doc", default=str(DEFAULT_DOC), help="grfc.md path")
    parser.add_argument("--format", choices=["text", "markdown"], default="text")
    parser.add_argument("--output", help="write the report to this file instead of stdout")
    parser.add_argument("--self-test", action="store_true", help="run the offline fixture check")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    doc_path = Path(args.doc)
    if not doc_path.is_file():
        print(f"error: doc not found: {doc_path}", file=sys.stderr)
        return EXIT_USAGE_ERROR
    tracked = parse_doc(doc_path)
    ref = args.ref or default_branch()
    sha = resolve_sha(ref)
    merged = list_merged(ref)
    prs = list_open_prs()
    report = build_report(ref, sha, merged, tracked, prs)
    rendered = render_markdown(report, merged) if args.format == "markdown" else render_text(report, merged)
    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return EXIT_DRIFT_DETECTED if report.has_drift else EXIT_SUCCESS


if __name__ == "__main__":
    raise SystemExit(main())
