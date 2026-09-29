# SV-04 static native routing evidence

## Change

Native stubs now dispatch server methods by matching the full `/<service>/<method>` path generated for each RPC. The generated server also exposes `add_static_service`, which returns a `pbrs_grpc::Server<(Self, S)>`; `pbrs-grpc` implements `Service` for service tuples up to six entries. Tuple dispatch checks static service names and aliases with prefix matches, avoiding `Router`'s per-RPC service `HashMap` lookup and boxed dynamic service future.

The existing dynamic `Router::add_service` API is unchanged.

## Validation commands

All commands ran on a contended macOS development host with `CARGO_BUILD_JOBS=3` and pinned `protoc` on `PATH`.

| Command | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `CARGO_BUILD_JOBS=3 cargo test -p pbrs-grpc --test static_routing --test codegen --test compat_fixtures` | Passed |
| `CARGO_BUILD_JOBS=3 cargo test --test codegen_compat` | Passed |
| `CARGO_BUILD_JOBS=3 cargo test --test pbrs_build` | Passed |
| `CARGO_BUILD_JOBS=3 cargo test --test documentation` | Passed |
| `CARGO_BUILD_JOBS=3 cargo test -p pbrs-grpc-example-greeter` | Passed |
| `CARGO_BUILD_JOBS=3 cargo clippy --lib --all-features -p pbrs-grpc -p pbrs-grpc-example-greeter -- -D warnings` | Passed |
| `CARGO_BUILD_JOBS=3 cargo clippy --lib --all-features -p pbrs -- -D warnings` | Passed |

`CARGO_BUILD_JOBS=3 cargo test --test onboarding` also ran. It passed 13 tests, ignored the pinned-protoc test, and failed only `packed_core_and_both_adapters_build_cold_without_protoc` because the staged package workspace referenced `examples/axum-cohost/Cargo.toml` but that file was absent from the package stage. SV-04 does not touch package staging or the axum example.

## Dev-loop unary RPC measurements

Command shape:

```bash
PATH="$PWD/target/pinned-protoc-build:$PATH" \
  CARGO_BUILD_JOBS=3 \
  ./scripts/devloop.sh --cells rpc.pbrs.unary --iters 2000 --repeats 3 \
  --out target/sv04/<baseline-or-after>.json
```

Baseline was `638bb775d80a` before the SV-04 patch. After was the SV-04 patch on the same base.

| Variant | Heap allocs/RPC median | Heap bytes/RPC median | Wall ns/op median | Wall CV | Notes |
| --- | ---: | ---: | ---: | ---: | --- |
| Baseline | 54.33 | 54,775.16 | 393,594.17 | 0.378 | `target/sv04/baseline-unary-current.json` |
| SV-04 | 54.345 | 54,776.04 | 336,281.88 | 0.184 | `target/sv04/after-unary.json` |

Instructions, syscall counts, and lock waits were not collected: `perf`, `strace`, and `valgrind` were unavailable on this host.

## Interleaved A/B unary QPS

Built a temporary baseline worktree at `638bb775d80a`, then alternated `devloop run-cell rpc.pbrs.unary --iters 2000` between the baseline binary and the SV-04 binary on the same host.

| Variant | QPS samples | Median QPS | Median wall ns/op | Median allocs/RPC | Median bytes/RPC |
| --- | --- | ---: | ---: | ---: | ---: |
| Baseline | 6345.57, 3673.25, 7215.23, 8925.41, 10693.27 | 7215.23 | 138,595.62 | 54.033 | 54,738.15 |
| SV-04 | 4473.78, 8304.15, 10160.02, 11785.38, 7438.45 | 8304.15 | 120,421.65 | 54.034 | 54,738.17 |

Median QPS improved by 15.09% in this noisy interleaved run. Allocation medians are effectively unchanged, as expected for replacing dispatch branches rather than transport/message buffers.

## Profiling limitation

`scripts/profile.sh --cell rpc.pbrs.unary --iters 2000 --skip-build` used macOS `sample`. Both baseline and after profiles produced only one sample and zero distinct symbols:

```text
# 0 distinct symbols, 1 samples/frames
```

Those profiles confirm the command ran but are not useful for attributing the delta. The dev-loop allocation and interleaved A/B wall-time data above are the actionable evidence for this host.
