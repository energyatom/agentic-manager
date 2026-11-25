//! RULE-9 trust test (story 2.10; AD-12, AD-15 *Keyless loopback
//! Account*, *Key use*; owner Q4, 2026-11-25): a keyless Account's calls.
//!
//! - A directory states an Account's key state in its stamp
//!   (`AccountStamp::keyed`); the default answer of a directory that keeps
//!   no key state is keyed, the safe side.
//! - A request for a keyless Account with no credential reference reaches
//!   its `http://127.0.0.1:<port>` endpoint with no credential header,
//!   no name looked up and no Keychain call (the ticket's "no
//!   `Authorization` header, to that port only", on the wire).
//! - A request for a keyless Account that carries a credential reference
//!   is refused before any Keychain call, and the refusal is audited
//!   (`egress.blocked`, the Account named, the reason saying it has no
//!   key): a key a removed Account left under the same id is never read
//!   or sent.
//! - Its redirects follow the rules every Account has (story 2.2): off its
//!   endpoint (another port on this Mac, another host) refused and
//!   audited; a POST never redirected.
//!
//! On am-e2e's scripted resolver and transport, and once through the real
//! constructor to a loopback server; no network. Written first.
#![forbid(unsafe_code)]

#[path = "support/egress.rs"]
mod support;

use std::sync::{Arc, Mutex};

use am_domain::net::{AccountStamp, EgressDirectory, Endpoint};
use am_domain::{
    AuthScheme, EgressClient, HostPattern, HttpMethod, SecretError, SecretStore, SecretString,
};
use am_net::{NetEgress, Seams};
use am_testkit::CollectAudit;
use support::*;

/// The keyless Account's endpoint, as `LLAMA` is pinned in the standard
/// directory.
const LOOP_COMPLETIONS: &str = "http://127.0.0.1:8080/v1/chat/completions";
const LOOP_MODELS: &str = "http://127.0.0.1:8080/v1/models";

/// The standard directory, with the Accounts named in `keyless` stated as
/// keyless in their stamp. Every other answer is the standard one.
struct KeylessDirectory {
    inner: Directory,
    keyless: Vec<String>,
}

impl KeylessDirectory {
    fn standard(keyless: &[&str]) -> KeylessDirectory {
        KeylessDirectory::over(Directory::standard(), keyless)
    }

