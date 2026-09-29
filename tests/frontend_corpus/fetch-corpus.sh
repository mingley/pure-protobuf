#!/usr/bin/env bash
# Regenerate tests/frontend_corpus/{proto,corpus.json} from pinned upstreams (GN-08).
#
# Every upstream is fetched at an exact commit SHA and verified before any
# file is copied, so regeneration is reproducible: re-running this script
# must leave the working tree unchanged (check with git status). Test time
# (scripts/frontend-diff.sh) never touches the network; it only reads the
# checked-in proto/ tree and corpus.json.
#
# Usage: ./tests/frontend_corpus/fetch-corpus.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CORPUS="$ROOT/tests/frontend_corpus"
STAGE="$ROOT/target/frontend-corpus-upstream"
DEST="$CORPUS/proto"

# Pinned upstreams: name|git-url|sha|license-spdx. The SHAs are the pins;
# tags/branches are never resolved at fetch time (except protobuf, which
# reuses the repository's own vendor/google pin via fetch-protobuf.sh).
PROTOBUF_SHA="$(cat "$ROOT/vendor/google/SHA")"  # v35.1, BSD-3-Clause
PINS=(
  "googleapis|https://github.com/googleapis/googleapis.git|5d2a5100759be0b6fe5a1d3ce5e025d53d8283f4|Apache-2.0"
  "envoy|https://github.com/envoyproxy/data-plane-api.git|e23a28a41013518863542bdd7e8daba568624bc4|Apache-2.0"
  "otel|https://github.com/open-telemetry/opentelemetry-proto.git|a8951735f7801e8adfaec5c0ace9262771cfec6e|Apache-2.0"
  "grpc|https://github.com/grpc/grpc-proto.git|813330824839bfdd3abc52f41807095c0de2ec19|Apache-2.0"
  "xds|https://github.com/cncf/xds.git|dba9d589def2cd10099a3a64887d859188c2f57a|Apache-2.0"
  "pgv|https://github.com/envoyproxy/protoc-gen-validate.git|414042a5ff2e98dc47f8161937316a25b1da5bba|Apache-2.0"
)

fetch_repo() {
  local name="$1" url="$2" sha="$3"
  local dir="$STAGE/$name"
  if [[ -f "$dir/.frontend-corpus-pin" && "$(cat "$dir/.frontend-corpus-pin")" == "$sha" ]]; then
    echo "reuse $name @ $sha"
    return
  fi
  echo "fetch $name @ $sha"
  rm -rf "$dir"
  mkdir -p "$dir"
  git init -q "$dir"
  git -C "$dir" fetch -q --depth 1 "$url" "$sha"
  git -C "$dir" checkout -q "$sha"
  local got
  got="$(git -C "$dir" rev-parse HEAD)"
  if [[ "$got" != "$sha" ]]; then
    echo "pin mismatch for $name: wanted $sha got $got" >&2
    exit 1
  fi
  printf '%s\n' "$sha" > "$dir/.frontend-corpus-pin"
}

# protobuf reuses the repository pin (vendor/google) and fetcher.
"$ROOT/scripts/fetch-protobuf.sh"
ACTUAL_PB_SHA="$(git -C "$ROOT/third_party/protobuf" rev-parse HEAD)"
if [[ "$ACTUAL_PB_SHA" != "$PROTOBUF_SHA" ]]; then
  echo "protobuf pin mismatch: wanted $PROTOBUF_SHA got $ACTUAL_PB_SHA" >&2
  exit 1
fi

for pin in "${PINS[@]}"; do
  fetch_repo "${pin%%|*}" "$(echo "$pin" | cut -d'|' -f2)" "$(echo "$pin" | cut -d'|' -f3)"
done

PB="$ROOT/third_party/protobuf"
rm -rf "$DEST"
mkdir -p "$DEST"

copy() { # <src-file> <dest-relpath>
  local src="$1" rel="$2"
  mkdir -p "$DEST/$(dirname "$rel")"
  cp "$src" "$DEST/$rel"
}

