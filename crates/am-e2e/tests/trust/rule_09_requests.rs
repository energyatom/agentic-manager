//! RULE-9 trust test (story 2.2, AD-12, AD-17, AD-25): what a request may
//! carry and what an audit entry may hold.
//!
//! - Caller headers: a request setting `authorization`,
//!   `proxy-authorization`, `x-api-key`, `cookie` or `host` itself (any
//!   case) is refused and audited; only the client attaches a credential.
//! - Oversize: a response past the cap is cut with `TooLarge`; a request
//!   body past the cap is refused before anything is sent; both audited.
//! - Audit content: an entry holds scheme, host, port, reason and ids only:
//!   never a path, a query, userinfo, a header or a body.
//!
//! On the scripted resolver and transport; no network. Written first.
#![forbid(unsafe_code)]

#[path = "support/egress.rs"]
mod support;

use am_domain::{EgressError, HttpRequest};
use am_net::{Limits, Timeouts};
use support::*;

// ------------------------------------------------------------- headers

/// A caller-supplied credential or `host` header is refused before any
/// lookup or hop, and audited; an ordinary header passes.
#[tokio::test]
async fn a_caller_supplied_credential_or_host_header_is_refused() {
    let rig = Rig::standard();
    for (name, value) in [
        ("authorization", "Bearer sk-caller-set"),
        ("Authorization", "Bearer sk-caller-set"),
        ("proxy-authorization", "Basic c2s6c2s="),
        ("x-api-key", "sk-caller-set"),
        ("X-Api-Key", "sk-caller-set"),
        ("cookie", "session=sk-caller-set"),
        ("Cookie", "session=sk-caller-set"),
        ("host", "internal.example.org"),
        ("HOST", "127.0.0.1:1234"),
        // Story 2.2 review: more credential-like headers.
        ("api-key", "sk-caller-set"),
        ("x-goog-api-key", "sk-caller-set"),
        ("X-Auth-Token", "sk-caller-set"),
        ("cookie2", "session=sk-caller-set"),
    ] {
        let mut request = get(seat(SEAT), "https://docs.example.org/a");
        request.headers.push((name.into(), value.into()));
        let before = rig.blocks().len();
        blocked(rig.send(request).await, name);
        assert_eq!(rig.blocks().len(), before + 1, "{name}: audited");
        assert_last_block(&rig, &[("seat", SEAT), ("host", "docs.example.org")], name);
    }
    // An Account's request may not set its own key either.
    let mut request = post(account(ANTHROPIC), "https://api.anthropic.com/v1/messages");
    request
        .headers
        .push(("x-api-key".into(), "sk-caller-set".into()));
    blocked(rig.send(request).await, "Account x-api-key");
    assert!(rig.transport.hops().is_empty(), "nothing sent");
    assert!(rig.resolver.all_lookups().is_empty(), "nothing resolved");

    let mut request = get(seat(SEAT), "https://docs.example.org/a");
    request.headers.push(("accept".into(), "text/html".into()));
    request
        .headers
        .push(("anthropic-version".into(), "2023-06-01".into()));
    allowed(rig.send(request).await, "ordinary headers");
    let hops = rig.transport.hops();
    assert_eq!(hops[0].header("accept").as_deref(), Some("text/html"));
}

// ------------------------------------------------------------- sizes

fn small() -> Limits {
    Limits {
        max_response_bytes: 16,
        max_request_bytes: 32,
        ..Limits::default()
    }
}

