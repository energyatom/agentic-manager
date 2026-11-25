//! Epic 3 story 4: `agentic gate [<item id>] [--project <folder>]`, run in
//! process through `am_app::cli::gate` with am-testkit's `FakeTerminal`
//! and `FakeLlm` injected through its context (the fakes never reach the
//! binary), on stream copies of `fixtures/projects/hand-offs` (its `gated`
//! and `returned` workflows: `draft` on `first`, the gate `approve` on the
//! `owner` seat, `publish` on `second`, every model on `local-lmstudio`).
//! The real binary's rows are in `gate_tty.rs`. No network, no real model.
//! Written before the command exists.
//!
//! What these tests pin:
//!
//! ```text
//! am_app::cli::GateContext {
//!     places: Places,
//!     identity: Arc<dyn ProjectIdentity>,
//!     terminal: Box<dyn Terminal>,
//!     providers: am_app::cli::Providers,
//!     clock: Arc<dyn am_domain::Clock>,   // story 3.24: the continued run's clock
//! }
//! am_app::cli::gate(args: &[String], ctx: GateContext, out, err).await -> i32
//!     args: [<item id>] [--project <folder>]
//! ```
//!
//! - Checks, in order: usage (2); stdin and stdout an interactive terminal
//!   (nothing read, the project not opened); no enabled stdio MCP server
//!   or `render: true` Source (a broken file fails closed), pointing at the
//!   app's Inbox; the project opens as `agentic run` opens it (refused
//!   while held, naming `agentic serve`); for approve and return, before
//!   anything is written, `agentic run`'s checks on the run's pinned
//!   snapshot (its hash equal to the current one, no config error).
//! - The terminal (`Terminal::show`) shows each open item (Review, then
//!   Question, then Failure; oldest first) with its id, run, workflow,
//!   step and attempt, the engine's words, for a Review the returns so far
//!   (`0 of 2`) and its payload with control characters other than newline
//!   and tab neutralised; a tainted Review says, above the payload and
//!   again at the choice, that its output came from untrusted content.
//! - One choice per item, read with a 10-minute limit: `a` approve (a
//!   tainted Review then needs its item id typed back, surrounding spaces
//!   trimmed); `r` return (past `max_returns` first a warning that the run
//!   will stop with a Failure item, confirmed by `y`; then the note); `j`
//!   reject, confirmed by typing `reject`; `d` dismiss (a Failure); `answer`
//!   then the answer (a Question); `s` skip. Ctrl-C, end of input or the
//!   time limit write nothing for that item; Ctrl-C reported between the
//!   last prompt and the write (`Terminal::interrupted`) too.
//! - It resolves as You (Reviewer), then continues the run in the
//!   foreground and shows an item the continuation raises in the same
//!   session; it prints `run <id> <status>` on stdout. Exit 0 when each
//!   decision was recorded and each continued run succeeded, waits again or
//!   ended by the owner's reject; 1 on a refusal, an interrupt, a timeout or
//!   a continued run that failed otherwise (past `max_returns` included).
//! - `agentic status` prints every item with its status and, when resolved,
//!   its decision and "You (Reviewer)", never a note; a run an API resolve
//!   left ready says "ready to continue". `agentic run` at a gate adds one
//!   line naming `agentic gate`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use am_app::cli::{GateContext, RunContext, gate, run, status};
use am_app::runtime::{OpenRequest, Opened, Places, Prompt, open_project};
use am_config::ModelRef;
use am_domain::runtime::open_marker;
use am_domain::{
    Actor, Decision, HumanActor, InboxKind, LlmProvider, Part, REVIEWER_HAT, RuntimeLayout, Taint,
};
use am_store::{
    InboxItemRow, NewInboxItem, Reader, RunRow, StepInput, StepRow, WriteCommand, new_id,
};
use am_testkit::{FakeLlm, FakeProjectIdentity, FakeTerminal, ScriptedTurn};

/// The provider factory of the command contexts.
type Providers = Arc<dyn Fn(&ModelRef) -> Result<Arc<dyn LlmProvider>, String> + Send + Sync>;

/// Every prompt's idle limit (Decision 5).
const TEN_MINUTES: Duration = Duration::from_secs(600);
const DRAFT_1: &str = "the version to review";
const NOTE: &str = "tighten the summary";
/// The words a tainted Review is shown with (owner condition 2).
const MARKER: &str = "came from untrusted content";
const STDIO_TOOLS: &str = "schema_version: 2\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n";

/// The open-time prompt of the setup: never asked.
struct NoPrompt;

impl Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// What one command printed.
#[derive(Debug, Clone)]
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

/// A scripted model whose turns are plain texts, in order (the last
/// repeats).
fn texts(turns: &[&str]) -> Arc<FakeLlm> {
    Arc::new(FakeLlm::new(
        turns.iter().map(|t| ScriptedTurn::text(*t)).collect(),
    ))
}

/// Every model on `local-lmstudio` is served by `llm`.
fn local(llm: Arc<FakeLlm>) -> Providers {
    Arc::new(move |model: &ModelRef| {
        if model.account == "local-lmstudio" {
            Ok(llm.clone() as Arc<dyn LlmProvider>)
        } else {
            Err(format!("no provider for Account {}", model.account))
        }
    })
}

/// True when `text` names the number `n` as a word of its own.
fn names_number(text: &str, n: u64) -> bool {
    let n = n.to_string();
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == n)
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn edit(root: &Path, rel: &str, from: &str, to: &str) {
    let text = std::fs::read_to_string(root.join(rel)).unwrap();
    assert!(text.contains(from), "{rel} has no `{from}`");
    write(root, rel, &text.replacen(from, to, 1));
}

/// The hash of the config snapshot the project's files give now.
fn snapshot_hash(root: &Path) -> String {
    let loaded = am_config::load(root).unwrap();
    loaded
        .snapshot
        .unwrap_or_else(|| panic!("{:#?}", loaded.findings))
        .hash()
        .to_string()
}

/// One audit export line's action.
fn action_of(line: &str) -> Option<String> {
    let start = line.find("\"action\":\"")? + "\"action\":\"".len();
    let end = line[start..].find('"')?;
    Some(line[start..start + end].to_string())
}

fn by_reviewer(line: &str) -> bool {
    line.contains("\"kind\":\"human\"")
        && line.contains("\"person\":\"owner\"")
        && line.contains("\"hat\":\"Reviewer\"")
}

fn by_engine(line: &str) -> bool {
    line.contains("\"kind\":\"system\"") && line.contains("\"component\":\"engine\"")
}

/// The `(key, attempt, status)` of each attempt.
fn shape(steps: &[StepRow]) -> Vec<(String, i64, String)> {
    steps
        .iter()
        .map(|s| (s.step_key.clone(), s.attempt, s.status.clone()))
        .collect()
}

fn want(rows: &[(&str, i64, &str)]) -> Vec<(String, i64, String)> {
    rows.iter()
        .map(|(k, a, s)| (k.to_string(), *a, s.to_string()))
        .collect()
}

/// A temporary HOME with its places, and a folder for projects.
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

impl Env {
    /// A stream copy of `fixtures/projects/<fixture>`, never opened here.
    fn copy_of(&self, fixture: &str) -> PathBuf {
        let root = self.dir.path().join("projects").join(fixture);
        am_testkit::copy_tree(&am_testkit::fixture(&format!("projects/{fixture}")), &root);
        root.canonicalize().unwrap()
    }

    fn layout(&self, root: &Path) -> RuntimeLayout {
        let id = am_config::read_company(root)
            .unwrap()
            .project_id
            .expect("the project was opened once");
        self.places.base.project(&id)
    }

    fn db(&self, root: &Path) -> PathBuf {
        self.layout(root).state_db()
    }

    fn reader(&self, root: &Path) -> Reader {
        Reader::open(&self.db(root)).unwrap()
    }

    fn runs(&self, root: &Path) -> Vec<RunRow> {
        self.reader(root).runs().unwrap()
    }

    fn steps(&self, root: &Path, run: &str) -> Vec<StepRow> {
        self.reader(root).steps(run).unwrap()
    }

    fn run_status(&self, root: &Path, run: &str) -> String {
        self.reader(root).run(run).unwrap().unwrap().status
    }

    /// Every item of every run.
    fn items(&self, root: &Path) -> Vec<InboxItemRow> {
        let reader = self.reader(root);
        let mut all = Vec::new();
        for run in reader.runs().unwrap() {
            all.extend(reader.inbox_items(&run.id).unwrap());
        }
        all
    }

    fn item(&self, root: &Path, id: &str) -> InboxItemRow {
        self.reader(root)
            .inbox_item(id)
            .unwrap()
            .unwrap_or_else(|| panic!("no item {id}"))
    }

