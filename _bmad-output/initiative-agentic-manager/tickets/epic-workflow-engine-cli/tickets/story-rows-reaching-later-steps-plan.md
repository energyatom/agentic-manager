---
title: 'Rows reaching later steps'
type: feature
ticket: 22
created: '2026-11-24'
status: built
baseline_revision: '648b1b9ad5c5e9de32c354dd5a06c36b4e06671d'
after: [23]
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
- **The mark is a floor (AD-10 as restated 2026-11-24):** the input edge's taint is at least the after-image's taint; the writer raises a lower one to the row's taint (it never lowers; manager decision 2026-11-25, consistent with the store slice's raise-only rule). A tainted row read by a step leaves that step tainted, however long after; the row's own mark never changes on a read.
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

- 2026-11-25, manager: the taint floor is "raise", not "refuse". The writer raises an edge's taint to the row's after-image taint, consistent with the store and ToolGate slices (taint only rises, never lowered). The tests pin "raise". Wording elsewhere that says "refuse a lower" reads as "raise". Baseline: main 648b1b9 (3.25 merged).

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

### API the tests fix

*Tester, 2026-11-25, on Opus, on `story/3-22` over ce14439, after the store slice (entry 6) and the ToolGate slice (entry 23) merged. The tests written for 3.6 were applied from the saved patch and reconciled with the built API: the only names they missed were this slice's own (`from_row`, `row_inputs`, `RowInput`); the store's readers already had their built names (`table_rows_of`, `table_row_by_id`, `row_history_of`, `row_written_at`). New types derive `Debug, Clone, PartialEq, Eq`; the tests compare them with `assert_eq!`. Choices the plan left open are marked (tester's choice).*

**am-domain** (`am_domain::rows`):
- `pub struct RowInput { pub table_id: String, pub row_id: Uuid }`.
- The module docs' `compile_fail` tripwire for `RowInput` is removed (doc lines, done by the tester); the allowed block stays.

**am-store** (root exports):
- `StepInput` gains `pub from_row: Option<i64>`, read back by `Reader::step_inputs` (`None` on every old input and every input that is not a `row`). Its `compile_fail` tripwire and allowed block are removed (doc lines, in the applied patch).
- `pub struct RowSnapshot { pub seq: i64, pub version: i64, pub data: String, pub taint: Taint }`.
- `Reader::row_snapshot(&self, table_id: &str, row_id: &str) -> Result<Option<RowSnapshot>, StoreError>`:
  - the row's current after-image: the newest `row_history` image's `seq`, version, data as stored (equal to `table_rows.data`) and taint, after an update too;
  - `None` for an id the table does not hold, and for a row of another table asked under this one;
  - it writes nothing.
- `StartAttempt`, an input of kind `row`:
  - it needs `from_row: Some(seq)` naming an existing after-image, and no `from_step_run` or `from_item`; anything else is refused (`Err`), nothing is written, the attempt stays `queued`, and the words hold no row data;
  - its journaled taint is raised to at least that image's taint: `Clean` sent on a tainted image is journaled `Tainted`, and `Tainted` sent on a clean image stays `Tainted` (Ambiguity 1);
  - it raises the attempt's taint like any input;
  - an input of any other kind that names `from_row` is refused (tester's choice: the mirror of 3.4's "only a `gate_note` names an Inbox item").
- `check_attempt_prompt`, for a part whose label a `row` edge of the attempt holds:
  - an `Untrusted` part in the untrusted slot, taint 1, `text: None`, is accepted whatever the edge's taint, a clean row's included, and taints the attempt (the moved test's second case);
  - a `Trusted` part is refused, even on a clean edge (tester's choice, from the Boundaries: a row read never becomes a `TrustedInput`; today's rule would accept a trusted part naming any clean input);
  - a part with `text: Some(_)` is refused, in words without the text (tester's choice, from AD-43 and the Design Notes: the text stays on the after-image);
  - a `row:` part that no `row` edge names is refused (3.5's rule, unchanged).
- The journaled row part reads back as slot `untrusted`, kind `untrusted`, taint 1, `text: None`, `bytes: 0`.
- Migration 0011 (row 45):
  - `step_inputs.from_row`, a foreign key to `row_history(seq)` (as `pragma_foreign_key_list('step_inputs')` reads it), NULL on every old row;
  - `SCHEMA_VERSION > 10` (a `const` assert; no test pins it equal), the report line `from version 10 to 11`, `store.migrated` audited with an outcome starting `from version 10 to 11`;
  - AD-7's pre-migration copy opens at version 10 and holds every row, also after a reopen; reopening applies nothing and makes no new copy;
  - a `from_row` column added by hand fails the step with `MigrationFailed { from: 10, to: 11, backup: Some(_) }`, whose words name the backup, and leaves version 10 with every row;
  - a database made at exactly version 11 (`testing::database_at_version`) has `from_row` and no `document_versions`.

**am-engine**:
- `StepPlan` gains `pub row_inputs: Vec<RowInput>`. The `compile_fail` tripwire on `StepPlan` is removed (doc lines, done by the tester); the allowed read of `grants`, `seat_team`, `table`, `document` stays.
- `pub mod row_inputs` (`crates/am-engine/src/row_inputs.rs`) *(tester's choice of names, after 3.4's `gate_note`, which answers `Result<Result<_, String>, StoreError>`)*:
  - `pub struct RowRead { pub input: am_store::StepInput, pub part: am_domain::UntrustedData }`;
  - `pub fn read_rows(reader: &am_store::Reader, rows: &[RowInput]) -> Result<Result<Vec<RowRead>, String>, am_store::StoreError>`;
  - `Ok(Ok(reads))`: one read per input, in the order named, with `input == StepInput { kind: "row", from_step_run: None, from_item: None, from_row: Some(snapshot.seq), label: "row:<table>/<row id>@<version>", taint: snapshot.taint }` and `part == UntrustedData::text(label, snapshot.data)`;
  - `Ok(Err(words))` for a row the table does not hold (an unknown id, or a row of another table): the words name the table asked and the row id, never any row's data;
  - the file names no `Instruction`, `TrustedInput`, `authored`, `from_step_output`, `from_owner_note` or `Part::Trusted` (xtask's shipped-tree check).
- Behaviour the e2e tests pin:
  - each `row_inputs` entry journals its edge with the attempt's start and puts its part in slot 12, fenced, under the edge's label;
  - the reading attempt is tainted, clean row or not; the step after it gets the reader's output fenced;
  - `step.started`'s `inputs` counts the row edges, its `taint` is 1, and no audit entry holds row data;
  - a missing row fails the run before the attempt starts: the reason names the table asked and the row id; no model call; the attempt has no `started_at`, no inputs and no prompt parts; no reason, Inbox item or audit entry holds row data (the status the attempt keeps is not pinned).

**am-app**: `plan_of` gives every step `row_inputs: Vec::new()`. On the `rows-documents` fixture, `agentic run record` journals no `row` edge and no `from_row`, no `row:` prompt part, and `plain`'s request holds no row text.

**xtask** (`check-deps`, row 46):
- `row_snapshot` joins the row-49 readers, and `RowSnapshot` the row-43 records. Each may stand outside `crates/am-store/src/` only in `crates/am-engine/src/row_inputs.rs`.
- The four other readers and the two other records stay am-store's alone, in `row_inputs.rs` too.
- An alias of `RowSnapshot` (`use … as`, `type`) is a finding in every shipped file, the read site included.
- Each finding names, on one stderr line, `file:line:` and the exact token.

**Guards widened on purpose (each with its plan row):**

| Guard | Was (store and ToolGate slices) | Now | Plan row |
|-------|----------------------------------|-----|----------|
| check-deps row 49 (`xtask/tests/check_deps.rs` `no_shipped_code_reads_row_content`) | no shipped file outside am-store names a row-content reader | the same four readers; plus `row_snapshot`, outside am-store only in `crates/am-engine/src/row_inputs.rs` (its shipped-tree scan gains that check) | 46 (Code Map, xtask; Design Notes, "The check-deps rule, widened by one site") |
| check-deps row 43 records (`row_content_is_read_only_in_the_store`) | `RowRecord`, `RowHistoryRecord` only in am-store | the same two; plus `RowSnapshot`, outside am-store only in `row_inputs.rs` | 46 |
| `the_one_row_read_site_is_the_engines_row_inputs` (new) | none | the widening itself, with its planted files, supersedes 3.6a's `the_shipped_tree_passes_the_three_rules` (named `the_shipped_tree_passes_the_rules` when built; it is unchanged, since the widened rule still passes it) | 46 |
| `taint_rows_no_read_path` (am-e2e trust) | "no row content reaches a model call": no read path exists | "a row reaches a model only through the `row_inputs` slot, and there fenced and tainting": the two tests stand for steps that fill no slot (and now also pin `from_row: None` on their inputs); the new `the_slot_is_the_one_read_path_and_it_fences_and_taints` runs a reading step after them | 38, 43, 47 (Acceptance: "nothing in it reads rows through a slot"; Design Notes) |
| am-store `there_is_no_row_input_kind_and_no_from_row_column` | a `row` input is refused | removed (in the applied patch); superseded by `a_row_input_edge_carries_the_rows_taint` | 38 (Code Map, "Existing tests that must change") |
| `compile_fail` tripwires for `RowInput`, `StepPlan::row_inputs`, `StepInput::from_row` | the types are absent | removed; none added | 42 → 38 (Tests-first list, Doctests) |

`rows.rs`'s module docs now point to `tests/row_inputs.rs` for the read path; its 0010 tests pin nothing equal to version 10 (`SCHEMA_VERSION > 9`; "exactly at version 10" is made with `database_at_version`), so none changes.

**Ambiguities resolved (tester):**
1. *The taint floor: raise or refuse.* The Boundaries, matrix row 47 and the Design Notes say the writer "refuses a lower" taint. The Code Map, this plan's moved API list and the moved test (`a_row_input_edge_carries_the_rows_taint`) say the taint is "raised", as `journal_inputs` already does for a `step_output` since 3.3. The tests pin **raise**: no edge is ever journaled below its after-image's taint, which is the invariant both wordings protect. **The manager decides** whether a refusal is wanted instead; that would change the moved test's first case and the new test's `low` case, nothing else.
2. *A row part's text* is refused by the store, not merely left out by the engine (tester's choice, above).
3. *A trusted part on a clean row edge* is refused by the store (tester's choice, above). Without it, 3.5's label check would accept `Part::Trusted` for a clean row.
4. *The order of row parts in slot 12*, relative to a fenced producer output and the plan's own inputs, is not pinned; each row is one fenced part under its own label.
5. *A missing row's attempt status* is not pinned (3.4's `FailBeforeStart` keeps it `queued`); only "never started" is.

**Owed by the implementer outside the tests (src, not changed by the tester):**
- `row_inputs: Vec::new()` on the two `StepPlan` literals in `crates/am-app/src/run.rs` and on the unit-test plan in `crates/am-engine/src/run.rs` (`#[cfg(test)]`).
- `from_row` on the shipped `StepInput` literals: `crates/am-store/src/lib.rs` (two), `crates/am-engine/src/journal.rs`, `crates/am-engine/src/run.rs` (two), `crates/am-server/src/inbox.rs`.
- Stale words: `StepInput`'s doc (its kinds list); the finding of the row-reader rule in `xtask/src/deps.rs` ("no step reads rows before story 3.6c"); the `justfile` `doctest` comment ("no row-input type exists", "a StepPlan has no row-input slot").

**Checked against stubs** (as the earlier slices did):
- A copy of this working tree in the tester's scratch folder, built with its **own** `CARGO_TARGET_DIR`. The shared cache was not cleaned, and no copy was built against it.
- Signature-only stubs of the API above, then a minimal implementation, made only in the copy:
  - migration 0011;
  - the `row` kind and `row_snapshot`;
  - the prompt-part rules;
  - `read_rows` wired into `inputs_of`;
  - the widened check-deps rule.
- With signature-only stubs, these pass with no finding:
  - `cargo check --tests` and `cargo clippy --tests -- -D warnings` for am-store, am-engine, am-app, am-e2e, am-domain, am-server and xtask;
  - `cargo test --doc -p am-domain -p am-engine -p am-store`.
- With the minimal implementation, all of these pass:
  - am-store's 9 `row_inputs` tests, and all 236 am-store tests;
  - am-engine's 2 `row_inputs` tests;
  - the 8 tests of am-e2e `taint_rows_reader`, `taint_rows_reader_fence` and `taint_rows_no_read_path`;
  - am-app `run_rows` (6);
  - xtask `check_deps` (43);
  - every am-engine, am-e2e, am-app and am-domain test (840 run, 840 passed, 6 skipped), so no other suite depends on the absence of a read path.
- So a test that does not compile after the build means the build differs from this API list.

**Review triage round 1 (tester, 2026-11-25): what the round-1 tests add to this API.** The items above stand.

- **R1, check-deps** (`the_read_sites_items_stand_only_in_the_engines_driver`):
  - `read_rows` and `RowRead` stand in shipped code only in `crates/am-engine/src/row_inputs.rs` (their definitions) and `crates/am-engine/src/run.rs`;
  - in `run.rs`, `read_rows` stands only as a call (followed by `(`), never as a function value;
  - neither is renamed (`use … as`, `type`) or re-exported in any file, the two allowed files included;
  - `row_inputs.rs` holds exactly one `.row_snapshot(`;
  - each finding names, on one stderr line, `file:line:` and the exact token, and spells no allowed file's path (the round-0 rule's habit: "am-engine's row_inputs module").
- **R2, the fence** (am-providers `labels.rs`; am-e2e `an_escaped_closing_tag_in_a_row_cannot_close_its_fence`):
  - `render_untrusted_text` neutralises `<\/untrusted-` and `\u003c/untrusted-` (either hex case), in any letter case, as it does `</untrusted-`;
  - the fence's only closing tag, in any form, is the wrapper's own, at its end;
  - the data around each form is kept, and a row keeps the forms as written;
  - `neutralise` is private (`mod common`), so the unit-level test goes through `render_untrusted_text`, which both adapters use.
  - *Tester's addition, for the manager to confirm or drop* (`every_json_escape_of_a_closing_tag_is_neutralised`): the same class with the slash escaped, `<\u002funtrusted-` and `\u003c\u002funtrusted-`, and the two escapes together, `\u003c\/untrusted-`. None of these contains `<\/` or `\u003c/`, so R2's two forms alone leave them open.
- **R3:** a plan whose gate step names `row_inputs` is refused by `run_plan` with an `Err` that names the step key, before any run row. As for a gate's `table:`, the variant is the implementer's (`RunError::Writes` fits).
- **R4:**
  - `read_rows` answers `Ok(Err(words))` for a row named twice, whatever stands between; the words name the table and the row id, and hold no row data;
  - in a run, the step fails before it starts: `run_plan` returns `Ok(outcome)` with `failed`, a Failure item (`outcome.item`), no model call, and no `started_at`, inputs or prompt parts;
  - the reason names the table and the row id.
- **R5, 0011's guards** (tester's choice of words):
  - a `BEFORE UPDATE` on a row edge (`OLD.from_row IS NOT NULL`) aborts any change, with `a row input never changes`: taint, kind, label, attempt, time. A change of `from_row` may abort with that guard or the existing `after-image never changes`.
  - The insert guard aborts a label that is not the after-image's own `row:<table>/<row id>@<version>`, with `a row input's label names its own after-image`.
  - An `AFTER INSERT` raises `step_runs.taint` to 1 for a row edge, a clean one too.
  - *Tester's addition, for the manager:* an `UPDATE` that turns another input into a `row` edge (`SET kind = 'row'`) is aborted, with any words. Today neither trigger sees it, and it would leave a `row` edge with no `from_row`.
- **R6:** `StartAttempt` refuses a row edge labelled with another version, row or table, or with no row label. This is the implementer's existing check, now tested; it was green on arrival.
- **R7:**
  - `StartAttempt` refuses a row edge that names an older after-image of its row, even when the label is that image's own, with `StoreError::RowRefused(words)`. The words name the row id and hold no row data (tester's choice, so the engine tells a moved row from any other refusal).
  - The engine then reads the rows again and starts once more; a second `RowRefused` fails the step before it starts, in words naming the table and the row id (3.4's `FailBeforeStart` path fits).
  - **The seam (no new API):** the tests force the move through `RunPlan.clock`. The engine must read the plan's clock **once per try** of an attempt's start, after reading that try's rows and before its `StartAttempt`. Today it does exactly that, once: `prompt::assemble(.., self.plan.clock.now())` sits between `inputs_of` and `StartAttempt`.
  - So a retry re-reads the rows and re-assembles, reading the clock again; it must not reuse the first try's `now` or read the clock before the rows. The tests count the readings: 2 for one move (two tries), and 2, not 3, for two moves.
  - If the implementer prefers an explicit seam, the smallest is a test-only `run_plan` variant over a `Clock` (as `run_plan_with_tools` takes a runner, pinned by seam-guard); the tests would then change by one call.

**Review triage round 2 (tester, 2026-11-25): what the round-2 tests add to this API.** The items above stand, except where this block changes them.

- **T1, the read site's decoding** (`row_inputs::tests::printable_escapes_are_decoded_once_and_the_rest_is_kept`; am-e2e `an_escaped_letter_of_the_tag_cannot_close_its_fence`):
  - `RowRead.part`'s text is the row's data after **one** pass of JSON unescaping. A `\uXXXX` of a printable ASCII character (0x20–0x7E, either hex case) and `\/` become the character.
  - Every other escape stays exactly as stored, and so does all other text, number text included. That covers an escaped backslash (and the `u0041` after it stays text), an escaped quote, and a `\uXXXX` of a control or non-ASCII character. A big integer and `1e2` reach the prompt unchanged.
  - The pinned pair: `{"title":"a\u0041b\u004ac\u004Ad<\/e\u00e9f\u001fg\\u0041h\"i","n":12345678901234567890123,"x":1e2}` gives `{"title":"aAbJcJd</e\u00e9f\u001fg\\u0041h\"i","n":12345678901234567890123,"x":1e2}`.
  - Whether `\u0022` and `\u005c` are decoded is left to the implementer; the pinned pair has neither.
  - The edge, its label and the stored row are unchanged.
  - In both adapters' bodies, the fence of a row whose tag letters are escaped has one closing tag, the wrapper's, and decoding it once more reveals none. No escaped form stands as written.
  - *Changed meaning:* round 1's `an_escaped_closing_tag_in_a_row_cannot_close_its_fence` compared the part with the stored data; it now compares it with the data decoded once.
  - *Dropped (manager, 2026-11-25; D3):* the tester's double-escape test. Decoding to a fixpoint would change real row text that holds backslashes; one pass matches JSON's meaning.
- **T2, placement:**
  - `pub(crate) mod row_inputs` in am-engine's `lib.rs`, and no `pub mod row_inputs`.
  - Its tests are an in-crate `#[cfg(test)] mod tests` at the end of `crates/am-engine/src/row_inputs.rs` (test code in a src file, as the manager allowed). `crates/am-engine/tests/row_inputs.rs` is deleted.
  - The moved tests name none of the store's other row readers, its row command or its records: rows are made, and read back, by raw SQL on the store's own database (rusqlite is am-engine's dev-dependency). So check-deps needs **no** test-range exemption, and the real tree passes it today.
  - check-deps rules:
    - the one `.row_snapshot(` call sits inside `fn read_rows`;
    - in `run.rs`, every `read_rows` (a call) and every `RowRead` sits inside `fn inputs_of`;
    - the read site's `#[cfg(test)]` module may call `read_rows` and name `RowRead`.
  - Findings name `file:line:` and the token on one line, and spell no allowed path.
  - *Changed meaning:* round 1's allowed driver (`RUN_READS_ROWS`) now names its function `fn inputs_of` (was `fn inputs`). Its planted forms are unchanged.
- **T3** (as the manager narrowed it, 2026-11-25): a row's text reaches a `TrustedInput` only if it is first taken out of its `UntrustedData`. So `fn inputs_of` names none of these:
  - `content`
  - `into_content`
  - `write!`
  - `{:?}` (a part's text through `Debug`; the rule must read it inside a format string, which check-deps' comment-and-string stripping removes today).
  - A planted `run.rs` naming each inside its row-read loop fails.
  - `TrustedInput` and `from_step_output` are not banned: the shipped `inputs_of` builds a step output's and a loop-back's.
  - `format!` is left out: the shipped `inputs_of` calls it twice for labels, `format!("step:{}@{}", prev.step_key, prev.attempt)` and `format!("loop-back:{}@{}", producer.step_key, producer.attempt)`.
  - On the shipped tree `fn inputs_of` names none of the four, and passes today.
- **T4:** a `BEFORE INSERT` on `step_inputs` aborts an insert whose `id` exists, with `a step input is never replaced` (tester's choice of words). This holds with `recursive_triggers` off and on, whether the existing row edge is replaced by a clean plan input, a clean `gate_note` or the very same edge. Every input and the attempt's taint stay as they were.
- **T5:** `render_untrusted_text` neutralises `<`, optional whitespace, `/`, optional whitespace, `untrusted`, in any letter case, with or without the hyphen: `</ untrusted-`, `< /untrusted-`, `</\nuntrusted-`, `</untrusted>`, `<  /  UNTRUSTED-`, `</\tUntrusted >`. The fence then holds exactly one such tag, the wrapper's.
  - 3.5's literal-form output is pinned byte for byte in 3.5's own `a_label_cannot_span_lines_or_hide_characters` (`taint_prompt_fence.rs`): `render_untrusted_text("test:page", HOSTILE)`, as the shipped neutraliser renders it today.

### Red run (2026-11-25)

*Tester, on Opus, in the real working tree with the shared cache (no `cargo clean`, no `--workspace`). Nothing recorded in git; no `src/` change but the two tripwires' doc-comment lines; no dependency change (am-e2e's `Cargo.toml` gains one `[[test]]` target, `taint_rows_reader_fence`).*

**Commands:**
```
cargo fmt --all
cargo check --keep-going --tests -p am-store -p am-engine -p am-app --message-format short
cargo check --keep-going --tests -p am-e2e --message-format short
cargo nextest run -p xtask --test check_deps --no-fail-fast
cargo nextest run -p am-store --test rows
cargo test --doc -p am-domain -p am-engine -p am-store
```

**Red, with the reason:**

| Target | Tests | Red because |
|--------|-------|-------------|
| am-store `row_inputs` (new; 1 moved, 8 new) | `a_row_input_edge_carries_the_rows_taint`, `row_snapshot_is_the_rows_current_after_image`, `a_row_edge_needs_an_existing_after_image_and_no_lower_taint`, `the_row_parts_text_is_not_journaled`, `a_row_edge_is_never_a_trusted_part`, `migration_0011_keeps_every_row_and_the_pre_migration_copy`, `migration_0011_adds_a_null_from_row`, `reopening_a_version_11_database_applies_nothing`, `a_failing_0011_names_the_backup_and_leaves_version_10` | does not compile: no `am_store::RowSnapshot` (E0432), no `StepInput::from_row` (E0560, 8 places), no `Reader::row_snapshot` (E0599, 6 places). Under `cargo test` the `const` assert `SCHEMA_VERSION > 10` also fails at build (E0080, `SCHEMA_VERSION` is 10). |
| am-store `inbox` (9), `inbox_resolve` (4), `prompt_journal` (1), `run_journal` (2) | the existing suites | do not compile: `from_row: None` on their `StepInput` literals (E0560) |
| am-store `rows` | 59 | **green** (its no-read-path test is removed; nothing else changed) |
| am-engine `row_inputs` (new) | `a_row_input_becomes_one_fenced_part_and_one_edge`, `a_row_the_table_does_not_hold_is_refused_in_store_words` | does not compile: no `am_engine::row_inputs`, no `am_domain::rows::RowInput`, no `am_store::RowSnapshot` (E0432); no `from_row` (E0560); no `row_snapshot` (E0599, 2) |
| am-engine `run`, `loop_caps`, `toolgate` | the existing suites | do not compile: `row_inputs: vec![]` on their `StepPlan` literals (E0560) |
| am-app `run_rows` | `agentic_run_leaves_the_row_input_slot_empty` (new) and the suite | does not compile: no field `from_row` on `StepInput` (E0609) |
| am-app `gate_cli` | the existing suite | does not compile: `from_row: None` (E0560, 2) |
| am-e2e, 29 of 51 targets | every target that includes `tests/engine/support/mod.rs`, among them `taint_rows_reader` (3: `a_row_written_by_a_tainted_step_taints_a_later_reader`, `a_clean_rows_edge_is_clean_and_its_text_still_fenced`, `a_clean_row_read_still_taints_the_reader`), `taint_rows_reader_fence` (new, 2: `row_text_never_leaves_its_fence`, `a_missing_row_fails_the_start_in_store_words`), `taint_rows_no_read_path` (2 existing plus `the_slot_is_the_one_read_path_and_it_fences_and_taints`) | do not compile: `support/mod.rs` imports `am_domain::rows::RowInput` (E0432) and sets `row_inputs` (E0560, 2); the `from_row: None` literals in `handoff_gate`, `handoff_gate_resolve`, `handoff_loop_back`, `handoff_next`, `rule_15_inbox`, `rule_16_audit_log`, `taint_at_rest`, `taint_handoff_chain`, `taint_prompt_type_wall` (E0560); reads of `from_row` in `taint_rows_reader`, `taint_rows_no_read_path` (E0609) |
| xtask `check_deps` | `the_one_row_read_site_is_the_engines_row_inputs` (new) | fails at run time, 42 of 43 pass: the allowed tree passes, then the first planted form, `row_snapshot` in `crates/am-engine/src/run.rs`, passes check-deps ("row_snapshot in the engine's driver: must fail"): the rule does not know `row_snapshot` yet. The widened scans of `no_shipped_code_reads_row_content` and `row_content_is_read_only_in_the_store` are green on arrival (nothing names `row_snapshot` or `RowSnapshot` yet). |
| doctests am-domain, am-engine, am-store | 8, 14, 1, 3, 6 | **green**: the three tripwires are removed, none added |

**Test lines** (working tree against ce14439, `git diff --numstat` plus the four new files): about 2,650 added and 75 removed in test files and the am-e2e manifest. Of these, the applied patch brought about 700: `taint_rows_reader.rs` 208, `row_inputs.rs` 416 as applied, and the literals. The tester's own lines are about 1,950:
- am-store `row_inputs.rs` +946: about 230 lines of version-10 fixture and helpers (after `rows.rs`'s), about 300 of the four migration tests, the rest the store read tests;
- am-engine `row_inputs.rs` 237;
- `taint_rows_reader_fence.rs` 323;
- `taint_rows_no_read_path.rs` +127 and −21;
- `run_rows.rs` +58;
- `check_deps.rs` +278 and −2.

That is over the plan's 800 to 1,000 test-line estimate, mostly because of the migration fixture and the e2e fence test. No code lines: `src/` changed only in doc-comment lines (33 removed, 14 of them in the applied patch; 2 added).

### Implementation (2026-11-25)

*Implementer, on Opus, on `story/3-22` over 5d96e9f, in the real working tree with the shared cache (no `cargo clean`, no `--workspace`, no copy). Nothing recorded in git. No test file changed; no dependency added; no new caller of `Instruction::authored`.*

**Built, as the API list above fixes it:**
- **am-domain** `rows.rs`: `RowInput { table_id, row_id }` (derives `Debug, Clone, PartialEq, Eq`; ids only).
- **am-store:**
  - `migrations/0011_row_inputs.sql`: `step_inputs.from_row INTEGER REFERENCES row_history(seq)`; `SCHEMA_VERSION` 11. Two guards, beside the writer's checks: an insert trigger (only a `row` input names an after-image and every `row` input names one; no `from_step_run`, no `from_item`; taint not below the image's) and an update trigger (`from_row` never changes).
  - `StepInput.from_row`, read back by `Reader::step_inputs`; its docs list the `row` kind.
  - `journal_inputs`: kind `row` (`INPUT_KINDS` gains it). `row_input` refuses a `from_step_run`, a `from_item`, a missing or unknown `from_row`, and a label other than the image's own `row:<table>/<row id>@<version>`, in words with store ids only. The edge's taint is **raised** to the image's (never refused or lowered), and the attempt is raised to tainted by any `row` edge. Any other kind naming `from_row` is refused.
  - `check_attempt_prompt`: a part whose label a `row` edge of the attempt holds must be `Untrusted` (a `Trusted` part is refused, a clean edge's too) and carry no text. It is accepted whatever the edge's taint. Every other part keeps 3.5's rule.
  - `rows.rs`: `RowSnapshot { seq, version, data, taint }` and `Reader::row_snapshot` (the current after-image, joined on the row's version; `None` for an id the table does not hold).
- **am-engine:**
  - `row_inputs.rs` (new): `RowRead { input, part }` and `read_rows`, as fixed. It names no `Instruction`, `TrustedInput`, `authored`, `from_step_output`, `from_owner_note` or `Part::Trusted`.
  - `run.rs`: `StepPlan.row_inputs`. `inputs_of` reads the rows after the gate notes and before the plan's own inputs. It adds each edge and its fenced part (slot 12, `text: None` through the assembler's produced-input record), and raises the attempt to tainted. A missing row sets `refused`, so 3.4's `FailBeforeStart` path fails the run in words naming the table and row id.
- **am-app** `plan_of`: `row_inputs: Vec::new()` on both literals.
- **xtask** `deps.rs`:
  - `row_snapshot` and `RowSnapshot` stand outside `crates/am-store/src/` only in `crates/am-engine/src/row_inputs.rs`.
  - An alias of `RowSnapshot` is a finding in every file.
  - The four other readers and two other records are unchanged.
  - The finding texts name the site as "am-engine's row_inputs module" (the planted test refuses a finding that spells an allowed file's path).

**Implementer's choices (mechanical, none changes an AD or an owner answer):**
1. `RowSnapshot`'s and `RowRead`'s `Debug` print no row data (byte count or label only), after the store slice's review T6. The tester's "derive `Debug`" is satisfied by a hand-written impl.
2. The store checks a `row` edge's label against its after-image, so the prompt-part rule's label lookup cannot be pointed at another image.
3. The store taints the attempt for any `row` edge, also without a prompt (a gate's `PauseStep`, an empty prompt). This follows AD-10: reading any row taints the reader. It only raises.
4. Schema guards in 0011, mirroring the writer's checks (the 0010 pattern).

**Owed items done:**
- `from_row: None` on the shipped `StepInput` literals: am-store `lib.rs` (the gate-note literal; the reader reads the column), am-engine `journal.rs` and `run.rs` (two), am-server `inbox.rs`.
- `row_inputs: vec![]` on `run.rs`'s `#[cfg(test)]` plan.
- Stale words: `StepInput`'s docs; the row-reader finding ("no step reads rows before story 3.6c"); check-deps' success line; the `deps.rs` module docs; the `justfile` `doctest` comment.

**Size (owner rule: code and test lines apart):**
- **Code:** about +416 and −34 lines, comments and docs included, well under the ~4,000-line stop.
  - New files: `row_inputs.rs` 79, `0011_row_inputs.sql` 28.
  - am-store `lib.rs` +101/−28 (14 of the removed lines are the tester's doctest lines), `rows.rs` +79, `migrations.rs` +1.
  - am-engine `run.rs` +36/−11 (9 of them the tester's), `lib.rs` +4/−1, `journal.rs` +1.
  - am-domain `rows.rs` +14/−9 (the tester's included).
  - am-app `run.rs` +6; am-server `inbox.rs` +1; xtask `deps.rs` +64/−14; `justfile` +3/−3.
  - Within the plan's 400 to 600 estimate.
- **Tests:** none written or changed by the implementer; the tester's about 2,650 added and 75 removed stand (Red run).

**Verification:**
- Per crate, each fully green:
  - am-store: 236 tests;
  - am-engine and am-domain: 124;
  - am-e2e, am-app and am-server: 728 run, 6 skipped;
  - xtask `check_deps`: 43.
- `cargo test --doc -p am-domain -p am-store -p am-engine`: am-domain 8 and 14, am-engine 1, am-store 3 and 6, all passed.
- `just check`: exit 0.
  - fmt-check and clippy `-D warnings` clean.
  - nextest: 1,621 tests run, 1,621 passed, 8 skipped.
  - Doctests green.
  - check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard and register all pass.
  - `agentic validate` passes: `templates/samples/company-a/` gives 0 errors and 1 warning, the known 3.14 fan-out `table:` warning.
- After that run, only check-deps' success line changed (a string literal). `cargo fmt --check`, `cargo xtask check-deps`, xtask `check_deps` (43) and `cargo clippy -p xtask --all-targets -D warnings` were re-run green.

### Review triage round 1 (manager, 2026-11-25)

Two Opus reviews ran: correctness (C) and adversarial security (S). Neither found a way to get row text unfenced or into an Instruction or `TrustedInput`, or a writer path that leaves a reader untainted. Migration 0011 is additive and blocks no old path. The fixes below are written tests first.

| Id | From | Finding | Decision |
|----|------|---------|----------|
| R1 | C1, S1 (medium) | `pub fn read_rows`, `RowRead` and its `.part` sit under no rule. am-app and am-server could call them, and any engine file could pass a row's text to a trusted constructor. | Fix: check-deps allows `read_rows` and `RowRead` only in `crates/am-engine/src/run.rs` (and their definitions in `row_inputs.rs`). The read site holds exactly one `.row_snapshot(` call. Planted-file tests cover another engine file, am-app, am-server, an alias, a function value, and a second call in `row_inputs.rs`. |
| R2 | S2 (medium-low) | `neutralise` matches only the literal `</untrusted-`. Row data is kept as written (SQLite `json()`), so a JSON-escaped close tag (`<\/untrusted-data>`, or `</untrusted-`, either hex case) reaches the fence unneutralised. | Fix in am-providers `neutralise` (3.5's fence; it only narrows, decided by the manager under the standing rules): neutralise the escaped forms `<\/untrusted-` and `</untrusted-` / `</untrusted-`, in any letter case, as well as the literal one. Add both forms to `taint_rows_reader_fence` for both adapters. |
| R3 | C2 | A gate step with `row_inputs` reads its rows and taints the gate, although no model or person sees them. | Fix: `check_writes` refuses `row_inputs` on a gate, in words, as it refuses `table:`. Test. |
| R4 | C3 | The same row named twice fails as a store error with no Failure item. At a gate it is journaled twice silently. | Fix: `read_rows` refuses a repeated row in words (`Ok(Err(words))`). Test. |
| R5 | C4, S3 | 0011's triggers guard inserts only. A raw `UPDATE` can lower a row edge's taint or change its kind; an insert can carry a label that does not match its row; and inserting a row edge does not raise `step_runs.taint`. | Fix: a `BEFORE UPDATE` trigger aborts any change to a row edge (`OLD.from_row IS NOT NULL`). The insert trigger checks the label against the after-image. An `AFTER INSERT` trigger raises the attempt's taint for a row edge. Raw-SQL tests for each. |
| R6 | C5, S5 | Two of the implementer's guards have no tests: the label/version-against-image refusal, and "a gate that reads a row is tainted" (now refused by R3). | Fix: a `StartAttempt` test with a mismatched label, covered with R5's raw-SQL tests. |
| R7 | C6, S4 | Rows are read on a separate reader before `StartAttempt`. A concurrent update can leave the edge naming an older after-image, though the plan says "in the transaction that journals the edges". | Fix: `StartAttempt` refuses a row edge whose `from_row` is not the row's current after-image. The engine then re-reads the rows and retries once. A second mismatch fails the step in words. Lineage then always names the image that was sent, and it is current at commit. Tests: a forced concurrent update between the read and the start gives one retry, then success, and a second forced update gives a worded failure. |
| R8 | C7 | Stale comments: `run.rs:1180`, and 0011's header saying "one guard". | Fix the comments. |

**Accepted:**
- **D1 (S3, part):** no `BEFORE DELETE` on `step_inputs`. AD-25 requires that deleting a run really deletes its content (as for `tool_calls`, D1 of the store slice).

### Red run round 1 (2026-11-25)

*Tester, on Opus, in the real working tree with the shared cache, over the implementer's build (no `cargo clean`, no `--workspace`, no copy). Nothing recorded in git; no `src/` change; no dependency change. am-e2e's `Cargo.toml` gains one `[[test]]` target, `taint_rows_reader_start`. Every new test compiles against the built API; each red one fails at run time for the reason given.*

**Commands:**
```
cargo fmt --all
cargo nextest run -p am-store --test row_inputs --no-fail-fast
cargo nextest run -p am-engine --test row_inputs --no-fail-fast
cargo nextest run -p am-e2e --test taint_rows_reader_start --test taint_rows_reader_fence --no-fail-fast
cargo nextest run -p am-providers --test labels --no-fail-fast
cargo nextest run -p xtask --test check_deps --no-fail-fast
cargo clippy --tests -p am-store -p am-engine -p am-e2e -p am-providers -p xtask -- -D warnings
```
`fmt` and `clippy -D warnings` are clean.

**Test by test:**

| Row | Test (file) | Result now | Red because |
|-----|-------------|------------|-------------|
| R1 | `the_read_sites_items_stand_only_in_the_engines_driver` (xtask `check_deps`) | red (43 of 44 pass) | The allowed tree passes. The first planted form, `read_rows` called in `crates/am-engine/src/gate.rs`, passes check-deps: "read_rows called in another engine file: must fail". No rule names `read_rows` or `RowRead` yet. Its 11 other planted forms and the shipped-tree checks run after it. |
| R2 | `an_escaped_closing_tag_cannot_close_a_fence` (am-providers `labels`) | red | `<\/untrusted-` comes through `render_untrusted_text` as written. |
| R2 | `every_json_escape_of_a_closing_tag_is_neutralised` (am-providers `labels`; tester's addition) | red | `<\u002funtrusted-` comes through as written. |
| R2 | `an_escaped_closing_tag_in_a_row_cannot_close_its_fence` (am-e2e `taint_rows_reader_fence`) | red | The row keeps all three forms as written (that precondition passes). In the Anthropic body, `<\/untrusted-data>` and `\u003c/…`, `\u003C/…` stand in the fence as written. |
| R3 | `a_gate_that_names_rows_is_refused_before_any_run` (am-e2e `taint_rows_reader_start`) | red | `run_plan` is `Ok`: the gate read the row and waits (`waiting_human`, with a Review item), so a run row exists. |
| R4 | `a_row_named_twice_is_refused_in_store_words` (am-engine `row_inputs`) | red | `read_rows` answers `Ok(Ok(reads))` with the same edge twice. |
| R4 | `a_row_named_twice_fails_the_run_in_words` (am-e2e `taint_rows_reader_start`) | red | `run_plan` is `Err(Stopped { status: "failed", reason: None, source: Invalid("… is a second part labelled so in the untrusted slot …") })`: a bare store error, with no Failure item. |
| R5 | `a_row_edge_never_changes_once_journaled` (am-store `row_inputs`) | red | `UPDATE step_inputs SET taint = 0 WHERE kind = 'row'` changes 1 row: "lowering its taint: must abort". |
| R5 | `a_raw_row_edge_must_carry_its_after_images_label` (am-store `row_inputs`) | red | A raw insert labelled `@1` naming the version-2 image changes 1 row: "another version: must abort". |
| R5 | `a_raw_row_edge_taints_its_attempt` (am-store `row_inputs`) | red | After the raw insert the attempt's taint is still `Clean`. |
| R6 | `a_start_whose_row_edge_is_mislabelled_is_refused` (am-store `row_inputs`) | **green on arrival** | It tests the implementer's existing label check (Implementation, choice 2); the control starts. |
| R7 | `a_start_whose_row_edge_names_an_older_image_is_refused` (am-store `row_inputs`) | red | `StartAttempt` accepts an edge naming version 1 of a row at version 2. |
| R7 | `a_row_that_moves_before_the_start_is_read_again_once` (am-e2e `taint_rows_reader_start`) | red | "two tries": the clock was read once. The start took the stale edge, so there was no retry. |
| R7 | `a_row_that_moves_again_fails_the_step_in_words` (am-e2e `taint_rows_reader_start`) | red | The run `succeeded`, where it should fail. |

Every earlier test in these targets stays green: am-store `row_inputs` 10, am-engine `row_inputs` 2, am-e2e `taint_rows_reader_fence` 2, am-providers `labels` 4, xtask `check_deps` 43.

**Not checked against a stub implementation.** Unlike round 0, the R1 rule, the R2 neutraliser and the R7 retry were not built in a scratch copy. The R7 tests rest on the clock seam above.

**Test lines (round 1):** about 1,210 added and none removed:
- am-store `row_inputs.rs` about +370;
- am-e2e `taint_rows_reader_start.rs` 421 (new);
- `check_deps.rs` about +225;
- `taint_rows_reader_fence.rs` about +92;
- am-providers `labels.rs` +70;
- am-engine `row_inputs.rs` +27;
- the am-e2e manifest +4.

### Implementation round 1 (2026-11-25)

*Implementer, on Opus, over round 0 in the real working tree with the shared cache (no `cargo clean`, no `--workspace`, no copy). Nothing recorded in git. No test file changed, no dependency added, no new caller of `Instruction::authored`. The manager's two confirmations are built: the `/` forms (R2) and the abort on `UPDATE … SET kind='row'` (R5).*

**Fixes:**
- **R1 (xtask `deps.rs`):** new `read_path_findings`.
  - `read_rows` and `RowRead` (`READ_ITEMS`) stand only in `crates/am-engine/src/row_inputs.rs` (their definitions) and in `crates/am-engine/src/run.rs`.
  - In `run.rs`, `read_rows` must be followed by `(`, so it is a call, never a value or an import.
  - An alias of either (`use … as`, `type`) is a finding in every file.
  - `row_inputs.rs` holds exactly one `row_snapshot` token, and it is a `.` method call.
  - The finding texts name modules, not the allowed paths. The success line says it.
- **R2 (am-providers `common.rs`, `neutralise`; both adapters render through it):**
  - Any letter case or hex case of `<` or `<`, followed by `/`, `\/` or `/`, then `untrusted-`, is neutralised.
  - Its `<` (or escape) becomes `&lt;`, and the letters after it are kept. The literal form's output is unchanged.
- **R3 (am-engine `check_writes`):** a gate with `row_inputs` is refused before any run row: `RunError::Writes`, "it is a gate and names rows to read; …".
- **R4 (`read_rows`):** a row named twice gives `Ok(Err(words))` naming the row id and table, before any read.
- **R5 (0011, edited in place since it is unmerged):** four triggers.
  - `step_inputs_row_edge`, the insert guard, is unchanged.
  - `step_inputs_row_label`: an insert's label must be its image's own `row:<table>/<row id>@<version>`. Words: "a row input's label names its own after-image".
  - `step_inputs_row_taints_reader`: `AFTER INSERT` of a `row` edge sets `step_runs.taint = 1`.
  - `step_inputs_row_fixed`: `BEFORE UPDATE` aborts when the old or new kind is `row`, or the old or new `from_row` is set. Words: "a row input never changes, and no other input becomes one". It replaces round 0's `from_row` update guard.
  - The header is reworded (R8).
- **R6:** green on arrival.
- **R7:**
  - **am-store:** `row_input` refuses an edge whose image is not its row's current one: `StoreError::RowRefused`, naming the row, its table and its version now (`rows::image_moved`).
  - **am-engine:** `run_step` starts in a two-try loop. Each try runs `inputs_of`, then reads the clock once in `prompt::assemble`, then sends `StartAttempt`. On `RowRefused`, with row inputs, it retries once. A second refusal goes through the new `fail_before_start` helper (the Failure item path, extracted unchanged) with words naming the row and table.
- **R8:** `run_step`'s fail-closed comment now covers row refusals. 0011's header is reworded. `check_writes`'s and `RunError::Writes`'s docs mention the gate refusal.

**Code lines:**
- **Round 1:** about +288 and −82; the `run_step` loop's re-indent is most of `run.rs`'s +123/−75. Per file:
  - am-providers `common.rs` +42/−6;
  - am-store `lib.rs` +9, `rows.rs` +21;
  - 0011 +20;
  - am-engine `row_inputs.rs` +13;
  - xtask `deps.rs` +60/−1.
- **Story total:** about +705 and −148 against the baseline, 33 of those removed lines being the tester's doctest lines. That is well under the ~4,000-line stop.
- **Tests:** none written by the implementer.

**Verification:**
- `just check`: exit 0.
  - fmt-check and clippy `-D warnings` clean.
  - nextest: 1,635 tests run, 1,635 passed, 8 skipped.
  - Doctests green.
  - check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard and register all pass.
  - `agentic validate`: 0 errors; `company-a` and `rows-documents` each give their 1 known warning.
- `cargo test --doc -p am-store -p am-engine -p am-providers`: am-engine 1, am-providers 0, am-store 3 and 6, all passed.
- Per target, before the full run: am-store `row_inputs` 14/14; am-engine and am-e2e 424 passed, 5 skipped; am-providers 25/25; xtask `check_deps` 44/44.

### Review triage round 2 (manager, 2026-11-25)

A quick Opus follow-up review confirmed that R3–R8 hold, found no regression, and judged R1 and R2 partial. Every fix below is written tests first.

| Id | From | Finding | Decision |
|----|------|---------|----------|
| T1 | F1 | A JSON escape of any letter of the tag (`untrusted`, `t`…) still gets past `neutralise`, and SQLite `json()` keeps these escapes as written. | Fix at the read site (`row_inputs.rs`), before the part is built: decode the JSON escapes `\uXXXX` that stand for printable ASCII (0x20–0x7E), and `\/`, into their literal characters. The rest of the text, number text included, is left exactly as stored. This is not a re-serialisation. `neutralise` then catches the literal form. Tests: every escaped-letter form, in both adapters' bodies; big integers and `1e2` reach the prompt unchanged. |
| T2 | F2 | The read-site rule counts names, not call placement. A helper wrapping the snapshot, a new `pub fn`, or a `run.rs` function returning `RowRead` can reach am-app or am-server. | Fix: `pub(crate) mod row_inputs`. Its tests become in-crate unit tests (`#[cfg(test)]`), moved by the tester. check-deps rules: the snapshot call sits inside `fn read_rows`, and `read_rows(` is called only inside `fn inputs_of` in `run.rs`. Planted-file tests. |
| T3 | F3 | Nothing ties `run.rs`'s use of a read to the fenced slot-12 path. `run.rs` could take `.content()` into a `TrustedInput` under another label. | Fix: a check-deps rule that `fn inputs_of` names no `content`, `into_content`, `TrustedInput` or `from_step_output` near a row read. If the whole of `run.rs` can carry the ban with no legitimate use, ban it file-wide. The implementer reports which, with its reason. Planted-file test. |
| T4 | F4 | `INSERT OR REPLACE INTO step_inputs` by an existing id swaps a tainted row edge for a clean one (the 3.5 lesson). | Fix: in 0011 (not merged), a `BEFORE INSERT` abort when a `step_inputs` row with `NEW.id` exists, with or without `recursive_triggers`. Test. |
| T5 | F5 (pre-existing, 3.5) | `neutralise` misses whitespace around `/` (`</ untrusted-`, `< /untrusted-`, `</\nuntrusted-`) and a closing tag without the hyphen (`</untrusted>`). | Fix: treat `<`, optional whitespace, `/`, optional whitespace, then `untrusted` in any case, as closing, and neutralise it. 3.5's literal-form output must stay byte for byte unchanged. Tests. |

**Accepted:**
- **D2 (F5, part).** A fullwidth `＜`, a U+2011 hyphen, or a zero-width space inside the tag is not neutralised. These are not tags a model reliably reads as the fence's close, and the wrapper's own literal close tag stays the only structural one.

### Red run round 2 (2026-11-25)

*Tester, on Opus, in the real working tree with the shared cache, over the implementer's round-1 build (no `cargo clean`, no `--workspace`, no copy). Nothing recorded in git; no dependency change.*

**`src/` change (test code):**
- A `#[cfg(test)] mod tests` is appended to `crates/am-engine/src/row_inputs.rs`. It holds the three tests moved from `crates/am-engine/tests/row_inputs.rs` (now deleted), rebuilt on raw SQL, plus T1's unit test.
- No other `src/` line changed.

**Commands:**
```
cargo fmt --all
cargo nextest run -p am-store --test row_inputs --no-fail-fast
cargo nextest run -p am-engine --lib --no-fail-fast
cargo nextest run -p am-providers --test labels --no-fail-fast
cargo nextest run -p am-e2e --test taint_rows_reader_fence --test taint_prompt_fence --no-fail-fast
cargo nextest run -p xtask --test check_deps --no-fail-fast
cargo clippy --tests -p am-store -p am-engine -p am-e2e -p am-providers -p xtask -- -D warnings
cargo xtask check-deps; cargo xtask seam-guard; cargo xtask denylist
```
- fmt and clippy `-D warnings` are clean.
- check-deps, seam-guard and denylist pass on the tree, the moved tests included.
- The review ids `F1`…`F5` are kept out of test text: the denylist reads `F1` as a flow id.

**Test by test:**

| Row | Test (file) | Result now | Red because |
|-----|-------------|------------|-------------|
| T1 | `printable_escapes_are_decoded_once_and_the_rest_is_kept` (am-engine `row_inputs::tests`) | red | The part holds the data as stored: `a\u0041b…` is not decoded. |
| T1 | `an_escaped_letter_of_the_tag_cannot_close_its_fence` (am-e2e `taint_rows_reader_fence`) | red | "decoded once at the read site": the part holds `</\u0075ntrusted-data>`, `untrus\u0074ed`, … as stored. The precondition passes: the row keeps every form and both numbers as written. |
| T1 | `an_escaped_closing_tag_in_a_row_cannot_close_its_fence` (round 1's test, changed meaning) | red | The part is the stored data, not the data decoded once. |
| T2 | `the_read_paths_calls_stand_in_read_rows_and_inputs_of` (xtask) | red | The allowed tree passes. The first planted form, the one `.row_snapshot(` in a helper outside `fn read_rows`, passes check-deps ("must fail"). The shipped checks (`pub(crate)`, placement) run after it. |
| T2 | the moved tests: `a_row_input_becomes_one_fenced_part_and_one_edge`, `a_row_the_table_does_not_hold_is_refused_in_store_words`, `a_row_named_twice_is_refused_in_store_words` (am-engine `row_inputs::tests`) | green | Moved, not new (17 of 18 am-engine lib tests pass). |
| T3 | `inputs_of_names_no_way_out_of_the_fence` (xtask; narrowed) | red | `read.part.content()` in `fn inputs_of` passes check-deps ("the part's content: must fail"). The `into_content`, `write!` and `{:?}` plants run after it; the shipped `inputs_of` passes its check. |
| T4 | `a_row_edge_is_never_replaced_by_its_id` (am-store `row_inputs`) | red | With `recursive_triggers` off, `INSERT OR REPLACE` of the row edge's id by a clean plan input changes 1 row. |
| T5 | `a_spaced_or_hyphenless_closing_tag_cannot_close_a_fence` (am-providers `labels`) | red | `</ untrusted-data>`, `< /untrusted-`, `</\nuntrusted-`, `</untrusted>` and the others pass through `render_untrusted_text` as written. |
| T5 | 3.5's `a_label_cannot_span_lines_or_hide_characters`, with its byte-for-byte pin (am-e2e `taint_prompt_fence`) | **green on arrival** | It is a guard: the widened neutraliser must keep it. |

Every earlier test in these targets stays green: am-store `row_inputs` 14, am-providers `labels` 6, am-e2e fence 2 and `taint_prompt_fence` 2, xtask `check_deps` 44, round 1's R1 test among them.

**Not checked against a stub implementation** (as in round 1).

**Test lines (round 2):** about 870 added and 264 removed. About 265 of the added lines are the moved tests, and the 264 removed are the deleted integration file. Per file:
- the am-engine in-crate module about +265;
- `taint_rows_reader_fence.rs` about +230;
- `check_deps.rs` about +245;
- am-store `row_inputs.rs` +68;
- am-providers `labels.rs` +56;
- `taint_prompt_fence.rs` +8.

**Round 2 decisions (manager, 2026-11-25):**
- **D3:** a double-escaped tag (`\\u0075…`) is not decoded to a fixpoint. One JSON decode pass matches the row's meaning, and repeated decoding would alter genuine text containing backslashes.
- **T3 narrowed:** inside `fn inputs_of` the rule bans `content`, `into_content`, `format!`, `write!` and `{:?}`, so a row's text can never leave its `UntrustedData` there. It does not ban `TrustedInput`/`from_step_output`, which `inputs_of` uses legitimately for step outputs.

**Manager's decisions on round 2 (2026-11-25), applied by the tester:**
1. **The double-escape test is dropped (D3).** `a_doubly_escaped_letter_cannot_close_its_fence` and its `decoded_fully` helper are removed from `taint_rows_reader_fence.rs`. One decode pass matches JSON's meaning; a fixpoint would change real row text that holds backslashes. The fence target now runs 4 tests: 2 red (T1) and 2 green.
2. **T3 is narrowed.**
   - `inputs_of_names_no_way_out_of_the_fence` now plants `content`, `into_content`, `write!` and `{:?}` in the driver's row-read loop, each failing alone.
   - It no longer plants `TrustedInput` or `from_step_output`, and its shipped check bans only those four, which the shipped `inputs_of` passes.
   - `format!` is left out: the shipped `inputs_of` calls it twice for labels (`step:{}@{}`, `loop-back:{}@{}`).
   - Still red: check-deps lets `content` pass ("the part's content: must fail").
   - xtask `check_deps`: 44 of 46 pass, the 2 red being T2 and T3; clippy `-D warnings` and denylist clean.

### Implementation round 2 (2026-11-25)

*Implementer, on Opus, over round 1 in the real working tree with the shared cache (no `cargo clean`, no `--workspace`, no copy). Nothing recorded in git. No test changed, no dependency added, no new caller of `Instruction::authored`. T3 is built as the test and the coordinator's follow-up pin it: `format!` is **not** banned, so nothing was moved out of `fn inputs_of`.*

**Fixes:**
- **T1** (am-engine `row_inputs.rs`): new private `printable_unescaped`, applied to the part's text only. The edge, the label and the row are unchanged.
  - It makes one pass, never to a fixpoint (D3).
  - `\uXXXX` with four hex digits (either case) for a character from 0x20 to 0x7E becomes that character, and so does `\/`.
  - Every other escape is copied as its two characters, so `\\u0041` stays as written. Invalid or non-printable `\uXXXX` is copied as its six characters. All other text, number text included, is kept exactly.
- **T2** (am-engine `lib.rs`): `pub(crate) mod row_inputs`. The intra-doc links to it in `lib.rs` and `run.rs` became plain code spans (private-item links).
- **T2** (xtask `deps.rs`, `read_path_findings`):
  - The one `.row_snapshot(` call must lie inside `fn_body(code, "read_rows")`.
  - In `run.rs`, every `read_rows` (a call) and every `RowRead` must lie inside `fn inputs_of`.
  - A `pub mod row_inputs` in the engine's `lib.rs` is a finding.
- **T3** (xtask): inside `fn inputs_of`, the rule refuses the tokens `content` and `into_content`, `write` followed by `!`, and any string literal in the body's lines holding a Debug placeholder (`:?}` or `:#?}`). That literal check reads the file's literals, since the code scan blanks them. The shipped `inputs_of` names none of these.
- **T4** (0011, edited in place): `step_inputs_never_replaced`, a `BEFORE INSERT` that aborts when `step_inputs` already holds `NEW.id`, with the words "a step input is never replaced". The header now counts five triggers.
- **T5** (am-providers `neutralise`): a closing tag is `<` or `<`, optional ASCII whitespace, then `/`, `\/` or `/`, optional ASCII whitespace, then `untrusted` (no hyphen needed), in any case. Only the `<` (or its escape) becomes `&lt;`, so 3.5's literal output is byte for byte unchanged; `taint_prompt_fence`'s pin stays green.

**Code lines:**
- **Round 2:** about +150.
  - xtask `deps.rs` +76/−1;
  - `row_inputs.rs` about +51 (its code part is 151 lines, 8 of them the tester's test-module docs);
  - am-providers `common.rs` +6;
  - 0011 +9;
  - am-engine `lib.rs` and `run.rs`: 3 lines changed.
- **Story total:** about +855 and −149 against the baseline, well under the ~4,000-line stop.
- **Tests:** none written by the implementer.

**Verification:**
- `just check`: exit 0.
  - fmt-check and clippy `-D warnings` clean.
  - nextest: 1,641 tests run, 1,641 passed, 8 skipped.
  - Doctests green.
  - check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard and register all pass.
  - `agentic validate`: 0 errors; the known warnings only.
- `cargo test --doc -p am-store -p am-engine -p am-providers`: am-engine 1, am-providers 0, am-store 3 and 6, all passed.
- Per target, before the full run:
  - am-engine lib 18/18;
  - am-store and am-providers 268/268;
  - am-engine, am-e2e, am-app, am-providers and am-store 1,079 passed, 6 skipped;
  - xtask `check_deps` 46/46.

## Auto Run Result

**Status:** built (2026-11-25). This is slice 3 of 4 of the re-sliced story 3.6. The slices are store (merged), ToolGate (merged), this one, then documents and Drafts (21). Planned, tested, built and reviewed on Opus.

**Summary:** rows reach later steps only as fenced, tainting `UntrustedData`.
- **Migration 0011:** adds `step_inputs.from_row`. It is additive, and the pre-migration copy is kept. Triggers enforce:
  - a row edge is never updated, relabelled or replaced (by id, or by `INSERT OR REPLACE`);
  - its label matches its after-image;
  - its taint is at least the row's;
  - inserting one raises the attempt's taint.
- **The row-input slot:** a step's plan carries one. Tests fill it; the shipped `agentic run` leaves it empty, and a gate step with rows is refused.
- **`read_rows`** (`pub(crate)`, called only inside `inputs_of`) gives each row one `row` edge and one fenced `UntrustedData` part, and taints the step.
  - It refuses a repeated row in words.
  - The edge's taint is raised to the row's, never lowered.
  - `StartAttempt` refuses an edge naming a non-current image. The engine re-reads and retries once; a second change fails the step in words.
- **Row text:**
  - JSON escapes of printable ASCII, and `\/`, are decoded once before fencing; number text is untouched.
  - `neutralise` closes every tag of the form `<`, optional whitespace, `/`, optional whitespace, `untrusted` (any case, hyphen or not, plain or JSON-escaped). 3.5's literal output is unchanged.
- **check-deps:**
  - the one `.row_snapshot(` call sits inside `fn read_rows`;
  - `read_rows`/`RowRead` appear only inside `fn inputs_of`;
  - inside `inputs_of`, `content`, `into_content`, `write!` and Debug placeholders are refused;
  - a public `row_inputs` module is refused.
- **The owner's Q4 reader condition:** a row written by a tainted step taints its later reader, and a clean row's text is still fenced and still taints. Tested.

**Size (owner rule: code and test lines separately):** 1,109 lines of code (including the in-crate `#[cfg(test)]` module in `row_inputs.rs`) and 4,132 lines of tests.

**Tests:** written first, with red runs logged for the build and both review rounds. `just check`: 1641 passed, 8 skipped.

**Review:** correctness and adversarial security reviews, then a follow-up, all on Opus. None reached row text unfenced or trusted, or a reader left untainted.
- Round 1 fixed R1–R8.
- Round 2 fixed T1–T5.
- D1–D3 are accepted.

**For the owner, decided under the standing rules:**
- **Taint floor:** raise, never refuse or lower.
- **Fence code:** the R2 and T5 neutralise fixes extend 3.5's fence code. They only narrow, and the literal output is unchanged.
- **D3:** double escapes are decoded once, not to a fixpoint.
