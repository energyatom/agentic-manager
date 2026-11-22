//! This Mac's user store (story 2.12; AD-6, AD-7, AD-17, AD-25):
//! `<app data dir>/user.db`, shared by every process of the owner on this
//! Mac. It records the user Accounts file's intents and History and holds
//! its own hash-chained audit log, which carries the Account confirmation
//! records. am-store opens it from the [`AppDataLayout`] am-app gives; it
//! never computes the app data dir.
//!
//! - **Format:** WAL, `synchronous=FULL`, `foreign_keys=ON`; its own
//!   migration set ([`USER_SCHEMA_VERSION`]) and its own `application_id`
//!   ([`USER_APPLICATION_ID`]), distinct from a project store's
//!   ([`crate::APPLICATION_ID`]). A database carrying another
//!   `application_id` is refused in words naming what it is; so is a newer
//!   `user_version`, naming the newer app.
//! - **No long-lived writer:** a write happens only through
//!   [`UserStore::locked`], which takes `user.lock` with a bounded wait
//!   (`File::try_lock` polled until the deadline, refused in words after
//!   it), opens one read-write connection and migrates it under the lock
//!   (backing up first when it already holds data). Each write is a
//!   transaction that begins `IMMEDIATE` and runs the one command executor
//!   the project store's writer runs ([`crate::execute`]), so the two
//!   stores record intents, History and audit alike. The guard releases the
//!   lock on drop.
//! - **Readers:** [`UserReader::open`] is read-only (`query_only=ON`) and
//!   takes no lock; a missing `user.db` reads as empty and is not created.

