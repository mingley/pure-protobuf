//! Parsers for the exact command formats used by the dev-loop harness.

fn count(text: &str) -> Option<f64> {
    let value = text.trim().parse::<f64>().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

/// `perf stat -x, -e instructions`: count, unit (usually empty), event, ...
/// No interval, CPU aggregation, or repeat prefixes are requested by the caller.
pub(crate) fn parse_perf_instructions(stderr: &str) -> Option<f64> {
    let mut result = None;
    for line in stderr.lines() {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        let Some(event) = fields.get(2) else { continue };
        if event.split(':').next() != Some("instructions") {
            continue;
        }
        // Multiple rows need explicit aggregation semantics, not a first-row win.
        if result.is_some() {
            return None;
        }
        result = Some(count(fields.first()?)?);
    }
    result
}

pub(crate) fn parse_callgrind_instructions(stderr: &str) -> Option<f64> {
    stderr.lines().find_map(|line| {
        let (_, rest) = line.split_once("I   refs:")?;
        count(&rest.split_whitespace().next()?.replace(',', ""))
    })
}

/// Default `strace -c -f` summary: %time, seconds, usecs/call, calls,
/// optional errors, syscall. The total row ends with `total`.
/// Futex calls include wakes and unsuccessful waits; they are not lock acquisitions.
pub(crate) fn parse_strace_summary(stderr: &str) -> Option<(f64, f64)> {
    let mut total = None;
    let mut futex = 0.0;
    for line in stderr.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(name) = fields.last() else { continue };
        if !matches!(*name, "total" | "futex" | "futex_waitv") {
            continue;
        }
        if !matches!(fields.len(), 5 | 6) {
            return None;
        }
        let calls = fields.get(3)?.parse::<u64>().ok()? as f64;
        if *name == "total" {
            if total.replace(calls).is_some() {
                return None;
            }
        } else {
            futex += calls;
        }
    }
    let total = total?;
    (futex <= total).then_some((total, futex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_reads_event_after_empty_unit() {
        assert_eq!(
            parse_perf_instructions("__CHILD__ {}\n123456,,instructions,100,100.00,,\n"),
            Some(123456.0)
        );
        assert_eq!(
            parse_perf_instructions("42,,instructions:u,100,100.00,,"),
            Some(42.0)
        );
        assert_eq!(
            parse_perf_instructions("0,,instructions,100,100.00,,"),
            Some(0.0)
        );
    }

    #[test]
    fn perf_rejects_unavailable_nonfinite_ambiguous_or_wrong_events() {
        for value in ["<not supported>", "<not counted>", "NaN", "inf", "-1"] {
            assert_eq!(
                parse_perf_instructions(&format!("{value},,instructions,100,100.00,,")),
                None
            );
        }
        assert_eq!(parse_perf_instructions("123,,cycles,100,100.00,,"), None);
        assert_eq!(
            parse_perf_instructions("1,,instructions,\n2,,instructions,"),
            None
        );
    }

    #[test]
    fn callgrind_reads_summary_and_rejects_invalid_counts() {
        assert_eq!(
            parse_callgrind_instructions("==123== I   refs:      1,234,567"),
            Some(1234567.0)
        );
        assert_eq!(parse_callgrind_instructions("==123== I   refs: NaN"), None);
    }

    #[test]
    fn strace_total_is_last_column_with_optional_error_count() {
        assert_eq!(
            parse_strace_summary(
                "25.00 0.001 3 7 2 futex\n25.00 0.001 3 2 futex_waitv\n100.00 0.004 4 20 2 total"
            ),
            Some((20.0, 9.0))
        );
        assert_eq!(
            parse_strace_summary("100.00 0.004 4 20 total"),
            Some((20.0, 0.0))
        );
    }

    #[test]
    fn strace_missing_or_malformed_summary_is_not_a_measured_zero() {
        assert_eq!(parse_strace_summary("strace: permission denied"), None);
        assert_eq!(parse_strace_summary("1 0 0 2 futex"), None);
        assert_eq!(parse_strace_summary("100 0 0 NaN total"), None);
        assert_eq!(parse_strace_summary("1 0 0 8 futex\n100 0 0 7 total"), None);
        assert_eq!(parse_strace_summary("1 0 0 7 total\n1 0 0 7 total"), None);
    }
}
