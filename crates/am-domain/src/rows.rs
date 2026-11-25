//! A seat's write of one table row (AD-21; epic 3 story 6, the store
//! slice): what the store's row command takes. Owned types only (AD-5):
//! no store, SQLite or JSON-library type crosses here, and the row's data
//! crosses as JSON text.
//!
//! The row id is a UUIDv7 the engine mints (never the model's text); the
//! step run, the call sequence and the tool call id name the journaled
//! `table_write` call the write answers; `taint` is the write's own, which
//! the store raises to the attempt's committed taint and the row's (AD-10:
//! a row's taint only rises). The port that carries a `RowWrite` from
//! ToolGate to the store comes with the ToolGate slice.
//!
//! Neither type prints the row's data or the table id (story 3.6, reviews
//! R11 and T6): `Debug` shows their byte lengths only, never a column
//! name, a value or the table's name.
//!
//! Story 3.22 adds [`RowInput`], a row a step reads at its start (ids
//! only).
//!
//! The types a write, its binding and its port take are here:
//!
//! ```
//! use am_domain::rows::{
//!     ColumnKind, ColumnSpec, DeclaredTable, PreparedRow, RowStatus, RowTool, RowWriteError,
//!     RowWriter, RowWritten,
//! };
//! let _ = (ColumnKind::Text, RowStatus::Inserted);
//! fn _bound(_: &dyn RowWriter, _: &dyn RowTool, _: DeclaredTable, _: ColumnSpec) {}
//! fn _results(_: PreparedRow, _: RowWritten, _: RowWriteError) {}
//! ```

use std::fmt;

use uuid::Uuid;

use crate::{Actor, Taint, ToolSpec};

/// What one write does to its row.
#[derive(Clone, PartialEq, Eq)]
pub enum RowOp {
    /// A new row: `data_json` is a JSON object, one key per column.
    Insert { row_id: Uuid, data_json: String },
    /// A change of the row at `expected_version`, the version the writer
    /// read: `patch_json` is a JSON object whose keys replace the row's
    /// columns of the same name (`null` clears one); the other columns are
    /// kept. A row at another version is refused (owner Q2).
    Update {
        row_id: Uuid,
        expected_version: i64,
        patch_json: String,
    },
}

/// One row write, keyed by its step run and call sequence (AD-8: a second
/// write at a key writes nothing).
#[derive(Clone, PartialEq, Eq)]
pub struct RowWrite {
    pub tool_call_id: String,
    pub step_run_id: String,
    pub call_seq: i64,
    pub table_id: String,
    pub op: RowOp,
    pub taint: Taint,
}

impl fmt::Debug for RowOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowOp::Insert { row_id, data_json } => f
                .debug_struct("Insert")
                .field("row_id", row_id)
                .field("data_json_bytes", &data_json.len())
                .finish(),
            RowOp::Update {
                row_id,
                expected_version,
                patch_json,
            } => f
                .debug_struct("Update")
                .field("row_id", row_id)
                .field("expected_version", expected_version)
                .field("patch_json_bytes", &patch_json.len())
                .finish(),
        }
    }
}

impl fmt::Debug for RowWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowWrite")
            .field("tool_call_id", &self.tool_call_id)
            .field("step_run_id", &self.step_run_id)
            .field("call_seq", &self.call_seq)
            .field("table_id_bytes", &self.table_id.len())
            .field("op", &self.op)
            .field("taint", &self.taint)
            .finish()
    }
}

/// A column's type, as a table file declares it (AD-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Text,
    Number,
    Integer,
    Boolean,
    Date,
    Datetime,
    Url,
    Status,
    Json,
}

impl ColumnKind {
    /// The type as written in a table file: `text`, `number`, `integer`,
    /// `boolean`, `date`, `datetime`, `url`, `status` or `json`.
    pub fn as_str(self) -> &'static str {
        match self {
            ColumnKind::Text => "text",
            ColumnKind::Number => "number",
            ColumnKind::Integer => "integer",
            ColumnKind::Boolean => "boolean",
            ColumnKind::Date => "date",
            ColumnKind::Datetime => "datetime",
            ColumnKind::Url => "url",
            ColumnKind::Status => "status",
            ColumnKind::Json => "json",
        }
    }

    /// Parses the table file's spelling.
    pub fn parse(s: &str) -> Option<ColumnKind> {
        [
            ColumnKind::Text,
            ColumnKind::Number,
            ColumnKind::Integer,
            ColumnKind::Boolean,
            ColumnKind::Date,
            ColumnKind::Datetime,
            ColumnKind::Url,
            ColumnKind::Status,
            ColumnKind::Json,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// One declared column: its config id, its type, whether a row must hold
/// it, and a status column's allowed values (empty for any other type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    pub id: String,
    pub kind: ColumnKind,
    pub required: bool,
    pub values: Vec<String>,
}

/// The one table a step declares (`table:`), as the config snapshot holds
/// it: its id, its one owning team and its columns. Config ids and types
/// only; never a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredTable {
    pub id: String,
    pub team: String,
    pub columns: Vec<ColumnSpec>,
}

