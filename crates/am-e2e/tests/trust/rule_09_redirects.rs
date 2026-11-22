//! RULE-9 trust test (story 2.2, AD-12): redirects.
//!
//! - At most 3 are followed; each hop is re-checked by the same caller's
//!   rule, re-resolved and pinned again. A 4th is refused.
//! - Only GET (and HEAD) are followed; a redirect answering any other
//!   method is refused.
//! - An Account or MCP request never leaves its endpoint.
//! - When a followed redirect changes scheme, host or port, the credential
//!   and every credential-like header are dropped for the rest of the chain.
//! - Every refused hop is audited, naming the hop's destination.
//!
//! On the scripted resolver and transport; no network. Written first.
#![forbid(unsafe_code)]

#[path = "support/egress.rs"]
mod support;

use am_domain::{AuthScheme, HttpMethod};
use support::*;

/// Redirect, allowed: a seat GET redirected twice among listed hosts (one
/// absolute, one relative) is followed; each hop is resolved and pinned to
/// its own checked address, and the final response is the caller's.
#[tokio::test]
async fn a_seat_get_follows_redirects_among_listed_hosts_each_hop_pinned() {
    let rig = Rig::standard();
    rig.transport.on(
        "https://docs.example.org/a",
        Reply::redirect(302, "https://cdn.example.org/b"),
    );
    rig.transport
        .on("https://cdn.example.org/b", Reply::redirect(301, "/c"));
    rig.transport
        .on("https://cdn.example.org/c", Reply::ok("done"));

    let resp = allowed(
        rig.send(get(seat(SEAT), "https://docs.example.org/a"))
            .await,
        "two redirects",
    );
    assert_eq!(resp.status, 200);
    assert_eq!(body_text(resp).await, "done");
    let hops = rig.transport.hops();
    let urls: Vec<&str> = hops.iter().map(|h| h.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://docs.example.org/a",
            "https://cdn.example.org/b",
            "https://cdn.example.org/c"
        ]
    );
    assert_eq!(hops[0].addr, format!("{DOCS_IP}:443").parse().unwrap());
    assert_eq!(hops[1].addr, format!("{CDN_IP}:443").parse().unwrap());
    assert_eq!(hops[2].addr, format!("{CDN_IP}:443").parse().unwrap());
    assert!(hops.iter().all(|h| h.method == HttpMethod::Get));
    assert_eq!(rig.resolver.lookups("docs.example.org"), 1);
    assert_eq!(
        rig.resolver.lookups("cdn.example.org"),
        2,
        "re-resolved per hop"
    );
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());
}

/// Exactly 3 redirects are followed (4 hops); the 4th redirect is refused
/// after the 3rd was followed, and audited.
#[tokio::test]
async fn three_redirects_are_followed_and_a_fourth_is_refused() {
    let rig = Rig::standard();
    for i in 0..3 {
        rig.transport.on(
            &format!("https://docs.example.org/r{i}"),
            Reply::redirect(302, &format!("https://docs.example.org/r{}", i + 1)),
        );
    }
    rig.transport
        .on("https://docs.example.org/r3", Reply::ok("third"));
    let resp = allowed(
        rig.send(get(seat(SEAT), "https://docs.example.org/r0"))
            .await,
        "three redirects",
    );
    assert_eq!(body_text(resp).await, "third");
    assert_eq!(rig.transport.hops().len(), 4);

    let rig = Rig::standard();
    for i in 0..4 {
        rig.transport.on(
            &format!("https://docs.example.org/r{i}"),
            Reply::redirect(302, &format!("https://docs.example.org/r{}", i + 1)),
        );
    }
    let words = blocked(
        rig.send(get(seat(SEAT), "https://docs.example.org/r0"))
            .await,
        "a 4th redirect",
    );
    assert!(words.contains("after 3 redirects"), "worded: {words}");
    let urls: Vec<String> = rig.transport.hops().into_iter().map(|h| h.url).collect();
    assert_eq!(
        urls,
        [
            "https://docs.example.org/r0",
            "https://docs.example.org/r1",
            "https://docs.example.org/r2",
            "https://docs.example.org/r3"
        ],
        "refused after the 3rd was followed; r4 never requested"
    );
    assert_last_block(
        &rig,
        &[("seat", SEAT), ("host", "docs.example.org")],
        "4th redirect",
    );
}

