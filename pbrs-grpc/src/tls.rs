//! TLS for the kernel: rustls over Graviola, ALPN `h2`, no C compiler.
//!
//! Certificate verification is not optional. There is no "insecure" constructor.
//! Trust either Mozilla's WebPKI roots or a CA you pass in.
//!
//! Hardening (GF-06): pinned-CA constructors accept CRLs for revocation
//! checking (A69) and SPIFFE IDs as an additional leaf constraint (A87).
//! Both only ever reject certificates the WebPKI verifier already accepted;
//! neither creates a skip-verify path. [`ServerTls::with_handshake_observer`]
//! and [`ClientTls::with_handshake_observer`] report per-handshake TLS and
//! TCP telemetry (A118, A80), and
//! [`post_quantum_key_exchange_available`] tracks post-quantum key exchange
//! in the shipping provider (A120).

use crate::status::Status;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier as RustlsServerCertVerifier,
};
use rustls::pki_types::{
    CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName, UnixTime,
};
use rustls::server::WebPkiClientVerifier;
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier as RustlsClientCertVerifier};
use rustls::{
    CertificateError, ClientConfig as RustlsClientConfig, DigitallySignedStruct, DistinguishedName,
    Error as RustlsError, HandshakeKind, KeyLogFile, NamedGroup, ProtocolVersion, RootCertStore,
    ServerConfig as RustlsServerConfig, SignatureScheme,
};
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::TlsConnector;

const ALPN_H2: &[u8] = b"h2";

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls_graviola::default_provider())
}

fn certs_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, Status> {
    let certs: Result<Vec<_>, _> = rustls_pemfile::certs(&mut &*pem).collect();
    let certs = certs.map_err(|e| Status::invalid_argument(format!("certificate PEM: {e}")))?;
    if certs.is_empty() {
        return Err(Status::invalid_argument(
            "certificate PEM contained no certificates",
        ));
    }
    Ok(certs)
}

fn key_from_pem(pem: &[u8]) -> Result<PrivateKeyDer<'static>, Status> {
    rustls_pemfile::private_key(&mut &*pem)
        .map_err(|e| Status::invalid_argument(format!("private key PEM: {e}")))?
        .ok_or_else(|| Status::invalid_argument("private key PEM contained no key"))
}

fn crls_from_pem(pem: &[u8]) -> Result<Vec<CertificateRevocationListDer<'static>>, Status> {
    let crls: Result<Vec<_>, _> = rustls_pemfile::crls(&mut &*pem).collect();
    let crls = crls.map_err(|e| Status::invalid_argument(format!("CRL PEM: {e}")))?;
    if crls.is_empty() {
        return Err(Status::invalid_argument(
            "CRL PEM contained no certificate revocation lists",
        ));
    }
    Ok(crls)
}

fn roots_from_certs(certs: Vec<CertificateDer<'static>>) -> Result<RootCertStore, Status> {
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots
            .add(cert)
            .map_err(|e| Status::invalid_argument(format!("trust anchor: {e}")))?;
    }
    if roots.is_empty() {
        return Err(Status::invalid_argument("no trust anchors"));
    }
    Ok(roots)
}

fn webpki_roots() -> RootCertStore {
    RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    }
}

#[cfg(feature = "native-roots")]
fn native_roots() -> Result<RootCertStore, Status> {
    let loaded = rustls_native_certs::load_native_certs();
    if !loaded.errors.is_empty() {
        return Err(Status::unavailable(format!(
            "native roots: {:?}",
            loaded.errors
        )));
    }
    roots_from_certs(loaded.certs)
}

fn server_name(name: &str) -> Result<ServerName<'static>, Status> {
    ServerName::try_from(name)
        .map_err(|_| Status::invalid_argument(format!("invalid TLS server name {name:?}")))
        .map(|n| n.to_owned())
}

fn require_h2(alpn: Option<&[u8]>) -> Result<(), Status> {
    if alpn == Some(ALPN_H2) {
        Ok(())
    } else {
        Err(Status::unauthenticated(
            "tls: peer did not negotiate ALPN h2",
        ))
    }
}

/// Map a rustls handshake I/O error onto a gRPC code.
///
/// A reset or abort while the peer is dropping the socket (accept-loop cap,
/// mute close) is [`crate::Code::Unavailable`], matching an h2c preface
/// close. [`std::io::ErrorKind::AddrNotAvailable`] is that same dropped-socket
/// set (unroutable bind), Distinct from leftover kinds which stay
/// [`crate::Code::Unauthenticated`]. Certificate and protocol failures stay
/// [`crate::Code::Unauthenticated`]. Distinct from [`crate::Status`]'s
/// `From<std::io::Error>`: that maps local I/O `InvalidData` to
/// [`crate::Code::Internal`]; this handshake maps `InvalidData` to
/// Unauthenticated.
fn tls_handshake_status(err: std::io::Error) -> Status {
    let status = match err.kind() {
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::UnexpectedEof
        | std::io::ErrorKind::AddrNotAvailable => {
            Status::unavailable(format!("tls handshake: {err}"))
        }
        _ => Status::unauthenticated(format!("tls handshake: {err}")),
    };
    status.with_cause(err)
}

/// A PEM certificate chain and private key.
pub struct Identity {
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

impl Identity {
    /// Parse a certificate chain and private key from PEM.
    ///
    /// The key may be PKCS#8 or SEC1 (EC). The chain is the leaf first.
    pub fn from_pem(cert_pem: impl AsRef<[u8]>, key_pem: impl AsRef<[u8]>) -> Result<Self, Status> {
        Ok(Self {
            certs: certs_from_pem(cert_pem.as_ref())?,
            key: key_from_pem(key_pem.as_ref())?,
        })
    }

    /// DER certificates in this identity, leaf first.
    pub fn certificates(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.certs.iter().map(|c| c.as_ref())
    }
}

/// Client certificate chain from a TLS handshake, DER-encoded, leaf first.
///
/// Present when the peer sent a certificate (mTLS), or when
/// [`crate::Incoming::peer`] supplies a chain via [`Self::from_der_certs`].
/// TLS without client authentication, h2c, Unix, the default
/// [`crate::Incoming`], and [`crate::Server::serve_connection`] yield `None`
/// from [`crate::Rpc::peer_identity`]. The kernel does not parse X.509; an
/// interceptor that needs a CN or SAN decodes the leaf itself. Applies to
/// every call shape.
///
/// ```
/// # use pbrs_grpc::PeerIdentity;
/// fn allow(id: &PeerIdentity, known_leaf: &[u8]) -> bool {
///     id.leaf() == Some(known_leaf)
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    certs: Arc<[Box<[u8]>]>,
}

impl fmt::Debug for PeerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerIdentity")
            .field("certificates", &self.certs.len())
            .finish()
    }
}

impl PeerIdentity {
    /// Construct from DER certificates, leaf first.
    ///
    /// Empty input yields `None` — the same representation as "no client
    /// certificate". An [`crate::Incoming`] implementor that already verified
    /// a TLS stack uses this; the kernel does not parse X.509.
    ///
    /// ```
    /// # use pbrs_grpc::PeerIdentity;
    /// assert!(PeerIdentity::from_der_certs(None::<&[u8]>).is_none());
    /// let id = PeerIdentity::from_der_certs([b"leaf"]).expect("leaf");
    /// assert_eq!(id.leaf(), Some(&b"leaf"[..]));
    /// ```
    #[must_use]
    pub fn from_der_certs<I>(certs: I) -> Option<Self>
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        let certs: Vec<Box<[u8]>> = certs
            .into_iter()
            .map(|c| Box::<[u8]>::from(c.as_ref()))
            .collect();
        if certs.is_empty() {
            None
        } else {
            Some(Self {
                certs: Arc::from(certs),
            })
        }
    }

    pub(crate) fn from_rustls(certs: &[CertificateDer<'_>]) -> Option<Self> {
        Self::from_der_certs(certs)
    }

    /// DER certificates, leaf first.
    pub fn certificates(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.certs.iter().map(|c| c.as_ref())
    }

    /// End-entity (leaf) certificate, DER.
    #[must_use]
    pub fn leaf(&self) -> Option<&[u8]> {
        self.certs.first().map(|c| c.as_ref())
    }

    /// Certificates in the chain, including the leaf.
    #[must_use]
    pub fn len(&self) -> usize {
        self.certs.len()
    }

    /// Always false: an empty chain is represented as `None`, not this type.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.certs.is_empty()
    }
}

pub(crate) fn peer_identity_of(
    stream: &tokio_rustls::server::TlsStream<TcpStream>,
) -> Option<PeerIdentity> {
    PeerIdentity::from_rustls(stream.get_ref().1.peer_certificates()?)
}

/// An expected SPIFFE ID (A87), e.g. `spiffe://example.org/server`.
///
/// Compared by exact string equality against the URI entries of the leaf
/// certificate's subject alternative names. There is no wildcard or
/// trust-domain-only match: the whole ID must be equal.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SpiffeId {
    id: String,
}

impl SpiffeId {
    /// Parse and validate `id`, rejecting anything not shaped like
    /// `spiffe://trust-domain[/path]` with [`Status::invalid_argument`].
    fn parse(id: &str) -> Result<Self, Status> {
        let invalid = || Status::invalid_argument(format!("invalid SPIFFE ID {id:?}"));
        let rest = id.strip_prefix("spiffe://").ok_or_else(invalid)?;
        let trust_domain = rest.split('/').next().unwrap_or_default();
        if trust_domain.is_empty()
            || id
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(invalid());
        }
        Ok(Self { id: id.to_string() })
    }

    fn as_str(&self) -> &str {
        &self.id
    }
}

/// OID 2.5.29.17 (subjectAltName) as DER value bytes.
const OID_SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1d, 0x11];

/// Read one DER tag-length-value, advancing `input` past it.
#[allow(
    clippy::explicit_auto_deref,
    reason = "copying the reference out preserves the 'a lifetime; auto-deref would reborrow for a shorter one"
)]
fn der_tlv<'a>(input: &mut &'a [u8]) -> Option<(u8, &'a [u8])> {
    let buf: &'a [u8] = *input;
    let (tag, rest) = buf.split_first()?;
    *input = rest;
    let len = der_length(input)?;
    let buf: &'a [u8] = *input;
    let (value, rest) = buf.split_at_checked(len)?;
    *input = rest;
    Some((*tag, value))
}

