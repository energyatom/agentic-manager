//! The run driver (epic 3 stories 1 and 3; AD-8, AD-6, AD-10, AD-11,
//! AD-16, AD-27, AD-28, AD-36): a plan of steps in order, every transition
//! committed before the effect it authorises.
//!
//! The plan is plain types: am-app maps a config snapshot to it and
//! resolves each step's provider (AD-4; this crate reads no config). The
//! driver writes each command with its audit entry in one transaction: the
//! run (`running`, with the plan's snapshot hash pinned) as the Actor who
//! started it, and every later transition as `System(engine)`.
//!
//! **The loop decides from the journal.** After the run is created, the
//! driver reads the run's step attempts back each time it moves on and runs
//! the first step (in plan order) whose latest attempt is `queued`; never
//! an in-memory index, so stories 3.4 and 3.9 can enter it later.
//!
//! **A step attempt (story 3.3)** starts with its inputs journaled
//! (`step_inputs`) and its taint committed, then is a bounded loop of model
//! turns (AD-11, RULE-11): each turn's call is reserved before it is made;
//! a turn that asks for tools goes on once ToolGate has decided each call
//! and each allowed call's result is appended (as `UntrustedData`, which
//! taints the attempt with the next reservation). Three caps end the
//! attempt `failed` with one Failure Inbox item: `iterations` (model turns,
//! counted from `model_calls` rows), `tool_calls` (counted from `tool_calls`
//! rows) and `wall_clock` (a fresh attempt's deadline is tokio's clock
//! right after its `StartAttempt` plus the cap; story 3.9's resume will
//! count a resumed attempt's persisted `started_at`). Nothing starts once
//! the deadline has passed, and a reply or tool ready at the deadline loses
//! to it; the call in flight is cancelled through its own child token and
//! settles by the ledger rule.
//!
//! **Inputs and taint (AD-10, AD-27, AD-36).** By `next` a step receives
//! the previous step's committed output as one input labelled
//! `step:<key>@<attempt>`; a loop-back target receives the reasons labelled
//! `loop-back:<key>@<attempt>` (its `loop_back` input, journaled by the
//! loop-back itself); a plan input is untrusted. Each goes through
//! `TrustedInput::from_step_output` with the producer's taint as committed
//! in the journal: a clean producer's text is a `TrustedInput`, a tainted
//! one's is fenced `UntrustedData` that taints the receiver. The request's
//! one user message holds the instruction, then the trusted inputs, then
//! the fenced ones. Since story 3.22 a step may read rows at its start
//! (its plan's `row_inputs`, through `row_inputs`): each is a
//! `row` input naming the after-image it read, with at least that image's
//! taint, and fenced `UntrustedData` that taints the attempt, whatever the
//! row's mark (AD-10, AD-43).
//!
//! **Hand-offs (AD-36).** `next` is the default. A `loop_back` step's final
//! message must be a JSON object whose named field is a list of strings
//! (fail closed until story 3.5's schemas): an empty list is `next`; a
//! non-empty one ends the attempt `looped_back` and queues new attempts
//! from the target through the producer, until the producer's `looped_back`
//! attempts in the journal reach `max`, when the step fails with a Failure
//! item. A `gate` step makes no model call: it waits (`waiting_human`) with
//! one Review item whose payload is the previous step's attempt (and its
//! `max_returns`), and the run stays `running` (the outcome says
//! `waiting_human`). Only a Human resolve (story 3.4) moves it on.
//!
//! **After a person decides (story 3.4; AD-8, AD-10, AD-15, AD-16, AD-36).**
//! [`continue_run`] takes a run that stopped cleanly: still `running`, its
//! plan built from the snapshot the run pinned, no call unsettled and no
//! step `running` but a gate whose Review a person resolved. It applies that
//! decision first ([`crate::gate::apply_resolution`], as `System(engine)`),
//! then enters the same loop. The step after an approved gate receives the
//! Review's payload (the producer's output, with its committed taint) as
//! its `next` input; a returned producer's new attempt receives the
//! person's note as a trusted part, built with
//! `TrustedInput::from_owner_note` from the resolver the journal holds (no
//! person, no note, and the attempt does not start).
//!
//! **Tools.** Every run has a ToolGate, and every tool request goes through
//! it (story 3.6, AD-11). [`run_plan`] builds the shipped gate: the
//! built-in tools' manifests and `table_write`, which writes through the
//! engine's row-writer port with the plan's row tool (am-app hands in
//! am-tools'). A step is offered only what its seat is granted (AD-46) and
//! `table_write` only when it declares a table of its seat's team. Runner
//! tools run only through [`run_plan_with_tools`]'s runner, a constructor
//! parameter shipped code never passes (owner condition 2, `cargo xtask
//! seam-guard`), and only ToolGate calls it, for a call it admitted; the
//! driver holds the gate, never the runner (review R7). A tool name the
//! model chose and no manifest holds is labelled by ToolGate and journaled,
//! audited and reported only as `unknown:<sha256 of the name>` (review P15,
//! R9). A ToolGate refusal fails the step (no Failure item); a call it
//! admitted that errs is an untrusted error result the model reads, and
//! taints the attempt from its next call; a successful `table_write` is
//! the engine's own result (`Part::EngineResult`), which leaves the
//! attempt's taint as it was (AD-10 as amended 2026-11-24).
//!
//! **Documents (story 3.21; AD-28, AD-29, AD-43).** A step that declares
//! a document stores its final message, once validated (not empty after
//! trimming, no NUL, at most 1 MiB), as the document's next version when it
//! succeeds, and raises one Draft item on its attempt, in the transaction
//! that finishes it (`SettleAndFinishDocumentStep`); the Draft's words are
//! the engine's and it never blocks the run. A final message that cannot be
//! a version fails the step with a Failure item and no version. A document
//! step takes no output schema, no loop-back and is no gate (refused before
//! any row).
//!
//! A provider error, a reply cut off at its output limit or a cancelled
//! call fails the step and the run with the reason in words and no item
//! (story 3.8 adds Failure items for those); the failed turn's text is not
//! stored, and later steps stay queued.
//!
//! Story 3.7 (AD-8, AD-9, AD-16; owner Q1, Q2, Q11, Q14, Q15): each call
//! is reserved before it is made, from its request's counted prompt tokens
//! ([`crate::count`]), the step's `max_output_tokens` and the step's price
//! terms with the run's rate, both read from the pinned snapshot when the
//! plan was built ([`StepPlan::price`], [`RunPlan::money`]); the writer
//! computes the amounts. The reservation journals the budget week (the run
//! clock's instant in the company's timezone and `week_start`) and the
//! seat's team. Each settlement carries the provider's usage only; the
//! writer prices it at the reserved terms, or at the reservation without
//! usage. A step that calls a model and has no price is refused before any
//! row. After a store error, the cleanup marks the open call lost, once, so
//! no reservation stays open. No cap is checked (epic 4).
//!
//! Not here: full labelled prompt assembly (story 5), retries and fallback
//! (story 8), resume (story 9). The spike's resumable driver stays in
//! [`crate::journal`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::money::{CallPrice, Rate};
use am_domain::rows::{DeclaredTable, RowInput, RowTool};
use am_domain::{
    Actor, AuditEvent, AuditSink, ChatOutcome, ChatRequest, Clock, EventSink, InboxKind,
    Instruction, LlmProvider, Message, Part, ProviderError, Role, StepStatus, StopReason,
    StreamEvent, Taint, ToolCall, ToolRunner, TrustedInput, UntrustedData,
};
use am_store::{
    AttemptWrites, CallPurpose, CallSettlement, NewInboxItem, NewPromptPart, PromptKind,
    PromptSlot, Reader, StepInput, StepRow, Store, StoreError, WriteCommand, markdown_refusal,
    new_id,
};
use tokio_util::sync::CancellationToken;

use crate::TokenSignal;
use crate::prompt::{self, Slots, StepPrompt};
use crate::row_inputs;
use crate::row_writer::StoreRowWriter;
use crate::schema::{self, Compiled, OutputSchema};
use crate::toolgate::{CallPlace, GateError, GateOutput, StepTools, TABLE_WRITE, ToolGate};

/// How long a call cancelled at its wall-clock cap may take to stop before
/// it is settled at its reservation anyway.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// How a step hands its work on (AD-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandOff {
    /// The following step runs and receives this step's output.
    Next,
    /// When this step's final message lists reasons in `field`, the run goes
    /// back to the earlier step `to`, at most `max` times.
    LoopBack { to: String, field: String, max: u32 },
    /// A human gate: no model call; the step waits with a Review item. The
    /// owner may return its work `max_returns` times (story 3.4, AD-36);
    /// past that, the gate fails with a Failure item.
    Gate { max_returns: u32 },
}

/// A step attempt's caps (AD-11, RULE-11). The defaults are the owner's
/// placeholders (story 3.3, Question 7); am-app sets each step's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Model turns per attempt.
    pub iterations: u32,
    /// Tool calls per attempt.
    pub tool_calls: u32,
    /// Time per attempt, from its persisted start.
    pub wall_clock: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            iterations: 8,
            tool_calls: 20,
            wall_clock: Duration::from_secs(600),
        }
    }
}

/// One step: a bounded loop of model turns on `provider`, or, for a gate,
/// a wait for a person (its provider is never called).
///
/// The fields that bind a write are read like any other:
///
/// ```
/// let _ = |p: am_engine::run::StepPlan| (p.grants, p.seat_team, p.table, p.document);
/// ```
#[derive(Clone)]
pub struct StepPlan {
    /// The step's key in the run (its id in the workflow).
    pub key: String,
    /// The seat that runs it: a tool call's `Actor::Seat`.
    pub seat: String,
    /// The seat's instructions, as the request's system part.
    pub system: Vec<Instruction>,
    /// The step's instruction, as the first user part.
    pub instruction: Instruction,
    /// Inputs the plan hands the step (untrusted; journaled `untrusted`,
    /// and they taint every attempt of the step).
    pub inputs: Vec<UntrustedData>,
    pub hand_off: HandOff,
    pub limits: Limits,
    /// The model id on `provider`'s Account.
    pub model: String,
    pub max_output_tokens: u32,
    /// A gate step's provider is never called.
    pub provider: Arc<dyn LlmProvider>,
    /// Story 3.5 (AD-27): the seat's goal, backstory, skills, notes, file
    /// names and links, as typed slots; `system` and `instruction` above
    /// are slots 1 and 10. A gate assembles nothing.
    pub prompt: StepPrompt,
    /// Story 3.5 (AD-36): the schema the step's final message must match;
    /// compiled before any row is written.
    pub output_schema: Option<OutputSchema>,
    /// Story 3.6 (AD-46): the seat's tool grants, its capabilities'
    /// expansion; ToolGate admits no tool outside them.
    pub grants: BTreeSet<String>,
    /// Story 3.6 (AD-21): the seat's team; a seat writes only its own
    /// team's tables.
    pub seat_team: String,
    /// Story 3.6 (AD-21, owner Q3): the one table the step writes (`table:`),
    /// with its owning team and columns; binds `table_write`, never grants
    /// it. A gate declares none.
    pub table: Option<DeclaredTable>,
    /// Story 3.21 (AD-29, owner Q5 to Q8): the one document the step
    /// versions (`document:`): its validated final message becomes the
    /// document's next version when it succeeds, with a Draft item. Not with
    /// an output schema or a loop-back, and not on a gate.
    pub document: Option<String>,
    /// Story 3.22 (AD-10, AD-43): the rows the step reads at each attempt's
    /// start, through `row_inputs`: each is a `row` input naming
    /// the after-image it read and a fenced part that taints the attempt.
    /// The shipped plan leaves it empty (3.15 and 7.2 fill it from config).
    pub row_inputs: Vec<RowInput>,
    /// Story 3.7 (AD-9, AD-16; owner Q1, Q8): the price terms of the
    /// step's model, read from the pinned snapshot when the plan was built
    /// (a model on this Mac: [`CallPrice::local`]). `None` only on a gate,
    /// which makes no call; any other step without one is refused before
    /// any row.
    pub price: Option<CallPrice>,
}

/// Story 3.7 (AD-9, AD-16; owner Q6, Q11): the run's money, from its
/// pinned snapshot: the company currency's rate (with the price file's
/// snapshot id and date), and the timezone and `week_start` a reservation's
/// budget week is computed in.
#[derive(Debug, Clone)]
pub struct RunMoney {
    pub rate: Rate,
    pub timezone: jiff::tz::TimeZone,
    pub week_start: jiff::civil::Weekday,
}

