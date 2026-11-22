//! RULE-9 trust test (story 2.2, AD-12, AD-48): every request names who it
//! is for, and the client decides from one directory what that caller may
//! reach. One test per caller row of the story's I/O matrix:
//!
//! - a seat reaches only its `allowed_hosts` (`https` on 443, `http` on
//!   80), never loopback, a private address or an IP literal;
//! - an Account reaches only its pinned endpoint (scheme, host and port),
//!   with its key attached by the client; the reserved local Accounts on
//!   their ports, defined once; an owner-typed loopback Account on its
//!   port, never on the app's own serving port;
//! - an MCP server reaches only its approved URL's endpoint;
//! - a request with no caller, or for an id the directory does not know,
//!   is refused;
//! - every refusal is an `egress.blocked` audit entry by `System(am-net)`
//!   naming the destination's scheme, host and port, the reason and the id.
//!
//! No network: names resolve through the scripted resolver and hops go to
//! the scripted transport, except the owner-typed loopback test, which runs
//! the real constructor against a server on 127.0.0.1. Written first.
#![forbid(unsafe_code)]

#[path = "support/egress.rs"]
mod support;

use std::sync::Arc;

use am_domain::net::{EgressDirectory, reserved_local_account};
use am_domain::{AuthScheme, EgressClient, EgressError};
use am_net::NetEgress;
use am_testkit::CollectAudit;
use support::*;

// ------------------------------------------------------------------ seat

/// Seat, listed: a GET to a host its patterns admit goes out, pinned to the
/// one address the resolver answered, after one lookup; nothing is audited.
#[tokio::test]
async fn a_seat_reaches_a_listed_host_pinned_to_the_resolved_address() {
    let rig = Rig::standard();
    let resp = allowed(
        rig.send(get(seat(SEAT), "https://docs.example.org/a"))
            .await,
        "listed",
    );
    assert_eq!(resp.status, 200);
    assert_eq!(body_text(resp).await, "ok");
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 1, "{hops:?}");
    assert_eq!(hops[0].url, "https://docs.example.org/a");
    assert_eq!(
        hops[0].addr,
        format!("{DOCS_IP}:443").parse().unwrap(),
        "the connection goes to the checked address"
    );
    assert_eq!(rig.resolver.lookups("docs.example.org"), 1, "one lookup");
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());

    // `*.example.org` admits a subdomain; plain http on port 80 is admitted.
    allowed(
        rig.send(get(seat(SEAT), "https://cdn.example.org/b")).await,
        "a subdomain of a wildcard",
    );
    allowed(
        rig.send(get(seat(SEAT), "http://docs.example.org/c")).await,
        "http on port 80",
    );
    let hops = rig.transport.hops();
    assert_eq!(hops[1].addr, format!("{CDN_IP}:443").parse().unwrap());
    assert_eq!(hops[2].addr, format!("{DOCS_IP}:80").parse().unwrap());
}

/// Seat, unlisted: refused in words naming the host, audited with the seat
/// id, host and port; nothing is resolved or sent.
#[tokio::test]
async fn a_seat_is_refused_an_unlisted_host_and_the_block_is_audited() {
    let rig = Rig::standard();
    let words = blocked(
        rig.send(get(seat(SEAT), "https://other.example.net/"))
            .await,
        "unlisted",
    );
    assert!(
        words.contains("other.example.net"),
        "names the host: {words}"
    );
    assert!(rig.transport.hops().is_empty(), "nothing sent");
    assert_eq!(rig.resolver.lookups("other.example.net"), 0, "no lookup");
    assert_last_block(
        &rig,
        &[
            ("seat", SEAT),
            ("scheme", "https"),
            ("host", "other.example.net"),
            ("port", "443"),
        ],
        "unlisted",
    );

    // Another seat's list does not count: s2 may reach files.example.net, s1 may not.
    blocked(
        rig.send(get(seat(SEAT), "https://files.example.net/"))
            .await,
        "another seat's host",
    );
    allowed(
        rig.send(get(seat(OTHER_SEAT), "https://files.example.net/"))
            .await,
        "its own host",
    );
    // `*.example.org` does not admit example.org itself.
    blocked(
        rig.send(get(seat(SEAT), "https://example.org/")).await,
        "the wildcard's own domain",
    );
}

