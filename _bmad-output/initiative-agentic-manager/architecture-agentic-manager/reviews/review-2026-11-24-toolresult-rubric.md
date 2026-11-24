# Rubric review: AD-10 `TrustedToolResult`, AD-21 row-writer path, AD-15 widening kind (2026-11-24)

Reviewed: the uncommitted `git diff` of `architecture-agentic-manager.md` (AD-10 Binds, Prevents and Rule; AD-11 cross-reference; AD-15 widening list; AD-21 Binds and Rule; the new Open Questions "Resolved 2026-11-24 (owner, epic 3 story 6 plan …)" bullet) and the eight new `.memlog.md` entries. Checked against the owner's verbatim decision, the rest of the spine (AD-4, AD-8, AD-11, AD-15, AD-21, AD-22, AD-27, AD-29, AD-35, AD-36, AD-39, AD-41, AD-43, AD-46, Design Paradigm, Structural Seed, Open Questions of 2026-11-23), the brownfield code at 53ad57f (`crates/am-domain/src/llm.rs` `Part`, `crates/am-domain/src/tool_manifest.rs` `ToolRunner`, `crates/am-engine/src/run.rs` tool loop and `tool_call`, `crates/am-engine/src/toolgate.rs`, `crates/am-providers/src/{anthropic,openai_compat}.rs`, `crates/am-store/src/lib.rs` and `migrations/0001_initial.sql`, `xtask/src/deps.rs`, `xtask/src/seam_guard.rs`), and the approved story 3.6 plan (`tickets/epic-workflow-engine-cli/tickets/story-rows-and-documents-written-with-provenance-plan.md`, status approved, in the main checkout). Read-only; nothing else was edited; no cargo command was run (none was needed: the update adds no dependency).

## Verdict

**Not ready to merge as written.** The AD-10 `TrustedToolResult` core is sound. It records the owner's Q4 faithfully: ToolGate alone builds it, from engine values only, it never taints or clears taint, every error result stays `UntrustedData`, and a planted-string test proves it. Its placement as its own `Part` variant in the tool-result position, with the provider's call id beside it, ratifies how the brownfield adapters already render `Part::ToolResult`. Building it from "the writer's committed record of that call" matches the approved plan's `RowWritten`, read back from the journal by `(step_run_id, call_seq)`.

Two defects block it:

1. **AD-21 contradicts the owner's Q1 condition.** The new row-writer sentence names the wrong caller of the port: the spine says `am-tools`' `table_write` calls `RowWriter`, but the owner said only ToolGate does. It also implies `table_write` runs through the test-only `ToolRunner` seam, which story 3.3's owner condition 2 and `seam-guard` forbid in shipped code (C1).
2. **The restated AD-10 (c) leaves a taint-laundering gap.** It does not say what taint a row's history after-image carries, while updates merge patches and AD-22 reads after-images "as of `start_seq`". The approved plan's schema allows an after-image less tainted than its row (H1).

Fix C1 and H1 before story 3.6 starts, because the amendment exists to merge "first". The five mediums can go in the same edit. No new owner call is needed for any blocking fix. One future owner call is flagged (L3).

## Findings

### Critical

**C1. AD-21 "Row-writer path": the port's caller contradicts the owner's verbatim Q1 condition and the approved plan, and breaks the 3.3 seam.**

- Spine (AD-21 Rule): "`table_write` lives in `am-tools` and writes through one `RowWriter` port in `am-domain`, implemented by `am-engine` over the writer's row command and wired by `am-app`; the row command is called only by that implementation, **the port only by `am-tools`' `table_write`, and that tool is run only by ToolGate (AD-11)**, each enforced by call site in `cargo xtask check-deps`, so only ToolGate's path writes a seat's row." The memlog entry repeats this, adding "(AD-11; the existing ToolRunner seam)". The Resolved bullet adds: "For story 3.6's build: the `ToolRunner` doc in `crates/am-domain/src/tool_manifest.rs` … name[s] the one exception".
- Owner, verbatim: "Q1: a check-deps rule so **only ToolGate calls the row-writer**." The approved plan builds exactly that:
  - `RowWriter::write_row` is called only in `crates/am-engine/src/toolgate.rs`;
  - am-tools' `TableWrite` is a `RowTool` (spec, argument parsing, data checks) that "writes nothing and never names the port's method";
  - "`ToolRunner` stays test-only (seam-guard unchanged)".
