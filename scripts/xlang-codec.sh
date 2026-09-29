#!/usr/bin/env bash
# Cross-language codec peers (SB-06, SB-07).
#
# Builds the pinned C++ protobuf harness (heap and arena modes), the upb C
# harness, and the pinned Go harnesses (protobuf generated code, vtprotobuf
# fast paths, hyperpb dynamic parser) from SHA-verified sources, drives them
# with the SB-05 corpora payloads, and emits devloop/1 JSON (the dev-loop/
# claim report schema, so `devloop compare` can threshold these cells like
# any other).
#
# Accept-item mapping:
#   (1) Every cell runs --verify-only first (wire bytes, else deterministic
#       wire bytes, else semantic equality). A cell is timed only if its
#       verification passed; the verify method is recorded per cell.
#       Harness exit code 3 means structurally unsupported (hyperpb encode,
#       a message without vt fast paths): recorded as not_run without
#       failing the run; any other nonzero verify exit fails the run.
#   (2) Peers that fail to build are reported as not_run with a recorded
#       reason, never omitted. All build steps are scripted below and pinned
#       (protobuf PIN/SHA from vendor/google, abseil pin from protobuf's own
#       cmake/dependencies.cmake via FetchContent, Go module pins in
#       bench/xlang/go/go.mod with go.sum hashes). Go peers degrade per
#       corpus: one corpus's codegen or compile failure becomes that
#       corpus's not_run cells (reason in target/xlang-codec/work/
#       go-build.tsv) while the rest of the peer still measures.
#   (3) Test tools only: the harnesses are standalone C++/C/Go binaries built
#       under target/; no Cargo manifest references them and they never enter
#       a shipping dependency graph.
#
# Usage:
#   scripts/xlang-codec.sh [--out report.json] [options]
#
# Options:
#   --peers LIST     subset of cpp,cpp_arena,upb,go,go_vt,go_hyperpb (default: all)
#   --ops LIST       subset of encode,decode (default: all)
#   --tiers LIST     subset of tiny,typical,large,huge (default: all)
#   --corpus ID      only this corpus (repeatable)
#   --cells SUBSTR   only cells whose id contains SUBSTR (repeatable)
#   --iters N        base iteration count, scaled per payload size (default 2000)
#   --repeats N      timed repeats per cell; median is reported (default 3)
#   --jobs N         build parallelism (default: ncpu)
#   --out FILE       write the devloop/1 report to FILE (default: stdout)
#   --list-cells     print the selected cell ids and exit (no build needed)
#   --build-only     build peer libraries and harnesses, then exit
#   --force-rebuild  drop cached peer builds, descriptors and regen payloads
#
# Iteration scaling: iters(cell) = clamp(ITERS * 512 / payload_bytes, 1, 50000),
# so every cell processes a comparable byte volume however large the payload.
#
# Exit status: 0 when the report is complete; 1 when any attempted cell failed
# verification or when zero cells could be measured (a fully not_run report
# fails loudly instead of passing vacuously); 2 on usage errors or when the
# filters select no cells.
#
# Results are dev-loop grade on any host; on Linux with `perf`, retired
# instructions are collected with the devloop N/2N differential method and the
# report is claim-capable. Allocation/syscall metrics are not_run: the C/C++
# heaps and the Go heap are not instrumented (see bench/xlang/*/README.md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PEERS="cpp,cpp_arena,upb,go,go_vt,go_hyperpb"
OPS="encode,decode"
TIERS="tiny,typical,large,huge"
CORPUS_FILTERS=()
CELL_FILTERS=()
ITERS="2000"
REPEATS="3"
JOBS=""
OUT=""
LIST_CELLS=0
BUILD_ONLY=0
FORCE_REBUILD=0

while [ $# -gt 0 ]; do
  case "$1" in
    --peers) PEERS="$2"; shift 2;;
    --ops) OPS="$2"; shift 2;;
    --tiers) TIERS="$2"; shift 2;;
    --corpus) CORPUS_FILTERS+=("$2"); shift 2;;
    --cells) CELL_FILTERS+=("$2"); shift 2;;
    --iters) ITERS="$2"; shift 2;;
    --repeats) REPEATS="$2"; shift 2;;
    --jobs) JOBS="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    --list-cells) LIST_CELLS=1; shift;;
    --build-only) BUILD_ONLY=1; shift;;
    --force-rebuild) FORCE_REBUILD=1; shift;;
    -h|--help) sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//'; exit 0;;
    *) echo "xlang-codec: unknown arg: $1" >&2; exit 2;;
  esac
done

for p in ${PEERS//,/ }; do
  case "$p" in cpp|cpp_arena|upb|go|go_vt|go_hyperpb) ;; *) echo "xlang-codec: unknown peer: $p" >&2; exit 2;; esac
done
for o in ${OPS//,/ }; do
  case "$o" in encode|decode) ;; *) echo "xlang-codec: unknown op: $o" >&2; exit 2;; esac
done
for t in ${TIERS//,/ }; do
  case "$t" in tiny|typical|large|huge) ;; *) echo "xlang-codec: unknown tier: $t" >&2; exit 2;; esac
done

if [ -z "$JOBS" ]; then
  if command -v nproc >/dev/null 2>&1; then JOBS="$(nproc)"; else JOBS="$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"; fi
fi

PIN="$(cat vendor/google/PIN)"
SHA="$(cat vendor/google/SHA)"
PEER_BUILD="target/xlang-peer-build"
PROTOC_BUILD="target/pinned-protoc-build"
PROTOC="$PROTOC_BUILD/protoc"
XLANG_OUT="target/xlang-codec"
BIN_DIR="$XLANG_OUT/bin"
DESC_DIR="$XLANG_OUT/descs"
REGEN_DIR="$XLANG_OUT/payloads"
WORK="$XLANG_OUT/work"
LOG_DIR="$XLANG_OUT/logs"
MANIFEST="bench/corpora/manifest.json"

log() { echo "xlang-codec: $*" >&2; }

