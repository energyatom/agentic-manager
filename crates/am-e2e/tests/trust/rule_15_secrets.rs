//! RULE-15 trust test (story 3.2, AD-13, AD-14): secrets stay in memory.
//! The session ids, CSRF values, launch and pairing codes and the CLI token
//! are generated per launch and never written to a log line, a URL (except
//! the launch code in the opened URL's fragment) or a file (except the CLI
//! token in its own 0600 file); every error is the one envelope and no
//! secret appears in any of its fields; `Debug` never shows a secret.
//! Rows: "Secret nowhere", "Envelope". Written first.
//!
//! Log lines are captured by a `tracing` subscriber installed for this
//! test binary (nextest runs each test in its own process).
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use am_app::runtime::Places;
use am_app::serving::write_cli_token;
use support::*;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

// ------------------------------------------------------------ log capture

#[derive(Clone, Default)]
struct Capture {
    lines: Arc<Mutex<Vec<String>>>,
    next: Arc<AtomicU64>,
}

struct Line(String);

impl Visit for Line {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={:?}", field.name(), value);
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        let _ = write!(self.0, " {}={}", field.name(), value);
    }
}

impl Capture {
    fn push(&self, line: String) {
        self.lines.lock().unwrap().push(line);
    }
    fn text(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let mut l = Line(format!("span {}", attrs.metadata().name()));
        attrs.record(&mut l);
        self.push(l.0);
        Id::from_u64(self.next.fetch_add(1, Ordering::Relaxed) + 1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut l = Line("record".into());
        values.record(&mut l);
        self.push(l.0);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut l = Line(format!("event {}", event.metadata().target()));
        event.record(&mut l);
        self.push(l.0);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

fn capture() -> Capture {
    let c = Capture::default();
    tracing::subscriber::set_global_default(c.clone()).expect("one subscriber per test process");
    c
}

// ------------------------------------------------------------------ tests

/// Secret nowhere: after a sign-in, a pairing and a few requests (good and
/// refused, the event stream included), the session ids, CSRF values,
/// pairing code and token are in no log line, in no URL but the launch
/// fragment, in no response other than the exchange that hands them over,
/// and in no file under the app data dir or the runtime folder except the
/// token in its own file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn secrets_are_in_no_log_url_or_file() {
    let logs = capture();
    let rig = Rig::with_project().await;
    let run = rig.create_run(&["a"]);
    let places = Places::under(
        &rig.dir.path().join("support"),
        &rig.dir.path().join("home"),
    );
    let token_path = write_cli_token(&places, std::process::id(), &rig.state.cli_token()).unwrap();
    let token = rig.token();

    // The launch URL: the code only in its fragment.
    let url = rig.state.launch_url();
    let (before, fragment) = url.split_once('#').unwrap();
    let launch = fragment.strip_prefix("code=").unwrap().to_string();
    let r1 = rig.exchange(SESSION, &launch).await;
    let s1 = rig.session_of(&r1);
    let pairing = rig.state.issue_pairing_code().unwrap().expose().to_string();
    let r2 = rig.exchange(PAIR, &pairing).await;
    let s2 = rig.session_of(&r2);

    let secrets = vec![
        s1.sid.clone(),
        s1.csrf.clone(),
        s2.sid.clone(),
        s2.csrf.clone(),
        launch.clone(),
        pairing.clone(),
        token.clone(),
    ];
    assert_free_of("the launch URL before its fragment", before, &secrets);
    assert_free_of("the launch URL's fragment", fragment, &secrets[..4]);
    assert_free_of("the launch URL's fragment", fragment, &secrets[5..]);
    // The exchanges hand over only their own session.
    assert_free_of(
        "the launch exchange",
        &r1.text(),
        &[
            s2.sid.clone(),
            s2.csrf.clone(),
            pairing.clone(),
            token.clone(),
        ],
    );
    assert_free_of(
        "the pair exchange",
        &r2.text(),
        &[
            s1.sid.clone(),
            s1.csrf.clone(),
            launch.clone(),
            token.clone(),
        ],
    );

    let mut responses = vec![
        rig.get(RUNS, &s1.auth()).await,
        rig.get(&format!("{RUNS}/{run}"), &s2.auth()).await,
        rig.get(RUNS, &[(TOKEN_HEADER, token.clone())]).await,
        rig.get(HEALTH, &s1.auth()).await,
        rig.get("/", &[]).await,
        rig.get("/app.js", &[]).await,
        rig.get(RUNS, &s1.cookie_only()).await,
        rig.exchange(SESSION, "wrong").await,
        rig.exchange(PAIR, "AAAAAAAA").await,
        rig.exchange(PAIR, &pairing).await,
        rig.send("POST", FIXTURE, &[(TOKEN_HEADER, token.clone())], "{}")
            .await,
        rig.get(
            RUNS,
            &[("Origin", rig.origin()), (TOKEN_HEADER, token.clone())],
        )
        .await,
        rig.mutate(&s1).await,
    ];
    let mut ws = rig.events_with_session(&s1, Some(0)).await;
    rig.state
        .events()
        .publish("test.tick", serde_json::json!({ "n": 1 }));
    let e = ws.next_event(std::time::Duration::from_secs(5)).await;
    assert_free_of("an event", &format!("{e:?}"), &secrets);
    assert_free_of(
        "the 101 response",
        &ws.head,
        &[
            s1.sid.clone(),
            s2.sid.clone(),
            s2.csrf.clone(),
            pairing.clone(),
            token.clone(),
            launch.clone(),
        ],
    );
    ws.close().await;
    responses.push(
        rig.get(RUNS, &[("Host", format!("evil.example:{}", rig.port))])
            .await,
    );
    for r in &responses {
        assert_free_of("a response", &r.text(), &secrets);
    }

    // No log line holds a secret.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_free_of("the log", &logs.text(), &secrets);

    // No file but the token's own holds a secret (the store included).
    let mut roots = vec![places.app_data.clone()];
    roots.push(rig.runtime.as_ref().unwrap().dir().to_path_buf());
    for root in roots {
        for f in files_under(&root) {
            let bytes = std::fs::read(&f).unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            if f == token_path {
                assert_eq!(text.trim(), token);
                assert_free_of(&f.display().to_string(), &text, &secrets[..6]);
            } else {
                assert_free_of(&f.display().to_string(), &text, &secrets);
            }
        }
    }
}

/// Envelope: every refusal the guard and the routes give is JSON
/// `{code, message, fix?, details?}` with its stable code and status, and
/// no secret (session id, CSRF, launch or pairing code, token) in any
/// field.
#[tokio::test]
async fn every_error_is_one_envelope_with_no_secret() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let launch = rig.state.issue_launch_code();
    let pairing = rig.state.issue_pairing_code().unwrap();
    let token = rig.token();
    let secrets = vec![
        s.sid.clone(),
        s.csrf.clone(),
        launch.clone(),
        pairing.expose().to_string(),
        token.clone(),
    ];
    let evil_host = format!("evil.example:{}", rig.port);
    let mut cases: Vec<(Resp, u16, &str)> = Vec::new();
    let mut h = s.auth();
    h.push(("Host", evil_host.clone()));
    cases.push((rig.get(RUNS, &h).await, 403, "host-not-allowed"));
    let mut h = s.auth();
    h.push(("Origin", EVIL.to_string()));
    cases.push((rig.get(RUNS, &h).await, 403, "origin-not-allowed"));
    cases.push((
        rig.send("POST", FIXTURE, &s.auth(), "{}").await,
        403,
        "origin-required",
    ));
    cases.push((rig.get(RUNS, &s.cookie_only()).await, 403, "csrf-required"));
    cases.push((rig.get(RUNS, &[]).await, 401, "session-required"));
    cases.push((rig.get(RUNS, &s.auth()).await, 409, "no-project"));
    cases.push((
        rig.get("/api/v1/no-such-route", &s.auth()).await,
        404,
        "not-found",
    ));
    cases.push((rig.get(FIXTURE, &s.auth()).await, 405, "method-not-allowed"));
    cases.push((
        rig.send("POST", FIXTURE, &[(TOKEN_HEADER, token.clone())], "{}")
            .await,
        403,
        "ui-class-only",
    ));
    cases.push((
        rig.get(
            RUNS,
            &[("Origin", rig.origin()), (TOKEN_HEADER, token.clone())],
        )
        .await,
        403,
        "cli-token-with-origin",
    ));
    cases.push((
        rig.get(RUNS, &[(TOKEN_HEADER, "0".repeat(64))]).await,
        401,
        "cli-token-invalid",
    ));
    cases.push((
        rig.send("POST", PAIR, &[("Origin", rig.origin())], "{")
            .await,
        400,
        "bad-request",
    ));
    cases.push((
        rig.exchange(PAIR, "AAAAAAAA").await,
        401,
        "pairing-code-invalid",
    ));
    // The launch code last: its failure invalidates the outstanding one.
    cases.push((
        rig.exchange(SESSION, "wrong").await,
        401,
        "launch-code-invalid",
    ));
    for (r, status, code) in &cases {
        let e = envelope(r, *status, code);
        assert_free_of(&format!("the {code} envelope"), &r.text(), &secrets);
        let fields = format!("{} {} {:?} {:?}", e.code, e.message, e.fix, e.details);
        assert_free_of(&format!("the {code} fields"), &fields, &secrets);
    }
}

/// `Debug` never shows a secret: the CLI token and the pairing code print
/// redacted.
#[tokio::test]
async fn debug_output_shows_no_secret() {
    let rig = Rig::start().await;
    let token = rig.state.cli_token();
    let code = rig.state.issue_pairing_code().unwrap();
    let shown = format!("{token:?} {code:?}");
    assert_free_of(
        "Debug output",
        &shown,
        &[token.expose().to_string(), code.expose().to_string()],
    );
}

// ------------------------------------------- review patch round 1 (P7)

/// P7: the exchange request bodies print redacted: `Debug` of
/// `am_api::SessionBody` and `am_api::PairBody` never shows the code.
#[test]
fn exchange_bodies_show_no_code_in_debug() {
    let launch = "4f1c0d9e8b7a6f5e4d3c2b1a0f9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e".to_string();
    let pairing = "K7Q2M4ZD".to_string();
    let shown = format!(
        "{:?} {:?}",
        am_api::SessionBody {
            code: launch.clone()
        },
        am_api::PairBody {
            code: pairing.clone()
        }
    );
    assert_free_of("Debug of the exchange bodies", &shown, &[launch, pairing]);
}
