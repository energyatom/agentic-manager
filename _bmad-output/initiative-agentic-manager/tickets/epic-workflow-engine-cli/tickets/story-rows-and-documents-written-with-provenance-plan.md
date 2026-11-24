---
title: 'Rows and documents written with provenance'
type: feature
ticket: 6
created: '2026-11-24'
status: approved
baseline_revision: '9d2301e97a8a4b5b537115544dc2b8ac1a21d93c'
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
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** No step can write a row or a document, and the binary runs no tool.
- There is no row store: no `table_rows`, no `row_history`, no row command (am-store, migrations 0001 to 0009).
- ToolGate checks only a manifest and a per-step count. It does not check the seat's grants (AD-11), and it refuses every class but `Read` (`crates/am-engine/src/toolgate.rs` L279-L290: "until stories 3.6 and 4.8").
- The shipped binary has no tool: `run_plan` takes none (`crates/am-engine/src/run.rs` L352), and a tool request fails the step in story 3.1's words (`tool_words`, L286). Tools run only through the test seam `run_plan_with_tools` (L369), guarded by `cargo xtask seam-guard` (owner condition 2 of story 3.3).
- A step declares no table and no document. `am-config`'s `Step` has no such key (`crates/am-config/src/entities.rs` L319-L354). Tables exist as config with one owning team (`Table`, L401-L413).
- am-tools holds only the folder tools (`crates/am-tools/src/lib.rs`); it depends on am-domain, `sha2`, `hex` and `thiserror`, never on am-store (AD-4).
- Every tool result taints the attempt (`run.rs` L1460: "Tool results are untrusted").
- Nothing stores a document version, and nothing raises a Draft item. Three places say Draft items are resolved by "3.6" (`crates/am-store/src/lib.rs` L2398-L2406, `crates/am-app/src/gate.rs` L373-L380, `crates/am-server/src/inbox.rs` L150-L154).

The ticket's verify: a FakeLlm-scripted step's `table_write` lands rows with version and taint, and row history naming run, step and seat. A write to another table, or from another team's seat, is refused. A repeated call with the same call sequence writes once. A document step leaves one Markdown version per run and Draft items in the Inbox. (The owner approved one version per succeeded attempt instead: Owner answers, Q7.)

**Approach (owner-approved, 2026-11-24):**
- **The row store (AD-21, AD-35, AD-7):** migration 0010 adds `table_rows` (table id, row id, JSON data, version, taint) and an append-only `row_history` of after-images. Each after-image names the Actor, run, step run, seat and call sequence. Both are written in one transaction.
- **`table_write` keeps the architecture's placement (Q1):**
  - the tool lives in am-tools: its spec, its argument parsing and its data checks against the declared columns;
  - it writes through a row-writer port, a trait in am-domain with owned types only;
  - am-engine implements the port over the store's writer;
  - only ToolGate calls the port's method, enforced by a new `cargo xtask check-deps` rule;
  - `agentic run` offers `table_write` only to a step that declares a table.
- **ToolGate is the only tool path (AD-11, AD-46):** every run gets a ToolGate, the shipped one included. ToolGate checks the seat's grants, the class, and for `table_write` the step's declared table and its team.
- **A trusted result built from engine values (Q4; AD-10 as amended 2026-11-24):** a successful `table_write` returns a result that ToolGate alone builds, from the row id, the version and a fixed status word. It does not taint. An error result stays `UntrustedData` and taints.
- **Idempotent by run, step run and call sequence (AD-8):** the row write needs a journaled `issued` `table_write` call. It finishes that call in the same transaction. A second write with the same key writes nothing, and the engine reads the result back from the journal.
- **Documents (AD-29, AD-35, AD-43, AD-28):** a step that declares a document stores its final Markdown as a version in the database, with the producing attempt and its taint. It raises one Draft item, in the transaction that finishes the step. A person dismisses it through 3.4's Human-only path.
- **Taint and provenance (AD-10, AD-43):** rows, history rows and versions carry the writer's taint at rest. A row's taint only rises. A row written by a tainted step taints a later step that reads it. Lineage stays in the journal: `row_history.step_run_id`, `document_versions.step_run_id` and a `row` input edge, never a second copy.

## Boundaries & Constraints

**Always:**
- **One tool path (AD-11):** every tool request of every step goes through `ToolGate::admit`, then the journal (`RecordToolCall`), then `ToolGate::run`. Nothing else runs a tool or writes a row for a seat.
- **One caller of the row-writer port (owner, Q1):** `RowWriter::write_row` is called only in `crates/am-engine/src/toolgate.rs`. The store method behind it, `Writer::apply_row_write`, is named only in am-store and in am-engine's implementation of the port (`crates/am-engine/src/row_writer.rs`). `cargo xtask check-deps` enforces both over whole-file text with whitespace normalised; aliases and glob imports are refused.
- **Owned types at the port (AD-5, AD-4):** the port and its types in `am_domain::rows` use only std, `uuid`, `serde` and am-domain types. No am-store, rusqlite or serde_json type crosses it; row data crosses as JSON text.
- **Grants come only from capabilities (AD-46):** `table_write` reaches a seat only through its Tables capability (`crates/am-domain/src/capability.rs` L97-L102). The plan carries the seat's `grants` set from the snapshot (`Seat.grants`, `entities.rs` L196); ToolGate refuses a tool not in it.
- **The binding (AD-21, AD-10 (b), RULE-8):** a seat writes only the table its step declares, and only when that table's team is the seat's team. This holds for every step, tainted or clean. Checked by `agentic validate`, by ToolGate, and again by the writer from the journal.
- **Every row change has its after-image (AD-21, AD-24):** the history row is inserted before the row changes, in the same transaction; a schema trigger aborts a row change with no matching after-image.
- **Append-only (the 3.5 lesson):** `row_history` and `document_versions` refuse UPDATE, DELETE, and any INSERT that meets an existing key (`INSERT OR REPLACE` by id or by unique key). `table_rows` refuses DELETE and REPLACE; its UPDATE must raise the version by one.
- **Optimistic concurrency (owner, Q2):** an update names the version it read. A stale version is refused in words, nothing changes, and the old version stays readable from the row's history and as the current row.
- **Idempotency (AD-8):** the key is the attempt's step run plus the call sequence of a journaled `table_write` call. The write and the tool call's finish commit together.
- **Writer commands with an explicit Actor (AD-6, AD-24):** the row write takes an `Actor` with no default and must be `Actor::Seat` of the attempt; the document finish and the Draft item are `Actor::System` (engine); each commits with its audit entry in one transaction.
- **State machine (AD-8):** no new state or edge. A row is written only while the attempt is `running`. A document step ends `running → succeeded` with its version and Draft item, or `running → failed` with a Failure item.
- **External effects (AD-11, RULE-10):** `table_write` is local-write with no network. External-effect tools stay refused until 4.8, and v1 has no send tools. A Draft item is never sent anywhere.
- **Results (AD-10 as amended 2026-11-24, owner Q4):**
  - a successful `table_write` result is an `EngineResult`: private fields, built only by ToolGate (check-deps), from the row id, the version and a fixed status word; never from row data, an argument, a column name or any model text; it does not taint;
  - every error result (a refusal of an admitted call, a bad row, a stale version) and every runner result stays `UntrustedData` and taints the attempt.
- **Taint never launders (AD-10):** the writer takes the higher of the attempt's committed taint and the command's taint. A row's taint only rises. A version and its Draft item carry the attempt's taint. A row read by a later step carries its at-rest taint into that step's input edge, and its text enters fenced.
- **Audit (AD-25; AD-32 keyed-hash rule, 2026-11-24):** row and document entries hold ids, versions, counts, the call sequence and the taint. Never row data, never Markdown, and no new `*_sha256` of free text. The unknown-tool label keeps the one existing helper (`am_store::audit::sha256_hex`, via `label_of`, `toolgate.rs` L84); story 9.10 keys it.
- **No model words in refusals:** a refusal never echoes a model-chosen table name, row id, column name or value. It names config ids (table, team, seat, columns) only.
- **Inbox (AD-28, AD-15; owner Q8):** only `Actor::System` creates a Draft item. Only a person dismisses one, through 3.4's `Writer::resolve_inbox_item(by: HumanActor, …)` (`lib.rs` L808) and its two sites. 3.4's check-deps resolve rule stays as it is.
- **Model output stays untrusted at render time (AD-29):** Markdown is stored byte for byte. Nothing in this story renders it, fetches a link in it or loads an image from it.
- **Interim widening risk (owner Q3, recorded):** `is_widening` does not exist until 4.17. Until then, adding or changing a step's `table:` or `document:` is not held for the two-step confirmation: an edit that reaches a run's snapshot takes effect unchecked. Accepted by the owner; Sofia adds "changing `table:` or `document:`" to 4.17 as widening in a separate replan. Same shape as 3.3's gate residual.
- **New callers of `Instruction::authored` (owner, 2026-11-24):** this story adds none. If the build needs one, it goes through the same adversarial security review as 3.5.
- **Order of work (owner, 2026-11-24):** Winston's AD-10 amendment merges before the build starts. Then the plan is committed, the red run is logged, and the tests are built first on Opus.
- **Tests first:** every test in the Tests-first list is written and seen red, with its reason logged, before any `src/` change. FakeLlm and FakeClock only; no network, no real model.

