---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-24 AD-10 TrustedToolResult amendment for epic 3 story 6 (uncommitted diff on 53ad57f). Changed: AD-10 Binds/Prevents/Rule (lines 190-192: Trusted tool result, (c) restated), AD-11 cross-reference (line 198), AD-15 widening kind (line 222), AD-21 Binds and Row-writer path (lines 256, 258), Open Questions Resolved entry (line 818), and the last 8 memlog entries
date: 2026-11-24
---

# Review: trusted tool results and the row-writer path, checked against the code

## Verdict

**Pass with changes: no critical, 1 high, 2 medium, 5 low.**

The update names no new library, so this review checks it against the code (brownfield, 53ad57f), not the web. Every type, crate, port, check and test path it names either exists or fits as a new addition. AD-4 holds: no new dependency edge is needed. One part does not match: the AD-21 call chain. It contradicts the owner's verbatim Q1 condition and the approved 3.6 plan, and the build list that follows from it runs into `cargo xtask seam-guard`. The names also differ from the approved plan's, so the builder has to rename.

These claims check out against the code:

- **Tool results taint today.** Every tool result is `Part::ToolResult { call_id, content: UntrustedData }` (`crates/am-domain/src/llm.rs` L47-L50), and the loop taints the attempt after any result (`crates/am-engine/src/run.rs` L1458-L1460). So the memlog's background is accurate.
- **A new `Part` variant fits.** The call id already sits beside the content as a sibling field, never inside it. Every adapter already matches each variant by name (`crates/am-providers/src/anthropic.rs` L90-L97, `openai_compat.rs` L153, L176-L179). `render_trusted_input` (`crates/am-providers/src/common.rs` L33-L48) is the precedent for "one shared rendering function".
- **Call-site rules exist in `check-deps`.** `xtask/src/deps.rs` L6-L44 and `call_site_findings` (L120 onward) already enforce `TrustedInput`'s constructors, `Instruction::authored` and the Inbox resolve. "Enforced by call site in `cargo xtask check-deps`" names a real mechanism.
- **Dependency edges (`cargo metadata --no-deps`):**
  - `am-tools` depends only on `am-domain`.
  - `am-engine` depends on `am-domain` and `am-store`.
  - `am-providers` depends on `am-domain` and `am-net`.
  - `am-app` depends on all of them, including `am-tools` and `am-engine`.

  So a port in `am-domain`, implemented in `am-engine` and wired by `am-app`, needs no new edge, whichever way it is called. The matrix in `deps.rs` L384-L447 permits every edge involved.
- **`table_write` and the Tables capability already exist.** `table_write` is a built-in `LocalWrite` tool with no network and 100 uses (`crates/am-domain/src/tool_manifest.rs` L99). Tables expands to `table_read` and `table_write` (`crates/am-domain/src/capability.rs` L97-L102). The memlog's "AD-46 needs no change" holds.
- **The widening classifier does not exist yet.** `Step` (`crates/am-config/src/entities.rs` L319-L354) has no `table` or `document` field today, and `is_widening` does not exist anywhere in `crates/`. So the AD-15 interim-risk sentence ("until epic 4 story 17 ships the classifier") is accurate.
- **The journal has the key AD-8 needs.** `tool_calls (id, step_run_id, call_seq, tool, status)` exists (`crates/am-store/src/lib.rs` L1354). So AD-8's `(run, step, call_seq)` key maps to `(step_run_id, call_seq)`, as the memlog says. `table_rows`, `row_history` and `document_versions` do not exist yet (migrations 0001-0009): they are 3.6's new additions.
- **Trust-test names fit.** AGENTS.md L54 allows `taint_*.rs`, which covers both new globs. The existing `crates/am-e2e/tests/trust/taint_at_rest.rs` (3.3, step runs and Inbox items) does not clash with `taint_row_at_rest_*.rs`.
- **The three docs the Resolved entry names exist:**
  - the `ToolRunner` doc, `tool_manifest.rs` L16-L18 ("A result is always `UntrustedData`");
  - the `Part::ToolResult` doc, `llm.rs` L47;
  - the run-loop comment, `run.rs` L1458.

  See M1 for which of them should change.
- **The memlog's AD-5 reading holds.** AD-5's port list is already incomplete in the code: `AuditSink`, `ConfigHistory`, `ToolRunner`, `CancelSignal` and `EventSink` are am-domain ports it does not list. Structural Seed L551 and L558 place port traits in `am-domain` and `table` in `am-tools`.

## Method

