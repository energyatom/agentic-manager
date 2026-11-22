//! Config History (AD-16, R3, story 1.8): the writer's half of the config
//! apply order, its readers and [`StoreConfigHistory`], the adapter behind
//! `am_domain::ConfigHistory`.
//!
//! - `BeginConfigIntent` inserts the intent row (file, kind, the entry it acts
//!   on, both sha256 values, both byte strings and the diff), audited
//!   `config.intent_opened`. The hashes must be the bytes' sha256, so `args`
//!   can only ever hold hashes.
//! - `CommitConfigEntry` deletes the intent and inserts the History entry in
//!   one transaction, audited `config.edited`, `config.undone`,
//!   `config.restored`, `config.outside_edit` (story 1.9),
//!   `config.migrated` (story 1.10; also a `removal`, story 2.13) or
//!   `config.created` (story 2.1) by its kind;
//!   `ReconcileConfigIntent` does the same at
//!   startup, audited `config.reconciled`; `DropConfigIntent` deletes an
//!   intent whose file was never written, audited `config.intent_dropped`.
//! - A History entry keeps the Actor of the intent (who asked for the change);
//!   the audit entry names the Actor of the command (the system, for a
//!   reconcile). `config_history` is append-only (triggers).
//! - Audit `args` hold only the entry id, the file path, the sha256 values,
//!   the kind and the entry acted on; never file content (AD-25).
//! - Story 2.1 (AD-15, AD-25): an intent may carry `AuditNote`s (a widening
//!   record such as `account_added`). They are kept on the intent row and
//!   appended to the audit log right after the `config.*` entry of the
//!   commit or reconcile, in its transaction, with the intent's own Actor
//!   (who asked for and confirmed the change; a reconcile is the system's,
//!   the note is not); never at `begin`, never for a dropped intent. A note with an empty action or
//!   target refuses the intent.

use std::sync::Mutex;

