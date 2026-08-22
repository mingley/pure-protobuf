#!/usr/bin/env bash
# INVENTORY ONLY. Re-run rust_out vs pbrs and refresh tmp/rust-out-link-errors.txt.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/tmp/rust-out-link-errors.txt"
mkdir -p "$ROOT/tmp"
# rust_out hardcodes ::protobuf; Cargo.toml remaps package pbrs as protobuf.
set +e
(cd "$(dirname "$0")" && cargo check) >"$OUT.raw" 2>&1
status=$?
set -e
python3 - "$OUT.raw" "$OUT" "$status" <<'PY'
import sys
from pathlib import Path
raw, dest, status = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
text = raw.read_text()
start = text.find("error[")
end_marker = "error: could not compile"
end = text.find(end_marker)
if start < 0:
    dest.write_text(text)
    raise SystemExit(f"no rustc error[ in cargo output; cargo exit {status}")
# include through the last "could not compile" line
tail = text[end:]
nl = tail.find("\n")
rustc = text[start:end + (nl if nl >= 0 else len(tail))]
if not rustc.endswith("\n"):
    rustc += "\n"
header = dest.read_text().split("error[", 1)[0] if dest.exists() else ""
if "MEASURE ONLY" not in header:
    header = "# rust_out inventory — MEASURE ONLY\n# see tests/rust_out_inventory/\n\n"
dest.write_text(header + rustc)
raw.unlink()
print(f"wrote {dest} (cargo exit {status})")
PY
exit "$status"
