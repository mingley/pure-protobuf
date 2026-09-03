//! TCP connect (optional source bind) and socket options after accept or connect.
//!
//! `socket2` is a safe wrapper around the same libc syscalls tokio already
//! issues. It does not compile C. Unix domain sockets skip this module.
//! `TCP_NODELAY` is always on for TCP connect and accept (Nagle off).
//! There is no `tcp_nodelay(bool)` setter. Distinct from tonic, which
//! defaults Nagle off but lets you turn it back on.

use std::io::{Error, ErrorKind};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpSocket, TcpStream};

/// Dial `host`, optionally binding `local` first.
///
/// `local` `None` is [`TcpStream::connect`]. A bound address must share the
/// remote's family; otherwise the dial fails with [`ErrorKind::AddrNotAvailable`].
pub(crate) async fn connect(host: &str, local: Option<SocketAddr>) -> std::io::Result<TcpStream> {
    match local {
        None => TcpStream::connect(host).await,
        Some(local) => connect_bound(host, local).await,
    }
}

async fn connect_bound(host: &str, local: SocketAddr) -> std::io::Result<TcpStream> {
    let mut last_err = None;
    for remote in tokio::net::lookup_host(host).await? {
        if local.is_ipv4() != remote.is_ipv4() {
            continue;
        }
        match bind_connect(local, remote).await {
            Ok(stream) => return Ok(stream),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        Error::new(
            ErrorKind::AddrNotAvailable,
            format!("connect {host} from {local}: no usable address"),
        )
    }))
}

async fn bind_connect(local: SocketAddr, remote: SocketAddr) -> std::io::Result<TcpStream> {
    let socket = if local.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    socket.bind(local)?;
    socket.connect(remote).await
}

/// `TCP_NODELAY` always; `SO_KEEPALIVE` when `keepalive` is `Some`.
/// `keepalive_interval` is `TCP_KEEPINTVL` after that idle time.
/// `keepalive_retries` is `TCP_KEEPCNT`. Neither turns `SO_KEEPALIVE` on by
/// itself.
pub(crate) fn tune(
    tcp: &TcpStream,
    keepalive: Option<Duration>,
    keepalive_interval: Option<Duration>,
    keepalive_retries: Option<u32>,
) -> std::io::Result<()> {
    tcp.set_nodelay(true)?;
    if let Some(time) = keepalive {
        let ka = socket2::TcpKeepalive::new().with_time(time);
        let ka = apply_keepalive_interval(ka, keepalive_interval);
        let ka = apply_keepalive_retries(ka, keepalive_retries);
        socket2::SockRef::from(tcp).set_tcp_keepalive(&ka)?;
    } else {
        let _ = (keepalive_interval, keepalive_retries);
    }
    Ok(())
}

#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "visionos",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "windows",
    target_os = "cygwin",
    target_os = "nuttx",
    all(target_os = "wasi", not(target_env = "p1")),
))]
fn apply_keepalive_interval(
    ka: socket2::TcpKeepalive,
    interval: Option<Duration>,
) -> socket2::TcpKeepalive {
    match interval {
        Some(interval) => ka.with_interval(interval),
        None => ka,
    }
}

#[cfg(not(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "visionos",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "windows",
    target_os = "cygwin",
    target_os = "nuttx",
    all(target_os = "wasi", not(target_env = "p1")),
)))]
fn apply_keepalive_interval(
    ka: socket2::TcpKeepalive,
    _interval: Option<Duration>,
) -> socket2::TcpKeepalive {
    ka
}

#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "visionos",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "cygwin",
    target_os = "windows",
    target_os = "nuttx",
    all(target_os = "wasi", not(target_env = "p1")),
))]
fn apply_keepalive_retries(
    ka: socket2::TcpKeepalive,
    retries: Option<u32>,
) -> socket2::TcpKeepalive {
    match retries {
        Some(retries) => ka.with_retries(retries),
        None => ka,
    }
}

#[cfg(not(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "visionos",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "cygwin",
    target_os = "windows",
    target_os = "nuttx",
    all(target_os = "wasi", not(target_env = "p1")),
)))]
fn apply_keepalive_retries(
    ka: socket2::TcpKeepalive,
    _retries: Option<u32>,
) -> socket2::TcpKeepalive {
    ka
}

#[cfg(test)]
mod tests {
    use super::{connect, tune};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn tune_sets_nodelay_and_optional_keepalive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();

        tune(
            &client,
            Some(Duration::from_secs(15)),
            Some(Duration::from_secs(5)),
            Some(3),
        )
        .unwrap();
        assert!(client.nodelay().unwrap());
        assert!(socket2::SockRef::from(&client).keepalive().unwrap());

        tune(&server, None, Some(Duration::from_secs(5)), Some(3)).unwrap();
        assert!(server.nodelay().unwrap());
        assert!(!socket2::SockRef::from(&server).keepalive().unwrap());
    }

    #[tokio::test]
    async fn connect_bound_source_is_the_loopback_alias() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), 0);
        let client = connect(&addr.to_string(), Some(bind)).await.unwrap();
        let (_server, peer) = listener.accept().await.unwrap();
        assert_eq!(peer.ip(), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)));
        assert_eq!(client.local_addr().unwrap().ip(), peer.ip());
    }

    #[tokio::test]
    async fn connect_unbound_still_dials() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = connect(&addr.to_string(), None).await.unwrap();
        let (_server, peer) = listener.accept().await.unwrap();
        assert_eq!(peer.ip(), client.local_addr().unwrap().ip());
    }
}
