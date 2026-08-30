//! gRPC metadata: ASCII headers and base64 `-bin` headers.

use crate::status::Status;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine;
use http::{HeaderMap, HeaderName, HeaderValue};
use std::fmt;

/// gRPC metadata, i.e. HTTP/2 headers or trailers minus the reserved ones.
///
/// Keys ending in `-bin` carry arbitrary bytes and travel base64-encoded;
/// every other key carries ASCII. The two namespaces are kept apart by
/// [`Self::insert`] / [`Self::set`] and [`Self::insert_bin`] /
/// [`Self::set_bin`], which reject a mismatched suffix rather than silently
/// producing metadata no gRPC peer can read. `insert` appends; `set`
/// replaces.
///
/// ```
/// use pbrs_grpc::Metadata;
///
/// let mut md = Metadata::new();
/// md.insert("x-request-id", "abc123")?;
/// md.insert("x-request-id", "again")?;
/// md.insert_bin("x-trace-bin", [0xde, 0xad])?;
///
/// assert_eq!(md.get("X-Request-Id"), Some("abc123"));
/// assert_eq!(
///     md.get_all("x-request-id").collect::<Vec<_>>(),
///     vec!["abc123", "again"]
/// );
/// md.set("x-request-id", "other")?;
/// assert_eq!(md.get_all("x-request-id").collect::<Vec<_>>(), vec!["other"]);
/// assert!(md.contains("x-request-id"));
/// md.set_bin("x-trace-bin", [0xbe, 0xef])?;
/// assert_eq!(md.get_bin("x-trace-bin").as_deref(), Some(&[0xbe, 0xef][..]));
/// assert!(md.contains_bin("x-trace-bin"));
/// md.insert("legacy", "drop-me")?;
/// md.retain(|k| k.starts_with("x-"));
/// assert_eq!(md.get("legacy"), None);
/// assert_eq!(md.keys().collect::<Vec<_>>(), vec!["x-request-id", "x-trace-bin"]);
/// md.merge(&md.clone());
/// assert_eq!(md.len(), 4);
/// md.clear();
/// assert!(md.is_empty());
/// assert!(md.insert("bad-bin", "not base64").is_err());
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
///
/// Reserved keys (`grpc-*`, `content-type`, HTTP/2 pseudo-headers,
/// hop-by-hop headers, ...) are invisible here and are never written out, so
/// echoing received metadata back cannot corrupt the protocol framing.
/// [`Self::insert`], [`Self::set`], [`Self::insert_bin`], and
/// [`Self::set_bin`] reject them rather than storing a value you cannot
/// read back. `Debug` omits them too, so a dumped interceptor `Rpc` or
/// `Outgoing` does not look like it can rewrite `grpc-status`. `user-agent`
/// is readable; on outbound requests the kernel overwrites it after user
/// metadata so a smuggled value cannot win.
///
/// The total size a peer can send is bounded by
/// [`ServerConfig::max_header_list_size`](crate::ServerConfig::max_header_list_size),
/// not by this type.
#[derive(Clone, Default)]
pub struct Metadata {
    map: HeaderMap,
}

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_map();
        for (k, v) in &self.map {
            if is_reserved(k.as_str()) {
                continue;
            }
            s.entry(&k.as_str(), v);
        }
        s.finish()
    }
}

