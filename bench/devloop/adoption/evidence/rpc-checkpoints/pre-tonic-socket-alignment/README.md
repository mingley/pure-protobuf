# RPC socket-default checkpoint

This incomplete collection used runtime source
`3024b7e4c1bd922b12c149d809e128e3d700ee27`, N=16/2N=32, three repeats,
100-RPC warmup, two Tokio workers and three independent collectors. The
unaltered progress record binds the source/binary/tools and the three
completed native reports (183 measured cells). No tonic report completed.

The run was stopped with exit 143 after review found that a raw
`TcpListenerStream` passed to tonic's `serve_with_incoming` bypasses the
server's default TCP_NODELAY=true. Tonic documents this behavior in 0.14.6.
The complete four-profile matrix must use an incoming adapter that applies
that default to accepted sockets. These native reports remain historical
measurements; this directory is not a complete or scored RPC baseline.

The accepted-socket regression was run with the unset `TcpIncoming::from`
recipe before adding `with_nodelay(Some(true))`:

```sh
cargo test --locked --manifest-path bench/devloop/Cargo.toml \
  --target-dir target/devloop --bin devloop \
  tonic_listener_preserves_default_tcp_nodelay
```

It failed with exit 101 and `tonic's default TCP_NODELAY must apply to accepted
sockets`. After the fix, the complete devloop binary test target passed all
eight tests (exit 0), including that real accepted-socket check. Rust 1.98.1
and pinned protoc 35.1 were used for both builds.
