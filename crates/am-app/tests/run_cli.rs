//! Story 3.1 (epic 3 story 1): `agentic run <workflow>` and `agentic
//! status`, run in process through `am_app::cli::{run, status}` with
//! am-testkit's `FakeLlm` injected through the command's context (the fake
//! never reaches the binary), `FakeTerminal` and `FakeProjectIdentity`, on
//! stream copies of `fixtures/projects/one-step` and a two-step project the
//! tests write. One test per row of the story's I/O matrix; the rows that
//! need the real `agentic` (`CARGO_BIN_EXE_agentic`: piped stdin) run it
//! with HOME in a temporary folder. The engine's side is
//! `crates/am-engine/tests/run.rs`, the store's
//! `crates/am-store/tests/run_journal.rs`. No network, no real model.
//! Written before the commands exist.
//!
//! What these tests pin:
//!
//! ```text
//! am_app::cli::RunContext {
//!     places: Places,
//!     identity: Arc<dyn ProjectIdentity>,
//!     terminal: Box<dyn Terminal>,
//!     providers: Arc<dyn Fn(&am_config::ModelRef)
//!                    -> Result<Arc<dyn LlmProvider>, String> + Send + Sync>,
//! }
//! am_app::cli::unwired_providers() -> that factory type
//!     the factory `main` gives the binary: it wires no provider and
//!     refuses every model in words
//! am_app::cli::run(args: &[String], ctx: RunContext, out, err).await -> i32
//!     args: <workflow> [--project <folder>]
//! am_app::cli::status(args: &[String], places: &Places, out, err) -> i32
//!     args: [--project <folder>]
//! ```
//!
//! `run` prints `run <id> <status>` on `out` and exits 0 on `succeeded`, 1
//! otherwise, 2 on bad usage. `status` prints, newest first, one line per
//! run holding its id, workflow, status, at least the first 12 characters
//! of its snapshot hash and its duration (a number followed by `s` or `ms`,
//! or the word `running`), and under it one line per step holding its key,
//! its status and its duration.
//!
//! A manual run start is a CLI-class decision (AD-15): `run` is refused in
//! words, before the project is opened, unless the context's terminal is
//! interactive, and while the project's config enables a stdio MCP server
//! or a Source with `render: true`. `status` has no such rule.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_app::cli::{RunContext, run, status, unwired_providers};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_config::ModelRef;
use am_domain::runtime::open_marker;
use am_domain::{
    BoxFuture, CancelSignal, ChatOutcome, ChatRequest, EventSink, LlmProvider, MacId, Message,
    ModelCapabilities, Part, ProviderError, Role, RuntimeLayout, StopReason, Taint, TrustedInput,
    Usage,
};
use am_store::{CallRow, Reader, RunRow, StepRow};
use am_testkit::{FakeLlm, FakeProjectIdentity, FakeTerminal, ScriptedTurn};

/// The schema version before migration 0006 (the run's pin and the times).
const BEFORE_RUN_PIN: i64 = 5;

/// The provider factory of `RunContext`: a seat's model to the provider
/// that serves it, or a refusal in words.
type Providers = Arc<dyn Fn(&ModelRef) -> Result<Arc<dyn LlmProvider>, String> + Send + Sync>;

/// The audit actions of one run, in the order the engine writes them.
const RUN_ACTIONS: [&str; 5] = [
    "run.created",
    "step.started",
    "model_call.reserved",
    "model_call.settled+step.finished",
    "run.finished",
];

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

fn layout_in(places: &Places, root: &Path) -> Option<RuntimeLayout> {
    let id = am_config::read_company(root).ok()?.project_id?;
    Some(places.base.project(&id))
}

/// The runs in the database at `layout`, newest first; none when there is
/// no database.
fn runs_at(layout: Option<&RuntimeLayout>) -> Vec<RunRow> {
    match layout.map(RuntimeLayout::state_db) {
        Some(db) if db.is_file() => Reader::open(&db).unwrap().runs().unwrap(),
        _ => Vec::new(),
    }
}

impl Env {
    /// A stream copy of `fixtures/projects/<fixture>`, never opened on this
    /// Mac: no id, no runtime folder.
    fn copy_of(&self, fixture: &str) -> PathBuf {
        self.copy_as(fixture, fixture)
    }

    /// [`Env::copy_of`], in the project folder `name`.
    fn copy_as(&self, fixture: &str, name: &str) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        am_testkit::copy_tree(&am_testkit::fixture(&format!("projects/{fixture}")), &root);
        root.canonicalize().unwrap()
    }

    /// A project written here, never opened: two seats on two Accounts with
    /// two models, and the workflow `digest` of two steps (`gather` on
    /// `alpha`, `summarise` on `beta` with its own output limit). It enables
    /// no MCP server and has no Source, so the CLI may start its runs.
    fn two_step(&self) -> PathBuf {
        let root = self.dir.path().join("projects").join("two");
        for (rel, text) in [
            (
                "company.yaml",
                "schema_version: 2\nname: Two Steps\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n",
            ),
            (
                "seats/alpha.md",
                "---\nschema_version: 2\nname: Alpha\nrole: Analyst\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\n---\nList what is new, one item per line.\n",
            ),
            (
                "seats/beta.md",
                "---\nschema_version: 2\nname: Beta\nrole: Writer\nteam: core\nmodel:\n  account: local-ollama\n  model: example-small\n---\nWrite a short summary.\n",
            ),
            (
                "workflows/digest.yaml",
                "schema_version: 2\nname: Digest\nteam: core\nsteps:\n  - id: gather\n    seat: alpha\n    instruction: List the items added since the last run.\n  - id: summarise\n    seat: beta\n    instruction: Write the summary.\n    max_tokens: 256\n",
            ),
        ] {
            write(&root, rel, text);
        }
        root.canonicalize().unwrap()
    }

    /// Opens the project once on this Mac (an id, a runtime folder and a
    /// store) and closes it again.
    fn open_once(&self, root: &Path) {
        let req = OpenRequest {
            root: root.to_path_buf(),
            confirm_move: false,
            take_over: false,
        };
        drop(
            open_project(
                &self.places,
                &FakeProjectIdentity::new("Test Mac"),
                &req,
                &mut NoPrompt,
            )
            .unwrap(),
        );
    }

    fn layout(&self, root: &Path) -> Option<RuntimeLayout> {
        layout_in(&self.places, root)
    }

    fn reader(&self, root: &Path) -> Reader {
        let layout = self.layout(root).expect("the project has an id");
        Reader::open(&layout.state_db()).unwrap()
    }

    /// Every run row of the project, newest first; none when the project
    /// has no id or no database yet.
    fn runs(&self, root: &Path) -> Vec<RunRow> {
        runs_at(self.layout(root).as_ref())
    }

    fn steps(&self, root: &Path, run_id: &str) -> Vec<StepRow> {
        self.reader(root).steps(run_id).unwrap()
    }

    fn calls(&self, root: &Path, run_id: &str) -> Vec<CallRow> {
        self.reader(root).calls_for_run(run_id).unwrap()
    }

    /// The audit entries of the run commands (see [`RUN_ACTIONS`]), as
    /// export lines, in order.
    fn run_audit(&self, root: &Path) -> Vec<String> {
        run_audit_of(&self.reader(root))
    }

    /// The project is closed again: no marker, and nothing holds its lock.
    fn assert_closed(&self, root: &Path) {
        assert!(!open_marker(root).exists(), "the marker is removed");
        if let Some(layout) = self.layout(root) {
            assert!(!am_store::lock_is_held(&layout), "the lock is released");
        }
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
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

/// A seat's instructions as the project's config holds them.
fn seat_instructions(root: &Path, seat: &str) -> String {
    let snapshot = am_config::load(root).unwrap().snapshot.unwrap();
    snapshot.seats()[seat].instructions.clone()
}

/// The text value of `key` on an audit export line.
fn field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let end = line[start..].find('"')?;
    Some(line[start..start + end].to_string())
}

fn run_audit_of(reader: &Reader) -> Vec<String> {
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .filter(|l| field(l, "action").is_some_and(|a| RUN_ACTIONS.contains(&a.as_str())))
        .map(str::to_string)
        .collect()
}

fn actions(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| field(l, "action").expect("an action"))
        .collect()
}

