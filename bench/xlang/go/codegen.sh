#!/usr/bin/env bash
# Go peer codegen (SB-07). Generates protobuf-go and vtprotobuf code for every
# .proto vendored under one SB-05 corpus, plus a blank-import register package
# that links the generated types into the per-corpus harness binaries.
#
# Usage: codegen.sh PROTOC LOG_DIR STATUS_TSV [corpus...]
#
# Go import mapping: protoc-gen-go derives the Go package NAME from each
# file's go_package option but the import PATH from our M flags, so files
# that share a proto dir but declare different package names must not share
# an import path, and neither may files that share a dir but declare
# different proto packages (their Go type names can collide, e.g. the two
# GoogleMessage1 variants). Every file maps uniformly to
# <module>/gen/<corpus>/p/<proto-dir>/<name>/<protopkg>, where <name> is the
# go_package name when declared, else the proto dir basename, and <protopkg>
# is the proto package with dots as underscores. Output uses paths=import so
# each Go package lands in its own directory.
#
# A corpus whose codegen fails is recorded in STATUS_TSV (never fatal: the
# remaining corpora still generate, and the driver reports the failed cells
# as not_run with the recorded reason).
#
# Well-known types are NOT generated in-tree. They are M-aliased to the
# upstream google.golang.org/protobuf WKT packages: protoc-gen-go-vtproto
# assumes WKTs generated in the same invocation live in the current Go
# package and emits broken `__.X` references otherwise, while WKTs from
# another package take vtprotobuf's designed path (upstream field types with
# identical-layout casts to its own copies inside the fast paths). Both typed
# peers thus share the upstream WKT type graph; the standard peer never calls
# vt methods (see README.md).
#
# Two file classes are excluded from the vtproto invocation (they still get
# --go_out, so the standard peer is unaffected):
# - service-bearing files (today: only grpc-testing's test.proto, which
#   defines no messages): protoc-gen-go-vtproto emits gRPC service stubs for
#   them, which would add a google.golang.org/grpc module dependency whose
#   own vtprotobuf requirement floats our pinned v0.6.0 to a pseudo-version.
# - group-bearing files (today: only google-messages' message2.proto):
#   protoc-gen-go-vtproto v0.6.0 emits non-compiling code for group fields
#   (it calls MarshalToSizedBufferVT on the repeated-group slice itself).
# Excluded files are listed with reasons in the corpus register package
# (vtexcluded.go) so vt-peer cells for their messages report a precise
# not_run reason instead of a generic one.
set -u

PROTOC="$1"; LOG_DIR="$2"; STATUS_TSV="$3"; shift 3
GODIR="$(cd "$(dirname "$0")" && pwd)"
CORPORA_DIR="$GODIR/../../corpora"

