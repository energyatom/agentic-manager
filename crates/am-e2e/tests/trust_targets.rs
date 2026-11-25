//! The release gate runs every file in `tests/trust/` (AD-33) and, from
//! story 3.3, in `tests/engine/` (AGENTS.md: the engine tests of the
//! cross-department primitives); from story 3.24, `just check` runs the
//! golden run in `tests/golden/` on every PR. Cargo does not discover files in a
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

/// Story 3.24 (matrix row 18): every file in `tests/golden/` has its
/// target, as the trust and engine files do, so the golden run is never
/// silently skipped.
#[test]
fn golden_targets_listed() {
    let missing = missing_targets("golden");
    assert!(
        missing.is_empty(),
        "add a [[test]] target (name = the file stem, path = \"tests/golden/<file>\") to crates/am-e2e/Cargo.toml for: {missing:?}"
    );
}

/// The block of the `[[test]]` target named `name`, up to the next table.
fn target_block(manifest: &str, name: &str) -> Option<String> {
    manifest
        .split("[[test]]")
        .skip(1)
        .map(|block| block.split("\n[").next().unwrap_or(block))
        .find(|block| {
            block.lines().any(|l| {
                l.split_once('=')
                    .is_some_and(|(k, v)| k.trim() == "name" && v.trim().trim_matches('"') == name)
            })
        })
        .map(str::to_string)
}

/// The body lines of the justfile recipe `name` (its indented lines).
fn recipe(justfile: &str, name: &str) -> Vec<String> {
    let mut lines = justfile.lines().skip_while(|l| {
        !l.split_once(':')
            .is_some_and(|(head, _)| head.split_whitespace().next() == Some(name))
    });
    let Some(_) = lines.next() else {
        return Vec::new();
    };
    lines
        .take_while(|l| l.starts_with(' ') || l.starts_with('\t'))
        .map(|l| l.trim().to_string())
        .collect()
}

/// Story 3.24 (matrix row 14): `just check` runs the golden run on every
/// PR with no feature, no ignore and no recipe of its own. The target
/// `golden_so_far` has no `required-features`; its files hold no attribute
/// that skips a test or drops it on a platform ([`skipping_attributes`]:
/// review R7); am-e2e is a default workspace member, which `cargo nextest
/// run` without `--workspace` builds ([`default_member`], R7); `check`
/// lists `test`, which is `cargo nextest run` with no filter; nextest has
/// no default filter; and CI runs `just check` on macOS and the
/// workspace's tests on Linux.
#[test]
fn golden_so_far_runs_in_just_check() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = root.join("../..");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();

    let block =
        target_block(&manifest, "golden_so_far").expect("a [[test]] target named golden_so_far");
    assert!(
        block.contains("path = \"tests/golden/golden_so_far.rs\""),
        "{block}"
    );
    for word in ["required-features", "harness", "test = false"] {
        assert!(!block.contains(word), "the target sets `{word}`: {block}");
    }

    let mut files = Vec::new();
    let mut dirs = vec![root.join("tests/golden")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    assert!(!files.is_empty(), "tests/golden/ holds the golden run");
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let found = skipping_attributes(&text);
        assert!(
            found.is_empty(),
            "{} would skip a golden test, or drop it on a platform: {found:?}",
            file.display()
        );
    }
    let workspace = std::fs::read_to_string(repo.join("Cargo.toml")).unwrap();
    assert!(
        default_member(&workspace, "crates/am-e2e"),
        "am-e2e is a default workspace member, so `cargo nextest run` builds it"
    );

    let justfile = std::fs::read_to_string(repo.join("justfile")).unwrap();
    let check = justfile
        .lines()
        .find_map(|l| l.strip_prefix("check:"))
        .expect("a `check` recipe");
    assert!(
        check.split_whitespace().any(|w| w == "test"),
        "`check` runs `test`: {check}"
    );
    assert_eq!(
        recipe(&justfile, "test"),
        vec!["cargo nextest run".to_string()],
        "`test` is `cargo nextest run` with no filter"
    );

    for config in [".config/nextest.toml", "nextest.toml"] {
        let path = repo.join(config);
        if let Ok(text) = std::fs::read_to_string(&path) {
            assert!(
                !text.contains("default-filter"),
                "{config} sets a default filter, which could leave the golden run out"
            );
        }
    }

    let ci = std::fs::read_to_string(repo.join(".github/workflows/ci.yml")).unwrap();
    let runs: Vec<&str> = ci
        .lines()
        .filter_map(|l| l.trim().strip_prefix("run: "))
        .collect();
    assert!(
        runs.contains(&"just check"),
        "macOS runs `just check`: {runs:?}"
    );
    assert!(
        runs.contains(&"cargo nextest run --workspace --exclude am-desktop"),
        "Linux runs the workspace's tests: {runs:?}"
    );
}

