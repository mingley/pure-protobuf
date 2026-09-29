//! TLS for the kernel: rustls over Graviola, ALPN `h2`, no C compiler.
//!
//! Certificate verification is not optional. There is no "insecure" constructor.
//! Trust either Mozilla's WebPKI roots or a CA you pass in.

use crate::status::Status;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    ClientConfig as RustlsClientConfig, KeyLogFile, RootCertStore,
    ServerConfig as RustlsServerConfig,
};
use std::fmt;
use std::sync::Arc;
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
        build_server(identity, ClientAuth::None)
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
        build_server(identity, ClientAuth::Required(cas))
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
        build_server(identity, ClientAuth::Optional(cas))
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
        let stream = self.acceptor.accept(tcp).await?;
        if stream.get_ref().1.alpn_protocol() == Some(ALPN_H2) {
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

fn build_server(identity: Identity, client_auth: ClientAuth) -> Result<ServerTls, Status> {
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
            let mut verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
                Arc::new(cas),
                provider,
            );
            if optional_client_auth {
                verifier = verifier.allow_unauthenticated();
            }
            let verifier = verifier
                .build()
                .map_err(|e| Status::invalid_argument(format!("client CA: {e}")))?;
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
        build_client(server_name.into(), webpki_roots(), None)
    }

    /// Trust Mozilla's CA set and present `identity` (mTLS).
    pub fn webpki_mtls(server_name: impl Into<String>, identity: Identity) -> Result<Self, Status> {
        build_client(server_name.into(), webpki_roots(), Some(identity))
    }

    /// Trust the operating system's native root store and verify `server_name`.
    ///
    /// Requires the `native-roots` feature. This is equivalent to tonic's
    /// native-root mode where safe, but still has no skip-verify constructor.
    #[cfg(feature = "native-roots")]
    pub fn native_roots(server_name: impl Into<String>) -> Result<Self, Status> {
        build_client(server_name.into(), native_roots()?, None)
    }

    /// Trust the operating system's native root store and present `identity`.
    ///
    /// Requires the `native-roots` feature.
    #[cfg(feature = "native-roots")]
    pub fn native_roots_mtls(
        server_name: impl Into<String>,
        identity: Identity,
    ) -> Result<Self, Status> {
        build_client(server_name.into(), native_roots()?, Some(identity))
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
        build_client(server_name.into(), roots, None)
    }

    /// Trust this CA bundle and present `identity` (mTLS).
    pub fn ca_mtls(
        server_name: impl Into<String>,
        ca_pem: impl AsRef<[u8]>,
        identity: Identity,
    ) -> Result<Self, Status> {
        let roots = roots_from_certs(certs_from_pem(ca_pem.as_ref())?)?;
        build_client(server_name.into(), roots, Some(identity))
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
) -> Result<ClientTls, Status> {
    let server_name = server_name(&name)?;
    let provider = provider();
    let builder = RustlsClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Status::internal(format!("tls versions: {e}")))?
        .with_root_certificates(roots);
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
    })
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