    /// The audit export, one line per entry.
    fn audit(&self, root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        self.reader(root).export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn resolved_entries(&self, root: &Path) -> Vec<String> {
        self.audit(root)
            .into_iter()
            .filter(|l| action_of(l).as_deref() == Some("inbox.resolved"))
            .collect()
    }

    /// The item is open, with no decision, and its step waits.
    fn assert_open(&self, root: &Path, id: &str) {
        let item = self.item(root, id);
        assert_eq!(item.status, "open", "{item:?}");
        assert_eq!(
            (item.decision, item.note, item.resolved_by),
            (None, None, None)
        );
        let step = self
            .reader(root)
            .steps(&item.run_id)
            .unwrap()
            .into_iter()
            .find(|s| s.id == item.step_run_id)
            .unwrap();
        if item.kind == InboxKind::Review {
            assert_eq!(step.status, "waiting_human");
        }
    }

    /// The project is closed again: no marker, and nothing holds its lock.
    fn assert_closed(&self, root: &Path) {
        assert!(!open_marker(root).exists(), "the marker is removed");
        assert!(
            !am_store::lock_is_held(&self.layout(root)),
            "the lock is released"
        );
    }

    /// Opens the project in process (for System store commands that make
    /// what the binary cannot: a tainted Review, a Question, a Failure).
    fn open(&self, root: &Path) -> Opened {
        let req = OpenRequest {
            root: root.to_path_buf(),
            confirm_move: false,
            take_over: false,
        };
        open_project(
            &self.places,
            &FakeProjectIdentity::new("Test Mac"),
            &req,
            &mut NoPrompt,
        )
        .unwrap()
    }

    /// `agentic run <workflow> --project <root>` on an interactive terminal.
    async fn run(&self, root: &Path, workflow: &str, llm: Arc<FakeLlm>) -> Ran {
        let argv = vec![
            workflow.to_string(),
            "--project".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let ctx = RunContext {
            places: self.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(FakeTerminal::new()),
            providers: local(llm),
            clock: am_testkit::FakeClock::at("2026-11-25T09:00:00Z"),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// `agentic gate <args> --project <root>` on the terminal `term`.
    async fn gate(
        &self,
        root: &Path,
        args: &[&str],
        term: &FakeTerminal,
        llm: Arc<FakeLlm>,
    ) -> Ran {
        let mut argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        argv.push("--project".into());
        argv.push(root.to_string_lossy().into_owned());
        self.gate_argv(&argv, term, local(llm)).await
    }

    async fn gate_argv(&self, argv: &[String], term: &FakeTerminal, providers: Providers) -> Ran {
        let ctx = GateContext {
            places: self.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(term.clone()),
            providers,
            clock: am_testkit::FakeClock::at("2026-11-25T09:00:00Z"),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = gate(argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// `agentic status --project <root>`.
    fn status(&self, root: &Path) -> Ran {
        let argv = vec!["--project".to_string(), root.to_string_lossy().into_owned()];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = status(&argv, &self.places, &mut out, &mut err);
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
}

/// `agentic run <workflow>` on `llm`, which waits at its gate: the run's id
/// and its open Review item's.
async fn waiting(env: &Env, root: &Path, workflow: &str, llm: &Arc<FakeLlm>) -> (String, String) {
    let ran = env.run(root, workflow, llm.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let line = ran
        .out
        .lines()
        .find(|l| l.starts_with("run "))
        .unwrap_or_else(|| panic!("no run line: {}", ran.all()));
    let words: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(words.get(2), Some(&"waiting_human"), "{line}");
    let run = words[1].to_string();
    let item = env
        .items(root)
        .into_iter()
        .find(|i| i.run_id == run && i.status == "open")
        .expect("its Review item")
        .id;
    (run, item)
}

/// A run of `gated` made by System store commands, as the engine would
/// make it (the binary has no taint source yet; tests make one, as in
/// story 3.3): `draft` read an untrusted page and wrote `output`; the gate
/// `approve` waits with a tainted Review item (`max_returns` 2) whose
/// payload is that output. The run pins the current snapshot's hash.
fn tainted_waiting(env: &Env, root: &Path, output: &str) -> (String, String) {
    let opened = env.open(root);
    let w = opened.store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "gated".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(snapshot_hash(root)),
        },
    )
    .unwrap();
    let steps = opened.store.reader().unwrap().steps(&run).unwrap();
    let (draft, approve) = (steps[0].id.clone(), steps[1].id.clone());
    let call = new_id();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![StepInput {
                kind: "untrusted".into(),
                from_step_run: None,
                from_item: None,
                label: "test:poisoned-page".into(),
                taint: Taint::Tainted,
            }],
            taint: Taint::Tainted,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "first".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    w.exec(
        engine(),
        WriteCommand::ReserveCall {
            step_run_id: draft.clone(),
            call_id: call.clone(),
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
            call_id: call,
            usage: None,
            cost_micro_usd: None,
            step_run_id: draft.clone(),
            status: "succeeded".into(),
            output: Some(output.into()),
        },
    )
    .unwrap();
    let item = new_id();
    w.exec(
        engine(),
        WriteCommand::PauseStep {
            step_run_id: approve,
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(draft.clone()),
                from_item: None,
                label: "step:draft@1".into(),
                taint: Taint::Tainted,
            }],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Review,
                payload: Some(draft),
                taint: Taint::Tainted,
                title: "Review before step approve goes on".into(),
                body: "Step `approve` waits for your review of the output of step `draft`.".into(),
                max_returns: Some(2),
            },
        },
    )
    .unwrap();
    drop(w);
    drop(opened);
    (run, item)
}

/// A run of `gated` whose step `draft` waits with an open Question made by
/// a System pause (nothing creates one before stories 3.10 and 3.14).
fn question_waiting(env: &Env, root: &Path) -> (String, String) {
    let opened = env.open(root);
    let w = opened.store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "gated".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(snapshot_hash(root)),
        },
    )
    .unwrap();
    let draft = opened.store.reader().unwrap().steps(&run).unwrap()[0]
        .id
        .clone();
    let item = new_id();
    w.exec(
        engine(),
        WriteCommand::PauseStep {
            step_run_id: draft,
            inputs: vec![],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Question,
                payload: None,
                taint: Taint::Clean,
                title: "A question before step draft goes on".into(),
                body: "Step `draft` asks which source to use.".into(),
                max_returns: None,
            },
        },
    )
    .unwrap();
    drop(w);
    drop(opened);
    (run, item)
}

/// A run of `gated` whose step `draft` failed with an open Failure item
/// (the run failed), made by System commands.
fn failed_run(env: &Env, root: &Path) -> (String, String) {
    let opened = env.open(root);
    let w = opened.store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "gated".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(snapshot_hash(root)),
        },
    )
    .unwrap();
    let draft = opened.store.reader().unwrap().steps(&run).unwrap()[0]
        .id
        .clone();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "first".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    let item = new_id();
    w.exec(
        engine(),
        WriteCommand::FailWithItem {
            step_run_id: draft.clone(),
            settle: None,
            output: None,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Failure,
                payload: Some(draft),
                taint: Taint::Clean,
                title: "A step stopped".into(),
                body: "Step `draft` stopped at its limit of 8 iterations.".into(),
                max_returns: None,
            },
        },
    )
    .unwrap();
    drop(w);
    drop(opened);
    (run, item)
}

/// Row 1: on an interactive terminal the waiting Review is shown: its id,
/// run, workflow, step `approve` and attempt 1, the engine's words, the
/// returns so far `0 of 2` and `draft`'s output. Nothing is written until
/// a choice (skip here: one read, nothing changed, the project closed).
#[tokio::test]
async fn it_lists_the_waiting_review_with_its_payload() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let row = env.item(&root, &item);
    let before = (env.items(&root), env.steps(&root, &run));
    let t = FakeTerminal::new();
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let shown = t.shown();
    for word in [
        item.as_str(),
        run.as_str(),
        "gated",
        "approve",
        "attempt 1",
        row.title.as_str(),
        row.body.as_str(),
        "0 of 2",
        DRAFT_1,
    ] {
        assert!(shown.contains(word), "shows `{word}`:\n{shown}");
    }
    assert_eq!(t.reads(), 1, "one choice is read");
    assert_eq!((env.items(&root), env.steps(&root, &run)), before);
    assert!(env.resolved_entries(&root).is_empty());
    assert_eq!(llm.calls(), 1);
    env.assert_closed(&root);
}

/// Row 1 (Decision 5): the payload is model text, so it is shown with
/// every control character but newline and tab neutralised (escape
/// sequences, a bell, a carriage return, a C1 control); its visible words
/// remain.
#[tokio::test]
async fn the_payload_is_printed_with_control_characters_neutralised() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let raw = "first line\n\tindented \u{1b}[31mRED\u{1b}[0m a bell\u{7} a\rreturn \u{9b}2J end";
    let llm = texts(&[raw]);
    let (_, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let shown = t.shown();
    assert!(shown.contains(&item));
    for visible in ["first line", "indented", "RED", "a bell", "return", "end"] {
        assert!(shown.contains(visible), "`{visible}`:\n{shown}");
    }
    let controls: Vec<char> = shown
        .chars()
        .filter(|c| c.is_control() && *c != '\n' && *c != '\t')
        .collect();
    assert!(controls.is_empty(), "neutralised: {controls:?}\n{shown:?}");
}