/// The attributes in `text` (comment lines left out) that could keep a
/// golden test out of `just check` or out of the Linux job: `#[ignore]`, a
/// `cfg_attr(…)` that ignores, any file-level `#![cfg(…)]`, and a
/// `cfg(…)` or `cfg_attr(…)` that names a platform. Each is returned with
/// its whitespace removed.
fn skipping_attributes(text: &str) -> Vec<String> {
    const PLATFORMS: [&str; 6] = [
        "target_os",
        "target_family",
        "unix",
        "windows",
        "macos",
        "linux",
    ];
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut found = Vec::new();
    let mut rest = code.as_str();
    while let Some(at) = rest.find('#') {
        let after = &rest[at + 1..];
        let (inner, body) = match (after.strip_prefix("!["), after.strip_prefix('[')) {
            (Some(body), _) => (true, body),
            (None, Some(body)) => (false, body),
            _ => {
                rest = after;
                continue;
            }
        };
        let mut depth = 1;
        let end = body
            .char_indices()
            .find(|&(_, c)| {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .map(|(i, _)| i)
            .unwrap_or(body.len());
        let attr: String = body[..end].chars().filter(|c| !c.is_whitespace()).collect();
        let platform = PLATFORMS.iter().any(|p| attr.contains(p));
        let skips = attr == "ignore"
            || attr.starts_with("ignore=")
            || (attr.starts_with("cfg_attr(") && (attr.contains(",ignore") || platform))
            || (inner && attr.starts_with("cfg("))
            || (attr.starts_with("cfg(") && platform);
        if skips {
            found.push(format!("#{}[{attr}]", if inner { "!" } else { "" }));
        }
        rest = &body[end.min(body.len())..];
    }
    found
}

/// The quoted entries of the array `key = [ … ]` in a TOML text, or `None`
/// when no line starts with `key`.
fn toml_list(text: &str, key: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == key))?;
    let mut out = Vec::new();
    for (n, line) in lines[start..].iter().enumerate() {
        let value = if n == 0 {
            line.split_once('=').map(|(_, v)| v).unwrap_or_default()
        } else {
            line
        };
        out.extend(value.split('"').skip(1).step_by(2).map(str::to_string));
        if value.contains(']') {
            break;
        }
    }
    Some(out)
}

/// Whether `cargo nextest run` without `--workspace` builds the workspace
/// member `member`: it is in `default-members` when the workspace sets
/// that list, and in `members` otherwise.
fn default_member(workspace: &str, member: &str) -> bool {
    let wanted = |list: Option<Vec<String>>| list.is_some_and(|l| l.iter().any(|m| m == member));
    match toml_list(workspace, "default-members") {
        Some(defaults) => wanted(Some(defaults)) && wanted(toml_list(workspace, "members")),
        None => wanted(toml_list(workspace, "members")),
    }
}

/// Review R7: the guards catch, in planted text, `#[ignore]`, a
/// `cfg_attr(…, ignore)`, a file-level `#![cfg(…)]` and an item `cfg(…)`
/// naming a platform; and a crate left out of `default-members`.
#[test]
fn the_skip_guards_catch_planted_attributes() {
    for planted in [
        "#[test]\n#[ignore]\nfn a() {}",
        "#[test]\n#[cfg_attr(not(target_os = \"macos\"), ignore)]\nfn a() {}",
        "#[test]\n#[cfg_attr(\n    feature = \"slow\",\n    ignore = \"later\"\n)]\nfn a() {}",
        "#![forbid(unsafe_code)]\n#![cfg(target_os = \"macos\")]\nfn a() {}",
        "#![cfg(feature = \"golden\")]\nfn a() {}",
        "#[cfg(target_os = \"macos\")]\n#[test]\nfn a() {}",
    ] {
        assert!(
            !skipping_attributes(planted).is_empty(),
            "not caught: {planted:?}"
        );
    }
    for clean in [
        "#![forbid(unsafe_code)]\n#[test]\nfn a() {}",
        "#[tokio::test]\nasync fn b() {}",
        "// a test is never #[ignore]d here\n#[derive(Debug)]\nstruct C;",
    ] {
        assert_eq!(
            skipping_attributes(clean),
            Vec::<String>::new(),
            "{clean:?}"
        );
    }
    let with = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/am-e2e\",\n]\ndefault-members = [\n    \"crates/a\",\n    \"crates/am-e2e\",\n]\n";
    let left_out = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/am-e2e\",\n]\ndefault-members = [\n    \"crates/a\",\n]\n";
    let no_default = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/am-e2e\",\n]\n";
    let not_member = "[workspace]\nmembers = [\n    \"crates/a\",\n]\n";
    assert!(default_member(with, "crates/am-e2e"));
    assert!(!default_member(left_out, "crates/am-e2e"));
    assert!(default_member(no_default, "crates/am-e2e"));
    assert!(!default_member(not_member, "crates/am-e2e"));
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
