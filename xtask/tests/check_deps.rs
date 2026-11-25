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

// ---------------------------------------------------------------- story 3.3
//
// AD-10 (amended 2026-11-23): `TrustedInput`'s two named constructors,
// `TrustedInput::from_step_output` and `TrustedInput::from_owner_note`, are
// called only by am-engine. `check-deps` scans the shipped source
// (`crates/*/src`, comments and literals left out) under `--root` (the repo
// root by default) besides the metadata: naming either constructor is
// allowed in `crates/am-engine/src/` and, in am-domain, only on its
// definition line; anywhere else in shipped code (am-domain's other lines
// included) it fails, naming the file and line. Tests are not shipped.

/// am-domain's definition of the two constructors, with a doc example that
/// names one (a comment, so not a call).
const TRUSTED_INPUT: &str = "pub struct TrustedInput { label: String, text: String }\nimpl TrustedInput {\n    /// ```\n    /// let p = TrustedInput::from_step_output(\"step:a@1\", \"x\", Taint::Clean);\n    /// ```\n    pub fn from_step_output(label: impl Into<String>, text: impl Into<String>, producer: Taint) -> Part {\n        todo!()\n    }\n    pub fn from_owner_note(label: impl Into<String>, text: impl Into<String>, author: &Actor) -> Option<TrustedInput> {\n        todo!()\n    }\n}\n";

fn deps_at(root: &std::path::Path) -> std::process::Output {
    xtask(&[
        "check-deps",
        "--metadata",
        &metadata("clean.json"),
        "--root",
        &root.to_string_lossy(),
    ])
}

#[test]
fn trusted_input_constructors_are_called_only_from_am_engine() {
    use common::write;

    // Allowed: the definition, am-engine's calls, tests, and the words in
    // comments and literals.
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/am-domain/src/content.rs", TRUSTED_INPUT);
    write(
        tmp.path(),
        "crates/am-engine/src/run.rs",
        "fn input(row: &Row) -> Part {\n    am_domain::TrustedInput::from_step_output(label(row), row.output(), row.taint())\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-engine/src/gate.rs",
        "use am_domain::TrustedInput;\nfn note(t: &str, a: &Actor) -> Option<TrustedInput> {\n    TrustedInput::from_owner_note(\"note:return@1\", t, a)\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-e2e/tests/trust/taint_trusted_input_sources.rs",
        "fn t() {\n    let _ = am_domain::TrustedInput::from_step_output(\"step:a@1\", \"x\", Taint::Clean);\n}\n",
    );
    write(
        tmp.path(),
        "crates/am-providers/src/common.rs",
        "// Rendered as a TrustedInput::from_step_output part would be.\nconst WHY: &str = \"TrustedInput::from_owner_note is am-engine's\";\n",
    );
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the definition, am-engine and tests pass: {}",
        stderr(&out)
    );

    // Refused: any other shipped caller, a call beside the definition and a
    // renamed import.
    for (what, file, source, name) in [
        (
            "an adapter",
            "crates/am-tools/src/lib.rs",
            "pub fn wrap(t: &str) -> Part {\n    am_domain::TrustedInput::from_step_output(\"tool:x\", t, Taint::Clean)\n}\n",
            "from_step_output",
        ),
        (
            "the composition root",
            "crates/am-app/src/run.rs",
            "fn note(t: &str) -> Option<TrustedInput> {\n    TrustedInput::from_owner_note(\"note:x\", t, &owner())\n}\n",
            "from_owner_note",
        ),
        (
            "a renamed import",
            "crates/am-server/src/inbox.rs",
            "use am_domain::TrustedInput as T;\nfn f(t: &str) -> Part {\n    T::from_step_output(\"x\", t, Taint::Clean)\n}\n",
            "from_step_output",
        ),
        (
            "am-domain beside the definition",
            "crates/am-domain/src/llm.rs",
            "impl From<UntrustedData> for Part {\n    fn from(d: UntrustedData) -> Part {\n        TrustedInput::from_step_output(d.label(), text(&d), Taint::Clean)\n    }\n}\n",
            "from_step_output",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/am-domain/src/content.rs", TRUSTED_INPUT);
        write(tmp.path(), file, source);
        let out = deps_at(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains(file), "{what}: names the file: {err}");
        assert!(err.contains(name), "{what}: names the constructor: {err}");
        assert!(
            err.contains("TrustedInput") && err.contains("am-engine"),
            "{what}: states the rule: {err}"
        );
        assert!(
            !err.contains("crates/am-domain/src/content.rs"),
            "{what}: the definition is not a finding: {err}"
        );
    }
}

// ---------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P4
// (every Rust 2024 literal prefix is read as a literal, so none hides code
// from the scans; an unterminated literal fails closed), P5 (the call-site
// rules read every `.rs` of a crate but `tests/`, `benches/` and
// `examples/`, `build.rs` included) and P14 (`Instruction::authored` has
// a call-site rule of its own: only where config or the owner's words
// become an instruction). Written before the patches.

/// am-domain's `Instruction` and its one constructor.
const INSTRUCTION: &str = "pub struct Instruction(String);\nimpl Instruction {\n    pub fn authored(text: impl Into<String>) -> Self {\n        Instruction(text.into())\n    }\n}\n";

/// A shipped call of a `TrustedInput` constructor outside am-engine.
const TRUSTED_CALL: &str = "pub fn wrap(t: &str) -> Part {\n    am_domain::TrustedInput::from_step_output(\"tool:x\", t, Taint::Clean)\n}\n";

/// A tree holding am-domain's definitions and `files`.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    use common::write;
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-domain/src/content.rs",
        &format!("{INSTRUCTION}{TRUSTED_INPUT}"),
    );
    for (file, text) in files {
        write(tmp.path(), file, text);
    }
    tmp
}

/// `check-deps` on `files` fails, naming `file` and every one of `words`.
fn refused(what: &str, files: &[(&str, &str)], file: &str, words: &[&str]) {
    let tmp = tree(files);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    assert!(err.contains(file), "{what}: names {file}: {err}");
    for w in words {
        assert!(err.contains(w), "{what}: names `{w}`: {err}");
    }
}

/// P4: a literal of any Rust 2024 prefix (`c"…"`, `cr"…"`, `cr#"…"#`,
/// `br##"…"##`, `r#"…"#`), even one that holds a backslash or a quote,
/// never hides the code after it: the `TrustedInput` call after it is
/// still found.
#[test]
fn a_literal_prefix_never_hides_a_trusted_input_call() {
    for (what, before) in [
        (
            "a raw C string ending in a backslash",
            r##"pub const S: &core::ffi::CStr = cr#"\"#;"##,
        ),
        (
            "a raw C string with no hashes",
            r#"pub const S: &core::ffi::CStr = cr"\";"#,
        ),
        (
            "a C string with an escaped quote",
            r#"pub const S: &core::ffi::CStr = c"a\"b";"#,
        ),
        (
            "a raw byte string with two hashes",
            r###"pub const S: &[u8] = br##"a"#"##;"###,
        ),
        (
            "a raw string holding a quote",
            r##"pub const S: &str = r#"say "hi""#;"##,
        ),
    ] {
        let source = format!("{before}\n{TRUSTED_CALL}");
        refused(
            what,
            &[("crates/am-tools/src/lib.rs", &source)],
            "crates/am-tools/src/lib.rs",
            &["from_step_output"],
        );
    }
}

/// P4: a literal that never ends fails the scan closed, naming the file,
/// instead of hiding everything after it.
#[test]
fn an_unterminated_literal_fails_check_deps_closed() {
    for source in [
        "pub const S: &str = \"never closed;\npub fn f() {}\n",
        "pub const S: &str = r#\"never closed;\npub fn f() {}\n",
        "pub const S: &core::ffi::CStr = cr#\"never closed;\npub fn f() {}\n",
    ] {
        refused(
            source,
            &[("crates/am-tools/src/lib.rs", source)],
            "crates/am-tools/src/lib.rs",
            &[],
        );
    }
}

/// P5: the call-site rule reads every `.rs` of a crate but its `tests/`,
/// `benches/` and `examples/`: a `TrustedInput` call in `build.rs`, in a
/// file a `#[path]` module or an `include!` pulls in from outside `src/`,
/// or in any other `.rs` under the crate is found, naming that file.
#[test]
fn a_trusted_input_call_outside_src_is_found() {
    let build = "fn main() {\n    let _ = am_domain::TrustedInput::from_step_output(\"x\", \"y\", Taint::Clean);\n}\n";
    // (what, the files written, the file the finding must name)
    type Case<'a> = (&'a str, Vec<(&'a str, &'a str)>, &'a str);
    let cases: [Case; 4] = [
        (
            "build.rs",
            vec![("crates/am-tools/build.rs", build)],
            "crates/am-tools/build.rs",
        ),
        (
            "a #[path] module",
            vec![
                (
                    "crates/am-tools/src/lib.rs",
                    "#[path = \"../extra/inner.rs\"]\nmod inner;\n",
                ),
                ("crates/am-tools/extra/inner.rs", TRUSTED_CALL),
            ],
            "crates/am-tools/extra/inner.rs",
        ),
        (
            "an include!",
            vec![
                (
                    "crates/am-tools/src/lib.rs",
                    "include!(\"../generated/calls.rs\");\n",
                ),
                ("crates/am-tools/generated/calls.rs", TRUSTED_CALL),
            ],
            "crates/am-tools/generated/calls.rs",
        ),
        (
            "another .rs under the crate",
            vec![("crates/am-tools/other/helper.rs", TRUSTED_CALL)],
            "crates/am-tools/other/helper.rs",
        ),
    ];
    for (what, files, file) in cases {
        refused(what, &files, file, &["from_step_output"]);
    }

    // Tests, benches and examples are not shipped.
    let tmp = tree(&[
        ("crates/am-tools/tests/t.rs", TRUSTED_CALL),
        ("crates/am-tools/benches/b.rs", TRUSTED_CALL),
        ("crates/am-tools/examples/e.rs", TRUSTED_CALL),
    ]);
    let out = deps_at(tmp.path());
    assert!(out.status.success(), "{}", stderr(&out));
}

/// P14: `Instruction::authored` turns text into a trusted instruction, so
/// it has a call-site rule: today's sites (am-domain's definition, am-app
/// building a plan from config, am-engine's spike driver) pass; an adapter,
/// a provider, a second site in am-domain or a renamed import fails,
/// naming the file and the constructor.
#[test]
fn instruction_authored_is_called_only_at_its_allowed_sites() {
    let tmp = tree(&[
        (
            "crates/am-app/src/run.rs",
            "fn seat(text: &str) -> Instruction {\n    Instruction::authored(text)\n}\n",
        ),
        (
            "crates/am-app/src/spike.rs",
            "fn prompt(text: String) -> Part {\n    Part::Instruction(Instruction::authored(text))\n}\n",
        ),
        (
            "crates/am-tools/tests/t.rs",
            "fn t() {\n    let _ = am_domain::Instruction::authored(\"x\");\n}\n",
        ),
        (
            "crates/am-providers/src/lib.rs",
            "// Instruction::authored is config's, never a provider's.\nconst WHY: &str = \"Instruction::authored\";\n",
        ),
    ]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "today's sites, tests, comments and literals pass: {}",
        stderr(&out)
    );

    for (what, file, source) in [
        (
            "an adapter",
            "crates/am-tools/src/lib.rs",
            "pub fn page(text: &str) -> Part {\n    Part::Instruction(am_domain::Instruction::authored(text))\n}\n",
        ),
        (
            "a provider",
            "crates/am-providers/src/common.rs",
            "pub fn system(text: &str) -> Instruction {\n    Instruction::authored(text)\n}\n",
        ),
        (
            "am-domain beside the definition",
            "crates/am-domain/src/llm.rs",
            "impl From<UntrustedData> for Instruction {\n    fn from(d: UntrustedData) -> Instruction {\n        Instruction::authored(text_of(&d))\n    }\n}\n",
        ),
        (
            "a renamed import",
            "crates/am-server/src/inbox.rs",
            "use am_domain::Instruction as I;\nfn f(t: &str) -> I {\n    I::authored(t)\n}\n",
        ),
    ] {
        refused(what, &[(file, source)], file, &["authored", "Instruction"]);
    }
}

/// Story 3.3 review patch round 2 (R1): a shebang first line (`#!/x "`)
/// hides no `TrustedInput` or `Instruction::authored` call after it: the
/// scan drops it as rustc does.
#[test]
fn a_shebang_line_hides_no_call() {
    let source = "#!/x \"\npub fn t(o: String) { let _ = am_domain::TrustedInput::from_step_output(o); let _ = am_domain::Instruction::authored(String::new()); } // \"\n";
    refused(
        "after a shebang",
        &[("crates/am-app/src/hide.rs", source)],
        "crates/am-app/src/hide.rs",
        &["from_step_output", "authored"],
    );
}

// ------------------------------------------------------------------ story 3.4
//
// Epic 3 story 4 (AD-15, AD-28; matrix row 21): the one Human-only resolve
// of an Inbox item, `Writer::resolve_inbox_item`, and the Human actor's
// constructor, `HumanActor::new`, are named in shipped code only by the two
// command handlers, `crates/am-server/src/inbox.rs` and
// `crates/am-app/src/gate.rs`, plus their definitions (am-store's method,
// am-domain's type). No engine, tool, MCP or other path can reach them
// without failing `check-deps`. Written before the rule exists.

/// am-domain's `HumanActor` and its constructor, with a doc example and a
/// unit test that name it (neither is shipped code).
const HUMAN_ACTOR: &str = "pub struct HumanActor {\n    person: String,\n    hat: String,\n}\nimpl HumanActor {\n    /// ```\n    /// let h = am_domain::HumanActor::new(\"owner\", \"Reviewer\");\n    /// ```\n    pub fn new(person: impl Into<String>, hat: impl Into<String>) -> HumanActor {\n        HumanActor { person: person.into(), hat: hat.into() }\n    }\n}\nimpl From<HumanActor> for Actor {\n    fn from(h: HumanActor) -> Actor {\n        Actor::Human { person: h.person, hat: h.hat }\n    }\n}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn into_an_actor() {\n        let _a: super::Actor = super::HumanActor::new(\"owner\", \"Reviewer\").into();\n    }\n}\n";

/// am-store's resolve, defined with a `HumanActor`, and a unit test that
/// calls it.
const RESOLVE: &str = "impl Writer {\n    pub fn resolve_inbox_item(&self, by: HumanActor, item_id: &str, decision: Decision, note: Option<String>) -> Result<(), StoreError> {\n        self.job(Actor::from(by), item_id, decision, note)\n    }\n}\n#[cfg(test)]\nmod tests {\n    fn resolves(w: &Writer) {\n        let _ = w.resolve_inbox_item(am_domain::HumanActor::new(\"owner\", \"Reviewer\"), \"i\", Decision::Approve, None);\n    }\n}\n";

/// A command handler's call of the resolve.
const HANDLER: &str = "pub fn decide(w: &Writer, id: &str, decision: Decision, note: Option<String>) -> Result<(), StoreError> {\n    w.resolve_inbox_item(HumanActor::new(\"owner\", REVIEWER_HAT), id, decision, note)\n}\n";

/// A tree holding the two definitions and `files`.
fn resolve_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    use common::write;
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/am-domain/src/actor.rs", HUMAN_ACTOR);
    write(tmp.path(), "crates/am-store/src/lib.rs", RESOLVE);
    for (file, text) in files {
        write(tmp.path(), file, text);
    }
    tmp
}

