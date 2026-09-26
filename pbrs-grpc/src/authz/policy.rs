//! A43 authorization policy: JSON parsing and matching.
//!
//! [`Policy::from_json`] parses the gRFC A43 policy language strictly:
//! `name` and `allow_rules` are required, unknown fields are rejected, and
//! anything malformed fails closed (the server cannot start with it).
//! Matching follows the gRFC: deny rules first, then allow rules, default
//! deny; within a rule, source and request are ANDed while principals,
//! paths, and header values are ORed and headers are ANDed.

use super::audit::{AuditEvent, AuditOptions};
use super::matcher::StringMatcher;
use super::principal::PeerPrincipals;

/// A parsed A43 authorization policy.
#[derive(Clone, Debug)]
pub struct Policy {
    name: String,
    allow_rules: Vec<Rule>,
    deny_rules: Vec<Rule>,
    audit: Option<AuditOptions>,
}

impl Policy {
    /// Parse policy JSON. Fails closed on any schema violation.
    pub fn from_json(json: &str) -> Result<Self, PolicyError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| PolicyError(format!("invalid JSON: {e}")))?;
        Self::from_value(&value)
    }

    /// Parse an already-decoded JSON value (file watchers reuse this).
    pub fn from_value(value: &serde_json::Value) -> Result<Self, PolicyError> {
        let obj = value
            .as_object()
            .ok_or_else(|| PolicyError::new("policy must be an object"))?;
        for key in obj.keys() {
            if !matches!(
                key.as_str(),
                "name" | "allow_rules" | "deny_rules" | "audit_logging_options"
            ) {
                return Err(PolicyError::new(format!("unknown policy field {key:?}")));
            }
        }
        let name = obj
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| PolicyError::new("policy requires a string \"name\""))?;
        let allow_rules = obj
            .get("allow_rules")
            .ok_or_else(|| PolicyError::new("policy requires \"allow_rules\""))?;
        let allow_rules = parse_rules(allow_rules, "allow_rules")?;
        let deny_rules = obj
            .get("deny_rules")
            .map(|v| parse_rules(v, "deny_rules"))
            .transpose()?
            .unwrap_or_default();
        let audit = obj
            .get("audit_logging_options")
            .map(AuditOptions::parse)
            .transpose()?;
        Ok(Self {
            name: name.to_owned(),
            allow_rules,
            deny_rules,
            audit,
        })
    }

    /// Policy name, for monitoring and error messages.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Allow rules in policy order.
    #[must_use]
    pub fn allow_rules(&self) -> &[Rule] {
        &self.allow_rules
    }

    /// Deny rules in policy order.
    #[must_use]
    pub fn deny_rules(&self) -> &[Rule] {
        &self.deny_rules
    }

    /// Authorize one call: deny rules first, then allow rules, else deny.
    /// Returns the matched rule name on allow, or the denying reason.
    pub(crate) fn decide(&self, call: &CallAttributes<'_, '_, '_>) -> Decision {
        for rule in &self.deny_rules {
            if rule.matches(call) {
                return Decision::Deny {
                    rule: rule.name.clone(),
                };
            }
        }
        for rule in &self.allow_rules {
            if rule.matches(call) {
                return Decision::Allow {
                    rule: rule.name.clone(),
                };
            }
        }
        Decision::Deny {
            rule: String::new(),
        }
    }

    /// Audit `decision` when the policy configures audit logging (A59).
    /// Runs synchronously right after the decision; never changes it.
    pub(crate) fn audit(&self, call: &CallAttributes<'_, '_, '_>, decision: &Decision) {
        let Some(audit) = self.audit.as_ref() else {
            return;
        };
        let (authorized, matched_rule) = match decision {
            Decision::Allow { rule } => (true, rule.clone()),
            Decision::Deny { rule } => (false, rule.clone()),
        };
        audit.audit(&AuditEvent {
            rpc_method: call.path.to_owned(),
            principal: call
                .peer
                .identities()
                .first()
                .copied()
                .unwrap_or_default()
                .to_owned(),
            policy_name: self.name.clone(),
            matched_rule,
            authorized,
        });
    }
}

/// Allow or deny, with the deciding rule name (empty on default-deny).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow { rule: String },
    Deny { rule: String },
}

/// Everything a rule can match against for one call.
pub(crate) struct CallAttributes<'h, 'v, 'p> {
    /// Fully qualified path, e.g. `/pkg.Service/Method`.
    pub(crate) path: &'v str,
    /// Metadata values for `key`, in wire order.
    pub(crate) headers: &'h (dyn for<'k> Fn(&'k str) -> Vec<&'v str> + 'h),
    /// Peer TLS facts for principal matching.
    pub(crate) peer: &'p PeerPrincipals,
}

