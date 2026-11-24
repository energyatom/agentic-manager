//! CLI commands that need no runtime. `agentic validate [--template]
//! [<folder>]` (AD-16, epic R1) prints every finding as `file:line: message`
//! with a `fix:` line, sorted by file and line, then a summary. `agentic
//! audit export|verify` (AD-25, story 1.7) exports a project's audit log as
//! JSON Lines and checks an export's hash chain. `agentic run` and `agentic
//! status` (epic 3 story 1) are in [`crate::run`] and re-exported here.
//!
//! Both find a project's runtime folder read-only (story 1.13, AD-32): no
//! lock, no write and no identity tool. `validate` on a project checks
//! reference files against its runtime folder (or skips that check when it
//! has none on this Mac) and never writes an id; `validate --template`
//! checks them against the folder's `runtime-seed/` and refuses a
//! `project_id`.

use std::io::Write;
use std::path::Path;

/// `agentic accounts add|remove|list|test|migrate` (stories 2.1 and 2.13, AD-15's interim CLI
/// Account edit); see [`crate::accounts`].
pub use crate::accounts::{AccountsContext, accounts};
/// `agentic gate [<item id>]` (epic 3 story 4); see [`crate::gate`].
pub use crate::gate::{GateContext, gate};
/// `agentic run <workflow>` and `agentic status` (epic 3 story 1); see
/// [`crate::run`].
pub use crate::run::{Providers, RunContext, run, status, unwired_providers};

/// What the `agentic` binary gives every CLI-class command (AD-15): this
/// platform's places, the OS identity adapter and the owner's real
/// terminal. `main` builds it in one place, so the binary builds one
/// terminal for every command (story 2.1's release guard counts them), and
/// each command's context ([`AccountsContext`], [`RunContext`]) takes its
/// parts from it.
pub struct OwnerSide {
    pub places: crate::runtime::Places,
    pub identity: std::sync::Arc<dyn am_domain::ProjectIdentity>,
    pub terminal: Box<dyn am_domain::Terminal>,
}

/// Exit codes of `agentic validate`.
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERRORS: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

const USAGE: &str = "usage: agentic validate [--template] [<project or template folder>]  (default: the current folder)";

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The root of every project recorded in this Mac's runtime base except
/// `root` itself (paths compared canonicalised, or as written when one
/// cannot be).
fn other_recorded_roots(places: &crate::runtime::Places, root: &Path) -> Vec<std::path::PathBuf> {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let own = canonical(root);
    crate::runtime::records(&places.base)
        .into_iter()
        .map(|r| r.root.path)
        .filter(|path| canonical(path) != own)
        .collect()
}

