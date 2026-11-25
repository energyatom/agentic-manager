//! Epic 3 story 6's ToolGate slice (AD-11, AD-21, AD-46; plan matrix rows 7
//! and 34, owner Q1): `agentic run`'s plan builder maps each step's seat
//! grants, the seat's team and the declared table (its columns, from the
//! config snapshot), and wires am-tools' `table_write` as the row tool, so
//! the shipped binary's one tool runs through ToolGate and the engine's
//! port implementation.
//!
//! On a stream copy of `fixtures/projects/rows-documents` (teams `core` and
//! `side`; seat `writer` of `core` with the Tables capability on; tables
//! `entries` of `core` and `others` of `side`; workflow `record`: `collect`
//! declares `table: entries`, `plain` declares nothing), run in process
//! through `am_app::cli::run` with am-testkit's `FakeLlm` serving every
//! model:
//!
//! - `collect` is offered exactly am-tools' `table_write` spec for
//!   `entries` as the fixture declares it (every column, type, required
//!   flag and status value), and its row lands as the seat `writer`;
//!   `plain` is offered no tool;
//! - a `table_write` asked for by `plain` is refused by ToolGate ("declares
//!   no table") and the run fails;
//! - a step that declares another team's table is a config error: nothing
//!   runs and no row is written;
//! - story 3.6c: the plan builder fills no step's `row_inputs` slot, so no
//!   step of `agentic run` reads a row (3.15 and 7.2 fill it from config);
//! - the fan-out workflow `spread` (its step `share` lists `writer`, which
//!   holds `table_write`) is refused before any run or provider, in the
//!   words of `agentic validate`'s warning (matrix row 40).
//!
//! No network, no real model. Written before the keys, the mapping or the
//! row tool exist.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_app::cli::{RunContext, run};
use am_app::runtime::Places;
use am_config::ModelRef;
use am_domain::rows::{ColumnKind, ColumnSpec, DeclaredTable, RowTool};
use am_domain::{Actor, LlmProvider, RuntimeLayout};
use am_store::Reader;
use am_testkit::{FakeLlm, FakeProjectIdentity, FakeTerminal, ScriptedTurn};
use am_tools::table_write::TableWrite;

const INSERT: &str = r#"{"table":"entries","data":{"ref":"r-1","title":"from the command line"}}"#;

struct Env {
    dir: tempfile::TempDir,
    places: Places,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let places = Places::under(&home.join("Library/Application Support"), &home);
    Env { dir, places }
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

impl Ran {
    fn all(&self) -> String {
        format!("{}\n{}", self.out, self.err)
    }
}

impl Env {
    /// A stream copy of `fixtures/projects/rows-documents`, never opened.
    fn fixture(&self) -> PathBuf {
        let root = self.dir.path().join("projects").join("rows-documents");
        am_testkit::copy_tree(&am_testkit::fixture("projects/rows-documents"), &root);
        root.canonicalize().unwrap()
    }

    fn layout(&self, root: &Path) -> Option<RuntimeLayout> {
        let id = am_config::read_company(root).ok()?.project_id?;
        Some(self.places.base.project(&id))
    }

    /// The project's journal, once it exists.
    fn reader(&self, root: &Path) -> Option<Reader> {
        let db = self.layout(root)?.state_db();
        db.exists().then(|| Reader::open(&db).unwrap())
    }

    /// `agentic run record` on the fixture, every model served by `llm`.
    async fn run(&self, root: &Path, llm: &Arc<FakeLlm>) -> Ran {
        self.run_workflow(root, "record", llm).await
    }

