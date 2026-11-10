//! Built-in TLS behavior across supported and unsupported architectures.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "bounded TLS regression tests"
)]

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
use super::ClientTls;
use super::{Identity, ServerTls, fixtures};
use crate::{Code, Status};

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn identity() -> Identity {
    Identity::from_pem(fixtures::SERVER_GOOD_CRT, fixtures::SERVER_GOOD_KEY)
        .expect("valid test identity")
}

fn error<T>(label: &str, result: Result<T, Status>, code: Code, message: &str) {
    let status = match result {
        Ok(_) => panic!("{label}: unexpectedly succeeded"),
        Err(status) => status,
    };
    assert_eq!(status.code(), code, "{label}: {status}");
    assert!(status.message().contains(message), "{label}: {status}");
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn unavailable<T>(label: &str, result: Result<T, Status>) {
    error(label, result, Code::Unimplemented, "from_rustls");
}

fn invalid<T>(label: &str, result: Result<T, Status>, message: &str) {
    error(label, result, Code::InvalidArgument, message);
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn builtin_constructors_report_unavailable() {
    unavailable("server", ServerTls::new(identity()));
    unavailable("mtls", ServerTls::mtls(identity(), fixtures::CA));
    unavailable(
        "optional mtls",
        ServerTls::optional_mtls(identity(), fixtures::CA),
    );
    unavailable(
        "server CRL",
        ServerTls::mtls_with_crl(identity(), fixtures::CA, fixtures::CRL),
    );
    unavailable(
        "server SPIFFE",
        ServerTls::mtls_spiffe(identity(), fixtures::CA, fixtures::CLIENT_SPIFFE_ID),
    );
    unavailable("webpki", ClientTls::webpki("localhost"));
    unavailable(
        "webpki mtls",
        ClientTls::webpki_mtls("localhost", identity()),
    );
    unavailable("CA", ClientTls::ca("localhost", fixtures::CA));
    unavailable(
        "CA mtls",
        ClientTls::ca_mtls("localhost", fixtures::CA, identity()),
    );
    unavailable(
        "client CRL",
        ClientTls::ca_with_crl("localhost", fixtures::CA, fixtures::CRL),
    );
    unavailable(
        "client CRL mtls",
        ClientTls::ca_mtls_with_crl("localhost", fixtures::CA, fixtures::CRL, identity()),
    );
    unavailable(
        "client SPIFFE",
        ClientTls::ca_spiffe("localhost", fixtures::CA, fixtures::SERVER_SPIFFE_ID),
    );
    unavailable(
        "client SPIFFE mtls",
        ClientTls::ca_mtls_spiffe(
            "localhost",
            fixtures::CA,
            fixtures::SERVER_SPIFFE_ID,
            identity(),
        ),
    );
    // A parse-valid mismatched pair needs a provider for the later key check.
    unavailable(
        "mismatched server key",
        ServerTls::new(mismatched_identity()),
    );
    unavailable(
        "mismatched client key",
        ClientTls::ca_mtls("localhost", fixtures::CA, mismatched_identity()),
    );
    assert!(!super::post_quantum_key_exchange_available());
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn input_validation_precedes_unavailable_provider() {
    const BAD: &str = "invalid";
    const CERT: &str = "certificate PEM";
    const CRL: &str = "CRL PEM";
    const SPIFFE: &str = "invalid SPIFFE ID";
    const NAME: &str = "invalid TLS server name";

    invalid("identity", Identity::from_pem(BAD, BAD), CERT);
    invalid(
        "identity key",
        Identity::from_pem(fixtures::SERVER_GOOD_CRT, BAD),
        "private key PEM",
    );
    invalid("server mtls", ServerTls::mtls(identity(), BAD), CERT);
    invalid(
        "server optional mtls",
        ServerTls::optional_mtls(identity(), BAD),
        CERT,
    );
    invalid(
        "server CRL checks CA first",
        ServerTls::mtls_with_crl(identity(), BAD, BAD),
        CERT,
    );
    invalid(
        "server CRL",
        ServerTls::mtls_with_crl(identity(), fixtures::CA, BAD),
        CRL,
    );
    invalid(
        "server SPIFFE checks CA first",
        ServerTls::mtls_spiffe(identity(), BAD, BAD),
        CERT,
    );
    invalid(
        "server SPIFFE",
        ServerTls::mtls_spiffe(identity(), fixtures::CA, BAD),
        SPIFFE,
    );

    invalid("webpki", ClientTls::webpki("invalid name"), NAME);
    invalid(
        "webpki mtls",
        ClientTls::webpki_mtls("invalid name", identity()),
        NAME,
    );
    invalid(
        "CA checks certificate first",
        ClientTls::ca("invalid name", BAD),
        CERT,
    );
    invalid(
        "CA mtls checks certificate first",
        ClientTls::ca_mtls("invalid name", BAD, identity()),
        CERT,
    );
    invalid(
        "client CRL checks CA first",
        ClientTls::ca_with_crl("invalid name", BAD, BAD),
        CERT,
    );
    invalid(
        "client CRL",
        ClientTls::ca_with_crl("invalid name", fixtures::CA, BAD),
        CRL,
    );
    invalid(
        "client CRL mtls checks CA first",
        ClientTls::ca_mtls_with_crl("invalid name", BAD, BAD, identity()),
        CERT,
    );
    invalid(
        "client CRL mtls",
        ClientTls::ca_mtls_with_crl("invalid name", fixtures::CA, BAD, identity()),
        CRL,
    );
    invalid(
        "client SPIFFE checks CA first",
        ClientTls::ca_spiffe("invalid name", BAD, BAD),
        CERT,
    );
    invalid(
        "client SPIFFE",
        ClientTls::ca_spiffe("invalid name", fixtures::CA, BAD),
        SPIFFE,
    );
    invalid(
        "client SPIFFE mtls checks CA first",
        ClientTls::ca_mtls_spiffe("invalid name", BAD, BAD, identity()),
        CERT,
    );
    invalid(
        "client SPIFFE mtls",
        ClientTls::ca_mtls_spiffe("invalid name", fixtures::CA, BAD, identity()),
        SPIFFE,
    );
    invalid(
        "valid CA checks name",
        ClientTls::ca("invalid name", fixtures::CA),
        NAME,
    );
    invalid(
        "valid CA mtls checks name",
        ClientTls::ca_mtls("invalid name", fixtures::CA, identity()),
        NAME,
    );
    invalid(
        "valid CA and CRL check name",
        ClientTls::ca_with_crl("invalid name", fixtures::CA, fixtures::CRL),
        NAME,
    );
    invalid(
        "valid CA and CRL mtls check name",
        ClientTls::ca_mtls_with_crl("invalid name", fixtures::CA, fixtures::CRL, identity()),
        NAME,
    );
    invalid(
        "valid CA and SPIFFE check name",
        ClientTls::ca_spiffe("invalid name", fixtures::CA, fixtures::SERVER_SPIFFE_ID),
        NAME,
    );
    invalid(
        "valid CA and SPIFFE mtls check name",
        ClientTls::ca_mtls_spiffe(
            "invalid name",
            fixtures::CA,
            fixtures::SERVER_SPIFFE_ID,
            identity(),
        ),
        NAME,
    );
}

#[cfg(all(
    feature = "native-roots",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
#[test]
fn native_roots_reach_provider_after_loading_and_name_validation() {
    if let Err(status) = super::native_roots() {
        eprintln!("native roots are unavailable on this host: {status}");
        return;
    }
    unavailable("native roots", ClientTls::native_roots("localhost"));
    unavailable(
        "native roots mtls",
        ClientTls::native_roots_mtls("localhost", identity()),
    );
    invalid(
        "native roots name",
        ClientTls::native_roots("invalid name"),
        "invalid TLS server name",
    );
    invalid(
        "native roots mtls name",
        ClientTls::native_roots_mtls("invalid name", identity()),
        "invalid TLS server name",
    );
}

fn mismatched_identity() -> Identity {
    Identity::from_pem(fixtures::SERVER_GOOD_CRT, fixtures::CLIENT_GOOD_KEY)
        .expect("PEM parsing does not match certificate and key")
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn supported_provider_rejects_mismatched_server_key() {
    invalid(
        "mismatched server key",
        ServerTls::new(mismatched_identity()),
        "server certificate",
    );
}