/// A run to make: the workflow's steps in order, built from one config
/// snapshot.
#[derive(Clone)]
pub struct RunPlan {
    pub workflow: String,
    /// The hash of the config snapshot the plan was built from (AD-16),
    /// pinned on the run row.
    pub config_hash: String,
    pub steps: Vec<StepPlan>,
    /// Story 3.5 (AD-27): read at each attempt's start, so a guidance note
    /// that has expired is absent from the prompt.
    pub clock: Arc<dyn Clock>,
    /// Story 3.6 (owner Q1): the row tool `table_write` is prepared with
    /// (am-tools', which am-app hands in: am-engine may not depend on
    /// am-tools, AD-4); `None`: no step writes a row.
    pub row_tool: Option<Arc<dyn RowTool>>,
    /// Story 3.7: the rate, timezone and week start every reservation of
    /// the run is priced and dated with.
    pub money: RunMoney,
}

/// How a run ended, or where it waits. A failed run is an outcome, not an
/// error: the caller still has its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub run_id: String,
    /// `succeeded`, `failed` or `waiting_human` (the run row stays
    /// `running` while a step waits).
    pub status: String,
    /// Why the run failed, in words; `None` when it succeeded or waits.
    pub reason: Option<String>,
    /// The Inbox item the run stopped at: the Review item a waiting run
    /// waits on, or the Failure item a failed run raised.
    pub item: Option<String>,
}

/// Why a run could not be made or journaled.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("the workflow {0} has no steps, so there is nothing to run")]
    NoSteps(String),
    /// Story 3.5 (AD-3, AD-36): a step's output schema cannot be used (a
    /// remote or file `$ref`, a pattern the linear-time engine refuses, an
    /// unknown type); nothing was written.
    #[error("step `{step}` cannot run: {why}")]
    Schema { step: String, why: String },
    /// The tools handed to the run cannot be used (story 3.3, review R7).
    #[error("the run's tools were refused: {0}")]
    Tools(String),
    /// Story 3.6 (AD-21, AD-36): what a step declares it writes cannot be
    /// written (a table on a gate, a table of another team than its seat's;
    /// since story 3.21 a document with an output schema or a loop-back, or
    /// on a gate), or a gate names rows to read (story 3.22); nothing was
    /// written.
    #[error("step `{step}` cannot run: {why}")]
    Writes { step: String, why: String },
    /// Story 3.7 (AD-9): a step that calls a model has no price, so its
    /// calls cannot be reserved; nothing was written.
    #[error(
        "step `{0}` cannot run: it calls a model and has no price, so its calls cannot be reserved (AD-9); nothing was written"
    )]
    Unpriced(String),
    /// Story 3.7, review M6 (AD-9): a step whose calls cost money, in a run
    /// whose company currency has no rate; nothing was written.
    #[error(
        "step `{step}` cannot run: its calls cost money and the run has no rate for the company currency {currency}, so they cannot be reserved (AD-9); nothing was written"
    )]
    Unrated { step: String, currency: String },
    /// The run could not be created: the journal holds nothing of it.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The store failed a command after the run was created. The engine
    /// tried once to record how the run ended: the status it was about to
    /// record when every started step had finished, `failed` (with the
    /// open step) when one had not. `status` is what the journal holds
    /// now: that status, or `running` when the store refused that too.
    /// `reason` is why a step failed, when one did before the store erred.
    #[error("run {run_id} stopped ({status}): {source}")]
    Stopped {
        run_id: String,
        status: String,
        reason: Option<String>,
        source: Box<StoreError>,
    },
    /// Story 3.4 (AD-16, Question 2): a run goes on only on the snapshot it
    /// pinned; nothing was written.
    #[error(
        "the project's config is not the snapshot the run pinned: the run pinned {pinned}, and the project's files now give {now}; a run goes on only on the config it started with (AD-16), so nothing was written. Put the files back as they were when the run started, or reject the item and start a new run"
    )]
    ConfigChanged { pinned: String, now: String },
    /// Story 3.4: the run did not stop cleanly (a step left running, a call
    /// unsettled) or has finished; nothing was written.
    #[error("{0}")]
    NotContinuable(String),
}

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

struct NullSink;

impl EventSink for NullSink {
    fn on_event(&self, _event: StreamEvent) {}
}

/// Why a turn did not finish its step, in words; `None` for a turn that
/// ended its turn or asked for tools.
fn failure_of(step: &StepPlan, turn: &Result<ChatOutcome, ProviderError>) -> Option<String> {
    let out = match turn {
        Ok(out) => out,
        Err(e) => return Some(format!("the model call failed: {e}")),
    };
    match out.stop {
        StopReason::Cancelled => Some("the model call was cancelled before it finished".into()),
        StopReason::MaxTokens => Some(format!(
            "the reply was cut off at the step's output limit of {} tokens (max_tokens)",
            step.max_output_tokens
        )),
        StopReason::ToolUse if out.tool_calls.is_empty() => {
            Some("the model stopped to use a tool but named none".into())
        }
        StopReason::ToolUse => None,
        _ if !out.tool_calls.is_empty() => None,
        StopReason::EndTurn => None,
        StopReason::Other => Some("the model stopped before it finished its turn".into()),
    }
}

