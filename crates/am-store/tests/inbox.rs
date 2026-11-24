//! Epic 3 story 3 at the store (AD-8, AD-15, AD-24, AD-25, AD-28): the one
//! `inbox_items` table and the step state machine the writer enforces.
//!
//! - An Inbox item is created only by a writer command whose actor is
//!   `Actor::System`, together with the step change it belongs to: a pause
//!   (`queued`/`running` → `waiting_human`, `WriteCommand::PauseStep`) or a
//!   failure that ends the step and its run (`WriteCommand::FailWithItem`).
//!   Any other actor is refused in words and nothing is written.
//! - Its kind is one of AD-28's seven (a CHECK constraint); items are
//!   insert-only, one per (step run, kind): a second create changes nothing.
//! - Nothing leaves `waiting_human` in this story: starting, settling,
//!   finishing, looping back or failing a waiting step is refused in words
//!   and changes nothing (Human resolve is story 3.4, cancel 3.11).
//! - Each creation is audited `inbox.created` (kind, item id, step run,
//!   taint), never with the item's words.
//!
//! No command resolves an item: `WriteCommand::ResolveInboxItem` does not
//! exist until story 3.4 (the implementer adds the `compile_fail` doctest on
//! `Writer::exec` that says so). Written before the table and the commands
//! exist.
#![forbid(unsafe_code)]

use am_domain::{Actor, InboxKind, Taint, Usage};
use am_store::{
    CallSettlement, InboxItemRow, NewInboxItem, Reader, StepInput, StepRow, Store, StoreError,
    WriteCommand, new_id,
};
use rusqlite::Connection;
use serde_json::Value;
use uuid::Uuid;

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// Every actor but the app itself (AD-24).
fn not_the_engine() -> Vec<Actor> {
    let run = Uuid::now_v7();
    vec![
        Actor::Human {
            person: "owner".into(),
            hat: "Owner".into(),
        },
        Actor::Seat {
            seat: "seat-a".into(),
            run,
            step: "approve".into(),
        },
        Actor::Assistant { proposal: run },
        Actor::Import {
            source: "an import".into(),
        },
    ]
}

/// A store with a run of `draft`, `approve` and `publish`: `draft` ran and
/// succeeded; `approve` and `publish` are queued.
struct Rig {
    _dir: tempfile::TempDir,
    db: std::path::PathBuf,
    store: Store,
    run: String,
    draft: String,
    approve: String,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    let w = store.writer();
    let run = new_id();
    w.exec(
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
    let ids: Vec<String> = store
        .reader()
        .unwrap()
        .steps(&run)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: ids[0].clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    w.exec(
        engine(),
        WriteCommand::ReserveCall {
            step_run_id: ids[0].clone(),
            call_id: "call-1".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(
        engine(),
        WriteCommand::SettleAndFinishStep {
            call_id: "call-1".into(),
            usage: None,
            cost_micro_usd: None,
            step_run_id: ids[0].clone(),
            status: "succeeded".into(),
            output: Some("the version to approve".into()),
        },
    )
    .unwrap();
    Rig {
        _dir: dir,
        db,
        store,
        run,
        draft: ids[0].clone(),
        approve: ids[1].clone(),
    }
}

impl Rig {
    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    fn steps(&self) -> Vec<StepRow> {
        self.reader().steps(&self.run).unwrap()
    }

    fn step(&self, id: &str) -> StepRow {
        self.steps().into_iter().find(|s| s.id == id).unwrap()
    }

    fn items(&self) -> Vec<InboxItemRow> {
        self.reader().inbox_items(&self.run).unwrap()
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

    fn created(&self) -> Vec<Value> {
        self.audit()
            .into_iter()
            .filter(|e| e["action"] == "inbox.created")
            .collect()
    }

    /// The pause of the gate `approve` with its Review item, whose payload
    /// is `draft`'s attempt.
    fn pause(&self, item_id: &str) -> WriteCommand {
        WriteCommand::PauseStep {
            step_run_id: self.approve.clone(),
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(self.draft.clone()),
                from_item: None,
                label: "step:draft@1".into(),
                taint: Taint::Clean,
            }],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: item_id.into(),
                kind: InboxKind::Review,
                payload: Some(self.draft.clone()),
                taint: Taint::Clean,
                title: "Review the version".into(),
                body: "Step approve waits for your review of step draft's output.".into(),
                max_returns: Some(2),
            },
        }
    }

    /// Starts `approve` with a reserved, unsettled call `call`.
    fn start_with_call(&self, call: &str) {
        let w = self.store.writer();
        w.exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: self.approve.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
                writes: am_store::AttemptWrites {
                    seat: "seat-a".into(),
                    table: None,
                    document: None,
                },
            },
        )
        .unwrap();
        w.exec(
            engine(),
            WriteCommand::ReserveCall {
                step_run_id: self.approve.clone(),
                call_id: call.into(),
                call_seq: 1,
                reserved_micro_usd: 0,
                rate_snapshot: "none".into(),
                taint: Taint::Clean,
                purpose: am_store::CallPurpose::Turn,
                prompt: vec![],
            },
        )
        .unwrap();
    }

    fn fail(&self, item_id: &str, call: Option<&str>) -> WriteCommand {
        WriteCommand::FailWithItem {
            step_run_id: self.approve.clone(),
            settle: call.map(|c| CallSettlement {
                call_id: c.into(),
                usage: Some(Usage {
                    input_tokens: 5,
                    output_tokens: 1,
                }),
                cost_micro_usd: None,
            }),
            output: None,
            item: NewInboxItem {
                id: item_id.into(),
                kind: InboxKind::Failure,
                payload: Some(self.approve.clone()),
                taint: Taint::Clean,
                title: "A step stopped".into(),
                body: "Step approve stopped at its limit of 3 iterations.".into(),
                max_returns: None,
            },
        }
    }
}

