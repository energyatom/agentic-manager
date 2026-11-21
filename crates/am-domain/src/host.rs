//! Host patterns (AD-48, Consistency Conventions: Host patterns): the one
//! parser and matcher of the hosts a seat may reach. A pattern is a
//! lowercase ASCII DNS host name, matched exactly, or `*.` followed by a
//! domain of at least two labels, matching any subdomain at any depth but
//! not that domain itself. A pattern admits `https` on port 443 and `http`
//! on port 80 only. Nothing else in the workspace matches hosts by hand.
//!
//! Uppercase ASCII is lowercased (DNS is case-insensitive). An
//! internationalised name is written in its `xn--` form: non-ASCII text is
//! refused, never converted.

use std::fmt;

/// A parsed host pattern: an exact host name or `*.<domain>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostPattern {
    /// The normalised pattern as written (`docs.example.org`, `*.example.org`).
    text: String,
    /// True for `*.<domain>`.
    wildcard: bool,
}

/// Why a text is not a host pattern, in words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostPatternError {
    #[error("a host pattern cannot be empty")]
    Empty,
    #[error("a host pattern has no scheme (such as https://); write the host only")]
    Scheme {
        /// The host the text seems to name, for the fix.
        host: String,
    },
    #[error("a host pattern has no path (nothing from the first /); write the host only")]
    Path { host: String },
    #[error(
        "a host pattern has no port; a pattern admits https on port 443 and http on port 80 only"
    )]
    Port { host: String },
    #[error("an IP address is not a host pattern; write the host name")]
    IpAddress,
    #[error("localhost is not a host pattern: a seat never reaches this Mac's own services")]
    Localhost,
    #[error(
        "a host pattern is ASCII only; write an internationalised name in its xn-- form (for example xn--exmple-cua.org)"
    )]
    NotAscii,
    #[error("a wildcard is allowed only as a leading `*.` followed by a domain (*.example.org)")]
    Wildcard,
    #[error(
        "a wildcard needs a domain of at least two labels after `*.` (*.example.org, not *.org)"
    )]
    WildcardTooWide,
    #[error("a host name has at least two labels joined by dots (example.org)")]
    OneLabel,
    #[error("a host name is at most 253 characters")]
    TooLong,
    #[error("a host name has an empty label (two dots in a row, or a dot at either end)")]
    EmptyLabel,
    #[error("the label '{0}' is longer than 63 characters")]
    LabelTooLong(String),
    #[error("the label '{0}' may hold only letters, digits and hyphens")]
    LabelCharacters(String),
    #[error("the label '{0}' may not start or end with a hyphen")]
    LabelHyphen(String),
}

impl HostPatternError {
    /// The host the refused text seems to name, when the text carried a
    /// scheme, a path or a port around it and that host is itself a valid
    /// pattern (for a fix such as "write the host only, e.g. x.org"); `None`
    /// otherwise, so a fix never suggests a host that is refused too.
    pub fn host_hint(&self) -> Option<&str> {
        match self {
            HostPatternError::Scheme { host }
            | HostPatternError::Path { host }
            | HostPatternError::Port { host } => {
                Some(host.as_str()).filter(|h| HostPattern::parse(h).is_ok())
            }
            _ => None,
        }
    }
}

/// The text up to the first `/`, `?`, `#` or `:` (a host candidate).
fn host_part(text: &str) -> String {
    let end = text.find(['/', '?', '#', ':']).unwrap_or(text.len());
    text[..end].to_ascii_lowercase()
}

