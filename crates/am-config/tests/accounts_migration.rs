//! Story 2.13 (am-config side; AD-7, AD-15 *Migrate*, AD-16, AD-17): the
//! config writer's `ConfigMigration` gains two steps, the removal of a
//! project's old `models/accounts.yaml` and the rename of an Account id in
//! this project's references (`--as <old>=<new>`). Removal is reachable only
//! through this path, and only for that one file. Written before the steps
//! exist, on copies of `fixtures/projects/legacy-accounts/` (and of
//! `anatomy` and `sources-ok` for a capability's provider and a Source's
//! `account`), with am-testkit's `InMemoryConfigHistory`.
//!
//! The API these tests expect (decided here, plan Code Map "writer.rs: the
//! migration path, plus a removal step and a rename step"):
//!
//! - `am_config::ConfigMigration { steps: Vec<MigrationStep> }`;
//! - `am_config::MigrationStep::Remove { file, before_sha256 }`: removes
//!   `file`, which must still hold the bytes hashed by `before_sha256`
//!   (otherwise `WriteError::ChangedSince`, nothing removed);
//! - `am_config::MigrationStep::RenameAccount { old, new }`: every reference
//!   to the Account `old` in this project (a seat's `model` and `fallback`,
//!   a capability's `provider`, a Source's `account`) becomes `new`, one
//!   `migration` History entry per file, the Markdown body kept byte for
//!   byte;
//! - `ConfigWriter::apply_migration(&self, Actor, ConfigMigration) ->
//!   Result<Vec<HistoryEntry>, WriteError>`, all steps or none, owner only
//!   (`WriteError::NotOwner` otherwise).
//!
//! A removal is a `HistoryKind::Removal` entry: `before` the removed bytes,
//! `after` empty. Its intent is committed before the unlink (the existing
//! `FailPoint::BeforeRename` stops it there, `FailPoint::AfterRename` right
//! after the unlink), and reconcile treats "file absent" as written.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use am_config::{
    ConfigMigration, ConfigWriter, FailPoint, MigrationStep, WriteError, canonical_bytes,
    config_files,
};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::{copy_of, errors, findings};
use sha2::{Digest, Sha256};

const LEGACY_FILE: &str = "models/accounts.yaml";

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap()
}

fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

/// Every file under `root` (config or not) with its bytes.
fn all_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                stack.push(p);
            } else {
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

fn remove_legacy(root: &Path) -> MigrationStep {
    MigrationStep::Remove {
        file: LEGACY_FILE.into(),
        before_sha256: sha(&read(root, LEGACY_FILE)),
    }
}

fn rename(old: &str, new: &str) -> MigrationStep {
    MigrationStep::RenameAccount {
        old: old.into(),
        new: new.into(),
    }
}

/// The seat file's bytes with one reference line renamed, as the canonical
/// writer emits them (the body byte for byte).
fn renamed(file: &str, before: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8(before.to_vec()).unwrap();
    let changed = text.replace(from, to);
    assert_ne!(changed, text, "{file} names `{from}`");
    canonical_bytes(file, changed.as_bytes()).unwrap()
}

// ----------------------------------------------------------------- removal

/// Removal: the old file is gone, one `removal` entry records its bytes as
/// `before` and nothing after; nothing else in the project changed; the
/// project then loads without the old file's error.
#[test]
fn a_migration_removes_the_old_accounts_file_with_its_bytes_in_history() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let before = read(root, LEGACY_FILE);
    let mut others = all_files(root);
    others.remove(LEGACY_FILE);
    let (w, history) = writer(root);

    let entries = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![remove_legacy(root)],
            },
        )
        .unwrap();

    assert!(!root.join(LEGACY_FILE).exists(), "removed");
    assert_eq!(entries.len(), 1, "{entries:#?}");
    let e = &entries[0];
    assert_eq!(e.kind, HistoryKind::Removal);
    assert_eq!(e.file, LEGACY_FILE);
    assert_eq!(e.before, before);
    assert_eq!(e.before_sha256, sha(&before));
    assert!(e.after.is_empty());
    assert_eq!(e.after_sha256, sha(b""));
    assert_eq!(e.ref_entry, None);
    assert_eq!(e.actor, owner());
    assert_eq!(history.entries(None).unwrap(), entries);
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(all_files(root), others, "nothing else changed");
    assert!(errors(&findings(root)).is_empty(), "{:#?}", findings(root));
}

