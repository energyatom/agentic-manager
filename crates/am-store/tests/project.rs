//! Stories 1.6 and 1.13: opening a project's runtime folder (AD-6, AD-7,
//! AD-32). Since story 1.13 the folder lies outside the project
//! (`<runtime base>/<project-id>/`, resolved by am-app) and am-store opens the
//! folder it is given; the lock record's host is the one it is given (the
//! identity port's Mac name). One test per row of the stories' I/O matrices;
//! files and modes are checked on disk. The "no Actor" row is the
//! `compile_fail` doctest on `Writer::exec` (`cargo test --doc -p am-store`).
//! The process-level rows (`agentic serve --project`) are in
//! `am-app/tests/serve_project.rs`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use am_domain::{Actor, RuntimeLayout};
use am_store::{OpenReport, SCHEMA_VERSION, Store, StoreError, WriteCommand};
use rusqlite::{Connection, OpenFlags};

const CHILD_ENV: &str = "AM_STORE_LOCK_CHILD";
const HOST: &str = "Test Mac (port)";

/// The runtime folder used by these tests: `rt/` in a temporary base.
fn layout(base: &Path) -> RuntimeLayout {
    RuntimeLayout::at(base.join("rt"))
}

fn open_project(base: &Path) -> Result<(Store, OpenReport), StoreError> {
    am_store::open_runtime(&layout(base), HOST.to_string())
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn agentic(base: &Path) -> PathBuf {
    layout(base).dir().to_path_buf()
}

/// The runtime folder made by hand, owner-only, as an earlier open left it.
fn existing_agentic(root: &Path) -> PathBuf {
    let dir = agentic(root);
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn user_version(db: &Path) -> i64 {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    c.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn backups(root: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(layout(root).backups_pre_migration()) {
        Ok(rd) => rd.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

/// (from, to, backup) rows of the `migration_reports` table.
fn report_rows(db: &Path) -> Vec<(i64, i64, Option<String>)> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut st = c
        .prepare("SELECT from_version, to_version, backup FROM migration_reports ORDER BY id")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn sys() -> Actor {
    Actor::System {
        component: "test".into(),
    }
}

// ------------------------------------------------------------ new project

#[test]
fn new_project_creates_an_owner_only_folder_at_the_latest_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let (store, report) = open_project(root).unwrap();

    let ag = agentic(root);
    assert_eq!(mode(&ag), 0o700, "runtime folder mode");
    assert!(!ag.join(".gitignore").exists(), "no .gitignore any more");
    let db = ag.join("state.db");
    assert_eq!(mode(&db), 0o600, "state.db mode");
    assert_eq!(mode(&ag.join("lock")), 0o600, "lock mode");
    assert_eq!(user_version(&db), SCHEMA_VERSION);
    assert!(backups(root).is_empty(), "a new database needs no backup");

    assert!(report.created);
    assert_eq!(report.version, SCHEMA_VERSION);
    assert!(report.applied.iter().all(|m| m.backup.is_none()));
    assert_eq!(
        report.lines(),
        vec![format!("created at version {SCHEMA_VERSION}")]
    );
    // Each step after `migration_reports` exists is kept, in its own
    // transaction (the steps before it cannot be).
    let rows = report_rows(&db);
    let want: Vec<(i64, i64, Option<String>)> =
        (1..SCHEMA_VERSION).map(|v| (v, v + 1, None)).collect();
    assert_eq!(rows, want);

    // Nothing is written outside the runtime folder it was given.
    let top: Vec<String> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(top, vec!["rt".to_string()]);
    let mut inside: Vec<String> = std::fs::read_dir(&ag)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with("state.db-"))
        .collect();
    inside.sort();
    assert_eq!(inside, vec!["lock".to_string(), "state.db".to_string()]);

    // The store works through the single writer.
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::Audit {
                action: "x".into(),
                target: "y".into(),
                outcome: "ok".into(),
                hash: None,
                args: Default::default(),
            },
        )
        .unwrap();
}

// ------------------------------------------------------------ older database

#[test]
fn older_database_is_backed_up_then_migrated_keeping_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let db = ag.join("state.db");
    let old = SCHEMA_VERSION - 1;
    am_store::testing::database_at_version(&db, old).unwrap();
    am_store::testing::insert_fixture_trigger(&db, "fixture-row").unwrap();
    assert_eq!(user_version(&db), old);

    let (store, report) = open_project(root).unwrap();

    assert!(!report.created);
    assert_eq!(report.version, SCHEMA_VERSION);
    assert_eq!(report.applied.len(), 1);
    let step = &report.applied[0];
    assert_eq!((step.from, step.to), (old, SCHEMA_VERSION));
    let backup = step.backup.clone().expect("a backup before migrating");

    // The backup: in backups/pre-migration/, named by time and version,
    // 0600, old data.
    assert_eq!(
        backup.parent().unwrap(),
        layout(root).backups_pre_migration()
    );
    let name = backup.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("state-") && name.ends_with(&format!("-v{old}.db")),
        "{name}"
    );
    assert_eq!(mode(&backup), 0o600, "backup mode");
    assert_eq!(mode(&ag.join("backups")), 0o700, "backups/ mode");
    assert_eq!(
        mode(&layout(root).backups_pre_migration()),
        0o700,
        "pre-migration/ mode"
    );
    assert_eq!(backups(root), vec![backup.clone()]);
    assert_eq!(user_version(&backup), old);
    let b = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let n: i64 = b
        .query_row(
            "SELECT count(*) FROM triggers WHERE id = 'fixture-row'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "the backup holds the old data");

    // Migrated, and the row is kept.
    assert_eq!(user_version(&db), SCHEMA_VERSION);
    assert!(
        store
            .reader()
            .unwrap()
            .trigger("fixture-row")
            .unwrap()
            .is_some()
    );

    // Reported: returned, as a line, and kept for the later banner.
    assert_eq!(
        report.lines(),
        vec![format!(
            "migrated from version {old} to {SCHEMA_VERSION}, backup: {}",
            backup.display()
        )]
    );
    let rows = report_rows(&db);
    assert_eq!(
        rows,
        vec![(old, SCHEMA_VERSION, Some(backup.display().to_string()))]
    );
}

// ------------------------------------------------------------ newer database

#[test]
fn newer_database_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let db = ag.join("state.db");
    let newer = SCHEMA_VERSION + 1;
    am_store::testing::database_at_version(&db, newer).unwrap();
    let before = std::fs::read(&db).unwrap();

    let err = match open_project(root) {
        Err(e) => e,
        Ok(_) => panic!("a newer database must be refused"),
    };
    assert!(
        matches!(err, StoreError::TooNew { found, known } if found == newer && known == SCHEMA_VERSION),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "the database is version {newer}, newer than this app knows ({SCHEMA_VERSION}); update the app"
        )
    );
    assert_eq!(std::fs::read(&db).unwrap(), before, "state.db unchanged");
    assert_eq!(user_version(&db), newer);
    assert!(backups(root).is_empty(), "no backup");
}