# ---------------------------------------------------------------------------
# Peer build status. A peer that cannot be built is recorded here and all of
# its cells become not_run with this reason (accept 2).
# ---------------------------------------------------------------------------
PEER_STATUS_cpp="pending"
PEER_REASON_cpp=""
PEER_STATUS_cpp_arena="pending"
PEER_REASON_cpp_arena=""
PEER_STATUS_upb="pending"
PEER_REASON_upb=""
PEER_STATUS_go="pending"
PEER_REASON_go=""
PEER_STATUS_go_vt="pending"
PEER_REASON_go_vt=""
PEER_STATUS_go_hyperpb="pending"
PEER_REASON_go_hyperpb=""

peer_fail() { # peer reason...
  local peer="$1"; shift
  local reason="$*"
  log "peer $peer unavailable: $reason"
  case "$peer" in
    cpp) PEER_STATUS_cpp="fail"; PEER_REASON_cpp="$reason";;
    cpp_arena) PEER_STATUS_cpp_arena="fail"; PEER_REASON_cpp_arena="$reason";;
    upb) PEER_STATUS_upb="fail"; PEER_REASON_upb="$reason";;
    go) PEER_STATUS_go="fail"; PEER_REASON_go="$reason";;
    go_vt) PEER_STATUS_go_vt="fail"; PEER_REASON_go_vt="$reason";;
    go_hyperpb) PEER_STATUS_go_hyperpb="fail"; PEER_REASON_go_hyperpb="$reason";;
  esac
}

peer_ok() {
  case "$1" in
    cpp) PEER_STATUS_cpp="ok";;
    cpp_arena) PEER_STATUS_cpp_arena="ok";;
    upb) PEER_STATUS_upb="ok";;
    go) PEER_STATUS_go="ok";;
    go_vt) PEER_STATUS_go_vt="ok";;
    go_hyperpb) PEER_STATUS_go_hyperpb="ok";;
  esac
}

peer_status() {
  case "$1" in
    cpp) echo "$PEER_STATUS_cpp";;
    cpp_arena) echo "$PEER_STATUS_cpp_arena";;
    upb) echo "$PEER_STATUS_upb";;
    go) echo "$PEER_STATUS_go";;
    go_vt) echo "$PEER_STATUS_go_vt";;
    go_hyperpb) echo "$PEER_STATUS_go_hyperpb";;
  esac
}

peer_reason() {
  case "$1" in
    cpp) echo "$PEER_REASON_cpp";;
    cpp_arena) echo "$PEER_REASON_cpp_arena";;
    upb) echo "$PEER_REASON_upb";;
    go) echo "$PEER_REASON_go";;
    go_vt) echo "$PEER_REASON_go_vt";;
    go_hyperpb) echo "$PEER_REASON_go_hyperpb";;
  esac
}

want_peer() { case ",$PEERS," in *",$1,"*) return 0;; *) return 1;; esac; }

fail_go_peers() { # reason... (fails all wanted Go peers with one reason)
  local reason="$*"
  want_peer go && peer_fail go "$reason"
  want_peer go_vt && peer_fail go_vt "$reason"
  want_peer go_hyperpb && peer_fail go_hyperpb "$reason"
}

want_go_peer() { want_peer go || want_peer go_vt || want_peer go_hyperpb; }
want_cmake_peer() { want_peer cpp || want_peer cpp_arena || want_peer upb; }

