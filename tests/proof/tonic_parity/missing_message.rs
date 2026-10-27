//! Raw-wire behavior comparisons against independent tonic/prost messages.
use bytes::Bytes;
use pbrs_grpc_example_tonic_ports as ports;
use std::{net::SocketAddr, pin::Pin, time::Duration};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

mod routeguide {
    tonic::include_proto!("routeguide");
}

#[derive(Default)]
struct Service;
#[tonic::async_trait]
impl routeguide::route_guide_server::RouteGuide for Service {
    async fn get_feature(
        &self,
        _: Request<routeguide::Point>,
    ) -> Result<Response<routeguide::Feature>, Status> {
        Ok(Response::new(routeguide::Feature::default()))
    }
    type ListFeaturesStream = ReceiverStream<Result<routeguide::Feature, Status>>;
    async fn list_features(
        &self,
        _: Request<routeguide::Rectangle>,
    ) -> Result<Response<Self::ListFeaturesStream>, Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(Ok(routeguide::Feature::default())).await.unwrap();
        Ok(Response::new(ReceiverStream::new(rx)))
    }
    async fn record_route(
        &self,
        request: Request<tonic::Streaming<routeguide::Point>>,
    ) -> Result<Response<routeguide::RouteSummary>, Status> {
        let mut stream = request.into_inner();
        let mut count = 0;
        while stream.message().await?.is_some() {
            count += 1;
        }
        Ok(Response::new(routeguide::RouteSummary {
            point_count: count,
            ..Default::default()
        }))
    }
    type RouteChatStream =
        Pin<Box<dyn futures_util::Stream<Item = Result<routeguide::RouteNote, Status>> + Send>>;
    async fn route_chat(
        &self,
        request: Request<tonic::Streaming<routeguide::RouteNote>>,
    ) -> Result<Response<Self::RouteChatStream>, Status> {
        Ok(Response::new(Box::pin(request.into_inner())))
    }
}

#[derive(Debug)]
struct Reply {
    code: String,
    message: String,
}
async fn raw_call(addr: SocketAddr, method: &str, payload: Bytes) -> Reply {
    let socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) = h2::client::handshake(socket).await.unwrap();
    let driver = tokio::spawn(async move {
        connection.await.ok();
    });
    let request = http::Request::post(format!("http://{addr}/routeguide.RouteGuide/{method}"))
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(())
        .unwrap();
    let empty = payload.is_empty();
    let (response, mut send) = sender.send_request(request, empty).unwrap();
    if !empty {
        send.send_data(payload, true).unwrap();
    }
    let response = response.await.unwrap();
    let mut headers = response.headers().clone();
    let mut body = response.into_body();
    while let Some(data) = body.data().await {
        let data = data.unwrap();
        body.flow_control().release_capacity(data.len()).unwrap();
    }
    if let Some(trailers) = body.trailers().await.unwrap() {
        headers.extend(trailers);
    }
    let reply = Reply {
        code: headers
            .get("grpc-status")
            .expect("grpc-status")
            .to_str()
            .unwrap()
            .to_owned(),
        message: headers
            .get("grpc-message")
            .map_or("", |v| v.to_str().unwrap())
            .to_owned(),
    };
    driver.abort();
    reply
}

#[tokio::test]
async fn request_message_presence_matches_tonic_for_all_shapes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tonic_addr = listener.local_addr().unwrap();
    let tonic_server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(routeguide::route_guide_server::RouteGuideServer::new(
                Service,
            ))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let (native_addr, _native_guard) = ports::spawn_pbrs_routeguide_server().await.unwrap();
    let mut differences = Vec::new();
    for (method, missing_code) in [
        ("GetFeature", "13"),
        ("ListFeatures", "13"),
        ("RecordRoute", "0"),
        ("RouteChat", "0"),
    ] {
        for (label, payload, expected) in [
            ("missing", Bytes::new(), missing_code),
            ("framed_empty", Bytes::from_static(&[0, 0, 0, 0, 0]), "0"),
        ] {
            let tonic = tokio::time::timeout(
                Duration::from_secs(5),
                raw_call(tonic_addr, method, payload.clone()),
            )
            .await
            .unwrap();
            let native = tokio::time::timeout(
                Duration::from_secs(5),
                raw_call(native_addr, method, payload),
            )
            .await
            .unwrap();
            println!(
                "{{\"method\":\"{method}\",\"body\":\"{label}\",\"tonic_code\":\"{}\",\"native_code\":\"{}\",\"tonic_message\":\"{}\",\"native_message\":\"{}\"}}",
                tonic.code, native.code, tonic.message, native.message
            );
            assert_eq!(
                tonic.code, expected,
                "independent tonic control for {method}/{label}"
            );
            if native.code != tonic.code {
                differences.push(format!(
                    "{method}/{label}: tonic={}, native={}",
                    tonic.code, native.code
                ));
            }
        }
    }
    tonic_server.abort();
    assert!(differences.is_empty(), "{}", differences.join("; "));
}

