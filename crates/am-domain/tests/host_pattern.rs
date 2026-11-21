//! Story 1.14 acceptance tests (AD-48, Consistency Conventions: Host
//! patterns): `am_domain::HostPattern` is the only host parser and matcher.
//! Matrix row "Pattern table", plus the boundaries the plan lists. Written
//! before the type exists.
//!
//! The API these tests expect:
//! - `HostPattern::parse(&str) -> Result<HostPattern, HostPatternError>`;
//! - `as_str(&self) -> &str` and `Display`, both the normalised pattern;
//! - `matches(&self, host: &str) -> bool` (a host name, compared ignoring
//!   ASCII case);
//! - `admits(&self, scheme: &str, host: &str, port: u16) -> bool`:
//!   `https` on 443 and `http` on 80 only;
//! - `covers(&self, other: &HostPattern) -> bool`;
//! - `HostPatternError: Display` naming why the text was refused;
//! - `Debug + Clone + PartialEq + Eq`.
#![forbid(unsafe_code)]

use am_domain::{HostPattern, HostPatternError};

fn ok(text: &str) -> HostPattern {
    HostPattern::parse(text).unwrap_or_else(|e| panic!("{text:?} is refused: {e}"))
}

fn refused(text: &str) -> HostPatternError {
    match HostPattern::parse(text) {
        Ok(p) => panic!("{text:?} is accepted as {p}"),
        Err(e) => e,
    }
}

/// The error's words, lowercased, for the "naming why" checks.
fn why(text: &str) -> String {
    refused(text).to_string().to_lowercase()
}

// ------------------------------------------------------------ pattern table

#[test]
fn an_exact_pattern_matches_only_itself() {
    let p = ok("docs.example.org");
    assert_eq!(p.as_str(), "docs.example.org");
    assert_eq!(p.to_string(), "docs.example.org");
    assert!(p.matches("docs.example.org"));
    assert!(p.matches("DOCS.Example.org"), "DNS is case-insensitive");
    for host in [
        "example.org",
        "a.docs.example.org",
        "docs.example.org.evil.net",
        "xdocs.example.org",
        "docs.example.com",
        "",
    ] {
        assert!(!p.matches(host), "{host:?}");
    }
}

#[test]
fn a_wildcard_matches_any_subdomain_depth_but_not_the_bare_domain() {
    let p = ok("*.example.org");
    assert_eq!(p.as_str(), "*.example.org");
    for host in ["a.example.org", "a.b.example.org", "x.y.z.example.org"] {
        assert!(p.matches(host), "{host}");
    }
    for host in [
        "example.org",
        "badexample.org",
        "a.badexample.org",
        "example.org.evil.net",
        "a.example.org.evil.net",
        "org",
        ".example.org",
    ] {
        assert!(!p.matches(host), "{host:?}");
    }
}

#[test]
fn a_wildcard_covers_patterns_under_it_and_never_the_reverse() {
    let wild = ok("*.example.org");
    let docs = ok("docs.example.org");
    assert!(wild.covers(&docs));
    assert!(!docs.covers(&wild), "an exact pattern covers only itself");
    assert!(wild.covers(&ok("a.b.example.org")));
    assert!(wild.covers(&ok("*.docs.example.org")));
    assert!(wild.covers(&wild.clone()), "a pattern covers itself");
    assert!(!wild.covers(&ok("example.org")), "not the bare domain");
    assert!(!wild.covers(&ok("*.org.example.net")));
    assert!(!wild.covers(&ok("badexample.org")));
    assert!(!ok("*.docs.example.org").covers(&wild));
    assert!(!ok("*.docs.example.org").covers(&docs));
}

#[test]
fn an_exact_pattern_covers_only_itself() {
    let docs = ok("docs.example.org");
    assert!(docs.covers(&ok("docs.example.org")));
    assert!(docs.covers(&ok("DOCS.example.org")), "after lowercasing");
    assert!(!docs.covers(&ok("a.docs.example.org")));
    assert!(!docs.covers(&ok("*.docs.example.org")));
    assert!(!docs.covers(&ok("example.org")));
}

#[test]
fn uppercase_ascii_is_lowercased() {
    let p = ok("Example.ORG");
    assert_eq!(p.as_str(), "example.org");
    assert_eq!(p, ok("example.org"));
    assert_eq!(ok("*.Example.ORG").as_str(), "*.example.org");
    assert!(p.matches("example.org"));
}

