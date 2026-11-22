//! The hash-chained audit log (AD-25, R10, RULE-16).
//!
//! - Entries are appended only by the writer, in the transaction of the
//!   change they record ([`append`]); `audit_log`'s triggers refuse UPDATE
//!   and DELETE.
//! - Each entry holds ids, hashes and redacted `args` (compact JSON with
//!   sorted keys, built by the store from the command; never content).
//! - [`entry_hash`] is sha256 over `prev_hash`, `at`, `actor`, `action`,
//!   `target` and `outcome`, each followed by a NUL byte, then `args` and a
//!   NUL only when `args` is present. Entries without `args` (written before
//!   schema version 3, and every migration entry) keep the spike's formula,
//!   so older chains verify unchanged and nothing is ever recomputed.
//! - [`Reader::export_audit_jsonl`] writes one JSON object per entry (seq,
//!   at, actor, action, target, outcome, args, prev_hash, entry_hash);
//!   [`verify_jsonl`] recomputes the chain from exactly those fields, with no
//!   database, and names the first broken entry.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, Write};

use am_domain::Actor;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{Reader, StoreError, migrations};

/// The `prev_hash` of the first entry.
pub const GENESIS: &str = "genesis";

/// sha256 hex of `text` (content stands in the log only as this).
pub(crate) fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// The hash of one entry, from its stored text (see the module docs).
pub(crate) fn entry_hash(
    prev: &str,
    at: &str,
    actor: &str,
    action: &str,
    target: &str,
    outcome: &str,
    args: Option<&str>,
) -> String {
    let mut h = Sha256::new();
    for part in [prev, at, actor, action, target, outcome]
        .into_iter()
        .chain(args)
    {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

/// `v` with every object's keys sorted, so its compact text is the same
/// whether or not serde_json keeps insertion order.
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let b: BTreeMap<String, Value> = m.into_iter().map(|(k, v)| (k, sorted(v))).collect();
            Value::Object(b.into_iter().collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// A NUL byte would let a field boundary shift (the hash separates fields
/// with NUL), so no hashed text may hold one.
fn nul_free(fields: &[(&str, &str)]) -> Result<(), String> {
    match fields.iter().find(|(_, v)| v.contains('\0')) {
        Some((k, _)) => Err(format!("field `{k}` holds a NUL byte")),
        None => Ok(()),
    }
}

/// The nine fields of an export line.
const EXPORT_FIELDS: [&str; 9] = [
    "seq",
    "at",
    "actor",
    "action",
    "target",
    "outcome",
    "args",
    "prev_hash",
    "entry_hash",
];

/// The stored (and hashed) text of `args`.
fn args_text(args: &Value) -> String {
    sorted(args.clone()).to_string()
}

/// Append one entry in the caller's transaction. With `args` None the
/// column is not named, so a migration entry can be written at any schema
/// version (the column exists from version 3).
pub(crate) fn append(
    tx: &rusqlite::Transaction,
    actor: &Actor,
    action: &str,
    target: &str,
    outcome: &str,
    args: Option<&Value>,
    at: &str,
) -> Result<(), StoreError> {
    let prev: String = tx
        .query_row(
            "SELECT entry_hash FROM audit_log ORDER BY seq DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| GENESIS.to_string());
    let actor_json =
        serde_json::to_string(actor).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let args = args.map(args_text);
    nul_free(&[
        ("at", at),
        ("actor", &actor_json),
        ("action", action),
        ("target", target),
        ("outcome", outcome),
        ("args", args.as_deref().unwrap_or("")),
    ])
    .map_err(|e| StoreError::Invalid(format!("refused audit entry: {e}")))?;
    let hash = entry_hash(
        &prev,
        at,
        &actor_json,
        action,
        target,
        outcome,
        args.as_deref(),
    );
    match args {
        Some(args) => tx.execute(
            "INSERT INTO audit_log (at, actor, action, target, outcome, args, prev_hash, entry_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![at, actor_json, action, target, outcome, args, prev, hash],
        )?,
        None => tx.execute(
            "INSERT INTO audit_log (at, actor, action, target, outcome, prev_hash, entry_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![at, actor_json, action, target, outcome, prev, hash],
        )?,
    };
    Ok(())
}

/// One stored entry, as text.
struct Row {
    seq: i64,
    at: String,
    actor: String,
    action: String,
    target: String,
    outcome: String,
    args: Option<String>,
    prev_hash: String,
    entry_hash: String,
}

impl Row {
    fn recomputed(&self) -> String {
        entry_hash(
            &self.prev_hash,
            &self.at,
            &self.actor,
            &self.action,
            &self.target,
            &self.outcome,
            self.args.as_deref(),
        )
    }
}

fn exists(conn: &Connection, sql: &str) -> Result<bool, StoreError> {
    Ok(conn.query_row(sql, [], |r| r.get(0))?)
}

/// Every entry in `seq` order. A database from before schema version 3
/// has no `args` column; its entries read as `args` NULL.
fn each_row(
    conn: &Connection,
    mut f: impl FnMut(Row) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    if !exists(
        conn,
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'audit_log')",
    )? {
        return Err(StoreError::Invalid(
            "the database has no audit log yet".into(),
        ));
    }
    let args = if exists(
        conn,
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('audit_log') WHERE name = 'args')",
    )? {
        "args"
    } else {
        "NULL"
    };
    let mut st = conn.prepare(&format!(
        "SELECT seq, at, actor, action, target, outcome, {args}, prev_hash, entry_hash FROM audit_log ORDER BY seq"
    ))?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        f(Row {
            seq: r.get(0)?,
            at: r.get(1)?,
            actor: r.get(2)?,
            action: r.get(3)?,
            target: r.get(4)?,
            outcome: r.get(5)?,
            args: r.get(6)?,
            prev_hash: r.get(7)?,
            entry_hash: r.get(8)?,
        })?;
    }
    Ok(())
}

fn stored_json(what: &str, seq: i64, text: &str) -> Result<Value, StoreError> {
    serde_json::from_str(text)
        .map_err(|e| StoreError::Invalid(format!("audit entry {seq}: its {what} is not JSON: {e}")))
}

impl Reader {
    /// Write the audit log as JSON Lines to `out`: one object per entry, in
    /// `seq` order. Returns the entry count and the head (last) hash
    /// ([`GENESIS`] for an empty log). A database newer than this binary is
    /// refused before anything is written.
    pub fn export_audit_jsonl(&self, out: &mut dyn Write) -> Result<(u64, String), StoreError> {
        migrations::refuse_newer(migrations::user_version(&self.conn)?)?;
        let mut count = 0u64;
        let mut head = GENESIS.to_string();
        each_row(&self.conn, |row| {
            let mut obj = Map::new();
            obj.insert("seq".into(), row.seq.into());
            obj.insert("at".into(), row.at.into());
            obj.insert("actor".into(), stored_json("actor", row.seq, &row.actor)?);
            obj.insert("action".into(), row.action.into());
            obj.insert("target".into(), row.target.into());
            obj.insert("outcome".into(), row.outcome.into());
            let args = match &row.args {
                Some(a) => stored_json("args", row.seq, a)?,
                None => Value::Null,
            };
            obj.insert("args".into(), args);
            obj.insert("prev_hash".into(), row.prev_hash.into());
            obj.insert("entry_hash".into(), row.entry_hash.clone().into());
            writeln!(out, "{}", Value::Object(obj))
                .map_err(|e| StoreError::io("cannot write the audit export".into(), e))?;
            count += 1;
            head = row.entry_hash;
            Ok(())
        })?;
        out.flush()
            .map_err(|e| StoreError::io("cannot write the audit export".into(), e))?;
        Ok((count, head))
    }

    /// The `args` of every entry of `action` on `target`, oldest first: each
    /// as its text values by key (what an [`am_domain::AuditNote`] holds). An
    /// entry without `args` gives an empty map, and a value that is not
    /// text is left out. Read-only and lock-free, so `agentic accounts
    /// test` can find what the latest `account_added` note of an Account
    /// recorded (story 2.3).
    pub fn audit_args(
        &self,
        action: &str,
        target: &str,
    ) -> Result<Vec<BTreeMap<String, String>>, StoreError> {
        migrations::refuse_newer(migrations::user_version(&self.conn)?)?;
        let mut found = Vec::new();
        each_row(&self.conn, |row| {
            if row.action != action || row.target != target {
                return Ok(());
            }
            let mut args = BTreeMap::new();
            if let Some(text) = &row.args
                && let Value::Object(map) = stored_json("args", row.seq, text)?
            {
                for (key, value) in map {
                    if let Value::String(value) = value {
                        args.insert(key, value);
                    }
                }
            }
            found.push(args);
            Ok(())
        })?;
        Ok(found)
    }

    /// Every entry of `action` on `target` that a committed config intent
    /// wrote as one of its notes, oldest first: written right after that
    /// intent's History entry (`config.edited`, `config.created`, …), in the
    /// same transaction (the same `at`). Each comes with its actor (the
    /// intent's) and the file of its History entry, so a caller can accept
    /// only the owner's note on the right file. An entry put into the log
    /// any other way is left out (story 2.2: the Account confirmation of
    /// AD-15's Key use).
    pub fn config_note_args(
        &self,
        action: &str,
        target: &str,
    ) -> Result<Vec<ConfigNote>, StoreError> {
        migrations::refuse_newer(migrations::user_version(&self.conn)?)?;
        const ENTRIES: [&str; 7] = [
            "config.edited",
            "config.undone",
            "config.restored",
            "config.outside_edit",
            "config.migrated",
            "config.created",
            "config.reconciled",
        ];
        let text_args = |row: &Row| -> Result<BTreeMap<String, String>, StoreError> {
            let mut args = BTreeMap::new();
            if let Some(text) = &row.args
                && let Value::Object(map) = stored_json("args", row.seq, text)?
            {
                for (key, value) in map {
                    if let Value::String(value) = value {
                        args.insert(key, value);
                    }
                }
            }
            Ok(args)
        };
        let mut found = Vec::new();
        // The `at` and file of the History entry whose notes may follow.
        let mut open: Option<(String, String)> = None;
        each_row(&self.conn, |row| {
            if ENTRIES.contains(&row.action.as_str()) {
                let file = text_args(&row)?.remove("file").unwrap_or_default();
                open = Some((row.at.clone(), file));
                return Ok(());
            }
            let Some((_, file)) = open.clone().filter(|(at, _)| *at == row.at) else {
                open = None;
                return Ok(());
            };
            if row.action != action || row.target != target {
                return Ok(());
            }
            let actor: Actor = serde_json::from_str(&row.actor).map_err(|e| {
                StoreError::Invalid(format!(
                    "audit entry {}: its actor cannot be read: {e}",
                    row.seq
                ))
            })?;
            found.push(ConfigNote {
                seq: row.seq,
                actor,
                file,
                args: text_args(&row)?,
            });
            Ok(())
        })?;
        Ok(found)
    }

    /// Recompute the stored chain; returns the first broken sequence number.
    pub fn verify_audit_chain(&self) -> Result<Option<i64>, StoreError> {
        let mut prev = GENESIS.to_string();
        let mut broken = None;
        each_row(&self.conn, |row| {
            if broken.is_none() {
                if row.prev_hash != prev || row.recomputed() != row.entry_hash {
                    broken = Some(row.seq);
                }
                prev = row.entry_hash;
            }
            Ok(())
        })?;
        Ok(broken)
    }
}

/// A verified export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainOk {
    /// The number of entries.
    pub entries: u64,
    /// The last entry's hash ([`GENESIS`] for an empty export).
    pub head: String,
}

