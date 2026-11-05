//! Compare generated attribute placement with the pinned prost-build control.
use std::collections::BTreeSet;
use std::path::Path;
use std::{env, fs};

fn placements(source: &str) -> BTreeSet<(String, String)> {
    let mut result = BTreeSet::new();
    let mut owner = None;
    let mut marked = false;
    for line in source.lines().map(str::trim) {
        let declaration = line
            .strip_prefix("pub struct ")
            .or_else(|| line.strip_prefix("struct "));
        if let Some(declaration) = declaration {
            owner =
                if declaration.starts_with("PacketCold {") || declaration.starts_with("Packet {") {
                    Some("Packet")
                } else if declaration.starts_with("Child {") {
                    Some("Child")
                } else if declaration.starts_with("Other {") {
                    Some("Other")
                } else {
                    None
                };
            marked = false;
        } else if line == "}" {
            owner = None;
            marked = false;
        } else if matches!(
            line,
            "#[doc = \"selector-probe\"]" | "/// selector-probe" | "///selector-probe"
        ) {
            marked = true;
        } else if !line.starts_with('#') && !line.starts_with("///") {
            let candidate = line.strip_prefix("pub ").unwrap_or(line);
            if let (Some(owner), Some((field, _))) = (owner, candidate.split_once(':')) {
                if ["id", "payload", "note", "nested"].contains(&field) {
                    if marked {
                        assert!(result.insert((owner.to_owned(), field.to_owned())));
                    }
                    marked = false;
                }
            }
        }
    }
    result
}

fn main() {
    let root =
        Path::new(env!("OUT_DIR")).join(format!("attribute-selectors-{}", std::process::id()));
    fs::create_dir(&root).expect("create unique selector proof directory");
    let proto = root.join("selectors.proto");
    fs::write(
        &proto,
        r#"syntax = "proto3";
package demo;
message Packet { int32 id = 1; bytes payload = 2; string note = 3; Child nested = 4; }
message Child { string note = 1; }
message Other { int32 id = 1; string note = 2; }
"#,
    )
    .unwrap();
    let selectors = [
        ".",
        ".demo",
        ".demo.Packet",
        ".demo.Packet.payload",
        "Packet.payload",
        "payload",
        "note",
        "Child.note",
        ".dem",
        "ote",
        "Packet",
        ".demo.PacketExtra",
        ".demo.Child.note",
    ];
    for (i, selector) in selectors.iter().enumerate() {
        let pbrs_dir = root.join(format!("{i:02}-pbrs"));
        let prost_dir = root.join(format!("{i:02}-prost"));
        fs::create_dir(&pbrs_dir).unwrap();
        fs::create_dir(&prost_dir).unwrap();
        let marker = "#[doc = \"selector-probe\"]";
        pbrs::codegen::Config::new()
            .out_dir(&pbrs_dir)
            .emit_kernel_stubs(false)
            .emit_rerun_if_changed(false)
            .field_attribute(*selector, marker)
            .compile_protos(&[&proto], &[&root])
            .unwrap();
        prost_build::Config::new()
            .out_dir(&prost_dir)
            .field_attribute(*selector, marker)
            .compile_protos(&[&proto], &[&root])
            .unwrap();
        let actual = placements(&fs::read_to_string(pbrs_dir.join("selectors.rs")).unwrap());
        let reference = placements(&fs::read_to_string(prost_dir.join("demo.rs")).unwrap());
        assert_eq!(actual, reference, "selector {selector:?}");
        match *selector {
            "." => assert_eq!(actual.len(), 7),
            ".demo.Packet" => assert_eq!(actual.len(), 4),
            "note" => assert_eq!(actual.len(), 3),
            _ => {}
        }
        println!("PASS attribute-selector {selector:?} {}", actual.len());
    }
    fs::remove_dir_all(root).expect("remove generated selector proof files");
}