#[test]
fn an_internationalised_name_is_written_in_its_xn_form() {
    let p = ok("xn--exmple-cua.org");
    assert_eq!(p.as_str(), "xn--exmple-cua.org");
    assert!(p.matches("xn--exmple-cua.org"));
}

// ------------------------------------------------------------ refused forms

#[test]
fn each_refused_form_names_why() {
    for (text, words) in [
        ("*.com", "label"),
        ("10.1.2.3", "ip"),
        ("localhost", "localhost"),
        ("LOCALHOST", "localhost"),
        ("example.org:8080", "port"),
        ("https://x.org/", "scheme"),
        ("http://x.org", "scheme"),
        ("x.org/docs", "path"),
        ("exämple.org", "ascii"),
        ("a.*.org", "wildcard"),
        ("*", "wildcard"),
        ("**.example.org", "wildcard"),
        ("*example.org", "wildcard"),
        ("docs.*", "wildcard"),
        ("[::1]", "ip"),
        ("::1", "ip"),
        ("fe80::1", "ip"),
    ] {
        let message = why(text);
        assert!(message.contains(words), "{text:?}: {message}");
    }
}

#[test]
fn malformed_names_are_refused() {
    let long_label = format!("{}.org", "a".repeat(64));
    let long_name = format!(
        "{}.org",
        std::iter::repeat_n("a".repeat(60), 5)
            .collect::<Vec<_>>()
            .join(".")
    );
    assert!(long_name.len() > 253, "{}", long_name.len());
    for text in [
        "",
        " ",
        "example",
        "-example.org",
        "example-.org",
        "exa_mple.org",
        "exa mple.org",
        "example..org",
        ".example.org",
        long_label.as_str(),
        long_name.as_str(),
        "*.",
        "*.-a.org",
    ] {
        let e = refused(text);
        assert!(!e.to_string().is_empty(), "{text:?}");
    }
}

#[test]
fn the_longest_allowed_label_and_name_are_accepted() {
    let label = format!("{}.org", "a".repeat(63));
    ok(&label);
    // Exactly 253 characters: 63 + 1 + 61 + 1 + 61 + 1 + 61 + 4.
    let name = format!(
        "{}.{}.{}.{}.org",
        "a".repeat(63),
        "b".repeat(61),
        "c".repeat(61),
        "d".repeat(61)
    );
    assert_eq!(name.len(), 253);
    ok(&name);
    ok("a-b.example.org");
    ok("1a.example.org");
}

// ------------------------------------------------------------ ports and schemes

#[test]
fn a_pattern_admits_https_on_443_and_http_on_80_only() {
    for p in [ok("docs.example.org"), ok("*.example.org")] {
        let host = "docs.example.org";
        assert!(p.admits("https", host, 443), "{p}");
        assert!(p.admits("http", host, 80), "{p}");
        assert!(!p.admits("https", host, 8443), "{p}");
        assert!(!p.admits("http", host, 8080), "{p}");
        assert!(!p.admits("http", host, 443), "{p}");
        assert!(!p.admits("https", host, 80), "{p}");
        assert!(!p.admits("ftp", host, 21), "{p}");
        assert!(!p.admits("wss", host, 443), "{p}");
        assert!(!p.admits("https", "other.example.net", 443), "{p}");
    }
}

// ------------------------------------------------------------ review fixes

/// A last label the WHATWG URL parser reads as a number (decimal, or `0x`
/// with hex digits or none) makes the whole name an IPv4 address there.
#[test]
fn whatwg_numeric_ipv4_forms_are_refused_as_ip_addresses() {
    for text in [
        "127.0x1",
        "1.2.3.0x4",
        "example.0x",
        "example.0X1F",
        "a.b.0xff",
        "10.1",
        "example.0x7f000001",
    ] {
        assert_eq!(
            HostPattern::parse(text),
            Err(HostPatternError::IpAddress),
            "{text}"
        );
    }
    // A label that only starts like a number is a name.
    ok("example.0xg");
    ok("0x1.example.org");
}

/// The scheme, path or port fix names a host only when that host is itself
/// a valid pattern.
#[test]
fn the_host_hint_is_given_only_for_a_valid_host() {
    assert_eq!(refused("https://x.org/").host_hint(), Some("x.org"));
    assert_eq!(refused("x.org/docs").host_hint(), Some("x.org"));
    assert_eq!(refused("example.org:8080").host_hint(), Some("example.org"));
    for text in [
        "https://user@x.org/",
        "https://exämple.org/",
        "https://10.1.2.3/",
        "https://localhost/",
        "/docs",
    ] {
        assert_eq!(refused(text).host_hint(), None, "{text}");
    }
}