/// A duration in its shortest whole unit: `2h`, `10m`, `30s`.
fn duration_words(d: Duration) -> String {
    let s = d.as_secs();
    if s > 0 && s.is_multiple_of(3600) {
        format!("{}h", s / 3600)
    } else if s > 0 && s.is_multiple_of(60) {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

/// The function that executes one write command (the store's writer).
type Exec<'a> = &'a (dyn Fn(Actor, WriteCommand) -> Result<(), StoreError> + Send + Sync);

/// How far a run got, for the cleanup after a store error.
#[derive(Default)]
struct Progress {
    /// The step run that is started and not finished.
    open_step: Option<String>,
    /// Why a step failed, from the moment its turn's failure is known.
    reason: Option<String>,
    /// The status the engine is about to record for the run, once every
    /// started step has finished: `succeeded` after the last step, `failed`
    /// after a failed one.
    finishing: Option<&'static str>,
    /// The Inbox item the run stopped at.
    item: Option<String>,
}

impl Progress {
    /// What the cleanup records for the run: what the engine was about to
    /// record, or `failed` when a step did not finish.
    fn to_record(&self) -> &'static str {
        match (&self.open_step, self.finishing) {
            (None, Some(status)) => status,
            _ => "failed",
        }
    }
}

/// Creates the run for `plan` as `started_by` and drives it to its end, or
/// to a gate where it waits. `origin` says what started it (`manual`). Its
/// tools are the shipped ToolGate's (story 3.6): `table_write` with the
/// plan's row tool, and no runner. See the module docs for the order of the
/// commits.
///
/// A store error once the run exists never leaves it `running` unsaid: the
/// engine tries once to record how the run ended (see
/// [`RunError::Stopped`]) and returns that error with the run's id, the
/// status the journal now holds and the failed step's reason, if any. (A
/// call the store could not settle stays as the journal has it.)
pub async fn run_plan(
    store: &Store,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    let tooling = Tooling::shipped(store, plan);
    run_on(store, &exec, started_by, origin, plan, cancel, tooling).await
}

/// [`run_plan`] with a tool runner: tests only (story 3.3, owner condition
/// 2). Shipped code never names it (`cargo xtask seam-guard`); its one
/// runner is am-testkit's fixture tool. It builds the ToolGate over the
/// runner, and the driver holds only that gate (review R7). A runner that
/// declares one tool id twice is refused before any run is created.
pub async fn run_plan_with_tools(
    store: &Store,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
    tools: Arc<dyn ToolRunner>,
) -> Result<RunOutcome, RunError> {
    let refusals = Arc::new(Refusals::default());
    let gate = ToolGate::over(
        tools,
        Arc::new(StoreRowWriter::new(store)),
        plan.row_tool.clone(),
        refusals.clone(),
    );
    let twice = gate.duplicates();
    if !twice.is_empty() {
        return Err(RunError::Tools(format!(
            "its runner declares the tool id(s) {} more than once, so which manifest decides is not known; no run was created",
            twice.join(", ")
        )));
    }
    // Review round 1, R5: a runner never takes a built-in tool's id.
    let shadowing = gate.shadowing();
    if !shadowing.is_empty() {
        return Err(RunError::Tools(format!(
            "its runner declares the built-in tool id(s) {}, which only the built-in tools hold; no run was created",
            shadowing.join(", ")
        )));
    }
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    let tooling = Tooling { gate, refusals };
    run_on(store, &exec, started_by, origin, plan, cancel, tooling).await
}

/// [`run_plan`], with every write command executed by `exec` (the seam for
/// the tests of a store that fails a command) and the tools (a ToolGate).
async fn run_on(
    store: &Store,
    exec: Exec<'_>,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
    tools: Tooling,
) -> Result<RunOutcome, RunError> {
    if plan.steps.is_empty() {
        return Err(RunError::NoSteps(plan.workflow.clone()));
    }
    // Story 3.6: what each step declares it writes, before any row.
    check_writes(plan)?;
    // Story 3.7: every step that calls a model has its price.
    check_prices(plan)?;
    // Story 3.5: every step's schema is compiled before any row is written.
    let schemas = compile_schemas(plan)?;
    let run_id = new_id();
    // A tool call acts as `Actor::Seat` of this run, named by its UUID: an
    // id that does not parse is an error, never a made-up UUID (review P13).
    let run_uuid = uuid::Uuid::parse_str(&run_id)
        .map_err(|e| StoreError::Invalid(format!("the new run id {run_id} is not a UUID: {e}")))?;
    exec(
        started_by,
        WriteCommand::CreateRun {
            run_id: run_id.clone(),
            workflow: plan.workflow.clone(),
            origin: origin.into(),
            steps: plan.steps.iter().map(|s| s.key.clone()).collect(),
            config_hash: Some(plan.config_hash.clone()),
        },
    )?;
    let run = Run {
        store,
        exec,
        run_id: &run_id,
        run_uuid,
        plan,
        schemas: &schemas,
        cancel,
        tools: &tools,
    };
    run.drive_to_outcome(Progress::default()).await
}

/// Story 3.7 (AD-9): refuses a plan, before anything is written, whose step
/// calls a model (any step but a gate) and has no price, or (review M6)
/// whose step's calls cost money when the run has no rate.
fn check_prices(plan: &RunPlan) -> Result<(), RunError> {
    for step in &plan.steps {
        if matches!(step.hand_off, HandOff::Gate { .. }) {
            continue;
        }
        match &step.price {
            None => return Err(RunError::Unpriced(step.key.clone())),
            Some(price) if !price.local && plan.money.rate.company_micro_per_usd.is_none() => {
                return Err(RunError::Unrated {
                    step: step.key.clone(),
                    currency: plan.money.rate.currency.clone(),
                });
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Story 3.6 (AD-21, AD-36): refuses a plan, before anything is written,
/// whose step declares a table on a gate (a gate makes no model call) or a
/// table of another team than its seat's (a seat writes only its own
/// team's tables); since story 3.22, a gate that names rows to read; since
/// story 3.21, a document step that is a gate, loops back or has an output
/// schema (its version is its final message as written), a document id that
/// is not a config id, a writing step with no seat, and one document
/// declared by seats of two teams. The words name config ids only.
fn check_writes(plan: &RunPlan) -> Result<(), RunError> {
    for step in &plan.steps {
        let refused = |why: String| RunError::Writes {
            step: step.key.clone(),
            why,
        };
        // Review round 2: a step that writes names the seat that writes it.
        if (step.document.is_some() || step.table.is_some()) && step.seat.trim().is_empty() {
            return Err(refused(
                "it writes a document or a table and names no seat; a write is made by the seat that runs the step, so a writing step names one".into(),
            ));
        }
        if let Some(document) = &step.document {
            // Review round 2: the one config-id rule (am-domain's), as the
            // validator and the store apply it.
            if !am_domain::config_names::is_config_id(document) {
                return Err(refused(
                    "it declares a document whose id is not a config id (kebab-case: lowercase letters and digits joined by hyphens); a document is named by its config id".into(),
                ));
            }
            // Owner Q3: a document belongs to one team, its declaring
            // seats'.
            if let Some(first) = plan.steps.iter().find(|other| {
                other.document.as_deref() == Some(document.as_str())
                    && other.seat_team != step.seat_team
            }) {
                return Err(refused(format!(
                    "it declares the document `{document}` on a seat of team `{}`, and step `{}` declares it on a seat of team `{}`; a document belongs to one team",
                    step.seat_team, first.key, first.seat_team
                )));
            }
            if matches!(step.hand_off, HandOff::Gate { .. }) {
                return Err(refused(format!(
                    "it is a gate and declares the document `{document}`; a gate makes no model call, so it writes no document"
                )));
            }
            if matches!(step.hand_off, HandOff::LoopBack { .. }) {
                return Err(refused(format!(
                    "it declares the document `{document}` and loops back; a loop-back step's final message is its reasons, not the document's text, so a document step takes no loop_back"
                )));
            }
            if let Some(schema) = &step.output_schema {
                return Err(refused(format!(
                    "it declares the document `{document}` and the output schema `{}`; the document's version is the step's final message as written (Markdown), so a document step takes no output schema",
                    schema.id
                )));
            }
        }
        // Story 3.22 (review triage round 1, R3): no model and no person
        // sees what a gate would read, so a gate reads no row.
        if matches!(step.hand_off, HandOff::Gate { .. }) && !step.row_inputs.is_empty() {
            return Err(refused(
                "it is a gate and names rows to read; a gate makes no model call, so it reads no row".into(),
            ));
        }
        let Some(table) = &step.table else {
            continue;
        };
        if matches!(step.hand_off, HandOff::Gate { .. }) {
            return Err(refused(format!(
                "it is a gate and declares the table `{}`; a gate makes no model call, so it writes no table",
                table.id
            )));
        }
        if table.team != step.seat_team {
            return Err(refused(format!(
                "it declares the table `{}` of team `{}`, and its seat `{}` is in team `{}`; a seat writes only its own team's tables (AD-21)",
                table.id, table.team, step.seat, step.seat_team
            )));
        }
    }
    Ok(())
}

/// Story 3.4 (AD-8, AD-15, AD-16, AD-36; Question 1): goes on with the run
/// `run_id`, which stopped cleanly (at a gate a person has resolved, or
/// between steps), on `plan`, built from the snapshot the run pinned. Only
/// then: a decision a person committed on the waiting gate and the engine
/// has not applied yet is applied first, as `System(engine)`
/// ([`crate::gate::apply_resolution`]), and the run is driven from the
/// journal as [`run_plan`] drives it, to its end or the next gate.
///
/// Refused before anything is written: an unknown run, or one that has
/// finished ([`RunError::NotContinuable`]); a plan from another snapshot
/// ([`RunError::ConfigChanged`], naming both hashes); a step left
/// `running` (not a resolved gate), a model call unsettled or a tool call
/// in flight (resume is story 3.9: [`RunError::NotContinuable`]). An open
/// waiting gate is no decision: the run still waits, and nothing changes.
pub async fn continue_run(
    store: &Store,
    run_id: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    if plan.steps.is_empty() {
        return Err(RunError::NoSteps(plan.workflow.clone()));
    }
    check_writes(plan)?;
    check_prices(plan)?;
    let schemas = compile_schemas(plan)?;
    let reader = store.reader()?;
    let Some(run) = reader.run(run_id)? else {
        return Err(RunError::NotContinuable(format!(
            "there is no run {run_id} in this project, so there is nothing to continue"
        )));
    };
    if run.finished_at.is_some()
        || matches!(run.status.as_str(), "succeeded" | "failed" | "cancelled")
    {
        return Err(RunError::NotContinuable(format!(
            "run {run_id} has finished ({}), so there is nothing to continue",
            run.status
        )));
    }
    // R6: the plan is the run's own workflow's.
    if plan.workflow != run.workflow {
        return Err(RunError::NotContinuable(format!(
            "run {run_id} is a run of the workflow `{}`, and the plan given is for `{}`; nothing was changed",
            run.workflow, plan.workflow
        )));
    }
    if run.config_hash.as_deref() != Some(plan.config_hash.as_str()) {
        return Err(RunError::ConfigChanged {
            pinned: run.config_hash.unwrap_or_else(|| "no snapshot".into()),
            now: plan.config_hash.clone(),
        });
    }
    let decided = ready_to_go_on(&reader, run_id)?;
    let run_uuid = uuid::Uuid::parse_str(run_id)
        .map_err(|e| StoreError::Invalid(format!("the run id {run_id} is not a UUID: {e}")))?;
    let mut progress = Progress::default();
    if let Some(item) = decided {
        let applied = crate::gate::apply_resolution(&store.writer(), &reader, &item)?;
        progress.reason = applied.reason;
        progress.item = applied.item;
        if applied.run_failed {
            progress.finishing = Some("failed");
        }
    }
    drop(reader);
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    let tooling = Tooling::shipped(store, plan);
    let run = Run {
        store,
        exec: &exec,
        run_id,
        run_uuid,
        plan,
        schemas: &schemas,
        cancel,
        tools: &tooling,
    };
    run.drive_to_outcome(progress).await
}

/// Story 3.4 (review R2): whether the run `run_id` stopped cleanly enough
/// for [`continue_run`], read-only: no step `running` but at most one gate
/// whose Review a person resolved and the engine has not applied yet (its
/// item's id is given back, to apply first), no model call unsettled and
/// no tool call in flight. `agentic gate` and the API's resolve ask it
/// before they write an approve or a return, so a decision is never
/// recorded on a run that cannot go on. Refused in words
/// ([`RunError::NotContinuable`], naming story 3.9's resume).
pub fn ready_to_go_on(reader: &Reader, run_id: &str) -> Result<Option<String>, RunError> {
    let rows = reader.steps(run_id)?;
    let items = reader.inbox_items(run_id)?;
    let resume = |what: String| {
        RunError::NotContinuable(format!(
            "{what}, and going on from there (resume) arrives with story 3.9; nothing was changed"
        ))
    };
    let mut decided = None;
    for row in rows
        .iter()
        .filter(|r| r.status == StepStatus::Running.as_str())
    {
        let resolved = items.iter().find(|item| {
            item.step_run_id == row.id
                && item.kind == InboxKind::Review
                && item.status == "resolved"
        });
        match resolved {
            Some(item) if decided.is_none() => decided = Some(item.id.clone()),
            _ => {
                return Err(resume(format!(
                    "step `{}` attempt {} of run {run_id} was left running",
                    row.step_key, row.attempt
                )));
            }
        }
    }
    if let Some(call) = reader.calls_for_run(run_id)?.iter().find(|c| !c.settled) {
        return Err(resume(format!(
            "the model call {} of run {run_id} never settled",
            call.id
        )));
    }
    for row in &rows {
        if let Some(call) = reader
            .tool_calls(&row.id)?
            .into_iter()
            .find(|t| t.status == "issued")
        {
            return Err(resume(format!(
                "the tool call {} of step `{}` of run {run_id} was left in flight",
                call.id, row.step_key
            )));
        }
    }
    Ok(decided)
}

/// How a run's drive stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Succeeded,
    Failed,
    /// A step waits for a person; the run row stays `running`.
    Waiting,
}

impl Ended {
    fn as_str(self) -> &'static str {
        match self {
            Ended::Succeeded => "succeeded",
            Ended::Failed => "failed",
            Ended::Waiting => StepStatus::WaitingHuman.as_str(),
        }
    }
}

/// How one step attempt ended for the drive loop.
enum StepEnd {
    /// It succeeded or looped back: the loop reads the journal again.
    Moved,
    /// It failed, and the run is finished `failed`.
    Failed,
    /// It waits for a person.
    Waiting,
}

/// The tools of a run: its ToolGate (the shipped one, or the one
/// [`run_plan_with_tools`] built over a runner, the only holder and caller
/// of the runner, AD-11) and the refusals ToolGate audited (written to the
/// store after each check).
struct Tooling {
    gate: ToolGate,
    refusals: Arc<Refusals>,
}

impl Tooling {
    /// The shipped gate (story 3.6): `table_write` through the engine's
    /// row-writer port over `store`, with the plan's row tool.
    fn shipped(store: &Store, plan: &RunPlan) -> Tooling {
        let refusals = Arc::new(Refusals::default());
        let gate = ToolGate::shipped(
            Arc::new(StoreRowWriter::new(store)),
            plan.row_tool.clone(),
            refusals.clone(),
        );
        Tooling { gate, refusals }
    }
}

/// What ToolGate needs of `step` (story 3.6): its seat's grants and team,
/// and the table it declares.
fn step_tools(step: &StepPlan) -> StepTools {
    StepTools {
        grants: step.grants.clone(),
        seat_team: step.seat_team.clone(),
        table: step.table.clone(),
    }
}

/// ToolGate's audit sink: the refusals it records, written to the store by
/// the driver right after the check.
#[derive(Default)]
struct Refusals(Mutex<Vec<AuditEvent>>);

impl AuditSink for Refusals {
    fn record(&self, event: AuditEvent) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).push(event);
    }
}

impl Refusals {
    fn take(&self) -> Vec<AuditEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// Story 3.5 (AD-3, AD-36): each step's output schema, compiled; `None` for
/// a step that declares none. A schema that cannot be compiled refuses the
/// run in words naming the step and the schema's id.
fn compile_schemas(plan: &RunPlan) -> Result<Vec<Option<Compiled>>, RunError> {
    plan.steps
        .iter()
        .map(|step| {
            step.output_schema
                .as_ref()
                .map(|s| {
                    schema::compile(&s.id, &s.schema).map_err(|e| RunError::Schema {
                        step: step.key.clone(),
                        why: e.to_string(),
                    })
                })
                .transpose()
        })
        .collect()
}

/// One run being driven.
struct Run<'a> {
    store: &'a Store,
    exec: Exec<'a>,
    run_id: &'a str,
    run_uuid: uuid::Uuid,
    plan: &'a RunPlan,
    /// The compiled output schema of each plan step, by index.
    schemas: &'a [Option<Compiled>],
    cancel: CancellationToken,
    tools: &'a Tooling,
}

/// The latest attempt of each step key among `rows`.
fn latest(rows: &[StepRow]) -> BTreeMap<&str, &StepRow> {
    let mut out: BTreeMap<&str, &StepRow> = BTreeMap::new();
    for row in rows {
        let keep = out
            .get(row.step_key.as_str())
            .is_none_or(|seen| seen.attempt < row.attempt);
        if keep {
            out.insert(row.step_key.as_str(), row);
        }
    }
    out
}

/// The reasons a loop-back step's final message lists: its JSON object's
/// `field`, a list of strings. Anything else is refused in words.
fn reasons_in(output: &str, field: &str) -> Result<Vec<String>, String> {
    let shape = format!(
        "it declares `loop_back`, so its final message must be a JSON object whose `{field}` field is a list of strings (an empty list moves on)"
    );
    let value: serde_json::Value = serde_json::from_str(output.trim())
        .map_err(|_| format!("{shape}; its final message is not JSON"))?;
    let Some(object) = value.as_object() else {
        return Err(format!("{shape}; its final message is not a JSON object"));
    };
    let Some(list) = object.get(field) else {
        return Err(format!("{shape}; the object has no `{field}` field"));
    };
    let Some(items) = list.as_array() else {
        return Err(format!("{shape}; its `{field}` field is not a list"));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{shape}; an item of its `{field}` field is not text"))
        })
        .collect()
}

