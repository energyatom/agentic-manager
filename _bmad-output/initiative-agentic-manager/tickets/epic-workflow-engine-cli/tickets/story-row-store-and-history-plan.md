---
title: 'Row store and history'
type: feature
ticket: 6
created: '2026-11-24'
status: built
baseline_revision: '8a305a3cca78154ccc2bade96435be0cdbd1b788'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [correctness, adversarial-security]
review_loop_iteration: 2
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
  - `table_rows_of(&self, table_id: &str) -> Vec<RowRecord>`, in creation order;
  - `table_row_by_id(&self, table_id: &str, row_id: &str) -> Option<RowRecord>`;
  - `row_history_of(&self, table_id: &str, row_id: &str) -> Vec<RowHistoryRecord>`, in version order;
  - `row_written_at(&self, step_run_id: &str, call_seq: i64) -> Option<RowHistoryRecord>`;
  - `row_receipt(&self, step_run_id: &str, call_seq: i64) -> Option<RowReceipt>`;
  - `attempt_writes(&self, step_run_id: &str) -> Option<AttemptWrites>`, which is `None` for an old attempt.
  The first four read row content and are test-facing (row 49). *(Review triage round 1, R4: renamed from `rows`, `row`, `row_history` and `row_write` to these names, which are unique in `crates/` (tester's choice). `row_receipt` and `attempt_writes` keep their names.)*
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
- Row 49 (R4, R13): `table_rows_of`, `table_row_by_id`, `row_history_of` or `row_written_at`, named anywhere in shipped code outside `crates/am-store/src/`, fails. It is matched as a bare token over the whole file. That covers a method call; a call split over lines or spaced around `.`; a call on a `use … as` alias of `Reader`; a function value (`let f = Reader::<name>`); `.map(Reader::<name>)`; a turbofish; `r#<name>`; and a path call. Each finding names, on one line, `file:line` and the exact reader. A call of `.row_receipt(` passes. The shipped tree has none.

**Review triage round 1 (tester, 2026-11-24): what the round-1 tests add to this API.**
- **Readers (R4).** `table_rows_of`, `table_row_by_id`, `row_history_of` and `row_written_at`, as listed above.
- **Caps (R10)** *(tester's choice: named constants the implementer sets)*:
  - `pub const ROW_MAX_BYTES: usize` and `pub const ROW_MAX_COLUMNS: usize`, exported at the am-store root;
  - the byte cap applies to the row's stored JSON text after the write, so a minified insert of exactly `ROW_MAX_BYTES` bytes is written and one byte more is refused;
  - an object of exactly `ROW_MAX_COLUMNS` keys is written and one more is refused;
  - an update that grows a row past either cap is refused;
  - refusals are `Err`, with words holding `byte` or `column`, never the data, and nothing written.
- **Recursive triggers (R3).** `am_store::testing::configured_pragma(path: &Path, name: &str) -> Result<i64, StoreError>` (test-support): it opens `path` the way the store opens its connections and returns `PRAGMA <name>`. `recursive_triggers` and `foreign_keys` are 1.
- **Trigger words** (tester's choice where the guard is new). Each test asserts the error text holds:
  - R2: `a tool call with a result kind never changes`. Once `result_kind` is set, it aborts a change of `id`, `step_run_id`, `call_seq`, `tool`, `status` or the rowid, `UPDATE OR REPLACE` included. It covers an `engine` and an `untrusted` call alike.
  - R3: `a row is never re-created`. It aborts an insert of a row whose history goes beyond version 1.
  - R5: `an after-image never lowers its row's taint`.
  - R6: `an attempt's writes are set only when it starts`. It aborts a change of `seat` or `writes_table` once the attempt is past `queued`.
  - Existing guards, now reached on their own (R1):
    - `table, id and creation time never change`;
    - `table_rows.taint only rises`;
    - `an engine result answers only a table_write call whose row was written`;
    - `result kind is set once`;
    - `a row is never replaced`.
- **Actor check (R5).** A raw after-image whose actor is `{}`, `"seat"`, `[1]`, or a seat with no provenance, is refused. The message is not pinned.
- **Blank seat (R7).** `AttemptWrites { seat: "" }` is journaled as a NULL seat, and `attempt_writes` gives `None`. The spike's attempts get `None` too.
- **Closed run (R8).** `apply_row_write` into a run whose status is not open (`cancelled`, `succeeded`, `failed`) is refused. The words hold the run's id; nothing changes and the call stays `issued`.
- **Numbers as written (R9).** An insert's data text is stored exactly as written (minified input). The tests use `{"ref":12345678901234567890123}` and `{"x":1e2}`. An update merging `{"y":2}` keeps `"ref":12345678901234567890123`, and the after-image equals the row.
- **Debug (R11).** `Debug` of `RowOp` and `RowWrite` holds no key or value of the data or the patch. It does hold the JSON text's byte length.
- **Table words (R12).** A refusal for a table that is not the attempt's names the journaled table and never the caller's table id.

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

### Implementation (store slice, 2026-11-24)

*Implementer, on Opus. No test file was changed; no dependency or feature was added; nothing was recorded in git.*

**Built:**
- **am-domain** `src/rows.rs` (new, `pub mod rows`): `RowOp { Insert, Update }` and `RowWrite`, owned types, as the API list fixes them.
- **am-store** migration `0010_rows.sql`:
  - `table_rows` and `row_history` with the plan's columns;
  - `step_runs.seat` and `.writes_table`;
  - `tool_calls.result_kind` (CHECK `engine`, `untrusted`) and `.result_label`;
  - the plan's triggers: `row_history_no_update`, `_no_delete`, `_never_replaced` (`seq`, version key, call key); `table_rows_no_delete`, `_never_replaced` (key or rowid), `_fixed_fields`, `_version_steps`, `_taint_only_rises`, `_insert_needs_after_image`, `_update_needs_after_image`; `step_runs_writes_set_once`; `tool_calls_engine_result_needs_its_write`, `_result_kind_set_once`.
- **am-store** `src/rows.rs` (new): `Writer::apply_row_write`'s body (`Command::Row`, not a `WriteCommand`), in one immediate transaction with checks in the plan's order:
  1. the replay is `Ok`, with nothing written;
  2. the actor is the attempt's own `Actor::Seat` (its run, its step, the journaled seat);
  3. the attempt is `running`;
  4. the call at the key is an `issued` `table_write` call with this id;
  5. the table is the journaled `writes_table`;
  6. an insert needs a new row id, and an update needs the row at the current version (`RowRefused`).

  Then the after-image, the row, the call `succeeded` with `engine` / `tool:table_write`, and `row.written`. The module also holds `RowRecord`, `RowHistoryRecord`, `RowReceipt` and the readers `rows`, `row`, `row_history`, `row_write`, `row_receipt` and `attempt_writes`.
- **am-store** `src/lib.rs`:
  - `StoreError::RowRefused`;
  - `AttemptWrites`, and `StartAttempt.writes` journaled by `start_attempt` (`StartStep` and a gate's `PauseStep` journal none);
  - a `document` value refused in words naming 3.6b;
  - `FinishToolCall` sets `untrusted` / `'tool:' || tool`;
  - `ToolCallRow.result_kind` and `.result_label`;
  - module docs.
- **am-engine**: the two `StartAttempt` literals.
- **xtask** `deps.rs`: `row_findings`, over whole-file code (comments and literals stripped):
  - row 48: the token `apply_row_write` outside `crates/am-store/src/`, naming `file:line`, the owner's condition and AD-21;
  - row 49: a call of `rows`, `row`, `row_history` or `row_write` after `.` or `::` and before `(`, with whitespace and line breaks allowed, naming `file:line` and the reader.

**Choices the plan left open:**
- **The spike's linear executor** (`am-engine/src/journal.rs`) has no seat; its `StepRow` has no `seat` field. It journals `seat: ""`, with `table: None` and `document: None`. So `attempt_writes` gives `Some` with an empty seat. `StartAttempt` refuses a table that is empty or holds a control character, and a table with an empty seat.
- **Row data is normalised.** The writer stores data as serde_json's compact text: keys sorted, since `preserve_order` is not enabled. The insert's text and its after-image's text are byte-equal.
- **Updates merge top-level keys.** A `null` in the patch removes the column.
- **`columns`** (in the receipt and in `row.written`) is the row's column count after the write. For an insert, that is the number of data keys. The receipt counts it in SQL (`json_each`), so `row_receipt` never loads the data.
- **`row.written` args:** `op`, `version`, `step_run_id`, `call_seq`, `tool_call_id`, `taint` (0 or 1) and `columns`. The target is `row:<table_id>/<row_id>`. There is no hash.
- **Refusal types.** Only the stale-version and unknown-row refusals are `RowRefused`. Every other refusal is `Invalid`: "refused: …; nothing was changed", naming store and config ids only. A refusal never echoes the data, the patch or the tool name. An unknown row's words never hold its id.
- **Extra guards.** These only narrow; none is in the plan's trigger list.
  - Triggers:
    - `row_history_next_version`: an after-image is the current version + 1, so it is written before the row.
    - `tool_calls_result_kind_not_on_insert`.
    - `tool_calls_result_kind_on_finish`: the kind and label are set together, and only on a `succeeded` or `failed` call.
    - `tool_calls_never_replaced`: by id or by (step run, call sequence); without it, an `INSERT OR REPLACE` on 0007's table would clear a kind that is set once. It is the 3.5 lesson. No code or test depends on the old conflict error.
  - CHECKs:
    - `data` is a JSON object, and `actor` is valid JSON;
    - the provenance columns are all set or all NULL;
    - the actor is a seat exactly when the provenance is set;
    - `op = 'insert'` exactly when `version = 1`.
- **Row 49's rule also catches `::name(`** (a path call such as `Reader::rows(&r, …)`). A use path such as `am_domain::rows::RowWrite` is not a call, so it passes.

**Mechanical test edits:** none.

**Mis-specified test: `xtask/tests/check_deps.rs` `no_shipped_code_reads_row_content`, the case "spaced around the dot".**
- Its planted source line is `reader . row_write ("s", 1)`, but its markers are `row_write(` and `Reader as`.
- `lines_holding` therefore finds no line, and `assert!(!lines.is_empty(), …)` (`:1295`) panics before check-deps runs. That happens with any implementation.
- I did not change it. A fix changes the test's input or assertion: for example, write the call as `reader . row_write("s", 1)`, or match the marker with whitespace normalised.
- Run by hand on the same planted tree (`cargo run -p xtask -- check-deps --metadata xtask/tests/fixtures/metadata/clean.json --root <tree>`), the rule fails it, naming `crates/am-engine/src/scheduler.rs:2` and `row_write`.
- The two parts the panic skips pass when run by hand or checked:
  - the alias case fails, naming `crates/am-app/src/gate.rs:3`, which holds `rows(`;
  - the shipped-tree scan is clean: no `.rows(`, `.row(`, `.row_history(` or `.row_write(` outside am-store, and the real workspace's `cargo xtask check-deps` passes.
- Every earlier case of that test passes.

**Deferred:** nothing of this slice. The ToolGate slice still widens row 48 to `crates/am-engine/src/row_writer.rs`.

**Size.** Added code is about 924 lines, counting the SQL and the doc comments:
- `0010_rows.sql`: 173 lines;
- am-store `src/rows.rs`: 503 lines;
- am-store `src/lib.rs`: about 122 lines, not counting the tester's 33 doc lines;
- `src/migrations.rs`: 1 line;
- am-domain `src/rows.rs`: 43 lines, plus 1 line in `lib.rs`;
- am-engine: 20 lines;
- xtask `deps.rs`: 61 lines.

That is inside the plan's 900 to 1,200 and far under the ~4,000-line stop. The tests are the tester's, about 2,600 lines, unchanged.

**Verification:**
- `cargo nextest run -p am-store --test rows`: 23 of 23 pass.
- The am-store, am-engine, am-domain, am-app, am-e2e and am-server suites: 917 of 917 pass.
- `cargo test --doc -p am-store -p am-domain`: green (am-store 4 + 7, am-domain 5 + 8).
- `cargo test -p xtask --test check_deps`: 32 pass, 1 fails (the mis-specified test above).
- `cargo clippy --all-targets -D warnings`: clean.
- These `just check` recipes are green: `fmt-check`, `doctest`, `check-deps`, `check-forbid`, `denylist`, `runtime-paths`, `fixed-hosts`, `seam-guard` and `validate`.
- One slip: an early `cargo clippy --workspace` also built `am-desktop`. Nothing was cleaned.

**Final `just check`:** red only on the mis-specified test. `cargo nextest run --no-fail-fast` gives `1429 tests run: 1428 passed, 1 failed, 8 skipped`. `just check` itself stops at that failure: `Summary [55.176s] 1353/1429 tests run: 1352 passed, 1 failed, 8 skipped` (`xtask::check_deps no_shipped_code_reads_row_content`).

**Test correction (tester, 2026-11-24):** `no_shipped_code_reads_row_content`'s "spaced around the dot" case plants `reader . row_write ("s", 1)`, with a space before the paren. The test looked for its planted line with the marker `row_write(`, found none, and panicked before check-deps ran, whatever the rule did.

- **The fix:** the test now finds a planted line by the reader's call with spaces taken out (`.row_write(`), or by the `Reader as` alias, and passes those exact lines on.
- **What still holds:** the finding must name `file:line` of a planted line and the reader. No case or assertion was dropped.
- **Result:** `cargo test -p xtask --test check_deps` gives 33 passed, 0 failed against the implemented rule.
- **Patches:** the ToolGate patch's xtask hunks still apply.

### Review triage round 1 (manager, 2026-11-24)

Two Opus reviews ran on the working tree over 0824ab9: correctness (C) and adversarial security (S). Neither found a way for the row command to lower taint, rewrite history or leak row content. Every hole needs raw SQL inside am-store, or a gap in check-deps row 49. The fixes are below, all done tests first.

| Id | From | Finding | Decision |
|----|------|---------|----------|
| R1 | C1 | Three tests pass for the wrong reason: row 13's taint UPDATE, row 18's `table_id`/`created_at` UPDATEs and row 44's "engine on …" cases are aborted by other triggers, so the trigger each test names is never reached. | The tester fixes the tests. Each one sets up a state where only the named trigger can abort (a matching after-image at v+1; a finished call), and asserts that trigger's message. |
| R2 | C2, S3 | Once a call's result kind is set, a raw UPDATE can still change its `tool`, `status`, `step_run_id`, `call_seq` or `id`, or move its rowid. `UPDATE OR REPLACE` on `call_seq` deletes another call and clears its kind. | A BEFORE UPDATE trigger aborts any change to `id`, `step_run_id`, `call_seq`, `tool` or `status` when `OLD.result_kind IS NOT NULL`; a rowid change is aborted; `recursive_triggers` is on (R3). Tests for each. |
| R2b | tester, 2026-11-24 | The reverse collision: an unkinded call `UPDATE OR REPLACE`d onto a kinded call's `call_seq` deletes the kinded call (no BEFORE DELETE, D1). | Fix: `id`, `step_run_id`, `call_seq` and `tool` are fixed for every call, not only kinded ones (none changes on any legitimate path; finishing a call sets only status and result). Test: the reverse collision aborts and the kinded call stays. |
| R3 | S1 | Rowid collision: `UPDATE OR REPLACE table_rows SET rowid=<R>` deletes row R without firing `table_rows_no_delete` (REPLACE deletes skip triggers while `recursive_triggers` is off). R can then be re-inserted clean at v1, lowering its taint and rolling back its version. | Turn `PRAGMA recursive_triggers=ON` on every store connection; check every existing trigger still behaves, and stop and report if any conflicts. Abort any rowid change on `table_rows`. Allow an insert only for a row with no history beyond its v1 after-image (a row is never re-created). Tests: the exact attack aborts; a REPLACE by rowid aborts. |
| R4 | C4, S2 | Row 49 is bypassed by a function value (`let f = Reader::rows; f(..)`, `.map(Reader::row_history)`), a turbofish or `r#`. It also matches by name with no receiver, so it would hit unrelated `.row(` calls later. | Give the row-content readers names unique in `crates/` (the implementer picks them, as AD-21 already requires of the port's method and the row command). Match them as bare tokens over the whole file, as row 48 does. The tester updates every test and the three `.git/wip` patches that use the old names. Tests: every bypass form fails; the shipped tree passes. |
| R5 | C5, S4 | `row_history` accepts an after-image with a lower taint than its row. Its actor CHECK passes when the value is NULL (`{}`, `"seat"`, `[1]`). | Refuse `NEW.taint` below the row's current taint. Require `json_type(actor)='object'` and test the kind with `IS`. Tests for each. |
| R6 | C6 | An attempt's seat and table can be set by raw UPDATE after it has started. | Set them only on the queued→running transition; abort when `OLD.status <> 'queued'`. Test. |
| R7 | C7 | The spike executor journals an empty seat, so `attempt_writes` returns `Some("")`, and the set-once trigger locks it. | Journal NULL for a blank seat. Test. |
| R8 | C8, S6 | A row can be written into a cancelled or finished run, because only the step's status is checked. | `write_row` refuses when the run is not open (`run_open_for`), in words. Test. |
| R9 | C3 | Row data is normalised through serde `Value`, so big integers lose precision and `1e2` becomes `100.0`. The after-image is then not what the seat wrote. | Store the insert's text through SQLite `json(?)`, keeping each number as written, and merge an update's patch in SQL (`json_set` and `json_remove` per top-level key). Tests: a big integer and `1e2` round-trip as written. If the bundled SQLite does not keep them, stop and report. |
| R10 | S7 | No cap on row size or column count, and each update re-stores the whole row. | A byte cap and a column cap, refused in words. The implementer picks values within the architecture and records them. Test at the boundary. |
| R11 | S8 | `RowOp` and `RowWrite` derive `Debug` and print row data in full. | Write `Debug` by hand, showing only the column count and byte length. Test. |
| R12 | S9 | A refusal repeats the caller's table id verbatim; from the ToolGate slice on, that could be model text. | Refusal words name only the journaled table. Test. |
| R13 | C9 | Two weak xtask assertions: `err.contains(reader)` is always true for `"row"`, and the path-call form is built but never tested. | The tester strengthens both (folded into R4's tests). |

**Accepted or deferred, with reasons:**
- **D1 (S3, part):** no BEFORE DELETE trigger on `tool_calls`. AD-25 requires that deleting a run really deletes its content, the same reason as 3.4's D2.
- **D2 (S, informational):** `Actor::Seat` carries no attempt id, so concurrent attempts of one step under a future fan-out cannot be told apart by actor. This goes to 3.14's plan.
- **D3 (S):** an identifier-pasting proc-macro could hide a name from check-deps. None is in `Cargo.lock`, and adding one is an owner decision on a new dependency.

### Red run round 1 (store slice, 2026-11-24)

The tester's response to review triage round 1, tests first. No `src/` line changed and no dependency changed. I ran no `--workspace` build. `cargo fmt --all` passes.

**Commands, on the checkout** (`CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager`):
- `cargo nextest run -p am-store --test rows`: does not compile. The first missing items are `E0425 cannot find function configured_pragma in module am_store::testing`, then `ROW_MAX_BYTES`, `ROW_MAX_COLUMNS` and the four renamed readers.
- `cargo nextest run -p am-engine --test journal`: 1 of 3 fails, `a_spike_attempt_journals_no_seat` (left `Some(AttemptWrites { seat: "", .. })`, right `None`).
- `cargo test -p xtask --test check_deps`: 32 pass, 1 fails, `no_shipped_code_reads_row_content`, with "a call in the engine: must fail:". Today's rule matches the old names only.

**Each test's own red reason.** These come from a scratch copy of the checkout with its own target directory, after which I deleted the copy. The shim did three things only:
- the four new reader names, delegating to today's readers;
- placeholder caps (65,536 bytes, 256 columns);
- a `configured_pragma` that reads the pragma after the store's `configure`.

Command: `cargo test -p am-store --test rows`. Result: 25 pass, 15 fail. Clippy `-D warnings` on the am-store, am-engine and xtask test targets is clean.

| Row | Test | Red reason (today's build) |
|---|---|---|
| R1 | `a_rows_table_id_and_creation_time_never_change` (new) | green on arrival. It now reaches `table_rows_fixed_fields` alone: a person's after-image at v+1, then a version+1 update naming the field. The control update goes through |
| R1 | `a_rows_taint_never_falls_by_its_own_guard` (new) | "a lower-taint after-image: must abort, changed 1 row(s)". That is R5's missing guard; the taint guard's own half runs once it is there |
| R1 | `each_result_kind_guard_aborts_with_its_own_words` (new; replaces `an_engine_kind_needs_its_after_image_and_is_set_once`) | green on arrival. Each case now reaches its named guard: the calls are finished by hand first, and a renamed label reaches set-once alone |
| R1 | `row_taint_only_rises`, `rows_and_history_are_append_only` | the cases that passed for the wrong reason move to the two tests above; the rest stay green |
| R2 | `a_tool_call_with_a_result_kind_never_changes` | "tool: must abort, changed 1 row(s)" |
| R3 | `the_rowid_collision_attack_aborts` | "the rowid collision: must abort, changed 1 row(s)". The exact attack succeeds today: a person's after-image for the other row, then `UPDATE OR REPLACE … SET rowid = <tainted row's>, version = version + 1` |
| R3 | `a_replace_by_rowid_aborts` | green on arrival. `table_rows_never_replaced` checks the rowid, now reached alone |
| R3 | `a_row_is_never_re_created` | "a re-created row: must abort, changed 1 row(s)" |
| R3 | `store_connections_turn_recursive_triggers_on` | `recursive_triggers` is 0 (left 0, right 1) |
| R5 | `an_after_image_never_lowers_its_rows_taint` | "taint 0: must abort, changed 1 row(s)" |
| R5 | `an_after_images_actor_is_an_object_with_a_kind` | "actor {}: refused, got Ok(1)" |
| R6 | `an_attempts_writes_are_set_only_when_it_starts` | "UPDATE step_runs SET seat = 'seat-late' …: must abort, changed 1 row(s)" |
| R7 | `a_blank_seat_is_journaled_as_none`, and the spike's `a_spike_attempt_journals_no_seat` | `Some(AttemptWrites { seat: "", table: None, document: None })`, not `None` |
| R8 | `a_row_is_never_written_into_a_finished_run` | "the run is not open: ()". The write succeeds into a cancelled run |
| R9 | `numbers_are_stored_as_written` | the row holds `{"ref":1.2345678901234568e+22}` |
| R10 | `a_row_over_the_byte_cap_is_refused`, `a_row_over_the_column_cap_is_refused` | "one byte over: ()" and "one column over: ()": no cap today |
| R11 | `a_row_writes_debug_holds_no_data` | `column_canary_7d21` is in `Insert { …, data_json: "{\"column_canary_7d21\":…" }` |
| R12 | `a_wrong_tables_refusal_never_echoes_its_id` | the words hold `not \`table-canary-0c41\`` |
| R4, R13 | xtask `no_shipped_code_reads_row_content` | rewritten for the four new names and every bypass form (function value, `.map(Reader::…)`, turbofish, `r#`, path call, `use … as`, split, spaced). It asserts one finding naming both `file:line:` and the exact reader. Red as above |
| R4 | every test that used the old names | renamed. The checkout's store tests are in `rows.rs`; the later slices' are in the patches |

**Patches.** I applied the round-1 changes to all four slice states and regenerated the three patches in `.git/wip/`:
- `tests-toolgate.patch`: 27 files, +4,434/−74 lines;
- `tests-rows-reaching.patch`: 23 files, +634/−63 lines;
- `tests-documents.patch`: 18 files, +1,695/−84 lines.

All three check clean in order on a temporary index (`git read-tree HEAD` and this checkout, with nothing recorded and the real index untouched), and the final state equals the full test set file for file. The slice-4 state, with every later slice's tests, type-checks and passes clippy on test files against the earlier stubs plus the reader-name shim.

**One case left out.** R2's finding also describes the reverse collision: an `UPDATE OR REPLACE` on a call *without* a kind, onto a call *with* one, deletes the latter. The decision's trigger only guards calls whose `OLD.result_kind` is set, so this case is not tested. The manager decides whether to add it.

**R2b, added by the tester on 2026-11-24 after the triage decided it.** New test: `a_tool_calls_identity_never_changes` in `rows.rs`.
- **The reverse collision.** `UPDATE OR REPLACE tool_calls SET call_seq = <kinded call's seq> WHERE id = <unkinded, issued call>` must abort, and the kinded call must stay exactly as it was. Today it already aborts, but only through `FOREIGN KEY constraint failed`: the kinded call's after-image references it, and the bundled SQLite turns foreign keys on by default. It is not stopped by a guard.
- **Plain updates.** A plain update of `id`, `step_run_id`, `call_seq` or `tool` on the unkinded, issued call must abort. Red today: "id: must abort, changed 1 row(s)". I took that reason from a scratch copy with the reader-name shim, which has since been deleted.
- **No words are pinned.** The decision names none for R2b.
- **Patches and lint.** All three patches were regenerated (the test is in every slice state). They still check clean in order on a temporary index, and the final state equals the full test set file for file. Clippy on `rows.rs` is clean.

### Implementation round 1 (store slice, 2026-11-24)

*Implementer, on Opus, against "Review triage round 1" and "Red run round 1". No test file was changed; no dependency or feature was added; no `--workspace` build; nothing was recorded in git.*

**Built, by triage row:**
- **R2, R2b** (0010, two triggers that split on `OLD.result_kind`, so one guard answers each change):
  - `tool_calls_identity_fixed`: on a call with no kind, a change of `id`, `step_run_id`, `call_seq`, `tool` or the rowid aborts with "a tool call's id, step run, call sequence and tool never change". Finishing a call (status, finish time, result) still passes.
  - `tool_calls_with_a_kind_never_change`: once a kind is set, the same fields plus `status`, `created_at` and `finished_at` are fixed. It aborts with "a tool call with a result kind never changes".
  - A change of the kind or the label itself still meets `tool_calls_result_kind_set_once`.
- **R3:**
  - `configure()` (every read-write connection: the project store, the user store and migrations) and `Reader::open` set `PRAGMA recursive_triggers=ON`. `testing::configured_pragma` opens a connection through `configure()` and reads the pragma.
  - New triggers on `table_rows`: `table_rows_rowid_fixed` ("a row's rowid never changes") and `table_rows_never_re_created` ("a row is never re-created": an insert whose row has history past version 1).
- **R4:** the readers are renamed `table_rows_of`, `table_row_by_id`, `row_history_of` and `row_written_at`. Row 49's rule now matches these four names as bare tokens over the whole file outside `crates/am-store/src/`, as row 48 does. Each finding names `file:line` and the exact name.
- **R5:**
  - `row_history_keeps_taint` aborts with "an after-image never lowers its row's taint".
  - The actor column's CHECK is now `json_type(actor) = 'object' AND json_type(actor, '$.kind') IS 'text'`, and the seat CHECK uses `IS 'seat'`.
- **R6:** `step_runs_writes_set_at_start` replaces `step_runs_writes_set_once`. It aborts any update of `seat` or `writes_table` unless that update takes the attempt from `queued` to `running` ("an attempt's writes are set only when it starts"). `start_attempt` now journals the writes in that same update.
- **R7:** a blank seat is journaled as NULL, so `attempt_writes` gives `None`; the spike's attempts get `None` too. A table still needs a seat that is not blank.
- **R8:** `write_row` calls `run_open_to(tx, &step, "writes a row")`. That is `run_open_for` with the action words passed in; its other callers keep "starts or waits". So a write into a finished or cancelled run is refused in words naming the run.
- **R9:**
  - An insert is checked with `json_valid` (strict RFC 8259) and stored as `json(?)`.
  - An update's patch is merged in SQL, one top-level key at a time: `json_set(data, fullkey, json(value))`, or `json_remove` for a `null`. Each key comes from `json_each(patch)`, and its value is read as JSON text with `patch -> fullkey`.
  - Number text is kept: the bundled SQLite 3.54.1 has the same JSONB code as the 3.51.0 I probed (`{"x":1e2}` and a 23-digit integer round-trip as written), and `numbers_are_stored_as_written` passes. Nothing to stop on.
- **R10:**
  - `pub const ROW_MAX_BYTES: usize = 65_536` (64 KiB), counted on the row's stored text after the write. Why: it is the size the config already allows a skill's body (`SKILL_BODY_MAX_BYTES`); a row is a record of columns, not a document; and every update stores the whole row again as an after-image, so the cap also bounds what one write adds to history.
  - `pub const ROW_MAX_COLUMNS: usize = 256` top-level keys. Why: it is far past what a config table declares, yet it bounds a write that spreads a model's output over thousands of keys.
  - The architecture names no row cap. Both refusals give sizes only (`bytes`, `columns`), never data.
- **R11:** `RowOp` and `RowWrite` have hand-written `Debug`. It shows `data_json_bytes` or `patch_json_bytes`, the row id, the version and ids. It shows no column count: am-domain may not parse JSON (AD-4: serde, thiserror, jiff and uuid only), and the test pins only the byte length.
- **R12:** after the table check, every refusal, the SQL and the audit target use the journaled table. A wrong table is refused as "writes only the table `<journaled>`, and this write names another".

**The `recursive_triggers` check:**
- Every trigger in migrations 0001–0010 and in the user store's 0001–0002 has a body that is only `SELECT RAISE(ABORT, …)`. None runs DML, so nothing can recurse.
- No store code uses `INSERT OR REPLACE` or `REPLACE`. The one conflict clause is `UpsertTrigger`'s `ON CONFLICT DO UPDATE` on `triggers`, which has no trigger.
- The pragma only makes a REPLACE's delete fire the delete guards. No conflict was found, and the whole suite passes with it on.
- By hand (Python's sqlite3 over the migrations), the rowid collision is stopped by `table_rows_rowid_fixed` and R2b's reverse collision by `tool_calls_identity_fixed`, not by a foreign key, and an unkinded call still finishes.

**Mechanical test edits:** none.

**Size, this round:** about +195 net lines of code:
- `0010_rows.sql`: +51;
- am-store `src/rows.rs`: +78;
- am-domain `src/rows.rs`: +40;
- am-store `src/lib.rs`: +24;
- xtask: +1.

The slice's code is now about 1,120 lines, far under the ~4,000-line stop.

**Verification:**
- `--test rows`: 41 of 41 pass.
- am-engine `--test journal`: 3 of 3 pass.
- `cargo test -p xtask --test check_deps`: 33 of 33 pass.
- `cargo test --doc -p am-store -p am-domain`: green (am-store 4 + 7, am-domain 5 + 8).

**Final `just check`:** green (exit 0). `Summary [52.823s] 1448 tests run: 1448 passed, 8 skipped`, with every other recipe clean.

### Review triage round 2 (manager, 2026-11-24)

An Opus follow-up review read round 1. It confirms R1–R13 and R2b hold, and finds no way through the row command. Six smaller gaps were found, one of them opened by R9. Each is fixed tests first.

| Id | From | Finding | Decision |
|----|------|---------|----------|
| T1 | F1 | `step_runs` has had no guard against being replaced since 0007. An `INSERT OR REPLACE` (or a status round-trip through `queued`) rewrites an attempt's `seat` and `writes_table`, and lowers its taint, which `write_row` then copies into the row. | Fix: a `step_runs_never_replaced` trigger (on id, rowid, and run/step/attempt). R6 also aborts when `OLD.seat`, `OLD.writes_table` or `OLD.started_at` is set. The implementer checks every legitimate `step_runs` path from 3.1–3.5 (requeue, loop-back, gate return) and stops and reports if one conflicts. Tests for each route. |
| T2 | F2 | Since R9, `json()` keeps duplicate keys and `json_set` changes only the first, so SQL readers and serde readers see different values. | Fix: refuse duplicate keys, at any depth, in an insert and in a patch, in words. Test. |
| T3 | F3 | The caps are checked after the merge, and the merge's cost grows with the square of the patch's key count on the single writer thread (a 1 MB patch took 18 s). | Fix: refuse a patch over `ROW_MAX_BYTES`, or with more than `ROW_MAX_COLUMNS` keys, before merging, in words. Test (bounded time). |
| T4 | F4 | `tool_calls_never_replaced` does not check the rowid, so an insert by rowid silently deletes a kinded call. | Fix: add `OR rowid = NEW.rowid`. Test. |
| T5 | F5 | A seat after-image planted by raw SQL at the next version makes that key a false replay: `apply_row_write` returns `Ok` while the call stays `issued`. | Fix: a replay requires the call at that key to be finished with kind `engine`. Anything else is refused in words. Test. |
| T6 | F6 | `Debug` prints `table_id` verbatim, and `RowRecord`/`RowHistoryRecord` derive `Debug` with data. | Fix: hand-written `Debug` showing lengths only. Test. |

**Accepted:** **D4 (F7):** the unique reader names could collide with a future field name. A collision makes check-deps fail, so it fails closed and is noticed at once, and renaming then is cheap.

### Red run round 2 (store slice, 2026-11-24)

These are the tester's tests for review triage round 2. Seven tests are appended to `crates/am-store/tests/rows.rs`, with no `src/` change, and `cargo fmt --all` passes.
- Commands, on the checkout with `CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager`:
  - `cargo nextest run -p am-store --test rows --no-fail-fast`: 48 run, 41 passed (every earlier test, round 1 included), 7 failed (the new ones);
  - `cargo clippy -p am-store --tests -- -D warnings`: clean.
- Words pinned (tester's choice): T1(a)'s new guard is `an attempt is never replaced`. The other guards' words are as implemented.

| Row | Test | Red reason |
|---|---|---|
| T1 (a) | `an_attempt_is_never_replaced`: an `INSERT OR REPLACE` of a started, tainted, child-free attempt with another seat, another table and taint 0, by its id, by its rowid (under a new id) and by its run, step and attempt; each must abort with `an attempt is never replaced`, and the writes and taint must stay | "by its id: must abort, changed 1 row(s)" |
| T1 (b) | `a_status_round_trip_never_reopens_an_attempts_writes`: status back to `queued` (allowed or not), then `running` with another seat or table, on an attempt with writes and on one started by `StartStep`; each must abort with `an attempt's writes are set only when it starts` | "UPDATE step_runs SET status = 'running', seat = 'seat-other' …: must abort, changed 1 row(s)" |
| T2 | `a_repeated_key_is_refused_in_words`: a repeated key at the top level and nested in an insert, and in a patch; each refused, the words holding `duplicate` and never the key; nothing changed, the call `issued` | "a repeated top-level key: ()" (it is written) |
| T3 | `an_oversized_patch_is_refused_before_it_is_merged`: a patch over `ROW_MAX_BYTES` (`byte`), one with `ROW_MAX_COLUMNS` + 1 keys (`column`), and a ~0.5 MB patch of 50,000 keys, each refused within a generous 2 s; nothing changed | the first two cases already refuse (after the merge). The large one is refused only after 77.9 s: "a large patch: refused before the merge, in 77.873572459s" |
| T4 | `a_kinded_call_is_never_replaced_by_its_rowid`: an `INSERT OR REPLACE` at the rowid of an `untrusted` call (no after-image references it, so no foreign key intervenes) must abort with `a tool call is never replaced`, and the call must stay | "a replace by rowid: must abort, changed 1 row(s)" |
| T5 | `a_planted_after_image_is_no_replay`: a seat's after-image planted by raw SQL at an `issued` call's key; `apply_row_write` there must be refused in words, with nothing changed and the call `issued` with no kind | "a planted after-image is no replay: ()" (it returned `Ok`) |
| T6 | `no_row_types_debug_holds_data_or_a_table_id`: canaries in the data and the table id must not appear in the `Debug` of `RowWrite`, `RowOp`, `RowRecord` or `RowHistoryRecord` | `RowWrite` prints `table_id: "table-canary-5a91"` |

**Patches.**
- I appended the tests to every slice state and regenerated the three patches in `.git/wip/`: `tests-toolgate.patch` (+4,434/−74), `tests-rows-reaching.patch` (+634/−63) and `tests-documents.patch` (+1,695/−84).
- They check clean in order on a temporary index built from the checkout, with nothing recorded in git.
- The final state equals the full test set file for file, except `crates/am-store/src/lib.rs`. The slice trees hold the source from before round 1's implementation; the patches' doc-comment hunks on that file apply cleanly to the implemented source.

### Implementation round 2 (store slice, 2026-11-24)

*Implementer, on Opus, against "Review triage round 2" and "Red run round 2". No test file was changed; no dependency was added; no `--workspace` build; nothing was recorded in git.*

**Built:**
- **T1 (a):** `step_runs_never_replaced` aborts an insert that meets an attempt's id, its rowid, or its (run, step, attempt), with "an attempt is never replaced".
- **T1 (b):** `step_runs_writes_set_at_start` now also aborts when `OLD.seat` or `OLD.writes_table` is set. The `OLD.started_at` clause is left out (the conflict is below). Following the manager's T1(b) decision, `step_runs_never_requeued` aborts any update that moves `status` to `queued` from any other status, with "an attempt never goes back to queued".
- **T2:** an insert's data and an update's patch are refused when they repeat a key at any depth. The check is `json_tree` grouped by parent and key, and the words say "duplicate key" without naming the key.
- **T3:** a patch is refused before it is parsed further or merged if:
  - its text is over `ROW_MAX_BYTES`, refused in words with `bytes`;
  - it has more top-level keys than `ROW_MAX_COLUMNS`, refused in words with `columns`.

  The ~0.5 MB patch is now refused at once; the whole `rows` target runs in about 1 s.
- **T4:** `tool_calls_never_replaced` also matches the rowid.
- **T5:** a key that holds an after-image is a replay only when the tool call at that key has result kind `engine`. Anything else is refused in words ("… so it is no replay").
- **T6:** `Debug` is written by hand for `RowWrite` (`table_id_bytes`), `RowRecord` and `RowHistoryRecord` (`table_id_bytes`, `data_bytes`). `RowOp` already showed lengths only.

**The `step_runs` path check (T1):**
- Store paths, all compatible:
  - inserts: `CreateRun`, and the new attempts queued by loop-back and by a gate return (always a new id and `MAX(attempt) + 1`);
  - updates: `start_attempt` (`queued` to `running`), `PauseStep`, `raise_taint`, the Human resolve, `finish_step` and the `looped_back` mark;
  - `FailBeforeStart` writes no `step_runs`, and `continue_run` and the spike write only through commands.
- No update ever moves a status back to `queued`.
- **The conflict:** `crates/am-engine/tests/loop_caps.rs` (story 3.3 review P1) stamps `started_at` on a still-queued attempt by raw SQL. It relies on the store's documented behaviour that `StartAttempt` keeps a `started_at` already there, which story 3.9's resume will count.
- With the `OLD.started_at` clause in, `StartAttempt`'s queued-to-running update, which sets the seat, aborts. Two existing tests fail: `a_fresh_attempt_gets_its_whole_wall_clock_cap` and `a_fresh_attempt_started_just_before_a_second_boundary_waits_the_full_cap`. I confirmed this by running them.
- Without the clause, `a_status_round_trip_never_reopens_an_attempts_writes` stays red on its `StartStep` attempt.
- **Resolved** by the manager's T1(b) decision: the guard that refuses any move back to `queued`, built above. `loop_caps.rs` stays green.

**Mechanical test edits:** none.

**Size, this round:** about +100 lines of code:
- `0010_rows.sql`: +14;
- am-store `src/rows.rs`: +86;
- am-domain `src/rows.rs`: +1.

The no-requeue guard adds 8 more lines of SQL. The slice's code is about 1,230 lines.

**Verification:**
- `cargo test --doc -p am-store -p am-domain`: green.
- Every `just check` recipe but `test` is green.
- Before the T1(b) decision, `a_status_round_trip_never_reopens_an_attempts_writes` was the one red test. It passes with the no-requeue guard.

**Final `just check`:** green (exit 0). `Summary [54.118s] 1455 tests run: 1455 passed, 8 skipped`, with every other recipe clean.

**T1(b) decision (manager, 2026-11-24):** the `OLD.started_at` clause conflicts with story 3.3's `loop_caps.rs` fixture, which sets `started_at` on a still-queued attempt. So T1(b) is closed another way: an attempt's status never moves back to `queued` from any other status. No code path from 3.1 to 3.5 does that, so the round trip fails at its first step, and 3.3's behaviour is unchanged. This only narrows. Note for story 3.9: if resume ever needs to requeue an attempt, it relaxes this guard in its own migration and review. The tester adjusts T1(b) to expect the move back to `queued` to abort with the guard's words.

**T1(b), revised after the manager's decision (tester, 2026-11-24).** `a_status_round_trip_never_reopens_an_attempts_writes` now pins the first step of the round trip, with the words (tester's choice) `an attempt never goes back to queued`.
- **What it asserts:**
  - a raw `UPDATE step_runs SET status = 'queued'` aborts with those words on four attempts: one `running` with its writes journaled, one `running` started by `StartStep` with none, one `succeeded` and one `failed`;
  - every attempt, its seat and its table stay as they were;
  - a queued attempt still starts through `StartAttempt` and journals its writes.
- **Red reason** (`cargo test -p am-store --test rows -- a_status_round_trip`): "running, with writes: must abort, changed 1 row(s)", because today the move back to `queued` goes through.
- **Lint and patches:** clippy is clean. The three patches are regenerated and still check clean in order on a temporary index.

## Auto Run Result

**Status:** built (2026-11-24). This is slice 1 of 4 of the re-sliced story 3.6: store, then ToolGate (23), then rows reaching later steps (22), then documents and Drafts (21). It was planned, tested, built and reviewed on Opus, as the owner asked.

**Summary:** the row store and its history exist at the store level only. Nothing shipped writes or reads rows yet.
- **Migration 0010:**
  - `table_rows` and an append-only `row_history` of after-images, plus `step_runs.seat`/`writes_table` and the result kind and label on `tool_calls`.
  - Every guard is a trigger, and `recursive_triggers` is on for every store connection, so a REPLACE delete fires them too.
  - No UPDATE or DELETE on history, no DELETE on rows, and no replace by key, id or rowid on rows, history, tool calls or attempts.
  - A row is never re-created. Taint only rises.
  - A tool call's identity is fixed, and its status is fixed once its result kind is set.
  - An attempt's seat and table are set only when it starts, and an attempt never goes back to `queued`.
  - The pre-migration copy is kept, and an old database upgrades with every row kept.
- **The row write (`apply_row_write`):** a typed writer command with an explicit Actor.
  - It runs in one transaction with its after-image at the next version.
  - Its taint is the larger of the attempt's, the write's and the row's.
  - A stale version is refused in words, and the old version stays readable.
  - A cancelled or finished run is refused.
  - Row data is stored through SQLite `json()`, so numbers keep their text, and an update's patch is merged one level deep in SQL.
  - A duplicate key at any depth is refused.
  - The caps are 64 KiB and 256 columns, and an oversized patch is refused before merging.
  - Refusals give sizes, ids and the journaled table only.
  - A replay is honoured only for a finished `engine` call.
  - The result kind `engine`, labelled `tool:table_write`, is set on the tool call's journal row (AD-10 as amended). The data-free receipt counts columns in SQL.
  - Audit arguments are ids, counts and versions, with no data and no hash.
- **The owner's conditions:**
  - check-deps row 48: the row-write command is named only in am-store and its tests until the ToolGate slice.
  - check-deps row 49: the row-content readers have unique names (`table_rows_of`, `table_row_by_id`, `row_history_of`, `row_written_at`) and are matched as bare tokens.
  - A step has no read path: no `from_row`, no row-input slot.
  - `Debug` on the row types shows lengths only.

**Size (owner rule: code and test lines reported separately):** 1,288 lines of code (source and migration, doc comments included) and 3,844 lines of tests.

**Tests:** 48 store tests in `rows.rs`, plus migration tests, xtask tests for rows 48 and 49, doctests, and literal updates. Every test was written and seen red first, with red runs logged for the build and both review rounds. `just check`: 1464 passed, 8 skipped (after round 4).

**Review:** a correctness review and an adversarial security review, then a follow-up review, all on Opus. None found a way through the row command.
- Round 1 fixed R1–R13 and R2b. The worst was a rowid collision that could re-create a tainted row clean.
- Round 2 fixed T1–T6. T1(b) was closed by the never-back-to-`queued` guard, because the `started_at` clause conflicted with a 3.3 fixture.
- Round 3 (owner, 2026-11-24) brought that guard in line with AD-8. A move to `queued` is refused except from `waiting_branches`, the fan-in edge story 3.14 builds. A move into `waiting_branches` is refused except from `queued`. `waiting_branches → queued` is refused when the row has a seat, a table or a `started_at`.
- Round 4 (quick security re-review) fixed U1–U3. An attempt's identity never changes, which closes an `UPDATE OR REPLACE` identity swap that could reopen a finished, written attempt. `started_at` is set once. A new attempt is inserted only as `queued`, with no seat, table or start. D5 and D6 are accepted and recorded in the register for 3.14.
- D1–D4 are accepted, with reasons above.

**For the owner:**
- **Decided under the standing rules:**
  - the row caps (64 KiB, 256 columns);
  - row 49 moved into this slice (it only narrows);
  - the never-back-to-`queued` guard.
- **Note for 3.9:** a resume that requeues an attempt must relax that guard in its own review.
- **Residuals:**
  - a proc-macro could hide a name from check-deps (D3);
  - `Actor::Seat` carries no attempt id, which matters for 3.14 (D2);
  - deleting a tool call stays possible, for AD-25 run deletion (D1).

Round 3 (owner, 2026-11-24): the no-requeue guard must allow AD-8's `waiting_branches → queued` edge (story 3.14). The owner's terms:
- every other move to `queued` is refused;
- a move into `waiting_branches` comes only from `queued`;
- `waiting_branches → queued` is refused for a row with a seat, a table or a `started_at`.

### Red run round 3 (store slice, 2026-11-24)

The tester's response, tests first. Five tests are appended to `crates/am-store/tests/rows.rs`, with no `src/` change, and `cargo fmt --all` passes.

**Words pinned** (tester's choice):
- `an attempt waits for branches only from queued`: a move into `waiting_branches` from any status but `queued`;
- `only an attempt that never started goes back to queued`: `waiting_branches → queued` on a row with a seat, a table or a `started_at`;
- `an attempt never goes back to queued` stays the words for every other move to `queued`.

**Commands** (`CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager`):
- `cargo nextest run -p am-store --test rows --no-fail-fast`: 53 run, 49 passed, 4 failed;
- `cargo clippy -p am-store --tests -- -D warnings`: clean.

| Test | Covers | Today |
|---|---|---|
| `the_fan_in_edges_of_ad_8_are_allowed` | the allowed edges: `queued → waiting_branches` on a fresh attempt, then `waiting_branches → queued` (no seat, table or `started_at`); the attempt then starts and journals its writes | **red**: the edge back is refused, `Some("an attempt never goes back to queued")` |
| `no_status_but_waiting_branches_goes_back_to_queued` | a move to `queued` from `running`, `waiting_human`, `waiting_review`, `succeeded`, `failed`, `cancelled` and `looped_back` is refused with "an attempt never goes back to queued", and the row is unchanged | green on arrival: today's guard refuses every move to `queued` |
| `only_a_queued_attempt_waits_for_branches` | a move into `waiting_branches` from each of those seven statuses is refused with the new words, and the row is unchanged | **red**: "running: must abort, changed 1 row(s)" |
| `a_started_attempt_never_goes_back_from_waiting_branches` | three `waiting_branches` rows made by hand, one with a seat, one with a table, one with a `started_at`; `→ queued` is refused with the new words | **red** on words: today's guard refuses with "an attempt never goes back to queued" |
| `the_round_trip_through_waiting_branches_is_refused` | `running → waiting_branches → queued` on a started attempt with writes: refused at the first step with the new words, the second step refused too, and the attempt, its seat and table unchanged | **red**: "running → waiting_branches: must abort, changed 1 row(s)" |
| `a_status_round_trip_never_reopens_an_attempts_writes` (round 2) | unchanged | green: it still holds |

The `.git/wip` patches are untouched. The ToolGate tests are stashed by the manager.

### Implementation round 3 (store slice, 2026-11-24)

*Implementer, on Opus, against the owner's decision of 2026-11-24 and "Red run round 3". Only migration 0010 changed, in place (+15/−5 lines). No test file or `src/` file changed; no `--workspace` build; nothing was recorded in git.*

**Built:** three guards on `step_runs.status`, in place of round 2's single `step_runs_never_requeued`:
- **`step_runs_never_requeued`:** a move to `queued` from any status but `queued` and `waiting_branches` aborts, with "an attempt never goes back to queued".
- **`step_runs_requeued_only_unstarted`:** `waiting_branches → queued` aborts when the row has a `seat`, a `writes_table` or a `started_at`, with "only an attempt that never started goes back to queued".
- **`step_runs_waits_for_branches_from_queued`:** a move into `waiting_branches` from any status but `queued` (or `waiting_branches`) aborts, with "an attempt waits for branches only from queued".

So AD-8's fan-in edges, `queued → waiting_branches → queued`, pass for an attempt that never started, and round 2's T1(b) test still holds. The `OLD.started_at` clause stays out of `step_runs_writes_set_at_start`.

**Verification:**
- `--test rows`: 53 of 53 pass.
- `cargo test --doc -p am-store -p am-domain`: green.

**Final `just check`:** green (exit 0). `Summary [56.182s] 1460 tests run: 1460 passed, 8 skipped`.

### Review triage round 4 (manager, 2026-11-24)

A quick security re-review of round 3 (owner: "quick security re-review of the trigger change") found the three new triggers sound. Every route back to `queued` or `waiting_branches` is refused, and every AD-8 edge the engine uses today works. It found one medium gap that was already in the slice, and two low ones.

| Id | From | Finding | Decision |
|----|------|---------|----------|
| U1 | Q1 | Identity swap: `UPDATE OR REPLACE step_runs SET id=<A>, rowid=<A's rowid>, …` on a fresh queued row B deletes the finished, written A. B takes over A's identity, with foreign keys intact, taint 0, and a free choice of seat and table. | Fix: an `id`, rowid, `run_id`, `step_key`, `seq` or `attempt` change on `step_runs` aborts, the same pattern as `tool_calls_identity_fixed`. Test with the exact swap. |
| U2 | Q2 | `started_at` can be changed or nulled. | Fix: `started_at` is set once. Test. |
| U3 | Q3 | An INSERT can preset a seat or table, or use any status. | Fix: a new attempt is inserted only as `queued`, with no seat, no table and no `started_at`. The implementer checks every insert path (`lib.rs` near 1175 and 2362, the spike `journal.rs`) and stops and reports if one conflicts. Test. |

**Accepted:**
- **D5 (Q1, part):** a plain DELETE of an attempt, followed by a re-insert, stays possible with raw SQL. AD-25's run deletion must be able to delete attempts (as D1 for `tool_calls`), and raw SQL by the same user is AD-13's accepted residual.
- **D6 (Q4):** AD-8's `waiting_branches → waiting_human → running` path cannot set a seat or table under `writes_set_at_start`, and AD-8's `looped_back → queued` edge is realised as a new attempt row (story 3.3), never a same-row move. Both go into the register for 3.14.

### Red run round 4 (store slice, 2026-11-24)

The tester's response to U1–U3, tests first. Four tests are appended to `crates/am-store/tests/rows.rs`, with no `src/` or migration change, and `cargo fmt --all` passes.

**Words pinned** (tester's choice):
- U1: `an attempt's identity never changes`;
- U2: `an attempt's start time is set once`;
- U3: `a new attempt is inserted only as queued, with no seat, table or start`.

**Commands** (`CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager`):
- `cargo nextest run -p am-store --test rows --no-fail-fast`: 57 run, 53 passed, 4 failed;
- `cargo clippy -p am-store --tests -- -D warnings`: clean.

| Test | Covers | Today |
|---|---|---|
| `the_identity_swap_onto_a_finished_attempt_aborts` | **U1, the exact swap.** A is `succeeded`, tainted, with a seat, a table, a tool call and an after-image. B is a fresh queued attempt. The statement is `UPDATE OR REPLACE step_runs SET id = A, rowid = A's rowid, step_key, seq, attempt = A's WHERE id = B`. It must abort with the words. A and B must stay cell for cell (rowid included), every table must be unchanged, and A keeps its writes, its call and its after-image | **red**: "the identity swap: must abort, changed 1 row(s)". The swap goes through today. |
| `an_attempts_identity_never_changes` | **U1, one case per column**, on a fresh queued attempt: `id`, rowid, `run_id` (to a run that exists), `step_key`, `seq`, `attempt`. Each must abort with the words and leave the attempt unchanged. Controls: an `updated_at`-only update goes through, and the attempt then starts and finishes | **red**: "id: must abort, changed 1 row(s)" |
| `an_attempts_start_time_is_set_once` | **U2.** The normal start sets `started_at` the first time, from `NULL`, both by `StartAttempt` with writes and by `StartStep`. After that, a raw change and a raw `NULL` must each abort with the words, on a running attempt (with and without writes) and on a succeeded one, and leave the attempt unchanged | **red**: "running, with writes, changed: must abort, changed 1 row(s)" |
| `a_new_attempt_is_inserted_only_queued_and_unstarted` | **U3.** A raw insert must abort with the words for any status other than `queued` (the seven of round 3, plus `waiting_branches`). So must a queued insert with a seat, with a table, or with a `started_at`. Each leaves every table unchanged. Controls: a plain queued insert goes through. A new run (`lib.rs` near 1175) and a loop-back's next attempt (`lib.rs` near 2362) still queue unstarted attempts with no writes | **red**: "running: must abort, changed 1 row(s)" |

**Corrections to two tests** (tester; they still pass today):
- U3's guard also refuses the rows that `an_attempt_is_never_replaced` (T1 (a)) re-inserts. SQLite fires the trigger created last first: checked with `sqlite3`, two `BEFORE INSERT` guards abort with the newer one's words. So the test now sets the other `step_runs` guards aside with `leave_only`, and keeps asserting its own words.
- Round 3's `a_started_attempt_never_goes_back_from_waiting_branches` builds its `waiting_branches` rows by a raw insert with a seat, table or start, which U3 refuses. That test now sets the other guards aside the same way.

**Stub check.** This was a scratch copy with its own `CARGO_TARGET_DIR` under the tester's scratchpad. It had three stub triggers on 0010: `tool_calls_identity_fixed`'s pattern over `rowid`, `id`, `run_id`, `step_key`, `seq` and `attempt`; `started_at` refused when `OLD.started_at IS NOT NULL AND NEW.started_at IS NOT OLD.started_at`; and an insert refused unless it is `queued` with `seat`, `writes_table` and `started_at` all NULL. Results:
- `cargo nextest run -p am-store`: 221 run, 221 passed, so all four tests and both corrections are green with the fix;
- `cargo nextest run -p am-engine`: 72 run, 72 passed, `loop_caps` included.

**Note for the implementer (U2):** `crates/am-engine/tests/loop_caps.rs` (`backdate`, near line 640) sets `started_at` by hand on a *queued* attempt. That is a first set, from `NULL`. A guard that refuses only a change or a `NULL` once the column is set keeps that test green. A guard that allows the first set only on `queued → running` breaks it.

The `.git/wip` patches are untouched. The ToolGate tests stay stashed.

### Implementation round 4 (store slice, 2026-11-24)

*Implementer, on Opus, against "Review triage round 4" and "Red run round 4". Only migration 0010 changed, in place (+22 lines). No test or `src/` file changed; no `--workspace` build; nothing was recorded in git.*

**Built:**
- **U1, `step_runs_identity_fixed`:** any update that changes an attempt's rowid, `id`, `run_id`, `step_key`, `seq` or `attempt` aborts, with "an attempt's identity never changes". That includes the exact `UPDATE OR REPLACE` identity swap.
- **U2, `step_runs_started_at_set_once`:** a `started_at` already set may not change or go back to NULL, with "an attempt's start time is set once". The first set, from NULL, passes on any status. That keeps `StartAttempt`'s `COALESCE` and `loop_caps`'s queued backdate working.
- **U3, `step_runs_inserted_queued`:** an insert is refused unless it is `queued` with `seat`, `writes_table` and `started_at` all NULL, with "a new attempt is inserted only as queued, with no seat, table or start".

**The insert-path check (U3):** no path conflicts.
- `lib.rs` 1175 (`CreateRun`) and 2362 (the next attempts of a loop-back or a gate return) insert `queued`, with no seat, table or start.
- The spike's `journal.rs` and the engine insert only through those commands.
- The raw inserts in `run_journal.rs` and `prompt_journal.rs` all go into databases older than 0010.

**Verification:**
- `cargo nextest run -p am-store -p am-engine`: 293 of 293 pass, `loop_caps` included.
- `cargo test --doc -p am-store -p am-domain`: green.

**Final `just check`:** green (exit 0). `Summary [53.714s] 1464 tests run: 1464 passed, 8 skipped`.
