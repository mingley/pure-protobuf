mod routeguide {
    include!(concat!(env!("OUT_DIR"), "/routeguide.rs"));
    include!(concat!(
        env!("OUT_DIR"),
        "/routeguide/route_guide.pbrs_grpc.rs"
    ));
}

use pbrs_grpc::codec::prost::Streaming;
use pbrs_grpc::{Channel, Code, Request, Response, Router, Rpc, Server, Service, Status};
use routeguide::{
    Feature, Point, Rectangle, RouteGuideClient, RouteGuideServer, RouteNote, RouteSummary,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Svc;

impl routeguide::RouteGuide for Svc {
    async fn get_feature(&self, request: Request<Point>) -> Result<Response<Feature>, Status> {
        let point = request.into_inner();
        Ok(Response::new(Feature {
            name: format!("feature:{}:{}", point.latitude, point.longitude),
            location: Some(point),
        }))
    }

    async fn list_features(
        &self,
        _request: Request<Rectangle>,
    ) -> Result<Response<Streaming<Feature>>, Status> {
        let (tx, stream) = Streaming::channel(2);
        tokio::spawn(async move {
            let _ = tx
                .send(Feature {
                    name: "one".into(),
                    location: Some(Point {
                        latitude: 1,
                        longitude: 2,
                    }),
                })
                .await;
            let _ = tx
                .send(Feature {
                    name: "two".into(),
                    location: Some(Point {
                        latitude: 3,
                        longitude: 4,
                    }),
                })
                .await;
        });
        Ok(Response::new(stream))
    }

    async fn record_route(
        &self,
        request: Request<Streaming<Point>>,
    ) -> Result<Response<RouteSummary>, Status> {
        let mut stream = request.into_inner();
        let mut count = 0;
        while stream.message().await?.is_some() {
            count += 1;
        }
        Ok(Response::new(RouteSummary {
            point_count: count,
            feature_count: 0,
            distance: 0,
            elapsed_time: 0,
        }))
    }

    async fn route_chat(
        &self,
        request: Request<Streaming<RouteNote>>,
    ) -> Result<Response<Streaming<RouteNote>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(2);
        tokio::spawn(async move {
            while let Ok(Some(note)) = inbound.message().await {
                if tx
                    .send(RouteNote {
                        message: format!("echo:{}", note.message),
                        location: note.location,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

struct DispatchProbe {
    inner: RouteGuideServer<Svc>,
    boxed_calls: Arc<AtomicUsize>,
    unary_bytes: Arc<AtomicUsize>,
    chat_bytes: Arc<AtomicUsize>,
    selected: bool,
}

impl Service for DispatchProbe {
    const NAME: &'static str = <RouteGuideServer<Svc> as Service>::NAME;

    async fn call(&self, rpc: Rpc) {
        self.inner.call(rpc).await;
    }

    fn call_boxed(&self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.boxed_calls.fetch_add(1, Ordering::Relaxed);
        let slot = match rpc.method() {
            "GetFeature" => Some(&self.unary_bytes),
            "RouteChat" => Some(&self.chat_bytes),
            _ => None,
        };
        let future: Pin<Box<dyn Future<Output = ()> + Send + '_>> = if self.selected {
            self.inner.call_boxed(rpc)
        } else {
            // Exercise the generated direct hook as well as its boxed override.
            Box::pin(self.inner.call(rpc))
        };
        if let Some(slot) = slot {
            slot.store(
                std::mem::size_of_val(future.as_ref().get_ref()),
                Ordering::Relaxed,
            );
        }
        future
    }
}

async fn check_dispatch(router: bool, selected: bool) -> Result<(usize, usize), Status> {
    let boxed_calls = Arc::new(AtomicUsize::new(0));
    let unary_bytes = Arc::new(AtomicUsize::new(0));
    let chat_bytes = Arc::new(AtomicUsize::new(0));
    let service = DispatchProbe {
        inner: RouteGuideServer::new(Svc),
        boxed_calls: Arc::clone(&boxed_calls),
        unary_bytes: Arc::clone(&unary_bytes),
        chat_bytes: Arc::clone(&chat_bytes),
        selected,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let server = tokio::spawn(async move {
        if router {
            Router::new()
                .add_service(service)
                .serve_listener(listener)
                .await
        } else {
            Server::new(service).serve_listener(listener).await
        }
        .expect("serve generated prost service");
    });
    let channel = Channel::connect(addr).await?;
    let client = RouteGuideClient::new(channel.clone());

    let feature = client
        .get_feature(Request::new(Point {
            latitude: 7,
            longitude: 8,
        }))
        .await?
        .into_inner();
    assert_eq!(feature.name, "feature:7:8");

    let mut features = client
        .list_features(Request::new(Rectangle {
            lo: Some(Point {
                latitude: 0,
                longitude: 0,
            }),
            hi: Some(Point {
                latitude: 10,
                longitude: 10,
            }),
        }))
        .await?
        .into_inner();
    let features = features.collect().await?;
    assert_eq!(
        features
            .iter()
            .map(|feature| feature.name.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );

    let (tx, record) = client.record_route(Request::new(()));
    tx.send(Point {
        latitude: 1,
        longitude: 1,
    })
    .await?;
    tx.send(Point {
        latitude: 2,
        longitude: 2,
    })
    .await?;
    tx.close();
    assert_eq!(record.await?.into_inner().point_count, 2);

    let (tx, chat) = client.route_chat(Request::new(()));
    tx.send(RouteNote {
        message: "hi".into(),
        location: Some(Point {
            latitude: 1,
            longitude: 1,
        }),
    })
    .await?;
    tx.close();
    let mut replies = chat.await?.into_inner();
    assert_eq!(replies.message().await?.expect("reply").message, "echo:hi");
    assert!(replies.message().await?.is_none());

    let status = channel
        .unary::<
            pbrs_grpc::codec::prost::Message<Point>,
            pbrs_grpc::codec::prost::Message<Feature>,
        >(
            "/routeguide.RouteGuide/Unknown",
            Request::new(pbrs_grpc::codec::prost::Message(Point::default())),
        )
        .await
        .expect_err("unknown method must fail");
    assert_eq!(status.code(), Code::Unimplemented);
    assert_eq!(boxed_calls.load(Ordering::Relaxed), 5);
    let unary = unary_bytes.load(Ordering::Relaxed);
    let chat = chat_bytes.load(Ordering::Relaxed);
    assert!(
        unary > 0 && chat > 0,
        "both generated method futures were measured"
    );
    println!("dispatch router={router} selected={selected} unary={unary} chat={chat}");
    server.abort();
    Ok((unary, chat))
}

#[tokio::main]
async fn main() -> Result<(), Status> {
    let single = check_dispatch(false, true).await?;
    let router = check_dispatch(true, true).await?;
    let direct = check_dispatch(false, false).await?;
    // Compare the generated hooks with the same service, methods and data.
    // The old inherited call_boxed hook boxes exactly the direct future.
    assert!(
        single.0 < direct.0,
        "single selected unary future must be smaller: selected={}, direct={}",
        single.0,
        direct.0
    );
    assert!(
        router.0 < direct.0,
        "router selected unary future must be smaller: selected={}, direct={}",
        router.0,
        direct.0
    );
    println!("prost routeguide ok");
    Ok(())
}
