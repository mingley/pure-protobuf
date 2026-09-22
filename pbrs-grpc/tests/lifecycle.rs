//! Seeded lifecycle fault histories across all shapes, boundaries, faults, and transports.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::lifecycle::{
    CallShape, FaultKind, LifecycleBoundary, LifecycleRunner, LifecycleScenario, RstReason,
    TransportKind, ALL_TRANSPORTS, QUALIFICATION_CYCLES_PER_TRANSPORT,
};
use common::{name_of, req, Echo};
use pbrs_grpc::hello::GreeterClient;
use pbrs_grpc::hello::GreeterServer;
use pbrs_grpc::{Channel, Code, Request, Server, Status};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

#[tokio::test]
async fn pr_suite_fast_cycles() {
    // 48 curated deterministic scenarios spanning all 4 shapes, 5 transports,
    // 6 boundaries, and 10 fault types.
    let scenarios = [
        // 1. Unary over FromIo
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::Cancel,
            seed: 101,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::RstStream(RstReason::RefusedStream),
            seed: 102,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::RstStream(RstReason::Cancel),
            seed: 103,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Goaway,
            seed: 104,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::TcpReset,
            seed: 105,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::TrailersReceived,
            fault: FaultKind::TcpDisconnect,
            seed: 106,
        },
        // 2. ClientStreaming over FromIo
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::FutureDropClient,
            seed: 201,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::FutureDropServer,
            seed: 202,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::StreamHalfClose,
            seed: 203,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Cancel,
            seed: 204,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::RstStream(RstReason::InternalError),
            seed: 205,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::TrailersReceived,
            fault: FaultKind::Goaway,
            seed: 206,
        },
        // 3. ServerStreaming over FromIo
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::Cancel,
            seed: 301,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::RstStream(RstReason::RefusedStream),
            seed: 302,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::FutureDropClient,
            seed: 303,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::FutureDropServer,
            seed: 304,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::Cancel,
            seed: 305,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::TrailersReceived,
            fault: FaultKind::TcpReset,
            seed: 306,
        },
        // 4. Bidi over FromIo
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::FutureDropClient,
            seed: 401,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::RstStream(RstReason::Cancel),
            seed: 402,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::StreamHalfClose,
            seed: 403,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Goaway,
            seed: 404,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::FutureDropClient,
            seed: 405,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::FromIo,
            boundary: LifecycleBoundary::TrailersReceived,
            fault: FaultKind::TcpDisconnect,
            seed: 406,
        },
        // 5. TCP transport coverage
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Tcp,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::RstStream(RstReason::RefusedStream),
            seed: 501,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::Tcp,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::Cancel,
            seed: 502,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::Tcp,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::TcpReset,
            seed: 503,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::Tcp,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::Goaway,
            seed: 504,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Tcp,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::FutureDropClient,
            seed: 505,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::Tcp,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::StreamHalfClose,
            seed: 506,
        },
        // 6. TLS transport coverage
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::Cancel,
            seed: 601,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::StreamHalfClose,
            seed: 602,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Goaway,
            seed: 603,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::FutureDropClient,
            seed: 604,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::TcpReset,
            seed: 605,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::FutureDropServer,
            seed: 606,
        },
        // 7. mTLS transport coverage (all 4 shapes)
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Mtls,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::Cancel,
            seed: 701,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::Mtls,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::RstStream(RstReason::Cancel),
            seed: 702,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::Mtls,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Goaway,
            seed: 703,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::Mtls,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::FutureDropClient,
            seed: 704,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Mtls,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::FutureDropServer,
            seed: 705,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::Mtls,
            boundary: LifecycleBoundary::TrailersReceived,
            fault: FaultKind::TcpDisconnect,
            seed: 706,
        },
        // 8. UDS transport coverage (all 4 shapes)
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Uds,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::RstStream(RstReason::RefusedStream),
            seed: 801,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::Uds,
            boundary: LifecycleBoundary::BodyStarted,
            fault: FaultKind::StreamHalfClose,
            seed: 802,
        },
        LifecycleScenario {
            shape: CallShape::ServerStreaming,
            transport: TransportKind::Uds,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Goaway,
            seed: 803,
        },
        LifecycleScenario {
            shape: CallShape::Bidi,
            transport: TransportKind::Uds,
            boundary: LifecycleBoundary::ResponseBodyReceived,
            fault: FaultKind::TcpReset,
            seed: 804,
        },
        LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::Uds,
            boundary: LifecycleBoundary::Queued,
            fault: FaultKind::FutureDropClient,
            seed: 805,
        },
        LifecycleScenario {
            shape: CallShape::ClientStreaming,
            transport: TransportKind::Uds,
            boundary: LifecycleBoundary::TrailersReceived,
            fault: FaultKind::Cancel,
            seed: 806,
        },
    ];

    for (idx, scenario) in scenarios.iter().enumerate() {
        println!("PR scenario {idx}: {scenario:?}");
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            LifecycleRunner::run_scenario(*scenario),
        )
        .await
        .unwrap_or_else(|_| panic!("PR suite scenario {idx} timed out: {scenario:?}"));
    }
}

