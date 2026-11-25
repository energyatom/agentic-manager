//! Epic 3 story 4 at the store (AD-8, AD-10, AD-15, AD-24, AD-25, AD-28,
//! AD-36, AD-43): the one Human-only resolve of an Inbox item and the
//! engine's apply of a decision a person committed.
//!
//! - `Writer::resolve_inbox_item(by: HumanActor, item_id, decision, note)`
//!   is the only way an item is resolved: its actor is Human by type, and
//!   no `WriteCommand` resolves. In one transaction it records the item's
//!   decision (`status = 'resolved'`, `decision`, `note` for a return or an
//!   answer only, `resolved_by` as the actor's JSON, `resolved_at`), takes
//!   AD-8's one Human edge (`waiting_human → running`) for a Review or a
//!   Question, and writes one `inbox.resolved` audit entry (kind, decision,
//!   step run, taint, the note's sha256; never its text).
//! - A resolved item never changes again (the writer refuses, naming who
//!   and when; triggers abort a raw `UPDATE`). A decision fits its kind
//!   (`Decision::allowed_for`). A note is one line of 1 to 2000 characters
//!   with no control character but tab.
//! - `WriteCommand::ApplyResolution { item_id, past_max }` (System only) is
//!   the engine's follow-up, decided from committed rows: approve finishes
//!   the gate attempt `succeeded`; return loops it back (`looped_back`,
//!   attempt n+1 of the producer through the gate queued, the producer's
//!   new attempt's `gate_note` input journaled once) or, past the Review
//!   item's `max_returns`, fails it with one Failure item and the run;
//!   reject fails the gate and the run. The writer checks the return count
//!   from the journal and refuses an apply that does not match the
//!   decision, on an open item, or twice.
//!
//! The four doctests on `Writer::resolve_inbox_item` (each `compile_fail`
//! paired with an allowed build) and the writer-refusal src unit test are
//! the implementer's; the last test here checks the doctests are there.
//! Written before the command, the apply or the readers exist.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_domain::{Actor, Decision, HumanActor, InboxKind, REVIEWER_HAT, Taint};
use am_store::{
    InboxItemRow, NewInboxItem, Reader, RunRow, StepInput, StepRow, Store, StoreError,
    WriteCommand, new_id,
};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// A return note.
const NOTE: &str = "tighten the summary";
/// The draft's committed output (the Review's payload).
const DRAFT_OUT: &str = "the version to review";
/// A plan input's label (an untrusted page, AD-10).
const POISON_LABEL: &str = "test:poisoned-page";

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// The owner wearing the Reviewer hat: the one resolver (AD-15, AD-24).
fn reviewer() -> HumanActor {
    HumanActor::new("owner", REVIEWER_HAT)
}

/// The same person as an `Actor`, as `resolved_by` and the audit hold it.
fn you_reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

/// Every actor but the app itself (AD-24).
fn not_the_engine() -> Vec<Actor> {
    let id = Uuid::now_v7();
    vec![
        you_reviewer(),
        Actor::Seat {
            seat: "seat-a".into(),
            run: id,
            step: "approve".into(),
        },
        Actor::Assistant { proposal: id },
        Actor::Import {
            source: "an import".into(),
        },
    ]
}

fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// A taint flag as an audit argument: `0` or `false` (the writer's choice).
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

/// What a refused command must leave as it was: every attempt, every item,
/// the run, every attempt's journaled inputs and the number of audit
/// entries.
#[derive(Debug, PartialEq)]
struct State {
    steps: Vec<StepRow>,
    items: Vec<InboxItemRow>,
    run: RunRow,
    inputs: Vec<Vec<StepInput>>,
    audit: usize,
}

/// A store with one run of `draft`, `approve` and `publish` (a step, a
/// human gate, a step after it). `item` names the item a test acts on.
struct Rig {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
    run: String,
    item: String,
}

/// `draft` succeeded; nothing else happened.
fn base(taint: Taint) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    let run = new_id();
    store
        .writer()
        .exec(
            engine(),
            WriteCommand::CreateRun {
                run_id: run.clone(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["draft".into(), "approve".into(), "publish".into()],
                config_hash: None,
            },
        )
        .unwrap();
    let r = Rig {
        _dir: dir,
        db,
        store,
        run,
        item: new_id(),
    };
    let inputs = if taint.is_tainted() {
        vec![untrusted()]
    } else {
        vec![]
    };
    r.produce(inputs, DRAFT_OUT);
    r
}

/// The gate `approve` waits with an open, clean Review item (`max_returns`
/// 2) whose payload is `draft`'s attempt.
fn rig() -> Rig {
    rig_with(Some(2), Taint::Clean)
}

/// [`rig`] with the Review item's `max_returns` and the draft's taint.
fn rig_with(max_returns: Option<u32>, taint: Taint) -> Rig {
    let r = base(taint);
    r.pause(&r.item, max_returns).unwrap();
    r
}

/// The gate `approve` waits with an open Question (no payload).
fn question() -> Rig {
    let r = base(Taint::Clean);
    let approve = r.latest("approve");
    r.exec(WriteCommand::PauseStep {
        step_run_id: approve.id,
        inputs: vec![],
        taint: Taint::Clean,
        item: NewInboxItem {
            id: r.item.clone(),
            kind: InboxKind::Question,
            payload: None,
            taint: Taint::Clean,
            title: "A question about step approve".into(),
            body: "Step `approve` asks which input to use.".into(),
            max_returns: None,
        },
    })
    .unwrap();
    r
}

/// The gate `approve` waits with an open item of `kind` (refused kinds).
fn waiting_with(kind: InboxKind) -> Rig {
    let r = base(Taint::Clean);
    let approve = r.latest("approve");
    let draft = r.latest("draft");
    r.exec(WriteCommand::PauseStep {
        step_run_id: approve.id,
        inputs: vec![],
        taint: Taint::Clean,
        item: NewInboxItem {
            id: r.item.clone(),
            kind,
            payload: Some(draft.id),
            taint: Taint::Clean,
            title: format!("A {} item", kind.title()),
            body: "An item of a kind this story does not resolve.".into(),
            max_returns: None,
        },
    })
    .unwrap();
    r
}

/// `approve` ran and failed with an open Failure item; the run failed.
fn failed() -> Rig {
    let r = base(Taint::Clean);
    let approve = r.latest("approve");
    r.exec(WriteCommand::StartAttempt {
        step_run_id: approve.id.clone(),
        inputs: vec![],
        taint: Taint::Clean,
        prompt: vec![],
        writes: am_store::AttemptWrites {
            seat: "seat-a".into(),
            table: None,
            document: None,
        },
    })
    .unwrap();
    r.exec(WriteCommand::FailWithItem {
        step_run_id: approve.id.clone(),
        settle: None,
        output: None,
        item: NewInboxItem {
            id: r.item.clone(),
            kind: InboxKind::Failure,
            payload: Some(approve.id),
            taint: Taint::Clean,
            title: "A step stopped".into(),
            body: "Step `approve` stopped at a cap.".into(),
            max_returns: None,
        },
    })
    .unwrap();
    r
}

/// A plan input: untrusted, so it taints its attempt (AD-10).
fn untrusted() -> StepInput {
    StepInput {
        kind: "untrusted".into(),
        from_step_run: None,
        from_item: None,
        from_row: None,
        label: POISON_LABEL.into(),
        taint: Taint::Tainted,
    }
}

/// The Failure item an engine's past-max apply carries.
fn failure_item() -> NewInboxItem {
    NewInboxItem {
        id: new_id(),
        kind: InboxKind::Failure,
        payload: None,
        taint: Taint::Clean,
        title: "A gate passed its maximum of returns".into(),
        body: "Step `approve` was returned past its max_returns.".into(),
        max_returns: None,
    }
}

impl Rig {
    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    fn exec(&self, cmd: WriteCommand) -> Result<(), StoreError> {
        self.store.writer().exec(engine(), cmd)
    }

    fn resolve(
        &self,
        item_id: &str,
        decision: Decision,
        note: Option<&str>,
    ) -> Result<(), StoreError> {
        self.store.writer().resolve_inbox_item(
            reviewer(),
            item_id,
            decision,
            note.map(str::to_string),
        )
    }