/// Row 21: the definitions, the two handlers, tests, comments and
/// literals pass; a call of `resolve_inbox_item` or `HumanActor::new`
/// anywhere else in shipped code (am-engine, am-tools, am-mcp, another file
/// of am-app or am-server, a second site in am-store) fails `check-deps`,
/// naming the file, the line, the name and the rule (AD-15).
#[test]
fn the_inbox_resolve_is_named_only_by_its_two_handlers() {
    let tmp = resolve_tree(&[
        ("crates/am-server/src/inbox.rs", HANDLER),
        ("crates/am-app/src/gate.rs", HANDLER),
        (
            "crates/am-e2e/tests/trust/inbox_resolve_human_only.rs",
            HANDLER,
        ),
        ("crates/am-store/tests/inbox_resolve.rs", HANDLER),
        (
            "crates/am-engine/src/gate.rs",
            "// The engine never calls resolve_inbox_item or HumanActor::new (AD-15).\nconst WHY: &str = \"resolve_inbox_item is the Human's; HumanActor::new is the CLI's\";\npub fn apply(w: &Writer, item: &str) -> Result<(), StoreError> {\n    w.exec(engine(), WriteCommand::ApplyResolution { item_id: item.into(), past_max: None })\n}\n",
        ),
    ]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the definitions, the two handlers, tests, comments and literals pass: {}",
        stderr(&out)
    );

    for (what, file, source, name) in [
        (
            "the engine",
            "crates/am-engine/src/gate.rs",
            "pub fn apply(w: &Writer, item: &str) -> Result<(), StoreError> {\n    w.resolve_inbox_item(HumanActor::new(\"owner\", \"Reviewer\"), item, Decision::Approve, None)\n}\n",
            "resolve_inbox_item",
        ),
        (
            "a tool",
            "crates/am-tools/src/lib.rs",
            "pub fn reviewer() -> Actor {\n    am_domain::HumanActor::new(\"owner\", \"Reviewer\").into()\n}\n",
            "HumanActor::new",
        ),
        (
            "an MCP client",
            "crates/am-mcp/src/lib.rs",
            "pub fn approve(w: &Writer, by: HumanActor, item: &str) {\n    let _ = w.resolve_inbox_item(by, item, Decision::Approve, None);\n}\n",
            "resolve_inbox_item",
        ),
        (
            "another file of am-app",
            "crates/am-app/src/run.rs",
            "fn reviewer() -> HumanActor {\n    HumanActor::new(\"owner\", REVIEWER_HAT)\n}\n",
            "HumanActor::new",
        ),
        (
            "another file of am-server",
            "crates/am-server/src/runs.rs",
            "pub fn approve(w: &Writer, by: HumanActor, id: &str) {\n    let _ = w.resolve_inbox_item(by, id, Decision::Approve, None);\n}\n",
            "resolve_inbox_item",
        ),
        (
            "am-store beside the definition",
            "crates/am-store/src/inbox.rs",
            "pub fn approve_everything(w: &Writer, id: &str) {\n    let _ = w.resolve_inbox_item(am_domain::HumanActor::new(\"owner\", \"Reviewer\"), id, Decision::Approve, None);\n}\n",
            "resolve_inbox_item",
        ),
    ] {
        let tmp = resolve_tree(&[(file, source)]);
        let out = deps_at(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        let line = source
            .lines()
            .position(|l| l.contains(name))
            .map(|i| i + 1)
            .unwrap();
        assert!(
            err.contains(&format!("{file}:{line}")),
            "{what}: names {file}:{line}: {err}"
        );
        assert!(err.contains(name), "{what}: names `{name}`: {err}");
        assert!(err.contains("AD-15"), "{what}: states the rule: {err}");
        for allowed in [
            "crates/am-domain/src/actor.rs",
            "crates/am-store/src/lib.rs",
        ] {
            assert!(
                !err.contains(allowed),
                "{what}: a definition is not a finding: {err}"
            );
        }
    }
}

// ------------------------------------------------------- review round 1
//
// Story 3.4 review round 1 (the coordinator's triage, 2026-11-24), R12: the
// rule matches whole files by token, not lines. The token `HumanActor` (and
// `resolve_inbox_item`) may stand in shipped `src/` only in am-domain (the
// definition and its re-export), am-store's `src/lib.rs` (the resolve),
// `crates/am-server/src/inbox.rs` and `crates/am-app/src/gate.rs`; a
// `#[cfg(test)]` item is exempt only in am-store. Written before the fix.

/// A command handler's file as shipped: its import, then its call.
const HANDLER_FILE: &str = "use am_domain::{Decision, HumanActor, REVIEWER_HAT};\npub fn decide(w: &Writer, id: &str, decision: Decision, note: Option<String>) -> Result<(), StoreError> {\n    w.resolve_inbox_item(HumanActor::new(\"owner\", REVIEWER_HAT), id, decision, note)\n}\n";

/// [`resolve_tree`] with am-domain's re-export and am-store's import, the
/// two handlers (each with its import), and a `#[cfg(test)]` use in
/// another file of am-store.
fn token_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let mut all: Vec<(&str, &str)> = vec![
        (
            "crates/am-domain/src/lib.rs",
            "pub mod actor;\npub use actor::{Actor, HumanActor};\n",
        ),
        ("crates/am-server/src/inbox.rs", HANDLER_FILE),
        ("crates/am-app/src/gate.rs", HANDLER_FILE),
        (
            "crates/am-store/src/migrations.rs",
            "pub fn migrate() {}\n#[cfg(test)]\nmod tests {\n    use am_domain::HumanActor;\n    fn t(w: &crate::Writer) {\n        let _ = w.resolve_inbox_item(HumanActor::new(\"owner\", \"Reviewer\"), \"i\", Decision::Approve, None);\n    }\n}\n",
        ),
    ];
    all.extend_from_slice(files);
    let tmp = resolve_tree(&all);
    common::write(
        tmp.path(),
        "crates/am-store/src/lib.rs",
        &format!("use am_domain::{{Actor, Decision, HumanActor}};\n{RESOLVE}"),
    );
    tmp
}

