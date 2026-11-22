//! Story 2.14 (am-config side; AD-13, AD-15 *Change*, *Open*): the scan
//! and the edit `agentic accounts change` builds on.
//!
//! - `owner_children(root)` keeps its lists of children that run as the
//!   owner (`enabled`: stdio MCP servers and Sources with `render: true`;
//!   `broken`) and gains `loopback_http`: one entry, in words naming the
//!   server's id, per enabled `transport: http` MCP server whose `url` host
//!   is `127.0.0.1`, `[::1]` or `localhost` (http or https, any port). Only
//!   the Account commands read it, so `decisions_locked` (`agentic run`,
//!   `--take-over`, `--confirm-move`) and `words()` keep the stdio rule. An
//!   https server on another host is in no list.
//! - `change_account_edit(&AccountsFile, id, provider, base_url)` sets one
//!   Account's provider and `base_url` (none for a provider with a fixed
//!   endpoint), keeping its id, name and `secret_ref`; `None` when the file
//!   has no such Account.
//!
//! Tests work in temporary folders only. Written before the list and the
//! edit exist.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use am_config::{
    ConfigCreate, ConfigWriter, USER_ACCOUNTS_FILE, accounts_file, change_account_edit,
    decisions_locked, new_accounts_file, owner_children,
};
use am_domain::Actor;
use am_testkit::InMemoryConfigHistory;
use common::write;

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\n";

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn server(id: &str, transport: &str) -> String {
    format!(
        "  - id: {id}\n{transport}    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n"
    )
}

fn http(id: &str, url: &str) -> String {
    server(id, &format!("    transport: http\n    url: {url}\n"))
}

/// A project holding `servers` in its `tools.yaml`.
fn project(servers: &[String]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "company.yaml", COMPANY);
    write(
        dir.path(),
        "tools.yaml",
        &format!("schema_version: 2\nmcp_servers:\n{}", servers.concat()),
    );
    dir
}

/// Loopback-HTTP list: each enabled http MCP server on this Mac is listed
/// apart, naming its id; none is in `enabled`, `broken` or `words()`, so
/// `decisions_locked` stays `None`.
#[test]
fn loopback_http_servers_are_listed_apart_from_the_stdio_rule() {
    let p = project(&[
        http("on-loopback", "http://127.0.0.1:3001/mcp"),
        http("on-localhost", "https://localhost/mcp"),
        http("on-v6", "https://[::1]:8443/mcp"),
        http("on-localhost-port", "https://localhost:9443/mcp"),
    ]);
    let found = owner_children(p.path());
    assert!(found.enabled.is_empty(), "{found:?}");
    assert!(found.broken.is_empty(), "{found:?}");
    assert_eq!(found.loopback_http.len(), 4, "{found:?}");
    for id in ["on-loopback", "on-localhost", "on-v6", "on-localhost-port"] {
        assert!(
            found.loopback_http.iter().any(|w| w.contains(id)),
            "{id}: {found:?}"
        );
    }
    assert_eq!(found.words(), None, "the stdio rule's words");
    assert_eq!(decisions_locked(p.path()), None);
}

/// Remote HTTP: an https server on another host is in no list.
#[test]
fn a_remote_http_server_is_in_no_list() {
    let p = project(&[http("far", "https://mcp.example/mcp")]);
    let found = owner_children(p.path());
    assert!(found.loopback_http.is_empty(), "{found:?}");
    assert!(found.enabled.is_empty(), "{found:?}");
    assert_eq!(decisions_locked(p.path()), None);
}

/// Both kinds at once: the stdio server stays in `enabled` (and locks the
/// decisions); the loopback one is in `loopback_http` only.
#[test]
fn a_stdio_server_stays_in_enabled_beside_a_loopback_one() {
    let p = project(&[
        server(
            "notes",
            "    transport: stdio\n    command: example-notes-server\n",
        ),
        http("loop-notes", "http://127.0.0.1:3001/mcp"),
    ]);
    let found = owner_children(p.path());
    assert_eq!(found.enabled.len(), 1, "{found:?}");
    assert!(found.enabled[0].contains("notes"), "{found:?}");
    assert!(!found.enabled[0].contains("loop-notes"), "{found:?}");
    assert_eq!(found.loopback_http.len(), 1, "{found:?}");
    assert!(found.loopback_http[0].contains("loop-notes"), "{found:?}");
    assert!(decisions_locked(p.path()).is_some());
}

