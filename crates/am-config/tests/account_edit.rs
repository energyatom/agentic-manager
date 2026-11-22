//! Story 2.1 (review fix): the interim CLI Account edit is built from a
//! read of `models/accounts.yaml` and names list items by their number, so
//! the writer applies it only while the file still holds the bytes it was
//! built on (`AccountsFile::sha256`, given to
//! `ConfigWriter::apply_with_audit`); a file that changed meanwhile is
//! refused as `ChangedOnDisk` and nothing is written. Written before the
//! check exists.
//!
//! Story 2.3: the Account the edit writes may carry a `base_url` (an
//! OpenAI-compatible Account's endpoint), beside `secret_ref` and never a
//! key; `add_account_edit` and `new_accounts_file` take it as their last
//! argument, `None` for a provider with one fixed endpoint. Written before
//! the argument exists.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use am_config::{
    ACCOUNTS_FILE, ConfigCreate, ConfigWriter, Severity, WriteError, accounts_file,
    add_account_edit, load, new_accounts_file, remove_account_edit,
};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::copy_of;
use sha2::{Digest, Sha256};

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn ids(root: &std::path::Path) -> Vec<String> {
    accounts_file(root).ids.into_iter().flatten().collect()
}

/// The read carries the sha256 of the bytes it saw (none for no file).
#[test]
fn the_read_names_the_bytes_it_was_made_on() {
    let dir = copy_of("valid");
    let root = dir.path();
    let bytes = std::fs::read(root.join(ACCOUNTS_FILE)).unwrap();
    assert_eq!(
        accounts_file(root).sha256,
        Some(hex::encode(Sha256::digest(&bytes)))
    );
    std::fs::remove_file(root.join(ACCOUNTS_FILE)).unwrap();
    assert_eq!(accounts_file(root).sha256, None);
}

/// An Account added to the file after the read: the edit (an append at the
/// index the read saw) is refused and the file keeps the Account that
/// appeared; an edit built on the file's bytes applies.
#[test]
fn an_edit_built_on_other_bytes_is_refused_and_writes_nothing() {
    let dir = copy_of("valid");
    let root = dir.path();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();

    let seen = accounts_file(root);
    let built_on = seen.sha256.clone().unwrap();
    let appeared = format!(
        "{}  - id: other\n    provider: anthropic\n    secret_ref: other\n",
        std::fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap()
    );
    std::fs::write(root.join(ACCOUNTS_FILE), &appeared).unwrap();

    let add = add_account_edit(&seen, "cloud2", "anthropic", None, None);
    let err = w
        .apply_with_audit(owner(), add, &built_on, Vec::new())
        .unwrap_err();
    assert!(
        matches!(&err, WriteError::ChangedOnDisk { files } if files == &[ACCOUNTS_FILE.to_string()]),
        "{err:?}"
    );
    let remove = remove_account_edit(&seen, "cloud");
    assert!(remove.is_some());
    assert_eq!(
        std::fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap(),
        appeared
    );
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());

    // Built on what the file holds now, the same add applies.
    let now = accounts_file(root);
    let entry = w
        .apply_with_audit(
            owner(),
            add_account_edit(&now, "cloud2", "anthropic", None, None),
            now.sha256.as_deref().unwrap(),
            Vec::new(),
        )
        .unwrap();
    assert_eq!(entry.file, ACCOUNTS_FILE);
    assert_eq!(ids(root), ["cloud", "other", "cloud2"]);
}

// ------------------------------------------------------------ story 2.3

/// (id, provider, name, base_url, secret_ref) of each Account the loader
/// accepts in the file now.
type Row = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn rows(root: &std::path::Path) -> Vec<Row> {
    accounts_file(root)
        .accounts
        .into_iter()
        .map(|a| (a.id, a.provider, a.name, a.base_url, a.secret_ref))
        .collect()
}

fn row(id: &str, provider: &str, name: Option<&str>, base_url: Option<&str>) -> Row {
    (
        id.to_string(),
        provider.to_string(),
        name.map(str::to_string),
        base_url.map(str::to_string),
        Some(id.to_string()),
    )
}

fn errors(root: &std::path::Path) -> Vec<String> {
    load(root)
        .expect("the project folder exists")
        .findings
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| format!("{f:?}"))
        .collect()
}

/// An edit that appends a compatible Account writes its `base_url` as given
/// (remote, then on this Mac), with `secret_ref` naming the Account's id; an
/// Account with a fixed endpoint gets no `base_url`. The file stays valid
/// config, and the Accounts already there are untouched.
#[test]
fn an_appended_account_carries_its_base_url_only_when_given() {
    let dir = copy_of("valid");
    let root = dir.path();
    let w = ConfigWriter::open(root, Arc::new(InMemoryConfigHistory::default())).unwrap();
    let existing = rows(root);
    assert_eq!(existing.len(), 1, "{existing:?}");

    let adds: [(&str, &str, Option<&str>, Option<&str>); 3] = [
        (
            "router",
            "openai-compatible",
            Some("Router"),
            Some("https://openrouter.example/api/v1"),
        ),
        (
            "llama",
            "openai-compatible",
            None,
            Some("http://127.0.0.1:8080/v1"),
        ),
        ("oa", "openai", None, None),
    ];
    for (id, provider, name, base_url) in adds {
        let now = accounts_file(root);
        w.apply_with_audit(
            owner(),
            add_account_edit(&now, id, provider, name, base_url),
            now.sha256.as_deref().unwrap(),
            Vec::new(),
        )
        .unwrap();
    }

    let mut expected = existing;
    expected.extend(
        adds.iter()
            .map(|(id, provider, name, base_url)| row(id, provider, *name, *base_url)),
    );
    assert_eq!(rows(root), expected);
    assert_eq!(ids(root), ["cloud", "router", "llama", "oa"]);
    assert_eq!(errors(root), Vec::<String>::new());
    let text = std::fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap();
    assert_eq!(
        text.matches("base_url").count(),
        2,
        "only the two compatible Accounts have one: {text}"
    );
}

