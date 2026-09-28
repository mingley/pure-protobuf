//! Audit logging for authorization decisions (gRFC A59).
//!
//! A policy may carry `audit_logging_options`: a condition (`NONE` /
//! `ON_DENY` / `ON_ALLOW` / `ON_DENY_AND_ALLOW`, default `NONE`) plus a
//! list of logger configs. Right after each authorization decision, every
//! configured logger's [`AuditLogger::log`] runs synchronously with an
//! [`AuditEvent`]: the RPC method, peer principal, policy name, matched
//! rule (empty on default-deny), and whether the call was authorized.
//!
//! The built-in `stdout_logger` prints one JSON object per event (see
//! [`format_record`]). Custom logger types plug in through
//! [`register_audit_logger_factory`]; a config naming an unknown logger
//! or failing validation fails policy load unless it sets
//! `is_optional`, in which case it is ignored.
//!
//! Records deliberately carry no headers or metadata, so credential or
//! PII-bearing fields can never leak through audit (OB-03): the event is
//! exactly the five A59 fields.

#![allow(
    clippy::disallowed_types,
    reason = "init-time factory registry; reads happen at policy parse, never across await"
)]

use super::policy::PolicyError;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// When audit logging fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AuditCondition {
    /// Never audit. This is also the default when the condition is omitted.
    #[default]
    None,
    /// Audit when the decision denies the call.
    OnDeny,
    /// Audit when the decision allows the call.
    OnAllow,
    /// Audit every decision.
    OnDenyAndAllow,
}

impl AuditCondition {
    pub(crate) fn parse(name: &str) -> Result<Self, PolicyError> {
        match name {
            "NONE" => Ok(Self::None),
            "ON_DENY" => Ok(Self::OnDeny),
            "ON_ALLOW" => Ok(Self::OnAllow),
            "ON_DENY_AND_ALLOW" => Ok(Self::OnDenyAndAllow),
            other => Err(PolicyError::message(format!(
                "audit_logging_options.audit_condition must be NONE, ON_DENY, ON_ALLOW or ON_DENY_AND_ALLOW, got {other:?}"
            ))),
        }
    }

    fn fires(&self, authorized: bool) -> bool {
        match self {
            Self::None => false,
            Self::OnDeny => !authorized,
            Self::OnAllow => authorized,
            Self::OnDenyAndAllow => true,
        }
    }
}

/// The A59 audit context for one authorization decision.
///
/// Owned strings: loggers may retain the event or hand it to background
/// work. There are deliberately no headers or metadata fields (OB-03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEvent {
    /// Fully qualified path, e.g. `/pkg.Service/Method`.
    pub rpc_method: String,
    /// Peer identity (first URI SAN, else DNS SAN, else subject), or `""`
    /// without certificate-based TLS authentication.
    pub principal: String,
    /// The authorization policy name.
    pub policy_name: String,
    /// The matched rule name, empty when no rule matched.
    pub matched_rule: String,
    /// Whether the call was authorized.
    pub authorized: bool,
}

/// A sink for audit events.
///
/// `log` runs synchronously in the RPC path right after the
/// authorization decision; it must not block the RPC. Fan slow work out
/// to background tasks and return promptly. Nothing is returned, so a
/// logger can never change the decision.
pub trait AuditLogger: Send + Sync + 'static {
    /// Logger type name (e.g. `stdout_logger`), for diagnostics.
    fn name(&self) -> &str;

    /// Record one event.
    fn log(&self, event: &AuditEvent);
}

/// Builds [`AuditLogger`]s of one named type from policy config.
///
/// `build` receives the logger's `config` object (`null` when the
/// policy omits it) after the policy parser validated the surrounding
/// shape. Return `Err` to reject the config: policy load fails unless
/// the entry sets `is_optional`.
pub trait AuditLoggerFactory: Send + Sync + 'static {
    /// Logger type name this factory builds.
    fn name(&self) -> &str;

    /// Validate `config` and build a logger.
    fn build(&self, config: &serde_json::Value) -> Result<Box<dyn AuditLogger>, PolicyError>;
}

