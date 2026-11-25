//! The golden rig (epic 3 story 24; AD-23, AD-33; owner, 2026-11-25, Q1
//! and Q3): the sample company validated and run through the shipped
//! commands, in process, on scripted fakes.
//!
//! One pass ([`pass`]) has three stages, each on its own stream copy of the
//! sample under its own temporary HOME:
//!
//! 1. `agentic validate --template` on a copy never opened;
//! 2. `agentic run` of the real flow (the flow `sample.json` names, as the
//!    sample holds it): today refused before any run row;
//! 3. `agentic run` of the cut-down copy (`fixtures/golden/so-far/
//!    workflow.yaml`), written over the flow's file before the copy's first
//!    open, on one FakeLlm script per seat behind a [`Router`].
//!
//! Its journal and outputs render as tagged text lines ([`render`]),
//! normalised ([`normalise`]) and compared with the hand-written
//! `snapshot.txt` ([`compare`], [`assert_snapshot`]); the test never
//! writes a fixture.
//!
//! The drift check ([`cut`], [`text_drift`], [`structure_drift`],
//! [`list_problems`]) proves the copy is the real flow with exactly the
//! entries of `golden.json`'s removed list taken out, each tagged with a
//! story of the epic that restores it.
//!
//! Every sample id (the sample's folder, the flow, its steps and seats) is
//! read from `fixtures/templates/sample.json` and `fixtures/golden/so-far/`;
//! this folder spells none (AD-23, DR-016, [`spelled_ids`]).
//!
//! No model; no network and no keychain, structurally (review R6): the only
//! providers are the router and a tripwire on an empty `FakeEgress`
//! ([`Factory`]), and `RunContext` is built field by field ([`run_flow`]),
//! so a secret port added to it later fails to compile here (register
//! DR-173). What the tests prove is this wiring, not a count of sockets or
//! keychain calls.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use am_app::cli::RunContext;
use am_config::{ConfigSnapshot, LoadOptions, ModelRef, Step, Workflow};
use am_domain::{
    BoxFuture, CancelSignal, ChatOutcome, ChatRequest, EventSink, LlmProvider, ModelCapabilities,
    Part, ProviderError, Role,
};
use am_providers::OpenAiCompatProvider;
use am_testkit::{FakeClock, FakeEgress, FakeLlm, FakeProjectIdentity, FakeTerminal, ScriptedTurn};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::support::{self, Cli, Providers, Ran};

/// The account id of the tripwire provider.
pub const TRIPWIRE: &str = "golden-tripwire";
/// A loopback endpoint nothing listens on; the tripwire's egress has no
/// route for it anyway, so a request is recorded and refused.
pub const TRIPWIRE_URL: &str = "http://127.0.0.1:1/v1";
/// What a planted change appends.
pub const PLANTED: &str = " (planted)";
/// The name of the actual snapshot under `CARGO_TARGET_TMPDIR`.
pub const ACTUAL: &str = "golden_so_far.actual.txt";

// ------------------------------------------------------------------ fixtures

/// `fixtures/golden/so-far/`.
pub fn home() -> PathBuf {
    am_testkit::fixture("golden/so-far")
}

/// The removed list (`golden.json`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub about: String,
    /// The FakeClock instant of every `agentic run` (RFC 3339).
    pub clock: String,
    pub stories: Stories,
    pub removed_keys: Vec<RemovedKey>,
    pub removed_steps: Vec<RemovedStep>,
}

/// Where the restoring stories are listed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stories {
    /// The epic's `tickets.toml`, from the repository root.
    pub tickets: String,
    /// The epic's number: a story is named `<epic>.<id>`.
    pub epic: u32,
    /// The golden story's own entry: a restoring story comes after it in
    /// the table's order (the build order) and is not built (review R4).
    pub own: u32,
}

/// A step-level key the copy leaves out, and the stories that restore it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovedKey {
    pub step: String,
    pub key: String,
    #[serde(default)]
    pub until: Vec<String>,
}

/// A whole step the copy leaves out, and the stories that restore it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovedStep {
    pub step: String,
    #[serde(default)]
    pub until: Vec<String>,
    #[serde(default)]
    pub why: String,
}

/// One seat's script (`scripts/<seat>.json`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub turns: Vec<Turn>,
}

/// One scripted turn: the step it is for (its snapshot line goes under
/// that step), its text, its usage and its tool calls.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub step: String,
    pub text: String,
    pub usage: [u64; 2],
    #[serde(default)]
    pub tool_calls: Vec<ScriptedCall>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptedCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Everything the golden run reads from `fixtures/`.
#[derive(Debug, Clone)]
pub struct Golden {
    /// The sample's folder under `templates/samples/`.
    pub sample: String,
    /// The real flow's id (`report_flow.workflow` in `sample.json`).
    pub workflow: String,
    pub list: List,
    /// By seat id (the file stem).
    pub scripts: BTreeMap<String, Script>,
    /// The cut-down copy's bytes.
    pub copy: String,
    /// The expected snapshot.
    pub expected: String,
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The golden fixtures, each refusing a key it does not know.
pub fn golden() -> Golden {
    let sample: serde_json::Value =
        serde_json::from_str(&read(&am_testkit::fixture("templates/sample.json"))).unwrap();
    let name = sample["sample"].as_str().expect("sample.json: `sample`");
    let workflow = sample["report_flow"]["workflow"]
        .as_str()
        .expect("sample.json: `report_flow.workflow`");
    let list: List = serde_json::from_str(&read(&home().join("golden.json")))
        .unwrap_or_else(|e| panic!("fixtures/golden/so-far/golden.json: {e}"));
    let mut scripts = BTreeMap::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(home().join("scripts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let seat = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a script named after its seat")
            .to_string();
        assert_eq!(
            path.extension().and_then(|e| e.to_str()),
            Some("json"),
            "{} is not a seat's JSON script",
            path.display()
        );
        let script: Script = serde_json::from_str(&read(&path))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        scripts.insert(seat, script);
    }
    Golden {
        sample: name.to_string(),
        workflow: workflow.to_string(),
        list,
        scripts,
        copy: read(&home().join("workflow.yaml")),
        expected: read(&home().join("snapshot.txt")),
    }
}

impl Golden {
    /// The sample's template folder (read only: every open is of a copy).
    pub fn sample_dir(&self) -> PathBuf {
        am_testkit::repo_root()
            .join("templates/samples")
            .join(&self.sample)
    }

    /// The flow's file, relative to a project.
    pub fn flow_file(&self) -> String {
        format!("workflows/{}.yaml", self.workflow)
    }

    /// The real flow's bytes, as the sample holds them.
    pub fn real(&self) -> String {
        read(&self.sample_dir().join(self.flow_file()))
    }

    /// The epic's stories (review R4): their build order from
    /// `tickets.toml`, and which are built, from the plans beside it.
    pub fn story_book(&self) -> StoryBook {
        let tickets = am_testkit::repo_root().join(&self.list.stories.tickets);
        StoryBook {
            epic: self.list.stories.epic,
            own: self.list.stories.own,
            order: story_order(&read(&tickets)),
            built: built_stories(tickets.parent().expect("the tickets' folder")),
        }
    }
}

/// The epic's stories as the restoring-story check reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryBook {
    pub epic: u32,
    /// The golden story's own entry.
    pub own: u32,
    /// Every entry's id, in the table's order (the build order).
    pub order: Vec<u32>,
    /// The entries whose plan says `status: built` or `status: done`.
    pub built: BTreeSet<u32>,
}

impl StoryBook {
    /// The stories that may restore an entry: after the golden story in
    /// the build order, and not built.
    pub fn restoring(&self) -> Vec<u32> {
        let own = self.order.iter().position(|&id| id == self.own);
        self.order
            .iter()
            .enumerate()
            .filter(|(i, id)| own.is_some_and(|o| *i > o) && !self.built.contains(id))
            .map(|(_, &id)| id)
            .collect()
    }
}

/// The `id = <n>` of every `[[entry]]` of a `tickets.toml`, in table order.
pub fn story_order(tickets: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    let mut in_entry = false;
    for line in tickets.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[[entry]]";
            continue;
        }
        if in_entry
            && let Some(value) = line.strip_prefix("id = ")
            && let Ok(id) = value.trim().parse()
        {
            ids.push(id);
            in_entry = false;
        }
    }
    ids
}

/// The stories whose plan in `dir` (`*-plan.md`, frontmatter `ticket: <id>`)
/// says `status: built` or `status: done`.
pub fn built_stories(dir: &Path) -> BTreeSet<u32> {
    let mut built = BTreeSet::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with("-plan.md") {
            continue;
        }
        let text = read(&path);
        let head: Vec<&str> = text
            .lines()
            .skip(1)
            .take_while(|l| l.trim_end() != "---")
            .collect();
        let field = |key: &str| {
            head.iter()
                .find_map(|l| l.strip_prefix(key))
                .map(|v| v.trim().trim_matches('\'').to_string())
        };
        if let (Some(ticket), Some(status)) = (field("ticket:"), field("status:"))
            && matches!(status.as_str(), "built" | "done")
            && let Ok(id) = ticket.parse()
        {
            built.insert(id);
        }
    }
    built
}

// ------------------------------------------------------------------ config