# ---------------------------------------------------------------------------
# Phase 0: pinned sources + peer libraries + harnesses.
# ---------------------------------------------------------------------------
phase_build() {
  mkdir -p "$BIN_DIR" "$DESC_DIR" "$REGEN_DIR" "$WORK" "$LOG_DIR"
  if [ "$FORCE_REBUILD" = 1 ]; then
    log "--force-rebuild: dropping $PEER_BUILD $DESC_DIR $REGEN_DIR harness binaries, Go codegen"
    rm -rf "$PEER_BUILD" "$DESC_DIR" "$REGEN_DIR" "$BIN_DIR" \
      bench/xlang/go/gen "$XLANG_OUT/go-codegen.stamp"
    mkdir -p "$BIN_DIR" "$DESC_DIR" "$REGEN_DIR"
  fi

  # Pinned protobuf sources (established fetch pattern + SHA check).
  if ! ./scripts/fetch-protobuf.sh >&2; then
    local r="fetch-protobuf.sh failed; see log"
    want_peer cpp && peer_fail cpp "$r"
    want_peer cpp_arena && peer_fail cpp_arena "$r"
    want_peer upb && peer_fail upb "$r"
    fail_go_peers "$r"
    return 0
  fi
  local actual_sha
  actual_sha="$(git -C third_party/protobuf rev-parse HEAD)"
  if [ "$actual_sha" != "$SHA" ]; then
    local r="pinned source mismatch: want $SHA got $actual_sha"
    want_peer cpp && peer_fail cpp "$r"
    want_peer cpp_arena && peer_fail cpp_arena "$r"
    want_peer upb && peer_fail upb "$r"
    fail_go_peers "$r"
    return 0
  fi

  # Pinned protoc for descriptor generation (and Go codegen).
  if ! PBRS_PROTOC_JOBS="$JOBS" ./scripts/build-pinned-protoc.sh >&2; then
    local r="pinned protoc build failed"
    want_peer cpp && peer_fail cpp "$r"
    want_peer cpp_arena && peer_fail cpp_arena "$r"
    want_peer upb && peer_fail upb "$r"
    fail_go_peers "$r"
    return 0
  fi

  # Release peer libraries in a dedicated build dir (the shared pinned-protoc
  # dir has no CMAKE_BUILD_TYPE and may link a system abseil; benchmarks need
  # -O3 and the pinned FetchContent abseil from dependencies.cmake).
  # Skipped entirely when only Go peers are wanted (they need no cmake).
  local cmake_ok=1
  if want_cmake_peer; then
    local stamp="$PEER_BUILD/.xlang-pin"
    if [ ! -f "$stamp" ] || [ "$(cat "$stamp")" != "$PIN $SHA" ]; then
      log "configuring $PEER_BUILD ($PIN $SHA, Release, pinned abseil)"
      rm -rf "$PEER_BUILD"
      if ! cmake -S third_party/protobuf -B "$PEER_BUILD" \
          -Dprotobuf_BUILD_TESTS=OFF \
          -Dprotobuf_BUILD_CONFORMANCE=OFF \
          -Dprotobuf_INSTALL=OFF \
          -Dprotobuf_FORCE_FETCH_DEPENDENCIES=ON \
          -DCMAKE_BUILD_TYPE=Release >"$LOG_DIR/cmake-configure.log" 2>&1; then
        local r="peer cmake configure failed; see $LOG_DIR/cmake-configure.log"
        want_peer cpp && peer_fail cpp "$r"
        want_peer cpp_arena && peer_fail cpp_arena "$r"
        want_peer upb && peer_fail upb "$r"
        cmake_ok=0
      else
        printf '%s %s\n' "$PIN" "$SHA" >"$stamp"
      fi
    fi
  fi

  local have_libprotobuf=0 have_libupb=0
  if [ "$cmake_ok" = 1 ] && { want_peer cpp || want_peer cpp_arena; }; then
    if cmake --build "$PEER_BUILD" --parallel "$JOBS" --target libprotobuf \
        >"$LOG_DIR/libprotobuf.log" 2>&1; then
      have_libprotobuf=1
    else
      local r="libprotobuf build failed; see $LOG_DIR/libprotobuf.log"
      want_peer cpp && peer_fail cpp "$r"
      want_peer cpp_arena && peer_fail cpp_arena "$r"
    fi
  fi
  if [ "$cmake_ok" = 1 ] && want_peer upb; then
    if cmake --build "$PEER_BUILD" --parallel "$JOBS" \
        --target libupb utf8_range utf8_validity \
        >"$LOG_DIR/libupb.log" 2>&1; then
      have_libupb=1
    else
      peer_fail upb "libupb build failed; see $LOG_DIR/libupb.log"
    fi
  fi

  if [ "$have_libprotobuf" = 1 ]; then
    if make -C bench/xlang/cpp >"$LOG_DIR/harness-cpp.log" 2>&1; then
      want_peer cpp && peer_ok cpp
      want_peer cpp_arena && peer_ok cpp_arena
    else
      local r="harness-cpp build failed; see $LOG_DIR/harness-cpp.log"
      want_peer cpp && peer_fail cpp "$r"
      want_peer cpp_arena && peer_fail cpp_arena "$r"
    fi
  fi
  if [ "$have_libupb" = 1 ] && want_peer upb; then
    if make -C bench/xlang/upb >"$LOG_DIR/harness-upb.log" 2>&1; then
      peer_ok upb
    else
      peer_fail upb "harness-upb build failed; see $LOG_DIR/harness-upb.log"
    fi
  fi

  # Go peers (SB-07): independent of the cmake build above.
  if want_go_peer; then
    phase_build_go
  fi
}

# ---------------------------------------------------------------------------
# Go peer build (SB-07). The Makefile fetches the pinned modules, builds the
# pinned plugins, runs codegen, and compiles the per-corpus binaries,
# recording per-(peer,corpus) status in $WORK/go-build.tsv. A peer is ok if
# at least one of its binaries built; finer-grained failures degrade to
# not_run cells via go_build_status, never to a failed run.
# ---------------------------------------------------------------------------
phase_build_go() {
  if ! command -v go >/dev/null 2>&1; then
    fail_go_peers "go toolchain not found in PATH"
    return 0
  fi
  if ! make -C bench/xlang/go >"$LOG_DIR/go-make.log" 2>&1; then
    fail_go_peers "go peer build failed; see $LOG_DIR/go-make.log"
    return 0
  fi
  local tsv="$WORK/go-build.tsv"
  for peer in go go_vt go_hyperpb; do
    want_peer "$peer" || continue
    if [ -f "$tsv" ] && grep -q "^$peer"$'\t''.*'$'\t''ok$' "$tsv"; then
      peer_ok "$peer"
    else
      local reason="no binaries built (see $LOG_DIR/go-make.log)"
      if [ -f "$tsv" ]; then
        reason="$(awk -F'\t' -v p="$peer" '$1==p && $3!="ok" {print $2": "$3}' "$tsv" | head -3 | tr '\n' ';' | sed 's/;  */; /g')"
        [ -n "$reason" ] || reason="no $peer rows in $tsv"
      fi
      peer_fail "$peer" "$reason"
    fi
  done
}

# Per-(peer,corpus) Go build status: prints "ok" or the recorded reason.
go_build_status() { # peer corpus
  local tsv="$WORK/go-build.tsv"
  [ -f "$tsv" ] || { echo "go-build.tsv missing"; return 0; }
  local st
  st="$(awk -F'\t' -v p="$1" -v c="$2" '$1==p && $2==c {print $3}' "$tsv" | head -1)"
  [ -n "$st" ] && echo "$st" || echo "no build row for $1/$2"
}

# ---------------------------------------------------------------------------
# Phase 1: cell matrix from the SB-05 manifest. TSV columns:
# corpus, short, tier, message, payload_rel, sha256, size
# ---------------------------------------------------------------------------
phase_matrix() {
  : >"$WORK/matrix.tsv"
  CORPORA_CSV="$(IFS=,; echo "${CORPUS_FILTERS[*]:-}")" \
  TIERS="$TIERS" MANIFEST="$MANIFEST" OUT="$WORK/matrix.tsv" \
  python3 - <<'EOF'
import json, os
manifest = json.load(open(os.environ["MANIFEST"]))
corpora = [c for c in os.environ["CORPORA_CSV"].split(",") if c] or None
tiers = set(os.environ["TIERS"].split(","))
rows = []
for corpus, entry in sorted(manifest["corpora"].items()):
    if corpora and corpus not in corpora:
        continue
    for key, p in sorted(entry["payloads"].items()):
        short, tier = key.split("/", 1)
        if tier not in tiers:
            continue
        rows.append((corpus, short, tier, p["message"],
                     f"payloads/{corpus}/{short}/{tier}.bin",
                     p["sha256"], str(p["size"])))
with open(os.environ["OUT"], "w") as f:
    for r in rows:
        f.write("\t".join(r) + "\n")
EOF
  if [ ! -s "$WORK/matrix.tsv" ]; then
    log "no payloads selected (check --corpus/--tiers)"
    exit 2
  fi
}