#[test]
fn negative_version_is_refused_as_not_ours_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let db = ag.join("state.db");
    am_store::testing::database_at_version(&db, SCHEMA_VERSION).unwrap();
    let c = Connection::open(&db).unwrap();
    c.pragma_update(None, "user_version", -1).unwrap();
    drop(c);
    let before = std::fs::read(&db).unwrap();

    let err = match open_project(root) {
        Err(e) => e,
        Ok(_) => panic!("a negative version must be refused"),
    };
    assert!(matches!(err, StoreError::NotOurs { found: -1 }), "{err:?}");
    assert!(
        err.to_string().contains("not a database this app made"),
        "{err}"
    );
    assert_eq!(std::fs::read(&db).unwrap(), before, "state.db unchanged");
    assert!(backups(root).is_empty(), "no backup");
}

// ------------------------------------------------------------ second open

#[test]
fn second_open_is_refused_naming_the_holder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (store, _) = open_project(root).unwrap();
    let started = std::fs::read_to_string(agentic(root).join("lock")).unwrap();

    let err = match open_project(root) {
        Err(e) => e,
        Ok(_) => panic!("a second open must be refused"),
    };
    let StoreError::Locked { holder, .. } = &err else {
        panic!("expected Locked, got {err:?}");
    };
    let holder = holder.clone().expect("the holder is named");
    assert_eq!(holder.pid, std::process::id());
    assert_eq!(
        holder.host, HOST,
        "the host is the identity port's Mac name"
    );
    let own_command = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['\n', '\r'], " ");
    assert_eq!(holder.command, own_command);
    assert!(started.contains(&holder.started_at));
    let msg = err.to_string();
    for part in [
        format!("pid {}", holder.pid),
        holder.host.clone(),
        holder.command.clone(),
        holder.started_at.clone(),
    ] {
        assert!(msg.contains(&part), "`{part}` missing from: {msg}");
    }

    // The first holder is unaffected.
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::Audit {
                action: "x".into(),
                target: "y".into(),
                outcome: "ok".into(),
                hash: None,
                args: Default::default(),
            },
        )
        .unwrap();
}

