# OSS-Fuzz integration packet (QG-02)

This page is a draft [OSS-Fuzz](https://google.github.io/oss-fuzz/)
integration packet. The three files below belong in
`projects/pure-protobuf/` in an OSS-Fuzz checkout. Rehearse the build before
submitting it; this record does not mean the project is enrolled.

No in-tree changes are needed. OSS-Fuzz builds the existing cargo-fuzz targets
with the same `cargo fuzz build` flow used by the weekly campaign lane
(`fuzz-campaign` in `.github/workflows/compatibility.yml`).

The target list lives in [fuzz/Cargo.toml](../../fuzz/Cargo.toml). Keep the
staging loop below in sync: `descriptors`, `formats`, `grpc_wire`,
`varint_diff`, `wire`.

## `projects/pure-protobuf/project.yaml`

```yaml
homepage: "https://github.com/mingley/pure-protobuf"
language: rust
primary_contact: "michael.ingley@gmail.com"
fuzzing_engines:
  - libfuzzer
sanitizers:
  - address
  - undefined
main_repo: "https://github.com/mingley/pure-protobuf.git"
```

## `projects/pure-protobuf/Dockerfile`

```dockerfile
FROM gcr.io/oss-fuzz-base/base-builder-rust
RUN git clone --depth 1 https://github.com/mingley/pure-protobuf.git pure-protobuf
WORKDIR pure-protobuf
COPY build.sh $SRC/
```

## `projects/pure-protobuf/build.sh`

```bash
#!/bin/bash -eu
# OSS-Fuzz build: compile every cargo-fuzz target and stage the binaries
# plus seed corpora into $OUT.
cd "$SRC/pure-protobuf"
cargo fuzz build -O
for target in descriptors formats grpc_wire varint_diff wire; do
  bin="fuzz/target/x86_64-unknown-linux-gnu/release/$target"
  cp "$bin" "$OUT/"
  if compgen -G "fuzz/corpus/$target/*" > /dev/null; then
    zip -j "$OUT/${target}_seed_corpus.zip" "fuzz/corpus/$target"/*
  fi
done
```

## Notes

- `base-builder-rust` ships nightly cargo with AddressSanitizer-capable
  LLVM; `cargo fuzz build` picks up `$RUSTFLAGS`/`$CFLAGS` from the
  OSS-Fuzz environment, so the exact sanitizer set in `project.yaml`
  applies without further flags here.
- Seed corpora: `fuzz/corpus/<target>/` holds minimized regression
  inputs (including `crash-<sha16>.bin` files retained by
  `scripts/fuzz-campaign.sh` triage); zipping them as
  `<target>_seed_corpus.zip` gives OSS-Fuzz the same starting corpus
  the local campaigns use.
- Dictionaries: none yet. If a target grows a branchy text/escape
  grammar worth a `.dict`, add a `fuzz/dicts/<target>.dict` file,
  copy it to `$OUT/<target>.dict` in `build.sh`, and document it here.
- Local rehearsal of the OSS-Fuzz build (Linux, nightly, cargo-fuzz
  installed):
  `cd fuzz && cargo +nightly fuzz build -O && ls target/*/release/{descriptors,formats,grpc_wire,varint_diff,wire}`.
  (On OSS-Fuzz itself the base builder's default toolchain is nightly,
  so `build.sh` above uses plain `cargo fuzz`.)
