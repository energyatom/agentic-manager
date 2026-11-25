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

/// Story 3.3, owner condition 2 (AD-11): the engine takes a tool runner
/// only through `am_engine::run::run_plan_with_tools`, a constructor
/// parameter shipped code never passes; am-testkit's `FixtureTool` is its
/// only runner, in tests. Shipped code naming it (am-app's `agentic run`, a
/// `cfg(test)` module, am-engine beside its definition) fails, naming the
/// file and the name; its definition and the tests folders pass.
const ENGINE_HOME: &str = "crates/am-engine/src/run.rs";
const ENGINE_DEFINITION: &str = "pub async fn run_plan(store: &Store, plan: &RunPlan) -> Result<RunOutcome, RunError> {\n    drive(store, plan, None).await\n}\n\n/// Tests only: see the seam guard.\npub async fn run_plan_with_tools(store: &Store, plan: &RunPlan, tools: Arc<dyn ToolRunner>) -> Result<RunOutcome, RunError> {\n    drive(store, plan, Some(tools)).await\n}\n";

#[test]
fn shipped_code_naming_the_engines_tool_seam_fails() {
    for (what, file, source) in [
        (
            "agentic run",
            "crates/am-app/src/run.rs",
            "async fn start() {\n    let _ = am_engine::run::run_plan_with_tools(&store, &plan, tools).await;\n}\n",
        ),
        (
            "a cfg(test) module",
            "crates/am-server/src/lib.rs",
            "#[cfg(test)]\nmod tests {\n    async fn rig() {\n        let _ = am_engine::run::run_plan_with_tools(&s, &p, t).await;\n    }\n}\n",
        ),
        (
            "am-engine beside its definition",
            "crates/am-engine/src/journal.rs",
            "pub async fn resume() {\n    let _ = crate::run::run_plan_with_tools(&s, &p, t).await;\n}\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ENGINE_HOME, ENGINE_DEFINITION);
        write(tmp.path(), file, source);
        let out = guard(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains(file), "{what}: names the file: {err}");
        assert!(
            err.contains("run_plan_with_tools"),
            "{what}: names the seam: {err}"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), ENGINE_HOME, ENGINE_DEFINITION);
    write(
        tmp.path(),
        "crates/am-engine/tests/loop_caps.rs",
        "async fn t() {\n    let _ = am_engine::run::run_plan_with_tools(&s, &p, FixtureTool::new()).await;\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-app/src/run.rs",
        "async fn start() {\n    let _ = am_engine::run::run_plan(&store, &plan).await;\n}\n",
    );
    let out = guard(tmp.path());
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
}

// ---------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P4
// (no literal prefix hides a seam; an unterminated literal fails closed)
// and P5 (the guard reads every `.rs` of a crate but `tests/`, `benches/`
// and `examples/`; `#[path`, `include!` of a `.rs` and `macro_rules!` in
// shipped source are findings; a `ToolRunner` is implemented or used only
// in am-domain (its definition), am-engine (ToolGate) and am-testkit (its
// one runner, `FixtureTool`, a dev-dependency)). Written before the
// patches.

/// P5: a `ToolRunner` implemented or named outside am-domain, am-engine
/// and am-testkit fails the guard, naming the file and `ToolRunner`.
#[test]
fn a_tool_runner_outside_the_domain_and_engine_fails() {
    let allowed = [
        (
            "crates/am-domain/src/tool_manifest.rs",
            "pub trait ToolRunner: Send + Sync {\n    fn run(&self);\n}\n",
        ),
        (
            "crates/am-engine/src/toolgate.rs",
            "use am_domain::ToolRunner;\npub struct ToolGate {\n    runner: Arc<dyn ToolRunner>,\n}\n",
        ),
        (
            "crates/am-testkit/src/lib.rs",
            "impl am_domain::ToolRunner for FixtureTool {\n    fn run(&self) {}\n}\n",
        ),
        (
            "crates/am-e2e/tests/trust/t.rs",
            "impl am_domain::ToolRunner for Rigged {\n    fn run(&self) {}\n}\n",
        ),
    ];
    let tmp = tempfile::tempdir().unwrap();
    for (file, text) in allowed {
        write(tmp.path(), file, text);
    }
    let out = guard(tmp.path());
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));

    for (what, file, source) in [
        (
            "an adapter's implementation",
            "crates/am-tools/src/lib.rs",
            "pub struct Shell;\nimpl am_domain::ToolRunner for Shell {\n    fn run(&self) {}\n}\n",
        ),
        (
            "the composition root's import",
            "crates/am-app/src/run.rs",
            "use am_domain::ToolRunner;\nfn tools() -> Option<Arc<dyn ToolRunner>> {\n    None\n}\n",
        ),
        (
            "a server handler's parameter",
            "crates/am-server/src/lib.rs",
            "fn handle(r: Arc<dyn am_domain::ToolRunner>) {}\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        for (f, text) in allowed {
            write(tmp.path(), f, text);
        }
        write(tmp.path(), file, source);
        let out = guard(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains(file), "{what}: names the file: {err}");
        assert!(
            err.contains("ToolRunner"),
            "{what}: names ToolRunner: {err}"
        );
    }
}

