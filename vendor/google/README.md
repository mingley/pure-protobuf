# Google / upb test corpus (v35.1)

Pin: `v35.1` (`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`).
License: BSD-3 (see `LICENSE`). Source: `github.com/protocolbuffers/protobuf`.

## What rust_upb actually runs

1. Official conformance suite. 5631 binary+JSON + 909 text cases. Test
   cases are generated in C++ (`conformance/binary_json_conformance_suite.cc`),
   not data files. The only way to run them is `conformance_test_runner`
   built from this pin. That is the cross-language spec suite every kernel
   including upb uses.

   rust_upb's expected-unfixed recommended list is
   `conformance/failure_list_rust_upb.txt` (proto2 UTF-8 reject). We
   currently pass recommended without that skip list.

2. `rust/test/shared/*.rs`. These are v4 application API tests (accessors,
   merge, serialize, proto!, UTF-8). They run against both the cpp and upb
   kernels in Google's tree. A proto3 subset is compiled by `cargo test`
   (`tests/google_shared.rs` + `tests/google_gen/`, regen with
   `./scripts/regen-google-unittest.sh`). The original googletest sources
   stay here as the remaining port corpus.

3. `upb/test/*.cc` and `rust/test/upb/`. These are C/minitable/arena
   internals. They are not applicable to a pure-Rust kernel and are not
   vendored.

## Layout

| Path | What |
|---|---|
| `PIN` / `SHA` | protobuf tag + commit |
| `conformance/failure_list_rust_upb.txt` | rust_upb recommended skips |
| `rust/test/*.proto` | unittest protos those shared tests compile |
| `rust-tests/shared/` | Google rust shared tests (source) |

The full protobuf tree (C++/upb needed to build the runner) is not in git
(~115 MiB). Fetch it:

```bash
./scripts/fetch-protobuf.sh
./scripts/conformance.sh
```
