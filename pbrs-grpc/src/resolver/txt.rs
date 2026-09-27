//! DNS TXT fetching for A2 service configs.
//!
//! [`SystemTxt`] speaks just enough DNS-over-UDP to fetch one TXT
//! record set: a single query, a 2s timeout, no retries, no TCP
//! fallback (truncated answers fail). Responses are parsed with
//! bounds-checked reads only; malformed packets are errors, never
//! panics. Tests inject [`TxtLookup`] fakes or point [`SystemTxt`] at
//! a loopback script.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

/// One TXT record-set lookup.
pub trait TxtLookup: Send + Sync + 'static {
    /// Fetch TXT strings for `name` (e.g. `_grpc_config.example.com`).
    /// Character-strings within a record are concatenated; one entry
    /// per record. Empty means NOERROR with no TXT records.
    fn fetch_txt(
        &self,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, io::Error>> + Send + '_>>;
}

/// Production TXT via UDP to a nameserver.
#[derive(Clone, Debug)]
pub struct SystemTxt {
    nameserver: SocketAddr,
}

/// Query timeout and only attempt budget: one datagram, one wait.
const TXT_TIMEOUT: Duration = Duration::from_secs(2);

impl SystemTxt {
    /// Use the first `nameserver` in `/etc/resolv.conf` (port 53).
    pub async fn new() -> Result<Self, io::Error> {
        let conf = tokio::fs::read_to_string("/etc/resolv.conf")
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("cannot read /etc/resolv.conf: {e}"),
                )
            })?;
        for line in conf.lines() {
            let mut words = line.split_whitespace();
            if words.next() == Some("nameserver") {
                if let Some(ip) = words.next().and_then(|s| s.parse().ok()) {
                    return Ok(Self {
                        nameserver: SocketAddr::new(ip, 53),
                    });
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no nameserver in /etc/resolv.conf",
        ))
    }

    /// Use an explicit nameserver (tests and custom DNS stacks).
    #[must_use]
    pub fn nameserver(addr: SocketAddr) -> Self {
        Self { nameserver: addr }
    }
}

impl TxtLookup for SystemTxt {
    fn fetch_txt(
        &self,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, io::Error>> + Send + '_>> {
        let nameserver = self.nameserver;
        let name = name.to_owned();
        Box::pin(async move {
            let (id, query) = encode_query(&name)?;
            let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            sock.connect(nameserver).await?;
            sock.send(&query).await?;
            let mut buf = [0u8; 512];
            let len = tokio::time::timeout(TXT_TIMEOUT, sock.recv(&mut buf))
                .await
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "dns txt query timed out")
                })??;
            let datagram = buf
                .get(..len)
                .ok_or_else(|| invalid("short datagram"))?;
            parse_response(datagram, id)
        })
    }
}

/// Query `_grpc_config.<host>` for service-config TXT (A2).
#[must_use]
pub fn grpc_config_name(host: &str) -> String {
    format!("_grpc_config.{host}")
}

fn encode_query(name: &str) -> Result<(u16, Vec<u8>), io::Error> {
    if name.is_empty() || name.len() > 253 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("bad dns name {name:?}"),
        ));
    }
    let mixed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        ^ std::process::id();
    let id = u16::try_from(mixed & 0xFFFF).unwrap_or(0);
    let mut out = Vec::with_capacity(32 + name.len());
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x00]); // RD, opcode 0
    out.extend_from_slice(&[0x00, 0x01]); // QDCOUNT 1
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad dns label in {name:?}"),
            ));
        }
        let len = u8::try_from(label.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad dns label in {name:?}"),
            )
        })?;
        out.push(len);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&16u16.to_be_bytes()); // TXT
    out.extend_from_slice(&1u16.to_be_bytes()); // IN
    Ok((id, out))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Parse a DNS response, collecting every TXT record's strings.
