//! Story 2.2: the gate runs the new scans. `just check` (which CI's macOS
//! job runs) depends on recipes that run `cargo xtask fixed-hosts` (AD-12,
//! no telemetry) and `cargo xtask seam-guard` (decision 7); the clippy
//! probe (`egress_clippy.rs`) is an ordinary test, so `just check`'s
//! nextest runs it. Written first.
#![forbid(unsafe_code)]

mod common;

use common::repo_root;

/// The body lines of `recipe` in the justfile (until the next unindented line).
fn recipe<'j>(justfile: &'j str, name: &str) -> Option<Vec<&'j str>> {
    let mut lines = justfile.lines();
    lines.find(|l| l.starts_with(&format!("{name}:")))?;
    Some(
        lines
            .take_while(|l| l.starts_with(' ') || l.starts_with('\t'))
            .map(str::trim)
            .collect(),
    )
}

#[test]
fn just_check_runs_the_fixed_host_scan_and_the_seam_guard() {
    let justfile = std::fs::read_to_string(repo_root().join("justfile")).unwrap();
    let check = justfile
        .lines()
        .find(|l| l.starts_with("check:"))
        .expect("a check recipe");
    let deps: Vec<&str> = check["check:".len()..].split_whitespace().collect();
    for (step, command) in [
        ("fixed-hosts", "cargo xtask fixed-hosts"),
        ("seam-guard", "cargo xtask seam-guard"),
    ] {
        assert!(deps.contains(&step), "`just check` runs {step}: {check}");
        let body = recipe(&justfile, step).unwrap_or_else(|| panic!("a {step} recipe"));
        assert!(
            body.contains(&command),
            "the {step} recipe runs `{command}`: {body:?}"
        );
    }
    let ci = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();
    assert!(ci.contains("run: just check"), "CI runs just check");
}
