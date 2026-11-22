//! Story 2.13 (am-store side; AD-7, AD-15 *Migrate*, AD-16, AD-17, AD-25):
//! what `agentic accounts migrate` needs from the two stores.
//!
//! - **The `recovered` mark:** user migration `0002` adds a one-row marks
//!   table, so `USER_SCHEMA_VERSION` is 2. `UserStore::mark_recovered()`
//!   (under the guard, so under `user.lock`) sets the mark and
//!   `UserReader::is_recovered()` reads it. A store created normally, and a
//!   store made before this story (version 1) once migrated, is not marked;
//!   a missing `user.db` reads as not marked and is not created. Migrating
//!   a version-1 store that holds data takes the user store's pre-migration
//!   backup first (`backups/pre-migration/`), which story 2.12 could not
//!   test, and keeps its rows.
//! - **The removal kind:** a project store accepts a `removal` intent
//!   (`HistoryKind::Removal`: `before` is the removed bytes, `after` empty,
//!   naming no earlier entry), commits it as a History entry and writes the
//!   `config.migrated` audit entry for it (args `kind: removal` and the
//!   file). A removal that removes no bytes, leaves bytes behind or names an
//!   earlier entry is refused, and nothing is recorded.
//!
//! Written before the migration, the mark and the kind exist.
#![forbid(unsafe_code)]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use am_domain::runtime::AppDataLayout;
use am_domain::{Actor, ConfigHistory, HistoryKind, NewIntent};
use am_store::user::{USER_APPLICATION_ID, USER_SCHEMA_VERSION, UserReader, UserStore};
use am_store::{Store, StoreConfigHistory, verify_jsonl};
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};

const WAIT: Duration = Duration::from_secs(10);
const LEGACY_FILE: &str = "models/accounts.yaml";
const LEGACY_BYTES: &str =
    "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n";
/// The user store's first migration, as story 2.12 shipped it: a store made
/// before this story is at this version.
const USER_0001: &str = include_str!("../migrations/user/0001_user.sql");

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn app_data() -> (tempfile::TempDir, AppDataLayout) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("agentic-manager");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    (tmp, AppDataLayout::new(dir))
}

/// A user store at version 1 (story 2.12's schema) holding one History
/// entry, as a Mac that ran story 2.12 has it.
fn version_one_store(layout: &AppDataLayout) {
    let conn = Connection::open(layout.user_db()).unwrap();
    conn.execute_batch(USER_0001).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA application_id = {USER_APPLICATION_ID}; PRAGMA user_version = 1;"
    ))
    .unwrap();
    let after = LEGACY_BYTES.as_bytes();
    conn.execute(
        "INSERT INTO config_history (id, at, actor, file, kind, ref_entry, before_sha256, after_sha256, before_bytes, after_bytes, diff)
         VALUES ('0193a000-0000-7000-8000-000000000001', '2026-11-22T10:00:00.000Z', ?1, 'accounts.yaml', 'create', NULL, ?2, ?3, x'', ?4, '+cloud')",
        rusqlite::params![
            serde_json::to_string(&owner()).unwrap(),
            sha(b""),
            sha(after),
            after
        ],
    )
    .unwrap();
    drop(conn);
    std::fs::set_permissions(layout.user_db(), std::fs::Permissions::from_mode(0o600)).unwrap();
}

// ---------------------------------------------------------- the marks table

/// The marks table is user migration 0002.
#[test]
fn the_user_store_is_at_version_two() {
    assert_eq!(USER_SCHEMA_VERSION, 2);
    let (_tmp, layout) = app_data();
    drop(UserStore::locked(&layout, WAIT).unwrap());
    let conn = Connection::open(layout.user_db()).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
}

/// A store created normally is not marked; a missing `user.db` reads as not
/// marked and is not created by the read.
#[test]
fn a_store_created_normally_is_not_recovered() {
    let (_tmp, layout) = app_data();
    assert!(!UserReader::open(&layout).unwrap().is_recovered().unwrap());
    assert!(!layout.user_db().exists(), "a read creates nothing");

    drop(UserStore::locked(&layout, WAIT).unwrap());
    assert!(!UserReader::open(&layout).unwrap().is_recovered().unwrap());
}

