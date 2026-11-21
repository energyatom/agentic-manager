//! Story 1.14 acceptance tests (AD-48, AD-12, AD-39): a seat's host
//! allow-list is its own `allowed_hosts`, parsed by `am_domain::HostPattern`
//! and matched only through it. Matrix rows "Seat hosts", "Host warnings",
//! "No Account host", "v2 leftover" and "Shipped". Each project is written
//! in a temporary folder at `schema_version: 2`. Written before the field
//! exists.
//!
//! What these tests expect besides the loader's findings:
//! - `Seat::allowed_hosts: Vec<HostPattern>`;
//! - `ConfigSnapshot::allowed_hosts(seat)`: exactly the seat's patterns
//!   (items printable with `to_string()`);
//! - `allowed_hosts` after `links` in a seat file's canonical key order.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use am_config::{Severity, canonical_bytes, load};
use common::*;

/// A seat that fetches (Web search on) with three saved links and two host
/// patterns: line 11 is the link to `other.net`, lines 17-18 the patterns.
const ALPHA: &str = "---
schema_version: 2
name: Alpha
role: Researcher
team: core
capabilities:
  web_search:
    on: true
links:
  - title: Other
    url: https://other.net/
  - title: Docs
    url: https://docs.example.org/a
  - title: Wild
    url: https://a.wild.example.net/b
allowed_hosts:
  - docs.example.org
  - \"*.Wild.Example.NET\"
---
Find new items.
";

/// A Source owned by alpha whose one seed (line 6) is on `feeds.x.org`.
const FEEDS: &str = "schema_version: 2
type: feed
owner_seat: alpha
scope:
  seeds:
    - https://feeds.x.org/rss.xml
output:
  records: true
terms_checked: 2026-11-01
";

/// The same Source with a scope host pattern on line 8.
const FEEDS_SCOPED: &str = "schema_version: 2
type: feed
owner_seat: alpha
scope:
  seeds:
    - https://feeds.x.org/rss.xml
  allowed_hosts:
    - \"*.x.org\"
output:
  records: true
terms_checked: 2026-11-01
";

/// A seat listing one host pattern, written on line 7.
fn beta_with(pattern: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Beta\nrole: Analyst\nteam: core\nallowed_hosts:\n  - \"{pattern}\"\n---\nRead.\n"
    )
}

/// alpha's file with its two patterns replaced by `patterns`.
fn alpha_with(patterns: &[&str]) -> String {
    let list: String = patterns.iter().map(|p| format!("  - \"{p}\"\n")).collect();
    ALPHA.replace("  - docs.example.org\n  - \"*.Wild.Example.NET\"\n", &list)
}

// ------------------------------------------------------------- seat hosts

#[test]
fn seat_allowed_hosts_validate_and_are_exactly_the_snapshot_allow_list() {
    let dir = project_with(&[
        ("seats/alpha.md", ALPHA),
        ("seats/beta.md", &beta_with("docs.example.org")),
    ]);
    let loaded = load(dir.path()).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.expect("no errors gives a snapshot");

    // Uppercase is lowercased (DNS is case-insensitive).
    assert_eq!(
        hosts_of(&snap, "alpha"),
        ["docs.example.org", "*.wild.example.net"]
    );
    let alpha: Vec<String> = snap.seats()["alpha"]
        .allowed_hosts
        .iter()
        .map(|h| h.to_string())
        .collect();
    assert_eq!(alpha, ["docs.example.org", "*.wild.example.net"]);
    assert_eq!(hosts_of(&snap, "beta"), ["docs.example.org"]);
}

#[test]
fn a_seat_without_allowed_hosts_may_reach_no_host() {
    let dir = project_with(&[(
        "seats/beta.md",
        "---\nschema_version: 2\nname: Beta\nrole: Analyst\nteam: core\n---\nRead.\n",
    )]);
    let snap = snapshot(dir.path());
    assert!(hosts_of(&snap, "beta").is_empty());
    assert!(snap.seats()["beta"].allowed_hosts.is_empty());
}

