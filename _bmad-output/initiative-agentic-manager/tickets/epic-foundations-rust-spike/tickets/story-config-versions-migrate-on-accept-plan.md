---
title: 'Config versions migrate on accept'
type: feature
ticket: 10
created: '2026-11-21'
status: done
baseline_revision: '6626ce4fc99ca92597b86c7928e4c7c3765efa28'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
warnings: [oversized]
deferred:
  - summary: "ConfigWriter::apply (story 1.8) hashes the file once and renames over it without re-checking, so an editor save between the check and the rename is overwritten and missing from History."
    evidence: "1.10 follow-up review, finding 3; the migrate path now re-checks the sha256 before each write, apply does not."
    location: crates/am-config/src/writer.rs (apply, write_step)
    severity: medium
---

<intent-contract>

## Intent

**Problem:** A config file at an older `schema_version` is just an error today ("unknown schema_version"). R4 and AD-7 want it stepped forward: offered as a diff, written only on the owner's accept through the writer with History, while the database migrates after a backup as before.

**Approach:** am-config gets a migration registry (ordered steps `from -> from+1`, each a pure change on the parsed tree, emitted canonically with `schema_version` bumped). Loading a project with older files offers them as `MigrationOffer`s with a diff and builds no snapshot; `accept_migrations` writes each file through `ConfigWriter` as a new History kind, `Migration`. The shipped registry is empty, because `SCHEMA_VERSION` stays 1. A step from 0 to 1 that exists only with the `test-support` feature proves the path end to end.

## Boundaries & Constraints

**Always:**
- No file is written until `accept_migrations(actor, ...)` is called. Offering is read-only, and a project with a pending migration builds no snapshot, so nothing runs on unaccepted content (AD-16).
- An offer covers every older file in the project. Accept writes them all, or refuses before writing any (re-validating as 1.8 does: refuse only added errors). Each file gets one History entry of kind `Migration` (`migration`, no `ref_entry`), audited as `config.migrated`, through the 1.8 writer's apply order (intent, file write, commit).
- A file older than the first registered step, or with no path to `SCHEMA_VERSION`, stays an error with a worded fix. A newer file is still refused as today (`CompanyRefusal::TooNew` and the checks error).
- The migrated bytes are what the writer's canonical emitter produces for the stepped tree; a seat file's Markdown body is kept byte for byte.
- `ConfigSession::open`/`open_in` returns `SessionError::MigrationPending { offers }` when offers exist, instead of `Invalid`.
- The database side is unchanged: am-store migrates after a `VACUUM INTO` backup and reports it, and the end-to-end test proves both sides on one project.