/// The file changed since its hash was taken: nothing is removed and
/// nothing recorded.
#[test]
fn a_removal_of_a_file_that_changed_is_refused() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let step = remove_legacy(root);
    let mut text = read(root, LEGACY_FILE);
    text.extend_from_slice(b"# changed\n");
    std::fs::write(root.join(LEGACY_FILE), &text).unwrap();
    let (w, history) = writer(root);

    let err = w
        .apply_migration(owner(), ConfigMigration { steps: vec![step] })
        .unwrap_err();
    assert!(matches!(err, WriteError::ChangedSince { .. }), "{err:?}");
    assert_eq!(read(root, LEGACY_FILE), text, "not removed");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Removal guard: the writer removes no other file, whatever path names
/// it, and no actor but the owner removes even the old file; nothing is
/// removed or recorded.
#[test]
fn the_writer_removes_no_other_file() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    std::fs::write(root.join("models/other.yaml"), "schema_version: 2\n").unwrap();
    let files = all_files(root);
    let (w, history) = writer(root);

    for file in [
        "company.yaml",
        "seats/alpha.md",
        "workflows/hello.yaml",
        "models/other.yaml",
        "models/../company.yaml",
        "./models/accounts.yaml",
        "models/Accounts.yaml",
        "/models/accounts.yaml",
        "",
    ] {
        let bytes = std::fs::read(root.join(file.trim_start_matches('/'))).unwrap_or_default();
        let err = w
            .apply_migration(
                owner(),
                ConfigMigration {
                    steps: vec![MigrationStep::Remove {
                        file: file.into(),
                        before_sha256: sha(&bytes),
                    }],
                },
            )
            .unwrap_err();
        assert!(
            !matches!(err, WriteError::History(_)),
            "{file}: refused before History: {err:?}"
        );
    }
    // The old file, by anyone but the owner.
    for actor in [
        Actor::System {
            component: "config-writer".into(),
        },
        Actor::Import {
            source: "an imported template".into(),
        },
    ] {
        let err = w
            .apply_migration(
                actor.clone(),
                ConfigMigration {
                    steps: vec![remove_legacy(root)],
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, WriteError::NotOwner { .. }),
            "{actor:?}: {err:?}"
        );
    }
    assert_eq!(all_files(root), files, "nothing removed or changed");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// The writer of this Mac's user Accounts file removes nothing, not even
/// its own file: removal is a project migration's step only.
#[test]
fn the_user_accounts_writer_removes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n";
    std::fs::write(dir.path().join("accounts.yaml"), bytes).unwrap();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(dir.path(), history.clone()).unwrap();

    for file in ["accounts.yaml", LEGACY_FILE] {
        assert!(
            w.apply_migration(
                owner(),
                ConfigMigration {
                    steps: vec![MigrationStep::Remove {
                        file: file.into(),
                        before_sha256: sha(bytes),
                    }],
                },
            )
            .is_err(),
            "{file}"
        );
    }
    assert_eq!(read(dir.path(), "accounts.yaml"), bytes);
    assert!(history.entries(None).unwrap().is_empty());
}

/// No other write removes a file: an edit that empties the old file leaves
/// it in place, and config_files still lists it.
#[test]
fn an_edit_never_removes_the_old_file() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let (w, _history) = writer(root);
    let _ = w.apply(
        owner(),
        am_config::ConfigEdit {
            file: LEGACY_FILE.into(),
            ops: vec![am_config::EditOp::Remove {
                path: vec!["accounts".into()],
            }],
            body: None,
        },
    );
    assert!(root.join(LEGACY_FILE).exists());
    assert!(config_files(root).contains(&LEGACY_FILE.to_string()));
}

// ------------------------------------------------- intent before the unlink