/// R12: the allowed sites pass (am-domain's definition and re-export,
/// am-store's resolve and a `#[cfg(test)]` use in another am-store file,
/// the two handlers, tests). In a disallowed crate's shipped source, each
/// way of naming `HumanActor` fails `check-deps`, naming the file, the
/// line, `HumanActor` and AD-15: a constructor split over lines, a `use …
/// as` alias, a `type` alias, `<HumanActor>::new`, and a `#[cfg(test)]` use
/// outside am-store.
#[test]
fn the_human_actor_token_is_matched_in_whole_files() {
    let tmp = token_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the allowed sites pass: {}",
        stderr(&out)
    );

    for (what, file, source) in [
        (
            "a constructor split over lines",
            "crates/am-tools/src/lib.rs",
            "pub fn reviewer() -> Actor {\n    am_domain::HumanActor\n        ::new(\"owner\", \"Reviewer\")\n        .into()\n}\n",
        ),
        (
            "a use alias",
            "crates/am-tools/src/lib.rs",
            "use am_domain::HumanActor as H;\npub fn reviewer() -> Actor {\n    H::new(\"owner\", \"Reviewer\").into()\n}\n",
        ),
        (
            "a type alias",
            "crates/am-mcp/src/lib.rs",
            "type H = am_domain::HumanActor;\npub fn reviewer() -> Actor {\n    H::new(\"owner\", \"Reviewer\").into()\n}\n",
        ),
        (
            "a qualified path",
            "crates/am-engine/src/gate.rs",
            "pub fn reviewer() -> Actor {\n    <am_domain::HumanActor>::new(\"owner\", \"Reviewer\").into()\n}\n",
        ),
        (
            "a #[cfg(test)] use outside am-store",
            "crates/am-engine/src/run.rs",
            "pub fn drive() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        let _a: Actor = am_domain::HumanActor::new(\"owner\", \"Reviewer\").into();\n    }\n}\n",
        ),
    ] {
        let tmp = token_tree(&[(file, source)]);
        let out = deps_at(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        let line = source
            .lines()
            .position(|l| l.contains("HumanActor"))
            .map(|i| i + 1)
            .unwrap();
        assert!(
            err.contains(&format!("{file}:{line}")),
            "{what}: names {file}:{line}: {err}"
        );
        assert!(err.contains("HumanActor"), "{what}: names the token: {err}");
        assert!(err.contains("AD-15"), "{what}: states the rule: {err}");
        for allowed in [
            "crates/am-domain/src/actor.rs",
            "crates/am-domain/src/lib.rs",
            "crates/am-store/src/lib.rs",
            "crates/am-store/src/migrations.rs",
            "crates/am-server/src/inbox.rs",
            "crates/am-app/src/gate.rs",
        ] {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

// ------------------------------------------------------- review round 2
//
// Story 3.4 review round 2 (the coordinator's triage, 2026-11-24), S5: in
// am-domain only `actor.rs` (the definition) and `lib.rs`'s re-export line
// may name `HumanActor`; in any allowed file an `as` alias or a `pub use`
// of either token fails the check. Written before the fix.

/// S5: the real allowed sites (each handler with its plain import) pass;
/// each of these fails `check-deps`, naming the file, the line, the token
/// and AD-15: a `pub use … as` re-export in another am-domain file, a `pub
/// use … as` in `crates/am-app/src/gate.rs`, a `use … as` alias in
/// `crates/am-server/src/inbox.rs`, and a mention of `HumanActor` in an
/// am-domain file that is not `actor.rs`.
#[test]
fn an_alias_or_a_re_export_of_human_actor_fails_even_in_an_allowed_file() {
    let tmp = token_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the allowed sites pass: {}",
        stderr(&out)
    );

    let gate_with_reexport = format!("{HANDLER_FILE}pub use am_domain::HumanActor as Owner;\n");
    let inbox_with_alias = "use am_domain::HumanActor as H;\nuse am_domain::{Decision, REVIEWER_HAT};\npub fn decide(w: &Writer, id: &str, decision: Decision, note: Option<String>) -> Result<(), StoreError> {\n    w.resolve_inbox_item(H::new(\"owner\", REVIEWER_HAT), id, decision, note)\n}\n";
    for (what, file, source, marker) in [
        (
            "a re-export in another am-domain file",
            "crates/am-domain/src/inbox.rs",
            "pub enum InboxKind { Review }\npub use crate::actor::HumanActor as Owner;\n",
            "as Owner",
        ),
        (
            "a re-export in the CLI handler",
            "crates/am-app/src/gate.rs",
            gate_with_reexport.as_str(),
            "as Owner",
        ),
        (
            "an alias in the API handler",
            "crates/am-server/src/inbox.rs",
            inbox_with_alias,
            "as H",
        ),
        (
            "a mention in an am-domain file that is not actor.rs",
            "crates/am-domain/src/content.rs",
            "pub struct Instruction(String);\npub fn signed_by(_who: &crate::HumanActor) {}\n",
            "HumanActor",
        ),
    ] {
        let tmp = token_tree(&[(file, source)]);
        let out = deps_at(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        let line = source
            .lines()
            .position(|l| l.contains(marker))
            .map(|i| i + 1)
            .unwrap();
        assert!(
            err.contains(&format!("{file}:{line}")),
            "{what}: names {file}:{line}: {err}"
        );
        assert!(err.contains("HumanActor"), "{what}: names the token: {err}");
        assert!(err.contains("AD-15"), "{what}: states the rule: {err}");
    }
}

// ---------------------------------------------------------------- story 3.5
//
// The type wall's source rule (AD-10, epic 3 story 5): `UntrustedData` has
// no conversion into an `Instruction`, but a string can still be pulled out
// of `UntrustedContent::Text(s)` (or `into_content()`) and handed to
// `Instruction::authored`. So a shipped file that is allowed to call
// `Instruction::authored` (`INSTRUCTION_SITES`) may not name
// `UntrustedData`, `UntrustedContent` or `into_content` in code (comments
// and literals left out). The rule only narrows: no file that passed
// before fails unless it names one of them. Written before the rule.

/// A file that is a site for `Instruction::authored` and names `token` in
/// code fails `check-deps`, naming the file, the token, `Instruction` and
/// the rule's AD.
#[test]
fn an_instruction_site_naming_untrusted_data_fails() {
    for (what, token, source) in [
        (
            "the type, in a signature",
            "UntrustedData",
            "use am_domain::{Instruction, UntrustedData};\nfn seat(d: &UntrustedData) -> Instruction {\n    Instruction::authored(\"x\")\n}\n",
        ),
        (
            "the content enum",
            "UntrustedContent",
            "fn seat(c: &am_domain::UntrustedContent) -> Instruction {\n    match c {\n        am_domain::UntrustedContent::Text(s) => Instruction::authored(s.clone()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "the accessor that hands the content out",
            "into_content",
            "fn seat(d: Data) -> Instruction {\n    let c = d.into_content();\n    Instruction::authored(format!(\"{c:?}\"))\n}\n",
        ),
        (
            "a use with a rename",
            "UntrustedData",
            "use am_domain::UntrustedData as Page;\nfn seat() -> Instruction {\n    Instruction::authored(\"x\")\n}\n",
        ),
    ] {
        for file in INSTRUCTION_SITES_NOW {
            refused(
                &format!("{what} in {file}"),
                &[(file, source)],
                file,
                &[token, "Instruction", "AD-10"],
            );
        }
    }
}

/// The files that may call `Instruction::authored` today (round 2, T1: am-app's only).
const INSTRUCTION_SITES_NOW: [&str; 3] = [
    "crates/am-app/src/run.rs",
    "crates/am-app/src/spike.rs",
    "crates/am-app/src/accounts.rs",
];

/// What the rule leaves alone: a comment or a literal naming the types in a
/// site, a file that is no site (an adapter, the engine's driver, whose own
/// unit tests call `authored` inside `#[cfg(test)]` and which names
/// `UntrustedData` in code as the plan's inputs) and am-domain's own
/// definitions all pass.
#[test]
fn the_rule_leaves_comments_literals_and_other_files_alone() {
    let tmp = tree(&[
        (
            "crates/am-app/src/run.rs",
            "// An instruction is never built from UntrustedData, UntrustedContent or into_content.\nconst WHY: &str = \"UntrustedData\";\nfn seat(t: &str) -> Instruction {\n    Instruction::authored(t)\n}\n",
        ),
        (
            "crates/am-engine/src/run.rs",
            "use am_domain::UntrustedData;\npub struct StepPlan {\n    pub inputs: Vec<UntrustedData>,\n}\n#[cfg(test)]\nmod tests {\n    fn seat() -> Instruction {\n        Instruction::authored(\"x\")\n    }\n}\n",
        ),
        (
            "crates/am-providers/src/common.rs",
            "pub fn text_of(d: &UntrustedData) -> String {\n    match d.content() {\n        UntrustedContent::Text(t) => t.clone(),\n        _ => String::new(),\n    }\n}\n",
        ),
    ]);

    let out = deps_at(tmp.path());

    assert!(
        out.status.success(),
        "comments, literals and non-sites pass: {}",
        stderr(&out)
    );
}

/// The shipped sites name none of the three in code today (a comment that
/// names the type is not code): the rule is green on shipped source. The
/// check itself is `current_workspace_passes`; this pins what it relies on,
/// file by file, so a failure names the file.
#[test]
fn the_shipped_instruction_sites_name_none() {
    let root = repo_root();
    for file in INSTRUCTION_SITES_NOW {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for token in ["UntrustedData", "UntrustedContent", "into_content"] {
                assert!(
                    !line.contains(token),
                    "{file}:{} names {token} in code; an Instruction is never built from it (AD-10)",
                    n + 1
                );
            }
        }
    }
    let out = xtask(&["check-deps"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

// ------------------------------------------------------- review round 1
//
// Story 3.5 review triage round 1 (manager, 2026-11-24), R3: a site file
// can still pass untrusted text on through `Part::Untrusted(d)` and its
// accessors, or through a trusted input, or an alias defined elsewhere. So
// a file that may call `Instruction::authored` may not name `Part::Untrusted`,
// `Part::Trusted` or `TrustedInput` either, and an alias of `UntrustedData`,
// `UntrustedContent` or `TrustedInput` (`type X = ..` or `use .. as X`) is
// refused anywhere in `crates/`. Written before the rule grows.

/// A site file naming a part or a trusted input in code fails, naming the
/// file, the token, `Instruction` and `AD-10`.
#[test]
fn an_instruction_site_naming_a_part_or_a_trusted_input_fails() {
    for (what, token, source) in [
        (
            "Part::Untrusted in a match",
            "Part::Untrusted",
            "fn seat(p: &Part) -> Instruction {\n    match p {\n        Part::Untrusted(d) => Instruction::authored(d.label()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "Part::Trusted in a match",
            "Part::Trusted",
            "fn seat(p: &Part) -> Instruction {\n    match p {\n        Part::Trusted(t) => Instruction::authored(t.text()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "TrustedInput in a signature",
            "TrustedInput",
            "fn seat(t: &TrustedInput) -> Instruction {\n    Instruction::authored(t.text())\n}\n",
        ),
    ] {
        for file in INSTRUCTION_SITES_NOW {
            refused(
                &format!("{what} in {file}"),
                &[(file, source)],
                file,
                &[token, "Instruction", "AD-10"],
            );
        }
    }
}

/// An alias of one of the three types, by `type` or by `use .. as`, fails
/// wherever it stands in `crates/` (a file that is no site, another crate's
/// source), naming the file, the type and the word `alias`; the plain use
/// of the types passes (see the rule's pass test).
#[test]
fn an_alias_of_an_untrusted_type_is_refused_anywhere_in_crates() {
    for (what, file, token, source) in [
        (
            "a type alias of UntrustedData",
            "crates/am-providers/src/common.rs",
            "UntrustedData",
            "pub type Page = am_domain::UntrustedData;\n",
        ),
        (
            "a use-as of UntrustedContent",
            "crates/am-engine/src/run.rs",
            "UntrustedContent",
            "use am_domain::UntrustedContent as Content;\nfn f(_c: Content) {}\n",
        ),
        (
            "a use-as in a group, of TrustedInput",
            "crates/am-server/src/inbox.rs",
            "TrustedInput",
            "use am_domain::{Part, TrustedInput as Note};\nfn f(_n: Option<Note>, _p: Part) {}\n",
        ),
        (
            "a type alias of TrustedInput",
            "crates/am-tools/src/lib.rs",
            "TrustedInput",
            "type Trusted = TrustedInput;\n",
        ),
    ] {
        refused(what, &[(file, source)], file, &[token, "alias", "AD-10"]);
    }
}

// ------------------------------------------------------- review round 2
//
// Story 3.5 review triage round 2 (manager, 2026-11-24): T1 (am-engine calls
// `Instruction::authored` nowhere in shipped code, `journal.rs` included;
// the sites are am-app files only) and T5 (the part ban reads whole-file
// text with whitespace normalised: glob and braced imports of `Part`'s
// variants, aliases of `Part`, `Part ::Untrusted` and a path split over
// lines all fail in a site). Written before the rule is changed.

/// T1: the engine's spike-scope executor builds no instruction itself.
#[test]
fn the_engine_journal_names_no_authored() {
    let text =
        std::fs::read_to_string(repo_root().join("crates/am-engine/src/journal.rs")).unwrap();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        assert!(
            !line.contains("authored"),
            "journal.rs:{} names authored; am-engine authors no Instruction (AD-10): its caller passes them",
            n + 1
        );
    }
}

/// T1: `INSTRUCTION_SITES` (read from the rule's own source) holds only
/// files of `crates/am-app/src/`.
#[test]
fn the_instruction_sites_are_am_app_files_only() {
    let text = std::fs::read_to_string(repo_root().join("xtask/src/deps.rs")).unwrap();
    let start = text.find("pub const INSTRUCTION_SITES").expect("the list");
    let block = &text[start..start + text[start..].find("];").unwrap()];
    let entries: Vec<&str> = block.split('"').skip(1).step_by(2).collect();
    assert!(!entries.is_empty(), "{block}");
    for entry in entries {
        assert!(
            entry.starts_with("crates/am-app/src/"),
            "{entry} is a site; only am-app builds an Instruction from config"
        );
    }
}

/// T1: a planted `journal.rs` (or any engine file) calling `authored` fails;
/// the engine driver's `#[cfg(test)]` items still may.
#[test]
fn am_engine_calls_authored_only_in_test_items() {
    let call = "fn prompt(text: String) -> Part {\n    Part::Instruction(Instruction::authored(text))\n}\n";
    for file in [
        "crates/am-engine/src/journal.rs",
        "crates/am-engine/src/prompt.rs",
        "crates/am-engine/src/run.rs",
    ] {
        refused(
            &format!("{file} outside a test item"),
            &[(file, call)],
            file,
            &["authored", "Instruction"],
        );
    }
    let tmp = tree(&[(
        "crates/am-engine/src/run.rs",
        "#[cfg(test)]\nmod tests {\n    fn seat() -> Instruction {\n        Instruction::authored(\"x\")\n    }\n}\n",
    )]);
    let out = deps_at(tmp.path());
    assert!(out.status.success(), "{}", stderr(&out));
}

/// T5: whole-file matching of the ban on `Part`'s untrusted and trusted
/// variants in a site file, with spacing and line breaks normalised.
#[test]
fn a_site_cannot_reach_the_part_variants_by_import_alias_spacing_or_line_breaks() {
    for (what, source) in [
        (
            "a glob import of the variants",
            "use am_domain::Part::*;\nfn seat(p: &Part) -> Instruction {\n    match p {\n        Untrusted(d) => Instruction::authored(d.label()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "a braced import with a rename",
            "use am_domain::Part::{Untrusted as U};\nfn seat(p: &Part) -> Instruction {\n    match p {\n        U(d) => Instruction::authored(d.label()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "an alias of Part",
            "type P = am_domain::Part;\nfn seat(p: &P) -> Instruction {\n    match p {\n        P::Untrusted(d) => Instruction::authored(d.label()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "a space before the path separator",
            "fn seat(p: &Part) -> Instruction {\n    match p {\n        Part ::Untrusted(d) => Instruction::authored(d.label()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "a path split across lines",
            "fn seat(p: &Part) -> Instruction {\n    match p {\n        Part\n            ::Trusted(t) => Instruction::authored(t.text()),\n        _ => Instruction::authored(\"\"),\n    }\n}\n",
        ),
        (
            "a use-as alias of Part",
            "use am_domain::Part as Piece;\nfn seat(_p: &Piece) -> Instruction {\n    Instruction::authored(\"x\")\n}\n",
        ),
    ] {
        let file = "crates/am-app/src/run.rs";
        refused(what, &[(file, source)], file, &["AD-10"]);
    }
}

// ------------------------------------------------ the store slice of 3.6
//
// Epic 3, the store slice of story 3.6 ("Row store and history"; the
// owner's condition of the third re-slice and the manager's row 49,
// 2026-11-24; AD-21): two call-site rules beside 3.4's resolve rule, each
// over whole-file shipped text with whitespace around `.` and `::`
// normalised and line breaks joined:
//
// - row 48: the store's row command `apply_row_write` stands in shipped
//   code only in `crates/am-store/src/` (its definition and its
//   `#[cfg(test)]` items): nothing else calls it until the ToolGate slice
//   adds its one caller, the engine's port implementation;
// - row 49: the readers of row content on the store's `Reader` (`rows`,
//   `row`, `row_history`, `row_write`) are named in shipped code only in
//   `crates/am-store/src/`; the data-free receipt (`row_receipt`) is not one
//   of them.
//
// Tests, comments and literals are not shipped code. Written before the
// rules exist.

/// am-store's row command and a unit test that calls it.
const STORE_ROWS: &str = "impl Writer {\n    pub fn apply_row_write(&self, actor: Actor, write: RowWrite) -> Result<(), StoreError> {\n        self.send(actor, Command::Row(write))\n    }\n}\n#[cfg(test)]\nmod tests {\n    fn replay(w: &super::Writer, a: Actor, x: RowWrite) {\n        let _ = w.apply_row_write(a, x);\n    }\n}\n";

/// am-store's readers of row content and its receipt (their definitions),
/// a reader that calls another, and a unit test that calls them.
const STORE_READERS: &str = "impl Reader {\n    pub fn table_rows_of(&self, table_id: &str) -> Result<Vec<RowRecord>, StoreError> {\n        self.select(table_id)\n    }\n    pub fn table_row_by_id(&self, table_id: &str, row_id: &str) -> Result<Option<RowRecord>, StoreError> {\n        Ok(self.table_rows_of(table_id)?.into_iter().find(|r| r.row_id == row_id))\n    }\n    pub fn row_history_of(&self, table_id: &str, row_id: &str) -> Result<Vec<RowHistoryRecord>, StoreError> {\n        self.images(table_id, row_id)\n    }\n    pub fn row_written_at(&self, step_run_id: &str, call_seq: i64) -> Result<Option<RowHistoryRecord>, StoreError> {\n        self.image_at(step_run_id, call_seq)\n    }\n    pub fn row_receipt(&self, step_run_id: &str, call_seq: i64) -> Result<Option<RowReceipt>, StoreError> {\n        Ok(self.row_written_at(step_run_id, call_seq)?.map(|i| receipt(&i)))\n    }\n}\n#[cfg(test)]\nmod tests {\n    fn t(w: &crate::Writer, r: &super::Reader, a: Actor, x: RowWrite) {\n        let _ = w.apply_row_write(a, x);\n        let _ = r.row_history_of(\"entries\", \"r-1\");\n    }\n}\n";

/// The store slice's allowed sites: am-store's command and readers, the
/// engine's `StartAttempt` with no table (no write path) and a call of the
/// data-free receipt, tests that call everything, and comments and
/// literals that name the tokens.
fn store_slice_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("crates/am-store/src/lib.rs", STORE_ROWS),
        ("crates/am-store/src/rows.rs", STORE_READERS),
        (
            "crates/am-engine/src/run.rs",
            "// No step writes or reads a row yet: apply_row_write and table_rows_of are the store's.\nconst WHY: &str = \"apply_row_write is called from the ToolGate slice on; reader.row_history_of( never\";\nfn start(step: &StepPlan) -> WriteCommand {\n    WriteCommand::StartAttempt { step_run_id: String::new(), inputs: vec![], taint: Taint::Clean, prompt: vec![], writes: AttemptWrites { seat: step.seat.clone(), table: None, document: None } }\n}\n",
        ),
        (
            "crates/am-engine/src/toolgate.rs",
            "fn receipt(r: &am_store::Reader, step_run: &str, seq: i64) -> Option<am_store::RowReceipt> {\n    r.row_receipt(step_run, seq).ok().flatten()\n}\n",
        ),
        (
            "crates/am-store/tests/rows.rs",
            "fn t(w: &am_store::Writer, r: &am_store::Reader, a: Actor, x: RowWrite) {\n    let _ = w.apply_row_write(a, x);\n    let _ = r.table_rows_of(\"entries\");\n    let _ = r.table_row_by_id(\"entries\", \"r-1\");\n    let _ = r.row_history_of(\"entries\", \"r-1\");\n    let _ = r.row_written_at(\"s\", 1);\n}\n",
        ),
        (
            "crates/am-e2e/tests/engine/support/mod.rs",
            "pub fn rows_of(db: &Path, table: &str) -> Vec<RowRecord> {\n    Reader::open(db).unwrap().table_rows_of(table).unwrap()\n}\n",
        ),
    ]
}

/// A tree holding [`store_slice_files`], with each of `files` added or put
/// in place of the file at the same path.
fn store_slice_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let mut all = store_slice_files();
    for (file, _) in files {
        all.retain(|(f, _)| f != file);
    }
    for (file, text) in all.iter().chain(files.iter()) {
        common::write(tmp.path(), file, text);
    }
    tmp
}

/// The 1-based lines of `source` that hold any of `markers`.
fn lines_holding(source: &str, markers: &[&str]) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| markers.iter().any(|m| l.contains(m)))
        .map(|(i, _)| i + 1)
        .collect()
}

/// `check-deps` on [`store_slice_tree`] with `(file, source)` planted
/// fails, naming `file:<line>` for one of the lines of `source` that hold
/// one of `markers` and every one of `words`; no allowed site is a finding.
fn store_slice_refused(what: &str, file: &str, source: &str, markers: &[&str], words: &[&str]) {
    let tmp = store_slice_tree(&[(file, source)]);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, markers);
    assert!(!lines.is_empty(), "{what}: the source holds {markers:?}");
    assert!(
        lines.iter().any(|n| err.contains(&format!("{file}:{n}"))),
        "{what}: names {file} at a line holding {markers:?} ({lines:?}): {err}"
    );
    for w in words {
        assert!(err.contains(w), "{what}: names `{w}`: {err}");
    }
    for (allowed, _) in store_slice_files() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

/// Row 48 (the owner's condition: nothing in shipped code calls the
/// row-write command until the ToolGate slice): am-store's definition and
/// its own `#[cfg(test)]` calls, tests, comments and literals pass. Each
/// planted call fails, naming the file, the line, `apply_row_write` and
/// AD-21: in the engine's driver, in am-app and in am-server (the ToolGate
/// slice widens the rule to its one caller, the engine's port
/// implementation, `crates/am-engine/src/row_writer.rs`); split over lines
/// and spaced around `.`; by path (`Writer::apply_row_write(&w, …)`), also
/// with spaces around `::`.
#[test]
fn the_row_write_command_is_named_only_in_the_store() {
    let tmp = store_slice_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "am-store, tests, comments and literals pass: {}",
        stderr(&out)
    );

    let call = "fn write(w: &am_store::Writer, a: Actor, x: RowWrite) {\n    let _ = w.apply_row_write(a, x);\n}\n";
    for (what, file, source) in [
        ("the engine's driver", "crates/am-engine/src/run.rs", call),
        ("am-app", "crates/am-app/src/run.rs", call),
        ("am-server", "crates/am-server/src/inbox.rs", call),
        (
            "split over lines and spaced",
            "crates/am-tools/src/lib.rs",
            "fn write(w: &Writer, a: Actor, x: RowWrite) {\n    let _ = w\n        . apply_row_write (a, x);\n}\n",
        ),
        (
            "by path",
            "crates/am-engine/src/gate.rs",
            "fn write(w: &am_store::Writer, a: Actor, x: RowWrite) {\n    let _ = am_store::Writer::apply_row_write(w, a, x);\n}\n",
        ),
        (
            "by path, spaces around ::",
            "crates/am-mcp/src/lib.rs",
            "fn write(w: &Writer, a: Actor, x: RowWrite) {\n    let _ = Writer :: apply_row_write(&w, a, x);\n}\n",
        ),
    ] {
        store_slice_refused(
            what,
            file,
            source,
            &["apply_row_write"],
            &["apply_row_write", "AD-21"],
        );
    }
}

