//! Story 3.3 review patch round 2 (coordinator's triage, 2026-11-23): R7
//! (only ToolGate runs a tool, and that holds by construction: a `Permit`
//! runs only on the gate that admitted it; a runner whose manifests repeat
//! an id is refused) and R9 (ToolGate labels the tool names it audits
//! itself, so a raw model-chosen name never reaches the audit sink or the
//! refusal's words, whatever the caller passes). Through ToolGate's own
//! API (`ToolGate::over`, `admit`, `run`) and `run_plan_with_tools`, with
//! test-local runners; tests are not shipped. Written before the patches.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use am_domain::rows::{
    ColumnKind, ColumnSpec, DeclaredTable, PreparedRow, RowOp, RowStatus, RowTool, RowWrite,
    RowWriteError, RowWriter, RowWritten,
};
use am_domain::tool_manifest::{
    ActionClass as Declared, NetworkNeed, ToolManifest as DomainManifest,
};
use am_domain::{
    Actor, BoxFuture, CancelSignal, EngineResult, Instruction, Taint, ToolCall, ToolRunner,
    ToolSpec, UntrustedData,
};
use am_engine::TokenSignal;
use am_engine::row_writer::StoreRowWriter;
use am_engine::run::{HandOff, Limits, RunPlan, StepPlan, run_plan_with_tools};
use am_engine::toolgate::{
    CallPlace, GateError, GateOutput, StepTools, ToolGate, engine_result_of,
};
use am_store::{AttemptWrites, Reader, Store, WriteCommand, new_id};
use am_testkit::{CollectAudit, FakeLlm, ScriptedTurn};
use tokio_util::sync::CancellationToken;

/// A test-local runner: manifests as given (ids may repeat), and the id of
/// every call it ran.
struct Runner {
    manifests: Vec<(&'static str, Declared)>,
    ran: Mutex<Vec<String>>,
}

impl Runner {
    fn new(manifests: Vec<(&'static str, Declared)>) -> Arc<Runner> {
        Arc::new(Runner {
            manifests,
            ran: Mutex::new(Vec::new()),
        })
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

impl ToolRunner for Runner {
    fn manifests(&self) -> Vec<DomainManifest> {
        self.manifests
            .iter()
            .map(|(id, class)| DomainManifest {
                id: id.to_string(),
                class: *class,
                network: NetworkNeed::None,
                max_uses_per_step: 100,
            })
            .collect()
    }

    fn run(
        &self,
        call: ToolCall,
        _cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<UntrustedData, String>> {
        self.ran.lock().unwrap().push(call.name.clone());
        Box::pin(async move { Ok(UntrustedData::text(format!("tool:{}", call.name), "ran")) })
    }
}

/// The seat `seat-work` in step `work` of one run (the same run each call,
/// so an audited actor compares equal to it).
fn seat() -> Actor {
    static RUN: std::sync::OnceLock<uuid::Uuid> = std::sync::OnceLock::new();
    Actor::Seat {
        seat: "seat-work".into(),
        run: *RUN.get_or_init(uuid::Uuid::now_v7),
        step: "work".into(),
    }
}

fn call(name: &str) -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: name.into(),
        arguments_json: "{}".into(),
    }
}

fn cancel() -> Arc<dyn CancelSignal> {
    TokenSignal::signal(CancellationToken::new())
}

// ------------------------------------------------------------ story 3.6
//
// Story 3.6 (AD-10 as amended 2026-11-24, AD-11, AD-21, AD-46; plan matrix
// rows 2, 6 to 11, 35 and 36): ToolGate checks the seat's grants, lets of
// the local-write class only `table_write` through, binds it to the step's
// declared table of the seat's own team, calls the row-writer port (the
// one caller) and builds the engine result of a successful write; an error
// comes back for the model as words, untrusted. `admit` takes the step's
// facts (`StepTools`) and the call's place (`CallPlace`). Written before
// any of it exists.

/// The step's facts with `grants`, the seat in team `core`, declaring
/// `table`.
fn facts(grants: &[&str], table: Option<DeclaredTable>) -> StepTools {
    StepTools {
        grants: grants
            .iter()
            .map(|g| g.to_string())
            .collect::<BTreeSet<String>>(),
        seat_team: "core".into(),
        table,
    }
}

/// The step's facts with `grants` and no table.
fn granted(grants: &[&str]) -> StepTools {
    facts(grants, None)
}

/// The call's place: step run `step-run-1`, the first call, clean.
fn place() -> CallPlace {
    CallPlace {
        step_run_id: "step-run-1".into(),
        call_seq: 1,
        tool_call_id: "tool-call-1".into(),
        taint: Taint::Clean,
    }
}

/// `entries` (team `core`) with one required text column `ref`.
fn entries() -> DeclaredTable {
    DeclaredTable {
        id: "entries".into(),
        team: "core".into(),
        columns: vec![ColumnSpec {
            id: "ref".into(),
            kind: ColumnKind::Text,
            required: true,
            values: vec![],
        }],
    }
}

/// `others` of team `side`.
fn others() -> DeclaredTable {
    DeclaredTable {
        id: "others".into(),
        team: "side".into(),
        ..entries()
    }
}

/// A `table_write` call naming `table`.
fn write_call(table: &str) -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: "table_write".into(),
        arguments_json: format!("{{\"table\":\"{table}\",\"data\":{{\"ref\":\"r-1\"}}}}"),
    }
}

/// A row-writer port that records each write it is asked for and answers
/// as it was told.
struct Rows {
    asked: Mutex<Vec<(Actor, RowWrite)>>,
    answer: Result<RowWritten, RowWriteError>,
}

impl Rows {
    fn answering(answer: Result<RowWritten, RowWriteError>) -> Arc<Rows> {
        Arc::new(Rows {
            asked: Mutex::new(Vec::new()),
            answer,
        })
    }

