-- 0002: every applied migration, kept for the owner's banner (AD-7).
-- `backup` is the pre-migration VACUUM INTO copy; NULL for a new database.
CREATE TABLE migration_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_version INTEGER NOT NULL,
    to_version INTEGER NOT NULL,
    backup TEXT,
    applied_at TEXT NOT NULL
);
