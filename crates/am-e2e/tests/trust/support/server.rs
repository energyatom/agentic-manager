//! Shared rig of the RULE-15 trust tests (story 3.2, AD-13, AD-14, AD-15):
//! a real `am-server` bound on 127.0.0.1 with a fake clock, the test-only
//! UI-class fixture route (added through `router_with_fixture`, a
//! constructor parameter, never a feature, env var or flag), and, when a
//! test asks for one, a project store holding runs. Requests are raw
//! HTTP/1.1 over a loopback socket, so every header (Host, Origin, Cookie)
//! is exactly what the test wrote; the WebSocket client is a minimal one
//! over the same socket. No network beyond 127.0.0.1.
//!
//! Seeded from the spike's `tests/spike/c6_server.rs`.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use am_api::{ApiError, EventDto};
use am_domain::{Actor, Clock, RuntimeLayout};
use am_server::{AppState, FixtureRoute, bind, router_with_fixture, serve_router};
use am_store::{Store, WriteCommand};
use am_testkit::FakeClock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;

/// The fake clock's start.
pub const T0: &str = "2026-11-23T10:00:00Z";
/// A foreign page's origin.
pub const EVIL: &str = "http://evil.example";
/// The test-only UI-class state change (`FixtureRoute`).
pub const FIXTURE: &str = "/api/v1/fixture";
pub const SESSION: &str = "/api/v1/session";
pub const PAIR: &str = "/api/v1/pair";
pub const RUNS: &str = "/api/v1/runs";
pub const EVENTS: &str = "/api/v1/events";
pub const HEALTH: &str = "/api/v1/health";
/// The CLI token's header (plan decision 1).
pub const TOKEN_HEADER: &str = "X-AM-CLI-Token";
/// The fields `am_api::ApiError` may carry (AD-14).
pub const ENVELOPE_FIELDS: [&str; 4] = ["code", "message", "fix", "details"];

pub fn secs(n: i64) -> jiff::SignedDuration {
    jiff::SignedDuration::from_secs(n)
}

// ------------------------------------------------------------- raw HTTP

#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    /// The status line and headers as sent.
    pub head: String,
    pub body: String,
}

impl Resp {
    pub fn head_lower(&self) -> String {
        self.head.to_ascii_lowercase()
    }

    /// Every value of header `name` (case-insensitive name).
    pub fn header(&self, name: &str) -> Vec<String> {
        let name = name.to_ascii_lowercase();
        self.head
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .filter(|(k, _)| k.trim().to_ascii_lowercase() == name)
            .map(|(_, v)| v.trim().to_string())
            .collect()
    }

    /// The whole response, for "no secret here" checks.
    pub fn text(&self) -> String {
        format!("{}\r\n\r\n{}", self.head, self.body)
    }
}

/// Builds a request. A `("Host", "")` entry means "send no Host header".
pub fn req(method: &str, path: &str, headers: &[(&str, String)], body: &str) -> String {
    let mut s = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("host") && v.is_empty() {
            continue;
        }
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || method == "POST" || method == "PUT" {
        s.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    s.push_str("\r\n");
    s.push_str(body);
    s
}

fn parse(text: &str) -> Resp {
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Resp {
        status,
        head: head.to_string(),
        body: body.to_string(),
    }
}

/// Sends `request` to 127.0.0.1:`port` and reads until the server closes.
pub async fn raw(port: u16, request: String) -> Resp {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
    parse(&String::from_utf8_lossy(&buf))
}

pub fn has_host(headers: &[(&str, String)]) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host"))
}

// ------------------------------------------------------------- envelope

/// No `Access-Control-*` header on any response (AD-13).
pub fn assert_no_cors(r: &Resp) {
    assert!(
        !r.head_lower().contains("access-control-"),
        "CORS header sent:\n{}",
        r.head
    );
}

/// The response is `status` with the one error envelope (AD-14): JSON
/// `{code, message, fix?, details?}`, `code` the stable kebab-case `code`,
/// `message` plain words, no other field, no CORS header. Returns it.
pub fn envelope(r: &Resp, status: u16, code: &str) -> ApiError {
    assert_eq!(r.status, status, "{}", r.text());
    assert_no_cors(r);
    let ct = r.header("content-type").join(",").to_ascii_lowercase();
    assert!(
        ct.contains("application/json"),
        "an error is JSON, got content-type {ct:?}:\n{}",
        r.text()
    );
    let v: serde_json::Value = serde_json::from_str(&r.body)
        .unwrap_or_else(|e| panic!("the envelope is JSON ({e}):\n{}", r.text()));
    let obj = v.as_object().expect("the envelope is an object");
    for k in obj.keys() {
        assert!(
            ENVELOPE_FIELDS.contains(&k.as_str()),
            "unexpected envelope field {k:?}: {}",
            r.body
        );
    }
    let e: ApiError = serde_json::from_value(v.clone()).expect("an am_api::ApiError");
    assert_eq!(e.code, code, "{}", r.body);
    assert!(
        e.code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "code is kebab-case: {}",
        e.code
    );
    assert!(!e.message.trim().is_empty(), "message in words: {}", r.body);
    e
}

