mod reference {
    pub mod proof {
        pub mod bytes_external {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-bytes/proof.bytes_external.rs"
            ));
        }
        pub mod bytes_legacy {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-bytes/proof.bytes_legacy.rs"
            ));
        }
        pub mod bytes {
            include!(concat!(env!("OUT_DIR"), "/prost-bytes/proof.bytes.rs"));
        }
    }
}

fn fixture(len: usize) -> reference::proof::bytes::Example {
    use bytes::Bytes;
    use reference::proof::{
        bytes::{Child, Example, example::Choice},
        bytes_external::{Imported, imported::Nested},
    };
    let payload = || Bytes::from(vec![0x9b; len]);
    let child = || Child { payload: payload() };
    let mut message = Example::default();
    message.chosen = payload();
    message.ordinary = vec![0x7c; len];
    message.maybe = Some(payload());
    message.chunks = vec![payload(), Bytes::new()];
    message.lookup = [
        ("key".to_owned(), payload()),
        ("empty".to_owned(), Bytes::new()),
    ]
    .into();
    message.choice = Some(Choice::Raw(payload()));
    message.nested = Some(child());
    message.children = vec![child()];
    message.nodes = [(7, child())].into();
    message.imported = Some(Imported {
        payload: payload(),
        nested: Some(Nested { payload: payload() }),
        children: vec![Nested { payload: payload() }],
    });
    message.r#type = payload();
    message
}

fn within(bytes: &[u8], frame: &[u8]) -> bool {
    let start = frame.as_ptr().addr();
    let pointer = bytes.as_ptr().addr();
    pointer >= start
        && pointer
            .checked_add(bytes.len())
            .is_some_and(|end| end <= start + frame.len())
}
