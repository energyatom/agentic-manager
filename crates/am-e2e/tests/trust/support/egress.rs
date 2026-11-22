//! Shared fakes of the RULE-9 trust tests (story 2.2, AD-12, AD-48): an
//! `EgressDirectory` built by hand, a scripted resolver and a scripted
//! per-hop transport (am-net's two test seams, passed to
//! `NetEgress::with_seams`), and a loopback HTTP server for the tests that
//! run the real constructor. No network: every name resolves through the
//! scripted resolver, and the only sockets are on 127.0.0.1.
//!
//! The test seams are constructor parameters only (the plan's decision 7):
//! `NetEgress::new(directory, secrets, audit)` always uses the system
//! resolver and reqwest; `NetEgress::with_seams(directory, secrets, audit,
//! Seams { resolver, transport, limits })` is the second constructor, which
//! `cargo xtask seam-guard` keeps out of every `crates/*/src` but am-net's.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::net::{Caller, EgressDirectory, Endpoint, reserved_local_account};
use am_domain::{
    Actor, AuditEvent, AuditSink, AuthRef, AuthScheme, BodyChunks, BoxFuture, EgressClient,
    EgressError, HostPattern, HttpMethod, HttpRequest, HttpResponse, SecretError, SecretStore,
    SecretString,
};
use am_net::{Hop, Limits, NetEgress, Resolver, Seams, Transport};
use am_testkit::CollectAudit;

/// The seat of most tests: `docs.example.org` and `*.example.org`.
pub const SEAT: &str = "s1";
/// A second seat, with `files.example.net` only.
pub const OTHER_SEAT: &str = "s2";
/// An Anthropic Account (`https://api.anthropic.com`, port 443).
pub const ANTHROPIC: &str = "anthropic-main";
/// An owner-typed OpenAI-compatible Account on this Mac (`http://127.0.0.1:8080`).
pub const LLAMA: &str = "llama";
/// An MCP server approved at `http://127.0.0.1:3001/mcp`.
pub const MCP: &str = "everything";
/// An Account whose key a seat's request may reference (a keyed search API).
pub const SEARCH_KEY: &str = "search-key";
pub const LOCAL_LMSTUDIO: &str = "local-lmstudio";
pub const LOCAL_OLLAMA: &str = "local-ollama";

/// The keys in the fake keychain: canaries, one per Account.
pub const ANTHROPIC_CANARY: &str = "sk-ant-egress-canary-7d41";
pub const LLAMA_CANARY: &str = "sk-llama-egress-canary-0b9e";
pub const SEARCH_CANARY: &str = "sk-search-egress-canary-55c2";

/// Public addresses the scripted resolver answers (none is private,
/// loopback, documentation or otherwise blocked).
pub const DOCS_IP: &str = "93.184.216.34";
pub const CDN_IP: &str = "93.184.216.35";
pub const FILES_IP: &str = "93.184.216.36";
pub const OTHER_IP: &str = "93.184.216.37";
pub const ANTHROPIC_IP: &str = "160.79.104.10";

pub fn endpoint(scheme: &str, host: &str, port: u16) -> Endpoint {
    Endpoint {
        scheme: scheme.into(),
        host: host.into(),
        port,
    }
}

// ------------------------------------------------------------ directory

/// A directory built by hand: what each seat, Account and MCP server may
/// reach. An id it does not hold is unknown (`None`); the reserved local
/// Accounts answer from `am_domain::net::reserved_local_account`, the one
/// place their endpoints are defined.
#[derive(Default)]
pub struct Directory {
    seats: HashMap<String, Vec<HostPattern>>,
    accounts: HashMap<String, Endpoint>,
    mcp: HashMap<String, Endpoint>,
    /// The app's own serving port, as the project's runtime folder records
    /// it (story 2.2 review): refused to every Account and MCP server.
    own_port: Option<u16>,
}

impl Directory {
    pub fn empty() -> Directory {
        Directory::default()
    }

