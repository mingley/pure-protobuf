# TC-03 prost native transport evidence

## Hypothesis

Native `pbrs-grpc` transport carrying prost messages should keep the lower
allocation profile of the native client/server path compared with tonic +
prost on equivalent unary and server-streaming loopback RPCs.

## Method

Added committed dev-loop cells:

- `rpc.prost.unary`
- `rpc.prost.server_stream`
- `rpc.tonic_prost.unary`
- `rpc.tonic_prost.server_stream`

Both stacks use the same `bench/devloop/proto/echo.proto` prost messages and
the same 1 KiB bytes payload. The native prost cells serve `devloop.Echo`
through `pbrs-grpc` with the optional `prost` feature and the
`CodecMessage` prost wrapper. The tonic-prost cells are aliases of the
existing tonic server/client over `tonic-prost`.

Linux/container command:

```sh
CARGO_BUILD_JOBS=3 scripts/devloop-linux.sh \
  --cells rpc.prost.unary,rpc.prost.server_stream,rpc.tonic_prost.unary,rpc.tonic_prost.server_stream \
  --iters 500 --repeats 3 --out target/tc03-prost/linux-500.json

CARGO_BUILD_JOBS=3 scripts/devloop-linux.sh \
  --cells rpc.prost.unary,rpc.prost.server_stream,rpc.tonic_prost.unary,rpc.tonic_prost.server_stream \
  --iters 1000 --repeats 3 --out target/tc03-prost/linux-1000.json
```

The two iteration counts provide a differential instruction estimate
(`1000 - 500`) in case process-startup instructions are still included in the
callgrind wrapper. Allocation counts are the primary signal.

## Results

| Cell | allocs/RPC | bytes/RPC | callgrind IR/RPC @1000 | differential IR/RPC |
|---|---:|---:|---:|---:|
| `rpc.prost.unary` | 35.000 | 52,021.000 | 129,591.225 | 130,666.515 |
| `rpc.tonic_prost.unary` | 71.510 | 49,177.015 | 138,867.125 | 141,959.095 |
| `rpc.prost.server_stream` | 54.025 | 75,172.640 | 165,659.860 | 170,930.135 |
| `rpc.tonic_prost.server_stream` | 81.430 | 58,058.310 | 150,939.090 | 150,408.940 |

## Verdict

The native prost cells beat tonic-prost on allocation count for both targeted
RPC shapes:

- unary: 35.000 vs 71.510 allocs/RPC;
- server-streaming: 54.025 vs 81.430 allocs/RPC.

Instruction counts are mixed: native prost unary is lower, while native prost
server-streaming is higher in this cell. Wall time was collected by dev-loop on
a contended host and is not claim-grade evidence. The accepted TC-03
performance claim here is allocation-count parity/win on the committed dev-loop
cells, not an end-to-end throughput claim.

## Limits

These cells use loopback, concurrency 1, and the dev-loop echo schema. They do
not replace the full `rpc-bench` claim-grade transport matrix. Prost encode in
the native wrapper currently materializes a `Vec`, so the pbrs direct-segment
encode fast path does not apply to prost messages.
