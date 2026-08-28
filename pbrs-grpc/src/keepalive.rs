//! HTTP/2 PING so a dead peer is noticed before the next RPC.

use std::time::Duration;
use tokio::sync::watch;

/// Drive PINGs until one fails or times out. `None` means keepalive is off.
pub(crate) fn spawn(
    ping_pong: Option<h2::PingPong>,
    interval: Option<Duration>,
    timeout: Duration,
) -> Option<watch::Receiver<bool>> {
    let interval = interval?;
    let mut ping_pong = ping_pong?;
    let (tx, rx) = watch::channel(false);
    drop(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is immediate; skip it so we do not PING on connect.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match tokio::time::timeout(timeout, ping_pong.ping(h2::Ping::opaque())).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => {
                    tx.send(true).ok();
                    break;
                }
            }
        }
    }));
    Some(rx)
}

/// Wait until a keepalive task declares the connection dead.
pub(crate) async fn wait(mut dead: watch::Receiver<bool>) {
    dead.wait_for(|v| *v).await.ok();
}
