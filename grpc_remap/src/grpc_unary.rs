//! Unary helloworld encode/decode through grpc-protobuf with protobuf = pbrs.
//! Not protobuf-tonic.

#![allow(nonstandard_style, unused, dead_code, clippy::all)]

#[path = "../generated/helloworld.rs"]
mod helloworld;

use bytes::Buf;
use grpc::core::{RecvMessage, SendMessage};
use grpc_protobuf::{ProtoRecvMessage, ProtoSendMessage};
use helloworld::{HelloReply, HelloRequest};
use protobuf::{AsMut, AsView, ClearAndParse, Message, MutProxied, Proxied, Serialize};

fn encode_via_grpc<M>(msg: &M) -> Vec<u8>
where
    M: AsView + Sync,
    M::Proxied: Proxied + Sync,
    for<'a> <M::Proxied as Proxied>::View<'a>: Serialize + Send + Sync,
{
    let send = ProtoSendMessage::<M::Proxied>::from_view(msg);
    let mut buf = send.encode().expect("ProtoSendMessage::encode");
    let mut out = vec![0u8; buf.remaining()];
    buf.copy_to_slice(&mut out);
    out
}

fn decode_via_grpc<M>(bytes: &[u8]) -> M
where
    M: Message + Default,
{
    let mut msg = M::default();
    {
        let mut recv = ProtoRecvMessage::from_mut(&mut msg);
        let mut buf = bytes::Bytes::copy_from_slice(bytes);
        recv.decode(&mut buf).expect("ProtoRecvMessage::decode");
    }
    msg
}

fn main() {
    fn _traits<M: Message + Proxied + MutProxied + ClearAndParse + Serialize>() {}
    _traits::<HelloRequest>();
    _traits::<HelloReply>();

    let mut req = HelloRequest::new();
    req.set_name("ada");
    assert_eq!(
        req.name().to_str().expect("utf8"),
        "ada",
        "HelloRequest.name after set"
    );

    let wire = encode_via_grpc(&req);
    let parsed = decode_via_grpc::<HelloRequest>(&wire);
    assert_eq!(
        parsed.name().to_str().expect("utf8"),
        "ada",
        "grpc-protobuf ProtoSend/Recv HelloRequest"
    );

    let mut reply = HelloReply::new();
    reply.set_message("Hello ada");
    let ser = Serialize::serialize(&reply).expect("Serialize HelloReply");
    let mut round = HelloReply::new();
    ClearAndParse::clear_and_parse(&mut round, &ser).expect("ClearAndParse HelloReply");
    assert_eq!(
        round.message().to_str().expect("utf8"),
        "Hello ada",
        "ClearAndParse/Serialize HelloReply"
    );

    let reply_wire = encode_via_grpc(&reply);
    let reply_parsed = decode_via_grpc::<HelloReply>(&reply_wire);
    assert_eq!(
        reply_parsed.message().to_str().expect("utf8"),
        "Hello ada",
        "grpc-protobuf ProtoSend/Recv HelloReply"
    );

    println!("ok name=ada message=Hello ada");
}
