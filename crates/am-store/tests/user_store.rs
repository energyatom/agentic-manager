//! Story 2.12 (am-store side; AD-6, AD-7, AD-17, AD-25): the user store,
//! `<app data dir>/user.db`, shared by every process of the owner on this
//! Mac. am-store opens it from the `AppDataLayout` am-app gives:
//!
//! - **Format:** WAL, `synchronous=FULL`, `foreign_keys=ON`; its own
//!   migration set (`USER_SCHEMA_VERSION`) and its own `application_id`
//!   (`USER_APPLICATION_ID`), distinct from the project store's
//!   (`am_store::APPLICATION_ID`), so neither store ever opens as the other.
//! - **Wrong file:** a database with another non-zero `application_id` (a
//!   project store) is refused in words naming what it is; so is a newer
//!   `user_version`, saying a newer app made it.
//! - **No long-lived writer:** a write is `UserStore::locked(&layout,
//!   timeout)`, which takes `user.lock` with a bounded wait (refused in
//!   words after it), opens one read-write connection and migrates it under
//!   the lock. The guard implements `ConfigHistory` (the same intent, History
//!   and audit shapes as a project store: one executor) and
//!   `append_audit`; it releases the lock on drop.
//! - **Readers:** `UserReader::open(&layout)` is read-only, takes no lock,
//!   and reads a missing `user.db` as empty without creating it.
//!
//! Written before the module exists.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use am_domain::runtime::AppDataLayout;
use am_domain::{Actor, AuditEvent, AuditNote, ConfigHistory, HistoryKind, NewIntent};
use am_store::user::{USER_APPLICATION_ID, USER_SCHEMA_VERSION, UserReader, UserStore};
use am_store::{Store, StoreConfigHistory, verify_jsonl};
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The wait every test gives a lock that is free.
const WAIT: Duration = Duration::from_secs(10);

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A temporary app data dir, owner-only as am-app creates it.
fn app_data() -> (tempfile::TempDir, AppDataLayout) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("agentic-manager");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    (tmp, AppDataLayout::new(dir))
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The `account_added` confirmation record an add carries (AD-15 *Record*).
fn added(id: &str, host: &str) -> AuditNote {
    AuditNote {
        action: "config.widened".into(),
        target: id.into(),
        args: BTreeMap::from(
            [
                ("kind", "account_added"),
                ("file", "accounts.yaml"),
                ("account", id),
                ("provider", "anthropic"),
                ("scheme", "https"),
                ("host", host),
                ("port", "443"),
                ("key", "stored"),
                ("confirmation", "typed_id"),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string())),
        ),
    }
}

/// A `create` of the user Accounts file holding `id`.
fn create(id: &str, notes: Vec<AuditNote>) -> NewIntent {
    let after = format!(
        "schema_version: 2\naccounts:\n  - id: {id}\n    provider: anthropic\n    secret_ref: {id}\n"
    )
    .into_bytes();
    NewIntent {
        file: "accounts.yaml".into(),
        kind: HistoryKind::Create,
        ref_entry: None,
        before_sha256: sha(b""),
        after_sha256: sha(&after),
        before: Vec::new(),
        diff: format!("+  - id: {id}\n"),
        after,
        audit: notes,
    }
}

/// Commits `intent` through `history` as one write.
fn commit(history: &dyn ConfigHistory, intent: NewIntent) {
    let id = history.begin_intent(owner(), intent).unwrap();
    history.commit_entry(owner(), id).unwrap();
}

