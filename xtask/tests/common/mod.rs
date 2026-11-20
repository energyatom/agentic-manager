//! Shared helpers for the xtask acceptance tests (story 1.2).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The repository root (xtask's parent directory).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits inside the repo root")
        .to_path_buf()
}

/// Runs the built `xtask` binary with `args` from the repo root.
pub fn xtask(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("xtask binary runs")
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Writes `contents` at `root/rel`, creating parent folders.
pub fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("mkdir");
    std::fs::write(path, contents).expect("write file");
}
