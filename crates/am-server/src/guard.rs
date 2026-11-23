//! The one guard (AD-13, AD-15) on every request, the WebSocket upgrade
//! included, in this order:
//!
//! 1. Host exactly `127.0.0.1:<port>` or `localhost:<port>` (and an
//!    absolute-form target naming one of them), else 403
//!    `host-not-allowed` before anything else (DNS rebinding);
//! 2. an Origin, when present, must be the served origin, else 403
//!    `origin-not-allowed`;
//! 3. the route table: an unknown path is 404 `not-found`, a method not
//!    declared for it 405 `method-not-allowed` (HEAD only where GET is
//!    declared on a read);
//! 4. the route's class:
//!    - `Page`: nothing more;
//!    - `Exchange`: a present Origin (403 `origin-required`);
//!    - `X-AM-CLI-Token` present: no Origin (403 `cli-token-with-origin`),
//!      not a UI-class route (403 `ui-class-only`), this launch's value
//!      (401 `cli-token-invalid`);
//!    - the session: a non-GET request or an upgrade needs an Origin (403
//!      `origin-required`); the `am_s_<port>` cookie names a session (401
//!      `session-required`); the `X-AM-CSRF` header, or on the upgrade the
//!      `am.csrf.<value>` subprotocol, matches (403 `csrf-required`).
//!
//! Every refusal is the envelope; no response carries a CORS header.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::Response;

use crate::envelope;
use crate::routes::{RouteClass, RouteSpec};
use crate::secret::ct_eq;
use crate::state::AppState;
use crate::{CLI_TOKEN_HEADER, CSRF_HEADER, WS_PROTOCOL_PREFIX};

/// How a request was authenticated, for the handler (the event stream
/// echoes the session's subprotocol). `Debug` is not derived: it holds the
/// CSRF value.
#[derive(Clone)]
pub(crate) enum Authed {
    Session { csrf: String },
    Token,
}

#[derive(Clone)]
pub(crate) struct Guard {
    pub(crate) app: AppState,
    pub(crate) table: Arc<Vec<RouteSpec>>,
}

/// A refusal: the envelope it answers (a small value, built on use).
type Refused = fn() -> Response;

pub(crate) async fn guard(State(g): State<Guard>, mut req: Request, next: Next) -> Response {
    let resp = match check(&g, &mut req) {
        Ok(()) => next.run(req).await,
        Err(refusal) => refusal(),
    };
    finish(&g.app, resp)
}

/// The declared route `req` reaches, or the 404 / 405 refusal.
fn lookup<'a>(
    table: &'a [RouteSpec],
    method: &Method,
    path: &str,
) -> Result<&'a RouteSpec, Refused> {
    let on_path: Vec<&RouteSpec> = table.iter().filter(|s| s.matches_path(path)).collect();
    if on_path.is_empty() {
        return Err(envelope::not_found);
    }
    if let Some(s) = on_path.iter().find(|s| s.method == method.as_str()) {
        return Ok(s);
    }
    if method == Method::HEAD
        && let Some(s) = on_path
            .iter()
            .find(|s| s.method == "GET" && !s.changes_state)
    {
        return Ok(s);
    }
    Err(envelope::method_not_allowed)
}

fn check(g: &Guard, req: &mut Request) -> Result<(), Refused> {
    let state = &g.app;
    let headers = req.headers();
    // 1. Host.
    if !state.host_ok(headers, req.uri()) {
        return Err(envelope::host_not_allowed);
    }
    // 2. Origin, when present.
    let mut origins = headers.get_all(header::ORIGIN).iter().peekable();
    let has_origin = origins.peek().is_some();
    for o in origins {
        if !o.to_str().is_ok_and(|o| state.origin_allowed(o)) {
            return Err(envelope::origin_not_allowed);
        }
    }
    // 3. The route.
    let spec = lookup(&g.table, req.method(), req.uri().path())?;
    // 4. The class.
    let is_upgrade = headers.contains_key(header::UPGRADE);
    let is_read = req.method() == Method::GET || req.method() == Method::HEAD;
    match spec.class {
        RouteClass::Page => return Ok(()),
        RouteClass::Exchange => {
            return if has_origin {
                Ok(())
            } else {
                Err(envelope::origin_required)
            };
        }
        RouteClass::UiClass | RouteClass::CliClass => {}
    }
    if let Some(token) = headers.get(CLI_TOKEN_HEADER) {
        if has_origin {
            return Err(envelope::cli_token_with_origin);
        }
        if spec.class == RouteClass::UiClass {
            return Err(envelope::ui_class_only);
        }
        if !token.to_str().is_ok_and(|t| state.token_ok(t)) {
            return Err(envelope::cli_token_invalid);
        }
        req.extensions_mut().insert(Authed::Token);
        return Ok(());
    }
    if (!is_read || is_upgrade) && !has_origin {
        return Err(envelope::origin_required);
    }
    let Some(csrf) = state.session_csrf(headers) else {
        return Err(envelope::session_required);
    };
    let presented = if is_upgrade {
        offered_protocol(headers, &csrf)
    } else {
        headers
            .get(CSRF_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| ct_eq(v, &csrf))
    };
    if !presented {
        return Err(envelope::csrf_required);
    }
    req.extensions_mut().insert(Authed::Session { csrf });
    Ok(())
}

/// Whether the upgrade offers the `am.csrf.<csrf>` subprotocol.
fn offered_protocol(headers: &HeaderMap, csrf: &str) -> bool {
    let wanted = format!("{WS_PROTOCOL_PREFIX}{csrf}");
    headers
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|p| p.to_str().ok())
        .flat_map(|p| p.split(','))
        .any(|p| ct_eq(p.trim(), &wanted))
}

/// Security headers on every response; never a CORS header.
fn finish(state: &AppState, mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&format!(
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self' ws://127.0.0.1:{p} ws://localhost:{p}; frame-ancestors 'none'",
            p = state.port()
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
