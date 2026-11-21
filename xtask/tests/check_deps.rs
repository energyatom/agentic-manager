//! `cargo xtask check-deps` against the AD-4 matrix (story 1.2, written first).
#![forbid(unsafe_code)]

mod common;

use common::{repo_root, stderr, xtask};

fn metadata(name: &str) -> String {
    repo_root()
        .join("xtask/tests/fixtures/metadata")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn adapter_to_adapter_edge_fails_naming_both_crates_and_the_rule() {
    let out = xtask(&["check-deps", "--metadata", &metadata("adapter_edge.json")]);
    let err = stderr(&out);
    assert!(!out.status.success(), "an adapter edge must fail: {err}");
    assert!(err.contains("am-mcp"), "names the source crate: {err}");
    assert!(
        err.contains("am-providers"),
        "names the target crate: {err}"
    );
    assert!(
        err.contains("adapters may depend only on am-domain and am-net (AD-4)"),
        "states the rule: {err}"
    );
}

#[test]
fn io_crate_in_the_domain_fails_naming_the_crate_and_the_rule() {
    let out = xtask(&["check-deps", "--metadata", &metadata("domain_tokio.json")]);
    let err = stderr(&out);
    assert!(!out.status.success(), "tokio in am-domain must fail: {err}");
    assert!(err.contains("am-domain"), "names the crate: {err}");
    assert!(err.contains("tokio"), "names the dependency: {err}");
    assert!(
        err.contains("am-domain may depend only on std, serde, thiserror, jiff and uuid (AD-4)"),
        "states the rule: {err}"
    );
}

#[test]
fn a_crate_missing_from_the_matrix_fails() {
    let out = xtask(&[
        "check-deps",
        "--metadata",
        &metadata("unclassified_crate.json"),
    ]);
    let err = stderr(&out);
    assert!(
        !out.status.success(),
        "an unclassified crate must fail: {err}"
    );
    assert!(err.contains("am-widgets"), "names the crate: {err}");
}

#[test]
fn clean_synthetic_graph_passes() {
    let out = xtask(&["check-deps", "--metadata", &metadata("clean.json")]);
    assert!(out.status.success(), "clean graph passes: {}", stderr(&out));
}

#[test]
fn current_workspace_passes() {
    let out = xtask(&["check-deps"]);
    assert!(
        out.status.success(),
        "the real workspace graph passes: {}",
        stderr(&out)
    );
}

// ---------------------------------------------------------------- story 1.14
//
// am-glob, the one glob matcher (architecture Design Paradigm table, amended
// 2026-11-20): it may depend on no internal crate and on no external crate
// but globset and unicode-normalization; only am-config, am-engine, am-tools
// and am-connectors may depend on it.

#[test]
fn am_glob_and_its_four_users_pass() {
    let out = xtask(&["check-deps", "--metadata", &metadata("glob_ok.json")]);
    assert!(
        out.status.success(),
        "am-glob on globset and unicode-normalization, used by am-config, am-engine, am-tools and am-connectors: {}",
        stderr(&out)
    );
}

#[test]
fn an_am_glob_to_am_domain_edge_fails_naming_the_rule() {
    let out = xtask(&["check-deps", "--metadata", &metadata("glob_domain.json")]);
    let err = stderr(&out);
    assert!(
        !out.status.success(),
        "am-glob -> am-domain must fail: {err}"
    );
    assert!(
        err.contains("am-glob -> am-domain"),
        "names the edge: {err}"
    );
    assert!(
        err.contains("am-glob may depend on no workspace crate"),
        "states the rule: {err}"
    );
    assert!(err.contains("(AD-4)"), "{err}");
    assert!(
        !err.contains("am-config -> am-glob"),
        "the allowed users are not findings: {err}"
    );
}

#[test]
fn an_am_glob_external_other_than_its_two_fails() {
    let out = xtask(&["check-deps", "--metadata", &metadata("glob_external.json")]);
    let err = stderr(&out);
    assert!(!out.status.success(), "am-glob -> regex must fail: {err}");
    assert!(err.contains("am-glob -> regex"), "names the edge: {err}");
    assert!(
        err.contains("globset") && err.contains("unicode-normalization"),
        "names the two allowed crates: {err}"
    );
    assert!(!err.contains("am-glob -> globset"), "{err}");
    assert!(!err.contains("am-glob -> unicode-normalization"), "{err}");
}

#[test]
fn a_crate_outside_the_four_users_may_not_depend_on_am_glob() {
    let out = xtask(&["check-deps", "--metadata", &metadata("glob_user.json")]);
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    for edge in [
        "am-store -> am-glob",
        "am-net -> am-glob",
        "am-server -> am-glob",
    ] {
        assert!(err.contains(edge), "names {edge}: {err}");
    }
    for allowed in [
        "am-config -> am-glob",
        "am-engine -> am-glob",
        "am-tools -> am-glob",
        "am-connectors -> am-glob",
    ] {
        assert!(!err.contains(allowed), "{allowed} is allowed: {err}");
    }
}

/// The workspace members named in the root manifest's `members = [..]`.
fn workspace_members() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    let start = text.find("\nmembers = [").expect("a members list") + 1;
    let list = &text[start..];
    let list = &list[..list.find(']').expect("the members list closes")];
    list.lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| l.starts_with('"'))
        .map(|l| l.trim_end_matches(',').trim_matches('"').to_string())
        .collect()
}

