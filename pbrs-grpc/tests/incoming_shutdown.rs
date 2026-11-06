//! Shutdown readiness must preempt an immediately ready incoming backlog.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "bounded public server regression controls"
)]

use pbrs_grpc::{
    Channel, Code, ConnectionInfo, Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest,
    Incoming, IncomingAccept, PeerCred, Request, Response, Server, Status,
};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::DuplexStream;
use tokio::sync::oneshot;

const BACKLOG: usize = 8;

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        assert_eq!(
            request.remote_addr(),
            Some("192.0.2.1:8".parse().expect("remote"))
        );
        assert_eq!(
            request.local_addr(),
            Some("127.0.0.1:9".parse().expect("local"))
        );
        assert_eq!(request.scheme(), Some("https"));
        assert_eq!(
            request.peer_cred().map(|cred| (cred.uid(), cred.gid())),
            Some((42, 43))
        );
        let mut reply = HelloReply::new();
        reply.set_message("metadata preserved");
        Ok(Response::new(reply))
    }
}

struct ReadyIncoming {
    entries: VecDeque<Result<(DuplexStream, Option<SocketAddr>), Status>>,
    polls: Arc<AtomicUsize>,
    stop_after_first: Option<oneshot::Sender<()>>,
    pending_after_entries: bool,
}

impl ReadyIncoming {
    fn backlog(polls: Arc<AtomicUsize>) -> Self {
        let entries = (0..BACKLOG)
            .map(|_| {
                let (io, peer) = tokio::io::duplex(64);
                drop(peer);
                Ok((io, None))
            })
            .collect();
        Self {
            entries,
            polls,
            stop_after_first: None,
            pending_after_entries: false,
        }
    }
}

impl Incoming for ReadyIncoming {
    type Io = DuplexStream;

    async fn accept(&mut self) -> IncomingAccept<Self::Io> {
        // This counter records future polling, not construction of accept().
        self.polls.fetch_add(1, Ordering::SeqCst);
        if let Some(entry) = self.entries.pop_front() {
            if let Some(stop) = self.stop_after_first.take() {
                stop.send(()).expect("shutdown receiver exists");
            }
            return Some(entry);
        }
        if self.pending_after_entries {
            std::future::pending().await
        } else {
            None
        }
    }

    fn peer(&self, _io: &Self::Io, _remote: Option<SocketAddr>) -> ConnectionInfo {
        ConnectionInfo::new()
            .with_remote_addr("192.0.2.1:8".parse().expect("remote"))
            .with_local_addr("127.0.0.1:9".parse().expect("local"))
            .with_peer_cred(PeerCred::new(42, 43, None))
            .with_scheme("https")
    }
}

#[tokio::test]
async fn already_ready_shutdown_does_not_accept_ready_backlog() {
    let polls = Arc::new(AtomicUsize::new(0));
    Server::new(GreeterServer::new(Echo))
        .serve_with_incoming_shutdown(
            ReadyIncoming::backlog(Arc::clone(&polls)),
            std::future::ready(()),
        )
        .await
        .expect("shutdown succeeds");
    assert_eq!(
        polls.load(Ordering::SeqCst),
        0,
        "shutdown must win before accepting"
    );
}

#[tokio::test]
async fn shutdown_after_first_accept_preempts_remaining_ready_backlog() {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut incoming = ReadyIncoming::backlog(Arc::clone(&polls));
    let (stop, stopped) = oneshot::channel();
    incoming.stop_after_first = Some(stop);
    Server::new(GreeterServer::new(Echo))
        .serve_with_incoming_shutdown(incoming, async move {
            stopped.await.expect("shutdown sent");
        })
        .await
        .expect("shutdown succeeds");
    assert_eq!(
        polls.load(Ordering::SeqCst),
        1,
        "shutdown must precede another ready accept"
    );
}

#[tokio::test]
async fn pending_shutdown_preserves_incoming_eof() {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut incoming = ReadyIncoming::backlog(Arc::clone(&polls));
    incoming.entries.clear();
    Server::new(GreeterServer::new(Echo))
        .serve_with_incoming_shutdown(incoming, std::future::pending())
        .await
        .expect("EOF drains cleanly");
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn pending_shutdown_preserves_incoming_error() {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut incoming = ReadyIncoming::backlog(Arc::clone(&polls));
    incoming.entries = [Err(Status::unavailable("incoming marker"))].into();
    let error = Server::new(GreeterServer::new(Echo))
        .serve_with_incoming_shutdown(incoming, std::future::pending())
        .await
        .expect_err("accept error surfaces");
    assert_eq!(error.code(), Code::Unavailable);
    assert_eq!(error.message(), "incoming marker");
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn pending_shutdown_preserves_peer_metadata() {
    let polls = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let incoming = ReadyIncoming {
        entries: [Ok((
            server_io,
            Some("203.0.113.1:7".parse().expect("accepted")),
        ))]
        .into(),
        polls: Arc::clone(&polls),
        stop_after_first: None,
        pending_after_entries: true,
    };
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        Server::new(GreeterServer::new(Echo))
            .serve_with_incoming_shutdown(incoming, async move {
                stopped.await.expect("shutdown sent");
            })
            .await
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("client connects");
    let client = GreeterClient::new(channel);
    let response = client
        .say_hello(Request::new(HelloRequest::new()))
        .await
        .expect("metadata reaches handler");
    assert_eq!(response.get_ref().message(), "metadata preserved");
    drop(client);
    stop.send(()).expect("server is listening for shutdown");
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("bounded drain")
        .expect("server task")
        .expect("server drains");
    assert!(polls.load(Ordering::SeqCst) >= 1);
}

#[tokio::test(flavor = "current_thread")]
async fn ready_incoming_yields_to_a_scheduled_shutdown_task() {
    const READY_BACKLOG: usize = 4096;
    let polls = Arc::new(AtomicUsize::new(0));
    let mut incoming = ReadyIncoming::backlog(Arc::clone(&polls));
    for _ in BACKLOG..READY_BACKLOG {
        let (io, peer) = tokio::io::duplex(64);
        drop(peer);
        incoming.entries.push_back(Ok((io, None)));
    }
    let (stop, stopped) = oneshot::channel();
    let server =
        Server::new(GreeterServer::new(Echo)).serve_with_incoming_shutdown(incoming, async move {
            stopped.await.expect("shutdown sent");
        });
    // The server branch runs first. The stopper can send only after it yields,
    // even though the incoming backlog can produce every entry immediately.
    let (result, accepted_before_shutdown) = tokio::join!(
        biased;
        server,
        async {
            let accepted = polls.load(Ordering::SeqCst);
            let _shutdown_result = stop.send(());
            accepted
        }
    );
    result.expect("server drains");
    assert!(
        accepted_before_shutdown > 0 && accepted_before_shutdown < READY_BACKLOG,
        "stopper must run while ready connections remain: {accepted_before_shutdown}"
    );
    assert!(
        polls.load(Ordering::SeqCst) < READY_BACKLOG,
        "shutdown must stop before exhausting the finite ready backlog"
    );
}