#[test]
fn a_host_written_with_a_scheme_fails_at_its_line_with_the_fix() {
    let dir = project_with(&[
        ("seats/alpha.md", ALPHA),
        ("seats/beta.md", &beta_with("https://x.org/")),
    ]);
    let found = findings(dir.path());
    let f = at(&found, "seats/beta.md", 7, "https://x.org/");
    assert_eq!(f.severity, Severity::Error);
    assert!(
        f.fix.contains("write the host only, e.g. x.org"),
        "{}",
        f.fix
    );
    // "Validates" is the matrix row's word: no error. The same ALPHA text
    // must still warn about its off-list link (the Host warnings row).
    assert!(
        errors(
            &in_file(&found, "seats/alpha.md")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        )
        .is_empty(),
        "the first seat validates: {found:#?}"
    );
}

#[test]
fn every_refused_pattern_is_an_error_at_its_line() {
    for pattern in [
        "*.com",
        "10.1.2.3",
        "localhost",
        "example.org:8080",
        "https://x.org/",
        "x.org/docs",
        "exämple.org",
        "a.*.org",
        "*",
        "[::1]",
        "example",
        "-example.org",
    ] {
        let dir = project_with(&[("seats/beta.md", &beta_with(pattern))]);
        let found = findings(dir.path());
        let errs: Vec<_> = errors(&found)
            .into_iter()
            .filter(|f| f.file == "seats/beta.md")
            .collect();
        assert_eq!(errs.len(), 1, "{pattern}: {found:#?}");
        assert_eq!(errs[0].line, 7, "{pattern}");
        assert!(errs[0].message.contains(pattern), "{pattern}: {errs:#?}");
        assert!(!errs[0].fix.is_empty(), "{pattern}");
    }
}

#[test]
fn allowed_hosts_sits_after_links_in_canonical_order() {
    let shuffled = "---\nallowed_hosts:\n  - docs.example.org\nfolders:\n  - id: work\n    path: work\nname: Alpha\nlinks:\n  - title: Docs\n    url: https://docs.example.org/a\nschema_version: 2\nrole: Researcher\nteam: core\n---\nFind new items.\n";
    let out =
        String::from_utf8(canonical_bytes("seats/alpha.md", shuffled.as_bytes()).unwrap()).unwrap();
    let pos = |key: &str| {
        out.find(&format!("\n{key}:"))
            .unwrap_or_else(|| panic!("no {key} in {out}"))
    };
    assert!(pos("links") < pos("allowed_hosts"), "{out}");
    assert!(pos("allowed_hosts") < pos("folders"), "{out}");
    assert!(out.ends_with("---\nFind new items.\n"), "{out}");
}

// ------------------------------------------------------------- host warnings

#[test]
fn a_link_host_and_a_source_scope_host_off_the_seat_list_are_two_warnings() {
    let dir = project_with(&[("seats/alpha.md", ALPHA), ("sources/feeds.yaml", FEEDS)]);
    let loaded = load(dir.path()).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    assert!(loaded.snapshot.is_some(), "warnings keep the snapshot");
    let found = &loaded.findings;
    assert_eq!(warnings(found).len(), 2, "{found:#?}");

    // The link to other.net (line 11); the docs and wildcard links match.
    let link = at(found, "seats/alpha.md", 11, "other.net");
    assert_eq!(link.severity, Severity::Warning);
    // The Source seed on feeds.x.org (line 6).
    let scope = at(found, "sources/feeds.yaml", 6, "feeds.x.org");
    assert_eq!(scope.severity, Severity::Warning);
    for f in [link, scope] {
        assert!(f.message.contains("alpha"), "{f:#?}");
        assert!(f.fix.contains("allowed_hosts"), "{f:#?}");
        assert!(f.fix.contains("seats/alpha.md"), "{f:#?}");
        assert!(!f.fix.contains("egress_hosts"), "{f:#?}");
    }
}

