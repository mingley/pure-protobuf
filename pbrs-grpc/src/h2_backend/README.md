# Embedded HTTP/2 backend

The native client and server compile this module directly. The registry `h2`
dependency remains an independent test peer and supports optional adapters.

`provenance.json` retains the upstream h2 0.4.19 import and original
transformation hashes. Later changes are recorded in Git. Those original
hashes do not certify the modified files at a newer revision. Qualification
records must identify the current commit, binary and workload.

The Linux stream mutex uses futexes and `UnsafeCell`. A guard is `Sync` only
when its protected value is `Sync`; moving a guard between threads remains
valid because futex ownership is not thread-specific. Other platforms use
the standard mutex. The crate's unsafe-invariant inventory documents these
constraints.

`scripts/miri-grpc-kernels.py` checks that its test groups exist before
running the lock, stream-store, header and frame safety tests. Full protocol,
interoperability, fuzz, resource and performance qualification remain separate.