/// Read a DER length prefix, advancing `input` past it. Rejects the
/// indefinite form and lengths that do not fit in four octets.
#[allow(
    clippy::explicit_auto_deref,
    reason = "copying the reference out avoids borrow conflicts with the cursor update below"
)]
fn der_length(input: &mut &[u8]) -> Option<usize> {
    let buf: &[u8] = *input;
    let (first, rest) = buf.split_first()?;
    *input = rest;
    if *first < 0x80 {
        return Some(usize::from(*first));
    }
    let octets = usize::from(*first & 0x7f);
    if octets == 0 || octets > 4 {
        return None;
    }
    let buf: &[u8] = *input;
    let (digits, rest) = buf.split_at_checked(octets)?;
    *input = rest;
    let mut len = 0usize;
    for digit in digits {
        len = len.checked_shl(8)?.checked_add(usize::from(*digit))?;
    }
    Some(len)
}

/// URI entries of the leaf certificate's subject alternative names (A87).
///
/// Walks the DER structure positionally (certificate → TBSCertificate →
/// extensions → subjectAltName → general names) instead of scanning for
/// bytes, so a URI-looking byte string anywhere else in the certificate
/// cannot match. Any malformation yields an empty list, which fails the
/// SPIFFE check closed: the expected ID can never equal "no names".
fn uri_subject_alternative_names(leaf_der: &[u8]) -> Vec<&str> {
    fn inner(leaf_der: &[u8]) -> Option<Vec<&str>> {
        let mut outer = leaf_der;
        let (tag, certificate) = der_tlv(&mut outer)?;
        if tag != 0x30 || !outer.is_empty() {
            return None;
        }
        let mut cursor = certificate;
        let (tag, mut tbs) = der_tlv(&mut cursor)?;
        if tag != 0x30 {
            return None;
        }
        // TBSCertificate children in order: [0] version when explicit, then
        // serial, signature, issuer, validity, subject, subjectPublicKeyInfo,
        // then optional unique IDs and [3] extensions.
        if tbs.first() == Some(&0xa0) {
            der_tlv(&mut tbs)?; // version
        }
        for _ in 0..6 {
            der_tlv(&mut tbs)?;
        }
        let mut uris = Vec::new();
        while !tbs.is_empty() {
            let (tag, value) = der_tlv(&mut tbs)?;
            if tag != 0xa3 {
                continue; // issuer/subject unique IDs
            }
            // [3] EXPLICIT Extensions ::= SEQUENCE OF Extension.
            let mut extensions = value;
            let (tag, sequence) = der_tlv(&mut extensions)?;
            if tag != 0x30 || !extensions.is_empty() {
                return None;
            }
            let mut extensions = sequence;
            while !extensions.is_empty() {
                let (tag, extension) = der_tlv(&mut extensions)?;
                if tag != 0x30 {
                    return None;
                }
                let mut fields = extension;
                let (tag, oid) = der_tlv(&mut fields)?;
                if tag != 0x06 {
                    return None;
                }
                if fields.first() == Some(&0x01) {
                    der_tlv(&mut fields)?; // optional critical flag
                }
                let (tag, value) = der_tlv(&mut fields)?;
                if tag != 0x04 || !fields.is_empty() {
                    return None;
                }
                if oid == OID_SUBJECT_ALT_NAME {
                    let mut names = value;
                    let (tag, sequence) = der_tlv(&mut names)?;
                    if tag != 0x30 || !names.is_empty() {
                        return None;
                    }
                    let mut names = sequence;
                    while !names.is_empty() {
                        let (tag, name) = der_tlv(&mut names)?;
                        if tag == 0x86 {
                            // uniformResourceIdentifier.
                            if let Ok(uri) = std::str::from_utf8(name) {
                                uris.push(uri);
                            }
                        }
                    }
                }
            }
        }
        Some(uris)
    }
    inner(leaf_der).unwrap_or_default()
}

/// A WebPKI server verifier with an added SPIFFE ID constraint (A87).
///
/// Verification delegates to the inner WebPKI verifier first: chain, name,
/// expiry and (when configured) revocation checks all still apply. Only a
/// certificate that already verified is then required to carry the expected
/// SPIFFE ID in a leaf URI SAN. This wrapper can only reject more
/// certificates than the inner verifier, never fewer; it is not a
/// skip-verify path.
#[derive(Debug)]
struct SpiffeServerCertVerifier {
    inner: Arc<WebPkiServerVerifier>,
    expected: SpiffeId,
}

