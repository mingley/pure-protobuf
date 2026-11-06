//! Preserve valid callers that already implement Default for their own view.
use pbrs::prelude::*;

fn pool() -> ::std::sync::Arc<pbrs::DescriptorPool> {
    static POOL: ::std::sync::OnceLock<::std::sync::Arc<pbrs::DescriptorPool>> =
        ::std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        ::std::sync::Arc::new(
            pbrs::DescriptorPool::from_file_descriptor_set(
                crate::native::proof::symbols::FILE_DESCRIPTOR_SET,
            )
            .unwrap(),
        )
    })
    .clone()
}

pbrs::impl_generated_message!(
    Original,
    OriginalView,
    OriginalMut,
    "proof.symbols.Record",
    pool()
);

impl ::core::default::Default for OriginalView<'_> {
    fn default() -> Self {
        static DEFAULT: ::std::sync::OnceLock<Original> = ::std::sync::OnceLock::new();
        Self(pbrs::gen_support::default_instance(&DEFAULT))
    }
}

pub fn run() {
    assert!(OriginalView::default().serialize().unwrap().is_empty());
    let value = Original::from_json("{\"count\":7}").unwrap();
    assert_eq!(value.serialize().unwrap(), [8, 7]);
    assert_eq!(Original::parse(&[8, 7]).unwrap(), value);
}

#[cfg(feature = "bare-legacy")]
mod bare {
    // Expected E0277: the original form still requires a caller Default impl.
    pbrs::impl_generated_message!(
        Bare,
        BareView,
        BareMut,
        "proof.symbols.Record",
        super::pool()
    );
}
