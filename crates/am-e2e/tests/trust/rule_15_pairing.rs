//! RULE-15 trust test (story 3.2, AD-13, owner Decision 2026-11-23): the
//! terminal pairing code. 8 RFC 4648 base32 characters, single use, valid
//! 60 s, at most one outstanding; printed at start and again when the owner
//! presses Enter in the `agentic serve` terminal (a new code invalidates
//! the unused one); never issued over the API. `POST /api/v1/pair {code}`
//! gives the same session as the launch code. 5 attempts in any 60 s (the
//! 6th is 429); locked after 10 failures until restart. Only attempts that
//! passed the Host and Origin checks (and carried a well-formed body)
//! count. Rows: "Pairing", "Pairing replay", "Old pairing code", "New
//! pairing code", "Pairing rate", "Pairing lock", "Foreign requests don't
//! count", "Exchange without Origin" (pair). Written first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use std::io::Cursor;

use am_server::{pairing_terminal, print_pairing_code};
use support::*;

fn pair_body(code: &str) -> String {
    format!(r#"{{"code":"{code}"}}"#)
}

/// The code is 8 characters of the RFC 4648 base32 alphabet (40 bits).
#[tokio::test]
async fn the_pairing_code_is_eight_base32_characters() {
    let rig = Rig::start().await;
    for _ in 0..20 {
        let code = rig.state.issue_pairing_code().unwrap();
        let c = code.expose();
        assert_eq!(c.len(), 8, "{c}");
        assert!(
            c.chars()
                .all(|ch| ch.is_ascii_uppercase() || ('2'..='7').contains(&ch)),
            "{c}"
        );
    }
}

/// Pairing: the printed code at `/api/v1/pair` gives the same session as
/// the launch code (the port's cookie and a CSRF value that reads).
#[tokio::test]
async fn pairing_gives_the_same_session_as_the_launch_code() {
    let rig = Rig::with_project().await;
    let code = rig.state.issue_pairing_code().unwrap();
    let r = rig.exchange(PAIR, code.expose()).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_cors(&r);
    let cookie = r.header("set-cookie");
    assert_eq!(cookie.len(), 1);
    let c = cookie[0].to_ascii_lowercase();
    assert!(c.starts_with(&format!("am_s_{}=", rig.port)), "{c}");
    assert!(c.contains("httponly") && c.contains("samesite=strict") && c.contains("path=/"));
    let s = rig.session_of(&r);
    assert_eq!(rig.get(RUNS, &s.auth()).await.status, 200);
    let h = ws_headers(
        &rig.host(),
        Some(&rig.origin()),
        Some(&s.cookie),
        Some(&s.protocol()),
    );
    let ws = upgrade(rig.port, EVENTS, &h)
        .await
        .unwrap_or_else(|r| panic!("{}", r.text()));
    ws.close().await;
}

/// A wrong code is refused `pairing-code-invalid` and does not invalidate
/// the outstanding one (the lock bounds guessing).
#[tokio::test]
async fn a_wrong_code_leaves_the_outstanding_one_valid() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    let r = rig.exchange(PAIR, "AAAAAAAA").await;
    envelope(&r, 401, "pairing-code-invalid");
    assert!(r.header("set-cookie").is_empty());
    assert_eq!(rig.exchange(PAIR, code.expose()).await.status, 200);
}

/// Pairing replay: the same code twice; the second is refused.
#[tokio::test]
async fn a_replayed_pairing_code_is_refused() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    assert_eq!(rig.exchange(PAIR, code.expose()).await.status, 200);
    let r = rig.exchange(PAIR, code.expose()).await;
    envelope(&r, 401, "pairing-code-invalid");
    assert!(r.header("set-cookie").is_empty());
}

/// Old pairing code: used after 61 s it is refused; within 59 s it works.
#[tokio::test]
async fn an_old_pairing_code_is_refused() {
    let rig = Rig::start().await;
    let old = rig.state.issue_pairing_code().unwrap();
    rig.clock.advance(secs(61));
    envelope(
        &rig.exchange(PAIR, old.expose()).await,
        401,
        "pairing-code-invalid",
    );
    let fresh = rig.state.issue_pairing_code().unwrap();
    rig.clock.advance(secs(59));
    assert_eq!(rig.exchange(PAIR, fresh.expose()).await.status, 200);
}

/// New pairing code: issuing one while another is unused invalidates the
/// old one; the new one works.
#[tokio::test]
async fn a_new_pairing_code_invalidates_the_unused_one() {
    let rig = Rig::start().await;
    let old = rig.state.issue_pairing_code().unwrap();
    let new = rig.state.issue_pairing_code().unwrap();
    assert_ne!(old.expose(), new.expose());
    envelope(
        &rig.exchange(PAIR, old.expose()).await,
        401,
        "pairing-code-invalid",
    );
    assert_eq!(rig.exchange(PAIR, new.expose()).await.status, 200);
}