impl Metadata {
    /// Empty metadata.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether any non-reserved entry is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.map.keys().any(|k| !is_reserved(k.as_str()))
    }

    /// Number of non-reserved entries, counting repeats of the same key.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map
            .iter()
            .filter(|(k, _)| !is_reserved(k.as_str()))
            .count()
    }

    /// Add an ASCII entry. The key must not end in `-bin` and must not be a
    /// reserved protocol key (`grpc-*`, `content-type`, hop-by-hop headers,
    /// ...).
    ///
    /// Repeated keys accumulate rather than replace, matching gRPC's
    /// comma-joined multi-value semantics. To overwrite, use [`Self::set`].
    pub fn insert(&mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Result<(), Status> {
        let key = key.as_ref();
        if is_reserved(key) {
            return Err(Status::invalid_argument(format!(
                "reserved metadata key {key:?}"
            )));
        }
        if key.ends_with("-bin") {
            return Err(Status::invalid_argument(
                "ascii metadata key must not end in -bin",
            ));
        }
        let name = header_name(key)?;
        let value = HeaderValue::from_str(value.as_ref())
            .map_err(|_| Status::invalid_argument("metadata value is not valid ASCII"))?;
        self.map.append(name, value);
        Ok(())
    }

    /// Replace every ASCII entry for `key` with `value`.
    ///
    /// [`Self::insert`] appends. This is the last-write-wins form an
    /// interceptor uses when it owns a hop (`authorization`, `x-request-id`).
    /// Validation matches [`Self::insert`]: reserved names and `-bin` keys
    /// are rejected, and a failed call leaves the map unchanged.
    pub fn set(&mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Result<(), Status> {
        let key = key.as_ref();
        if is_reserved(key) {
            return Err(Status::invalid_argument(format!(
                "reserved metadata key {key:?}"
            )));
        }
        if key.ends_with("-bin") {
            return Err(Status::invalid_argument(
                "ascii metadata key must not end in -bin",
            ));
        }
        let name = header_name(key)?;
        let value = HeaderValue::from_str(value.as_ref())
            .map_err(|_| Status::invalid_argument("metadata value is not valid ASCII"))?;
        drop(self.map.insert(name, value));
        Ok(())
    }

    /// Add a binary entry. The key must end in `-bin` and must not be reserved.
    ///
    /// Repeats accumulate. To overwrite, use [`Self::set_bin`].
    pub fn insert_bin(
        &mut self,
        key: impl AsRef<str>,
        value: impl AsRef<[u8]>,
    ) -> Result<(), Status> {
        let key = key.as_ref();
        if is_reserved(key) {
            return Err(Status::invalid_argument(format!(
                "reserved metadata key {key:?}"
            )));
        }
        if !key.ends_with("-bin") {
            return Err(Status::invalid_argument(
                "binary metadata key must end in -bin",
            ));
        }
        let name = header_name(key)?;
        let encoded = STANDARD_NO_PAD.encode(value.as_ref());
        let value = HeaderValue::from_str(&encoded)
            .map_err(|e| Status::internal(format!("base64 metadata: {e}")))?;
        self.map.append(name, value);
        Ok(())
    }

    /// Replace every `-bin` entry for `key` with `value`.
    ///
    /// [`Self::insert_bin`] appends. This is the last-write-wins form an
    /// interceptor uses when it owns a `-bin` hop. Validation matches
    /// [`Self::insert_bin`]:
    /// reserved names and non-`-bin` keys are rejected, and a failed call
    /// leaves the map unchanged.
    pub fn set_bin(&mut self, key: impl AsRef<str>, value: impl AsRef<[u8]>) -> Result<(), Status> {
        let key = key.as_ref();
        if is_reserved(key) {
            return Err(Status::invalid_argument(format!(
                "reserved metadata key {key:?}"
            )));
        }
        if !key.ends_with("-bin") {
            return Err(Status::invalid_argument(
                "binary metadata key must end in -bin",
            ));
        }
        let name = header_name(key)?;
        let encoded = STANDARD_NO_PAD.encode(value.as_ref());
        let value = HeaderValue::from_str(&encoded)
            .map_err(|e| Status::internal(format!("base64 metadata: {e}")))?;
        drop(self.map.insert(name, value));
        Ok(())
    }

    /// Remove every ASCII entry for `key`. Reserved keys and `-bin` keys are
    /// left alone. Returns whether anything was removed.
    pub fn remove(&mut self, key: &str) -> bool {
        if is_reserved(key) || key.ends_with("-bin") {
            return false;
        }
        let Ok(name) = HeaderName::from_bytes(key.to_ascii_lowercase().as_bytes()) else {
            return false;
        };
        self.map.remove(name).is_some()
    }

    /// Remove every `-bin` entry for `key`. Returns whether anything was removed.
    pub fn remove_bin(&mut self, key: &str) -> bool {
        if is_reserved(key) || !key.ends_with("-bin") {
            return false;
        }
        let Ok(name) = HeaderName::from_bytes(key.to_ascii_lowercase().as_bytes()) else {
            return false;
        };
        self.map.remove(name).is_some()
    }

    /// First ASCII value for `key`, matched case-insensitively.
    ///
    /// Repeats are kept; this is the first. Use [`Self::get_all`] for every
    /// value in insertion order.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        if is_reserved(key) {
            return None;
        }
        self.map.get(key.to_ascii_lowercase())?.to_str().ok()
    }

    /// First `-bin` value for `key`, base64-decoded.
    ///
    /// Repeats are kept; this is the first. Use [`Self::get_all_bin`] for
    /// every value in insertion order.
    #[must_use]
    pub fn get_bin(&self, key: &str) -> Option<Vec<u8>> {
        if is_reserved(key) {
            return None;
        }
        let raw = self.map.get(key.to_ascii_lowercase())?.to_str().ok()?;
        decode_base64(raw)
    }

    /// Every ASCII value for `key`, in insertion order.
    ///
    /// [`Self::insert`] appends rather than replacing, so a peer (or an
    /// interceptor that adds a second `x-forwarded-for`) is visible here.
    /// [`Self::set`] replaces every value. [`Self::get`] is the first of
    /// these. Reserved keys yield nothing.
    pub fn get_all(&self, key: &str) -> impl Iterator<Item = &str> + '_ {
        let skip = is_reserved(key);
        self.map
            .get_all(key)
            .iter()
            .filter(move |_| !skip)
            .filter_map(|value| value.to_str().ok())
    }

    /// Every `-bin` value for `key`, base64-decoded, in insertion order.
    ///
    /// [`Self::insert_bin`] appends; [`Self::set_bin`] replaces.
    /// [`Self::get_bin`] is the first of these. Reserved keys yield nothing.
    pub fn get_all_bin(&self, key: &str) -> impl Iterator<Item = Vec<u8>> + '_ {
        let skip = is_reserved(key);
        self.map
            .get_all(key)
            .iter()
            .filter(move |_| !skip)
            .filter_map(|value| decode_base64(value.to_str().ok()?))
    }

    /// Whether `key` has an ASCII value. Reserved names are never present.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Whether `key` has a `-bin` value. Reserved names are never present.
    #[must_use]
    pub fn contains_bin(&self, key: &str) -> bool {
        self.get_bin(key).is_some()
    }

    /// Unique non-reserved keys, ASCII and `-bin`, in first-insertion order.
    ///
    /// Repeats of the same key appear once; use [`Self::get_all`] /
    /// [`Self::get_all_bin`] for the values. Reserved protocol names are
    /// omitted, matching every other read path.
    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.map.keys().filter_map(|name| {
            let key = name.as_str();
            if is_reserved(key) {
                None
            } else {
                Some(key)
            }
        })
    }

    /// Drop every entry.
    ///
    /// After this, [`Self::is_empty`] is true and nothing is written on the
    /// wire. An interceptor that wants to forward none of the caller's
    /// metadata can clear and then insert what it still needs.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Append every user entry from `other`.
    ///
    /// Reserved names in `other` are not copied. Repeats accumulate, same as
    /// [`Self::insert`]. `other` is left unchanged.
    pub fn merge(&mut self, other: &Self) {
        for (name, value) in &other.map {
            if is_reserved(name.as_str()) {
                continue;
            }
            self.map.append(name.clone(), value.clone());
        }
    }

    /// Keep user entries for which `f(key)` is true.
    ///
    /// The predicate is called once per unique name; repeats of that name
    /// are kept or dropped together. Reserved protocol names are always
    /// dropped (they were never readable). An interceptor that forwards a
    /// subset of hops uses this instead of rebuilding the map with
    /// [`Self::clear`] and [`Self::insert`].
    pub fn retain(&mut self, mut f: impl FnMut(&str) -> bool) {
        let drop_names: Vec<HeaderName> = self
            .map
            .keys()
            .filter(|name| {
                let key = name.as_str();
                is_reserved(key) || !f(key)
            })
            .cloned()
            .collect();
        for name in drop_names {
            drop(self.map.remove(name));
        }
    }

    /// Every ASCII entry, skipping reserved and `-bin` keys.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.map.iter().filter_map(|(name, value)| {
            let key = name.as_str();
            if is_reserved(key) || key.ends_with("-bin") {
                return None;
            }
            Some((key, value.to_str().ok()?))
        })
    }

    /// Every `-bin` entry, base64-decoded.
    pub fn iter_bin(&self) -> impl Iterator<Item = (&str, Vec<u8>)> + '_ {
        self.map.iter().filter_map(|(name, value)| {
            let key = name.as_str();
            if is_reserved(key) || !key.ends_with("-bin") {
                return None;
            }
            Some((key, decode_base64(value.to_str().ok()?)?))
        })
    }

    /// Take ownership of a received header map.
    ///
    /// No per-entry copying: reserved keys are filtered on read and on write,
    /// so receiving metadata costs nothing until it is used.
    pub(crate) fn from_owned_headers(map: HeaderMap) -> Self {
        Self { map }
    }

    pub(crate) fn from_headers(map: &HeaderMap) -> Self {
        Self { map: map.clone() }
    }

    pub(crate) fn write_to(&self, headers: &mut HeaderMap) -> Result<(), Status> {
        for (name, value) in &self.map {
            if is_reserved(name.as_str()) {
                continue;
            }
            headers.append(name.clone(), value.clone());
        }
        Ok(())
    }
}

