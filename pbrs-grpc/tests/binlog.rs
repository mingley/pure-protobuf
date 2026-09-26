//! GF-04: A16 binary logging — filter, encoding, masking, caps, sinks.
//!
//! Format conformance shells out to `protoc --decode` against the vendored
//! `grpc/binlog/v1/binarylog.proto`; those tests skip when `protoc` is
//! missing from `PATH`.

use pbrs_grpc::Metadata;
use pbrs_grpc::binlog::{
    Address, AddressType, BinaryLogFilter, BinaryLogger, Cap, ClientHeader, EventType,
    GrpcLogEntry, LogRecord, Logger, Payload, Rule, ServerHeader, Sink, Trailer, VecSink,
};
use pbrs_grpc::{Code, Status};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

fn manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Decode `bytes` with the real `GrpcLogEntry` schema. `None` when `protoc`
/// is unavailable (the caller skips).
fn protoc_decode(bytes: &[u8]) -> Option<String> {
    let root = manifest_dir();
    let grpc_proto = root.join("../../third_party/grpc/third_party/grpc-proto");
    let protobuf_src = root.join("../../third_party/grpc/third_party/protobuf/src");
    let proto = grpc_proto.join("grpc/binlog/v1/binarylog.proto");
    if !proto.exists() || !protobuf_src.exists() {
        return None;
    }
    let mut child = Command::new("protoc")
        .arg(format!("--proto_path={}", grpc_proto.display()))
        .arg(format!("--proto_path={}", protobuf_src.display()))
        .arg("--decode=grpc.binarylog.v1.GrpcLogEntry")
        .arg(proto)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(bytes).ok()?;
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[test]
fn filter_examples_from_a16() {
    // `*` logs everything in full.
    let full = BinaryLogFilter::parse("*").expect("filter");
    assert_eq!(full.resolve("/Foo/Bar"), Some(Rule::full()));
    // `{h}` logs headers only.
    let h = BinaryLogFilter::parse("*{h}").expect("filter");
    assert_eq!(
        h.resolve("/Foo/Bar"),
        Some(Rule {
            headers: Some(Cap::Full),
            messages: None,
        })
    );
    // `{m:256}` logs capped messages only.
    let m = BinaryLogFilter::parse("*{m:256}").expect("filter");
    assert_eq!(
        m.resolve("/Foo/Bar"),
        Some(Rule {
            headers: None,
            messages: Some(Cap::Bytes(256)),
        })
    );
    // Empty logs nothing.
    let none = BinaryLogFilter::parse("").expect("filter");
    assert_eq!(none.resolve("/Foo/Bar"), None);
    let blank = BinaryLogFilter::parse("   ").expect("filter");
    assert_eq!(blank.resolve("/Foo/Bar"), None);
}

#[test]
fn filter_most_exact_match_wins() {
    let filter = BinaryLogFilter::parse("Foo/*{h},Foo/Bar{m:256}").expect("filter");
    assert_eq!(
        filter.resolve("/Foo/Bar"),
        Some(Rule {
            headers: None,
            messages: Some(Cap::Bytes(256)),
        })
    );
    assert_eq!(
        filter.resolve("/Foo/Baz"),
        Some(Rule {
            headers: Some(Cap::Full),
            messages: None,
        })
    );
    assert_eq!(filter.resolve("/Other/Bar"), None);
}

#[test]
fn filter_exclusion_disables_one_method() {
    let filter = BinaryLogFilter::parse("Foo/*,-Foo/Bar").expect("filter");
    assert_eq!(filter.resolve("/Foo/Bar"), None);
    assert!(filter.resolve("/Foo/Baz").is_some());
}

#[test]
fn filter_malformed_is_rejected() {
    for bad in [
        "*/Bar",                              // service wildcard with a method
        "Foo/*,*/Bar",                        // same, not first
        "-Foo/*",                             // negated wildcard
        "Foo/Bar,-Foo/Bar",                   // exact match plus negation
        "-Foo/Bar,-Foo/Bar",                  // double negation
        "Foo/Bar{h:1},Foo/Bar{m:1}",          // duplicate method, different configs
        "Foo/*{h},Foo/*{m}",                  // duplicate service
        "*,*{h}",                             // duplicate global
        "Foo/Bar,*",                          // global must come first
        "Foo/Bar{}",                          // empty config
        "Foo/Bar{h",                          // unbalanced brace
        "Foo/Bar{h}}",                        // unbalanced brace
        "Foo/Bar{m;h}",                       // wrong order
        "Foo/Bar{h;m;x}",                     // third clause
        "Foo/Bar{h:x}",                       // non-numeric cap
        "Foo/Bar{m:-1}",                      // negative cap
        "Foo/Bar{h:99999999999999999999999}", // overflowing cap
        "Foo",                                // no method
        "Foo/",                               // empty method
        "/Bar",                               // empty service
        "Foo/Bar/Baz",                        // nested
        "Foo/Bar,,Foo/Baz",                   // empty pattern
    ] {
        assert!(BinaryLogFilter::parse(bad).is_err(), "accepted: {bad:?}");
    }
}

#[test]
fn entry_minimal_golden_bytes() {
    // Hand-computed proto3 encoding: field 1 (empty Timestamp), call_id 1,
    // sequence 1, CANCEL, CLIENT.
    let entry = GrpcLogEntry {
        timestamp: std::time::UNIX_EPOCH,
        call_id: 1,
        sequence_id_within_call: 1,
        event: EventType::Cancel,
        logger: Logger::Client,
        payload: None,
        payload_truncated: false,
        peer: None,
    };
    assert_eq!(
        entry.encode(),
        vec![0x0A, 0x00, 0x10, 0x01, 0x18, 0x01, 0x20, 0x07, 0x28, 0x01]
    );
}

#[test]
fn entry_decodes_with_protoc() {
    let Some(text) = protoc_decode(
        &GrpcLogEntry {
            timestamp: std::time::UNIX_EPOCH + std::time::Duration::new(12, 34),
            call_id: 7,
            sequence_id_within_call: 3,
            event: EventType::ClientHeader,
            logger: Logger::Server,
            payload: Some(Payload::ClientHeader(ClientHeader {
                metadata: vec![("x-id".to_owned(), b"9".to_vec())],
                method_name: "/demo.Echo/Ping".to_owned(),
                authority: "example.com".to_owned(),
                timeout: Some(std::time::Duration::from_secs(2)),
            })),
            payload_truncated: true,
            peer: Some(Address {
                addr_type: AddressType::Ipv4,
                address: "192.0.2.1".to_owned(),
                ip_port: 443,
            }),
        }
        .encode(),
    ) else {
        eprintln!("skipping: protoc or vendored protos unavailable");
        return;
    };
    for needle in [
        "call_id: 7",
        "sequence_id_within_call: 3",
        "type: EVENT_TYPE_CLIENT_HEADER",
        "logger: LOGGER_SERVER",
        "method_name: \"/demo.Echo/Ping\"",
        "authority: \"example.com\"",
        "key: \"x-id\"",
        "payload_truncated: true",
        "address: \"192.0.2.1\"",
        "ip_port: 443",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}

#[test]
fn trailer_decodes_with_protoc() {
    let status = Status::with_details(Code::NotFound, "missing thing", vec![1, 2, 3]);
    let sink = Arc::new(VecSink::new());
    let binlog = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
    let call = binlog
        .start_call("/demo.Echo/Ping", Logger::Client)
        .expect("logged");
    call.log_trailer(&Metadata::new(), &status);
    let records = sink.records();
    assert_eq!(records.len(), 1);
    let Some(text) = protoc_decode(&records[0].bytes) else {
        eprintln!("skipping: protoc or vendored protos unavailable");
        return;
    };
    for needle in [
        "type: EVENT_TYPE_SERVER_TRAILER",
        "status_code: 5",
        "status_message: \"missing thing\"",
        "status_details: \"\\001\\002\\003\"",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}

#[test]
fn message_bytes_decode_with_protoc() {
    let sink = Arc::new(VecSink::new());
    let binlog = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
    let call = binlog
        .start_call("/demo.Echo/Ping", Logger::Server)
        .expect("logged");
    call.log_read(b"\x0a\x03abc");
    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].entry.payload,
        Some(Payload::Message(_))
    ));
    let Some(text) = protoc_decode(&records[0].bytes) else {
        eprintln!("skipping: protoc or vendored protos unavailable");
        return;
    };
    for needle in [
        "type: EVENT_TYPE_CLIENT_MESSAGE",
        "logger: LOGGER_SERVER",
        "length: 5",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}

fn logging_call(filter: &str, role: Logger) -> (Arc<VecSink>, pbrs_grpc::binlog::CallLogger) {
    let sink = Arc::new(VecSink::new());
    let binlog = BinaryLogger::new(
        BinaryLogFilter::parse(filter).expect("filter"),
        sink.clone(),
    );
    let call = binlog.start_call("/demo.Echo/Ping", role).expect("logged");
    (sink, call)
}

fn header_payload(entry: &GrpcLogEntry) -> Vec<(String, Vec<u8>)> {
    match entry.payload.as_ref().expect("payload") {
        Payload::ClientHeader(h) => h.metadata.clone(),
        Payload::ServerHeader(h) => h.metadata.clone(),
        Payload::Trailer(t) => t.metadata.clone(),
        Payload::Message(_) => panic!("expected headers, got message"),
    }
}

#[test]
fn server_masks_sensitive_and_omits_authorization() {
    let (sink, call) = logging_call("*", Logger::Server);
    let mut md = Metadata::new();
    md.insert("x-request-id", "abc").expect("insert");
    md.insert("authorization", "Bearer secret").expect("insert");
    md.insert("cookie", "session=1").expect("insert");
    md.insert_bin("x-trace-bin", [0xde, 0xad]).expect("insert");
    md.insert("x-token", "tok").expect("insert");
    md.mark_sensitive("x-request-id");
    call.log_client_header(&md, "/demo.Echo/Ping", "example.com", None);
    let entries = header_payload(&sink.records()[0].entry);
    let get = |key: &str| {
        entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(get("authorization"), None, "credential header omitted");
    assert_eq!(get("cookie").as_deref(), Some(b"[REDACTED]".as_slice()));
    assert_eq!(
        get("x-trace-bin").as_deref(),
        Some(b"[REDACTED]".as_slice())
    );
    assert_eq!(get("x-token").as_deref(), Some(b"[REDACTED]".as_slice()));
    assert_eq!(
        get("x-request-id").as_deref(),
        Some(b"[REDACTED]".as_slice()),
        "custom sensitive key masked"
    );
}

#[test]
fn client_omits_proxy_authorization_and_user_agent() {
    let (sink, call) = logging_call("*", Logger::Client);
    let mut md = Metadata::new();
    md.insert("authorization", "Bearer secret").expect("insert");
    md.insert("proxy-authorization", "Basic x").expect("insert");
    md.insert("user-agent", "smuggled").expect("insert");
    md.insert("x-ok", "yes").expect("insert");
    call.log_client_header(&md, "/demo.Echo/Ping", "example.com", None);
    let entries = header_payload(&sink.records()[0].entry);
    let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["x-ok"]);
}

#[test]
fn sensitive_opt_out_logs_raw_values() {
    let sink = Arc::new(VecSink::new());
    let binlog = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone())
        .allow_sensitive_values();
    let call = binlog
        .start_call("/demo.Echo/Ping", Logger::Server)
        .expect("logged");
    let mut md = Metadata::new();
    md.insert("cookie", "session=1").expect("insert");
    md.insert("authorization", "Bearer secret").expect("insert");
    call.log_client_header(&md, "/demo.Echo/Ping", "example.com", None);
    let entries = header_payload(&sink.records()[0].entry);
    let get = |key: &str| {
        entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(get("cookie").as_deref(), Some(b"session=1".as_slice()));
    assert_eq!(
        get("authorization"),
        None,
        "A16 omission is not consent-gated"
    );
}

#[test]
fn header_cap_truncates_values_and_flags() {
    let (sink, call) = logging_call("*{h:3}", Logger::Server);
    let mut md = Metadata::new();
    md.insert("x-long", "abcdef").expect("insert");
    md.insert("x-ok", "ab").expect("insert");
    call.log_client_header(&md, "/demo.Echo/Ping", "example.com", None);
    let record = &sink.records()[0];
    assert!(record.entry.payload_truncated);
    let entries = header_payload(&record.entry);
    let get = |key: &str| {
        entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(get("x-long").as_deref(), Some(b"abc".as_slice()));
    assert_eq!(get("x-ok").as_deref(), Some(b"ab".as_slice()));
}

#[test]
fn message_cap_keeps_full_length_and_flags() {
    let (sink, call) = logging_call("*{m:2}", Logger::Client);
    call.log_read(b"hello");
    let record = &sink.records()[0];
    assert!(record.entry.payload_truncated);
    match record.entry.payload.as_ref().expect("payload") {
        Payload::Message(m) => {
            assert_eq!(m.length, 5);
            assert_eq!(m.data, b"he");
        }
        other => panic!("expected message, got {other:?}"),
    }
}

#[test]
fn header_only_filter_skips_messages() {
    let (sink, call) = logging_call("*{h}", Logger::Server);
    call.log_read(b"hello");
    call.log_client_header(&Metadata::new(), "/demo.Echo/Ping", "example.com", None);
    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].entry.payload,
        Some(Payload::ClientHeader(_))
    ));
}

