//! `cargo xtask seam-guard` (story 2.2, decision 7): the egress client's
//! test seams (a resolver and a per-hop transport) are parameters of a
//! second constructor, `NetEgress::with_seams`, used only by tests. Shipped
//! code never names it: the guard fails when `with_seams` appears in any
//! `crates/*/src` but am-net's own (where it is defined), naming the file
//! and line. `NetEgress::new` always uses the system resolver and reqwest,
//! and no env var, flag or feature changes either. Written first.
#![forbid(unsafe_code)]

mod common;

use common::{stderr, stdout, write, xtask};

fn guard(root: &std::path::Path) -> std::process::Output {
    xtask(&["seam-guard", "--root", &root.to_string_lossy()])
}

const DEFINITION: &str = "pub struct NetEgress;\nimpl NetEgress {\n    /// Tests only: see `with_seams`.\n    pub fn with_seams() -> NetEgress {\n        NetEgress\n    }\n}\n";

#[test]
fn a_source_file_naming_the_test_constructor_fails() {
    for (what, file, source) in [
        (
            "a call",
            "crates/am-app/src/lib.rs",
            "pub fn egress() {\n    let _ = am_net::NetEgress::with_seams();\n}\n",
        ),
        (
            "a cfg(test) module",
            "crates/am-providers/src/lib.rs",
            "#[cfg(test)]\nmod tests {\n    fn rig() {\n        let _ = am_net::NetEgress::with_seams();\n    }\n}\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-net/src/lib.rs", DEFINITION);
        write(tmp.path(), file, source);
        let out = guard(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains(file), "{what}: names the file: {err}");
        assert!(
            err.contains("with_seams"),
            "{what}: names the constructor: {err}"
        );
    }
}

#[test]
fn the_definition_in_am_net_and_tests_folders_pass() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/am-net/src/lib.rs", DEFINITION);
    write(
        tmp.path(),
        "crates/am-e2e/tests/trust/support/egress.rs",
        "fn rig() {\n    let _ = am_net::NetEgress::with_seams();\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-app/src/lib.rs",
        "pub fn egress() -> am_net::NetEgress {\n    am_net::NetEgress::new()\n}\n",
    );
    let out = guard(tmp.path());
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
}

#[test]
fn the_repository_never_names_the_test_constructor_in_shipped_code() {
    let out = xtask(&["seam-guard"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
}

/// Story 2.2 review (low): inside am-net only the definition is skipped:
/// shipped am-net code that calls `with_seams` fails the guard.
#[test]
fn shipped_am_net_code_calling_the_test_constructor_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/am-net/src/lib.rs", DEFINITION);
    write(
        tmp.path(),
        "crates/am-net/src/default.rs",
        "pub fn egress() -> super::NetEgress {\n    super::NetEgress::with_seams()\n}\n",
    );
    let out = guard(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "must fail: {err}");
    assert!(err.contains("crates/am-net/src/default.rs"), "{err}");

    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-net/src/lib.rs",
        &format!("{DEFINITION}pub fn egress() -> NetEgress {{\n    NetEgress::with_seams()\n}}\n"),
    );
    let out = guard(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "a call beside the definition: {err}");
    assert!(err.contains("crates/am-net/src/lib.rs"), "{err}");
}