use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use am_domain::runtime::AppDataLayout;
use am_domain::{
    Actor, AuditEvent, ConfigHistory, HistoryEntry, HistoryError, NewIntent, PendingIntent,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde_json::Value;

use crate::audit::{ConfigNote, GENESIS};
use crate::migrations::{self, MigrationSet};
use crate::{Reader, StoreError, WriteCommand, fsx, new_id, now_ts};

const USER_MIGRATIONS: &[&str] = &[include_str!("../migrations/user/0001_user.sql")];

/// The user store's `application_id`: never a project store's.
pub const USER_APPLICATION_ID: i32 = 0x414D_5553;

/// The user store's schema version this binary knows.
pub const USER_SCHEMA_VERSION: i64 = USER_MIGRATIONS.len() as i64;

const USER: MigrationSet = MigrationSet {
    migrations: USER_MIGRATIONS,
    application_id: USER_APPLICATION_ID,
    backup_prefix: "user",
};

/// How often a held `user.lock` is tried again.
const POLL: Duration = Duration::from_millis(25);

/// Refuses a database at `path` that is not this Mac's user store, or one a
/// newer app wrote. `0` with version 0 is a new database.
fn check(path: &Path, conn: &Connection) -> Result<(), StoreError> {
    let id = migrations::application_id(conn)?;
    let version = migrations::user_version(conn)?;
    // A new database is empty: one at version 0 that holds tables of its own
    // is a foreign file, never migrated into.
    let empty = || -> Result<bool, StoreError> {
        Ok(conn.query_row(
            "SELECT NOT EXISTS (SELECT 1 FROM sqlite_master WHERE substr(name, 1, 7) <> 'sqlite_')",
            [],
            |r| r.get(0),
        )?)
    };
    let what = match id {
        USER_APPLICATION_ID => None,
        0 if version == 0 && empty()? => None,
        crate::APPLICATION_ID => Some("a project's database (state.db)"),
        _ => Some("not a database this app made"),
    };
    if let Some(what) = what {
        return Err(StoreError::WrongStore {
            path: path.to_path_buf(),
            what: what.into(),
            expected: "this Mac's user store (user.db)".into(),
        });
    }
    if version < 0 {
        return Err(StoreError::NotOurs { found: version });
    }
    if version > USER_SCHEMA_VERSION {
        return Err(StoreError::UserTooNew {
            path: path.to_path_buf(),
            found: version,
            known: USER_SCHEMA_VERSION,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------- the lock

/// `user.lock`, held while this guard lives.
struct UserLock {
    file: File,
}

impl Drop for UserLock {
    fn drop(&mut self) {
        // The file stays: removing it could race a process that has it open.
        let _ = self.file.unlock();
    }
}

fn take_lock(layout: &AppDataLayout, wait: Duration) -> Result<UserLock, StoreError> {
    let path = layout.user_lock();
    let file = fsx::open_private_file(&path)
        .map_err(|e| StoreError::io(format!("cannot open {}", path.display()), e))?;
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(UserLock { file }),
            Err(TryLockError::WouldBlock) => {
                let waited = started.elapsed();
                if waited >= wait {
                    return Err(StoreError::UserLocked {
                        lock: path,
                        waited_s: waited.as_secs_f64().round() as u64,
                    });
                }
                std::thread::sleep(POLL.min(wait - waited));
            }
            Err(TryLockError::Error(e)) => {
                return Err(StoreError::io(format!("cannot lock {}", path.display()), e));
            }
        }
    }
}

/// Whether another process (or another guard) holds `user.lock` now,
/// checked by trying a shared lock on a fresh handle that is released at
/// once; the file is neither created nor rewritten.
pub fn user_lock_is_held(layout: &AppDataLayout) -> bool {
    match File::open(layout.user_lock()) {
        Ok(f) => matches!(f.try_lock_shared(), Err(TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------- the guard

/// The one way to write this Mac's user store: `user.lock` held, one
/// read-write connection, migrated under the lock. Implements
/// [`ConfigHistory`] for am-config's writer of the user Accounts file.
pub struct UserStore {
    db: Mutex<Reader>,
    path: PathBuf,
    // Dropped after `db`, so the connection closes before the lock goes.
    _lock: UserLock,
}

impl std::fmt::Debug for UserStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl UserStore {
    /// Takes `user.lock` (waiting at most `wait`, then refused in words),
    /// then opens, checks and migrates `user.db` under it (creating the
    /// database 0600 on the first write on this Mac).
    pub fn locked(layout: &AppDataLayout, wait: Duration) -> Result<UserStore, StoreError> {
        if !cfg!(unix) {
            return Err(StoreError::Unsupported);
        }
        // The app data dir is created 0700 on the first write on this Mac
        // (its parents as the OS makes them).
        if let Some(parent) = layout.dir().parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StoreError::io(format!("cannot create {}", parent.display()), e))?;
        }
        fsx::ensure_private_dir(layout.dir())
            .map_err(|e| StoreError::io(format!("cannot create {}", layout.dir().display()), e))?;
        let lock = take_lock(layout, wait)?;
        let path = layout.user_db();
        if path.exists() {
            // Checked read-only first, so nothing is written to a wrong file.
            let conn = crate::open_read_only(&path)?;
            check(&path, &conn)?;
        }
        fsx::create_private_file_if_missing(&path)
            .map_err(|e| StoreError::io(format!("cannot create {}", path.display()), e))?;
        let mut conn = Connection::open(&path)?;
        check(&path, &conn)?;
        crate::configure(&conn)?;
        migrations::migrate_set(&mut conn, &layout.backups_pre_migration(), &USER)?;
        Ok(UserStore {
            db: Mutex::new(Reader { conn }),
            path,
            _lock: lock,
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Reader) -> Result<T, StoreError>) -> Result<T, StoreError> {
        let db = self
            .db
            .lock()
            .map_err(|_| StoreError::Invalid("the user store connection is unusable".into()))?;
        f(&db)
    }

    /// One write: an `IMMEDIATE` transaction running the shared executor,
    /// after checking that no newer app migrated the store meanwhile.
    fn write(&self, actor: &Actor, cmd: &WriteCommand) -> Result<(), StoreError> {
        self.with(|db| {
            let tx = Transaction::new_unchecked(&db.conn, TransactionBehavior::Immediate)?;
            check(&self.path, &tx)?;
            crate::execute(&tx, actor, cmd)?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Appends one entry to the user store's audit chain, in its own
    /// transaction. No `config.*` entry is written here: those come only
    /// with a config intent.
    pub fn append_audit(&self, event: AuditEvent) -> Result<(), StoreError> {
        // A `config.*` entry comes only with a config intent, so none can
        // pass for an owner's confirmation or its removal.
        if event.action.starts_with("config.") {
            return Err(StoreError::Invalid(format!(
                "refused audit entry: a `{}` entry is written only with a config intent",
                event.action
            )));
        }
        if let Some(h) = &event.hash
            && !crate::is_sha256_hex(h)
        {
            return Err(StoreError::Invalid(
                "refused audit entry: `hash` must be 64 lowercase hex characters (sha256)".into(),
            ));
        }
        let mut named: serde_json::Map<String, Value> = event
            .args
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        if let Some(h) = &event.hash {
            named.insert("sha256".into(), Value::String(h.clone()));
        }
        let args = (!named.is_empty()).then_some(Value::Object(named));
        self.with(|db| {
            let tx = Transaction::new_unchecked(&db.conn, TransactionBehavior::Immediate)?;
            check(&self.path, &tx)?;
            crate::audit::append(
                &tx,
                &event.actor,
                &event.action,
                &event.target,
                &event.outcome,
                args.as_ref(),
                &now_ts(),
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    /// The notes of `action` on `target` that committed config intents
    /// wrote, oldest first (read on the guard's own connection).
    pub fn config_note_args(
        &self,
        action: &str,
        target: &str,
    ) -> Result<Vec<ConfigNote>, StoreError> {
        self.with(|db| db.config_note_args(action, target))
    }
}

fn history_error(e: StoreError) -> HistoryError {
    match e {
        StoreError::NoConfigIntent(id) => HistoryError::NoIntent(id),
        other => HistoryError::Store(other.to_string()),
    }
}

impl UserStore {
    fn entry_after(&self, id: &str) -> Result<HistoryEntry, HistoryError> {
        self.with(|db| db.config_entry(id))
            .map_err(history_error)?
            .ok_or_else(|| HistoryError::Store(format!("History entry {id} was not recorded")))
    }
}

impl ConfigHistory for UserStore {
    fn begin_intent(&self, actor: Actor, intent: NewIntent) -> Result<String, HistoryError> {
        let id = new_id();
        self.write(
            &actor,
            &WriteCommand::BeginConfigIntent {
                id: id.clone(),
                intent,
            },
        )
        .map_err(history_error)?;
        Ok(id)
    }

    fn commit_entry(&self, actor: Actor, intent_id: String) -> Result<HistoryEntry, HistoryError> {
        self.write(
            &actor,
            &WriteCommand::CommitConfigEntry {
                intent_id: intent_id.clone(),
            },
        )
        .map_err(history_error)?;
        self.entry_after(&intent_id)
    }

    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), HistoryError> {
        self.write(&actor, &WriteCommand::DropConfigIntent { intent_id })
            .map_err(history_error)
    }

    fn reconcile_intent(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<HistoryEntry, HistoryError> {
        self.write(
            &actor,
            &WriteCommand::ReconcileConfigIntent {
                intent_id: intent_id.clone(),
            },
        )
        .map_err(history_error)?;
        self.entry_after(&intent_id)
    }

    fn pending_intents(&self) -> Result<Vec<PendingIntent>, HistoryError> {
        self.with(|db| db.config_intents()).map_err(history_error)
    }

    fn entries(&self, file: Option<String>) -> Result<Vec<HistoryEntry>, HistoryError> {
        self.with(|db| db.config_history(file.as_deref()))
            .map_err(history_error)
    }

    fn entry(&self, id: String) -> Result<Option<HistoryEntry>, HistoryError> {
        self.with(|db| db.config_entry(&id)).map_err(history_error)
    }
}

// ---------------------------------------------------------------- the reader

/// A read-only view of this Mac's user store; takes no lock. A missing
/// `user.db` reads as empty and is not created.
pub struct UserReader {
    db: Option<Reader>,
}

impl UserReader {
    pub fn open(layout: &AppDataLayout) -> Result<UserReader, StoreError> {
        let path = layout.user_db();
        if !path.exists() {
            return Ok(UserReader { db: None });
        }
        // Read-only with `query_only`, never `immutable`: a commit made
        // after the reader opened is seen.
        let conn = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.pragma_update(None, "query_only", "ON")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        check(&path, &conn)?;
        if migrations::user_version(&conn)? == 0 {
            // Created but never migrated (a first write that stopped).
            return Ok(UserReader { db: None });
        }
        Ok(UserReader {
            db: Some(Reader { conn }),
        })
    }

    /// History entries in `seq` order, all or one file's.
    pub fn config_history(&self, file: Option<&str>) -> Result<Vec<HistoryEntry>, StoreError> {
        match &self.db {
            Some(db) => db.config_history(file),
            None => Ok(Vec::new()),
        }
    }

    /// The latest History entry of `file`, read alone (not every row with
    /// its bytes).
    pub fn latest_config_entry(&self, file: &str) -> Result<Option<HistoryEntry>, StoreError> {
        match &self.db {
            Some(db) => db.latest_config_entry(file),
            None => Ok(None),
        }
    }

    /// Intents committed but neither completed nor dropped, oldest first.
    pub fn config_intents(&self) -> Result<Vec<PendingIntent>, StoreError> {
        match &self.db {
            Some(db) => db.config_intents(),
            None => Ok(Vec::new()),
        }
    }

    /// The notes of `action` on `target` that committed config intents
    /// wrote, oldest first (see [`Reader::config_note_args`]).
    pub fn config_note_args(
        &self,
        action: &str,
        target: &str,
    ) -> Result<Vec<ConfigNote>, StoreError> {
        match &self.db {
            Some(db) => db.config_note_args(action, target),
            None => Ok(Vec::new()),
        }
    }

    /// The `args` of every entry of `action` on `target`, oldest first.
    pub fn audit_args(
        &self,
        action: &str,
        target: &str,
    ) -> Result<Vec<BTreeMap<String, String>>, StoreError> {
        match &self.db {
            Some(db) => db.audit_args(action, target),
            None => Ok(Vec::new()),
        }
    }

    /// The audit log as JSON Lines (see [`Reader::export_audit_jsonl`]).
    pub fn export_audit_jsonl(&self, out: &mut dyn Write) -> Result<(u64, String), StoreError> {
        match &self.db {
            Some(db) => db.export_audit_jsonl(out),
            None => {
                out.flush()
                    .map_err(|e| StoreError::io("cannot write the audit export".into(), e))?;
                Ok((0, GENESIS.to_string()))
            }
        }
    }

    /// Recomputes the stored chain; returns the first broken sequence number.
    pub fn verify_audit_chain(&self) -> Result<Option<i64>, StoreError> {
        match &self.db {
            Some(db) => db.verify_audit_chain(),
            None => Ok(None),
        }
    }
}
