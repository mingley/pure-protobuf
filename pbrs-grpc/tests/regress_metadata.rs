//! Regression coverage for public metadata validation.

use pbrs_grpc::{Code, Metadata};

#[test]
fn ascii_metadata_rejects_non_ascii_values() {
    let mut md = Metadata::new();

    let err = md
        .insert("x-name", "\u{0572}")
        .expect_err("non-ASCII metadata value must be rejected");
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(md.get("x-name"), None);

    let err = md
        .set("x-name", "\u{0572}")
        .expect_err("non-ASCII replacement must be rejected");
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(md.get("x-name"), None);
}

#[test]
fn binary_suffix_mutations_are_case_insensitive() {
    let mut md = Metadata::new();
    md.insert_bin("x-value-bin", [0, 1])
        .expect("first binary value");

    for key in ["x-value-bin", "X-VALUE-BIN", "x-VaLuE-bIn"] {
        assert_eq!(
            md.insert(key, "not binary")
                .expect_err("ASCII suffix")
                .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            md.set(key, "not binary").expect_err("ASCII suffix").code(),
            Code::InvalidArgument
        );
        assert!(!md.remove(key), "ASCII remove must leave binary data");
    }
    assert_eq!(
        md.get_all_bin("x-value-bin").collect::<Vec<_>>(),
        vec![vec![0, 1]]
    );
    md.insert_bin("X-VALUE-BIN", [2, 3])
        .expect("mixed-case repeated value");
    assert_eq!(
        md.get_all_bin("x-value-bin").collect::<Vec<_>>(),
        vec![vec![0, 1], vec![2, 3]]
    );

    md.set_bin("X-VALUE-BIN", [4, 5])
        .expect("mixed-case replacement");
    assert_eq!(
        md.get_all_bin("x-value-bin").collect::<Vec<_>>(),
        vec![vec![4, 5]]
    );
    assert!(md.remove_bin("x-VALUE-bIN"));
    assert_eq!(md.get_bin("x-value-bin"), None);

    md.insert_bin("-BIN", [9]).expect("short binary key");
    assert_eq!(md.get_bin("-bin").as_deref(), Some(&[9][..]));
    assert!(md.remove_bin("-bIn"));
    md.insert("X-BIN-extra", "plain").expect("not a suffix");
    assert_eq!(md.get("x-bin-extra"), Some("plain"));
    assert!(!md.remove_bin("X-BIN-extra"));
    assert!(md.remove("x-BIN-extra"));
}

#[test]
fn invalid_binary_keys_do_not_change_metadata() {
    let mut md = Metadata::new();
    md.insert_bin("x-keep-bin", [1, 2]).expect("seed");
    for key in [
        "",
        "b",
        "bin",
        "-bi",
        "é",
        "é-BIN",
        "🚀-BIN",
        "invalid key-BIN",
        "x\0-BIN",
        "GRPC-RESERVED-BIN",
    ] {
        assert_eq!(
            md.insert_bin(key, [0]).expect_err("invalid key").code(),
            Code::InvalidArgument,
            "{key:?}"
        );
        assert_eq!(
            md.set_bin(key, [0]).expect_err("invalid key").code(),
            Code::InvalidArgument,
            "{key:?}"
        );
        assert!(!md.remove_bin(key), "{key:?}");
        assert_eq!(md.get_bin("x-keep-bin").as_deref(), Some(&[1, 2][..]));
        assert_eq!(md.len(), 1);
    }
}

