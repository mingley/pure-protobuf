//! JSON service config: method config, retry/hedging policy, throttling, LB selection.
//!
//! Implements the client-side service-config schema from gRPC A6 (retry,
//! hedging, throttling), A21 (error handling), and A24 (LB policy config).
//! Parse with [`ServiceConfig::parse`], attach with
//! [`crate::Channel::service_config`], and look up the per-method entry with
//! [`ServiceConfig::method_config`].
//!
//! Unknown top-level and per-method fields are ignored so newer configs keep
//! working. A config that fails validation is [`Code::InvalidArgument`]; per
//! A21 a method whose own entry is invalid fails when it is called, which is
//! why [`ServiceConfig::parse`] validates eagerly and reports the first bad
//! entry.

use crate::status::{Code, Status};
use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// A parsed gRPC service config document.
///
/// Distinct from [`crate::ChannelConfig`]: that is typed `Copy` handshake
/// fields; this is the JSON document a resolver (or the application) supplies.
#[derive(Clone, Debug)]
pub struct ServiceConfig {
    methods: Vec<MethodConfigEntry>,
    lb_policies: Vec<LbPolicyConfig>,
    throttling: Option<RetryThrottling>,
    health_service_name: Option<String>,
}

/// One `methodConfig` entry plus the names it covers.
#[derive(Clone, Debug)]
struct MethodConfigEntry {
    names: Vec<MethodName>,
    config: MethodConfig,
}

impl ServiceConfig {
    /// Parse a JSON service-config document.
    ///
    /// Returns [`Code::InvalidArgument`] naming the first invalid entry.
    /// Unknown fields are ignored. Duplicate method names are rejected.
    pub fn parse(json: &str) -> Result<Self, Status> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            Status::invalid_argument(format!("service config is not valid JSON: {e}"))
        })?;
        Self::parse_value(&value)
    }

    /// Parse an already-decoded JSON value (for resolvers that merge documents).
    ///
    /// Distinct from [`Self::parse`]: that takes the raw document text; this
    /// takes a decoded value so a resolver can overlay DNS-TXT and bootstrap
    /// fragments before validating once.
    pub fn parse_value(value: &serde_json::Value) -> Result<Self, Status> {
        let obj = value
            .as_object()
            .ok_or_else(|| Status::invalid_argument("service config must be a JSON object"))?;
        let mut methods = Vec::new();
        if let Some(list) = obj.get("methodConfig") {
            let list = list.as_array().ok_or_else(|| {
                Status::invalid_argument("service config methodConfig must be an array")
            })?;
            for (index, entry) in list.iter().enumerate() {
                methods.push(parse_method_entry(index, entry)?);
            }
        }
        check_duplicate_names(&methods)?;
        let mut lb_policies = Vec::new();
        if let Some(list) = obj.get("loadBalancingConfig") {
            lb_policies = parse_lb_list(list)?;
        }
        let throttling = obj
            .get("retryThrottling")
            .map(parse_throttling)
            .transpose()?;
        let health_service_name = obj
            .get("healthCheckConfig")
            .and_then(serde_json::Value::as_object)
            .and_then(|hc| hc.get("serviceName"))
            .map(parse_json_string)
            .transpose()?
            .filter(|name| !name.is_empty());
        Ok(Self {
            methods,
            lb_policies,
            throttling,
            health_service_name,
        })
    }

    /// The method config covering `service`/`method`, most specific first.
    ///
    /// An entry naming both service and method wins over a service-only entry,
    /// which wins over the empty default entry. Returns `None` when no entry
    /// covers the call.
    #[must_use]
    pub fn method_config(&self, service: &str, method: &str) -> Option<&MethodConfig> {
        let mut fallback: Option<&MethodConfig> = None;
        for entry in &self.methods {
            for name in &entry.names {
                if name.matches(service, method) {
                    if name.is_exact() {
                        return Some(&entry.config);
                    }
                    if name.is_service_default() {
                        fallback.get_or_insert(&entry.config);
                    } else if fallback.is_none() {
                        fallback = Some(&entry.config);
                    }
                }
            }
        }
        // An exact match returns early; otherwise prefer a service default
        // over the global default even when the file lists it later.
        if fallback.is_some_and(|_| {
            self.methods.iter().any(|entry| {
                entry
                    .names
                    .iter()
                    .any(|name| name.is_service_default() && name.matches(service, method))
            })
        }) {
            for entry in &self.methods {
                if entry
                    .names
                    .iter()
                    .any(|name| name.is_service_default() && name.matches(service, method))
                {
                    return Some(&entry.config);
                }
            }
        }
        fallback
    }

    /// The `loadBalancingConfig` list in preference order (first is preferred).
    ///
    /// The channel uses the first entry it supports and skips the rest, per
    /// A24. Empty when the document sets no LB config.
    #[must_use]
    pub fn lb_policies(&self) -> &[LbPolicyConfig] {
        &self.lb_policies
    }

    /// The `retryThrottling` budget, if the document sets one.
    #[must_use]
    pub fn retry_throttling(&self) -> Option<&RetryThrottling> {
        self.throttling.as_ref()
    }

    /// The `healthCheckConfig.serviceName` override, if the document sets one.
    ///
    /// Client-side health checking uses this name instead of the channel
    /// target when set.
    #[must_use]
    pub fn health_service_name(&self) -> Option<&str> {
        self.health_service_name.as_deref()
    }

    /// Whether the document carries any method entries, LB config, or throttling.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.lb_policies.is_empty() && self.throttling.is_none()
    }
}

