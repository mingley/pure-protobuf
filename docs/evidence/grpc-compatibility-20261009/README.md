# Compiler, documentation and counter-runner checks

The stalled-upload response fix used syntax unavailable on the declared Rust 1.85 minimum. Source `930b3e17` fails the retained compiler check with E0658. Source `89edc7e2` preserves lazy response polling using syntax supported by 1.85. Its locked check, 530 unit tests and five stalled-upload regressions pass on the actual 1.85 compiler; one unit test is ignored. CI now locks dependencies and runs those regressions on the minimum compiler.

Source `2c66a91f` simplifies API documentation and keeps the non-documentation lines of config.rs and request.rs unchanged. Generated-code tests now check the relevant API links rather than require redundant English sentences. All 197 codegen tests and 569 all-feature unit tests pass; one unit test is ignored. The old wording-sensitive failure is retained.

Formatting, plan checks, queue tests and strict all-target/all-feature Clippy passed. The first documentation run failed because the launcher omitted protoc. With protoc supplied, all 25 documentation tests and Rustdoc with warnings denied pass. The first broader native run stopped at the same missing-protoc error in generated consumer fixtures. That output is retained; the corrected continuation is separate and this capsule does not claim the full suite passed.

Three counter-runner checks each completed 200 captures on source `f8517b53`, using empty or 1 KiB payloads, TLS and h2c, identity and gzip, and one or 16 in-flight calls. The final runner also records interruptions and source drift as failures, keeps completed captures, and continues other profiles after failed RPCs. These are runner checks, not measurements qualifying the current source against tonic. The complete counter matrix, known performance losses, remaining features and actual 24-hour soak remain open.

The archive retains commands, Cargo-selected artifacts, profiles, source pins, stdout, stderr, controllers, failed attempts and raw endpoint accounting. Executable hashes are retained in reports; executable bytes remain local and are excluded from this portable archive. Run `python3 check.py` to verify the inventory.
