//! gRPC client: [`Channel`] and the four call shapes.

pub(crate) mod call;
pub(crate) mod channel;
pub(crate) mod config_glue;
pub(crate) mod pool;
pub(crate) mod retry;
pub(crate) mod streaming;
pub(crate) mod unary;

pub use channel::{Channel, Endpoint, Target};
pub use retry::RetryStats;

#[allow(dead_code, reason = "silence dead code")]
fn _silence_dead_code() {
    let _ = crate::wire::pump_outbound::<crate::hello::HelloRequest>;
}