/// A `methodConfig.name` selector: empty service/method fields are wildcards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MethodName {
    /// Service name, or empty for all services.
    pub service: String,
    /// Method name, or empty for all methods of the service.
    pub method: String,
}

impl MethodName {
    /// Whether this selector names one exact method.
    #[must_use]
    fn is_exact(&self) -> bool {
        !self.service.is_empty() && !self.method.is_empty()
    }

    /// Whether this selector is a service-wide default.
    #[must_use]
    fn is_service_default(&self) -> bool {
        !self.service.is_empty() && self.method.is_empty()
    }

    /// Whether this selector covers the call.
    #[must_use]
    fn matches(&self, service: &str, method: &str) -> bool {
        (self.service.is_empty() || self.service == service)
            && (self.method.is_empty() || self.method == method)
    }
}

/// Per-method behavior: timeouts, caps, and retry/hedging policy.
///
/// A method carries at most one of [`Self::retry_policy`] and
/// [`Self::hedging_policy`]; a document setting both on one method is invalid.
#[derive(Clone, Debug, Default)]
pub struct MethodConfig {
    /// Names this entry covers, in document order.
    pub names: Vec<MethodName>,
    /// Default `waitForReady` for calls without an explicit overlay.
    pub wait_for_ready: Option<bool>,
    /// Default call timeout for calls without an explicit overlay.
    pub timeout: Option<Duration>,
    /// `maxRequestMessageBytes`, if set.
    pub max_request_message_bytes: Option<usize>,
    /// `maxResponseMessageBytes`, if set.
    pub max_response_message_bytes: Option<usize>,
    /// A6 retry policy, if set.
    pub retry_policy: Option<RetryPolicy>,
    /// A6 hedging policy, if set.
    pub hedging_policy: Option<HedgingPolicy>,
}

/// A6 retry policy for one method.
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    /// Maximum attempts including the first. Parsed values above 5 are
    /// treated as 5, per A6.
    pub max_attempts: u32,
    /// First backoff delay.
    pub initial_backoff: Duration,
    /// Backoff cap.
    pub max_backoff: Duration,
    /// Backoff multiplier, always positive.
    pub backoff_multiplier: f64,
    /// Per-attempt receive timeout, if set.
    pub per_attempt_recv_timeout: Option<Duration>,
    /// Status codes that may be retried. Never empty.
    pub retryable_status_codes: BTreeSet<Code>,
}

/// A6 hedging policy for one method.
#[derive(Clone, Debug)]
pub struct HedgingPolicy {
    /// Maximum attempts including the first. Parsed values above 5 are
    /// treated as 5, per A6.
    pub max_attempts: u32,
    /// Delay before issuing the next hedged attempt, if set. `None` means
    /// hedging is effectively immediate once the previous attempt is sent.
    pub hedging_delay: Option<Duration>,
    /// Statuses that do not commit the call: the client keeps waiting for a
    /// better outcome from an outstanding attempt. Empty means every
    /// non-OK status commits.
    pub non_fatal_status_codes: BTreeSet<Code>,
}

/// A6 retry-throttling budget shared by every method on the channel.
#[derive(Clone, Debug)]
pub struct RetryThrottling {
    /// Token bucket capacity.
    pub max_tokens: f64,
    /// Tokens refunded per successful call.
    pub token_ratio: f64,
}

/// Runtime token bucket for [`RetryThrottling`].
///
/// Starts full at `max_tokens`. Each failed call removes one token; each
/// successful call refunds `token_ratio`, capped at `max_tokens`. A retry (or
/// a hedged send past the first) is allowed only while more than half the
/// bucket remains.
#[derive(Debug)]
pub struct RetryThrottler {
    max_tokens: f64,
    token_ratio: f64,
    tokens: Mutex<f64>,
}