async fn raw_response_peer(
    payload: Bytes,
    code: &'static str,
    trailers_only: bool,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let payload = payload.clone();
            tokio::spawn(async move {
                let mut connection = h2::server::handshake(socket).await.unwrap();
                while let Some(Ok((_request, mut respond))) = connection.accept().await {
                    let mut headers = http::Response::builder()
                        .status(200)
                        .header("content-type", "application/grpc");
                    if trailers_only {
                        headers = headers
                            .header("grpc-status", code)
                            .header("grpc-message", "peer%20error");
                    }
                    let mut send = respond
                        .send_response(headers.body(()).unwrap(), trailers_only)
                        .unwrap();
                    if !trailers_only {
                        if !payload.is_empty() {
                            send.send_data(payload.clone(), false).unwrap();
                        }
                        let mut trailers = http::HeaderMap::new();
                        trailers.insert("grpc-status", http::HeaderValue::from_static(code));
                        if code != "0" {
                            trailers.insert(
                                "grpc-message",
                                http::HeaderValue::from_static("peer%20error"),
                            );
                        }
                        send.send_trailers(trailers).unwrap();
                    }
                }
            });
        }
    });
    (addr, server)
}

#[tokio::test]
async fn response_message_presence_matches_tonic_and_preserves_peer_errors() {
    let mut differences = Vec::new();
    for (label, payload, code, trailers_only, expected) in [
        ("missing_ok", Bytes::new(), "0", false, 13),
        (
            "framed_empty",
            Bytes::from_static(&[0, 0, 0, 0, 0]),
            "0",
            false,
            0,
        ),
        ("missing_error", Bytes::new(), "7", false, 7),
        ("trailers_only_error", Bytes::new(), "7", true, 7),
        (
            "partial_frame_error",
            Bytes::from_static(&[0, 0]),
            "7",
            false,
            7,
        ),
    ] {
        let (addr, server) = raw_response_peer(payload, code, trailers_only).await;
        let mut tonic =
            routeguide::route_guide_client::RouteGuideClient::connect(format!("http://{addr}"))
                .await
                .unwrap();
        let native = ports::prost_gen::routeguide::RouteGuideClient::new(
            pbrs_grpc::Channel::connect(addr).await.unwrap(),
        );
        let tonic_reply = tonic.get_feature(routeguide::Point::default()).await;
        let native_reply = native
            .get_feature(pbrs_grpc::Request::new(
                ports::prost_gen::routeguide::Point::default(),
            ))
            .await;
        let tonic_code = tonic_reply.as_ref().err().map_or(0, |s| s.code() as i32);
        let native_code = native_reply.as_ref().err().map_or(0, |s| s.code() as i32);
        println!(
            "{{\"method\":\"GetFeature\",\"response\":\"{label}\",\"tonic_code\":{tonic_code},\"native_code\":{native_code}}}"
        );
        assert_eq!(
            tonic_code, expected,
            "independent tonic unary control {label}"
        );
        if tonic_code != native_code {
            differences.push(format!(
                "unary/{label}: tonic={tonic_code}, native={native_code}"
            ));
        }
        if expected == 7 {
            assert_eq!(tonic_reply.unwrap_err().message(), "peer error");
            assert_eq!(native_reply.unwrap_err().message(), "peer error");
        }

        let tonic_reply = tonic
            .record_route(tokio_stream::empty::<routeguide::Point>())
            .await;
        let (sender, call) = native.record_route(pbrs_grpc::Request::new(()));
        sender.close();
        let native_reply = call.await;
        let tonic_code = tonic_reply.as_ref().err().map_or(0, |s| s.code() as i32);
        let native_code = native_reply.as_ref().err().map_or(0, |s| s.code() as i32);
        println!(
            "{{\"method\":\"RecordRoute\",\"response\":\"{label}\",\"tonic_code\":{tonic_code},\"native_code\":{native_code}}}"
        );
        assert_eq!(
            tonic_code, expected,
            "independent tonic clientstream control {label}"
        );
        if tonic_code != native_code {
            differences.push(format!(
                "clientstream/{label}: tonic={tonic_code}, native={native_code}"
            ));
        }
        server.abort();
    }
    assert!(differences.is_empty(), "{}", differences.join("; "));
}
