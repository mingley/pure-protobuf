//! A1 HTTP CONNECT proxy (CH-09): env-driven tunneling.
//!
//! Reads `HTTPS_PROXY` (or lowercase) plus `NO_PROXY`, dials the proxy,
//! and runs the `CONNECT` handshake so the resulting stream is a tunnel
//! to the target. TLS and HTTP/2 run end-to-end above the tunnel; the
//! proxy only sees opaque bytes after the `200`. No config plumbing:
//! every dial consults the process environment, matching gRPC A1.
//!
//! Parsing is split into pure `*_from_map` functions (unit-tested
//! without touching process env) plus thin `from_env` wrappers.

use base64::Engine;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::net::IpAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Upper bound on a proxy response head (no body is legal on 200).
const MAX_RESPONSE_HEAD: usize = 64 * 1024;

/// Proxy endpoint plus bypass rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyConfig {
    /// Proxy host (DNS name or IP literal, no brackets).
    pub(crate) host: String,
    /// Proxy port.
    pub(crate) port: u16,
    /// Basic-auth username from the URL, if any.
    pub(crate) username: Option<String>,
    /// Basic-auth password from the URL, if any.
    pub(crate) password: Option<String>,
    /// Bypass rules from `NO_PROXY`.
    pub(crate) no_proxy: Vec<NoProxyRule>,
}

/// One `NO_PROXY` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NoProxyRule {
    /// `*`: bypass everything.
    Any,
    /// Host suffix: matches the name and any subdomain.
    Host(String),
    /// Exact IP.
    Ip(IpAddr),
    /// IP prefix with length in bits.
    Cidr(IpAddr, u8),
}

impl ProxyConfig {
    /// Read proxy configuration from the process environment. Uppercase
    /// wins when both cases are set.
    pub(crate) fn from_env() -> Option<Self> {
        let mut vars = HashMap::new();
        for key in ["HTTPS_PROXY", "https_proxy", "NO_PROXY", "no_proxy"] {
            if let Ok(value) = std::env::var(key) {
                vars.insert(key.to_string(), value);
            }
        }
        Self::from_map(&vars)
    }

    /// Parse proxy configuration from `vars` (pure; test seam).
    pub(crate) fn from_map(vars: &HashMap<String, String>) -> Option<Self> {
        let raw = vars
            .get("HTTPS_PROXY")
            .or_else(|| vars.get("https_proxy"))?;
        if raw.trim().is_empty() {
            return None;
        }
        let (host, port, username, password) = parse_proxy_url(raw.trim())?;
        // `NO_PROXY` (gRPC A1) with lowercase fallback.
        let no_proxy = vars
            .get("NO_PROXY")
            .or_else(|| vars.get("no_proxy"))
            .map(|list| parse_no_proxy(list))
            .unwrap_or_default();
        Some(Self {
            host,
            port,
            username,
            password,
            no_proxy,
        })
    }

    /// Whether `target` bypasses the proxy. `target` is `host` or
    /// `host:port`; IP literals also match IP/CIDR rules.
    pub(crate) fn bypasses(&self, target: &str) -> bool {
        let host = strip_port(target).to_lowercase();
        let ip: Option<IpAddr> = host.parse().ok();
        self.no_proxy.iter().any(|rule| match rule {
            NoProxyRule::Any => true,
            NoProxyRule::Host(suffix) => host == *suffix || host.ends_with(&format!(".{suffix}")),
            NoProxyRule::Ip(addr) => ip == Some(*addr),
            NoProxyRule::Cidr(net, bits) => ip.is_some_and(|ip| cidr_contains(*net, *bits, ip)),
        })
    }

    /// `host:port` dial string for the proxy itself.
    pub(crate) fn dial(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// `Proxy-Authorization` value, if the URL carried credentials.
    fn authorization(&self) -> Option<String> {
        let user = self.username.as_deref()?;
        let secret = self.password.as_deref().unwrap_or("");
        let raw = format!("{user}:{secret}");
        Some(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
        ))
    }
}

