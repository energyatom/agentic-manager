//! `cargo xtask runtime-paths` (story 1.13, AD-32): a Rust string literal
//! under crates/ that spells a runtime path (`.agentic` at its start or
//! right after `/`) fails the check, naming file and line, unless it is in
//! `crates/am-domain/src/runtime.rs`. Written first.
#![forbid(unsafe_code)]

mod common;

use common::{stderr, stdout, write, xtask};

fn scan(root: &std::path::Path) -> std::process::Output {
    xtask(&["runtime-paths", "--root", &root.to_string_lossy()])
}

#[test]
fn a_planted_runtime_path_literal_fails_with_file_and_line() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "//! generic\n\npub const P: &str = \"/a/.agentic/b\";\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "a planted literal must fail: {err}");
    assert!(
        err.contains("crates/x/src/lib.rs:3"),
        "names file:line: {err}"
    );
    assert!(err.contains("/a/.agentic/b"), "names the literal: {err}");
}

#[test]
fn the_layout_file_comments_and_other_spellings_pass() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-domain/src/runtime.rs",
        "pub const LEGACY_DIR: &str = \".agentic\";\npub const OPEN_MARKER: &str = \".agentic-open\";\n",
    );
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "// the old `.agentic/` folder\npub const ID: &str = \"app.agentic-manager\";\npub const W: &str = \"a .agentic folder\";\n",
    );
    let out = scan(tmp.path());
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 files clean"), "{}", stdout(&out));
}

#[test]
fn the_repository_is_clean() {
    let out = xtask(&["runtime-paths"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

/// Story 2.12 (AD-17): `am_domain::runtime::AppDataLayout` joins
/// `RuntimeLayout` as the only speller of the app data dir's store files:
/// a literal naming the user store or its lock (`user.db`, `user.lock`, at
/// the start or after a `/`) outside `crates/am-domain/src/runtime.rs`
/// fails, naming file, line and the layout; the same literals in that file
/// pass.
#[test]
fn a_planted_user_store_path_literal_fails_and_the_layout_file_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "//! generic\n\npub const DB: &str = \"user.db\";\npub const LOCK: &str = \"/a/b/user.lock\";\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "a planted literal must fail: {err}");
    assert!(err.contains("crates/x/src/lib.rs:3"), "{err}");
    assert!(err.contains("crates/x/src/lib.rs:4"), "{err}");
    assert!(err.contains("AppDataLayout"), "names the layout: {err}");

    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-domain/src/runtime.rs",
        "pub fn user_db() -> &'static str { \"user.db\" }\npub fn user_lock() -> &'static str { \"user.lock\" }\n",
    );
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "// the user.db of this Mac\npub const W: &str = \"a superuser.dbx name\";\n",
    );
    let out = scan(tmp.path());
    assert!(out.status.success(), "{}", stderr(&out));
}