/// Row 2: `a` on the waiting Review (named by its id): the item is resolved
/// `approve` by You (Reviewer), the gate succeeds, `publish` runs on its
/// model, the run succeeds and `run <id> succeeded` is printed; exit 0. An
/// untainted Review needs no more than `a` (one read). One
/// `inbox.resolved` entry by the owner as Reviewer; the run's entries
/// after it are the engine's.
#[tokio::test]
async fn approve_resumes_the_run() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "prepared"]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("a");

    let ran = env.gate(&root, &[item.as_str()], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.out.lines().any(|l| l == format!("run {run} succeeded")),
        "{}",
        ran.all()
    );
    assert_eq!(t.reads(), 1, "`a` alone approves an untainted Review");
    let row = env.item(&root, &item);
    assert_eq!(row.status, "resolved");
    assert_eq!(row.decision, Some(Decision::Approve));
    assert!(by_reviewer(row.resolved_by.as_deref().unwrap()), "{row:?}");
    assert_eq!(
        shape(&env.steps(&root, &run)),
        want(&[
            ("draft", 1, "succeeded"),
            ("approve", 1, "succeeded"),
            ("publish", 1, "succeeded"),
        ])
    );
    assert_eq!(env.run_status(&root, &run), "succeeded");
    assert_eq!(llm.calls(), 2);
    let audit = env.audit(&root);
    let at = audit
        .iter()
        .position(|l| action_of(l).as_deref() == Some("inbox.resolved"))
        .expect("an inbox.resolved entry");
    assert!(by_reviewer(&audit[at]), "{}", audit[at]);
    assert_eq!(env.resolved_entries(&root).len(), 1);
    for line in &audit[at + 1..] {
        let action = action_of(line).unwrap_or_default();
        if action.starts_with("step.")
            || action.starts_with("run.")
            || action.starts_with("model_call.")
        {
            assert!(by_engine(line), "{line}");
        }
    }
    env.assert_closed(&root);
}

/// Row 3: `r`, then the note: the item is resolved `return` with the note;
/// the gate attempt 1 is `looped_back`; `draft` runs again with the note
/// as a trusted part of its prompt (after its instruction); the gate waits
/// again with a new Review item, shown in the same session (skipped here);
/// exit 0.
#[tokio::test]
async fn return_reruns_the_producer_with_the_note_in_its_prompt() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "the second version"]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("r");
    t.line(NOTE);
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let row = env.item(&root, &item);
    assert_eq!(row.decision, Some(Decision::Return));
    assert_eq!(row.note.as_deref(), Some(NOTE));
    assert_eq!(
        shape(&env.steps(&root, &run)),
        want(&[
            ("draft", 1, "succeeded"),
            ("draft", 2, "succeeded"),
            ("approve", 1, "looped_back"),
            ("approve", 2, "waiting_human"),
            ("publish", 1, "queued"),
        ])
    );
    assert_eq!(llm.calls(), 2);
    let request = &llm.requests()[1];
    let user = request
        .messages
        .iter()
        .find(|m| m.role == am_domain::Role::User)
        .unwrap();
    assert!(matches!(user.parts[0], Part::Instruction(_)), "{user:#?}");
    let notes: Vec<&Part> = user
        .parts
        .iter()
        .filter(|p| matches!(p, Part::Trusted(t) if t.label() == "gate-note:approve@1" && t.text() == NOTE))
        .collect();
    assert_eq!(notes.len(), 1, "{user:#?}");
    let again = env
        .items(&root)
        .into_iter()
        .find(|i| i.run_id == run && i.status == "open")
        .expect("a new Review item");
    assert_ne!(again.id, item);
    assert!(t.shown().contains(&again.id), "shown in the same session");
    assert_eq!(env.run_status(&root, &run), "running");
    env.assert_closed(&root);
}

/// Rows 4 and 6 on `returned` (`max_returns: 1`): the first return loops
/// back; at the new Review a return would pass the maximum, so the gate
/// says the run will stop with a Failure item and asks to confirm:
/// anything but `y` writes nothing. Confirmed, the return is recorded, the
/// gate fails with one Failure item (the engine's words: step, max_returns,
/// 1, the Review item's id; never a note), the run fails; exit 1.
#[tokio::test]
async fn returning_past_max_returns_fails_with_one_failure_item() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&["v1", "v2", "v3"]);
    let (run, first) = waiting(&env, &root, "returned", &llm).await;
    let t = FakeTerminal::new();
    t.line("r");
    t.line("the first note");
    t.line("r");
    t.line("n");

    let _ = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(env.item(&root, &first).decision, Some(Decision::Return));
    let second = env
        .items(&root)
        .into_iter()
        .find(|i| i.run_id == run && i.status == "open" && i.kind == InboxKind::Review)
        .expect("the gate waits again")
        .id;
    let shown = t.shown();
    let at = shown.find(&second).expect("the new item is shown");
    assert!(
        shown[at..].contains("Failure item"),
        "the warning before the last return:\n{shown}"
    );
    env.assert_open(&root, &second);
    assert_eq!(env.resolved_entries(&root).len(), 1);
    assert_eq!(llm.calls(), 2);

    let t = FakeTerminal::new();
    t.line("r");
    t.line("y");
    t.line("the second note");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 1, "a run that failed: {}", ran.all());
    let row = env.item(&root, &second);
    assert_eq!(row.decision, Some(Decision::Return));
    assert_eq!(row.note.as_deref(), Some("the second note"));
    let steps = env.steps(&root, &run);
    let gate2 = steps
        .iter()
        .find(|s| s.step_key == "approve" && s.attempt == 2)
        .unwrap();
    assert_eq!(gate2.status, "failed");
    let failures: Vec<InboxItemRow> = env
        .items(&root)
        .into_iter()
        .filter(|i| i.run_id == run && i.kind == InboxKind::Failure)
        .collect();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    let body = &failures[0].body;
    assert!(
        body.contains("approve") && body.contains("max_returns"),
        "{body}"
    );
    assert!(names_number(body, 1), "{body}");
    assert!(body.contains(&second), "names the Review item: {body}");
    for note in ["the first note", "the second note"] {
        assert!(!body.contains(note) && !failures[0].title.contains(note));
    }
    assert_eq!(env.run_status(&root, &run), "failed");
    assert_eq!(llm.calls(), 2, "no third draft");
    env.assert_closed(&root);
}

/// Row 5: with `max_returns: 0` (written into a copy of `gated`) the first
/// return is past the maximum: after the warning and `y`, the gate fails
/// with one Failure item and the run fails; `draft` does not run again;
/// exit 1.
#[tokio::test]
async fn max_returns_zero_fails_on_the_first_return() {
    let env = env();
    let root = env.copy_of("hand-offs");
    edit(
        &root,
        "workflows/gated.yaml",
        "    gate: review\n",
        "    gate: review\n    max_returns: 0\n",
    );
    let llm = texts(&["v1"]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    assert_eq!(env.item(&root, &item).max_returns, Some(0));
    let t = FakeTerminal::new();
    t.line("r");
    t.line("y");
    t.line("the only note");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        t.shown().contains("Failure item"),
        "the warning:\n{}",
        t.shown()
    );
    assert_eq!(env.item(&root, &item).decision, Some(Decision::Return));
    assert_eq!(
        shape(&env.steps(&root, &run)),
        want(&[
            ("draft", 1, "succeeded"),
            ("approve", 1, "failed"),
            ("publish", 1, "queued"),
        ])
    );
    assert_eq!(
        env.items(&root)
            .iter()
            .filter(|i| i.kind == InboxKind::Failure)
            .count(),
        1
    );
    assert_eq!(env.run_status(&root, &run), "failed");
    assert_eq!(llm.calls(), 1);
}

/// Row 7 (Question 7): `j`, then `reject`: the item is resolved `reject`,
/// the gate and the run fail ("rejected" in what is printed), `publish`
/// stays queued, no Failure item; exit 0 (the owner ended it).
#[tokio::test]
async fn reject_ends_the_run() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("j");
    t.line("reject");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("rejected"), "{}", ran.all());
    assert_eq!(env.item(&root, &item).decision, Some(Decision::Reject));
    assert_eq!(
        shape(&env.steps(&root, &run)),
        want(&[
            ("draft", 1, "succeeded"),
            ("approve", 1, "failed"),
            ("publish", 1, "queued"),
        ])
    );
    assert_eq!(env.run_status(&root, &run), "failed");
    assert_eq!(env.items(&root).len(), 1, "no Failure item");
    assert_eq!(llm.calls(), 1);
    env.assert_closed(&root);
}

/// Row 8: `j` and anything but `reject` (nothing, a part of it, `yes`, `j`
/// again, more words) writes nothing; the item stays open.
#[tokio::test]
async fn reject_needs_its_confirmation() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (_, item) = waiting(&env, &root, "gated", &llm).await;
    for typed in ["", "rejec", "yes", "j", "reject it"] {
        let t = FakeTerminal::new();
        t.line("j");
        t.line(typed);

        let _ = env.gate(&root, &[], &t, llm.clone()).await;

        assert!(t.reads() >= 2, "{typed:?}: the confirmation was asked");
        env.assert_open(&root, &item);
        assert!(env.resolved_entries(&root).is_empty(), "{typed:?}");
        env.assert_closed(&root);
    }
}

/// Row 9: `s` writes nothing; exit 0.
#[tokio::test]
async fn skip_writes_nothing() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let before = (env.items(&root), env.steps(&root, &run));
    let t = FakeTerminal::new();
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!((env.items(&root), env.steps(&root, &run)), before);
    env.assert_open(&root, &item);
    assert!(env.resolved_entries(&root).is_empty());
}

/// Row 16: stdin or stdout not a terminal: refused in words naming the
/// terminal before anything is read or the project is opened (the
/// database untouched, no marker, no lock); exit 1.
#[tokio::test]
async fn a_non_interactive_terminal_is_refused_before_anything_is_read() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (_, item) = waiting(&env, &root, "gated", &llm).await;
    let db = env.db(&root);
    let bytes = std::fs::read(&db).unwrap();
    let t = FakeTerminal::piped();
    t.line("a");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("terminal"), "{}", ran.all());
    assert_eq!(t.reads(), 0, "nothing is read");
    assert_eq!(std::fs::read(&db).unwrap(), bytes, "not opened");
    env.assert_closed(&root);
    env.assert_open(&root, &item);
}

