//! Peer identities for principal matching (gRFC A43 §Details).
//!
//! With a client certificate, principals match against URI SANs, then DNS
//! SANs, then the subject. The kernel does not parse X.509 anywhere else,
//! so this module carries a minimal DER reader that extracts exactly those
//! three facts from the leaf certificate. Anything unparseable fails
//! closed: no identities, so only rules without principals can match.

/// Authenticated peer facts a [`Policy`](super::Policy) matches against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerPrincipals {
    /// URI SANs from the leaf certificate, in certificate order.
    pub(crate) uri_sans: Vec<String>,
    /// DNS SANs from the leaf certificate, in certificate order.
    pub(crate) dns_sans: Vec<String>,
    /// Rendered subject (`CN=a,O=b`), when the subject held any attribute.
    pub(crate) subject: Option<String>,
    /// Whether the transport was TLS at all.
    pub(crate) is_tls: bool,
    /// Whether the peer presented a client certificate.
    pub(crate) has_cert: bool,
}

impl PeerPrincipals {
    /// Plaintext (or otherwise non-TLS) peer: no principal matches, not
    /// even `""` — A43 step 1, "if TLS is not used, matching fails".
    #[must_use]
    pub fn plaintext() -> Self {
        Self::default()
    }

    /// TLS peer without a client certificate: only the `""` principal
    /// matches (A43 step 2).
    #[must_use]
    pub fn tls_no_cert() -> Self {
        Self {
            is_tls: true,
            ..Self::default()
        }
    }

    /// TLS peer with the given leaf DER certificate.
    #[must_use]
    pub fn from_leaf_der(leaf_der: &[u8]) -> Self {
        let mut out = Self {
            is_tls: true,
            has_cert: true,
            ..Self::default()
        };
        if let Some(info) = parse_leaf(leaf_der) {
            out.uri_sans = info.uri_sans;
            out.dns_sans = info.dns_sans;
            out.subject = info.subject;
        }
        out
    }

    /// Identities to match principals against, in A43 priority order:
    /// URI SANs, else DNS SANs, else the subject. Empty when there is no
    /// certificate or nothing parseable was found.
    pub(crate) fn identities(&self) -> Vec<&str> {
        if !self.uri_sans.is_empty() {
            return self.uri_sans.iter().map(String::as_str).collect();
        }
        if !self.dns_sans.is_empty() {
            return self.dns_sans.iter().map(String::as_str).collect();
        }
        self.subject.as_deref().into_iter().collect()
    }
}

struct LeafInfo {
    uri_sans: Vec<String>,
    dns_sans: Vec<String>,
    subject: Option<String>,
}

fn parse_leaf(der: &[u8]) -> Option<LeafInfo> {
    let mut outer = Der::new(der);
    let cert_body = outer.sequence()?;
    let mut cert = Der::new(cert_body);
    let tbs_body = cert.sequence()?;
    // tbsCertificate: [0] version?, serial, signature, issuer, validity, subject, ...
    let mut tbs = Der::new(tbs_body);
    tbs.skip_optional_explicit0()?;
    tbs.skip_any()?; // serialNumber
    tbs.skip_any()?; // signature
    tbs.skip_any()?; // issuer
    tbs.skip_any()?; // validity
    let subject_der = tbs.any_raw()?;
    let subject = render_subject(subject_der);
    // subjectPublicKeyInfo, then optional [1] issuerUniqueID, [2]
    // subjectUniqueID, [3] extensions.
    tbs.skip_any()?;
    let mut uri_sans = Vec::new();
    let mut dns_sans = Vec::new();
    while let Some((class, num, body)) = tbs.context_specific() {
        if class == 2 && num == 3 {
            parse_extensions(body, &mut uri_sans, &mut dns_sans);
        }
    }
    Some(LeafInfo {
        uri_sans,
        dns_sans,
        subject,
    })
}

