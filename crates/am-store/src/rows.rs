//! Rows with their history (epic 3 story 6, the store slice; AD-8, AD-10,
//! AD-21, AD-24, AD-25).
//!
//! - [`crate::Writer::apply_row_write`] is the one row command: a seat's
//!   insert or update of one row, keyed by its step run and call sequence.
//!   In one transaction it writes the after-image (`row_history`, the whole
//!   row after the change with the Actor, run, step run, seat, call
//!   sequence and tool call), then the row (`table_rows`), finishes the
//!   journaled `table_write` call `succeeded` with its result kind `engine`
//!   (AD-10 as amended: on its tool call's journal row, AD-8), and appends
//!   `row.written` (ids, op, version, call sequence, taint and a column
//!   count; never the row's data, nor a hash of it).
//! - A key that already holds its after-image writes nothing and is `Ok`
//!   (the replay, checked first), but only for the same write again: the
//!   same tool call id, actor, table and op kind (the ToolGate slice's
//!   review round 1, R4); anything else at the key is refused in words. Everything else is checked against the
//!   journal and refused in words with nothing written: the actor must be
//!   the attempt's own `Actor::Seat` (its run, its step and the seat
//!   journaled when it started), the attempt `running`, the call at the key
//!   an `issued` `table_write` call with this id, the table the one the
//!   attempt journaled; an insert needs a new row id and an update the
//!   row's current version (owner Q2: [`StoreError::RowRefused`]). The
//!   run must be open (review R8), and the row stays within
//!   [`ROW_MAX_BYTES`] and [`ROW_MAX_COLUMNS`] (review R10). A refusal names
//!   the attempt's journaled table, never the write's (review R12).
//! - A row's taint is its own, stored on write: the highest of the
//!   attempt's committed taint, the write's and the row's. It only rises,
//!   and each after-image records it.
//! - The data is kept as the seat wrote it (review R9): SQLite's `json()`
//!   stores an insert minified with each number's text as written, and an
//!   update's patch is merged in SQL, one top-level key at a time
//!   (`json_set`, or `json_remove` for a `null`).
//!
//! The readers of row content ([`Reader::table_rows_of`],
//! [`Reader::table_row_by_id`], [`Reader::row_history_of`],
//! [`Reader::row_written_at`]) are for tests: their names are unique in
//! `crates/`, and no shipped code outside this crate names them (`cargo
//! xtask check-deps`).
//! [`Reader::row_receipt`] reads the typed record a result is rebuilt from,
//! which holds no data.
//!
//! Story 3.22 (AD-10, AD-43): [`Reader::row_snapshot`] is a row's current
//! after-image, [`RowSnapshot`]: the one content reader shipped code
//! outside this crate names, and only in am-engine's `row_inputs` module,
//! which hands a step the rows it reads as fenced untrusted data (`cargo
//! xtask check-deps`).

use am_domain::rows::{RowOp, RowWrite};
use am_domain::{Actor, StepStatus, Taint};
use std::fmt;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use crate::{
    AttemptWrites, Reader, StepState, StoreError, audit, flag, now_ts, run_open_to, step_of,
    taint_of,
};

/// The most bytes a row's stored JSON text may hold after a write (review
/// R10): 64 KiB, the size the config already allows a skill's body. A row
/// is a record of columns, and every update stores the whole row again as
/// its after-image, so the cap also bounds what one write adds to history.
pub const ROW_MAX_BYTES: usize = 64 * 1024;

/// The most top-level columns a row may hold after a write (review R10):
/// far past what a config table declares, yet a bound on a write that
/// would spread a model's output over thousands of keys.
pub const ROW_MAX_COLUMNS: usize = 256;

/// The one tool whose successful call the writer finishes itself.
const ROW_TOOL: &str = "table_write";

/// Its result's label on the tool call's journal row (AD-10 as amended).
const ENGINE_LABEL: &str = "tool:table_write";

/// One current row of a table (test-facing: it carries the row's data,
/// which its `Debug` never prints, review T6).
#[derive(Clone, PartialEq, Eq)]
pub struct RowRecord {
    pub table_id: String,
    pub row_id: String,
    /// The row's columns, a JSON object.
    pub data: String,
    pub version: i64,
    /// The row's own taint (AD-10): it only rises.
    pub taint: Taint,
    pub created_at: String,
    pub updated_at: String,
}