/// Seat, wrong port or scheme: a listed host on another port, or http on a
/// port other than 80, is refused and audited.
#[tokio::test]
async fn a_seat_is_refused_a_listed_host_on_another_port_or_scheme() {
    let rig = Rig::standard();
    for (url, scheme, port) in [
        ("https://docs.example.org:8443/", "https", "8443"),
        ("http://docs.example.org:8080/", "http", "8080"),
        ("http://docs.example.org:443/", "http", "443"),
        ("https://docs.example.org:80/", "https", "80"),
        ("ftp://docs.example.org/", "ftp", "21"),
    ] {
        let before = rig.blocks().len();
        blocked(rig.send(get(seat(SEAT), url)).await, url);
        assert_eq!(rig.blocks().len(), before + 1, "{url}: audited");
        if scheme != "ftp" {
            assert_last_block(
                &rig,
                &[
                    ("seat", SEAT),
                    ("scheme", scheme),
                    ("host", "docs.example.org"),
                    ("port", port),
                ],
                url,
            );
        }
    }
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}

/// Seat to loopback or a private address: refused even though nothing in
/// the seat's list could admit it (no loopback or private exception, no IP
/// literal); audited.
#[tokio::test]
async fn a_seat_never_reaches_loopback_a_private_address_or_an_ip_literal() {
    let rig = Rig::standard();
    for url in [
        "http://127.0.0.1:1234/",
        "http://127.0.0.1:11434/api/ps",
        "http://localhost:1234/",
        "http://[::1]:1234/",
        "https://10.0.0.5/",
        "http://169.254.169.254/latest/meta-data/",
        "https://93.184.216.34/",
        "http://127.0.0.1:3001/mcp",
    ] {
        let before = rig.blocks().len();
        blocked(rig.send(get(seat(SEAT), url)).await, url);
        assert_eq!(rig.blocks().len(), before + 1, "{url}: audited");
        assert_last_block(&rig, &[("seat", SEAT)], url);
    }
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}

// --------------------------------------------------------------- Account

/// Account, its endpoint: allowed, pinned to the resolved address, and the
/// key is attached by the client from the request's `AuthRef` (the request
/// itself carries none).
#[tokio::test]
async fn an_account_reaches_its_endpoint_with_its_key_attached_by_the_client() {
    let rig = Rig::standard();
    let request = keyed(
        post(account(ANTHROPIC), "https://api.anthropic.com/v1/messages"),
        AuthScheme::XApiKey,
        ANTHROPIC,
    );
    assert!(
        request
            .headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("x-api-key")),
        "the request carries a reference, never the key"
    );
    let resp = allowed(rig.send(request).await, "its endpoint");
    assert_eq!(resp.status, 200);
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 1);
    assert_eq!(hops[0].addr, format!("{ANTHROPIC_IP}:443").parse().unwrap());
    assert_eq!(
        hops[0].header("x-api-key").as_deref(),
        Some(ANTHROPIC_CANARY),
        "the client attached the key"
    );
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());
}

/// A Bearer key is attached as `authorization: Bearer <key>`.
#[tokio::test]
async fn a_bearer_key_is_attached_as_authorization() {
    let rig = Rig::standard();
    allowed(
        rig.send(keyed(
            post(account(LLAMA), "http://127.0.0.1:8080/v1/chat/completions"),
            AuthScheme::Bearer,
            LLAMA,
        ))
        .await,
        "owner-typed loopback",
    );
    let hops = rig.transport.hops();
    assert_eq!(
        hops[0].header("authorization").as_deref(),
        Some(format!("Bearer {LLAMA_CANARY}").as_str())
    );
}

