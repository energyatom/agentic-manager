---
title: 'table_write through ToolGate'
type: feature
ticket: 23
created: '2026-11-24'
status: approved
baseline_revision: 'ec930b6f82943ca36c640f793d3e0686619cd7d6'
after: [6]
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
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-row-store-and-history-plan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Slice:** the ToolGate slice, epic 3 entry 23, "table_write through ToolGate", `after = [6]`. It is the second of four slices cut from story 3.6 by the owner on 2026-11-24 (Design Notes, Re-slices). Build order:
1. the store slice (entry 6, "Row store and history"): migration 0010, the row command, the readers;
2. **this slice**;
3. 3.6c (entry 22, "Rows reaching later steps", `after = [23]`);
4. 3.6b (entry 21, "Document versions and Draft items", `after = [22]`).

Its baseline is set when it starts, after the store slice merges. Matrix row numbers follow the approved 3.6 plan, so the written tests still point at them. Rows whose store half went to the store slice keep their number here for the engine half.

**Problem (after the store slice):** rows can be stored, but no step can write one, and the binary runs no tool.
- The store has `table_rows`, `row_history`, the result-kind columns on `tool_calls`, `Writer::apply_row_write` and the data-free `Reader::row_receipt` (migration 0010). A `cargo xtask check-deps` rule names `apply_row_write` only in am-store, so nothing in shipped code calls it.
- ToolGate checks only a manifest and a per-step count. It does not check the seat's grants (AD-11), and it refuses every class but `Read` (`crates/am-engine/src/toolgate.rs` L279-L290 at the 3.6 baseline).
- The shipped binary has no tool: `run_plan` takes none (`run.rs` L352), and a tool request fails the step in story 3.1's words (`tool_words`, L286). Tools run only through the test seam `run_plan_with_tools` (L369, `cargo xtask seam-guard`).
- A step declares no table: `am-config`'s `Step` has no such key (`entities.rs` L319-L354). Tables exist with one owning team (`Table`, L401-L413).
- am-tools holds only the folder tools; it never depends on am-store (AD-4).
- Every tool result taints the attempt (`run.rs` L1460).
- The Company A sample's `f2-update` is a fan-out step whose seats hold `table_write`; fan-out arrives with 3.14.

The verify (Sofia's entry 23 is the spec once merged): a FakeLlm-scripted step's `table_write` lands rows with version and taint, and row history naming run, step and seat. A write to another table, or from another team's seat, is refused. A repeated call with the same call sequence writes once. With the owner's conditions:
- no step can read rows, and a test and a check-deps rule show that no engine path passes row content to a model call;
- the engine result is journaled on its tool call's journal row (AD-8) and rebuilt from the writer's committed record;
- `agentic validate` names why a fan-out step that writes a table cannot run until 3.14, and stays green over `templates/`.

**Approach (owner-approved 2026-11-24, carried over unchanged):**
- **`table_write` keeps the architecture's placement (Q1; AD-21 as amended 2026-11-24, "Row-writer path"):** the tool lives in am-tools (spec, argument parsing, data checks); it writes through the `RowWriter` port in am-domain (owned types only); am-engine implements the port over the store's `apply_row_write`; only ToolGate calls the port's method; `agentic run` offers `table_write` only to a step that declares a table.
- **ToolGate is the only tool path (AD-11, AD-46):** every run gets a ToolGate, the shipped one included. ToolGate checks the seat's grants, the class, and for `table_write` the step's declared table and its team.
- **A trusted result built from engine values (Q4; AD-10 as amended 2026-11-24, "Engine result"):** a successful `table_write` returns an `EngineResult` that ToolGate alone builds from the row id, the version and a fixed status word. It does not taint. An error result stays `UntrustedData` and taints.
- **Journaled on its tool call's journal row (AD-8; AD-10 with Winston's wording, owner 2026-11-24):** the store slice's `apply_row_write` marks the call `engine` with the label `tool:table_write`; this slice rebuilds the result from the writer's committed record (`Reader::row_receipt`) through ToolGate's one constructor site, never from journal text or tool output.
- **Idempotent by run, step run and call sequence (AD-8 as amended):** the store refuses a second write at a key; this slice returns the first write's result (`AlreadyWritten`) from the receipt.
- **No row reaches a model (owner):** no step reads rows; the row-content readers stay inside am-store (check-deps), and the engine reads only the data-free receipt.
- **Fan-out (owner):** a fan-out step that declares `table:` or whose seats hold `table_write` gets a validate WARNING naming 3.14 and the fix; `agentic run` refuses it in the same words.

## Boundaries & Constraints

