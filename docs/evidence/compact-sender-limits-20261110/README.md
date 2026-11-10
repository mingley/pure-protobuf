# Compact streaming sender limits experiment source

This archive supports the [benchmark note](../../benchmarks/compact-sender-limits-20261110.md). Candidate `c134820046c26bcbbce9e23c96967bc0f6493631` is held and unmerged.

[compact-sender.patch](compact-sender.patch) reconstructs the candidate from baseline `c8dd4bd7738074a29dee8727814832a4e754198f`. The extraction check applies it and verifies both changed source blobs.

[probe.rs](probe.rs) is the CPU fixture; [alloc.rs](alloc.rs) adds process-wide allocation counting. [instrument-layout.py](instrument-layout.py) adds the separate layout probes. [sender-limit-controls.rs](sender-limit-controls.rs) is the test-only edge-value and Debug module. The archived [performance workflow](performance-workflow.yml.txt) and [correctness workflow](correctness-workflow.yml.txt) contain the exact scratch checks and source pins; they are historical records, not active workflows. The [raw rows](../../benchmarks/compact-sender-limits-20261110.json) retain every sample and control.
