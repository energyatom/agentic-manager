//! Story 1.14 acceptance tests (AD-30 as amended 2026-11-22): the config
//! names are one pure `am-domain` policy, `CONFIG_NAMES` and
//! `is_config_bearing`, compared ignoring case. am-config's root policy and
//! the folder tools call it. (That the names equal what the loader reads is
//! asserted in `crates/am-config/tests/folder_roots.rs`.) Written before the
//! policy exists.
//!
//! The API these tests expect: `am_domain::CONFIG_NAMES: &[&str]` and
//! `am_domain::is_config_bearing(rel: &str) -> bool`, where `rel` is a
//! project-relative path written with `/`.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use am_domain::runtime::{LEGACY_DIR, OPEN_MARKER};
use am_domain::{CONFIG_NAMES, is_config_bearing};

// The runtime names are spelled through am_domain's constants: the repo's
// runtime-paths check refuses `.agentic` literals under crates/ (AD-32).

#[test]
fn the_config_names_are_the_loader_folders_files_and_runtime_names() {
    let names: BTreeSet<&str> = CONFIG_NAMES.iter().copied().collect();
    let want: BTreeSet<&str> = [
        "models",
        "seats",
        "skills",
        "workflows",
        "tables",
        "schemas",
        "dashboards",
        "sources",
        "company.yaml",
        "tools.yaml",
        LEGACY_DIR,
        "runtime-seed",
        OPEN_MARKER,
        ".git",
    ]
    .into_iter()
    .collect();
    assert_eq!(names, want);
    assert_eq!(CONFIG_NAMES.len(), want.len(), "no name twice");
}

#[test]
fn a_path_with_any_config_name_component_is_config_bearing() {
    let legacy_inside = format!("files/{LEGACY_DIR}/x");
    for rel in [
        "seats",
        "seats/alpha.md",
        "files/seats",
        "files/in/seats/x.md",
        "company.yaml",
        "files/company.yaml",
        "tools.yaml",
        "models",
        "skills/brief",
        "workflows",
        "tables",
        "schemas",
        "dashboards",
        "sources",
        LEGACY_DIR,
        legacy_inside.as_str(),
        "runtime-seed",
        "runtime-seed/knowledge/x",
        OPEN_MARKER,
        ".git",
        "files/.git/HEAD",
    ] {
        assert!(is_config_bearing(rel), "{rel}");
    }
}

#[test]
fn config_names_are_compared_ignoring_case() {
    for rel in [
        "SEATS",
        "files/Seats",
        "Company.YAML",
        "files/TOOLS.yaml",
        ".Agentic",
        "Runtime-Seed",
        ".GIT",
        "files/.Agentic-Open",
        "Models/x",
    ] {
        assert!(is_config_bearing(rel), "{rel}");
    }
}

#[test]
fn other_paths_are_not_config_bearing() {
    let legacy_like = format!("files/{LEGACY_DIR}-x");
    for rel in [
        "files",
        "files/in",
        "files/x1-inputs",
        "notes/seat.md",
        "files/seatsx",
        "files/my-seats",
        "files/company.yaml.bak",
        "files/company.yml",
        "files/tools.yaml.old",
        legacy_like.as_str(),
        "files/runtime-seeds",
        "files/.github",
        "work",
        "",
    ] {
        assert!(!is_config_bearing(rel), "{rel:?}");
    }
}