/// True for a label the WHATWG URL parser reads as a number: decimal
/// digits, or `0x`/`0X` followed by hex digits or nothing.
fn whatwg_number(label: &str) -> bool {
    if let Some(hex) = label
        .strip_prefix("0x")
        .or_else(|| label.strip_prefix("0X"))
    {
        return hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    !label.is_empty() && label.chars().all(|c| c.is_ascii_digit())
}

/// Checks a DNS host name of at least two labels (already lowercase ASCII).
fn check_name(name: &str) -> Result<(), HostPatternError> {
    if name.len() > 253 {
        return Err(HostPatternError::TooLong);
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.iter().any(|l| l.is_empty()) {
        return Err(HostPatternError::EmptyLabel);
    }
    for label in &labels {
        if label.len() > 63 {
            return Err(HostPatternError::LabelTooLong((*label).to_string()));
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(HostPatternError::LabelCharacters((*label).to_string()));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(HostPatternError::LabelHyphen((*label).to_string()));
        }
    }
    if labels.len() < 2 {
        return Err(HostPatternError::OneLabel);
    }
    // A last label that is a WHATWG URL number (decimal digits, or `0x`
    // followed by hex digits or nothing) makes the URL standard read the
    // whole name as an IPv4 address (`127.0x1`, `1.2.3.0x4`); no top-level
    // domain looks like that.
    if labels.last().is_some_and(|l| whatwg_number(l)) {
        return Err(HostPatternError::IpAddress);
    }
    if labels.last() == Some(&"localhost") {
        return Err(HostPatternError::Localhost);
    }
    Ok(())
}

impl HostPattern {
    /// Parses and normalises `text` (uppercase ASCII is lowercased).
    pub fn parse(text: &str) -> Result<HostPattern, HostPatternError> {
        if text.is_empty() {
            return Err(HostPatternError::Empty);
        }
        if let Some((_, rest)) = text.split_once("://") {
            return Err(HostPatternError::Scheme {
                host: host_part(rest),
            });
        }
        if !text.is_ascii() {
            return Err(HostPatternError::NotAscii);
        }
        if text.starts_with('[') || text.parse::<std::net::Ipv6Addr>().is_ok() {
            return Err(HostPatternError::IpAddress);
        }
        if let Some(slash) = text.find('/') {
            return Err(HostPatternError::Path {
                host: host_part(&text[..slash]),
            });
        }
        if let Some((host, port)) = text.rsplit_once(':') {
            if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) && !host.contains(':') {
                return Err(HostPatternError::Port {
                    host: host.to_ascii_lowercase(),
                });
            }
            // Any other `:` is an IPv6 address or worse.
            return Err(HostPatternError::IpAddress);
        }
        let lower = text.to_ascii_lowercase();
        if lower.parse::<std::net::Ipv4Addr>().is_ok() {
            return Err(HostPatternError::IpAddress);
        }
        if lower == "localhost" {
            return Err(HostPatternError::Localhost);
        }
        let (wildcard, name) = match lower.strip_prefix("*.") {
            Some(domain) => (true, domain),
            None => (false, lower.as_str()),
        };
        if name.contains('*') {
            return Err(HostPatternError::Wildcard);
        }
        // `*.com` or `*.`: fewer than two labels after the wildcard.
        if wildcard && !name.contains('.') {
            return Err(HostPatternError::WildcardTooWide);
        }
        check_name(name)?;
        Ok(HostPattern {
            text: lower,
            wildcard,
        })
    }

    /// The normalised pattern.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// True for `*.<domain>`.
    pub fn is_wildcard(&self) -> bool {
        self.wildcard
    }

    /// The domain a wildcard stands over, or the exact host.
    fn name(&self) -> &str {
        if self.wildcard {
            &self.text[2..]
        } else {
            &self.text
        }
    }

    /// True when the host name `host` (compared ignoring ASCII case) is the
    /// exact host, or a subdomain at any depth of a wildcard's domain (never
    /// the domain itself).
    pub fn matches(&self, host: &str) -> bool {
        let name = self.name();
        if !self.wildcard {
            return host.eq_ignore_ascii_case(name);
        }
        // `<labels>.<domain>`, each label of the prefix non-empty.
        if host.len() <= name.len() + 1 || !host.is_char_boundary(host.len() - name.len() - 1) {
            return false;
        }
        let (prefix, rest) = host.split_at(host.len() - name.len() - 1);
        let Some(domain) = rest.strip_prefix('.') else {
            return false;
        };
        domain.eq_ignore_ascii_case(name) && prefix.split('.').all(|l| !l.is_empty())
    }

    /// True when a request with `scheme` to `host` on `port` is admitted:
    /// `https` on 443 or `http` on 80, to a host this pattern matches.
    pub fn admits(&self, scheme: &str, host: &str, port: u16) -> bool {
        let scheme_ok = (scheme.eq_ignore_ascii_case("https") && port == 443)
            || (scheme.eq_ignore_ascii_case("http") && port == 80);
        scheme_ok && self.matches(host)
    }

    /// True when every host `other` matches is matched by this pattern: an
    /// exact pattern covers only itself; `*.d` covers every pattern under
    /// `d` (and itself), never `d`.
    pub fn covers(&self, other: &HostPattern) -> bool {
        if !self.wildcard {
            return self == other;
        }
        if other.wildcard {
            let (mine, theirs) = (self.name(), other.name());
            theirs == mine || self.matches(theirs)
        } else {
            self.matches(other.name())
        }
    }
}

impl fmt::Display for HostPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
