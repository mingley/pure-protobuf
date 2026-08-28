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

use std::time::Duration;

/// Largest value the eight-digit field can hold.
const MAX_DIGITS_VALUE: u128 = 99_999_999;

const NANOS_PER_MICRO: u128 = 1_000;
const NANOS_PER_MILLI: u128 = 1_000_000;
const NANOS_PER_SEC: u128 = 1_000_000_000;
const NANOS_PER_HOUR: u128 = 3_600 * NANOS_PER_SEC;

/// Encode a duration as a `grpc-timeout` value.
///
/// Picks the coarsest unit that represents the duration exactly and still fits
/// in eight digits, so a whole number of milliseconds travels as `m` rather
/// than as a long nanosecond count.
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
/// A duration too large for eight digits of hours is truncated to hours, which
/// can shorten it by up to an hour. Deadlines that long are not meaningfully
/// deadlines.
#[must_use]
pub fn encode_timeout(d: Duration) -> String {
    let nanos = d.as_nanos();
    for (per_unit, suffix) in [
        (NANOS_PER_SEC, 'S'),
        (NANOS_PER_MILLI, 'm'),
        (NANOS_PER_MICRO, 'u'),
    ] {
        if nanos % per_unit == 0 {
            let value = nanos / per_unit;
            if value <= MAX_DIGITS_VALUE {
                return format!("{value}{suffix}");
            }
        }
    }
    if nanos <= MAX_DIGITS_VALUE {
        return format!("{nanos}n");
    }
    let hours = (nanos / NANOS_PER_HOUR).min(MAX_DIGITS_VALUE);
    format!("{hours}H")
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
    let split = s.len().checked_sub(1)?;
    let (digits, unit) = s.split_at(split);
    let n: u64 = digits.parse().ok()?;
    match unit {
        "n" => Some(Duration::from_nanos(n)),
        "u" => Some(Duration::from_micros(n)),
        "m" => Some(Duration::from_millis(n)),
        "S" => Some(Duration::from_secs(n)),
        "M" => Some(Duration::from_secs(n.checked_mul(60)?)),
        "H" => Some(Duration::from_secs(n.checked_mul(3_600)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_timeout, parse_timeout};
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
    fn the_coarsest_exact_unit_wins() {
        assert_eq!(encode_timeout(Duration::from_secs(5)), "5S");
        assert_eq!(encode_timeout(Duration::from_millis(1_500)), "1500m");
        assert_eq!(encode_timeout(Duration::from_micros(1_500)), "1500u");
        // 1500 ns is not a whole number of microseconds, so it stays in `n`.
        assert_eq!(encode_timeout(Duration::from_nanos(1_500)), "1500n");
        assert_eq!(encode_timeout(Duration::from_nanos(2_000)), "2u");
        assert_eq!(encode_timeout(Duration::ZERO), "0S");
    }

    #[test]
    fn oversized_durations_fall_back_to_hours() {
        // More seconds than eight digits hold.
        let huge = Duration::from_secs(100_000_000);
        assert_eq!(encode_timeout(huge), "27777H");
        // Still parseable by any conformant peer.
        assert!(parse_timeout(&encode_timeout(huge)).is_some());
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
}
