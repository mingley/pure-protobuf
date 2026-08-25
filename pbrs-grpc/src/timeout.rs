//! `grpc-timeout` encode / parse (`{digits}{n|u|m|S|M|H}`).

use std::time::Duration;

/// Encode a duration as a `grpc-timeout` value (at most 8 digits).
#[must_use]
pub fn encode_timeout(d: Duration) -> String {
    let nanos = d.as_nanos();
    const MAX: u128 = 99_999_999;
    if nanos % 1_000_000_000 == 0 {
        let s = nanos / 1_000_000_000;
        if s <= MAX {
            return format!("{s}S");
        }
    }
    if nanos % 1_000_000 == 0 {
        let millis = nanos / 1_000_000;
        if millis <= MAX {
            return format!("{millis}m");
        }
    }
    if nanos % 1_000 == 0 {
        let micros = nanos / 1_000;
        if micros <= MAX {
            return format!("{micros}u");
        }
    }
    if nanos <= MAX {
        return format!("{nanos}n");
    }
    let hours = (nanos / 3_600_000_000_000).min(MAX);
    format!("{hours}H")
}

/// Parse a `grpc-timeout` header value.
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
        "H" => Some(Duration::from_secs(n.checked_mul(3600)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_timeout, parse_timeout};
    use std::time::Duration;

    #[test]
    fn millis_roundtrip() {
        let s = encode_timeout(Duration::from_millis(50));
        assert_eq!(s, "50m");
        assert_eq!(parse_timeout(&s), Some(Duration::from_millis(50)));
    }
}