/// The first place an export's chain breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBreak {
    /// 1-based line number in the export.
    pub line: u64,
    /// The entry's `seq`, when the line has one.
    pub seq: Option<i64>,
    pub reason: String,
}

impl fmt::Display for ChainBreak {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.seq {
            Some(seq) => write!(f, "line {}, entry {seq}: {}", self.line, self.reason),
            None => write!(f, "line {}: {}", self.line, self.reason),
        }
    }
}

impl std::error::Error for ChainBreak {}

fn text_field(obj: &Map<String, Value>, key: &str) -> Result<String, String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("field `{key}` is missing or not text"))
}

/// One export line, back to the stored text its hash covers.
fn parse_line(text: &str) -> Result<Row, (Option<i64>, String)> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| (None, format!("is not JSON: {e}")))?;
    let Value::Object(obj) = value else {
        return Err((None, "is not a JSON object".into()));
    };
    let seq = obj
        .get("seq")
        .and_then(Value::as_i64)
        .ok_or_else(|| (None, "has no whole-number `seq`".to_string()))?;
    let bad = |reason: String| (Some(seq), reason);
    if let Some(k) = obj.keys().find(|k| !EXPORT_FIELDS.contains(&k.as_str())) {
        return Err(bad(format!("has a field `{k}` that no export holds")));
    }
    let field = |key: &str| text_field(&obj, key).map_err(bad);
    let given = obj
        .get("actor")
        .ok_or_else(|| bad("field `actor` is missing".to_string()))?;
    let actor: Actor = serde_json::from_value(given.clone())
        .map_err(|e| bad(format!("`actor` is not an actor this app writes: {e}")))?;
    // Only the exact form the store writes: no extra keys, no other spelling.
    let written = serde_json::to_value(&actor).map_err(|e| bad(e.to_string()))?;
    if &written != given {
        return Err(bad("`actor` is not in the form this app writes".into()));
    }
    let actor = serde_json::to_string(&actor).map_err(|e| bad(e.to_string()))?;
    let args = match obj.get("args") {
        None => return Err(bad("field `args` is missing".into())),
        Some(Value::Null) => None,
        Some(a) => Some(args_text(a)),
    };
    let row = Row {
        seq,
        at: field("at")?,
        actor,
        action: field("action")?,
        target: field("target")?,
        outcome: field("outcome")?,
        args,
        prev_hash: field("prev_hash")?,
        entry_hash: field("entry_hash")?,
    };
    nul_free(&[
        ("at", &row.at),
        ("actor", &row.actor),
        ("action", &row.action),
        ("target", &row.target),
        ("outcome", &row.outcome),
        ("args", row.args.as_deref().unwrap_or("")),
        ("prev_hash", &row.prev_hash),
        ("entry_hash", &row.entry_hash),
    ])
    .map_err(bad)?;
    Ok(row)
}

