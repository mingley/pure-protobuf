# dev-loop baselines (SB-04)

This directory documents baseline policy for the deterministic dev-loop harness
(`../README.md`, schema `devloop/1`). It is for contributors reading local
comparison files or the CI perf lane. CI compares base and head in
one job, so committed machine-specific reports are not the source of truth.

If a hand-kept report is added, name it `devloop-<12-hex-sha>.json`.

## What CI compares

The `dev-loop perf` workflow (`.github/workflows/perf.yml`) does **not** read
committed files here. Each pull request (PR) job builds the base SHA and the
head SHA in one Linux job, runs the full suite for both, and compares
base-vs-head. Both sides share one host and one toolchain. Reports are kept as
run artifacts for 180 days for SB-20 noise analysis across 30+ runs.

The workflow is advisory and can upload error placeholders after a failed
build. Count only reports with the required measured cells and valid source
revisions toward noise analysis. SB-23's local repair evidence still needs
confirmation in an actual CI run; a job badge or artifact name alone is not
proof that the measurements completed.

## What lives here

There is no committed report because instruction and wall-clock values are
host-specific. A checked-in baseline from one machine would mislead on another.
This directory holds this policy plus any hand-kept references with their host
recorded inside the JSON (`host` block). Compare locally with:

```sh
scripts/devloop.sh --baseline bench/devloop/baselines/<file>.json --out /tmp/now.json
```

## Absolute budgets (CH-10)

The one exception to "no committed numbers": `picker.json` pins absolute
ceilings for metrics that are exact counts, identical on every host and
toolchain — heap allocations and blocking lock waits per pick. A steady
pick must do zero of both. Instructions and wall stay out: they are
host-specific and gated relatively (base-vs-head). `compare --budget`
checks the current report against these ceilings and fails on violation;
`not_run` metrics skip, per policy. The perf lane passes this file on
every run (see `.github/workflows/perf.yml`).

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
