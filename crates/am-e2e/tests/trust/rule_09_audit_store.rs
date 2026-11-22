//! RULE-9 trust test (story 2.2, AD-12, AD-25): a blocked call reaches the
//! project's audit log. am-app's `egress::StoreAuditSink` writes each
//! `egress.blocked` entry through the store's writer, actor
//! `System(am-net)`, with the destination's scheme, host and port, the
//! reason and the seat, Account or server id in the entry's `args`, and
//! nothing of the path, query, headers or body. The chain still verifies.
//!
//! On a real store in a temp folder, with the scripted resolver and
//! transport; no network. Written first.
#![forbid(unsafe_code)]

#[path = "support/egress.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;

use am_app::egress::StoreAuditSink;
use am_domain::{AuditSink, AuthScheme, RuntimeLayout};
use am_store::Store;
use serde_json::{Value, json};
use support::*;

fn open(root: &Path) -> Store {
    let (store, _) = am_store::open_runtime(&RuntimeLayout::at(root.join("rt")), "test-mac".into())
        .expect("the store opens");
    store
}

fn export(store: &Store) -> Vec<Value> {
    let mut out = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn a_blocked_call_is_written_to_the_projects_audit_log() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let sink: Arc<dyn AuditSink> = Arc::new(StoreAuditSink::new(store.writer()));
    let rig = Rig::build(Directory::standard(), roomy(), Some(sink));

    blocked(
        rig.send(get(
            seat(SEAT),
            "https://other.example.net/canarypath?canaryquery=canaryvalue",
        ))
        .await,
        "seat, unlisted",
    );
    blocked(
        rig.send(keyed(
            post(
                account(ANTHROPIC),
                "https://api.anthropic.com:8443/v1/canarypath",
            ),
            AuthScheme::XApiKey,
            ANTHROPIC,
        ))
        .await,
        "Account, off its port",
    );

    let entries = export(&store);
    let blocks: Vec<&Value> = entries
        .iter()
        .filter(|e| e["action"] == "egress.blocked")
        .collect();
    assert_eq!(blocks.len(), 2, "{entries:#?}");
    for entry in &blocks {
        assert_eq!(
            entry["actor"],
            json!({"kind": "system", "component": "am-net"}),
            "{entry}"
        );
        let args = &entry["args"];
        for key in ["scheme", "host", "port", "reason"] {
            assert!(
                args[key].as_str().is_some_and(|v| !v.is_empty()),
                "args.{key}: {entry}"
            );
        }
    }
    assert_eq!(blocks[0]["args"]["seat"], SEAT);
    assert_eq!(blocks[0]["args"]["host"], "other.example.net");
    assert_eq!(blocks[0]["args"]["port"], "443");
    assert_eq!(blocks[0]["args"]["scheme"], "https");
    assert_eq!(blocks[1]["args"]["account"], ANTHROPIC);
    assert_eq!(blocks[1]["args"]["host"], "api.anthropic.com");
    assert_eq!(blocks[1]["args"]["port"], "8443");

    let text = format!("{entries:?}");
    for canary in ["canarypath", "canaryquery", "canaryvalue", ANTHROPIC_CANARY] {
        assert!(!text.contains(canary), "{canary} in the audit log: {text}");
    }
    assert_eq!(
        store.reader().unwrap().verify_audit_chain().unwrap(),
        None,
        "the chain verifies"
    );
}

/// Story 2.2 review (medium): the store-backed sink writes only
/// `egress.blocked` entries; any other event handed to it is not written
/// (it cannot become a `config.*` note that confirms an Account).
#[test]
fn the_store_sink_writes_only_egress_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let sink = StoreAuditSink::new(store.writer());
    let before = export(&store).len();
    for action in ["config.widened", "folder.read", "egress.allowed"] {
        sink.record(am_domain::AuditEvent {
            actor: am_domain::Actor::System {
                component: "am-net".into(),
            },
            action: action.into(),
            target: "anthropic-main".into(),
            outcome: "ok".into(),
            hash: None,
            args: [("kind".to_string(), "account_added".to_string())]
                .into_iter()
                .collect(),
        });
    }
    assert_eq!(export(&store).len(), before, "nothing written");
}
