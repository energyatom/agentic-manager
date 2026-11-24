//! Forward-only migrations by `user_version` (AD-7). Owner decision (a),
//! story 1.6: a small project-owned migrator instead of rusqlite_migration
//! (its 2.7.0 needs rusqlite ^0.41; the Stack pins =0.43.0).
//!
//! - Migrations are the numbered SQL files in `crates/am-store/migrations/`,
//!   embedded with `include_str!`, applied in order, each in its own
//!   transaction that also sets `user_version`. They never run backwards.
//! - A database newer than this binary is refused before anything is written.
//! - Before migrating an existing database (version >= 1), a `VACUUM INTO`
//!   copy is written to `backups/pre-migration/state-<UTC timestamp>-v<from>.db`
//!   (mode 0600; the folder is the caller's, named by `RuntimeLayout`). A new
//!   database needs no backup. Once every step committed, only the three
//!   newest pre-migration copies are kept (AD-7, amended 2026-11-20); nothing
//!   else in the folder is touched.
//! - Every applied migration is returned in the [`OpenReport`], kept in
//!   `migration_reports` once that table exists, and audited
//!   (`store.migrated`, `args` NULL) once `audit_log` exists, so a new
//!   database's first step is audited too; both in the step's own
//!   transaction. A failed step's error names the backup.
//! - Story 3.3: a step may rebuild a table (SQLite's own procedure: create
//!   the new table, copy, drop, rename), which foreign key enforcement
//!   would refuse half-way. So the steps run with `foreign_keys` off and
//!   each counts the broken references (`PRAGMA foreign_key_check`) before
//!   and after its SQL, inside its transaction: a step that breaks one
//!   fails and rolls back (one an older bug left broken does not count).
//!   Enforcement is switched back on afterwards, also on error.

use std::path::{Path, PathBuf};

use am_domain::{Actor, RuntimeLayout};
use rusqlite::{Connection, Transaction, params};

use crate::{StoreError, audit, fsx, now_ts};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_migration_reports.sql"),
    include_str!("../migrations/0003_audit_args.sql"),
    include_str!("../migrations/0004_config_history.sql"),
    include_str!("../migrations/0005_config_intent_audit.sql"),
    include_str!("../migrations/0006_run_pin_and_times.sql"),
    include_str!("../migrations/0007_steps_inputs_inbox.sql"),
    include_str!("../migrations/0008_inbox_resolution.sql"),
    include_str!("../migrations/0009_prompt_parts.sql"),
];

/// The schema version this binary knows.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// One store's migration set (story 2.12, AD-7): its numbered SQL files,
/// the `application_id` it stamps (so neither store ever opens as the
/// other) and the name prefix of its pre-migration copies.
pub(crate) struct MigrationSet {
    pub(crate) migrations: &'static [&'static str],
    pub(crate) application_id: i32,
    pub(crate) backup_prefix: &'static str,
}

/// The project store's set.
pub(crate) const PROJECT: MigrationSet = MigrationSet {
    migrations: MIGRATIONS,
    application_id: crate::APPLICATION_ID,
    backup_prefix: "state",
};

/// One applied migration: `from` to `to` (`to = from + 1`), with the backup
/// taken before the first of them (none for a new database).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub from: i64,
    pub to: i64,
    pub backup: Option<PathBuf>,
}

/// How many pre-migration backups are kept (AD-7).
pub const PRE_MIGRATION_KEPT: usize = 3;

/// What opening the database did to its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenReport {
    /// The version the database is at now.
    pub version: i64,
    /// The database was new (version 0) and was created at `version`.
    pub created: bool,
    pub applied: Vec<AppliedMigration>,
    /// The kept legacy folder, when this open moved a legacy in-project
    /// runtime folder out of the project (AD-32); reported like a migration.
    pub legacy_kept: Option<PathBuf>,
}