/// The reasons as one input's text, one per line.
fn reasons_text(reasons: &[String]) -> String {
    reasons
        .iter()
        .map(|r| format!("- {r}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A step attempt's inputs: the rows its start journals (the previous
/// step's output, then the plan's inputs; a loop-back target's reasons are
/// journaled already, by the loop-back) and the request's parts (trusted
/// first, then fenced; within each, the previous output, the reasons, the
/// plan's inputs), with the attempt's taint.
struct Inputs {
    rows: Vec<StepInput>,
    trusted: Vec<Part>,
    fenced: Vec<Part>,
    taint: Taint,
    /// The previous step's attempt, by `next` (a gate's payload).
    previous: Option<StepRow>,
    /// Why the attempt must not start (story 3.4: a gate note no person
    /// wrote), in words.
    refused: Option<String>,
}

impl Inputs {
    /// An input to journal with the attempt's start, and its part.
    fn add(&mut self, row: StepInput, part: Part) {
        self.place(row.taint, part);
        self.rows.push(row);
    }

    /// The part of an input the journal already holds for the attempt (a
    /// loop-back target's `loop_back` input), with its taint.
    fn place(&mut self, taint: Taint, part: Part) {
        self.taint = self.taint.raise(taint);
        match part {
            Part::Trusted(_) => self.trusted.push(part),
            other => self.fenced.push(other),
        }
    }
}

impl Run<'_> {
    fn exec(&self, actor: Actor, cmd: WriteCommand) -> Result<(), StoreError> {
        (self.exec)(actor, cmd)
    }

    /// Drives the run from `progress` and says how it ended, or where it
    /// waits. A store error once the run exists never leaves it `running`
    /// unsaid (see [`RunError::Stopped`]).
    async fn drive_to_outcome(&self, mut progress: Progress) -> Result<RunOutcome, RunError> {
        match self.drive(&mut progress).await {
            Ok(ended) => {
                // A run that already waited (continued with no decision)
                // names the item it waits on.
                if ended == Ended::Waiting && progress.item.is_none() {
                    progress.item = self.waiting_item()?;
                }
                Ok(RunOutcome {
                    run_id: self.run_id.to_string(),
                    status: ended.as_str().into(),
                    reason: progress.reason,
                    item: progress.item,
                })
            }
            Err(source) => {
                let to_record = progress.to_record();
                // Story 3.7 (owner Q14, DR-109): the open step's open call is
                // marked lost, once, so its reservation settles and none is
                // left open for ever.
                if let Some(step_run_id) = &progress.open_step {
                    self.settle_open_calls(step_run_id);
                }
                if let Some(step_run_id) = progress.open_step {
                    let _ = self.exec(
                        engine(),
                        WriteCommand::FinishStep {
                            step_run_id,
                            status: "failed".into(),
                            output: None,
                        },
                    );
                }
                let recorded = self.exec(
                    engine(),
                    WriteCommand::FinishRun {
                        run_id: self.run_id.to_string(),
                        status: to_record.into(),
                    },
                );
                Err(RunError::Stopped {
                    run_id: self.run_id.to_string(),
                    status: if recorded.is_ok() {
                        to_record
                    } else {
                        "running"
                    }
                    .into(),
                    reason: progress.reason,
                    source: Box::new(source),
                })
            }
        }
    }

    /// Story 3.7 (owner Q14): after a store error, each call of the step
    /// attempt `step_run_id` still open is marked lost (settled at its
    /// reservation). Best effort, like the cleanup's other commands: the
    /// store may refuse them too.
    fn settle_open_calls(&self, step_run_id: &str) {
        let open = self
            .store
            .reader()
            .and_then(|r| r.calls_for_step(step_run_id))
            .map(|calls| calls.into_iter().filter(|c| !c.settled));
        for call in open.into_iter().flatten() {
            let _ = self.exec(engine(), WriteCommand::MarkLost { call_id: call.id });
        }
    }

    /// The open item a waiting step of the run waits on.
    fn waiting_item(&self) -> Result<Option<String>, StoreError> {
        let reader = self.store.reader()?;
        let waiting: Vec<String> = reader
            .steps(self.run_id)?
            .into_iter()
            .filter(|s| s.status == StepStatus::WaitingHuman.as_str())
            .map(|s| s.id)
            .collect();
        Ok(reader
            .inbox_items(self.run_id)?
            .into_iter()
            .find(|i| i.status == "open" && waiting.contains(&i.step_run_id))
            .map(|i| i.id))
    }

    fn steps(&self) -> Result<Vec<StepRow>, StoreError> {
        self.store.reader()?.steps(self.run_id)
    }

    /// Drives the created run: each time, from the journal, the first step
    /// whose latest attempt is `queued`, until every step succeeded, one
    /// failed or one waits.
    async fn drive(&self, progress: &mut Progress) -> Result<Ended, StoreError> {
        loop {
            let rows = self.steps()?;
            let latest = latest(&rows);
            let mut next = None;
            for (i, step) in self.plan.steps.iter().enumerate() {
                let Some(row) = latest.get(step.key.as_str()) else {
                    return Err(StoreError::Invalid(format!(
                        "the journal of run {} holds no step `{}`, which its plan has",
                        self.run_id, step.key
                    )));
                };
                match StepStatus::parse(&row.status) {
                    Some(StepStatus::Succeeded) => continue,
                    Some(StepStatus::Queued) => {
                        next = Some((i, (*row).clone()));
                        break;
                    }
                    Some(StepStatus::WaitingHuman) => return Ok(Ended::Waiting),
                    Some(StepStatus::Failed | StepStatus::Cancelled) => return Ok(Ended::Failed),
                    _ => {
                        return Err(StoreError::Invalid(format!(
                            "step `{}` attempt {} of run {} is {}, and this version does not resume a run from there",
                            row.step_key, row.attempt, self.run_id, row.status
                        )));
                    }
                }
            }
            let Some((i, row)) = next else {
                progress.finishing = Some("succeeded");
                self.finish_run("succeeded")?;
                return Ok(Ended::Succeeded);
            };
            match self.run_step(i, &row, &rows, progress).await? {
                StepEnd::Moved => continue,
                StepEnd::Failed => return Ok(Ended::Failed),
                StepEnd::Waiting => return Ok(Ended::Waiting),
            }
        }
    }

    fn finish_run(&self, status: &str) -> Result<(), StoreError> {
        self.exec(
            engine(),
            WriteCommand::FinishRun {
                run_id: self.run_id.into(),
                status: status.into(),
            },
        )
    }

    /// Step `i`'s inputs for its attempt `row`, from the journal `rows`:
    /// every producer's text and taint as committed (AD-10: never
    /// recomputed).
    fn inputs_of(&self, i: usize, row: &StepRow, rows: &[StepRow]) -> Result<Inputs, StoreError> {
        let mut inputs = Inputs {
            rows: Vec::new(),
            trusted: Vec::new(),
            fenced: Vec::new(),
            taint: Taint::Clean,
            previous: None,
            refused: None,
        };
        let latest = latest(rows);
        // `next`: the previous step's latest attempt, which succeeded; across
        // an approved gate, the output that gate's Review carried (story
        // 3.4, Decision 4: the producer's attempt, with its committed taint).
        if let Some(prev) = i
            .checked_sub(1)
            .and_then(|p| self.previous_output(p, &latest, rows).transpose())
            .transpose()?
        {
            inputs.previous = Some(prev.clone());
            let prev = &prev;
            if let Some(output) = &prev.output {
                let label = format!("step:{}@{}", prev.step_key, prev.attempt);
                let part =
                    TrustedInput::from_step_output(label.clone(), output.clone(), prev.taint);
                inputs.add(
                    StepInput {
                        kind: "step_output".into(),
                        from_step_run: Some(prev.id.clone()),
                        from_item: None,
                        from_row: None,
                        label,
                        taint: prev.taint,
                    },
                    part,
                );
            }
        }
        // A loop-back target's new attempt: the producer's reasons, from the
        // `loop_back` input the loop-back journaled for it (AD-43: the edge
        // is stored once).
        let journaled = self.store.reader()?.step_inputs(&row.id)?;
        for producer in journaled
            .iter()
            .filter(|input| input.kind == "loop_back")
            .filter_map(|input| input.from_step_run.as_ref())
            .filter_map(|id| rows.iter().find(|r| &r.id == id))
        {
            let field = self
                .plan
                .steps
                .iter()
                .find_map(|s| match &s.hand_off {
                    HandOff::LoopBack { field, .. } if s.key == producer.step_key => {
                        Some(field.as_str())
                    }
                    _ => None,
                })
                .unwrap_or("reasons");
            let output = producer.output.clone().unwrap_or_default();
            let text = reasons_in(&output, field)
                .map(|reasons| reasons_text(&reasons))
                .unwrap_or(output);
            let label = format!("loop-back:{}@{}", producer.step_key, producer.attempt);
            let part = TrustedInput::from_step_output(label, text, producer.taint);
            inputs.place(producer.taint, part);
        }
        // Story 3.4 (AD-10 (b), AD-27, AD-43): a returned producer's new
        // attempt receives the person's note, from the `gate_note` input
        // the apply journaled for it: trusted only through
        // `from_owner_note` with the resolver the journal holds, after the
        // other trusted inputs and before the fence. It neither adds nor
        // clears taint.
        for input in journaled.iter().filter(|input| input.kind == "gate_note") {
            match self.gate_note(input)? {
                Ok(note) => inputs.place(Taint::Clean, Part::Trusted(note)),
                Err(why) => {
                    inputs.refused = Some(why);
                    return Ok(inputs);
                }
            }
        }
        // Story 3.22 (AD-10, AD-43): the rows the plan names, read now from
        // their committed after-images through the one read site. Each is a
        // `row` edge and a fenced part; reading any row taints the attempt,
        // whatever the row's mark. A row the table does not hold stops the
        // attempt before it starts, in words naming only store ids.
        let rows_read = &self.plan.steps[i].row_inputs;
        if !rows_read.is_empty() {
            match row_inputs::read_rows(&self.store.reader()?, rows_read)? {
                Ok(reads) => {
                    for read in reads {
                        inputs.taint = inputs.taint.raise(Taint::Tainted);
                        inputs.add(read.input, Part::from(read.part));
                    }
                }
                Err(why) => {
                    inputs.refused = Some(why);
                    return Ok(inputs);
                }
            }
        }
        // The plan's own inputs: untrusted, so every attempt is tainted.
        // Their parts are the assembler's (it fences them last, after the
        // produced ones); only their rows are journaled here.
        for data in &self.plan.steps[i].inputs {
            inputs.taint = inputs.taint.raise(Taint::Tainted);
            inputs.rows.push(StepInput {
                kind: "untrusted".into(),
                from_step_run: None,
                from_item: None,
                from_row: None,
                label: data.label().to_string(),
                taint: Taint::Tainted,
            });
        }
        Ok(inputs)
    }

    /// The output step `p` hands the next step by `next`: its latest
    /// attempt when that succeeded; for an approved gate, the attempt its
    /// Review item carried (story 3.4, Decision 4). `None` when there is
    /// none.
    fn previous_output(
        &self,
        p: usize,
        latest: &BTreeMap<&str, &StepRow>,
        rows: &[StepRow],
    ) -> Result<Option<StepRow>, StoreError> {
        let step = &self.plan.steps[p];
        let Some(prev) = latest
            .get(step.key.as_str())
            .filter(|row| row.status == StepStatus::Succeeded.as_str())
        else {
            return Ok(None);
        };
        if !matches!(step.hand_off, HandOff::Gate { .. }) {
            return Ok(Some((*prev).clone()));
        }
        let payload = self
            .store
            .reader()?
            .inbox_items(self.run_id)?
            .into_iter()
            .find(|item| item.step_run_id == prev.id && item.kind == InboxKind::Review)
            .and_then(|item| item.payload);
        Ok(payload.and_then(|id| rows.iter().find(|r| r.id == id).cloned()))
    }

    /// The trusted part of a `gate_note` input (story 3.4, AD-10 (b)): the
    /// note on the item it names, trusted only through
    /// `TrustedInput::from_owner_note` with the resolver as committed in
    /// `resolved_by`. `Err` (in words) when the item holds no note or no
    /// person resolved it: the attempt must not start.
    fn gate_note(&self, input: &StepInput) -> Result<Result<TrustedInput, String>, StoreError> {
        let Some(item_id) = &input.from_item else {
            return Ok(Err(format!(
                "its gate note `{}` names no Inbox item",
                input.label
            )));
        };
        let Some(item) = self.store.reader()?.inbox_item(item_id)? else {
            return Ok(Err(format!(
                "its gate note names the Inbox item {item_id}, which does not exist"
            )));
        };
        // The resolver as committed (AD-10 (b)): never recomputed.
        let resolver = item.resolver();
        let (Some(resolver), Some(note)) = (resolver, item.note) else {
            return Ok(Err(format!(
                "its gate note's Inbox item {item_id} holds no note from a person"
            )));
        };
        match TrustedInput::from_owner_note(input.label.clone(), note, &resolver) {
            Some(trusted) => Ok(Ok(trusted)),
            None => Ok(Err(format!(
                "its gate note's Inbox item {item_id} was resolved by {}, not by a person, so the note is not trusted and the step does not start (AD-10)",
                resolver.describe()
            ))),
        }
    }

    /// Step `i`'s attempt `row` does not start: the run stops with one
    /// Failure item in the engine's words (`why`), never an input's text,
    /// and the attempt stays queued.
    fn fail_before_start(
        &self,
        step: &StepPlan,
        row: &StepRow,
        why: &str,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        progress.reason = Some(format!("step `{}` did not start: {why}", step.key));
        let item = NewInboxItem {
            id: new_id(),
            kind: InboxKind::Failure,
            payload: None,
            taint: Taint::Clean,
            title: format!("Step {} did not start", step.key),
            body: format!(
                "Step `{}` did not start, so the run failed: {why}. Step run {} stays queued.",
                step.key, row.id
            ),
            max_returns: None,
        };
        let item_id = item.id.clone();
        progress.finishing = Some("failed");
        self.exec(
            engine(),
            WriteCommand::FailBeforeStart {
                step_run_id: row.id.clone(),
                item,
            },
        )?;
        progress.item = Some(item_id);
        Ok(StepEnd::Failed)
    }

    /// Runs step `i`'s queued attempt `row` to its end, or to its gate.
    async fn run_step(
        &self,
        i: usize,
        row: &StepRow,
        rows: &[StepRow],
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        let step = &self.plan.steps[i];
        // Story 3.22 (review triage round 1, R7; AD-8, AD-43): the rows a
        // step reads are read before its start commits, and the store
        // refuses an edge whose after-image is no longer its row's current
        // one (`RowRefused`). The inputs are then read again and the start
        // tried once more; a second refusal fails the step in words.
        let mut tries = 0;
        let (system, user, taint) = loop {
            tries += 1;
            let inputs = self.inputs_of(i, row, rows)?;
            if let Some(why) = &inputs.refused {
                // Fail closed (AD-10): an attempt whose gate note no person
                // wrote, or whose rows cannot be read (a row the table does
                // not hold, or one named twice), never starts (story 3.4,
                // review R5; story 3.22).
                return self.fail_before_start(step, row, why, progress);
            }
            if let HandOff::Gate { max_returns } = step.hand_off {
                return self.pause_at_gate(step, row, inputs, max_returns, progress);
            }
            let Inputs {
                rows: journaled,
                trusted,
                fenced,
                taint,
                ..
            } = inputs;
            // Story 3.5 (AD-27): the one assembler builds the request's parts
            // in AD-27's order, and the journal's record of each, read at the
            // attempt's start (the clock once per try).
            let assembled = prompt::assemble(
                Slots {
                    role: step.system.clone(),
                    prompt: step.prompt.clone(),
                    step: step.instruction.clone(),
                    trusted,
                    produced_untrusted: fenced,
                    plan_inputs: step.inputs.clone(),
                },
                self.plan.clock.now(),
            );
            // Committed before the first call (AD-8): the attempt's start,
            // its inputs, its taint and the prompt it was sent.
            let started = self.exec(
                engine(),
                WriteCommand::StartAttempt {
                    step_run_id: row.id.clone(),
                    inputs: journaled,
                    taint,
                    prompt: assembled.parts,
                    // Story 3.6 (AD-21): the attempt's seat and the one table
                    // its step declares, which the store checks every row
                    // write against.
                    // Story 3.21: and the one document it versions, which the
                    // store checks its finish against.
                    writes: AttemptWrites {
                        seat: step.seat.clone(),
                        table: step.table.as_ref().map(|t| t.id.clone()),
                        document: step.document.clone(),
                    },
                },
            );
            match started {
                Ok(()) => break (assembled.system, assembled.user, taint),
                Err(StoreError::RowRefused(words)) if !step.row_inputs.is_empty() => {
                    if tries < 2 {
                        continue;
                    }
                    return self.fail_before_start(
                        step,
                        row,
                        &format!(
                            "a row it reads changed while it was starting, again after it read its rows a second time ({words})"
                        ),
                        progress,
                    );
                }
                Err(e) => return Err(e),
            }
        };
        progress.open_step = Some(row.id.clone());
        // A fresh attempt's whole cap, from tokio's clock right after its
        // start committed (review P1; story 3.9's resume will count a
        // resumed attempt's persisted `started_at`).
        let deadline = tokio::time::Instant::now() + step.limits.wall_clock;
        let mut messages = vec![Message {
            role: Role::User,
            parts: user,
        }];
        let seat = Actor::Seat {
            seat: step.seat.clone(),
            run: self.run_uuid,
            step: step.key.clone(),
        };
        // The taint the next reservation carries: the attempt's, then
        // tainted once an untrusted tool result is appended (an engine
        // result leaves it as it is).
        let mut carry = taint;
        // Story 3.6 (AD-46): what this step is offered, and ToolGate's view
        // of it.
        let facts = step_tools(step);
        let offered = self.tools.gate.offered(&facts);
        // Story 3.5 (AD-36): the repair call's own parts, once the final
        // message failed its schema and a turn and time remain; `repaired`
        // once that call is reserved (at most one per attempt).
        let mut pending_repair: Option<Vec<NewPromptPart>> = None;
        let mut repaired = false;
        loop {
            // Nothing starts once the deadline has passed (review P7).
            if tokio::time::Instant::now() >= deadline {
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    None,
                    None,
                    Cap::WallClock(step.limits.wall_clock),
                    progress,
                );
            }
            let made = self.store.reader()?.calls_for_step(&row.id)?.len() as i64;
            let call_id = new_id();
            let (purpose, own_parts) = match pending_repair.take() {
                Some(parts) => (CallPurpose::Repair, parts),
                None => (CallPurpose::Turn, Vec::new()),
            };
            let request = ChatRequest {
                model: step.model.clone(),
                system: system.clone(),
                messages: messages.clone(),
                // A repair call offers no tool (story 3.5, review R9): it
                // asks for one message that matches the schema.
                tools: match purpose {
                    CallPurpose::Repair => Vec::new(),
                    CallPurpose::Turn => offered.clone(),
                },
                max_output_tokens: step.max_output_tokens,
            };
            // Story 3.7 (AD-8, AD-9): reserved from this request's counted
            // prompt tokens, before it is sent.
            self.exec(
                engine(),
                self.reservation(
                    step,
                    row,
                    &call_id,
                    made + 1,
                    &request,
                    carry,
                    purpose,
                    own_parts,
                )?,
            )?;
            let (turn, timed_out) = self.call_within(step, request, deadline).await;
            if timed_out {
                let usage = turn.as_ref().ok().and_then(|out| out.usage);
                // Story 3.5 (review R10): a final message in hand that does
                // not match its schema fails as a schema failure, with no
                // time left to repair it. A valid one, a tool request or a
                // cancelled call still loses to the deadline (P7).
                if let Ok(out) = &turn
                    && out.tool_calls.is_empty()
                    && out.stop == StopReason::EndTurn
                    && let Some((id, words)) = self.schema_findings(i, step, &out.text)
                {
                    let cap = if purpose == CallPurpose::Repair {
                        // The one repair was used and did not fix it.
                        Cap::Schema { id, words }
                    } else {
                        Cap::SchemaNoTime {
                            id,
                            words,
                            wall_clock: step.limits.wall_clock,
                        }
                    };
                    return self.fail_with_item(
                        step,
                        row,
                        carry,
                        Some(CallSettlement { call_id, usage }),
                        None,
                        cap,
                        progress,
                    );
                }
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    Some(CallSettlement { call_id, usage }),
                    None,
                    Cap::WallClock(step.limits.wall_clock),
                    progress,
                );
            }
            if let Some(why) = failure_of(step, &turn) {
                let usage = turn.as_ref().ok().and_then(|out| out.usage);
                return self.fail(step, row, Some((call_id, usage)), why, progress);
            }
            let Ok(out) = turn else {
                unreachable!("failure_of names every provider error");
            };
            // Story 3.5 (review R9): a repair call offers no tool, so a tool
            // call in its reply is an invalid final message; the step fails.
            if purpose == CallPurpose::Repair
                && !out.tool_calls.is_empty()
                && let Some(declared) = &step.output_schema
            {
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    Some(CallSettlement {
                        call_id,
                        usage: out.usage,
                    }),
                    None,
                    Cap::Schema {
                        id: declared.id.clone(),
                        words: "its reply asked for a tool, and the repair call offers none".into(),
                    },
                    progress,
                );
            }
            if out.tool_calls.is_empty() {
                // Story 3.5 (AD-36): the final message is checked against
                // the step's schema before anything reads it (a loop-back's
                // reasons included).
                if let (Some(declared), Some(Some(compiled))) =
                    (&step.output_schema, self.schemas.get(i))
                    && let Err(findings) = schema::check(compiled, &out.text)
                {
                    let words = schema::words(&findings);
                    let settle = CallSettlement {
                        call_id: call_id.clone(),
                        usage: out.usage,
                    };
                    if repaired {
                        return self.fail_with_item(
                            step,
                            row,
                            carry,
                            Some(settle),
                            None,
                            Cap::Schema {
                                id: declared.id.clone(),
                                words,
                            },
                            progress,
                        );
                    }
                    // No time left (review R10): the repair is a model call
                    // like any other, and nothing starts past the deadline.
                    if tokio::time::Instant::now() >= deadline {
                        return self.fail_with_item(
                            step,
                            row,
                            carry,
                            Some(settle),
                            None,
                            Cap::SchemaNoTime {
                                id: declared.id.clone(),
                                words,
                                wall_clock: step.limits.wall_clock,
                            },
                            progress,
                        );
                    }
                    // The repair is a model call like any other: the
                    // iteration cap counts it.
                    let made = self.store.reader()?.calls_for_step(&row.id)?.len() as u32;
                    if made >= step.limits.iterations {
                        return self.fail_with_item(
                            step,
                            row,
                            carry,
                            Some(settle),
                            None,
                            Cap::SchemaNoTurn {
                                id: declared.id.clone(),
                                words,
                                iterations: step.limits.iterations,
                            },
                            progress,
                        );
                    }
                    self.exec(
                        engine(),
                        WriteCommand::SettleCall {
                            call_id,
                            usage: out.usage,
                        },
                    )?;
                    // The repair's parts: the invalid reply (the attempt's
                    // own words, carrying the attempt's taint) and the
                    // schema's repair instruction, which am-app built from
                    // config. The validation words go into no prompt.
                    pending_repair = Some(vec![
                        NewPromptPart {
                            slot: PromptSlot::Reply,
                            kind: PromptKind::Assistant,
                            label: format!("reply:{}@{}", step.key, row.attempt),
                            taint: carry,
                            text: Some(out.text.clone()),
                        },
                        NewPromptPart {
                            slot: PromptSlot::Repair,
                            kind: PromptKind::Instruction,
                            label: "repair".into(),
                            taint: Taint::Clean,
                            text: Some(declared.repair.text().to_string()),
                        },
                    ]);
                    repaired = true;
                    // The repair request (review T3): the attempt's own first
                    // user message, the invalid final text, the repair
                    // instruction. Earlier tool turns are not replayed: the
                    // repair call offers no tool, and a provider may refuse a
                    // tool block with none defined.
                    let first = messages[0].clone();
                    messages = vec![
                        first,
                        Message {
                            role: Role::Assistant,
                            parts: vec![Part::AssistantText(out.text)],
                        },
                        Message {
                            role: Role::User,
                            parts: vec![Part::Instruction(declared.repair.clone())],
                        },
                    ];
                    continue;
                }
                return self.end_turn(step, row, call_id, out, carry, progress);
            }
            // The iteration cap, from the journal: a turn that asks for
            // tools needs one more turn, which the cap may not allow.
            let made = self.store.reader()?.calls_for_step(&row.id)?.len() as u32;
            if made >= step.limits.iterations {
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    Some(CallSettlement {
                        call_id,
                        usage: out.usage,
                    }),
                    None,
                    Cap::Iterations(step.limits.iterations),
                    progress,
                );
            }
            self.exec(
                engine(),
                WriteCommand::SettleCall {
                    call_id,
                    usage: out.usage,
                },
            )?;
            let mut results = Vec::new();
            let mut untrusted = false;
            for call in &out.tool_calls {
                match self
                    .tool_call(&facts, step, row, &seat, call, carry, deadline, progress)
                    .await?
                {
                    ToolEnd::Result(content) => {
                        untrusted = true;
                        results.push(Part::ToolResult {
                            call_id: call.id.clone(),
                            content,
                        });
                    }
                    ToolEnd::Engine(result) => results.push(Part::EngineResult {
                        call_id: call.id.clone(),
                        result,
                    }),
                    ToolEnd::Stopped(end) => return Ok(end),
                }
            }
            let mut said = Vec::new();
            if !out.text.is_empty() {
                said.push(Part::AssistantText(out.text.clone()));
            }
            said.extend(out.tool_calls.iter().cloned().map(Part::ToolCall));
            messages.push(Message {
                role: Role::Assistant,
                parts: said,
            });
            messages.push(Message {
                role: Role::User,
                parts: results,
            });
            // Tool results are untrusted (AD-10): the attempt is tainted,
            // committed with the next call's reservation. An engine result
            // (a successful `table_write`, AD-10 as amended 2026-11-24) is
            // built from engine values only and leaves it as it is.
            if untrusted {
                carry = Taint::Tainted;
            }
        }
    }

    /// Story 3.5: the schema's id and the findings in words when `text` does
    /// not match step `i`'s output schema; `None` when it matches or the step
    /// declares none.
    fn schema_findings(&self, i: usize, step: &StepPlan, text: &str) -> Option<(String, String)> {
        let declared = step.output_schema.as_ref()?;
        let compiled = self.schemas.get(i)?.as_ref()?;
        schema::check(compiled, text)
            .err()
            .map(|findings| (declared.id.clone(), schema::words(&findings)))
    }

    /// One model call, cancelled through its own child token when the
    /// wall-clock `deadline` passes (the run's token is untouched); `true`
    /// when that happened. A reply ready at the deadline loses to it.
    async fn call_within(
        &self,
        step: &StepPlan,
        request: ChatRequest,
        deadline: tokio::time::Instant,
    ) -> (Result<ChatOutcome, ProviderError>, bool) {
        let child = self.cancel.child_token();
        let mut call = step.provider.stream(
            request,
            TokenSignal::signal(child.clone()),
            Arc::new(NullSink),
        );
        tokio::select! {
            biased;
            () = tokio::time::sleep_until(deadline) => {
                child.cancel();
                match tokio::time::timeout(CANCEL_GRACE, call).await {
                    Ok(turn) => (turn, true),
                    // A call that does not stop settles at its reservation.
                    Err(_) => (
                        Ok(ChatOutcome {
                            text: String::new(),
                            tool_calls: vec![],
                            usage: None,
                            stop: StopReason::Cancelled,
                        }),
                        true,
                    ),
                }
            }
            turn = &mut call => (turn, false),
        }
    }

    /// One tool call the model asked for: the deadline, the tool-call cap
    /// from the journal, then ToolGate's admission (uses counted per step
    /// run; story 3.6: the seat's grants, the class and, for `table_write`,
    /// the declared table), then the call journaled `issued` before ToolGate
    /// runs it, within the wall-clock `deadline`. The name is journaled and
    /// audited as ToolGate labels it. A successful `table_write` was
    /// finished by the store's row command itself (`succeeded`, its result
    /// kind `engine`): it comes back as the engine's result.
    #[allow(clippy::too_many_arguments)]
    async fn tool_call(
        &self,
        facts: &StepTools,
        step: &StepPlan,
        row: &StepRow,
        seat: &Actor,
        call: &ToolCall,
        carry: Taint,
        deadline: tokio::time::Instant,
        progress: &mut Progress,
    ) -> Result<ToolEnd, StoreError> {
        let tools = self.tools;
        // Nothing starts once the deadline has passed (review P7).
        if tokio::time::Instant::now() >= deadline {
            let end = self.fail_with_item(
                step,
                row,
                carry,
                None,
                None,
                Cap::WallClock(step.limits.wall_clock),
                progress,
            )?;
            return Ok(ToolEnd::Stopped(end));
        }
        // The model's name for the tool, as the journal may hold it; the
        // gate labels the call it is handed itself (once).
        let name = tools.gate.label(&call.name);
        let journaled = self.store.reader()?.tool_calls(&row.id)?;
        let used = journaled.iter().filter(|t| t.status != "refused").count() as u32;
        // AD-8: the call's key is its attempt and its sequence; its row id
        // is the one a row write names.
        let place = CallPlace {
            step_run_id: row.id.clone(),
            call_seq: journaled.len() as i64 + 1,
            tool_call_id: new_id(),
            taint: carry,
        };
        let record = |status: &str| WriteCommand::RecordToolCall {
            tool_call_id: place.tool_call_id.clone(),
            step_run_id: row.id.clone(),
            call_seq: place.call_seq,
            tool: name.clone(),
            status: status.into(),
        };
        if used >= step.limits.tool_calls {
            self.exec(engine(), record("refused"))?;
            let end = self.fail_with_item(
                step,
                row,
                carry,
                None,
                None,
                Cap::ToolCalls(step.limits.tool_calls),
                progress,
            )?;
            return Ok(ToolEnd::Stopped(end));
        }
        let admitted = tools.gate.admit(seat, facts, &place, call);
        for event in tools.refusals.take() {
            self.exec(
                event.actor,
                WriteCommand::Audit {
                    action: event.action,
                    target: event.target,
                    outcome: event.outcome,
                    hash: event.hash,
                    args: event.args,
                },
            )?;
        }
        let permit = match admitted {
            Ok(permit) => permit,
            Err(why) => {
                self.exec(engine(), record("refused"))?;
                let end = self.fail(step, row, None, why, progress)?;
                return Ok(ToolEnd::Stopped(end));
            }
        };
        // Committed before the tool runs (AD-8).
        self.exec(engine(), record("issued"))?;
        let child = self.cancel.child_token();
        let running = tools.gate.run(permit, TokenSignal::signal(child.clone()));
        let result = if name == TABLE_WRITE {
            // The row write commits on the store's writer before the gate
            // answers: its outcome is reported as it is, never raced.
            Some(running.await)
        } else {
            // A tool done at the deadline loses to it (review P7).
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline) => {
                    child.cancel();
                    None
                }
                result = running => Some(result),
            }
        };
        let finish = |status: &str| WriteCommand::FinishToolCall {
            tool_call_id: place.tool_call_id.clone(),
            status: status.into(),
        };
        match result {
            None => {
                self.exec(engine(), finish("failed"))?;
                let end = self.fail_with_item(
                    step,
                    row,
                    carry,
                    None,
                    None,
                    Cap::WallClock(step.limits.wall_clock),
                    progress,
                )?;
                Ok(ToolEnd::Stopped(end))
            }
            // Finished `succeeded`, result kind `engine`, by the row
            // command's own transaction (AD-10 as amended).
            Some(Ok(GateOutput::Engine(result))) => Ok(ToolEnd::Engine(result)),
            Some(Ok(GateOutput::Untrusted(content))) => {
                self.exec(engine(), finish("succeeded"))?;
                Ok(ToolEnd::Result(content))
            }
            Some(Err(GateError::Tool(why))) => {
                self.exec(engine(), finish("failed"))?;
                // The tool's own words are untrusted too (AD-10).
                Ok(ToolEnd::Result(UntrustedData::text(
                    format!("tool:{name}"),
                    why,
                )))
            }
            Some(Err(GateError::Store(why))) => {
                // A call still in flight ends `failed`; one the row command
                // finished stays as it committed.
                let open = self
                    .store
                    .reader()?
                    .tool_calls(&row.id)?
                    .iter()
                    .any(|t| t.id == place.tool_call_id && t.status == "issued");
                if open {
                    self.exec(engine(), finish("failed"))?;
                }
                let why = format!("tool \"{name}\" could not finish: {why}");
                let end = self.fail(step, row, None, why, progress)?;
                Ok(ToolEnd::Stopped(end))
            }
        }
    }

    /// Story 3.7 (AD-9; owner Q1, Q2, Q11, Q15): the command that reserves
    /// the call `call_id` of `request`: the request's counted prompt tokens,
    /// its `max_output_tokens`, the step's price and the run's rate, the
    /// week the run's clock is in (company time) and the seat's team.
    #[allow(clippy::too_many_arguments)]
    fn reservation(
        &self,
        step: &StepPlan,
        row: &StepRow,
        call_id: &str,
        call_seq: i64,
        request: &ChatRequest,
        carry: Taint,
        purpose: CallPurpose,
        prompt: Vec<NewPromptPart>,
    ) -> Result<WriteCommand, StoreError> {
        let price = step.price.clone().ok_or_else(|| {
            StoreError::Invalid(format!(
                "step `{}` calls a model and has no price; nothing was reserved",
                step.key
            ))
        })?;
        let money = &self.plan.money;
        let week =
            am_domain::week::budget_week(self.plan.clock.now(), &money.timezone, money.week_start);
        Ok(WriteCommand::ReserveCall {
            step_run_id: row.id.clone(),
            call_id: call_id.to_string(),
            call_seq,
            price,
            rate: money.rate.clone(),
            counted_input_tokens: crate::count::prompt_tokens(request),
            max_output_tokens: request.max_output_tokens,
            week: week.key(),
            team: step.seat_team.clone(),
            taint: carry,
            purpose,
            prompt,
        })
    }

    /// A turn that ended without asking for tools: its step's hand-off.
    fn end_turn(
        &self,
        step: &StepPlan,
        row: &StepRow,
        call_id: String,
        out: ChatOutcome,
        carry: Taint,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        let HandOff::LoopBack { to, field, max } = &step.hand_off else {
            return self.succeed(step, row, call_id, out, carry, progress);
        };
        let reasons = match reasons_in(&out.text, field) {
            Ok(reasons) => reasons,
            Err(why) => return self.fail(step, row, Some((call_id, out.usage)), why, progress),
        };
        if reasons.is_empty() {
            return self.succeed(step, row, call_id, out, carry, progress);
        }
        // The count is the journal's: this step's attempts that looped back.
        let looped = self
            .steps()?
            .iter()
            .filter(|s| s.step_key == step.key && s.status == StepStatus::LoopedBack.as_str())
            .count() as u32;
        if looped >= *max {
            return self.fail_with_item(
                step,
                row,
                carry,
                Some(CallSettlement {
                    call_id,
                    usage: out.usage,
                }),
                Some(out.text),
                Cap::LoopBack {
                    to: to.clone(),
                    max: *max,
                },
                progress,
            );
        }
        // The settlement, the attempt's end and the new attempts commit
        // together (AD-8).
        self.exec(
            engine(),
            WriteCommand::SettleAndLoopBack {
                call_id,
                usage: out.usage,
                step_run_id: row.id.clone(),
                output: out.text,
                to: to.clone(),
            },
        )?;
        progress.open_step = None;
        Ok(StepEnd::Moved)
    }

    fn succeed(
        &self,
        step: &StepPlan,
        row: &StepRow,
        call_id: String,
        out: ChatOutcome,
        carry: Taint,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        if let Some(document) = &step.document {
            return self.version(step, row, document, call_id, out, carry, progress);
        }
        // The settlement and the step's finish commit together (AD-8).
        self.exec(
            engine(),
            WriteCommand::SettleAndFinishStep {
                call_id,
                usage: out.usage,
                step_run_id: row.id.clone(),
                status: "succeeded".into(),
                output: Some(out.text),
            },
        )?;
        progress.open_step = None;
        Ok(StepEnd::Moved)
    }

    /// Story 3.21 (AD-28, AD-29, AD-43): a document step's success. Its
    /// final message must be a document ([`markdown_refusal`]): otherwise
    /// the step fails with a Failure item and no version. Then the call's
    /// settlement, the step's finish with the Markdown as its output, the
    /// version (numbered by the writer) and one Draft item on the attempt
    /// commit together. The Draft's words are the engine's: the document id,
    /// the version, the step and the run; never the Markdown. Its number is
    /// the one the writer commits: the engine sends the number it read as
    /// next, the writer refuses any other ([`StoreError::StaleVersion`]),
    /// and the engine reads it again once (review round 2).
    #[allow(clippy::too_many_arguments)]
    fn version(
        &self,
        step: &StepPlan,
        row: &StepRow,
        document: &str,
        call_id: String,
        out: ChatOutcome,
        carry: Taint,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        if let Some(words) = markdown_refusal(&out.text) {
            return self.fail_with_item(
                step,
                row,
                carry,
                Some(CallSettlement {
                    call_id,
                    usage: out.usage,
                }),
                None,
                Cap::Document {
                    id: document.to_string(),
                    words,
                },
                progress,
            );
        }
        // Review round 2: the Draft names the number the writer gives the
        // version. The engine reads the next one and sends it as the number
        // it expects; a finish of another run that took it first is refused
        // (`StaleVersion`, nothing written), and the number is read again
        // once. A second refusal fails the step with a Failure item.
        let mut stale = None;
        for _ in 0..2 {
            let next = self
                .store
                .reader()?
                .latest_document_version(document)?
                .map_or(1, |v| v + 1);
            let item = NewInboxItem {
                id: new_id(),
                kind: InboxKind::Draft,
                payload: Some(row.id.clone()),
                taint: carry,
                title: format!("Draft: {document} version {next}"),
                body: format!(
                    "Step `{}` wrote version {next} of {document} in run {}. Drafts only: Agentic Manager never sends it.",
                    step.key, self.run_id
                ),
                max_returns: None,
            };
            let sent = self.exec(
                engine(),
                WriteCommand::SettleAndFinishDocumentStep {
                    call_id: call_id.clone(),
                    usage: out.usage,
                    step_run_id: row.id.clone(),
                    output: out.text.clone(),
                    version_id: new_id(),
                    document_id: document.to_string(),
                    expected_version: next,
                    item,
                },
            );
            match sent {
                Ok(()) => {
                    progress.open_step = None;
                    return Ok(StepEnd::Moved);
                }
                Err(StoreError::StaleVersion { expected, next, .. }) => {
                    stale = Some((expected, next));
                }
                Err(e) => return Err(e),
            }
        }
        let (expected, next) = stale.unwrap_or_default();
        self.fail_with_item(
            step,
            row,
            carry,
            Some(CallSettlement {
                call_id,
                usage: out.usage,
            }),
            None,
            Cap::VersionTaken {
                id: document.to_string(),
                expected,
                next,
            },
            progress,
        )
    }

    /// The step fails for `why` with no Inbox item (story 3.1's failures):
    /// the open call, if any, settles with the step's finish, then the run
    /// finishes `failed`. A failed turn's text is not stored.
    fn fail(
        &self,
        step: &StepPlan,
        row: &StepRow,
        call: Option<(String, Option<am_domain::Usage>)>,
        why: String,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        progress.reason = Some(format!("step `{}` failed: {why}", step.key));
        let finish = match call {
            Some((call_id, usage)) => WriteCommand::SettleAndFinishStep {
                call_id,
                usage,
                step_run_id: row.id.clone(),
                status: "failed".into(),
                output: None,
            },
            None => WriteCommand::FinishStep {
                step_run_id: row.id.clone(),
                status: "failed".into(),
                output: None,
            },
        };
        self.exec(engine(), finish)?;
        progress.open_step = None;
        progress.finishing = Some("failed");
        self.finish_run("failed")?;
        Ok(StepEnd::Failed)
    }

    /// The step fails at a maximum (a cap, or the loop-back maximum): the
    /// call settles (if given), the step fails, its Failure item is
    /// created and the run finishes `failed`, in one transaction. The
    /// item's words are the engine's, never model text.
    #[allow(clippy::too_many_arguments)]
    fn fail_with_item(
        &self,
        step: &StepPlan,
        row: &StepRow,
        carry: Taint,
        settle: Option<CallSettlement>,
        output: Option<String>,
        cap: Cap,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        progress.reason = Some(format!("step `{}` failed: {}", step.key, cap.reason()));
        let item = NewInboxItem {
            id: new_id(),
            kind: InboxKind::Failure,
            payload: Some(row.id.clone()),
            taint: carry,
            title: cap.title(&step.key),
            body: cap.body(&step.key, &row.id),
            max_returns: None,
        };
        let item_id = item.id.clone();
        progress.finishing = Some("failed");
        self.exec(
            engine(),
            WriteCommand::FailWithItem {
                step_run_id: row.id.clone(),
                settle,
                output,
                item,
            },
        )?;
        progress.open_step = None;
        progress.item = Some(item_id);
        Ok(StepEnd::Failed)
    }

    /// A gate step (AD-36): no model call. It moves from `queued` to
    /// `waiting_human` with its inputs and one Review item whose payload is
    /// the previous step's attempt, in one transaction. No model output
    /// decides it; only a Human resolve (story 3.4) moves it on.
    fn pause_at_gate(
        &self,
        step: &StepPlan,
        row: &StepRow,
        inputs: Inputs,
        max_returns: u32,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        let (payload, payload_taint, body) = match &inputs.previous {
            Some(prev) => (
                Some(prev.id.clone()),
                prev.taint,
                format!(
                    "Step `{}` waits for your review of the output of step `{}` (attempt {}, step run {}). The run goes on only when you resolve this item.",
                    step.key, prev.step_key, prev.attempt, prev.id
                ),
            ),
            None => (
                None,
                Taint::Clean,
                format!(
                    "Step `{}` waits for your review. The run goes on only when you resolve this item.",
                    step.key
                ),
            ),
        };
        let item = NewInboxItem {
            id: new_id(),
            kind: InboxKind::Review,
            payload,
            taint: payload_taint.raise(inputs.taint),
            title: format!("Review before step {} goes on", step.key),
            body,
            // Journaled at the pause, so a decision is applied from the
            // journal alone (story 3.4, Decision 3).
            max_returns: Some(max_returns),
        };
        let item_id = item.id.clone();
        self.exec(
            engine(),
            WriteCommand::PauseStep {
                step_run_id: row.id.clone(),
                inputs: inputs.rows,
                taint: inputs.taint,
                item,
            },
        )?;
        progress.item = Some(item_id);
        Ok(StepEnd::Waiting)
    }
}