/// One after-image of a row (test-facing: it carries the row's data, which
/// its `Debug` never prints, review T6).
#[derive(Clone, PartialEq, Eq)]
pub struct RowHistoryRecord {
    pub seq: i64,
    pub table_id: String,
    pub row_id: String,
    pub version: i64,
    /// The whole row after the change.
    pub data: String,
    /// The row's taint after this write.
    pub taint: Taint,
    pub actor: Actor,
    /// A seat's write's provenance; `None` for a Human or Import write.
    pub run_id: Option<String>,
    pub step_run_id: Option<String>,
    pub seat: Option<String>,
    pub call_seq: Option<i64>,
    pub tool_call_id: Option<String>,
    /// `insert` or `update`.
    pub op: String,
    pub created_at: String,
}

/// The record of a seat's write that a result is rebuilt from (AD-10 as
/// amended): ids, the version, the op, the taint and how many columns the
/// row holds after the write; never the row's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowReceipt {
    pub table_id: String,
    pub row_id: String,
    pub version: i64,
    /// `insert` or `update`.
    pub op: String,
    pub taint: Taint,
    pub step_run_id: String,
    pub call_seq: i64,
    pub tool_call_id: String,
    pub columns: i64,
}

/// A row's current after-image (story 3.22): its `row_history.seq`, the
/// version, the data as stored and the row's taint after that write. It
/// carries the row's data, which its `Debug` never prints (review T6).
#[derive(Clone, PartialEq, Eq)]
pub struct RowSnapshot {
    pub seq: i64,
    pub version: i64,
    /// The whole row, a JSON object.
    pub data: String,
    pub taint: Taint,
}

impl fmt::Debug for RowSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowSnapshot")
            .field("seq", &self.seq)
            .field("version", &self.version)
            .field("data_bytes", &self.data.len())
            .field("taint", &self.taint)
            .finish()
    }
}

/// The label a `row` input reading after-image `seq` carries
/// (`row:<table>/<row id>@<version>`), with that image's taint; `None` when
/// no after-image has that `seq`. Ids only, never the row's data.
pub(crate) fn image_label(
    conn: &Connection,
    seq: i64,
) -> Result<Option<(String, Taint)>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT table_id, row_id, version, taint FROM row_history WHERE seq = ?1",
            params![seq],
            |r| {
                let (table, row, version): (String, String, i64) =
                    (r.get(0)?, r.get(1)?, r.get(2)?);
                Ok((format!("row:{table}/{row}@{version}"), taint_of(r.get(3)?)))
            },
        )
        .optional()?)
}

/// `Some(words)` when the after-image `seq` is not its row's current one:
/// the words name the row, its table and its version now (ids only).
pub(crate) fn image_moved(conn: &Connection, seq: i64) -> Result<Option<String>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT h.table_id, h.row_id, t.version
             FROM row_history h
             JOIN table_rows t ON t.table_id = h.table_id AND t.row_id = h.row_id
             WHERE h.seq = ?1 AND t.version <> h.version",
            params![seq],
            |r| {
                let (table, row, version): (String, String, i64) =
                    (r.get(0)?, r.get(1)?, r.get(2)?);
                Ok(format!(
                    "row {row} of table `{table}` is at version {version} now"
                ))
            },
        )
        .optional()?)
}

impl fmt::Debug for RowRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowRecord")
            .field("table_id_bytes", &self.table_id.len())
            .field("row_id", &self.row_id)
            .field("data_bytes", &self.data.len())
            .field("version", &self.version)
            .field("taint", &self.taint)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl fmt::Debug for RowHistoryRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowHistoryRecord")
            .field("seq", &self.seq)
            .field("table_id_bytes", &self.table_id.len())
            .field("row_id", &self.row_id)
            .field("version", &self.version)
            .field("data_bytes", &self.data.len())
            .field("taint", &self.taint)
            .field("actor", &self.actor)
            .field("run_id", &self.run_id)
            .field("step_run_id", &self.step_run_id)
            .field("seat", &self.seat)
            .field("call_seq", &self.call_seq)
            .field("tool_call_id", &self.tool_call_id)
            .field("op", &self.op)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// The row command (see the module docs), in its own transaction.