/// An error envelope with this status, whatever its code.
pub fn any_envelope(r: &Resp, status: u16) -> ApiError {
    let v: serde_json::Value = serde_json::from_str(&r.body)
        .unwrap_or_else(|e| panic!("the envelope is JSON ({e}):\n{}", r.text()));
    let code = v["code"].as_str().unwrap_or("").to_string();
    envelope(r, status, &code)
}

/// None of `secrets` appears in `text` (`what` names the place).
pub fn assert_free_of(what: &str, text: &str, secrets: &[String]) {
    for s in secrets {
        assert!(!s.is_empty(), "empty secret in the list");
        assert!(
            !text.contains(s.as_str()),
            "a secret appears in {what}:\n{text}"
        );
    }
}

// ------------------------------------------------------------- the rig

/// One browser session: the `am_s_<port>` cookie and its CSRF value.
#[derive(Debug, Clone)]
pub struct Session {
    pub sid: String,
    pub cookie: String,
    pub csrf: String,
}

impl Session {
    pub fn cookie_only(&self) -> Vec<(&'static str, String)> {
        vec![("Cookie", self.cookie.clone())]
    }

    /// Cookie and `X-AM-CSRF`.
    pub fn auth(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Cookie", self.cookie.clone()),
            ("X-AM-CSRF", self.csrf.clone()),
        ]
    }

    /// The WebSocket subprotocol carrying the CSRF value.
    pub fn protocol(&self) -> String {
        format!("am.csrf.{}", self.csrf)
    }
}

pub struct Rig {
    pub state: AppState,
    pub port: u16,
    pub clock: Arc<FakeClock>,
    pub fixture: FixtureRoute,
    /// The open project's store (when started with one).
    pub store: Option<Store>,
    pub runtime: Option<RuntimeLayout>,
    pub dir: tempfile::TempDir,
}

pub struct Options {
    pub project: bool,
    pub event_window: Option<usize>,
}

impl Rig {
    /// No project, the default event window.
    pub async fn start() -> Rig {
        Rig::start_with(Options {
            project: false,
            event_window: None,
        })
        .await
    }

    /// With an open project (an empty store in a 0700 runtime folder).
    pub async fn with_project() -> Rig {
        Rig::start_with(Options {
            project: true,
            event_window: None,
        })
        .await
    }

    pub async fn start_with(opts: Options) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let clock = FakeClock::at(T0);
        let (listener, port) = bind(0).await.unwrap();
        let mut builder = AppState::builder(port).clock(clock.clone() as Arc<dyn Clock>);
        if let Some(n) = opts.event_window {
            builder = builder.event_window(n);
        }
        let (store, runtime) = if opts.project {
            let rt = dir.path().join("rt");
            private_dir(&rt);
            let layout = RuntimeLayout::at(rt);
            let store = Store::open(&layout.state_db()).unwrap();
            builder = builder.project_db(layout.state_db());
            (Some(store), Some(layout))
        } else {
            (None, None)
        };
        let state = builder.build();
        let fixture = FixtureRoute::new();
        let router = router_with_fixture(state.clone(), fixture.clone());
        tokio::spawn(async move {
            let _ = serve_router(listener, router, std::future::pending()).await;
        });
        Rig {
            state,
            port,
            clock,
            fixture,
            store,
            runtime,
            dir,
        }
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn token(&self) -> String {
        self.state.cli_token().expose().to_string()
    }