    /// A port that is never meant to be called (it fails if it is).
    fn none() -> Arc<Rows> {
        Rows::answering(Err(RowWriteError::Failed("this test writes no row".into())))
    }

    fn asked(&self) -> Vec<(Actor, RowWrite)> {
        self.asked.lock().unwrap().clone()
    }
}

impl RowWriter for Rows {
    fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError> {
        self.asked.lock().unwrap().push((actor, write));
        self.answer.clone()
    }
}

/// A row tool: a fixed spec; `prepare` gives an insert of `{"ref":"r-1"}`,
/// or refuses arguments that hold `"bad"` (in its own words).
struct Tool;

const PREPARED: &str = r#"{"ref":"r-1"}"#;

impl RowTool for Tool {
    fn spec(&self, table: &DeclaredTable) -> ToolSpec {
        ToolSpec {
            name: "table_write".into(),
            description: format!("Writes a row of {}.", table.id),
            input_schema_json: "{\"type\":\"object\"}".into(),
        }
    }
    fn prepare(&self, arguments_json: &str, _table: &DeclaredTable) -> Result<PreparedRow, String> {
        if arguments_json.contains("\"bad\"") {
            Err("the column `ref` is text".into())
        } else {
            Ok(PreparedRow::Insert {
                data_json: PREPARED.into(),
            })
        }
    }
}

/// The shipped gate over `rows` and [`Tool`], auditing into `audit`.
fn shipped(rows: Arc<Rows>, audit: Arc<CollectAudit>) -> ToolGate {
    ToolGate::shipped(rows, Some(Arc::new(Tool)), audit)
}

/// R7: a `Permit` runs only on the gate that admitted it. Two gates over
/// two runners with the same tool: a permit admitted by the first, handed
/// to the second, is refused, and neither runner runs it.
#[tokio::test]
async fn a_permit_from_one_gate_is_refused_by_another() {
    let (a, b) = (
        Runner::new(vec![("lookup", Declared::Read)]),
        Runner::new(vec![("lookup", Declared::Read)]),
    );
    let gate_a = ToolGate::over(a.clone(), Rows::none(), None, CollectAudit::new());
    let gate_b = ToolGate::over(b.clone(), Rows::none(), None, CollectAudit::new());
    let permit = gate_a
        .admit(&seat(), &granted(&["lookup"]), &place(), &call("lookup"))
        .expect("gate A admits a granted read tool");

    let ran = gate_b.run(permit, cancel()).await;

    assert!(ran.is_err(), "another gate's permit is refused: {ran:?}");
    assert!(b.ran().is_empty(), "gate B's runner never runs it");
    assert!(a.ran().is_empty(), "nor gate A's");
}

/// R7, the control: the gate that admitted the permit runs it.
#[tokio::test]
async fn a_permit_runs_on_its_own_gate() {
    let a = Runner::new(vec![("lookup", Declared::Read)]);
    let gate = ToolGate::over(a.clone(), Rows::none(), None, CollectAudit::new());
    let permit = gate
        .admit(&seat(), &granted(&["lookup"]), &place(), &call("lookup"))
        .unwrap();

    let ran = gate.run(permit, cancel()).await;

    assert!(
        matches!(ran, Ok(GateOutput::Untrusted(_))),
        "a runner's result is untrusted: {ran:?}"
    );
    assert_eq!(a.ran(), vec!["lookup"]);
}

/// R7: a runner whose manifests name one id twice is refused (which
/// manifest would decide is not left to chance): with tools, the run never
/// runs that tool, and nothing reports success.
#[tokio::test]
async fn duplicate_manifest_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    // The same id, declared external-effect first and read second.
    let runner = Runner::new(vec![
        ("twice_named", Declared::ExternalEffect),
        ("twice_named", Declared::Read),
    ]);
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("looking").tool_call("twice_named", "{}"),
        ScriptedTurn::text("done"),
    ]));
    let plan = RunPlan {
        workflow: "wf".into(),
        config_hash: "0".repeat(64),
        steps: vec![StepPlan {
            key: "work".into(),
            seat: "seat-work".into(),
            system: vec![],
            instruction: Instruction::authored("Do the work."),
            inputs: vec![],
            hand_off: HandOff::Next,
            limits: Limits::default(),
            model: "example-local".into(),
            max_output_tokens: 64,
            provider: llm,
            prompt: am_engine::prompt::StepPrompt::default(),
            output_schema: None,
            // Story 3.6: the seat is granted the tool it asks for.
            grants: ["twice_named".to_string()].into_iter().collect(),
            seat_team: "core".into(),
            table: None,
            document: None,
            row_inputs: vec![],
        }],
        clock: am_testkit::FakeClock::at("2026-11-24T10:00:00Z"),
        row_tool: None,
    };
    let owner = Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    };

    let result = run_plan_with_tools(
        &store,
        owner,
        "manual",
        &plan,
        CancellationToken::new(),
        runner.clone(),
    )
    .await;

    assert!(runner.ran().is_empty(), "the tool never ran: {result:?}");
    match &result {
        Err(e) => assert!(e.to_string().contains("twice_named"), "{e}"),
        Ok(outcome) => {
            assert_eq!(outcome.status, "failed", "{outcome:?}");
            let reason = outcome.reason.clone().unwrap_or_default();
            assert!(reason.contains("twice_named"), "{reason}");
            let reader = Reader::open(&db).unwrap();
            let step = reader.steps(&outcome.run_id).unwrap().remove(0);
            assert!(
                reader
                    .tool_calls(&step.id)
                    .unwrap()
                    .iter()
                    .all(|t| t.status == "refused"),
                "no call of it is allowed"
            );
        }
    }
}