use am_domain::{
    Actor, AuditNote, ConfigHistory, HistoryEntry, HistoryError, HistoryKind, NewIntent,
    PendingIntent,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{Reader, Store, StoreError, WriteCommand, Writer, is_sha256_hex, new_id};

/// What a config command's audit entry holds: action, target, args.
pub(crate) type AuditTriple = (String, String, Option<Value>);

/// A config command's audit entries: its own, then one per note the intent
/// carried (commit and reconcile only), each with the intent's Actor.
pub(crate) type ConfigAudit = (AuditTriple, Vec<(Actor, AuditTriple)>);

/// A note as an audit entry: its args as a JSON object of strings.
fn note_entry(note: &AuditNote) -> AuditTriple {
    let args: Map<String, Value> = note
        .args
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    (
        note.action.clone(),
        note.target.clone(),
        Some(Value::Object(args)),
    )
}

fn sha256_bytes(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn invalid(msg: impl Into<String>) -> StoreError {
    StoreError::Invalid(format!("refused config intent: {}", msg.into()))
}

/// The redacted `args` of every config entry.
fn args(
    entry_id: Option<&str>,
    file: &str,
    kind: &str,
    ref_entry: Option<&str>,
    before: &str,
    after: &str,
) -> Value {
    let mut m = Map::new();
    m.insert("after_sha256".into(), after.into());
    m.insert("before_sha256".into(), before.into());
    if let Some(id) = entry_id {
        m.insert("entry_id".into(), id.into());
    }
    m.insert("file".into(), file.into());
    m.insert("kind".into(), kind.into());
    if let Some(r) = ref_entry {
        m.insert("ref_entry".into(), r.into());
    }
    Value::Object(m)
}

/// A project-relative path with forward slashes, nothing that climbs out.
fn plain_relative(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with('/')
        && !file.contains('\\')
        && !file.contains('\0')
        && file
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

pub(crate) fn begin(
    tx: &Transaction,
    actor: &Actor,
    id: &str,
    i: &NewIntent,
    now: &str,
) -> Result<AuditTriple, StoreError> {
    if id.is_empty() || id.contains('\0') {
        return Err(invalid("the intent needs an id"));
    }
    if !plain_relative(&i.file) {
        return Err(invalid(format!(
            "'{}' is not a project-relative path",
            i.file
        )));
    }
    for (name, h, bytes) in [
        ("before_sha256", &i.before_sha256, &i.before),
        ("after_sha256", &i.after_sha256, &i.after),
    ] {
        if !is_sha256_hex(h) {
            return Err(invalid(format!(
                "`{name}` must be 64 lowercase hex characters (sha256)"
            )));
        }
        if sha256_bytes(bytes) != *h {
            return Err(invalid(format!("`{name}` is not the sha256 of its bytes")));
        }
    }
    if i.before_sha256 == i.after_sha256 {
        return Err(invalid("the file would not change"));
    }
    if i.kind == HistoryKind::Create && !i.before.is_empty() {
        return Err(invalid("a create starts from no bytes"));
    }
    if i.kind == HistoryKind::Removal && (i.before.is_empty() || !i.after.is_empty()) {
        return Err(invalid(
            "a removal records the removed bytes before and none after",
        ));
    }
    for note in &i.audit {
        if note.action.trim().is_empty() || note.target.trim().is_empty() {
            return Err(invalid("an audit note needs an action and a target"));
        }
    }
    let notes = serde_json::to_string(&i.audit).map_err(|e| StoreError::Invalid(e.to_string()))?;
    match (i.kind, &i.ref_entry) {
        (
            HistoryKind::Edit
            | HistoryKind::OutsideEdit
            | HistoryKind::Migration
            | HistoryKind::Create
            | HistoryKind::Removal,
            None,
        ) => {}
        (HistoryKind::Removal, Some(_)) => {
            return Err(invalid("a removal names no earlier entry"));
        }
        (HistoryKind::Create, Some(_)) => {
            return Err(invalid("a create names no earlier entry"));
        }
        (HistoryKind::Edit, Some(_)) => {
            return Err(invalid("an edit names no earlier entry"));
        }
        (HistoryKind::OutsideEdit, Some(_)) => {
            return Err(invalid("an outside edit names no earlier entry"));
        }
        (HistoryKind::Migration, Some(_)) => {
            return Err(invalid("a migration names no earlier entry"));
        }
        (_, None) => return Err(invalid("an undo or restore names the entry it acts on")),
        (_, Some(r)) => {
            let known: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM config_history WHERE id = ?1)",
                params![r],
                |row| row.get(0),
            )?;
            if !known {
                return Err(invalid(format!("no History entry {r}")));
            }
        }
    }
    let actor_json =
        serde_json::to_string(actor).map_err(|e| StoreError::Invalid(e.to_string()))?;
    tx.execute(
        "INSERT INTO config_intents (id, opened_at, actor, file, kind, ref_entry, before_sha256, after_sha256, before_bytes, after_bytes, diff, audit_notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            now,
            actor_json,
            i.file,
            i.kind.as_str(),
            i.ref_entry,
            i.before_sha256,
            i.after_sha256,
            i.before,
            i.after,
            i.diff,
            notes
        ],
    )?;
    Ok((
        "config.intent_opened".into(),
        id.to_string(),
        Some(args(
            None,
            &i.file,
            i.kind.as_str(),
            i.ref_entry.as_deref(),
            &i.before_sha256,
            &i.after_sha256,
        )),
    ))
}

/// The intent's summary: file, kind, ref_entry, before and after sha256.
struct Summary {
    file: String,
    kind: String,
    ref_entry: Option<String>,
    before: String,
    after: String,
    /// The intent's audit notes, as stored (a JSON list).
    notes: String,
    /// The intent's Actor, as stored (JSON).
    actor: String,
}

