//! `check-deps`: the AD-4 dependency matrix (architecture, Design Paradigm
//! table) checked against `cargo metadata`. Only normal and build edges count;
//! dev-dependencies are test scaffolding. Every workspace crate must be
//! classified here, so a new crate cannot slip past the matrix.
//!
//! Story 3.3 (AD-10, amended 2026-11-23): call-site rules beside the
//! matrix, over the shipped source under `--root` (every `.rs` of a crate
//! but its `tests/`, `benches/` and `examples/`, comments and literals left
//! out; a literal or comment that never ends fails closed):
//!
//! - `TrustedInput`'s two named constructors, `from_step_output` and
//!   `from_owner_note`, are named only in `crates/am-engine/src/` and, in
//!   am-domain, on their definition lines;
//! - `Instruction::authored` (review P14), which turns text into a trusted
//!   instruction, is named only where config or the owner's own words
//!   become one: [`INSTRUCTION_SITES`], the `#[cfg(test)]` items of
//!   [`INSTRUCTION_TEST_SITES`], and am-domain's definition line.
//!
//! Story 3.4 (AD-15, AD-28; review R12): the one Human-only resolve of an
//! Inbox item, `Writer::resolve_inbox_item`, and the Human actor's type,
//! `HumanActor`, are matched as tokens over whole files (so a split line, a
//! `use … as` or `type` alias or `<HumanActor>::new` is still seen): they
//! stand in shipped code only in am-domain (the definition and its
//! re-export), am-store's `src/lib.rs` (the resolve), the two command
//! handlers ([`RESOLVE_SITES`]) and `#[cfg(test)]` items of am-store.
//! Review S5: in am-domain only `actor.rs` and `lib.rs`'s plain re-export,
//! and in every file an `as` alias or a `pub use` of either fails.

use crate::Failure;
use crate::forbid::{strip_comments_and_strings, unterminated_finding};
use crate::seam_guard::names;
use crate::shipped::{in_ranges, sources, test_ranges};
use crate::walk::is_word;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// `TrustedInput`'s two named constructors (AD-10).
const TRUSTED_CONSTRUCTORS: [&str; 2] = ["from_step_output", "from_owner_note"];

/// The one crate that calls them.
const TRUSTED_CALLER: &str = "crates/am-engine/src/";

/// The crate that defines them.
const TRUSTED_HOME: &str = "crates/am-domain/src/";

/// The shipped files that may call `Instruction::authored` (review P14):
/// am-app building a run's plan from the config snapshot (a seat's
/// instructions, a step's instruction), the spike harness's and the
/// Account test's fixed prompts, and the spike driver's fixed prompt.
pub const INSTRUCTION_SITES: [&str; 4] = [
    "crates/am-app/src/run.rs",
    "crates/am-app/src/spike.rs",
    "crates/am-app/src/accounts.rs",
    "crates/am-engine/src/journal.rs",
];

/// The shipped files whose `#[cfg(test)]` items (only) may call it: the
/// engine driver's own unit tests build plans with it.
pub const INSTRUCTION_TEST_SITES: [&str; 1] = ["crates/am-engine/src/run.rs"];

/// The Human-only resolve of an Inbox item and the Human actor's type
/// (AD-15, story 3.4), matched as tokens.
const RESOLVE_TOKENS: [&str; 2] = ["resolve_inbox_item", "HumanActor"];

/// The two command handlers that may name them: the API's resolve route
/// and `agentic gate`.
pub const RESOLVE_SITES: [&str; 2] = ["crates/am-server/src/inbox.rs", "crates/am-app/src/gate.rs"];

/// Where the type is defined (review S5: only there in am-domain).
const HUMAN_HOME: &str = "crates/am-domain/src/actor.rs";

/// Where the type is re-exported, by a plain `pub use`.
const HUMAN_REEXPORT: &str = "crates/am-domain/src/lib.rs";

/// The file that defines the resolve.
const RESOLVE_HOME: &str = "crates/am-store/src/lib.rs";

/// The crate whose `#[cfg(test)]` items may name both (its own tests of
/// the resolve).
const RESOLVE_TEST_CRATE: &str = "crates/am-store/src/";

