//! Repo automation (story 1.2): `check-deps` (AD-4 dependency direction),
//! `denylist` (AD-23 department vocabulary) and `check-forbid` (no `unsafe`);
//! story 1.13 adds `runtime-paths` (AD-32: runtime paths named only by
//! `am_domain::RuntimeLayout`); story 2.2 adds `fixed-hosts` (AD-12: every
//! fixed host of the shipped source is on one named list) and `seam-guard`
//! (shipped code never names am-net's test-only constructor; story 3.2 adds
//! am-server's test-only fixture route, `router_with_fixture`; story 3.3
//! adds am-engine's tool seam, `run_plan_with_tools`, and `check-deps`'s
//! call-site rule: only am-engine calls `TrustedInput`'s constructors).
//! Story 3.25 adds `register`: `docs/deferred-register.md` checked against
//! the ticket tree, its `(assumed)` entries listed as warnings (the normal
//! mode, in `just check`); its release mode (`--release --gate <epic>.<id>`)
//! is for the release gate stories 4.13 and 11.8.
//! Later stories add `gen-types` and `bundle-services`. Every check is
//! offline; `just check` runs them.
#![forbid(unsafe_code)]

mod denylist;
mod deps;
mod fixed_hosts;
mod forbid;
mod register;
mod runtime_paths;
mod seam_guard;
mod shipped;
mod stories;
mod walk;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <task> [options]
  check-deps   [--metadata <cargo-metadata.json>] [--root <dir>]
                                                    AD-4 dependency matrix; AD-10 TrustedInput built only in am-engine
  denylist     [--root <dir>] [--list <file>]       AD-23 vocabulary scan
  check-forbid [--root <dir>]                       every crate forbids unsafe
  runtime-paths [--root <dir>]                      AD-32 runtime paths only in RuntimeLayout
  fixed-hosts  [--root <dir>]                       AD-12 fixed hosts only from the named list
  seam-guard   [--root <dir>]                       test-only seams (NetEgress::with_seams, router_with_fixture) only in tests
  register     [--root <dir>] [--register <file>] [--tickets <dir>] [--release --gate <epic>.<id>]
                                                    deferred register checked against the ticket tree; --release: the gate's refusals";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(task) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let Some(takes) = flags(task) else {
        eprintln!("xtask: unknown task `{task}`\n{USAGE}");
        return ExitCode::from(2);
    };
    let opts = match Options::parse(task, takes, &args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("xtask {task}: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match task.as_str() {
        "check-deps" => deps::run(
            opts.metadata.as_deref(),
            &opts.root.clone().unwrap_or_else(repo_root),
        ),
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
        "fixed-hosts" => fixed_hosts::run(&opts.root.clone().unwrap_or_else(repo_root)),
        "seam-guard" => seam_guard::run(&opts.root.clone().unwrap_or_else(repo_root)),
        "register" => {
            let root = opts.root.clone().unwrap_or_else(repo_root);
            let release = match register::gate(opts.release, opts.gate.as_deref()) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("xtask {task}: {e}\n{USAGE}");
                    return ExitCode::from(2);
                }
            };
            register::run(&register::Inputs {
                register: opts
                    .register
                    .clone()
                    .unwrap_or_else(|| root.join("docs/deferred-register.md")),
                tickets: opts.tickets.clone().unwrap_or_else(|| {
                    root.join("_bmad-output/initiative-agentic-manager/tickets")
                }),
                display_root: root,
                release,
            })
        }
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

/// The flags each task takes (story 3.25, review R4); any other flag is
/// refused with exit 2 before the task runs. `None`: no such task.
fn flags(task: &str) -> Option<&'static [&'static str]> {
    Some(match task {
        "check-deps" => &["--metadata", "--root"],
        "denylist" => &["--root", "--list"],
        "check-forbid" | "runtime-paths" | "fixed-hosts" | "seam-guard" => &["--root"],
        "register" => &["--root", "--register", "--tickets", "--release", "--gate"],
        _ => return None,
    })
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
    register: Option<PathBuf>,
    tickets: Option<PathBuf>,
    release: bool,
    gate: Option<String>,
}

impl Options {
    fn parse(task: &str, takes: &[&str], args: &[String]) -> Result<Self, String> {
        let mut o = Options::default();
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            if flag.starts_with("--") && !takes.contains(&flag.as_str()) {
                let takes = if takes.is_empty() {
                    "none".to_string()
                } else {
                    takes.join(", ")
                };
                return Err(format!(
                    "`{flag}` is not an option of `{task}`; it takes: {takes}"
                ));
            }
            let mut value = || {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag.as_str() {
                "--metadata" => o.metadata = Some(value()?.into()),
                "--root" => o.root = Some(value()?.into()),
                "--list" => o.list = Some(value()?.into()),
                "--register" => o.register = Some(value()?.into()),
                "--tickets" => o.tickets = Some(value()?.into()),
                "--gate" => o.gate = Some(value()?),
                "--release" => o.release = true,
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