/// The sha256 of `text`'s UTF-8 bytes, lowercase hex.
fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// R9: ToolGate labels the names it audits and words itself. A caller that
/// passes the model's raw name (an unknown one, one with control
/// characters) gets a refusal, and the audit sink an entry, that hold
/// `unknown:<sha256 of the name>` and nothing of the raw name; a name a
/// manifest holds, or a built-in tool id, stays as itself.
#[test]
fn toolgate_labels_the_names_it_audits() {
    for (what, name, marker) in [
        (
            "an unknown name",
            "exfiltrate_every_key".to_string(),
            "exfiltrate_every_key".to_string(),
        ),
        (
            "control characters",
            "zq9\u{1b}[2J\u{7}wipe\nname".to_string(),
            "zq9".to_string(),
        ),
        ("10 KB", "x".repeat(10 * 1024), "x".repeat(64)),
    ] {
        let audit = CollectAudit::new();
        let gate = ToolGate::over(
            Runner::new(vec![("lookup", Declared::Read)]),
            Rows::none(),
            None,
            audit.clone(),
        );

        let refused = gate
            .admit(&seat(), &granted(&["lookup"]), &place(), &call(&name))
            .expect_err("an unknown tool is refused");

        let label = format!("unknown:{}", sha256_hex(&name));
        assert!(
            !refused.contains(&marker),
            "{what}: the words: {refused:.200}"
        );
        assert!(!refused.chars().any(char::is_control), "{what}");
        let events = audit.events();
        assert_eq!(events.len(), 1, "{what}");
        let e = &events[0];
        let seen = format!("{} {} {:?}", e.target, e.outcome, e.args);
        assert!(
            !seen.contains(&marker),
            "{what}: the audit entry: {seen:.300}"
        );
        assert!(!seen.chars().any(char::is_control), "{what}: {seen:.300}");
        assert!(e.target.contains(&label), "{what}: labelled: {}", e.target);
    }

    // Names a manifest holds, and built-in ids, stay as themselves.
    let audit = CollectAudit::new();
    let gate = ToolGate::over(
        Runner::new(vec![("lookup", Declared::ExternalEffect)]),
        Rows::none(),
        None,
        audit.clone(),
    );
    let both = granted(&["lookup", "web_search"]);
    assert!(
        gate.admit(&seat(), &both, &place(), &call("lookup"))
            .is_err()
    );
    assert!(
        gate.admit(&seat(), &both, &place(), &call("web_search"))
            .is_err()
    );
    let targets: Vec<String> = audit.events().into_iter().map(|e| e.target).collect();
    assert!(targets[0].contains("tool:lookup"), "{targets:?}");
    assert!(targets[1].contains("tool:web_search"), "{targets:?}");
}

