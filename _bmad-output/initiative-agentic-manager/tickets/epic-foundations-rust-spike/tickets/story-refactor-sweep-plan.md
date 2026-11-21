---
title: 'Refactor sweep'
type: refactor
ticket: 11
created: '2026-11-22'
status: done
baseline_revision: '24d48a95dddc9389f1b344bdb91cd7c585a21eba'
route: oneshot
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Epic 1 closes with deferred findings still open. Two are cleanup in epic 1's own code:
- **The 1.8 writer path re-reads too early.** `step`/`write_step`, used by `apply`, `undo` and `restore`, reads the file, runs a whole-project validation, then renames over it without reading it again. An editor save landing in that window is overwritten, and it is missing from History. This was deferred from 1.10's review; the migrate path already re-checks.
- **Paused time can hide a missing feature.** Tokio's `test-util` feature is unified across a workspace test build. A crate whose tests pause time without declaring the feature therefore passes `just check` but fails `cargo nextest run -p <crate>`. Deferred from 1.3.

The other deferred findings are now owned elsewhere:
- 4.19 owns the T1 drop files;
- 3.18 owns writer commands that change nothing;
- 4.2 owns the company week cap key.

Sofia commit `24d48a9` added the two new stories and marked these items resolved in `deferred-work.md`.

**Approach:**
- `write_step` re-reads the file right before it begins the intent. If the bytes are no longer `before`, it refuses with `WriteError::ChangedOnDisk { files: [file] }`, writing nothing and leaving no intent.
- An xtask test fails when a crate's tests pause tokio time but its `Cargo.toml` does not enable `test-util` itself.
- `deferred-work.md` marks the tokio item resolved by this story.

No new behaviour beyond the refusal, which completes 1.8's existing guarantee that an outside change is never taken in silently.

</intent-contract>

## Implementation Notes

**Route:** oneshot, because the change is under 100 lines: one re-check in `write_step`, two writer tests and one xtask guard test.

### Red run (2026-11-22, before any `src/` change)
`cargo nextest run -p am-config --test writer -E 'test(saved_outside_while_it_runs)' --no-fail-fast`: 2 run, 0 passed, 2 failed.
- `writer_edit_refuses_a_file_saved_outside_while_it_runs_and_keeps_the_save` panicked at `crates/am-config/tests/writer.rs:973` at `unwrap_err()` on an `Ok` value. The edit was written over the outside save.
- `writer_undo_refuses_a_file_saved_outside_while_it_runs_and_keeps_the_save` panicked at `crates/am-config/tests/writer.rs:1009` at `unwrap_err()` on an `Ok` value. The Undo was written over the outside save.

The seam is a test-only `ConfigHistory` wrapper (`SaveDuringWrite`). It writes the file on the first `pending_intents()` call after it is armed. `write_step` makes that call after the whole-project validation and before `begin_intent`. No hook is added to `src/`.

`xtask/tests/paused_time.rs` (the guard for tokio's `test-util` feature) passes from the start. No crate breaks the rule today: only am-testkit pauses time, and it enables `test-util`. It has no `src/` code behind it. Its self-test, `the_guard_names_a_crate_that_pauses_time_without_test_util`, shows that it names an offending crate. Its first run named `xtask` itself, because the guard's own file spells the calls out as text. That file is now excluded.

## Review Triage Log

### 2026-11-22 — Review pass
- verdicts: 8 findings — high 0, medium 1, low 6, false 1, maybe-false 0
- findings:
  - `[medium]` `[patch]` The re-read sat before `begin_intent`, a durable SQLite write in production, so a save during it or before the rename was still overwritten. Fix: the file is read again after `begin_intent`; on a change the intent is dropped and the write refused with `ChangedOnDisk`. The window is now only the rename.
  - `[low]` `[patch]` A save during a migration accept gave `ChangedSince` or `ChangedOnDisk` depending on which of two back-to-back checks it hit, against `migrate`'s doc. Fix: `migrate` reports a `ChangedOnDisk` from `write_step` as `ChangedSince` for that file.
  - `[low]` `[patch]` The `ChangedOnDisk` text said "then make the edit" (wrong for Undo and Restore), and the doc listed only edit, Undo and Restore. Fix: the text now ends "then try again". The docs name every path, and `write_step`'s doc states the refusal.
  - `[false]` `[reject]` The logged red-run line for the Undo test (1009) does not match the committed file (1013). `cargo fmt --all` ran after the red run (it was logged before the xtask guard was formatted) and shifted the lines. The test's statements are unchanged, and the edit test's line still matches because it sits above the reflowed code.
  - `[low]` `[patch]` Restore had no "saved outside while it runs" test. Fix: `writer_restore_refuses_a_file_saved_outside_while_it_runs_and_keeps_the_save`. Identity assignment goes through the same `write_step`, so it is covered by the shared check.
  - `[low]` `[patch]` The 1.10 plan's deferred writer finding was not recorded as closed. Fix: recorded under Auto Run Result. The 1.10 plan itself is done and is not edited.
  - `[low]` `[patch]` The guard missed `use tokio::time::{advance, pause}` with bare calls. Fix: in a file naming `tokio::time`, `pause()`, `advance(` or `resume()` also count; the self-test covers it.
  - `[low]` `[patch]` `enables_test_util` accepted a `tokio-util` or `tokio-stream` line with a `"test-util"` feature. Fix: the dependency key must be exactly `tokio`; the self-test covers a look-alike.

## Auto Run Result

Status: built (2026-11-22).

**Summary:**
- **Writer re-check:** `write_step` (shared by edit, Undo, Restore, identity assignment and migration) reads the file again once the intent is recorded. If an outside save changed it, the intent is dropped and the write is refused with `ChangedOnDisk`, so the save is never overwritten or missing from History. A migration accept reports it as `ChangedSince`.
- **Guard test:** `xtask/tests/paused_time.rs` fails the build when a crate pauses tokio time (written with its path, or imported from `tokio::time`) without enabling tokio's `test-util` in its own manifest.

**Deferred findings of epic 1, all closed or owned:**
- 1.10's writer race: fixed here.
- 1.3's `test-util` unification: guarded here, and marked resolved in `deferred-work.md`.
- 1.5's T1 drop files: new story 4.19.
- 1.7's writer commands that change no row: new story 3.18.
- 1.5's company cap key: owned by 4.2.
- `CARGO_TARGET_DIR`: resolved 2026-11-20.

The two new stories and their resolved lines landed in Sofia commit `24d48a9`.

**Files changed:**
- `crates/am-config/src/writer.rs`: the re-check, the error mapping in `migrate`, and the wording and docs.
- `crates/am-config/tests/writer.rs`: three tests (edit, Undo, Restore) through a test-only `ConfigHistory` wrapper.
- `xtask/tests/paused_time.rs` (new): the guard test.
- `_bmad-output/initiative-agentic-manager/deferred-work.md`: the tokio item marked resolved.

**Review:** one quick pass, 8 findings. 7 were patched (1 medium, 6 low) and 1 was false (the red-run line numbers moved under `cargo fmt`). Nothing deferred. Follow-up recommended: false (no high, and one medium).

**Verification:** `just check` passes. The red run was logged before the `src/` change.

**Residual risks:**
- The rename itself is the only remaining window, as in any writer without file locks.
- The guard matches text, so a pausing call written through an alias outside `tokio::time` is not seen.

## Verification

**Commands:**
- `cargo nextest run -p am-config --test writer -p xtask` -- expected: all pass.
- `just check` -- expected: green.