/// The snapshot of a temporary copy of the sample, loaded through
/// am-config in template mode, with `flow` (when given) written over the
/// flow's file first.
pub fn load(g: &Golden, flow: Option<&str>) -> ConfigSnapshot {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(&g.sample);
    support::copy_files(&g.sample_dir(), &root);
    if let Some(text) = flow {
        std::fs::write(root.join(g.flow_file()), text).unwrap();
    }
    let loaded = am_config::load_in(&root, LoadOptions::template(&root)).unwrap();
    loaded
        .snapshot
        .unwrap_or_else(|| panic!("the sample does not load: {:#?}", loaded.findings))
}

/// The flow `g.workflow` of `snapshot`.
pub fn flow_of(g: &Golden, snapshot: &ConfigSnapshot) -> Workflow {
    snapshot
        .workflows()
        .get(&g.workflow)
        .cloned()
        .unwrap_or_else(|| panic!("the sample has no flow `{}`", g.workflow))
}

/// The real flow's seats (each step's `seat:` and a fan-out's `seats:`).
pub fn flow_seats(flow: &Workflow) -> BTreeSet<String> {
    let mut seats = BTreeSet::new();
    for step in &flow.steps {
        seats.extend(step.seat.clone());
        seats.extend(step.seats.iter().cloned());
    }
    seats
}

/// The flow's seats that have a model, and their primary models.
pub fn seats_with_models(
    snapshot: &ConfigSnapshot,
    flow: &Workflow,
) -> BTreeMap<String, (String, String)> {
    flow_seats(flow)
        .into_iter()
        .filter_map(|id| {
            let model = snapshot.seats().get(&id)?.model.clone()?;
            Some((id, (model.account, model.model)))
        })
        .collect()
}

// ------------------------------------------------------------------ providers

/// One served turn, as the router recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub seat: String,
    /// 1-based.
    pub turn: usize,
    /// The turn's `step` tag in its script.
    pub step: String,
    /// The names of the tools the request offered.
    pub offered: Vec<String>,
    /// Review R5: one line per part of the request the model was sent
    /// ([`request_lines`]), so a wrong or empty hand-on shows.
    pub request: Vec<String>,
}

struct SeatScript {
    seat: String,
    instructions: String,
    turns: Vec<Turn>,
    llm: FakeLlm,
    next: Mutex<usize>,
}

/// The seat router (rows 16, 17): it serves each request from the script
/// of the one seat whose instructions are a system part of it, and refuses
/// in words naming the seats when none or several match, or when the
/// seat's script is exhausted.
pub struct Router {
    seats: Vec<SeatScript>,
    served: Mutex<Vec<Served>>,
    /// The `EmptyHandOn` plant: record every trusted part's text as empty,
    /// as if the engine had handed the step nothing.
    blank_hand_ons: bool,
}

impl Router {
    pub fn new(snapshot: &ConfigSnapshot, scripts: &BTreeMap<String, Script>) -> Arc<Router> {
        Router::recording(snapshot, scripts, false)
    }

    /// [`Router::new`], recording every hand-on as empty when
    /// `blank_hand_ons` (a plant: the requests themselves are unchanged).
    pub fn recording(
        snapshot: &ConfigSnapshot,
        scripts: &BTreeMap<String, Script>,
        blank_hand_ons: bool,
    ) -> Arc<Router> {
        let seats = scripts
            .iter()
            .map(|(seat, script)| {
                let instructions = snapshot
                    .seats()
                    .get(seat)
                    .unwrap_or_else(|| {
                        panic!("a script names the seat `{seat}`, which the sample does not have")
                    })
                    .instructions
                    .clone();
                let turns: Vec<ScriptedTurn> = script.turns.iter().map(scripted).collect();
                SeatScript {
                    seat: seat.clone(),
                    instructions,
                    turns: script.turns.clone(),
                    llm: FakeLlm::new(turns),
                    next: Mutex::new(0),
                }
            })
            .collect();
        Arc::new(Router {
            seats,
            served: Mutex::new(Vec::new()),
            blank_hand_ons,
        })
    }

    pub fn served(&self) -> Vec<Served> {
        self.served.lock().unwrap().clone()
    }

    /// Per seat: (turns served, turns scripted).
    pub fn counts(&self) -> BTreeMap<String, (usize, usize)> {
        self.seats
            .iter()
            .map(|s| (s.seat.clone(), (*s.next.lock().unwrap(), s.turns.len())))
            .collect()
    }
}

/// One line per part of `request` (review R5): each system part, then each
/// message's parts, with the part's kind, its label, its length and its
/// fingerprint ([`fingerprint`] of its text with ids and times normalised,
/// so a row id in a result does not move it). Every kind of part is
/// matched by name, so a new kind fails to compile here until it is shown.
pub fn request_lines(request: &ChatRequest, blank_hand_ons: bool) -> Vec<String> {
    let line = |at: String, kind: &str, label: Option<&str>, text: &str| {
        let label = label.map(|l| format!(" `{l}`")).unwrap_or_default();
        format!(
            "{at} {kind}{label} bytes {} sha256 {}",
            text.len(),
            fingerprint(&normalise(text, &[]))
        )
    };
    let mut out: Vec<String> = request
        .system
        .iter()
        .enumerate()
        .map(|(i, part)| {
            line(
                format!("system {}", i + 1),
                "instruction",
                None,
                part.text(),
            )
        })
        .collect();
    for (m, message) in request.messages.iter().enumerate() {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        for (p, part) in message.parts.iter().enumerate() {
            let at = format!("{role} {}.{}", m + 1, p + 1);
            out.push(match part {
                Part::Instruction(i) => line(at, "instruction", None, i.text()),
                Part::Trusted(t) => {
                    let text = if blank_hand_ons { "" } else { t.text() };
                    line(at, "trusted", Some(t.label()), text)
                }
                Part::Untrusted(d) => line(at, "untrusted", Some(d.label()), &untrusted_text(d)),
                Part::AssistantText(text) => line(at, "assistant", None, text),
                Part::ToolCall(c) => line(at, "tool_call", Some(&c.name), &c.arguments_json),
                Part::ToolResult { call_id, content } => {
                    line(at, "tool_result", Some(call_id), &untrusted_text(content))
                }
                Part::EngineResult { call_id, result } => {
                    line(at, "engine_result", Some(call_id), result.text())
                }
            });
        }
    }
    out
}

/// An untrusted part's text, or a file part's name.
fn untrusted_text(data: &am_domain::UntrustedData) -> String {
    match data.content() {
        am_domain::UntrustedContent::Text(t) => t.clone(),
        am_domain::UntrustedContent::File(f) => f.name.clone(),
    }
}

fn scripted(turn: &Turn) -> ScriptedTurn {
    let mut out = ScriptedTurn::text(turn.text.clone()).usage(turn.usage[0], turn.usage[1]);
    for call in &turn.tool_calls {
        out = out.tool_call(&call.name, &call.arguments.to_string());
    }
    out
}

fn refuse(words: String) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
    Box::pin(async move { Err(ProviderError::Protocol(words)) })
}

impl LlmProvider for Router {
    fn account_id(&self) -> String {
        "golden-router".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        request: ChatRequest,
        cancel: Arc<dyn CancelSignal>,
        sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let matching: Vec<&SeatScript> = self
            .seats
            .iter()
            .filter(|s| request.system.iter().any(|i| i.text() == s.instructions))
            .collect();
        let seat = match matching.as_slice() {
            [one] => *one,
            [] => {
                return refuse(
                    "the golden router has no scripted seat whose instructions are a system part of this request".into(),
                );
            }
            many => {
                let names: Vec<&str> = many.iter().map(|s| s.seat.as_str()).collect();
                return refuse(format!(
                    "the golden router matched the seats {} to one request; a request is served from one seat's script",
                    names.join(", ")
                ));
            }
        };
        let n = {
            let mut next = seat.next.lock().unwrap();
            if *next >= seat.turns.len() {
                return refuse(format!(
                    "the script of the seat `{}` is exhausted after {} turns",
                    seat.seat,
                    seat.turns.len()
                ));
            }
            *next += 1;
            *next
        };
        self.served.lock().unwrap().push(Served {
            seat: seat.seat.clone(),
            turn: n,
            step: seat.turns[n - 1].step.clone(),
            offered: request.tools.iter().map(|t| t.name.clone()).collect(),
            request: request_lines(&request, self.blank_hand_ons),
        });
        seat.llm.stream(request, cancel, sink)
    }
}

/// The golden provider factory (row 16): the flow's seats' primary models
/// get the router; any other model the tripwire, an OpenAI-compatible
/// provider with no `AuthRef` on an empty `FakeEgress`, which records any
/// request it is asked to send.
pub struct Factory {
    pub router: Arc<Router>,
    pub egress: Arc<FakeEgress>,
    pub primaries: BTreeSet<(String, String)>,
    asked: Mutex<Vec<(String, String)>>,
}

impl Factory {
    pub fn new(router: Arc<Router>, primaries: BTreeSet<(String, String)>) -> Arc<Factory> {
        Arc::new(Factory {
            router,
            egress: FakeEgress::new(),
            primaries,
            asked: Mutex::new(Vec::new()),
        })
    }

    /// Every model the factory was asked for, in order.
    pub fn asked(&self) -> Vec<(String, String)> {
        self.asked.lock().unwrap().clone()
    }

    /// The tripwire (also used by the test that proves it records).
    pub fn tripwire(&self) -> Arc<dyn LlmProvider> {
        Arc::new(OpenAiCompatProvider::new(
            self.egress.clone(),
            TRIPWIRE,
            TRIPWIRE_URL,
        ))
    }

