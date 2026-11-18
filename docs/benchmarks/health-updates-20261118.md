# Health status updates

The health reporter updates its status map in place when it owns the only
reference. An active List snapshot retains the previous map. Repeating a
status or clearing an absent service leaves watchers idle. Check looks up
the service name by reference.

The Rust 1.100.0 release probe compared seven pairs on a shared Linux
runner, with 256 named services.

| Operation | Base | Candidate |
| --- | ---: | ---: |
| Change one status | 7,851 ns | 207 ns |
| Set the current status | 7,799 ns | 30 ns |
| Clear an absent name | 7,783 ns | 22 ns |
| Shutdown and resume | 7,940 ns | 330 ns |
| Check handler | 183 ns | 166 ns |

These are direct reporter and handler calls without contention or TCP.
The tested candidate also passed Check, List, Watch, snapshot isolation
and load-balancer health tests. [Raw timings](https://github.com/mingley/pure-protobuf/actions/runs/44412897554/artifacts/14477498724)
· [Correctness run](https://github.com/mingley/pure-protobuf/actions/runs/44411938948/job/135182108070)