impl RustlsServerCertVerifier for SpiffeServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let verified = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        let uris = uri_subject_alternative_names(end_entity.as_ref());
        if uris.iter().any(|uri| *uri == self.expected.as_str()) {
            Ok(verified)
        } else {
            Err(RustlsError::InvalidCertificate(
                CertificateError::NotValidForName,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// A WebPKI client verifier with an added SPIFFE ID constraint (A87).
///
/// Same delegation contract as [`SpiffeServerCertVerifier`]: the inner
/// WebPKI checks run first and the SPIFFE constraint can only reject more.
#[derive(Debug)]
struct SpiffeClientCertVerifier {
    inner: Arc<dyn RustlsClientCertVerifier>,
    expected: SpiffeId,
}

impl RustlsClientCertVerifier for SpiffeClientCertVerifier {
    fn offer_client_auth(&self) -> bool {
        self.inner.offer_client_auth()
    }

    fn client_auth_mandatory(&self) -> bool {
        self.inner.client_auth_mandatory()
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, RustlsError> {
        let verified = self
            .inner
            .verify_client_cert(end_entity, intermediates, now)?;
        let uris = uri_subject_alternative_names(end_entity.as_ref());
        if uris.iter().any(|uri| *uri == self.expected.as_str()) {
            Ok(verified)
        } else {
            Err(RustlsError::InvalidCertificate(
                CertificateError::NotValidForName,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Per-connection TCP telemetry snapshot (A80).
///
/// Delivered inside [`TlsHandshakeInfo`] after every successful handshake
/// that has an observer installed. Kernel TCP_INFO (round-trip time,
/// congestion window, retransmits) is a recorded boundary: `socket2` 0.6
/// exposes no safe TCP_INFO API and this module is `forbid(unsafe_code)`,
/// so that enrichment needs an `unsafe` `getsockopt` helper outside this
/// module (follow-up).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TcpStats {
    /// Local socket address of the connection.
    pub local_addr: Option<SocketAddr>,
    /// Remote socket address of the connection.
    pub remote_addr: Option<SocketAddr>,
}

/// Per-handshake TLS telemetry snapshot (A118).
///
/// Delivered to the observer installed via
/// [`ServerTls::with_handshake_observer`] or
/// [`ClientTls::with_handshake_observer`] after every successful handshake.
/// Failed handshakes are not reported; the handshake error itself carries
/// the failure. Contains no key material and no certificate bytes, only
/// counts and negotiated parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsHandshakeInfo {
    /// Negotiated TLS version (`"TLSv1.3"`, `"TLSv1.2"`, or `"unknown"`).
    pub version: &'static str,
    /// Negotiated cipher suite (rustls suite name, e.g.
    /// `"TLS13_AES_128_GCM_SHA256"`).
    pub cipher_suite: String,
    /// Whether the handshake resumed an earlier session (TLS 1.3 PSK)
    /// instead of a full handshake.
    pub resumed: bool,
    /// Negotiated ALPN protocol. Always `Some("h2")`: peers that do not
    /// negotiate `h2` fail before any observation is delivered.
    pub alpn: Option<&'static str>,
    /// Number of certificates the peer presented (0 for anonymous TLS).
    pub peer_certificates: usize,
    /// Wall time from TCP handoff to a usable TLS stream, including the
    /// client's post-handshake alert drain.
    pub handshake_duration: Duration,
    /// TCP telemetry for the same connection (A80).
    pub tcp: TcpStats,
}

/// Invoked after every successful TLS handshake when installed via
/// [`ServerTls::with_handshake_observer`] or
/// [`ClientTls::with_handshake_observer`].
type HandshakeObserver = Arc<dyn Fn(&TlsHandshakeInfo) + Send + Sync>;

fn protocol_name(version: Option<ProtocolVersion>) -> &'static str {
    match version {
        Some(ProtocolVersion::TLSv1_3) => "TLSv1.3",
        Some(ProtocolVersion::TLSv1_2) => "TLSv1.2",
        _ => "unknown",
    }
}

/// Whether the shipping TLS provider negotiates post-quantum key exchange
/// (A120).
///
/// This reads the live provider key-exchange groups, so it tracks provider
/// upgrades: it returns `true` exactly when the provider offers a pure
/// ML-KEM or hybrid ML-KEM group. On the pinned `rustls-graviola` 0.2.1
/// (X25519, P-256, P-384 only) it returns `false`.
///
/// Re-evaluate on provider bumps: `rustls-graviola` 0.4 ships pure-Rust
/// `X25519MLKEM768`, but needs `graviola` 0.4 (`rust-version` 1.89), above
/// this crate's MSRV of 1.85.
#[allow(
    dead_code,
    reason = "GF-06: only called by in-module tests until the lib.rs export lands; the export silences this"
)]
#[must_use]
pub fn post_quantum_key_exchange_available() -> bool {
    provider().kx_groups.iter().any(|group| {
        matches!(
            group.name(),
            NamedGroup::MLKEM512
                | NamedGroup::MLKEM768
                | NamedGroup::MLKEM1024
                | NamedGroup::secp256r1MLKEM768
                | NamedGroup::X25519MLKEM768
        )
    })
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("certs", &self.certs.len())
            .finish_non_exhaustive()
    }
}

/// Server-side TLS: a rustls acceptor with ALPN `h2` and TLS 1.3 session
/// tickets, so repeat clients resume instead of re-doing a full handshake.
/// Ticket keys are per-`ServerTls` and rotate every 6 h; restarting the
/// server (or building a second `ServerTls`) invalidates outstanding tickets.
///
/// There is no tonic `ServerTlsConfig::timeout`: that is a TLS-handshake-only
/// timeout on the tonic acceptor. This type has no timeout setter; the bound is
/// [`crate::ServerConfig::handshake_timeout`] (20 s TLS accept and 20 s HTTP/2
/// preface, separately). Distinct from grpc-go `ConnectionTimeout` (one 120 s
/// deadline covering both). Distinct from [`crate::ChannelConfig::connect_timeout`]
/// (client whole dial). Distinct from tonic `ClientTlsConfig::timeout` (client
/// TLS handshake). Distinct from [`crate::ServerConfig::timeout`] (RPC deadline
/// overlay).
///
/// ```no_run
/// # use pbrs_grpc::{Identity, Rpc, Server, ServerTls, Service};
/// # struct Echo;
/// # impl Service for Echo {
/// #     const NAME: &'static str = "demo.Echo";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # async fn example(cert_pem: &[u8], key_pem: &[u8]) -> Result<(), pbrs_grpc::Status> {
/// let identity = Identity::from_pem(cert_pem, key_pem)?;
/// Server::new(Echo)
///     .serve_tls(
///         "0.0.0.0:443".parse().expect("addr"),
///         ServerTls::new(identity)?,
///     )
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ServerTls {
    config: Arc<RustlsServerConfig>,
    acceptor: TlsAcceptor,
    observer: Option<HandshakeObserver>,
}

impl fmt::Debug for ServerTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerTls").finish_non_exhaustive()
    }
}

impl ServerTls {
    /// Serve with `identity`. Clients are not asked for a certificate.
    ///
    /// This constructor does not enable rustls key logging; call
    /// [`Self::key_log_file`] to opt into `SSLKEYLOGFILE` for local debugging.
    /// Distinct from tonic `ClientTlsConfig::use_key_log` (client handshake).
    /// Distinct from [`Self::mtls`] (client cert require) and
    /// [`Self::optional_mtls`] (client cert requested but optional). Distinct
    /// from a skip-verify constructor (there is none).
    pub fn new(identity: Identity) -> Result<Self, Status> {
        build_server(identity, ClientAuth::None, Vec::new(), None)
    }

    /// Serve with `identity` and require a client certificate issued by `client_ca_pem`.
    ///
    /// Use [`Self::optional_mtls`] for tonic-style optional client auth. This
    /// constructor always requires a client certificate issued by that CA.
    /// Distinct from [`Self::new`] (clients are not asked). Distinct from a
    /// skip-verify constructor (there is none). Distinct from
    /// [`ClientTls::ca_mtls`] / [`ClientTls::webpki_mtls`] (client presents;
    /// this is the server require).
    pub fn mtls(identity: Identity, client_ca_pem: impl AsRef<[u8]>) -> Result<Self, Status> {
        let cas = roots_from_certs(certs_from_pem(client_ca_pem.as_ref())?)?;
        build_server(identity, ClientAuth::Required(cas), Vec::new(), None)
    }

    /// Serve with `identity`, require a client certificate issued by
    /// `client_ca_pem`, and check revocation against the CRLs in `crl_pem`
    /// (A69).
    ///
    /// Every certificate in the presented chain except the trust anchor must
    /// have a determinable, non-revoked status in the CRLs. Unknown status
    /// (no covering CRL) and expired CRLs fail the handshake: revocation
    /// checking fails closed. CRL freshness is the operator's job; reload by
    /// rebuilding this value. Distinct from [`Self::mtls`] (no revocation
    /// check). Distinct from a skip-verify constructor (there is none).
    pub fn mtls_with_crl(
        identity: Identity,
        client_ca_pem: impl AsRef<[u8]>,
        crl_pem: impl AsRef<[u8]>,
    ) -> Result<Self, Status> {
        let cas = roots_from_certs(certs_from_pem(client_ca_pem.as_ref())?)?;
        let crls = crls_from_pem(crl_pem.as_ref())?;
        build_server(identity, ClientAuth::Required(cas), crls, None)
    }

    /// Serve with `identity`, require a client certificate issued by
    /// `client_ca_pem`, and constrain the leaf to the SPIFFE ID `spiffe_id`
    /// (A87).
    ///
    /// The WebPKI checks run first; a leaf that already verified must then
    /// carry exactly `spiffe_id` (e.g. `spiffe://example.org/client`) in a
    /// URI subject alternative name. There is no wildcard or
    /// trust-domain-only match. A malformed expected ID is
    /// [`crate::Code::InvalidArgument`]. Distinct from [`Self::mtls`] (no
    /// SPIFFE constraint). Distinct from a skip-verify constructor (there is
    /// none).
    pub fn mtls_spiffe(
        identity: Identity,
        client_ca_pem: impl AsRef<[u8]>,
        spiffe_id: &str,
    ) -> Result<Self, Status> {
        let cas = roots_from_certs(certs_from_pem(client_ca_pem.as_ref())?)?;
        let spiffe = SpiffeId::parse(spiffe_id)?;
        build_server(
            identity,
            ClientAuth::Required(cas),
            Vec::new(),
            Some(spiffe),
        )
    }

    /// Serve with `identity` and request, but do not require, a client certificate.
    ///
    /// If the peer presents a certificate issued by `client_ca_pem`, the verified
    /// chain is available from [`crate::Rpc::peer_identity`]. If the peer sends
    /// no certificate, the handshake still succeeds and `peer_identity` is
    /// `None`. Verification is still mandatory for any certificate the client
    /// does send; unknown CAs fail the handshake. Distinct from [`Self::mtls`]
    /// (client certificate required) and [`Self::new`] (client certificate not
    /// requested).
    pub fn optional_mtls(
        identity: Identity,
        client_ca_pem: impl AsRef<[u8]>,
    ) -> Result<Self, Status> {
        let cas = roots_from_certs(certs_from_pem(client_ca_pem.as_ref())?)?;
        build_server(identity, ClientAuth::Optional(cas), Vec::new(), None)
    }

    /// Observe every accepted handshake with `TlsHandshakeInfo` (A118/A80).
    ///
    /// The observer runs on the accepting task after ALPN negotiation, so it
    /// must be cheap and must not block. Without an observer nothing is
    /// recorded. Failed handshakes are never reported. Clones share the
    /// observer.
    #[must_use]
    pub fn with_handshake_observer(
        mut self,
        observer: impl Fn(&TlsHandshakeInfo) + Send + Sync + 'static,
    ) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    /// Enable rustls NSS-format key logging through `SSLKEYLOGFILE`.
    ///
    /// This is for local packet-decryption diagnostics only. It does not change
    /// certificate verification, does not create a skip-verify path, and writes
    /// nothing unless `SSLKEYLOGFILE` is set in the process environment.
    #[must_use]
    pub fn key_log_file(mut self) -> Self {
        Arc::make_mut(&mut self.config).key_log = Arc::new(KeyLogFile::new());
        self.acceptor = TlsAcceptor::from(Arc::clone(&self.config));
        self
    }

    pub(crate) async fn accept(
        &self,
        tcp: TcpStream,
    ) -> Result<tokio_rustls::server::TlsStream<TcpStream>, std::io::Error> {
        let started = Instant::now();
        let stream = self.acceptor.accept(tcp).await?;
        if stream.get_ref().1.alpn_protocol() == Some(ALPN_H2) {
            if let Some(observer) = self.observer.as_ref() {
                let (tcp, conn) = stream.get_ref();
                observer(&TlsHandshakeInfo {
                    version: protocol_name(conn.protocol_version()),
                    cipher_suite: conn.negotiated_cipher_suite().map_or_else(
                        || "none".to_string(),
                        |suite| format!("{:?}", suite.suite()),
                    ),
                    resumed: matches!(conn.handshake_kind(), Some(HandshakeKind::Resumed)),
                    alpn: conn
                        .alpn_protocol()
                        .and_then(|proto| (proto == ALPN_H2).then_some("h2")),
                    peer_certificates: conn.peer_certificates().map_or(0, |certs| certs.len()),
                    handshake_duration: started.elapsed(),
                    tcp: TcpStats {
                        local_addr: tcp.local_addr().ok(),
                        remote_addr: tcp.peer_addr().ok(),
                    },
                });
            }
            Ok(stream)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "peer did not negotiate ALPN h2",
            ))
        }
    }
}

enum ClientAuth {
    None,
    Required(RootCertStore),
    Optional(RootCertStore),
}

fn build_server(
    identity: Identity,
    client_auth: ClientAuth,
    crls: Vec<CertificateRevocationListDer<'static>>,
    spiffe: Option<SpiffeId>,
) -> Result<ServerTls, Status> {
    let provider = provider();
    let builder = RustlsServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| Status::internal(format!("tls versions: {e}")))?;
    let optional_client_auth = matches!(&client_auth, ClientAuth::Optional(_));
    let mut config = match client_auth {
        ClientAuth::None => builder
            .with_no_client_auth()
            .with_single_cert(identity.certs, identity.key)
            .map_err(|e| Status::invalid_argument(format!("server certificate: {e}")))?,
        ClientAuth::Required(cas) | ClientAuth::Optional(cas) => {
            let mut verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(cas), provider);
            if optional_client_auth {
                verifier = verifier.allow_unauthenticated();
            }
            if !crls.is_empty() {
                verifier = verifier.with_crls(crls).enforce_revocation_expiration();
            }
            let inner = verifier
                .build()
                .map_err(|e| Status::invalid_argument(format!("client CA: {e}")))?;
            let verifier: Arc<dyn RustlsClientCertVerifier> = match spiffe {
                Some(expected) => Arc::new(SpiffeClientCertVerifier { inner, expected }),
                None => inner,
            };
            builder
                .with_client_cert_verifier(verifier)
                .with_single_cert(identity.certs, identity.key)
                .map_err(|e| Status::invalid_argument(format!("server certificate: {e}")))?
        }
    };
    config.alpn_protocols = vec![ALPN_H2.to_vec()];
    // TLS 1.3 session tickets so repeat clients can resume instead of paying
    // a full handshake (RX-04). rustls defaults to `NeverProducesTickets`,
    // which silently disables all TLS 1.3 resumption even though
    // `send_tls13_tickets` defaults to 2. Each `ServerTls` mints its own
    // ticket keys (XChaCha20-Poly1305, rotated every 6 h); keys are never
    // shared across configs, so the rustls cross-config resumption warning
    // (authenticated sessions resumed into unauthenticated configs) cannot
    // trigger between `new` / `mtls` / `optional_mtls` instances.
    // Verification, ALPN and cipher policy are unchanged.
    config.ticketer = rustls_graviola::Ticketer::new()
        .map_err(|e| Status::internal(format!("tls ticketer: {e}")))?;
    let config = Arc::new(config);
    Ok(ServerTls {
        acceptor: TlsAcceptor::from(Arc::clone(&config)),
        config,
        observer: None,
    })
}

