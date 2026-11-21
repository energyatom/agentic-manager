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
//!   names its holder. [`legacy::move_legacy`] moves an older binary's
//!   in-project folder there first (story 1.13). [`Store::open`] on a bare
//!   path remains for the spike's callers.
//!
//! The spike keeps the minimal journal: runs, step runs, model calls, the
//! ledger (reserve, then settle), triggers and fires. Config History (the
//! intent rows and History entries of AD-16's apply order) is in
//! [`StoreConfigHistory`].
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;

use am_domain::{Actor, NewIntent, Usage};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};

pub mod audit;
mod config_history;
mod fsx;
pub mod legacy;
mod migrations;
pub mod project;

pub use audit::{ChainBreak, ChainOk, verify_jsonl};
pub use config_history::StoreConfigHistory;
pub use fsx::stream_copy;
pub use migrations::{AppliedMigration, OpenReport, PRE_MIGRATION_KEPT, SCHEMA_VERSION};
use project::ProjectLock;
pub use project::{LockHolder, lock_holder, lock_is_held, open_runtime};

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
    #[error("opening a project is not supported on this platform yet")]
    Unsupported,
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
    CreateRun {
        run_id: String,
        workflow: String,
        origin: String,
        steps: Vec<String>,
    },
    StartStep {
        step_run_id: String,
    },
    /// Journal the call and reserve its spend, before the call is made (AD-8, AD-9).
    ReserveCall {
        step_run_id: String,
        call_id: String,
        call_seq: i64,
        reserved_micro_usd: i64,
        rate_snapshot: String,
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
    FinishStep {
        step_run_id: String,
        status: String,
        output: Option<String>,
    },
    /// Settle a call and finish its step in one transaction, so a crash can
    /// never leave a settled (paid) call under an unfinished step (AD-8).
    SettleAndFinishStep {
        call_id: String,
        usage: Option<Usage>,
        cost_micro_usd: Option<i64>,
        step_run_id: String,
        status: String,
        output: Option<String>,
    },
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
    /// the content's sha256 hex when relevant (as `AuditEvent::hash`).
    Audit {
        action: String,
        target: String,
        outcome: String,
        hash: Option<String>,
    },
    /// Config History (AD-16, story 1.8, see [`StoreConfigHistory`]): the
    /// intent row, committed before the file is written. `id` becomes the
    /// History entry's id. Audited `config.intent_opened`.
    BeginConfigIntent {
        id: String,
        intent: NewIntent,
    },
    /// Deletes the intent and inserts its History entry in one transaction,
    /// audited `config.edited`, `config.undone`, `config.restored` or
    /// `config.outside_edit`.
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
    /// transaction instead (`None` here).
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
            } => (
                "run.created".into(),
                run_id.clone(),
                Some(json!({"origin": origin, "steps": steps.len(), "workflow": workflow})),
            ),
            WriteCommand::StartStep { step_run_id } => {
                ("step.started".into(), step_run_id.clone(), Some(json!({})))
            }
            WriteCommand::ReserveCall {
                step_run_id,
                call_id,
                call_seq,
                reserved_micro_usd,
                rate_snapshot,
            } => (
                "model_call.reserved".into(),
                call_id.clone(),
                Some(json!({
                    "call_seq": call_seq,
                    "rate_snapshot": rate_snapshot,
                    "reserved_micro_usd": reserved_micro_usd,
                    "step_run_id": step_run_id,
                })),
            ),
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
                ..
            } => (
                action.clone(),
                target.clone(),
                hash.as_ref().map(|h| json!({"sha256": h})),
            ),
        })
    }
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

