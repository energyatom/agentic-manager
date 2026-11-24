//! `am-store`: the SQLite system of record (AD-6, AD-7, AD-8, AD-9, AD-25, AD-32).
//!
//! - One **writer thread** owns the only read-write connection. Every mutation
//!   is a typed [`WriteCommand`] carrying an [`Actor`] and commits together with
//!   its hash-chained audit entry in one transaction. The entry holds ids,
//!   content hashes and redacted arguments the store derives from the
//!   command, never content; the log exports as JSON Lines and an export is
//!   checked with [`verify_jsonl`] (AD-25, see [`audit`]).
//! - **Readers** open `SQLITE_OPEN_READ_ONLY` with `query_only=ON`.
//! - The database runs WAL, `synchronous=FULL`, `foreign_keys=ON`; migrations
//!   are the numbered, embedded, forward-only SQL files in `migrations/`, run
//!   after a `VACUUM INTO` backup and reported; a database newer than the
//!   binary is refused.
//! - [`open_runtime`] opens a project's runtime folder (owner-only, outside
//!   the project; am-app resolves it, AD-32) under an exclusive lock that
//!   names its holder. [`open_runtime_unchanged`] (story 2.1, AD-15) does
//!   the same for a command that must never create, back up or migrate the
//!   database: it is refused in words unless the database is already at
//!   this binary's version. [`legacy::move_legacy`] moves an older binary's
//!   in-project folder there first (story 1.13). [`Store::open`] on a bare
//!   path remains for the spike's callers.
//!
//! The spike keeps the minimal journal: runs, step runs, model calls, the
//! ledger (reserve, then settle), triggers and fires. Since epic 3 story 1 a
//! run pins the hash of the config snapshot it was built from (AD-16) in
//! the transaction that creates it, and the writer stamps when a step
//! started and when a step and a run finished. Config History (the intent
//! rows and History entries of AD-16's apply order) is in
//! [`StoreConfigHistory`].
//!
//! Epic 3 story 3 (AD-8, AD-10, AD-28, AD-43): every step attempt is a
//! step-run row with an `attempt` number and a `taint` flag that only ever
//! rises; the writer enforces AD-8's step transitions (a step starts only
//! from `queued`, finishes only from `running`, and nothing leaves
//! `waiting_human` in this story); a step attempt's inputs are journaled
//! (`step_inputs`) in the transaction that starts it; tool calls are
//! journal rows; and the one Inbox item entity (`inbox_items`, AD-28's
//! seven kinds) is created only by an `Actor::System` command, together
//! with the step change it belongs to, and never changed after.
//!
//! Epic 3 story 4 (AD-8, AD-10, AD-15, AD-28, AD-36, AD-43): only a person
//! resolves an Inbox item, through [`Writer::resolve_inbox_item`], whose
//! actor is a [`HumanActor`] by type; no [`WriteCommand`] resolves. The
//! resolve records the decision on the item and takes AD-8's one Human edge
//! (`waiting_human → running`) in one transaction. The engine then applies
//! the committed decision with [`WriteCommand::ApplyResolution`] (only
//! `Actor::System`), decided from committed rows; the writer refuses any
//! other transition of a gate attempt whose Review a person resolved. A
//! return's note is stored once, on the item; the producer's new attempt
//! refers to it with a `gate_note` input (`step_inputs.from_item`).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;

use am_domain::{Actor, Decision, HumanActor, InboxKind, NewIntent, StepStatus, Taint, Usage};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};

pub mod audit;
mod config_history;
mod fsx;
pub mod legacy;
mod migrations;
pub mod project;
pub mod user;

pub use audit::{ChainBreak, ChainOk, ConfigNote, verify_jsonl};
pub use config_history::StoreConfigHistory;
pub use fsx::stream_copy;
pub use migrations::{AppliedMigration, OpenReport, PRE_MIGRATION_KEPT, SCHEMA_VERSION};
use project::ProjectLock;
pub use project::{LockHolder, lock_holder, lock_is_held, open_runtime, open_runtime_unchanged};

/// The project store's `application_id` (story 2.12, AD-7): stamped on
/// every project database, so this Mac's user store
/// ([`user::USER_APPLICATION_ID`]) never opens as a project store and a
/// project store never opens as the user store.
pub const APPLICATION_ID: i32 = 0x414D_5052;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("the database is version {found}, newer than this app knows ({known}); update the app")]
    TooNew { found: i64, known: i64 },
    #[error(
        "the database has version {found}, so it is not a database this app made; move the file aside and open the project again"
    )]
    NotOurs { found: i64 },
    #[error(
        "the migration from version {from} to {to} failed: {source}; the database stays at version {from}{}",
        migrations::backup_words(.backup)
    )]
    MigrationFailed {
        from: i64,
        to: i64,
        backup: Option<PathBuf>,
        source: Box<StoreError>,
    },
    #[error(
        "the folder {} can be read or entered by other users (mode {mode:o}); run `chmod 700 {}` and open the project again",
        .path.display(),
        .path.display()
    )]
    LoosePermissions { path: PathBuf, mode: u32 },
    #[error(
        "the project is already open in another process ({}); close it there first (lock file {})",
        project::holder_words(.holder),
        .lock.display()
    )]
    Locked {
        lock: PathBuf,
        holder: Option<LockHolder>,
    },
    #[error(
        "the project's old runtime folder {} is still open in another process ({}); close the older app there first",
        .dir.display(),
        project::holder_words(.holder)
    )]
    LegacyLocked {
        dir: PathBuf,
        holder: Option<LockHolder>,
    },
    #[error(
        "moving the project's old runtime folder {} out of the project stopped: the copy of {file} did not match the original (sha256); the old folder is left where it is, untouched; free some disk space or check the disk, then open the project again",
        .dir.display()
    )]
    LegacyUnverified { dir: PathBuf, file: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    /// Story 2.1 (AD-15): [`open_runtime_unchanged`] never migrates.
    #[error(
        "the project's database is at version {found} and this app would migrate it to version {known}, which this command never does; open the project once with `agentic serve --project <folder>` (it backs the database up and migrates it), then run this command again"
    )]
    NeedsMigration { found: i64, known: i64 },
    /// Story 2.1 (AD-15): [`open_runtime_unchanged`] never creates.
    #[error(
        "the project has no database at {} yet, and this command never creates one; open the project once with `agentic serve --project <folder>`, then run this command again",
        .path.display()
    )]
    NoDatabase { path: PathBuf },
    #[error("opening a project is not supported on this platform yet")]
    Unsupported,
    /// Story 2.12: a database carrying another store's `application_id`.
    #[error("{} is {what}, not {expected}; move it aside and try again", .path.display())]
    WrongStore {
        path: PathBuf,
        what: String,
        expected: String,
    },
    /// Story 2.12 (AD-7): a user store a newer app wrote.
    #[error(
        "this Mac's user store {} is at version {found}, newer than this app knows ({known}): a newer version of the app wrote it; use that newer app, or update this one",
        .path.display()
    )]
    UserTooNew {
        path: PathBuf,
        found: i64,
        known: i64,
    },
    /// Story 2.12 (AD-6): `user.lock` stayed held past the bounded wait.
    #[error(
        "this Mac's Accounts are being changed by another process: its lock {} is held and stayed held for {waited_s} s; nothing was written; run the command again when that process is done",
        .lock.display()
    )]
    UserLocked { lock: PathBuf, waited_s: u64 },
    #[error("the writer stopped")]
    WriterGone,
    #[error("{0}")]
    Invalid(String),
    #[error("no pending config intent {0}")]
    NoConfigIntent(String),
}

impl StoreError {
    pub(crate) fn io(context: String, source: std::io::Error) -> StoreError {
        StoreError::Io { context, source }
    }
}

