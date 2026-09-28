//! Exact / prefix / suffix / presence string matchers (gRFC A43).
//!
//! Every matchable policy field (principals, paths, header values) uses the
//! same four match kinds: exact (`"abc"`), prefix (`"abc*"`), suffix
//! (`"*abc"`), and presence (`"*"` matches any non-empty value).

/// One A43 string match rule, compiled from its policy spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StringMatcher {
    /// Matches only `value`.
    Exact(String),
    /// Matches `value` and anything starting with it (`"abc*"`).
    Prefix(String),
    /// Matches `value` and anything ending with it (`"*abc"`).
    Suffix(String),
    /// Matches any non-empty value (`"*"`).
    Presence,
}

impl StringMatcher {
    /// Compile the policy spelling. A `*` anywhere but alone, first, or
    /// last is invalid, as is an empty exact match... except `""`, which is
    /// a valid exact match (unauthenticated-TLS principal).
    pub(crate) fn parse(pattern: &str) -> Result<Self, String> {
        if pattern == "*" {
            return Ok(Self::Presence);
        }
        let starts = pattern.as_bytes().first() == Some(&b'*');
        let ends = pattern.as_bytes().last() == Some(&b'*');
        let inner_stars = pattern
            .trim_start_matches('*')
            .trim_end_matches('*')
            .contains('*');
        if inner_stars || (starts && ends) {
            return Err(format!("invalid match pattern {pattern:?}"));
        }
        if starts {
            Ok(Self::Suffix(pattern[1..].to_owned()))
        } else if ends {
            Ok(Self::Prefix(pattern[..pattern.len() - 1].to_owned()))
        } else {
            Ok(Self::Exact(pattern.to_owned()))
        }
    }

    /// Whether `value` matches.
    pub(crate) fn matches(&self, value: &str) -> bool {
        match self {
            Self::Exact(want) => value == want,
            Self::Prefix(prefix) => value.starts_with(prefix.as_str()),
            Self::Suffix(suffix) => value.ends_with(suffix.as_str()),
            Self::Presence => !value.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StringMatcher;

    #[test]
    fn match_kinds() {
        assert!(StringMatcher::parse("abc").expect("exact").matches("abc"));
        assert!(!StringMatcher::parse("abc").expect("exact").matches("abcd"));
        assert!(StringMatcher::parse("abc*").expect("prefix").matches("abc"));
        assert!(
            StringMatcher::parse("abc*")
                .expect("prefix")
                .matches("abcd")
        );
        assert!(
            !StringMatcher::parse("abc*")
                .expect("prefix")
                .matches("xabc")
        );
        assert!(StringMatcher::parse("*abc").expect("suffix").matches("abc"));
        assert!(
            StringMatcher::parse("*abc")
                .expect("suffix")
                .matches("xabc")
        );
        assert!(
            !StringMatcher::parse("*abc")
                .expect("suffix")
                .matches("abcx")
        );
        assert!(StringMatcher::parse("*").expect("presence").matches("x"));
        assert!(!StringMatcher::parse("*").expect("presence").matches(""));
        // Empty exact match is meaningful (TLS without client certificate).
        assert!(StringMatcher::parse("").expect("empty").matches(""));
        assert!(!StringMatcher::parse("").expect("empty").matches("x"));
    }

    #[test]
    fn rejects_embedded_stars() {
        assert!(StringMatcher::parse("a*b").is_err());
        assert!(StringMatcher::parse("*a*").is_err());
        assert!(StringMatcher::parse("**").is_err());
    }
}
