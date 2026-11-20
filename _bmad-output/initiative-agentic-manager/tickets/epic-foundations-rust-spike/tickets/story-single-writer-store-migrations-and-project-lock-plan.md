---
title: 'Single-writer store, migrations and project lock'
type: feature
ticket: '6'
created: '2026-11-20'
status: done
baseline_revision: '63d62cdafd8052e42c48d4a0c57162390bf40f93'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/docs/crate-notes/rusqlite.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `am-store` is still spike code. It opens a bare database path, holds one inline migration, and takes no backup before migrating. Nothing creates or protects a project's `.agentic/` folder, and nothing stops two processes from opening the same project (epic R9 and R4; AD-6, AD-7, AD-24, AD-32).

**Approach:** keep the spike's single writer thread, its read-only readers, its mandatory-Actor `Writer::exec` and the newer-database refusal, and add a project-level open:
- Create `<project>/.agentic/` owner-only and git-ignored, and refuse it when its permissions are loose.
- Take an exclusive lock file that names its holder.
- Apply the embedded, numbered, forward-only migrations after a `VACUUM INTO` backup in `.agentic/backups/`, and report each one.

`agentic serve --project <path>` opens a project this way, prints the report and holds the lock.

**Tests first (owner rule):** the acceptance tests are written first and recorded red.

**Owner decisions (2026-11-20):**
- **(a) Migrator:** keep a small project-owned `user_version` migrator under AD-7's rules. That means numbered embedded SQL files, forward-only, refusing a newer database, backing up first and reporting each migration. `rusqlite_migration` is not added: `=2.7.0` requires `rusqlite ^0.41`, and the Stack pins `=0.43.0`. Implementation Notes record this as a fourth Stack conflict for the architect.
- **(b) Pre-migration backup:** written plain, mode 0600, beside `state.db`, the same exposure as `state.db` itself. Encryption comes with the daily-backup story (R49, AD-32).

## Boundaries & Constraints

