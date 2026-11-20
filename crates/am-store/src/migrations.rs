//! Forward-only migrations by `user_version` (AD-7). Owner decision (a),
//! story 1.6: a small project-owned migrator instead of rusqlite_migration
//! (its 2.7.0 needs rusqlite ^0.41; the Stack pins =0.43.0).
//!
//! - Migrations are the numbered SQL files in `crates/am-store/migrations/`,
//!   embedded with `include_str!`, applied in order, each in its own
//!   transaction that also sets `user_version`. They never run backwards.
//! - A database newer than this binary is refused before anything is written.
//! - Before migrating an existing database (version >= 1), a `VACUUM INTO`
//!   copy is written to `backups/state-v<from>-<UTC timestamp>.db` (mode 0600)
//!   beside the database. A new database needs no backup.
//! - Every applied migration is returned in the [`OpenReport`], kept in
//!   `migration_reports` once that table exists, and audited
//!   (`store.migrated`, `args` NULL) once `audit_log` exists, so a new
//!   database's first step is audited too; both in the step's own
//!   transaction. A failed step's error names the backup.

use std::path::{Path, PathBuf};

use am_domain::Actor;
use rusqlite::{Connection, params};

use crate::{StoreError, audit, fsx, now_ts};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_migration_reports.sql"),
    include_str!("../migrations/0003_audit_args.sql"),
];

/// The schema version this binary knows.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// One applied migration: `from` to `to` (`to = from + 1`), with the backup
/// taken before the first of them (none for a new database).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub from: i64,
    pub to: i64,
    pub backup: Option<PathBuf>,
}

/// What opening the database did to its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenReport {
    /// The version the database is at now.
    pub version: i64,
    /// The database was new (version 0) and was created at `version`.
    pub created: bool,
    pub applied: Vec<AppliedMigration>,
}

impl OpenReport {
    /// The owner-facing lines, one per applied migration (one line for a new
    /// database); empty when nothing was applied.
    pub fn lines(&self) -> Vec<String> {
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
/// at any version) commit in the same transaction.
fn apply_step(
    conn: &mut Connection,
    sql: &str,
    from: i64,
    backup: Option<&Path>,
    report: bool,
) -> Result<(), StoreError> {
    let to = from + 1;
    let tx = conn.transaction()?;
    tx.execute_batch(sql)?;
    tx.pragma_update(None, "user_version", to)?;
    if report {
        let at = now_ts();
        let backup = backup.map(|b| b.display().to_string());
        if table_exists(&tx, "migration_reports")? {
            tx.execute(
                "INSERT INTO migration_reports (from_version, to_version, backup, applied_at) VALUES (?1, ?2, ?3, ?4)",
                params![from, to, backup, at],
            )?;
        }
        if table_exists(&tx, "audit_log")? {
            let outcome = match &backup {
                Some(b) => format!("from version {from} to {to}, backup: {b}"),
                None => format!("from version {from} to {to}"),
            };
            let actor = Actor::System {
                component: "migrations".into(),
            };
            audit::append(
                &tx,
                &actor,
                "store.migrated",
                &format!("schema version {to}"),
                &outcome,
                None,
                &at,
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

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
    migrate_with(conn, backups, MIGRATIONS)
}

fn migrate_with(
    conn: &mut Connection,
    backups: &Path,
    migrations: &[&str],
) -> Result<OpenReport, StoreError> {
    let known = migrations.len() as i64;
    let found = user_version(conn)?;
    refuse_unknown(found, known)?;
    let backup = if (1..known).contains(&found) {
        Some(backup_into(conn, backups, found)?)
    } else {
        None
    };
    let mut applied = Vec::new();
    for (index, sql) in migrations.iter().enumerate().skip(found as usize) {
        let from = index as i64;
        apply_step(conn, sql, from, backup.as_deref(), true).map_err(|e| {
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
    Ok(OpenReport {
        version: known,
        created: found == 0,
        applied,
    })
}

/// `VACUUM INTO` a new owner-only file in `dir` (created 0700 if missing).
fn backup_into(conn: &Connection, dir: &Path, from: i64) -> Result<PathBuf, StoreError> {
    fsx::ensure_private_dir(dir)
        .map_err(|e| StoreError::io(format!("cannot create {}", dir.display()), e))?;
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%S%.3fZ")
        .to_string();
    let path = dir.join(format!("state-v{from}-{stamp}.db"));
    // An empty 0600 file first, so the copy is never readable by others;
    // VACUUM INTO accepts an empty target.
    fsx::create_private_file(&path)
        .map_err(|e| StoreError::io(format!("cannot create {}", path.display()), e))?;
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
    for (index, sql) in MIGRATIONS
        .iter()
        .enumerate()
        .take(upto as usize)
        .skip(have.max(0) as usize)
    {
        apply_step(&mut conn, sql, index as i64, None, false)?;
    }
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
        let new = OpenReport {
            version: 2,
            created: true,
            applied: vec![],
        };
        assert_eq!(new.lines(), vec!["created at version 2"]);
        let old = OpenReport {
            version: 3,
            created: false,
            applied: vec![
                AppliedMigration {
                    from: 1,
                    to: 2,
                    backup: Some(PathBuf::from("/p/.agentic/backups/b.db")),
                },
                AppliedMigration {
                    from: 2,
                    to: 3,
                    backup: Some(PathBuf::from("/p/.agentic/backups/b.db")),
                },
            ],
        };
        assert_eq!(
            old.lines(),
            vec![
                "migrated from version 1 to 2, backup: /p/.agentic/backups/b.db",
                "migrated from version 2 to 3, backup: /p/.agentic/backups/b.db",
            ]
        );
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

        let err = migrate_with(&mut conn, &backups, &chain).unwrap_err();
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