pub(crate) fn apply_row_write(
    conn: &mut Connection,
    actor: &Actor,
    write: &RowWrite,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // AD-8: a key that holds its write is answered by it; nothing more is
    // written, whatever the attempt's state (story 3.9's resume replays a
    // call of a finished attempt). It is a replay only when the row command
    // wrote it (the call at the key finished with its `engine` result in
    // the same transaction, review T5) and only for the same write again:
    // the same tool call id, actor, table and op kind (the ToolGate slice's
    // review round 1, R4).
    let replay: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM row_history WHERE step_run_id = ?1 AND call_seq = ?2)",
        params![write.step_run_id, write.call_seq],
        |r| r.get(0),
    )?;
    if replay {
        let kind: Option<Option<String>> = tx
            .query_row(
                "SELECT result_kind FROM tool_calls WHERE step_run_id = ?1 AND call_seq = ?2",
                params![write.step_run_id, write.call_seq],
                |r| r.get(0),
            )
            .optional()?;
        if kind.flatten().as_deref() != Some("engine") {
            return Err(refused(format!(
                "call {} of step run {} holds an after-image that the row command did not write (its tool call has no engine result), so it is no replay",
                write.call_seq, write.step_run_id
            )));
        }
        return same_write(&tx, actor, write);
    }
    let now = now_ts();
    let (target, args) = checked_write(&tx, actor, write, &now)?;
    audit::append(&tx, actor, "row.written", &target, "ok", Some(&args), &now)?;
    tx.commit()?;
    Ok(())
}

fn refused(words: String) -> StoreError {
    StoreError::Invalid(format!("refused: {words}; nothing was changed"))
}

/// A refusal of the data a seat wrote (a repeated key, a row or patch past
/// a cap; the ToolGate slice's review round 1, R1): words the model may act
/// on, so the step goes on. The guards are the store slice's, unchanged.
fn data_refused(words: String) -> StoreError {
    StoreError::RowRefused(format!("refused: {words}; nothing was changed"))
}

