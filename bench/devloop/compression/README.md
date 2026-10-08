# Compression decode diagnostic

Measures gzip and deflate decoding without RPC setup. Inputs cover empty
messages, repeated text, zeros, and deterministic random bytes at 1 KiB,
64 KiB, and 1 MiB. Each decode checks the message-size limit.

Build and retain a binary before changing the decoder:

```sh
cargo build --locked --release --manifest-path bench/devloop/compression/Cargo.toml
cp bench/devloop/compression/target/release/compression-devloop work/decode-before
```

Build the candidate with the same compiler and lockfile, then compare:

```sh
cp bench/devloop/compression/target/release/compression-devloop work/decode-after
python3 bench/devloop/compression/measure.py \
  --baseline work/decode-before --candidate work/decode-after \
  --valgrind /usr/bin/valgrind --out work/decode-pair --repeats 3
```

The output directory must be new. Each child checks decoded contents before
the measured loop. The collector requires identical compressed-input
fingerprints across binaries, repeats, and N/2N runs. It retains commands,
exit codes, stdout, stderr, and full Callgrind graphs.

Allocation counts and requested bytes come from a counting system allocator.
Setup, warmup, and printing are outside its enabled window. Reallocations
count as allocations and contribute their requested size. Instruction counts
use Callgrind's `(2N-N)/N` method. N is 200 for small inputs, 32 for 64 KiB,
and 8 for 1 MiB. Pair order uses seed 11011.

Without `--valgrind`, instructions are marked `not_run`. Wall time under
Callgrind includes instrumentation overhead. These results isolate decoder
cost; RX-11's client/server RPC and peer comparisons remain separate work.

The standalone lockfile pins the diagnostic's dependencies. It does not
change the workspace dependency graph.

## Allocator safety check

The allocator has a unit test for alignment, zeroing, reallocation contents,
and deallocation. To run its exact source under Miri without building the
transport dependencies:

```sh
mkdir -p work/compression-allocator-miri
python3 - <<'PY'
from pathlib import Path
source = Path("bench/devloop/compression/src/allocator.rs").resolve()
Path("work/compression-allocator-miri/Cargo.toml").write_text(
    '[package]\nname = "compression-allocator-miri"\n'
    'version = "0.0.0"\nedition = "2024"\n[workspace]\n'
    f'[lib]\npath = "{source}"\n'
)
PY
cargo +nightly-2025-06-01 miri test \
  --manifest-path work/compression-allocator-miri/Cargo.toml
```