impl RetryThrottler {
    /// Build a full bucket from the config.
    #[must_use]
    pub fn new(config: &RetryThrottling) -> Self {
        Self {
            max_tokens: config.max_tokens,
            token_ratio: config.token_ratio,
            tokens: Mutex::new(config.max_tokens),
        }
    }

    /// Record a failed call. Returns the remaining tokens.
    pub async fn on_failure(&self) -> f64 {
        let mut tokens = self.tokens.lock().await;
        *tokens -= 1.0;
        *tokens
    }

    /// Record a successful call. Returns the remaining tokens.
    pub async fn on_success(&self) -> f64 {
        let mut tokens = self.tokens.lock().await;
        *tokens = (*tokens + self.token_ratio).min(self.max_tokens);
        *tokens
    }

    /// Whether another retry or hedged send is allowed right now.
    pub async fn retry_allowed(&self) -> bool {
        *self.tokens.lock().await > self.max_tokens / 2.0
    }

    /// Current token balance, for tests and telemetry.
    pub async fn tokens(&self) -> f64 {
        *self.tokens.lock().await
    }
}

/// An `loadBalancingConfig` entry: one policy plus its config object.
///
/// Unknown policy names parse as [`Self::Unknown`] so selection can skip them
/// per A24 instead of failing the whole document.
#[derive(Clone, Debug)]
pub enum LbPolicyConfig {
    /// `pick_first` with optional address-list shuffling (A62/A113).
    PickFirst {
        /// Shuffle the address list before connecting.
        shuffle_address_list: bool,
    },
    /// `round_robin`.
    RoundRobin,
    /// Client-side weighted round robin (A58).
    WeightedRoundRobin(WeightedRoundRobinConfig),
    /// Request-hash ring (A42/A76).
    RingHash(RingHashConfig),
    /// Priority failover across localities (A56).
    Priority,
    /// A policy name this kernel does not implement. Selection skips it.
    Unknown(String),
}

/// A58 client-side weighted-round-robin tunables.
#[derive(Clone, Debug)]
pub struct WeightedRoundRobinConfig {
    /// Enable out-of-band ORCA utilization reports.
    pub enable_oob_load_report: bool,
    /// How often OOB reports are sampled. Default 10s.
    pub oob_reporting_period: Duration,
    /// New endpoints get the average weight during this window. Default 10s.
    pub blackout_period: Duration,
    /// How often weights are recomputed. Default 1s.
    pub weight_update_period: Duration,
    /// Weights older than this expire. Default 3m.
    pub weight_expiration_period: Duration,
    /// Penalty added to the utilization of endpoints with errors.
    pub error_utilization_penalty: f64,
}

impl Default for WeightedRoundRobinConfig {
    fn default() -> Self {
        Self {
            enable_oob_load_report: false,
            oob_reporting_period: Duration::from_secs(10),
            blackout_period: Duration::from_secs(10),
            weight_update_period: Duration::from_secs(1),
            weight_expiration_period: Duration::from_secs(180),
            error_utilization_penalty: 1.0,
        }
    }
}

/// A42/A76 ring-hash tunables.
#[derive(Clone, Debug)]
pub struct RingHashConfig {
    /// Minimum ring entries. Default 1024.
    pub min_ring_size: u64,
    /// Maximum ring entries. Default 4096.
    pub max_ring_size: u64,
}

impl Default for RingHashConfig {
    fn default() -> Self {
        Self {
            min_ring_size: 1024,
            max_ring_size: 4096,
        }
    }
}

/// Re-exported for call sites that only name this module.
pub use crate::status::Pushback;

/// Read A6 pushback from a received status.
///
/// The wire layer parses `grpc-retry-pushback-ms` into
/// [`Status::retry_pushback`] on receipt (`grpc-*` keys are never user
/// metadata, so there is nothing to read from [`crate::Metadata`]).
/// Returns `None` when the peer sent no pushback.
#[must_use]
pub fn pushback_delay(status: &Status) -> Option<Pushback> {
    status.retry_pushback()
}

/// A6 backoff for the `n`th retry (`n = 0` is the first retry).
///
/// `initial * multiplier^n`, capped at `max`, with ±20% uniform jitter.
/// The jitter source is a process-mixed splitmix64: unpredictable enough for
/// backoff decorrelation, not a cryptographic secret.
#[must_use]
pub fn retry_backoff(
    initial: Duration,
    max: Duration,
    multiplier: f64,
    retry_index: u32,
) -> Duration {
    let exp = i32::try_from(retry_index).unwrap_or(i32::MAX);
    let base = initial.as_secs_f64() * multiplier.powi(exp);
    let capped = base.min(max.as_secs_f64()).max(0.0);
    let jitter = 0.8 + 0.4 * jitter_unit(retry_index);
    Duration::from_secs_f64(capped * jitter)
}