**Always:**
- The writer owns the only read-write connection: WAL, `synchronous=FULL`, `foreign_keys=ON`.
- Readers open `SQLITE_OPEN_READ_ONLY` with `query_only=ON`.
- `Writer::exec` takes an `Actor` with no default; `Actor` gets no `Default`.
- Migrations are numbered SQL files under `crates/am-store/migrations/`, embedded with `include_str!`. They are forward-only and gated by `user_version`. A database newer than the binary is refused in plain words, naming both versions and the fix.
- Before any migration of an existing database (version ≥ 1), take a `VACUUM INTO` backup at `.agentic/backups/state-v<from>-<UTC timestamp>.db` (mode 0600). A new database needs no backup.
- Every applied migration is reported: from-version, to-version and backup file. The report is returned by the open and printed by `serve` as one CLI line. A new migration `0002` adds the `migration_reports` table that keeps these records for the later UI banner.
- `.agentic/` is created with mode 0700. Its files (`state.db`, `lock`, backups) are created 0600. `.agentic/.gitignore` holds `*`, so git ignores the folder without the app touching project files.
- If `.agentic/` exists with any group or other permission bit, the open is refused, naming the folder and the fix (`chmod 700`).
- The lock is `.agentic/lock`, held with the OS file lock (`std::fs::File::try_lock`). It records pid, host, command and start time. A second open is refused, naming that holder. The OS releases the lock when its holder dies, so a stale file never blocks.
- Unix permission code compiles and runs on macOS and Linux (CI's Linux job). Nothing here is macOS-only.
- Every existing spike caller (am-engine journal and scheduler tests, am-app `c3`, e2e spike tests) keeps working, still through `Store::open(path)` on a bare path.

**Never:**
- The audit-log hash chain, export or RULE-16 test (story 1.7).
- Config files, History or Undo (1.8).
- Daily or encrypted backups (R49 and AD-32's daily copies).
- A second read-write connection.
- A new crate outside the Stack.
- Writing into the project outside `.agentic/`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|---|---|---|
| New project | empty folder | `.agentic/` 0700 with `.gitignore` `*`; `state.db` 0600 at the latest version; no backup; report "created at version N" |
| Older database | `state.db` at version N−1 (built from the first N−1 migrations, with one row) | `.agentic/backups/state-v<N−1>-<ts>.db` exists (0600) and holds the old data; version N; the row is kept; report and `serve` line "migrated from version N−1 to N, backup: …" |
| Newer database | `user_version` N+1 | refused: "the database is version N+1, newer than this app knows (N); update the app"; nothing written; no backup |
| Second process | a project already open in another process | refused, naming the holder's pid, host, command and start time; exit non-zero; the first process is unaffected |
| Stale lock file | `lock` left behind by a killed process | the open succeeds and rewrites the holder |
| Loose permissions | `.agentic/` mode 0755 | refused, naming the folder and `chmod 700 <path>` |
| No Actor | `writer.exec(cmd)` with no Actor | does not compile (`compile_fail` doctest, run by `just check`) |

</frozen-after-approval>

## Code Map

- **`crates/am-store/src/lib.rs`**:
  - `Store::open(path)` (:192) opens the connection, calls `configure` (:182) and `migrations::migrate`, then spawns the writer thread.
  - `Writer::exec(actor, cmd)` (:168) and `Reader::open` (later in the file) stay as they are.
  - `StoreError::TooNew` (:30) is the plain-words refusal.
  - Add a `project` module with `open_project(root) -> Result<(Store, ProjectLock, OpenReport), StoreError>`. Keep `Store::open(path)` for the spike callers.
- **`crates/am-store/src/migrations.rs`**: one inline SQL string, plus `migrate()`.
  - Move that SQL to `crates/am-store/migrations/0001_initial.sql` and add `0002_migration_reports.sql`.
  - `migrate` returns the applied steps.
  - Expose a test helper (`#[doc(hidden)]` or a test-support feature) that builds a database at a given version, for the older-database fixture.
- **`crates/am-domain/src/actor.rs`**: `Actor` has no `Default`; keep it that way. Add a doc note.
- **`crates/am-app/src/main.rs`**: `serve` (:70) binds and serves with no project. Add `--project <path>`: open the project, print each report line, then hold the lock for the server's life. Without `--project`, behaviour is unchanged.
- **Callers that must keep compiling:**
  - `crates/am-engine/src/journal.rs` (:263, :284) and `scheduler.rs` (:335);
  - `crates/am-app/src/main.rs` `c3` (:190) and `spike.rs` (:90);
  - `crates/am-e2e/tests/spike/c3_scheduler.rs`.
- **`docs/crate-notes/rusqlite.md`**: it says a later story moves to rusqlite_migration; update it to decision (a).
- **`justfile`**: `check` runs nextest only. Add `cargo test --doc -p am-store` so the `compile_fail` doctest runs.

## Tasks & Acceptance

**Execution:**
- [x] `crates/am-store/tests/project.rs` and `crates/am-e2e/tests/project_open.rs` (moved to `crates/am-app/tests/serve_project.rs` after CI; see Implementation Notes) -- FIRST: one test per matrix row. The second-process row runs two `agentic serve --project` processes; the stale-lock row is a child process that holds the lock and is then killed. Add the `compile_fail` doctest on `Writer::exec`. Run them and paste the red output into Implementation Notes.
- [x] `crates/am-store/migrations/*.sql` and `crates/am-store/src/migrations.rs` -- the embedded numbered files, the backup hook and the report.
- [x] `crates/am-store/src/project.rs` and `crates/am-store/src/lib.rs` -- `.agentic/` creation and the permission check, the lock and the project open.
- [x] `crates/am-app/src/main.rs` -- `serve --project`.
- [x] `justfile` and `docs/crate-notes/rusqlite.md` -- the doctest step and the note.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the result matches, with the files and modes checked on disk.
- Given the workspace, when `just check` runs, then it is green on macOS, and the Linux job is green in CI.

## Implementation Notes

**Red run (2026-11-20, before any implementation).** Tests first: `crates/am-store/tests/project.rs` (new project, older database, newer database, second open in-process, drop-then-reopen, stale lock from a re-executed child killed with SIGKILL, loose permissions at 0755/0750/0701), `crates/am-e2e/tests/project_open.rs` (the same rows through the real `agentic serve --project` binary: new project report, older database line, newer database exit, two serve processes, a killed serve's stale lock, loose permissions, and serve without `--project` unchanged), and the doctests on `Writer::exec`. `cargo nextest run -p am-store -p am-e2e -E 'package(am-store) | binary(project_open)' --no-fail-fast` failed to compile both new test binaries, all missing API:

```
error[E0432]: unresolved import `am_store::open_project`
error[E0433]: cannot find `testing` in `am_store`   (x3 in am-store, x3 in am-e2e)
error[E0599]: no variant named `Locked` found for enum `StoreError`   (x2)
error[E0599]: no variant named `LoosePermissions` found for enum `StoreError`
error: could not compile `am-store` (test "project") due to 7 previous errors
error: could not compile `am-e2e` (test "project_open") due to 3 previous errors
```

The no-Actor row already held before this story (`Actor` had no `Default`): `cargo test --doc -p am-store` ran the two `compile_fail` doctests green from the start. A throwaway probe confirmed each fails for the right reason (E0061 for the missing argument, E0277 `Actor: Default` not satisfied); a `no_run` doctest beside them shows the call with an Actor compiles.

**Green run (2026-11-20).**
- `crates/am-store/migrations/0001_initial.sql` (the spike's schema, moved verbatim) and `0002_migration_reports.sql` (`id`, `from_version`, `to_version`, `backup` NULL for a new database, `applied_at`). `migrations.rs` embeds them with `include_str!`; a unit test checks every `NNNN_*.sql` file is embedded in order with no gap.
- `migrate` refuses a newer database, takes the `VACUUM INTO` backup at `<db dir>/backups/state-v<from>-<YYYYMMDDTHHMMSS.mmmZ>.db` (folder 0700, file pre-created empty 0600) when the version is 1..N-1, applies each missing migration in its own transaction, and returns `OpenReport { version, created, applied: [AppliedMigration { from, to, backup }] }`. Each applied step is kept in `migration_reports` with a `store.migrated` audit entry by `Actor::System { component: "migrations" }` in one transaction. `OpenReport::lines()`: "created at version N" for a new database, else one "migrated from version a to b, backup: <path>" per step.
- `Store::open(path)` is unchanged for its callers; it now goes through `open_reporting`, which pre-creates `state.db` 0600 and reads `user_version` (refusing a newer database) before setting WAL or anything else. Backups for a bare path go to `backups/` beside it (for a project that is `.agentic/backups/`).
- `am-store::project` (`open_project(root) -> (Store, ProjectLock, OpenReport)`): `.agentic/` created 0700, `.gitignore` `*` (0600, written only when missing), loose mode refused as `StoreError::LoosePermissions` ("… can be read or entered by other users (mode 755); run `chmod 700 <path>` and open the project again"); `.agentic/lock` 0600 opened without truncation, `File::try_lock` (std, stable since 1.89), then rewritten with `pid`, `host`, `command`, `started`; refused as `StoreError::Locked { lock, holder }` naming them. The lock is released on drop; the file is left in place (removing it would race a process that has it open).
- `#[doc(hidden)] pub mod testing` in am-store: `database_at_version(path, v)` (first `v` migrations; past the latest, all of them and then `user_version = v`) and `insert_fixture_trigger(path, id)`.
- `agentic serve --project <path>`: opens the project before binding, prints each report line as `agentic: project <path>: <line>` (or "database at version N" when nothing was applied), holds store and lock for the server's life; on refusal prints `agentic: cannot open the project <path>: <reason>` to stderr and exits 1. `--project` with no folder is a usage error. Without `--project` nothing changes (tested).
- `justfile`: new `doctest` recipe (`cargo test --doc -p am-store`) in `check`. `docs/crate-notes/rusqlite.md`: decision (a). `Actor` doc note: no `Default`, proved by the doctests.
- `cargo nextest run -p am-store -p am-e2e -E 'package(am-store) | binary(project_open)'`: 24 passed (run four times, stable). `cargo test --doc -p am-store`: 1 compile + 2 compile_fail passed. `just check`: fmt, clippy `-D warnings`, nextest (251 passed, 6 skipped), doctest, check-deps, check-forbid, denylist and validate green on macOS. The Linux CI job has not run yet (nothing pushed).

**Changes outside the listed files (recorded).**
- `crates/am-store/src/fsx.rs` (new): owner-only folder and file helpers, Unix modes behind `#[cfg(unix)]` with a compiling plain branch; `open_project` returns `StoreError::Unsupported` ("not supported on this platform yet") off Unix.
- (Superseded by the CI move below.) `crates/am-e2e/Cargo.toml`: the `project_open` test target and `serde_json` (workspace pin, no new crate) as a dev-dependency. The e2e tests build the binary with `cargo build -p am-app --bin agentic --message-format=json` (the `CARGO` that built the test) and take its path from cargo's artifact report: with toolchain 1.100 the test binaries live under the build dir (`target/debug/build/am-e2e/<hash>/`), so a path derived from the test executable is wrong. The first test in a cold tree spends ~15 s on that build.

**Interpretations (recorded, not decisions).**
- "Nothing written" for a newer database means the database: `state.db`'s bytes are unchanged and no backup is taken. Opening still creates `.agentic/.gitignore` if missing and writes the lock record, both inside `.agentic/`.
- The lock's host comes from `/proc/sys/kernel/hostname` (Linux), else the `hostname` command (macOS), else `$HOSTNAME`, else "an unknown host"; the Stack has no hostname crate and `gethostname` would need `unsafe`. The command is the process's arguments joined by spaces.
- A refusal that races the holder's write of its record waits up to 0.5 s for it, then says the holder could not be read.
- `.agentic` that is a file or a symlink is refused ("not a folder (a file or a link); move it aside"), so the owner-only check cannot be sidestepped through a link.
- A migration of several steps takes one backup (before the first step); each step's report names it.

**Review fixes (2026-11-20, pass 1; same implementer).** These supersede the matching lines above.
- **Report rows per step:** each migration step's `migration_reports` row and `store.migrated` audit entry now commit in that step's own transaction, whenever the table exists once the step's SQL ran. So a new database keeps rows for steps 1→2 onward (step 0→1 runs before the table exists). A failed step returns `StoreError::MigrationFailed { from, to, backup, source }`, whose message says the database stays at `from` and names the backup. New unit test `a_failed_later_step_keeps_the_earlier_report_and_names_the_backup` runs a chain whose third step is not SQL.
- **Negative `user_version`:** refused as `StoreError::NotOurs` ("… not a database this app made; move the file aside …") before anything is written. New test `negative_version_is_refused_as_not_ours_and_nothing_is_written`.
- **Lock record:** the holder record is built before `try_lock`, then truncated and written right after, so a refused opener during a stale-lock takeover can read the dead holder's record only for the length of that write.
- **Lock lifetime:** the lock moves into the writer thread with the read-write connection. `Writer` now shares an `Arc` of the writer thread; when the last `Store` or `Writer` handle drops, the channel closes and the thread is joined. The thread closes the connection, then drops the lock, so the lock is released exactly when the connection closes. `open_project` returns `(Store, OpenReport)`, and `ProjectLock` is crate-private. `dropping_the_store_lets_the_next_open_in` replaces the drop-the-lock test. It checks that a cloned `Writer` still holds the lock after the store drops, and that the next open succeeds once the writer is dropped.
- **`am_store::testing`:** gated behind a new `test-support` feature of am-store. Only am-store's own dev-dependency on itself and am-e2e's dev-dependency turn it on; `cargo tree` shows am-app's normal graph without it. `cargo xtask check-deps` passes (it ignores dev edges).
- **`serve --project`:** a missing value, or one starting with `--`, prints the usage and exits 2. New e2e test `serve_with_a_missing_project_folder_is_a_usage_error`.
- **Test build of `agentic`:** the e2e build of `agentic` uses `--offline --locked`.
- **Command assertion:** the recorded command must equal this test process's own arguments joined as `LockHolder::current` joins them.
- **Bare-path open:** the `Store::open` doc now says AD-7's backup-first rule applies to bare paths too: a migration of an existing database creates `backups/` (0700) beside the given file. No behaviour change.
- **Checks run:** `cargo nextest run -p am-store -p am-e2e -E 'package(am-store) | binary(project_open)'` 27 passed. `cargo test --doc -p am-store` passed. `cargo clippy --workspace --all-targets -D warnings`, fmt and `cargo xtask check-deps` clean. The full `just check` is left to the coordinator.

**CI fix (2026-11-20): the serve tests moved to am-app.** CI on macOS failed `serve_with_a_missing_project_folder_is_a_usage_error`: spawning `agentic` returned `NotFound`. Each test process ran its own `cargo build -p am-app --bin agentic`, and cargo re-links the uplifted `target/debug/agentic` on every build, even a fresh one. So one test's build removed the file at the moment another test spawned it. Linux passed by luck. The file moved from `crates/am-e2e/tests/project_open.rs` to `crates/am-app/tests/serve_project.rs` and now uses `env!("CARGO_BIN_EXE_agentic")`: cargo builds that binary before the integration tests and never rewrites it while they run. Every test and assertion is kept. Removed: the `agentic_bin` cargo-build helper, am-e2e's `project_open` `[[test]]` target, am-e2e's `serde_json` dev-dependency (nothing else used it) and am-e2e's `test-support` feature on am-store. am-app gained dev-dependencies on am-store with `test-support` and on tempfile (workspace pins). `cargo nextest run -p am-app -E 'binary(serve_project)'` passed 8 of 8 on five runs in a row, and `cargo xtask check-deps` passes.

**For the architect (decision (a)).** Fourth Stack conflict: the Stack names rusqlite_migration, but its `=2.7.0` requires `rusqlite ^0.41` while the Stack pins rusqlite `=0.43.0`. am-store keeps a project-owned `user_version` migrator under AD-7's rules (numbered embedded SQL, forward-only, newer database refused, `VACUUM INTO` backup first, each migration reported). AD-7's wording "applied by rusqlite_migration" no longer matches the code.

**Caller re-check after the review fixes (2026-11-20).**
- `just check` is green: 254 passed and 6 skipped, plus the doctest step (1 compile test, 2 `compile_fail`).
- The release `agentic` binary contains no `test-support` symbol.
- Probes with a fresh binary:
  - `serve --project --no-browser` prints the usage line and exits 2.
  - A new project prints "created at version 2".
  - A second `serve` is refused, naming the holder, with exit 1.
  - `.agentic/` at mode 755 is refused, naming `chmod 700`, with exit 1.
- **For the architect:** this is the fourth Stack conflict. `rusqlite_migration =2.7.0` requires `rusqlite ^0.41`, but the Stack pins `=0.43.0`, so a project-owned migrator applies AD-7 (owner decision a).

## Plan Change Log

## Review Triage Log

### Pass 1 (quick lens, 2026-11-20): high 0 · medium 4 · low 5 · false 0 · maybe-false 0

Caller checks before the review:
- `just check` green: 251 passed, plus the doctest step.
- Probes with a fresh binary: a new project prints "created at version 2" with owner-only files, a second `serve` is refused naming its holder, and a version-99 database is refused with no backup.
- Every matrix row has a passing test.

**Test-first exception (recorded, not a defect):** the no-Actor `compile_fail` doctest passed before any implementation. `Actor` already had no `Default` from the spike, so this guarantee predates the story; the doctest now pins it.

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | Each migration step, its `migration_reports` row and its audit entry commit in separate transactions. A crash or a failed `record` after the last step leaves that migration never reported; a failure mid-chain does not name the backup | medium | patch | `apply_one` commits each step; `record()` runs afterwards in its own transaction. AD-7: every applied migration is reported. Fix: write each step's report row (and audit entry, once its table exists) in that step's own transaction, and have a migration error name the backup file. |
| 2 | A negative `user_version` is accepted silently and nothing is applied | medium | patch | `refuse_newer` only checks `found > SCHEMA_VERSION`, and `found as usize` wraps. Fix: refuse `found < 0` in plain words as not this app's database. |
| 3 | During a stale-lock takeover, a refused opener can read the dead holder's complete record and name the wrong process | low | patch | `LockHolder::current()` (which spawns `hostname`) runs after `try_lock` and before `set_len(0)`. Fix: build the holder record before `try_lock`, then truncate and write at once; this narrows the window to a few syscalls. |
| 4 | `ProjectLock` and `Store` are independent: a caller can drop the lock and keep the read-write connection, as can a cloned `Writer` | medium | patch | `open_project` returns both separately, and the writer thread lives while any sender lives. AD-6 and AD-32 tie one read-write connection to the lock. Fix: move the lock into the writer thread so it lives exactly as long as the connection; `open_project` returns `(Store, OpenReport)`. |
| 5 | `pub mod testing` ships a write path with no Actor and no audit in release builds | medium | patch | It is `#[doc(hidden)]` only: `insert_fixture_trigger` opens its own read-write connection. Fix: gate the module behind a `test-support` cargo feature, enabled only by the test crates' dev-dependencies. |
| 6 | `serve --project` with no value exits 1 rather than the usage code 2, and `--project --no-browser` takes the flag as a folder | low | patch | `fail()` exits 1; `Args::value` takes the next token. Fix: a missing value, or one starting with `--`, is a usage error with exit 2. |
| 7 | The e2e helper runs `cargo build` without `--offline --locked` | low | patch | It may reach the registry or rewrite `Cargo.lock`, against "tests never touch the network". Fix: add `--offline --locked`. |
| 8 | The holder-command assertion passes only because the test binary is named `project-<hash>` | low | patch | `holder.command.contains("project")`. Fix: compare with the test process's own joined `std::env::args()`, as `LockHolder::current` builds it. |
| 9 | A bare-path `Store::open` now creates `backups/` beside the given database file when it migrates | low | patch | Spike commands (`--db PATH`) change behaviour. It is AD-7's backup-first rule, so it stays. Fix: document it in the `Store::open` doc comment and the Implementation Notes. |

## Verification

**Commands:**
- `cargo nextest run -p am-store -p am-app -E 'package(am-store) | binary(serve_project)'` -- expected: red first (recorded, under the file's earlier name `am-e2e` `binary(project_open)`), then green.
- `cargo test --doc -p am-store` -- expected: the `compile_fail` doctest passes.
- `just check` -- expected: green.