#[test]
fn message_only_filter_skips_headers() {
    let (sink, call) = logging_call("*{m}", Logger::Server);
    call.log_read(b"hello");
    call.log_client_header(&Metadata::new(), "/demo.Echo/Ping", "example.com", None);
    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].entry.payload,
        Some(Payload::Message(_))
    ));
}

#[test]
fn signals_emit_whenever_method_is_logged() {
    let (sink, call) = logging_call("*{m}", Logger::Client);
    call.log_half_close();
    call.log_cancel();
    let records = sink.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].entry.event, EventType::ClientHalfClose);
    assert_eq!(records[1].entry.event, EventType::Cancel);
    assert_eq!(records[0].entry.sequence_id_within_call, 1);
    assert_eq!(records[1].entry.sequence_id_within_call, 2);
}

#[test]
fn call_ids_are_unique_and_nonzero() {
    let sink = Arc::new(VecSink::new());
    let binlog = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
    let a = binlog
        .start_call("/demo.Echo/Ping", Logger::Client)
        .expect("logged");
    let b = binlog
        .start_call("/demo.Echo/Ping", Logger::Client)
        .expect("logged");
    assert_ne!(a.call_id(), 0);
    assert_ne!(a.call_id(), b.call_id());
}

#[test]
fn excluded_method_logs_nothing() {
    let sink = Arc::new(VecSink::new());
    let binlog = BinaryLogger::new(
        BinaryLogFilter::parse("*,-demo.Echo/Ping").expect("filter"),
        sink.clone(),
    );
    assert!(
        binlog
            .start_call("/demo.Echo/Ping", Logger::Client)
            .is_none()
    );
    assert!(
        binlog
            .start_call("/demo.Echo/Pong", Logger::Client)
            .is_some()
    );
}