fn by_owner(line: &str) -> bool {
    line.contains("\"kind\":\"human\"") && line.contains("\"hat\":\"Owner\"")
}

fn by_engine(line: &str) -> bool {
    line.contains("\"kind\":\"system\"") && line.contains("\"component\":\"engine\"")
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

    /// The id and status of the `run <id> <status>` line on stdout.
    fn run_line(&self) -> (String, String) {
        let line = self
            .out
            .lines()
            .find(|l| l.starts_with("run "))
            .unwrap_or_else(|| panic!("no `run <id> <status>` line: {}", self.all()));
        let words: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(words.len(), 3, "`run <id> <status>`: {line}");
        assert!(is_uuid_v7(words[1]), "a UUIDv7 run id: {line}");
        (words[1].to_string(), words[2].to_string())
    }
}

fn is_uuid_v7(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            14 => c == '7',
            _ => c.is_ascii_hexdigit() && !c.is_ascii_uppercase(),
        })
}

/// What one command runs with: the terminal (interactive unless
/// [`Kit::piped`]), one provider per Account id, and every model the
/// factory was asked for.
struct Kit {
    term: FakeTerminal,
    by_account: BTreeMap<String, Arc<dyn LlmProvider>>,
    asked: Arc<Mutex<Vec<ModelRef>>>,
}

impl Kit {
    fn new() -> Kit {
        Kit {
            term: FakeTerminal::new(),
            by_account: BTreeMap::new(),
            asked: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Stdin is a pipe or a file: not an interactive terminal.
    fn piped(mut self) -> Kit {
        self.term = FakeTerminal::piped();
        self
    }

    /// One provider for the fixtures' `local-lmstudio` Account.
    fn local(provider: Arc<dyn LlmProvider>) -> Kit {
        Kit::new().with("local-lmstudio", provider)
    }

    fn with(mut self, account: &str, provider: Arc<dyn LlmProvider>) -> Kit {
        self.by_account.insert(account.to_string(), provider);
        self
    }

    fn asked(&self) -> Vec<ModelRef> {
        self.asked.lock().unwrap().clone()
    }

    fn providers(&self) -> Providers {
        let by_account = self.by_account.clone();
        let asked = self.asked.clone();
        Arc::new(move |model: &ModelRef| {
            asked.lock().unwrap().push(model.clone());
            by_account
                .get(&model.account)
                .cloned()
                .ok_or_else(|| format!("the test has no provider for Account {}", model.account))
        })
    }

    /// Runs `agentic run <args> --project <root>` on this kit's terminal.
    async fn run(&self, env: &Env, root: &Path, args: &[&str]) -> Ran {
        run_argv(
            env,
            &with_project(args, root),
            self.providers(),
            self.term.clone(),
        )
        .await
    }
}

fn with_project(args: &[&str], root: &Path) -> Vec<String> {
    let mut argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    argv.push("--project".into());
    argv.push(root.to_string_lossy().into_owned());
    argv
}

/// Runs `agentic run <args> --project <root>` with `providers`, on an
/// interactive terminal.
async fn run_with(env: &Env, root: &Path, args: &[&str], providers: Providers) -> Ran {
    run_argv(
        env,
        &with_project(args, root),
        providers,
        FakeTerminal::new(),
    )
    .await
}

async fn run_argv(env: &Env, argv: &[String], providers: Providers, term: FakeTerminal) -> Ran {
    let ctx = RunContext {
        places: env.places.clone(),
        identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
        terminal: Box::new(term),
        providers,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(argv, ctx, &mut out, &mut err).await;
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// Runs `agentic status <argv>`.
fn status_argv(places: &Places, argv: &[String]) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = status(argv, places, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// Runs `agentic status --project <root>`.
fn status_of(places: &Places, root: &Path) -> Ran {
    status_argv(
        places,
        &["--project".to_string(), root.to_string_lossy().into_owned()],
    )
}

fn fake(turn: ScriptedTurn) -> Arc<FakeLlm> {
    Arc::new(FakeLlm::new(vec![turn]))
}

/// One turn per step: the seat's instructions as the system instruction,
/// the step's instruction as the one user part, the seat's model, the
/// step's output limit and no tools.
fn assert_request(
    request: &ChatRequest,
    model: &str,
    system: &str,
    instruction: &str,
    max_output_tokens: u32,
) {
    assert_eq!(request.model, model);
    assert_eq!(request.system.len(), 1, "{:?}", request.system);
    assert_eq!(request.system[0].text().trim(), system.trim());
    assert_eq!(request.messages.len(), 1, "{:?}", request.messages);
    let Message { role, parts } = &request.messages[0];
    assert_eq!(*role, Role::User);
    match parts.as_slice() {
        [Part::Instruction(i)] => assert_eq!(i.text().trim(), instruction),
        other => panic!("one user Instruction part, found {other:?}"),
    }
    assert!(request.tools.is_empty(), "no tools exist yet");
    assert_eq!(request.max_output_tokens, max_output_tokens);
}

/// A duration as `status` prints it: a number followed by `s` or `ms`
/// (`0.042s`, `42ms`, `3 s`).
fn has_duration(line: &str) -> bool {
    let number = |t: &str| {
        t.chars().any(|c| c.is_ascii_digit()) && t.chars().all(|c| c.is_ascii_digit() || c == '.')
    };
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| ",;()[]".contains(c)))
        .collect();
    words.iter().enumerate().any(|(i, w)| {
        ["ms", "s"].iter().any(|unit| {
            w.strip_suffix(unit).is_some_and(number) || (w == unit && i > 0 && number(words[i - 1]))
        })
    })
}

/// The lines `status` printed for the run `id`: its own line first, then
/// the lines under it, up to the next run's line.
fn status_block(out: &str, id: &str, all_ids: &[String]) -> Vec<String> {
    let lines: Vec<&str> = out.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains(id))
        .unwrap_or_else(|| panic!("run {id} is not listed in: {out}"));
    lines[start..]
        .iter()
        .enumerate()
        .take_while(|(i, l)| *i == 0 || !all_ids.iter().any(|other| l.contains(other.as_str())))
        .map(|(_, l)| l.to_string())
        .collect()
}

/// The line of the step `key` in a run's status block.
fn step_line<'a>(block: &'a [String], key: &str) -> &'a str {
    block[1..]
        .iter()
        .find(|l| l.split_whitespace().any(|w| w.trim_matches(':') == key))
        .unwrap_or_else(|| panic!("step {key} is not listed under its run: {block:#?}"))
}

// ------------------------------------------------------------------ run

/// The fixture the rows run on is a valid project: one seat on
/// `local-lmstudio`, one workflow with one step.
#[test]
fn the_one_step_fixture_is_a_valid_project() {
    let root = am_testkit::fixture("projects/one-step");
    let loaded = am_config::load_in(&root, am_config::LoadOptions::template(&root)).unwrap();
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    let snapshot = loaded.snapshot.expect("a snapshot");
    assert_eq!(
        snapshot.workflows().keys().collect::<Vec<_>>(),
        vec!["hello"]
    );
    let steps = &snapshot.workflows()["hello"].steps;
    assert_eq!(steps.len(), 1);
    assert_eq!(
        (steps[0].id.as_str(), steps[0].seat.as_deref()),
        ("greet", Some("alpha"))
    );
    assert_eq!(steps[0].max_tokens, None, "the default limit applies");
    let model = snapshot.seats()["alpha"].model.clone().expect("a model");
    assert_eq!(
        (model.account.as_str(), model.model.as_str()),
        ("local-lmstudio", "example-local")
    );
}

