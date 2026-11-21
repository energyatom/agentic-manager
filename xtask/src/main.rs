//! Repo automation (story 1.2): `check-deps` (AD-4 dependency direction),
//! `denylist` (AD-23 department vocabulary) and `check-forbid` (no `unsafe`);
//! story 1.13 adds `runtime-paths` (AD-32: runtime paths named only by
//! `am_domain::RuntimeLayout`).
//! Later stories add `gen-types` and `bundle-services`. All three checks are
//! offline; `just check` runs them.
#![forbid(unsafe_code)]

mod denylist;
mod deps;
mod forbid;
mod runtime_paths;
mod walk;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <task> [options]
  check-deps   [--metadata <cargo-metadata.json>]   AD-4 dependency matrix
  denylist     [--root <dir>] [--list <file>]       AD-23 vocabulary scan
  check-forbid [--root <dir>]                       every crate forbids unsafe
  runtime-paths [--root <dir>]                      AD-32 runtime paths only in RuntimeLayout";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(task) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let opts = match Options::parse(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("xtask {task}: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match task.as_str() {
        "check-deps" => deps::run(opts.metadata.as_deref(), &repo_root()),
        "denylist" => {
            let root = opts.root.clone().unwrap_or_else(repo_root);
            let list = opts
                .list
                .clone()
                .unwrap_or_else(|| repo_root().join("xtask/denylist.txt"));
            denylist::run(&root, &list)
        }
        "check-forbid" => forbid::run(&opts.root.clone().unwrap_or_else(repo_root)),
        "runtime-paths" => runtime_paths::run(&opts.root.clone().unwrap_or_else(repo_root)),
        other => {
            eprintln!("xtask: unknown task `{other}`\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(summary) => {
            println!("{task}: {summary}");
            ExitCode::SUCCESS
        }
        Err(Failure::Findings(findings)) => {
            for f in &findings {
                eprintln!("{f}");
            }
            eprintln!("{task}: FAILED with {} finding(s)", findings.len());
            ExitCode::FAILURE
        }
        Err(Failure::Setup(e)) => {
            eprintln!("{task}: {e}");
            ExitCode::from(2)
        }
    }
}

/// A check either found violations or could not run.
pub enum Failure {
    Findings(Vec<String>),
    Setup(String),
}

#[derive(Default)]
struct Options {
    metadata: Option<PathBuf>,
    root: Option<PathBuf>,
    list: Option<PathBuf>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut o = Options::default();
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            let mut value = || {
                it.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag.as_str() {
                "--metadata" => o.metadata = Some(value()?),
                "--root" => o.root = Some(value()?),
                "--list" => o.list = Some(value()?),
                other => return Err(format!("unknown option `{other}`")),
            }
        }
        Ok(o)
    }
}

/// The repository root: xtask's parent directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
