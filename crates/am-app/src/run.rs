//! `agentic run <workflow>` and `agentic status` (epic 3 story 1).
//!
//! `run` starts one run of a configured workflow and drives it to its end
//! with am-engine's driver ([`am_engine::run`]): one model turn per step,
//! every transition committed before the effect it authorises (AD-8). A
//! manual run start is a CLI-class decision (AD-15), so its checks run in
//! this order, and the first that fails is what the owner reads:
//!
//! 1. usage (exit 2);
//! 2. the terminal: stdin and stdout are both a real interactive terminal
//!    (the `Terminal` port; [`crate::terminal::interactive`] in the
//!    binary). Nothing is read from it: the command asks no question;
//! 3. no stdio MCP server and no Source with `render: true` is enabled
//!    (children that run as the owner, AD-13); a `tools.yaml` or Source
//!    file with an error is refused too, pointing at `agentic validate`;
//! 4. the project opens as `agentic serve` opens it (AD-32: the store under
//!    its lock, migrations, the marker) and is held for the whole run. No
//!    question is asked: an open that needs `--confirm-move` or
//!    `--take-over` is refused in words pointing at `agentic serve`;
//! 5. no config file the app has written (one with a History entry: the
//!    `company.yaml` it gave the id to, an Accounts file it edited) differs
//!    on disk from what the app last wrote (such a change waits for
//!    Reload, AD-16). A file the app never wrote has no History entry and
//!    is not judged: a hand edit of such a workflow or seat file is not
//!    refused here, and the run is built from it as it is;
//! 6. no config migration waits for the owner's accept, and the config has
//!    no error (the snapshot is loaded once, here; the run pins its hash
//!    and its plan is built from it only, AD-16);
//! 7. check 3 again, on the snapshot the run pins;
//! 8. the workflow exists, and no step declares what this version cannot
//!    honour yet (a run-graph key such as `review`, `handoff` or `seats`,
//!    or an `output_schema`): such a step is never run as a plain turn;
//! 9. every step has a seat with a model, and the context's provider
//!    factory gives a provider for it.
//!
//! Only then is the run created, by the owner (`Actor::Human`); the
//! engine's transitions are `System(engine)`'s. The command prints `run
//! <id> <status>` and exits 0 when the run succeeded, 1 otherwise (the
//! reason on stderr). A run the store stopped after it was created is
//! named the same way, with the status its journal holds (which also
//! decides the exit code), the failed step's reason if there was one, and
//! the store's error.
//!
//! The binary's factory ([`unwired_providers`]) wires no provider yet and
//! refuses every model in words; tests pass am-testkit's `FakeLlm` through
//! [`RunContext`]. No env var, flag or feature swaps a provider in here.
//!
//! `status` lists the project's runs, newest first, with their steps. It
//! reads without the project's lock ([`crate::runtime::find_runtime`], a
//! read-only connection), so it works while a run or `serve` holds the
//! project, and it has no terminal rule.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{ConfigSnapshot, ModelRef, Severity};
use am_domain::{Instruction, LlmProvider, ProjectIdentity, Terminal};
use am_engine::run::{RunError, RunOutcome, RunPlan, StepPlan, run_plan};
use am_store::{RunRow, StepRow};
use tokio_util::sync::CancellationToken;

use crate::cli::{EXIT_ERRORS, EXIT_OK, EXIT_USAGE};
use crate::runtime::{OpenRequest, Places, Prompt, owner};

const RUN_USAGE: &str = "usage: agentic run <workflow> [--project <folder>]  (--project defaults to the current folder)";
const STATUS_USAGE: &str =
    "usage: agentic status [--project <folder>]  (--project defaults to the current folder)";

/// A step's output limit when it names no `max_tokens`.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4096;

/// What started a run made by `agentic run`.
const ORIGIN_MANUAL: &str = "manual";

/// What a refused `run` did not do.
const NOT_STARTED: &str = "No run was started";

/// A seat's model to the provider that serves it, or a refusal in words.
pub type Providers = Arc<dyn Fn(&ModelRef) -> Result<Arc<dyn LlmProvider>, String> + Send + Sync>;

/// What `agentic run` runs with. `main` builds it with the owner's real
/// terminal, the OS identity adapter and [`unwired_providers`]; tests pass
/// am-testkit's fakes.
pub struct RunContext {
    pub places: Places,
    /// Supplies the open's identity checks and the host recorded in the
    /// store's lock (AD-32).
    pub identity: Arc<dyn ProjectIdentity>,
    /// Asked only whether it is interactive (AD-15); nothing is read.
    pub terminal: Box<dyn Terminal>,
    pub providers: Providers,
}

