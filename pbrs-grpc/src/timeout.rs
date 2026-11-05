//! The `grpc-timeout` header: `{digits}{unit}`.
//!
//! gRPC carries a deadline as a duration rather than an absolute time, so peers
//! need no clock agreement. The value is up to eight digits followed by a unit:
//!
//! | Unit | Meaning |
//! |---|---|
//! | `n` | nanoseconds |
//! | `u` | microseconds |
//! | `m` | milliseconds |
//! | `S` | seconds |
//! | `M` | minutes |
//! | `H` | hours |
//!
//! ```
//! use pbrs_grpc::timeout;
//! use std::time::Duration;
//!
//! assert_eq!(timeout::encode_timeout(Duration::from_millis(250)), "250m");
//! assert_eq!(
//!     timeout::parse_timeout("250m"),
//!     Some(Duration::from_millis(250))
//! );
//! ```

use crate::Status;
use crate::rt::{Runtime, TokioRuntime};
use std::time::Duration;

/// Largest value the eight-digit field can hold.
const MAX_DIGITS_VALUE: u128 = 99_999_999;

const NANOS_PER_MICRO: u128 = 1_000;
const NANOS_PER_MILLI: u128 = 1_000_000;
const NANOS_PER_SEC: u128 = 1_000_000_000;
const NANOS_PER_MINUTE: u128 = 60 * NANOS_PER_SEC;
const NANOS_PER_HOUR: u128 = 3_600 * NANOS_PER_SEC;

/// Encode a duration as a `grpc-timeout` value.
///
/// Uses an exact unit when possible, preferring seconds for whole-second
/// timeouts. Otherwise picks the most precise unit that fits in eight digits
/// and rounds up, so conversion does not shorten the requested timeout.
///
/// ```
/// use pbrs_grpc::timeout::encode_timeout;
/// use std::time::Duration;
///
/// assert_eq!(encode_timeout(Duration::from_secs(5)), "5S");
/// assert_eq!(encode_timeout(Duration::from_millis(1500)), "1500m");
/// assert_eq!(encode_timeout(Duration::from_nanos(7)), "7n");
/// ```
///
/// A duration above the wire format's maximum of `99999999H` (about 11,415
/// years) saturates at that value. The local call still uses its original
/// timeout when that timeout fits the platform's monotonic clock.
#[must_use]
pub fn encode_timeout(d: Duration) -> String {
    let nanos = d.as_nanos();
    for (per_unit, suffix) in [
        (NANOS_PER_SEC, 'S'),
        (NANOS_PER_MILLI, 'm'),
        (NANOS_PER_MICRO, 'u'),
        (1, 'n'),
        (NANOS_PER_MINUTE, 'M'),
        (NANOS_PER_HOUR, 'H'),
    ] {
        if nanos % per_unit == 0 {
            let value = nanos / per_unit;
            if value <= MAX_DIGITS_VALUE {
                return format!("{value}{suffix}");
            }
        }
    }
    for (per_unit, suffix) in [
        (NANOS_PER_MICRO, 'u'),
        (NANOS_PER_MILLI, 'm'),
        (NANOS_PER_SEC, 'S'),
        (NANOS_PER_MINUTE, 'M'),
        (NANOS_PER_HOUR, 'H'),
    ] {
        let value = nanos.div_ceil(per_unit);
        if value <= MAX_DIGITS_VALUE {
            return format!("{value}{suffix}");
        }
    }
    format!("{MAX_DIGITS_VALUE}H")
}

