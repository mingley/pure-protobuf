# Documentation

Start with a working example, then open the guide for your task.

| Task | Read |
|---|---|
| Generate and use protobuf messages | [Message quickstart](../README.md#quick-start-install-generate-use) |
| Build a gRPC service | [Greeter example](../examples/greeter/README.md), [gRPC guide](grpc.md) |
| Use streaming calls | [RPC shapes](guides/rpc-shapes.md) |
| Tune connections and scheduling | [Transport controls](guides/transport-controls.md) |
| Configure code generation | [Codegen guide](guides/codegen.md) |
| Move from tonic/prost | [Migration guide](guides/migration.md), [comparison](guides/comparison.md), [parity audit](parity.md) |
| Add authentication and middleware | [Interceptors](guides/interceptors.md) |
| Deploy and operate a service | [TLS, limits and shutdown](guides/production-service.md), [operations](guides/operations.md) |
| Check release readiness | [Current candidate checks](evidence/release-readiness-20261030/README.md), [release policy](RELEASE.md) |
| Compare performance | [Results](benchmarks.md), [run benchmarks](benchmarks/running.md) |

For exact API signatures, use [docs.rs](https://docs.rs/pbrs) or run
`cargo doc --workspace --no-deps --open`. Check the
[support policy](support-policy.md) and [implementation status](status.md)
for supported versions and remaining compatibility work.

Implementation details live in [architecture](architecture.md),
[runtime design](design.md) and [design decisions](decisions/README.md).
Contributors should start with the [current work queue](../TODO.md).
The [full documentation map](documentation-map.md) links the behavior
references, test tools and historical evidence.