**Never:**
- **No second tool path:** no call of `RowWriter::write_row` outside ToolGate, and no naming of `apply_row_write` outside am-store and the engine's port implementation. Not from am-tools, am-app, am-server, a hand-off or the Assistant. `ToolRunner` stays test-only (seam-guard unchanged).
- **No grant outside capabilities:** no step key, plan field or flag grants `table_write`. Declaring a table binds a write; it never grants one.
- **No delete or move of rows or documents.** No `table_delete`, no row rollback, no version replace.
- **No status-transition rule, no reason field, no `v_*` view, no grant read** (stories 7.4, 7.5, 7.2). No Human or Import row write (later stories add their own sites).
- **No document rendering, export or citation resolution** (6.9). No PDF, no HTML conversion.
- **No prompt content type beyond AD-10 as amended 2026-11-24.** `EngineResult` is the one addition, and it carries no free text.
- **No new crate.** One new edge only: am-tools uses the workspace-pinned `serde_json` (in the Stack and already in `Cargo.lock`) to read the call's arguments.
- **No department vocabulary in `crates/`.** Fixture nouns stay in `fixtures/`.
- **Outside this story:** the RULE-8 tainted-step suite (4.9), external-effect approvals (4.8), cost amounts (3.7), resume replays (3.9), hand-off payloads `rows:` and `document:` (3.15), lineage reads (3.17), the golden run (3.13), dry-run marks (AD-37), `write_document` and `draft_message` (Sofia places them), the `is_widening` classification (4.17).

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Offered tools | A step declaring table `T` of its seat's team; the seat's Tables capability on | The request offers exactly `table_write`; its input schema (from am-tools) names `T` and `T`'s column ids and types. A step with no table, or a seat without Tables, is offered no `table_write` | — |
| 2 | Insert | FakeLlm turn: `table_write {table: T, data: {...}}` | One `table_rows` row, version 1, taint = the attempt's; one `row_history` after-image naming the Seat actor, run, step run, seat and call sequence; the tool call `succeeded`; audit `row.written` with ids only; the model gets an `EngineResult` (row id, version 1, `inserted`); the attempt stays clean | — |
| 3 | Update | A later turn: `table_write {table: T, row_id, expected_version: 1, data: {col: v}}` | Row version 2 = version 1 merged with the named columns; a second after-image (the whole row); version 1's after-image unchanged; result `updated`, version 2 | — |
| 4 | Stale version (Q2) | `expected_version: 1` while the row is at 2; and `expected_version: 2` on a row still at 1 | Refused in words ("row `<id>` is at version 2"); nothing changes: the row's data, version and taint as before, no new after-image; every earlier version readable from `row_history`; the current row readable as it was; the tool call `failed`; the model gets the words as an untrusted error result; the step goes on | Tool error |
| 5 | Unknown row | `row_id` not a UUID, or not in `T` | Nothing written; untrusted error result "a row this table does not hold"; the id is not echoed | Tool error |
| 6 | Another table | `table_write {table: U}`, `U` not declared | ToolGate refuses: tool call `refused`, `toolgate.refused` audited (Seat actor, `tool:table_write run:<run> step:<key>`), words name the declared table and never the model's text; the step fails (3.3's rule); no row | Refused, step fails |
| 7 | Another team's table | Step declares a table of team B; the seat is in team A | `agentic validate` error (file, line, fix); a hand-built plan is offered nothing, a call is refused by ToolGate, and the writer refuses it too | Refused |
| 8 | Not granted | Tables off (`table_write` not in `grants`) | `agentic validate` refuses the step's `table:`; a hand-built plan offers no `table_write`; a call is refused "not granted to seat `<id>`" and audited | Refused |
| 9 | Unknown tool | A name no manifest holds | Refused as today, labelled `unknown:<sha256>` by the existing helper; audited | Refused |
| 10 | Other classes | `write_document`, `draft_message`, `workspace_write`, a fixture external-effect tool | Refused as a class this version does not run, granted or not; never run | Refused |
| 11 | No declared table | A granted seat's step with no `table:` calls `table_write` | Refused "step `<key>` declares no table" | Refused |
| 12 | Tainted step | A step with an untrusted plan input writes `T`, then tries `U` | `T`'s row and after-image have taint 1; `U` refused as row 6, audit args carry the taint | Refused for `U` |
| 13 | Taint only rises | Row tainted at version 2; a clean step writes version 3 | Row taint stays 1; version 3's after-image records taint 1, the row's taint after that write, not the writer's (AD-10 restated 2026-11-24); a direct `UPDATE` lowering taint aborts | Trigger |
| 14 | Bad data | Not an object; an undeclared column; a wrong type; a missing required column on insert; a required column set to `null`; a `status` value outside `values`; data over 64 KiB | Nothing written; untrusted error result worded from the table's schema (column ids, types), never the model's keys or values | Tool error |
| 15 | Repeated call sequence | The port's `write_row` twice with one (step run, call sequence) | One after-image, one version; the second writes nothing and returns the first's row id and version (`already written`); `Reader::row_write` gives the same | — |
| 16 | A new attempt | Loop-back attempt 2 of the step, call sequence 1 again | A new write (the key is the attempt's step run, not the step key) | — |
| 17 | Writer refuses | Actor not a Seat; a Seat of another run, step or seat; step not `running`; no `issued` `table_write` call at the key; a table not the attempt's journaled table; insert over an existing row id; version not current + 1 | Refused in words; nothing written | Worded |
| 18 | Append-only | `UPDATE`/`DELETE` on `row_history` or `document_versions`; `INSERT OR REPLACE` on either by id or unique key; `DELETE`/`REPLACE` on `table_rows`; a row change without its after-image; changing a row's table, id or creation time | Aborted by trigger | Trigger |
| 19 | One transaction | A write refused after its after-image was inserted (forced in a store test) | No row, no history, the tool call still `issued` | Rolled back |
| 20 | Repair call | A step with a schema whose final message is invalid | The repair call offers no tools (3.5); a tool call in its reply fails the step; rows from earlier turns stay | — |
| 21 | Step fails after writing | Rows written, then the step fails | Rows stay; their after-images name the failed attempt; no rollback | — |
| 22 | Audit | Any row or document write, with a canary in row data and in the Markdown | Audit args hold ids, op, version, call sequence, counts, taint; no canary and no hash of it in the audit export; the chain verifies | — |
| 23 | Document step | A step declaring `document: D`; FakeLlm's final message is Markdown | Step `succeeded`, output = the Markdown; one `document_versions` row (D, version 1, run, attempt, step run, taint, bytes); one Draft item (payload = that attempt, taint = the version's, title and body in the engine's words); all in the step's finishing transaction; audit `document.versioned` and `inbox.created` | — |
| 24 | Next run | A second run of the same workflow | Version 2 of D; version 1 unchanged | — |
| 25 | Same run again (Q7, owner-approved deviation) | The document step's attempt 2 in the same run (loop-back or gate return) | Version n+1 with its own Draft item; earlier versions and items unchanged; the run's version is its latest | — |
| 26 | Tainted document | A tainted document step | Version taint 1; Draft item taint 1 | — |
| 27 | Empty document | Final message empty, only whitespace, or holding NUL; over 1 MiB | Step fails with a Failure item; no version, no Draft | Failure item |
| 28 | Mixed declarations | `document:` with `output_schema:`, `loop_back:` or `gate:`; `table:` or `document:` on a gate or fan-out step | `agentic validate` error; `run_on` refuses a hand-built plan before any row | Worded |
| 29 | Hostile Markdown | Script tags, raw HTML, a remote image, a link, control characters | Stored byte for byte; the read route returns it as a JSON string; `agentic gate` prints it with control characters neutralised (3.4); FakeEgress sees no request | — |
| 30 | Draft dismissal (Q8) | `agentic gate` on a TTY, a Draft item | Shown with document id, version, run and taint marker; `d` dismisses through `resolve_inbox_item(HumanActor, …, Dismiss, None)` as You (Reviewer); approve, return, reject and answer refused; the run is unaffected | Refused in words |
| 31 | Validate | `table:` unknown; table of another team; seat without Tables; `document:` not kebab-case; one document id in two teams' workflows | Each an error with file, line and fix | Validate error |
| 32 | Migration 0010 | A version-9 database with runs, attempts, calls, tool calls, Inbox items, prompt parts | Pre-migration copy; every row kept value for value; new tables empty; new `step_runs` and `step_inputs` columns NULL on old rows; report 9 → 10; `store.migrated` audited; reopening applies nothing; a failing 0010 names the backup and leaves version 9 | Migration error |
| 33 | Only ToolGate calls the port (Q1) | Planted files: a `write_row` call outside `toolgate.rs`; the same split over lines or with spaces around `.`/`::`; `use …::RowWriter as W`; `use am_domain::rows::*`; `apply_row_write` named outside am-store and `row_writer.rs` | `cargo xtask check-deps` fails naming file and line for each; the shipped tree passes | CI fails |
| 34 | Shipped `agentic run` (Q1) | The `rows-documents` fixture through `plan_of` with FakeLlm | The writing step is offered `table_write` (am-tools' tool, through ToolGate and the engine's port implementation) and its rows land as row 2; the document step and a step with no `table:` are offered none | — |
| 35 | Engine values only (Q4) | Canaries planted in a data value, in an unknown argument key, in an update's patch and in the tool call's arguments text | The `EngineResult` text holds none of them; it holds only the row id, the version and one of the fixed words; after the write the attempt is still clean (taint 0) and its next row is clean | — |
| 36 | Error results stay untrusted (Q4) | A stale version (row 4) or bad data (row 14) | The result is `Part::ToolResult` with `UntrustedData`; the attempt is tainted from its next call; a row it writes after that has taint 1 | — |
| 37 | Only ToolGate builds an engine result (Q4) | Planted files: `EngineResult::row_written` named outside `toolgate.rs`; an alias or glob import of `EngineResult`; spacing or line breaks in the path | `cargo xtask check-deps` fails for each; doctests: no `Default`, `From`, `Deserialize` or struct literal builds one | CI fails |
| 38 | A tainted row taints its reader (Q4) | Step A (tainted) writes row `r`; a later step B reads `r` through its `row_inputs` slot | B's `row` input edge has taint 1 and names `r`'s after-image `seq`; B's prompt holds `r`'s data fenced; B's attempt is tainted; `r`'s mark stays 1. A clean row's edge has taint 0, but its text is still fenced `UntrustedData` (row text is always untrusted, AD-10), so B's attempt is tainted too: the engine raises the attempt's taint for every fenced row input, not only from the edge's mark | — |
| 39 | Non-Human dismissal (Q8) | Dismissing a Draft as `Actor::System` or `Actor::Seat` | Does not compile at `Writer::resolve_inbox_item` (doctest); the writer's own check refuses it with nothing written (a store unit test); no `WriteCommand` resolves (3.4's doctest stands) | Refused |

