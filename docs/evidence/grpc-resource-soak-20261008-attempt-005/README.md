# Optimized resource campaign

Prepared for source `0a357d148286fc9badc7b9fcee26011dd880a6bc` with an explicit release build. The controller checks Cargo optimization and assertion metadata, runs a 30-second preview, and starts an actual 86,400-second run only after the preview and its independent validator pass. RPC deadlines, flow-control windows, workloads, and finite process limits are unchanged.

The earlier debug campaign failed after 705.18 seconds. That failure remains recorded in attempt 004. Selecting the production build profile does not diagnose or close the debug failure. This preparation record contains no completed day. Overall production and performance qualification remain false.

The controller retains and publishes either successful or failed outcomes directly to main. Check launch.json and outcome.json for observed execution; started.json records the requested command.
