//! Story 1.12 acceptance tests: the seat anatomy (capabilities and their
//! expansion into grants, linked folders, identity, knowledge and links), one
//! test per row of the plan's I/O matrix plus variants built in a temporary
//! copy of a fixture.
#![forbid(unsafe_code)]

mod common;

use am_config::Severity;
use am_domain::capability::{AccountKind, Capability};
use am_domain::runtime::LEGACY_DIR;
use am_domain::workspace::Access;
use common::*;

const FOLDER_TOOLS: &[&str] = &["list_files", "read_file", "search_knowledge"];

/// keeper.md laid out as the fixture holds it, with its one folder's `path`
/// (line 8) and `access` (line 9) given. Story 1.14: the fixture's `work`
/// stays linked, read-only, because `read_write` is refused inside any
/// project (AD-30 as amended 2026-11-22).
fn keeper_text(path: &str, access: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Keeper\nrole: Writer\nteam: core\nfolders:\n  - id: work\n    path: \"{path}\"\n    access: {access}\n    include:\n      - \"**/*.txt\"\n    exclude:\n      - \"old/**\"\ntext_fallback: true\n---\nKeep the notes in the work folder up to date.\n"
    )
}

fn set_keeper(root: &std::path::Path, path: &str, access: &str) {
    write(root, "seats/keeper.md", &keeper_text(path, access));
}

/// Load options with `home` as the owner's home folder (story 1.14).
fn with_home(home: &std::path::Path) -> am_config::LoadOptions {
    am_config::LoadOptions {
        home: Some(home.to_path_buf()),
        ..am_config::LoadOptions::default()
    }
}

// Acceptance: the valid fixture's grants are the table expansion.

#[test]
fn valid_fixture_grants_are_the_table_expansion() {
    let snap = snapshot(&fixture("valid"));
    let alpha = &snap.seats()["alpha"];
    // web_search + tables + folder_access (default) + the attached skill,
    // plus the MCP tool listed in tools:.
    assert_eq!(
        alpha.grants,
        set(&[
            "web_search",
            "table_read",
            "table_write",
            "list_files",
            "read_file",
            "search_knowledge",
            "read_skill_file",
            "notes/list_notes",
        ])
    );
    // A built-in tool of no capability, listed in tools:, is granted as is.
    let beta = &snap.seats()["beta"];
    assert_eq!(
        beta.grants,
        set(&[
            "table_read",
            "table_write",
            "list_files",
            "read_file",
            "search_knowledge",
            "write_document",
        ])
    );
    assert!(snap.seats()["alpha"].capabilities[&Capability::WebSearch].on);
    assert_eq!(
        snap.seats()["alpha"].capabilities[&Capability::WebSearch].provider,
        "builtin"
    );
}

// Row: default capabilities.

#[test]
fn a_seat_with_no_capabilities_gets_folder_access_only() {
    let snap = snapshot(&fixture("anatomy"));
    let plain = &snap.seats()["plain"];
    assert_eq!(plain.grants, set(FOLDER_TOOLS));
    assert_eq!(plain.capabilities.len(), 9, "every capability is filled in");
    for (cap, setting) in &plain.capabilities {
        assert_eq!(setting.on, *cap == Capability::FolderAccess, "{cap:?}");
        assert_eq!(setting.provider, "builtin");
    }
    assert!(!plain.text_fallback, "text_fallback is off by default");
}

// Row: read_write folder.

#[test]
fn the_fixture_links_its_project_subfolder_read_only_with_its_patterns() {
    let snap = snapshot(&fixture("anatomy"));
    let keeper = &snap.seats()["keeper"];
    assert_eq!(keeper.grants, set(FOLDER_TOOLS));
    assert_eq!(keeper.folders.len(), 1);
    let work = &keeper.folders[0];
    assert_eq!(work.id, "work");
    assert_eq!(work.path, "work");
    assert_eq!(work.access, Access::Read);
    assert_eq!(work.include, ["**/*.txt"]);
    assert_eq!(work.exclude, ["old/**"]);
    assert!(keeper.text_fallback);
}

#[test]
fn a_read_write_folder_outside_the_project_adds_write_file() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("notes")).unwrap();
    let copy = copy_of("anatomy");
    set_keeper(copy.path(), "~/notes", "read_write");
    let loaded = am_config::load_in(copy.path(), with_home(home.path())).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.expect("no errors gives a snapshot");
    let keeper = &snap.seats()["keeper"];
    let mut want = set(FOLDER_TOOLS);
    want.insert("write_file".into());
    assert_eq!(keeper.grants, want);
    assert_eq!(keeper.folders[0].access, Access::ReadWrite);
    assert_eq!(keeper.folders[0].path, "~/notes");
}

