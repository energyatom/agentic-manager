---
title: 'Row store and history'
type: feature
ticket: 6
created: '2026-11-24'
status: approved
baseline_revision: '8a305a3cca78154ccc2bade96435be0cdbd1b788'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: []
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Slice:** the store slice, epic 3 entry 6, "Row store and history", `after = [5]`. It is the first of four slices the owner cut from story 3.6 on 2026-11-24 (Design Notes, Re-slices). Build order:
1. **this slice**: migration 0010, the row command, the readers, store-level tests only;
2. the ToolGate slice (entry 23, "table_write through ToolGate", `after = [6]`);
3. 3.6c (entry 22, "Rows reaching later steps", `after = [23]`);
4. 3.6b (entry 21, "Document versions and Draft items", `after = [22]`).

Each has its own plan; the other three are committed on their own branches. This file was 3.6's plan, renamed twice. Matrix row numbers follow the approved 3.6 plan, so the written tests still point at them; a row whose engine half went to the ToolGate slice keeps its number here for the store half. Row 48 is new.

**Problem:** the store holds no rows.
- No `table_rows`, no `row_history`, no row command (am-store, migrations 0001 to 0009, `crates/am-store/src/migrations.rs` L35-L48).
- A step attempt journals no seat and nothing about what it may write (`start_attempt`, `crates/am-store/src/lib.rs` L1898).
- A tool call's journal row (0007's `tool_calls`) records its status only, so a result's kind has nowhere to live (AD-10 as amended, with Winston's wording: "on its tool call's journal row (AD-8)").

