//! `GRPC_BINARY_LOG_FILTER` parsing and per-method resolution (gRFC A16).
//!
//! A filter is a comma-separated list of patterns. The optional global `*`
//! pattern comes first; after it come per-service (`Svc/*`), per-method
//! (`Svc/Method`), and exclusion (`-Svc/Method`) patterns. Each keeps an
//! optional `{h[:N][;m[:M]]}` config: `h` logs headers, `m` logs messages,
//! and a `:N` suffix caps each header value or message payload at `N` bytes.
//! A class without a letter is not logged at all; a letter without a number
//! logs the class in full.
//!
//! Resolution picks the most exact match: an exact method rule beats a
//! service rule, which beats the global rule. An exclusion disables logging
//! for that method.

use std::collections::{HashMap, HashSet};
use std::fmt;

/// How much of one payload class to log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// Log the class in full.
    Full,
    /// Log at most this many bytes per header value or message payload.
    Bytes(u64),
}

/// The `{h;m}` config attached to one filter pattern.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    /// Header logging for this pattern, or `None` to skip header events.
    pub headers: Option<Cap>,
    /// Message logging for this pattern, or `None` to skip message events.
    pub messages: Option<Cap>,
}

impl Rule {
    /// Log everything in full (`*` with no config).
    #[must_use]
    pub fn full() -> Self {
        Self {
            headers: Some(Cap::Full),
            messages: Some(Cap::Full),
        }
    }
}

/// A parsed `GRPC_BINARY_LOG_FILTER` value.
#[derive(Clone, Debug, Default)]
pub struct BinaryLogFilter {
    global: Option<Rule>,
    services: HashMap<String, Rule>,
    methods: HashMap<String, Rule>,
    excluded: HashSet<String>,
}

/// Why a filter string was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError {
    message: String,
}

impl FilterError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid binary log filter: {}", self.message)
    }
}

impl std::error::Error for FilterError {}

impl BinaryLogFilter {
    /// Parse a `GRPC_BINARY_LOG_FILTER` value.
    ///
    /// An empty string is valid and logs nothing. Anything that does not
    /// match the A16 ABNF — a misplaced global, `*/method`, `-Svc/*`, a
    /// duplicate rule, a bad `{h;m}` config, or a non-numeric cap — is an
    /// error, and the caller must refuse to start logging.
    ///
    /// ```
    /// use pbrs_grpc::binlog::BinaryLogFilter;
    ///
    /// let filter = BinaryLogFilter::parse("*,Foo/Bar{m:256}").expect("filter");
    /// assert!(filter.resolve("/Foo/Bar").is_some());
    /// assert!(filter.resolve("/Foo/Baz").is_some());
    /// assert!(BinaryLogFilter::parse("*/Bar").is_err());
    /// ```
    pub fn parse(filter: &str) -> Result<Self, FilterError> {
        let mut parsed = Self::default();
        if filter.trim().is_empty() {
            return Ok(parsed);
        }
        let mut seen_global = false;
        for (index, raw) in filter.split(',').enumerate() {
            let pattern = raw.trim();
            if pattern.is_empty() {
                return Err(FilterError::new("empty pattern"));
            }
            let (head, config) = split_config(pattern)?;
            if head == "*" {
                if index != 0 || seen_global {
                    return Err(FilterError::new(
                        "the global `*` pattern must come first and only once",
                    ));
                }
                seen_global = true;
                parsed.global = Some(config.unwrap_or_else(Rule::full));
                continue;
            }
            if let Some(method) = head.strip_prefix('-') {
                if method.contains('*') || !is_full_method(method) {
                    return Err(FilterError::new(format!(
                        "exclusions name one exact method, got `{head}`"
                    )));
                }
                if !parsed.excluded.insert(method.to_owned()) || parsed.methods.contains_key(method)
                {
                    return Err(FilterError::new(format!("duplicate rule for `{method}`")));
                }
                continue;
            }
            if head.contains('*') {
                let Some(service) = head.strip_suffix("/*") else {
                    return Err(FilterError::new(format!(
                        "only `<service>/*` wildcards are supported, got `{head}`"
                    )));
                };
                if service.is_empty() || service.contains('/') {
                    return Err(FilterError::new(format!("bad service wildcard `{head}`")));
                }
                if parsed
                    .services
                    .insert(service.to_owned(), config.unwrap_or_else(Rule::full))
                    .is_some()
                {
                    return Err(FilterError::new(format!("duplicate rule for `{head}`")));
                }
                continue;
            }
            if !is_full_method(head) {
                return Err(FilterError::new(format!(
                    "expected `<service>/<method>`, got `{head}`"
                )));
            }
            if parsed.excluded.contains(head)
                || parsed
                    .methods
                    .insert(head.to_owned(), config.unwrap_or_else(Rule::full))
                    .is_some()
            {
                return Err(FilterError::new(format!("duplicate rule for `{head}`")));
            }
        }
        Ok(parsed)
    }

