-- user 0001: this Mac's user store (story 2.12; AD-6, AD-7, AD-17, AD-25).
-- It records the user Accounts file's intents and History and holds its own
-- hash-chained audit log (the Account confirmation records), in the same
-- shapes as a project store's tables, so one executor writes both. Its own
-- `application_id` (set by the migrator) keeps it apart from a project store.
CREATE TABLE migration_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_version INTEGER NOT NULL,
    to_version INTEGER NOT NULL,
    backup TEXT,
    applied_at TEXT NOT NULL
);
CREATE TABLE audit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    outcome TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    entry_hash TEXT NOT NULL,
    args TEXT
);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
CREATE TABLE config_history (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    file TEXT NOT NULL,
    kind TEXT NOT NULL,
    ref_entry TEXT REFERENCES config_history(id),
    before_sha256 TEXT NOT NULL,
    after_sha256 TEXT NOT NULL,
    before_bytes BLOB NOT NULL,
    after_bytes BLOB NOT NULL,
    diff TEXT NOT NULL
);
CREATE TABLE config_intents (
    id TEXT PRIMARY KEY,
    opened_at TEXT NOT NULL,
    actor TEXT NOT NULL,
    file TEXT NOT NULL,
    kind TEXT NOT NULL,
    ref_entry TEXT REFERENCES config_history(id),
    before_sha256 TEXT NOT NULL,
    after_sha256 TEXT NOT NULL,
    before_bytes BLOB NOT NULL,
    after_bytes BLOB NOT NULL,
    diff TEXT NOT NULL,
    audit_notes TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX config_history_by_file ON config_history (file, seq);
CREATE TRIGGER config_history_no_update BEFORE UPDATE ON config_history
BEGIN SELECT RAISE(ABORT, 'config_history is append-only'); END;
CREATE TRIGGER config_history_no_delete BEFORE DELETE ON config_history
BEGIN SELECT RAISE(ABORT, 'config_history is append-only'); END;
