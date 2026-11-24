//! Story 3.2 (AD-15): every route is declared once, in one table, with its
//! class (`Page`, `Exchange`, `UiClass`, `CliClass`) and whether it changes
//! state. This test fails if the router serves a path the table does not
//! hold, if a table entry is not served, or if a state-changing route
//! accepts GET or HEAD. Today's `/api/v1` routes are `health`, `runs`,
//! `runs/{id}` and `events` (all CLI class, reads) plus the `session` and
//! `pair` exchanges; the UI-class fixture route exists only when a test
//! builds the router with `router_with_fixture`. Written first. Story 3.4
//! adds the Inbox: `inbox` and `inbox/{id}` (CLI class, reads) and
//! `inbox/{id}/resolve` (UI class, POST).
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::time::Duration;

use am_server::{
    AppState, FixtureRoute, RouteClass, RouteSpec, bind, route_table, router_with_fixture,
    serve_router,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH"];

fn spec(method: &str, path: &str, class: RouteClass, changes_state: bool) -> RouteSpec {
    RouteSpec {
        method: method.to_string(),
        path: path.to_string(),
        class,
        changes_state,
    }
}

/// The shipped table, exactly.
fn expected() -> Vec<RouteSpec> {
    vec![
        spec("GET", "/", RouteClass::Page, false),
        spec("GET", "/app.js", RouteClass::Page, false),
        spec("POST", "/api/v1/session", RouteClass::Exchange, true),
        spec("POST", "/api/v1/pair", RouteClass::Exchange, true),
        spec("GET", "/api/v1/health", RouteClass::CliClass, false),
        spec("GET", "/api/v1/runs", RouteClass::CliClass, false),
        spec("GET", "/api/v1/runs/{id}", RouteClass::CliClass, false),
        spec("GET", "/api/v1/events", RouteClass::CliClass, false),
        // Story 3.4: the Inbox reads are CLI class; the resolve is UI class
        // (Question 4, an owner-approved deviation from the ticket's "CLI
        // class", 2026-11-24) and changes state.
        spec("GET", "/api/v1/inbox", RouteClass::CliClass, false),
        spec("GET", "/api/v1/inbox/{id}", RouteClass::CliClass, false),
        spec(
            "POST",
            "/api/v1/inbox/{id}/resolve",
            RouteClass::UiClass,
            true,
        ),
    ]
}

fn key(s: &RouteSpec) -> (String, String) {
    (s.method.clone(), s.path.clone())
}

struct Resp {
    status: u16,
    head: String,
    body: String,
}

async fn send(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: &str) -> Resp {
    let mut s =
        format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nHost: 127.0.0.1:{port}\r\n");
    for (k, v) in headers {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ));
    let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    c.write_all(s.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), c.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Resp {
        status: head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        head: head.to_string(),
        body: body.to_string(),
    }
}

/// The error envelope's `code` (AD-14), asserting the shape.
fn code_of(r: &Resp) -> String {
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap_or_else(|e| {
        panic!(
            "an error is the JSON envelope ({e}):\n{}\n\n{}",
            r.head, r.body
        )
    });
    let obj = v.as_object().expect("an object");
    for k in obj.keys() {
        assert!(
            ["code", "message", "fix", "details"].contains(&k.as_str()),
            "unexpected field {k}: {}",
            r.body
        );
    }
    assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()));
    v["code"].as_str().expect("a code").to_string()
}

