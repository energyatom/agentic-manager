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
