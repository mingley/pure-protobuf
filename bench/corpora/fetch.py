#!/usr/bin/env python3
"""Fetch pinned .proto trees for the bench/corpora/ codec corpora (SB-05).

Reads the repo/pin table below, downloads each corpus's root protos plus
their transitive import closure, and writes everything under
bench/corpora/<name>/protos/ preserving import-relative paths.

Usage:
    python3 bench/corpora/fetch.py            # fetch all corpora
    python3 bench/corpora/fetch.py otlp xds   # fetch a subset
    python3 bench/corpora/fetch.py --verify   # re-download and compare hashes

Every fetched file's SHA-256 is recorded in bench/corpora/manifest.json
(scaffolded here; payload hashes are added by generate.py).
"""
import hashlib
import json
import os
import re
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(HERE, "manifest.json")

# repo -> (github slug, pinned commit, subdir inside repo that is the
# import root, license SPDX found in the repo LICENSE file)
REPOS = {
    # OTLP trace/metrics/logs schemas.
    "otlp": ("open-telemetry/opentelemetry-proto",
             "790608c4d51e6ffc12210b541e8514cbed9e91a4",
             "", "Apache-2.0"),
    # Envoy xDS schemas (api/ is the import root).
    "envoy": ("envoyproxy/envoy",
              "b579d07d3ad7ee11d32b105e91a5a39ad24718d7",
              "api", "Apache-2.0"),
    # google/api + google/rpc (+ google/longrunning closure).
    "googleapis": ("googleapis/googleapis",
                   "5174d7c27d275560a3c963d41c3911d0a060a207",
                   "", "Apache-2.0"),
    # validate/validate.proto, imported by Envoy options.
    "pgv": ("bufbuild/protoc-gen-validate",
            "92b9a7df69ca9f71bfc492f7a90adf4d36eab569",
            "", "Apache-2.0"),
    # udpa/annotations/*, imported by Envoy options.
    "udpa": ("cncf/udpa",
             "c52dc94e7fbe6449d8465faaeda22c76ca62d4ff",
             "", "Apache-2.0"),
    # xds/ annotations/core/type, imported by Envoy.
    "xds": ("cncf/xds",
            "dba9d589def2cd10099a3a64887d859188c2f57a",
            "", "Apache-2.0"),
}

# Import path prefix -> repo key above.
IMPORT_MAP = [
    ("opentelemetry/", "otlp"),
    ("envoy/", "envoy"),
    ("validate/", "pgv"),
    ("udpa/", "udpa"),
    ("xds/", "xds"),
    ("google/api/", "googleapis"),
    ("google/rpc/", "googleapis"),
    ("google/longrunning/", "googleapis"),
    ("google/cloud/", "googleapis"),
]

# Well-known google/protobuf/* imports resolve from the repo's pinned
# protobuf checkout (same v35.1 pin the compat lane builds).
WKT_SRC = os.path.join(ROOT, "third_party", "protobuf", "src")
WKT_PIN = "35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03"
WKT_LICENSE = "BSD-3-Clause"

# grpc.testing schemas resolve from the repo's pinned grpc checkout.
GRPC_SRC = os.path.join(ROOT, "third_party", "grpc")
GRPC_PIN = "d1487957db6658bc532b72871775148229836627"
GRPC_LICENSE = "Apache-2.0"

# corpus -> (classification, [root protos, import-relative])
CORPORA = {
    "otlp": ("holdout", [
        "opentelemetry/proto/trace/v1/trace.proto",
        "opentelemetry/proto/metrics/v1/metrics.proto",
        "opentelemetry/proto/logs/v1/logs.proto",
    ]),
    "xds": ("holdout", [
        "envoy/config/cluster/v3/cluster.proto",
        "envoy/config/endpoint/v3/endpoint.proto",
        "envoy/config/route/v3/route.proto",
    ]),
    "googleapis": ("holdout", [
        "google/rpc/status.proto",
        "google/rpc/error_details.proto",
        "google/api/annotations.proto",
        "google/api/http.proto",
    ]),
    "grpc-testing": ("primary", [
        "src/proto/grpc/testing/messages.proto",
        "src/proto/grpc/testing/empty.proto",
        "src/proto/grpc/testing/test.proto",
    ]),
}

