//! A consumer codec predating the optional contiguous hook keeps compiling and serving.

#![allow(clippy::expect_used, reason = "consumer compatibility regression")]

use bytes::{Bytes, BytesMut};
use pbrs_grpc::{Channel, CodecMessage, Request, Response, Rpc, Server, Service, Status};

#[derive(Default)]
struct LegacyCodec(Bytes);

impl CodecMessage for LegacyCodec {
    fn encoded_len(&self) -> usize {
        self.0.len()
    }

    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        out.put_slice(&self.0);
        Ok(())
    }

    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        Ok(Self(payload))
    }

    fn empty() -> Self {
        Self::default()
    }
}

struct LegacyEcho;

impl Service for LegacyEcho {
    const NAME: &'static str = "cl08.Legacy";

    async fn call(&self, rpc: Rpc) {
        rpc.unary(|request: Request<LegacyCodec>| async move {
            Ok::<_, Status>(Response::new(request.into_inner()))
        })
        .await;
    }
}

#[tokio::test]
async fn existing_consumer_codec_uses_unchanged_default_and_round_trips() {
    let payload = Bytes::from_static(b"\x0a\x03old");
    let codec = LegacyCodec(payload.clone());
    let mut prefix = BytesMut::from(b"existing prefix".as_slice());
    assert!(codec.encode_contiguous(&mut prefix).is_none());
    assert_eq!(prefix.as_ref(), b"existing prefix");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::new(LegacyEcho)
            .serve_with_shutdown(listener, async {
                stop_rx.await.expect("shutdown signal");
            })
            .await
    });
    let channel = Channel::connect(address).await.expect("connect");
    let response: Response<LegacyCodec> = channel
        .unary("/cl08.Legacy/Echo", Request::new(codec))
        .await
        .expect("round trip");
    assert_eq!(response.into_inner().0, payload);
    drop(channel);
    stop_tx.send(()).expect("shutdown");
    server.await.expect("server task").expect("server drain");
}
