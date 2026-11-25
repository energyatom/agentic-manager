//! Document versions and their Draft items (epic 3 story 21; AD-10, AD-28,
//! AD-29, AD-35, AD-43; owner Q5 to Q8).
//!
//! - [`crate::WriteCommand::SettleAndFinishDocumentStep`] is a document
//!   step's success, in one transaction: the call settles, the attempt
//!   finishes `succeeded` with its Markdown as its output (kept once, there:
//!   AD-43), the version is numbered (the document's newest + 1, one per
//!   succeeded attempt: the owner's approved deviation from "one version per
//!   run", Q7) and stored, and one Draft item is raised on the attempt (its
//!   payload the attempt, AD-28).
//! - The writer refuses, with nothing written: an attempt that is not
//!   `running`, one whose journaled document is not the command's (or that
//!   journaled none), Markdown that [`markdown_refusal`] refuses, an
//!   expected version that is not the document's newest + 1
//!   ([`crate::StoreError::StaleVersion`]), and an item that is not a Draft
//!   on the attempt. Only an `Actor::System` sends it (AD-28: only the app
//!   itself raises an Inbox item).
//! - Review round 2: a document attempt never ends `succeeded` or
//!   `looped_back` any other way (it may fail or be cancelled): `FinishStep`,
//!   `SettleAndFinishStep`, a loop-back (`SettleAndLoopBack`, a gate's
//!   return) and, as a backstop, `ApplyResolution`'s approve all refuse
//!   it; and since round 3 it never waits on a Review (`PauseStep` refuses
//!   one), whose approve would finish it. A Draft is raised only on an
//!   attempt a version names; and once a version names an attempt, its
//!   output and status never change (0012's trigger).
//! - A version carries the attempt's taint at its finish, and its Draft at
//!   least that (AD-10: marked at rest; it never lowers).
//! - Audit (AD-25): the step's finish keeps 3.5's `output_sha256` through
//!   the existing entry; `document.versioned` holds ids, the version, the
//!   taint and the byte count, never the Markdown and no hash of it.
//!
//! A Draft is dismissed only by a person, through
//! [`crate::Writer::resolve_inbox_item`] (story 3.4's Human-only path).

use am_domain::{InboxKind, StepStatus, Taint};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::json;

use crate::{
    Entry, NewInboxItem, Reader, StepState, StoreError, call_of_step, create_item, finish_step,
    flag, require, settle, settled_and_finished, step_of, taint_of,
};

/// The most bytes a document version's Markdown may hold: 1 MiB.
pub const DOCUMENT_MAX_BYTES: usize = 1024 * 1024;

/// Why `markdown` cannot be a document version, in words naming the rule
/// and never quoting the text; `None` when it can: it is not empty after
/// trimming, holds no NUL and is at most [`DOCUMENT_MAX_BYTES`] bytes. The
/// engine's check and the writer's are this one.
pub fn markdown_refusal(markdown: &str) -> Option<String> {
    if markdown.trim().is_empty() {
        return Some("its final message is empty or only whitespace, so it is no document".into());
    }
    if markdown.contains('\0') {
        return Some(
            "its final message holds a NUL character, which a document never holds".into(),
        );
    }
    if markdown.len() > DOCUMENT_MAX_BYTES {
        return Some(format!(
            "its final message is {} bytes, past the {DOCUMENT_MAX_BYTES} bytes (1 MiB) a document version may hold",
            markdown.len()
        ));
    }
    None
}

/// One version of a document (AD-29, AD-35): the producing attempt names
/// its Markdown, which is never copied here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentVersionRecord {
    pub id: String,
    pub document_id: String,
    /// 1, 2, 3, … per document, in commit order.
    pub version: i64,
    pub run_id: String,
    pub attempt: i64,
    /// The producing attempt, whose output is the Markdown.
    pub step_run_id: String,
    /// The attempt's taint at its finish (AD-10 (c)).
    pub taint: Taint,
    /// The Markdown's size in bytes.
    pub bytes: i64,
    pub created_at: String,
}

