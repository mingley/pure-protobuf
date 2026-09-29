//! ORCA load reports (gRFC A51): per-call trailers and the OOB service.
//!
//! [`OrcaLoadReport`] is the `xds.data.orca.v3` wire message, generated from
//! the vendored proto. Backends attach it to the `endpoint-load-metrics-bin`
//! trailer (`report`); the `OpenRcaService/StreamCoreMetrics` stubs below
//! serve and consume out-of-band reports. `weighted_round_robin` (A58) reads
//! both; see [`crate::lb`] once the policy lands.

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/orca.rs"));

/// Generated `xds.data.orca.v3` messages, addressed by the stubs above via
/// `extern_path`. Re-exported at the module root for callers.
pub mod proto {
    #![allow(
        missing_docs,
        deprecated,
        reason = "messages come from the code generator; its JSON/text mirrors read the deprecated rps field"
    )]
    include!(concat!(env!("OUT_DIR"), "/orca_load_report.rs"));
}

pub use proto::OrcaLoadReport;

mod recorder;
mod report;
mod service;

pub use recorder::{OrcaRecorder, OrcaResponseHook, stamp};
pub use report::{
    DEFAULT_MIN_REPORT_INTERVAL, TRAILER, clamp_report_interval, decode_trailer, encode_trailer,
    eps, filter_request_cost, qps, report_from_trailers, request_interval, utilization,
};
pub use service::{OrcaService, service};

/// Maximum entries copied from one peer-supplied per-query map
/// ([`per_query_report`]).
///
/// With [`MAX_PER_QUERY_LABEL_LEN`]-byte labels the worst-case encoded
/// report stays under 5 KiB, comfortably inside the header-list budgets
/// peers enforce by default, while the official `orca_per_rpc` vector (one
/// entry per map) passes with wide headroom.
pub const MAX_PER_QUERY_ENTRIES: usize = 32;

/// Maximum bytes of one peer-supplied per-query metric label
/// ([`per_query_report`]).
pub const MAX_PER_QUERY_LABEL_LEN: usize = 64;

/// Build the per-call report for a peer-supplied interop `TestOrcaReport`
/// (the `orca_per_rpc` procedure's `orca_per_query_report` field).
///
/// Copies exactly the four load fields the procedure defines —
/// `cpu_utilization`, `memory_utilization` (as `mem_utilization`),
/// `request_cost`, and `utilization` — and nothing else: rates, named
/// metrics, and application utilization stay unset so the report equals
/// what the pinned peers compare against, and no application payload bytes
/// are ever read. Over-cap maps fail `RESOURCE_EXHAUSTED`; empty,
/// non-UTF-8, or non-finite metrics fail `INVALID_ARGUMENT`, each naming
/// the offending field.
pub fn per_query_report(
    data: &crate::testing::TestOrcaReport,
) -> Result<OrcaLoadReport, crate::Status> {
    check_per_query_len("request_cost", data.request_cost().len())?;
    check_per_query_len("utilization", data.utilization().len())?;
    let cpu = data.cpu_utilization();
    let mem = data.memory_utilization();
    check_per_query_scalar("cpu_utilization", cpu)?;
    check_per_query_scalar("memory_utilization", mem)?;
    let mut report = OrcaLoadReport::new();
    report.set_cpu_utilization(cpu);
    report.set_mem_utilization(mem);
    {
        let mut costs = report.request_cost_mut();
        copy_per_query_map("request_cost", data.request_cost(), |name, value| {
            costs.insert(name, value);
        })?;
    }
    {
        let mut utils = report.utilization_mut();
        copy_per_query_map("utilization", data.utilization(), |name, value| {
            utils.insert(name, value);
        })?;
    }
    Ok(report)
}

fn check_per_query_len(map: &str, len: usize) -> Result<(), crate::Status> {
    if len > MAX_PER_QUERY_ENTRIES {
        return Err(crate::Status::resource_exhausted(format!(
            "orca_per_query_report.{map} has {len} entries, max {MAX_PER_QUERY_ENTRIES}"
        )));
    }
    Ok(())
}

fn check_per_query_scalar(field: &str, value: f64) -> Result<(), crate::Status> {
    if !value.is_finite() {
        return Err(crate::Status::invalid_argument(format!(
            "orca_per_query_report.{field} is not finite"
        )));
    }
    Ok(())
}

fn check_per_query_label<'a>(map: &str, label: &'a [u8]) -> Result<&'a str, crate::Status> {
    let label = std::str::from_utf8(label).map_err(|_| {
        crate::Status::invalid_argument(format!(
            "orca_per_query_report.{map} label is not valid UTF-8"
        ))
    })?;
    if label.is_empty() {
        return Err(crate::Status::invalid_argument(format!(
            "orca_per_query_report.{map} label must not be empty"
        )));
    }
    if label.len() > MAX_PER_QUERY_LABEL_LEN {
        return Err(crate::Status::resource_exhausted(format!(
            "orca_per_query_report.{map} label of {} bytes exceeds the {MAX_PER_QUERY_LABEL_LEN}-byte cap",
            label.len()
        )));
    }
    Ok(label)
}

