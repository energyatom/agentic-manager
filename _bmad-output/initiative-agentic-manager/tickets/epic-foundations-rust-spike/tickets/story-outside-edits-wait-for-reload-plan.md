---
title: 'Outside edits wait for Reload'
type: feature
ticket: 9
created: '2026-11-21'
status: done
baseline_revision: '2a38f54cff64c3c294b46912bedc1239456304a0'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** A project file changed outside the app (another editor, git) is not noticed, so the owner cannot see it or choose when it applies (R2, AD-16).

**Approach:** am-config gets a `ConfigSession`. It holds the accepted bytes of every config file and the snapshot built only from them, and a `notify` watcher announces each outside change once, with its diff. A Reload command applies all pending changes as one new snapshot and records each file in History as an outside edit; an invalid change is reported with file, line and fix and never applied. The app's own writes go through the session and raise nothing.

## Boundaries & Constraints

**Always:**
- Only am-config reads project files. The snapshot comes only from accepted bytes, never from disk directly, so nothing outside applies until Reload.
- One event per distinct outside content: settle a burst (debounce), then compare the file's sha256 with the accepted bytes, with the last announced hash, and with the writer's own-write fingerprints (`is_own_write`).
- Reload is all-or-nothing over every pending file. The new snapshot is validated as a whole with 1.8's rule: refuse only errors the change adds (`added_errors`). Each file gets one History entry of a new kind, `OutsideEdit` (string `outside_edit`, no `ref_entry`), with the Reload's Actor, through the 1.8 port (`begin_intent` then `commit_entry`; no file write). The commit is audited as `config.outside_edit`.
- A changed or removed `project_id` in `company.yaml` is an IdentityChange: it is announced, and Reload refuses it in words (AD-16, AD-32).
- An event's diff never shows a secret-looking value: when the new bytes hold one, the diff is replaced by a worded note naming the lines, and the change is invalid.
- `notify =8.2.2` (default features), pinned in `[workspace.dependencies]`. `deny.toml` gets one `[[licenses.exceptions]]` entry: CC0-1.0 for `notify@8.2.2` only (AD-3, 2026-11-21).

**Never:**
- No widening check: `is_widening` and the held two-step Reload are epic 4 story 18. Reload stays single-click here.
- No UI, no `/api/v1` route and no WebSocket event. The banner and Show diff are epic 5's, and wiring the session into `serve` waits for the API story. Recorded scope trim.
- No polling fallback, no second watcher, and no change to the 1.8 writer's apply order.
- No silent apply of an outside change, not even a valid one, and no rewriting of the file on disk by Reload.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Outside edit | A seat file rewritten by another process (temp file, then rename) | Exactly one `ChangedOnDisk{file, diff, before/after sha256, findings: []}`; snapshot hash unchanged | — |
| Reload | Pending valid edit; `reload(Human)` | New hash live; History has one `outside_edit` entry with the diff and Actor; audit `config.outside_edit` | — |
| Own write | `session.apply(edit)` through the writer | No event; snapshot moves to the new bytes | — |
| Burst | Three saves of the same content within the debounce window | One event | — |
| Second change before Reload | Edit A, event, edit A again | A second event (new content); Reload applies the latest bytes | — |
| Invalid edit | Seat names an unknown team | Event with findings (file, line, fix); snapshot unchanged | Reload refused: `Invalid{findings}`; nothing written to History |
| Secret pasted | The new bytes hold a key-looking value | Event diff withheld with a worded note naming the lines; invalid | Reload refused; no History |
| Created / deleted file | A new `workflows/x.yaml`; a seat file removed | An event each (before or after empty); Reload applies both | — |
| Identity change | `project_id` edited or removed | Event flagged `identity_change` | Reload refused in words; snapshot unchanged |
| Not config | `.am-tmp`, `.agentic-open`, `notes.txt`, a `.git/` file | No event | — |
| Nothing pending | `reload` | `NothingPending` | No History |

</intent-contract>

## Code Map

