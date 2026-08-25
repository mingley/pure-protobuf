//! Official conformance_test_runner child: stdin/stdout ConformanceRequest/Response.
//! TestAllTypes path uses generated typed wrappers (same codec as the plugin).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    reason = "conformance child exits on stdin framing errors; wire lengths fit u32"
)]

use pbrs::gencode::{
    EditionsTestAllRequiredTypesProto2, EditionsTestAllTypesProto2, EditionsTestAllTypesProto3,
    TestAllRequiredTypesProto2, TestAllTypesEdition2023, TestAllTypesEditionUnstable,
    TestAllTypesProto2, TestAllTypesProto3,
};
use pbrs::{Parse, ParseError, Serialize};
use std::io::{Read, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    loop {
        let mut lenb = [0u8; 4];
        match stdin.read_exact(&mut lenb) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => panic!("{e}"),
        }
        let len = u32::from_le_bytes(lenb) as usize;
        let mut buf = vec![0u8; len];
        stdin.read_exact(&mut buf).expect("payload");
        let resp = handle(&buf);
        let n = resp.len() as u32;
        stdout.write_all(&n.to_le_bytes()).ok();
        stdout.write_all(&resp).ok();
        stdout.flush().ok();
    }
}

fn handle(req: &[u8]) -> Vec<u8> {
    match handle_inner(req) {
        Ok(bytes) => bytes,
        Err(e) => encode_response_parse_error(&e.to_string()),
    }
}

trait Conf: Parse + Serialize + Sized {
    fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError>;
    fn to_json(&self) -> Result<String, pbrs::SerializeError>;
    fn from_text(text: &str) -> Result<Self, ParseError>;
    fn to_text(&self) -> Result<String, pbrs::SerializeError>;
    fn to_text_with_unknown(&self) -> Result<String, pbrs::SerializeError>;
}

macro_rules! impl_conf {
    ($($t:ty),* $(,)?) => {
        $(
            impl Conf for $t {
                fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {
                    <$t>::from_json_ignore(json, ignore)
                }
                fn to_json(&self) -> Result<String, pbrs::SerializeError> {
                    <$t>::to_json(self)
                }
                fn from_text(text: &str) -> Result<Self, ParseError> {
                    <$t>::from_text(text)
                }
                fn to_text(&self) -> Result<String, pbrs::SerializeError> {
                    <$t>::to_text(self)
                }
                fn to_text_with_unknown(&self) -> Result<String, pbrs::SerializeError> {
                    <$t>::to_text_with_unknown(self)
                }
            }
        )*
    };
}

impl_conf!(
    TestAllTypesProto3,
    TestAllTypesProto2,
    TestAllRequiredTypesProto2,
    TestAllTypesEdition2023,
    TestAllTypesEditionUnstable,
    EditionsTestAllTypesProto2,
    EditionsTestAllRequiredTypesProto2,
    EditionsTestAllTypesProto3,
);

fn run<T: Conf>(parsed: &Request) -> Result<Vec<u8>, ParseError> {
    let msg = match &parsed.payload {
        Payload::Protobuf(b) => T::parse(b)?,
        Payload::Json(s) => T::from_json_ignore(s, parsed.category == 3)?,
        Payload::Text(s) => T::from_text(s)?,
        Payload::Jspb => return Ok(encode_response_skipped("jspb")),
        Payload::None => return Ok(encode_response_parse_error("empty payload")),
    };
    Ok(match parsed.output {
        1 => encode_response_protobuf(
            &Serialize::serialize(&msg).map_err(|e| ParseError::owned(e.to_string()))?,
        ),
        2 => match msg.to_json() {
            Ok(s) => encode_response_json(&s),
            Err(e) => encode_response_serialize_error(&e.to_string()),
        },
        4 => {
            let r = if parsed.print_unknown {
                msg.to_text_with_unknown()
            } else {
                msg.to_text()
            };
            match r {
                Ok(s) => encode_response_text(&s),
                Err(e) => encode_response_serialize_error(&e.to_string()),
            }
        }
        3 => encode_response_skipped("jspb"),
        _ => encode_response_skipped("unspecified output"),
    })
}