/// Client-side TLS: a rustls connector with ALPN `h2` and a session cache
/// (up to 256 server names), so reconnected or pooled sockets covered by one
/// `ClientTls` value resume via TLS 1.3 tickets instead of a full handshake.
///
/// `server_name` is both SNI and the name verified against the certificate.
/// It is independent of the TCP address, so you can dial `127.0.0.1` while
/// verifying `localhost`.
/// There is no grpc-go `WithPerRPCCredentials`: that is a DialOption plugging
/// `credentials.PerRPCCredentials` that add per-RPC metadata. This type is
/// transport TLS, not call credentials. There is no `WithCredentialsBundle`
/// (transport plus per-RPC credentials). Distinct from GCP-auth (a library,
/// not this DialOption). Distinct from grpc-go `WithTransportCredentials`
/// (transport; TLS is [`crate::Channel::connect_tls`] plus this type). Distinct
/// from [`crate::ClientInterceptor`] (user hook that can add metadata, not a
/// credentials plugin). Distinct from [`crate::Channel::intercept`] (attaches
/// that hook after connect).
///
/// ```no_run
/// use pbrs_grpc::{Channel, ClientTls};
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// let tls = ClientTls::webpki("api.example.com")?;
/// let channel = Channel::connect_tls("api.example.com:443", tls).await?;
/// # let _ = channel;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ClientTls {
    config: Arc<RustlsClientConfig>,
    connector: TlsConnector,
    server_name: ServerName<'static>,
    observer: Option<HandshakeObserver>,
}

impl fmt::Debug for ClientTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientTls")
            .field("server_name", &self.server_name)
            .finish_non_exhaustive()
    }
}

impl ClientTls {
    /// Trust Mozilla's CA set ([`webpki_roots`]) and verify `server_name`.
    ///
    /// There is no tonic `Endpoint::tls_config_with_verifier`: that replaces
    /// WebPKI with a custom rustls `ServerCertVerifier`. This constructor
    /// always verifies against Mozilla's CA set. Distinct from
    /// `ClientTls::native_roots` (operating-system roots, when the
    /// `native-roots` feature is enabled) and [`Self::ca`] (pin a CA, still verifies).
    /// Distinct from a skip-verify constructor (there is none).
    pub fn webpki(server_name: impl Into<String>) -> Result<Self, Status> {
        build_client(server_name.into(), webpki_roots(), None, Vec::new(), None)
    }

    /// Trust Mozilla's CA set and present `identity` (mTLS).
    pub fn webpki_mtls(server_name: impl Into<String>, identity: Identity) -> Result<Self, Status> {
        build_client(
            server_name.into(),
            webpki_roots(),
            Some(identity),
            Vec::new(),
            None,
        )
    }

    /// Trust the operating system's native root store and verify `server_name`.
    ///
    /// Requires the `native-roots` feature. This is equivalent to tonic's
    /// native-root mode where safe, but still has no skip-verify constructor.
    #[cfg(feature = "native-roots")]
    pub fn native_roots(server_name: impl Into<String>) -> Result<Self, Status> {
        build_client(server_name.into(), native_roots()?, None, Vec::new(), None)
    }

    /// Trust the operating system's native root store and present `identity`.
    ///
    /// Requires the `native-roots` feature.
    #[cfg(feature = "native-roots")]
    pub fn native_roots_mtls(
        server_name: impl Into<String>,
        identity: Identity,
    ) -> Result<Self, Status> {
        build_client(
            server_name.into(),
            native_roots()?,
            Some(identity),
            Vec::new(),
            None,
        )
    }

    /// Trust this CA bundle (PEM) and verify `server_name`. For private PKI
    /// and tests; WebPKI roots are not consulted.
    /// There is no tonic `ClientTlsConfig::assume_http2`: that skips ALPN and
    /// still treats the socket as HTTP/2. This constructor always requires ALPN
    /// `h2` after handshake. Distinct from [`crate::Channel::connect`] (h2c, no
    /// TLS). Distinct from grpc-web / HTTP/1.1 (not prior-knowledge HTTP/2 on
    /// TLS). Distinct from [`crate::ServerTls`] (server ALPN require; this is
    /// the client require). Distinct from a skip-verify constructor (there is
    /// none).
    pub fn ca(server_name: impl Into<String>, ca_pem: impl AsRef<[u8]>) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        build_client(server_name.into(), roots, None, Vec::new(), None)
    }

    /// Trust this CA bundle and present `identity` (mTLS).
    pub fn ca_mtls(
        server_name: impl Into<String>,
        ca_pem: impl AsRef<[u8]>,
        identity: Identity,
    ) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        build_client(server_name.into(), roots, Some(identity), Vec::new(), None)
    }

    /// Trust this CA bundle, verify `server_name`, and check revocation
    /// against the CRLs in `crl_pem` (A69).
    ///
    /// Every certificate in the presented chain except the trust anchor must
    /// have a determinable, non-revoked status in the CRLs. Unknown status
    /// (no covering CRL) and expired CRLs fail the handshake: revocation
    /// checking fails closed. CRL freshness is the operator's job; reload by
    /// rebuilding this value. Distinct from [`Self::ca`] (no revocation
    /// check). Distinct from a skip-verify constructor (there is none).
    pub fn ca_with_crl(
        server_name: impl Into<String>,
        ca_pem: impl AsRef<[u8]>,
        crl_pem: impl AsRef<[u8]>,
    ) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        let crls = crls_from_pem(crl_pem.as_ref())?;
        build_client(server_name.into(), roots, None, crls, None)
    }

    /// Trust this CA bundle, present `identity` (mTLS), and check server
    /// revocation against the CRLs in `crl_pem` (A69).
    ///
    /// The CRLs cover the server chain this client verifies; whether the
    /// server checks this client's revocation is the server's configuration
    /// ([`ServerTls::mtls_with_crl`]). Otherwise the same fail-closed
    /// semantics as [`Self::ca_with_crl`].
    pub fn ca_mtls_with_crl(
        server_name: impl Into<String>,
        ca_pem: impl AsRef<[u8]>,
        crl_pem: impl AsRef<[u8]>,
        identity: Identity,
    ) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        let crls = crls_from_pem(crl_pem.as_ref())?;
        build_client(server_name.into(), roots, Some(identity), crls, None)
    }

    /// Trust this CA bundle, verify `server_name`, and constrain the server
    /// leaf to the SPIFFE ID `spiffe_id` (A87).
    ///
    /// The WebPKI checks (including `server_name`, still required for SNI
    /// and chain verification) run first; a leaf that already verified must
    /// then carry exactly `spiffe_id` (e.g.
    /// `spiffe://example.org/server`) in a URI subject alternative name.
    /// There is no wildcard or trust-domain-only match. A malformed
    /// expected ID is [`crate::Code::InvalidArgument`]. Distinct from
    /// [`Self::ca`] (no SPIFFE constraint). Distinct from a skip-verify
    /// constructor (there is none).
    pub fn ca_spiffe(
        server_name: impl Into<String>,
        ca_pem: impl AsRef<[u8]>,
        spiffe_id: &str,
    ) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        let spiffe = SpiffeId::parse(spiffe_id)?;
        build_client(server_name.into(), roots, None, Vec::new(), Some(spiffe))
    }

    /// Trust this CA bundle, present `identity` (mTLS), and constrain the
    /// server leaf to the SPIFFE ID `spiffe_id` (A87).
    ///
    /// Same server-side constraint as [`Self::ca_spiffe`], plus this client
    /// presents `identity`. Whether the server constrains this client's
    /// SPIFFE ID is the server's configuration
    /// ([`ServerTls::mtls_spiffe`]).
    pub fn ca_mtls_spiffe(
        server_name: impl Into<String>,
        ca_pem: impl AsRef<[u8]>,
        spiffe_id: &str,
        identity: Identity,
    ) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        let spiffe = SpiffeId::parse(spiffe_id)?;
        build_client(
            server_name.into(),
            roots,
            Some(identity),
            Vec::new(),
            Some(spiffe),
        )
    }

    /// Observe every established handshake with `TlsHandshakeInfo`
    /// (A118/A80).
    ///
    /// The observer runs on the connecting task after the post-handshake
    /// alert drain, so it must be cheap and must not block. Without an
    /// observer nothing is recorded. Failed handshakes are never reported.
    /// Clones share the observer.
    #[must_use]
    pub fn with_handshake_observer(
        mut self,
        observer: impl Fn(&TlsHandshakeInfo) + Send + Sync + 'static,
    ) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    /// Enable rustls NSS-format key logging through `SSLKEYLOGFILE`.
    ///
    /// This is for local packet-decryption diagnostics only. It does not change
    /// certificate verification, does not create a skip-verify path, and writes
    /// nothing unless `SSLKEYLOGFILE` is set in the process environment.
    #[must_use]
    pub fn key_log_file(mut self) -> Self {
        Arc::make_mut(&mut self.config).key_log = Arc::new(KeyLogFile::new());
        self.connector = TlsConnector::from(Arc::clone(&self.config));
        self
    }

    pub(crate) async fn connect(
        &self,
        tcp: TcpStream,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>, Status> {
        let started = Instant::now();
        let mut stream = self
            .connector
            .connect(self.server_name.clone(), tcp)
            .await
            .map_err(tls_handshake_status)?;
        require_h2(stream.get_ref().1.alpn_protocol())?;
        // TLS 1.3 lets the client finish before the server has applied a
        // mandatory client-certificate check. The alert is already in the
        // socket; pull it in so connect fails instead of the first RPC.
        for _ in 0..4 {
            tokio::task::yield_now().await;
            check_post_handshake_alert(&mut stream)?;
        }
        if let Some(observer) = self.observer.as_ref() {
            let (tcp, conn) = stream.get_ref();
            observer(&TlsHandshakeInfo {
                version: protocol_name(conn.protocol_version()),
                cipher_suite: conn.negotiated_cipher_suite().map_or_else(
                    || "none".to_string(),
                    |suite| format!("{:?}", suite.suite()),
                ),
                resumed: matches!(conn.handshake_kind(), Some(HandshakeKind::Resumed)),
                alpn: conn
                    .alpn_protocol()
                    .and_then(|proto| (proto == ALPN_H2).then_some("h2")),
                peer_certificates: conn.peer_certificates().map_or(0, |certs| certs.len()),
                handshake_duration: started.elapsed(),
                tcp: TcpStats {
                    local_addr: tcp.local_addr().ok(),
                    remote_addr: tcp.peer_addr().ok(),
                },
            });
        }
        Ok(stream)
    }
}

