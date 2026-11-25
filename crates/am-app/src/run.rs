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
//! 8. the workflow exists; no fan-out step would write a table (story 3.6:
//!    refused first, in `agentic validate`'s warning words, until story
//!    3.14); and no step declares what this version cannot honour yet (a
//!    run-graph key such as `review`, `handoff` or `seats`):
//!    such a step is never run as a plain turn. A step's `output_schema` is
//!    honoured since story 3.5: its final message is checked, repaired
//!    once and failed in words (AD-36), and a schema the engine cannot
//!    compile refuses the run before any row is written;
//! 9. every step has a seat with a model, and the context's provider
//!    factory gives a provider for it; a gate step (`gate: review`, story
//!    3.3) makes no model call, so its seat needs no model and no provider
//!    is asked for it.
//!
//! Only then is the run created, by the owner (`Actor::Human`); the
//! engine's transitions are `System(engine)`'s. The command prints `run
//! <id> <status>` and exits 0 when the run succeeded, 1 otherwise (the
//! reason on stderr). A run that waits at a gate (story 3.3, Decision 4)
//! prints `run <id> waiting_human` and `Review item <id> waits for you in
//! the Inbox` and exits 0: waiting is not failing. A run the store stopped
//! after it was created is named the same way, with the status its journal
//! holds (which also decides the exit code), the failed step's reason if
//! there was one, and the store's error.
//!
//! Story 3.3: each step's `loop_back` and `limits` go to the engine's plan
//! as they are. Story 3.6 (owner Q1; AD-11, AD-21, AD-46): each step's seat
//! grants and team and its declared table (columns from the snapshot) go to
//! the plan too, with am-tools' `table_write` as the row tool, so the
//! binary's one tool runs through the engine's shipped ToolGate; every
//! other tool request is refused by ToolGate in its words.
//!
//! The binary's factory ([`unwired_providers`]) wires no provider yet and
//! refuses every model in words; tests pass am-testkit's `FakeLlm` through
//! [`RunContext`]. No env var, flag or feature swaps a provider in here.
//! Story 3.24: the run's clock is [`RunContext`]'s `clock` port, shared with
//! the plan (never a copy); the binary gives [`system_clock`], tests pass
//! am-testkit's `FakeClock`.
//!
//! `status` lists the project's runs, newest first, with every attempt of
//! their steps (story 3.3: `attempt <n>` and the attempt's state) and
//! their Inbox items (kind, id and status; story 3.4: a resolved one's
//! decision and who resolved it, never a note; a run left ready after a
//! decision says so). It
//! reads without the project's lock ([`crate::runtime::find_runtime`], a
//! read-only connection), so it works while a run or `serve` holds the
//! project, and it has no terminal rule.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{ConfigSnapshot, ModelRef, Severity};
use am_domain::rows::{ColumnKind, ColumnSpec, DeclaredTable, RowTool};
use am_domain::{
    BoxFuture, CancelSignal, ChatOutcome, ChatRequest, Clock, EventSink, Instruction, LlmProvider,
    ModelCapabilities, ProjectIdentity, ProviderError, Terminal,
};
use am_engine::prompt::{self, StepPrompt};
use am_engine::run::{HandOff, Limits, RunError, RunOutcome, RunPlan, StepPlan, run_plan};
use am_engine::schema::OutputSchema;
use am_store::{InboxItemRow, RunRow, StepRow};
use am_tools::table_write::TableWrite;
use tokio_util::sync::CancellationToken;

use crate::cli::{EXIT_ERRORS, EXIT_OK, EXIT_USAGE};
use crate::runtime::{OpenRequest, Places, Prompt, owner};

const RUN_USAGE: &str = "usage: agentic run <workflow> [--project <folder>]  (--project defaults to the current folder)";
const STATUS_USAGE: &str =
    "usage: agentic status [--project <folder>]  (--project defaults to the current folder)";

/// A step's output limit when it names no `max_tokens`.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4096;

/// The outcome of a run that waits for a person (story 3.3).
const WAITING: &str = "waiting_human";

/// What started a run made by `agentic run`.
const ORIGIN_MANUAL: &str = "manual";

/// What a refused `run` did not do.
const NOT_STARTED: &str = "No run was started";

/// A seat's model to the provider that serves it, or a refusal in words.
pub type Providers = Arc<dyn Fn(&ModelRef) -> Result<Arc<dyn LlmProvider>, String> + Send + Sync>;

