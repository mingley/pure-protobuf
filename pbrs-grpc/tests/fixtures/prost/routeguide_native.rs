mod routeguide {
    include!(concat!(env!("OUT_DIR"), "/routeguide.rs"));
    include!(concat!(
        env!("OUT_DIR"),
        "/routeguide/route_guide.pbrs_grpc.rs"
    ));
}

use pbrs_grpc::codec::prost::Streaming;
use pbrs_grpc::{Channel, Request, Response, Router, Status};
use routeguide::{Feature, Point, Rectangle, RouteGuideClient, RouteGuideServer, RouteNote, RouteSummary};

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

#[tokio::main]
async fn main() -> Result<(), Status> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(RouteGuideServer::new(Svc))
            .serve_listener(listener)
            .await
            .ok();
    });
    let mut client = RouteGuideClient::new(Channel::connect(addr).await?);

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
    assert_eq!(features.collect().await?.len(), 2);

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
    assert_eq!(
        replies.message().await?.expect("reply").message,
        "echo:hi"
    );
    server.abort();
    println!("prost routeguide ok");
    Ok(())
}