/// The `.rs` files of the shipped tree (every crate's `src/`), with their
/// code (lines that are only a comment left out) as one line, whitespace
/// around `.` and `::` and line breaks normalised away.
fn shipped_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let root = repo_root();
    let mut files = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        walk(&krate.path().join("src"), &mut files);
    }
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let code: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join(" ");
            let flat = code
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace(" ::", "::")
                .replace(":: ", "::")
                .replace(" .", ".")
                .replace(". ", ".");
            let rel = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, flat)
        })
        .collect()
}

/// Row 48, widened by the ToolGate slice: on the shipped tree am-store
/// defines the row command, no file outside `crates/am-store/src/` but the
/// engine's port implementation names it, and `cargo xtask check-deps`
/// passes on the real workspace.
#[test]
fn the_shipped_tree_passes_the_row_write_rule() {
    let sources = shipped_sources();
    let defined: Vec<&String> = sources
        .iter()
        .filter(|(rel, code)| {
            rel.starts_with("crates/am-store/src/") && code.contains("fn apply_row_write(")
        })
        .map(|(rel, _)| rel)
        .collect();
    assert!(!defined.is_empty(), "am-store defines the row command");
    let elsewhere: Vec<&String> = sources
        .iter()
        .filter(|(rel, code)| {
            !rel.starts_with("crates/am-store/src/")
                && rel != "crates/am-engine/src/row_writer.rs"
                && code.contains("apply_row_write")
        })
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        elsewhere.is_empty(),
        "no shipped caller of the row command: {elsewhere:?}"
    );
    let out = xtask(&["check-deps"]);
    assert!(
        out.status.success(),
        "the real workspace passes the rule: {}",
        stderr(&out)
    );
}

/// The readers of row content on the store's `Reader`, by their names
/// unique in `crates/` (story 3.6's review triage round 1, R4).
const ROW_READERS: [&str; 4] = [
    "table_rows_of",
    "table_row_by_id",
    "row_history_of",
    "row_written_at",
];

/// Row 49 (manager, 2026-11-24; review triage round 1, R4 and R13: no
/// shipped code reads row content). The store's readers, a test and a
/// shipped call of the data-free receipt pass. A reader named anywhere in
/// shipped code outside `crates/am-store/src/` fails, matched as a bare
/// token over the whole file: called in the engine, am-providers, am-tools,
/// am-app and am-server; split over lines; spaced around `.`; on a `use …
/// as` alias of `Reader`; taken as a function value
/// (`let f = Reader::<name>`); passed to `.map(Reader::<name>)`; with a
/// turbofish; as a raw identifier (`r#<name>`); and called by its path.
/// Each finding names `file:line` of a planted line and, on that same
/// finding, the exact reader. On the shipped tree no file outside
/// `crates/am-store/src/` names one. Story 3.6c widens the rule by one
/// reader and one file, never these four: the snapshot reader
/// `row_snapshot` stands outside am-store only in
/// `crates/am-engine/src/row_inputs.rs`, the one read site (matrix row 46;
/// `the_one_row_read_site_is_the_engines_row_inputs`).
#[test]
fn no_shipped_code_reads_row_content() {
    let tmp = store_slice_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the store's readers, tests and the receipt pass: {}",
        stderr(&out)
    );

    for (what, file, source, reader) in [
        (
            "a call in the engine",
            "crates/am-engine/src/run.rs",
            "fn peek(r: &am_store::Reader) -> usize {\n    r.table_rows_of(\"entries\").unwrap().len()\n}\n",
            "table_rows_of",
        ),
        (
            "a call in am-providers",
            "crates/am-providers/src/common.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> Option<String> {\n    r.table_row_by_id(\"entries\", id).ok().flatten().map(|x| x.data)\n}\n",
            "table_row_by_id",
        ),
        (
            "a call in am-tools",
            "crates/am-tools/src/lib.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> usize {\n    r.row_history_of(\"entries\", id).unwrap().len()\n}\n",
            "row_history_of",
        ),
        (
            "a call in am-app",
            "crates/am-app/src/run.rs",
            "fn peek(r: &am_store::Reader, step_run: &str) -> bool {\n    r.row_written_at(step_run, 1).unwrap().is_some()\n}\n",
            "row_written_at",
        ),
        (
            "a call in am-server",
            "crates/am-server/src/runs.rs",
            "fn peek(r: &am_store::Reader) -> usize {\n    r.table_rows_of(\"entries\").unwrap().len()\n}\n",
            "table_rows_of",
        ),
        (
            "split over lines",
            "crates/am-engine/src/gate.rs",
            "fn peek(reader: &am_store::Reader, id: &str) -> usize {\n    reader\n        .row_history_of(\"entries\", id)\n        .unwrap()\n        .len()\n}\n",
            "row_history_of",
        ),
        (
            "spaced around the dot",
            "crates/am-engine/src/scheduler.rs",
            "fn peek(reader: &am_store::Reader) -> bool {\n    reader . row_written_at (\"s\", 1).unwrap().is_some()\n}\n",
            "row_written_at",
        ),
        (
            "on a use-as alias of Reader",
            "crates/am-app/src/gate.rs",
            "use am_store::Reader as Journal;\nfn peek(j: &Journal) -> usize {\n    j.table_rows_of(\"entries\").unwrap().len()\n}\n",
            "table_rows_of",
        ),
        (
            "a function value",
            "crates/am-engine/src/prompt.rs",
            "fn peek(r: &am_store::Reader) -> usize {\n    let f = am_store::Reader::table_rows_of;\n    f(r, \"entries\").unwrap().len()\n}\n",
            "table_rows_of",
        ),
        (
            "passed to map",
            "crates/am-engine/src/schema.rs",
            "fn peek(r: &am_store::Reader, keys: Vec<(&str, &str)>) -> usize {\n    keys.into_iter().map(|(t, id)| (r, t, id)).map(Reader::row_history_of).count()\n}\n",
            "row_history_of",
        ),
        (
            "a turbofish",
            "crates/am-server/src/inbox.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> bool {\n    r.table_row_by_id::<>(\"entries\", id).unwrap().is_some()\n}\n",
            "table_row_by_id",
        ),
        (
            "a raw identifier",
            "crates/am-mcp/src/lib.rs",
            "fn peek(r: &am_store::Reader) -> bool {\n    r.r#row_written_at(\"s\", 1).unwrap().is_some()\n}\n",
            "row_written_at",
        ),
        (
            "a path call",
            "crates/am-engine/src/cancel.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> usize {\n    am_store::Reader::row_history_of(r, \"entries\", id).unwrap().len()\n}\n",
            "row_history_of",
        ),
    ] {
        reader_refused(what, file, source, reader);
    }

    let sources = shipped_sources();
    let elsewhere: Vec<(&String, &str)> = sources
        .iter()
        .filter(|(rel, _)| !rel.starts_with("crates/am-store/src/"))
        .flat_map(|(rel, code)| {
            ROW_READERS
                .iter()
                .filter(|reader| names_token(code, reader))
                .map(move |reader| (rel, *reader))
        })
        .collect();
    assert!(
        elsewhere.is_empty(),
        "no shipped reader of row content outside am-store: {elsewhere:?}"
    );
    let snapshot_readers: Vec<&String> = sources
        .iter()
        .filter(|(rel, code)| {
            !rel.starts_with("crates/am-store/src/")
                && rel != "crates/am-engine/src/row_inputs.rs"
                && names_token(code, "row_snapshot")
        })
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        snapshot_readers.is_empty(),
        "row_snapshot outside am-store only in the one read site: {snapshot_readers:?}"
    );
}

/// True when `code` holds `token` with no word character directly before
/// or after it.
fn names_token(code: &str, token: &str) -> bool {
    code.match_indices(token).any(|(at, _)| {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let before = code[..at].chars().next_back();
        let after = code[at + token.len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// `check-deps` on [`store_slice_tree`] with `(file, source)` planted
/// fails with a finding that names, on one stderr line, `file:<line>` for a
/// line of `source` holding `reader` and the exact `reader` (R13); no
/// allowed site is a finding.
fn reader_refused(what: &str, file: &str, source: &str, reader: &str) {
    let tmp = store_slice_tree(&[(file, source)]);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, &[reader]);
    assert!(!lines.is_empty(), "{what}: the source holds {reader}");
    assert!(
        err.lines().any(|finding| {
            lines
                .iter()
                .any(|n| finding.contains(&format!("{file}:{n}:")))
                && names_token(finding, reader)
        }),
        "{what}: a finding names {file} at a line holding {reader} ({lines:?}) and `{reader}`: {err}"
    );
    for (allowed, _) in store_slice_files() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

// ------------------------------------------------- the ToolGate slice of 3.6
//
// Epic 3 story 6's ToolGate slice (owner conditions Q1 and Q4; AD-10 and
// AD-21 as amended 2026-11-24; matrix rows 33 and 37): three call-site
// rules beside 3.4's resolve rule, each over whole-file shipped text with
// whitespace around `.` and `::` normalised and line breaks joined (the
// store slice's row-48 rule is widened to the engine's port
// implementation):
//
// - the row-writer port's method `write_row` stands only as its call in
//   `crates/am-engine/src/toolgate.rs`, the trait's definition in
//   `crates/am-domain/src/rows.rs` and the `fn write_row` line of the
//   engine's implementation, `crates/am-engine/src/row_writer.rs`;
// - the store's row command `apply_row_write` stands only in
//   `crates/am-store/src/` and `crates/am-engine/src/row_writer.rs`;
// - `EngineResult::row_written` stands only in
//   `crates/am-engine/src/toolgate.rs` and its definition in
//   `crates/am-domain/src/content.rs`;
// - an `as` alias (or a `type` alias) of `RowWriter` or `EngineResult`, and
//   a glob import of `am_domain::rows::*` or `am_domain::content::*` outside
//   am-domain, fail anywhere in `crates/`.
//
// Tests, comments and literals are not shipped code. Written before the
// rules exist.

/// am-domain's port (the trait's one definition of `write_row`, with a doc
/// example that names it: a comment).
const ROWS_PORT: &str = "use crate::{Actor, Taint};\npub struct RowWrite {\n    pub table_id: String,\n    pub taint: Taint,\n}\n/// ```\n/// let _ = writer.write_row(actor, write);\n/// ```\npub trait RowWriter: Send + Sync {\n    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError>;\n}\n";

/// am-domain's `EngineResult` and its one constructor.
const ENGINE_RESULT_DEF: &str = "pub struct EngineResult {\n    label: String,\n    text: String,\n}\nimpl EngineResult {\n    pub fn row_written(written: &RowWritten) -> EngineResult {\n        EngineResult { label: \"tool:table_write\".into(), text: format!(\"{} {}\", written.row_id, written.version) }\n    }\n    pub fn text(&self) -> &str {\n        &self.text\n    }\n}\n";

/// ToolGate: the one call of the port's method and the one build of an
/// engine result.
const TOOLGATE: &str = "use std::sync::Arc;\nuse am_domain::rows::{RowTool, RowWrite, RowWriter};\nuse am_domain::{Actor, EngineResult};\npub struct ToolGate {\n    rows: Arc<dyn RowWriter>,\n    tool: Option<Arc<dyn RowTool>>,\n}\nimpl ToolGate {\n    fn write(&self, actor: Actor, write: RowWrite) -> Result<EngineResult, String> {\n        match self.rows.write_row(actor, write) {\n            Ok(written) => Ok(EngineResult::row_written(&written)),\n            Err(e) => Err(words(e)),\n        }\n    }\n}\n";

/// The engine's implementation of the port, over the store's row command.
const ROW_WRITER: &str = "use am_domain::rows::{RowWrite, RowWriteError, RowWriter, RowWritten};\nuse am_domain::Actor;\npub struct StoreRowWriter {\n    writer: am_store::Writer,\n}\nimpl RowWriter for StoreRowWriter {\n    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {\n        let key = (write.step_run_id.clone(), write.call_seq);\n        self.writer.apply_row_write(actor, write).map_err(refused)?;\n        self.read_back(&key)\n    }\n}\n";

/// Every allowed site of the three rules, as [`rows_tree`] writes them, plus
/// other shipped files that name the types (not the method or the
/// constructor), tests that call all three, and comments and literals.
fn rows_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("crates/am-domain/src/rows.rs", ROWS_PORT),
        ("crates/am-domain/src/content.rs", ENGINE_RESULT_DEF),
        ("crates/am-engine/src/toolgate.rs", TOOLGATE),
        ("crates/am-engine/src/row_writer.rs", ROW_WRITER),
        ("crates/am-store/src/lib.rs", STORE_ROWS),
        (
            "crates/am-store/src/rows.rs",
            "/// The checks `Writer::apply_row_write` makes.\npub(crate) fn checks(write: &RowWrite) -> Result<(), StoreError> {\n    Ok(())\n}\n#[cfg(test)]\nmod tests {\n    fn t(w: &crate::Writer, a: Actor, x: RowWrite) {\n        let _ = w.apply_row_write(a, x);\n    }\n}\n",
        ),
        (
            "crates/am-engine/src/run.rs",
            "use std::sync::Arc;\nuse am_domain::rows::RowWriter;\nfn gate(store: &am_store::Store) -> ToolGate {\n    let rows: Arc<dyn RowWriter> = Arc::new(crate::row_writer::StoreRowWriter::new(store));\n    ToolGate::shipped(rows, None, audit())\n}\n",
        ),
        (
            "crates/am-app/src/run.rs",
            "use std::sync::Arc;\nuse am_tools::table_write::TableWrite;\npub fn row_tool() -> Option<Arc<dyn am_domain::rows::RowTool>> {\n    Some(Arc::new(TableWrite::new()))\n}\n",
        ),
        (
            "crates/am-providers/src/common.rs",
            "pub(crate) fn render_engine_result(result: &am_domain::EngineResult) -> String {\n    result.text().to_string()\n}\n",
        ),
        (
            "crates/am-tools/src/table_write.rs",
            "// Never calls write_row or apply_row_write; ToolGate does (AD-21).\nconst WHY: &str = \"EngineResult::row_written is ToolGate's, and so is .write_row(\";\npub struct TableWrite;\n",
        ),
        (
            "crates/am-e2e/tests/trust/taint_tool_result_engine_values.rs",
            "fn t(w: &dyn RowWriter, s: &am_store::Writer, a: Actor, x: RowWrite, y: RowWrite) {\n    let written = w.write_row(a.clone(), x).unwrap();\n    let _ = EngineResult::row_written(&written);\n    let _ = s.apply_row_write(a, y);\n}\n",
        ),
        (
            "crates/am-engine/tests/row_writer.rs",
            "fn t(w: &StoreRowWriter, a: Actor, x: RowWrite) {\n    let _ = w.write_row(a, x);\n}\n",
        ),
    ]
}

/// A tree holding [`rows_files`], with each of `files` added or put in
/// place of the file at the same path.
fn rows_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let mut all = rows_files();
    for (file, _) in files {
        all.retain(|(f, _)| f != file);
    }
    for (file, text) in all.iter().chain(files.iter()) {
        common::write(tmp.path(), file, text);
    }
    tmp
}

/// `check-deps` on [`rows_tree`] with `(file, source)` planted fails,
/// naming `file:<line>` for one of the lines of `source` that hold one of
/// `markers` (the token, or either part of a path split over lines) and
/// every one of `words`; no other allowed site is a finding.
fn rows_refused(what: &str, file: &str, source: &str, markers: &[&str], words: &[&str]) {
    let tmp = rows_tree(&[(file, source)]);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, markers);
    assert!(!lines.is_empty(), "{what}: the source holds {markers:?}");
    assert!(
        lines.iter().any(|n| err.contains(&format!("{file}:{n}"))),
        "{what}: names {file} at a line holding {markers:?} ({lines:?}): {err}"
    );
    for w in words {
        assert!(err.contains(w), "{what}: names `{w}`: {err}");
    }
    for (allowed, _) in rows_files() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

/// `check-deps` on [`rows_tree`] with `(file, source)` planted fails and
/// names `file` and every one of `words` (a finding with no one token
/// line, such as a glob import).
fn rows_refused_in_file(what: &str, file: &str, source: &str, words: &[&str]) {
    let tmp = rows_tree(&[(file, source)]);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    assert!(err.contains(file), "{what}: names {file}: {err}");
    for w in words {
        assert!(err.contains(w), "{what}: names `{w}`: {err}");
    }
}

/// Row 33 (owner Q1): only ToolGate calls the row-writer port. The allowed
/// sites pass: the trait's definition, ToolGate's call, the
/// implementation's `fn write_row` line, other shipped files naming the
/// trait, tests, comments and literals. Each planted form fails, naming the
/// file, the line, `write_row` (or the aliased `RowWriter`) and AD-21: a
/// call in another am-engine file, in am-tools and in am-app; a call split
/// over lines; spaces around `.`; the method called by path with spaces
/// around `::`; a second call in the implementation's file; an `as` alias
/// of `RowWriter` in a plain file and in ToolGate's own file; a glob import
/// of `am_domain::rows::*` outside am-domain.
#[test]
fn only_toolgate_calls_the_row_writer() {
    let tmp = rows_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the allowed sites, tests, comments and literals pass: {}",
        stderr(&out)
    );

    for (what, file, source) in [
        (
            "another am-engine file",
            "crates/am-engine/src/run.rs",
            "fn record(rows: &dyn RowWriter, a: Actor, x: RowWrite) {\n    let _ = rows.write_row(a, x);\n}\n",
        ),
        (
            "am-tools",
            "crates/am-tools/src/table_write.rs",
            "pub fn write(rows: &dyn am_domain::rows::RowWriter, a: Actor, x: RowWrite) {\n    let _ = rows.write_row(a, x);\n}\n",
        ),
        (
            "am-app",
            "crates/am-app/src/run.rs",
            "pub fn write(rows: &dyn am_domain::rows::RowWriter, a: Actor, x: RowWrite) {\n    let _ = rows.write_row(a, x);\n}\n",
        ),
        (
            "a call split over lines",
            "crates/am-engine/src/scheduler.rs",
            "fn record(&self, a: Actor, x: RowWrite) {\n    let _ = self\n        .rows\n        .write_row(a, x);\n}\n",
        ),
        (
            "spaces around the dot",
            "crates/am-engine/src/gate.rs",
            "fn record(&self, a: Actor, x: RowWrite) {\n    let _ = self.rows . write_row (a, x);\n}\n",
        ),
        (
            "the method by path, spaces around ::",
            "crates/am-server/src/runs.rs",
            "fn record(w: &dyn RowWriter, a: Actor, x: RowWrite) {\n    let _ = RowWriter :: write_row(w, a, x);\n}\n",
        ),
        (
            "a second call in the implementation's file",
            "crates/am-engine/src/row_writer.rs",
            "use am_domain::rows::{RowWrite, RowWriteError, RowWriter, RowWritten};\nuse am_domain::Actor;\npub struct StoreRowWriter {\n    writer: am_store::Writer,\n}\nimpl RowWriter for StoreRowWriter {\n    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {\n        self.writer.apply_row_write(actor, write).map_err(refused)?;\n        todo!()\n    }\n}\nimpl StoreRowWriter {\n    fn again(&self, a: Actor, x: RowWrite) {\n        let _ = self.write_row(a, x);\n    }\n}\n",
        ),
    ] {
        let marker = if what.starts_with("a second call") {
            ".write_row("
        } else {
            "write_row"
        };
        rows_refused(what, file, source, &[marker], &["write_row", "AD-21"]);
    }

    for (what, file, source) in [
        (
            "an alias in a plain file",
            "crates/am-engine/src/run.rs",
            "use am_domain::rows::RowWriter as W;\nfn keep(_w: &dyn W) {}\n",
        ),
        (
            "an alias in ToolGate's own file",
            "crates/am-engine/src/toolgate.rs",
            "use std::sync::Arc;\nuse am_domain::rows::RowWriter as Rows;\nuse am_domain::{Actor, EngineResult};\npub struct ToolGate {\n    rows: Arc<dyn Rows>,\n}\nimpl ToolGate {\n    fn write(&self, actor: Actor, write: RowWrite) -> Result<EngineResult, String> {\n        match self.rows.write_row(actor, write) {\n            Ok(written) => Ok(EngineResult::row_written(&written)),\n            Err(e) => Err(words(e)),\n        }\n    }\n}\n",
        ),
        (
            "a type alias",
            "crates/am-mcp/src/lib.rs",
            "type W = dyn am_domain::rows::RowWriter;\nfn keep(_w: &W) {}\n",
        ),
    ] {
        rows_refused(what, file, source, &["RowWriter"], &["RowWriter"]);
    }

    rows_refused_in_file(
        "a glob import of am_domain::rows outside am-domain",
        "crates/am-app/src/run.rs",
        "use std::sync::Arc;\nuse am_domain::rows::*;\npub fn row_tool() -> Option<Arc<dyn RowTool>> {\n    None\n}\n",
        &[],
    );
}