/// A crash before the unlink: the intent was committed first, the file is
/// still there, and reconcile drops the intent (the file holds its old
/// bytes); a crash right after the unlink: reconcile treats the absent file
/// as written and completes the `removal` entry.
#[test]
fn the_removal_intent_is_committed_before_the_unlink() {
    // Before the unlink.
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let before = read(root, LEGACY_FILE);
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    w.fail_at(Some(FailPoint::BeforeRename));
    assert!(
        w.apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![remove_legacy(root)],
            },
        )
        .is_err()
    );
    assert_eq!(read(root, LEGACY_FILE), before, "still there");
    let pending = history.pending_intents().unwrap();
    assert_eq!(pending.len(), 1, "the intent came first");
    assert_eq!(pending[0].intent.kind, HistoryKind::Removal);
    assert_eq!(pending[0].intent.file, LEGACY_FILE);
    assert!(pending[0].intent.after.is_empty());
    drop(w);
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    let report = w
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .unwrap();
    assert!(report.completed.is_empty(), "{report:?}");
    assert_eq!(report.dropped.len(), 1);
    assert!(history.entries(None).unwrap().is_empty());
    assert!(root.join(LEGACY_FILE).exists());

    // Right after the unlink.
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    w.fail_at(Some(FailPoint::AfterRename));
    assert!(
        w.apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![remove_legacy(root)],
            },
        )
        .is_err()
    );
    assert!(!root.join(LEGACY_FILE).exists(), "unlinked");
    assert!(history.entries(None).unwrap().is_empty());
    drop(w);
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    let report = w
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .unwrap();
    assert!(report.dropped.is_empty(), "{report:?}");
    assert_eq!(report.completed.len(), 1);
    assert_eq!(report.completed[0].kind, HistoryKind::Removal);
    assert_eq!(report.completed[0].before, before);
    assert!(history.pending_intents().unwrap().is_empty());
}

// ------------------------------------------------------------------ rename

/// `--as clash=clash-old-host`: every reference to `clash` in this project
/// (a seat's model, another seat's fallback) names `clash-old-host`, in the
/// same migration as the removal; one `migration` entry per renamed file,
/// one `removal` entry; each renamed file is the canonical form of the
/// original with only that reference changed (bodies byte for byte, the
/// word in a body untouched); no other file changed.
#[test]
fn a_rename_changes_every_reference_in_this_project_with_the_removal() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    // A body that says the id in words is not a reference.
    let beta = String::from_utf8(read(root, "seats/beta.md")).unwrap();
    std::fs::write(
        root.join("seats/beta.md"),
        beta.replace(
            "Write the longer draft.",
            "Write the longer draft; account: clash is only words here.",
        ),
    )
    .unwrap();
    let before_beta = read(root, "seats/beta.md");
    let before_gamma = read(root, "seats/gamma.md");
    let mut others = all_files(root);
    for f in ["seats/beta.md", "seats/gamma.md", LEGACY_FILE] {
        others.remove(f);
    }
    let (w, history) = writer(root);

    let entries = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![rename("clash", "clash-old-host"), remove_legacy(root)],
            },
        )
        .unwrap();

    let mut kinds: Vec<(String, HistoryKind)> =
        entries.iter().map(|e| (e.file.clone(), e.kind)).collect();
    kinds.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        kinds,
        vec![
            (LEGACY_FILE.to_string(), HistoryKind::Removal),
            ("seats/beta.md".to_string(), HistoryKind::Migration),
            ("seats/gamma.md".to_string(), HistoryKind::Migration),
        ]
    );
    assert_eq!(history.entries(None).unwrap().len(), 3);

    let beta_after = read(root, "seats/beta.md");
    let text = String::from_utf8(beta_after.clone()).unwrap();
    assert!(text.contains("  account: clash-old-host\n"), "{text}");
    assert!(
        text.ends_with("Write the longer draft; account: clash is only words here.\n"),
        "the body byte for byte: {text}"
    );
    let front = text.split("\n---\n").next().unwrap();
    assert!(!front.contains("account: clash\n"), "{text}");
    assert!(front.contains("account: spare\n"), "other ids stay: {text}");

    assert_eq!(
        read(root, "seats/gamma.md"),
        renamed(
            "seats/gamma.md",
            &before_gamma,
            "  - account: clash\n",
            "  - account: clash-old-host\n"
        )
    );
    let beta_entry = entries.iter().find(|e| e.file == "seats/beta.md").unwrap();
    assert_eq!(beta_entry.before, before_beta);
    assert_eq!(beta_entry.after, beta_after);

    let mut left = all_files(root);
    left.remove("seats/beta.md");
    left.remove("seats/gamma.md");
    assert_eq!(left, others, "no other file changed");
    assert!(!root.join(LEGACY_FILE).exists());
    assert!(errors(&findings(root)).is_empty(), "{:#?}", findings(root));
}