/// Row 17 (AD-13, AD-15): while the project's files enable a stdio MCP
/// server or a Source with `render: true` (programs that run as the
/// owner), or a `tools.yaml` has an error (fails closed), `agentic gate`
/// is refused on a terminal, naming it (and, for an enabled one, the app's
/// Inbox); nothing is read or written.
#[tokio::test]
async fn an_owner_child_moves_decisions_to_the_app() {
    let docs =
        std::fs::read_to_string(am_testkit::fixture("projects/sources-ok/sources/docs.yaml"))
            .unwrap();
    let pages =
        std::fs::read_to_string(am_testkit::fixture("projects/sources-ok/tables/pages.yaml"))
            .unwrap();
    // (what, files written, the name refused, whether the Inbox is named)
    type Case<'a> = (&'a str, Vec<(&'a str, &'a str)>, &'a str, bool);
    let cases: Vec<Case> = vec![
        (
            "a stdio MCP server",
            vec![("tools.yaml", STDIO_TOOLS)],
            "notes",
            true,
        ),
        (
            "a rendering Source",
            vec![
                ("sources/docs.yaml", docs.as_str()),
                ("tables/pages.yaml", pages.as_str()),
            ],
            "docs",
            true,
        ),
        (
            "a tools.yaml with an error",
            vec![("tools.yaml", "schema_version: 2\nmcp_servers: [\n")],
            "tools.yaml",
            false,
        ),
    ];
    for (what, files, name, inbox) in cases {
        let env = env();
        let root = env.copy_of("hand-offs");
        let llm = texts(&[DRAFT_1]);
        let (_, item) = waiting(&env, &root, "gated", &llm).await;
        for (rel, text) in files {
            write(&root, rel, text);
        }
        let t = FakeTerminal::new();
        t.line("a");

        let ran = env.gate(&root, &[], &t, llm.clone()).await;

        assert_eq!(ran.code, 1, "{what}: {}", ran.all());
        assert!(
            ran.err.contains(name),
            "{what}: names `{name}`: {}",
            ran.all()
        );
        if inbox {
            assert!(ran.err.contains("Inbox"), "{what}: {}", ran.all());
        }
        assert_eq!(t.reads(), 0, "{what}");
        env.assert_open(&root, &item);
        assert!(env.resolved_entries(&root).is_empty(), "{what}");
        env.assert_closed(&root);
    }
}

/// Row 18 (Question 6): while another process holds the project (as
/// `agentic serve` does), `agentic gate` is refused in words naming
/// `agentic serve`; nothing is read or written.
#[tokio::test]
async fn a_project_held_by_serve_is_refused() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (_, item) = waiting(&env, &root, "gated", &llm).await;
    let (held, _) = am_store::open_runtime(&env.layout(&root), "Holder Mac".into()).unwrap();
    let t = FakeTerminal::new();
    t.line("a");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("agentic serve"), "{}", ran.all());
    assert_eq!(t.reads(), 0);
    drop(held);
    env.assert_open(&root, &item);
    assert!(env.resolved_entries(&root).is_empty());
}

/// Row 31 (Question 2, AD-16): the run pinned its snapshot; a workflow
/// file edited while it waits changes the project's snapshot, so approve
/// and return are refused before anything is written, naming both hashes,
/// and no model is called; a config error refuses them too. Reject still
/// works.
#[tokio::test]
async fn approve_waits_while_the_config_differs_from_the_runs_pin() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "prepared"]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let pinned = env
        .reader(&root)
        .run(&run)
        .unwrap()
        .unwrap()
        .config_hash
        .expect("the pin");
    edit(
        &root,
        "workflows/gated.yaml",
        "Prepare the approved version.",
        "Prepare the approved version for print.",
    );
    let now = snapshot_hash(&root);
    assert_ne!(now, pinned);

    for choices in [vec!["a"], vec!["r", NOTE]] {
        let t = FakeTerminal::new();
        for c in &choices {
            t.line(c);
        }

        let ran = env.gate(&root, &[], &t, llm.clone()).await;

        assert_eq!(ran.code, 1, "{choices:?}: {}", ran.all());
        for hash in [&pinned, &now] {
            assert!(
                ran.err.contains(&hash[..12]),
                "{choices:?}: names {hash}: {}",
                ran.all()
            );
        }
        env.assert_open(&root, &item);
        assert!(env.resolved_entries(&root).is_empty());
        assert_eq!(llm.calls(), 1);
    }

    // A config error refuses approve too.
    let seat = std::fs::read_to_string(root.join("seats/second.md")).unwrap();
    write(
        &root,
        "seats/second.md",
        "---\nschema_version: 2\nname: [broken\n---\n",
    );
    let t = FakeTerminal::new();
    t.line("a");
    let ran = env.gate(&root, &[], &t, llm.clone()).await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    env.assert_open(&root, &item);
    write(&root, "seats/second.md", &seat);

    let t = FakeTerminal::new();
    t.line("j");
    t.line("reject");
    let ran = env.gate(&root, &[], &t, llm.clone()).await;
    assert_eq!(ran.code, 0, "reject still works: {}", ran.all());
    assert_eq!(env.item(&root, &item).decision, Some(Decision::Reject));
    assert_eq!(env.run_status(&root, &run), "failed");
    assert_eq!(llm.calls(), 1);
}

/// Row 27 (AD-15's v0.1 clause, Question 5): a tainted Review is shown
/// with the untrusted-content words and is resolved on the CLI (a return
/// here); resolving keeps the item's and every attempt's taint.
#[tokio::test]
async fn a_tainted_review_shows_the_untrusted_marker_and_resolves() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let (run, item) = tainted_waiting(&env, &root, "a version that read a page");
    let llm = texts(&["the second version"]);
    let taints = |env: &Env| {
        env.steps(&root, &run)
            .into_iter()
            .filter(|s| s.attempt == 1)
            .map(|s| (s.step_key, s.taint))
            .collect::<Vec<_>>()
    };
    let before = taints(&env);
    let t = FakeTerminal::new();
    t.line("r");
    t.line(NOTE);
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(t.shown().contains(MARKER), "{}", t.shown());
    let row = env.item(&root, &item);
    assert_eq!(row.decision, Some(Decision::Return));
    assert_eq!(row.taint, Taint::Tainted, "resolving keeps the taint");
    assert_eq!(taints(&env), before);
    assert_eq!(before[0], ("draft".to_string(), Taint::Tainted));
    assert_eq!(llm.calls(), 1, "the producer ran again");
}

