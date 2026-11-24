//! Test fakes (R8, AD-33): no test needs the network, a real model or the keychain.
//!
//! - [`FakeLlm`]: an `LlmProvider` that plays [`ScriptedTurn`]s in order (the last
//!   repeats): streamed text, tool calls, usage, a delay, a fault
//!   ([`ScriptedTurn::fail`]) or a hang that only cancel ends ([`ScriptedTurn::hang`]),
//!   so a timeout is the caller's deadline elapsing. It records every request.
//! - [`FakeEgress`]: an `EgressClient` that serves scripted ([`FakeResponse`]) or
//!   recorded ([`FakeEgress::replay`], `fixtures/http/`) responses, records every
//!   request it was asked to send and refuses an unrouted URL by name.
//! - [`FixtureTool`]: the one `ToolRunner` (story 3.3), a read-class tool
//!   with a fixed `UntrustedData` result, for the engine's loop tests.
//! - [`FakeClock`]: a `Clock` that moves only when told (`set`, `advance`).
//! - [`FakeSecretStore`]: an in-memory `SecretStore` for canary values.
//! - [`CollectEvents`] (an `EventSink`) and [`CollectAudit`] (an `AuditSink`).
//! - [`InMemoryConfigHistory`]: a `ConfigHistory` without SQLite, for am-config's tests.
//! - [`recorded`] and [`fixture`]: paths and bytes under `fixtures/`.
//! - [`FakeProjectIdentity`]: a `ProjectIdentity` (AD-32) from std metadata,
//!   with scripted tool failures, unmounted volumes and a network share.
//! - [`copy_tree`]: a stream copy of a folder (never `std::fs::copy`, whose
//!   `clonefile(2)` step fails with EPERM under `~/Library/CloudStorage/`).
//! - [`FakeTerminal`]: the only fake of the `Terminal` port (story 2.1,
//!   AD-15): scripted typed lines, an interrupt, a timeout, and a record of
//!   the echo state at each read. It lives here, in a dev-dependency, so
//!   the `agentic` binary can never be given one (`xtask/tests/release_deps.rs`).
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::{
    AuditEvent, AuditSink, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, Clock, EgressClient,
    EgressError, EventSink, HttpRequest, HttpResponse, LlmProvider, ModelCapabilities,
    ProviderError, SecretError, SecretStore, SecretString, StopReason, StreamEvent, Terminal,
    TerminalError, ToolCall, Usage,
};
use tokio::sync::mpsc;

/// The repository root (for fixtures).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// A path under `fixtures/`.
pub fn fixture(rel: &str) -> PathBuf {
    repo_root().join("fixtures").join(rel)
}

/// Read a recorded HTTP body from `fixtures/http/`.
pub fn recorded(rel: &str) -> Vec<u8> {
    let p = fixture(&format!("http/{rel}"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("missing fixture {}: {e}", p.display()))
}

// ---------------------------------------------------------------- egress

/// A scripted HTTP response.
#[derive(Clone)]
pub struct FakeResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub chunks: Vec<Vec<u8>>,
    /// Delay before each chunk.
    pub chunk_delay: Duration,
    /// Keep the body open after the last chunk until the reader drops it.
    pub hang_after: bool,
}

impl FakeResponse {
    /// Serve `body` as `text/event-stream`, normalised: CRLF becomes LF, the
    /// text is split after each `\n\n`, and whitespace-only chunks are dropped,
    /// so the chunks need not concatenate to `body`. Use
    /// [`FakeEgress::replay`] for a byte-exact recorded file.
    pub fn sse(body: Vec<u8>) -> Self {
        FakeResponse {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            chunks: split_events(&body),
            chunk_delay: Duration::from_millis(0),
            hang_after: false,
        }
    }
    pub fn json(status: u16, body: &str) -> Self {
        FakeResponse {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            chunks: vec![body.as_bytes().to_vec()],
            chunk_delay: Duration::from_millis(0),
            hang_after: false,
        }
    }
    pub fn with_delay(mut self, d: Duration) -> Self {
        self.chunk_delay = d;
        self
    }
    pub fn hanging(mut self) -> Self {
        self.hang_after = true;
        self
    }
}

/// Split an SSE body for [`FakeResponse::sse`], normalised: CRLF becomes LF,
/// the text is split after each `\n\n`, and whitespace-only chunks are
/// dropped (so delays apply per event, not per byte of padding).
fn split_events(body: &[u8]) -> Vec<Vec<u8>> {
    let text = String::from_utf8_lossy(body).replace("\r\n", "\n");
    text.split_inclusive("\n\n")
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.as_bytes().to_vec())
        .collect()
}

