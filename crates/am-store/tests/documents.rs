//! Story 3.6b, "Document versions and Draft items", at the store (AD-15,
//! AD-28, AD-29, AD-35, AD-43; owner Q5 to Q8): a document step's finish
//! stores its Markdown as a version and raises one Draft item, in one
//! transaction.
//!
//! - `WriteCommand::SettleAndFinishDocumentStep` settles the call, finishes
//!   the step with its Markdown (kept once, on the attempt's output),
//!   numbers the version per document (one per succeeded attempt, the
//!   owner's approved deviation, Q7) and raises one Draft item, in one
//!   transaction; a finish the writer refuses leaves nothing.
//! - `document_versions` is append-only; a version and its Draft carry the
//!   attempt's taint.
//! - A Draft is dismissed only by a person, through 3.4's
//!   `Writer::resolve_inbox_item` (owner Q8).
//! - Audit entries hold ids, versions and counts, never Markdown.
//! - Migration 0012 adds `document_versions` and `step_runs.writes_document`
//!   behind AD-7's pre-migration copy, NULL on every old attempt; a failing
//!   0012 leaves version 11.
//!
//! Moved here from 3.6's `rows.rs` in the owner's re-slices (2026-11-24);
//! the migration 0012 tests joined it in 3.6b's own tests-first step
//! (2026-11-25). The pinned API is in 3.6b's plan, Implementation Notes.
//! Written before document versions exist.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use am_domain::{Actor, Decision, HumanActor, InboxKind, REVIEWER_HAT, RuntimeLayout, Taint};
use am_store::audit::sha256_hex;
use am_store::{
    AttemptWrites, CallPurpose, CallRow, CallSettlement, DocumentVersionRecord, InboxItemRow,
    NewInboxItem, NewPromptPart, OpenReport, Reader, RowHistoryRecord, RowRecord, SCHEMA_VERSION,
    StepInput, StepRow, Store, StoreError, ToolCallRow, WriteCommand, new_id,
};
use rusqlite::types::Value as Cell;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use uuid::Uuid;

const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

/// The table every writing attempt declares, and another one.
const TABLE: &str = "entries";
const OTHER_TABLE: &str = "others";
/// The seat every attempt runs on.
const SEAT: &str = "seat-writer";
/// The document a document step declares, and another one.
const DOCUMENT: &str = "summary-note";
const OTHER_DOCUMENT: &str = "other-note";

/// Canaries no audit entry may hold (AD-25).
const CANARY_MARKDOWN: &str = "canary-markdown-text-c30a71";

/// A return note.
const NOTE: &str = "make it shorter";

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// The owner wearing the Reviewer hat: the one resolver (AD-15, AD-24).
fn reviewer() -> HumanActor {
    HumanActor::new("owner", REVIEWER_HAT)
}

fn you_reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: REVIEWER_HAT.into(),
    }
}

/// A clean flag as an audit argument: `0` or `false`.
fn audited_clean(arg: &Value) -> bool {
    arg == &Value::from(0) || arg == &Value::Bool(false)
}

/// A store timestamp: UTC RFC 3339 with milliseconds and `Z`.
fn assert_timestamp(text: &str) {
    assert_eq!(text.len(), "2026-11-24T10:00:00.000Z".len(), "{text}");
    assert!(text.ends_with('Z') && text.as_bytes()[19] == b'.', "{text}");
    text.parse::<jiff::Timestamp>()
        .unwrap_or_else(|e| panic!("{text} is not a timestamp: {e}"));
}

fn writes(table: Option<&str>, document: Option<&str>) -> AttemptWrites {
    AttemptWrites {
        seat: SEAT.into(),
        table: table.map(str::to_string),
        document: document.map(str::to_string),
    }
}

// ------------------------------------------------------------------ the rig

/// A run's id, as text and as the UUID an `Actor::Seat` names.
#[derive(Debug, Clone)]
struct Run {
    id: String,
    uuid: Uuid,
}

/// One started step attempt.
#[derive(Debug, Clone)]
struct Attempt {
    run: Run,
    key: String,
    /// The step run's id.
    id: String,
}

impl Attempt {
    /// The seat acting in this attempt, as ToolGate names it.
    fn actor(&self) -> Actor {
        Actor::Seat {
            seat: SEAT.into(),
            run: self.run.uuid,
            step: self.key.clone(),
        }
    }
}

/// What a refused command must leave as it was.
#[derive(Debug, PartialEq)]
struct State {
    rows: Vec<RowRecord>,
    history: Vec<RowHistoryRecord>,
    /// Every row of `row_history`, also one whose row does not exist.
    history_rows: i64,
    versions: Vec<DocumentVersionRecord>,
    version_rows: i64,
    steps: Vec<StepRow>,
    calls: Vec<CallRow>,
    tool_calls: Vec<ToolCallRow>,
    items: Vec<InboxItemRow>,
    audit: usize,
}

/// A store on a bare path, with one run of `steps`.
struct Rig {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
    run: Run,
}

fn rig_with(steps: &[&str]) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    let mut r = Rig {
        _dir: dir,
        db,
        store,
        run: Run {
            id: String::new(),
            uuid: Uuid::nil(),
        },
    };
    r.run = r.new_run(steps);
    r
}

impl Rig {
    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    fn exec(&self, cmd: WriteCommand) -> Result<(), StoreError> {
        self.store.writer().exec(engine(), cmd)
    }

    /// Another run of `steps` on the same store.
    fn new_run(&self, steps: &[&str]) -> Run {
        let uuid = Uuid::now_v7();
        let id = uuid.to_string();
        self.exec(WriteCommand::CreateRun {
            run_id: id.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            config_hash: Some(HASH.into()),
        })
        .unwrap();
        Run { id, uuid }
    }

    /// The latest attempt of step `key` of `run`.
    fn latest(&self, run: &Run, key: &str) -> StepRow {
        self.reader()
            .steps(&run.id)
            .unwrap()
            .into_iter()
            .filter(|s| s.step_key == key)
            .max_by_key(|s| s.attempt)
            .unwrap_or_else(|| panic!("no step {key}"))
    }

