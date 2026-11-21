//! `check-forbid`: every crate forbids `unsafe`. Each crate under `crates/`
//! (and `xtask/`) must inherit the workspace lints (`[lints] workspace = true`,
//! where `unsafe_code = "forbid"` lives), carry `#![forbid(unsafe_code)]` in
//! each crate root (`src/lib.rs`, `src/main.rs`, `src/bin/*.rs`, `build.rs`),
//! and contain no `unsafe` keyword outside comments and string literals.

use crate::Failure;
use crate::walk::{files, is_word, rel};
use std::path::{Path, PathBuf};

pub fn run(root: &Path) -> Result<String, Failure> {
    let crates = crate_dirs(root);
    let mut findings = Vec::new();
    for dir in &crates {
        let manifest = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).unwrap_or_default();
        if !inherits_workspace_lints(&text) {
            findings.push(format!(
                "{}: missing `[lints] workspace = true` (the workspace forbids unsafe_code)",
                rel(root, &manifest)
            ));
        }
        for path in files(dir) {
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            let code = strip_comments_and_strings(&src);
            if is_crate_root(dir, &path) && !forbids_unsafe(&code) {
                findings.push(format!(
                    "{}: crate root lacks `#![forbid(unsafe_code)]`",
                    rel(root, &path)
                ));
            }
            for line in unsafe_lines(&code) {
                findings.push(format!(
                    "{}:{line}: `unsafe` code is forbidden in every crate",
                    rel(root, &path)
                ));
            }
        }
    }
    if findings.is_empty() {
        Ok(format!("{} crates forbid unsafe code", crates.len()))
    } else {
        Err(Failure::Findings(findings))
    }
}

fn crate_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for e in entries.flatten() {
            if e.path().join("Cargo.toml").is_file() {
                dirs.push(e.path());
            }
        }
    }
    if root.join("xtask/Cargo.toml").is_file() {
        dirs.push(root.join("xtask"));
    }
    dirs.sort();
    dirs
}

fn is_crate_root(dir: &Path, path: &Path) -> bool {
    let r = rel(dir, path);
    matches!(r.as_str(), "src/lib.rs" | "src/main.rs" | "build.rs")
        || (r.starts_with("src/bin/") && r.matches('/').count() == 2)
}

fn inherits_workspace_lints(manifest: &str) -> bool {
    let mut in_lints = false;
    for line in manifest.lines() {
        let l: String = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if l.starts_with('[') {
            in_lints = l == "[lints]";
            continue;
        }
        if (in_lints && l == "workspace=true")
            || l == "lints.workspace=true"
            || l == "lints={workspace=true}"
        {
            return true;
        }
    }
    false
}

fn forbids_unsafe(code: &str) -> bool {
    let flat: String = code.split_whitespace().collect();
    flat.split("#![forbid(").skip(1).any(|rest| {
        rest.split(')')
            .next()
            .is_some_and(|args| args.split(',').any(|a| a == "unsafe_code"))
    })
}

/// 1-based line numbers holding the `unsafe` keyword.
fn unsafe_lines(code: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    for (n, line) in code.lines().enumerate() {
        let mut from = 0;
        while let Some(pos) = line[from..].find("unsafe") {
            let start = from + pos;
            let end = start + "unsafe".len();
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
                lines.push(n + 1);
                break;
            }
            from = end;
        }
    }
    lines
}

/// Blanks comments, string literals and char literals (keeping newlines), so
/// only code remains for the keyword and attribute checks.
fn strip_comments_and_strings(src: &str) -> String {
    scan(src).0
}

/// Every string literal in `src` (normal, byte and raw), as (1-based line
/// of its opening quote, its text between the quotes, escapes as written).
/// Literals in comments are not code and are skipped.
pub fn string_literals(src: &str) -> Vec<(usize, String)> {
    scan(src).1
}