/// Parse `host:port`, `http://host:port`, or
/// `http://user:pass@host:port` (https scheme rejected: CONNECT proxies
/// are plain-HTTP; TLS runs end-to-end inside the tunnel).
fn parse_proxy_url(raw: &str) -> Option<(String, u16, Option<String>, Option<String>)> {
    if raw
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        return None;
    }
    let rest = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("HTTP://"))
        .unwrap_or(raw);
    let (credentials, authority) = match rest.rsplit_once('@') {
        Some((creds, authority)) => (Some(creds), authority),
        None => (None, rest),
    };
    let (username, password) = match credentials {
        None => (None, None),
        Some(creds) => match creds.split_once(':') {
            Some((user, pass)) => {
                if user.is_empty() {
                    return None;
                }
                (Some(user.to_string()), Some(pass.to_string()))
            }
            None => {
                if creds.is_empty() {
                    return None;
                }
                (Some(creds.to_string()), None)
            }
        },
    };
    // `authority` is host:port; `[v6]:port` keeps brackets for the dial.
    let (host, port) = split_host_port(authority)?;
    if host.is_empty() {
        return None;
    }
    Some((host, port, username, password))
}

/// Split `host:port` / `[v6]:port` / bare `host` (default port 8080).
fn split_host_port(authority: &str) -> Option<(String, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        return Some((format!("[{host}]"), port.parse().ok()?));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            // A second colon without brackets is a bare IPv6 literal.
            if host.contains(':') {
                return Some((format!("[{authority}]"), 8080));
            }
            Some((host.to_string(), port.parse().ok()?))
        }
        None => Some((authority.to_string(), 8080)),
    }
}

/// Parse a comma-separated `NO_PROXY` list. Entries may carry `:port`
/// (ignored), except bare IPv6 literals which keep their colons.
fn parse_no_proxy(list: &str) -> Vec<NoProxyRule> {
    let mut rules = Vec::new();
    for entry in list.split(',') {
        let entry = entry.trim().trim_matches('.').to_lowercase();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            rules.push(NoProxyRule::Any);
            continue;
        }
        if let Some((net, bits)) = entry.split_once('/') {
            if let (Ok(ip), Ok(bits)) = (net.parse::<IpAddr>(), bits.parse::<u8>()) {
                let max = if ip.is_ipv4() { 32 } else { 128 };
                if bits <= max {
                    rules.push(NoProxyRule::Cidr(ip, bits));
                    continue;
                }
            }
        }
        let host = strip_port(&entry);
        if let Ok(ip) = host.parse::<IpAddr>() {
            rules.push(NoProxyRule::Ip(ip));
        } else if !host.is_empty() {
            rules.push(NoProxyRule::Host(host.to_string()));
        }
    }
    rules
}

/// Strip `:port`, preserving bare IPv6 literals and `[v6]` brackets.
fn strip_port(host: &str) -> &str {
    if let Some(stripped) = host.strip_prefix('[') {
        return stripped.split(']').next().unwrap_or(host);
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') && !port.is_empty() => name,
        _ => host,
    }
}

/// Whether `ip` falls inside `net/bits`.
fn cidr_contains(net: IpAddr, bits: u8, ip: IpAddr) -> bool {
    match (net, ip) {
        (IpAddr::V4(net), IpAddr::V4(ip)) => {
            if bits == 0 {
                return true;
            }
            let shift = 32 - bits;
            (u32::from(net) >> shift) == (u32::from(ip) >> shift)
        }
        (IpAddr::V6(net), IpAddr::V6(ip)) => {
            if bits == 0 {
                return true;
            }
            let shift = 128 - bits;
            (u128::from(net) >> shift) == (u128::from(ip) >> shift)
        }
        _ => false,
    }
}