- Brownfield: `xtask/src/seam_guard.rs` allows `ToolRunner` only in am-domain, `toolgate.rs`, am-testkit and `run_plan_with_tools`' signature. Its 3.3 test refuses "an impl in am-tools, an import in am-app" (story 3.3, owner condition 2). The spine's chain (am-tools' tool calls the port, and the tool is "run only by ToolGate" through the existing `ToolRunner` seam) cannot be built without loosening that owner condition.
- The chain also has no source for the write's facts. `ToolRunner::run(call: ToolCall, cancel)` carries no step run, `call_seq`, Actor, taint or declared table. Under the spine's wording, am-tools would have to supply the row's provenance and taint, or take them from the model's arguments. That is exactly the divergence AD-10 (c) and AD-24 must prevent.
- Consequence: the spine is binding, so a 3.6 builder must either follow it, against the owner's condition and the approved plan, or stop for a REPLAN. That defeats "Winston merges the narrow AD-10 amendment first".
- Fix: replace the AD-21 sentence with:

  > **Row-writer path** (amended 2026-11-24, owner, epic 3 story 6 plan Q1): `am-tools` holds `table_write`'s spec, argument parsing and data checks behind a port in `am-domain` that writes nothing; the write goes through one `RowWriter` port in `am-domain`, implemented by `am-engine` over the writer's row command. ToolGate is the only caller of the `RowWriter` write method: it takes the prepared row from `am-tools`, mints an insert's row id, and builds the write from engine facts only — the tool call's journal key and id, the attempt's `Actor::Seat`, the attempt's committed taint and the step's declared table — which the writer re-checks against the journal; nothing of the write's key, actor, taint or target comes from the tool's arguments beyond the one `table` argument ToolGate compares with the declaration. Only that implementation calls the row command. `cargo xtask check-deps` enforces both call sites (the method only in ToolGate's file, the row command only in `am-store` and the implementation's file), refusing aliases and glob imports. `table_write` is never a `ToolRunner`; the runner seam stays test-only (story 3.3, owner condition 2), and `am-app` only hands `am-tools`' tool to the run plan. A later Human or Import write path (CSV and xlsx imports, AD-39 Source loads) adds its own named call site in its own story.

  Also:
  - AD-21 Binds: add `am-app` (hands `am-tools`' tool to the plan) and `am-config` (the step's `table:` key and its team check).
  - AD-21 Prevents: add "a second path that writes a seat's row".
  - Resolved bullet: drop the `ToolRunner` doc item. That doc stays true under the owner's design, because a runner's result is always `UntrustedData`. Name instead `seam_guard.rs`'s doc line 29 ("so the binary never runs a tool"), which the plan rewords to "the binary runs only `table_write`, through ToolGate".
  - Correct the memlog entry to match.

### High

**H1. AD-10 (c) restated and AD-21: the taint of a row's history after-images is unstated. Patch-merged updates plus AD-22's as-of reads give a laundering path.**

- Spine (AD-10 (c), restated): "a row or document version written by a tainted step carries that taint at rest to every later reader, **by any path** (a table read or query, a rows or document input, a rows or document hand-off) … A row's taint only rises, so a later clean write to it never clears it."
- AD-21: "Every row mutation appends a `row_history` after-image with its Actor, reason and run, step and seat … in the same transaction". It gives no taint for the after-image.
- AD-22: "A run records `start_seq`, the `row_history` high-water mark at start, and reads granted tables as of `start_seq`."
- Problem: "only rises" is stated for the row (`table_rows.taint`), but readers by other paths read after-images. These include AD-22's as-of read (story 7.2), a `row` input edge that "names `r`'s after-image `seq`" (plan, row 38), and a rows hand-off payload (3.15).
- Updates merge a patch into the current data. Plan: "merge an update's patch into the current data (top-level keys)". So a clean update after a tainted one yields an after-image whose data still holds the tainted fields.
- The approved plan's schema permits exactly the unsafe reading:
  - `row_history.taint` is documented as "The writer's taint for this version";
  - the trigger `table_rows_needs_after_image` requires only `taint <= NEW.taint`.
- If 3.6 stores the writer's taint and 7.2 reads the after-image's taint, a tainted value reaches a reader marked clean. That violates AGENTS.md "Taint never launders". It is a cross-story divergence the spine must close now, because 3.6 creates the schema.
- Fix: add to AD-10 (c), after "never clears it":

  > Every `row_history` after-image carries the row's taint as it stands after that write — the highest taint of any write to the row so far, never only that writer's — and a read of any version (current, historical, or as of `start_seq`) returns at least that taint; a document version, which is whole and never a patch, carries its producing attempt's taint.

  Add one trust case to `taint_row_at_rest_*`: a tainted step writes v2, a clean step patches v3, and a read of v3 from history, and as of a `start_seq` after it, is tainted. The plan's column doc and trigger then need `taint = NEW.taint` (or equal to the running maximum) before code.

### Medium

**M1. AD-8: the local-write idempotency key is still "(run, step, call_seq)". The reading "(step_run_id, call_seq)" lives only in the memlog.**

- Spine (AD-8): "Local-write tool calls are idempotent by (run, step, call_seq)." The memlog's "Checks with no change": "AD-8's local-write idempotency key (run, step, call_seq) is the step run's (step_run_id, call_seq)." The ticket says "idempotent by run, step and call sequence".
- Brownfield: each attempt is its own `step_runs` row, and `tool_calls` is `UNIQUE (step_run_id, call_seq)` with `call_seq` restarting at 1 per attempt (`run.rs` `record`, `0001_initial.sql`). Read literally, "(run, step key, call_seq)" makes a loop-back or retried attempt's first `table_write` a silent "replay" of the previous attempt's. The plan names that trap itself (matrix row 16).
- The new AD-10 text builds the trusted result "from the writer's committed record of that call". That sentence also depends on this key, and so do story 3.9's resume and every later local-write tool (`write_document`, `workspace_write`, `remember`).
- Fix: in AD-8, "Local-write tool calls are idempotent by the step run (the attempt) and the tool call's journaled `call_seq`, `(step_run_id, call_seq)` (clarified 2026-11-24, epic 3 story 6); a loop-back or retried attempt is a new step run, and a replayed call's result is rebuilt from the writer's record under that key."

**M2. AD-10 Rule still says prompt content is "one of three types".**

- Spine: "Prompt content is one of three types (amended 2026-11-23 …): `Instruction` …, `TrustedInput` … or `UntrustedData` …". Later the same Rule adds a `TrustedToolResult` that "is its own `Part` variant", and the "no conversion" sentence covers only the three.
- The approved plan says "Module docs name the fourth kind" (`content.rs`). The spine says three, so a builder could model the result as a `TrustedInput` labelled `tool:table_write`. That would break `TrustedInput`'s closed list of two sources and its call-site rule.
- Fix: "Prompt content is one of three types … ; a tool's successful local-write result is a fourth, `TrustedToolResult` (below), and nothing else." Extend the conversion sentence: "There is no conversion between `TrustedToolResult` and any of the other three."

**M3. The TrustedToolResult rule should make eligibility structural, pin the constructor's call site to ToolGate's file, and stay inside the owner's three values.**

- Spine: "The successful result of a local-write built-in tool (in v1 only `table_write`) is a `TrustedToolResult` …, which ToolGate builds itself solely from engine values: ids the engine minted (the row id), version numbers, **counts** and fixed status words written in code …; its one constructor is called only by ToolGate in `am-engine`, which `cargo xtask check-deps` enforces by call site."
- Problems:
  - (a) Nothing says how eligibility is decided. A builder could add a manifest or config flag ("trusted result") that a later tool, or an MCP tool from `tools.yaml`, could set. The spine should say eligibility is the one write path in ToolGate, not a field. (A same-named MCP tool is already harmless, because the writer holds no record for it, but that relies on C1's wording.)
  - (b) The owner approved "(row id, version, a fixed status word)" (plan Q4). "Counts" and the plural forms widen what the type may carry beyond the narrow amendment.
  - (c) The existing `TrustedInput` rule is crate-scoped (`TRUSTED_CALLER = "crates/am-engine/src/"` in `xtask/src/deps.rs`). "Only by ToolGate in am-engine" could be satisfied by a crate-wide rule. The plan scopes it to `toolgate.rs`.
  - (d) The constructor should take only typed values, so the canary test checks a structural guarantee rather than being the only one.
- Fix: "… solely from the write's committed record: the row id (an engine-minted UUIDv7), the version and one status word from a closed set in code; its one constructor takes only those typed values (no string parameter) and is called only in ToolGate's file, `crates/am-engine/src/toolgate.rs`, which `cargo xtask check-deps` enforces by call site, refusing aliases and glob imports. Eligibility is that one write path in ToolGate, never a manifest, config or tool field; making any other tool's result trusted is an AD-10 amendment."

**M4. AD-29 still says "versioned per run". The owner's Q7 deviation is recorded only as a quotation.**

- Spine (AD-29): "Documents and Assistant answers are stored as Markdown and versioned per run." The Resolved bullet quotes "Q7: record as my approved deviation from 'one version per run'" but says nothing about AD-29.
- The memlog reads AD-29 as "every version naming its run, the run's version being its latest", and the plan builds "the run's version is its latest" (row 25). Later readers of AD-29 need "the run's version" and will hit the conflict: 3.15's document hand-off (`handoff:…@<version>`, AD-27, AD-41), 6.9's rendering and AD-43's lineage.
- Fix: append to the AD-29 sentence: "(a run may hold several versions, one per succeeded attempt of its document step; the run's version is its latest; amended 2026-11-24, owner, epic 3 story 6 plan Q7, the owner's approved deviation from the ticket's 'one version per run')." Alternatively, put that reading in the Resolved bullet.

**M5. Names differ between the binding spine and the approved plan.**

- Spine: `TrustedToolResult`, `crates/am-e2e/tests/trust/taint_trusted_tool_result_*.rs`, `taint_row_at_rest_*.rs`.
- Plan: `EngineResult`, `Part::EngineResult`, `EngineResult::row_written`, `taint_tool_result_engine_values.rs`, `taint_rows_at_rest.rs` and `taint_rows_reader.rs`.
- The check-deps rule matches tokens, so the rule, the `Part` variant, the journal kind and review greps must agree. The release gate is unaffected, because it runs `taint_*` by directory.
- Fix: add to the Resolved bullet: "the plan's `EngineResult` is built as `TrustedToolResult` (`Part::TrustedToolResult`), and its trust-test files take the spine's `taint_trusted_tool_result_` and `taint_row_at_rest_` prefixes". Alternatively, adopt the plan's names in AD-10.

### Low

**L1. AD-15: the "removing" half is attributed to the owner, but neither the owner's words nor the plan contain it.**

- Spine: "adding a step's declared table or document (`table:`, `document:`, epic 3 story 6) or changing either, while removing one is not widening (owner, 2026-11-24, epic 3 story 6 plan Q3; …)". The owner said "add 'changing table: or document:' to 4.17 as widening". The plan's Q3 and its Boundaries say "adding or changing" and never "removing".
- The reading is sound, because removing a write target narrows privilege. It is the architect's reading, though.
- Fix: "… or changing either (owner, 2026-11-24, epic 3 story 6 plan Q3); removing one is not widening (architect's reading: it narrows what the step may write); until epic 4 story 17 …".

**L2. The error-result sentence reads as if every failure is handed to the model.**

- Spine: "any refusal or failure (… a store error, or a call the writer holds no committed record of) is `UntrustedData` and taints as every other tool result does." The plan maps a store `Failed` to "stops the step as a store error", not to a tool result. Both are safe.
- Fix: "… is never a `TrustedToolResult`; when the model is told of it, it is `UntrustedData` and taints as every other tool result does."

**L3. Cross-reference the open questions of 2026-11-23, and flag a future owner call.**

- "Every tool result is `UntrustedData` except one kind" now settles, by default, Open Questions 2026-11-23 (1) and (2). Those are AD-38's `read_skill_file` result "as a trusted input" and a clean `ask_seat` answer, both still open for the owner. Add "(Open Questions 2026-11-23 (1) and (2) stay open; until decided, those results are `UntrustedData`)".
- **Future owner call, not blocking:** in v1 every other local-write tool's success acknowledgement still taints. That covers `write_document`, `workspace_write`, `write_file`, `draft_message`, `remember`, `propose_change` and `ask_seat`. One consequence: a step's second `remember` or `propose_change` is tainted-origin and never approvable (AD-10 (b)), because the first call's acknowledgement tainted the step. Raise this with the owner before epic 4 or 9 builds those tools. The options are to extend the `TrustedToolResult` (an AD-10 amendment) or to accept the behaviour.

**L4. Journal and drill-down form are unstated.**

- AD-27 says "Each part carries a source label stored in the journal and shown in drill-down". The new part has no stated label or journal kind. "No `Deserialize`" implies resume rebuilds the part from the writer's record, not from journal text, but that is said only in the memlog ("makes 3.9's resume rebuild the same result from the journal").
- Fix: one clause in AD-10: "journaled as its own part kind labelled `tool:<tool id>`, shown as such in drill-down, and rebuilt on resume from the writer's record under the call's key, never parsed back from the journal".

## Checklist walk

| Check | Result |
|---|---|
| Fixes the real divergence points one level down, misses none | Partly. Fixed: who builds the result, from what, whether it taints, whether errors stay untrusted, and rendering placement. Missed or wrong: the port's caller and the source of the write's facts (C1), after-image taint (H1), the idempotency key (M1), eligibility (M3), and "the run's version" (M4). |
| Each changed AD's Rule is enforceable and prevents its divergence | AD-10 TrustedToolResult: yes, with M3's file-scoped call site and typed constructor. AD-10 (c) restated: not for history readers (H1). AD-21: enforceable, but enforces the wrong caller (C1). AD-15: yes, as an interim risk the owner accepted. AD-11: cross-reference only. |
| Nothing left unsaid could let two units diverge | Not met: C1, H1, M1, M4 and M5. |
| Ratifies the brownfield code | `Part` variant and adapter placement: yes, it matches how `anthropic.rs` and `openai_compat.rs` render `Part::ToolResult` by name. `run.rs` "Tool results are untrusted": named for change. `deps.rs`: the new rule should copy 3.4 and 3.5's whole-file, alias-refusing form (M3). `ToolRunner` and `seam_guard.rs`: contradicted (C1). |
| Consistent with the rest of the spine | AD-4 and the Design Paradigm hold: ports in am-domain, am-tools depends only on am-domain, am-engine implements. AD-46: the Tables grant path is unchanged. AD-22: "a grant never enables `table_write`" holds. AD-35, AD-36 and AD-43 hold. AD-8 (M1), AD-27 (L4) and AD-29 (M4) need the noted words. AD-38 and the open questions: L3. |
| Stays narrow; weakens no other AD | Narrow, apart from "counts" (M3) and the "removing" attribution (L1). Nothing is weakened: a tainted step stays tainted and bound by (b), the result never clears taint, error results taint, and RULE-8 and the injection-pack gate are unchanged. |
| Needs a new owner call | None for C1, H1 or M1–M5: each is recording the owner's decision correctly. Future: L3, the other local-write tools' acknowledgements. |