    /// One step attempt by id.
    fn step(&self, id: &str) -> StepRow {
        let reader = self.reader();
        reader
            .runs()
            .unwrap()
            .into_iter()
            .flat_map(|run| reader.steps(&run.id).unwrap())
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("no step run {id}"))
    }

    /// Starts the latest (queued) attempt of `key` with `inputs` and
    /// `prompt`, journaling its writes.
    fn start_with(
        &self,
        run: &Run,
        key: &str,
        writes: AttemptWrites,
        taint: Taint,
        inputs: Vec<StepInput>,
        prompt: Vec<NewPromptPart>,
    ) -> Result<Attempt, StoreError> {
        let row = self.latest(run, key);
        self.exec(WriteCommand::StartAttempt {
            step_run_id: row.id.clone(),
            inputs,
            taint,
            prompt,
            writes,
        })?;
        Ok(Attempt {
            run: run.clone(),
            key: key.into(),
            id: row.id,
        })
    }

    /// Starts the latest attempt of `key`, which may write `table` and
    /// version `document`.
    fn begin(
        &self,
        run: &Run,
        key: &str,
        table: Option<&str>,
        document: Option<&str>,
        taint: Taint,
    ) -> Attempt {
        self.start_with(run, key, writes(table, document), taint, vec![], vec![])
            .unwrap()
    }

    /// Reserves the attempt's next model call: its id.
    fn reserve(&self, a: &Attempt) -> String {
        let call = new_id();
        let seq = self.reader().calls_for_step(&a.id).unwrap().len() as i64 + 1;
        self.exec(WriteCommand::ReserveCall {
            step_run_id: a.id.clone(),
            call_id: call.clone(),
            call_seq: seq,
            price: am_domain::money::CallPrice::local(),
            rate: am_domain::money::Rate::none("EUR"),
            counted_input_tokens: 0,
            max_output_tokens: 0,
            week: "2026-11-23".into(),
            team: "core".into(),
            taint: Taint::Clean,
            purpose: CallPurpose::Turn,
            prompt: vec![],
        })
        .unwrap();
        call
    }

    /// Step `from` of `run` runs and loops back to `to`: attempt n+1 of
    /// every step from `to` through `from` is queued.
    fn loop_back(&self, run: &Run, from: &str, to: &str) {
        let a = self.begin(run, from, None, None, Taint::Clean);
        let call = self.reserve(&a);
        self.exec(WriteCommand::SettleAndLoopBack {
            call_id: call,
            usage: None,
            step_run_id: a.id,
            output: r#"{"reasons":["once more"]}"#.into(),
            to: to.into(),
        })
        .unwrap();
    }

    /// The number the next version of `document_id` gets: its newest + 1
    /// (review round 2, fix 3: `Reader::latest_document_version`).
    fn next_version(&self, document_id: &str) -> i64 {
        self.reader()
            .latest_document_version(document_id)
            .unwrap()
            .unwrap_or(0)
            + 1
    }

    /// The document attempt `a` finishes with `markdown` on its reserved
    /// call `call`, as `document_id`, raising the Draft item `item` (taint
    /// `item_taint`), expecting the document's next version: the version's
    /// id.
    fn document_with(
        &self,
        a: &Attempt,
        call: &str,
        document_id: &str,
        markdown: &str,
        item: &str,
        item_taint: Taint,
    ) -> Result<String, StoreError> {
        let expected = self.next_version(document_id);
        self.document_at(a, call, document_id, markdown, item, item_taint, expected)
    }

    /// As [`Rig::document_with`], expecting the version `expected_version`
    /// (review round 2, fix 3): the version's id.
    #[allow(clippy::too_many_arguments)]
    fn document_at(
        &self,
        a: &Attempt,
        call: &str,
        document_id: &str,
        markdown: &str,
        item: &str,
        item_taint: Taint,
        expected_version: i64,
    ) -> Result<String, StoreError> {
        let version = new_id();
        self.exec(WriteCommand::SettleAndFinishDocumentStep {
            call_id: call.into(),
            usage: None,
            step_run_id: a.id.clone(),
            output: markdown.into(),
            version_id: version.clone(),
            document_id: document_id.into(),
            expected_version,
            item: draft_item(item, a, document_id, item_taint),
        })?;
        Ok(version)
    }

    /// The document attempt `a` finishes with `markdown` as [`DOCUMENT`]:
    /// the version's and the Draft item's ids.
    fn document(&self, a: &Attempt, markdown: &str) -> (String, String) {
        let call = self.reserve(a);
        let item = new_id();
        let version = self
            .document_with(a, &call, DOCUMENT, markdown, &item, Taint::Clean)
            .unwrap();
        (version, item)
    }

    fn versions(&self, document: &str) -> Vec<DocumentVersionRecord> {
        self.reader().document_versions(document).unwrap()
    }

    fn item(&self, id: &str) -> InboxItemRow {
        self.reader()
            .inbox_item(id)
            .unwrap()
            .unwrap_or_else(|| panic!("no item {id}"))
    }

    fn audit(&self) -> Vec<Value> {
        let mut out = Vec::new();
        self.reader().export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn entries(&self, action: &str) -> Vec<Value> {
        self.audit()
            .into_iter()
            .filter(|e| e["action"] == action)
            .collect()
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.db).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        let c = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn state(&self) -> State {
        let reader = self.reader();
        let mut rows = reader.table_rows_of(TABLE).unwrap();
        rows.extend(reader.table_rows_of(OTHER_TABLE).unwrap());
        let history = rows
            .iter()
            .flat_map(|r| reader.row_history_of(&r.table_id, &r.row_id).unwrap())
            .collect();
        let mut versions = reader.document_versions(DOCUMENT).unwrap();
        versions.extend(reader.document_versions(OTHER_DOCUMENT).unwrap());
        let (mut steps, mut calls, mut tool_calls, mut items) = (vec![], vec![], vec![], vec![]);
        for run in reader.runs().unwrap() {
            for s in reader.steps(&run.id).unwrap() {
                tool_calls.extend(reader.tool_calls(&s.id).unwrap());
                steps.push(s);
            }
            calls.extend(reader.calls_for_run(&run.id).unwrap());
            items.extend(reader.inbox_items(&run.id).unwrap());
        }
        State {
            rows,
            history,
            history_rows: self.count("row_history"),
            versions,
            version_rows: self.count("document_versions"),
            steps,
            calls,
            tool_calls,
            items,
            audit: self.audit().len(),
        }
    }
}

/// The Draft item the engine raises for a version of `document` by `a`:
/// the engine's words, never the Markdown.
fn draft_item(id: &str, a: &Attempt, document: &str, taint: Taint) -> NewInboxItem {
    NewInboxItem {
        id: id.into(),
        kind: InboxKind::Draft,
        payload: Some(a.id.clone()),
        taint,
        title: format!("Draft: {document}"),
        body: format!(
            "Step `{}` wrote a version of {document}. Drafts only: Agentic Manager never sends it.",
            a.key
        ),
        max_returns: None,
    }
}

/// Markdown with a canary, a heading and a link.
fn markdown() -> String {
    format!(
        "# Summary\n\nThree entries were recorded. {CANARY_MARKDOWN}\n\n- [a link](https://example.invalid/page)\n"
    )
}

// ------------------------------------------------------------ documents

