//! Story 2.1 (review fix): the interim CLI Account edit is built from a
//! read of the Accounts file and names list items by their number, so the
//! writer applies it only while the file still holds the bytes it was built
//! on (`AccountsFile::sha256`, given to `ConfigWriter::apply_with_audit`); a
//! file that changed meanwhile is refused as `ChangedOnDisk` and nothing is
//! written. Written before the check exists.
//!
//! Story 2.3: the Account the edit writes may carry a `base_url` (an
//! OpenAI-compatible Account's endpoint), beside `secret_ref` and never a
//! key; `add_account_edit` and `new_accounts_file` take it as their last
//! argument, `None` for a provider with one fixed endpoint. Written before
//! the argument exists.
//!
//! Story 2.12 (AD-17): the edit targets the user Accounts file,
//! `<app data dir>/accounts.yaml` (`USER_ACCOUNTS_FILE`), read by
//! `accounts_file(<app data dir>)` and written by
//! `ConfigWriter::for_user_accounts(<app data dir>, history)`; each test
//! works in a temporary app data dir, never the real one. Rewritten before
//! the retarget.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{
    ConfigCreate, ConfigWriter, Severity, USER_ACCOUNTS_FILE, WriteError, accounts_file,
    add_account_edit, new_accounts_file, remove_account_edit, user_accounts,
};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use sha2::{Digest, Sha256};

/// A temporary app data dir holding `fixtures/accounts/valid.yaml` as its
/// Accounts file (one Anthropic Account, `cloud`).
fn app_data_with_valid() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/accounts/valid.yaml");
    std::fs::write(
        dir.path().join(USER_ACCOUNTS_FILE),
        std::fs::read(fixture).unwrap(),
    )
    .unwrap();
    dir
}

fn file_of(root: &Path) -> PathBuf {
    root.join(USER_ACCOUNTS_FILE)
}

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn ids(root: &Path) -> Vec<String> {
    accounts_file(root).ids.into_iter().flatten().collect()
}

/// The read carries the sha256 of the bytes it saw (none for no file).
#[test]
fn the_read_names_the_bytes_it_was_made_on() {
    let dir = app_data_with_valid();
    let root = dir.path();
    let bytes = std::fs::read(file_of(root)).unwrap();
    assert_eq!(
        accounts_file(root).sha256,
        Some(hex::encode(Sha256::digest(&bytes)))
    );
    std::fs::remove_file(file_of(root)).unwrap();
    assert_eq!(accounts_file(root).sha256, None);
}

/// An Account added to the file after the read: the edit (an append at the
/// index the read saw) is refused and the file keeps the Account that
/// appeared; an edit built on the file's bytes applies.
#[test]
fn an_edit_built_on_other_bytes_is_refused_and_writes_nothing() {
    let dir = app_data_with_valid();
    let root = dir.path();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(root, history.clone()).unwrap();

    let seen = accounts_file(root);
    let built_on = seen.sha256.clone().unwrap();
    let appeared = format!(
        "{}  - id: other\n    provider: anthropic\n    secret_ref: other\n",
        std::fs::read_to_string(file_of(root)).unwrap()
    );
    std::fs::write(file_of(root), &appeared).unwrap();

    let add = add_account_edit(&seen, "cloud2", "anthropic", None, None);
    let err = w
        .apply_with_audit(owner(), add, &built_on, Vec::new())
        .unwrap_err();
    assert!(
        matches!(&err, WriteError::ChangedOnDisk { files } if files == &[USER_ACCOUNTS_FILE.to_string()]),
        "{err:?}"
    );
    let remove = remove_account_edit(&seen, "cloud");
    assert!(remove.is_some());
    assert_eq!(std::fs::read_to_string(file_of(root)).unwrap(), appeared);
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
    assert_eq!(entry.file, USER_ACCOUNTS_FILE);
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

fn rows(root: &Path) -> Vec<Row> {
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

fn errors(root: &Path) -> Vec<String> {
    user_accounts::load(&file_of(root))
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
    let dir = app_data_with_valid();
    let root = dir.path();
    let w =
        ConfigWriter::for_user_accounts(root, Arc::new(InMemoryConfigHistory::default())).unwrap();
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
    let text = std::fs::read_to_string(file_of(root)).unwrap();
    assert_eq!(
        text.matches("base_url").count(),
        2,
        "only the two compatible Accounts have one: {text}"
    );
}

/// The edit that sets the whole list (a file whose `accounts:` is absent)
/// writes the `base_url` too.
#[test]
fn an_account_set_into_an_empty_file_carries_its_base_url() {
    let dir = app_data_with_valid();
    let root = dir.path();
    std::fs::write(file_of(root), "schema_version: 2\n").unwrap();
    let w =
        ConfigWriter::for_user_accounts(root, Arc::new(InMemoryConfigHistory::default())).unwrap();

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

/// The bytes of a new user Accounts file hold the Account's `base_url`
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
        std::fs::write(file_of(root), &bytes).unwrap();
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
/// the file it creates is valid.
#[test]
fn the_writer_creates_a_new_accounts_file_that_holds_a_base_url() {
    for base_url in [
        "https://openrouter.example/api/v1",
        "http://127.0.0.1:8080/v1",
    ] {
        // The first add on a Mac: no Accounts file yet.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let w = ConfigWriter::for_user_accounts(root, Arc::new(InMemoryConfigHistory::default()))
            .unwrap();

        let bytes = new_accounts_file("cloud", "openai-compatible", None, Some(base_url)).unwrap();
        let entry = w
            .create(
                owner(),
                ConfigCreate {
                    file: USER_ACCOUNTS_FILE.into(),
                    bytes: bytes.clone(),
                    audit: Vec::new(),
                },
            )
            .unwrap();

        assert_eq!(entry.kind, HistoryKind::Create, "{base_url}");
        assert_eq!(std::fs::read(file_of(root)).unwrap(), bytes, "{base_url}");
        assert_eq!(
            rows(root),
            vec![row("cloud", "openai-compatible", None, Some(base_url))]
        );
        assert_eq!(errors(root), Vec::<String>::new(), "{base_url}");
    }
}

/// Story 2.3 (review): the read of the Accounts file says which entry
/// has an error, so the CLI never calls, or shows a host for, an Account
/// whose entry the loader refused in part (a `base_url` it rejects reads as
/// no `base_url`). An entry without an error has none.
#[test]
fn the_read_names_the_errors_of_each_entry() {
    let dir = app_data_with_valid();
    let root = dir.path();
    let mut text = std::fs::read_to_string(file_of(root)).unwrap();
    text.push_str(
        "  - id: oa\n    provider: openai\n    base_url: http://example.org/v1\n    secret_ref: oa\n\
         \x20 - id: box\n    provider: openai-compatible\n    base_url: http://localhost:8080/v1\n\
         \x20 - id: bare\n    provider: openai-compatible\n\
         \x20 - id: fine\n    provider: openai-compatible\n    base_url: http://127.0.0.1:8080/v1\n",
    );
    std::fs::write(file_of(root), text).unwrap();

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
