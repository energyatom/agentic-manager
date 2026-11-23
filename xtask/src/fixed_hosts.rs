//! `fixed-hosts` (story 2.2, AD-12 "No telemetry"): every fixed host the
//! shipped source names must be on one named list, kept here with a reason
//! for each. The scan reads every string literal of `crates/*/src` (a
//! crate's `tests/` and its `#[cfg(test)]` items are left out; comments are
//! not code; story 3.3 reads every `.rs` of a crate but its `tests/`,
//! `benches/` and `examples/`, reads C strings as literals, and fails
//! closed on a literal that never ends) and finds each host by its URL (`scheme://host[:port]`,
//! anywhere in a literal) or by its bare name (a literal that is only a DNS
//! name ending in a common top-level domain, or an IPv4 address, with an
//! optional `:port`; a name inside a sentence, such as an example in a
//! refusal's words, is not a destination). A host not on the list fails,
//! naming the host and the file. A port written as a placeholder
//! (`http://127.0.0.1:{port}`) is read as no fixed port.
//!
//! The list has no release host yet (epic 11 adds it) and no skill-import
//! hosts (epic 9 story 16 adds them, as AD-12 says).

use std::path::Path;

use crate::Failure;
use crate::forbid::{string_literals, unterminated_finding};
use crate::shipped::{in_ranges, sources, test_ranges};

/// The one named list: host, port, and why the app may name it. A host
/// with a fixed port (written, or its URL scheme's default) must match an
/// entry's port; an entry with no port admits the host only with no fixed
/// port (a placeholder such as `{port}`).
const LISTED: &[(&str, Option<u16>, &str)] = &[
    (
        "api.anthropic.com",
        Some(443),
        "the Anthropic provider's endpoint, pinned to an anthropic Account (AD-31, AD-48)",
    ),
    (
        "api.openai.com",
        Some(443),
        "the OpenAI provider's endpoint, pinned to an openai Account (AD-31, AD-48)",
    ),
    (
        "127.0.0.1",
        Some(1234),
        "LM Studio on this Mac, the reserved local Account local-lmstudio (AD-31)",
    ),
    (
        "127.0.0.1",
        Some(11434),
        "Ollama on this Mac, the reserved local Account local-ollama (AD-31)",
    ),
    (
        "localhost",
        None,
        "the app's own loopback server: `agentic serve`'s Host, Origin and CSP checks and the base URL forms name it with its port; the egress client never reaches it (no host pattern admits it, no endpoint is it)",
    ),
    (
        "json-schema.org",
        Some(443),
        "the JSON Schema reference named in the words of a schema fix (am-config); never contacted",
    ),
];

