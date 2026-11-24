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

use std::fmt;

use uuid::Uuid;

use crate::Taint;

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