/// Row 33 (owner Q1): the store's row command is named only in am-store
/// (any file of its `src/`, its `#[cfg(test)]` items included) and in the
/// engine's implementation of the port. Named in am-app, am-server, the
/// engine's driver or ToolGate (which calls the port, never the store), or
/// with spacing and a line break before it, it fails, naming the file, the
/// line, `apply_row_write` and AD-21.
#[test]
fn the_store_row_write_is_named_only_by_the_engines_row_writer() {
    let tmp = rows_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "am-store, the engine's implementation and tests pass: {}",
        stderr(&out)
    );

    let call = "fn write(w: &am_store::Writer, a: Actor, x: RowWrite) {\n    let _ = w.apply_row_write(a, x);\n}\n";
    for (what, file, source) in [
        ("am-app", "crates/am-app/src/run.rs", call),
        ("am-server", "crates/am-server/src/inbox.rs", call),
        ("the engine's driver", "crates/am-engine/src/run.rs", call),
        (
            "ToolGate itself",
            "crates/am-engine/src/toolgate.rs",
            "use am_domain::{Actor, EngineResult};\npub struct ToolGate {\n    store: am_store::Writer,\n}\nimpl ToolGate {\n    fn write(&self, actor: Actor, write: RowWrite) -> Result<(), String> {\n        self.store.apply_row_write(actor, write).map_err(|e| e.to_string())\n    }\n}\n",
        ),
        (
            "spacing and a line break",
            "crates/am-tools/src/lib.rs",
            "fn write(w: &Writer, a: Actor, x: RowWrite) {\n    let _ = w\n        . apply_row_write (a, x);\n}\n",
        ),
    ] {
        rows_refused(
            what,
            file,
            source,
            &["apply_row_write"],
            &["apply_row_write", "AD-21"],
        );
    }
}

/// Row 37 (owner Q4; AD-10 as amended 2026-11-24): only ToolGate builds an
/// engine result. The definition and ToolGate's call pass, and so does any
/// file that only names the type (an adapter rendering it). Each planted
/// form fails: `EngineResult::row_written` in the engine's driver, am-tools,
/// am-providers, am-app and another am-domain file (naming the file, the
/// line, `EngineResult` and AD-10); spaces around `::`, a path split over
/// lines and a qualified path `<EngineResult>::row_written`; a `use … as`
/// and a `type` alias of `EngineResult` (naming it); a glob import of
/// `am_domain::content::*` outside am-domain (naming the file).
#[test]
fn only_toolgate_builds_an_engine_result() {
    let tmp = rows_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the definition, ToolGate's call and the type named alone pass: {}",
        stderr(&out)
    );

    for (what, file, source, marker) in [
        (
            "the engine's driver",
            "crates/am-engine/src/run.rs",
            "fn result(w: &RowWritten) -> Part {\n    Part::EngineResult { call_id: \"c\".into(), result: EngineResult::row_written(w) }\n}\n",
            "row_written",
        ),
        (
            "am-tools",
            "crates/am-tools/src/table_write.rs",
            "pub fn result(w: &RowWritten) -> am_domain::EngineResult {\n    am_domain::EngineResult::row_written(w)\n}\n",
            "row_written",
        ),
        (
            "am-providers",
            "crates/am-providers/src/common.rs",
            "pub(crate) fn render(w: &RowWritten) -> String {\n    EngineResult::row_written(w).text().to_string()\n}\n",
            "row_written",
        ),
        (
            "am-app",
            "crates/am-app/src/run.rs",
            "pub fn result(w: &RowWritten) -> EngineResult {\n    EngineResult::row_written(w)\n}\n",
            "row_written",
        ),
        (
            "another am-domain file",
            "crates/am-domain/src/llm.rs",
            "impl From<RowWritten> for Part {\n    fn from(w: RowWritten) -> Part {\n        Part::EngineResult { call_id: String::new(), result: EngineResult::row_written(&w) }\n    }\n}\n",
            "row_written",
        ),
        (
            "spaces around ::",
            "crates/am-engine/src/gate.rs",
            "fn r(w: &RowWritten) -> EngineResult {\n    EngineResult :: row_written(w)\n}\n",
            "row_written",
        ),
        (
            "a path split over lines",
            "crates/am-mcp/src/lib.rs",
            "fn r(w: &RowWritten) -> am_domain::EngineResult {\n    am_domain::EngineResult\n        ::row_written(w)\n}\n",
            "::row_written",
        ),
        (
            "a qualified path",
            "crates/am-engine/src/scheduler.rs",
            "fn r(w: &RowWritten) -> EngineResult {\n    <EngineResult>::row_written(w)\n}\n",
            "row_written",
        ),
    ] {
        // A path split over lines may be reported at either of its lines.
        let markers: &[&str] = if marker == "::row_written" {
            &["    am_domain::EngineResult", "::row_written"]
        } else {
            &[marker]
        };
        rows_refused(what, file, source, markers, &["EngineResult", "AD-10"]);
    }

    for (what, file, source) in [
        (
            "a use-as alias",
            "crates/am-tools/src/lib.rs",
            "use am_domain::EngineResult as Done;\nfn r(w: &RowWritten) -> Done {\n    Done::row_written(w)\n}\n",
        ),
        (
            "a type alias",
            "crates/am-server/src/runs.rs",
            "type Done = am_domain::EngineResult;\nfn r(w: &RowWritten) -> Done {\n    Done::row_written(w)\n}\n",
        ),
    ] {
        rows_refused(what, file, source, &["EngineResult"], &["EngineResult"]);
    }

    rows_refused_in_file(
        "a glob import of am_domain::content outside am-domain",
        "crates/am-providers/src/anthropic.rs",
        "use am_domain::content::*;\npub(crate) fn render(r: &EngineResult) -> String {\n    r.text().to_string()\n}\n",
        &[],
    );
}

/// A shipped file's code (lines that are only a comment left out), with
/// whitespace around `.` and `::` and line breaks normalised away.
fn shipped_code(rel: &str) -> String {
    let path = repo_root().join(rel);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{rel} must exist ({e}); the ToolGate slice adds it"));
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ");
    code.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ::", "::")
        .replace(":: ", "::")
        .replace(" .", ".")
        .replace(". ", ".")
}

/// Rows 33 and 37: the shipped tree has each rule's one site and passes the
/// rules: am-domain defines the port's `fn write_row` and the engine
/// result's `fn row_written`; ToolGate calls `write_row` and
/// `EngineResult::row_written`; the engine's implementation names
/// `apply_row_write`; and `cargo xtask check-deps` passes on the real
/// workspace.
#[test]
fn the_shipped_tree_passes_the_rules() {
    let rows = shipped_code("crates/am-domain/src/rows.rs");
    assert!(
        rows.contains("fn write_row("),
        "am-domain defines the port's method"
    );
    let content = shipped_code("crates/am-domain/src/content.rs");
    assert!(
        content.contains("fn row_written("),
        "am-domain defines the engine result's one constructor"
    );
    let gate = shipped_code("crates/am-engine/src/toolgate.rs");
    assert!(
        gate.contains(".write_row("),
        "ToolGate calls the port's method"
    );
    assert!(
        gate.contains("EngineResult::row_written("),
        "ToolGate builds the engine result"
    );
    let writer = shipped_code("crates/am-engine/src/row_writer.rs");
    assert!(
        writer.contains(".apply_row_write("),
        "the engine's implementation calls the store's row command"
    );
    assert!(
        writer.contains("fn write_row("),
        "the engine's implementation defines the port's method"
    );
    let out = xtask(&["check-deps"]);
    assert!(
        out.status.success(),
        "the real workspace passes the rules: {}",
        stderr(&out)
    );
}

// --------------------------------------- row 43 (the ToolGate slice of 3.6)
//
// Owner condition b of the second re-slice (2026-11-24; matrix row 43): no
// engine path passes row content to a model call. The store slice's row-49
// rule keeps the four row-content readers in am-store; this slice adds the
// two record types they return, the only types that carry a row's data:
// `RowRecord` and `RowHistoryRecord` stand in shipped code only in
// `crates/am-store/src/`, matched as bare tokens over the whole file (so a
// `use … as` alias, a `type` alias, a path split over lines or spaced
// around `::`, and a raw identifier are caught). And `rusqlite` is a normal
// (or target-specific) dependency of am-store alone, so no other crate can
// open the database and read a row by SQL; a dev-dependency (tests) is not
// shipped. Written before the rules exist.

/// The row records' definitions in am-store, and the data-free receipt.
const STORE_RECORDS: &str = "pub struct RowRecord {\n    pub table_id: String,\n    pub data: String,\n}\npub struct RowHistoryRecord {\n    pub version: i64,\n    pub data: String,\n}\npub struct RowReceipt {\n    pub version: i64,\n}\n";

/// The store slice's allowed sites, plus the records' definitions and
/// re-export in am-store, a shipped engine file that reads the receipt and
/// names the records only in a comment and a literal, and a test that
/// names both records.
fn record_files() -> Vec<(&'static str, &'static str)> {
    let mut files = store_slice_files();
    files.extend([
        ("crates/am-store/src/records.rs", STORE_RECORDS),
        (
            "crates/am-store/src/export.rs",
            "pub use crate::records::{RowHistoryRecord, RowReceipt, RowRecord};\n",
        ),
        (
            "crates/am-engine/src/journal.rs",
            "// The engine reads a RowReceipt, never a RowRecord or a RowHistoryRecord.\nconst WHY: &str = \"RowRecord and RowHistoryRecord stay in am-store\";\nfn version(r: &am_store::RowReceipt) -> i64 {\n    r.version\n}\n",
        ),
        (
            "crates/am-app/tests/run_rows.rs",
            "fn t(r: &am_store::RowRecord, h: &am_store::RowHistoryRecord) -> usize {\n    r.data.len() + h.data.len()\n}\n",
        ),
    ]);
    files
}

/// `check-deps` on a tree of [`record_files`] with `(file, source)` planted
/// fails with a finding that names, on one stderr line, `file:<line>` for
/// a line of `source` holding `record` and the exact `record`; no allowed
/// site is a finding.
fn record_refused(what: &str, file: &str, source: &str, record: &str) {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in record_files().iter().filter(|(f, _)| *f != file) {
        common::write(tmp.path(), f, text);
    }
    common::write(tmp.path(), file, source);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, &[record]);
    assert!(!lines.is_empty(), "{what}: the source holds {record}");
    assert!(
        err.lines().any(|finding| {
            lines
                .iter()
                .any(|n| finding.contains(&format!("{file}:{n}:")))
                && names_token(finding, record)
        }),
        "{what}: a finding names {file} at a line holding {record} ({lines:?}) and `{record}`: {err}"
    );
    for (allowed, _) in record_files() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