fn jitter_unit(salt: u32) -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0x1234_5678);
    let count = COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let mut z = nanos
        .wrapping_add(count)
        .wrapping_add(u64::from(salt).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 53 bits as a unit fraction.
    #[allow(clippy::cast_precision_loss, reason = "u64>>11 fits in f64 exactly")]
    let unit = ((z >> 11) as f64) / ((1u64 << 53) as f64);
    unit
}

// --- parsing ---------------------------------------------------------------

fn parse_method_entry(
    index: usize,
    entry: &serde_json::Value,
) -> Result<MethodConfigEntry, Status> {
    let obj = entry.as_object().ok_or_else(|| {
        Status::invalid_argument(format!("methodConfig[{index}] must be an object"))
    })?;
    let names = match obj.get("name") {
        None => vec![MethodName::default()],
        Some(names) => {
            let names = names.as_array().ok_or_else(|| {
                Status::invalid_argument(format!("methodConfig[{index}].name must be an array"))
            })?;
            if names.is_empty() {
                // An empty selector list covers everything, like a missing one.
                vec![MethodName::default()]
            } else {
                names
                    .iter()
                    .enumerate()
                    .map(|(ni, n)| parse_method_name(index, ni, n))
                    .collect::<Result<Vec<_>, _>>()?
            }
        }
    };
    let wait_for_ready = obj.get("waitForReady").map(parse_json_bool).transpose()?;
    let timeout = obj.get("timeout").map(parse_json_duration).transpose()?;
    let max_request_message_bytes = obj
        .get("maxRequestMessageBytes")
        .map(parse_byte_limit)
        .transpose()?;
    let max_response_message_bytes = obj
        .get("maxResponseMessageBytes")
        .map(parse_byte_limit)
        .transpose()?;
    let retry_policy = obj
        .get("retryPolicy")
        .map(|v| parse_retry_policy(index, v))
        .transpose()?;
    let hedging_policy = obj
        .get("hedgingPolicy")
        .map(|v| parse_hedging_policy(index, v))
        .transpose()?;
    if retry_policy.is_some() && hedging_policy.is_some() {
        return Err(Status::invalid_argument(format!(
            "methodConfig[{index}] sets both retryPolicy and hedgingPolicy"
        )));
    }
    Ok(MethodConfigEntry {
        names: names.clone(),
        config: MethodConfig {
            names,
            wait_for_ready,
            timeout,
            max_request_message_bytes,
            max_response_message_bytes,
            retry_policy,
            hedging_policy,
        },
    })
}

fn parse_method_name(
    index: usize,
    ni: usize,
    value: &serde_json::Value,
) -> Result<MethodName, Status> {
    let obj = value.as_object().ok_or_else(|| {
        Status::invalid_argument(format!(
            "methodConfig[{index}].name[{ni}] must be an object"
        ))
    })?;
    let service = obj
        .get("service")
        .map(parse_json_string)
        .transpose()?
        .unwrap_or_default();
    let method = obj
        .get("method")
        .map(parse_json_string)
        .transpose()?
        .unwrap_or_default();
    if service.is_empty() && !method.is_empty() {
        return Err(Status::invalid_argument(format!(
            "methodConfig[{index}].name[{ni}] sets method without service"
        )));
    }
    Ok(MethodName { service, method })
}

fn check_duplicate_names(methods: &[MethodConfigEntry]) -> Result<(), Status> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in methods {
        for name in &entry.names {
            let key = (name.service.clone(), name.method.clone());
            if !seen.insert(key) {
                return Err(Status::invalid_argument(format!(
                    "duplicate methodConfig name {}/{}",
                    name.service, name.method
                )));
            }
        }
    }
    Ok(())
}

