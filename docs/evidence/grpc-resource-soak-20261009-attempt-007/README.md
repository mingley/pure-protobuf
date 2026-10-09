# Stalled-upload source resource campaign

This campaign uses source `930b3e17b585542d151b3b2054116c11dd3b3bf7`, which includes both queued and flow-control-stalled peer rejection fixes. It builds the release-profile resource fixture, runs an unchanged-limit 30-second preview, validates it independently, then attempts an actual 86,400-second child only on success.

Preparation is not a completed build or a running day. The earlier-source attempt 006 continues independently and cannot qualify this runtime change. The controller publishes terminal build, preview and day outcomes directly to main. Launch snapshots will use `preview-at-launch`; final capsules use `preview` and `24h`.

The original seed, 3-second server deadlines, 1 GiB address-space limit, 128-descriptor cap, 4,096 same-uid-process cap, buffers and windows remain unchanged. Overall production and performance qualification remain false.
