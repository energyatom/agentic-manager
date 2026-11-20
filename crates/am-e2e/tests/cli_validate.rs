//! Story 1.4: `agentic validate <project>` as the owner runs it. The command
//! is `am_app::cli::validate`, the same function the `agentic` binary calls;
//! it returns the process exit code (0 no errors, 1 errors, 2 usage or a
//! missing folder) and writes the report to the given streams.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/projects")
        .join(name)
}

/// Runs `agentic validate <args…>` and returns (exit code, stdout, stderr).
fn validate(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = am_app::cli::validate(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn valid_fixture_exits_zero_with_no_errors() {
    let path = fixture("valid");
    let (code, out, err) = validate(&[path.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains(": warning:"), "{out}");
    assert!(out.contains("0 errors"), "{out}");
}

#[test]
fn broken_fixture_prints_every_problem_with_a_fix_and_exits_one() {
    let path = fixture("broken");
    let (code, out, _) = validate(&[path.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let expect = [
        "workflows/ghost.yaml:6: unknown seat 'ghost-seat'",
        "workflows/nightly.yaml:6: invalid cron",
        "models/accounts.yaml:6: secret-looking value",
        "tables/bare.yaml:1: missing schema_version",
        "seats/teleporter.md:7: unknown tool 'teleport'",
        "dashboards/overview.yaml:4: warning: comments are not kept when the app saves this file",
    ];
    for want in expect {
        let i = lines
            .iter()
            .position(|l| l.starts_with(want))
            .unwrap_or_else(|| panic!("missing `{want}` in:\n{out}"));
        let fix = lines.get(i + 1).copied().unwrap_or_default();
        assert!(
            fix.trim_start().starts_with("fix: "),
            "no fix after `{want}`:\n{out}"
        );
    }
    // Findings come sorted by file, then line.
    let keys: Vec<(String, u32)> = lines
        .iter()
        .filter(|l| !l.starts_with(' '))
        .filter_map(|l| {
            let mut parts = l.splitn(3, ':');
            let file = parts.next()?.to_string();
            let line = parts.next()?.parse().ok()?;
            Some((file, line))
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "{out}");
    assert!(out.contains("1 warning"), "{out}");
}

#[test]
fn warnings_alone_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("valid"), dir.path());
    let company = std::fs::read_to_string(dir.path().join("company.yaml")).unwrap();
    std::fs::write(
        dir.path().join("company.yaml"),
        format!("# note\n{company}"),
    )
    .unwrap();
    let (code, out, _) = validate(&[dir.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("company.yaml:1: warning: comments are not kept"),
        "{out}"
    );
}

#[test]
fn a_missing_folder_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope");
    let (code, out, err) = validate(&[missing.to_str().unwrap()]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn bad_usage_exits_two() {
    let (code, _, err) = validate(&["one", "two"]);
    assert_eq!(code, 2);
    assert!(err.contains("usage"), "{err}");
    let (code, _, _) = validate(&["--frobnicate"]);
    assert_eq!(code, 2);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}
