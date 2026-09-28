# H2-02: internal HTTP/2 transport seam

`pbrs-grpc/src/transport/` isolates every h2 call behind crate-private traits
so H2-04 can add a native engine without touching call sites.

## Shape

- `transport/mod.rs`: 11 traits (`SendStream`, `RecvStream`, `FlowControl`,
  `SendRequest`, `SendResponse`, `ClientConnection`, `ServerConnection`,
  `ClientBuilder`, `ServerBuilder`, `PingPong`) plus shared value types
  (`Error`, `Reason`, `Accepted`). No `h2::` paths.
- `transport/h2.rs`: the only module naming `h2::` types. Newtypes fix the
  buffer generic to `Bytes` and implement the traits 1:1. Call sites import
  the traits for methods and name `transport::h2` types in signatures.
- 14 files rerouted: `wire/{send,frame_reader,headers,out_batch}`,
  `client/{call,unary,streaming,pool}`, `server/{connection,drain,rpc}`,
  `config`, `keepalive`, `status`, plus `lib.rs` registration.
  `pbrs-grpc/tests/` still uses h2 directly as a test framing tool; those
  are separate crates that cannot see the private seam.

## Decisions

- Trait-only dispatch (no inherent methods on the newtypes): every transport
  call resolves through a trait definition, so the seam is structural, not
  decorative. Proven by zero dead-code warnings without allows, except two
  reasoned ones: unused RFC 9113 `Reason` codes and test-only
  `SendResponse::send_reset` (production answers trailers-only).
- `flow_control()` returns an owned handle: h2's handle is a shared
  refcounted window reference, so clone-then-release is behavior-identical
  to the borrowed call.
- Server accept loop uses `accept()` (identical to its old
  `poll_fn(poll_accept)`; that is h2's own definition of `accept`).
- Handshake boxes `io`: coroutine layouts keep dead upvar storage while
  h2's handshake future owns `io` again, so an unboxed upvar pays for a
  ~1KB TLS stream twice (+1264B in the dial future, +4.2% dev-loop
  alloc_bytes). Boxing keeps 8B in the state machine for one setup-time
  alloc; steady-state delta is +0.5%, inside the 1% budget.
- Follow-up for H2-04: the dial future still lives inside the per-RPC
  client box via pool acquire; boxing the cold dial path there would shrink
  the hot box below baseline.

## Gates (all on final SHA)

- `cargo test -p pbrs-grpc --lib`: 390 passed
- `cargo test -p pbrs-grpc --test rpc`: 11 passed
- `cargo test -p pbrs-grpc --test gaps`: 6 passed
- `cargo test -p pbrs-grpc --test hostile`: 37 passed
- `cargo test -p pbrs-grpc --test tls -- --test-threads=1`: 20 passed
- `./scripts/grpc-interop.sh --self-only`: PASS
- `./scripts/devloop.sh --baseline /tmp/h202-baseline.json`: exit 0,
  no regressions (unary alloc_bytes +0.5%)
- `cargo clippy -p pbrs-grpc --lib -- -D warnings`: clean
- `cargo fmt -p pbrs-grpc -- --check`: clean
- Leak check: no `h2::` path in `pbrs-grpc/src` outside
  `transport/h2.rs`, `transport::h2` imports, and one doc comment.
