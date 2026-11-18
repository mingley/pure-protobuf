# Server connection admission

An admitted RPC borrows its connection's dispatcher, peer information and
configuration until it builds the request. The task no longer needs to
store owned copies of that context in the admission queue.

A warmed in-process test measured 1,024 RPCs in each sample. The change
saved 112 requested bytes per RPC in all three cases. The allocation
request counts matched within each case.

| Call | Base | Candidate |
| --- | ---: | ---: |
| Cached unary | 16,281 bytes | 16,169 bytes |
| Owned unary | 16,541 bytes | 16,429 bytes |
| Two-reply stream | 23,595 bytes | 23,483 bytes |

The archived hook control expected `Metadata::insert` to replace a header.
It stopped at that assertion on the base. The ordinary rows ran before
that control. [Allocation log and probe](https://github.com/mingley/pure-protobuf/actions/runs/44413112293/artifacts/14477678934)

## TCP comparison

Rust 1.100.0 built each server with the same Prost codec. One tonic client
drove every server over TCP. All 48 runs completed 24,000 RPCs each with
zero reported failures or timeouts on a shared Linux runner.

| Case | Candidate / base median throughput | Candidate / tonic median throughput |
| --- | ---: | ---: |
| Empty unary | 1.007× | 1.045× |
| 1 KiB unary | 1.000× | 1.065× |
| Four 1 KiB replies | 1.003× | 1.102× |
| Eight connections and 32 in flight | 1.033× | 1.012× |

Candidate/base throughput and CPU sample ranges included parity in all
four cases. The multiplexed throughput range against tonic also included
parity. The direct allocation result measures the smaller admission state.
[TCP captures](https://github.com/mingley/pure-protobuf/actions/runs/44413112293/artifacts/14478497707)
· [181 server tests](https://github.com/mingley/pure-protobuf/actions/runs/44410509727/job/135177071015)