/// Owner condition 2: a tainted Review says that its output came from
/// untrusted content above the payload and again at the choice (after the
/// payload); an untainted Review never does.
#[tokio::test]
async fn a_tainted_review_says_its_output_came_from_untrusted_content() {
    const PAYLOAD: &str = "the tainted payload text";
    let env = env();
    let root = env.copy_of("hand-offs");
    tainted_waiting(&env, &root, PAYLOAD);
    let t = FakeTerminal::new();
    t.line("s");

    let ran = env.gate(&root, &[], &t, texts(&["never"])).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let shown = t.shown();
    let payload = shown.find(PAYLOAD).expect("the payload is shown");
    let first = shown.find(MARKER).expect("the untrusted-content words");
    let last = shown.rfind(MARKER).unwrap();
    assert!(first < payload, "above the payload:\n{shown}");
    assert!(last > payload, "again at the choice:\n{shown}");

    let env2 = self::env();
    let clean = env2.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    waiting(&env2, &clean, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("s");
    let ran = env2.gate(&clean, &[], &t, llm.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(t.shown().contains(DRAFT_1));
    assert!(
        !t.shown().contains("untrusted content"),
        "never for an untainted Review:\n{}",
        t.shown()
    );
}

/// Row 27a (owner condition 2): approving a tainted Review needs its item
/// id typed back. After `a`, an empty line, `a` again, a prefix of the id,
/// another item's id or the id in capitals each write nothing and leave
/// the item open (the id was asked for: a second read). The id with spaces
/// around it (trimmed) approves, and so does the exact id; the run goes
/// on. An untainted Review approves on `a` alone.
#[tokio::test]
async fn approving_a_tainted_review_needs_its_id_typed() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let (first_run, first) = tainted_waiting(&env, &root, "the first tainted version");
    let (second_run, second) = tainted_waiting(&env, &root, "the second tainted version");
    let upper = first.to_uppercase();
    let llm = texts(&["prepared"]);
    for (what, typed) in [
        ("an empty line", ""),
        ("`a` again", "a"),
        ("a prefix of the id", &first[..8]),
        ("another item's id", second.as_str()),
        ("the id in capitals", upper.as_str()),
    ] {
        let t = FakeTerminal::new();
        t.line("a");
        t.line(typed);

        let _ = env.gate(&root, &[first.as_str()], &t, llm.clone()).await;

        assert!(t.reads() >= 2, "{what}: the id was asked for");
        env.assert_open(&root, &first);
        assert!(env.resolved_entries(&root).is_empty(), "{what}");
        env.assert_closed(&root);
    }
    assert_eq!(llm.calls(), 0);

    let t = FakeTerminal::new();
    t.line("a");
    t.line(&format!("  {first}  "));
    let ran = env.gate(&root, &[first.as_str()], &t, llm.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(env.item(&root, &first).decision, Some(Decision::Approve));
    assert_eq!(env.run_status(&root, &first_run), "succeeded");

    let t = FakeTerminal::new();
    t.line("a");
    t.line(&second);
    let ran = env.gate(&root, &[second.as_str()], &t, llm.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(env.item(&root, &second).decision, Some(Decision::Approve));
    assert_eq!(env.run_status(&root, &second_run), "succeeded");
    assert_eq!(env.resolved_entries(&root).len(), 2);

    let env2 = self::env();
    let clean = env2.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "prepared"]);
    let (_, item) = waiting(&env2, &clean, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("a");
    let ran = env2.gate(&clean, &[], &t, llm.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(t.reads(), 1);
    assert_eq!(env2.item(&clean, &item).decision, Some(Decision::Approve));
}

/// Row 34 (Question 3): `d` on a Failure item resolves it `dismiss` by You
/// (Reviewer); no step and not the run (already failed) changes; exit 0.
#[tokio::test]
async fn dismiss_resolves_a_failure() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let (run, item) = failed_run(&env, &root);
    let before = env.steps(&root, &run);
    let t = FakeTerminal::new();
    t.line("d");

    let ran = env.gate(&root, &[], &t, texts(&["never"])).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(t.shown().contains(&item), "{}", t.shown());
    let row = env.item(&root, &item);
    assert_eq!(row.status, "resolved");
    assert_eq!(row.decision, Some(Decision::Dismiss));
    assert!(by_reviewer(row.resolved_by.as_deref().unwrap()));
    assert_eq!(env.steps(&root, &run), before);
    assert_eq!(env.run_status(&root, &run), "failed");
    assert_eq!(env.resolved_entries(&root).len(), 1);
}

/// Row 35 (Question 3): a Question (made by a System pause) is answered
/// with `answer` and then the answer's text: stored once on the item, the
/// step leaves `waiting_human` for `running`, and `agentic gate` does not
/// continue the run (no model is called): it says the step continues with
/// its story (3.10, 3.14).
#[tokio::test]
async fn a_question_is_answered_and_not_continued() {
    const ANSWER: &str = "use the second source";
    let env = env();
    let root = env.copy_of("hand-offs");
    let (run, item) = question_waiting(&env, &root);
    let llm = texts(&["never"]);
    let t = FakeTerminal::new();
    t.line("answer");
    t.line(ANSWER);

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let row = env.item(&root, &item);
    assert_eq!(row.decision, Some(Decision::Answer));
    assert_eq!(row.note.as_deref(), Some(ANSWER));
    let draft = env
        .steps(&root, &run)
        .into_iter()
        .find(|s| s.step_key == "draft")
        .unwrap();
    assert_eq!(draft.status, "running");
    assert_eq!(llm.calls(), 0, "the run is not continued");
    let said = format!("{}{}", t.shown(), ran.all());
    assert!(said.contains("3.10") || said.contains("3.14"), "{said}");
    env.assert_closed(&root);
}

/// What the owner does at one prompt.
#[derive(Debug, Clone, Copy)]
enum Key {
    Line(&'static str),
    CtrlC,
    Idle,
}

/// Row 36: Ctrl-C or the 10-minute limit at any prompt (the choice, the
/// note, the reject confirmation), and a Ctrl-C reported between the last
/// prompt and the write, write nothing for that item; exit 1; the project
/// is closed again (marker removed, lock released). Every prompt is read
/// with the 10-minute limit.
#[tokio::test]
async fn ctrl_c_or_a_timeout_writes_nothing() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "never"]);
    let (_, item) = waiting(&env, &root, "gated", &llm).await;
    let cases: Vec<(&str, Vec<Key>, Option<usize>)> = vec![
        ("Ctrl-C at the choice", vec![Key::CtrlC], None),
        ("the limit at the choice", vec![Key::Idle], None),
        ("Ctrl-C at the note", vec![Key::Line("r"), Key::CtrlC], None),
        (
            "the limit at the note",
            vec![Key::Line("r"), Key::Idle],
            None,
        ),
        (
            "Ctrl-C at the confirmation",
            vec![Key::Line("j"), Key::CtrlC],
            None,
        ),
        (
            "the limit at the confirmation",
            vec![Key::Line("j"), Key::Idle],
            None,
        ),
        (
            "Ctrl-C after the choice",
            vec![Key::Line("a"), Key::CtrlC],
            None,
        ),
        (
            "Ctrl-C between the prompt and the write",
            vec![Key::Line("a")],
            Some(1),
        ),
    ];
    for (what, keys, check) in cases {
        let t = FakeTerminal::new();
        for key in &keys {
            match key {
                Key::Line(text) => t.line(text),
                Key::CtrlC => t.interrupt(),
                Key::Idle => t.time_out(),
            }
        }
        if let Some(n) = check {
            t.interrupt_on_check(n);
        }

        let ran = env.gate(&root, &[], &t, llm.clone()).await;

        assert_eq!(ran.code, 1, "{what}: {}", ran.all());
        env.assert_open(&root, &item);
        assert!(env.resolved_entries(&root).is_empty(), "{what}");
        assert_eq!(llm.calls(), 1, "{what}");
        env.assert_closed(&root);
        assert!(!t.timeouts().is_empty(), "{what}");
        assert!(
            t.timeouts().iter().all(|limit| *limit == Some(TEN_MINUTES)),
            "{what}: {:?}",
            t.timeouts()
        );
    }
}

/// The lines `status` printed for the run `id`: its own line first, then
/// the lines under it, up to the next run's line.
fn status_block(out: &str, id: &str, all_ids: &[&str]) -> Vec<String> {
    let lines: Vec<&str> = out.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains(id))
        .unwrap_or_else(|| panic!("run {id} is not listed in: {out}"));
    lines[start..]
        .iter()
        .enumerate()
        .take_while(|(i, l)| *i == 0 || !all_ids.iter().any(|other| l.contains(other)))
        .map(|(_, l)| l.to_string())
        .collect()
}

/// The one line of `block` naming `item`.
fn item_line<'a>(block: &'a [String], item: &str) -> &'a str {
    let found: Vec<&String> = block.iter().filter(|l| l.contains(item)).collect();
    assert_eq!(found.len(), 1, "one line for {item}: {block:#?}");
    found[0]
}

/// Rows 33 and 38: `agentic status` prints every item of a run with its
/// status and, when resolved, its decision and "You (Reviewer)" (a
/// rejected Review, an approved one, a returned one and the new open
/// Review after it), never a note; a run whose approve was recorded and
/// applied as an API resolve does it (the store's resolve, then the
/// engine's apply) is shown `running`, its gate `succeeded`, `publish`
/// queued, and "ready to continue".
#[tokio::test]
async fn status_shows_each_items_decision_and_who() {
    const SECRET_NOTE: &str = "status-note-canary-2f61";
    let env = env();
    let root = env.copy_of("hand-offs");

    let (rejected_run, rejected) = waiting(&env, &root, "gated", &texts(&["a1"])).await;
    let t = FakeTerminal::new();
    t.line("j");
    t.line("reject");
    let ran = env
        .gate(&root, &[rejected.as_str()], &t, texts(&["never"]))
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let (ready_run, ready) = waiting(&env, &root, "gated", &texts(&["b1"])).await;
    {
        let (store, _) = am_store::open_runtime(&env.layout(&root), "Test Mac".into()).unwrap();
        store
            .writer()
            .resolve_inbox_item(
                HumanActor::new("owner", REVIEWER_HAT),
                &ready,
                Decision::Approve,
                None,
            )
            .unwrap();
        let reader = store.reader().unwrap();
        am_engine::gate::apply_resolution(&store.writer(), &reader, &ready).unwrap();
    }

    let returned_llm = texts(&["c1", "c2"]);
    let (returned_run, returned) = waiting(&env, &root, "gated", &returned_llm).await;
    let t = FakeTerminal::new();
    t.line("r");
    t.line(SECRET_NOTE);
    t.line("s");
    let ran = env
        .gate(&root, &[returned.as_str()], &t, returned_llm.clone())
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let reopened = env
        .items(&root)
        .into_iter()
        .find(|i| i.run_id == returned_run && i.status == "open")
        .expect("the gate waits again")
        .id;

    let ran = env.status(&root);

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        !ran.all().contains(SECRET_NOTE),
        "never a note: {}",
        ran.all()
    );
    let ids = [
        rejected_run.as_str(),
        ready_run.as_str(),
        returned_run.as_str(),
    ];
    for (run, item, decision) in [
        (rejected_run.as_str(), rejected.as_str(), "reject"),
        (ready_run.as_str(), ready.as_str(), "approve"),
        (returned_run.as_str(), returned.as_str(), "return"),
    ] {
        let block = status_block(&ran.out, run, &ids);
        let line = item_line(&block, item);
        for word in ["resolved", decision, "You (Reviewer)"] {
            assert!(line.contains(word), "{word}: {line}");
        }
    }
    let block = status_block(&ran.out, &returned_run, &ids);
    let line = item_line(&block, &reopened);
    assert!(line.contains("open"), "{line}");
    assert!(!line.contains("You (Reviewer)"), "{line}");

    let block = status_block(&ran.out, &ready_run, &ids);
    assert!(block[0].contains("running"), "{block:#?}");
    assert!(
        block
            .iter()
            .any(|l| l.contains("approve") && l.contains("succeeded")),
        "{block:#?}"
    );
    assert!(
        block
            .iter()
            .any(|l| l.contains("publish") && l.contains("queued")),
        "{block:#?}"
    );
    assert!(
        block.iter().any(|l| l.contains("ready to continue")),
        "{block:#?}"
    );
    let block = status_block(&ran.out, &rejected_run, &ids);
    assert!(block[0].contains("failed"), "{block:#?}");
    assert!(
        !block.iter().any(|l| l.contains("ready to continue")),
        "{block:#?}"
    );
}

