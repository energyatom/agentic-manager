//! RULE-15 trust test (story 3.2, AD-13): the launch code. `agentic serve`
//! opens the launch URL with a single-use code valid 60 s in its fragment;
//! the page exchanges it at `POST /api/v1/session` for an `HttpOnly;
//! SameSite=Strict; Path=/` cookie named `am_s_<port>` and `{csrf}` in the
//! body; a failed exchange invalidates every outstanding code. Rows:
//! "Signed in once", "Replayed launch code", "Old launch code", "Failed
//! exchange", "Cookie name", "Exchange without Origin" (session). Written
//! first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use support::*;

/// Signed in once: the launch URL's fragment code is exchanged for the
/// port's cookie and a CSRF value; with both, `GET /api/v1/runs` is 200.
#[tokio::test]
async fn signed_in_once() {
    let rig = Rig::with_project().await;
    let url = rig.state.launch_url();
    let (before, fragment) = url.split_once('#').expect("the code is in the fragment");
    assert_eq!(before, format!("http://127.0.0.1:{}/", rig.port));
    let code = fragment.strip_prefix("code=").expect("#code=<code>");
    assert!(!code.is_empty());

    let r = rig.exchange(SESSION, code).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_cors(&r);
    let cookies = r.header("set-cookie");
    assert_eq!(cookies.len(), 1, "exactly one cookie: {cookies:?}");
    let cookie = cookies[0].to_ascii_lowercase();
    assert!(
        cookie.starts_with(&format!("am_s_{}=", rig.port)),
        "{cookie}"
    );
    assert!(cookie.contains("httponly"), "{cookie}");
    assert!(cookie.contains("samesite=strict"), "{cookie}");
    assert!(cookie.contains("path=/"), "{cookie}");
    let body: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["csrf"], "the body is {{csrf}}: {}", r.body);
    let s = rig.session_of(&r);
    assert!(
        !cookies[0].contains(&s.csrf),
        "the CSRF value is never in a cookie"
    );

    // With the cookie and the CSRF header: no Origin (a same-origin GET)
    // and the served Origin both read.
    let r = rig.get(RUNS, &s.auth()).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_cors(&r);
    let mut h = s.auth();
    h.push(("Origin", rig.origin()));
    assert_eq!(rig.get(RUNS, &h).await.status, 200);
    let mut h = s.auth();
    h.push(("Host", format!("localhost:{}", rig.port)));
    h.push(("Origin", format!("http://localhost:{}", rig.port)));
    assert_eq!(rig.get(RUNS, &h).await.status, 200);
}

/// Replayed launch code: the second exchange of one code is refused
/// `launch-code-invalid` and sets no cookie.
#[tokio::test]
async fn a_replayed_launch_code_is_refused() {
    let rig = Rig::start().await;
    let code = rig.state.issue_launch_code();
    assert_eq!(rig.exchange(SESSION, &code).await.status, 200);
    let r = rig.exchange(SESSION, &code).await;
    envelope(&r, 401, "launch-code-invalid");
    assert!(r.header("set-cookie").is_empty());
}

/// Old launch code: exchanged after 61 s it is refused; within 59 s it
/// works (the injected clock).
#[tokio::test]
async fn an_old_launch_code_is_refused() {
    let rig = Rig::start().await;
    let old = rig.state.issue_launch_code();
    rig.clock.advance(secs(61));
    let r = rig.exchange(SESSION, &old).await;
    envelope(&r, 401, "launch-code-invalid");
    assert!(r.header("set-cookie").is_empty());

    let fresh = rig.state.issue_launch_code();
    rig.clock.advance(secs(59));
    assert_eq!(rig.exchange(SESSION, &fresh).await.status, 200);
}