/// How a project (not a template) is loaded, by `agentic validate` and
/// `agentic run` alike: reference files are checked against its runtime
/// folder's `knowledge/` when it has one on this Mac, and a linked folder
/// that is, holds or lies inside another project this Mac has opened (a
/// recorded root) is refused (AD-30). With no `places`, neither is known.
pub(crate) fn project_load_options(
    places: Option<&crate::runtime::Places>,
    root: &Path,
) -> am_config::LoadOptions {
    let knowledge = places.and_then(|places| crate::runtime::knowledge_root(places, root));
    let recent_roots = places
        .map(|places| other_recorded_roots(places, root))
        .unwrap_or_default();
    am_config::LoadOptions {
        recent_roots,
        ..am_config::LoadOptions::project(knowledge)
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
    let template = args.iter().any(|a| a == "--template");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--template").collect();
    if rest.len() > 1 || rest.iter().any(|a| a.starts_with('-')) {
        let _ = writeln!(err, "agentic validate: {USAGE}");
        return EXIT_USAGE;
    }
    let project = rest.first().map(|s| s.as_str()).unwrap_or(".");
    let root = Path::new(project);
    // A project names Accounts by id only (story 2.12, AD-17): it is
    // checked against this Mac's user list (an empty one when the Mac has
    // none), whose own findings are reported too. A template is checked for
    // form only and never reads the app data dir.
    let mut user_findings = Vec::new();
    let opts = if template {
        am_config::LoadOptions::template(root)
    } else {
        let places = crate::runtime::Places::from_env().ok();
        let list = match &places {
            Some(places) => {
                let loaded = am_config::user_accounts::load(&places.user.accounts_file());
                user_findings = loaded.findings;
                loaded.accounts
            }
            None => am_config::UserAccounts::default(),
        };
        am_config::LoadOptions {
            user_accounts: Some(list),
            ..project_load_options(places.as_ref(), root)
        }
    };
    let loaded = match am_config::load_in(root, opts) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(err, "agentic validate: {e}");
            return EXIT_USAGE;
        }
    };
    for finding in user_findings.iter().chain(&loaded.findings) {
        let _ = writeln!(out, "{finding}");
    }
    let user_errors = user_findings
        .iter()
        .filter(|f| f.severity == am_config::Severity::Error)
        .count();
    let errors = loaded.error_count() + user_errors;
    let warnings = loaded.warning_count() + user_findings.len() - user_errors;
    let summary = format!(
        "{}, {}",
        plural(errors, "error"),
        plural(warnings, "warning")
    );
    match loaded.snapshot.as_ref().filter(|_| user_errors == 0) {
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

const AUDIT_USAGE: &str =
    "usage: agentic audit export <project folder> [--out <file>] | agentic audit verify <file>";

fn audit_usage(err: &mut dyn Write, why: &str) -> i32 {
    let _ = writeln!(err, "{AUDIT_USAGE}");
    let _ = writeln!(err, "agentic: {why}");
    EXIT_USAGE
}

fn entries(n: u64) -> String {
    if n == 1 {
        "1 entry".to_string()
    } else {
        format!("{n} entries")
    }
}

/// `agentic audit export|verify …`: `args` are the words after `audit`.
/// Returns the exit code: 0 done (export written, or the chain is OK), 1 a
/// failure or a broken chain (named by line and entry), 2 bad usage.
pub fn audit(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        Some("export") => audit_export(&args[1..], out, err),
        Some("verify") => audit_verify(&args[1..], out, err),
        Some(other) => audit_usage(err, &format!("unknown audit command `{other}`")),
        None => audit_usage(err, "audit needs `export` or `verify`"),
    }
}