impl OpenReport {
    /// The owner-facing lines, one per applied migration (one line for a new
    /// database), then the legacy move's line; empty when nothing was applied.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = self.schema_lines();
        if let Some(kept) = &self.legacy_kept {
            lines.push(format!(
                "moved the project's old runtime folder out of the project; it is kept at {}: it still holds data that later deletions do not reach, and you may delete it by hand",
                kept.display()
            ));
        }
        lines
    }

    fn schema_lines(&self) -> Vec<String> {
        if self.created {
            return vec![format!("created at version {}", self.version)];
        }
        self.applied
            .iter()
            .map(|m| match &m.backup {
                Some(b) => format!(
                    "migrated from version {} to {}, backup: {}",
                    m.from,
                    m.to,
                    b.display()
                ),
                None => format!("migrated from version {} to {}", m.from, m.to),
            })
            .collect()
    }
}

pub(crate) fn user_version(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// A database newer than this binary is refused, in plain words; so is a
/// negative version, which no build of this app ever wrote.
pub(crate) fn refuse_newer(found: i64) -> Result<(), StoreError> {
    refuse_unknown(found, SCHEMA_VERSION)
}

fn refuse_unknown(found: i64, known: i64) -> Result<(), StoreError> {
    if found < 0 {
        return Err(StoreError::NotOurs { found });
    }
    if found > known {
        return Err(StoreError::TooNew { found, known });
    }
    Ok(())
}

/// The tail of a migration error: the backup to restore from, if one was taken.
pub(crate) fn backup_words(backup: &Option<PathBuf>) -> String {
    match backup {
        Some(b) => format!("; backup: {}", b.display()),
        None => String::new(),
    }
}

/// One migration in its own transaction, with `user_version`. When `report`
/// is set, the step's report row (once `migration_reports` exists) and its
/// audit entry (once `audit_log` exists, `args` NULL so it can be written
/// at any version) commit in the same transaction. `finish`, when given,
/// runs last in the step's transaction (a new store's creating mark).
fn apply_step(
    conn: &mut Connection,
    sql: &str,
    from: i64,
    backup: Option<&Path>,
    report: bool,
    stamp: Option<i32>,
    finish: Option<OnCreate<'_>>,
) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    step_in(&tx, sql, from, backup, report, stamp)?;
    if let Some(finish) = finish {
        finish(&tx)?;
    }
    tx.commit()?;
    Ok(())
}

/// One migration step inside the caller's transaction (see [`apply_step`]).
fn step_in(
    tx: &Transaction,
    sql: &str,
    from: i64,
    backup: Option<&Path>,
    report: bool,
    stamp: Option<i32>,
) -> Result<(), StoreError> {
    let to = from + 1;
    let broken_before = broken_references(tx)?;
    tx.execute_batch(sql)?;
    let broken_after = broken_references(tx)?;
    if broken_after > broken_before {
        return Err(StoreError::Invalid(format!(
            "the migration would leave {} more broken reference(s) between rows",
            broken_after - broken_before
        )));
    }
    tx.pragma_update(None, "user_version", to)?;
    if let Some(id) = stamp {
        tx.pragma_update(None, "application_id", id)?;
    }
    if report {
        let at = now_ts();
        let backup = backup.map(|b| b.display().to_string());
        if table_exists(tx, "migration_reports")? {
            tx.execute(
                "INSERT INTO migration_reports (from_version, to_version, backup, applied_at) VALUES (?1, ?2, ?3, ?4)",
                params![from, to, backup, at],
            )?;
        }
        if table_exists(tx, "audit_log")? {
            let outcome = match &backup {
                Some(b) => format!("from version {from} to {to}, backup: {b}"),
                None => format!("from version {from} to {to}"),
            };
            let actor = Actor::System {
                component: "migrations".into(),
            };
            audit::append(
                tx,
                &actor,
                "store.migrated",
                &format!("schema version {to}"),
                &outcome,
                None,
                &at,
            )?;
        }
    }
    Ok(())
}