IMPORT_RE = re.compile(r'^\s*import\s+"([^"]+)"\s*;', re.M)


def sha256_bytes(b):
    return hashlib.sha256(b).hexdigest()


def fetch_url(url):
    req = urllib.request.Request(url, headers={"User-Agent": "pure-protobuf-corpora"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def repo_for(path):
    for prefix, key in IMPORT_MAP:
        if path.startswith(prefix):
            return key
    if path.startswith("google/protobuf/"):
        return "wkt"
    if path.startswith("src/proto/grpc/"):
        return "grpc"
    return None


def pull(repo_key, path):
    """Return file bytes for an import-relative path."""
    if repo_key == "wkt":
        with open(os.path.join(WKT_SRC, path), "rb") as f:
            return f.read()
    if repo_key == "grpc":
        with open(os.path.join(GRPC_SRC, path), "rb") as f:
            return f.read()
    slug, sha, subdir, _ = REPOS[repo_key]
    repo_path = f"{subdir}/{path}" if subdir else path
    return fetch_url(f"https://raw.githubusercontent.com/{slug}/{sha}/{repo_path}")


def closure(roots):
    seen = {}
    stack = list(roots)
    while stack:
        path = stack.pop()
        if path in seen:
            continue
        repo_key = repo_for(path)
        if repo_key is None:
            raise SystemExit(f"no repo mapped for import {path!r}")
        try:
            data = pull(repo_key, path)
        except Exception as e:
            raise SystemExit(f"fetch failed for {path!r} from {repo_key}: {e}")
        seen[path] = (repo_key, data)
        for imp in IMPORT_RE.findall(data.decode("utf-8")):
            if imp not in seen:
                stack.append(imp)
    return seen


def write_corpus(name, files):
    protos = os.path.join(HERE, name, "protos")
    for path, (_, data) in sorted(files.items()):
        dest = os.path.join(protos, path)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        with open(dest, "wb") as f:
            f.write(data)
    return {p: {"repo": r, "sha256": sha256_bytes(d)} for p, (r, d) in files.items()}


def load_manifest():
    if os.path.exists(MANIFEST):
        return json.load(open(MANIFEST))
    return {"schema": "sb05/1", "corpora": {}}


def upstream_for(files):
    info = {k: {"repo": v[0], "commit": v[1], "license": v[3]}
            for k, v in REPOS.items()}
    info["wkt"] = {"repo": "protocolbuffers/protobuf", "commit": WKT_PIN,
                   "license": WKT_LICENSE}
    info["grpc"] = {"repo": "grpc/grpc", "commit": GRPC_PIN,
                    "license": GRPC_LICENSE}
    return [info[k] for k in sorted({r for _, (r, _) in files.items()})]


def main(args):
    verify = "--verify" in args
    names = [a for a in args if not a.startswith("--")] or sorted(CORPORA)
    manifest = load_manifest()
    failed = 0
    for name in names:
        if name not in CORPORA:
            raise SystemExit(f"unknown corpus: {name} (have: {sorted(CORPORA)})")
        cls, roots = CORPORA[name]
        print(f"== {name} ({cls}): {len(roots)} roots", flush=True)
        files = closure(roots)
        if verify:
            want = manifest.get("corpora", {}).get(name, {}).get("files", {})
            for path, (repo_key, data) in sorted(files.items()):
                got = sha256_bytes(data)
                exp = want.get(path, {}).get("sha256")
                if got != exp:
                    print(f"  MISMATCH {path}: manifest={exp} fetched={got}")
                    failed += 1
            print(f"  {len(files)} files verified" if not failed else "  FAILED")
            continue
        hashes = write_corpus(name, files)
        entry = manifest["corpora"].setdefault(name, {})
        entry["classification"] = cls
        entry["roots"] = roots
        entry["upstream"] = upstream_for(files)
        entry["files"] = hashes
        print(f"  wrote {len(files)} files", flush=True)
    if not verify:
        json.dump(manifest, open(MANIFEST, "w"), indent=1, sort_keys=True)
        print(f"manifest: {MANIFEST}")


if __name__ == "__main__":
    main(sys.argv[1:])