fn registry() -> &'static Mutex<HashMap<String, Arc<dyn AuditLoggerFactory>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<dyn AuditLoggerFactory>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map: HashMap<String, Arc<dyn AuditLoggerFactory>> = HashMap::new();
        map.insert(
            StdoutAuditLogger::TYPE.to_owned(),
            Arc::new(StdoutAuditLoggerFactory),
        );
        Mutex::new(map)
    })
}

/// Register a logger factory for policy configs naming
/// [`AuditLoggerFactory::name`]. Call during initialization, before
/// parsing policies; registering the same name twice replaces the
/// previous factory.
pub fn register_audit_logger_factory(factory: Arc<dyn AuditLoggerFactory>) {
    if let Ok(mut guard) = registry().lock() {
        guard.insert(factory.name().to_owned(), factory);
    }
}

fn build_logger(
    type_name: &str,
    config: &serde_json::Value,
) -> Result<Box<dyn AuditLogger>, PolicyError> {
    let factory = registry()
        .lock()
        .ok()
        .and_then(|guard| guard.get(type_name).cloned());
    match factory {
        Some(factory) => factory.build(config),
        None => Err(PolicyError::message(format!(
            "unknown audit logger type {type_name:?}"
        ))),
    }
}

/// Parsed `audit_logging_options`: the condition plus built loggers.
#[derive(Clone)]
pub(crate) struct AuditOptions {
    condition: AuditCondition,
    loggers: Vec<Arc<dyn AuditLogger>>,
}

impl core::fmt::Debug for AuditOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AuditOptions")
            .field("condition", &self.condition)
            .field(
                "loggers",
                &self.loggers.iter().map(|l| l.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl AuditOptions {
    pub(crate) fn parse(value: &serde_json::Value) -> Result<Self, PolicyError> {
        let obj = value
            .as_object()
            .ok_or_else(|| PolicyError::message("audit_logging_options must be an object"))?;
        for key in obj.keys() {
            if !matches!(key.as_str(), "audit_condition" | "audit_logger") {
                return Err(PolicyError::message(format!(
                    "audit_logging_options has unknown field {key:?}"
                )));
            }
        }
        let condition = obj
            .get("audit_condition")
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    PolicyError::message("audit_logging_options.audit_condition must be a string")
                })
            })
            .transpose()?
            .map(AuditCondition::parse)
            .transpose()?
            .unwrap_or_default();
        let mut loggers = Vec::new();
        if let Some(configs) = obj.get("audit_logger") {
            let list = configs.as_array().ok_or_else(|| {
                PolicyError::message("audit_logging_options.audit_logger must be an array")
            })?;
            for (index, item) in list.iter().enumerate() {
                if let Some(logger) = parse_logger_config(item, index)? {
                    loggers.push(Arc::from(logger));
                }
            }
        }
        Ok(Self { condition, loggers })
    }

    /// Run the loggers when `event` meets the condition.
    pub(crate) fn audit(&self, event: &AuditEvent) {
        if !self.condition.fires(event.authorized) {
            return;
        }
        for logger in &self.loggers {
            logger.log(event);
        }
    }
}

fn parse_logger_config(
    value: &serde_json::Value,
    index: usize,
) -> Result<Option<Box<dyn AuditLogger>>, PolicyError> {
    let obj = value.as_object().ok_or_else(|| {
        PolicyError::message(format!(
            "audit_logging_options.audit_logger[{index}] must be an object"
        ))
    })?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "name" | "config" | "is_optional") {
            return Err(PolicyError::message(format!(
                "audit_logging_options.audit_logger[{index}] has unknown field {key:?}"
            )));
        }
    }
    let name = obj
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            PolicyError::message(format!(
                "audit_logging_options.audit_logger[{index}] requires a string \"name\""
            ))
        })?;
    let config = obj.get("config").unwrap_or(&serde_json::Value::Null);
    if !config.is_null() && !config.is_object() {
        return Err(PolicyError::message(format!(
            "audit_logging_options.audit_logger[{index}].config must be an object"
        )));
    }
    let is_optional = obj
        .get("is_optional")
        .map(|v| {
            v.as_bool().ok_or_else(|| {
                PolicyError::message(format!(
                    "audit_logging_options.audit_logger[{index}].is_optional must be a boolean"
                ))
            })
        })
        .transpose()?
        .unwrap_or(false);
    match build_logger(name, config) {
        Ok(logger) => Ok(Some(logger)),
        Err(_) if is_optional => Ok(None),
        Err(e) => Err(PolicyError::message(format!(
            "audit_logging_options.audit_logger[{index}]: {e}"
        ))),
    }
}