/// Row 41: `agentic run` at a gate prints its unchanged lines (`run <id>
/// waiting_human` and `Review item <id> waits for you in the Inbox`), exit
/// 0, plus one line naming `agentic gate`.
#[tokio::test]
async fn agentic_run_names_agentic_gate_at_a_gate() {
    let env = env();
    let root = env.copy_of("hand-offs");

    let ran = env.run(&root, "gated", texts(&[DRAFT_1])).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let item = env.items(&root)[0].id.clone();
    let run = env.runs(&root)[0].id.clone();
    assert!(
        ran.out
            .lines()
            .any(|l| l == format!("run {run} waiting_human")),
        "{}",
        ran.out
    );
    assert!(
        ran.out
            .lines()
            .any(|l| l == format!("Review item {item} waits for you in the Inbox")),
        "{}",
        ran.out
    );
    let naming: Vec<&str> = ran
        .out
        .lines()
        .filter(|l| l.contains("agentic gate"))
        .collect();
    assert_eq!(
        naming.len(),
        1,
        "one line names `agentic gate`: {}",
        ran.out
    );
}

// ------------------------------------------------------- review round 1
//
// Story 3.4 review round 1 (the coordinator's triage, 2026-11-24): R1 (the
// check serve hands the server is `agentic gate`'s check 5), R2 (a decision
// whose run cannot go on is refused before anything is written), R7 (the
// payload is shown behind a fixed prefix, invisible characters escaped and
// counted, blank runs collapsed), R8 (a note the store refuses ends the
// command with exit 1), R9 (lines typed ahead of a prompt never answer it;
// the recorded note is printed back), R10 (an interrupt cancels the run the
// command continues) and R11 (a partial line then the end of input is no
// answer). Written before the fixes, against the FakeTerminal API they
// need (`type_ahead`, `discards`, `unfinished`; see the plan's API notes).

/// Every payload line is shown behind this prefix (R7).
const PREFIX: &str = "  | ";

/// R1: `am_app::serving::pin_check`, the check `agentic serve` hands the
/// server (`am_server::PinCheck`), is `agentic gate`'s check 5: the hash of
/// the snapshot the project's files give now; another hash once a workflow
/// is edited; an error (fails closed) for a config error and for an outside
/// change of a file the app wrote.
#[test]
fn the_servers_pin_check_is_agentic_gates_check_5() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let opened = env.open(&root);
    let check = am_app::serving::pin_check(&env.places, &root, &opened.store);

    let first = snapshot_hash(&root);
    assert_eq!(check.current_snapshot(), Ok(first.clone()));

    edit(
        &root,
        "workflows/gated.yaml",
        "Prepare the approved version.",
        "Prepare the approved version for print.",
    );
    let edited = snapshot_hash(&root);
    assert_ne!(edited, first);
    assert_eq!(check.current_snapshot(), Ok(edited));

    let seat = std::fs::read_to_string(root.join("seats/second.md")).unwrap();
    write(
        &root,
        "seats/second.md",
        "---\nschema_version: 2\nname: [broken\n---\n",
    );
    assert!(
        check.current_snapshot().is_err(),
        "a config error fails closed"
    );
    write(&root, "seats/second.md", &seat);
    assert!(check.current_snapshot().is_ok());

    // company.yaml has a History entry (the app gave the project its id):
    // a change outside the app waits for Reload.
    let company = std::fs::read_to_string(root.join("company.yaml")).unwrap();
    write(
        &root,
        "company.yaml",
        &company.replacen("name: Hand-offs", "name: Hand-offs, edited", 1),
    );
    assert!(
        check.current_snapshot().is_err(),
        "an outside change fails closed"
    );
    drop(opened);
}

/// Why a run cannot go on (R2): what `continue_run` refuses.
#[derive(Debug, Clone, Copy)]
enum Stuck {
    Running,
    UnsettledCall,
    IssuedToolCall,
}

/// A run of `gated` (pinned to the current snapshot) whose gate waits with
/// an open Review while the run cannot go on as `how` says, made by System
/// commands; the run's and the item's ids.
fn stuck_run(env: &Env, root: &Path, how: Stuck) -> (String, String) {
    let opened = env.open(root);
    let w = opened.store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "gated".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(snapshot_hash(root)),
        },
    )
    .unwrap();
    let steps = opened.store.reader().unwrap().steps(&run).unwrap();
    let (draft, approve) = (steps[0].id.clone(), steps[1].id.clone());
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "first".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    let finish = WriteCommand::FinishStep {
        step_run_id: draft.clone(),
        status: "succeeded".into(),
        output: Some(DRAFT_1.into()),
    };
    let payload = match how {
        Stuck::Running => None,
        Stuck::UnsettledCall => {
            w.exec(
                engine(),
                WriteCommand::ReserveCall {
                    step_run_id: draft.clone(),
                    call_id: new_id(),
                    call_seq: 1,
                    reserved_micro_usd: 0,
                    rate_snapshot: "none".into(),
                    taint: Taint::Clean,
                    purpose: am_store::CallPurpose::Turn,
                    prompt: vec![],
                },
            )
            .unwrap();
            w.exec(engine(), finish).unwrap();
            Some(draft.clone())
        }
        Stuck::IssuedToolCall => {
            w.exec(
                engine(),
                WriteCommand::RecordToolCall {
                    tool_call_id: new_id(),
                    step_run_id: draft.clone(),
                    call_seq: 1,
                    tool: "read_file".into(),
                    status: "issued".into(),
                },
            )
            .unwrap();
            w.exec(engine(), finish).unwrap();
            Some(draft.clone())
        }
    };
    let item = new_id();
    w.exec(
        engine(),
        WriteCommand::PauseStep {
            step_run_id: approve,
            inputs: vec![],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Review,
                payload,
                taint: Taint::Clean,
                title: "Review before step approve goes on".into(),
                body: "Step `approve` waits for your review.".into(),
                max_returns: Some(2),
            },
        },
    )
    .unwrap();
    drop(w);
    drop(opened);
    (run, item)
}

/// R2: on a run that `continue_run` would refuse (another step `running`,
/// an unsettled model call, an issued tool call), `a` and `r` are refused
/// before anything is written: exit 1, the item open, its gate waiting, no
/// `inbox.resolved` entry and no model call.
#[tokio::test]
async fn a_decision_whose_run_cannot_go_on_is_refused_before_writing() {
    for how in [Stuck::Running, Stuck::UnsettledCall, Stuck::IssuedToolCall] {
        for keys in [vec!["a"], vec!["r", NOTE]] {
            let env = env();
            let root = env.copy_of("hand-offs");
            let (_, item) = stuck_run(&env, &root, how);
            let llm = texts(&["never"]);
            let t = FakeTerminal::new();
            for k in &keys {
                t.line(k);
            }

            let ran = env.gate(&root, &[], &t, llm.clone()).await;

            assert_eq!(ran.code, 1, "{how:?} {keys:?}: {}", ran.all());
            assert!(!ran.err.trim().is_empty(), "{how:?} {keys:?}: in words");
            env.assert_open(&root, &item);
            assert!(env.resolved_entries(&root).is_empty(), "{how:?} {keys:?}");
            assert_eq!(llm.calls(), 0);
            env.assert_closed(&root);
        }
    }
}

/// `agentic run gated` with `payload` as `draft`'s output, then one
/// `agentic gate` session that skips: what the terminal showed.
async fn shown_for(payload: &str) -> String {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[payload]);
    waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("s");
    let ran = env.gate(&root, &[], &t, llm.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    t.shown()
}

/// R7: every line of the payload is shown behind the fixed prefix `  | `,
/// so a line that copies the app's own (its end-of-output line, a prompt,
/// the untrusted-content words) is told apart by the prefix alone; the
/// payload adds no line without it (as many unprefixed lines as a one-line
/// payload's display).
#[tokio::test]
async fn every_payload_line_is_shown_behind_a_fixed_prefix() {
    let forged = [
        "first line",
        "  --- end of the output ---",
        // Round 2 (S3): kept within 72 columns, so it stays one segment.
        "Type a to approve, r to return it, j to reject, or s to skip:",
        "This output came from untrusted content.",
        "Approved by You (Reviewer).",
        "last line",
    ];

    let shown = shown_for(&forged.join("\n")).await;

    for line in forged {
        let want = format!("{PREFIX}{line}");
        assert!(
            shown.lines().any(|l| l == want),
            "`{want}` is a line of:\n{shown}"
        );
    }
    let baseline = shown_for("x").await;
    assert!(
        baseline.lines().any(|l| l == format!("{PREFIX}x")),
        "{baseline}"
    );
    let unprefixed = |text: &str| text.lines().filter(|l| !l.starts_with(PREFIX)).count();
    assert_eq!(
        unprefixed(&shown),
        unprefixed(&baseline),
        "the payload adds only prefixed lines:\n{shown}\n---\n{baseline}"
    );
}