#[test]
fn mixed_case_suffix_preserves_validation_precedence() {
    let mut md = Metadata::new();
    md.insert("x-keep", "value").expect("seed");

    let reserved = md
        .insert("GRPC-RESERVED-BIN", "value")
        .expect_err("reserved");
    assert!(reserved.message().starts_with("reserved metadata key"));
    let reserved = md
        .insert_bin("GRPC-RESERVED-BIN", [1])
        .expect_err("reserved binary");
    assert!(reserved.message().starts_with("reserved metadata key"));

    let suffix = md
        .insert("invalid key-BIN", "value")
        .expect_err("binary suffix");
    assert_eq!(suffix.message(), "ascii metadata key must not end in -bin");
    let suffix = md
        .insert_bin("invalid key", [1])
        .expect_err("missing binary suffix");
    assert_eq!(suffix.message(), "binary metadata key must end in -bin");
    let invalid = md
        .insert_bin("invalid key-BIN", [1])
        .expect_err("invalid header");
    assert!(invalid.message().starts_with("invalid metadata key"));
    assert_eq!(md.len(), 1);
    assert_eq!(md.get("x-keep"), Some("value"));
}

#[cfg(feature = "tonic")]
#[test]
fn mixed_case_metadata_key_kinds_match_tonic() {
    use tonic::metadata::{Ascii, Binary, MetadataKey};

    for key in [
        "x-value",
        "X-VALUE",
        "x-value-bin",
        "X-VALUE-BIN",
        "x-VaLuE-bIn",
        "-BIN",
        "bin",
        "",
        "é-BIN",
        "bad key-BIN",
        "x\0-BIN",
    ] {
        let mut md = Metadata::new();
        let ascii = md.insert(key, "value").is_ok();
        md.clear();
        let binary = md.insert_bin(key, [1, 2]).is_ok();
        assert_eq!(
            ascii,
            MetadataKey::<Ascii>::from_bytes(key.as_bytes()).is_ok(),
            "ASCII {key:?}"
        );
        assert_eq!(
            binary,
            MetadataKey::<Binary>::from_bytes(key.as_bytes()).is_ok(),
            "binary {key:?}"
        );
    }
}

#[tokio::test]
async fn mixed_case_binary_metadata_round_trips_over_grpc() {
    use pbrs_grpc::hello::HelloRequest;
    use pbrs_grpc::{Channel, Request, Response, Router, Rpc, Service, Status};
    use std::time::Duration;
    use tokio::net::TcpListener;

    struct BinaryEcho;

    impl Service for BinaryEcho {
        const NAME: &'static str = "test.Metadata";

        async fn call(&self, rpc: Rpc) {
            rpc.unary(|req: Request<HelloRequest>| async move {
                let received = req
                    .metadata()
                    .get_all_bin("X-INPUT-BIN")
                    .collect::<Vec<_>>();
                if received != vec![vec![0, 1, 255], vec![9, 8]] {
                    return Err(Status::internal("unexpected request metadata"));
                }
                let mut response = Response::new(req.into_inner());
                response.metadata_mut().insert_bin("X-OUTPUT-BIN", [3, 4])?;
                response.metadata_mut().insert_bin("x-OUTPUT-Bin", [5])?;
                response.trailers_mut().insert_bin("X-TRAILER-BIN", [7])?;
                Ok(response)
            })
            .await;
        }
    }

    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(BinaryEcho)
            .serve_listener(listener)
            .await
            .expect("serve");
    });
    let _server = AbortOnDrop(server);
    let channel = Channel::connect(addr).await.expect("connect");

    let mut request = Request::new(HelloRequest::new());
    assert!(
        request
            .metadata_mut()
            .insert("X-INPUT-BIN", "plain")
            .is_err()
    );
    request
        .metadata_mut()
        .insert_bin("X-INPUT-BIN", [0, 1, 255])
        .expect("first request value");
    request
        .metadata_mut()
        .insert_bin("x-Input-Bin", [9, 8])
        .expect("second request value");
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        channel.unary::<HelloRequest, HelloRequest>("/test.Metadata/Echo", request),
    )
    .await
    .expect("deadline")
    .expect("unary");
    assert_eq!(
        response
            .metadata()
            .get_all_bin("X-OUTPUT-BIN")
            .collect::<Vec<_>>(),
        vec![vec![3, 4], vec![5]]
    );
    assert_eq!(
        response.trailers().get_bin("x-trailer-bin").as_deref(),
        Some(&[7][..])
    );
}
