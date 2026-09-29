#!/usr/bin/env bash
# QG-04: pure-Rust dependency audit per shipping profile.
#
# Resolves the feature-unified graph (cargo tree, all features, all
# targets) for each shipping profile and fails on crates with C build
# steps, `links` keys, or non-approved licenses. Prints each profile's
# crate set for review. Stdlib python3 only.
set -euo pipefail
cd "$(dirname "$0")/.."

python3 - "$@" <<'PY'
import json
import re
import subprocess
import sys

# Profiles: (name, cargo tree -p args). --all-features keeps the gate
# strict: any shippable feature combo must stay pure.
PROFILES = [
    ("core", ["-p", "pbrs"]),
    ("grpc", ["-p", "pbrs-grpc"]),
    ("tonic", ["-p", "protobuf-tonic"]),
    # protoc-gen-pbrs ships from the core package; the profile asserts
    # the same closure plus no protoc-wrapper crates (Rust-only codegen).
    ("codegen", ["-p", "pbrs"]),
]

# Crates that compile or link C/C++: never shippable in a pure profile.
C_DENY = {
    "aws-lc-rs",
    "aws-lc-sys",
    "boring-sys",
    "cc",
    "cmake",
    "libz-sys",
    "openssl-src",
    "openssl-sys",
    "ring",
    "zlib-sys",
}

# protoc wrappers: forbidden in the codegen profile only.
PROTOC_DENY = {
    "protobuf-src",
    "protoc-bin-vendored",
    "protoc-rust",
}

# Approved license identifiers (expressions may combine them with
# OR/AND/parens). CDLA-Permissive-2.0 covers the Mozilla trust store
# data in webpki-roots; Unicode-3.0 covers unicode-ident tables.
LICENSE_OK = {
    "MIT",
    "Apache-2.0",
    "ISC",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Zlib",
    "Unlicense",
    "0BSD",
    "MIT-0",
    "Unicode-3.0",
    "CDLA-Permissive-2.0",
}

# `links` keys that are pure import-linkage shims, not C builds.
LINKS_ALLOW = {
    # wasm32 import namespace; no build script, no C compilation.
    ("wasm-bindgen-shared", "wasm_bindgen"),
}


def license_approved(expression):
    """Approve when some top-level OR branch uses only approved ids.

    Strips WITH exceptions and parens, then splits on OR: an AND branch
    containing copyleft still rejects. Sound for realistic SPDX license
    expressions (OR at top, AND for combined work).
    """
    expression = re.sub(r"\s+WITH\s+\S+", "", expression)
    expression = expression.replace("(", " ").replace(")", " ")
    for branch in expression.split("OR"):
        tokens = re.split(r"[\s/]+", branch)
        idents = {t for t in tokens if t not in ("", "AND")}
        if idents and idents <= LICENSE_OK:
            return True
    return False


def tree_set(args):
    cmd = [
        "cargo", "tree",
        *args,
        "--all-features",
        "--target", "all",
        "-e", "normal,build",
        "--prefix", "none",
        "--no-dedupe",
        "-f", "{p}",
    ]
    out = subprocess.run(cmd, capture_output=True, text=True, check=True).stdout
    members = set()
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        line = re.sub(r" \(\*\)$", "", line)
        # Strip "(proc-macro)" / "(/path)" suffixes, keep name + version.
        line = re.sub(r" \([^()]*\)$", "", line)
        name, _, version = line.rpartition(" v")
        if not name or not version:
            print(f"cannot parse tree line: {line!r}", file=sys.stderr)
            sys.exit(2)
        members.add((name, version))
    return members


def main():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--all-features"],
            capture_output=True, text=True, check=True,
        ).stdout
    )
    pkgs = {(p["name"], p["version"]): p for p in meta["packages"]}
    by_id = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

    failures = 0
    for profile, args in PROFILES:
        members = tree_set(args)
        print(f"profile {profile} ({len(members)} crates):")
        for name, version in sorted(members):
            print(f"  {name} {version}")
        violations = []

        build_tools = {n for n, _ in members} & {"cc", "cmake"}
        for name, version in sorted(members):
            pkg = pkgs.get((name, version))
            if pkg is None:
                violations.append(f"{name} {version}: not in cargo metadata")
                continue
            if name in C_DENY:
                violations.append(f"{name} {version}: denied C crate")
            if profile == "codegen" and name in PROTOC_DENY:
                violations.append(f"{name} {version}: protoc wrapper in codegen profile")
            links = pkg.get("links")
            if links and (name, links) not in LINKS_ALLOW:
                violations.append(f"{name} {version}: links key `{links}`")
            if build_tools:
                node = nodes.get(pkg["id"])
                if node:
                    build_deps = {
                        by_id[d["pkg"]]["name"]
                        for d in node["deps"]
                        if any((k["kind"] or "normal") == "build" for k in d["dep_kinds"])
                        and d["pkg"] in by_id
                    }
                    culprits = sorted(build_deps & build_tools)
                    if culprits:
                        violations.append(
                            f"{name} {version}: build script uses {', '.join(culprits)}"
                        )
            lic = pkg.get("license") or ""
            if not lic:
                violations.append(f"{name} {version}: missing license")
                continue
            if not license_approved(lic):
                violations.append(
                    f"{name} {version}: non-approved license `{lic}`"
                )
        if violations:
            print(f"profile {profile}: FAIL")
            for v in violations:
                print(f"  violation: {v}")
            failures += 1
        else:
            print(f"profile {profile}: pure")
    if failures:
        sys.exit(1)
    print("pure-rust audit: all profiles pure")


main()
PY