    fn apply(&self, item_id: &str, past_max: Option<NewInboxItem>) -> Result<(), StoreError> {
        self.exec(WriteCommand::ApplyResolution {
            item_id: item_id.into(),
            past_max,
        })
    }

    fn steps(&self) -> Vec<StepRow> {
        self.reader().steps(&self.run).unwrap()
    }

    /// The attempts of step `key`, in attempt order.
    fn attempts(&self, key: &str) -> Vec<StepRow> {
        self.steps()
            .into_iter()
            .filter(|s| s.step_key == key)
            .collect()
    }

    /// The latest attempt of step `key`.
    fn latest(&self, key: &str) -> StepRow {
        self.attempts(key)
            .into_iter()
            .max_by_key(|s| s.attempt)
            .unwrap_or_else(|| panic!("no step {key}"))
    }

    fn items(&self) -> Vec<InboxItemRow> {
        self.reader().inbox_items(&self.run).unwrap()
    }

    /// One item, read by id.
    fn item_row(&self, id: &str) -> InboxItemRow {
        self.reader()
            .inbox_item(id)
            .unwrap()
            .unwrap_or_else(|| panic!("no item {id}"))
    }

    fn run_row(&self) -> RunRow {
        self.reader().run(&self.run).unwrap().unwrap()
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

    fn state(&self) -> State {
        let steps = self.steps();
        let reader = self.reader();
        let inputs = steps
            .iter()
            .map(|s| reader.step_inputs(&s.id).unwrap())
            .collect();
        State {
            steps,
            items: self.items(),
            run: self.run_row(),
            inputs,
            audit: self.audit().len(),
        }
    }

    /// The latest attempt of `draft` (queued) runs with `inputs`: started,
    /// its call reserved and settled with its finish `succeeded`, holding
    /// `output`.
    fn produce(&self, inputs: Vec<StepInput>, output: &str) {
        let draft = self.latest("draft");
        let call = new_id();
        self.exec(WriteCommand::StartAttempt {
            step_run_id: draft.id.clone(),
            inputs,
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        })
        .unwrap();
        self.exec(WriteCommand::ReserveCall {
            step_run_id: draft.id.clone(),
            call_id: call.clone(),
            call_seq: 1,
            price: am_domain::money::CallPrice::local(),
            rate: am_domain::money::Rate::none("EUR"),
            counted_input_tokens: 0,
            max_output_tokens: 0,
            week: "2026-11-23".into(),
            team: "core".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        })
        .unwrap();
        self.exec(WriteCommand::SettleAndFinishStep {
            call_id: call,
            usage: None,
            step_run_id: draft.id,
            status: "succeeded".into(),
            output: Some(output.into()),
        })
        .unwrap();
    }

    /// The latest attempt of `approve` (queued) waits with the Review item
    /// `id`, whose payload is `draft`'s latest attempt, as the engine
    /// pauses a gate (story 3.4: with the gate's `max_returns`).
    fn pause(&self, id: &str, max_returns: Option<u32>) -> Result<(), StoreError> {
        let draft = self.latest("draft");
        let approve = self.latest("approve");
        self.exec(WriteCommand::PauseStep {
            step_run_id: approve.id,
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(draft.id.clone()),
                from_item: None,
                from_row: None,
                label: format!("step:draft@{}", draft.attempt),
                taint: draft.taint,
            }],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: id.into(),
                kind: InboxKind::Review,
                payload: Some(draft.id.clone()),
                taint: draft.taint,
                title: "Review before step approve goes on".into(),
                body: format!(
                    "Step `approve` waits for your review of the output of step `draft` (attempt {}).",
                    draft.attempt
                ),
                max_returns,
            },
        })
    }

    /// After an applied return: the producer's new attempt runs, and the
    /// gate's new attempt waits with a new Review item; its id.
    fn round(&self, max_returns: Option<u32>) -> String {
        self.produce(vec![], "another version");
        let id = new_id();
        self.pause(&id, max_returns).unwrap();
        id
    }
}

