//! The shipped source the story 2.2 scans read: every `.rs` file of a crate
//! but its `tests/`, `benches/` and `examples/` (story 3.3, review P5: a
//! `build.rs`, or a file a `#[path]` module or an `include!` pulls in from
//! outside `src/`, is read too), and the line ranges of its `#[cfg(test)]`
//! items, which are not shipped either.

use std::path::{Component, Path, PathBuf};

use crate::forbid::strip_comments_and_strings;
use crate::walk::rel;

/// One shipped source file: its path relative to the root, and its text.
pub struct Source {
    pub name: String,
    pub text: String,
}

/// What the scans read under `root`'s `crates/`: the shipped sources, and
/// the findings of the tree itself (story 3.3, review R3): a symlink, a
/// `.rs` or folder that cannot be read, and a crate manifest whose `[lib]`
/// or `[[bin]]` `path`, or whose `build` script, is outside the shipped
/// files.
#[derive(Default)]
pub struct Tree {
    pub sources: Vec<Source>,
    pub findings: Vec<String>,
}

/// Every shipped `.rs` file under `root`'s `crates/`, sorted (see
/// [`is_shipped`] and [`tree`]).
pub fn sources(root: &Path) -> Vec<Source> {
    tree(root).sources
}

/// The folders skipped at a crate's top level only: build output and
/// packages, and hidden folders. The same names deeper in a crate are read
/// like any folder (review R3).
fn top_level_skipped(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "dist")
}

/// Walks `root`'s `crates/` itself (no git listing, no symlink followed):
/// see [`Tree`].
pub fn tree(root: &Path) -> Tree {
    let mut tree = Tree::default();
    let crates = root.join("crates");
    for entry in entries(root, &crates, &mut tree) {
        let path = entry.path();
        let name = rel(root, &path);
        let Ok(kind) = entry.file_type() else {
            tree.findings.push(unreadable(&name));
            continue;
        };
        if kind.is_symlink() {
            tree.findings.push(symlink(&name));
        } else if kind.is_dir() {
            walk(root, &path, true, &mut tree);
            manifest_targets(root, &path, &mut tree);
        }
    }
    tree.sources.sort_by(|a, b| a.name.cmp(&b.name));
    tree.findings.sort();
    tree
}

/// The entries of `dir`, sorted; a folder that cannot be read is a finding.
fn entries(root: &Path, dir: &Path, tree: &mut Tree) -> Vec<std::fs::DirEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        if dir.exists() {
            tree.findings.push(unreadable(&rel(root, dir)));
        }
        return Vec::new();
    };
    let mut all: Vec<std::fs::DirEntry> = read.flatten().collect();
    all.sort_by_key(|e| e.file_name());
    all
}

fn walk(root: &Path, dir: &Path, top: bool, tree: &mut Tree) {
    for entry in entries(root, dir, tree) {
        let path = entry.path();
        let file = entry.file_name().to_string_lossy().into_owned();
        if top && top_level_skipped(&file) {
            continue;
        }
        let name = rel(root, &path);
        let Ok(kind) = entry.file_type() else {
            tree.findings.push(unreadable(&name));
            continue;
        };
        if kind.is_symlink() {
            tree.findings.push(symlink(&name));
        } else if kind.is_dir() {
            walk(root, &path, false, tree);
        } else if kind.is_file() && is_shipped(&name) {
            match std::fs::read_to_string(&path) {
                Ok(text) => tree.sources.push(Source { name, text }),
                Err(_) => tree.findings.push(unreadable(&name)),
            }
        }
    }
}

fn symlink(name: &str) -> String {
    format!(
        "{name}: a symlink under crates/; the scans read files where they are, and a link could bring an unread file into the build (story 3.3); replace it with the file or folder itself"
    )
}

fn unreadable(name: &str) -> String {
    format!(
        "{name}: cannot be read, so the scans cannot tell what it holds (it fails closed); make it readable"
    )
}

/// A crate manifest's `[lib]` and `[[bin]]` `path` keys and its `build`
/// key: each must name a file the scans read (review R3).
fn manifest_targets(root: &Path, krate: &Path, tree: &mut Tree) {
    let manifest = krate.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return;
    };
    let shown = rel(root, &manifest);
    let mut section = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let checked = matches!(
            (section.as_str(), key),
            ("[lib]" | "[[bin]]", "path") | ("[package]", "build")
        );
        let value = value.trim();
        if !checked || !value.starts_with('"') {
            continue;
        }
        let target = value.trim_matches('"');
        if !in_scanned_set(root, krate, target) {
            tree.findings.push(format!(
                "{shown}: `{key} = \"{target}\"` in {} names a file outside the shipped source the scans read; keep the crate's library and binaries under src/ and its build script at build.rs (story 3.3)",
                if section.is_empty() { "the manifest" } else { section.as_str() }
            ));
        }
    }
}