/// Split an SSE body for [`FakeEgress::replay`], byte-exact: a chunk ends
/// after each `\n\n` or `\r\n\r\n` and nowhere else, nothing is normalised
/// or dropped, and the chunks concatenate to `body` exactly. Events delimited
/// any other way (e.g. `\r\r`) stay in one chunk with what follows them.
fn split_events_exact(body: &[u8]) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < body.len() {
        let end = if body[i..].starts_with(b"\n\n") {
            Some(i + 2)
        } else if body[i..].starts_with(b"\r\n\r\n") {
            Some(i + 4)
        } else {
            None
        };
        match end {
            Some(e) => {
                chunks.push(body[start..e].to_vec());
                start = e;
                i = e;
            }
            None => i += 1,
        }
    }
    if start < body.len() {
        chunks.push(body[start..].to_vec());
    }
    chunks
}

type Matcher = Box<dyn Fn(&HttpRequest) -> bool + Send + Sync>;

#[derive(Default)]
pub struct FakeEgress {
    routes: Mutex<Vec<(Matcher, FakeResponse)>>,
    requests: Mutex<Vec<HttpRequest>>,
    body_dropped: Arc<AtomicBool>,
}

impl FakeEgress {
    pub fn new() -> Arc<Self> {
        Arc::new(FakeEgress::default())
    }
    /// Answer requests whose URL contains `url_part`.
    pub fn respond(&self, url_part: &str, resp: FakeResponse) {
        let part = url_part.to_string();
        self.routes
            .lock()
            .unwrap()
            .push((Box::new(move |r: &HttpRequest| r.url.contains(&part)), resp));
    }
    /// Answer requests whose URL contains `url_part` and whose body contains `body_part`.
    pub fn respond_if(&self, url_part: &str, body_part: &str, resp: FakeResponse) {
        let part = url_part.to_string();
        let body = body_part.as_bytes().to_vec();
        self.routes.lock().unwrap().push((
            Box::new(move |r: &HttpRequest| {
                r.url.contains(&part)
                    && r.body
                        .windows(body.len().max(1))
                        .any(|w| w == body.as_slice())
            }),
            resp,
        ));
    }
    /// Answer requests whose URL contains `url_part` with the recorded file
    /// `fixtures/http/<fixture_rel>`, status 200, byte-exact (unlike
    /// [`FakeResponse::sse`], nothing is normalised). An `.sse` file is served as
    /// `text/event-stream`, split after each `\n\n` or `\r\n\r\n` and nowhere
    /// else; a `.json` file as one `application/json` chunk; any other file as
    /// one `application/octet-stream` chunk.
    pub fn replay(&self, url_part: &str, fixture_rel: &str) {
        let body = recorded(fixture_rel);
        let (content_type, chunks) = if fixture_rel.ends_with(".sse") {
            ("text/event-stream", split_events_exact(&body))
        } else if fixture_rel.ends_with(".json") {
            ("application/json", vec![body])
        } else {
            ("application/octet-stream", vec![body])
        };
        self.respond(
            url_part,
            FakeResponse {
                status: 200,
                headers: vec![("content-type".into(), content_type.into())],
                chunks,
                chunk_delay: Duration::from_millis(0),
                hang_after: false,
            },
        );
    }
    /// Every request the code under test asked to send.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
    /// Total request body bytes the code under test asked to send.
    pub fn bytes_sent(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.body.len())
            .sum()
    }
    /// True once a response body was dropped by its reader before it ended.
    pub fn body_dropped(&self) -> bool {
        self.body_dropped.load(Ordering::SeqCst)
    }
}