/// The user store's audit log as JSON objects, after checking the chain.
fn audit(layout: &AppDataLayout) -> Vec<Value> {
    let reader = UserReader::open(layout).unwrap();
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    verify_jsonl(out.as_slice()).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

// ------------------------------------------------------------ the format

/// First add on a Mac (store side): `locked` creates `user.db` and
/// `user.lock` owner-only, migrates the database to this binary's user
/// version, stamps the user store's `application_id`, and runs WAL with
/// `synchronous=FULL` and `foreign_keys=ON`.
#[test]
fn a_new_user_store_is_owner_only_wal_and_carries_its_own_application_id() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    commit(
        &guard,
        create("cloud", vec![added("cloud", "api.anthropic.com")]),
    );

    assert_eq!(mode(&layout.user_db()), 0o600, "user.db");
    assert_eq!(mode(&layout.user_lock()), 0o600, "user.lock");
    for suffix in ["-wal", "-shm"] {
        let side = layout.user_db().with_file_name(format!("user.db{suffix}"));
        if side.exists() {
            assert_eq!(mode(&side), 0o600, "{}", side.display());
        }
    }
    drop(guard);

    let conn = Connection::open(layout.user_db()).unwrap();
    let pragma = |name: &str| -> i64 {
        conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(pragma("application_id"), i64::from(USER_APPLICATION_ID));
    assert_eq!(pragma("user_version"), USER_SCHEMA_VERSION);
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal.to_lowercase(), "wal");
    const { assert!(USER_SCHEMA_VERSION >= 1) };
}

/// The two stores carry distinct, non-zero ids.
#[test]
fn the_user_and_project_stores_have_distinct_application_ids() {
    assert_ne!(USER_APPLICATION_ID, 0);
    assert_ne!(am_store::APPLICATION_ID, 0);
    assert_ne!(USER_APPLICATION_ID, am_store::APPLICATION_ID);

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state.db");
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    let id: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(id, i64::from(am_store::APPLICATION_ID));
}

/// Wrong database: a project store at `user.db` is refused in words naming
/// it as a project's database; nothing is migrated or written into it.
#[test]
fn a_project_store_at_user_db_is_refused_naming_what_it_is() {
    let (_tmp, layout) = app_data();
    drop(Store::open(&layout.user_db()).unwrap());
    let before = std::fs::read(layout.user_db()).unwrap();

    let Err(err) = UserStore::locked(&layout, WAIT) else {
        panic!("refused")
    };
    let words = err.to_string();
    assert!(words.contains("project"), "{words}");
    assert!(words.contains("user.db"), "names the file: {words}");
    let Err(err) = UserReader::open(&layout) else {
        panic!("refused")
    };
    assert!(err.to_string().contains("project"), "{err}");
    assert_eq!(std::fs::read(layout.user_db()).unwrap(), before);
}

/// The other way round: a user store placed as a project's `state.db` is
/// never opened as a project store.
#[test]
fn a_user_store_never_opens_as_a_project_store() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    commit(&guard, create("cloud", Vec::new()));
    drop(guard);

    let Err(err) = Store::open(&layout.user_db()) else {
        panic!("refused")
    };
    let words = err.to_string();
    assert!(
        words.contains("user") || words.contains("Accounts"),
        "names it as this Mac's user store: {words}"
    );
}

/// A newer `user_version` (a newer app wrote it) is refused in words, by
/// the writer and the reader, and nothing is written.
#[test]
fn a_newer_user_store_is_refused_naming_the_newer_binary() {
    let (_tmp, layout) = app_data();
    {
        let conn = Connection::open(layout.user_db()).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA application_id = {USER_APPLICATION_ID}; PRAGMA user_version = {};",
            USER_SCHEMA_VERSION + 7
        ))
        .unwrap();
    }
    let before = std::fs::read(layout.user_db()).unwrap();

    let Err(err) = UserStore::locked(&layout, WAIT) else {
        panic!("refused")
    };
    let words = err.to_string();
    assert!(words.contains("newer"), "{words}");
    assert!(
        words.contains(&(USER_SCHEMA_VERSION + 7).to_string()),
        "{words}"
    );
    let Err(err) = UserReader::open(&layout) else {
        panic!("refused")
    };
    assert!(err.to_string().contains("newer"), "{err}");
    assert_eq!(std::fs::read(layout.user_db()).unwrap(), before);
}

