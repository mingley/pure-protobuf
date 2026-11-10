# Metadata lookup screen

This screen compares [candidate `5dc9b456`](https://github.com/mingley/pure-protobuf/commit/5dc9b456c9af076c79755310992f6869244a44b0)
with its parent `cab0e580`. The candidate lets HTTP's header map match
metadata keys without allocating a lowercase copy first.

The measured change removes one allocation from each tested first-value lookup.
On this shared runner, every tested lookup case had a lower median process CPU
time than its paired baseline. These are component results, not a tonic
comparison or an RPC throughput claim.

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

## Method and limits

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
count and byte count. Artifact `13757219475` has SHA-256
`1bd415b5fa75ab753dc5fb522d4cf17c3272a791370afc686a258145037fcade`
and expires on 2027-02-08. The [independent replay](https://github.com/mingley/pure-protobuf/actions/runs/42805277792)
checked all ratios, case counts and allocation counts.

The [probe](../evidence/metadata-lookup-20261110/probe.rs) and
[runner](../evidence/metadata-lookup-20261110/run.py) are retained with a
checker. Run `python3 docs/evidence/metadata-lookup-20261110/check.py` to
verify the checked-in samples. To rerun the screen, place `probe.rs` at
`pbrs-grpc/examples/metadata_lookup_perf.rs` in both source checkouts, build
that example in release mode with each source pin, then run `run.py` with the
baseline binary, candidate binary and a JSON output path. Fresh measurements
will vary by machine and host load.