/// Row 8 (AD-46: grants come only from capabilities): a tool the seat is
/// not granted is refused before its class, in words naming the tool, `not
/// granted` and the seat, and audited `toolgate.refused` with the tool, the
/// run and the step; the port and the runner are never asked. The same
/// tools, granted, are admitted (the control).
#[tokio::test]
async fn a_tool_not_granted_to_the_seat_is_refused_and_audited() {
    let audit = CollectAudit::new();
    let rows = Rows::none();
    let gate = shipped(rows.clone(), audit.clone());

    let refused = gate
        .admit(
            &seat(),
            &facts(&[], Some(entries())),
            &place(),
            &write_call("entries"),
        )
        .expect_err("table_write is not granted");

    assert!(
        refused.contains("table_write")
            && refused.contains("not granted")
            && refused.contains("seat-work"),
        "{refused}"
    );
    let events = audit.events();
    assert_eq!(events.len(), 1, "{events:#?}");
    assert_eq!(events[0].action, "toolgate.refused");
    assert_eq!(events[0].actor, seat());
    assert!(
        events[0].target.starts_with("tool:table_write run:"),
        "{}",
        events[0].target
    );
    assert!(
        events[0].target.ends_with(" step:work"),
        "{}",
        events[0].target
    );
    assert!(rows.asked().is_empty());

    // A runner's read tool is refused the same way until it is granted.
    let runner = Runner::new(vec![("lookup", Declared::Read)]);
    let audit = CollectAudit::new();
    let gate = ToolGate::over(runner.clone(), Rows::none(), None, audit.clone());
    let refused = gate
        .admit(&seat(), &granted(&[]), &place(), &call("lookup"))
        .expect_err("lookup is not granted");
    assert!(
        refused.contains("lookup") && refused.contains("not granted"),
        "{refused}"
    );
    assert_eq!(audit.events().len(), 1);
    let permit = gate
        .admit(&seat(), &granted(&["lookup"]), &place(), &call("lookup"))
        .expect("granted, it is admitted");
    assert!(gate.run(permit, cancel()).await.is_ok());
    assert_eq!(runner.ran(), vec!["lookup"]);

    // Granted and declared, `table_write` is admitted.
    let gate = shipped(Rows::none(), CollectAudit::new());
    assert!(
        gate.admit(
            &seat(),
            &facts(&["table_read", "table_write"], Some(entries())),
            &place(),
            &write_call("entries")
        )
        .is_ok()
    );
}

/// Rows 6 and 11 (AD-21, AD-10 (b)): a granted `table_write` is admitted
/// only when its step declares a table and the call names that table. A
/// step with no table: refused "declares no table", naming the step. A call
/// naming another table, or none: refused, naming the declared table and
/// "another table". Each refusal is audited with the step and the run.
#[test]
fn table_write_is_admitted_only_for_the_declared_table() {
    let tables_on = ["table_read", "table_write"];

    let audit = CollectAudit::new();
    let gate = shipped(Rows::none(), audit.clone());
    let refused = gate
        .admit(
            &seat(),
            &facts(&tables_on, None),
            &place(),
            &write_call("entries"),
        )
        .expect_err("the step declares no table");
    assert!(
        refused.contains("declares no table") && refused.contains("work"),
        "{refused}"
    );

    for args in [
        r#"{"table":"others","data":{"ref":"r-1"}}"#,
        r#"{"table":"ENTRIES","data":{"ref":"r-1"}}"#,
    ] {
        let call = ToolCall {
            arguments_json: args.into(),
            ..write_call("entries")
        };
        let refused = gate
            .admit(
                &seat(),
                &facts(&tables_on, Some(entries())),
                &place(),
                &call,
            )
            .expect_err("only the declared table");
        assert!(refused.contains("entries"), "{args}: {refused}");
        assert!(refused.contains("another table"), "{args}: {refused}");
    }
    // Review round 1, R6 (manager, 2026-11-25): admission refuses only a
    // `table` that is a string naming another table. A call with no
    // `table`, a `table` that is not a string, arguments that are not JSON
    // or JSON that is no object is admitted, so the row tool's `prepare`
    // words it for the model as an untrusted error result and the step
    // goes on (before R6, admission refused the first and the third and
    // the step failed).
    for args in [
        r#"{"data":{"ref":"r-1"}}"#,
        r#"{"table":7,"data":{"ref":"r-1"}}"#,
        "not json at all",
        r#"["entries"]"#,
    ] {
        let call = ToolCall {
            arguments_json: args.into(),
            ..write_call("entries")
        };
        let admitted = gate.admit(
            &seat(),
            &facts(&tables_on, Some(entries())),
            &place(),
            &call,
        );
        assert!(admitted.is_ok(), "{args}: admitted: {admitted:?}");
    }
    let events = audit.events();
    assert_eq!(events.len(), 3, "{events:#?}");
    for e in &events {
        assert_eq!(e.action, "toolgate.refused");
        assert!(
            e.target.starts_with("tool:table_write run:"),
            "{}",
            e.target
        );
        assert!(e.target.ends_with(" step:work"), "{}", e.target);
    }

    assert!(
        gate.admit(
            &seat(),
            &facts(&tables_on, Some(entries())),
            &place(),
            &write_call("entries")
        )
        .is_ok(),
        "the declared table is admitted"
    );
}

/// Row 7 (AD-21: a table has one owning team): a step that declares
/// another team's table is refused whatever the call names, in words naming
/// the table, its team, the seat and the seat's team; audited.
#[test]
fn another_teams_table_is_refused() {
    let audit = CollectAudit::new();
    let rows = Rows::none();
    let gate = shipped(rows.clone(), audit.clone());

    let refused = gate
        .admit(
            &seat(),
            &facts(&["table_read", "table_write"], Some(others())),
            &place(),
            &write_call("others"),
        )
        .expect_err("another team's table");

    for word in ["others", "side", "core", "seat-work"] {
        assert!(refused.contains(word), "names `{word}`: {refused}");
    }
    assert_eq!(audit.events().len(), 1);
    assert_eq!(audit.events()[0].action, "toolgate.refused");
    assert!(rows.asked().is_empty());
}