    /// The directory most tests use.
    pub fn standard() -> Directory {
        Directory::empty()
            .seat(SEAT, &["docs.example.org", "*.example.org"])
            .seat(OTHER_SEAT, &["files.example.net"])
            .account(ANTHROPIC, endpoint("https", "api.anthropic.com", 443))
            .account(LLAMA, endpoint("http", "127.0.0.1", 8080))
            .mcp(MCP, endpoint("http", "127.0.0.1", 3001))
    }

    pub fn seat(mut self, id: &str, patterns: &[&str]) -> Directory {
        let parsed = patterns
            .iter()
            .map(|p| HostPattern::parse(p).expect("a valid host pattern"))
            .collect();
        self.seats.insert(id.into(), parsed);
        self
    }

    pub fn account(mut self, id: &str, at: Endpoint) -> Directory {
        self.accounts.insert(id.into(), at);
        self
    }

    pub fn mcp(mut self, id: &str, at: Endpoint) -> Directory {
        self.mcp.insert(id.into(), at);
        self
    }

    pub fn own_port(mut self, port: u16) -> Directory {
        self.own_port = Some(port);
        self
    }
}

impl EgressDirectory for Directory {
    fn seat_hosts(&self, seat: &str) -> Option<Vec<HostPattern>> {
        self.seats.get(seat).cloned()
    }

    fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
        self.accounts
            .get(account)
            .cloned()
            .or_else(|| reserved_local_account(account))
    }

    fn mcp_endpoint(&self, server: &str) -> Option<Endpoint> {
        self.mcp.get(server).cloned()
    }

    fn own_ports(&self) -> Vec<u16> {
        self.own_port.into_iter().collect()
    }
}

// ------------------------------------------------------------- resolver

/// Answers each host from a script: the answers queued for it in order,
/// the last one repeating. Counts every lookup by host.
#[derive(Default)]
pub struct ScriptedResolver {
    answers: Mutex<HashMap<String, VecDeque<Vec<IpAddr>>>>,
    lookups: Mutex<Vec<String>>,
}

impl ScriptedResolver {
    pub fn new() -> Arc<ScriptedResolver> {
        Arc::new(ScriptedResolver::default())
    }

    /// The resolver most tests use: every listed name answers a public
    /// address; `internal.example.org` answers a private one.
    pub fn standard() -> Arc<ScriptedResolver> {
        let r = ScriptedResolver::new();
        r.answer("docs.example.org", &[DOCS_IP]);
        r.answer("cdn.example.org", &[CDN_IP]);
        r.answer("files.example.net", &[FILES_IP]);
        r.answer("other.example.net", &[OTHER_IP]);
        r.answer("api.anthropic.com", &[ANTHROPIC_IP]);
        r.answer("internal.example.org", &["10.1.2.3"]);
        r
    }

    /// Queue one answer for `host` (several addresses at once).
    pub fn answer(&self, host: &str, ips: &[&str]) {
        let ips = ips
            .iter()
            .map(|ip| ip.parse::<IpAddr>().expect("an IP address"))
            .collect();
        self.answers
            .lock()
            .unwrap()
            .entry(host.into())
            .or_default()
            .push_back(ips);
    }

    /// Replace every queued answer for `host`.
    pub fn reset(&self, host: &str) {
        self.answers.lock().unwrap().remove(host);
    }

    pub fn lookups(&self, host: &str) -> usize {
        self.lookups
            .lock()
            .unwrap()
            .iter()
            .filter(|h| *h == host)
            .count()
    }

    pub fn all_lookups(&self) -> Vec<String> {
        self.lookups.lock().unwrap().clone()
    }
}

impl Resolver for ScriptedResolver {
    fn lookup(&self, host: &str, _port: u16) -> BoxFuture<Result<Vec<IpAddr>, EgressError>> {
        self.lookups.lock().unwrap().push(host.to_string());
        let answer = {
            let mut all = self.answers.lock().unwrap();
            match all.get_mut(host) {
                Some(queue) if queue.len() > 1 => queue.pop_front(),
                Some(queue) => queue.front().cloned(),
                None => None,
            }
        };
        let host = host.to_string();
        Box::pin(async move {
            answer.ok_or_else(|| EgressError::Network(format!("no such host {host}")))
        })
    }
}