/// Run and After: on a folder never opened on this Mac, `agentic run hello`
/// opens the project as `serve` does, runs the one step on the scripted
/// model, prints `run <uuid> succeeded` and exits 0. The step's output is
/// the model's text; the run pins the hash of the snapshot it was built
/// from; the call is settled with the usage; step and run have their
/// finish times; the audit log holds the five entries in order, the first
/// by the owner and the rest by the engine, and its chain verifies; the
/// project is closed again afterwards.
#[tokio::test]
async fn run_one_step_succeeds_prints_the_run_and_pins_the_snapshot() {
    let env = env();
    let root = env.copy_of("one-step");
    assert!(env.layout(&root).is_none(), "never opened");
    let llm = fake(ScriptedTurn::text("done").usage(12, 3));
    let kit = Kit::local(llm.clone());

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let (id, status) = ran.run_line();
    assert_eq!(status, "succeeded");
    assert_eq!(kit.term.reads(), 0, "it asks no question");

    // One provider asked for, for the seat's model; one turn.
    assert_eq!(
        kit.asked(),
        vec![ModelRef {
            account: "local-lmstudio".into(),
            model: "example-local".into(),
        }]
    );
    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        "example-local",
        &seat_instructions(&root, "alpha"),
        "Say hello in one line.",
        4096,
    );

    // The journal.
    let runs = env.runs(&root);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].id, id);
    assert_eq!(
        (runs[0].workflow.as_str(), runs[0].status.as_str()),
        ("hello", "succeeded")
    );
    assert_eq!(
        runs[0].config_hash.as_deref(),
        Some(snapshot_hash(&root).as_str()),
        "the run pins the snapshot it was built from (the project's files as the open left them)"
    );
    assert!(runs[0].finished_at.is_some(), "{:?}", runs[0]);
    let steps = env.steps(&root, &id);
    assert_eq!(steps.len(), 1);
    assert_eq!(
        (steps[0].step_key.as_str(), steps[0].status.as_str()),
        ("greet", "succeeded")
    );
    assert_eq!(steps[0].output.as_deref(), Some("done"));
    assert!(
        steps[0].started_at.is_some() && steps[0].finished_at.is_some(),
        "{:?}",
        steps[0]
    );
    let calls = env.calls(&root, &id);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].settled, "{:?}", calls[0]);
    assert_eq!(
        (calls[0].input_tokens, calls[0].output_tokens),
        (Some(12), Some(3))
    );

    // The audit entries, each committed with its command.
    let audit = env.run_audit(&root);
    assert_eq!(actions(&audit), RUN_ACTIONS);
    assert!(by_owner(&audit[0]), "the owner started it: {}", audit[0]);
    assert_eq!(field(&audit[0], "target").as_deref(), Some(id.as_str()));
    assert!(audit[0].contains("\"origin\":\"manual\""), "{}", audit[0]);
    assert!(
        audit[0].contains(&format!("\"config_hash\":\"{}\"", snapshot_hash(&root))),
        "the hash is audited as an argument: {}",
        audit[0]
    );
    for line in &audit[1..] {
        assert!(by_engine(line), "the engine's transition: {line}");
    }
    assert!(!audit.concat().contains("done"), "no content in the log");
    assert_eq!(env.reader(&root).verify_audit_chain().unwrap(), None);

    env.assert_closed(&root);
}

/// The step's `max_tokens` is the call's output limit (4096 when the step
/// names none, as the row above shows).
#[tokio::test]
async fn the_steps_max_tokens_is_the_output_limit() {
    let env = env();
    let root = env.copy_of("one-step");
    write(
        &root,
        "workflows/hello.yaml",
        "schema_version: 2\nname: Hello\nteam: core\nsteps:\n  - id: greet\n    seat: alpha\n    instruction: Say hello in one line.\n    max_tokens: 512\n",
    );
    let llm = fake(ScriptedTurn::text("hello"));
    let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(llm.requests()[0].max_output_tokens, 512);
}

/// What the journal held, and what the commands said, when the provider
/// was called.
#[derive(Debug, Clone)]
struct AtCall {
    runs: Vec<RunRow>,
    steps: Vec<StepRow>,
    calls: Vec<CallRow>,
    audit: Vec<String>,
    lock_held: bool,
    marker: bool,
    status: Ran,
}

/// A provider that, when it is called, reads the project's database
/// through its own read-only connection and runs `agentic status`, then
/// answers "done".
struct Spy {
    places: Places,
    root: PathBuf,
    seen: Mutex<Vec<AtCall>>,
}

impl LlmProvider for Spy {
    fn account_id(&self) -> String {
        "spy".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        _request: ChatRequest,
        _cancel: Arc<dyn CancelSignal>,
        _sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let layout = layout_in(&self.places, &self.root).expect("an open project has an id");
        let reader = Reader::open(&layout.state_db()).unwrap();
        let runs = reader.runs().unwrap();
        let (mut steps, mut calls) = (Vec::new(), Vec::new());
        for run in &runs {
            steps.extend(reader.steps(&run.id).unwrap());
            calls.extend(reader.calls_for_run(&run.id).unwrap());
        }
        self.seen.lock().unwrap().push(AtCall {
            runs,
            steps,
            calls,
            audit: run_audit_of(&reader),
            lock_held: am_store::lock_is_held(&layout),
            marker: open_marker(&self.root).exists(),
            status: status_of(&self.places, &self.root),
        });
        Box::pin(async {
            Ok(ChatOutcome {
                text: "done".into(),
                tool_calls: vec![],
                usage: Some(Usage {
                    input_tokens: 7,
                    output_tokens: 2,
                }),
                stop: StopReason::EndTurn,
            })
        })
    }
}

/// Journal order (AD-8): when the provider is called, the run row
/// (`running`, with its hash), the step's `running` state with its start
/// time and the model call's reservation (not settled) are committed and
/// readable from another connection, and the audit log holds
/// `run.created`, `step.started`, `model_call.reserved` in that order and
/// nothing later. The project is held open for the whole run (its lock and
/// marker), and `agentic status` reads it meanwhile without the lock: it
/// lists the run as running.
#[tokio::test]
async fn at_the_model_call_the_journal_is_committed_and_status_reads_it() {
    let env = env();
    let root = env.copy_of("one-step");
    let spy = Arc::new(Spy {
        places: env.places.clone(),
        root: root.clone(),
        seen: Mutex::new(Vec::new()),
    });
    let kit = Kit::local(spy.clone());

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let (id, _) = ran.run_line();
    let seen = spy.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "one turn per step");
    let at = &seen[0];

    assert_eq!(at.runs.len(), 1);
    assert_eq!(at.runs[0].id, id);
    assert_eq!(at.runs[0].status, "running");
    assert_eq!(
        at.runs[0].config_hash.as_deref(),
        Some(snapshot_hash(&root).as_str())
    );
    assert_eq!(at.runs[0].finished_at, None);
    assert_eq!(at.steps.len(), 1);
    assert_eq!(at.steps[0].status, "running");
    assert!(at.steps[0].started_at.is_some(), "{:?}", at.steps);
    assert_eq!(at.steps[0].finished_at, None);
    assert_eq!(at.calls.len(), 1, "one model call, reserved");
    assert!(!at.calls[0].settled, "{:?}", at.calls);
    assert_eq!(
        actions(&at.audit),
        vec!["run.created", "step.started", "model_call.reserved"]
    );
    assert!(by_owner(&at.audit[0]), "{}", at.audit[0]);
    assert!(
        by_engine(&at.audit[1]) && by_engine(&at.audit[2]),
        "{:?}",
        at.audit
    );

    // The project is held for the whole run.
    assert!(at.lock_held, "the store's lock is held during the call");
    assert!(at.marker, "the open marker is there during the call");

    // `status` reads meanwhile, without the lock.
    assert_eq!(at.status.code, 0, "{}", at.status.all());
    let block = status_block(&at.status.out, &id, std::slice::from_ref(&id));
    assert!(block[0].contains("hello"), "{block:#?}");
    assert!(block[0].contains("running"), "{block:#?}");
    assert!(step_line(&block, "greet").contains("running"), "{block:#?}");

    // After: the same three entries, then the settlement and the finishes.
    let audit = env.run_audit(&root);
    assert_eq!(audit[..3], at.audit[..]);
    assert_eq!(actions(&audit), RUN_ACTIONS);
    assert!(env.calls(&root, &id)[0].settled);
    assert_eq!(env.reader(&root).verify_audit_chain().unwrap(), None);
    env.assert_closed(&root);
}