/// Top-level domains a bare name is read as a host by: the common generic
/// ones and every two-letter country code but those that are common file
/// extensions in this repository (`rs`, `md`, `py`, `sh`), so `lib.rs` or
/// `notes.md` is not a host.
const TLDS: &[&str] = &[
    "com",
    "net",
    "org",
    "io",
    "ai",
    "dev",
    "app",
    "co",
    "cloud",
    "xyz",
    "site",
    "online",
    "tech",
    "info",
    "biz",
    "gov",
    "edu",
    "mil",
    "int",
    "me",
    "tv",
    "example",
    "test",
    "invalid",
    "localhost",
    "ac",
    "ad",
    "ae",
    "af",
    "ag",
    "al",
    "am",
    "ao",
    "aq",
    "ar",
    "as",
    "at",
    "au",
    "aw",
    "ax",
    "az",
    "ba",
    "bb",
    "bd",
    "be",
    "bf",
    "bg",
    "bh",
    "bi",
    "bj",
    "bm",
    "bn",
    "bo",
    "br",
    "bs",
    "bt",
    "bw",
    "by",
    "bz",
    "ca",
    "cc",
    "cd",
    "cf",
    "cg",
    "ch",
    "ci",
    "ck",
    "cl",
    "cm",
    "cn",
    "cr",
    "cu",
    "cv",
    "cw",
    "cx",
    "cy",
    "cz",
    "de",
    "dj",
    "dk",
    "dm",
    "do",
    "dz",
    "ec",
    "ee",
    "eg",
    "er",
    "es",
    "et",
    "eu",
    "fi",
    "fj",
    "fk",
    "fm",
    "fo",
    "fr",
    "ga",
    "gd",
    "ge",
    "gf",
    "gg",
    "gh",
    "gi",
    "gl",
    "gm",
    "gn",
    "gp",
    "gq",
    "gr",
    "gs",
    "gt",
    "gu",
    "gw",
    "gy",
    "hk",
    "hm",
    "hn",
    "hr",
    "ht",
    "hu",
    "id",
    "ie",
    "il",
    "im",
    "in",
    "iq",
    "ir",
    "is",
    "it",
    "je",
    "jm",
    "jo",
    "jp",
    "ke",
    "kg",
    "kh",
    "ki",
    "km",
    "kn",
    "kp",
    "kr",
    "kw",
    "ky",
    "kz",
    "la",
    "lb",
    "lc",
    "li",
    "lk",
    "lr",
    "ls",
    "lt",
    "lu",
    "lv",
    "ly",
    "ma",
    "mc",
    "mg",
    "mh",
    "mk",
    "ml",
    "mm",
    "mn",
    "mo",
    "mp",
    "mq",
    "mr",
    "ms",
    "mt",
    "mu",
    "mv",
    "mw",
    "mx",
    "my",
    "mz",
    "na",
    "nc",
    "ne",
    "nf",
    "ng",
    "ni",
    "nl",
    "no",
    "np",
    "nr",
    "nu",
    "nz",
    "om",
    "pa",
    "pe",
    "pf",
    "pg",
    "ph",
    "pk",
    "pl",
    "pm",
    "pn",
    "pr",
    "ps",
    "pt",
    "pw",
    "qa",
    "re",
    "ro",
    "ru",
    "rw",
    "sa",
    "sb",
    "sc",
    "sd",
    "se",
    "sg",
    "si",
    "sk",
    "sl",
    "sm",
    "sn",
    "so",
    "sr",
    "ss",
    "st",
    "sv",
    "sx",
    "sy",
    "sz",
    "tc",
    "td",
    "tf",
    "tg",
    "th",
    "tj",
    "tk",
    "tl",
    "tm",
    "tn",
    "to",
    "tr",
    "tt",
    "tw",
    "tz",
    "ua",
    "ug",
    "uk",
    "us",
    "uy",
    "uz",
    "va",
    "vc",
    "ve",
    "vg",
    "vi",
    "vn",
    "vu",
    "wf",
    "ws",
    "ye",
    "yt",
    "za",
    "zm",
    "zw",
];

/// One host a literal names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    host: String,
    /// An explicit port, or the URL scheme's default; `None` for a bare
    /// name without one, or a port written as a placeholder.
    port: Option<u16>,
    /// The port was written out (`host:port`).
    explicit: bool,
}

impl Found {
    fn shown(&self) -> String {
        match (self.explicit, self.port) {
            (true, Some(p)) => format!("{}:{p}", self.host),
            _ => self.host.clone(),
        }
    }

    fn listed(&self) -> bool {
        LISTED.iter().any(|(host, port, _)| {
            *host == self.host
                && match (self.port, port) {
                    (None, _) => true,
                    (Some(found), Some(listed)) => found == *listed,
                    (Some(_), None) => false,
                }
        })
    }
}