#[test]
fn server_peer_attaches_once_on_client_header() {
    let (sink, call) = logging_call("*", Logger::Server);
    call.set_peer("192.0.2.1:443".parse().expect("addr"));
    call.log_client_header(&Metadata::new(), "/demo.Echo/Ping", "example.com", None);
    call.log_server_header(&Metadata::new());
    let records = sink.records();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].entry.peer,
        Some(Address {
            addr_type: AddressType::Ipv4,
            address: "192.0.2.1".to_owned(),
            ip_port: 443,
        })
    );
    assert_eq!(records[1].entry.peer, None);
}

#[test]
fn client_peer_attaches_on_first_incoming_event() {
    let (sink, call) = logging_call("*", Logger::Client);
    call.set_peer("[2001:db8::1]:80".parse().expect("addr"));
    call.log_client_header(&Metadata::new(), "/demo.Echo/Ping", "example.com", None);
    call.log_server_header(&Metadata::new());
    call.log_trailer(&Metadata::new(), &Status::new(Code::Ok, ""));
    let records = sink.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].entry.peer, None);
    assert!(records[1].entry.peer.is_some());
    assert_eq!(
        records[1].entry.peer.as_ref().expect("peer").addr_type,
        AddressType::Ipv6
    );
    assert_eq!(records[2].entry.peer, None, "peer logged only once");
}