/// How many foreign key references are broken (`PRAGMA
/// foreign_key_check`). A step is checked against the count before it: it
/// fails only on a reference it broke, so a row an older bug left broken
/// never stops a migration (story 3.3, review P10).
fn broken_references(conn: &Connection) -> Result<i64, StoreError> {
    Ok(
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?,
    )
}

/// Runs `f` with foreign key enforcement off (it cannot change inside a
/// transaction, and a table rebuild needs it off), then puts it back as it
/// was, whatever `f` returned.
fn with_foreign_keys_off<T>(
    conn: &mut Connection,
    f: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let was_on = conn.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))? != 0;
    if was_on {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
    }
    let out = f(conn);
    if was_on {
        conn.pragma_update(None, "foreign_keys", "ON")?;
    }
    out
}

/// What a new database's last creating step also commits (story 2.13: the
/// user store's `recovered` mark), in that step's transaction.
pub(crate) type OnCreate<'a> = &'a dyn Fn(&Transaction) -> Result<(), StoreError>;

fn table_exists(conn: &Connection, name: &str) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |r| r.get(0),
    )?)
}

/// Applies every migration the database lacks; `backups` is the folder for
/// the pre-migration copy.
pub(crate) fn migrate(conn: &mut Connection, backups: &Path) -> Result<OpenReport, StoreError> {
    migrate_set(conn, backups, &PROJECT, None)
}

/// The database's `application_id`.
pub(crate) fn application_id(conn: &Connection) -> Result<i32, StoreError> {
    Ok(conn.pragma_query_value(None, "application_id", |r| r.get(0))?)
}

/// Applies every migration of `set` the database lacks, then stamps the
/// set's `application_id` on a database that has none yet (a new one, or a
/// project store from before the stamp). `on_create` runs in the last
/// step's transaction, and only when the database was new (version 0).
pub(crate) fn migrate_set(
    conn: &mut Connection,
    backups: &Path,
    set: &MigrationSet,
    on_create: Option<OnCreate<'_>>,
) -> Result<OpenReport, StoreError> {
    // A database with no id yet gets it in the transaction of its first
    // step, so no crash leaves a migrated database without it.
    let stamp = (application_id(conn)? == 0).then_some(set.application_id);
    let report = migrate_with(
        conn,
        backups,
        set.migrations,
        set.backup_prefix,
        stamp,
        on_create,
    )?;
    if application_id(conn)? == 0 {
        // Nothing to migrate (a project store from before the stamp).
        conn.pragma_update(None, "application_id", set.application_id)?;
    }
    Ok(report)
}

fn migrate_with(
    conn: &mut Connection,
    backups: &Path,
    migrations: &[&str],
    prefix: &str,
    stamp: Option<i32>,
    on_create: Option<OnCreate<'_>>,
) -> Result<OpenReport, StoreError> {
    with_foreign_keys_off(conn, |conn| {
        migrate_steps(conn, backups, migrations, prefix, stamp, on_create)
    })
}

/// [`migrate_with`]'s steps, run with foreign keys off.
fn migrate_steps(
    conn: &mut Connection,
    backups: &Path,
    migrations: &[&str],
    prefix: &str,
    stamp: Option<i32>,
    on_create: Option<OnCreate<'_>>,
) -> Result<OpenReport, StoreError> {
    let known = migrations.len() as i64;
    let found = user_version(conn)?;
    refuse_unknown(found, known)?;
    let backup = if (1..known).contains(&found) {
        Some(backup_into(conn, backups, found, prefix)?)
    } else {
        None
    };
    let mut applied = Vec::new();
    // A new database with a creating mark (story 2.13: the user store's
    // `recovered`): every step and the mark in one transaction, so no crash
    // leaves a created store unmarked.
    if let (0, Some(finish)) = (found, on_create) {
        let all = (|| -> Result<(), StoreError> {
            let tx = conn.transaction()?;
            for (index, sql) in migrations.iter().enumerate() {
                let id = if index == 0 { stamp } else { None };
                step_in(&tx, sql, index as i64, None, true, id)?;
            }
            finish(&tx)?;
            tx.commit()?;
            Ok(())
        })();
        all.map_err(|e| StoreError::MigrationFailed {
            from: 0,
            to: known,
            backup: None,
            source: Box::new(e),
        })?;
        applied = (0..known)
            .map(|from| AppliedMigration {
                from,
                to: from + 1,
                backup: None,
            })
            .collect();
    }
    // Otherwise each step in its own transaction.
    let done = applied.len();
    for (index, sql) in migrations.iter().enumerate().skip(found as usize + done) {
        let from = index as i64;
        let id = if applied.is_empty() { stamp } else { None };
        apply_step(conn, sql, from, backup.as_deref(), true, id, None).map_err(|e| {
            StoreError::MigrationFailed {
                from,
                to: from + 1,
                backup: backup.clone(),
                source: Box::new(e),
            }
        })?;
        applied.push(AppliedMigration {
            from,
            to: from + 1,
            backup: backup.clone(),
        });
    }
    if backup.is_some() {
        prune_backups(backups, prefix)?;
    }
    Ok(OpenReport {
        version: known,
        created: found == 0,
        applied,
        legacy_kept: None,
    })
}