/// Short names for the attribute OIDs Go's `pkix.Name.String` renders.
fn attr_short_name(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x55, 0x04, 0x03] => Some("CN"),
        [0x55, 0x04, 0x04] => Some("SN"),
        [0x55, 0x04, 0x05] => Some("SERIALNUMBER"),
        [0x55, 0x04, 0x06] => Some("C"),
        [0x55, 0x04, 0x07] => Some("L"),
        [0x55, 0x04, 0x08] => Some("ST"),
        [0x55, 0x04, 0x09] => Some("STREET"),
        [0x55, 0x04, 0x0A] => Some("O"),
        [0x55, 0x04, 0x0B] => Some("OU"),
        [0x55, 0x04, 0x0C] => Some("TITLE"),
        [0x55, 0x04, 0x0D] => Some("DNQUALIFIER"),
        [0x55, 0x04, 0x11] => Some("POSTALCODE"),
        [0x55, 0x04, 0x2A] => Some("GN"),
        _ => None,
    }
}

/// Render a Name body (the RDNSequence contents) as comma-joined
/// `SHORT=value` pairs in DER order.
fn render_subject(body: &[u8]) -> Option<String> {
    let mut parts = Vec::new();
    let mut rdns = Der::new(body);
    while let Some(set) = rdns.set() {
        let mut atvs = Der::new(set);
        while let Some(seq) = atvs.sequence() {
            let mut atv = Der::new(seq);
            let oid = atv.oid()?;
            let value = atv.any_string()?;
            let short = attr_short_name(oid).unwrap_or("?");
            parts.push(format!("{short}={value}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Collect URI (tag 6) and DNS (tag 2) SANs from the extensions block.
fn parse_extensions(der: &[u8], uris: &mut Vec<String>, dns: &mut Vec<String>) {
    let mut outer = Der::new(der);
    let Some(list) = outer.sequence() else { return };
    let mut exts = Der::new(list);
    while let Some(seq) = exts.sequence() {
        let mut ext = Der::new(seq);
        let Some(oid) = ext.oid() else { continue };
        // subjectAltName is 2.5.29.17.
        if oid != [0x55, 0x1D, 0x11] {
            continue;
        }
        // Optional critical BOOLEAN, then the OCTET STRING value.
        let Some(value) = ext.optional_bool_then_octets() else {
            continue;
        };
        let mut names = Der::new(value);
        let Some(seq) = names.sequence() else {
            continue;
        };
        let mut general_names = Der::new(seq);
        while let Some((class, tag, body)) = general_names.context_primitive() {
            if class != 2 {
                continue;
            }
            let Ok(text) = core::str::from_utf8(body) else {
                continue;
            };
            if tag == 6 {
                uris.push(text.to_owned());
            } else if tag == 2 {
                dns.push(text.to_owned());
            }
        }
    }
}

/// Minimal DER cursor. Every method fails closed with `None`.
struct Der<'a> {
    buf: &'a [u8],
}

impl<'a> Der<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn read_byte(&mut self) -> Option<u8> {
        let (first, rest) = self.buf.split_first()?;
        self.buf = rest;
        Some(*first)
    }

    fn read_len(&mut self) -> Option<usize> {
        let first = self.read_byte()?;
        if first & 0x80 == 0 {
            return Some(usize::from(first));
        }
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 {
            return None;
        }
        if self.buf.len() < count {
            return None;
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = (len << 8) | usize::from(self.read_byte()?);
        }
        Some(len)
    }

    fn read_tlv(&mut self) -> Option<(u8, &'a [u8])> {
        let tag = self.read_byte()?;
        let len = self.read_len()?;
        if self.buf.len() < len {
            return None;
        }
        let (body, rest) = self.buf.split_at(len);
        self.buf = rest;
        Some((tag, body))
    }

    fn expect_tag(&mut self, tag: u8) -> Option<&'a [u8]> {
        let (got, body) = self.read_tlv()?;
        if got != tag { None } else { Some(body) }
    }

    fn sequence(&mut self) -> Option<&'a [u8]> {
        self.expect_tag(0x30)
    }

    fn set(&mut self) -> Option<&'a [u8]> {
        if self.is_empty() {
            return None;
        }
        self.expect_tag(0x31)
    }

    fn oid(&mut self) -> Option<&'a [u8]> {
        self.expect_tag(0x06)
    }

    /// Any one TLV, returned as the raw body regardless of tag.
    fn any_raw(&mut self) -> Option<&'a [u8]> {
        self.read_tlv().map(|(_, body)| body)
    }

    /// Skip one TLV.
    fn skip_any(&mut self) -> Option<()> {
        self.read_tlv().map(|_| ())
    }

    /// Skip `[0]` EXPLICIT (version), when present.
    fn skip_optional_explicit0(&mut self) -> Option<()> {
        if self.buf.first() == Some(&0xA0) {
            self.skip_any()?;
        }
        Some(())
    }

    /// Next `[n]` element (constructed context-specific), if any remain.
    fn context_specific(&mut self) -> Option<(u8, u8, &'a [u8])> {
        if self.is_empty() {
            return None;
        }
        let (tag, body) = self.read_tlv()?;
        if tag & 0xC0 != 0x80 {
            return None;
        }
        Some(((tag >> 6) & 3, tag & 0x1F, body))
    }

    /// Next context-specific primitive (GeneralName choice), if any remain.
    fn context_primitive(&mut self) -> Option<(u8, u8, &'a [u8])> {
        if self.is_empty() {
            return None;
        }
        let (tag, body) = self.read_tlv()?;
        if tag & 0xC0 != 0x80 || tag & 0x20 != 0 {
            return None;
        }
        Some(((tag >> 6) & 3, tag & 0x1F, body))
    }

    /// PrintableString / UTF8String / TeletexString / IA5String / BMPString.
    fn any_string(&mut self) -> Option<String> {
        let (tag, body) = self.read_tlv()?;
        match tag {
            0x0C | 0x13 | 0x14 | 0x16 => core::str::from_utf8(body).ok().map(str::to_owned),
            0x1E => {
                if body.len() % 2 != 0 {
                    return None;
                }
                let units: Vec<u16> = body
                    .chunks_exact(2)
                    .filter_map(|c| <[u8; 2]>::try_from(c).ok())
                    .map(u16::from_be_bytes)
                    .collect();
                String::from_utf16(&units).ok()
            }
            _ => None,
        }
    }

    /// The extension value: optional BOOLEAN critical, then OCTET STRING.
    fn optional_bool_then_octets(&mut self) -> Option<&'a [u8]> {
        if self.buf.first() == Some(&0x01) {
            self.skip_any()?;
        }
        self.expect_tag(0x04)
    }
}

