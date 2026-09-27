//! OOB load-report service (A51 §Out of Band).
//!
//! [`OrcaService`] is the builtin `xds.service.orca.v3.OpenRcaService`
//! implementation: each `StreamCoreMetrics` call gets an immediate
//! state-of-the-world snapshot and then one per clamped report interval
//! until the client disconnects. Snapshots render from the service
//! recorder, restricted to the requested `request_cost_names` (empty
//! means all known costs).

use super::OrcaLoadReportRequest;
use super::recorder::OrcaRecorder;
use crate::Request;
use crate::Response;
use crate::Status;
use crate::Streaming;
use crate::orca::OrcaLoadReport;
use std::time::Duration;

/// Builtin OOB service over an [`OrcaRecorder`].
#[derive(Clone, Debug)]
pub struct OrcaService {
    recorder: OrcaRecorder,
    min_interval: Duration,
}

impl OrcaService {
    /// Serve `recorder` with the default 30s minimum report interval.
    #[must_use]
    pub fn new(recorder: OrcaRecorder) -> Self {
        Self {
            recorder,
            min_interval: super::DEFAULT_MIN_REPORT_INTERVAL,
        }
    }

    /// Serve `recorder` with a custom minimum report interval floor.
    #[must_use]
    pub fn with_min_interval(recorder: OrcaRecorder, min_interval: Duration) -> Self {
        Self {
            recorder,
            min_interval,
        }
    }

    fn snapshot(&self, cost_names: &[String]) -> OrcaLoadReport {
        let mut report = self.recorder.snapshot();
        super::filter_request_cost(&mut report, cost_names);
        report
    }
}

impl super::OpenRcaService for OrcaService {
    async fn stream_core_metrics(
        &self,
        request: Request<OrcaLoadReportRequest>,
    ) -> Result<Response<Streaming<OrcaLoadReport>>, Status> {
        let period = super::clamp_report_interval(
            super::request_interval(request.get_ref()),
            self.min_interval,
        );
        let cost_names: Vec<String> = request
            .get_ref()
            .request_cost_names()
            .iter()
            .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
            .collect();
        let cancelled = request.cancelled();
        let service = self.clone();
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            // Immediate state-of-the-world snapshot, then one per period.
            if tx.send(service.snapshot(&cost_names)).await.is_err() {
                return;
            }
            let mut ticker = tokio::time::interval(period);
            // The first tick fires immediately; the snapshot above already
            // covered it, so discard one tick to start the period clock.
            ticker.tick().await;
            tokio::pin!(cancelled);
            loop {
                tokio::select! {
                    biased;
                    () = cancelled.as_mut() => break,
                    () = tx.closed() => break,
                    _ = ticker.tick() => {
                        if tx.send(service.snapshot(&cost_names)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

/// The OOB service and the recorder that drives it, with the default 30s
/// minimum report interval.
///
/// ```no_run
/// # async fn example() -> Result<(), pbrs_grpc::Status> {
/// let (orca, recorder) = pbrs_grpc::orca::service();
/// recorder.set_cpu_utilization(0.5);
/// pbrs_grpc::Router::new()
///     .add_service(orca)
///     .serve("127.0.0.1:50051".parse().expect("addr"))
///     .await?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn service() -> (super::OpenRcaServiceServer<OrcaService>, OrcaRecorder) {
    let recorder = OrcaRecorder::new();
    let service = OrcaService::new(recorder.clone());
    (super::OpenRcaServiceServer::new(service), recorder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orca::OpenRcaService as _;

    fn report_request(interval: Duration, costs: &[&str]) -> OrcaLoadReportRequest {
        let mut req = OrcaLoadReportRequest::new();
        req.report_interval_mut()
            .set_seconds(i64::try_from(interval.as_secs()).unwrap_or(i64::MAX));
        req.report_interval_mut()
            .set_nanos(i32::try_from(interval.subsec_nanos()).unwrap_or(i32::MAX));
        let mut names = req.request_cost_names_mut();
        for cost in costs {
            names.push(*cost);
        }
        req
    }

    #[tokio::test]
    async fn streams_immediate_snapshot_then_periodic() {
        let recorder = OrcaRecorder::new();
        recorder.set_cpu_utilization(0.5);
        recorder.set_request_cost("bytes", 42.0);
        recorder.set_request_cost("other", 7.0);
        // Tiny floor so the test observes two ticks quickly.
        let service = OrcaService::with_min_interval(recorder, Duration::from_millis(20));
        let request = Request::new(report_request(Duration::from_millis(20), &["bytes"]));
        let response = service
            .stream_core_metrics(request)
            .await
            .expect("stream starts");
        let mut stream = response.into_inner();
        let first = stream.message().await.expect("msg").expect("some");
        assert_eq!(first.cpu_utilization(), 0.5);
        // Only the requested cost ships.
        assert_eq!(first.request_cost().get("bytes"), Some(42.0));
        assert_eq!(first.request_cost().get("other"), None);
        let second = stream.message().await.expect("msg").expect("some");
        assert_eq!(second.cpu_utilization(), 0.5);
    }

    #[tokio::test]
    async fn zero_interval_means_minimum() {
        let recorder = OrcaRecorder::new();
        let service = OrcaService::with_min_interval(recorder, Duration::from_secs(30));
        let request = Request::new(report_request(Duration::ZERO, &[]));
        // Would hang for 30s per tick if the clamp failed; the immediate
        // snapshot arrives regardless.
        let response = service
            .stream_core_metrics(request)
            .await
            .expect("stream starts");
        let mut stream = response.into_inner();
        tokio::time::timeout(Duration::from_secs(5), stream.message())
            .await
            .expect("immediate snapshot")
            .expect("msg")
            .expect("some");
    }
}
