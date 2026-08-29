//! Resource caps applied to every RPC.
//!
//! Every limit here is enforced *before* the memory it guards is committed:
//! a frame length is rejected from the 5-byte header, and a compressed frame
//! is inflated through a bounded reader that stops one byte past the cap.
//! See [the threat model](crate#threat-model).

use crate::status::Status;

/// Default inbound message cap: 4 MiB, matching gRPC's cross-language default.
pub const DEFAULT_MAX_DECODING_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Per-message size caps. `None` means unlimited.
///
/// Both caps count *uncompressed* protobuf bytes, so a compressed frame is
/// measured by what it inflates to, not by what arrived on the wire.
///
/// ```
/// # use pbrs_grpc::MessageLimits;
/// let limits = MessageLimits::default();
/// assert_eq!(limits.max_decoding(), Some(4 * 1024 * 1024));
/// assert_eq!(limits.max_encoding(), None);
///
/// let unlimited = MessageLimits::unlimited();
/// assert_eq!(unlimited.max_decoding(), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageLimits {
    max_decoding: Option<usize>,
    max_encoding: Option<usize>,
}

impl Default for MessageLimits {
    /// 4 MiB inbound, unlimited outbound.
    ///
    /// Inbound is capped because a peer controls it; outbound is not because
    /// the local service does.
    fn default() -> Self {
        Self {
            max_decoding: Some(DEFAULT_MAX_DECODING_MESSAGE_SIZE),
            max_encoding: None,
        }
    }
}

impl MessageLimits {
    /// Defaults: 4 MiB inbound, unlimited outbound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// No caps in either direction.
    ///
    /// Only appropriate when every peer is trusted: a single hostile frame
    /// header can then ask for as much memory as `u32::MAX` allows.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_decoding: None,
            max_encoding: None,
        }
    }

    /// Cap inbound messages at `limit` uncompressed bytes.
    /// Applies to every call shape.
    #[must_use]
    pub fn with_max_decoding(mut self, limit: usize) -> Self {
        self.max_decoding = Some(limit);
        self
    }

    /// Cap outbound messages at `limit` uncompressed bytes.
    /// Applies to every call shape.
    #[must_use]
    pub fn with_max_encoding(mut self, limit: usize) -> Self {
        self.max_encoding = Some(limit);
        self
    }

    /// Lift the inbound cap.
    #[must_use]
    pub fn with_unlimited_decoding(mut self) -> Self {
        self.max_decoding = None;
        self
    }

    /// Lift the outbound cap.
    #[must_use]
    pub fn with_unlimited_encoding(mut self) -> Self {
        self.max_encoding = None;
        self
    }

    /// Inbound cap in bytes.
    #[must_use]
    pub fn max_decoding(self) -> Option<usize> {
        self.max_decoding
    }

    /// Outbound cap in bytes.
    #[must_use]
    pub fn max_encoding(self) -> Option<usize> {
        self.max_encoding
    }

    pub(crate) fn check_decode(self, n: usize) -> Result<(), Status> {
        match self.max_decoding {
            Some(max) if n > max => Err(Status::resource_exhausted(format!(
                "decoded message length {n} exceeds limit {max}"
            ))),
            _ => Ok(()),
        }
    }

    pub(crate) fn check_encode(self, n: usize) -> Result<(), Status> {
        match self.max_encoding {
            Some(max) if n > max => Err(Status::resource_exhausted(format!(
                "encoded message length {n} exceeds limit {max}"
            ))),
            _ => Ok(()),
        }
    }

    /// How many decompressed bytes an inbound frame may produce.
    ///
    /// An unlimited configuration really is unlimited here: bounded inflate can
    /// only stop where a cap tells it to. That is why [`Self::unlimited`]
    /// documents itself as trusted-peer only.
    pub(crate) fn inflate_budget(self) -> usize {
        self.max_decoding.unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::MessageLimits;
    use crate::status::Code;

    #[test]
    fn default_caps_inbound_only() {
        let limits = MessageLimits::default();
        assert!(limits.check_decode(4 * 1024 * 1024).is_ok());
        let err = limits
            .check_decode(4 * 1024 * 1024 + 1)
            .expect_err("over cap");
        assert_eq!(err.code(), Code::ResourceExhausted);
        assert!(limits.check_encode(usize::MAX).is_ok());
    }

    #[test]
    fn unlimited_accepts_anything() {
        let limits = MessageLimits::unlimited();
        assert!(limits.check_decode(usize::MAX).is_ok());
        assert!(limits.check_encode(usize::MAX).is_ok());
    }

    #[test]
    fn builders_round_trip() {
        let limits = MessageLimits::new()
            .with_max_decoding(7)
            .with_max_encoding(9);
        assert_eq!(limits.max_decoding(), Some(7));
        assert_eq!(limits.max_encoding(), Some(9));
        let lifted = limits.with_unlimited_decoding().with_unlimited_encoding();
        assert_eq!(lifted, MessageLimits::unlimited());
    }
}
