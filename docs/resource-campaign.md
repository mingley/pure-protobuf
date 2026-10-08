# gRPC resource campaign

The campaign tests the current transport with plaintext/TLS and identity/gzip.
Every cycle checks exact unary and bidi payloads at 0, 1 KiB, 64 KiB and 1 MiB;
a paused reader of 128 responses; RPC overload; cancellation; deadlines;
recovery; and shutdown. A separate lifecycle fixture injects plaintext
RST_STREAM, GOAWAY and TCP resets on a rotating schedule, then checks recovery.
A warmed connection to the same server bypasses the faulted proxy. Its probe must return the expected reply within 300 ms; an
error response cannot prove that the server released its single RPC slot.
Compressed slow-reader responses use deterministic varied bytes so compression
cannot make the entire response fit into initial HTTP/2 credit. The fixture
allows up to one second for production to reach a stall, then checks unchanged
progress over a 30 ms observation.

Run from a clean, committed checkout on Linux:

```sh
python3 scripts/grpc-resource-campaign.py --source "$(git rev-parse HEAD)" \
  --duration 30 --output target/resource-campaign/preview
python3 scripts/grpc-resource-campaign.py --source "$(git rev-parse HEAD)" \
  --duration 86400 --output target/resource-campaign/24h
python3 scripts/grpc-resource-campaign.py --validate target/resource-campaign/24h/report.json
```

The runner builds the named test executable and records the source tree,
lockfile, tool versions, binary hash, commands and host. It rejects source or
binary drift. Output directories must be new. The child has finite limits:
1 GiB address space, 128 descriptors, 4,096 same-UID processes/threads, and
requested duration plus 30 CPU seconds. The wall watchdog adds 15 seconds.
The four profiles share a two-worker process. Each side has an explicit 8 MiB
byte tracker; message caps are 2 MiB, send buffers 16 KiB, and deadlines 3 seconds.
These settings differ from the older plaintext diagnostic.

Raw events include bytes/tokens, observed `ClientHello`/`ServerHello` lifecycle calls, tasks, descriptors,
RSS and process RSS high-water. After faults and drain, accounted bytes, tokens
and observed calls must return to zero. Starts must equal ends. Every drain
must stay within its initial baseline plus 32 MiB RSS, one descriptor and two
Tokio tasks. Independent process samples must cover the requested duration.
Missing phases, profiles, faults, failed children or unrecovered resources fail
validation. `progress.json` records the running PID and final disposition.

Schema v4 reads RSS and high-water from one `/proc/self/status` snapshot and
retains both raw counters. [Linux documents these counters as approximate](https://man7.org/linux/man-pages/man5/proc_pid_status.5.html);
their relative ordering is not an atomic measurement. `sampled_rss_peak_bytes`
is the maximum RSS in the retained phase and independent process samples.
It is a sampled peak, not allocator high-water. The post-drain RSS threshold
and finite address-space limit are unchanged.

A preview cannot become a completed 24-hour run. An interrupted or failed
24-hour attempt records `soak_24h.status=failed`. `completed` requires the
requested duration, a successful child, and passing resource checks. The
independent validator and `smoke.status` must also pass. Overall `qualified` remains false: this campaign does not measure
allocator high-water or kernel socket memory, separate endpoint resources,
encrypted HTTP/2 fault injection, or dedicated-host latency/goodput. It does
not replace feature, conformance and release gates. The environment must remain
running for the whole campaign.
