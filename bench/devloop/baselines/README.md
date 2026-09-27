# dev-loop baselines (SB-04)

Reference reports for the deterministic dev-loop harness (`../README.md`,
schema `devloop/1`). File naming: `devloop-<12-hex-sha>.json`.

## What CI compares

The `dev-loop perf` workflow (`.github/workflows/perf.yml`) does NOT read
committed files here. Each PR job builds the base SHA and the head SHA in
one Linux job, runs the full suite for both, and compares base-vs-head, so
both sides share one host and one toolchain. Reports are kept as run
artifacts (180 days) for SB-20 noise analysis across 30+ runs.

## What lives here

No committed report: instruction and wall-clock values are host-specific,
so a checked-in baseline from one machine would mislead on another. This
directory holds this policy plus any hand-kept references with their host
recorded inside the JSON (`host` block). Compare locally with:

```sh
scripts/devloop.sh --baseline bench/devloop/baselines/<file>.json --out /tmp/now.json
```

## Minimal example cell

```json
{
  "schema": "devloop/1",
  "cells": [
    {
      "id": "rpc.pbrs.unary",
      "instructions": {"status": "not_run", "data": {"reason": "no perf or valgrind on PATH"}},
      "allocs": {"status": "measured", "data": {"value": 57.335, "unit": "heap allocs per RPC (median)"}}
    }
  ]
}
```

`not_run` metrics never pass or fail a comparison; they skip.
