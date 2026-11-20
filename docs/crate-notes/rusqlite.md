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
- Migrations (story 1.6, owner decision (a), 2026-11-20): a small
  project-owned migrator in `am-store/src/migrations.rs`, not
  rusqlite_migration (its 2.7.0 requires `rusqlite ^0.41`; the Stack pins
  `=0.43.0`; recorded as a Stack conflict for the architect). The numbered
  files `crates/am-store/migrations/NNNN_*.sql` are embedded with
  `include_str!` and applied forward-only, each in a transaction that sets
  `user_version` (`pragma_query_value` / `pragma_update`). A database newer
  than the binary is refused before anything is written. Before migrating an
  existing database (version >= 1), `VACUUM INTO ?1` writes
  `backups/state-v<from>-<UTC timestamp>.db` beside it; the target is first
  created empty with mode 0600 (VACUUM INTO accepts an empty file), and the
  copy keeps `user_version`. Each applied migration is returned in
  `OpenReport`; once `migration_reports` exists, each step's report row and
  audit entry commit in that step's own transaction. A failed step's error
  names the backup. A negative `user_version` is refused as not this app's.
- Files: `state.db` is pre-created 0600 before `Connection::open`; SQLite
  gives `-wal` and `-shm` the database file's mode.
- `OptionalExtension::optional()` for single-row lookups.

## Notes

- Audit append-only is enforced by `BEFORE UPDATE/DELETE` triggers that
  `RAISE(ABORT, ..)`.
- `kill -9` loses nothing committed: WAL frames are fsynced at commit under
  `synchronous=FULL` (criterion 2).