/// P4 and P5: the engine's tool seam is found after a raw C-string literal
/// that holds a backslash, in a crate's `build.rs` and in a `.rs` outside
/// its `src/`.
#[test]
fn the_engines_tool_seam_is_found_after_any_literal_and_outside_src() {
    let call = "pub async fn start() {\n    let _ = am_engine::run::run_plan_with_tools(&s, &p, t).await;\n}\n";
    let after_literal = format!(
        "{}\n{call}",
        r##"pub const S: &core::ffi::CStr = cr#"\"#;"##
    );
    let build = "fn main() {\n    let _ = am_engine::run::run_plan_with_tools;\n}\n";
    for (what, file, source) in [
        (
            "after a raw C string",
            "crates/am-app/src/run.rs",
            after_literal.as_str(),
        ),
        ("in build.rs", "crates/am-app/build.rs", build),
        ("outside src/", "crates/am-app/extra/start.rs", call),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ENGINE_HOME, ENGINE_DEFINITION);
        write(tmp.path(), file, source);
        let out = guard(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains(file), "{what}: names the file: {err}");
        assert!(err.contains("run_plan_with_tools"), "{what}: {err}");
    }
}

/// P4: a literal that never ends fails the guard closed, naming the file.
#[test]
fn an_unterminated_literal_fails_the_guard_closed() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/src/run.rs",
        "pub const S: &str = \"never closed;\npub async fn start() {}\n",
    );
    let out = guard(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "must fail: {err}");
    assert!(err.contains("crates/am-app/src/run.rs"), "{err}");
}

