---
title: 'Hash-chained audit log'
type: feature
ticket: '7'
created: '2026-11-20'
status: built
baseline_revision: '02143c69c173188dc48caa18f998015393c49687'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** the spike already writes a hash-chained `audit_log` in the writer's transaction, with UPDATE and DELETE triggers. Three parts of R10 / AD-25 are still missing:
- entries carry no redacted arguments or content hashes;
- a new database's first migration is not audited;
- the log cannot be exported as JSON Lines, and an export cannot be checked.

The RULE-16 trust test does not exist yet.

**Approach:**
- Add an `args` column holding redacted arguments: ids, content hashes, counts and statuses that the store derives from each command, never content.
- Audit every migration step once `audit_log` exists.
- Add an export to JSON Lines and a chain check over an export that names the first broken entry.
- Add `agentic audit export` and `agentic audit verify` so the owner can reach both.
- Write `crates/am-e2e/tests/trust/rule_16_audit_log.rs` first and record it red.

## Boundaries & Constraints

**Always:**
- The writer is the only path into `audit_log`, in the same transaction as the change it records.
- `args` is built by am-store from each `WriteCommand` and never copies a content field. Step `output` becomes `output_sha256`; the free-text `Audit` command gains `hash: Option<String>` (mirroring `am_domain::AuditEvent::hash`).
- Older entries stay verifiable. Their `args` is NULL and their hash is the spike's formula. The new formula appends `args` only when it is present. Chains written by 1.6 binaries verify unchanged; no chain is ever recomputed.
- The migration adding `args` is `crates/am-store/migrations/0003_*.sql` (forward-only, AD-7). Migration entries keep `args` NULL, so they can be written at any schema version.
- The export holds one JSON object per entry with seq, at, actor, action, target, outcome, args, prev_hash and entry_hash. The verifier recomputes from exactly those fields and needs no database.
- `audit export` reads through a read-only `Reader`. It works while `serve` holds the project lock, and it refuses a database newer than the binary.
- Tests first: the trust test and the store and CLI tests are written and run red before any implementation.

**Never:**
- Row content, prompts, outputs or secrets in any audit field.
- A redaction or leak-scan layer. That is epic 2 (rule_12); here, content is kept out by construction.
- A store-backed `AuditSink` wired into tools. That comes with the stories that wire those tools.
- A UI.
- A new crate outside the Stack.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected |
|---|---|---|
| Raw UPDATE / DELETE | rusqlite read-write connection to a project's `state.db` | both refused with "audit_log is append-only"; rows unchanged |
| Clean export | project after a run, a settle, a trigger and an `Audit` with a hash | `verify` reports OK with the entry count and head hash |
| Altered line | one middle line's `target` or `args` edited | the check names that entry's seq and line number |
| Removed / reordered line | a line deleted, or two lines swapped | the check names the first entry whose `prev_hash` or seq order breaks |
| Not JSON | a line of garbage | the check names that line |
| No content or secret | a step output and a FakeSecretStore canary written as step output | neither canary appears anywhere in the export |
| Migrations audited | new project; older (v N−1) project | one `store.migrated` entry per applied step, the first step included |
| 1.6 chain | database with spike-formula entries, then migrated and written to | the chain verifies across old and new entries |
| CLI | `agentic audit export <project> [--out <file>]`, `agentic audit verify <file>` | export prints count and head hash; verify exits 0 on OK, 1 naming the entry, 2 on usage |

</frozen-after-approval>

## Code Map

**`crates/am-store/src/lib.rs`**
- `WriteCommand` (:104). `audit_fields` (:168) becomes `audit_entry() -> (action, target, Option<Value>)`, with per-variant redacted args, e.g. FinishStep `{status, output_sha256}`, SettleCall `{input_tokens, output_tokens, cost_micro_usd}`.
- `apply` (:356) calls `append_audit` (:525). That function gains `args: Option<&serde_json::Value>`. When `args` is None it inserts without the column.
- `Reader::verify_audit_chain` (:764): rewrite it over a shared `entry_hash()` function.
- Keep `Writer::exec`, `Actor` and the triggers as they are.