/// How a tool call ended for the loop.
enum ToolEnd {
    /// Its untrusted result, to append for the next turn.
    Result(UntrustedData),
    /// The engine's result of a row write (story 3.6), to append for the
    /// next turn; it does not taint.
    Engine(am_domain::EngineResult),
    /// The step ended (a cap, a refusal).
    Stopped(StepEnd),
}

/// A maximum a step attempt reached.
enum Cap {
    Iterations(u32),
    ToolCalls(u32),
    WallClock(Duration),
    LoopBack {
        to: String,
        max: u32,
    },
    /// Story 3.5 (AD-36): the final message did not match the step's output
    /// schema after its one repair. `words` are built from the schema.
    Schema {
        id: String,
        words: String,
    },
    /// The final message did not match the schema and the wall clock had
    /// passed, so no time was left to repair it.
    SchemaNoTime {
        id: String,
        words: String,
        wall_clock: Duration,
    },
    /// The final message did not match the schema and the iterations cap
    /// left no turn to repair it.
    SchemaNoTurn {
        id: String,
        words: String,
        iterations: u32,
    },
    /// Story 3.21 (AD-29): a document step's final message cannot be a
    /// version of its document `id`. `words` are the engine's rule
    /// ([`markdown_refusal`]), never the text.
    Document {
        id: String,
        words: String,
    },
    /// Review round 2: twice, another run versioned the document `id`
    /// between the engine's read of the next number and its finish; the
    /// last finish expected `expected`, and the next number was `next`.
    VersionTaken {
        id: String,
        expected: i64,
        next: i64,
    },
}