/// The mark is set under the guard, read by any reader, kept across opens,
/// and setting it twice keeps one mark.
#[test]
fn the_recovered_mark_is_set_under_the_lock_and_kept() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    guard.mark_recovered().unwrap();
    guard.mark_recovered().unwrap();
    assert!(UserReader::open(&layout).unwrap().is_recovered().unwrap());
    drop(guard);

    // Another process's guard sees it, and it stays.
    drop(UserStore::locked(&layout, WAIT).unwrap());
    assert!(UserReader::open(&layout).unwrap().is_recovered().unwrap());

    let conn = Connection::open(layout.user_db()).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE '%mark%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(tables.len(), 1, "one marks table: {tables:?}");
    let rows: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {}", tables[0]), [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows, 1, "one row");
}

/// A store made before this story: migrating it to version 2 takes the
/// pre-migration backup first (owner-only), keeps its History, and leaves it
/// not marked `recovered`.
#[test]
fn a_store_made_before_this_story_is_backed_up_migrated_and_not_recovered() {
    let (_tmp, layout) = app_data();
    version_one_store(&layout);
    let before = UserReader::open(&layout)
        .unwrap()
        .config_history(None)
        .unwrap();
    assert_eq!(before.len(), 1);

    let guard = UserStore::locked(&layout, WAIT).unwrap();
    drop(guard);

    let backups: Vec<_> = std::fs::read_dir(layout.backups_pre_migration())
        .expect("the pre-migration backup folder")
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
    let mode = std::fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "{}", backups[0].display());
    let copy = Connection::open(&backups[0]).unwrap();
    let copied: i64 = copy
        .query_row("SELECT COUNT(*) FROM config_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(copied, 1, "the backup holds the version-1 rows");

    let reader = UserReader::open(&layout).unwrap();
    assert_eq!(reader.config_history(None).unwrap(), before, "rows kept");
    assert!(!reader.is_recovered().unwrap());
    let conn = Connection::open(layout.user_db()).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, USER_SCHEMA_VERSION);
}

// ---------------------------------------------------------- the removal kind

fn project_store() -> (tempfile::TempDir, Store, StoreConfigHistory) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state.db")).unwrap();
    let history = StoreConfigHistory::new(&store).unwrap();
    (dir, store, history)
}

fn removal(before: &[u8], after: &[u8], ref_entry: Option<String>) -> NewIntent {
    NewIntent {
        file: LEGACY_FILE.into(),
        kind: HistoryKind::Removal,
        ref_entry,
        before_sha256: sha(before),
        after_sha256: sha(after),
        before: before.to_vec(),
        after: after.to_vec(),
        diff: String::from_utf8_lossy(before)
            .lines()
            .map(|l| format!("-{l}\n"))
            .collect(),
        audit: Vec::new(),
    }
}