/// A response past the cap is cut with a worded `TooLarge`, and the cut is
/// audited; a response at the cap is read whole.
#[tokio::test]
async fn a_response_past_the_cap_is_cut_and_audited() {
    let rig = Rig::with(Directory::standard(), small());
    rig.transport.on(
        "https://docs.example.org/big",
        Reply::chunks(&[b"0123456789", b"0123456789"]),
    );
    rig.transport.on(
        "https://docs.example.org/fits",
        Reply::chunks(&[b"01234567", b"89abcdef"]),
    );

    let err = match rig
        .send(get(seat(SEAT), "https://docs.example.org/big"))
        .await
    {
        Err(e) => e,
        Ok(resp) => am_net::read_all(resp.body.as_ref())
            .await
            .expect_err("the body is cut"),
    };
    assert_eq!(err, EgressError::TooLarge(16));
    assert!(err.to_string().contains("16"), "worded: {err}");
    assert_last_block(
        &rig,
        &[
            ("seat", SEAT),
            ("host", "docs.example.org"),
            ("port", "443"),
        ],
        "response cut",
    );

    let blocks_before = rig.blocks().len();
    let resp = allowed(
        rig.send(get(seat(SEAT), "https://docs.example.org/fits"))
            .await,
        "at the cap",
    );
    assert_eq!(body_text(resp).await, "0123456789abcdef");
    assert_eq!(rig.blocks().len(), blocks_before, "nothing more audited");
}

/// A request body past the cap is refused before any lookup or hop, and
/// audited; a body at the cap is sent.
#[tokio::test]
async fn a_request_body_past_the_cap_is_refused_before_anything_is_sent() {
    let rig = Rig::with(Directory::standard(), small());
    let over = HttpRequest {
        caller: seat(SEAT),
        ..HttpRequest::post_json("https://docs.example.org/form", vec![b'x'; 33])
    };
    let err = rig.send(over).await.err().expect("refused");
    assert!(
        matches!(err, EgressError::TooLarge(_) | EgressError::Blocked(_)),
        "{err:?}"
    );
    assert!(rig.transport.hops().is_empty(), "nothing sent");
    assert!(rig.resolver.all_lookups().is_empty(), "nothing resolved");
    assert_last_block(
        &rig,
        &[("seat", SEAT), ("host", "docs.example.org")],
        "request too large",
    );

    let at = HttpRequest {
        caller: seat(SEAT),
        ..HttpRequest::post_json("https://docs.example.org/form", vec![b'x'; 32])
    };
    allowed(rig.send(at).await, "at the cap");
    assert_eq!(rig.transport.hops()[0].body.len(), 32);
}

// ------------------------------------------------------------- audit content

const CANARIES: [&str; 7] = [
    "canaryuser",
    "canarypw",
    "canarypath",
    "canaryquery",
    "canaryvalue",
    "canaryheader",
    "canarybody",
];

/// A request whose path, query, userinfo, headers and body all carry
/// canaries.
fn loaded(caller: Option<am_domain::net::Caller>, url: &str) -> HttpRequest {
    let mut request = HttpRequest {
        caller,
        ..HttpRequest::post_json(url, b"{\"note\":\"canarybody\"}".to_vec())
    };
    request
        .headers
        .push(("x-note".into(), "canaryheader".into()));
    request
}