The verify of this slice (Sofia's entry 6 is the spec once merged), at the store, without the engine:
- the row command writes a row with its version and its own taint, and an after-image naming the Actor, run, step run and seat, in one transaction;
- a stale-version update is refused in words, nothing changes, and the old version stays readable;
- a repeated call sequence writes once;
- the tables are append-only, `INSERT OR REPLACE` included;
- nothing in shipped code calls the row command (owner, check-deps).

**Approach (owner-approved 2026-11-24, carried over unchanged):**
- **Migration 0010 (AD-21, AD-35, AD-7):** `table_rows` (table id, row id, JSON data, version, taint) and an append-only `row_history` of after-images; `step_runs.seat` and `step_runs.writes_table`; `tool_calls.result_kind` and `result_label`; their triggers. Every schema change of the row work is here, so this slice stands alone and the ToolGate slice needs no migration.
- **One row command with an explicit Actor (AD-6, AD-24):** `Writer::apply_row_write(actor, RowWrite)` inserts or updates one row and its after-image, finishes the call's `issued` journal row `succeeded` and marks its result kind `engine`, in one transaction. It is a writer method, not a `WriteCommand`.
- **Each row's own taint on write (AD-10 as amended and restated 2026-11-24):** the highest of the attempt's committed taint, the write's taint and the row's taint; it only rises; each after-image records it.
- **Stale versions (owner Q2):** an update names the version it read; a stale one is refused in words and changes nothing.
- **No shipped caller yet (owner, third re-slice):** `cargo xtask check-deps` names `apply_row_write` only in `crates/am-store/src/`. The ToolGate slice widens it to the engine's port implementation alone.

## Boundaries & Constraints

**Always:**
- **No shipped caller of the row command (owner condition):** `apply_row_write` stands in shipped code only in `crates/am-store/src/` (its definition and its `#[cfg(test)]` items); tests outside `src/` are not shipped code. Matched over whole-file text with whitespace normalised and line breaks joined, path calls (`Writer::apply_row_write(&w, …)`) included (3.5's T5 fix, as 3.4's resolve rule). Until the ToolGate slice lands, no engine, app or server path writes a row.
- **Writer commands with an explicit Actor (AD-6, AD-24):** the row command takes an `Actor` with no default and must be `Actor::Seat` of the attempt (its run, its step, its journaled seat); it commits with its audit entry in one transaction. Later Human (7.x) and Import (12.x) writes get their own commands.
- **Every row change has its after-image (AD-21, AD-24):** the history row is inserted before the row changes, in the same transaction; a schema trigger aborts a row change with no matching after-image.
- **Append-only (the 3.5 lesson):** `row_history` refuses UPDATE, DELETE and any INSERT that meets an existing `seq`, `(table_id, row_id, version)` or `(step_run_id, call_seq)`; `table_rows` refuses DELETE and any INSERT that meets an existing `(table_id, row_id)` (`INSERT OR REPLACE` by key or by id), and its UPDATE must raise the version by one and never change its table, id or creation time.
- **Each row's own taint, stored on write, only rising (owner; AD-10 as amended and restated 2026-11-24):** taint = max(attempt's committed taint, the write's taint, the row's taint); a trigger refuses lowering it; each after-image records the row's taint after that write, so no version holding a tainted step's data reads as clean.
- **Optimistic concurrency (owner Q2):** a stale `expected_version` is refused with `StoreError::RowRefused` in words naming the row id and its current version; the row, its version, its taint and its history are unchanged; earlier versions stay readable from `row_history`, and the current row from `Reader::row`.
- **Idempotency at the store (AD-8 as amended):** the key is (step run, call sequence) of a journaled `issued` `table_write` call; a second write at a key writes nothing and returns `Ok`. The write, the call's `succeeded` and its result kind commit together.
- **The result kind on the call's journal row (AD-10 as amended, Winston's wording; AD-8):** `apply_row_write` sets `result_kind = 'engine'` and `result_label = 'tool:table_write'`; `FinishToolCall` sets `untrusted` and `tool:<tool>`; triggers allow `engine` only for a `table_write` call with an after-image at its key, and a kind is set once. The data-free `Reader::row_receipt` is the record a result is rebuilt from (the rebuild itself is the ToolGate slice's).
- **The binding, at the store (AD-21, RULE-8):** a write's table must equal the attempt's journaled `writes_table`; the engine journals `None` in this slice (no `table:` key until the ToolGate slice), so no shipped path could write even if it called the command.
- **No read path (owner):** no `from_row`, no `row` input kind, no row-input slot, even test-only; `journal_inputs` keeps refusing an unknown kind. The row-content readers this slice adds (`rows`, `row`, `row_history`, `row_write`) are test-facing; no shipped code calls them, enforced from this slice by a check-deps rule (row 49, manager 2026-11-24): a reader is named in shipped code only in `crates/am-store/src/`. The ToolGate slice keeps that rule and adds its own.
- **Audit (AD-25; AD-32):** `row.written` holds ids, op, version, call sequence, taint and a column count; never row data, no hash of it, no new `*_sha256` of free text.
- **Inert document field:** `AttemptWrites.document` exists so `StartAttempt` literals change once; a value is refused in words naming 3.6b.
- **New callers of `Instruction::authored` (owner):** none.
- **Order of work (owner):** this plan is committed; the tester splits the written tests by slice; this slice's red run is logged before any `src/` change; one squash commit, on Opus; stop and report if the code passes about 4,000 lines.
- **Tests first:** store-level tests only; no network, no real model.

**Never:**
- **No shipped caller of `apply_row_write`,** and no engine, tool, ToolGate, app or server change beyond the `StartAttempt` literal (`writes` with the seat and `table: None`).
- **No `RowWriter` port, no `table_write` tool, no `EngineResult`, no `table:` key, no ToolGate change** (the ToolGate slice).
- **No delete or move of rows; no rollback.**
- **No status-transition rule, reason field, `v_*` view or grant read** (7.4, 7.5, 7.2).
- **No document versions or Draft items** (3.6b); **no row reads** (3.6c).
- **No new crate. No department vocabulary in `crates/`.**

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 2 | Insert at the store | An attempt started with `writes { seat, table: T }`; an `issued` `table_write` call at (attempt, 1); `apply_row_write` as that Seat, `Insert { row_id, data }` | One `table_rows` row, version 1, taint the attempt's; one `row_history` after-image naming the Seat actor, run, step run, seat, call sequence and tool call; the call `succeeded`, `result_kind = 'engine'`, `result_label = 'tool:table_write'`; audit `row.written` with ids only; `row_receipt` gives row id, version 1, op `insert`, no data | — |
| 3 | Update at the store | `Update { row_id, expected_version: 1, patch }` at a new call | Version 2 = version 1 merged with the patch's columns (`null` clears one); a second after-image (the whole row); version 1's after-image unchanged | — |
| 4 | Stale version (owner Q2) | `expected_version: 1` while the row is at 2; and `expected_version: 2` on a row still at 1 | `StoreError::RowRefused` in words ("row `<id>` is at version 2"); nothing changes: the row's data, version and taint byte-equal, no new after-image, the call still `issued` with no kind; every earlier version readable from `row_history`; the current row readable from `row` as it was | Refused in words |
| 12 | A tainted attempt's write (owner: taint on write) | An attempt with committed taint 1 writes a row | Row taint 1; after-image taint 1; audit args carry it | — |
| 13 | Taint only rises (owner; AD-10 as restated) | Row tainted at version 2; a clean attempt writes version 3 | Row taint stays 1; version 3's after-image records 1, not the clean writer's 0; a direct `UPDATE` lowering taint aborts | Trigger |
| 15 | Repeated call sequence | `apply_row_write` twice with one (step run, call sequence) | One after-image, one version; the second returns `Ok` and writes nothing (no row, version, history row or audit entry); `row_receipt` gives the first write | — |
| 16 | A new attempt | Attempt 2 of the step, call sequence 1 again | A new write: the key is the attempt's step run, not the step key | — |
| 17 | The store refuses | Actor not a Seat; a Seat of another run, step or seat; step not `running`; no `issued` `table_write` call at the key, or another tool's; a table not the attempt's `writes_table` (`None` included); insert over an existing row id; an update of an unknown row | Refused in words naming store ids only; nothing written | Worded |
| 18 | Append-only | `UPDATE`/`DELETE` on `row_history`; `INSERT OR REPLACE` on it by `seq`, version key or call key; `DELETE` or `INSERT OR REPLACE` on `table_rows` by key; a row change without its after-image; changing a row's table, id or creation time; a version that is not current + 1 | Aborted by trigger | Trigger |
| 19 | One transaction | A write refused after its after-image was inserted (forced) | No row, no history, the call still `issued`, no result kind | Rolled back |
| 22 | Audit | A write whose data holds a canary | `row.written` args: op, version, step run, call sequence, taint, column count; no canary and no hash of it in the audit export; the chain verifies | — |
| 28 | Document refused (store half) | `StartAttempt` with `writes.document: Some(..)` | Refused in words naming 3.6b; nothing written | Worded |
| 32 | Migration 0010 | A version-9 database with runs, attempts, calls, tool calls, Inbox items, prompt parts | Pre-migration copy; every row kept value for value; `table_rows` and `row_history` empty; `step_runs.seat`, `.writes_table`, `tool_calls.result_kind`, `.result_label` NULL on old rows; no `step_inputs.from_row` (3.6c) and no `document_versions` (3.6b); report 9 → 10; `store.migrated` audited; reopening applies nothing; a failing 0010 names the backup and leaves version 9 | Migration error |
| 42 | No way to read rows (owner, store half) | This slice's tree | `step_inputs` has no `from_row` after 0010; `StartAttempt` with an input of kind `row` is refused in words; `compile_fail`: no `StepInput::from_row` (3.6c supersedes it) | Refused; does not compile |
| 44 | Result kind on the call's journal row (store half) | A succeeded write; a failed call finished by `FinishToolCall`; a refused call; a forced `engine` on a call with no after-image or on another tool; a second change of a set kind | `engine` / `tool:table_write`; `untrusted` / `tool:<tool>`; none; aborted; aborted. `row_receipt` carries no data field | Trigger |
| 48 | No shipped caller of the row command (owner condition) | Planted files: `.apply_row_write(` in am-engine `run.rs`, am-app and am-server; the same split over lines or spaced around `.`; `Writer::apply_row_write(&w, …)`; the shipped tree | `cargo xtask check-deps` fails for each planted form, naming file and line and the owner's condition; the shipped tree passes | CI fails |
| 49 | No shipped reader of row content (manager, 2026-11-24; narrows only) | Planted files: a call of the row-content readers (`.rows(`, `.row(`, `.row_history(`, `.row_write(` on the store `Reader`) in am-engine, am-providers, am-tools, am-app and am-server; split over lines, spaced around `.`, through a `use … as` alias | `cargo xtask check-deps` fails for each, naming file and line; the shipped tree passes (the readers are named only in `crates/am-store/src/` and tests) | CI fails |

</intent-contract>

## Code Map

- **`crates/am-domain/src/rows.rs`** (new, the store's part): `RowOp::Insert { row_id: Uuid, data_json }`, `RowOp::Update { row_id: Uuid, expected_version: i64, patch_json }`, `RowWrite { tool_call_id, step_run_id, call_seq, table_id, op, taint }`. Owned types only (AD-5). The port and the other types are the ToolGate slice's.
- **`crates/am-store`:**
  - `migrations/0010_rows.sql` (new): `table_rows`, `row_history`, `step_runs.seat`, `step_runs.writes_table`, `tool_calls.result_kind` (CHECK `engine`, `untrusted`), `tool_calls.result_label`, and the triggers of Design Notes 2; `src/migrations.rs` `MIGRATIONS` L35-L45 gains it; `SCHEMA_VERSION` L48 becomes 10. (0011 is 3.6c's; 0012 is 3.6b's.)
  - `src/lib.rs`:
    - `StoreError` L86: `RowRefused(String)`.
    - `WriteCommand::StartAttempt` L245 gains `writes: AttemptWrites { seat, table, document }`; `start_attempt` L1898 journals the seat and table (set once); a `document` value is refused in words naming 3.6b.
    - New `Writer::apply_row_write(actor, am_domain::rows::RowWrite) -> Result<(), StoreError>` beside `resolve_inbox_item` L808; not a `WriteCommand`, so `Writer::exec` (L752) cannot write a row.
    - `FinishToolCall` arm L1358: also sets `result_kind = 'untrusted'` and `result_label`; no new field.
    - `journal_inputs` L1634: unchanged (a `row` kind stays refused).
    - `Reader` L2959: `rows`, `row`, `row_history`, `row_write` (test-facing, carry data); `row_receipt(step_run_id, call_seq) -> Option<RowReceipt>` (no data); `attempt_writes`; `ToolCallRow` L2934 gains `result_kind`, `result_label`.
  - `src/rows.rs` (new): the command's checks, the patch merge, the SQL.
  - `src/audit.rs` L35 `sha256_hex`: not called for row data.
- **`crates/am-engine`** (literal only): `src/run.rs` `StartAttempt` L1136 and `src/journal.rs`: `writes: AttemptWrites { seat: step.seat, table: None, document: None }`. No other engine change.
- **`xtask/src/deps.rs`:** one call-site rule beside `RESOLVE_TOKENS` L99-L116: `apply_row_write` stands in shipped code only in `crates/am-store/src/`; the finding names the owner's condition and AD-21. Tests in `xtask/tests/check_deps.rs`.
- **Docs:** none (no config key).
- **Existing tests that must change (supersession):**
  - `crates/am-store/tests/prompt_journal.rs` L244-L248: `SCHEMA_VERSION == BEFORE_PROMPT_PARTS + 1` ("0009 is this story's") becomes "the 8 to 9 step is applied".
  - Every `WriteCommand::StartAttempt` literal gains `writes` (am-store `src/lib.rs` tests and `tests/{run_journal,inbox,prompt_journal,inbox_resolve}.rs`; am-app `tests/{gate_cli,gate_tty}.rs`; am-e2e `tests/trust/{rule_16_audit_log,taint_trusted_input_sources,taint_at_rest,rule_15_inbox,taint_prompt_type_wall}.rs`, `tests/engine/handoff_gate_resolve.rs`; am-engine `src/{journal,run}.rs`).
  - `crates/am-store/tests/inbox_resolve.rs` L575-L611, L651-L655 (Draft): unchanged here (3.6b).

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build (owner):** Winston's AD-10 amendment merged (#35), with "on its tool call's journal row (AD-8)" in the re-slice PR. This plan committed. The tester splits the written tests by slice.
- [ ] **Tests first (no `src/` change), on Opus:** this slice's files in the Tests-first list and the `StartAttempt` literals. Log this slice's red run.
- [ ] **am-domain:** `RowWrite`, `RowOp`.
- [ ] **am-store:** migration 0010, `apply_row_write`, `AttemptWrites`, `FinishToolCall`'s kind, `RowRefused`, the readers and `row_receipt`, the audit args.
- [ ] **am-engine:** the `StartAttempt` literal.
- [ ] **xtask:** the `apply_row_write` rule and its planted-file tests.
- [ ] **Size (owner):** one squash commit; stop and report if the code passes about 4,000 lines.

**Acceptance Criteria:**
- Given each matrix row of this slice, when its test runs, then rows, after-images, versions, taint, journal kinds, refusals and audit entries match the row.
- Given the store-level verify, when the store tests run, then a write lands with version, taint and an after-image naming run, step and seat; a stale update is refused with nothing changed and the old version readable; a repeated call sequence writes once.
- Given `cargo xtask check-deps`, then no shipped file but am-store's names `apply_row_write`.
- Given the workspace, when `just check` runs, then it is green. Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-24, manager: row 49 moves the rule that keeps the row-content readers out of shipped code into this slice, so it holds from the moment the readers exist (it only narrows; the ToolGate slice keeps it).
- 2026-11-24, owner: approved with the recommended answers to Q1–Q9 and conditions (Design Notes, Owner answers). The plan now builds Q1 as am-tools' tool over an am-domain row-writer port that am-engine implements and only ToolGate calls, and Q4 as an engine-built trusted result after Winston's AD-10 amendment. Added matrix rows 33 to 39, the row-input slot for the reader test, and their tests. Status set to approved.
- 2026-11-24, owner (re-slice): the tests alone reached about 8,400 lines, so 3.6 overran. Re-sliced into 3.6a (this plan, entry 6, renamed from `story-rows-and-documents-written-with-provenance-plan.md`) and 3.6b (entry 21, "Document versions and Draft items", its own plan). Approved answers carried over unchanged. Moved to 3.6b: rows 23–27, 29, 30, 39 and the document halves of 18, 22, 28, 31; Q5–Q8's build; `document_versions` and `step_runs.writes_document`. Added row 40 (fan-out validate warning, owner). Row 13 corrected to AD-10 as restated 2026-11-24 (the after-image records the row's taint after the write; the written tests already assert it). Baseline moved to `1c44dfb` (main after #36).
- 2026-11-24, owner (second re-slice): the row-read side (the row-input slot, the `from_row` link, reader taint, fenced row text, row 38 and its tests, the `from_row` column) moved to a new 3.6c, entry 22, "Rows reaching later steps", built after 3.6a and before 3.6b; Q4's reader condition moved with it, unchanged. Added rows 42 (no read path), 43 (no row content reaches a model call, test and check-deps rule) and 44 (the engine result journaled on the tool-call journal and rebuilt from the writer's record, AD-10 as amended); rows 12 and 13 carry the owner's "each row's own taint stored on write". Migrations renumbered: 0010 here, 0011 for 3.6c, 0012 for 3.6b. Size estimate updated (Q9).
- 2026-11-24, owner (third re-slice): 3.6a split by layer. This plan, renamed from `story-rows-written-with-provenance-through-toolgate-plan.md` to `story-row-store-and-history-plan.md`, is now the store slice (entry 6, "Row store and history", `after = [5]`): migration 0010 with every schema change of the row work, the row command, append-only triggers, taint on write, the stale-version refusal, the result-kind columns, store-level tests, and new row 48 (no shipped caller of the row command, check-deps). Everything else of 3.6a moved to the ToolGate slice (entry 23, `.git/wip/story-table-write-through-toolgate-plan.md`): rows 1, 5–11, 14, 20, 21, 31, 33–37, 40, 43 and the engine halves of 2–4, 12, 13, 15, 22, 28, 42 and 44. AD-10 takes Winston's wording ("on its tool call's journal row (AD-8)"). Size now reported as code and test lines; the overrun stop is about 4,000 lines of code.

## Design Notes

### Owner answers (2026-11-24)

**Owner, verbatim:** "Approved: the 3.6 plan with the recommended answers to Q1–Q9 and these conditions. Q1: a check-deps rule so only ToolGate calls the row-writer. Q2: tests that a stale-version update is refused and the old version stays readable. Q3: record the interim risk, and add "changing table: or document:" to 4.17 as widening. Q4: Winston merges the narrow AD-10 amendment first (results built solely from engine values by ToolGate, enforced by check-deps, error results excluded and still untrusted, planted-string test), and a row written by a tainted step carries that taint to any later reader, with a test. Q7: record as my approved deviation from "one version per run". Q8: Draft dismissal uses the 3.4 Human-only path, with a test that a non-human call cannot dismiss. Q9: one squash commit; stop and report if it overruns. Merge #34 on green on every run. Then commit the plan, log the red run, and build tests first on Opus. Merge only with both CI jobs green after review triage."

**Answers and conditions, as the plans build them:**
- **Q1:** `table_write` in am-tools, writing through an am-domain row-writer port (owned types); am-engine implements it over the store's writer; only ToolGate calls it; `agentic run` offers it only to a step that declares a table. *Condition:* a check-deps rule so only ToolGate calls the port's method (Code Map, xtask; matrix row 33).
- **Q2:** insert, or update by row id with an expected version; no delete; every after-image kept. *Condition:* tests that a stale update is refused in words, nothing changes, and the old version stays readable (matrix row 4).
- **Q3:** step keys `table:` and `document:`; a seat writes only its own team's table; one team per document id. *Condition:* the interim risk is recorded (Boundaries); Sofia adds "changing `table:` or `document:`" to 4.17 as widening in a separate replan.
- **Q4:** a trusted result built by ToolGate solely from engine values (row id, version, a fixed status word). *Conditions:* Winston's narrow AD-10 amendment merges first; check-deps keeps its constructor in ToolGate; error results stay untrusted; a planted-string test; a row written by a tainted step taints a later reader, with a test (matrix rows 35 to 38).
- **Q5:** a version comes from the document step's final message.
- **Q6:** a `document_versions` row, with the Markdown kept once.
- **Q7:** one version per succeeded attempt. *Condition:* recorded as the owner's approved deviation from the ticket's "one Markdown version per run" (Intent, matrix row 25).
- **Q8:** one Draft item per version, dismissable here. *Condition:* dismissal uses 3.4's Human-only path, with a test that a non-Human call cannot dismiss (matrix row 39).
- **Q9:** one squash commit. *Condition:* stop and report on an overrun.
- **Sequence:** #34 merges green on every run; then the plan is committed, the red run logged, and tests built first on Opus; merge only with both CI jobs green after review triage.

**In this slice:** Q2 with its condition, at the store (the refusal, nothing changed, the old version readable; the model's side is the ToolGate slice's); Q1's store side (the row command, `Writer::apply_row_write`, which the ToolGate slice's port implementation calls; the owner's later condition keeps it uncalled here); the store side of Q4's journaling (the result kind on the call's journal row, the data-free receipt); and Q9 for this slice, with the owner's code-based stop. Q3, the rest of Q1 and Q4 are the ToolGate slice's; Q4's reader condition is 3.6c's; Q5 to Q8 are 3.6b's. No answer changed.

### Re-slices (owner, 2026-11-24)

**First, verbatim:** "Don't accept a single 12,000-line commit. Sofia re-slices 3.6 into 3.6a (row store, history, row-writer interface, table_write via ToolGate only, engine-built result, tainted-row rules) and 3.6b (document versions, Draft items with Human-only dismissal). My approved answers carry over unchanged. Split the written tests by slice, log a red run for each, one squash commit per slice, on Opus. Fan-out: accept that the Company A f2-update step fails until 3.14, but agentic validate must name the reason, and 3.14's ticket must include allowing table: on fan-out steps."

**Second, verbatim:** "Split again: move the row-read side (row-input slot, from_row link, reader taint, fenced row text) into a new 3.6c, "Rows reaching later steps", built right after 3.6a and before 3.6b. Condition: 3.6a contains no way for a step to read rows, with no from_row and no row-input slot, even test-only, plus a test or check-deps rule proving no engine path passes row content to a model call. 3.6a still stores each row's own taint on write. My Q4 condition moves to 3.6c unchanged. Add the AD-10 journaling of the engine result to 3.6a."

**Third, verbatim:** "Split 3.6a by layer: a store slice (migration 0010, row and history write commands, append-only triggers, taint on write, stale-version refusal, store-level tests) and a ToolGate slice (row-writer interface, table_write via ToolGate only, engine result and journaling, idempotency, check-deps rules, no-read-path proof, fan-out warning). Order: store, ToolGate, then 3.6c, then 3.6b. Condition: nothing in shipped code calls the row-write commands until the ToolGate slice lands, enforced by check-deps. Accept Winston's wording "on its tool call's journal row (AD-8)" in the same re-slice PR. From now on, plan estimates report code and test lines separately, and the overrun stop is about 4,000 lines of code."

**What this means here:**
- This slice is the third decision's store slice. Every schema change of the row work is in its 0010 (`step_runs.seat`, `.writes_table` and the result-kind columns included), so it stands alone and the ToolGate slice adds no migration.
- The second decision's conditions that land at the store land here: each row's own taint on write; no `from_row` and no `row` kind.
- Winston confirmed the AD-10 reading: the engine result is journaled on its tool call's journal row (AD-8) and rebuilt from the writer's committed record; no `prompt_parts` kind is needed. This slice adds the columns and the kind; the ToolGate slice adds the result and its rebuild.

### 1. The command, uncalled (owner condition)

- `Writer::apply_row_write(actor: Actor, write: RowWrite) -> Result<(), StoreError>`; a writer method, not a `WriteCommand`, so `exec` cannot write a row.
- **The check-deps rule (row 48):** the token `apply_row_write`, in shipped code, only in `crates/am-store/src/` (its definition and `#[cfg(test)]` items). Whole-file text, whitespace around `.` and `::` normalised, line breaks joined, so `. apply_row_write\n(` and `Writer::apply_row_write(&w, …)` are caught. A method cannot be called without its name: seam-guard bans `macro_rules!`, `include!` and `#[path]` in shipped source. The finding names the file, the line, the owner's condition and AD-21.
- **Widening:** the ToolGate slice adds exactly one site, `crates/am-engine/src/row_writer.rs`, its port implementation, and its own rules (only ToolGate calls the port; the row-content readers stay in am-store).
- **Belt and braces:** the engine journals `writes.table = None` in this slice, and the command refuses a table that is not the attempt's `writes_table`, so even a call would write nothing.

### 2. The row store (migration 0010)

`table_rows` (one row per current row, AD-21):

| Column | Why |
|--------|-----|
| `table_id TEXT NOT NULL` | The config table (`tables/<id>.yaml`); one generic store, no per-table DDL (AD-21) |
| `row_id TEXT NOT NULL` | UUIDv7 minted by the engine (ToolGate, next slice); never the model's text, so audit can name it |
| `data TEXT NOT NULL` (`json_valid`, an object) | The row's columns as JSON (AD-21) |
| `version INTEGER NOT NULL ≥ 1` | Optimistic concurrency; the version of the newest after-image |
| `taint INTEGER NOT NULL (0, 1)` | The row's own taint, stored on write (AD-10 (c)); only rises |
| `created_at`, `updated_at` | AD conventions (UTC, ms, `Z`) |
| `PRIMARY KEY (table_id, row_id)` | One row per id |

`row_history` (append-only after-images, AD-21, AD-24):

| Column | Why |
|--------|-----|
| `seq INTEGER PRIMARY KEY AUTOINCREMENT` | Order; AD-22's `start_seq` high-water mark (7.2); the snapshot 3.6c's `row` edge names (AD-43) |
| `table_id`, `row_id`, `version` | Which row, which version; `UNIQUE (table_id, row_id, version)` |
| `data TEXT NOT NULL` | The whole row after the change |
| `taint INTEGER NOT NULL` | The row's taint after this write (AD-10 as restated) |
| `actor TEXT NOT NULL` | The Actor as JSON (AD-24); a Seat here |
| `run_id`, `step_run_id`, `seat`, `call_seq`, `tool_call_id` | Provenance and the idempotency key `UNIQUE (step_run_id, call_seq)`; all set for a Seat, all NULL for a later Human or Import write (CHECK) |
| `op TEXT` (`insert`, `update`) | What the change was |
| `created_at` | When |

Other columns, NULL on every old row:
- `step_runs.seat`, `step_runs.writes_table`: what the attempt may write, journaled at its start, set once (trigger). The store checks a write against them, not only against the engine's word (as 3.4 journaled `max_returns`); a fact of the attempt, not a copy of config (the run pins only the snapshot's hash, AD-16).
- `tool_calls.result_kind` (CHECK `engine`, `untrusted`), `tool_calls.result_label`: the result's kind on its tool call's journal row (AD-8, AD-10 with Winston's wording).

Triggers:
- `row_history_no_update`, `row_history_no_delete`, `row_history_never_replaced` (an INSERT meeting an existing `seq`, version key or call key aborts).
- `table_rows_no_delete`, `table_rows_never_replaced` (an INSERT meeting `(table_id, row_id)` aborts), `table_rows_fixed_fields` (`table_id`, `row_id`, `created_at`), `table_rows_version_steps` (+1 only), `table_rows_taint_only_rises`.
- `table_rows_needs_after_image`: INSERT and UPDATE abort unless `row_history` holds `(table_id, row_id, NEW.version)` with `data = NEW.data` and `taint = NEW.taint`.
- `step_runs_writes_set_once`.
- `tool_calls_engine_result_needs_its_write` (`engine` only for a `table_write` call with an after-image at its `(step_run_id, call_seq)`), `tool_calls_result_kind_set_once`.
- No `reason` or `dry_run` column yet (7.4, AD-37 add them additively). Later deletion (AD-25) is a Human command in a later story, with its own migration.

`apply_row_write`'s checks, in one transaction, refused in words with nothing written:
1. an after-image at `(step_run_id, call_seq)` exists: write nothing, return `Ok` (the replay);
2. the actor is `Actor::Seat`, its run and step are the step run's, its seat the journaled `seat`;
3. the step run is `running`;
4. a `table_write` call at `(step_run_id, call_seq)` is `issued`, with this `tool_call_id`;
5. `table_id` equals the attempt's `writes_table`;
6. insert: no row with `row_id`; update: the row exists and is at `expected_version`, else `RowRefused` naming the row id and its current version.

Then: taint = max(attempt's committed taint, the write's taint, the row's taint); merge an update's patch into the current data (top-level keys; `null` clears); insert the after-image with that taint; insert or update the row; finish the call `succeeded` with `result_kind = 'engine'`, `result_label = 'tool:table_write'`; audit `row.written` (target `row:<table_id>/<row_id>`; args op, version, step run, call sequence, taint, column count).

Readers: `rows`, `row`, `row_history`, `row_write` carry data and are test-facing (the ToolGate slice's rule keeps them out of shipped code outside am-store); `row_receipt` carries the typed record a result is rebuilt from (table id, row id, version, op, taint, call, tool call id, column count), never data; `attempt_writes`.

### 3. What stays for the other slices

- **ToolGate slice (entry 23):** the `RowWriter` port and the rest of `am_domain::rows`; `table_write` in am-tools; `StoreRowWriter` (the one caller of `apply_row_write`); ToolGate's grants, class gate, binding, `EngineResult`, its rebuild and engine-level idempotency; the `table:` key, team checks and the fan-out warning; the other check-deps rules (port, engine result, row-content readers, `rusqlite`); the no-read-path run test; the end-to-end tests.
- **3.6c (entry 22):** the row-input slot, `from_row` (0011), the `row` kind, fenced row text, reader taint, Q4's reader condition.
- **3.6b (entry 21):** documents (0012), Draft items, their Human-only dismissal.

### Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code:** about 900 to 1,200 lines: migration 0010 with its triggers (~180), am-store's command, checks, merge, readers, receipt, `AttemptWrites`, `FinishToolCall` kind and audit (~600 to 800), am-domain `RowWrite` and `RowOp` (~40), the engine `StartAttempt` literal (~10), the xtask rule (~80 to 120). **Well under the ~4,000-line stop.**
- **Tests:** about 2,300 to 2,600 lines: am-store `tests/rows.rs`'s store tests (~1,750 of the written 2,121, less the document and row-input tests), the new store tests (result kind, no read path, ~250), the xtask planted files (~120), the `StartAttempt` literal sweep across store, app and e2e tests (~250), doctest (~10).

### Tests-first list

Store-level only; no network; no real model. The tester splits these from the tests written for 3.6 and logs this slice's red run.

**am-store (`crates/am-store/tests/rows.rs`, the store tests):**
- `migration_0010_keeps_every_row_and_the_pre_migration_copy`, `migration_0010_adds_empty_tables_and_null_columns` (row 32: `table_rows`, `row_history`, `step_runs.seat`, `.writes_table`, `tool_calls.result_kind`, `.result_label`; no `from_row`, no `document_versions`), `reopening_a_version_10_database_applies_nothing`, `a_failing_0010_names_the_backup_and_leaves_version_9` (row 32). Red: `SCHEMA_VERSION` is 9.
- `an_insert_writes_the_row_and_its_after_image_together` (row 2); `an_update_needs_the_current_version_and_appends_an_after_image` (row 3).
- `a_stale_update_is_refused_in_words_and_nothing_changes`, `the_old_version_stays_readable_from_history_and_as_the_current_row` (row 4, owner Q2).
- `a_repeated_call_sequence_writes_once` (row 15); `the_key_is_the_attempt_not_the_step` (row 16).
- `the_writer_refuses_a_write_that_is_not_its_attempts` (row 17); `rows_and_history_are_append_only`, `a_row_change_without_its_after_image_is_aborted` (row 18); `a_refused_write_leaves_nothing` (row 19).
- `a_tainted_attempts_row_stores_its_taint` (row 12) and `row_taint_only_rises` (row 13) (owner: taint on write).
- `row_audit_holds_ids_never_data` (row 22).
- `a_document_value_is_refused_until_3_6b` (row 28).
- `there_is_no_row_input_kind_and_no_from_row_column` (row 42).
- `a_succeeded_write_journals_an_engine_result_kind`, `an_engine_kind_needs_its_after_image_and_is_set_once`, `finish_tool_call_journals_untrusted`, `the_receipt_carries_no_row_data` (row 44).
- Red for each: no `apply_row_write`, tables, columns, triggers or readers.
- **Superseded:** `prompt_journal.rs` L244-L248.

**am-store doctest:** `compile_fail` for `|i: am_store::StepInput| i.from_row` (row 42). New; green on arrival, logged; 3.6c removes it.

**xtask (`xtask/tests/check_deps.rs`):** `no_shipped_code_reads_row_content` (row 49: planted reader calls in am-engine, am-providers, am-tools, am-app, am-server, split, spaced and aliased, each must fail; the shipped tree passes); `the_row_write_command_is_named_only_in_the_store` (row 48: planted `.apply_row_write(` in am-engine `run.rs`, in am-app, in am-server; split over lines and spaced; `Writer::apply_row_write(&w, …)`; each must fail, naming file and line); `the_shipped_tree_passes_the_row_write_rule` (row 48). Red: no such rule. The ToolGate slice widens both.

**Literals:** `writes` on every `StartAttempt` literal (the tester's edits written for 3.6, without `from_row`).

## Verification

- `cargo nextest run -p am-store --test rows` and `--test prompt_journal`; the store, app and e2e targets whose `StartAttempt` literals changed; `cargo test -p xtask --test check_deps`.
- `cargo test --doc -p am-store`.
- `cargo xtask check-deps`: the `apply_row_write` rule green on the shipped tree and red on each planted file.
- `just check` at the end.
- No `cargo clean`, and no build of a workspace copy against the shared target (AGENTS.md, owner 2026-11-23).
- Merge only with both CI jobs green after review triage (owner, 2026-11-24).

## Implementation Notes

### API the tests fix (store slice)

*Tester, 2026-11-24, after the third re-slice. The ToolGate, row-read, document and Draft items are in their slices' plans; the tests that pin them are saved as patches (below). New types derive `Debug, Clone, PartialEq, Eq` unless said otherwise; the tests compare them with `assert_eq!`. Choices the plan left open are marked (tester's choice).*

**am-domain** (`pub mod rows`; the tests import from the module):
- `pub enum RowOp { Insert { row_id: Uuid, data_json: String }, Update { row_id: Uuid, expected_version: i64, patch_json: String } }`.
- `pub struct RowWrite { pub tool_call_id: String, pub step_run_id: String, pub call_seq: i64, pub table_id: String, pub op: RowOp, pub taint: Taint }` (owned types only: `Send + Sync + 'static`; a crate-docs doctest builds both ops).

**am-store** (root exports):
- `StoreError::RowRefused(String)`: its `Display` is the words. Stale version: the words name the row id and `version <current>` (for example "row `<id>` is at version 2") and never a patch value. Unknown row: the words hold `does not hold` and never the id.
- `pub struct AttemptWrites { pub seat: String, pub table: Option<String>, pub document: Option<String> }`, and `WriteCommand::StartAttempt { step_run_id, inputs, taint, prompt, writes: AttemptWrites }` with no other new field. A `document: Some(_)` is refused in words that hold `3.6b`; the attempt stays queued and nothing is written. An input of kind `row` is refused in words (the kinds stay 3.4's four), and those words never hold a row id.
- `Writer::apply_row_write(&self, actor: Actor, write: am_domain::rows::RowWrite) -> Result<(), StoreError>`. A replay at a key that already has an after-image is `Ok` and writes nothing, also after the attempt finished. Every other refusal is an `Err` with nothing written. Only the unknown row's refusal is pinned to `RowRefused`; the others may be `Invalid`.
- `ToolCallRow` gains `pub result_kind: Option<String>` and `pub result_label: Option<String>`:
  - a succeeded write gives `engine` / `tool:table_write`;
  - `FinishToolCall` gives `untrusted` / `tool:<tool>`, for `succeeded` and `failed` alike;
  - a refused call and an `issued` one have `None` / `None`.
- `pub struct RowReceipt { pub table_id: String, pub row_id: String, pub version: i64, pub op: String, pub taint: Taint, pub step_run_id: String, pub call_seq: i64, pub tool_call_id: String, pub columns: i64 }` (tester's choice of field names, from the plan's list: table id, row id, version, op, taint, call, tool call id, column count). `columns` is the insert's number of data keys (3 for a three-column insert; the update's count is not pinned). Its `Debug` holds no row data. A doctest pins that it has no `data` field.
- Records (tester's choice of names and fields):
  - `pub struct RowRecord { pub table_id: String, pub row_id: String, pub data: String, pub version: i64, pub taint: Taint, pub created_at: String, pub updated_at: String }`;
  - `pub struct RowHistoryRecord { pub seq: i64, pub table_id: String, pub row_id: String, pub version: i64, pub data: String, pub taint: Taint, pub actor: Actor, pub run_id: Option<String>, pub step_run_id: Option<String>, pub seat: Option<String>, pub call_seq: Option<i64>, pub tool_call_id: Option<String>, pub op: String, pub created_at: String }`. Here `op` is `insert` or `update`, and `data` is the whole row after the change.
- `Reader` methods, each returning `Result<_, StoreError>`:
  - `rows(&self, table_id: &str) -> Vec<RowRecord>`, in creation order;
  - `row(&self, table_id: &str, row_id: &str) -> Option<RowRecord>`;
  - `row_history(&self, table_id: &str, row_id: &str) -> Vec<RowHistoryRecord>`, in version order;
  - `row_write(&self, step_run_id: &str, call_seq: i64) -> Option<RowHistoryRecord>`;
  - `row_receipt(&self, step_run_id: &str, call_seq: i64) -> Option<RowReceipt>`;
  - `attempt_writes(&self, step_run_id: &str) -> Option<AttemptWrites>`, which is `None` for an old attempt.
  The first four read row content and are test-facing (row 49).
- Migration 0010 (`SCHEMA_VERSION` at least 10; no test pins it equal):
  - tables `table_rows` and `row_history`, with the plan's columns;
  - `step_runs.seat` and `.writes_table`;
  - `tool_calls.result_kind` and `.result_label`, whose CHECK refuses any kind but `engine` and `untrusted`.
  A database made at exactly version 10 (`testing::database_at_version`) has no `step_inputs.from_row` and no `document_versions`. The report line is `from version 9 to 10`, and `store.migrated` is audited. A table named `table_rows` made by hand fails the step with `MigrationFailed { from: 9, to: 10, backup: Some(_) }`.
- Triggers abort each of these raw changes:
  - on `row_history`: `UPDATE`, `DELETE`, and `INSERT OR REPLACE` by `seq` or by version key;
  - on `table_rows`: `DELETE`; `INSERT OR REPLACE`; an `UPDATE` of `table_id`, `row_id` or `created_at`; a version step other than +1; a lowered taint; a row change with no after-image;
  - on `tool_calls`: `engine` on a call with no after-image at its key or on another tool's call; changing or clearing a kind once set.
- Audit `row.written`: the target is `row:<table_id>/<row_id>`; the args are `op`, `version`, `step_run_id`, `call_seq`, `taint` (`1`/`true` or `0`/`false`) and `columns`. No row data, no hash of it, no `*_sha256` key.
- Doctests (doc-comment lines, written by the tester):
  - on `StepInput`: `compile_fail` for `i.from_row`, beside an allowed read of its five fields (3.6c removes both);
  - on `ToolCallRow`: an allowed read of `result_kind`, `result_label` and every `RowReceipt` field, and a `compile_fail` for `r.data`.

**am-engine** (literal only): `StartAttempt { .., writes: AttemptWrites { seat, table: None, document: None } }`.

**xtask** (`check-deps`):
- Row 48: `apply_row_write` in shipped code outside `crates/am-store/src/` fails. That includes `crates/am-engine/src/row_writer.rs` until the ToolGate slice widens the rule (its patch removes that planted case). Each finding names `file:line` of the token, `apply_row_write` and `AD-21`.
- Row 49: a call of `.rows(`, `.row(`, `.row_history(` or `.row_write(` in shipped code outside `crates/am-store/src/` fails. This covers a call split over lines, spaced around `.`, or made on a `use … as` alias of `Reader`. Each finding names `file:line` and the reader. A call of `.row_receipt(` passes. The shipped tree has none.

**Checked against stubs:**
- I used a scratch tree, which was git-archived from HEAD and given its own target directory.
- For each of the four states in build order (this slice; then each patch applied), I added signature-only stubs of that slice's API on top of the earlier slices' stubs. Each state passed:
  - `cargo check --workspace --all-targets`;
  - `cargo clippy --all-targets -D warnings`, with no finding in a test file;
  - `cargo test --doc -p am-store -p am-domain`.
- So a test that does not compile after a slice's build means that build differs from its plan's API list.

### Red run of the whole 3.6 (2026-11-24, before the re-slices; superseded)

*Superseded by "Red run (store slice, 2026-11-24)" below. Kept as the record of the first red run, over all of 3.6's tests at once; the owner's re-slices ask for one red run per slice.*

No `src/` change but doc-comment lines (`crates/am-domain/src/content.rs` module docs, `crates/am-domain/src/lib.rs` crate docs, `crates/am-store/src/lib.rs` on `Writer::resolve_inbox_item`). Every command ran from the repository root with `CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager`: `cargo nextest run -p <crate> --test <target>` for each target, `cargo test -p xtask --test check_deps`, `cargo test --doc -p am-store` and `cargo test --doc -p am-domain`. A compile failure names the first missing item.

| Target | Tests | Red reason |
|---|---|---|
| am-store `rows` (new) | 22 | does not compile: `E0432 unresolved import am_domain::rows` (then `am_store::{AttemptWrites, RowRecord, RowHistoryRecord, DocumentVersionRecord}`, `Writer::apply_row_write`, the readers, `SettleAndFinishDocumentStep`, `StoreError::RowRefused`) |
| am-store `prompt_journal`, `inbox_resolve`, `run_journal`, `inbox` | literals; `prompt_journal` 2 asserts superseded, `inbox_resolve` 1 test superseded | do not compile: `E0422 cannot find struct AttemptWrites in crate am_store` (then `StartAttempt.writes`, `StepInput.from_row`) |
| `cargo test --doc -p am-store` | +1 `compile_fail` (seat dismissal) | 8 passed: the new `compile_fail` is green on arrival (an `Actor` is not a `HumanActor`, today and after) |
| `cargo test --doc -p am-domain` | +2 allowed builds, +5 `compile_fail` | 17 passed, 2 failed: the allowed builds, `E0432 unresolved import am_domain::EngineResult` (content.rs module docs) and `am_domain::rows` (lib.rs crate docs). The five `compile_fail` blocks are green on arrival only because the type does not exist yet; the stub check above shows they fail for the right reason once it does |
| am-engine `toolgate` | 11 (+7 new; the 4 existing moved to the new `over`, `admit` and `run`) | does not compile: `E0432 unresolved import am_domain::rows` (then `am_domain::EngineResult`, `am_engine::toolgate::{CallPlace, GateError, GateOutput, StepTools}`) |
| am-engine `row_writer` (new) | 2 | does not compile: `E0432 unresolved import am_domain::rows` (then `am_engine::row_writer`, `am_store::AttemptWrites`) |
| am-engine `loop_caps`, `run` | literals; 1 test superseded in each | do not compile: `E0560 struct StepPlan has no field named grants` |
| am-tools `table_write` (new) | 4 | does not compile: `E0432 unresolved import am_domain::rows` (then `am_tools::table_write`, `serde_json`: the implementer adds am-tools' `serde_json` edge) |
| am-config `step_writes` (new) | 6 | does not compile: `E0609 no field table on type am_config::Step` (and `document`) |
| am-providers `labels` | +1 (both adapters) | does not compile: `E0432 unresolved import am_domain::rows`; `no variant named EngineResult found for enum Part` |
| xtask `check_deps` | +4 | 30 pass, 4 fail: `only_toolgate_calls_the_row_writer` "another am-engine file: must fail:", `the_store_row_write_is_named_only_by_the_engines_row_writer` "am-app: must fail:", `only_toolgate_builds_an_engine_result` "the engine's driver: must fail:" (each at `xtask/tests/check_deps.rs:1321`: the planted forms pass today; the allowed trees already pass, so no existing rule trips on them), `the_shipped_tree_passes_the_three_rules` "crates/am-domain/src/rows.rs must exist (No such file or directory (os error 2)); story 3.6 adds it" |
| am-app `run_rows` (new) | 3 | does not compile: `E0432 unresolved import am_domain::rows` (then `am_tools::table_write`, `Reader::rows`) |
| am-app `gate_cli` | +2 (Draft listed and dismissed; Draft takes no approve); literals | does not compile: `E0422 cannot find struct AttemptWrites in crate am_store` |
| am-app `gate_tty` | literal | does not compile: same |
| am-app `run_cli` | 32 (1 superseded) | compiles; `a_tool_request_the_seat_is_not_granted_fails_the_run_in_toolgates_words` fails at `run_cli.rs:1117` (the words are story 3.1's "tools are not available", not ToolGate's "was refused … not granted"); 22 others passed before nextest stopped |
| am-e2e engine `rows_table_write` / `rows_idempotent` / `documents_versions` (new) | 6 / 3 / 5 | do not compile: `E0432 unresolved import am_domain::rows` (support/mod.rs; then `am_domain::EngineResult`, the store records) |
| am-e2e trust `capabilities_tables_grant` / `taint_rows_declared_table` / `taint_rows_at_rest` / `taint_tool_result_engine_values` / `taint_rows_reader` / `taint_documents_untrusted_markdown` / `rule_16_row_provenance` (new) | 3 / 3 / 3 / 3 / 3 / 2 / 4 | do not compile: same |
| am-e2e trust `inbox_resolve_human_only` | +1 (`a_draft_cannot_be_dismissed_by_a_non_human`) | does not compile: same |
| am-e2e `handoff_next`, `handoff_loop_back`, `handoff_gate`, `handoff_gate_resolve`, `prompt_assembly_journal`, `schema_repair`, `rule_15_inbox`, `taint_handoff_chain`, `taint_prompt_type_wall`, `taint_trusted_input_sources`, `taint_trusted_input_gate_note`, `taint_prompt_fence` | literals (support's `step`/`plan`, `from_row`, `writes`; `taint_handoff_chain`'s step granted `FixtureTool::NAME`) | do not compile: `E0432 unresolved import am_domain::rows` (support/mod.rs) |
| am-e2e `rule_16_audit_log`, `taint_at_rest` | literals | do not compile: `E0432 unresolved import am_store::AttemptWrites` |
| am-e2e `trust_targets` | 3 | green on arrival: the ten new `[[test]]` targets are listed |
| am-e2e `taint_trusted_input_refusals` | 3 | green on arrival (unchanged) |

Also run, all clean with the new files: `cargo fmt --all -- --check`, `cargo xtask denylist`, `runtime-paths`, `fixed-hosts`, `seam-guard`, `check-forbid`, `check-deps` (the shipped tree). `agentic validate --template` on a copy of `fixtures/projects/rows-documents` with the two new keys removed: 0 errors, 0 warnings; with them, the current binary reports `unknown key 'table'` and `unknown key 'document'` (red until am-config reads them).

**Size (owner Q9):** the tests alone are about 8,000 lines (about 5,700 in new files, about 2,200 inserted in changed ones), before any `src/` line. The plan's estimate is 5,000 to 6,500 changed lines in all, so the story will overrun it; the manager decides per Q9. *(re-slices: the owner re-sliced on this; each slice's code and test estimates are in its Size section.)*

### Red run (store slice, 2026-11-24)

**The split (tester, 2026-11-24).** I split the tests written for 3.6 by slice and by matrix row:
- The working tree on `story/3-6a` holds only this slice's tests.
- The three later slices' tests are patches in `.git/wip/`, each in build order. Each applies on this slice's tests plus the earlier patches:
  - `tests-toolgate.patch`: 27 files, +4,426/−74 lines;
  - `tests-rows-reaching.patch`: 23 files, +634/−63 lines;
  - `tests-documents.patch`: 18 files, +1,695/−84 lines.
- I checked the chain on a temporary index (`GIT_INDEX_FILE`; `git read-tree HEAD`, this slice applied, then `git apply --cached --check` and `--cached` for each patch in order):
  - every patch checks clean in order;
  - the final index equals the full re-sliced test set, file for file;
  - nothing was recorded in git, and the real index is untouched.

Each later slice's tester applies its patch, adds the new tests its plan lists that 3.6 never wrote, and logs its own red run:
- ToolGate slice: row 40's fan-out tests, `engine_result_journal.rs`, `taint_rows_no_read_path.rs`, the row-43 rules, and the `RowInput` and `row_inputs` tripwires;
- 3.6c: the 0011 migration tests, rows 46 and 47;
- 3.6b: the 0012 migration tests.

How the straddling tests were cut:
- **Store `rows.rs`.** It loses three things:
  - `a_row_input_edge_carries_the_rows_taint` goes to 3.6c's new `crates/am-store/tests/row_inputs.rs`;
  - the four document tests go to 3.6b's new `crates/am-store/tests/documents.rs`, along with the Draft-dismissal test and the row-39 guard. The version-and-Draft test's audit half there becomes `a_document_audit_holds_ids_never_markdown` (row 22);
  - 3.6c's patch removes `there_is_no_row_input_kind_and_no_from_row_column`, and 3.6b's patch removes `a_document_value_is_refused_until_3_6b` (supersessions).
- **`taint_rows_at_rest.rs`.** The document test moves to 3.6b's `taint_documents_at_rest.rs`, with its own `[[test]]` target.
- **`rule_16_row_provenance.rs`.** It runs rows and a plain step in the ToolGate slice; 3.6b's patch restores the document step and the document halves (row 22).
- **`step_writes.rs`.**
  - The ToolGate slice has the table tests and `a_gate_step_declares_no_table`. It drops the "table on a fan-out step" error case, because ToolGate row 40 makes that a warning.
  - 3.6b adds `a_step_declares_its_document`, the document mixed-declaration test, `a_fan_out_step_declares_no_document` and `one_document_belongs_to_one_team`.
- **`run_rows.rs`.** The ToolGate slice has `plan_of_maps_grants_team_table_and_the_row_tool`; 3.6b adds `plan_of_maps_the_document_step` and the third step to the offered-tools test.
- **The fixture.** `rows-documents` has no `summarise` step until 3.6b's patch.
- **Literals.**
  - `from_row: None` and `row_inputs: vec![]` move to 3.6c's patch.
  - The `StepPlan`, `RunPlan` and support helper fields move to the ToolGate patch.
  - 3.6c's patch adds the `reading` helper, and 3.6b's patch adds the document helpers.
  - `writes: AttemptWrites { .. }` stays here.
- **Doctests.**
  - The `EngineResult` doctests (`content.rs` module docs) move to the ToolGate patch.
  - The seat-dismissal `compile_fail` moves to 3.6b's patch, and 3.6c's patch removes this slice's `from_row` tripwire.
- **xtask.** The ToolGate patch widens this slice's row-48 tests to `row_writer.rs` and renames 3.6's `the_shipped_tree_passes_the_three_rules` to the plan's `the_shipped_tree_passes_the_rules`.
- **`row_writer.rs`'s first test.** It is renamed `the_store_row_writer_reads_its_result_from_the_receipt` and reads `Reader::row_receipt`.

**New store tests, written now:**
- `a_tainted_attempts_row_stores_its_taint` (row 12): a tainted start, a taint raised by a later call, a tainted command and a clean write.
- `a_document_value_is_refused_until_3_6b` (row 28).
- `there_is_no_row_input_kind_and_no_from_row_column` (row 42).
- Four row-44 tests: `a_succeeded_write_journals_an_engine_result_kind`, `an_engine_kind_needs_its_after_image_and_is_set_once`, `finish_tool_call_journals_untrusted` and `the_receipt_carries_no_row_data`.
- Additions to existing tests:
  - row 2: the kind and the receipt;
  - row 4: no kind on the stale call;
  - row 15: the receipt is the first write's;
  - row 19: no kind after a forced failure.
- The migration tests check 0010's own columns. Their "no `from_row`, no `document_versions`" checks run on a database made at exactly version 10, so no later slice has to supersede them.
- xtask (rows 48 and 49): `the_row_write_command_is_named_only_in_the_store`, `the_shipped_tree_passes_the_row_write_rule` and `no_shipped_code_reads_row_content`.

**Commands.**
- Every command ran from the repository root with `CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager`, after `cargo fmt --all`:
  - `cargo nextest run -p <crate> --test <target> --no-fail-fast`;
  - `cargo test --doc -p am-store`;
  - `cargo test --doc -p am-domain`;
  - `cargo test -p xtask --test check_deps`.
- The only `src/` changes are doc-comment lines: `crates/am-domain/src/lib.rs` crate docs, and `crates/am-store/src/lib.rs` on `StepInput` and `ToolCallRow`.

| Target | Tests | Red reason |
|---|---|---|
| am-store `rows` (new) | 23 | does not compile: `E0432 unresolved import am_domain::rows` (then `am_store::{AttemptWrites, RowHistoryRecord, RowRecord}`, `Writer::apply_row_write`, the readers, `row_receipt`, `ToolCallRow.result_kind`, `StoreError::RowRefused`) |
| am-store `prompt_journal` | 2 asserts superseded (the 8→9 step is applied, not last); literals | does not compile: `E0422 cannot find struct AttemptWrites in crate am_store` |
| am-store `inbox_resolve`, `run_journal`, `inbox` | literals (`writes`) | do not compile: same |
| `cargo test --doc -p am-store` | +2 allowed, +2 `compile_fail` | 10 passed, 1 failed: the `ToolCallRow` allowed read, `E0425 cannot find type RowReceipt in crate am_store`. Green on arrival: the `StepInput` allowed read (its five fields exist today), the `from_row` `compile_fail` (no such field, today and after this slice) and the `RowReceipt.data` `compile_fail` (green only because the type does not exist yet; the stub check shows it fails for the right reason once it does) |
| `cargo test --doc -p am-domain` | +1 allowed | 1 failed: `E0432 unresolved import am_domain::rows` (crate docs, the `RowWrite` build) |
| xtask `check_deps` | +3 | 30 pass, 3 fail: `the_row_write_command_is_named_only_in_the_store` "the engine's driver: must fail:" and `no_shipped_code_reads_row_content` "the engine: must fail:" (each at `xtask/tests/check_deps.rs:1293`: the planted calls pass today, and the allowed trees already pass); `the_shipped_tree_passes_the_row_write_rule` "am-store defines the row command" (`:1430`) |
| am-app `gate_cli`, `gate_tty` | literals | do not compile: `E0422 cannot find struct AttemptWrites in crate am_store` |
| am-e2e `handoff_gate_resolve` | literal | does not compile: same |
| am-e2e `rule_15_inbox`, `rule_16_audit_log`, `taint_at_rest`, `taint_prompt_type_wall`, `taint_trusted_input_sources` | literals | do not compile: `E0432 unresolved import am_store::AttemptWrites` |
| am-e2e `trust_targets` | 3 | green on arrival (no new trust or engine file in this slice) |

Also clean with this slice's files: `cargo fmt --all -- --check`, `cargo xtask denylist`, `runtime-paths`, `fixed-hosts`, `seam-guard`, `check-forbid` and `check-deps` on the shipped tree.

**Not written, and why.** The plan says, without naming it, that the writer refuses a non-Seat actor and that this is reached by a store test. That test is `the_writer_refuses_a_write_that_is_not_its_attempts`, whose engine and person cases cover it. Nothing else of this slice's list is missing.

**Size.** This slice's tests come to about 2,600 lines:
- about 2,000 in `rows.rs`;
- about 330 xtask lines;
- about 230 literal lines;
- about 60 doc lines.
That is at the top of the plan's 2,300 to 2,600.
