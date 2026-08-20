# protobuf-tonic

tonic `Codec` over **pure-protobuf** (`Parse` / `Serialize`).

This is not `tonic-prost`. tonic’s workspace default is prost. These message
types do not implement `prost::Message` and will not compile as
`tonic-prost` request/response types.

The crate depends on **tonic 0.12** (conservative pin for a unary proof). The
kernel itself does not depend on tonic. Bumping to 0.13/0.14 is an adapter
change, not a kernel change.

## Client

```rust
use protobuf_tonic::ProtobufCodec;
use tonic::client::Grpc;
use tonic::{Request, Response};

let mut grpc = Grpc::new(channel);
grpc.ready().await?;
let resp: Response<HelloReply> = grpc
    .unary(
        Request::new(req),
        "/helloworld.Greeter/SayHello".parse()?,
        ProtobufCodec::<HelloRequest, HelloReply>::default(),
    )
    .await?;
```

`ProtobufCodec<Encode, Decode>`: encode type first, decode type second.

See `tests/unary.rs` for a localhost server + client echo.