#[test]
fn a_read_write_folder_inside_the_project_is_refused_at_its_line() {
    let copy = copy_of("anatomy");
    set_keeper(copy.path(), "work", "read_write");
    let found = findings(copy.path());
    let f = at(&found, "seats/keeper.md", 8, "not allowed");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("access: read"), "{}", f.fix);
}

#[test]
fn a_folder_is_read_only_by_default() {
    let copy = copy_of("anatomy");
    let text = keeper_text("work", "read").replace("    access: read\n", "");
    write(copy.path(), "seats/keeper.md", &text);
    let snap = snapshot(copy.path());
    let keeper = &snap.seats()["keeper"];
    assert_eq!(keeper.folders[0].access, Access::Read);
    assert_eq!(keeper.grants, set(FOLDER_TOOLS));
}

// Row: refused folder.

#[test]
fn refused_folders_are_errors_naming_the_allowed_forms() {
    let found = findings(&fixture("broken-anatomy"));
    let legacy = format!("{LEGACY_DIR}/x");
    for (line, path) in [(8, "~/Library/x"), (10, legacy.as_str())] {
        let f = at(&found, "seats/refused.md", line, path);
        assert_eq!(f.severity, Severity::Error);
        assert!(f.message.contains("not allowed"), "{}", f.message);
        assert!(!f.fix.is_empty(), "{f:#?}");
    }
    // Story 1.14: the fix for a ~/Library folder names the two cloud roots
    // that may be linked (AD-30 as amended 2026-11-20).
    let f = at(&found, "seats/refused.md", 8, "~/Library/x");
    assert!(f.fix.contains("~/Library/CloudStorage"), "{}", f.fix);
    assert!(f.fix.contains("com~apple~CloudDocs"), "{}", f.fix);
    assert_eq!(in_file(&found, "seats/refused.md").len(), 2, "{found:#?}");
}

#[test]
fn every_refused_folder_form_is_an_error() {
    for path in [
        "/",
        "/Users/someone/notes",
        "~",
        "~/",
        "~/Library",
        "~/Library/Application Support/agentic-manager",
        "~/notes/../Library/x",
        &format!("notes/{LEGACY_DIR}/x"),
        LEGACY_DIR,
        ".",
        "notes/..",
        "../../../../../../../../../../../../..",
        "~/../../../../../../../..",
        // Story 1.14: ~/Library except the two cloud roots, and project
        // subfolders whose path bears a config name.
        "~/Library/CloudStorage",
        "~/Library/Mobile Documents",
        "~/Library/Mobile Documents/com~apple~Pages/x",
        "~/Library/Application Support/agentic-manager-projects",
        "seats",
        "Seats",
        "runtime-seed",
        "skills/brief",
    ] {
        let copy = copy_of("anatomy");
        set_keeper(copy.path(), path, "read");
        let found = findings(copy.path());
        let refused: Vec<_> = errors(&found)
            .into_iter()
            .filter(|f| f.file == "seats/keeper.md" && f.message.contains("not allowed"))
            .collect();
        assert_eq!(refused.len(), 1, "{path}: {found:#?}");
        assert_eq!(refused[0].line, 8, "{path}");
    }
}

#[test]
fn a_path_that_is_not_a_directory_is_an_error() {
    let copy = copy_of("anatomy");
    set_keeper(copy.path(), "work/notes.txt", "read");
    let found = findings(copy.path());
    let f = at(&found, "seats/keeper.md", 8, "not a folder");
    assert_eq!(f.severity, Severity::Error);
}

#[test]
fn folder_ids_are_kebab_case_and_unique_and_globs_compile() {
    let copy = copy_of("anatomy");
    set_keeper(copy.path(), "work", "read");
    edit(
        copy.path(),
        "seats/keeper.md",
        "text_fallback: true\n",
        "  - id: work\n    path: work\n  - id: Bad_Id\n    path: work\n    include:\n      - \"[x\"\ntext_fallback: true\n",
    );
    let found = findings(copy.path());
    at(&found, "seats/keeper.md", 14, "appears twice");
    at(&found, "seats/keeper.md", 16, "not a kebab-case id");
    at(&found, "seats/keeper.md", 19, "does not compile");
}