/// Row 10 (AD-11; external effects wait for 4.8, the other local-write
/// tools for their stories): a granted `write_document`, `draft_message` or
/// `workspace_write`, and a runner's granted local-write or external-effect
/// tool, are refused by class in words naming the tool and its class; none
/// runs.
#[tokio::test]
async fn other_local_write_and_external_effect_tools_stay_refused() {
    for id in ["write_document", "draft_message", "workspace_write"] {
        let audit = CollectAudit::new();
        let gate = shipped(Rows::none(), audit.clone());
        let refused = gate
            .admit(&seat(), &granted(&[id]), &place(), &call(id))
            .expect_err("refused by class");
        assert!(
            refused.contains(id) && refused.contains("local-write"),
            "{id}: {refused}"
        );
        assert_eq!(audit.events().len(), 1, "{id}");
    }
    let runner = Runner::new(vec![
        ("save_it", Declared::LocalWrite),
        ("send_it", Declared::ExternalEffect),
    ]);
    let gate = ToolGate::over(runner.clone(), Rows::none(), None, CollectAudit::new());
    for (id, class) in [("save_it", "local-write"), ("send_it", "external-effect")] {
        let refused = gate
            .admit(
                &seat(),
                &granted(&["save_it", "send_it"]),
                &place(),
                &call(id),
            )
            .expect_err("refused by class");
        assert!(
            refused.contains(id) && refused.contains(class),
            "{id}: {refused}"
        );
    }
    assert!(runner.ran().is_empty(), "nothing ran");
}

/// Row 6 (no model words in refusals): the model's table name, a canary,
/// is in neither the refusal's words nor the audit entry (target, outcome,
/// arguments), raw or hashed.
#[test]
fn a_refusal_never_echoes_the_models_table_name() {
    const CANARY: &str = "table-name-canary-5e1d";
    let audit = CollectAudit::new();
    let gate = shipped(Rows::none(), audit.clone());

    let refused = gate
        .admit(
            &seat(),
            &facts(&["table_write"], Some(entries())),
            &place(),
            &write_call(CANARY),
        )
        .expect_err("another table");

    assert!(!refused.contains(CANARY), "{refused}");
    let events = audit.events();
    assert_eq!(events.len(), 1);
    let seen = format!(
        "{} {} {:?} {:?}",
        events[0].target, events[0].outcome, events[0].args, events[0].hash
    );
    assert!(!seen.contains(CANARY), "{seen}");
    assert!(
        !seen.contains(&sha256_hex(CANARY)),
        "not hashed either: {seen}"
    );
}

/// Rows 2 and 35 (AD-10 as amended 2026-11-24, AD-21): an admitted
/// `table_write` is prepared by the row tool and written through the port,
/// once, as the seat, with the call's place, the attempt's taint and a row
/// id ToolGate minted (a UUIDv7); the port's `RowWritten` comes back as
/// the engine result its one constructor builds (label `tool:table_write`),
/// not as untrusted data. A tainted place writes tainted.
#[tokio::test]
async fn a_write_comes_back_as_an_engine_result() {
    for taint in [Taint::Clean, Taint::Tainted] {
        let written = RowWritten {
            row_id: uuid::Uuid::now_v7(),
            version: 1,
            status: RowStatus::Inserted,
        };
        let rows = Rows::answering(Ok(written));
        let gate = shipped(rows.clone(), CollectAudit::new());
        let at = CallPlace {
            call_seq: 3,
            taint,
            ..place()
        };
        let permit = gate
            .admit(
                &seat(),
                &facts(&["table_write"], Some(entries())),
                &at,
                &write_call("entries"),
            )
            .expect("admitted");

        let ran = gate.run(permit, cancel()).await;

        let result = match ran {
            Ok(GateOutput::Engine(result)) => result,
            other => panic!("{taint:?}: an engine result: {other:?}"),
        };
        assert_eq!(result, EngineResult::row_written(&written), "{taint:?}");
        assert_eq!(result.label(), "tool:table_write");
        assert!(
            result.text().contains(&written.row_id.to_string()),
            "{}",
            result.text()
        );
        let asked = rows.asked();
        assert_eq!(asked.len(), 1, "one write: {asked:#?}");
        let (actor, write) = &asked[0];
        assert_eq!(actor, &seat());
        assert_eq!(
            (
                write.tool_call_id.as_str(),
                write.step_run_id.as_str(),
                write.call_seq,
                write.table_id.as_str(),
                write.taint
            ),
            ("tool-call-1", "step-run-1", 3, "entries", taint)
        );
        let RowOp::Insert { row_id, data_json } = &write.op else {
            panic!("an insert: {write:?}");
        };
        assert_eq!(data_json, PREPARED);
        assert_eq!(row_id.get_version_num(), 7, "a UUIDv7 ToolGate minted");
    }
}