// ------------------------------------------------------------ transport

/// What a scripted reply sends back.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub chunks: Vec<Vec<u8>>,
    /// The response head arrives after this long.
    pub head_delay: Duration,
    /// After its chunks, the body sends nothing more and never ends.
    pub stalls: bool,
}

impl Reply {
    pub fn ok(body: &str) -> Reply {
        Reply {
            status: 200,
            headers: vec![("content-type".into(), "text/plain".into())],
            chunks: vec![body.as_bytes().to_vec()],
            head_delay: Duration::ZERO,
            stalls: false,
        }
    }

    /// A 200 whose head arrives only after `delay`.
    pub fn late(delay: Duration) -> Reply {
        Reply {
            head_delay: delay,
            ..Reply::ok("late")
        }
    }

    /// A 200 that sends `chunks`, then nothing more, never ending.
    pub fn stalling(chunks: &[&[u8]]) -> Reply {
        Reply {
            stalls: true,
            ..Reply::chunks(chunks)
        }
    }

    pub fn redirect(status: u16, location: &str) -> Reply {
        Reply {
            status,
            headers: vec![("location".into(), location.into())],
            chunks: Vec::new(),
            head_delay: Duration::ZERO,
            stalls: false,
        }
    }

    /// A 200 whose body arrives in these chunks (no content-length).
    pub fn chunks(chunks: &[&[u8]]) -> Reply {
        Reply {
            status: 200,
            headers: vec![("content-type".into(), "application/octet-stream".into())],
            chunks: chunks.iter().map(|c| c.to_vec()).collect(),
            head_delay: Duration::ZERO,
            stalls: false,
        }
    }
}

/// One hop as the transport saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenHop {
    pub method: HttpMethod,
    pub url: String,
    pub addr: SocketAddr,
    /// Header names lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl SeenHop {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    /// True when the hop carries any credential-like header.
    pub fn carries_a_credential(&self) -> bool {
        self.headers.iter().any(|(k, _)| {
            [
                "authorization",
                "proxy-authorization",
                "x-api-key",
                "cookie",
            ]
            .contains(&k.as_str())
        })
    }
}

/// Answers each hop by its exact URL (a 200 "ok" when none is scripted)
/// and records every hop it was asked to make.
#[derive(Default)]
pub struct ScriptedTransport {
    replies: Mutex<HashMap<String, Reply>>,
    hops: Mutex<Vec<SeenHop>>,
}

impl ScriptedTransport {
    pub fn new() -> Arc<ScriptedTransport> {
        Arc::new(ScriptedTransport::default())
    }

    pub fn on(&self, url: &str, reply: Reply) {
        self.replies.lock().unwrap().insert(url.into(), reply);
    }

    pub fn hops(&self) -> Vec<SeenHop> {
        self.hops.lock().unwrap().clone()
    }
}

impl Transport for ScriptedTransport {
    fn send(&self, hop: Hop) -> BoxFuture<Result<HttpResponse, EgressError>> {
        // The credential travels apart from the headers (a `SecretString`
        // until the transport builds its header); seen here as the header
        // it becomes.
        let mut headers: Vec<(String, String)> = hop
            .headers
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
            .collect();
        if let Some(credential) = &hop.credential {
            headers.push((
                credential.header_name().to_string(),
                credential.header_value().expose().to_string(),
            ));
        }
        let seen = SeenHop {
            method: hop.method,
            url: hop.url.clone(),
            addr: hop.addr,
            headers,
            body: hop.body.clone(),
        };
        self.hops.lock().unwrap().push(seen);
        let reply = self
            .replies
            .lock()
            .unwrap()
            .get(&hop.url)
            .cloned()
            .unwrap_or_else(|| Reply::ok("ok"));
        Box::pin(async move {
            if !reply.head_delay.is_zero() {
                tokio::time::sleep(reply.head_delay).await;
            }
            Ok(HttpResponse {
                status: reply.status,
                headers: reply.headers,
                body: Box::new(Chunks {
                    chunks: Mutex::new(reply.chunks.into()),
                    stalls: reply.stalls,
                }),
            })
        })
    }
}