#[test]
fn a_scope_host_is_judged_by_the_owner_seat_patterns_with_covers() {
    let source_warnings = |patterns: &[&str], source: &str| -> Vec<u32> {
        let dir = project_with(&[
            ("seats/alpha.md", &alpha_with(patterns)),
            ("sources/feeds.yaml", source),
        ]);
        let found = findings(dir.path());
        assert!(errors(&found).is_empty(), "{patterns:?}: {found:#?}");
        let mut lines: Vec<u32> = in_file(&found, "sources/feeds.yaml")
            .into_iter()
            .filter(|f| f.severity == Severity::Warning)
            .map(|f| f.line)
            .collect();
        lines.sort();
        lines
    };
    // A wildcard owner pattern covers the seed host and the scope pattern.
    assert!(source_warnings(&["*.x.org"], FEEDS).is_empty());
    assert!(source_warnings(&["*.x.org"], FEEDS_SCOPED).is_empty());
    // An exact owner pattern matches the seed host only.
    assert!(source_warnings(&["feeds.x.org"], FEEDS).is_empty());
    assert_eq!(source_warnings(&["feeds.x.org"], FEEDS_SCOPED), [8]);
    // The bare domain covers neither.
    assert_eq!(source_warnings(&["x.org"], FEEDS_SCOPED), [6, 8]);
    // A deeper wildcard does not cover the wider scope pattern.
    assert_eq!(source_warnings(&["*.feeds.x.org"], FEEDS_SCOPED), [6, 8]);

    let dir = project_with(&[
        ("seats/alpha.md", &alpha_with(&["feeds.x.org"])),
        ("sources/feeds.yaml", FEEDS_SCOPED),
    ]);
    let found = findings(dir.path());
    let f = at(&found, "sources/feeds.yaml", 8, "*.x.org");
    assert!(f.message.contains("alpha"), "{f:#?}");
    assert!(f.fix.contains("seats/alpha.md"), "{f:#?}");
    assert!(f.fix.contains("allowed_hosts"), "{f:#?}");
}

#[test]
fn a_scope_host_pattern_is_parsed_by_host_pattern() {
    let source = FEEDS_SCOPED.replace("\"*.x.org\"", "\"https://x.org\"");
    let dir = project_with(&[("seats/alpha.md", ALPHA), ("sources/feeds.yaml", &source)]);
    let found = findings(dir.path());
    let f = at(&found, "sources/feeds.yaml", 8, "https://x.org");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("write the host only"), "{}", f.fix);
    for bad in ["10.1.2.3", "*.org", "localhost"] {
        let source = FEEDS_SCOPED.replace("\"*.x.org\"", &format!("\"{bad}\""));
        let dir = project_with(&[("seats/alpha.md", ALPHA), ("sources/feeds.yaml", &source)]);
        let found = findings(dir.path());
        let f = at(&found, "sources/feeds.yaml", 8, bad);
        assert_eq!(f.severity, Severity::Error, "{bad}");
    }
}

// ------------------------------------------------------------- no Account host

#[test]
fn an_account_endpoint_never_joins_a_seat_allow_list() {
    let accounts = "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    name: Cloud account\n    secret_ref: cloud\n";
    let model = "model:\n  account: cloud\n  model: example-large\nfallback:\n  - account: cloud\n    model: example-large\n";
    let writer = format!(
        "---\nschema_version: 2\nname: Writer\nrole: Writer\nteam: core\n{model}allowed_hosts:\n  - docs.example.org\n---\nWrite.\n"
    );
    let plain = format!(
        "---\nschema_version: 2\nname: Plain\nrole: Analyst\nteam: core\n{model}---\nRead.\n"
    );
    let dir = project_with(&[
        ("models/accounts.yaml", accounts),
        ("seats/writer.md", &writer),
        ("seats/plain.md", &plain),
    ]);
    let snap = snapshot(dir.path());
    assert_eq!(hosts_of(&snap, "writer"), ["docs.example.org"]);
    assert!(hosts_of(&snap, "plain").is_empty());
    for seat in ["writer", "plain"] {
        assert!(
            !hosts_of(&snap, seat)
                .iter()
                .any(|h| h.contains("anthropic")),
            "{seat}"
        );
    }
}

// ------------------------------------------------------------- v2 leftover

