# Architecture decisions

These records explain why a design was chosen, rejected, or left as a
proposal. A completed design card does not imply that its implementation
ships. Use the [execution plan](../plan/world-class/README.md) for work order
and the [status page](../status.md) for qualification limits.

| Area | Read this | Decision or boundary |
|---|---|---|
| Runtime features | [Runtime/build split](runtime-build-split.md) | Keep one `pbrs` crate; opt out of default features for smaller runtime builds. |
| Parser | [Table-driven parse](table-driven-parse.md) | Keep small messages inline; generate tables for wider messages after PK-07 validation. |
| Owned-message allocation | [Owned arena](owned-arena.md) | Per-request arenas were rejected for the current owned API. |
| Borrowed messages | [Borrowed views](borrowed-views.md) | Proposed separate view roots; approval and implementation remain distinct. |
| Google-generated messages | [upb kernel](upb-kernel.md) | Proposed replacement behind the official generated-code interface. |
| Source compilation | [Rust frontend route](rust-frontend-route.md) | Proposed upstream-first route; checked descriptors remain the Rust-only generation path. |
| Transport engine | [H2 decision](h2-engine.md), [contingent design](pbrs-h2.md) | Keep `h2`; a replacement needs evidence that reopens the no-go decision. |
| Server scheduling | [Server dispatch](server-dispatch.md) | Keep per-RPC task spawning until profiling justifies a change. |
| Resolver and balancing | [Channel architecture](channel-architecture.md) | Direct channels stay the default; resolver-managed channels are opt-in. |
| xDS | [Build versus reuse](xds-build-vs-reuse.md) | Proposed native implementation with explicit review and interop gates. |
| Compression | [Backend and formats](compression.md) | Keep miniz_oxide; optional zstd has a separate MSRV and evidence boundary. |
| Tonic adoption | [Product shape](better-tonic.md) | Preserve crate names and provide optional adapters. |
| Browser protocols | [gRPC-Web and Connect](grpc-web-connect.md), [HTTP/1.1 recipe](http1-grpc-web.md) | Native gRPC-Web is HTTP/2; HTTP/1.1 uses a Tower composition recipe. Connect is separate work. |

Preserve dated measurements and rejected alternatives when updating a
record. Add a clear status update when later work changes the conclusion;
do not silently rewrite an old experiment as current evidence.
