# Endpoint counter captures

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`, phase `native`, state `finished`. Failed RPCs and partial captures are retained. See summary.json and the per-group ledgers for outcomes; absent cells are not wins.

These are shared-host synthetic workload diagnostics. Read-all corpora, lifecycle, saturation, task wakeups and dedicated-host timing remain separate. Production qualification remains false. Run `python3 check.py --check .` to verify every retained file.

The [2026-10-22 validation](../../grpc-validation-20261022/README.md) invalidates the declared closed-loop concurrency in this capture set: the old generator always used four workers, and fixed counts could overshoot. Preserve these raw records, but repeat the declared-load comparisons with the repaired generator.