pub fn run(root: &Path) -> Result<String, Failure> {
    let mut findings = Vec::new();
    let mut scanned = 0usize;
    for source in sources(root) {
        scanned += 1;
        // A literal that never ends hides the rest: fail closed (review P4).
        if let Some(finding) = unterminated_finding(&source.name, &source.text) {
            findings.push(finding);
            continue;
        }
        let tests = test_ranges(&source.text);
        for (line, text) in string_literals(&source.text) {
            if in_ranges(&tests, line) {
                continue;
            }
            for found in hosts_in(&text) {
                if !found.listed() {
                    findings.push(format!(
                        "{}:{line}: the fixed host {} is not on the list of hosts the app may name (AD-12: no telemetry); a provider or service endpoint belongs in config, or on this list in xtask/src/fixed_hosts.rs with its reason",
                        source.name,
                        found.shown()
                    ));
                }
            }
        }
    }
    if !findings.is_empty() {
        return Err(Failure::Findings(findings));
    }
    let list: Vec<String> = LISTED
        .iter()
        .map(|(host, port, why)| match port {
            Some(p) if *p != 443 => format!("{host}:{p} ({why})"),
            _ => format!("{host} ({why})"),
        })
        .collect();
    Ok(format!(
        "{scanned} files clean; the fixed hosts the app may name: {}",
        list.join("; ")
    ))
}

/// Every host `text` names, by URL or by bare name.
fn hosts_in(text: &str) -> Vec<Found> {
    let mut found = Vec::new();
    let mut rest_from = 0;
    let lower = text.to_ascii_lowercase();
    // URLs first; their authority is then blanked for the bare-name pass.
    let mut blanked: Vec<char> = lower.chars().collect();
    while let Some(pos) = lower[rest_from..].find("://") {
        let at = rest_from + pos;
        // A scheme as written, or a format placeholder (`{scheme}://`,
        // `{}://`), which names no default port but still a host.
        let scheme = if lower[..at].ends_with('}') && lower[..at].contains('{') {
            "{placeholder}"
        } else {
            let scheme_start = lower[..at]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
                .map_or(0, |i| i + 1);
            &lower[scheme_start..at]
        };
        let auth_start = at + 3;
        let auth_end = lower[auth_start..]
            .find(|c: char| !is_authority_char(c))
            .map_or(lower.len(), |i| auth_start + i);
        let authority = &lower[auth_start..auth_end];
        let hostport = authority.rsplit('@').next().unwrap_or("");
        if !scheme.is_empty() && !hostport.is_empty() {
            let (host, port_text) = split_port(hostport);
            if is_host(host) {
                let explicit = port_text
                    .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty());
                let port = match port_text {
                    Some(p) if explicit => p.parse().ok(),
                    Some(_) => None,
                    None => default_port(scheme),
                };
                found.push(Found {
                    host: host.to_string(),
                    port,
                    explicit,
                });
            }
        }
        // Char indices equal byte indices here only for ASCII; blank by chars.
        let start_char = lower[..auth_start].chars().count();
        let end_char = lower[..auth_end].chars().count();
        for c in blanked.iter_mut().take(end_char).skip(start_char) {
            *c = ' ';
        }
        rest_from = auth_end.max(at + 3);
    }
    let blanked: String = blanked.into_iter().collect();
    found.extend(bare_name(blanked.trim()));
    found
}

/// The characters of a URL's authority, placeholders (`{port}`, `<host>`,
/// `…`) included so that they spoil the host instead of ending it.
fn is_authority_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '.' | '-' | ':' | '@' | '[' | ']' | '%' | '_' | '{' | '}' | '<' | '>' | '…' | '*'
        )
}

/// A host as written: a DNS name or IPv4 address (letters, digits, dots
/// and hyphens, with a letter or digit), or an IPv6 address.
fn is_host(host: &str) -> bool {
    if host.contains(':') {
        return host.parse::<std::net::Ipv6Addr>().is_ok();
    }
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && host.chars().any(|c| c.is_ascii_alphanumeric())
}

/// `host[:port]` split; the port text as written (possibly a placeholder).
fn split_port(hostport: &str) -> (&str, Option<&str>) {
    if let Some(v6) = hostport.strip_prefix('[') {
        return match v6.split_once(']') {
            Some((host, rest)) => (host, rest.strip_prefix(':')),
            None => (v6, None),
        };
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (hostport, None),
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "https" | "wss" => Some(443),
        "http" | "ws" => Some(80),
        _ => None,
    }
}

