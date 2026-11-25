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
//!
//! Story 3.3, review P4 and P5: the guard reads every `.rs` of a crate but
//! its `tests/`, `benches/` and `examples/` (`build.rs` included), fails
//! closed on a literal or comment that never ends, refuses `#[path]`,
//! `include!` and `macro_rules!` in shipped source (each could carry code
//! past these scans), and allows `ToolRunner` only in am-domain, am-engine
//! and am-testkit.
//!
//! Story 3.3 (owner condition 2, AD-11) adds am-engine's tool seam: the
//! engine takes a tool runner only through `run_plan_with_tools`, defined
//! in `crates/am-engine/src/run.rs` and named nowhere else in shipped code
//! (am-app's `agentic run` included), so the binary runs no runner tool;
//! its one runner is am-testkit's `FixtureTool`, in tests. Since story 3.6
//! the binary's only tool is the built-in `table_write`, which the
//! engine's shipped ToolGate runs through the row-writer port, not a
//! runner.
//!
//! Story 3.6's ToolGate slice, review round 1 (R3): the gate and the row
//! writer are built, and a call admitted, only by the engine's driver, so
//! no other shipped code can hand ToolGate a forged step (`StepTools`, a
//! seat's team) or call place (`CallPlace`). `ToolGate` is named only in
//! `crates/am-engine/src/toolgate.rs` and `run.rs`; `StoreRowWriter` only
//! in `crates/am-engine/src/row_writer.rs` and `run.rs`; `admit` only in
//! `toolgate.rs` and `run.rs`. A gate's `run` needs a `Permit`, which only
//! `admit` makes.

use std::path::Path;

use crate::Failure;
use crate::forbid::{strip_comments_and_strings, unterminated_finding};
use crate::shipped::tree;
use crate::walk::is_word;

/// One test-only name and where it may be named.
struct Seam {
    /// The name, matched as a whole word.
    name: &'static str,
    /// The source paths (a folder ending in `/`, or one file) where it may
    /// be named.
    homes: &'static [&'static str],
    /// In a home, only its definition (the name right after `fn`) may
    /// name it; `None`: any place in a home may.
    definition: Option<&'static str>,
    /// Outside the homes, one more file where the name may appear only in
    /// a `use` and in the signature of the named `fn` (story 3.3, review
    /// R7: am-engine's `run_plan_with_tools` takes the runner it hands to
    /// the ToolGate it builds).
    also: Option<(&'static str, &'static str)>,
    /// What a finding says.
    why: &'static str,
}

const SEAMS: [Seam; 8] = [
    Seam {
        name: "with_seams",
        homes: &["crates/am-net/src/"],
        definition: Some("fn with_seams"),
        also: None,
        why: "names NetEgress::with_seams, am-net's test-only constructor; shipped code builds the client with NetEgress::new (decision 7: test seams are constructor parameters, never switches)",
    },
    Seam {
        name: "router_with_fixture",
        homes: &["crates/am-server/src/fixture.rs"],
        definition: Some("fn router_with_fixture"),
        also: None,
        why: "names am_server::router_with_fixture, the test-only router builder; shipped code serves am_server::router (story 3.2: the fixture route is a constructor parameter, never a feature, env var or flag)",
    },
    Seam {
        name: "FixtureRoute",
        homes: &["crates/am-server/src/fixture.rs"],
        definition: None,
        also: None,
        why: "names am_server::FixtureRoute, the test-only UI-class route; shipped code serves am_server::router (story 3.2: the fixture route is a constructor parameter, never a feature, env var or flag)",
    },
    Seam {
        name: "run_plan_with_tools",
        homes: &["crates/am-engine/src/run.rs"],
        definition: Some("fn run_plan_with_tools"),
        also: None,
        why: "names am_engine::run::run_plan_with_tools, the engine's test-only tool seam; shipped code runs a plan with run_plan and hands the engine no tool (story 3.3, owner condition 2: the tool runner is a constructor parameter only tests pass)",
    },
    Seam {
        name: "ToolRunner",
        homes: &[
            "crates/am-domain/src/",
            "crates/am-engine/src/toolgate.rs",
            "crates/am-testkit/src/",
        ],
        definition: None,
        also: Some(("crates/am-engine/src/run.rs", "run_plan_with_tools")),
        why: "names am_domain::ToolRunner outside am-domain (its definition), ToolGate (crates/am-engine/src/toolgate.rs, its only caller), run_plan_with_tools' signature and am-testkit (its one runner, FixtureTool, a dev-dependency); the binary has no tool (story 3.3, owner condition 2, AD-11)",
    },
    Seam {
        name: "ToolGate",
        homes: &[
            "crates/am-engine/src/toolgate.rs",
            "crates/am-engine/src/run.rs",
        ],
        definition: None,
        also: None,
        why: "names ToolGate outside its definition and the engine's driver (crates/am-engine/src/run.rs), the one place a gate is built and given a step's facts and a call's place; other shipped code could forge them (story 3.6, review round 1, R3; AD-11)",
    },
    Seam {
        name: "StoreRowWriter",
        homes: &[
            "crates/am-engine/src/row_writer.rs",
            "crates/am-engine/src/run.rs",
        ],
        definition: None,
        also: None,
        why: "names StoreRowWriter outside its definition and the engine's driver (crates/am-engine/src/run.rs), which alone hands it to ToolGate, its only caller (story 3.6, review round 1, R3; AD-21)",
    },
    Seam {
        name: "admit",
        homes: &[
            "crates/am-engine/src/toolgate.rs",
            "crates/am-engine/src/run.rs",
        ],
        definition: None,
        also: None,
        why: "names admit, ToolGate's admission of a call, outside ToolGate and the engine's driver (crates/am-engine/src/run.rs), which alone admits a call with its step's facts and its journaled place (story 3.6, review round 1, R3; AD-11)",
    },
];