/// A response body served from memory, chunk by chunk; a stalling one
/// never ends after its chunks.
pub struct Chunks {
    chunks: Mutex<VecDeque<Vec<u8>>>,
    stalls: bool,
}

impl BodyChunks for Chunks {
    fn next(&self) -> BoxFuture<Option<Result<Vec<u8>, EgressError>>> {
        let chunk = self.chunks.lock().unwrap().pop_front();
        let stalls = self.stalls;
        Box::pin(async move {
            match chunk {
                Some(c) => Some(Ok(c)),
                None if stalls => std::future::pending().await,
                None => None,
            }
        })
    }
}

// ------------------------------------------------------------------ rig

/// Limits large enough for every test but the size-cap ones.
pub fn roomy() -> Limits {
    Limits {
        max_response_bytes: 1 << 20,
        max_request_bytes: 1 << 20,
        ..Limits::default()
    }
}

/// The client under test with its fakes.
pub struct Rig {
    pub egress: NetEgress,
    /// The fake keychain, counting every read.
    pub secrets: Arc<CountingKeychain>,
    pub resolver: Arc<ScriptedResolver>,
    pub transport: Arc<ScriptedTransport>,
    pub audit: Arc<CollectAudit>,
}

/// A keychain that counts every read (a key read for a request the client
/// refuses is a leak waiting to happen).
pub struct CountingKeychain {
    inner: Arc<am_testkit::FakeSecretStore>,
    reads: AtomicUsize,
}

impl CountingKeychain {
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl SecretStore for CountingKeychain {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(account)
    }
    fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError> {
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.inner.delete(account)
    }
}

/// The fake keychain: one canary key per Account that has one.
pub fn keychain() -> Arc<am_testkit::FakeSecretStore> {
    let secrets = am_testkit::FakeSecretStore::new();
    for (account, key) in [
        (ANTHROPIC, ANTHROPIC_CANARY),
        (LLAMA, LLAMA_CANARY),
        (SEARCH_KEY, SEARCH_CANARY),
    ] {
        secrets
            .set(account, SecretString::new(key.to_string()))
            .unwrap();
    }
    secrets
}

impl Rig {
    /// The standard directory, resolver and roomy limits.
    pub fn standard() -> Rig {
        Rig::build(Directory::standard(), roomy(), None)
    }

    pub fn with(directory: Directory, limits: Limits) -> Rig {
        Rig::build(directory, limits, None)
    }

    /// `audit`: another sink than the collecting one (the store-backed
    /// sink of am-app); `self.audit` then stays empty.
    pub fn build(directory: Directory, limits: Limits, audit: Option<Arc<dyn AuditSink>>) -> Rig {
        let resolver = ScriptedResolver::standard();
        let transport = ScriptedTransport::new();
        let collect = CollectAudit::new();
        let sink: Arc<dyn AuditSink> = audit.unwrap_or_else(|| collect.clone());
        let secrets = Arc::new(CountingKeychain {
            inner: keychain(),
            reads: AtomicUsize::new(0),
        });
        let egress = NetEgress::with_seams(
            Arc::new(directory),
            secrets.clone(),
            sink,
            Seams {
                resolver: resolver.clone(),
                transport: transport.clone(),
                limits,
            },
        );
        Rig {
            egress,
            secrets,
            resolver,
            transport,
            audit: collect,
        }
    }

    pub async fn send(&self, request: HttpRequest) -> Result<HttpResponse, EgressError> {
        self.egress.send(request).await
    }

    /// Every `egress.blocked` entry so far.
    pub fn blocks(&self) -> Vec<AuditEvent> {
        self.audit
            .events()
            .into_iter()
            .filter(|e| e.action == "egress.blocked")
            .collect()
    }
}

// ------------------------------------------------------------- requests

pub fn seat(seat: &str) -> Option<Caller> {
    Some(Caller::Seat { seat: seat.into() })
}

pub fn account(account: &str) -> Option<Caller> {
    Some(Caller::Account {
        account: account.into(),
        seat: None,
    })
}

