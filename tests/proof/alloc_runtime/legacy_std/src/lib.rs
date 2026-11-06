//! Existing no-default-feature consumers retain their standard-library API.
use bytes::Buf as _;
use pbrs::IntoProxied as _;

/// Exercise std-specific runtime conversions and Bytes integration.
pub fn standard_apis() {
    let _: pbrs::ProtoString =
        std::ffi::OsStr::new("old API").into_proxied(pbrs::__internal::Private);
    let mut input = pbrs::rt::Bytes::from_static(b"ok").reader();
    let mut out = [0; 2];
    std::io::Read::read_exact(&mut input, &mut out).unwrap();
    assert_eq!(&out, b"ok");
    fn error<T: std::error::Error>() {}
    error::<pbrs::ParseError>();
    error::<pbrs::SerializeError>();
    error::<pbrs::Utf8Error>();
}

#[cfg(test)]
mod tests {
    #[test]
    fn existing_standard_library_apis() {
        super::standard_apis();
    }
}