/// One allow or deny rule.
#[derive(Clone, Debug)]
pub struct Rule {
    name: String,
    source: Option<Source>,
    request: Option<Request>,
}

impl Rule {
    /// Rule name, for monitoring and error messages.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn matches(&self, call: &CallAttributes<'_, '_, '_>) -> bool {
        let source_ok = self.source.as_ref().is_none_or(|s| s.matches(call.peer));
        let request_ok = self.request.as_ref().is_none_or(|r| r.matches(call));
        source_ok && request_ok
    }
}

/// Peer attributes: currently just principals.
#[derive(Clone, Debug)]
pub struct Source {
    principals: Vec<StringMatcher>,
}

impl Source {
    fn matches(&self, peer: &PeerPrincipals) -> bool {
        if !peer.is_tls {
            return false;
        }
        if !peer.has_cert {
            // TLS without a client certificate: only `""` matches.
            return self.principals.iter().any(|m| m.matches(""));
        }
        let identities = peer.identities();
        self.principals
            .iter()
            .any(|m| identities.iter().any(|id| m.matches(id)))
    }
}

/// Request attributes: paths AND headers.
#[derive(Clone, Debug)]
pub struct Request {
    paths: Vec<StringMatcher>,
    headers: Vec<HeaderMatcher>,
}

impl Request {
    fn matches(&self, call: &CallAttributes<'_, '_, '_>) -> bool {
        if !self.paths.is_empty() && !self.paths.iter().any(|m| m.matches(call.path)) {
            return false;
        }
        self.headers.iter().all(|h| {
            let joined = (call.headers)(h.key.as_str()).join(",");
            h.values.iter().any(|m| m.matches(&joined))
        })
    }
}

/// One header key plus its ORed value matchers.
#[derive(Clone, Debug)]
pub struct HeaderMatcher {
    key: String,
    values: Vec<StringMatcher>,
}

fn parse_rules(value: &serde_json::Value, field: &str) -> Result<Vec<Rule>, PolicyError> {
    let list = value
        .as_array()
        .ok_or_else(|| PolicyError::new(format!("{field:?} must be an array")))?;
    let mut rules = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        rules.push(parse_rule(item, field, index)?);
    }
    let mut names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    names.sort_unstable();
    for pair in names.windows(2) {
        if let [first, second] = pair {
            if first == second {
                return Err(PolicyError::new(format!(
                    "duplicate rule name {first:?} in {field:?}"
                )));
            }
        }
    }
    Ok(rules)
}

fn parse_rule(value: &serde_json::Value, field: &str, index: usize) -> Result<Rule, PolicyError> {
    let obj = value
        .as_object()
        .ok_or_else(|| PolicyError::new(format!("{field}[{index}] must be an object")))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "name" | "source" | "request") {
            return Err(PolicyError::new(format!(
                "{field}[{index}] has unknown field {key:?}"
            )));
        }
    }
    let name = obj
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| PolicyError::new(format!("{field}[{index}] requires a string \"name\"")))?;
    let source = obj
        .get("source")
        .map(|v| parse_source(v, field, index))
        .transpose()?
        .filter(|s: &Source| !s.principals.is_empty());
    let request = obj
        .get("request")
        .map(|v| parse_request(v, field, index))
        .transpose()?
        .filter(|r: &Request| !r.paths.is_empty() || !r.headers.is_empty());
    Ok(Rule {
        name: name.to_owned(),
        source,
        request,
    })
}

fn parse_source(
    value: &serde_json::Value,
    field: &str,
    index: usize,
) -> Result<Source, PolicyError> {
    let obj = value
        .as_object()
        .ok_or_else(|| PolicyError::new(format!("{field}[{index}].source must be an object")))?;
    for key in obj.keys() {
        if key != "principals" {
            return Err(PolicyError::new(format!(
                "{field}[{index}].source has unknown field {key:?}"
            )));
        }
    }
    let principals = obj
        .get("principals")
        .map(|v| parse_matchers(v, &format!("{field}[{index}].source.principals")))
        .transpose()?
        .unwrap_or_default();
    Ok(Source { principals })
}

fn parse_request(
    value: &serde_json::Value,
    field: &str,
    index: usize,
) -> Result<Request, PolicyError> {
    let obj = value
        .as_object()
        .ok_or_else(|| PolicyError::new(format!("{field}[{index}].request must be an object")))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "paths" | "headers") {
            return Err(PolicyError::new(format!(
                "{field}[{index}].request has unknown field {key:?}"
            )));
        }
    }
    let paths = obj
        .get("paths")
        .map(|v| parse_matchers(v, &format!("{field}[{index}].request.paths")))
        .transpose()?
        .unwrap_or_default();
    let headers = obj
        .get("headers")
        .map(|v| parse_headers(v, field, index))
        .transpose()?
        .unwrap_or_default();
    Ok(Request { paths, headers })
}