/// What [`settle_and_finish`] takes: the command's fields.
pub(crate) struct DocumentFinish<'a> {
    pub call_id: &'a str,
    pub usage: Option<am_domain::Usage>,
    pub step_run_id: &'a str,
    pub output: &'a str,
    pub version_id: &'a str,
    pub document_id: &'a str,
    pub expected_version: i64,
    pub item: &'a NewInboxItem,
}

/// The document an attempt journaled when it started (`None`: none).
pub(crate) fn journaled_document(
    conn: &Connection,
    step_run_id: &str,
) -> Result<Option<String>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT writes_document FROM step_runs WHERE id = ?1",
            params![step_run_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

/// Review round 2 (AD-29): a document attempt ends `succeeded` only through
/// [`crate::WriteCommand::SettleAndFinishDocumentStep`], with its version,
/// and never `looped_back`; any other finish to `status` of the attempt
/// `step` that journaled a document is refused (`failed` and `cancelled`
/// stay allowed).
pub(crate) fn refuse_other_success(
    tx: &Transaction,
    step: &StepState,
    status: &str,
) -> Result<(), StoreError> {
    if !matches!(status, "succeeded" | "looped_back") {
        return Ok(());
    }
    let Some(document) = journaled_document(tx, &step.id)? else {
        return Ok(());
    };
    Err(StoreError::Invalid(format!(
        "refused: step run {} ({} attempt {}) versions the document `{document}`, so it succeeds only with its version and never ends `{status}` otherwise (it may fail); nothing was changed",
        step.id, step.step_key, step.attempt
    )))
}

/// Review round 3 (G1; AD-8, AD-28, AD-29): an attempt that journaled a
/// document never waits on a Review item, since a person's approve of it
/// would finish the attempt `succeeded` with no version. Any other kind
/// (a Question) is left to its own rules.
pub(crate) fn refuse_review_on_document(
    tx: &Transaction,
    step: &StepState,
    kind: InboxKind,
) -> Result<(), StoreError> {
    if kind != InboxKind::Review {
        return Ok(());
    }
    let Some(document) = journaled_document(tx, &step.id)? else {
        return Ok(());
    };
    Err(StoreError::Invalid(format!(
        "refused: step run {} ({} attempt {}) versions the document `{document}`, and a document step never waits on a Review (an approve would finish it with no version); nothing was changed",
        step.id, step.step_key, step.attempt
    )))
}

/// Review round 2 (AD-28, AD-29): a Draft item is raised only on an attempt
/// a version names, in the transaction that stored the version.
pub(crate) fn refuse_draft_without_version(
    tx: &Transaction,
    step: &StepState,
) -> Result<(), StoreError> {
    let versioned: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM document_versions WHERE step_run_id = ?1)",
        params![step.id],
        |r| r.get(0),
    )?;
    if versioned {
        return Ok(());
    }
    Err(StoreError::Invalid(format!(
        "refused: a Draft item is raised only with the document version its attempt wrote, and no version names step run {} ({} attempt {}); nothing was changed",
        step.id, step.step_key, step.attempt
    )))
}

