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
            "crates/am-engine/src/journal.rs",
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
