//! Spike harnesses (E1 story 1). Each criterion's runnable evidence calls into
//! here, both from the tests in `am-e2e` and from `agentic spike ...` for the
//! owner's live runs.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use am_domain::{
    Access, Actor, AuditSink, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, EventSink,
    Instruction, LlmProvider, Message, ModelCapabilities, Part, ProviderError, Role, StopReason,
    StreamEvent, ToolCall, ToolSpec, Usage,
};
use am_engine::TokenSignal;
use am_engine::journal::{LinearWorkflow, Price, create_run, drive_run};
use am_store::Store;
use tokio_util::sync::CancellationToken;

// ------------------------------------------------------------------ C2

/// The model of the crash-resume run: answers after a delay, honours cancel.
/// (A deterministic stand-in so `kill -9` can land mid-call; the provider
/// adapters themselves are proven by criterion 1.)
pub struct DelayProvider {
    pub delay: Duration,
}

impl LlmProvider for DelayProvider {
    fn account_id(&self) -> String {
        "spike-delay".into()
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
        let delay = self.delay;
        Box::pin(async move {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(ChatOutcome { text: String::new(), tool_calls: vec![], usage: None, stop: StopReason::Cancelled }),
                _ = tokio::time::sleep(delay) => {}
            }
            let usage = Usage {
                input_tokens: 100,
                output_tokens: 20,
            };
            let text = format!("done ({} chars of prompt)", request.messages.len());
            sink.on_event(StreamEvent::TextDelta(text.clone()));
            sink.on_event(StreamEvent::Usage(usage));
            Ok(ChatOutcome {
                text,
                tool_calls: vec![],
                usage: Some(usage),
                stop: StopReason::EndTurn,
            })
        })
    }
}

pub fn spike_workflow(steps: usize) -> LinearWorkflow {
    LinearWorkflow {
        workflow: "spike-linear".into(),
        steps: (1..=steps).map(|i| format!("step-{i}")).collect(),
        model: "spike-delay".into(),
        max_output_tokens: 200,
        price: Price {
            input_micro_per_mtok: 3_000_000,
            output_micro_per_mtok: 15_000_000,
            snapshot: "spike-2026-11-19".into(),
        },
    }
}