/// Row 23 (AD-29, AD-35, AD-43): a document attempt's finish settles its
/// call, ends the step `succeeded` with the Markdown as its output, stores
/// version 1 (the run, the attempt, the step run, the taint, the bytes) and
/// raises one open Draft item on it (payload its own attempt), audited
/// `document.versioned` and `inbox.created`. A tainted attempt's version and
/// Draft are tainted whatever the item says.
/// A finish the writer refuses (another document than the attempt's, an
/// attempt that declares none, a seat as the actor) leaves the call
/// unsettled, the step running, no version, no item, no audit entry.
#[test]
fn a_document_version_and_its_draft_commit_with_the_step() {
    let r = rig_with(&["summarise"]);
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(None, Some(DOCUMENT)))
    );
    let md = markdown();

    let (version, item) = r.document(&a, &md);

    let step = r.step(&a.id);
    assert_eq!(step.status, "succeeded");
    assert_eq!(
        step.output.as_deref(),
        Some(md.as_str()),
        "kept once, as is"
    );
    assert!(step.finished_at.is_some());
    assert!(r.reader().calls_for_step(&a.id).unwrap()[0].settled);
    let versions = r.versions(DOCUMENT);
    assert_eq!(
        versions,
        vec![DocumentVersionRecord {
            id: version,
            document_id: DOCUMENT.into(),
            version: 1,
            run_id: r.run.id.clone(),
            attempt: 1,
            step_run_id: a.id.clone(),
            taint: Taint::Clean,
            bytes: md.len() as i64,
            created_at: versions[0].created_at.clone(),
        }]
    );
    assert_timestamp(&versions[0].created_at);
    let draft = r.item(&item);
    assert_eq!(draft.kind, InboxKind::Draft);
    assert_eq!(draft.status, "open");
    assert_eq!(draft.payload.as_deref(), Some(a.id.as_str()));
    assert_eq!(draft.step_run_id, a.id);
    assert_eq!(draft.taint, Taint::Clean);
    assert_eq!(r.reader().inbox_items(&r.run.id).unwrap().len(), 1);
    assert_eq!(r.entries("document.versioned").len(), 1);
    assert_eq!(r.entries("inbox.created").len(), 1);

    // A tainted attempt: the version and its Draft carry the taint.
    let run = r.new_run(&["summarise"]);
    let t = r.begin(&run, "summarise", None, Some(DOCUMENT), Taint::Tainted);
    let call = r.reserve(&t);
    let t_item = new_id();
    r.document_with(&t, &call, DOCUMENT, &md, &t_item, Taint::Clean)
        .unwrap();
    assert_eq!(r.versions(DOCUMENT)[1].taint, Taint::Tainted);
    assert_eq!(r.item(&t_item).taint, Taint::Tainted);

    // Refused finishes leave nothing.
    let run = r.new_run(&["summarise"]);
    let declared = r.begin(&run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let declared_call = r.reserve(&declared);
    let other = r.new_run(&["summarise"]);
    let undeclared = r.begin(&other, "summarise", None, None, Taint::Clean);
    let undeclared_call = r.reserve(&undeclared);
    for (what, attempt, call, document) in [
        (
            "another document than the attempt's",
            &declared,
            &declared_call,
            OTHER_DOCUMENT,
        ),
        (
            "an attempt that declares none",
            &undeclared,
            &undeclared_call,
            DOCUMENT,
        ),
    ] {
        let before = r.state();
        let refused = r.document_with(attempt, call, document, &md, &new_id(), Taint::Clean);
        assert!(refused.is_err(), "{what}: refused");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&attempt.id).status, "running", "{what}");
        assert!(
            !r.reader().calls_for_step(&attempt.id).unwrap()[0].settled,
            "{what}: the call stays unsettled"
        );
    }
    let before = r.state();
    let by_seat = r.store.writer().exec(
        declared.actor(),
        WriteCommand::SettleAndFinishDocumentStep {
            call_id: declared_call.clone(),
            usage: None,
            step_run_id: declared.id.clone(),
            output: md.clone(),
            version_id: new_id(),
            document_id: DOCUMENT.into(),
            expected_version: r.next_version(DOCUMENT),
            item: draft_item(&new_id(), &declared, DOCUMENT, Taint::Clean),
        },
    );
    assert!(by_seat.is_err(), "only the engine raises a Draft");
    assert_eq!(r.state(), before);
}

/// Row 22, the document half (AD-25): `document.versioned` names the
/// document with its version, the step run, its byte count and its taint,
/// and carries no hash argument; the Markdown's canary and its hash are
/// nowhere in the export (the step's own finish keeps 3.5's
/// `output_sha256` of the stored output); the chain verifies.
#[test]
fn a_document_audit_holds_ids_never_markdown() {
    let r = rig_with(&["summarise"]);
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let md = markdown();

    r.document(&a, &md);

    let versioned = r.entries("document.versioned");
    assert_eq!(versioned.len(), 1, "{versioned:#?}");
    let e = &versioned[0];
    assert_eq!(e["args"]["version"], 1);
    assert_eq!(e["args"]["bytes"], md.len());
    assert!(audited_clean(&e["args"]["taint"]), "{e}");
    assert!(e.to_string().contains(DOCUMENT), "{e}");
    assert!(e.to_string().contains(&a.id), "names the step run: {e}");
    let args = e["args"].as_object().expect("args");
    assert!(
        args.keys().all(|k| !k.ends_with("_sha256")),
        "no hash argument on the version: {e}"
    );
    let export = serde_json::to_string(&r.audit()).unwrap();
    for secret in [CANARY_MARKDOWN.to_string(), sha256_hex(CANARY_MARKDOWN)] {
        assert!(!export.contains(&secret), "{secret} in {export}");
    }
    assert_eq!(r.reader().verify_audit_chain().unwrap(), None);
}

