//! Refactor sweep (deferred from story 1.3's review): tokio's `test-util`
//! feature is unified across a workspace test build, so a crate whose tests
//! pause time without enabling `test-util` itself passes `just check` but
//! fails `cargo nextest run -p <crate>`. This guard names such a crate.
#![forbid(unsafe_code)]

mod common;

use std::path::{Path, PathBuf};

use common::{repo_root, write};

/// Calls that need tokio's `test-util` feature.
const PAUSING: &[&str] = &[
    "start_paused",
    "time::pause(",
    "time::advance(",
    "time::resume(",
];

/// The same calls after `use tokio::time::{..}`, matched only in a file
/// that names `tokio::time`.
const PAUSING_IMPORTED: &[&str] = &["pause()", "advance(", "resume()"];

/// The workspace members named in the root manifest's `members = [..]`.
fn members(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
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

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// True when any Rust file of the crate at `dir` pauses tokio time.
fn pauses_time(dir: &Path) -> bool {
    let mut files = Vec::new();
    for sub in ["src", "tests", "benches", "examples"] {
        rust_files(&dir.join(sub), &mut files);
    }
    // This guard's own file names the calls as text.
    files.retain(|f| !f.ends_with("xtask/tests/paused_time.rs"));
    files.iter().any(|f| {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        PAUSING.iter().any(|p| text.contains(p))
            || (text.contains("tokio::time") && PAUSING_IMPORTED.iter().any(|p| text.contains(p)))
    })
}

/// True when the crate's own manifest enables tokio's `test-util` (in any
/// dependency table).
fn enables_test_util(manifest: &str) -> bool {
    manifest.lines().any(|l| {
        let key = l
            .split(['=', '.'])
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        key == "tokio" && l.contains("\"test-util\"")
    })
}

/// The members whose code pauses time without enabling `test-util`.
fn offenders(root: &Path) -> Vec<String> {
    members(root)
        .into_iter()
        .filter(|m| {
            let dir = root.join(m);
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
            pauses_time(&dir) && !enables_test_util(&manifest)
        })
        .collect()
}

#[test]
fn every_crate_that_pauses_time_enables_tokio_test_util_itself() {
    let found = offenders(&repo_root());
    assert!(
        found.is_empty(),
        "these crates pause tokio time in their code or tests without enabling \
         tokio's `test-util` in their own Cargo.toml (add it to their tokio \
         dev-dependency): {found:?}"
    );
}

#[test]
fn the_guard_names_a_crate_that_pauses_time_without_test_util() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\n    \"crates/quiet\",\n    \"crates/paused\",\n    \"crates/declared\",\n]\n",
    );
    write(
        root,
        "crates/quiet/Cargo.toml",
        "[dependencies]\ntokio.workspace = true\n",
    );
    write(
        root,
        "crates/quiet/tests/t.rs",
        "#[tokio::test]\nasync fn t() {}\n",
    );
    write(
        root,
        "crates/paused/Cargo.toml",
        "[dev-dependencies]\ntokio.workspace = true\n",
    );
    write(
        root,
        "crates/paused/tests/t.rs",
        "#[tokio::test(start_paused = true)]\nasync fn t() {}\n",
    );
    write(
        root,
        "crates/declared/Cargo.toml",
        "[dev-dependencies]\ntokio = { workspace = true, features = [\"test-util\"] }\n",
    );
    write(
        root,
        "crates/declared/src/lib.rs",
        "pub fn f() { tokio::time::pause(); }\n",
    );
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\n    \"crates/quiet\",\n    \"crates/paused\",\n    \"crates/declared\",\n    \"crates/imported\",\n    \"crates/lookalike\",\n]\n",
    );
    write(
        root,
        "crates/imported/Cargo.toml",
        "[dev-dependencies]\ntokio.workspace = true\n",
    );
    write(
        root,
        "crates/imported/tests/t.rs",
        "use tokio::time::{advance, pause};\n#[tokio::test]\nasync fn t() { pause(); advance(std::time::Duration::from_secs(1)).await; }\n",
    );
    write(
        root,
        "crates/lookalike/Cargo.toml",
        "[dev-dependencies]\ntokio.workspace = true\ntokio-util = { version = \"1\", features = [\"test-util\"] }\n",
    );
    write(
        root,
        "crates/lookalike/tests/t.rs",
        "#[tokio::test(start_paused = true)]\nasync fn t() {}\n",
    );
    assert_eq!(
        offenders(root),
        vec![
            "crates/paused".to_string(),
            "crates/imported".to_string(),
            "crates/lookalike".to_string(),
        ]
    );
}
