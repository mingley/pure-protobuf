# Resource attempt 008

Source `07e409aed328c9716718eb70ff05c15848fbb664`, release profile. At this snapshot the campaign is queued behind the remaining native regression tests. The actual 24-hour run has not started.

The queue requires every native test target and strict documentation checks to pass, retains the exact source used for each target, and records the isolated caller-rustls consumer output. It then builds the source-pinned RPC benchmark before launching the resource controller. The controller attempts a 30-second preview and runs the actual 86,400-second campaign only if the preview and its validator pass. Resource limits, the three-second server deadline and the 300 ms probe warmup are unchanged.

The controller publishes the final raw reports and outcome directly to main, including failures. `started.json` records this initial queue snapshot; the later `outcome.json` records the terminal result. The previous failed attempts remain [006](../grpc-resource-soak-20261008-attempt-006/README.md) and [007](../grpc-resource-soak-20261009-attempt-007/README.md).

Production qualification remains false. Features, the complete performance matrix, known tonic losses, allocator high-water and kernel-memory checks, and dedicated-host comparisons remain open.