#[tokio::test]
async fn generator_1000_seeded_cycles() {
    // Executes the recorded 1000-cycle FromIo qualification schedule.
    let schedule = LifecycleScenario::qualification_schedule(TransportKind::FromIo);
    assert_eq!(schedule.len(), QUALIFICATION_CYCLES_PER_TRANSPORT);

    for (cycle, scenario) in schedule.iter().enumerate() {
        let scenario = *scenario;
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::spawn(async move {
                LifecycleRunner::run_scenario(scenario).await;
            }),
        )
        .await;

        match res {
            Ok(Ok(_)) => {}
            Ok(Err(panic_err)) => {
                panic!(
                    "Generator cycle {cycle}/1000 panicked! Failing seed: {:#x}. Scenario: {scenario:?}. Err: {panic_err:?}",
                    scenario.seed
                );
            }
            Err(_) => {
                panic!(
                    "Generator cycle {cycle}/1000 timed out! Failing seed: {:#x}. Scenario: {scenario:?}",
                    scenario.seed
                );
            }
        }
    }
}

#[test]
fn qualification_schedules_cover_every_cell() {
    use std::collections::HashSet;

    // Every advertised transport records exactly 1000 qualification cycles
    // covering all 4 shapes, 6 boundaries, and 10 faults. No I/O: this pins
    // the recorded seed schedules that qualification executes.
    assert_eq!(ALL_TRANSPORTS.len(), 5);
    for transport in ALL_TRANSPORTS {
        let schedule = LifecycleScenario::qualification_schedule(transport);
        assert_eq!(
            schedule.len(),
            QUALIFICATION_CYCLES_PER_TRANSPORT,
            "transport {transport:?} must record 1000 cycles"
        );

        // Deterministic: rebuilding the schedule yields identical scenarios.
        let rebuilt = LifecycleScenario::qualification_schedule(transport);
        assert_eq!(schedule, rebuilt, "transport {transport:?} schedule");

        assert!(
            schedule.iter().all(|s| s.transport == transport),
            "transport {transport:?} schedule must be pinned to its cell"
        );

        let shapes: HashSet<CallShape> = schedule.iter().map(|s| s.shape).collect();
        assert_eq!(
            shapes.len(),
            4,
            "transport {transport:?} shapes: {shapes:?}"
        );

        let boundaries: HashSet<LifecycleBoundary> = schedule.iter().map(|s| s.boundary).collect();
        assert_eq!(
            boundaries.len(),
            6,
            "transport {transport:?} boundaries: {boundaries:?}"
        );

        let faults: HashSet<FaultKind> = schedule.iter().map(|s| s.fault).collect();
        assert_eq!(
            faults.len(),
            10,
            "transport {transport:?} faults: {faults:?}"
        );

        let seeds: HashSet<u64> = schedule.iter().map(|s| s.seed).collect();
        assert_eq!(
            seeds.len(),
            QUALIFICATION_CYCLES_PER_TRANSPORT,
            "transport {transport:?} seeds must be unique"
        );
    }
}

