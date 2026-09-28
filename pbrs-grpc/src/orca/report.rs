//! Per-call ORCA report encode/decode, A114 utilization selection, and OOB helpers.
//!
//! Backends attach the serialized [`OrcaLoadReport`][super::OrcaLoadReport] to
//! the [`TRAILER`] trailing-metadata key (A51 §Per-Request). Clients parse it
//! once per RPC and share the decoded report across LB policies; malformed
//! payloads are ignored, never fatal. [`utilization`] implements the A114
//! metric-name selection with the A58 `application_utilization`-then-`cpu`
//! fallback, and [`qps`]/[`eps`] normalize the rate fields for the WRR weight
//! formula.

use super::OrcaLoadReport;
use std::time::Duration;

/// Trailing-metadata key carrying the serialized per-call ORCA report.
pub const TRAILER: &str = "endpoint-load-metrics-bin";

/// Default OOB minimum report interval (A51: 30s unless configured).
pub const DEFAULT_MIN_REPORT_INTERVAL: Duration = Duration::from_secs(30);

/// Serialize `report` for [`TRAILER`].
pub fn encode_trailer(report: &OrcaLoadReport) -> Result<Vec<u8>, crate::Status> {
    use pbrs::Serialize as _;
    report
        .serialize()
        .map_err(|e| crate::Status::internal(format!("orca serialize: {e}")))
}

/// Parse a [`TRAILER`] payload. `None` on malformed input: per-call reports
/// are best-effort and a corrupt trailer must not fail the RPC.
#[must_use]
pub fn decode_trailer(bytes: &[u8]) -> Option<OrcaLoadReport> {
    use pbrs::Parse as _;
    OrcaLoadReport::parse(bytes).ok()
}

/// First [`TRAILER`] value in `trailers`, decoded. Repeats keep the first,
/// matching [`crate::Metadata::get_bin`].
#[must_use]
pub fn report_from_trailers(trailers: &crate::Metadata) -> Option<OrcaLoadReport> {
    decode_trailer(&trailers.get_bin(TRAILER)?)
}

/// Queries served per second: `rps_fractional` when positive, else the
/// deprecated `rps` for producers that predate the fractional field.
/// Non-positive and non-finite readings mean "no rate info".
#[must_use]
pub fn qps(report: &OrcaLoadReport) -> f64 {
    let fractional = report.rps_fractional();
    if fractional > 0.0 && fractional.is_finite() {
        return fractional;
    }
    #[allow(deprecated, reason = "fallback for pre-fractional producers")]
    let legacy = report.rps();
    let legacy = legacy as f64;
    if legacy > 0.0 && legacy.is_finite() {
        legacy
    } else {
        0.0
    }
}

/// Errors served per second. Non-positive and non-finite readings mean
/// "no error info" rather than a penalty.
#[must_use]
pub fn eps(report: &OrcaLoadReport) -> f64 {
    let eps = report.eps();
    if eps > 0.0 && eps.is_finite() {
        eps
    } else {
        0.0
    }
}

/// Utilization for the WRR weight, or `None` when the report carries no
/// usable utilization signal.
///
/// With `metric_names` configured (A114), resolves each name and takes the
/// max over positive finite hits: `named_metrics.<key>` and
/// `utilization.<key>` look up the maps (the first `.` splits field from
/// key, so `named_metrics.foo.bar` is key `foo.bar`), while bare
/// `application_utilization`, `cpu_utilization`, and `mem_utilization` read
/// the scalar fields. Anything else is not a utilization source and is
/// skipped. Without a configured hit, falls back to A58: a positive finite
/// `application_utilization`, else a positive finite `cpu_utilization`.
#[must_use]
pub fn utilization(report: &OrcaLoadReport, metric_names: &[String]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for name in metric_names {
        if let Some(key) = name.strip_prefix("named_metrics.") {
            if let Some(v) = report.named_metrics().get(key) {
                consider(&mut best, v);
            }
        } else if let Some(key) = name.strip_prefix("utilization.") {
            if let Some(v) = report.utilization().get(key) {
                consider(&mut best, v);
            }
        } else if name == "application_utilization" {
            consider(&mut best, report.application_utilization());
        } else if name == "cpu_utilization" {
            consider(&mut best, report.cpu_utilization());
        } else if name == "mem_utilization" {
            consider(&mut best, report.mem_utilization());
        }
    }
    if best.is_some() {
        return best;
    }
    // No configured hit (or nothing configured): A114 falls back to
    // application_utilization, then cpu_utilization.
    consider(&mut best, report.application_utilization());
    if best.is_some() {
        return best;
    }
    consider(&mut best, report.cpu_utilization());
    best
}

fn consider(best: &mut Option<f64>, value: f64) {
    if value > 0.0 && value.is_finite() {
        *best = Some(best.map_or(value, |b: f64| b.max(value)));
    }
}

