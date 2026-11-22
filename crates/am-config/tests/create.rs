//! Story 2.1 acceptance tests (am-config side): `ConfigCreate` (AD-16), the
//! writer's one way to make a new config file. It is placed without
//! replacing anything: a temp file, fsync, a hard link to the final name
//! (which fails when the name exists), removal of the temp file, then a
//! folder fsync. Missing parent folders inside the project are created. It
//! carries the same intent, History and audit as an edit, under
//! `HistoryKind::Create`; Undo of a create is refused in words (the writer
//! never deletes a file), and a volume that refuses hard links refuses the
//! create in words. History is faked by am-testkit's
//! `InMemoryConfigHistory`; the store's side is in
//! `crates/am-store/tests/config_history.rs`. Written before `create`
//! exists.
//!
//! Story 2.12 (AD-17): the Accounts file is user-level, so these tests make
//! the user Accounts file (`accounts.yaml`) in a temporary app data dir
//! through `ConfigWriter::for_user_accounts`; the two rows that need a
//! project folder (a missing parent folder, a linked parent) create a
//! project's `models/prices.yaml` instead. Rewritten before the retarget.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use am_config::{
    ConfigCreate, ConfigWriter, FailPoint, USER_ACCOUNTS_FILE, WriteError, canonical_bytes, load,
    user_accounts,
};
use am_domain::{Actor, AuditNote, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::copy_of;
use sha2::{Digest, Sha256};

const FILE: &str = USER_ACCOUNTS_FILE;
/// A project file in a folder, for the rows about folders.
const PRICES: &str = "models/prices.yaml";
const ACCOUNTS: &str =
    "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n";

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The writer of the user Accounts file in the app data dir `root`.
fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(root, history.clone()).unwrap();
    (w, history)
}

/// The writer of the project at `root`.
fn project_writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

fn create(file: &str, bytes: &[u8]) -> ConfigCreate {
    ConfigCreate {
        file: file.into(),
        bytes: bytes.to_vec(),
        audit: Vec::new(),
    }
}

/// An app data dir with no Accounts file yet (the first add on a Mac).
fn without_accounts() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// An app data dir holding an Accounts file.
fn with_accounts() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE), ACCOUNTS).unwrap();
    dir
}

/// Names in `dir` that a write left behind (temp files start with `.`).
fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default()
}

fn assert_nothing_recorded(history: &InMemoryConfigHistory) {
    assert!(
        history.entries(None).unwrap().is_empty(),
        "no History entry"
    );
    assert!(
        history.pending_intents().unwrap().is_empty(),
        "no intent left pending"
    );
}

// ------------------------------------------------------------- matrix rows

/// Missing file: the create writes it as given (canonical bytes), as a
/// `create` History entry from no bytes, remembered as the writer's own
/// write, with no temp file left; the project loads.
#[test]
fn create_writes_a_missing_file_as_a_create_entry() {
    let dir = without_accounts();
    let root = dir.path();
    let bytes = canonical_bytes(FILE, ACCOUNTS.as_bytes()).unwrap();
    let (w, history) = writer(root);

    let entry = w.create(owner(), create(FILE, &bytes)).unwrap();

    assert_eq!(std::fs::read(root.join(FILE)).unwrap(), bytes);
    assert_eq!(entry.kind, HistoryKind::Create);
    assert_eq!(entry.file, FILE);
    assert_eq!(entry.ref_entry, None);
    assert!(entry.before.is_empty());
    assert_eq!(entry.before_sha256, sha(b""));
    assert_eq!(entry.after, bytes);
    assert_eq!(entry.after_sha256, sha(&bytes));
    assert_eq!(entry.who(), "You (Owner)");
    assert!(entry.diff.contains("+  - id: cloud") || entry.diff.contains("+- id: cloud"));
    assert_eq!(history.entries(None).unwrap(), vec![entry.clone()]);
    assert!(history.pending_intents().unwrap().is_empty());
    assert!(w.is_own_write(FILE, &entry.after_sha256));
    assert!(leftovers(root).is_empty(), "no temp file");
    let loaded = user_accounts::load(&root.join(FILE));
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    assert_eq!(loaded.accounts.ids(), ["cloud"]);
}

/// Missing parent folders inside the project are created.
#[test]
fn create_makes_missing_parent_folders_inside_the_project() {
    let dir = copy_of("valid");
    let root = dir.path();
    let prices = std::fs::read(root.join(PRICES)).unwrap();
    std::fs::remove_dir_all(root.join("models")).unwrap();
    let bytes = canonical_bytes(PRICES, &prices).unwrap();
    let (w, history) = project_writer(root);

    let entry = w.create(owner(), create(PRICES, &bytes)).unwrap();

    assert!(root.join("models").is_dir());
    assert_eq!(std::fs::read(root.join(PRICES)).unwrap(), bytes);
    assert_eq!(entry.kind, HistoryKind::Create);
    assert_eq!(history.entries(None).unwrap().len(), 1);
    assert!(leftovers(root).is_empty());
    assert_eq!(load(root).unwrap().error_count(), 0);
}