fn audit(store: &Store) -> Vec<Value> {
    let mut out = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    verify_jsonl(out.as_slice()).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// A removal of the old Accounts file: committed as a `removal` History
/// entry with the removed bytes as `before` and no bytes after, audited as
/// `config.migrated` with `kind: removal` and the file; the chain verifies.
#[test]
fn a_removal_is_recorded_with_its_bytes_and_audited_as_migrated() {
    let (_dir, store, history) = project_store();
    let id = history
        .begin_intent(owner(), removal(LEGACY_BYTES.as_bytes(), b"", None))
        .unwrap();
    let entry = history.commit_entry(owner(), id).unwrap();

    assert_eq!(entry.kind, HistoryKind::Removal);
    assert_eq!(entry.file, LEGACY_FILE);
    assert_eq!(entry.before, LEGACY_BYTES.as_bytes());
    assert!(entry.after.is_empty());
    assert_eq!(entry.after_sha256, sha(b""));
    assert_eq!(entry.ref_entry, None);
    assert_eq!(entry.actor, owner());
    assert_eq!(history.entries(None).unwrap(), vec![entry.clone()]);

    let migrated: Vec<Value> = audit(&store)
        .into_iter()
        .filter(|e| e["action"] == "config.migrated")
        .collect();
    assert_eq!(migrated.len(), 1, "{migrated:#?}");
    assert_eq!(migrated[0]["args"]["kind"], "removal");
    assert_eq!(migrated[0]["args"]["file"], LEGACY_FILE);
    let text = serde_json::to_string(&migrated[0]).unwrap();
    assert!(
        !text.contains("provider: anthropic"),
        "no file content in the audit log: {text}"
    );
}

/// A removal that removes nothing, keeps bytes or names an earlier entry is
/// refused, and nothing is recorded.
#[test]
fn a_removal_that_is_not_one_is_refused() {
    let (_dir, store, history) = project_store();
    let earlier = {
        let id = history
            .begin_intent(
                owner(),
                NewIntent {
                    file: LEGACY_FILE.into(),
                    kind: HistoryKind::Create,
                    ref_entry: None,
                    before_sha256: sha(b""),
                    after_sha256: sha(LEGACY_BYTES.as_bytes()),
                    before: Vec::new(),
                    after: LEGACY_BYTES.as_bytes().to_vec(),
                    diff: "+cloud\n".into(),
                    audit: Vec::new(),
                },
            )
            .unwrap();
        history.commit_entry(owner(), id).unwrap()
    };
    let entries = history.entries(None).unwrap().len();
    let entries_audit = audit(&store).len();

    for (why, intent) in [
        ("removes no bytes", removal(b"", b"x", None)),
        (
            "leaves bytes behind",
            removal(LEGACY_BYTES.as_bytes(), b"schema_version: 2\n", None),
        ),
        (
            "names an earlier entry",
            removal(LEGACY_BYTES.as_bytes(), b"", Some(earlier.id.clone())),
        ),
    ] {
        assert!(
            history.begin_intent(owner(), intent).is_err(),
            "{why}: refused"
        );
    }
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(history.entries(None).unwrap().len(), entries);
    assert_eq!(audit(&store).len(), entries_audit);
}

/// A removal intent committed before the unlink and reconciled after it
/// (the file is gone: written) is audited as reconciled, with its kind.
#[test]
fn a_removal_intent_is_reconciled_like_any_other() {
    let (_dir, store, history) = project_store();
    let id = history
        .begin_intent(owner(), removal(LEGACY_BYTES.as_bytes(), b"", None))
        .unwrap();
    let pending = history.pending_intents().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].intent.kind, HistoryKind::Removal);

    let entry = history
        .reconcile_intent(
            Actor::System {
                component: "config-writer".into(),
            },
            id,
        )
        .unwrap();
    assert_eq!(entry.kind, HistoryKind::Removal);
    let reconciled: Vec<Value> = audit(&store)
        .into_iter()
        .filter(|e| e["action"] == "config.reconciled")
        .collect();
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0]["args"]["kind"], "removal");
}

// ======================================================================
// Review fixes (2026-11-22 review pass, plan § Review Triage Log).
// ======================================================================

/// G-open (Quick 8, Adversarial 2): reading the `recovered` mark fails
/// closed. A version-2 store whose one marks row is gone (forced past its
/// trigger, as damage or a hand edit would) is a read error, never "not
/// recovered", for the reader and for the guard.
#[test]
fn a_mark_that_cannot_be_read_is_an_error_not_unmarked() {
    let (_tmp, layout) = app_data();
    drop(UserStore::locked(&layout, WAIT).unwrap());
    {
        let conn = Connection::open(layout.user_db()).unwrap();
        conn.execute_batch("DROP TRIGGER user_marks_no_delete; DELETE FROM user_marks;")
            .unwrap();
    }
    assert!(
        UserReader::open(&layout).unwrap().is_recovered().is_err(),
        "the reader"
    );
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    assert!(guard.is_recovered().is_err(), "the guard");
}

/// G-open: a version-2 store whose marks table is gone is a read error too
/// (only a store made before the marks table, version 1, reads as not
/// marked).
#[test]
fn a_missing_marks_table_at_version_two_is_an_error() {
    let (_tmp, layout) = app_data();
    drop(UserStore::locked(&layout, WAIT).unwrap());
    {
        let conn = Connection::open(layout.user_db()).unwrap();
        conn.execute_batch("DROP TABLE user_marks;").unwrap();
    }
    assert!(UserReader::open(&layout).unwrap().is_recovered().is_err());
}