fn check_post_handshake_alert(
    stream: &mut tokio_rustls::client::TlsStream<TcpStream>,
) -> Result<(), Status> {
    let (io, conn) = stream.get_mut();
    let mut buf = [0u8; 4096];
    match io.try_read(&mut buf) {
        Ok(0) => Err(Status::unauthenticated("tls: peer closed after handshake")),
        Ok(n) => {
            let slice = buf
                .get(..n)
                .ok_or_else(|| Status::internal("tls: short buffer"))?;
            let mut cursor = std::io::Cursor::new(slice);
            conn.read_tls(&mut cursor)
                .map_err(|e| Status::unauthenticated(format!("tls: {e}")))?;
            conn.process_new_packets()
                .map_err(|e| Status::unauthenticated(format!("tls: {e}")))?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
        Err(e) => Err(Status::unauthenticated(format!("tls: {e}"))),
    }
}

fn build_client(
    name: String,
    roots: RootCertStore,
    identity: Option<Identity>,
    crls: Vec<CertificateRevocationListDer<'static>>,
    spiffe: Option<SpiffeId>,
) -> Result<ClientTls, Status> {
    let server_name = server_name(&name)?;
    let provider = provider();
    let verifier_stage = RustlsClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| Status::internal(format!("tls versions: {e}")))?;
    // The default path keeps `with_root_certificates` untouched so existing
    // callers see byte-identical behavior. CRL and SPIFFE callers build an
    // explicit verifier: CRL-only stays on the plain WebPKI verifier, while
    // the SPIFFE wrapper (WebPKI checks first, then an added constraint)
    // needs the custom-verifier hook. That hook only ever adds rejection;
    // verification itself is never skipped.
    let builder = if crls.is_empty() && spiffe.is_none() {
        verifier_stage.with_root_certificates(roots)
    } else {
        let mut verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider);
        if !crls.is_empty() {
            verifier = verifier.with_crls(crls).enforce_revocation_expiration();
        }
        let inner = verifier
            .build()
            .map_err(|e| Status::invalid_argument(format!("trust anchor: {e}")))?;
        match spiffe {
            Some(expected) => {
                verifier_stage
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(SpiffeServerCertVerifier {
                        inner,
                        expected,
                    }))
            }
            None => verifier_stage.with_webpki_verifier(inner),
        }
    };
    let mut config = match identity {
        None => builder.with_no_client_auth(),
        Some(id) => builder
            .with_client_auth_cert(id.certs, id.key)
            .map_err(|e| Status::invalid_argument(format!("client certificate: {e}")))?,
    };
    config.alpn_protocols = vec![ALPN_H2.to_vec()];
    // Session cache for TLS 1.3 ticket resumption (RX-04). Clones of one
    // `ClientTls` share this `ClientConfig` (and its store), so pooled and
    // reconnected sockets in a `Channel` resume; a separately constructed
    // `ClientTls` starts with an empty store. Same parameters as the rustls
    // default (up to 256 server names; TLS 1.2 session-ID-or-ticket), stated
    // explicitly so a future default change cannot silently drop resumption.
    config.resumption = rustls::client::Resumption::in_memory_sessions(256);
    let config = Arc::new(config);
    Ok(ClientTls {
        connector: TlsConnector::from(Arc::clone(&config)),
        config,
        server_name,
        observer: None,
    })
}

/// Throwaway GF-06 test PKI, generated 2026-09-29 with openssl (ECDSA
/// P-256, 10-year validity) and inlined so these tests need no new files.
/// Independent oracle: `openssl verify -CAfile ca.crt -crl_check -CRLfile
/// crl.pem` accepts every `good`/`spiffe` leaf and rejects both `revoked`
/// leaves. The CA key never entered the tree. Must not be used anywhere else.
#[cfg(test)]
mod fixtures {
    /// Trust anchor for every GF-06 leaf below.
    pub(crate) const CA: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBcjCCARigAwIBAgIUSSEFcIqrWyZEN071uqfWgfrou74wCgYIKoZIzj0EAwIw\n\
        FzEVMBMGA1UEAwwMZ2YwNi10ZXN0LWNhMB4XDTI2MDkyOTA0NTc1NloXDTM2MDky\n\
        NjA0NTc1NlowFzEVMBMGA1UEAwwMZ2YwNi10ZXN0LWNhMFkwEwYHKoZIzj0CAQYI\n\
        KoZIzj0DAQcDQgAE57OEzuikYoHett1fN9NdBThzdyLx2Ps/lLSjjk37EVE4dT+g\n\
        K6d507HF4oo2dZyHuj0st37QVlQgQf+B2C28oaNCMEAwDwYDVR0TAQH/BAUwAwEB\n\
        /zAOBgNVHQ8BAf8EBAMCAYYwHQYDVR0OBBYEFO/ulSCOmbpGOvP3REuWvMDNu81q\n\
        MAoGCCqGSM49BAMCA0gAMEUCIQDCCWuG12yHjBTcEJsX3ORJdG37t55vha/y1mug\n\
        ANZa2gIgKONFb6BwoJJHJNV2fHjv9bsAjFRAC439nz2B5Tv1Cmg=\n\
        -----END CERTIFICATE-----\n";

    /// CRL revoking `server-revoked` and `client-revoked` (nextUpdate 2036).
    pub(crate) const CRL: &str = "-----BEGIN X509 CRL-----\n\
        MIHdMIGDAgEBMAoGCCqGSM49BAMCMBcxFTATBgNVBAMMDGdmMDYtdGVzdC1jYRcN\n\
        MjYwOTI5MDQ1ODA5WhcNMzYwOTI2MDQ1ODA5WjAqMBMCAhACFw0yNjA5MjkwNDU4\n\
        MDlaMBMCAiACFw0yNjA5MjkwNDU4MDlaoA8wDTALBgNVHRQEBAICEAAwCgYIKoZI\n\
        zj0EAwIDSQAwRgIhAJfFHhiW4vXt8GL/UPoPEeWwaMqtRciZwqkboCQOMKXFAiEA\n\
        8emBaDaHFegWy6y9zxZLii2bJY3Y/weujzO7whyrPgc=\n\
        -----END X509 CRL-----\n";

    /// Server leaf, SAN `DNS:localhost, IP:127.0.0.1`, not revoked.
    pub(crate) const SERVER_GOOD_CRT: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBZzCCAQ2gAwIBAgICEAEwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMZ2YwNi10\n\
        ZXN0LWNhMB4XDTI2MDkyOTA0NTc1N1oXDTM2MDkyNjA0NTc1N1owFjEUMBIGA1UE\n\
        AwwLc2VydmVyLWdvb2QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAR3L9g1OMRs\n\
        Dn19d3VngkicDtbzNl7DTvfrTZ+zrTBNFwhu/fYVAckSQ13WxWCdI92FLep31yMR\n\
        hzq7mj9d13j1o0owSDAaBgNVHREEEzARgglsb2NhbGhvc3SHBH8AAAEwCQYDVR0T\n\
        BAIwADAfBgNVHSMEGDAWgBTv7pUgjpm6Rjrz90RLlrzAzbvNajAKBggqhkjOPQQD\n\
        AgNIADBFAiEA/3kpM3AuBJJYjcf6yO8wjydB3IAIy8m/qqQaL82MA9sCIEISTk28\n\
        2m2EpCB1xTBe+OxUYku3yGI8sBYpTAslaqAY\n\
        -----END CERTIFICATE-----\n";
    pub(crate) const SERVER_GOOD_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\n\
        MHcCAQEEIHK7wFxtDx2Jy0xOgVphbQiRca56HOPWznixhS5IyPkAoAoGCCqGSM49\n\
        AwEHoUQDQgAEdy/YNTjEbA59fXd1Z4JInA7W8zZew073602fs60wTRcIbv32FQHJ\n\
        EkNd1sVgnSPdhS3qd9cjEYc6u5o/Xdd49Q==\n\
        -----END EC PRIVATE KEY-----\n";

    /// Server leaf with the same SANs, revoked in [`CRL`](self::CRL).
    pub(crate) const SERVER_REVOKED_CRT: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBazCCARCgAwIBAgICEAIwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMZ2YwNi10\n\
        ZXN0LWNhMB4XDTI2MDkyOTA0NTc1N1oXDTM2MDkyNjA0NTc1N1owGTEXMBUGA1UE\n\
        AwwOc2VydmVyLXJldm9rZWQwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAASKqI8d\n\
        5oGjaClZ5ovCJkoucLxElegfrBVazv9lyp9KkzsqAzkzn/y+IXnwaHt3Qt0kC7uu\n\
        Jrz6p6q3w9fxMh/oo0owSDAaBgNVHREEEzARgglsb2NhbGhvc3SHBH8AAAEwCQYD\n\
        VR0TBAIwADAfBgNVHSMEGDAWgBTv7pUgjpm6Rjrz90RLlrzAzbvNajAKBggqhkjO\n\
        PQQDAgNJADBGAiEAqCSpFVVR+si87oyKX6hf5rjLLoDjELVi5Nxdw1muWUUCIQDG\n\
        iaXYR93MAjvsIKoPRytgT2eLfO5sg0fk4HYqcwysYg==\n\
        -----END CERTIFICATE-----\n";
    pub(crate) const SERVER_REVOKED_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\n\
        MHcCAQEEIChqNMHD9LXoU0fGnnMgtwjXYSl/RiXzACSmirC69Z0EoAoGCCqGSM49\n\
        AwEHoUQDQgAEiqiPHeaBo2gpWeaLwiZKLnC8RJXoH6wVWs7/ZcqfSpM7KgM5M5/8\n\
        viF58Gh7d0LdJAu7ria8+qeqt8PX8TIf6A==\n\
        -----END EC PRIVATE KEY-----\n";

    /// Server leaf with SAN `DNS:localhost` plus
    /// `URI:spiffe://example.org/server`, not revoked.
    pub(crate) const SERVER_SPIFFE_CRT: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBhTCCASygAwIBAgICEAMwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMZ2YwNi10\n\
        ZXN0LWNhMB4XDTI2MDkyOTA0NTc1N1oXDTM2MDkyNjA0NTc1N1owGDEWMBQGA1UE\n\
        AwwNc2VydmVyLXNwaWZmZTBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABKrPG6Nv\n\
        xNKb1ieFreluE9TGi0t4Nzshk+vx8azMErE1MzLPsEYUkBkGMrdvuNmq3fGI1+ou\n\
        ITsYHXN6GS8y+JGjZzBlMDcGA1UdEQQwMC6CCWxvY2FsaG9zdIcEfwAAAYYbc3Bp\n\
        ZmZlOi8vZXhhbXBsZS5vcmcvc2VydmVyMAkGA1UdEwQCMAAwHwYDVR0jBBgwFoAU\n\
        7+6VII6ZukY68/dES5a8wM27zWowCgYIKoZIzj0EAwIDRwAwRAIgF6DE1eoen1lY\n\
        zCcNF0EaVg6AylvZjnYLWlVg1jMtZV4CIB5PYX2kWRc6YFvvG76YVGt3EtC7CoZx\n\
        EjUI9vL1aPER\n\
        -----END CERTIFICATE-----\n";
    pub(crate) const SERVER_SPIFFE_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\n\
        MHcCAQEEIBEO7BmDHH4RO4mAWUEd75Y19z0DXvV9QqJYqt7tdOG+oAoGCCqGSM49\n\
        AwEHoUQDQgAEqs8bo2/E0pvWJ4Wt6W4T1MaLS3g3OyGT6/HxrMwSsTUzMs+wRhSQ\n\
        GQYyt2+42ard8YjX6i4hOxgdc3oZLzL4kQ==\n\
        -----END EC PRIVATE KEY-----\n";

