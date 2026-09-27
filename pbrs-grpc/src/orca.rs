//! ORCA load reports (gRFC A51): per-call trailers and the OOB service.
//!
//! [`OrcaLoadReport`] is the `xds.data.orca.v3` wire message, generated from
//! the vendored proto. Backends attach it to the `endpoint-load-metrics-bin`
//! trailer ([`report`]); the `OpenRcaService/StreamCoreMetrics` stubs below
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

mod report;

pub use report::{
    DEFAULT_MIN_REPORT_INTERVAL, TRAILER, clamp_report_interval, decode_trailer, encode_trailer,
    eps, filter_request_cost, qps, report_from_trailers, request_interval, utilization,
};