/// The provider factory of the `agentic` binary: it wires no provider yet
/// and refuses every model in words, so the command stops before any run is
/// created.
pub fn unwired_providers() -> Providers {
    Arc::new(|model: &ModelRef| {
        Err(format!(
            "no model provider is wired into `agentic run` yet, so it cannot call the model `{}` on the Account `{}`; running a workflow from the command line arrives in a later version",
            model.model, model.account
        ))
    })
}

/// The words after the command: the positional ones and `--project`.
fn parse(args: &[String]) -> Result<(Vec<String>, PathBuf), String> {
    let mut positional = Vec::new();
    let mut project: Option<String> = None;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--project" => match words.next() {
                Some(value) if !value.starts_with("--") && !value.is_empty() => {
                    if project.replace(value.clone()).is_some() {
                        return Err("--project is given twice".into());
                    }
                }
                _ => return Err("--project needs the project folder".into()),
            },
            w if w.starts_with('-') => return Err(format!("unknown option `{w}`")),
            _ => positional.push(word.clone()),
        }
    }
    Ok((
        positional,
        PathBuf::from(project.unwrap_or_else(|| ".".into())),
    ))
}

fn usage(err: &mut dyn Write, usage: &str, why: &str) -> i32 {
    let _ = writeln!(err, "{usage}");
    let _ = writeln!(err, "agentic: {why}");
    EXIT_USAGE
}

// ------------------------------------------------------------------ run

/// The open-time prompt of `agentic run`: it asks nothing. The open
/// consults it only for `--confirm-move` and `--take-over`, which this
/// command never passes.
struct NoQuestion;

impl Prompt for NoQuestion {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

/// The refusal while a child that runs as the owner is enabled.
fn owner_child_words(why: &str) -> String {
    format!(
        "`agentic run` cannot start a run from the command line while {why}: a program that runs as you could type here. {NOT_STARTED}; remove or disable it first"
    )
}

/// Check 3, before the open: no child that runs as the owner is enabled in
/// the project's files (AD-13, AD-15). A `tools.yaml` or Source file with
/// an error cannot say what it enables, so it is refused too (fails
/// closed), in its own words: the file has an error, and `agentic validate`
/// names it.
fn no_owner_child(project: &Path) -> Result<(), String> {
    let found = am_config::owner_children(project);
    if !found.enabled.is_empty() {
        return Err(owner_child_words(&found.words().unwrap_or_default()));
    }
    if found.broken.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} {}, so `agentic run` cannot tell whether the project enables a program that runs as you, and starts no run from the command line. {NOT_STARTED}; run `agentic validate {}` to see each error with its fix",
        found.broken.join(", "),
        if found.broken.len() == 1 {
            "has an error"
        } else {
            "have errors"
        },
        project.display()
    ))
}

/// Check 7: check 3 on the snapshot the run pins, so what is refused is
/// what the run would be built from, whatever the files hold by now.
fn no_pinned_owner_child(snapshot: &ConfigSnapshot) -> Result<(), String> {
    let enabled = snapshot.owner_children();
    if enabled.is_empty() {
        return Ok(());
    }
    Err(owner_child_words(&enabled.join("; ")))
}

/// Check 5: no config file the app has written differs from what its last
/// History entry wrote (an outside change that no Reload has taken in yet,
/// AD-16), in the words of story 2.1's Account edit. Only files with a
/// History entry are judged (`am_config::outside_changes`): a file the app
/// never wrote passes, whatever was edited in it by hand.
fn no_outside_change(root: &Path, store: &am_store::Store) -> Result<(), String> {
    let history =
        am_store::StoreConfigHistory::new(store).map_err(|e| format!("{e}. {NOT_STARTED}"))?;
    let changed =
        am_config::outside_changes(root, &history).map_err(|e| format!("{e}. {NOT_STARTED}"))?;
    if changed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} changed on disk outside the app since the app last wrote it, so no run is built from that change. {NOT_STARTED}. Put the file back as the app last wrote it (undo your change), then run this command again; Reload, which takes such a change in, arrives with the app's UI",
        changed.join(", ")
    ))
}

/// What a step declares that this version of `agentic run` cannot honour
/// yet: its run-graph keys (AD-40..AD-42) and an `output_schema`, by name.
fn not_honoured(step: &am_config::Step) -> Vec<&str> {
    let mut keys: Vec<&str> = step.run_graph.iter().map(String::as_str).collect();
    if step.output_schema.is_some() {
        keys.push("output_schema");
    }
    keys
}

