use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    pbrs::codegen::Config::new()
        .out_dir(&out)
        .compile_protos(&["proof.proto"], &["."])
        .unwrap();
    let path = out.join("proof.rs");
    let source = fs::read_to_string(&path).unwrap();
    let prefix = "while *pos < data.len() && data[*pos] == ((";
    let mut legacy = String::new();
    let mut tail = source.as_str();
    let mut count = 0;
    while let Some(start) = tail.find(prefix) {
        legacy.push_str(&tail[..start]);
        let run = &tail[start + prefix.len()..];
        let number_end = run.find(" << 3) | ").unwrap();
        let number = &run[..number_end];
        let run = &run[number_end + " << 3) | ".len()..];
        let wire_end = run.find(") as u8 { *pos += 1; ").unwrap();
        let wire = &run[..wire_end];
        let body = &run[wire_end + ") as u8 { *pos += 1; ".len()..];
        let mut depth = 1usize;
        let mut end = None;
        for (index, byte) in body.bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.unwrap();
        legacy.push_str(&format!("while *pos < data.len() {{ let save = *pos; match pbrs::rt::decode_tag(data, pos) {{ Ok((n2, w2)) if n2 == {number} && w2 == {wire} => {{ {} }} Ok(_) => {{ *pos = save; break; }} Err(e) => return Err(e), }} }}", &body[..end]));
        tail = &body[end + 1..];
        count += 1;
    }
    assert_eq!(count, 5, "expected every generated one-byte repeated run");
    legacy.push_str(tail);
    fs::write(out.join("legacy.rs"), legacy).unwrap();
    println!("cargo:rerun-if-changed=proof.proto");
}