/// Start a new run, or with `resume` continue the first unfinished one.
/// Returns (run id, final status).
pub async fn spike_run(
    db: &Path,
    steps: usize,
    delay: Duration,
    resume: bool,
) -> Result<(String, String), String> {
    let store = Store::open(db).map_err(|e| e.to_string())?;
    let writer = store.writer();
    let wf = spike_workflow(steps);
    let run_id = if resume {
        let reader = store.reader().map_err(|e| e.to_string())?;
        match reader.unfinished_runs().map_err(|e| e.to_string())?.first() {
            Some(r) => r.id.clone(),
            None => return Err("no unfinished run to resume".into()),
        }
    } else {
        create_run(&writer, &wf, "manual").map_err(|e| e.to_string())?
    };
    let provider: Arc<dyn LlmProvider> = Arc::new(DelayProvider { delay });
    let status = drive_run(
        db,
        &writer,
        provider,
        &wf,
        &run_id,
        CancellationToken::new(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok((run_id, status))
}

// ------------------------------------------------------------------ sinks

#[derive(Default)]
pub struct Collect {
    events: Mutex<Vec<(Instant, StreamEvent)>>,
}

impl EventSink for Collect {
    fn on_event(&self, event: StreamEvent) {
        if let Ok(mut e) = self.events.lock() {
            e.push((Instant::now(), event));
        }
    }
}

impl Collect {
    pub fn text_deltas(&self) -> usize {
        self.events
            .lock()
            .map(|e| {
                e.iter()
                    .filter(|(_, ev)| matches!(ev, StreamEvent::TextDelta(_)))
                    .count()
            })
            .unwrap_or(0)
    }
    pub fn first_text_at(&self) -> Option<Instant> {
        self.events
            .lock()
            .ok()?
            .iter()
            .find_map(|(t, ev)| matches!(ev, StreamEvent::TextDelta(_)).then_some(*t))
    }
}

// ------------------------------------------------------------------ C1

#[derive(Debug, Clone)]
pub struct C1Report {
    pub provider: String,
    pub model: String,
    pub tool_call: Option<ToolCall>,
    pub tool_usage: Option<Usage>,
    pub streamed_text_deltas: usize,
    pub cancel_latency: Duration,
    pub cancelled_stop: StopReason,
    pub usage_at_cancel: Option<Usage>,
}

impl C1Report {
    pub fn passed(&self) -> bool {
        self.tool_call.is_some()
            && self.tool_usage.is_some()
            && self.streamed_text_deltas > 0
            && self.cancelled_stop == StopReason::Cancelled
            && self.cancel_latency <= Duration::from_secs(1)
    }
}

pub const C1_TOOL_PROMPT: &str =
    "Use the lookup_word tool to look up the word otter. Call the tool; do not answer in text.";
pub const C1_STREAM_PROMPT: &str =
    "Count from 1 to 400, writing each number as a word on its own line.";

fn user(text: &str) -> Message {
    Message {
        role: Role::User,
        parts: vec![Part::Instruction(Instruction::authored(text))],
    }
}

/// Criterion 1 against one provider: a tool call with usage, then a long
/// stream cancelled after its first text, timing how fast it stops.
pub async fn c1_probe(
    provider: Arc<dyn LlmProvider>,
    model: &str,
    max_output_tokens: u32,
) -> Result<C1Report, ProviderError> {
    let tool = ToolSpec {
        name: "lookup_word".into(),
        description: "Look up a word in the test dictionary.".into(),
        input_schema_json:
            r#"{"type":"object","properties":{"word":{"type":"string"}},"required":["word"]}"#
                .into(),
    };
    let req = ChatRequest {
        model: model.into(),
        system: vec![Instruction::authored("You are a test harness assistant.")],
        messages: vec![user(C1_TOOL_PROMPT)],
        tools: vec![tool],
        max_output_tokens,
    };
    let sink = Arc::new(Collect::default());
    let out = provider
        .stream(req, TokenSignal::signal(CancellationToken::new()), sink)
        .await?;
    let tool_call = out.tool_calls.first().cloned();
    let tool_usage = out.usage;

    let req = ChatRequest {
        model: model.into(),
        system: vec![Instruction::authored("You are a test harness assistant.")],
        messages: vec![user(C1_STREAM_PROMPT)],
        tools: vec![],
        max_output_tokens,
    };
    let sink = Arc::new(Collect::default());
    let token = CancellationToken::new();
    let fut = provider.stream(req, TokenSignal::signal(token.clone()), sink.clone());
    let handle = tokio::spawn(fut);
    // Cancel once text is flowing (or after 20 s at most).
    let wait_start = Instant::now();
    while sink.first_text_at().is_none() && wait_start.elapsed() < Duration::from_secs(20) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let cancelled_at = Instant::now();
    token.cancel();
    let out = handle
        .await
        .map_err(|e| ProviderError::Protocol(e.to_string()))??;
    let cancel_latency = cancelled_at.elapsed();
    Ok(C1Report {
        provider: provider.account_id(),
        model: model.into(),
        tool_call,
        tool_usage,
        streamed_text_deltas: sink.text_deltas(),
        cancel_latency,
        cancelled_stop: out.stop,
        usage_at_cancel: out.usage,
    })
}

// ------------------------------------------------------------------ C4

pub struct ModelUnderTest {
    /// e.g. "frontier: anthropic claude-opus-5-5"
    pub label: String,
    pub provider: Arc<dyn LlmProvider>,
    pub model: String,
    /// `None` for local models (cost 0, AD-9 conventions).
    pub price: Option<Price>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum C4Outcome {
    Accepted {
        answer: String,
        /// `None` when the folder gives no known word for this file.
        names_known_word: Option<bool>,
    },
    /// Refused before any request, with the worded refusal.
    Refused {
        message: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct C4Row {
    pub file: String,
    pub media_type: String,
    pub model: String,
    pub outcome: C4Outcome,
    pub usage: Option<Usage>,
    pub cost_micro_usd: i64,
}

pub const C4_QUESTION: &str = "Name one distinctive word written in the attached file, or if it has no words, its main colour. Answer with that single word only.";

/// `.known-words` in the folder: `relative/path: word, word` per line.
pub fn known_words(dir: &Path) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(dir.join(".known-words")) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((file, words)) = line.split_once(':') {
                out.insert(
                    file.trim().to_string(),
                    words
                        .split(',')
                        .map(|w| w.trim().to_lowercase())
                        .filter(|w| !w.is_empty())
                        .collect(),
                );
            }
        }
    }
    out
}

/// Criterion 4: link `dir` read-only, list it, read every file and attach it
/// to each model, recording accepted or refused, tokens and cost.
pub async fn files_to_models(
    dir: &Path,
    models: &[ModelUnderTest],
    audit: Arc<dyn AuditSink>,
) -> Result<Vec<C4Row>, String> {
    let root = am_tools::link_folder("spike-docs", dir, Access::Read).map_err(|e| e.to_string())?;
    let folder = am_tools::FolderAccess::new(
        vec![root],
        audit,
        Actor::System {
            component: "spike-c4".into(),
        },
    );
    let known = known_words(dir);
    let mut rows = Vec::new();
    for entry in folder.list_files("spike-docs").map_err(|e| e.to_string())? {
        let data = match folder.read_file("spike-docs", &entry.relative_path) {
            Ok(d) => d,
            Err(e) => {
                for m in models {
                    rows.push(C4Row {
                        file: entry.relative_path.clone(),
                        media_type: entry.media_type.clone(),
                        model: m.label.clone(),
                        outcome: C4Outcome::Refused {
                            message: e.to_string(),
                        },
                        usage: None,
                        cost_micro_usd: 0,
                    });
                }
                continue;
            }
        };
        for m in models {
            let req = ChatRequest {
                model: m.model.clone(),
                system: vec![Instruction::authored(
                    "You read files the user attaches and answer in one word.",
                )],
                messages: vec![Message {
                    role: Role::User,
                    parts: vec![
                        Part::Untrusted(data.clone()),
                        Part::Instruction(Instruction::authored(C4_QUESTION)),
                    ],
                }],
                tools: vec![],
                max_output_tokens: 1024,
            };
            let (outcome, usage) = match m.provider.check_file_parts(&req) {
                Err(ProviderError::Unsupported { message, .. }) => {
                    (C4Outcome::Refused { message }, None)
                }
                Err(e) => (
                    C4Outcome::Failed {
                        error: e.to_string(),
                    },
                    None,
                ),
                Ok(()) => {
                    let sink = Arc::new(Collect::default());
                    match m
                        .provider
                        .stream(req, TokenSignal::signal(CancellationToken::new()), sink)
                        .await
                    {
                        Ok(out) => {
                            let answer = out.text.trim().to_string();
                            let lower = answer.to_lowercase();
                            let names_known_word = known
                                .get(&entry.relative_path)
                                .map(|ws| ws.iter().any(|w| lower.contains(w.as_str())));
                            (
                                C4Outcome::Accepted {
                                    answer,
                                    names_known_word,
                                },
                                out.usage,
                            )
                        }
                        Err(ProviderError::Unsupported { message, .. }) => {
                            (C4Outcome::Refused { message }, None)
                        }
                        Err(e) => (
                            C4Outcome::Failed {
                                error: e.to_string(),
                            },
                            None,
                        ),
                    }
                }
            };
            let cost = match (&m.price, usage) {
                (Some(p), Some(u)) => p.cost(u.input_tokens, u.output_tokens),
                _ => 0,
            };
            rows.push(C4Row {
                file: entry.relative_path.clone(),
                media_type: entry.media_type.clone(),
                model: m.label.clone(),
                outcome,
                usage,
                cost_micro_usd: cost,
            });
        }
    }
    Ok(rows)
}

/// A Markdown table of the C4 rows (the evidence the report links).
pub fn c4_table(rows: &[C4Row]) -> String {
    let mut s = String::from(
        "| File | Type | Model | Result | Answer or refusal | Known word | Tokens in/out | Cost (USD) |\n|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let (result, detail, known) = match &r.outcome {
            C4Outcome::Accepted {
                answer,
                names_known_word,
            } => (
                "accepted",
                answer.replace('|', "/").replace('\n', " "),
                match names_known_word {
                    Some(true) => "yes",
                    Some(false) => "NO",
                    None => "unchecked",
                },
            ),
            C4Outcome::Refused { message } => ("refused", message.clone(), "-"),
            C4Outcome::Failed { error } => ("FAILED", error.replace('\n', " "), "-"),
        };
        let tokens = r
            .usage
            .map(|u| format!("{}/{}", u.input_tokens, u.output_tokens))
            .unwrap_or_else(|| "-".into());
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.6} |\n",
            r.file,
            r.media_type,
            r.model,
            result,
            detail,
            known,
            tokens,
            r.cost_micro_usd as f64 / 1_000_000.0
        ));
    }
    s
}

/// Pass rule for C4: every accepted answer names a known word (where one is
/// given) and nothing failed.
pub fn c4_passed(rows: &[C4Row]) -> bool {
    rows.iter().all(|r| match &r.outcome {
        C4Outcome::Accepted {
            names_known_word, ..
        } => *names_known_word != Some(false),
        C4Outcome::Refused { .. } => true,
        C4Outcome::Failed { .. } => false,
    })
}

/// Anthropic list prices for the spike's frontier model (USD per MTok, from
/// the provider's price list on 2026-11-19); the price file lands in epic 2.
pub fn anthropic_price(model: &str) -> Price {
    let (i, o) = match model {
        "claude-opus-5-5" => (4_000_000, 20_000_000),
        "claude-sonnet-5-5" => (2_000_000, 10_000_000),
        "claude-haiku-5-5" => (100_000, 500_000),
        _ => (4_000_000, 20_000_000),
    };
    Price {
        input_micro_per_mtok: i,
        output_micro_per_mtok: o,
        snapshot: "anthropic-list-2026-11-19".into(),
    }
}