    /// The factory as `RunContext.providers`.
    pub fn providers(self: &Arc<Self>) -> Providers {
        let factory = self.clone();
        Arc::new(move |model: &ModelRef| {
            let key = (model.account.clone(), model.model.clone());
            factory.asked.lock().unwrap().push(key.clone());
            if factory.primaries.contains(&key) {
                Ok(factory.router.clone() as Arc<dyn LlmProvider>)
            } else {
                Ok(factory.tripwire())
            }
        })
    }
}

// ------------------------------------------------------------------ the commands

/// `agentic validate <args…>`.
pub fn validate(args: &[String]) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = am_app::cli::validate(args, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// `agentic run <workflow> --project <root>` on an interactive terminal,
/// with `providers` and a FakeClock at `clock`. The context is built field
/// by field with no `..`: a port added to `RunContext` (a secret store,
/// register DR-173) fails to compile here until the golden run fakes it.
pub async fn run_flow(
    cli: &Cli,
    root: &Path,
    workflow: &str,
    providers: Providers,
    clock: &str,
) -> Ran {
    let argv = vec![
        workflow.to_string(),
        "--project".to_string(),
        root.to_string_lossy().into_owned(),
    ];
    let ctx = RunContext {
        places: cli.places.clone(),
        identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
        terminal: Box::new(FakeTerminal::new()),
        providers,
        clock: FakeClock::at(clock),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = am_app::cli::run(&argv, ctx, &mut out, &mut err).await;
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

// ------------------------------------------------------------------ the flow file's blocks

/// A step's lines in a flow file: `[start, end)`, 0-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub key: String,
    pub start: usize,
    pub end: usize,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// A flow file's lines, each with its line ending.
pub fn lines_of(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The steps under `steps:`, and the indent of their `- id:` lines. A
/// step's block runs to the next step, or to the first top-level line.
pub fn steps_in(lines: &[&str]) -> Result<(usize, Vec<Block>), String> {
    let at = lines
        .iter()
        .position(|l| l.trim_end() == "steps:" && indent_of(l) == 0)
        .ok_or("the flow file has no top-level `steps:` line")?;
    let mut blocks: Vec<Block> = Vec::new();
    let mut step_indent: Option<usize> = None;
    let mut i = at + 1;
    while i < lines.len() {
        let line = lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        let indent = indent_of(line);
        if indent == 0 {
            break;
        }
        let text = line.trim();
        if text.starts_with("- ") && indent == *step_indent.get_or_insert(indent) {
            let Some(id) = text.strip_prefix("- id: ") else {
                return Err(format!(
                    "line {} starts a step without `- id:`: {:?}",
                    i + 1,
                    line.trim_end()
                ));
            };
            if let Some(last) = blocks.last_mut() {
                last.end = i;
            }
            blocks.push(Block {
                key: id.trim().to_string(),
                start: i,
                end: lines.len(),
            });
        }
        i += 1;
    }
    if let Some(last) = blocks.last_mut() {
        last.end = i;
    }
    Ok((step_indent.unwrap_or(2), blocks))
}

/// The step-level key `key` of `block`: its line and every line indented
/// under it, `[start, end)`.
pub fn key_in(
    lines: &[&str],
    block: &Block,
    step_indent: usize,
    key: &str,
) -> Option<(usize, usize)> {
    let key_indent = step_indent + 2;
    let start = (block.start + 1..block.end).find(|&i| {
        let line = lines[i];
        !blank(line)
            && indent_of(line) == key_indent
            && line
                .trim_start()
                .strip_prefix(key)
                .is_some_and(|rest| rest.starts_with(':'))
    })?;
    let mut end = start + 1;
    while end < block.end && (blank(lines[end]) || indent_of(lines[end]) > key_indent) {
        end += 1;
    }
    Some((start, end))
}

/// Where line `i` of a flow file is: its step (or the header) and its
/// key (a nested line's enclosing step-level key).
pub fn place(lines: &[&str], i: usize) -> String {
    let key_of = |line: &str| {
        line.trim()
            .trim_start_matches("- ")
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string()
    };
    let Ok((step_indent, blocks)) = steps_in(lines) else {
        return format!("line {}", i + 1);
    };
    match blocks.iter().find(|b| (b.start..b.end).contains(&i)) {
        Some(b) => {
            let key = (b.start..=i)
                .rev()
                .find(|&j| j == b.start || indent_of(lines[j]) == step_indent + 2)
                .map(|j| key_of(lines[j]))
                .unwrap_or_default();
            format!("step `{}`, key `{key}`", b.key)
        }
        None => {
            let key = (0..=i.min(lines.len().saturating_sub(1)))
                .rev()
                .find(|&j| !blank(lines[j]) && indent_of(lines[j]) == 0)
                .map(|j| key_of(lines[j]))
                .unwrap_or_default();
            format!("the header, key `{key}`")
        }
    }
}

/// The real flow with the removed list's entries taken out: its text, and
/// the real file's 1-based line of each kept line.
#[derive(Debug, Clone)]
pub struct Cut {
    pub text: String,
    pub lines: Vec<usize>,
}

/// Takes each removed step's block and each removed key's block out of the
/// real flow's text. An entry naming a step or key the real flow does not
/// have is a problem, in words.
pub fn cut(real: &str, list: &List) -> Result<Cut, Vec<String>> {
    let lines = lines_of(real);
    let (step_indent, blocks) = steps_in(&lines).map_err(|e| vec![e])?;
    let mut dropped = vec![false; lines.len()];
    let mut problems = Vec::new();
    for removed in &list.removed_steps {
        match blocks.iter().find(|b| b.key == removed.step) {
            Some(b) => {
                for d in &mut dropped[b.start..b.end] {
                    *d = true;
                }
            }
            None => problems.push(format!(
                "the removed list names the step `{}`, which the real flow does not have",
                removed.step
            )),
        }
    }
    for removed in &list.removed_keys {
        let Some(b) = blocks.iter().find(|b| b.key == removed.step) else {
            problems.push(format!(
                "the removed list names the key `{}` of the step `{}`, which the real flow does not have",
                removed.key, removed.step
            ));
            continue;
        };
        match key_in(&lines, b, step_indent, &removed.key) {
            Some((start, end)) => {
                for d in &mut dropped[start..end] {
                    *d = true;
                }
            }
            None => problems.push(format!(
                "the removed list names the key `{}` of the step `{}`, which that step of the real flow does not declare",
                removed.key, removed.step
            )),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let mut out = Cut {
        text: String::new(),
        lines: Vec::new(),
    };
    for (i, line) in lines.iter().enumerate() {
        if !dropped[i] {
            out.text.push_str(line);
            out.lines.push(i + 1);
        }
    }
    Ok(out)
}

/// The text drift check (row 11): the real flow minus the removed list
/// equals the copy byte for byte. A difference names the first differing
/// line of each file and the step and key that hold it.
pub fn text_drift(real: &str, copy: &str, list: &List) -> Vec<String> {
    let cut = match cut(real, list) {
        Ok(cut) => cut,
        Err(problems) => return problems,
    };
    if cut.text == copy {
        return Vec::new();
    }
    let want = lines_of(&cut.text);
    let got = lines_of(copy);
    let i = (0..=want.len().max(got.len()))
        .find(|&i| want.get(i) != got.get(i))
        .unwrap_or(0);
    let shown = |line: Option<&&str>| {
        line.map(|l| format!("{:?}", l.trim_end_matches('\n')))
            .unwrap_or_else(|| "(the end of the file)".into())
    };
    let real_line = cut
        .lines
        .get(i)
        .map(|n| format!("line {n}"))
        .unwrap_or_else(|| "end".into());
    let real_place = if i < want.len() {
        place(&want, i)
    } else {
        "its end".into()
    };
    let copy_place = if i < got.len() {
        place(&got, i)
    } else {
        "its end".into()
    };
    vec![format!(
        "the copy differs from the real flow minus the removed list: the real flow's {real_line} ({real_place}) reads {}, and the copy's line {} ({copy_place}) reads {}",
        shown(want.get(i)),
        i + 1,
        shown(got.get(i)),
    )]
}

fn differ<T: Debug + PartialEq>(problems: &mut Vec<String>, what: &str, want: &T, got: &T) {
    if want != got {
        problems.push(format!(
            "{what} differs: the real flow minus the removed list has {want:?}, the copy {got:?}"
        ));
    }
}

/// The structure drift check (row 11), on both flows as am-config loads
/// them: every workflow field is equal, the copy's steps are the real
/// flow's minus the removed steps, in order, and each kept step equals the
/// real one with exactly its removed keys gone from `run_graph`. Every
/// field is named (no `..`), so a new field of `Workflow` or `Step` fails
/// to compile here until the check compares it.
pub fn structure_drift(real: &Workflow, copy: &Workflow, list: &List) -> Vec<String> {
    let mut problems = Vec::new();
    let Workflow {
        id,
        name,
        team,
        description,
        triggers,
        caps,
        steps,
    } = real;
    differ(&mut problems, "the workflow field `id`", id, &copy.id);
    differ(&mut problems, "the workflow field `name`", name, &copy.name);
    differ(&mut problems, "the workflow field `team`", team, &copy.team);
    differ(
        &mut problems,
        "the workflow field `description`",
        description,
        &copy.description,
    );
    differ(
        &mut problems,
        "the workflow field `triggers`",
        triggers,
        &copy.triggers,
    );
    differ(&mut problems, "the workflow field `caps`", caps, &copy.caps);
    let gone: BTreeSet<&str> = list.removed_steps.iter().map(|s| s.step.as_str()).collect();
    let kept: Vec<&Step> = steps
        .iter()
        .filter(|s| !gone.contains(s.id.as_str()))
        .collect();
    let want_ids: Vec<&str> = kept.iter().map(|s| s.id.as_str()).collect();
    let got_ids: Vec<&str> = copy.steps.iter().map(|s| s.id.as_str()).collect();
    differ(&mut problems, "the list of steps", &want_ids, &got_ids);
    for want in kept {
        let Some(got) = copy.steps.iter().find(|s| s.id == want.id) else {
            continue;
        };
        let removed: BTreeSet<&str> = list
            .removed_keys
            .iter()
            .filter(|k| k.step == want.id)
            .map(|k| k.key.as_str())
            .collect();
        let mut expected = want.clone();
        expected
            .run_graph
            .retain(|key| !removed.contains(key.as_str()));
        step_fields(&mut problems, &expected, got);
    }
    problems
}

fn step_fields(problems: &mut Vec<String>, want: &Step, got: &Step) {
    let Step {
        id,
        seat,
        instruction,
        description,
        output_schema,
        max_tokens,
        run_graph,
        loop_back,
        gate,
        max_returns,
        limits,
        table,
        fan_out,
        seats,
    } = want;
    let at = |field: &str| format!("step `{id}`, field `{field}`,");
    differ(problems, &at("seat"), seat, &got.seat);
    differ(problems, &at("instruction"), instruction, &got.instruction);
    differ(problems, &at("description"), description, &got.description);
    differ(
        problems,
        &at("output_schema"),
        output_schema,
        &got.output_schema,
    );
    differ(problems, &at("max_tokens"), max_tokens, &got.max_tokens);
    differ(problems, &at("run_graph"), run_graph, &got.run_graph);
    differ(problems, &at("loop_back"), loop_back, &got.loop_back);
    differ(problems, &at("gate"), gate, &got.gate);
    differ(problems, &at("max_returns"), max_returns, &got.max_returns);
    differ(problems, &at("limits"), limits, &got.limits);
    differ(problems, &at("table"), table, &got.table);
    differ(problems, &at("fan_out"), fan_out, &got.fan_out);
    differ(problems, &at("seats"), seats, &got.seats);
}

/// The removed list's own rules (owner Q3): every entry names a step, or a
/// step-level key, that the real flow has; no entry is listed twice or
/// names a key of a removed step; and every entry names at least one
/// restoring story, each `<epic>.<id>` with an id the epic's `tickets.toml`
/// holds.
pub fn list_problems(list: &List, real: &str, book: &StoryBook) -> Vec<String> {
    let mut problems = cut(real, list).err().unwrap_or_default();
    let gone: BTreeSet<&str> = list.removed_steps.iter().map(|s| s.step.as_str()).collect();
    let mut seen = BTreeSet::new();
    for k in &list.removed_keys {
        let what = format!("the removed key `{}.{}`", k.step, k.key);
        if !seen.insert(what.clone()) {
            problems.push(format!("{what} is listed twice"));
        }
        if gone.contains(k.step.as_str()) {
            problems.push(format!(
                "{what} is of a step the list removes whole; list the step alone"
            ));
        }
        problems.extend(story_problems(&what, &k.until, book));
    }
    for s in &list.removed_steps {
        let what = format!("the removed step `{}`", s.step);
        if !seen.insert(what.clone()) {
            problems.push(format!("{what} is listed twice"));
        }
        problems.extend(story_problems(&what, &s.until, book));
    }
    problems
}

/// Each restoring story of an entry (review R4) is a story of the epic,
/// after the golden story in the build order, and not built.
pub fn story_problems(what: &str, until: &[String], book: &StoryBook) -> Vec<String> {
    let epic = book.epic;
    if until.is_empty() {
        return vec![format!(
            "{what} names no story that restores it; tag it with the story (`{epic}.<id>`) that makes it runnable"
        )];
    }
    let place = |id: u32| book.order.iter().position(|&x| x == id);
    until
        .iter()
        .filter_map(|story| {
            let named = story
                .split_once('.')
                .and_then(|(e, id)| Some((e.parse::<u32>().ok()?, id.parse::<u32>().ok()?)))
                .filter(|&(e, id)| e == epic && place(id).is_some());
            let Some((_, id)) = named else {
                return Some(format!(
                    "{what} names the story `{story}`, which is not a story of epic {epic}'s tickets.toml"
                ));
            };
            if place(id) <= place(book.own) {
                return Some(format!(
                    "{what} names the story `{story}`, which does not come after story {epic}.{} in the epic's build order, so it cannot restore what the golden run leaves out",
                    book.own
                ));
            }
            if book.built.contains(&id) {
                return Some(format!(
                    "{what} names the story `{story}`, which is built already: the entry is stale or tagged with the wrong story"
                ));
            }
            None
        })
        .collect()
}

/// One entry of the removed list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Key(RemovedKey),
    Step(RemovedStep),
}

impl Entry {
    /// `<step>.<key>`, or `<step>` for a whole step.
    pub fn name(&self) -> String {
        match self {
            Entry::Key(k) => format!("{}.{}", k.step, k.key),
            Entry::Step(s) => s.step.clone(),
        }
    }

    pub fn step(&self) -> &str {
        match self {
            Entry::Key(k) => &k.step,
            Entry::Step(s) => &s.step,
        }
    }
}

/// Every entry of `list`: its keys, then its whole steps.
pub fn entries(list: &List) -> Vec<Entry> {
    let mut out: Vec<Entry> = list.removed_keys.iter().cloned().map(Entry::Key).collect();
    out.extend(list.removed_steps.iter().cloned().map(Entry::Step));
    out
}

/// `list` without `entry`.
pub fn without(list: &List, entry: &Entry) -> List {
    let mut out = list.clone();
    match entry {
        Entry::Key(k) => out.removed_keys.retain(|x| x != k),
        Entry::Step(s) => out.removed_steps.retain(|x| x != s),
    }
    out
}

/// Review R1 (the list only shrinks): the copy with only `entry` put back
/// (the real flow cut by `list` without it), run in a fresh copy of the
/// sample, is refused by `agentic run` at the entry's step (and, for a key,
/// naming that key), before any run row and with no model call. Otherwise
/// the entry is stale: a story made it runnable, and the error names it.
pub async fn put_back(g: &Golden, list: &List, entry: &Entry) -> Result<(), String> {
    let name = entry.name();
    let text = cut(&g.real(), &without(list, entry)).map_err(|p| {
        format!(
            "the removed entry `{name}` cannot be put back: {}",
            p.join("; ")
        )
    })?;
    let snapshot = load(g, None);
    let real = flow_of(g, &snapshot);
    let primaries = seats_with_models(&snapshot, &real).into_values().collect();
    let router = Router::new(&snapshot, &g.scripts);
    let factory = Factory::new(router.clone(), primaries);
    let cli = support::cli();
    let root = cli.copy_from(&g.sample_dir());
    std::fs::write(root.join(g.flow_file()), &text.text).unwrap();
    let ran = run_flow(&cli, &root, &g.workflow, factory.providers(), &g.list.clock).await;
    let (step, keys) = refused_at(&ran.err);
    let runs = runs_in(&cli, &root);
    let calls = router.served().len() + factory.egress.requests().len();
    let (for_key, wanted) = match entry {
        Entry::Key(k) => (keys.contains(&k.key), format!(" naming `{}`", k.key)),
        Entry::Step(_) => (true, String::new()),
    };
    if ran.code == 1 && runs == 0 && calls == 0 && step.as_deref() == Some(entry.step()) && for_key
    {
        return Ok(());
    }
    Err(format!(
        "the removed entry `{name}` is stale: put back alone into the copy, `agentic run` must refuse at step `{}`{wanted}, before any run row and with no model call; it exited {} with {runs} run rows and {calls} model calls, refused at `{}` naming {keys:?}: {}. A story made it runnable, or it is tagged wrong: take it out of golden.json, put it back in workflow.yaml and move both expected results",
        entry.step(),
        ran.code,
        step.as_deref().unwrap_or("-"),
        ran.err.trim()
    ))
}

/// Why a plant cannot be made on the fixtures as they are (review R2), so
/// its test is skipped and says so; `None` when it can.
pub fn plant_skip(g: &Golden, plant: Plant) -> Option<String> {
    let real = flow_of(g, &load(g, None));
    let copy = flow_of(g, &load(g, Some(&g.copy)));
    match plant {
        Plant::None | Plant::CopyFirstStep => None,
        Plant::ScriptText => {
            let seat = copy.steps.first()?.seat.clone()?;
            let turns = g.scripts.get(&seat).map_or(0, |s| s.turns.len());
            (turns == 0)
                .then(|| format!("the seat `{seat}` of the copy's first step has no scripted turn"))
        }
        Plant::RealFirstStep => real.steps[0].run_graph.is_empty().then(|| {
            format!(
                "the real flow's first step `{}` declares no run-graph key to take out",
                real.steps[0].id
            )
        }),
        Plant::EmptyHandOn => (copy.steps.len() < 2)
            .then(|| "the copy has one step, so no step receives a hand-on".to_string()),
    }
}

/// What the planted drift cases found: the cases the checks missed, and
/// the cases the list's shape does not allow, each in words.
#[derive(Debug, Clone, Default)]
pub struct Planted {
    pub missed: Vec<String>,
    pub skipped: Vec<String>,
}

/// Review R2: the drift check's planted cases on `list` and its copy, each
/// aimed at a target picked from the list's shape and the copy (no index
/// fixed in advance). A case the shape does not allow is skipped, in
/// words; a case the checks do not catch, naming the step and the key or
/// field, is missed.
pub fn planted_drift_cases(g: &Golden, list: &List, copy_text: &str, book: &StoryBook) -> Planted {
    let mut out = Planted::default();
    let real_text = g.real();
    let snapshot = load(g, None);
    let real = flow_of(g, &snapshot);
    let copy = flow_of(g, &load(g, Some(copy_text)));
    let base: Vec<String> = text_drift(&real_text, copy_text, list)
        .into_iter()
        .chain(structure_drift(&real, &copy, list))
        .chain(list_problems(list, &real_text, book))
        .collect();
    if !base.is_empty() {
        out.missed
            .push(format!("the unplanted copy and list drift: {base:?}"));
        return out;
    }
    let mut expect = |problems: Vec<String>, words: &[String], case: &str| {
        let all = problems.join("\n");
        if problems.is_empty() {
            out.missed.push(format!("{case}: no problem found"));
        } else if let Some(word) = words.iter().find(|w| !all.contains(w.as_str())) {
            out.missed
                .push(format!("{case}: `{word}` not named in: {all}"));
        }
    };
    let mut skipped = Vec::new();
    let first_key = list.removed_keys.first();
    let first_step = list.removed_steps.first();

    // An extra byte in a kept step of the copy (its last).
    match copy.steps.last() {
        Some(last) => expect(
            text_drift(
                &real_text,
                &edit_line(copy_text, Some(last.id.as_str()), "instruction", |l| {
                    format!("{l}x")
                }),
                list,
            ),
            &[format!("step `{}`", last.id), "key `instruction`".into()],
            "an extra byte in a kept step",
        ),
        None => skipped.push("an extra byte: the copy keeps no step".to_string()),
    }

    // A changed `seat:` of a kept step, as text and as loaded.
    let own = copy
        .steps
        .iter()
        .find_map(|s| Some((s.id.clone(), s.seat.clone()?)));
    let other = own.as_ref().and_then(|(_, seat)| {
        seats_with_models(&snapshot, &real)
            .into_keys()
            .find(|s| s != seat)
    });
    match (own, other) {
        (Some((step, _)), Some(other)) => {
            let planted = edit_line(copy_text, Some(step.as_str()), "seat", |l| {
                format!("{}: {other}", l.split_once(": ").unwrap().0)
            });
            expect(
                text_drift(&real_text, &planted, list),
                &[format!("step `{step}`"), "key `seat`".into()],
                "a changed seat, as text",
            );
            expect(
                structure_drift(&real, &flow_of(g, &load(g, Some(&planted))), list),
                &[format!("step `{step}`"), "field `seat`".into()],
                "a changed seat, as loaded",
            );
        }
        _ => {
            skipped.push("a changed seat: no kept step has a seat with another seat to take".into())
        }
    }

    // A key the copy leaves out that the list does not name.
    match first_key {
        Some(k) => {
            let short = without(list, &Entry::Key(k.clone()));
            expect(
                text_drift(&real_text, copy_text, &short),
                &[format!("step `{}`", k.step), format!("key `{}`", k.key)],
                "a key left out that the list does not name, as text",
            );
            expect(
                structure_drift(&real, &copy, &short),
                &[format!("step `{}`", k.step), "field `run_graph`".into()],
                "a key left out that the list does not name, as loaded",
            );
            // A listed key that the copy still holds.
            let holding = cut(&real_text, &short).unwrap().text;
            expect(
                text_drift(&real_text, &holding, list),
                &[format!("step `{}`", k.step), format!("key `{}`", k.key)],
                "a listed key the copy still holds",
            );
        }
        None => skipped.push(
            "a key left out unnamed, and a listed key still held: the list names no key".into(),
        ),
    }

    // A whole step the copy leaves out that the list does not name.
    match first_step {
        Some(s) => {
            let short = without(list, &Entry::Step(s.clone()));
            expect(
                text_drift(&real_text, copy_text, &short),
                &[format!("step `{}`", s.step)],
                "a step left out that the list does not name, as text",
            );
            expect(
                structure_drift(&real, &copy, &short),
                &["the list of steps".into()],
                "a step left out that the list does not name, as loaded",
            );
        }
        None => skipped.push("a step left out unnamed: the list names no whole step".into()),
    }

    // A change to the real flow (in memory) that the copy does not mirror.
    let real_first = real.steps[0].id.clone();
    expect(
        text_drift(
            &edit_line(&real_text, None, "instruction", |l| format!("{l} Now.")),
            copy_text,
            list,
        ),
        &[format!("step `{real_first}`"), "key `instruction`".into()],
        "a change to the real flow the copy does not mirror",
    );

    // Entries with no restoring story, with a story the epic lacks, and
    // with a story that comes before this one (R4).
    let entries = entries(list);
    match entries.first() {
        Some(entry) => {
            let retag = |until: Vec<String>| {
                let mut planted = list.clone();
                match entry {
                    Entry::Key(e) => planted
                        .removed_keys
                        .iter_mut()
                        .filter(|k| **k == *e)
                        .for_each(|k| k.until = until.clone()),
                    Entry::Step(e) => planted
                        .removed_steps
                        .iter_mut()
                        .filter(|s| **s == *e)
                        .for_each(|s| s.until = until.clone()),
                }
                list_problems(&planted, &real_text, book)
            };
            let name = format!("`{}`", entry.name());
            expect(
                retag(Vec::new()),
                &[format!("{name} names no story")],
                "an entry with no restoring story",
            );
            let absent = book.order.iter().max().copied().unwrap_or(0) + 1000;
            expect(
                retag(vec![format!("{}.{absent}", book.epic)]),
                &[
                    format!("`{}.{absent}`", book.epic),
                    "is not a story of epic".into(),
                ],
                "an entry naming a story the epic lacks",
            );
            expect(
                retag(vec![format!("{}.{}", book.epic + 1, book.own)]),
                &["is not a story of epic".into()],
                "an entry naming another epic's story",
            );
            match book.order.first().filter(|&&id| id != book.own) {
                Some(early) => expect(
                    retag(vec![format!("{}.{early}", book.epic)]),
                    &[
                        format!("`{}.{early}`", book.epic),
                        "does not come after".into(),
                    ],
                    "an entry naming a story before this one",
                ),
                None => {
                    skipped.push("a story before this one: this story is the epic's first".into())
                }
            }
        }
        None => skipped.push("the restoring-story cases: the list is empty".into()),
    }

    // Entries naming a key or a step the real flow does not have.
    match book.restoring().first() {
        Some(&story) => {
            let until = vec![format!("{}.{story}", book.epic)];
            let mut ghost = list.clone();
            ghost.removed_keys.push(RemovedKey {
                step: real_first.clone(),
                key: "no_such_key".into(),
                until: until.clone(),
            });
            ghost.removed_steps.push(RemovedStep {
                step: "no_such_step".into(),
                until,
                why: String::new(),
            });
            expect(
                list_problems(&ghost, &real_text, book),
                &[
                    "`no_such_key`".into(),
                    "`no_such_step`".into(),
                    "does not".into(),
                ],
                "an entry naming a key or step the real flow lacks",
            );
        }
        None => skipped.push("a ghost entry: no story is left to restore one".into()),
    }
    out.skipped = skipped;
    out
}

/// Where the real flow stopped (stage 2): the step and the keys its words
/// name, the exit code, the run rows and how often the factory was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    pub step: Option<String>,
    pub keys: Vec<String>,
    pub code: i32,
    pub runs: usize,
    pub asked: usize,
    pub err: String,
}

/// The step and the keys a run-graph refusal names: "step `<step>` of the
/// workflow `<flow>` declares `<key>`, `<key>`, which this version …".
pub fn refused_at(err: &str) -> (Option<String>, Vec<String>) {
    let Some(rest) = err.lines().find_map(|l| l.strip_prefix("agentic: step `")) else {
        return (None, Vec::new());
    };
    let step = rest.split('`').next().unwrap_or_default().to_string();
    let keys = rest
        .split_once(" declares ")
        .and_then(|(_, after)| after.split_once(", which "))
        .map(|(keys, _)| {
            keys.split(", ")
                .map(|k| k.trim_matches('`').to_string())
                .collect()
        })
        .unwrap_or_default();
    (Some(step), keys)
}

/// Row 12: the removed list tracks what the product refuses. Each removed
/// key is in its real step's `run_graph`; no step before the real flow's
/// stop step has a removed key; the stop step's removed keys are the keys
/// its words name; and the first removed step is the stop step or after it.
pub fn tracks_product(list: &List, real: &Workflow, stopped: &Stopped) -> Vec<String> {
    let mut problems = Vec::new();
    let index = |step: &str| real.steps.iter().position(|s| s.id == step);
    for k in &list.removed_keys {
        match real.steps.iter().find(|s| s.id == k.step) {
            Some(s) if s.run_graph.contains(&k.key) => {}
            Some(s) => problems.push(format!(
                "the removed key `{}.{}` is not one of the step's run-graph keys {:?}: `agentic run` honours it, so the copy must keep it",
                k.step, k.key, s.run_graph
            )),
            None => problems.push(format!(
                "the removed key `{}.{}` names a step the real flow does not have",
                k.step, k.key
            )),
        }
    }
    let Some(step) = &stopped.step else {
        problems.push(format!(
            "the real flow was not refused at a step (exit {}): {}",
            stopped.code, stopped.err
        ));
        return problems;
    };
    let Some(at) = index(step) else {
        problems.push(format!(
            "the real flow was refused at `{step}`, which is not one of its steps"
        ));
        return problems;
    };
    for k in &list.removed_keys {
        if index(&k.step).is_some_and(|i| i < at) {
            problems.push(format!(
                "the removed key `{}.{}` is on a step before `{step}`, where the real flow is refused: a story honoured it, so shrink the list and edit the copy",
                k.step, k.key
            ));
        }
    }
    let mut listed: Vec<String> = list
        .removed_keys
        .iter()
        .filter(|k| &k.step == step)
        .map(|k| k.key.clone())
        .collect();
    listed.sort();
    let mut named = stopped.keys.clone();
    named.sort();
    if listed != named {
        problems.push(format!(
            "the real flow is refused at `{step}` for {named:?}, and the removed list takes {listed:?} out of that step"
        ));
    }
    if let Some(first) = real
        .steps
        .iter()
        .position(|s| list.removed_steps.iter().any(|r| r.step == s.id))
        && first < at
    {
        problems.push(format!(
            "the removed step `{}` comes before `{step}`, where the real flow is refused",
            real.steps[first].id
        ));
    }
    problems
}

// ------------------------------------------------------------------ fixtures only

/// A glob over a path: `*` matches any run of characters but `/`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == path,
        Some((head, tail)) => {
            let Some(rest) = path.strip_prefix(head) else {
                return false;
            };
            (0..=rest.len())
                .take_while(|&i| !rest[..i].contains('/'))
                .any(|i| glob_match(tail, &rest[i..]))
        }
    }
}

/// The folders `just validate` runs `agentic validate --template` over.
pub fn validate_list(justfile: &str) -> Vec<String> {
    let mut in_recipe = false;
    for line in justfile.lines() {
        if line.starts_with("validate:") {
            in_recipe = true;
            continue;
        }
        if in_recipe && !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        if in_recipe && let Some(rest) = line.trim().strip_prefix("for project in ") {
            let words = rest.split(';').next().unwrap_or_default();
            return words.split_whitespace().map(str::to_string).collect();
        }
    }
    Vec::new()
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Row 13: the copy and its list are fixtures only, never a template.
pub fn fixture_only_problems() -> Vec<String> {
    let repo = am_testkit::repo_root();
    let folder = home();
    let mut problems = Vec::new();
    let rel = |p: &Path| p.strip_prefix(&repo).unwrap_or(p).display().to_string();
    for name in ["workflow.yaml", "golden.json"] {
        let path = folder.join(name);
        if !path.is_file() {
            problems.push(format!("{} is missing", rel(&path)));
        }
    }
    if !folder.starts_with(repo.join("fixtures")) {
        problems.push(format!("{} is not under fixtures/", rel(&folder)));
    }
    if folder.join("company.yaml").exists() {
        problems.push(format!(
            "{} holds a company.yaml, so it could be taken for a project or a template",
            rel(&folder)
        ));
    }
    let mut templates = Vec::new();
    files_under(&repo.join("templates"), &mut templates);
    for name in ["workflow.yaml", "golden.json"] {
        let Ok(bytes) = std::fs::read(folder.join(name)) else {
            continue;
        };
        for path in &templates {
            if std::fs::read(path).is_ok_and(|b| b == bytes) {
                problems.push(format!(
                    "{} has the bytes of the fixture {name}: the copy and its list are never a template",
                    rel(path)
                ));
            }
        }
    }
    let target = format!("{}/", rel(&folder));
    let list = validate_list(&read(&repo.join("justfile")));
    if list.is_empty() {
        problems.push("the justfile's `validate` recipe lists no folder".into());
    }
    for glob in list {
        if glob.contains("fixtures/golden") || glob_match(&glob, &target) {
            problems.push(format!(
                "the justfile's `validate` list holds `{glob}`, which reaches {target}"
            ));
        }
    }
    problems
}

/// Row 15: the sample ids (the flow, its steps, the scripted seats, the
/// sample's folder) that the golden sources spell as a whole token,
/// ignoring case. The denylist (`cargo xtask denylist`) checks every other
/// term over all of `crates/`.
pub fn spelled_ids(g: &Golden, real: &Workflow) -> Vec<String> {
    let mut ids: BTreeSet<String> = real.steps.iter().map(|s| s.id.to_lowercase()).collect();
    ids.extend(g.scripts.keys().map(|s| s.to_lowercase()));
    ids.insert(g.workflow.to_lowercase());
    ids.insert(g.sample.to_lowercase());
    let mut files = Vec::new();
    files_under(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        &mut files,
    );
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    let mut found = Vec::new();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
    {
        for (n, line) in read(path).lines().enumerate() {
            let lower = line.to_lowercase();
            for id in &ids {
                let hit = lower.match_indices(id.as_str()).any(|(at, _)| {
                    !word(lower[..at].chars().next_back())
                        && !word(lower[at + id.len()..].chars().next())
                });
                if hit {
                    found.push(format!("{}:{}: `{id}`", path.display(), n + 1));
                }
            }
        }
    }
    found
}

// ------------------------------------------------------------------ a pass

/// A change planted for one pass (rows 8 to 10), in memory or in a
/// temporary copy only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plant {
    None,
    /// The first turn of the seat of the copy's first step gets other text.
    ScriptText,
    /// The installed copy's first step gets a suffix on its instruction.
    CopyFirstStep,
    /// The real flow's first step loses its run-graph keys, in stage 2's
    /// copy of the sample.
    RealFirstStep,
    /// Review R5: the router records every hand-on (a trusted part) as
    /// empty, as if the engine had handed the next step nothing.
    EmptyHandOn,
}

/// One pass: its normalised snapshot and what the tests check besides.
#[derive(Debug, Clone)]
pub struct Pass {
    pub snapshot: String,
    pub stopped: Stopped,
    /// The copy's exit code.
    pub code: i32,
    /// Every model the factories were asked for (both runs).
    pub asked: Vec<(String, String)>,
    /// The flow's seats' primary models.
    pub primaries: BTreeSet<(String, String)>,
    /// The requests the tripwires' egress recorded (both runs).
    pub requests: usize,
    /// Per seat: (turns served, turns scripted), in the copy's run.
    pub counts: BTreeMap<String, (usize, usize)>,
    /// The copy's first step.
    pub first_step: String,
}

/// `text` with the `key:` line of the step `step` (the first step when
/// `None`) replaced by `edit` of it (without its line ending).
pub fn edit_line(
    text: &str,
    step: Option<&str>,
    key: &str,
    edit: impl Fn(&str) -> String,
) -> String {
    let lines = lines_of(text);
    let (step_indent, blocks) = steps_in(&lines).unwrap();
    let block = match step {
        Some(step) => blocks
            .iter()
            .find(|b| b.key == step)
            .unwrap_or_else(|| panic!("no step `{step}`")),
        None => blocks.first().expect("a flow has a step"),
    };
    let (at, _) = key_in(&lines, block, step_indent, key)
        .unwrap_or_else(|| panic!("the step `{}` has no `{key}:`", block.key));
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == at {
            out.push_str(&edit(line.trim_end_matches('\n')));
            out.push('\n');
        } else {
            out.push_str(line);
        }
    }
    out
}

/// `text` with `suffix` appended to the first step's `key:` line.
pub fn suffixed(text: &str, key: &str, suffix: &str) -> String {
    edit_line(text, None, key, |line| format!("{line}{suffix}"))
}

/// `text` without the first step's `keys` blocks.
pub fn without_keys(text: &str, keys: &[String]) -> String {
    let lines = lines_of(text);
    let (step_indent, blocks) = steps_in(&lines).unwrap();
    let first = blocks.first().expect("a flow has a step");
    let mut dropped = vec![false; lines.len()];
    for key in keys {
        let (start, end) = key_in(&lines, first, step_indent, key)
            .unwrap_or_else(|| panic!("the first step has no `{key}:`"));
        for d in &mut dropped[start..end] {
            *d = true;
        }
    }
    lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !dropped[*i])
        .map(|(_, l)| *l)
        .collect()
}

