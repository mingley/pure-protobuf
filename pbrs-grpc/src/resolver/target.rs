//! Target URI parsing: `dns:///`, `passthrough:`, `ipv4:`, `ipv6:`,
//! `unix:`, `unix-abstract:`.

use crate::status::Status;
use std::net::SocketAddr;
use std::path::PathBuf;

/// A parsed resolver target: scheme plus its validated remainder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedTarget {
    /// Lowercase scheme (`dns`, `passthrough`, `ipv4`, `ipv6`, `unix`,
    /// `unix-abstract`).
    pub scheme: String,
    /// The remainder after `scheme:` with leading slashes trimmed.
    pub remainder: String,
}

impl ParsedTarget {
    /// `:authority` this target sends. DNS and passthrough keep the
    /// original `host:port` (never a resolved IP); IP schemes use the
    /// first address; Unix sockets use `localhost`.
    #[must_use]
    pub fn authority(&self) -> String {
        match self.scheme.as_str() {
            "unix" | "unix-abstract" => "localhost".to_owned(),
            "ipv4" | "ipv6" => self
                .remainder
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned(),
            _ => self.remainder.clone(),
        }
    }
}

/// Parse a resolver target URI. Plain `host:port` is not a URI: it stays
/// on [`crate::Channel::connect`] and is rejected here with directions.
pub fn parse_target_uri(input: &str) -> Result<ParsedTarget, Status> {
    let Some((scheme, rest)) = input.split_once(':') else {
        return Err(Status::invalid_argument(format!(
            "target {input:?} is host:port, not a resolver URI; use Channel::connect for host:port or one of dns:/// passthrough: ipv4: ipv6: unix: unix-abstract:"
        )));
    };
    if scheme.is_empty() || scheme.contains('/') {
        return Err(Status::invalid_argument(format!(
            "target {input:?} has no scheme; use Channel::connect for host:port or one of dns:/// passthrough: ipv4: ipv6: unix: unix-abstract:"
        )));
    }
    let scheme = scheme.to_ascii_lowercase();
    let remainder = rest.trim_start_matches('/').to_owned();
    match scheme.as_str() {
        "dns" | "passthrough" => {
            check_host_port(&scheme, &remainder)?;
        }
        "ipv4" => {
            check_ip_list(&remainder, true)?;
        }
        "ipv6" => {
            check_ip_list(&remainder, false)?;
        }
        "unix" => {
            if remainder.is_empty() {
                return Err(Status::invalid_argument(
                    "unix: target needs a socket path, e.g. unix:/run/grpc.sock",
                ));
            }
        }
        "unix-abstract" => {
            if remainder.is_empty() {
                return Err(Status::invalid_argument(
                    "unix-abstract: target needs an abstract name, e.g. unix-abstract:grpc",
                ));
            }
        }
        _ => {
            return Err(Status::invalid_argument(format!(
                "unknown target scheme {scheme:?}; want one of dns:/// passthrough: ipv4: ipv6: unix: unix-abstract:, or plain host:port on Channel::connect"
            )));
        }
    }
    Ok(ParsedTarget { scheme, remainder })
}

/// Split `host:port` for the DNS and passthrough schemes.
pub(crate) fn split_host_port(remainder: &str) -> Option<(&str, u16)> {
    let (host, port) = remainder.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    Some((host, port))
}

fn check_host_port(scheme: &str, remainder: &str) -> Result<(), Status> {
    if split_host_port(remainder).is_none() {
        return Err(Status::invalid_argument(format!(
            "{scheme}: target needs host:port with a numeric port 1-65535, got {remainder:?}"
        )));
    }
    Ok(())
}

