# Adversarial review: AD-10 `TrustedInput` amendment (2026-11-23)

- **Reviewed:** working-tree diff of `architecture-agentic-manager.md` on 2026-11-23 (AD-10 third prompt-content type `TrustedInput { label, text }`; AD-27 and AD-36 now name it), read against AD-8, AD-11, AD-27, AD-36, AD-38, AD-40 to AD-44, AD-46 and the built code in `crates/am-domain/src/content.rs`, `crates/am-domain/src/llm.rs` (`Part`) and `crates/am-providers/src/{anthropic,openai_compat,common}.rs`.
- **Scope:** how the decision is written. The owner decision (a third type, built by `am-engine` from a clean step's journaled output or an owner-authored note) is taken as final; every fix below stays inside it.
- **Verdict:** **amend before story 3.3 writes code.** One critical laundering path, two high shape clashes, five medium and two low findings. Findings 1 to 3 change what story 3.3 builds; the rest can land in the same amendment.

| # | Severity | Finding |
|---|---|---|
| 1 | critical | A `rows:` or `document:` hand-off payload from a clean sender becomes a `TrustedInput` (AD-27 + AD-41); an AI reviewer's note then carries it on |
| 2 | high | Trusted content that arrives as a tool result (`read_skill_file`, an `ask_seat` answer) has no legal type or shape |
| 3 | high | "Built only by `am-engine`" and "no conversion" cannot be enforced as written |
| 4 | medium | AD-11's "trusted inputs" now admits model-written URLs (possibly table-derived) to a tainted step's recorded-URL set; the search-words rule leaves `TrustedInput` unnamed |
| 5 | medium | Three wordings of "the producer was clean", with no rule for retries, fallback, repair, resume or loop-back iterations |
| 6 | medium | Owner-authored text fits both `Instruction` and `TrustedInput` (b) |
| 7 | medium | A mixed fan-in cannot keep both the declared branch order and the AD-27 region order; the missing-branch and `unrouted` lists have no type |
| 8 | medium | Items clean at rest (documents, rows, workspace files) have no stated type, and the journal does not record a part's type |
| 9 | low | The rendered form is unspecified, so two providers and FakeLlm can differ; the wrapper is not protected against forgery |
| 10 | low | Lower-case "trusted input" remains in AD-11, AD-38, AD-42 and three tickets |

---

## 1. Critical: a clean sender launders rows and documents into `TrustedInput`

**Two units, each obeying the letter.**
- *Unit A, hand-off delivery (`am-engine`, AD-41):* "the delivery carries the producing step's taint", and the payload may be `document:<id>` or `rows:<table-id> [query]`, a snapshot at the producing step's commit `seq`. The sending step never has to read those rows: the payload is declared in config. So a clean sender writes `handoff_deliveries.taint = clean`.
- *Unit B, prompt assembly (`am-engine`, AD-27):* "a hand-off payload is labelled `handoff:…`; each such part is a `TrustedInput` (AD-10) only when its producer was untainted". The producer is the sending step, which is clean, so the payload is built as a `TrustedInput`.

**Path.** A Source load writes rows (always tainted at rest, AD-39), or a tainted step writes free text into a row, or a document version is written by a tainted step. A clean step in department A with `handoff: { payload: rows:<table-id> }` (or `document:<id>`) sends it. Department B's step receives the injected text as a `TrustedInput`: outside the fence, it does not taint the step, and the step may act outward without Approval. A review gate on the hand-off makes it worse. The AI reviewer "reads a tainted payload" only if the delivery is marked tainted, which it is not, so the reviewer is clean and its return note (which may quote the row) is itself a `TrustedInput` under AD-10 (a). AD-10's sentence "reading a tainted row … never yields one" does not stop this: Unit B does not read a row; it renders a delivery whose recorded taint is clean. The same applies to free text in rows, which AD-10 lists as `UntrustedData` whatever the `taint` column says.

**Fix (spine wording).**
- AD-27, Branch and hand-off inputs: replace "each such part is a `TrustedInput` (AD-10) only when its producer was untainted" with: "a branch output, or a hand-off payload of kind `output`, is a `TrustedInput` (AD-10) only when the step run that produced it was untainted; a hand-off payload of kind `document:` or `rows:` is never a `TrustedInput`: it enters by the rules for a document or table input (AD-10 (c)), inside the untrusted fence with its `handoff:` label."
- AD-41, Taint and provenance travel: "the delivery's taint is the producing step's taint joined with the at-rest taint of everything in the payload (a document version's `taint`, any row's `taint`) and, for `rows:`, is always tainted when a delivered column holds free text (AD-10)."
- AD-42: "an AI reviewer is a tainted reader when the delivery's taint (AD-41) is tainted, or when the payload is of kind `document:` or `rows:`."
- AD-10 (a): after "the journaled output of a step", add "(its final message, AD-36; never a document version, row, workspace file or file record, whatever its taint)".
- Trust test: `taint_handoff_rows_never_trusted.rs` (a clean sender, Source-loaded rows, a review gate in between; assert a fenced part and a tainted reviewer).

## 2. High: trusted content that arrives as a tool result has no legal type or shape

**Clash.** `Part::ToolResult { content: UntrustedData }` (llm.rs) makes every tool result untrusted, and AD-27 puts `TrustedInput` parts in one place: after the step instruction and before the fence, in the opening user turn. Two spine rules then have no legal type:
- **`read_skill_file`** (AD-38): it "returns the file as a trusted input labelled `skill:<id>/<file>`". Under the new AD-10, a skill file is neither (a) nor (b), and "Nothing else yields one". The epic 9 story 16 builder must choose:
  - `TrustedInput`, which breaks AD-10;
  - `UntrustedData`, the only `ToolResult` shape, which taints every seat that opens a reference file of its own reviewed skill and binds its writes to its declared outputs;
  - `Instruction`, which the `ToolResult` shape cannot carry.
- **`ask_seat` answer** (AD-46): it is "`UntrustedData` when the answering step read untrusted content". Otherwise it is the journaled output of a clean one-step run, so AD-10 (a) makes it a `TrustedInput`. But it arrives mid-turn as a tool result, where neither the `Part` shape nor AD-27's position allows one.

**Fix (spine wording).**
- AD-38: replace "returns the file as a trusted input labelled `skill:<id>/<file>`" with "returns the file as `Instruction` text labelled `skill:<id>/<file>` (it is reviewed project config served from the pinned snapshot, like the SKILL.md body), never a `TrustedInput`; reading it never taints the step."
- AD-10 (new sentence): "A tool result is `UntrustedData`, except (i) `read_skill_file`, whose result is `Instruction` (AD-38), and (ii) an `ask_seat` answer from an answering step run that was untainted, which is a `TrustedInput` labelled `ask:<seat-id>@<step-run>`. `am-domain`'s tool-result part carries one of these three types, and only `am-engine` sets the non-`UntrustedData` arms."
- AD-27: "`TrustedInput` parts built at step start sit after the step instruction; a `TrustedInput` or `Instruction` that a tool returns renders in that tool result's place, labelled, outside any fence."
- Alternatively, if the owner prefers the `ask_seat` answer always fenced, AD-46 says so ("always `UntrustedData`") and drops the conditional. Either way, one sentence must decide.

## 3. High: "built only by `am-engine`" and "no conversion" cannot be enforced as written

**Gap.** `am-domain` is a dependency of every crate (Design Paradigm). A public `TrustedInput::new(label, text)` can be called from `am-tools`, `am-mcp`, `am-connectors`, `am-providers`, `am-server` and `am-api`. "No `From` or `TryFrom`" is cosmetic: `TrustedInput::new(l, u_text)` and `Instruction::authored(ti.text())` are conversions in all but name. The built precedent already leaks:
- `Instruction::authored` is `pub`, and `Instruction` derives `Deserialize`, so any crate that deserializes JSON can mint one.
- `UntrustedData::content()` plus `Instruction::authored` is an open `UntrustedData` → `Instruction` path today.

A story builder copying `Instruction`'s derives gives `TrustedInput` the same holes. `am-store` (journal reload) or `am-server` (an API body for a Human return note) would then mint one without any taint or `Actor::Human` check.

**Fix (spine wording, AD-10).**
- "`TrustedInput` has private fields, derives `Serialize` but never `Deserialize`, and its constructors are called only from `am-engine`'s `src/`. `cargo xtask check-deps` fails the build on any call to a `TrustedInput` constructor, or to `Instruction::authored`, outside `am-engine` (and `am-config` for `Instruction`), except in `#[cfg(test)]` code."
- "The journal stores a `TrustedInput` as its label, its text and its producer reference (step run or Human audit entry). On resume, `am-engine` mints it again only after re-reading that producer's committed taint (step-output kind) or confirming `Actor::Human` on the audit entry (note kind). A stored part is never trusted because of its stored type."
- "A Human note reaches `am-engine` as a typed writer command carrying `Actor::Human`, never as a deserialized `TrustedInput`."
- Name the trust tests: `taint_trusted_input_*.rs` covers each source in (a) and (b), and the refusal of each path in findings 1, 2 and 5.

## 4. Medium: AD-11 now reads model-written URLs as recorded URLs

**Interaction.** AD-11's tainted-read rule allows a URL "byte-identical to a URL recorded for this step, meaning one present in the step's trusted inputs", and forbids "model-composed path, query string or body". After this amendment, "trusted inputs" reads as `TrustedInput`, which is mostly *model-written* output of clean steps. A clean branch that read structured columns of its own table (not free text, so not tainted) may honestly emit `https://<allowed-host>/x?id=<row value>`. A sibling branch reads an injected page, so the fan-in is tainted and holds the clean output as a `TrustedInput`. The injection then steers the fan-in to fetch that URL, carrying table-derived data out. AD-10 says that must never happen. The search-query rule uses "`Instruction` parts" only, which is right, but nothing tells the ToolGate builder not to add `TrustedInput` to that word set.

**Fix (spine wording, AD-11).** Replace "present in the step's trusted inputs" with "present in the step's `Instruction` parts or in an owner-authored `TrustedInput` (AD-10 (b)), or in a step-output `TrustedInput` only when that URL was itself a recorded URL of the producing step under this rule". After "occur in the step's `Instruction` parts", add "(never words from a `TrustedInput`)".

## 5. Medium: "clean when it wrote it" has three wordings and no rule for retries and resume

**Clash.** The spine has three wordings for the same test:
- AD-10: "a step that was untainted when it wrote it";
- AD-27: "its producer was untainted";
- AD-36: "the step that wrote them was untainted".

AD-10 also says a step is tainted "once any `UntrustedData` enters its context", but context is per model call. Two builders can differ:
- *The loop-back builder* judges taint from the context of the call that produced the final message, so a fallback or retry attempt with a fresh context reads as clean.
- *The lineage builder* (AD-43, `step_inputs` with taint) marks the step run tainted because attempt 1 read a web page.

The same output is then a `TrustedInput` to one unit and a tainted node to the other. Similar splits come from:
- a schema repair;
- a step that asked a Question, went `waiting_human` and resumed;
- a resumed model call (AD-8);
- a loop-back re-run that is handed its own earlier, tainted output.

**Fix (spine wording, AD-10).** "Taint is a property of the step run and only ever turns on: it is committed in the journal, in the transaction that admits the first `UntrustedData` to any model call of that step run (attempts, retries, fallback, schema repair and resume included), and never cleared. 'Untainted' in (a), AD-27 and AD-36 means that committed flag was off when the output committed. A loop-back re-run that receives its own earlier output takes that output's taint." Then use that one phrase, "the step run that produced it was untainted", in AD-27, AD-36 and AD-42.

## 6. Medium: owner-authored text fits two types

**Clash.** AD-10 defines `Instruction` as "config- or owner-authored" and `TrustedInput` (b) as "an owner-authored note". Run-time owner text other than a gate return note has no owner:
- a Question answer typed while a step waits (AD-8 `waiting_human` → `running`);
- a Budget-ask note;
- the free-text question of an ad-hoc "Ask a seat";
- a guidance note (AD-27 places it in the instruction region).

The Question unit (Inbox) and the gate unit can then choose differently. One choice moves owner words into the instruction region and the AD-11 search-word set; the other keeps them out.

**Fix (spine wording, AD-10).** "`Instruction` is text from project config files (including applied guidance and memory notes) and the step instruction of an owner's ad-hoc Ask. `TrustedInput` (b) is exactly: a Human gate or review return note, and the free text of a Human answer to a Question, each from a writer command carrying `Actor::Human`. A new kind of run-time owner text joins this list by amendment."

## 7. Medium: mixed fan-in ordering, and the extra lists, have no rule

**Clash.** AD-27 requires branch parts "in the fan-out's declared order", and also requires `TrustedInput` parts and then fenced `UntrustedData`. With branches X (clean), Y (tainted), Z (clean), one builder interleaves X, fence(Y), Z, putting a `TrustedInput` after a fence. Another builder splits the regions, X, Z then fence(Y), which breaks the declared order. Drill-down and FakeLlm fixtures will disagree. Two related lists have no type:
- The missing-branch list ("timed out", "failed", "cancelled at quorum") is engine-composed.
- The folder fan-out's `unrouted` list (AD-40) holds file names from a linked folder, which are `UntrustedData` by AD-10. A builder might place it beside the clean branch parts.

**Fix (spine wording, AD-27).** "Within each region, parts keep the fan-out's declared order. The missing-branch list (seat ids and the fixed reason words) is `Instruction`, composed by `am-engine`. The `unrouted` list and every file name or path from a linked folder are `UntrustedData`, fenced with the label `unrouted:<fan-out-step>`."

## 8. Medium: clean-at-rest items have no stated type, and the journal does not record part types

**Gap.** AD-10 (c) says a tainted-at-rest document, workspace file or row "returns `UntrustedData`", which implies a clean one returns something else. But "Nothing else yields" a `TrustedInput`, it is not `Instruction`, and the built `ToolResult` is `UntrustedData`-only. So the `workspace_read` builder (AD-46) and the document-input builder (epic runs-inbox-documents) each guess. Separately, AD-27 stores "a source label" per part in the journal, but not the part's *type*. Drill-down cannot then show which parts were trusted, and resume cannot rebuild the prompt without recomputing.

**Fix (spine wording).**
- AD-10 (c): add "A document version, row, workspace file or file record is never a `TrustedInput`. Its content enters as `UntrustedData`, and it taints the reader unless it is clean at rest *and* holds no free text from a row (AD-10). Clean-at-rest content is still fenced."
- AD-27: "Each part's type (`Instruction`, `TrustedInput`, `UntrustedData`), label and producer reference are stored in the journal and shown in drill-down."
- If the intended rule is that a clean document read does not taint, say so here in the same words.

## 9. Low: rendered form unspecified; wrapper not protected

**Gap.** "Renders as its own part, with its label … never in the system or instruction position; `am-providers` renders it so." But `Part::Instruction` already renders as plain user-role text in both `anthropic.rs` and `openai_compat.rs`, so the spine does not say what makes a `TrustedInput` look different. The Anthropic adapter, the OpenAI-compatible adapter and FakeLlm fixtures can each invent a wrapper. `neutralise` escapes only `</untrusted-`. Untrusted text inside the fence can therefore forge a `<trusted-input label="review:…">` block, and clean model text can forge a closing tag.

**Fix (spine wording, AD-27).** "A `TrustedInput` renders as one user-role text block, `<input source="<label>">…</input>`, built by one function in `am-providers::common`, used by every adapter and by FakeLlm's recorder. That function and `render_untrusted_text` neutralise every opening and closing tag of both wrappers, in any letter case, in the text and in the label."

## 10. Low: lower-case "trusted input" remains

AD-11 ("the step's trusted inputs"), AD-38 ("a trusted input labelled `skill:…`"; see finding 2), AD-42 ("the note as a trusted input") and the tickets still use the lower-case phrase. The tickets are `epic-workflow-engine-cli/tickets/tickets.toml` lines 66 and 176, and `epic-connectors-sources/tickets/tickets.toml` line 71 ("its trusted inputs" in the tainted-read rule; see finding 4). A builder reading only a ticket will not meet the type. **Fix:** name `TrustedInput` (AD-10) in AD-42, and apply findings 2 and 4 to AD-38 and AD-11. Then ask Sofia to refresh those three ticket lines in the same REPLAN.