</intent-contract>

## Code Map

- **`crates/am-domain`:**
  - `src/rows.rs` (new): the row-writer port and its owned types.
    - `DeclaredTable { id, team, columns: Vec<ColumnSpec> }`, `ColumnSpec { id, kind: ColumnKind, required, values }`, `ColumnKind` (text, number, integer, boolean, date, datetime, url, status, json).
    - `RowOp::Insert { row_id: Uuid, data_json }` and `RowOp::Update { row_id: Uuid, expected_version: i64, patch_json }`.
    - `RowWrite { tool_call_id, step_run_id, call_seq, table_id, op, taint }`; `RowWritten { row_id: Uuid, version: i64, status: RowStatus }` with `RowStatus` `Inserted`, `Updated`, `AlreadyWritten`; `RowWriteError::{Refused(String), Failed(String)}`.
    - `trait RowWriter: Send + Sync { fn write_row(&self, actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError>; }`.
    - `trait RowTool: Send + Sync`: `spec(&DeclaredTable) -> ToolSpec` and `prepare(arguments_json, &DeclaredTable) -> Result<PreparedRow, String>` (the op without a row id for an insert).
    - `RowInput { table_id, row_id: Uuid }` (the read slot, Design Notes 5).
  - `src/content.rs` (`Instruction` L88, `UntrustedData` L171, `TrustedInput` L282; module docs L1-L17): `EngineResult` per AD-10 as amended (private fields, one constructor `row_written(&RowWritten)`, no `Default`, `From`, `Deserialize`; its text is fixed words, the row id and the version). Module docs name the fourth kind.
  - `src/llm.rs` `Part` L34-L51: `Part::EngineResult { call_id, result: EngineResult }` beside `ToolResult` (L48-L51).
  - `src/inbox.rs` `Decision::allowed_for` L103-L112: `Draft => [Dismiss]`.
  - `src/tool_manifest.rs` `BUILTINS` L86-L108 (`table_write`, local-write, no network, 100 uses): unchanged. `ToolRunner` L19-L28: unchanged.
  - `src/capability.rs` Tables row L97-L102: unchanged; the only grant source.
- **`crates/am-tools`:**
  - `src/table_write.rs` (new): `TableWrite` implements `RowTool`: the spec from the declared table (column ids and types only), argument parsing, the data checks (types, required, declared columns, status values, 64 KiB), words from config ids only. It never names `write_row`.
  - `src/lib.rs`: module and re-export; crate doc and `Cargo.toml` description widen from folder access.
  - `Cargo.toml`: `serde_json.workspace = true` (Stack, already in the lock).
- **`crates/am-engine`:**
  - `src/row_writer.rs` (new): `StoreRowWriter` implements `RowWriter` over `am_store::Writer::apply_row_write`, reads the result back with `Reader::row_write`, and maps `StoreError::RowRefused` to `RowWriteError::Refused`. The only shipped file besides am-store that names `apply_row_write`.
  - `src/toolgate.rs`:
    - module docs L1-L22;
    - holds `Arc<dyn RowWriter>` and `Option<Arc<dyn RowTool>>`; new `ToolGate::shipped(row_writer, row_tool, audit)`; `over` L130 keeps the runner and adds both;
    - `admit` L214 takes the step's facts (`StepTools`: grants, seat team, declared table) and the call's place (step run, call sequence);
    - `decide` L251: grant check after the unknown check (L272); the class gate L279-L290 lets `LocalWrite` through only for `table_write`; then the binding (it reads only the `table` argument, compares it with the declared id and keeps nothing); `refuse` L321 adds the taint to the audit args;
    - `Permit` L70 carries the call's place; `run` L230 sends a `table_write` permit to `RowTool::prepare`, mints an insert's row id, calls `RowWriter::write_row` (the one call), and builds the `EngineResult` (the one constructor call); an error becomes `UntrustedData`; every other permit goes to the runner.
  - `src/run.rs`:
    - `RunPlan` L181 gains `row_tool: Option<Arc<dyn RowTool>>` (am-app sets it from am-tools).
    - `StepPlan` L150-L177 gains `grants`, `seat_team`, `table: Option<DeclaredTable>`, `document: Option<String>`, `row_inputs: Vec<RowInput>`.
    - `run_plan` L352 builds `StoreRowWriter` and `ToolGate::shipped`; `run_plan_with_tools` L369 builds `over` with both too.
    - `run_on` L404 checks each step's declarations before `CreateRun` (team, document with schema, loop-back or gate).
    - `Tooling` L615-L638: specs per step (granted and available), not per gate.
    - `inputs_of` L909: resolves `row_inputs` from the journal (current version, its after-image `seq`, its taint) into `row` edges and fenced parts.
    - `run_step` L1069: `StartAttempt` L1136 carries `writes`; tools offered L1203-L1206 per step.
    - tool loop L1433-L1460: an `EngineResult` becomes `Part::EngineResult` and leaves `carry` as it is; any untrusted result taints (L1460).
    - `tool_call` L1518 hands ToolGate the step's facts (admit L1569, run L1598, results L1626-L1638).
    - `succeed` L1702: a document step finishes through `SettleAndFinishDocumentStep`; an empty document fails with a Failure item.
    - `tool_words` L286: no longer reached (every request goes through ToolGate).