/// New pairing code, the terminal wiring: the start print and each Enter
/// in the `agentic serve` terminal print a fresh code (one line each), and
/// each new one invalidates the one before.
#[tokio::test]
async fn enter_in_the_serve_terminal_prints_a_new_code() {
    let rig = Rig::start().await;
    let mut out: Vec<u8> = Vec::new();
    print_pairing_code(&rig.state, &mut out).unwrap();
    let mut input = Cursor::new(b"\n\n".to_vec());
    pairing_terminal(&rig.state, &mut input, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let codes = base32_words(&text);
    assert_eq!(
        codes.len(),
        3,
        "one code at start and one per Enter:\n{text}"
    );
    assert_ne!(codes[0], codes[1]);
    assert_ne!(codes[1], codes[2]);
    for old in &codes[..2] {
        envelope(&rig.exchange(PAIR, old).await, 401, "pairing-code-invalid");
    }
    assert_eq!(rig.exchange(PAIR, &codes[2]).await.status, 200);
}

/// Pairing rate: the 6th attempt within 60 s is 429
/// `pairing-rate-limited`, even with the right code; after the window a
/// fresh code pairs.
#[tokio::test]
async fn the_sixth_attempt_in_a_minute_is_rate_limited() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    for _ in 0..5 {
        envelope(
            &rig.exchange(PAIR, "AAAAAAAA").await,
            401,
            "pairing-code-invalid",
        );
    }
    let r = rig.exchange(PAIR, code.expose()).await;
    envelope(&r, 429, "pairing-rate-limited");
    assert!(r.header("set-cookie").is_empty());

    rig.clock.advance(secs(61));
    let fresh = rig.state.issue_pairing_code().unwrap();
    assert_eq!(rig.exchange(PAIR, fresh.expose()).await.status, 200);
}

/// Pairing lock: after 10 failures (spread so the rate limit never
/// answers) even the right, fresh code is refused `pairing-locked`; no new
/// code is issued while locked, and Enter prints that pairing is locked
/// until restart. Launch codes are unaffected.
#[tokio::test]
async fn pairing_locks_after_ten_failures() {
    let rig = Rig::start().await;
    let _first = rig.state.issue_pairing_code().unwrap();
    for _ in 0..5 {
        envelope(
            &rig.exchange(PAIR, "AAAAAAAA").await,
            401,
            "pairing-code-invalid",
        );
    }
    rig.clock.advance(secs(61));
    for _ in 0..4 {
        envelope(
            &rig.exchange(PAIR, "AAAAAAAA").await,
            401,
            "pairing-code-invalid",
        );
    }
    rig.clock.advance(secs(61));
    let right = rig
        .state
        .issue_pairing_code()
        .expect("9 failures: not locked yet");
    // The 10th failure.
    let tenth = rig.exchange(PAIR, "AAAAAAAA").await;
    assert_eq!(tenth.status, 401, "{}", tenth.text());
    let r = rig.exchange(PAIR, right.expose()).await;
    envelope(&r, 403, "pairing-locked");
    assert!(r.header("set-cookie").is_empty());

    assert!(
        rig.state.issue_pairing_code().is_err(),
        "no code while locked"
    );
    let mut out: Vec<u8> = Vec::new();
    pairing_terminal(&rig.state, &mut Cursor::new(b"\n".to_vec()), &mut out).unwrap();
    print_pairing_code(&rig.state, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(base32_words(&text).is_empty(), "no code printed: {text}");
    let lower = text.to_ascii_lowercase();
    assert!(
        lower.contains("locked") && lower.contains("restart"),
        "{text}"
    );

    // Still locked an hour later.
    rig.clock.advance(secs(3600));
    envelope(
        &rig.exchange(PAIR, right.expose()).await,
        403,
        "pairing-locked",
    );
    assert!(rig.state.issue_pairing_code().is_err());
    // The launch code is not affected by the pairing lock.
    assert_eq!(
        rig.exchange(SESSION, &rig.state.issue_launch_code())
            .await
            .status,
        200
    );
}

/// Foreign requests don't count: 20 pair attempts refused for a foreign
/// Host, a foreign Origin or no Origin (right and wrong codes alike) leave
/// no rate limit and no lock; the right code then pairs.
#[tokio::test]
async fn attempts_refused_for_host_or_origin_do_not_count() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    let evil_host = format!("evil.example:{}", rig.port);
    for i in 0..20 {
        let attempt = if i % 2 == 0 {
            "AAAAAAAA"
        } else {
            code.expose()
        };
        let body = pair_body(attempt);
        let (headers, want): (Vec<(&str, String)>, &str) = match i % 3 {
            0 => (
                vec![("Host", evil_host.clone()), ("Origin", rig.origin())],
                "host-not-allowed",
            ),
            1 => (vec![("Origin", EVIL.to_string())], "origin-not-allowed"),
            _ => (vec![], "origin-required"),
        };
        let r = rig.send("POST", PAIR, &headers, &body).await;
        envelope(&r, 403, want);
        assert!(r.header("set-cookie").is_empty());
    }
    let r = rig.exchange(PAIR, code.expose()).await;
    assert_eq!(r.status, 200, "{}", r.text());
}