/// Rows 2, 3 and 7 at the store, and row 10: each decision a Review takes
/// is recorded on the item (`resolved`, the decision, the note for a
/// return only, `resolved_by` the Human actor's JSON, `resolved_at` a
/// store timestamp), and the gate attempt takes AD-8's Human edge,
/// `waiting_human → running`, and nothing else: no other attempt, no
/// attempt added, the run unchanged. One audit entry, `inbox.resolved`, by
/// "You (Reviewer)": target the item, args kind, decision, step run,
/// taint and the note's sha256, never the note.
#[test]
fn a_resolve_records_the_decision_and_only_the_human_edge() {
    for (decision, note) in [
        (Decision::Approve, None),
        (Decision::Return, Some(NOTE)),
        (Decision::Reject, None),
    ] {
        let r = rig();
        let gate = r.latest("approve");
        assert_eq!(gate.status, "waiting_human");
        let (steps, run, audit) = (r.steps(), r.run_row(), r.audit().len());

        r.resolve(&r.item, decision, note)
            .unwrap_or_else(|e| panic!("{decision:?}: {e}"));

        let item = r.item_row(&r.item);
        assert_eq!(item.status, "resolved", "{decision:?}");
        assert_eq!(item.decision, Some(decision));
        assert_eq!(item.note.as_deref(), note, "{decision:?}");
        let by: Actor = serde_json::from_str(item.resolved_by.as_deref().expect("resolved_by"))
            .expect("resolved_by holds the actor's JSON");
        assert_eq!(by, you_reviewer());
        assert_eq!(by.describe(), "You (Reviewer)");
        assert_timestamp(item.resolved_at.as_deref().expect("resolved_at"));
        // The item's own fields are as the engine made them.
        assert_eq!(item.kind, InboxKind::Review);
        assert_eq!(item.max_returns, Some(2));
        assert_eq!(item.taint, Taint::Clean);

        let after = r.steps();
        assert_eq!(after.len(), steps.len(), "no attempt is added");
        for (b, a) in steps.iter().zip(&after) {
            if b.id == gate.id {
                assert_eq!(a.status, "running", "AD-8's Human edge");
                assert_eq!(
                    (&a.output, a.taint, &a.started_at, &a.finished_at),
                    (&b.output, b.taint, &b.started_at, &b.finished_at)
                );
            } else {
                assert_eq!(a, b, "{decision:?}: only the gate moves");
            }
        }
        assert_eq!(r.run_row(), run, "the run is unchanged");

        let all = r.audit();
        assert_eq!(all.len(), audit + 1, "one entry: {all:#?}");
        let e = all.last().unwrap();
        assert_eq!(e["action"], "inbox.resolved");
        assert_eq!(e["target"], r.item.as_str());
        assert_eq!(
            e["actor"],
            json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
        assert_eq!(e["args"]["kind"], "review");
        assert_eq!(e["args"]["decision"], decision.as_str());
        assert_eq!(e["args"]["step_run_id"], gate.id.as_str());
        assert!(audited_clean(&e["args"]["taint"]), "{e}");
        match note {
            Some(n) => assert_eq!(e["args"]["note_sha256"], sha256_hex(n)),
            None => assert!(e["args"]["note_sha256"].is_null(), "{e}"),
        }
        assert!(!e.to_string().contains(NOTE), "never the note: {e}");
        assert_eq!(r.reader().verify_audit_chain().unwrap(), None);
    }
}

/// Row 23: a resolved item is resolved once. A second resolve, with any
/// decision the kind takes, is refused in words naming who resolved it and
/// when; nothing changes, before and after the engine applied it. A raw
/// `UPDATE` of a resolved row (its note, decision, status, resolver or
/// taint) is aborted by a trigger.
#[test]
fn a_resolved_item_never_changes_again() {
    let r = rig();
    r.resolve(&r.item, Decision::Approve, None).unwrap();
    let at = r.item_row(&r.item).resolved_at.expect("resolved_at");
    let again = |r: &Rig| {
        let before = r.state();
        for (decision, note) in [
            (Decision::Approve, None),
            (Decision::Return, Some(NOTE)),
            (Decision::Reject, None),
        ] {
            let refused = r
                .resolve(&r.item, decision, note)
                .expect_err("an item is resolved once");
            let words = refused.to_string();
            assert!(words.contains("You (Reviewer)"), "names who: {words}");
            assert!(words.contains(&at), "names when ({at}): {words}");
            assert_eq!(r.state(), before, "{decision:?}");
        }
        for (decision, note) in [(Decision::Dismiss, None), (Decision::Answer, Some("x"))] {
            assert!(r.resolve(&r.item, decision, note).is_err(), "{decision:?}");
            assert_eq!(r.state(), before, "{decision:?}");
        }
    };
    again(&r);
    r.apply(&r.item, None).unwrap();
    again(&r);

    let before = r.state();
    let c = Connection::open(&r.db).unwrap();
    for sql in [
        "UPDATE inbox_items SET note = 'changed' WHERE id = ?1",
        "UPDATE inbox_items SET decision = 'reject' WHERE id = ?1",
        "UPDATE inbox_items SET status = 'open', decision = NULL, resolved_by = NULL, resolved_at = NULL WHERE id = ?1",
        "UPDATE inbox_items SET resolved_by = '{\"kind\":\"system\",\"component\":\"engine\"}' WHERE id = ?1",
        "UPDATE inbox_items SET resolved_at = '2020-01-01T00:00:00.000Z' WHERE id = ?1",
        "UPDATE inbox_items SET taint = 1 WHERE id = ?1",
    ] {
        assert!(
            c.execute(sql, params![r.item]).is_err(),
            "a trigger aborts: {sql}"
        );
    }
    drop(c);
    assert_eq!(r.state(), before);
}

/// Row 24: a decision fits its item's kind. `Decision`'s spellings; the
/// table (Review: approve, return, reject; Question: answer; Failure:
/// dismiss; Draft: dismiss, since story 3.6b; Approval, Budget ask and
/// Proposal: none yet). A decision that does not fit is refused and changes
/// nothing; the three kinds no story built so far resolves are refused in
/// words naming the story each arrives with.
#[test]
fn a_decision_must_fit_the_kind() {
    assert_eq!(
        Decision::ALL.map(Decision::as_str),
        ["approve", "return", "reject", "answer", "dismiss"]
    );
    for d in Decision::ALL {
        assert_eq!(Decision::parse(d.as_str()), Some(d));
    }
    assert_eq!(Decision::parse("Approve"), None);
    assert_eq!(Decision::parse("accept"), None);
    assert_eq!(
        Decision::allowed_for(InboxKind::Review),
        &[Decision::Approve, Decision::Return, Decision::Reject]
    );
    assert_eq!(
        Decision::allowed_for(InboxKind::Question),
        &[Decision::Answer]
    );
    assert_eq!(
        Decision::allowed_for(InboxKind::Failure),
        &[Decision::Dismiss]
    );
    // Story 3.6b (owner Q8): a Draft is dismissed, through this same
    // Human-only resolve (its own tests are in documents.rs).
    assert_eq!(
        Decision::allowed_for(InboxKind::Draft),
        &[Decision::Dismiss]
    );
    for kind in [
        InboxKind::Approval,
        InboxKind::BudgetAsk,
        InboxKind::Proposal,
    ] {
        assert!(Decision::allowed_for(kind).is_empty(), "{kind:?}");
    }

    let refused = |r: &Rig, tries: &[(Decision, Option<&str>)]| {
        let before = r.state();
        for (decision, note) in tries {
            assert!(
                r.resolve(&r.item, *decision, *note).is_err(),
                "{decision:?} on a {:?}",
                r.item_row(&r.item).kind
            );
            assert_eq!(r.state(), before, "{decision:?}");
            assert_eq!(r.item_row(&r.item).status, "open");
        }
    };
    refused(
        &rig(),
        &[
            (Decision::Answer, Some("an answer")),
            (Decision::Dismiss, None),
        ],
    );
    refused(
        &question(),
        &[
            (Decision::Approve, None),
            (Decision::Return, Some(NOTE)),
            (Decision::Reject, None),
            (Decision::Dismiss, None),
        ],
    );
    refused(
        &failed(),
        &[
            (Decision::Approve, None),
            (Decision::Return, Some(NOTE)),
            (Decision::Reject, None),
            (Decision::Answer, Some("an answer")),
        ],
    );
    for (kind, story) in [
        (InboxKind::Approval, "4.8"),
        (InboxKind::BudgetAsk, "4.3"),
        (InboxKind::Proposal, "4.10"),
    ] {
        let r = waiting_with(kind);
        let before = r.state();
        for (decision, note) in [
            (Decision::Approve, None),
            (Decision::Return, Some(NOTE)),
            (Decision::Reject, None),
            (Decision::Answer, Some("an answer")),
            (Decision::Dismiss, None),
        ] {
            let words = r
                .resolve(&r.item, decision, note)
                .expect_err("this kind is resolved by a later story")
                .to_string();
            assert!(
                words.contains(story),
                "{kind:?}, {decision:?}: names story {story}: {words}"
            );
            assert_eq!(r.state(), before, "{kind:?}, {decision:?}");
        }
    }
}

/// Row 25: a return (on a Review) and an answer (on a Question) need a
/// note: one line of 1 to 2000 characters (characters, not bytes) with no
/// control character but tab. Anything else is refused and writes nothing.
/// Approve, reject and dismiss carry no note: one given is refused too
/// (the note is stored for a return and an answer only).
#[test]
fn a_return_or_answer_needs_a_note_within_its_rules() {
    let over = "é".repeat(2001);
    for (what, note) in [
        ("no note", None),
        ("an empty note", Some("")),
        ("a blank note", Some(" \t  ")),
        ("over 2000 characters", Some(over.as_str())),
        ("a newline", Some("one\ntwo")),
        ("a carriage return", Some("one\rtwo")),
        ("an escape", Some("\u{1b}[31mred")),
        ("a bell", Some("ding\u{7}")),
        ("a NUL", Some("a\0b")),
        ("a DEL", Some("a\u{7f}b")),
    ] {
        for (decision, r) in [(Decision::Return, rig()), (Decision::Answer, question())] {
            let before = r.state();
            assert!(
                r.resolve(&r.item, decision, note).is_err(),
                "{decision:?} with {what}"
            );
            assert_eq!(r.state(), before, "{decision:?} with {what}");
        }
    }

    let longest = "é".repeat(2000);
    for note in [longest.as_str(), "keep\tthe table", "x"] {
        for (decision, r) in [(Decision::Return, rig()), (Decision::Answer, question())] {
            r.resolve(&r.item, decision, Some(note))
                .unwrap_or_else(|e| panic!("{decision:?} with {note:?}: {e}"));
            assert_eq!(r.item_row(&r.item).note.as_deref(), Some(note));
        }
    }

    for (decision, r) in [
        (Decision::Approve, rig()),
        (Decision::Reject, rig()),
        (Decision::Dismiss, failed()),
    ] {
        let before = r.state();
        assert!(
            r.resolve(&r.item, decision, Some(NOTE)).is_err(),
            "{decision:?} carries no note"
        );
        assert_eq!(r.state(), before, "{decision:?}");
    }
}

/// Every (table, column) of the database at `db` whose text holds
/// `needle`, one entry per cell.
fn cells_holding(db: &Path, needle: &str) -> Vec<(String, String)> {
    let c = Connection::open(db).unwrap();
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut found = Vec::new();
    for table in tables {
        let mut st = c.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns: Vec<String> = st.column_names().map(str::to_string).collect();
        let mut rows = st.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for (i, column) in columns.iter().enumerate() {
                let holds = match row.get_ref(i).unwrap() {
                    ValueRef::Text(t) => String::from_utf8_lossy(t).contains(needle),
                    ValueRef::Blob(b) => String::from_utf8_lossy(b).contains(needle),
                    _ => false,
                };
                if holds {
                    found.push((table.clone(), column.clone()));
                }
            }
        }
    }
    found
}