fn handle_inner(req: &[u8]) -> Result<Vec<u8>, ParseError> {
    let parsed = parse_request(req)?;
    if parsed.message_type == "conformance.FailureSet" {
        return Ok(encode_response_protobuf(&[]));
    }
    if parsed.category == 4 {
        return Ok(encode_response_skipped("jspb"));
    }
    Ok(match parsed.message_type.as_str() {
        "protobuf_test_messages.proto3.TestAllTypesProto3" => run::<TestAllTypesProto3>(&parsed)?,
        "protobuf_test_messages.proto2.TestAllTypesProto2" => run::<TestAllTypesProto2>(&parsed)?,
        "protobuf_test_messages.proto2.TestAllRequiredTypesProto2" => {
            run::<TestAllRequiredTypesProto2>(&parsed)?
        }
        "protobuf_test_messages.editions.TestAllTypesEdition2023" => {
            run::<TestAllTypesEdition2023>(&parsed)?
        }
        "protobuf_test_messages.edition_unstable.TestAllTypesEditionUnstable" => {
            run::<TestAllTypesEditionUnstable>(&parsed)?
        }
        "protobuf_test_messages.editions.proto2.TestAllTypesProto2" => {
            run::<EditionsTestAllTypesProto2>(&parsed)?
        }
        "protobuf_test_messages.editions.proto2.TestAllRequiredTypesProto2" => {
            run::<EditionsTestAllRequiredTypesProto2>(&parsed)?
        }
        "protobuf_test_messages.editions.proto3.TestAllTypesProto3" => {
            run::<EditionsTestAllTypesProto3>(&parsed)?
        }
        other => {
            return Ok(encode_response_parse_error(&format!(
                "unknown type {other}"
            )))
        }
    })
}

enum Payload {
    None,
    Protobuf(Vec<u8>),
    Json(String),
    Text(String),
    Jspb,
}

struct Request {
    payload: Payload,
    output: u32,
    message_type: String,
    category: u32,
    print_unknown: bool,
}

fn parse_request(bytes: &[u8]) -> Result<Request, ParseError> {
    use pbrs::rt::{decode_tag, decode_varint, read_len_bytes, skip_field, WIRE_LEN, WIRE_VARINT};
    let mut req = Request {
        payload: Payload::None,
        output: 0,
        message_type: String::new(),
        category: 0,
        print_unknown: false,
    };
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => {
                req.payload = Payload::Protobuf(read_len_bytes(bytes, &mut pos)?.to_vec())
            }
            (2, WIRE_LEN) => {
                req.payload = Payload::Json(
                    String::from_utf8_lossy(read_len_bytes(bytes, &mut pos)?).into_owned(),
                )
            }
            (7, WIRE_LEN) => {
                let _ = read_len_bytes(bytes, &mut pos)?;
                req.payload = Payload::Jspb;
            }
            (8, WIRE_LEN) => {
                req.payload = Payload::Text(
                    String::from_utf8_lossy(read_len_bytes(bytes, &mut pos)?).into_owned(),
                )
            }
            (3, WIRE_VARINT) => req.output = decode_varint(bytes, &mut pos)? as u32,
            (4, WIRE_LEN) => {
                req.message_type =
                    String::from_utf8_lossy(read_len_bytes(bytes, &mut pos)?).into_owned()
            }
            (5, WIRE_VARINT) => req.category = decode_varint(bytes, &mut pos)? as u32,
            (9, WIRE_VARINT) => req.print_unknown = decode_varint(bytes, &mut pos)? != 0,
            _ => skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(req)
}

fn encode_response_parse_error(msg: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 1, msg.as_bytes());
    out
}

fn encode_response_protobuf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 3, bytes);
    out
}

fn encode_response_json(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 4, s.as_bytes());
    out
}

fn encode_response_skipped(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 5, s.as_bytes());
    out
}

fn encode_response_serialize_error(msg: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 6, msg.as_bytes());
    out
}

fn encode_response_text(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 8, s.as_bytes());
    out
}