/// Rows 24 and 25 (owner Q7, the approved deviation): versions are
/// numbered per document in commit order, across runs and across attempts
/// of one run (a loop-back's attempt 2 adds a version; nothing is
/// replaced); each version has its own Draft; another document counts from
/// 1; an earlier version never changes.
#[test]
fn document_versions_number_per_document() {
    let r = rig_with(&["summarise", "check"]);
    let first = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let (_, item_1) = r.document(&first, &markdown());
    let v1 = r.versions(DOCUMENT);
    r.loop_back(&r.run, "check", "summarise");
    let second = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let (_, item_2) = r.document(&second, "# Summary\n\nShorter.\n");
    let run = r.new_run(&["summarise"]);
    let third = r.begin(&run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let (_, item_3) = r.document(&third, "# Summary\n\nThe next run.\n");

    let versions = r.versions(DOCUMENT);
    let shape: Vec<(i64, String, i64, String)> = versions
        .iter()
        .map(|v| {
            (
                v.version,
                v.run_id.clone(),
                v.attempt,
                v.step_run_id.clone(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            (1, r.run.id.clone(), 1, first.id.clone()),
            (2, r.run.id.clone(), 2, second.id.clone()),
            (3, run.id.clone(), 1, third.id.clone()),
        ]
    );
    assert_eq!(versions[0], v1[0], "version 1 is unchanged");
    for (item, attempt) in [(&item_1, &first), (&item_2, &second), (&item_3, &third)] {
        let row = r.item(item);
        assert_eq!(row.kind, InboxKind::Draft);
        assert_eq!(row.payload.as_deref(), Some(attempt.id.as_str()));
    }

    let elsewhere = r.new_run(&["note"]);
    let other = r.begin(&elsewhere, "note", None, Some(OTHER_DOCUMENT), Taint::Clean);
    let call = r.reserve(&other);
    r.document_with(
        &other,
        &call,
        OTHER_DOCUMENT,
        "# Other\n",
        &new_id(),
        Taint::Clean,
    )
    .unwrap();
    assert_eq!(
        r.versions(OTHER_DOCUMENT)
            .iter()
            .map(|v| v.version)
            .collect::<Vec<_>>(),
        vec![1],
        "another document counts on its own"
    );
    assert_eq!(r.versions(DOCUMENT).len(), 3);
}

/// Row 18: `document_versions` is append-only: a raw `UPDATE` or `DELETE`,
/// an `INSERT OR REPLACE` by its id, and a copy meeting its unique keys
/// (document and version, step run) are aborted.
#[test]
fn document_versions_are_append_only() {
    let r = rig_with(&["summarise"]);
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    r.document(&a, &markdown());
    let before = r.state();
    let c = r.raw();
    c.execute_batch(
        "CREATE TEMP TABLE version_copy AS SELECT * FROM document_versions;
         UPDATE version_copy SET id = 'a-forged-version-id';",
    )
    .unwrap();

    for sql in [
        "UPDATE document_versions SET taint = 1",
        "UPDATE document_versions SET version = 7",
        "UPDATE document_versions SET bytes = 0",
        "DELETE FROM document_versions",
        "INSERT OR REPLACE INTO document_versions SELECT * FROM document_versions",
        "INSERT OR REPLACE INTO document_versions SELECT * FROM temp.version_copy",
        "INSERT INTO document_versions SELECT * FROM temp.version_copy",
    ] {
        assert!(c.execute_batch(sql).is_err(), "aborted: {sql}");
    }
    drop(c);
    assert_eq!(r.state(), before);
}

/// Row 30 (owner Q8, AD-15, AD-28): a Draft takes one decision, dismiss,
/// and only a person makes it, through 3.4's `resolve_inbox_item`: approve,
/// return, reject, answer and a dismiss with a note are refused with
/// nothing changed; the dismiss records `dismiss` by You (Reviewer), with
/// one `inbox.resolved` entry; no step and not the run changes; a second
/// resolve is refused.
#[test]
fn a_draft_is_dismissed_only_by_a_person() {
    let r = rig_with(&["summarise"]);
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let (_, item) = r.document(&a, &markdown());
    let resolve = |decision: Decision, note: Option<&str>| {
        r.store
            .writer()
            .resolve_inbox_item(reviewer(), &item, decision, note.map(str::to_string))
    };
    let before = r.state();

    for (decision, note) in [
        (Decision::Approve, None),
        (Decision::Return, Some(NOTE)),
        (Decision::Reject, None),
        (Decision::Answer, Some(NOTE)),
        (Decision::Dismiss, Some(NOTE)),
    ] {
        assert!(
            resolve(decision, note).is_err(),
            "{decision:?} on a Draft is refused"
        );
        assert_eq!(r.state(), before, "{decision:?}: nothing changed");
    }

    resolve(Decision::Dismiss, None).unwrap();

    let row = r.item(&item);
    assert_eq!(row.status, "resolved");
    assert_eq!(row.decision, Some(Decision::Dismiss));
    assert_eq!(row.note, None);
    assert_eq!(row.resolver(), Some(you_reviewer()));
    assert_timestamp(row.resolved_at.as_deref().unwrap());
    let after = r.state();
    assert_eq!(after.steps, before.steps, "no step changes");
    assert_eq!(after.versions, before.versions, "the version stays");
    assert_eq!(
        r.reader().run(&r.run.id).unwrap().unwrap().status,
        "running",
        "the run is unaffected"
    );
    let resolved = r.entries("inbox.resolved");
    assert_eq!(resolved.len(), 1, "{resolved:#?}");
    assert_eq!(
        serde_json::from_value::<Actor>(resolved[0]["actor"].clone()).ok(),
        Some(you_reviewer())
    );
    assert!(resolve(Decision::Dismiss, None).is_err(), "resolved once");
}

/// Row 39: the writer's own refusal of a non-Human resolve of a Draft is a
/// unit test in `src/lib.rs` (only the crate can send a resolve as another
/// actor: `resolve_inbox_item` takes a `HumanActor`). The tester may change
/// no `src/` line but doc comments, so the implementer writes it, as 3.4's
/// writer-refusal test; this guard is red until it is there.
#[test]
fn the_writer_unit_test_for_a_non_human_draft_is_there() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("am-store's src/lib.rs");
    assert!(
        lib.contains("fn a_non_human_resolve_of_a_draft_is_refused_in_the_writer"),
        "the src unit test of row 39 is missing"
    );
    assert!(
        lib.contains("resolve_inbox_item(seat, \"i\", am_domain::Decision::Dismiss, None)"),
        "the compile_fail doctest of a seat dismissing a Draft is there"
    );
}

// --------------------------------------- review round 2 (tester, 2026-11-25)
//
// The manager's triage of round 1's two reviews (plan, "Review triage,
// round 1"): fixes 1, 2, 3 and 6 at the store. Written before the fixes.

/// A Failure item on the attempt `a`: the engine's item for a final message
/// that is no document.
fn failure_item(id: &str, a: &Attempt) -> NewInboxItem {
    NewInboxItem {
        id: id.into(),
        kind: InboxKind::Failure,
        payload: Some(a.id.clone()),
        taint: Taint::Clean,
        title: "A step stopped".into(),
        body: format!("Step `{}` wrote no version.", a.key),
        max_returns: None,
    }
}

/// fix 1 (S1; AD-29, AD-43): a version's text stays at rest. Once a version
/// names an attempt, a raw `UPDATE` of that attempt's `output` or `status`,
/// alone or together, is aborted; the Markdown, the status and every row
/// stay as they were.
#[test]
fn a_versioned_attempts_output_and_status_never_change() {
    let r = rig_with(&["summarise"]);
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let md = markdown();
    r.document(&a, &md);
    let before = r.state();
    let c = r.raw();

    for sql in [
        "UPDATE step_runs SET output = 'a forged summary' WHERE id = ?1",
        "UPDATE step_runs SET output = NULL WHERE id = ?1",
        "UPDATE step_runs SET status = 'failed' WHERE id = ?1",
        "UPDATE step_runs SET status = 'cancelled', output = 'gone' WHERE id = ?1",
    ] {
        assert!(
            c.execute(sql, rusqlite::params![a.id]).is_err(),
            "aborted: {sql}"
        );
    }

    drop(c);
    let step = r.step(&a.id);
    assert_eq!(step.status, "succeeded");
    assert_eq!(step.output.as_deref(), Some(md.as_str()), "the text stays");
    assert_eq!(r.state(), before);
}

/// fix 2 (S2, C1; AD-29): a document attempt succeeds only with its version.
/// `FinishStep` and `SettleAndFinishStep` finishing it `succeeded`, and
/// `SettleAndLoopBack` ending it `looped_back`, are each refused with
/// nothing written: the attempt stays `running`, its call unsettled; no
/// version, no item, no audit entry. It may still fail (the controls: a
/// `FinishStep` to `failed` commits, and so does a `FailWithItem` with a
/// Failure item, the engine's path for a message that is no document).
#[test]
fn a_document_attempt_succeeds_only_with_its_version() {
    let r = rig_with(&["first", "summarise"]);
    // `first` succeeds, so `summarise` has an earlier step to loop back to.
    let first = r.begin(&r.run, "first", None, None, Taint::Clean);
    let first_call = r.reserve(&first);
    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: first_call,
        usage: None,
        step_run_id: first.id.clone(),
        status: "succeeded".into(),
        output: Some("first done".into()),
    })
    .unwrap();
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let call = r.reserve(&a);
    let md = markdown();

    for (what, cmd) in [
        (
            "FinishStep succeeded",
            WriteCommand::FinishStep {
                step_run_id: a.id.clone(),
                status: "succeeded".into(),
                output: Some(md.clone()),
            },
        ),
        (
            "SettleAndFinishStep succeeded",
            WriteCommand::SettleAndFinishStep {
                call_id: call.clone(),
                usage: None,
                step_run_id: a.id.clone(),
                status: "succeeded".into(),
                output: Some(md.clone()),
            },
        ),
        (
            "SettleAndLoopBack",
            WriteCommand::SettleAndLoopBack {
                call_id: call.clone(),
                usage: None,
                step_run_id: a.id.clone(),
                output: r#"{"reasons":["once more"]}"#.into(),
                to: "first".into(),
            },
        ),
    ] {
        let before = r.state();
        assert!(r.exec(cmd).is_err(), "{what}: refused");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&a.id).status, "running", "{what}");
        assert!(
            !r.reader().calls_for_step(&a.id).unwrap()[0].settled,
            "{what}: the call stays unsettled"
        );
    }
    assert!(r.versions(DOCUMENT).is_empty());

    // The controls: a document attempt still fails, with or without its
    // Failure item.
    r.exec(WriteCommand::FinishStep {
        step_run_id: a.id.clone(),
        status: "failed".into(),
        output: None,
    })
    .unwrap();
    assert_eq!(r.step(&a.id).status, "failed");
    let run = r.new_run(&["summarise"]);
    let b = r.begin(&run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let b_call = r.reserve(&b);
    let failure = new_id();
    r.exec(WriteCommand::FailWithItem {
        step_run_id: b.id.clone(),
        settle: Some(CallSettlement {
            call_id: b_call,
            usage: None,
        }),
        output: None,
        item: failure_item(&failure, &b),
    })
    .unwrap();
    assert_eq!(r.step(&b.id).status, "failed");
    assert_eq!(r.item(&failure).kind, InboxKind::Failure);
    assert!(r.versions(DOCUMENT).is_empty());
}