/// The run rows a project's journal holds (0 when it was never opened).
fn runs_in(cli: &Cli, root: &Path) -> usize {
    match cli.layout(root).map(|l| l.state_db()) {
        Some(db) if db.exists() => support::journal(&db).runs.len(),
        _ => 0,
    }
}

/// One full pass with `plant`: validate, the real flow, the copy.
pub async fn pass(g: &Golden, plant: Plant) -> Pass {
    let real_snapshot = load(g, None);
    let real = flow_of(g, &real_snapshot);
    let seats = seats_with_models(&real_snapshot, &real);
    let primaries: BTreeSet<(String, String)> = seats.values().cloned().collect();

    // Stage 1: validate a copy never opened.
    let cli0 = support::cli();
    let root0 = cli0.copy_from(&g.sample_dir());
    let validated = validate(&[
        "--template".to_string(),
        root0.to_string_lossy().into_owned(),
    ]);

    // Stage 2: the real flow, as the sample holds it.
    let cli1 = support::cli();
    let root1 = cli1.copy_from(&g.sample_dir());
    if plant == Plant::RealFirstStep {
        let keys = real.steps[0].run_graph.clone();
        let file = root1.join(g.flow_file());
        std::fs::write(&file, without_keys(&read(&file), &keys)).unwrap();
    }
    let factory1 = Factory::new(Router::new(&real_snapshot, &g.scripts), primaries.clone());
    let stopped_ran = run_flow(
        &cli1,
        &root1,
        &g.workflow,
        factory1.providers(),
        &g.list.clock,
    )
    .await;
    let (step, keys) = refused_at(&stopped_ran.err);
    let stopped = Stopped {
        step,
        keys,
        code: stopped_ran.code,
        runs: runs_in(&cli1, &root1),
        asked: factory1.asked().len(),
        err: stopped_ran.err.clone(),
    };

    // Stage 3: the cut-down copy, over the flow's file before the first open.
    let copy_text = match plant {
        Plant::CopyFirstStep => suffixed(&g.copy, "instruction", PLANTED),
        _ => g.copy.clone(),
    };
    let copy_snapshot = load(g, Some(&copy_text));
    let copy = flow_of(g, &copy_snapshot);
    let first_step = copy.steps[0].id.clone();
    let mut scripts = g.scripts.clone();
    if plant == Plant::ScriptText {
        let seat = copy.steps[0]
            .seat
            .clone()
            .expect("the copy's first step has a seat");
        let script = scripts
            .get_mut(&seat)
            .unwrap_or_else(|| panic!("no script for the seat `{seat}`"));
        script.turns[0].text.push_str(PLANTED);
    }
    let cli2 = support::cli();
    let root2 = cli2.copy_from(&g.sample_dir());
    std::fs::write(root2.join(g.flow_file()), &copy_text).unwrap();
    let router = Router::recording(&copy_snapshot, &scripts, plant == Plant::EmptyHandOn);
    let factory2 = Factory::new(router.clone(), primaries.clone());
    let ran = run_flow(
        &cli2,
        &root2,
        &g.workflow,
        factory2.providers(),
        &g.list.clock,
    )
    .await;
    let journal = match cli2.layout(&root2).map(|l| l.state_db()) {
        Some(db) if db.exists() => Some((support::journal(&db), db)),
        _ => None,
    };
    let tables: Vec<String> = copy_snapshot.tables().keys().cloned().collect();

    let mut lines = Vec::new();
    lines.extend(render_validate(&validated));
    lines.extend(render_stop(&stopped, &stopped_ran));
    lines.extend(render_copy(&g.list, &ran, &factory2.asked()));
    if let Some((journal, db)) = &journal {
        lines.extend(render_journal(journal, db, &tables, &router));
    }
    let raw = lines.join("\n") + "\n";
    let mut asked = factory1.asked();
    asked.extend(factory2.asked());
    Pass {
        snapshot: normalise(&raw, &[root0, root1, root2]),
        stopped,
        code: ran.code,
        asked,
        primaries,
        requests: factory1.egress.requests().len() + factory2.egress.requests().len(),
        counts: router.counts(),
        first_step,
    }
}