fn summary(tx: &Transaction, id: &str) -> Result<Summary, StoreError> {
    tx.query_row(
        "SELECT file, kind, ref_entry, before_sha256, after_sha256, audit_notes, actor FROM config_intents WHERE id = ?1",
        params![id],
        |r| {
            Ok(Summary {
                file: r.get(0)?,
                kind: r.get(1)?,
                ref_entry: r.get(2)?,
                before: r.get(3)?,
                after: r.get(4)?,
                notes: r.get(5)?,
                actor: r.get(6)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| StoreError::NoConfigIntent(id.to_string()))
}

fn notes_of(text: &str) -> Result<Vec<AuditNote>, serde_json::Error> {
    serde_json::from_str(text)
}

/// Moves the intent into History (`reconciled`: at startup); the notes the
/// intent carried come back as the audit entries to write after its own.
pub(crate) fn complete(
    tx: &Transaction,
    id: &str,
    reconciled: bool,
    now: &str,
) -> Result<ConfigAudit, StoreError> {
    let s = summary(tx, id)?;
    let notes = notes_of(&s.notes)
        .map_err(|e| invalid(format!("the intent's audit notes cannot be read: {e}")))?;
    let notes: Vec<(Actor, AuditTriple)> = if notes.is_empty() {
        Vec::new()
    } else {
        // A note records who confirmed the change: the intent's Actor.
        let asked: Actor = serde_json::from_str(&s.actor)
            .map_err(|e| invalid(format!("the intent's actor cannot be read: {e}")))?;
        notes
            .iter()
            .map(|note| (asked.clone(), note_entry(note)))
            .collect()
    };
    tx.execute(
        "INSERT INTO config_history (id, at, actor, file, kind, ref_entry, before_sha256, after_sha256, before_bytes, after_bytes, diff)
         SELECT id, ?2, actor, file, kind, ref_entry, before_sha256, after_sha256, before_bytes, after_bytes, diff
         FROM config_intents WHERE id = ?1",
        params![id, now],
    )?;
    tx.execute("DELETE FROM config_intents WHERE id = ?1", params![id])?;
    let action = if reconciled {
        "config.reconciled"
    } else {
        match HistoryKind::parse(&s.kind) {
            Some(HistoryKind::Edit) => "config.edited",
            Some(HistoryKind::Undo) => "config.undone",
            Some(HistoryKind::Restore) => "config.restored",
            Some(HistoryKind::OutsideEdit) => "config.outside_edit",
            Some(HistoryKind::Migration) => "config.migrated",
            Some(HistoryKind::Create) => "config.created",
            // A removal is a step of a `ConfigMigration` (story 2.13).
            Some(HistoryKind::Removal) => "config.migrated",
            None => return Err(invalid(format!("unknown kind '{}'", s.kind))),
        }
    };
    Ok((
        (
            action.into(),
            id.to_string(),
            Some(args(
                Some(id),
                &s.file,
                &s.kind,
                s.ref_entry.as_deref(),
                &s.before,
                &s.after,
            )),
        ),
        notes,
    ))
}

pub(crate) fn drop_intent(tx: &Transaction, id: &str) -> Result<AuditTriple, StoreError> {
    let s = summary(tx, id)?;
    tx.execute("DELETE FROM config_intents WHERE id = ?1", params![id])?;
    Ok((
        "config.intent_dropped".into(),
        id.to_string(),
        Some(args(
            None,
            &s.file,
            &s.kind,
            s.ref_entry.as_deref(),
            &s.before,
            &s.after,
        )),
    ))
}

// ------------------------------------------------------------------ reads

fn actor_of(text: &str) -> rusqlite::Result<Actor> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn kind_of(text: &str) -> rusqlite::Result<HistoryKind> {
    HistoryKind::parse(text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown History kind '{text}'").into(),
        )
    })
}

const ENTRY_COLUMNS: &str = "id, seq, at, actor, file, kind, ref_entry, before_sha256, after_sha256, before_bytes, after_bytes, diff";

fn entry_of(r: &Row) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: r.get(0)?,
        seq: r.get(1)?,
        at: r.get(2)?,
        actor: actor_of(&r.get::<_, String>(3)?)?,
        file: r.get(4)?,
        kind: kind_of(&r.get::<_, String>(5)?)?,
        ref_entry: r.get(6)?,
        before_sha256: r.get(7)?,
        after_sha256: r.get(8)?,
        before: r.get(9)?,
        after: r.get(10)?,
        diff: r.get(11)?,
    })
}