/// True when a table header names a dependency table: `[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]`, `[target.<cfg>.…]` forms,
/// and a dotted `[….dependencies.<name>]` table.
fn is_dependency_table(header: &str) -> bool {
    let inner = header.trim_start_matches('[').trim_end_matches(']');
    inner.split('.').any(|part| {
        matches!(
            part.trim().trim_matches('"'),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    })
}

/// True when the manifest names globset in any dependency table, as a key,
/// as a dotted table (`[dependencies.globset]`) or through `package =`.
fn names_globset(manifest: &str) -> bool {
    let mut inside = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = is_dependency_table(t);
            if inside && t.trim_end_matches(']').ends_with(".globset") {
                return true;
            }
            continue;
        }
        if !inside {
            continue;
        }
        let key = t
            .split(['=', '.', ' '])
            .next()
            .unwrap_or("")
            .trim_matches('"');
        let renamed = t.replace(' ', "").contains("package=\"globset\"");
        if key == "globset" || renamed {
            return true;
        }
    }
    false
}

/// No workspace member but am-glob names globset in any dependency table
/// (normal, dev, build or target-specific), xtask and the exempt test crates
/// included (Consistency Conventions: Globs).
#[test]
fn only_am_glob_depends_on_globset() {
    let members = workspace_members();
    assert!(members.iter().any(|m| m == "xtask"), "{members:?}");
    let mut users = Vec::new();
    for member in &members {
        let manifest = repo_root().join(member).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
        if names_globset(&text) {
            users.push(member.clone());
        }
    }
    assert_eq!(users, ["crates/am-glob"]);
}

/// The manifest scan finds globset in every kind of dependency table.
#[test]
fn the_globset_scan_reads_every_dependency_table() {
    for manifest in [
        "[dependencies]\nglobset.workspace = true\n",
        "[dev-dependencies]\nglobset = \"0.4\"\n",
        "[build-dependencies]\nglobset = { workspace = true }\n",
        "[target.'cfg(unix)'.dependencies]\nglobset.workspace = true\n",
        "[target.\"cfg(unix)\".dev-dependencies]\nglobset = \"0.4\"\n",
        "[dependencies.globset]\nworkspace = true\n",
        "[dependencies]\nglobs = { package = \"globset\", version = \"0.4\" }\n",
    ] {
        assert!(names_globset(manifest), "{manifest}");
    }
    for manifest in [
        "[package]\nname = \"globset\"\n",
        "[dependencies]\nam-glob.workspace = true\n",
        "[features]\nglobset = []\n",
    ] {
        assert!(!names_globset(manifest), "{manifest}");
    }
}

/// check-deps refuses a globset edge from a record crate (am-config), an
/// adapter (am-net) and a composition root (am-app), naming the rule, and
/// reports no edge from am-glob itself.
#[test]
fn a_globset_edge_outside_am_glob_fails_naming_the_rule() {
    let out = xtask(&["check-deps", "--metadata", &metadata("globset_user.json")]);
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    for edge in [
        "am-config -> globset",
        "am-net -> globset",
        "am-app -> globset",
    ] {
        assert!(err.contains(edge), "names {edge}: {err}");
    }
    assert!(
        err.contains("only am-glob may depend on globset"),
        "states the rule: {err}"
    );
    assert!(!err.contains("am-glob -> globset"), "{err}");
}