/// Row 25 and AD-43: the note is stored once. After a return, its apply and
/// the producer's new attempt starting, the note's text is in exactly one
/// cell of the whole database, `inbox_items.note` (not in `step_inputs`, an
/// output or the audit log), and not in the audit export.
#[test]
fn the_note_is_stored_once() {
    const CANARY: &str = "note-canary-7c1e4b: tighten the opening";
    let r = rig();
    r.resolve(&r.item, Decision::Return, Some(CANARY)).unwrap();
    r.apply(&r.item, None).unwrap();
    let draft = r.latest("draft");
    assert_eq!((draft.attempt, draft.status.as_str()), (2, "queued"));
    r.exec(WriteCommand::StartAttempt {
        step_run_id: draft.id,
        inputs: vec![],
        taint: Taint::Clean,
        prompt: vec![],
        writes: am_store::AttemptWrites {
            seat: "seat-a".into(),
            table: None,
            document: None,
        },
    })
    .unwrap();

    assert_eq!(
        cells_holding(&r.db, CANARY),
        vec![("inbox_items".to_string(), "note".to_string())]
    );
    let mut out = Vec::new();
    r.reader().export_audit_jsonl(&mut out).unwrap();
    assert!(!String::from_utf8(out).unwrap().contains(CANARY));
}

/// Row 26: the engine cannot override a decision. Applying an open item is
/// refused; only `System` applies; after approve, finishing the gate
/// `failed`, failing it with an item or looping it back is refused; after
/// reject, finishing it `succeeded`; after a return within the maximum,
/// finishing it either way. Each refusal changes nothing. The apply that
/// matches the decision commits (approve: the gate `succeeded`; reject:
/// the gate and the run `failed`, no Failure item; return: the gate
/// `looped_back`, attempt 2 of `draft` and `approve` queued with one
/// `gate_note` input, `publish` untouched), and a second apply is refused.
#[test]
fn apply_follows_the_decision_and_nothing_else() {
    // An open item is not applied, by anyone.
    let r = rig();
    let before = r.state();
    assert!(r.apply(&r.item, None).is_err(), "nothing was decided");
    assert_eq!(r.state(), before);

    // Approve.
    r.resolve(&r.item, Decision::Approve, None).unwrap();
    let gate = r.latest("approve").id;
    let before = r.state();
    for actor in not_the_engine() {
        let refused = r.store.writer().exec(
            actor.clone(),
            WriteCommand::ApplyResolution {
                item_id: r.item.clone(),
                past_max: None,
            },
        );
        assert!(refused.is_err(), "only the engine applies: {actor:?}");
        assert_eq!(r.state(), before, "{actor:?}");
    }
    for cmd in [
        WriteCommand::FinishStep {
            step_run_id: gate.clone(),
            status: "failed".into(),
            output: None,
        },
        WriteCommand::FailWithItem {
            step_run_id: gate.clone(),
            settle: None,
            output: None,
            item: failure_item(),
        },
    ] {
        let what = format!("{cmd:?}");
        assert!(r.exec(cmd).is_err(), "against approve: {what}");
        assert_eq!(r.state(), before, "{what}");
    }
    r.apply(&r.item, None).unwrap();
    assert_eq!(r.latest("approve").status, "succeeded");
    assert!(r.latest("approve").finished_at.is_some());
    assert_eq!(r.latest("publish").status, "queued");
    assert_eq!(r.run_row().status, "running");
    assert_eq!(r.items().len(), 1);
    let before = r.state();
    assert!(r.apply(&r.item, None).is_err(), "applied once");
    assert!(r.apply(&r.item, Some(failure_item())).is_err());
    assert_eq!(r.state(), before);

    // Reject.
    let r = rig();
    r.resolve(&r.item, Decision::Reject, None).unwrap();
    let before = r.state();
    let finish = WriteCommand::FinishStep {
        step_run_id: r.latest("approve").id,
        status: "succeeded".into(),
        output: None,
    };
    assert!(r.exec(finish).is_err(), "against reject");
    assert_eq!(r.state(), before);
    r.apply(&r.item, None).unwrap();
    assert_eq!(r.latest("approve").status, "failed");
    assert_eq!(r.latest("publish").status, "queued");
    let run = r.run_row();
    assert_eq!(run.status, "failed");
    assert!(run.finished_at.is_some());
    assert_eq!(r.items().len(), 1, "no Failure item for a reject");
    let before = r.state();
    assert!(r.apply(&r.item, None).is_err(), "applied once");
    assert_eq!(r.state(), before);

    // Return, within the maximum.
    let r = rig();
    r.resolve(&r.item, Decision::Return, Some(NOTE)).unwrap();
    let gate = r.latest("approve").id;
    let before = r.state();
    for status in ["succeeded", "failed"] {
        let finish = WriteCommand::FinishStep {
            step_run_id: gate.clone(),
            status: status.into(),
            output: None,
        };
        assert!(r.exec(finish).is_err(), "against return: {status}");
        assert_eq!(r.state(), before, "{status}");
    }
    r.apply(&r.item, None).unwrap();
    let shape: Vec<(String, i64, String)> = r
        .steps()
        .into_iter()
        .map(|s| (s.step_key, s.attempt, s.status))
        .collect();
    let want: Vec<(String, i64, String)> = [
        ("draft", 1, "succeeded"),
        ("draft", 2, "queued"),
        ("approve", 1, "looped_back"),
        ("approve", 2, "queued"),
        ("publish", 1, "queued"),
    ]
    .iter()
    .map(|(k, a, s)| (k.to_string(), *a, s.to_string()))
    .collect();
    assert_eq!(shape, want);
    assert_eq!(r.run_row().status, "running");
    let inputs = r.reader().step_inputs(&r.latest("draft").id).unwrap();
    assert_eq!(inputs.len(), 1, "{inputs:#?}");
    assert_eq!(inputs[0].kind, "gate_note");
    let before = r.state();
    assert!(r.apply(&r.item, None).is_err(), "applied once");
    assert_eq!(r.state(), before);

    // A model call reserved on the waiting gate's running attempt (were the
    // writer to take one) is never a way to loop it back against approve.
    let r = rig();
    r.resolve(&r.item, Decision::Approve, None).unwrap();
    let gate = r.latest("approve").id;
    let call = new_id();
    let _ = r.exec(WriteCommand::ReserveCall {
        step_run_id: gate.clone(),
        call_id: call.clone(),
        call_seq: 1,
        price: am_domain::money::CallPrice::local(),
        rate: am_domain::money::Rate::none("EUR"),
        counted_input_tokens: 0,
        max_output_tokens: 0,
        week: "2026-11-23".into(),
        team: "core".into(),
        taint: Taint::Clean,
        purpose: am_store::CallPurpose::Turn,
        prompt: vec![],
    });
    let before = r.state();
    let back = WriteCommand::SettleAndLoopBack {
        call_id: call,
        usage: None,
        step_run_id: gate,
        output: r#"{"reasons":["do it again"]}"#.into(),
        to: "draft".into(),
    };
    assert!(r.exec(back).is_err(), "against approve");
    assert_eq!(r.state(), before);
}

/// Rows 4 and 5 (AD-36): the return count is read from the journal inside
/// the apply's transaction: the number of this run's Review items on
/// attempts of the gate step resolved `return`, against the Review item's
/// `max_returns`. With 2, returns 1 and 2 loop back; an apply that claims
/// either is past the maximum is refused; the third return's apply must
/// carry the Failure item (one without it is refused), and then the gate
/// attempt fails, the item is created on it and the run fails. With 0, the
/// first return is past it.
#[test]
fn the_return_count_is_checked_inside_the_transaction() {
    let r = rig_with(Some(2), Taint::Clean);
    let mut item = r.item.clone();
    for n in 1..=2 {
        r.resolve(&item, Decision::Return, Some(format!("note {n}").as_str()))
            .unwrap();
        let before = r.state();
        assert!(
            r.apply(&item, Some(failure_item())).is_err(),
            "return {n} of 2 is within the maximum"
        );
        assert_eq!(r.state(), before);
        r.apply(&item, None).unwrap();
        assert_eq!(r.latest("approve").attempt, n + 1);
        assert_eq!(r.latest("draft").attempt, n + 1);
        item = r.round(Some(2));
    }
    r.resolve(&item, Decision::Return, Some("note 3")).unwrap();
    let before = r.state();
    assert!(
        r.apply(&item, None).is_err(),
        "the third return is past max_returns 2: no loop-back"
    );
    assert_eq!(r.state(), before);
    let failure = failure_item();
    r.apply(&item, Some(failure.clone())).unwrap();
    let gate = r.latest("approve");
    assert_eq!((gate.attempt, gate.status.as_str()), (3, "failed"));
    assert_eq!(r.attempts("draft").len(), 3, "no fourth draft");
    let failures: Vec<InboxItemRow> = r
        .items()
        .into_iter()
        .filter(|i| i.kind == InboxKind::Failure)
        .collect();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert_eq!(failures[0].id, failure.id);
    assert_eq!(failures[0].step_run_id, gate.id);
    assert_eq!(failures[0].status, "open");
    assert_eq!(r.run_row().status, "failed");
    assert!(r.apply(&item, Some(failure_item())).is_err(), "once");

    let r = rig_with(Some(0), Taint::Clean);
    r.resolve(&r.item, Decision::Return, Some("the only note"))
        .unwrap();
    let before = r.state();
    assert!(r.apply(&r.item, None).is_err(), "max_returns 0");
    assert_eq!(r.state(), before);
    r.apply(&r.item, Some(failure_item())).unwrap();
    assert_eq!(r.latest("approve").status, "failed");
    assert_eq!(r.attempts("draft").len(), 1);
    assert_eq!(r.run_row().status, "failed");
}

