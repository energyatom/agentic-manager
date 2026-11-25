//! The engine's implementation of the row-writer port (epic 3 story 6, the
//! ToolGate slice; AD-8, AD-21 as amended 2026-11-24, owner Q1):
//! [`StoreRowWriter`], over the store's row command. It is the one shipped
//! file outside am-store that names the command, and ToolGate is its only
//! caller (`cargo xtask check-deps`).
//!
//! A write commits through the store's single writer; the answer is read
//! back from the store's data-free receipt at the write's key (its step
//! run and call sequence), never from what was asked: the row id and the
//! version the journal holds. A key that held this write already writes
//! nothing (the store's replay, only for the same write again) and answers
//! the first write with its own status word, so the result equals the one
//! a resume rebuilds (AD-8 as amended; review round 1, R4). The store's
//! refusals a model may act on (a stale version, a row the table does not
//! hold, data with a repeated key or past a cap) are
//! [`RowWriteError::Refused`] in the store's words, which name no value of
//! the write; every other failure is [`RowWriteError::Failed`].

use std::sync::Mutex;

use am_domain::Actor;
use am_domain::rows::{RowStatus, RowWrite, RowWriteError, RowWriter, RowWritten};
use am_store::{Reader, RowReceipt, Store, StoreError, Writer};

/// The row-writer port over a project's store.
pub struct StoreRowWriter {
    writer: Writer,
    /// A read-only connection for the receipts, or why it did not open (then
    /// every write fails in those words, before anything is sent).
    reader: Result<Mutex<Reader>, String>,
}

impl StoreRowWriter {
    pub fn new(store: &Store) -> StoreRowWriter {
        StoreRowWriter {
            writer: store.writer(),
            reader: store.reader().map(Mutex::new).map_err(|e| {
                format!("the journal could not be read for a row write's receipt: {e}")
            }),
        }
    }

    /// The data-free receipt of the write at (`step_run_id`, `call_seq`).
    fn receipt(
        &self,
        step_run_id: &str,
        call_seq: i64,
    ) -> Result<Option<RowReceipt>, RowWriteError> {
        let reader = self
            .reader
            .as_ref()
            .map_err(|e| RowWriteError::Failed(e.clone()))?;
        let reader = reader.lock().unwrap_or_else(|p| p.into_inner());
        reader
            .row_receipt(step_run_id, call_seq)
            .map_err(|e| RowWriteError::Failed(e.to_string()))
    }
}

impl RowWriter for StoreRowWriter {
    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {
        let (step_run_id, call_seq) = (write.step_run_id.clone(), write.call_seq);
        // A key that holds this write already is a replay: the store writes
        // nothing, and the receipt answers with the first write (review
        // round 1, R4), as a rebuild does.
        self.writer
            .apply_row_write(actor, write)
            .map_err(|e| match e {
                StoreError::RowRefused(words) => RowWriteError::Refused(words),
                other => RowWriteError::Failed(other.to_string()),
            })?;
        let receipt = self.receipt(&step_run_id, call_seq)?.ok_or_else(|| {
            RowWriteError::Failed(format!(
                "the row write of call {call_seq} of step run {step_run_id} committed, and the journal holds no receipt of it"
            ))
        })?;
        let row_id = uuid::Uuid::parse_str(&receipt.row_id).map_err(|e| {
            RowWriteError::Failed(format!(
                "the receipt of call {call_seq} of step run {step_run_id} names a row id that is not a UUID: {e}"
            ))
        })?;
        let status = match receipt.op.as_str() {
            "insert" => RowStatus::Inserted,
            "update" => RowStatus::Updated,
            op => {
                return Err(RowWriteError::Failed(format!(
                    "the receipt of call {call_seq} of step run {step_run_id} records the op `{op}`, which no row write makes"
                )));
            }
        };
        Ok(RowWritten {
            row_id,
            version: receipt.version,
            status,
        })
    }
}