// Row: missing folder.

#[test]
fn a_missing_folder_is_a_warning() {
    let loaded = am_config::load(&fixture("anatomy")).unwrap();
    let f = at(&loaded.findings, "seats/scout.md", 8, "missing-dir");
    assert_eq!(f.severity, Severity::Warning);
    assert!(loaded.snapshot.is_some(), "a warning keeps the snapshot");
}

// Row: legacy key.

#[test]
fn a_legacy_documents_key_is_read_as_folder_access() {
    let snap = snapshot(&fixture("anatomy"));
    let legacy = &snap.seats()["legacy"];
    assert!(legacy.capabilities[&Capability::FolderAccess].on);
    assert_eq!(legacy.grants, set(FOLDER_TOOLS));
    // Switched off through the old key, the folder tools go.
    let copy = copy_of("anatomy");
    edit(copy.path(), "seats/legacy.md", "on: true", "on: false");
    let snap = snapshot(copy.path());
    let legacy = &snap.seats()["legacy"];
    assert!(!legacy.capabilities[&Capability::FolderAccess].on);
    assert!(legacy.grants.is_empty(), "{:?}", legacy.grants);
}

#[test]
fn documents_and_folder_access_together_are_an_error() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/legacy.md",
        "    on: true\n",
        "    on: true\n  folder_access:\n    on: true\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "seats/legacy.md", 7, "documents");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("folder_access"), "{}", f.fix);
}

// Row: tool while its capability is off.

#[test]
fn a_tool_whose_capability_is_off_is_an_error_at_its_line() {
    let found = findings(&fixture("broken-anatomy"));
    let f = at(&found, "seats/reader.md", 7, "read_url");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("switch on browse"), "{}", f.fix);
}

#[test]
fn write_file_needs_a_read_write_folder() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\ntools:\n  - write_file\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "seats/plain.md", 7, "write_file");
    assert!(f.fix.contains("read_write"), "{}", f.fix);
}

#[test]
fn unknown_capabilities_and_missing_on_are_errors() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\ncapabilities:\n  web_serch:\n    on: true\n  browse:\n    provider: builtin\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "seats/plain.md", 7, "web_serch");
    assert!(f.fix.contains("web_search"), "{}", f.fix);
    at(&found, "seats/plain.md", 9, "'on'");
}

// Row: wrong-kind provider.

#[test]
fn a_provider_account_of_the_wrong_kind_is_an_error() {
    let found = findings(&fixture("broken-anatomy"));
    let f = at(&found, "seats/searcher.md", 9, "crawly");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.message.contains("crawl-service"), "{}", f.message);
    assert!(f.fix.contains("search"), "{}", f.fix);
}

#[test]
fn built_in_only_capabilities_refuse_an_account() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\ncapabilities:\n  tables:\n    on: true\n    provider: finder\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "seats/plain.md", 9, "finder");
    assert!(f.fix.contains("builtin"), "{}", f.fix);
}

#[test]
fn account_kinds_follow_the_provider() {
    let snap = snapshot(&fixture("anatomy"));
    let kinds: Vec<(&str, AccountKind)> = snap
        .accounts()
        .iter()
        .map(|a| (a.id.as_str(), a.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("finder", AccountKind::Search),
            ("crawly", AccountKind::CrawlService)
        ]
    );
    let valid = snapshot(&fixture("valid"));
    assert_eq!(valid.accounts()[0].kind, AccountKind::Model);
}

#[test]
fn firecrawl_and_searxng_remote_need_a_base_url() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "models/accounts.yaml",
        "    base_url: https://crawl.example.org\n",
        "",
    );
    edit(
        copy.path(),
        "models/accounts.yaml",
        "provider: tavily",
        "provider: searxng-remote",
    );
    let found = findings(copy.path());
    at(&found, "models/accounts.yaml", 3, "base_url");
    at(&found, "models/accounts.yaml", 7, "base_url");
}

// Row: full seat.