# ---------------------------------------------------------------------------
# Phase 2: descriptor sets, one per corpus from its manifest roots, built with
# the pinned protoc. Corpora vendor their import closure (including WKTs), so
# the corpus root is tried first; the pinned protobuf/src is appended only as
# a fallback, and the include mode is recorded per corpus for provenance.
# ---------------------------------------------------------------------------
phase_descs() {
  : >"$WORK/desc-mode.tsv"
  while IFS=$'\t' read -r corpus _short _tier _msg _rel _sha _size; do
    echo "$corpus"
  done <"$WORK/matrix.tsv" | sort -u | while read -r corpus; do
    desc="$DESC_DIR/$corpus.desc"
    stamp="$DESC_DIR/$corpus.stamp"
    if [ -f "$desc" ] && [ -f "$stamp" ] && \
        [ "$(cut -d' ' -f1-2 "$stamp")" = "$PIN $SHA" ]; then
      echo -e "$corpus\t$(cut -d' ' -f3 "$stamp")" >>"$WORK/desc-mode.tsv"
      continue
    fi
    root="bench/corpora/$corpus/protos"
    roots="$(MANIFEST="$MANIFEST" CORPUS="$corpus" python3 -c "
import json, os
m = json.load(open(os.environ['MANIFEST']))
print(' '.join(m['corpora'][os.environ['CORPUS']]['roots']))")"
    mode="vendored"
    # shellcheck disable=SC2086
    if ! "$PROTOC" -I "$root" --descriptor_set_out="$desc" \
        --include_imports $roots >"$LOG_DIR/protoc-$corpus.log" 2>&1; then
      mode="vendored+pinned-wkt"
      # shellcheck disable=SC2086
      if ! "$PROTOC" -I "$root" -I third_party/protobuf/src \
          --descriptor_set_out="$desc" --include_imports $roots \
          >"$LOG_DIR/protoc-$corpus.log" 2>&1; then
        log "protoc descriptor build failed for $corpus; see $LOG_DIR/protoc-$corpus.log"
        rm -f "$desc"
        echo -e "$corpus\tFAILED" >>"$WORK/desc-mode.tsv"
        continue
      fi
    fi
    printf '%s %s %s\n' "$PIN" "$SHA" "$mode" >"$stamp"
    echo -e "$corpus\t$mode" >>"$WORK/desc-mode.tsv"
  done
}

desc_mode() { # corpus -> mode line or FAILED
  awk -F'\t' -v c="$1" '$1==c{print $2}' "$WORK/desc-mode.tsv" | head -1
}

# ---------------------------------------------------------------------------
# Phase 3: payload files. tiny/typical ship in the repo; large/huge are
# regenerated on demand into target/ (never into bench/corpora, which this
# card must not write) by importing SB-05's generate.py as a module, then
# hash-verified against the manifest. Anything missing becomes not_run.
# Resolved paths land in matrix.tsv column 8 (absolute path or MISSING:<why>).
# ---------------------------------------------------------------------------
phase_payloads() {
  : >"$WORK/resolved.tsv"
  # Group missing large/huge payloads per corpus for one regen call each.
  : >"$WORK/regen-need.tsv"
  while IFS=$'\t' read -r corpus short tier message rel sha size; do
    local checked_in="bench/corpora/$rel"
    if [ -f "$checked_in" ]; then
      local got
      got="$(shasum -a 256 "$checked_in" | awk '{print $1}')"
      if [ "$got" = "$sha" ]; then
        echo -e "$corpus\t$short\t$tier\t$message\t$rel\t$sha\t$size\t$ROOT/$checked_in" >>"$WORK/resolved.tsv"
      else
        echo -e "$corpus\t$short\t$tier\t$message\t$rel\t$sha\t$size\tMISSING:checked-in payload hash mismatch (want $sha got $got)" >>"$WORK/resolved.tsv"
      fi
    else
      local regen="$REGEN_DIR/$corpus/$short/$tier.bin"
      if [ -f "$regen" ]; then
        local got
        got="$(shasum -a 256 "$regen" | awk '{print $1}')"
        if [ "$got" = "$sha" ]; then
          echo -e "$corpus\t$short\t$tier\t$message\t$rel\t$sha\t$size\t$regen" >>"$WORK/resolved.tsv"
          continue
        fi
        rm -f "$regen"
      fi
      echo -e "$corpus\t$short\t$tier" >>"$WORK/regen-need.tsv"
      echo -e "$corpus\t$short\t$tier\t$message\t$rel\t$sha\t$size\tREGEN" >>"$WORK/resolved.tsv"
    fi
  done <"$WORK/matrix.tsv"

  if [ -s "$WORK/regen-need.tsv" ]; then
    cut -f1 "$WORK/regen-need.tsv" | sort -u | while read -r corpus; do
      local specs
      specs="$(awk -F'\t' -v c="$corpus" '$1==c{print $2"/"$3}' "$WORK/regen-need.tsv" | sort -u)"
      log "regenerating $corpus large/huge payloads: $(echo "$specs" | tr '\n' ' ')"
      # shellcheck disable=SC2086
      if ! PROTOC="$ROOT/$PROTOC" MANIFEST="$ROOT/$MANIFEST" REGEN_DIR="$ROOT/$REGEN_DIR" \
          PYTHONPATH="$ROOT/bench/corpora" \
          python3 - "$corpus" $specs >"$LOG_DIR/regen-$corpus.log" 2>&1 <<'EOF'
import json, os, sys
sys.path.insert(0, os.environ["PYTHONPATH"])
import generate
corpus, specs = sys.argv[1], sys.argv[2:]
manifest = json.load(open(os.environ["MANIFEST"]))
entry = manifest["corpora"][corpus]
pool = generate.build_pool(corpus, entry["roots"])
any_msg = generate.ANY_CONTENT.get(corpus, "google.protobuf.Struct")
for spec in specs:
    short, tier = spec.split("/", 1)
    full = dict(generate.MESSAGES[corpus])[short]
    seed = generate.seed_for(corpus, short, tier)
    msg = generate.build_payload(pool, full, any_msg, tier, seed)
    blob = msg.SerializeToString(deterministic=True)
    want = entry["payloads"][f"{short}/{tier}"]
    assert len(blob) == want["size"], (spec, len(blob), want["size"])
    import hashlib
    digest = hashlib.sha256(blob).hexdigest()
    assert digest == want["sha256"], (spec, digest, want["sha256"])
    dest = os.path.join(os.environ["REGEN_DIR"], corpus, short, f"{tier}.bin")
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    open(dest, "wb").write(blob)
    print(f"regenerated {spec}: {len(blob)} bytes")
EOF
      then
        log "payload regen failed for $corpus; see $LOG_DIR/regen-$corpus.log"
      fi
    done
    # Re-resolve REGEN rows: success if the file now exists with the right hash.
    : >"$WORK/resolved2.tsv"
    while IFS=$'\t' read -r corpus short tier message rel sha size path; do
      if [ "$path" = "REGEN" ]; then
        local regen="$REGEN_DIR/$corpus/$short/$tier.bin"
        if [ -f "$regen" ] && [ "$(shasum -a 256 "$regen" | awk '{print $1}')" = "$sha" ]; then
          path="$ROOT/$regen"
        else
          path="MISSING:on-demand regen unavailable (see $LOG_DIR/regen-$corpus.log); generate with: python3 bench/corpora/generate.py $corpus"
        fi
      fi
      echo -e "$corpus\t$short\t$tier\t$message\t$rel\t$sha\t$size\t$path" >>"$WORK/resolved2.tsv"
    done <"$WORK/resolved.tsv"
    mv "$WORK/resolved2.tsv" "$WORK/resolved.tsv"
  fi
}

