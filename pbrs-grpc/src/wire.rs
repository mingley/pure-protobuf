//! gRPC-over-HTTP/2 protocol: request/response headers, data frames, status
//! trailers, and the stream pumps that connect them to [`Streaming`].

pub(crate) mod encode;
pub(crate) mod frame_reader;
pub(crate) mod headers;
pub(crate) mod out_batch;
pub(crate) mod send;

pub(crate) use encode::encode_msg;
pub(crate) use frame_reader::{
    WireStream, finish_stream, finish_unary, read_one_message, status_from,
};
pub(crate) use headers::{
    DEFAULT_UA, PBRS_GRPC_UA, RequestReject, accepts_codec, accepts_gzip, check_request,
    effective_timeout, grpc_encoding, grpc_request, inbound_codec, preferred_codec,
    select_outbound_codec, select_stream_codec, soonest, timeout_from_headers, user_agent_value,
};
pub(crate) use out_batch::{OutBatch, let_producer_catch_up};
pub(crate) use send::{
    PumpEnd, grpc_trailers, pump_outbound, reject, reject_request, reset_on_cancel, send_bytes,
    send_ok_headers, send_trailers_only, wrap_timeout,
};
// Re-exported for the unit tests below, which drive internals directly.
#[cfg(test)]
pub(crate) use frame_reader::{FrameReader, percent_decode};
#[cfg(test)]
pub(crate) use headers::{grpc_content_type, grpc_encoding_admitted, grpc_encoding_supported};
#[cfg(test)]
pub(crate) use send::percent_encode;

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_UA, FrameReader, PBRS_GRPC_UA, accepts_gzip, effective_timeout, grpc_content_type,
        grpc_encoding, grpc_encoding_supported, grpc_request, percent_decode, percent_encode,
        preferred_codec, select_outbound_codec, select_stream_codec, soonest,
    };
    use crate::codec;
    use crate::compression::Codec;
    use crate::gzip;
    use crate::limits::MessageLimits;
    use crate::metadata::Metadata;
    use crate::status::Code;
    use bytes::{Bytes, BytesMut};
    use http::uri::Authority;

    #[test]
    fn outbound_requests_identify_the_kernel() {
        let authority: Authority = "127.0.0.1:1".parse().expect("authority");
        let req = grpc_request(
            &authority,
            "/svc/Method",
            &Metadata::new(),
            None,
            None,
            true,
            &PBRS_GRPC_UA,
            false,
        )
        .expect("request");
        assert_eq!(req.uri().scheme_str(), Some("http"));
        assert_eq!(
            req.headers()
                .get("user-agent")
                .and_then(|v| v.to_str().ok()),
            Some(DEFAULT_UA)
        );
    }

    #[test]
    fn outbound_tls_requests_use_the_https_scheme() {
        let authority: Authority = "127.0.0.1:1".parse().expect("authority");
        let req = grpc_request(
            &authority,
            "/svc/Method",
            &Metadata::new(),
            None,
            None,
            true,
            &PBRS_GRPC_UA,
            true,
        )
        .expect("request");
        assert_eq!(req.uri().scheme_str(), Some("https"));
    }

    #[test]
    fn grpc_content_type_accepts_the_spec_prefix() {
        for ok in [
            "application/grpc",
            "application/grpc+proto",
            "application/grpc;charset=utf-8",
            "application/grpc+proto; charset=utf-8",
            "Application/Grpc",
            "APPLICATION/GRPC+PROTO",
            " application/grpc ",
        ] {
            assert!(grpc_content_type(ok), "{ok}");
        }
        for no in [
            "application/json",
            "application/grpc+json",
            "application/grpc+json;charset=utf-8",
            "APPLICATION/GRPC+JSON",
            "application/grpc+thrift",
            "application/grpc-web",
            "application/grpc-web+proto",
            "application/grpcweb",
            "text/plain",
            "",
        ] {
            assert!(!grpc_content_type(no), "{no}");
        }
    }

    #[test]
    fn grpc_encoding_accepts_identity_and_gzip_case_insensitively() {
        for ok in [
            "gzip",
            "GZIP",
            "Gzip",
            " gzip ",
            "gzip;q=1.0",
            "deflate",
            "DEFLATE",
            " deflate;q=1.0 ",
            "identity",
            "IDENTITY",
            " identity ",
        ] {
            assert!(grpc_encoding_supported(ok), "{ok}");
        }
        for no in ["snappy", "gzip,identity", "", "br"] {
            assert!(!grpc_encoding_supported(no), "{no}");
        }
        assert!(super::grpc_encoding_admitted("identity", false));
        assert!(!super::grpc_encoding_admitted("gzip", false));
        assert!(!super::grpc_encoding_admitted("GZIP", false));
        assert!(!super::grpc_encoding_admitted("deflate", false));
    }

    #[test]
    fn outbound_requests_can_omit_gzip_from_accept_encoding() {
        let authority: Authority = "127.0.0.1:1".parse().expect("authority");
        let gzip = grpc_request(
            &authority,
            "/svc/Method",
            &Metadata::new(),
            None,
            None,
            true,
            &PBRS_GRPC_UA,
            false,
        )
        .expect("gzip accept");
        assert_eq!(
            gzip.headers()
                .get("grpc-accept-encoding")
                .and_then(|v| v.to_str().ok()),
            Some("identity,gzip,deflate")
        );
        let identity = grpc_request(
            &authority,
            "/svc/Method",
            &Metadata::new(),
            None,
            None,
            false,
            &PBRS_GRPC_UA,
            false,
        )
        .expect("identity accept");
        assert_eq!(
            identity
                .headers()
                .get("grpc-accept-encoding")
                .and_then(|v| v.to_str().ok()),
            Some("identity")
        );
    }

    #[test]
    fn accepts_gzip_parses_the_usual_header_shapes() {
        use http::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        assert!(!accepts_gzip(&headers));
        headers.insert("grpc-accept-encoding", HeaderValue::from_static("identity"));
        assert!(!accepts_gzip(&headers));
        headers.insert(
            "grpc-accept-encoding",
            HeaderValue::from_static("identity,gzip"),
        );
        assert!(accepts_gzip(&headers));
        headers.insert(
            "grpc-accept-encoding",
            HeaderValue::from_static("gzip;q=1.0, identity"),
        );
        assert!(accepts_gzip(&headers));
        headers.insert("grpc-accept-encoding", HeaderValue::from_static("GZIP"));
        assert!(accepts_gzip(&headers));
        assert_eq!(grpc_encoding(&headers), None);
        headers.insert("grpc-encoding", HeaderValue::from_static("gzip"));
        assert_eq!(grpc_encoding(&headers), Some("gzip"));
        headers.insert("grpc-encoding", HeaderValue::from_static("GZIP"));
        assert_eq!(grpc_encoding(&headers), Some("GZIP"));
        headers.insert("grpc-encoding", HeaderValue::from_static(" gzip;q=1.0 "));
        assert_eq!(grpc_encoding(&headers), Some("gzip"));
        for identity in ["identity", "IDENTITY", " identity ", "identity;q=0"] {
            headers.insert("grpc-encoding", HeaderValue::from_static(identity));
            assert_eq!(grpc_encoding(&headers), None, "{identity}");
        }
        let gzip = Some(Codec::Gzip);
        assert_eq!(select_outbound_codec(Some(true), true, None), None);
        assert_eq!(select_outbound_codec(None, true, gzip), gzip);
        assert_eq!(select_outbound_codec(Some(true), false, gzip), gzip);
        assert_eq!(select_outbound_codec(None, false, gzip), None);
        assert_eq!(select_outbound_codec(Some(false), true, gzip), None);
        // Mixed stream: set_compress(true) advertises gzip and must not rewrite
        // identity send() frames. Overlay still fills those when the envelope
        // is unset; set_compress(false) opts that fill out.
        assert_eq!(select_stream_codec(true, Some(true), false, gzip), gzip);
        assert_eq!(select_stream_codec(false, Some(true), false, gzip), None);
        assert_eq!(select_stream_codec(false, None, true, gzip), gzip);
        assert_eq!(select_stream_codec(false, Some(false), true, gzip), None);
        assert_eq!(select_stream_codec(true, Some(false), true, gzip), gzip);
        assert_eq!(select_stream_codec(true, Some(true), true, None), None);
        // Negotiation prefers the configured coding and falls back.
        assert_eq!(preferred_codec(Codec::Gzip, true, true), Some(Codec::Gzip));
        assert_eq!(
            preferred_codec(Codec::Deflate, true, true),
            Some(Codec::Deflate)
        );
        assert_eq!(
            preferred_codec(Codec::Deflate, true, false),
            Some(Codec::Gzip)
        );
        assert_eq!(preferred_codec(Codec::Gzip, false, false), None);
    }

    #[test]
    fn user_agent_prefixes_the_kernel_identity() {
        assert_eq!(super::user_agent_value("").expect("empty"), PBRS_GRPC_UA);
        assert_eq!(
            super::user_agent_value("  ").expect("whitespace"),
            PBRS_GRPC_UA
        );
        let ua = super::user_agent_value("inventory/2.1").expect("prefix");
        assert_eq!(
            ua.to_str().expect("ascii"),
            format!("inventory/2.1 {DEFAULT_UA}")
        );
        assert!(super::user_agent_value("bad\nagent").is_err());
    }

    #[test]
    fn effective_timeout_picks_the_sooner_deadline() {
        use http::{HeaderMap, HeaderValue};
        use std::time::Duration;

        let mut headers = HeaderMap::new();
        headers.insert("grpc-timeout", HeaderValue::from_static("10S"));
        assert_eq!(
            effective_timeout(&headers, Some(Duration::from_secs(3))),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            effective_timeout(&headers, Some(Duration::from_secs(30))),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            effective_timeout(&HeaderMap::new(), Some(Duration::from_secs(5))),
            Some(Duration::from_secs(5))
        );
        assert_eq!(effective_timeout(&HeaderMap::new(), None), None);
        assert_eq!(
            soonest(
                Some(Duration::from_millis(20)),
                Some(Duration::from_secs(5))
            ),
            Some(Duration::from_millis(20))
        );
        assert_eq!(
            soonest(None, Some(Duration::from_secs(1))),
            Some(Duration::from_secs(1))
        );
        assert_eq!(soonest(None, None), None);
    }

    #[test]
    fn status_details_round_trip_on_the_wire() {
        use super::{grpc_trailers, status_from};
        use crate::status::Status;
        use http::HeaderMap;

        let mut status = Status::not_found("gone");
        status.set_details(vec![0x08, 0x05]);
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        let trailers = grpc_trailers(&status).expect("trailers");
        assert!(trailers.get("grpc-status-details-bin").is_some());
        let restored = status_from(&HeaderMap::new(), Some(&trailers));
        assert_eq!(restored.code(), Code::NotFound);
        assert_eq!(restored.message(), "gone");
        assert_eq!(restored.details(), &[0x08, 0x05]);
        assert_eq!(restored.metadata().get("x-retry-after"), Some("30"));
        assert!(
            restored
                .metadata()
                .get_bin("grpc-status-details-bin")
                .is_none()
        );
    }

    #[test]
    fn padded_details_bin_is_accepted() {
        use super::status_from;
        use http::{HeaderMap, HeaderName, HeaderValue};

        let mut map = HeaderMap::new();
        map.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("5"),
        );
        map.insert(
            HeaderName::from_static("grpc-status-details-bin"),
            HeaderValue::from_static("CAU="),
        );
        let restored = status_from(&map, None);
        assert_eq!(restored.code(), Code::NotFound);
        assert_eq!(restored.details(), &[0x08, 0x05]);
    }

    #[test]
    fn message_encoding_matches_the_spec_set() {
        assert_eq!(percent_encode("plain text"), "plain text");
        assert_eq!(percent_encode("50%"), "50%25");
        assert_eq!(percent_encode("tab\there"), "tab%09here");
        assert_eq!(percent_encode("\u{00e9}"), "%C3%A9");
    }

    #[test]
    fn message_decoding_round_trips() {
        for original in ["plain text", "50%", "tab\there", "\u{00e9}\u{1f600}", ""] {
            assert_eq!(percent_decode(&percent_encode(original)), original);
        }
    }

    #[test]
    fn stray_percent_decodes_literally() {
        assert_eq!(percent_decode("100% sure"), "100% sure");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    fn frame(payload: &[u8]) -> Bytes {
        codec::encode(payload, false).expect("encode")
    }

    #[test]
    fn whole_frames_in_one_chunk_are_not_copied() {
        let mut joined = BytesMut::new();
        joined.extend_from_slice(&frame(b"one"));
        joined.extend_from_slice(&frame(b"two"));
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(joined.freeze());
        assert!(reader.carry.is_empty());
        let a = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&a.payload[..], b"one");
        let b = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&b.payload[..], b"two");
        assert!(reader.next_frame().expect("pop").is_none());
        reader.finish().expect("clean end");
        assert!(reader.carry.is_empty());
    }

    #[test]
    fn frames_split_across_chunks_are_rejoined() {
        let wire = frame(b"straddling");
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(wire.slice(..4));
        assert!(reader.next_frame().expect("pop").is_none());
        reader.push(wire.slice(4..9));
        assert!(reader.next_frame().expect("pop").is_none());
        reader.push(wire.slice(9..));
        let got = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&got.payload[..], b"straddling");
        reader.finish().expect("clean end");
    }

    #[test]
    fn leftover_bytes_after_a_frame_carry_into_the_next_chunk() {
        let first = frame(b"a");
        let second = frame(b"bb");
        let mut joined = BytesMut::from(first.as_ref());
        joined.extend_from_slice(&second[..3]);
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(joined.freeze());
        let got = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&got.payload[..], b"a");
        assert!(reader.next_frame().expect("pop").is_none());
        reader.push(second.slice(3..));
        let got = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&got.payload[..], b"bb");
        reader.finish().expect("clean end");
    }

    #[test]
    fn truncation_is_an_error() {
        let wire = frame(b"cut short");
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(wire.slice(..7));
        assert!(reader.next_frame().expect("pop").is_none());
        reader.finish().expect_err("truncated");
    }

    /// Deterministic xorshift, so a failure reproduces from the seed alone
    /// rather than needing a fuzzing dependency.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            if n == 0 {
                0
            } else {
                usize::try_from(self.next_u64() % u64::try_from(n).unwrap_or(1)).unwrap_or(0)
            }
        }

        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n)
                .map(|_| u8::try_from(self.next_u64() & 0xff).unwrap_or(0))
                .collect()
        }
    }

    /// Split `data` at random boundaries, so the reader sees every alignment of
    /// frames against chunks that HTTP/2 could produce.
    fn random_chunks(rng: &mut Rng, data: &Bytes) -> Vec<Bytes> {
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let remaining = data.len() - offset;
            let take = 1 + rng.below(remaining.min(64));
            chunks.push(data.slice(offset..offset + take));
            offset += take;
        }
        chunks
    }

    /// Property: however the bytes are split, the frames come back intact and
    /// in order. This is the invariant the zero-copy fast path could break.
    #[test]
    fn arbitrary_chunk_boundaries_preserve_every_frame() {
        let mut rng = Rng(0x5eed_1234_abcd_0001);
        for _ in 0..2_000 {
            let count = 1 + rng.below(6);
            let payloads: Vec<Vec<u8>> = (0..count)
                .map(|_| {
                    let len = rng.below(200);
                    rng.bytes(len)
                })
                .collect();
            let mut wire = BytesMut::new();
            for payload in &payloads {
                wire.extend_from_slice(&codec::encode(payload, false).expect("encode"));
            }
            let wire = wire.freeze();

            let mut reader = FrameReader::new(MessageLimits::unlimited());
            let mut got: Vec<Vec<u8>> = Vec::new();
            for chunk in random_chunks(&mut rng, &wire) {
                reader.push(chunk);
                while let Some(frame) = reader.next_frame().expect("well-formed") {
                    got.push(frame.payload.to_vec());
                }
            }
            reader.finish().expect("clean end");
            assert_eq!(got, payloads, "chunking must not change the frames");
        }
    }

    /// Property: arbitrary bytes in arbitrary chunks produce frames or a
    /// `Status`, never a panic and never a frame longer than the cap.
    #[test]
    fn arbitrary_bytes_never_panic_and_never_exceed_the_cap() {
        const CAP: usize = 512;
        let limits = MessageLimits::unlimited().with_max_decoding(CAP);
        let mut rng = Rng(0xf00d_0bad_1dea_0002);
        for _ in 0..4_000 {
            let len = rng.below(600);
            let garbage = Bytes::from(rng.bytes(len));
            let mut reader = FrameReader::new(limits);
            for chunk in random_chunks(&mut rng, &garbage) {
                reader.push(chunk);
                loop {
                    match reader.next_frame() {
                        Ok(Some(frame)) => assert!(frame.payload.len() <= CAP),
                        Ok(None) => break,
                        // A `Status` is the correct answer for garbage.
                        Err(_) => break,
                    }
                }
            }
            // Truncation is a legitimate verdict on garbage; either arm is
            // fine, and neither may panic.
            match reader.finish() {
                Ok(()) | Err(_) => {}
            }
        }
    }

    /// Property: a compressed frame never inflates past the cap, whatever it
    /// claims. Random data barely compresses, so this also exercises the case
    /// where the inflated size is close to the input size.
    #[test]
    fn compressed_frames_respect_the_cap() {
        const CAP: usize = 256;
        let limits = MessageLimits::unlimited().with_max_decoding(CAP);
        let mut rng = Rng(0xdead_beef_cafe_0003);
        for _ in 0..300 {
            let len = rng.below(2_000);
            // Runs of zeros compress well; random bytes do not. Mix both.
            let payload: Vec<u8> = if rng.below(2) == 0 {
                vec![0u8; len]
            } else {
                rng.bytes(len)
            };
            let compressed = gzip::encode(&payload).expect("encode");
            match gzip::decode_limited(&compressed, limits) {
                Ok(inflated) => {
                    assert!(inflated.len() <= CAP);
                    assert_eq!(inflated, payload);
                }
                Err(status) => {
                    assert!(payload.len() > CAP, "only oversize payloads may fail");
                    assert_eq!(status.code(), Code::ResourceExhausted);
                }
            }
        }
    }
}