/// Two steps: a two-step workflow runs both steps in order, each on the
/// provider of its own seat's Account, with its own seat's instructions
/// and model, its own instruction and its own output limit; two calls,
/// both settled.
#[tokio::test]
async fn two_steps_run_in_order_each_with_its_own_seat_and_model() {
    let env = env();
    let root = env.two_step();
    let first = fake(ScriptedTurn::text("the new items").usage(20, 5));
    let second = fake(ScriptedTurn::text("the summary").usage(30, 6));
    let kit = Kit::new()
        .with("local-lmstudio", first.clone())
        .with("local-ollama", second.clone());

    let ran = kit.run(&env, &root, &["digest"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let (id, status) = ran.run_line();
    assert_eq!(status, "succeeded");

    let mut asked: Vec<(String, String)> = kit
        .asked()
        .into_iter()
        .map(|m| (m.account, m.model))
        .collect();
    asked.sort();
    asked.dedup();
    assert_eq!(
        asked,
        vec![
            ("local-lmstudio".to_string(), "example-local".to_string()),
            ("local-ollama".to_string(), "example-small".to_string()),
        ]
    );
    assert_eq!((first.calls(), second.calls()), (1, 1));
    assert_request(
        &first.requests()[0],
        "example-local",
        &seat_instructions(&root, "alpha"),
        "List the items added since the last run.",
        4096,
    );
    // Story 3.3 (matrix row 2, Question 4 (a)): by `next`, the second step
    // also receives the first step's output, as one trusted input after its
    // instruction (3.1 sent the instruction alone).
    let mut second_request = second.requests()[0].clone();
    let input = second_request.messages[0].parts.pop();
    assert_eq!(
        input,
        Some(TrustedInput::from_step_output(
            "step:gather@1",
            "the new items",
            Taint::Clean
        )),
        "{:?}",
        second.requests()[0]
    );
    assert_request(
        &second_request,
        "example-small",
        &seat_instructions(&root, "beta"),
        "Write the summary.",
        256,
    );

    let steps = env.steps(&root, &id);
    assert_eq!(
        steps
            .iter()
            .map(|s| (
                s.seq,
                s.step_key.as_str(),
                s.status.as_str(),
                s.output.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, "gather", "succeeded", Some("the new items")),
            (1, "summarise", "succeeded", Some("the summary")),
        ]
    );
    // In order: the second step started after the first finished (the
    // store's timestamps sort as text).
    assert!(
        steps[0].finished_at.as_deref().unwrap() <= steps[1].started_at.as_deref().unwrap(),
        "{steps:?}"
    );
    let calls = env.calls(&root, &id);
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.settled), "{calls:?}");
    assert_eq!(
        calls
            .iter()
            .map(|c| (c.input_tokens, c.output_tokens))
            .collect::<Vec<_>>(),
        vec![(Some(20), Some(5)), (Some(30), Some(6))]
    );
    let runs = env.runs(&root);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].workflow.as_str(), runs[0].status.as_str()),
        ("digest", "succeeded")
    );
    assert_eq!(
        runs[0].config_hash.as_deref(),
        Some(snapshot_hash(&root).as_str())
    );
    assert_eq!(
        actions(&env.run_audit(&root)),
        vec![
            "run.created",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "run.finished",
        ]
    );
    assert_eq!(env.reader(&root).verify_audit_chain().unwrap(), None);
}

// ------------------------------------------------------------------ status

/// Status: after one succeeded and one failed run, `agentic status` lists
/// both, newest first, each with its workflow, status, snapshot hash and
/// duration, and under each its steps with their status and duration; a
/// step that never started is listed as queued.
#[tokio::test]
async fn status_lists_runs_newest_first_with_steps_hash_and_duration() {
    let env = env();
    let root = env.two_step();
    let ok = Kit::new()
        .with("local-lmstudio", fake(ScriptedTurn::text("the new items")))
        .with("local-ollama", fake(ScriptedTurn::text("the summary")));
    let first = ok.run(&env, &root, &["digest"]).await;
    assert_eq!(first.code, 0, "{}", first.all());
    tokio::time::sleep(Duration::from_millis(5)).await;
    let failing = Kit::new()
        .with(
            "local-lmstudio",
            fake(ScriptedTurn::fail(ProviderError::Http {
                status: 500,
                body: "upstream is down".into(),
            })),
        )
        .with("local-ollama", fake(ScriptedTurn::text("never")));
    let second = failing.run(&env, &root, &["digest"]).await;
    assert_eq!(second.code, 1, "{}", second.all());
    let (ok_id, _) = first.run_line();
    let (failed_id, failed_status) = second.run_line();
    assert_eq!(failed_status, "failed");
    let hash = snapshot_hash(&root);

    let ran = status_of(&env.places, &root);

    assert_eq!(ran.code, 0, "{}", ran.all());
    let at = |id: &str| {
        ran.out
            .find(id)
            .unwrap_or_else(|| panic!("{id}: {}", ran.out))
    };
    assert!(at(&failed_id) < at(&ok_id), "newest first: {}", ran.out);
    let ids = [ok_id.clone(), failed_id.clone()];

    let block = status_block(&ran.out, &ok_id, &ids);
    assert!(block[0].contains("digest"), "{block:#?}");
    assert!(block[0].contains("succeeded"), "{block:#?}");
    assert!(
        block[0].contains(&hash[..12]),
        "the snapshot hash: {block:#?}"
    );
    assert!(has_duration(&block[0]), "the run's duration: {block:#?}");
    assert!(!block[0].contains("running"), "{block:#?}");
    for key in ["gather", "summarise"] {
        let line = step_line(&block, key);
        assert!(line.contains("succeeded"), "{line}");
        assert!(has_duration(line), "the step's duration: {line}");
    }
    let (gather, summarise) = (step_line(&block, "gather"), step_line(&block, "summarise"));
    assert!(
        block.iter().position(|l| l == gather) < block.iter().position(|l| l == summarise),
        "steps in order: {block:#?}"
    );

    let block = status_block(&ran.out, &failed_id, &ids);
    assert!(block[0].contains("digest"), "{block:#?}");
    assert!(block[0].contains("failed"), "{block:#?}");
    assert!(block[0].contains(&hash[..12]), "{block:#?}");
    assert!(has_duration(&block[0]), "{block:#?}");
    let line = step_line(&block, "gather");
    assert!(line.contains("failed") && has_duration(line), "{line}");
    assert!(
        step_line(&block, "summarise").contains("queued"),
        "{block:#?}"
    );

    // Reading changed nothing and took no lock.
    assert_eq!(env.runs(&root).len(), 2);
    env.assert_closed(&root);
}

/// Status, no runs: a project opened once with no run says so and exits 0.
#[test]
fn status_with_no_runs_says_so() {
    let env = env();
    let root = env.copy_of("one-step");
    env.open_once(&root);

    let ran = status_of(&env.places, &root);

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.out.to_lowercase().contains("no runs"), "{}", ran.all());
}

/// Status reads while another process holds the project (`agentic serve`):
/// it takes no lock.
#[tokio::test]
async fn status_reads_while_the_project_is_held() {
    let env = env();
    let root = env.copy_of("one-step");
    let ran = Kit::local(fake(ScriptedTurn::text("done")))
        .run(&env, &root, &["hello"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let (id, _) = ran.run_line();
    let (held, _) =
        am_store::open_runtime(&env.layout(&root).unwrap(), "Holder Mac".into()).unwrap();

    let ran = status_of(&env.places, &root);

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.out.contains(&id), "{}", ran.all());
    drop(held);
}

/// Old database: `status` on a database from before this schema says, in
/// words, that the project must be opened once by this version; it
/// migrates nothing and writes nothing.
#[test]
fn status_on_an_older_database_says_to_open_the_project_once() {
    let env = env();
    let root = env.copy_of("one-step");
    env.open_once(&root);
    let db = env.layout(&root).unwrap().state_db();
    std::fs::remove_file(&db).unwrap();
    am_store::testing::database_at_version(&db, BEFORE_RUN_PIN).unwrap();
    let before = std::fs::read(&db).unwrap();

    let ran = status_of(&env.places, &root);

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("once"), "open it once: {}", ran.all());
    assert!(
        !ran.err.contains("no such column"),
        "in words: {}",
        ran.all()
    );
    assert_eq!(std::fs::read(&db).unwrap(), before, "not migrated");
}

/// No runtime folder: `status` on a project never opened on this Mac says
/// so in words and creates nothing.
#[test]
fn status_without_a_runtime_folder_says_so() {
    let env = env();
    let root = env.copy_of("one-step");

    let ran = status_of(&env.places, &root);

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("agentic"), "{}", ran.all());
    assert!(ran.out.trim().is_empty(), "{}", ran.all());
    assert!(!env.places.base.dir().exists(), "no runtime folder is made");
    assert!(env.layout(&root).is_none(), "no id is minted");
}