# ---------------------------------------------------------------------------
# Phase 4: verify-then-time every cell. Verification ALWAYS precedes timing
# (accept 1): a cell whose --verify-only run fails is never timed, is
# recorded as not_run with the harness stderr, and fails the whole run.
# ---------------------------------------------------------------------------
VERIFY_FAILED=0

harness_for() { # peer corpus -> "binary mode"
  case "$1" in
    cpp) echo "$BIN_DIR/harness-cpp heap";;
    cpp_arena) echo "$BIN_DIR/harness-cpp arena";;
    upb) echo "$BIN_DIR/harness-upb arena";;
    go) echo "$BIN_DIR/harness-go-$2 generated";;
    go_vt) echo "$BIN_DIR/harness-go-vt-$2 vt";;
    go_hyperpb) echo "$BIN_DIR/harness-hyperpb dynamic";;
  esac
}

# Peers whose harness reads the --desc FileDescriptorSet at runtime. The
# generated-code Go peers link their types in and ignore --desc, so a failed
# descriptor build must not block their cells.
peer_uses_desc() {
  case "$1" in go|go_vt) return 1;; *) return 0;; esac
}

cell_iters() { # size -> iters
  python3 -c "print(min(50000, max(1, int($ITERS * 512 / max(1, $1)))))"
}

have_perf() { command -v perf >/dev/null 2>&1; }

run_perf_count() { # outvar iters binary desc message payload op mode warmup
  # Differential instruction counting needs identical preparation, so warmup
  # is pinned explicitly (the harness would otherwise derive it from iters).
  local _o="$1" it="$2" bin="$3" desc="$4" msg="$5" pay="$6" op="$7" mode="$8" wu="$9"
  local stat="$WORK/perf.csv"
  if ! perf stat -x, -e instructions:u -o "$stat" -- "$bin" \
      --desc "$desc" --message "$msg" --payload "$pay" --op "$op" \
      --mode "$mode" --iters "$it" --warmup "$wu" >"$WORK/perf.out" 2>"$WORK/perf.err"; then
    return 1
  fi
  local count
  count="$(awk -F, '$3 ~ /instructions/ {gsub(/[^0-9]/, "", $1); print $1}' "$stat" | head -1)"
  [ -n "$count" ] || return 1
  printf -v "$_o" '%s' "$count"
}