#[test]
fn a_full_seat_keeps_every_field_in_the_snapshot() {
    let loaded = am_config::load(&fixture("anatomy")).unwrap();
    assert!(
        in_file(&loaded.findings, "seats/full.md").is_empty(),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.unwrap();
    let full = &snap.seats()["full"];
    assert_eq!(
        full.goal.as_deref(),
        Some("Keep the team's reading list current and short.")
    );
    assert!(full.backstory.as_deref().unwrap().starts_with("Has read"));
    assert_eq!(full.avatar.as_ref().unwrap().tint, "sage");
    assert_eq!(full.skills, ["brief"]);
    assert_eq!(full.knowledge.len(), 2);
    assert_eq!(full.knowledge[0].file, "guide.md");
    assert_eq!(
        full.knowledge[0].note.as_deref(),
        Some("How the reading list is laid out.")
    );
    assert_eq!(full.knowledge[1].file, "terms.txt");
    assert_eq!(full.knowledge[1].note, None);
    assert_eq!(full.links.len(), 3);
    assert_eq!(full.links[1].title, "Reference");
    assert_eq!(full.links[1].url, "https://docs.example.org/reference");
    assert_eq!(full.links[1].group.as_deref(), Some("Sites"));
    assert_eq!(full.links[2].group, None);
    assert_eq!(full.capabilities[&Capability::WebSearch].provider, "finder");
    assert_eq!(full.capabilities[&Capability::Browse].provider, "crawly");
    assert_eq!(
        full.grants,
        set(&[
            "web_search",
            "read_website",
            "read_url",
            "arxiv",
            "list_files",
            "read_file",
            "search_knowledge",
            "read_skill_file",
        ])
    );
}

#[test]
fn goal_and_avatar_are_checked() {
    let copy = copy_of("anatomy");
    let goal = "g".repeat(501);
    edit(
        copy.path(),
        "seats/full.md",
        "goal: Keep the team's reading list current and short.",
        &format!("goal: {goal}"),
    );
    edit(copy.path(), "seats/full.md", "tint: sage", "tint: teal");
    let found = findings(copy.path());
    let f = at(&found, "seats/full.md", 6, "goal");
    assert!(f.message.contains("501"), "{}", f.message);
    let f = at(&found, "seats/full.md", 9, "teal");
    assert!(f.fix.contains("slate"), "{}", f.fix);
    // Characters, not bytes: 500 two-byte characters fit.
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/full.md",
        "goal: Keep the team's reading list current and short.",
        &format!("goal: {}", "é".repeat(500)),
    );
    assert!(errors(&findings(copy.path())).is_empty());
}

// Row: broken skills and seat.

#[test]
fn broken_skills_and_seat_give_one_error_each() {
    let found = findings(&fixture("broken-anatomy"));
    for (file, line, message) in [
        ("seats/messy.md", 6, "backstory"),
        ("seats/messy.md", 8, "unknown skill 'ghost'"),
        ("seats/messy.md", 11, "ftp://example.org/files"),
        ("skills/nodesc/SKILL.md", 2, "missing 'description'"),
        ("skills/scripted/scripts", 1, "scripts/"),
        ("skills/scripted/tool.py", 1, "tool.py"),
        ("skills/bulky/SKILL.md", 5, "64 KB"),
    ] {
        let f = at(&found, file, line, message);
        assert_eq!(f.severity, Severity::Error, "{file}:{line}");
        assert!(!f.fix.is_empty(), "{file}:{line}");
    }
    assert_eq!(in_file(&found, "seats/messy.md").len(), 3, "{found:#?}");
    assert_eq!(in_file(&found, "skills/nodesc/SKILL.md").len(), 1);
    assert_eq!(in_file(&found, "skills/bulky/SKILL.md").len(), 1);
    let scripted: Vec<_> = found
        .iter()
        .filter(|f| f.file.starts_with("skills/scripted/"))
        .collect();
    assert_eq!(scripted.len(), 2, "{scripted:#?}");
    // Every other seat file in the set holds its own one problem only.
    assert_eq!(errors(&found).len(), 11, "{found:#?}");
}

// Row: soft problems.

#[test]
fn an_unlisted_link_host_and_a_missing_reference_file_are_warnings() {
    // Story 1.13: reference files are checked against the template's
    // runtime-seed/ (validate --template), not a folder in the project.
    let root = fixture("anatomy");
    let loaded = am_config::load_in(&root, am_config::LoadOptions::template(&root)).unwrap();
    let soft = in_file(&loaded.findings, "seats/soft.md");
    assert_eq!(soft.len(), 2, "{soft:#?}");
    let f = at(&loaded.findings, "seats/soft.md", 7, "absent.md");
    assert_eq!(f.severity, Severity::Warning);
    let f = at(
        &loaded.findings,
        "seats/soft.md",
        10,
        "elsewhere.example.net",
    );
    assert_eq!(f.severity, Severity::Warning);
    assert!(loaded.snapshot.is_some());
}