/// A key the keychain does not hold is `MissingSecret`, and nothing is sent.
#[tokio::test]
async fn a_missing_key_sends_nothing() {
    let rig = Rig::with(
        Directory::standard().account("keyless", endpoint("https", "api.anthropic.com", 443)),
        roomy(),
    );
    let err = rig
        .send(keyed(
            post(account("keyless"), "https://api.anthropic.com/v1/messages"),
            AuthScheme::XApiKey,
            "keyless",
        ))
        .await
        .err()
        .expect("refused");
    assert_eq!(err, EgressError::MissingSecret("keyless".into()));
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}

/// Account, elsewhere: another host, another port or plain http is refused
/// and audited with the Account id; so is an endpoint the directory pins on
/// a private or LAN address (only loopback literals are an Account's).
#[tokio::test]
async fn an_account_is_refused_anywhere_but_its_endpoint() {
    let rig = Rig::with(
        Directory::standard()
            .account("lan", endpoint("http", "192.168.1.20", 8080))
            .account("private", endpoint("https", "10.0.0.5", 443)),
        roomy(),
    );
    for (url, scheme, host, port) in [
        (
            "https://other.example.net/v1/messages",
            "https",
            "other.example.net",
            "443",
        ),
        (
            "https://api.anthropic.com:8443/v1/messages",
            "https",
            "api.anthropic.com",
            "8443",
        ),
        (
            "http://api.anthropic.com/v1/messages",
            "http",
            "api.anthropic.com",
            "80",
        ),
        (
            "https://api.anthropic.com.example.org/v1/messages",
            "https",
            "api.anthropic.com.example.org",
            "443",
        ),
        (
            "http://127.0.0.1:1234/v1/chat/completions",
            "http",
            "127.0.0.1",
            "1234",
        ),
    ] {
        let before = rig.blocks().len();
        blocked(
            rig.send(keyed(
                post(account(ANTHROPIC), url),
                AuthScheme::XApiKey,
                ANTHROPIC,
            ))
            .await,
            url,
        );
        assert_eq!(rig.blocks().len(), before + 1, "{url}: audited");
        assert_last_block(
            &rig,
            &[
                ("account", ANTHROPIC),
                ("scheme", scheme),
                ("host", host),
                ("port", port),
            ],
            url,
        );
    }
    for (id, url) in [
        ("lan", "http://192.168.1.20:8080/v1/chat/completions"),
        ("private", "https://10.0.0.5/v1/chat/completions"),
    ] {
        blocked(rig.send(post(account(id), url)).await, url);
        assert_last_block(&rig, &[("account", id)], url);
    }
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}

// ----------------------------------------------------------- local Accounts

/// The reserved local Accounts' endpoints are defined once, in am-domain.
#[test]
fn the_reserved_local_accounts_resolve_to_their_well_known_ports() {
    assert_eq!(
        reserved_local_account(LOCAL_LMSTUDIO),
        Some(endpoint("http", "127.0.0.1", 1234))
    );
    assert_eq!(
        reserved_local_account(LOCAL_OLLAMA),
        Some(endpoint("http", "127.0.0.1", 11434))
    );
    assert_eq!(reserved_local_account("anthropic-main"), None);
    assert_eq!(reserved_local_account("local-other"), None);
}