phase_run() {
  rm -rf "$WORK/results"
  mkdir -p "$WORK/results"
  local use_perf=0
  if have_perf; then use_perf=1; fi
  while IFS=$'\t' read -r corpus short tier message _rel sha size path; do
    for peer in ${PEERS//,/ }; do
      for op in ${OPS//,/ }; do
        local id="xlang.$peer.$corpus.$short.$tier.$op"
        local keep=1
        if [ "${#CELL_FILTERS[@]}" -gt 0 ]; then
          keep=0
          for f in "${CELL_FILTERS[@]}"; do
            case "$id" in *"$f"*) keep=1;; esac
          done
        fi
        [ "$keep" = 1 ] || continue
        run_cell "$id" "$peer" "$op" "$corpus" "$short" "$tier" "$message" \
          "$sha" "$size" "$path" "$use_perf"
      done
    done
  done <"$WORK/resolved.tsv"
  if [ -z "$(ls -A "$WORK/results")" ]; then
    log "no cells selected (check --peers/--ops/--cells)"
    exit 2
  fi
}

record_not_run() { # file id reason
  NR_OUT="$1" NR_ID="$2" NR_REASON="$3" python3 - <<'EOF'
import json, os
json.dump({"id": os.environ["NR_ID"], "not_run": os.environ["NR_REASON"]},
          open(os.environ["NR_OUT"], "w"))
EOF
}

run_cell() { # id peer op corpus short tier message sha size path use_perf
  local id="$1" peer="$2" op="$3" corpus="$4" short="$5" tier="$6" message="$7"
  local sha="$8" size="$9" path="${10}" use_perf="${11}"
  local out="$WORK/results/$id.json"

  if [ "$(peer_status "$peer")" != "ok" ]; then
    record_not_run "$out" "$id" "peer build failed: $(peer_reason "$peer")"
    return 0
  fi
  # hyperpb is decode-only (dynamic-parse category A12); its encode cells
  # exist in the matrix but are never attempted.
  if [ "$peer" = "go_hyperpb" ] && [ "$op" = "encode" ]; then
    record_not_run "$out" "$id" \
      "hyperpb is decode-only; compared only in A12 (dynamic/reflection decode)"
    return 0
  fi
  # Go peers degrade per corpus (codegen/compile status in go-build.tsv).
  case "$peer" in
    go|go_vt)
      local go_st
      go_st="$(go_build_status "$peer" "$corpus")"
      if [ "$go_st" != "ok" ]; then
        record_not_run "$out" "$id" "go peer build failed for corpus $corpus: $go_st"
        return 0
      fi
      ;;
  esac
  case "$path" in
    MISSING:*)
      record_not_run "$out" "$id" "${path#MISSING:}"
      return 0
      ;;
  esac
  local mode
  mode="$(desc_mode "$corpus")"
  if peer_uses_desc "$peer"; then
    if [ "$mode" = "FAILED" ] || [ -z "$mode" ]; then
      record_not_run "$out" "$id" \
        "descriptor build failed for corpus $corpus; see $LOG_DIR/protoc-$corpus.log"
      return 0
    fi
  else
    # Generated-code peers link their types in and never read the set.
    mode="compiled-in"
  fi

  local bin hmode
  read -r bin hmode <<<"$(harness_for "$peer" "$corpus")"
  local desc="$DESC_DIR/$corpus.desc"
  local iters
  iters="$(cell_iters "$size")"

  # (1) Verify first. Exit code 2 from the harness means the codec did NOT
  # round-trip this payload: record, flag the run, never time. Exit code 3
  # means structurally unsupported (a message without vt fast paths, a
  # schema hyperpb cannot compile): record as not_run without failing.
  local vout verr vcode=0
  vout="$("$bin" --desc "$desc" --message "$message" --payload "$path" \
    --op "$op" --mode "$hmode" --verify-only 2>"$WORK/verify.err")" || vcode=$?
  verr="$(head -c 400 "$WORK/verify.err" | tr '\n' ' ')"
  if [ "$vcode" -eq 3 ]; then
    log "UNSUPPORTED: $id: ${verr:-exit 3}"
    record_not_run "$out" "$id" "unsupported: $(echo "$verr" | head -c 300)"
    return 0
  fi
  if [ "$vcode" -ne 0 ]; then
    VERIFY_FAILED=1
    log "VERIFY FAILED: $id: ${verr:-exit $vcode}"
    record_not_run "$out" "$id" "verification failed: $(echo "$verr" | head -c 300)"
    return 0
  fi
  local verify
  verify="$(echo "$vout" | python3 -c "import json,sys; print(json.load(sys.stdin)['verify'])")"

  # (2) Timed repeats.
  local walls=()
  local r w wall
  for r in $(seq 1 "$REPEATS"); do
    w="$("$bin" --desc "$desc" --message "$message" --payload "$path" \
      --op "$op" --mode "$hmode" --iters "$iters" 2>"$WORK/run.err")" || {
      VERIFY_FAILED=1
      log "RUN FAILED: $id repeat $r: $(head -c 300 "$WORK/run.err" | tr '\n' ' ')"
      record_not_run "$out" "$id" "timed run failed after passing verify (repeat $r)"
      return 0
    }
    wall="$(echo "$w" | python3 -c "import json,sys; print(json.load(sys.stdin)['wall_ns'])")"
    walls+=("$wall")
  done

  # (3) Instructions via perf (Linux), N/2N differential like devloop.
  local instr_json="null" instr_method="not_run_no_perf"
  if [ "$use_perf" = 1 ]; then
    local wu c1 c2
    wu=$(( iters >= 10 ? iters / 10 : 0 ))
    if run_perf_count c1 "$iters" "$bin" "$desc" "$message" "$path" "$op" "$hmode" "$wu" \
      && run_perf_count c2 "$(( iters * 2 ))" "$bin" "$desc" "$message" "$path" "$op" "$hmode" "$wu"; then
      instr_json="$(python3 -c "print(($c2 - $c1) / $iters)")"
      instr_method="perf_diff_n_2n"
    else
      instr_method="not_run_perf_failed"
    fi
  fi

  local walls_csv
  walls_csv="$(IFS=,; echo "${walls[*]}")"
  ID="$id" PEER="$peer" OP="$op" CORPUS="$corpus" SHORT="$short" TIER="$tier" \
  MESSAGE="$message" SHA="$sha" SIZE="$size" ITERS="$iters" REPEATS="$REPEATS" \
  VERIFY="$verify" WALLS="$walls_csv" INSTR="$instr_json" INSTR_METHOD="$instr_method" \
  DESC_MODE="$mode" OUT="$out" python3 - <<'EOF'
import json, os
doc = {
    "id": os.environ["ID"],
    "peer": os.environ["PEER"],
    "op": os.environ["OP"],
    "corpus": os.environ["CORPUS"],
    "short": os.environ["SHORT"],
    "tier": os.environ["TIER"],
    "message": os.environ["MESSAGE"],
    "sha256": os.environ["SHA"],
    "size": int(os.environ["SIZE"]),
    "iters": int(os.environ["ITERS"]),
    "repeats": int(os.environ["REPEATS"]),
    "verify": os.environ["VERIFY"],
    "walls": [int(x) for x in os.environ["WALLS"].split(",")],
    "instr_per_op": None if os.environ["INSTR"] == "null" else float(os.environ["INSTR"]),
    "instr_method": os.environ["INSTR_METHOD"],
    "desc_mode": os.environ["DESC_MODE"],
}
json.dump(doc, open(os.environ["OUT"], "w"))
EOF
}