/// A replay (R4): the write at the key must be this one again, its tool
/// call id, its actor, its table and its op kind; the row id and the data
/// may differ (a re-run mints a new row id). Anything else is refused in
/// words that name neither data nor the write's table.
fn same_write(tx: &Transaction, actor: &Actor, write: &RowWrite) -> Result<(), StoreError> {
    let (tool_call_id, by, table, op): (String, String, String, String) = tx.query_row(
        "SELECT tool_call_id, actor, table_id, op FROM row_history WHERE step_run_id = ?1 AND call_seq = ?2",
        params![write.step_run_id, write.call_seq],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let by = serde_json::from_str::<Actor>(&by).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let kind = match write.op {
        RowOp::Insert { .. } => "insert",
        RowOp::Update { .. } => "update",
    };
    if tool_call_id == write.tool_call_id && by == *actor && table == write.table_id && op == kind {
        return Ok(());
    }
    Err(refused(format!(
        "call {} of step run {} holds the {op} of tool call {tool_call_id}, and this write is not that write again (another tool call, actor, table or op), so it is no replay of it",
        write.call_seq, write.step_run_id
    )))
}

/// Checks the write against the journal, then writes its after-image, its
/// row and its call's finish; the audit entry's target and arguments.
fn checked_write(
    tx: &Transaction,
    actor: &Actor,
    write: &RowWrite,
    now: &str,
) -> Result<(String, Value), StoreError> {
    let step = step_of(tx, &write.step_run_id)?;
    let (seat, table) = journaled_writes(tx, &step.id)?;
    the_attempts_seat(actor, &step, seat.as_deref())?;
    if step.status != StepStatus::Running {
        return Err(refused(format!(
            "step run {} ({} attempt {}) is {}, and only a running attempt writes a row",
            step.id,
            step.step_key,
            step.attempt,
            step.status.as_str()
        )));
    }
    run_open_to(tx, &step, "writes a row")?;
    issued_call(tx, write)?;
    // Review R12: the words name the attempt's journaled table, never the
    // write's, which the model may one day have chosen.
    let table = match table {
        Some(t) if t == write.table_id => t,
        Some(t) => {
            return Err(refused(format!(
                "step run {} writes only the table `{t}`, and this write names another (AD-21)",
                step.id
            )));
        }
        None => {
            return Err(refused(format!(
                "step run {} declares no table, so it writes no row (AD-21)",
                step.id
            )));
        }
    };
    let seat = seat.unwrap_or_default();
    let (row_id, op, data, version, taint) = match &write.op {
        RowOp::Insert { row_id, data_json } => {
            let row_id = row_id.to_string();
            if current(tx, &table, &row_id)?.is_some() {
                return Err(refused(format!(
                    "table `{table}` already holds row `{row_id}`, and an insert never replaces a row"
                )));
            }
            let data = object(tx, data_json, "the row's data")?;
            (row_id, "insert", data, 1, step.taint.raise(write.taint))
        }
        RowOp::Update {
            row_id,
            expected_version,
            patch_json,
        } => {
            let row_id = row_id.to_string();
            let Some((data, version, row_taint)) = current(tx, &table, &row_id)? else {
                return Err(StoreError::RowRefused(format!(
                    "refused: the update names a row this table does not hold (table `{table}`); nothing was changed"
                )));
            };
            if version != *expected_version {
                return Err(StoreError::RowRefused(format!(
                    "refused: row `{row_id}` is at version {version}, and this update was read at another version; read the row again and write against version {version}; nothing was changed"
                )));
            }
            // Review T3: a patch past either cap is refused before it is
            // parsed further or merged, on the one writer thread.
            if patch_json.len() > ROW_MAX_BYTES {
                return Err(data_refused(format!(
                    "the update's patch holds {} bytes, over the cap of {ROW_MAX_BYTES} bytes a row",
                    patch_json.len()
                )));
            }
            let patch = object(tx, patch_json, "the update's patch")?;
            let keys = columns_of(tx, &patch)?;
            if usize::try_from(keys).map_or(true, |n| n > ROW_MAX_COLUMNS) {
                return Err(data_refused(format!(
                    "the update's patch names {keys} columns, over the cap of {ROW_MAX_COLUMNS} columns a row"
                )));
            }
            let data = merged(tx, data, &patch)?;
            let taint = step.taint.raise(write.taint).raise(row_taint);
            (row_id, "update", data, version + 1, taint)
        }
    };
    let patch = match &write.op {
        RowOp::Insert { .. } => None,
        RowOp::Update { patch_json, .. } => Some(patch_json.len()),
    };
    let columns = within_caps(tx, &data, patch)?;
    let actor_json =
        serde_json::to_string(actor).map_err(|e| StoreError::Invalid(e.to_string()))?;
    // The after-image first: the row's triggers need it (AD-21, AD-24).
    tx.execute(
        "INSERT INTO row_history (table_id, row_id, version, data, taint, actor, run_id, step_run_id, seat, call_seq, tool_call_id, op, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            table,
            row_id,
            version,
            data,
            flag(taint),
            actor_json,
            step.run_id,
            step.id,
            seat,
            write.call_seq,
            write.tool_call_id,
            op,
            now
        ],
    )?;
    if op == "insert" {
        tx.execute(
            "INSERT INTO table_rows (table_id, row_id, data, version, taint, created_at, updated_at) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?5)",
            params![table, row_id, data, flag(taint), now],
        )?;
    } else {
        tx.execute(
            "UPDATE table_rows SET data = ?3, version = ?4, taint = ?5, updated_at = ?6 WHERE table_id = ?1 AND row_id = ?2",
            params![table, row_id, data, version, flag(taint), now],
        )?;
    }
    let finished = tx.execute(
        "UPDATE tool_calls SET status = 'succeeded', finished_at = ?2, result_kind = 'engine', result_label = ?3 WHERE id = ?1 AND status = 'issued'",
        params![write.tool_call_id, now, ENGINE_LABEL],
    )?;
    if finished != 1 {
        return Err(refused(format!(
            "tool call {} is not an issued call",
            write.tool_call_id
        )));
    }
    Ok((
        format!("row:{table}/{row_id}"),
        json!({
            "call_seq": write.call_seq,
            "columns": columns,
            "op": op,
            "step_run_id": write.step_run_id,
            "taint": flag(taint),
            "tool_call_id": write.tool_call_id,
            "version": version,
        }),
    ))
}