#[cfg(test)]
mod tests {
    use super::PeerPrincipals;

    const CLIENT_PEM: &str = include_str!("../../tests/tls_data/client.crt");

    fn cert_der(pem: &str) -> Vec<u8> {
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        // Minimal base64 decode (test-only; certs are small and valid).
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0u8;
        for c in body.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => break,
                _ => continue,
            };
            acc = (acc << 6) | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(u8::try_from((acc >> bits) & 0xFF).expect("masked byte"));
            }
        }
        out
    }

    #[test]
    fn extracts_dns_san_and_subject() {
        let der = cert_der(CLIENT_PEM);
        let peer = PeerPrincipals::from_leaf_der(&der);
        assert!(peer.is_tls && peer.has_cert);
        assert!(peer.uri_sans.is_empty());
        assert_eq!(peer.dns_sans, vec!["pbrs-grpc-test-client".to_owned()]);
        assert_eq!(peer.subject.as_deref(), Some("CN=pbrs-grpc-test-client"));
        assert_eq!(peer.identities(), vec!["pbrs-grpc-test-client"]);
    }

    #[test]
    fn garbage_fails_closed() {
        let peer = PeerPrincipals::from_leaf_der(b"not a certificate");
        assert!(peer.is_tls && peer.has_cert);
        assert!(peer.identities().is_empty());
    }
}