**`crates/am-store/src/audit.rs` (new)**
- `entry_hash`, `AuditEntry` (serde), `Reader::export_audit_jsonl(&mut impl Write) -> (count, head)`.
- `verify_jsonl(impl BufRead) -> Result<ChainOk, ChainBreak { line, seq, reason }>`.
- For `actor`, hash the exact stored text. The export emits it as a JSON object, and the verifier re-serializes it through `am_domain::Actor`, which reproduces the stored text. `args` round-trips through `serde_json::Value` (stable both with and without `preserve_order`).

**`crates/am-store/src/migrations.rs`**
- `apply_step` (:102) writes the audit entry only when `migration_reports` exists. Change it to: write the report row when that table exists, and write the audit entry (args None) whenever `audit_log` exists after the step's SQL. That brings step 0→1 into the log.

**`crates/am-store/migrations/0003_audit_args.sql`**
- `ALTER TABLE audit_log ADD COLUMN args TEXT;`

**Callers of `WriteCommand::Audit`** (they need the new `hash` field):
- `crates/am-store/tests/project.rs:117, :292, :313`;
- `lib.rs` test at :994.

Tests that use `SCHEMA_VERSION` adapt to version 3 without edits.

**`crates/am-app/src/main.rs`**
- Dispatch at :45 gets `audit export|verify`.
- Open with `Reader::open(<project>/.agentic/state.db)`. This needs no lock. It must refuse a too-new database, so check `user_version` the way `open_reporting` does.
- Reuse `fail()` and the usage style of `serve --project`, which exits 2.

**`crates/am-e2e/Cargo.toml`**
- One `[[test]]` per `tests/trust/*.rs` file, named after it.
- `rusqlite` dev-dependency (workspace pin; `check-deps` ignores dev edges).
- A test `trust_targets_listed` fails when a file under `tests/trust/` has no target, so the gate stays "by directory".

## Tasks & Acceptance

**Execution:**
- [x] `crates/am-e2e/tests/trust/rule_16_audit_log.rs`, `crates/am-store/tests/audit.rs`, `crates/am-app/tests/audit_cli.rs` -- FIRST: one test per matrix row. Build the 1.6 chain with `testing::database_at_version(2)` plus spike-formula entries. Run them and paste the red output into Implementation Notes. Record which assertions passed before any change: the triggers predate this story.
- [x] `crates/am-store/migrations/0003_audit_args.sql`, `src/lib.rs`, `src/audit.rs`, `src/migrations.rs` -- the args column, the redacted args per command, the shared hash, migration auditing, export and verify.
- [x] `crates/am-app/src/main.rs` -- `audit export` and `audit verify`.
- [x] `crates/am-e2e/Cargo.toml` and the targets-listed test.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the result matches.
- Given the workspace, when `just check` runs, then it is green, and both CI jobs are green on the story branch.

## Implementation Notes

**Red run (2026-11-20, before any implementation).** Tests first: `crates/am-e2e/tests/trust/rule_16_audit_log.rs` (raw UPDATE/DELETE refused with rows unchanged, clean export verifies with count and head, an altered middle `target` and `args` named by line and seq, no content or FakeSecretStore canary in the export or in `audit_log`), `crates/am-store/tests/audit.rs` (the nine-field export shape, redacted args per command, an `Audit` without a hash, altered / removed / swapped / not-JSON lines, empty export, export through a separate `Reader` while the writer holds the project, too-new database refused, migrations audited on a new and a v N−1 project, the 1.6 chain built with `testing::database_at_version(2)` plus entries inserted with the spike formula computed in the test), `crates/am-app/tests/audit_cli.rs` (the real binary: export with `--out` and to stdout, verify OK / altered / not JSON, too-new database, no project database, six usage errors) and `crates/am-e2e/tests/trust_targets.rs`.