/// Audit content: every refusal's entry holds the destination's scheme,
/// host and port, the reason and the ids, and none of the request's path,
/// query, userinfo, headers or body.
#[tokio::test]
async fn an_audit_entry_holds_no_path_query_userinfo_header_or_body() {
    let rig = Rig::with(Directory::standard(), small());
    let cases: Vec<(&str, HttpRequest)> = vec![
        (
            "unlisted host",
            loaded(
                seat(SEAT),
                "https://other.example.net/canarypath/x?canaryquery=canaryvalue",
            ),
        ),
        (
            "userinfo on a listed host",
            loaded(
                seat(SEAT),
                "https://canaryuser:canarypw@docs.example.org/canarypath?canaryquery=canaryvalue",
            ),
        ),
        (
            "Account off its port",
            loaded(
                account(ANTHROPIC),
                "https://api.anthropic.com:8443/canarypath?canaryquery=canaryvalue",
            ),
        ),
        (
            "no caller",
            loaded(
                None,
                "https://docs.example.org/canarypath?canaryquery=canaryvalue",
            ),
        ),
        ("body too large", {
            let mut r = loaded(
                seat(SEAT),
                "https://docs.example.org/canarypath?canaryquery=canaryvalue",
            );
            r.body =
                format!("{{\"note\":\"canarybody\",\"pad\":\"{}\"}}", "x".repeat(64)).into_bytes();
            r
        }),
        ("caller cookie", {
            let mut r = loaded(
                seat(SEAT),
                "https://docs.example.org/canarypath?canaryquery=canaryvalue",
            );
            r.headers.push(("cookie".into(), "canaryheader".into()));
            r
        }),
    ];
    for (what, request) in cases {
        let before = rig.blocks().len();
        let _ = rig.send(request).await.err().expect(what);
        let blocks = rig.blocks();
        assert_eq!(blocks.len(), before + 1, "{what}: audited");
        let entry = format!("{:?}", blocks.last().unwrap());
        for canary in CANARIES {
            assert!(!entry.contains(canary), "{what}: {canary} in {entry}");
        }
        assert_block(blocks.last().unwrap(), &[], what);
    }

    // A redirect whose target carries a path and query: the entry names
    // the target's host and port only.
    let rig = Rig::standard();
    rig.transport.on(
        "https://docs.example.org/a",
        Reply::redirect(
            302,
            "https://canaryuser:canarypw@other.example.net/canarypath?canaryquery=canaryvalue",
        ),
    );
    // (Story 2.2 review: a seat carries no credential, so none is sent.)
    let _ = rig
        .send(get(seat(SEAT), "https://docs.example.org/a"))
        .await
        .err()
        .expect("refused");
    let entry = format!("{:?}", rig.blocks());
    for canary in CANARIES.iter().chain([&SEARCH_CANARY]) {
        assert!(!entry.contains(canary), "redirect: {canary} in {entry}");
    }
    assert_last_block(&rig, &[("host", "other.example.net")], "redirect");
}

// ------------------------------------------------------------- timeouts

/// Story 2.2 review: the time limits depend on the caller kind. A seat's
/// hop whose head does not arrive within its head timeout, or whose body
/// goes idle past its idle timeout, is refused and audited. An Account's
/// call has no head timeout from am-net (the engine's step timeout and
/// cancel cover a model that loads or reads a long prompt, AD-19) and an
/// idle timeout once its head arrived. An MCP request has neither (its
/// session and the engine's cancel cover a long tool call and a quiet SSE
/// stream).
const SHORT: std::time::Duration = std::time::Duration::from_millis(50);

fn quick() -> Limits {
    Limits {
        seat: Timeouts {
            head: Some(SHORT),
            idle: Some(SHORT),
        },
        account: Timeouts {
            head: None,
            idle: Some(SHORT),
        },
        mcp: Timeouts {
            head: None,
            idle: None,
        },
        ..roomy()
    }
}

#[test]
fn the_default_timeouts_depend_on_the_caller_kind() {
    let limits = Limits::default();
    let secs = |s| Some(std::time::Duration::from_secs(s));
    assert_eq!(
        limits.seat,
        Timeouts {
            head: secs(30),
            idle: secs(120)
        }
    );
    assert_eq!(
        limits.account,
        Timeouts {
            head: None,
            idle: secs(300)
        }
    );
    assert_eq!(
        limits.mcp,
        Timeouts {
            head: None,
            idle: None
        }
    );
}

