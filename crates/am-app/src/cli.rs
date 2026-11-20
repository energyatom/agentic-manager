//! CLI commands that need no runtime. `agentic validate [<project>]` (AD-16,
//! epic R1) prints every finding as `file:line: message` with a `fix:` line,
//! sorted by file and line, then a summary.

use std::io::Write;
use std::path::Path;

/// Exit codes of `agentic validate`.
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERRORS: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

const USAGE: &str = "usage: agentic validate [<project folder>]  (default: the current folder)";

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// `agentic validate [<project>]`: `args` are the words after `validate`.
/// Returns the exit code: 0 with no errors (warnings allowed), 1 with any
/// error, 2 for bad usage or a missing project folder.
pub fn validate(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        let _ = writeln!(out, "{USAGE}");
        return EXIT_OK;
    }
    if args.len() > 1 || args.iter().any(|a| a.starts_with('-')) {
        let _ = writeln!(err, "agentic validate: {USAGE}");
        return EXIT_USAGE;
    }
    let project = args.first().map(String::as_str).unwrap_or(".");
    let loaded = match am_config::load(Path::new(project)) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(err, "agentic validate: {e}");
            return EXIT_USAGE;
        }
    };
    for finding in &loaded.findings {
        let _ = writeln!(out, "{finding}");
    }
    let errors = loaded.error_count();
    let summary = format!(
        "{}, {}",
        plural(errors, "error"),
        plural(loaded.warning_count(), "warning")
    );
    match &loaded.snapshot {
        Some(snap) => {
            let _ = writeln!(
                out,
                "agentic validate: {project}: {summary} (snapshot {})",
                &snap.hash()[..12]
            );
        }
        None => {
            let _ = writeln!(out, "agentic validate: {project}: {summary}");
        }
    }
    if errors == 0 { EXIT_OK } else { EXIT_ERRORS }
}