    /// `agentic run <workflow>` on the fixture, every model served by `llm`.
    async fn run_workflow(&self, root: &Path, workflow: &str, llm: &Arc<FakeLlm>) -> Ran {
        let served = llm.clone();
        let providers: am_app::cli::Providers =
            Arc::new(move |_model: &ModelRef| Ok(served.clone() as Arc<dyn LlmProvider>));
        let ctx = RunContext {
            places: self.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(FakeTerminal::new()),
            providers,
            clock: am_testkit::FakeClock::at("2026-11-25T09:00:00Z"),
        };
        let argv = vec![
            workflow.to_string(),
            "--project".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
}

/// `entries` as the fixture's `tables/entries.yaml` declares it.
fn entries() -> DeclaredTable {
    let column = |id: &str, kind: ColumnKind, required: bool, values: &[&str]| ColumnSpec {
        id: id.into(),
        kind,
        required,
        values: values.iter().map(|v| v.to_string()).collect(),
    };
    DeclaredTable {
        id: "entries".into(),
        team: "core".into(),
        columns: vec![
            column("ref", ColumnKind::Text, true, &[]),
            column("title", ColumnKind::Text, false, &[]),
            column("amount", ColumnKind::Number, false, &[]),
            column("count", ColumnKind::Integer, false, &[]),
            column("done", ColumnKind::Boolean, false, &[]),
            column("due", ColumnKind::Date, false, &[]),
            column("link", ColumnKind::Url, false, &[]),
            column("state", ColumnKind::Status, false, &["new", "done"]),
            column("extra", ColumnKind::Json, false, &[]),
        ],
    }
}

/// The names of the tools each request offered, in call order.
fn offered(llm: &FakeLlm) -> Vec<Vec<String>> {
    llm.requests()
        .iter()
        .map(|r| r.tools.iter().map(|t| t.name.clone()).collect())
        .collect()
}

/// Row 34 (owner Q1): the plan maps the seat's grants and team, the
/// declared table's columns and the declared document, and wires am-tools'
/// row tool. `collect`'s first request offers exactly
/// `TableWrite::new().spec(&entries)` for the table as the fixture declares
/// it; its call lands one row whose after-image names the seat `writer`
/// (an `Actor::Seat` of this run and step) and `collect`'s attempt; the run
/// succeeds.
#[tokio::test]
async fn plan_of_maps_grants_team_table_and_the_row_tool() {
    let env = env();
    let root = env.fixture();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("recording").tool_call("table_write", INSERT),
        ScriptedTurn::text("recorded"),
        ScriptedTurn::text("One entry was recorded."),
    ]));

    let ran = env.run(&root, &llm).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let requests = llm.requests();
    assert_eq!(
        requests[0].tools,
        vec![TableWrite::new().spec(&entries())],
        "collect is offered am-tools' spec for the declared table"
    );
    let reader = env.reader(&root).expect("the run's journal");
    let run_row = reader.runs().unwrap().remove(0);
    assert_eq!(run_row.status, "succeeded");
    let steps = reader.steps(&run_row.id).unwrap();
    let collect = steps.iter().find(|s| s.step_key == "collect").unwrap();

    let rows = reader.table_rows_of("entries").unwrap();
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert!(
        rows[0].data.contains("from the command line"),
        "{}",
        rows[0].data
    );
    let history = reader.row_history_of("entries", &rows[0].row_id).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].seat.as_deref(), Some("writer"));
    assert_eq!(history[0].step_run_id.as_deref(), Some(collect.id.as_str()));
    match &history[0].actor {
        Actor::Seat { seat, run, step } => {
            assert_eq!(
                (seat.as_str(), run.to_string(), step.as_str()),
                ("writer", run_row.id.clone(), "collect")
            );
        }
        other => panic!("the seat wrote it: {other:?}"),
    }
    let attempt_writes = reader
        .attempt_writes(&collect.id)
        .unwrap()
        .expect("journaled");
    assert_eq!(attempt_writes.seat, "writer");
    assert_eq!(attempt_writes.table.as_deref(), Some("entries"));
}

/// Row 34 (owner Q1): only the step that declares a table is offered
/// `table_write`: `collect` is, `plain` is not. A
/// `table_write` that `plain`'s model asks for anyway is refused by
/// ToolGate in its words (`plain` "declares no table"); the run fails and
/// no row of `plain`'s is written.
#[tokio::test]
async fn agentic_run_offers_table_write_only_to_a_declaring_step() {
    let (first, second) = (env(), env());
    let root = first.fixture();
    let ok = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("nothing to record"),
        ScriptedTurn::text("Nothing was recorded."),
    ]));

    let ran = first.run(&root, &ok).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(offered(&ok), vec![vec!["table_write".to_string()], vec![]]);

    let root = second.fixture();
    let asks = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("nothing to record"),
        ScriptedTurn::text("recording").tool_call("table_write", INSERT),
        ScriptedTurn::text("never reached"),
    ]));

    let ran = second.run(&root, &asks).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    let said = ran.all();
    assert!(
        said.contains("table_write")
            && said.contains("declares no table")
            && said.contains("plain"),
        "{said}"
    );
    let reader = second.reader(&root).expect("the run's journal");
    assert!(reader.table_rows_of("entries").unwrap().is_empty());
    let run_row = reader.runs().unwrap().remove(0);
    assert_eq!(run_row.status, "failed");
    let plain = reader
        .steps(&run_row.id)
        .unwrap()
        .into_iter()
        .find(|s| s.step_key == "plain")
        .unwrap();
    let tools = reader.tool_calls(&plain.id).unwrap();
    assert_eq!(tools.len(), 1, "{tools:#?}");
    assert_eq!(tools[0].status, "refused");
}