/// A malformed body never counts either: 20 of them, then the right code
/// pairs.
#[tokio::test]
async fn malformed_bodies_do_not_count() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    for body in ["", "{", "[]", r#"{"cod":"x"}"#, r#"{"code":5}"#]
        .iter()
        .cycle()
        .take(20)
    {
        let r = rig
            .send("POST", PAIR, &[("Origin", rig.origin())], body)
            .await;
        envelope(&r, 400, "bad-request");
    }
    assert_eq!(rig.exchange(PAIR, code.expose()).await.status, 200);
}

/// Exchange without Origin (pair): refused `origin-required`, not counted
/// and not consuming the code.
#[tokio::test]
async fn a_pair_exchange_without_origin_is_refused_and_not_counted() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    for _ in 0..6 {
        let r = rig.send("POST", PAIR, &[], &pair_body(code.expose())).await;
        envelope(&r, 403, "origin-required");
    }
    assert_eq!(rig.exchange(PAIR, code.expose()).await.status, 200);
}

/// A pairing failure does not invalidate launch codes (and a failed launch
/// exchange is the launch code's own rule, not pairing's).
#[tokio::test]
async fn pairing_failures_leave_launch_codes_valid() {
    let rig = Rig::start().await;
    let launch = rig.state.issue_launch_code();
    let _pairing = rig.state.issue_pairing_code().unwrap();
    for _ in 0..3 {
        envelope(
            &rig.exchange(PAIR, "AAAAAAAA").await,
            401,
            "pairing-code-invalid",
        );
    }
    assert_eq!(rig.exchange(SESSION, &launch).await.status, 200);
}

/// No API issues or returns a pairing code: with a session and with the
/// CLI token, the likely paths answer 404 or 405, and no response holds
/// the outstanding code (so the CLI token can never become a browser
/// session).
#[tokio::test]
async fn no_api_route_issues_a_pairing_code() {
    let rig = Rig::with_project().await;
    let s = rig.sign_in().await;
    let code = rig.state.issue_pairing_code().unwrap();
    let secret = vec![code.expose().to_string()];
    let paths = [
        PAIR,
        "/api/v1/pair/code",
        "/api/v1/pair/new",
        "/api/v1/pairing",
        "/api/v1/pairing-code",
        "/api/v1/pairing/code",
        "/api/v1/session/pair",
    ];
    let mut with_origin = s.auth();
    with_origin.push(("Origin", rig.origin()));
    let token = vec![(TOKEN_HEADER, rig.token())];
    for path in paths {
        for (m, h) in [
            ("GET", s.auth()),
            ("POST", with_origin.clone()),
            ("GET", token.clone()),
            ("POST", token.clone()),
        ] {
            let r = rig.send(m, path, &h, "{}").await;
            assert_free_of(&format!("{m} {path}"), &r.text(), &secret);
            if path != PAIR {
                assert!(
                    r.status == 404 || r.status == 405,
                    "{m} {path} answered {}",
                    r.status
                );
            }
        }
    }
    // Still the one outstanding code, unconsumed.
    assert_eq!(rig.exchange(PAIR, code.expose()).await.status, 200);
}

// ------------------------------------------- review patch round 1 (P6)

/// P6: a pairing code whose issue time is in the future (the clock stepped
/// back after it was issued) is refused; so is one used past 60 s after a
/// step back and forward again.
#[tokio::test]
async fn a_pairing_code_issued_in_the_future_is_refused() {
    let rig = Rig::start().await;
    let code = rig.state.issue_pairing_code().unwrap();
    rig.clock.advance(secs(-120));
    envelope(
        &rig.exchange(PAIR, code.expose()).await,
        401,
        "pairing-code-invalid",
    );

    rig.clock.set(T0);
    let code = rig.state.issue_pairing_code().unwrap();
    rig.clock.advance(secs(-30));
    rig.clock.advance(secs(91));
    envelope(
        &rig.exchange(PAIR, code.expose()).await,
        401,
        "pairing-code-invalid",
    );
}