/// P5: `#[path`, `include!` of a `.rs` file and `macro_rules!` in shipped
/// source are findings of their own (each could carry code past the
/// call-site scans), named with the file by `seam-guard` or `check-deps`;
/// `include_str!` and `include_bytes!` of data are not.
#[test]
fn path_include_and_macro_rules_in_shipped_source_are_findings() {
    let deps = |root: &std::path::Path| {
        xtask(&[
            "check-deps",
            "--metadata",
            &common::repo_root()
                .join("xtask/tests/fixtures/metadata/clean.json")
                .to_string_lossy(),
            "--root",
            &root.to_string_lossy(),
        ])
    };
    for (what, source, construct) in [
        (
            "a #[path] module",
            "#[path = \"../extra/inner.rs\"]\nmod inner;\n",
            "#[path",
        ),
        (
            "an include! of a .rs",
            "include!(\"../generated/inner.rs\");\n",
            "include!",
        ),
        (
            "a macro_rules!",
            "macro_rules! wrap {\n    ($t:expr) => {\n        $t\n    };\n}\n",
            "macro_rules!",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-tools/src/lib.rs", source);
        write(
            tmp.path(),
            "crates/am-tools/extra/inner.rs",
            "pub fn f() {}\n",
        );
        write(
            tmp.path(),
            "crates/am-tools/generated/inner.rs",
            "pub fn f() {}\n",
        );
        let said: Vec<String> = [guard(tmp.path()), deps(tmp.path())]
            .into_iter()
            .filter(|o| !o.status.success())
            .map(|o| stderr(&o))
            .collect();
        assert!(
            said.iter()
                .any(|err| err.contains("crates/am-tools/src/lib.rs") && err.contains(construct)),
            "{what}: a finding naming the file and `{construct}`: {said:?}"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-tools/src/lib.rs",
        "const README: &str = include_str!(\"../README.md\");\nconst ICON: &[u8] = include_bytes!(\"../icon.png\");\n",
    );
    let out = guard(tmp.path());
    assert!(out.status.success(), "data includes pass: {}", stderr(&out));
    let out = deps(tmp.path());
    assert!(out.status.success(), "data includes pass: {}", stderr(&out));
}

// ---------------------------------------------------------- review round 2
//
// Story 3.3 review patch round 2 (coordinator's triage, 2026-11-23), from
// the follow-up reviewer's probe trees: R1 (a shebang first line is read
// as rustc reads it), R2 (`#[path`, `include!` and `macro_rules!` are
// found however they are spelled), R3 (only a crate's top-level `dist`,
// `target`, `node_modules` and hidden folders are skipped; a symlink under
// `crates/`, a manifest target or build script outside the scanned files
// and an unreadable `.rs` are findings) and R7 (am-engine names
// `ToolRunner` only in ToolGate). Written before the patches.

/// The stderr of each guard that ran over `root` and failed: seam-guard and
/// check-deps (on the clean synthetic metadata), the two that read every
/// shipped file for call sites and constructs.
fn failures(root: &std::path::Path) -> Vec<String> {
    let deps = xtask(&[
        "check-deps",
        "--metadata",
        &common::repo_root()
            .join("xtask/tests/fixtures/metadata/clean.json")
            .to_string_lossy(),
        "--root",
        &root.to_string_lossy(),
    ]);
    [guard(root), deps]
        .into_iter()
        .filter(|o| !o.status.success())
        .map(|o| stderr(&o))
        .collect()
}

/// A guard fails on `root`, naming `file` and every one of `words`.
fn found(what: &str, root: &std::path::Path, file: &str, words: &[&str]) {
    let said = failures(root);
    assert!(
        said.iter()
            .any(|err| err.contains(file) && words.iter().all(|w| err.contains(w))),
        "{what}: a finding naming {file} and {words:?}; found {said:#?}"
    );
}

/// No guard fails on `root`.
fn clean(what: &str, root: &std::path::Path) {
    let said = failures(root);
    assert!(said.is_empty(), "{what}: no finding; found {said:#?}");
}

const RUNNER_USE: &str = "use am_domain::ToolRunner;\n";

/// R1: rustc drops a first line that starts with `#!` and is not an inner
/// attribute (`#![…]`, `#! […]`); the scans do the same, so a quote on a
/// shebang line hides nothing. An inner attribute on line 1 is still code.
#[test]
fn a_shebang_line_hides_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/src/a_shebang.rs",
        "#!/x \"\nuse am_domain::ToolRunner; // \"\nfn main() {}\n",
    );
    found(
        "after a shebang",
        tmp.path(),
        "crates/am-app/src/a_shebang.rs",
        &["ToolRunner"],
    );

    for (what, source) in [
        (
            "an inner attribute on line 1",
            "#![forbid(unsafe_code)]\nuse am_domain::ToolRunner;\n",
        ),
        (
            "an inner attribute with a space",
            "#! [forbid(unsafe_code)]\nuse am_domain::ToolRunner;\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-app/src/attr.rs", source);
        found(
            what,
            tmp.path(),
            "crates/am-app/src/attr.rs",
            &["ToolRunner"],
        );
    }
}

/// R2: `#[path`, `include!` and `macro_rules!` are found however they are
/// spelled: `path` inside a `cfg_attr`, an attribute split over lines,
/// `#[r#path`, `include` or `macro_rules` with `!` on the next line,
/// `include` imported under another name (or at all). The spellings the
/// scan already finds stay found.
#[test]
fn path_include_and_macro_rules_are_found_however_spelled() {
    for (what, source, word) in [
        (
            "path in a cfg_attr",
            "#[cfg_attr(all(), path = \"../tests/hid/mod.rs\")]\nmod hid;\n",
            "path",
        ),
        (
            "an attribute split over lines",
            "#[\npath = \"x.rs\"]\nmod y;\n",
            "path",
        ),
        (
            "a raw-identifier path",
            "#[r#path = \"../tests/x.rs\"]\nmod yy;\n",
            "path",
        ),
        (
            "include renamed",
            "use core::include as grab;\ngrab!(\"../tests/hid/mod.rs\");\n",
            "include",
        ),
        ("include imported", "use std::include;\n", "include"),
        (
            "include with ! on the next line",
            "include\n!(\"../tests/hid/mod.rs\");\n",
            "include",
        ),
        (
            "macro_rules with ! on the next line",
            "macro_rules\n! m2 { () => {} }\n",
            "macro_rules",
        ),
        // Already found: regression guards.
        (
            "a plain #[path]",
            "#[path = \"../tests/x.rs\"]\nmod zz;\n",
            "path",
        ),
        (
            "macro_rules with a space",
            "macro_rules ! m1 { () => {} }\n",
            "macro_rules",
        ),
        (
            "a raw-identifier macro_rules",
            "r#macro_rules! m3 { () => {} }\n",
            "macro_rules",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-app/src/probe.rs", source);
        found(what, tmp.path(), "crates/am-app/src/probe.rs", &[word]);
    }

    // Ordinary code that only uses the words is not a finding.
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/src/ordinary.rs",
        "#[derive(Debug)]\npub struct Opts {\n    pub path: String,\n}\n#[doc = \"see path and include\"]\npub fn include_all(path: &str) -> usize {\n    let macro_rules_count = path.len();\n    macro_rules_count\n}\n",
    );
    clean("ordinary uses of the words", tmp.path());
}

