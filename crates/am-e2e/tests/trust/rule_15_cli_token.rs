//! RULE-15 trust test (story 3.2, AD-13, AD-15): the per-launch CLI token.
//! 256 random bits, hex, written owner-only to `AppDataLayout::cli_token(pid)`
//! = `<app data dir>/serving/<pid>.token` beside the port record and removed
//! at exit; it authenticates only CLI-class routes, and only on a request
//! with no `Origin` header; a stale file's value is refused because the
//! server compares against its own in-memory value. Rows: "CLI token, CLI
//! route", "CLI token, UI route", "CLI token with Origin", "Stale token".
//! Written first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use am_app::runtime::Places;
use am_app::serving::{clear_cli_token, write_cli_token};
use support::*;

/// A temporary app support folder's places.
fn places(dir: &std::path::Path) -> Places {
    Places::under(&dir.join("support"), &dir.join("home"))
}

/// CLI token, CLI route: the token read back from its file, with no
/// Origin, reads `/api/v1/runs` and `/api/v1/health` (200).
#[tokio::test]
async fn the_token_reads_a_cli_route_without_origin() {
    let rig = Rig::with_project().await;
    let places = places(rig.dir.path());
    let path = write_cli_token(&places, std::process::id(), &rig.state.cli_token()).unwrap();
    let token = std::fs::read_to_string(&path).unwrap().trim().to_string();
    let h = vec![(TOKEN_HEADER, token)];
    let r = rig.get(RUNS, &h).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_cors(&r);
    assert_eq!(rig.get(HEALTH, &h).await.status, 200);
    assert!(
        r.header("set-cookie").is_empty(),
        "the token never sets a cookie"
    );
}

/// CLI token, UI route: the token (no Origin) on the UI-class fixture is
/// refused `ui-class-only`; nothing changed.
#[tokio::test]
async fn the_token_is_refused_on_a_ui_class_route() {
    let rig = Rig::start().await;
    let h = vec![(TOKEN_HEADER, rig.token())];
    envelope(
        &rig.send("POST", FIXTURE, &h, "{}").await,
        403,
        "ui-class-only",
    );
    assert_eq!(rig.fixture.count(), 0, "nothing changed");
}

/// CLI token with Origin: the token plus the served Origin (either
/// spelling) is refused `cli-token-with-origin`; plus a foreign Origin,
/// `origin-not-allowed`. A request carrying Origin is never authenticated
/// by the token.
#[tokio::test]
async fn the_token_with_any_origin_is_refused() {
    let rig = Rig::with_project().await;
    for (host, origin) in [
        (rig.host(), rig.origin()),
        (
            format!("localhost:{}", rig.port),
            format!("http://localhost:{}", rig.port),
        ),
    ] {
        let h = vec![
            ("Host", host),
            ("Origin", origin),
            (TOKEN_HEADER, rig.token()),
        ];
        envelope(&rig.get(RUNS, &h).await, 403, "cli-token-with-origin");
    }
    let h = vec![("Origin", EVIL.to_string()), (TOKEN_HEADER, rig.token())];
    envelope(&rig.get(RUNS, &h).await, 403, "origin-not-allowed");
}

/// Stale token: the value in a token file left by an earlier launch (a
/// dead process) is refused `cli-token-invalid`, as are a made-up and an
/// empty value.
#[tokio::test]
async fn a_stale_token_from_a_dead_process_is_refused() {
    let rig = Rig::with_project().await;
    let places = places(rig.dir.path());
    let earlier = am_server::AppState::new(rig.port);
    let stale_path = write_cli_token(&places, 999_999, &earlier.cli_token()).unwrap();
    let stale = std::fs::read_to_string(&stale_path)
        .unwrap()
        .trim()
        .to_string();
    assert_ne!(stale, rig.token(), "every launch has its own token");
    for value in [stale, "0".repeat(64), "x".to_string(), String::new()] {
        let r = rig.get(RUNS, &[(TOKEN_HEADER, value.clone())]).await;
        envelope(&r, 401, "cli-token-invalid");
    }
    // The live one still reads.
    assert_eq!(
        rig.get(RUNS, &[(TOKEN_HEADER, rig.token())]).await.status,
        200
    );
}