/// Row 36 (AD-10 as amended: error results are excluded): the port's
/// refusal (a stale version) comes back as words for the model
/// (`GateError::Tool`, which the engine sends as untrusted data), a row the
/// row tool refuses too, without asking the port; a store failure stops
/// the step (`GateError::Store`). None is an engine result.
#[tokio::test]
async fn an_error_comes_back_untrusted() {
    let tables = facts(&["table_write"], Some(entries()));

    let rows = Rows::answering(Err(RowWriteError::Refused(
        "row 0192 is at version 2".into(),
    )));
    let gate = shipped(rows.clone(), CollectAudit::new());
    let permit = gate
        .admit(&seat(), &tables, &place(), &write_call("entries"))
        .unwrap();
    match gate.run(permit, cancel()).await {
        Err(GateError::Tool(words)) => assert!(words.contains("version 2"), "{words}"),
        other => panic!("words for the model: {other:?}"),
    }
    assert_eq!(rows.asked().len(), 1);

    let rows = Rows::none();
    let gate = shipped(rows.clone(), CollectAudit::new());
    let bad = ToolCall {
        arguments_json: r#"{"table":"entries","data":{"ref":"bad"}}"#.into(),
        ..write_call("entries")
    };
    let permit = gate.admit(&seat(), &tables, &place(), &bad).unwrap();
    match gate.run(permit, cancel()).await {
        Err(GateError::Tool(words)) => assert!(words.contains("ref"), "{words}"),
        other => panic!("the row tool's words: {other:?}"),
    }
    assert!(rows.asked().is_empty(), "a bad row never reaches the port");

    let rows = Rows::answering(Err(RowWriteError::Failed("disk full".into())));
    let gate = shipped(rows, CollectAudit::new());
    let permit = gate
        .admit(&seat(), &tables, &place(), &write_call("entries"))
        .unwrap();
    assert!(
        matches!(gate.run(permit, cancel()).await, Err(GateError::Store(_))),
        "a store failure stops the step"
    );
}

// ------------------------------------------------------------ row 44
//
// AD-8 and AD-10 as amended 2026-11-24 (Winston's wording: the engine
// result is journaled on its tool call's journal row): a resume (3.9) or a
// drill-down (epic 6) rebuilds the result with
// `toolgate::engine_result_of(reader, step_run_id, call_seq)` from the
// store's data-free receipt (`Reader::row_receipt`), through ToolGate's one
// constructor; nothing parses journal text or tool output.

/// A row tool that prepares the one row it was made with, whatever the
/// arguments (the spec is [`Tool`]'s).
struct Fixed(PreparedRow);

impl RowTool for Fixed {
    fn spec(&self, table: &DeclaredTable) -> ToolSpec {
        Tool.spec(table)
    }
    fn prepare(
        &self,
        _arguments_json: &str,
        _table: &DeclaredTable,
    ) -> Result<PreparedRow, String> {
        Ok(self.0.clone())
    }
}

/// A store with one run whose step `work` is running, journaled to write
/// `entries` as `seat-work`.
struct Journaled {
    _dir: tempfile::TempDir,
    db: std::path::PathBuf,
    store: Store,
    run: uuid::Uuid,
    step_run: String,
}