/// Restrict `report`'s `request_cost` entries to `names`. An empty `names`
/// keeps everything: per the proto, empty means "all known request costs".
/// Unknown names select nothing.
pub fn filter_request_cost(report: &mut OrcaLoadReport, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let drop: Vec<Vec<u8>> = report
        .request_cost()
        .keys()
        .map(|k| k.as_bytes().to_vec())
        .filter(|k| !names.iter().any(|n| n.as_bytes() == k.as_slice()))
        .collect();
    let mut costs = report.request_cost_mut();
    for key in &drop {
        let Ok(key) = std::str::from_utf8(key) else {
            continue;
        };
        costs.remove(key);
    }
}

/// Clamp a client-requested OOB `report_interval` to the server minimum
/// (A51): requests below `minimum`, zero, or negative arrive as `minimum`.
/// There is no upper bound.
#[must_use]
pub fn clamp_report_interval(requested: Duration, minimum: Duration) -> Duration {
    requested.max(minimum)
}

/// `report_interval` from an [`OrcaLoadReportRequest`][super::OrcaLoadReportRequest],
/// as a [`Duration`]. Missing, zero, and negative components become
/// [`Duration::ZERO`] so the server minimum applies (see
/// [`clamp_report_interval`]).
#[must_use]
pub fn request_interval(request: &super::OrcaLoadReportRequest) -> Duration {
    let seconds = u64::try_from(request.report_interval().seconds().max(0)).unwrap_or(0);
    let nanos = u32::try_from(request.report_interval().nanos().max(0)).unwrap_or(0);
    Duration::new(seconds, nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OrcaLoadReport {
        let mut r = OrcaLoadReport::new();
        r.set_cpu_utilization(0.5);
        r.set_application_utilization(0.8);
        r.set_rps_fractional(100.0);
        r.set_eps(2.0);
        r.named_metrics_mut().insert("queue", 0.9);
        r.named_metrics_mut().insert("foo.bar", 0.7);
        r.utilization_mut().insert("gpu", 0.3);
        r.request_cost_mut().insert("bytes", 3487.0);
        r.request_cost_mut().insert("cpu_ms", 12.0);
        r
    }

    #[test]
    fn trailer_round_trip() {
        let report = sample();
        let bytes = encode_trailer(&report).expect("encode");
        let back = decode_trailer(&bytes).expect("decode");
        assert_eq!(back.cpu_utilization(), 0.5);
        assert_eq!(back.application_utilization(), 0.8);
        assert_eq!(back.rps_fractional(), 100.0);
        assert_eq!(back.named_metrics().get("queue"), Some(0.9));
    }

    #[test]
    fn malformed_trailer_is_none() {
        assert!(decode_trailer(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_none());
    }

    #[test]
    fn qps_prefers_fractional_then_legacy() {
        let mut r = OrcaLoadReport::new();
        assert_eq!(qps(&r), 0.0);
        #[allow(deprecated, reason = "legacy fallback under test")]
        r.set_rps(50);
        assert_eq!(qps(&r), 50.0);
        r.set_rps_fractional(100.5);
        assert_eq!(qps(&r), 100.5);
        r.set_rps_fractional(f64::NAN);
        assert_eq!(qps(&r), 50.0);
    }

    #[test]
    fn utilization_falls_back_app_then_cpu() {
        let mut r = OrcaLoadReport::new();
        assert_eq!(utilization(&r, &[]), None);
        r.set_cpu_utilization(0.5);
        assert_eq!(utilization(&r, &[]), Some(0.5));
        r.set_application_utilization(0.8);
        assert_eq!(utilization(&r, &[]), Some(0.8));
        r.set_application_utilization(-1.0);
        assert_eq!(utilization(&r, &[]), Some(0.5));
    }

    #[test]
    fn utilization_selects_configured_metric_max() {
        let r = sample();
        let names = vec![
            "named_metrics.queue".to_string(),
            "cpu_utilization".to_string(),
        ];
        assert_eq!(utilization(&r, &names), Some(0.9));
        // Dotted keys: first dot splits field from key.
        let names = vec!["named_metrics.foo.bar".to_string()];
        assert_eq!(utilization(&r, &names), Some(0.7));
        // Utilization map participates too.
        let names = vec!["utilization.gpu".to_string()];
        assert_eq!(utilization(&r, &names), Some(0.3));
        // Unknown names resolve nothing, so A114 falls back to app util.
        let names = vec!["request_cost.bytes".to_string(), "nope".to_string()];
        assert_eq!(utilization(&r, &names), Some(0.8));
    }

    #[test]
    fn request_cost_filter_empty_keeps_all() {
        let mut r = sample();
        filter_request_cost(&mut r, &[]);
        assert_eq!(r.request_cost().len(), 2);
        filter_request_cost(&mut r, &["bytes".to_string(), "missing".to_string()]);
        assert_eq!(r.request_cost().len(), 1);
        assert_eq!(r.request_cost().get("bytes"), Some(3487.0));
    }

    #[test]
    fn interval_clamp_applies_minimum() {
        let min = Duration::from_secs(30);
        assert_eq!(clamp_report_interval(Duration::ZERO, min), min);
        assert_eq!(clamp_report_interval(Duration::from_secs(5), min), min);
        assert_eq!(
            clamp_report_interval(Duration::from_secs(60), min),
            Duration::from_secs(60)
        );
    }
}