fn check_ip_list(remainder: &str, want_v4: bool) -> Result<(), Status> {
    let scheme = if want_v4 { "ipv4" } else { "ipv6" };
    let mut count = 0;
    for entry in remainder.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let addr: SocketAddr = entry.parse().map_err(|_| {
            Status::invalid_argument(format!(
                "{scheme}: entry {entry:?} is not an IP:port address"
            ))
        })?;
        if addr.is_ipv4() != want_v4 {
            return Err(Status::invalid_argument(format!(
                "{scheme}: entry {entry:?} is the wrong address family"
            )));
        }
        count += 1;
    }
    if count == 0 {
        return Err(Status::invalid_argument(format!(
            "{scheme}: target needs at least one IP:port address"
        )));
    }
    Ok(())
}

/// Parse the validated `ipv4:`/`ipv6:` list into socket addresses.
pub(crate) fn parse_ip_list(remainder: &str) -> Vec<SocketAddr> {
    remainder
        .split(',')
        .filter_map(|entry| entry.trim().parse().ok())
        .collect()
}

/// Unix socket path for a validated `unix:` target.
#[must_use]
pub(crate) fn unix_path(target: &ParsedTarget) -> PathBuf {
    PathBuf::from(&target.remainder)
}

#[cfg(test)]
mod tests {
    use super::parse_target_uri;

    #[test]
    fn accepts_every_scheme() {
        let dns = parse_target_uri("dns:///example.com:443").expect("dns");
        assert_eq!(dns.scheme, "dns");
        assert_eq!(dns.authority(), "example.com:443");
        let pass = parse_target_uri("passthrough:///127.0.0.1:50051").expect("passthrough");
        assert_eq!(pass.authority(), "127.0.0.1:50051");
        // Single-colon spellings work too.
        assert!(parse_target_uri("dns:example.com:443").is_ok());
        assert!(parse_target_uri("passthrough:example.com:443").is_ok());
        // Schemes are case-insensitive.
        assert_eq!(
            parse_target_uri("DNS:///example.com:443")
                .expect("upper")
                .scheme,
            "dns"
        );
        let v4 = parse_target_uri("ipv4:127.0.0.1:80,10.0.0.1:80").expect("ipv4");
        assert_eq!(v4.authority(), "127.0.0.1:80");
        let v6 = parse_target_uri("ipv6:[::1]:80").expect("ipv6");
        assert_eq!(v6.authority(), "[::1]:80");
        let unix = parse_target_uri("unix:/run/grpc.sock").expect("unix");
        assert_eq!(unix.authority(), "localhost");
        let abs = parse_target_uri("unix-abstract:grpc").expect("abstract");
        assert_eq!(abs.authority(), "localhost");
    }

    #[test]
    fn rejects_plain_host_port() {
        let err = parse_target_uri("example.com:443").expect_err("not a URI");
        assert!(err.message().contains("host:port"));
        assert!(parse_target_uri("/no/scheme").is_err());
        assert!(parse_target_uri(":bare").is_err());
    }

    #[test]
    fn rejects_bad_inputs_per_scheme() {
        assert!(parse_target_uri("grpc://example.com:443").is_err());
        assert!(parse_target_uri("xds:///cluster").is_err());
        assert!(parse_target_uri("dns:///example.com").is_err());
        assert!(parse_target_uri("dns:///example.com:0").is_err());
        assert!(parse_target_uri("dns:///:443").is_err());
        assert!(parse_target_uri("dns:///example.com:http").is_err());
        assert!(parse_target_uri("passthrough:///").is_err());
        assert!(parse_target_uri("ipv4:example.com:80").is_err());
        assert!(parse_target_uri("ipv4:::1").is_err());
        assert!(parse_target_uri("ipv4:").is_err());
        assert!(parse_target_uri("ipv4:::80").is_err());
        assert!(parse_target_uri("ipv6:127.0.0.1:80").is_err());
        assert!(parse_target_uri("ipv6:").is_err());
        assert!(parse_target_uri("unix:").is_err());
        assert!(parse_target_uri("unix:///").is_err());
        assert!(parse_target_uri("unix-abstract:").is_err());
    }
}