/// [`crate::WriteCommand::SettleAndFinishDocumentStep`] in the caller's
/// transaction (the actor was checked: only the app's engine raises an
/// Inbox item); its audit entries.
pub(crate) fn settle_and_finish(
    tx: &Transaction,
    f: &DocumentFinish,
    now: &str,
) -> Result<Vec<Entry>, StoreError> {
    let refuse = |why: String| {
        Err(StoreError::Invalid(format!(
            "refused: {why}; nothing was changed"
        )))
    };
    let step = step_of(tx, f.step_run_id)?;
    require(&step, &[StepStatus::Running], "finishes")?;
    match journaled_document(tx, f.step_run_id)? {
        Some(declared) if declared == f.document_id => {}
        Some(declared) => {
            return refuse(format!(
                "step run {} ({} attempt {}) versions the document `{declared}`, journaled when it started, not `{}`",
                step.id, step.step_key, step.attempt, f.document_id
            ));
        }
        None => {
            return refuse(format!(
                "step run {} ({} attempt {}) journaled no document when it started, so it versions none",
                step.id, step.step_key, step.attempt
            ));
        }
    }
    if let Some(why) = markdown_refusal(f.output) {
        return refuse(format!("step run {}: {why}", step.id));
    }
    // Review round 2: the number the Draft's words name is the one this
    // transaction gives (the document's newest + 1).
    let next: i64 = tx.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM document_versions WHERE document_id = ?1",
        params![f.document_id],
        |r| r.get(0),
    )?;
    if f.expected_version != next {
        return Err(StoreError::StaleVersion {
            document_id: f.document_id.to_string(),
            expected: f.expected_version,
            next,
        });
    }
    if f.item.kind != InboxKind::Draft || f.item.payload.as_deref() != Some(f.step_run_id) {
        return refuse(format!(
            "a document version raises one Draft item whose payload is its own attempt, step run {}",
            step.id
        ));
    }
    call_of_step(tx, f.call_id, f.step_run_id)?;
    let settled = settle(tx, f.call_id, f.usage, now)?;
    let output = Some(f.output.to_string());
    finish_step(tx, f.step_run_id, "succeeded", &output, now)?;
    // The attempt's taint at its finish, as committed (AD-10 (c)).
    let finished = step_of(tx, f.step_run_id)?;
    let version = next;
    let bytes = f.output.len() as i64;
    tx.execute(
        "INSERT INTO document_versions (id, document_id, version, run_id, attempt, step_run_id, taint, bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            f.version_id,
            f.document_id,
            version,
            finished.run_id,
            finished.attempt,
            f.step_run_id,
            flag(finished.taint),
            bytes,
            now
        ],
    )?;
    // The Draft carries at least the version's taint (`create_item` raises
    // it to the attempt's, which is the version's).
    let item = NewInboxItem {
        taint: f.item.taint.raise(finished.taint),
        ..f.item.clone()
    };
    let mut entries: Vec<Entry> = vec![settled_and_finished(
        f.call_id,
        &settled,
        f.step_run_id,
        "succeeded",
        &output,
    )];
    entries.push((
        "document.versioned".into(),
        format!("document:{}", f.document_id),
        Some(json!({
            "attempt": finished.attempt,
            "bytes": bytes,
            "document_id": f.document_id,
            "run_id": finished.run_id,
            "step_run_id": f.step_run_id,
            "taint": flag(finished.taint),
            "version": version,
            "version_id": f.version_id,
        })),
    ));
    entries.push(create_item(tx, &finished, &item, now)?);
    Ok(entries)
}

const VERSION_COLUMNS: &str =
    "id, document_id, version, run_id, attempt, step_run_id, taint, bytes, created_at";

fn version_record(r: &rusqlite::Row) -> rusqlite::Result<DocumentVersionRecord> {
    Ok(DocumentVersionRecord {
        id: r.get(0)?,
        document_id: r.get(1)?,
        version: r.get(2)?,
        run_id: r.get(3)?,
        attempt: r.get(4)?,
        step_run_id: r.get(5)?,
        taint: taint_of(r.get(6)?),
        bytes: r.get(7)?,
        created_at: r.get(8)?,
    })
}

impl Reader {
    /// Every version of `document_id`, in version order (story 3.21); empty
    /// for a document with none. Holds no Markdown: a version's text is its
    /// producing attempt's output.
    pub fn document_versions(
        &self,
        document_id: &str,
    ) -> Result<Vec<DocumentVersionRecord>, StoreError> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {VERSION_COLUMNS} FROM document_versions WHERE document_id = ?1 ORDER BY version"
        ))?;
        let rows = st.query_map(params![document_id], version_record)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The newest version number of `document_id` (`MAX(version)`), `None`
    /// for a document with none (review round 2): the engine words a Draft
    /// with this + 1 and sends that number as the version it expects.
    pub fn latest_document_version(&self, document_id: &str) -> Result<Option<i64>, StoreError> {
        Ok(self.conn.query_row(
            "SELECT MAX(version) FROM document_versions WHERE document_id = ?1",
            params![document_id],
            |r| r.get(0),
        )?)
    }

    /// The version the attempt `step_run_id` wrote, if any (story 3.21):
    /// how `agentic gate` names a Draft's document and version.
    pub fn document_version_of(
        &self,
        step_run_id: &str,
    ) -> Result<Option<DocumentVersionRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {VERSION_COLUMNS} FROM document_versions WHERE step_run_id = ?1"),
                params![step_run_id],
                version_record,
            )
            .optional()?)
    }
}
