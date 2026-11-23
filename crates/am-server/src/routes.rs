//! The route table (AD-15): every route is declared once, here, with its
//! class and whether it changes state, and the router is built from it, so
//! the guard and the router cannot disagree. A state-changing route is
//! POST, PUT or DELETE, never GET or HEAD.

use std::sync::Arc;

use am_api::{PairBody, SessionBody};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{FromRequestParts, Request, State};
use axum::handler::Handler;
use axum::http::{StatusCode, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get, post};

use crate::envelope;
use crate::events;
use crate::guard::{self, Authed, Guard};
use crate::pairing::Attempt;
use crate::runs;
use crate::state::AppState;
use crate::{EVENTS_PATH, WS_PROTOCOL_PREFIX};

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_JS: &str = include_str!("../../../web/app.js");

/// Who may reach a route (AD-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    /// The page and its script: Host and Origin checks only.
    Page,
    /// The pre-session sign-in exchanges (`session`, `pair`): a matching
    /// Origin, no session.
    Exchange,
    /// The browser session only (cookie plus CSRF value).
    UiClass,
    /// The browser session, or the CLI token on a request with no Origin.
    CliClass,
}

/// One declared route: its method, its path (axum syntax), its class and
/// whether it changes state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSpec {
    pub method: String,
    pub path: String,
    pub class: RouteClass,
    pub changes_state: bool,
}

impl RouteSpec {
    pub(crate) fn new(method: &str, path: &str, class: RouteClass, changes_state: bool) -> Self {
        RouteSpec {
            method: method.to_string(),
            path: path.to_string(),
            class,
            changes_state,
        }
    }

    /// Whether `path` (a request's) is this route's path; `{name}` matches
    /// one non-empty segment.
    pub(crate) fn matches_path(&self, path: &str) -> bool {
        let want: Vec<&str> = self.path.split('/').collect();
        let got: Vec<&str> = path.split('/').collect();
        want.len() == got.len()
            && want.iter().zip(&got).all(|(w, g)| {
                if w.starts_with('{') && w.ends_with('}') {
                    !g.is_empty()
                } else {
                    w == g
                }
            })
    }
}

/// A declared route with its handler.
pub(crate) struct Route {
    pub(crate) spec: RouteSpec,
    pub(crate) handler: MethodRouter<AppState>,
}

impl Route {
    /// A read (GET; axum answers HEAD from it too).
    fn read<H, T>(path: &str, class: RouteClass, h: H) -> Route
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        Route {
            spec: RouteSpec::new("GET", path, class, false),
            handler: get(h),
        }
    }

    /// A state change (POST).
    pub(crate) fn change<H, T>(path: &str, class: RouteClass, h: H) -> Route
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        Route {
            spec: RouteSpec::new("POST", path, class, true),
            handler: post(h),
        }
    }
}

/// The shipped routes with their handlers.
fn shipped() -> Vec<Route> {
    use RouteClass::{CliClass, Exchange, Page};
    vec![
        Route::read("/", Page, index),
        Route::read("/app.js", Page, app_js),
        Route::change("/api/v1/session", Exchange, session),
        Route::change("/api/v1/pair", Exchange, pair),
        Route::read("/api/v1/health", CliClass, health),
        Route::read("/api/v1/runs", CliClass, runs::list),
        Route::read("/api/v1/runs/{id}", CliClass, runs::one),
        Route::read(EVENTS_PATH, CliClass, event_stream),
    ]
}

/// The shipped route table.
pub fn route_table() -> Vec<RouteSpec> {
    shipped().into_iter().map(|r| r.spec).collect()
}

/// The shipped router.
pub fn router(state: AppState) -> Router {
    build(state, Vec::new())
}

/// The router of the shipped routes plus `extra` (the test-only fixture
/// route, through `router_with_fixture`), behind the one guard.
pub(crate) fn build(state: AppState, extra: Vec<Route>) -> Router {
    let mut all = shipped();
    all.extend(extra);
    let table: Arc<Vec<RouteSpec>> = Arc::new(all.iter().map(|r| r.spec.clone()).collect());
    let mut router = Router::new();
    for r in all {
        router = router.route(&r.spec.path, r.handler);
    }
    router
        .fallback(|| async { envelope::not_found() })
        .method_not_allowed_fallback(|| async { envelope::method_not_allowed() })
        .layer(middleware::from_fn_with_state(
            Guard {
                app: state.clone(),
                table,
            },
            guard::guard,
        ))
        .with_state(state)
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

async fn health() -> Response {
    envelope::json(StatusCode::OK, &serde_json::json!({ "ok": true }))
}

/// The session both exchanges open: the `am_s_<port>` cookie (HttpOnly,
/// SameSite=Strict, Path=/) and `{csrf}` in the body.
fn opened(state: &AppState) -> Response {
    let (sid, csrf) = state.open_session();
    tracing::info!(port = state.port(), "session opened");
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
        .unwrap_or_else(|_| envelope::internal())
}

const BAD_EXCHANGE: &str = "The body must be JSON {\"code\": \"<code>\"}.";

/// `POST /api/v1/session {code}`: the launch code for a session.
async fn session(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(SessionBody { code }) = serde_json::from_slice::<SessionBody>(&body) else {
        return envelope::bad_request(BAD_EXCHANGE);
    };
    if !state.take_launch_code(&code) {
        tracing::info!(port = state.port(), "a launch code exchange was refused");
        return envelope::launch_code_invalid();
    }
    opened(&state)
}

/// `POST /api/v1/pair {code}`: the terminal pairing code for a session.
/// Only an attempt with a well-formed body counts.
async fn pair(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(PairBody { code }) = serde_json::from_slice::<PairBody>(&body) else {
        return envelope::bad_request(BAD_EXCHANGE);
    };
    let now = state.now();
    let outcome = state.auth().pairing.attempt(&code, now);
    match outcome {
        Attempt::Paired => opened(&state),
        Attempt::Invalid => {
            tracing::info!(port = state.port(), "a pairing attempt was refused");
            envelope::pairing_code_invalid()
        }
        Attempt::RateLimited => envelope::pairing_rate_limited(),
        Attempt::Locked => envelope::pairing_locked(),
    }
}

/// `since=<seq>` from the query: `Ok(None)` when absent.
fn since(query: Option<&str>) -> Result<Option<u64>, ()> {
    let Some(q) = query else {
        return Ok(None);
    };
    for kv in q.split('&') {
        if let Some(v) = kv.strip_prefix("since=") {
            return v.parse::<u64>().map(Some).map_err(|_| ());
        }
    }
    Ok(None)
}

/// `GET /api/v1/events[?since=<seq>]`: the upgrade (the guard has checked
/// the session's subprotocol, or the CLI token).
async fn event_stream(State(state): State<AppState>, req: Request) -> Response {
    let Ok(since) = since(req.uri().query()) else {
        return envelope::bad_request("`since` must be a whole number (an event's seq).");
    };
    let authed = req.extensions().get::<Authed>().cloned();
    let (mut parts, _) = req.into_parts();
    let Ok(ws) = WebSocketUpgrade::from_request_parts(&mut parts, &state).await else {
        return envelope::upgrade_required();
    };
    let ws = match authed {
        Some(Authed::Session { csrf }) => ws.protocols([format!("{WS_PROTOCOL_PREFIX}{csrf}")]),
        Some(Authed::Token) => ws,
        None => return envelope::internal(),
    };
    let log = state.events();
    ws.on_upgrade(move |socket| events::stream(socket, log, since))
}