fn parse_retry_policy(index: usize, value: &serde_json::Value) -> Result<RetryPolicy, Status> {
    let obj = value.as_object().ok_or_else(|| {
        Status::invalid_argument(format!(
            "methodConfig[{index}].retryPolicy must be an object"
        ))
    })?;
    let max_attempts = parse_max_attempts(index, obj, "retryPolicy")?;
    let initial_backoff = parse_required_duration(index, obj, "retryPolicy", "initialBackoff")?;
    let max_backoff = parse_required_duration(index, obj, "retryPolicy", "maxBackoff")?;
    let backoff_multiplier = obj
        .get("backoffMultiplier")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| {
            Status::invalid_argument(format!(
                "methodConfig[{index}].retryPolicy.backoffMultiplier is required"
            ))
        })?;
    if !backoff_multiplier.is_finite() || backoff_multiplier <= 0.0 {
        return Err(Status::invalid_argument(format!(
            "methodConfig[{index}].retryPolicy.backoffMultiplier must be a positive finite number"
        )));
    }
    let per_attempt_recv_timeout = obj
        .get("perAttemptRecvTimeout")
        .map(parse_json_duration)
        .transpose()?;
    let codes = obj.get("retryableStatusCodes").ok_or_else(|| {
        Status::invalid_argument(format!(
            "methodConfig[{index}].retryPolicy.retryableStatusCodes is required"
        ))
    })?;
    let codes = codes.as_array().ok_or_else(|| {
        Status::invalid_argument(format!(
            "methodConfig[{index}].retryPolicy.retryableStatusCodes must be an array"
        ))
    })?;
    if codes.is_empty() {
        return Err(Status::invalid_argument(format!(
            "methodConfig[{index}].retryPolicy.retryableStatusCodes must not be empty"
        )));
    }
    let mut retryable_status_codes = BTreeSet::new();
    for code in codes {
        let name = parse_json_string(code)?;
        let code = Code::from_str(&name)
            .map_err(|_| Status::invalid_argument(format!("unknown retryable code {name:?}")))?;
        retryable_status_codes.insert(code);
    }
    Ok(RetryPolicy {
        max_attempts,
        initial_backoff,
        max_backoff,
        backoff_multiplier,
        per_attempt_recv_timeout,
        retryable_status_codes,
    })
}

fn parse_hedging_policy(index: usize, value: &serde_json::Value) -> Result<HedgingPolicy, Status> {
    let obj = value.as_object().ok_or_else(|| {
        Status::invalid_argument(format!(
            "methodConfig[{index}].hedgingPolicy must be an object"
        ))
    })?;
    let max_attempts = parse_max_attempts(index, obj, "hedgingPolicy")?;
    let hedging_delay = obj
        .get("hedgingDelay")
        .map(parse_json_duration)
        .transpose()?;
    let mut non_fatal_status_codes = BTreeSet::new();
    if let Some(codes) = obj.get("nonFatalStatusCodes") {
        let codes = codes.as_array().ok_or_else(|| {
            Status::invalid_argument(format!(
                "methodConfig[{index}].hedgingPolicy.nonFatalStatusCodes must be an array"
            ))
        })?;
        for code in codes {
            let name = parse_json_string(code)?;
            let code = Code::from_str(&name)
                .map_err(|_| Status::invalid_argument(format!("unknown hedging code {name:?}")))?;
            non_fatal_status_codes.insert(code);
        }
    }
    Ok(HedgingPolicy {
        max_attempts,
        hedging_delay,
        non_fatal_status_codes,
    })
}

fn parse_max_attempts(
    index: usize,
    obj: &serde_json::Map<String, serde_json::Value>,
    policy: &str,
) -> Result<u32, Status> {
    let attempts = obj
        .get("maxAttempts")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            Status::invalid_argument(format!(
                "methodConfig[{index}].{policy}.maxAttempts is required"
            ))
        })?;
    if attempts < 2 {
        return Err(Status::invalid_argument(format!(
            "methodConfig[{index}].{policy}.maxAttempts must be at least 2"
        )));
    }
    Ok(u32::try_from(attempts.min(5)).unwrap_or(5))
}

fn parse_required_duration(
    index: usize,
    obj: &serde_json::Map<String, serde_json::Value>,
    policy: &str,
    field: &str,
) -> Result<Duration, Status> {
    obj.get(field)
        .map(parse_json_duration)
        .transpose()?
        .ok_or_else(|| {
            Status::invalid_argument(format!(
                "methodConfig[{index}].{policy}.{field} is required"
            ))
        })
}

fn parse_throttling(value: &serde_json::Value) -> Result<RetryThrottling, Status> {
    let obj = value
        .as_object()
        .ok_or_else(|| Status::invalid_argument("retryThrottling must be an object"))?;
    let max_tokens = obj
        .get("maxTokens")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| Status::invalid_argument("retryThrottling.maxTokens is required"))?;
    let token_ratio = obj
        .get("tokenRatio")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| Status::invalid_argument("retryThrottling.tokenRatio is required"))?;
    if !max_tokens.is_finite() || max_tokens <= 0.0 {
        return Err(Status::invalid_argument(
            "retryThrottling.maxTokens must be a positive finite number",
        ));
    }
    if !token_ratio.is_finite() || token_ratio <= 0.0 {
        return Err(Status::invalid_argument(
            "retryThrottling.tokenRatio must be a positive finite number",
        ));
    }
    Ok(RetryThrottling {
        max_tokens,
        token_ratio,
    })
}

