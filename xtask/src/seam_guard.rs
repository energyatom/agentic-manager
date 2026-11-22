//! `seam-guard` (story 2.2, decision 7): the egress client's test seams (a
//! resolver and a per-hop transport) are parameters of a second
//! constructor, `NetEgress::with_seams`, used only by tests. Shipped code
//! never names it: the guard fails when `with_seams` appears in the code
//! (comments and literals are not code) of any `crates/*/src` file, a
//! `#[cfg(test)]` module included, naming the file and line. The one
//! exception is its definition (`fn with_seams`) in am-net's source: a call
//! from shipped am-net code fails too. `NetEgress::new` always uses the system
//! resolver and reqwest, and no env var, flag or feature changes either.

use std::path::Path;

use crate::Failure;
use crate::forbid::strip_comments_and_strings;
use crate::shipped::sources;
use crate::walk::is_word;

/// The test-only constructor.
const NAME: &str = "with_seams";
/// Where it is defined.
const HOME: &str = "crates/am-net/src/";
/// The definition, as code reads (whitespace collapsed).
const DEFINITION: &str = "fn with_seams";

pub fn run(root: &Path) -> Result<String, Failure> {
    let mut findings = Vec::new();
    let mut scanned = 0usize;
    for source in sources(root) {
        scanned += 1;
        let home = source.name.starts_with(HOME);
        let code = strip_comments_and_strings(&source.text);
        for (n, line) in code.lines().enumerate() {
            let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if home && flat.contains(DEFINITION) && flat.matches(NAME).count() == 1 {
                continue;
            }
            if names(line) {
                findings.push(format!(
                    "{}:{}: names NetEgress::{NAME}, am-net's test-only constructor; shipped code builds the client with NetEgress::new (decision 7: test seams are constructor parameters, never switches)",
                    source.name,
                    n + 1
                ));
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{scanned} files clean: only tests and its definition in {HOME} name NetEgress::{NAME}"
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}

/// `with_seams` as a whole word in `line`.
fn names(line: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = line[from..].find(NAME) {
        let start = from + pos;
        let end = start + NAME.len();
        let before = line[..start].chars().next_back();
        let after = line[end..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::names;

    #[test]
    fn a_whole_word_only() {
        assert!(names("let _ = am_net::NetEgress::with_seams(a, b);"));
        assert!(!names("fn with_seams_x() {}"));
        assert!(!names("fn build_with_seams() {}"));
    }
}