impl EgressClient for FakeEgress {
    fn send(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, EgressError>> {
        let found = self
            .routes
            .lock()
            .unwrap()
            .iter()
            .find(|(m, _)| m(&request))
            .map(|(_, r)| r.clone());
        self.requests.lock().unwrap().push(request.clone());
        let dropped = self.body_dropped.clone();
        Box::pin(async move {
            let Some(resp) = found else {
                return Err(EgressError::Blocked(format!(
                    "FakeEgress has no route for {}",
                    request.url
                )));
            };
            let (tx, rx) = mpsc::channel(4);
            let chunks = resp.chunks.clone();
            let delay = resp.chunk_delay;
            let hang = resp.hang_after;
            tokio::spawn(async move {
                for c in chunks {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    if tx.send(Ok(c)).await.is_err() {
                        dropped.store(true, Ordering::SeqCst);
                        return;
                    }
                }
                if hang {
                    tx.closed().await;
                    dropped.store(true, Ordering::SeqCst);
                }
            });
            Ok(HttpResponse {
                status: resp.status,
                headers: resp.headers.clone(),
                body: am_net::channel_body(rx),
            })
        })
    }
}

// ---------------------------------------------------------------- llm

/// What a scripted turn does once its delay is over.
#[derive(Debug)]
enum Ending {
    /// Stream the text, then the tool calls, then the usage.
    Reply,
    /// Return this error, with no events.
    Fail(ProviderError),
    /// Never answer; only cancel ends it.
    Hang,
}

/// One scripted model turn, built with [`ScriptedTurn::text`],
/// [`ScriptedTurn::fail`] or [`ScriptedTurn::hang`].
#[derive(Debug)]
pub struct ScriptedTurn {
    text: String,
    tool_calls: Vec<ToolCall>,
    usage: Option<Usage>,
    delay: Duration,
    ending: Ending,
}

impl ScriptedTurn {
    /// A turn that streams `text` word by word and ends with `EndTurn`
    /// (`ToolUse` when it also has tool calls). Empty text streams no text.
    pub fn text(text: impl Into<String>) -> Self {
        ScriptedTurn {
            text: text.into(),
            tool_calls: Vec::new(),
            usage: None,
            delay: Duration::ZERO,
            ending: Ending::Reply,
        }
    }
    /// A turn that fails with `error` (no events), e.g. `ProviderError::Http`.
    pub fn fail(error: ProviderError) -> Self {
        ScriptedTurn {
            ending: Ending::Fail(error),
            ..ScriptedTurn::text("")
        }
    }
    /// A turn that never answers: the caller's deadline elapses, or cancel
    /// ends it with `StopReason::Cancelled`.
    pub fn hang() -> Self {
        ScriptedTurn {
            ending: Ending::Hang,
            ..ScriptedTurn::text("")
        }
    }
    /// The usage the turn reports after its text and tool calls.
    ///
    /// Panics on a [`ScriptedTurn::fail`] or [`ScriptedTurn::hang`] turn, which
    /// reports no usage.
    pub fn usage(mut self, input_tokens: u64, output_tokens: u64) -> Self {
        self.refuse_on_no_reply("usage");
        self.usage = Some(Usage {
            input_tokens,
            output_tokens,
        });
        self
    }
    /// Add a tool call (repeatable); ids are `call-1`, `call-2`, … in order.
    ///
    /// Panics on a [`ScriptedTurn::fail`] or [`ScriptedTurn::hang`] turn, which
    /// makes no tool calls.
    pub fn tool_call(mut self, name: &str, arguments_json: &str) -> Self {
        self.refuse_on_no_reply("tool_call");
        let id = format!("call-{}", self.tool_calls.len() + 1);
        self.tool_calls.push(ToolCall {
            id,
            name: name.to_string(),
            arguments_json: arguments_json.to_string(),
        });
        self
    }
    /// A fault or hang turn never streams, so a setting for the reply would be
    /// dropped silently; refuse it instead.
    fn refuse_on_no_reply(&self, setting: &str) {
        let ending = match self.ending {
            Ending::Reply => return,
            Ending::Fail(_) => "fail",
            Ending::Hang => "hang",
        };
        panic!(
            "ScriptedTurn::{ending}() streams nothing, so .{setting}() would be ignored; \
             script it on a ScriptedTurn::text(..) turn instead"
        );
    }
    /// Wait this long before answering (cancel still ends the wait).
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

impl Clone for ScriptedTurn {
    fn clone(&self) -> Self {
        ScriptedTurn {
            text: self.text.clone(),
            tool_calls: self.tool_calls.clone(),
            usage: self.usage,
            delay: self.delay,
            ending: match &self.ending {
                Ending::Reply => Ending::Reply,
                Ending::Fail(e) => Ending::Fail(copy_error(e)),
                Ending::Hang => Ending::Hang,
            },
        }
    }
}

/// `ProviderError` is not `Clone` (am-domain); a fault turn repeats, so copy it
/// here. The match is exhaustive: a new variant must be handled.
fn copy_error(e: &ProviderError) -> ProviderError {
    match e {
        ProviderError::Unsupported {
            media_type,
            model,
            message,
        } => ProviderError::Unsupported {
            media_type: media_type.clone(),
            model: model.clone(),
            message: message.clone(),
        },
        ProviderError::Egress(m) => ProviderError::Egress(m.clone()),
        ProviderError::Http { status, body } => ProviderError::Http {
            status: *status,
            body: body.clone(),
        },
        ProviderError::Protocol(m) => ProviderError::Protocol(m.clone()),
    }
}

/// A deterministic `LlmProvider`: plays its scripted turns in order (the last
/// one repeats) and records every request.
pub struct FakeLlm {
    turns: Vec<ScriptedTurn>,
    next: AtomicUsize,
    requests: Mutex<Vec<ChatRequest>>,
    caps: ModelCapabilities,
}

impl FakeLlm {
    pub fn new(turns: Vec<ScriptedTurn>) -> Self {
        FakeLlm {
            turns,
            next: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            caps: ModelCapabilities::default(),
        }
    }
    /// How many times `stream` was called.
    pub fn calls(&self) -> usize {
        self.next.load(Ordering::SeqCst)
    }
    /// Every request `stream` was called with, in order.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn cancelled_outcome() -> ChatOutcome {
    ChatOutcome {
        text: String::new(),
        tool_calls: vec![],
        usage: None,
        stop: StopReason::Cancelled,
    }
}

impl LlmProvider for FakeLlm {
    fn account_id(&self) -> String {
        "fake".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        self.caps
    }
    fn check_file_parts(&self, request: &ChatRequest) -> Result<(), ProviderError> {
        for (name, mt) in request.file_parts() {
            if !self.caps.reads(am_domain::FileKind::of(&mt)) {
                return Err(ProviderError::unsupported(&name, &mt, &request.model));
            }
        }
        Ok(())
    }
    fn stream(
        &self,
        request: ChatRequest,
        cancel: Arc<dyn CancelSignal>,
        sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let checked = self.check_file_parts(&request);
        // Take the turn index and record the request under one lock, so
        // `requests()[i]` is the request that played turn `i` even when calls race.
        let i = {
            let mut requests = self.requests.lock().unwrap();
            let i = requests.len();
            requests.push(request);
            self.next.fetch_add(1, Ordering::SeqCst);
            i
        };
        let turn = self
            .turns
            .get(i.min(self.turns.len().saturating_sub(1)))
            .cloned();
        Box::pin(async move {
            checked?;
            let turn = turn.ok_or_else(|| ProviderError::Protocol("no scripted turn".into()))?;
            tokio::select! {
                _ = cancel.cancelled() => return Ok(cancelled_outcome()),
                _ = tokio::time::sleep(turn.delay) => {}
            }
            match turn.ending {
                Ending::Fail(e) => return Err(e),
                Ending::Hang => {
                    cancel.cancelled().await;
                    return Ok(cancelled_outcome());
                }
                Ending::Reply => {}
            }
            for w in turn.text.split_inclusive(' ') {
                sink.on_event(StreamEvent::TextDelta(w.to_string()));
            }
            for c in &turn.tool_calls {
                sink.on_event(StreamEvent::ToolCall(c.clone()));
            }
            if let Some(u) = turn.usage {
                sink.on_event(StreamEvent::Usage(u));
            }
            let stop = if turn.tool_calls.is_empty() {
                StopReason::EndTurn
            } else {
                StopReason::ToolUse
            };
            Ok(ChatOutcome {
                text: turn.text,
                tool_calls: turn.tool_calls,
                usage: turn.usage,
                stop,
            })
        })
    }
}

/// Collects stream events.
#[derive(Default)]
pub struct CollectEvents(pub Mutex<Vec<StreamEvent>>);

impl EventSink for CollectEvents {
    fn on_event(&self, event: StreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

impl CollectEvents {
    pub fn events(&self) -> Vec<StreamEvent> {
        self.0.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------- tools

/// The one tool runner there is (story 3.3, owner condition 2): a
/// read-class tool that returns a fixed text as `UntrustedData`, as every
/// tool result is (AD-10). It lives here, in a dev-dependency, so the
/// `agentic` binary never has a tool; the engine takes it only through
/// `am_engine::run::run_plan_with_tools`, which shipped code never names
/// (`cargo xtask seam-guard`). Its per-step limit is high (100), so only the
/// story's caps refuse it.
pub struct FixtureTool {
    runs: AtomicUsize,
}

impl FixtureTool {
    /// The tool's id, as a model asks for it.
    pub const NAME: &str = "fixture_read";
    /// The fixed text of every result.
    pub const RESULT: &str = "The fixture tool read this fixed text.";

    pub fn new() -> Arc<FixtureTool> {
        Arc::new(FixtureTool {
            runs: AtomicUsize::new(0),
        })
    }

    /// How many calls ran.
    pub fn calls(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl am_domain::ToolRunner for FixtureTool {
    fn manifests(&self) -> Vec<am_domain::tool_manifest::ToolManifest> {
        vec![am_domain::tool_manifest::ToolManifest {
            id: FixtureTool::NAME.to_string(),
            class: am_domain::tool_manifest::ActionClass::Read,
            network: am_domain::tool_manifest::NetworkNeed::None,
            max_uses_per_step: 100,
        }]
    }

    fn run(
        &self,
        call: ToolCall,
        _cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<am_domain::UntrustedData, String>> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let result = if call.name == FixtureTool::NAME {
            Ok(am_domain::UntrustedData::text(
                format!("tool:{}", FixtureTool::NAME),
                FixtureTool::RESULT,
            ))
        } else {
            Err(format!("the fixture tool does not run `{}`", call.name))
        };
        Box::pin(async move { result })
    }
}

// ---------------------------------------------------------------- clock

pub struct FakeClock {
    now: Mutex<jiff::Timestamp>,
}

impl FakeClock {
    pub fn at(rfc3339: &str) -> Arc<Self> {
        Arc::new(FakeClock {
            now: Mutex::new(rfc3339.parse().expect("valid RFC 3339 instant")),
        })
    }
    pub fn set(&self, rfc3339: &str) {
        *self.now.lock().unwrap() = rfc3339.parse().expect("valid RFC 3339 instant");
    }
    pub fn advance(&self, d: jiff::SignedDuration) {
        let mut now = self.now.lock().unwrap();
        *now = now.checked_add(d).expect("in range");
    }
}

impl Clock for FakeClock {
    fn now(&self) -> jiff::Timestamp {
        *self.now.lock().unwrap()
    }
}

// ---------------------------------------------------------------- secrets

#[derive(Default)]
pub struct FakeSecretStore {
    items: Mutex<HashMap<String, String>>,
}

impl FakeSecretStore {
    pub fn new() -> Arc<Self> {
        Arc::new(FakeSecretStore::default())
    }
    pub fn with(account: &str, canary: &str) -> Arc<Self> {
        let s = FakeSecretStore::default();
        s.items
            .lock()
            .unwrap()
            .insert(account.to_string(), canary.to_string());
        Arc::new(s)
    }
}

impl SecretStore for FakeSecretStore {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .get(account)
            .map(|v| SecretString::new(v.clone())))
    }
    fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError> {
        self.items
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.expose().to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.items.lock().unwrap().remove(account);
        Ok(())
    }
}

// ---------------------------------------------------------------- audit

#[derive(Default)]
pub struct CollectAudit(Mutex<Vec<AuditEvent>>);

impl CollectAudit {
    pub fn new() -> Arc<Self> {
        Arc::new(CollectAudit::default())
    }
    pub fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap().clone()
    }
}

impl AuditSink for CollectAudit {
    fn record(&self, event: AuditEvent) {
        self.0.lock().unwrap().push(event);
    }
}

// ------------------------------------------------------- config history

/// An in-memory `ConfigHistory` for am-config's tests, without SQLite or
/// audit. It checks only that an intent's `ref_entry` names an existing entry
/// and that a commit, reconcile or drop names a pending intent; it does not
/// check hashes, kinds or paths as `am_store::StoreConfigHistory` does. Ids
/// are UUID-shaped counters (`00000000-0000-7000-8000-000000000001`, …).
#[derive(Default)]
pub struct InMemoryConfigHistory {
    state: Mutex<HistoryState>,
}

#[derive(Default)]
struct HistoryState {
    next: u64,
    intents: Vec<am_domain::PendingIntent>,
    entries: Vec<am_domain::HistoryEntry>,
}

fn history_now() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

impl InMemoryConfigHistory {
    fn state(&self) -> Result<std::sync::MutexGuard<'_, HistoryState>, am_domain::HistoryError> {
        self.state
            .lock()
            .map_err(|_| am_domain::HistoryError::Store("poisoned".into()))
    }

    fn complete(
        &self,
        intent_id: String,
    ) -> Result<am_domain::HistoryEntry, am_domain::HistoryError> {
        let mut st = self.state()?;
        let pos = st
            .intents
            .iter()
            .position(|p| p.id == intent_id)
            .ok_or(am_domain::HistoryError::NoIntent(intent_id))?;
        let p = st.intents.remove(pos);
        let seq = st.entries.last().map_or(1, |e| e.seq + 1);
        let entry = am_domain::HistoryEntry {
            id: p.id,
            seq,
            at: history_now(),
            actor: p.actor,
            file: p.intent.file,
            kind: p.intent.kind,
            ref_entry: p.intent.ref_entry,
            before_sha256: p.intent.before_sha256,
            after_sha256: p.intent.after_sha256,
            before: p.intent.before,
            after: p.intent.after,
            diff: p.intent.diff,
        };
        st.entries.push(entry.clone());
        Ok(entry)
    }
}

impl am_domain::ConfigHistory for InMemoryConfigHistory {
    fn begin_intent(
        &self,
        actor: am_domain::Actor,
        intent: am_domain::NewIntent,
    ) -> Result<String, am_domain::HistoryError> {
        let mut st = self.state()?;
        if let Some(r) = &intent.ref_entry
            && !st.entries.iter().any(|e| &e.id == r)
        {
            return Err(am_domain::HistoryError::Store(format!(
                "no History entry {r}"
            )));
        }
        st.next += 1;
        let id = format!("00000000-0000-7000-8000-{:012x}", st.next);
        st.intents.push(am_domain::PendingIntent {
            id: id.clone(),
            at: history_now(),
            actor,
            intent,
        });
        Ok(id)
    }

    fn commit_entry(
        &self,
        _actor: am_domain::Actor,
        intent_id: String,
    ) -> Result<am_domain::HistoryEntry, am_domain::HistoryError> {
        self.complete(intent_id)
    }

    fn drop_intent(
        &self,
        _actor: am_domain::Actor,
        intent_id: String,
    ) -> Result<(), am_domain::HistoryError> {
        let mut st = self.state()?;
        let before = st.intents.len();
        st.intents.retain(|p| p.id != intent_id);
        if st.intents.len() == before {
            return Err(am_domain::HistoryError::NoIntent(intent_id));
        }
        Ok(())
    }

    fn reconcile_intent(
        &self,
        _actor: am_domain::Actor,
        intent_id: String,
    ) -> Result<am_domain::HistoryEntry, am_domain::HistoryError> {
        self.complete(intent_id)
    }

    fn pending_intents(&self) -> Result<Vec<am_domain::PendingIntent>, am_domain::HistoryError> {
        Ok(self.state()?.intents.clone())
    }

    fn entries(
        &self,
        file: Option<String>,
    ) -> Result<Vec<am_domain::HistoryEntry>, am_domain::HistoryError> {
        Ok(self
            .state()?
            .entries
            .iter()
            .filter(|e| file.as_ref().is_none_or(|f| &e.file == f))
            .cloned()
            .collect())
    }

    fn entry(
        &self,
        id: String,
    ) -> Result<Option<am_domain::HistoryEntry>, am_domain::HistoryError> {
        Ok(self.state()?.entries.iter().find(|e| e.id == id).cloned())
    }
}

// ---------------------------------------------------------------- files

/// Copies the folder `from` into `to` (created as needed), reading and
/// writing every file's bytes: never `std::fs::copy`, whose first step on
/// macOS is `clonefile(2)`. Links are not followed or copied.
pub fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("cannot create {}: {e}", to.display()));
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", from.display()))
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let target = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else if kind.is_file() {
            stream_copy_file(&entry.path(), &target);
        }
    }
}

/// One file's bytes, read and written through a buffer.
pub fn stream_copy_file(from: &std::path::Path, to: &std::path::Path) {
    use std::io::{Read, Write};
    let mut src =
        std::fs::File::open(from).unwrap_or_else(|e| panic!("cannot read {}: {e}", from.display()));
    let mut dst =
        std::fs::File::create(to).unwrap_or_else(|e| panic!("cannot create {}: {e}", to.display()));
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = src.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).unwrap();
    }
    dst.sync_all().unwrap();
}