/// Redirect, refused: a seat redirected to an unlisted host, an IP literal
/// (private, loopback, metadata) or a listed name answering a private
/// address is refused at that hop, audited with the hop's host; the hop is
/// never made.
#[tokio::test]
async fn a_seat_redirect_off_its_list_or_to_a_blocked_address_is_refused() {
    for (target, host) in [
        ("https://other.example.net/x", "other.example.net"),
        ("https://10.0.0.5/x", "10.0.0.5"),
        ("http://127.0.0.1:1234/v1/models", "127.0.0.1"),
        (
            "http://169.254.169.254/latest/meta-data/",
            "169.254.169.254",
        ),
        ("https://internal.example.org/x", "internal.example.org"),
        ("https://docs.example.org:8443/x", "docs.example.org"),
        ("file:///etc/passwd", ""),
    ] {
        let rig = Rig::standard();
        rig.transport
            .on("https://docs.example.org/a", Reply::redirect(302, target));
        blocked(
            rig.send(get(seat(SEAT), "https://docs.example.org/a"))
                .await,
            target,
        );
        assert_eq!(
            rig.transport.hops().len(),
            1,
            "{target}: the hop was not made"
        );
        let mut expect = vec![("seat", SEAT)];
        if !host.is_empty() {
            expect.push(("host", host));
        }
        if host.is_empty() {
            let blocks = rig.blocks();
            assert_eq!(blocks.len(), 1, "{target}: audited");
        } else {
            assert_last_block(&rig, &expect, target);
        }
    }
}

/// An Account never leaves its endpoint: a redirect to another host, port
/// or scheme is refused and audited with the Account id; a redirect within
/// the endpoint is followed and keeps the key.
#[tokio::test]
async fn an_account_redirect_off_its_endpoint_is_refused() {
    for (target, host, port) in [
        (
            "https://other.example.net/v1/models",
            "other.example.net",
            "443",
        ),
        (
            "https://api.anthropic.com:8443/v1/models",
            "api.anthropic.com",
            "8443",
        ),
        (
            "http://api.anthropic.com/v1/models",
            "api.anthropic.com",
            "80",
        ),
        ("http://127.0.0.1:1234/v1/models", "127.0.0.1", "1234"),
    ] {
        let rig = Rig::standard();
        rig.transport.on(
            "https://api.anthropic.com/v1/models",
            Reply::redirect(302, target),
        );
        blocked(
            rig.send(keyed(
                get(account(ANTHROPIC), "https://api.anthropic.com/v1/models"),
                AuthScheme::XApiKey,
                ANTHROPIC,
            ))
            .await,
            target,
        );
        assert_eq!(
            rig.transport.hops().len(),
            1,
            "{target}: the hop was not made"
        );
        assert_last_block(
            &rig,
            &[("account", ANTHROPIC), ("host", host), ("port", port)],
            target,
        );
    }

    let rig = Rig::standard();
    rig.transport.on(
        "https://api.anthropic.com/v1/models",
        Reply::redirect(301, "https://api.anthropic.com/v1/models/"),
    );
    allowed(
        rig.send(keyed(
            get(account(ANTHROPIC), "https://api.anthropic.com/v1/models"),
            AuthScheme::XApiKey,
            ANTHROPIC,
        ))
        .await,
        "within the endpoint",
    );
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 2);
    assert_eq!(
        hops[1].header("x-api-key").as_deref(),
        Some(ANTHROPIC_CANARY),
        "same scheme, host and port: the key stays"
    );
}

/// An MCP server never leaves its approved URL's endpoint.
#[tokio::test]
async fn an_mcp_redirect_off_its_endpoint_is_refused() {
    let rig = Rig::standard();
    rig.transport.on(
        "http://127.0.0.1:3001/mcp",
        Reply::redirect(302, "http://127.0.0.1:3002/mcp"),
    );
    blocked(
        rig.send(get(mcp(MCP), "http://127.0.0.1:3001/mcp")).await,
        "off the approved URL",
    );
    assert_eq!(rig.transport.hops().len(), 1);
    assert_last_block(&rig, &[("server", MCP), ("port", "3002")], "mcp redirect");
}