- `trust_targets_listed`, run with only its own target in the manifest, failed: `add a [[test]] target … for: ["rule_16_audit_log.rs"]`.
- With the rule_16 target added, `cargo nextest run -p am-e2e -p am-store -p am-app --no-fail-fast -E 'binary(rule_16_audit_log) | binary(audit) | binary(audit_cli) | test(trust_targets_listed)'` failed to compile all three new binaries on missing API:

```
error[E0432]: unresolved import `am_store::verify_jsonl`   (x2)
error[E0559]: variant `WriteCommand::Audit` has no field named `hash`   (x5)
error[E0599]: no method named `export_audit_jsonl` found for struct `Reader`   (x3)
error: could not compile `am-store` (test "audit") due to 6 previous errors
error: could not compile `am-e2e` (test "rule_16_audit_log") due to 3 previous errors
error: could not compile `am-app` (test "audit_cli") due to 1 previous error
```

- Held before this story: the append-only triggers (0001). The existing unit test `readers_cannot_write_and_audit_is_append_only` passed before any change; the rule_16 raw UPDATE/DELETE assertions rest on the same triggers and were not expected to go red on their own.

**Green run (2026-11-20).**
- `migrations/0003_audit_args.sql`: `ALTER TABLE audit_log ADD COLUMN args TEXT;`. `SCHEMA_VERSION` is 3.
- `src/audit.rs` (new, `pub mod audit`; `ChainOk`, `ChainBreak`, `verify_jsonl` re-exported at the crate root): `entry_hash` (spike formula; `args` + NUL appended only when present), `append` (moved from lib.rs; with `args` None it inserts without naming the column), `Reader::export_audit_jsonl(&mut dyn Write) -> (count, head)`, `Reader::verify_audit_chain` rewritten over `entry_hash`, `verify_jsonl(impl BufRead) -> Result<ChainOk { entries, head }, ChainBreak { line, seq: Option<i64>, reason }>`. `ChainBreak` displays as `line L, entry S: reason` (or `line L: reason`).
- `src/lib.rs`: `WriteCommand::Audit` gained `hash: Option<String>`; `audit_fields` became `audit_entry() -> (action, target, Option<Value>)`. Args per command: CreateRun `{origin, steps (count), workflow}`, StartStep `{}`, ReserveCall `{call_seq, rate_snapshot, reserved_micro_usd, step_run_id}`, SettleCall `{cost_micro_usd, input_tokens, output_tokens}`, MarkLost `{}`, FinishStep `{output_sha256, status}`, SettleAndFinishStep (both sets plus `step_run_id`), FinishRun `{status}`, UpsertTrigger `{catch_up, cron, last_fired, next_fire, state, tz}`, RecordFire `{kind, next_fire, scheduled_for}`, Audit `{sha256}` or NULL.
- `src/migrations.rs`: with `report` set, the report row is written once `migration_reports` exists and the `store.migrated` entry (args NULL) once `audit_log` exists, so step 0→1 is audited. `database_at_version` (report off) still writes neither.
- `am-app`: `am_app::cli::audit` (same shape as `cli::validate`, returns the exit code) and the `audit` dispatch in `main.rs`. Export opens `<project>/.agentic/state.db` with `Reader::open` (no lock) and the export itself refuses a too-new database before writing anything.
- `am-e2e/Cargo.toml`: `[[test]]` targets `trust_targets` and `rule_16_audit_log`; dev-dependencies rusqlite, serde_json, sha2 and hex (all workspace pins).
- Callers updated with `hash: None`: `crates/am-store/tests/project.rs` (3) and the lib.rs unit test.
- The four-binary nextest filter: 25 passed. `just check`: fmt, clippy `-D warnings`, nextest (283 passed, 6 skipped), doctest, check-deps, check-forbid, denylist and validate green on macOS. Linux CI not run (nothing pushed).