impl Cap {
    /// Why the step failed, in words that name the maximum and its value.
    fn reason(&self) -> String {
        match self {
            Cap::Iterations(n) => format!(
                "it reached its iterations cap of {n} model turns (limits.iterations) and its last turn asked for more"
            ),
            Cap::ToolCalls(n) => format!(
                "it reached its tool_calls cap of {n} tool calls (limits.tool_calls) and asked for one more"
            ),
            Cap::WallClock(d) => format!(
                "it reached its wall_clock cap of {} (limits.wall_clock); the call in flight was cancelled",
                duration_words(*d)
            ),
            Cap::LoopBack { to, max } => format!(
                "it asked to loop back to step `{to}` again, past its maximum of {max} loop-backs (loop_back.max)"
            ),
            Cap::Schema { id, words } => format!(
                "its final message does not match the output schema `{id}`, and the one repair did not fix it: {words}"
            ),
            Cap::SchemaNoTime {
                id,
                words,
                wall_clock,
            } => format!(
                "its final message does not match the output schema `{id}` ({words}), and there was no time left to repair it: its wall_clock cap of {} (limits.wall_clock) had passed",
                duration_words(*wall_clock)
            ),
            Cap::SchemaNoTurn {
                id,
                words,
                iterations,
            } => format!(
                "its final message does not match the output schema `{id}` ({words}), and its iterations cap of {iterations} model turns (limits.iterations) left no turn to repair it"
            ),
            Cap::Document { id, words } => {
                format!("it declares the document `{id}`, and {words}")
            }
            Cap::VersionTaken { id, expected, next } => format!(
                "another run versioned the document `{id}` while this step finished, twice: it expected version {expected}, and the next was {next}"
            ),
        }
    }