/// UTC RFC 3339 with milliseconds and `Z`.
pub fn ts(t: jiff::Timestamp) -> String {
    t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn now_ts() -> String {
    ts(jiff::Timestamp::now())
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Every mutation the spike needs. Each carries its Actor (AD-24).
#[derive(Debug, Clone)]
pub enum WriteCommand {
    /// A run (`running`) with its steps queued. `config_hash` is the hash
    /// of the `ConfigSnapshot` the run was built from (AD-16: sha256 hex),
    /// pinned here and audited as an argument; `None` only for a run with
    /// no snapshot (the spike's hard-coded workflow).
    CreateRun {
        run_id: String,
        workflow: String,
        origin: String,
        steps: Vec<String>,
        config_hash: Option<String>,
    },
    /// A `queued` step is `running`; stamps its `started_at` (kept if
    /// already set). Any other state is refused in words (AD-8); an unknown
    /// id changes nothing, as in story 3.1 (story 3.18's). It starts the
    /// attempt with no inputs and leaves its taint as it is; the engine
    /// starts an attempt with [`WriteCommand::StartAttempt`].
    StartStep {
        step_run_id: String,
    },
    /// Story 3.3: a `queued` step attempt is `running`, with its inputs
    /// journaled (`step_inputs`) and its taint raised, in one transaction,
    /// before its first model call (AD-8, AD-10, AD-43). The attempt's taint
    /// becomes the highest of what it was, `taint`, each input's and each
    /// input already journaled for it (a loop-back target's); an input's
    /// taint is at least its producer's committed taint, and a producer must
    /// be a step run of the same run that finished with its output
    /// (`succeeded` or `looped_back`). An input's kind decides its producer
    /// and taint: `untrusted` has none and is always tainted, `step_output`
    /// and `loop_back` name one. Refused on a finished run, or while
    /// another step of the run waits for a person. Audited `step.started`
    /// with the taint and the number of inputs, never an input's text.
    StartAttempt {
        step_run_id: String,
        inputs: Vec<StepInput>,
        taint: Taint,
    },
    /// Journal the call and reserve its spend, before the call is made (AD-8, AD-9).
    /// The step must be `running`. `taint` raises the attempt's taint in the
    /// same transaction (a tool result the call carries, AD-10); a clean
    /// value never lowers it. Audited `model_call.reserved` with the taint
    /// the call goes out with (the attempt's, after the raise).
    ReserveCall {
        step_run_id: String,
        call_id: String,
        call_seq: i64,
        reserved_micro_usd: i64,
        rate_snapshot: String,
        taint: Taint,
    },
    /// Settle from provider usage; missing usage settles at the reservation.
    SettleCall {
        call_id: String,
        usage: Option<Usage>,
        cost_micro_usd: Option<i64>,
    },
    /// A call that never settled (crash): marked lost, settled at the reservation.
    MarkLost {
        call_id: String,
    },
    /// The step's final status and output; stamps its `finished_at`.
    FinishStep {
        step_run_id: String,
        status: String,
        output: Option<String>,
    },
    /// Settle a call and finish its step in one transaction, so a crash can
    /// never leave a settled (paid) call under an unfinished step (AD-8).
    /// Stamps the step's `finished_at`.
    SettleAndFinishStep {
        call_id: String,
        usage: Option<Usage>,
        cost_micro_usd: Option<i64>,
        step_run_id: String,
        status: String,
        output: Option<String>,
    },
    /// Story 3.3 (AD-36): settle the producing attempt's call and end the
    /// attempt `looped_back` with `output` (which holds its reasons) stored,
    /// and queue attempt n+1 of every step from the step `to` through the
    /// producer, with the target's new attempt's `loop_back` input (from the
    /// producer, labelled `loop-back:<key>@<attempt>`, with its taint), in
    /// one transaction. `to` must be an earlier step of the run, and no step
    /// from `to` through the producer may wait for a person; anything else
    /// is refused with nothing changed. Old attempts are never rewritten.
    /// Audited `model_call.settled+step.looped_back`.
    SettleAndLoopBack {
        call_id: String,
        usage: Option<Usage>,
        cost_micro_usd: Option<i64>,
        step_run_id: String,
        output: String,
        to: String,
    },
    /// Story 3.3 (AD-8, AD-28): a `running` step is `waiting_human`, with
    /// its inputs and taint (as for [`WriteCommand::StartAttempt`]) and its
    /// Inbox item, in one transaction; a `queued` one (a gate) first starts
    /// (`running`, `started_at` stamped, audited `step.started`) in the
    /// same transaction, as AD-8 has no edge from `queued` to
    /// `waiting_human`. Only an `Actor::System` creates an item. The item's
    /// taint is at least its payload's and its attempt's; its payload is a
    /// finished step run (`succeeded`, `looped_back`, `failed`) or its own
    /// attempt. Refused on a finished run, or while another step of the run
    /// waits. Audited `step.waiting` and `inbox.created`.
    PauseStep {
        step_run_id: String,
        inputs: Vec<StepInput>,
        taint: Taint,
        item: NewInboxItem,
    },
    /// Story 3.3 (AD-36, AD-11): settle the call (if given), end the
    /// `running` step `failed` with `output`, create its Inbox item (a
    /// Failure) and finish the run `failed`, in one transaction (refused
    /// while another step of the run waits). Only an `Actor::System`
    /// creates an item. Audited as the settlement or the
    /// step's finish, `inbox.created` and `run.finished`.
    FailWithItem {
        step_run_id: String,
        settle: Option<CallSettlement>,
        output: Option<String>,
        item: NewInboxItem,
    },
    /// Story 3.3 (AD-8, AD-11): a tool call a `running` step's model asked
    /// for, journaled `issued` before the tool runs or `refused` (by
    /// ToolGate or a cap). Audited `tool_call.recorded`.
    RecordToolCall {
        tool_call_id: String,
        step_run_id: String,
        call_seq: i64,
        tool: String,
        status: String,
    },
    /// Story 3.3: an `issued` tool call ended, `succeeded` or `failed`.
    /// Audited `tool_call.finished`.
    FinishToolCall {
        tool_call_id: String,
        status: String,
    },
    /// Story 3.4, review R5 (AD-10 (b), AD-28): a `queued` step attempt that
    /// must not start (its gate note's item was not resolved by a person)
    /// stays `queued`; its Failure item is created on it (the engine's
    /// words) and the run finishes `failed`, in one transaction. Only an
    /// `Actor::System` creates the item. Audited `inbox.created` and
    /// `run.finished`.
    FailBeforeStart {
        step_run_id: String,
        item: NewInboxItem,
    },
    /// Story 3.4 (AD-8, AD-15, AD-28, AD-36): the engine's follow-up to a
    /// decision a person committed on a gate's Review item, decided from
    /// committed rows, in one transaction; only an `Actor::System` sends
    /// it. The item must be resolved and its gate attempt `running` (so it
    /// is applied once):
    ///
    /// - approve: the gate attempt `running → succeeded`;
    /// - reject: the gate attempt `running → failed`, the run `failed` (no
    ///   Failure item: the Review records the decision);
    /// - return: the number of this run's Review items on attempts of the
    ///   gate's step resolved `return` (this one included), against the
    ///   item's `max_returns`. Within it, the gate attempt is `looped_back`
    ///   and attempt n+1 of every step from the payload's step through the
    ///   gate is queued, the producer's new attempt with its one `gate_note`
    ///   input (from the item, labelled `gate-note:<gate>@<attempt>`, taint
    ///   0). Past it, `past_max` must carry the Failure item: the gate
    ///   attempt fails, the item is created on it and the run fails. A
    ///   `past_max` that disagrees with the journal, either way, is refused.
    ///
    /// Audited `step.finished`, `step.looped_back`, `inbox.created` and
    /// `run.finished` as they apply; never the note.
    ApplyResolution {
        item_id: String,
        past_max: Option<NewInboxItem>,
    },
    /// The run's final status; stamps its `finished_at`. Refused while a
    /// step of the run waits for a person (story 3.3).
    FinishRun {
        run_id: String,
        status: String,
    },
    UpsertTrigger {
        trigger: TriggerRow,
    },
    RecordFire {
        trigger_id: String,
        scheduled_for: String,
        kind: String,
        next_fire: String,
    },
    /// An audit-only entry (refusals, blocked egress). `target` names what
    /// was acted on (ids, root and relative path), never content; `hash` is
    /// the content's sha256 hex when relevant (as `AuditEvent::hash`);
    /// `args` are its named arguments (as `AuditEvent::args`: ids, hosts,
    /// ports, reasons), stored as the entry's `args` next to `sha256`.
    Audit {
        action: String,
        target: String,
        outcome: String,
        hash: Option<String>,
        args: BTreeMap<String, String>,
    },
    /// Config History (AD-16, story 1.8, see [`StoreConfigHistory`]): the
    /// intent row, committed before the file is written. `id` becomes the
    /// History entry's id. Audited `config.intent_opened`.
    BeginConfigIntent {
        id: String,
        intent: NewIntent,
    },
    /// Deletes the intent and inserts its History entry in one transaction,
    /// audited `config.edited`, `config.undone`, `config.restored`,
    /// `config.outside_edit`, `config.migrated` or `config.created`, then
    /// one entry per audit note the intent carried (story 2.1).
    CommitConfigEntry {
        intent_id: String,
    },
    /// Deletes an intent whose file was never written (`config.intent_dropped`).
    DropConfigIntent {
        intent_id: String,
    },
    /// At startup: completes an intent whose file holds its new bytes, as
    /// [`WriteCommand::CommitConfigEntry`] but audited `config.reconciled`.
    ReconcileConfigIntent {
        intent_id: String,
    },
}

impl WriteCommand {
    /// The audit entry's action, target and redacted `args` (AD-25): ids,
    /// content hashes, counts and statuses only. Content fields (a step's
    /// `output`) appear only as their sha256 hex; nothing is copied. The
    /// config History commands build theirs from the intent row in the
    /// transaction instead (`None` here), and so do the story 3.3 commands
    /// whose entries carry a committed taint ([`execute`] builds those).
    fn audit_entry(&self) -> Option<(String, String, Option<Value>)> {
        let output_sha256 = |o: &Option<String>| o.as_deref().map(audit::sha256_hex);
        let tokens = |u: &Option<Usage>| {
            (
                u.map(|u| u.input_tokens as i64),
                u.map(|u| u.output_tokens as i64),
            )
        };
        Some(match self {
            WriteCommand::CreateRun {
                run_id,
                workflow,
                origin,
                steps,
                config_hash,
            } => (
                "run.created".into(),
                run_id.clone(),
                Some(json!({
                    "config_hash": config_hash,
                    "origin": origin,
                    "steps": steps.len(),
                    "workflow": workflow,
                })),
            ),
            WriteCommand::StartStep { .. }
            | WriteCommand::StartAttempt { .. }
            | WriteCommand::SettleAndLoopBack { .. }
            | WriteCommand::PauseStep { .. }
            | WriteCommand::FailWithItem { .. }
            | WriteCommand::FailBeforeStart { .. }
            | WriteCommand::ApplyResolution { .. } => return None,
            WriteCommand::RecordToolCall {
                tool_call_id,
                step_run_id,
                call_seq,
                tool,
                status,
            } => (
                "tool_call.recorded".into(),
                tool_call_id.clone(),
                Some(json!({
                    "call_seq": call_seq,
                    "status": status,
                    "step_run_id": step_run_id,
                    "tool": tool,
                })),
            ),
            WriteCommand::FinishToolCall {
                tool_call_id,
                status,
            } => (
                "tool_call.finished".into(),
                tool_call_id.clone(),
                Some(json!({"status": status})),
            ),
            // Built in `execute`, with the taint the call goes out with.
            WriteCommand::ReserveCall { .. } => return None,
            WriteCommand::SettleCall {
                call_id,
                usage,
                cost_micro_usd,
            } => {
                let (input, output) = tokens(usage);
                (
                    "model_call.settled".into(),
                    call_id.clone(),
                    Some(json!({
                        "cost_micro_usd": cost_micro_usd,
                        "input_tokens": input,
                        "output_tokens": output,
                    })),
                )
            }
            WriteCommand::MarkLost { call_id } => {
                ("model_call.lost".into(), call_id.clone(), Some(json!({})))
            }
            WriteCommand::FinishStep {
                step_run_id,
                status,
                output,
            } => (
                "step.finished".into(),
                step_run_id.clone(),
                Some(json!({"output_sha256": output_sha256(output), "status": status})),
            ),
            WriteCommand::SettleAndFinishStep {
                call_id,
                usage,
                cost_micro_usd,
                step_run_id,
                status,
                output,
            } => {
                let (input, output_tokens) = tokens(usage);
                (
                    "model_call.settled+step.finished".into(),
                    call_id.clone(),
                    Some(json!({
                        "cost_micro_usd": cost_micro_usd,
                        "input_tokens": input,
                        "output_sha256": output_sha256(output),
                        "output_tokens": output_tokens,
                        "status": status,
                        "step_run_id": step_run_id,
                    })),
                )
            }
            WriteCommand::FinishRun { run_id, status } => (
                "run.finished".into(),
                run_id.clone(),
                Some(json!({"status": status})),
            ),
            WriteCommand::UpsertTrigger { trigger: t } => (
                "trigger.saved".into(),
                t.id.clone(),
                Some(json!({
                    "catch_up": t.catch_up,
                    "cron": t.cron,
                    "last_fired": t.last_fired,
                    "next_fire": t.next_fire,
                    "state": t.state,
                    "tz": t.tz,
                })),
            ),
            WriteCommand::RecordFire {
                trigger_id,
                scheduled_for,
                kind,
                next_fire,
            } => (
                "trigger.fired".into(),
                trigger_id.clone(),
                Some(json!({
                    "kind": kind,
                    "next_fire": next_fire,
                    "scheduled_for": scheduled_for,
                })),
            ),
            WriteCommand::BeginConfigIntent { .. }
            | WriteCommand::CommitConfigEntry { .. }
            | WriteCommand::DropConfigIntent { .. }
            | WriteCommand::ReconcileConfigIntent { .. } => return None,
            WriteCommand::Audit {
                action,
                target,
                hash,
                args,
                ..
            } => {
                let mut named: serde_json::Map<String, Value> = args
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                if let Some(h) = hash {
                    named.insert("sha256".into(), Value::String(h.clone()));
                }
                (
                    action.clone(),
                    target.clone(),
                    (!named.is_empty()).then_some(Value::Object(named)),
                )
            }
        })
    }
}

/// One input of a step attempt (AD-43's `step_inputs`; story 3.3): the
/// command's and the read's. `kind` is `step_output` (a `next` input),
/// `loop_back` (a loop-back's reasons), `untrusted` (a plan input) or,
/// since story 3.4, `gate_note` (a person's return note at a gate);
/// `from_step_run` is the producing attempt (`None` for a plan input and a
/// gate note), `from_item` the Inbox item a `gate_note` refers to (`None`
/// otherwise). The text stays where it was written (AD-43: never stored
/// twice): a note stays on its item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInput {
    pub kind: String,
    pub from_step_run: Option<String>,
    pub from_item: Option<String>,
    pub label: String,
    pub taint: Taint,
}

/// An Inbox item to create (AD-28; story 3.3). `payload` is the step run
/// whose output it carries; `title` and `body` are the engine's words,
/// never model text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInboxItem {
    pub id: String,
    pub kind: InboxKind,
    pub payload: Option<String>,
    pub taint: Taint,
    pub title: String,
    pub body: String,
    /// A Review's: how many times its gate may be returned (0 to 5), from
    /// the run's plan (story 3.4); `None` on every other kind.
    pub max_returns: Option<u32>,
}

/// The settlement of a call that ends with a failure
/// ([`WriteCommand::FailWithItem`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSettlement {
    pub call_id: String,
    pub usage: Option<Usage>,
    pub cost_micro_usd: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRow {
    pub id: String,
    pub cron: String,
    pub tz: String,
    /// `once`, `skip` or `all` (AD-18).
    pub catch_up: String,
    /// `active` or `paused:<reason>`.
    pub state: String,
    pub next_fire: Option<String>,
    pub last_fired: Option<String>,
}

type Reply = mpsc::Sender<Result<(), StoreError>>;

/// What crosses the writer channel: a typed command, or the resolve of an
/// Inbox item, which only [`Writer::resolve_inbox_item`] sends (story 3.4:
/// no `WriteCommand` resolves).
pub(crate) enum Command {
    Write(WriteCommand),
    Resolve(Resolve),
}

/// A person's decision on an Inbox item, as [`Writer::resolve_inbox_item`]
/// sends it (crate-private: no other caller can build one).
pub(crate) struct Resolve {
    pub(crate) item_id: String,
    pub(crate) decision: Decision,
    pub(crate) note: Option<String>,
}

type Job = (Actor, Command, Reply);

/// Handle to the single writer. Cloning shares the same writer thread.
#[derive(Clone)]
pub struct Writer {
    inner: Arc<WriterThread>,
}

