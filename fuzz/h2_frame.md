# Shipping HTTP/2 receive-codec fuzz target

`h2_frame` drives the native transport's private, modified embedded h2 receive
codec. That source was imported from upstream h2 0.4.19 and subsequently
modified. It is not the optional registry h2 peer, and it does not qualify the
proposed custom pbrs-h2 engine.

The bridge exists only under `cfg(test)` or cargo-fuzz's `cfg(fuzzing)`. Its
hidden crate entry point exists only under `cfg(fuzzing)`; normal builds and
Cargo features expose no new API. The target uses the shipping `Codec`,
including frame-length splitting, frame loaders, HPACK, and CONTINUATION state.
It compares semantic frames and error classes from contiguous and fragmented
in-memory reads. Each pair is dropped before the next is read. Header-cache and
pool bookkeeping and error debug strings are excluded from that comparison.
The snapshot consumes the shipping cache-hit header publications before polling
the other decoder and materializes the shared fields/pseudos they represent.

The harness accepts at most 64 KiB, sets the receive frame limit to 16 KiB,
and sets header-list and HPACK table limits to 4096 bytes. Header lists marked
oversize remain valid codec outcomes; the connection owns the subsequent
stream-reset policy. Reads inject finite Pending results and have a finite
poll budget; no sockets, runtime, TLS, clocks, or peer tasks are created. The
driver stops at EOF or the first codec error. It does not cover the connection
preface, SETTINGS application, stream/flow-control lifecycle or recovery after
a stream error.

Checked seeds include valid control/DATA frames, HPACK dynamic references
split across CONTINUATION, header-list overflow with dynamic-table continuity,
invalid padding, invalid HPACK indices, continuation ordering/stream/flood
violations, truncation, and the maximum/oversized frame-length boundary. Eight
unit tests check decoded values and rejection classes as well as fragmentation.

Run the finite regressions on the normal toolchain:

```sh
cargo test --locked -p pbrs-grpc --lib h2_backend::fuzz_bridge::tests
```

The required CI test job also installs cargo-fuzz 0.13.2 and explicitly selects
nightly for `scripts/fuzz-h2-frame-smoke.sh`. That script really compiles and
runs the target with a 15-second requested wall budget, a 64 KiB libFuzzer input
limit, a five-second per-input timeout, a 512 MiB RSS limit, and a 120-second
outer backstop. It copies the seeds to a scratch corpus and retains build/run
logs, source/tool versions, generated corpus units and crash artifacts, including
on failure. Ordinary Cargo test/Clippy checks retain their stable toolchain.
The existing scheduled campaign discovers the new registered target as well.

These are parser regressions and a bounded smoke check. The seeds are synthetic
parser controls, with no asserted SB-26 export/mapping. The smoke's wall budget
does not measure CPU use or establish QG-08's aggregate 24 CPU-hours. Full
protocol/connection qualification, sustained fuzz evidence and the remaining
adopter security packet are separate work.
