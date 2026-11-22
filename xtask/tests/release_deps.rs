//! Story 2.1 guard (AD-15, owner 2026-11-22): the interim CLI Account edit
//! reads the typed id and the key only from a real terminal. A fake
//! terminal exists only in am-testkit (a dev-dependency) or under
//! `cfg(test)`, so am-testkit must never reach a non-dev dependency of the
//! `agentic` binary, through any feature. The guard reads the workspace's
//! live `cargo metadata --no-deps` (offline) and walks am-app's declared
//! normal, build and optional edges through the workspace crates (only a
//! workspace crate can depend on am-testkit); a self-check on a synthetic
//! graph shows the walk finds such an edge and ignores a dev edge.
#![forbid(unsafe_code)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use common::repo_root;
use serde_json::{Value, json};

fn live_metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    // `--no-deps`: the workspace's own packages with every declared
    // dependency (kind, optional), offline. Only a workspace crate can
    // depend on am-testkit, so walking them is enough.
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(repo_root())
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("cargo metadata prints JSON")
}

fn package<'m>(meta: &'m Value, name: &str) -> &'m Value {
    meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no package {name}"))
}

/// The workspace packages `from` reaches through declared dependencies
/// whose kind passes `follow` (`None` normal, `Some("build")`,
/// `Some("dev")`). An optional dependency counts as reached: some feature
/// can turn it on.
fn reached(meta: &Value, from: &str, follow: &dyn Fn(Option<&str>) -> bool) -> BTreeSet<String> {
    let deps: BTreeMap<String, Vec<(String, Option<String>)>> = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let list = p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| {
                    (
                        d["name"].as_str().unwrap().to_string(),
                        d["kind"].as_str().map(str::to_string),
                    )
                })
                .collect();
            (p["name"].as_str().unwrap().to_string(), list)
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut todo = vec![from.to_string()];
    while let Some(name) = todo.pop() {
        for (dep, kind) in deps.get(&name).into_iter().flatten() {
            if follow(kind.as_deref()) && seen.insert(dep.clone()) {
                todo.push(dep.clone());
            }
        }
    }
    seen
}

fn non_dev(kind: Option<&str>) -> bool {
    kind != Some("dev")
}

/// The test crates the `agentic` binary would link (am-app's normal and
/// build edges, optional ones included).
fn release_offenders(meta: &Value) -> Vec<String> {
    reached(meta, "am-app", &non_dev)
        .into_iter()
        .filter(|n| n == "am-testkit")
        .collect()
}