    /// The rule in effect for `path` (`/<service>/<method>`), if it is logged.
    ///
    /// An exact method rule wins over a service rule, which wins over the
    /// global rule. Excluded methods and methods matching nothing return
    /// `None`.
    #[must_use]
    pub fn resolve(&self, path: &str) -> Option<Rule> {
        let name = path.strip_prefix('/').unwrap_or(path);
        if self.excluded.contains(name) {
            return None;
        }
        if let Some(rule) = self.methods.get(name) {
            return Some(*rule);
        }
        if let Some((service, _)) = name.rsplit_once('/') {
            if let Some(rule) = self.services.get(service) {
                return Some(*rule);
            }
        }
        self.global
    }
}

/// Split `Svc/Method{h;m}` into its head and optional config.
fn split_config(pattern: &str) -> Result<(&str, Option<Rule>), FilterError> {
    let Some(brace) = pattern.find('{') else {
        return Ok((pattern, None));
    };
    if !pattern.ends_with('}') {
        return Err(FilterError::new(format!("unbalanced `{{` in `{pattern}`")));
    }
    let head = &pattern[..brace];
    let config = parse_config(&pattern[brace + 1..pattern.len() - 1], pattern)?;
    Ok((head, Some(config)))
}

/// Parse the inside of `{...}`: `m[:N]` or `h[:N][;m[:M]]`.
fn parse_config(config: &str, pattern: &str) -> Result<Rule, FilterError> {
    let malformed = || FilterError::new(format!("bad config in `{pattern}`"));
    let (header_part, message_part) = match config.split_once(';') {
        None => {
            if config.starts_with('h') {
                (Some(config), None)
            } else if config.starts_with('m') {
                (None, Some(config))
            } else {
                return Err(malformed());
            }
        }
        Some((h, m)) => {
            if !h.starts_with('h') || !m.starts_with('m') || m.contains(';') {
                return Err(malformed());
            }
            (Some(h), Some(m))
        }
    };
    let mut rule = Rule {
        headers: None,
        messages: None,
    };
    if let Some(part) = header_part {
        rule.headers = Some(parse_cap(part, 'h').ok_or_else(malformed)?);
    }
    if let Some(part) = message_part {
        rule.messages = Some(parse_cap(part, 'm').ok_or_else(malformed)?);
    }
    Ok(rule)
}

/// Parse one `h[:N]` / `m[:N]` clause. `None` rejects the clause.
fn parse_cap(clause: &str, letter: char) -> Option<Cap> {
    let rest = clause.strip_prefix(letter)?;
    if rest.is_empty() {
        return Some(Cap::Full);
    }
    let digits = rest.strip_prefix(':')?;
    if digits.is_empty() {
        // `h:` matches `[":" *DIGIT]` with zero digits; read it as Full.
        return Some(Cap::Full);
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok().map(Cap::Bytes)
}

/// `true` for `Svc/Method` with neither side empty, wildcarded, or nested.
fn is_full_method(head: &str) -> bool {
    if head.contains('*') {
        return false;
    }
    match head.split_once('/') {
        Some((service, method)) => {
            !service.is_empty() && !method.is_empty() && !method.contains('/')
        }
        None => false,
    }
}