#[test]
fn knowledge_files_are_relative_without_parent_steps() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/soft.md",
        "file: absent.md",
        "file: ../guide.md",
    );
    let found = findings(copy.path());
    let f = at(&found, "seats/soft.md", 7, "../guide.md");
    assert_eq!(f.severity, Severity::Error);
}

/// Story 1.14 (AD-48): the allow-list is the seat's own `allowed_hosts`.
/// The fixture's former `tools.yaml` `egress_hosts` moved to `full`, its
/// only seat that can fetch; every other seat has none.
#[test]
fn the_seat_allow_list_is_the_seats_own_allowed_hosts() {
    let snap = snapshot(&fixture("anatomy"));
    let hosts = |seat: &str| -> std::collections::BTreeSet<String> {
        snap.allowed_hosts(seat)
            .iter()
            .map(|h| h.to_string())
            .collect()
    };
    assert_eq!(hosts("full"), set(&["example.org", "docs.example.org"]));
    for seat in ["plain", "keeper", "legacy", "scout", "soft"] {
        assert!(hosts(seat).is_empty(), "{seat}: {:?}", hosts(seat));
    }
}

#[test]
fn the_anatomy_fixture_has_only_its_three_warnings() {
    // As `agentic validate --template` reads it (story 1.13).
    let root = fixture("anatomy");
    let loaded = am_config::load_in(&root, am_config::LoadOptions::template(&root)).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    assert_eq!(
        warnings(&loaded.findings).len(),
        3,
        "{:#?}",
        loaded.findings
    );
}

// Review fixes: lexical resolution through `..`, the project and its
// ancestors, and `.agentic` in any case.

/// The `not allowed` errors on keeper.md's folder path after writing `path`
/// (linked read-only).
fn refused_with(root: &std::path::Path, path: &str) -> Vec<am_config::Finding> {
    set_keeper(root, path, "read");
    findings(root)
        .into_iter()
        .filter(|f| {
            f.file == "seats/keeper.md"
                && f.line == 8
                && f.severity == Severity::Error
                && f.message.contains("not allowed")
        })
        .collect()
}

#[test]
fn a_project_loaded_through_dot_dot_is_judged_where_it_really_is() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("a/proj");
    copy_dir(&fixture("anatomy"), &real);
    std::fs::create_dir_all(dir.path().join("a/sub")).unwrap();
    let via = dir.path().join("a/sub/../proj");
    // `../../a` lands on <tmp>/a, which holds the project.
    let refused = refused_with(&via, "../../a");
    assert_eq!(refused.len(), 1, "{refused:#?}");
    assert!(refused[0].message.contains("holds the project folder"));
}

#[test]
fn the_project_folder_and_its_ancestors_are_refused_however_written() {
    // Each spelling gets a fresh copy, whose folder name the path names.
    for spelling in ["..", "../{name}", "../{name}/work/.."] {
        let copy = copy_of("anatomy");
        let name = copy
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let path = spelling.replace("{name}", &name);
        let refused = refused_with(copy.path(), &path);
        assert_eq!(refused.len(), 1, "{path}: {refused:#?}");
        // The project no longer holds a runtime folder (AD-32); the words
        // name what it does hold.
        assert!(
            refused[0].message.contains("holds its configuration"),
            "{path}: {}",
            refused[0].message
        );
    }
}

#[test]
fn a_dot_agentic_part_is_refused_in_any_case() {
    for path in [".Agentic/x", "notes/.AGENTIC"] {
        let copy = copy_of("anatomy");
        let refused = refused_with(copy.path(), path);
        assert_eq!(refused.len(), 1, "{path}: {refused:#?}");
        assert!(refused[0].message.contains(LEGACY_DIR), "{path}");
    }
}

#[test]
fn read_skill_file_without_a_skill_is_an_error() {
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\ntools:\n  - read_skill_file\n",
    );
    let found = findings(copy.path());
    let f = at(
        &found,
        "seats/plain.md",
        7,
        "read_skill_file comes with an attached skill",
    );
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("attach a skill"), "{}", f.fix);
    // With a skill attached it is allowed (and granted anyway).
    let copy = copy_of("anatomy");
    edit(
        copy.path(),
        "seats/plain.md",
        "team: core\n",
        "team: core\nskills:\n  - brief\ntools:\n  - read_skill_file\n",
    );
    assert!(
        snapshot(copy.path()).seats()["plain"]
            .grants
            .contains("read_skill_file")
    );
}
