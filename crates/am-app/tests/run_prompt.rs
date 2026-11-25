//! Epic 3 story 5 (AD-27, AD-36, AD-38; matrix rows 26, 27 and 29): `agentic
//! run`'s plan builder fills a step's prompt slots from the seat in the
//! config snapshot and hands the engine the step's `output_schema`.
//!
//! On a stream copy of `fixtures/projects/prompt-parts` (one seat with a
//! goal, a backstory, one skill with a reference file, two reference files
//! and two saved links; one workflow whose one step declares `output_schema:
//! line`), run in process through `am_app::cli::run` with am-testkit's
//! `FakeLlm`:
//!
//! - the request holds, in order, the seat's role text (unchanged), `Goal`,
//!   `Backstory`, `skill:brief` (its body, then `references/layout.md`),
//!   `Reference files` and `Saved links` as system parts, each headed
//!   (`am_engine::prompt::heading`), and the step's instruction as the one
//!   user part; no CEO, guidance or memory part (their slots have no
//!   source yet, Question 1) and no part for anything the seat lacks;
//! - the journal holds the same parts, labelled;
//! - `output_schema` is no longer refused: a matching message succeeds, an
//!   invalid one is repaired once with the schema's own text in the repair
//!   instruction, a second invalid one fails the step and the command; a
//!   schema the engine cannot compile refuses the run before a row is
//!   written.
//!
//! No network, no real model. Written before the slots or the schema are
//! built.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_app::cli::{RunContext, run};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_config::ModelRef;
use am_domain::{LlmProvider, Part, Role, RuntimeLayout};
use am_engine::prompt::heading;
use am_store::{PromptPartRow, Reader};
use am_testkit::{FakeLlm, FakeProjectIdentity, FakeTerminal, ScriptedTurn};

const GOOD: &str = r#"{"line":"hello"}"#;
const BAD: &str = "hello there";

struct NoPrompt;

impl Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

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
    /// A copy of the fixture, opened once on this Mac, with the seat's
    /// reference files in the runtime folder where the app keeps them.
    fn fixture(&self) -> PathBuf {
        let root = self.dir.path().join("projects").join("prompt-parts");
        am_testkit::copy_tree(&am_testkit::fixture("projects/prompt-parts"), &root);
        let root = root.canonicalize().unwrap();
        let req = OpenRequest {
            root: root.clone(),
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
        let layout = self.layout(&root);
        let to = layout.knowledge("alpha");
        std::fs::create_dir_all(&to).unwrap();
        for entry in std::fs::read_dir(root.join("runtime-seed/knowledge/alpha")).unwrap() {
            let entry = entry.unwrap();
            am_testkit::stream_copy_file(&entry.path(), &to.join(entry.file_name()));
        }
        root
    }

    fn layout(&self, root: &Path) -> RuntimeLayout {
        let id = am_config::read_company(root).unwrap().project_id.unwrap();
        self.places.base.project(&id)
    }

    fn reader(&self, root: &Path) -> Reader {
        Reader::open(&self.layout(root).state_db()).unwrap()
    }

    /// `agentic run hello` on the fixture, every model served by `llm`.
    async fn run(&self, root: &Path, llm: &Arc<FakeLlm>) -> Ran {
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
            "hello".to_string(),
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

    /// The prompt rows of the one step attempt of the project's one run.
    fn parts(&self, root: &Path) -> Vec<PromptPartRow> {
        let reader = self.reader(root);
        let run = reader.runs().unwrap().remove(0);
        let step = reader.steps(&run.id).unwrap().remove(0);
        reader.prompt_parts(&step.id).unwrap()
    }
}

fn llm(turns: &[&str]) -> Arc<FakeLlm> {
    Arc::new(FakeLlm::new(
        turns.iter().map(|t| ScriptedTurn::text(*t)).collect(),
    ))
}

/// Row 29: the request's system parts are the seat's role text, then the
/// five headed parts in AD-27's order; the one user part is the step's
/// instruction; the journal holds the same six, then the step.
#[tokio::test]
async fn plan_of_builds_the_seat_parts_from_the_prompt_parts_fixture() {
    let env = env();
    let root = env.fixture();
    let model = llm(&[GOOD]);

    let ran = env.run(&root, &model).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let request = model.requests()[0].clone();
    let system: Vec<String> = request
        .system
        .iter()
        .map(|i| i.text().to_string())
        .collect();
    assert_eq!(system.len(), 6, "{system:#?}");
    assert_eq!(
        system[0].trim(),
        "Answer in one short line.",
        "the role text, unchanged"
    );
    for (text, label, body) in [
        (&system[1], "Goal", "Keep every line short and plain."),
        (
            &system[2],
            "Backstory",
            "Has written one-line notes for years and prefers few words.",
        ),
        (
            &system[3],
            "skill:brief",
            "Keep each brief under ten lines.",
        ),
        (&system[4], "Reference files", "guide.md"),
        (
            &system[5],
            "Saved links",
            "Home \u{2014} https://example.org/",
        ),
    ] {
        assert!(text.starts_with(&heading(label)), "{label}: {text}");
        assert!(text.contains(body), "{label}: {text}");
    }
    assert!(system[3].contains("references/layout.md"), "{}", system[3]);
    assert!(system[4].contains("terms.txt"), "{}", system[4]);
    assert!(
        system[5].contains("Reference \u{2014} https://docs.example.org/reference"),
        "{}",
        system[5]
    );
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, Role::User);
    match request.messages[0].parts.as_slice() {
        [Part::Instruction(i)] => assert_eq!(i.text().trim(), "Say hello in one line."),
        other => panic!("one user Instruction part: {other:?}"),
    }

    let parts = env.parts(&root);
    let labels: Vec<&str> = parts.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Role instructions",
            "Goal",
            "Backstory",
            "skill:brief",
            "Reference files",
            "Saved links",
            "Step instruction"
        ]
    );
    for (row, sent) in parts.iter().zip(system.iter()) {
        assert_eq!(row.text.as_deref(), Some(sent.as_str()), "{}", row.label);
    }
}