#[tokio::test]
async fn a_seats_response_head_that_never_arrives_times_out_and_is_audited() {
    let rig = Rig::with(Directory::standard(), quick());
    rig.transport.on(
        "https://docs.example.org/slow",
        Reply::late(std::time::Duration::from_secs(30)),
    );
    let started = std::time::Instant::now();
    let err = rig
        .send(get(seat(SEAT), "https://docs.example.org/slow"))
        .await
        .err()
        .expect("timed out");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "gave up after {:?}",
        started.elapsed()
    );
    assert!(
        matches!(err, EgressError::Network(_) | EgressError::Blocked(_)),
        "{err:?}"
    );
    assert!(err.to_string().contains("timed out"), "worded: {err}");
    assert_last_block(
        &rig,
        &[("seat", SEAT), ("host", "docs.example.org")],
        "head timeout",
    );
}

#[tokio::test]
async fn a_seats_body_that_goes_idle_times_out_and_is_audited() {
    let rig = Rig::with(Directory::standard(), quick());
    rig.transport.on(
        "https://docs.example.org/stall",
        Reply::stalling(&[b"first"]),
    );
    let resp = allowed(
        rig.send(get(seat(SEAT), "https://docs.example.org/stall"))
            .await,
        "the head arrives",
    );
    assert_eq!(resp.body.next().await.unwrap().unwrap(), b"first".to_vec());
    let err = tokio::time::timeout(std::time::Duration::from_secs(5), resp.body.next())
        .await
        .expect("the idle body ends within 5 s")
        .expect("an error, not the end")
        .expect_err("timed out");
    assert!(err.to_string().contains("timed out"), "worded: {err}");
    assert_last_block(
        &rig,
        &[("seat", SEAT), ("host", "docs.example.org")],
        "idle timeout",
    );
}

/// An Account's call waits for its head as long as it takes (a local model
/// loading, a long prompt read, a large upload), then times out only when
/// its body goes idle.
#[tokio::test]
async fn an_accounts_call_has_no_head_timeout_but_an_idle_timeout() {
    let rig = Rig::with(Directory::standard(), quick());
    let slow = "http://127.0.0.1:1234/v1/chat/completions";
    rig.transport
        .on(slow, Reply::late(std::time::Duration::from_millis(300)));
    let resp = allowed(
        rig.send(post(account(LOCAL_LMSTUDIO), slow)).await,
        "a head after 300 ms, six times the seat's head timeout",
    );
    assert_eq!(body_text(resp).await, "late");

    let stall = "http://127.0.0.1:11434/v1/chat/completions";
    rig.transport.on(stall, Reply::stalling(&[b"first"]));
    let resp = allowed(
        rig.send(post(account(LOCAL_OLLAMA), stall)).await,
        "the head arrives",
    );
    assert_eq!(resp.body.next().await.unwrap().unwrap(), b"first".to_vec());
    let err = tokio::time::timeout(std::time::Duration::from_secs(5), resp.body.next())
        .await
        .expect("the idle body ends within 5 s")
        .expect("an error, not the end")
        .expect_err("timed out");
    assert!(err.to_string().contains("timed out"), "worded: {err}");
    assert_last_block(&rig, &[("account", LOCAL_OLLAMA)], "account idle timeout");
}

/// An MCP request has no head and no idle timeout from am-net: a long tool
/// call and a quiet server-to-client stream are not cut.
#[tokio::test]
async fn an_mcp_request_has_no_head_or_idle_timeout() {
    let rig = Rig::with(Directory::standard(), quick());
    let url = "http://127.0.0.1:3001/mcp";
    rig.transport
        .on(url, Reply::late(std::time::Duration::from_millis(300)));
    let resp = allowed(rig.send(post(mcp(MCP), url)).await, "a long tool call");
    assert_eq!(body_text(resp).await, "late");

    let rig = Rig::with(Directory::standard(), quick());
    rig.transport.on(url, Reply::stalling(&[b"event"]));
    let resp = allowed(rig.send(get(mcp(MCP), url)).await, "the stream opens");
    assert_eq!(resp.body.next().await.unwrap().unwrap(), b"event".to_vec());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(400), resp.body.next())
            .await
            .is_err(),
        "a quiet stream stays open past eight times the seat's idle timeout"
    );
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());
}