/// The file: `<app data dir>/serving/<pid>.token`, 0600 in a 0700
/// `serving` folder (created owner-only from the start), holding the
/// 64-hex-digit value; removed at a clean exit.
#[cfg(unix)]
#[tokio::test]
async fn the_token_file_is_owner_only_beside_the_port_record_and_removed_at_exit() {
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::start().await;
    let places = places(rig.dir.path());
    let pid = std::process::id();
    let token = rig.state.cli_token();
    let path = write_cli_token(&places, pid, &token).unwrap();
    assert_eq!(
        path,
        places.app_data.join("serving").join(format!("{pid}.token"))
    );
    assert_eq!(path, places.user.cli_token(pid));
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&places.app_data.join("serving")), 0o700);
    assert_eq!(mode(&places.app_data), 0o700);
    let text = std::fs::read_to_string(&path).unwrap();
    let value = text.trim();
    assert_eq!(value, token.expose());
    assert_eq!(value.len(), 64, "256 bits, hex");
    assert!(value.chars().all(|c| c.is_ascii_hexdigit()));

    clear_cli_token(&places, pid);
    assert!(!path.exists(), "removed at exit");
}

/// The token never becomes a browser session: with the token, the session
/// and pair exchanges (no Origin) are refused and set no cookie.
#[tokio::test]
async fn the_token_never_opens_a_browser_session() {
    let rig = Rig::start().await;
    let launch = rig.state.issue_launch_code();
    let pairing = rig.state.issue_pairing_code().unwrap();
    for (path, code) in [(SESSION, launch.as_str()), (PAIR, pairing.expose())] {
        let r = rig
            .send(
                "POST",
                path,
                &[(TOKEN_HEADER, rig.token())],
                &format!(r#"{{"code":"{code}"}}"#),
            )
            .await;
        envelope(&r, 403, "origin-required");
        assert!(r.header("set-cookie").is_empty());
    }
}

/// CLI class includes the event stream: the token on an upgrade with no
/// Origin opens it (no subprotocol needed); with an Origin it is refused.
#[tokio::test]
async fn the_token_opens_the_event_stream_without_origin_only() {
    let rig = Rig::start().await;
    let mut h = ws_headers(&rig.host(), None, None, None);
    h.push((TOKEN_HEADER, rig.token()));
    let ws = upgrade(rig.port, EVENTS, &h)
        .await
        .unwrap_or_else(|r| panic!("{}", r.text()));
    ws.close().await;
    let mut h = ws_headers(&rig.host(), Some(&rig.origin()), None, None);
    h.push((TOKEN_HEADER, rig.token()));
    match upgrade(rig.port, EVENTS, &h).await {
        Ok(_) => panic!("the token opened the stream with an Origin"),
        Err(r) => {
            envelope(&r, 403, "cli-token-with-origin");
        }
    }
}

// ------------------------------------------- review patch round 1 (P3)

/// P3: a token file left by a dead process (`serving/<dead pid>.token`) is
/// removed by the same pruning that removes dead pid records
/// (`am_app::runtime::serving_ports`); a live process's token and record
/// are kept.
#[test]
fn a_dead_process_token_file_is_pruned_with_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let places = places(dir.path());
    let dead: u32 = 99_999_999;
    let live = std::process::id();
    let earlier = am_server::AppState::new(1);
    let dead_token = write_cli_token(&places, dead, &earlier.cli_token()).unwrap();
    let live_token = write_cli_token(&places, live, &earlier.cli_token()).unwrap();
    let serving = places.user.serving_dir();
    std::fs::write(serving.join(dead.to_string()), "9090\n").unwrap();
    std::fs::write(serving.join(live.to_string()), "8080\n").unwrap();

    let ports = am_app::runtime::serving_ports(&places.app_data);
    assert_eq!(ports, vec![8080]);
    assert!(
        !serving.join(dead.to_string()).exists(),
        "the dead record is pruned"
    );
    assert!(
        !dead_token.exists(),
        "the dead process's token file is pruned too"
    );
    assert!(live_token.exists(), "a live process's token is kept");
    assert!(serving.join(live.to_string()).exists());
}