/// What `agentic run` runs with. `main` builds it with the owner's real
/// terminal, the OS identity adapter, [`unwired_providers`] and
/// [`system_clock`]; tests pass am-testkit's fakes.
pub struct RunContext {
    pub places: Places,
    /// Supplies the open's identity checks and the host recorded in the
    /// store's lock (AD-32).
    pub identity: Arc<dyn ProjectIdentity>,
    /// Asked only whether it is interactive (AD-15); nothing is read.
    pub terminal: Box<dyn Terminal>,
    pub providers: Providers,
    /// The run's clock (story 3.24): the plan holds this one, shared, so
    /// the engine reads the time the context gives.
    pub clock: Arc<dyn Clock>,
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

/// A gate step's provider (story 3.3): a gate makes no model call, so the
/// engine never calls it; were it called, it refuses in words and sends
/// nothing.
struct NoModelAtGate;

impl LlmProvider for NoModelAtGate {
    fn account_id(&self) -> String {
        "none".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        _request: ChatRequest,
        _cancel: std::sync::Arc<dyn CancelSignal>,
        _sink: std::sync::Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        Box::pin(async {
            Err(ProviderError::Protocol(
                "a gate step makes no model call; a person resolves it in the Inbox".into(),
            ))
        })
    }
}

/// The words after the command: the positional ones and `--project`.
pub(crate) fn parse(args: &[String]) -> Result<(Vec<String>, PathBuf), String> {
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

pub(crate) fn usage(err: &mut dyn Write, usage: &str, why: &str) -> i32 {
    let _ = writeln!(err, "{usage}");
    let _ = writeln!(err, "agentic: {why}");
    EXIT_USAGE
}

// ------------------------------------------------------------------ run

/// The open-time prompt of `agentic run`: it asks nothing. The open
/// consults it only for `--confirm-move` and `--take-over`, which this
/// command never passes.
pub(crate) struct NoQuestion;

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
pub(crate) fn no_outside_change(
    root: &Path,
    store: &am_store::Store,
    not_done: &str,
) -> Result<(), String> {
    let history =
        am_store::StoreConfigHistory::new(store).map_err(|e| format!("{e}. {not_done}"))?;
    no_outside_change_in(root, &history, not_done)
}

/// [`no_outside_change`] against `history` (story 3.4: the server's pin
/// check holds its own).
pub(crate) fn no_outside_change_in(
    root: &Path,
    history: &dyn am_domain::ConfigHistory,
    not_done: &str,
) -> Result<(), String> {
    let changed =
        am_config::outside_changes(root, history).map_err(|e| format!("{e}. {not_done}"))?;
    if changed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} changed on disk outside the app since the app last wrote it, so no run is built from that change. {not_done}. Put the file back as the app last wrote it (undo your change), then run this command again; Reload, which takes such a change in, arrives with the app's UI",
        changed.join(", ")
    ))
}

/// What a step declares that this version of `agentic run` cannot honour
/// yet: its run-graph keys (AD-40..AD-42), by name.
fn not_honoured(step: &am_config::Step) -> Vec<&str> {
    step.run_graph.iter().map(String::as_str).collect()
}

/// The real clock: the engine reads it at each attempt's start, so a
/// guidance note that has expired is absent from the prompt.
struct SystemClock;

/// The clock the `agentic` binary gives `run` and `gate` (story 3.24): it
/// reads the system time.
pub fn system_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

impl Clock for SystemClock {
    fn now(&self) -> jiff::Timestamp {
        jiff::Timestamp::now()
    }
}

/// A headed system part (AD-27): the part's heading, then its body. am-app
/// builds every `Instruction` from config; am-engine authors none.
fn headed(label: &str, body: &str) -> Instruction {
    Instruction::authored(format!("{}{body}", prompt::heading(label)))
}

