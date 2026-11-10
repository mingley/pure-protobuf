# Split streaming observer experiment source

This archive supports the [benchmark note](../../benchmarks/streaming-observer-split-20261110.md). The experiment is rejected and unmerged.

Starting from runtime `41d94325b8aa48b10b31fb6709c1a41dbc44a72a`, [observer.patch](observer.patch) reconstructs observer prototype `7056ff07a93903c7926ceb4dab1e8acd9c58f897`. Applying [split.patch](split.patch) after it reconstructs split candidate `b9533c54a2fda532420ba9381bdfbdcc39a87243`. The extraction check applies both patches to the pinned runtime and verifies the resulting source blobs.

[probe.rs](probe.rs) is the CPU fixture; [alloc.rs](alloc.rs) adds process-wide allocation counting. [performance-workflow.yml.txt](performance-workflow.yml.txt) archives the exact scratch workflow used for the screen, including its source pins and balanced run order. It is a historical record, not an active workflow. The [raw rows](../../benchmarks/streaming-observer-split-20261110.json) include all samples and controls.
