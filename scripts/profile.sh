#!/usr/bin/env bash
# SB-14: profile one devloop cell (codec or RPC) with one command.
#
#   ./scripts/profile.sh --cell rpc.pbrs.unary [--iters N] [--warmup N]
#       [--out DIR] [--tool auto|samply|perf|sample|dtrace] [--skip-build]
#
# Builds release devloop, samples `run-cell` with the best available tool
# (samply > perf on Linux > sample on macOS > dtrace with root), and writes:
#   cell.json    run-cell JSON: wall time + exact allocs/bytes (counting allocator)
#   top.txt      top-40 sampled symbols with counts (always available)
#   folded.txt   folded stacks for flamegraph.pl, when the tool emits stacks
#   profile.*    raw capture (samply.json / perf.data+script / sample.txt / dtrace.out)
#   meta.json    cell, tool + version, base SHA, timestamps
# Missing tools yield a clear error naming the install; a partial capture is
# labeled, never presented as complete.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CELL=""
ITERS=""
WARMUP=1000
OUT=""
TOOL="auto"
SKIP_BUILD=0

usage() { sed -n '2,20p' "$0"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --cell) CELL="$2"; shift 2 ;;
    --cell=*) CELL="${1#--cell=}"; shift ;;
    --iters) ITERS="$2"; shift 2 ;;
    --iters=*) ITERS="${1#--iters=}"; shift ;;
    --warmup) WARMUP="$2"; shift 2 ;;
    --warmup=*) WARMUP="${1#--warmup=}"; shift ;;
    --out) OUT="$2"; shift 2 ;;
    --out=*) OUT="${1#--out=}"; shift ;;
    --tool) TOOL="$2"; shift 2 ;;
    --tool=*) TOOL="${1#--tool=}"; shift ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

[ -n "$CELL" ] || { echo "--cell <id> is required (see: devloop list)" >&2; exit 2; }
case "$TOOL" in auto|samply|perf|sample|dtrace) ;; *) echo "unknown --tool '$TOOL'" >&2; exit 2 ;; esac

DEVLOOP="$ROOT/bench/devloop/target/release/devloop"
if [ "$SKIP_BUILD" -eq 0 ]; then
  (cd "$ROOT/bench/devloop" && CARGO_BUILD_JOBS=2 cargo build --locked --release --offline 2>/dev/null \
    || CARGO_BUILD_JOBS=2 cargo build --locked --release)
fi
[ -x "$DEVLOOP" ] || { echo "devloop binary missing at $DEVLOOP" >&2; exit 1; }

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
[ -n "$OUT" ] || OUT="$ROOT/target/profile/$CELL-$STAMP"
mkdir -p "$OUT"

"$DEVLOOP" list >"$OUT/cells.txt" 2>"$OUT/list.err" || { echo "devloop list failed" >&2; exit 1; }
grep -q "^$CELL " "$OUT/cells.txt" || { echo "unknown cell '$CELL' (see: devloop list)" >&2; exit 2; }

# Default workload: scale a timing probe to ~10 s of sampling unless the
# caller pinned --iters.
if [ -z "$ITERS" ]; then
  PROBE_SECS="$(python3 -c "
import subprocess, sys, time
t = time.perf_counter()
r = subprocess.run(['$DEVLOOP', 'run-cell', '$CELL', '--iters', '2000', '--warmup', '100'],
                   stdout=open('$OUT/probe.out', 'w'), stderr=open('$OUT/probe.err', 'w'))
print(f'{time.perf_counter() - t:.3f}')
sys.exit(r.returncode)
")" || { echo "probe run failed; see $OUT/probe.err" >&2; exit 1; }
  # 2000 iters in PROBE_SECS -> iters for ~10 s, clamped to [10000, 20M].
  ITERS="$(python3 -c "
secs = max(float('$PROBE_SECS'), 0.001)
print(min(20000000, max(10000, int(2000 * 10.0 / secs))))")"
  echo "probe: 2000 iters in ${PROBE_SECS}s -> sampling with --iters $ITERS" >&2
fi