**Changes beyond the Code Map (recorded).**
- am-e2e dev-dependencies also include serde_json, sha2 and hex (workspace pins, no new crate): the trust test edits export lines and checks that the canaries' sha256 stands in for them.
- The `trust_targets_listed` test lives in its own target, `tests/trust_targets.rs`, so it runs even when a trust file has no target. It reads the manifest as text (no TOML crate), and a second test checks that reader.
- The CLI logic is in `am_app::cli::audit` rather than inline in `main.rs`. Its messages use the `agentic: ` prefix of `fail()`, and usage errors print the usage line and exit 2, like `serve --project`.
- `audit.rs` unit tests: the no-args hash equals the spike formula written out independently; args text has sorted keys; the `ChainBreak` wording.

**Interpretations (recorded, not decisions).**
- `args` is stored as compact JSON with keys sorted recursively, and the verifier re-sorts before hashing. The hash is therefore the same whether or not serde_json's `preserve_order` is on. A key reordering inside `args` in an export is not reported as tampering, because it changes no value.
- (Superseded by review fix 2.) The verifier requires the first line's `prev_hash` to be `genesis` and its `seq` to be 1, and each later `seq` to be the previous one + 1: rows are never deleted and a failed insert rolls back, so a real log is contiguous. It requires exactly the nine fields, with `args` present and possibly null. Deleting the last line cannot be detected from the file alone: compare the count and head hash that `export` printed.
- `audit export` without `--out` writes JSON Lines to stdout and the summary to stderr. With `--out`, it writes a new 0600 temporary file beside the target and renames it over the target only on success (superseded by review fix 4), then prints the summary to stdout. A folder with no `.agentic/state.db` exits 1 and creates nothing. An unreadable verify file exits 1. Exit 2 is for usage only.
- Export of a database older than version 3 (read-only, so not migrated) reads `args` as NULL. A database with no `audit_log` table is refused with "the database has no audit log yet".
- Free-text `target` and `outcome` on `WriteCommand::Audit` remain the caller's responsibility (ids and paths, never content), as `AuditEvent` documents. There is no leak-scan layer (epic 2).

**Review fixes (2026-11-20, pass 1; same implementer).**
1. **NUL bytes.** The hash separates fields with NUL, so moving `args` into `outcome` as `outcome NUL args` with `args` null hashed the same bytes. Now `audit::append` refuses any hashed field holding a NUL (`StoreError::Invalid`, nothing written), and `parse_line` refuses one with a `ChainBreak` naming the line and entry. Tests: `moving_args_into_the_outcome_through_a_nul_is_named`, `the_writer_refuses_a_field_with_a_nul_byte`.
2. **Contiguous seq.** `seq` is not hashed. The first line's `seq` must now be 1 and each later one the previous + 1, so a renumbered entry is named. The break reads "is not the next entry: expected entry N, found entry M; …". Test: `an_edited_middle_seq_is_named`. `a_removed_line_names_the_first_entry_after_it` now expects the word "missing" in place of "prev_hash", since the seq check fires first.
3. **Exact fields.** A top-level key outside the nine export fields is refused. So is an `actor` that is not exactly equal, as a `serde_json::Value`, to the value re-serialized from the parsed `Actor`: an extra key or another UUID spelling. Tests: `an_extra_top_level_field_is_named`, `an_extra_actor_field_is_named`.
4. **`--out` safety.** The export goes to a new owner-only file `.<name>.tmp-<pid>` beside the target (`create_new`, 0600), is synced, and is renamed over the target only on success; the temporary file is removed on any failure. An `--out` path inside `<project>/.agentic/` (compared canonically, by the file or its folder) is refused with exit 2. Tests: `a_refused_export_leaves_an_existing_out_file_unchanged` (too-new database; the earlier file is unchanged and no temporary file is left) and `out_inside_the_project_agentic_folder_is_refused` (`state.db` and a new file in `.agentic/`; `state.db` is unchanged).
5. **No claimed status.** StartStep and MarkLost args are now `{}`. Before, they held a fixed status even when the command changed no row.
6. **Hash check.** `WriteCommand::Audit`'s `hash` must be exactly 64 lowercase hex characters. Anything else is refused with `StoreError::Invalid` before the transaction starts, and nothing is written. Test: `an_audit_hash_that_is_not_sha256_hex_is_refused` (free text, uppercase, 63 and 65 characters).
- Checks run: the story's four binaries plus am-store's unit tests (46 passed), clippy `-D warnings` on am-store, am-app and am-e2e, and `cargo fmt --check`.

