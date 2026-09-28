//! Service-config glue: method lookup, throttling, and per-call wire settings.

use super::pool::HealthDirective;
use crate::config::Wire;
use crate::service_config::MethodConfig;

impl super::Channel {
    /// The method entry covering `path`, if a document is attached and covers it.
    pub(crate) fn method_config_for(&self, path: &str) -> Option<MethodConfig> {
        let state = self.service_config.get()?;
        let (service, method) = crate::telemetry::split_path(path);
        state.config.method_config(service, method).cloned()
    }

    /// Client-side health checking for LB subchannels (A17): the
    /// watched service name when the master switch is on and the
    /// attached document opts in with `healthCheckConfig`. Resolved
    /// per acquire so live adoption and per-clone documents apply;
    /// subchannels snapshot it at dial.
    pub(crate) fn health_directive(&self) -> Option<HealthDirective> {
        if !self.config.health_checking_enabled() {
            return None;
        }
        let state = self.service_config.get()?;
        if !state.config.health_check_config() {
            return None;
        }
        Some(HealthDirective {
            service: state
                .config
                .health_service_name()
                .unwrap_or_default()
                .to_owned(),
        })
    }

    /// Request hash for ring_hash picks (A76): the configured header's
    /// value hashed, a random hash when the header is configured but
    /// absent, or `None` when this channel runs another policy or the
    /// policy has no hash source (picks fail then). Resolved per
    /// acquire from the call's metadata.
    pub(crate) fn ring_request_hash(&self, md: &crate::metadata::Metadata) -> Option<u64> {
        let super::pool::Endpoint::Resolved { lb: Some(lb), .. } = &self.inner.endpoint else {
            return None;
        };
        lb.request_hash(md)
    }

    /// Record a finished unary call in the throttling bucket, if configured.
    ///
    /// Success refunds `tokenRatio`; any failure (including cancellation and
    /// deadline) removes one token. Local rejections that never ran (a
    /// refused interceptor, an unencodable message, a full concurrency
    /// semaphore) are not call outcomes and are not recorded.
    pub(crate) async fn note_call_outcome(&self, ok: bool) {
        if let Some(state) = self.service_config.get() {
            if let Some(throttler) = &state.throttler {
                if ok {
                    throttler.on_success().await;
                } else {
                    throttler.on_failure().await;
                }
            }
        }
    }

    /// Whether the throttling bucket allows another retry or hedged send.
    ///
    /// `true` when no `retryThrottling` is configured.
    pub(crate) async fn retry_allowed(&self) -> bool {
        let state = self.service_config.get();
        match state.as_ref().and_then(|state| state.throttler.as_ref()) {
            Some(throttler) => throttler.retry_allowed().await,
            None => true,
        }
    }

    /// Per-call wire settings: channel settings tightened by the method entry.
    ///
    /// `maxRequestMessageBytes` tightens the encoding cap and
    /// `maxResponseMessageBytes` tightens the decoding cap; a method entry
    /// never loosens an explicit channel cap.
    pub(crate) fn wire_for(&self, path: &str) -> Wire {
        let mut wire = self.config.wire();
        if let Some(method) = self.method_config_for(path) {
            if let Some(max) = method.max_request_message_bytes {
                let tight = wire.limits.max_encoding().map_or(max, |base| base.min(max));
                wire.limits = wire.limits.with_max_encoding(tight);
            }
            if let Some(max) = method.max_response_message_bytes {
                let tight = wire.limits.max_decoding().map_or(max, |base| base.min(max));
                wire.limits = wire.limits.with_max_decoding(tight);
            }
        }
        wire
    }
}