/// One token of a file's code (comments and literals blanked): an
/// identifier (a raw `r#ident` read as `ident`) or one punctuation
/// character, with its 1-based line.
struct Token {
    text: String,
    line: usize,
}

/// The tokens of `code`, over the whole file (story 3.3, review R2: no
/// spelling split over lines, or raw, escapes them).
fn tokens(code: &str) -> Vec<Token> {
    let c: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if is_word(ch) {
            let start = i;
            while i < c.len() && is_word(c[i]) {
                i += 1;
            }
            let mut text: String = c[start..i].iter().collect();
            // A raw identifier: `r#path` is `path`.
            if text == "r"
                && c.get(i) == Some(&'#')
                && c.get(i + 1).is_some_and(|n| n.is_alphabetic() || *n == '_')
            {
                let from = i + 1;
                i = from;
                while i < c.len() && is_word(c[i]) {
                    i += 1;
                }
                text = c[from..i].iter().collect();
            }
            out.push(Token { text, line });
        } else {
            out.push(Token {
                text: ch.to_string(),
                line,
            });
            i += 1;
        }
    }
    out
}

/// The constructs that would carry code past the call-site scans, each a
/// finding of its own in shipped source (story 3.3, review P5 and R2), as
/// (line, construct, why): `path` as an attribute's name anywhere inside
/// `#[…]` (`cfg_attr` included), `include!`, `macro_rules!`, and a `use`
/// that imports `include` or `macro_rules` (under any name).
/// `include_str!` and `include_bytes!` hold data and pass.
fn constructs(toks: &[Token]) -> Vec<(usize, &'static str, &'static str)> {
    const PATH: &str = "a `#[path]` attribute (or `path = …` inside `cfg_attr`) reads a module file the call-site scans may not; keep modules at their default paths";
    const INCLUDE: &str = "`include!` pulls Rust source into this file past the call-site scans (imported under any name too); write the code here or in a module";
    const MACRO: &str = "a `macro_rules!` macro can spell a call the call-site scans cannot see; write the code out";
    let text = |k: usize| toks.get(k).map(|t| t.text.as_str()).unwrap_or("");
    let mut found = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        match t.text.as_str() {
            "include" if text(k + 1) == "!" => found.push((t.line, "include!", INCLUDE)),
            "macro_rules" if text(k + 1) == "!" => found.push((t.line, "macro_rules!", MACRO)),
            "use" => {
                for u in toks[k + 1..].iter().take_while(|u| u.text != ";") {
                    match u.text.as_str() {
                        "include" => found.push((u.line, "include!", INCLUDE)),
                        "macro_rules" => found.push((u.line, "macro_rules!", MACRO)),
                        _ => {}
                    }
                }
            }
            "#" => {
                let mut j = k + 1;
                if text(j) == "!" {
                    j += 1;
                }
                if text(j) != "[" {
                    continue;
                }
                let mut depth = 0usize;
                while j < toks.len() {
                    match toks[j].text.as_str() {
                        "[" => depth += 1,
                        "]" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        "path" if text(j + 1) == "=" => {
                            found.push((toks[j].line, "#[path", PATH));
                        }
                        _ => {}
                    }
                    j += 1;
                }
            }
            _ => {}
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The token ranges of `toks` where a seam's `also` name may stand: every
/// `use` statement, and the signature of the `fn` named `function` (from
/// `fn` to its body's `{`).
fn also_ranges(toks: &[Token], function: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        let signature = t.text == "fn" && toks.get(k + 1).is_some_and(|n| n.text == function);
        let end_at = if t.text == "use" {
            ";"
        } else if signature {
            "{"
        } else {
            continue;
        };
        let end = toks[k..]
            .iter()
            .position(|x| x.text == end_at)
            .map_or(toks.len(), |p| k + p);
        ranges.push((k, end));
    }
    ranges
}

impl Seam {
    fn is_home(&self, file: &str) -> bool {
        self.homes.iter().any(|home| {
            if home.ends_with('/') {
                file.starts_with(home)
            } else {
                file == *home
            }
        })
    }
}

pub fn run(root: &Path) -> Result<String, Failure> {
    let mut findings = Vec::new();
    let mut scanned = 0usize;
    let tree = tree(root);
    findings.extend(tree.findings);
    for source in tree.sources {
        scanned += 1;
        if let Some(finding) = unterminated_finding(&source.name, &source.text) {
            findings.push(finding);
            continue;
        }
        let toks = tokens(&strip_comments_and_strings(&source.text));
        for (line, construct, why) in constructs(&toks) {
            findings.push(format!(
                "{}:{line}: `{construct}` in shipped source: {why} (story 3.3)",
                source.name
            ));
        }
        for seam in &SEAMS {
            let also = seam
                .also
                .filter(|(file, _)| *file == source.name)
                .map(|(_, function)| also_ranges(&toks, function));
            for (k, t) in toks.iter().enumerate() {
                if t.text != seam.name {
                    continue;
                }
                if seam.is_home(&source.name) {
                    let after_fn = k > 0 && toks[k - 1].text == "fn";
                    if seam.definition.is_none() || after_fn {
                        continue;
                    }
                }
                let allowed = also
                    .as_ref()
                    .is_some_and(|ranges| ranges.iter().any(|(a, b)| (*a..=*b).contains(&k)));
                if !allowed {
                    findings.push(format!("{}:{}: {}", source.name, t.line, seam.why));
                }
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{scanned} files clean: only tests and their definitions name NetEgress::with_seams, router_with_fixture, FixtureRoute and run_plan_with_tools; ToolRunner only in am-domain, ToolGate, run_plan_with_tools' signature and am-testkit; ToolGate, StoreRowWriter and admit only in their definitions and the engine's driver; no #[path], include! or macro_rules!; no symlink, unreadable file or manifest target outside the scanned source under crates/"
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}

/// `name` as a whole word in `line`.
pub(crate) fn names(line: &str, name: &str) -> bool {
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
    fn the_constructs_are_found_as_written_and_data_includes_pass() {
        let found = |line: &str| {
            super::constructs(&super::tokens(line))
                .iter()
                .map(|c| c.1)
                .collect::<Vec<_>>()
        };
        assert_eq!(found("#[cfg_attr(all(), path = x)]"), vec!["#[path"]);
        assert_eq!(found("#[\nr#path = x]"), vec!["#[path"]);
        assert_eq!(found("use core::include as grab;"), vec!["include!"]);
        assert_eq!(found("macro_rules\n! m {}"), vec!["macro_rules!"]);
        assert!(found("pub struct O { pub path: String }").is_empty());
        assert!(found("fn include_all(path: &str) { let macro_rules_count = 1; }").is_empty());
        assert_eq!(found("#[path = \"x.rs\"]"), vec!["#[path"]);
        assert_eq!(found("include!(\"gen.rs\");"), vec!["include!"]);
        assert_eq!(found("macro_rules! wrap {"), vec!["macro_rules!"]);
        assert!(found("const A: &str = include_str!(\"a.md\");").is_empty());
        assert!(found("const B: &[u8] = include_bytes!(\"b.png\");").is_empty());
        assert!(found("#[path_like]").is_empty());
    }

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
