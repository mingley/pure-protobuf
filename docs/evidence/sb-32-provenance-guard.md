# SB-32 complete build-input provenance guard

Source `4cc3c99ad25a3c241c83b150e90e60d43bb3c86b` closes a gap found
in independent review of the measurement driver. The previous source
`61e6f9f1`, its eight passing synthetic gates and their evidence remain
unchanged in history and [the original evidence directory](sb-32-driver/validation.json).
This leaf changes the three Python support scripts and adds evidence. It
executes no Rust build, code generation, RPC or performance measurement.

The required tracked source snapshot now includes the OTLP schemas compiled
by `bench/devloop/build.rs` and the root build script's fallback conformance
descriptor set. The required third-party schema set covers all fourteen
roots/imports consumed by the current root, prost TAT, protobuf v4 TAT and
adoption/devloop build recipes. Exact recipe and schema hashes, consumers
and the pinned protobuf checkout commit are retained in
[required-build-inputs.json](sb-32-provenance-guard/required-build-inputs.json).

The release sidecar retains its existing normalized fields. Its
`source_sha256` must include every required tracked compiled input; extra
entries must be repository-relative tracked inputs. Its `schema_sha256`
must include all fourteen required third-party schemas, and may include
additional repository-relative schema inputs. The driver validates every
entry of both maps against actual bytes, binds every source entry to the
capture's Git commit, retains the complete maps and rechecks all their
inputs between measurement batches and at completion. Additional entries
are never discarded merely because they lie outside the required roots.

Every schema is archived beneath `schemas/<repository-relative-path>`.
This preserves distinct inputs with the same basename. The independent
audit checks required source coverage, all extra source pins, the complete
sidecar schema map and every archived schema hash. Registry and preflight
commands must also name the pinned release binary. Repeated reports of the
same verified source commit reuse that verification without skipping a
distinct commit.

Twelve Python tests pass. The four added tests exercise missing OTLP,
fallback FDS and each of the fourteen required schema pins; extra compiled
source drift and mismatched commit pins; extra schema drift; omitted extra
sidecar pins; changed archived schema bytes; equal-basename archives; and
escaping input paths. The full-envelope regression also rejects a foreign
registry/preflight binary while preserving visible replay losses. Exact
source hashes, commands, raw stdout/stderr, process exits, UTC times and
Python provenance are retained in
[validation.json](sb-32-provenance-guard/validation.json).

The workloads, N16/32 and three repeats, default warmup and runtimes,
original RPC 2% replay policy, separate codec/20-control 1% policy,
manifests and dependency pins are unchanged. The root-owned shared release
build, complete 512-state live capture, 24 retained map blocks and all
numeric qualification remain open.