/// The seat's slots 2 to 9 (AD-27), built from the seat and its skills in
/// the snapshot. Names only for files and skill references: nothing is
/// opened or read; a link is its title and URL. A seat that lacks a field
/// gets no part for it.
///
/// CEO goals, guidance notes and memory notes have no source in the config
/// yet, so their slots stay empty here: story 5.8 ("Human seats, CEO goals
/// and guidance notes") fills the first two and story 5.3 (the seat sheet,
/// with 10.2 and 10.4, the learning loop) fills memory. The engine's order,
/// types, expiry and journal are built and tested with values.
fn prompt_of(snapshot: &ConfigSnapshot, seat: &am_config::Seat) -> StepPrompt {
    let mut prompt = StepPrompt::default();
    if let Some(goal) = seat.goal.as_deref().filter(|g| !g.is_empty()) {
        prompt = prompt.with_goal(headed(prompt::GOAL, goal));
    }
    if let Some(backstory) = seat.backstory.as_deref().filter(|b| !b.is_empty()) {
        prompt = prompt.with_backstory(headed(prompt::BACKSTORY, backstory));
    }
    for id in &seat.skills {
        // An attached skill the snapshot does not hold is already a
        // validate error (1.12); it adds no part here.
        let Some(skill) = snapshot.skills().get(id) else {
            continue;
        };
        // The one list of the skill's reference files, by name as
        // `references/<name>` (the snapshot's keys are the bare names); the
        // files' text is in the snapshot but never read for the prompt.
        // Story 9.16 swaps this one list for the loader's list of every
        // skill file (AD-38).
        let names: Vec<String> = skill
            .references
            .keys()
            .map(|name| format!("references/{name}"))
            .collect();
        let mut body = skill.body.clone();
        if !names.is_empty() {
            body.push_str("\n\n");
            body.push_str(&prompt::file_list(&names));
        }
        prompt = prompt.with_skill(id.clone(), headed(&prompt::skill_label(id), &body));
    }
    if !seat.knowledge.is_empty() {
        let names: Vec<String> = seat.knowledge.iter().map(|k| k.file.clone()).collect();
        prompt = prompt
            .with_reference_files(headed(prompt::REFERENCE_FILES, &prompt::file_list(&names)));
    }
    if !seat.links.is_empty() {
        let links: Vec<(String, String)> = seat
            .links
            .iter()
            .map(|l| (l.title.clone(), l.url.clone()))
            .collect();
        prompt = prompt.with_saved_links(headed(prompt::SAVED_LINKS, &prompt::link_list(&links)));
    }
    prompt
}

/// The step's output schema and its repair instruction (AD-36), built from
/// the schema file in the snapshot: a fixed sentence and the schema's own
/// text. The validation words never go into a prompt.
fn schema_of(
    snapshot: &ConfigSnapshot,
    step: &am_config::Step,
    workflow: &str,
    not_done: &str,
) -> Result<Option<OutputSchema>, String> {
    let Some(id) = &step.output_schema else {
        return Ok(None);
    };
    let Some(schema) = snapshot.schemas().get(id) else {
        return Err(format!(
            "step `{}` of the workflow `{workflow}` names the output schema `{id}`, which this project does not have. {not_done}",
            step.id
        ));
    };
    // `{:#}` is the value's own pretty JSON text.
    let text = format!("{schema:#}");
    let repair = Instruction::authored(format!(
        "Your last message did not match the output schema `{id}`. Reply again with one JSON object that matches this schema, and nothing else:\n{text}"
    ));
    Ok(Some(OutputSchema::new(id.clone(), schema.clone(), repair)))
}