/// A file that already exists is never replaced by a create.
#[test]
fn create_refuses_a_file_that_already_exists() {
    let dir = with_accounts();
    let root = dir.path();
    let before = std::fs::read(root.join(FILE)).unwrap();
    let (w, history) = writer(root);

    let err = w
        .create(owner(), create(FILE, ACCOUNTS.as_bytes()))
        .unwrap_err();

    assert!(err.to_string().contains("already exists"), "{err}");
    assert_eq!(std::fs::read(root.join(FILE)).unwrap(), before);
    assert_nothing_recorded(&history);
    assert!(leftovers(root).is_empty());
}

/// A `ConfigHistory` that writes `bytes` at `file` right after the intent
/// is recorded: a file that appears while the create runs.
struct AppearDuringCreate {
    inner: Arc<InMemoryConfigHistory>,
    root: std::path::PathBuf,
    bytes: Vec<u8>,
    armed: AtomicBool,
}

impl ConfigHistory for AppearDuringCreate {
    fn begin_intent(
        &self,
        actor: Actor,
        intent: am_domain::NewIntent,
    ) -> Result<String, am_domain::HistoryError> {
        let id = self.inner.begin_intent(actor, intent)?;
        if self.armed.swap(false, Ordering::SeqCst) {
            std::fs::write(self.root.join(FILE), &self.bytes).unwrap();
        }
        Ok(id)
    }
    fn commit_entry(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<am_domain::HistoryEntry, am_domain::HistoryError> {
        self.inner.commit_entry(actor, intent_id)
    }
    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), am_domain::HistoryError> {
        self.inner.drop_intent(actor, intent_id)
    }
    fn reconcile_intent(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<am_domain::HistoryEntry, am_domain::HistoryError> {
        self.inner.reconcile_intent(actor, intent_id)
    }
    fn pending_intents(&self) -> Result<Vec<am_domain::PendingIntent>, am_domain::HistoryError> {
        self.inner.pending_intents()
    }
    fn entries(
        &self,
        file: Option<String>,
    ) -> Result<Vec<am_domain::HistoryEntry>, am_domain::HistoryError> {
        self.inner.entries(file)
    }
    fn entry(
        &self,
        id: String,
    ) -> Result<Option<am_domain::HistoryEntry>, am_domain::HistoryError> {
        self.inner.entry(id)
    }
}

/// Missing file, error column: a file that appears meanwhile (after the
/// intent, before the hard link) refuses the create; the file that appeared
/// stays as it is, and no entry, intent or temp file is left.
#[test]
fn create_refuses_a_file_that_appears_while_it_runs_and_keeps_it() {
    let dir = without_accounts();
    let root = dir.path();
    let outside = b"schema_version: 2\naccounts: []\n".to_vec();
    let inner = Arc::new(InMemoryConfigHistory::default());
    let history = Arc::new(AppearDuringCreate {
        inner: inner.clone(),
        root: root.to_path_buf(),
        bytes: outside.clone(),
        armed: AtomicBool::new(true),
    });
    let w = ConfigWriter::for_user_accounts(root, history).unwrap();

    let err = w
        .create(owner(), create(FILE, ACCOUNTS.as_bytes()))
        .unwrap_err();

    assert!(err.to_string().contains("already exists"), "{err}");
    assert_eq!(
        std::fs::read(root.join(FILE)).unwrap(),
        outside,
        "the file that appeared stays"
    );
    assert_nothing_recorded(&inner);
    assert!(leftovers(root).is_empty(), "no temp file");
}

/// A volume that refuses hard links refuses the create in words, writing
/// nothing.
#[test]
fn create_on_a_volume_without_hard_links_is_refused_in_words() {
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    w.fail_at(Some(FailPoint::NoHardLinks));

    let err = w
        .create(owner(), create(FILE, ACCOUNTS.as_bytes()))
        .unwrap_err();

    let words = err.to_string();
    assert!(words.contains("hard link"), "{words}");
    assert!(!root.join(FILE).exists());
    assert_nothing_recorded(&history);
    assert!(leftovers(root).is_empty());
}

/// Undo of a create is refused in words: the writer never deletes a file.
#[test]
fn undo_of_a_create_is_refused_in_words_and_keeps_the_file() {
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    let entry = w
        .create(owner(), create(FILE, ACCOUNTS.as_bytes()))
        .unwrap();
    let bytes = std::fs::read(root.join(FILE)).unwrap();

    let err = w.undo(owner(), &entry.id).unwrap_err();

    assert!(err.to_string().contains("never deletes"), "{err}");
    assert_eq!(std::fs::read(root.join(FILE)).unwrap(), bytes);
    assert_eq!(history.entries(None).unwrap(), vec![entry]);
}

/// A create names the one file the writer writes by its plain name; a
/// parent that is a link is not followed, so nothing is written outside.
#[test]
fn create_refuses_bad_paths_and_a_linked_parent() {
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    for file in [
        "/tmp/accounts.yaml",
        "../accounts.yaml",
        "models/../../accounts.yaml",
        "notes/accounts.yaml",
        "models/accounts.yaml",
        "",
    ] {
        let err = w
            .create(owner(), create(file, ACCOUNTS.as_bytes()))
            .unwrap_err();
        assert!(matches!(err, WriteError::BadPath { .. }), "{file}: {err:?}");
    }
    assert_nothing_recorded(&history);

    #[cfg(unix)]
    {
        let project = copy_of("valid");
        let root = project.path();
        let prices = std::fs::read(root.join(PRICES)).unwrap();
        let (w, history) = project_writer(root);
        let outside = tempfile::tempdir().unwrap();
        std::fs::remove_dir_all(root.join("models")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("models")).unwrap();
        let err = w.create(owner(), create(PRICES, &prices)).unwrap_err();
        assert!(err.to_string().contains("link"), "{err}");
        assert!(
            std::fs::read_dir(outside.path()).unwrap().next().is_none(),
            "nothing written through the link"
        );
        assert_nothing_recorded(&history);
    }
}

/// As for an edit, a create whose file has an error is refused with the
/// finding, and nothing is written.
#[test]
fn create_refuses_a_result_that_adds_an_error() {
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    let reserved = "schema_version: 2\naccounts:\n  - id: local-ollama\n    provider: anthropic\n";

    let err = w
        .create(owner(), create(FILE, reserved.as_bytes()))
        .unwrap_err();

    assert!(matches!(err, WriteError::Invalid { .. }), "{err:?}");
    assert!(err.to_string().contains("reserved"), "{err}");
    assert!(!root.join(FILE).exists());
    assert_nothing_recorded(&history);
}

/// No key reaches a config file or History: a create whose bytes hold a
/// secret-looking value is refused without naming it.
#[test]
fn create_refuses_a_secret_looking_value_and_writes_nothing() {
    const KEY: &str = "sk-ant-canary0canary0canary";
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    let text = format!("{ACCOUNTS}    description: {KEY}\n");

    let err = w
        .create(owner(), create(FILE, text.as_bytes()))
        .unwrap_err();

    let words = err.to_string();
    assert!(words.contains("secret-looking"), "{words}");
    assert!(!words.contains(KEY), "{words}");
    assert!(!root.join(FILE).exists());
    assert_nothing_recorded(&history);
}

/// AD-16's apply order: a crash after the file was placed leaves the intent
/// (with the audit notes it carries) pending, and `reconcile` completes it
/// as a `create` entry.
#[test]
fn create_interrupted_after_placing_is_completed_by_reconcile() {
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    let note = AuditNote {
        action: "config.widened".into(),
        target: "cloud".into(),
        args: BTreeMap::from([
            ("kind".to_string(), "account_added".to_string()),
            ("account".to_string(), "cloud".to_string()),
        ]),
    };
    let bytes = canonical_bytes(FILE, ACCOUNTS.as_bytes()).unwrap();
    w.fail_at(Some(FailPoint::AfterRename));

    let err = w
        .create(
            owner(),
            ConfigCreate {
                file: FILE.into(),
                bytes: bytes.clone(),
                audit: vec![note.clone()],
            },
        )
        .unwrap_err();

    assert!(matches!(err, WriteError::Interrupted(_)), "{err:?}");
    assert_eq!(std::fs::read(root.join(FILE)).unwrap(), bytes);
    let pending = history.pending_intents().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].intent.kind, HistoryKind::Create);
    assert_eq!(pending[0].intent.audit, vec![note]);
    assert!(pending[0].intent.before.is_empty());

    w.fail_at(None);
    let report = w.reconcile(owner()).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert_eq!(report.completed[0].kind, HistoryKind::Create);
    assert!(history.pending_intents().unwrap().is_empty());
    assert!(leftovers(root).is_empty());
}

/// A crash before the file was placed leaves no file; `reconcile` drops the
/// intent.
#[test]
fn create_interrupted_before_placing_leaves_no_file() {
    let dir = without_accounts();
    let root = dir.path();
    let (w, history) = writer(root);
    w.fail_at(Some(FailPoint::BeforeRename));

    let err = w
        .create(owner(), create(FILE, ACCOUNTS.as_bytes()))
        .unwrap_err();

    assert!(matches!(err, WriteError::Interrupted(_)), "{err:?}");
    assert!(!root.join(FILE).exists());
    drop(w);
    // A writer opened afterwards (the next start) removes the temp file.
    let w2 = ConfigWriter::for_user_accounts(root, history.clone()).unwrap();
    let report = w2.reconcile(owner()).unwrap();
    assert!(report.completed.is_empty());
    assert_eq!(report.dropped.len(), 1);
    assert!(history.pending_intents().unwrap().is_empty());
    assert!(!root.join(FILE).exists());
    assert!(leftovers(root).is_empty());
}