// --------------------------------------------------------------- the lock

/// Lock held: while one guard holds `user.lock` (a separate file
/// description, as another process has), a second `locked` waits its
/// bounded time and is refused in words; nothing is written meanwhile.
#[test]
fn a_held_lock_refuses_after_the_bounded_wait() {
    let (_tmp, layout) = app_data();
    let held = UserStore::locked(&layout, WAIT).unwrap();
    commit(&held, create("cloud", Vec::new()));

    let started = Instant::now();
    let Err(err) = UserStore::locked(&layout, Duration::from_millis(400)) else {
        panic!("refused while held")
    };
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(400),
        "it waits before refusing: {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(5),
        "the wait is bounded: {waited:?}"
    );
    let words = err.to_string();
    assert!(words.contains("user.lock"), "names the lock: {words}");
    assert!(
        words.contains("another") || words.contains("held"),
        "says who has it: {words}"
    );
    drop(held);
    assert_eq!(
        UserReader::open(&layout)
            .unwrap()
            .config_history(None)
            .unwrap()
            .len(),
        1
    );
    UserStore::locked(&layout, WAIT).expect("free again after the drop");
}

/// Concurrency (store side): two threads, each with its own guard, write
/// one after the other; both commit and the audit chain verifies.
#[test]
fn two_writers_take_turns_and_the_chain_verifies() {
    let (_tmp, layout) = app_data();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|id| {
            let layout = layout.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let guard = UserStore::locked(&layout, WAIT).unwrap();
                let intent = create(id, vec![added(id, "api.anthropic.com")]);
                let pending = guard.begin_intent(owner(), intent).unwrap();
                // Hold the lock a little: the other thread waits.
                std::thread::sleep(Duration::from_millis(150));
                guard.commit_entry(owner(), pending).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let reader = UserReader::open(&layout).unwrap();
    assert_eq!(reader.config_history(None).unwrap().len(), 2);
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    let actions: Vec<String> = audit(&layout)
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        actions.iter().filter(|a| *a == "config.created").count(),
        2,
        "{actions:?}"
    );
}

// ------------------------------------------------------------- the reader

/// A missing `user.db` reads as empty, and the reader creates nothing.
#[test]
fn a_missing_user_store_reads_as_empty_and_is_not_created() {
    let (_tmp, layout) = app_data();
    let reader = UserReader::open(&layout).unwrap();
    assert!(reader.config_history(None).unwrap().is_empty());
    assert!(reader.config_intents().unwrap().is_empty());
    assert!(
        reader
            .config_note_args("config.widened", "cloud")
            .unwrap()
            .is_empty()
    );
    let mut out = Vec::new();
    assert_eq!(reader.export_audit_jsonl(&mut out).unwrap().0, 0);
    assert!(!layout.user_db().exists(), "the reader creates no user.db");
    assert!(!layout.user_lock().exists(), "nor a lock");
}