**Never:**
- No bump of `SCHEMA_VERSION`, no change to shipped `templates/` or `fixtures/` files, and no real migration step (the first real one, `egress_hosts` to `allowed_hosts`, is story 1.14's).
- No CLI or `/api/v1` accept surface and no UI (epic 5). `agentic validate` only words the error for an older file differently.
- No automatic accept, no migration in a `serve` start, and no widening classification of a migration (epic 4).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Offer | Project where two files are `schema_version: 0` (test step registered) | `migration_offers(root)` lists both with from 0, to 1 and a unified diff; disk bytes unchanged | — |
| Session refuses | Same project, `ConfigSession::open` | `MigrationPending { offers }`; no snapshot | — |
| Accept | `accept_migrations(Human, ...)` | Both files are written at schema_version 1; two History entries `migration` audited `config.migrated`; `load` has no errors; session opens | — |
| Body kept | A seat file at v0 with a Markdown body | The body is byte-identical after accept | — |
| Invalid after migrate | The stepped project would add an error | Nothing written, no History | `WriteError::Invalid` naming the findings |
| No path | A file at a version with no step (e.g. -1, or 0 without test-support) | Not offered; still a load error | The error's fix says the app cannot migrate it |
| Newer | A file at `SCHEMA_VERSION + 1` | Not offered | Refused as today (too new) |
| Changed since offer | A file edited on disk between offer and accept | Nothing written | `WriteError::ChangedSince` (the offer carries each file's sha256) |
| Nothing to migrate | Current project | `migration_offers` is empty; `accept_migrations` refuses | `NothingToMigrate` |
| End to end | Runtime database one version back plus config at v0 | `open_runtime` migrates the DB after a backup and reports it; the config offer is shown; after accept, `load` passes and History lists the migration | — |

</intent-contract>

## Code Map

- `crates/am-config/src/checks.rs:8` `SCHEMA_VERSION = 1`; `:25` `schema_version()` emits "unknown schema_version". Word the older case differently: offered when a path exists, else "cannot migrate". `lib.rs:736` `CompanyRefusal::TooNew` stays.
- `crates/am-config/src/writer.rs`: `ConfigWriter` (`apply` :265, `step` :540, which validates with `errors`/`added_errors` and settles pending intents). Add a public `migrate` path that writes given bytes through `step` with `HistoryKind::Migration`, checking the expected before-sha256. `WriteError` (:97). `canon.rs` is the canonical emitter; `node.rs` is the parsed tree that steps change.
- New `crates/am-config/src/migrate.rs`: the registry (an empty `const` slice in normal builds; `#[cfg(feature = "test-support")]` adds the 0→1 step, which only bumps the version), `MigrationOffer { file, from, to, before_sha256, diff, after }`, `migration_offers(root)` and `accept_migrations(writer, actor, offers)`. Export these from `lib.rs`.
- `crates/am-config/src/session.rs`: `open_in` gains the `MigrationPending` branch before the `Invalid` one.
- `crates/am-domain/src/config_history.rs`: `HistoryKind::Migration` (`"migration"`).
- `crates/am-store/src/config_history.rs`: the kind checks (as `OutsideEdit`: no ref), the completion action `config.migrated`, and the docs listing the audits (module doc, `lib.rs` near :206).
- `crates/am-store/src/lib.rs:775`: `testing::database_at_version(path, v)` builds a database one version back for the end-to-end test.
- Tests: `crates/am-config/tests/migrate.rs` (new; the matrix rows, `tests/common/mod.rs` `copy_of`), the `Migration` rows in `crates/am-store/tests/config_history.rs`, and `crates/am-e2e/tests/config_migrate.rs` (new, with a `[[test]]` entry; the end-to-end row on a real store and `open_runtime`, as `config_reload.rs` does).
- Don't touch: `templates/`, `fixtures/projects/*` (tests write their v0 copies into temp folders), am-app and am-server.

## Tasks & Acceptance

**Execution:**
- [x] Tests first, outside any `src/`: every matrix row plus the store rows; run them and log the red run under Implementation Notes.
- [x] `crates/am-domain`, `crates/am-store`: the `Migration` kind and its audit.
- [x] `crates/am-config/src/{migrate.rs,writer.rs,checks.rs,session.rs,lib.rs}`: the registry, offers, accept, wording and the session branch.

**Acceptance Criteria:**
- Given a project with the runtime database one version back and config one `schema_version` back (test step), when it opens, then the database migrates after a backup with the migration reported, the config migration is offered as a diff, and no config file changes until accept; after accept, `load` has no errors and History records one `migration` entry per file.
- Given a normal build, when the shipped templates and fixtures load, then nothing is offered and every file stays at schema_version 1.

## Implementation Notes

**Red run (2026-11-21, before any `src/` change).** Tests written from the matrix, outside every `src/`:
- `crates/am-config/tests/migrate.rs` (new): `migrate_offer_lists_every_older_file_with_its_diff_and_writes_nothing`, `migrate_session_open_refuses_with_migration_pending`, `migrate_accept_writes_every_file_with_one_migration_entry_each`, `migrate_keeps_a_seat_body_byte_for_byte`, `migrate_accept_that_adds_an_error_writes_nothing`, `migrate_file_with_no_path_is_not_offered_and_says_it_cannot_be_migrated`, `migrate_older_file_with_a_path_is_a_load_error_worded_as_offered`, `migrate_newer_file_is_not_offered_and_still_refused`, `migrate_accept_refuses_a_file_changed_since_the_offer_and_writes_nothing`, `migrate_current_project_offers_nothing_and_accept_refuses`, `migrate_shipped_templates_and_fixtures_offer_nothing` (acceptance 2).
- `crates/am-store/tests/config_history.rs` (appended): `config_history_migration_takes_no_ref_and_is_audited_as_config_migrated` (kind `migration`, no ref, `config.migrated`, reconcile keeps the kind).
- `crates/am-e2e/tests/config_migrate.rs` (new, `[[test]] config_migrate` in `crates/am-e2e/Cargo.toml`; am-e2e's am-store dev-dependency gains `test-support` for `testing::database_at_version`): `config_migrate_database_and_config_one_version_back_on_a_real_store` (acceptance 1, the end-to-end row).
- API the tests fix (test-structure choice under the standing rules): `MigrationOffer` with public fields `file, from, to, before_sha256, diff, after` (`Debug + Clone`); `migration_offers(&Path) -> Result<Vec<MigrationOffer>, LoadError>`, sorted by file; `accept_migrations(&ConfigWriter, Actor, Vec<MigrationOffer>) -> Result<Vec<HistoryEntry>, WriteError>`, one entry per offer in order; `WriteError::NothingToMigrate`; `WriteError::ChangedSince { file, .. }` names the changed file; `SessionError::MigrationPending { offers }`; `HistoryKind::Migration`. The older-file load error's fix says "migrat…" when offered and "cannot migrate" with no path; a newer file still says "update the app".
- "Invalid after migrate": the bump-only test step cannot add an error by itself, so the test changes the offer's stepped bytes (`after`) to name an unknown team; accept must re-validate the bytes it writes and refuse with `Invalid`, writing nothing. "Changed since offer" edits the second file, so a per-file write would already have written the first.

Commands and output (each test target fails to compile on the missing API; `cargo nextest run -p am-config -p am-store -p am-e2e -E 'test(migrat) | test(config_history)'` stops at the first, am-store's):

```
== cargo nextest run -p am-config --test migrate
error[E0432]: unresolved imports `am_config::MigrationOffer`, `am_config::accept_migrations`, `am_config::migration_offers`
  --> crates/am-config/tests/migrate.rs:29:63
error[E0599]: no variant named `MigrationPending` found for enum `SessionError`
   --> crates/am-config/tests/migrate.rs:179:23
error[E0599]: no variant, associated function, or constant named `Migration` found for enum `HistoryKind` in the current scope
   --> crates/am-config/tests/migrate.rs:210:45
error[E0599]: no variant named `NothingToMigrate` found for enum `WriteError`
   --> crates/am-config/tests/migrate.rs:417:35
error: could not compile `am-config` (test "migrate") due to 4 previous errors
== cargo nextest run -p am-store --test config_history
error[E0599]: no variant, associated function, or constant named `Migration` found for enum `HistoryKind` in the current scope
   --> crates/am-store/tests/config_history.rs:439:29
(… the same error at :442, :445, :450, :454, :484, :496, :500)
error: could not compile `am-store` (test "config_history") due to 8 previous errors
== cargo nextest run -p am-e2e --test config_migrate
error[E0432]: unresolved imports `am_config::accept_migrations`, `am_config::migration_offers`
  --> crates/am-e2e/tests/config_migrate.rs:19:71
error[E0599]: no variant named `MigrationPending` found for enum `SessionError`
   --> crates/am-e2e/tests/config_migrate.rs:149:27
error[E0599]: no variant, associated function, or constant named `Migration` found for enum `HistoryKind` in the current scope
   --> crates/am-e2e/tests/config_migrate.rs:169:45
error: could not compile `am-e2e` (test "config_migrate") due to 4 previous errors
```

**Green (2026-11-21).** `cargo nextest run -p am-config -p am-store -p am-e2e -E 'test(migrat) | test(config_history)'`: 31 passed. `just check`: green (nextest 436 passed, 7 skipped; doctests, check-deps, check-forbid, denylist, runtime-paths, validate).

Choices taken under the standing rules (internal layout, wording):
- One test fix, no assertion changed: `migrate_accept_writes_every_file_with_one_migration_entry_each` called the helper `offers(root)` while a local `offers` shadowed it (E0618, hidden in the red run by the earlier unresolved imports). It now calls `migration_offers(root).unwrap()`, which is what the helper does.
- `ConfigWriter::migrate` is `pub(crate)`; the public path is `accept_migrations` (one public way in).
- Accept checks the whole set before any write: no empty set (`NothingToMigrate`), no duplicate file, each offer's own fields (`to` is this app's version, a path exists from `from`, `after` parses, carries `to` and is the canonical emit of itself), each file still holds `before_sha256` (`ChangedSince` with an empty `entry`, worded for a migration) and no secret-looking value, the offers cover every older file on disk (else `BadEdit`), and the migrated project as a whole adds no error (`Invalid`). Each file is then written through the 1.8 intent, write and commit sequence (`write_step`, split out of `step`) without a per-file re-check, so a later real step whose files depend on each other is never refused half-way by a transient cross-file error. An I/O failure mid-way leaves the files already written with their entries; an intent left by a crash is settled by `reconcile` as before.
- SKILL.md frontmatter carries no `schema_version` (Agent Skills format), so skill files are never offered.
- The session computes offers from the bytes it scanned (the same bytes it would load), before the exact-map load.
- Wording: an older file with a path is "schema_version N is older than this app's schema_version 1; the file waits for its migration", fix "review and accept the migration the app offers …"; with no path the fix says "this app cannot migrate a file from schema_version N …"; a newer file keeps "unknown schema_version N …; … update the app".

**Review fix red run (2026-11-21, before the src change).** `cargo nextest run -p am-config --test migrate`: 11 passed, 2 failed.
- Owner-only accept: `migrate_accept_by_an_actor_other_than_the_owner_is_refused_and_writes_nothing` panicked at `migrate.rs:449`: "only the owner accepts a migration: [HistoryEntry { … actor: System { component: \"config-writer\" } … kind: Migration …" (the System accept was applied).
- No offer for a file holding a secret: `migrate_file_holding_a_secret_is_not_offered_and_the_value_is_never_shown` panicked at `migrate.rs:474`: "assertion `left == right` failed: the seat holding a key is not offered; left: [\"seats/alpha.md\", \"workflows/digest.yaml\"]".

**Review fixes green (2026-11-21).** `cargo nextest run -p am-config`: 174 passed; `cargo clippy -p am-config --all-targets -D warnings` and `--lib` (no `test-support`) clean. Accept refuses any Actor but `Human` first (`WriteError::NotOwner`); `offer_for` offers no file whose bytes hold a secret-looking value (`secret_lines`), and the loader rewords that file's schema_version fix to "remove the value by hand first, then the app offers the migration"; the `test-support` comment in `Cargo.toml` names the 0→1 step.

**Review fix held (tamper check).** Not done: re-deriving the stepped bytes and refusing a mismatch makes `migrate_accept_that_adds_an_error_writes_nothing` (red-run row "Invalid after migrate") fail, and no tamper-free way to reach `Invalid` exists with the version-only step. The loader checks every field whatever the `schema_version` (`kinds::top` reports the version error and goes on), so a v0 seat naming an unknown team already has that error before the step; added errors are compared without lines, and no finding's words depend on the version or on line numbers, so a bump-only step re-emitted canonically adds no error. Waiting for the coordinator's decision.

**Review fix red run, tamper check (2026-11-21, before the src change; coordinator chose option (b)).** `cargo nextest run -p am-config --test migrate -E 'test(adds_an_error) | test(tampered)'`: 2 failed.
- `migrate_accept_that_adds_an_error_writes_nothing` (reworked: a v0 seat's v0-only `group: no-such-team`, offer not tampered) panicked at `migrate.rs:289`: the stepped seat holds no `team: no-such-team` (the step only bumps the version).
- `migrate_accept_refuses_a_tampered_offer_and_writes_nothing` (new) panicked at `migrate.rs:335`: "called `Result::unwrap_err()` on an `Ok` value: [HistoryEntry { … file: \"seats/alpha.md\", kind: Migration …" (the tampered bytes were written).

**Review fix green, tamper check (2026-11-21).** The `test-support` step now renames a seat's v0-only `group:` key to `team:` (refused when the seat has both) and bumps the version; other files get only the bump. `writer::migrate` re-derives each offer from the file's current bytes (`migrate::check_offer`, after the changed-since and secret checks) and refuses unless `from`, `to` and `after` match exactly ("the offer does not match the app's migration of this file"); this replaces the earlier own-fields check. `cargo nextest run -p am-config`: 175 passed; clippy `--all-targets` and `--lib` clean. The "Review fix held" note above is resolved by this.

**Follow-up review red run (2026-11-21, before the src change).** `cargo nextest run -p am-config --test migrate`: 19 run, 15 passed, 4 failed.
- Item 1 (half-migrated accept): `migrate_accept_refuses_when_an_older_file_is_not_offered_and_writes_nothing` panicked at `migrate.rs:600`: "called `Result::unwrap_err()` on an `Ok` value: [HistoryEntry { … file: \"workflows/digest.yaml\", kind: Migration …" (the workflow was written, the secret seat left at v0).
- Item 2 (step failure wording): `migrate_file_whose_step_fails_states_the_reason_and_an_empty_accept_names_it` panicked at `migrate.rs:631`: the seat's fix is "review and accept the migration the app offers …", without the step's reason.
- Item 3 (edit during accept): `migrate_accept_stops_at_a_file_edited_during_the_accept` panicked at `migrate.rs:712`: "called `Result::unwrap_err()` on an `Ok` value" (the workflow edited after the seat's commit was overwritten). Seam: a test `ConfigHistory` wrapper that edits the workflow on disk when the seat's entry is committed; no src hook needed.
- Item 4 (diff not compared): `migrate_accept_refuses_an_offer_whose_diff_was_altered` panicked at `migrate.rs:369`: "called `Result::unwrap_err()` on an `Ok` value".
- Item 5: `migrate_accept_renames_a_v0_group_to_team` passes already (it covers the step added in the previous fix; a regression test, not a behaviour change). Item 6 is a comment.

**Follow-up review green (2026-11-21).** `migrate::assess` classifies each file (not older, no path, blocked by a secret or a step's reason, offer); `offer_for`, the loader's rewording (`word_blocked`) and the accept guard all use it. Accept refuses before any write when an older file with a step path is not among the offers, naming each file and why (`left_behind`); an empty accept is `NothingToMigrate` only when no file is older (else the same refusal, plus files with no path). The fix of a file a step refuses gives the step's reason and says to fix it by hand. Before each file's write the migrate path re-reads the file and stops with `ChangedSince` when it changed (no intent opened; earlier files keep their entries; the wording says so). `check_offer` compares `diff` too. `cargo nextest run -p am-config`: 180 passed; clippy `--all-targets` and `--lib` clean; `cargo nextest run -p am-config -p am-store -p am-e2e -E 'test(migrat) | test(config_history)'`: 39 passed; `just check`: green (nextest 444 passed, 7 skipped).

**Narrow review red run (2026-11-21, before the src change).** `cargo nextest run -p am-config --test migrate --no-fail-fast`: 25 run, 20 passed, 5 failed.
- Item 1a: `migrate_linked_seat_in_a_folder_is_not_offered_and_the_accept_agrees` panicked at `migrate.rs:790`: "the linked seat is not offered; left: [\"seats/alpha.md\", \"seats/gamma.md\", \"workflows/digest.yaml\"]".
- Item 1b: `migrate_linked_company_yaml_is_not_offered_and_an_accept_names_it` panicked at `migrate.rs:820`: "the linked company.yaml is not offered; left: [\"company.yaml\", \"workflows/digest.yaml\"]".
- Item 2 (pre-write): `migrate_accept_refuses_an_edit_made_after_the_offer_and_migrates_nothing` panicked at `migrate.rs:863`: the refusal says "(a file migrated before it keeps its History entry)".
- Item 2 (mid-way, renamed from `migrate_accept_stops_at_a_file_edited_during_the_accept`; its `ChangedSince` match is now a Debug check on the cause, plus the new wording): `migrate_accept_stops_mid_way_at_a_file_edited_during_the_accept` panicked at `migrate.rs:721`: the refusal has no "stopped at workflows/digest.yaml".
- Item 3: `migrate_session_names_a_blocked_file_instead_of_asking_to_accept` panicked at `migrate.rs:888`: "… review each diff and accept the migration before the project opens".
- Item 4: `migrate_accept_of_only_some_offers_is_refused_naming_the_rest` and `migrate_accept_of_the_same_offer_twice_is_refused` pass from the start (the guards already refuse; regression tests).
- No deterministic seam exists for an edit made *during* the whole-project validation (no `ConfigHistory` call runs between the first read and the writes), so the post-validation re-check of every file is covered by the pre-write test above (same check, same refusal) and by review, not by a test that edits mid-validation.

**Narrow review green (2026-11-21).**
- Links (item 1): migration lists files by the loader's own rule (`migrate::migration_files`, the session's `scan`: a link inside an entity folder is skipped; top-level and model files follow links), for `migration_offers`, the writer's accept guard and the session. `assess` blocks an older file whose path holds a link (`Blocked::Link`): not offered, its finding says "this file is a link; the app does not write through links. Replace it with the file itself, then the migration … is offered", and an accept is refused naming it (never `BadPath` for offered files).
- Refuse before any write (item 2): after the whole-project validation every offered file is re-read and compared once more (`ConfigWriter::unchanged`), so an edit made meanwhile refuses with `ChangedSince` and nothing written; its words no longer mention migrated files. The per-file re-check before each write stays; any stop after a file was written (changed file, I/O or History error) is `WriteError::MigrationStopped { file, migrated, cause }`: "stopped at <file>: <cause>; already migrated: <files> (each has its History entry); …". A stop before the first write stays the bare cause.
- Session (item 3): `SessionError::MigrationPending` gains `blocked` (each blocked file with what to do); with any blocked file it reads "the project waits for a config migration that cannot be accepted yet, because: …" and never "accept the migration". The three exact-field patterns on `MigrationPending { offers }` in `tests/migrate.rs` and `am-e2e/tests/config_migrate.rs` gained `..` (syntax only; no assertion changed).
- Commands: `cargo nextest run -p am-config`: 186 passed; `cargo clippy -p am-config --all-targets -D warnings` and `--lib` (no `test-support`) clean; `cargo nextest run -p am-config -p am-store -p am-e2e -E 'test(migrat) | test(config_history)'`: 45 passed; `just check`: green (nextest 450 passed, 7 skipped).

**Last review round red run (2026-11-22, before the src change).** `cargo nextest run -p am-config --test migrate --no-fail-fast`: 29 run, 25 passed, 4 failed.
- Item 1 (first file): `migrate_failure_after_the_first_rename_counts_that_file_as_migrated` panicked at `migrate.rs:1008`: the refusal is the raw "the write stopped: simulated crash after the rename" (the seat already holds its migrated bytes).
- Item 1 (second file; a test `ConfigHistory` sets the existing `AfterRename` failpoint once the seat's entry is committed): `migrate_failure_after_the_second_rename_counts_both_files_as_migrated` panicked at `migrate.rs:1057`: "stopped at workflows/digest.yaml: …; already migrated: seats/alpha.md …" (the workflow is not counted as migrated).
- Item 2: `migrate_seat_under_a_linked_folder_names_the_folder` panicked at `migrate.rs:1114`: the fix says "this file is a link; … Replace it with the file itself".
- Item 4: `migrate_session_names_files_that_cannot_be_migrated` panicked at `migrate.rs:1149`: the session's words do not name `tables/items.yaml (schema_version -1)`.
- Item 3, no seam added: the existing failpoints (`BeforeRename`, `AfterRename`) simulate a crash inside one file's replace and stop the write; a seam that edits a file between the whole-project validation and the first write would be a new kind of test hook (a callback that changes disk), not a crash point, and nothing on that path calls the `ConfigHistory` port that the other tests wrap. So the post-validation re-check keeps no dedicated test; it runs the same `unchanged` check and refusal that `migrate_accept_refuses_an_edit_made_after_the_offer_and_migrates_nothing` covers.

**Last review round green (2026-11-22).**
- Failure after rename (item 1): when `write_step` fails, the migrate path reads the file back; if it holds the migrated bytes (a failure after the rename: folder sync, the `AfterRename` crash point, the History commit) the file counts as migrated with its intent pending. `WriteError::MigrationStopped` gains `written` and is used for this on the first file too: "stopped at <file>: <cause>; already migrated: <files> (<file>'s History entry is recorded at the next start …)". A stop that wrote nothing stays the bare cause. Both tests also check that `reconcile` then records the `migration` entry.
- Linked folder (item 2): `Blocked::Link` carries the linked component; a linked folder is named "seats/ is a link …; Replace the link with the folder itself", in the finding and the refusal.
- Re-check seam (item 3): none added (reason in the red-run note above).
- Session (item 4): `SessionError::MigrationPending` gains `unmigratable` (`<file> (schema_version <n>)`), named as "these files cannot be migrated by the app and stay errors after the accept (rewrite them by hand): …".
- Commands: `cargo nextest run -p am-config`: 190 passed; `cargo clippy -p am-config --all-targets -D warnings` and `--lib` (no `test-support`) clean; story filter (`-p am-config -p am-store -p am-e2e -E 'test(migrat) | test(config_history)'`): 49 passed; `just check`: green (nextest 454 passed, 7 skipped).

## Plan Change Log

- 2026-11-21 — Code Map/Design Notes "version-only step" replaced by "renames a v0-only reference key, then bumps the version", because a version-only step cannot add an error without tampering, and the review's re-derive check (high) refuses tampering; intent-contract unchanged; KEEP everything else.

## Review Triage Log

### 2026-11-21 — Review pass
- verdicts: 7 findings — high 1, medium 2, low 4, false 0, maybe-false 0
- findings:
  - `[medium]` `[patch]` `accept_migrations` takes any `Actor`; AD-7 says a migration is written only on the owner's accept. Verified: no `Actor::Human` match on the path. Fix: any other actor is refused in words before anything is checked or written, with a test.
  - `[high]` `[patch]` Accept writes whatever canonical, error-free bytes an offer carries (public fields) without re-deriving the registered step, so any `schema_version: 1` content can be written and recorded as a `migration`, bypassing the edit path. Widening classification of migrations is epic 4's, which assumes a migration is only the registered step. Fix: re-derive the step from the current bytes and refuse a mismatch; test for a tampered offer; the added-error test reworked to need no tampering.
  - `[medium]` `[patch]` Offers carry secret-looking values in `diff` and `after`, and the derived `Debug` prints them. Verified: `offer_for` has no secret check, unlike 1.9's Reload path. Fix: a file holding such a value is not offered, and its finding says to remove the value first; test.
  - `[low]` `[reject]` The session's `scan` skips linked files inside folders while `config_files` follows them, so the two offer lists differ for a linked older seat. A linked config file is already a load error the owner must fix first (seat and skill folders refuse links), so it is rare; a fix unifies two listings.
  - `[low]` `[reject]` The "waits for its migration" wording is chosen by `has_path` alone, while `offer_for` returns `None` when a step fails. Unreachable today: the only step is the test-only version bump, which cannot fail. Noted for 1.14, whose real step can fail and must carry its reason into the finding.
  - `[low]` `[reject]` No test compiles the shipped empty registry, because dev-dependencies enable `test-support`. Feature unification makes it untestable in this workspace's test builds. The library is built without the feature (`cargo clippy -p am-config --lib` in the implementation run), and the shipped files are all version 1 (acceptance 2's test).
  - `[low]` `[patch]` The `test-support` comment in `crates/am-config/Cargo.toml` is stale (it says crash simulation only). Fix: it now says the feature also registers the 0→1 step, so test builds offer v0 files and release builds do not.

### 2026-11-21 — Follow-up review pass (after the four fixes above)
- verdicts: 4 findings plus 2 test gaps — high 1, medium 2, low 3, false 0, maybe-false 0
- findings:
  - `[high]` `[patch]` All-or-nothing guard computes "missing" from `offers_in`, which skips older files that are not offered (secret held, step failed), so accept writes the offered files and leaves those at v0: a half-migrated project, against the contract. Fix: refuse when any file below `SCHEMA_VERSION` that has a path is not among the offers, whatever the reason; test with the secret seat plus a v0 workflow.
  - `[medium]` `[patch]` A file whose step fails is worded "accept the migration the app offers" although no offer exists, the step's reason is dropped (`.ok()?`), and an empty accept says "every config file is at the schema_version this app reads" even when older files exist. Reachable now through the test step (a seat with both `group` and `team`). Supersedes the earlier low reject "noted for 1.14". Fix: the finding carries the step's reason and says to fix it by hand; `NothingToMigrate` only when no file is older; test.
  - `[medium]` `[patch]` Bytes are hashed once at the start; a file saved in an editor during the multi-file accept is overwritten and its edit lost from History. Fix: re-check the file's sha256 just before each file's write and refuse `ChangedSince` for that file and the rest. The same window in 1.8's `apply` is pre-existing (deferred, not this story).
  - `[low]` `[patch]` `check_offer` does not compare `diff`, so a caller could show the owner a diff that is not what accept writes. Fix: compare it too; covered by the tamper test.
  - `[low]` `[patch]` No happy-path test renames `group` to `team`. Fix: one accept test with a v0 seat holding only `group:` asserts the written seat has `team:` and no `group:`.
  - `[low]` `[patch]` The `stepped()` test helper still says "a version bump only". Fix: reword.

### 2026-11-21 — Narrow review pass (the six patches above)
- verdicts: 4 findings — high 1, medium 3, low 0, false 0, maybe-false 0
- findings:
  - `[high]` `[patch]` Linked config files are seen three ways: `config_files` follows links (`is_file`), the loader and session `scan` skip links in folders, `resolve` refuses every link. A v0 link makes the migration impossible to accept (offered, then `BadPath`), and a lone linked v0 seat the loader never reads is still offered. Supersedes the first pass's low reject of the listing mismatch. Fix: the migration listing follows the loader's own rule (a link the loader skips is not a config file here either), and a linked file the loader does read is blocked with a worded reason (replace the link with the file); tests.
  - `[medium]` `[patch]` The per-file sha256 re-check refuses after earlier files are written, against the contract's "refuses before writing any". Fix: re-check every file once more right before the first write, so an edit made during validation refuses with nothing written; keep the per-file re-check as a guard against overwriting an owner's save (protecting the file beats all-or-nothing), treated like the mid-way I/O failure the Implementation Notes already accept, and word every mid-way stop (changed file, I/O, History) with the files already migrated.
  - `[medium]` `[patch]` `MigrationPending` says "accept the migration" when another older file is blocked and every accept will be refused; the pre-write `ChangedSince` says "a file migrated before it keeps its History entry" when nothing was written. Fix: word both for the state they are in.
  - `[medium]` `[patch]` No test leaves an offerable file out of the accept, and none covers "offered twice"; deleting either guard passes every test. Fix: one test each.

### 2026-11-21 — Final narrow review pass (the four patches above)
- verdicts: 4 findings — high 0, medium 1, low 3, false 0, maybe-false 0
- findings:
  - `[medium]` `[patch]` `write_step` can fail after the rename (`Written::Maybe`, or `commit_entry` failing), leaving the file at v1 with a pending intent, yet `MigrationStopped` reports it as not written and a first-file failure returns the raw error as if nothing changed. Fix: a failure after the rename counts the file as migrated, its History entry pending until `reconcile`, and the wording says so in both the first-file and later cases; test with the existing after-rename failpoint.
  - `[low]` `[patch]` A link through a parent folder (a linked `seats/`) is worded "`seats/alpha.md` is a link; replace it with the file itself", which cannot clear it. Fix: name the linked component; test.
  - `[low]` `[patch]` No test reaches the re-check after validation (the edit-before-accept test is caught by the first check). Fix: a test seam between validation and the first write if one fits the existing test-support failpoints; otherwise record why it stays untested.
  - `[low]` `[patch]` `MigrationPending` omits older files with no migration path, so it tells the owner to accept while a file will still be an error after. Fix: name those files too.

## Design Notes

Decided under `docs/owner-standing-rules.md` (the question fits neither list, so the reversible engineering choice is taken and recorded):
- **Why a test-only step:** every shipped file is at schema_version 1, so a project "one schema_version back" cannot exist with a real step yet. Bumping to 2 with no real change would change the format of all 86 shipped files and every owner project for nothing, and that is hard to undo. A step behind `test-support` proves the mechanism with no format change; 1.14 adds the first real step and moves `SCHEMA_VERSION` to 2.
- **No snapshot while a migration is pending:** running on migrated bytes that were never accepted would be a silent apply (AD-16).
- **All or nothing:** a project half on v0 and half on v1 cannot load, so accepting part of the migration helps no one.

## Verification

**Commands:**
- `cargo nextest run -p am-config -p am-store -p am-e2e -E 'test(migrat) | test(config_history)'` -- expected: all pass.
- `just check` -- expected: green.

## Auto Run Result

Status: built (2026-11-22).

**Summary:** am-config has a migration registry of ordered steps (`from -> from+1`). The shipped registry is empty and `SCHEMA_VERSION` stays 1; with `test-support`, a 0→1 step renames a seat's v0-only `group:` to `team:` and bumps the version.
- **Offers:** `migration_offers(root)` lists every older file with its diff, read-only. Files are listed by the loader's own rule.
- **Blocked files:** a file holding a secret-looking value, a link, or a failed step is not offered, and its finding says what to fix by hand.
- **Session:** `ConfigSession::open` returns `MigrationPending { offers, blocked, unmigratable }` and builds no snapshot.
- **Accept:** `accept_migrations(Human, offers)` is refused before any write when:
  - the actor is not Human;
  - the list is empty, or an offer appears twice;
  - a file changed since the offer (checked again after validation);
  - an offer does not re-derive exactly from the file's current bytes (`from`, `to`, `after`, `diff`);
  - an older file is left out;
  - the migrated project adds an error.

  It then writes each file through the 1.8 intent, write and commit order, with one `migration` History entry audited `config.migrated`.
- **Mid-way stops:** a stop after a write (an outside save, I/O, History) never overwrites the owner's save, and `MigrationStopped` names the files already migrated.

**Files changed:**
- `crates/am-domain/src/config_history.rs`: `HistoryKind::Migration`.
- `crates/am-store/src/{config_history.rs,lib.rs}`: no ref, audited `config.migrated`.
- `crates/am-config/src/migrate.rs` (new): registry, `assess`, offers, `check_offer`, listing.
- `crates/am-config/src/writer.rs`: `migrate`, `write_step` split out of `step`, `WriteError::{NothingToMigrate, NotOwner, MigrationStopped}`.
- `crates/am-config/src/checks.rs`: older-version wording (offered, blocked with a reason, or cannot migrate).
- `crates/am-config/src/session.rs`: `MigrationPending`.
- `crates/am-config/src/lib.rs`: exports.
- `crates/am-config/Cargo.toml`: the `test-support` note.
- Tests: `crates/am-config/tests/migrate.rs`, `crates/am-store/tests/config_history.rs`, `crates/am-e2e/tests/config_migrate.rs` (with `crates/am-e2e/Cargo.toml`).

**Review:** four quick passes; the Review Triage Log has every row.
- Pass 1, 7 findings: 4 patched (1 high, 2 medium, 1 low) and 3 low rejected; the listing mismatch and the step-failure wording were later reopened and patched.
- Pass 2, 6 findings: all patched (1 high, 2 medium, 3 low).
- Pass 3, 4 findings: all patched (1 high, 3 medium).
- Pass 4, 4 findings: all patched (1 medium, 3 low). One was a missing test, recorded as not added, with the reason in Implementation Notes.
- One deferred item (frontmatter `deferred`): 1.8's `apply` has the same read-to-rename window that the migrate path now guards.
- Follow-up recommended: false. Pass 4 found no high and its four patches each have their own test, so the review has converged.

**Verification:** `just check` passes (454 tests, 7 skipped). The story filter passes (49). Clippy is clean with and without `test-support`. Every matrix row has a named test that ran and passed, and each review fix had its red run logged first.

**Residual risks:**
- The re-check between whole-project validation and the first write has no test of its own (no seam). The same check and refusal is covered by the edit-before-accept test.
- A crash or I/O failure mid-way can leave a project partly migrated. The files written keep their entries (pending ones are settled by `reconcile`), and the refusal names them. Rolling back the written files is not attempted.
- The first real step (`egress_hosts` → `allowed_hosts`, story 1.14) moves `SCHEMA_VERSION` to 2. It must carry its step errors as worded reasons, which the `assess` path now supports.
- The session is not wired into `serve` yet; the API story shows offers and calls accept.