/// The engine's plan for `workflow`, built from `snapshot` only: per step,
/// the seat's instructions as the system part, the step's instruction, the
/// seat's model and the step's output limit, on the provider the factory
/// gives for that model, and (story 3.5) the seat's prompt slots and the
/// step's output schema. A workflow with a step that declares a run-graph
/// key gives no plan (see [`not_honoured`]). The plan runs on `clock`, the
/// context's own (story 3.24).
pub(crate) fn plan_of(
    snapshot: &ConfigSnapshot,
    workflow: &str,
    providers: &Providers,
    clock: Arc<dyn Clock>,
    not_done: &str,
) -> Result<RunPlan, String> {
    let Some(wf) = snapshot.workflows().get(workflow) else {
        let known: Vec<&str> = snapshot.workflows().keys().map(String::as_str).collect();
        let known = if known.is_empty() {
            "it has no workflows".to_string()
        } else {
            format!("its workflows are: {}", known.join(", "))
        };
        return Err(format!(
            "this project has no workflow `{workflow}`; {known}. {not_done}"
        ));
    };
    // Story 3.6 (row 40), first: a fan-out step that would write a table,
    // in `agentic validate`'s words, until story 3.14.
    for step in &wf.steps {
        if let Some((message, fix)) = am_config::fan_out_table_refusal(snapshot, step) {
            return Err(format!(
                "workflow `{workflow}`: {message}; fix: {fix}. {not_done}"
            ));
        }
    }
    // Before any provider is asked for: a step this version would run as
    // one plain turn, with its gate, hand-off, fan-out or schema skipped.
    for step in &wf.steps {
        let keys = not_honoured(step);
        if !keys.is_empty() {
            let keys: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
            return Err(format!(
                "step `{}` of the workflow `{workflow}` declares {}, which this version of `agentic run` cannot honour yet: it runs each step as one plain model turn and would skip it without saying so. {not_done}",
                step.id,
                keys.join(", ")
            ));
        }
    }
    let mut steps = Vec::new();
    for step in &wf.steps {
        let Some(seat_id) = step.seat.as_deref() else {
            return Err(format!(
                "step `{}` of the workflow `{workflow}` has no `seat:` of its own, and this version of `agentic run` runs a step only on its one seat. {not_done}",
                step.id
            ));
        };
        let Some(seat) = snapshot.seats().get(seat_id) else {
            return Err(format!(
                "step `{}` of the workflow `{workflow}` names the seat `{seat_id}`, which this project does not have. {not_done}",
                step.id
            ));
        };
        // A seat with no instructions sends no system part (never an empty one).
        let system = if seat.instructions.trim().is_empty() {
            Vec::new()
        } else {
            vec![Instruction::authored(seat.instructions.clone())]
        };
        let limits = Limits {
            iterations: step.limits.iterations,
            tool_calls: step.limits.tool_calls,
            wall_clock: step.limits.wall_clock,
        };
        // Story 3.6 (AD-21, AD-46): the seat's grants and team, and the
        // table the step declares, with its columns as the snapshot holds
        // them.
        let table = table_of(snapshot, step, workflow, not_done)?;
        // A gate makes no model call (AD-36): its seat needs no model, and
        // no provider is asked for it.
        if step.gate.is_some() {
            steps.push(StepPlan {
                key: step.id.clone(),
                seat: seat_id.to_string(),
                system,
                instruction: Instruction::authored(step.instruction.clone()),
                inputs: Vec::new(),
                hand_off: HandOff::Gate {
                    max_returns: step.max_returns,
                },
                limits,
                model: String::new(),
                max_output_tokens: step.max_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
                provider: Arc::new(NoModelAtGate),
                // A gate makes no model call and assembles nothing.
                prompt: StepPrompt::default(),
                output_schema: None,
                grants: seat.grants.clone(),
                seat_team: seat.team.clone(),
                table,
                // Story 3.21: a gate declares none (a config error); the
                // engine refuses one that does.
                document: step.document.clone(),
                // Story 3.22: no config key names rows a step reads yet
                // (3.15 and 7.2 fill the slot).
                row_inputs: Vec::new(),
            });
            continue;
        }
        let Some(model) = &seat.model else {
            return Err(format!(
                "step `{}` of the workflow `{workflow}` runs on the seat `{seat_id}`, which has no model; set `model:` (its `account` and `model`) in seats/{seat_id}.md. {not_done}",
                step.id
            ));
        };
        let provider = providers(model).map_err(|why| format!("{why}. {not_done}"))?;
        let output_schema = schema_of(snapshot, step, workflow, not_done)?;
        let hand_off = match &step.loop_back {
            Some(l) => HandOff::LoopBack {
                to: l.to.clone(),
                field: l.reasons.clone(),
                max: l.max,
            },
            None => HandOff::Next,
        };
        steps.push(StepPlan {
            key: step.id.clone(),
            seat: seat_id.to_string(),
            system,
            instruction: Instruction::authored(step.instruction.clone()),
            inputs: Vec::new(),
            hand_off,
            limits,
            model: model.model.clone(),
            max_output_tokens: step.max_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
            provider,
            prompt: prompt_of(snapshot, seat),
            output_schema,
            grants: seat.grants.clone(),
            seat_team: seat.team.clone(),
            table,
            // Story 3.21 (AD-29): the one document the step versions.
            document: step.document.clone(),
            // Story 3.22: no config key names rows a step reads yet (3.15
            // and 7.2 fill the slot).
            row_inputs: Vec::new(),
        });
    }
    // Owner Q1: am-tools' `table_write`; only a step whose seat is granted
    // it and that declares a table is offered it (the engine's ToolGate).
    let row_tool: Arc<dyn RowTool> = Arc::new(TableWrite::new());
    Ok(RunPlan {
        workflow: workflow.to_string(),
        config_hash: snapshot.hash().to_string(),
        steps,
        clock,
        row_tool: Some(row_tool),
    })
}