**Always:**
- **One tool path (AD-11):** every tool request of every step goes through `ToolGate::admit`, then the journal (`RecordToolCall`), then `ToolGate::run`. Nothing else runs a tool or writes a row for a seat.
- **One caller of the row-writer port (owner Q1; AD-21 as amended):** `RowWriter::write_row` is called only in `crates/am-engine/src/toolgate.rs`. The store slice's rule widens to exactly one shipped site outside am-store for `apply_row_write`: `crates/am-engine/src/row_writer.rs`, the port's implementation. Whole-file text with whitespace normalised; aliases and glob imports refused (3.5's T5).
- **Owned types at the port (AD-5, AD-4):** `am_domain::rows` uses only std, `uuid`, `serde` and am-domain types; row data crosses as JSON text, one way, into the store.
- **Grants come only from capabilities (AD-46):** `table_write` reaches a seat only through its Tables capability; the plan carries the seat's `grants`; ToolGate refuses a tool not in it.
- **The binding (AD-21, AD-10 (b), RULE-8):** a seat writes only the table its step declares, and only when that table's team is the seat's team, for every step, tainted or clean. Checked by `agentic validate`, by ToolGate, and again by the store from the journal (the store slice's `writes_table` check).
- **Results (AD-10 as amended 2026-11-24, owner Q4):** a successful `table_write` result is an `EngineResult` (private fields, built only in `toolgate.rs`, from the row id, the version and a fixed status word; never from row data, an argument, a column name or model text; it does not taint and never clears taint). Every error result and every runner result stays `UntrustedData` and taints.
- **Rebuilt, never parsed (AD-10 with Winston's wording; owner):** a resume (3.9) or drill-down (epic 6) rebuilds the result with `toolgate::engine_result_of` from `Reader::row_receipt`; nothing parses journal text or tool output.
- **No row reaches a model (owner conditions):** no row-input slot, no `RowInput`, no `from_row`, no `row` input kind, even test-only; the row-content readers and the record types carrying row data stand in shipped code only inside am-store; only am-store links `rusqlite`. A run test and check-deps show it (rows 42, 43).
- **Writer commands with an explicit Actor (AD-6, AD-24):** ToolGate writes as `Actor::Seat` of the attempt; the store refuses any other.
- **State machine (AD-8):** no new state or edge.
- **External effects (AD-11, RULE-10):** `table_write` is local-write with no network; external-effect tools stay refused until 4.8; v1 has no send tools.
- **No model words in refusals:** a refusal never echoes a model-chosen table name, row id, column name or value; it names config ids only.
- **Audit (AD-25; AD-32):** `toolgate.refused` carries the taint and config ids; the store slice's `row.written` holds no data; no new `*_sha256` of free text. The unknown-tool label keeps the existing helper (`label_of`, `toolgate.rs` L84); 9.10 keys it.
- **Interim widening risk (owner Q3; AD-15 as amended):** until 4.17's `is_widening`, adding or changing a step's `table:` takes effect at Reload or startup without the two-step confirmation. Accepted; 4.17 classifies it (#35).
- **Fan-out steps (owner):** a fan-out step that declares `table:` or whose seats hold `table_write` is a validate WARNING (never an error) naming 3.14 and the fix; `agentic run` refuses it before any row or provider, in the same words. 3.14's ticket allows `table:` on fan-out steps (Sofia).
- **Inert document field:** `StepPlan.document` exists here so plan literals change once for documents; any value is refused in words naming 3.6b.
- **New callers of `Instruction::authored` (owner):** none; if needed, the same security review as 3.5.
- **Order of work (owner):** after the store slice merges, this plan's baseline is set and it is committed on its own branch; this slice's red run is logged before any `src/` change; one squash commit, on Opus; stop and report if the code passes about 4,000 lines.
- **Tests first:** FakeLlm and FakeClock only; no network, no real model.

**Never:**
- **No second tool path, no read path, no grant outside capabilities.** `ToolRunner` stays test-only (seam-guard unchanged).
- **No migration:** every schema change of the row work is the store slice's 0010.
- **No delete or move of rows; no status-transition rule, reason field, `v_*` view or grant read** (7.4, 7.5, 7.2).
- **No document version, Draft item or `document:` key** (3.6b).
- **No prompt content type beyond AD-10 as amended;** no `prompt_parts` change.
- **No new crate.** One new edge: am-tools uses the workspace-pinned `serde_json` (Stack, in the lock).
- **No department vocabulary in `crates/`.**
- **Outside this slice:** the store (entry 6), reading rows (3.6c), documents and Drafts (3.6b), the RULE-8 suite (4.9), external-effect approvals (4.8), cost (3.7), resume (3.9), fan-out writes (3.14), `rows:` hand-offs (3.15), lineage reads (3.17), the golden run (3.13), `is_widening` (4.17).

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Offered tools | A step declaring table `T` of its seat's team; the seat's Tables capability on | The request offers exactly `table_write`; its input schema (from am-tools) names `T` and `T`'s column ids and types. A step with no table, or a seat without Tables, is offered no `table_write` | — |
| 2 | Insert through ToolGate | FakeLlm turn: `table_write {table: T, data: {...}}` | The row and its after-image land (store slice); the call is journaled `engine` / `tool:table_write`; the model gets an `EngineResult` (row id, version 1, `inserted`); the attempt stays clean | — |
| 3 | Update through ToolGate | A later turn: `table_write {table: T, row_id, expected_version: 1, data: {col: v}}` | Version 2 lands; result `updated`, version 2 | — |
| 4 | Stale version, at the step (Q2) | `expected_version` not the row's | The store refuses with nothing changed (store slice, row 4); the tool call `failed`, journaled `untrusted`; the model gets the words as an untrusted error result; the step goes on | Tool error |
| 5 | Unknown row | `row_id` not a UUID, or not in `T` | Nothing written; untrusted error result "a row this table does not hold"; the id is not echoed | Tool error |
| 6 | Another table | `table_write {table: U}`, `U` not declared | ToolGate refuses: tool call `refused`, `toolgate.refused` audited (Seat actor, `tool:table_write run:<run> step:<key>`), words name the declared table and never the model's text; the step fails (3.3's rule); no row | Refused, step fails |
| 7 | Another team's table | Step declares a table of team B; the seat is in team A | `agentic validate` error (file, line, fix); a hand-built plan is refused before any row, or offered nothing with every call refused by ToolGate; the store refuses it too | Refused |
| 8 | Not granted | Tables off (`table_write` not in `grants`) | `agentic validate` refuses the step's `table:`; a hand-built plan offers no `table_write`; a call is refused "not granted to seat `<id>`" and audited | Refused |
| 9 | Unknown tool | A name no manifest holds | Refused as today, labelled `unknown:<sha256>` by the existing helper; audited | Refused |
| 10 | Other classes | `write_document`, `draft_message`, `workspace_write`, a fixture external-effect tool | Refused as a class this version does not run, granted or not; never run | Refused |
| 11 | No declared table | A granted seat's step with no `table:` calls `table_write` | Refused "step `<key>` declares no table" | Refused |
| 12 | Tainted step (engine half) | A step with an untrusted plan input writes `T`, then tries `U` | `T`'s row stores taint 1 (store slice); `U` refused as row 6, audit args carry the taint | Refused for `U` |
| 13 | Taint only rises (run half) | A tainted step writes a row; a later clean step rewrites it | Through ToolGate, the row and its newest after-image stay tainted (the store slice's rule, proven end to end) | — |
| 14 | Bad data | Not an object; an undeclared column; a wrong type; a required column missing on insert or set to `null`; a `status` value outside `values`; data over 64 KiB | Nothing written; untrusted error result worded from the table's schema, never the model's keys or values | Tool error |
| 15 | Repeated call sequence (engine half) | The port's `write_row` twice with one key | The store writes once; the port returns the first write's row id and version as `AlreadyWritten`, from the receipt | — |
| 20 | Repair call | A step with a schema whose final message is invalid | The repair call offers no tools (3.5); a tool call in its reply fails the step; rows from earlier turns stay | — |
| 21 | Step fails after writing | Rows written, then the step fails | Rows stay; their after-images name the failed attempt; no rollback | — |
| 22 | Provenance end to end | A run's writes, with a canary in row data | Every row change has an after-image naming run, step and seat; the audit export holds no canary and no hash of it; the chain verifies | — |
| 28 | Mixed declarations (rows) | `table:` on a gate step; a hand-built plan with a table on a gate, or any `document` value | `agentic validate` error for the gate; `run_on` refuses the plan before any row, a document in words naming 3.6b | Worded |
| 31 | Validate (tables) | `table:` unknown; table of another team; seat without Tables | Each an error with file, line and fix | Validate error |
| 33 | Only ToolGate calls the port (Q1) | Planted files: a `write_row` call outside `toolgate.rs`; split over lines or spaced around `.`/`::`; `use …::RowWriter as W`; `use am_domain::rows::*`; `apply_row_write` outside am-store and `row_writer.rs` | `cargo xtask check-deps` fails naming file and line for each; the shipped tree passes | CI fails |
| 34 | Shipped `agentic run` (Q1) | The `rows-documents` fixture (no document step yet) through `plan_of` with FakeLlm | The writing step is offered `table_write` and its rows land as row 2; a step with no `table:` is offered none | — |
| 35 | Engine values only (Q4) | Canaries in a data value, an unknown argument key, an update's patch and the arguments text | The `EngineResult` text holds none of them, only the row id, the version and one fixed word; the attempt stays clean and its next row is clean | — |
| 36 | Error results stay untrusted (Q4) | A stale version (row 4) or bad data (row 14) | `Part::ToolResult` with `UntrustedData`; the call journaled `untrusted`; the attempt tainted from its next call; a later row has taint 1 | — |
| 37 | Only ToolGate builds an engine result (Q4) | Planted files: `EngineResult::row_written` outside `toolgate.rs`; an alias or glob import of `EngineResult`; spacing or line breaks | `cargo xtask check-deps` fails for each; doctests: no `Default`, `From`, `Deserialize` or struct literal builds one | CI fails |
| 40 | Fan-out step that writes a table (owner) | A fan-out step (`seats:` or `over_folder:`) that declares `table:`, or whose seats hold `table_write`; the Company A sample (`f2-update`, step `update`); `agentic run` of such a step | `agentic validate`: a WARNING (not an error) at the step's line, saying the step cannot write a table until story 3.14 and naming the fix; exit 0. The sample gets exactly that one warning; `agentic validate` over `templates/` stays green. Seats without `table_write` and no `table:` give none. `agentic run` of such a step is refused before any run row or provider, in the same words | Warning; run refused |
| 42 | No way to read rows (owner condition a, engine half) | This slice's tree | No `am_domain::rows::RowInput`, no `StepPlan::row_inputs` (`compile_fail` doctests; 3.6c supersedes them); the store half (no `from_row`, no `row` kind) is the store slice's | Does not compile |
| 43 | No row content reaches a model call (owner condition b) | Step A inserts rows holding canaries; a later clean step B updates a canary row with a canary-free patch; step C has no tools; and planted files naming a row-content reader or record type outside am-store, or a crate other than am-store declaring `rusqlite` | Every later request holds no canary in any system, instruction, trusted, untrusted, tool-result or engine-result part; none at all in B's or C's requests; A's later turns hold it only in A's own replayed `Part::ToolCall` arguments; each engine result holds only engine values. `cargo xtask check-deps` fails for each planted form and passes the shipped tree | CI fails |
| 44 | Engine result rebuilt (AD-8, AD-10 with Winston's wording) | A run with an insert, an update, a refused write and a stale update | Each succeeded call is journaled `engine` / `tool:table_write` on its tool-call row (store slice); `toolgate::engine_result_of(reader, step_run, call_seq)` rebuilds, from `Reader::row_receipt`, an `EngineResult` whose text equals what the model received; failed calls `untrusted`; refused calls none; no canary in the journal row, the receipt or the rebuilt result | — |

</intent-contract>

## Code Map

- **`crates/am-domain`:**
  - `src/rows.rs` (the store slice adds `RowWrite` and `RowOp`): adds `DeclaredTable`, `ColumnSpec`, `ColumnKind`, `RowWritten`, `RowStatus` (`Inserted`, `Updated`, `AlreadyWritten`), `RowWriteError::{Refused, Failed}`, `PreparedRow`, the `RowWriter` port (`write_row`) and the `RowTool` trait (`spec`, `prepare`). A `compile_fail` doctest pins that there is no `RowInput` (row 42).
  - `src/content.rs`: `EngineResult` per AD-10 as amended (private fields, one constructor `row_written(&RowWritten)`, no `Default`, `From`, `TryFrom`, `Deserialize`; fixed words, the row id and the version; label `tool:table_write`). Module docs name the fourth kind.
  - `src/llm.rs` `Part` L34-L51: `Part::EngineResult { call_id, result }` beside `ToolResult` (L48-L51).
- **`crates/am-tools`:** `src/table_write.rs` (new): `TableWrite` implements `RowTool` (the spec from config ids and types, argument parsing, the data checks, words from config ids only; it never names `write_row`); `src/lib.rs` module and description; `Cargo.toml` `serde_json.workspace = true`.
- **`crates/am-engine`:**
  - `src/row_writer.rs` (new): `StoreRowWriter` implements `RowWriter` over `apply_row_write`, reads back with `Reader::row_receipt`, maps `StoreError::RowRefused` to `Refused`. The one shipped site outside am-store for `apply_row_write`.
  - `src/toolgate.rs`: holds `Arc<dyn RowWriter>` and `Option<Arc<dyn RowTool>>`; `ToolGate::shipped(row_writer, row_tool, audit)`; `over` L130 adds both; `admit` L214 takes `StepTools` and `CallPlace`; `decide` L251 adds the grant check (after L272), opens `LocalWrite` only for `table_write` (L279-L290), then the binding; `refuse` L321 adds the taint; `Permit` L70 carries the call's place; `run` L230 runs `prepare`, mints an insert's row id, calls `write_row` (the one call) and builds the `EngineResult`; new `engine_result_of(reader, step_run_id, call_seq)`, the rebuild. The only constructor sites.
  - `src/run.rs`: `RunPlan` L181 gains `row_tool`; `StepPlan` L150-L177 gains `grants`, `seat_team`, `table: Option<DeclaredTable>`, `document` (inert); a `compile_fail` doctest pins that there is no `row_inputs` (row 42); `run_plan` L352 builds `StoreRowWriter` and `ToolGate::shipped`; `run_plan_with_tools` L369; `run_on` L404 checks declarations; `Tooling` L615-L638 specs per step; `run_step` L1069 passes the declared table in `StartAttempt`'s `writes` (the store slice passes `None`); tools offered L1203-L1206; tool loop L1433-L1460 (an `EngineResult` leaves `carry` as it is); `tool_call` L1518; `tool_words` L286 no longer reached.
- **`crates/am-providers`:** `src/anthropic.rs` L60, L96 and `src/openai_compat.rs` L143, L179: render `Part::EngineResult` through one shared function, outside any fence.
- **`crates/am-config`:** `src/entities.rs` `Step` gains `table`; `src/kinds.rs` step key `table` (L2355-L2364), `Step` build L2421, cross-checks beside `hand_off_checks` L2249 (table exists; single-seat step: the seat's team owns it and the seat has Tables on; none on a gate), the fan-out warning (row 40); one function builds the fan-out words for validate and `agentic run`.
- **`crates/am-app/src/run.rs`** `plan_of` L392-L503: the fan-out check first (before `not_honoured` L283 and any provider); `row_tool` from am-tools; grants, team, declared table; `document: None`.
- **`crates/am-store`:** no change but reads (`row_receipt`, `attempt_writes`).
- **`xtask/src/deps.rs`:** the store slice's `apply_row_write` rule gains its one allowed site, `crates/am-engine/src/row_writer.rs`; new rules: the port method `write_row` (call only in `toolgate.rs`; definition in `am-domain/src/rows.rs`; the `fn` line in `row_writer.rs`); `EngineResult::row_written` (only `toolgate.rs` and its definition); no alias of `RowWriter` or `EngineResult` in `crates/`; the row-content rule (the readers in call position and `RowRecord`, `RowHistoryRecord` only in `crates/am-store/src/`); only am-store declares `rusqlite` as a normal dependency. Whole-file text, whitespace normalised, aliases and globs refused.
- **`xtask/src/seam_guard.rs`:** `SEAMS` unchanged; doc lines L27-L31 reworded (the binary runs only `table_write`, through ToolGate).
- **`justfile`** `validate` L75-L81: add `fixtures/projects/rows-documents/`. **Docs:** `docs/config-schema.md` L315-L320 (the `table` key, the fan-out warning).
- **Fixture:** `fixtures/projects/rows-documents/`: teams `core`, `side`; seats `writer` (Tables on), `outsider` (Tables off); tables `entries`, `others`; workflow `record` (`collect` with `table: entries`, `plain`); a fan-out workflow whose seats hold `table_write`.
- **Existing tests that must change (supersession):**
  - `crates/am-engine/tests/loop_caps.rs` L428-L469 and `crates/am-engine/tests/run.rs` L609-L645: a tool request with no tool offered goes through ToolGate.
  - `crates/am-app/tests/run_cli.rs` L1100-L1122: ToolGate's "not granted" words.
  - Every `StepPlan { .. }` and `RunPlan { .. }` literal gains the new fields, without `row_inputs` (`am-app/src/run.rs` L450, L485; am-engine tests; am-e2e `tests/engine/{support/mod.rs,handoff_gate_resolve,prompt_assembly_journal,schema_repair}.rs` and `tests/trust/{taint_trusted_input_gate_note,taint_prompt_fence,inbox_resolve_human_only,taint_prompt_type_wall,rule_15_inbox}.rs`). Test helpers grant the fixture tool by name.
  - Every exhaustive `match` on `Part` gains `EngineResult`.
  - The store slice's xtask test `the_row_write_command_is_named_only_in_the_store` is widened to allow `row_writer.rs`.
  - `crates/am-engine/tests/loop_caps.rs` L723-L760 stays green.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:** the store slice merged; this plan's baseline set to main after it; this plan committed on its own branch.
- [ ] **Tests first (no `src/` change), on Opus:** this slice's files in the Tests-first list, the fixture, the `[[test]]` targets and the superseded literals. Log this slice's red run.
- [ ] **am-domain:** the port, the types, `RowTool`, `EngineResult`, `Part::EngineResult`, the row-42 doctests.
- [ ] **am-tools:** `table_write.rs`; `serde_json` edge.
- [ ] **am-engine:** `row_writer.rs`; ToolGate's grants, class gate, binding, port call, engine result and `engine_result_of`; the plan fields; the shipped gate.
- [ ] **am-providers:** render `Part::EngineResult`.
- [ ] **am-config:** the `table` key, cross-checks, fan-out warning.
- [ ] **am-app:** `plan_of`'s fan-out refusal, mapping and `row_tool`.
- [ ] **xtask:** the widened store rule and the new rules, with planted files; seam-guard doc lines.
- [ ] **justfile and docs.**
- [ ] **Size (owner):** one squash commit; stop and report if the code passes about 4,000 lines.

**Acceptance Criteria:**
- Given each matrix row of this slice, when its test runs, then results, journal kinds, taint, refusals, warnings and requests match the row.
- Given the verify, when the fixture's writing step runs on FakeLlm: rows land with version and taint and after-images naming run, step and seat; another table and another team's seat are refused; a repeated call sequence writes once.
- Given each owner condition, its row (4, 33 to 37, 40, 42 to 44) and tests pass.
- Given the workspace, when `just check` runs, then it is green, `agentic validate` over `templates/` included (the one fan-out warning allowed). Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-24, owner (third re-slice): written from 3.6a's plan (`story-rows-written-with-provenance-through-toolgate-plan.md`, renamed to the store slice's `story-row-store-and-history-plan.md`). Carries 3.6a's ToolGate side: rows 1–3, 5–11, 14, 20, 21, 28, 31, 33–37, 40, 43, the engine halves of 4, 12, 13, 15, 22, 42 and 44, and their tests. No migration: 0010 is the store slice's. AD-10's journaling with Winston's wording ("on its tool call's journal row (AD-8)"). Size reported as code and test lines.

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

**In this slice:** Q1 with its condition; Q2 at the step (a stale version is an untrusted error the model can act on; the store refusal is the store slice's); Q3's `table:` half and its interim risk; Q4 without its reader condition (which the owner moved to 3.6c); Q9 for this slice, with the owner's code-based stop. Q5 to Q8 are 3.6b's. No answer changed.

### Re-slices (owner, 2026-11-24)

**First, verbatim:** "Don't accept a single 12,000-line commit. Sofia re-slices 3.6 into 3.6a (row store, history, row-writer interface, table_write via ToolGate only, engine-built result, tainted-row rules) and 3.6b (document versions, Draft items with Human-only dismissal). My approved answers carry over unchanged. Split the written tests by slice, log a red run for each, one squash commit per slice, on Opus. Fan-out: accept that the Company A f2-update step fails until 3.14, but agentic validate must name the reason, and 3.14's ticket must include allowing table: on fan-out steps."

**Second, verbatim:** "Split again: move the row-read side (row-input slot, from_row link, reader taint, fenced row text) into a new 3.6c, "Rows reaching later steps", built right after 3.6a and before 3.6b. Condition: 3.6a contains no way for a step to read rows, with no from_row and no row-input slot, even test-only, plus a test or check-deps rule proving no engine path passes row content to a model call. 3.6a still stores each row's own taint on write. My Q4 condition moves to 3.6c unchanged. Add the AD-10 journaling of the engine result to 3.6a."

**Third, verbatim:** "Split 3.6a by layer: a store slice (migration 0010, row and history write commands, append-only triggers, taint on write, stale-version refusal, store-level tests) and a ToolGate slice (row-writer interface, table_write via ToolGate only, engine result and journaling, idempotency, check-deps rules, no-read-path proof, fan-out warning). Order: store, ToolGate, then 3.6c, then 3.6b. Condition: nothing in shipped code calls the row-write commands until the ToolGate slice lands, enforced by check-deps. Accept Winston's wording "on its tool call's journal row (AD-8)" in the same re-slice PR. From now on, plan estimates report code and test lines separately, and the overrun stop is about 4,000 lines of code."

**What this means here:** this slice is the third decision's ToolGate slice. It widens the store slice's no-caller rule by exactly one site (`row_writer.rs`), adds every other check-deps rule, and carries the no-read-path proof, the engine result's rebuild and the fan-out warning. It adds no schema.

### ToolGate as the only tool path (owner Q1; AD-11, AD-21 as amended)

- **Who calls whom:** am-tools' `TableWrite` (a `RowTool`) builds the spec and turns arguments into a checked `PreparedRow`; it writes nothing and never names `write_row`. am-engine's `StoreRowWriter` implements `RowWriter` over `apply_row_write`. ToolGate holds both and is the one caller of `write_row` and the one builder of `EngineResult`. am-app wires am-tools' tool into the plan (`RunPlan.row_tool`) because am-engine may not depend on am-tools (AD-4).
- **Every request through `admit`**, in this order (first refusal wins, as in 3.3): a Seat actor; no duplicate id; a known tool (else `unknown:<label>`); granted (new); class (`Read` runs; `LocalWrite` only for `table_write`); for `table_write`, the step declares a table, the call's `table` argument equals it (read, compared, kept nowhere), and the table's team is the seat's team (new); the per-step use limit.
- **Run:** `RecordToolCall issued`; `RowTool::prepare` (a bad row is an untrusted error result); ToolGate mints an insert's row id (UUIDv7), builds the `RowWrite` from the call's place and the attempt's taint, calls `write_row`. `RowWritten` becomes the `EngineResult`; `Refused` words an untrusted error result; `Failed` stops the step.
- **Grants (AD-46):** only the Tables capability grants `table_write` (`capability::expand`); `agentic validate` already refuses it in `tools:` while Tables is off; `table:` binds, never grants; manifest local-write, no network, 100 uses; `limits.tool_calls` still applies.

**Refusals, in words and audited:**

| Case | Words (engine's; config ids only) | Journal and audit |
|------|-----------------------------------|-------------------|
| Unknown tool | `Tool "unknown:<hex>" was refused: it is an unknown tool (no manifest).` (existing) | tool call `refused`; `toolgate.refused`, target `tool:unknown:<hex> run:<run> step:<key>` |
| Not granted | `Tool "<id>" was refused: it is not available to seat <seat>, which is not granted it (its <capability> capability is off).` | as above, target `tool:<id> …` |
| Class not run | `Tool "<id>" was refused: it is a <class> tool, and this version runs read tools and table_write only.` | as above |
| No declared table | `Tool "table_write" was refused: step <key> declares no table, so it writes none.` | as above |
| Another table | `Tool "table_write" was refused: step <key> writes only table <declared>, and this call names another table.` | as above; the model's table text is not stored or hashed |
| Another team | `Tool "table_write" was refused: table <declared> belongs to team <team>, and seat <seat> is in team <seat-team>; a seat writes only its own team's tables.` | as above |
| Tainted step, another output | The same words as "another table" | as above; audit args carry `taint: 1` |

- A refusal ends the step as 3.3 does (`fail`, no Failure item). Refusals of an admitted call are untrusted error results; the call ends `failed` and the step goes on.

### The data (am-tools, `table_write.rs`)

- Checked by hand against the declared columns: `data` is an object; every key a declared column; `text`, `url`, `date`, `datetime` → string, `number` → number, `integer` → integer, `boolean` → boolean, `status` → one of `values`, `json` → any; `null` only for a column that is not required. Insert: every required column present. Update: only the named columns; a required column cannot become `null`. At most 64 KiB. No cleaning or normalising. No transition check (7.4). Words name config ids and types only. `jsonschema` stays in am-engine's `schema.rs` only.

### Idempotency at the engine (AD-8 as amended)

- The key is (run, step run, call sequence): the attempt's step run and its journaled call. The store slice writes once per key; this slice's port returns `AlreadyWritten` with the first write's row id and version, read from `row_receipt`.
- Crash before the store's transaction commits: the call stays `issued`; 3.9 re-runs it and it writes once. Crash after: the call is `succeeded` and journaled `engine`; 3.9 rebuilds the result with `engine_result_of`; a re-sent write is a replay. Until 3.9, `continue_run` refuses a run with a call in flight.
- A retried turn (3.8) that asks again gets a new call sequence: a new write. 3.5's repair call offers no tools, so no key is spent on one.

### The engine result and its journal row (owner Q4; AD-10 with Winston's wording)

- **Built from engine values only:** the row id (a store UUID), the version, one fixed word (`inserted`, `updated`, `already written`), from `RowWritten`, which comes from the journal's committed record. Nothing the model wrote, no row data, no column name. A successful write leaves the attempt as clean as it was; a tainted step stays tainted. Every error result stays `UntrustedData`.
- **Journaled on its tool call's journal row (AD-8):** the store slice's `apply_row_write` sets `result_kind = 'engine'` and `result_label = 'tool:table_write'` in the write's transaction (triggers: `engine` only with an after-image at the key and only for `table_write`; set once). Winston confirmed the reading on 2026-11-24, and AD-10 gets that wording in the re-slice PR; no `prompt_parts` kind is needed.
- **Rebuilt, never parsed:** `toolgate::engine_result_of(reader, step_run_id, call_seq)` reads `Reader::row_receipt` (typed values, no data) and builds the result through the one constructor in `toolgate.rs`. The word comes from the recorded op, so a rebuild equals what the model first received. 3.9 and epic 6 call it.

### No row reaches a model (owner conditions a and b)

- **(a)** No row-input slot, no `RowInput`, no `from_row`, no `row` kind: `compile_fail` doctests for `RowInput` and `StepPlan::row_inputs` here (the store slice pins `StepInput::from_row` and the refused kind). 3.6c supersedes them.
- **(b) The rule, narrowest that holds:** in shipped source, the row-content readers in call position (`.rows(`, `::rows(`, `.row(`, `::row(`, `.row_history(`, `::row_history(`, `.row_write(`, `::row_write(`) and `RowRecord`, `RowHistoryRecord` stand only in `crates/am-store/src/`; only am-store declares `rusqlite` as a normal dependency. No shipped code calls them at the 3.6 baseline. Seam-guard bans `macro_rules!`, `include!` and `#[path]` in shipped source, so a call needs its name. The engine reads only `row_receipt`; am-providers cannot reach the store (AD-4). Why not am-engine alone: am-app could otherwise put row text in a plan's untrusted inputs.
- **(b) The test (row 43, `taint_rows_no_read_path.rs`):** step A inserts canary rows; clean step B updates a canary row with a canary-free patch; step C has no tools. No later request holds a canary in any system, instruction, trusted, untrusted, tool-result or engine-result part; B's and C's none at all; A's later turns only in A's own replayed `Part::ToolCall` arguments (the model's own words).

### Fan-out steps that write tables (owner)

- A step with `seats:` or `over_folder:` that declares `table:`, or names a seat whose `grants` hold `table_write`, gets one WARNING at its line, never an error. Message, in substance: "step `<id>` is a fan-out step that would write a table; a fan-out step cannot write a table until story 3.14, so `agentic run` refuses it". Fix: "keep it until story 3.14 allows `table:` on fan-out steps, or move the writes to a step with one `seat:` and a `table:`". On a fan-out step `table:` is checked for existence only.
- `plan_of` runs the same check first (before `not_honoured` and any provider) and refuses in the same words, from one am-config function. No run row.
- `agentic validate --template templates/samples/company-a/` reports exactly that warning, on `f2-update`'s `update`, and exits 0 (warnings allowed, `am-app/src/cli.rs` L87). `f1` `branches` and `f3-folder` stay silent (their seats lack `table_write`).
- 3.14's ticket allows `table:` on fan-out steps (Sofia) and lifts this. The warning is not the widening classifier (4.17).

### Decided within the ADs (accepted with the plan)

1. No new crate; am-tools gains the workspace-pinned `serde_json`.
2. ToolGate refusals fail the step; refusals of admitted calls are untrusted errors the model can act on.
3. Row data checked in am-tools against the declared columns, at most 64 KiB, never normalised.
4. Row ids are UUIDv7s ToolGate mints; the `key` column is data.
5. Rows of a step that later fails or loops back stay; no rollback.
6. Every tool request goes through ToolGate, so 3.1's "tools are not available" path is superseded.
7. The `table_write` spec is built from config ids and types only, not column descriptions.
8. `agentic status` and the API list no rows here (7.1).

### Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code:** about 2,400 to 2,900 lines: am-domain port, types and `EngineResult` (~350), am-tools `table_write.rs` (~350), am-engine ToolGate, `row_writer.rs`, plan and loop (~900), am-providers (~60), am-config key, checks and fan-out warning (~250), am-app `plan_of` (~120), xtask rules (~350), docs, justfile and fixture (~120). **Below the ~4,000-line stop, but the largest of the four slices.**
- **Tests:** about 5,300 to 5,900 lines: the written ToolGate-side files (`toolgate.rs` changes ~540, `row_writer.rs` ~330, `table_write.rs` ~310, `taint_tool_result_engine_values.rs` ~300, `rows_table_write.rs` ~420, `rows_idempotent.rs` ~190, `capabilities_tables_grant.rs` ~310, `taint_rows_declared_table.rs` ~230, the run halves of `taint_rows_at_rest.rs` and `rule_16_row_provenance.rs` ~340, `run_rows.rs` ~260, `step_writes.rs` ~280, `labels.rs` ~110, `check_deps.rs` ~450 plus ~250 new, the e2e support and literal sweep ~450), plus new tests (`engine_result_journal.rs`, `taint_rows_no_read_path.rs`, fan-out and CLI tests, doctests: ~750).

### Tests-first list

All FakeLlm and FakeClock; no network; no real model. The tester splits these from the tests written for 3.6 and logs this slice's red run after the store slice merges. Each am-e2e file has its `[[test]]` target.

- **am-domain doctests:** `EngineResult` allowed use and four `compile_fail` (row 37); `compile_fail` for `am_domain::rows::RowInput` (row 42). New ones green on arrival, logged.
- **am-engine doctest:** `compile_fail` for `|p: StepPlan| p.row_inputs` (row 42).
- **am-tools `tests/table_write.rs` (all four):** rows 1, 14, 22.
- **am-engine `tests/toolgate.rs`:** grant, binding, team, class, echo refusals (rows 6–8, 10, 11); `a_write_comes_back_as_an_engine_result` (rows 2, 35); `an_error_comes_back_untrusted` (row 36); `the_rebuilt_result_equals_the_one_sent` (row 44). **`tests/row_writer.rs`:** `the_store_row_writer_reads_its_result_from_the_receipt` (rows 2, 15); `a_stale_version_maps_to_refused` (row 4). **Superseded:** `loop_caps.rs` L428-L469; `run.rs` L609-L645.
- **am-config `tests/step_writes.rs` (table tests and row 40):** `a_step_declares_its_table`; unknown table, other team, seat without Tables, gate (rows 7, 8, 28, 31); `a_fan_out_step_that_writes_a_table_is_warned_with_the_reason`, `the_sample_f2_update_gets_exactly_that_warning` (row 40).
- **am-providers `tests/labels.rs`:** `an_engine_result_renders_as_a_tool_result_outside_any_fence` (rows 2, 35).
- **am-e2e engine:** `rows_table_write.rs` (all six: rows 2–5, 14, 20, 21, the verify); `rows_idempotent.rs` (all three: rows 15, 16, 19 end to end); `engine_result_journal.rs` (new: `each_result_kind_is_journaled_with_its_label`, `a_resume_rebuilds_the_same_result_from_the_record`, `the_journal_and_the_rebuild_hold_no_canary`; row 44).
- **am-e2e trust:** `capabilities_tables_grant.rs` (rows 1, 6, 8, 9, 34); `taint_rows_declared_table.rs` (rows 6, 7, 12); `taint_rows_at_rest.rs`'s two row tests (rows 12, 13 end to end); `taint_tool_result_engine_values.rs` (rows 35, 36, the planted-string test); `taint_rows_no_read_path.rs` (new, row 43); `rule_16_row_provenance.rs` row tests (row 22). Not `rule_08_*` (4.9).
- **am-app `tests/run_rows.rs`:** `plan_of_maps_grants_team_table_and_the_row_tool`, `agentic_run_offers_table_write_only_to_a_declaring_step` (row 34); `a_step_declaring_another_teams_table_is_refused_before_any_row` (row 7); `a_fan_out_step_that_writes_a_table_is_refused_with_the_validate_words` (row 40). **Superseded:** `run_cli.rs` L1100-L1122.
- **am-e2e `tests/cli_validate.rs`:** `the_sample_validates_with_the_one_fan_out_warning` (row 40).
- **xtask `tests/check_deps.rs`:** `only_toolgate_calls_the_row_writer` (row 33); `the_store_row_write_is_named_only_by_the_engines_row_writer` (row 33, widening the store slice's rule); `only_toolgate_builds_an_engine_result` (row 37); `row_content_is_read_only_in_the_store`, `only_the_store_links_rusqlite` (row 43); `the_shipped_tree_passes_the_rules` (rows 33, 37, 43).
- **Fixture:** `fixtures/projects/rows-documents/` without the document step; plus the fan-out workflow.

## Verification

- `cargo nextest run` per crate, filters first: am-tools `table_write`; am-engine `toolgate`, `row_writer`, `loop_caps`, `run`; am-config `step_writes`; am-providers `labels`; am-e2e this slice's engine and trust targets and `cli_validate`; am-app `run_rows`, `run_cli`; xtask `check_deps`.
- `cargo test --doc -p am-domain -p am-engine`.
- `cargo xtask check-deps` (the rules green on shipped code, red on each planted file); `cargo xtask seam-guard` green.
- `agentic validate` over `templates/` and `fixtures/`: green; the sample prints exactly the one fan-out warning.
- `just check`; `just supply-chain` (the am-tools → `serde_json` edge adds no package).
- No `cargo clean`, no build of a workspace copy against the shared target.
- Merge only with both CI jobs green after review triage.

## Implementation Notes

### API the tests fix (tester, 2026-11-24): the ToolGate slice's items

*Moved word for word from 3.6's plan in the third re-slice (2026-11-24); the store slice's plan keeps the store items, 3.6c's and 3.6b's theirs. Lines the splits changed are marked (re-slice).*

**am-domain**
- In `am_domain::rows` *(re-slice: `RowWrite` and `RowOp` arrive with the store slice)*:
  - `pub enum ColumnKind { Text, Number, Integer, Boolean, Date, Datetime, Url, Status, Json }` (`Copy`).
  - `pub struct ColumnSpec { pub id: String, pub kind: ColumnKind, pub required: bool, pub values: Vec<String> }` (`values`: a status column's allowed values, empty otherwise).
  - `pub struct DeclaredTable { pub id: String, pub team: String, pub columns: Vec<ColumnSpec> }`.
  - `pub enum RowStatus { Inserted, Updated, AlreadyWritten }` (`Copy`).
  - `pub struct RowWritten { pub row_id: Uuid, pub version: i64, pub status: RowStatus }` (`Copy`).
  - `pub enum RowWriteError { Refused(String), Failed(String) }`.
  - `pub trait RowWriter: Send + Sync { fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError>; }` (synchronous).
  - `pub enum PreparedRow { Insert { data_json: String }, Update { row_id: Uuid, expected_version: i64, patch_json: String } }` *(tester's choice: the plan's "the op without a row id for an insert")*.
  - `pub trait RowTool: Send + Sync { fn spec(&self, table: &DeclaredTable) -> ToolSpec; fn prepare(&self, arguments_json: &str, table: &DeclaredTable) -> Result<PreparedRow, String>; }`.
- `am_domain::EngineResult` (in `content.rs`, re-exported at the root like `TrustedInput`): private fields; `pub fn row_written(written: &RowWritten) -> EngineResult` (the one constructor); `pub fn label(&self) -> &str`, which is `tool:table_write` *(tester's choice)*; `pub fn text(&self) -> &str`, built only from the row id, the version and one fixed status word (the tests compare a run's result with `EngineResult::row_written(&RowWritten { .. }).text()` for the same three values, and look for `row_id.to_string()` and the version as a word of its own in it). No `Default`, `From`, `TryFrom`, `Deserialize`.
- `Part::EngineResult { call_id: String, result: EngineResult }`.

**am-tools**
- `am_tools::table_write::TableWrite`, built with `TableWrite::new()`, implements `RowTool` *(tester's choice of constructor)*.
- `spec(&table)`: `name == "table_write"`; `input_schema_json` is a JSON object schema with `properties.table` naming the table id (its JSON text holds `"<id>"`), `properties.data.properties.<column id>` for every column with `"type"`: `text`, `url`, `date`, `datetime` → `"string"`; `number` → `"number"`; `integer` → `"integer"`; `boolean` → `"boolean"`; `status` → an `"enum"` equal to its values; `json` → no `"type"` key; and `properties.row_id` and `properties.expected_version` (an update). Nothing but config ids, types and fixed words.
- `prepare(arguments_json, &table)`: `{"table", "data"}` gives `PreparedRow::Insert { data_json }` (the `data` object, compared as JSON); `{"table", "row_id", "expected_version", "data"}` gives `PreparedRow::Update`. An unknown top-level argument is ignored (row 35's canary in an unknown argument key still writes). Refusals are `Err(words)`: `data` not an object (words hold `object`); a key that is no declared column (words name declared column ids); a wrong type (the column id and its type word); a required column missing on insert, or set to `null` (the column id); a status value outside `values` (the column id and its values); serialised `data` over 64 KiB (words hold `64 KiB`); a `row_id` that is not a UUID (words hold `a row this table does not hold`). Words never hold a key or value the model chose.

**am-engine**
- `am_engine::row_writer::StoreRowWriter`, `StoreRowWriter::new(store: &am_store::Store) -> StoreRowWriter`, implements `RowWriter` *(tester's choice of constructor)*. *(re-slice: it reads back through `row_receipt`.)*
- `am_engine::toolgate` *(tester's choices: `StepTools`, `CallPlace`, `GateOutput`, `GateError`)*:
  - `pub struct StepTools { pub grants: BTreeSet<String>, pub seat_team: String, pub table: Option<DeclaredTable> }`;
  - `pub struct CallPlace { pub step_run_id: String, pub call_seq: i64, pub tool_call_id: String, pub taint: Taint }`;
  - `pub enum GateOutput { Engine(EngineResult), Untrusted(UntrustedData) }` (`Debug`);
  - `pub enum GateError { Tool(String), Store(String) }` (`Debug`): `Tool` is an error result the model is shown (untrusted), `Store` stops the step;
  - `ToolGate::shipped(row_writer: Arc<dyn RowWriter>, row_tool: Option<Arc<dyn RowTool>>, audit: Arc<dyn AuditSink>) -> ToolGate`;
  - `ToolGate::over(runner: Arc<dyn ToolRunner>, row_writer: Arc<dyn RowWriter>, row_tool: Option<Arc<dyn RowTool>>, audit: Arc<dyn AuditSink>) -> ToolGate`;
  - `ToolGate::admit(&self, actor: &Actor, step: &StepTools, place: &CallPlace, call: &ToolCall) -> Result<Permit, String>`;
  - `ToolGate::run(&self, permit: Permit, cancel: Arc<dyn CancelSignal>) -> BoxFuture<Result<GateOutput, GateError>>`;
  - *(re-slice, row 44: `pub fn engine_result_of(reader: &am_store::Reader, step_run_id: &str, call_seq: i64) -> Result<Option<EngineResult>, am_store::StoreError>`.)*
  - Refusal words (the tests look for these fragments): not granted: the tool id, `not granted` and the seat id; class: the tool id and its class word (`local-write`, `external-effect`); no table: `declares no table` and the step key; another table: the declared table id and `another table`, never the model's table text; another team: the table id, both team ids and the seat id; unknown: `unknown:<sha256>` (as today). Audit `toolgate.refused`, target `tool:<label> run:<run> step:<key>`, and `args.taint` the call's taint.
- `am_engine::run`: `StepPlan` gains `pub grants: BTreeSet<String>`, `pub seat_team: String`, `pub table: Option<DeclaredTable>`, `pub document: Option<String>`; `RunPlan` gains `pub row_tool: Option<Arc<dyn RowTool>>`. A plan whose step declares a table on a gate, or a table of another team than `seat_team`, is refused by `run_plan` with an `Err` naming the step key and no run row (the variant is the implementer's). *(re-slice: any `document` value is refused the same way, naming 3.6b; `row_inputs` is 3.6c's.)*
- Behaviour the e2e tests pin: a step whose `grants` hold `table_write` and that declares a table (and a plan with a `row_tool`) is offered exactly `[row_tool.spec(&table)]`; every other step none (no runner tool is offered unless granted). Every tool request goes through ToolGate (a refused tool call row and a `toolgate.refused` entry; the step fails in ToolGate's words, no Failure item). A successful `table_write` gives `Part::EngineResult { call_id, result }` in the next request's user message and leaves the attempt's taint as it was; an error gives `Part::ToolResult` with `UntrustedData` labelled `tool:table_write` and taints the attempt from its next call.

**am-config**: `Step` gains `pub table: Option<String>` (key `table:`). *(re-slice: the fan-out warning of row 40 is new.)*

**am-providers**: `Part::EngineResult` renders as the call's tool result (Anthropic `tool_result` block with `tool_use_id`; OpenAI-compatible `role: tool` message with `tool_call_id`) whose content holds `result.text()` and no `<untrusted-data` fence, the same content in both adapters.

**xtask** (`check-deps` findings): name the file and the line of the token; `write_row` and `apply_row_write` findings hold the token and `AD-21`; `EngineResult::row_written` findings hold `EngineResult` and `AD-10`; an `as` alias of `RowWriter` or `EngineResult` names the type; a glob import of `am_domain::rows::*` or `am_domain::content::*` outside `crates/am-domain/` names the file. *(re-slice: the row-content and `rusqlite` rules of row 43 are new; the `apply_row_write` rule is the store slice's, widened here.)*

**Fixture and support** (tests only): `fixtures/projects/rows-documents/` (teams `core` and `side`; seats `writer` (core, Tables on) and `outsider` (side, Tables off); tables `entries` (core) and `others` (side); workflow `record`: `collect` (`table: entries`), `plain` (no table)). *(re-slice: `summarise` joins in 3.6b; a fan-out workflow joins here for row 40.)* Add `fixtures/projects/rows-documents/` to `just validate`'s list with the implementation.

**Ambiguities resolved (tester, 2026-11-24), this slice's:**
- Row 7 at the engine: `taint_rows_declared_table::another_teams_seat_is_refused` accepts either an `Err` naming the step and table with no run row, or a run that offers no `table_write` and refuses every call; am-engine's `toolgate.rs` pins ToolGate's own refusal; am-app's `run_rows.rs` and am-config pin the validate error.
- Row 35's canary "in an unknown argument key": am-tools ignores unknown top-level arguments (an undeclared key inside `data` is still refused, row 14), so the write succeeds and its engine result is checked.
- The planted forms refuse a second `write_row` in `row_writer.rs` and any `fn write_row` in `toolgate.rs`: a fake `RowWriter` for ToolGate's unit tests belongs in `crates/am-engine/tests/`.
- `toolgate.rs`'s existing tests grant the runner's tool by name (`granted(&["lookup"])`), and `loop_caps.rs`'s `step` grants the fixture tool and its test-local runner tools (`GRANTED`); the class test grants `act_on_it` so its refusal stays the class's.

### Red run of the ToolGate slice

*(The tester logs it here after the store slice merges.)*