type Job = (Actor, WriteCommand, Reply);

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
        // Refuse a newer (or foreign) database before anything, even the
        // journal mode, is written.
        migrations::refuse_newer(migrations::user_version(&conn)?)?;
        configure(&conn)?;
        let report = migrations::migrate(&mut conn, backups)?;
        let (tx, rx) = mpsc::channel::<Job>();
        let handle = thread::Builder::new()
            .name("am-store-writer".into())
            .spawn(move || {
                let mut conn = conn;
                while let Ok((actor, cmd, reply)) = rx.recv() {
                    let r = apply(&mut conn, &actor, &cmd);
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
    migrations::refuse_newer(migrations::user_version(&conn)?)
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
    if let WriteCommand::Audit { hash: Some(h), .. } = cmd
        && !is_sha256_hex(h)
    {
        return Err(StoreError::Invalid(
            "refused audit entry: `hash` must be 64 lowercase hex characters (sha256)".into(),
        ));
    }
    let tx = conn.transaction()?;
    let now = now_ts();
    let mut config_entry = None;
    match cmd {
        WriteCommand::CreateRun {
            run_id,
            workflow,
            origin,
            steps,
        } => {
            tx.execute(
                "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
                params![run_id, workflow, origin, now],
            )?;
            for (i, s) in steps.iter().enumerate() {
                tx.execute(
                    "INSERT INTO step_runs (id, run_id, step_key, seq, status, updated_at) VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
                    params![new_id(), run_id, s, i as i64, now],
                )?;
            }
        }
        WriteCommand::StartStep { step_run_id } => {
            tx.execute(
                "UPDATE step_runs SET status = 'running', updated_at = ?2 WHERE id = ?1",
                params![step_run_id, now],
            )?;
        }
        WriteCommand::ReserveCall {
            step_run_id,
            call_id,
            call_seq,
            reserved_micro_usd,
            rate_snapshot,
        } => {
            let run_id: String = tx.query_row(
                "SELECT run_id FROM step_runs WHERE id = ?1",
                params![step_run_id],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, created_at) VALUES (?1, ?2, ?3, 'issued', 0, ?4)",
                params![call_id, step_run_id, call_seq, now],
            )?;
            tx.execute(
                "INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, state, rate_snapshot, created_at) VALUES (?1, ?2, ?3, 'model', ?4, 'reserved', ?5, ?6)",
                params![new_id(), call_id, run_id, reserved_micro_usd, rate_snapshot, now],
            )?;
        }
        WriteCommand::SettleCall {
            call_id,
            usage,
            cost_micro_usd,
        } => settle(&tx, call_id, *usage, *cost_micro_usd)?,
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
        } => finish_step(&tx, step_run_id, status, output, &now)?,
        WriteCommand::SettleAndFinishStep {
            call_id,
            usage,
            cost_micro_usd,
            step_run_id,
            status,
            output,
        } => {
            settle(&tx, call_id, *usage, *cost_micro_usd)?;
            finish_step(&tx, step_run_id, status, output, &now)?;
        }
        WriteCommand::FinishRun { run_id, status } => {
            tx.execute(
                "UPDATE runs SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![run_id, status, now],
            )?;
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
            config_entry = Some(config_history::begin(&tx, actor, id, intent, &now)?);
        }
        WriteCommand::CommitConfigEntry { intent_id } => {
            config_entry = Some(config_history::complete(&tx, intent_id, false, &now)?);
        }
        WriteCommand::ReconcileConfigIntent { intent_id } => {
            config_entry = Some(config_history::complete(&tx, intent_id, true, &now)?);
        }
        WriteCommand::DropConfigIntent { intent_id } => {
            config_entry = Some(config_history::drop_intent(&tx, intent_id)?);
        }
    }
    let Some((action, target, args)) = config_entry.or_else(|| cmd.audit_entry()) else {
        return Err(StoreError::Invalid("the command has no audit entry".into()));
    };
    let outcome = match cmd {
        WriteCommand::Audit { outcome, .. } => outcome.clone(),
        _ => "ok".to_string(),
    };
    audit::append(&tx, actor, &action, &target, &outcome, args.as_ref(), &now)?;
    tx.commit()?;
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

fn finish_step(
    tx: &rusqlite::Transaction,
    step_run_id: &str,
    status: &str,
    output: &Option<String>,
    now: &str,
) -> Result<(), StoreError> {
    let n = tx.execute(
        "UPDATE step_runs SET status = ?2, output = ?3, updated_at = ?4 WHERE id = ?1",
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRow {
    pub id: String,
    pub step_key: String,
    pub seq: i64,
    pub status: String,
    pub output: Option<String>,
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

    pub fn unfinished_runs(&self) -> Result<Vec<RunRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT id, workflow, status FROM runs WHERE status NOT IN ('succeeded', 'failed', 'cancelled') ORDER BY created_at",
        )?;
        let rows = st.query_map([], |r| {
            Ok(RunRow {
                id: r.get(0)?,
                workflow: r.get(1)?,
                status: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn run(&self, run_id: &str) -> Result<Option<RunRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, workflow, status FROM runs WHERE id = ?1",
                params![run_id],
                |r| {
                    Ok(RunRow {
                        id: r.get(0)?,
                        workflow: r.get(1)?,
                        status: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn steps(&self, run_id: &str) -> Result<Vec<StepRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT id, step_key, seq, status, output FROM step_runs WHERE run_id = ?1 ORDER BY seq",
        )?;
        let rows = st.query_map(params![run_id], |r| {
            Ok(StepRow {
                id: r.get(0)?,
                step_key: r.get(1)?,
                seq: r.get(2)?,
                status: r.get(3)?,
                output: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn calls_for_run(&self, run_id: &str) -> Result<Vec<CallRow>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT c.id, c.step_run_id, c.call_seq, c.status, c.settled, c.input_tokens, c.output_tokens
             FROM model_calls c JOIN step_runs s ON s.id = c.step_run_id WHERE s.run_id = ?1 ORDER BY s.seq, c.call_seq",
        )?;
        let rows = st.query_map(params![run_id], |r| {
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
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let step = r.steps("r1").unwrap()[0].id.clone();
        for (call, seq) in [("c1", 1), ("c2", 2), ("c3", 3)] {
            w.exec(
                sys(),
                WriteCommand::ReserveCall {
                    step_run_id: step.clone(),
                    call_id: call.into(),
                    call_seq: seq,
                    reserved_micro_usd: 1000,
                    rate_snapshot: "test".into(),
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
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let step = r.steps("r1").unwrap()[0].id.clone();
        w.exec(
            sys(),
            WriteCommand::ReserveCall {
                step_run_id: step,
                call_id: "c1".into(),
                call_seq: 1,
                reserved_micro_usd: 1000,
                rate_snapshot: "t".into(),
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
            },
        )
        .unwrap();
        let r = store.reader().unwrap();
        let step = r.steps("r1").unwrap()[0].id.clone();
        w.exec(
            sys(),
            WriteCommand::ReserveCall {
                step_run_id: step.clone(),
                call_id: "c1".into(),
                call_seq: 1,
                reserved_micro_usd: 1000,
                rate_snapshot: "t".into(),
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