/// The reader takes no lock: it reads while a guard holds `user.lock`, and
/// it cannot write.
#[test]
fn the_reader_takes_no_lock_and_cannot_write() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    commit(
        &guard,
        create("cloud", vec![added("cloud", "api.anthropic.com")]),
    );

    let reader = UserReader::open(&layout).unwrap();
    let history = reader.config_history(Some("accounts.yaml")).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Create);
    let notes = reader.config_note_args("config.widened", "cloud").unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].actor, owner());
    assert_eq!(notes[0].file, "accounts.yaml");
    assert_eq!(
        notes[0].args.get("host").map(String::as_str),
        Some("api.anthropic.com")
    );
    drop(guard);

    let conn =
        Connection::open_with_flags(layout.user_db(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert!(conn.execute_batch("CREATE TABLE x (y)").is_err());
}

// ---------------------------------------------------------- one executor

/// The user store records a config write exactly as a project store does
/// (one executor over a transaction): the same intent becomes the same
/// History entry and the same `config.*` and note entries, apart from the
/// ids, times and hashes.
#[test]
fn a_config_write_is_recorded_alike_in_both_stores() {
    let (_tmp, layout) = app_data();
    let intent = create("cloud", vec![added("cloud", "api.anthropic.com")]);

    let guard = UserStore::locked(&layout, WAIT).unwrap();
    commit(&guard, intent.clone());
    let user_entry = guard.entries(None).unwrap().remove(0);
    drop(guard);

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("state.db")).unwrap();
    let project = StoreConfigHistory::new(&store).unwrap();
    commit(&project, intent);
    let project_entry = project.entries(None).unwrap().remove(0);

    let shape = |e: &am_domain::HistoryEntry| {
        (
            e.actor.clone(),
            e.file.clone(),
            e.kind,
            e.before.clone(),
            e.after.clone(),
            e.after_sha256.clone(),
            e.diff.clone(),
        )
    };
    assert_eq!(shape(&user_entry), shape(&project_entry));

    let strip = |entries: Vec<Value>| -> Vec<Value> {
        entries
            .into_iter()
            .filter(|e| {
                let a = e["action"].as_str().unwrap();
                a.starts_with("config.")
            })
            .map(|mut e| {
                let o = e.as_object_mut().unwrap();
                for k in ["seq", "at", "prev_hash", "entry_hash", "target"] {
                    o.remove(k);
                }
                if let Some(args) = o.get_mut("args").and_then(Value::as_object_mut) {
                    args.remove("entry_id");
                }
                e
            })
            .collect()
    };
    let mut out = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    let project_audit: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(strip(audit(&layout)), strip(project_audit));
}

/// The guard's audit append writes one chained entry under the lock (an
/// entry that is not a `config.*` one: those go only with their intent,
/// review fix G6, 2026-11-22).
#[test]
fn the_guard_appends_an_audit_entry_to_the_chain() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    guard
        .append_audit(AuditEvent {
            actor: owner(),
            action: "accounts.tested".into(),
            target: "cloud".into(),
            outcome: "ok".into(),
            hash: None,
            args: BTreeMap::from([("kind".to_string(), "account_tested".to_string())]),
        })
        .unwrap();
    drop(guard);
    let entries = audit(&layout);
    let tested: Vec<&Value> = entries
        .iter()
        .filter(|e| e["args"]["kind"] == "account_tested")
        .collect();
    assert_eq!(tested.len(), 1, "{entries:?}");
    assert_eq!(tested[0]["target"], "cloud");
}

// ======================================================================
// Review fixes (2026-11-22 review pass; plan `## Review Triage Log`),
// written first, before any `src/` change.
// ======================================================================

/// G6 (Quick 6, Adversarial 6): `append_audit` writes no `config.*` entry
/// at all (`config.narrowed` with `kind: account_removed`, `config.edited`,
/// `config.widened`, `config.created`): a config entry goes only with its
/// intent, through the shared executor. Each is refused in words and
/// nothing reaches the chain.
#[test]
fn the_guard_refuses_every_config_audit_entry() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    let mut written = Vec::new();
    for (action, kind) in [
        ("config.narrowed", Some("account_removed")),
        ("config.edited", None),
        ("config.widened", None),
        ("config.created", None),
    ] {
        let result = guard.append_audit(AuditEvent {
            actor: owner(),
            action: action.into(),
            target: "cloud".into(),
            outcome: "ok".into(),
            hash: None,
            args: kind
                .map(|k| BTreeMap::from([("kind".to_string(), k.to_string())]))
                .unwrap_or_default(),
        });
        if result.is_ok() {
            written.push(action);
        }
    }
    drop(guard);
    assert_eq!(written, Vec::<&str>::new(), "written without an intent");
    let entries = audit(&layout);
    assert!(
        entries
            .iter()
            .all(|e| !e["action"].as_str().unwrap_or("").starts_with("config.")),
        "{entries:?}"
    );
}