/// Parse a `grpc-timeout` header value, or `None` if it is not one.
///
/// This runs on peer-controlled input, so everything malformed — an empty
/// string, a missing or unknown unit, a negative or non-numeric value, or a
/// number that would overflow — is `None` rather than a panic or a wrong
/// duration. `None` means "no deadline", which is the safe reading of a header
/// we cannot understand.
///
/// Values longer than the specified eight digits are accepted when they fit a
/// `u64`, because rejecting a deadline a peer clearly meant to set would be
/// less safe than honouring it.
///
/// ```
/// use pbrs_grpc::timeout::parse_timeout;
/// use std::time::Duration;
///
/// assert_eq!(parse_timeout("1H"), Some(Duration::from_secs(3600)));
/// assert_eq!(parse_timeout("500u"), Some(Duration::from_micros(500)));
///
/// assert_eq!(parse_timeout(""), None);
/// assert_eq!(parse_timeout("100"), None);      // no unit
/// assert_eq!(parse_timeout("-1S"), None);      // not a count
/// assert_eq!(parse_timeout("10Y"), None);      // unknown unit
/// ```
#[must_use]
pub fn parse_timeout(s: &str) -> Option<Duration> {
    let (&unit, digits) = s.as_bytes().split_last()?;
    if digits.is_empty() {
        return None;
    }
    let n = digits.iter().try_fold(0_u64, |n, digit| {
        let digit = digit.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        n.checked_mul(10)?.checked_add(u64::from(digit))
    })?;
    match unit {
        b'n' => Some(Duration::from_nanos(n)),
        b'u' => Some(Duration::from_micros(n)),
        b'm' => Some(Duration::from_millis(n)),
        b'S' => Some(Duration::from_secs(n)),
        b'M' => Some(Duration::from_secs(n.checked_mul(60)?)),
        b'H' => Some(Duration::from_secs(n.checked_mul(3_600)?)),
        _ => None,
    }
}

/// Turn an optional duration timeout into an absolute deadline Instant from `now`.
///
/// A timeout beyond the platform's monotonic clock range returns `None`, as
/// for an absent timeout, rather than panicking. Its original relative value
/// can still be sent in the `grpc-timeout` header.
#[must_use]
pub fn deadline_from(timeout: Option<Duration>) -> Option<tokio::time::Instant> {
    timeout.and_then(|d| TokioRuntime::now().checked_add(d))
}

/// Compute the remaining duration until an absolute `tokio::time::Instant` deadline.
///
/// Returns:
/// - `Ok(None)` if no deadline was set (`deadline` is `None`).
/// - `Ok(Some(remaining))` if the deadline has not expired (`remaining > Duration::ZERO`).
/// - `Err(Status::deadline_exceeded())` if the deadline has expired (`remaining == Duration::ZERO`).
#[inline]
pub fn remaining_timeout(
    deadline: Option<tokio::time::Instant>,
) -> Result<Option<Duration>, Status> {
    match deadline {
        // The steady path has no deadline. Keep the clock read out of line
        // so each of these checks is a single branch.
        None => Ok(None),
        Some(at) => remaining_until(at),
    }
}

#[cold]
#[inline(never)]
fn remaining_until(at: tokio::time::Instant) -> Result<Option<Duration>, Status> {
    let remaining = at.saturating_duration_since(TokioRuntime::now());
    if remaining.is_zero() {
        Err(Status::deadline_exceeded())
    } else {
        Ok(Some(remaining))
    }
}

#[cfg(test)]
mod tests {
    use super::{deadline_from, encode_timeout, parse_timeout, remaining_timeout};
    use std::time::Duration;

    #[test]
    fn every_unit_round_trips() {
        for d in [
            Duration::from_nanos(7),
            Duration::from_micros(50),
            Duration::from_millis(250),
            Duration::from_secs(5),
            Duration::from_secs(3_600),
        ] {
            let encoded = encode_timeout(d);
            assert_eq!(parse_timeout(&encoded), Some(d), "{encoded:?}");
        }
    }

    #[test]
    fn preferred_exact_unit_wins() {
        assert_eq!(encode_timeout(Duration::from_secs(5)), "5S");
        assert_eq!(encode_timeout(Duration::from_millis(1_500)), "1500m");
        assert_eq!(encode_timeout(Duration::from_micros(1_500)), "1500u");
        // 1500 ns is not a whole number of microseconds, so it stays in `n`.
        assert_eq!(encode_timeout(Duration::from_nanos(1_500)), "1500n");
        assert_eq!(encode_timeout(Duration::from_nanos(2_000)), "2u");
        assert_eq!(encode_timeout(Duration::ZERO), "0S");
    }