/// The writer thread, shared by the [`Store`] and every [`Writer`] clone.
/// When the last handle drops, the channel closes and the thread is joined,
/// so the read-write connection (and a project's lock, which the thread owns
/// with it) is released by the time that drop returns.
struct WriterThread {
    tx: Option<mpsc::Sender<Job>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for WriterThread {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Writer {
    /// Execute one command; returns after it committed (durably: synchronous=FULL).
    ///
    /// Every command names its [`Actor`] (AD-24); there is no default:
    ///
    /// ```no_run
    /// # fn f(w: am_store::Writer) -> Result<(), am_store::StoreError> {
    /// let actor = am_domain::Actor::System { component: "docs".into() };
    /// w.exec(actor, am_store::WriteCommand::FinishRun { run_id: "r".into(), status: "succeeded".into() })
    /// # }
    /// ```
    ///
    /// No command resolves an Inbox item: only
    /// [`Writer::resolve_inbox_item`] does, with a [`HumanActor`].
    ///
    /// A command without an Actor does not compile:
    ///
    /// ```compile_fail
    /// # fn f(w: am_store::Writer) -> Result<(), am_store::StoreError> {
    /// w.exec(am_store::WriteCommand::FinishRun { run_id: "r".into(), status: "succeeded".into() })
    /// # }
    /// ```
    ///
    /// and neither does a defaulted one:
    ///
    /// ```compile_fail
    /// # fn f(w: am_store::Writer) -> Result<(), am_store::StoreError> {
    /// w.exec(Default::default(), am_store::WriteCommand::FinishRun { run_id: "r".into(), status: "succeeded".into() })
    /// # }
    /// ```
    pub fn exec(&self, actor: Actor, cmd: WriteCommand) -> Result<(), StoreError> {
        self.send(actor, Command::Write(cmd))
    }

    /// Story 3.4 (AD-8, AD-15, AD-24, AD-25, AD-28): a person's decision on
    /// the Inbox item `item_id`, the one way an item is resolved. In one
    /// transaction: the item becomes `resolved` with `decision`, the note
    /// (a return's or a Question's answer: one line of 1 to 2000
    /// characters, no control character but tab; refused on approve,
    /// reject and dismiss), `resolved_by` (the actor as JSON) and
    /// `resolved_at`; a Review's or a Question's step takes AD-8's one Human
    /// edge, `waiting_human → running`, and nothing else changes; one audit
    /// entry `inbox.resolved` (the kind, the decision, the step run, the
    /// taint and the note's sha256, never its text). Refused with nothing
    /// written: an item resolved already (naming who and when), a decision
    /// that does not fit the item's kind, a kind this version does not
    /// resolve, a note against its rules. The engine applies the decision
    /// afterwards ([`WriteCommand::ApplyResolution`]).
    ///
    /// Its actor is a person by type:
    ///
    /// ```no_run
    /// # fn f(w: am_store::Writer) -> Result<(), am_store::StoreError> {
    /// w.resolve_inbox_item(am_domain::HumanActor::new("owner", "Reviewer"), "i", am_domain::Decision::Approve, None)?;
    /// // A person becomes an `Actor`, one way only.
    /// let _owner: am_domain::Actor = am_domain::HumanActor::new("owner", "Reviewer").into();
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// so the engine (or a seat, a tool, an import, the Assistant) does not
    /// compile there:
    ///
    /// ```compile_fail
    /// # fn f(w: am_store::Writer) -> Result<(), am_store::StoreError> {
    /// w.resolve_inbox_item(am_domain::Actor::System { component: "engine".into() }, "i", am_domain::Decision::Approve, None)
    /// # }
    /// ```
    ///
    /// no `Actor` becomes a `HumanActor`:
    ///
    /// ```compile_fail
    /// # fn f(actor: am_domain::Actor) {
    /// let _h: am_domain::HumanActor = actor.into();
    /// # }
    /// ```
    ///
    /// and no `WriteCommand` resolves, so [`Writer::exec`] cannot, with any
    /// actor (kept from story 3.3):
    ///
    /// ```compile_fail
    /// # fn f(w: am_store::Writer) -> Result<(), am_store::StoreError> {
    /// let owner = am_domain::Actor::Human { person: "owner".into(), hat: "Reviewer".into() };
    /// w.exec(owner, am_store::WriteCommand::ResolveInboxItem { item_id: "i".into() })
    /// # }
    /// ```
    pub fn resolve_inbox_item(
        &self,
        by: HumanActor,
        item_id: &str,
        decision: Decision,
        note: Option<String>,
    ) -> Result<(), StoreError> {
        self.send(
            Actor::from(by),
            Command::Resolve(Resolve {
                item_id: item_id.to_string(),
                decision,
                note,
            }),
        )
    }

    /// Sends one job to the writer thread; returns after it committed (or
    /// was refused).
    fn send(&self, actor: Actor, cmd: Command) -> Result<(), StoreError> {
        let (rtx, rrx) = mpsc::channel();
        self.inner
            .tx
            .as_ref()
            .ok_or(StoreError::WriterGone)?
            .send((actor, cmd, rtx))
            .map_err(|_| StoreError::WriterGone)?;
        rrx.recv().map_err(|_| StoreError::WriterGone)?
    }
}

pub struct Store {
    path: PathBuf,
    writer: Writer,
}

fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

impl Store {
    /// Open (creating and migrating if needed) and start the writer thread.
    /// A project opens through [`open_runtime`]; this bare-path open is kept
    /// for the spike's callers. The AD-7 rules apply here too: before an
    /// existing database (version >= 1) is migrated, its `VACUUM INTO` backup
    /// is written to the `backups/pre-migration/` folder `RuntimeLayout`
    /// names beside `path`.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let backups = am_domain::RuntimeLayout::at(dir.to_path_buf()).backups_pre_migration();
        Store::open_reporting(path, &backups, None).map(|(store, _)| store)
    }

    /// [`Store::open`], returning what the migrations did; pre-migration
    /// backups go to `backups`. A project's `lock` moves into the writer
    /// thread and is released only when the read-write connection closes.
    pub(crate) fn open_reporting(
        path: &Path,
        backups: &Path,
        lock: Option<ProjectLock>,
    ) -> Result<(Store, OpenReport), StoreError> {
        fsx::create_private_file_if_missing(path)
            .map_err(|e| StoreError::io(format!("cannot create {}", path.display()), e))?;
        let mut conn = Connection::open(path)?;
        // Refuse another store's database, a newer one (or a foreign one)
        // before anything, even the journal mode, is written.
        refuse_other_store(path, migrations::application_id(&conn)?)?;
        migrations::refuse_newer(migrations::user_version(&conn)?)?;
        configure(&conn)?;
        let report = migrations::migrate(&mut conn, backups)?;
        let (tx, rx) = mpsc::channel::<Job>();
        let handle = thread::Builder::new()
            .name("am-store-writer".into())
            .spawn(move || {
                let mut conn = conn;
                while let Ok((actor, cmd, reply)) = rx.recv() {
                    let r = match &cmd {
                        Command::Write(cmd) => apply(&mut conn, &actor, cmd),
                        Command::Resolve(resolve) => apply_resolve(&mut conn, &actor, resolve),
                    };
                    let _ = reply.send(r);
                }
                // The connection closes first, then the project lock goes.
                drop(conn);
                drop(lock);
            })
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        Ok((
            Store {
                path: path.to_path_buf(),
                writer: Writer {
                    inner: Arc::new(WriterThread {
                        tx: Some(tx),
                        handle: Some(handle),
                    }),
                },
            },
            report,
        ))
    }

    pub fn writer(&self) -> Writer {
        self.writer.clone()
    }

    /// A new read-only connection.
    pub fn reader(&self) -> Result<Reader, StoreError> {
        Reader::open(&self.path)
    }
}

/// Refuses a database at `path` that is newer than this binary (or not one
/// this app made), reading it through [`open_read_only`]; nothing is
/// created or written, not even a `-wal` or `-shm` file. An absent file passes (the resolver's check before
/// any write, AD-32).
pub fn check_database(path: &Path) -> Result<(), StoreError> {
    if !path.exists() {
        return Ok(());
    }
    let conn = open_read_only(path)?;
    refuse_other_store(path, migrations::application_id(&conn)?)?;
    migrations::refuse_newer(migrations::user_version(&conn)?)
}

/// A project store refuses a database stamped with another
/// `application_id`: this Mac's user store above all (story 2.12).
pub(crate) fn refuse_other_store(path: &Path, id: i32) -> Result<(), StoreError> {
    let what = match id {
        0 | APPLICATION_ID => return Ok(()),
        user::USER_APPLICATION_ID => "this Mac's user store (its Accounts records, user.db)",
        _ => "not a database this app made",
    };
    Err(StoreError::WrongStore {
        path: path.to_path_buf(),
        what: what.into(),
        expected: "a project's database".into(),
    })
}

/// A database opened read-only without writing beside it. With no `-wal`
/// file (a clean close) it is opened `immutable`, so SQLite creates no
/// `-wal` or `-shm` file; with one, a plain read-only connection reads the
/// WAL's pages too.
pub(crate) fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if path.with_file_name(format!("{name}-wal")).exists() {
        return Connection::open_with_flags(path, flags);
    }
    let mut uri = String::from("file:");
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3f"),
            '#' => uri.push_str("%23"),
            c => uri.push(c),
        }
    }
    uri.push_str("?immutable=1");
    Connection::open_with_flags(uri, flags | OpenFlags::SQLITE_OPEN_URI)
}

/// A content hash is exactly 64 lowercase hex characters (sha256), so
/// nothing else can reach `args` through it.
fn is_sha256_hex(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn apply(conn: &mut Connection, actor: &Actor, cmd: &WriteCommand) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    execute(&tx, actor, cmd)?;
    tx.commit()?;
    Ok(())
}

/// A person's resolve (story 3.4), in its own transaction with its audit
/// entry; refused, with nothing written, unless `actor` is `Actor::Human`.
fn apply_resolve(conn: &mut Connection, actor: &Actor, r: &Resolve) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let now = now_ts();
    let (action, target, args) = resolve_item(&tx, actor, r, &now)?;
    audit::append(&tx, actor, &action, &target, "ok", args.as_ref(), &now)?;
    tx.commit()?;
    Ok(())
}

