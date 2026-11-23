//! RULE-15 trust test (story 3.2, AD-13): owner-only start. The server
//! refuses to start, in words naming the path and the `chmod`, when the
//! app data dir or the open project's runtime folder is not 0700, or a file
//! in either is not 0600 (2.12's owner-only check, extended to the runtime
//! folder and the `serving` records). `am_app::serving::owner_only_at_start`
//! is the check `agentic serve` runs before it binds. Rows: "Loose app
//! data", "Loose runtime folder". Written first.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use am_app::runtime::Places;
use am_app::serving::{owner_only_at_start, write_cli_token};
use am_domain::RuntimeLayout;

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

struct Env {
    dir: tempfile::TempDir,
    places: Places,
    runtime: RuntimeLayout,
}

/// An owner-only app data dir (with a `serving` folder and a token file)
/// and an owner-only runtime folder holding a 0600 file.
fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let places = Places::under(&dir.path().join("support"), &dir.path().join("home"));
    let server = am_server::AppState::new(1);
    write_cli_token(&places, 4242, &server.cli_token()).unwrap();
    let rt = dir.path().join("rt");
    std::fs::create_dir_all(&rt).unwrap();
    chmod(&rt, 0o700);
    let runtime = RuntimeLayout::at(rt);
    std::fs::write(runtime.serve_port(), "1\n").unwrap();
    chmod(&runtime.serve_port(), 0o600);
    Env {
        dir,
        places,
        runtime,
    }
}

fn refused(e: &Env, path: &Path, chmod_words: &str) {
    let err = owner_only_at_start(&e.places, Some(&e.runtime))
        .expect_err("the server must not start")
        .to_string();
    assert!(
        err.contains(&path.display().to_string()),
        "names the path {}: {err}",
        path.display()
    );
    assert!(
        err.contains(&format!("{chmod_words} {}", path.display())),
        "names `{chmod_words} {}`: {err}",
        path.display()
    );
}

/// The owner-only case starts (so the refusals below are about the modes).
#[test]
fn owner_only_folders_and_files_start() {
    let e = env();
    owner_only_at_start(&e.places, Some(&e.runtime)).unwrap();
    owner_only_at_start(&e.places, None).unwrap();
    let _ = &e.dir;
}

/// Loose app data: the app data dir at 0755 refuses the start, worded with
/// `chmod 700 <dir>`.
#[test]
fn a_loose_app_data_dir_refuses_the_start() {
    let e = env();
    chmod(&e.places.app_data, 0o755);
    refused(&e, &e.places.app_data, "chmod 700");
    // Also with no project open.
    assert!(owner_only_at_start(&e.places, None).is_err());
}

/// Loose app data: a token file at 0644 (from an earlier launch, or this
/// one loosened) refuses the start, worded with `chmod 600 <file>`.
#[test]
fn a_loose_token_file_refuses_the_start() {
    let e = env();
    let token = e.places.user.cli_token(4242);
    assert!(token.exists());
    chmod(&token, 0o644);
    refused(&e, &token, "chmod 600");
}

/// Loose app data: the `serving` folder at 0755 refuses the start.
#[test]
fn a_loose_serving_folder_refuses_the_start() {
    let e = env();
    let serving = e.places.user.serving_dir();
    chmod(&serving, 0o755);
    refused(&e, &serving, "chmod 700");
}

/// Loose runtime folder: the project's runtime folder at 0755 refuses the
/// start, worded with `chmod 700 <folder>`.
#[test]
fn a_loose_runtime_folder_refuses_the_start() {
    let e = env();
    chmod(e.runtime.dir(), 0o755);
    refused(&e, e.runtime.dir(), "chmod 700");
}

/// Loose runtime folder: a file in it at 0644 refuses the start, worded
/// with `chmod 600 <file>`.
#[test]
fn a_loose_file_in_the_runtime_folder_refuses_the_start() {
    let e = env();
    chmod(&e.runtime.serve_port(), 0o644);
    refused(&e, &e.runtime.serve_port(), "chmod 600");
}

// ------------------------------------------- review patch round 1 (P1)