/// Failed exchange: a wrong code invalidates every outstanding code, so the
/// right ones are refused afterwards.
#[tokio::test]
async fn a_failed_exchange_invalidates_every_outstanding_code() {
    let rig = Rig::start().await;
    let a = rig.state.issue_launch_code();
    let b = rig.state.issue_launch_code();
    envelope(
        &rig.exchange(SESSION, "not-a-code").await,
        401,
        "launch-code-invalid",
    );
    envelope(&rig.exchange(SESSION, &a).await, 401, "launch-code-invalid");
    envelope(&rig.exchange(SESSION, &b).await, 401, "launch-code-invalid");
}

/// Cookie name: every session cookie, from either exchange, is named
/// `am_s_<port>`, and no other cookie is ever set.
#[tokio::test]
async fn the_cookie_is_named_for_the_port_and_no_other_is_set() {
    let rig = Rig::with_project().await;
    let launch = rig.exchange(SESSION, &rig.state.issue_launch_code()).await;
    let pair = rig
        .exchange(PAIR, rig.state.issue_pairing_code().unwrap().expose())
        .await;
    let s = rig.session_of(&launch);
    let mut set = Vec::new();
    set.extend(launch.header("set-cookie"));
    set.extend(pair.header("set-cookie"));
    for r in [
        rig.get(RUNS, &s.auth()).await,
        rig.get(HEALTH, &s.auth()).await,
        rig.get("/", &[]).await,
        rig.get(RUNS, &[]).await,
        rig.exchange(SESSION, "wrong").await,
    ] {
        set.extend(r.header("set-cookie"));
    }
    assert_eq!(set.len(), 2, "only the two exchanges set a cookie: {set:?}");
    for c in set {
        let name = c.split('=').next().unwrap().trim().to_string();
        assert_eq!(name, format!("am_s_{}", rig.port));
    }
}

/// Exchange without Origin (session): `POST /api/v1/session` with no
/// Origin, or a foreign one, is refused 403 before the exchange, so the
/// code is neither consumed nor invalidated.
#[tokio::test]
async fn a_session_exchange_without_a_matching_origin_is_refused() {
    let rig = Rig::start().await;
    let code = rig.state.issue_launch_code();
    let body = format!(r#"{{"code":"{code}"}}"#);
    let r = rig.send("POST", SESSION, &[], &body).await;
    envelope(&r, 403, "origin-required");
    assert!(r.header("set-cookie").is_empty());
    let r = rig
        .send("POST", SESSION, &[("Origin", EVIL.to_string())], &body)
        .await;
    envelope(&r, 403, "origin-not-allowed");
    assert!(r.header("set-cookie").is_empty());
    assert_eq!(rig.exchange(SESSION, &code).await.status, 200);
}

/// No session at all: an API read with no cookie and no token is refused
/// `session-required` (401), and a cookie that names no session too.
#[tokio::test]
async fn an_api_read_with_no_session_is_refused() {
    let rig = Rig::with_project().await;
    envelope(&rig.get(RUNS, &[]).await, 401, "session-required");
    let made_up = vec![
        ("Cookie", format!("am_s_{}={}", rig.port, "0".repeat(64))),
        ("X-AM-CSRF", "0".repeat(64)),
    ];
    envelope(&rig.get(RUNS, &made_up).await, 401, "session-required");
}

// ------------------------------------------- review patch round 1 (P6)

/// P6: a launch code whose issue time is in the future (the clock stepped
/// back after it was issued) is refused; so is one used past 60 s after a
/// step back and forward again.
#[tokio::test]
async fn a_launch_code_issued_in_the_future_is_refused() {
    let rig = Rig::start().await;
    let code = rig.state.issue_launch_code();
    rig.clock.advance(secs(-120));
    envelope(
        &rig.exchange(SESSION, &code).await,
        401,
        "launch-code-invalid",
    );

    // Back and forward: issued at t, the clock steps back 30 s, then on to
    // t + 61 s.
    rig.clock.set(T0);
    let code = rig.state.issue_launch_code();
    rig.clock.advance(secs(-30));
    rig.clock.advance(secs(91));
    envelope(
        &rig.exchange(SESSION, &code).await,
        401,
        "launch-code-invalid",
    );
}
