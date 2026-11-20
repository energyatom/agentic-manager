//! Story 1.12 acceptance tests: the skill loader (AD-38). A skill folder is
//! instructions only; `allowed-tools` is stored as "asks for" and never
//! reaches a seat's grants.
#![forbid(unsafe_code)]

mod common;

use am_config::Severity;
use common::*;

const SKILL: &str = "---\nname: helper\ndescription: Helps.\n---\nHelp.\n";
const SKILL_ASKING: &str =
    "---\nname: helper\ndescription: Helps.\nallowed-tools: read_website\n---\nHelp.\n";

// Acceptance: skills() holds the skill with asks_for.

#[test]
fn the_valid_fixture_skill_is_in_the_snapshot() {
    let snap = snapshot(&fixture("valid"));
    let brief = &snap.skills()["brief"];
    assert_eq!(brief.id, "brief");
    assert_eq!(brief.name, "brief");
    assert_eq!(
        brief.description,
        "Writes a short brief from the gathered items."
    );
    assert_eq!(brief.license.as_deref(), Some("MIT"));
    assert_eq!(brief.asks_for, ["read_website"]);
    assert!(brief.body.starts_with("Keep each brief"), "{}", brief.body);
    let refs: Vec<&String> = brief.references.keys().collect();
    assert_eq!(refs, ["style.md"]);
    assert!(brief.references["style.md"].contains("One item per line"));
    // asks_for is information: alpha's grants do not gain read_website.
    assert!(!snap.seats()["alpha"].grants.contains("read_website"));
}

#[test]
fn metadata_is_kept_as_text() {
    let snap = snapshot(&fixture("anatomy"));
    let brief = &snap.skills()["brief"];
    assert_eq!(
        brief.metadata.get("version").map(String::as_str),
        Some("1.0")
    );
    assert!(brief.asks_for.is_empty());
}

#[test]
fn skill_files_count_in_the_snapshot_hash() {
    let copy = copy_of("valid");
    let before = snapshot(copy.path()).hash().to_string();
    write(
        copy.path(),
        "skills/brief/references/style.md",
        "# Style\n\nChanged.\n",
    );
    assert_ne!(before, snapshot(copy.path()).hash());
}

// Row: skill asks for a tool.

#[test]
fn allowed_tools_never_change_the_grants() {
    let without = copy_of("anatomy");
    write(without.path(), "skills/helper/SKILL.md", SKILL);
    edit(
        without.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\nskills:\n  - helper\n",
    );
    let with = copy_of("anatomy");
    write(with.path(), "skills/helper/SKILL.md", SKILL_ASKING);
    edit(
        with.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\nskills:\n  - helper\n",
    );
    let a = snapshot(without.path());
    let b = snapshot(with.path());
    let grants_without = &a.seats()["plain"].grants;
    let grants_with = &b.seats()["plain"].grants;
    assert_eq!(grants_without, grants_with);
    assert!(!grants_with.contains("read_website"));
    assert_eq!(b.skills()["helper"].asks_for, ["read_website"]);
    // The only difference from the same seat with no skill: read_skill_file.
    let none = snapshot(&fixture("anatomy"));
    let mut expected = none.seats()["plain"].grants.clone();
    expected.insert("read_skill_file".into());
    assert_eq!(grants_with, &expected);
}

#[test]
fn allowed_tools_may_be_a_list_or_words() {
    let copy = copy_of("anatomy");
    write(
        copy.path(),
        "skills/helper/SKILL.md",
        "---\nname: helper\ndescription: Helps.\nallowed-tools:\n  - read_website\n  - web_search\n---\nHelp.\n",
    );
    write(
        copy.path(),
        "skills/other/SKILL.md",
        "---\nname: other\ndescription: Helps too.\nallowed-tools: read_url web_search\n---\nHelp.\n",
    );
    let snap = snapshot(copy.path());
    assert_eq!(
        snap.skills()["helper"].asks_for,
        ["read_website", "web_search"]
    );
    assert_eq!(snap.skills()["other"].asks_for, ["read_url", "web_search"]);
}

// Row: broken skills (the fixture rows are in anatomy.rs; these are variants).