/// Row 13: the item-creating commands as `Human`, `Seat`, `Assistant` or
/// `Import` are refused in words naming the actor, and write nothing: no
/// item, no step change, no audit entry. As `System(engine)` they apply.
#[test]
fn only_a_system_actor_creates_an_item() {
    let r = rig();
    let before = (r.steps(), r.audit().len());
    for actor in not_the_engine() {
        let refused = r
            .store
            .writer()
            .exec(actor.clone(), r.pause(&new_id()))
            .expect_err("only the engine creates an Inbox item");
        assert!(
            refused.to_string().contains(&actor.describe()),
            "names who asked: {refused}"
        );
        assert!(r.items().is_empty(), "{actor:?}");
        assert_eq!((r.steps(), r.audit().len()), before, "{actor:?}");
    }
    r.store.writer().exec(engine(), r.pause(&new_id())).unwrap();
    assert_eq!(r.items().len(), 1);

    // A failure with its item, the same way.
    let r = rig();
    r.start_with_call("call-2");
    let before = (r.steps(), r.audit().len());
    for actor in not_the_engine() {
        let refused = r
            .store
            .writer()
            .exec(actor.clone(), r.fail(&new_id(), Some("call-2")))
            .expect_err("only the engine creates an Inbox item");
        assert!(refused.to_string().contains(&actor.describe()), "{refused}");
        assert_eq!((r.steps(), r.audit().len()), before, "{actor:?}");
        assert!(r.items().is_empty());
    }
    r.store
        .writer()
        .exec(engine(), r.fail(&new_id(), Some("call-2")))
        .unwrap();
    assert_eq!(r.step(&r.approve).status, "failed");
    assert_eq!(r.items().len(), 1);
    assert_eq!(r.items()[0].kind, InboxKind::Failure);
    // The failure ends the run with it (AD-36).
    let run = r.reader().run(&r.run).unwrap().unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.finished_at.is_some());
}

/// Question 9 (a): an item's kind is one of AD-28's seven, by a CHECK
/// constraint. A copy of a valid item under each other of the seven kinds
/// inserts; under any other kind (EXPERIENCE.md's informational ones
/// included) the database refuses it.
#[test]
fn an_item_kind_is_one_of_ad_28s_seven() {
    assert_eq!(
        InboxKind::ALL.map(InboxKind::as_str),
        [
            "review",
            "approval",
            "draft",
            "question",
            "budget_ask",
            "failure",
            "proposal"
        ]
    );
    let r = rig();
    let id = new_id();
    r.store.writer().exec(engine(), r.pause(&id)).unwrap();
    let c = Connection::open(&r.db).unwrap();
    let cols: Vec<String> = c
        .prepare("SELECT name FROM pragma_table_info('inbox_items')")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(cols.contains(&"id".to_string()) && cols.contains(&"kind".to_string()));
    let copy_as = |kind: &str| {
        let picked: Vec<String> = cols
            .iter()
            .map(|col| match col.as_str() {
                "id" => format!("'{}'", new_id()),
                "kind" => format!("'{kind}'"),
                other => other.to_string(),
            })
            .collect();
        c.execute(
            &format!(
                "INSERT INTO inbox_items ({}) SELECT {} FROM inbox_items WHERE id = ?1",
                cols.join(", "),
                picked.join(", ")
            ),
            [&id],
        )
    };
    for kind in InboxKind::ALL.iter().skip(1) {
        copy_as(kind.as_str()).unwrap_or_else(|e| panic!("{}: {e}", kind.as_str()));
    }
    for kind in ["reapproval", "returned", "Review", "note", ""] {
        let refused = copy_as(kind).expect_err(kind);
        assert!(
            refused.to_string().contains("CHECK"),
            "{kind}: refused by the CHECK constraint: {refused}"
        );
    }
}

