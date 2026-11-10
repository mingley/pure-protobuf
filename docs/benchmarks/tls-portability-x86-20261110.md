# TLS portability: x86-64 performance control, November 10

[Benchmarks](../benchmarks.md)

On these shared-runner samples, the TLS portability candidate showed no clear
performance change in x86-64 configuration construction or warmed unary
RPCs. The controls overlap the observed differences; these measurements do
not establish a speedup, a precise regression bound or an overall ranking.

The screens compare baseline
[`f73b85d`](https://github.com/mingley/pure-protobuf/commit/f73b85d067ea861fe36f23536153e4c5add3137f)
with candidate
[`1713882`](https://github.com/mingley/pure-protobuf/commit/171388257f15d261a1667799f40a38edc3bbeffe).
The candidate selects the built-in provider by target architecture. Both
builds used the same active x86-64 dependency features and locked registry
versions. These tests do not qualify another architecture or compare with
tonic.

## Configuration construction

The [initial screen](https://github.com/mingley/pure-protobuf/actions/runs/42792686993)
measured fresh built-in config construction and drop on one pinned guest CPU
on a shared AMD EPYC 7763 runner. Each cell has eight alternating pairs.
Values are candidate / baseline process CPU: median (range; slower pairs).
The next columns show the range of repeated same-binary CPU ratios. Lower is
faster.

| Constructor | CPU ratio | Baseline control | Candidate control | Allocation requests / bytes per config |
|---|---:|---:|---:|---:|
| client-ca | 0.9881 (0.5809–1.0282; 2/8) | 0.9961–1.3885 | 0.6646–1.0064 | 31 / 34,190 |
| server | 1.0019 (0.9947–1.0205; 4/8) | 0.9938–1.0081 | 0.9947–1.0044 | 24 / 32,671 |
| client-mtls | 0.9996 (0.9924–1.0034; 4/8) | 0.9905–1.0130 | 0.9901–1.0008 | 36 / 34,569 |
| server-mtls | 1.0003 (0.9831–1.0090; 5/8) | 0.9969–1.0057 | 0.9919–1.0058 | 36 / 35,981 |

The identity PEM was parsed before timing; CA parsing inside the constructor
and server ticket-key creation were included. Allocation figures are
successful global-allocator requests, including reallocations at their new
requested size, across three 16-operation samples per variant. Both variants
matched in every sample. These are cumulative requested bytes, not peak
memory. No handshake, socket or RPC is included.

The initial client-CA samples hit a 10,000-operation cap and used only
about 28–48 ms of process CPU, with especially wide controls. A separate
[longer repeat](https://github.com/mingley/pure-protobuf/actions/runs/42793832275)
on a shared Intel Xeon Platinum 8573C used 12 pairs of 65,536 operations.
Its candidate / baseline median was 0.9974 (0.9787–1.0234;
5/12 slower); baseline and candidate controls were 0.9683–1.0320
and 0.9868–1.0448. The allocation samples again matched at
31 requests / 34,190 requested bytes per config. The longer result is still
inconclusive.

## Warmed unary RPCs

The [RPC screen](https://github.com/mingley/pure-protobuf/actions/runs/42792639677)
used a native client and server on one loopback connection, two Tokio workers
and two distinct guest-visible cores on a shared AMD EPYC 7763 runner.
Connection setup, TLS config construction and handshake were outside the
timed interval. Each process warmed 256 calls and validated every response.
A separate witness checked the HTTP/HTTPS scheme and rejected a wrong CA.
The table reports combined client-and-server process CPU over eight pairs;
width is the number of concurrent calls.

| Transport | Width | Calls per sample | CPU ratio | Baseline control | Candidate control |
|---|---:|---:|---:|---:|---:|
| plain | 1 | 8,566 | 0.9928 (0.9606–1.0074; 2/8) | 0.9712–1.0081 | 0.9929–1.0088 |
| plain | 32 | 35,680 | 0.9938 (0.9768–1.0024; 2/8) | 0.9789–1.0195 | 0.9926–1.0245 |
| tls | 1 | 7,573 | 0.9997 (0.9911–1.0202; 4/8) | 0.9999–1.0145 | 0.9628–1.0201 |
| tls | 32 | 32,064 | 0.9968 (0.9827–1.0157; 3/8) | 0.9910–1.0232 | 0.9796–1.0070 |

Separate counting-allocator runs estimate process-wide per-call differences
between 1,024 and 2,048 completed requests. There was one such pair of
sample sizes per variant and workload. These values include both endpoints
and background activity; they are not peak memory or stable per-call bounds.

| Transport | Width | Allocation requests per call: base → candidate | Requested bytes per call: base → candidate |
|---|---:|---:|---:|
| plain | 1 | 23.006 → 23.006 | 16,526.1 → 16,526.1 |
| plain | 32 | 23.010 → 23.021 | 16,605.7 → 16,646.2 |
| tls | 1 | 30.006 → 30.008 | 16,853.1 → 16,853.1 |
| tls | 32 | 28.202 → 28.198 | 16,962.4 → 16,950.3 |

## Evidence

Both builds used Rust 1.99.1 with release optimization, thin LTO and one
codegen unit. The [constructor audit](https://github.com/mingley/pure-protobuf/actions/runs/42793537897),
[longer-repeat audit](https://github.com/mingley/pure-protobuf/actions/runs/42794616430)
and [RPC audit](https://github.com/mingley/pure-protobuf/actions/runs/42794081010)
verified the source and fixture pins, active x86 feature trees, paired
ratios and allocation samples. The RPC audit also rechecked the four
scheme/CA witnesses and rejected a modified CPU sample.

The [initial construction data](tls-portability-x86-config-20261110.json),
[longer client-CA data](tls-portability-x86-ca-repeat-20261110.json) and
[RPC data](tls-portability-x86-rpc-20261110.json) retain all samples. Probe
source and lockfiles are in the [evidence directory](../evidence/tls-portability-x86-20261110/README.md).
The 90-day [construction bundle](https://github.com/mingley/pure-protobuf/actions/runs/42794616430/artifacts/13753513195)
has SHA-256
`26e0a7fe22564379741b54e6f575d5a870ac897b9fa78f6e013b88bd063b6f43`;
the [RPC bundle](https://github.com/mingley/pure-protobuf/actions/runs/42794081010/artifacts/13753172394)
has SHA-256
`828533e69a0b37e3ee4589ae62d029b988e1c37ffc0bc80934b1d0a2f058120a`.
Both expire February 8, 2027.

To recheck the retained data while the bundles are available (Python 3.11+
and the baseline commit are required):

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  mkdir -p "$temp_dir/certs"
  for name in ca.crt server.crt server.key client.crt client.key; do
    git -C "$repo_dir" show "f73b85d067ea861fe36f23536153e4c5add3137f:pbrs-grpc/tests/tls_data/$name" > "$temp_dir/certs/$name"
  done
  gh run download 42794616430 --repo mingley/pure-protobuf \
    --name verified-tls-client-ca-long-f73-171 --dir "$temp_dir/config"
  python3 "$repo_dir/docs/benchmarks/tls-portability-x86-config-verify.py" \
    "$temp_dir/config/initialbundle/tls" "$temp_dir/certs" "$repo_dir/docs/benchmarks/tls-portability-x86-config-20261110.json"
  python3 "$repo_dir/docs/benchmarks/tls-portability-x86-ca-repeat-verify.py" \
    "$temp_dir/config/tls" "$temp_dir/certs" "$repo_dir/docs/benchmarks/tls-portability-x86-ca-repeat-20261110.json"
  gh run download 42794081010 --repo mingley/pure-protobuf \
    --name verified-tls-portability-rpc-20261110 --dir "$temp_dir/rpc-bundle"
  unzip -q "$temp_dir/rpc-bundle/original.zip" -d "$temp_dir/rpc"
  python3 "$repo_dir/docs/benchmarks/tls-portability-x86-rpc-verify.py" "$temp_dir/rpc"
  cmp "$temp_dir/rpc/verified-tls-rpc.json" "$repo_dir/docs/benchmarks/tls-portability-x86-rpc-20261110.json"
)
```