- **`crates/am-providers`:** `src/anthropic.rs` L60, L96 and `src/openai_compat.rs` L143, L179: render `Part::EngineResult` as the tool's result block with its fixed text, outside any fence. Every adapter matches each `Part` variant by name (AD-10).
- **`crates/am-store`:**
  - `migrations/0010_rows_documents.sql` (new); `src/migrations.rs` `MIGRATIONS` L35-L45 gains it; `SCHEMA_VERSION` L48 becomes 10.
  - `src/lib.rs`:
    - `StoreError` L86: `RowRefused(String)`, a refusal the model may read (stale version, unknown row).
    - `WriteCommand::StartAttempt` L245 gains `writes: AttemptWrites` (seat, declared table, declared document); `start_attempt` L1898 journals them on `step_runs`.
    - New `Writer::apply_row_write(actor, am_domain::rows::RowWrite)` beside `resolve_inbox_item` L808. Not a `WriteCommand`, so `Writer::exec` (L752) cannot write a row.
    - New `WriteCommand::SettleAndFinishDocumentStep` beside `SettleAndFinishStep` L293: settle, finish, version, Draft item, one transaction.
    - `journal_inputs` L1634: accepts a `row` input (taint at least the row's at-rest taint; `from_row` names an existing after-image).
    - `RecordToolCall` L349 and its arm L1336, `FinishToolCall` arm L1358: unchanged; `apply_row_write` finishes the `issued` call itself.
    - `create_item` L2270: unchanged rules; a Draft's payload is its own attempt.
    - `resolved_from` L2398-L2406: Draft leaves it. The resolve's non-Human refusal (3.4) is kept and reached by a unit test for a Draft.
    - `Reader` L2959: `rows(table)`, `row(table, row_id)`, `row_history(table, row_id)`, `row_write(step_run_id, call_seq)`, `document_versions(document)`, `attempt_writes(step_run_id)`.
  - `src/rows.rs` (new): the row write's checks, the merge of a patch into the current data, the SQL. `src/documents.rs` (new): the version and Draft insert.
  - `src/audit.rs` L35 `sha256_hex`: not called for row data or Markdown.
- **`crates/am-config`:**
  - `src/entities.rs` `Step` L319-L354 gains `table: Option<String>`, `document: Option<String>`.
  - `src/kinds.rs`: step keys L2355-L2364 (`table`, `document`); `Step` build L2421; cross-checks beside `hand_off_checks` L2249 (table exists, its team is the seat's team, the seat has Tables on, document id kebab-case and one team, no document with schema, loop-back or gate, none on a gate or fan-out step); `RefKind::Table` (`src/refs.rs` L455).
- **`crates/am-app`:**
  - `src/run.rs` `plan_of` L392-L503: sets `RunPlan.row_tool` to am-tools' `TableWrite`; maps the seat's grants and team, the declared table's columns and team (from `snapshot.tables()`, `snapshot.rs` L52), and the document id; leaves `row_inputs` empty (3.15 and 7.2 fill it); the gate branch L448 builds none.
  - `src/gate.rs`: `later_story` L373-L380 and `decide` L589-L593 (a Draft: shown, `d` to dismiss through the existing `dismiss` L780).
- **`crates/am-server/src/inbox.rs`** L150-L154: Draft leaves the later-story list; `allowed_for` (L176) then admits dismiss.
- **`xtask/src/deps.rs`:** three call-site rules beside `RESOLVE_TOKENS` L99-L116, each over whole-file text with whitespace normalised, refusing `as` aliases and glob imports of the names (3.5's type-wall fix, T5):
  - `write_row` (the port method) stands in shipped code only as its call in `crates/am-engine/src/toolgate.rs`, its definition in `crates/am-domain/src/rows.rs` and the implementation's `fn` line in `crates/am-engine/src/row_writer.rs`;
  - `apply_row_write` stands only in `crates/am-store/src/` and `crates/am-engine/src/row_writer.rs`;
  - `EngineResult::row_written` stands only in `crates/am-engine/src/toolgate.rs` and its definition in `crates/am-domain/src/content.rs`; no file in `crates/` aliases `EngineResult` or `RowWriter`.
  - 3.4's resolve rule (`RESOLVE_TOKENS`, `RESOLVE_SITES` L99-L116): unchanged.
- **`xtask/src/seam_guard.rs`:** `SEAMS` L58-L98 unchanged; the doc lines L27-L31 ("the binary never runs a tool") are reworded: the binary runs only `table_write`, through ToolGate.
- **`crates/am-testkit/src/lib.rs`:** `ScriptedTurn::tool_call` L343 already scripts calls; no change.
- **Docs:** `docs/config-schema.md` L315-L320 (the step keys `table` and `document`).
- **Fixture:** `fixtures/projects/rows-documents/`: two teams, one seat each (Tables on for one), one table per team with `key`, typed and required columns and a status column, a workflow with a writing step and a document step.
- **Existing tests that must change (supersession, as in 3.3 to 3.5):**
  - `crates/am-engine/tests/loop_caps.rs` L428-L469 and `crates/am-engine/tests/run.rs` L609-L645: a tool request with no tool offered now goes through ToolGate (a `refused` tool call, `toolgate.refused` audited, ToolGate's words).
  - `crates/am-app/tests/run_cli.rs` L1100-L1122: kept if ToolGate's words still say "not available"; else the words change.
  - `crates/am-store/tests/prompt_journal.rs` L244-L248: `SCHEMA_VERSION == BEFORE_PROMPT_PARTS + 1` ("0009 is this story's") becomes "the 8 to 9 step is applied".
  - `crates/am-store/tests/inbox_resolve.rs` L575-L611 and L651-L655: Draft leaves the "takes none" and "3.6" lists.
  - Every `StepPlan { .. }` and `RunPlan { .. }` literal gains the new fields (`am-app/src/run.rs` L450, L485; am-engine `src/run.rs` tests and `tests/{run,loop_caps,toolgate}.rs`; am-e2e `tests/engine/{support/mod.rs,handoff_gate_resolve,prompt_assembly_journal,schema_repair}.rs` and `tests/trust/{taint_trusted_input_gate_note,taint_prompt_fence,inbox_resolve_human_only,taint_prompt_type_wall,rule_15_inbox}.rs`). Test helpers grant the fixture tool by name.
  - Every `WriteCommand::StartAttempt` literal gains `writes` (am-store `src/lib.rs` tests and `tests/{run_journal,inbox,prompt_journal,inbox_resolve}.rs`; am-app `tests/{gate_cli,gate_tty}.rs`; am-e2e `tests/trust/{rule_16_audit_log,taint_trusted_input_sources,taint_at_rest,rule_15_inbox,taint_prompt_type_wall}.rs`, `tests/engine/handoff_gate_resolve.rs`; am-engine `src/{journal,run}.rs`).
  - Every exhaustive `match` on `Part` in tests or adapters gains `EngineResult`.
  - `crates/am-engine/tests/loop_caps.rs` L723-L760 (a fixture local-write or external-effect tool is refused): stays green, since only `table_write` passes.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build (owner):** Winston's narrow AD-10 amendment (results built solely from engine values by ToolGate, enforced by check-deps, error results excluded and still untrusted, planted-string test) is merged. #34 merged green on every run. The plan is committed.
- [ ] **Tests first (no `src/` change), on Opus:** every file in the Tests-first list, the fixture, the `[[test]]` targets and the superseded literals. Log the red run with each test's failure.
- [ ] **am-domain:** `rows.rs` (port, types, `RowTool`, `RowInput`); `EngineResult` and `Part::EngineResult`; `Decision::allowed_for`.
- [ ] **am-tools:** `table_write.rs`; `serde_json` edge.
- [ ] **am-store:** migration 0010, `apply_row_write`, `SettleAndFinishDocumentStep`, `AttemptWrites`, the `row` input, the readers, the audit args.
- [ ] **am-config:** the step keys and the cross-checks.
- [ ] **am-engine:** `row_writer.rs`; ToolGate's grants, class gate, binding, port call and engine result; the plan fields; the row-input slot; the shipped gate; the document finish.
- [ ] **am-providers:** render `Part::EngineResult`.
- [ ] **am-app:** `plan_of`'s mapping and `row_tool`; `agentic gate`'s Draft.
- [ ] **am-server:** the Draft later-story line.
- [ ] **xtask:** the three call-site rules and their planted-file tests; the seam-guard doc lines.
- [ ] **Docs:** `docs/config-schema.md` step keys.
- [ ] **Size (owner, Q9):** one squash commit; if the build overruns, stop and report, never split.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then rows, after-images, versions, Inbox items, tool calls, results, taint, audit entries and requests match the row.
- Given the ticket's verify, when the fixture's writing step runs on FakeLlm: its `table_write` lands rows with version and taint, and after-images naming run, step and seat; a write to another table and a write from another team's seat are refused; a repeated call sequence writes once; the document step leaves one Markdown version per succeeded attempt (the owner's approved deviation) and a Draft item in the Inbox.
- Given each owner condition, its matrix row (33 to 39, and 4) and its tests pass.
- Given the workspace, when `just check` runs, then it is green, with 3.3's `taint_*` and `handoff_*`, 3.4's `rule_15_*` and gate suites, and 3.5's prompt and schema suites included. Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-24, manager, from the AD-10 amendment's reviewer gate (PR #35): row 13's after-image records the row's taint after the write (1), not the writer's; row 38: reading any row, clean or tainted, taints the reader, because row text is fenced `UntrustedData` (one test added).
- 2026-11-24, owner: approved with the recommended answers to Q1–Q9 and conditions (Design Notes, Owner answers). The plan now builds Q1 as am-tools' tool over an am-domain row-writer port that am-engine implements and only ToolGate calls, and Q4 as an engine-built trusted result after Winston's AD-10 amendment. Added matrix rows 33 to 39, the row-input slot for the reader test, and their tests. Status set to approved.

## Design Notes

### Owner answers (2026-11-24)

**Owner, verbatim:** "Approved: the 3.6 plan with the recommended answers to Q1–Q9 and these conditions. Q1: a check-deps rule so only ToolGate calls the row-writer. Q2: tests that a stale-version update is refused and the old version stays readable. Q3: record the interim risk, and add "changing table: or document:" to 4.17 as widening. Q4: Winston merges the narrow AD-10 amendment first (results built solely from engine values by ToolGate, enforced by check-deps, error results excluded and still untrusted, planted-string test), and a row written by a tainted step carries that taint to any later reader, with a test. Q7: record as my approved deviation from "one version per run". Q8: Draft dismissal uses the 3.4 Human-only path, with a test that a non-human call cannot dismiss. Q9: one squash commit; stop and report if it overruns. Merge #34 on green on every run. Then commit the plan, log the red run, and build tests first on Opus. Merge only with both CI jobs green after review triage."

**Answers and conditions, as the plan builds them:**
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

### 1. ToolGate as the only tool path

**Today (baseline 53ad57f):**
- The agent loop sends each requested call to `tool_call` (`run.rs` L1518): the deadline, the tool-call cap from the journal, `ToolGate::admit` (L1569), `RecordToolCall issued` (committed before the tool runs, AD-8), then `ToolGate::run` (L1598).
- Only `ToolGate::run` calls a `ToolRunner` (`toolgate.rs` L230-L247). A `Permit` is made only by `admit` and run only by the gate that made it.
- No `check-deps` rule names tools today. "Only ToolGate calls tools" is held by `cargo xtask seam-guard` (`xtask/src/seam_guard.rs` L80-L97): `ToolRunner` is named only in am-domain, `toolgate.rs`, am-testkit and `run_plan_with_tools`' signature, and `run_plan_with_tools` is named nowhere else in shipped code. So the shipped binary runs no tool.
- ToolGate checks: a Seat actor; a duplicate id; a manifest; class `Read` only; the per-step use limit. It does not check grants.

**After this story (owner Q1):**
- **The pieces and who calls whom:**
  - am-tools' `TableWrite` (a `RowTool`) builds the spec and turns a call's arguments into a checked `PreparedRow`. It writes nothing and never names the port's method.
  - am-domain's `RowWriter` is the port. am-engine's `StoreRowWriter` implements it over `am_store::Writer::apply_row_write`.
  - ToolGate holds both. It is the one caller of `RowWriter::write_row` and the one builder of an `EngineResult`.
  - am-app wires am-tools' tool into the plan (`RunPlan.row_tool`), because am-engine may not depend on am-tools (AD-4). am-engine builds its own `StoreRowWriter` from the store it runs on.
- Every run has a ToolGate. `run_plan` builds `ToolGate::shipped`. Tests still pass fixture runners through `run_plan_with_tools`.
- Every tool request of every step goes through `admit`, including a step offered no tool. There is no other path.
- `agentic run` offers `table_write` only to a step that declares a table, whose seat holds the grant and whose table belongs to the seat's team.
- Admission order (the first refusal wins, as in 3.3):
  1. the actor is a Seat in a step (existing);
  2. not a duplicate id (existing);
  3. a known tool: a runner manifest or a built-in id, else `unknown:<label>` (existing);
  4. granted: the tool is in the seat's `grants` (new);
  5. class: `Read` runs; `LocalWrite` only for `table_write`; anything else refused (4.8 brings external-effect);
  6. for `table_write`: the step declares a table; the call's `table` argument equals it; the table's team is the seat's team (new). ToolGate reads only that one argument and keeps nothing of it;
  7. the per-step use limit (existing).
- **Run:** `RecordToolCall issued`, then `RowTool::prepare` (a bad row is an untrusted error result), then ToolGate mints an insert's row id (UUIDv7), builds the `RowWrite` from the call's place and the attempt's taint, and calls `RowWriter::write_row`. Its `RowWritten` becomes the `EngineResult`; its `Refused` words become an untrusted error result; `Failed` stops the step as a store error.

**The check-deps rules (owner Q1 and Q4 conditions):**
- Each is matched over whole-file text with whitespace around `.` and `::` normalised and line breaks joined (3.5's T5 fix, as 3.4's resolve rule does).
- `as` aliases of `RowWriter`, `write_row`'s trait path or `EngineResult`, and glob imports of `am_domain::rows::*` or `am_domain::content::*` outside am-domain, are refused.
- **Port method:** `write_row` stands in shipped code only as the call in `crates/am-engine/src/toolgate.rs`, the trait's definition in `crates/am-domain/src/rows.rs` and the `fn write_row` line of `crates/am-engine/src/row_writer.rs`.
- **Store method:** `apply_row_write` stands only in `crates/am-store/src/` (its definition and `#[cfg(test)]` items) and `crates/am-engine/src/row_writer.rs`.
- **Engine result:** `EngineResult::row_written` stands only in `crates/am-engine/src/toolgate.rs` and its definition in `crates/am-domain/src/content.rs`.
- Later Human (7.x) and Import (12.x) row writes add their own named sites, each in its own story.

**How `table_write` reaches a seat (AD-46):**
- Its only grant is the Tables capability: `capability::expand` (`capability.rs` L240) puts `table_read` and `table_write` in `Seat.grants`.
- `agentic validate` already refuses `table_write` in `tools:` while Tables is off (`kinds.rs` L2022-L2040).
- A step's `table:` does not grant anything. It names the one table the granted tool may write.
- Its manifest is local-write, no network, 100 uses per step (`tool_manifest.rs` L100). The step's `limits.tool_calls` cap (default 20) still applies.

**Refusals, in words and audited:**

| Case | Words (engine's; config ids only) | Journal and audit |
|------|-----------------------------------|-------------------|
| Unknown tool | `Tool "unknown:<hex>" was refused: it is an unknown tool (no manifest).` (existing) | tool call `refused`; `toolgate.refused`, target `tool:unknown:<hex> run:<run> step:<key>` |
| Not granted | `Tool "<id>" was refused: it is not available to seat <seat>, which is not granted it (its <capability> capability is off).` | as above, target `tool:<id> …` |
| Class not run | `Tool "<id>" was refused: it is a <class> tool, and this version runs read tools and table_write only.` | as above |
| No declared table | `Tool "table_write" was refused: step <key> declares no table, so it writes none.` | as above |
| Another table | `Tool "table_write" was refused: step <key> writes only table <declared>, and this call names another table.` | as above; the model's table text is not stored or hashed |
| Another team | `Tool "table_write" was refused: table <declared> belongs to team <team>, and seat <seat> is in team <seat-team>; a seat writes only its own team's tables.` | as above |
| Tainted step, another output | The same words as "another table" (the binding already holds for every step) | as above; the audit args carry `taint: 1` |

- Each refusal ends the step as 3.3 does (`fail`, no Failure item). The run fails in words.
- Refusals of a call ToolGate admitted (stale version, unknown row, bad data) are untrusted error results the model can act on (matrix rows 4, 5, 14, 36). The tool call ends `failed` and the step goes on.

### 2. The row store

**Migration `0010_rows_documents.sql`** (additive, forward-only, AD-7):

`table_rows` (one row per current row, AD-21):

| Column | Why |
|--------|-----|
| `table_id TEXT NOT NULL` | The config table (`tables/<id>.yaml`); one generic store, no per-table DDL (AD-21) |
| `row_id TEXT NOT NULL` | UUIDv7 minted by ToolGate; never the model's text, so audit and the result can name it |
| `data TEXT NOT NULL` (`json_valid`, an object) | The row's columns as JSON (AD-21) |
| `version INTEGER NOT NULL ≥ 1` | Optimistic concurrency; the version of the newest after-image |
| `taint INTEGER NOT NULL (0, 1)` | Marked at rest (AD-10 (c)); only rises |
| `created_at`, `updated_at` | AD conventions (UTC, ms, `Z`) |
| `PRIMARY KEY (table_id, row_id)` | One row per id |

`row_history` (append-only after-images, AD-21, AD-24):

| Column | Why |
|--------|-----|
| `seq INTEGER PRIMARY KEY AUTOINCREMENT` | Order; the high-water mark AD-22's `start_seq` reads (7.2); the snapshot a `row` input edge names (AD-43) |
| `table_id`, `row_id`, `version` | Which row, which version; `UNIQUE (table_id, row_id, version)` |
| `data TEXT NOT NULL` | The whole row after the change (an after-image, not a patch) |
| `taint INTEGER NOT NULL` | The writer's taint for this version |
| `actor TEXT NOT NULL` | The Actor as JSON (AD-24); a Seat here |
| `run_id`, `step_run_id`, `seat`, `call_seq`, `tool_call_id` | Provenance: run, step attempt and seat; the idempotency key `UNIQUE (step_run_id, call_seq)`; the tool call it finished. All set for a Seat, all NULL for a later Human or Import write (CHECK) |
| `op TEXT` (`insert`, `update`) | What the change was |
| `created_at` | When |

- No `reason` column yet: 7.4 adds it with `ALTER TABLE … ADD COLUMN`, additive.
- No `dry_run` column yet: the run-mode story (AD-37) adds it the same way.

`step_runs` gains, NULL for every old row (what the attempt may write, journaled at its start):
- `seat TEXT`, `writes_table TEXT`, `writes_document TEXT`.
- A trigger refuses changing any of them once set.
- Why: the writer checks a row write against the journal, not only against the engine's word (as 3.4 journaled `max_returns`). It is a fact of the attempt, not a second copy of config: the run pins only the snapshot's hash (AD-16).

`step_inputs` gains `from_row INTEGER REFERENCES row_history(seq)`, NULL for every old row (the `row` input edge, Design Notes 5).

`document_versions`: see 4.

**Triggers:**
- `row_history_no_update`, `row_history_no_delete`, `row_history_never_replaced` (an INSERT meeting an existing `seq`, `(table_id, row_id, version)` or `(step_run_id, call_seq)` aborts).
- `table_rows_no_delete`; `table_rows_never_replaced` (an INSERT meeting an existing `(table_id, row_id)` aborts); `table_rows_fixed_fields` (`table_id`, `row_id`, `created_at`).
- `table_rows_version_steps` (UPDATE must set `version = OLD.version + 1`); `table_rows_taint_only_rises`.
- `table_rows_needs_after_image`: INSERT and UPDATE abort unless `row_history` already holds `(table_id, row_id, NEW.version)` with `data = NEW.data` and `taint <= NEW.taint`. So no row changes without its after-image, and the after-image is written first, in the same transaction.
- Later deletion (AD-25: deleting a row really deletes its content) is a Human command in a later story, with its own migration replacing these triggers.

**The port and the store method:**
- `RowWriter::write_row(actor: Actor, write: RowWrite) -> Result<RowWritten, RowWriteError>` (am-domain). `StoreRowWriter` (am-engine) calls `Writer::apply_row_write(actor, write) -> Result<(), StoreError>`, then reads the result back with `Reader::row_write(step_run_id, call_seq)`.
- The actor must be `Actor::Seat`. Its run and step must be the step run's; its seat must be the attempt's journaled `seat`.
- `apply_row_write` checks, in one transaction, refused in words with nothing written:
  1. a history row at `(step_run_id, call_seq)` exists: write nothing (the replay, 3);
  2. the step run is `running`;
  3. a `table_write` tool call at `(step_run_id, call_seq)` is `issued`, with this `tool_call_id`;
  4. `table_id` equals the attempt's `writes_table`;
  5. insert: no row with `row_id`; update: the row exists and is at `expected_version`, else `RowRefused` (matrix rows 4 and 5).
- Then: taint = max(attempt's committed taint, the write's taint, the row's taint); merge an update's patch into the current data (top-level keys; `null` clears a column); insert the after-image; insert or update the row; finish the tool call `succeeded`; audit `row.written`.
- **A stale update (owner Q2):** refused at check 5 before anything is written, so the row, its version, its taint and its history stay exactly as they were. The words name the row id (a store id) and its current version. The earlier versions stay readable through `Reader::row_history`, and the current one through `Reader::row`.

**The data (am-tools, `table_write.rs`):**
- Checked against the declared columns, by hand, in am-tools: the call's `data` is an object; every key a declared column; types `text`, `url`, `date`, `datetime` → string, `number` → number, `integer` → integer, `boolean` → boolean, `status` → one of `values`, `json` → any; `null` only for a column that is not required.
- Insert: every required column present. Update: only the named columns; a required column cannot become `null`.
- At most 64 KiB of serialised data.
- No cleaning, trimming or normalising (AGENTS.md: that is the seat's job). No status-transition check (7.4).
- Words name config ids and types only, never the model's keys or values.
- `jsonschema` stays only in am-engine's `schema.rs` (3.5); am-tools adds no validator crate.

**Is row data hashed into audit?** No.
- `row.written`: target `row:<table_id>/<row_id>`; args `op`, `version`, `step_run_id`, `call_seq`, `taint`, `columns` (a count).
- No `data`, no hash of it, no `*_sha256` key. Table ids and row ids are config ids and UUIDv7s, not free text.
- The step's own finish keeps 3.5's `output_sha256` through the existing helper; this story adds no new hash.

### 3. Idempotency

- **The key:** (run, step run, call sequence). The step run is the attempt (`step_runs.id`, unique per run, step key and attempt), so the run is implied. The call sequence is the `tool_calls.call_seq` the engine journaled `issued` before the tool runs (`UNIQUE (step_run_id, call_seq)` since 0007).
- **Why the attempt, not the step key:** a loop-back attempt 2 starts its call sequences at 1 again. A key of the step key alone would silently drop attempt 2's writes (matrix row 16).
- **A repeated call:** `apply_row_write` with a key that already has an after-image writes nothing: no row, version, history row or audit entry. The port returns the first call's row id and version with the status `AlreadyWritten`, read from the journal (`Reader::row_write`). How a no-op is said in the audit is story 3.18's.
- **Crash between the write and the tool result:** there is no such window in the store. The after-image, the row and the tool call's `succeeded` commit in one transaction.
  - Crash before it commits: the tool call stays `issued`, nothing written. 3.9 re-runs the call with the same key and it writes once.
  - Crash after it commits, before the model saw the result: the call is `succeeded`. 3.9 rebuilds the engine result from `row_write`; a re-sent write is a replay.
  - Until 3.9, `continue_run` refuses a run with a tool call in flight (`run.rs` L460, 3.4).
- **Retries (3.8):** a retried model turn that asks for `table_write` again gets a new call sequence. That is a new write, as the model asked for it twice. Retry never re-sends a tool call.
- **3.5's repair call:** it offers no tools (`run.rs` L1203-L1206), and a tool call in its reply fails the step. So no row is written by a repair, and no key is spent on one. Rows from turns before a failed repair stay (matrix row 20).

### 4. Documents

- **What makes a version (owner Q5):** the document step's validated final message, as the ticket and R10 say. A step declaring `document: D` stores that Markdown as a version when it succeeds. "Validated" means: not empty after trimming, no NUL, at most 1 MiB. Otherwise the step fails with a Failure item and no version.
- **Where it lives (owner Q6; AD-35: documents and versions are database rows):** `document_versions`:

| Column | Why |
|--------|-----|
| `id TEXT PRIMARY KEY` | UUIDv7 |
| `document_id TEXT NOT NULL` | The step's declared document |
| `version INTEGER NOT NULL ≥ 1` | Numbered per document by the writer (MAX + 1 in the transaction); `UNIQUE (document_id, version)` |
| `run_id`, `attempt` | "Versioned per run" (AD-29); the run's latest version is its version |
| `step_run_id TEXT NOT NULL UNIQUE` | The producing attempt (AD-43); one version per attempt |
| `taint INTEGER NOT NULL` | The attempt's taint at finish, marked at rest (AD-10 (c)) |
| `bytes INTEGER NOT NULL` | Size, for lists without reading text |
| `created_at` | When |

  - The Markdown is not copied. It stays once on the producing attempt's `step_runs.output` (AD-43's "never stored twice", as `step_inputs` and `prompt_parts` do). The existing read route already serves an item's payload text (`am-server/src/inbox.rs` L83-L110).
  - Triggers: no update, no delete, never replaced.
- **Version numbering (owner Q7, an approved deviation):** per document, 1, 2, 3 in commit order across runs. A second attempt in the same run (a loop-back or a gate return) adds a version and never replaces one. The ticket says "one Markdown version per run"; the owner approved one version per succeeded attempt instead, recorded here as the owner's deviation. A run with one attempt still leaves exactly one.
- **The finishing transaction (`SettleAndFinishDocumentStep`):** settle the call; the step `succeeded` with its output; the version; one Draft item; audits `model_call.settled`, `step.finished` (3.5's `output_sha256` via the helper), `document.versioned` (document id, version, run, step run, taint, bytes) and `inbox.created`. The writer checks the attempt's `writes_document` equals `document_id`.
- **Draft items (owner Q8; AD-28):**
  - Kind `draft`; payload the producing attempt; taint at least the version's (`create_item`, `lib.rs` L2276-L2292, already raises it).
  - One per version. 0008's `UNIQUE (step_run_id, kind)` already allows exactly that, so no table rebuild.
  - Title and body are the engine's words: "Draft: `<document-id>` version `<n>`" and "Step `<key>` wrote version `<n>` of `<document-id>` in run `<run>`. Drafts only: Agentic Manager never sends it." No model text.
  - It never blocks the run: the step succeeded and the run goes on.
  - **Dismissal uses 3.4's Human-only path:** `Writer::resolve_inbox_item(by: HumanActor, item, Decision::Dismiss, None)`, from its two sites (`am-server/src/inbox.rs`, UI class; `am-app/src/gate.rs`). 3.4's check-deps resolve rule is unchanged. A non-Human call cannot dismiss: `HumanActor` cannot be built from an `Actor` (compile_fail doctests), the writer refuses any non-Human actor in its own check (3.4's defence in depth, reached by a unit test for a Draft), and no `WriteCommand` resolves.
  - 3.4's rules hold: a resolved item never changes; `inbox.updated` carries only id and status; dismiss carries no note; a tainted item's dismiss needs no typed id (only approve does).
  - `agentic gate` shows a Draft's document id, version, run and taint marker, and the Markdown through 3.4's neutraliser (`gate.rs` L258, L341).
- **Untrusted at render time (AD-29):** the Markdown is model output. It is stored byte for byte, returned by the API as a JSON string, and printed by the CLI with control characters neutralised. It is rendered only through the sanitising Markdown renderer (6.9), which is not in this story. Nothing here fetches a link or loads an image (matrix row 29).

### 5. Taint and provenance

- **At rest (AD-10 (c)):**
  - A tainted attempt's rows: `table_rows.taint = 1` and the after-image's `taint = 1`.
  - A tainted document step's version: `document_versions.taint = 1`, and its Draft item's taint is 1.
  - A row's taint only rises: a clean rewrite of a tainted row keeps it tainted (matrix row 13). Taint never launders through a later write.
- **The engine result does not launder (owner Q4; AD-10 as amended 2026-11-24):**
  - It carries only the row id (a store UUID), the version (a number) and one fixed status word (`inserted`, `updated`, `already written`). It is built from `RowWritten`, which the engine reads from the journal. Nothing the model wrote, no row data and no column name reaches it.
  - So a successful write leaves the attempt as clean as it was (matrix row 35). A step that was tainted stays tainted, and its rows are tainted.
  - Every error result stays `UntrustedData`: its words can follow from the model's input, so it taints (matrix row 36).
- **A tainted row taints its reader (owner Q4 condition):**
  - The reader: `StepPlan.row_inputs`, a typed slot of rows a step reads at its start (3.5's precedent: a slot the shipped `plan_of` leaves empty and tests fill). 3.15 (`rows:` hand-off payloads) and 7.2 (granted reads) fill it from config.
  - At the attempt's start the engine reads each row's current version from the journal and journals a `row` input edge: label `row:<table>/<row-id>@<version>`, `from_row` the after-image's `seq` (AD-43's "row snapshot seq"), taint at least the row's at-rest taint (the writer checks it).
  - The row's data enters the prompt as `UntrustedData` inside the fence (AD-10: free text in rows is always untrusted), so the reading attempt is tainted. Its text is not copied into `prompt_parts` (it stays on the after-image).
  - Test (matrix row 38): a tainted step writes a row; a later step reading it is tainted, its edge has taint 1, and the row's mark is unchanged. A clean row's edge has taint 0, so lineage tells the two apart, and its text is still fenced, so the reading attempt is tainted either way (row text is `UntrustedData`).
  - No row or version yields a `TrustedInput` (AD-10): its two constructors stay am-engine's (check-deps), and neither takes a row or a version. A `next` input from a document step is still a step output (3.3's path, with the producer's committed taint).
- **A tainted step's writes (AD-10 (b)):** the binding holds for every step, so a tainted step writes only its declared table. Other local-write tools stay refused by class. Story 4.9 adds the RULE-8 suite for the tainted cases.
- **Lineage (AD-43):** computed from the journal, never stored twice.
  - Each after-image names its step run; each version names its producing step run; each `row` edge names the after-image it read. That is what AD-43 asks for.
  - No lineage table, no denormalised copy. Story 3.17 walks `step_inputs`, `row_history` and `document_versions`.

### 6. Owner decisions (were Questions for the owner)

All nine were answered on 2026-11-24 with the recommended answers and the conditions above. None remains open.

**Q1. Where `table_write` lives. Decided: in am-tools, over an am-domain port, implemented by am-engine, called only by ToolGate.**
- Keeps the Structural Seed and the AD-21/AD-36 Binds lines (am-tools). AD-4 lets am-tools depend only on am-domain and am-net, hence the port.
- The port's types are owned (std, `uuid`, am-domain); row data crosses as JSON text.
- am-app wires am-tools' tool into the plan; am-engine builds the port's implementation from its own store.
- `agentic run` offers it only to a step that declares a table.
- Condition: three check-deps rules (port method in ToolGate only; store method in am-store and the engine's implementation only; aliases and globs refused), with planted-file tests.
- `ToolRunner` stays test-only; seam-guard's rules are unchanged.

**Q2. Updates. Decided: insert, or update by row id with an expected version; no delete; every after-image kept.**
- Condition: a stale update is refused in words, nothing changes, and the old version stays readable (from `row_history`, and as the current row when the update was refused).

**Q3. Declarations and owners. Decided: step keys `table:` and `document:`.**
- Validate: the table exists, its team is the seat's team, the seat has Tables on; a document id is kebab-case and owned by the declaring seat's team, one team per id.
- Condition: the interim risk is recorded (Boundaries, "Interim widening risk"). Sofia adds "changing `table:` or `document:`" to 4.17 as widening in a separate replan; this plan edits no ticket.

**Q4. The result. Decided: an engine-built trusted result, under AD-10 as amended 2026-11-24.**
- ToolGate builds `EngineResult` solely from engine values: the row id, the version and a fixed status word. No row data, argument or model text.
- Conditions: Winston's narrow AD-10 amendment merges before the build; the constructor is named only in `toolgate.rs` (check-deps, planted files); error results stay `UntrustedData` and taint; a planted-string test; a row written by a tainted step taints a later reader, with a test.

**Q5. What writes a version. Decided: the document step's final message.** `write_document` stays refused here; Sofia places it before 3.13, whose golden run calls it.

**Q6. Database row or file. Decided: a `document_versions` row, with the Markdown kept once on the producing attempt's output.**

**Q7. Per run or per attempt. Decided: one version per succeeded attempt.** Condition: recorded as the owner's approved deviation from the ticket's "one Markdown version per run" (Intent, Documents, matrix row 25). The ticket text is unchanged.

**Q8. Draft items. Decided: one per version, dismissable here.** Condition: dismissal uses 3.4's Human-only path, with a test that a non-Human call cannot dismiss.

**Q9. Size. Decided: one squash commit.** Condition: if the build overruns, stop and report; never split.
- Touches: am-domain (a port module, the engine result, a `Part` variant, one line), am-tools (a module and one edge), am-store (a migration with three tables, triggers and two columns, a writer method, a command, an input kind, readers), am-engine (the port implementation, ToolGate, the plan, the row-input slot, the document finish), am-providers (two adapters), am-config (two keys, cross-checks), am-app (`plan_of`, `agentic gate`), am-server (one line), xtask (three rules), a fixture, about 20 superseded test files.
- Estimate: 5,000 to 6,500 changed lines with tests, larger than 3.5 and larger than the draft's estimate.

**Decided within the ADs (accepted with the plan):**
1. No new crate. am-tools gains the workspace-pinned `serde_json` (Stack, already in the lock). `uuid` and `serde_json` are already am-engine and am-store dependencies; `rusqlite` is bundled with JSON functions.
2. ToolGate refusals fail the step as 3.3 does. Refusals of admitted calls (stale version, unknown row, bad data) are untrusted error results the model can act on.
3. Row data is checked against the declared columns in am-tools, at most 64 KiB, never normalised.
4. An update merges the named columns into a whole after-image, in the store's transaction.
5. Row ids are UUIDv7s that ToolGate mints. The `key` column is data.
6. A row's taint only rises.
7. Rows written by a step that later fails or loops back stay; their after-images name the attempt. No rollback, since rollback would delete data.
8. Each attempt journals its seat, declared table and declared document at its start.
9. Every tool request goes through ToolGate, so 3.1's "tools are not available" path is superseded (Code Map).
10. A document step takes no `output_schema`, `loop_back` or `gate`. Its check is non-empty, no NUL, at most 1 MiB; there is no repair.
11. `agentic status` and the API do not list rows or versions here (7.1, 6.9).
12. The `table_write` spec is built from config ids and types only, not column descriptions, so no new owner text reaches the model outside the journaled prompt.
13. The row-input slot is filled only by tests in this story; `plan_of` leaves it empty.

### 7. Tests-first list

All on FakeLlm and FakeClock; no network; no real model. Red reasons are what the first run shows before any `src/` change; most are compile errors for names that do not exist yet. Each am-e2e file gets its `[[test]]` target (`trust_targets_listed`). Tests are written on Opus (owner).

**am-domain (doctests, `crates/am-domain/src/content.rs` and `src/rows.rs`):**
- `EngineResult`: an allowed use (read its text); `compile_fail` for `EngineResult::default()`, `EngineResult::from(String)`, a struct literal, and `serde_json::from_str::<EngineResult>` (row 37). Red: no type (the doctests are added with the failing names, as 3.5 logged).
- `RowWrite` and `RowOp` hold owned types only (an allowed build from `String`, `Uuid`, `i64`).

**am-tools (`crates/am-tools/tests/table_write.rs`, new):**
- `the_spec_names_the_declared_table_and_columns` (row 1).
- `bad_rows_are_refused_in_words_from_the_columns` (row 14, seven cases).
- `refusals_never_echo_the_models_keys_or_values` (rows 14, 22).
- `table_write_names_no_row_writer` (the module text holds no `write_row`; the xtask rule is the real guard).
- Red for each: no `am_tools::table_write`.

**am-store (`crates/am-store/tests/rows.rs`, new):**
- `migration_0010_keeps_every_row_and_the_pre_migration_copy` (row 32). Red: `SCHEMA_VERSION` is 9.
- `migration_0010_adds_empty_tables_and_null_columns` (row 32). Red: no `table_rows`.
- `reopening_a_version_10_database_applies_nothing` (row 32). Red: as above.
- `a_failing_0010_names_the_backup_and_leaves_version_9` (row 32). Red: as above.
- `an_insert_writes_the_row_and_its_after_image_together` (row 2). Red: no `Writer::apply_row_write`.
- `an_update_needs_the_current_version_and_appends_an_after_image` (row 3). Red: as above.
- `a_stale_update_is_refused_in_words_and_nothing_changes` (row 4, owner Q2: row data, version, taint and history byte-equal before and after). Red: as above.
- `the_old_version_stays_readable_from_history_and_as_the_current_row` (row 4, owner Q2: after a successful update, version 1 from `row_history`; after a refused update, the unchanged current row from `Reader::row`). Red: no readers.
- `a_repeated_call_sequence_writes_once` (row 15, the verify). Red: no `apply_row_write`.
- `the_key_is_the_attempt_not_the_step` (row 16). Red: as above.
- `the_writer_refuses_a_write_that_is_not_its_attempts` (row 17, seven cases). Red: as above.
- `rows_and_history_are_append_only` (row 18). Red: no tables, no triggers.
- `a_row_change_without_its_after_image_is_aborted` (row 18). Red: as above.
- `a_refused_write_leaves_nothing` (row 19). Red: no `apply_row_write`.
- `row_taint_only_rises` (row 13). Red: no table.
- `a_row_input_edge_carries_the_rows_taint` (row 38: `journal_inputs` takes a `row` input whose taint is at least the row's). Red: no `row` kind.
- `row_audit_holds_ids_never_data` (row 22). Red: no `row.written`.
- `a_document_version_and_its_draft_commit_with_the_step` (row 23). Red: no `SettleAndFinishDocumentStep`.
- `document_versions_number_per_document` (rows 24, 25). Red: as above.
- `document_versions_are_append_only` (row 18). Red: no table.
- `a_draft_is_dismissed_only_by_a_person` (row 30: Human dismiss resolves; approve, return, reject, answer refused). Red: `allowed_for(Draft)` is empty.
- Unit test in `src/lib.rs` (`#[cfg(test)]`): `a_non_human_resolve_of_a_draft_is_refused_in_the_writer` (row 39: `Actor::System` and `Actor::Seat` through the writer's resolve path, refused, nothing written). Red: Draft refused as "3.6" for every actor, so the test asserts the Human case succeeds and fails.
- Doctest on `Writer::resolve_inbox_item`: `compile_fail` dismissing with `Actor::Seat { .. }` (row 39), beside 3.4's `Actor::System` one.
- **Superseded:** `prompt_journal.rs` L244-L248; `inbox_resolve.rs` L575-L611, L651-L655.

**am-engine (`crates/am-engine/tests/`):**
- `toolgate.rs` (existing): `a_tool_not_granted_to_the_seat_is_refused_and_audited` (row 8); `table_write_is_admitted_only_for_the_declared_table` (rows 6, 11); `another_teams_table_is_refused` (row 7); `other_local_write_and_external_effect_tools_stay_refused` (row 10); `a_refusal_never_echoes_the_models_table_name` (row 6); `a_write_comes_back_as_an_engine_result` (rows 2, 35); `an_error_comes_back_untrusted` (row 36). Red: `admit` takes no step facts; no port.
- `row_writer.rs` (new): `the_store_row_writer_reads_its_result_from_the_journal` (rows 2, 15); `a_stale_version_maps_to_refused` (row 4). Red: no `am_engine::row_writer`.
- **Superseded:** `loop_caps.rs` L428-L469; `run.rs` L609-L645.

**am-config (`crates/am-config/tests/step_writes.rs`, new):**
- `a_step_declares_its_table_and_document` (row 31). Red: unknown keys `table`, `document`.
- `an_unknown_table_is_an_error_with_file_line_and_fix` (row 31).
- `a_table_of_another_team_is_refused` (rows 7, 31).
- `a_seat_without_tables_declares_no_table` (rows 8, 31).
- `a_document_step_takes_no_schema_loop_back_or_gate` (rows 28, 31).
- `one_document_belongs_to_one_team` (row 31).
- Red for each: the keys are not read.

**am-providers (existing test files):** `an_engine_result_renders_as_a_tool_result_outside_any_fence` for both adapters (rows 2, 35). Red: no `Part::EngineResult`.

**am-e2e engine (`crates/am-e2e/tests/engine/`):**
- `rows_table_write.rs`: `a_scripted_table_write_lands_rows_with_version_taint_and_history` (rows 2, 3, the verify); `a_stale_version_is_a_tool_error_and_the_row_is_unchanged` (row 4, owner Q2); `an_unknown_row_is_a_tool_error_without_its_id` (row 5); `a_bad_row_is_a_tool_error_in_words` (row 14); `rows_of_a_failed_step_stay_and_name_its_attempt` (row 21); `a_repair_call_offers_no_table_write` (row 20). Red: no `StepPlan.table`.
- `rows_idempotent.rs`: `a_repeated_call_sequence_writes_once_and_reads_back_the_same_result` (row 15, the verify); `a_loop_back_attempt_writes_again` (row 16); `a_succeeded_call_always_has_its_after_image` (row 19, over every write of a run). Red: as above.
- `documents_versions.rs`: `a_document_step_leaves_one_version_and_one_draft` (row 23, the verify); `a_second_run_adds_version_two` (row 24); `a_returned_document_step_adds_a_version_in_the_same_run` (row 25, the owner's approved deviation, Q7); `an_empty_document_fails_with_a_failure_item` (row 27); `a_mixed_declaration_is_refused_before_any_row` (row 28). Red: no `StepPlan.document`.

**am-e2e trust (`crates/am-e2e/tests/trust/`):**
- `capabilities_tables_grant.rs` (AGENTS.md: "off means absent and refused"): `tables_off_means_table_write_absent_and_refused` (rows 1, 8); `an_unknown_tool_is_refused_and_audited_by_label` (row 9); `every_tool_request_goes_through_toolgate` (rows 6, 9, 34, the shipped gate). Red: no grants check, no shipped gate.
- `taint_rows_declared_table.rs`: `a_write_to_another_table_is_refused_and_audited` (row 6, the verify); `another_teams_seat_is_refused` (row 7, the verify); `a_tainted_step_writes_only_its_declared_table` (row 12). Red: no binding.
- `taint_rows_at_rest.rs`: `a_tainted_steps_rows_carry_taint_in_row_and_history` (row 12); `taint_never_falls_on_a_clean_rewrite` (row 13: the row and version 3's after-image both carry taint 1); `a_tainted_document_version_and_its_draft_carry_taint` (row 26). Red: no tables.
- `taint_tool_result_engine_values.rs` (owner Q4, the planted-string test): `a_write_result_holds_only_engine_values` (row 35: canaries in a data value, an unknown argument key, an update's patch and the arguments text are absent from the `EngineResult` text; only the row id, the version and one fixed word appear); `a_successful_write_leaves_the_attempt_clean` (row 35: taint 0 after the write, the next row clean); `an_error_result_stays_untrusted_and_taints` (row 36: stale version and bad data come back as `Part::ToolResult`, the attempt tainted, a later row tainted). Red: every result is `UntrustedData`.
- `taint_rows_reader.rs` (owner Q4 condition): `a_row_written_by_a_tainted_step_taints_a_later_reader` (row 38: edge taint 1, the reading attempt tainted, the row's mark unchanged); `a_clean_rows_edge_is_clean_and_its_text_still_fenced` (row 38); `a_clean_row_read_still_taints_the_reader` (row 38: the reading attempt's taint is 1 although the edge's is 0). Red: no `row_inputs`.
- `taint_documents_untrusted_markdown.rs`: `hostile_markdown_is_stored_as_is_and_nothing_is_fetched` (row 29, FakeEgress sees no request); `the_cli_prints_a_draft_neutralised` (rows 29, 30). Red: no versions.
- `inbox_resolve_human_only.rs` (existing, 3.4; tests added, none changed): `a_draft_cannot_be_dismissed_by_a_non_human` (row 39: through the store and through the API with the CLI token alone). Red: Draft refused for everyone.
- `rule_16_row_provenance.rs` (RULE-16: provenance in the audit): `every_row_change_has_an_after_image_naming_run_step_and_seat` (rows 2, 3, the verify); `audit_holds_ids_never_row_content` (row 22, canary in data and Markdown absent from the export); `the_chain_verifies_after_row_and_document_writes` (row 22); `no_new_sha256_arg_of_free_text` (row 22: every `*_sha256` arg of the run equals `audit::sha256_hex` of a stored text, as 3.5's test does). Red: no `row.written`.
- Not `rule_08_*`: story 4.9 writes the RULE-8 suite, and it must fail first there.

**am-app (`crates/am-app/tests/`):**
- `run_rows.rs` (new): `plan_of_maps_grants_team_table_document_and_the_row_tool` (row 34); `agentic_run_offers_table_write_only_to_a_declaring_step` (row 34, owner Q1); `a_step_declaring_another_teams_table_is_refused_before_any_row` (row 7). Red: no fixture, no plan fields.
- `gate_cli.rs` / `gate_tty.rs` (existing): `a_draft_is_listed_and_dismissed` and `a_draft_takes_no_approve` (row 30). Red: Draft is refused as "3.6".
- **Superseded:** `run_cli.rs` L1100-L1122 (words only, if ToolGate's words drop "not available").

**xtask (`xtask/tests/check_deps.rs`, planted files in `xtask/tests/fixtures`):**
- `only_toolgate_calls_the_row_writer` (row 33, owner Q1): planted forms each fail: a `.write_row(` call in another am-engine file; in am-tools; in am-app; split over lines; spaces around `.` and `::`; `RowWriter :: write_row` called by path; `use am_domain::rows::RowWriter as W`; `use am_domain::rows::*` outside am-domain.
- `the_store_row_write_is_named_only_by_the_engines_row_writer` (row 33): `apply_row_write` in am-app or am-server fails.
- `only_toolgate_builds_an_engine_result` (row 37, owner Q4): `EngineResult::row_written` outside `toolgate.rs`; an alias of `EngineResult`; a glob of `am_domain::content::*` outside am-domain; spacing and line breaks.
- `the_shipped_tree_passes_the_three_rules` (rows 33, 37).
- Red for each: no such rules.

**Fixture:** `fixtures/projects/rows-documents/`: `company.yaml` (two teams), two seats (Tables on for one), two tables (one per team, with `key`, typed and required columns and a `status` column), one workflow with a writing step (`table:`) and a document step (`document:`). Neutral names; the denylist scans `crates/`.

## Verification

- `cargo nextest run` per crate, filters first: am-tools `table_write`; am-store `rows`, `prompt_journal`, `inbox_resolve`; am-engine `toolgate`, `row_writer`, `loop_caps`, `run`; am-config `step_writes`; am-providers; am-e2e the new `engine` and `trust` targets and `inbox_resolve_human_only`; am-app `run_rows`, `run_cli`, `gate_cli`, `gate_tty`; xtask `check_deps`.
- `cargo test --doc -p am-domain -p am-store` (the `EngineResult` and resolve doctests).
- `cargo xtask check-deps`: the three new rules green on shipped code and red on each planted file; 3.4's resolve rule unchanged; `cargo xtask seam-guard` unchanged and green.
- `agentic validate` over `templates/` and `fixtures/`: green, the new fixture included.
- `just check` at the end (fmt, clippy `-D warnings`, nextest, doctests, check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard, validate).
- `just supply-chain` on the real lock: the am-tools → `serde_json` edge adds no package.
- No `cargo clean`, and no build of a workspace copy against the shared target (AGENTS.md, owner 2026-11-23).
- Merge only with both CI jobs green after review triage (owner, 2026-11-24).

## Implementation Notes