# --- protobuf: every compiler-visible test proto (unittest*, test_messages*,
# WKT, compiler/plugin.proto) plus the two java feature protos, which live
# outside src/ upstream but import as google/protobuf/*.proto. ---
for f in "$PB"/src/google/protobuf/*.proto "$PB"/src/google/protobuf/compiler/*.proto; do
  rel="protobuf/${f#"$PB"/src/}"
  copy "$f" "$rel"
done
for f in "$PB"/java/core/src/main/resources/google/protobuf/*.proto; do
  copy "$f" "protobuf/google/protobuf/$(basename "$f")"
done

# --- googleapis: top-level google/api + google/rpc (+context). Deeper
# google/api subpackages and google/cloud are future corpus extensions. ---
for f in "$STAGE"/googleapis/google/api/*.proto \
         "$STAGE"/googleapis/google/rpc/*.proto \
         "$STAGE"/googleapis/google/rpc/context/*.proto; do
  copy "$f" "googleapis/${f#"$STAGE"/googleapis/}"
done

# --- envoy: data-plane-api subset whose import closure is fully pinned
# (udpa/annotations from cncf/xds, validate/validate.proto from PGV). ---
while IFS= read -r f; do
  copy "$STAGE/envoy/$f" "envoy/$f"
done <<'EOF'
envoy/type/v3/percent.proto
envoy/type/v3/http_status.proto
envoy/type/v3/range.proto
envoy/type/v3/semantic_version.proto
envoy/annotations/deprecation.proto
envoy/annotations/resource.proto
envoy/config/core/v3/base.proto
envoy/config/core/v3/address.proto
envoy/config/core/v3/backoff.proto
envoy/config/core/v3/extension.proto
envoy/config/core/v3/grpc_service.proto
envoy/config/core/v3/http_service.proto
envoy/config/core/v3/http_uri.proto
envoy/config/core/v3/socket_option.proto
envoy/config/trace/v3/trace.proto
envoy/config/trace/v3/datadog.proto
envoy/config/trace/v3/dynamic_ot.proto
envoy/config/trace/v3/http_tracer.proto
envoy/config/trace/v3/lightstep.proto
envoy/config/trace/v3/opentelemetry.proto
envoy/config/trace/v3/service.proto
envoy/config/trace/v3/zipkin.proto
EOF

# --- support legs: files the envoy closure imports (pinned like the rest). ---
while IFS= read -r f; do
  copy "$STAGE/xds/$f" "xds/$f"
done <<'EOF'
udpa/annotations/migrate.proto
udpa/annotations/sensitive.proto
udpa/annotations/status.proto
udpa/annotations/versioning.proto
xds/annotations/v3/status.proto
xds/core/v3/context_params.proto
EOF
copy "$STAGE/pgv/validate/validate.proto" "pgv/validate/validate.proto"

# --- otel: whole opentelemetry/proto tree (self-contained). ---
(cd "$STAGE/otel" && find opentelemetry/proto -name '*.proto' | sort) | while IFS= read -r f; do
  copy "$STAGE/otel/$f" "otel/$f"
done

# --- grpc-proto: whole grpc tree (needs googleapis root for google/rpc). ---
(cd "$STAGE/grpc" && find grpc -name '*.proto' | sort) | while IFS= read -r f; do
  copy "$STAGE/grpc/$f" "grpc/$f"
done

echo "vendored $(find "$DEST" -name '*.proto' | wc -l | tr -d ' ') protos into $DEST"

# --- corpus.json: pins, include roots, and one entry per file. ---
PROTOC_PIN="$(cat "$ROOT/vendor/google/PIN")"
python3 - "$CORPUS" "$PROTOC_PIN" "$PROTOBUF_SHA" <<'EOF'
import hashlib
import json
import os
import sys

corpus_dir, protoc_pin, protobuf_sha = sys.argv[1], sys.argv[2], sys.argv[3]
proto_dir = os.path.join(corpus_dir, "proto")

repos = {
    "protobuf": {
        "url": "https://github.com/protocolbuffers/protobuf.git",
        "rev": protoc_pin,
        "sha": protobuf_sha,
        "license": "BSD-3-Clause",
        "license_file": "LICENSE",
        "role": "corpus",
        "note": "Reuses the repository vendor/google pin; fetched by scripts/fetch-protobuf.sh.",
    },
    "googleapis": {
        "url": "https://github.com/googleapis/googleapis.git",
        "rev": "5d2a5100759be0b6fe5a1d3ce5e025d53d8283f4",
        "sha": "5d2a5100759be0b6fe5a1d3ce5e025d53d8283f4",
        "license": "Apache-2.0",
        "license_file": "LICENSE",
        "role": "corpus",
    },
    "envoy": {
        "url": "https://github.com/envoyproxy/data-plane-api.git",
        "rev": "e23a28a41013518863542bdd7e8daba568624bc4",
        "sha": "e23a28a41013518863542bdd7e8daba568624bc4",
        "license": "Apache-2.0",
        "license_file": "LICENSE",
        "role": "corpus",
    },
    "otel": {
        "url": "https://github.com/open-telemetry/opentelemetry-proto.git",
        "rev": "v1.9.0",
        "sha": "a8951735f7801e8adfaec5c0ace9262771cfec6e",
        "license": "Apache-2.0",
        "license_file": "LICENSE",
        "role": "corpus",
    },
    "grpc": {
        "url": "https://github.com/grpc/grpc-proto.git",
        "rev": "813330824839bfdd3abc52f41807095c0de2ec19",
        "sha": "813330824839bfdd3abc52f41807095c0de2ec19",
        "license": "Apache-2.0",
        "license_file": "LICENSE",
        "role": "corpus",
    },
    "xds": {
        "url": "https://github.com/cncf/xds.git",
        "rev": "dba9d589def2cd10099a3a64887d859188c2f57a",
        "sha": "dba9d589def2cd10099a3a64887d859188c2f57a",
        "license": "Apache-2.0",
        "license_file": "LICENSE",
        "role": "support",
        "note": "Provides udpa/annotations and xds/ imports of the envoy leg.",
    },
    "pgv": {
        "url": "https://github.com/envoyproxy/protoc-gen-validate.git",
        "rev": "414042a5ff2e98dc47f8161937316a25b1da5bba",
        "sha": "414042a5ff2e98dc47f8161937316a25b1da5bba",
        "license": "Apache-2.0",
        "license_file": "LICENSE",
        "role": "support",
        "note": "Provides validate/validate.proto imported by the envoy leg.",
    },
    "edition2024": {
        "url": None,
        "rev": None,
        "sha": None,
        "license": "BSD-3-Clause",
        "license_file": None,
        "role": "corpus",
        "note": "In-repo fixtures at tests/fixtures/edition2024 (no upstream fetch).",
    },
}

# Include roots are repository-relative directories.
roots = {
    "protobuf": "tests/frontend_corpus/proto/protobuf",
    "googleapis": "tests/frontend_corpus/proto/googleapis",
    "envoy": "tests/frontend_corpus/proto/envoy",
    "otel": "tests/frontend_corpus/proto/otel",
    "grpc": "tests/frontend_corpus/proto/grpc",
    "xds": "tests/frontend_corpus/proto/xds",
    "pgv": "tests/frontend_corpus/proto/pgv",
    "edition2024": "tests/fixtures/edition2024/proto",
    "edition2024-rejected": "tests/fixtures/edition2024/rejected",
    "repo": ".",
}

# Per-leg include roots (minimal sets verified against pinned protoc).
leg_roots = {
    "protobuf": ["protobuf"],
    "googleapis": ["googleapis", "protobuf"],
    "envoy": ["envoy", "xds", "pgv", "protobuf"],
    "xds": ["xds", "protobuf"],
    "pgv": ["pgv", "protobuf"],
    "otel": ["otel"],
    "grpc": ["grpc", "googleapis", "protobuf"],
}

REJECTED = {
    "ctype_option.proto",
    "group_syntax.proto",
    "import_weak.proto",
    "java_multiple_files.proto",
    "naming_style.proto",
    "optional_keyword.proto",
    "required_keyword.proto",
    "visibility_import_local.proto",
}


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        h.update(f.read())
    return h.hexdigest()


entries = []
for leg in ["protobuf", "googleapis", "envoy", "xds", "pgv", "otel", "grpc"]:
    leg_dir = os.path.join(proto_dir, leg)
    for dirpath, _, filenames in os.walk(leg_dir):
        for name in sorted(filenames):
            if not name.endswith(".proto"):
                continue
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, leg_dir)
            entries.append({
                "id": "%s:%s" % (leg, rel),
                "roots": leg_roots[leg],
                "files": [rel],
                "expect": "ok",
                "sha256": sha256_of(full),
            })

fixtures = os.path.join(os.path.dirname(corpus_dir), "fixtures", "edition2024")
for name in sorted(os.listdir(os.path.join(fixtures, "proto"))):
    if not name.endswith(".proto"):
        continue
    entries.append({
        "id": "edition2024:%s" % name,
        "roots": ["edition2024"],
        "files": [name],
        "expect": "ok",
        "sha256": sha256_of(os.path.join(fixtures, "proto", name)),
    })
for name in sorted(os.listdir(os.path.join(fixtures, "rejected"))):
    if not name.endswith(".proto"):
        continue
    if name in REJECTED:
        # Repo-root include covers every import shape uniformly (notably
        # the repo-relative import in visibility_import_local.proto).
        entries.append({
            "id": "edition2024-rejected:%s" % name,
            "roots": ["repo", "edition2024-rejected"],
            "files": ["tests/fixtures/edition2024/rejected/%s" % name],
            "expect": "fail",
            "sha256": sha256_of(os.path.join(fixtures, "rejected", name)),
        })
    else:
        # Support-only defs file: must compile standalone.
        entries.append({
            "id": "edition2024-rejected:%s" % name,
            "roots": ["edition2024-rejected"],
            "files": [name],
            "expect": "ok",
            "sha256": sha256_of(os.path.join(fixtures, "rejected", name)),
        })

entries.sort(key=lambda e: e["id"])
manifest = {
    "version": 1,
    "protoc": {
        "pin": protoc_pin,
        "sha": protobuf_sha,
        "build": "scripts/build-pinned-protoc.sh",
    },
    "repos": repos,
    "roots": roots,
    "entries": entries,
}
with open(os.path.join(corpus_dir, "corpus.json"), "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=1, sort_keys=False)
    f.write("\n")
print("wrote corpus.json with %d entries (%d ok, %d fail)" % (
    len(entries),
    sum(1 for e in entries if e["expect"] == "ok"),
    sum(1 for e in entries if e["expect"] == "fail")))
EOF