pick_tool() {
  if [ "$TOOL" != "auto" ]; then echo "$TOOL"; return; fi
  if command -v samply >/dev/null 2>&1; then echo samply; return; fi
  if [ "$(uname -s)" = "Linux" ] && command -v perf >/dev/null 2>&1; then echo perf; return; fi
  if [ "$(uname -s)" = "Darwin" ] && command -v sample >/dev/null 2>&1; then echo sample; return; fi
  if command -v dtrace >/dev/null 2>&1 && [ "$(id -u)" -eq 0 ]; then echo dtrace; return; fi
  echo none
}

SELECTED="$(pick_tool)"
if [ "$SELECTED" = "none" ]; then
  echo "no sampler available: install samply (cargo install samply), or perf (Linux), or run on macOS (sample)" >&2
  exit 1
fi

CELL_CMD=("$DEVLOOP" run-cell "$CELL" --iters "$ITERS" --warmup "$WARMUP")
echo "profiling $CELL with $SELECTED (iters=$ITERS warmup=$WARMUP)" >&2

case "$SELECTED" in
  samply)
    samply record --save-only -o "$OUT/profile.samply.json" -- "${CELL_CMD[@]}" >"$OUT/cell.out" 2>"$OUT/cell.err" || {
      echo "samply run failed; see $OUT/cell.err" >&2; exit 1;
    }
    ;;
  perf)
    perf record -F 997 -g -o "$OUT/perf.data" -- "${CELL_CMD[@]}" >"$OUT/cell.out" 2>"$OUT/cell.err" || {
      echo "perf run failed; see $OUT/cell.err" >&2; exit 1;
    }
    perf script -i "$OUT/perf.data" >"$OUT/perf.script" 2>"$OUT/perf.err" || {
      echo "perf script failed; see $OUT/perf.err" >&2; exit 1;
    }
    ;;
  sample)
    "${CELL_CMD[@]}" >"$OUT/cell.out" 2>"$OUT/cell.err" &
    CHILD=$!
    # Sample up to 30 s; sample(1) ends early if the child exits.
    sample "$CHILD" 30 -f "$OUT/sample.txt" >/dev/null 2>"$OUT/sample.err" || {
      echo "sample failed; see $OUT/sample.err" >&2; exit 1;
    }
    wait "$CHILD" || { echo "run-cell failed; see $OUT/cell.err" >&2; exit 1; }
    ;;
  dtrace)
   cat >"$OUT/profile.d" <<'DTRACE'
profile-997 /pid == $target/ { @[ustack()] = count(); }
DTRACE
    "${CELL_CMD[@]}" >"$OUT/cell.out" 2>"$OUT/cell.err" &
    CHILD=$!
    dtrace -s "$OUT/profile.d" -p "$CHILD" -o "$OUT/dtrace.out" >/dev/null 2>"$OUT/dtrace.err" || {
      echo "dtrace failed (needs root); see $OUT/dtrace.err" >&2; kill "$CHILD" 2>/dev/null; exit 1;
    }
    wait "$CHILD" || { echo "run-cell failed; see $OUT/cell.err" >&2; exit 1; }
    ;;
esac

# Extract cell.json (allocs + time) and top-40 symbols; always produced.
TOOL="$SELECTED" OUT="$OUT" CELL="$CELL" python3 - <<'PY'
import json, os, re, subprocess, sys
from collections import Counter
from pathlib import Path

out = Path(os.environ["OUT"])
tool = os.environ["TOOL"]
cell = os.environ["CELL"]

# --- cell.json from the __CHILD__ line (run-cell prints it on stderr) ---
cell_json = None
for name in ("cell.out", "cell.err"):
    for line in (out / name).read_text(errors="replace").splitlines():
        if line.startswith("__CHILD__ "):
            cell_json = json.loads(line[len("__CHILD__ "):])
            break
    if cell_json is not None:
        break
if cell_json is None:
    sys.exit(f"no __CHILD__ line in {out / 'cell.out'} or cell.err")
(out / "cell.json").write_text(json.dumps(cell_json, indent=2) + "\n")