/// Row 12: the pause sent twice for one gate attempt (with the same item id,
/// then with a new one, as a resumed engine would mint) leaves one item,
/// unchanged, and one `inbox.created` entry; the step stays
/// `waiting_human`. The same for a failure sent twice.
#[test]
fn creating_an_item_twice_changes_nothing() {
    let r = rig();
    let id = new_id();
    r.store.writer().exec(engine(), r.pause(&id)).unwrap();
    let (items, steps) = (r.items(), r.steps());
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.id, id);
    assert_eq!(item.kind, InboxKind::Review);
    assert_eq!(item.status, "open");
    assert_eq!(item.run_id, r.run);
    assert_eq!(item.step_run_id, r.approve);
    assert_eq!(item.payload.as_deref(), Some(r.draft.as_str()));
    assert_eq!(item.taint, Taint::Clean);
    assert_eq!(item.title, "Review the version");
    assert!(item.created_at.ends_with('Z'), "{}", item.created_at);
    assert_eq!(
        (item.resolved_at.clone(), item.resolved_by.clone()),
        (None, None)
    );
    assert_eq!(r.step(&r.approve).status, "waiting_human");
    // Its input row is journaled with the pause.
    assert_eq!(
        r.reader().step_inputs(&r.approve).unwrap(),
        vec![StepInput {
            kind: "step_output".into(),
            from_step_run: Some(r.draft.clone()),
            from_item: None,
            label: "step:draft@1".into(),
            taint: Taint::Clean,
        }]
    );
    // Audited without its words.
    let created = r.created();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["target"], id.as_str());
    assert_eq!(created[0]["args"]["kind"], "review");
    assert_eq!(created[0]["args"]["step_run_id"], r.approve.as_str());
    assert!(created[0]["args"].get("taint").is_some(), "{}", created[0]);
    assert!(!created[0].to_string().contains("waits for your review"));

    for again in [id.clone(), new_id()] {
        let _ = r.store.writer().exec(engine(), r.pause(&again));
        assert_eq!(r.items(), items, "one item, unchanged");
        assert_eq!(r.steps(), steps);
        assert_eq!(r.created().len(), 1);
    }

    let r = rig();
    r.start_with_call("call-2");
    let first = new_id();
    r.store
        .writer()
        .exec(engine(), r.fail(&first, Some("call-2")))
        .unwrap();
    let (items, steps) = (r.items(), r.steps());
    for again in [first, new_id()] {
        let _ = r.store.writer().exec(engine(), r.fail(&again, None));
        assert_eq!(r.items(), items);
        assert_eq!(r.steps(), steps);
        assert_eq!(r.created().len(), 1);
    }
}

/// The pause and its item commit together or not at all: a pause whose
/// item names a payload that is no step run of the run is refused, and the
/// step stays `queued` with no item, no input row and no audit entry. A
/// failure whose item cannot be written leaves the step `running`, its call
/// open and the run `running`.
#[test]
fn the_pause_and_its_item_commit_together_or_not_at_all() {
    let r = rig();
    let before = (r.steps(), r.audit().len());
    let mut bad = r.pause(&new_id());
    if let WriteCommand::PauseStep { item, .. } = &mut bad {
        item.payload = Some("no-such-step-run".into());
    }
    let refused = r.store.writer().exec(engine(), bad);
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(r.step(&r.approve).status, "queued");
    assert!(r.items().is_empty());
    assert!(r.reader().step_inputs(&r.approve).unwrap().is_empty());
    assert_eq!((r.steps(), r.audit().len()), before);

    let r = rig();
    r.start_with_call("call-2");
    let before = (r.steps(), r.audit().len());
    let mut bad = r.fail(&new_id(), Some("call-2"));
    if let WriteCommand::FailWithItem { item, .. } = &mut bad {
        item.payload = Some("no-such-step-run".into());
    }
    assert!(r.store.writer().exec(engine(), bad).is_err());
    assert_eq!(r.step(&r.approve).status, "running");
    assert!(r.items().is_empty());
    let calls = r.reader().calls_for_run(&r.run).unwrap();
    assert!(!calls.iter().find(|c| c.id == "call-2").unwrap().settled);
    assert_eq!(r.reader().run(&r.run).unwrap().unwrap().status, "running");
    assert_eq!((r.steps(), r.audit().len()), before);
}