/// `agentic audit export <project> [--out <file>]`: finds the project's
/// runtime folder read-only (story 1.13: refused in words without a
/// `company.yaml`, a `project_id`, a runtime folder on this Mac, or with an
/// unmoved legacy folder) and reads its `state.db` through a read-only
/// connection (no project lock, so it works while `serve` holds the
/// project; a database newer than this app is refused). Without `--out` the
/// JSON Lines go to stdout and the summary to stderr; with it, the export
/// goes to an owner-only temporary file beside the target, renamed over it
/// only when the export succeeded, and the summary goes to stdout. A target
/// inside the runtime folder is refused as bad usage.
fn audit_export(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut project: Option<&str> = None;
    let mut file: Option<&str> = None;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--out" => match words.next() {
                Some(f) if !f.starts_with('-') => file = Some(f),
                _ => return audit_usage(err, "--out needs the file to write"),
            },
            w if w.starts_with('-') => {
                return audit_usage(err, &format!("unknown option `{w}`"));
            }
            w if project.is_none() => project = Some(w),
            _ => return audit_usage(err, "audit export takes one project folder"),
        }
    }
    let Some(project) = project else {
        return audit_usage(err, "audit export needs the project folder");
    };
    let layout = match crate::runtime::Places::from_env()
        .and_then(|places| crate::runtime::find_runtime(&places, Path::new(project)))
    {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(
                err,
                "agentic: cannot export the audit log of {project}: {e}"
            );
            return EXIT_ERRORS;
        }
    };
    if let Some(file) = file
        && inside(layout.dir(), Path::new(file))
    {
        return audit_usage(
            err,
            &format!(
                "--out {file} is inside the project's runtime folder {}; write the export elsewhere",
                layout.dir().display()
            ),
        );
    }
    let db = layout.state_db();
    if !db.is_file() {
        let _ = writeln!(
            err,
            "agentic: no project database at {}; is {project} a project folder?",
            db.display()
        );
        return EXIT_ERRORS;
    }
    let reader = match am_store::Reader::open(&db) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "agentic: cannot read {}: {e}", db.display());
            return EXIT_ERRORS;
        }
    };
    match file {
        None => match reader.export_audit_jsonl(out) {
            Ok((count, head)) => {
                let _ = writeln!(
                    err,
                    "agentic: exported {} from {project}, head hash {head}",
                    entries(count)
                );
                EXIT_OK
            }
            Err(e) => {
                let _ = writeln!(
                    err,
                    "agentic: cannot export the audit log of {project}: {e}"
                );
                EXIT_ERRORS
            }
        },
        Some(file) => match export_to_file(&reader, Path::new(file)) {
            Ok((count, head)) => {
                let _ = writeln!(
                    out,
                    "agentic: exported {} from {project} to {file}, head hash {head}",
                    entries(count)
                );
                EXIT_OK
            }
            Err(e) => {
                let _ = writeln!(
                    err,
                    "agentic: cannot export the audit log of {project}: {e}"
                );
                EXIT_ERRORS
            }
        },
    }
}

/// The folder `path` sits in (`.` for a bare file name).
fn folder_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// `file`, or the folder it would be written in, is inside `dir` (compared
/// as canonical paths, so relative paths and links count).
fn inside(dir: &Path, file: &Path) -> bool {
    let Ok(dir) = dir.canonicalize() else {
        return false;
    };
    let inside = |p: &Path| p.canonicalize().is_ok_and(|c| c.starts_with(&dir));
    inside(file) || inside(folder_of(file))
}

/// Export into a new owner-only temporary file beside `target`, then
/// rename it over `target`; on any failure the temporary file is removed
/// and `target` is left as it was.
fn export_to_file(reader: &am_store::Reader, target: &Path) -> Result<(u64, String), String> {
    let name = target
        .file_name()
        .ok_or_else(|| format!("{} is not a file name", target.display()))?;
    let tmp = folder_of(target).join(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let f = create_private(&tmp)?;
        let mut w = std::io::BufWriter::new(f);
        let summary = reader
            .export_audit_jsonl(&mut w)
            .map_err(|e| e.to_string())?;
        let f = w
            .into_inner()
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, target)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        Ok(summary)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// A new file (never an existing one), owner-only (0600) on Unix.
fn create_private(path: &Path) -> Result<std::fs::File, String> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// `agentic audit verify <file>`: recomputes the chain of an export with no
/// database. OK prints the count and head hash (exit 0); a break names its
/// line and entry (exit 1).
fn audit_verify(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let file = match args {
        [f] if !f.starts_with('-') => f,
        [] => return audit_usage(err, "audit verify needs the export file"),
        _ => return audit_usage(err, "audit verify takes one export file"),
    };
    let input = match std::fs::File::open(file) {
        Ok(f) => std::io::BufReader::new(f),
        Err(e) => {
            let _ = writeln!(err, "agentic: cannot read {file}: {e}");
            return EXIT_ERRORS;
        }
    };
    match am_store::verify_jsonl(input) {
        Ok(ok) => {
            let _ = writeln!(
                out,
                "agentic: {file}: audit chain OK, {}, head hash {}",
                entries(ok.entries),
                ok.head
            );
            EXIT_OK
        }
        Err(brk) => {
            let _ = writeln!(out, "agentic: {file}: audit chain broken at {brk}");
            EXIT_ERRORS
        }
    }
}