/// fix 2 (S2, C1; AD-28, AD-29): a Draft is raised only with its version. A
/// `PauseStep` or a `FailWithItem` raising a `draft` item on an attempt
/// that wrote no version (a document attempt still running, and an attempt
/// that declares no document) is refused with nothing written: the
/// attempt stays `running`, its call unsettled, no item.
#[test]
fn a_draft_is_raised_only_with_its_version() {
    let r = rig_with(&["summarise"]);
    let declared = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let declared_call = r.reserve(&declared);
    let other = r.new_run(&["note"]);
    let undeclared = r.begin(&other, "note", None, None, Taint::Clean);
    let undeclared_call = r.reserve(&undeclared);

    for (a, call) in [(&declared, &declared_call), (&undeclared, &undeclared_call)] {
        for (what, cmd) in [
            (
                "PauseStep",
                WriteCommand::PauseStep {
                    step_run_id: a.id.clone(),
                    inputs: vec![],
                    taint: Taint::Clean,
                    item: draft_item(&new_id(), a, DOCUMENT, Taint::Clean),
                },
            ),
            (
                "FailWithItem",
                WriteCommand::FailWithItem {
                    step_run_id: a.id.clone(),
                    settle: Some(CallSettlement {
                        call_id: call.clone(),
                        usage: None,
                    }),
                    output: None,
                    item: draft_item(&new_id(), a, DOCUMENT, Taint::Clean),
                },
            ),
        ] {
            let before = r.state();
            assert!(r.exec(cmd).is_err(), "{what} on `{}`: refused", a.key);
            assert_eq!(r.state(), before, "{what} on `{}`: nothing written", a.key);
            assert_eq!(r.step(&a.id).status, "running", "{what} on `{}`", a.key);
            assert!(
                !r.reader().calls_for_step(&a.id).unwrap()[0].settled,
                "{what} on `{}`: the call stays unsettled",
                a.key
            );
        }
    }
    assert!(r.versions(DOCUMENT).is_empty());
}

/// An item of `kind` on the attempt `a` with no payload: a Review carries
/// `max_returns` 0, as 3.4's tests build one; any other kind none.
fn pause_item(id: &str, a: &Attempt, kind: InboxKind) -> NewInboxItem {
    NewInboxItem {
        id: id.into(),
        kind,
        payload: None,
        taint: Taint::Clean,
        title: format!("{} on step {}", kind.title(), a.key),
        body: format!("Step `{}` waits for a person.", a.key),
        max_returns: (kind == InboxKind::Review).then_some(0),
    }
}

/// Review round 3, fix G1 (AD-8, AD-28, AD-29; tester, 2026-11-25): a
/// document attempt never waits on a Review. A person's approve of one
/// would finish the attempt `succeeded` with no version (`ApplyResolution`'s
/// approve arm), so the pause itself is refused: a `PauseStep` with a
/// Review item on a running attempt that journaled a document changes
/// nothing (the attempt `running`, no item, no audit entry). The controls:
/// the same Review on a plain attempt (no document) is written, and so is a
/// Question on a document attempt (story 3.10's overflow pause may need
/// one).
#[test]
fn a_document_attempt_never_waits_on_a_review() {
    let r = rig_with(&["summarise"]);
    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let before = r.state();

    let refused = r.exec(WriteCommand::PauseStep {
        step_run_id: a.id.clone(),
        inputs: vec![],
        taint: Taint::Clean,
        item: pause_item(&new_id(), &a, InboxKind::Review),
    });

    assert!(
        refused.is_err(),
        "a Review on a document attempt is refused"
    );
    assert_eq!(r.state(), before, "nothing written");
    assert_eq!(r.step(&a.id).status, "running");
    assert!(r.reader().inbox_items(&r.run.id).unwrap().is_empty());

    // The control: the same Review on a plain attempt.
    let run = r.new_run(&["check"]);
    let plain = r.begin(&run, "check", None, None, Taint::Clean);
    let review = new_id();
    r.exec(WriteCommand::PauseStep {
        step_run_id: plain.id.clone(),
        inputs: vec![],
        taint: Taint::Clean,
        item: pause_item(&review, &plain, InboxKind::Review),
    })
    .unwrap();
    assert_eq!(r.step(&plain.id).status, "waiting_human");
    assert_eq!(r.item(&review).kind, InboxKind::Review);

    // And a Question on the document attempt still pauses it.
    let question = new_id();
    r.exec(WriteCommand::PauseStep {
        step_run_id: a.id.clone(),
        inputs: vec![],
        taint: Taint::Clean,
        item: pause_item(&question, &a, InboxKind::Question),
    })
    .unwrap();
    assert_eq!(r.step(&a.id).status, "waiting_human");
    assert_eq!(r.item(&question).kind, InboxKind::Question);
    assert!(r.versions(DOCUMENT).is_empty());
}

/// fix 3 (S3; AD-29, owner Q7): the finish names the version it expects, the
/// document's newest + 1 as the engine read it through
/// `Reader::latest_document_version` (`None` for a document with no
/// version). A stale number (one another run took first), one ahead of
/// the journal and 0 are each refused with nothing written: the attempt
/// stays `running`, its call unsettled, no version, no item. The right
/// number commits.
#[test]
fn a_stale_expected_version_is_refused_with_nothing_written() {
    let r = rig_with(&["summarise"]);
    assert_eq!(r.reader().latest_document_version(DOCUMENT).unwrap(), None);
    let first = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    r.document(&first, &markdown());
    assert_eq!(
        r.reader().latest_document_version(DOCUMENT).unwrap(),
        Some(1)
    );
    assert_eq!(
        r.reader().latest_document_version(OTHER_DOCUMENT).unwrap(),
        None,
        "another document has none"
    );

    let run = r.new_run(&["summarise"]);
    let a = r.begin(&run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    let call = r.reserve(&a);
    for (what, expected) in [("stale", 1), ("ahead", 3), ("zero", 0)] {
        let before = r.state();
        let refused = r.document_at(
            &a,
            &call,
            DOCUMENT,
            "# Second\n",
            &new_id(),
            Taint::Clean,
            expected,
        );
        assert!(refused.is_err(), "{what} ({expected}): refused");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&a.id).status, "running", "{what}");
        assert!(
            !r.reader().calls_for_step(&a.id).unwrap()[0].settled,
            "{what}: the call stays unsettled"
        );
    }

    let item = new_id();
    let id = r
        .document_at(&a, &call, DOCUMENT, "# Second\n", &item, Taint::Clean, 2)
        .unwrap();
    let versions = r.versions(DOCUMENT);
    assert_eq!(
        versions
            .iter()
            .map(|v| (v.version, v.id.clone(), v.step_run_id.clone()))
            .next_back(),
        Some((2, id, a.id.clone()))
    );
    assert_eq!(
        r.reader().latest_document_version(DOCUMENT).unwrap(),
        Some(2)
    );
    assert_eq!(r.item(&item).kind, InboxKind::Draft);
}