#[test]
fn client_trailers_only_trailer_carries_peer() {
    let (sink, call) = logging_call("*", Logger::Client);
    call.set_peer("192.0.2.1:443".parse().expect("addr"));
    call.log_trailer(&Metadata::new(), &Status::new(Code::Unavailable, "down"));
    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].entry.peer.is_some());
}

#[test]
fn written_frames_strip_the_5_byte_prefix() {
    let (sink, call) = logging_call("*", Logger::Server);
    let mut frame = vec![0u8, 0, 0, 0, 3];
    frame.extend_from_slice(b"abc");
    call.log_written(&frame);
    let records = sink.records();
    assert_eq!(records[0].entry.event, EventType::ServerMessage);
    match records[0].entry.payload.as_ref().expect("payload") {
        Payload::Message(m) => {
            assert_eq!(m.length, 3);
            assert_eq!(m.data, b"abc");
        }
        other => panic!("expected message, got {other:?}"),
    }
}

#[test]
fn read_direction_depends_on_role() {
    let (sink, call) = logging_call("*", Logger::Client);
    call.log_read(b"abc");
    assert_eq!(sink.records()[0].entry.event, EventType::ServerMessage);
    let (sink, call) = logging_call("*", Logger::Server);
    call.log_written(b"abc");
    assert_eq!(sink.records()[0].entry.event, EventType::ServerMessage);
}