#[test]
fn the_name_must_equal_the_folder() {
    let copy = copy_of("anatomy");
    write(
        copy.path(),
        "skills/helper/SKILL.md",
        "---\nname: other\ndescription: Helps.\n---\nHelp.\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "skills/helper/SKILL.md", 2, "other");
    assert!(f.fix.contains("helper"), "{}", f.fix);
}

#[test]
fn a_skill_folder_needs_skill_md() {
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/references/notes.md", "Notes.\n");
    let found = findings(copy.path());
    let f = at(&found, "skills/helper/SKILL.md", 1, "missing SKILL.md");
    assert_eq!(f.severity, Severity::Error);
}

#[test]
fn unknown_frontmatter_keys_are_errors() {
    let copy = copy_of("anatomy");
    write(
        copy.path(),
        "skills/helper/SKILL.md",
        "---\nname: helper\ndescripton: Helps.\n---\nHelp.\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "skills/helper/SKILL.md", 3, "descripton");
    assert!(f.fix.contains("description"), "{}", f.fix);
}

#[test]
fn reference_files_have_caps() {
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    write(
        copy.path(),
        "skills/helper/references/big.md",
        &"x".repeat(256 * 1024 + 1),
    );
    write(copy.path(), "skills/helper/references/table.xlsx", "x");
    write(copy.path(), "skills/helper/references/deep/notes.md", "x");
    let found = findings(copy.path());
    at(&found, "skills/helper/references/big.md", 1, "256 KB");
    at(
        &found,
        "skills/helper/references/table.xlsx",
        1,
        "not accepted",
    );
    at(&found, "skills/helper/references/deep", 1, "not accepted");
    // At most 20 reference files.
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    for i in 0..21 {
        write(
            copy.path(),
            &format!("skills/helper/references/r{i}.txt"),
            "x\n",
        );
    }
    let found = findings(copy.path());
    let f = at(&found, "skills/helper/references", 1, "21");
    assert!(f.message.contains("20"), "{}", f.message);
    // Each allowed extension passes.
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    for ext in ["md", "txt", "csv", "json", "yaml", "yml"] {
        write(
            copy.path(),
            &format!("skills/helper/references/r.{ext}"),
            "{}\n",
        );
    }
    assert!(errors(&findings(copy.path())).is_empty());
}

#[cfg(unix)]
#[test]
fn an_executable_file_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    write(copy.path(), "skills/helper/references/run.md", "x\n");
    let path = copy.path().join("skills/helper/references/run.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let found = findings(copy.path());
    let f = at(&found, "skills/helper/references/run.md", 1, "executable");
    assert_eq!(f.severity, Severity::Error);
}

#[test]
fn secrets_and_comments_are_checked_in_skill_files() {
    let copy = copy_of("anatomy");
    write(
        copy.path(),
        "skills/helper/SKILL.md",
        "---\nname: helper\n# note\ndescription: Helps.\n---\nUse sk-ant-canary0canary0canary.\n",
    );
    write(
        copy.path(),
        "skills/helper/references/keys.txt",
        "line one\nhf_canary0canary0canary\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "skills/helper/SKILL.md", 3, "comments are not kept");
    assert_eq!(f.severity, Severity::Warning);
    at(&found, "skills/helper/SKILL.md", 6, "secret-looking value");
    at(
        &found,
        "skills/helper/references/keys.txt",
        2,
        "secret-looking value",
    );
}

#[test]
fn a_file_in_skills_is_not_read() {
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/readme.md", "Notes.\n");
    let found = findings(copy.path());
    let f = at(&found, "skills/readme.md", 1, "not read");
    assert_eq!(f.severity, Severity::Warning);
}

// Review fixes: hidden entries and structured reference files.

#[test]
fn hidden_entries_are_checked_and_only_ds_store_is_skipped() {
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    write(copy.path(), "skills/helper/.DS_Store", "x");
    write(copy.path(), "skills/helper/.scripts/run.sh", "x\n");
    write(copy.path(), "skills/helper/.hidden", "x\n");
    write(copy.path(), "skills/helper/references/.DS_Store", "x");
    write(copy.path(), "skills/helper/references/.secret", "x\n");
    let found = findings(copy.path());
    at(&found, "skills/helper/.scripts", 1, "not accepted");
    at(&found, "skills/helper/.hidden", 1, "not accepted");
    at(
        &found,
        "skills/helper/references/.secret",
        1,
        "not accepted",
    );
    assert!(
        found.iter().all(|f| !f.file.ends_with(".DS_Store")),
        "{found:#?}"
    );
}

#[cfg(unix)]
#[test]
fn a_hidden_executable_file_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    write(copy.path(), "skills/helper/references/.run.md", "x\n");
    let path = copy.path().join("skills/helper/references/.run.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let found = findings(copy.path());
    at(&found, "skills/helper/references/.run.md", 1, "executable");
}

#[test]
fn structured_references_get_the_key_name_and_comment_checks() {
    let copy = copy_of("anatomy");
    write(copy.path(), "skills/helper/SKILL.md", SKILL);
    write(
        copy.path(),
        "skills/helper/references/x.yaml",
        "title: Notes\n# a note\napi_key: abc123canary\n",
    );
    write(
        copy.path(),
        "skills/helper/references/y.json",
        "{\n  \"github_token\": \"abc123canary\"\n}\n",
    );
    write(copy.path(), "skills/helper/references/z.yaml", "a: [1\n");
    let found = findings(copy.path());
    let f = at(
        &found,
        "skills/helper/references/x.yaml",
        3,
        "secret-looking value",
    );
    assert_eq!(f.severity, Severity::Error);
    let f = at(
        &found,
        "skills/helper/references/x.yaml",
        2,
        "comments are not kept",
    );
    assert_eq!(f.severity, Severity::Warning);
    at(
        &found,
        "skills/helper/references/y.json",
        2,
        "secret-looking value",
    );
    assert!(
        in_file(&found, "skills/helper/references/z.yaml").is_empty(),
        "a reference that does not parse gets no new finding: {found:#?}"
    );
}