/// fix 6 (S4, C4; AD-21, AD-29): a document is named by its config id, which
/// is kebab-case (am-config's rule, shared with the engine's check). An
/// attempt whose writes name a document with a space, a `/`, a
/// right-to-left override (U+202E) or a zero-width space (U+200B) does not
/// start: refused with nothing written, the attempt `queued` with no
/// writes. A kebab-case id starts (the control).
#[test]
fn a_document_id_outside_the_id_rule_never_starts() {
    let r = rig_with(&["summarise"]);
    let queued = r.latest(&r.run, "summarise");

    for bad in [
        "summary note",
        "summary/note",
        "summary\u{202E}note",
        "summary\u{200B}note",
    ] {
        let before = r.state();
        let refused = r.start_with(
            &r.run,
            "summarise",
            writes(None, Some(bad)),
            Taint::Clean,
            vec![],
            vec![],
        );
        assert!(refused.is_err(), "{bad:?}: refused");
        assert_eq!(r.state(), before, "{bad:?}: nothing written");
        assert_eq!(r.step(&queued.id).status, "queued", "{bad:?}");
        assert_eq!(
            r.reader().attempt_writes(&queued.id).unwrap(),
            None,
            "{bad:?}"
        );
    }

    let a = r.begin(&r.run, "summarise", None, Some(DOCUMENT), Taint::Clean);
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(None, Some(DOCUMENT)))
    );
}

// ------------------------------------------------------------ migration 0012
//
// Row 41 (AD-7, AD-35): 0012 adds `document_versions` and
// `step_runs.writes_document` behind AD-7's pre-migration copy. Written in
// 3.6b's tests-first step (tester, 2026-11-25), in the pattern of 3.6c's
// 0011 tests (`tests/row_inputs.rs`).

/// The schema version before migration 0012.
const BEFORE_DOCUMENTS: i64 = 11;

fn layout(root: &Path) -> RuntimeLayout {
    RuntimeLayout::at(root.join("rt"))
}

fn open_project(root: &Path) -> Result<(Store, OpenReport), StoreError> {
    am_store::open_runtime(&layout(root), "test-mac".into())
}

/// The runtime folder made by hand, owner-only, as an earlier open left it.
fn existing_runtime(root: &Path) {
    let dir = layout(root).dir().to_path_buf();
    std::fs::create_dir(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn columns(db: &Path, table: &str) -> Vec<String> {
    let c = Connection::open(db).unwrap();
    let mut st = c
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// A schema-11 database in the project's runtime folder, made by hand.
fn database_at_11(root: &Path) -> PathBuf {
    existing_runtime(root);
    let path = layout(root).state_db();
    am_store::testing::database_at_version(&path, BEFORE_DOCUMENTS).unwrap();
    path
}

/// The id of the row the fixture's seat wrote, and of a person's row.
const ROW_1: &str = "0193f1a2-0000-7000-8000-00000000b001";
const ROW_2: &str = "0193f1a2-0000-7000-8000-00000000b002";

/// Rows in every table a run touches, as 3.3's to 3.6c's writers made them
/// at version 11: two runs (one running, one failed); attempts with their
/// journaled writes (seat, table); settled and open calls (a turn and a
/// repair) with their ledger rows; inputs (a step output, a plan input, a
/// gate note and a row edge naming an after-image); tool calls with each
/// result kind (`engine` for two row writes, `untrusted` for a read, none
/// for a refused call); a seat's row with two after-images and a person's
/// row with one; an open Review and a dismissed Failure item; prompt parts;
/// a trigger and a fire.
fn rows_at_11(path: &Path) {
    let step = "id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at";
    let call = "id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at, purpose";
    let tool = "id, step_run_id, call_seq, tool, status, created_at, finished_at";
    let image = "table_id, row_id, version, data, taint, actor, run_id, step_run_id, seat, call_seq, tool_call_id, op, created_at";
    let row = "table_id, row_id, data, version, taint, created_at, updated_at";
    let input = "step_run_id, kind, from_step_run, label, taint, created_at, from_item, from_row";
    let part =
        "step_run_id, call_seq, ord, slot, kind, label, taint, text, bytes, sha256, created_at";
    let sql = r#"
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r1', 'wf', 'manual', 'running', '2026-11-25T10:00:00.000Z', '2026-11-25T10:00:09.000Z', '@HASH@', NULL);
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r2', 'wf', 'schedule', 'failed', '2026-11-25T11:00:00.000Z', '2026-11-25T11:00:09.000Z', NULL, '2026-11-25T11:00:09.000Z');
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-a', 'r1', 'a', 0, 1, 'queued', NULL, 0, '2026-11-25T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-b', 'r1', 'b', 1, 1, 'queued', NULL, 0, '2026-11-25T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-c', 'r1', 'c', 2, 1, 'queued', NULL, 0, '2026-11-25T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r2-a', 'r2', 'a', 0, 1, 'queued', NULL, 0, '2026-11-25T11:00:00.000Z', NULL, NULL);
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', writes_table = 'entries', taint = 1,
          started_at = '2026-11-25T10:00:01.000Z', updated_at = '2026-11-25T10:00:01.000Z' WHERE id = 'r1-a';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t1', 'r1-a', 1, 'table_write', 'issued', '2026-11-25T10:00:02.000Z', NULL);
        INSERT INTO row_history (@IMAGE@)
          VALUES ('entries', '@ROW1@', 1, '{"ref":"r-1"}', 1, '@SEAT@', 'r1', 'r1-a', 'seat-writer', 1, 't1', 'insert', '2026-11-25T10:00:02.100Z');
        INSERT INTO table_rows (@ROW@)
          VALUES ('entries', '@ROW1@', '{"ref":"r-1"}', 1, 1, '2026-11-25T10:00:02.100Z', '2026-11-25T10:00:02.100Z');
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-25T10:00:02.100Z',
          result_kind = 'engine', result_label = 'tool:table_write' WHERE id = 't1';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t2', 'r1-a', 2, 'table_write', 'issued', '2026-11-25T10:00:02.200Z', NULL);
        INSERT INTO row_history (@IMAGE@)
          VALUES ('entries', '@ROW1@', 2, '{"ref":"r-1","state":"done"}', 1, '@SEAT@', 'r1', 'r1-a', 'seat-writer', 2, 't2', 'update', '2026-11-25T10:00:02.300Z');
        UPDATE table_rows SET data = '{"ref":"r-1","state":"done"}', version = 2, updated_at = '2026-11-25T10:00:02.300Z'
          WHERE table_id = 'entries' AND row_id = '@ROW1@';
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-25T10:00:02.300Z',
          result_kind = 'engine', result_label = 'tool:table_write' WHERE id = 't2';
        UPDATE step_runs SET status = 'succeeded', output = 'out of a', finished_at = '2026-11-25T10:00:03.000Z',
          updated_at = '2026-11-25T10:00:03.000Z' WHERE id = 'r1-a';
        INSERT INTO row_history (@IMAGE@)
          VALUES ('others', '@ROW2@', 1, '{"ref":"o-1"}', 0, '@HUMAN@', NULL, NULL, NULL, NULL, NULL, 'insert', '2026-11-25T10:00:03.500Z');
        INSERT INTO table_rows (@ROW@)
          VALUES ('others', '@ROW2@', '{"ref":"o-1"}', 1, 0, '2026-11-25T10:00:03.500Z', '2026-11-25T10:00:03.500Z');
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', taint = 1,
          started_at = '2026-11-25T10:00:04.000Z', updated_at = '2026-11-25T10:00:09.000Z' WHERE id = 'r1-b';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t3', 'r1-b', 1, 'fixture_read', 'issued', '2026-11-25T10:00:06.000Z', NULL);
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-25T10:00:07.000Z',
          result_kind = 'untrusted', result_label = 'tool:fixture_read' WHERE id = 't3';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t4', 'r1-b', 2, 'fixture_act', 'refused', '2026-11-25T10:00:08.000Z', '2026-11-25T10:00:08.000Z');
        UPDATE step_runs SET status = 'failed', started_at = '2026-11-25T11:00:01.000Z',
          finished_at = '2026-11-25T11:00:09.000Z', updated_at = '2026-11-25T11:00:09.000Z' WHERE id = 'r2-a';
        INSERT INTO model_calls (@CALL@)
          VALUES ('c1', 'r1-a', 1, 'settled', 1, 12, 7, '2026-11-25T10:00:02.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c4', 'r1-a', 2, 'settled', 1, 9, 4, '2026-11-25T10:00:02.500Z', 'repair');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c2', 'r1-b', 1, 'reserved', 0, NULL, NULL, '2026-11-25T10:00:05.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c3', 'r2-a', 1, 'settled', 1, 3, 0, '2026-11-25T11:00:02.000Z', 'turn');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('l1', 'c1', 'r1', 'model_call', 0, 0, 'settled', 'none', '2026-11-25T10:00:02.000Z');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('l2', 'c2', 'r1', 'model_call', 0, NULL, 'reserved', 'none', '2026-11-25T10:00:05.000Z');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('l3', 'c3', 'r2', 'model_call', 0, 0, 'settled', 'none', '2026-11-25T11:00:02.000Z');
        INSERT INTO step_inputs (@INPUT@)
          VALUES ('r1-b', 'step_output', 'r1-a', 'step:a@1', 1, '2026-11-25T10:00:04.000Z', NULL, NULL);
        INSERT INTO step_inputs (@INPUT@)
          VALUES ('r1-b', 'untrusted', NULL, 'test:page', 1, '2026-11-25T10:00:04.000Z', NULL, NULL);
        INSERT INTO step_inputs (@INPUT@)
          VALUES ('r1-b', 'row', NULL, 'row:entries/@ROW1@@2', 1, '2026-11-25T10:00:04.000Z', NULL,
                  (SELECT seq FROM row_history WHERE table_id = 'entries' AND row_id = '@ROW1@' AND version = 2));
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns)
          VALUES ('i1', 'review', 'open', 'r1', 'r1-b', 'r1-a', 1, 'Review', 'Step b waits for your review.', '2026-11-25T10:00:09.000Z', 2);
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by, decision)
          VALUES ('i2', 'failure', 'resolved', 'r2', 'r2-a', 'r2-a', 0, 'A step stopped', 'Step a stopped.', '2026-11-25T11:00:09.000Z', '2026-11-25T11:05:00.000Z', '@HUMAN@', 'dismiss');
        INSERT INTO step_inputs (@INPUT@)
          VALUES ('r1-c', 'gate_note', NULL, 'gate-note:b@1', 0, '2026-11-25T10:00:10.000Z', 'i1', NULL);
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 1, 'step', 'instruction', 'Step instruction', 0, 'Do the work of b.', 17, NULL, '2026-11-25T10:00:04.000Z');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 2, 'untrusted', 'untrusted', 'step:a@1', 1, NULL, 0, NULL, '2026-11-25T10:00:04.000Z');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 3, 'untrusted', 'untrusted', 'test:page', 1, 'a page', 6, NULL, '2026-11-25T10:00:04.000Z');
        INSERT INTO triggers (id, cron, tz, catch_up, state, next_fire, last_fired)
          VALUES ('trg', '0 9 * * *', 'UTC', 'once', 'active', '2026-11-26T09:00:00.000Z', NULL);
        INSERT INTO fires (trigger_id, scheduled_for, fired_at, kind)
          VALUES ('trg', '2026-11-25T09:00:00.000Z', '2026-11-25T09:00:01.000Z', 'on_time');
    "#;
    let seat = json!({
        "kind": "seat",
        "seat": SEAT,
        "run": "0193f1a2-0000-7000-8000-000000000001",
        "step": "a",
    })
    .to_string();
    let human = json!({ "kind": "human", "person": "owner", "hat": "Reviewer" }).to_string();
    let sql = sql
        .replace("@STEP@", step)
        .replace("@CALL@", call)
        .replace("@TOOL@", tool)
        .replace("@IMAGE@", image)
        .replace("@ROW@", row)
        .replace("@INPUT@", input)
        .replace("@PART@", part)
        .replace("@HASH@", HASH)
        .replace("@ROW1@", ROW_1)
        .replace("@ROW2@", ROW_2)
        .replace("@SEAT@", &seat)
        .replace("@HUMAN@", &human);
    Connection::open(path).unwrap().execute_batch(&sql).unwrap();
}