/// The engine's plan for `workflow`, built from `snapshot` only: per step,
/// the seat's instructions as the system part, the step's instruction, the
/// seat's model and the step's output limit, on the provider the factory
/// gives for that model. A workflow with a step that declares a run-graph
/// key or an `output_schema` gives no plan (see [`not_honoured`]).
fn plan_of(
    snapshot: &ConfigSnapshot,
    workflow: &str,
    providers: &Providers,
) -> Result<RunPlan, String> {
    let Some(wf) = snapshot.workflows().get(workflow) else {
        let known: Vec<&str> = snapshot.workflows().keys().map(String::as_str).collect();
        let known = if known.is_empty() {
            "it has no workflows".to_string()
        } else {
            format!("its workflows are: {}", known.join(", "))
        };
        return Err(format!(
            "this project has no workflow `{workflow}`; {known}. {NOT_STARTED}"
        ));
    };
    // Before any provider is asked for: a step this version would run as
    // one plain turn, with its gate, hand-off, fan-out or schema skipped.
    for step in &wf.steps {
        let keys = not_honoured(step);
        if !keys.is_empty() {
            let keys: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
            return Err(format!(
                "step `{}` of the workflow `{workflow}` declares {}, which this version of `agentic run` cannot honour yet: it runs each step as one plain model turn and would skip it without saying so. {NOT_STARTED}",
                step.id,
                keys.join(", ")
            ));
        }
    }
    let mut steps = Vec::new();
    for step in &wf.steps {
        let Some(seat_id) = step.seat.as_deref() else {
            return Err(format!(
                "step `{}` of the workflow `{workflow}` has no `seat:` of its own, and this version of `agentic run` runs a step only on its one seat. {NOT_STARTED}",
                step.id
            ));
        };
        let Some(seat) = snapshot.seats().get(seat_id) else {
            return Err(format!(
                "step `{}` of the workflow `{workflow}` names the seat `{seat_id}`, which this project does not have. {NOT_STARTED}",
                step.id
            ));
        };
        let Some(model) = &seat.model else {
            return Err(format!(
                "step `{}` of the workflow `{workflow}` runs on the seat `{seat_id}`, which has no model; set `model:` (its `account` and `model`) in seats/{seat_id}.md. {NOT_STARTED}",
                step.id
            ));
        };
        let provider = providers(model).map_err(|why| format!("{why}. {NOT_STARTED}"))?;
        // A seat with no instructions sends no system part (never an empty one).
        let system = if seat.instructions.trim().is_empty() {
            Vec::new()
        } else {
            vec![Instruction::authored(seat.instructions.clone())]
        };
        steps.push(StepPlan {
            key: step.id.clone(),
            system,
            instruction: Instruction::authored(step.instruction.clone()),
            model: model.model.clone(),
            max_output_tokens: step.max_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
            provider,
        });
    }
    Ok(RunPlan {
        workflow: workflow.to_string(),
        config_hash: snapshot.hash().to_string(),
        steps,
    })
}

/// The project's config snapshot (check 5), or why no run starts on it.
fn snapshot_of(places: &Places, root: &Path, project: &Path) -> Result<ConfigSnapshot, String> {
    let offers = am_config::migration_offers(root).map_err(|e| format!("{e}. {NOT_STARTED}"))?;
    if !offers.is_empty() {
        let files: Vec<String> = offers
            .iter()
            .map(|o| format!("{} from schema_version {} to {}", o.file, o.from, o.to))
            .collect();
        return Err(format!(
            "the project waits for a config migration ({}), and nothing runs on config you have not accepted. {NOT_STARTED}; `agentic validate {}` names each file and its fix",
            files.join(", "),
            project.display()
        ));
    }
    let loaded = am_config::load_in(root, crate::cli::project_load_options(Some(places), root))
        .map_err(|e| format!("{e}. {NOT_STARTED}"))?;
    match loaded.snapshot {
        Some(snapshot) => Ok(snapshot),
        None => {
            let errors: Vec<&am_config::Finding> = loaded
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .collect();
            let first = errors
                .first()
                .map(|f| format!(" (the first: {}:{}: {})", f.file, f.line, f.message))
                .unwrap_or_default();
            Err(format!(
                "the project's config has {}{first}, so no run can be built from it. {NOT_STARTED}; run `agentic validate {}` to see each error with its fix",
                if errors.len() == 1 {
                    "1 error".to_string()
                } else {
                    format!("{} errors", errors.len())
                },
                project.display()
            ))
        }
    }
}

