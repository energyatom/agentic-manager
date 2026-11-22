//! Story 2.2 (AD-12): only am-net builds an HTTP client. The workspace's
//! clippy `disallowed-methods` rule bans `reqwest::Client::new`,
//! `Client::builder`, `ClientBuilder::new`, `reqwest::get` and, from this
//! story, `Client::default()` and `ClientBuilder::default()`.
//!
//! The probe copies `fixtures/egress-probe` to a temp folder with the
//! repo's own `clippy.toml`, `Cargo.lock` and `rust-toolchain.toml`, puts
//! one case in as `src/lib.rs`, and runs clippy offline against the real
//! reqwest at the workspace's pin: each case that constructs a client must
//! fail on the rule, naming AD-12; the control case, which uses reqwest
//! without building a client, passes. Written first.
#![forbid(unsafe_code)]

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};

use common::{repo_root, stderr, stdout};

/// The shared build output (so reqwest's graph is checked once): the
/// `CARGO_TARGET_DIR` the tests run with, read against the repo root when
/// it is relative (CI sets `target`), else the repo's `target/`.
fn target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                dir
            } else {
                repo_root().join(dir)
            }
        }
        None => repo_root().join("target"),
    }
}

fn copy(from: &str, to: &std::path::Path) {
    let bytes = std::fs::read(repo_root().join(from)).unwrap_or_else(|e| panic!("{from}: {e}"));
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, bytes).unwrap();
}

/// Clippy, offline and with `-D warnings`, on the probe crate with `case`.
fn clippy_on(case: &str) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // One package name per case: cargo fingerprints a workspace's own
    // package by its path relative to the workspace root, so two cases
    // under one name in the shared target folder would pass as each other.
    let manifest = std::fs::read_to_string(repo_root().join("fixtures/egress-probe/Cargo.toml"))
        .unwrap()
        .replace(
            "name = \"egress-probe\"",
            &format!("name = \"egress-probe-{}\"", case.replace('_', "-")),
        );
    assert!(manifest.contains(&format!("egress-probe-{}", case.replace('_', "-"))));
    std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    copy(
        &format!("fixtures/egress-probe/cases/{case}.rs"),
        &dir.join("src/lib.rs"),
    );
    copy("clippy.toml", &dir.join("clippy.toml"));
    copy("Cargo.lock", &dir.join("Cargo.lock"));
    copy("rust-toolchain.toml", &dir.join("rust-toolchain.toml"));
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "clippy",
            "--offline",
            "--color",
            "never",
            "--",
            "-D",
            "warnings",
        ])
        .env("CARGO_TARGET_DIR", target_dir())
        .current_dir(dir)
        .output()
        .expect("cargo clippy runs");
    drop(tmp);
    out
}

#[test]
fn the_control_case_passes_clippy() {
    let out = clippy_on("no_client");
    assert!(
        out.status.success(),
        "the probe crate itself is clean: {}{}",
        stdout(&out),
        stderr(&out)
    );
}

#[test]
fn clippy_fails_on_each_way_to_build_a_reqwest_client() {
    for (case, call, lint) in [
        ("client_new", "Client::new", "disallowed method"),
        ("client_default", "Client::default", "disallowed type"),
        (
            "builder_default",
            "ClientBuilder::default",
            "disallowed type",
        ),
        ("names_type", "reqwest::Client", "disallowed type"),
        // Story 2.2 review: the call alone, no type in any signature.
        ("default_in_body", "Client::default", "disallowed type"),
    ] {
        let out = clippy_on(case);
        let text = format!("{}{}", stdout(&out), stderr(&out));
        assert!(!out.status.success(), "{case}: clippy must fail: {text}");
        assert!(
            text.contains(lint),
            "{case}: fails on the {lint} rule: {text}"
        );
        assert!(text.contains(call), "{case}: names {call}: {text}");
        assert!(
            text.contains("use am_net::EgressClient (AD-12)"),
            "{case}: gives the rule's reason: {text}"
        );
    }
}