/// Run the CONNECT handshake for `target` (`host:port`) over an open
/// proxy connection. Returns the tunnel on `2xx`.
pub(crate) async fn tunnel(
    mut stream: TcpStream,
    target: &str,
    proxy: &ProxyConfig,
) -> std::io::Result<TcpStream> {
    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(auth) = proxy.authorization() {
        request.push_str(&format!("Proxy-Authorization: {auth}\r\n"));
    }
    request.push_str("Proxy-Connection: Keep-Alive\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if head.len() >= MAX_RESPONSE_HEAD {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("proxy {}: response head exceeds 64 KiB", proxy.dial()),
            ));
        }
        if stream.read_exact(&mut byte).await.is_err() {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                format!("proxy {}: closed during CONNECT", proxy.dial()),
            ));
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let status = parse_status(&head).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidData,
            format!("proxy {}: malformed CONNECT response", proxy.dial()),
        )
    })?;
    if !(200..300).contains(&status) {
        let hint = if status == 407 {
            " (proxy authentication required)"
        } else {
            ""
        };
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!(
                "proxy {}: CONNECT {target} refused with {status}{hint}",
                proxy.dial()
            ),
        ));
    }
    Ok(stream)
}

/// A18 `TCP_USER_TIMEOUT` (Linux): set the option in milliseconds.
///
/// Lives here rather than `tcp.rs` because that module forbids unsafe
/// code and socket2 exposes no public API for this option.
#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "raw TCP_USER_TIMEOUT setsockopt; socket2 exposes no public API"
)]
pub(crate) fn set_user_timeout(
    tcp: &TcpStream,
    timeout: std::time::Duration,
) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let ms = libc::c_uint::try_from(timeout.as_millis()).unwrap_or(libc::c_uint::MAX);
    // SAFETY: the fd is borrowed from a live `TcpStream` for the call;
    // `TCP_USER_TIMEOUT` takes a `u32` by pointer; `ms` outlives the
    // call; nothing aliases or retains the pointer afterwards.
    let ret = unsafe {
        libc::setsockopt(
            tcp.as_raw_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_USER_TIMEOUT,
            &ms as *const _ as *const libc::c_void,
            C_UINT_LEN,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::cast_possible_truncation,
    reason = "size_of::<c_uint>() is 4 and always fits socklen_t"
)]
const C_UINT_LEN: libc::socklen_t = std::mem::size_of::<libc::c_uint>() as libc::socklen_t;

#[cfg(not(target_os = "linux"))]
pub(crate) fn set_user_timeout(
    _tcp: &TcpStream,
    _timeout: std::time::Duration,
) -> std::io::Result<()> {
    Ok(())
}