/// Story 3.6c (plan Intent and Boundaries; owner Q4's reader condition):
/// no config key names rows a step reads yet, so the shipped plan builder
/// leaves every step's `row_inputs` slot empty (3.15 and 7.2 fill it).
/// `collect` writes a row whose title holds a canary; `plain`, the step
/// after it, journals no `row` edge and no `from_row`, its prompt holds no
/// `row:` part, and neither request holds the canary (only `collect`'s own
/// replayed call does, in its second request).
#[tokio::test]
async fn agentic_run_leaves_the_row_input_slot_empty() {
    const CANARY: &str = "canary-cli-row-0c3e";
    let env = env();
    let root = env.fixture();
    let insert = format!(r#"{{"table":"entries","data":{{"ref":"r-1","title":"{CANARY}"}}}}"#);
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("recording").tool_call("table_write", &insert),
        ScriptedTurn::text("recorded"),
        ScriptedTurn::text("One entry was recorded."),
    ]));

    let ran = env.run(&root, &llm).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let reader = env.reader(&root).expect("the run's journal");
    let rows = reader.table_rows_of("entries").unwrap();
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert!(rows[0].data.contains(CANARY));
    let run_row = reader.runs().unwrap().remove(0);
    for step in reader.steps(&run_row.id).unwrap() {
        for input in reader.step_inputs(&step.id).unwrap() {
            assert!(
                input.kind != "row" && input.from_row.is_none(),
                "{}: {input:?}",
                step.step_key
            );
        }
        for part in reader.prompt_parts(&step.id).unwrap() {
            assert!(
                !part.label.starts_with("row:"),
                "{}: {part:?}",
                step.step_key
            );
        }
    }
    let requests = llm.requests();
    assert_eq!(requests.len(), 3, "collect's two turns, then plain's");
    assert!(
        !format!("{:?}", requests[0]).contains(CANARY),
        "collect's first request"
    );
    assert!(
        !format!("{:?}", requests[2]).contains(CANARY),
        "plain's request holds no row text: {:#?}",
        requests[2]
    );
}

/// Row 7 (AD-21: a seat writes only its own team's tables): `collect`
/// declares `others`, a table of team `side`, while its seat `writer` is in
/// `core`. `agentic run` refuses it as a config error naming the workflow
/// file and the table: exit 1, no model called, no run and no row.
#[tokio::test]
async fn a_step_declaring_another_teams_table_is_refused_before_any_row() {
    let env = env();
    let root = env.fixture();
    let wf = root.join("workflows/record.yaml");
    let text = std::fs::read_to_string(&wf).unwrap();
    assert!(text.contains("table: entries"));
    std::fs::write(&wf, text.replace("table: entries", "table: others")).unwrap();
    let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::text("never")]));

    let ran = env.run(&root, &llm).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    let said = ran.all();
    assert!(
        said.contains("workflows/record.yaml") && said.contains("others"),
        "{said}"
    );
    assert_eq!(llm.calls(), 0);
    if let Some(reader) = env.reader(&root) {
        assert!(reader.runs().unwrap().is_empty(), "no run");
        assert!(reader.table_rows_of("entries").unwrap().is_empty());
        assert!(reader.table_rows_of("others").unwrap().is_empty());
    }
}

