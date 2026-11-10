//! Bounded client-default response reserve and explicit budget controls.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "bounded protocol test fixture"
)]
use crate::h2_backend as h2;
use bytes::Bytes;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

type Opened = (h2::client::ResponseFuture, h2::SendStream<Bytes>);
const WAIT: Duration = Duration::from_secs(10);
const FRAME: [u8; 9] = [0, 0, 0, 0, 4, 10, 2, b'o', b'k'];
const TRAILER: &[u8] = b"\x00\x0bgrpc-status\x010";

#[derive(Debug)]
struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

async fn read_frame(io: &mut DuplexStream) -> Frame {
    tokio::time::timeout(WAIT, async {
        let mut h = [0; 9];
        io.read_exact(&mut h).await.unwrap();
        let len = usize::from(h[0]) * 65536 + usize::from(h[1]) * 256 + usize::from(h[2]);
        assert!(len <= 16384);
        let mut payload = vec![0; len];
        io.read_exact(&mut payload).await.unwrap();
        Frame {
            kind: h[3],
            flags: h[4],
            stream: u32::from_be_bytes(h[5..9].try_into().unwrap()) & 0x7fff_ffff,
            payload,
        }
    })
    .await
    .expect("fixture frame timed out")
}

async fn write_frame(io: &mut DuplexStream, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let len = u32::try_from(payload.len()).unwrap();
    let mut frame = Vec::with_capacity(9 + payload.len());
    frame.extend_from_slice(&len.to_be_bytes()[1..]);
    frame.extend_from_slice(&[kind, flags]);
    frame.extend_from_slice(&stream.to_be_bytes());
    frame.extend_from_slice(payload);
    io.write_all(&frame).await.unwrap();
}

async fn setup(
    budget: usize,
    n: usize,
) -> (DuplexStream, Vec<Opened>, tokio::task::JoinHandle<()>) {
    setup_reserve(budget, n, false).await
}

async fn setup_reserve(
    budget: usize,
    n: usize,
    reserve: bool,
) -> (DuplexStream, Vec<Opened>, tokio::task::JoinHandle<()>) {
    let (client_io, mut peer) = tokio::io::duplex(65536);
    let mut builder = h2::client::Builder::new();
    builder
        .initial_max_send_streams(0)
        .data_frame_budget(budget);
    builder.default_small_data_reserve(reserve);
    let (mut send, connection) = builder.handshake::<_, Bytes>(client_io).await.unwrap();
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let mut preface = [0; 24];
    peer.read_exact(&mut preface).await.unwrap();
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    assert_eq!(read_frame(&mut peer).await.kind, 4);
    let mut settings = vec![0, 3];
    settings.extend_from_slice(&2048_u32.to_be_bytes());
    write_frame(&mut peer, 4, 0, 0, &settings).await;
    loop {
        let next = read_frame(&mut peer).await;
        assert_ne!(next.kind, 7, "unexpected GOAWAY during setup: {next:?}");
        if next.kind == 4 && next.flags == 1 {
            break;
        }
    }
    let mut opened = Vec::with_capacity(n);
    for i in 0..n {
        let request = http::Request::builder()
            .uri("http://localhost/probe")
            .body(())
            .unwrap();
        let response = tokio::time::timeout(WAIT, send.send_request_when_ready(request, true))
            .await
            .expect("open stalled")
            .unwrap();
        opened.push(response);
        loop {
            let next = read_frame(&mut peer).await;
            assert_ne!(next.kind, 7, "unexpected GOAWAY during opens: {next:?}");
            if next.kind == 1 {
                assert_eq!(next.stream, u32::try_from(2 * i + 1).unwrap());
                break;
            }
        }
    }
    (peer, opened, driver)
}

async fn replies(peer: &mut DuplexStream, start: usize, end: usize, payload: &[u8]) {
    for index in start..end {
        let id = u32::try_from(2 * index + 1).unwrap();
        write_frame(peer, 1, 4, id, &[0x88]).await; // HTTP/2 :status 200
        write_frame(peer, 0, 0, id, payload).await; // non-final DATA
        write_frame(peer, 1, 5, id, TRAILER).await; // grpc-status: 0 + END_STREAM
    }
    peer.flush().await.unwrap();
}