#[test]
fn dropping_the_store_lets_the_next_open_in() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (store, _) = open_project(root).unwrap();
    // A cloned writer keeps the read-write connection, and with it the lock.
    let writer = store.writer();
    drop(store);
    assert!(matches!(open_project(root), Err(StoreError::Locked { .. })));
    writer
        .exec(
            sys(),
            WriteCommand::Audit {
                action: "x".into(),
                target: "y".into(),
                outcome: "ok".into(),
                hash: None,
                args: Default::default(),
            },
        )
        .unwrap();
    // The last handle gone, the connection closes and the lock goes with it.
    drop(writer);
    let (_s, report) = open_project(root).unwrap();
    assert!(!report.created);
    assert!(report.applied.is_empty());
    assert!(report.lines().is_empty());
}

// ------------------------------------------------------------ stale lock

/// Runs only inside the re-executed child: open the project, say so, hold it.
#[test]
fn child_holds_the_lock() {
    let Ok(root) = std::env::var(CHILD_ENV) else {
        return;
    };
    let (_store, _) = open_project(Path::new(&root)).unwrap();
    println!("ready {}", std::process::id());
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn stale_lock_left_by_a_killed_process_does_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_holds_the_lock", "--nocapture"])
        .env(CHILD_ENV, root)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let child_pid = loop {
        let mut line = String::new();
        if out.read_line(&mut line).unwrap() == 0 {
            let _ = child.kill();
            panic!("child exited before holding the lock");
        }
        // libtest may print "test … ..." on the same line first.
        if let Some(i) = line.find("ready ") {
            break line[i + 6..].trim().parse::<u32>().unwrap();
        }
    };
    let lock = agentic(root).join("lock");
    assert!(
        std::fs::read_to_string(&lock)
            .unwrap()
            .contains(&format!("pid: {child_pid}"))
    );
    // While it lives, the project is refused.
    assert!(matches!(open_project(root), Err(StoreError::Locked { .. })));

    // SIGKILL: the lock file stays behind, the OS lock does not.
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(lock.exists());

    let (_s, _) = open_project(root).unwrap();
    let text = std::fs::read_to_string(&lock).unwrap();
    assert!(
        text.contains(&format!("pid: {}", std::process::id())),
        "{text}"
    );
    assert!(!text.contains(&format!("pid: {child_pid}\n")), "{text}");
}

// ------------------------------------------------------------ loose permissions

