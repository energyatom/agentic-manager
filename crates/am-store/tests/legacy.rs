//! Story 1.13: moving a legacy in-project runtime folder (`<project>/.agentic/`)
//! into the runtime folder (AD-32). One test per legacy row of the story's
//! I/O matrix at the store level: the database copied with `VACUUM INTO`,
//! every other file stream-copied and checked by sha256, the old folder kept
//! as `legacy-agentic-<date>/` (renamed, or copied and verified on a cloud
//! root), the journal that a crashed move resumes from, an unverified copy
//! refused with the old folder untouched, a held legacy lock refused, and a
//! file whose clone is refused still copied by stream. The resolver's order
//! around it (the mint, the id write, the report) is am-app's
//! `tests/runtime_open.rs`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use am_domain::RuntimeLayout;
use am_domain::runtime::{LEGACY_COPY_PREFIX, legacy_dir};
use am_store::legacy::{self, FailPoint, LegacyMove, Phase};
use am_store::{SCHEMA_VERSION, StoreError};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A project (`p/`) with a legacy folder as story 1.6 left it, and a fresh
/// runtime folder (`base/rt/`, 0700) the resolver made for it.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    layout: RuntimeLayout,
}

fn fixture(with_db: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    write(&root.join("company.yaml"), b"schema_version: 1\n");
    let old = legacy_dir(&root);
    std::fs::DirBuilder::new().mode(0o700).create(&old).unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
    write(&old.join(".gitignore"), b"*\n");
    write(
        &old.join("lock"),
        b"pid: 1\nhost: old\ncommand: agentic\nstarted: x\n",
    );
    write(
        &old.join("knowledge/seat-one/guide.md"),
        b"# Guide\nthe reference text\n",
    );
    write(&old.join("knowledge/seat-one/terms.txt"), b"terms");
    if with_db {
        let db = old.join("state.db");
        am_store::testing::database_at_version(&db, SCHEMA_VERSION - 1).unwrap();
        am_store::testing::insert_fixture_trigger(&db, "legacy-row").unwrap();
    }
    let base = dir.path().join("base");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&base)
        .unwrap();
    let layout = RuntimeLayout::at(base.join("rt"));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(layout.dir())
        .unwrap();
    std::fs::set_permissions(layout.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    Fixture {
        _dir: dir,
        root,
        layout,
    }
}

fn request(f: &Fixture, copy_always: bool) -> LegacyMove {
    LegacyMove {
        project_root: f.root.clone(),
        layout: f.layout.clone(),
        project_id: "0196a6e2-0000-7000-8000-000000000001".into(),
        id_minted: true,
        copy_always,
    }
}

/// Every file under `dir` as (relative path, sha256 of its bytes), sorted.
fn tree(dir: &Path) -> Vec<(String, String)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                let bytes = std::fs::read(&p).unwrap();
                out.push((rel, hex::encode(Sha256::digest(&bytes))));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Every folder and file mode under `dir` (itself included).
