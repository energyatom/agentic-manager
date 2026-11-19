//! Test fakes (AD-33): no test needs the network, a real model or the keychain.
//!
//! - [`FakeEgress`]: an `EgressClient` that serves scripted or recorded HTTP
//!   responses (`fixtures/http/`) and records every request it was asked to send.
//! - [`FakeLlm`]: an `LlmProvider` with scripted turns, delays and usage.
//! - [`FakeClock`], [`FakeSecretStore`], [`CollectAudit`].
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::{
    AuditEvent, AuditSink, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, Clock, EgressClient,
    EgressError, EventSink, HttpRequest, HttpResponse, LlmProvider, ModelCapabilities,
    ProviderError, SecretError, SecretStore, SecretString, StopReason, StreamEvent, Usage,
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

/// Split an SSE body into one chunk per event, so delays apply per event.
fn split_events(body: &[u8]) -> Vec<Vec<u8>> {
    let text = String::from_utf8_lossy(body).replace("\r\n", "\n");
    text.split_inclusive("\n\n")
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.as_bytes().to_vec())
        .collect()
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

/// One scripted model turn.
#[derive(Debug, Clone)]
pub struct ScriptedTurn {
    pub text: String,
    pub usage: Option<Usage>,
    pub delay: Duration,
}

/// A deterministic `LlmProvider`: returns its scripted turns in order (the
/// last one repeats), streaming the text word by word.
pub struct FakeLlm {
    turns: Vec<ScriptedTurn>,
    next: AtomicUsize,
    caps: ModelCapabilities,
}

impl FakeLlm {
    pub fn new(turns: Vec<ScriptedTurn>) -> Self {
        FakeLlm {
            turns,
            next: AtomicUsize::new(0),
            caps: ModelCapabilities::default(),
        }
    }
    pub fn calls(&self) -> usize {
        self.next.load(Ordering::SeqCst)
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
        let i = self.next.fetch_add(1, Ordering::SeqCst);
        let turn = self
            .turns
            .get(i.min(self.turns.len().saturating_sub(1)))
            .cloned();
        let checked = self.check_file_parts(&request);
        Box::pin(async move {
            checked?;
            let turn = turn.ok_or_else(|| ProviderError::Protocol("no scripted turn".into()))?;
            tokio::select! {
                _ = cancel.cancelled() => {
                    return Ok(ChatOutcome { text: String::new(), tool_calls: vec![], usage: None, stop: StopReason::Cancelled });
                }
                _ = tokio::time::sleep(turn.delay) => {}
            }
            for w in turn.text.split_inclusive(' ') {
                sink.on_event(StreamEvent::TextDelta(w.to_string()));
            }
            if let Some(u) = turn.usage {
                sink.on_event(StreamEvent::Usage(u));
            }
            Ok(ChatOutcome {
                text: turn.text,
                tool_calls: vec![],
                usage: turn.usage,
                stop: StopReason::EndTurn,
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