// ------------------------------------------------------------------ failures

/// Provider fails: the step and the run are `failed`; the command prints
/// `run <id> failed` and the reason, and exits 1; the call is settled as
/// the store settles a call with no usage; the project is closed again.
#[tokio::test]
async fn a_provider_failure_fails_the_run_and_prints_the_reason() {
    let env = env();
    let root = env.copy_of("one-step");
    let llm = fake(ScriptedTurn::fail(ProviderError::Http {
        status: 500,
        body: "upstream is down".into(),
    }));

    let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    let (id, status) = ran.run_line();
    assert_eq!(status, "failed");
    assert!(ran.all().contains("500"), "the reason: {}", ran.all());
    assert_eq!(llm.calls(), 1, "no retry in this story");

    let runs = env.runs(&root);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].id.as_str(), runs[0].status.as_str()),
        (id.as_str(), "failed")
    );
    assert!(runs[0].finished_at.is_some());
    let steps = env.steps(&root, &id);
    assert_eq!(steps[0].status, "failed");
    assert!(steps[0].finished_at.is_some());
    let calls = env.calls(&root, &id);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].settled, "no call is left open: {calls:?}");
    assert_eq!(
        (calls[0].input_tokens, calls[0].output_tokens),
        (None, None)
    );
    assert_eq!(actions(&env.run_audit(&root)), RUN_ACTIONS);
    assert_eq!(env.reader(&root).verify_audit_chain().unwrap(), None);
    env.assert_closed(&root);
}

/// Tool request: a turn that asks for a tool fails the step, because tools
/// are not available in this version, and the command says so.
#[tokio::test]
async fn a_tool_request_fails_the_run_because_no_tools_exist_yet() {
    let env = env();
    let root = env.copy_of("one-step");
    let llm = fake(ScriptedTurn::text("").tool_call("web_search", "{\"query\":\"x\"}"));

    let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    let (id, status) = ran.run_line();
    assert_eq!(status, "failed");
    let said = ran.all().to_lowercase();
    assert!(
        said.contains("tool") && said.contains("not available"),
        "{}",
        ran.all()
    );
    assert_eq!(llm.calls(), 1, "one turn, no tool loop");
    assert_eq!(env.steps(&root, &id)[0].status, "failed");
    assert_eq!(env.runs(&root)[0].status, "failed");
}

// ------------------------------------------------------------------ refusals

/// Unknown workflow: `run nope` exits 1 naming the workflows that exist;
/// no run row is written and no provider is called.
#[tokio::test]
async fn an_unknown_workflow_names_the_ones_that_exist() {
    let env = env();
    let root = env.copy_of("one-step");
    let llm = fake(ScriptedTurn::text("never"));

    let ran = Kit::local(llm.clone()).run(&env, &root, &["nope"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("nope"), "{}", ran.all());
    assert!(
        ran.err.contains("hello"),
        "names what exists: {}",
        ran.all()
    );
    assert!(
        !ran.out.lines().any(|l| l.starts_with("run ")),
        "{}",
        ran.out
    );
    assert!(env.runs(&root).is_empty(), "no run row");
    assert_eq!(llm.calls(), 0);
    env.assert_closed(&root);
}

/// Invalid config: a project whose config has an error starts no run and
/// says to run `agentic validate`.
#[tokio::test]
async fn a_config_error_starts_no_run_and_points_at_validate() {
    let env = env();
    let root = env.copy_of("one-step");
    write(
        &root,
        "workflows/hello.yaml",
        "schema_version: 2\nname: Hello\nteam: core\nsteps:\n  - id: greet\n    seat: nobody\n    instruction: Say hello in one line.\n",
    );
    let llm = fake(ScriptedTurn::text("never"));
    let kit = Kit::local(llm.clone());

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("agentic validate"), "{}", ran.all());
    assert!(env.runs(&root).is_empty(), "no run row");
    assert!(kit.asked().is_empty() && llm.calls() == 0);
    env.assert_closed(&root);
}

/// Pending migration: a config file older than this app's schema waits for
/// its migration; no run starts, and the command says why.
#[tokio::test]
async fn a_pending_config_migration_starts_no_run_and_says_why() {
    let env = env();
    let root = env.copy_of("one-step");
    write(
        &root,
        "workflows/hello.yaml",
        "schema_version: 1\nname: Hello\nteam: core\nsteps:\n  - id: greet\n    seat: alpha\n    instruction: Say hello in one line.\n",
    );
    let llm = fake(ScriptedTurn::text("never"));

    let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("migrat"), "{}", ran.all());
    assert!(env.runs(&root).is_empty(), "no run row");
    assert_eq!(llm.calls(), 0);
}

/// No model: a step whose seat has no `model` refuses the run, in words
/// naming the seat, before the run is created.
#[tokio::test]
async fn a_seat_without_a_model_refuses_before_the_run_is_created() {
    let env = env();
    let root = env.copy_of("one-step");
    write(
        &root,
        "seats/alpha.md",
        "---\nschema_version: 2\nname: Alpha\nrole: Writer\nteam: core\n---\nAnswer in one short line.\n",
    );
    let llm = fake(ScriptedTurn::text("never"));
    let kit = Kit::local(llm.clone());

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("alpha"), "names the seat: {}", ran.all());
    assert!(ran.err.contains("model"), "{}", ran.all());
    assert!(env.runs(&root).is_empty(), "no run row");
    assert!(kit.asked().is_empty() && llm.calls() == 0);
    env.assert_closed(&root);
}

/// A provider factory that refuses (as the binary's does) stops the
/// command with the factory's words before any run is created.
#[tokio::test]
async fn a_refusing_provider_factory_starts_no_run() {
    let env = env();
    let root = env.copy_of("one-step");
    let refuse: Providers =
        Arc::new(|model: &ModelRef| Err(format!("no provider for {} in this test", model.account)));

    let ran = run_with(&env, &root, &["hello"], refuse).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.err.contains("no provider for local-") && ran.err.contains("in this test"),
        "the factory's words: {}",
        ran.all()
    );
    assert!(env.runs(&root).is_empty(), "no run row");
    env.assert_closed(&root);
}

/// Binary's providers: the factory `main` gives the binary wires no
/// provider in this story. Run in process with an interactive terminal, it
/// stops the command saying so before any run is created. (The fake
/// provider lives in am-testkit and never reaches the binary:
/// `xtask/tests/release_deps.rs`.)
#[tokio::test]
async fn the_binarys_own_provider_factory_wires_no_provider() {
    let env = env();
    let root = env.copy_of("one-step");

    let ran = run_with(&env, &root, &["hello"], unwired_providers()).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    let said = ran.err.to_lowercase();
    assert!(
        said.contains("no model provider is wired") && said.contains("agentic run"),
        "{}",
        ran.all()
    );
    assert!(
        !ran.out.lines().any(|l| l.starts_with("run ")),
        "{}",
        ran.out
    );
    assert!(env.runs(&root).is_empty(), "no run row");
    env.assert_closed(&root);
}

/// Not a terminal (AD-15): a manual run start is the owner's decision, so
/// with a stdin that is not an interactive terminal the run is refused in
/// words naming the rule, before the project is opened: no id is minted,
/// no runtime folder is made, no marker is written, a database an open
/// would migrate is left as it is, nothing is read from the terminal and
/// no provider is asked for.
#[tokio::test]
async fn without_an_interactive_terminal_the_run_is_refused_before_the_project_is_opened() {
    let env = env();
    // A folder never opened on this Mac: nothing is created for it.
    let root = env.copy_of("one-step");
    let llm = fake(ScriptedTurn::text("never"));
    let kit = Kit::local(llm.clone()).piped();

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.err.contains("interactive terminal"),
        "names the terminal rule: {}",
        ran.all()
    );
    assert!(ran.err.contains("agentic run"), "{}", ran.all());
    assert!(
        !ran.out.lines().any(|l| l.starts_with("run ")),
        "{}",
        ran.out
    );
    assert!(env.layout(&root).is_none(), "no id is minted");
    assert!(!env.places.base.dir().exists(), "no runtime folder is made");
    assert!(!open_marker(&root).exists(), "no marker is written");
    assert_eq!(kit.term.reads(), 0, "nothing is read from the terminal");
    assert!(kit.asked().is_empty() && llm.calls() == 0);

    // A project opened before, whose database an open would migrate: the
    // store is not opened, so it is not migrated and holds no run row.
    let root = env.copy_as("one-step", "opened");
    env.open_once(&root);
    let layout = env.layout(&root).unwrap();
    let db = layout.state_db();
    std::fs::remove_file(&db).unwrap();
    am_store::testing::database_at_version(&db, BEFORE_RUN_PIN).unwrap();
    let before = std::fs::read(&db).unwrap();
    let kit = Kit::local(llm.clone()).piped();

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("interactive terminal"), "{}", ran.all());
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "not migrated, no run row"
    );
    assert!(!open_marker(&root).exists(), "no marker is written");
    assert!(!am_store::lock_is_held(&layout), "no lock is left held");
    assert!(kit.asked().is_empty() && llm.calls() == 0);
}

