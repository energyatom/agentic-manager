//! `am-server`: the loopback HTTP and WebSocket server (AD-13, AD-14), spike scope.
//!
//! - binds 127.0.0.1 only;
//! - every request: the Host must be `127.0.0.1:<port>` or `localhost:<port>`
//!   (DNS rebinding) and an Origin, when present, must be the served origin;
//!   state-changing requests and the WebSocket upgrade must carry an Origin;
//! - a single-use launch code (60 s, URL fragment) is exchanged for an
//!   HttpOnly, SameSite=Strict cookie named for the port (`am_s_<port>`) plus a
//!   per-session CSRF value returned in the body and held in page memory; a
//!   failed exchange invalidates every outstanding code;
//! - every `/api/v1` request needs the cookie and the `X-AM-CSRF` header; the
//!   WebSocket upgrade carries the CSRF value as a subprotocol (`am.csrf.<v>`);
//! - no CORS headers are ever sent.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tokio::net::TcpListener;

pub const LAUNCH_CODE_TTL: Duration = Duration::from_secs(60);
pub const CSRF_HEADER: &str = "x-am-csrf";
pub const WS_PROTOCOL_PREFIX: &str = "am.csrf.";

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_JS: &str = include_str!("../../../web/app.js");

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS randomness");
    hex::encode(buf)
}

#[derive(Default)]
struct Inner {
    launch_codes: HashMap<String, Instant>,
    /// session id -> CSRF value. In memory only; never written to disk.
    sessions: HashMap<String, String>,
}

#[derive(Clone)]
pub struct AppState {
    port: u16,
    extra_origins: Vec<String>,
    inner: Arc<Mutex<Inner>>,
}

impl AppState {
    pub fn new(port: u16) -> Self {
        AppState {
            port,
            extra_origins: Vec::new(),
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// Allow another origin (the Tauri origin in desktop builds).
    pub fn allow_origin(mut self, origin: &str) -> Self {
        self.extra_origins.push(origin.to_string());
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn cookie_name(&self) -> String {
        format!("am_s_{}", self.port)
    }

    /// Issue a single-use launch code valid for 60 s.
    pub fn issue_launch_code(&self) -> String {
        let code = random_hex(32);
        if let Ok(mut inner) = self.inner.lock() {
            inner.launch_codes.insert(code.clone(), Instant::now());
        }
        code
    }

    /// The URL the browser opens: the code travels only in the fragment.
    pub fn launch_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/#code={}",
            self.port,
            self.issue_launch_code()
        )
    }

    fn host_ok(&self, headers: &HeaderMap) -> bool {
        let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
            return false;
        };
        host == format!("127.0.0.1:{}", self.port) || host == format!("localhost:{}", self.port)
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        origin == format!("http://127.0.0.1:{}", self.port)
            || origin == format!("http://localhost:{}", self.port)
            || self.extra_origins.iter().any(|o| o == origin)
    }

    fn session_csrf(&self, headers: &HeaderMap) -> Option<String> {
        let cookie_name = self.cookie_name();
        let sid = headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|kv| kv.trim().split_once('='))
            .find(|(k, _)| *k == cookie_name)
            .map(|(_, v)| v.to_string())?;
        self.inner.lock().ok()?.sessions.get(&sid).cloned()
    }
}

fn forbidden(why: &'static str) -> Response {
    (StatusCode::FORBIDDEN, why).into_response()
}

/// Host and Origin checks on every request (AD-13).
async fn guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.host_ok(req.headers()) {
        return forbidden("host not allowed");
    }
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .map(str::to_string);
    let is_upgrade = req.headers().contains_key(header::UPGRADE);
    match origin {
        Some(o) if !state.origin_allowed(&o) => return forbidden("origin not allowed"),
        None if req.method() != Method::GET || is_upgrade => {
            return forbidden("origin required");
        }
        _ => {}
    }
    let path = req.uri().path().to_string();
    if path.starts_with("/api/v1/") && path != "/api/v1/session" && !is_upgrade {
        let Some(expected) = state.session_csrf(req.headers()) else {
            return (StatusCode::UNAUTHORIZED, "no session").into_response();
        };
        let presented = req
            .headers()
            .get(CSRF_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if presented != expected {
            return forbidden("csrf value missing or wrong");
        }
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&format!(
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self' ws://127.0.0.1:{p} ws://localhost:{p}; frame-ancestors 'none'",
            p = state.port
        ))
        .unwrap_or(HeaderValue::from_static("default-src 'self'")),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn index() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        INDEX_HTML,
    )
        .into_response()
}

async fn app_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP_JS,
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct Exchange {
    code: String,
}

async fn session(State(state): State<AppState>, body: axum::body::Bytes) -> Response {
    let code = serde_json::from_slice::<Exchange>(&body)
        .map(|e| e.code)
        .unwrap_or_default();
    let mut inner = match state.inner.lock() {
        Ok(i) => i,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let valid = inner
        .launch_codes
        .remove(&code)
        .map(|issued| issued.elapsed() <= LAUNCH_CODE_TTL)
        .unwrap_or(false);
    if !valid {
        // A failed exchange invalidates every outstanding code (AD-13).
        inner.launch_codes.clear();
        return (StatusCode::UNAUTHORIZED, "launch code invalid or expired").into_response();
    }
    let sid = random_hex(32);
    let csrf = random_hex(32);
    inner.sessions.insert(sid.clone(), csrf.clone());
    drop(inner);
    tracing::info!(port = state.port, "session opened");
    eprintln!("am-server: session opened on port {}", state.port);
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/",
        state.cookie_name(),
        sid
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({ "csrf": csrf }).to_string()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn health() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"ok":true}"#,
    )
        .into_response()
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(expected) = state.session_csrf(&headers) else {
        return (StatusCode::UNAUTHORIZED, "no session").into_response();
    };
    let wanted = format!("{WS_PROTOCOL_PREFIX}{expected}");
    let offered = ws
        .requested_protocols()
        .filter_map(|p| p.to_str().ok())
        .flat_map(|p| p.split(','))
        .any(|p| p.trim() == wanted);
    if !offered {
        return forbidden("csrf subprotocol missing or wrong");
    }
    ws.protocols([wanted]).on_upgrade(event_stream)
}

async fn event_stream(mut socket: WebSocket) {
    let hello = serde_json::json!({"seq": 1, "type": "server.hello"}).to_string();
    let _ = socket.send(Message::Text(hello.into())).await;
    while let Some(Ok(msg)) = socket.recv().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/api/v1/session", post(session))
        .route("/api/v1/health", get(health))
        .route("/api/v1/events", get(events))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Bind 127.0.0.1 (port 0 picks a free port).
pub async fn bind(port: u16) -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Serve until the future completes.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}