/// Every finding of the call-site rules on the shipped source under
/// `root`.
fn call_site_findings(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    for source in sources(root) {
        if let Some(finding) = unterminated_finding(&source.name, &source.text) {
            findings.push(finding);
            continue;
        }
        let code = strip_comments_and_strings(&source.text);
        let tests = test_ranges(&source.text);
        for (n, line) in code.lines().enumerate() {
            let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
            let definition = |name: &str| {
                source.name.starts_with(TRUSTED_HOME)
                    && flat.contains(&format!("fn {name}"))
                    && flat.matches(name).count() == 1
            };
            for name in TRUSTED_CONSTRUCTORS {
                if !names(line, name) || source.name.starts_with(TRUSTED_CALLER) || definition(name)
                {
                    continue;
                }
                findings.push(format!(
                    "{}:{}: names TrustedInput::{name}; only am-engine builds a TrustedInput, from a clean step's journaled output or an owner's note (AD-10: its two constructors are called only in {TRUSTED_CALLER})",
                    source.name,
                    n + 1
                ));
            }
            let allowed = INSTRUCTION_SITES.contains(&source.name.as_str())
                || (INSTRUCTION_TEST_SITES.contains(&source.name.as_str())
                    && in_ranges(&tests, n + 1))
                || definition("authored");
            if names(line, "authored") && !allowed {
                findings.push(format!(
                    "{}:{}: names Instruction::authored; text becomes a trusted Instruction only where config or the owner's own words do, at the sites xtask/src/deps.rs lists (AD-10; model, tool and file text is never an Instruction)",
                    source.name,
                    n + 1
                ));
            }
        }
        findings.extend(resolve_findings(&source.name, &code, &tests));
    }
    findings
}

