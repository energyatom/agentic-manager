//! The release gate runs every file in `tests/trust/` (AD-33) and, from
//! story 3.3, in `tests/engine/` (AGENTS.md: the engine tests of the
//! cross-department primitives). Cargo does not discover files in a
//! subfolder of `tests/`, so each one needs its own `[[test]]` target in
//! am-e2e's Cargo.toml, named after the file. These tests fail when a trust
//! or engine file has no such target, so none is silently skipped.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::Path;

/// (name, path) of every `[[test]]` target in the manifest text.
fn targets(manifest: &str) -> BTreeSet<(String, String)> {
    let value = |block: &str, key: &str| -> Option<String> {
        block.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
    };
    manifest
        .split("[[test]]")
        .skip(1)
        .map(|block| block.split("\n[").next().unwrap_or(block))
        .filter_map(|block| Some((value(block, "name")?, value(block, "path")?)))
        .collect()
}

/// The `.rs` files directly in `tests/<folder>/` that have no `[[test]]`
/// target named after them with that path (subfolders, such as a shared
/// `support/`, are not targets).
fn missing_targets(folder: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let listed = targets(&manifest);
    let mut files: Vec<String> = std::fs::read_dir(root.join("tests").join(folder))
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".rs"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "tests/{folder}/ has no test files");
    files
        .iter()
        .filter(|f| {
            let name = f.trim_end_matches(".rs").to_string();
            !listed.contains(&(name, format!("tests/{folder}/{f}")))
        })
        .cloned()
        .collect()
}

#[test]
fn trust_targets_listed() {
    let missing = missing_targets("trust");
    assert!(
        missing.is_empty(),
        "add a [[test]] target (name = the file stem, path = \"tests/trust/<file>\") to crates/am-e2e/Cargo.toml for: {missing:?}"
    );
}

/// Story 3.3: every file in `tests/engine/` has its target, as the trust
/// files do.
#[test]
fn engine_targets_listed() {
    let missing = missing_targets("engine");
    assert!(
        missing.is_empty(),
        "add a [[test]] target (name = the file stem, path = \"tests/engine/<file>\") to crates/am-e2e/Cargo.toml for: {missing:?}"
    );
}

#[test]
fn targets_are_read_from_the_manifest() {
    let m = "[package]\nname = \"x\"\n\n[[test]]\nname = \"rule_16_audit_log\"\npath = \"tests/trust/rule_16_audit_log.rs\"\n\n[dev-dependencies]\nname = \"not-a-target\"\n";
    let t = targets(m);
    assert_eq!(
        t.into_iter().collect::<Vec<_>>(),
        vec![(
            "rule_16_audit_log".to_string(),
            "tests/trust/rule_16_audit_log.rs".to_string()
        )]
    );
}
