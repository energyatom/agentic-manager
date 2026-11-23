//! Story 3.19, row 8 of the plan's I/O matrix: the owner's constraints,
//! verbatim "no new dependencies, and keep the `nix` crate's `process`
//! feature unchanged (not enabled)" and (2026-11-24) "do not call nix's
//! process functions". Checked on the workspace's files:
//!
//! - the workspace nix line is the Stack's, `features = ["term"]` only;
//! - no Cargo.toml in the repository names nix with another feature (a
//!   member takes `nix.workspace = true` and nothing more, and no
//!   `nix/<feature>` appears in a features table);
//! - am-app's and am-server's code (comments left out) uses no nix item
//!   outside `term` (`nix::sys::termios`, `nix::pty`; `nix::errno`,
//!   `nix::Error`, `nix::Result` are featureless), and no `nix::unistd`,
//!   `nix::sys::signal`, `tcgetpgrp`, `getpgrp`, `setpgid`, `sigaction`,
//!   `libc::`, and no SIGTTIN or SIGTTOU handler (the plan's Never list).
//!
//! "No new dependencies" (Cargo.toml and Cargo.lock gain no crate) was this
//! story's constraint, not a standing rule: it was checked once against the
//! plan's baseline revision and recorded in the plan (review round 1, P1),
//! not kept as a test.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

/// The Stack's nix line, unchanged.
const NIX_LINE: &str =
    r#"nix = { version = "=0.31.4", default-features = false, features = ["term"] }"#;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every file named `name` under `dir`, leaving out build output, VCS and
/// tool folders and the git-ignored `_archive/` (never read).
fn files_named(dir: &Path, name: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for en in rd.flatten() {
        let p = en.path();
        let file = en.file_name().to_string_lossy().into_owned();
        match en.file_type() {
            Ok(t) if t.is_dir() => {
                if file.starts_with('.')
                    || ["target", "node_modules", "_archive", "_bmad-output"]
                        .contains(&file.as_str())
                {
                    continue;
                }
                files_named(&p, name, out);
            }
            Ok(t) if t.is_file() && file == name => out.push(p),
            _ => {}
        }
    }
}

/// A TOML line without its comment, trimmed.
fn toml_code(line: &str) -> &str {
    line.split('#').next().unwrap_or("").trim()
}

/// The key a TOML line sets (before `=` or a dotted `.`), unquoted.
fn toml_key(line: &str) -> &str {
    line.split(['=', '.'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
}

#[test]
fn the_workspace_nix_line_declares_the_term_feature_only() {
    let manifest = read(&repo_root().join("Cargo.toml"));
    let nix: Vec<&str> = manifest
        .lines()
        .map(toml_code)
        .filter(|l| !l.starts_with('[') && toml_key(l) == "nix")
        .collect();
    assert_eq!(nix, vec![NIX_LINE], "the workspace's nix line");
}

#[test]
fn no_manifest_names_nix_with_another_feature() {
    let root = repo_root();
    let mut manifests = Vec::new();
    files_named(&root, "Cargo.toml", &mut manifests);
    assert!(manifests.len() > 10, "{manifests:?}");
    for m in manifests {
        let is_root = m == root.join("Cargo.toml");
        for line in read(&m).lines().map(toml_code) {
            let shown = m.display();
            if line.starts_with('[') {
                assert!(
                    !line.trim_end_matches(']').ends_with(".nix") && !line.contains(".nix."),
                    "{shown}: nix as a table ({line}); use the workspace line"
                );
                continue;
            }
            assert!(
                !line.contains("nix/"),
                "{shown}: a feature turns on a nix feature: {line}"
            );
            if toml_key(line) != "nix" {
                continue;
            }
            if is_root {
                assert_eq!(line, NIX_LINE, "{shown}");
            } else {
                let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
                assert!(
                    compact == "nix.workspace=true" || compact == "nix={workspace=true}",
                    "{shown}: a member takes the workspace's nix as it is: {line}"
                );
            }
        }
    }
}

/// The code of a Rust file with its line comments (`//`, `///`, `//!`)
/// left out.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn am_app_and_am_server_use_no_nix_item_outside_term_and_no_process_or_signal_call() {
    const FORBIDDEN: [&str; 11] = [
        "nix::unistd",
        "nix::sys::signal",
        "tcgetpgrp",
        "getpgrp",
        "setpgid",
        "sigaction",
        "libc::",
        "SIGTTIN",
        "SIGTTOU",
        "from_raw(21)",
        "from_raw(22)",
    ];
    const TERM_ITEMS: [&str; 5] = ["sys::termios", "pty", "errno", "Error", "Result"];
    let root = repo_root();
    let mut sources = Vec::new();
    for krate in ["am-app", "am-server"] {
        let mut rs = Vec::new();
        collect_rs(&root.join("crates").join(krate).join("src"), &mut rs);
        assert!(!rs.is_empty(), "{krate} has sources");
        sources.extend(rs);
    }
    for f in sources {
        let code = code_only(&read(&f));
        let shown = f.display();
        for word in FORBIDDEN {
            assert!(!code.contains(word), "{shown} uses {word}");
        }
        for (i, _) in code.match_indices("nix::") {
            // `unix::` is not nix.
            if code[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let rest = &code[i + "nix::".len()..];
            assert!(
                TERM_ITEMS.iter().any(|t| rest.starts_with(t)),
                "{shown}: a nix item outside `term`: nix::{}",
                rest.lines().next().unwrap_or("")
            );
        }
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for en in rd.flatten() {
        let p = en.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}