impl Journaled {
    fn new() -> Journaled {
        let engine = Actor::System {
            component: "engine".into(),
        };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let store = Store::open(&db).unwrap();
        let run = new_id();
        let w = store.writer();
        w.exec(
            engine.clone(),
            WriteCommand::CreateRun {
                run_id: run.clone(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["work".into()],
                config_hash: None,
            },
        )
        .unwrap();
        let step_run = store.reader().unwrap().steps(&run).unwrap()[0].id.clone();
        w.exec(
            engine,
            WriteCommand::StartAttempt {
                step_run_id: step_run.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
                writes: AttemptWrites {
                    seat: "seat-work".into(),
                    table: Some("entries".into()),
                    document: None,
                },
            },
        )
        .unwrap();
        Journaled {
            _dir: dir,
            db,
            store,
            run: uuid::Uuid::parse_str(&run).unwrap(),
            step_run,
        }
    }

    /// The attempt's own seat.
    fn seat(&self) -> Actor {
        Actor::Seat {
            seat: "seat-work".into(),
            run: self.run,
            step: "work".into(),
        }
    }

    /// A `table_write` call journaled `issued` at `call_seq`, as the engine
    /// journals it before ToolGate runs it; its place.
    fn issued(&self, call_seq: i64) -> CallPlace {
        let id = new_id();
        self.store
            .writer()
            .exec(
                Actor::System {
                    component: "engine".into(),
                },
                WriteCommand::RecordToolCall {
                    tool_call_id: id.clone(),
                    step_run_id: self.step_run.clone(),
                    call_seq,
                    tool: "table_write".into(),
                    status: "issued".into(),
                },
            )
            .unwrap();
        CallPlace {
            step_run_id: self.step_run.clone(),
            call_seq,
            tool_call_id: id,
            taint: Taint::Clean,
        }
    }

    /// The shipped gate over the store's row writer, preparing `row`.
    fn gate(&self, row: PreparedRow) -> ToolGate {
        ToolGate::shipped(
            Arc::new(StoreRowWriter::new(&self.store)),
            Some(Arc::new(Fixed(row))),
            CollectAudit::new(),
        )
    }

    /// ToolGate admits and runs a `table_write` at `place` preparing `row`.
    async fn write(&self, place: &CallPlace, row: PreparedRow) -> Result<GateOutput, GateError> {
        let gate = self.gate(row);
        let permit = gate
            .admit(
                &self.seat(),
                &facts(&["table_write"], Some(entries())),
                place,
                &write_call("entries"),
            )
            .expect("admitted");
        gate.run(permit, cancel()).await
    }
}

/// Row 44 (AD-8; AD-10 as amended 2026-11-24): an insert and an update run
/// through the shipped gate over the store's row writer; for each,
/// `engine_result_of` on a fresh read-only connection, after the row has
/// moved on, rebuilds exactly the engine result the gate gave (its label,
/// its text, its version and word). A stale update (an error result, the
/// call left for the engine to finish `failed`) and a key with no call
/// rebuild nothing (`Ok(None)`).
#[tokio::test]
async fn the_rebuilt_result_equals_the_one_sent() {
    let j = Journaled::new();
    let first = j.issued(1);
    let sent_insert = match j
        .write(
            &first,
            PreparedRow::Insert {
                data_json: r#"{"ref":"r-1"}"#.into(),
            },
        )
        .await
    {
        Ok(GateOutput::Engine(result)) => result,
        other => panic!("an engine result: {other:?}"),
    };
    let row_id = uuid::Uuid::parse_str(
        &j.store
            .reader()
            .unwrap()
            .row_receipt(&j.step_run, 1)
            .unwrap()
            .expect("the insert's receipt")
            .row_id,
    )
    .unwrap();
    assert_eq!(
        sent_insert,
        EngineResult::row_written(&RowWritten {
            row_id,
            version: 1,
            status: RowStatus::Inserted,
        })
    );

    let second = j.issued(2);
    let sent_update = match j
        .write(
            &second,
            PreparedRow::Update {
                row_id,
                expected_version: 1,
                patch_json: r#"{"ref":"r-2"}"#.into(),
            },
        )
        .await
    {
        Ok(GateOutput::Engine(result)) => result,
        other => panic!("an engine result: {other:?}"),
    };
    let third = j.issued(3);
    let stale = j
        .write(
            &third,
            PreparedRow::Update {
                row_id,
                expected_version: 1,
                patch_json: r#"{"ref":"r-3"}"#.into(),
            },
        )
        .await;
    assert!(
        matches!(stale, Err(GateError::Tool(_))),
        "a stale update is words for the model: {stale:?}"
    );

    let reader = Reader::open(&j.db).unwrap();
    for (seq, sent) in [(1, &sent_insert), (2, &sent_update)] {
        let rebuilt = engine_result_of(&reader, &j.step_run, seq)
            .unwrap()
            .unwrap_or_else(|| panic!("call {seq} is rebuilt"));
        assert_eq!(&rebuilt, sent, "call {seq}");
        assert_eq!(rebuilt.label(), "tool:table_write");
        assert_eq!(rebuilt.text(), sent.text(), "call {seq}");
    }
    assert_eq!(
        sent_update,
        EngineResult::row_written(&RowWritten {
            row_id,
            version: 2,
            status: RowStatus::Updated,
        })
    );
    assert_eq!(
        engine_result_of(&reader, &j.step_run, 3).unwrap(),
        None,
        "an error result is not rebuilt as an engine result"
    );
    assert_eq!(engine_result_of(&reader, &j.step_run, 4).unwrap(), None);
    assert_eq!(
        engine_result_of(&reader, "no-such-step-run", 1).unwrap(),
        None
    );
}

// ------------------------------------------------- review round 1 (3.23)
//
// The ToolGate slice's review triage round 1 (manager, 2026-11-25): R4 (a
// replay answers with the first write's status word, so the result the
// model receives equals the one a resume rebuilds) and R5 (a runner tool
// whose id is a built-in's is refused: it never replaces the built-in's
// manifest, its class or its use limit). Written before the patches.

/// R4, through the shipped gate over the store's row writer: an insert
/// (call 1) and an update (call 2) run once; then each call runs again at
/// its own place (the same tool call, seat and table, another prepared row
/// of the same op kind), as a resume re-runs a call whose write committed.
/// Each replay writes nothing and gives exactly the result the first run
/// gave and `engine_result_of` rebuilds: the first write's row id, version
/// and word (`inserted`, `updated`), never another word.
#[tokio::test]
async fn a_replay_gives_the_first_writes_result() {
    let j = Journaled::new();
    let first = j.issued(1);
    let sent_insert = match j
        .write(
            &first,
            PreparedRow::Insert {
                data_json: r#"{"ref":"r-1"}"#.into(),
            },
        )
        .await
    {
        Ok(GateOutput::Engine(result)) => result,
        other => panic!("an engine result: {other:?}"),
    };
    let row_id = uuid::Uuid::parse_str(
        &j.store
            .reader()
            .unwrap()
            .row_receipt(&j.step_run, 1)
            .unwrap()
            .expect("the insert's receipt")
            .row_id,
    )
    .unwrap();
    let second = j.issued(2);
    let update = PreparedRow::Update {
        row_id,
        expected_version: 1,
        patch_json: r#"{"ref":"r-2"}"#.into(),
    };
    let sent_update = match j.write(&second, update.clone()).await {
        Ok(GateOutput::Engine(result)) => result,
        other => panic!("an engine result: {other:?}"),
    };
    let rows_before = Reader::open(&j.db)
        .unwrap()
        .table_rows_of("entries")
        .unwrap();

    let replayed_insert = j
        .write(
            &first,
            PreparedRow::Insert {
                data_json: r#"{"ref":"r-9"}"#.into(),
            },
        )
        .await;
    let replayed_update = j.write(&second, update).await;

    let reader = Reader::open(&j.db).unwrap();
    for (seq, sent, replayed, status, version) in [
        (1, &sent_insert, replayed_insert, RowStatus::Inserted, 1),
        (2, &sent_update, replayed_update, RowStatus::Updated, 2),
    ] {
        let replayed = match replayed {
            Ok(GateOutput::Engine(result)) => result,
            other => panic!("call {seq}: the replay is an engine result: {other:?}"),
        };
        assert_eq!(&replayed, sent, "call {seq}: the first run's result");
        assert_eq!(
            replayed,
            EngineResult::row_written(&RowWritten {
                row_id,
                version,
                status,
            }),
            "call {seq}: the first write's word"
        );
        assert_eq!(
            engine_result_of(&reader, &j.step_run, seq).unwrap(),
            Some(replayed),
            "call {seq}: what a resume rebuilds"
        );
    }
    assert_eq!(
        reader.table_rows_of("entries").unwrap(),
        rows_before,
        "a replay writes nothing"
    );
}

/// A runner whose manifests declare, at a limit of its own, the ids given.
struct Declaring {
    manifests: Vec<(&'static str, Declared, u32)>,
    ran: Mutex<Vec<String>>,
}

impl ToolRunner for Declaring {
    fn manifests(&self) -> Vec<DomainManifest> {
        self.manifests
            .iter()
            .map(|(id, class, max)| DomainManifest {
                id: id.to_string(),
                class: *class,
                network: NetworkNeed::None,
                max_uses_per_step: *max,
            })
            .collect()
    }