    /// Sends a request; `Host: 127.0.0.1:<port>` is added unless the
    /// headers name a Host (`("Host", "")` sends none).
    pub async fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &str,
    ) -> Resp {
        let mut h: Vec<(&str, String)> = Vec::new();
        if !has_host(headers) {
            h.push(("Host", self.host()));
        }
        h.extend(headers.iter().cloned());
        raw(self.port, req(method, path, &h, body)).await
    }

    pub async fn get(&self, path: &str, headers: &[(&str, String)]) -> Resp {
        self.send("GET", path, headers, "").await
    }

    /// POST `{"code": code}` to an exchange (`SESSION` or `PAIR`) with the
    /// served Origin.
    pub async fn exchange(&self, path: &str, code: &str) -> Resp {
        self.send(
            "POST",
            path,
            &[("Origin", self.origin())],
            &format!(r#"{{"code":"{code}"}}"#),
        )
        .await
    }

    /// A launch code exchanged for a session.
    pub async fn sign_in(&self) -> Session {
        let code = self.state.issue_launch_code();
        let r = self.exchange(SESSION, &code).await;
        assert_eq!(r.status, 200, "{}", r.text());
        self.session_of(&r)
    }

    /// The session a successful exchange set.
    pub fn session_of(&self, r: &Resp) -> Session {
        let name = format!("am_s_{}", self.port);
        let cookies = r.header("set-cookie");
        assert_eq!(cookies.len(), 1, "one cookie: {cookies:?}");
        let (k, rest) = cookies[0].split_once('=').expect("name=value");
        assert_eq!(k.trim(), name);
        let sid = rest.split(';').next().unwrap().trim().to_string();
        let body: serde_json::Value = serde_json::from_str(&r.body).expect("JSON body");
        let csrf = body["csrf"].as_str().expect("csrf in the body").to_string();
        Session {
            cookie: format!("{name}={sid}"),
            sid,
            csrf,
        }
    }

    /// POST to the fixture route with a full browser session.
    pub async fn mutate(&self, s: &Session) -> Resp {
        let mut h = s.auth();
        h.push(("Origin", self.origin()));
        self.send("POST", FIXTURE, &h, "{}").await
    }

    /// A run with `steps` queued in the open project; returns its id.
    pub fn create_run(&self, steps: &[&str]) -> String {
        let id = am_store::new_id();
        self.store
            .as_ref()
            .expect("started with a project")
            .writer()
            .exec(
                system(),
                WriteCommand::CreateRun {
                    run_id: id.clone(),
                    workflow: "wf-test".into(),
                    origin: "manual".into(),
                    steps: steps.iter().map(|s| s.to_string()).collect(),
                    config_hash: None,
                },
            )
            .unwrap();
        id
    }

    pub fn exec(&self, cmd: WriteCommand) {
        self.store
            .as_ref()
            .expect("started with a project")
            .writer()
            .exec(system(), cmd)
            .unwrap();
    }

    /// Opens the event stream with the browser session (served Origin,
    /// cookie, CSRF subprotocol).
    pub async fn events_with_session(&self, s: &Session, since: Option<u64>) -> Ws {
        let path = match since {
            Some(n) => format!("{EVENTS}?since={n}"),
            None => EVENTS.to_string(),
        };
        let h = ws_headers(
            &self.host(),
            Some(&self.origin()),
            Some(&s.cookie),
            Some(&s.protocol()),
        );
        upgrade(self.port, &path, &h)
            .await
            .unwrap_or_else(|r| panic!("upgrade refused:\n{}", r.text()))
    }
}

pub fn system() -> Actor {
    Actor::System {
        component: "test".into(),
    }
}

/// A 0700 folder.
pub fn private_dir(p: &std::path::Path) {
    std::fs::create_dir_all(p).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

/// Every file under `root`, recursively.
pub fn files_under(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// The 8-character RFC 4648 base32 words of `text` (pairing codes).
pub fn base32_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| {
            w.len() == 8
                && w.chars()
                    .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))
        })
        .map(str::to_string)
        .collect()
}

// ------------------------------------------------------------- WebSocket

/// Upgrade headers: Host, optional Origin, Cookie and subprotocol.
pub fn ws_headers(
    host: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
    protocol: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut h = vec![("Host", host.to_string())];
    if let Some(o) = origin {
        h.push(("Origin", o.to_string()));
    }
    if let Some(c) = cookie {
        h.push(("Cookie", c.to_string()));
    }
    h.push(("Upgrade", "websocket".to_string()));
    h.push(("Sec-WebSocket-Version", "13".to_string()));
    h.push(("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==".to_string()));
    if let Some(p) = protocol {
        h.push(("Sec-WebSocket-Protocol", p.to_string()));
    }
    h
}

/// A connected event stream.
pub struct Ws {
    stream: TcpStream,
    buf: Vec<u8>,
    /// The 101 response's head.
    pub head: String,
}