// ---------------------------------------------------------------- identity

/// A [`am_domain::ProjectIdentity`] for tests. A root's identity comes from
/// std metadata (volume `fake-volume-<device>`, the folder's inode), so a
/// copy of a folder is another root and a renamed one is the same root.
/// Tool failures, unmounted volumes and a network share are scripted.
pub struct FakeProjectIdentity {
    state: Mutex<FakeIdentityState>,
}

struct FakeIdentityState {
    mac_name: Result<String, am_domain::IdentityError>,
    root_failure: Option<am_domain::IdentityError>,
    locate_failure: Option<am_domain::IdentityError>,
    unmounted: Vec<String>,
    calls: usize,
}

impl FakeProjectIdentity {
    /// A Mac called `mac_name` whose tools all work.
    pub fn new(mac_name: &str) -> FakeProjectIdentity {
        FakeProjectIdentity {
            state: Mutex::new(FakeIdentityState {
                mac_name: Ok(mac_name.to_string()),
                root_failure: None,
                locate_failure: None,
                unmounted: Vec::new(),
                calls: 0,
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeIdentityState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Every root lookup fails with `err` (a tool that failed, hung or
    /// printed no key; or `NetworkShare`).
    pub fn fail_root(&self, err: am_domain::IdentityError) {
        self.state().root_failure = Some(err);
    }

    /// The Mac-name lookup fails with `err`.
    pub fn fail_mac_name(&self, err: am_domain::IdentityError) {
        self.state().mac_name = Err(err);
    }

    /// Locating a recorded root fails with `err`.
    pub fn fail_locate(&self, err: am_domain::IdentityError) {
        self.state().locate_failure = Some(err);
    }

    /// The volume `volume` reads as not mounted.
    pub fn unmount(&self, volume: &str) {
        self.state().unmounted.push(volume.to_string());
    }

    /// The volume `volume` reads as mounted again.
    pub fn mount(&self, volume: &str) {
        self.state().unmounted.retain(|v| v != volume);
    }

    /// How many identity calls were made.
    pub fn calls(&self) -> usize {
        self.state().calls
    }

    /// The fake's identity of `root` (as [`am_domain::ProjectIdentity::root`]
    /// gives it when no failure is scripted).
    pub fn identity_of(root: &std::path::Path) -> std::io::Result<am_domain::RootIdentity> {
        let (dev, ino) = dev_ino(root)?;
        Ok(am_domain::RootIdentity {
            volume: format!("fake-volume-{dev}"),
            file_id: ino,
            path: root.to_path_buf(),
        })
    }
}

#[cfg(unix)]
fn dev_ino(path: &std::path::Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    Ok((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn dev_ino(_path: &std::path::Path) -> std::io::Result<(u64, u64)> {
    Err(std::io::Error::other("not supported on this platform yet"))
}

impl am_domain::ProjectIdentity for FakeProjectIdentity {
    fn root(
        &self,
        root: &std::path::Path,
    ) -> Result<am_domain::RootIdentity, am_domain::IdentityError> {
        let failure = {
            let mut st = self.state();
            st.calls += 1;
            st.root_failure.clone()
        };
        if let Some(e) = failure {
            return Err(e);
        }
        FakeProjectIdentity::identity_of(root).map_err(|e| am_domain::IdentityError::Tool {
            tool: "fake stat".into(),
            output: e.to_string(),
        })
    }

    fn locate(
        &self,
        recorded: &am_domain::RootIdentity,
    ) -> Result<am_domain::VolumeState, am_domain::IdentityError> {
        let (failure, unmounted) = {
            let mut st = self.state();
            st.calls += 1;
            (
                st.locate_failure.clone(),
                st.unmounted.contains(&recorded.volume),
            )
        };
        if let Some(e) = failure {
            return Err(e);
        }
        if unmounted {
            return Ok(am_domain::VolumeState::Unmounted);
        }
        let present = dev_ino(&recorded.path).is_ok_and(|(dev, ino)| {
            format!("fake-volume-{dev}") == recorded.volume && ino == recorded.file_id
        });
        Ok(am_domain::VolumeState::Mounted {
            root: present.then(|| recorded.path.clone()),
        })
    }

    fn mac_name(&self) -> Result<String, am_domain::IdentityError> {
        let mut st = self.state();
        st.calls += 1;
        st.mac_name.clone()
    }
}

// ---------------------------------------------------------------- terminal

/// What the owner does at one prompt of a [`FakeTerminal`].
enum Typed {
    Line(String),
    /// A line, with something that happens while the prompt waits for it.
    LineAfter(String, Box<dyn FnOnce() + Send>),
    Interrupt,
    TimeOut,
    /// A line typed with Return before the next prompt was shown (story
    /// 3.4, R9): read like a line unless the command discards what was
    /// typed ahead first.
    Ahead(String),
    /// Text typed with no Return, then the end of input (Ctrl-D twice;
    /// story 3.4, R11): the read is `Closed`, and the text is no answer.
    Unfinished,
}

struct TerminalState {
    interactive: bool,
    script: std::collections::VecDeque<Typed>,
    echo: bool,
    echo_reads: Vec<bool>,
    timeouts: Vec<Option<Duration>>,
    shown: String,
    /// How many times `interrupted()` was asked.
    checks: usize,
    /// The `interrupted()` calls (1-based) that report a Ctrl-C.
    interrupt_checks: Vec<usize>,
    /// How many times `discard_typed_ahead()` was called.
    discards: usize,
}

/// A scripted `Terminal` (story 2.1): what is typed at each prompt is
/// queued with [`FakeTerminal::line`], [`FakeTerminal::interrupt`] and
/// [`FakeTerminal::time_out`]; an exhausted script reads as the end of
/// input. Clones share one state, so a test keeps a handle to the terminal
/// it gave the command and reads back what was shown, how many reads were
/// made, the echo state at each read and the time limit each was given.
#[derive(Clone)]
pub struct FakeTerminal {
    state: Arc<Mutex<TerminalState>>,
}

impl Default for FakeTerminal {
    fn default() -> Self {
        FakeTerminal::new()
    }
}

impl FakeTerminal {
    fn with(interactive: bool) -> FakeTerminal {
        FakeTerminal {
            state: Arc::new(Mutex::new(TerminalState {
                interactive,
                script: std::collections::VecDeque::new(),
                echo: true,
                echo_reads: Vec::new(),
                timeouts: Vec::new(),
                shown: String::new(),
                checks: 0,
                interrupt_checks: Vec::new(),
                discards: 0,
            })),
        }
    }

    /// A real interactive terminal: stdin and stdout are terminals, echo on.
    pub fn new() -> FakeTerminal {
        FakeTerminal::with(true)
    }

    /// Stdin is a pipe or a file: not interactive.
    pub fn piped() -> FakeTerminal {
        FakeTerminal::with(false)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, TerminalState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The owner types `text` and Return at the next prompt.
    pub fn line(&self, text: &str) {
        self.state().script.push_back(Typed::Line(text.to_string()));
    }

    /// The owner types `text` and Return at the next prompt, and `meanwhile`
    /// runs while that prompt waits (another program changing a file, say),
    /// before the read returns.
    pub fn line_and(&self, text: &str, meanwhile: impl FnOnce() + Send + 'static) {
        self.state()
            .script
            .push_back(Typed::LineAfter(text.to_string(), Box::new(meanwhile)));
    }

    /// The owner presses Ctrl-C at the next prompt.
    pub fn interrupt(&self) {
        self.state().script.push_back(Typed::Interrupt);
    }

    /// The owner presses Ctrl-C between prompts, so that the `n`-th time
    /// the command asks `interrupted()` (counting from 1) it is reported.
    pub fn interrupt_on_check(&self, n: usize) {
        self.state().interrupt_checks.push(n);
    }

    /// The owner types nothing at the next prompt until its time is up.
    pub fn time_out(&self) {
        self.state().script.push_back(Typed::TimeOut);
    }

    /// The owner types `text` and Return before the next prompt is shown
    /// (story 3.4, R9): the next read returns it, unless the command
    /// discarded what was typed ahead first (`discard_typed_ahead` drops
    /// every such line at the front of the script).
    pub fn type_ahead(&self, text: &str) {
        self.state()
            .script
            .push_back(Typed::Ahead(text.to_string()));
    }

    /// The owner types `text` with no Return, then ends the input (Ctrl-D
    /// twice; story 3.4, R11): the read is `Err(TerminalError::Closed)`.
    pub fn unfinished(&self, text: &str) {
        // What was typed is never read: only a line ended by Return is.
        let _ = text;
        self.state().script.push_back(Typed::Unfinished);
    }

    /// How many times the command discarded what was typed ahead.
    pub fn discards(&self) -> usize {
        self.state().discards
    }

    /// Whether the terminal echoes now.
    pub fn echo(&self) -> bool {
        self.state().echo
    }

    /// The echo state during each read, in order.
    pub fn echo_reads(&self) -> Vec<bool> {
        self.state().echo_reads.clone()
    }

    /// The time limit each read was given, in order.
    pub fn timeouts(&self) -> Vec<Option<Duration>> {
        self.state().timeouts.clone()
    }

    /// How many reads were made.
    pub fn reads(&self) -> usize {
        self.state().echo_reads.len()
    }

    /// Everything written to the terminal itself (prompts).
    pub fn shown(&self) -> String {
        self.state().shown.clone()
    }

    fn next(&self, echo: bool, timeout: Option<Duration>) -> Result<String, TerminalError> {
        let typed = {
            let mut st = self.state();
            st.echo = echo;
            st.echo_reads.push(echo);
            st.timeouts.push(timeout);
            let typed = st.script.pop_front();
            // Echo is on again whenever a read returns, however it returns.
            st.echo = true;
            typed
        };
        match typed {
            Some(Typed::Line(text) | Typed::Ahead(text)) => Ok(text),
            // Only a line ended by Return is a line (the port's contract).
            Some(Typed::Unfinished) => Err(TerminalError::Closed),
            Some(Typed::LineAfter(text, meanwhile)) => {
                // Outside the lock: what happens meanwhile may look at the
                // terminal too.
                meanwhile();
                Ok(text)
            }
            Some(Typed::Interrupt) => Err(TerminalError::Interrupted),
            Some(Typed::TimeOut) => Err(TerminalError::TimedOut),
            None => Err(TerminalError::Closed),
        }
    }
}

impl Terminal for FakeTerminal {
    fn interactive(&self) -> bool {
        self.state().interactive
    }

    fn show(&mut self, text: &str) {
        self.state().shown.push_str(text);
    }

    fn read_line(&mut self, timeout: Option<Duration>) -> Result<String, TerminalError> {
        self.next(true, timeout)
    }

    fn read_secret(&mut self) -> Result<SecretString, TerminalError> {
        self.next(false, None).map(SecretString::new)
    }

    /// Drops every line typed ahead at the front of the script (a pending
    /// Ctrl-C, or what the owner types at the prompt, stays).
    fn discard_typed_ahead(&mut self) {
        let mut st = self.state();
        st.discards += 1;
        while matches!(st.script.front(), Some(Typed::Ahead(_))) {
            st.script.pop_front();
        }
    }

    /// A scripted Ctrl-C that is next in the script was pressed between
    /// prompts: it is consumed and reported; so is one scripted for this
    /// call by [`FakeTerminal::interrupt_on_check`]. It is not a read.
    fn interrupted(&mut self) -> bool {
        let mut st = self.state();
        st.checks += 1;
        let now = st.checks;
        if st.interrupt_checks.contains(&now) {
            return true;
        }
        if matches!(st.script.front(), Some(Typed::Interrupt)) {
            st.script.pop_front();
            return true;
        }
        false
    }
}