/// True for a pre-migration copy's name (`<prefix>-<stamp>-v<n>.db`).
fn is_backup_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('-'))
        && name.ends_with(".db")
        && !RuntimeLayout::is_kept_legacy(name)
}

/// Keeps the [`PRE_MIGRATION_KEPT`] newest pre-migration copies in `dir`
/// (names sort by time) and deletes the older ones; nothing else is touched.
fn prune_backups(dir: &Path, prefix: &str) -> Result<(), StoreError> {
    let read = std::fs::read_dir(dir)
        .map_err(|e| StoreError::io(format!("cannot read {}", dir.display()), e))?;
    let mut names: Vec<String> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_backup_name(n, prefix))
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(PRE_MIGRATION_KEPT);
    for name in &names[..excess] {
        let path = dir.join(name);
        std::fs::remove_file(&path)
            .map_err(|e| StoreError::io(format!("cannot remove {}", path.display()), e))?;
    }
    Ok(())
}

/// `VACUUM INTO` a new owner-only file in `dir` (created 0700, with its
/// parent, if missing), named by time so names sort oldest first.
fn backup_into(
    conn: &Connection,
    dir: &Path,
    from: i64,
    prefix: &str,
) -> Result<PathBuf, StoreError> {
    if let Some(parent) = dir.parent() {
        fsx::ensure_private_dir(parent)
            .map_err(|e| StoreError::io(format!("cannot create {}", parent.display()), e))?;
    }
    fsx::ensure_private_dir(dir)
        .map_err(|e| StoreError::io(format!("cannot create {}", dir.display()), e))?;
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%S%.6fZ")
        .to_string();
    // An empty 0600 file first, so the copy is never readable by others;
    // VACUUM INTO accepts an empty target. A name already taken (the same
    // microsecond) gets a sequence number that still sorts after it.
    let mut n = 1;
    let path = loop {
        let name = if n == 1 {
            format!("{prefix}-{stamp}-v{from}.db")
        } else {
            format!("{prefix}-{stamp}~{n}-v{from}.db")
        };
        let path = dir.join(name);
        match fsx::create_private_file(&path) {
            Ok(_) => break path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) => {
                return Err(StoreError::io(
                    format!("cannot create {}", path.display()),
                    e,
                ));
            }
        }
    };
    let target = path
        .to_str()
        .ok_or_else(|| StoreError::Invalid(format!("{} is not UTF-8", path.display())))?;
    if let Err(e) = conn.execute("VACUUM INTO ?1", params![target]) {
        let _ = std::fs::remove_file(&path);
        return Err(e.into());
    }
    Ok(path)
}