/// Factory for the built-in `stdout_logger`.
#[derive(Debug, Default)]
struct StdoutAuditLoggerFactory;

impl AuditLoggerFactory for StdoutAuditLoggerFactory {
    fn name(&self) -> &str {
        StdoutAuditLogger::TYPE
    }

    fn build(&self, config: &serde_json::Value) -> Result<Box<dyn AuditLogger>, PolicyError> {
        if !config.is_null() && config.as_object().is_none_or(|o| !o.is_empty()) {
            return Err(PolicyError::message(
                "stdout_logger takes no configuration fields",
            ));
        }
        Ok(Box::new(StdoutAuditLogger))
    }
}

/// Built-in logger: one [`format_record`] JSON object per event on stdout.
#[derive(Debug, Default)]
pub struct StdoutAuditLogger;

impl StdoutAuditLogger {
    /// Logger type name used in policy configs.
    pub const TYPE: &'static str = "stdout_logger";
}

impl AuditLogger for StdoutAuditLogger {
    fn name(&self) -> &str {
        Self::TYPE
    }

    fn log(&self, event: &AuditEvent) {
        println!("{}", format_record(event, SystemTime::now()));
    }
}

/// Render one A59 log entry: `{"grpc_audit_log":{...}}` with exactly the
/// five audit fields plus the RFC 3339 nanosecond timestamp. No headers
/// or metadata are emitted (OB-03).
#[must_use]
pub fn format_record(event: &AuditEvent, timestamp: SystemTime) -> String {
    format!(
        "{{\"grpc_audit_log\":{{\"timestamp\":{},\"rpc_method\":{},\"principal\":{},\"policy_name\":{},\"matched_rule\":{},\"authorized\":{}}}}}",
        json_string(&format_timestamp(timestamp)),
        json_string(&event.rpc_method),
        json_string(&event.principal),
        json_string(&event.policy_name),
        json_string(&event.matched_rule),
        event.authorized,
    )
}

/// Quote a string for JSON.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Format a timestamp as RFC 3339 UTC with nanosecond precision, e.g.
/// `2006-01-02T15:04:05.999999999Z`.
fn format_timestamp(timestamp: SystemTime) -> String {
    let elapsed = timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
    let days = secs / 86_400;
    let day_secs = secs % 86_400;
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:09}Z",
        day_secs / 3_600,
        (day_secs % 3_600) / 60,
        day_secs % 60,
        elapsed.subsec_nanos(),
    )
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's
/// `civil_from_days`, shifted from the 1970 epoch (719_468 days after
/// 0000-03-01).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (i32::try_from(y + i64::from(m <= 2)).unwrap_or(1970), m, d)
}

#[cfg(test)]
mod tests {
    use super::{AuditCondition, AuditEvent, format_record, format_timestamp};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    fn event() -> AuditEvent {
        AuditEvent {
            rpc_method: "/pkg.Service/Foo".to_owned(),
            principal: "spiffe://foo/user1".to_owned(),
            policy_name: "example_policy".to_owned(),
            matched_rule: "admin_access".to_owned(),
            authorized: true,
        }
    }

    #[test]
    fn conditions_parse_and_fire() {
        assert_eq!(
            AuditCondition::parse("NONE").expect("none"),
            AuditCondition::None
        );
        assert_eq!(
            AuditCondition::parse("ON_DENY").expect("deny"),
            AuditCondition::OnDeny
        );
        assert_eq!(
            AuditCondition::parse("ON_ALLOW").expect("allow"),
            AuditCondition::OnAllow
        );
        assert_eq!(
            AuditCondition::parse("ON_DENY_AND_ALLOW").expect("both"),
            AuditCondition::OnDenyAndAllow
        );
        assert!(AuditCondition::parse("SOMETIMES").is_err());

        assert!(!AuditCondition::None.fires(true));
        assert!(!AuditCondition::None.fires(false));
        assert!(!AuditCondition::OnDeny.fires(true));
        assert!(AuditCondition::OnDeny.fires(false));
        assert!(AuditCondition::OnAllow.fires(true));
        assert!(!AuditCondition::OnAllow.fires(false));
        assert!(AuditCondition::OnDenyAndAllow.fires(true));
        assert!(AuditCondition::OnDenyAndAllow.fires(false));
    }

