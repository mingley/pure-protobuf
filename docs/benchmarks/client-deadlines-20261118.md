# Warm client calls with deadlines

A direct native client with one ready connection can now use it for a
timed call without allocating a connection-acquisition future. It still
checks the deadline after trying the ready sender.

The comparison ran a native client before and after the change, plus a
tonic client, against the same native server. Each client used Prost and one
TCP connection, with a two-second wire timeout and identity framing. Requests
carried a 32-byte body and replies a 128-byte body. The server-stream call
read four replies.

| Call | Client CPU per RPC, base → candidate | tonic client CPU per RPC |
| --- | ---: | ---: |
| Unary | 8.986 → 8.898 µs | 14.495 µs |
| Server stream | 13.916 → 13.838 µs | 14.635 µs |

Eight paired blocks used separate client and server processes on pinned
cores. Median unary throughput went from 117,172 to 118,823 RPC/s against
64,430 RPC/s for the tonic client. This fixed-count run had zero reported
timeouts or failed RPCs. Stream throughput stayed within the baseline
control variation. [Per-call throughput](https://github.com/mingley/pure-protobuf/actions/runs/44412947796/job/135185639622)

The CPU differences between pbrs builds are small. A separate allocator
build recorded about one fewer request per call, using the difference
between N and 2N successful RPCs. These are hosted results for warm calls
with a deadline. [Raw pairs, allocation counts and provenance](https://github.com/mingley/pure-protobuf/actions/runs/44410971348/artifacts/14478260916)

The measurement compared base `7b34d68c` with `c04ca7fa`. The exact
formatted candidate `fc3e9adf` passed Clippy and the deadline,
cancellation, retry and pool tests on Rust 1.100.0.
[Correctness run](https://github.com/mingley/pure-protobuf/actions/runs/44411320924)