/// A redirect answering any method but GET (or HEAD) is refused, even to
/// the same URL's host; audited.
#[tokio::test]
async fn a_redirect_on_post_is_refused() {
    let rig = Rig::standard();
    rig.transport.on(
        "https://docs.example.org/form",
        Reply::redirect(307, "https://docs.example.org/form2"),
    );
    blocked(
        rig.send(post(seat(SEAT), "https://docs.example.org/form"))
            .await,
        "seat POST redirected",
    );
    assert_eq!(rig.transport.hops().len(), 1);
    assert_last_block(&rig, &[("seat", SEAT)], "seat POST");

    let rig = Rig::standard();
    rig.transport.on(
        "https://api.anthropic.com/v1/messages",
        Reply::redirect(308, "https://api.anthropic.com/v1/messages2"),
    );
    blocked(
        rig.send(keyed(
            post(account(ANTHROPIC), "https://api.anthropic.com/v1/messages"),
            AuthScheme::XApiKey,
            ANTHROPIC,
        ))
        .await,
        "Account POST redirected within its endpoint",
    );
    assert_eq!(rig.transport.hops().len(), 1);
    assert_last_block(&rig, &[("account", ANTHROPIC)], "Account POST");
}

/// Auth on host change (rewritten in the story 2.2 review: a credential
/// now goes only with its own Account): an Account request carrying its key
/// is redirected within its endpoint, where the key stays, then off it,
/// where the hop is refused and never made, so the key never reaches
/// another origin; a seat request's `mcp-session-id` header is stripped
/// once a redirect changes origin.
#[tokio::test]
async fn the_credential_is_dropped_for_the_rest_of_the_chain_on_an_origin_change() {
    for off in [
        "https://cdn.example.org/v1/models",
        "https://api.anthropic.com:8443/v1/models",
        "http://api.anthropic.com/v1/models",
    ] {
        let rig = Rig::standard();
        rig.transport.on(
            "https://api.anthropic.com/v1/models",
            Reply::redirect(302, "https://api.anthropic.com/v1/models/"),
        );
        rig.transport.on(
            "https://api.anthropic.com/v1/models/",
            Reply::redirect(302, off),
        );
        blocked(
            rig.send(keyed(
                get(account(ANTHROPIC), "https://api.anthropic.com/v1/models"),
                AuthScheme::XApiKey,
                ANTHROPIC,
            ))
            .await,
            off,
        );
        let hops = rig.transport.hops();
        assert_eq!(hops.len(), 2, "{off}: the off-endpoint hop was not made");
        for hop in &hops {
            assert_eq!(
                hop.header("x-api-key").as_deref(),
                Some(ANTHROPIC_CANARY),
                "same scheme, host and port: kept"
            );
        }
        assert!(
            hops.iter()
                .all(|h| h.url.starts_with("https://api.anthropic.com/")),
            "{off}: {hops:?}"
        );
        assert_last_block(&rig, &[("account", ANTHROPIC)], off);
    }

    // A seat's MCP session header does not cross an origin change.
    let rig = Rig::standard();
    rig.transport.on(
        "https://docs.example.org/a",
        Reply::redirect(302, "https://docs.example.org/same"),
    );
    rig.transport.on(
        "https://docs.example.org/same",
        Reply::redirect(302, "https://cdn.example.org/b"),
    );
    rig.transport.on(
        "https://cdn.example.org/b",
        Reply::redirect(302, "https://docs.example.org/c"),
    );
    let mut request = get(seat(SEAT), "https://docs.example.org/a");
    request
        .headers
        .push(("mcp-session-id".into(), "session-canary".into()));
    allowed(rig.send(request).await, "host change");
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 4, "{hops:?}");
    assert_eq!(
        hops[0].header("mcp-session-id").as_deref(),
        Some("session-canary")
    );
    assert_eq!(
        hops[1].header("mcp-session-id").as_deref(),
        Some("session-canary"),
        "same origin: kept"
    );
    for hop in &hops[2..] {
        assert_eq!(hop.header("mcp-session-id"), None, "{hop:?}");
    }
}