/// Question 1: CEO goals, guidance notes and memory notes have no source
/// in the config yet, so the shipped plan builder leaves their slots
/// empty: no part of theirs is sent or journaled.
#[tokio::test]
async fn plan_of_leaves_ceo_guidance_and_memory_empty() {
    let env = env();
    let root = env.fixture();
    let model = llm(&[GOOD]);

    let ran = env.run(&root, &model).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let request = model.requests()[0].clone();
    for label in ["CEO goal:", "Guidance:", "Memory:"] {
        let heading_start = heading(label);
        let stem = heading_start.trim_end();
        assert!(
            request.system.iter().all(|i| !i.text().contains(stem)),
            "no `{label}` part: {:?}",
            request.system
        );
    }
    for row in env.parts(&root) {
        for label in ["CEO goal", "Guidance", "Memory"] {
            assert!(!row.label.starts_with(label), "{row:?}");
        }
    }
}

/// Row 29 and AD-36: `output_schema` is honoured, not refused. A matching
/// message succeeds in one call. An invalid one is repaired once: the
/// second request ends with the assistant's reply and the repair
/// instruction, which holds the schema's text (built here from the schema
/// file); the command succeeds and the journal marks the repair. Two
/// invalid messages fail the step and the command (exit 1) naming the step
/// and the schema, with no third call.
#[tokio::test]
async fn output_schema_is_honoured_by_agentic_run() {
    // Valid first time.
    let env_a = env();
    let root = env_a.fixture();
    let model = llm(&[GOOD]);
    let ran = env_a.run(&root, &model).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(!ran.err.contains("cannot honour"), "{}", ran.err);
    assert_eq!(model.calls(), 1);

    // Repaired once.
    let env_b = env();
    let root = env_b.fixture();
    let model = llm(&[BAD, GOOD]);
    let ran = env_b.run(&root, &model).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(model.calls(), 2);
    let second = model.requests()[1].clone();
    let last = second.messages.last().unwrap();
    assert_eq!(last.role, Role::User);
    let instruction = match last.parts.as_slice() {
        [Part::Instruction(i)] => i.text().to_string(),
        other => panic!("the repair instruction last: {other:?}"),
    };
    for word in ["line", "required", "string"] {
        assert!(
            instruction.contains(word),
            "the schema's text, `{word}`: {instruction}"
        );
    }
    let reply = &second.messages[second.messages.len() - 2];
    assert_eq!(reply.role, Role::Assistant);
    assert_eq!(reply.parts, [Part::AssistantText(BAD.to_string())]);
    let reader = env_b.reader(&root);
    let run_row = reader.runs().unwrap().remove(0);
    let step = reader.steps(&run_row.id).unwrap().remove(0);
    let calls = reader.calls_for_step(&step.id).unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].purpose, am_store::CallPurpose::Repair);

    // Invalid twice.
    let env_c = env();
    let root = env_c.fixture();
    let model = llm(&[BAD, BAD, GOOD]);
    let ran = env_c.run(&root, &model).await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert_eq!(model.calls(), 2, "one repair, then fail");
    for word in ["write", "line"] {
        assert!(ran.err.contains(word), "`{word}`: {}", ran.err);
    }
    assert!(
        !ran.err.contains(BAD),
        "never the model's text: {}",
        ran.err
    );
    let reader = env_c.reader(&root);
    let run_row = reader.runs().unwrap().remove(0);
    assert_eq!(run_row.status, "failed");
}

/// Rows 26 and 27: a schema the engine cannot compile (a look-around
/// pattern the linear-time engine refuses) refuses the run before any row
/// is written, naming the schema; no model is asked.
#[tokio::test]
async fn a_schema_the_engine_cannot_compile_refuses_the_run() {
    let env = env();
    let root = env.fixture();
    std::fs::write(
        root.join("schemas/line.json"),
        r#"{
  "schema_version": 2,
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["line"],
  "properties": { "line": { "type": "string", "pattern": "(?=h)hello" } }
}
"#,
    )
    .unwrap();
    let model = llm(&[GOOD]);

    let ran = env.run(&root, &model).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("line"), "names the schema: {}", ran.err);
    assert_eq!(model.calls(), 0, "no model was asked");
    assert!(env.reader(&root).runs().unwrap().is_empty(), "no run row");
}
