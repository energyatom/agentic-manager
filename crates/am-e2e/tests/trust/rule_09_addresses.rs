//! RULE-9 trust test (story 2.2, AD-12): every DNS answer is checked and
//! the connection is pinned to the checked address.
//!
//! - DNS answer: a listed name answering a private, loopback, link-local,
//!   CGNAT, metadata, multicast or unspecified address, or a NAT64 or 6to4
//!   form of one, is refused, and so is a mixed answer `[public, private]`;
//!   each refusal is audited. A public IPv4 or IPv6 answer is allowed.
//! - Rebinding: one lookup per hop; the hop goes to the address that was
//!   checked, so a later answer cannot move it.
//!
//! On the scripted resolver and transport; no network. Written first.
#![forbid(unsafe_code)]

#[path = "support/egress.rs"]
mod support;

use support::*;

/// A listed name whose answer holds any blocked address is refused before
/// any hop, and audited with the seat, host and port.
#[tokio::test]
async fn a_listed_name_answering_a_blocked_address_is_refused() {
    let rig = Rig::standard();
    for (case, answer) in [
        ("private-10", vec!["10.0.0.5"]),
        ("private-172", vec!["172.16.3.4"]),
        ("private-192", vec!["192.168.1.1"]),
        ("loopback", vec!["127.0.0.1"]),
        ("metadata", vec!["169.254.169.254"]),
        ("link-local", vec!["169.254.10.10"]),
        ("cgnat", vec!["100.64.0.1"]),
        ("multicast", vec!["224.0.0.1"]),
        ("unspecified", vec!["0.0.0.0"]),
        ("broadcast", vec!["255.255.255.255"]),
        ("v6-loopback", vec!["::1"]),
        ("v6-unique-local", vec!["fd00::1"]),
        ("v6-link-local", vec!["fe80::1"]),
        ("v6-multicast", vec!["ff02::1"]),
        ("v6-unspecified", vec!["::"]),
        ("v4-mapped", vec!["::ffff:10.0.0.1"]),
        // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address,
        // checked as that address.
        ("nat64-private", vec!["64:ff9b::a00:5"]),
        ("nat64-metadata", vec!["64:ff9b::a9fe:a9fe"]),
        ("6to4-private", vec!["2002:a00:5::1"]),
        ("6to4-loopback", vec!["2002:7f00:1::1"]),
        // Story 2.2 review: IPv4-translated (::ffff:0:0/96) and Teredo
        // (2001::/32, server and inverted client) carry an IPv4 address.
        ("v4-translated", vec!["::ffff:0:a00:5"]),
        (
            "teredo-client-private",
            vec!["2001:0:4136:e378:8000:63bf:f5ff:fffa"],
        ),
        (
            "teredo-server-private",
            vec!["2001:0:a00:5:8000:63bf:f7f7:f7f7"],
        ),
        ("mixed", vec![DOCS_IP, "10.0.0.5"]),
        ("mixed-v6", vec!["2606:4700::1111", "fd12:3456::1"]),
    ] {
        let host = format!("{case}.example.org");
        rig.resolver.answer(&host, &answer);
        let url = format!("https://{host}/");
        blocked(rig.send(get(seat(SEAT), &url)).await, case);
        assert_last_block(
            &rig,
            &[
                ("seat", SEAT),
                ("scheme", "https"),
                ("host", host.as_str()),
                ("port", "443"),
            ],
            case,
        );
    }
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}

/// The same check binds an Account's endpoint: a provider's name answering
/// a private address is refused.
#[tokio::test]
async fn an_account_endpoint_answering_a_private_address_is_refused() {
    let rig = Rig::standard();
    rig.resolver.reset("api.anthropic.com");
    rig.resolver.answer("api.anthropic.com", &["10.0.0.7"]);
    blocked(
        rig.send(post(
            account(ANTHROPIC),
            "https://api.anthropic.com/v1/messages",
        ))
        .await,
        "private answer",
    );
    assert_last_block(
        &rig,
        &[("account", ANTHROPIC), ("host", "api.anthropic.com")],
        "private answer",
    );
    assert!(rig.transport.hops().is_empty());
}

/// Public answers pass: the hop goes to the answered address, IPv4 or IPv6.
#[tokio::test]
async fn public_answers_are_allowed_and_pinned() {
    let rig = Rig::standard();
    // A Teredo address whose server and client are public passes.
    rig.resolver.answer(
        "teredo.example.org",
        &["2001:0:4136:e378:8000:63bf:f7f7:f7f7"],
    );
    allowed(
        rig.send(get(seat(SEAT), "https://teredo.example.org/"))
            .await,
        "public Teredo",
    );
    rig.resolver.answer("v6.example.org", &["2606:4700::1111"]);
    allowed(
        rig.send(get(seat(SEAT), "https://v6.example.org/")).await,
        "public v6",
    );
    let hops = rig.transport.hops();
    assert_eq!(hops[1].addr, "[2606:4700::1111]:443".parse().unwrap());
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());
}

/// Rebinding: the name answers public at the check and private on any
/// later lookup. The one hop goes to the checked public address after
/// exactly one lookup.
#[tokio::test]
async fn the_connection_goes_to_the_checked_address_after_one_lookup() {
    let rig = Rig::standard();
    rig.resolver.reset("docs.example.org");
    rig.resolver.answer("docs.example.org", &[DOCS_IP]);
    rig.resolver.answer("docs.example.org", &["10.0.0.5"]);
    allowed(
        rig.send(get(seat(SEAT), "https://docs.example.org/a"))
            .await,
        "checked public",
    );
    assert_eq!(
        rig.resolver.lookups("docs.example.org"),
        1,
        "one lookup per hop"
    );
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 1);
    assert_eq!(
        hops[0].addr,
        format!("{DOCS_IP}:443").parse().unwrap(),
        "pinned to the checked address"
    );
}

/// Rebinding across a redirect: each hop is re-resolved and re-checked, so
/// a same-host redirect whose second lookup answers private is refused at
/// that hop.
#[tokio::test]
async fn a_redirect_is_re_resolved_and_a_rebound_answer_refused() {
    let rig = Rig::standard();
    rig.resolver.reset("docs.example.org");
    rig.resolver.answer("docs.example.org", &[DOCS_IP]);
    rig.resolver.answer("docs.example.org", &["10.0.0.5"]);
    rig.transport.on(
        "https://docs.example.org/a",
        Reply::redirect(302, "https://docs.example.org/b"),
    );
    blocked(
        rig.send(get(seat(SEAT), "https://docs.example.org/a"))
            .await,
        "rebound on the second hop",
    );
    assert_eq!(
        rig.resolver.lookups("docs.example.org"),
        2,
        "one lookup per hop"
    );
    assert_eq!(rig.transport.hops().len(), 1, "the second hop was not made");
    assert_last_block(
        &rig,
        &[("seat", SEAT), ("host", "docs.example.org")],
        "rebound",
    );
}
