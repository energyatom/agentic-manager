//! Forward-only migrations by `user_version` (AD-7). Story 1.x moves this to
//! rusqlite_migration with `VACUUM INTO` backups; the spike keeps the rule that
//! a database newer than the binary is refused.

use rusqlite::Connection;

use crate::StoreError;

const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    workflow TEXT NOT NULL,
    origin TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE step_runs (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    step_key TEXT NOT NULL,
    seq INTEGER NOT NULL,
    status TEXT NOT NULL,
    output TEXT,
    updated_at TEXT NOT NULL,
    UNIQUE (run_id, step_key)
);
CREATE TABLE model_calls (
    id TEXT PRIMARY KEY,
    step_run_id TEXT NOT NULL REFERENCES step_runs(id),
    call_seq INTEGER NOT NULL,
    status TEXT NOT NULL,
    settled INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER,
    output_tokens INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE (step_run_id, call_seq)
);
CREATE TABLE ledger (
    id TEXT PRIMARY KEY,
    model_call_id TEXT NOT NULL UNIQUE REFERENCES model_calls(id),
    run_id TEXT NOT NULL REFERENCES runs(id),
    kind TEXT NOT NULL,
    reserved_micro_usd INTEGER NOT NULL,
    settled_micro_usd INTEGER,
    state TEXT NOT NULL,
    rate_snapshot TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE triggers (
    id TEXT PRIMARY KEY,
    cron TEXT NOT NULL,
    tz TEXT NOT NULL,
    catch_up TEXT NOT NULL,
    state TEXT NOT NULL,
    next_fire TEXT,
    last_fired TEXT
);
CREATE TABLE fires (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trigger_id TEXT NOT NULL REFERENCES triggers(id),
    scheduled_for TEXT NOT NULL,
    fired_at TEXT NOT NULL,
    kind TEXT NOT NULL
);
CREATE TABLE audit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    outcome TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    entry_hash TEXT NOT NULL
);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
"#];

/// The schema version this binary knows.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let found: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if found > SCHEMA_VERSION {
        return Err(StoreError::TooNew {
            found,
            known: SCHEMA_VERSION,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
