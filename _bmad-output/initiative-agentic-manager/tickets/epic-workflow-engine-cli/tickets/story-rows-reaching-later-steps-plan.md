---
title: 'Rows reaching later steps'
type: feature
ticket: 22
created: '2026-11-24'
status: approved
baseline_revision: 'df47903 (stacked on 3.25, PR #44; rebased onto main when it merges)'
after: [23]
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
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-rows-written-with-provenance-through-toolgate-plan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Slice:** 3.6c, epic 3 entry 22, re-sliced from story 3.6 by the owner on 2026-11-24 (Design Notes, Re-slice). It builds right after the ToolGate slice (entry 23, "table_write through ToolGate"), which follows the store slice (entry 6, "Row store and history"), and before 3.6b (entry 21, "Document versions and Draft items", `after = [22]`), on its own branch; its own `after = [23]`; its baseline is set when it starts.

**Naming (owner, third re-slice):** "3.6a" below means the two slices that replaced it: the store slice (entry 6: migration 0010, the row command, the readers, the `StepInput::from_row` tripwire) and the ToolGate slice (entry 23: the port, `table_write`, the engine result, the row-content check-deps rule, the `RowInput` and `StepPlan::row_inputs` tripwires, `taint_rows_no_read_path`). Matrix row numbers follow the approved 3.6 plan, so the written tests still point at them: row 38 comes from 3.6a's plan; rows 45 to 47 are new.

**Problem (after 3.6a):** rows are written with their own taint, but by the owner's condition nothing reads them.
- 3.6a has no row-input slot on `StepPlan`, no `RowInput` type, no `from_row` link and no `row` input kind, even test-only; three `compile_fail` doctests pin the absence.
- 3.6a's `cargo xtask check-deps` rule lets the store's row-content readers stand only in `crates/am-store/src/`, and only am-store links `rusqlite`; so no shipped path passes row content to a model call.
- AD-10 as restated 2026-11-24 requires a row written by a tainted step to carry that taint to every later reader, and says each story that adds a read path proves that path. The owner's Q4 condition asks for that proof, with a test, and moved it here.