/// A 0700 folder (and its parents, 0700) under `root`.
fn private_dirs(root: &Path, rel: &str) -> std::path::PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/') {
        p = p.join(part);
        if !p.exists() {
            std::fs::create_dir(&p).unwrap();
        }
        chmod(&p, 0o700);
    }
    p
}

/// A file at `mode`.
fn file_at(path: &Path, mode: u32) {
    std::fs::write(path, "x\n").unwrap();
    chmod(path, mode);
}

/// P1: a 0644 file two levels deep in the app data dir refuses the start,
/// worded with its path and `chmod 600 <file>` (the walk is recursive).
#[test]
fn a_loose_file_two_levels_deep_in_app_data_refuses_the_start() {
    let e = env();
    let deep = private_dirs(&e.places.app_data, "one/two");
    let file = deep.join("record");
    file_at(&file, 0o644);
    refused(&e, &file, "chmod 600");
}

/// P1: a 0644 file two levels deep in the runtime folder refuses the start.
#[test]
fn a_loose_file_two_levels_deep_in_the_runtime_folder_refuses_the_start() {
    let e = env();
    let deep = private_dirs(e.runtime.dir(), "one/two");
    let file = deep.join("record");
    file_at(&file, 0o644);
    refused(&e, &file, "chmod 600");
}

/// P1: a 0755 subfolder of the app data dir refuses the start, worded with
/// `chmod 700 <folder>`.
#[test]
fn a_loose_subfolder_of_app_data_refuses_the_start() {
    let e = env();
    let sub = private_dirs(&e.places.app_data, "one");
    chmod(&sub, 0o755);
    refused(&e, &sub, "chmod 700");
}

/// P1: a 0755 subfolder two levels deep in the runtime folder refuses the
/// start.
#[test]
fn a_loose_subfolder_of_the_runtime_folder_refuses_the_start() {
    let e = env();
    let sub = private_dirs(e.runtime.dir(), "one/two");
    chmod(&sub, 0o755);
    refused(&e, &sub, "chmod 700");
}

/// P1: a symlink anywhere inside the app data dir or the runtime folder
/// refuses the start, in words naming it (even one pointing at a 0600
/// file).
#[test]
fn a_symlink_inside_refuses_the_start() {
    for in_runtime in [false, true] {
        let e = env();
        let target = e.dir.path().join("elsewhere");
        file_at(&target, 0o600);
        let root = if in_runtime {
            e.runtime.dir().to_path_buf()
        } else {
            e.places.app_data.clone()
        };
        let link = private_dirs(&root, "one").join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = owner_only_at_start(&e.places, Some(&e.runtime))
            .expect_err("a symlink inside must refuse the start")
            .to_string();
        assert!(
            err.contains(&link.display().to_string()),
            "names the link {}: {err}",
            link.display()
        );
    }
}

/// P1: a clean nested tree (folders 0700, files 0600 at every depth) starts,
/// and a missing app data dir or runtime folder is still accepted.
#[test]
fn a_clean_nested_tree_and_missing_folders_start() {
    let e = env();
    for root in [e.places.app_data.clone(), e.runtime.dir().to_path_buf()] {
        let deep = private_dirs(&root, "one/two/three");
        file_at(&deep.join("record"), 0o600);
        file_at(&root.join("one").join("other"), 0o600);
    }
    owner_only_at_start(&e.places, Some(&e.runtime)).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let places = Places::under(&dir.path().join("support"), &dir.path().join("home"));
    let missing = RuntimeLayout::at(dir.path().join("rt"));
    owner_only_at_start(&places, Some(&missing)).unwrap();
}

/// P1: a metadata error other than "not found" is refused, not taken for a
/// missing folder: the app data dir cannot be looked at because its parent
/// denies search.
#[test]
fn an_unreadable_app_data_dir_refuses_the_start() {
    let e = env();
    let parent = e.places.app_data.parent().unwrap().to_path_buf();
    chmod(&parent, 0o000);
    let result = owner_only_at_start(&e.places, Some(&e.runtime));
    chmod(&parent, 0o700);
    assert!(
        result.is_err(),
        "a folder that cannot be looked at must not pass as missing"
    );
}