    fn over(inner: Directory, keyless: &[&str]) -> KeylessDirectory {
        KeylessDirectory {
            inner,
            keyless: keyless.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl EgressDirectory for KeylessDirectory {
    fn seat_hosts(&self, seat: &str) -> Option<Vec<HostPattern>> {
        self.inner.seat_hosts(seat)
    }
    fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
        self.inner.account_endpoint(account)
    }
    fn mcp_endpoint(&self, server: &str) -> Option<Endpoint> {
        self.inner.mcp_endpoint(server)
    }
    fn own_ports(&self) -> Vec<u16> {
        self.inner.own_ports()
    }
    fn account_stamp(&self, account: &str) -> Option<AccountStamp> {
        self.inner
            .account_endpoint(account)
            .map(|endpoint| AccountStamp {
                endpoint,
                version: "file:1".into(),
                keyed: !self.keyless.iter().any(|k| k == account),
            })
    }
}

/// A Keychain that records every call (`get:<id>`, `set:<id>`,
/// `delete:<id>`), holding the standard canaries.
struct Recording {
    inner: Arc<am_testkit::FakeSecretStore>,
    calls: Mutex<Vec<String>>,
}

impl Recording {
    fn standard() -> Arc<Recording> {
        Arc::new(Recording {
            inner: keychain(),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl SecretStore for Recording {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError> {
        self.record(format!("get:{account}"));
        self.inner.get(account)
    }
    fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError> {
        self.record(format!("set:{account}"));
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.record(format!("delete:{account}"));
        self.inner.delete(account)
    }
}

/// The client under test over `LLAMA` as a keyless Account, with the
/// standard canaries in the Keychain (`LLAMA_CANARY` among them: a key a
/// removed keyed Account of that id left behind).
struct KeylessRig {
    egress: NetEgress,
    secrets: Arc<Recording>,
    resolver: Arc<ScriptedResolver>,
    transport: Arc<ScriptedTransport>,
    audit: Arc<CollectAudit>,
}

impl KeylessRig {
    fn new() -> KeylessRig {
        let resolver = ScriptedResolver::standard();
        resolver.answer("elsewhere.example", &[OTHER_IP]);
        let transport = ScriptedTransport::new();
        let secrets = Recording::standard();
        let audit = CollectAudit::new();
        let egress = NetEgress::with_seams(
            Arc::new(KeylessDirectory::standard(&[LLAMA])),
            secrets.clone(),
            audit.clone(),
            Seams {
                resolver: resolver.clone(),
                transport: transport.clone(),
                limits: roomy(),
            },
        );
        KeylessRig {
            egress,
            secrets,
            resolver,
            transport,
            audit,
        }
    }

    fn blocks(&self) -> Vec<am_domain::AuditEvent> {
        self.audit
            .events()
            .into_iter()
            .filter(|e| e.action == "egress.blocked")
            .collect()
    }

    fn last_block(&self, what: &str) -> am_domain::AuditEvent {
        self.blocks()
            .last()
            .cloned()
            .unwrap_or_else(|| panic!("{what}: the refusal is audited"))
    }

    /// True when no hop carried a credential header or any canary.
    fn no_credential_on_any_hop(&self) -> bool {
        self.transport.hops().iter().all(|h| {
            !h.carries_a_credential()
                && h.headers.iter().all(|(_, v)| {
                    [LLAMA_CANARY, ANTHROPIC_CANARY, SEARCH_CANARY]
                        .iter()
                        .all(|c| !v.contains(c))
                })
        })
    }
}

/// Matrix row 12 (first half), and the ticket's verify: a keyless
/// Account's POST and GET with no credential reference reach
/// 127.0.0.1:8080 only, with no credential header, no name looked up and
/// no Keychain call; nothing is refused. Through the real constructor to a
/// loopback server, the request that arrives carries no `Authorization`
/// header.
#[tokio::test]
async fn a_keyless_accounts_request_carries_no_credential_and_reads_no_key() {
    let rig = KeylessRig::new();
    for request in [
        post(account(LLAMA), LOOP_COMPLETIONS),
        get(account(LLAMA), LOOP_MODELS),
    ] {
        let resp = allowed(rig.egress.send(request).await, "a keyless call");
        assert_eq!(resp.status, 200);
    }
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 2);
    for hop in &hops {
        assert_eq!(hop.addr, "127.0.0.1:8080".parse().unwrap());
        assert!(!hop.carries_a_credential(), "{:?}", hop.headers);
    }
    assert!(rig.no_credential_on_any_hop());
    assert!(rig.secrets.calls().is_empty(), "{:?}", rig.secrets.calls());
    assert!(
        rig.resolver.all_lookups().is_empty(),
        "{:?}",
        rig.resolver.all_lookups()
    );
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());

    // The real constructor, to a server on 127.0.0.1.
    let server = LoopbackServer::start("ok").await;
    let directory = KeylessDirectory::over(
        Directory::empty().account(LLAMA, endpoint("http", "127.0.0.1", server.port)),
        &[LLAMA],
    );
    let secrets = Recording::standard();
    let egress = NetEgress::new(Arc::new(directory), secrets.clone(), CollectAudit::new());
    let url = format!("http://127.0.0.1:{}/v1/chat/completions", server.port);
    let resp = egress
        .send(post(account(LLAMA), &url))
        .await
        .expect("its own endpoint is reached");
    assert_eq!(body_text(resp).await, "ok");
    let heads = server.heads();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(
        heads[0].starts_with("post /v1/chat/completions"),
        "{heads:?}"
    );
    assert!(!heads[0].contains("authorization"), "{heads:?}");
    assert!(!heads[0].contains("x-api-key"), "{heads:?}");
    assert!(
        !heads[0].contains(&LLAMA_CANARY.to_ascii_lowercase()),
        "{heads:?}"
    );
    assert!(secrets.calls().is_empty(), "{:?}", secrets.calls());
}

/// Matrix row 13 (owner Q4): a request for the keyless Account carrying a
/// credential reference to its id, in either scheme, for POST and GET, is
/// refused before any Keychain call, though the Keychain holds a key under
/// that id; nothing is sent, and each refusal is audited `egress.blocked`
/// naming the Account, its endpoint and a reason saying it has no key. A
/// keyed Account beside it still gets its key, and a directory that keeps
/// no key state answers keyed.
#[tokio::test]
async fn a_credential_for_a_keyless_account_is_refused_before_any_keychain_read() {
    let rig = KeylessRig::new();
    let mut n = 0;
    for scheme in [AuthScheme::Bearer, AuthScheme::XApiKey] {
        for request in [
            post(account(LLAMA), LOOP_COMPLETIONS),
            get(account(LLAMA), LOOP_MODELS),
        ] {
            let what = format!("{scheme:?} {:?}", request.method);
            let words = blocked(rig.egress.send(keyed(request, scheme, LLAMA)).await, &what);
            n += 1;
            assert!(!words.contains(LLAMA_CANARY), "{what}: {words}");
            assert!(
                rig.secrets.calls().is_empty(),
                "{what}: no Keychain call of any kind: {:?}",
                rig.secrets.calls()
            );
            assert!(rig.transport.hops().is_empty(), "{what}: nothing sent");
            let block = rig.last_block(&what);
            assert_block(
                &block,
                &[
                    ("account", LLAMA),
                    ("scheme", "http"),
                    ("host", "127.0.0.1"),
                    ("port", "8080"),
                ],
                &what,
            );
            let reason = block.args.get("reason").cloned().unwrap_or_default();
            assert!(
                reason.contains("has no key"),
                "{what}: the reason says the Account has no key: {reason}"
            );
            assert_eq!(rig.blocks().len(), n, "{what}: one entry per refusal");
        }
    }

    // A keyed Account beside it: its key is read and attached.
    allowed(
        rig.egress
            .send(keyed(
                post(account(ANTHROPIC), "https://api.anthropic.com/v1/messages"),
                AuthScheme::XApiKey,
                ANTHROPIC,
            ))
            .await,
        "the keyed Account",
    );
    assert_eq!(rig.secrets.calls(), vec![format!("get:{ANTHROPIC}")]);
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 1);
    assert_eq!(
        hops[0].header("x-api-key").as_deref(),
        Some(ANTHROPIC_CANARY)
    );

    // The default stamp of a directory with no key state: keyed.
    let plain = Directory::standard();
    let stamp = plain.account_stamp(LLAMA).expect("answered");
    assert!(stamp.keyed, "the safe side: {stamp:?}");
    let keyless = KeylessDirectory::standard(&[LLAMA]);
    assert!(!keyless.account_stamp(LLAMA).expect("answered").keyed);
}

/// Matrix row 12: a keyless Account's GET redirected off its endpoint
/// (another port on this Mac, another host) is refused and audited with
/// the Account and the hop's destination, with no name looked up for the
/// refused hop; a POST is never redirected, even within the endpoint; a
/// GET redirect within the endpoint is followed. No hop carries a
/// credential and the Keychain is never called.
#[tokio::test]
async fn a_keyless_accounts_redirect_off_its_endpoint_is_refused_and_audited() {
    for (target, host, port) in [
        ("http://127.0.0.1:9090/v1/models", "127.0.0.1", "9090"),
        (
            "https://elsewhere.example/v1/models",
            "elsewhere.example",
            "443",
        ),
        ("http://127.0.0.1:1234/v1/models", "127.0.0.1", "1234"),
    ] {
        let rig = KeylessRig::new();
        rig.transport.on(LOOP_MODELS, Reply::redirect(302, target));
        blocked(
            rig.egress.send(get(account(LLAMA), LOOP_MODELS)).await,
            target,
        );
        assert_eq!(
            rig.transport.hops().len(),
            1,
            "{target}: the hop was not made"
        );
        let block = rig.last_block(target);
        assert_block(
            &block,
            &[("account", LLAMA), ("host", host), ("port", port)],
            target,
        );
        assert_eq!(rig.resolver.lookups("elsewhere.example"), 0, "{target}");
        assert!(rig.no_credential_on_any_hop(), "{target}");
        assert!(rig.secrets.calls().is_empty(), "{target}");
    }

    // A POST is never redirected.
    let rig = KeylessRig::new();
    rig.transport.on(
        LOOP_COMPLETIONS,
        Reply::redirect(307, "http://127.0.0.1:8080/v1/chat/completions/"),
    );
    blocked(
        rig.egress
            .send(post(account(LLAMA), LOOP_COMPLETIONS))
            .await,
        "a POST redirect",
    );
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 1);
    assert_eq!(hops[0].method, HttpMethod::Post);
    assert_block(
        &rig.last_block("a POST redirect"),
        &[("account", LLAMA), ("host", "127.0.0.1"), ("port", "8080")],
        "a POST redirect",
    );

    // A GET redirect within the endpoint is followed, with no credential.
    let rig = KeylessRig::new();
    rig.transport.on(
        LOOP_MODELS,
        Reply::redirect(301, "http://127.0.0.1:8080/v1/models/"),
    );
    allowed(
        rig.egress.send(get(account(LLAMA), LOOP_MODELS)).await,
        "within the endpoint",
    );
    let hops = rig.transport.hops();
    assert_eq!(hops.len(), 2);
    assert!(
        hops.iter()
            .all(|h| h.addr == "127.0.0.1:8080".parse().unwrap())
    );
    assert!(rig.no_credential_on_any_hop());
    assert!(rig.secrets.calls().is_empty());
    assert!(rig.blocks().is_empty(), "{:?}", rig.blocks());
}

// ======================================== review round 2 (2026-11-25)

/// A directory whose stamp of `LLAMA` changes between takes: it answers
/// each stamp in `stamps` in turn, then repeats the last, and counts every
/// take. `LLAMA`'s endpoint is 127.0.0.1:8080 throughout.
struct Turning {
    stamps: Mutex<std::collections::VecDeque<AccountStamp>>,
    last: Mutex<Option<AccountStamp>>,
    takes: Mutex<usize>,
}

impl Turning {
    fn new(stamps: Vec<AccountStamp>) -> Turning {
        Turning {
            stamps: Mutex::new(stamps.into()),
            last: Mutex::new(None),
            takes: Mutex::new(0),
        }
    }

    fn takes(&self) -> usize {
        *self.takes.lock().unwrap()
    }
}

impl EgressDirectory for Turning {
    fn seat_hosts(&self, _seat: &str) -> Option<Vec<HostPattern>> {
        None
    }
    fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
        (account == LLAMA).then(|| endpoint("http", "127.0.0.1", 8080))
    }
    fn mcp_endpoint(&self, _server: &str) -> Option<Endpoint> {
        None
    }
    fn account_stamp(&self, account: &str) -> Option<AccountStamp> {
        if account != LLAMA {
            return None;
        }
        *self.takes.lock().unwrap() += 1;
        let next = self.stamps.lock().unwrap().pop_front();
        let mut last = self.last.lock().unwrap();
        if let Some(next) = next {
            *last = Some(next);
        }
        last.clone()
    }
}

/// Review K8 (S6): am-net's second keyless refusal, inside the retry loop.
/// The first stamp of the Account says keyed, but for another port than
/// the request's, so no key is read on the first pass; the stamp taken
/// again says keyless, at the request's endpoint. The call is refused on
/// the second pass, before any Keychain call (though the Keychain holds a
/// key under the id), nothing is sent, and the refusal is audited
/// `egress.blocked` naming the Account and the reason. Without the check
/// in the loop, that pass would read the key and send it.
#[tokio::test]
async fn a_stamp_that_turns_keyless_between_takes_is_refused_before_any_keychain_read() {
    let stamp = |port: u16, keyed: bool| AccountStamp {
        endpoint: endpoint("http", "127.0.0.1", port),
        version: format!("file:{port}"),
        keyed,
    };
    let directory = Arc::new(Turning::new(vec![stamp(8081, true), stamp(8080, false)]));
    let secrets = Recording::standard();
    let transport = ScriptedTransport::new();
    let audit = CollectAudit::new();
    let egress = NetEgress::with_seams(
        directory.clone(),
        secrets.clone(),
        audit.clone(),
        Seams {
            resolver: ScriptedResolver::standard(),
            transport: transport.clone(),
            limits: roomy(),
        },
    );

    let words = blocked(
        egress
            .send(keyed(
                post(account(LLAMA), LOOP_COMPLETIONS),
                AuthScheme::Bearer,
                LLAMA,
            ))
            .await,
        "a stamp turned keyless",
    );

    assert!(!words.contains(LLAMA_CANARY), "{words}");
    assert!(
        secrets.calls().is_empty(),
        "no Keychain call of any kind: {:?}",
        secrets.calls()
    );
    assert!(transport.hops().is_empty(), "nothing sent");
    assert_eq!(
        directory.takes(),
        2,
        "refused at the stamp taken again inside the loop, not at the first"
    );
    let blocks: Vec<_> = audit
        .events()
        .into_iter()
        .filter(|e| e.action == "egress.blocked")
        .collect();
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_block(
        &blocks[0],
        &[
            ("account", LLAMA),
            ("scheme", "http"),
            ("host", "127.0.0.1"),
            ("port", "8080"),
        ],
        "a stamp turned keyless",
    );
    let reason = blocks[0].args.get("reason").cloned().unwrap_or_default();
    assert!(reason.contains("has no key"), "{reason}");

    // The control: had the stamp taken again stayed keyed, that same second
    // pass reads the key and sends it. So the refusal above is the loop's.
    let directory = Arc::new(Turning::new(vec![stamp(8081, true), stamp(8080, true)]));
    let secrets = Recording::standard();
    let transport = ScriptedTransport::new();
    let egress = NetEgress::with_seams(
        directory.clone(),
        secrets.clone(),
        CollectAudit::new(),
        Seams {
            resolver: ScriptedResolver::standard(),
            transport: transport.clone(),
            limits: roomy(),
        },
    );
    allowed(
        egress
            .send(keyed(
                post(account(LLAMA), LOOP_COMPLETIONS),
                AuthScheme::Bearer,
                LLAMA,
            ))
            .await,
        "a stamp that stays keyed",
    );
    assert_eq!(secrets.calls(), vec![format!("get:{LLAMA}")]);
    let hops = transport.hops();
    assert_eq!(hops.len(), 1);
    assert!(hops[0].carries_a_credential(), "the control sends the key");
}