/// The seat and the table the attempt journaled when it started.
fn journaled_writes(
    conn: &Connection,
    step_run_id: &str,
) -> Result<(Option<String>, Option<String>), StoreError> {
    Ok(conn
        .query_row(
            "SELECT seat, writes_table FROM step_runs WHERE id = ?1",
            params![step_run_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((None, None)))
}

/// AD-21, AD-24: only the attempt's own seat writes its rows.
fn the_attempts_seat(
    actor: &Actor,
    step: &StepState,
    seat: Option<&str>,
) -> Result<(), StoreError> {
    let Actor::Seat {
        seat: by,
        run,
        step: key,
    } = actor
    else {
        return Err(refused(format!(
            "only the seat running step run {} writes its rows (AD-21), and {} is not a seat",
            step.id,
            actor.describe()
        )));
    };
    if run.to_string() != step.run_id || *key != step.step_key || Some(by.as_str()) != seat {
        return Err(refused(format!(
            "{} is not the seat running step run {} ({} attempt {} of run {}), which alone writes its rows (AD-21)",
            actor.describe(),
            step.id,
            step.step_key,
            step.attempt,
            step.run_id
        )));
    }
    Ok(())
}

/// The call at the write's key is an `issued` `table_write` call with the
/// write's tool call id.
fn issued_call(conn: &Connection, write: &RowWrite) -> Result<(), StoreError> {
    let call: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, tool, status FROM tool_calls WHERE step_run_id = ?1 AND call_seq = ?2",
            params![write.step_run_id, write.call_seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let at = format!("call {} of step run {}", write.call_seq, write.step_run_id);
    let Some((id, tool, status)) = call else {
        return Err(refused(format!("{at} journaled no tool call")));
    };
    if id != write.tool_call_id {
        return Err(refused(format!(
            "the tool call at {at} is {id}, not {}",
            write.tool_call_id
        )));
    }
    if tool != ROW_TOOL {
        return Err(refused(format!(
            "the tool call at {at} is not a {ROW_TOOL} call"
        )));
    }
    if status != "issued" {
        return Err(refused(format!(
            "the tool call at {at} is {status}, and a row is written only for an issued call"
        )));
    }
    Ok(())
}

/// The row's data, version and taint, as committed.
fn current(
    conn: &Connection,
    table_id: &str,
    row_id: &str,
) -> Result<Option<(String, i64, Taint)>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT data, version, taint FROM table_rows WHERE table_id = ?1 AND row_id = ?2",
            params![table_id, row_id],
            |r| Ok((r.get(0)?, r.get(1)?, taint_of(r.get(2)?))),
        )
        .optional()?)
}

/// `text` as SQLite stores a JSON object (review R9): minified, each
/// number's text as written. Anything but strict RFC 8259 JSON text holding
/// an object, or an object that repeats a key at any depth (review T2:
/// SQLite keeps both, and readers would disagree on which counts), is
/// refused without echoing it.
fn object(conn: &Connection, text: &str, what: &str) -> Result<String, StoreError> {
    let refuse = || refused(format!("{what} is not a JSON object of columns"));
    let valid: bool = conn.query_row("SELECT json_valid(?1)", params![text], |r| r.get(0))?;
    if !valid {
        return Err(refuse());
    }
    let (minified, kind): (String, String) =
        conn.query_row("SELECT json(?1), json_type(?1)", params![text], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    if kind != "object" {
        return Err(refuse());
    }
    let repeated: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM json_tree(?1) WHERE typeof(key) = 'text' GROUP BY parent, key HAVING count(*) > 1)",
        params![minified],
        |r| r.get(0),
    )?;
    if repeated {
        return Err(data_refused(format!(
            "{what} holds a duplicate key, and a key appears once in a row"
        )));
    }
    Ok(minified)
}