/// The byte offsets of `token` in `code`, as a whole token.
fn token_offsets(code: &str, token: &str) -> Vec<usize> {
    code.match_indices(token)
        .filter(|(at, _)| {
            let before = code[..*at].chars().next_back();
            let after = code[at + token.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        })
        .map(|(at, _)| at)
        .collect()
}

/// The 1-based line of the byte offset `at` in `code`.
fn line_at(code: &str, at: usize) -> usize {
    code[..at].matches('\n').count() + 1
}

/// The byte spans of the `pub use …;` statements of `code` (also
/// `pub(crate) use` and the like), from `pub` to the `;`.
fn pub_use_spans(code: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for start in token_offsets(code, "pub") {
        let mut rest = code[start + 3..].trim_start();
        if let Some(scoped) = rest.strip_prefix('(') {
            let Some(close) = scoped.find(')') else {
                continue;
            };
            rest = scoped[close + 1..].trim_start();
        }
        let is_use = rest.starts_with("use") && !rest[3..].chars().next().is_some_and(is_word);
        if !is_use {
            continue;
        }
        let from = code.len() - rest.len();
        let end = code[from..].find(';').map_or(code.len(), |e| from + e + 1);
        spans.push((start, end));
    }
    spans
}

/// Story 3.4 (AD-15; reviews R12, S5): the Human-only resolve
/// (`resolve_inbox_item`) and the Human actor's type (`HumanActor`),
/// matched as tokens over the whole of one shipped file's code.
///
/// - They stand only in am-domain's `actor.rs` (the definition) and
///   `lib.rs` (its plain re-export), am-store's `lib.rs` (the resolve),
///   the two command handlers ([`RESOLVE_SITES`]), and `#[cfg(test)]`
///   items of am-store.
/// - In every file, the allowed ones included, an `as` alias of either, or
///   a `pub use` of either, fails, but am-domain `lib.rs`'s plain
///   re-export: another name, or another crate's re-export, would carry
///   them past this rule.
fn resolve_findings(name: &str, code: &str, tests: &[(usize, usize)]) -> Vec<String> {
    let pub_uses = pub_use_spans(code);
    let mut renamed: BTreeSet<usize> = BTreeSet::new();
    let mut misplaced: BTreeSet<usize> = BTreeSet::new();
    for token in RESOLVE_TOKENS {
        for at in token_offsets(code, token) {
            let line = line_at(code, at);
            let after = code[at + token.len()..].trim_start();
            let aliased =
                after.starts_with("as") && !after[2..].chars().next().is_some_and(is_word);
            let in_pub_use = pub_uses.iter().any(|(from, to)| (*from..*to).contains(&at));
            if aliased || (in_pub_use && name != HUMAN_REEXPORT) {
                renamed.insert(line);
                continue;
            }
            let allowed = name == HUMAN_HOME
                || (name == HUMAN_REEXPORT && in_pub_use)
                || name == RESOLVE_HOME
                || RESOLVE_SITES.contains(&name)
                || (name.starts_with(RESOLVE_TEST_CRATE) && in_ranges(tests, line));
            if !allowed {
                misplaced.insert(line);
            }
        }
    }
    let mut findings = Vec::new();
    for line in renamed {
        findings.push(format!(
            "{name}:{line}: renames or re-exports HumanActor or resolve_inbox_item (`as` or `pub use`); they are named only as themselves, so this rule sees every use of them (AD-15: only a person resolves an Inbox item)"
        ));
    }
    for line in misplaced {
        findings.push(format!(
            "{name}:{line}: names HumanActor or resolve_inbox_item; they (and HumanActor::new, which builds the one resolver) stand in shipped code only in am-domain's definition and re-export, am-store's lib.rs and the two command handlers, the API's Inbox route and `agentic gate` (AD-15: no engine, tool, MCP or other code path resolves an item)"
        ));
    }
    findings
}

/// What `am-domain` may use besides std (AD-4: no IO, no async runtime).
const DOMAIN_EXTERNALS: &[&str] = &["serde", "thiserror", "jiff", "uuid"];

/// The only external crates `am-glob` may use (architecture Design
/// Paradigm table, amended 2026-11-20).
const GLOB_EXTERNALS: &[&str] = &["globset", "unicode-normalization"];

/// The only crates that may depend on `am-glob`.
const GLOB_USERS: &[&str] = &["am-config", "am-engine", "am-tools", "am-connectors"];

const GLOB: &str = "am-glob";

const ADAPTERS: &[&str] = &[
    "am-net",
    "am-providers",
    "am-tools",
    "am-connectors",
    "am-render",
    "am-services",
    "am-mcp",
    "am-extract",
    "am-secrets",
];

#[derive(Clone, Copy)]
enum Layer {
    Domain,
    /// am-glob: the one glob matcher, a leaf.
    Glob,
    Engine,
    /// am-store and am-config.
    Record,
    Adapter,
    Api,
    Server,
    /// am-app and am-desktop: anything.
    Root,
    /// am-testkit, am-e2e and xtask: exempt from the matrix.
    Exempt,
}

fn layer(name: &str) -> Option<Layer> {
    Some(match name {
        "am-domain" => Layer::Domain,
        GLOB => Layer::Glob,
        "am-engine" => Layer::Engine,
        "am-store" | "am-config" => Layer::Record,
        n if ADAPTERS.contains(&n) => Layer::Adapter,
        "am-api" => Layer::Api,
        "am-server" => Layer::Server,
        "am-app" | "am-desktop" => Layer::Root,
        "am-testkit" | "am-e2e" | "xtask" => Layer::Exempt,
        _ => return None,
    })
}

/// The workspace crates `krate` (of `layer`) may depend on, and the rule's
/// wording. `am-glob` is added for its four users only.
fn allowed(layer: Layer, krate: &str) -> Option<(Vec<&'static str>, String)> {
    let user = GLOB_USERS.contains(&krate);
    let (mut list, rule): (Vec<&'static str>, String) = match layer {
        Layer::Domain => (
            vec![],
            "am-domain may depend only on std, serde, thiserror, jiff and uuid (AD-4)".into(),
        ),
        Layer::Glob => (
            vec![],
            "am-glob may depend on no workspace crate, and on no external crate but globset and unicode-normalization (AD-4)".into(),
        ),
        Layer::Engine => (
            vec!["am-domain", "am-store"],
            "am-engine may depend only on am-domain, am-store and am-glob (AD-4)".into(),
        ),
        Layer::Record if user => (
            vec!["am-domain"],
            format!("{krate} may depend only on am-domain and am-glob (AD-4)"),
        ),
        Layer::Record => (
            vec!["am-domain"],
            format!("{krate} may depend only on am-domain (AD-4)"),
        ),
        Layer::Adapter if user => (
            vec!["am-domain", "am-net"],
            format!("{krate} may depend only on am-domain, am-net and am-glob (AD-4)"),
        ),
        Layer::Adapter => (
            vec!["am-domain", "am-net"],
            "adapters may depend only on am-domain and am-net (AD-4)".into(),
        ),
        Layer::Api => (
            vec!["am-domain"],
            "am-api may depend only on am-domain (AD-4)".into(),
        ),
        Layer::Server => (
            vec!["am-domain", "am-engine", "am-api", "am-store"],
            "am-server may depend only on am-domain, am-engine, am-api and am-store (AD-4)".into(),
        ),
        Layer::Root | Layer::Exempt => return None,
    };
    if user {
        list.push(GLOB);
    }
    Some((list, rule))
}

/// The one glob library, which only am-glob may name (Consistency
/// Conventions: Globs).
const GLOBSET: &str = "globset";

/// The rule for an edge into globset from any crate but am-glob.
fn globset_rule() -> String {
    "only am-glob may depend on globset, the one glob matcher's engine; match globs through am_glob::Glob (AD-4, Conventions: Globs)".into()
}

/// The rule for an edge into am-glob from a crate that is not one of its
/// users.
fn glob_user_rule() -> String {
    "only am-config, am-engine, am-tools and am-connectors may depend on am-glob (AD-4)".into()
}

pub fn run(metadata: Option<&Path>, root: &Path) -> Result<String, Failure> {
    let json = match metadata {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| Failure::Setup(format!("cannot read {}: {e}", path.display())))?,
        None => cargo_metadata(root)?,
    };
    let doc: Value = serde_json::from_str(&json)
        .map_err(|e| Failure::Setup(format!("cargo metadata is not JSON: {e}")))?;
    let calls = call_site_findings(root);
    match check(&doc) {
        Ok(summary) if calls.is_empty() => Ok(format!(
            "{summary}; only am-engine builds a TrustedInput, and Instruction::authored is called only at its listed sites (AD-10); only the two command handlers name the Inbox resolve and HumanActor outside am-domain and am-store (AD-15)"
        )),
        Ok(_) => Err(Failure::Findings(calls)),
        Err(Failure::Findings(mut findings)) => {
            findings.extend(calls);
            Err(Failure::Findings(findings))
        }
        Err(e) => Err(e),
    }
}