/// The one command executor (story 2.12, AD-6): checks the command, runs it
/// and appends its audit entries in the caller's transaction. The project
/// store's writer thread and the user store's guard ([`user::UserStore`])
/// both call it, so the two stores record intents, History and audit alike.
pub(crate) fn execute(
    tx: &rusqlite::Transaction,
    actor: &Actor,
    cmd: &WriteCommand,
) -> Result<(), StoreError> {
    // A `config.*` entry comes only with a config intent (its History
    // entry and the notes it carried, in one transaction): a free audit
    // entry cannot write one, so none can pass for an owner's confirmation.
    if let WriteCommand::Audit { action, .. } = cmd
        && action.starts_with("config.")
    {
        return Err(StoreError::Invalid(format!(
            "refused audit entry: a `{action}` entry is written only with a config intent"
        )));
    }
    if let WriteCommand::Audit { hash: Some(h), .. } = cmd
        && !is_sha256_hex(h)
    {
        return Err(StoreError::Invalid(
            "refused audit entry: `hash` must be 64 lowercase hex characters (sha256)".into(),
        ));
    }
    // The pin is audited as an argument (AD-25), so it is a hash or nothing.
    if let WriteCommand::CreateRun {
        config_hash: Some(h),
        ..
    } = cmd
        && !is_sha256_hex(h)
    {
        return Err(StoreError::Invalid(
            "refused run: `config_hash` must be 64 lowercase hex characters (sha256)".into(),
        ));
    }
    // AD-28: only the app itself creates an Inbox item, before anything.
    if matches!(
        cmd,
        WriteCommand::PauseStep { .. }
            | WriteCommand::FailWithItem { .. }
            | WriteCommand::FailBeforeStart { .. }
    ) && !matches!(actor, Actor::System { .. })
    {
        return Err(StoreError::Invalid(format!(
            "refused: only the app's engine creates an Inbox item (AD-28); {} cannot, and nothing was written",
            actor.describe()
        )));
    }
    // Story 3.4 (AD-15): only the app itself applies a decision a person
    // committed; nobody else moves a resolved gate on.
    if matches!(cmd, WriteCommand::ApplyResolution { .. }) && !matches!(actor, Actor::System { .. })
    {
        return Err(StoreError::Invalid(format!(
            "refused: only the app's engine applies a decision a person made on an Inbox item (AD-15); {} cannot, and nothing was written",
            actor.describe()
        )));
    }
    // Story 3.4: a gate attempt whose Review a person resolved moves on only
    // by the engine's apply of that decision, never by another transition.
    match cmd {
        WriteCommand::FinishStep { step_run_id, .. }
        | WriteCommand::SettleAndFinishStep { step_run_id, .. }
        | WriteCommand::SettleAndLoopBack { step_run_id, .. }
        | WriteCommand::PauseStep { step_run_id, .. }
        | WriteCommand::FailWithItem { step_run_id, .. } => {
            no_decision_on(tx, step_run_id)?;
        }
        _ => {}
    }
    let now = now_ts();
    let mut config_entry = None;
    // The entries of the story 3.3 commands, built from what committed.
    let mut built: Option<Vec<Entry>> = None;
    // The audit notes a committed or reconciled config intent carried
    // (story 2.1): written right after its own entry, in this transaction,
    // each with the intent's own Actor.
    let mut config_notes = Vec::new();
    match cmd {
        WriteCommand::CreateRun {
            run_id,
            workflow,
            origin,
            steps,
            config_hash,
        } => {
            tx.execute(
                "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash) VALUES (?1, ?2, ?3, 'running', ?4, ?4, ?5)",
                params![run_id, workflow, origin, now, config_hash],
            )?;
            for (i, s) in steps.iter().enumerate() {
                tx.execute(
                    "INSERT INTO step_runs (id, run_id, step_key, seq, status, updated_at) VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
                    params![new_id(), run_id, s, i as i64, now],
                )?;
            }
        }
        WriteCommand::StartStep { step_run_id } => {
            // A known step starts as a clean attempt with no inputs; an
            // unknown id changes nothing and is audited as before (story
            // 3.18 owns commands that change no row). Its entry is 3.1's.
            let known: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM step_runs WHERE id = ?1)",
                params![step_run_id],
                |r| r.get(0),
            )?;
            if known {
                start_attempt(tx, step_run_id, &[], Taint::Clean, &now)?;
            }
            built = Some(vec![(
                "step.started".into(),
                step_run_id.clone(),
                Some(json!({})),
            )]);
        }
        WriteCommand::StartAttempt {
            step_run_id,
            inputs,
            taint,
        } => {
            built = Some(vec![start_attempt(tx, step_run_id, inputs, *taint, &now)?]);
        }
        WriteCommand::ReserveCall {
            step_run_id,
            call_id,
            call_seq,
            reserved_micro_usd,
            rate_snapshot,
            taint,
        } => {
            let step = step_of(tx, step_run_id)?;
            require(&step, &[StepStatus::Running], "reserves a model call")?;
            raise_taint(tx, step_run_id, *taint, &now)?;
            // The taint the call goes out with: the attempt's, after the
            // raise (story 3.3, review P16).
            let carried = step.taint.raise(*taint);
            let run_id = step.run_id;
            tx.execute(
                "INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, created_at) VALUES (?1, ?2, ?3, 'issued', 0, ?4)",
                params![call_id, step_run_id, call_seq, now],
            )?;
            tx.execute(
                "INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, state, rate_snapshot, created_at) VALUES (?1, ?2, ?3, 'model', ?4, 'reserved', ?5, ?6)",
                params![new_id(), call_id, run_id, reserved_micro_usd, rate_snapshot, now],
            )?;
            built = Some(vec![(
                "model_call.reserved".into(),
                call_id.clone(),
                Some(json!({
                    "call_seq": call_seq,
                    "rate_snapshot": rate_snapshot,
                    "reserved_micro_usd": reserved_micro_usd,
                    "step_run_id": step_run_id,
                    "taint": flag(carried),
                })),
            )]);
        }
        WriteCommand::SettleCall {
            call_id,
            usage,
            cost_micro_usd,
        } => settle(tx, call_id, *usage, *cost_micro_usd)?,
        WriteCommand::MarkLost { call_id } => {
            let n = tx.execute(
                "UPDATE model_calls SET status = 'lost', settled = 1 WHERE id = ?1 AND settled = 0",
                params![call_id],
            )?;
            // Only an open call is marked lost; a settled call keeps its cost.
            if n == 1 {
                tx.execute(
                    "UPDATE ledger SET settled_micro_usd = reserved_micro_usd, state = 'settled' WHERE model_call_id = ?1",
                    params![call_id],
                )?;
            }
        }
        WriteCommand::FinishStep {
            step_run_id,
            status,
            output,
        } => {
            require(
                &step_of(tx, step_run_id)?,
                &[StepStatus::Running],
                "finishes",
            )?;
            finish_step(tx, step_run_id, status, output, &now)?;
        }
        WriteCommand::SettleAndFinishStep {
            call_id,
            usage,
            cost_micro_usd,
            step_run_id,
            status,
            output,
        } => {
            require(
                &step_of(tx, step_run_id)?,
                &[StepStatus::Running],
                "finishes",
            )?;
            call_of_step(tx, call_id, step_run_id)?;
            settle(tx, call_id, *usage, *cost_micro_usd)?;
            finish_step(tx, step_run_id, status, output, &now)?;
        }
        WriteCommand::SettleAndLoopBack {
            call_id,
            usage,
            cost_micro_usd,
            step_run_id,
            output,
            to,
        } => {
            let step = step_of(tx, step_run_id)?;
            require(&step, &[StepStatus::Running], "loops back")?;
            call_of_step(tx, call_id, step_run_id)?;
            settle(tx, call_id, *usage, *cost_micro_usd)?;
            let queued = loop_back(tx, &step, Some(output), to, Back::LoopBack, &now)?;
            let (input, output_tokens) = (
                usage.map(|u| u.input_tokens as i64),
                usage.map(|u| u.output_tokens as i64),
            );
            built = Some(vec![(
                "model_call.settled+step.looped_back".into(),
                call_id.clone(),
                Some(json!({
                    "cost_micro_usd": cost_micro_usd,
                    "input_tokens": input,
                    "output_sha256": audit::sha256_hex(output),
                    "output_tokens": output_tokens,
                    "queued": queued,
                    "step_run_id": step_run_id,
                    "to": to,
                })),
            )]);
        }
        WriteCommand::PauseStep {
            step_run_id,
            inputs,
            taint,
            item,
        } => {
            let step = step_of(tx, step_run_id)?;
            require(
                &step,
                &[StepStatus::Queued, StepStatus::Running],
                "waits for a person",
            )?;
            run_open_for(tx, &step)?;
            let mut entries = Vec::new();
            // AD-8 has no edge from `queued` to `waiting_human`: a queued
            // step starts first (its inputs, taint and `started_at`), then
            // waits, in this one transaction (story 3.3, review P3).
            let journaled = if step.status == StepStatus::Queued {
                entries.push(start_attempt(tx, step_run_id, inputs, *taint, &now)?);
                0
            } else {
                let (raised, journaled) = journal_inputs(tx, &step, inputs, *taint, &now)?;
                raise_taint(tx, step_run_id, raised, &now)?;
                journaled
            };
            let attempt = step_of(tx, step_run_id)?;
            tx.execute(
                "UPDATE step_runs SET status = 'waiting_human', updated_at = ?2 WHERE id = ?1",
                params![step_run_id, now],
            )?;
            entries.push((
                "step.waiting".into(),
                step_run_id.clone(),
                Some(json!({
                    "inputs": journaled,
                    "item": item.id,
                    "taint": flag(attempt.taint),
                })),
            ));
            entries.push(create_item(tx, &attempt, item, &now)?);
            built = Some(entries);
        }
        WriteCommand::FailWithItem {
            step_run_id,
            settle: settlement,
            output,
            item,
        } => {
            let step = step_of(tx, step_run_id)?;
            require(&step, &[StepStatus::Running], "fails")?;
            let first = match settlement {
                Some(c) => {
                    call_of_step(tx, &c.call_id, step_run_id)?;
                    settle(tx, &c.call_id, c.usage, c.cost_micro_usd)?;
                    finish_step(tx, step_run_id, "failed", output, &now)?;
                    WriteCommand::SettleAndFinishStep {
                        call_id: c.call_id.clone(),
                        usage: c.usage,
                        cost_micro_usd: c.cost_micro_usd,
                        step_run_id: step_run_id.clone(),
                        status: "failed".into(),
                        output: output.clone(),
                    }
                }
                None => {
                    finish_step(tx, step_run_id, "failed", output, &now)?;
                    WriteCommand::FinishStep {
                        step_run_id: step_run_id.clone(),
                        status: "failed".into(),
                        output: output.clone(),
                    }
                }
            };
            let created = create_item(tx, &step, item, &now)?;
            let finished = WriteCommand::FinishRun {
                run_id: step.run_id.clone(),
                status: "failed".into(),
            };
            finish_run(tx, &step.run_id, "failed", &now)?;
            built = Some(
                [first.audit_entry(), Some(created), finished.audit_entry()]
                    .into_iter()
                    .flatten()
                    .collect(),
            );
        }
        WriteCommand::RecordToolCall {
            tool_call_id,
            step_run_id,
            call_seq,
            tool,
            status,
        } => {
            require(
                &step_of(tx, step_run_id)?,
                &[StepStatus::Running],
                "calls a tool",
            )?;
            if !matches!(status.as_str(), "issued" | "refused") {
                return Err(StoreError::Invalid(format!(
                    "refused: a tool call is recorded `issued` or `refused`, not `{status}`"
                )));
            }
            tx.execute(
                "INSERT INTO tool_calls (id, step_run_id, call_seq, tool, status, created_at, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?5 = 'refused' THEN ?6 END)",
                params![tool_call_id, step_run_id, call_seq, tool, status, now],
            )?;
        }
        WriteCommand::FinishToolCall {
            tool_call_id,
            status,
        } => {
            if !matches!(status.as_str(), "succeeded" | "failed") {
                return Err(StoreError::Invalid(format!(
                    "refused: a tool call ends `succeeded` or `failed`, not `{status}`"
                )));
            }
            let n = tx.execute(
                "UPDATE tool_calls SET status = ?2, finished_at = ?3 WHERE id = ?1 AND status = 'issued'",
                params![tool_call_id, status, now],
            )?;
            if n != 1 {
                return Err(StoreError::Invalid(format!(
                    "refused: tool call {tool_call_id} is not an issued call"
                )));
            }
        }
        WriteCommand::ApplyResolution { item_id, past_max } => {
            built = Some(apply_resolution(tx, item_id, past_max.as_ref(), &now)?);
        }
        WriteCommand::FailBeforeStart { step_run_id, item } => {
            let step = step_of(tx, step_run_id)?;
            require(&step, &[StepStatus::Queued], "fails before it starts")?;
            run_open_for(tx, &step)?;
            if item.kind != InboxKind::Failure {
                return Err(StoreError::Invalid(format!(
                    "refused: a step that does not start raises a Failure item, not a {}; nothing was changed",
                    item.kind.title()
                )));
            }
            let created = create_item(tx, &step, item, &now)?;
            finish_run(tx, &step.run_id, "failed", &now)?;
            built = Some(vec![
                created,
                (
                    "run.finished".into(),
                    step.run_id.clone(),
                    Some(json!({"status": "failed"})),
                ),
            ]);
        }
        WriteCommand::FinishRun { run_id, status } => {
            finish_run(tx, run_id, status, &now)?;
        }
        WriteCommand::UpsertTrigger { trigger: t } => {
            tx.execute(
                "INSERT INTO triggers (id, cron, tz, catch_up, state, next_fire, last_fired) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET cron = ?2, tz = ?3, catch_up = ?4, state = ?5, next_fire = ?6, last_fired = ?7",
                params![t.id, t.cron, t.tz, t.catch_up, t.state, t.next_fire, t.last_fired],
            )?;
        }
        WriteCommand::RecordFire {
            trigger_id,
            scheduled_for,
            kind,
            next_fire,
        } => {
            tx.execute(
                "INSERT INTO fires (trigger_id, scheduled_for, fired_at, kind) VALUES (?1, ?2, ?3, ?4)",
                params![trigger_id, scheduled_for, now, kind],
            )?;
            tx.execute(
                "UPDATE triggers SET last_fired = ?2, next_fire = ?3 WHERE id = ?1",
                params![trigger_id, scheduled_for, next_fire],
            )?;
        }
        WriteCommand::Audit { .. } => {}
        WriteCommand::BeginConfigIntent { id, intent } => {
            config_entry = Some(config_history::begin(tx, actor, id, intent, &now)?);
        }
        WriteCommand::CommitConfigEntry { intent_id } => {
            let (entry, notes) = config_history::complete(tx, intent_id, false, &now)?;
            config_entry = Some(entry);
            config_notes = notes;
        }
        WriteCommand::ReconcileConfigIntent { intent_id } => {
            let (entry, notes) = config_history::complete(tx, intent_id, true, &now)?;
            config_entry = Some(entry);
            config_notes = notes;
        }
        WriteCommand::DropConfigIntent { intent_id } => {
            config_entry = Some(config_history::drop_intent(tx, intent_id)?);
        }
    }
    let entries = match (config_entry, built) {
        (Some(entry), _) => vec![entry],
        (None, Some(entries)) => entries,
        (None, None) => cmd.audit_entry().into_iter().collect(),
    };
    if entries.is_empty() {
        return Err(StoreError::Invalid("the command has no audit entry".into()));
    }
    let outcome = match cmd {
        WriteCommand::Audit { outcome, .. } => outcome.clone(),
        _ => "ok".to_string(),
    };
    for (action, target, args) in &entries {
        audit::append(tx, actor, action, target, &outcome, args.as_ref(), &now)?;
    }
    for (asked, (action, target, args)) in &config_notes {
        audit::append(tx, asked, action, target, "ok", args.as_ref(), &now)?;
    }
    Ok(())
}

fn settle(
    tx: &rusqlite::Transaction,
    call_id: &str,
    usage: Option<Usage>,
    cost_micro_usd: Option<i64>,
) -> Result<(), StoreError> {
    let n = tx.execute(
        "UPDATE model_calls SET status = 'settled', settled = 1, input_tokens = ?2, output_tokens = ?3 WHERE id = ?1 AND settled = 0",
        params![
            call_id,
            usage.map(|u| u.input_tokens as i64),
            usage.map(|u| u.output_tokens as i64)
        ],
    )?;
    if n != 1 {
        return Err(StoreError::Invalid(format!("call {call_id} is not open")));
    }
    match cost_micro_usd {
        Some(c) => tx.execute(
            "UPDATE ledger SET settled_micro_usd = ?2, state = 'settled' WHERE model_call_id = ?1",
            params![call_id, c],
        )?,
        None => tx.execute(
            "UPDATE ledger SET settled_micro_usd = reserved_micro_usd, state = 'settled' WHERE model_call_id = ?1",
            params![call_id],
        )?,
    };
    Ok(())
}

/// One audit entry: action, target and redacted `args`.
type Entry = (String, String, Option<Value>);

/// A taint as its journal column holds it.
fn flag(taint: Taint) -> i64 {
    i64::from(taint.is_tainted())
}

/// The journal column's 0 or 1 as a taint.
fn taint_of(flag: i64) -> Taint {
    Taint::of_flag(flag != 0)
}

/// What the writer reads of one step attempt before it changes it.
struct StepState {
    id: String,
    run_id: String,
    step_key: String,
    seq: i64,
    attempt: i64,
    status: StepStatus,
    taint: Taint,
}