/// A literal that is only a bare name: a DNS name ending in a listed
/// top-level domain, or an IPv4 address, with an optional `:port`.
fn bare_name(text: &str) -> Option<Found> {
    // An IPv6 address, bare or in brackets with an optional `:port`.
    let (v6, v6_port) = match text.strip_prefix('[').and_then(|t| t.split_once(']')) {
        Some((inside, rest)) => (inside, rest.strip_prefix(':')),
        None => (text, None),
    };
    if v6.contains(':')
        && v6.chars().any(|c| c.is_ascii_hexdigit())
        && v6.parse::<std::net::Ipv6Addr>().is_ok()
    {
        let port = v6_port.and_then(|p| p.parse().ok());
        return Some(Found {
            host: v6.to_string(),
            port,
            explicit: port.is_some(),
        });
    }
    let (host, port_text) = match text.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (text, None),
    };
    let host = host.trim_end_matches('.');
    if !is_host(host) {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return None;
    }
    let ipv4 = host.parse::<std::net::Ipv4Addr>().is_ok();
    let named = labels
        .last()
        .is_some_and(|tld| TLDS.contains(tld) && tld.chars().all(|c| c.is_ascii_alphabetic()));
    if !(ipv4 || named) {
        return None;
    }
    let port = port_text.and_then(|p| p.parse().ok());
    Some(Found {
        host: host.to_string(),
        port,
        explicit: port.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(text: &str) -> Vec<String> {
        hosts_in(text).iter().map(Found::shown).collect()
    }

    #[test]
    fn urls_and_bare_names_are_found() {
        assert_eq!(
            shown("https://telemetry.example.com/v1/collect"),
            ["telemetry.example.com"]
        );
        assert_eq!(shown("telemetry.example.com"), ["telemetry.example.com"]);
        assert_eq!(
            shown("set it to https://<host>[/path] or http://localhost:<port>"),
            ["localhost"]
        );
        assert!(
            shown("write a host such as docs.example.org").is_empty(),
            "a name in a sentence is not a destination"
        );
        assert_eq!(shown("http://127.0.0.1:9999/v1"), ["127.0.0.1:9999"]);
        assert_eq!(shown("https://u:p@10.0.0.9/collect"), ["10.0.0.9"]);
        assert_eq!(shown("127.0.0.1:1234"), ["127.0.0.1:1234"]);
        assert_eq!(shown("http://127.0.0.1:{port}/v1"), ["127.0.0.1"]);
        assert_eq!(
            shown("{scheme}://telemetry.example.com/x"),
            ["telemetry.example.com"]
        );
        assert_eq!(shown("{}://127.0.0.1:1234/v1"), ["127.0.0.1:1234"]);
        for none in [
            "models/accounts.yaml",
            "lib.rs",
            "1.2.3",
            "v0.13.5",
            "a_b.com_x",
            "https://…",
        ] {
            assert!(shown(none).is_empty(), "{none}: {:?}", shown(none));
        }
    }

    #[test]
    fn the_list_admits_its_hosts_on_their_ports() {
        let listed = |t: &str| hosts_in(t).iter().all(Found::listed);
        assert!(listed("https://api.anthropic.com/v1/messages"));
        assert!(listed("api.openai.com"));
        assert!(listed("http://127.0.0.1:11434/v1"));
        assert!(listed("http://127.0.0.1:{port}"));
        assert!(!listed("https://api.anthropic.com:8443/"));
        assert!(!listed("http://127.0.0.1:9999/"));
        assert!(!listed("codeload.github.com"));
        assert!(!listed("collector.vercel.app"));
        assert!(!listed("[2001:db8::1]:443"));
        assert!(shown("::").is_empty(), "a path separator is not an address");
        assert!(listed("http://localhost:{}"));
        assert!(!listed("http://localhost:8080/"));
    }
}
