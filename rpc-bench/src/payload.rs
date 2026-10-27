//! Deterministic payload fixtures for matched Prost transport comparisons.
//!
//! Noise is prepared before serving or measuring. Per-message work clones the
//! same byte vectors and compares complete bodies on both transports. Zero mode
//! keeps the existing zero-filled allocation and length-only checks.

use std::sync::OnceLock;

pub const DEFAULT_SEED: u64 = 20261026;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pattern {
    #[default]
    Zero,
    Noise,
}

impl std::str::FromStr for Pattern {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "zero" => Ok(Self::Zero),
            "noise" => Ok(Self::Noise),
            _ => Err("--payload-pattern must be zero or noise".into()),
        }
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
            Self::Zero => "zero",
            Self::Noise => "noise",
        })
    }
}

struct Fixtures {
    request: Vec<u8>,
    response: Vec<u8>,
}

impl Fixtures {
    fn new(seed: u64, request: usize, response: usize) -> Self {
        Self {
            request: noise(request, seed ^ 0x7265_7175_6573_7401),
            response: noise(response, seed ^ 0x7265_7370_6f6e_7365),
        }
    }

    fn response(&self, len: usize) -> Result<Vec<u8>, String> {
        self.response
            .get(..len)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "requested response exceeds --resp-bytes fixture bound".into())
    }

    fn validate_request(&self, body: &[u8]) -> Result<(), String> {
        if body == self.request {
            Ok(())
        } else {
            Err("request payload differs from configured noise fixture".into())
        }
    }

    fn validate_response(&self, body: &[u8]) -> Result<(), String> {
        if body == self.response {
            Ok(())
        } else {
            Err("response payload differs from configured noise fixture".into())
        }
    }
}

// A load process owns one workload. Tests use local Fixtures and never reset or
// configure this global, so concurrently running tests cannot change fixtures.
static FIXTURES: OnceLock<Fixtures> = OnceLock::new();

pub fn configure(
    pattern: Pattern,
    seed: u64,
    request: usize,
    response: usize,
) -> Result<(), String> {
    if request > crate::LOAD_MAX_PAYLOAD_BYTES || response > crate::LOAD_MAX_PAYLOAD_BYTES {
        return Err("payload fixture exceeds load payload bound".into());
    }
    if pattern == Pattern::Zero {
        if FIXTURES.get().is_some() {
            return Err("noise fixture already configured for this process".into());
        }
        return Ok(());
    }
    FIXTURES
        .set(Fixtures::new(seed, request, response))
        .map_err(|_| "payload fixture already configured for this process".into())
}

pub fn request_body(len: usize) -> Vec<u8> {
    match FIXTURES.get() {
        Some(fixtures) => {
            // Request constructors only use the exact size checked during
            // workload setup. A mismatch is a harness bug, not a new corpus.
            assert_eq!(len, fixtures.request.len(), "request fixture size changed");
            fixtures.request.clone()
        }
        None => vec![0; len],
    }
}

pub fn response_body(len: usize) -> Result<Vec<u8>, String> {
    match FIXTURES.get() {
        Some(fixtures) => fixtures.response(len),
        None => Ok(vec![0; len]),
    }
}

pub fn validate_request(body: &[u8]) -> Result<(), String> {
    FIXTURES
        .get()
        .map_or(Ok(()), |fixtures| fixtures.validate_request(body))
}

pub fn validate_response(body: &[u8]) -> Result<(), String> {
    FIXTURES
        .get()
        .map_or(Ok(()), |fixtures| fixtures.validate_response(body))
}

// SplitMix64 with fixed little-endian output gives a reproducible byte corpus;
// it is independent of the load-arrival seed and is not cryptographic randomness.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut out = vec![0; len];
    let mut state = seed;
    for bytes in out.chunks_mut(8) {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut word = state;
        word = (word ^ (word >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        word = (word ^ (word >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        word ^= word >> 31;
        bytes.copy_from_slice(&word.to_le_bytes()[..bytes.len()]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Fixtures, noise};

    #[test]
    fn noise_corpus_is_stable_and_prefix_preserving() {
        assert_eq!(
            noise(8, 0),
            0xe220_a839_7b1d_cdaf_u64.to_le_bytes().to_vec()
        );
        assert_eq!(noise(11, 7), noise(19, 7)[..11]);
        assert_ne!(noise(19, 7), noise(19, 8));
    }

    #[test]
    fn fixtures_distinguish_directions_and_reject_corruption_and_zero_substitution() {
        let fixtures = Fixtures::new(7, 1024, 1024);
        assert_ne!(fixtures.request, fixtures.response);
        assert!(fixtures.validate_request(&fixtures.request).is_ok());
        assert!(fixtures.validate_response(&fixtures.response).is_ok());
        assert!(fixtures.validate_response(&fixtures.request).is_err());
        assert!(fixtures.validate_response(&vec![0; 1024]).is_err());
        let mut corrupt = fixtures.response.clone();
        corrupt[1023] ^= 1;
        assert!(fixtures.validate_response(&corrupt).is_err());
        assert!(fixtures.validate_response(&corrupt[..1023]).is_err());
    }

    #[test]
    fn response_fixture_bound_does_not_generate_new_payloads() {
        let fixtures = Fixtures::new(0, 0, 9);
        assert!(fixtures.validate_request(&[]).is_ok());
        assert_eq!(fixtures.response(0).unwrap(), Vec::<u8>::new());
        assert_eq!(fixtures.response(9).unwrap(), fixtures.response);
        assert!(fixtures.response(10).is_err());
    }
}