const STDIO_TOOLS: &str = "schema_version: 2\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n";

/// MCP or render enabled (AD-15): while the project's config enables a
/// stdio MCP server or a Source with `render: true` (children that run as
/// the owner), the CLI may not start a run. The refusal names what is
/// enabled; no run row is written and no provider is asked for. The valid
/// fixture enables such a server, so it is refused too.
#[tokio::test]
async fn an_enabled_stdio_mcp_server_or_rendering_source_refuses_the_run() {
    let env = env();
    let with_server = env.copy_as("one-step", "server");
    write(&with_server, "tools.yaml", STDIO_TOOLS);
    let valid = env.copy_of("valid");
    // The fixture with a rendering Source, given the one-step seat and
    // workflow.
    let rendering = env.copy_of("sources-ok");
    for rel in ["seats/alpha.md", "workflows/hello.yaml"] {
        let text =
            std::fs::read_to_string(am_testkit::fixture("projects/one-step").join(rel)).unwrap();
        write(&rendering, rel, &text);
    }

    for (root, workflow, names) in [
        (&with_server, "hello", ["MCP", "notes"]),
        (&valid, "digest", ["MCP", "notes"]),
        (&rendering, "hello", ["render", "docs"]),
    ] {
        assert!(
            am_config::load(root).unwrap().snapshot.is_some(),
            "{workflow}: the config itself is valid"
        );
        let llm = fake(ScriptedTurn::text("never"));
        let kit = Kit::new()
            .with("local-lmstudio", llm.clone())
            .with("local-ollama", llm.clone());

        let ran = kit.run(&env, root, &[workflow]).await;

        assert_eq!(ran.code, 1, "{}: {}", root.display(), ran.all());
        for name in names {
            assert!(
                ran.err.contains(name),
                "{}: names `{name}`: {}",
                root.display(),
                ran.all()
            );
        }
        assert!(
            !ran.out.lines().any(|l| l.starts_with("run ")),
            "{}",
            ran.out
        );
        assert!(env.runs(root).is_empty(), "no run row");
        assert!(kit.asked().is_empty() && llm.calls() == 0);
        env.assert_closed(root);
    }
}

/// Story 2.14 (AD-15 *Open*): an enabled `transport: http` MCP server on
/// this Mac (`http://127.0.0.1:<port>`, `https://localhost`) refuses the
/// Account commands like a stdio one, but `agentic run` keeps the stdio
/// rule: such a server does not refuse the run, which succeeds.
#[tokio::test]
async fn a_loopback_http_mcp_server_does_not_refuse_the_run() {
    let env = env();
    for (name, url) in [
        ("loopback", "http://127.0.0.1:3001/mcp"),
        ("localhost", "https://localhost/mcp"),
    ] {
        let root = env.copy_as("one-step", name);
        write(
            &root,
            "tools.yaml",
            &format!(
                "schema_version: 2\nmcp_servers:\n  - id: loop-notes\n    transport: http\n    url: {url}\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n"
            ),
        );
        assert!(
            am_config::load(&root).unwrap().snapshot.is_some(),
            "{url}: the config itself is valid"
        );
        let llm = fake(ScriptedTurn::text("done"));
        let kit = Kit::local(llm.clone());

        let ran = kit.run(&env, &root, &["hello"]).await;

        assert_eq!(ran.code, 0, "{url}: {}", ran.all());
        assert_eq!(ran.run_line().1, "succeeded", "{url}");
        assert_eq!(env.runs(&root).len(), 1, "{url}");
        env.assert_closed(&root);
    }
}

/// A file error is not an enabled child (review of story 3.1): when
/// `tools.yaml` has an error, the refusal says the file has an error and
/// points at `agentic validate`; it does not say a server is enabled or to
/// remove one. The project is not opened and no run row is written.
#[tokio::test]
async fn a_broken_tools_file_refuses_the_run_pointing_at_validate() {
    let env = env();
    let root = env.copy_of("one-step");
    write(&root, "tools.yaml", "schema_version: 2\nmcp_servers: 7\n");
    assert!(
        am_config::load(&root).unwrap().snapshot.is_none(),
        "the file has an error"
    );
    let llm = fake(ScriptedTurn::text("never"));
    let kit = Kit::local(llm.clone());

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("tools.yaml"), "{}", ran.all());
    assert!(ran.err.contains("has an error"), "{}", ran.all());
    assert!(ran.err.contains("agentic validate"), "{}", ran.all());
    assert!(
        !ran.err.contains("is enabled") && !ran.err.contains("remove or disable"),
        "nothing is said to be enabled: {}",
        ran.all()
    );
    assert!(env.layout(&root).is_none(), "the project is not opened");
    assert!(env.runs(&root).is_empty(), "no run row");
    assert!(kit.asked().is_empty() && llm.calls() == 0);
}

/// An identity adapter that, the first time the open asks it for this
/// Mac's name (after `run`'s own check before the open), writes a
/// `tools.yaml` that enables a stdio MCP server.
struct EnablesServerDuringOpen {
    inner: FakeProjectIdentity,
    root: PathBuf,
    done: std::sync::atomic::AtomicBool,
}

impl am_domain::ProjectIdentity for EnablesServerDuringOpen {
    fn root(&self, root: &Path) -> Result<am_domain::RootIdentity, am_domain::IdentityError> {
        self.inner.root(root)
    }
    fn locate(
        &self,
        recorded: &am_domain::RootIdentity,
    ) -> Result<am_domain::VolumeState, am_domain::IdentityError> {
        self.inner.locate(recorded)
    }
    fn mac_name(&self) -> Result<String, am_domain::IdentityError> {
        if !self.done.swap(true, std::sync::atomic::Ordering::SeqCst) {
            write(&self.root, "tools.yaml", STDIO_TOOLS);
        }
        self.inner.mac_name()
    }
}