/// True when `target`, relative to the crate folder `krate`, is a file the
/// scans read: inside the crate, not in a top-level skipped folder, and
/// shipped (see [`is_shipped`]).
fn in_scanned_set(root: &Path, krate: &Path, target: &str) -> bool {
    let mut path = PathBuf::new();
    for part in krate.join(target).components() {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            other => path.push(other),
        }
    }
    let Ok(inner) = path.strip_prefix(krate) else {
        return false;
    };
    let top_skipped = inner
        .components()
        .next()
        .is_some_and(|c| top_level_skipped(&c.as_os_str().to_string_lossy()));
    !top_skipped && is_shipped(&rel(root, &path))
}

/// The folders of a crate that are never shipped.
const NOT_SHIPPED: [&str; 3] = ["tests", "benches", "examples"];

/// `crates/<crate>/…/<file>.rs`, but not under the crate's `tests/`,
/// `benches/` or `examples/`.
fn is_shipped(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    parts.len() >= 3
        && parts[0] == "crates"
        && name.ends_with(".rs")
        && !(parts.len() > 3 && NOT_SHIPPED.contains(&parts[2]))
}

/// The 1-based, inclusive line ranges of the `#[cfg(test)]` items of
/// `src`: from the attribute to the end of the item (its matching `}`, or
/// its `;`). Comments and literals are blanked first, so neither can open
/// or close a range.
pub fn test_ranges(src: &str) -> Vec<(usize, usize)> {
    let code: Vec<char> = strip_comments_and_strings(src).chars().collect();
    let line_at = |i: usize| {
        code[..i.min(code.len())]
            .iter()
            .filter(|c| **c == '\n')
            .count()
            + 1
    };
    let flat_at = |from: usize, word: &str| -> Option<usize> {
        // `word` at `from`, ignoring whitespace inside it; the index after it.
        let mut i = from;
        for want in word.chars() {
            while code.get(i).is_some_and(|c| c.is_whitespace()) {
                i += 1;
            }
            if code.get(i) != Some(&want) {
                return None;
            }
            i += 1;
        }
        Some(i)
    };
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < code.len() {
        let Some(after) = (code[i] == '#')
            .then(|| flat_at(i, "#[cfg(test)]"))
            .flatten()
        else {
            i += 1;
            continue;
        };
        // The item's end, at the attribute's own depth: a `;` or `,` (an
        // item, a field, a variant or an arm), the `}` closing a body the
        // item opened, or just before a bracket that closes the enclosing
        // item (a last field or arm with no comma). Brackets inside the
        // item are skipped over, so only its own end counts.
        let mut j = after;
        let mut depth = 0usize;
        let mut end = code.len().saturating_sub(1);
        while j < code.len() {
            match code[j] {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => {
                    end = j.saturating_sub(1);
                    break;
                }
                ')' | ']' => depth -= 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = j;
                        break;
                    }
                }
                ';' | ',' if depth == 0 => {
                    end = j;
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        ranges.push((line_at(i), line_at(end)));
        i = end + 1;
    }
    ranges
}

/// True when `line` lies in one of `ranges`.
pub fn in_ranges(ranges: &[(usize, usize)], line: usize) -> bool {
    ranges.iter().any(|(a, b)| (*a..=*b).contains(&line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cfg_test_module_is_a_range() {
        let src =
            "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { let _ = \"}\"; }\n}\nfn c() {}\n";
        assert_eq!(test_ranges(src), vec![(2, 5)]);
        assert_eq!(
            test_ranges("#[cfg(test)]\nuse x;\nfn a() {}\n"),
            vec![(1, 2)]
        );
        assert!(test_ranges("// #[cfg(test)]\nfn a() {}\n").is_empty());
        // A field, a variant or an arm: only its own line.
        let field = "struct S {\n    #[cfg(test)]\n    a: (u8, u8),\n    b: u8,\n}\nfn c() {}\n";
        assert_eq!(test_ranges(field), vec![(2, 3)]);
        let last = "enum E {\n    A,\n    #[cfg(test)]\n    B\n}\nfn c() {}\n";
        assert_eq!(test_ranges(last), vec![(3, 4)]);
        let arm = "match n {\n    #[cfg(test)]\n    0 => { x() }\n    _ => y(),\n}\n";
        assert_eq!(test_ranges(arm), vec![(2, 3)]);
    }

    #[test]
    fn shipped_means_a_crates_rust_but_its_tests_benches_and_examples() {
        assert!(is_shipped("crates/am-net/src/lib.rs"));
        assert!(is_shipped("crates/am-app/src/bin/x.rs"));
        assert!(is_shipped("crates/am-app/build.rs"));
        assert!(is_shipped("crates/am-app/extra/start.rs"));
        assert!(!is_shipped("crates/am-net/tests/t.rs"));
        assert!(!is_shipped("crates/am-net/benches/b.rs"));
        assert!(!is_shipped("crates/am-net/examples/e.rs"));
        assert!(!is_shipped("xtask/src/main.rs"));
        assert!(!is_shipped("crates/am-net/src/notes.md"));
    }
}