- `Cargo.toml` (`[workspace.dependencies]`, exact pins) -- add `notify = "=8.2.2"`. `deny.toml` (header lines 1-4 say "Never add ... `exceptions`") -- add the one exception and reword the header to name it. `xtask/tests/supply_chain.rs:47-77` `deny_toml_allows_exactly_the_nine_licences_and_no_exceptions` -- rename it, and assert the only exception is notify, CC0-1.0, at 8.2.2. `THIRD_PARTY.md:3-5` -- one sentence on the exception. New `docs/crate-notes/notify.md` (version, use, FSEvents on macOS, inotify on Linux, CC0 exception).
- `crates/am-domain/src/config_history.rs:23` `HistoryKind` -- add `OutsideEdit` (`as_str`/`parse` `"outside_edit"`).
- `crates/am-store/src/config_history.rs:108-123` kind checks -- `OutsideEdit` takes no ref, like `Edit`; `:204-206` completion action -- `config.outside_edit`; `kind_of` (`:249`). Migration 0004 has no CHECK on `kind`, so no migration is needed.
- `crates/am-config/src/snapshot.rs:13` `ConfigSnapshot`, built only in `Loader::finish` (`lib.rs:438`); `content_hash` (`:90`). `lib.rs:139` `load_in`, `:147` `load_with` (overrides, else disk). Add a load from an exact file map (no disk reads of config files) so the snapshot never sees unaccepted bytes.
- `crates/am-config/src/writer.rs` -- `ConfigWriter` (`:206`), `is_own_write` (`:448`), `step` (`:540`), private `errors` (`:608`) and `added_errors` (`:820`); `config_files(root)` (`:771`). Reuse; make them `pub(crate)` where the session needs them. `linediff::unified` (`linediff.rs:24`). `canon::kind_of` (`canon.rs:76`) decides which paths are config. `read_company` (`lib.rs:550`) reads `project_id`. The secret check that `step` uses is reused for the event diff.
- New `crates/am-config/src/session.rs` -- `ConfigSession`, `ConfigEvent`, `ReloadError`, and the watcher thread (std threads and channels; am-config has no tokio). Events go to a caller-given `Arc<dyn Fn(ConfigEvent) + Send + Sync>`.
- `crates/am-testkit/src/lib.rs` -- `InMemoryConfigHistory` accepts `OutsideEdit`; `copy_tree` (`:770`) for fixtures.
- Tests: `crates/am-config/tests/session.rs` (new; real notify on a temp copy of `fixtures/projects/valid`, `tests/common/mod.rs` `copy_of`), `crates/am-store/tests/config_history.rs` (`OutsideEdit` rows), `crates/am-e2e/tests/config_reload.rs` (new; real store; `[[test]]` in `crates/am-e2e/Cargo.toml`).
- Don't touch: `xtask/src/deps.rs` (am-config may take external crates), am-app or am-server (no wiring this story).

## Tasks & Acceptance

**Execution:**
- [x] Tests first, outside any `src/`: one test per matrix row in `crates/am-config/tests/session.rs`; `OutsideEdit` store tests; `crates/am-e2e/tests/config_reload.rs` with the ticket's verify on a real store; the renamed supply-chain test. Run them, and log the red run (command and failing output) under Implementation Notes.
- [x] `Cargo.toml`, `deny.toml`, `THIRD_PARTY.md`, `docs/crate-notes/notify.md` -- the pin and the exception.
- [x] `crates/am-domain`, `crates/am-store`, `crates/am-testkit` -- `OutsideEdit`.
- [x] `crates/am-config/src/{snapshot.rs,lib.rs,session.rs,writer.rs}` -- the exact-map load, the session, the watcher and Reload; export them from `lib.rs`.

**Acceptance Criteria:**
- Given a running session on a valid project, when another process saves a seat file, then exactly one event arrives with its diff and `snapshot().hash()` is unchanged until `reload`, after which the new hash is live and History lists one `outside_edit` entry for the file.
- Given `cargo deny check licenses`, when it runs, then it passes with notify linked, and a planted second crate under CC0-1.0 still fails it.
- Given every matrix row, when its test runs, then it passes, having first been seen to fail.

## Implementation Notes