fn step_of(tx: &rusqlite::Transaction, step_run_id: &str) -> Result<StepState, StoreError> {
    let row: Option<(String, String, i64, i64, String, i64)> = tx
        .query_row(
            "SELECT run_id, step_key, seq, attempt, status, taint FROM step_runs WHERE id = ?1",
            params![step_run_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((run_id, step_key, seq, attempt, status, taint)) = row else {
        return Err(StoreError::Invalid(format!("step {step_run_id} not found")));
    };
    let status = StepStatus::parse(&status).ok_or_else(|| {
        StoreError::Invalid(format!(
            "step run {step_run_id} has the unknown state `{status}`"
        ))
    })?;
    Ok(StepState {
        id: step_run_id.to_string(),
        run_id,
        step_key,
        seq,
        attempt,
        status,
        taint: taint_of(taint),
    })
}

/// AD-8's transition table: `step` moves only from one of `from`; anything
/// else is refused in words naming its state, and nothing changes.
fn require(step: &StepState, from: &[StepStatus], what: &str) -> Result<(), StoreError> {
    if from.contains(&step.status) {
        return Ok(());
    }
    let states: Vec<&str> = from.iter().map(|s| s.as_str()).collect();
    let mut words = format!(
        "refused: step run {} ({} attempt {}) is {}, and a step {what} only from {} (AD-8); nothing was changed",
        step.id,
        step.step_key,
        step.attempt,
        step.status.as_str(),
        states.join(" or ")
    );
    if step.status == StepStatus::WaitingHuman {
        words.push_str(". A step leaves waiting_human only when a person resolves its Inbox item");
    }
    Err(StoreError::Invalid(words))
}

/// The call `call_id` belongs to the step run `step_run_id`.
fn call_of_step(
    tx: &rusqlite::Transaction,
    call_id: &str,
    step_run_id: &str,
) -> Result<(), StoreError> {
    let owner: Option<String> = tx
        .query_row(
            "SELECT step_run_id FROM model_calls WHERE id = ?1",
            params![call_id],
            |r| r.get(0),
        )
        .optional()?;
    match owner {
        Some(o) if o == step_run_id => Ok(()),
        Some(_) => Err(StoreError::Invalid(format!(
            "refused: call {call_id} is not a call of step run {step_run_id}"
        ))),
        None => Err(StoreError::Invalid(format!("call {call_id} is not open"))),
    }
}

/// Raises the step run's taint to at least `taint` (AD-10: it only rises).
fn raise_taint(
    tx: &rusqlite::Transaction,
    step_run_id: &str,
    taint: Taint,
    now: &str,
) -> Result<(), StoreError> {
    if taint.is_tainted() {
        tx.execute(
            "UPDATE step_runs SET taint = 1, updated_at = ?2 WHERE id = ?1 AND taint = 0",
            params![step_run_id, now],
        )?;
    }
    Ok(())
}

/// The kinds of input stories 3.3 and 3.4 journal (AD-43; story 3.17 adds
/// the rest).
const INPUT_KINDS: [&str; 4] = ["step_output", "loop_back", "untrusted", "gate_note"];

/// Journals `inputs` of the attempt `step` and returns the attempt's taint
/// (the highest of its own, `taint` and each input's) and how many rows
/// were written. An input from a producer carries at least the producer's
/// committed taint; a producer must be a step run of the same run.
fn journal_inputs(
    tx: &rusqlite::Transaction,
    step: &StepState,
    inputs: &[StepInput],
    taint: Taint,
    now: &str,
) -> Result<(Taint, usize), StoreError> {
    let mut attempt = step.taint.raise(taint);
    for input in inputs {
        if !INPUT_KINDS.contains(&input.kind.as_str()) {
            return Err(StoreError::Invalid(format!(
                "refused: `{}` is not a kind of step input ({})",
                input.kind,
                INPUT_KINDS.join(", ")
            )));
        }
        // Story 3.4: only a gate note comes from an Inbox item, and it is
        // checked on its own (AD-10 (b): a person's return, never tainted).
        if input.kind == "gate_note" {
            gate_note_input(tx, step, input)?;
            tx.execute(
                "INSERT INTO step_inputs (step_run_id, kind, from_step_run, from_item, label, taint, created_at) VALUES (?1, 'gate_note', NULL, ?2, ?3, 0, ?4)",
                params![step.id, input.from_item, input.label, now],
            )?;
            continue;
        }
        if input.from_item.is_some() {
            return Err(StoreError::Invalid(format!(
                "refused: a `{}` input comes from no Inbox item (only a `gate_note` does); nothing was changed",
                input.kind
            )));
        }
        // The kind decides the producer and the taint (review R6): a plan
        // input is untrusted, always tainted and has no producer; a step's
        // output and a loop-back's reasons come from a producer.
        let mut input_taint = input.taint;
        match (input.kind.as_str(), &input.from_step_run) {
            ("untrusted", None) => input_taint = Taint::Tainted,
            ("untrusted", Some(_)) => {
                return Err(StoreError::Invalid(
                    "refused: an `untrusted` input comes from no step run (a step's output is a `step_output` input); nothing was changed".into(),
                ));
            }
            (kind, None) => {
                return Err(StoreError::Invalid(format!(
                    "refused: a `{kind}` input names the step run that produced it; nothing was changed"
                )));
            }
            (_, Some(from)) => {
                input_taint = input_taint.raise(finished_producer(tx, from, &step.run_id)?);
            }
        }
        attempt = attempt.raise(input_taint);
        tx.execute(
            "INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![step.id, input.kind, input.from_step_run, input.label, flag(input_taint), now],
        )?;
    }
    Ok((attempt, inputs.len()))
}

/// Story 3.4 (AD-10 (b), AD-43): a `gate_note` input of the attempt `step`
/// names, by `from_item`, a Review item of the same run that a person
/// (`Actor::Human`, as committed in `resolved_by`) resolved `return`; it
/// has no producing step run and no taint. Anything else is refused.
fn gate_note_input(
    tx: &rusqlite::Transaction,
    step: &StepState,
    input: &StepInput,
) -> Result<(), StoreError> {
    let refuse = |why: &str| {
        Err(StoreError::Invalid(format!(
            "refused: a `gate_note` input {why}; a gate note is a person's return note at a gate, read from its Review item (AD-10 (b)); nothing was changed"
        )))
    };
    if input.from_step_run.is_some() {
        return refuse("comes from no step run");
    }
    if input.taint.is_tainted() {
        return refuse("is never tainted (the note is a person's own words)");
    }
    let Some(item_id) = &input.from_item else {
        return refuse("names the Inbox item that carries the note");
    };
    let Some(item) = item_of(tx, item_id)? else {
        return refuse(&format!(
            "names the Inbox item {item_id}, which does not exist"
        ));
    };
    if item.run_id != step.run_id {
        return refuse(&format!(
            "names the Inbox item {item_id}, which is an item of another run"
        ));
    }
    if item.kind != InboxKind::Review {
        return refuse(&format!(
            "names the Inbox item {item_id}, which is not a Review"
        ));
    }
    if item.status != "resolved" || item.decision != Some(Decision::Return) {
        return refuse(&format!(
            "names the Inbox item {item_id}, which was not resolved `return`"
        ));
    }
    let human = item
        .resolved_by
        .as_deref()
        .and_then(|by| serde_json::from_str::<Actor>(by).ok())
        .is_some_and(|by| matches!(by, Actor::Human { .. }));
    if !human {
        return refuse(&format!(
            "names the Inbox item {item_id}, which no person resolved"
        ));
    }
    Ok(())
}

/// The committed taint of `step_run_id`, which must be a step run of
/// `run_id`.
fn producer_in_run(
    tx: &rusqlite::Transaction,
    step_run_id: &str,
    run_id: &str,
) -> Result<Taint, StoreError> {
    let found: Option<i64> = tx
        .query_row(
            "SELECT taint FROM step_runs WHERE id = ?1 AND run_id = ?2",
            params![step_run_id, run_id],
            |r| r.get(0),
        )
        .optional()?;
    found.map(taint_of).ok_or_else(|| {
        StoreError::Invalid(format!(
            "refused: {step_run_id} is not a step run of run {run_id}"
        ))
    })
}

/// The committed taint of `step_run_id`, a step run of `run_id` that has
/// finished with its output (`succeeded`, or `looped_back` with its
/// reasons): an input never comes from an attempt still queued, running or
/// waiting (story 3.3, review P16).
fn finished_producer(
    tx: &rusqlite::Transaction,
    step_run_id: &str,
    run_id: &str,
) -> Result<Taint, StoreError> {
    let taint = producer_in_run(tx, step_run_id, run_id)?;
    let producer = step_of(tx, step_run_id)?;
    if !matches!(
        producer.status,
        StepStatus::Succeeded | StepStatus::LoopedBack
    ) {
        return Err(StoreError::Invalid(format!(
            "refused: step run {step_run_id} ({} attempt {}) is {}, and an input comes only from a step run that has finished with its output; nothing was changed",
            producer.step_key,
            producer.attempt,
            producer.status.as_str()
        )));
    }
    Ok(taint)
}

/// The run of `step` is open to a step starting or pausing (review R4):
/// it has not finished, and no other step of it waits for a person.
fn run_open_for(tx: &rusqlite::Transaction, step: &StepState) -> Result<(), StoreError> {
    let (status, finished): (String, Option<String>) = tx.query_row(
        "SELECT status, finished_at FROM runs WHERE id = ?1",
        params![step.run_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if finished.is_some() || matches!(status.as_str(), "succeeded" | "failed" | "cancelled") {
        return Err(StoreError::Invalid(format!(
            "refused: run {} has finished ({status}), so no step of it starts or waits; nothing was changed",
            step.run_id
        )));
    }
    let waiting: Option<String> = tx
        .query_row(
            "SELECT step_key FROM step_runs WHERE run_id = ?1 AND status = 'waiting_human' AND id <> ?2 LIMIT 1",
            params![step.run_id, step.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(key) = waiting {
        return Err(StoreError::Invalid(format!(
            "refused: step `{key}` of run {} waits for a person (waiting_human), so no other step of the run starts or waits until its Inbox item is resolved; nothing was changed",
            step.run_id
        )));
    }
    match unapplied_decision(tx, &step.run_id, Some(&step.id))? {
        Some((key, item)) => Err(StoreError::Invalid(format!(
            "refused: a person resolved Inbox item {item} of step `{key}` of run {}, and the engine has not applied that decision yet, so no other step of the run starts or waits; nothing was changed",
            step.run_id
        ))),
        None => Ok(()),
    }
}

/// Story 3.4: a gate attempt of `run_id` (other than `except`) that is
/// `running` because a person resolved its Review, before the engine
/// applied the decision; its step key and the item's id.
fn unapplied_decision(
    tx: &rusqlite::Transaction,
    run_id: &str,
    except: Option<&str>,
) -> Result<Option<(String, String)>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT s.step_key, i.id FROM inbox_items i JOIN step_runs s ON s.id = i.step_run_id
             WHERE i.run_id = ?1 AND i.kind = 'review' AND i.status = 'resolved' AND s.status = 'running' AND s.id IS NOT ?2 LIMIT 1",
            params![run_id, except],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Story 3.4 (AD-15): the step attempt `step_run_id` has no Review item a
/// person resolved: such a gate attempt moves on only by the engine's
/// apply of that decision ([`WriteCommand::ApplyResolution`]).
fn no_decision_on(tx: &rusqlite::Transaction, step_run_id: &str) -> Result<(), StoreError> {
    let decided: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT id, decision FROM inbox_items WHERE step_run_id = ?1 AND kind = 'review' AND status = 'resolved' LIMIT 1",
            params![step_run_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match decided {
        Some((item, decision)) => Err(StoreError::Invalid(format!(
            "refused: a person resolved the Review item {item} of step run {step_run_id} `{}`, and only the engine's apply of that decision moves the gate on (AD-15); nothing was changed",
            decision.unwrap_or_default()
        ))),
        None => Ok(()),
    }
}

/// No step of `run_id` waits for a person: a run does not finish while
/// one does (story 3.3, review P16; only a Human resolve moves it on).
fn no_step_waits(tx: &rusqlite::Transaction, run_id: &str) -> Result<(), StoreError> {
    let waiting: Option<String> = tx
        .query_row(
            "SELECT step_key FROM step_runs WHERE run_id = ?1 AND status = 'waiting_human' LIMIT 1",
            params![run_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(key) = waiting {
        return Err(StoreError::Invalid(format!(
            "refused: step `{key}` of run {run_id} is waiting_human, so the run does not finish until a person resolves its Inbox item; nothing was changed"
        )));
    }
    // Story 3.4: nor before the engine applied a decision a person made.
    match unapplied_decision(tx, run_id, None)? {
        Some((key, item)) => Err(StoreError::Invalid(format!(
            "refused: a person resolved Inbox item {item} of step `{key}` of run {run_id}, and the run does not finish before the engine applies that decision; nothing was changed"
        ))),
        None => Ok(()),
    }
}

/// Starts the `queued` attempt `step_run_id` with its inputs and taint;
/// its `step.started` entry. The attempt's taint is at least that of every
/// input already journaled for it (a loop-back target's `loop_back` input).
fn start_attempt(
    tx: &rusqlite::Transaction,
    step_run_id: &str,
    inputs: &[StepInput],
    taint: Taint,
    now: &str,
) -> Result<Entry, StoreError> {
    let step = step_of(tx, step_run_id)?;
    require(&step, &[StepStatus::Queued], "starts")?;
    run_open_for(tx, &step)?;
    let journaled_before: i64 = tx.query_row(
        "SELECT COALESCE(MAX(taint), 0) FROM step_inputs WHERE step_run_id = ?1",
        params![step_run_id],
        |r| r.get(0),
    )?;
    let taint = taint.raise(taint_of(journaled_before));
    let (taint, journaled) = journal_inputs(tx, &step, inputs, taint, now)?;
    tx.execute(
        "UPDATE step_runs SET status = 'running', taint = MAX(taint, ?3), started_at = COALESCE(started_at, ?2), updated_at = ?2 WHERE id = ?1",
        params![step_run_id, now, flag(taint)],
    )?;
    Ok((
        "step.started".into(),
        step_run_id.to_string(),
        Some(json!({"inputs": journaled, "taint": flag(taint)})),
    ))
}

/// What the target's new attempt of a loop-back receives: the producer's
/// reasons (story 3.3), or a person's return note at a gate (story 3.4),
/// referred to on its Review item.
#[derive(Clone, Copy)]
enum Back<'a> {
    LoopBack,
    GateNote { item: &'a str },
}

/// Ends the producing attempt `looped_back` with `output` and queues
/// attempt n+1 of every step from `to` through the producer; returns how
/// many attempts were queued. The target's new attempt gets its `loop_back`
/// input (from the producer, with the producer's taint) here, so the edge
/// is journaled once (AD-43). A loop-back never crosses a step that waits
/// for a person.
fn loop_back(
    tx: &rusqlite::Transaction,
    producer: &StepState,
    output: Option<&str>,
    to: &str,
    back: Back<'_>,
    now: &str,
) -> Result<usize, StoreError> {
    let target: Option<i64> = tx
        .query_row(
            "SELECT seq FROM step_runs WHERE run_id = ?1 AND step_key = ?2 LIMIT 1",
            params![producer.run_id, to],
            |r| r.get(0),
        )
        .optional()?;
    let Some(target_seq) = target else {
        return Err(StoreError::Invalid(format!(
            "refused: run {} has no step `{to}` to loop back to; nothing was changed",
            producer.run_id
        )));
    };
    if target_seq >= producer.seq {
        return Err(StoreError::Invalid(format!(
            "refused: step `{to}` is not earlier than step `{}`, and a loop-back returns to an earlier step (AD-36); nothing was changed",
            producer.step_key
        )));
    }
    let waiting: Option<String> = tx
        .query_row(
            "SELECT step_key FROM step_runs WHERE run_id = ?1 AND seq BETWEEN ?2 AND ?3 AND status = 'waiting_human' LIMIT 1",
            params![producer.run_id, target_seq, producer.seq],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(key) = waiting {
        return Err(StoreError::Invalid(format!(
            "refused: step `{key}` waits for a person (waiting_human), and a loop-back from step `{}` to step `{to}` would pass it; nothing was changed",
            producer.step_key
        )));
    }
    tx.execute(
        "UPDATE step_runs SET status = 'looped_back', output = ?2, finished_at = ?3, updated_at = ?3 WHERE id = ?1",
        params![producer.id, output, now],
    )?;
    let mut st = tx.prepare(
        "SELECT step_key, seq, MAX(attempt) FROM step_runs WHERE run_id = ?1 AND seq BETWEEN ?2 AND ?3 GROUP BY step_key, seq ORDER BY seq",
    )?;
    let rows: Vec<(String, i64, i64)> = st
        .query_map(params![producer.run_id, target_seq, producer.seq], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    for (key, seq, attempt) in &rows {
        let id = new_id();
        tx.execute(
            "INSERT INTO step_runs (id, run_id, step_key, seq, attempt, status, taint, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', 0, ?6)",
            params![id, producer.run_id, key, seq, attempt + 1, now],
        )?;
        if key == to {
            match back {
                Back::LoopBack => tx.execute(
                    "INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at) VALUES (?1, 'loop_back', ?2, ?3, ?4, ?5)",
                    params![
                        id,
                        producer.id,
                        format!("loop-back:{}@{}", producer.step_key, producer.attempt),
                        flag(producer.taint),
                        now
                    ],
                )?,
                // Story 3.4 (AD-10 (b), AD-43): the note stays on its item;
                // the input refers to it, untainted, through the same
                // checks as any `gate_note` input (review R3).
                Back::GateNote { item } => {
                    let input = StepInput {
                        kind: "gate_note".into(),
                        from_step_run: None,
                        from_item: Some(item.to_string()),
                        label: format!("gate-note:{}@{}", producer.step_key, producer.attempt),
                        taint: Taint::Clean,
                    };
                    gate_note_input(tx, &step_of(tx, &id)?, &input)?;
                    tx.execute(
                        "INSERT INTO step_inputs (step_run_id, kind, from_step_run, from_item, label, taint, created_at) VALUES (?1, 'gate_note', NULL, ?2, ?3, 0, ?4)",
                        params![id, item, input.label, now],
                    )?
                }
            };
        }
    }
    Ok(rows.len())
}

/// Inserts the Inbox item of the attempt `step` (status `open`), with a
/// taint at least its payload's and its attempt's (`step` as committed);
/// its `inbox.created` entry (no words).
fn create_item(
    tx: &rusqlite::Transaction,
    step: &StepState,
    item: &NewInboxItem,
    now: &str,
) -> Result<Entry, StoreError> {
    let mut taint = item.taint.raise(step_of(tx, &step.id)?.taint);
    if let Some(payload) = &item.payload {
        taint = taint.raise(producer_in_run(tx, payload, &step.run_id)?);
        // A payload's taint can no longer rise once it finished (review
        // R5): a finished step run, or the item's own attempt.
        let carried = step_of(tx, payload)?;
        let finished = matches!(
            carried.status,
            StepStatus::Succeeded | StepStatus::LoopedBack | StepStatus::Failed
        );
        if payload != &step.id && !finished {
            return Err(StoreError::Invalid(format!(
                "refused: an item's payload is a finished step run or its own attempt, and step run {payload} ({} attempt {}) is {}; nothing was changed",
                carried.step_key,
                carried.attempt,
                carried.status.as_str()
            )));
        }
    }
    // Story 3.4: a Review carries its gate's `max_returns` (0 to 5), so a
    // decision can be applied from the journal alone; no other kind has one.
    match (item.kind, item.max_returns) {
        (InboxKind::Review, Some(m)) if m <= 5 => {}
        (InboxKind::Review, Some(m)) => {
            return Err(StoreError::Invalid(format!(
                "refused: a Review item's max_returns is 0 to 5, not {m}; nothing was changed"
            )));
        }
        (InboxKind::Review, None) => {
            return Err(StoreError::Invalid(
                "refused: a Review item carries its gate's max_returns; nothing was changed".into(),
            ));
        }
        (_, Some(_)) => {
            return Err(StoreError::Invalid(format!(
                "refused: only a Review item carries max_returns, not a {}; nothing was changed",
                item.kind.title()
            )));
        }
        (_, None) => {}
    }
    tx.execute(
        "INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns) VALUES (?1, ?2, 'open', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            item.id,
            item.kind.as_str(),
            step.run_id,
            step.id,
            item.payload,
            flag(taint),
            item.title,
            item.body,
            now,
            item.max_returns
        ],
    )?;
    Ok((
        "inbox.created".into(),
        item.id.clone(),
        Some(json!({
            "kind": item.kind.as_str(),
            "step_run_id": step.id,
            "taint": flag(taint),
        })),
    ))
}

/// The longest note a person may give (characters, not bytes; story 3.4).
pub const NOTE_MAX_CHARS: usize = 2000;

/// Story 3.4 (row 25; review R7): a return's note and a Question's answer
/// are one line of 1 to 2000 characters with no control character but tab
/// and no format, separator or tag character; approve, reject and dismiss
/// carry none.
fn check_note(decision: Decision, note: Option<&str>) -> Result<(), StoreError> {
    let refuse = |why: String| {
        Err(StoreError::Invalid(format!(
            "refused: {why}; nothing was written"
        )))
    };
    if !decision.takes_note() {
        return match note {
            None => Ok(()),
            Some(_) => refuse(format!(
                "a decision `{}` carries no note (only a return's note and a Question's answer are stored)",
                decision.as_str()
            )),
        };
    }
    let what = if decision == Decision::Answer {
        "an answer"
    } else {
        "a return"
    };
    let Some(note) = note else {
        return refuse(format!("{what} needs its text"));
    };
    if note.trim().is_empty() {
        return refuse(format!("{what} needs its text, and this one is empty"));
    }
    let chars = note.chars().count();
    if chars > NOTE_MAX_CHARS {
        return refuse(format!(
            "{what}'s text is {chars} characters, and it may be at most {NOTE_MAX_CHARS}"
        ));
    }
    // R7: no line break, no control character but tab, and no format,
    // separator or tag character a terminal or a prompt would not show as
    // itself (one table, `am_domain::text`).
    if note
        .chars()
        .any(|c| c == '\n' || am_domain::text::is_hidden(c))
    {
        return refuse(format!(
            "{what}'s text is one line with no control, invisible, format or separator character but tab"
        ));
    }
    Ok(())
}

/// The story an Inbox kind's decisions arrive with, for a kind this version
/// does not resolve (story 3.4, Question 3).
fn resolved_from(kind: InboxKind) -> Option<&'static str> {
    match kind {
        InboxKind::Approval => Some("4.8"),
        InboxKind::Draft => Some("3.6"),
        InboxKind::BudgetAsk => Some("4.3"),
        InboxKind::Proposal => Some("4.10"),
        InboxKind::Review | InboxKind::Question | InboxKind::Failure => None,
    }
}

/// Who resolved an item, in the words of History and audit: the committed
/// `resolved_by` as its actor's words ("You (Reviewer)").
fn resolver_words(resolved_by: Option<&str>) -> String {
    match resolved_by.map(serde_json::from_str::<Actor>) {
        Some(Ok(actor)) => actor.describe(),
        Some(Err(_)) => "an unknown resolver".into(),
        None => "nobody".into(),
    }
}

/// Story 3.4 (AD-8, AD-15, AD-25, AD-28): a person's decision on an Inbox
/// item, in the caller's transaction; its `inbox.resolved` entry. See
/// [`Writer::resolve_inbox_item`].
fn resolve_item(
    tx: &rusqlite::Transaction,
    actor: &Actor,
    r: &Resolve,
    now: &str,
) -> Result<Entry, StoreError> {
    // The parameter's type already makes it a person; the writer checks
    // again (defence in depth).
    if !matches!(actor, Actor::Human { .. }) {
        return Err(StoreError::Invalid(format!(
            "refused: only a person resolves an Inbox item (AD-15, AD-28); {} cannot, and nothing was written",
            actor.describe()
        )));
    }
    let Some(item) = item_of(tx, &r.item_id)? else {
        return Err(StoreError::Invalid(format!(
            "refused: there is no Inbox item {}; nothing was written",
            r.item_id
        )));
    };
    if item.status == "resolved" {
        return Err(StoreError::Invalid(format!(
            "refused: Inbox item {} was resolved already: `{}` by {} at {}; an item is resolved once; nothing was changed",
            item.id,
            item.decision.map(Decision::as_str).unwrap_or("?"),
            resolver_words(item.resolved_by.as_deref()),
            item.resolved_at.as_deref().unwrap_or("?")
        )));
    }
    if let Some(story) = resolved_from(item.kind) {
        return Err(StoreError::Invalid(format!(
            "refused: a {} item is resolved from story {story} on, not in this version; Inbox item {} stays open and nothing was written",
            item.kind.title(),
            item.id
        )));
    }
    let allowed = Decision::allowed_for(item.kind);
    if !allowed.contains(&r.decision) {
        let names: Vec<&str> = allowed.iter().map(|d| d.as_str()).collect();
        return Err(StoreError::Invalid(format!(
            "refused: a {} item takes {}, not `{}`; nothing was written",
            item.kind.title(),
            names.join(", "),
            r.decision.as_str()
        )));
    }
    check_note(r.decision, r.note.as_deref())?;
    // AD-8's one Human edge: a Review's or a Question's step leaves
    // `waiting_human` for `running`, and nothing else moves.
    if matches!(item.kind, InboxKind::Review | InboxKind::Question) {
        let moved = tx.execute(
            "UPDATE step_runs SET status = 'running', updated_at = ?2 WHERE id = ?1 AND status = 'waiting_human'",
            params![item.step_run_id, now],
        )?;
        if moved != 1 {
            let step = step_of(tx, &item.step_run_id)?;
            return Err(StoreError::Invalid(format!(
                "refused: step `{}` attempt {} of Inbox item {} is {}, not waiting_human, so the item has nothing to resolve (AD-8); nothing was changed",
                step.step_key,
                step.attempt,
                item.id,
                step.status.as_str()
            )));
        }
    }
    let by = serde_json::to_string(actor).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let note = if r.decision.takes_note() {
        r.note.as_deref()
    } else {
        None
    };
    let n = tx.execute(
        "UPDATE inbox_items SET status = 'resolved', decision = ?2, note = ?3, resolved_by = ?4, resolved_at = ?5 WHERE id = ?1 AND status = 'open'",
        params![item.id, r.decision.as_str(), note, by, now],
    )?;
    if n != 1 {
        return Err(StoreError::Invalid(format!(
            "refused: Inbox item {} is no longer open; nothing was changed",
            item.id
        )));
    }
    Ok((
        "inbox.resolved".into(),
        item.id.clone(),
        Some(json!({
            "decision": r.decision.as_str(),
            "kind": item.kind.as_str(),
            "note_sha256": note.map(audit::sha256_hex),
            "step_run_id": item.step_run_id,
            "taint": flag(item.taint),
        })),
    ))
}

/// Story 3.4 (AD-8, AD-15, AD-28, AD-36): the engine's apply of a
/// decision a person committed on a gate's Review item, decided from
/// committed rows; its audit entries. See [`WriteCommand::ApplyResolution`].
fn apply_resolution(
    tx: &rusqlite::Transaction,
    item_id: &str,
    past_max: Option<&NewInboxItem>,
    now: &str,
) -> Result<Vec<Entry>, StoreError> {
    let refuse = |why: String| {
        Err(StoreError::Invalid(format!(
            "refused: {why}; nothing was changed"
        )))
    };
    let Some(item) = item_of(tx, item_id)? else {
        return refuse(format!("there is no Inbox item {item_id}"));
    };
    let decision = match (item.status.as_str(), item.decision) {
        ("resolved", Some(d)) => d,
        _ => {
            return refuse(format!(
                "Inbox item {item_id} is open: no person has decided on it, so there is nothing to apply (AD-15)"
            ));
        }
    };
    if item.kind != InboxKind::Review {
        return refuse(format!(
            "the decision on {} item {item_id} has nothing for the engine to apply in this version",
            item.kind.title()
        ));
    }
    // R3 (AD-15): only a decision a person committed is applied; the
    // resolver is read from the row as committed.
    if !matches!(item.resolver(), Some(Actor::Human { .. })) {
        return refuse(format!(
            "Inbox item {item_id} was not resolved by a person ({}), and the engine applies only a person's decision (AD-15)",
            resolver_words(item.resolved_by.as_deref())
        ));
    }
    let gate = step_of(tx, &item.step_run_id)?;
    if gate.status != StepStatus::Running {
        return refuse(format!(
            "the decision `{}` on Inbox item {item_id} was applied already: step `{}` attempt {} is {}",
            decision.as_str(),
            gate.step_key,
            gate.attempt,
            gate.status.as_str()
        ));
    }
    let finished = |status: &str| -> Entry {
        (
            "step.finished".into(),
            gate.id.clone(),
            Some(json!({
                "decision": decision.as_str(),
                "item": item_id,
                "output_sha256": Value::Null,
                "status": status,
            })),
        )
    };
    let run_finished = || -> Entry {
        (
            "run.finished".into(),
            gate.run_id.clone(),
            Some(json!({"status": "failed"})),
        )
    };
    match decision {
        Decision::Approve | Decision::Reject if past_max.is_some() => refuse(format!(
            "the decision `{}` on Inbox item {item_id} raises no Failure item",
            decision.as_str()
        )),
        Decision::Approve => {
            finish_step(tx, &gate.id, "succeeded", &None, now)?;
            Ok(vec![finished("succeeded")])
        }
        Decision::Reject => {
            finish_step(tx, &gate.id, "failed", &None, now)?;
            finish_run(tx, &gate.run_id, "failed", now)?;
            Ok(vec![finished("failed"), run_finished()])
        }
        Decision::Return => {
            let Some(max) = item.max_returns else {
                return refuse(format!(
                    "the Review item {item_id} carries no max_returns, so its return cannot be counted"
                ));
            };
            // AD-36: the count is the journal's, this return included.
            let returns: i64 = tx.query_row(
                "SELECT count(*) FROM inbox_items i JOIN step_runs s ON s.id = i.step_run_id
                 WHERE i.run_id = ?1 AND i.kind = 'review' AND i.status = 'resolved' AND i.decision = 'return' AND s.step_key = ?2",
                params![gate.run_id, gate.step_key],
                |r| r.get(0),
            )?;
            let past = returns > i64::from(max);
            match (past, past_max) {
                (false, Some(_)) => refuse(format!(
                    "return {returns} at step `{}` is within its max_returns of {max}, so the gate loops back and raises no Failure item",
                    gate.step_key
                )),
                (true, None) => refuse(format!(
                    "return {returns} at step `{}` is past its max_returns of {max}, so the gate fails with a Failure item, and an apply without one is refused",
                    gate.step_key
                )),
                (false, None) => {
                    let Some(payload) = &item.payload else {
                        return refuse(format!(
                            "the Review item {item_id} carries no step's output, so there is no step to return to"
                        ));
                    };
                    producer_in_run(tx, payload, &gate.run_id)?;
                    let producer = step_of(tx, payload)?;
                    let queued = loop_back(
                        tx,
                        &gate,
                        None,
                        &producer.step_key,
                        Back::GateNote { item: item_id },
                        now,
                    )?;
                    Ok(vec![(
                        "step.looped_back".into(),
                        gate.id.clone(),
                        Some(json!({
                            "decision": decision.as_str(),
                            "item": item_id,
                            "max_returns": max,
                            "queued": queued,
                            "returns": returns,
                            "to": producer.step_key,
                        })),
                    )])
                }
                (true, Some(failure)) => {
                    if failure.kind != InboxKind::Failure {
                        return refuse(format!(
                            "a gate past its max_returns raises a Failure item, not a {}",
                            failure.kind.title()
                        ));
                    }
                    finish_step(tx, &gate.id, "failed", &None, now)?;
                    let created = create_item(tx, &gate, failure, now)?;
                    finish_run(tx, &gate.run_id, "failed", now)?;
                    Ok(vec![finished("failed"), created, run_finished()])
                }
            }
        }
        Decision::Answer | Decision::Dismiss => refuse(format!(
            "a Review item is not decided `{}`",
            decision.as_str()
        )),
    }
}

/// Finishes the run, never while a step of it waits for a person (review
/// P16, R4: also when a failure would finish it).
fn finish_run(
    tx: &rusqlite::Transaction,
    run_id: &str,
    status: &str,
    now: &str,
) -> Result<(), StoreError> {
    no_step_waits(tx, run_id)?;
    tx.execute(
        "UPDATE runs SET status = ?2, finished_at = ?3, updated_at = ?3 WHERE id = ?1",
        params![run_id, status, now],
    )?;
    Ok(())
}

/// Finishes a `running` step (the caller checked the state) as
/// `succeeded`, `failed` or `cancelled` (AD-8's edges out of `running`
/// that finish it).
fn finish_step(
    tx: &rusqlite::Transaction,
    step_run_id: &str,
    status: &str,
    output: &Option<String>,
    now: &str,
) -> Result<(), StoreError> {
    if !matches!(
        StepStatus::parse(status),
        Some(StepStatus::Succeeded | StepStatus::Failed | StepStatus::Cancelled)
    ) {
        return Err(StoreError::Invalid(format!(
            "refused: a running step finishes succeeded, failed or cancelled, not `{status}` (AD-8)"
        )));
    }
    let n = tx.execute(
        "UPDATE step_runs SET status = ?2, output = ?3, finished_at = ?4, updated_at = ?4 WHERE id = ?1",
        params![step_run_id, status, output, now],
    )?;
    if n != 1 {
        return Err(StoreError::Invalid(format!("step {step_run_id} not found")));
    }
    Ok(())
}

/// Fixtures for tests (older- and newer-database rows of story 1.6). Only
/// with the `test-support` feature, which only dev-dependencies enable; never
/// in the app.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod testing {
    use std::path::Path;

    use rusqlite::{Connection, params};

    use crate::StoreError;

    /// A database (0600) at `version`, built from the first `version`
    /// migrations with no reports; past the latest, all of them and then
    /// that `user_version`.
    pub fn database_at_version(path: &Path, version: i64) -> Result<(), StoreError> {
        crate::migrations::database_at_version(path, version)
    }

    pub use crate::fsx::testing::{refuse_clone_of, stream_copies_of_refused};

    /// One row in `triggers` (present since version 1), written directly.
    pub fn insert_fixture_trigger(path: &Path, id: &str) -> Result<(), StoreError> {
        let conn = Connection::open(path)?;
        conn.execute(
            "INSERT INTO triggers (id, cron, tz, catch_up, state) VALUES (?1, '0 9 * * *', 'UTC', 'once', 'active')",
            params![id],
        )?;
        Ok(())
    }
}

// ------------------------------------------------------------------ reads

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: String,
    pub workflow: String,
    pub status: String,
    /// The hash of the config snapshot the run was built from (AD-16);
    /// `None` for a run with no snapshot and for rows from before the pin.
    pub config_hash: Option<String>,
    pub created_at: String,
    /// When the run finished; `None` while it runs and for rows from
    /// before the column.
    pub finished_at: Option<String>,
}

/// The columns of a [`RunRow`], in [`run_row`]'s order.
const RUN_COLUMNS: &str = "id, workflow, status, config_hash, created_at, finished_at";

fn run_row(r: &rusqlite::Row) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: r.get(0)?,
        workflow: r.get(1)?,
        status: r.get(2)?,
        config_hash: r.get(3)?,
        created_at: r.get(4)?,
        finished_at: r.get(5)?,
    })
}

/// One step attempt (story 3.3: a step's attempts are rows of their own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRow {
    pub id: String,
    pub step_key: String,
    pub seq: i64,
    /// 1 for a step's first attempt (and every row from before 0007); a
    /// loop-back queues the next.
    pub attempt: i64,
    pub status: String,
    pub output: Option<String>,
    /// Whether the attempt read untrusted content (AD-10): the mark at rest
    /// on it and its output. It only ever rises.
    pub taint: Taint,
    /// When the step started; `None` while it is queued.
    pub started_at: Option<String>,
    /// When the step finished; `None` until then.
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRow {
    pub id: String,
    pub step_run_id: String,
    pub call_seq: i64,
    pub status: String,
    pub settled: bool,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

/// One Inbox item (AD-28; stories 3.3 and 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItemRow {
    pub id: String,
    pub kind: InboxKind,
    /// `open` or `resolved` (story 3.4).
    pub status: String,
    pub run_id: String,
    pub step_run_id: String,
    pub payload: Option<String>,
    pub taint: Taint,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
    /// The resolver as the JSON of its `Actor` (a person's).
    pub resolved_by: Option<String>,
    /// What the person decided, once resolved.
    pub decision: Option<Decision>,
    /// A return's note or a Question's answer, stored here once.
    pub note: Option<String>,
    /// A Review's: how many returns its gate takes (story 3.4).
    pub max_returns: Option<u32>,
}

impl InboxItemRow {
    /// Who resolved the item, as committed in `resolved_by` (story 3.4);
    /// `None` while it is open, or when the column holds no actor.
    pub fn resolver(&self) -> Option<Actor> {
        self.resolved_by
            .as_deref()
            .and_then(|by| serde_json::from_str::<Actor>(by).ok())
    }
}

/// The columns of an [`InboxItemRow`], in [`raw_item`]'s order.
const ITEM_COLUMNS: &str = "id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by, decision, note, max_returns";

/// An item row as SQLite holds it, before its kind and decision are read.
struct RawItem {
    row: InboxItemRow,
    kind: String,
    decision: Option<String>,
}

fn raw_item(r: &rusqlite::Row) -> rusqlite::Result<RawItem> {
    let max_returns: Option<i64> = r.get(14)?;
    Ok(RawItem {
        row: InboxItemRow {
            id: r.get(0)?,
            kind: InboxKind::Review,
            status: r.get(2)?,
            run_id: r.get(3)?,
            step_run_id: r.get(4)?,
            payload: r.get(5)?,
            taint: taint_of(r.get(6)?),
            title: r.get(7)?,
            body: r.get(8)?,
            created_at: r.get(9)?,
            resolved_at: r.get(10)?,
            resolved_by: r.get(11)?,
            decision: None,
            note: r.get(13)?,
            max_returns: max_returns.and_then(|m| u32::try_from(m).ok()),
        },
        kind: r.get(1)?,
        decision: r.get(12)?,
    })
}

impl RawItem {
    fn parse(self) -> Result<InboxItemRow, StoreError> {
        let RawItem {
            mut row,
            kind,
            decision,
        } = self;
        row.kind = InboxKind::parse(&kind).ok_or_else(|| {
            StoreError::Invalid(format!(
                "Inbox item {} has the unknown kind `{kind}`",
                row.id
            ))
        })?;
        row.decision = match decision {
            None => None,
            Some(d) => Some(Decision::parse(&d).ok_or_else(|| {
                StoreError::Invalid(format!(
                    "Inbox item {} has the unknown decision `{d}`",
                    row.id
                ))
            })?),
        };
        Ok(row)
    }
}

/// The Inbox items `filter` selects (with `id` as its one parameter),
/// in `order`.
fn items_where(
    conn: &Connection,
    filter: &str,
    id: Option<&str>,
    order: &str,
) -> Result<Vec<InboxItemRow>, StoreError> {
    let mut st = conn.prepare(&format!(
        "SELECT {ITEM_COLUMNS} FROM inbox_items WHERE {filter} ORDER BY {order}"
    ))?;
    let rows: Vec<RawItem> = match id {
        Some(id) => st
            .query_map(params![id], raw_item)?
            .collect::<Result<_, _>>()?,
        None => st.query_map([], raw_item)?.collect::<Result<_, _>>()?,
    };
    rows.into_iter().map(RawItem::parse).collect()
}

/// One Inbox item by id, as committed.
fn item_of(conn: &Connection, id: &str) -> Result<Option<InboxItemRow>, StoreError> {
    Ok(items_where(conn, "id = ?1", Some(id), "rowid")?
        .into_iter()
        .next())
}

/// One tool call of a step attempt (AD-8; story 3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRow {
    pub id: String,
    pub step_run_id: String,
    pub call_seq: i64,
    pub tool: String,
    /// `issued`, `succeeded`, `failed` or `refused`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    pub model_call_id: String,
    pub reserved_micro_usd: i64,
    pub settled_micro_usd: Option<i64>,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRow {
    pub trigger_id: String,
    pub scheduled_for: String,
    pub fired_at: String,
    pub kind: String,
}

pub struct Reader {
    conn: Connection,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Reader, StoreError> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.pragma_update(None, "query_only", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Reader { conn })
    }

    /// The database's schema version (`user_version`), so a lock-free
    /// reader can tell a database this binary has not migrated yet (older
    /// than [`SCHEMA_VERSION`]) before it reads columns that are not there.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        migrations::user_version(&self.conn)
    }

    pub fn unfinished_runs(&self) -> Result<Vec<RunRow>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE status NOT IN ('succeeded', 'failed', 'cancelled') ORDER BY created_at"
        ))?;
        let rows = st.query_map([], run_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every run, newest first (by creation time, then by insertion).
    pub fn runs(&self) -> Result<Vec<RunRow>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs ORDER BY created_at DESC, rowid DESC"
        ))?;
        let rows = st.query_map([], run_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn run(&self, run_id: &str) -> Result<Option<RunRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"),
                params![run_id],
                run_row,
            )
            .optional()?)
    }

    /// Every step attempt of the run, in workflow order, each step's
    /// attempts in order.
    pub fn steps(&self, run_id: &str) -> Result<Vec<StepRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT id, step_key, seq, attempt, status, output, taint, started_at, finished_at FROM step_runs WHERE run_id = ?1 ORDER BY seq, attempt",
        )?;
        let rows = st.query_map(params![run_id], |r| {
            Ok(StepRow {
                id: r.get(0)?,
                step_key: r.get(1)?,
                seq: r.get(2)?,
                attempt: r.get(3)?,
                status: r.get(4)?,
                output: r.get(5)?,
                taint: taint_of(r.get(6)?),
                started_at: r.get(7)?,
                finished_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn calls_for_run(&self, run_id: &str) -> Result<Vec<CallRow>, StoreError> {
        self.calls_where("s.run_id = ?1", run_id)
    }

    /// The model calls of one step attempt, in order (story 3.3: a step's
    /// iterations are counted from these rows).
    pub fn calls_for_step(&self, step_run_id: &str) -> Result<Vec<CallRow>, StoreError> {
        self.calls_where("c.step_run_id = ?1", step_run_id)
    }

    fn calls_where(&self, filter: &str, id: &str) -> Result<Vec<CallRow>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT c.id, c.step_run_id, c.call_seq, c.status, c.settled, c.input_tokens, c.output_tokens
             FROM model_calls c JOIN step_runs s ON s.id = c.step_run_id WHERE {filter} ORDER BY s.seq, s.attempt, c.call_seq"
        ))?;
        let rows = st.query_map(params![id], |r| {
            Ok(CallRow {
                id: r.get(0)?,
                step_run_id: r.get(1)?,
                call_seq: r.get(2)?,
                status: r.get(3)?,
                settled: r.get::<_, i64>(4)? != 0,
                input_tokens: r.get(5)?,
                output_tokens: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The journaled inputs of one step attempt, in the order they were
    /// written (story 3.3, AD-43).
    pub fn step_inputs(&self, step_run_id: &str) -> Result<Vec<StepInput>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT kind, from_step_run, from_item, label, taint FROM step_inputs WHERE step_run_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map(params![step_run_id], |r| {
            Ok(StepInput {
                kind: r.get(0)?,
                from_step_run: r.get(1)?,
                from_item: r.get(2)?,
                label: r.get(3)?,
                taint: taint_of(r.get(4)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The tool calls of one step attempt, in call order (story 3.3).
    pub fn tool_calls(&self, step_run_id: &str) -> Result<Vec<ToolCallRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT id, step_run_id, call_seq, tool, status FROM tool_calls WHERE step_run_id = ?1 ORDER BY call_seq",
        )?;
        let rows = st.query_map(params![step_run_id], |r| {
            Ok(ToolCallRow {
                id: r.get(0)?,
                step_run_id: r.get(1)?,
                call_seq: r.get(2)?,
                tool: r.get(3)?,
                status: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The Inbox items of a run, oldest first (story 3.3, AD-28).
    pub fn inbox_items(&self, run_id: &str) -> Result<Vec<InboxItemRow>, StoreError> {
        items_where(&self.conn, "run_id = ?1", Some(run_id), "created_at, rowid")
    }

    /// One Inbox item by id (story 3.4); `None` when there is none.
    pub fn inbox_item(&self, id: &str) -> Result<Option<InboxItemRow>, StoreError> {
        item_of(&self.conn, id)
    }

    /// Every open Inbox item of every run, oldest first (story 3.4).
    pub fn open_items(&self) -> Result<Vec<InboxItemRow>, StoreError> {
        items_where(&self.conn, "status = 'open'", None, "created_at, rowid")
    }

    /// How many of `run_id`'s Review items on attempts of the step
    /// `step_key` a person resolved `return` (story 3.4, AD-36: a gate's
    /// returns so far, from the journal).
    pub fn returns_at(&self, run_id: &str, step_key: &str) -> Result<u32, StoreError> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM inbox_items i JOIN step_runs s ON s.id = i.step_run_id
             WHERE i.run_id = ?1 AND i.kind = 'review' AND i.status = 'resolved' AND i.decision = 'return' AND s.step_key = ?2",
            params![run_id, step_key],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }

    pub fn ledger_for_run(&self, run_id: &str) -> Result<Vec<LedgerRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT model_call_id, reserved_micro_usd, settled_micro_usd, state FROM ledger WHERE run_id = ?1 ORDER BY created_at",
        )?;
        let rows = st.query_map(params![run_id], |r| {
            Ok(LedgerRow {
                model_call_id: r.get(0)?,
                reserved_micro_usd: r.get(1)?,
                settled_micro_usd: r.get(2)?,
                state: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn trigger(&self, id: &str) -> Result<Option<TriggerRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, cron, tz, catch_up, state, next_fire, last_fired FROM triggers WHERE id = ?1",
                params![id],
                |r| {
                    Ok(TriggerRow {
                        id: r.get(0)?,
                        cron: r.get(1)?,
                        tz: r.get(2)?,
                        catch_up: r.get(3)?,
                        state: r.get(4)?,
                        next_fire: r.get(5)?,
                        last_fired: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn fires(&self, trigger_id: &str) -> Result<Vec<FireRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT trigger_id, scheduled_for, fired_at, kind FROM fires WHERE trigger_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map(params![trigger_id], |r| {
            Ok(FireRow {
                trigger_id: r.get(0)?,
                scheduled_for: r.get(1)?,
                fired_at: r.get(2)?,
                kind: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys() -> Actor {
        Actor::System {
            component: "test".into(),
        }
    }

    #[test]
    fn reserve_settle_and_lost() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let w = store.writer();
        w.exec(
            sys(),
            WriteCommand::CreateRun {
                run_id: "r1".into(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["a".into()],
                config_hash: None,
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let step = r.steps("r1").unwrap()[0].id.clone();
        w.exec(
            sys(),
            WriteCommand::StartStep {
                step_run_id: step.clone(),
            },
        )
        .unwrap();
        for (call, seq) in [("c1", 1), ("c2", 2), ("c3", 3)] {
            w.exec(
                sys(),
                WriteCommand::ReserveCall {
                    step_run_id: step.clone(),
                    call_id: call.into(),
                    call_seq: seq,
                    reserved_micro_usd: 1000,
                    rate_snapshot: "test".into(),
                    taint: Taint::Clean,
                },
            )
            .unwrap();
        }
        w.exec(
            sys(),
            WriteCommand::SettleCall {
                call_id: "c1".into(),
                usage: Some(Usage {
                    input_tokens: 1,
                    output_tokens: 2,
                }),
                cost_micro_usd: Some(40),
            },
        )
        .unwrap();
        w.exec(
            sys(),
            WriteCommand::SettleCall {
                call_id: "c2".into(),
                usage: None,
                cost_micro_usd: None,
            },
        )
        .unwrap();
        w.exec(
            sys(),
            WriteCommand::MarkLost {
                call_id: "c3".into(),
            },
        )
        .unwrap();
        let l = r.ledger_for_run("r1").unwrap();
        let settled: Vec<i64> = l.iter().map(|x| x.settled_micro_usd.unwrap()).collect();
        assert_eq!(settled, vec![40, 1000, 1000]);
        assert!(l.iter().all(|x| x.state == "settled"));
        assert_eq!(r.verify_audit_chain().unwrap(), None);
        // A second settle of a closed call is refused.
        assert!(
            w.exec(
                sys(),
                WriteCommand::SettleCall {
                    call_id: "c1".into(),
                    usage: None,
                    cost_micro_usd: None
                }
            )
            .is_err()
        );
    }

    /// The pin is audited as an argument, so anything but a sha256 hex
    /// string is refused and nothing is written.
    #[test]
    fn a_config_hash_that_is_not_a_sha256_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        for bad in ["", "not a hash", &"A".repeat(64), &"a".repeat(63)] {
            let refused = store.writer().exec(
                sys(),
                WriteCommand::CreateRun {
                    run_id: "r1".into(),
                    workflow: "wf".into(),
                    origin: "manual".into(),
                    steps: vec!["a".into()],
                    config_hash: Some(bad.to_string()),
                },
            );
            assert!(matches!(refused, Err(StoreError::Invalid(_))), "{bad:?}");
        }
        assert!(store.reader().unwrap().runs().unwrap().is_empty());
    }

    #[test]
    fn mark_lost_never_overwrites_a_settled_cost() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let w = store.writer();
        w.exec(
            sys(),
            WriteCommand::CreateRun {
                run_id: "r1".into(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["a".into()],
                config_hash: None,
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let step = r.steps("r1").unwrap()[0].id.clone();
        w.exec(
            sys(),
            WriteCommand::StartStep {
                step_run_id: step.clone(),
            },
        )
        .unwrap();
        w.exec(
            sys(),
            WriteCommand::ReserveCall {
                step_run_id: step,
                call_id: "c1".into(),
                call_seq: 1,
                reserved_micro_usd: 1000,
                rate_snapshot: "t".into(),
                taint: Taint::Clean,
            },
        )
        .unwrap();
        w.exec(
            sys(),
            WriteCommand::SettleCall {
                call_id: "c1".into(),
                usage: None,
                cost_micro_usd: Some(40),
            },
        )
        .unwrap();
        w.exec(
            sys(),
            WriteCommand::MarkLost {
                call_id: "c1".into(),
            },
        )
        .unwrap();
        assert_eq!(
            r.ledger_for_run("r1").unwrap()[0].settled_micro_usd,
            Some(40)
        );
        assert_eq!(r.calls_for_run("r1").unwrap()[0].status, "settled");
    }

    /// The crash window between settling a call and finishing its step is
    /// closed: both commit together or neither does.
    #[test]
    fn settle_and_finish_step_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let w = store.writer();
        w.exec(
            sys(),
            WriteCommand::CreateRun {
                run_id: "r1".into(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["a".into()],
                config_hash: None,
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let step = r.steps("r1").unwrap()[0].id.clone();
        w.exec(
            sys(),
            WriteCommand::StartStep {
                step_run_id: step.clone(),
            },
        )
        .unwrap();
        w.exec(
            sys(),
            WriteCommand::ReserveCall {
                step_run_id: step.clone(),
                call_id: "c1".into(),
                call_seq: 1,
                reserved_micro_usd: 1000,
                rate_snapshot: "t".into(),
                taint: Taint::Clean,
            },
        )
        .unwrap();
        // The step half fails: the settle half must roll back with it.
        let bad = WriteCommand::SettleAndFinishStep {
            call_id: "c1".into(),
            usage: None,
            cost_micro_usd: Some(40),
            step_run_id: "no-such-step".into(),
            status: "succeeded".into(),
            output: Some("x".into()),
        };
        assert!(w.exec(sys(), bad).is_err());
        assert!(!r.calls_for_run("r1").unwrap()[0].settled);
        assert_eq!(r.ledger_for_run("r1").unwrap()[0].state, "reserved");
        let good = WriteCommand::SettleAndFinishStep {
            call_id: "c1".into(),
            usage: None,
            cost_micro_usd: Some(40),
            step_run_id: step,
            status: "succeeded".into(),
            output: Some("x".into()),
        };
        w.exec(sys(), good).unwrap();
        assert!(r.calls_for_run("r1").unwrap()[0].settled);
        assert_eq!(r.steps("r1").unwrap()[0].status, "succeeded");
        assert_eq!(r.steps("r1").unwrap()[0].output.as_deref(), Some("x"));
    }

    #[test]
    fn readers_cannot_write_and_audit_is_append_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let store = Store::open(&path).unwrap();
        store
            .writer()
            .exec(
                sys(),
                WriteCommand::Audit {
                    action: "x".into(),
                    target: "y".into(),
                    outcome: "ok".into(),
                    hash: None,
                    args: Default::default(),
                },
            )
            .unwrap();
        let r = store.reader().unwrap();
        assert!(r.conn.execute("DELETE FROM runs", []).is_err());
        let rw = Connection::open(&path).unwrap();
        assert!(rw.execute("DELETE FROM audit_log", []).is_err());
        assert!(
            rw.execute("UPDATE audit_log SET outcome = 'z'", [])
                .is_err()
        );
    }

    /// Story 3.4, row 20 (AD-15, AD-28): the crate-private resolve, sent
    /// through the writer with an actor that is not a person (the engine,
    /// a seat, the Assistant, an import), is refused in the writer and
    /// nothing is written: the item stays open, its gate waits, no audit
    /// entry is added. The same job from a person commits.
    #[test]
    fn the_writer_refuses_a_resolve_by_anyone_but_a_person() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let w = store.writer();
        w.exec(
            sys(),
            WriteCommand::CreateRun {
                run_id: "r1".into(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["draft".into(), "approve".into()],
                config_hash: None,
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let gate = r.steps("r1").unwrap()[1].id.clone();
        w.exec(
            sys(),
            WriteCommand::PauseStep {
                step_run_id: gate.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                item: NewInboxItem {
                    id: "i1".into(),
                    kind: InboxKind::Review,
                    payload: None,
                    taint: Taint::Clean,
                    title: "Review".into(),
                    body: "Step `approve` waits for your review.".into(),
                    max_returns: Some(2),
                },
            },
        )
        .unwrap();
        let resolve = || {
            Command::Resolve(Resolve {
                item_id: "i1".into(),
                decision: Decision::Approve,
                note: None,
            })
        };
        let entries = |r: &Reader| {
            let mut out = Vec::new();
            r.export_audit_jsonl(&mut out).unwrap();
            out.split(|b| *b == b'\n').filter(|l| !l.is_empty()).count()
        };
        let before = entries(&r);
        let run = uuid::Uuid::now_v7();
        for actor in [
            sys(),
            Actor::Seat {
                seat: "seat-a".into(),
                run,
                step: "approve".into(),
            },
            Actor::Assistant { proposal: run },
            Actor::Import {
                source: "an import".into(),
            },
        ] {
            let refused = w.send(actor.clone(), resolve());
            assert!(
                matches!(&refused, Err(StoreError::Invalid(words)) if words.contains("only a person")),
                "{actor:?}: {refused:?}"
            );
            let item = r.inbox_item("i1").unwrap().unwrap();
            assert_eq!(
                (item.status.as_str(), item.decision, item.resolved_by),
                ("open", None, None),
                "{actor:?}"
            );
            assert_eq!(r.steps("r1").unwrap()[1].status, "waiting_human");
            assert_eq!(entries(&r), before, "{actor:?}: nothing audited");
        }
        let person = Actor::Human {
            person: "owner".into(),
            hat: am_domain::REVIEWER_HAT.into(),
        };
        w.send(person, resolve()).unwrap();
        assert_eq!(r.inbox_item("i1").unwrap().unwrap().status, "resolved");
        assert_eq!(r.steps("r1").unwrap()[1].status, "running");
    }

    #[test]
    fn newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        drop(Store::open(&path).unwrap());
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(c);
        assert!(matches!(Store::open(&path), Err(StoreError::TooNew { .. })));
    }
}