    /// Client leaf without a URI SAN, not revoked.
    pub(crate) const CLIENT_GOOD_CRT: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBSzCB8aADAgECAgIgATAKBggqhkjOPQQDAjAXMRUwEwYDVQQDDAxnZjA2LXRl\n\
        c3QtY2EwHhcNMjYwOTI5MDQ1NzU3WhcNMzYwOTI2MDQ1NzU3WjAWMRQwEgYDVQQD\n\
        DAtjbGllbnQtZ29vZDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABEFaGtBLkGWE\n\
        InG+/XDZtvkrXijofPXYAfZMgYXJibahOLyK3MeuxSIrcsSZgDWXcF6HapiGrEIy\n\
        UXWVHO2bstCjLjAsMAkGA1UdEwQCMAAwHwYDVR0jBBgwFoAU7+6VII6ZukY68/dE\n\
        S5a8wM27zWowCgYIKoZIzj0EAwIDSQAwRgIhAJvU5bs58kw44MNO1uamn+pgi3Ej\n\
        rm+oT2NJdvTQ9ZewAiEAxlc45iyeRHhRm88BG94fbVwqBEJsOjPjoq/TV76fHcU=\n\
        -----END CERTIFICATE-----\n";
    pub(crate) const CLIENT_GOOD_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\n\
        MHcCAQEEICxlYaVFY4RvMdj3UgkQKTm6W9dSJ2uaBkTvbHP9y304oAoGCCqGSM49\n\
        AwEHoUQDQgAEQVoa0EuQZYQicb79cNm2+SteKOh89dgB9kyBhcmJtqE4vIrcx67F\n\
        IityxJmANZdwXodqmIasQjJRdZUc7Zuy0A==\n\
        -----END EC PRIVATE KEY-----\n";

    /// Client leaf revoked in [`CRL`](self::CRL).
    pub(crate) const CLIENT_REVOKED_CRT: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBTTCB9KADAgECAgIgAjAKBggqhkjOPQQDAjAXMRUwEwYDVQQDDAxnZjA2LXRl\n\
        c3QtY2EwHhcNMjYwOTI5MDQ1NzU3WhcNMzYwOTI2MDQ1NzU3WjAZMRcwFQYDVQQD\n\
        DA5jbGllbnQtcmV2b2tlZDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABBKS24He\n\
        izY2JOnXjIdYQPR1WMaoZmN3C5Cq/7qo4PjqVV3iyYWTogriZMZMiFBJ5xTHlKEc\n\
        YmO+XmBozlWnXpujLjAsMAkGA1UdEwQCMAAwHwYDVR0jBBgwFoAU7+6VII6ZukY6\n\
        8/dES5a8wM27zWowCgYIKoZIzj0EAwIDSAAwRQIgZ8tYLXKwLowk2OdmGz+tOwWl\n\
        onONZosaV0dyKc3GsDgCIQCg6Fo7MD/JLkPbnmDrnunuZFDxnJ+jTbIAlshKhby+\n\
        og==\n\
        -----END CERTIFICATE-----\n";
    pub(crate) const CLIENT_REVOKED_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\n\
        MHcCAQEEIBnwC6KXFPS1ST8FsQODbC/ezSY/SKz2UUe6dGJBTtNroAoGCCqGSM49\n\
        AwEHoUQDQgAEEpLbgd6LNjYk6deMh1hA9HVYxqhmY3cLkKr/uqjg+OpVXeLJhZOi\n\
        CuJkxkyIUEnnFMeUoRxiY75eYGjOVademw==\n\
        -----END EC PRIVATE KEY-----\n";

    /// Client leaf with SAN `URI:spiffe://example.org/client`, not revoked.
    pub(crate) const CLIENT_SPIFFE_CRT: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBdjCCARugAwIBAgICIAMwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMZ2YwNi10\n\
        ZXN0LWNhMB4XDTI2MDkyOTA0NTc1N1oXDTM2MDkyNjA0NTc1N1owGDEWMBQGA1UE\n\
        AwwNY2xpZW50LXNwaWZmZTBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABH8WQME9\n\
        OJoK79e1d8aS1eAvVt4XCGc3bz1mjx8VZ5jfXh7Wx4yQTvgkL26rqxFJZ3mSCzLd\n\
        zTmklEwrgxGhLC2jVjBUMCYGA1UdEQQfMB2GG3NwaWZmZTovL2V4YW1wbGUub3Jn\n\
        L2NsaWVudDAJBgNVHRMEAjAAMB8GA1UdIwQYMBaAFO/ulSCOmbpGOvP3REuWvMDN\n\
        u81qMAoGCCqGSM49BAMCA0kAMEYCIQCZ0TW8HgM2njB9znBK167z2H9sb5z2KiMV\n\
        XV9RKgjVrQIhAMAGvDubBzKJjHdWjll7jh+qUD9hCpO2GEMuV1gS3Zle\n\
        -----END CERTIFICATE-----\n";
    pub(crate) const CLIENT_SPIFFE_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\n\
        MHcCAQEEIBL+45zNjCqwiTOXGz7Q7iqoPN4Aza16hFQRO1+YoJ4KoAoGCCqGSM49\n\
        AwEHoUQDQgAEfxZAwT04mgrv17V3xpLV4C9W3hcIZzdvPWaPHxVnmN9eHtbHjJBO\n\
        +CQvbqurEUlneZILMt3NOaSUTCuDEaEsLQ==\n\
        -----END EC PRIVATE KEY-----\n";

    /// Expected SPIFFE ID for [`SERVER_SPIFFE_CRT`](self::SERVER_SPIFFE_CRT).
    pub(crate) const SERVER_SPIFFE_ID: &str = "spiffe://example.org/server";
    /// Expected SPIFFE ID for [`CLIENT_SPIFFE_CRT`](self::CLIENT_SPIFFE_CRT).
    pub(crate) const CLIENT_SPIFFE_ID: &str = "spiffe://example.org/client";
}

#[cfg(test)]
mod tests {
    use super::{Identity, certs_from_pem, key_from_pem};
    use crate::status::Code;