#[test]
fn egress_hosts_in_a_v2_tools_yaml_is_an_unknown_key_naming_the_seats_lists() {
    let dir = project_with(&[
        (
            "tools.yaml",
            "schema_version: 2\negress_hosts:\n  - example.org\n",
        ),
        ("seats/alpha.md", ALPHA),
    ]);
    let found = findings(dir.path());
    let f = at(&found, "tools.yaml", 2, "egress_hosts");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("allowed_hosts"), "{}", f.fix);
    assert!(f.fix.contains("seats/"), "{}", f.fix);
    assert_eq!(in_file(&found, "tools.yaml").len(), 1, "{found:#?}");
}

#[test]
fn a_v2_tools_yaml_without_egress_hosts_validates() {
    let dir = project_with(&[
        (
            "tools.yaml",
            "schema_version: 2\nsearch:\n  provider: builtin\n",
        ),
        ("seats/alpha.md", ALPHA),
    ]);
    let found = findings(dir.path());
    assert!(in_file(&found, "tools.yaml").is_empty(), "{found:#?}");
}

// ------------------------------------------------------------- shipped

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn shipped_templates_and_fixtures_hold_no_egress_hosts() {
    // The one fixture kept at schema_version 1 for the migration tests is
    // the exception (migrate_hosts.rs).
    let old = repo().join("fixtures/projects/hosts-v1");
    let mut files = Vec::new();
    files_under(&repo().join("templates"), &mut files);
    files_under(&repo().join("fixtures"), &mut files);
    for path in files.into_iter().filter(|p| !p.starts_with(&old)) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        assert!(
            !text.contains("egress_hosts"),
            "{} still holds egress_hosts",
            path.display()
        );
    }
}

#[test]
fn role_templates_carry_no_allowed_hosts() {
    let roles = repo().join("templates/roles");
    let mut files = Vec::new();
    files_under(&roles, &mut files);
    assert!(files.len() >= 8, "{files:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("allowed_hosts"), "{}", path.display());
    }
}

#[test]
fn shipped_fetching_seats_carry_the_hosts_tools_yaml_listed() {
    let hosts = |root: &str, seat: &str| -> BTreeSet<String> {
        hosts_of(&snapshot(&fixture(root)), seat)
            .into_iter()
            .collect()
    };
    assert_eq!(hosts("valid", "alpha"), set(&["example.org"]));
    assert!(hosts("valid", "beta").is_empty());
    assert_eq!(
        hosts("anatomy", "full"),
        set(&["example.org", "docs.example.org"])
    );
    assert_eq!(hosts("sources-ok", "owner"), set(&["example.org"]));
    // The broken fixtures do not load cleanly; their seat files carry the
    // list as text.
    for (root, seat) in [("sources-broken", "owner"), ("broken-anatomy", "searcher")] {
        let text = std::fs::read_to_string(fixture(root).join(format!("seats/{seat}.md"))).unwrap();
        assert!(
            text.contains("allowed_hosts:\n  - example.org\n"),
            "{root}/{seat}: {text}"
        );
    }
    // Company A listed no hosts, so none of its seats gains one.
    // The one sample company under templates/samples/ (its id is not
    // spelled here: the AD-23 denylist keeps it out of crates/).
    let samples: Vec<PathBuf> = std::fs::read_dir(repo().join("templates/samples"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(samples.len(), 1, "{samples:?}");
    let sample = samples[0].clone();
    let loaded = am_config::load_in(&sample, am_config::LoadOptions::template(&sample)).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.expect("the sample validates");
    assert!(!snap.seats().is_empty());
    for seat in snap.seats().keys() {
        assert!(hosts_of(&snap, seat).is_empty(), "{seat}");
    }
}

/// Review fix: the "write the host only" fix never suggests a host that is
/// refused too; the generic fix stands instead.
#[test]
fn a_refused_host_inside_a_url_gets_the_generic_fix() {
    for (pattern, hidden) in [
        ("https://user@x.org/", "user@x.org"),
        ("https://exämple.org/", "exämple.org"),
    ] {
        let dir = project_with(&[("seats/beta.md", &beta_with(pattern))]);
        let found = findings(dir.path());
        let f = at(&found, "seats/beta.md", 7, pattern);
        assert_eq!(f.severity, Severity::Error);
        assert!(!f.fix.contains(hidden), "{pattern}: {}", f.fix);
        assert!(f.fix.contains("docs.example.org"), "{pattern}: {}", f.fix);
    }
}
