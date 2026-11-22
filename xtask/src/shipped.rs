//! The shipped source the story 2.2 scans read: every `.rs` file under a
//! crate's `src/` (`crates/<crate>/src/**`), never its `tests/`, and the
//! line ranges of its `#[cfg(test)]` items, which are not shipped either.

use std::path::Path;

use crate::forbid::strip_comments_and_strings;
use crate::walk::{files, rel};

/// One shipped source file: its path relative to the root, and its text.
pub struct Source {
    pub name: String,
    pub text: String,
}

/// Every `crates/<crate>/src/**/*.rs` file under `root`, sorted.
pub fn sources(root: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    for path in files(&root.join("crates")) {
        let name = rel(root, &path);
        if !is_shipped(&name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        out.push(Source { name, text });
    }
    out
}

/// `crates/<crate>/src/…/<file>.rs`.
fn is_shipped(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    parts.len() >= 4 && parts[0] == "crates" && parts[2] == "src" && name.ends_with(".rs")
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
    fn shipped_means_a_crates_src() {
        assert!(is_shipped("crates/am-net/src/lib.rs"));
        assert!(is_shipped("crates/am-app/src/bin/x.rs"));
        assert!(!is_shipped("crates/am-net/tests/t.rs"));
        assert!(!is_shipped("xtask/src/main.rs"));
        assert!(!is_shipped("crates/am-net/src/notes.md"));
    }
}