// ------------------------------------------------------------------ rendering

fn taint(t: am_domain::Taint) -> u8 {
    u8::from(t.is_tainted())
}

/// The first 12 hex digits of the sha256 of a journaled part's text, so a
/// change that keeps its length still shows (`printf '%s' "<text>" |
/// shasum -a 256 | cut -c1-12`).
pub fn fingerprint(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..12].to_string()
}

fn one_line(text: &str) -> String {
    text.replace('\n', "\\n")
}

fn opt<T: ToString>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "-".into())
}

/// Whether a stdout line is the open's (it names the database version and
/// a new project id): left out of the snapshot.
fn open_line(line: &str) -> bool {
    line.starts_with("agentic: project ")
}

pub fn render_validate(ran: &Ran) -> Vec<String> {
    let mut out = vec![format!("validate: exit {}", ran.code)];
    out.extend(ran.out.lines().map(|l| format!("validate: {l}")));
    out.extend(ran.err.lines().map(|l| format!("validate: err {l}")));
    out
}

pub fn render_stop(stopped: &Stopped, ran: &Ran) -> Vec<String> {
    let mut out = vec![
        format!("stop: step {}", stopped.step.as_deref().unwrap_or("(none)")),
        format!(
            "stop: keys {}",
            if stopped.keys.is_empty() {
                "(none)".to_string()
            } else {
                stopped.keys.join(", ")
            }
        ),
        format!("stop: exit {}", stopped.code),
        format!("stop: runs {}", stopped.runs),
        format!("stop: asked {}", stopped.asked),
    ];
    out.extend(
        ran.out
            .lines()
            .filter(|l| !open_line(l))
            .map(|l| format!("stop: out {l}")),
    );
    out.extend(ran.err.lines().map(|l| format!("stop: err {l}")));
    out
}