    fn run(
        &self,
        call: ToolCall,
        _cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<UntrustedData, String>> {
        self.ran.lock().unwrap().push(call.name.clone());
        Box::pin(async move { Ok(UntrustedData::text(format!("tool:{}", call.name), "ran")) })
    }
}

/// R5: a runner that declares `table_write` (as a read tool, with a use
/// limit far past the built-in's 100) and `web_search` (a built-in read
/// tool), beside its own `lookup`, never takes either id over. With the
/// step granted all three and declaring `entries`:
/// - `lookup` is admitted and runs on the runner, as before;
/// - a `table_write` call is either refused or written through the
///   row-writer port as the built-in (an engine result), never run on the
///   runner, and from its 101st call in one step run it is refused (the
///   built-in's limit holds);
/// - a `web_search` call never runs on the runner.
#[tokio::test]
async fn a_runner_tool_named_like_a_built_in_is_refused() {
    let runner = Arc::new(Declaring {
        manifests: vec![
            ("table_write", Declared::Read, 10_000),
            ("web_search", Declared::Read, 10_000),
            ("lookup", Declared::Read, 100),
        ],
        ran: Mutex::new(Vec::new()),
    });
    let written = RowWritten {
        row_id: uuid::Uuid::now_v7(),
        version: 1,
        status: RowStatus::Inserted,
    };
    let rows = Rows::answering(Ok(written));
    let gate = ToolGate::over(
        runner.clone(),
        rows.clone(),
        Some(Arc::new(Tool)),
        CollectAudit::new(),
    );
    let step = facts(&["table_write", "web_search", "lookup"], Some(entries()));

    let permit = gate
        .admit(&seat(), &step, &place(), &call("lookup"))
        .expect("the runner's own tool is admitted");
    let ran = gate.run(permit, cancel()).await;
    assert!(matches!(ran, Ok(GateOutput::Untrusted(_))), "{ran:?}");

    let mut admitted = 0;
    for n in 1..=101 {
        let at = CallPlace {
            call_seq: n + 1,
            ..place()
        };
        match gate.admit(&seat(), &step, &at, &write_call("entries")) {
            Ok(permit) => {
                admitted += 1;
                let out = gate.run(permit, cancel()).await;
                assert!(
                    matches!(out, Ok(GateOutput::Engine(_))),
                    "call {n}: table_write is the built-in, written through the port: {out:?}"
                );
            }
            Err(words) => assert!(words.contains("table_write"), "call {n}: {words}"),
        }
    }
    assert!(
        admitted <= 100,
        "the built-in's limit holds: {admitted} admitted"
    );

    let search = ToolCall {
        arguments_json: r#"{"query":"x"}"#.into(),
        ..call("web_search")
    };
    if let Ok(permit) = gate.admit(&seat(), &step, &place(), &search) {
        let _ = gate.run(permit, cancel()).await;
    }
    assert_eq!(
        runner.ran.lock().unwrap().clone(),
        vec!["lookup".to_string()],
        "the runner ran only its own tool"
    );
    assert!(rows.asked().len() <= 100, "{}", rows.asked().len());
}
