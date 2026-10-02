//! Source-pinned CL08 layout probe. These futures are created but never polled.
//! Public `Call` size describes its erased-box wrapper, not the boxed RPC body.

use super::{Channel, handshake};
use crate::rt::TokioRuntime;
use std::mem::{size_of, size_of_val};
use tokio::sync::watch;

#[test]
fn report_unpolled_future_sizes() {
    let channel = Channel::connect_lazy("127.0.0.1:9").expect("valid lazy target");
    let inner = &channel.inner;
    let acquire = inner.acquire(false, None, None, None);
    let (_cancel, cancel_rx) = watch::channel(false);
    let grab = channel.grab_in::<TokioRuntime>(cancel_rx, None, false, None);
    let cold = handshake(&inner.endpoint, inner.dial, inner.tls.as_ref());
    println!(
        "CL08_FUTURE_SIZES {{\"acquire_bytes\":{},\"grab_in_bytes\":{},\"handshake_bytes\":{},\"public_call_wrapper_bytes\":{},\"boxed_rpc_body_bytes\":null,\"polled\":false}}",
        size_of_val(&acquire),
        size_of_val(&grab),
        size_of_val(&cold),
        size_of::<crate::Call<crate::Response<crate::HelloReply>>>(),
    );
}