#[test]
fn loose_permissions_on_the_folder_are_refused() {
    for loose in [0o755, 0o750, 0o701] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ag = agentic(root);
        std::fs::create_dir(&ag).unwrap();
        std::fs::set_permissions(&ag, std::fs::Permissions::from_mode(loose)).unwrap();

        let err = match open_project(root) {
            Err(e) => e,
            Ok(_) => panic!("mode {loose:o} must be refused"),
        };
        assert!(
            matches!(err, StoreError::LoosePermissions { .. }),
            "{err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains(&ag.display().to_string()), "{msg}");
        assert!(
            msg.contains(&format!("chmod 700 {}", ag.display())),
            "{msg}"
        );
        // Refused before anything is written.
        assert_eq!(std::fs::read_dir(&ag).unwrap().count(), 0);
        assert_eq!(mode(&ag), loose);
    }
}

/// A kept legacy folder inside the runtime folder is covered by the mode
/// check too (AD-32: exempt from mode repair, not from the check).
#[test]
fn a_loose_kept_legacy_folder_is_refused_not_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let kept = layout(root).legacy_copy("2026-11-21", 1);
    std::fs::create_dir(&kept).unwrap();
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = match open_project(root) {
        Err(e) => e,
        Ok(_) => panic!("a loose kept folder must be refused"),
    };
    assert!(
        err.to_string()
            .contains(&format!("chmod 700 {}", kept.display())),
        "{err}"
    );
    assert_eq!(mode(&kept), 0o755, "not repaired");
    assert!(
        !ag.join("state.db").exists(),
        "refused before anything is written"
    );
}

// ------------------------------------------------------------ backups (1.13)

/// AD-7: five successive migrations leave exactly the three newest
/// pre-migration backups; a kept legacy folder beside them is untouched.
#[test]
fn five_migrations_keep_the_three_newest_backups_and_no_sweep_touches_kept_legacy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let kept = layout(root).legacy_copy("2026-11-21", 1);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&kept)
        .unwrap();
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(kept.join("state.db"), b"kept bytes").unwrap();
    let db = ag.join("state.db");
    let mut made = Vec::new();
    for _ in 0..5 {
        let _ = std::fs::remove_file(&db);
        for side in ["state.db-wal", "state.db-shm"] {
            let _ = std::fs::remove_file(ag.join(side));
        }
        am_store::testing::database_at_version(&db, SCHEMA_VERSION - 1).unwrap();
        let (store, report) = open_project(root).unwrap();
        made.push(report.applied[0].backup.clone().unwrap());
        drop(store);
    }
    let mut left = backups(root);
    left.sort();
    assert_eq!(left.len(), 3, "{left:?}");
    assert_eq!(left, made[2..].to_vec(), "the three newest are kept");
    assert_eq!(std::fs::read(kept.join("state.db")).unwrap(), b"kept bytes");
}

/// The version check the resolver runs before anything is written: a
/// read-only connection, nothing created or changed.
#[test]
fn checking_a_database_reads_only() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    am_store::check_database(&db).unwrap();
    assert!(!db.exists(), "an absent database is not created");
    am_store::testing::database_at_version(&db, SCHEMA_VERSION + 1).unwrap();
    let before = std::fs::read(&db).unwrap();
    let names = || {
        let mut v: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(), vec!["state.db".to_string()]);
    let err = am_store::check_database(&db).unwrap_err();
    assert!(matches!(err, StoreError::TooNew { .. }), "{err:?}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
    // No -wal or -shm file appears beside it (a WAL database).
    assert_eq!(names(), vec!["state.db".to_string()]);
    // A current database passes, and nothing appears beside it either.
    let current = tempfile::tempdir().unwrap();
    let db = current.path().join("state.db");
    am_store::testing::database_at_version(&db, SCHEMA_VERSION).unwrap();
    let side = |d: &std::path::Path| {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    let before = side(current.path());
    am_store::check_database(&db).unwrap();
    assert_eq!(side(current.path()), before);
}