/// Row 43 (owner condition b): the records that carry row data are named
/// in shipped code only in am-store. The allowed tree passes (the
/// definitions, the re-export, the receipt read in the engine, comments,
/// literals and tests). Each planted form fails, naming `file:line` and
/// the record: a parameter in the engine's driver; a reference in
/// am-providers; a `use` in am-app; a `use … as` alias in am-server; a path
/// spaced around `::` in am-tools; a path split over lines; a raw
/// identifier; inside a generic; a `type` alias. On the shipped tree no
/// file outside `crates/am-store/src/` names either record. Story 3.6c
/// adds one record with one more site, never these two: `RowSnapshot`
/// stands outside am-store only in `crates/am-engine/src/row_inputs.rs`
/// (matrix row 46).
#[test]
fn row_content_is_read_only_in_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in record_files() {
        common::write(tmp.path(), f, text);
    }
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the store's records, the receipt, comments, literals and tests pass: {}",
        stderr(&out)
    );

    for (what, file, source, record) in [
        (
            "a parameter in the engine's driver",
            "crates/am-engine/src/run.rs",
            "fn peek(rows: Vec<am_store::RowRecord>) -> usize {\n    rows.len()\n}\n",
            "RowRecord",
        ),
        (
            "a reference in am-providers",
            "crates/am-providers/src/common.rs",
            "pub(crate) fn render(h: &am_store::RowHistoryRecord) -> String {\n    h.data.clone()\n}\n",
            "RowHistoryRecord",
        ),
        (
            "a use in am-app",
            "crates/am-app/src/run.rs",
            "use am_store::RowRecord;\nfn input(r: RowRecord) -> String {\n    r.data\n}\n",
            "RowRecord",
        ),
        (
            "a use-as alias in am-server",
            "crates/am-server/src/runs.rs",
            "use am_store::RowHistoryRecord as Image;\nfn peek(i: &Image) -> usize {\n    i.data.len()\n}\n",
            "RowHistoryRecord",
        ),
        (
            "a path spaced around ::",
            "crates/am-tools/src/lib.rs",
            "fn peek(r: &am_store :: RowRecord) -> usize {\n    r.data.len()\n}\n",
            "RowRecord",
        ),
        (
            "a path split over lines",
            "crates/am-engine/src/gate.rs",
            "fn peek(r: &am_store::\n    RowRecord) -> usize {\n    r.data.len()\n}\n",
            "RowRecord",
        ),
        (
            "a raw identifier",
            "crates/am-mcp/src/lib.rs",
            "fn peek(r: &am_store::r#RowHistoryRecord) -> usize {\n    r.data.len()\n}\n",
            "RowHistoryRecord",
        ),
        (
            "inside a generic",
            "crates/am-engine/src/prompt.rs",
            "fn peek(r: Option<Vec<am_store::RowHistoryRecord>>) -> usize {\n    r.map(|v| v.len()).unwrap_or(0)\n}\n",
            "RowHistoryRecord",
        ),
        (
            "a type alias",
            "crates/am-connectors/src/lib.rs",
            "type Row = am_store::RowRecord;\nfn peek(r: &Row) -> usize {\n    r.data.len()\n}\n",
            "RowRecord",
        ),
    ] {
        record_refused(what, file, source, record);
    }

    let sources = shipped_sources();
    let elsewhere: Vec<(&String, &str)> = sources
        .iter()
        .filter(|(rel, _)| !rel.starts_with("crates/am-store/src/"))
        .flat_map(|(rel, code)| {
            ["RowRecord", "RowHistoryRecord"]
                .into_iter()
                .filter(|record| names_token(code, record))
                .map(move |record| (rel, record))
        })
        .collect();
    assert!(
        elsewhere.is_empty(),
        "no shipped record of row content outside am-store: {elsewhere:?}"
    );
    let snapshots: Vec<&String> = sources
        .iter()
        .filter(|(rel, code)| {
            !rel.starts_with("crates/am-store/src/")
                && rel != "crates/am-engine/src/row_inputs.rs"
                && names_token(code, "RowSnapshot")
        })
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        snapshots.is_empty(),
        "RowSnapshot outside am-store only in the one read site: {snapshots:?}"
    );
}

/// True when the manifest names rusqlite in a normal, build or
/// target-specific dependency table (as a key, a dotted table or through
/// `package =`); a dev-dependency (tests) does not count.
fn links_rusqlite(manifest: &str) -> bool {
    let mut inside = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            let header = t.trim_start_matches('[').trim_end_matches(']');
            inside = is_dependency_table(t)
                && !header
                    .split('.')
                    .any(|part| part.trim().trim_matches('"') == "dev-dependencies");
            if inside && header.ends_with(".rusqlite") {
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
        let renamed = t.replace(' ', "").contains("package=\"rusqlite\"");
        if key == "rusqlite" || renamed {
            return true;
        }
    }
    false
}

/// Row 43 (owner condition b): only am-store links rusqlite. check-deps on
/// a graph where the engine, am-config (target-specific), am-providers and
/// am-app depend on rusqlite fails, naming each edge and the rule ("only
/// am-store may depend on rusqlite"); am-store's own edge and the
/// dev-dependencies of the engine, am-tools and am-e2e (tests) are not
/// findings. On the workspace, no member's manifest but am-store's names
/// rusqlite outside its dev-dependencies, and the scan reads every kind of
/// table.
#[test]
fn only_the_store_links_rusqlite() {
    let out = xtask(&["check-deps", "--metadata", &metadata("rusqlite_user.json")]);
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    for edge in [
        "am-engine -> rusqlite",
        "am-config -> rusqlite",
        "am-providers -> rusqlite",
        "am-app -> rusqlite",
    ] {
        assert!(err.contains(edge), "names {edge}: {err}");
    }
    assert!(
        err.contains("only am-store may depend on rusqlite"),
        "states the rule: {err}"
    );
    for allowed in [
        "am-store -> rusqlite",
        "am-tools -> rusqlite",
        "am-e2e -> rusqlite",
    ] {
        assert!(!err.contains(allowed), "{allowed} is allowed: {err}");
    }

    for manifest in [
        "[dependencies]\nrusqlite.workspace = true\n",
        "[build-dependencies]\nrusqlite = { workspace = true }\n",
        "[target.'cfg(unix)'.dependencies]\nrusqlite.workspace = true\n",
        "[dependencies.rusqlite]\nworkspace = true\n",
        "[dependencies]\ndb = { package = \"rusqlite\", version = \"0.43\" }\n",
    ] {
        assert!(links_rusqlite(manifest), "{manifest}");
    }
    for manifest in [
        "[dev-dependencies]\nrusqlite.workspace = true\n",
        "[target.'cfg(unix)'.dev-dependencies]\nrusqlite = \"0.43\"\n",
        "[package]\nname = \"rusqlite\"\n",
        "[features]\nrusqlite = []\n",
    ] {
        assert!(!links_rusqlite(manifest), "{manifest}");
    }
    let mut linking = Vec::new();
    for member in workspace_members() {
        let manifest = repo_root().join(&member).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
        if links_rusqlite(&text) {
            linking.push(member);
        }
    }
    assert_eq!(linking, ["crates/am-store"]);
}

// ---------------------------- ToolGate slice, review round 1 (R2, 3.23)
//
// Review triage round 1 (manager, 2026-11-25), R2: the row rules pin one
// call site, not one file. The store's row command is called exactly once
// in shipped code outside am-store: as `.apply_row_write(`, inside `fn
// write_row` of `crates/am-engine/src/row_writer.rs`. The port's method is
// called exactly once: one `.write_row(` in ToolGate's write path in
// `crates/am-engine/src/toolgate.rs`. And `StoreRowWriter`'s public items
// are only `new` and the trait's `write_row`. Written before the patch.

/// The engine's port implementation as shipped: `new` and the trait impl.
const ROW_WRITER_WITH_NEW: &str = "use am_domain::rows::{RowWrite, RowWriteError, RowWriter, RowWritten};\nuse am_domain::Actor;\npub struct StoreRowWriter {\n    writer: am_store::Writer,\n}\nimpl StoreRowWriter {\n    pub fn new(store: &am_store::Store) -> StoreRowWriter {\n        StoreRowWriter { writer: store.writer() }\n    }\n}\nimpl RowWriter for StoreRowWriter {\n    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {\n        let key = (write.step_run_id.clone(), write.call_seq);\n        self.writer.apply_row_write(actor, write).map_err(refused)?;\n        self.read_back(&key)\n    }\n}\n";

/// R2: one call site each. The allowed tree (with `new` on the port
/// implementation) passes. Each planted form fails: a second method of
/// `row_writer.rs` that calls the row command (naming the file, a line of
/// that method, `apply_row_write` and AD-21); a second call of it inside
/// `fn write_row`; a second `.write_row(` in another method of
/// `toolgate.rs` (naming `write_row` and AD-21); and a public item on
/// `StoreRowWriter` beyond `new` and the trait impl, a method or a field
/// (naming the file, its line and `StoreRowWriter`).
#[test]
fn the_row_rules_pin_one_call_site() {
    let tmp = rows_tree(&[("crates/am-engine/src/row_writer.rs", ROW_WRITER_WITH_NEW)]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "one call each, and `new`, pass: {}",
        stderr(&out)
    );

    let second_method = format!(
        "{ROW_WRITER_WITH_NEW}impl StoreRowWriter {{\n    fn sneak(&self, a: Actor, x: RowWrite) {{\n        let _ = self.writer.apply_row_write(a, x);\n    }}\n}}\n"
    );
    rows_refused(
        "a second method of row_writer.rs calling the row command",
        "crates/am-engine/src/row_writer.rs",
        &second_method,
        &["apply_row_write(a, x)", "fn sneak"],
        &["apply_row_write", "AD-21"],
    );
    let twice = ROW_WRITER_WITH_NEW.replace(
        "        self.read_back(&key)\n",
        "        let _ = self.writer.apply_row_write(actor, again);\n        self.read_back(&key)\n",
    );
    assert_ne!(twice, ROW_WRITER_WITH_NEW);
    rows_refused(
        "a second call inside fn write_row",
        "crates/am-engine/src/row_writer.rs",
        &twice,
        &["apply_row_write"],
        &["apply_row_write", "AD-21"],
    );
    let toolgate_twice = format!(
        "{TOOLGATE}impl ToolGate {{\n    fn again(&self, a: Actor, x: RowWrite) {{\n        let _ = self.rows.write_row(a, x);\n    }}\n}}\n"
    );
    rows_refused(
        "a second .write_row( in toolgate.rs",
        "crates/am-engine/src/toolgate.rs",
        &toolgate_twice,
        &["write_row"],
        &["write_row", "AD-21"],
    );
    let public_method = ROW_WRITER_WITH_NEW.replace(
        "        StoreRowWriter { writer: store.writer() }\n    }\n",
        "        StoreRowWriter { writer: store.writer() }\n    }\n    pub fn raw_writer(&self) -> &am_store::Writer {\n        &self.writer\n    }\n",
    );
    assert_ne!(public_method, ROW_WRITER_WITH_NEW);
    rows_refused(
        "a public method on StoreRowWriter",
        "crates/am-engine/src/row_writer.rs",
        &public_method,
        &["pub fn raw_writer"],
        &["StoreRowWriter"],
    );
    let public_field = ROW_WRITER_WITH_NEW.replace(
        "    writer: am_store::Writer,\n",
        "    pub writer: am_store::Writer,\n",
    );
    assert_ne!(public_field, ROW_WRITER_WITH_NEW);
    rows_refused(
        "a public field on StoreRowWriter",
        "crates/am-engine/src/row_writer.rs",
        &public_field,
        &["pub writer"],
        &["StoreRowWriter"],
    );
}

/// R2 on the shipped tree: `row_writer.rs` calls the row command once, in
/// `fn write_row`; `toolgate.rs` calls the port's method once; and
/// `StoreRowWriter`'s only public items are the struct and `new` (the
/// trait's `write_row` is the impl's). `cargo xtask check-deps` passes on
/// the real workspace (`the_shipped_tree_passes_the_rules`).
#[test]
fn the_shipped_tree_has_one_call_site_each() {
    let writer = shipped_code("crates/am-engine/src/row_writer.rs");
    assert_eq!(writer.matches(".apply_row_write(").count(), 1, "{writer}");
    let in_write_row = writer
        .split("fn write_row(")
        .nth(1)
        .expect("row_writer.rs defines write_row");
    assert!(in_write_row.contains(".apply_row_write("), "{writer}");
    let gate = shipped_code("crates/am-engine/src/toolgate.rs");
    let shipped_gate = gate.split("#[cfg(test)]").next().unwrap_or_default();
    assert_eq!(shipped_gate.matches(".write_row(").count(), 1);
    let public: Vec<&str> = writer
        .match_indices("pub ")
        .map(|(at, _)| {
            writer[at..]
                .split(['(', '{', ';', ':'])
                .next()
                .unwrap_or("")
        })
        .collect();
    assert_eq!(
        public,
        ["pub struct StoreRowWriter ", "pub fn new"],
        "StoreRowWriter's public items: {writer}"
    );
}

// ---------------------------- ToolGate slice, review round 2 (T1, 3.23)
//
// Review triage round 2 (manager, 2026-11-25), T1: ToolGate's one call of
// the port's method sits inside its write path, `fn write`, as the row
// command's one call sits inside `fn write_row` of `row_writer.rs`. A
// file with one `.write_row(` elsewhere (a new `pub fn` that `fn write`
// calls) hands the port on and fails. Written before the patch.

/// T1: a planted `toolgate.rs` whose only `.write_row(` stands in a new
/// `pub fn write_through`, which `fn write` calls, fails check-deps, naming
/// the file, a line of that function, `write_row` and AD-21. The allowed
/// tree (the call inside `fn write`) passes, and on the shipped tree the
/// one call is inside `fn write`.
#[test]
fn toolgates_one_port_call_is_inside_fn_write() {
    let tmp = rows_tree(&[]);
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the call inside fn write passes: {}",
        stderr(&out)
    );

    let moved = "use std::sync::Arc;\nuse am_domain::rows::{RowTool, RowWrite, RowWriteError, RowWriter, RowWritten};\nuse am_domain::{Actor, EngineResult};\npub struct ToolGate {\n    rows: Arc<dyn RowWriter>,\n    tool: Option<Arc<dyn RowTool>>,\n}\nimpl ToolGate {\n    pub fn write_through(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {\n        self.rows.write_row(actor, write)\n    }\n    fn write(&self, actor: Actor, write: RowWrite) -> Result<EngineResult, String> {\n        match self.write_through(actor, write) {\n            Ok(written) => Ok(EngineResult::row_written(&written)),\n            Err(e) => Err(words(e)),\n        }\n    }\n}\n";
    assert_eq!(
        moved.matches(".write_row(").count(),
        1,
        "one call in the file"
    );
    rows_refused(
        "the only call in a new pub fn",
        "crates/am-engine/src/toolgate.rs",
        moved,
        &["write_row(actor, write)", "fn write_through"],
        &["write_row", "AD-21"],
    );

    let gate = shipped_code("crates/am-engine/src/toolgate.rs");
    let shipped_gate = gate.split("#[cfg(test)]").next().unwrap_or_default();
    let body = shipped_gate
        .split("fn write(")
        .nth(1)
        .expect("toolgate.rs defines fn write");
    let body = &body[..body.find(" fn ").unwrap_or(body.len())];
    assert!(
        body.contains(".write_row("),
        "the shipped call is inside fn write"
    );
}

