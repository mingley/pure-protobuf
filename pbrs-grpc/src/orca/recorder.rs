//! ORCA metric recorders (A51 server API).
//!
//! An [`OrcaRecorder`] is a cloneable handle to shared metric state: an
//! application records utilization, rates, and named metrics into it, and
//! the per-call trailer hook ([`stamp`]) plus the OOB service render
//! snapshots from it. All metrics start unset and stay until cleared;
//! recording the same metric overrides the previous value. A per-request
//! recorder merged over the server recorder wins per metric.

use super::OrcaLoadReport;
use std::collections::HashMap;
use std::sync::Arc;

/// Set-or-unset metric state behind an [`OrcaRecorder`].
#[derive(Clone, Debug, Default)]
struct Recorded {
    cpu_utilization: Option<f64>,
    mem_utilization: Option<f64>,
    application_utilization: Option<f64>,
    rps_fractional: Option<f64>,
    eps: Option<f64>,
    request_cost: HashMap<String, f64>,
    utilization: HashMap<String, f64>,
    named_metrics: HashMap<String, f64>,
}

impl Recorded {
    /// Render this state, with `overlay` winning per metric.
    fn render(&self, overlay: &Recorded) -> OrcaLoadReport {
        let mut report = OrcaLoadReport::new();
        if let Some(v) = overlay.cpu_utilization.or(self.cpu_utilization) {
            report.set_cpu_utilization(v);
        }
        if let Some(v) = overlay.mem_utilization.or(self.mem_utilization) {
            report.set_mem_utilization(v);
        }
        if let Some(v) = overlay
            .application_utilization
            .or(self.application_utilization)
        {
            report.set_application_utilization(v);
        }
        if let Some(v) = overlay.rps_fractional.or(self.rps_fractional) {
            report.set_rps_fractional(v);
        }
        if let Some(v) = overlay.eps.or(self.eps) {
            report.set_eps(v);
        }
        {
            let mut costs = report.request_cost_mut();
            for (k, v) in self.request_cost.iter().chain(overlay.request_cost.iter()) {
                costs.insert(k.as_str(), *v);
            }
        }
        {
            let mut utils = report.utilization_mut();
            for (k, v) in self.utilization.iter().chain(overlay.utilization.iter()) {
                utils.insert(k.as_str(), *v);
            }
        }
        {
            let mut named = report.named_metrics_mut();
            for (k, v) in self
                .named_metrics
                .iter()
                .chain(overlay.named_metrics.iter())
            {
                named.insert(k.as_str(), *v);
            }
        }
        report
    }
}

/// Cloneable handle to ORCA metric state (A51 metrics recorder).
///
/// All methods take `&self` and may be called concurrently; recording the
/// same metric overrides the previous value. Serve snapshots with
/// [`stamp`] (per-call trailers) or the OOB service in [`super::service`].
#[derive(Clone, Debug, Default)]
pub struct OrcaRecorder {
    #[allow(
        clippy::disallowed_types,
        reason = "short Mutex held only across sync record/render; never across await"
    )]
    state: Arc<std::sync::Mutex<Recorded>>,
}

impl OrcaRecorder {
    /// A recorder with every metric unset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn with(&self, f: impl FnOnce(&mut Recorded)) {
        if let Ok(mut state) = self.state.lock() {
            f(&mut state);
        }
    }

    /// Record CPU utilization as a fraction of available CPU.
    pub fn set_cpu_utilization(&self, value: f64) {
        self.with(|s| s.cpu_utilization = Some(value));
    }

    /// Clear the CPU utilization back to unset.
    pub fn clear_cpu_utilization(&self) {
        self.with(|s| s.cpu_utilization = None);
    }

    /// Record memory utilization as a fraction of available memory.
    pub fn set_mem_utilization(&self, value: f64) {
        self.with(|s| s.mem_utilization = Some(value));
    }

    /// Clear the memory utilization back to unset.
    pub fn clear_mem_utilization(&self) {
        self.with(|s| s.mem_utilization = None);
    }

    /// Record application utilization as a fraction of available resources.
    pub fn set_application_utilization(&self, value: f64) {
        self.with(|s| s.application_utilization = Some(value));
    }

    /// Clear the application utilization back to unset.
    pub fn clear_application_utilization(&self) {
        self.with(|s| s.application_utilization = None);
    }

    /// Record the served queries rate.
    pub fn set_rps_fractional(&self, value: f64) {
        self.with(|s| s.rps_fractional = Some(value));
    }

    /// Clear the served queries rate back to unset.
    pub fn clear_rps_fractional(&self) {
        self.with(|s| s.rps_fractional = None);
    }

    /// Record the served errors rate.
    pub fn set_eps(&self, value: f64) {
        self.with(|s| s.eps = Some(value));
    }

    /// Clear the served errors rate back to unset.
    pub fn clear_eps(&self) {
        self.with(|s| s.eps = None);
    }

    /// Record a request-cost entry. Re-recording the name overrides it.
    pub fn set_request_cost(&self, name: impl Into<String>, value: f64) {
        let name = name.into();
        self.with(|s| {
            s.request_cost.insert(name, value);
        });
    }

    /// Delete a request-cost entry.
    pub fn delete_request_cost(&self, name: &str) {
        self.with(|s| {
            s.request_cost.remove(name);
        });
    }

    /// Record a resource-utilization entry. Re-recording overrides it.
    pub fn set_utilization(&self, name: impl Into<String>, value: f64) {
        let name = name.into();
        self.with(|s| {
            s.utilization.insert(name, value);
        });
    }

    /// Delete a resource-utilization entry.
    pub fn delete_utilization(&self, name: &str) {
        self.with(|s| {
            s.utilization.remove(name);
        });
    }

    /// Record an opaque named metric. Re-recording the name overrides it.
    pub fn set_named_metric(&self, name: impl Into<String>, value: f64) {
        let name = name.into();
        self.with(|s| {
            s.named_metrics.insert(name, value);
        });
    }

    /// Delete an opaque named metric.
    pub fn delete_named_metric(&self, name: &str) {
        self.with(|s| {
            s.named_metrics.remove(name);
        });
    }

    /// Clear every metric back to unset.
    pub fn clear(&self) {
        self.with(|s| *s = Recorded::default());
    }

    /// Snapshot this recorder's metrics as a report.
    #[must_use]
    pub fn snapshot(&self) -> OrcaLoadReport {
        let empty = Recorded::default();
        self.state
            .lock()
            .map(|state| state.render(&empty))
            .unwrap_or_default()
    }

    /// Snapshot `server` merged with `request`: per-request recordings win
    /// per metric (A51 precedence).
    #[must_use]
    pub fn merged(server: &OrcaRecorder, request: &OrcaRecorder) -> OrcaLoadReport {
        let (server, request) = (server.state.lock(), request.state.lock());
        match (server, request) {
            (Ok(server), Ok(request)) => server.render(&request),
            _ => OrcaLoadReport::new(),
        }
    }
}