/// Inserts a Review item by raw SQL (a forged row) on `step_run_id`.
fn forge(
    db: &Path,
    run: &str,
    step_run_id: &str,
    status: &str,
    decision: Option<&str>,
    resolved_by: Option<&Actor>,
) -> String {
    let id = new_id();
    let by = resolved_by.map(|a| serde_json::to_string(a).unwrap());
    let at = resolved_by.map(|_| "2026-11-24T10:00:00.000Z");
    let note = (decision == Some("return")).then_some("a forged note");
    Connection::open(db)
        .unwrap()
        .execute(
            "INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns, decision, note, resolved_by, resolved_at)
             VALUES (?1, 'review', ?2, ?3, ?4, NULL, 0, 'A forged item', 'Forged.', '2026-11-24T09:00:00.000Z', 2, ?5, ?6, ?7, ?8)",
            params![id, status, run, step_run_id, decision, note, by, at],
        )
        .unwrap();
    id
}

/// Row 28 (AD-10 (b)): a `gate_note` input is the apply's own: after a
/// return, the producer's new attempt has exactly one, from the item, with
/// no producing step run, labelled `gate-note:approve@1`, taint 0. The
/// writer refuses any other `gate_note` input (nothing is journaled, the
/// attempt stays queued): one from an item resolved by a non-Human actor,
/// resolved `approve`, still open, of another run, with a producing step
/// run, tainted, or naming no item.
#[test]
fn a_gate_note_input_needs_a_human_return() {
    let r = rig();
    r.resolve(&r.item, Decision::Return, Some(NOTE)).unwrap();
    r.apply(&r.item, None).unwrap();
    let draft = r.latest("draft");
    assert_eq!(
        r.reader().step_inputs(&draft.id).unwrap(),
        vec![StepInput {
            kind: "gate_note".into(),
            from_step_run: None,
            from_item: Some(r.item.clone()),
            from_row: None,
            label: "gate-note:approve@1".into(),
            taint: Taint::Clean,
        }]
    );

    // Forged rows, by raw SQL.
    let first = r.attempts("draft")[0].id.clone();
    let publish = r.latest("publish").id;
    let gate2 = r.latest("approve").id;
    let by_system = forge(
        &r.db,
        &r.run,
        &first,
        "resolved",
        Some("return"),
        Some(&engine()),
    );
    let approved = forge(
        &r.db,
        &r.run,
        &publish,
        "resolved",
        Some("approve"),
        Some(&you_reviewer()),
    );
    let open = forge(&r.db, &r.run, &gate2, "open", None, None);
    // Another run of the same store.
    let other = new_id();
    r.exec(WriteCommand::CreateRun {
        run_id: other.clone(),
        workflow: "wf".into(),
        origin: "manual".into(),
        steps: vec!["draft".into(), "approve".into()],
        config_hash: None,
    })
    .unwrap();
    let other_gate = r.reader().steps(&other).unwrap()[1].id.clone();
    let elsewhere = forge(
        &r.db,
        &other,
        &other_gate,
        "resolved",
        Some("return"),
        Some(&you_reviewer()),
    );
    let note_input = |from_item: Option<&str>| StepInput {
        kind: "gate_note".into(),
        from_step_run: None,
        from_item: from_item.map(str::to_string),
        from_row: None,
        label: "gate-note:approve@1".into(),
        taint: Taint::Clean,
    };
    let forged = [
        (
            "resolved by the engine",
            note_input(Some(by_system.as_str())),
        ),
        ("resolved approve", note_input(Some(approved.as_str()))),
        ("still open", note_input(Some(open.as_str()))),
        ("of another run", note_input(Some(elsewhere.as_str()))),
        ("naming no item", note_input(None)),
        (
            "with a producing step run",
            StepInput {
                from_step_run: Some(first.clone()),
                ..note_input(Some(r.item.as_str()))
            },
        ),
        (
            "tainted",
            StepInput {
                taint: Taint::Tainted,
                ..note_input(Some(r.item.as_str()))
            },
        ),
    ];
    let before = r.state();
    for (what, input) in forged {
        let refused = r.exec(WriteCommand::StartAttempt {
            step_run_id: draft.id.clone(),
            inputs: vec![input],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        });
        assert!(refused.is_err(), "a gate_note input {what}");
        assert_eq!(r.state(), before, "{what}");
        assert_eq!(r.latest("draft").status, "queued", "{what}");
    }

    // Review R13, the positive control: the same command with no forged
    // input starts the attempt, with the apply's own `gate_note` row; the
    // refusals above were the inputs'.
    r.exec(WriteCommand::StartAttempt {
        step_run_id: draft.id.clone(),
        inputs: vec![],
        taint: Taint::Clean,
        prompt: vec![],
        writes: am_store::AttemptWrites {
            seat: "seat-a".into(),
            table: None,
            document: None,
        },
    })
    .expect("the attempt starts with the apply's gate note");
    assert_eq!(r.latest("draft").status, "running");
    let notes: Vec<StepInput> = r
        .reader()
        .step_inputs(&draft.id)
        .unwrap()
        .into_iter()
        .filter(|i| i.kind == "gate_note")
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].from_item.as_deref(), Some(r.item.as_str()));
}

/// Row 22 at the store: no System command changes an open item's status.
/// Every command an engine could send on the run (start, finish or fail the
/// waiting gate, pause it again, apply, finish the run, start the step
/// after the gate, an audit entry naming a resolution) leaves the item open
/// with no decision and the gate waiting.
#[test]
fn no_system_command_changes_an_open_items_status() {
    let r = rig();
    let gate = r.latest("approve").id;
    let publish = r.latest("publish").id;
    let audit_claim = WriteCommand::Audit {
        action: "inbox.resolved".into(),
        target: r.item.clone(),
        outcome: "ok".into(),
        hash: None,
        args: [("decision".to_string(), "approve".to_string())]
            .into_iter()
            .collect(),
    };
    let commands = vec![
        WriteCommand::StartStep {
            step_run_id: gate.clone(),
        },
        WriteCommand::StartAttempt {
            step_run_id: gate.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        },
        WriteCommand::FinishStep {
            step_run_id: gate.clone(),
            status: "succeeded".into(),
            output: None,
        },
        WriteCommand::FinishStep {
            step_run_id: gate.clone(),
            status: "failed".into(),
            output: None,
        },
        WriteCommand::FailWithItem {
            step_run_id: gate.clone(),
            settle: None,
            output: None,
            item: failure_item(),
        },
        WriteCommand::ApplyResolution {
            item_id: r.item.clone(),
            past_max: None,
        },
        WriteCommand::ApplyResolution {
            item_id: r.item.clone(),
            past_max: Some(failure_item()),
        },
        WriteCommand::FinishRun {
            run_id: r.run.clone(),
            status: "succeeded".into(),
        },
        WriteCommand::FinishRun {
            run_id: r.run.clone(),
            status: "failed".into(),
        },
        WriteCommand::StartAttempt {
            step_run_id: publish.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        },
        WriteCommand::StartStep {
            step_run_id: publish,
        },
        audit_claim,
    ];
    for cmd in commands {
        let what = format!("{cmd:?}");
        let _ = r.exec(cmd);
        let item = r.item_row(&r.item);
        assert_eq!(item.status, "open", "{what}");
        assert_eq!(
            (item.decision, item.note, item.resolved_by, item.resolved_at),
            (None, None, None, None),
            "{what}"
        );
        assert_eq!(r.latest("approve").status, "waiting_human", "{what}");
    }
    // Pausing the gate again, as a resumed engine would, changes nothing.
    let _ = r.pause(&new_id(), Some(2));
    assert_eq!(r.item_row(&r.item).status, "open");
    assert_eq!(r.items().len(), 1);
    // `open_items` lists it; once resolved, it is listed no more.
    let open: Vec<String> = r
        .reader()
        .open_items()
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(open, vec![r.item.clone()]);
    r.resolve(&r.item, Decision::Reject, None).unwrap();
    assert!(r.reader().open_items().unwrap().is_empty());
}