fn header_name(key: &str) -> Result<HeaderName, Status> {
    HeaderName::from_bytes(key.as_bytes())
        .map_err(|_| Status::invalid_argument(format!("invalid metadata key {key:?}")))
}

/// Keys the gRPC wire protocol owns, plus HTTP/2 hop-by-hop headers.
///
/// The spec forbids user metadata whose names start with `grpc-`. Pseudo-headers
/// and hop-by-hop names (`connection`, `host`, ...) are similarly not metadata.
/// Never surfaced, never echoed.
fn is_reserved(key: &str) -> bool {
    key.starts_with(':')
        || key
            .as_bytes()
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case(b"grpc-"))
        || key.eq_ignore_ascii_case("content-type")
        || key.eq_ignore_ascii_case("te")
        || key.eq_ignore_ascii_case("connection")
        || key.eq_ignore_ascii_case("keep-alive")
        || key.eq_ignore_ascii_case("proxy-connection")
        || key.eq_ignore_ascii_case("transfer-encoding")
        || key.eq_ignore_ascii_case("upgrade")
        || key.eq_ignore_ascii_case("host")
        || key.eq_ignore_ascii_case("content-length")
}

/// Accept padded and unpadded standard base64. Peers disagree on padding;
/// outbound `-bin` values are unpadded, inbound either form is accepted.
pub(crate) fn decode_base64(raw: &str) -> Option<Vec<u8>> {
    match STANDARD_NO_PAD.decode(raw) {
        Ok(bytes) => Some(bytes),
        Err(_) => STANDARD.decode(raw).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::Metadata;
    use http::{HeaderMap, HeaderName, HeaderValue};

    #[test]
    fn ascii_and_bin_are_separate_namespaces() {
        let mut md = Metadata::new();
        md.insert("a", "1").expect("ascii");
        md.insert_bin("b-bin", [7u8, 8]).expect("bin");
        assert_eq!(md.get("A"), Some("1"));
        assert_eq!(md.get_bin("B-Bin").as_deref(), Some(&[7u8, 8][..]));
        assert!(md.insert("c-bin", "x").is_err());
        assert!(md.insert_bin("d", [0u8]).is_err());
    }

    #[test]
    fn reserved_keys_are_invisible() {
        let mut raw = HeaderMap::new();
        raw.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/grpc"),
        );
        raw.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        raw.insert(
            HeaderName::from_static("grpc-status-details-bin"),
            HeaderValue::from_static("CAU"),
        );
        raw.insert(
            HeaderName::from_static("x-real"),
            HeaderValue::from_static("v"),
        );
        let md = Metadata::from_headers(&raw);
        assert!(!md.is_empty());
        assert_eq!(md.len(), 1);
        assert_eq!(md.get("content-type"), None);
        assert_eq!(md.get("grpc-status"), None);
        assert_eq!(md.get_bin("grpc-status-details-bin"), None);
        assert_eq!(md.get("x-real"), Some("v"));
        let shown = format!("{md:?}");
        assert!(shown.contains("x-real"), "{shown}");
        assert!(!shown.contains("grpc-status"), "{shown}");
        assert!(!shown.contains("content-type"), "{shown}");

        let mut out = HeaderMap::new();
        md.write_to(&mut out).expect("write");
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("x-real"));
    }

    #[test]
    fn only_reserved_keys_reads_as_empty() {
        let mut raw = HeaderMap::new();
        raw.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/grpc"),
        );
        assert!(Metadata::from_headers(&raw).is_empty());
    }

    #[test]
    fn repeated_keys_accumulate() {
        let mut md = Metadata::new();
        md.insert("k", "1").expect("first");
        md.insert("k", "2").expect("second");
        assert_eq!(md.len(), 2);
        assert_eq!(md.get("k"), Some("1"));
        let values: Vec<_> = md.iter().collect();
        assert_eq!(values, vec![("k", "1"), ("k", "2")]);
        assert_eq!(md.get_all("k").collect::<Vec<_>>(), vec!["1", "2"]);
        assert_eq!(md.get_all("K").collect::<Vec<_>>(), vec!["1", "2"]);
        assert_eq!(md.get_all("missing").count(), 0);
    }

    #[test]
    fn get_all_bin_yields_repeated_values() {
        let mut md = Metadata::new();
        md.insert_bin("blob-bin", [1u8]).expect("first");
        md.insert_bin("blob-bin", [2u8]).expect("second");
        assert_eq!(md.get_bin("blob-bin").as_deref(), Some(&[1u8][..]));
        assert_eq!(
            md.get_all_bin("blob-bin").collect::<Vec<_>>(),
            vec![vec![1u8], vec![2u8]]
        );
        assert_eq!(md.get_all_bin("missing-bin").count(), 0);
    }

    #[test]
    fn get_all_hides_reserved_keys() {
        let mut raw = HeaderMap::new();
        raw.append(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("5"),
        );
        raw.append(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("14"),
        );
        raw.append(
            HeaderName::from_static("grpc-status-details-bin"),
            HeaderValue::from_static("CAU"),
        );
        let md = Metadata::from_headers(&raw);
        assert_eq!(md.get_all("grpc-status").count(), 0);
        assert_eq!(md.get_all_bin("grpc-status-details-bin").count(), 0);
        assert!(!md.contains("grpc-status"));
        assert!(!md.contains_bin("grpc-status-details-bin"));
    }

    #[test]
    fn contains_matches_get() {
        let mut md = Metadata::new();
        md.insert("x-tenant", "acme").expect("ascii");
        md.insert_bin("x-trace-bin", [0xdeu8, 0xad]).expect("bin");
        assert!(md.contains("X-Tenant"));
        assert!(md.contains_bin("x-trace-bin"));
        assert!(!md.contains("missing"));
        assert!(!md.contains_bin("missing-bin"));
        assert!(!md.contains("grpc-timeout"));
        assert!(!md.contains_bin("grpc-status-details-bin"));
    }

    #[test]
    fn remove_drops_user_keys_and_ignores_reserved() {
        let mut md = Metadata::new();
        md.insert("x-trace", "a").expect("ascii");
        md.insert_bin("x-blob-bin", [1u8]).expect("bin");
        assert!(md.remove("x-trace"));
        assert!(!md.remove("x-trace"));
        assert_eq!(md.get("x-trace"), None);
        assert!(md.remove_bin("x-blob-bin"));
        assert_eq!(md.get_bin("x-blob-bin"), None);
        assert!(md.insert("grpc-status", "5").is_err());
        assert!(!md.remove("grpc-status"));
    }

    #[test]
    fn insert_rejects_reserved_keys() {
        let mut md = Metadata::new();
        assert!(md.insert("grpc-timeout", "1S").is_err());
        assert!(md.insert("GRPC-previous-rpc-attempts", "1").is_err());
        assert!(md.insert("grpc-foo", "x").is_err());
        assert!(md.insert("content-type", "application/grpc").is_err());
        assert!(md.insert("connection", "close").is_err());
        assert!(md.insert("host", "evil").is_err());
        assert!(md.insert_bin("grpc-status-details-bin", [1u8]).is_err());
        assert!(md.insert_bin("grpc-retry-pushback-ms", [1u8]).is_err());
        assert!(md.is_empty());
    }

    #[test]
    fn keys_lists_unique_user_names() {
        let mut md = Metadata::new();
        md.insert("x-tenant", "acme").expect("ascii");
        md.insert("x-tenant", "other").expect("repeat");
        md.insert_bin("x-trace-bin", [1u8]).expect("bin");
        assert_eq!(
            md.keys().collect::<Vec<_>>(),
            vec!["x-tenant", "x-trace-bin"]
        );

        let mut raw = HeaderMap::new();
        raw.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        raw.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/grpc"),
        );
        raw.insert(
            HeaderName::from_static("x-real"),
            HeaderValue::from_static("v"),
        );
        let md = Metadata::from_headers(&raw);
        assert_eq!(md.keys().collect::<Vec<_>>(), vec!["x-real"]);
        assert!(Metadata::new().keys().next().is_none());
    }

    #[test]
    fn clear_drops_user_entries() {
        let mut md = Metadata::new();
        md.insert("x-tenant", "acme").expect("ascii");
        md.insert_bin("x-trace-bin", [1u8]).expect("bin");
        md.clear();
        assert!(md.is_empty());
        assert_eq!(md.len(), 0);
        assert!(md.keys().next().is_none());

        let mut raw = HeaderMap::new();
        raw.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        raw.insert(
            HeaderName::from_static("x-real"),
            HeaderValue::from_static("v"),
        );
        let mut md = Metadata::from_headers(&raw);
        md.clear();
        assert!(md.is_empty());
        assert_eq!(md.get("x-real"), None);
        assert_eq!(md.get("grpc-status"), None);
    }

    #[test]
    fn merge_appends_user_entries_and_skips_reserved() {
        let mut src = HeaderMap::new();
        src.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("5"),
        );
        src.insert(
            HeaderName::from_static("x-from"),
            HeaderValue::from_static("a"),
        );
        src.append(
            HeaderName::from_static("x-from"),
            HeaderValue::from_static("b"),
        );
        src.insert(
            HeaderName::from_static("blob-bin"),
            HeaderValue::from_static("AQ"),
        );
        let src = Metadata::from_headers(&src);

        let mut dst = Metadata::new();
        dst.insert("x-from", "keep").expect("existing");
        dst.merge(&src);
        assert_eq!(
            dst.get_all("x-from").collect::<Vec<_>>(),
            vec!["keep", "a", "b"]
        );
        assert_eq!(dst.get_bin("blob-bin").as_deref(), Some(&[1u8][..]));
        assert_eq!(dst.get_all("grpc-status").count(), 0);
        assert_eq!(src.get("x-from"), Some("a"));
    }

    #[test]
    fn set_replaces_existing_values() {
        let mut md = Metadata::new();
        md.insert("x-id", "a").expect("first");
        md.insert("x-id", "b").expect("second");
        assert_eq!(md.len(), 2);
        md.set("x-id", "c").expect("replace");
        assert_eq!(md.get("x-id"), Some("c"));
        assert_eq!(md.get_all("x-id").collect::<Vec<_>>(), vec!["c"]);
        assert_eq!(md.len(), 1);
        md.insert("x-id", "d").expect("append after set");
        assert_eq!(md.get_all("x-id").collect::<Vec<_>>(), vec!["c", "d"]);
    }

    #[test]
    fn set_bin_replaces_existing_values() {
        let mut md = Metadata::new();
        md.insert_bin("x-token-bin", b"aa").expect("first");
        md.insert_bin("x-token-bin", b"bb").expect("second");
        assert_eq!(md.len(), 2);
        md.set_bin("x-token-bin", b"cc").expect("replace");
        assert_eq!(md.get_bin("x-token-bin").as_deref(), Some(&b"cc"[..]));
        assert_eq!(
            md.get_all_bin("x-token-bin").collect::<Vec<_>>(),
            vec![b"cc".to_vec()]
        );
        assert_eq!(md.len(), 1);
        md.insert_bin("x-token-bin", b"dd")
            .expect("append after set");
        assert_eq!(
            md.get_all_bin("x-token-bin").collect::<Vec<_>>(),
            vec![b"cc".to_vec(), b"dd".to_vec()]
        );
    }

    #[test]
    fn set_rejects_reserved_and_leaves_map() {
        let mut md = Metadata::new();
        md.insert("x-ok", "v").expect("seed");
        assert!(md.set("grpc-status", "0").is_err());
        assert!(md.set("x-token-bin", "ascii").is_err());
        assert!(md.set("x-ok", "line\nbreak").is_err());
        assert!(md.set_bin("authorization", b"x").is_err());
        assert!(md.set_bin("x-ok", b"x").is_err());
        assert_eq!(md.len(), 1);
        assert_eq!(md.get("x-ok"), Some("v"));
    }

    #[test]
    fn retain_keeps_matching_keys() {
        let mut md = Metadata::new();
        md.insert("x-keep", "a").expect("ascii");
        md.insert("x-keep", "b").expect("repeat");
        md.insert("y-drop", "c").expect("other");
        md.insert_bin("x-trace-bin", [1u8]).expect("bin");
        md.retain(|k| k.starts_with("x-"));
        assert_eq!(md.get_all("x-keep").collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(md.get("y-drop"), None);
        assert_eq!(md.get_bin("x-trace-bin").as_deref(), Some(&[1u8][..]));
        assert_eq!(md.keys().collect::<Vec<_>>(), vec!["x-keep", "x-trace-bin"]);
    }

    #[test]
    fn retain_drops_reserved_names() {
        let mut raw = HeaderMap::new();
        raw.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        raw.insert(
            HeaderName::from_static("x-ok"),
            HeaderValue::from_static("v"),
        );
        let mut md = Metadata::from_headers(&raw);
        md.retain(|_| true);
        assert_eq!(md.get("x-ok"), Some("v"));
        assert_eq!(md.get("grpc-status"), None);
        md.retain(|_| false);
        assert!(md.is_empty());
        assert_eq!(md.len(), 0);
    }

    #[test]
    fn iterators_split_by_suffix() {
        let mut md = Metadata::new();
        md.insert("plain", "v").expect("ascii");
        md.insert_bin("blob-bin", b"raw").expect("bin");
        assert_eq!(md.iter().collect::<Vec<_>>(), vec![("plain", "v")]);
        let bins: Vec<_> = md.iter_bin().collect();
        assert_eq!(bins, vec![("blob-bin", b"raw".to_vec())]);
    }

    #[test]
    fn padded_and_unpadded_bin_values_decode() {
        let mut raw = HeaderMap::new();
        raw.insert(
            HeaderName::from_static("a-bin"),
            HeaderValue::from_static("AA"),
        );
        raw.insert(
            HeaderName::from_static("b-bin"),
            HeaderValue::from_static("AA=="),
        );
        let md = Metadata::from_headers(&raw);
        assert_eq!(md.get_bin("a-bin").as_deref(), Some(&[0u8][..]));
        assert_eq!(md.get_bin("b-bin").as_deref(), Some(&[0u8][..]));
    }

    #[test]
    fn invalid_keys_and_values_are_rejected() {
        let mut md = Metadata::new();
        assert!(md.insert("bad key", "v").is_err());
        assert!(md.insert("k", "line\nbreak").is_err());
    }
}