/// The table `step` declares (story 3.6, AD-21): its id, its owning team and
/// its columns (ids, types, required flags, a status column's values), as
/// the snapshot holds them; `None` when it declares none.
fn table_of(
    snapshot: &ConfigSnapshot,
    step: &am_config::Step,
    workflow: &str,
    not_done: &str,
) -> Result<Option<DeclaredTable>, String> {
    let Some(id) = &step.table else {
        return Ok(None);
    };
    let Some(table) = snapshot.tables().get(id) else {
        return Err(format!(
            "step `{}` of the workflow `{workflow}` declares the table `{id}`, which this project does not have. {not_done}",
            step.id
        ));
    };
    let mut columns = Vec::new();
    for column in &table.columns {
        let Some(kind) = ColumnKind::parse(&column.column_type) else {
            return Err(format!(
                "the table `{id}` declares the column `{}` of type `{}`, which this version cannot write. {not_done}",
                column.id, column.column_type
            ));
        };
        columns.push(ColumnSpec {
            id: column.id.clone(),
            kind,
            required: column.required,
            values: column.values.clone(),
        });
    }
    Ok(Some(DeclaredTable {
        id: id.clone(),
        team: table.team.clone(),
        columns,
    }))
}

/// The project's config snapshot (check 5), or why no run starts on it.
pub(crate) fn snapshot_of(
    places: &Places,
    root: &Path,
    project: &Path,
    not_done: &str,
) -> Result<ConfigSnapshot, String> {
    let offers = am_config::migration_offers(root).map_err(|e| format!("{e}. {not_done}"))?;
    if !offers.is_empty() {
        let files: Vec<String> = offers
            .iter()
            .map(|o| format!("{} from schema_version {} to {}", o.file, o.from, o.to))
            .collect();
        return Err(format!(
            "the project waits for a config migration ({}), and nothing runs on config you have not accepted. {not_done}; `agentic validate {}` names each file and its fix",
            files.join(", "),
            project.display()
        ));
    }
    let loaded = am_config::load_in(root, crate::cli::project_load_options(Some(places), root))
        .map_err(|e| format!("{e}. {not_done}"))?;
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
                "the project's config has {}{first}, so no run can be built from it. {not_done}; run `agentic validate {}` to see each error with its fix",
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
    no_outside_change(&root, &opened.store, NOT_STARTED)?;
    let snapshot = snapshot_of(&ctx.places, &root, project, NOT_STARTED)?;
    // The owner's files may have changed since check 3 (the open can wait
    // on a migration): what the pinned snapshot holds decides.
    no_pinned_owner_child(&snapshot)?;
    let plan = plan_of(
        &snapshot,
        workflow,
        &ctx.providers,
        ctx.clock.clone(),
        NOT_STARTED,
    )?;
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
            if outcome.status == WAITING {
                // Waiting is not failing (Decision 4): the run row stays
                // `running`, and only a person resolves the item.
                if let Some(item) = &outcome.item {
                    let _ = writeln!(out, "Review item {item} waits for you in the Inbox");
                    // Story 3.4: where the owner decides it today.
                    let _ = writeln!(
                        out,
                        "Decide it on this terminal with `agentic gate {item}` (add `--project <folder>` outside the project's folder)"
                    );
                }
                return EXIT_OK;
            }
            if let Some(reason) = &outcome.reason {
                let _ = writeln!(
                    err,
                    "agentic: run {} {}: {reason}",
                    outcome.run_id, outcome.status
                );
            }
            if let Some(item) = &outcome.item {
                let _ = writeln!(
                    err,
                    "agentic: run {}: Failure item {item} is in the Inbox",
                    outcome.run_id
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
    // A step that waits for a person is not running (story 3.3).
    let took = if step.status == WAITING {
        "waiting".to_string()
    } else {
        duration(
            step.started_at.as_deref(),
            step.finished_at.as_deref(),
            ended(&step.status),
        )
    };
    format!(
        "    {}  attempt {}  {}  {took}",
        step.step_key, step.attempt, step.status
    )
}

/// An Inbox item of a run (stories 3.3 and 3.4): its kind, id and status,
/// and once resolved its decision and who resolved it ("You (Reviewer)");
/// never its words or a note.
fn item_line(item: &InboxItemRow) -> String {
    let mut line = format!(
        "    {} item {}  {}",
        item.kind.title(),
        item.id,
        item.status
    );
    if let Some(decision) = item.decision {
        let who = item
            .resolver()
            .map(|by| by.describe())
            .unwrap_or_else(|| "an unknown resolver".into());
        line.push_str(&format!("  {}  by {who}", decision.as_str()));
    }
    line
}

/// Story 3.4 (Question 1): a run a person's decision was recorded and
/// applied for, which nothing has continued since (an API resolve, or an
/// `agentic gate` that stopped): still `running`, no step running or
/// waiting, a Review resolved.
fn ready_to_continue(run: &RunRow, steps: &[StepRow], items: &[InboxItemRow]) -> bool {
    run.status == "running"
        && !steps
            .iter()
            .any(|s| s.status == "running" || s.status == WAITING)
        && items
            .iter()
            .any(|i| i.kind == am_domain::InboxKind::Review && i.status == "resolved")
}

/// A run, its step attempts and its Inbox items.
type RunLines = (RunRow, Vec<StepRow>, Vec<InboxItemRow>);

/// The runs of the project at `project`, newest first, each with its steps
/// and its Inbox items.
fn read_runs(places: &Places, project: &Path) -> Result<Vec<RunLines>, String> {
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
        let items = reader.inbox_items(&run.id).map_err(|e| e.to_string())?;
        all.push((run, steps, items));
    }
    Ok(all)
}

/// `agentic status [--project <folder>]`: `args` are the words after
/// `status`. Prints each run, newest first (id, workflow, status, the first
/// 12 characters of its snapshot hash, duration), and under it each step
/// attempt (key, `attempt <n>`, status, duration, or `waiting` for a step
/// that waits for a person) and each Inbox item (kind, id, status, and a
/// resolved one's decision and resolver). Reads only, without the project's lock and with
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
    for (run, steps, items) in &runs {
        let _ = writeln!(out, "{}", run_line(run));
        for step in steps {
            let _ = writeln!(out, "{}", step_line(step));
        }
        for item in items {
            let _ = writeln!(out, "{}", item_line(item));
        }
        if ready_to_continue(run, steps, items) {
            let _ = writeln!(
                out,
                "    ready to continue (resume arrives with story 3.9; `agentic gate` continues only a run it resolves)"
            );
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

    /// Story 3.24 (matrix row 19): the plan's clock is the context's clock
    /// port, the same one (a later `set` shows through), never a clock of
    /// `plan_of`'s own; `agentic run` and `agentic gate`'s continue both
    /// build their plan here. The binary's clock
    /// ([`crate::cli::system_clock`]) reads the system time.
    #[test]
    fn plan_of_runs_on_the_contexts_clock() {
        // am-app/src never names the test kit (xtask's release guard), so
        // the fixture path and a settable clock are spelled out here.
        struct SetClock(std::sync::Mutex<jiff::Timestamp>);
        impl SetClock {
            fn at(t: &str) -> Arc<Self> {
                Arc::new(SetClock(std::sync::Mutex::new(t.parse().unwrap())))
            }
            fn set(&self, t: &str) {
                *self.0.lock().unwrap() = t.parse().unwrap();
            }
        }
        impl Clock for SetClock {
            fn now(&self) -> jiff::Timestamp {
                *self.0.lock().unwrap()
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/one-step");
        let loaded = am_config::load_in(&root, am_config::LoadOptions::template(&root)).unwrap();
        let snapshot = loaded.snapshot.expect("the one-step fixture loads");
        let providers: Providers =
            Arc::new(|_model: &ModelRef| Ok(Arc::new(NoModelAtGate) as Arc<dyn LlmProvider>));
        let clock = SetClock::at("2026-11-25T09:00:00Z");

        let plan = plan_of(&snapshot, "hello", &providers, clock.clone(), NOT_STARTED).unwrap();

        let at = |t: &str| t.parse::<jiff::Timestamp>().unwrap();
        assert_eq!(plan.clock.now(), at("2026-11-25T09:00:00Z"));
        clock.set("2026-11-26T10:30:00Z");
        assert_eq!(
            plan.clock.now(),
            at("2026-11-26T10:30:00Z"),
            "the plan holds the context's clock, not a copy"
        );

        let system = crate::cli::system_clock();
        let before = jiff::Timestamp::now();
        let now = system.now();
        let after = jiff::Timestamp::now();
        assert!(before <= now && now <= after, "{before} {now} {after}");
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