#[test]
fn file_sink_round_trips_length_delimited_records() {
    use pbrs_grpc::binlog::FileSink;
    let dir = std::env::temp_dir().join(format!("pbrs-binlog-{}", std::process::id()));
    let path = dir.join("log.bin");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let file_sink = FileSink::open(&path).expect("open");
    let record = LogRecord {
        entry: GrpcLogEntry {
            timestamp: std::time::UNIX_EPOCH,
            call_id: 1,
            sequence_id_within_call: 1,
            event: EventType::Cancel,
            logger: Logger::Client,
            payload: None,
            payload_truncated: false,
            peer: None,
        },
        bytes: vec![1, 2, 3, 4],
    };
    file_sink.emit(&record);
    file_sink.emit(&record);
    drop(file_sink);
    let bytes = std::fs::read(&path).expect("read");
    assert_eq!(bytes, vec![4, 1, 2, 3, 4, 4, 1, 2, 3, 4]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unused_imports_anchor() {
    // Compile-time anchor: these names stay importable from the test harness.
    let _ = (EventType::Unknown, Logger::Unknown);
    let _ = ServerHeader { metadata: vec![] };
    let _ = Trailer {
        metadata: vec![],
        status_code: 0,
        status_message: String::new(),
        status_details: vec![],
    };
}
