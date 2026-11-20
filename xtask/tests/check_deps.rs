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