#[test]
fn am_testkit_is_never_a_non_dev_dependency_of_the_agentic_binary() {
    let meta = live_metadata();
    let app = package(&meta, "am-app");
    assert!(
        app["targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "agentic" && t["kind"] == json!(["bin"])),
        "am-app builds the agentic binary"
    );
    assert_eq!(
        release_offenders(&meta),
        Vec::<String>::new(),
        "am-testkit (and its FakeTerminal) reaches the agentic binary"
    );
    // The walk looks at the right graph: with dev edges, am-app reaches it.
    assert!(
        reached(&meta, "am-app", &|_| true).contains("am-testkit"),
        "am-testkit is am-app's dev-dependency"
    );
    // No feature of any crate the binary links turns a test crate on.
    let linked = reached(&meta, "am-app", &non_dev);
    for name in linked.iter().chain(std::iter::once(&"am-app".to_string())) {
        let Some(p) = meta["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name.as_str())
        else {
            continue;
        };
        for (feature, enables) in p["features"].as_object().into_iter().flatten() {
            for e in enables.as_array().unwrap() {
                let e = e.as_str().unwrap();
                assert!(
                    !e.contains("am-testkit"),
                    "{name} feature `{feature}` enables `{e}`"
                );
            }
        }
    }
}

/// The source of the binary never names the test kit.
#[test]
fn am_app_sources_never_name_the_test_kit() {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                if text.contains("am_testkit") {
                    out.push(p.display().to_string());
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(&repo_root().join("crates/am-app/src"), &mut found);
    assert!(found.is_empty(), "am-app/src names am_testkit: {found:?}");
}

/// Self-check of the walk on a synthetic graph: a normal, build or
/// optional edge from am-app (directly or through another workspace crate)
/// to am-testkit is found; a dev edge is not.
#[test]
fn the_guard_finds_a_normal_edge_to_the_test_kit() {
    let graph = |kind: Value, optional: bool, via: bool| {
        let to_kit = json!([{ "name": "am-testkit", "kind": kind, "optional": optional }]);
        let (app, mid) = if via {
            (
                json!([{ "name": "am-config", "kind": null, "optional": false }]),
                to_kit,
            )
        } else {
            (to_kit, json!([]))
        };
        json!({ "packages": [
            { "name": "am-app", "dependencies": app },
            { "name": "am-config", "dependencies": mid },
            { "name": "am-testkit", "dependencies": [] }
        ]})
    };
    let kit = vec!["am-testkit".to_string()];
    assert_eq!(release_offenders(&graph(Value::Null, false, false)), kit);
    assert_eq!(release_offenders(&graph(Value::Null, false, true)), kit);
    assert_eq!(release_offenders(&graph(Value::Null, true, false)), kit);
    assert_eq!(release_offenders(&graph(json!("build"), false, false)), kit);
    assert!(release_offenders(&graph(json!("dev"), false, false)).is_empty());
    assert!(release_offenders(&graph(json!("dev"), false, true)).is_empty());
}

// ------------------------------------------------ the terminal itself

/// One `impl … Terminal for <type>` found in a crate's sources.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalImpl {
    /// Path from the repo root, forward slashes.
    file: String,
    /// The implementing type.
    ty: String,
}

/// The type of an `impl [<…>] [path::]Terminal for <type>` line, if `line`
/// is one (the trait's last path segment is exactly `Terminal`).
fn terminal_impl_type(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("impl")?;
    if !rest.starts_with([' ', '<']) {
        return None;
    }
    let (head, tail) = rest.split_once(" for ")?;
    let trait_path = head.rsplit([' ', '>']).next()?;
    if trait_path.rsplit("::").next()? != "Terminal" {
        return None;
    }
    let ty: String = tail
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    (!ty.is_empty()).then_some(ty)
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every `Terminal` implementation under `crates/*/src` of the repo at
/// `root`, am-testkit left out (the one crate that may hold a fake).
fn terminal_impls(root: &Path) -> Vec<TerminalImpl> {
    let mut found = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join("crates")) else {
        return found;
    };
    for krate in crates.flatten() {
        if krate.file_name() == "am-testkit" {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&krate.path().join("src"), &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let rel = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            for line in text.lines() {
                if let Some(ty) = terminal_impl_type(line) {
                    found.push(TerminalImpl {
                        file: rel.clone(),
                        ty,
                    });
                }
            }
        }
    }
    found.sort_by(|a, b| (&a.file, &a.ty).cmp(&(&b.file, &b.ty)));
    found
}

const REAL_TERMINAL_FILE: &str = "crates/am-app/src/terminal.rs";
const REAL_TERMINAL_TYPE: &str = "OwnerTerminal";
const MAIN_FILE: &str = "crates/am-app/src/main.rs";
/// The one construction of the real terminal.
const REAL_CONSTRUCTION: &str = "OwnerTerminal::new()";
/// How `main.rs` binds it: the only value a `terminal` may be given there.
const REAL_BOXED: &str = "Box::new(am_app::terminal::OwnerTerminal::new())";

/// The code of a Rust file as one string: whole-line comments dropped
/// (they may name the rule), all whitespace removed.
fn code_of(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .flat_map(str::split_whitespace)
        .collect()
}

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Why the terminal a command of the `agentic` binary gets could be other
/// than the owner's real one, read from am-app's sources (review of story
/// 3.1: the terminal is built once in `main.rs` and handed to each
/// command's context, by field shorthand too, so a `terminal:` field count
/// would not see it):
///
/// - outside `terminal.rs`, am-app's code names `OwnerTerminal` only as the
///   one `OwnerTerminal::new()`, which appears exactly once, in `main.rs`
///   (no second construction, no `OwnerTerminal::default()`, no literal);
/// - `main.rs` names no other type ending in `Terminal`, nor the `Terminal`
///   trait (`Box<dyn Terminal>`): it can hold a terminal only by building
///   the real one;
/// - every `terminal:` field and every `terminal =` binding in `main.rs`
///   is given exactly `Box::new(am_app::terminal::OwnerTerminal::new())`,
///   so a shorthand `terminal` cannot come from another constructor.
fn construction_offences(root: &Path) -> Vec<String> {
    let mut why = Vec::new();
    let mut files = Vec::new();
    rust_files(&root.join("crates/am-app/src"), &mut files);
    files.sort();
    // (file, constructions, other mentions) outside the real terminal's file.
    let mut built: Vec<(String, usize)> = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == REAL_TERMINAL_FILE {
            continue;
        }
        let code = code_of(&std::fs::read_to_string(file).unwrap_or_default());
        let constructions = code.matches(REAL_CONSTRUCTION).count();
        let mentions = code.matches(REAL_TERMINAL_TYPE).count();
        if constructions > 0 {
            built.push((rel.clone(), constructions));
        }
        if mentions > constructions {
            why.push(format!(
                "{rel} names {REAL_TERMINAL_TYPE} other than as `{REAL_CONSTRUCTION}`: outside {REAL_TERMINAL_FILE} the real terminal is only built, once, in {MAIN_FILE}"
            ));
        }
    }
    if built != [(MAIN_FILE.to_string(), 1)] {
        let found: Vec<String> = built.iter().map(|(f, n)| format!("{n} in {f}")).collect();
        why.push(format!(
            "`{REAL_CONSTRUCTION}` must appear exactly once in crates/am-app/src, in {MAIN_FILE}: the binary builds one terminal and every command takes it (found {})",
            if found.is_empty() {
                "none".to_string()
            } else {
                found.join(", ")
            }
        ));
    }
    let main = code_of(&std::fs::read_to_string(root.join(MAIN_FILE)).unwrap_or_default());
    // A type whose name ends in `Terminal` (or the trait itself): only the
    // real one, in its one construction.
    for (at, word) in main.match_indices("Terminal") {
        let after = &main[at + word.len()..];
        if after.starts_with(ident_char) {
            // A longer name (`TerminalPrompt`), not a `Terminal` type.
            continue;
        }
        let before = &main[..at];
        let real = before.ends_with("::Owner") && after.starts_with("::new()");
        if !real {
            let name: String = before
                .chars()
                .rev()
                .take_while(|c| ident_char(*c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            why.push(format!(
                "{MAIN_FILE} names `{name}Terminal`: it may name no `Terminal` type or trait but the one `{REAL_BOXED}`"
            ));
        }
    }
    // A `terminal` field or binding: given only the real terminal.
    for (at, word) in main.match_indices("terminal") {
        let after = &main[at + word.len()..];
        let given = if let Some(rest) = after.strip_prefix(':') {
            // `terminal::` is a path, not a field.
            (!rest.starts_with(':')).then_some(rest)
        } else if let Some(rest) = after.strip_prefix('=') {
            // `==` compares and `=>` is a match arm; neither binds.
            (!rest.starts_with('=') && !rest.starts_with('>')).then_some(rest)
        } else {
            None
        };
        if given.is_some_and(|rest| !rest.starts_with(REAL_BOXED)) {
            why.push(format!(
                "{MAIN_FILE} gives a `terminal` something other than `{REAL_BOXED}`: every command's terminal is the one real terminal"
            ));
        }
    }
    why
}

/// Why the repo at `root` could hand the `agentic` binary a terminal that
/// is not the owner's real one; empty when it cannot:
///
/// - a `Terminal` implementation outside am-testkit that is not
///   `OwnerTerminal`'s in `crates/am-app/src/terminal.rs`;
/// - that file, or the line declaring its module, under a `cfg(feature`
///   attribute (a feature could then swap the terminal);
/// - the real terminal built anywhere but once, as `OwnerTerminal::new()`
///   in `crates/am-app/src/main.rs`, or `main.rs` naming another `Terminal`
///   type or binding a `terminal` from anything else (see
///   [`construction_offences`]).
fn terminal_offences(root: &Path) -> Vec<String> {
    let mut why = Vec::new();
    let impls = terminal_impls(root);
    if !impls
        .iter()
        .any(|i| i.file == REAL_TERMINAL_FILE && i.ty == REAL_TERMINAL_TYPE)
    {
        why.push(format!(
            "{REAL_TERMINAL_FILE} does not implement Terminal for {REAL_TERMINAL_TYPE}"
        ));
    }
    for i in &impls {
        if i.file != REAL_TERMINAL_FILE || i.ty != REAL_TERMINAL_TYPE {
            why.push(format!(
                "{} implements Terminal for {}: the only one outside am-testkit is {REAL_TERMINAL_TYPE} in {REAL_TERMINAL_FILE}",
                i.file, i.ty
            ));
        }
    }
    let feature_gate = |text: &str| text.contains("cfg(feature") || text.contains("feature =");
    let real = std::fs::read_to_string(root.join(REAL_TERMINAL_FILE)).unwrap_or_default();
    if feature_gate(&real) {
        why.push(format!(
            "{REAL_TERMINAL_FILE} names a cargo feature: no feature may choose the terminal"
        ));
    }
    // No environment variable may choose or loosen the terminal (owner,
    // 2026-11-22): the real terminal's code reads none. Comments may name
    // the rule.
    let reads_env = real
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .any(|l| l.contains("env::") || l.contains("env!("));
    if reads_env {
        why.push(format!(
            "{REAL_TERMINAL_FILE} reads the environment: no env var may choose the terminal"
        ));
    }
    let lib = std::fs::read_to_string(root.join("crates/am-app/src/lib.rs")).unwrap_or_default();
    let lines: Vec<&str> = lib.lines().map(str::trim).collect();
    match lines.iter().position(|l| *l == "pub mod terminal;") {
        None => why.push("crates/am-app/src/lib.rs does not declare `pub mod terminal;`".into()),
        Some(at) => {
            // The attributes right above the declaration.
            let gated = lines[..at]
                .iter()
                .rev()
                .take_while(|l| l.starts_with("#["))
                .any(|l| l.starts_with("#[cfg"));
            if gated {
                why.push(
                    "crates/am-app/src/lib.rs declares `mod terminal` under a cfg attribute".into(),
                );
            }
        }
    }
    why.extend(construction_offences(root));
    why
}

/// Story 2.1 (review): the guard above knows only the name `am-testkit`.
/// This one reads the sources: outside am-testkit the only `Terminal`
/// implementations are `OwnerTerminal`'s, in am-app's `terminal.rs`, under
/// no cargo feature, and am-app builds it exactly once, in `main.rs`, as
/// the only terminal any command is given (review of story 3.1).
#[test]
fn the_only_terminal_outside_the_test_kit_is_the_owners_real_one() {
    let root = repo_root();
    assert_eq!(terminal_offences(&root), Vec::<String>::new());
    let impls = terminal_impls(&root);
    assert!(!impls.is_empty(), "the scan found the real terminal");
    assert!(
        impls
            .iter()
            .all(|i| i.file == REAL_TERMINAL_FILE && i.ty == REAL_TERMINAL_TYPE),
        "{impls:?}"
    );
    // The scan sees the fake where it may live: it is not blind to it.
    let kit = std::fs::read_to_string(root.join("crates/am-testkit/src/lib.rs")).unwrap();
    assert!(
        kit.lines()
            .any(|l| terminal_impl_type(l).as_deref() == Some("FakeTerminal")),
        "am-testkit holds the fake terminal"
    );
}

/// Self-check on a synthetic tree: a clean tree passes; a second
/// implementation (in am-app or another crate), a feature-gated real one, a
/// gated module and a `main.rs` built with another terminal are each
/// found; an implementation inside am-testkit is not. One construction
/// shared by field shorthand is clean; a second construction of the real
/// terminal, a shorthand `terminal` bound from another constructor and
/// another `Terminal` type or the trait named in `main.rs` are each found.
#[test]
fn the_terminal_guard_finds_a_fake_a_feature_gate_and_another_main() {
    const REAL: &str = "pub struct OwnerTerminal;\nimpl Terminal for OwnerTerminal {\n}\n";
    const LIB: &str = "pub mod accounts;\npub mod terminal;\n";
    const MAIN: &str = "fn main() {\n    let ctx = AccountsContext {\n        terminal: Box::new(am_app::terminal::OwnerTerminal::new()),\n    };\n}\n";
    let tree = |files: &[(&str, &str)]| {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in [
            (REAL_TERMINAL_FILE, REAL),
            ("crates/am-app/src/lib.rs", LIB),
            ("crates/am-app/src/main.rs", MAIN),
            (
                "crates/am-testkit/src/lib.rs",
                "impl Terminal for FakeTerminal {}\n",
            ),
        ]
        .iter()
        .chain(files)
        {
            common::write(dir.path(), rel, text);
        }
        dir
    };
    let offences = |files: &[(&str, &str)]| terminal_offences(tree(files).path());

    assert_eq!(offences(&[]), Vec::<String>::new(), "a clean tree");

    let fake = "impl am_domain::Terminal for ScriptedTerminal {\n}\n";
    for rel in ["crates/am-app/src/scripted.rs", "crates/am-net/src/lib.rs"] {
        let why = offences(&[(rel, fake)]);
        assert_eq!(why.len(), 1, "{why:?}");
        assert!(why[0].contains("ScriptedTerminal") && why[0].contains(rel));
    }
    let generic = "impl<T: Send> Terminal for Wrapped<T> {\n}\n";
    assert_eq!(
        offences(&[("crates/am-app/src/wrapped.rs", generic)]).len(),
        1
    );
    // Another type in the real file is a second terminal too.
    let second = format!("{REAL}impl Terminal for EnvTerminal {{}}\n");
    assert_eq!(offences(&[(REAL_TERMINAL_FILE, &second)]).len(), 1);

    let gated = format!("#[cfg(feature = \"scripted\")]\nmod scripted {{}}\n{REAL}");
    let why = offences(&[(REAL_TERMINAL_FILE, &gated)]);
    assert!(why.iter().any(|w| w.contains("feature")), "{why:?}");

    let by_env = format!(
        "{REAL}pub fn interactive() -> bool {{ std::env::var(\"AGENTIC_TTY\").is_ok() }}\n"
    );
    let why = offences(&[(REAL_TERMINAL_FILE, &by_env)]);
    assert!(why.iter().any(|w| w.contains("environment")), "{why:?}");

    let gated_mod = "pub mod accounts;\n#[cfg(feature = \"real-terminal\")]\npub mod terminal;\n";
    let why = offences(&[("crates/am-app/src/lib.rs", gated_mod)]);
    assert!(why.iter().any(|w| w.contains("cfg attribute")), "{why:?}");

    let other_main = MAIN.replace("OwnerTerminal::new()", "terminal_from_env()");
    let why = offences(&[("crates/am-app/src/main.rs", &other_main)]);
    assert!(why.iter().any(|w| w.contains("main.rs")), "{why:?}");
    let two = format!("{MAIN}fn other() {{ let c = C {{ terminal: Box::new(Scripted) }}; }}\n");
    let why = offences(&[("crates/am-app/src/main.rs", &two)]);
    assert!(why.iter().any(|w| w.contains("main.rs")), "{why:?}");

    // Review of story 3.1: the terminal is built once and shared. A main
    // that builds the owner's side once and hands its terminal to two
    // contexts by field shorthand is clean.
    const SHARED: &str = "fn owner_side() -> OwnerSide {\n    OwnerSide {\n        terminal: Box::new(am_app::terminal::OwnerTerminal::new()),\n    }\n}\nfn accounts() {\n    let OwnerSide { terminal } = owner_side();\n    let ctx = AccountsContext { terminal };\n}\nfn run() {\n    let OwnerSide { terminal } = owner_side();\n    let ctx = RunContext { terminal };\n    open(&mut TerminalPrompt);\n}\n";
    assert_eq!(
        offences(&[("crates/am-app/src/main.rs", SHARED)]),
        Vec::<String>::new(),
        "one construction, shared by shorthand"
    );
    // A second construction of the real terminal, in main or elsewhere in
    // am-app, is found although no `terminal:` field names it.
    let twice = format!("{MAIN}fn other() {{ let t = am_app::terminal::OwnerTerminal::new(); }}\n");
    let why = offences(&[("crates/am-app/src/main.rs", &twice)]);
    assert!(why.iter().any(|w| w.contains("exactly once")), "{why:?}");
    let elsewhere = "pub fn context() { let t = crate::terminal::OwnerTerminal::new(); }\n";
    let why = offences(&[("crates/am-app/src/run.rs", elsewhere)]);
    assert!(
        why.iter()
            .any(|w| w.contains("exactly once") && w.contains("crates/am-app/src/run.rs")),
        "{why:?}"
    );
    let by_default = "pub fn context() { let t = crate::terminal::OwnerTerminal::default(); }\n";
    let why = offences(&[("crates/am-app/src/run.rs", by_default)]);
    assert!(
        why.iter().any(|w| w.contains("crates/am-app/src/run.rs")),
        "{why:?}"
    );
    // A context whose shorthand `terminal` comes from another constructor.
    let shorthand = format!(
        "{MAIN}fn other() {{\n    let terminal = Box::new(scripted::Scripted::new());\n    let ctx = RunContext {{ terminal }};\n}}\n"
    );
    let why = offences(&[("crates/am-app/src/main.rs", &shorthand)]);
    assert!(
        why.iter()
            .any(|w| w.contains("main.rs") && w.contains("`terminal`")),
        "{why:?}"
    );
    // Another `Terminal` type, or the trait itself, named in main.
    for other in [
        "fn other() { let t = FakeTerminal::new(); }\n",
        "fn other(t: Box<dyn Terminal>) {}\n",
        "fn other(t: Box<dyn am_domain::Terminal>) {}\n",
    ] {
        let named = format!("{MAIN}{other}");
        let why = offences(&[("crates/am-app/src/main.rs", &named)]);
        assert!(
            why.iter()
                .any(|w| w.contains("main.rs") && w.contains("Terminal")),
            "{other}: {why:?}"
        );
    }
    // Comments may name the rule and the type.
    let commented = format!(
        "{MAIN}// The owner's terminal: OwnerTerminal::new() is built once; no Box<dyn Terminal> here.\n"
    );
    assert_eq!(
        offences(&[("crates/am-app/src/main.rs", &commented)]),
        Vec::<String>::new()
    );

    // Not a Terminal implementation: other traits, and the trait itself.
    for line in [
        "impl Display for OwnerTerminal {",
        "impl TerminalPrompt for X {",
        "pub trait Terminal: Send {",
        "impl Drop for Restore {",
    ] {
        assert_eq!(terminal_impl_type(line), None, "{line}");
    }
    assert_eq!(
        terminal_impl_type("    impl Terminal for OwnerTerminal {").as_deref(),
        Some("OwnerTerminal")
    );
}