pub fn render_copy(list: &List, ran: &Ran, asked: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for k in &list.removed_keys {
        out.push(format!(
            "copy: removed {}.{} until {}",
            k.step,
            k.key,
            k.until.join(", ")
        ));
    }
    for s in &list.removed_steps {
        out.push(format!(
            "copy: removed step {} until {}",
            s.step,
            s.until.join(", ")
        ));
    }
    out.push(format!("copy: exit {}", ran.code));
    let mut counted: BTreeMap<&(String, String), usize> = BTreeMap::new();
    for model in asked {
        *counted.entry(model).or_default() += 1;
    }
    for ((account, model), n) in counted {
        out.push(format!("copy: asked {account} {model} {n}"));
    }
    out.extend(
        ran.out
            .lines()
            .filter(|l| !open_line(l))
            .map(|l| format!("copy: out {l}")),
    );
    out.extend(ran.err.lines().map(|l| format!("copy: err {l}")));
    out
}

/// The copy's journal, in journal order (runs as created, steps by `seq`
/// and attempt, calls by `call_seq`, parts by `call_seq` and `ord`), never
/// by time.
pub fn render_journal(
    j: &support::Journal,
    db: &Path,
    tables: &[String],
    router: &Router,
) -> Vec<String> {
    let mut out = Vec::new();
    let served = router.served();
    for run in j.runs.iter().rev() {
        out.push(format!(
            "run {}: workflow {}, status {}, config {}",
            run.id,
            run.workflow,
            run.status,
            opt(&run.config_hash)
        ));
    }
    let mut history = Vec::new();
    let mut rows = Vec::new();
    for table in tables {
        let table_rows = support::rows_of(db, table);
        for row in &table_rows {
            history.extend(support::history_of(db, table, &row.row_id));
        }
        rows.push((table.clone(), table_rows));
    }
    let keys: BTreeMap<&str, &str> = j
        .steps
        .iter()
        .map(|s| (s.id.as_str(), s.step_key.as_str()))
        .collect();
    let mut steps = j.steps.clone();
    steps.sort_by_key(|s| (s.seq, s.attempt));
    let mut rendered_keys = BTreeSet::new();
    for s in &steps {
        let tag = format!("step {}", s.step_key);
        let mut put = |fact: String| out.push(format!("{tag}: {fact}"));
        put(format!(
            "attempt {} {} {}, taint {}",
            s.attempt,
            s.id,
            s.status,
            taint(s.taint)
        ));
        let inputs = j.inputs_of(&s.id);
        if inputs.is_empty() {
            put("input (none)".into());
        }
        for i in inputs {
            let from = i
                .from_step_run
                .clone()
                .or_else(|| i.from_item.clone())
                .unwrap_or_else(|| "-".into());
            put(format!(
                "input {} from {from} `{}` taint {}",
                i.kind,
                i.label,
                taint(i.taint)
            ));
        }
        for p in j.parts_of(&s.id) {
            put(format!(
                "prompt call {} part {} {} {} `{}` taint {} bytes {} sha256 {}",
                p.call_seq,
                p.ord,
                p.slot.as_str(),
                p.kind.as_str(),
                p.label,
                taint(p.taint),
                p.bytes,
                p.text
                    .as_deref()
                    .map(fingerprint)
                    .unwrap_or_else(|| "-".into())
            ));
        }
        let mut calls = j.calls_of(&s.id);
        calls.sort_by_key(|c| c.call_seq);
        if calls.is_empty() {
            put("model call (none)".into());
        }
        for c in &calls {
            put(format!(
                "model call {} {} {} {}, tokens {}/{}",
                c.call_seq,
                c.id,
                c.purpose.as_str(),
                c.status,
                opt(&c.input_tokens),
                opt(&c.output_tokens)
            ));
            for l in j.ledger.iter().filter(|l| l.model_call_id == c.id) {
                put(format!(
                    "ledger call {} reserved {} settled {} {}",
                    c.call_seq,
                    l.reserved_micro_usd,
                    opt(&l.settled_micro_usd),
                    l.state
                ));
            }
        }
        let tools = j.tool_calls_of(&s.id);
        if tools.is_empty() {
            put("tool call (none)".into());
        }
        for t in tools {
            put(format!(
                "tool call {} {} {} {} {}",
                t.call_seq,
                t.tool,
                t.status,
                opt(&t.result_kind),
                opt(&t.result_label)
            ));
        }
        put(match &s.output {
            Some(text) => format!("output {}", one_line(text)),
            None => "output (none)".into(),
        });
        let items: Vec<_> = j.items.iter().filter(|i| i.step_run_id == s.id).collect();
        if items.is_empty() {
            put("item (none)".into());
        }
        for i in items {
            put(format!(
                "item {} {} {} taint {}: {}",
                i.kind.as_str(),
                i.id,
                i.status,
                taint(i.taint),
                one_line(&i.title)
            ));
        }
        let written: Vec<_> = history
            .iter()
            .filter(|h| h.step_run_id.as_deref() == Some(s.id.as_str()))
            .collect();
        if written.is_empty() {
            put("row (none)".into());
        }
        for h in written {
            put(format!(
                "row {} {} v{} {} taint {} by seat {}",
                h.table_id,
                h.row_id,
                h.version,
                h.op,
                taint(h.taint),
                opt(&h.seat)
            ));
        }
        if rendered_keys.insert(s.step_key.clone()) {
            for t in served.iter().filter(|t| t.step == s.step_key) {
                put(format!(
                    "script {} turn {} served, offered [{}]",
                    t.seat,
                    t.turn,
                    t.offered.join(", ")
                ));
                // Review R5: what the model was sent, part by part.
                for line in &t.request {
                    put(format!("request {} turn {} {line}", t.seat, t.turn));
                }
            }
        }
    }
    for (table, table_rows) in rows {
        if table_rows.is_empty() {
            out.push(format!("rows {table}: (none)"));
        }
        for r in table_rows {
            out.push(format!(
                "rows {table}: {} v{} taint {} {}",
                r.row_id,
                r.version,
                taint(r.taint),
                one_line(&r.data)
            ));
        }
    }
    if j.items.is_empty() {
        out.push("inbox: (none)".into());
    }
    for i in &j.items {
        out.push(format!(
            "inbox: {} {} {} step {} taint {}: {}",
            i.kind.as_str(),
            i.id,
            i.status,
            keys.get(i.step_run_id.as_str()).unwrap_or(&"-"),
            taint(i.taint),
            one_line(&i.title)
        ));
    }
    for t in served.iter().filter(|t| !rendered_keys.contains(&t.step)) {
        out.push(format!(
            "script {}: turn {} served for `{}`, which is no step of the run",
            t.seat, t.turn, t.step
        ));
    }
    for (seat, (served, scripted)) in router.counts() {
        out.push(format!("script {seat}: served {served} of {scripted}"));
    }
    out
}