/// The change edit: a compatible Account moved to another address keeps
/// its name and `secret_ref`; moved to a fixed-endpoint provider it loses
/// its `base_url`; the file stays valid; an unknown id gives no edit.
#[test]
fn the_change_edit_sets_provider_and_base_url_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let w =
        ConfigWriter::for_user_accounts(root, Arc::new(InMemoryConfigHistory::default())).unwrap();
    w.create(
        owner(),
        ConfigCreate {
            file: USER_ACCOUNTS_FILE.into(),
            bytes: new_accounts_file(
                "router",
                "openai-compatible",
                Some("Router"),
                Some("https://openrouter.example/api/v1"),
            )
            .unwrap(),
            audit: Vec::new(),
        },
    )
    .unwrap();

    let now = accounts_file(root);
    let edit = change_account_edit(
        &now,
        "router",
        "openai-compatible",
        Some("https://router2.example/api/v1"),
    )
    .expect("an edit for router");
    w.apply_with_audit(owner(), edit, now.sha256.as_deref().unwrap(), Vec::new())
        .unwrap();
    let a = accounts_file(root).accounts.remove(0);
    assert_eq!(
        (
            a.id.as_str(),
            a.provider.as_str(),
            a.name.as_deref(),
            a.base_url.as_deref(),
            a.secret_ref.as_deref()
        ),
        (
            "router",
            "openai-compatible",
            Some("Router"),
            Some("https://router2.example/api/v1"),
            Some("router")
        )
    );

    let now = accounts_file(root);
    let edit = change_account_edit(&now, "router", "anthropic", None).expect("an edit");
    w.apply_with_audit(owner(), edit, now.sha256.as_deref().unwrap(), Vec::new())
        .unwrap();
    let file = accounts_file(root);
    assert_eq!(file.problem, None);
    assert!(file.entry_errors("router").is_empty(), "{file:?}");
    let a = &file.accounts[0];
    assert_eq!(a.provider, "anthropic");
    assert_eq!(a.base_url, None);
    assert_eq!(a.name.as_deref(), Some("Router"));
    assert_eq!(a.secret_ref.as_deref(), Some("router"));

    assert!(change_account_edit(&file, "missing", "anthropic", None).is_none());
}

/// Review fix Adversarial 5 (2026-11-23): the loopback-HTTP list uses the
/// same "this Mac" predicate as `compatible_base_url`: every loopback and
/// unspecified address (IPv4-mapped included) and `localhost` or
/// `*.localhost`, with or without a trailing dot. A remote host is not in it.
#[test]
fn every_form_that_reaches_this_mac_is_in_the_loopback_http_list() {
    let urls = [
        ("unspecified-v4", "https://0.0.0.0/mcp"),
        ("unspecified-v6", "https://[::]/mcp"),
        ("mapped", "https://[::ffff:127.0.0.1]/mcp"),
        ("other-127", "https://127.0.0.2/mcp"),
        ("dotted", "https://localhost./mcp"),
        ("sub", "https://x.localhost/mcp"),
        ("sub-dotted", "https://x.localhost./mcp"),
    ];
    let servers: Vec<String> = urls.iter().map(|(id, url)| http(id, url)).collect();
    let p = project(&servers);
    let found = owner_children(p.path());
    assert!(
        found.broken.is_empty(),
        "the loader accepts them: {found:?}"
    );
    for (id, url) in urls {
        assert!(
            found
                .loopback_http
                .iter()
                .any(|w| w.contains(&format!("`{id}`"))),
            "{url}: {found:?}"
        );
    }
    let p = project(&[http("far", "https://localhost.example/mcp")]);
    assert!(owner_children(p.path()).loopback_http.is_empty());
}