fn parse_matchers(
    value: &serde_json::Value,
    where_: &str,
) -> Result<Vec<StringMatcher>, PolicyError> {
    let list = value
        .as_array()
        .ok_or_else(|| PolicyError::new(format!("{where_} must be an array")))?;
    list.iter()
        .enumerate()
        .map(|(i, v)| {
            let s = v
                .as_str()
                .ok_or_else(|| PolicyError::new(format!("{where_}[{i}] must be a string")))?;
            StringMatcher::parse(s).map_err(|e| PolicyError::new(format!("{where_}[{i}]: {e}")))
        })
        .collect()
}

fn parse_headers(
    value: &serde_json::Value,
    field: &str,
    index: usize,
) -> Result<Vec<HeaderMatcher>, PolicyError> {
    let list = value.as_array().ok_or_else(|| {
        PolicyError::new(format!("{field}[{index}].request.headers must be an array"))
    })?;
    list.iter()
        .enumerate()
        .map(|(i, v)| {
            let obj = v.as_object().ok_or_else(|| {
                PolicyError::new(format!(
                    "{field}[{index}].request.headers[{i}] must be an object"
                ))
            })?;
            for key in obj.keys() {
                if !matches!(key.as_str(), "key" | "values") {
                    return Err(PolicyError::new(format!(
                        "{field}[{index}].request.headers[{i}] has unknown field {key:?}"
                    )));
                }
            }
            let key = obj
                .get("key")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    PolicyError::new(format!(
                        "{field}[{index}].request.headers[{i}] requires a string \"key\""
                    ))
                })?;
            check_header_key(key, field, index, i)?;
            let values = obj
                .get("values")
                .ok_or_else(|| {
                    PolicyError::new(format!(
                        "{field}[{index}].request.headers[{i}] requires \"values\""
                    ))
                })
                .and_then(|v| {
                    parse_matchers(v, &format!("{field}[{index}].request.headers[{i}].values"))
                })?;
            Ok(HeaderMatcher {
                key: key.to_ascii_lowercase(),
                values,
            })
        })
        .collect()
}

/// Reject keys A43 does not support: Host, hop-by-hop headers, `:`
/// pseudo-headers, and `grpc-` headers.
fn check_header_key(key: &str, field: &str, index: usize, i: usize) -> Result<(), PolicyError> {
    let lower = key.to_ascii_lowercase();
    let bad = lower.is_empty()
        || lower == "host"
        || lower.starts_with(':')
        || lower.starts_with("grpc-")
        || matches!(
            lower.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "trailers"
                | "transfer-encoding"
                | "upgrade"
        );
    if bad {
        return Err(PolicyError::new(format!(
            "{field}[{index}].request.headers[{i}] has unsupported key {key:?}"
        )));
    }
    Ok(())
}

/// Policy parse failure. The policy is unusable; fail closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyError(String);

impl PolicyError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn io(path: std::path::PathBuf, err: std::io::Error) -> Self {
        Self(format!("cannot read {}: {err}", path.display()))
    }
}

impl core::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "authorization policy error: {}", self.0)
    }
}

impl std::error::Error for PolicyError {}

/// The gRFC A43 example policy, used by tests and docs.
#[cfg(any(test, doc))]
pub(crate) const EXAMPLE_POLICY: &str = r#"{
	"name": "example-policy",
	"allow_rules": [
		{
			"name": "admin-access",
			"source": {
				"principals": [
					"spiffe://foo.com/sa/admin1",
					"spiffe://foo.com/sa/admin2"
				]
			},
			"request": {
				"paths": ["/pkg.service/*"]
			}
		},
		{
			"name": "dev-access",
			"source": {
				"principals": ["*", ""]
			},
			"request": {
				"paths": [
					"/pkg.service/foo",
					"/pkg.service/bar"
				],
				"headers": [
					{
						"key": "dev-path",
						"values": ["/dev/path/*"]
					}
				]
			}
		}
	],
	"deny_rules": [
		{
			"name": "deny-access",
			"request": {
				"paths": [
					"*/secret"
				]
			}
		}
	]
}"#;

#[cfg(test)]
mod tests {
    use super::{CallAttributes, Decision, EXAMPLE_POLICY, PeerPrincipals, Policy};