/// A server-recorder trailer hook for every reply shape: register with
/// [`Router::on_response`][crate::Router::on_response] (or
/// [`Server::on_response`][crate::Server::on_response]) and each response
/// carries the recorder snapshot in [`TRAILER`][super::TRAILER]. Handlers
/// needing per-request metrics stamp explicitly with [`stamp`].
#[derive(Clone, Debug)]
pub struct OrcaResponseHook {
    server: OrcaRecorder,
}

impl OrcaResponseHook {
    /// Hook stamping `server` snapshots onto responses.
    #[must_use]
    pub fn new(server: OrcaRecorder) -> Self {
        Self { server }
    }
}

impl crate::ResponseInterceptor for OrcaResponseHook {
    fn intercept(&self, parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
        stamp(parts, Some(&self.server), None)
    }
}

/// Stamp the merged recorder snapshot onto response trailers: `request`
/// recordings win per metric over `server` (A51 precedence); either side
/// may be absent. Overwrites any previous [`TRAILER`][super::TRAILER] value
/// so hooks compose last-wins.
pub fn stamp(
    parts: &mut crate::ResponseParts,
    server: Option<&OrcaRecorder>,
    request: Option<&OrcaRecorder>,
) -> Result<(), crate::Status> {
    let report = match (server, request) {
        (Some(server), Some(request)) => OrcaRecorder::merged(server, request),
        (Some(server), None) => server.snapshot(),
        (None, Some(request)) => request.snapshot(),
        (None, None) => OrcaLoadReport::new(),
    };
    let bytes = super::encode_trailer(&report)?;
    parts
        .trailers_mut()
        .set_bin(super::TRAILER, bytes)
        .map_err(|e| crate::Status::internal(format!("orca trailer: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_set_metrics() {
        let r = OrcaRecorder::new();
        r.set_cpu_utilization(0.5);
        r.set_application_utilization(0.8);
        r.set_rps_fractional(100.0);
        r.set_eps(2.0);
        r.set_named_metric("queue", 0.9);
        r.set_utilization("gpu", 0.3);
        r.set_request_cost("bytes", 42.0);
        let snap = r.snapshot();
        assert_eq!(snap.cpu_utilization(), 0.5);
        assert_eq!(snap.application_utilization(), 0.8);
        assert_eq!(snap.rps_fractional(), 100.0);
        assert_eq!(snap.eps(), 2.0);
        assert_eq!(snap.named_metrics().get("queue"), Some(0.9));
        assert_eq!(snap.utilization().get("gpu"), Some(0.3));
        assert_eq!(snap.request_cost().get("bytes"), Some(42.0));
    }

    #[test]
    fn clear_and_delete_unset() {
        let r = OrcaRecorder::new();
        r.set_cpu_utilization(0.5);
        r.set_named_metric("queue", 0.9);
        r.clear_cpu_utilization();
        r.delete_named_metric("queue");
        let snap = r.snapshot();
        assert_eq!(snap.cpu_utilization(), 0.0);
        assert_eq!(snap.named_metrics().len(), 0);
        r.set_mem_utilization(0.1);
        r.clear();
        assert_eq!(r.snapshot().mem_utilization(), 0.0);
    }

    #[test]
    fn request_wins_per_metric() {
        let server = OrcaRecorder::new();
        server.set_cpu_utilization(0.5);
        server.set_named_metric("queue", 0.9);
        server.set_named_metric("server_only", 0.1);
        let request = OrcaRecorder::new();
        request.set_cpu_utilization(0.7);
        request.set_named_metric("queue", 0.2);
        let merged = OrcaRecorder::merged(&server, &request);
        assert_eq!(merged.cpu_utilization(), 0.7);
        assert_eq!(merged.named_metrics().get("queue"), Some(0.2));
        assert_eq!(merged.named_metrics().get("server_only"), Some(0.1));
    }

    #[test]
    fn stamp_writes_decodable_trailer() {
        let server = OrcaRecorder::new();
        server.set_cpu_utilization(0.5);
        let mut parts = crate::Response::new(()).into_message_and_parts().1;
        stamp(&mut parts, Some(&server), None).expect("stamp");
        let raw = parts
            .trailers()
            .get_bin(super::super::TRAILER)
            .expect("trailer");
        let back = super::super::decode_trailer(&raw).expect("decode");
        assert_eq!(back.cpu_utilization(), 0.5);
    }
}
