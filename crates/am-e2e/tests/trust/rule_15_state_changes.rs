//! RULE-15 trust test (story 3.2, AD-13, owner Decision 2026-11-23): a
//! route that changes state is POST, PUT or DELETE, never GET or HEAD; a
//! GET or HEAD to it is refused `405 method-not-allowed` and changes
//! nothing. Reached with the browser session it needs a present, matching
//! Origin plus the CSRF value. Proven on the test-only UI-class fixture
//! route, whose counter shows whether anything changed. Rows: "GET to a
//! state change", "State change without Origin". Written first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use support::*;

/// GET to a state change: GET and HEAD to the fixture mutation with a
/// valid cookie and CSRF (with and without the served Origin) are 405
/// `method-not-allowed`, as are PUT and DELETE (the route is POST only);
/// nothing changed. The POST itself goes through.
#[tokio::test]
async fn get_and_head_to_a_state_change_are_refused_and_change_nothing() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let mut with_origin = s.auth();
    with_origin.push(("Origin", rig.origin()));
    for h in [s.auth(), with_origin.clone()] {
        envelope(&rig.get(FIXTURE, &h).await, 405, "method-not-allowed");
        let head = rig.send("HEAD", FIXTURE, &h, "").await;
        assert_eq!(head.status, 405, "{}", head.text());
        assert_no_cors(&head);
    }
    for m in ["PUT", "DELETE", "PATCH"] {
        envelope(
            &rig.send(m, FIXTURE, &with_origin, "{}").await,
            405,
            "method-not-allowed",
        );
    }
    assert_eq!(rig.fixture.count(), 0, "nothing changed");

    let r = rig.mutate(&s).await;
    assert!((200..300).contains(&r.status), "{}", r.text());
    assert_eq!(rig.fixture.count(), 1);
}

/// The exchanges change state too: a GET or HEAD to either is 405 and
/// sets no cookie.
#[tokio::test]
async fn get_to_an_exchange_is_refused() {
    let rig = Rig::start().await;
    for path in [SESSION, PAIR] {
        let r = rig.get(path, &[]).await;
        envelope(&r, 405, "method-not-allowed");
        assert!(r.header("set-cookie").is_empty());
        let head = rig.send("HEAD", path, &[], "").await;
        assert_eq!(head.status, 405, "{}", head.text());
    }
}

/// State change without Origin: the fixture mutation with cookie and CSRF
/// but no Origin is refused `origin-required`; with a foreign Origin
/// `origin-not-allowed`; nothing changed.
#[tokio::test]
async fn a_state_change_needs_a_matching_origin() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    envelope(
        &rig.send("POST", FIXTURE, &s.auth(), "{}").await,
        403,
        "origin-required",
    );
    let mut foreign = s.auth();
    foreign.push(("Origin", EVIL.to_string()));
    envelope(
        &rig.send("POST", FIXTURE, &foreign, "{}").await,
        403,
        "origin-not-allowed",
    );
    assert_eq!(rig.fixture.count(), 0, "nothing changed");

    // The localhost spelling of the served origin is the served origin.
    let mut h = s.auth();
    h.push(("Host", format!("localhost:{}", rig.port)));
    h.push(("Origin", format!("http://localhost:{}", rig.port)));
    let r = rig.send("POST", FIXTURE, &h, "{}").await;
    assert!((200..300).contains(&r.status), "{}", r.text());
    assert_eq!(rig.fixture.count(), 1);
}