/// Check a JSON Lines export (from [`Reader::export_audit_jsonl`]) with no
/// database: every line must be an entry with exactly the nine export
/// fields and no NUL byte, `seq` running 1, 2, 3, … with no gap (rows are
/// never deleted and a failed insert rolls back, so a real log is
/// contiguous), the first `prev_hash` [`GENESIS`], each later one the hash
/// of the line before, and each `entry_hash` recomputed from its fields.
/// Returns the first break.
pub fn verify_jsonl(input: impl BufRead) -> Result<ChainOk, ChainBreak> {
    let mut prev = GENESIS.to_string();
    let mut prev_seq: Option<i64> = None;
    let mut entries = 0u64;
    for (i, line) in input.lines().enumerate() {
        let line_no = i as u64 + 1;
        let brk = |seq: Option<i64>, reason: String| ChainBreak {
            line: line_no,
            seq,
            reason,
        };
        let text = line.map_err(|e| brk(None, format!("cannot be read: {e}")))?;
        let row = parse_line(&text).map_err(|(seq, reason)| brk(seq, reason))?;
        let seq = Some(row.seq);
        let expected = prev_seq.map_or(1, |p| p + 1);
        if row.seq != expected {
            return Err(brk(
                seq,
                format!(
                    "is not the next entry: expected entry {expected}, found entry {}; an entry is missing, out of place or renumbered",
                    row.seq
                ),
            ));
        }
        if row.prev_hash != prev {
            return Err(brk(
                seq,
                match prev_seq {
                    None => "`prev_hash` is not the start of the chain (genesis): an entry before it is missing".into(),
                    Some(p) => format!(
                        "`prev_hash` does not match the entry before it (entry {p}): an entry is missing or out of place"
                    ),
                },
            ));
        }
        if row.recomputed() != row.entry_hash {
            return Err(brk(
                seq,
                "`entry_hash` does not match its fields: the entry was altered".into(),
            ));
        }
        prev = row.entry_hash;
        prev_seq = seq;
        entries += 1;
    }
    Ok(ChainOk {
        entries,
        head: prev,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formula of the spike (story 1.6), written out independently.
    #[test]
    fn without_args_the_hash_is_the_spike_formula() {
        let mut h = Sha256::new();
        for p in ["genesis", "t", "a", "x", "y", "ok"] {
            h.update(p.as_bytes());
            h.update([0u8]);
        }
        assert_eq!(
            entry_hash("genesis", "t", "a", "x", "y", "ok", None),
            hex::encode(h.finalize())
        );
        assert_ne!(
            entry_hash("genesis", "t", "a", "x", "y", "ok", None),
            entry_hash("genesis", "t", "a", "x", "y", "ok", Some("{}"))
        );
    }

    #[test]
    fn args_text_has_sorted_keys() {
        let v = serde_json::json!({"b": 1, "a": {"d": null, "c": "x"}});
        assert_eq!(args_text(&v), r#"{"a":{"c":"x","d":null},"b":1}"#);
    }

    #[test]
    fn a_break_names_line_and_entry() {
        let b = ChainBreak {
            line: 4,
            seq: Some(9),
            reason: "r".into(),
        };
        assert_eq!(b.to_string(), "line 4, entry 9: r");
        let b = ChainBreak {
            line: 2,
            seq: None,
            reason: "is not JSON".into(),
        };
        assert_eq!(b.to_string(), "line 2: is not JSON");
    }
}

/// A note a committed config intent wrote ([`Reader::config_note_args`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigNote {
    /// The entry's place in the audit chain (its `seq`): later notes, of
    /// any action, have a higher one.
    pub seq: i64,
    /// Who confirmed the change: the intent's Actor.
    pub actor: Actor,
    /// The file of the History entry the note was written with.
    pub file: String,
    /// The note's text arguments by key.
    pub args: BTreeMap<String, String>,
}
