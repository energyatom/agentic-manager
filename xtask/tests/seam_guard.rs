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

/// Story 3.2 (AD-15): am-server's test-only fixture route. Its definition
/// file, `crates/am-server/src/fixture.rs`.
const FIXTURE_HOME: &str = "crates/am-server/src/fixture.rs";
const FIXTURE_DEFINITION: &str = "pub struct FixtureRoute;\nimpl FixtureRoute {\n    pub fn new() -> FixtureRoute {\n        FixtureRoute\n    }\n}\npub fn router_with_fixture(state: AppState, fixture: FixtureRoute) -> Router {\n    build(state, fixture)\n}\n";

/// Shipped code naming `router_with_fixture` or `FixtureRoute` outside
/// its definition file fails, naming the file and the name: am-app's
/// `agentic serve`, am-server's own router, and a `cfg(test)` module.
#[test]
fn shipped_code_naming_the_fixture_route_fails() {
    for (what, file, source, name) in [
        (
            "agentic serve",
            "crates/am-app/src/main.rs",
            "async fn serve() {\n    let r = am_server::router_with_fixture(state, am_server::FixtureRoute::new());\n}\n",
            "router_with_fixture",
        ),
        (
            "the shipped router",
            "crates/am-server/src/routes.rs",
            "pub fn router(state: AppState) -> Router {\n    crate::router_with_fixture(state, Default::default())\n}\n",
            "router_with_fixture",
        ),
        (
            "the fixture type alone",
            "crates/am-app/src/serving.rs",
            "fn extra() -> am_server::FixtureRoute {\n    Default::default()\n}\n",
            "FixtureRoute",
        ),
        (
            "a cfg(test) module",
            "crates/am-server/src/lib.rs",
            "#[cfg(test)]\nmod tests {\n    fn rig() {\n        let _ = super::FixtureRoute::new();\n    }\n}\n",
            "FixtureRoute",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), FIXTURE_HOME, FIXTURE_DEFINITION);
        write(tmp.path(), file, source);
        let out = guard(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains(file), "{what}: names the file: {err}");
        assert!(err.contains(name), "{what}: names {name}: {err}");
    }
}

/// The definition file, a glob re-export and the tests folders pass; a
/// call of `router_with_fixture` beside its definition fails.
#[test]
fn the_fixture_definition_and_tests_pass_and_a_call_beside_it_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), FIXTURE_HOME, FIXTURE_DEFINITION);
    write(
        tmp.path(),
        "crates/am-server/src/lib.rs",
        "mod fixture;\npub use fixture::*;\npub fn router(state: AppState) -> Router {\n    build(state)\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-e2e/tests/trust/support/server.rs",
        "fn rig() {\n    let _ = am_server::router_with_fixture(s, am_server::FixtureRoute::new());\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-server/tests/route_table.rs",
        "fn rig() {\n    let _ = am_server::router_with_fixture(s, am_server::FixtureRoute::new());\n}\n",
    );
    let out = guard(tmp.path());
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));

    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        FIXTURE_HOME,
        &format!(
            "{FIXTURE_DEFINITION}pub fn shipped(s: AppState) -> Router {{\n    router_with_fixture(s, FixtureRoute::new())\n}}\n"
        ),
    );
    let out = guard(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "a call beside the definition: {err}");
    assert!(err.contains(FIXTURE_HOME), "{err}");
}