    fn title(&self, key: &str) -> String {
        match self {
            Cap::LoopBack { .. } => format!("Step {key} looped back too often"),
            Cap::Schema { .. } | Cap::SchemaNoTurn { .. } | Cap::SchemaNoTime { .. } => {
                format!("Step {key} did not match its output schema")
            }
            Cap::Document { id, .. } | Cap::VersionTaken { id, .. } => {
                format!("Step {key} wrote no version of {id}")
            }
            _ => format!("Step {key} stopped at a cap"),
        }
    }

    /// The item's body: the step, the maximum and its value, and the step
    /// run it points at; never the model's words.
    fn body(&self, key: &str, step_run: &str) -> String {
        match self {
            Cap::Iterations(n) => format!(
                "Step `{key}` stopped at its iterations cap of {n} model turns (limits.iterations) and failed, so the run failed. Step run {step_run} holds its calls."
            ),
            Cap::ToolCalls(n) => format!(
                "Step `{key}` stopped at its tool_calls cap of {n} tool calls (limits.tool_calls) and failed, so the run failed. Step run {step_run} holds its calls."
            ),
            Cap::WallClock(d) => format!(
                "Step `{key}` stopped at its wall_clock cap of {} (limits.wall_clock) and failed, so the run failed. Step run {step_run} holds its calls.",
                duration_words(*d)
            ),
            Cap::LoopBack { to, max } => format!(
                "Step `{key}` asked to loop back to step `{to}` past its maximum of {max} loop-backs (loop_back.max) and failed, so the run failed. Its reasons are in the output of its last attempt, step run {step_run}."
            ),
            Cap::Schema { id, words } => format!(
                "Step `{key}` ended with a final message that does not match its output schema `{id}`, and the one repair did not fix it, so the step failed and the run failed: {words}. The calls are in step run {step_run}; an invalid message is not stored as the step's output."
            ),
            Cap::SchemaNoTime {
                id,
                words,
                wall_clock,
            } => format!(
                "Step `{key}` ended with a final message that does not match its output schema `{id}` ({words}), and there was no time left to repair it: its wall_clock cap of {} (limits.wall_clock) had passed, so the step failed and the run failed. The call is in step run {step_run}.",
                duration_words(*wall_clock)
            ),
            Cap::SchemaNoTurn {
                id,
                words,
                iterations,
            } => format!(
                "Step `{key}` ended with a final message that does not match its output schema `{id}` ({words}), and its iterations cap of {iterations} model turns (limits.iterations) left no turn to repair it, so the step failed and the run failed. The call is in step run {step_run}."
            ),
            Cap::Document { id, words } => format!(
                "Step `{key}` declares the document `{id}`, and {words}. The step failed and the run failed; no version and no Draft were written. The call is in step run {step_run}; a message that is no document is not stored as the step's output."
            ),
            Cap::VersionTaken { id, expected, next } => format!(
                "Step `{key}` declares the document `{id}`, and twice another run versioned it while this step finished (it expected version {expected}, and the next was {next}). The step failed and the run failed; no version and no Draft were written. The call is in step run {step_run}; run the workflow again."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 3.7: a company in EUR with no price file (no rate, the
    /// snapshot `none`), on UTC with weeks from Monday; the steps here run
    /// on this Mac, so their calls cost 0.
    fn money() -> RunMoney {
        RunMoney {
            rate: Rate::none("EUR"),
            timezone: jiff::tz::TimeZone::UTC,
            week_start: jiff::civil::Weekday::Monday,
        }
    }

    /// A plan with no steps is refused before anything is written.
    #[tokio::test]
    async fn a_plan_with_no_steps_creates_no_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = RunPlan {
            workflow: "wf".into(),
            config_hash: "0".repeat(64),
            steps: vec![],
            clock: am_testkit::FakeClock::at("2026-11-24T10:00:00Z"),
            row_tool: None,
            money: money(),
        };
        let refused = run_plan(
            &store,
            Actor::System {
                component: "test".into(),
            },
            "manual",
            &plan,
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(refused, Err(RunError::NoSteps(wf)) if wf == "wf"));
        assert!(store.reader().unwrap().runs().unwrap().is_empty());
    }

    // -------------------------------------------------------------- store errors
    //
    // The seam (second review of story 3.1): `run_on` takes the function
    // that executes a write command, so these tests run the driver on a
    // real store whose writer refuses the commands a test picks.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use am_store::Writer;
    use am_testkit::{FakeLlm, ScriptedTurn};

    fn owner() -> Actor {
        Actor::Human {
            person: "owner".into(),
            hat: "Owner".into(),
        }
    }

    fn one_step(provider: Arc<dyn LlmProvider>) -> RunPlan {
        RunPlan {
            workflow: "wf".into(),
            config_hash: "0".repeat(64),
            steps: vec![StepPlan {
                key: "only".into(),
                seat: "seat-only".into(),
                system: vec![],
                instruction: Instruction::authored("Do the work."),
                inputs: vec![],
                hand_off: HandOff::Next,
                limits: Limits::default(),
                model: "example-local".into(),
                max_output_tokens: 64,
                provider,
                prompt: StepPrompt::default(),
                output_schema: None,
                grants: Default::default(),
                seat_team: "core".into(),
                table: None,
                document: None,
                row_inputs: vec![],
                price: Some(CallPrice::local()),
            }],
            clock: am_testkit::FakeClock::at("2026-11-24T10:00:00Z"),
            row_tool: None,
            money: money(),
        }
    }

    fn refused() -> StoreError {
        StoreError::Invalid("the disk is full".into())
    }

    /// Runs `plan` on `store` with a writer that refuses every command for
    /// which `refuse` says so; `refuse` is given the command and how many
    /// commands of the same audit action came before it.
    async fn run_refusing(
        store: &Store,
        plan: &RunPlan,
        refuse: impl Fn(&WriteCommand, usize) -> bool + Send + Sync,
    ) -> Result<RunOutcome, RunError> {
        let writer: Writer = store.writer();
        let (finishes, others) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let exec = move |actor: Actor, cmd: WriteCommand| {
            let seen = match &cmd {
                WriteCommand::FinishRun { .. } => finishes.fetch_add(1, Ordering::SeqCst),
                _ => others.fetch_add(1, Ordering::SeqCst),
            };
            if refuse(&cmd, seen) {
                return Err(refused());
            }
            writer.exec(actor, cmd)
        };
        run_on(
            store,
            &exec,
            owner(),
            "manual",
            plan,
            CancellationToken::new(),
            Tooling::shipped(store, plan),
        )
        .await
    }

    /// The last `FinishRun` fails once: every step succeeded, so the
    /// cleanup records what the run earned, `succeeded`, not `failed`, and
    /// the error says that is what the journal holds.
    #[tokio::test]
    async fn a_refused_last_finish_is_retried_with_the_status_the_run_earned() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done")])));

        let stopped = run_refusing(&store, &plan, |cmd, seen| {
            matches!(cmd, WriteCommand::FinishRun { .. }) && seen == 0
        })
        .await
        .expect_err("the store refused a command");

        let RunError::Stopped {
            run_id,
            status,
            reason,
            source,
        } = &stopped
        else {
            panic!("{stopped:?}");
        };
        assert_eq!(status, "succeeded");
        assert_eq!(reason, &None);
        assert!(source.to_string().contains("the disk is full"), "{source}");
        let reader = store.reader().unwrap();
        let run = reader.run(run_id).unwrap().unwrap();
        assert_eq!(run.status, "succeeded");
        assert!(run.finished_at.is_some());
        let steps = reader.steps(run_id).unwrap();
        assert_eq!(steps[0].status, "succeeded");
        assert_eq!(steps[0].output.as_deref(), Some("done"));
    }

    /// The store refuses every `FinishRun`: the run stays `running`, and
    /// the error says so instead of naming a status that was not recorded.
    #[tokio::test]
    async fn a_store_that_refuses_every_finish_leaves_the_run_running_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done")])));

        let stopped = run_refusing(&store, &plan, |cmd, _| {
            matches!(cmd, WriteCommand::FinishRun { .. })
        })
        .await
        .expect_err("the store refused a command");

        let RunError::Stopped { run_id, status, .. } = &stopped else {
            panic!("{stopped:?}");
        };
        assert_eq!(status, "running");
        let run = store.reader().unwrap().run(run_id).unwrap().unwrap();
        assert_eq!(run.status, "running");
        assert_eq!(run.finished_at, None);
    }

    // ------------------------------------------- a version taken first
    //
    // Review round 2 (S3): the Draft names the number the writer commits.
    // The seam is `run_on`'s `exec`: before it sends the engine's document
    // finish it may commit a version of the same document from another run,
    // so the number the engine read is taken between its read and its send.

    /// The document the tests' step versions.
    const NOTE: &str = "note-doc";

    /// `one_step` on `provider`, versioning [`NOTE`].
    fn one_document_step(provider: Arc<dyn LlmProvider>) -> RunPlan {
        let mut plan = one_step(provider);
        plan.steps[0].document = Some(NOTE.into());
        plan
    }

    /// Another run versions [`NOTE`], taking its next number: that number.
    fn take_the_next_version(store: &Store) -> i64 {
        let w = store.writer();
        let engine = Actor::System {
            component: "engine".into(),
        };
        let run = new_id();
        w.exec(
            engine.clone(),
            WriteCommand::CreateRun {
                run_id: run.clone(),
                workflow: "other".into(),
                origin: "manual".into(),
                steps: vec!["other".into()],
                config_hash: None,
            },
        )
        .unwrap();
        let reader = store.reader().unwrap();
        let step = reader.steps(&run).unwrap()[0].id.clone();
        w.exec(
            engine.clone(),
            WriteCommand::StartAttempt {
                step_run_id: step.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
                writes: AttemptWrites {
                    seat: "seat-other".into(),
                    table: None,
                    document: Some(NOTE.into()),
                },
            },
        )
        .unwrap();
        let call = new_id();
        w.exec(
            engine.clone(),
            WriteCommand::ReserveCall {
                step_run_id: step.clone(),
                call_id: call.clone(),
                call_seq: 1,
                price: CallPrice::local(),
                rate: Rate::none("EUR"),
                counted_input_tokens: 0,
                max_output_tokens: 0,
                week: "2026-11-23".into(),
                team: "core".into(),
                taint: Taint::Clean,
                purpose: CallPurpose::Turn,
                prompt: vec![],
            },
        )
        .unwrap();
        let next = reader
            .latest_document_version(NOTE)
            .unwrap()
            .map_or(1, |v| v + 1);
        w.exec(
            engine,
            WriteCommand::SettleAndFinishDocumentStep {
                call_id: call,
                usage: None,
                step_run_id: step.clone(),
                output: "# Another run's note\n".into(),
                version_id: new_id(),
                document_id: NOTE.into(),
                expected_version: next,
                item: NewInboxItem {
                    id: new_id(),
                    kind: InboxKind::Draft,
                    payload: Some(step),
                    taint: Taint::Clean,
                    title: format!("Draft: {NOTE} version {next}"),
                    body: "Another run's Draft.".into(),
                    max_returns: None,
                },
            },
        )
        .unwrap();
        next
    }

    /// Runs `plan` on `store`; before the engine's n-th document finish is
    /// sent, other runs take the next `taken[n]` versions (none past the
    /// list). The outcome and the version each of the engine's finishes
    /// expected.
    async fn run_with_versions_taken(
        store: &Store,
        plan: &RunPlan,
        taken: &[usize],
    ) -> (RunOutcome, Vec<i64>) {
        let writer: Writer = store.writer();
        let sent = Mutex::new(Vec::new());
        let exec = |actor: Actor, cmd: WriteCommand| {
            if let WriteCommand::SettleAndFinishDocumentStep {
                expected_version, ..
            } = &cmd
            {
                let mut sent = sent.lock().unwrap();
                for _ in 0..taken.get(sent.len()).copied().unwrap_or(0) {
                    take_the_next_version(store);
                }
                sent.push(*expected_version);
            }
            writer.exec(actor, cmd)
        };
        let outcome = run_on(
            store,
            &exec,
            owner(),
            "manual",
            plan,
            CancellationToken::new(),
            Tooling::shipped(store, plan),
        )
        .await
        .unwrap();
        let sent = sent.into_inner().unwrap();
        (outcome, sent)
    }

    /// The engine read 1 as the next version; other runs took versions 1
    /// and 2 before the finish was sent. The writer refuses that finish with
    /// nothing written; the engine reads the number again (a fresh read
    /// gives 3, where a blind + 1 would send 2 and be refused again) and
    /// sends 3, and the Draft names version 3, the one committed, never 1
    /// or 2 (review round 3, G2).
    #[tokio::test]
    async fn a_taken_version_is_read_again_and_the_draft_names_the_committed_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_document_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text(
            "# Note\n\nOne line.\n",
        )])));

        let (outcome, sent) = run_with_versions_taken(&store, &plan, &[2]).await;

        assert_eq!(outcome.status, "succeeded", "{outcome:?}");
        assert_eq!(sent, vec![1, 3], "the number is read again, fresh, once");
        let reader = store.reader().unwrap();
        let versions = reader.document_versions(NOTE).unwrap();
        let ours: Vec<_> = versions
            .iter()
            .filter(|v| v.run_id == outcome.run_id)
            .collect();
        assert_eq!(versions.len(), 3, "{versions:#?}");
        assert_eq!(ours.len(), 1, "{versions:#?}");
        assert_eq!(ours[0].version, 3);
        let items = reader.inbox_items(&outcome.run_id).unwrap();
        assert_eq!(items.len(), 1, "{items:#?}");
        assert_eq!(items[0].kind, InboxKind::Draft);
        assert_eq!(
            items[0].payload.as_deref(),
            Some(ours[0].step_run_id.as_str())
        );
        for words in [&items[0].title, &items[0].body] {
            assert!(words.contains("version 3"), "{words}");
            for stale in ["version 1", "version 2"] {
                assert!(!words.contains(stale), "{words}");
            }
        }
        let step = &reader.steps(&outcome.run_id).unwrap()[0];
        assert_eq!(step.status, "succeeded");
        assert!(reader.calls_for_step(&step.id).unwrap()[0].settled);
    }

    /// Another run takes the next version before each of the engine's two
    /// finishes: the second refusal fails the step with one Failure item
    /// (its words the engine's), and the run writes no version and no
    /// Draft.
    #[tokio::test]
    async fn a_version_taken_twice_fails_the_step_with_a_failure_item() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_document_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text(
            "# Note\n\nOne line.\n",
        )])));

        let (outcome, sent) = run_with_versions_taken(&store, &plan, &[1, 1]).await;

        assert_eq!(outcome.status, "failed", "{outcome:?}");
        assert_eq!(sent, vec![1, 2], "read again once, then no more");
        let reader = store.reader().unwrap();
        let versions = reader.document_versions(NOTE).unwrap();
        assert_eq!(versions.len(), 2, "the other runs' two: {versions:#?}");
        assert!(versions.iter().all(|v| v.run_id != outcome.run_id));
        let items = reader.inbox_items(&outcome.run_id).unwrap();
        assert_eq!(
            items.iter().map(|i| i.kind).collect::<Vec<_>>(),
            vec![InboxKind::Failure],
            "{items:#?}"
        );
        assert_eq!(outcome.item.as_deref(), Some(items[0].id.as_str()));
        assert!(items[0].body.contains(NOTE), "{}", items[0].body);
        let step = &reader.steps(&outcome.run_id).unwrap()[0];
        assert_eq!(step.status, "failed");
        assert_eq!(step.output, None);
        assert!(reader.calls_for_step(&step.id).unwrap()[0].settled);
        assert!(
            outcome.reason.as_deref().unwrap_or_default().contains(NOTE),
            "{outcome:?}"
        );
    }

    /// A step failed for the provider's reason, then the store refused a
    /// command (the settlement, or the run's finish): the error keeps the
    /// step's reason beside the store's, and the run is recorded `failed`.
    #[tokio::test]
    async fn a_store_error_after_a_failed_step_keeps_the_steps_reason() {
        type Refuse = fn(&WriteCommand, usize) -> bool;
        let cases: [(&str, Refuse); 2] = [
            ("the settlement", |cmd, _| {
                matches!(cmd, WriteCommand::SettleAndFinishStep { .. })
            }),
            ("the run's first finish", |cmd, seen| {
                matches!(cmd, WriteCommand::FinishRun { .. }) && seen == 0
            }),
        ];
        for (case, refuse) in cases {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(&dir.path().join("state.db")).unwrap();
            let failing = FakeLlm::new(vec![ScriptedTurn::fail(am_domain::ProviderError::Http {
                status: 500,
                body: "upstream is down".into(),
            })]);
            let plan = one_step(Arc::new(failing));

            let stopped = run_refusing(&store, &plan, refuse)
                .await
                .expect_err("the store refused a command");

            let RunError::Stopped {
                run_id,
                status,
                reason,
                source,
            } = &stopped
            else {
                panic!("{case}: {stopped:?}");
            };
            assert_eq!(status, "failed", "{case}");
            let reason = reason.as_deref().expect("the step's reason is kept");
            assert!(
                reason.contains("only") && reason.contains("500"),
                "{case}: {reason}"
            );
            assert!(source.to_string().contains("the disk is full"), "{case}");
            let reader = store.reader().unwrap();
            assert_eq!(
                reader.run(run_id).unwrap().unwrap().status,
                "failed",
                "{case}"
            );
            assert_eq!(reader.steps(run_id).unwrap()[0].status, "failed", "{case}");
        }
    }
}