fn parse_lb_list(value: &serde_json::Value) -> Result<Vec<LbPolicyConfig>, Status> {
    let list = value
        .as_array()
        .ok_or_else(|| Status::invalid_argument("loadBalancingConfig must be an array"))?;
    if list.is_empty() {
        return Err(Status::invalid_argument(
            "loadBalancingConfig must not be empty",
        ));
    }
    list.iter().map(parse_lb_entry).collect()
}

fn parse_lb_entry(value: &serde_json::Value) -> Result<LbPolicyConfig, Status> {
    let obj = value
        .as_object()
        .ok_or_else(|| Status::invalid_argument("loadBalancingConfig entries must be objects"))?;
    if obj.len() != 1 {
        return Err(Status::invalid_argument(
            "loadBalancingConfig entries must hold exactly one policy",
        ));
    }
    let (name, config) = obj
        .iter()
        .next()
        .ok_or_else(|| Status::invalid_argument("loadBalancingConfig entry is empty"))?;
    match name.as_str() {
        "pick_first" => {
            let shuffle = config
                .as_object()
                .and_then(|o| o.get("shuffleAddressList"))
                .map(parse_json_bool)
                .transpose()?
                .unwrap_or(false);
            Ok(LbPolicyConfig::PickFirst {
                shuffle_address_list: shuffle,
            })
        }
        "round_robin" => Ok(LbPolicyConfig::RoundRobin),
        "weighted_round_robin" => Ok(LbPolicyConfig::WeightedRoundRobin(parse_wrr(config)?)),
        "ring_hash" => Ok(LbPolicyConfig::RingHash(parse_ring_hash(config)?)),
        "priority" => Ok(LbPolicyConfig::Priority),
        other => Ok(LbPolicyConfig::Unknown(other.to_owned())),
    }
}

fn parse_wrr(value: &serde_json::Value) -> Result<WeightedRoundRobinConfig, Status> {
    let mut out = WeightedRoundRobinConfig::default();
    let Some(obj) = value.as_object() else {
        return Ok(out);
    };
    if let Some(v) = obj.get("enableOobLoadReport") {
        out.enable_oob_load_report = parse_json_bool(v)?;
    }
    if let Some(v) = obj.get("oobReportingPeriod") {
        out.oob_reporting_period = parse_json_duration(v)?;
    }
    if let Some(v) = obj.get("blackoutPeriod") {
        out.blackout_period = parse_json_duration(v)?;
    }
    if let Some(v) = obj.get("weightUpdatePeriod") {
        out.weight_update_period = parse_json_duration(v)?;
    }
    if let Some(v) = obj.get("weightExpirationPeriod") {
        out.weight_expiration_period = parse_json_duration(v)?;
    }
    if let Some(v) = obj.get("errorUtilizationPenalty") {
        out.error_utilization_penalty = v.as_f64().ok_or_else(|| {
            Status::invalid_argument(
                "weighted_round_robin.errorUtilizationPenalty must be a number",
            )
        })?;
    }
    Ok(out)
}

fn parse_ring_hash(value: &serde_json::Value) -> Result<RingHashConfig, Status> {
    let mut out = RingHashConfig::default();
    let Some(obj) = value.as_object() else {
        return Ok(out);
    };
    if let Some(v) = obj.get("minRingSize") {
        out.min_ring_size = v.as_u64().ok_or_else(|| {
            Status::invalid_argument("ring_hash.minRingSize must be an unsigned integer")
        })?;
    }
    if let Some(v) = obj.get("maxRingSize") {
        out.max_ring_size = v.as_u64().ok_or_else(|| {
            Status::invalid_argument("ring_hash.maxRingSize must be an unsigned integer")
        })?;
    }
    if out.min_ring_size == 0 || out.max_ring_size < out.min_ring_size {
        return Err(Status::invalid_argument(
            "ring_hash requires 0 < minRingSize <= maxRingSize",
        ));
    }
    Ok(out)
}

fn parse_json_bool(value: &serde_json::Value) -> Result<bool, Status> {
    value
        .as_bool()
        .ok_or_else(|| Status::invalid_argument(format!("expected a JSON boolean, found {value}")))
}

fn parse_json_string(value: &serde_json::Value) -> Result<String, Status> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Status::invalid_argument(format!("expected a JSON string, found {value}")))
}

