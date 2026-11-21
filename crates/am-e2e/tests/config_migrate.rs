//! Story 1.10 acceptance test (R4, AD-7, AD-16): a project whose runtime
//! database is one version back and whose config is one `schema_version`
//! back (the `test-support` step from 0 to 1) opens on a real store: the
//! database migrates after a `VACUUM INTO` backup and the migration is
//! reported; the config migration is offered as a diff and no config file
//! changes until accept; the session refuses while it is pending; after the
//! owner's accept `load` has no errors, History lists one `migration` entry
//! per file with the Actor, and the audit log holds `config.migrated` (no
//! file content in it, chain verifies). Written before the registry exists.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{
    ConfigEvent, ConfigSession, ConfigWriter, SessionError, Severity, accept_migrations,
    config_files, load, migration_offers,
};
use am_domain::{Actor, ConfigHistory, HistoryKind, RuntimeLayout};
use am_store::{SCHEMA_VERSION, Store, StoreConfigHistory, verify_jsonl};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SEAT: &str = "seats/alpha.md";
const WORKFLOW: &str = "workflows/digest.yaml";
/// Stands for private file content: it must never reach the audit log.
const CANARY: &str = "am-config-canary-7b3e keep these instructions private";

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn all_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    config_files(root)
        .into_iter()
        .map(|f| {
            let b = std::fs::read(root.join(&f)).unwrap();
            (f, b)
        })
        .collect()
}

fn to_v0(root: &Path, rel: &str) {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("schema_version: 1"), "{rel}");
    std::fs::write(
        &path,
        text.replacen("schema_version: 1", "schema_version: 0", 1),
    )
    .unwrap();
}

/// The exported audit log (checked), and its `config.*` entries.
fn audit(store: &Store) -> (String, Vec<Value>) {
    let mut out = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    verify_jsonl(out.as_slice()).expect("the audit chain verifies");
    let text = String::from_utf8(out).unwrap();
    let config = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e["action"].as_str().unwrap().starts_with("config."))
        .collect();
    (text, config)
}

fn session(root: &Path, history: Arc<StoreConfigHistory>) -> Result<ConfigSession, SessionError> {
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(|_| {});
    ConfigSession::open(root, history, on_event)
}

#[test]
fn config_migrate_database_and_config_one_version_back_on_a_real_store() {
    // The project: the valid fixture, the seat (with a canary in its body)
    // and the workflow at schema_version 0.
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/valid");
    am_testkit::copy_tree(&fixture, root);
    let seat = std::fs::read_to_string(root.join(SEAT)).unwrap();
    std::fs::write(root.join(SEAT), format!("{seat}{CANARY}\n")).unwrap();
    to_v0(root, SEAT);
    to_v0(root, WORKFLOW);

    // The runtime folder (outside the project, AD-32) with a database one
    // version back.
    let runtime = tempfile::tempdir().unwrap();
    let layout = RuntimeLayout::at(runtime.path().join("rt"));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(layout.dir())
        .unwrap();
    std::fs::set_permissions(layout.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let old = SCHEMA_VERSION - 1;
    assert!(old >= 1, "the store has a migration to step back over");
    am_store::testing::database_at_version(&layout.state_db(), old).unwrap();

    // 1. The database migrates after a backup and reports it.
    let (store, report) = am_store::open_runtime(&layout, "test-mac".into()).unwrap();
    assert!(!report.created);
    assert_eq!(report.version, SCHEMA_VERSION);
    assert_eq!(report.applied.len(), 1, "{report:?}");
    let step = &report.applied[0];
    assert_eq!((step.from, step.to), (old, SCHEMA_VERSION));
    let backup = step.backup.clone().expect("a backup before migrating");
    assert!(backup.is_file(), "{}", backup.display());
    assert_eq!(backup.parent().unwrap(), layout.backups_pre_migration());
    let lines = report.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&format!("migrated from version {old} to {SCHEMA_VERSION}"))),
        "{lines:?}"
    );
    let history = Arc::new(StoreConfigHistory::new(&store).unwrap());

    // 2. The config migration is offered as a diff; nothing changes on disk.
    let before = all_bytes(root);
    let offers = migration_offers(root).expect("the project folder exists");
    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, [SEAT, WORKFLOW]);
    for o in &offers {
        assert_eq!((o.from, o.to), (0, 1));
        assert_eq!(o.before_sha256, sha(&before[&o.file]));
        assert!(o.diff.contains("\n-schema_version: 0\n"), "{}", o.diff);
        assert!(o.diff.contains("\n+schema_version: 1\n"), "{}", o.diff);
    }
    assert_eq!(all_bytes(root), before, "offering writes nothing");

    // 3. The session refuses while the migration is pending; nothing is
    // recorded.
    match session(root, history.clone()) {
        Ok(_) => panic!("a session opened with a migration pending"),
        Err(SessionError::MigrationPending {
            offers: pending, ..
        }) => {
            assert_eq!(pending.len(), 2, "{pending:?}");
        }
        Err(other) => panic!("expected MigrationPending, got {other:?}"),
    }
    assert_eq!(all_bytes(root), before, "no file changes until accept");
    assert!(history.entries(None).unwrap().is_empty());
    let (_, config) = audit(&store);
    assert!(
        config.is_empty(),
        "nothing recorded before accept: {config:#?}"
    );

    // 4. The owner accepts: one `migration` entry per file.
    let writer = ConfigWriter::open(root, history.clone()).unwrap();
    let entries =
        accept_migrations(&writer, reviewer(), offers.clone()).expect("the accept applies");
    assert_eq!(entries.len(), 2);
    for (entry, offer) in entries.iter().zip(&offers) {
        assert_eq!(entry.file, offer.file);
        assert_eq!(entry.kind, HistoryKind::Migration);
        assert_eq!(entry.ref_entry, None);
        assert_eq!(entry.who(), "You (Reviewer)");
        assert_eq!(entry.before, before[&offer.file]);
        assert_eq!(entry.after, offer.after);
        assert_eq!(std::fs::read(root.join(&offer.file)).unwrap(), offer.after);
    }
    let seat_after = std::fs::read_to_string(root.join(SEAT)).unwrap();
    assert!(
        seat_after.ends_with(&format!("{CANARY}\n")),
        "the body is kept"
    );

    let loaded = load(root).unwrap();
    let errors: Vec<_> = loaded
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(migration_offers(root).unwrap().is_empty());
    let listed = history.entries(None).unwrap();
    assert_eq!(listed, entries, "History lists the migration");
    assert!(listed.iter().all(|e| e.kind == HistoryKind::Migration));
    assert!(history.pending_intents().unwrap().is_empty());

    let (text, config) = audit(&store);
    assert!(
        !text.contains("am-config-canary"),
        "file content reached the audit log"
    );
    let actions: Vec<&str> = config
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "config.intent_opened",
            "config.migrated",
            "config.intent_opened",
            "config.migrated"
        ]
    );
    for e in &config {
        assert_eq!(e["args"]["kind"], "migration");
        assert_eq!(
            e["actor"],
            json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
        for k in e["args"].as_object().unwrap().keys() {
            assert!(
                [
                    "after_sha256",
                    "before_sha256",
                    "entry_id",
                    "file",
                    "kind",
                    "ref_entry"
                ]
                .contains(&k.as_str()),
                "{k} in {e}"
            );
        }
    }

    // 5. The session opens on the migrated project.
    let open = session(root, history.clone()).expect("the session opens after accept");
    assert_eq!(
        open.snapshot().hash(),
        loaded.snapshot.expect("no errors gives a snapshot").hash()
    );
}