impl Reader {
    /// Intents committed but neither completed nor dropped, oldest first.
    pub fn config_intents(&self) -> Result<Vec<PendingIntent>, StoreError> {
        let mut st = self.conn.prepare(
            "SELECT id, opened_at, actor, file, kind, ref_entry, before_sha256, after_sha256, before_bytes, after_bytes, diff, audit_notes
             FROM config_intents ORDER BY opened_at, rowid",
        )?;
        let rows = st.query_map([], |r| {
            Ok(PendingIntent {
                id: r.get(0)?,
                at: r.get(1)?,
                actor: actor_of(&r.get::<_, String>(2)?)?,
                intent: NewIntent {
                    file: r.get(3)?,
                    kind: kind_of(&r.get::<_, String>(4)?)?,
                    ref_entry: r.get(5)?,
                    before_sha256: r.get(6)?,
                    after_sha256: r.get(7)?,
                    before: r.get(8)?,
                    after: r.get(9)?,
                    diff: r.get(10)?,
                    audit: notes_of(&r.get::<_, String>(11)?).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?,
                },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// History entries in `seq` order, all or one file's.
    pub fn config_history(&self, file: Option<&str>) -> Result<Vec<HistoryEntry>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM config_history WHERE ?1 IS NULL OR file = ?1 ORDER BY seq"
        ))?;
        let rows = st.query_map(params![file], entry_of)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The latest History entry of `file`, or none.
    pub fn latest_config_entry(&self, file: &str) -> Result<Option<HistoryEntry>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {ENTRY_COLUMNS} FROM config_history WHERE file = ?1 ORDER BY seq DESC LIMIT 1"
                ),
                params![file],
                entry_of,
            )
            .optional()?)
    }

    pub fn config_entry(&self, id: &str) -> Result<Option<HistoryEntry>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ENTRY_COLUMNS} FROM config_history WHERE id = ?1"),
                params![id],
                entry_of,
            )
            .optional()?)
    }
}

// ---------------------------------------------------------------- adapter

/// `am_domain::ConfigHistory` over the project store: commands through the
/// single writer, reads through one read-only connection.
pub struct StoreConfigHistory {
    writer: Writer,
    reader: Mutex<Reader>,
}

impl StoreConfigHistory {
    pub fn new(store: &Store) -> Result<StoreConfigHistory, StoreError> {
        Ok(StoreConfigHistory {
            writer: store.writer(),
            reader: Mutex::new(store.reader()?),
        })
    }

    fn read<T>(&self, f: impl FnOnce(&Reader) -> Result<T, StoreError>) -> Result<T, HistoryError> {
        let reader = self
            .reader
            .lock()
            .map_err(|_| HistoryError::Store("the History reader is unusable".into()))?;
        f(&reader).map_err(history_error)
    }

    fn entry_after(&self, id: &str) -> Result<HistoryEntry, HistoryError> {
        self.read(|r| r.config_entry(id))?
            .ok_or_else(|| HistoryError::Store(format!("History entry {id} was not recorded")))
    }
}

fn history_error(e: StoreError) -> HistoryError {
    match e {
        StoreError::NoConfigIntent(id) => HistoryError::NoIntent(id),
        other => HistoryError::Store(other.to_string()),
    }
}

impl ConfigHistory for StoreConfigHistory {
    fn begin_intent(&self, actor: Actor, intent: NewIntent) -> Result<String, HistoryError> {
        let id = new_id();
        self.writer
            .exec(
                actor,
                WriteCommand::BeginConfigIntent {
                    id: id.clone(),
                    intent,
                },
            )
            .map_err(history_error)?;
        Ok(id)
    }

    fn commit_entry(&self, actor: Actor, intent_id: String) -> Result<HistoryEntry, HistoryError> {
        self.writer
            .exec(
                actor,
                WriteCommand::CommitConfigEntry {
                    intent_id: intent_id.clone(),
                },
            )
            .map_err(history_error)?;
        self.entry_after(&intent_id)
    }

    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), HistoryError> {
        self.writer
            .exec(actor, WriteCommand::DropConfigIntent { intent_id })
            .map_err(history_error)
    }

    fn reconcile_intent(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<HistoryEntry, HistoryError> {
        self.writer
            .exec(
                actor,
                WriteCommand::ReconcileConfigIntent {
                    intent_id: intent_id.clone(),
                },
            )
            .map_err(history_error)?;
        self.entry_after(&intent_id)
    }

    fn pending_intents(&self) -> Result<Vec<PendingIntent>, HistoryError> {
        self.read(|r| r.config_intents())
    }

    fn entries(&self, file: Option<String>) -> Result<Vec<HistoryEntry>, HistoryError> {
        self.read(|r| r.config_history(file.as_deref()))
    }

    fn entry(&self, id: String) -> Result<Option<HistoryEntry>, HistoryError> {
        self.read(|r| r.config_entry(&id))
    }
}
