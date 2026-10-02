# SB-24 effective CPU capacity correction — 2026-10-02

Source `daf5407758f1a7edac830513b8f7545931d6a781` repairs the benchmark
headroom denominator. Eight observed threads pinned to one CPU now have a
one-CPU budget, further capped by the minimum ancestor quota, including
fractional CPUs. Thread and host CPU counts remain diagnostic. Thresholds
stay at 95% generator utilization, 80% reference-server utilization and 10%
scheduled-send lag. Shipping Rust, transport, codec and dependency policy
are unchanged. SB-24 remains open for its wider acceptance requirements.

The sampler requires matching per-PID mount/cgroup namespaces, consistent
live-task cgroup membership, observed task affinity, an unambiguous mount
mapping and matching filesystem device. It proves the genuine kernel
hierarchy root before treating ancestor quota minima as available capacity.
Unknown, unsupported or observed-changing capacity emits
`headroom_verified=false`, `effective_cpu_capacity=null`, unknown spare
capacity and `FAIL (UNVERIFIED)`. Existing saturation gates then reject it.
Missing CPU counter intervals cannot certify idle capacity either.
The stack server-headroom gate additionally checks actual affinity width
against the declared pin and uses effective capacity rather than configured
CPU count. Older records without these proofs cannot establish headroom.

## Genuine kernel root versus namespace root

[Pinned Linux source and excerpts](sb24-cpu-capacity-20261002/linux-source-pins.json)
use upstream v6.12 commit `adc218676eef25575469234709c2d87185ca223a`:

- `cpu.max`, `cgroup.type` and `cgroup.events` have `CFTYPE_NOT_ON_ROOT`.
  The kernel suppresses those interfaces only when there is no parent cgroup.
  A cgroup namespace can expose a subgroup as `/`; that subgroup retains its
  non-root interfaces and can have hidden throttling ancestors.
- A v2 genuine-root proof therefore requires those non-root interfaces to be
  absent, the CPU controller to be available and remaining core/CPU interfaces
  to be readable on the verified cgroup filesystem. Only that proven genuine
  root may omit `cpu.max` and be treated as unthrottled. Missing descendant
  quota files still reject the proof.
- V1 uses readable `release_agent` and `cgroup.sane_behavior`, both marked
  `CFTYPE_ONLY_ON_ROOT`, plus the same namespace/device mapping checks.

Subgroup roots retain visible quota readings and `visible_cpu_capacity` as
diagnostic upper bounds. They never certify full capacity. Missing markers,
controllers, files or namespace/device identity leave capacity unverified.
The source-informed fixtures test genuine roots and rejected hidden-parent
roots; they are not a live bare-host kernel qualification. The running host
is Linux 6.18.44, distinct from the retained v6.12 source characterization.

## Validation and real-host rejection

[Original red guards](sb24-cpu-capacity-20261002/capacity-before.log) retain
three denominator/headroom failures and six missing-helper errors, with
[three failed stack guards](sb24-cpu-capacity-20261002/headroom-before.log).
Final [34 matrix tests](sb24-cpu-capacity-20261002/capacity-after.log) and
[40 stack tests](sb24-cpu-capacity-20261002/headroom-after.log) pass; two
credential-dependent stack tests skip explicitly. Coverage includes many
threads on one CPU, fractional/ancestor quotas, live-thread affinity union,
unequal task cgroups, genuine-root interface absence, hidden namespace roots,
missing controllers/quotas, namespace/device ambiguity, sampled drift and
missing CPU deltas. Extended-cell and connection-scale self-tests, Python
compilation and diff checks pass. Existing Rust Clippy failures remain in
the earlier SB-21 evidence; this patch changes only Python benchmark code.

The [real-host functional fixture](sb24-cpu-capacity-20261002/real-host-failclosed.json)
ran at the clean source pin above. Two separate idle Python processes were
pinned to CPU 0; their observed client/server thread counts were eight/two.
Both recorded affinity `[0]`, `effective_cpu_capacity=null` and the reason
`cgroup mount root hides or ambiguously maps ancestors`. The visible root
`cpu.max` observation (`400000 100000`) remains unmapped diagnostic data.
The monitor returns **FAIL (UNVERIFIED)** with unknown spare capacity, even
for these idle endpoints. It cannot revive the historical thread-count PASS.
This 0.2-second fixture executes no RPC workload or throughput measurement.

The [fixture command](sb24-cpu-capacity-20261002/host-capacity-fixture.py) is
reproducible on this hidden-hierarchy host. Full source, interpreter and
kernel-source hashes accompany the result; [artifact hashes](sb24-cpu-capacity-20261002/artifact-sha256.json)
cover all logs and excerpts. Capacity constraints are read at monitor
checkpoints; distinct observations and sample times are retained. Reads are
not an atomic kernel-wide transaction and cannot prove unseen transient
configuration changes. CPU windows still include process setup/drain and
are not certified as aligned RPC measurement windows.

All ten prior invalid stack rows and both debug/optimized QPS diagnostics
remain intact. No heavy benchmark ran for this correction. Dedicated-host
headroom, effective peer settings, required peer/shape coverage, TLS/session
equality, aligned windows and saturation-knee/overload acceptance remain open.
