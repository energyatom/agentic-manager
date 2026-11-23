//! RULE-15 trust test (story 3.2, AD-13): Origin and CSRF. A foreign
//! Origin is always refused, with no CORS header; every API request needs
//! the session cookie plus the matching `X-AM-CSRF` (the WebSocket upgrade
//! carries the value as the `am.csrf.<value>` subprotocol), so a cookie
//! alone authenticates nothing. Rows: "Cross-origin fetch", "Cross-origin
//! upgrade", "Missing CSRF", "Cookie-only GET", "Cookie-only upgrade".
//! Written first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use support::*;

/// Cross-origin fetch: a GET with a valid cookie and CSRF but a foreign
/// Origin is refused `origin-not-allowed`, as is its preflight, and no
/// response carries a CORS header. Another localhost port and `null` are
/// foreign too.
#[tokio::test]
async fn a_cross_origin_fetch_is_refused_without_cors() {
    let rig = Rig::with_project().await;
    let s = rig.sign_in().await;
    let other = if rig.port == 65535 { 1 } else { rig.port + 1 };
    for origin in [
        EVIL.to_string(),
        format!("http://127.0.0.1:{other}"),
        format!("http://localhost:{other}"),
        format!("https://127.0.0.1:{}", rig.port),
        "null".to_string(),
    ] {
        let mut h = s.auth();
        h.push(("Origin", origin.clone()));
        let r = rig.get(RUNS, &h).await;
        assert_eq!(r.status, 403, "Origin {origin:?}:\n{}", r.text());
        envelope(&r, 403, "origin-not-allowed");
    }
    let preflight = rig
        .send(
            "OPTIONS",
            RUNS,
            &[
                ("Origin", EVIL.to_string()),
                ("Access-Control-Request-Method", "GET".to_string()),
                ("Access-Control-Request-Headers", "x-am-csrf".to_string()),
            ],
            "",
        )
        .await;
    envelope(&preflight, 403, "origin-not-allowed");
}

/// Cross-origin upgrade: a WebSocket upgrade with a foreign Origin is
/// refused 403 even with the cookie and the right subprotocol.
#[tokio::test]
async fn a_cross_origin_upgrade_is_refused() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let h = ws_headers(
        &rig.host(),
        Some(EVIL),
        Some(&s.cookie),
        Some(&s.protocol()),
    );
    match upgrade(rig.port, EVENTS, &h).await {
        Ok(_) => panic!("a cross-origin upgrade was accepted"),
        Err(r) => {
            envelope(&r, 403, "origin-not-allowed");
        }
    }
    // An upgrade must carry an Origin (unless it uses the CLI token).
    let h = ws_headers(&rig.host(), None, Some(&s.cookie), Some(&s.protocol()));
    match upgrade(rig.port, EVENTS, &h).await {
        Ok(_) => panic!("an upgrade with no Origin was accepted"),
        Err(r) => {
            envelope(&r, 403, "origin-required");
        }
    }
}

/// Missing CSRF: a valid cookie with no `X-AM-CSRF` (or a wrong one) is
/// refused `csrf-required` on a read and on the fixture mutation with a
/// matching Origin; the fixture is unchanged.
#[tokio::test]
async fn a_missing_or_wrong_csrf_value_is_refused() {
    let rig = Rig::with_project().await;
    let s = rig.sign_in().await;
    envelope(&rig.get(RUNS, &s.cookie_only()).await, 403, "csrf-required");
    let wrong = vec![("Cookie", s.cookie.clone()), ("X-AM-CSRF", "0".repeat(64))];
    envelope(&rig.get(RUNS, &wrong).await, 403, "csrf-required");

    let mut h = s.cookie_only();
    h.push(("Origin", rig.origin()));
    envelope(
        &rig.send("POST", FIXTURE, &h, "{}").await,
        403,
        "csrf-required",
    );
    let mut h = wrong.clone();
    h.push(("Origin", rig.origin()));
    envelope(
        &rig.send("POST", FIXTURE, &h, "{}").await,
        403,
        "csrf-required",
    );
    assert_eq!(rig.fixture.count(), 0, "nothing changed");
}

/// Cookie-only GET: the cookie a page of another localhost service would
/// send (no CSRF value; with or without that page's Origin) authenticates
/// nothing, on any API route.
#[tokio::test]
async fn a_cookie_alone_authenticates_nothing() {
    let rig = Rig::with_project().await;
    let s = rig.sign_in().await;
    for path in [RUNS, HEALTH, "/api/v1/runs/some-id", EVENTS] {
        let r = rig.get(path, &s.cookie_only()).await;
        assert_eq!(r.status, 403, "{path}:\n{}", r.text());
        any_envelope(&r, 403);
    }
    let mut h = s.cookie_only();
    h.push(("Origin", "http://localhost:3000".to_string()));
    let r = rig.get(RUNS, &h).await;
    assert_eq!(r.status, 403);
    any_envelope(&r, 403);
}

/// Cookie-only upgrade: the cookie and the served Origin with no
/// subprotocol, or a wrong one, is refused `csrf-required`; the right
/// subprotocol is echoed on the 101.
#[tokio::test]
async fn a_cookie_only_upgrade_is_refused() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    for proto in [
        None,
        Some("am.csrf.wrong".to_string()),
        Some(s.csrf.clone()),
    ] {
        let h = ws_headers(
            &rig.host(),
            Some(&rig.origin()),
            Some(&s.cookie),
            proto.as_deref(),
        );
        match upgrade(rig.port, EVENTS, &h).await {
            Ok(_) => panic!("upgrade accepted with subprotocol {proto:?}"),
            Err(r) => {
                envelope(&r, 403, "csrf-required");
            }
        }
    }
    let h = ws_headers(
        &rig.host(),
        Some(&rig.origin()),
        Some(&s.cookie),
        Some(&s.protocol()),
    );
    let ws = upgrade(rig.port, EVENTS, &h)
        .await
        .unwrap_or_else(|r| panic!("the right subprotocol is accepted:\n{}", r.text()));
    assert!(
        ws.head
            .to_ascii_lowercase()
            .contains(&format!("sec-websocket-protocol: {}", s.protocol())),
        "{}",
        ws.head
    );
    ws.close().await;
}