# --- stacks -> folded + top symbols ---
folded = []
if tool == "perf":
    stack, in_stack = [], False
    for line in (out / "perf.script").read_text(errors="replace").splitlines():
        if re.match(r"^\S.*:\s+\d+ cycles", line) or (line and not line[0].isspace()):
            if stack:
                folded.append(";".join(reversed(stack)))
            stack, in_stack = [], True
        elif line.startswith("\t"):
            m = re.match(r"^\s+[0-9a-f]+\s+(\S+?)(?:\+0x[0-9a-f]+)?\s+\(", line)
            if m:
                stack.append(m.group(1))
    if stack:
        folded.append(";".join(reversed(stack)))
elif tool == "sample":
    text = (out / "sample.txt").read_text(errors="replace")
    # Prefer the self-time ranking; fall back to call-graph frame counts.
    self_ranked = []
    in_self = False
    for line in text.splitlines():
        if line.startswith("Sort by top of stack"):
            in_self = True
            continue
        if in_self:
            m = re.match(r"^\s+(\S+)\s+\(in \S+\)\s+(\d+)\s*$", line)
            if m:
                self_ranked.append((m.group(1), int(m.group(2))))
            elif line.strip() == "" or line.startswith("Binary Images:"):
                break
    if self_ranked:
        for sym, n in self_ranked:
            folded.extend([sym] * min(n, 100000))
    else:
        in_graph = False
        for line in text.splitlines():
            if line.strip() == "Call graph:":
                in_graph = True
                continue
            if not in_graph:
                continue
            m = re.match(r"^(\s*)(\d+)\s+(\S+)", line)
            if m:
                folded.extend([m.group(3)] * min(int(m.group(2)), 100000))
elif tool == "dtrace":
    frames, count = [], 0
    for line in (out / "dtrace.out").read_text(errors="replace").splitlines():
        m = re.match(r"^\s+(\S+`[^+]+)\+\S+\s*$", line)
        if m:
            frames.append(m.group(1))
            continue
        m = re.match(r"^\s+(\d+)\s*$", line)
        if m and frames:
            folded.extend([";".join(reversed(frames))] * min(int(m.group(1)), 100000))
            frames = []
# samply: top symbols come from its own UI; keep a pointer, not a fake parse.
if tool == "samply":
    (out / "top.txt").write_text(
        "samply capture: open profile.samply.json via `samply load` or the Firefox Profiler.\n"
        "Per-symbol counts live in the interactive view, not here.\n"
    )
else:
    if folded:
        (out / "folded.txt").write_text("\n".join(folded) + "\n")
    syms = Counter()
    for entry in folded:
        for sym in entry.split(";"):
            syms[sym] += 1
    # Demangle Rust/C++ symbols when a demangler exists (best effort).
    demangled = {}
    for demangler in (["rustfilt"], ["c++filt"]):
        try:
            r = subprocess.run(demangler, input="\n".join(syms),
                               capture_output=True, text=True, timeout=30)
            if r.returncode == 0:
                for raw, clean in zip(syms, r.stdout.splitlines()):
                    demangled[raw] = clean.strip()[:160]
                break
        except Exception:
            continue
    total = sum(syms.values()) or 1
    lines = []
    for s, n in syms.most_common(40):
        show = demangled.get(s, s)
        lines.append(f"{n:>8}  {100.0 * n / total:5.1f}%  {show}")
    (out / "top.txt").write_text(
        "\n".join(lines) + f"\n\n# {len(syms)} distinct symbols, {total} samples/frames\n"
    )

# --- meta.json ---
def version(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=10)
        return (r.stdout + r.stderr).strip().splitlines()[0][:120]
    except Exception:
        return "unknown"

sha = subprocess.run(
    ["git", "rev-parse", "HEAD"], capture_output=True, text=True, cwd=os.environ.get("ROOT", ".")
).stdout.strip()
meta = {
    "cell": cell,
    "tool": tool,
    "tool_version": version({"samply": ["samply", "--version"], "perf": ["perf", "--version"],
                             "sample": ["sample", "-help"], "dtrace": ["dtrace", "-V"]}[tool]),
    "base_sha": sha,
    "files": sorted(p.name for p in out.iterdir()),
}
(out / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")
print(f"wrote {out}/cell.json + top.txt + meta.json")
PY

echo "profiled $CELL -> $OUT (tool=$SELECTED)"
echo "next: view top.txt, $([ "$SELECTED" = samply ] && echo "samply load $OUT/profile.samply.json" || echo "folded.txt with flamegraph.pl if installed")"