- **Diff:** `git -C W diff`, plus `git diff --word-diff=plain` of the spine. Read the full changed lines 190-192, 198, 222, 256-258 and 818, and the memlog tail.
- **Code read at W (53ad57f):**
  - `am-domain`: `llm.rs`, `tool_manifest.rs`, `content.rs`, `capability.rs`.
  - `am-engine`: `toolgate.rs`, `run.rs` (module docs, tool loop, `tool_call`), `parts.rs`.
  - `am-providers`: `common.rs`, `anthropic.rs`, `openai_compat.rs`.
  - `am-tools`: `Cargo.toml`, `src/lib.rs`.
  - `xtask`: `src/deps.rs`, `src/seam_guard.rs`.
  - `am-store`: migrations 0007 and 0009, `lib.rs` tool-call journaling.
  - `am-config`: `src/entities.rs`.
  - `crates/am-e2e/tests/trust/`, and AGENTS.md L54.
- **Port implementers and callers:** grepped every implementation and every holder of `AuditSink`, `ConfigHistory` and `ToolRunner`.
- **Dependency edges:** `cargo metadata --no-deps --format-version 1 --offline` (no build).
- **The approved 3.6 plan:** `/Users/hiteshmamgain/Developer/agentic-manager/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-rows-and-documents-written-with-provenance-plan.md`. It is untracked in the main checkout and not in W. The spine's Resolved entry quotes its owner approval. It is the existing project's statement of how 3.6 will be built, so the amendment has to fit it.
- **Not done:** no web check (no library, version or starter is named), no build, and no file edited but this review.

## Findings

### H1 (High): AD-21's row-writer call chain contradicts the owner's Q1 condition and the approved plan

**Where:** AD-21 Rule, "Row-writer path" (line 258): "`table_write` lives in `am-tools` and writes through one `RowWriter` port … the port only by `am-tools`' `table_write`, and that tool is run only by ToolGate (AD-11), each enforced by call site in `cargo xtask check-deps`". The memlog entry "AD-21 row-writer path" says the same.

**Evidence:**

- **The owner's condition contradicts it.** The spine's own Resolved entry (line 818) quotes the owner: "Q1: a check-deps rule so only ToolGate calls the row-writer." AD-21 puts the port's only call in `am-tools` instead.
- **The approved plan builds the opposite chain:**
  - Plan L44: "only ToolGate calls the port's method".
  - Plan L56: "`RowWriter::write_row` is called only in `crates/am-engine/src/toolgate.rs`".
  - Plan L270-L274: am-tools' `TableWrite` is a `RowTool` that "writes nothing and never names the port's method", and ToolGate holds both pieces.
  - Plan L287: ToolGate calls `RowTool::prepare`, mints the row id, then calls `RowWriter::write_row` itself.
  - Plan L583: the planted-file test `only_toolgate_calls_the_row_writer` fails on a `.write_row(` call "in am-tools". The spine's AD-21 would make that test fail on the design itself.
- **No `check-deps` rule enforces "that tool is run only by ToolGate", and none is planned.** Plan L267 says "No `check-deps` rule names tools today". In the code, "only ToolGate runs a tool" rests on two things:
  - the `Permit` type, which only `ToolGate::admit` makes and only `ToolGate::run` consumes (`crates/am-engine/src/toolgate.rs` L67-L74);
  - `cargo xtask seam-guard` (`xtask/src/seam_guard.rs` L80-L97).
- **AD-10 gets harder under AD-21's chain.** AD-10 says the values are "taken as typed values from the writer's committed record of that call". If `am-tools` calls the port, ToolGate never receives the writer's answer, so it would need a store reader it does not hold today. In the plan's chain, `StoreRowWriter` reads the record back (`Reader::row_write(step_run_id, call_seq)`, plan L157 and L367) and hands ToolGate a typed `RowWritten`.