/// A served router with the fixture route, and a session's headers
/// (cookie, CSRF, the served Origin).
async fn start() -> (u16, Vec<(&'static str, String)>) {
    let (listener, port) = bind(0).await.unwrap();
    let state = AppState::new(port);
    let router = router_with_fixture(state.clone(), FixtureRoute::new());
    tokio::spawn(async move {
        let _ = serve_router(listener, router, std::future::pending()).await;
    });
    let code = state.issue_launch_code();
    let origin = format!("http://127.0.0.1:{port}");
    let r = send(
        port,
        "POST",
        "/api/v1/session",
        &[("Origin", origin.clone())],
        &format!(r#"{{"code":"{code}"}}"#),
    )
    .await;
    assert_eq!(r.status, 200, "{}\n{}", r.head, r.body);
    let cookie = r
        .head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.split(';').next().unwrap().trim().to_string())
        .expect("a cookie");
    let csrf = serde_json::from_str::<serde_json::Value>(&r.body).unwrap()["csrf"]
        .as_str()
        .unwrap()
        .to_string();
    (
        port,
        vec![("Cookie", cookie), ("X-AM-CSRF", csrf), ("Origin", origin)],
    )
}

/// A concrete path for a table path (`{id}` filled).
fn concrete(path: &str) -> String {
    path.replace("{id}", "0190f5a8-0000-7000-8000-000000000000")
}

/// The table holds today's routes exactly, each once; the fixture route
/// is UI class, changes state and is not in the shipped table.
#[test]
fn the_table_holds_exactly_the_shipped_routes() {
    let table = route_table();
    let keys: Vec<(String, String)> = table.iter().map(key).collect();
    let unique: BTreeSet<_> = keys.iter().cloned().collect();
    assert_eq!(unique.len(), keys.len(), "a route declared twice: {keys:?}");
    let want: BTreeSet<_> = expected().iter().map(key).collect();
    assert_eq!(unique, want);
    for s in expected() {
        let got = table.iter().find(|t| key(t) == key(&s)).unwrap();
        assert_eq!(got, &s);
    }
    let fixture = FixtureRoute::spec();
    assert_eq!(
        fixture,
        spec("POST", "/api/v1/fixture", RouteClass::UiClass, true)
    );
    assert!(
        !table.iter().any(|t| key(t) == key(&fixture)),
        "the fixture route never ships"
    );
}

/// No state-changing route accepts GET or HEAD, in the table and on the
/// wire (405 `method-not-allowed`, with a full session).
#[tokio::test]
async fn no_state_changing_route_accepts_get_or_head() {
    let mut all = route_table();
    all.push(FixtureRoute::spec());
    for s in &all {
        if s.changes_state {
            assert!(
                s.method != "GET" && s.method != "HEAD",
                "state-changing route on {}: {s:?}",
                s.method
            );
            assert!(
                ["POST", "PUT", "DELETE"].contains(&s.method.as_str()),
                "{s:?}"
            );
        }
    }
    let (port, auth) = start().await;
    for s in all.iter().filter(|s| s.changes_state) {
        let path = concrete(&s.path);
        let r = send(port, "GET", &path, &auth, "").await;
        assert_eq!(r.status, 405, "GET {path}:\n{}\n{}", r.head, r.body);
        assert_eq!(code_of(&r), "method-not-allowed");
        let r = send(port, "HEAD", &path, &auth, "").await;
        assert_eq!(r.status, 405, "HEAD {path}:\n{}", r.head);
    }
}

/// Every served route is in the table: each table entry answers its
/// method (not 404 or 405); every other method on a table path is 405;
/// paths the table does not hold, likely ones included, are 404
/// `not-found`; every error is the envelope.
#[tokio::test]
async fn every_served_route_is_in_the_table() {
    let mut all = route_table();
    all.push(FixtureRoute::spec());
    let (port, auth) = start().await;
    let paths: BTreeSet<String> = all.iter().map(|s| s.path.clone()).collect();
    for path in &paths {
        let declared: Vec<&RouteSpec> = all.iter().filter(|s| &s.path == path).collect();
        for m in METHODS {
            let is_declared = declared.iter().any(|s| s.method == m)
                // HEAD answers where GET is declared on a read.
                || (m == "HEAD" && declared.iter().any(|s| s.method == "GET" && !s.changes_state));
            let r = send(port, m, &concrete(path), &auth, "{}").await;
            if is_declared {
                assert!(
                    r.status != 404 && r.status != 405,
                    "{m} {path} is in the table but answered {}:\n{}",
                    r.status,
                    r.head
                );
            } else {
                assert_eq!(r.status, 405, "{m} {path} is not declared:\n{}", r.head);
                if m != "HEAD" {
                    assert_eq!(code_of(&r), "method-not-allowed");
                }
            }
            if r.status >= 400 && m != "HEAD" {
                code_of(&r);
            }
        }
    }
    for path in [
        "/index.html",
        "/api",
        "/api/v1",
        "/api/v1/",
        "/api/v1/token",
        "/api/v1/cli-token",
        "/api/v1/pairing",
        "/api/v1/pairing-code",
        "/api/v1/pair/new",
        "/api/v1/launch",
        "/api/v1/launch-code",
        "/api/v1/sessions",
        "/api/v1/run",
        "/api/v1/runs/x/steps",
        "/api/v1/steps",
        "/api/v1/event",
        "/api/v1/debug",
        "/api/v1/test",
        "/api/v2/runs",
        "/api/v1/no-such-route",
    ] {
        for m in ["GET", "POST"] {
            let r = send(port, m, path, &auth, "{}").await;
            assert_eq!(
                r.status, 404,
                "{m} {path} is served but not in the table:\n{}",
                r.head
            );
            assert_eq!(code_of(&r), "not-found");
        }
    }
}

/// The shipped router has no fixture route: built with `AppState::new`,
/// the fixture path is 404.
#[tokio::test]
async fn the_shipped_router_has_no_fixture_route() {
    let (listener, port) = bind(0).await.unwrap();
    let state = AppState::new(port);
    let st = state.clone();
    tokio::spawn(async move {
        let _ = am_server::serve(listener, st, std::future::pending()).await;
    });
    let origin = format!("http://127.0.0.1:{port}");
    let r = send(port, "POST", "/api/v1/fixture", &[("Origin", origin)], "{}").await;
    assert_eq!(r.status, 404, "{}", r.head);
    assert_eq!(code_of(&r), "not-found");
}
