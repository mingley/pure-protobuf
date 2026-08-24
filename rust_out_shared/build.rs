//! Generate official `protoc --rust_out` (kernel=upb) for rust/test/shared.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct ProtoLib {
    module: &'static str,
    files: &'static [&'static str],
}

/// Proto libraries the non-skipped shared tests import. Grouping matches
/// third_party/protobuf/rust/release_crates/protobuf_tests/build.rs plus utf8.
const LIBS: &[ProtoLib] = &[
    ProtoLib {
        module: "bad_names_rust_proto",
        files: &["rust/test/bad_names.proto"],
    },
    ProtoLib {
        module: "child_rust_proto",
        files: &["rust/test/child.proto"],
    },
    ProtoLib {
        module: "cpp_features_rust_proto",
        files: &["google/protobuf/cpp_features.proto"],
    },
    ProtoLib {
        module: "descriptor_rust_proto",
        files: &["google/protobuf/descriptor.proto"],
    },
    ProtoLib {
        module: "edition2023_rust_proto",
        files: &["rust/test/edition2023.proto"],
    },
    ProtoLib {
        module: "enums_rust_proto",
        files: &["rust/test/enums.proto"],
    },
    ProtoLib {
        module: "fields_with_imported_types_rust_proto",
        files: &["rust/test/fields_with_imported_types.proto"],
    },
    ProtoLib {
        module: "imported_types_rust_proto",
        files: &["rust/test/imported_types.proto"],
    },
    ProtoLib {
        module: "import_public_grandparent_rust_proto",
        files: &["rust/test/import_public_grandparent.proto"],
    },
    ProtoLib {
        module: "import_public_non_primary_src1_rust_proto",
        files: &["rust/test/import_public_non_primary_src1.proto"],
    },
    ProtoLib {
        module: "import_public_non_primary_src2_rust_proto",
        files: &["rust/test/import_public_non_primary_src2.proto"],
    },
    ProtoLib {
        module: "import_public_primary_src_rust_proto",
        files: &["rust/test/import_public_primary_src.proto"],
    },
    ProtoLib {
        module: "import_public_rust_proto",
        files: &[
            "rust/test/import_public.proto",
            "rust/test/import_public2.proto",
        ],
    },
    ProtoLib {
        module: "map_unittest_rust_proto",
        files: &["rust/test/map_unittest.proto"],
    },
    ProtoLib {
        module: "nested_rust_proto",
        files: &["rust/test/nested.proto"],
    },
    ProtoLib {
        module: "no_package_import_rust_proto",
        files: &["rust/test/no_package_import.proto"],
    },
    ProtoLib {
        module: "no_package_rust_proto",
        files: &[
            "rust/test/no_package.proto",
            "rust/test/no_package_other.proto",
        ],
    },
    ProtoLib {
        module: "package_import_rust_proto",
        files: &["rust/test/package_import.proto"],
    },
    ProtoLib {
        module: "package_rust_proto",
        files: &[
            "rust/test/package.proto",
            "rust/test/package_other.proto",
            "rust/test/package_other_different.proto",
        ],
    },
    ProtoLib {
        module: "parent_rust_proto",
        files: &["rust/test/parent.proto"],
    },
    ProtoLib {
        module: "unittest_import_rust_proto",
        files: &["rust/test/unittest_import.proto"],
    },
    ProtoLib {
        module: "unittest_rust_proto",
        files: &["rust/test/unittest.proto"],
    },
    ProtoLib {
        module: "unittest_proto3_optional_rust_proto",
        files: &["rust/test/unittest_proto3_optional.proto"],
    },
    ProtoLib {
        module: "unittest_proto3_rust_proto",
        files: &["rust/test/unittest_proto3.proto"],
    },
    ProtoLib {
        module: "feature_verify_rust_proto",
        files: &["rust/test/shared/utf8/feature_verify.proto"],
    },
    ProtoLib {
        module: "no_features_proto2_rust_proto",
        files: &["rust/test/shared/utf8/no_features_proto2.proto"],
    },
    ProtoLib {
        module: "no_features_proto3_rust_proto",
        files: &["rust/test/shared/utf8/no_features_proto3.proto"],
    },
];

fn protobuf_root(manifest: &Path) -> PathBuf {
    let third = manifest.join("../third_party/protobuf");
    if third.join("rust/test/unittest.proto").is_file() {
        third
    } else {
        manifest.join("../vendor/google")
    }
}