/// One pass over `src`: the code with comments and literals blanked, and
/// the string literals found.
fn scan(src: &str) -> (String, Vec<(usize, String)>) {
    let mut literals = Vec::new();
    let line_at = |out: &str| out.matches('\n').count() + 1;
    let c: Vec<char> = src.chars().collect();
    let n = c.len();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let blank = |ch: char| if ch == '\n' { '\n' } else { ' ' };
    let ident_before = |i: usize| i > 0 && is_word(c[i - 1]);
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < n {
        let ch = c[i];
        // Line comment.
        if ch == '/' && at(i + 1) == '/' {
            while i < n && c[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        // Block comment (nested).
        if ch == '/' && at(i + 1) == '*' {
            let mut depth = 0usize;
            while i < n {
                if c[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(c[i]));
                    i += 1;
                }
            }
            continue;
        }
        // Raw (byte) string: r"..", r#".."#, br"..".
        if (ch == 'r' || (ch == 'b' && at(i + 1) == 'r')) && !ident_before(i) {
            let mut j = i + if ch == 'b' { 2 } else { 1 };
            let mut hashes = 0;
            while at(j) == '#' {
                hashes += 1;
                j += 1;
            }
            if at(j) == '"' {
                let mut k = j + 1;
                let mut end = n;
                while k < n {
                    if c[k] == '"' && (1..=hashes).all(|h| at(k + h) == '#') {
                        end = k;
                        k += 1 + hashes;
                        break;
                    }
                    k += 1;
                }
                let k = k.min(n);
                literals.push((line_at(&out), c[j + 1..end.min(n)].iter().collect()));
                for &x in &c[i..k] {
                    out.push(blank(x));
                }
                i = k;
                continue;
            }
        }
        // Normal (byte) string.
        if ch == '"' {
            let line = line_at(&out);
            let mut text = String::new();
            out.push(' ');
            i += 1;
            while i < n {
                if c[i] == '\\' {
                    out.push(' ');
                    text.push('\\');
                    if i + 1 < n {
                        out.push(blank(c[i + 1]));
                        text.push(c[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                let end = c[i] == '"';
                out.push(blank(c[i]));
                i += 1;
                if end {
                    break;
                }
                text.push(c[i - 1]);
            }
            literals.push((line, text));
            continue;
        }
        // Char literal ('x', '\n', '\''); anything else is a lifetime.
        if ch == '\'' {
            if at(i + 1) == '\\' {
                let mut k = i + 3;
                while k < n && c[k] != '\'' {
                    k += 1;
                }
                let k = (k + 1).min(n);
                for &x in &c[i..k] {
                    out.push(blank(x));
                }
                i = k;
                continue;
            }
            if at(i + 2) == '\'' {
                out.push_str("   ");
                i += 3;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    (out, literals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_strings_and_chars_but_keeps_code() {
        let src = "let a = '\"'; // unsafe\nlet b = \"unsafe { }\"; /* unsafe /* nested */ */\nunsafe { x }\nlet r = r##\"unsafe\"##; let l: &'static str = \"\";\n";
        let code = strip_comments_and_strings(src);
        assert_eq!(unsafe_lines(&code), vec![3]);
        assert_eq!(code.lines().count(), src.lines().count());
    }

    #[test]
    fn string_literals_are_found_with_their_line_but_not_in_comments() {
        let src = "// \"in a comment\"\nlet a = \"one\";\nlet b = r#\"two \"q\"\"#; /* \"no\" */\nlet c = b\"three\\\"x\";\n";
        assert_eq!(
            string_literals(src),
            vec![
                (2, "one".to_string()),
                (3, "two \"q\"".to_string()),
                (4, "three\\\"x".to_string())
            ]
        );
    }

    #[test]
    fn forbid_attribute_with_several_lints_counts() {
        assert!(forbids_unsafe("#![forbid(unsafe_code, missing_docs)]"));
        assert!(!forbids_unsafe("#![deny(unsafe_code)]"));
    }

    #[test]
    fn manifest_lints_forms() {
        assert!(inherits_workspace_lints("[lints]\nworkspace = true\n"));
        assert!(inherits_workspace_lints("lints.workspace = true\n"));
        assert!(!inherits_workspace_lints(
            "[lints.rust]\nworkspace = true\n"
        ));
        assert!(!inherits_workspace_lints("[package]\nname = \"a\"\n"));
    }
}