    #[test]
    fn record_matches_a59_shape() {
        let stamp = SystemTime::UNIX_EPOCH + Duration::new(1_672_531_200, 123_456_789);
        let record = format_record(&event(), stamp);
        let value: serde_json::Value = serde_json::from_str(&record).expect("valid JSON");
        let entry = value.get("grpc_audit_log").expect("wrapper");
        let mut keys: Vec<&str> = entry
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "authorized",
                "matched_rule",
                "policy_name",
                "principal",
                "rpc_method",
                "timestamp"
            ]
        );
        assert_eq!(
            entry.get("timestamp").expect("ts"),
            "2023-01-01T00:00:00.123456789Z"
        );
        assert_eq!(entry.get("rpc_method").expect("m"), "/pkg.Service/Foo");
        assert_eq!(entry.get("authorized").expect("a"), true);
    }

    #[test]
    fn timestamp_edges() {
        assert_eq!(
            format_timestamp(SystemTime::UNIX_EPOCH),
            "1970-01-01T00:00:00.000000000Z"
        );
        // A leap day.
        assert_eq!(
            format_timestamp(SystemTime::UNIX_EPOCH + Duration::new(1_582_934_400, 0)),
            "2020-02-29T00:00:00.000000000Z"
        );
    }

    #[test]
    fn record_escapes_strings() {
        let mut e = event();
        e.policy_name = "a\"b\\c\n".to_owned();
        let record = format_record(&e, SystemTime::UNIX_EPOCH);
        let value: serde_json::Value = serde_json::from_str(&record).expect("valid JSON");
        assert_eq!(
            value
                .get("grpc_audit_log")
                .expect("w")
                .get("policy_name")
                .expect("p"),
            "a\"b\\c\n"
        );
    }

    #[test]
    fn optional_unknown_logger_is_ignored() {
        let options = super::AuditOptions::parse(
            &serde_json::from_str::<serde_json::Value>(
                r#"{"audit_condition":"ON_DENY","audit_logger":[
                    {"name":"no_such_logger","is_optional":true}
                ]}"#,
            )
            .expect("json"),
        )
        .expect("optional unknown logger is skipped");
        assert!(options.loggers.is_empty());
    }

    #[test]
    fn required_unknown_logger_fails_closed() {
        assert!(
            super::AuditOptions::parse(
                &serde_json::from_str::<serde_json::Value>(
                    r#"{"audit_logger":[{"name":"no_such_logger"}]}"#,
                )
                .expect("json"),
            )
            .is_err()
        );
    }

    #[test]
    fn custom_factory_round_trip() {
        struct Probe;
        impl super::AuditLogger for Probe {
            fn name(&self) -> &str {
                "unit_probe"
            }
            fn log(&self, _event: &super::AuditEvent) {}
        }
        struct ProbeFactory;
        impl super::AuditLoggerFactory for ProbeFactory {
            fn name(&self) -> &str {
                "unit_probe"
            }
            fn build(
                &self,
                config: &serde_json::Value,
            ) -> Result<Box<dyn super::AuditLogger>, super::PolicyError> {
                assert!(config.is_null());
                Ok(Box::new(Probe))
            }
        }
        super::register_audit_logger_factory(Arc::new(ProbeFactory));
        let options = super::AuditOptions::parse(
            &serde_json::from_str::<serde_json::Value>(
                r#"{"audit_condition":"ON_DENY_AND_ALLOW","audit_logger":[{"name":"unit_probe"}]}"#,
            )
            .expect("json"),
        )
        .expect("custom logger builds");
        assert_eq!(options.condition, AuditCondition::OnDenyAndAllow);
        assert_eq!(options.loggers.len(), 1);
        assert_eq!(options.loggers[0].name(), "unit_probe");
    }
}
