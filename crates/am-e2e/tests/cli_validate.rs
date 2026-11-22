//! Story 1.4: `agentic validate <project>` as the owner runs it. The command
//! is `am_app::cli::validate`, the same function the `agentic` binary calls;
//! it returns the process exit code (0 no errors, 1 errors, 2 usage or a
//! missing folder) and writes the report to the given streams. Story 1.13:
//! `--template` (what `just validate` runs over templates/ and fixtures/)
//! checks reference files against the folder's `runtime-seed/` and refuses a
//! `project_id`; on a project, validate never writes an id.
//!
//! Story 2.12 (AD-17): on a project, `agentic validate` reads this Mac's user
//! Accounts list from the app data dir under HOME, and tests never read the
//! real one. So the fixture rows here run in `--template` mode (Account
//! references checked for form only), the fixtures' Account findings moved
//! with their Accounts to `fixtures/accounts/` (crates/am-config/tests/
//! user_accounts.rs), and the rows that need a user list run the real
//! binary with HOME in a temporary folder (crates/am-app/tests/
//! user_accounts.rs).
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
    let (code, out, err) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains(": warning:"), "{out}");
    assert!(out.contains("0 errors"), "{out}");
}

#[test]
fn broken_fixture_prints_every_problem_with_a_fix_and_exits_one() {
    let path = fixture("broken");
    let (code, out, _) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let expect = [
        "workflows/ghost.yaml:6: unknown seat 'ghost-seat'",
        "workflows/nightly.yaml:6: invalid cron",
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

/// The line after `want` (which must start a line of `out`) is its fix.
fn has_finding_with_fix(out: &str, want: &str) {
    let lines: Vec<&str> = out.lines().collect();
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

// Story 1.12: seat anatomy, skills and Sources.

#[test]
fn broken_anatomy_prints_every_error_with_a_fix_and_exits_one() {
    let path = fixture("broken-anatomy");
    let (code, out, _) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    for want in [
        "seats/refused.md:8: folder path '~/Library/x' is not allowed",
        "seats/refused.md:10: folder path '.agentic/x' is not allowed",
        "seats/reader.md:7: tool 'read_url' needs the browse capability",
        "seats/messy.md:6: backstory",
        "seats/messy.md:8: unknown skill 'ghost'",
        "seats/messy.md:11: link 'ftp://example.org/files'",
        "skills/bulky/SKILL.md:5: ",
        "skills/nodesc/SKILL.md:2: SKILL.md is missing 'description'",
        "skills/scripted/scripts:1: ",
        "skills/scripted/tool.py:1: ",
    ] {
        has_finding_with_fix(&out, want);
    }
    // The wrong-kind Account (seats/searcher.md:9) needs the user list:
    // crates/am-app/tests/user_accounts.rs.
    assert!(out.contains("10 errors"), "{out}");
}

#[test]
fn anatomy_fixture_exits_zero_with_its_warnings() {
    let path = fixture("anatomy");
    let (code, out, _) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "seats/scout.md:8: warning: ",
        "seats/soft.md:7: warning: ",
        "seats/soft.md:10: warning: ",
    ] {
        has_finding_with_fix(&out, want);
    }
    assert!(out.contains("0 errors, 3 warnings"), "{out}");
}

#[test]
fn sources_ok_exits_zero_with_two_warnings() {
    let path = fixture("sources-ok");
    let (code, out, _) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    has_finding_with_fix(&out, "sources/site.yaml:5: warning: ");
    has_finding_with_fix(&out, "sources/site.yaml:8: warning: ");
    assert!(out.contains("0 errors, 2 warnings"), "{out}");
}

#[test]
fn sources_broken_prints_each_error_and_exits_one() {
    let path = fixture("sources-broken");
    let (code, out, _) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    for want in [
        "sources/crawl.yaml:6: ",
        "sources/crawl.yaml:8: ",
        "sources/crawl.yaml:10: unknown table 'ghost'",
        "sources/news.yaml:9: ",
    ] {
        has_finding_with_fix(&out, want);
    }
    assert!(out.contains("4 errors"), "{out}");
}

// Story 1.5: every shipped template validates as it is.

fn templates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates")
}

/// Every template folder: the departments, the roles and the samples.
fn template_folders() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for group in ["departments", "roles", "samples"] {
        let dir = templates().join(group);
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn assert_clean(path: &Path) {
    let (code, out, err) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{}:\n{out}{err}", path.display());
    assert!(
        out.contains("0 errors, 0 warnings"),
        "{}:\n{out}",
        path.display()
    );
}

#[test]
fn blank_department_template_exits_zero() {
    assert_clean(&templates().join("departments/blank"));
}

#[test]
fn each_role_template_exits_zero() {
    let roles = [
        "head",
        "coordinator",
        "analyst",
        "researcher",
        "reviewer",
        "writer",
        "specialist",
        "human",
    ];
    for role in roles {
        assert_clean(&templates().join("roles").join(role));
    }
}

#[test]
fn sample_company_template_exits_zero() {
    // The sample's id is department vocabulary (AD-23); it is read from the
    // expectations file under fixtures/.
    let expect = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/templates/sample.json"),
    )
    .expect("fixtures/templates/sample.json");
    let name = expect
        .split_once("\"sample\": \"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(name, _)| name)
        .expect("the sample's folder name");
    assert_clean(&templates().join("samples").join(name));
}

#[test]
fn every_template_folder_exits_zero() {
    let folders = template_folders();
    assert_eq!(
        folders.len(),
        10,
        "1 department, 8 roles, 1 sample: {folders:?}"
    );
    for folder in folders {
        assert_clean(&folder);
    }
}

// Story 1.13: the project id and the seed reference files.

#[test]
fn template_mode_refuses_a_project_id_with_file_line_and_fix() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&templates().join("departments/blank"), dir.path());
    let company = dir.path().join("company.yaml");
    let text = std::fs::read_to_string(&company).unwrap();
    let id = am_domain::ProjectId::mint();
    // After the schema_version line, whatever version the template is at.
    let version = text
        .lines()
        .find(|l| l.starts_with("schema_version:"))
        .unwrap()
        .to_string();
    std::fs::write(
        &company,
        text.replacen(
            &format!("{version}\n"),
            &format!("{version}\nproject_id: {id}\n"),
            1,
        ),
    )
    .unwrap();
    let (code, out, _) = validate(&["--template", dir.path().to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    has_finding_with_fix(
        &out,
        "company.yaml:2: a template or fixture carries no project_id",
    );
    assert!(out.contains("remove the `project_id:` line"), "{out}");
    // As a project the same folder is fine, and validate writes nothing:
    // story 2.12 moved that half to crates/am-app/tests/user_accounts.rs
    // (project mode reads this Mac's user list under HOME).
}

// Story 2.12: `a_project_without_a_runtime_folder_skips_the_reference_file_
// warning_and_gets_no_id` moved to crates/am-app/tests/user_accounts.rs: as
// a project validate reads this Mac's user list, which needs a HOME in a
// temporary folder.

/// Story 2.12 (AD-17): the one old-format project fails `--template` with
/// an error at its old Accounts file, line 1, with the migrate fix.
#[test]
fn the_legacy_accounts_fixture_fails_with_the_migrate_fix() {
    let path = fixture("legacy-accounts");
    let (code, out, _) = validate(&["--template", path.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    has_finding_with_fix(&out, "models/accounts.yaml:1: ");
    assert!(out.contains("agentic accounts migrate --project"), "{out}");
    assert!(out.contains("1 error"), "{out}");
}

#[test]
fn no_template_or_fixture_ships_a_legacy_runtime_folder() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                if e.file_name() == am_domain::runtime::LEGACY_DIR {
                    found.push(e.path());
                }
                walk(&e.path(), found);
            }
        }
    }
    let mut found = Vec::new();
    walk(&root.join("templates"), &mut found);
    walk(&root.join("fixtures"), &mut found);
    assert!(found.is_empty(), "{found:?}");
    assert!(
        fixture("anatomy")
            .join(am_domain::runtime::RUNTIME_SEED)
            .join("knowledge/full/guide.md")
            .is_file()
    );
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
    let (code, out, _) = validate(&["--template", dir.path().to_str().unwrap()]);
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

/// A stream copy (story 1.13: never `std::fs::copy`).
fn copy_dir(from: &Path, to: &Path) {
    am_testkit::copy_tree(from, to);
}
