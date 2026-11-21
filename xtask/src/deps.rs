//! `check-deps`: the AD-4 dependency matrix (architecture, Design Paradigm
//! table) checked against `cargo metadata`. Only normal and build edges count;
//! dev-dependencies are test scaffolding. Every workspace crate must be
//! classified here, so a new crate cannot slip past the matrix.

use crate::Failure;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

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
    check(&doc)
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