// ------------------------------------------- story 3.6c, the one read site
//
// Story 3.6c ("Rows reaching later steps"; plan matrix row 46; owner Q4's
// reader condition): the one read path widens the store slice's row-49
// rule and the ToolGate slice's row-43 record rule by exactly one name
// each and one file. The store's `Reader::row_snapshot` (a row's current
// after-image) and its record `RowSnapshot` stand in shipped code only in
// `crates/am-store/src/` and `crates/am-engine/src/row_inputs.rs`, which
// reads a step's row inputs at the attempt's start. Every other row-content
// reader and record stays am-store's alone, in `row_inputs.rs` too; an
// alias of `RowSnapshot` (`use … as`, `type`) fails in every file, the
// read site included. Whole-file tokens, as in the rules it widens; tests,
// comments and literals are not shipped code. Written before the site
// exists.

/// am-store's snapshot reader and the record it returns (definitions).
const STORE_SNAPSHOT: &str = "pub struct RowSnapshot {\n    pub seq: i64,\n    pub version: i64,\n    pub data: String,\n    pub taint: Taint,\n}\nimpl Reader {\n    pub fn row_snapshot(&self, table_id: &str, row_id: &str) -> Result<Option<RowSnapshot>, StoreError> {\n        self.current_image(table_id, row_id)\n    }\n}\n";

/// The engine's one read site, as the plan has it: each row input read
/// through `row_snapshot`, a `row` edge and a fenced part.
const ROW_INPUTS: &str = "use am_domain::UntrustedData;\nuse am_domain::rows::RowInput;\nuse am_store::{Reader, RowSnapshot, StepInput, StoreError};\npub struct RowRead {\n    pub input: StepInput,\n    pub part: UntrustedData,\n}\npub fn read_rows(reader: &Reader, rows: &[RowInput]) -> Result<Result<Vec<RowRead>, String>, StoreError> {\n    let mut reads = Vec::new();\n    for row in rows {\n        let id = row.row_id.to_string();\n        let found: Option<RowSnapshot> = reader.row_snapshot(&row.table_id, &id)?;\n        let Some(snapshot) = found else {\n            return Ok(Err(format!(\"the table {} holds no row {id}\", row.table_id)));\n        };\n        reads.push(read_of(row, &id, snapshot));\n    }\n    Ok(Ok(reads))\n}\n";

/// The row-43 record tree, plus am-store's snapshot reader, the engine's
/// read site, and a test, a comment and a literal that name both (in place
/// of the record tree's `journal.rs`).
fn read_site_files() -> Vec<(&'static str, &'static str)> {
    let mut files: Vec<(&'static str, &'static str)> = record_files()
        .into_iter()
        .filter(|(f, _)| *f != "crates/am-engine/src/journal.rs")
        .collect();
    files.extend([
        ("crates/am-store/src/snapshot.rs", STORE_SNAPSHOT),
        ("crates/am-engine/src/row_inputs.rs", ROW_INPUTS),
        (
            "crates/am-engine/tests/row_inputs.rs",
            "fn t(r: &am_store::Reader) -> Option<am_store::RowSnapshot> {\n    r.row_snapshot(\"entries\", \"r-1\").unwrap()\n}\n",
        ),
        (
            "crates/am-engine/src/journal.rs",
            "// Rows are read in row_inputs.rs through row_snapshot, as a RowSnapshot.\nconst WHY: &str = \"reader.row_snapshot( and RowSnapshot stay in row_inputs.rs\";\nfn version(r: &am_store::RowReceipt) -> i64 {\n    r.version\n}\n",
        ),
    ]);
    files
}

/// `check-deps` on a tree of [`read_site_files`] with `(file, source)`
/// planted fails with a finding that names, on one stderr line,
/// `file:<line>` for a line of `source` holding `token` and the exact
/// `token`; no other allowed site is a finding.
fn read_site_refused(what: &str, file: &str, source: &str, token: &str) {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in read_site_files().iter().filter(|(f, _)| *f != file) {
        common::write(tmp.path(), f, text);
    }
    common::write(tmp.path(), file, source);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, &[token]);
    assert!(!lines.is_empty(), "{what}: the source holds {token}");
    assert!(
        err.lines().any(|finding| {
            lines
                .iter()
                .any(|n| finding.contains(&format!("{file}:{n}:")))
                && names_token(finding, token)
        }),
        "{what}: a finding names {file} at a line holding {token} ({lines:?}) and `{token}`: {err}"
    );
    for (allowed, _) in read_site_files() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

/// Row 46: the one row read site is the engine's `row_inputs.rs`.
///
/// - The allowed tree passes: am-store's `row_snapshot` and `RowSnapshot`,
///   `row_inputs.rs` naming both, a test, a comment and a literal.
/// - Each planted form fails, naming `file:line` and the exact token:
///   `row_snapshot` in the engine's driver (`run.rs`), in am-app and in
///   am-server; split over lines in another engine file; as a function
///   value in am-providers; in a file of the same name outside am-engine;
///   another content reader (`table_rows_of`, `row_history_of`) or another
///   record (`RowRecord`) in `row_inputs.rs`; `RowSnapshot` named in
///   am-providers; a `use … as` alias of `RowSnapshot` in `row_inputs.rs`
///   itself; a `type` alias of it in am-app.
/// - On the shipped tree: `row_inputs.rs` exists and calls
///   `.row_snapshot(`; it names no `Instruction`, `TrustedInput`, `authored`,
///   `from_step_output`, `from_owner_note` or `Part::Trusted` (a row is
///   never trusted, AD-10); am-store defines `fn row_snapshot(`; no shipped
///   file outside `crates/am-store/src/` and `row_inputs.rs` names
///   `row_snapshot` or `RowSnapshot`; and `cargo xtask check-deps` passes
///   on the real workspace.
#[test]
fn the_one_row_read_site_is_the_engines_row_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in read_site_files() {
        common::write(tmp.path(), f, text);
    }
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the store's snapshot, the one read site, tests, comments and literals pass: {}",
        stderr(&out)
    );

    let with = |extra: &str| format!("{ROW_INPUTS}{extra}");
    for (what, file, source, token) in [
        (
            "row_snapshot in the engine's driver",
            "crates/am-engine/src/run.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> bool {\n    r.row_snapshot(\"entries\", id).unwrap().is_some()\n}\n".to_string(),
            "row_snapshot",
        ),
        (
            "row_snapshot in am-app",
            "crates/am-app/src/run.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> Option<String> {\n    r.row_snapshot(\"entries\", id).ok().flatten().map(|s| s.data)\n}\n".to_string(),
            "row_snapshot",
        ),
        (
            "row_snapshot in am-server",
            "crates/am-server/src/runs.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> bool {\n    r.row_snapshot(\"entries\", id).unwrap().is_some()\n}\n".to_string(),
            "row_snapshot",
        ),
        (
            "row_snapshot split over lines in another engine file",
            "crates/am-engine/src/prompt.rs",
            "fn peek(reader: &am_store::Reader, id: &str) -> bool {\n    reader\n        .row_snapshot(\"entries\", id)\n        .unwrap()\n        .is_some()\n}\n".to_string(),
            "row_snapshot",
        ),
        (
            "row_snapshot as a function value in am-providers",
            "crates/am-providers/src/common.rs",
            "fn peek(r: &am_store::Reader) -> bool {\n    let f = am_store::Reader::row_snapshot;\n    f(r, \"entries\", \"r-1\").unwrap().is_some()\n}\n".to_string(),
            "row_snapshot",
        ),
        (
            "a file of the same name outside am-engine",
            "crates/am-app/src/row_inputs.rs",
            "fn peek(r: &am_store::Reader, id: &str) -> bool {\n    r.row_snapshot(\"entries\", id).unwrap().is_some()\n}\n".to_string(),
            "row_snapshot",
        ),
        (
            "another content reader in row_inputs.rs",
            "crates/am-engine/src/row_inputs.rs",
            with("fn all(r: &Reader) -> usize {\n    r.table_rows_of(\"entries\").unwrap().len()\n}\n"),
            "table_rows_of",
        ),
        (
            "the history reader in row_inputs.rs",
            "crates/am-engine/src/row_inputs.rs",
            with("fn images(r: &Reader, id: &str) -> usize {\n    r.row_history_of(\"entries\", id).unwrap().len()\n}\n"),
            "row_history_of",
        ),
        (
            "another record in row_inputs.rs",
            "crates/am-engine/src/row_inputs.rs",
            with("fn whole(r: am_store::RowRecord) -> String {\n    r.data\n}\n"),
            "RowRecord",
        ),
        (
            "RowSnapshot named in am-providers",
            "crates/am-providers/src/common.rs",
            "pub(crate) fn render(s: &am_store::RowSnapshot) -> String {\n    s.data.clone()\n}\n".to_string(),
            "RowSnapshot",
        ),
        (
            "a use-as alias of RowSnapshot in the read site itself",
            "crates/am-engine/src/row_inputs.rs",
            with("use am_store::RowSnapshot as Image;\nfn data(i: &Image) -> usize {\n    i.data.len()\n}\n"),
            "RowSnapshot",
        ),
        (
            "a type alias of RowSnapshot in am-app",
            "crates/am-app/src/runtime.rs",
            "type Image = am_store::RowSnapshot;\nfn data(i: &Image) -> usize {\n    i.data.len()\n}\n".to_string(),
            "RowSnapshot",
        ),
    ] {
        read_site_refused(what, file, &source, token);
    }

    let site = "crates/am-engine/src/row_inputs.rs";
    let path = repo_root().join(site);
    assert!(
        path.is_file(),
        "{site} must exist: the one read site (story 3.6c)"
    );
    let code = shipped_code(site);
    assert!(
        code.contains(".row_snapshot("),
        "the read site reads through row_snapshot: {code}"
    );
    for name in [
        "Instruction",
        "TrustedInput",
        "authored",
        "from_step_output",
        "from_owner_note",
        "Part::Trusted",
    ] {
        assert!(
            !names_token(&code, name),
            "the read site names no {name}: a row is never trusted (AD-10)"
        );
    }
    let sources = shipped_sources();
    assert!(
        sources
            .iter()
            .any(|(rel, code)| rel.starts_with("crates/am-store/src/")
                && code.contains("fn row_snapshot(")),
        "am-store defines the snapshot reader"
    );
    let elsewhere: Vec<(&String, &str)> = sources
        .iter()
        .filter(|(rel, _)| !rel.starts_with("crates/am-store/src/") && rel != site)
        .flat_map(|(rel, code)| {
            ["row_snapshot", "RowSnapshot"]
                .into_iter()
                .filter(|token| names_token(code, token))
                .map(move |token| (rel, token))
        })
        .collect();
    assert!(
        elsewhere.is_empty(),
        "no shipped reader of a snapshot but the one site: {elsewhere:?}"
    );
    let out = xtask(&["check-deps"]);
    assert!(
        out.status.success(),
        "the real workspace passes the widened rules: {}",
        stderr(&out)
    );
}

// --------------------------- story 3.22, review triage round 1 (R1, 3.22)
//
// Review triage round 1 (manager, 2026-11-25), R1: the read site's own
// items, `read_rows` and `RowRead` (whose `.part` holds a row's text), are
// named in shipped code only in their definitions in
// `crates/am-engine/src/row_inputs.rs` and in the engine's driver,
// `crates/am-engine/src/run.rs`, which turns each read into a `row` edge
// and a fenced part. In the driver `read_rows` stands only as a call (never
// a function value), and neither is renamed (`use … as`, `type`) in any
// file. The read site calls the store's snapshot reader exactly once
// (`.row_snapshot(`). Whole-file tokens, as the rules beside them; tests,
// comments and literals are not shipped code. Written before the rule.

/// The engine's driver as it reads a step's rows: one call of `read_rows`
/// by path, inside `fn inputs_of` (review triage round 2, T2: named so
/// since; `fn inputs` before), each read's edge and fenced part handed on.
const RUN_READS_ROWS: &str = "use crate::row_inputs;\nfn inputs_of(reader: &am_store::Reader, step: &StepPlan) -> Result<(), am_store::StoreError> {\n    match row_inputs::read_rows(reader, &step.row_inputs)? {\n        Ok(reads) => {\n            for read in reads {\n                add(read.input, Part::from(read.part));\n            }\n        }\n        Err(why) => refuse(why),\n    }\n    Ok(())\n}\n";

/// [`read_site_files`] with the driver reading rows, and a test, a comment
/// and a literal that name `read_rows` and `RowRead`.
fn read_path_files() -> Vec<(&'static str, &'static str)> {
    let mut files: Vec<(&'static str, &'static str)> = read_site_files()
        .into_iter()
        .filter(|(f, _)| {
            *f != "crates/am-engine/src/run.rs" && *f != "crates/am-engine/tests/row_inputs.rs"
        })
        .collect();
    files.extend([
        ("crates/am-engine/src/run.rs", RUN_READS_ROWS),
        (
            "crates/am-engine/tests/row_inputs.rs",
            "use am_engine::row_inputs::{RowRead, read_rows};\nfn t(r: &am_store::Reader, rows: &[am_domain::rows::RowInput]) -> Vec<RowRead> {\n    let f = read_rows;\n    f(r, rows).unwrap().unwrap()\n}\n",
        ),
        (
            "crates/am-engine/src/cancel.rs",
            "// Rows are read by read_rows into a RowRead, in the driver only.\nconst WHY: &str = \"read_rows( and RowRead stay in run.rs and row_inputs.rs\";\n",
        ),
    ]);
    files
}

/// The read site with its in-crate tests (review triage round 2, T2: the
/// module is `pub(crate)`, so its tests are a `#[cfg(test)]` module there,
/// which calls `read_rows` and names `RowRead`).
const ROW_INPUTS_WITH_TESTS: &str = "#[cfg(test)]\nmod tests {\n    use super::{RowRead, read_rows};\n    fn t(r: &am_store::Reader, rows: &[am_domain::rows::RowInput]) -> Vec<RowRead> {\n        read_rows(r, rows).unwrap().unwrap()\n    }\n}\n";

/// `check-deps` on a tree of [`read_path_files`] with `(file, source)`
/// planted fails with a finding that names, on one stderr line,
/// `file:<line>` for a line of `source` holding `token` and the exact
/// `token`; no other allowed site is a finding.
fn read_path_refused(what: &str, file: &str, source: &str, token: &str) {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in read_path_files().iter().filter(|(f, _)| *f != file) {
        common::write(tmp.path(), f, text);
    }
    common::write(tmp.path(), file, source);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, &[token]);
    assert!(!lines.is_empty(), "{what}: the source holds {token}");
    assert!(
        err.lines().any(|finding| {
            lines
                .iter()
                .any(|n| finding.contains(&format!("{file}:{n}:")))
                && names_token(finding, token)
        }),
        "{what}: a finding names {file} at a line holding {token} ({lines:?}) and `{token}`: {err}"
    );
    for (allowed, _) in read_path_files() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

