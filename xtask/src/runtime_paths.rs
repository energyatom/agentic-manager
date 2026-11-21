//! `runtime-paths` (story 1.13, AD-32): one type, `am_domain::RuntimeLayout`,
//! names every runtime path, so no Rust string literal under `crates/`
//! outside `crates/am-domain/src/runtime.rs` may hold `.agentic` at its
//! start or right after a `/` (`".agentic/state.db"`, `"/p/.agentic"`,
//! `".agentic-open"`). Other spellings pass (`app.agentic-manager`), and
//! comments are not code. Reuses the walk and the literal scanner of
//! `check-forbid`.

use std::path::Path;

use crate::Failure;
use crate::forbid::string_literals;
use crate::walk::{files, rel};

/// The one file allowed to spell a runtime path.
const HOME: &str = "crates/am-domain/src/runtime.rs";
const WORD: &str = ".agentic";

pub fn run(root: &Path) -> Result<String, Failure> {
    let mut findings = Vec::new();
    let mut scanned = 0usize;
    for path in files(&root.join("crates")) {
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let name = rel(root, &path);
        if name == HOME {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        scanned += 1;
        for (line, text) in string_literals(&src) {
            if spells_runtime_path(&text) {
                findings.push(format!(
                    "{name}:{line}: the string literal \"{text}\" spells a runtime path; name it through am_domain::RuntimeLayout or the constants in {HOME} (AD-32)"
                ));
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{scanned} files clean: runtime paths are named only in {HOME}"
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}

/// `.agentic` at the start of the literal or right after a `/`.
fn spells_runtime_path(text: &str) -> bool {
    text.starts_with(WORD) || text.contains(&format!("/{WORD}"))
}

#[cfg(test)]
mod tests {
    use super::spells_runtime_path;

    #[test]
    fn the_rule() {
        for bad in [
            ".agentic",
            ".agentic/state.db",
            "/x/.agentic/y",
            "p/.agentic-open",
        ] {
            assert!(spells_runtime_path(bad), "{bad}");
        }
        for ok in [
            "app.agentic-manager",
            "agentic",
            "a .agentic folder",
            "x.agentic",
        ] {
            assert!(!spells_runtime_path(ok), "{ok}");
        }
    }
}