/// Local Account: `local-lmstudio` reaches 127.0.0.1:1234 with no lookup
/// and no key; a seat, another Account, the other local Account or an MCP
/// server is refused that port (the local ports are no longer open to any
/// caller), and so is `localhost` for `local-lmstudio` itself (exact
/// endpoint).
#[tokio::test]
async fn only_the_local_account_reaches_its_port() {
    let rig = Rig::standard();
    allowed(
        rig.send(get(
            account(LOCAL_LMSTUDIO),
            "http://127.0.0.1:1234/v1/models",
        ))
        .await,
        "local-lmstudio",
    );
    allowed(
        rig.send(get(account(LOCAL_OLLAMA), "http://127.0.0.1:11434/api/ps"))
            .await,
        "local-ollama",
    );
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 2);
    assert_eq!(hops[0].addr, "127.0.0.1:1234".parse().unwrap());
    assert_eq!(hops[1].addr, "127.0.0.1:11434".parse().unwrap());
    assert!(
        !hops[0].carries_a_credential(),
        "no key for a reserved local Account"
    );
    assert!(
        rig.resolver.all_lookups().is_empty(),
        "no DNS for a loopback endpoint"
    );

    let url = "http://127.0.0.1:1234/v1/models";
    for (caller, id_key, id) in [
        (seat(SEAT), "seat", SEAT),
        (account(ANTHROPIC), "account", ANTHROPIC),
        (account(LLAMA), "account", LLAMA),
        (account(LOCAL_OLLAMA), "account", LOCAL_OLLAMA),
        (mcp(MCP), "server", MCP),
    ] {
        blocked(rig.send(get(caller, url)).await, id);
        assert_last_block(
            &rig,
            &[
                (id_key, id),
                ("scheme", "http"),
                ("host", "127.0.0.1"),
                ("port", "1234"),
            ],
            id,
        );
    }
    blocked(
        rig.send(get(
            account(LOCAL_LMSTUDIO),
            "http://localhost:1234/v1/models",
        ))
        .await,
        "localhost is not the endpoint's host",
    );
    blocked(
        rig.send(get(
            account(LOCAL_LMSTUDIO),
            "http://127.0.0.1:11434/api/ps",
        ))
        .await,
        "the other local port",
    );
    assert_eq!(rig.transport.hops().len(), 2, "nothing more sent");
}

/// Owner-typed loopback, through the real constructor and a real socket: a
/// compatible Account at `http://127.0.0.1:<port>` reaches that port with
/// its key attached; once `serve` hands the client its own port and it is
/// the same, the Account is refused and the refusal audited.
#[tokio::test]
async fn an_owner_typed_loopback_account_reaches_its_port_but_never_the_apps_own() {
    let server = LoopbackServer::start("ok").await;
    let port = server.port;
    let directory = Directory::empty().account(LLAMA, endpoint("http", "127.0.0.1", port));
    let audit = CollectAudit::new();
    let egress = NetEgress::new(Arc::new(directory), keychain(), audit.clone());
    let url = format!("http://127.0.0.1:{port}/v1/models");

    let resp = egress
        .send(keyed(get(account(LLAMA), &url), AuthScheme::Bearer, LLAMA))
        .await
        .expect("its own endpoint is allowed");
    assert_eq!(resp.status, 200);
    assert_eq!(body_text(resp).await, "ok");
    let heads = server.heads();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(
        heads[0].contains(&format!(
            "authorization: bearer {}",
            LLAMA_CANARY.to_ascii_lowercase()
        )),
        "the client attached the key: {heads:?}"
    );

    // Another port on 127.0.0.1 is not the Account's.
    let other = format!("http://127.0.0.1:{}/v1/models", port.wrapping_add(1).max(1));
    blocked(
        egress
            .send(keyed(
                get(account(LLAMA), &other),
                AuthScheme::Bearer,
                LLAMA,
            ))
            .await,
        "another loopback port",
    );

    // `agentic serve` binds, then gives the client its port.
    egress.set_own_port(port);
    blocked(
        egress
            .send(keyed(get(account(LLAMA), &url), AuthScheme::Bearer, LLAMA))
            .await,
        "the app's own port",
    );
    assert_eq!(server.heads().len(), 1, "nothing more reached the server");
    let blocks: Vec<_> = audit
        .events()
        .into_iter()
        .filter(|e| e.action == "egress.blocked")
        .collect();
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert_block(
        blocks.last().unwrap(),
        &[
            ("account", LLAMA),
            ("scheme", "http"),
            ("host", "127.0.0.1"),
            ("port", port.to_string().as_str()),
        ],
        "own port",
    );
}

/// The app's own port is refused for every Account, the reserved local
/// ones included.
#[tokio::test]
async fn the_apps_own_port_is_refused_even_for_a_reserved_local_account() {
    let rig = Rig::standard();
    rig.egress.set_own_port(1234);
    blocked(
        rig.send(get(
            account(LOCAL_LMSTUDIO),
            "http://127.0.0.1:1234/v1/models",
        ))
        .await,
        "own port",
    );
    rig.egress.set_own_port(8080);
    blocked(
        rig.send(keyed(
            get(account(LLAMA), "http://127.0.0.1:8080/v1/models"),
            AuthScheme::Bearer,
            LLAMA,
        ))
        .await,
        "own port",
    );
    assert!(rig.transport.hops().is_empty(), "nothing sent");
    assert_eq!(rig.blocks().len(), 2, "both audited");
}