/// Row 40 (owner, 2026-11-24): a fan-out step that would write a table
/// cannot run until story 3.14. `agentic validate --template` on the
/// fixture exits 0 with a warning at `workflows/spread.yaml` saying so,
/// with a fix; `agentic run spread` is refused in the same words, the
/// warning's message and its fix, from the one am-config function both
/// use. It is refused first, before `agentic run`'s generic refusal of
/// run-graph keys and before any provider: exit 1, no model called, no
/// run and no row.
#[tokio::test]
async fn a_fan_out_step_that_writes_a_table_is_refused_with_the_validate_words() {
    let env = env();
    let root = env.fixture();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = am_app::cli::validate(
        &[
            "--template".to_string(),
            root.to_string_lossy().into_owned(),
        ],
        &mut out,
        &mut err,
    );
    let out = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{out}{}", String::from_utf8_lossy(&err));
    let lines: Vec<&str> = out.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("workflows/spread.yaml:") && l.contains(": warning: "))
        .unwrap_or_else(|| panic!("a warning at workflows/spread.yaml:\n{out}"));
    let message = lines[at]
        .split_once(": warning: ")
        .map(|(_, m)| m.trim().to_string())
        .unwrap();
    let fix = lines
        .get(at + 1)
        .and_then(|l| l.trim_start().strip_prefix("fix: "))
        .unwrap_or_else(|| panic!("a fix after the warning:\n{out}"))
        .trim()
        .to_string();
    assert!(
        message.contains("share") && message.contains("3.14"),
        "{message}"
    );
    let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::text("never")]));

    let ran = env.run_workflow(&root, "spread", &llm).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    let said = ran.all();
    assert!(
        said.contains(&message),
        "the warning's words: {message}\n{said}"
    );
    assert!(said.contains(&fix), "the warning's fix: {fix}\n{said}");
    assert!(
        !said.contains("cannot honour yet"),
        "refused by the fan-out check, before the run-graph keys: {said}"
    );
    assert_eq!(llm.calls(), 0, "no model is called");
    if let Some(reader) = env.reader(&root) {
        assert!(reader.runs().unwrap().is_empty(), "no run");
        assert!(reader.table_rows_of("entries").unwrap().is_empty());
    }
}

/// Review round 1, R8 (manager, 2026-11-25): `agentic validate` and
/// `agentic run` take one fan-out decision. A workflow whose one step has
/// `over_folder: ~` (written with no value), its own `seat: writer` and
/// `table: entries` validates with no error. If validate warns that the
/// step is a fan-out step that would write a table, `agentic run` refuses
/// it in exactly those words (message and fix); if validate does not,
/// `agentic run` says nothing of the kind (no `3.14`), whatever else it
/// decides about the step.
#[tokio::test]
async fn an_empty_over_folder_gets_one_decision_from_validate_and_run() {
    let env = env();
    let root = env.fixture();
    std::fs::write(
        root.join("workflows/odd.yaml"),
        "schema_version: 2\nname: Odd\nteam: core\ndescription: A step whose over_folder key has no value.\nsteps:\n  - id: odd\n    seat: writer\n    over_folder: ~\n    instruction: Record the entries.\n    table: entries\n",
    )
    .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = am_app::cli::validate(
        &[
            "--template".to_string(),
            root.to_string_lossy().into_owned(),
        ],
        &mut out,
        &mut err,
    );
    let out = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{out}{}", String::from_utf8_lossy(&err));
    let lines: Vec<&str> = out.lines().collect();
    let warning = lines.iter().position(|l| {
        l.starts_with("workflows/odd.yaml:") && l.contains(": warning: ") && l.contains("3.14")
    });
    let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::text("never")]));

    let ran = env.run_workflow(&root, "odd", &llm).await;

    let said = ran.all();
    match warning {
        Some(at) => {
            let message = lines[at].split_once(": warning: ").unwrap().1.trim();
            let fix = lines[at + 1]
                .trim_start()
                .strip_prefix("fix: ")
                .unwrap()
                .trim();
            assert_eq!(ran.code, 1, "{said}");
            assert!(
                said.contains(message),
                "validate's words: {message}\n{said}"
            );
            assert!(said.contains(fix), "validate's fix: {fix}\n{said}");
            assert_eq!(llm.calls(), 0);
        }
        None => assert!(
            !said.contains("3.14"),
            "validate gave no fan-out warning, so agentic run gives no fan-out refusal:\n{out}\n{said}"
        ),
    }
}