# ---------------------------------------------------------------------------
# Phase 5: assemble the devloop/1 report. Unknown extra fields ("xlang" per
# cell, "xlang_provenance" top-level) are ignored by devloop's serde readers,
# so `devloop compare` keeps working on this output.
# ---------------------------------------------------------------------------
phase_emit() {
  local absl_pin="unknown"
  if [ -f third_party/protobuf/cmake/dependencies.cmake ]; then
    absl_pin="$(grep -m1 'set(abseil-cpp-version' third_party/protobuf/cmake/dependencies.cmake | sed 's/.*"\(.*\)".*/\1/')"
  fi
  local cxx_ver cc_ver protoc_ver cpu os arch commit
  cxx_ver="$({ c++ --version 2>/dev/null || clang++ --version 2>/dev/null; } | head -1)"
  cc_ver="$({ cc --version 2>/dev/null || clang --version 2>/dev/null; } | head -1)"
  protoc_ver="$("$PROTOC" --version 2>/dev/null || echo missing)"
  os="$(uname -s -r)"
  arch="$(uname -m)"
  if [ "$(uname)" = "Darwin" ]; then cpu="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"; else cpu="$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2 | xargs || echo unknown)"; fi
  commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  if [ -n "$(git status --porcelain 2>/dev/null | head -1)" ]; then commit="$commit-dirty"; fi
  local have_perf_s="false" have_strace="false" have_valgrind="false"
  have_perf && have_perf_s="true"
  command -v strace >/dev/null 2>&1 && have_strace="true"
  command -v valgrind >/dev/null 2>&1 && have_valgrind="true"
  go_ver="$(go version 2>/dev/null || echo missing)"
  go_pb="$(awk '$1=="google.golang.org/protobuf"{print $2}' bench/xlang/go/go.mod 2>/dev/null || true)"; [ -n "$go_pb" ] || go_pb="unknown"
  go_vt="$(awk '$1=="github.com/planetscale/vtprotobuf"{print $2}' bench/xlang/go/go.mod 2>/dev/null || true)"; [ -n "$go_vt" ] || go_vt="unknown"
  go_hyperpb="$(awk '$1=="buf.build/go/hyperpb"{print $2}' bench/xlang/go/go.mod 2>/dev/null || true)"; [ -n "$go_hyperpb" ] || go_hyperpb="unknown"

  PIN="$PIN" SHA="$SHA" ABSL_PIN="$absl_pin" CXX_VER="$cxx_ver" CC_VER="$cc_ver" \
  PROTOC_VER="$protoc_ver" OS="$os" ARCH="$arch" CPU="$cpu" COMMIT="$commit" \
  HAVE_PERF="$have_perf_s" HAVE_STRACE="$have_strace" HAVE_VALGRIND="$have_valgrind" \
  GO_VER="$go_ver" GO_PB_PIN="$go_pb" GO_VT_PIN="$go_vt" GO_HYPERPB_PIN="$go_hyperpb" \
  RESULTS_DIR="$WORK/results" \
  PEER_CPP="$(peer_status cpp)" PEER_CPP_R="$(peer_reason cpp)" \
  PEER_ARENA="$(peer_status cpp_arena)" PEER_ARENA_R="$(peer_reason cpp_arena)" \
  PEER_UPB="$(peer_status upb)" PEER_UPB_R="$(peer_reason upb)" \
  PEER_GO="$(peer_status go)" PEER_GO_R="$(peer_reason go)" \
  PEER_GOVT="$(peer_status go_vt)" PEER_GOVT_R="$(peer_reason go_vt)" \
  PEER_HYPERPB="$(peer_status go_hyperpb)" PEER_HYPERPB_R="$(peer_reason go_hyperpb)" \
  OUT="${OUT:-}" python3 - <<'EOF'
import glob, json, os, statistics

def measured(value, unit):
    return {"status": "measured", "data": {"value": value, "unit": unit}}

def not_run(reason):
    return {"status": "not_run", "data": {"reason": reason}}

def kind_of(op):
    return "xlang_encode" if op == "encode" else "xlang_decode"

cells = []
measured_cells = 0
for path in sorted(glob.glob(os.path.join(os.environ["RESULTS_DIR"], "*.json"))):
    r = json.load(open(path))
    if "not_run" in r:
        cells.append({
            "id": r["id"], "kind": "xlang_" + ("encode" if r["id"].endswith(".encode") else "decode"),
            "codec": r["id"].split(".")[1], "iters": 0, "repeats": 0,
            "instructions": not_run(r["not_run"]),
            "instruction_method": "not_run_cell_not_run",
            "allocs": not_run(r["not_run"]),
            "alloc_bytes": not_run(r["not_run"]),
            "syscalls": not_run(r["not_run"]),
            "locks": not_run(r["not_run"]),
            "wall_ns": not_run(r["not_run"]),
            "wall_cv": None, "copy_counts": None,
            "xlang": {"verify": "not_run", "reason": r["not_run"]},
        })
        continue
    # Accept-1 gate: a timed cell without a passing verify method is a bug.
    assert r["verify"] in ("wire_equal", "wire_equal_deterministic", "semantic_equal"), r
    per_op = [w / r["iters"] for w in r["walls"]]
    med = statistics.median(per_op)
    mean = sum(per_op) / len(per_op)
    cv = (statistics.pstdev(per_op) / mean) if mean > 0 else 0.0
    instr = (measured(r["instr_per_op"], "instructions")
             if r["instr_per_op"] is not None else not_run(r["instr_method"]))
    is_go = r["peer"] in ("go", "go_vt", "go_hyperpb")
    alloc_reason = "Go heap not instrumented" if is_go else "C/C++ heap not instrumented"
    lock_reason = ("single-goroutine harness (runtime threads not counted)"
                   if is_go else "single-threaded harness")
    cells.append({
        "id": r["id"], "kind": kind_of(r["op"]), "codec": r["peer"],
        "iters": r["iters"], "repeats": r["repeats"],
        "instructions": instr,
        "instruction_method": r["instr_method"],
        "allocs": not_run(alloc_reason),
        "alloc_bytes": not_run(alloc_reason),
        "syscalls": not_run("strace wrap not implemented for xlang peers"),
        "locks": not_run(lock_reason),
        "wall_ns": measured(med, "ns"),
        "wall_cv": cv, "copy_counts": None,
        "xlang": {
            "peer": r["peer"], "corpus": r["corpus"], "message": r["message"],
            "tier": r["tier"], "op": r["op"], "verify": r["verify"],
            "payload_sha256": r["sha256"], "payload_bytes": r["size"],
            "desc_include": r["desc_mode"],
        },
    })
    measured_cells += 1

report = {
    "schema": "devloop/1",
    "host": {
        "os": os.environ["OS"], "arch": os.environ["ARCH"], "cpu": os.environ["CPU"],
        "rustc": "n/a (xlang c++/c/go)",
        "perf": os.environ["HAVE_PERF"] == "true",
        "strace": os.environ["HAVE_STRACE"] == "true",
        "valgrind": os.environ["HAVE_VALGRIND"] == "true",
    },
    "devloop_commit": os.environ["COMMIT"],
    "cells": cells,
    "xlang_provenance": {
        "protobuf_pin": os.environ["PIN"], "protobuf_sha": os.environ["SHA"],
        "absl_pin": os.environ["ABSL_PIN"], "protoc": os.environ["PROTOC_VER"],
        "cxx": os.environ["CXX_VER"], "cc": os.environ["CC_VER"],
        "go": {
            "version": os.environ["GO_VER"],
            "protobuf": os.environ["GO_PB_PIN"],
            "vtprotobuf": os.environ["GO_VT_PIN"],
            "hyperpb": os.environ["GO_HYPERPB_PIN"],
        },
        "peers": {
            "cpp": {"status": os.environ["PEER_CPP"], "reason": os.environ["PEER_CPP_R"]},
            "cpp_arena": {"status": os.environ["PEER_ARENA"], "reason": os.environ["PEER_ARENA_R"]},
            "upb": {"status": os.environ["PEER_UPB"], "reason": os.environ["PEER_UPB_R"]},
            "go": {"status": os.environ["PEER_GO"], "reason": os.environ["PEER_GO_R"]},
            "go_vt": {"status": os.environ["PEER_GOVT"], "reason": os.environ["PEER_GOVT_R"]},
            "go_hyperpb": {"status": os.environ["PEER_HYPERPB"], "reason": os.environ["PEER_HYPERPB_R"]},
        },
    },
}
text = json.dumps(report, indent=1, sort_keys=True) + "\n"
out = os.environ["OUT"]
if out:
    open(out, "w").write(text)
else:
    print(text, end="")
# Summary table to stderr (per-peer status for accept 2).
import sys
def status_line(peer):
    mine = [c for c in cells if c["id"].split(".")[1] == peer]
    m = sum(1 for c in mine if c["wall_ns"]["status"] == "measured")
    print(f"xlang-codec: peer {peer}: {m}/{len(mine)} cells measured", file=sys.stderr)
for p in ("cpp", "cpp_arena", "upb", "go", "go_vt", "go_hyperpb"):
    if any(c["id"].split(".")[1] == p for c in cells):
        status_line(p)
print(f"xlang-codec: total {measured_cells}/{len(cells)} cells measured", file=sys.stderr)
print(f"xlang-codec: MEASURED={measured_cells}", file=sys.stderr)
EOF
}