fn resolve_input(proto_root: &Path, src: Option<&Path>, file: &str) -> PathBuf {
    let a = proto_root.join(file);
    if a.is_file() {
        return a;
    }
    if let Some(s) = src {
        let b = s.join(file);
        if b.is_file() {
            return b;
        }
    }
    panic!("missing proto {file} (looked under {} and src)", proto_root.display());
}

#[allow(dead_code)]
fn generated_entry_rel(name: &str, first_file: &str) -> PathBuf {
    let dir = Path::new(first_file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    Path::new("protobuf_generated")
        .join(name)
        .join(dir)
        .join("generated.rs")
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let proto_root = protobuf_root(&manifest).canonicalize().unwrap();
    let src_include = {
        let src_root = proto_root.join("src");
        if src_root.is_dir() {
            Some(src_root)
        } else {
            let vendor_src = manifest.join("../third_party/protobuf/src");
            vendor_src
                .is_dir()
                .then(|| vendor_src.canonicalize().unwrap())
        }
    };

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", proto_root.display());
    if let Some(ref s) = src_include {
        println!("cargo:rerun-if-changed={}", s.display());
    }

    let protoc = std::env::var_os("PROTOC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("protoc"));

    let mapping_path = out_dir.join("crate_mapping.txt");
    let mut mapping = String::new();
    for lib in LIBS {
        mapping.push_str(&format!("crate::{}\n", lib.module));
        mapping.push_str(&format!("{}\n", lib.files.len()));
        for f in lib.files {
            mapping.push_str(f);
            mapping.push('\n');
        }
    }
    fs::write(&mapping_path, mapping).unwrap();

    let gendir = out_dir.join("protobuf_generated");
    fs::create_dir_all(&gendir).unwrap();

    for lib in LIBS {
        let dest = gendir.join(lib.module.trim_end_matches("_rust_proto"));
        fs::create_dir_all(&dest).unwrap();
        let mut cmd = Command::new(&protoc);
        for f in lib.files {
            // Relative import paths (not absolute) so rust_out keeps
            // rust/test/... and google/protobuf/... layout.
            let _exists = resolve_input(&proto_root, src_include.as_deref(), f);
            cmd.arg(f);
        }
        cmd.arg(format!("--rust_out={}", dest.display()));
        cmd.arg(format!(
            "--rust_opt=experimental-codegen=enabled,kernel=upb,crate_mapping={}",
            mapping_path.display()
        ));
        cmd.arg(format!("--proto_path={}", proto_root.display()));
        if let Some(ref s) = src_include {
            cmd.arg(format!("--proto_path={}", s.display()));
        }
        let output = cmd.output().unwrap_or_else(|e| {
            panic!("failed to spawn protoc ({:?}): {e}", protoc);
        });
        if !output.status.success() {
            panic!(
                "protoc --rust_out failed for {}:\n{}\n{}",
                lib.module,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let entry = find_generated_rs(&dest).unwrap_or_else(|| {
            panic!(
                "expected generated.rs under {} for {}",
                dest.display(),
                lib.module
            )
        });
        let _ = entry;
    }

    let mut mods = String::from("// @generated by rust_out_shared/build.rs\n");
    for lib in LIBS {
        let stem = lib.module.trim_end_matches("_rust_proto");
        let dest = gendir.join(stem);
        let entry = find_generated_rs(&dest).expect(lib.module);
        mods.push_str(&format!(
            "#[allow(dead_code, unused, unused_imports, unused_mut, nonstandard_style)]\n\
             #[allow(clippy::all, unreachable_pub, static_mut_refs)]\n\
             #[path = \"{}\"]\n\
             pub mod {};\n",
            entry.display(),
            lib.module
        ));
    }
    fs::write(out_dir.join("mods.rs"), mods).unwrap();
}

fn find_generated_rs(dir: &Path) -> Option<PathBuf> {
    fn has_src(p: &Path) -> bool {
        p.components().any(|c| c.as_os_str() == "src")
    }
    fn walk(dir: &Path, out: &mut Option<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.file_name().and_then(|n| n.to_str()) == Some("generated.rs") {
                let take = match out.as_ref() {
                    None => true,
                    Some(old) => has_src(old) && !has_src(&p),
                };
                if take {
                    *out = Some(p);
                }
            }
        }
    }
    let mut found = None;
    walk(dir, &mut found);
    found
}