    #[test]
    fn oversized_seconds_use_minutes_without_shortening_the_timeout() {
        // More seconds than eight digits hold.
        let huge = Duration::from_secs(100_000_000);
        assert_eq!(encode_timeout(huge), "1666667M");
        // Still parseable by any conformant peer.
        assert!(parse_timeout(&encode_timeout(huge)).is_some());
    }

    #[test]
    fn inexact_timeouts_round_up_at_every_unit_boundary() {
        for (nanos, expected) in [
            (100_000_001, "100001u"),
            (99_999_999_001, "100000m"),
            (99_999_999_000_001, "100000S"),
            (99_999_999_000_000_001, "1666667M"),
            (5_999_999_940_000_000_001, "1666667H"),
        ] {
            let duration = Duration::from_nanos(nanos);
            let encoded = encode_timeout(duration);
            assert_eq!(encoded, expected);
            assert!(parse_timeout(&encoded).unwrap() >= duration);
        }
        let duration = Duration::from_secs(100_000) + Duration::from_nanos(1);
        assert_eq!(encode_timeout(duration), "100001S");
    }

    #[test]
    fn timeout_encoding_only_saturates_above_the_wire_maximum() {
        let maximum = Duration::from_secs(99_999_999 * 3_600);
        assert_eq!(encode_timeout(maximum), "99999999H");
        assert_eq!(
            encode_timeout(maximum + Duration::from_nanos(1)),
            "99999999H"
        );
        assert_eq!(encode_timeout(Duration::MAX), "99999999H");
        for unit in [1, 1_000, 1_000_000, 1_000_000_000, 60_000_000_000] {
            for offset in [0, 1, unit - 1] {
                let duration = Duration::from_nanos(99_999_999 * unit + offset);
                let encoded = encode_timeout(duration);
                assert!(encoded.len() <= 9, "{encoded}");
                assert!(parse_timeout(&encoded).unwrap() >= duration, "{encoded}");
            }
        }
    }

    #[test]
    fn nanosecond_precision_survives_when_no_coarser_unit_is_exact() {
        let d = Duration::from_nanos(99_999_999);
        assert_eq!(encode_timeout(d), "99999999n");
        assert_eq!(parse_timeout("99999999n"), Some(d));
    }

    #[test]
    fn malformed_values_are_none_not_panics() {
        for bad in [
            "",
            "S",
            "100",
            "-1S",
            "+1S",
            "+0n",
            "1.5S",
            "10Y",
            "abcS",
            " 10S",
            "10 S",
            "99999999999999999999999S",
            "\u{1f600}S",
        ] {
            assert_eq!(parse_timeout(bad), None, "{bad:?} must not parse");
        }
    }

    #[test]
    fn overflowing_minutes_and_hours_are_none() {
        let overflow = (u64::MAX / 60) + 1;
        assert_eq!(parse_timeout(&format!("{overflow}M")), None);
        let overflow = (u64::MAX / 3_600) + 1;
        assert_eq!(parse_timeout(&format!("{overflow}H")), None);
    }

    #[test]
    fn long_digit_runs_are_accepted_when_they_fit() {
        // Nine digits: longer than the spec's field, still a sane deadline.
        assert_eq!(
            parse_timeout("100000000m"),
            Some(Duration::from_millis(100_000_000))
        );
    }

    #[test]
    fn deadline_helpers_track_remaining_budget() {
        assert_eq!(deadline_from(None), None);
        let at = deadline_from(Some(Duration::from_secs(10))).unwrap();
        assert!(remaining_timeout(Some(at)).unwrap().unwrap() > Duration::from_secs(8));

        assert_eq!(remaining_timeout(None).unwrap(), None);

        let past = tokio::time::Instant::now() - Duration::from_millis(50);
        assert!(remaining_timeout(Some(past)).is_err());
    }

    #[test]
    fn unrepresentable_deadlines_are_checked_instead_of_panicking() {
        assert_eq!(
            deadline_from(Some(Duration::MAX)).is_some(),
            tokio::time::Instant::now()
                .checked_add(Duration::MAX)
                .is_some()
        );
    }
}