fn parse_response(buf: &[u8], want_id: u16) -> Result<Vec<String>, io::Error> {
    let header: &[u8; 12] = buf
        .get(..12)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| invalid("short dns header"))?;
    let id = u16::from_be_bytes([header[0], header[1]]);
    if id != want_id {
        return Err(invalid("dns id mismatch"));
    }
    if header[2] & 0x80 == 0 {
        return Err(invalid("not a dns response"));
    }
    if header[2] & 0x02 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated dns response (no tcp fallback)",
        ));
    }
    match header[3] & 0x0F {
        0 => {}
        3 => return Ok(Vec::new()), // NXDOMAIN: no config, not an error
        code => return Err(invalid(format!("dns rcode {code}"))),
    }
    let qd = u16::from_be_bytes([header[4], header[5]]);
    let an = u16::from_be_bytes([header[6], header[7]]);
    let mut off = 12usize;
    for _ in 0..qd {
        off = skip_name(buf, off)? + 4; // name + qtype + qclass
    }
    let mut txts = Vec::new();
    for _ in 0..an {
        off = skip_name(buf, off)?;
        let end = off.checked_add(10).ok_or_else(|| invalid("short rr"))?;
        let fixed: &[u8; 10] = buf
            .get(off..end)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| invalid("short rr"))?;
        let rtype = u16::from_be_bytes([fixed[0], fixed[1]]);
        let rdlen = usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
        let rdata_end = end.checked_add(rdlen).ok_or_else(|| invalid("short rdata"))?;
        let rdata = buf
            .get(end..rdata_end)
            .ok_or_else(|| invalid("short rdata"))?;
        if rtype == 16 {
            txts.push(parse_txt_rdata(rdata)?);
        }
        off = rdata_end;
    }
    Ok(txts)
}

/// Concatenate a TXT record's character-strings.
fn parse_txt_rdata(rdata: &[u8]) -> Result<String, io::Error> {
    let mut out = Vec::new();
    let mut rest = rdata;
    while let Some((len, tail)) = rest.split_first() {
        let len = usize::from(*len);
        let (head, remaining) = tail
            .split_at_checked(len)
            .ok_or_else(|| invalid("short txt string"))?;
        out.extend_from_slice(head);
        rest = remaining;
    }
    String::from_utf8(out).map_err(|_| invalid("non-utf8 txt"))
}