/// The child check is bound to what the run pins (review of story 3.1): a
/// stdio MCP server enabled after the check before the open, and so held
/// by the snapshot the run would be built from, still refuses the run,
/// naming it. The project was opened and is closed again; no run row.
#[tokio::test]
async fn a_server_the_pinned_snapshot_enables_refuses_the_run() {
    let env = env();
    let root = env.copy_of("one-step");
    let llm = fake(ScriptedTurn::text("never"));
    let kit = Kit::local(llm.clone());
    let ctx = RunContext {
        places: env.places.clone(),
        identity: Arc::new(EnablesServerDuringOpen {
            inner: FakeProjectIdentity::new("Test Mac"),
            root: root.clone(),
            done: std::sync::atomic::AtomicBool::new(false),
        }),
        terminal: Box::new(FakeTerminal::new()),
        providers: kit.providers(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());

    let code = run(&with_project(&["hello"], &root), ctx, &mut out, &mut err).await;

    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(err.contains("MCP") && err.contains("notes"), "{err}");
    assert!(!out.lines().any(|l| l.starts_with("run ")), "{out}");
    assert!(
        env.layout(&root).is_some(),
        "the open ran: the check before it passed"
    );
    assert!(env.runs(&root).is_empty(), "no run row");
    assert!(kit.asked().is_empty() && llm.calls() == 0);
    env.assert_closed(&root);
}

/// Outside change (AD-16; review of story 3.1): a config file the app
/// wrote (company.yaml, after the open gave the project its id) and that
/// was then edited by hand waits for Reload, so no run is built from it.
/// The refusal names the file in the words story 2.1's Account edit uses;
/// no run row is written. With the file put back, the run starts again.
/// Only a file the app wrote is judged: one it never wrote has no History
/// entry, and a hand edit of it is not refused by this check.
#[tokio::test]
async fn a_file_the_app_wrote_that_was_changed_outside_it_refuses_the_run() {
    let env = env();
    let root = env.copy_of("one-step");
    let first = Kit::local(fake(ScriptedTurn::text("done")))
        .run(&env, &root, &["hello"])
        .await;
    assert_eq!(first.code, 0, "{}", first.all());
    let written = std::fs::read_to_string(root.join("company.yaml")).unwrap();
    assert!(written.contains("project_id:"), "the app wrote the id");
    let edited = written.replace("name: One Step", "name: One Step, edited");
    assert_ne!(edited, written);
    std::fs::write(root.join("company.yaml"), &edited).unwrap();
    assert!(
        am_config::load(&root).unwrap().snapshot.is_some(),
        "the edited config is valid"
    );
    let llm = fake(ScriptedTurn::text("never"));
    let kit = Kit::local(llm.clone());

    let ran = kit.run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("company.yaml"), "{}", ran.all());
    assert!(
        ran.err.contains("changed on disk outside the app"),
        "{}",
        ran.all()
    );
    assert!(ran.err.contains("Reload"), "{}", ran.all());
    assert!(
        !ran.out.lines().any(|l| l.starts_with("run ")),
        "{}",
        ran.out
    );
    assert_eq!(env.runs(&root).len(), 1, "no new run row");
    assert!(kit.asked().is_empty() && llm.calls() == 0);
    env.assert_closed(&root);

    // Put back as the app last wrote it: the run starts.
    std::fs::write(root.join("company.yaml"), &written).unwrap();
    let again = Kit::local(fake(ScriptedTurn::text("done")))
        .run(&env, &root, &["hello"])
        .await;
    assert_eq!(again.code, 0, "{}", again.all());
    assert_eq!(env.runs(&root).len(), 2);
}

/// What this version cannot honour (review of story 3.1): a step that
/// declares a run-graph key (a review gate, a hand-off, a fan-out over
/// seats) or an `output_schema` would run as one plain turn and report
/// `succeeded` with the gate, hand-off, fan-out or schema skipped. Each is
/// refused before the run is created, naming the step and the key; no
/// provider is asked for and no run row is written.
#[tokio::test]
async fn a_step_this_version_cannot_honour_refuses_the_run_by_name() {
    const SCHEMA: &str = "{\n  \"schema_version\": 2,\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"type\": \"object\",\n  \"required\": [\"line\"],\n  \"properties\": {\n    \"line\": { \"type\": \"string\" }\n  }\n}\n";
    for (key, step) in [
        (
            "review",
            "    seat: alpha\n    review:\n      seat: alpha\n      max_returns: 2\n",
        ),
        (
            "handoff",
            "    seat: alpha\n    handoff:\n      to: core/hello/check\n      payload: document:note\n",
        ),
        (
            "output_schema",
            "    seat: alpha\n    output_schema: line\n",
        ),
        ("seats", "    seats:\n      - alpha\n"),
    ] {
        let env = env();
        let root = env.copy_of("one-step");
        write(&root, "schemas/line.json", SCHEMA);
        write(
            &root,
            "workflows/hello.yaml",
            &format!(
                "schema_version: 2\nname: Hello\nteam: core\nsteps:\n  - id: check\n    instruction: Say hello in one line.\n{step}"
            ),
        );
        let loaded = am_config::load(&root).unwrap();
        assert!(
            loaded.snapshot.is_some(),
            "{key}: the config itself is valid: {:#?}",
            loaded.findings
        );
        let llm = fake(ScriptedTurn::text("never"));
        let kit = Kit::local(llm.clone());

        let ran = kit.run(&env, &root, &["hello"]).await;

        assert_eq!(ran.code, 1, "{key}: {}", ran.all());
        assert!(
            ran.err.contains("`check`"),
            "{key}: names the step: {}",
            ran.all()
        );
        assert!(
            ran.err.contains(&format!("`{key}`")),
            "{key}: names the key: {}",
            ran.all()
        );
        assert!(
            ran.err.contains("agentic run") && ran.err.contains("cannot honour"),
            "{key}: {}",
            ran.all()
        );
        assert!(
            !ran.out.lines().any(|l| l.starts_with("run ")),
            "{key}: {}",
            ran.out
        );
        assert!(env.runs(&root).is_empty(), "{key}: no run row");
        assert!(kit.asked().is_empty() && llm.calls() == 0, "{key}");
        env.assert_closed(&root);
    }
}

/// Busy: while another process holds the store's lock, the run is refused
/// naming the holder; no run row, no model call.
#[tokio::test]
async fn while_the_store_is_held_the_run_is_refused_naming_the_holder() {
    let env = env();
    let root = env.copy_of("one-step");
    env.open_once(&root);
    let (held, _) =
        am_store::open_runtime(&env.layout(&root).unwrap(), "Holder Mac".into()).unwrap();
    let llm = fake(ScriptedTurn::text("never"));

    let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.err.contains("Holder Mac"),
        "names the holder: {}",
        ran.all()
    );
    assert_eq!(llm.calls(), 0);
    assert!(env.runs(&root).is_empty(), "no run row");
    drop(held);
}

/// No question is asked: a project marked open on another Mac is refused
/// in words pointing at `agentic serve`, which takes over; the marker is
/// left as it was.
#[tokio::test]
async fn another_macs_marker_refuses_the_run_pointing_at_serve() {
    let env = env();
    let root = env.copy_of("one-step");
    env.open_once(&root);
    let marker = format!(
        "mac_name: Studio Mac\nmac_id: {}\npid: 4242\nopened_at: 2026-11-20T10:00:00.000Z\n",
        MacId::mint()
    );
    std::fs::write(open_marker(&root), &marker).unwrap();
    let llm = fake(ScriptedTurn::text("never"));

    let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("Studio Mac"), "{}", ran.all());
    assert!(ran.err.contains("agentic serve"), "{}", ran.all());
    assert_eq!(llm.calls(), 0);
    assert!(env.runs(&root).is_empty(), "no run row");
    assert_eq!(
        std::fs::read_to_string(open_marker(&root)).unwrap(),
        marker,
        "not taken over"
    );
}

/// Usage errors exit 2 with the usage line; nothing is opened.
#[tokio::test]
async fn bad_usage_exits_two() {
    let env = env();
    let root = env.copy_of("one-step");
    let p = root.to_string_lossy().into_owned();
    for args in [
        vec![],
        vec!["--project", p.as_str()],
        vec!["hello", "again", "--project", p.as_str()],
        vec!["hello", "--frobnicate", "--project", p.as_str()],
        vec!["hello", "--project"],
    ] {
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let llm = fake(ScriptedTurn::text("never"));
        let ran = run_argv(
            &env,
            &argv,
            Kit::local(llm.clone()).providers(),
            FakeTerminal::new(),
        )
        .await;
        assert_eq!(ran.code, 2, "run {args:?}: {}", ran.all());
        assert!(
            ran.err.contains("usage: agentic run"),
            "run {args:?}: {}",
            ran.all()
        );
        assert_eq!(llm.calls(), 0);
    }
    for args in [
        vec!["extra", "--project", p.as_str()],
        vec!["--frobnicate"],
        vec!["--project"],
    ] {
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ran = status_argv(&env.places, &argv);
        assert_eq!(ran.code, 2, "status {args:?}: {}", ran.all());
        assert!(
            ran.err.contains("usage: agentic status"),
            "status {args:?}: {}",
            ran.all()
        );
    }
    assert!(env.layout(&root).is_none(), "nothing was opened");
}

// ------------------------------------------------------------------ binary