    fn call<'h, 'v, 'p>(
        path: &'v str,
        headers: &'h (dyn for<'k> Fn(&'k str) -> Vec<&'v str> + 'h),
        peer: &'p PeerPrincipals,
    ) -> CallAttributes<'h, 'v, 'p> {
        CallAttributes {
            path,
            headers,
            peer,
        }
    }

    fn empty_headers() -> impl for<'k> Fn(&'k str) -> Vec<&'static str> {
        |_: &str| Vec::new()
    }

    fn cert_peer(uris: &[&str], dns: &[&str]) -> PeerPrincipals {
        PeerPrincipals {
            uri_sans: uris.iter().map(ToString::to_string).collect(),
            dns_sans: dns.iter().map(ToString::to_string).collect(),
            subject: None,
            is_tls: true,
            has_cert: true,
        }
    }

    #[test]
    fn example_policy_decisions() {
        let policy = Policy::from_json(EXAMPLE_POLICY).expect("example parses");
        assert_eq!(policy.name(), "example-policy");
        let none = empty_headers();

        let admin = cert_peer(&["spiffe://foo.com/sa/admin1"], &[]);
        // Admin reaches pkg.service methods...
        let c = call("/pkg.service/anything", &none, &admin);
        assert!(matches!(policy.decide(&c), Decision::Allow { .. }));
        // ...but deny rules win over allow rules.
        let c = call("/pkg.service/secret", &none, &admin);
        assert!(matches!(
            policy.decide(&c),
            Decision::Deny { rule } if rule == "deny-access"
        ));

        // Dev without the header is denied (headers are ANDed).
        let dev = cert_peer(&[], &["dev.example.com"]);
        let c = call("/pkg.service/foo", &none, &dev);
        assert!(matches!(policy.decide(&c), Decision::Deny { .. }));
        // Dev with the header is allowed ("*" presence matches the cert).
        let with_header = |key: &str| {
            if key == "dev-path" {
                vec!["/dev/path/x"]
            } else {
                Vec::new()
            }
        };
        let c = call("/pkg.service/foo", &with_header, &dev);
        assert!(matches!(policy.decide(&c), Decision::Allow { .. }));

        // Plaintext matches no principal, so dev-access fails there.
        let plain = PeerPrincipals::plaintext();
        let c = call("/pkg.service/foo", &with_header, &plain);
        assert!(matches!(policy.decide(&c), Decision::Deny { .. }));

        // TLS without a client cert matches the "" principal.
        let nocert = PeerPrincipals::tls_no_cert();
        let c = call("/pkg.service/foo", &with_header, &nocert);
        assert!(matches!(policy.decide(&c), Decision::Allow { .. }));
    }

    #[test]
    fn empty_rule_matches_everything() {
        let policy =
            Policy::from_json(r#"{"name":"p","allow_rules":[{"name":"all"}]}"#).expect("parses");
        let none = empty_headers();
        let plain = PeerPrincipals::plaintext();
        let c = call("/anything/goes", &none, &plain);
        assert!(matches!(policy.decide(&c), Decision::Allow { .. }));
    }

    #[test]
    fn rejects_bad_policies() {
        // Missing allow_rules.
        assert!(Policy::from_json(r#"{"name":"p"}"#).is_err());
        // Missing name.
        assert!(Policy::from_json(r#"{"allow_rules":[]}"#).is_err());
        // Unknown top-level field fails closed.
        assert!(Policy::from_json(r#"{"name":"p","allow_rules":[],"future":{}}"#).is_err());
        // Unknown rule field fails closed.
        assert!(
            Policy::from_json(r#"{"name":"p","allow_rules":[{"name":"r","destination":{}}]}"#)
                .is_err()
        );
        // Rule without a name.
        assert!(Policy::from_json(r#"{"name":"p","allow_rules":[{}]}"#).is_err());
        // Duplicate rule names.
        assert!(
            Policy::from_json(r#"{"name":"p","allow_rules":[{"name":"d"},{"name":"d"}]}"#).is_err()
        );
        // Bad pattern.
        assert!(
            Policy::from_json(
                r#"{"name":"p","allow_rules":[{"name":"r","request":{"paths":["a*b"]}}]}"#
            )
            .is_err()
        );
        // Unsupported header keys.
        for key in ["grpc-foo", ":path", "host", "connection", ""] {
            let json = format!(
                r#"{{"name":"p","allow_rules":[{{"name":"r","request":{{"headers":[{{"key":{key:?},"values":["*"]}}]}}}}]}}"#
            );
            assert!(Policy::from_json(&json).is_err(), "key {key:?}");
        }
        // Header without values.
        assert!(
            Policy::from_json(
                r#"{"name":"p","allow_rules":[{"name":"r","request":{"headers":[{"key":"x"}]}}]}"#
            )
            .is_err()
        );
        // Not JSON / not an object.
        assert!(Policy::from_json("nope").is_err());
        assert!(Policy::from_json("[]").is_err());
    }
}
