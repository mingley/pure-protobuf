# Send-buffer RPC diagnostics

These local diagnostics examine the gRPC sender's change from two `Bytes`
slices to `split_to` for a frame sent in chunks. They use source
`5d280bfbced620bc63a761741fcac06b3c8a359d`. The before and after inputs
come from that commit; only `pbrs-grpc/src/wire/send.rs` was restored from
`a35701582fb9e6964c855386b0d9c991c50345ba` for the before build. The
checker reconstructs and compares all 596 selected source files per side.
These inputs are not a complete compiler filesystem-read trace.

The initial run completed 72 captures and 64,800 calls. It covered 64 KiB
and 8 MiB unary messages with three randomized paired repeats. A longer
8 MiB run completed 60 captures and 45,000 calls with five paired repeats.
Both used Prost messages, plaintext, identity compression, one connection,
one RPC in flight and two runtime workers per endpoint. Payload validation
checks lengths for zero-filled messages; it does not check every byte.
The binaries were release builds without allocation counters. The build
records retain commands, features, source and binary digests, and Cargo
artifact logs; they do not record the compiler version.

The longer run's paired CPU ratios and complete pair ranges were:

| Endpoint | New / old native | New native / tonic |
|---|---:|---:|
| Client | 0.777 (0.663–1.201) | 0.906 (0.782–1.808) |
| Server | 0.923 (0.763–1.043) | 1.021 (0.905–2.056) |

Lower is better. Each value is the median of paired N/2N CPU slopes; the
range includes all five pairs. The tonic-only controls also fluctuated
widely. In the initial three-pair screen, the 8 MiB client new/old median
was 1.389; a 64 KiB server control had a nonpositive slope, so its ratios
are absent. The runs disagree and are inconclusive about end-to-end gains.
They do not establish latency, throughput, an overall ranking or production
qualification.

The runs used one shared Linux x86_64 host. CPU totals derive from process
clock ticks. The recorded ticks and seconds imply a consistent tick rate,
but the host setting was not separately retained. N/2N slopes include
differences in startup and connection work. Scheduling, contention and
separately compiled binary layouts can affect these small comparisons.

Run `python3 check.py` in a checkout containing both named commits. The
checker verifies archive members, reconstructs source snapshots with the
same filtered Git archive extraction as `prepare.py`, checks build records
and the seeded execution plan, and recalculates every CPU summary from the
132 captured runs. It also checks terminal counts, command settings, and
internal consistency of recorded CPU ticks and seconds.

The archive keeps plans, helpers, build records and logs, and all capture
records and endpoint logs. Executables, caches and toolchains are omitted.
Binary hashes are checked against retained build and run records, but the
checker cannot rehash or replay the omitted executables. This packet is not
a compiler attestation, complete dependency mirror, independent signature,
or proof of any broader feature or performance claim.