/// A capability's provider and a Source's `account` are references too.
#[test]
fn a_rename_reaches_a_capabilitys_provider_and_a_sources_account() {
    let dir = copy_of("anatomy");
    let root = dir.path();
    let before = read(root, "seats/full.md");
    let (w, _history) = writer(root);
    let entries = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![rename("finder", "finder-two")],
            },
        )
        .unwrap();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].file, "seats/full.md");
    assert_eq!(entries[0].kind, HistoryKind::Migration);
    assert_eq!(
        read(root, "seats/full.md"),
        renamed(
            "seats/full.md",
            &before,
            "    provider: finder\n",
            "    provider: finder-two\n"
        )
    );

    let dir = copy_of("sources-ok");
    let root = dir.path();
    let before = read(root, "sources/docs.yaml");
    let untouched = read(root, "sources/search.yaml");
    let (w, _history) = writer(root);
    let entries = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![rename("crawly", "crawly-two")],
            },
        )
        .unwrap();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].file, "sources/docs.yaml");
    assert_eq!(
        read(root, "sources/docs.yaml"),
        renamed(
            "sources/docs.yaml",
            &before,
            "account: crawly\n",
            "account: crawly-two\n"
        )
    );
    assert_eq!(read(root, "sources/search.yaml"), untouched);
}

/// All or none: a rename step that would fail (a new id that is not a
/// config id) stops the whole migration before anything is written, the
/// removal included.
#[test]
fn a_migration_is_all_or_nothing() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let files = all_files(root);
    let (w, history) = writer(root);
    let err = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![rename("clash", "Not An Id"), remove_legacy(root)],
            },
        )
        .unwrap_err();
    assert!(!matches!(err, WriteError::History(_)), "{err:?}");
    assert_eq!(all_files(root), files, "nothing written or removed");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

// ======================================================================
// Review fixes (2026-11-22 review pass, plan § Review Triage Log).
// ======================================================================

/// Quick 9: Undo and Restore of a `removal` entry are refused in words
/// naming `agentic accounts migrate --rollback` (the one way back); the
/// file stays absent and nothing is recorded.
#[test]
fn undo_and_restore_of_a_removal_are_refused_naming_rollback() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let (w, history) = writer(root);
    let entries = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![remove_legacy(root)],
            },
        )
        .unwrap();
    let removal = entries[0].id.clone();

    for (what, result) in [
        ("undo", w.undo(owner(), &removal)),
        ("restore", w.restore(owner(), &removal)),
    ] {
        let err = result.expect_err(what);
        assert!(
            err.to_string().contains("--rollback"),
            "{what}: names the way back: {err}"
        );
        assert!(!root.join(LEGACY_FILE).exists(), "{what}: still absent");
        assert_eq!(history.entries(None).unwrap().len(), 1, "{what}");
        assert!(history.pending_intents().unwrap().is_empty(), "{what}");
    }
}

/// Adversarial 6: a removal that would fail (the old file changed after
/// the renames were previewed) writes no rename: every step is checked
/// before any is written.
#[test]
fn a_removal_that_would_fail_writes_no_rename() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let preview =
        am_config::account_renames(root, &[("clash".into(), "clash-old-host".into())]).unwrap();
    assert!(!preview.is_empty());
    let step = remove_legacy(root);
    let mut changed = read(root, LEGACY_FILE);
    changed.extend_from_slice(b"# changed after the preview\n");
    std::fs::write(root.join(LEGACY_FILE), &changed).unwrap();
    let files = all_files(root);
    let (w, history) = writer(root);

    let err = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![rename("clash", "clash-old-host"), step],
            },
        )
        .unwrap_err();

    assert!(
        !matches!(err, WriteError::MigrationStopped { .. }),
        "{err:?}"
    );
    assert_eq!(all_files(root), files, "no rename written");
    assert!(history.entries(None).unwrap().is_empty());
}

