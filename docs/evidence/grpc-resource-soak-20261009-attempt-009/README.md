# Resource attempt 009

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`, release profile. The complete native inventory has finished. The preview and 24-hour run have not started at this snapshot. Documentation generation exhausted disk during the following checks; the [failure records](../grpc-adapter-buffer-20261009/README.md) are retained. The resumed queue removes inactive debug caches before retrying documentation.

The controller runs strict checks, retains actual caller-rustls consumer output, removes inactive compiler caches and builds a source-pinned benchmark. After all checks and builds finish, the controller attempts the original 30-second preview, followed by an actual 86,400-second run only if that preview and validator pass. Failed regression checks remain recorded and make the resource run diagnostic; they do not certify the release. All resource ceilings, the three-second server deadline and the 300 ms warmup remain unchanged. The original resource controller publishes its terminal outcome and raw reports directly to main, including failures.

Native fairness failures remain open. Completing the diagnostic soak would establish its actual duration and fixture outcome, while leaving those failures and release qualification open. A successfully built benchmark remains available for subsequent diagnostic counter sweeps. Previous failures and the confirmed queued-reset fix are recorded [here](../grpc-reset-admission-20261009/README.md). Production qualification remains false; features, the wider matrix and resource release checks remain open.

The complete native inventory, including failures, is published before the subsequent checks and resource run. The publisher uses a separate checkout and preserves the active source pin.