// ------------------------------------------------------------------- MCP

/// An MCP server reaches its approved URL's endpoint and nothing else.
#[tokio::test]
async fn an_mcp_server_reaches_only_its_approved_url() {
    let rig = Rig::standard();
    allowed(
        rig.send(post(mcp(MCP), "http://127.0.0.1:3001/mcp")).await,
        "approved",
    );
    assert_eq!(
        rig.transport.hops()[0].addr,
        "127.0.0.1:3001".parse().unwrap()
    );
    for url in [
        "http://127.0.0.1:3002/mcp",
        "https://127.0.0.1:3001/mcp",
        "http://localhost:3001/mcp",
        "https://other.example.net/mcp",
    ] {
        blocked(rig.send(post(mcp(MCP), url)).await, url);
        assert_last_block(&rig, &[("server", MCP)], url);
    }
    assert_eq!(rig.transport.hops().len(), 1, "nothing more sent");
}

// ---------------------------------------------------------- no caller, unknown

/// No caller, or an id the directory does not know: refused and audited
/// (an unknown id is named in the entry); nothing is resolved or sent.
#[tokio::test]
async fn no_caller_or_an_unknown_id_is_refused_and_audited() {
    let rig = Rig::standard();
    blocked(
        rig.send(am_domain::HttpRequest::get("https://docs.example.org/"))
            .await,
        "no caller",
    );
    let blocks = rig.blocks();
    assert_eq!(blocks.len(), 1);
    assert_block(
        &blocks[0],
        &[
            ("scheme", "https"),
            ("host", "docs.example.org"),
            ("port", "443"),
        ],
        "no caller",
    );
    for key in ["seat", "account", "server"] {
        assert!(
            !blocks[0].args.contains_key(key),
            "no caller, so no {key}: {:?}",
            blocks[0]
        );
    }

    for (caller, key, id, url) in [
        (seat("ghost"), "seat", "ghost", "https://docs.example.org/"),
        (
            account("ghost-account"),
            "account",
            "ghost-account",
            "https://api.anthropic.com/v1/messages",
        ),
        (
            mcp("ghost-server"),
            "server",
            "ghost-server",
            "http://127.0.0.1:3001/mcp",
        ),
    ] {
        blocked(rig.send(get(caller, url)).await, id);
        assert_last_block(&rig, &[(key, id)], id);
    }
    assert!(rig.transport.hops().is_empty(), "nothing sent");
    assert!(rig.resolver.all_lookups().is_empty(), "nothing resolved");
}

/// The request carries ids only: the directory alone answers, so the client
/// holds no allow-list of its own and an empty directory reaches nothing.
#[tokio::test]
async fn an_empty_directory_reaches_nothing() {
    let rig = Rig::with(Directory::empty(), roomy());
    for (caller, url) in [
        (seat(SEAT), "https://docs.example.org/"),
        (account(ANTHROPIC), "https://api.anthropic.com/v1/messages"),
        (mcp(MCP), "http://127.0.0.1:3001/mcp"),
    ] {
        blocked(rig.send(get(caller, url)).await, url);
    }
    assert!(rig.transport.hops().is_empty());
    let directory: &dyn EgressDirectory = &Directory::empty();
    assert_eq!(directory.seat_hosts(SEAT), None);
}

// ------------------------------------------------- story 2.2 review fixes