/// A `ConfigHistory` that changes the old file on disk right after the
/// first History entry commits (another program writing it mid-migration).
struct ChangesTheOldFile {
    inner: InMemoryConfigHistory,
    path: std::path::PathBuf,
    done: std::sync::atomic::AtomicBool,
    /// When set, the old file is moved here and replaced by a link to it
    /// (same bytes) instead of being appended to.
    link_to: Option<std::path::PathBuf>,
}

impl ConfigHistory for ChangesTheOldFile {
    fn begin_intent(
        &self,
        actor: Actor,
        intent: am_domain::NewIntent,
    ) -> Result<String, am_domain::HistoryError> {
        self.inner.begin_intent(actor, intent)
    }
    fn commit_entry(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<am_domain::HistoryEntry, am_domain::HistoryError> {
        let entry = self.inner.commit_entry(actor, intent_id)?;
        if !self.done.swap(true, std::sync::atomic::Ordering::SeqCst) {
            match &self.link_to {
                Some(target) => {
                    std::fs::rename(&self.path, target).unwrap();
                    std::os::unix::fs::symlink(target, &self.path).unwrap();
                }
                None => {
                    let mut b = std::fs::read(&self.path).unwrap();
                    b.extend_from_slice(b"# written meanwhile\n");
                    std::fs::write(&self.path, b).unwrap();
                }
            }
        }
        Ok(entry)
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

/// Adversarial 6: a migration stopped part-way (the old file changed while
/// the renames were written) is completed by a re-run: the rename whose
/// `old` is no longer named is skipped, and the removal is made.
#[test]
fn a_re_run_completes_a_partial_migration() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let history = Arc::new(ChangesTheOldFile {
        inner: InMemoryConfigHistory::default(),
        path: root.join(LEGACY_FILE),
        done: std::sync::atomic::AtomicBool::new(false),
        link_to: None,
    });
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    let first = w.apply_migration(
        owner(),
        ConfigMigration {
            steps: vec![rename("clash", "clash-old-host"), remove_legacy(root)],
        },
    );
    assert!(first.is_err(), "stopped: {first:?}");
    assert!(root.join(LEGACY_FILE).exists(), "not removed");
    let written = history.entries(None).unwrap().len();

    let again = w
        .apply_migration(
            owner(),
            ConfigMigration {
                steps: vec![rename("clash", "clash-old-host"), remove_legacy(root)],
            },
        )
        .unwrap();

    assert!(!root.join(LEGACY_FILE).exists(), "removed by the re-run");
    assert!(
        again
            .iter()
            .any(|e| e.file == LEGACY_FILE && e.kind == HistoryKind::Removal)
    );
    for file in ["seats/beta.md", "seats/gamma.md"] {
        let text = String::from_utf8(read(root, file)).unwrap();
        assert!(text.contains("account: clash-old-host\n"), "{file}: {text}");
        assert!(!text.contains("account: clash\n"), "{file}: {text}");
    }
    assert_eq!(
        history.entries(None).unwrap().len(),
        written + again.len(),
        "each file written once"
    );
}

/// Adversarial 7/8: a linked old file, or a linked `models/` folder, is
/// refused before any step is written.
#[test]
fn a_linked_old_file_or_folder_is_refused_before_any_write() {
    for linked in ["file", "folder"] {
        let dir = copy_of("legacy-accounts");
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let before_sha = sha(&read(root, LEGACY_FILE));
        match linked {
            "file" => {
                let target = outside.path().join("accounts.yaml");
                std::fs::rename(root.join(LEGACY_FILE), &target).unwrap();
                std::os::unix::fs::symlink(&target, root.join(LEGACY_FILE)).unwrap();
            }
            _ => {
                let target = outside.path().join("models");
                std::fs::rename(root.join("models"), &target).unwrap();
                std::os::unix::fs::symlink(&target, root.join("models")).unwrap();
            }
        }
        let seats = (read(root, "seats/beta.md"), read(root, "seats/gamma.md"));
        let (w, history) = writer(root);
        let err = w
            .apply_migration(
                owner(),
                ConfigMigration {
                    steps: vec![
                        rename("clash", "clash-old-host"),
                        MigrationStep::Remove {
                            file: LEGACY_FILE.into(),
                            before_sha256: before_sha.clone(),
                        },
                    ],
                },
            )
            .unwrap_err();
        assert!(
            !matches!(err, WriteError::MigrationStopped { .. }),
            "{linked}: {err:?}"
        );
        assert_eq!(
            (read(root, "seats/beta.md"), read(root, "seats/gamma.md")),
            seats,
            "{linked}: no rename"
        );
        assert!(history.entries(None).unwrap().is_empty(), "{linked}");
        assert!(std::fs::symlink_metadata(root.join("models")).is_ok());
    }
}

/// Adversarial 7: the removal resolves its path again right before the read
/// and the unlink: an old file swapped for a link (to a copy with the same
/// bytes) while the renames were written is not unlinked as if it were the
/// file; the migration stops, the link and its target stay.
#[test]
fn a_removal_resolves_the_old_file_again_before_the_unlink() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("accounts.yaml");
    let bytes = read(root, LEGACY_FILE);
    let history = Arc::new(ChangesTheOldFile {
        inner: InMemoryConfigHistory::default(),
        path: root.join(LEGACY_FILE),
        done: std::sync::atomic::AtomicBool::new(false),
        link_to: Some(target.clone()),
    });
    let w = ConfigWriter::open(root, history.clone()).unwrap();

    let result = w.apply_migration(
        owner(),
        ConfigMigration {
            steps: vec![rename("clash", "clash-old-host"), remove_legacy(root)],
        },
    );

    let written = result
        .as_ref()
        .map(|v| v.iter().map(|e| e.file.clone()).collect::<Vec<_>>());
    assert!(result.is_err(), "a link is never removed: {written:?}");
    let meta = std::fs::symlink_metadata(root.join(LEGACY_FILE)).expect("the link stays");
    assert!(meta.file_type().is_symlink());
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    assert!(
        !history
            .entries(None)
            .unwrap()
            .iter()
            .any(|e| e.kind == HistoryKind::Removal),
        "no removal recorded"
    );
}

// ======================================================================
// Follow-up fixes (2026-11-22 follow-up review pass).
// ======================================================================

/// A `ConfigHistory` that deletes the old file right after a removal's
/// intent is committed (so the removal's re-read fails).
struct DeletesAfterRemovalIntent {
    inner: InMemoryConfigHistory,
    path: std::path::PathBuf,
}

impl ConfigHistory for DeletesAfterRemovalIntent {
    fn begin_intent(
        &self,
        actor: Actor,
        intent: am_domain::NewIntent,
    ) -> Result<String, am_domain::HistoryError> {
        let removal = intent.kind == HistoryKind::Removal;
        let id = self.inner.begin_intent(actor, intent)?;
        if removal {
            std::fs::remove_file(&self.path).unwrap();
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

/// Follow-up 4: a removal whose re-read fails (the old file is gone after
/// the intent was committed) is refused and leaves no pending intent.
#[test]
fn a_removal_whose_re_read_fails_leaves_no_pending_intent() {
    let dir = copy_of("legacy-accounts");
    let root = dir.path();
    let history = Arc::new(DeletesAfterRemovalIntent {
        inner: InMemoryConfigHistory::default(),
        path: root.join(LEGACY_FILE),
    });
    let w = ConfigWriter::open(root, history.clone()).unwrap();

    let result = w.apply_migration(
        owner(),
        ConfigMigration {
            steps: vec![remove_legacy(root)],
        },
    );

    assert!(result.is_err(), "refused");
    assert!(
        history.pending_intents().unwrap().is_empty(),
        "no pending intent: {:?}",
        history.pending_intents().unwrap()
    );
    assert!(history.entries(None).unwrap().is_empty());
}