/// R1: the read site's items stand only at the read site and in the
/// engine's driver.
///
/// - The allowed tree passes: `read_rows` and `RowRead` defined in
///   `row_inputs.rs` (one `.row_snapshot(` call), `read_rows` called by
///   path in `run.rs`, a test, a comment and a literal.
/// - Each planted form fails, naming `file:line` and the exact token:
///   `read_rows` called in another engine file, in am-app and in
///   am-server; `RowRead` named in another engine file and in am-server;
///   a `pub use` re-export of both in the engine's `lib.rs`; a `use … as`
///   alias of `read_rows` and of `RowRead` in `run.rs`; a `type` alias of
///   `RowRead` in `row_inputs.rs`; `read_rows` as a function value in
///   `run.rs` and in another engine file; a second `.row_snapshot(` call in
///   `row_inputs.rs`.
/// - On the shipped tree: no file outside `row_inputs.rs` and `run.rs`
///   names `read_rows` or `RowRead`; every `read_rows` in `run.rs` is a
///   call; `row_inputs.rs` calls `.row_snapshot(` exactly once; and `cargo
///   xtask check-deps` passes on the real workspace.
#[test]
fn the_read_sites_items_stand_only_in_the_engines_driver() {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in read_path_files() {
        common::write(tmp.path(), f, text);
    }
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the read site, the driver's call, a test, a comment and a literal pass: {}",
        stderr(&out)
    );

    let with = |extra: &str| format!("{ROW_INPUTS}{extra}");
    let driver = |extra: &str| format!("{extra}{RUN_READS_ROWS}");
    for (what, file, source, token) in [
        (
            "read_rows called in another engine file",
            "crates/am-engine/src/gate.rs",
            "fn peek(r: &am_store::Reader, rows: &[RowInput]) -> bool {\n    crate::row_inputs::read_rows(r, rows).is_ok()\n}\n".to_string(),
            "read_rows",
        ),
        (
            "RowRead named in another engine file",
            "crates/am-engine/src/prompt.rs",
            "fn shown(read: &crate::row_inputs::RowRead) -> Part {\n    Part::from(read.part.clone())\n}\n".to_string(),
            "RowRead",
        ),
        (
            "read_rows called in am-app",
            "crates/am-app/src/run.rs",
            "fn peek(r: &am_store::Reader, rows: &[am_domain::rows::RowInput]) -> bool {\n    am_engine::row_inputs::read_rows(r, rows).is_ok()\n}\n".to_string(),
            "read_rows",
        ),
        (
            "read_rows called in am-server",
            "crates/am-server/src/inbox.rs",
            "fn peek(r: &am_store::Reader, rows: &[am_domain::rows::RowInput]) -> bool {\n    am_engine::row_inputs::read_rows(r, rows).is_ok()\n}\n".to_string(),
            "read_rows",
        ),
        (
            "RowRead named in am-server",
            "crates/am-server/src/runs.rs",
            "fn shown(read: am_engine::row_inputs::RowRead) -> String {\n    format!(\"{:?}\", read.input)\n}\n".to_string(),
            "RowRead",
        ),
        (
            "a re-export in the engine's lib.rs",
            "crates/am-engine/src/lib.rs",
            "pub mod row_inputs;\npub use row_inputs::{RowRead, read_rows};\n".to_string(),
            "RowRead",
        ),
        (
            "a use-as alias of read_rows in the driver",
            "crates/am-engine/src/run.rs",
            driver("use crate::row_inputs::read_rows as fetch_rows;\n"),
            "read_rows",
        ),
        (
            "a use-as alias of RowRead in the driver",
            "crates/am-engine/src/run.rs",
            driver("use crate::row_inputs::RowRead as Read;\n"),
            "RowRead",
        ),
        (
            "a type alias of RowRead at the read site",
            "crates/am-engine/src/row_inputs.rs",
            with("pub type Read = RowRead;\n"),
            "RowRead",
        ),
        (
            "read_rows as a function value in the driver",
            "crates/am-engine/src/run.rs",
            driver("fn reader() -> fn(&am_store::Reader, &[RowInput]) -> Reads {\n    row_inputs::read_rows\n}\n"),
            "read_rows",
        ),
        (
            "read_rows as a function value in another engine file",
            "crates/am-engine/src/scheduler.rs",
            "fn later() -> usize {\n    let f = crate::row_inputs::read_rows;\n    std::mem::size_of_val(&f)\n}\n".to_string(),
            "read_rows",
        ),
        (
            "a second .row_snapshot( call at the read site",
            "crates/am-engine/src/row_inputs.rs",
            with("fn again(reader: &Reader, id: &str) -> bool {\n    reader.row_snapshot(\"entries\", id).unwrap().is_some()\n}\n"),
            "row_snapshot",
        ),
    ] {
        read_path_refused(what, file, &source, token);
    }

    let site = "crates/am-engine/src/row_inputs.rs";
    let driver_file = "crates/am-engine/src/run.rs";
    let sources = shipped_sources();
    let elsewhere: Vec<(&String, &str)> = sources
        .iter()
        .filter(|(rel, _)| rel != site && rel != driver_file)
        .flat_map(|(rel, code)| {
            ["read_rows", "RowRead"]
                .into_iter()
                .filter(|token| names_token(code, token))
                .map(move |token| (rel, token))
        })
        .collect();
    assert!(
        elsewhere.is_empty(),
        "read_rows and RowRead only at the read site and in the driver: {elsewhere:?}"
    );
    let run = shipped_code(driver_file);
    let uses: Vec<&str> = run
        .match_indices("read_rows")
        .map(|(at, _)| run[at + "read_rows".len()..].trim_start())
        .collect();
    assert!(!uses.is_empty(), "the driver reads rows: {run}");
    assert!(
        uses.iter().all(|after| after.starts_with('(')),
        "every read_rows in the driver is a call"
    );
    let read_site = shipped_code(site);
    assert_eq!(
        read_site.matches(".row_snapshot(").count(),
        1,
        "the read site calls row_snapshot once: {read_site}"
    );
    let out = xtask(&["check-deps"]);
    assert!(
        out.status.success(),
        "the real workspace passes: {}",
        stderr(&out)
    );
}

// --------------------------- story 3.22, review triage round 2 (T2, T3)
//
// Review triage round 2 (manager, 2026-11-25), T2: the read-site rule
// counts names, not where they stand. `row_inputs` becomes `pub(crate)`
// (its tests move into it as a `#[cfg(test)]` module), the store's
// snapshot is called inside `fn read_rows` itself (no helper wraps it),
// and in the driver `read_rows(` is called, and `RowRead` named, only
// inside `fn inputs_of`. T3 (narrowed by the manager): nothing ties the
// driver's use of a read to the fenced slot-12 path, so `fn inputs_of`
// names no way to take a part's text out: `content`, `into_content`,
// `write!` or `{:?}`. Written before the rules.

/// [`read_path_files`] with the read site's in-crate tests.
fn read_path_files_with_tests() -> Vec<(&'static str, &'static str)> {
    read_path_files()
        .into_iter()
        .map(|(f, text)| {
            if f == "crates/am-engine/src/row_inputs.rs" {
                let both: &'static str =
                    Box::leak(format!("{ROW_INPUTS}{ROW_INPUTS_WITH_TESTS}").into_boxed_str());
                (f, both)
            } else {
                (f, text)
            }
        })
        .collect()
}

/// `check-deps` on a tree of [`read_path_files_with_tests`] with `(file,
/// source)` planted fails with a finding that names, on one stderr line,
/// `file:<line>` for a line of `source` holding `token` and the exact
/// `token`; no other allowed site is a finding.
fn placement_refused(what: &str, file: &str, source: &str, token: &str) {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in read_path_files_with_tests()
        .iter()
        .filter(|(f, _)| *f != file)
    {
        common::write(tmp.path(), f, text);
    }
    common::write(tmp.path(), file, source);
    let out = deps_at(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "{what}: must fail: {err}");
    let lines = lines_holding(source, &[token]);
    assert!(!lines.is_empty(), "{what}: the source holds {token}");
    assert!(
        err.lines().any(|finding| {
            lines
                .iter()
                .any(|n| finding.contains(&format!("{file}:{n}:")))
                && names_token(finding, token)
        }),
        "{what}: a finding names {file} at a line holding {token} ({lines:?}) and `{token}`: {err}"
    );
    for (allowed, _) in read_path_files_with_tests() {
        if allowed != file {
            assert!(
                !err.contains(allowed),
                "{what}: an allowed site is not a finding ({allowed}): {err}"
            );
        }
    }
}

/// The body of `fn <name>(` in `code` (the text between its braces), or
/// `None`: braces matched, which holds for the flattened code these tests
/// read.
fn body_of<'a>(code: &'a str, name: &str) -> Option<&'a str> {
    let start = code.find(&format!("fn {name}("))?;
    let open = start + code[start..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in code[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&code[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// T2: where the read path's calls stand.
///
/// - The allowed tree passes: the read site with its in-crate tests, the
///   snapshot call inside `fn read_rows`, the driver's call inside `fn
///   inputs_of`.
/// - Each planted form fails, naming `file:line` and the exact token: the
///   read site's one `.row_snapshot(` in a helper that `fn read_rows`
///   calls; the driver's one `read_rows(` in a function that `fn
///   inputs_of` calls; a driver function, outside `fn inputs_of`, that
///   returns `RowRead`.
/// - On the shipped tree: the engine's `lib.rs` declares `pub(crate) mod
///   row_inputs` and no `pub mod row_inputs`; the read site's tests live
///   in it (no `crates/am-engine/tests/row_inputs.rs`); its one
///   `.row_snapshot(` is in `fn read_rows`; every `read_rows` and `RowRead`
///   of `run.rs` is in `fn inputs_of`; `cargo xtask check-deps` passes.
#[test]
fn the_read_paths_calls_stand_in_read_rows_and_inputs_of() {
    let tmp = tempfile::tempdir().unwrap();
    for (f, text) in read_path_files_with_tests() {
        common::write(tmp.path(), f, text);
    }
    let out = deps_at(tmp.path());
    assert!(
        out.status.success(),
        "the read site with its tests and the driver's call in inputs_of pass: {}",
        stderr(&out)
    );

    let wrapped = "use am_domain::UntrustedData;\nuse am_domain::rows::RowInput;\nuse am_store::{Reader, RowSnapshot, StepInput, StoreError};\npub struct RowRead {\n    pub input: StepInput,\n    pub part: UntrustedData,\n}\nfn snapshot_of(reader: &Reader, table: &str, id: &str) -> Result<Option<RowSnapshot>, StoreError> {\n    reader.row_snapshot(table, id)\n}\npub fn read_rows(reader: &Reader, rows: &[RowInput]) -> Result<Result<Vec<RowRead>, String>, StoreError> {\n    let mut reads = Vec::new();\n    for row in rows {\n        let id = row.row_id.to_string();\n        let Some(snapshot) = snapshot_of(reader, &row.table_id, &id)? else {\n            return Ok(Err(format!(\"the table {} holds no row {id}\", row.table_id)));\n        };\n        reads.push(read_of(row, &id, snapshot));\n    }\n    Ok(Ok(reads))\n}\n";
    let called_elsewhere = "use crate::row_inputs;\nfn rows_for(reader: &am_store::Reader, step: &StepPlan) -> Result<Result<Reads, String>, am_store::StoreError> {\n    row_inputs::read_rows(reader, &step.row_inputs)\n}\nfn inputs_of(reader: &am_store::Reader, step: &StepPlan) -> Result<(), am_store::StoreError> {\n    match rows_for(reader, step)? {\n        Ok(reads) => {\n            for read in reads {\n                add(read.input, Part::from(read.part));\n            }\n        }\n        Err(why) => refuse(why),\n    }\n    Ok(())\n}\n";
    let returns_reads = format!(
        "{RUN_READS_ROWS}fn kept(reads: &mut Vec<row_inputs::RowRead>) -> Vec<row_inputs::RowRead> {{\n    std::mem::take(reads)\n}}\n"
    );
    assert_eq!(
        wrapped.matches(".row_snapshot(").count(),
        1,
        "one snapshot call, which the round-1 count rule alone lets pass"
    );
    for (what, file, source, token) in [
        (
            "a helper that wraps the snapshot call outside fn read_rows",
            "crates/am-engine/src/row_inputs.rs",
            wrapped.to_string(),
            "row_snapshot",
        ),
        (
            "read_rows( called outside fn inputs_of",
            "crates/am-engine/src/run.rs",
            called_elsewhere.to_string(),
            "read_rows",
        ),
        (
            "a driver function that returns RowRead",
            "crates/am-engine/src/run.rs",
            returns_reads,
            "RowRead",
        ),
    ] {
        placement_refused(what, file, &source, token);
    }

    let lib = shipped_code("crates/am-engine/src/lib.rs");
    assert!(
        lib.contains("pub(crate) mod row_inputs;") && !lib.contains("pub mod row_inputs"),
        "row_inputs is the crate's own: {lib}"
    );
    assert!(
        !repo_root()
            .join("crates/am-engine/tests/row_inputs.rs")
            .exists(),
        "the read site's tests live in it"
    );
    let site = shipped_code("crates/am-engine/src/row_inputs.rs");
    let read = body_of(&site, "read_rows").expect("row_inputs.rs defines read_rows");
    assert_eq!(read.matches(".row_snapshot(").count(), 1, "{read}");
    let run = shipped_code("crates/am-engine/src/run.rs");
    let inputs = body_of(&run, "inputs_of").expect("run.rs defines inputs_of");
    for token in ["read_rows", "RowRead"] {
        let all = run.matches(token).count();
        assert!(all > 0 || token == "RowRead", "the driver reads rows");
        assert_eq!(
            inputs.matches(token).count(),
            all,
            "every {token} of run.rs is in fn inputs_of"
        );
    }
    let out = xtask(&["check-deps"]);
    assert!(
        out.status.success(),
        "the real workspace passes: {}",
        stderr(&out)
    );
}

/// T3, as the manager narrowed it (2026-11-25): a row's text reaches a
/// `TrustedInput` only if it is first taken out of its `UntrustedData`, so
/// `fn inputs_of` names no way to take it out: `content`, `into_content`,
/// `write!` or `{:?}` (a part's text through `Debug`). A planted driver
/// naming each, in its row-read loop, fails, naming `file:line` and the
/// exact form. `TrustedInput` and `from_step_output` are not banned (the
/// shipped `inputs_of` builds a step output's and a loop-back's), and nor
/// is `format!`: the shipped `inputs_of` calls it twice for labels
/// (`format!("step:{}@{}", …)` and `format!("loop-back:{}@{}", …)`). On the
/// shipped tree `fn inputs_of` names none of the four.
#[test]
fn inputs_of_names_no_way_out_of_the_fence() {
    let plant = |line: &str| {
        RUN_READS_ROWS.replace(
            "            for read in reads {\n",
            &format!("            for read in reads {{\n                {line}\n"),
        )
    };
    assert_ne!(plant("x"), RUN_READS_ROWS, "the plant lands in the loop");
    for (what, line, token) in [
        (
            "the part's content",
            "let text = read.part.content().clone();",
            "content",
        ),
        (
            "the part's content, taken",
            "let text = read.part.clone().into_content();",
            "into_content",
        ),
        (
            "a write into a sink",
            "let _ = write!(sink, \"{}\", read.input.label);",
            "write!",
        ),
        (
            "the part through Debug",
            "let shown = std::fmt::format(format_args!(\"{:?}\", read.part));",
            "{:?}",
        ),
    ] {
        placement_refused(what, "crates/am-engine/src/run.rs", &plant(line), token);
    }

    let run = shipped_code("crates/am-engine/src/run.rs");
    let inputs = body_of(&run, "inputs_of").expect("run.rs defines inputs_of");
    for token in ["content", "into_content", "write!", "{:?}"] {
        assert!(!names_token(inputs, token), "fn inputs_of names no {token}");
    }
}
