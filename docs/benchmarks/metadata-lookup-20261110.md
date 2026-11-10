# Metadata lookup screen

This screen compares scratch candidate `5dc9b456` with its parent
[`cab0e580`](https://github.com/mingley/pure-protobuf/commit/cab0e5803c80c163722b5ea7356327a812510edf).
The candidate lets HTTP's header map match metadata keys without allocating a
lowercase copy first. The scratch commit may not be available in a normal
clone. To reconstruct the measured source, check out `cab0e580` and apply the
[retained patch](../evidence/metadata-lookup-20261110/candidate.patch) with
`git apply`. The resulting `pbrs-grpc/src/metadata.rs` has Git blob SHA
`1d5822cf8f8ba0f8bb94f637728adc9d59e5e563`.

Main at `b7321226` contains that same metadata source, but also has other
changes; the results below measure the candidate binaries, not that main build.

The measured change removes one allocation from each tested first-value lookup.
On this shared runner, every tested lookup case had a lower median process CPU
time than its paired baseline. These are component results, not a tonic
comparison or an RPC throughput claim. The first table uses a counting
allocator; an uninstrumented screen follows.

| Lookup | Cases | Range of candidate / baseline CPU medians | Allocations per lookup |
| --- | ---: | ---: | ---: |
| Short ASCII hit or miss | 4 | 0.7338–0.7647 | 1 → 0 |
| 130-byte ASCII hit or miss | 4 | 0.9275–0.9583 | 1 → 0 |
| Short binary hit | 2 | 0.8579–0.8586 | 2 → 1 |
| 134-byte binary hit | 2 | 0.9471–0.9505 | 2 → 1 |

Each group covers lowercase and mixed-case lookup keys. The tested 16-byte
binary value still allocates during decoding. The unchanged `get_all` control had a 1.0015
median ratio and observed block ratios from 0.9383 to 1.0763. The unchanged
reserved-key control had a 1.0013 median ratio and a 0.9916–1.0141 range.
These controls compare operations in different binaries; they are not
same-binary controls.

## Uninstrumented component screen

A [separate run](https://github.com/mingley/pure-protobuf/actions/runs/42807169844)
used the default allocator without the counting wrapper. The driver verified
single-CPU affinity, which child processes inherited; the host was shared.
Each case used eight alternating ABBA and BAAB blocks. Each process performed
two million lookups; its CPU time includes startup and teardown.

| Lookup | Cases | Range of candidate / baseline CPU medians |
| --- | ---: | ---: |
| Short ASCII hit or miss | 4 | 0.8402–0.8556 |
| 130-byte ASCII hit or miss | 4 | 0.9400–0.9514 |
| Short binary hit | 2 | 0.9022–0.9052 |
| 134-byte binary hit | 2 | 0.9550–0.9571 |

Across all cases, same-binary control ratios ranged from 0.8713 to 1.1335
for the baseline and 0.9416 to 1.0675 for the candidate. The unchanged
`get_all` operation had a 0.9937 median cross-binary ratio
(0.9445–0.9981); the reserved-key operation had a 0.9888 median
(0.9858–0.9952). Both unchanged operations shifted below one in every block.
Binary layout or other systematic effects may contribute, so treat small
differences with care. The ranges are observed extremes, not confidence
intervals.

The [samples](metadata-lookup-20261110-uninstrumented.json),
[probe](../evidence/metadata-lookup-20261110/uninstrumented-probe.rs), and
[runner](../evidence/metadata-lookup-20261110/run-uninstrumented.py) are
retained. [Artifact `13761845077`](https://github.com/mingley/pure-protobuf/actions/runs/42807169844/artifacts/13761845077) has SHA-256
`4b2e8b57883dbe9b97e9c9478ce06bbfb176c41d0a1f3968a78cdfd0878b02fb`.

## Loopback RPC screen

The [loopback screen](https://github.com/mingley/pure-protobuf/actions/runs/42807168432)
used the default allocator and verified single-CPU affinity on a shared host.
A native client made unary calls to a native server on a current-thread Tokio
runtime. Each call carried the same metadata; the server read it zero, one, or
eight times. Each process warmed 2,000 calls and measured 20,000 more. There
were eight alternating ABBA and BAAB blocks. Process CPU includes setup,
warmup, and teardown.

| Reads per RPC | Candidate / baseline CPU median (observed range) | Baseline same-binary control range | Candidate same-binary control range |
| ---: | ---: | ---: | ---: |
| 0 | 0.9782 (0.9615–1.0022) | 0.9319–1.0212 | 0.9728–1.0546 |
| 1 | 0.9841 (0.9620–1.0070) | 0.9822–1.0547 | 0.9643–1.0444 |
| 8 | 0.9863 (0.9694–1.0055) | 0.9524–1.0275 | 0.9800–1.0244 |

The zero-read case shifted as much as or more than the cases with reads, and
same-binary controls overlap. This run establishes neither an RPC speedup nor
a regression. Its [samples](metadata-lookup-20261110-rpc.json),
[probe](../evidence/metadata-lookup-20261110/rpc-probe.rs), and
[runner](../evidence/metadata-lookup-20261110/run-rpc.py) are retained.
[Artifact `13761725201`](https://github.com/mingley/pure-protobuf/actions/runs/42807168432/artifacts/13761725201) has SHA-256
`c59e9eb07016cceb29d30532ac860ae2d6e12f89954560cea005829ce860506c`.
An [independent replay](https://github.com/mingley/pure-protobuf/actions/runs/42808106953)
checked both screens' samples, alternating orders, affinity records, and
control calculations. A [separate audit](https://github.com/mingley/pure-protobuf/actions/runs/42808304851)
also checked the corrected runs.

## Instrumented method and limits

[The screen](https://github.com/mingley/pure-protobuf/actions/runs/42804285037)
used Rust 1.99.1, a shared GitHub-hosted Linux runner, and the exact source
pins above. Each of the 14 cases ran ten paired blocks, alternating baseline
first and candidate first. Each timed process performed two million lookups.
The reported metric is child process CPU time, which includes process startup,
map setup and teardown. The script attempts to pin itself and its children to
one CPU but does not record whether that attempt succeeded.

The input map included repeated values, 32 unrelated keys, and short and long
ASCII and binary keys. A separate 10,000-lookup counting pass measured
allocation requests. The timing binaries still contain the forwarding counting
allocator with its counters disabled, so each allocation checks an atomic flag.
The CPU numbers include this instrumentation.

[Raw samples](metadata-lookup-20261110.json) retain every block, allocation
count and byte count. [Artifact `13757219475`](https://github.com/mingley/pure-protobuf/actions/runs/42804285037/artifacts/13757219475) has SHA-256
`1bd415b5fa75ab753dc5fb522d4cf17c3272a791370afc686a258145037fcade`
and expires on 2027-02-08. The [independent replay](https://github.com/mingley/pure-protobuf/actions/runs/42805277792)
checked all ratios, case counts and allocation counts.

The [probe](../evidence/metadata-lookup-20261110/probe.rs) and
[runner](../evidence/metadata-lookup-20261110/run.py) are retained with a
checker. Run `python3 docs/evidence/metadata-lookup-20261110/check.py` to
verify the checked-in samples. To rerun the screen, place `probe.rs` at
`pbrs-grpc/examples/metadata_lookup_perf.rs` in both source checkouts. In
each checkout run `cargo build --locked --release -p pbrs-grpc --example metadata_lookup_perf`,
then run `run.py` with the baseline binary, candidate binary and a JSON output
path. Fresh measurements will vary by machine and host load.