/// R3: only a crate's top-level `dist`, `target`, `node_modules` and hidden
/// folders are skipped (build output); the same names deeper in the crate
/// are read like any folder.
#[test]
fn only_top_level_build_folders_are_skipped() {
    for file in [
        "crates/am-app/src/dist/mod.rs",
        "crates/am-app/src/target/x.rs",
        "crates/am-app/src/node_modules/x.rs",
        "crates/am-app/src/.h/x.rs",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), file, RUNNER_USE);
        found(file, tmp.path(), file, &["ToolRunner"]);
    }

    let tmp = tempfile::tempdir().unwrap();
    for file in [
        "crates/am-app/target/debug/build/x.rs",
        "crates/am-app/dist/x.rs",
        "crates/am-app/node_modules/x.rs",
        "crates/am-app/.cache/x.rs",
    ] {
        write(tmp.path(), file, RUNNER_USE);
    }
    clean("a crate's top-level build output", tmp.path());
}

/// R3: a symlink under `crates/` (to a folder or to a file) is a finding
/// naming the link: the scans read the files where they are, and a link
/// could bring an unread file into the build.
#[cfg(unix)]
#[test]
fn a_symlink_under_crates_is_a_finding() {
    for (link, target) in [
        ("crates/am-app/src/linkdir", "../tests/hid"),
        ("crates/am-app/src/linkfile.rs", "../tests/hid/mod.rs"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-app/tests/hid/mod.rs", RUNNER_USE);
        write(
            tmp.path(),
            "crates/am-app/src/lib.rs",
            "mod linkdir;\nmod linkfile;\n",
        );
        std::os::unix::fs::symlink(target, tmp.path().join(link)).unwrap();
        found(link, tmp.path(), link, &[]);
    }
}

/// R3: a crate manifest whose `[lib]` or `[[bin]]` `path`, or whose
/// `build` key, names a file outside the files the scans read is a
/// finding naming the manifest and the key. The default places pass.
#[test]
fn a_manifest_target_outside_the_scanned_files_is_a_finding() {
    for (what, manifest, key) in [
        (
            "[lib] path in tests/",
            "[package]\nname = \"am-app\"\n\n[lib]\npath = \"tests/hid/lib.rs\"\n",
            "path",
        ),
        (
            "[[bin]] path in examples/",
            "[package]\nname = \"am-app\"\n\n[[bin]]\nname = \"x\"\npath = \"examples/x.rs\"\n",
            "path",
        ),
        (
            "build outside the crate",
            "[package]\nname = \"am-app\"\nbuild = \"../../tools/build.rs\"\n",
            "build",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-app/Cargo.toml", manifest);
        write(tmp.path(), "crates/am-app/src/lib.rs", "pub fn f() {}\n");
        write(tmp.path(), "crates/am-app/tests/hid/lib.rs", RUNNER_USE);
        write(tmp.path(), "crates/am-app/examples/x.rs", RUNNER_USE);
        write(tmp.path(), "tools/build.rs", RUNNER_USE);
        found(what, tmp.path(), "crates/am-app/Cargo.toml", &[key]);
    }

    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/Cargo.toml",
        "[package]\nname = \"am-app\"\nbuild = \"build.rs\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[[bin]]\nname = \"agentic\"\npath = \"src/main.rs\"\n",
    );
    write(tmp.path(), "crates/am-app/src/lib.rs", "pub fn f() {}\n");
    write(tmp.path(), "crates/am-app/src/main.rs", "fn main() {}\n");
    write(tmp.path(), "crates/am-app/build.rs", "fn main() {}\n");
    clean("targets in their default places", tmp.path());
}

/// R3: a `.rs` the scans cannot read is a finding naming it, never a file
/// silently left out.
#[cfg(unix)]
#[test]
fn an_unreadable_source_file_is_a_finding() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/am-app/src/secret.rs", RUNNER_USE);
    let path = tmp.path().join("crates/am-app/src/secret.rs");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(&path).is_ok() {
        // Running as a user who reads anything (root): nothing to show.
        return;
    }
    found(
        "an unreadable .rs",
        tmp.path(),
        "crates/am-app/src/secret.rs",
        &[],
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

/// R7: in am-engine, only ToolGate (`toolgate.rs`) names `ToolRunner`;
/// `run.rs` may name it only in `run_plan_with_tools`'s signature, the
/// test-only seam that hands a runner to a gate it builds (and in the `use`
/// that signature needs). `run_on`, `Tooling` and the rest of the driver
/// hold a gate, never a runner. Checked on the repository's own source.
#[test]
fn am_engine_names_tool_runner_only_in_toolgate() {
    let src = common::repo_root().join("crates/am-engine/src");
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name == "toolgate.rs" || path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // Comments removed, line by line.
        let lines: Vec<String> = text
            .lines()
            .map(|l| match l.find("//") {
                Some(i) => l[..i].to_string(),
                None => l.to_string(),
            })
            .collect();
        let mut in_use = false;
        let mut in_signature = false;
        let (mut in_uses, mut in_signatures) = (0, 0);
        for (n, line) in lines.iter().enumerate() {
            let t = line.trim();
            if t.starts_with("use ") || t.starts_with("pub use ") {
                in_use = true;
            }
            if t.contains("fn run_plan_with_tools") {
                in_signature = true;
            }
            let count = line.matches("ToolRunner").count();
            if count > 0 {
                assert!(
                    in_use || in_signature,
                    "{name}:{}: names ToolRunner outside ToolGate: `{}`",
                    n + 1,
                    t
                );
                if in_use {
                    in_uses += count;
                } else {
                    in_signatures += count;
                }
            }
            if in_use && t.ends_with(';') {
                in_use = false;
            }
            if in_signature && t.contains('{') {
                in_signature = false;
            }
        }
        assert!(
            in_uses <= 1 && in_signatures <= 1,
            "{name}: ToolRunner named {in_uses} time(s) in imports and {in_signatures} time(s) in run_plan_with_tools' signature; one each at most"
        );
    }
}

// ------------------------------ story 3.23 review round 1 (R3, ToolGate)
//
// The ToolGate slice's review triage round 1 (manager, 2026-11-25), R3:
// `ToolGate::shipped`, `ToolGate::admit` and `ToolGate::run`, and
// `StoreRowWriter::new`, are public, so shipped code could build a gate or
// a row writer and forge a step's facts (`StepTools`, `seat_team`) or a
// call's place (`CallPlace`). The mechanism (tester's choice, recorded in
// the plan): seam-guard pins the names. In shipped code, `ToolGate` is
// named only in `crates/am-engine/src/toolgate.rs` (its definition) and
// `crates/am-engine/src/run.rs` (the one user); `StoreRowWriter` only in
// `crates/am-engine/src/row_writer.rs` and `run.rs`; and a call of
// `.admit(` stands only in `run.rs` and `toolgate.rs`. A gate's `run`
// needs a `Permit`, which only `admit` makes, on a gate reached only where
// `ToolGate` is named. Tests, comments and literals are not shipped code.
// The tests accept a finding from either guard (seam-guard or check-deps,
// `failures` above), naming the file and one of the names. Written before
// the patch.

/// The allowed sites: the definitions, the engine's driver (which builds
/// the shipped gate over the store's row writer, admits and runs), tests
/// that call everything, and a comment and a literal in am-app.
const GATE_SITES: [(&str, &str); 7] = [
    (
        "crates/am-engine/src/toolgate.rs",
        "use std::sync::Arc;\nuse am_domain::rows::{RowTool, RowWrite, RowWriter};\npub struct Permit {\n    gate: u64,\n}\npub struct ToolGate {\n    rows: Arc<dyn RowWriter>,\n}\nimpl ToolGate {\n    pub fn shipped(rows: Arc<dyn RowWriter>, tool: Option<Arc<dyn RowTool>>, audit: Arc<dyn AuditSink>) -> ToolGate {\n        ToolGate { rows }\n    }\n    pub fn admit(&self, actor: &Actor, step: &StepTools, place: &CallPlace, call: &ToolCall) -> Result<Permit, String> {\n        Ok(Permit { gate: 1 })\n    }\n    pub fn run(&self, permit: Permit, actor: Actor, write: RowWrite) -> bool {\n        self.write(actor, write)\n    }\n    fn write(&self, actor: Actor, write: RowWrite) -> bool {\n        self.rows.write_row(actor, write).is_ok()\n    }\n}\n#[cfg(test)]\nmod tests {\n    fn t(g: &super::ToolGate, a: &Actor, s: &StepTools, p: &CallPlace, c: &ToolCall) {\n        let _ = g.admit(a, s, p, c);\n    }\n}\n",
    ),
    (
        "crates/am-engine/src/row_writer.rs",
        "use am_domain::rows::{RowWrite, RowWriteError, RowWriter, RowWritten};\nuse am_domain::Actor;\npub struct StoreRowWriter {\n    writer: am_store::Writer,\n}\nimpl StoreRowWriter {\n    pub fn new(store: &am_store::Store) -> StoreRowWriter {\n        StoreRowWriter { writer: store.writer() }\n    }\n}\nimpl RowWriter for StoreRowWriter {\n    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {\n        self.writer.apply_row_write(actor, write).map_err(refused)?;\n        todo!()\n    }\n}\n",
    ),
    (
        "crates/am-engine/src/run.rs",
        "use std::sync::Arc;\nuse crate::row_writer::StoreRowWriter;\nuse crate::toolgate::{CallPlace, StepTools, ToolGate};\nfn tooling(store: &am_store::Store, audit: Arc<dyn AuditSink>) -> ToolGate {\n    ToolGate::shipped(Arc::new(StoreRowWriter::new(store)), None, audit)\n}\nfn call(gate: &ToolGate, a: &Actor, s: &StepTools, p: &CallPlace, c: &ToolCall, w: RowWrite) -> bool {\n    match gate.admit(a, s, p, c) {\n        Ok(permit) => gate.run(permit, a.clone(), w),\n        Err(_) => false,\n    }\n}\n",
    ),
    (
        "crates/am-engine/src/lib.rs",
        "pub mod row_writer;\npub mod run;\npub mod toolgate;\n",
    ),
    (
        "crates/am-engine/tests/toolgate.rs",
        "fn t(rows: Arc<dyn RowWriter>, a: &Actor, s: &StepTools, p: &CallPlace, c: &ToolCall, w: RowWrite) {\n    let gate = am_engine::toolgate::ToolGate::shipped(rows, None, CollectAudit::new());\n    let permit = gate.admit(a, s, p, c).unwrap();\n    let _ = gate.run(permit, a.clone(), w);\n}\n",
    ),
    (
        "crates/am-e2e/tests/engine/rows_idempotent.rs",
        "fn t(store: &am_store::Store) {\n    let _ = am_engine::row_writer::StoreRowWriter::new(store);\n}\n",
    ),
    (
        "crates/am-app/src/run.rs",
        "// agentic run never builds a ToolGate or a StoreRowWriter; am-engine's run.rs does.\nconst WHY: &str = \"ToolGate::shipped, gate.admit( and StoreRowWriter::new are the engine's\";\n",
    ),
];

/// A tree of [`GATE_SITES`] with `(file, source)` added or put in place of
/// the file at the same path.
fn gate_tree(file: &str, source: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in GATE_SITES.iter().filter(|(f, _)| *f != file) {
        write(tmp.path(), f, text);
    }
    if !file.is_empty() {
        write(tmp.path(), file, source);
    }
    tmp
}

/// R3: the allowed sites pass both guards. Each planted site fails one of
/// them, with a finding naming the file and one of `names`: am-app building
/// the shipped gate; a server handler admitting a call on a gate it is
/// handed; another am-engine file building a gate; `admit` called on a gate
/// reached without naming `ToolGate`; `run` called on an aliased gate;
/// am-app building the row writer; `StoreRowWriter :: new` spaced; and
/// `ToolGate` with `::shipped` on the next line.
#[test]
fn only_the_engines_driver_builds_admits_and_runs_a_gate() {
    let tmp = gate_tree("", "");
    clean(
        "the definitions, the driver, tests, comments and literals",
        tmp.path(),
    );

    for (what, file, source, names) in [
        (
            "agentic run builds the shipped gate",
            "crates/am-app/src/run.rs",
            "fn gate(rows: Arc<dyn RowWriter>, audit: Arc<dyn AuditSink>) -> am_engine::toolgate::ToolGate {\n    am_engine::toolgate::ToolGate::shipped(rows, None, audit)\n}\n",
            &["ToolGate"][..],
        ),
        (
            "a server handler admits a call",
            "crates/am-server/src/runs.rs",
            "fn decide(gate: &am_engine::toolgate::ToolGate, a: &Actor, s: &StepTools, p: &CallPlace, c: &ToolCall) -> bool {\n    gate.admit(a, s, p, c).is_ok()\n}\n",
            &["ToolGate", "admit"][..],
        ),
        (
            "another am-engine file builds a gate",
            "crates/am-engine/src/scheduler.rs",
            "fn gate(rows: Arc<dyn RowWriter>, audit: Arc<dyn AuditSink>) -> crate::toolgate::ToolGate {\n    crate::toolgate::ToolGate::shipped(rows, None, audit)\n}\n",
            &["ToolGate"][..],
        ),
        (
            "admit on a gate reached without naming it",
            "crates/am-engine/src/journal.rs",
            "fn decide(t: &crate::run::Tooling, a: &Actor, s: &StepTools, p: &CallPlace, c: &ToolCall) -> bool {\n    t.gate.admit(a, s, p, c).is_ok()\n}\n",
            &["admit"][..],
        ),
        (
            "run on an aliased gate",
            "crates/am-app/src/gate.rs",
            "use am_engine::toolgate::ToolGate as Gate;\nfn go(g: &Gate, p: am_engine::toolgate::Permit, a: Actor, w: RowWrite) -> bool {\n    g.run(p, a, w)\n}\n",
            &["ToolGate"][..],
        ),
        (
            "agentic run builds the row writer",
            "crates/am-app/src/run.rs",
            "fn rows(store: &am_store::Store) -> Arc<dyn RowWriter> {\n    Arc::new(am_engine::row_writer::StoreRowWriter::new(store))\n}\n",
            &["StoreRowWriter"][..],
        ),
        (
            "StoreRowWriter :: new, spaced",
            "crates/am-engine/src/gate.rs",
            "fn rows(store: &am_store::Store) -> Arc<dyn RowWriter> {\n    Arc::new(crate::row_writer::StoreRowWriter :: new(store))\n}\n",
            &["StoreRowWriter"][..],
        ),
        (
            "ToolGate, then ::shipped on the next line",
            "crates/am-server/src/inbox.rs",
            "fn gate(r: Arc<dyn RowWriter>, a: Arc<dyn AuditSink>) {\n    let _ = am_engine::toolgate::ToolGate\n        ::shipped(r, None, a);\n}\n",
            &["ToolGate"][..],
        ),
    ] {
        let tmp = gate_tree(file, source);
        let said = failures(tmp.path());
        assert!(
            said.iter()
                .any(|err| err.contains(file) && names.iter().any(|n| err.contains(n))),
            "{what}: a finding naming {file} and one of {names:?}; found {said:#?}"
        );
    }
}

/// R3 on the repository: no shipped file but `toolgate.rs` and `run.rs`
/// names `ToolGate` or calls `.admit(`, and none but `row_writer.rs` and
/// `run.rs` names `StoreRowWriter` (comment lines left out); both guards
/// pass on the real workspace.
#[test]
fn the_repository_builds_gates_only_in_the_engines_driver() {
    let root = common::repo_root();
    let mut found = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let mut stack = vec![krate.path().join("src")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let rel = p
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let code: String = std::fs::read_to_string(&p)
                    .unwrap()
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let engine = |name: &str| rel == format!("crates/am-engine/src/{name}");
                if code.contains("ToolGate") && !engine("toolgate.rs") && !engine("run.rs") {
                    found.push(format!("{rel}: ToolGate"));
                }
                if code.contains(".admit(") && !engine("toolgate.rs") && !engine("run.rs") {
                    found.push(format!("{rel}: admit"));
                }
                if code.contains("StoreRowWriter") && !engine("row_writer.rs") && !engine("run.rs")
                {
                    found.push(format!("{rel}: StoreRowWriter"));
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
    clean("the repository", &root);
}
