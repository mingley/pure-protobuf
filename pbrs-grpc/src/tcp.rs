//! TCP socket options applied after accept or connect.
//!
//! `socket2` is a safe wrapper around the same libc syscalls tokio already
//! issues. It does not compile C. Unix domain sockets skip this module.
//! `TCP_NODELAY` is always on for TCP connect and accept (Nagle off).
//! There is no `tcp_nodelay(bool)` setter. Distinct from tonic, which
//! defaults Nagle off but lets you turn it back on.

use std::time::Duration;
use tokio::net::TcpStream;

/// `TCP_NODELAY` always; `SO_KEEPALIVE` when `keepalive` is `Some`.
pub(crate) fn tune(tcp: &TcpStream, keepalive: Option<Duration>) -> std::io::Result<()> {
    tcp.set_nodelay(true)?;
    if let Some(time) = keepalive {
        let ka = socket2::TcpKeepalive::new().with_time(time);
        socket2::SockRef::from(tcp).set_tcp_keepalive(&ka)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::tune;
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn tune_sets_nodelay_and_optional_keepalive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();

        tune(&client, Some(Duration::from_secs(15))).unwrap();
        assert!(client.nodelay().unwrap());
        assert!(socket2::SockRef::from(&client).keepalive().unwrap());

        tune(&server, None).unwrap();
        assert!(server.nodelay().unwrap());
        assert!(!socket2::SockRef::from(&server).keepalive().unwrap());
    }
}