/// Why `run` stopped short of an outcome.
#[derive(Debug)]
enum Stop {
    /// A refusal, in words: no run was created.
    Refused(String),
    /// The engine's error: a run that could not be created, or one the
    /// store stopped after it was created.
    Engine(RunError),
}

impl From<String> for Stop {
    fn from(words: String) -> Stop {
        Stop::Refused(words)
    }
}

/// Checks 2 to 9, then the run. The open project (its marker, then the
/// store's lock) is dropped when this returns.
async fn start(
    workflow: &str,
    project: &Path,
    ctx: &RunContext,
    out: &mut dyn Write,
) -> Result<RunOutcome, Stop> {
    if !ctx.terminal.interactive() {
        return Err(Stop::Refused(format!(
            "`agentic run` runs only in an interactive terminal: stdin and stdout must both be a terminal, not a pipe, a file or another program, because starting a run is your decision and only you make it. {NOT_STARTED} and the project was not opened; run the command yourself in a terminal"
        )));
    }
    if !project.is_dir() {
        return Err(Stop::Refused(format!(
            "the project folder {} does not exist. {NOT_STARTED}",
            project.display()
        )));
    }
    no_owner_child(project)?;
    let req = OpenRequest {
        root: project.to_path_buf(),
        confirm_move: false,
        take_over: false,
    };
    let opened =
        crate::runtime::open_project(&ctx.places, ctx.identity.as_ref(), &req, &mut NoQuestion)
            .map_err(|e| {
                format!(
                    "cannot open the project {}: {e}. {NOT_STARTED}",
                    project.display()
                )
            })?;
    for line in &opened.lines {
        let _ = writeln!(out, "agentic: project {}: {line}", project.display());
    }
    let root = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    no_outside_change(&root, &opened.store)?;
    let snapshot = snapshot_of(&ctx.places, &root, project)?;
    // The owner's files may have changed since check 3 (the open can wait
    // on a migration): what the pinned snapshot holds decides.
    no_pinned_owner_child(&snapshot)?;
    let plan = plan_of(&snapshot, workflow, &ctx.providers)?;
    run_plan(
        &opened.store,
        owner(),
        ORIGIN_MANUAL,
        &plan,
        CancellationToken::new(),
    )
    .await
    .map_err(Stop::Engine)
}

/// Prints how the command ended and gives its exit code: `run <id>
/// <status>` on `out` for every run that was created (also one the store
/// stopped), the reason or refusal on `err`.
fn report(done: Result<RunOutcome, Stop>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match done {
        Ok(outcome) => {
            let _ = writeln!(out, "run {} {}", outcome.run_id, outcome.status);
            if let Some(reason) = &outcome.reason {
                let _ = writeln!(
                    err,
                    "agentic: run {} {}: {reason}",
                    outcome.run_id, outcome.status
                );
            }
            if outcome.status == "succeeded" {
                EXIT_OK
            } else {
                EXIT_ERRORS
            }
        }
        Err(Stop::Engine(RunError::Stopped {
            run_id,
            status,
            reason,
            source,
        })) => {
            let _ = writeln!(out, "run {run_id} {status}");
            if let Some(reason) = &reason {
                let _ = writeln!(err, "agentic: run {run_id}: {reason}");
            }
            let _ = writeln!(
                err,
                "agentic: run {run_id}: the store refused a command while the run was journaled: {source}. The run is recorded as {status}; `agentic status` shows it"
            );
            // The exit code follows what the journal holds.
            if status == "succeeded" {
                EXIT_OK
            } else {
                EXIT_ERRORS
            }
        }
        Err(Stop::Engine(e)) => {
            let _ = writeln!(err, "agentic: {e}. {NOT_STARTED}");
            EXIT_ERRORS
        }
        Err(Stop::Refused(words)) => {
            let _ = writeln!(err, "agentic: {words}");
            EXIT_ERRORS
        }
    }
}

/// `agentic run <workflow> [--project <folder>]`: `args` are the words
/// after `run`. Returns the exit code: 0 when the run succeeded, 2 for bad
/// usage (with `usage: agentic run` on `err`), 1 for a failed run and for
/// every refusal, each in words on `err`. A run that was created prints
/// `run <id> <status>` on `out`.
pub async fn run(
    args: &[String],
    ctx: RunContext,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (workflow, project) = match parse(args) {
        Ok((positional, project)) => match positional.as_slice() {
            [workflow] => (workflow.clone(), project),
            [] => return usage(err, RUN_USAGE, "run needs the workflow's id"),
            _ => return usage(err, RUN_USAGE, "run takes one workflow"),
        },
        Err(why) => return usage(err, RUN_USAGE, &why),
    };
    let done = start(&workflow, &project, &ctx, out).await;
    report(done, out, err)
}

