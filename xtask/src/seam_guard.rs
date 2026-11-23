//! `seam-guard` (story 2.2, decision 7): the egress client's test seams (a
//! resolver and a per-hop transport) are parameters of a second
//! constructor, `NetEgress::with_seams`, used only by tests. Shipped code
//! never names it: the guard fails when `with_seams` appears in the code
//! (comments and literals are not code) of any `crates/*/src` file, a
//! `#[cfg(test)]` module included, naming the file and line. The one
//! exception is its definition (`fn with_seams`) in am-net's source: a call
//! from shipped am-net code fails too. `NetEgress::new` always uses the system
//! resolver and reqwest, and no env var, flag or feature changes either.
//!
//! Story 3.2 (AD-15) adds am-server's test-only UI-class fixture route: the
//! router builder `router_with_fixture` and its parameter `FixtureRoute`
//! live only in `crates/am-server/src/fixture.rs`. Every other shipped
//! source (am-app's `agentic serve`, am-server's own `lib.rs` and router
//! included) naming either fails, so the release binary never serves the
//! fixture route; inside `fixture.rs`, `router_with_fixture` may appear only
//! at its definition.

use std::path::Path;

use crate::Failure;
use crate::forbid::strip_comments_and_strings;
use crate::shipped::sources;
use crate::walk::is_word;

/// One test-only name and where it may be named.
struct Seam {
    /// The name, matched as a whole word.
    name: &'static str,
    /// The source path (a folder ending in `/`, or one file) that defines it.
    home: &'static str,
    /// In `home`, only a line holding this (whitespace collapsed) and the
    /// name once may name it; `None`: any line of `home` may.
    definition: Option<&'static str>,
    /// What a finding says.
    why: &'static str,
}

const SEAMS: [Seam; 3] = [
    Seam {
        name: "with_seams",
        home: "crates/am-net/src/",
        definition: Some("fn with_seams"),
        why: "names NetEgress::with_seams, am-net's test-only constructor; shipped code builds the client with NetEgress::new (decision 7: test seams are constructor parameters, never switches)",
    },
    Seam {
        name: "router_with_fixture",
        home: "crates/am-server/src/fixture.rs",
        definition: Some("fn router_with_fixture"),
        why: "names am_server::router_with_fixture, the test-only router builder; shipped code serves am_server::router (story 3.2: the fixture route is a constructor parameter, never a feature, env var or flag)",
    },
    Seam {
        name: "FixtureRoute",
        home: "crates/am-server/src/fixture.rs",
        definition: None,
        why: "names am_server::FixtureRoute, the test-only UI-class route; shipped code serves am_server::router (story 3.2: the fixture route is a constructor parameter, never a feature, env var or flag)",
    },
];

impl Seam {
    fn is_home(&self, file: &str) -> bool {
        if self.home.ends_with('/') {
            file.starts_with(self.home)
        } else {
            file == self.home
        }
    }
}

pub fn run(root: &Path) -> Result<String, Failure> {
    let mut findings = Vec::new();
    let mut scanned = 0usize;
    for source in sources(root) {
        scanned += 1;
        let code = strip_comments_and_strings(&source.text);
        for (n, line) in code.lines().enumerate() {
            let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
            for seam in &SEAMS {
                if !names(line, seam.name) {
                    continue;
                }
                if seam.is_home(&source.name) {
                    match seam.definition {
                        None => continue,
                        Some(def) if flat.contains(def) && flat.matches(seam.name).count() == 1 => {
                            continue;
                        }
                        Some(_) => {}
                    }
                }
                findings.push(format!("{}:{}: {}", source.name, n + 1, seam.why));
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{scanned} files clean: only tests and their definitions name NetEgress::with_seams, router_with_fixture and FixtureRoute"
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}

/// `name` as a whole word in `line`.
fn names(line: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = line[from..].find(name) {
        let start = from + pos;
        let end = start + name.len();
        let before = line[..start].chars().next_back();
        let after = line[end..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::names;

    #[test]
    fn a_whole_word_only() {
        assert!(names(
            "let _ = am_net::NetEgress::with_seams(a, b);",
            "with_seams"
        ));
        assert!(!names("fn with_seams_x() {}", "with_seams"));
        assert!(!names("fn build_with_seams() {}", "with_seams"));
        assert!(names(
            "let r = router_with_fixture(s, f);",
            "router_with_fixture"
        ));
        assert!(!names(
            "let r = router_with_fixtures(s);",
            "router_with_fixture"
        ));
        assert!(names("use am_server::FixtureRoute;", "FixtureRoute"));
        assert!(!names("struct FixtureRoutes;", "FixtureRoute"));
    }
}
