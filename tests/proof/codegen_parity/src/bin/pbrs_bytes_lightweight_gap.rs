#![forbid(unsafe_code)]
include!(concat!(
    env!("OUT_DIR"),
    "/native-bytes-lightweight-safe/mod.rs"
));

fn main() {
    let message = proof::bytes::Example::new();
    let _ = message.to_json();
    let _ = proof::bytes::Example::from_json("{}");
    let _ = message.to_text();
    let _ = proof::bytes::Example::from_text("");
}
