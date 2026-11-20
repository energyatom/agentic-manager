-- 0004: config History (AD-16, R3, story 1.8). A ConfigEdit, Undo or Restore
-- first commits an intent row, then the file is written and renamed, then the
-- intent becomes a History entry in one transaction with its audit entry. At
-- startup an intent whose file holds its new bytes is completed (reconciled)
-- and one whose file does not is dropped. Both tables hold the file's bytes
-- before and after (config holds no secrets: the validator refuses them,
-- AD-35); the audit log holds only ids, the file path, hashes and the kind.
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
    diff TEXT NOT NULL
);
CREATE INDEX config_history_by_file ON config_history (file, seq);
CREATE TRIGGER config_history_no_update BEFORE UPDATE ON config_history
BEGIN SELECT RAISE(ABORT, 'config_history is append-only'); END;
CREATE TRIGGER config_history_no_delete BEFORE DELETE ON config_history
BEGIN SELECT RAISE(ABORT, 'config_history is append-only'); END;
