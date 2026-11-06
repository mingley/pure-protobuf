#!/usr/bin/env python3
"""Read the pinned hosted artifact; never run its contents or a benchmark."""

import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import zipfile


BASE_SHA = "b6eb8c7b3c137e5173538baba14a192ab6b4dc65"
HEAD_SHA = "05a3cca50e7708f5be1facce6d27a7ed6b87006b"
RUN_ID = 42163543503
ARTIFACT_ID = 13480729136
ZIP_SHA256 = "aba4bb599b09ba2bf7f204380798e2a3ab08fd63c46e0aea9fc387944734ed21"
CELL_COUNT = 1634
METRICS = ("allocs", "alloc_bytes", "syscalls", "locks", "wall_ns")
FRESH_IDS = tuple(f"codec.{name}.fresh_encode" for name in ("pbrs", "prost", "v4"))
PROFILE = "independent-warmup-v1"
MEMBERS = {
    "base-rpc-fixture-overlay.json", "base.json", "base.log", "compare.txt",
    "head.json", "head.log", "summary.md", "validation.json",
}
SELECTED = (
    ("fresh_encode", "Fresh encode"),
    ("cached_encode", "Cached encode"),
    ("owned_decode", "Owned decode"),
    ("small_name80_decode", "80-byte name decode"),
    ("blob_encode_mixed", "Mixed blob encode"),
)
TABLE_START = "<!-- selected-head-table:start -->"
TABLE_END = "<!-- selected-head-table:end -->"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_json(data):
    return json.loads(data.decode("utf-8"))


def check_manifest(manifest):
    require(manifest["schema"] == "pbrs-hosted-devloop-evidence/1", "manifest schema")
    artifact = manifest["official_artifact"]
    require(artifact["run_id"] == RUN_ID, "official run ID")
    require(artifact["artifact_id"] == ARTIFACT_ID, "official artifact ID")
    require(artifact["zip_sha256"] == ZIP_SHA256, "official ZIP digest")
    require(artifact["zip_file"] == "official-artifact.zip", "official ZIP path")
    require(manifest["base_sha"] == BASE_SHA, "manifest base source")
    require(manifest["head_sha"] == HEAD_SHA, "manifest head source")
    require(manifest["cell_count"] == CELL_COUNT, "manifest cell count")
    require(tuple(manifest["measured_metrics"]) == METRICS, "manifest metrics")
    require(manifest["instruction_status"] == "not_run", "instruction availability")
    require(manifest["fresh_encode_profile"] == PROFILE, "manifest fresh lifecycle")
    require(manifest["ranking_claim"] is False, "ranking is unproven")


def metric(cell, name):
    record = cell[name]
    require(record["status"] == "measured", f"{cell['id']}: {name} not measured")
    value = record["data"]["value"]
    require(type(value) in (int, float), f"{cell['id']}: {name} is not numeric")
    require(math.isfinite(value) and value >= 0, f"{cell['id']}: {name} is not finite/nonnegative")
    require(bool(record["data"]["unit"]), f"{cell['id']}: {name} unit missing")
    return value


def selected_rows(cells):
    rows = []
    for suffix, label in SELECTED:
        pbrs = cells["codec.pbrs." + suffix]
        prost = cells["codec.prost." + suffix]
        rows.append({
            "operation": label,
            "pbrs_id": pbrs["id"], "prost_id": prost["id"],
            "pbrs_wall_ns": metric(pbrs, "wall_ns"),
            "prost_wall_ns": metric(prost, "wall_ns"),
            "pbrs_allocs": metric(pbrs, "allocs"),
            "prost_allocs": metric(prost, "allocs"),
        })
    return rows


def table(rows):
    lines = [
        "| Operation | pbrs ns/op | Prost ns/op | pbrs allocs/op | Prost allocs/op |",
        "|---|---:|---:|---:|---:|",
    ]
    for row in rows:
        lines.append(
            f"| {row['operation']} | {row['pbrs_wall_ns']:.2f} | "
            f"{row['prost_wall_ns']:.2f} | {row['pbrs_allocs']:g} | "
            f"{row['prost_allocs']:g} |"
        )
    return "\n".join(lines)