/// How many top-level keys the JSON object `text` holds.
fn columns_of(conn: &Connection, text: &str) -> Result<i64, StoreError> {
    Ok(
        conn.query_row("SELECT count(*) FROM json_each(?1)", params![text], |r| {
            r.get(0)
        })?,
    )
}

/// `data` with `patch`'s top-level keys merged in SQL (review R9): each
/// key's value replaces the column (`json_set`, its text as written), and
/// a `null` removes it (`json_remove`); the other columns are kept.
fn merged(conn: &Connection, data: String, patch: &str) -> Result<String, StoreError> {
    let mut st = conn.prepare("SELECT fullkey, type, ?1 -> fullkey FROM json_each(?1)")?;
    let members: Vec<(String, String, Option<String>)> = st
        .query_map(params![patch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut data = data;
    for (path, kind, value) in members {
        data = match value {
            Some(value) if kind != "null" => conn.query_row(
                "SELECT json_set(?1, ?2, json(?3))",
                params![data, path, value],
                |r| r.get(0),
            )?,
            _ => conn.query_row("SELECT json_remove(?1, ?2)", params![data, path], |r| {
                r.get(0)
            })?,
        };
    }
    Ok(data)
}

/// The row's column count, once its text and columns are within
/// [`ROW_MAX_BYTES`] and [`ROW_MAX_COLUMNS`] (review R10); refused in
/// words with sizes only. An insert's words name its own size. An update's
/// (`patch`: its patch's byte length) name the caps and the patch's own
/// size, never a size of the merged or the stored row, which the seat
/// cannot read (the ToolGate slice's review round 2, T3).
fn within_caps(conn: &Connection, data: &str, patch: Option<usize>) -> Result<i64, StoreError> {
    let over_bytes = data.len() > ROW_MAX_BYTES;
    let columns = if over_bytes {
        0
    } else {
        columns_of(conn, data)?
    };
    let over_columns = usize::try_from(columns).map_or(true, |n| n > ROW_MAX_COLUMNS);
    let words = match (patch, over_bytes, over_columns) {
        (_, false, false) => return Ok(columns),
        (None, true, _) => format!(
            "the row would hold {} bytes of JSON, over the cap of {ROW_MAX_BYTES} bytes a row",
            data.len()
        ),
        (None, false, true) => format!(
            "the row would hold {columns} columns, over the cap of {ROW_MAX_COLUMNS} columns a row"
        ),
        (Some(bytes), true, _) => format!(
            "this update (its patch holds {bytes} bytes) would take the row past the cap of {ROW_MAX_BYTES} bytes a row; write a smaller patch, or clear columns with null"
        ),
        (Some(_), false, true) => format!(
            "this update would take the row past the cap of {ROW_MAX_COLUMNS} columns a row; set fewer new columns, or clear columns with null"
        ),
    };
    Err(data_refused(words))
}

const ROW_COLUMNS: &str = "table_id, row_id, data, version, taint, created_at, updated_at";

fn row_record(r: &rusqlite::Row) -> rusqlite::Result<RowRecord> {
    Ok(RowRecord {
        table_id: r.get(0)?,
        row_id: r.get(1)?,
        data: r.get(2)?,
        version: r.get(3)?,
        taint: taint_of(r.get(4)?),
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

const HISTORY_COLUMNS: &str = "seq, table_id, row_id, version, data, taint, actor, run_id, step_run_id, seat, call_seq, tool_call_id, op, created_at";

fn history_record(r: &rusqlite::Row) -> rusqlite::Result<RowHistoryRecord> {
    let actor: String = r.get(6)?;
    let actor = serde_json::from_str::<Actor>(&actor)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, Type::Text, Box::new(e)))?;
    Ok(RowHistoryRecord {
        seq: r.get(0)?,
        table_id: r.get(1)?,
        row_id: r.get(2)?,
        version: r.get(3)?,
        data: r.get(4)?,
        taint: taint_of(r.get(5)?),
        actor,
        run_id: r.get(7)?,
        step_run_id: r.get(8)?,
        seat: r.get(9)?,
        call_seq: r.get(10)?,
        tool_call_id: r.get(11)?,
        op: r.get(12)?,
        created_at: r.get(13)?,
    })
}

impl Reader {
    /// The current rows of `table_id`, in creation order (test-facing).
    pub fn table_rows_of(&self, table_id: &str) -> Result<Vec<RowRecord>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM table_rows WHERE table_id = ?1 ORDER BY created_at, rowid"
        ))?;
        let rows = st.query_map(params![table_id], row_record)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One current row (test-facing).
    pub fn table_row_by_id(
        &self,
        table_id: &str,
        row_id: &str,
    ) -> Result<Option<RowRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {ROW_COLUMNS} FROM table_rows WHERE table_id = ?1 AND row_id = ?2"
                ),
                params![table_id, row_id],
                row_record,
            )
            .optional()?)
    }

    /// Every after-image of one row, in version order (test-facing).
    pub fn row_history_of(
        &self,
        table_id: &str,
        row_id: &str,
    ) -> Result<Vec<RowHistoryRecord>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {HISTORY_COLUMNS} FROM row_history WHERE table_id = ?1 AND row_id = ?2 ORDER BY version"
        ))?;
        let rows = st.query_map(params![table_id, row_id], history_record)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The after-image a seat's write wrote at its key (test-facing).
    pub fn row_written_at(
        &self,
        step_run_id: &str,
        call_seq: i64,
    ) -> Result<Option<RowHistoryRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {HISTORY_COLUMNS} FROM row_history WHERE step_run_id = ?1 AND call_seq = ?2"
                ),
                params![step_run_id, call_seq],
                history_record,
            )
            .optional()?)
    }

    /// The data-free record of the write at a key (AD-10 as amended): what
    /// a result is rebuilt from. The row's data is never read here; its
    /// column count is counted in SQL.
    pub fn row_receipt(
        &self,
        step_run_id: &str,
        call_seq: i64,
    ) -> Result<Option<RowReceipt>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT table_id, row_id, version, op, taint, step_run_id, call_seq, tool_call_id,
                        (SELECT count(*) FROM json_each(row_history.data))
                 FROM row_history WHERE step_run_id = ?1 AND call_seq = ?2",
                params![step_run_id, call_seq],
                |r| {
                    Ok(RowReceipt {
                        table_id: r.get(0)?,
                        row_id: r.get(1)?,
                        version: r.get(2)?,
                        op: r.get(3)?,
                        taint: taint_of(r.get(4)?),
                        step_run_id: r.get(5)?,
                        call_seq: r.get(6)?,
                        tool_call_id: r.get(7)?,
                        columns: r.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    /// The row's current after-image (story 3.22): the newest image of
    /// `row_id` in `table_id`, which the row holds now; `None` when the
    /// table holds no such row (an unknown id, or a row of another table).
    /// The one content reader am-engine's `row_inputs` module names.
    pub fn row_snapshot(
        &self,
        table_id: &str,
        row_id: &str,
    ) -> Result<Option<RowSnapshot>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT h.seq, h.version, h.data, h.taint
                 FROM table_rows t
                 JOIN row_history h
                   ON h.table_id = t.table_id AND h.row_id = t.row_id AND h.version = t.version
                 WHERE t.table_id = ?1 AND t.row_id = ?2",
                params![table_id, row_id],
                |r| {
                    Ok(RowSnapshot {
                        seq: r.get(0)?,
                        version: r.get(1)?,
                        data: r.get(2)?,
                        taint: taint_of(r.get(3)?),
                    })
                },
            )
            .optional()?)
    }

    /// What the attempt may write, as journaled when it started; `None` for
    /// an attempt that journaled none (one from before migration 0010, a
    /// gate, or one started without its writes).
    pub fn attempt_writes(&self, step_run_id: &str) -> Result<Option<AttemptWrites>, StoreError> {
        let (seat, table) = journaled_writes(&self.conn, step_run_id)?;
        Ok(seat.map(|seat| AttemptWrites {
            seat,
            table,
            document: None,
        }))
    }
}
