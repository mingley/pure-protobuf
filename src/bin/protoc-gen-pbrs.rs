//! protoc plugin: emit this crate's typed accessors (not Google upb gencode).
#![allow(clippy::expect_used, reason = "plugin IO failure exits the process")]

use std::io::{Read, Write};

fn main() {
    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin).expect("stdin");
    match pbrs::codegen::generate_from_code_generator_request(&stdin) {
        Ok(files) => {
            let out = pbrs::codegen::encode_code_generator_response(&files);
            std::io::stdout().write_all(&out).expect("stdout");
        }
        Err(e) => {
            eprintln!("protoc-gen-pbrs: {e}");
            std::process::exit(1);
        }
    }
}