#[tokio::test]
async fn qualification_1000_cycles_per_socket_transport() {
    // Executes the recorded 1000-cycle qualification schedules for every
    // socket transport (TCP, TLS, mTLS, UDS). FromIo runs in
    // `generator_1000_seeded_cycles`. Serial like the generator so every
    // failure reports its exact transport, cycle, and seed.
    for transport in [
        TransportKind::Tcp,
        TransportKind::Tls,
        TransportKind::Mtls,
        TransportKind::Uds,
    ] {
        let schedule = LifecycleScenario::qualification_schedule(transport);
        assert_eq!(schedule.len(), QUALIFICATION_CYCLES_PER_TRANSPORT);

        for (cycle, scenario) in schedule.iter().enumerate() {
            let scenario = *scenario;
            let res = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::spawn(async move {
                    LifecycleRunner::run_scenario(scenario).await;
                }),
            )
            .await;

            match res {
                Ok(Ok(_)) => {}
                Ok(Err(panic_err)) => {
                    panic!(
                        "Qualification {transport:?} cycle {cycle}/1000 panicked! Failing seed: {:#x}. Scenario: {scenario:?}. Err: {panic_err:?}",
                        scenario.seed
                    );
                }
                Err(_) => {
                    panic!(
                        "Qualification {transport:?} cycle {cycle}/1000 timed out! Failing seed: {:#x}. Scenario: {scenario:?}",
                        scenario.seed
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn unary_all_boundaries_from_io() {
    let boundaries = [
        LifecycleBoundary::Queued,
        LifecycleBoundary::HeadersSent,
        LifecycleBoundary::BodyStarted,
        LifecycleBoundary::ResponseHeadersReceived,
        LifecycleBoundary::ResponseBodyReceived,
        LifecycleBoundary::TrailersReceived,
    ];

    for (i, boundary) in boundaries.iter().enumerate() {
        let scenario = LifecycleScenario {
            shape: CallShape::Unary,
            transport: TransportKind::FromIo,
            boundary: *boundary,
            fault: FaultKind::Cancel,
            seed: 1000 + i as u64,
        };
        LifecycleRunner::run_scenario(scenario).await;
    }
}

#[tokio::test]
async fn client_streaming_half_close_and_drop() {
    let scenario_close = LifecycleScenario {
        shape: CallShape::ClientStreaming,
        transport: TransportKind::FromIo,
        boundary: LifecycleBoundary::BodyStarted,
        fault: FaultKind::StreamHalfClose,
        seed: 2001,
    };
    LifecycleRunner::run_scenario(scenario_close).await;

    let scenario_drop = LifecycleScenario {
        shape: CallShape::ClientStreaming,
        transport: TransportKind::FromIo,
        boundary: LifecycleBoundary::BodyStarted,
        fault: FaultKind::FutureDropClient,
        seed: 2002,
    };
    LifecycleRunner::run_scenario(scenario_drop).await;
}

#[tokio::test]
async fn server_streaming_rst_stream_and_goaway() {
    let scenario_rst = LifecycleScenario {
        shape: CallShape::ServerStreaming,
        transport: TransportKind::FromIo,
        boundary: LifecycleBoundary::ResponseBodyReceived,
        fault: FaultKind::RstStream(RstReason::RefusedStream),
        seed: 3001,
    };
    LifecycleRunner::run_scenario(scenario_rst).await;

    let scenario_goaway = LifecycleScenario {
        shape: CallShape::ServerStreaming,
        transport: TransportKind::FromIo,
        boundary: LifecycleBoundary::ResponseHeadersReceived,
        fault: FaultKind::Goaway,
        seed: 3002,
    };
    LifecycleRunner::run_scenario(scenario_goaway).await;
}

#[tokio::test]
async fn bidi_tcp_disconnect_and_reset() {
    let scenario_reset = LifecycleScenario {
        shape: CallShape::Bidi,
        transport: TransportKind::Tcp,
        boundary: LifecycleBoundary::BodyStarted,
        fault: FaultKind::TcpReset,
        seed: 4001,
    };
    LifecycleRunner::run_scenario(scenario_reset).await;

    let scenario_disconnect = LifecycleScenario {
        shape: CallShape::Bidi,
        transport: TransportKind::Tcp,
        boundary: LifecycleBoundary::ResponseHeadersReceived,
        fault: FaultKind::TcpDisconnect,
        seed: 4002,
    };
    LifecycleRunner::run_scenario(scenario_disconnect).await;
}

#[tokio::test]
async fn tls_cancel_and_shutdown_every_shape() {
    let shapes = [
        CallShape::Unary,
        CallShape::ClientStreaming,
        CallShape::ServerStreaming,
        CallShape::Bidi,
    ];

    for (i, shape) in shapes.iter().enumerate() {
        let scenario_cancel = LifecycleScenario {
            shape: *shape,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::HeadersSent,
            fault: FaultKind::Cancel,
            seed: 5000 + i as u64,
        };
        LifecycleRunner::run_scenario(scenario_cancel).await;

        let scenario_goaway = LifecycleScenario {
            shape: *shape,
            transport: TransportKind::Tls,
            boundary: LifecycleBoundary::ResponseHeadersReceived,
            fault: FaultKind::Goaway,
            seed: 5100 + i as u64,
        };
        LifecycleRunner::run_scenario(scenario_goaway).await;
    }
}

async fn spawn_lifecycle_shutdown_server(
    f: impl FnOnce(Server<GreeterServer<Echo>>) -> Server<GreeterServer<Echo>>,
) -> (
    SocketAddr,
    Server<GreeterServer<Echo>>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), Status>>,
) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let server = f(Server::new(GreeterServer::new(Echo)));
    let srv_clone = server.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        srv_clone
            .serve_with_shutdown(listener, async move {
                rx.await.ok();
            })
            .await
    });
    (addr, server, tx, handle)
}

async fn connect_client(addr: SocketAddr) -> Channel {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(ch) => return ch,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

#[tokio::test]
async fn test_lifecycle_graceful_shutdown_unending_client_stream() {
    let grace = Duration::from_millis(150);
    let (addr, server, shutdown_tx, server_handle) = spawn_lifecycle_shutdown_server(|s| {
        s.max_connection_age_grace(grace)
            .byte_budget(64 * 1024)
            .max_concurrent_rpcs(10)
    })
    .await;

    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    // Client starts client-streaming, sends 1 message, leaves stream unending.
    let (tx, call) = client.client_hello(Request::new(()));
    let _handle = tokio::spawn(call);
    tx.send(req("lifecycle_unending")).await.expect("send 1");

    tokio::time::sleep(Duration::from_millis(30)).await;

    // Trigger graceful shutdown
    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("shutdown signal");

    // Admission cutoff: new connections are refused
    let probe = Channel::connect(addr).await;
    if let Ok(ch) = probe {
        let probe_client = GreeterClient::new(ch);
        let probe_res = probe_client.say_hello(Request::new(req("probe"))).await;
        assert!(probe_res.is_err(), "new calls after shutdown must fail");
    }

    // Grace policy enforced: drain terminates bounded by grace period
    let server_res = tokio::time::timeout(Duration::from_millis(1500), server_handle)
        .await
        .expect("server shutdown must be bounded by grace policy")
        .expect("server join");
    assert!(server_res.is_ok());

    let elapsed = drain_start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(100),
        "must allow in-flight grace period before force-close, elapsed {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(1200),
        "must not extend termination indefinitely, elapsed {elapsed:?}"
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.is_byte_budget_quiescent());
}

#[tokio::test]
async fn test_lifecycle_graceful_shutdown_handshake_stall() {
    let (addr, server, shutdown_tx, server_handle) = spawn_lifecycle_shutdown_server(|s| {
        s.handshake_timeout(Duration::from_millis(200))
            .max_connection_age_grace(Duration::from_millis(150))
            .byte_budget(64 * 1024)
    })
    .await;

    // Connect raw TCP socket and stall (send nothing)
    let raw = tokio::net::TcpStream::connect(addr)
        .await
        .expect("tcp connect");
    tokio::time::sleep(Duration::from_millis(25)).await;

    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("shutdown signal");

    let server_res = tokio::time::timeout(Duration::from_millis(1000), server_handle)
        .await
        .expect("shutdown must terminate promptly despite stalled handshake")
        .expect("server join");
    assert!(server_res.is_ok());

    let elapsed = drain_start.elapsed();
    assert!(
        elapsed < Duration::from_millis(600),
        "stalled handshake must not extend shutdown, took {elapsed:?}"
    );

    drop(raw);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.is_byte_budget_quiescent());
}