pub fn mcp(server: &str) -> Option<Caller> {
    Some(Caller::McpServer {
        server: server.into(),
    })
}

/// GET `url` for `caller`.
pub fn get(caller: Option<Caller>, url: &str) -> HttpRequest {
    HttpRequest {
        caller,
        ..HttpRequest::get(url)
    }
}

/// POST a small JSON body to `url` for `caller`.
pub fn post(caller: Option<Caller>, url: &str) -> HttpRequest {
    HttpRequest {
        caller,
        ..HttpRequest::post_json(url, b"{\"probe\":1}".to_vec())
    }
}

/// A request for an Account carrying a reference to its key.
pub fn keyed(mut request: HttpRequest, scheme: AuthScheme, account_id: &str) -> HttpRequest {
    request.auth = Some(AuthRef {
        scheme,
        account_id: account_id.into(),
    });
    request
}

// ----------------------------------------------------------- assertions

/// The refusal's words; panics unless the result is `Blocked`.
pub fn blocked(result: Result<HttpResponse, EgressError>, what: &str) -> String {
    match result {
        Err(EgressError::Blocked(words)) => {
            assert!(!words.trim().is_empty(), "{what}: the refusal is worded");
            words
        }
        Err(other) => panic!("{what}: expected Blocked, got {other:?}"),
        Ok(resp) => panic!("{what}: expected Blocked, got HTTP {}", resp.status),
    }
}

/// The response; panics on an error.
pub fn allowed(result: Result<HttpResponse, EgressError>, what: &str) -> HttpResponse {
    match result {
        Ok(resp) => resp,
        Err(e) => panic!("{what}: expected the request to be allowed, got {e:?}"),
    }
}

/// The text of a whole body.
pub async fn body_text(resp: HttpResponse) -> String {
    let bytes = am_net::read_all(resp.body.as_ref())
        .await
        .expect("the body is read");
    String::from_utf8(bytes).expect("a text body")
}

/// The args of an audit entry, by key.
pub fn args(event: &AuditEvent) -> &BTreeMap<String, String> {
    &event.args
}

/// One `egress.blocked` entry by `System(am-net)` whose args hold every
/// `(key, value)` in `expect`, a non-empty `reason`, and the destination's
/// `scheme`, `host` and `port`.
pub fn assert_block(event: &AuditEvent, expect: &[(&str, &str)], what: &str) {
    assert_eq!(
        event.action, "egress.blocked",
        "{what}: the action: {event:?}"
    );
    assert_eq!(
        event.actor,
        Actor::System {
            component: "am-net".into()
        },
        "{what}: the actor: {event:?}"
    );
    let args = args(event);
    for key in ["scheme", "host", "port", "reason"] {
        assert!(
            args.get(key).is_some_and(|v| !v.is_empty()),
            "{what}: the entry records its {key}: {event:?}"
        );
    }
    for (key, value) in expect {
        assert_eq!(
            args.get(*key).map(String::as_str),
            Some(*value),
            "{what}: args[{key}]: {event:?}"
        );
    }
}

/// The last `egress.blocked` entry, checked as [`assert_block`] does.
pub fn assert_last_block(rig: &Rig, expect: &[(&str, &str)], what: &str) {
    let blocks = rig.blocks();
    let last = blocks
        .last()
        .unwrap_or_else(|| panic!("{what}: the refusal is audited; no egress.blocked entry"));
    assert_block(last, expect, what);
}

// ------------------------------------------------------ loopback server

/// A plain HTTP/1.1 server on 127.0.0.1 at a free port: it answers every
/// request `200` with `body` and records each request's head (request
/// line and headers) as text.
pub struct LoopbackServer {
    pub port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl LoopbackServer {
    pub async fn start(body: &'static str) -> LoopbackServer {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = heads.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    seen.lock().unwrap().push(head);
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(reply.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        LoopbackServer { port, heads }
    }

    /// The heads of every request received, lowercased.
    pub fn heads(&self) -> Vec<String> {
        self.heads
            .lock()
            .unwrap()
            .iter()
            .map(|h| h.to_ascii_lowercase())
            .collect()
    }
}