/// Skip a possibly compressed domain name, returning the offset past it.
fn skip_name(buf: &[u8], mut off: usize) -> Result<usize, io::Error> {
    let mut jumps = 0u8;
    let mut consumed = off;
    loop {
        if jumps > 64 {
            return Err(invalid("dns name pointer loop"));
        }
        let len = *buf.get(off).ok_or_else(|| invalid("short name"))?;
        if len & 0xC0 == 0xC0 {
            let second = *buf.get(off + 1).ok_or_else(|| invalid("short pointer"))?;
            let target = usize::from(u16::from_be_bytes([len, second]) & 0x3FFF);
            if target >= buf.len() {
                return Err(invalid("bad dns pointer"));
            }
            if jumps == 0 {
                consumed = off + 2;
            }
            off = target;
            jumps += 1;
        } else if len == 0 {
            return Ok(if jumps == 0 { off + 1 } else { consumed });
        } else {
            let len = usize::from(len);
            if buf.len() < off + 1 + len {
                return Err(invalid("short label"));
            }
            off += 1 + len;
            if jumps == 0 {
                consumed = off;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TXT_TIMEOUT, TxtLookup, encode_query, grpc_config_name, parse_response};
    use std::net::SocketAddr;
    use std::time::Duration;

    #[test]
    fn config_name_and_query_shape() {
        assert_eq!(grpc_config_name("example.com"), "_grpc_config.example.com");
        let (_, query) = encode_query("_grpc_config.example.com").expect("query");
        assert_eq!(u16::from_be_bytes([query[2], query[3]]), 0x0100);
        assert!(encode_query("").is_err());
        assert!(encode_query(&"x".repeat(300)).is_err());
    }

    /// Minimal response builder: one question echoed, given answers.
    fn response(id: u16, answers: &[u8], an: u8, rcode: u8) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&[0x81, 0x80 | rcode]);
        out.extend_from_slice(&[0x00, 0x01, 0x00, an, 0x00, 0x00, 0x00, 0x00]);
        // Question: _grpc_config.x.test TXT IN (name never inspected).
        for label in ["_grpc_config", "x", "test"] {
            out.push(label.len() as u8);
            out.extend_from_slice(label.as_bytes());
        }
        out.extend_from_slice(&[0x00, 0x00, 0x10, 0x00, 0x01]);
        out.extend_from_slice(answers);
        out
    }

    fn txt_answer(txt: &[u8]) -> Vec<u8> {
        let mut out = vec![0xC0, 0x0C]; // pointer to question name
        out.extend_from_slice(&[0x00, 0x10, 0x00, 0x01]); // TXT IN
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]); // TTL 60
        out.extend_from_slice(&u16::try_from(txt.len()).expect("short").to_be_bytes());
        out.extend_from_slice(txt);
        out
    }

    #[test]
    fn parses_txt_and_concatenates_strings() {
        // Two character-strings in one record concatenate.
        let mut rdata = vec![5];
        rdata.extend_from_slice(b"hello");
        rdata.push(6);
        rdata.extend_from_slice(b" world");
        let packet = response(7, &txt_answer(&rdata), 1, 0);
        assert_eq!(
            parse_response(&packet, 7).expect("txt"),
            vec!["hello world"]
        );
    }

    #[test]
    fn nxdomain_is_empty_not_error() {
        let packet = response(9, &[], 0, 3);
        assert_eq!(
            parse_response(&packet, 9).expect("nx"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn hostile_packets_are_errors() {
        assert!(parse_response(&[], 0).is_err());
        assert!(parse_response(&[0u8; 11], 0).is_err());
        // ID mismatch.
        let packet = response(1, &[], 0, 0);
        assert!(parse_response(&packet, 2).is_err());
        // Truncated bit.
        let mut packet = response(1, &[], 0, 0);
        packet[2] |= 0x02;
        assert!(parse_response(&packet, 1).is_err());
        // Server failure.
        let packet = response(1, &[], 0, 2);
        assert!(parse_response(&packet, 1).is_err());
        // Short RDATA claim.
        let bad = txt_answer(&[200]);
        let packet = response(1, &bad, 1, 0);
        assert!(parse_response(&packet, 1).is_err());
        // Non-UTF8 TXT.
        let bad = txt_answer(&[2, 0xFF, 0xFE]);
        let packet = response(1, &bad, 1, 0);
        assert!(parse_response(&packet, 1).is_err());
    }

    #[test]
    fn pointer_loop_terminates() {
        // Answer owner name points at itself: header (12) + question
        // (13 + 2 + 5 + 1 + 4 = 25) = 37.
        let mut answers = vec![0xC0, 37];
        answers.extend_from_slice(&[0x00, 0x10, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        let packet = response(1, &answers, 1, 0);
        assert!(parse_response(&packet, 1).is_err());
    }

    #[tokio::test]
    async fn loopback_fetch_round_trip() {
        // Fake nameserver: answer every query with one TXT record.
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = server.local_addr().expect("addr");
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (len, peer) = server.recv_from(&mut buf).await.expect("recv");
            let id = u16::from_be_bytes([buf[0], buf[1]]);
            let _ = len;
            let mut rdata = vec![4];
            rdata.extend_from_slice(b"grpc");
            let packet = response(id, &txt_answer(&rdata), 1, 0);
            server.send_to(&packet, peer).await.expect("send");
        });
        let txt = super::SystemTxt::nameserver(SocketAddr::new(
            "127.0.0.1".parse().expect("ip"),
            addr.port(),
        ));
        let records = txt.fetch_txt("_grpc_config.x.test").await.expect("fetch");
        assert_eq!(records, vec!["grpc"]);
        assert!(TXT_TIMEOUT >= Duration::from_secs(1));
    }
}