/// Builds a database at `version` from the first `version` migrations, with
/// no reports; past [`SCHEMA_VERSION`] it applies all of them and then sets
/// `user_version`. For tests' older- and newer-database fixtures only.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn database_at_version(path: &Path, version: i64) -> Result<(), StoreError> {
    fsx::create_private_file_if_missing(path)
        .map_err(|e| StoreError::io(format!("cannot create {}", path.display()), e))?;
    let mut conn = Connection::open(path)?;
    crate::configure(&conn)?;
    let have = user_version(&conn)?;
    let upto = version.clamp(0, SCHEMA_VERSION);
    with_foreign_keys_off(&mut conn, |conn| {
        for (index, sql) in MIGRATIONS
            .iter()
            .enumerate()
            .take(upto as usize)
            .skip(have.max(0) as usize)
        {
            apply_step(conn, sql, index as i64, None, false, None, None)?;
        }
        Ok(())
    })?;
    if version > SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", version)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every numbered file in `migrations/` is embedded, in order, with no gap.
    #[test]
    fn migration_files_are_numbered_and_all_embedded() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".sql"))
            .collect();
        names.sort();
        assert_eq!(names.len() as i64, SCHEMA_VERSION, "{names:?}");
        for (i, name) in names.iter().enumerate() {
            assert!(name.starts_with(&format!("{:04}_", i + 1)), "{name}");
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            assert_eq!(text, MIGRATIONS[i], "{name} is not embedded at {i}");
        }
    }

    #[test]
    fn report_lines() {
        let layout = RuntimeLayout::at(PathBuf::from("/b/p"));
        let backup = layout.backups_pre_migration().join("b.db");
        let new = OpenReport {
            version: 2,
            created: true,
            applied: vec![],
            legacy_kept: None,
        };
        assert_eq!(new.lines(), vec!["created at version 2"]);
        let old = OpenReport {
            version: 3,
            created: false,
            applied: vec![
                AppliedMigration {
                    from: 1,
                    to: 2,
                    backup: Some(backup.clone()),
                },
                AppliedMigration {
                    from: 2,
                    to: 3,
                    backup: Some(backup.clone()),
                },
            ],
            legacy_kept: None,
        };
        let b = backup.display();
        assert_eq!(
            old.lines(),
            vec![
                format!("migrated from version 1 to 2, backup: {b}"),
                format!("migrated from version 2 to 3, backup: {b}"),
            ]
        );
        let kept = layout.legacy_copy("2026-11-21", 1);
        let moved = OpenReport {
            legacy_kept: Some(kept.clone()),
            ..new
        };
        let lines = moved.lines();
        assert_eq!(lines[0], "created at version 2");
        assert!(lines[1].contains(&kept.display().to_string()), "{lines:?}");
        assert!(
            lines[1].contains("later deletions do not reach"),
            "{lines:?}"
        );
        assert!(lines[1].contains("delete it by hand"), "{lines:?}");
    }

    /// A later step fails: the earlier step stays applied and reported (its
    /// row committed with it), and the error names the backup.
    #[test]
    fn a_failed_later_step_keeps_the_earlier_report_and_names_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        database_at_version(&db, 1).unwrap();
        let mut conn = Connection::open(&db).unwrap();
        let backups = dir.path().join("backups");
        let chain = [MIGRATIONS[0], MIGRATIONS[1], "THIS IS NOT SQL;"];

        let err = migrate_with(&mut conn, &backups, &chain, "state", None, None).unwrap_err();
        let StoreError::MigrationFailed {
            from, to, backup, ..
        } = &err
        else {
            panic!("expected MigrationFailed, got {err:?}");
        };
        assert_eq!((*from, *to), (2, 3));
        let backup = backup.clone().expect("a backup was taken");
        assert!(backup.exists());
        let msg = err.to_string();
        assert!(
            msg.contains(&format!("backup: {}", backup.display())),
            "{msg}"
        );
        assert!(msg.contains("stays at version 2"), "{msg}");

        assert_eq!(user_version(&conn).unwrap(), 2);
        let rows: Vec<(i64, i64, Option<String>)> = conn
            .prepare("SELECT from_version, to_version, backup FROM migration_reports")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![(1, 2, Some(backup.display().to_string()))]);
    }
}
