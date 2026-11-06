macro_rules! consumer {
    ($directory:literal) => {
        include!(concat!(env!("OUT_DIR"), "/", $directory, "/native/mod.rs"));
        mod reference {
            pub mod proof {
                pub mod comments {
                    include!(concat!(
                        env!("OUT_DIR"),
                        "/",
                        $directory,
                        "/reference/proof.comments.rs"
                    ));
                }
                pub mod other {
                    include!(concat!(
                        env!("OUT_DIR"),
                        "/",
                        $directory,
                        "/reference/proof.other.rs"
                    ));
                }
            }
        }
        fn main() {
            use pbrs::{Parse, Serialize};
            use prost::Message as _;
            let expected = reference::proof::comments::Main {
                value: 17,
                nested: Some(reference::proof::comments::main::Nested {
                    label: "roundtrip".into(),
                }),
                imported: Some(reference::proof::other::Imported { value: 23 }),
                state: 1,
            };
            let mut native = proof::comments::Main::parse(&expected.encode_to_vec()).unwrap();
            assert_eq!(native.value(), 17);
            native.imported_mut().set_value(31);
            let mut expected = expected;
            expected.imported.as_mut().unwrap().value = 31;
            assert_eq!(
                reference::proof::comments::Main::decode(
                    Serialize::serialize(&native).unwrap().as_slice()
                )
                .unwrap(),
                expected
            );
            println!("PASS {}", $directory);
        }
    };
}
