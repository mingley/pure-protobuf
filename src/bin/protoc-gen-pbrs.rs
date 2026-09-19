//! protoc plugin: emit this crate's typed accessors (not Google upb gencode).
#![allow(clippy::expect_used, reason = "plugin IO failure exits the process")]

use std::io::{Read, Write};

fn main() {
    let mut stdin = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut stdin) {
        let out =
            pbrs::codegen::encode_code_generator_response_error(&format!("stdin read error: {e}"));
        std::io::stdout().write_all(&out).expect("stdout");
        return;
    }
    match pbrs::codegen::generate_from_code_generator_request(&stdin) {
        Ok(files) => {
            let out = pbrs::codegen::encode_code_generator_response(&files);
            std::io::stdout().write_all(&out).expect("stdout");
        }
        Err(e) => {
            let out = pbrs::codegen::encode_code_generator_response_error(&e.to_string());
            std::io::stdout().write_all(&out).expect("stdout");
        }
    }
}