// ------------------------------------------------------------------ normalising

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn boundary(bytes: &[u8], at: usize) -> bool {
    bytes.get(at).is_none_or(|b| !b.is_ascii_alphanumeric())
}

/// Whether a UUID (8-4-4-4-12 hex) starts at `i`.
fn uuid_at(bytes: &[u8], i: usize) -> bool {
    let groups = [8, 4, 4, 4, 12];
    let mut at = i;
    for (n, len) in groups.iter().enumerate() {
        if !(at..at + len).all(|k| bytes.get(k).copied().is_some_and(is_hex)) {
            return false;
        }
        at += len;
        if n < groups.len() - 1 {
            if bytes.get(at) != Some(&b'-') {
                return false;
            }
            at += 1;
        }
    }
    boundary(bytes, at)
}

fn digits(bytes: &[u8], at: usize, n: usize) -> bool {
    (at..at + n).all(|k| bytes.get(k).is_some_and(u8::is_ascii_digit))
}

/// The length of an RFC 3339 instant starting at `i`, if one does.
fn instant_at(bytes: &[u8], i: usize) -> Option<usize> {
    let fixed = digits(bytes, i, 4)
        && bytes.get(i + 4) == Some(&b'-')
        && digits(bytes, i + 5, 2)
        && bytes.get(i + 7) == Some(&b'-')
        && digits(bytes, i + 8, 2)
        && bytes.get(i + 10) == Some(&b'T')
        && digits(bytes, i + 11, 2)
        && bytes.get(i + 13) == Some(&b':')
        && digits(bytes, i + 14, 2)
        && bytes.get(i + 16) == Some(&b':')
        && digits(bytes, i + 17, 2);
    if !fixed {
        return None;
    }
    let mut at = i + 19;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    match bytes.get(at) {
        Some(b'Z') => Some(at + 1 - i),
        Some(b'+' | b'-')
            if digits(bytes, at + 1, 2)
                && bytes.get(at + 3) == Some(&b':')
                && digits(bytes, at + 4, 2) =>
        {
            Some(at + 6 - i)
        }
        _ => None,
    }
}