## Plan Change Log

## Review Triage Log

### Pass 1 (quick lens, 2026-11-20): high 0 · medium 4 · low 2 · false 0 · maybe-false 0

Coordinator checks before the review:
- The story filter passed 25 of 25, and `just check` was green.
- Every matrix row had a passing test.
- A probe with a fresh binary: `serve --project` created the project at version 3. `audit export --out` ran while serve held the lock, and the file was 0600 with three `store.migrated` entries. `verify` said OK. Editing line 2 was named as "line 2, entry 2", exit 1.

**Test-first exception (recorded, not a defect):** the refusal of raw UPDATE and DELETE passed before any change, because the spike's triggers predate this story.

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | The NUL field separator lets an edit move `args` into `outcome` (or shift any field boundary) with the same hashed bytes, so the altered line verifies OK | medium | patch | `entry_hash` joins with NUL and appends `args` only when present; JSON strings may hold `\u0000`. Fixed: `nul_free` refuses NUL in every hashed field, in `append` (`StoreError::Invalid`) and in `parse_line` (ChainBreak). Test `moving_args_into_the_outcome_through_a_nul_is_named`. |
| 2 | `seq` is not hashed and only has to rise, so a middle line's seq can be renumbered undetected and the break then names the chosen seq | low | patch | AUTOINCREMENT seq, rows never deleted, a failed insert rolls back, so a real log is contiguous. Fixed: the first seq must be 1 and each later one the previous + 1. Test `an_edited_middle_seq_is_named`. |
| 3 | Extra top-level keys, extra `actor` keys and other UUID spellings are dropped on re-serialization, so a misleading unverified field can be added to a line | medium | patch | `Actor` has no `deny_unknown_fields`, and `parse_line` ignored unknown keys. Fixed: only the nine export keys are allowed, and `actor` must equal the re-serialized `Actor` as a Value. Tests `an_extra_top_level_field_is_named`, `an_extra_actor_field_is_named`. |
| 4 | `--out` truncated or created the target before the export ran: a refused export left an empty file or wiped an earlier export, and `--out <project>/.agentic/state.db` zeroed the live database | medium | patch | `create_private` opened with `truncate(true)` before `export_audit_jsonl`. Fixed: an owner-only `create_new` temp file beside the target, synced and renamed only on success, removed on failure; an `--out` inside `.agentic/` exits 2. Tests `a_refused_export_leaves_an_existing_out_file_unchanged`, `out_inside_the_project_agentic_folder_is_refused`. |
| 5 | MarkLost and StartStep args claimed a fixed status (`lost`, `running`) even when the command changed no row | low | patch | MarkLost updates only `WHERE settled = 0`. Fixed: both now have args `{}`. The pre-existing "outcome `ok` when nothing changed" is deferred (`deferred-work.md`). |
| 6 | `WriteCommand::Audit`'s `hash` was copied into `args` unchecked, so a caller could carry content through it | medium | patch | `hash.as_ref().map(\|h\| json!({"sha256": h}))`. Fixed: anything but 64 lowercase hex characters is refused before the transaction. Test `an_audit_hash_that_is_not_sha256_hex_is_refused`. |

After the fixes, the coordinator reran the checks: the story filter passed 33 of 33, and `just check` exited 0.

## Verification

**Commands:**
- `cargo nextest run -p am-e2e -p am-store -p am-app -E 'binary(rule_16_audit_log) | binary(audit) | binary(audit_cli) | test(trust_targets_listed)'` -- expected: red first (recorded), then green.
- `just check` -- expected: green.