/// Row 40 at the store: two resolves of one item from two threads (approve
/// and reject): exactly one commits; the item holds the winner's decision
/// and there is one `inbox.resolved` entry.
#[test]
fn two_resolves_of_one_item_one_wins() {
    let r = rig();
    let (w1, w2) = (r.store.writer(), r.store.writer());
    let (i1, i2) = (r.item.clone(), r.item.clone());
    let a =
        std::thread::spawn(move || w1.resolve_inbox_item(reviewer(), &i1, Decision::Approve, None));
    let b =
        std::thread::spawn(move || w2.resolve_inbox_item(reviewer(), &i2, Decision::Reject, None));
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "one wins: {a:?}, {b:?}"
    );
    let winner = if a.is_ok() {
        Decision::Approve
    } else {
        Decision::Reject
    };
    assert_eq!(r.item_row(&r.item).decision, Some(winner));
    assert_eq!(r.entries("inbox.resolved").len(), 1);
}

/// Row 34 (Question 3): dismissing a Failure resolves the item `dismiss`
/// by You (Reviewer), audited, and changes no step and not the run (it
/// already failed).
#[test]
fn dismiss_changes_nothing_but_the_item() {
    let r = failed();
    let (steps, run) = (r.steps(), r.run_row());
    assert_eq!(run.status, "failed");

    r.resolve(&r.item, Decision::Dismiss, None).unwrap();

    let item = r.item_row(&r.item);
    assert_eq!(item.status, "resolved");
    assert_eq!(item.decision, Some(Decision::Dismiss));
    assert_eq!(item.note, None);
    let by: Actor = serde_json::from_str(item.resolved_by.as_deref().unwrap()).unwrap();
    assert_eq!(by, you_reviewer());
    assert_eq!(r.steps(), steps);
    assert_eq!(r.run_row(), run);
    let resolved = r.entries("inbox.resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["args"]["kind"], "failure");
    assert_eq!(resolved[0]["args"]["decision"], "dismiss");
    assert_eq!(resolved[0]["actor"]["hat"], "Reviewer");
}

/// Row 35: an answer to a Question is stored once, on the item, and takes
/// its step `waiting_human → running` (AD-8's Human edge); nothing else
/// changes (what the engine does with it is its creating story's).
#[test]
fn an_answer_takes_its_step_out_of_waiting() {
    const ANSWER: &str = "use the second source";
    let r = question();
    let step = r.latest("approve");
    assert_eq!(step.status, "waiting_human");
    let (steps, run) = (r.steps(), r.run_row());

    r.resolve(&r.item, Decision::Answer, Some(ANSWER)).unwrap();

    let item = r.item_row(&r.item);
    assert_eq!(item.status, "resolved");
    assert_eq!(item.decision, Some(Decision::Answer));
    assert_eq!(item.note.as_deref(), Some(ANSWER));
    for (b, a) in steps.iter().zip(&r.steps()) {
        if b.id == step.id {
            assert_eq!(a.status, "running");
        } else {
            assert_eq!(a, b);
        }
    }
    assert_eq!(r.steps().len(), steps.len());
    assert_eq!(r.run_row(), run);
    let resolved = r.entries("inbox.resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["args"]["decision"], "answer");
    assert_eq!(resolved[0]["args"]["note_sha256"], sha256_hex(ANSWER));
    assert_eq!(cells_holding(&r.db, ANSWER).len(), 1, "stored once");
}

/// Row 19: `crates/am-store/src/lib.rs` holds the doctests of Design Notes
/// (run by `cargo test --doc -p am-store`): an allowed build that resolves
/// as `HumanActor::new(..)` beside each `compile_fail` refusal (the same
/// call with `Actor::System`; a `HumanActor` made from an `Actor` with
/// `.into()`; `Writer::exec` with `WriteCommand::ResolveInboxItem`), so a
/// missing type cannot make a refusal pass for the wrong reason.
#[test]
fn the_store_doctests_pair_each_refusal_with_an_allowed_build() {
    let src =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    // The doc-comment code blocks: (fence info, code).
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in src.lines() {
        let trimmed = line.trim_start();
        let Some(doc) = trimmed
            .strip_prefix("///")
            .or_else(|| trimmed.strip_prefix("//!"))
        else {
            continue;
        };
        let doc = doc.strip_prefix(' ').unwrap_or(doc);
        if let Some(info) = doc.strip_prefix("```") {
            match open.take() {
                Some(block) => blocks.push(block),
                None => open = Some((info.trim().to_string(), String::new())),
            }
        } else if let Some((_, code)) = open.as_mut() {
            code.push_str(doc);
            code.push('\n');
        }
    }
    let refused: Vec<&String> = blocks
        .iter()
        .filter(|(info, _)| info.contains("compile_fail"))
        .map(|(_, code)| code)
        .collect();
    for (what, needles) in [
        (
            "a resolve by a System actor",
            &["resolve_inbox_item(", "Actor::System"][..],
        ),
        (
            "a HumanActor made from an Actor",
            &["HumanActor", ".into()"][..],
        ),
        (
            "a resolve through Writer::exec",
            &["exec(", "WriteCommand::ResolveInboxItem"][..],
        ),
    ] {
        assert!(
            refused
                .iter()
                .any(|code| needles.iter().all(|n| code.contains(n))),
            "a compile_fail doctest refusing {what} (naming {needles:?}) in am-store's lib.rs; found {refused:#?}"
        );
    }
    let allowed = blocks.iter().any(|(info, code)| {
        !info.contains("compile_fail")
            && !info.contains("ignore")
            && code.contains("resolve_inbox_item(")
            && code.contains("HumanActor::new(")
            && code.contains("Decision::")
    });
    assert!(
        allowed,
        "a doctest that resolves as HumanActor::new(..), beside the compile_fail ones; found {blocks:#?}"
    );
}

// ------------------------------------------------------- review round 1
//
// Story 3.4 review round 1 (the coordinator's triage, 2026-11-24): R3 (the
// apply reads the committed resolver and refuses anyone but a person; its
// `gate_note` row goes through the input checks), R4 (`INSERT OR REPLACE`
// never rewrites a resolved item) and R7 (a note holding an invisible or
// line-separator character is refused). Written before the fixes.

/// The item `r.item` resolved `decision` (with `note`) by `by`, written by
/// raw SQL as a forger would (no `inbox.resolved` entry), and its gate
/// `running` as after a resolve.
fn resolve_by_raw_sql(r: &Rig, decision: Decision, note: Option<&str>, by: &Actor) {
    let gate = r.latest("approve").id;
    let c = Connection::open(&r.db).unwrap();
    c.execute(
        "UPDATE inbox_items SET status = 'resolved', decision = ?2, note = ?3, resolved_by = ?4, resolved_at = '2026-11-24T10:00:00.000Z' WHERE id = ?1",
        params![
            r.item,
            decision.as_str(),
            note,
            serde_json::to_string(by).unwrap()
        ],
    )
    .unwrap();
    c.execute(
        "UPDATE step_runs SET status = 'running' WHERE id = ?1",
        params![gate],
    )
    .unwrap();
}

/// Every `gate_note` row of the run.
fn gate_notes(r: &Rig) -> Vec<StepInput> {
    let reader = r.reader();
    r.steps()
        .iter()
        .flat_map(|s| reader.step_inputs(&s.id).unwrap())
        .filter(|i| i.kind == "gate_note")
        .collect()
}

/// R3 (AD-15, AD-10 (b)): the apply reads the item's committed
/// `resolved_by` and refuses it unless it is a person: on an item resolved
/// (by raw SQL) `approve` or `return` by `System`, a `Seat` or the
/// `Assistant`, the apply is refused and nothing changes; a forged return
/// journals no `gate_note` row. The positive control: the same raw setup
/// resolved by the owner as Reviewer applies (approve: the gate succeeds;
/// return: one `gate_note` row, through the input checks).
#[test]
fn apply_needs_a_person_as_the_committed_resolver() {
    let id = Uuid::now_v7();
    let forgers = [
        engine(),
        Actor::Seat {
            seat: "seat-a".into(),
            run: id,
            step: "approve".into(),
        },
        Actor::Assistant { proposal: id },
    ];
    for (decision, note) in [(Decision::Approve, None), (Decision::Return, Some(NOTE))] {
        for forger in &forgers {
            let r = rig();
            resolve_by_raw_sql(&r, decision, note, forger);
            let before = r.state();

            let refused = r.apply(&r.item, None);

            assert!(refused.is_err(), "{decision:?} resolved by {forger:?}");
            assert_eq!(r.state(), before, "{decision:?} by {forger:?}");
            assert!(gate_notes(&r).is_empty(), "{decision:?} by {forger:?}");
            assert_eq!(r.latest("approve").status, "running");
        }

        let r = rig();
        resolve_by_raw_sql(&r, decision, note, &you_reviewer());
        r.apply(&r.item, None)
            .unwrap_or_else(|e| panic!("{decision:?} by a person: {e}"));
        if decision == Decision::Approve {
            assert_eq!(r.attempts("approve")[0].status, "succeeded");
            assert!(gate_notes(&r).is_empty());
        } else {
            assert_eq!(r.attempts("approve")[0].status, "looped_back");
            let notes = gate_notes(&r);
            assert_eq!(notes.len(), 1, "{notes:#?}");
            assert_eq!(notes[0].from_item.as_deref(), Some(r.item.as_str()));
        }
    }
}

/// R4: `INSERT OR REPLACE` (and `REPLACE`) over a resolved item, by its id
/// or by its `(step_run_id, kind)`, is aborted by a trigger (SQLite fires
/// no delete trigger on a REPLACE); the row stays as it was. A plain
/// INSERT of a new open item, in nothing's way, still works.
#[test]
fn insert_or_replace_never_rewrites_a_resolved_item() {
    let r = rig();
    r.resolve(&r.item, Decision::Approve, None).unwrap();
    let gate = r.latest("approve").id;
    let publish = r.latest("publish").id;
    let row = r.item_row(&r.item);
    let before = r.state();
    let c = Connection::open(&r.db).unwrap();
    let columns = "id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns, decision, note, resolved_by, resolved_at";
    let other = new_id();
    for (what, verb, id) in [
        (
            "INSERT OR REPLACE by its id",
            "INSERT OR REPLACE",
            r.item.as_str(),
        ),
        ("REPLACE by its id", "REPLACE", r.item.as_str()),
        (
            "INSERT OR REPLACE by its step run and kind",
            "INSERT OR REPLACE",
            other.as_str(),
        ),
        (
            "REPLACE by its step run and kind",
            "REPLACE",
            other.as_str(),
        ),
    ] {
        let replaced = c.execute(
            &format!(
                "{verb} INTO inbox_items ({columns}) VALUES (?1, 'review', 'open', ?2, ?3, NULL, 0, 'Replaced', 'Replaced.', '2026-11-24T11:00:00.000Z', 2, NULL, NULL, NULL, NULL)"
            ),
            params![id, r.run, gate],
        );
        assert!(replaced.is_err(), "a trigger aborts {what}");
        assert_eq!(r.item_row(&r.item), row, "{what}");
    }
    assert_eq!(r.state(), before);

    let fresh = new_id();
    c.execute(
        &format!(
            "INSERT INTO inbox_items ({columns}) VALUES (?1, 'question', 'open', ?2, ?3, NULL, 0, 'A question', 'Asked.', '2026-11-24T11:00:00.000Z', NULL, NULL, NULL, NULL, NULL)"
        ),
        params![fresh, r.run, publish],
    )
    .expect("a new open item inserts");
    drop(c);
    assert_eq!(r.item_row(&fresh).status, "open");
    assert_eq!(r.item_row(&r.item), row);
}

/// R7: a note (a return's or an answer's) holding a format, line- or
/// paragraph-separator or tag character, which a terminal or the next
/// step's prompt would not show as itself (U+2028, U+2029, U+202E, U+2066,
/// U+200B, U+FEFF, U+E0041), is refused, and nothing is written.
#[test]
fn a_note_with_an_invisible_or_separator_character_is_refused() {
    for (what, note) in [
        ("U+2028 LINE SEPARATOR", "one\u{2028}two"),
        ("U+2029 PARAGRAPH SEPARATOR", "one\u{2029}two"),
        ("U+202E RIGHT-TO-LEFT OVERRIDE", "abc\u{202E}fed"),
        ("U+2066 LEFT-TO-RIGHT ISOLATE", "a\u{2066}b"),
        ("U+200B ZERO WIDTH SPACE", "a\u{200B}b"),
        ("U+FEFF ZERO WIDTH NO-BREAK SPACE", "\u{FEFF}note"),
        ("U+E0041 TAG LATIN CAPITAL LETTER A", "a\u{E0041}b"),
    ] {
        for (decision, r) in [(Decision::Return, rig()), (Decision::Answer, question())] {
            let before = r.state();
            assert!(
                r.resolve(&r.item, decision, Some(note)).is_err(),
                "{decision:?} with {what}"
            );
            assert_eq!(r.state(), before, "{decision:?} with {what}");
            assert!(cells_holding(&r.db, note).is_empty(), "{what}: not stored");
        }
    }
}

// ------------------------------------------------------- review round 2
//
// Story 3.4 review round 2 (the coordinator's triage, 2026-11-24): S2 (the
// REPLACE guard holds over an open row too) and S4 (the hidden table takes
// in Default_Ignorable_Code_Point). Written before the fixes.

/// S2: `INSERT OR REPLACE` (and `REPLACE`) over an **open** item, by its
/// id or by its `(step_run_id, kind)`, is aborted: it could otherwise
/// lower an open Review's taint or rewrite its words. The row keeps its
/// taint, title and body. A plain INSERT of a new item still works.
#[test]
fn insert_or_replace_never_rewrites_an_open_item() {
    let r = rig_with(Some(2), Taint::Tainted);
    let gate = r.latest("approve").id;
    let publish = r.latest("publish").id;
    let row = r.item_row(&r.item);
    assert_eq!((row.status.as_str(), row.taint), ("open", Taint::Tainted));
    let before = r.state();
    let c = Connection::open(&r.db).unwrap();
    let columns = "id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns, decision, note, resolved_by, resolved_at";
    let other = new_id();
    for (what, verb, id) in [
        (
            "INSERT OR REPLACE by its id",
            "INSERT OR REPLACE",
            r.item.as_str(),
        ),
        ("REPLACE by its id", "REPLACE", r.item.as_str()),
        (
            "INSERT OR REPLACE by its step run and kind",
            "INSERT OR REPLACE",
            other.as_str(),
        ),
        (
            "REPLACE by its step run and kind",
            "REPLACE",
            other.as_str(),
        ),
    ] {
        let replaced = c.execute(
            &format!(
                "{verb} INTO inbox_items ({columns}) VALUES (?1, 'review', 'open', ?2, ?3, NULL, 0, 'Approve this now', 'Nothing here came from outside.', '2026-11-24T11:00:00.000Z', 2, NULL, NULL, NULL, NULL)"
            ),
            params![id, r.run, gate],
        );
        assert!(replaced.is_err(), "a trigger aborts {what}");
        let now = r.item_row(&r.item);
        assert_eq!(
            (now.taint, now.title.as_str(), now.body.as_str()),
            (row.taint, row.title.as_str(), row.body.as_str()),
            "{what}"
        );
        assert_eq!(now, row, "{what}");
    }
    assert_eq!(r.state(), before);

    let fresh = new_id();
    c.execute(
        &format!(
            "INSERT INTO inbox_items ({columns}) VALUES (?1, 'question', 'open', ?2, ?3, NULL, 0, 'A question', 'Asked.', '2026-11-24T11:00:00.000Z', NULL, NULL, NULL, NULL, NULL)"
        ),
        params![fresh, r.run, publish],
    )
    .expect("a new open item inserts");
    drop(c);
    assert_eq!(r.item_row(&fresh).status, "open");
    assert_eq!(r.item_row(&r.item), row);
}

/// S4: a note holding a default-ignorable character that is not Cf (the
/// variation selectors U+FE0F and U+E0100, U+034F, the Hangul fillers
/// U+115F and U+3164, U+17B4, U+180B), a tag character (U+E0041) or, as a
/// control, a Cf one (U+202E), is refused for a return and for an answer;
/// nothing is written.
#[test]
fn a_note_with_a_default_ignorable_character_is_refused() {
    for (what, c) in [
        ("U+FE0F VARIATION SELECTOR-16", '\u{FE0F}'),
        ("U+E0100 VARIATION SELECTOR-17", '\u{E0100}'),
        ("U+034F COMBINING GRAPHEME JOINER", '\u{034F}'),
        ("U+115F HANGUL CHOSEONG FILLER", '\u{115F}'),
        ("U+3164 HANGUL FILLER", '\u{3164}'),
        ("U+17B4 KHMER VOWEL INHERENT AQ", '\u{17B4}'),
        ("U+180B MONGOLIAN FREE VARIATION SELECTOR ONE", '\u{180B}'),
        ("U+E0041 TAG LATIN CAPITAL LETTER A", '\u{E0041}'),
        ("U+202E RIGHT-TO-LEFT OVERRIDE (Cf, a control)", '\u{202E}'),
    ] {
        let note = format!("keep{c}this");
        for (decision, r) in [(Decision::Return, rig()), (Decision::Answer, question())] {
            let before = r.state();
            assert!(
                r.resolve(&r.item, decision, Some(note.as_str())).is_err(),
                "{decision:?} with {what}"
            );
            assert_eq!(r.state(), before, "{decision:?} with {what}");
            assert!(cells_holding(&r.db, &note).is_empty(), "{what}: not stored");
        }
    }
}

// ------------------------------------- post-merge fix, 2026-11-25
//
// The second-resolve race (story 3.4; owner, 2026-11-25): under any
// interleaving of two people's resolves of one item, exactly one
// resolution is recorded and applied. At the store, every write goes
// through the one writer thread, so two handlers' writes reach it in one
// of the orders below; together they are every order.

/// One step of a side of the race: record its decision, or apply what
/// committed (as the server's handler does after its own resolve).
#[derive(Debug, Clone, Copy)]
enum Step {
    Resolve(usize),
    Apply(usize),
}

/// The six orders of two sides' resolve and apply, each side's resolve
/// before its own apply.
const ORDERS: [[Step; 4]; 6] = [
    [
        Step::Resolve(0),
        Step::Apply(0),
        Step::Resolve(1),
        Step::Apply(1),
    ],
    [
        Step::Resolve(0),
        Step::Resolve(1),
        Step::Apply(0),
        Step::Apply(1),
    ],
    [
        Step::Resolve(0),
        Step::Resolve(1),
        Step::Apply(1),
        Step::Apply(0),
    ],
    [
        Step::Resolve(1),
        Step::Resolve(0),
        Step::Apply(0),
        Step::Apply(1),
    ],
    [
        Step::Resolve(1),
        Step::Resolve(0),
        Step::Apply(1),
        Step::Apply(0),
    ],
    [
        Step::Resolve(1),
        Step::Apply(1),
        Step::Resolve(0),
        Step::Apply(0),
    ],
];

/// Every pair of a Review's decisions (approve vs reject, approve vs
/// return and approve vs approve among them); each order above puts
/// either side first.
const PAIRS: [(Decision, Decision); 6] = [
    (Decision::Approve, Decision::Reject),
    (Decision::Approve, Decision::Return),
    (Decision::Approve, Decision::Approve),
    (Decision::Return, Decision::Reject),
    (Decision::Return, Decision::Return),
    (Decision::Reject, Decision::Reject),
];

fn note_for(decision: Decision) -> Option<&'static str> {
    (decision == Decision::Return).then_some(NOTE)
}

/// Owner, 2026-11-25: two resolves of one item, in every order of their
/// writes and with every pair of decisions. Each side applies after its
/// own resolve, and tries even when its resolve lost (a handler that
/// applied after losing could not apply twice either). Exactly one
/// resolve commits; one `inbox.resolved` entry, the winner's; the item
/// holds the winner's decision; exactly one apply commits and the gate
/// attempt is moved once (one `step.finished` or `step.looped_back` entry
/// on it) to what the winner's decision gives; no other item.
#[test]
fn two_resolves_in_any_order_record_and_apply_one_decision() {
    for (a, b) in PAIRS {
        let decisions = [a, b];
        for order in ORDERS {
            let case = format!("{} vs {}, {order:?}", a.as_str(), b.as_str());
            let r = rig();
            let gate = r.latest("approve").id;
            let mut resolved: [Option<Result<(), StoreError>>; 2] = [None, None];
            let mut applied: [Option<Result<(), StoreError>>; 2] = [None, None];
            for step in order {
                match step {
                    Step::Resolve(i) => {
                        resolved[i] =
                            Some(r.resolve(&r.item, decisions[i], note_for(decisions[i])));
                    }
                    Step::Apply(i) => applied[i] = Some(r.apply(&r.item, None)),
                }
            }

            let won: Vec<usize> = (0..2)
                .filter(|i| matches!(resolved[*i], Some(Ok(()))))
                .collect();
            assert_eq!(won.len(), 1, "one resolve commits ({case}): {resolved:?}");
            let winner = decisions[won[0]];
            let item = r.item_row(&r.item);
            assert_eq!(item.status, "resolved", "{case}");
            assert_eq!(item.decision, Some(winner), "{case}");
            assert_eq!(item.note.as_deref(), note_for(winner), "{case}");
            let entries = r.entries("inbox.resolved");
            assert_eq!(
                entries.len(),
                1,
                "one inbox.resolved ({case}): {entries:#?}"
            );
            assert_eq!(entries[0]["args"]["decision"], winner.as_str(), "{case}");
            let applies = applied.iter().filter(|a| matches!(a, Some(Ok(())))).count();
            assert_eq!(applies, 1, "one apply commits ({case}): {applied:?}");
            let moves: Vec<Value> = r
                .audit()
                .into_iter()
                .filter(|e| {
                    e["target"] == gate.as_str()
                        && (e["action"] == "step.finished" || e["action"] == "step.looped_back")
                })
                .collect();
            assert_eq!(moves.len(), 1, "the gate moved once ({case}): {moves:#?}");
            let shape: Vec<(String, i64, String)> = r
                .steps()
                .into_iter()
                .map(|s| (s.step_key, s.attempt, s.status))
                .collect();
            let want: &[(&str, i64, &str)] = match winner {
                Decision::Approve => &[
                    ("draft", 1, "succeeded"),
                    ("approve", 1, "succeeded"),
                    ("publish", 1, "queued"),
                ],
                Decision::Reject => &[
                    ("draft", 1, "succeeded"),
                    ("approve", 1, "failed"),
                    ("publish", 1, "queued"),
                ],
                _ => &[
                    ("draft", 1, "succeeded"),
                    ("draft", 2, "queued"),
                    ("approve", 1, "looped_back"),
                    ("approve", 2, "queued"),
                    ("publish", 1, "queued"),
                ],
            };
            let want: Vec<(String, i64, String)> = want
                .iter()
                .map(|(k, n, s)| (k.to_string(), *n, s.to_string()))
                .collect();
            assert_eq!(shape, want, "{case}");
            let run_status = if winner == Decision::Reject {
                "failed"
            } else {
                "running"
            };
            assert_eq!(r.run_row().status, run_status, "{case}");
            assert_eq!(r.items().len(), 1, "no other item ({case})");
            if winner == Decision::Return {
                assert_eq!(gate_notes(&r).len(), 1, "one gate note ({case})");
            }
        }
    }
}
