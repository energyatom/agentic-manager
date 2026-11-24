# Adversarial review: trusted tool results, row taint at rest and the row-writer path (2026-11-24)

- **Reviewed:** the uncommitted diff on 53ad57f in the `replan-ad10-toolgate` worktree: AD-10 Binds, Prevents and Rule (line 190-192: *Trusted tool result*, (c) restated), the AD-11 cross-reference (line 198), the AD-15 widening kind (line 222), AD-21 Binds and *Row-writer path* (lines 256, 258), the Open Questions *Resolved* entry (line 818), and the last 8 `.memlog.md` entries.
- **Read against:** AD-8, AD-10, AD-11, AD-15, AD-21, AD-22, AD-26, AD-27, AD-29, AD-34, AD-36, AD-41, AD-42 and AD-43 as they now stand. Brownfield code: `crates/am-domain/src/{llm,content,tool_manifest}.rs`, `crates/am-engine/src/{run,toolgate}.rs`, `xtask/src/{deps,seam_guard}.rs`. Also read: the approved 3.6 plan (`story-rows-and-documents-written-with-provenance-plan.md`, untracked in the main checkout) and epic 3's and epic 4's `tickets.toml` entries 3.6, 3.13, 3.15, 3.16 and 4.9.
- **Scope:** how the owner's decision is recorded. The owner's Q1–Q9 answers and conditions are taken as final. The findings attack the wording and where it meets existing text, not the decisions.
- **Lens:** (1) two units one level down, each obeying every AD to the letter, that still build incompatibly; (2) a security adversary who obeys the changed ADs to the letter and still gets untrusted text to the model as trusted, or still launders taint.
- **Verdict:** **amend before the 3.6 build starts.** 0 critical, 4 high, 7 medium, 3 low.
  - The `TrustedToolResult` itself is sound as a type. Its type wall (private fields, no `Deserialize`, one constructor, its own `Part` variant, error results excluded) closes the direct paths.
  - H1 must be fixed before any 3.6 code. The AD-21 call chain contradicts the owner's Q1 words and the approved plan, and as written it cannot be built for the shipped binary.
  - H2–H4 are laundering routes. The restated (c) promises they are closed, but the ADs that the read paths follow (AD-10's taint mechanics, AD-41, AD-42) were not changed to match. They first become reachable in 3.15, 3.16, 7.x and 12.x, so they can go into the same amendment.
  - M1 and M2 are wording fixes the 3.6 build depends on.
  - One finding needs a new owner call: L1. M5 and M6 each have a part that needs one.

| # | Tier | Title | Where |
|---|---|---|---|
| H1 | High | AD-21's call chain misrecords Q1, and no channel carries the per-call context | AD-21 *Row-writer path*; seam-guard |
| H2 | High | Reader taint can follow the at-rest mark instead of the part, so a clean-marked row with injected text leaves its reader clean | AD-10 (c) restated; AD-43 edges; `run.rs` `place` |
| H3 | High | An after-image merges earlier tainted fields under a clean writer's mark; reads by `seq` launder | AD-10 (c); AD-21; AD-22/41/43 `seq` reads |
| H4 | High | A rows or document hand-off takes only the sender's taint (AD-41), and the AI reviewer fences "if tainted" (AD-42) | AD-41; AD-42; (c) restated |
| M1 | Medium | "Counts" and "version numbers" are not tied to the call itself, which opens a covert channel from other writers; row ids are not engine-minted for every actor | AD-10 *Trusted tool result*; AD-21 |
| M2 | Medium | AD-8's idempotency key `(run, step, call_seq)` was not amended; a replay can return another attempt's or branch's write as a trusted success | AD-8 |
| M3 | Medium | Nothing says how a `TrustedToolResult` is journaled or rebuilt on resume or in drill-down | AD-10; AD-8; AD-27 |
| M4 | Medium | Eligibility can be keyed on the name `table_write`, and MCP tool names are not namespaced | AD-10; AD-11; AD-26 |
| M5 | Medium | (c)'s "every later reader, by any path" leaves out the Assistant's `v_*` reads; views need not carry `taint` | AD-10 (c); AD-21; AD-34; AD-15 |
| M6 | Medium | Two ways to create a document version: the final message (Q5) and `write_document` (AD-10 (b), AD-11, golden 3.13) | AD-10 (b); AD-11; AD-29 |
| M7 | Medium | Two writers of a `table_write` call's final status: the row command's transaction and the run loop's finish or deadline | AD-8; AD-10 "committed record" |
| L1 | Low | Changing a table's or document's owning team is not a widening kind, though Q3 makes ownership the effective grant | AD-15 |
| L2 | Low | The interim-risk sentence says "a hand edit"; the gap covers every change that lands before 4.17 | AD-15 |
| L3 | Low | Enforcement precision: a crate-level vs file-level call-site rule, a unique method token, and the plan's `EngineResult` vs the spine's `TrustedToolResult` | AD-10 *Enforcement*; AD-21 |

---

## H1. High: AD-21's call chain misrecords the owner's Q1, and no channel carries the per-call context

**Spine text (AD-21, line 258):** "`table_write` lives in `am-tools` and writes through one `RowWriter` port in `am-domain`, implemented by `am-engine` over the writer's row command and wired by `am-app`; the row command is called only by that implementation, the port only by `am-tools`' `table_write`, and that tool is run only by ToolGate (AD-11)". The memlog adds "the existing ToolRunner seam".

**Owner, verbatim (Q1):** "a check-deps rule so only ToolGate calls the row-writer."

**Two units, each obeying the letter.**
- *Unit A, `am-tools`' `table_write`, built to AD-21.* It is a `ToolRunner` tool that calls `RowWriter` itself. That is the only way "the port only by `am-tools`' `table_write`" can hold. Its only input is `ToolRunner::run(call: ToolCall, cancel)` (`tool_manifest.rs` L19-L28): the model's `id`, `name` and `arguments_json`. So the `RowWrite` it sends must take the table id, the Actor, the step run, the idempotency key and the taint from the model's arguments, or default them.
- *Unit B, the approved 3.6 plan and its check-deps rule.* "`RowWriter::write_row` is called only in `crates/am-engine/src/toolgate.rs`" (plan L56, L273-L275, L287). am-tools' `TableWrite` is a `RowTool` that "writes nothing and never names the port's method". am-engine builds its own `StoreRowWriter`. am-app wires the `RowTool`, not the `RowWriter`.

**Clash.**
- Under Unit B's check-deps rule, Unit A's code fails, and the reverse also holds.
- Unit A also cannot ship. `ToolRunner` is test-only by `cargo xtask seam-guard` (`seam_guard.rs` L80-L97: `run_plan_with_tools` is named nowhere in shipped code, and the binary runs no tool). So "that tool is run only by ToolGate … the existing ToolRunner seam" describes a path the shipped binary does not have.
- The plan's second port (`RowTool`) is not in the spine at all.

**Attack.** Suppose a builder follows the spine and passes the context through the only channel it has. One way is ToolGate merging `"_ctx": {...}` into `arguments_json`; another is am-tools defaulting `taint: Clean` because it cannot know the step. Then a tainted step can write `{"table":"other","_taint":"clean",…}`, or simply be stamped clean. Its row is laundered at rest, and AD-10 (b)'s declared-table binding is decided by model text.

**Fix (no new owner call; it restores Q1's literal words and records the approved plan).** Replace AD-21's *Row-writer path* with:

> **Row-writer path** (amended 2026-11-24, owner, epic 3 story 6 plan Q1: "only ToolGate calls the row-writer"):
> - `table_write`'s argument checking lives in `am-tools` behind a `RowTool` port in `am-domain`. It turns a call's arguments into a checked row and writes nothing.
> - The one `RowWriter` port in `am-domain` is implemented by `am-engine` over the writer's row command.
> - ToolGate is the only caller of the `RowWriter` port. It builds each `RowWrite` itself from the Permit and the journal: the Actor, the run, the step run, the tool call's journaled id and `call_seq`, the step's declared table, and the attempt's committed taint. It never takes them from the call's arguments. The arguments supply only the checked row data, the operation, and an update's row id and expected version.
> - The writer's row command is called only by the engine's implementation.
> - `cargo xtask check-deps` enforces each rule by call site: the port's method only in `crates/am-engine/src/toolgate.rs`, the row command only in `am-store` and the engine's implementation.
> - `table_write` is not a `ToolRunner` tool, so the test-only runner seam is unchanged.
> - A later Human or Import write path adds its own named call site in its own story.

Then:
- AD-21 *Binds* gains `am-domain` (`RowTool`, `RowWriter`) and `am-app` (wires the `RowTool` into the plan).
- AD-10 *Binds* `am-tools` stays.
- The memlog's "(the existing ToolRunner seam)" is struck.

---

## H2. High: reader taint can follow the at-rest mark instead of the part, so a clean-marked row with injected text leaves its reader clean

**Spine text.**
- AD-10 (line 192): "A step is **tainted** once any `UntrustedData` enters its context."
- AD-10's type list: "free-text in table rows … and rows written by a tainted step or by a Source load, flagged by the row's `taint` column".
- (c) restated: "a row or document version written by a tainted step carries that taint at rest to every later reader … a step that reads it is tainted".
- AD-43 (line 418): each lineage edge records "its taint".

**Two units, each obeying the letter.**
- *Unit A, the rows-input and hand-off readers* (3.6 `row_inputs`, 3.15, 4.9). They follow (c) as restated and record each edge's taint as the row's at-rest mark. The brownfield computes an attempt's taint from that value, not from the part: `Inputs::place(taint, part)` raises `self.taint` by the `taint` argument (`run.rs` L766-L767), and the plan's row 38 gives "a clean row's edge has taint 0, and its text is still fenced". So a clean-marked row enters as `Part::Untrusted` while the attempt stays clean.
- *Unit B, AD-10's first rule.* Any `UntrustedData` taints the step.

**Clash.** AD-10's rule says a fenced part taints. (c)'s restatement and the edge's recorded taint invite "the mark taints". The brownfield mechanism implements the second.

**Attack, obeying (c) to the letter.**
1. A row is marked clean but holds injected text. Ways this can happen:
   - a Human grid edit (pasted from a page), or a CSV or xlsx Import (AD-21: imports are not flagged; epic 4 story 9's ticket says the opposite: "rows written by a tainted step or an import reach prompts only as `UntrustedData`");
   - an after-image under H3.
2. A later step reads it. The text is fenced, but the step is not tainted.
3. That step's output becomes a `TrustedInput` downstream, its Proposals are approvable, its external effects need no untrusted line, and AD-11's tainted-read rule does not bind it. It can then compose `read_url` paths or queries to allow-listed hosts that carry table data out.

**Fix (no new owner call; it is AD-10's existing letter).** Append to (c) restated:

> The at-rest mark is a floor, never the trigger. Every stored item that enters a prompt (a row's text, a document version, a workspace file) enters as `UntrustedData` and taints the reader whatever its mark says.
>
> The engine raises an attempt's taint from the kind of each part that enters its request: `Part::Untrusted` and `Part::ToolResult` taint; `Instruction`, `TrustedInput` and `TrustedToolResult` do not. It never raises it only from a taint value passed beside the part. An edge may record the item's mark for lineage, but the reading attempt is tainted.
>
> The mark adds taint where the text alone would not:
> - hand-off payloads (AD-41);
> - Proposal cards (AD-15);
> - the UI;
> - the injection pack.

**Owner call (separate, optional):** whether Human-imported rows are also *marked* tainted at rest, as epic 4 story 9's ticket assumes. With the fix above, their text taints readers either way.

---

## H3. High: an after-image merges earlier tainted fields under a clean writer's mark, so reads by `seq` launder

**Spine text.**
- AD-10 (c): "document versions and workspace files carry the taint of the step that wrote them"; rows "carry `taint`".
- (c) restated: "A row's taint only rises, so a later clean write to it never clears it."
- AD-21: "Every row mutation appends a `row_history` after-image".
- AD-22, AD-41 ("table rows as a snapshot at the producing step's commit `seq`") and AD-43 ("row snapshot `seq` … and its taint") read rows by after-image.

**Two units, each obeying the letter.**
- *Unit A, the 3.6 writer.* An update merges its patch into the current data. Plan row 3: "Row version 2 = version 1 merged with the named columns; a second after-image (the whole row)". Plan row 13 marks each after-image with "the writer's" taint: "version 3's after-image records taint 0 (the writer's)". Only `table_rows.taint` rises. That obeys "a row's taint only rises" to the letter.
- *Unit B, the readers by `seq`* (3.15's snapshot payloads, 3.17's lineage, AD-22 grant reads "as of `start_seq`", Q2's "old version stays readable"). They take the after-image's mark, which is "the taint of the step that wrote" that version.

**Attack.**
1. A tainted step patches field X with injected text (version 2, marked 1).
2. A clean step patches field Y (version 3). Version 3's after-image holds X and is marked 0.
3. A snapshot at version 3's `seq` is delivered or read as clean.

Combined with H2's mechanism or H4's fencing rule, the injected X reaches a clean reader.

**Fix (no new owner call; it implements the owner's Q4 condition "carries that taint to any later reader").** Add to (c) restated, and mirror it in AD-21:

> Each `row_history` after-image is marked with the row's taint after that write: the higher of the writer's taint and the taint of the version it replaced, because an update merges into the earlier data. A writer's own taint, if kept, is provenance only, and no reader uses it for trust. A row read at any `seq` (a grant read, a hand-off snapshot, a lineage edge, an earlier version kept readable) carries that after-image's mark.

Sofia: the 3.6 plan's row 13 changes to "version 3's after-image is marked 1".

---

## H4. High: a rows or document hand-off takes only the sender's taint (AD-41), and the AI reviewer fences "if tainted" (AD-42)

**Spine text.**
- AD-41 (line 406): "the delivery carries the producing step's taint …; a receiving step that reads a tainted payload is tainted".
- AD-42 (line 412): the AI reviewer's prompt holds "the exact payload (labelled per AD-27, fenced if tainted)". Epic 3 story 16 repeats "(fenced if tainted)".
- (c) restated names "a rows or document hand-off" among its paths but amends neither AD-41 nor AD-42.

**Two units, each obeying the letter.**
- *Unit A, 3.15's delivery.* It writes `handoff_deliveries.taint` as the producing (sending) step's taint, per AD-41. A `rows:<table-id>` payload is a snapshot the engine takes at the sender's commit `seq`, and the sending step need never have read the rows. So a clean sender delivers tainted-at-rest rows with delivery taint 0.
- *Unit B, 3.16's AI review step.* It fences "if tainted", meaning the delivery's taint. For a clean delivery, the only non-fenced part it can build is a `TrustedInput`. `TrustedInput::from_step_output(label, text, producer)` is callable anywhere in `crates/am-engine/src/` (`deps.rs` L56-L62: a crate-level rule that checks names, not where the text came from). The builder passes the delivery's taint as `producer`.

**Attack.** Tainted rows reach the AI reviewer as trusted text, outside the fence. AD-27 (line 294) says a rows or document payload is never a `TrustedInput`, but AD-42's narrower words are the ones its unit follows. The reviewer is then clean. So its return note becomes a `TrustedInput` to the producer (AD-36, AD-42), and its verdict is not marked tainted in lineage. 3.15's verify seeds taint-1 rows but never pairs a clean sender with tainted rows, so no test catches this.

**Fix (no new owner call; it implements the owner's Q4 condition).**
- AD-41: "the delivery carries the higher of the producing step's taint and the at-rest mark of every row version in a `rows:` snapshot, or of the document version in a `document:` payload (AD-10 (c), restated 2026-11-24)".
- AD-42: replace "(labelled per AD-27, fenced if tainted)" with "(labelled and placed per AD-27: a `TrustedInput` only when it is a step output whose producer was untainted; a rows or document payload is always fenced)".
- Sofia: 3.15 and 3.16 each gain a verify case: a clean sender with tainted-at-rest rows leaves the receiver and the AI reviewer tainted and the payload fenced.

---

## M1. Medium: "counts" and "version numbers" are not tied to the call itself, which opens a covert channel from other writers; row ids are not engine-minted for every actor

**Spine text (AD-10, line 192):** "solely from engine values: ids the engine minted (the row id), version numbers, counts and fixed status words written in code, taken as typed values from the writer's committed record of that call".

**Attack.** Every listed value is an "engine value", yet several can reflect state that other writers, including tainted steps and Source loads, set.
- A *count* of rows an update-by-filter matched, or a table's size, lets a tainted writer in another run encode bits by inserting rows. A clean step reads them in a non-tainting result.
- A *version number* from an update without an expected version reveals how many times other writers bumped the row.
- A partially refused batch ("3 written, 1 refused") turns store-state-dependent refusals into trusted counts.

**Two units.**
- *Unit A, a v1.x bulk or filtered `table_write`* (the type is general: "a local-write built-in tool").
- *Unit B, ToolGate.* It obeys "counts" to the letter.

**Second gap: row ids for every actor.** AD-21 never says row ids are engine-minted for every actor. Epic 12's Import could key rows by the file's own key column (external text). A clean step's update of that row would then carry import text in a "row id" of a trusted part. The plan's "row ids are engine-minted UUIDv7s" is an approved not-question, but it is not in the spine. Q2's "update by row id with an expected version" is not in the spine either.

**Fix (no new owner call; it records Q2 and the approved not-questions).**
- AD-10: replace "ids the engine minted (the row id), version numbers, counts and fixed status words written in code" with:

  > only facts fixed by this call's own input and the engine's mints:
  > - the row id the writer minted (a UUIDv7, AD-21);
  > - the version this call wrote (an update must state its expected version, and one without it is refused);
  > - a count of the rows this call itself wrote;
  > - a status word from a closed set written in code.
  >
  > It never holds a value that depends on rows, versions or counts other writers set (no table size, no matched-row count, no other row's version). A call any part of which is refused is an error result as a whole. The constructor takes only typed values (a UUID, integers, a closed enum), never text.
- Trust test, added to `taint_trusted_tool_result_*`: two runs that differ only in rows other writers wrote, tainted ones included, give byte-identical results for the same call.
- AD-21: "Every row id is a UUIDv7 the writer mints, for every actor (Seat, Human, Import); an imported key is a data column, never the row id."

---

## M2. Medium: AD-8's idempotency key `(run, step, call_seq)` was not amended, so a replay can return another attempt's or branch's write as a trusted success

**Spine text (AD-8, line 155):** "Local-write tool calls are idempotent by (run, step, call_seq)." The memlog reads this as `(step_run_id, call_seq)`, but the spine still says "(run, step, call_seq)". Epic 3 story 6's ticket repeats "idempotent by run, step and call sequence".

**Two units, each obeying the letter.**
- *Unit A, the store.* It keys on `(run_id, step_key, call_seq)`.
- *Unit B, the engine.* It keys on the step run, because loop-back attempts and fan-out branches are separate step runs (AD-8, AD-40).

The brownfield also has two counters, both named `call_seq`: `model_calls.call_seq` (`run.rs` L1189) and `tool_calls.call_seq` (`run.rs` L1552).

**Attack or clash.**
1. Branch B of a folder fan-out (F3) makes its first `table_write`.
2. Under Unit A's key, it collides with branch A's first write.
3. The status `AlreadyWritten` comes back with A's row id and version as B's trusted success, and B's data is silently dropped.

Keyed on the model call's sequence, two writes in one turn collide in the same way.

**Fix (no new owner call; it is the memlog's reading).** AD-8:

> Local-write tool calls are idempotent by (step run, `call_seq`). The step run is the attempt, and a fan-out branch is its own step run. `call_seq` is the tool call's sequence the engine journaled `issued` before the tool runs, never the model call's. A replay returns only the record of that same step run's same call.

Sofia updates 3.6's ticket words.

---

## M3. Medium: nothing says how a `TrustedToolResult` is journaled or rebuilt on resume or in drill-down

**Spine text.**
- AD-10: one constructor, "called only by ToolGate"; no `Deserialize`.
- AD-27: "Each part carries a source label stored in the journal and shown in drill-down."
- AD-8: resume re-enters an attempt.

The memlog says "this also makes 3.9's resume rebuild the same result from the journal", but no AD says it.

**Two units.**
- *Unit A, 3.9's resume.* It rebuilds an attempt's messages from journaled prompt parts. It cannot call the constructor (check-deps) and cannot deserialize the type. So it replays the stored text as some other part: `Part::Untrusted`, which taints and makes a clean attempt's taint diverge across a crash, or a hand-built text part in a worse position.
- *Unit B, ToolGate.* It expects to be the only builder.

**Fix (no new owner call).** Add to AD-10's *Trusted tool result*:

> A `TrustedToolResult` is journaled by reference: the journaled id of the tool call whose writer record it reports, never its text. On resume (AD-8) and in drill-down (AD-27), only ToolGate's constructor rebuilds it, from that same record. A result the engine cannot rebuild that way is replayed as `UntrustedData` and taints the resumed attempt.

---

## M4. Medium: eligibility can be keyed on the name `table_write`, and MCP tool names are not namespaced

**Spine text.**
- AD-11 (line 198): "A successful `table_write` call's result is the `TrustedToolResult`".
- AD-26: MCP tools are named by their server, defaulting to external-effect. "Lowering a class is privilege-widening", so an owner can confirm it.
- No AD forbids an MCP tool named `table_write`, and `ToolGate::label` keeps any manifest's name as is (`toolgate.rs` L168-L173).

**Two units.**
- *Unit A, ToolGate.* It decides trust by name and class (`table_write`, local-write).
- *Unit B, an MCP server assigned to a seat.* It exposes `table_write`, and its class was lowered to local-write with confirmation.

**Clash or attack.** Today the "no committed record → error" clause turns Unit B's result into `UntrustedData`. Combined with M2's colliding key, though, a record does exist. ToolGate then builds a trusted acknowledgement for an external server's call and discards what the server returned. Taint is skipped for a call to an untrusted server, and the journal is wrong.

**Fix (no new owner call; Sofia places the validator rule).** AD-11:

> Whether a result is a `TrustedToolResult` is decided by the engine's own row path (a `table_write` Permit that ToolGate sends to the `RowTool` and `RowWriter`), never by the tool's name.

AD-26:

> `agentic validate` refuses an MCP tool whose name equals a built-in tool id, and ToolGate refuses such a name from any runner.

---

## M5. Medium: (c)'s "every later reader, by any path" leaves out the Assistant's `v_*` reads, and views need not carry `taint`

**Spine text.**
- (c) restated: "every later reader, by any path (a table read or query, a rows or document input, a rows or document hand-off)", and "a Proposal built from it says so (AD-15)".
- AD-21: views `v_<table-id>` are "regenerated on config apply", with no column list.
- AD-34: the Assistant queries `v_*` and proposes.
- AD-15: "A Proposal whose source rows are tainted says so on its card".

**Two units.**
- *Unit A, epic 7's view generator.* It projects the data columns, which obeys AD-21.
- *Unit B, the Assistant's Proposal card.* It must say whether its source rows are tainted, but cannot see that through the jailed connection (AD-6 allows only `v_*`). The Assistant is not a step, so (c)'s "a step that reads it is tainted" does not reach it.

**Fix.**
- AD-21: "Each `v_<table-id>` view carries the row's `taint`."
- AD-34: "View rows enter the Assistant's prompt as `UntrustedData`; a Proposal it builds from a tainted row says so on its card (AD-15)."
- Add "the Assistant's `v_*` query" to (c)'s list.

None of that needs an owner call. **Owner call:** whether an Assistant Proposal built from tainted rows becomes unapprovable, like a tainted-origin Proposal.

---

## M6. Medium: two ways to create a document version, the final message (Q5) and `write_document`

**Spine text and plans.**
- Q5 and Q7 (memlog): "a document version comes from the document step's final message"; "one version per succeeded attempt".
- AD-10 (b) (line 192): "`write_document` only its declared document".
- AD-11: `write_document` is a local-write tool.
- AD-29 (line 306): "versioned per run". It was not amended; the memlog's reading, "the run's version being its latest", lives only in the memlog.
- Epic 3 story 13 (golden): "only table_write and write_document called".
- The 3.6 plan: "`write_document` … (Sofia places them)".

**Two units.**
- *Unit A, 3.6.* It versions the final message once per succeeded attempt.
- *Unit B, the story that builds `write_document`, or 3.13's golden scripts.* It creates versions by tool call, up to 5 per step (`tool_manifest.rs` L101).

The result is two owners of `document_versions`, and Q7's one-per-attempt rule breaks.

`write_document`'s success is also excluded from `TrustedToolResult` ("in v1 only `table_write`"). So every golden step that calls it is tainted, and so is everything downstream of it in F1.

**Fix.**
- AD-29: record the memlog's reading, "each version names its run; a run's version is its latest". No owner call.
- **Owner or Sofia call** on `write_document`. Either retire it from v1 (AD-10 (b), AD-11, 3.13's ticket), or route it through the same version command under Q7's one-per-attempt rule and name it beside `table_write` in AD-10's "in v1 only" list.

---

## M7. Medium: two writers of a `table_write` call's final status

**Spine text.**
- AD-8: "Each state transition commits before the side effect it authorises".
- AD-10: success means "the writer's committed record of that call"; "a call the writer holds no committed record of" is an error.

Neither says who finishes a local-write tool call.

**Two units.**
- *Unit A, the 3.6 row command.* Its transaction finishes the tool call `succeeded` (plan L375).
- *Unit B, the brownfield run loop.* It finishes every call itself: `succeeded` on `Ok`, `failed` on an error or when the deadline fires (`run.rs` L1608-L1636).

**Clash.**
1. A write commits.
2. The wall-clock deadline fires before the gate returns.
3. The loop relabels the call `failed`, or its second finish is refused and the attempt errors.
4. 3.9's resume then finds a committed record for a call the journal says failed, and a replay returns `AlreadyWritten` as trusted.

**Fix (no new owner call).** AD-8:

> A local-write call's `succeeded` commits in the same transaction as its write. A deadline, cancel or runner error after that commit leaves the call `succeeded`, though the attempt may still fail. AD-10's "successful" means exactly that this committed record exists for the call's journaled id.

---

## L1. Low: changing a table's or document's owning team is not a widening kind (needs an owner call)

Q3 (approved): "a seat writes only its own team's table; one team per document id". AD-21: "Each table has exactly one owning team." So moving table T from team A to team B gives every one of B's seats whose steps declare T the power to write it. No widening kind lists that move, while adding or changing a step's `table:` is now one.

**Fix (owner call):** add "changing a table's or a document's owning team" to the same AD-15 kind and to 4.17's verify.

## L2. Low: the interim-risk sentence says "a hand edit"

**Spine text (AD-15):** "until epic 4 story 17 ships the classifier, a hand edit of either is unchecked". Every path that lands before 4.17 is unchecked too, because no classifier exists (Reload and startup per AD-16, `agentic migrate`).

**Fix:** "until epic 4 story 17 ships the classifier, any change to either (a hand edit picked up at Reload or startup, or a migration) is unchecked". This is wording only.

## L3. Low: enforcement precision and names

- AD-10 says the constructor is "called only by ToolGate in `am-engine`". But the call-site rule `TrustedInput` already has is crate-level (`deps.rs` `TRUSTED_CALLER = "crates/am-engine/src/"`), and a builder copying it would let any engine file build one. Fix: name the file, `crates/am-engine/src/toolgate.rs`.
- check-deps matches tokens, so a generic method name (`write`, `insert`) is not enforceable. Fix: say "the port's method and the row command have names unique in `crates/`".
- The approved plan names the type `EngineResult` and the part `Part::EngineResult`; the spine says `TrustedToolResult` and tests `taint_trusted_tool_result_*`. Sofia or the builder adopts the spine's names.