/// G3 (Quick 3, Adversarial 7): the reader is never opened `immutable`.
/// Opened while `user.db` has no WAL file (the last connection closed), it
/// still sees a write another connection commits afterwards.
#[test]
fn a_reader_opened_without_a_wal_sees_a_later_commit() {
    let (_tmp, layout) = app_data();
    let guard = UserStore::locked(&layout, WAIT).unwrap();
    commit(&guard, create("cloud", Vec::new()));
    drop(guard);
    let wal = layout.dir().join("user.db-wal");
    assert!(
        !wal.exists(),
        "precondition: no WAL file once the last connection closed"
    );

    let reader = UserReader::open(&layout).unwrap();
    assert_eq!(reader.config_history(None).unwrap().len(), 1);

    let guard = UserStore::locked(&layout, WAIT).unwrap();
    commit(&guard, create("second", Vec::new()));
    assert_eq!(
        reader.config_history(None).unwrap().len(),
        2,
        "the reader sees the commit made after it was opened"
    );
    drop(guard);
    assert_eq!(reader.config_history(None).unwrap().len(), 2);
}

/// Adversarial 8b: a non-empty foreign SQLite file at `user.db` (a table of
/// its own, `application_id` 0, `user_version` 0) is refused in words
/// naming the file, not migrated into or stamped.
#[test]
fn a_foreign_database_at_user_db_is_refused_not_migrated() {
    let (_tmp, layout) = app_data();
    {
        let conn = Connection::open(layout.user_db()).unwrap();
        conn.execute_batch("CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('mine');")
            .unwrap();
    }
    std::fs::set_permissions(layout.user_db(), std::fs::Permissions::from_mode(0o600)).unwrap();

    let result = UserStore::locked(&layout, WAIT);
    let conn =
        Connection::open_with_flags(layout.user_db(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let pragma = |name: &str| -> i64 {
        conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
            .unwrap()
    };
    let (application_id, user_version) = (pragma("application_id"), pragma("user_version"));
    let mut tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tables.retain(|t| !t.starts_with("sqlite_"));

    let Err(err) = result else {
        panic!(
            "a foreign database was opened as the user store (application_id {application_id}, user_version {user_version}, tables {tables:?})"
        )
    };
    assert!(err.to_string().contains("user.db"), "names the file: {err}");
    assert_eq!((application_id, user_version), (0, 0), "not stamped");
    assert_eq!(tables, ["notes"], "not migrated into");
}

/// 8b (follow-up): the emptiness test matches the literal `sqlite_` prefix,
/// not `_` as a wildcard: a foreign database whose only table is `sqliteX`
/// is refused in words naming the file, not migrated into or stamped.
#[test]
fn a_foreign_database_whose_only_table_is_sqlitex_is_refused() {
    let (_tmp, layout) = app_data();
    {
        let conn = Connection::open(layout.user_db()).unwrap();
        conn.execute_batch(
            "CREATE TABLE sqliteX (body TEXT); INSERT INTO sqliteX VALUES ('mine');",
        )
        .unwrap();
    }
    std::fs::set_permissions(layout.user_db(), std::fs::Permissions::from_mode(0o600)).unwrap();

    let result = UserStore::locked(&layout, WAIT);
    let conn =
        Connection::open_with_flags(layout.user_db(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let pragma = |name: &str| -> i64 {
        conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
            .unwrap()
    };
    let (application_id, user_version) = (pragma("application_id"), pragma("user_version"));
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    let Err(err) = result else {
        panic!(
            "a foreign database was opened as the user store (application_id {application_id}, user_version {user_version}, tables {tables:?})"
        )
    };
    assert!(err.to_string().contains("user.db"), "names the file: {err}");
    assert_eq!((application_id, user_version), (0, 0), "not stamped");
    assert_eq!(tables, ["sqliteX"], "not migrated into");
}