**Fix (wording for AD-21's Row-writer path):**

"`table_write`'s spec, argument parsing and data checks live in `am-tools`, behind a `RowTool` port in `am-domain`; it writes nothing and never calls `RowWriter`. The write goes through one `RowWriter` port in `am-domain`. `am-engine` implements it over `am-store`'s row write, and ToolGate (`crates/am-engine/src/toolgate.rs`) is its only caller. `am-app` wires `am-tools`' `RowTool` into the run plan, because `am-engine` may not depend on `am-tools` (AD-4). `cargo xtask check-deps` enforces two call sites: the port's method is called only in ToolGate, and `am-store`'s row write is named only in `am-store` and in `am-engine`'s implementation. So only ToolGate's path writes a seat's row. A later Human or Import write path adds its own named call site in its own story."

Also add `am-app` (wiring) to AD-21's Binds, and reword the memlog entry to match.

### M1 (Medium): the build list implies `table_write` runs through `ToolRunner`, which seam-guard forbids in shipped code

**Where:** Resolved entry, line 818: "For story 3.6's build: the `ToolRunner` doc in `crates/am-domain/src/tool_manifest.rs` … name[s] the one exception". The memlog's "(AD-11; the existing ToolRunner seam)" says the same.

**Evidence:**

- **Seam-guard keeps `ToolRunner` out of shipped code.** It allows the name only in `crates/am-domain/src/`, `crates/am-engine/src/toolgate.rs`, `crates/am-testkit/src/` and `run_plan_with_tools`' signature. `run_plan_with_tools` itself may not be named anywhere else in shipped code, `agentic run` included (`xtask/src/seam_guard.rs` L80-L97, "story 3.3, owner condition 2"). The only runner is am-testkit's `FixtureTool` (`crates/am-testkit/src/lib.rs` L569). So putting `table_write` behind `ToolRunner` would break an owner condition and a CI guard.
- **The plan keeps `ToolRunner` test-only:**
  - Plan L81: "`ToolRunner` stays test-only (seam-guard unchanged)". Plan L464 repeats it.
  - Plan L150: "`ToolRunner` L19-L28: unchanged".
  - Plan L203: only seam-guard's doc lines L27-L31 are reworded.

  Under the plan, `ToolRunner`'s doc ("A result is always `UntrustedData`") stays true and should not change.
- **The list misses two docs that do go stale:**
  - `crates/am-domain/src/content.rs` L11 lists "tool results" under `UntrustedData`.
  - `crates/am-engine/src/toolgate.rs` L11-L14 says "every class but `Read` is refused".

**Fix:**

- In the Resolved entry's build list, replace "the `ToolRunner` doc in `crates/am-domain/src/tool_manifest.rs`" with "the module docs of `crates/am-domain/src/content.rs` (L11) and `crates/am-engine/src/toolgate.rs` (L11-L14), and `xtask/src/seam_guard.rs`'s doc lines L27-L31".
- Add one clause to AD-11 or AD-21: "`ToolRunner` stays the test-only seam (seam-guard unchanged); `table_write` reaches ToolGate as a `RowTool`, not a `ToolRunner`."

### M2 (Medium): the spine's names differ from the approved plan's, and no line tells the builder which wins

**Where:** AD-10 Rule (line 192) names the type `TrustedToolResult` and the trust tests `crates/am-e2e/tests/trust/taint_trusted_tool_result_*.rs` and `taint_row_at_rest_*.rs`.

**Evidence:** the plan names the type `EngineResult` (L68, L147), the variant `Part::EngineResult { call_id, result }` (L148) and the constructor `EngineResult::row_written` (L201, L294). Its trust tests are:

- `taint_tool_result_engine_values.rs`
- `taint_rows_at_rest.rs`
- `taint_rows_reader.rs`
- `taint_rows_declared_table.rs`

(plan L568-L571). Its xtask test is `only_toolgate_builds_an_engine_result` (L585).

None of these files matches the spine's globs: "rows" is not "row", and `taint_tool_result_*` is not `taint_trusted_tool_result_*`. A builder who follows the approved plan word for word breaks the binding AD-10 names. Nothing in the update says the plan's names give way.

**Fix:** add to the Resolved entry's build list:

"the 3.6 plan's `EngineResult`, `Part::EngineResult` and `EngineResult::row_written` are built as `TrustedToolResult`, `Part::TrustedToolResult` and its one constructor; its trust tests are named `taint_trusted_tool_result_engine_values.rs`, `taint_row_at_rest_rows.rs` and `taint_row_at_rest_reader.rs`."

Alternatively, adopt the plan's names in AD-10. Either way, one name set, written in both documents.

### L1 (Low): AD-10 treats a store error as an error result the model reads; the plan stops the step instead

**Where:** AD-10, line 192: "any refusal or failure (… a store error, …) is `UntrustedData` and taints as every other tool result does".

**Evidence:**

- Plan L287: "`Failed` stops the step as a store error". Only `Refused` words and `RowTool::prepare` errors become untrusted error results.
- Today a `StoreError` in `tool_call` propagates with `?` and ends the run (`crates/am-engine/src/run.rs` L1518 onward).

Both designs keep a store error out of a `TrustedToolResult`, so the safety property holds. But the spine's sentence would have the builder hand store-error text to the model.

**Fix:** "… a stale version, an unknown row or data the table's schema refuses is `UntrustedData` and taints; a store error stops the step (AD-8); none is ever a `TrustedToolResult`."

### L2 (Low): the rendering sentence is garbled

**Where:** AD-10, line 192: "rendered outside the untrusted fence through one shared `am-providers` function that every adapter matches by name".

**Evidence:** adapters match `Part` variants by name and then call a shared function, as with `Part::Trusted` → `render_trusted_input` (`anthropic.rs` L52-L53, `openai_compat.rs` L117-L118). Nothing matches a function by name. The tool-result position also differs by adapter:

- Anthropic: a `tool_result` block in a user message (`anthropic.rs` L76).
- OpenAI-compatible: a `role: "tool"` message (`openai_compat.rs` L150).

**Fix:** "every adapter matches `Part::TrustedToolResult` by name and renders it in its tool-result block (Anthropic `tool_result`, OpenAI-compatible `tool` message), outside the untrusted fence, through one shared `am-providers` function".

### L3 (Low): AD-10 does not say which file may call the constructor

**Where:** AD-10, line 192: "its one constructor is called only by ToolGate in `am-engine`, which `cargo xtask check-deps` enforces by call site".

**Evidence:** `check-deps`' `TrustedInput` rule allows a whole crate (`TRUSTED_CALLER = "crates/am-engine/src/"`, `deps.rs` L58-L59). Its `INSTRUCTION_SITES` rule allows named files (L69-L73). "ToolGate" is a type, not a path. The plan pins `crates/am-engine/src/toolgate.rs` (L201, L294).

**Fix:** "… called only in `crates/am-engine/src/toolgate.rs`, which `cargo xtask check-deps` enforces by call site". This also gives `xtask` an exact path to check.

### L4 (Low): the memlog's port precedents do not all run in the claimed direction

**Where:** memlog, "AD-21 row-writer path": "brownfield precedent AuditSink, ConfigHistory, ToolRunner". The direction being defended is a port that the engine implements and an adapter calls.

**Evidence:**

- **`AuditSink` fits.** `am-tools`' `FolderAccess` holds `Arc<dyn AuditSink>` (`crates/am-tools/src/lib.rs` L157-L167). `am-app` implements the port (`crates/am-app/src/egress.rs` L190), and so does `am-engine` (`Refusals`, `crates/am-engine/src/run.rs` L645).
- **`ToolRunner` runs the other way.** `am-testkit` implements it (L569) and `am-engine`'s ToolGate calls it.
- **`ConfigHistory` is record-to-record.** `am-store` implements it (`crates/am-store/src/config_history.rs` L483) and `am-config` calls it.

All three do support "`am-domain` holds port traits". With H1's fix the direction question goes away: `am-engine` both implements and calls `RowWriter`, and `RowTool` runs in `ToolRunner`'s direction.

**Fix:** reword the memlog entry as "precedent: am-domain holds port traits (AuditSink, ConfigHistory, ToolRunner); an adapter already calls a port implemented above it (am-tools → AuditSink)".

### L5 (Low): "recorded by her REPLAN" is not yet true in W

**Where:** Resolved entry, line 818: "For Sofia (recorded by her REPLAN of 2026-11-24): epic 4 story 17 carries the new widening kind with a verify case; epic 3's Notes carry the owner's decision."

**Evidence:** neither change exists under `W/_bmad-output/initiative-agentic-manager/tickets/` or in the main checkout. A grep for the widening kind, `TrustedToolResult` and "engine values" finds nothing. The sentence is true only if Sofia's REPLAN lands in the same PR, as the last replan did in 53ad57f.

**Fix:** merge the spine and Sofia's REPLAN together. Otherwise write "to be recorded by Sofia's REPLAN".

## Checked and needing no change

- **AD-15 wording.** The widening kind and its interim risk follow the 3.3 gate precedent (same line 222). `Step` gains `table` and `document` in 3.6 (plan L192), and 4.17 classifies them.
- **AD-10 (c) restatement.** Rows and document versions carry taint at rest. A row's taint only rises: the plan adds `table_rows_taint_only_rises` (plan L362), like the `*_taint_only_rises` triggers in migrations 0007 and 0008. A reader is tainted.
- **AD-10 and AD-21 Binds.** Both list existing crates. Apart from the `am-app` gap in H1, nothing is listed that the design does not touch.
- **The new `Part` variant and the journal.** Tool results are not journaled as `prompt_parts` today: the `prompt_parts.kind` CHECK in migration 0009 has no tool-result kind. So the new variant needs no change to the 0009 CHECK.