/// The edit that sets the whole list (a file whose `accounts:` is absent)
/// writes the `base_url` too. The Account takes the id the project's seat
/// already names, so the project is valid once it is added.
#[test]
fn an_account_set_into_an_empty_file_carries_its_base_url() {
    let dir = copy_of("valid");
    let root = dir.path();
    std::fs::write(root.join(ACCOUNTS_FILE), "schema_version: 2\n").unwrap();
    let w = ConfigWriter::open(root, Arc::new(InMemoryConfigHistory::default())).unwrap();

    let now = accounts_file(root);
    assert!(now.ids.is_empty());
    w.apply_with_audit(
        owner(),
        add_account_edit(
            &now,
            "cloud",
            "openai-compatible",
            None,
            Some("http://127.0.0.1:8080/v1"),
        ),
        now.sha256.as_deref().unwrap(),
        Vec::new(),
    )
    .unwrap();

    assert_eq!(
        rows(root),
        vec![row(
            "cloud",
            "openai-compatible",
            None,
            Some("http://127.0.0.1:8080/v1")
        )]
    );
    assert_eq!(accounts_file(root).problem, None);
    assert_eq!(errors(root), Vec::<String>::new());
}

/// The bytes of a new `models/accounts.yaml` hold the Account's `base_url`
/// when one is given and no such key otherwise; either way `secret_ref`
/// names the Account's id and the loader reads the file back.
#[test]
fn a_new_accounts_file_holds_the_base_url_only_when_given() {
    let cases: [(&str, &str, Option<&str>, Option<&str>); 4] = [
        (
            "router",
            "openai-compatible",
            Some("Router"),
            Some("https://openrouter.example/api/v1"),
        ),
        (
            "llama",
            "openai-compatible",
            None,
            Some("http://127.0.0.1:8080/v1"),
        ),
        ("oa", "openai", None, None),
        ("cloud2", "anthropic", Some("Second cloud"), None),
    ];
    for (id, provider, name, base_url) in cases {
        let bytes = new_accounts_file(id, provider, name, base_url).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(
            text.contains("base_url"),
            base_url.is_some(),
            "{id}: {text}"
        );

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::write(root.join(ACCOUNTS_FILE), &bytes).unwrap();
        let file = accounts_file(root);
        assert_eq!(file.problem, None, "{id}: {text}");
        assert_eq!(
            rows(root),
            vec![row(id, provider, name, base_url)],
            "{id}: {text}"
        );
    }
}

/// The writer creates the new file with a `base_url` in it (a URL is not a
/// secret-looking value), byte for byte as built, under a `create` entry;
/// the project it completes is valid.
#[test]
fn the_writer_creates_a_new_accounts_file_that_holds_a_base_url() {
    for base_url in [
        "https://openrouter.example/api/v1",
        "http://127.0.0.1:8080/v1",
    ] {
        // The valid project's seat names the Account `cloud`.
        let dir = copy_of("valid");
        let root = dir.path();
        std::fs::remove_file(root.join(ACCOUNTS_FILE)).unwrap();
        let w = ConfigWriter::open(root, Arc::new(InMemoryConfigHistory::default())).unwrap();

        let bytes = new_accounts_file("cloud", "openai-compatible", None, Some(base_url)).unwrap();
        let entry = w
            .create(
                owner(),
                ConfigCreate {
                    file: ACCOUNTS_FILE.into(),
                    bytes: bytes.clone(),
                    audit: Vec::new(),
                },
            )
            .unwrap();

        assert_eq!(entry.kind, HistoryKind::Create, "{base_url}");
        assert_eq!(
            std::fs::read(root.join(ACCOUNTS_FILE)).unwrap(),
            bytes,
            "{base_url}"
        );
        assert_eq!(
            rows(root),
            vec![row("cloud", "openai-compatible", None, Some(base_url))]
        );
        assert_eq!(errors(root), Vec::<String>::new(), "{base_url}");
    }
}

/// Story 2.3 (review): the read of `models/accounts.yaml` says which entry
/// has an error, so the CLI never calls, or shows a host for, an Account
/// whose entry the loader refused in part (a `base_url` it rejects reads as
/// no `base_url`). An entry without an error has none.
#[test]
fn the_read_names_the_errors_of_each_entry() {
    let dir = copy_of("valid");
    let root = dir.path();
    let mut text = std::fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap();
    text.push_str(
        "  - id: oa\n    provider: openai\n    base_url: http://example.org/v1\n    secret_ref: oa\n\
         \x20 - id: box\n    provider: openai-compatible\n    base_url: http://localhost:8080/v1\n\
         \x20 - id: bare\n    provider: openai-compatible\n\
         \x20 - id: fine\n    provider: openai-compatible\n    base_url: http://127.0.0.1:8080/v1\n",
    );
    std::fs::write(root.join(ACCOUNTS_FILE), text).unwrap();

    let file = accounts_file(root);
    assert_eq!(file.problem, None);
    for id in ["oa", "box", "bare"] {
        assert!(
            !file.entry_errors(id).is_empty(),
            "{id}: its entry has an error"
        );
    }
    assert!(
        file.entry_errors("box")
            .iter()
            .any(|e| e.contains("base_url")),
        "{:?}",
        file.entry_errors("box")
    );
    for id in ["cloud", "fine", "absent"] {
        assert_eq!(file.entry_errors(id), Vec::<String>::new(), "{id}");
    }
}