#[tokio::test]
async fn test_lifecycle_cancellation_cleanup_drops_call_futures_all_shapes() {
    let (addr, server, _shutdown_tx, _handle) =
        spawn_lifecycle_shutdown_server(|s| s.max_concurrent_rpcs(2).byte_budget(64 * 1024)).await;

    let channel = connect_client(addr)
        .await
        .max_concurrent_rpcs(2)
        .byte_budget(64 * 1024);
    let client = GreeterClient::new(channel.clone());

    // 1. ClientStreaming shape drop
    {
        let (tx1, call1) = client.client_hello(Request::new(()));
        let (tx2, call2) = client.client_hello(Request::new(()));
        let h1 = tokio::spawn(call1);
        let h2 = tokio::spawn(call2);
        tx1.send(req("c1")).await.ok();
        tx2.send(req("c2")).await.ok();
        tokio::time::sleep(Duration::from_millis(25)).await;

        // Overflow rejected
        let err = client
            .say_hello(Request::new(req("over")))
            .await
            .expect_err("cap hit");
        assert_eq!(err.code(), Code::ResourceExhausted);

        // Abort / drop futures
        h1.abort();
        h2.abort();
        drop(h1);
        drop(h2);
        drop(tx1);
        drop(tx2);
        tokio::time::sleep(Duration::from_millis(40)).await;

        // Next call immediately succeeds
        let res = client
            .say_hello(Request::new(req("recovered1")))
            .await
            .expect("success");
        assert_eq!(name_of(res.get_ref()), "recovered1");
    }

    // 2. ServerStreaming shape drop
    {
        let call1 = client.server_hello(Request::new(req("s1,s2,s3,s4")));
        let call2 = client.server_hello(Request::new(req("s5,s6,s7,s8")));
        let h1 = tokio::spawn(call1);
        let h2 = tokio::spawn(call2);
        tokio::time::sleep(Duration::from_millis(25)).await;

        let err = client
            .say_hello(Request::new(req("over")))
            .await
            .expect_err("cap hit");
        assert_eq!(err.code(), Code::ResourceExhausted);

        h1.abort();
        h2.abort();
        drop(h1);
        drop(h2);
        tokio::time::sleep(Duration::from_millis(40)).await;

        let res = client
            .say_hello(Request::new(req("recovered2")))
            .await
            .expect("success");
        assert_eq!(name_of(res.get_ref()), "recovered2");
    }

    // 3. Bidi shape drop
    {
        let (tx1, call1) = client.stream_hello(Request::new(()));
        let (tx2, call2) = client.stream_hello(Request::new(()));
        let h1 = tokio::spawn(call1);
        let h2 = tokio::spawn(call2);
        tx1.send(req("b1")).await.ok();
        tx2.send(req("b2")).await.ok();
        tokio::time::sleep(Duration::from_millis(25)).await;

        let err = client
            .say_hello(Request::new(req("over")))
            .await
            .expect_err("cap hit");
        assert_eq!(err.code(), Code::ResourceExhausted);

        h1.abort();
        h2.abort();
        drop(h1);
        drop(h2);
        drop(tx1);
        drop(tx2);
        tokio::time::sleep(Duration::from_millis(40)).await;

        let res = client
            .say_hello(Request::new(req("recovered3")))
            .await
            .expect("success");
        assert_eq!(name_of(res.get_ref()), "recovered3");
    }

    // Quiescence verification
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(channel.is_byte_budget_quiescent());
    assert!(server.is_byte_budget_quiescent());
}

#[tokio::test]
async fn test_lifecycle_unending_stream_deadline_and_graceful_drain() {
    let (addr, server, shutdown_tx, server_handle) = spawn_lifecycle_shutdown_server(|s| {
        s.timeout(Duration::from_millis(100))
            .max_connection_age_grace(Duration::from_millis(150))
            .byte_budget(64 * 1024)
    })
    .await;

    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    // Client starts unending stream
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("deadline_stream")).await.expect("send 1");

    // Wait for deadline to terminate the call
    let err = call
        .await
        .expect_err("deadline must terminate unending stream");
    assert_eq!(err.code(), Code::DeadlineExceeded);

    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(server.is_byte_budget_quiescent());

    // Trigger graceful shutdown
    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("shutdown");

    let server_res = tokio::time::timeout(Duration::from_millis(500), server_handle)
        .await
        .expect("shutdown must complete quickly after deadline termination")
        .expect("server join");
    assert!(server_res.is_ok());

    let elapsed = drain_start.elapsed();
    assert!(
        elapsed < Duration::from_millis(300),
        "post-deadline drain must complete quickly, took {elapsed:?}"
    );
    assert!(server.is_byte_budget_quiescent());
}