    #[test]
    fn empty_pem_is_invalid_argument() {
        let err = Identity::from_pem("", "").expect_err("empty");
        assert_eq!(err.code(), Code::InvalidArgument);
        let err = certs_from_pem(b"not pem").expect_err("garbage");
        assert_eq!(err.code(), Code::InvalidArgument);
        let err = key_from_pem(b"").expect_err("empty key");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn tls_handshake_reset_is_unavailable() {
        let err = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
        let status = super::tls_handshake_status(err);
        assert_eq!(status.code(), Code::Unavailable);
        let cause = std::error::Error::source(&status).expect("io cause");
        assert_eq!(
            cause.downcast_ref::<std::io::Error>().expect("io").kind(),
            std::io::ErrorKind::ConnectionReset
        );
    }

    #[test]
    fn tls_handshake_invalid_data_is_unauthenticated() {
        let err = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad cert");
        let status = super::tls_handshake_status(err);
        assert_eq!(status.code(), Code::Unauthenticated);
        assert!(std::error::Error::source(&status).is_some());
    }

    #[test]
    fn tls_handshake_addr_not_available_is_unavailable() {
        let err = std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "no usable address");
        let status = super::tls_handshake_status(err);
        assert_eq!(status.code(), Code::Unavailable);
        assert!(std::error::Error::source(&status).is_some());
        let err = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad cert");
        let status = super::tls_handshake_status(err);
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    #[test]
    fn server_name_must_be_a_dns_or_ip() {
        let err = super::ClientTls::ca(
            "not a host",
            "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n",
        )
        .expect_err("bad name or empty cert");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn identity_exposes_der_certificates() {
        let id = Identity::from_pem(
            include_str!("../tests/tls_data/client.crt"),
            include_str!("../tests/tls_data/client.key"),
        )
        .expect("identity");
        let leaf = id.certificates().next().expect("leaf");
        assert!(!leaf.is_empty());
    }

    #[test]
    fn peer_identity_skips_an_empty_chain() {
        assert!(super::PeerIdentity::from_rustls(&[]).is_none());
        assert!(super::PeerIdentity::from_der_certs(None::<&[u8]>).is_none());
        let der = rustls::pki_types::CertificateDer::from(vec![0x30, 0x00]);
        let id = super::PeerIdentity::from_rustls(&[der]).expect("leaf");
        assert_eq!(id.len(), 1);
        assert!(!id.is_empty());
        assert_eq!(id.leaf(), Some(&[0x30, 0x00][..]));
        assert_eq!(
            id.certificates().collect::<Vec<_>>(),
            vec![&[0x30, 0x00][..]]
        );
        let stamped = super::PeerIdentity::from_der_certs([b"leaf"]).expect("leaf");
        assert_eq!(stamped.leaf(), Some(&b"leaf"[..]));
    }

    #[test]
    fn crl_pem_parsing() {
        let crls = super::crls_from_pem(super::fixtures::CRL.as_bytes()).expect("crl");
        assert_eq!(crls.len(), 1);
        let err = super::crls_from_pem(b"").expect_err("empty");
        assert_eq!(err.code(), Code::InvalidArgument);
        let err = super::crls_from_pem(b"not pem").expect_err("garbage");
        assert_eq!(err.code(), Code::InvalidArgument);
        // A certificate PEM is not a CRL PEM.
        let err = super::crls_from_pem(super::fixtures::CA.as_bytes()).expect_err("cert, not crl");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn spiffe_id_validation() {
        assert!(super::SpiffeId::parse("spiffe://example.org/server").is_ok());
        assert!(super::SpiffeId::parse("spiffe://example.org").is_ok());
        assert!(super::SpiffeId::parse("spiffe://example.org/a/b").is_ok());
        for bad in [
            "",
            "http://example.org/server",
            "spiffe:/example.org",
            "spiffe://",
            "spiffe://example.org/has space",
            "spiffe://example.org/has\nnewline",
            "spiffe://exa\tmple.org",
        ] {
            let err = super::SpiffeId::parse(bad).expect_err(bad);
            assert_eq!(err.code(), Code::InvalidArgument, "{bad:?}");
        }
    }

    #[test]
    fn spiffe_constructors_reject_malformed_ids() {
        let server = Identity::from_pem(
            super::fixtures::SERVER_GOOD_CRT,
            super::fixtures::SERVER_GOOD_KEY,
        )
        .expect("identity");
        let err = super::ServerTls::mtls_spiffe(server, super::fixtures::CA, "not-a-spiffe-id")
            .expect_err("bad id");
        assert_eq!(err.code(), Code::InvalidArgument);
        let err = super::ClientTls::ca_spiffe("localhost", super::fixtures::CA, "spiffe://")
            .expect_err("bad id");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn uri_san_extraction() {
        let leaf = |pem: &str| certs_from_pem(pem.as_bytes()).expect("cert");
        let spiffe = leaf(super::fixtures::SERVER_SPIFFE_CRT);
        assert_eq!(
            super::uri_subject_alternative_names(spiffe[0].as_ref()),
            vec![super::fixtures::SERVER_SPIFFE_ID]
        );
        let spiffe = leaf(super::fixtures::CLIENT_SPIFFE_CRT);
        assert_eq!(
            super::uri_subject_alternative_names(spiffe[0].as_ref()),
            vec![super::fixtures::CLIENT_SPIFFE_ID]
        );
        // DNS-only and SAN-less leaves carry no URIs.
        let good = leaf(super::fixtures::SERVER_GOOD_CRT);
        assert!(super::uri_subject_alternative_names(good[0].as_ref()).is_empty());
        let good = leaf(super::fixtures::CLIENT_GOOD_CRT);
        assert!(super::uri_subject_alternative_names(good[0].as_ref()).is_empty());
        // Malformed input fails closed to "no names", never panics.
        assert!(super::uri_subject_alternative_names(b"").is_empty());
        assert!(super::uri_subject_alternative_names(b"not der").is_empty());
        assert!(super::uri_subject_alternative_names(&[0x30, 0x82]).is_empty());
    }

    #[test]
    fn uri_san_extraction_never_panics_on_truncation() {
        let leaf = certs_from_pem(super::fixtures::SERVER_SPIFFE_CRT.as_bytes()).expect("cert");
        let der = leaf[0].as_ref();
        for end in 0..der.len() {
            // Must not panic; only the complete input yields the URI.
            assert!(
                super::uri_subject_alternative_names(&der[..end]).is_empty(),
                "prefix {end} must not match"
            );
        }
        assert_eq!(
            super::uri_subject_alternative_names(der),
            vec![super::fixtures::SERVER_SPIFFE_ID]
        );
    }

    #[test]
    fn post_quantum_status_tracks_provider() {
        // Pinned rustls-graviola 0.2.1 negotiates classical groups only.
        assert!(!super::post_quantum_key_exchange_available());
        // Tripwire: any provider change (new group of any kind) fails here
        // and forces a revisit of the A120 status, so a future PQ-capable
        // provider cannot slip through unnoticed.
        let groups: Vec<String> = super::provider()
            .kx_groups
            .iter()
            .map(|group| format!("{:?}", group.name()))
            .collect();
        assert_eq!(groups, vec!["X25519", "secp256r1", "secp384r1"]);
    }
}

#[cfg(test)]
mod handshake {
    use super::{ClientTls, Identity, ServerTls, Status};
    use crate::status::Code;
    use rustls::HandshakeKind;
    use tokio::net::{TcpListener, TcpStream};

    const CA: &str = include_str!("../tests/tls_data/ca.crt");
    const SERVER_CERT: &str = include_str!("../tests/tls_data/server.crt");
    const SERVER_KEY: &str = include_str!("../tests/tls_data/server.key");

    #[tokio::test]
    async fn mtls_handshake_rejects_anonymous_client() {
        let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
        let tls = ServerTls::mtls(identity, CA).expect("mtls");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            drop(tls.accept(tcp).await);
        });
        let tcp = TcpStream::connect(addr).await.expect("connect");
        let client = ClientTls::ca("localhost", CA).expect("client");
        let err = match client.connect(tcp).await {
            Ok(mut stream) => {
                use tokio::io::AsyncReadExt;
                let mut buf = [0u8; 1];
                match stream.read(&mut buf).await {
                    Ok(0) => Status::unauthenticated("tls: peer closed after handshake"),
                    Ok(_) => panic!("anonymous client finished handshake and read data"),
                    Err(e) => Status::unauthenticated(format!("tls: {e}")),
                }
            }
            Err(e) => e,
        };
        assert_eq!(err.code(), Code::Unauthenticated, "{err}");
        let _server_outcome = server_task.await;
    }

    /// A second connection through shared configs must resume (RX-04): the
    /// server sends TLS 1.3 tickets and the client's session store reuses one.
    #[tokio::test]
    async fn session_resumes_across_connections_with_shared_configs() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
        let server_tls = ServerTls::new(identity).expect("server");
        let client_tls = ClientTls::ca("localhost", CA).expect("client");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server_task = tokio::spawn(async move {
            let mut kinds = Vec::new();
            for _ in 0..2 {
                let (tcp, _) = listener.accept().await.expect("accept");
                let mut stream = server_tls.accept(tcp).await.expect("tls accept");
                kinds.push(stream.get_ref().1.handshake_kind());
                // App data flushes the queued NewSessionTicket messages ahead
                // of it, so a client that reads the reply has tickets stored.
                // The socket stays open for the ack: closing here would race
                // the client's post-handshake alert check.
                stream.write_all(b"ok").await.expect("write");
                let mut ack = [0u8; 2];
                stream.read_exact(&mut ack).await.expect("ack");
                assert_eq!(&ack, b"ak");
            }
            kinds
        });

        let mut client_kinds = Vec::new();
        for _ in 0..2 {
            let tcp = TcpStream::connect(addr).await.expect("connect");
            let mut stream = client_tls.connect(tcp).await.expect("tls connect");
            client_kinds.push(stream.get_ref().1.handshake_kind());
            let mut buf = [0u8; 2];
            stream.read_exact(&mut buf).await.expect("read");
            assert_eq!(&buf, b"ok");
            stream.write_all(b"ak").await.expect("ack");
        }
        let server_kinds = server_task.await.expect("server");

        assert_eq!(client_kinds[0], Some(HandshakeKind::Full), "first is full");
        assert_eq!(
            client_kinds[1],
            Some(HandshakeKind::Resumed),
            "second resumes"
        );
        assert_eq!(server_kinds[0], Some(HandshakeKind::Full), "server first");
        assert_eq!(
            server_kinds[1],
            Some(HandshakeKind::Resumed),
            "server second"
        );
    }

    /// Drive one TLS pair to completion with an ok/ack exchange. Returns the
    /// server and client outcomes; `Ok(())` means the handshake plus the
    /// full ok/ack exchange succeeded. Client handshake failures keep their
    /// [`Status`] (notably [`Code::Unauthenticated`] for certificate
    /// rejections); transport failures map to [`Code::Unknown`].
    async fn connect_pair(
        server_tls: ServerTls,
        client_tls: ClientTls,
    ) -> (Result<(), Status>, Result<(), Status>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let to_unknown =
            |what: &'static str| move |e: std::io::Error| Status::unknown(format!("{what}: {e}"));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.map_err(to_unknown("accept"))?;
            let mut stream = server_tls.accept(tcp).await.map_err(to_unknown("accept"))?;
            stream.write_all(b"ok").await.map_err(to_unknown("write"))?;
            let mut ack = [0u8; 2];
            stream
                .read_exact(&mut ack)
                .await
                .map_err(to_unknown("ack"))?;
            if ack == *b"ak" {
                Ok(())
            } else {
                Err(Status::unknown("bad ack"))
            }
        });
        let tcp = TcpStream::connect(addr)
            .await
            .map_err(to_unknown("connect"));
        let client = match tcp {
            Err(e) => Err(e),
            Ok(tcp) => match client_tls.connect(tcp).await {
                Err(e) => Err(e),
                Ok(mut stream) => {
                    let mut buf = [0u8; 2];
                    match stream.read_exact(&mut buf).await {
                        Ok(_) if buf == *b"ok" => {
                            stream.write_all(b"ak").await.map_err(to_unknown("ack"))
                        }
                        Ok(_) => Err(Status::unknown("bad greeting")),
                        Err(e) => Err(to_unknown("read")(e)),
                    }
                }
            },
        };
        let server = server
            .await
            .map_err(|e| Status::unknown(format!("join: {e}")))
            .and_then(|r| r);
        (server, client)
    }

    fn gf06_identity(crt: &str, key: &str) -> Identity {
        Identity::from_pem(crt, key).expect("identity")
    }

    /// A CRL-configured client must reject the revoked server leaf (A69).
    #[tokio::test]
    async fn crl_rejects_revoked_server_cert() {
        use super::fixtures as f;
        let server = ServerTls::new(gf06_identity(f::SERVER_REVOKED_CRT, f::SERVER_REVOKED_KEY))
            .expect("server");
        let client = ClientTls::ca_with_crl("localhost", f::CA, f::CRL).expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        let err = client_outcome.expect_err("revoked server must fail");
        assert_eq!(err.code(), Code::Unauthenticated, "{err}");
        assert!(server_outcome.is_err(), "server side must fail too");
    }

    /// The same CRL must not disturb a non-revoked server leaf (A69).
    #[tokio::test]
    async fn crl_accepts_good_server_cert() {
        use super::fixtures as f;
        let server =
            ServerTls::new(gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY)).expect("server");
        let client = ClientTls::ca_with_crl("localhost", f::CA, f::CRL).expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        client_outcome.expect("good server must connect");
        server_outcome.expect("server side must succeed");
    }

    /// Without a CRL the revoked leaf still connects: revocation comes only
    /// from the CRL, and the default verification path is unchanged (A69).
    #[tokio::test]
    async fn without_crl_revoked_server_cert_connects() {
        use super::fixtures as f;
        let server = ServerTls::new(gf06_identity(f::SERVER_REVOKED_CRT, f::SERVER_REVOKED_KEY))
            .expect("server");
        let client = ClientTls::ca("localhost", f::CA).expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        client_outcome.expect("default path ignores revocation");
        server_outcome.expect("server side must succeed");
    }

    /// A CRL that does not cover the chain must fail closed, not be ignored:
    /// unknown revocation status is an error (A69). The GF-06 CRL is checked
    /// against the older `tls_data` PKI, whose issuer it cannot cover.
    #[tokio::test]
    async fn unrelated_crl_fails_closed() {
        let server = ServerTls::new(gf06_identity(SERVER_CERT, SERVER_KEY)).expect("server");
        let client = ClientTls::ca_with_crl("localhost", CA, super::fixtures::CRL).expect("client");
        let (_, client_outcome) = connect_pair(server, client).await;
        let err = client_outcome.expect_err("unknown status must fail");
        assert_eq!(err.code(), Code::Unauthenticated, "{err}");
    }

    /// An mTLS server with a CRL must reject the revoked client leaf (A69).
    #[tokio::test]
    async fn crl_rejects_revoked_client_cert() {
        use super::fixtures as f;
        let server = ServerTls::mtls_with_crl(
            gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY),
            f::CA,
            f::CRL,
        )
        .expect("server");
        let client = ClientTls::ca_mtls(
            "localhost",
            f::CA,
            gf06_identity(f::CLIENT_REVOKED_CRT, f::CLIENT_REVOKED_KEY),
        )
        .expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        assert!(server_outcome.is_err(), "server must reject revoked client");
        assert!(client_outcome.is_err(), "client must observe the abort");
    }

    /// The same mTLS+CRL server must accept a non-revoked client leaf (A69).
    #[tokio::test]
    async fn crl_accepts_good_client_cert() {
        use super::fixtures as f;
        let server = ServerTls::mtls_with_crl(
            gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY),
            f::CA,
            f::CRL,
        )
        .expect("server");
        let client = ClientTls::ca_mtls_with_crl(
            "localhost",
            f::CA,
            f::CRL,
            gf06_identity(f::CLIENT_GOOD_CRT, f::CLIENT_GOOD_KEY),
        )
        .expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        client_outcome.expect("good client must connect");
        server_outcome.expect("server side must succeed");
    }

    /// A SPIFFE-constrained client must accept the matching server leaf (A87).
    #[tokio::test]
    async fn spiffe_accepts_matching_server_cert() {
        use super::fixtures as f;
        let server = ServerTls::new(gf06_identity(f::SERVER_SPIFFE_CRT, f::SERVER_SPIFFE_KEY))
            .expect("server");
        let client = ClientTls::ca_spiffe("localhost", f::CA, f::SERVER_SPIFFE_ID).expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        client_outcome.expect("matching SPIFFE ID must connect");
        server_outcome.expect("server side must succeed");
    }

    /// The same client must reject a verified leaf without the URI SAN (A87):
    /// WebPKI success alone is not enough.
    #[tokio::test]
    async fn spiffe_rejects_server_without_uri_san() {
        use super::fixtures as f;
        let server =
            ServerTls::new(gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY)).expect("server");
        let client = ClientTls::ca_spiffe("localhost", f::CA, f::SERVER_SPIFFE_ID).expect("client");
        let (_, client_outcome) = connect_pair(server, client).await;
        let err = client_outcome.expect_err("missing URI SAN must fail");
        assert_eq!(err.code(), Code::Unauthenticated, "{err}");
    }

    /// The same client must reject a leaf carrying a different SPIFFE ID
    /// (A87): matching is exact, with no trust-domain-only fallback.
    #[tokio::test]
    async fn spiffe_rejects_wrong_server_id() {
        use super::fixtures as f;
        let server = ServerTls::new(gf06_identity(f::SERVER_SPIFFE_CRT, f::SERVER_SPIFFE_KEY))
            .expect("server");
        let client =
            ClientTls::ca_spiffe("localhost", f::CA, "spiffe://example.org/other").expect("client");
        let (_, client_outcome) = connect_pair(server, client).await;
        let err = client_outcome.expect_err("wrong SPIFFE ID must fail");
        assert_eq!(err.code(), Code::Unauthenticated, "{err}");
    }

    /// An mTLS server with a SPIFFE constraint must accept the matching
    /// client leaf (A87).
    #[tokio::test]
    async fn spiffe_accepts_matching_client_cert() {
        use super::fixtures as f;
        // Mutual SPIFFE: the client constrains the server leaf and the
        // server constrains the client leaf.
        let server = ServerTls::mtls_spiffe(
            gf06_identity(f::SERVER_SPIFFE_CRT, f::SERVER_SPIFFE_KEY),
            f::CA,
            f::CLIENT_SPIFFE_ID,
        )
        .expect("server");
        let client = ClientTls::ca_mtls_spiffe(
            "localhost",
            f::CA,
            f::SERVER_SPIFFE_ID,
            gf06_identity(f::CLIENT_SPIFFE_CRT, f::CLIENT_SPIFFE_KEY),
        )
        .expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        client_outcome.expect("matching SPIFFE IDs must connect");
        server_outcome.expect("server side must succeed");
    }

    /// The same server must reject a verified client leaf without the URI
    /// SAN (A87).
    #[tokio::test]
    async fn spiffe_rejects_client_without_uri_san() {
        use super::fixtures as f;
        let server = ServerTls::mtls_spiffe(
            gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY),
            f::CA,
            f::CLIENT_SPIFFE_ID,
        )
        .expect("server");
        let client = ClientTls::ca_mtls(
            "localhost",
            f::CA,
            gf06_identity(f::CLIENT_GOOD_CRT, f::CLIENT_GOOD_KEY),
        )
        .expect("client");
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        assert!(
            server_outcome.is_err(),
            "server must reject missing URI SAN"
        );
        assert!(client_outcome.is_err(), "client must observe the abort");
    }

    /// Both ends must observe a successful handshake: TLS parameters (A118)
    /// and TCP addresses (A80), with cross-matching socket pairs.
    #[allow(
        clippy::disallowed_types,
        reason = "sync test-only observer log; guard never held across await"
    )]
    #[tokio::test]
    async fn handshake_observer_fires_on_both_ends() {
        use super::fixtures as f;
        use std::sync::{Arc, Mutex};

        let server_seen: Arc<Mutex<Vec<super::TlsHandshakeInfo>>> =
            Arc::new(Mutex::new(Vec::new()));
        let client_seen: Arc<Mutex<Vec<super::TlsHandshakeInfo>>> =
            Arc::new(Mutex::new(Vec::new()));
        let server = ServerTls::new(gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY))
            .expect("server")
            .with_handshake_observer({
                let seen = Arc::clone(&server_seen);
                move |info: &super::TlsHandshakeInfo| seen.lock().expect("lock").push(info.clone())
            });
        let client = ClientTls::ca("localhost", f::CA)
            .expect("client")
            .with_handshake_observer({
                let seen = Arc::clone(&client_seen);
                move |info: &super::TlsHandshakeInfo| seen.lock().expect("lock").push(info.clone())
            });
        let (server_outcome, client_outcome) = connect_pair(server, client).await;
        client_outcome.expect("client must connect");
        server_outcome.expect("server must connect");

        let server_seen = server_seen.lock().expect("lock");
        let client_seen = client_seen.lock().expect("lock");
        assert_eq!(server_seen.len(), 1);
        assert_eq!(client_seen.len(), 1);
        let (server_info, client_info) = (&server_seen[0], &client_seen[0]);
        for info in [&server_info, &client_info] {
            assert_eq!(info.version, "TLSv1.3");
            assert!(!info.cipher_suite.is_empty());
            assert!(!info.resumed, "first handshake is full");
            assert_eq!(info.alpn, Some("h2"));
        }
        assert_eq!(server_info.peer_certificates, 0, "anonymous client");
        assert_eq!(client_info.peer_certificates, 1, "server chain");
        // Socket pairs cross-match: each end's remote is the other's local.
        assert_eq!(
            server_info.tcp.remote_addr, client_info.tcp.local_addr,
            "server remote == client local"
        );
        assert_eq!(
            client_info.tcp.remote_addr, server_info.tcp.local_addr,
            "client remote == server local"
        );
    }

    /// A resumed handshake must be reported as resumed on both ends (A118).
    #[allow(
        clippy::disallowed_types,
        reason = "sync test-only observer log; guard never held across await"
    )]
    #[tokio::test]
    async fn handshake_observer_reports_resumption() {
        use super::fixtures as f;
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let seen: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let server_tls = ServerTls::new(gf06_identity(f::SERVER_GOOD_CRT, f::SERVER_GOOD_KEY))
            .expect("server")
            .with_handshake_observer({
                let seen = Arc::clone(&seen);
                move |info: &super::TlsHandshakeInfo| {
                    seen.lock().expect("lock").push(info.resumed);
                }
            });
        let client_tls = ClientTls::ca("localhost", f::CA).expect("client");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server_task = tokio::spawn(async move {
            for _ in 0..2 {
                let (tcp, _) = listener.accept().await.expect("accept");
                let mut stream = server_tls.accept(tcp).await.expect("tls accept");
                stream.write_all(b"ok").await.expect("write");
                let mut ack = [0u8; 2];
                stream.read_exact(&mut ack).await.expect("ack");
                assert_eq!(&ack, b"ak");
            }
        });
        for _ in 0..2 {
            let tcp = TcpStream::connect(addr).await.expect("connect");
            let mut stream = client_tls.connect(tcp).await.expect("tls connect");
            let mut buf = [0u8; 2];
            stream.read_exact(&mut buf).await.expect("read");
            assert_eq!(&buf, b"ok");
            stream.write_all(b"ak").await.expect("ack");
        }
        server_task.await.expect("server");
        assert_eq!(*seen.lock().expect("lock"), vec![false, true]);
    }

    /// A separately constructed `ClientTls` (fresh session store) performs a
    /// full handshake even against a ticket-issuing server.
    #[tokio::test]
    async fn fresh_client_config_does_not_resume() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
        let server_tls = ServerTls::new(identity).expect("server");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server_task = tokio::spawn(async move {
            for _ in 0..2 {
                let (tcp, _) = listener.accept().await.expect("accept");
                let mut stream = server_tls.accept(tcp).await.expect("tls accept");
                stream.write_all(b"ok").await.expect("write");
                let mut ack = [0u8; 2];
                stream.read_exact(&mut ack).await.expect("ack");
                assert_eq!(&ack, b"ak");
            }
        });

        for _ in 0..2 {
            let fresh = ClientTls::ca("localhost", CA).expect("client");
            let tcp = TcpStream::connect(addr).await.expect("connect");
            let mut stream = fresh.connect(tcp).await.expect("tls connect");
            assert_eq!(
                stream.get_ref().1.handshake_kind(),
                Some(HandshakeKind::Full)
            );
            let mut buf = [0u8; 2];
            stream.read_exact(&mut buf).await.expect("read");
            assert_eq!(&buf, b"ok");
            stream.write_all(b"ak").await.expect("ack");
        }
        server_task.await.expect("server");
    }
}