fn modes(dir: &Path) -> Vec<(PathBuf, u32)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, u32)>) {
        out.push((dir.to_path_buf(), mode(dir)));
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                walk(&e.path(), out);
            } else {
                out.push((e.path(), mode(&e.path())));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn assert_owner_only(dir: &Path) {
    for (p, m) in modes(dir) {
        let want = if p.is_dir() { 0o700 } else { 0o600 };
        assert_eq!(m, want, "{}", p.display());
    }
}

fn trigger_rows(db: &Path) -> i64 {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    c.query_row(
        "SELECT count(*) FROM triggers WHERE id = 'legacy-row'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn today() -> String {
    jiff::Timestamp::now().strftime("%Y-%m-%d").to_string()
}

#[test]
fn a_legacy_folder_with_a_database_moves_out_and_is_kept() {
    let f = fixture(true);
    let before = tree(&legacy_dir(&f.root));
    let moved = legacy::move_legacy(&request(&f, false)).unwrap();

    // The store is the VACUUM INTO copy, with the rows.
    assert!(moved.had_database);
    assert_eq!(trigger_rows(&f.layout.state_db()), 1);
    // Every other file is copied and checked; lock, .gitignore and the
    // database's side files are not.
    assert_eq!(
        std::fs::read(f.layout.knowledge("seat-one").join("guide.md")).unwrap(),
        b"# Guide\nthe reference text\n"
    );
    assert!(!f.layout.dir().join(".gitignore").exists());
    assert!(!f.layout.lock().exists());
    // The old folder left the project as `legacy-agentic-<today>/`, intact.
    assert!(!legacy_dir(&f.root).exists());
    assert_eq!(moved.kept, f.layout.legacy_copy(&today(), 1));
    assert_eq!(tree(&moved.kept), before);
    assert_owner_only(&moved.kept);
    assert_owner_only(&f.layout.knowledge_dir());
    assert_eq!(mode(&f.layout.state_db()), 0o600);
    // The journal waits for the id write, then ends.
    let j = legacy::read_journal(&f.layout).unwrap().unwrap();
    assert!(j.unfinished());
    assert_eq!(j.phase, Phase::Moving);
    assert_eq!(j.project_id, "0196a6e2-0000-7000-8000-000000000001");
    legacy::mark(&f.layout, Phase::IdWritten).unwrap();
    legacy::mark(&f.layout, Phase::Done).unwrap();
    assert!(
        !legacy::read_journal(&f.layout)
            .unwrap()
            .unwrap()
            .unfinished()
    );
}

#[test]
fn a_legacy_folder_with_only_reference_files_moves_the_same_way() {
    let f = fixture(false);
    let moved = legacy::move_legacy(&request(&f, false)).unwrap();
    assert!(!moved.had_database);
    assert!(
        !f.layout.state_db().exists(),
        "the store is made fresh later"
    );
    assert!(f.layout.knowledge("seat-one").join("terms.txt").is_file());
    assert!(!legacy_dir(&f.root).exists());
    assert!(moved.kept.join("knowledge/seat-one/terms.txt").is_file());
}

#[test]
fn a_taken_name_gets_the_next_number() {
    let f = fixture(false);
    let taken = f.layout.legacy_copy(&today(), 1);
    std::fs::create_dir(&taken).unwrap();
    let moved = legacy::move_legacy(&request(&f, false)).unwrap();
    assert_eq!(moved.kept, f.layout.legacy_copy(&today(), 2));
    assert_eq!(std::fs::read_dir(&taken).unwrap().count(), 0, "untouched");
    let name = moved
        .kept
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with(LEGACY_COPY_PREFIX), "{name}");
}

#[test]
fn on_a_cloud_root_the_old_folder_is_copied_verified_then_removed() {
    let f = fixture(true);
    let before = tree(&legacy_dir(&f.root));
    let moved = legacy::move_legacy(&request(&f, true)).unwrap();
    assert!(legacy::testing::copied_kept_folder(), "the copy path ran");
    assert!(!legacy_dir(&f.root).exists());
    let after = tree(&moved.kept);
    assert_eq!(after, before);
    assert_owner_only(&moved.kept);
}

#[test]
fn a_crash_after_the_copy_resumes_from_the_journal() {
    let f = fixture(true);
    legacy::testing::fail_at(Some(FailPoint::AfterCopy));
    let err = legacy::move_legacy(&request(&f, false)).unwrap_err();
    assert!(err.to_string().contains("simulated crash"), "{err}");
    assert!(
        legacy_dir(&f.root).exists(),
        "the old folder is still there"
    );
    assert_eq!(
        legacy::read_journal(&f.layout).unwrap().unwrap().phase,
        Phase::Copied
    );
    legacy::testing::fail_at(None);
    let moved = legacy::move_legacy(&request(&f, false)).unwrap();
    assert_eq!(trigger_rows(&f.layout.state_db()), 1);
    assert!(!legacy_dir(&f.root).exists());
    assert!(moved.kept.join("state.db").is_file());
}

#[test]
fn a_crash_after_the_rename_resumes_without_moving_twice() {
    let f = fixture(true);
    legacy::testing::fail_at(Some(FailPoint::AfterRename));
    legacy::move_legacy(&request(&f, false)).unwrap_err();
    assert!(!legacy_dir(&f.root).exists());
    let j = legacy::read_journal(&f.layout).unwrap().unwrap();
    assert_eq!(j.phase, Phase::Moving);
    let kept_name = j.kept.clone().unwrap();
    legacy::testing::fail_at(None);
    let moved = legacy::move_legacy(&request(&f, false)).unwrap();
    assert_eq!(moved.kept, f.layout.dir().join(kept_name));
    assert_eq!(trigger_rows(&f.layout.state_db()), 1);
}

#[test]
fn an_unverified_copy_leaves_the_old_folder_untouched_and_refuses() {
    let f = fixture(true);
    let before = tree(&legacy_dir(&f.root));
    legacy::testing::fail_at(Some(FailPoint::CorruptCopy));
    let err = legacy::move_legacy(&request(&f, false)).unwrap_err();
    assert!(
        matches!(err, StoreError::LegacyUnverified { .. }),
        "{err:?}"
    );
    let msg = err.to_string();
    assert!(msg.contains("did not match"), "{msg}");
    assert!(msg.contains("left where it is"), "{msg}");
    assert_eq!(
        tree(&legacy_dir(&f.root)),
        before,
        "the old folder untouched"
    );
}

#[test]
fn a_file_whose_clone_is_refused_is_still_copied_by_stream_and_verified() {
    let f = fixture(false);
    let armed = legacy_dir(&f.root).join("knowledge/seat-one/guide.md");
    am_store::testing::refuse_clone_of(&armed);
    let moved = legacy::move_legacy(&request(&f, true)).unwrap();
    // Nothing in the store can clone (clippy bans `std::fs::copy`); the
    // armed file reached the stream copy and was verified.
    assert!(am_store::testing::stream_copies_of_refused() >= 1);
    assert_eq!(
        std::fs::read(f.layout.knowledge("seat-one").join("guide.md")).unwrap(),
        b"# Guide\nthe reference text\n"
    );
    assert!(moved.kept.join("knowledge/seat-one/guide.md").is_file());
}

#[test]
fn a_held_legacy_lock_is_refused_and_the_lock_file_is_not_rewritten() {
    let f = fixture(true);
    let lock_path = legacy_dir(&f.root).join("lock");
    let before = std::fs::read(&lock_path).unwrap();
    let held = std::fs::File::open(&lock_path).unwrap();
    held.try_lock().unwrap();
    let err = legacy::check_lock(&legacy_dir(&f.root)).unwrap_err();
    assert!(matches!(err, StoreError::LegacyLocked { .. }), "{err:?}");
    assert!(err.to_string().contains("still open"), "{err}");
    let err = legacy::move_legacy(&request(&f, false)).unwrap_err();
    assert!(matches!(err, StoreError::LegacyLocked { .. }), "{err:?}");
    assert_eq!(std::fs::read(&lock_path).unwrap(), before);
    assert!(legacy_dir(&f.root).join("state.db").exists());
    drop(held);
    legacy::check_lock(&legacy_dir(&f.root)).unwrap();
    assert_eq!(std::fs::read(&lock_path).unwrap(), before);
}