def check_records(records, manifest, readme):
    check_manifest(manifest)
    base, head, validation = (records[name] for name in ("base.json", "head.json", "validation.json"))
    all_cells = []
    for data, expected in ((base, BASE_SHA), (head, HEAD_SHA)):
        require(data["schema"] == "devloop/1", "measurement schema")
        require(data["devloop_commit"] == expected, "measurement source SHA")
        require(len(data["cells"]) == CELL_COUNT, "measurement cell count")
        cells = {cell["id"]: cell for cell in data["cells"]}
        require(len(cells) == CELL_COUNT, "duplicate measurement cell")
        for name, cell in cells.items():
            for measured in METRICS:
                metric(cell, measured)
            instructions = cell["instructions"]
            require(instructions["status"] == "not_run", f"{name}: instructions were not measured")
            require(cell["instruction_method"] == "not_run", f"{name}: instruction method")
            require(bool(instructions["data"]["reason"]), f"{name}: missing instruction reason")
            require("value" not in instructions["data"], f"{name}: fabricated instruction value")
            if name in FRESH_IDS:
                require(cell.get("fresh_encode_profile") == PROFILE, f"{name}: incompatible fresh lifecycle")
            else:
                require(cell.get("fresh_encode_profile") is None, f"{name}: misplaced fresh lifecycle")
        require(set(FRESH_IDS) <= cells.keys(), "canonical fresh cell missing")
        all_cells.append(cells)
    require(base["host"] == head["host"], "base/head host metadata differs")
    ids = set(all_cells[0])
    require(ids == set(all_cells[1]), "base/head coverage differs")
    require(validation["schema"] == "devloop-ci/1", "validation schema")
    require(validation["base_sha"] == BASE_SHA and validation["head_sha"] == HEAD_SHA, "validation source pair")
    require(validation["qualified_for_noise"] is True, "not qualified for noise comparison")
    require(validation["errors"] == [], "validation errors")
    require(validation["common_cells"] == sorted(ids), "common-cell coverage")
    require(validation["new_cells"] == [] and validation["removed_cells"] == [], "changed coverage")
    require(validation["fresh_encode_lifecycles"] == dict.fromkeys(FRESH_IDS, PROFILE), "validation lifecycles")
    eligible = validation["eligible_metrics"]
    require(set(eligible) == ids, "eligible-cell coverage")
    require(all(tuple(names) == METRICS for names in eligible.values()), "eligible metric coverage")
    rows = selected_rows(all_cells[1])
    require(rows == manifest["selected_head_rows"], "selected head values or losing rows changed")
    require(readme.count(TABLE_START) == 1 and readme.count(TABLE_END) == 1, "selected table markers")
    documented = readme.split(TABLE_START, 1)[1].split(TABLE_END, 1)[0].strip()
    require(documented == table(rows), "selected README values or losing rows changed")
    return rows


def check_packet(root):
    manifest = read_json((root / "manifest.json").read_bytes())
    check_manifest(manifest)
    archive = root / "official-artifact.zip"
    require(hashlib.sha256(archive.read_bytes()).hexdigest() == ZIP_SHA256, "ZIP differs from official artifact")
    require(archive.stat().st_size == manifest["official_artifact"]["zip_bytes"], "ZIP size")
    members = {member["name"]: member for member in manifest["members"]}
    require(len(manifest["members"]) == len(MEMBERS) and set(members) == MEMBERS, "manifest member inventory")
    payloads = {}
    with zipfile.ZipFile(archive) as zipped:
        require(len(zipped.namelist()) == len(MEMBERS) and set(zipped.namelist()) == MEMBERS, "ZIP member inventory")
        for name in sorted(MEMBERS):
            info = zipped.getinfo(name)
            require(info.file_size < 10_000_000, "unexpected large member")
            data = zipped.read(name)
            require(len(data) == members[name]["bytes"], f"{name}: member size")
            require(hashlib.sha256(data).hexdigest() == members[name]["sha256"], f"{name}: member digest")
            payloads[name] = data
    records = {name: read_json(payloads[name]) for name in ("base.json", "head.json", "validation.json")}
    readme = (root / "README.md").read_text()
    check_records(records, manifest, readme)
    return records, manifest, readme


def negative_controls(records, manifest, readme):
    controls = []

    def rejected(name, change, *, target="records"):
        altered_records, altered_manifest = copy.deepcopy(records), copy.deepcopy(manifest)
        altered_readme = readme
        if target == "records":
            change(altered_records)
        elif target == "manifest":
            change(altered_manifest)
        else:
            altered_readme = change(readme)
        try:
            check_records(altered_records, altered_manifest, altered_readme)
        except (KeyError, TypeError, ValueError):
            controls.append(name)
        else:
            raise ValueError(f"negative control accepted: {name}")

    rejected("missing metric", lambda r: r["head.json"]["cells"][0].pop("allocs"))
    rejected("nonfinite metric", lambda r: r["head.json"]["cells"][0]["wall_ns"]["data"].update(value=float("nan")))
    rejected("wrong source", lambda r: r["head.json"].update(devloop_commit=BASE_SHA))
    rejected("missing cell", lambda r: r["head.json"]["cells"].pop())
    rejected("unknown fresh profile", lambda r: next(c for c in r["head.json"]["cells"] if c["id"] == FRESH_IDS[0]).update(fresh_encode_profile="unknown-v2"))
    rejected("legacy fresh profile", lambda r: next(c for c in r["base.json"]["cells"] if c["id"] == FRESH_IDS[0]).pop("fresh_encode_profile"))
    rejected("missing eligible metrics", lambda r: r["validation.json"]["eligible_metrics"].pop(FRESH_IDS[0]))
    rejected("fabricated instructions", lambda r: r["head.json"]["cells"][0]["instructions"].update(status="measured", data={"value": 0}))
    rejected("changed losing metric", lambda r: next(c for c in r["head.json"]["cells"] if c["id"] == FRESH_IDS[0])["wall_ns"]["data"].update(value=1))
    rejected("missing losing manifest row", lambda m: m["selected_head_rows"].pop(0), target="manifest")
    rejected("changed official identity", lambda m: m["official_artifact"].update(zip_sha256="0" * 64), target="manifest")
    rejected("missing losing README row", lambda text: "\n".join(line for line in text.split("\n") if not line.startswith("| Fresh encode |")), target="readme")
    return controls


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--negative-controls", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    records, manifest, readme = check_packet(root)
    if args.negative_controls:
        print(f"Rejected {len(negative_controls(records, manifest, readme))} semantic tamper controls.")
    print("Verified 8 original members; 1,634 paired cells; 5 measured metrics each; instructions NOT_RUN; fresh lifecycle v1/v1.")


if __name__ == "__main__":
    main()