### Red run (2026-11-21, before any `src/` change)

Tests written, outside any `src/` folder:
- `crates/am-config/tests/session.rs` (new) -- one test per matrix row, real notify on a temp copy of `fixtures/projects/valid`, `InMemoryConfigHistory`: `session_outside_edit_raises_exactly_one_event_and_keeps_the_snapshot`, `session_reload_applies_the_pending_edit_and_records_one_outside_edit`, `session_own_write_raises_no_event_and_moves_the_snapshot`, `session_burst_of_saves_raises_one_event`, `session_second_change_before_reload_raises_a_second_event_and_reload_takes_the_latest`, `session_invalid_outside_edit_is_reported_with_findings_and_never_applied`, `session_secret_in_an_outside_edit_withholds_the_diff_and_is_refused`, `session_created_and_deleted_files_raise_an_event_each_and_reload_applies_both`, `session_identity_change_is_flagged_and_reload_refuses_it`, `session_files_that_are_not_config_raise_no_event`, `session_reload_with_nothing_pending_records_nothing`.
- `crates/am-store/tests/config_history.rs` -- `config_history_outside_edit_takes_no_ref_and_is_audited_as_config_outside_edit`, `config_history_outside_edit_records_created_and_deleted_files`.
- `crates/am-e2e/tests/config_reload.rs` (new; `[[test]] config_reload` in `crates/am-e2e/Cargo.toml`) -- `config_reload_outside_edit_waits_for_reload_on_a_real_store` (the ticket's verify on a real store, audit `config.outside_edit`, chain verifies, no content in the log).
- `xtask/tests/supply_chain.rs` -- `deny_toml_allows_exactly_the_nine_licences_and_no_exceptions` rewritten as `deny_toml_allows_the_nine_licences_and_only_the_notify_cc0_exception`; new `notify_licence_exception_is_pinned_at_8_2_2_and_recorded`; new ignored `cc0_crate_other_than_notify_fails_the_licence_policy` on the new fixtures `fixtures/supply-chain/cc0-consumer` and `cc0-probe` (`[lib] path = "lib.rs"`, no `src/`; `cc0-consumer/Cargo.lock` from `cargo generate-lockfile --offline`).

API the tests fix (Design Notes): `ConfigSession::open(root, Arc<dyn ConfigHistory>, Arc<dyn Fn(ConfigEvent) + Send + Sync>)` starts the watcher; `snapshot()`, `apply(actor, ConfigEdit) -> Result<HistoryEntry, WriteError>`, `reload(actor) -> Result<Vec<HistoryEntry>, ReloadError>`; `ReloadError::{NothingPending, Invalid { findings }, IdentityChange}`; the secret note in `diff` contains `secret` and `line <n>` and never `--- ` or the value.

Command (the plan's Verification command):

```
cargo nextest run -p am-config -p am-store -p am-e2e -p xtask -E 'test(session) | test(config_history) | test(config_reload) | test(supply_chain) | test(licence)' --no-fail-fast
```

```
error[E0599]: no variant, associated function, or constant named `OutsideEdit` found for enum `HistoryKind` in the current scope
   --> crates/am-store/tests/config_history.rs:322:28
error: could not compile `am-store` (test "config_history") due to 7 previous errors
```

Cargo stops at the first crate, so every crate's errors with `cargo build --tests --keep-going -p am-config -p am-store -p am-e2e`:

```
error[E0599]: no variant ... named `OutsideEdit` found for enum `HistoryKind`   --> crates/am-store/tests/config_history.rs:322:28 (7 errors)
error[E0432]: unresolved imports `am_config::ConfigEvent`, `am_config::ConfigSession`, `am_config::ReloadError`
  --> crates/am-config/tests/session.rs:34:29
error[E0599]: no variant ... named `OutsideEdit` found for enum `HistoryKind`   --> crates/am-config/tests/session.rs:264:41
error: could not compile `am-config` (test "session") due to 3 previous errors
error[E0432]: unresolved imports `am_config::ConfigEvent`, `am_config::ConfigSession`, `am_config::ReloadError`
  --> crates/am-e2e/tests/config_reload.rs:19:29
error[E0599]: no variant ... named `OutsideEdit` found for enum `HistoryKind`   --> crates/am-e2e/tests/config_reload.rs:179:41
error: could not compile `am-e2e` (test "config_reload") due to 2 previous errors
```

xtask (compiles), `cargo nextest run -p xtask --run-ignored all -E 'test(supply_chain) | test(licence) | test(notify) | test(cc0) | test(gpl)' --no-fail-fast`:

```
FAIL xtask::supply_chain notify_licence_exception_is_pinned_at_8_2_2_and_recorded
  panicked at xtask/tests/supply_chain.rs:150:5: one exact pin with default features in [workspace.dependencies]
  left: []  right: ["notify = \"=8.2.2\""]
FAIL xtask::supply_chain deny_toml_allows_the_nine_licences_and_only_the_notify_cc0_exception
  panicked at xtask/tests/supply_chain.rs:121:5: exactly one licence exception  left: 0  right: 1
PASS xtask::supply_chain gpl_dependency_fails_the_licence_policy_naming_crate_and_licence
PASS xtask::supply_chain cc0_crate_other_than_notify_fails_the_licence_policy
Summary 4 tests run: 2 passed, 2 failed
```

`cc0_crate_other_than_notify_fails_the_licence_policy` passes before the change by design: it is the guard that the notify exception stays narrow (AC 2, "a planted second crate under CC0-1.0 still fails"), so it must stay green after `deny.toml` gains the exception.

### Implementation (2026-11-21)

- `notify = "=8.2.2"` pinned; `deny.toml` has the one `[[licenses.exceptions]]` (CC0-1.0, `notify@8.2.2`) and a reworded header; `THIRD_PARTY.md` and `docs/crate-notes/notify.md` record it. `cargo deny check licenses` ok; `just supply-chain` green (both graphs, cargo-audit, the ignored probes including `cc0_crate_other_than_notify_fails_the_licence_policy`).
- `HistoryKind::OutsideEdit` (`outside_edit`); am-store takes it with no ref and audits its commit as `config.outside_edit`. `InMemoryConfigHistory` needed no change (it checks no kinds).
- am-config: `load_exact` (crate-private) reads every tracked file only from the given map; folders, metadata and untracked paths still come from disk. `skills.rs` now lists through `Loader::listing` (an `Entry` in place of `std::fs::Metadata`) so the exact map also decides which skill files exist. `read_company` is split so `company_head` reads `project_id` from bytes. `secret_lines`, `sha256_hex`, `added_errors` are `pub(crate)`. New `WriteError::ChangedOnDisk`.
- `session.rs`: `ConfigSession` (open, snapshot, apply, undo, restore, reload), `ConfigEvent`, `ReloadError`, `SessionError`; one std worker thread fed by the notify channel; dropping the session drops the watcher and joins the worker.

Decided under "Decide yourself", with reasons:
- **Tracked files include skill `references/`:** the loader reads them into the snapshot hash, so the Always rule (snapshot only from accepted bytes) needs them accepted too; `canon::kind_of` alone would let a reference change apply silently at the next Reload or own write. `tracked(rel)` = `kind_of` or `skills/<id>/references/<name>`.
- **Own writes refused while disk differs from the accepted bytes** (`WriteError::ChangedOnDisk`): the 1.8 writer reads the file from disk, so an edit on top of an unreloaded outside change would apply it silently. The check runs in the session, under its lock; the writer's apply order is unchanged. Undo and Restore go through the same session path.
- **Event findings** are the errors the change adds to the accepted bytes with every other pending change in place (what Reload checks); warnings are not reported. Findings that quote a secret-looking value are reworded.
- **Folder events rescan:** an event on the root, `models/`, an entity folder, `skills/`, `skills/<id>/` or its `references/` (and notify's `need_rescan`, or a watcher error) compares every tracked file under it, since a removed or renamed folder reports only itself.
- **`ReloadError::NothingPending {}`** is a struct variant with no fields, so the tests' `NothingPending { .. }` pattern passes clippy's `unneeded_struct_pattern`.

Test edits after the red run, mechanical only (no assertion changed): `crates/am-config/tests/session.rs` uses `am_domain::runtime::OPEN_MARKER` instead of the literal (the `runtime-paths` check refused it) and an array instead of `vec!` (clippy `useless_vec`).

Verification: the plan's nextest command 26/26 pass; the session and config_reload tests passed 4 more runs in a row; `just check` green; `just supply-chain` green. Not run locally: the Linux graph's build and tests (no Linux target installed); CI's Linux job covers them.

Risks left: if an outside change races an own write between the session's disk check and the writer's read, the post-write snapshot load can fail and the snapshot stays until that change is reloaded. A History commit failing mid-Reload leaves its intents pending for the writer's reconcile at the next start while the snapshot does not switch.

### Review fixes (2026-11-21)

Review fix red run, before any `src/` change. Phase 1 (`cargo nextest run -p am-config -p am-e2e -E 'test(session) | test(config_reload)' --no-fail-fast`): 13 passed, 4 failed:
- `session_outside_save_matching_an_earlier_app_write_is_announced` -- `session.rs:175: no event within 10s` (fingerprints never age out).
- `config_reload_empty_file_created_outside_reloads_without_a_history_entry` -- `config_reload.rs:325: the empty file reloads: History(Store("refused config intent: the file would not change"))`.
- `session_open_refuses_a_link_in_skill_references_as_load_does` -- `session.rs:633: the session opened`.
- `session_undo_of_an_outside_create_is_refused_in_words` -- `session.rs:717` (`words.contains("by hand")`): refused only by accident, `workflows/x.yaml:1: missing schema_version (the file is empty)`.

Phase 2 (`cargo build --tests -p am-config`), tests on new API:
- `session_reverted_outside_edit_is_withdrawn` -- `error[E0599]: no variant named Withdrawn found for enum ConfigEvent` (session.rs:743).
- `session_open_in_loads_with_the_given_options` -- `error[E0599]: no associated function or constant named open_in found for struct ConfigSession` (session.rs:782).

Fixes:
- Own-write fingerprints cover only writes in flight: `ConfigWriter::forget_own_writes(file)` (`pub(crate)`), called by the session's `own_write` for the written file once the write returns, done or not (Undo and Restore look the file up from the entry).
- Reload skips the History intent for a pending file whose before and after bytes are equal (an empty file created or deleted outside) and still applies it to the accepted bytes and the snapshot.
- `Loader::listing` (exact map) keeps every disk entry that is not a regular file (a link, a folder), so the skill folder's link check still runs; a tracked regular file exists only when the map names it.
- Reload's comment on a failed commit now says what happens: the error returns, the snapshot and pending stay, earlier entries stay committed, and the writer's reconcile completes the left intents whose file is a config file holding the new bytes and drops the rest (reference files, deleted files).
- `ConfigSession::open_in(root, opts, history, on_event)`; `open` calls it with the default options.
- `ConfigEvent::Withdrawn { file }`, sent when a pending change is reverted on disk to the accepted bytes.
- `ConfigWriter::undo`/`restore` refuse an `outside_edit` entry that would create or delete its file (Undo of an outside create or delete, Restore of an outside delete), in words that say to do it by hand; other outside edits undo and restore as before.
- am-store docs list `config.outside_edit` among the commit audit actions.

Verification of the fixes: `cargo nextest run -p am-config -p am-e2e -E 'test(session) | test(config_reload) | test(writer)'` 39/39; `cargo nextest run -p am-config` 159/159; am-store `test(config_history)` 7/7; clippy `-D warnings` on am-config, am-store and am-e2e clean; fmt clean.

### Follow-up review fixes (2026-11-21)

Follow-up review fix red run, before the code change:
- `cargo build --tests -p am-config` -- the new unit tests `session::tests::access_events_mark_nothing_dirty` and `content_events_and_rescans_mark_their_scope` fail to compile: `error[E0432]: unresolved import super::dirty_scopes` (session.rs:788). FSEvents never reports opens, so a unit test of the event filter stands in for the Linux loop.
- `cargo nextest run -p am-e2e -E 'test(config_reload_empty)'` -- `config_reload_empty_file_created_outside_is_not_announced_or_applied` (renamed from the first review round's `config_reload_empty_file_created_outside_reloads_without_a_history_entry`, expectation changed): `config_reload.rs:312: an empty new file is not announced: Ok(ChangedOnDisk { file: "skills/brief/references/notes.txt", before_sha256: None, ...`.

Fixes:
- `dirty_scopes(event, roots)` (pure) decides what an event marks: `""` for a rescan notice, nothing for an `EventKind::Access(_)` event, else each path's scope; watcher errors still rescan everything. `docs/crate-notes/notify.md` says why Access events are ignored.
- The first round's Reload skip for equal bytes is removed. An empty file and no file are the same content wherever the session compares disk with the accepted bytes (`examine`, `differing`): an empty file created or deleted outside is never announced or applied, a pending change reverted to empty or absent withdraws, and own writes are not refused over it. Every applied change keeps its `outside_edit` History entry.
- The IdentityChange refusal now says the other pending changes stay waiting and apply at the next Reload once `project_id` is written back (or removed).

Verification: `cargo nextest run -p am-config -p am-e2e -E 'test(session) | test(config_reload) | test(access_events) | test(content_events)'` 21/21; clippy `-D warnings` on am-config and am-e2e clean; fmt clean.

## Plan Change Log

- 2026-11-21 — First run halted `blocked` before planning (owner rule: licences; notify is CC0-1.0). The owner chose "Exception for notify"; Sofia's REPLAN is on main as `2a38f54` (AD-3, epic 1 Licence and Notes, AGENTS.md). The plan is written fresh from that baseline. The earlier Auto Run Result is superseded.

## Review Triage Log

### 2026-11-21 — Review pass
- verdicts: 11 findings — high 1, medium 3, low 7, false 0, maybe-false 0
- findings:
  - `[high]` `[patch]` Own-write fingerprints never age out, so an outside save matching any earlier app write is hidden for good (Undo A→B→A then a stale editor saves B; or a write that renamed then failed): no event, Reload NothingPending, every apply refused ChangedOnDisk. Verified: `examine` returns `None` on `is_own_write` before announcing, and the writer's ring holds 1024 entries with no expiry. Fix: the writer forgets a file's fingerprints when `own_write` returns, so they cover only in-flight writes (still the plan's fingerprint comparison).
  - `[medium]` `[patch]` An empty file created or deleted outside has equal before/after hashes (empty vs absent); am-store refuses the intent, so Reload never succeeds. Verified at am-store `begin` ("the file would not change"). Fix: no History intent when the bytes are equal; the file still enters `accepted` and the snapshot.
  - `[low]` `[reject]` `own_write` moves `accepted` but keeps the old snapshot when `load_exact` fails after a successful write. This needs untracked disk state (exec bit, a `scripts/` folder) to make the project invalid between edits, which is rare. A fix needs a new guard and recovery path.
  - `[low]` `[reject]` Reload lists errors the change did not add when the project already has errors. A snapshot needs zero errors (`Loader::finish`), so Reload cannot apply either way; the words name the errors to fix. Needs the same rare untracked-state errors as above.
  - `[medium]` `[patch]` A symlink under `skills/<id>/references/` is dropped by the exact-map listing, so its "links are not accepted" error is lost and the session opens a project `load` rejects. Verified in `Loader::listing` (`retain` drops non-dir tracked names absent from the map). Fix: keep link entries from the disk listing.
  - `[low]` `[patch]` The reload comment says reconcile completes left-over intents. Reconcile drops them for reference and deleted files. Duplicate History on a retried Reload after a failed commit is rare (a store failure mid-Reload); only the comment is corrected.
  - `[low]` `[patch]` `open` hard-codes `LoadOptions::default()`, so a caller cannot pass the runtime knowledge root (1.13's reference-file warning). Fix: `open_in(root, opts, ...)` on the type this change introduces; `open` keeps its signature.
  - `[low]` `[patch]` A pending change reverted on disk leaves `pending` silently, so a consumer that showed the event is never told. Common in everyday use (an editor undo and save). Fix: `ConfigEvent::Withdrawn { file }`, a variant on the enum this change introduces, which has no consumer yet.
  - `[low]` `[reject]` Tracked files are read whole, so oversize files are held in memory. The loader already caps reference files at 256 KB as errors; a multi-megabyte config file is unlikely, and a fix adds size guards to every read path.
  - `[medium]` `[patch]` Undo or Restore of an `OutsideEdit` entry for a created or deleted file treats "absent" as empty bytes: it writes a 0-byte file or fails with I/O. Verified: `undo`/`restore` write the entry's bytes. Fix: refused in words when the target side is absent.
  - `[low]` `[patch]` The am-store docs (`config_history.rs` module doc, `lib.rs:206`) list completion audits without `config.outside_edit`. Docs corrected.

### 2026-11-21 — Review pass (follow-up, after the first pass's patches)
- verdicts: 5 findings — high 0, medium 2, low 3, false 0, maybe-false 0
- findings:
  - `[medium]` `[patch]` On Linux, notify's inotify backend reports every open as `Access(Open)` (notify-8.2.2 `src/inotify.rs:349,429`). The worker's own reads re-dirty the scopes, so it re-reads the project every 300 ms forever (Linux CI; FSEvents on macOS reports no opens). Fix: `Access` events are ignored before scopes are marked dirty, with a unit test of the filter.
  - `[low]` `[reject]` An outside save that lands between `own_write`'s disk check and the writer's own read is folded into the app's edit, which is recorded as an `Edit` with no event. The window is a few milliseconds and needs a concurrent outside save during an app edit; it is the same read-to-rename race the 1.8 writer has with any outside writer. A fix threads an expected-before hash through the writer. Residual risk recorded with the right consequence (the change goes live inside the app's edit, not "the snapshot stays").
  - `[low]` `[reject]` After the fingerprint patch, a writer failure after the rename (folder fsync, or `commit_entry`) is announced as an outside edit, so one change gets two History entries (`outside_edit`, then the 1.8 intent completed by reconcile). This needs a failure after rename, which is rare. Before the patch the same failure hid the change for good; two entries are the lesser harm. A fix needs a recovery path in `own_write`. Residual risk recorded. `is_own_write` in `examine` now matches only during a write, which the state lock already excludes; it is kept as the plan's rule asks.
  - `[medium]` `[patch]` The first pass's empty-file patch applied an empty file created or deleted outside with no History entry or audit, breaking the Always rule. Fix: absent and empty are the same content where the session compares disk with accepted (`examine`, `differing`), so such a file is never announced or applied; the skip in `reload` is removed and the first-round test changed to that expectation.
  - `[low]` `[patch]` The IdentityChange refusal says the other changes "are then announced again"; already-announced content stays pending without a new event. Fix: the wording says they wait and apply at the next Reload.

## Design Notes

Decided under `docs/owner-standing-rules.md` "Decide yourself", with reasons:
- **Event shape:** `ConfigEvent::ChangedOnDisk { file, before_sha256: Option, after_sha256: Option, diff, findings: Vec<Finding>, identity_change: bool }` lives in am-config, not am-domain, because only am-config's consumers read it and `Finding` is am-config's.
- **Debounce:** 300 ms of quiet per file before comparing hashes. Editors save as temp-and-rename, and FSEvents coalesces events. Tests wait up to 10 s for an event and 1.5 s to prove none came.
- **Reload all-or-nothing:** a snapshot is one consistent version, so a partly applied Reload could hold two files that disagree.
- **Not wired into `serve`:** no API route or event stream type exists yet (am-api is a stub). Making one here would design the `/api/v1` contract outside its story. The session is complete as a library. Wiring it into `serve` (start the watcher, forward the event, a Reload route) goes with the story that defines the `/api/v1` config routes and the event stream.

## Verification

**Commands:**
- `cargo nextest run -p am-config -p am-store -p am-e2e -p xtask -E 'test(session) | test(config_history) | test(config_reload) | test(supply_chain) | test(licence)'` -- expected: all pass.
- `cargo deny check licenses` -- expected: ok.
- `just check` -- expected: green.

## Auto Run Result

Status: built (2026-11-21). The run first halted `blocked` on the owner's licence rule (notify is CC0-1.0). The owner chose "Exception for notify", the REPLAN landed on main as `2a38f54`, and this run resumed from that baseline.

**Summary:** `ConfigSession` in am-config holds the accepted bytes of every tracked file and a snapshot built only from them. A `notify` watcher (FSEvents on macOS, inotify on Linux) waits for 300 ms of quiet, then announces each new outside content once, as `ConfigEvent::ChangedOnDisk` with its diff, the errors it adds and an identity flag. A change reverted on disk sends `Withdrawn`. `reload(actor)` applies every pending change as one snapshot, all or nothing, with one `outside_edit` History entry per file. It is refused when nothing is pending, for a `project_id` change, and for an added error or a secret-looking value. The app's own edits go through the session and raise nothing.

**Files changed:**
- `Cargo.toml`, `Cargo.lock`, `crates/am-config/Cargo.toml`: notify =8.2.2 pinned.
- `deny.toml`: one exception, CC0-1.0 for notify@8.2.2, and the header reworded to match.
- `THIRD_PARTY.md`, `docs/crate-notes/notify.md`: the exception and how notify is used.
- `crates/am-domain/src/config_history.rs`: `HistoryKind::OutsideEdit` (`outside_edit`).
- `crates/am-store/src/{config_history.rs,lib.rs}`: `OutsideEdit` takes no ref and is audited as `config.outside_edit`; docs updated.
- `crates/am-config/src/lib.rs`: `load_exact` (the snapshot from an exact file map), `company_head` and `tracked`; the exact-map listing keeps link entries.
- `crates/am-config/src/skills.rs`: skill folders are listed through the loader.
- `crates/am-config/src/writer.rs`: crate-visible helpers, `forget_own_writes`, `WriteError::ChangedOnDisk`, and Undo/Restore of an outside create or delete refused in words.
- `crates/am-config/src/session.rs` (new): the session, watcher, events and Reload.
- Tests: `crates/am-config/tests/session.rs`, `crates/am-store/tests/config_history.rs`, `crates/am-e2e/tests/config_reload.rs` (with `crates/am-e2e/Cargo.toml`), `xtask/tests/supply_chain.rs`, and `fixtures/supply-chain/cc0-{probe,consumer}/`.

**Review:** two quick passes (the Review Triage Log has every row).
- Pass 1, 11 findings: 8 patched (1 high, 3 medium, 4 low) and 3 low rejected as rare, each needing new guards (stale snapshot after untracked-state errors, pre-existing errors listed by Reload, oversize reads).
- Pass 2 (follow-up, because pass 1 patched a high), 5 findings: 3 patched (2 medium, 1 low) and 2 low rejected (the race in an app edit, and a failure after rename recorded twice). Nothing deferred.
- Follow-up recommended: false. Pass 2 patched no high, so the work has converged.

**Verification:** `just check` passes (423 tests, 7 skipped). `just supply-chain` passes, including the guard that a second CC0-1.0 crate fails. `cargo deny check licenses` is ok. Every matrix row's test ran and passed. Each review fix had its own red run logged first.

**Residual risks:**
- The Linux watcher, including the filter that drops `Access` events, is first exercised on a real inotify by CI's Linux job.
- An outside save that lands in the few milliseconds between `own_write`'s disk check and the writer's read is folded into the app's edit, recorded as an `Edit` and never announced. This is the 1.8 writer's existing read-to-rename race with any outside writer.
- If a write fails after its rename, the change is announced as an outside edit and later also completed from the writer's intent: two History entries for one change.
- When an empty file that existed at open is deleted, nothing is announced, so the session's snapshot hash still counts it.
- The watcher timing tests wait up to 10 s; a slow CI runner could make them flaky.
- The session is not wired into `serve` yet. The story that adds the `/api/v1` config routes and event stream starts the watcher there, using `open_in` with the project's knowledge root.