/// Read back `TCP_USER_TIMEOUT` (Linux test verification).
#[cfg(all(target_os = "linux", test))]
#[allow(
    unsafe_code,
    reason = "raw TCP_USER_TIMEOUT getsockopt; socket2 exposes no public API"
)]
pub(crate) fn get_user_timeout(tcp: &TcpStream) -> std::io::Result<Option<std::time::Duration>> {
    use std::os::unix::io::AsRawFd;
    let mut ms: libc::c_uint = 0;
    let mut len = C_UINT_LEN;
    // SAFETY: same borrowing discipline as `set_user_timeout`; `len`
    // correctly sizes the out param.
    let ret = unsafe {
        libc::getsockopt(
            tcp.as_raw_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_USER_TIMEOUT,
            &mut ms as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if ms == 0 {
        return Ok(None);
    }
    Ok(Some(std::time::Duration::from_millis(u64::from(ms))))
}

/// Parse the status code from a response head.
fn parse_status(head: &[u8]) -> Option<u16> {
    let line = head.split(|b| *b == b'\r').next()?;
    let mut parts = line.split(|b| *b == b' ');
    let version = parts.next()?;
    if !version.eq_ignore_ascii_case(b"HTTP/1.0") && !version.eq_ignore_ascii_case(b"HTTP/1.1") {
        return None;
    }
    let code = parts.next()?;
    if code.len() != 3 {
        return None;
    }
    std::str::from_utf8(code).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use tokio::net::TcpListener;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn user_timeout_round_trips_on_linux() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = TcpStream::connect(addr).await.expect("connect");
        assert_eq!(get_user_timeout(&client).expect("get"), None);
        set_user_timeout(&client, std::time::Duration::from_millis(5000)).expect("set");
        assert_eq!(
            get_user_timeout(&client).expect("get"),
            Some(std::time::Duration::from_millis(5000))
        );
    }

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn proxy_url_shapes() {
        let cfg = ProxyConfig::from_map(&vars(&[("HTTPS_PROXY", "proxy:8080")])).unwrap();
        assert_eq!(cfg.dial(), "proxy:8080");
        assert!(cfg.username.is_none());
        let cfg =
            ProxyConfig::from_map(&vars(&[("https_proxy", "http://u:p@proxy:3128")])).unwrap();
        assert_eq!(cfg.dial(), "proxy:3128");
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
        assert_eq!(
            cfg.authorization().as_deref(),
            Some("Basic dTpw"),
            "base64(user:pass)"
        );
        // Uppercase wins; empty disables; https rejected.
        let cfg = ProxyConfig::from_map(&vars(&[("HTTPS_PROXY", "a:1"), ("https_proxy", "b:2")]))
            .unwrap();
        assert_eq!(cfg.dial(), "a:1");
        assert!(ProxyConfig::from_map(&vars(&[("HTTPS_PROXY", "   ")])).is_none());
        assert!(ProxyConfig::from_map(&vars(&[("HTTPS_PROXY", "https://proxy:8080")])).is_none());
        assert!(ProxyConfig::from_map(&vars(&[])).is_none());
    }

    #[test]
    fn no_proxy_matching() {
        let cfg = ProxyConfig::from_map(&vars(&[
            ("HTTPS_PROXY", "proxy:8080"),
            (
                "NO_PROXY",
                "example.com, 10.0.0.0/8, 192.168.1.7, internal:8080",
            ),
        ]))
        .unwrap();
        assert!(cfg.bypasses("example.com:443"));
        assert!(cfg.bypasses("api.example.com:443"), "subdomain suffix");
        assert!(!cfg.bypasses("notexample.com:443"));
        assert!(cfg.bypasses("10.9.9.9:443"), "CIDR");
        assert!(!cfg.bypasses("11.0.0.1:443"));
        assert!(cfg.bypasses("192.168.1.7:443"), "exact IP");
        assert!(!cfg.bypasses("192.168.1.8:443"));
        assert!(cfg.bypasses("internal:9090"), "port-insensitive");
        assert!(!cfg.bypasses("external:443"));

        let star =
            ProxyConfig::from_map(&vars(&[("HTTPS_PROXY", "proxy:8080"), ("NO_PROXY", "*")]))
                .unwrap();
        assert!(star.bypasses("anything.example:443"));

        let v6 = ProxyConfig::from_map(&vars(&[
            ("HTTPS_PROXY", "proxy:8080"),
            ("NO_PROXY", "fd00::/8"),
        ]))
        .unwrap();
        assert!(v6.bypasses("[fd00::1]:443"));
        assert!(!v6.bypasses("[fe80::1]:443"));
    }

    #[test]
    fn status_line_parsing() {
        assert_eq!(parse_status(b"HTTP/1.1 200 OK\r\n\r\n"), Some(200));
        assert_eq!(
            parse_status(b"HTTP/1.0 200 Connection established\r\n\r\n"),
            Some(200)
        );
        assert_eq!(
            parse_status(b"HTTP/1.1 407 Proxy Auth Required\r\n\r\n"),
            Some(407)
        );
        assert_eq!(parse_status(b"HTTP/1.1 502 Bad Gateway\r\n\r\n"), Some(502));
        assert_eq!(parse_status(b"HTTP/2 200 OK\r\n\r\n"), None);
        assert_eq!(parse_status(b"garbage\r\n\r\n"), None);
        assert_eq!(parse_status(b"HTTP/1.1 20 OK\r\n\r\n"), None);
    }
}