/// What a committed write did to its row.
///
/// The ToolGate slice's review round 2 (T2, manager 2026-11-25): a write
/// is an insert or an update, and nothing else. A replay answers with the
/// first write's own status, so a result always equals its rebuild
/// (`toolgate::engine_result_of`); there is no "already written" status
/// a writer could return instead (tests first: written before the variant
/// is removed).
///
/// ```
/// use am_domain::rows::RowStatus;
/// fn word(status: RowStatus) -> &'static str {
///     match status {
///         RowStatus::Inserted => "inserted",
///         RowStatus::Updated => "updated",
///     }
/// }
/// assert_eq!(word(RowStatus::Inserted), "inserted");
/// ```
///
/// ```compile_fail
/// let _ = am_domain::rows::RowStatus::AlreadyWritten;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    Inserted,
    Updated,
}

/// The committed write a [`RowWriter`] answers with, read back from the
/// store's data-free receipt: engine values only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowWritten {
    pub row_id: Uuid,
    pub version: i64,
    pub status: RowStatus,
}

/// Why a row write did not commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowWriteError {
    /// The row's state refuses it (a stale version, a row the table does
    /// not hold), in words the model may read: an error result for the
    /// model, untrusted; the step goes on.
    Refused(String),
    /// The store failed or refused the write for a reason the model cannot
    /// act on: the step stops.
    Failed(String),
}

impl fmt::Display for RowWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowWriteError::Refused(words) | RowWriteError::Failed(words) => f.write_str(words),
        }
    }
}

/// A row a step reads at its start (story 3.22, AD-10, AD-43): the table
/// by its config id and the row by its id. A plan names it in the step's
/// row-input slot; the engine reads the row's current after-image and
/// hands it to the step as fenced untrusted data, which taints the step.
/// Ids only, never the row's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowInput {
    pub table_id: String,
    pub row_id: Uuid,
}

/// The row-writer port (AD-21 as amended 2026-11-24, owner Q1): a seat's
/// write of one row, as `actor` (the attempt's own `Actor::Seat`). am-engine
/// implements it over the store's row command; ToolGate is its only caller
/// (`cargo xtask check-deps`). Synchronous: the store's writer commits it
/// before it returns.
pub trait RowWriter: Send + Sync {
    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError>;
}

/// A call's arguments, checked against the declared table: an insert's
/// data (its row id is the engine's to mint) or an update's patch at the
/// version the model read. Data crosses as JSON text.
#[derive(Clone, PartialEq, Eq)]
pub enum PreparedRow {
    Insert {
        data_json: String,
    },
    Update {
        row_id: Uuid,
        expected_version: i64,
        patch_json: String,
    },
}

impl fmt::Debug for PreparedRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreparedRow::Insert { data_json } => f
                .debug_struct("Insert")
                .field("data_json_bytes", &data_json.len())
                .finish(),
            PreparedRow::Update {
                row_id,
                expected_version,
                patch_json,
            } => f
                .debug_struct("Update")
                .field("row_id", row_id)
                .field("expected_version", expected_version)
                .field("patch_json_bytes", &patch_json.len())
                .finish(),
        }
    }
}

/// The built-in `table_write` (owner Q1): its spec for a declared table,
/// and the checks that turn a call's arguments into a [`PreparedRow`]. It
/// writes nothing: ToolGate writes through the [`RowWriter`] port. Refusals
/// are words built from the table's config ids and types, never from the
/// model's keys or values.
pub trait RowTool: Send + Sync {
    fn spec(&self, table: &DeclaredTable) -> ToolSpec;
    fn prepare(&self, arguments_json: &str, table: &DeclaredTable) -> Result<PreparedRow, String>;
}