async fn ping_ack(peer: &mut DuplexStream) {
    let opaque = b"barrier!";
    write_frame(peer, 6, 0, 0, opaque).await;
    peer.flush().await.unwrap();
    loop {
        let next = read_frame(peer).await;
        assert_ne!(next.kind, 7, "unexpected GOAWAY: {next:?}");
        if next.kind == 6 && next.flags == 1 {
            assert_eq!(next.payload, opaque);
            break;
        }
    }
}

async fn expect_budget_goaway(peer: &mut DuplexStream) {
    loop {
        let next = read_frame(peer).await;
        if next.kind == 6 && next.flags == 1 {
            panic!("expected budget GOAWAY before the processing barrier");
        }
        if next.kind == 7 {
            assert_eq!(next.stream, 0);
            assert!(next.payload.len() >= 8);
            assert_eq!(
                u32::from_be_bytes(next.payload[4..8].try_into().unwrap()),
                11
            );
            assert_eq!(&next.payload[8..], b"too_many_data_frames");
            break;
        }
    }
}

#[tokio::test]
async fn diagnostic_default_103_fit_but_104_unpolled_responses_goaway() {
    let (mut peer, opened, driver) = setup(25_600, 104).await;
    replies(&mut peer, 0, 103, &FRAME).await;
    ping_ack(&mut peer).await;
    replies(&mut peer, 103, 104, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    assert_eq!(opened.len(), 104);
    println!("BURST_DIAGNOSTIC default frames_fit=103 rejected=104 overhead_per_frame=247");
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn diagnostic_consuming_one_response_releases_room() {
    let (mut peer, mut opened, driver) = setup(25_600, 104).await;
    replies(&mut peer, 0, 103, &FRAME).await;
    ping_ack(&mut peer).await;
    let (response, send) = opened.remove(0);
    let mut body = tokio::time::timeout(WAIT, response)
        .await
        .unwrap()
        .unwrap()
        .into_body();
    assert_eq!(body.data().await.unwrap().unwrap().as_ref(), &FRAME);
    assert_eq!(
        body.trailers()
            .await
            .unwrap()
            .unwrap()
            .get("grpc-status")
            .unwrap(),
        "0"
    );
    replies(&mut peer, 103, 104, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("BURST_DIAGNOSTIC releasing one queued frame permits one replacement");
    drop((body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn diagnostic_explicit_budget_and_large_frame_controls() {
    let (mut peer, opened, driver) = setup(512, 3).await;
    replies(&mut peer, 0, 2, &FRAME).await;
    ping_ack(&mut peer).await;
    replies(&mut peer, 2, 3, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    drop(opened);
    driver.abort();

    let (mut peer, opened, driver) = setup(0, 104).await;
    replies(&mut peer, 0, 104, &[7; 256]).await;
    ping_ack(&mut peer).await;
    assert_eq!(opened.len(), 104);
    println!(
        "BURST_DIAGNOSTIC budget_512 rejects third nine-byte frame; budget_0 accepts 256-byte frames"
    );
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn default_reserve_accepts_normal_concurrent_response_bursts() {
    for n in [256, 512, 1024] {
        let (mut peer, opened, driver) = setup_reserve(25_600, n, true).await;
        replies(&mut peer, 0, n, &FRAME).await;
        ping_ack(&mut peer).await;
        assert_eq!(opened.len(), n);
        println!("RESERVE_PROCESSED {n}");
        drop(opened);
        driver.abort();
    }
}

#[tokio::test]
async fn default_reserve_is_capped_and_shared_budget_stays_bounded() {
    let (mut peer, opened, driver) = setup_reserve(25_600, 1128, true).await;
    replies(&mut peer, 0, 1127, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_BOUNDARY_PROCESSED 1127");
    replies(&mut peer, 1127, 1128, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn reserved_slot_returns_after_response_is_read() {
    let (mut peer, mut opened, driver) = setup_reserve(25_600, 1128, true).await;
    replies(&mut peer, 0, 1127, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_READ_BOUNDARY_PROCESSED");
    let (response, send) = opened.remove(0);
    let mut body = tokio::time::timeout(WAIT, response)
        .await
        .unwrap()
        .unwrap()
        .into_body();
    assert_eq!(body.data().await.unwrap().unwrap().as_ref(), &FRAME);
    assert_eq!(
        body.trailers()
            .await
            .unwrap()
            .unwrap()
            .get("grpc-status")
            .unwrap(),
        "0"
    );
    replies(&mut peer, 1127, 1128, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_READ_RELEASED");
    drop((body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn reserved_slot_returns_after_response_is_dropped() {
    let (mut peer, mut opened, driver) = setup_reserve(25_600, 1128, true).await;
    replies(&mut peer, 0, 1127, &FRAME).await;
    ping_ack(&mut peer).await;
    let (response, send) = opened.remove(0);
    drop((response, send));
    replies(&mut peer, 1127, 1128, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_DROP_RELEASED");
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn only_one_reserved_frame_per_stream() {
    let (mut peer, opened, driver) = setup_reserve(512, 3, true).await;
    for id in [1, 3] {
        write_frame(&mut peer, 1, 4, id, &[0x88]).await;
        write_frame(&mut peer, 0, 0, id, &FRAME).await;
        write_frame(&mut peer, 0, 0, id, &FRAME).await;
    }
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    println!("RESERVE_TWO_STREAMS_PROCESSED");
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    println!("RESERVE_ONE_PER_STREAM");
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn default_reserve_enables_104th_unpolled_response() {
    let (mut peer, opened, driver) = setup_reserve(25_600, 104, true).await;
    replies(&mut peer, 0, 103, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_103_PROCESSED");
    replies(&mut peer, 103, 104, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_104_PROCESSED");
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn diagnostic_locally_reset_discarded_data_uses_budget() {
    let (mut peer, mut opened, driver) = setup(512, 2).await;
    let (response, mut send) = opened.remove(0);
    send.send_reset(h2::Reason::CANCEL);
    loop {
        let next = read_frame(&mut peer).await;
        assert_ne!(next.kind, 7, "unexpected GOAWAY before reset: {next:?}");
        if next.kind == 3 && next.stream == 1 {
            assert_eq!(next.payload, 8_u32.to_be_bytes());
            break;
        }
    }
    // These model DATA already in flight when the peer receives our reset.
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    replies(&mut peer, 1, 2, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    println!("BURST_DIAGNOSTIC locally-reset discarded frames consumed the 512-byte budget");
    drop((response, send, opened));
    driver.abort();
}

#[tokio::test]
async fn diagnostic_unwatched_discarded_data_uses_budget() {
    let (mut peer, mut opened, driver) = setup(512, 2).await;
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    let (response, send) = opened.remove(0);
    let body = tokio::time::timeout(WAIT, response)
        .await
        .unwrap()
        .unwrap()
        .into_body();
    drop(body); // clears receive queue and marks it unwatched, send ref remains.
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    replies(&mut peer, 1, 2, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    println!("BURST_DIAGNOSTIC unwatched discarded frames consumed the 512-byte budget");
    drop((send, opened));
    driver.abort();
}

#[tokio::test]
async fn diagnostic_dropping_buffered_data_replenishes_budget() {
    let (mut peer, mut opened, driver) = setup(512, 2).await;
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    let (response, send) = opened.remove(0);
    let body = tokio::time::timeout(WAIT, response)
        .await
        .unwrap()
        .unwrap()
        .into_body();
    drop(body); // queued events are released as part of clearing the buffer.
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    println!(
        "BURST_DIAGNOSTIC dropping two buffered frames restored two charges, with the bound retained"
    );
    drop((send, opened));
    driver.abort();
}

#[tokio::test]
async fn diagnostic_empty_frames_keep_separate_connection_lifetime_limit() {
    let (mut peer, opened, driver) = setup(0, 2).await;
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    for _ in 0..50 {
        write_frame(&mut peer, 0, 0, 1, &[]).await;
        write_frame(&mut peer, 0, 0, 3, &[]).await;
    }
    write_frame(&mut peer, 0, 0, 1, &[7; 256]).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    write_frame(&mut peer, 0, 0, 3, &[]).await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    println!(
        "BURST_DIAGNOSTIC the 101st empty frame fails across streams despite a large intervening frame"
    );
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn reset_and_late_data_do_not_leak_a_reserve_slot() {
    let (mut peer, mut opened, driver) = setup_reserve(25_600, 1128, true).await;
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    let (response, mut send) = opened.remove(0);
    send.send_reset(h2::Reason::CANCEL);
    loop {
        let next = read_frame(&mut peer).await;
        assert_ne!(next.kind, 7, "unexpected GOAWAY before reset: {next:?}");
        if next.kind == 3 && next.stream == 1 {
            break;
        }
    }
    // Keep existing discarded-frame charging; the frame must not use a reserve.
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    println!("RESERVE_RESET_LATE_DATA_PROCESSED");
    // Reset alone retains its previously queued DATA while handles remain.
    // One discarded frame and 102 ordinary charges fill the shared budget.
    replies(&mut peer, 1, 1126, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_RESET_HELD_BOUNDARY_PROCESSED");
    // Dropping the last handles clears the queued event and returns its slot.
    drop((response, send));
    replies(&mut peer, 1126, 1127, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_RESET_DROPPED_SLOT_RELEASED");
    replies(&mut peer, 1127, 1128, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    drop(opened);
    driver.abort();
}

#[tokio::test]
async fn dropped_receiver_and_late_data_do_not_leak_a_reserve_slot() {
    let (mut peer, mut opened, driver) = setup_reserve(25_600, 1128, true).await;
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    let (response, send) = opened.remove(0);
    let body = tokio::time::timeout(WAIT, response)
        .await
        .unwrap()
        .unwrap()
        .into_body();
    drop(body);
    // Keep existing discarded-frame charging; the frame must not use a reserve.
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    replies(&mut peer, 1, 1127, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("RESERVE_UNWATCHED_BOUNDARY_PROCESSED");
    replies(&mut peer, 1127, 1128, &FRAME).await;
    expect_budget_goaway(&mut peer).await;
    drop((send, opened));
    driver.abort();
}

#[tokio::test]
async fn each_new_connection_starts_with_a_fresh_reserve() {
    for generation in 0..2 {
        let (mut peer, opened, driver) = setup_reserve(25_600, 104, true).await;
        replies(&mut peer, 0, 104, &FRAME).await;
        ping_ack(&mut peer).await;
        println!("RESERVE_FRESH_CONNECTION {generation}");
        drop(opened);
        driver.abort();
    }
}

async fn reset_first(peer: &mut DuplexStream, opened: &mut Vec<Opened>) -> Opened {
    let (response, mut send) = opened.remove(0);
    send.send_reset(h2::Reason::CANCEL);
    loop {
        let frame = read_frame(peer).await;
        assert_ne!(frame.kind, 7, "unexpected GOAWAY while resetting");
        if frame.kind == 3 && frame.stream == 1 {
            assert_eq!(frame.payload, 8_u32.to_be_bytes());
            break;
        }
    }
    (response, send)
}

async fn response_body(opened: &mut Vec<Opened>) -> (h2::RecvStream, h2::SendStream<Bytes>) {
    let (response, send) = opened.remove(0);
    let body = tokio::time::timeout(WAIT, response)
        .await
        .unwrap()
        .unwrap()
        .into_body();
    (body, send)
}

async fn read_small(body: &mut h2::RecvStream) {
    let data = tokio::time::timeout(WAIT, body.data())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(data.as_ref(), &FRAME);
}

#[tokio::test]
async fn aggregate_budget_reads_repay_discarded_charge() {
    let (mut peer, mut opened, driver) = setup(512, 3).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // queued: 247
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // discarded: 247
    ping_ack(&mut peer).await; // only 18 units remain.
    println!("AGGREGATE_READ_INITIAL_BOUNDARY");

    let (mut body, send) = response_body(&mut opened).await;
    read_small(&mut body).await; // queue refund and discarded-work credit
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_READ_TWO_REPLACEMENTS");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_queue_clear_does_not_credit_discarded_charge() {
    let (mut peer, mut opened, driver) = setup(512, 3).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    ping_ack(&mut peer).await;
    let (body, send) = response_body(&mut opened).await;
    drop(body); // refund exactly the queued 247 units, no discarded credit
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_CLEAR_ONE_REPLACEMENT");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_reserved_read_recredits_discarded_charge() {
    let (mut peer, mut opened, driver) = setup_reserve(247, 3, true).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // reserved, not charged
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // discarded: all 247 units
    ping_ack(&mut peer).await;
    println!("AGGREGATE_RESERVED_INITIAL_BOUNDARY");

    let (mut body, send) = response_body(&mut opened).await;
    let data = tokio::time::timeout(WAIT, std::future::poll_fn(|cx| body.poll_data_released(cx)))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(data.as_ref(), &FRAME);
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // slot
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // restored shared budget
    ping_ack(&mut peer).await;
    println!("AGGREGATE_RESERVED_READ_SLOT_AND_CREDIT");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_reserved_clear_returns_slot_only() {
    let (mut peer, mut opened, driver) = setup_reserve(247, 3, true).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    ping_ack(&mut peer).await;
    let (body, send) = response_body(&mut opened).await;
    drop(body); // return slot only, discarded work still fills the budget
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_RESERVED_CLEAR_SLOT_ONLY");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_final_read_recredits_discarded_charge() {
    let (mut peer, mut opened, driver) = setup(247, 3).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 1, 3, &FRAME).await; // final is not charged
    ping_ack(&mut peer).await;
    println!("AGGREGATE_FINAL_INITIAL_BOUNDARY");
    let (mut body, send) = response_body(&mut opened).await;
    read_small(&mut body).await;
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_FINAL_READ_CREDIT");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_large_credit_does_not_leave_repayable_debt() {
    let (mut peer, mut opened, driver) = setup(512, 4).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // 247 debt
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &[7; 503]).await; // 247 credit
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // consume 247
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 1, 5, &FRAME).await; // final, no charge
    ping_ack(&mut peer).await;
    println!("AGGREGATE_LARGE_INITIAL_BOUNDARY");
    let (queued_response, queued_send) = opened.remove(0); // keep stream 3
    let (mut final_body, final_send) = response_body(&mut opened).await;
    read_small(&mut final_body).await; // cannot credit the old debt again
    write_frame(&mut peer, 1, 4, 7, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 7, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_LARGE_ONE_REPLACEMENT");
    write_frame(&mut peer, 0, 0, 7, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((
        reset,
        queued_response,
        queued_send,
        final_body,
        final_send,
        opened,
    ));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_every_queue_refund_clamps_debt() {
    let (mut peer, mut opened, driver) = setup(512, 5).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // discarded 247
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // queued 247
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &[7; 356]).await; // 100 credit
    ping_ack(&mut peer).await;
    let (body, send) = response_body(&mut opened).await; // stream 3
    drop(body); // refund 247; only 147 units of debt remain outstanding

    write_frame(&mut peer, 1, 4, 7, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 7, &FRAME).await; // consume 247
    write_frame(&mut peer, 1, 4, 9, &[0x88]).await;
    write_frame(&mut peer, 0, 1, 9, &FRAME).await;
    ping_ack(&mut peer).await;
    let (large_response, large_send) = opened.remove(0); // stream 5
    let (charged_response, charged_send) = opened.remove(0); // stream 7
    let (mut final_body, final_send) = response_body(&mut opened).await; // stream 9
    read_small(&mut final_body).await; // credit remaining 147, not 247
    println!("AGGREGATE_REFUND_FINAL_READ");
    write_frame(&mut peer, 0, 0, 7, &[3; 100]).await; // costs 156, fits
    ping_ack(&mut peer).await;
    println!("AGGREGATE_REFUND_ONE_REPLACEMENT");
    write_frame(&mut peer, 0, 0, 7, &[3; 100]).await; // second exceeds 109
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((
        reset,
        send,
        large_response,
        large_send,
        charged_response,
        charged_send,
        final_body,
        final_send,
        opened,
    ));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_repeated_reads_prevent_discarded_debt_accumulation() {
    let (mut peer, mut opened, driver) = setup(512, 11).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    for index in 0..10 {
        let id = 2 * index + 3;
        write_frame(&mut peer, 0, 0, 1, &FRAME).await;
        write_frame(&mut peer, 1, 4, id, &[0x88]).await;
        write_frame(&mut peer, 0, 0, id, &FRAME).await;
        write_frame(&mut peer, 1, 5, id, TRAILER).await;
        ping_ack(&mut peer).await;
        let (mut body, send) = response_body(&mut opened).await;
        read_small(&mut body).await;
        drop((body, send));
    }
    println!("AGGREGATE_TEN_ROUNDS_PROGRESS");
    drop((reset, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_server_unwatched_charge_repaid_by_read() {
    let (mut peer, server_io) = tokio::io::duplex(65_536);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (body_tx, body_rx) = tokio::sync::oneshot::channel();
    let driver = tokio::spawn(async move {
        let mut builder = h2::server::Builder::new();
        builder.data_frame_budget(512);
        let mut connection = builder.handshake::<_, Bytes>(server_io).await.unwrap();
        let (request, response1) = connection.accept().await.unwrap().unwrap();
        drop(request.into_body());
        ready_tx.send(()).unwrap();
        let (request, response2) = connection.accept().await.unwrap().unwrap();
        body_tx.send(request.into_body()).unwrap();
        while let Some(Ok(_)) = connection.accept().await {}
        drop((response1, response2));
    });
    peer.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    write_frame(&mut peer, 4, 0, 0, &[]).await;
    loop {
        let next = read_frame(&mut peer).await;
        assert_ne!(next.kind, 7, "unexpected GOAWAY during setup");
        if next.kind == 4 && next.flags == 0 {
            write_frame(&mut peer, 4, 1, 0, &[]).await;
            break;
        }
    }
    // Static HPACK entries: :method POST, :scheme http, :path /.
    write_frame(&mut peer, 1, 4, 1, &[0x83, 0x86, 0x84]).await;
    peer.flush().await.unwrap();
    tokio::time::timeout(WAIT, ready_rx).await.unwrap().unwrap();
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // unwatched: 247
    write_frame(&mut peer, 1, 4, 3, &[0x83, 0x86, 0x84]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // queued: 247
    ping_ack(&mut peer).await;
    println!("AGGREGATE_SERVER_INITIAL_BOUNDARY");
    let mut body = tokio::time::timeout(WAIT, body_rx).await.unwrap().unwrap();
    read_small(&mut body).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_SERVER_TWO_REPLACEMENTS");
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop(body);
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_explicit_saturation_still_rejects_before_read() {
    let (mut peer, mut opened, driver) = setup(247, 2).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // consumes the complete budget
    ping_ack(&mut peer).await;
    println!("AGGREGATE_EXPLICIT_SATURATED");
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await; // no read can rescue this frame
    drop((reset, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_default_reserve_read_recovers_discarded_work() {
    let (mut peer, mut opened, driver) = setup_reserve(25_600, 3, true).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // one reserved frame
    for _ in 0..103 {
        write_frame(&mut peer, 0, 0, 1, &FRAME).await; // 25,441 units of debt
    }
    ping_ack(&mut peer).await; // only 159 of the base 25,600 remain
    println!("AGGREGATE_DEFAULT_SATURATED_BEFORE_RESERVED_READ");
    let (mut body, send) = response_body(&mut opened).await;
    read_small(&mut body).await; // release reserve slot and credit 247 debt
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // reserve
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // 247 fits in 406
    ping_ack(&mut peer).await;
    println!("AGGREGATE_DEFAULT_RESERVED_READ_CREDIT");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // 247 exceeds 159
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_failed_released_poll_does_not_credit_discarded_work() {
    let (mut peer, mut opened, driver) = setup_reserve(247, 3, true).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 3, &FRAME).await; // reserved
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // budget fully charged
    ping_ack(&mut peer).await;
    let (mut body, send) = response_body(&mut opened).await;
    // This public flow-control operation permits release before the queued
    // event is read. A later read-and-release must fail to release it twice.
    body.flow_control().release_capacity(FRAME.len()).unwrap();
    let error = tokio::time::timeout(WAIT, std::future::poll_fn(|cx| body.poll_data_released(cx)))
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.reason(), Some(h2::Reason::INTERNAL_ERROR));
    println!("AGGREGATE_FAILED_RELEASE_NO_READ");
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // slot returned
    ping_ack(&mut peer).await;
    println!("AGGREGATE_FAILED_RELEASE_SLOT_RETURNED");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await; // debt remains charged
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_final_data_does_not_refund_an_unpaid_queue_charge() {
    let (mut peer, mut opened, driver) = setup(512, 3).await;
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await; // charged 247
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 1, 3, &FRAME).await; // final, no charge
    ping_ack(&mut peer).await;
    let (queued_response, queued_send) = opened.remove(0);
    let (mut final_body, final_send) = response_body(&mut opened).await;
    read_small(&mut final_body).await;
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_FINAL_NO_PHANTOM_REFUND");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((queued_response, queued_send, final_body, final_send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_padded_reads_repay_discarded_charge() {
    let (mut peer, mut opened, driver) = setup(512, 3).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    let padded: [u8; 13] = [3, 0, 0, 0, 0, 4, 10, 2, b'o', b'k', 0, 0, 0];
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 8, 3, &padded).await; // queued, 9 visible bytes
    write_frame(&mut peer, 0, 8, 1, &padded).await; // discarded, 9 visible bytes
    ping_ack(&mut peer).await;
    let (mut body, send) = response_body(&mut opened).await;
    read_small(&mut body).await; // releases the queued cost and 247 debt
    write_frame(&mut peer, 1, 4, 5, &[0x88]).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    ping_ack(&mut peer).await;
    println!("AGGREGATE_PADDED_TWO_REPLACEMENTS");
    write_frame(&mut peer, 0, 0, 5, &FRAME).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await;
    drop((reset, body, send, opened));
    driver.abort();
}

#[tokio::test]
async fn aggregate_budget_counts_visible_payload_not_padding() {
    let (mut peer, mut opened, driver) = setup(490, 2).await;
    let reset = reset_first(&mut peer, &mut opened).await;
    let padded: [u8; 13] = [3, 0, 0, 0, 0, 4, 10, 2, b'o', b'k', 0, 0, 0];
    write_frame(&mut peer, 0, 8, 1, &padded).await; // 247, not 243
    ping_ack(&mut peer).await;
    println!("AGGREGATE_PADDED_FIRST_CHARGE");
    write_frame(&mut peer, 1, 4, 3, &[0x88]).await;
    write_frame(&mut peer, 0, 8, 3, &padded).await;
    write_frame(&mut peer, 6, 0, 0, b"barrier!").await;
    peer.flush().await.unwrap();
    expect_budget_goaway(&mut peer).await; // another 247 exceeds 243
    drop((reset, opened));
    driver.abort();
}

#[tokio::test]
async fn miri_reserved_padded_then_unwatched_data() {
    let (mut peer, mut opened, driver) = setup_reserve(512, 2, true).await;
    let mut padded = vec![1];
    padded.extend_from_slice(&FRAME);
    padded.push(0);
    write_frame(&mut peer, 1, 4, 1, &[0x88]).await;
    write_frame(&mut peer, 0, 8, 1, &padded).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;

    let (response, send) = opened.remove(0);
    let mut body = response.await.unwrap().into_body();
    assert_eq!(body.data().await.unwrap().unwrap().as_ref(), &FRAME);

    // Queue a second reserved frame, then drop the receiver. Late DATA is
    // discarded while the send handle keeps the stream entry reachable.
    write_frame(&mut peer, 0, 8, 1, &padded).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;
    drop(body);
    write_frame(&mut peer, 0, 8, 1, &padded).await;
    peer.flush().await.unwrap();
    ping_ack(&mut peer).await;

    replies(&mut peer, 1, 2, &FRAME).await;
    ping_ack(&mut peer).await;
    let (response, send2) = opened.remove(0);
    let mut body2 = response.await.unwrap().into_body();
    assert_eq!(body2.data().await.unwrap().unwrap().as_ref(), &FRAME);
    println!("MIRI_PADDED_AND_UNWATCHED_PROCESSED");
    drop((send, send2, body2, opened));
    driver.abort();
}

#[tokio::test]
async fn miri_reservation_rolls_back_on_content_length_error() {
    let (mut peer, mut opened, driver) = setup_reserve(512, 2, true).await;
    // :status 200 and content-length: 0, then a nonempty frame. This passes
    // reserve eligibility but recv_data must reset the stream before enqueue.
    write_frame(&mut peer, 1, 4, 1, &[0x88, 0x0f, 0x0d, 1, b'0']).await;
    write_frame(&mut peer, 0, 0, 1, &FRAME).await;
    peer.flush().await.unwrap();
    loop {
        let next = read_frame(&mut peer).await;
        assert_ne!(next.kind, 7, "unexpected connection error");
        if next.kind == 3 && next.stream == 1 {
            break;
        }
    }
    println!("MIRI_RESERVE_ROLLBACK_RESET");
    replies(&mut peer, 1, 2, &FRAME).await;
    ping_ack(&mut peer).await;
    let (_bad_response, bad_send) = opened.remove(0);
    let (response, send) = opened.remove(0);
    let mut body = response.await.unwrap().into_body();
    assert_eq!(body.data().await.unwrap().unwrap().as_ref(), &FRAME);
    drop((bad_send, send, body));
    driver.abort();
}