/// The snapshot's normal form (row 5): the temporary copies' roots become
/// `<sample>`; UUIDs `<id-N>`, numbered by first appearance; RFC 3339
/// instants `<time>`; a 64-hex config hash and validate's 12-hex snapshot
/// prefix `<config>`.
pub fn normalise(text: &str, roots: &[PathBuf]) -> String {
    let mut text = text.to_string();
    let mut forms: Vec<String> = Vec::new();
    for root in roots {
        forms.push(root.to_string_lossy().into_owned());
        if let Ok(canonical) = root.canonicalize() {
            forms.push(canonical.to_string_lossy().into_owned());
        }
    }
    forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
    for form in forms {
        text = text.replace(&form, "<sample>");
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if start && uuid_at(bytes, i) {
            let id = text[i..i + 36].to_ascii_lowercase();
            let n = ids.len() + 1;
            let n = *ids.entry(id).or_insert(n);
            out.push_str(&format!("<id-{n}>"));
            i += 36;
            continue;
        }
        if start && let Some(len) = instant_at(bytes, i) {
            out.push_str("<time>");
            i += len;
            continue;
        }
        if start {
            let run = bytes[i..].iter().take_while(|b| is_hex(**b)).count();
            let snapshot_prefix = run == 12 && text[..i].ends_with("(snapshot ");
            if (run == 64 || snapshot_prefix) && boundary(bytes, i + run) {
                out.push_str("<config>");
                i += run;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

// ------------------------------------------------------------------ comparing

/// A snapshot that differs: the differing tags (`step <key>`, `stop`, …)
/// and the diff, in words.
#[derive(Debug, Clone)]
pub struct Mismatch {
    pub tags: Vec<String>,
    pub text: String,
}

/// Each compared line with its tag (the text before its first `: `);
/// comments (`#`) and blank lines are not compared.
pub fn tagged(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let tag = l.split_once(": ").map(|(t, _)| t).unwrap_or(l);
            (tag.to_string(), l.to_string())
        })
        .collect()
}

/// The lines only in `a` (`-`) and only in `b` (`+`), by their longest
/// common subsequence, in order.
fn line_diff(a: &[&str], b: &[&str]) -> Vec<String> {
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for k in (0..m).rev() {
            lcs[i][k] = if a[i] == b[k] {
                lcs[i + 1][k + 1] + 1
            } else {
                lcs[i + 1][k].max(lcs[i][k + 1])
            };
        }
    }
    let (mut i, mut k) = (0, 0);
    let mut out = Vec::new();
    while i < n || k < m {
        if i < n && k < m && a[i] == b[k] {
            i += 1;
            k += 1;
        } else if k < m && (i == n || lcs[i][k + 1] >= lcs[i + 1][k]) {
            out.push(format!("  + {}", b[k]));
            k += 1;
        } else {
            out.push(format!("  - {}", a[i]));
            i += 1;
        }
    }
    out
}

/// Compares an expected snapshot with an actual one, tag by tag, in the
/// order the tags first appear.
pub fn compare(expected: &str, actual: &str) -> Result<(), Mismatch> {
    let want = tagged(expected);
    let got = tagged(actual);
    if want == got {
        return Ok(());
    }
    let mut order: Vec<String> = Vec::new();
    for (tag, _) in want.iter().chain(got.iter()) {
        if !order.contains(tag) {
            order.push(tag.clone());
        }
    }
    let mut tags = Vec::new();
    let mut text = String::new();
    for tag in order {
        let a: Vec<&str> = want
            .iter()
            .filter(|(t, _)| *t == tag)
            .map(|(_, l)| l.as_str())
            .collect();
        let b: Vec<&str> = got
            .iter()
            .filter(|(t, _)| *t == tag)
            .map(|(_, l)| l.as_str())
            .collect();
        if a == b {
            continue;
        }
        let shown = match tag.strip_prefix("step ") {
            Some(key) => format!("step `{key}`"),
            None => tag.clone(),
        };
        text.push_str(&format!("{shown}:\n"));
        for line in line_diff(&a, &b) {
            text.push_str(&line);
            text.push('\n');
        }
        tags.push(tag);
    }
    if tags.is_empty() {
        let seq = |lines: &[(String, String)]| {
            let mut s: Vec<String> = Vec::new();
            for (t, _) in lines {
                if s.last() != Some(t) {
                    s.push(t.clone());
                }
            }
            s
        };
        tags.push("order".into());
        text.push_str(&format!(
            "order: the same lines under each tag, in another order of tags:\n  - {:?}\n  + {:?}\n",
            seq(&want),
            seq(&got)
        ));
    }
    Err(Mismatch { tags, text })
}

/// Fails, with the diff naming each differing tag, when `actual` differs
/// from the expected snapshot. The actual snapshot is written under
/// `CARGO_TARGET_TMPDIR`; the fixture is never written.
pub fn assert_snapshot(expected: &str, actual: &str) {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(ACTUAL);
    std::fs::write(&path, actual).unwrap();
    if let Err(m) = compare(expected, actual) {
        panic!(
            "the golden snapshot differs at {}:\n{}\nthe actual snapshot is {}; fixtures/golden/so-far/snapshot.txt is edited by hand, never by the test",
            m.tags.join(", "),
            m.text,
            path.display()
        );
    }
}