fn check_per_query_value(map: &str, label: &str, value: f64) -> Result<(), crate::Status> {
    if !value.is_finite() {
        return Err(crate::Status::invalid_argument(format!(
            "orca_per_query_report.{map}[{label:?}] is not finite"
        )));
    }
    Ok(())
}

fn copy_per_query_map(
    map: &str,
    src: pbrs::MapView<pbrs::rt::LazyStr, f64>,
    mut insert: impl FnMut(&str, f64),
) -> Result<(), crate::Status> {
    for (key, value) in src.iter() {
        let name = check_per_query_label(map, key.as_bytes())?;
        check_per_query_value(map, name, value)?;
        insert(name, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Code;
    use crate::testing::TestOrcaReport;

    /// The official `orca_per_rpc` vector (pinned grpc-go
    /// `DoORCAPerRPCTest`).
    fn official_vector() -> TestOrcaReport {
        let mut data = TestOrcaReport::new();
        data.set_cpu_utilization(0.8210);
        data.set_memory_utilization(0.5847);
        data.request_cost_mut().insert("cost", 3456.32);
        data.utilization_mut().insert("util", 0.30499);
        data
    }

    #[test]
    fn per_query_copies_exactly_the_four_load_fields() {
        let report = per_query_report(&official_vector()).expect("official vector");
        assert_eq!(report.cpu_utilization(), 0.8210);
        assert_eq!(report.mem_utilization(), 0.5847);
        assert_eq!(report.request_cost().get("cost"), Some(3456.32));
        assert_eq!(report.utilization().get("util"), Some(0.30499));
        // Everything else stays unset: the pinned peers compare with
        // `proto.Equal` against a report carrying only these fields.
        assert_eq!(report.rps_fractional(), 0.0);
        assert_eq!(report.eps(), 0.0);
        assert_eq!(report.application_utilization(), 0.0);
        assert_eq!(report.named_metrics().len(), 0);
        let bytes = encode_trailer(&report).expect("encode");
        let back = decode_trailer(&bytes).expect("decode");
        assert_eq!(back.cpu_utilization(), 0.8210);
        assert_eq!(back.request_cost().get("cost"), Some(3456.32));
    }

    #[test]
    fn per_query_rejects_over_cap_maps() {
        for map in ["request_cost", "utilization"] {
            let mut data = TestOrcaReport::new();
            for i in 0..=MAX_PER_QUERY_ENTRIES {
                let name = format!("m{i}");
                if map == "request_cost" {
                    data.request_cost_mut().insert(name.as_str(), 1.0);
                } else {
                    data.utilization_mut().insert(name.as_str(), 1.0);
                }
            }
            let err = per_query_report(&data).expect_err("over-cap map");
            assert_eq!(err.code(), Code::ResourceExhausted);
            assert!(err.message().contains(map), "message: {}", err.message());
        }
        // Exactly at the cap passes.
        let mut data = TestOrcaReport::new();
        for i in 0..MAX_PER_QUERY_ENTRIES {
            data.request_cost_mut()
                .insert(format!("m{i}").as_str(), 1.0);
        }
        per_query_report(&data).expect("at-cap map");
    }

    #[test]
    fn per_query_rejects_bad_labels_and_values() {
        // Empty label.
        let mut data = TestOrcaReport::new();
        data.request_cost_mut().insert("", 1.0);
        let err = per_query_report(&data).expect_err("empty label");
        assert_eq!(err.code(), Code::InvalidArgument);
        // Oversize label: diagnosed by length, without echoing the bytes.
        let mut data = TestOrcaReport::new();
        let big = "l".repeat(MAX_PER_QUERY_LABEL_LEN + 1);
        data.utilization_mut().insert(big.as_str(), 1.0);
        let err = per_query_report(&data).expect_err("oversize label");
        assert_eq!(err.code(), Code::ResourceExhausted);
        assert!(!err.message().contains(&big), "message: {}", err.message());
        // Non-UTF-8 label (unreachable over our wire — the decoder
        // rejects it first — but reachable from unchecked constructors).
        let mut data = TestOrcaReport::new();
        data.request_cost_mut()
            .insert(pbrs::ProtoString::from_bytes(b"\xff\xfe"), 1.0);
        let err = per_query_report(&data).expect_err("non-UTF8 label");
        assert_eq!(err.code(), Code::InvalidArgument);
        // Non-finite values and scalars.
        let mut nan_cost = TestOrcaReport::new();
        nan_cost.request_cost_mut().insert("cost", f64::NAN);
        let mut inf_util = TestOrcaReport::new();
        inf_util.utilization_mut().insert("util", f64::INFINITY);
        let mut nan_cpu = TestOrcaReport::new();
        nan_cpu.set_cpu_utilization(f64::NAN);
        let mut neg_inf_mem = TestOrcaReport::new();
        neg_inf_mem.set_memory_utilization(f64::NEG_INFINITY);
        for data in [nan_cost, inf_util, nan_cpu, neg_inf_mem] {
            let err = per_query_report(&data).expect_err("non-finite metric");
            assert_eq!(err.code(), Code::InvalidArgument);
        }
    }
}