fn parse_byte_limit(value: &serde_json::Value) -> Result<usize, Status> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| {
            Status::invalid_argument(format!(
                "message byte limit must be an unsigned integer, found {value}"
            ))
        })
}

/// Parse a protobuf-JSON duration (`"3.2s"`).
fn parse_json_duration(value: &serde_json::Value) -> Result<Duration, Status> {
    let raw = parse_json_string(value)?;
    parse_duration_str(&raw)
        .ok_or_else(|| Status::invalid_argument(format!("invalid duration {raw:?}")))
}

fn parse_duration_str(raw: &str) -> Option<Duration> {
    let digits = raw.strip_suffix('s')?;
    if digits.is_empty() || digits.starts_with('-') {
        return None;
    }
    let (secs, frac) = match digits.split_once('.') {
        Some((s, f)) => (s, f),
        None => (digits, ""),
    };
    if frac.len() > 9 {
        return None;
    }
    let secs: u64 = if secs.is_empty() {
        0
    } else {
        secs.parse().ok()?
    };
    let mut nanos: u32 = if frac.is_empty() {
        0
    } else {
        frac.parse().ok()?
    };
    for _ in frac.len()..9 {
        nanos = nanos.checked_mul(10)?;
    }
    secs.checked_mul(1_000_000_000)
        .and_then(|ns| ns.checked_add(u64::from(nanos)))
        .map(Duration::from_nanos)
}

/// Shared, cloneable service-config handle for [`crate::Channel`].
#[derive(Clone, Debug, Default)]
pub(crate) struct SharedServiceConfig {
    inner: Option<Arc<ServiceConfigState>>,
}

#[derive(Debug)]
pub(crate) struct ServiceConfigState {
    /// The parsed document.
    pub(crate) config: ServiceConfig,
    /// Channel-wide throttling bucket, if the document sets one.
    pub(crate) throttler: Option<RetryThrottler>,
}

impl SharedServiceConfig {
    /// Attach a parsed document, building its throttling bucket.
    pub(crate) fn new(config: ServiceConfig) -> Self {
        let throttler = config.retry_throttling().map(RetryThrottler::new);
        Self {
            inner: Some(Arc::new(ServiceConfigState { config, throttler })),
        }
    }