The verify, as re-sliced (Sofia's entry 22 is the spec once merged): a row written by a tainted step taints a later step that reads it; the reader's input edge carries the row's at-rest taint and names the after-image it read; the row's text reaches the reader's prompt only fenced, as `UntrustedData`; and a clean row's text is fenced and taints its reader too (the mark is a floor, never a ceiling).

**Approach (owner-approved 2026-11-24, carried over unchanged; Q4's reader condition):**
- **One read path, one typed slot:** `StepPlan.row_inputs: Vec<RowInput>`, rows a step reads at its start. Like 3.5's CEO-goal slots, the shipped `plan_of` leaves it empty; 3.15 (`rows:` hand-off payloads) and 7.2 (granted reads) fill it from config. Tests fill it now.
- **The link (AD-43):** each read is a `step_inputs` edge of kind `row`, journaled in the transaction that starts the attempt, naming the after-image it read by `from_row` (its `row_history.seq`, migration 0011) and carrying at least the row's at-rest taint.
- **Fenced row text (AD-10):** the row's data enters the prompt as `UntrustedData` inside the untrusted fence, labelled `row:<table>/<row-id>@<version>`, so the reading attempt is tainted whatever the row's mark. Its text is not copied into `prompt_parts` (it stays on the after-image).
- **One named read site:** the engine reads a row's content in exactly one shipped file, `crates/am-engine/src/row_inputs.rs`, through one store reader, `Reader::row_snapshot`; 3.6a's check-deps rule gains that one site and nothing else.

## Boundaries & Constraints

**Always:**
- **Row text is `UntrustedData` (AD-10, free text in rows):** a row read never becomes an `Instruction` or a `TrustedInput`; there is no conversion, and `row_inputs.rs` names neither type's constructor. The reading attempt is tainted.
- **The mark is a floor (AD-10 as restated 2026-11-24):** the input edge's taint is at least the after-image's taint; the writer refuses a lower one. A tainted row read by a step leaves that step tainted, however long after; the row's own mark never changes on a read.
- **Lineage is the journal (AD-43):** the edge names the after-image (`from_row` = its `seq`); the text is never stored twice (not in `step_inputs`, not in `prompt_parts`).
- **One read site (3.6a's row-content rule, widened by one):** `Reader::row_snapshot` stands in shipped code only in `crates/am-store/src/` and `crates/am-engine/src/row_inputs.rs`; the other content readers stay am-store only; only am-store links `rusqlite`. Whole-file text with whitespace normalised; aliases and globs refused (3.5's T5).
- **Read at the start, from the journal (AD-8):** a step's rows are read when its attempt starts, from the committed after-images, in the transaction that journals the edges. A row missing at that point fails the attempt's start in words, naming the table and row id (store ids), never its data.
- **No grant path here (AD-21, AD-22):** `row_inputs` reads what a plan names; this slice adds no config key that names rows. Which tables a step may read (own, or granted as of `start_seq`) is 3.15's and 7.2's, through config and AD-15's widening rules.
- **Audit (AD-25; AD-32):** `step.started` counts the row inputs; no row data and no hash of it in any audit entry.
- **Owner's answers unchanged:** this slice builds only Q4's reader condition; 3.6a's and 3.6b's answers stand as written.
- **New callers of `Instruction::authored` (owner, 2026-11-24):** none.
- **Order of work (owner, 2026-11-24):** after the ToolGate slice merges, this plan's baseline is set and it is committed on its own branch; the tester's split tests for this slice are logged red before any `src/` change; one squash commit, on Opus.
- **Tests first:** every test in the Tests-first list is written and seen red, with its reason logged, before any `src/` change. FakeLlm and FakeClock only; no network, no real model.

**Never:**
- **No other read path:** no `table_read` tool, no rows in a plan's untrusted `inputs`, no row text in a step output, a tool result, an `EngineResult` or an Inbox item.
- **No config key** that fills `row_inputs` (3.15, 7.2), and no `start_seq` (7.2).
- **No write here:** no change to the row-writer path, the engine result or its journaling (3.6a).
- **No new crate, no new prompt content type.**
- **No department vocabulary in `crates/`.**
- **Outside this slice:** `table_read` and `query_table` (later stories), granted reads as of `start_seq` (7.2), `rows:` hand-off payloads (3.15), lineage reads (3.17), the RULE-8 suite (4.9), documents and Drafts (3.6b).

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 38 | A tainted row taints its reader (owner Q4) | Step A (tainted) writes row `r`; a later step B reads `r` through its `row_inputs` slot | B's `row` input edge has taint 1 and names `r`'s after-image `seq`; B's prompt holds `r`'s data fenced, labelled `row:<table>/<r>@<version>`; B's attempt is tainted; `r`'s mark stays 1. A clean row's edge has taint 0, and its text is still fenced and taints B (the mark is a floor, AD-10 as restated) | — |
| 45 | Migration 0011 | A version-10 database with rows, history, runs, attempts, calls, tool calls with result kinds, Inbox items, prompt parts | Pre-migration copy; every row kept value for value; `step_inputs.from_row` NULL on old rows; report 10 → 11; `store.migrated` audited; reopening applies nothing; a failing 0011 names the backup and leaves version 10 | Migration error |
| 46 | One read site | Planted files: `row_snapshot` named in `run.rs`, am-app or am-server; a content reader other than `row_snapshot` in `row_inputs.rs`; an alias of `RowSnapshot`; the shipped tree | `cargo xtask check-deps` fails for each planted form, naming file and line; the shipped tree passes with `row_inputs.rs` as the one site | CI fails |
| 47 | Row text never trusted | A row holding a canary, a fake fence close (`</untrusted-data>`), a fake label line and a bidi mark; read by a step | The canary appears only inside that row's own fenced part; no system, instruction or trusted part holds it; the fence holds (3.5's neutraliser); the `prompt_parts` row for it has kind `untrusted`, taint 1, no text; the edge's writer check refuses a taint below the row's; a row missing at the start fails the start in words naming only store ids | Worded |

</intent-contract>

## Code Map

- **`crates/am-domain/src/rows.rs`:** `pub struct RowInput { pub table_id: String, pub row_id: Uuid }`. 3.6a's `compile_fail` doctest on its absence is removed.
- **`crates/am-store`:**
  - `migrations/0011_row_inputs.sql` (new): `ALTER TABLE step_inputs ADD COLUMN from_row INTEGER REFERENCES row_history(seq)`; `src/migrations.rs` `MIGRATIONS` gains it; `SCHEMA_VERSION` becomes 11. (0010 is 3.6a's; 0012 is 3.6b's.)
  - `src/lib.rs`:
    - `StepInput` gains `pub from_row: Option<i64>`; 3.6a's `compile_fail` doctest on its absence is removed.
    - `journal_inputs` (L1634 at the 3.6 baseline): accepts kind `row` (no `from_step_run`, no `from_item`; `from_row` names an existing after-image; taint raised to at least that after-image's).
    - `Reader::row_snapshot(table_id, row_id) -> Option<RowSnapshot { seq, version, data, taint }>`: the current after-image, the one reader the engine may name for content.
    - The prompt-part check of 3.5 (`check_attempt_prompt`): an `untrusted` part labelled `row:…` must match a journaled `row` edge, as other input parts do.
- **`crates/am-engine`:**
  - `src/row_inputs.rs` (new): reads each `RowInput` with `Reader::row_snapshot`, builds the `row` edge and the fenced `UntrustedData` part. The one shipped file outside am-store that reads row content. It names no `Instruction` or `TrustedInput` constructor.
  - `src/run.rs`: `StepPlan` gains `row_inputs: Vec<RowInput>` (3.6a's `compile_fail` doctest removed); `inputs_of` (L909 at the 3.6 baseline) calls `row_inputs.rs`; the fenced parts go to `prompt::assemble`'s untrusted slot.
- **`crates/am-app/src/run.rs`** `plan_of`: `row_inputs: Vec::new()` on every step (3.15 and 7.2 fill it).
- **`xtask/src/deps.rs`:** 3.6a's row-content rule gains `row_snapshot` as a content reader with its one allowed site, `crates/am-engine/src/row_inputs.rs`; `RowSnapshot` joins the record types named only in am-store and that file.
- **Docs:** none (no config key).
- **Existing tests that must change (supersession):**
  - 3.6a's three `compile_fail` doctests (`RowInput`, `StepPlan::row_inputs`, `StepInput::from_row`) are removed.
  - 3.6a's `there_is_no_row_input_kind_and_no_from_row_column` (am-store `rows.rs`) becomes "a `row` input is journaled with its `from_row`".
  - 3.6a's migration test, if it pins `SCHEMA_VERSION == 10`, becomes "the 9 to 10 step is applied".
  - 3.6a's `the_shipped_tree_passes_the_three_rules` (xtask) allows the one read site.
  - Every `StepPlan { .. }` literal gains `row_inputs` (through the test helpers where they exist) and every `StepInput { .. }` literal gains `from_row: None`; the tester's edits written for 3.6 move here.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:** the store and ToolGate slices merged; this plan's baseline set to main after the ToolGate slice; this plan committed on its own branch.
- [ ] **Tests first (no `src/` change), on Opus:** this slice's files in the Tests-first list and the superseded literals. Log this slice's red run with each test's failure.
- [ ] **am-domain:** `RowInput`.
- [ ] **am-store:** migration 0011, `StepInput.from_row`, the `row` kind, `row_snapshot`.
- [ ] **am-engine:** `row_inputs.rs`; `StepPlan.row_inputs`; `inputs_of`.
- [ ] **am-app:** empty `row_inputs`.
- [ ] **xtask:** the one read site in the row-content rule, with planted files.
- [ ] **Size (owner Q9, re-slice):** one squash commit; if it overruns, stop and report.

**Acceptance Criteria:**
- Given each matrix row of this slice, when its test runs, then edges, prompts, taint, parts and findings match the row.
- Given this slice's verify (the owner's Q4 reader condition), when a tainted step's row is read by a later step on FakeLlm, then the reader is tainted, its edge carries the row's taint and names its after-image, and the row's text is fenced.
- Given the workspace, when `just check` runs, then it is green, 3.6a's suites included (its `taint_rows_no_read_path` test still green: nothing in it reads rows through a slot). Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-24, owner (second re-slice): split from 3.6a's plan (`story-rows-written-with-provenance-through-toolgate-plan.md`). Carries the row-read side: the row-input slot, the `from_row` link (migration 0011), the `row` input kind, fenced row text, reader taint, matrix row 38 and Q4's reader condition, unchanged. Added rows 45 (migration 0011), 46 (the one read site in 3.6a's row-content rule) and 47 (row text never trusted). Baseline set when it starts.
- 2026-11-24, owner (third re-slice): 3.6a split into the store slice (entry 6) and the ToolGate slice (entry 23); this slice now builds after the ToolGate slice (`after = [23]`, baseline `TBD-after-toolgate`). Its scope is unchanged; "3.6a" in this plan names the two (Intent, Naming). Size reported as code and test lines.

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

**In this slice:** Q4's reader condition, unchanged: "a row written by a tainted step carries that taint to any later reader, with a test" (matrix row 38). The owner moved it here from 3.6a on 2026-11-24 (second re-slice). Q9 for this slice. Every other answer is 3.6a's or 3.6b's, unchanged.

### Re-slice (owner, 2026-11-24)

**First, owner verbatim:** "Don't accept a single 12,000-line commit. Sofia re-slices 3.6 into 3.6a (row store, history, row-writer interface, table_write via ToolGate only, engine-built result, tainted-row rules) and 3.6b (document versions, Draft items with Human-only dismissal). My approved answers carry over unchanged. Split the written tests by slice, log a red run for each, one squash commit per slice, on Opus. Fan-out: accept that the Company A f2-update step fails until 3.14, but agentic validate must name the reason, and 3.14's ticket must include allowing table: on fan-out steps."

**Third, owner verbatim:** "Split 3.6a by layer: a store slice (migration 0010, row and history write commands, append-only triggers, taint on write, stale-version refusal, store-level tests) and a ToolGate slice (row-writer interface, table_write via ToolGate only, engine result and journaling, idempotency, check-deps rules, no-read-path proof, fan-out warning). Order: store, ToolGate, then 3.6c, then 3.6b. Condition: nothing in shipped code calls the row-write commands until the ToolGate slice lands, enforced by check-deps. Accept Winston's wording "on its tool call's journal row (AD-8)" in the same re-slice PR. From now on, plan estimates report code and test lines separately, and the overrun stop is about 4,000 lines of code."

**Second, owner verbatim:** "Split again: move the row-read side (row-input slot, from_row link, reader taint, fenced row text) into a new 3.6c, "Rows reaching later steps", built right after 3.6a and before 3.6b. Condition: 3.6a contains no way for a step to read rows, with no from_row and no row-input slot, even test-only, plus a test or check-deps rule proving no engine path passes row content to a model call. 3.6a still stores each row's own taint on write. My Q4 condition moves to 3.6c unchanged. Add the AD-10 journaling of the engine result to 3.6a."

**What this means here:**
- This slice: the row-input slot, `from_row` (migration 0011), the `row` input kind, fenced row text, reader taint, and the proof of Q4's reader condition.
- 3.6a keeps the writes, each row's own taint stored on write, the engine result and its journaling, and the proof that no row reaches a model; this slice opens exactly one read path in that proof.
- Build order and migrations: 3.6a (0010), this slice (0011), 3.6b (0012).

### Reading rows (AD-10 as amended and restated 2026-11-24, AD-43)

- **The slot:** `StepPlan.row_inputs` lists `RowInput { table_id, row_id }`. It is a typed plan slot, filled only by tests in this slice (3.5's precedent: CEO goals, guidance and memory notes were slots the shipped `plan_of` left empty until their stories filled them). 3.15 and 7.2 fill it from config, each under AD-15's widening rules and AD-22's `start_seq`.
- **At the attempt's start**, in `inputs_of`, `row_inputs.rs` reads each row's current after-image with `Reader::row_snapshot` (its `seq`, version, data and taint) and makes:
  - one `step_inputs` edge: kind `row`, label `row:<table>/<row-id>@<version>`, `from_row` the after-image's `seq`, taint at least the after-image's taint;
  - one `UntrustedData` part with that label, the row's data as text, placed in the untrusted fence by 3.5's assembler.
- **Journaled with the start (AD-8, AD-43):** the edges commit in `StartAttempt`'s transaction; the writer checks `from_row` exists and the edge's taint is not below the after-image's; the `prompt_parts` row for the part has kind `untrusted`, taint 1 and no text (the text stays on the after-image, as an input's text stays on its producer's row).
- **Taint:** row text is always `UntrustedData` (AD-10, free text in rows), so the reading attempt is tainted, clean row or not. The edge's taint carries the row's at-rest mark, so lineage (3.17) tells a tainted row's edge from a clean one's, and a tainted row's taint reaches every node below it. The mark is a floor: a read never lowers it, and a clean mark never makes a read trusted.
- **A row missing at the start** (an unknown id, or a table the row is not in) fails the start in words naming the table and row id (store ids, never data); no attempt runs.
- **Why `row_snapshot` and not 3.6a's test-facing readers:** one reader, one site, so 3.6a's row-content rule widens by exactly one name and one file.

### The check-deps rule, widened by one site

- 3.6a's rule: the store's row-content readers in call position and the record types that carry row data stand in shipped code only in `crates/am-store/src/`; only am-store links `rusqlite`.
- This slice adds `row_snapshot` to the readers and `RowSnapshot` to the record types, with one allowed site outside am-store: `crates/am-engine/src/row_inputs.rs`. Everything else is unchanged; aliases and globs stay refused.
- 3.6a's run test (`taint_rows_no_read_path`) stays green: its steps fill no `row_inputs`, so no row text reaches any request.

### Owner decisions in this slice

- **Q4, reader condition (moved here, unchanged):** proven by `taint_rows_reader.rs` (row 38), with the edge, the fence and the taint as above.
- **Q9:** one squash commit for this slice; stop and report if the code passes about 4,000 lines (owner, third re-slice).

### Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code:** about 400 to 600 lines: migration 0011 (~20), am-store's `row` kind, `from_row`, `row_snapshot` and the prompt-part check (~150 to 200), am-domain `RowInput` (~10), am-engine `row_inputs.rs` and `inputs_of` (~200 to 300), the xtask rule's one site (~30). **Well under the ~4,000-line stop.**
- **Tests:** about 800 to 1,000 lines: `taint_rows_reader.rs` (~210, moved), the store edge test (~70, moved), the migration tests (~150), the fence and engine tests (~200), the planted files (~60), the `row_inputs` and `from_row` literal edits (~200 to 300).

### Tests-first list

All on FakeLlm and FakeClock; no network; no real model. The tester splits these from the tests written for 3.6 and logs this slice's red run after the ToolGate slice merges.

**am-e2e trust (`crates/am-e2e/tests/trust/taint_rows_reader.rs`, both, moved from 3.6a):** `a_row_written_by_a_tainted_step_taints_a_later_reader` (row 38: edge taint 1, the reading attempt tainted, the row's mark unchanged); `a_clean_rows_edge_is_clean_and_its_text_still_fenced` (row 38). Red: no `row_inputs`.

**am-e2e trust (`crates/am-e2e/tests/trust/taint_rows_reader_fence.rs`, new):** `row_text_never_leaves_its_fence` (row 47: canary, fake fence close, fake label line and bidi mark in a row; only its own fenced part holds them; no system, instruction or trusted part does; the fence holds in both adapters' bodies); `a_missing_row_fails_the_start_in_store_words` (row 47). Red: no `row_inputs`.

**am-store (`crates/am-store/tests/row_inputs.rs`, new; the edge test moved from 3.6's `rows.rs`):**
- `a_row_input_edge_carries_the_rows_taint` (row 38; moved).
- `a_row_edge_needs_an_existing_after_image_and_no_lower_taint` (row 47).
- `the_row_parts_text_is_not_journaled` (row 47).
- `migration_0011_keeps_every_row_and_the_pre_migration_copy`, `migration_0011_adds_a_null_from_row`, `reopening_a_version_11_database_applies_nothing`, `a_failing_0011_names_the_backup_and_leaves_version_10` (row 45).
- Red: `SCHEMA_VERSION` is 10; no `from_row`, no `row` kind, no `row_snapshot`.
- **Superseded:** 3.6a's `there_is_no_row_input_kind_and_no_from_row_column`; 3.6a's `SCHEMA_VERSION == 10` pin if it has one.

**am-engine (`crates/am-engine/tests/row_inputs.rs`, new):** `a_row_input_becomes_one_fenced_part_and_one_edge` (rows 38, 47). Red: no `am_engine::row_inputs`.

**xtask (`xtask/tests/check_deps.rs`):** `the_one_row_read_site_is_the_engines_row_inputs` (row 46: planted `row_snapshot` in `run.rs`, am-app, am-server fail; `row_history(` in `row_inputs.rs` fails; an alias of `RowSnapshot` fails; `row_inputs.rs` naming `row_snapshot` passes). **Superseded:** 3.6a's `the_shipped_tree_passes_the_three_rules`, widened. Red: no such site.

**Doctests:** 3.6a's three `compile_fail` tripwires removed (superseded); none added.

**Literals (moved from 3.6's written tests):** `row_inputs` on every `StepPlan` literal and `from_row: None` on every `StepInput` literal.

## Verification

- `cargo nextest run` per crate, filters first: am-store `row_inputs`, `rows`; am-engine `row_inputs`, `run`; am-e2e `taint_rows_reader`, `taint_rows_reader_fence`, `taint_rows_no_read_path`, the prompt and fence suites of 3.5; xtask `check_deps`.
- `cargo test --doc -p am-domain -p am-engine -p am-store` (3.6a's tripwires gone).
- `cargo xtask check-deps`: the widened rule green on shipped code and red on each planted file; 3.6a's other rules unchanged.
- `just check` at the end.
- No `cargo clean`, and no build of a workspace copy against the shared target (AGENTS.md, owner 2026-11-23).
- Merge only with both CI jobs green after review triage (owner, 2026-11-24).

## Implementation Notes

### API the tests fix (tester, 2026-11-24): the row-read items

*Moved word for word from 3.6's plan in the second re-slice (2026-11-24); 3.6a's plan keeps the rest. Lines the split changed are marked (re-slice).*

- `pub struct RowInput { pub table_id: String, pub row_id: Uuid }` (in `am_domain::rows`).
- `StepInput` gains `pub from_row: Option<i64>` (the after-image's `row_history.seq`); kind `row` (no `from_step_run`, no `from_item`); its taint is raised to at least the after-image's taint; a `from_row` naming no after-image is refused.
- `am_engine::run`: `StepPlan` gains `pub row_inputs: Vec<RowInput>`.
- A `row_inputs` entry journals a `row` input (label `row:<table>/<row id>@<version>`, `from_row` the after-image's `seq`, taint at least the row's) and puts the row's data in the prompt as `Part::Untrusted` with that label; the reading attempt is tainted.
- *(re-slice: `step_inputs.from_row` is migration 0011, `SCHEMA_VERSION` 11, report line `from version 10 to 11`; the engine reads the row through `Reader::row_snapshot`, the one content reader its rule allows, in `crates/am-engine/src/row_inputs.rs`.)*

### Red run of 3.6c

*(The tester logs it here after the ToolGate slice merges.)*