fn cargo_metadata(root: &Path) -> Result<String, Failure> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| Failure::Setup(format!("cannot run cargo metadata: {e}")))?;
    if !out.status.success() {
        return Err(Failure::Setup(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    String::from_utf8(out.stdout)
        .map_err(|e| Failure::Setup(format!("cargo metadata output is not UTF-8: {e}")))
}

fn check(doc: &Value) -> Result<String, Failure> {
    let members: BTreeSet<&str> = doc["workspace_members"]
        .as_array()
        .ok_or_else(|| Failure::Setup("metadata has no workspace_members".into()))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let packages: Vec<&Value> = doc["packages"]
        .as_array()
        .ok_or_else(|| Failure::Setup("metadata has no packages".into()))?
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(id)))
        .collect();
    let names: BTreeSet<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();

    let mut findings = Vec::new();
    for pkg in &packages {
        let name = pkg["name"].as_str().unwrap_or("?");
        let Some(layer) = layer(name) else {
            findings.push(format!(
                "{name}: not in the AD-4 matrix; classify it in xtask/src/deps.rs before it lands"
            ));
            continue;
        };
        let deps = pkg["dependencies"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let Some((list, rule)) = allowed(layer, name) else {
            // Composition roots may use anything but am-glob, which only
            // its four users may, and globset, which only am-glob may
            // (exempt crates are test scaffolding).
            if matches!(layer, Layer::Root) {
                for dep in deps.iter().filter(|d| d["kind"].as_str() != Some("dev")) {
                    match dep["name"].as_str() {
                        Some(GLOB) => {
                            findings.push(format!("{name} -> {GLOB}: {}", glob_user_rule()))
                        }
                        Some(GLOBSET) => {
                            findings.push(format!("{name} -> {GLOBSET}: {}", globset_rule()))
                        }
                        _ => {}
                    }
                }
            }
            continue;
        };
        for dep in deps {
            // Normal (null) and build edges count; dev-dependencies do not.
            if dep["kind"].as_str() == Some("dev") {
                continue;
            }
            let dep_name = dep["name"].as_str().unwrap_or("?");
            let internal = names.contains(dep_name);
            let ok = if internal {
                list.contains(&dep_name)
            } else {
                match layer {
                    Layer::Domain => DOMAIN_EXTERNALS.contains(&dep_name),
                    Layer::Glob => GLOB_EXTERNALS.contains(&dep_name),
                    _ => true,
                }
            };
            if dep_name == GLOBSET && !matches!(layer, Layer::Glob) {
                findings.push(format!("{name} -> {dep_name}: {}", globset_rule()));
                continue;
            }
            if !ok {
                let rule = if dep_name == GLOB && !matches!(layer, Layer::Glob) {
                    glob_user_rule()
                } else {
                    rule.clone()
                };
                findings.push(format!("{name} -> {dep_name}: {rule}"));
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{} workspace crates follow the AD-4 matrix",
            packages.len()
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}