# ---------------------------------------------------------------------------
# Main.
# ---------------------------------------------------------------------------
if [ "$LIST_CELLS" = 1 ]; then
  mkdir -p "$WORK"
  phase_matrix
  while IFS=$'\t' read -r corpus short tier _m _r _s _z; do
    for peer in ${PEERS//,/ }; do
      for op in ${OPS//,/ }; do
        id="xlang.$peer.$corpus.$short.$tier.$op"
        keep=1
        if [ "${#CELL_FILTERS[@]}" -gt 0 ]; then
          keep=0
          for f in "${CELL_FILTERS[@]}"; do case "$id" in *"$f"*) keep=1;; esac; done
        fi
        [ "$keep" = 1 ] && echo "$id"
      done
    done
  done <"$WORK/matrix.tsv" | sort
  exit 0
fi

phase_build
if [ "$BUILD_ONLY" = 1 ]; then
  fail=0
  for peer in ${PEERS//,/ }; do
    if [ "$(peer_status "$peer")" = "ok" ]; then
      log "peer $peer: built"
    else
      log "peer $peer: BUILD FAILED: $(peer_reason "$peer")"
      fail=1
    fi
  done
  exit "$fail"
fi

phase_matrix
phase_descs
phase_payloads
phase_run
REPORT_TMP="$WORK/report.json"
OUT_SAVED="$OUT"
OUT="$REPORT_TMP"
summary="$(phase_emit 2>&1)" || { echo "$summary" >&2; exit 1; }
OUT="$OUT_SAVED"
echo "$summary" >&2
if [ -n "$OUT" ]; then cp "$REPORT_TMP" "$OUT"; else cat "$REPORT_TMP"; fi
measured="$(echo "$summary" | sed -n 's/xlang-codec: MEASURED=//p')"
if [ "$VERIFY_FAILED" = 1 ]; then
  log "failing: one or more cells failed verification"
  exit 1
fi
if [ "${measured:-0}" = "0" ]; then
  log "failing: zero cells measured (report is fully not_run)"
  exit 1
fi
exit 0