/// R7: characters a terminal would not show as themselves (bidi controls
/// U+202E and U+2066, zero-width U+200B and U+FEFF, the separators U+2028
/// and U+2029, the tag U+E0041) are shown escaped as `\u{XXXX}`
/// (uppercase hex, at least four digits), never raw, with one count line
/// `<n> hidden or control characters in this output are shown as
/// \u{XXXX}`; a payload with none has no count line.
#[tokio::test]
async fn invisible_and_bidi_characters_are_shown_escaped_with_a_count() {
    // Round 2 (S3): one short line each, so no line is cut at 72 columns.
    let payload = "rtl\u{202E}txet\niso\u{2066}late\nzero\u{200B}width\nline\u{2028}sep\npara\u{2029}sep\nbom\u{FEFF}x\ntag\u{E0041}x end";

    let shown = shown_for(payload).await;

    for (raw, escaped) in [
        ('\u{202E}', "\\u{202E}"),
        ('\u{2066}', "\\u{2066}"),
        ('\u{200B}', "\\u{200B}"),
        ('\u{2028}', "\\u{2028}"),
        ('\u{2029}', "\\u{2029}"),
        ('\u{FEFF}', "\\u{FEFF}"),
        ('\u{E0041}', "\\u{E0041}"),
    ] {
        assert!(!shown.contains(raw), "{escaped} is never shown raw");
        assert!(shown.contains(escaped), "{escaped}:\n{shown}");
    }
    assert!(
        shown.contains("7 hidden or control characters in this output are shown as \\u{XXXX}"),
        "the count line:\n{shown}"
    );
    assert!(shown.contains("rtl"), "{shown}");
    assert!(shown.contains("end"), "{shown}");

    let clean = shown_for(DRAFT_1).await;
    assert!(
        !clean.contains("hidden or control characters"),
        "no count line for a clean payload:\n{clean}"
    );
}

/// R7: a run of blank lines in the payload is shown as at most one, so
/// model text cannot push the untrusted-content words off the screen; a
/// newline is kept. Round 2 (S3): a tab is shown as 4 spaces.
#[tokio::test]
async fn blank_runs_collapse_and_newline_and_tab_are_kept() {
    let shown = shown_for("top\n\n\n\n\n\n\n\nmiddle\tcolumn\nbottom").await;

    let lines: Vec<&str> = shown.lines().collect();
    let at = |want: &str| {
        lines
            .iter()
            .position(|l| *l == want)
            .unwrap_or_else(|| panic!("no line `{want}` in:\n{shown}"))
    };
    let top = at(&format!("{PREFIX}top"));
    let middle = at(&format!("{PREFIX}middle    column"));
    let bottom = at(&format!("{PREFIX}bottom"));
    assert!(
        middle > top && middle - top <= 2,
        "one blank line at most:\n{shown}"
    );
    assert_eq!(bottom, middle + 1, "a newline is kept:\n{shown}");
}

/// R8: a return note or an answer the store refuses (here over 2000
/// characters, typed at the prompt) ends `agentic gate` with exit 1;
/// nothing is written.
#[tokio::test]
async fn a_note_the_store_refuses_ends_the_command_with_exit_one() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1]);
    let (_, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("r");
    t.line(&"x".repeat(2001));

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    env.assert_open(&root, &item);
    assert!(env.resolved_entries(&root).is_empty());

    let env = self::env();
    let root = env.copy_of("hand-offs");
    let (_, question) = question_waiting(&env, &root);
    let t = FakeTerminal::new();
    t.line("answer");
    t.line(&"y".repeat(2001));

    let ran = env.gate(&root, &[], &t, texts(&["never"])).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert_eq!(env.item(&root, &question).status, "open");
    assert!(env.resolved_entries(&root).is_empty());
}

/// R9: input typed ahead of a prompt never answers it. Before every prompt
/// `agentic gate` discards what was typed ahead (`Terminal::
/// discard_typed_ahead`; FakeTerminal's `type_ahead` lines). `a⏎a⏎` typed
/// while the returned run works does not approve the re-review (the
/// owner's `s` does skip it), and the second line of a two-line paste at
/// the note prompt does not decide the next item. After a return the
/// recorded note is printed back.
#[tokio::test]
async fn lines_typed_ahead_never_answer_a_prompt() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "the second version", "prepared"]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("r");
    t.line(NOTE);
    t.type_ahead("a");
    t.type_ahead("a");
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(env.item(&root, &item).decision, Some(Decision::Return));
    let again = env
        .items(&root)
        .into_iter()
        .find(|i| i.run_id == run && i.id != item)
        .expect("the re-review");
    assert_eq!(again.status, "open", "never approved by type-ahead");
    assert_eq!(llm.calls(), 2, "`publish` never ran");
    assert!(t.discards() >= t.reads(), "a discard before every prompt");
    assert!(t.shown().contains(NOTE), "the note is printed back");

    // A two-line paste at the note prompt.
    let env = self::env();
    let root = env.copy_of("hand-offs");
    let llm = texts(&[DRAFT_1, "the second version", "prepared"]);
    let (run, item) = waiting(&env, &root, "gated", &llm).await;
    let t = FakeTerminal::new();
    t.line("r");
    t.line("the first line of a pasted note");
    t.type_ahead("a");
    t.line("s");

    let ran = env.gate(&root, &[], &t, llm.clone()).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        env.item(&root, &item).note.as_deref(),
        Some("the first line of a pasted note")
    );
    let again = env
        .items(&root)
        .into_iter()
        .find(|i| i.run_id == run && i.id != item)
        .expect("the re-review");
    assert_eq!(
        again.status, "open",
        "the paste's second line decides nothing"
    );
    assert!(
        t.shown().contains("the first line of a pasted note"),
        "the recorded note is printed back:\n{}",
        t.shown()
    );
}

/// A model that, when called, has the owner press Ctrl-C on the terminal
/// (`FakeTerminal::interrupt`), then waits until its call is cancelled.
struct CtrlCWhenCalled {
    term: FakeTerminal,
    calls: std::sync::atomic::AtomicUsize,
}

impl LlmProvider for CtrlCWhenCalled {
    fn account_id(&self) -> String {
        "ctrl-c-when-called".into()
    }
    fn capabilities(&self, _model: &str) -> am_domain::ModelCapabilities {
        am_domain::ModelCapabilities::default()
    }
    fn check_file_parts(
        &self,
        _request: &am_domain::ChatRequest,
    ) -> Result<(), am_domain::ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        _request: am_domain::ChatRequest,
        cancel: Arc<dyn am_domain::CancelSignal>,
        _sink: Arc<dyn am_domain::EventSink>,
    ) -> am_domain::BoxFuture<Result<am_domain::ChatOutcome, am_domain::ProviderError>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.term.interrupt();
        Box::pin(async move {
            cancel.cancelled().await;
            Ok(am_domain::ChatOutcome {
                text: String::new(),
                tool_calls: vec![],
                usage: None,
                stop: am_domain::StopReason::Cancelled,
            })
        })
    }
}