/// Row 14: nothing leaves `waiting_human` in this story. Starting the
/// waiting step (either command) or finishing it is refused in words naming
/// `waiting_human`; settling a call with its finish, looping it back or
/// failing it is refused too; none changes anything: the step waits, the
/// item is open, the open call stays open (the transaction rolled back) and
/// the run stays `running`.
#[test]
fn nothing_leaves_waiting_human() {
    let r = rig();
    // An open call elsewhere in the run, for the settling commands. It is
    // made before the gate waits (review round 2, R4: nothing starts while
    // another step of the run waits).
    let w = r.store.writer();
    let publish = r.steps()[2].id.clone();
    w.exec(
        engine(),
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
    )
    .unwrap();
    w.exec(
        engine(),
        WriteCommand::ReserveCall {
            step_run_id: publish,
            call_id: "call-open".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(engine(), r.pause(&new_id())).unwrap();
    let before = (r.steps(), r.items(), r.audit().len());

    let waiting = r.approve.clone();
    let commands: Vec<(&str, WriteCommand)> = vec![
        (
            "StartStep",
            WriteCommand::StartStep {
                step_run_id: waiting.clone(),
            },
        ),
        (
            "StartAttempt",
            WriteCommand::StartAttempt {
                step_run_id: waiting.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
                writes: am_store::AttemptWrites {
                    seat: "seat-a".into(),
                    table: None,
                    document: None,
                },
            },
        ),
        (
            "FinishStep succeeded",
            WriteCommand::FinishStep {
                step_run_id: waiting.clone(),
                status: "succeeded".into(),
                output: Some("approved".into()),
            },
        ),
        (
            "FinishStep failed",
            WriteCommand::FinishStep {
                step_run_id: waiting.clone(),
                status: "failed".into(),
                output: None,
            },
        ),
        (
            "SettleAndFinishStep",
            WriteCommand::SettleAndFinishStep {
                call_id: "call-open".into(),
                usage: None,
                cost_micro_usd: None,
                step_run_id: waiting.clone(),
                status: "succeeded".into(),
                output: Some("approved".into()),
            },
        ),
        (
            "SettleAndLoopBack",
            WriteCommand::SettleAndLoopBack {
                call_id: "call-open".into(),
                usage: None,
                cost_micro_usd: None,
                step_run_id: waiting.clone(),
                output: r#"{"reasons":["again"]}"#.into(),
                to: "draft".into(),
            },
        ),
        ("FailWithItem", r.fail(&new_id(), Some("call-open"))),
    ];
    for (what, cmd) in commands {
        let settles = matches!(
            cmd,
            WriteCommand::SettleAndFinishStep { .. }
                | WriteCommand::SettleAndLoopBack { .. }
                | WriteCommand::FailWithItem { .. }
        );
        let refused: StoreError = w.exec(engine(), cmd).expect_err(what);
        // The settling commands name another step's call, so they may be
        // refused for that first; the rest are refused for the state.
        assert!(
            settles || refused.to_string().contains("waiting_human"),
            "{what}: in words naming the state: {refused}"
        );
        assert_eq!((r.steps(), r.items(), r.audit().len()), before, "{what}");
        let calls = r.reader().calls_for_run(&r.run).unwrap();
        assert!(
            !calls.iter().find(|c| c.id == "call-open").unwrap().settled,
            "{what}: the open call stays open"
        );
    }
    assert_eq!(r.step(&waiting).status, "waiting_human");
    assert_eq!(r.items()[0].status, "open");
    assert_eq!(r.reader().run(&r.run).unwrap().unwrap().status, "running");
}

/// A queued or finished step is not started twice, and a finished one is
/// not finished again: the writer's transition table (AD-8) refuses in
/// words and changes nothing.
#[test]
fn the_writer_refuses_an_edge_ad_8_does_not_have() {
    let r = rig();
    let before = (r.steps(), r.audit().len());
    let w = r.store.writer();
    // `draft` has succeeded.
    for cmd in [
        WriteCommand::StartStep {
            step_run_id: r.draft.clone(),
        },
        WriteCommand::StartAttempt {
            step_run_id: r.draft.clone(),
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
            step_run_id: r.draft.clone(),
            status: "failed".into(),
            output: None,
        },
    ] {
        assert!(w.exec(engine(), cmd).is_err());
    }
    // `approve` is queued: it does not finish without starting.
    assert!(
        w.exec(
            engine(),
            WriteCommand::FinishStep {
                step_run_id: r.approve.clone(),
                status: "succeeded".into(),
                output: Some("x".into()),
            },
        )
        .is_err()
    );
    assert_eq!((r.steps(), r.audit().len()), before);
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P3 (a
// gate's attempt passes through `running`, as AD-8's edges have it) and
// P16 (store-level guards: no run finishes while a step waits; an item is
// never cleaner than its attempt; an input comes only from a finished
// producer; a loop-back never crosses a waiting step; a reservation's
// audit entry carries its taint). Written before the patches.

/// The audit entries whose target is `target`, in order: their actions.
fn actions_on(r: &Rig, target: &str) -> Vec<String> {
    r.audit()
        .into_iter()
        .filter(|e| e["target"] == target || e["args"]["step_run_id"] == target)
        .map(|e| e["action"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// P3: AD-8 has no edge from `queued` to `waiting_human`: a queued gate
/// step that pauses starts first (`running`, its `started_at` stamped,
/// audited `step.started`) and then waits (`step.waiting`), in the pause's
/// one transaction.
#[test]
fn a_paused_gate_passes_through_running() {
    let r = rig();
    assert_eq!(r.step(&r.approve).started_at, None);
    r.store.writer().exec(engine(), r.pause(&new_id())).unwrap();

    let approve = r.step(&r.approve);
    assert_eq!(approve.status, "waiting_human");
    assert!(
        approve.started_at.is_some(),
        "the gate's attempt started before it waited: {approve:?}"
    );
    assert_eq!(approve.finished_at, None);
    let actions = actions_on(&r, &r.approve);
    let started = actions.iter().position(|a| a == "step.started");
    let waiting = actions.iter().position(|a| a == "step.waiting");
    assert!(
        matches!((started, waiting), (Some(s), Some(w)) if s < w),
        "step.started, then step.waiting: {actions:?}"
    );
}

/// P16: a run does not finish while one of its steps waits for a person
/// (its Review item is open): `FinishRun` is refused and changes nothing.
#[test]
fn a_run_does_not_finish_while_a_step_waits() {
    let r = rig();
    r.store.writer().exec(engine(), r.pause(&new_id())).unwrap();
    let before = (r.steps(), r.items(), r.audit().len());
    for status in ["succeeded", "failed"] {
        let refused = r.store.writer().exec(
            engine(),
            WriteCommand::FinishRun {
                run_id: r.run.clone(),
                status: status.into(),
            },
        );
        assert!(refused.is_err(), "{status}: a run with a waiting step");
        let run = r.reader().run(&r.run).unwrap().unwrap();
        assert_eq!((run.status.as_str(), run.finished_at), ("running", None));
        assert_eq!((r.steps(), r.items(), r.audit().len()), before, "{status}");
    }
}

/// P16: an item is never cleaner than the attempt it belongs to: a gate
/// whose own attempt is tainted (an untrusted input of its own) makes a
/// tainted Review item, with no payload or with a clean one, whatever the
/// command says.
#[test]
fn an_item_is_never_cleaner_than_its_attempt() {
    for payload in [false, true] {
        let r = rig();
        let mut pause = r.pause(&new_id());
        if let WriteCommand::PauseStep {
            inputs,
            taint,
            item,
            ..
        } = &mut pause
        {
            inputs.clear();
            inputs.push(StepInput {
                kind: "untrusted".into(),
                from_step_run: None,
                from_item: None,
                label: "test:poisoned-page".into(),
                taint: Taint::Tainted,
            });
            *taint = Taint::Tainted;
            item.taint = Taint::Clean;
            if !payload {
                item.payload = None;
            }
        }
        r.store.writer().exec(engine(), pause).unwrap();
        assert_eq!(
            r.step(&r.approve).taint,
            Taint::Tainted,
            "payload {payload}"
        );
        let items = r.items();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].taint,
            Taint::Tainted,
            "payload {payload}: the item carries its attempt's taint"
        );
    }
}

/// P16: an input is journaled only from a producer that has finished (its
/// output committed): naming a `queued` or `running` step run as the
/// producer is refused and writes nothing; a succeeded one is taken.
#[test]
fn an_input_from_an_unfinished_producer_is_refused() {
    let r = rig();
    let publish = r.steps()[2].id.clone();
    let from = |producer: &str| StepInput {
        kind: "step_output".into(),
        from_step_run: Some(producer.into()),
        from_item: None,
        label: "step:publish@1".into(),
        taint: Taint::Clean,
    };
    let start_approve = |producer: &str| WriteCommand::StartAttempt {
        step_run_id: r.approve.clone(),
        inputs: vec![from(producer)],
        taint: Taint::Clean,
        prompt: vec![],
        writes: am_store::AttemptWrites {
            seat: "seat-a".into(),
            table: None,
            document: None,
        },
    };
    // A queued producer.
    assert!(
        r.store
            .writer()
            .exec(engine(), start_approve(&publish))
            .is_err(),
        "from a queued producer"
    );
    assert_eq!(r.step(&r.approve).status, "queued");
    assert!(r.reader().step_inputs(&r.approve).unwrap().is_empty());
    // A running producer.
    r.store
        .writer()
        .exec(
            engine(),
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
        )
        .unwrap();
    assert!(
        r.store
            .writer()
            .exec(engine(), start_approve(&publish))
            .is_err(),
        "from a running producer"
    );
    assert_eq!(r.step(&r.approve).status, "queued");
    assert!(r.reader().step_inputs(&r.approve).unwrap().is_empty());
    // The same as a pause's input.
    let mut pause = r.pause(&new_id());
    if let WriteCommand::PauseStep { inputs, .. } = &mut pause {
        *inputs = vec![from(&publish)];
    }
    assert!(
        r.store.writer().exec(engine(), pause).is_err(),
        "a pause's input"
    );
    assert!(r.items().is_empty());
    // A succeeded producer is taken.
    r.store
        .writer()
        .exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: r.approve.clone(),
                inputs: vec![StepInput {
                    kind: "step_output".into(),
                    from_step_run: Some(r.draft.clone()),
                    from_item: None,
                    label: "step:draft@1".into(),
                    taint: Taint::Clean,
                }],
                taint: Taint::Clean,
                prompt: vec![],
                writes: am_store::AttemptWrites {
                    seat: "seat-a".into(),
                    table: None,
                    document: None,
                },
            },
        )
        .unwrap();
    assert_eq!(r.reader().step_inputs(&r.approve).unwrap().len(), 1);
}

/// P16: a loop-back never crosses a step that waits for a person: `publish`
/// (after the waiting gate) asking to loop back to `draft` or to the gate
/// itself is refused, and nothing changes (no new attempt, the call open,
/// the gate still waiting).
#[test]
fn a_loop_back_never_crosses_a_waiting_step() {
    let r = rig();
    let w = r.store.writer();
    // `publish` starts before the gate waits (review round 2, R4).
    let publish = r.steps()[2].id.clone();
    w.exec(
        engine(),
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
    )
    .unwrap();
    w.exec(
        engine(),
        WriteCommand::ReserveCall {
            step_run_id: publish.clone(),
            call_id: "call-p".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(engine(), r.pause(&new_id())).unwrap();
    let before = (r.steps(), r.items(), r.audit().len());
    for to in ["draft", "approve"] {
        let refused = w.exec(
            engine(),
            WriteCommand::SettleAndLoopBack {
                call_id: "call-p".into(),
                usage: None,
                cost_micro_usd: None,
                step_run_id: publish.clone(),
                output: r#"{"reasons":["again"]}"#.into(),
                to: to.into(),
            },
        );
        assert!(refused.is_err(), "back to {to}, across the waiting gate");
        assert_eq!((r.steps(), r.items(), r.audit().len()), before, "{to}");
        let calls = r.reader().calls_for_run(&r.run).unwrap();
        assert!(!calls.iter().find(|c| c.id == "call-p").unwrap().settled);
    }
    assert_eq!(r.step(&r.approve).status, "waiting_human");
}

/// P16: a reservation's audit entry (`model_call.reserved`) carries the
/// taint the call goes out with: the attempt's, after the reservation
/// raised it (`1`/`true` tainted, `0`/`false` clean).
#[test]
fn the_reservation_audit_carries_the_taint() {
    let r = rig();
    let w = r.store.writer();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: r.approve.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    for (seq, call, sent, carried) in [
        (1, "call-a", Taint::Clean, false),
        (2, "call-b", Taint::Tainted, true),
        (3, "call-c", Taint::Clean, true),
    ] {
        w.exec(
            engine(),
            WriteCommand::ReserveCall {
                step_run_id: r.approve.clone(),
                call_id: call.into(),
                call_seq: seq,
                reserved_micro_usd: 0,
                rate_snapshot: "none".into(),
                taint: sent,
                purpose: am_store::CallPurpose::Turn,
                prompt: vec![],
            },
        )
        .unwrap();
        let entry = r
            .audit()
            .into_iter()
            .find(|e| e["action"] == "model_call.reserved" && e["target"] == call)
            .unwrap();
        let taint = &entry["args"]["taint"];
        let tainted = taint == &Value::from(1) || taint == &Value::Bool(true);
        let clean = taint == &Value::from(0) || taint == &Value::Bool(false);
        assert!(
            if carried { tainted } else { clean },
            "{call}: the reservation's taint in its audit entry: {entry}"
        );
    }
}

// ------------------------------------------------------- review round 2
//
// Story 3.3 review patch round 2 (coordinator's triage, 2026-11-23): R4
// (a failure never finishes a run while another step waits; nothing starts
// or pauses on a finished run or while another step of the run waits), R5
// (an item's payload is a finished step run or the item's own attempt)
// and R6 (an input's kind decides its producer and taint: `untrusted` is
// always tainted and has no producer; `step_output` and `loop_back` have
// one). Two round-1 tests (`nothing_leaves_waiting_human`,
// `a_loop_back_never_crosses_a_waiting_step`) now start `publish` before
// the gate waits, as R4 requires. Written before the patches.

/// Starts `id` with no inputs, clean.
fn start(r: &Rig, id: &str) -> Result<(), StoreError> {
    r.store.writer().exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: id.into(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        },
    )
}

/// R4: `FailWithItem` finishes the run `failed`, so it is refused while
/// another step of the run waits (the waiting-step check is in the run's
/// finish itself): nothing changes, the running step keeps its open call.
#[test]
fn a_failure_never_finishes_a_run_while_another_step_waits() {
    let r = rig();
    let publish = r.steps()[2].id.clone();
    start(&r, &publish).unwrap();
    r.store
        .writer()
        .exec(
            engine(),
            WriteCommand::ReserveCall {
                step_run_id: publish.clone(),
                call_id: "call-p".into(),
                call_seq: 1,
                reserved_micro_usd: 0,
                rate_snapshot: "none".into(),
                taint: Taint::Clean,
                purpose: am_store::CallPurpose::Turn,
                prompt: vec![],
            },
        )
        .unwrap();
    r.store.writer().exec(engine(), r.pause(&new_id())).unwrap();
    let before = (r.steps(), r.items(), r.audit().len());

    let refused = r.store.writer().exec(
        engine(),
        WriteCommand::FailWithItem {
            step_run_id: publish.clone(),
            settle: Some(CallSettlement {
                call_id: "call-p".into(),
                usage: None,
                cost_micro_usd: None,
            }),
            output: None,
            item: NewInboxItem {
                id: new_id(),
                kind: InboxKind::Failure,
                payload: Some(publish.clone()),
                taint: Taint::Clean,
                title: "A step stopped".into(),
                body: "Step publish stopped at a cap.".into(),
                max_returns: None,
            },
        },
    );

    assert!(
        refused.is_err(),
        "a run with a waiting step is not finished"
    );
    assert_eq!((r.steps(), r.items(), r.audit().len()), before);
    let run = r.reader().run(&r.run).unwrap().unwrap();
    assert_eq!((run.status.as_str(), run.finished_at), ("running", None));
}

/// R4: on a finished run, no step starts (either command) or pauses.
#[test]
fn nothing_starts_or_pauses_on_a_finished_run() {
    let r = rig();
    r.store
        .writer()
        .exec(
            engine(),
            WriteCommand::FinishRun {
                run_id: r.run.clone(),
                status: "failed".into(),
            },
        )
        .unwrap();
    let before = (r.steps(), r.items(), r.audit().len());
    for (what, cmd) in [
        (
            "StartStep",
            WriteCommand::StartStep {
                step_run_id: r.approve.clone(),
            },
        ),
        (
            "StartAttempt",
            WriteCommand::StartAttempt {
                step_run_id: r.approve.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
                writes: am_store::AttemptWrites {
                    seat: "seat-a".into(),
                    table: None,
                    document: None,
                },
            },
        ),
        ("PauseStep", r.pause(&new_id())),
    ] {
        assert!(
            r.store.writer().exec(engine(), cmd).is_err(),
            "{what} on a finished run"
        );
        assert_eq!((r.steps(), r.items(), r.audit().len()), before, "{what}");
    }
}

/// R4: while one step of the run waits for a person, no other step starts
/// (either command) or pauses.
#[test]
fn nothing_starts_or_pauses_while_another_step_waits() {
    let r = rig();
    r.store.writer().exec(engine(), r.pause(&new_id())).unwrap();
    let publish = r.steps()[2].id.clone();
    let before = (r.steps(), r.items(), r.audit().len());
    for (what, cmd) in [
        (
            "StartStep",
            WriteCommand::StartStep {
                step_run_id: publish.clone(),
            },
        ),
        (
            "StartAttempt",
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
        ),
        (
            "PauseStep",
            WriteCommand::PauseStep {
                step_run_id: publish.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                item: NewInboxItem {
                    id: new_id(),
                    kind: InboxKind::Review,
                    payload: Some(r.draft.clone()),
                    taint: Taint::Clean,
                    title: "Review".into(),
                    body: "A second gate.".into(),
                    max_returns: Some(2),
                },
            },
        ),
    ] {
        assert!(
            r.store.writer().exec(engine(), cmd).is_err(),
            "{what} while another step waits"
        );
        assert_eq!((r.steps(), r.items(), r.audit().len()), before, "{what}");
    }
}

/// R5: an item's payload is a finished step run (`succeeded`,
/// `looped_back`, `failed`) or the item's own attempt: a queued or running
/// step run as the payload is refused and writes nothing.
#[test]
fn an_items_payload_is_a_finished_step_run_or_its_own_attempt() {
    // A Review whose payload is a queued step run.
    let r = rig();
    let publish = r.steps()[2].id.clone();
    let mut pause = r.pause(&new_id());
    if let WriteCommand::PauseStep { inputs, item, .. } = &mut pause {
        inputs.clear();
        item.payload = Some(publish.clone());
    }
    assert!(
        r.store.writer().exec(engine(), pause).is_err(),
        "a queued payload"
    );
    assert!(r.items().is_empty());
    assert_eq!(r.step(&r.approve).status, "queued");

    // A Failure whose payload is another running step run; then its own.
    let r = rig();
    let publish = r.steps()[2].id.clone();
    start(&r, &publish).unwrap();
    start(&r, &r.approve).unwrap();
    let fail = |payload: &str| WriteCommand::FailWithItem {
        step_run_id: r.approve.clone(),
        settle: None,
        output: None,
        item: NewInboxItem {
            id: new_id(),
            kind: InboxKind::Failure,
            payload: Some(payload.into()),
            taint: Taint::Clean,
            title: "A step stopped".into(),
            body: "Step approve stopped at a cap.".into(),
            max_returns: None,
        },
    };
    assert!(
        r.store.writer().exec(engine(), fail(&publish)).is_err(),
        "a running payload"
    );
    assert!(r.items().is_empty());
    assert_eq!(r.step(&r.approve).status, "running");
    r.store
        .writer()
        .exec(engine(), fail(&r.approve.clone()))
        .expect("its own attempt is a payload");
    assert_eq!(r.items().len(), 1);
}

/// R6: an input's kind decides its producer and taint. An `untrusted`
/// input is always tainted (one sent as clean is journaled tainted, and
/// taints its attempt) and has no producer (one naming a step run is
/// refused); a `step_output` or `loop_back` input needs its producer (one
/// with none is refused). A refusal writes nothing.
#[test]
fn an_inputs_kind_decides_its_producer_and_taint() {
    let r = rig();
    r.store
        .writer()
        .exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: r.approve.clone(),
                inputs: vec![StepInput {
                    kind: "untrusted".into(),
                    from_step_run: None,
                    from_item: None,
                    label: "test:poisoned-page".into(),
                    taint: Taint::Clean,
                }],
                taint: Taint::Clean,
                prompt: vec![],
                writes: am_store::AttemptWrites {
                    seat: "seat-a".into(),
                    table: None,
                    document: None,
                },
            },
        )
        .unwrap();
    assert_eq!(
        r.reader().step_inputs(&r.approve).unwrap()[0].taint,
        Taint::Tainted,
        "an untrusted input is journaled tainted"
    );
    assert_eq!(r.step(&r.approve).taint, Taint::Tainted);

    let publish = r.steps()[2].id.clone();
    for (what, input) in [
        (
            "an untrusted input with a producer",
            StepInput {
                kind: "untrusted".into(),
                from_step_run: Some(r.draft.clone()),
                from_item: None,
                label: "test:poisoned-page".into(),
                taint: Taint::Tainted,
            },
        ),
        (
            "a step_output with no producer",
            StepInput {
                kind: "step_output".into(),
                from_step_run: None,
                from_item: None,
                label: "step:draft@1".into(),
                taint: Taint::Clean,
            },
        ),
        (
            "a loop_back with no producer",
            StepInput {
                kind: "loop_back".into(),
                from_step_run: None,
                from_item: None,
                label: "loop-back:check@1".into(),
                taint: Taint::Clean,
            },
        ),
    ] {
        let refused = r.store.writer().exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: publish.clone(),
                inputs: vec![input],
                taint: Taint::Clean,
                prompt: vec![],
                writes: am_store::AttemptWrites {
                    seat: "seat-a".into(),
                    table: None,
                    document: None,
                },
            },
        );
        assert!(refused.is_err(), "{what}");
        assert_eq!(r.step(&publish).status, "queued", "{what}");
        assert!(
            r.reader().step_inputs(&publish).unwrap().is_empty(),
            "{what}"
        );
    }
}