    /// The attached state, if any.
    pub(crate) fn get(&self) -> Option<&ServiceConfigState> {
        self.inner.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{
        "loadBalancingConfig": [{"round_robin": {}}, {"pick_first": {"shuffleAddressList": true}}],
        "methodConfig": [
            {"name": [], "waitForReady": true},
            {"name": [{"service": "a.B"}], "timeout": "3s"},
            {"name": [{"service": "a.B", "method": "C"}],
             "retryPolicy": {
                 "maxAttempts": 3,
                 "initialBackoff": "0.1s",
                 "maxBackoff": "1s",
                 "backoffMultiplier": 2.0,
                 "retryableStatusCodes": ["UNAVAILABLE"]
             }},
            {"name": [{"service": "x.Y", "method": "Z"}],
             "hedgingPolicy": {"maxAttempts": 2, "nonFatalStatusCodes": ["UNAVAILABLE"]}}
        ],
        "retryThrottling": {"maxTokens": 10, "tokenRatio": 0.5},
        "healthCheckConfig": {"serviceName": "hc"},
        "unknownFutureField": {"ignored": true}
    }"#;

    #[test]
    fn parses_full_document() {
        let config = ServiceConfig::parse(FULL).unwrap();
        assert_eq!(config.lb_policies().len(), 2);
        assert!(matches!(
            config.lb_policies()[0],
            LbPolicyConfig::RoundRobin
        ));
        assert_eq!(config.health_service_name(), Some("hc"));
        assert!(!config.is_empty());
        let exact = config.method_config("a.B", "C").unwrap();
        assert!(exact.retry_policy.is_some());
        assert_eq!(exact.retry_policy.as_ref().unwrap().max_attempts, 3);
        let service = config.method_config("a.B", "Other").unwrap();
        assert_eq!(service.timeout, Some(Duration::from_secs(3)));
        assert!(service.retry_policy.is_none());
        let global = config.method_config("nope.Nope", "Nope").unwrap();
        assert_eq!(global.wait_for_ready, Some(true));
    }

    #[test]
    fn method_without_service_is_invalid() {
        let bad = r#"{"methodConfig": [{"name": [{"method": "C"}]}]}"#;
        assert!(ServiceConfig::parse(bad).is_err());
    }

    #[test]
    fn duplicate_names_are_invalid() {
        let bad = r#"{"methodConfig": [
            {"name": [{"service": "a.B"}]},
            {"name": [{"service": "a.B", "method": "C"}, {"service": "a.B"}]}
        ]}"#;
        assert!(ServiceConfig::parse(bad).is_err());
    }

    #[test]
    fn retry_and_hedging_are_exclusive() {
        let bad = r#"{"methodConfig": [{"name": [{}],
            "retryPolicy": {"maxAttempts": 2, "initialBackoff": "0.1s",
                "maxBackoff": "1s", "backoffMultiplier": 2.0,
                "retryableStatusCodes": ["UNAVAILABLE"]},
            "hedgingPolicy": {"maxAttempts": 2}}]}"#;
        assert!(ServiceConfig::parse(bad).is_err());
    }

    #[test]
    fn max_attempts_clamps_to_five() {
        let doc = r#"{"methodConfig": [{"name": [{}],
            "retryPolicy": {"maxAttempts": 9, "initialBackoff": "0.1s",
                "maxBackoff": "1s", "backoffMultiplier": 2.0,
                "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
        let config = ServiceConfig::parse(doc).unwrap();
        let method = config.method_config("", "").unwrap();
        assert_eq!(method.retry_policy.as_ref().unwrap().max_attempts, 5);
    }

    #[test]
    fn unknown_lb_policy_parses_as_unknown() {
        let doc = r#"{"loadBalancingConfig": [{"fancy_future_lb": {}}, {"round_robin": {}}]}"#;
        let config = ServiceConfig::parse(doc).unwrap();
        assert!(matches!(
            config.lb_policies()[0],
            LbPolicyConfig::Unknown(_)
        ));
        assert!(matches!(
            config.lb_policies()[1],
            LbPolicyConfig::RoundRobin
        ));
    }

    #[test]
    fn durations_parse_fractional_seconds() {
        assert_eq!(parse_duration_str("3s"), Some(Duration::from_secs(3)));
        assert_eq!(parse_duration_str("0.1s"), Some(Duration::from_millis(100)));
        assert_eq!(
            parse_duration_str("1.000000001s"),
            Some(Duration::new(1, 1))
        );
        assert_eq!(parse_duration_str("-1s"), None);
        assert_eq!(parse_duration_str("1"), None);
        assert_eq!(parse_duration_str("1.0000000001s"), None);
    }

    #[test]
    fn pushback_reads_status_slot() {
        let plain = Status::unavailable("down");
        assert_eq!(pushback_delay(&plain), None);
        let delayed = Status::unavailable("slow")
            .with_retry_pushback(Pushback::Delay(Duration::from_millis(250)));
        assert_eq!(
            pushback_delay(&delayed),
            Some(Pushback::Delay(Duration::from_millis(250)))
        );
        let stop = Status::unavailable("stop").with_retry_pushback(Pushback::DoNotRetry);
        assert_eq!(pushback_delay(&stop), Some(Pushback::DoNotRetry));
    }

    #[test]
    fn pushback_values_parse() {
        use crate::status::parse_pushback_value;
        assert_eq!(
            parse_pushback_value("250"),
            Some(Pushback::Delay(Duration::from_millis(250)))
        );
        assert_eq!(parse_pushback_value("-1"), Some(Pushback::DoNotRetry));
        assert_eq!(parse_pushback_value("bogus"), None);
        assert_eq!(parse_pushback_value("-2"), None);
        assert_eq!(parse_pushback_value(""), None);
    }

    #[test]
    fn backoff_stays_within_jitter_band() {
        for n in 0..6 {
            let delay = retry_backoff(Duration::from_millis(100), Duration::from_secs(10), 2.0, n);
            let nominal = 100.0 * 2.0f64.powi(i32::try_from(n).unwrap_or(i32::MAX)) / 1000.0;
            let capped = nominal.min(10.0);
            let secs = delay.as_secs_f64();
            assert!(secs >= capped * 0.79, "attempt {n}: {secs} vs {capped}");
            assert!(secs <= capped * 1.21, "attempt {n}: {secs} vs {capped}");
        }
    }

    #[tokio::test]
    async fn throttler_gates_at_half_bucket() {
        let config = RetryThrottling {
            max_tokens: 10.0,
            token_ratio: 1.0,
        };
        let throttler = RetryThrottler::new(&config);
        assert!(throttler.retry_allowed().await);
        for _ in 0..5 {
            throttler.on_failure().await;
        }
        assert!(!throttler.retry_allowed().await);
        throttler.on_success().await;
        assert!(throttler.retry_allowed().await);
    }
}
