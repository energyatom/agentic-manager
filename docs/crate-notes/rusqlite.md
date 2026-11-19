# rusqlite 0.43.0

Pinned `=0.43.0`, feature `bundled` (SQLite compiled in). Used only in
`am-store` (AD-6, AD-7).

## What we use

- Writer: `Connection::open`, then `pragma_update` for `journal_mode=WAL`,
  `synchronous=FULL`, `foreign_keys=ON`, and `busy_timeout`. One thread owns
  it; commands arrive over a channel; each runs in `conn.transaction()` with
  its audit row, then `commit()`.
- Readers: `Connection::open_with_flags(path, SQLITE_OPEN_READ_ONLY |
  SQLITE_OPEN_NO_MUTEX)` plus `query_only=ON`.
- Migrations: `pragma_query_value(None, "user_version", ..)`; a database newer
  than the binary is refused. (Story 1.x moves to rusqlite_migration 2.7.0 and
  adds `VACUUM INTO` backups.)
- `OptionalExtension::optional()` for single-row lookups.

## Notes

- Audit append-only is enforced by `BEFORE UPDATE/DELETE` triggers that
  `RAISE(ABORT, ..)`.
- `kill -9` loses nothing committed: WAL frames are fsynced at commit under
  `synchronous=FULL` (criterion 2).