// ------------------------------------------------------------------ status

/// Milliseconds from `from` to `to` (two store timestamps); `None` when
/// either is not one.
fn millis_between(from: &str, to: &str) -> Option<i64> {
    let from: jiff::Timestamp = from.parse().ok()?;
    let to: jiff::Timestamp = to.parse().ok()?;
    Some((to.as_millisecond() - from.as_millisecond()).max(0))
}

/// A duration as `status` prints it: seconds with milliseconds, `1.042s`.
fn seconds(millis: i64) -> String {
    format!("{}.{:03}s", millis / 1000, millis % 1000)
}

/// How long something took, from the store's own timestamps: the time
/// between its start and its finish, `running` while it has started and
/// not finished, `-` when the journal holds no times for it (a step that
/// never started, a row from before the times were kept).
fn duration(started: Option<&str>, finished: Option<&str>, ended: bool) -> String {
    match (started, finished) {
        (Some(from), Some(to)) => millis_between(from, to)
            .map(seconds)
            .unwrap_or_else(|| "-".into()),
        (Some(_), None) if !ended => "running".into(),
        _ => "-".into(),
    }
}

fn ended(status: &str) -> bool {
    matches!(status, "succeeded" | "failed" | "cancelled")
}

fn run_line(run: &RunRow) -> String {
    let snapshot = match &run.config_hash {
        Some(hash) => hash.chars().take(12).collect::<String>(),
        None => "-".into(),
    };
    format!(
        "{}  {}  {}  snapshot {snapshot}  {}",
        run.id,
        run.workflow,
        run.status,
        duration(
            Some(&run.created_at),
            run.finished_at.as_deref(),
            ended(&run.status)
        )
    )
}

fn step_line(step: &StepRow) -> String {
    format!(
        "    {}  {}  {}",
        step.step_key,
        step.status,
        duration(
            step.started_at.as_deref(),
            step.finished_at.as_deref(),
            ended(&step.status)
        )
    )
}

/// The runs of the project at `project`, newest first, each with its steps.
fn read_runs(places: &Places, project: &Path) -> Result<Vec<(RunRow, Vec<StepRow>)>, String> {
    let layout = crate::runtime::find_runtime(places, project).map_err(|e| e.to_string())?;
    let db = layout.state_db();
    if !db.is_file() {
        return Err(format!(
            "no project database at {}; open the project once with `agentic serve --project {}`",
            db.display(),
            project.display()
        ));
    }
    am_store::check_database(&db).map_err(|e| e.to_string())?;
    let reader = am_store::Reader::open(&db).map_err(|e| e.to_string())?;
    let found = reader.schema_version().map_err(|e| e.to_string())?;
    if found < am_store::SCHEMA_VERSION {
        return Err(format!(
            "the project's database is at version {found}, older than this app's version {}, and `agentic status` never migrates it; open the project once with this version (`agentic serve --project {}` backs the database up and migrates it), then run `agentic status` again",
            am_store::SCHEMA_VERSION,
            project.display()
        ));
    }
    let mut all = Vec::new();
    for run in reader.runs().map_err(|e| e.to_string())? {
        let steps = reader.steps(&run.id).map_err(|e| e.to_string())?;
        all.push((run, steps));
    }
    Ok(all)
}

