# Code Generation and Custom Stubs

This guide details how to generate Rust messages and gRPC service stubs using `pbrs::codegen` and the `protoc-gen-pbrs` plugin.

---

## 1. Using `build.rs` (Recommended)

Add `pbrs` as a build dependency in `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1.0-alpha.1"

[build-dependencies]
pbrs = "0.1"
```

### Basic Compilation
In `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

Then include the generated code in your crate (`src/lib.rs` or `src/main.rs`):

```rust
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/service.rs"));
}
```

---

## 2. Configuring Stub Flavours

By default, `pbrs::codegen::compile_protos` emits native `pbrs-grpc` stubs (`Stubs::Kernel`). You can customize stub generation using `pbrs::codegen::Config`:

```rust
use pbrs::codegen::{Config, Stubs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Config::new()
        // Default: native pbrs-grpc FooClient / FooServer stubs
        .emit_kernel_stubs(true)
        // Or for tonic services:
        // .emit_tonic_stubs(true)
        // Or messages only (no service stubs):
        // .stubs(Stubs::None)
        .compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

### Stub Generation Summary
| Flavour | Builder Setting | Generated Code | Target Stack |
|---|---|---|---|
| **Native Kernel** | `.emit_kernel_stubs(true)` (Default) | `Foo`, `FooServer`, `FooClient` | `pbrs-grpc` |
| **Tonic Adapter** | `.emit_tonic_stubs(true)` | `#[tonic::async_trait]` stubs | `tonic` 0.14+ via `protobuf-tonic` |
| **Messages Only** | `.stubs(Stubs::None)` | Structs, enums, and WKT traits only | Core `pbrs` |

---

## 3. Multi-File Protos and Dependency Handling

When a `.proto` file imports other `.proto` files:

```rust
Config::new()
    .out_dir("src/gen")
    // Include all imported non-WKT definitions in the generated output
    .emit_deps(true)
    // Optional: disable emission of Well-Known Types if handled separately
    .no_wkt(false)
    .compile_protos(
        &["proto/main.proto"],
        &["proto", "vendor/shared_protos"],
    )?;
```

Alternatively, pass `PURE_PROTOBUF_EMIT_DEPS=1` via environment variables.

---

## 4. Standalone CLI Plugin (`protoc-gen-pbrs`)

You can invoke `protoc` directly using the `protoc-gen-pbrs` binary.

### Installation
```bash
cargo install --path . --bin protoc-gen-pbrs
```

### Invocation
Ensure `protoc-gen-pbrs` is in your `$PATH`, then invoke `protoc`:

```bash
protoc \
  --pbrs_out=./gen \
  --pbrs_opt=stubs=kernel,emit_deps=true \
  --proto_path=./proto \
  ./proto/service.proto
```

Supported `--pbrs_opt` options:
- `stubs=kernel`: Emit native `pbrs-grpc` stubs.
- `stubs=tonic`: Emit `tonic` stubs.
- `stubs=none`: Emit messages only.
- `emit_deps=true`: Emit imported non-WKT definitions.
- `no_wkt=true`: Suppress Well-Known Types emission.

---

<a id="manual-service"></a>
## 5. Writing a Service Without Codegen

For dynamic proxies, gateways, or custom routing, you can implement the `Service` trait directly without generated stubs:

```rust
use pbrs_grpc::{Incoming, Request, Response, Rpc, Service, Status};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone)]
struct RawEchoService;

impl Service for RawEchoService {
    fn call(
        &self,
        rpc: Rpc,
    ) -> Pin<Box<dyn Future<Output = Result<(), Status>> + Send + 'static>> {
        Box::pin(async move {
            match rpc.path() {
                "/echo.Echo/Echo" => {
                    rpc.unary(|req: Request<Vec<u8>>| async move {
                        let payload = req.into_inner();
                        Ok(Response::new(payload))
                    }).await
                }
                _ => {
                    rpc.unimplemented().await
                }
            }
        })
    }
}
```

This pattern provides direct access to raw byte frames, interceptor context, and path-based dispatch.