/// R10: a Ctrl-C on the terminal while the run `agentic gate` continues is
/// working on a model step cancels that run's token: the step ends
/// (`failed` or `cancelled`, as 3.3's cancellation ends it), the run stops,
/// and the command exits 1. (Without it the model call waits forever; the
/// test gives up after 30 s.)
#[tokio::test]
async fn an_interrupt_while_the_run_goes_on_cancels_it() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let (run, item) = waiting(&env, &root, "gated", &texts(&[DRAFT_1])).await;
    let t = FakeTerminal::new();
    t.line("a");
    let model = Arc::new(CtrlCWhenCalled {
        term: t.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let provider: Arc<dyn LlmProvider> = model.clone();
    let providers: Providers = Arc::new(move |m: &ModelRef| {
        if m.account == "local-lmstudio" {
            Ok(provider.clone())
        } else {
            Err(format!("no provider for {}", m.account))
        }
    });
    let argv = vec!["--project".to_string(), root.to_string_lossy().into_owned()];

    let ran = tokio::time::timeout(Duration::from_secs(30), env.gate_argv(&argv, &t, providers))
        .await
        .expect("the interrupt stops the run; without it the model call never ends");

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert_eq!(model.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(env.item(&root, &item).decision, Some(Decision::Approve));
    let publish = env
        .steps(&root, &run)
        .into_iter()
        .find(|s| s.step_key == "publish")
        .unwrap();
    assert!(
        matches!(publish.status.as_str(), "failed" | "cancelled"),
        "{publish:?}"
    );
    assert!(
        matches!(env.run_status(&root, &run).as_str(), "failed" | "cancelled"),
        "the run stops"
    );
    env.assert_closed(&root);
}

/// R11: at every prompt (the choice, the note, the reject confirmation,
/// the tainted id, the answer, a Failure's choice), a partial line followed
/// by the end of input (`FakeTerminal::unfinished`: typed with no Return,
/// then Ctrl-D twice) is no answer: nothing is written, exit 1.
#[tokio::test]
async fn a_partial_line_then_the_end_of_input_writes_nothing() {
    type Setup = fn(&Env, &Path) -> String;
    async fn reviewed(env: &Env, root: &Path) -> String {
        waiting(env, root, "gated", &texts(&[DRAFT_1])).await.1
    }
    let cases: [(&str, &[&str], &str); 3] = [
        ("the choice", &[], "a"),
        ("the note", &["r"], NOTE),
        ("the reject confirmation", &["j"], "reject"),
    ];
    for (what, lines, partial) in cases {
        let env = env();
        let root = env.copy_of("hand-offs");
        let item = reviewed(&env, &root).await;
        let t = FakeTerminal::new();
        for l in lines {
            t.line(l);
        }
        t.unfinished(partial);

        let ran = env.gate(&root, &[], &t, texts(&["prepared"])).await;

        assert_eq!(ran.code, 1, "{what}: {}", ran.all());
        env.assert_open(&root, &item);
        assert!(env.resolved_entries(&root).is_empty(), "{what}");
    }

    let tainted: Setup = |env, root| tainted_waiting(env, root, "a tainted version").1;
    let question: Setup = |env, root| question_waiting(env, root).1;
    let failure: Setup = |env, root| failed_run(env, root).1;
    for (what, setup, lines, partial) in [
        ("the tainted id", tainted, &["a"][..], None),
        ("the answer", question, &["answer"][..], Some("an answer")),
        ("a Failure's choice", failure, &[][..], Some("d")),
    ] {
        let env = env();
        let root = env.copy_of("hand-offs");
        let item = setup(&env, &root);
        let t = FakeTerminal::new();
        for l in lines {
            t.line(l);
        }
        t.unfinished(partial.unwrap_or(item.as_str()));

        let ran = env
            .gate(&root, &[item.as_str()], &t, texts(&["prepared"]))
            .await;

        assert_eq!(ran.code, 1, "{what}: {}", ran.all());
        assert_eq!(env.item(&root, &item).status, "open", "{what}");
        assert!(env.resolved_entries(&root).is_empty(), "{what}");
    }
}

// ------------------------------------------------------- review round 2
//
// Story 3.4 review round 2 (the coordinator's triage, 2026-11-24): S1 (an
// interrupt ends the command), S3 (payload lines are cut at 72 columns,
// each segment behind the prefix), S4 (the hidden table is Cc but newline
// and tab, Zl, Zp and Default_Ignorable_Code_Point) and S7 (titles and
// bodies are shown through the same display). Written before the fixes.

/// The column limit of a payload segment, after the prefix (S3).
const SEGMENT: usize = 72;

/// A character in the common wide ranges takes two columns (S3).
fn wide(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    )
}

fn columns(text: &str) -> usize {
    text.chars().map(|c| if wide(c) { 2 } else { 1 }).sum()
}

/// The payload segments of a display: what follows the prefix on each
/// prefixed line.
fn segments(shown: &str) -> Vec<&str> {
    shown
        .lines()
        .filter_map(|l| l.strip_prefix(PREFIX))
        .collect()
}

/// S1: an interrupt ends the command. Two runs wait at their gates; the
/// first Review is approved, and a Ctrl-C while its run works on `publish`
/// cancels it. `agentic gate` reports that run's outcome (`run <id> …`)
/// and exits 1; the second Review is never shown and nothing more is read.
#[tokio::test]
async fn an_interrupt_ends_the_command_before_the_next_item() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let (first_run, first) = waiting(&env, &root, "gated", &texts(&[DRAFT_1])).await;
    let (_, second) = waiting(&env, &root, "gated", &texts(&["another version"])).await;
    let t = FakeTerminal::new();
    t.line("a");
    let model = Arc::new(CtrlCWhenCalled {
        term: t.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let provider: Arc<dyn LlmProvider> = model.clone();
    let providers: Providers = Arc::new(move |m: &ModelRef| {
        if m.account == "local-lmstudio" {
            Ok(provider.clone())
        } else {
            Err(format!("no provider for {}", m.account))
        }
    });
    let argv = vec!["--project".to_string(), root.to_string_lossy().into_owned()];

    let ran = tokio::time::timeout(Duration::from_secs(30), env.gate_argv(&argv, &t, providers))
        .await
        .expect("the interrupt stops the run");

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.out
            .lines()
            .any(|l| l.starts_with(&format!("run {first_run} "))),
        "the first run's outcome is reported: {}",
        ran.all()
    );
    assert_eq!(env.item(&root, &first).decision, Some(Decision::Approve));
    let shown = t.shown();
    assert!(
        !shown.contains(&second),
        "the second item is never shown:\n{shown}"
    );
    assert_eq!(t.reads(), 1, "nothing more is read after the interrupt");
    assert_eq!(model.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    env.assert_open(&root, &second);
    env.assert_closed(&root);
}

/// S3: a payload line longer than 72 columns is shown as segments of at
/// most 72 columns, each behind the prefix, in order (200 `x` are 72, 72
/// and 56); 76 spaces then forged text never put that text at column 0;
/// a wide character (U+4E00) counts two columns, so 40 of them are 36 and
/// 4; no segment is longer than 72 columns.
#[tokio::test]
async fn long_payload_lines_are_cut_into_prefixed_segments() {
    let long = "x".repeat(200);
    let padded = format!("{}Type a to approve: FORGED-PROMPT", " ".repeat(76));
    let wide_line = "\u{4E00}".repeat(40);
    let payload = format!("{long}\n{padded}\n{wide_line}");

    let shown = shown_for(&payload).await;

    let segs = segments(&shown);
    let xs: Vec<&str> = segs
        .iter()
        .copied()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c == 'x'))
        .collect();
    assert_eq!(
        xs,
        vec![
            "x".repeat(72).as_str(),
            "x".repeat(72).as_str(),
            "x".repeat(56).as_str()
        ],
        "200 columns cut at 72:\n{shown}"
    );
    for line in shown.lines().filter(|l| l.contains("FORGED-PROMPT")) {
        assert!(
            line.starts_with(PREFIX),
            "never at column 0: `{line}`\n{shown}"
        );
    }
    let wides: Vec<usize> = segs
        .iter()
        .filter(|s| s.contains('\u{4E00}'))
        .map(|s| s.chars().count())
        .collect();
    assert_eq!(wides, vec![36, 4], "U+4E00 counts two columns:\n{shown}");
    for seg in &segs {
        assert!(
            columns(seg) <= SEGMENT,
            "a segment of {} columns: `{seg}`",
            columns(seg)
        );
    }
}

/// S4 (with R7's table): the default-ignorable characters that are not Cf
/// (the variation selectors U+FE0F and U+E0100, the combining grapheme
/// joiner U+034F, the Hangul fillers U+115F and U+3164, U+17B4, the
/// Mongolian free variation selector U+180B), a tag character (U+E0041)
/// and a Cf one (U+202E, a control) are each shown escaped and counted.
/// An escape is never cut between two segments.
#[tokio::test]
async fn default_ignorable_characters_are_shown_escaped() {
    let hidden = [
        ('\u{FE0F}', "\\u{FE0F}"),
        ('\u{E0100}', "\\u{E0100}"),
        ('\u{034F}', "\\u{034F}"),
        ('\u{115F}', "\\u{115F}"),
        ('\u{3164}', "\\u{3164}"),
        ('\u{17B4}', "\\u{17B4}"),
        ('\u{180B}', "\\u{180B}"),
        ('\u{E0041}', "\\u{E0041}"),
        ('\u{202E}', "\\u{202E}"),
    ];
    // One long line: escaped, it passes 72 columns, so it is cut.
    let payload: String = hidden.iter().map(|(c, _)| format!("ab{c}cd ")).collect();

    let shown = shown_for(&payload).await;

    for (raw, escaped) in hidden {
        assert!(!shown.contains(raw), "{escaped} is never shown raw");
        assert!(shown.contains(escaped), "{escaped}:\n{shown}");
    }
    assert!(
        shown.contains(&format!(
            "{} hidden or control characters in this output are shown as \\u{{XXXX}}",
            hidden.len()
        )),
        "the count line:\n{shown}"
    );
    for seg in segments(&shown) {
        for (at, _) in seg.match_indices("\\u{") {
            let rest = &seg[at + 3..];
            let close = rest
                .find('}')
                .unwrap_or_else(|| panic!("a cut escape: `{seg}`"));
            assert!(
                (4..=6).contains(&close) && rest[..close].chars().all(|c| c.is_ascii_hexdigit()),
                "a whole escape: `{seg}`"
            );
        }
    }
}

/// S7: an item's title and body are shown through the same display as a
/// payload: a Failure item (written by System commands) whose title holds
/// U+202E and whose body holds ESC, U+202E and U+200B is shown with each
/// escaped, never raw.
#[tokio::test]
async fn titles_and_bodies_are_shown_escaped() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let opened = env.open(&root);
    let w = opened.store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "gated".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(snapshot_hash(&root)),
        },
    )
    .unwrap();
    let draft = opened.store.reader().unwrap().steps(&run).unwrap()[0]
        .id
        .clone();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "first".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    let item = new_id();
    w.exec(
        engine(),
        WriteCommand::FailWithItem {
            step_run_id: draft.clone(),
            settle: None,
            output: None,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Failure,
                payload: Some(draft),
                taint: Taint::Clean,
                title: "A step \u{202E}deppots".into(),
                body: "Resolved by \u{1b}[2Ksomeone\u{202E}esle, \u{200B}hidden.".into(),
                max_returns: None,
            },
        },
    )
    .unwrap();
    drop(w);
    drop(opened);
    let t = FakeTerminal::new();
    t.line("s");

    let ran = env.gate(&root, &[], &t, texts(&["never"])).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let shown = t.shown();
    assert!(shown.contains(&item), "{shown}");
    for (raw, escaped) in [
        ('\u{1b}', "\\u{001B}"),
        ('\u{202E}', "\\u{202E}"),
        ('\u{200B}', "\\u{200B}"),
    ] {
        assert!(
            !shown.contains(raw),
            "{escaped} is never shown raw:\n{shown:?}"
        );
        assert!(shown.contains(escaped), "{escaped}:\n{shown}");
    }
    assert!(
        shown.contains("deppots") && shown.contains("hidden."),
        "{shown}"
    );
}
