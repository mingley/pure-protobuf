
#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, reason = "isolated sender limit controls")]
mod sender_limit_diagnostics {
    use super::{SenderLimits, Streaming};
    use crate::{CodecMessage, MessageLimits, Status};
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    #[derive(Debug)]
    struct Probe {
        calls: Arc<AtomicUsize>,
        len: usize,
    }
    impl CodecMessage for Probe {
        fn encoded_len(&self) -> usize {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.len
        }
        fn encode_payload<W: pbrs::WireOut>(&self, _: &mut W) -> Result<(), Status> { Ok(()) }
        fn decode_payload(_: bytes::Bytes) -> Result<Self, Status> { Ok(Self::empty()) }
        fn empty() -> Self { Self { calls: Arc::new(AtomicUsize::new(0)), len: 0 } }
    }

    fn assert_sender_traits<T: Send + Sync + Unpin>() {}
    fn check_debug(limits: MessageLimits) {
        let packed = SenderLimits::new(limits);
        assert_eq!(packed.into_message_limits(), limits);
        let (tx, rx) = Streaming::<Probe>::channel(1);
        let tx = tx.with_limits(limits);
        let mut cloned = tx.clone();
        cloned.set_compress(true);
        assert!(!tx.compress());
        assert!(cloned.compress());
        assert_eq!(format!("{tx:?}"), format!("StreamSender {{ closed: false, limits: {limits:?}, compress: false }}"));
        assert_eq!(format!("{cloned:?}"), format!("StreamSender {{ closed: false, limits: {limits:?}, compress: true }}"));
        drop(rx);
        assert!(tx.is_closed() && cloned.is_closed());
        assert_eq!(format!("{tx:?}"), format!("StreamSender {{ closed: true, limits: {limits:?}, compress: false }}"));
    }

    #[test]
    fn all_limit_values_and_public_debug_round_trip() {
        assert_sender_traits::<super::StreamSender<Probe>>();
        let values = [None, Some(0), Some(1), Some(usize::MAX)];
        for decoding in values {
            for encoding in values {
                for frames in values {
                    let mut limits = MessageLimits::unlimited();
                    if let Some(v) = decoding { limits = limits.with_max_decoding(v); }
                    if let Some(v) = encoding { limits = limits.with_max_encoding(v); }
                    if let Some(v) = frames { limits = limits.with_max_compression_frames(v); }
                    check_debug(limits);
                    #[cfg(feature = "zstd")]
                    for window in [0, 1, u64::MAX] {
                        check_debug(limits.with_max_zstd_window(window));
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn send_clone_limit_error_and_length_walk_semantics() {
        for (max, len, expected_walks, error) in [
            (None, usize::MAX, 0, false),
            (Some(usize::MAX), usize::MAX, 1, false),
            (Some(0), 0, 1, false),
            (Some(0), 1, 1, true),
            (Some(usize::MAX - 1), usize::MAX, 1, true),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let (tx, mut rx) = Streaming::<Probe>::channel(1);
            let limits = max.map_or(MessageLimits::unlimited(), |n| MessageLimits::unlimited().with_max_encoding(n));
            let tx = tx.with_limits(limits);
            let mut sender = tx.clone();
            sender.set_compress(true);
            assert!(!tx.compress() && sender.compress());
            let result = sender.send(Probe { calls: calls.clone(), len }).await;
            assert_eq!(calls.load(Ordering::SeqCst), expected_walks, "encoded_len call policy for {max:?}");
            if error {
                let err = result.expect_err("encoding limit");
                assert_eq!(err.code(), crate::Code::ResourceExhausted);
                assert_eq!(err.message(), format!("encoded message length {len} exceeds limit {}", max.expect("cap")));
                let queued = rx.next_framed().await.expect_err("same queued error");
                assert_eq!(queued.code(), err.code());
                assert_eq!(queued.message(), err.message());
            } else {
                result.expect("send within limit");
                let frame = rx.next_framed().await.expect("message").expect("frame");
                assert!(frame.compressed);
                assert_eq!(frame.message.len, len);
            }
        }
    }

    #[tokio::test]
    async fn closed_receiver_and_explicit_failure_keep_existing_results() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = Streaming::<Probe>::channel(1);
        let tx = tx.with_limits(MessageLimits::unlimited().with_max_encoding(0));
        drop(rx);
        let err = tx.send(Probe { calls: calls.clone(), len: 1 }).await.expect_err("limit precedes closed queue");
        assert_eq!(err.code(), crate::Code::ResourceExhausted);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let err = tx.send(Probe { calls, len: 0 }).await.expect_err("closed queue");
        assert_eq!(err.code(), crate::Code::Cancelled);

        let (tx, mut rx) = Streaming::<Probe>::channel(1);
        let mut status = Status::invalid_argument("explicit");
        status.metadata_mut().insert("x-proof", "retained").expect("metadata");
        tx.clone().fail(status).await;
        let err = rx.message().await.expect_err("queued explicit failure");
        assert_eq!(err.code(), crate::Code::InvalidArgument);
        assert_eq!(err.message(), "explicit");
        assert_eq!(err.metadata().get("x-proof"), Some("retained"));
    }
}