/// Not a terminal, on the real binary: `agentic run` with piped stdin (a
/// "yes" ready on it) exits 1 naming the terminal rule and does not open
/// the project: the database is not touched, no marker is written and no
/// run row exists. `agentic status` has no terminal rule: with piped stdin
/// it reads the project and exits 0.
#[test]
fn the_binary_refuses_run_with_piped_stdin_and_status_needs_no_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let support = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        dir.path().join("xdg")
    };
    let places = Places::under(&support, &home);
    let root = dir.path().join("project");
    am_testkit::copy_tree(&am_testkit::fixture("projects/one-step"), &root);
    let root = root.canonicalize().unwrap();
    // Opened once through the system identity adapter, as the binary
    // resolves it.
    let identity = am_app::identity::system();
    let req = OpenRequest {
        root: root.clone(),
        confirm_move: false,
        take_over: false,
    };
    drop(open_project(&places, identity.as_ref(), &req, &mut NoPrompt).unwrap());
    let layout = layout_in(&places, &root).expect("the project has an id");
    let db_before = std::fs::read(layout.state_db()).unwrap();
    // Runs `agentic <args> --project <root>` with `typed` on a stdin pipe.
    let agentic = |args: &[&str], typed: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentic"))
            .args(args)
            .arg("--project")
            .arg(&root)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", dir.path().join("xdg"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The command may end before reading: a closed pipe is fine.
        let _ = child.stdin.take().unwrap().write_all(typed.as_bytes());
        let o = child.wait_with_output().unwrap();
        Ran {
            code: o.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&o.stdout).into_owned(),
            err: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    };

    let ran = agentic(&["run", "hello"], "y\nyes\n");

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.err.contains("terminal"),
        "names the terminal rule: {}",
        ran.all()
    );
    assert!(
        !ran.out.lines().any(|l| l.starts_with("run ")),
        "{}",
        ran.out
    );
    assert_eq!(
        std::fs::read(layout.state_db()).unwrap(),
        db_before,
        "the store was not opened"
    );
    assert!(!open_marker(&root).exists(), "no marker is written");
    assert!(!am_store::lock_is_held(&layout), "no lock is left held");
    assert!(runs_at(Some(&layout)).is_empty(), "no run row");

    let ran = agentic(&["status"], "");
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.out.to_lowercase().contains("no runs"), "{}", ran.all());
}

// ------------------------------------------------------------------ story 3.3
//
// Epic 3 story 3: `agentic status` lists every step attempt and a waiting
// step (matrix rows 8 and 27), and the run-graph keys stay refused by
// `agentic run` (row 30), on stream copies of `fixtures/projects/hand-offs`
// (all its model seats on `local-lmstudio`; the `owner` seat has no model).

/// Every line under a run's status block that names step `key` as a word.
fn step_lines<'a>(block: &'a [String], key: &str) -> Vec<&'a str> {
    block[1..]
        .iter()
        .filter(|l| l.split_whitespace().any(|w| w.trim_matches(':') == key))
        .map(String::as_str)
        .collect()
}

/// Rows 8 and 27: after a loop-back run of `relay` and a run of `gated`
/// that waits at its gate, `agentic status` lists, under the first run,
/// every attempt of each step (`attempt 1`, `attempt 2`), the producing
/// step's first attempt `looped_back`; and under the second, the run
/// `running`, the gate step `waiting_human` and the step after it `queued`.
/// `agentic run gated` said `waiting_human` and exited 0; status reads
/// without the lock.
#[tokio::test]
async fn status_lists_attempts_and_a_waiting_step() {
    let env = env();
    let root = env.copy_of("hand-offs");
    let relay = Kit::local(Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("version one"),
        ScriptedTurn::text("edited one"),
        ScriptedTurn::text(r#"{"reasons":["tighten the opening"]}"#),
        ScriptedTurn::text("version two"),
        ScriptedTurn::text("edited two"),
        ScriptedTurn::text(r#"{"reasons":[]}"#),
    ])))
    .run(&env, &root, &["relay"])
    .await;
    assert_eq!(relay.code, 0, "{}", relay.all());
    let (relay_id, status) = relay.run_line();
    assert_eq!(status, "succeeded");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let gated = Kit::local(fake(ScriptedTurn::text("the version to approve")))
        .run(&env, &root, &["gated"])
        .await;
    assert_eq!(gated.code, 0, "waiting is not failing: {}", gated.all());
    let (gated_id, status) = gated.run_line();
    assert_eq!(status, "waiting_human");

    let ran = status_of(&env.places, &root);

    assert_eq!(ran.code, 0, "{}", ran.all());
    let ids = [relay_id.clone(), gated_id.clone()];
    let block = status_block(&ran.out, &relay_id, &ids);
    assert!(block[0].contains("succeeded"), "{block:#?}");
    for key in ["draft", "edit", "check"] {
        let lines = step_lines(&block, key);
        assert_eq!(lines.len(), 2, "{key}: two attempts: {block:#?}");
        assert!(lines[0].contains("attempt 1"), "{key}: {block:#?}");
        assert!(lines[1].contains("attempt 2"), "{key}: {block:#?}");
    }
    assert!(
        step_lines(&block, "check")[0].contains("looped_back"),
        "{block:#?}"
    );
    assert!(
        step_lines(&block, "check")[1].contains("succeeded"),
        "{block:#?}"
    );

    let block = status_block(&ran.out, &gated_id, &ids);
    assert!(block[0].contains("gated"), "{block:#?}");
    assert!(block[0].contains("running"), "{block:#?}");
    assert!(
        step_line(&block, "draft").contains("succeeded"),
        "{block:#?}"
    );
    assert!(
        step_line(&block, "approve").contains("waiting_human"),
        "{block:#?}"
    );
    assert!(
        step_line(&block, "publish").contains("queued"),
        "{block:#?}"
    );
    env.assert_closed(&root);
}

/// Row 30: a step that declares a run-graph key (`handoff`, `review`,
/// `seats` or `join`; AD-40..AD-42) is still refused by `agentic run` as in
/// 3.1, before any run row and before any provider is asked for. Green on
/// arrival (a regression guard).
#[tokio::test]
async fn run_graph_keys_are_still_refused() {
    for (key, lines) in [
        (
            "handoff",
            "    seat: alpha\n    handoff:\n      to: core/hello/check\n      payload: document:note\n",
        ),
        (
            "review",
            "    seat: alpha\n    review:\n      seat: alpha\n      max_returns: 2\n",
        ),
        ("seats", "    seats:\n      - alpha\n"),
        ("join", "    seat: alpha\n    join: all\n"),
    ] {
        let env = env();
        let root = env.copy_of("one-step");
        write(
            &root,
            "workflows/hello.yaml",
            &format!(
                "schema_version: 2\nname: Hello\nteam: core\nsteps:\n  - id: check\n    instruction: Say hello in one line.\n{lines}"
            ),
        );
        let llm = fake(ScriptedTurn::text("never"));
        let kit = Kit::local(llm.clone());

        let ran = kit.run(&env, &root, &["hello"]).await;

        assert_eq!(ran.code, 1, "{key}: {}", ran.all());
        assert!(
            ran.err.contains(&format!("`{key}`")),
            "{key}: names the key: {}",
            ran.all()
        );
        assert!(env.runs(&root).is_empty(), "{key}: no run row");
        assert!(kit.asked().is_empty() && llm.calls() == 0, "{key}");
    }
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P15.

/// P15: a tool name is the model's choice, so `agentic run` never prints
/// it raw: with control characters (an escape sequence, a bell, a
/// newline), 10 KB long, or a plain unknown name, the run fails in words
/// that hold no control character (beyond the line ends), no part of the
/// raw name and nothing near its length.
#[tokio::test]
async fn a_tool_name_is_never_printed_raw() {
    let long = "x".repeat(10 * 1024);
    let cases: [(&str, String, String); 3] = [
        (
            "control characters",
            "zq9\u{1b}[2J\u{7}wipe\nname".to_string(),
            "zq9".to_string(),
        ),
        ("10 KB", long.clone(), "x".repeat(64)),
        (
            "unknown",
            "exfiltrate_every_key".to_string(),
            "exfiltrate_every_key".to_string(),
        ),
    ];
    for (what, name, marker) in cases {
        let env = env();
        let root = env.copy_of("one-step");
        let llm = fake(ScriptedTurn::text("").tool_call(&name, "{}"));

        let ran = Kit::local(llm.clone()).run(&env, &root, &["hello"]).await;

        assert_eq!(ran.code, 1, "{what}");
        let said = ran.all();
        assert!(!said.contains(&marker), "{what}: the raw name is printed");
        assert!(
            !said.chars().any(|c| c.is_control() && c != '\n'),
            "{what}: a control character is printed"
        );
        assert!(said.len() < 4096, "{what}: {} bytes printed", said.len());
        assert!(
            said.to_lowercase().contains("tool"),
            "{what}: still says why: {said:.300}"
        );
    }
}