/// Every table's columns and rows (raw cells, in rowid order) as `path`
/// holds them now.
type Tables = BTreeMap<String, (Vec<String>, Vec<Vec<Cell>>)>;

fn tables_of(path: &Path) -> Tables {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut out = BTreeMap::new();
    for name in names {
        let cols = columns(path, &name);
        out.insert(name.clone(), (cols.clone(), cells_of(&conn, &name, &cols)));
    }
    out
}

fn cells_of(conn: &Connection, table: &str, cols: &[String]) -> Vec<Vec<Cell>> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {} FROM \"{table}\" ORDER BY rowid",
            cols.join(", ")
        ))
        .unwrap();
    let n = st.column_count();
    st.query_map([], |r| {
        (0..n)
            .map(|i| r.get::<_, Cell>(i))
            .collect::<rusqlite::Result<Vec<Cell>>>()
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// The number of files under `dir`, recursively (0 when it is missing).
fn walk_count(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() { walk_count(&p) } else { 1 }
        })
        .sum()
}

/// Row 41: every row of every table the schema-11 database held is the
/// same value for value after 0012 (in its old columns; the two logs a
/// migration appends to keep their old rows as a prefix); the
/// pre-migration copy the report names exists, opens at version 11 and
/// holds every row as it was, also after a reopen; the step is reported
/// 11 to 12 and audited `store.migrated`.
#[test]
fn migration_0012_keeps_every_row_and_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_11(dir.path());
    rows_at_11(&path);
    let before = tables_of(&path);
    for table in [
        "runs",
        "step_runs",
        "model_calls",
        "ledger",
        "step_inputs",
        "tool_calls",
        "inbox_items",
        "prompt_parts",
        "table_rows",
        "row_history",
        "triggers",
        "fires",
    ] {
        assert!(!before[table].1.is_empty(), "{table} holds rows to keep");
    }
    let edges: Vec<&Vec<Cell>> = before["step_inputs"]
        .1
        .iter()
        .filter(|r| r.contains(&Cell::Text("row".into())))
        .collect();
    assert_eq!(edges.len(), 1, "a row-input edge to keep");
    assert!(
        !before.contains_key("document_versions"),
        "document_versions is 0012's"
    );
    assert!(
        !before["step_runs"]
            .0
            .contains(&"writes_document".to_string()),
        "writes_document is 0012's"
    );

    let (store, report) = open_project(dir.path()).unwrap();

    // A later story may add a migration after it: the 11 to 12 step is
    // applied on the way, not necessarily last.
    const { assert!(SCHEMA_VERSION > BEFORE_DOCUMENTS) };
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_DOCUMENTS)
        .expect("the 11 to 12 step is reported");
    assert_eq!(step.to, BEFORE_DOCUMENTS + 1);
    let backup = step.backup.clone().expect("a pre-migration copy (AD-7)");
    assert!(backup.is_file(), "{}", backup.display());
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("from version 11 to 12")),
        "{:?}",
        report.lines()
    );

    let after = tables_of(&path);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    for (table, (cols, rows)) in &before {
        let (_, now) = after
            .get(table)
            .unwrap_or_else(|| panic!("{table} was dropped"));
        let old_columns_now = cells_of(&conn, table, cols);
        if table == "audit_log" || table == "migration_reports" {
            assert!(
                old_columns_now.starts_with(rows),
                "{table}: the old entries are kept, in order"
            );
        } else {
            assert_eq!(
                &old_columns_now, rows,
                "{table}: every row, value for value"
            );
            assert_eq!(now.len(), rows.len(), "{table}: no row added or lost");
        }
    }
    drop(conn);

    let copy_is_as_it_was = |what: &str| {
        let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(user_version(&copy), BEFORE_DOCUMENTS, "{what}");
        drop(copy);
        assert_eq!(
            tables_of(&backup),
            before,
            "{what}: the copy holds every row"
        );
    };
    copy_is_as_it_was("after the upgrade");

    let reader = store.reader().unwrap();
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    let all: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with("from version 11 to 12"))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    drop(reader);
    drop(store);
    let (_store, again) = open_project(dir.path()).unwrap();
    assert!(again.applied.is_empty(), "reopening applies nothing");
    copy_is_as_it_was("after a reopen");
}