if [ $# -gt 0 ]; then CORPORA="$*"; else CORPORA="google-messages googleapis grpc-testing otlp xds"; fi

MOD="$(cd "$GODIR" && go list -m)"
mkdir -p "$LOG_DIR"
: >"$STATUS_TSV"

codegen_corpus() { # corpus
  local corpus="$1"
  local root="$CORPORA_DIR/$corpus/protos"
  local log="$LOG_DIR/codegen-go-$corpus.log"
  local gen="$GODIR/gen/$corpus"
  : >"$log"
  if [ ! -d "$root" ]; then
    echo -e "$corpus\tcodegen failed: corpus protos dir missing: $root" >>"$STATUS_TSV"
    return 0
  fi
  local mapfile="$gen/.mapping.tsv"
  rm -rf "$gen"
  mkdir -p "$gen"
  # Compute the M options and the register import list; refuse inconsistent
  # mappings (same import path, different package names) loudly.
  if ! ROOT="$root" CORPUS="$corpus" MOD="$MOD" MAP="$mapfile" GENLIST="$gen/.genlist" python3 - >"$gen/.opts" 2>>"$log" <<'EOF'
import os, re, subprocess
root, corpus, mod = os.environ["ROOT"], os.environ["CORPUS"], os.environ["MOD"]
UP = "google.golang.org/protobuf/types/known/"
ALIASES = {
    "google/protobuf/any.proto": UP + "anypb",
    "google/protobuf/duration.proto": UP + "durationpb",
    "google/protobuf/empty.proto": UP + "emptypb",
    "google/protobuf/field_mask.proto": UP + "fieldmaskpb",
    "google/protobuf/struct.proto": UP + "structpb",
    "google/protobuf/timestamp.proto": UP + "timestamppb",
    "google/protobuf/wrappers.proto": UP + "wrapperspb",
    "google/protobuf/descriptor.proto": "google.golang.org/protobuf/types/descriptorpb",
}
files = sorted(subprocess.run(["find", ".", "-name", "*.proto"],
                              capture_output=True, text=True, cwd=root).stdout.split())
files = [f[2:] for f in files]
assert files, "no .proto files"
opts, imports, names, genlist = ["module=%s" % mod, "paths=import"], {}, {}, []
for f in files:
    if f in ALIASES:
        opts.append("M%s=%s" % (f, ALIASES[f]))
        continue
    genlist.append(f)
    text = open(os.path.join(root, f)).read()
    m = re.search(r'^\s*option\s+go_package\s*=\s*"([^"]+)"', text, re.M)
    d = os.path.dirname(f)
    if m:
        gp = m.group(1)
        n = gp.split(";", 1)[1] if ";" in gp else gp.rsplit("/", 1)[-1]
    else:
        n = os.path.basename(d)
    assert re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", n), (f, n)
    pm = re.search(r'^\s*package\s+([A-Za-z_][A-Za-z0-9_.]*)\s*;', text, re.M)
    assert pm, (f, "missing package declaration")
    protopkg = pm.group(1).replace(".", "_")
    imp = "%s/gen/%s/p/%s/%s/%s" % (mod, corpus, d, n, protopkg)
    if imp in names and names[imp] != n:
        raise SystemExit("inconsistent Go package names for %s: %s vs %s" % (imp, names[imp], n))
    names[imp] = n
    opts.append("M%s=%s" % (f, imp))
    imports[imp] = True
assert genlist, "only aliased files present"
open(os.environ["MAP"], "w").write("".join(i + "\n" for i in sorted(imports)))
open(os.environ["GENLIST"], "w").write("".join(g + "\n" for g in genlist))
print(",".join(opts))
EOF
  then
    echo -e "$corpus\tcodegen failed: import mapping failed; see $log" >>"$STATUS_TSV"
    rm -rf "$gen"
    return 0
  fi
  local opts
  opts="$(cat "$gen/.opts")"
  rm -f "$gen/.opts"
  local all_files vt_files
  all_files="$(tr '\n' ' ' <"$gen/.genlist")"
  rm -f "$gen/.genlist"
  # shellcheck disable=SC2086
  vt_files="$(cd "$root" && for f in $all_files; do
    grep -q '^[[:space:]]*service ' "$f" && continue
    grep -Eq '(repeated|optional|required)[[:space:]]+group[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*=[[:space:]]*[0-9]+[[:space:]]*\{' "$f" && continue
    echo "$f"
  done)"
  # shellcheck disable=SC2086
  vt_excluded="$(cd "$root" && for f in $all_files; do
    if grep -q '^[[:space:]]*service ' "$f"; then
      echo "$f|services: protoc-gen-go-vtproto emits gRPC stubs (see codegen.sh)"
    elif grep -Eq '(repeated|optional|required)[[:space:]]+group[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*=[[:space:]]*[0-9]+[[:space:]]*\{' "$f"; then
      echo "$f|groups: protoc-gen-go-vtproto v0.6.0 emits non-compiling code"
    fi
  done)"
  # shellcheck disable=SC2086
  if ! ( cd "$root" && "$PROTOC" -I. \
      --go_out="$GODIR" --go_opt="$opts" \
      $all_files >>"$log" 2>&1 ); then
    echo -e "$corpus\tcodegen failed: protoc --go_out failed; see $log" >>"$STATUS_TSV"
    rm -rf "$gen"
    return 0
  fi
  # shellcheck disable=SC2086
  if [ -n "$vt_files" ] && ! ( cd "$root" && "$PROTOC" -I. \
      --go-vtproto_out="$GODIR" --go-vtproto_opt="$opts" \
      $vt_files >>"$log" 2>&1 ); then
    echo -e "$corpus\tcodegen failed: protoc --go-vtproto_out failed; see $log" >>"$STATUS_TSV"
    rm -rf "$gen"
    return 0
  fi
  # register.go blank-imports every generated package so the harness binary
  # links (and the global registry learns) all of the corpus types.
  local pkg
  pkg="$(echo "$corpus" | tr -d '-')"
  {
    echo "// Code generated by codegen.sh. DO NOT EDIT."
    echo "// source: bench/corpora/$corpus/protos"
    echo
    echo "package $pkg"
    echo
    echo "import ("
    sed 's/.*/\t_ "&"/' "$mapfile"
    echo ")"
  } >"$gen/register.go"
  rm -f "$mapfile"
  {
    echo "// Code generated by codegen.sh. DO NOT EDIT."
    echo "// Files excluded from the vtproto invocation (see codegen.sh)."
    echo
    echo "package $pkg"
    echo
    echo "// VTExcludedFiles maps proto paths excluded from vtproto codegen to"
    echo "// the reason; the vt peer reports these instead of timing the cell."
    echo "var VTExcludedFiles = map[string]string{"
    echo "$vt_excluded" | while IFS='|' read -r f reason; do
      [ -n "$f" ] && echo "	\"$f\": \"$reason\","
    done
    echo "}"
  } >"$gen/vtexcluded.go"
  echo -e "$corpus\tok" >>"$STATUS_TSV"
}

# shellcheck disable=SC2086
for c in $CORPORA; do
  codegen_corpus "$c"
done