/// `agentic status [--project <folder>]`: `args` are the words after
/// `status`. Prints each run, newest first (id, workflow, status, the first
/// 12 characters of its snapshot hash, duration), and under it each step
/// (key, status, duration). Reads only, without the project's lock and with
/// any stdin. Returns 0 (also with no runs), 2 for bad usage, 1 when the
/// project has no runtime folder or database on this Mac, or one this
/// version has not migrated yet, each in words on `err`.
pub fn status(args: &[String], places: &Places, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let project = match parse(args) {
        Ok((positional, project)) if positional.is_empty() => project,
        Ok(_) => return usage(err, STATUS_USAGE, "status takes no argument but --project"),
        Err(why) => return usage(err, STATUS_USAGE, &why),
    };
    let runs = match read_runs(places, &project) {
        Ok(runs) => runs,
        Err(why) => {
            let _ = writeln!(
                err,
                "agentic: cannot read the runs of {}: {why}",
                project.display()
            );
            return EXIT_ERRORS;
        }
    };
    if runs.is_empty() {
        let _ = writeln!(out, "agentic: no runs in {} yet", project.display());
        return EXIT_OK;
    }
    for (run, steps) in &runs {
        let _ = writeln!(out, "{}", run_line(run));
        for step in steps {
            let _ = writeln!(out, "{}", step_line(step));
        }
    }
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn durations_come_from_the_two_timestamps() {
        let (from, to) = ("2026-11-22T10:00:00.000Z", "2026-11-22T10:00:01.042Z");
        assert_eq!(duration(Some(from), Some(to), true), "1.042s");
        assert_eq!(duration(Some(from), Some(from), true), "0.000s");
        // A clock that went back never prints a negative time.
        assert_eq!(duration(Some(to), Some(from), true), "0.000s");
        assert_eq!(seconds(61_005), "61.005s");
    }

    #[test]
    fn an_unfinished_time_is_running_only_while_the_status_says_so() {
        let from = "2026-11-22T10:00:00.000Z";
        assert_eq!(duration(Some(from), None, false), "running");
        // A row from before the times were kept: ended, with no finish time.
        assert_eq!(duration(Some(from), None, true), "-");
        // A step that never started.
        assert_eq!(duration(None, None, false), "-");
        assert_eq!(duration(Some("not a time"), Some(from), true), "-");
    }

    /// A run the store stopped after it was created is still named: `run
    /// <id> <status>` on stdout with the status the journal holds (`failed`
    /// when the engine could finish it, `running` when it could not), and
    /// the store's error with a pointer at `agentic status` on stderr.
    #[test]
    fn a_run_the_store_stopped_is_named_with_its_state() {
        for status in ["failed", "running"] {
            let stopped = Stop::Engine(am_engine::run::RunError::Stopped {
                run_id: "run-1".into(),
                status: status.into(),
                reason: None,
                source: Box::new(am_store::StoreError::Invalid("call c1 is not open".into())),
            });
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = report(Err(stopped), &mut out, &mut err);
            let (out, err) = (
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            );
            assert_eq!(code, EXIT_ERRORS);
            assert_eq!(out, format!("run run-1 {status}\n"));
            assert!(err.contains("call c1 is not open"), "{err}");
            assert!(err.contains("agentic status"), "{err}");
        }
    }

    /// A stopped run whose step had failed for a reason of its own prints
    /// that reason beside the store's error; and the exit code follows the
    /// status the journal holds (0 only for a run recorded `succeeded`).
    #[test]
    fn a_stopped_run_prints_the_steps_reason_and_exits_by_its_recorded_status() {
        let stopped = |status: &str, reason: Option<&str>| {
            Stop::Engine(am_engine::run::RunError::Stopped {
                run_id: "run-1".into(),
                status: status.into(),
                reason: reason.map(str::to_string),
                source: Box::new(am_store::StoreError::Invalid("the disk is full".into())),
            })
        };
        let said = |stop: Stop| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = report(Err(stop), &mut out, &mut err);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };

        let why = "step `only` failed: the model call failed: the provider answered HTTP 500";
        let (code, out, err) = said(stopped("failed", Some(why)));
        assert_eq!(code, EXIT_ERRORS);
        assert_eq!(out, "run run-1 failed\n");
        assert!(err.contains(why), "the step's reason: {err}");
        assert!(err.contains("the disk is full"), "the store's error: {err}");
        assert!(err.contains("agentic status"), "{err}");

        let (code, out, err) = said(stopped("succeeded", None));
        assert_eq!(code, EXIT_OK, "the run is recorded as succeeded");
        assert_eq!(out, "run run-1 succeeded\n");
        assert!(err.contains("the disk is full"), "still said: {err}");
    }

    #[test]
    fn the_project_flag_takes_one_folder_once() {
        let (positional, project) = parse(&words(&["hello", "--project", "p"])).unwrap();
        assert_eq!(positional, vec!["hello".to_string()]);
        assert_eq!(project, PathBuf::from("p"));
        assert_eq!(parse(&words(&[])).unwrap().1, PathBuf::from("."));
        for bad in [
            vec!["--project"],
            vec!["--project", "--other"],
            vec!["--project", "a", "--project", "b"],
            vec!["-x"],
        ] {
            assert!(parse(&words(&bad)).is_err(), "{bad:?}");
        }
    }
}