/// Row 41: 0012 adds `document_versions` with the plan's columns, empty,
/// and `step_runs.writes_document`, NULL on every old attempt; the reader
/// reads every old attempt's writes with no document, no version of any
/// document, and the old inputs (the row edge with its after-image) and
/// rows as they were.
#[test]
fn migration_0012_adds_document_versions_and_a_null_column() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_11(dir.path());
    rows_at_11(&path);
    let edge_before = {
        let c = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        c.query_row(
            "SELECT from_row FROM step_inputs WHERE kind = 'row'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
    };

    let (store, _) = open_project(dir.path()).unwrap();

    let have = columns(&path, "document_versions");
    for column in [
        "id",
        "document_id",
        "version",
        "run_id",
        "attempt",
        "step_run_id",
        "taint",
        "bytes",
        "created_at",
    ] {
        assert!(
            have.contains(&column.to_string()),
            "document_versions.{column}: {have:?}"
        );
    }
    assert!(
        !have
            .iter()
            .any(|c| c == "text" || c == "markdown" || c == "output"),
        "the Markdown stays on the attempt's output (AD-43): {have:?}"
    );
    assert!(
        columns(&path, "step_runs").contains(&"writes_document".to_string()),
        "{:?}",
        columns(&path, "step_runs")
    );
    let c = Connection::open(&path).unwrap();
    let versions: i64 = c
        .query_row("SELECT COUNT(*) FROM document_versions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(versions, 0, "document_versions is empty");
    let set: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM step_runs WHERE writes_document IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(set, 0, "no old attempt versions a document");
    drop(c);

    let reader = store.reader().unwrap();
    assert_eq!(
        reader.attempt_writes("r1-a").unwrap(),
        Some(writes(Some(TABLE), None))
    );
    assert_eq!(
        reader.attempt_writes("r1-b").unwrap(),
        Some(writes(None, None))
    );
    for id in ["r1-c", "r2-a"] {
        assert_eq!(reader.attempt_writes(id).unwrap(), None, "{id}");
    }
    for document in [DOCUMENT, OTHER_DOCUMENT] {
        assert!(reader.document_versions(document).unwrap().is_empty());
    }
    let edges: Vec<StepInput> = reader
        .step_inputs("r1-b")
        .unwrap()
        .into_iter()
        .filter(|i| i.kind == "row")
        .collect();
    assert_eq!(
        edges,
        vec![StepInput {
            kind: "row".into(),
            from_step_run: None,
            from_item: None,
            from_row: Some(edge_before),
            label: format!("row:{TABLE}/{ROW_1}@2"),
            taint: Taint::Tainted,
        }],
        "the row edge as it was"
    );
    assert_eq!(reader.row_history_of(TABLE, ROW_1).unwrap().len(), 2);
    assert_eq!(
        reader
            .table_rows_of(OTHER_TABLE)
            .unwrap()
            .iter()
            .map(|r| r.row_id.clone())
            .collect::<Vec<_>>(),
        vec![ROW_2.to_string()]
    );
    let items: Vec<(String, InboxKind)> = reader
        .inbox_items("r1")
        .unwrap()
        .into_iter()
        .chain(reader.inbox_items("r2").unwrap())
        .map(|i| (i.id, i.kind))
        .collect();
    assert_eq!(
        items,
        vec![
            ("i1".to_string(), InboxKind::Review),
            ("i2".to_string(), InboxKind::Failure)
        ]
    );
}

/// Row 41: a database already at version 12 opens with nothing applied and
/// no new pre-migration copy.
#[test]
fn reopening_a_version_12_database_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_11(dir.path());
    rows_at_11(&path);
    let (store, first) = open_project(dir.path()).unwrap();
    assert!(
        first
            .applied
            .iter()
            .any(|m| (m.from, m.to) == (BEFORE_DOCUMENTS, BEFORE_DOCUMENTS + 1)),
        "{first:?}"
    );
    drop(store);
    let before = tables_of(&path);
    let backups = |root: &Path| -> usize { walk_count(&layout(root).dir().join("backups")) };
    let copies = backups(dir.path());

    let (_store, again) = open_project(dir.path()).unwrap();

    assert!(again.applied.is_empty(), "{again:?}");
    assert_eq!(again.version, SCHEMA_VERSION);
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), SCHEMA_VERSION);
    drop(c);
    assert_eq!(backups(dir.path()), copies, "no new pre-migration copy");
    let after = tables_of(&path);
    for table in before.keys().filter(|t| *t != "audit_log") {
        assert_eq!(after[table], before[table], "{table}");
    }
}

/// Row 41: the step fails closed and names its backup. A `writes_document`
/// column added to `step_runs` by hand stops 0012; the database is left at
/// version 11 with every row, and the error holds the copy's path.
#[test]
fn a_failing_0012_names_the_backup_and_leaves_version_11() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_11(dir.path());
    rows_at_11(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE step_runs ADD COLUMN writes_document TEXT;")
        .unwrap();
    let before = tables_of(&path);

    let refused = match open_project(dir.path()) {
        Ok((_, report)) => panic!("the forged column must fail 0012 closed: {report:?}"),
        Err(e) => e,
    };

    let words = refused.to_string();
    match &refused {
        StoreError::MigrationFailed {
            from, to, backup, ..
        } => {
            assert_eq!((*from, *to), (BEFORE_DOCUMENTS, BEFORE_DOCUMENTS + 1));
            let backup = backup.as_ref().expect("the step names its backup");
            assert!(backup.is_file(), "the pre-migration copy is kept");
            assert!(
                words.contains(&backup.display().to_string()),
                "the error names the backup: {words}"
            );
        }
        other => panic!("MigrationFailed expected: {other:?}"),
    }
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), BEFORE_DOCUMENTS, "the step rolled back");
    drop(c);
    assert_eq!(tables_of(&path), before, "every row as it was");
}