/// Sends the upgrade; `Ok` on `101`, else the refusal (read whole).
pub async fn upgrade(port: u16, path: &str, headers: &[(&str, String)]) -> Result<Ws, Resp> {
    let mut request = req("GET", path, headers, "");
    request = request.replacen("Connection: close", "Connection: Upgrade", 1);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let split = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        match tokio::time::timeout_at(deadline, stream.read(&mut tmp)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => {
                return Err(parse(&String::from_utf8_lossy(&buf)));
            }
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let rest = buf[split + 4..].to_vec();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if status == 101 {
        return Ok(Ws {
            stream,
            buf: rest,
            head,
        });
    }
    // A refusal: read its body (by Content-Length, or until close).
    let probe = parse(&format!("{head}\r\n\r\n"));
    let len: Option<usize> = probe
        .header("content-length")
        .first()
        .and_then(|v| v.parse().ok());
    let mut body = rest;
    let deadline = Instant::now() + Duration::from_secs(2);
    while len.is_none_or(|l| body.len() < l) {
        match tokio::time::timeout_at(deadline, stream.read(&mut tmp)).await {
            Ok(Ok(n)) if n > 0 => body.extend_from_slice(&tmp[..n]),
            _ => break,
        }
    }
    Err(Resp {
        status,
        head,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

impl Ws {
    async fn fill(&mut self, n: usize, deadline: Instant) -> bool {
        let mut tmp = [0u8; 4096];
        while self.buf.len() < n {
            match tokio::time::timeout_at(deadline, self.stream.read(&mut tmp)).await {
                Ok(Ok(k)) if k > 0 => self.buf.extend_from_slice(&tmp[..k]),
                _ => return false,
            }
        }
        true
    }

    /// The next frame (opcode, payload) within `wait`.
    pub async fn next_frame(&mut self, wait: Duration) -> Option<(u8, Vec<u8>)> {
        let deadline = Instant::now() + wait;
        if !self.fill(2, deadline).await {
            return None;
        }
        let op = self.buf[0] & 0x0f;
        let masked = self.buf[1] & 0x80 != 0;
        let mut len = (self.buf[1] & 0x7f) as usize;
        let mut off = 2;
        if len == 126 {
            if !self.fill(4, deadline).await {
                return None;
            }
            len = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
            off = 4;
        } else if len == 127 {
            if !self.fill(10, deadline).await {
                return None;
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.buf[2..10]);
            len = u64::from_be_bytes(b) as usize;
            off = 10;
        }
        let mask_at = off;
        if masked {
            off += 4;
        }
        if !self.fill(off + len, deadline).await {
            return None;
        }
        let mut payload = self.buf[off..off + len].to_vec();
        if masked {
            let key = [
                self.buf[mask_at],
                self.buf[mask_at + 1],
                self.buf[mask_at + 2],
                self.buf[mask_at + 3],
            ];
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= key[i % 4];
            }
        }
        self.buf.drain(..off + len);
        Some((op, payload))
    }

    /// The next event (text frame) within `wait`; `None` on close or
    /// timeout. Pings are answered.
    pub async fn next_event(&mut self, wait: Duration) -> Option<EventDto> {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let (op, payload) = self.next_frame(left).await?;
            match op {
                0x1 => {
                    let text = String::from_utf8_lossy(&payload).to_string();
                    return Some(
                        serde_json::from_str::<EventDto>(&text)
                            .unwrap_or_else(|e| panic!("an am_api::EventDto ({e}): {text}")),
                    );
                }
                0x8 => return None,
                0x9 => {
                    let mut f = vec![0x8a, 0x80 | payload.len() as u8, 0, 0, 0, 0];
                    f.extend_from_slice(&payload);
                    let _ = self.stream.write_all(&f).await;
                }
                _ => {}
            }
        }
    }

    /// The next `n` events, each within `wait`.
    pub async fn take(&mut self, n: usize, wait: Duration) -> Vec<EventDto> {
        let mut out = Vec::new();
        for _ in 0..n {
            match self.next_event(wait).await {
                Some(e) => out.push(e),
                None => break,
            }
        }
        out
    }

    /// Sends one text frame, masked as a client's must be (story 3.4: a
    /// client frame resolves nothing).
    pub async fn send_text(&mut self, text: &str) {
        let payload = text.as_bytes();
        let mut frame = vec![0x81];
        if payload.len() < 126 {
            frame.push(0x80 | payload.len() as u8);
        } else {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        let key = [0x12, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&key);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        let _ = self.stream.write_all(&frame).await;
    }

    /// Sends a close frame and drops the connection.
    pub async fn close(mut self) {
        let _ = self.stream.write_all(&[0x88, 0x80, 0, 0, 0, 0]).await;
        let _ = self.stream.shutdown().await;
    }
}