/// A credential belongs only to its own Account (review, high): a request
/// carrying a key reference is refused, before any Keychain read, lookup or
/// hop, and audited, unless its caller is that very Account, answered by
/// the directory and not a reserved local one.
#[tokio::test]
async fn a_credential_goes_only_with_its_own_account() {
    let rig = Rig::standard();
    for (what, request, id_key, id) in [
        (
            "a seat carrying an Account's key",
            keyed(
                get(seat(SEAT), "https://docs.example.org/a"),
                AuthScheme::Bearer,
                SEARCH_KEY,
            ),
            "seat",
            SEAT,
        ),
        (
            "an MCP server carrying an Account's key",
            keyed(
                post(mcp(MCP), "http://127.0.0.1:3001/mcp"),
                AuthScheme::Bearer,
                LLAMA,
            ),
            "server",
            MCP,
        ),
        (
            "an Account carrying another Account's key",
            keyed(
                post(account(LLAMA), "http://127.0.0.1:8080/v1/chat/completions"),
                AuthScheme::Bearer,
                ANTHROPIC,
            ),
            "account",
            LLAMA,
        ),
        (
            "a reserved local Account carrying a cloud key",
            keyed(
                get(account(LOCAL_LMSTUDIO), "http://127.0.0.1:1234/v1/models"),
                AuthScheme::Bearer,
                ANTHROPIC,
            ),
            "account",
            LOCAL_LMSTUDIO,
        ),
        (
            "a reserved local Account carrying its own id as a key",
            keyed(
                get(account(LOCAL_LMSTUDIO), "http://127.0.0.1:1234/v1/models"),
                AuthScheme::Bearer,
                LOCAL_LMSTUDIO,
            ),
            "account",
            LOCAL_LMSTUDIO,
        ),
    ] {
        let words = blocked(rig.send(request).await, what);
        assert!(
            words.contains("the credential does not belong to this caller"),
            "{what}: {words}"
        );
        assert_last_block(&rig, &[(id_key, id)], what);
        assert_eq!(
            args(rig.blocks().last().unwrap())
                .get("reason")
                .map(String::as_str),
            Some("the credential does not belong to this caller"),
            "{what}"
        );
    }
    assert_eq!(rig.secrets.reads(), 0, "no Keychain read");
    assert!(rig.transport.hops().is_empty(), "nothing sent");
    assert!(rig.resolver.all_lookups().is_empty(), "nothing resolved");
}

/// The reserved local ports belong to their own reserved Account (review,
/// low): an MCP server or another Account pinned there is refused.
#[tokio::test]
async fn the_reserved_local_ports_are_refused_to_every_other_caller() {
    let rig = Rig::with(
        Directory::standard()
            .mcp("on-lmstudio", endpoint("http", "127.0.0.1", 1234))
            .account("on-ollama", endpoint("http", "127.0.0.1", 11434)),
        roomy(),
    );
    blocked(
        rig.send(post(mcp("on-lmstudio"), "http://127.0.0.1:1234/mcp"))
            .await,
        "an MCP server on LM Studio's port",
    );
    assert_last_block(&rig, &[("server", "on-lmstudio"), ("port", "1234")], "mcp");
    blocked(
        rig.send(post(
            account("on-ollama"),
            "http://127.0.0.1:11434/v1/chat/completions",
        ))
        .await,
        "an Account on Ollama's port",
    );
    assert_last_block(
        &rig,
        &[("account", "on-ollama"), ("port", "11434")],
        "account",
    );
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}

/// The app's own port recorded in the project's runtime folder (review,
/// medium) reaches the client through the directory: refused to every
/// Account and MCP server, with no Keychain read.
#[tokio::test]
async fn the_own_port_the_directory_names_is_refused() {
    let rig = Rig::with(Directory::standard().own_port(8080), roomy());
    blocked(
        rig.send(keyed(
            get(account(LLAMA), "http://127.0.0.1:8080/v1/models"),
            AuthScheme::Bearer,
            LLAMA,
        ))
        .await,
        "an Account on the app's own port",
    );
    assert_last_block(&rig, &[("account", LLAMA), ("port", "8080")], "own port");
    let rig = Rig::with(Directory::standard().own_port(3001), roomy());
    blocked(
        rig.send(post(mcp(MCP), "http://127.0.0.1:3001/mcp")).await,
        "an MCP server on the app's own port",
    );
    assert_eq!(rig.secrets.reads(), 0);
    assert!(rig.transport.hops().is_empty(), "nothing sent");
}
