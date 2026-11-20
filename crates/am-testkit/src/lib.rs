//! Test fakes (R8, AD-33): no test needs the network, a real model or the keychain.
//!
//! - [`FakeLlm`]: an `LlmProvider` that plays [`ScriptedTurn`]s in order (the last
//!   repeats): streamed text, tool calls, usage, a delay, a fault
//!   ([`ScriptedTurn::fail`]) or a hang that only cancel ends ([`ScriptedTurn::hang`]),
//!   so a timeout is the caller's deadline elapsing. It records every request.
//! - [`FakeEgress`]: an `EgressClient` that serves scripted ([`FakeResponse`]) or
//!   recorded ([`FakeEgress::replay`], `fixtures/http/`) responses, records every
//!   request it was asked to send and refuses an unrouted URL by name.
//! - [`FakeClock`]: a `Clock` that moves only when told (`set`, `advance`).
//! - [`FakeSecretStore`]: an in-memory `SecretStore` for canary values.
//! - [`CollectEvents`] (an `EventSink`) and [`CollectAudit`] (an `AuditSink`).
//! - [`recorded`] and [`fixture`]: paths and bytes under `fixtures/`.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::{
    AuditEvent, AuditSink, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, Clock, EgressClient,
    EgressError, EventSink, HttpRequest, HttpResponse, LlmProvider, ModelCapabilities,
    ProviderError, SecretError, SecretStore, SecretString, StopReason, StreamEvent, ToolCall,
    Usage,
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
