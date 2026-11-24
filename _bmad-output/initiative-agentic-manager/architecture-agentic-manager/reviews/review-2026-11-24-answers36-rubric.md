# Rubric review: owner answers to the 3.6 gate (AD-10, AD-15, AD-29, AD-34, Deferred, Open Questions), 2026-11-24

Reviewed: the uncommitted `git diff` of `architecture-agentic-manager.md` on `replan/owner-answers-3-6` at main 9d2301e. That covers AD-10 Rule (the other local-write results, "The Assistant is not a step", the injection-pack gate), AD-15 Binds, Prevents, *Tainted Proposals* (1) to (3) and the two widening clauses, AD-29 (`write_document`), AD-34 Binds, Prevents and *Untrusted content*, the new Deferred row, the rewritten "Resolved 2026-11-24" entry, the "For Sofia" bullet and the new Open item. I also read the 13 new `.memlog.md` entries.

I checked the change against:

- the owner's verbatim answers;
- the rest of the spine: AD-6, AD-8, AD-11, AD-15 "Who may decide", AD-21, AD-28, AD-35, AD-36, AD-38, AD-41, AD-43 and AD-46;
- epic 3 story 4's plan (the tainted Review: warning, typed id, `confirmItemId`, 422 `confirm-required`);
- the approved story 3.6 plan, read only in the main checkout;
- tickets 3.6, 3.13, 4.9, 4.10, 4.17, 4.18, 6.8, 8.4 to 8.7 and 10.2;
- the Company A sample (`workflows/f1.yaml`, `seats/d1-head.md`, `seats/d2-head.md`);
- the brownfield store migrations 0001, 0007 and 0009;
- `am-domain` (`capability_of_tool("write_document") == None`);
- PRD RULE-8, `trust-gate.md` and the UX spine (EXPERIENCE.md, `mockups/key-assistant.html`).

The review was read-only: I edited nothing except this file and ran no cargo command.

## Verdict

**Not ready to merge as written.**

Owner answers 1, 3 and 5 are recorded faithfully. The binding constraint holds: a tainted-origin Proposal is still never approvable, "no typed id, token or class lifts it", and the class is fixed at creation and never moves. The Deferred row is safe, because while it stands every unit treats the other local-write results as `UntrustedData`. The architect's broader reading of "built from tainted rows" is stricter than the owner's words, not weaker, and it is flagged as an Open item.

Three defects block the merge, and each lets two units below diverge:

1. **H1. The definition of a document's owning team contradicts the approved story 3.6 plan.** The spine gives a document the team of the declaring *workflow*. Story 3.6 Q3, which the owner approved, gives it the team of the declaring *seat*, and F1 is a case where the two differ. As written, epic 4 story 17 would miss the two edits that actually move a document to another team.
2. **H2. The spine does not say where a `write_document` version's Markdown is stored.** Story 3.6 keeps a version's Markdown only on the attempt's `step_runs.output`, which AD-36 says is the final message. The journal has no column for a tool call's input. So the new story must either break AD-36 or invent a storage home and a resume path that the spine does not give.
3. **H3. AD-15's three-way partition says "workflow step" and makes everything else ordinary.** That contradicts AD-10 (b) and AD-46, which say "from a tainted step". It leaves a tainted one-step run's `remember` or `propose_change` approvable: an Ask a seat run, or an `ask_seat` answer run.

All three can be fixed by the architect in the same edit. None needs a new owner call: H1 follows the owner-approved 3.6 Q3. The six mediums can go in the same edit.

## Findings

### Critical

None.

### High

**H1. The owning team of a document is defined by the declaring workflow. Story 3.6 Q3 and AD-21 define ownership by the writing seat. The classifier would miss the real ownership changes.**

- Spine, AD-15 widening kinds: "changing which team owns a table or a document (owner, 2026-11-24): a table's `team` (AD-21), or any patch after which a document id a step declares belongs to another team than before (a document has no config file, AD-35; **its team is that of the workflow whose step declares it, one team per id**), so a declaring workflow's `team` changed or the declaration moved into another team's workflow, the classifier comparing each table's and each declared document's owning team in the old and new snapshot;"
- The same reading appears in the Open Questions "Resolved" entry (2): "a document's team is that of the workflow declaring it". The memlog says it rests on "epic 3 story 6 Q3: one team per document id".
- Story 3.6's approved plan, Q3, says otherwise: "a document id is kebab-case and **owned by the declaring seat's team**, one team per id". Its code map says the same: "table exists, its team is the seat's team … document id kebab-case and one team".
- AD-21: "Each table has exactly one owning team. Writes come only from that team's seats." AD-41 allows delivery "into a step of the same run (the step's seat belongs to another department …)". So a workflow's team and its steps' seats' teams can differ.
- Concrete divergence in the sample. `workflows/f1.yaml` is `team: d2`, but its `consolidate` step runs `seat: d1-head` (`team: d1`) and writes `d1-opinion` with `write_document`.
  - Under 3.6's validate, `d1-opinion` belongs to d1.
  - Under the spine's rule, it belongs to d2.
- Failure scenarios for epic 4 story 17:
  - (a) A patch changes `consolidate`'s `seat:` from `d1-head` to a d2 seat. Under 3.6, `d1-opinion` moves from d1 to d2, which the owner's answer makes widening. The spine's rule sees no change: same workflow, same `team`. It is not flagged.
  - (b) A patch changes `d1-head.md`'s `team:` while it runs a step that declares a document. The ownership moves, and again nothing is flagged.
  - (c) A patch changes `f1.yaml`'s `team:`. The spine flags it, but under 3.6 no document changed owner.
  - (d) Two workflows of different teams, each with a d1 seat, declare the same id. That passes 3.6's validate ("one team": d1). It gives two owning teams under the spine's definition, so the classifier's comparison is undefined.
- Neither the AD-15 text nor the Open Questions entry marks the workflow-based definition as the architect's reading. The memlog alone calls it "Architect's reading", so it reads as the owner's words.
- **Fix.** Replace the parenthetical and the "so …" clause with this:
  > "(a document has no config file, AD-35; its owning team is the team of the seat of each step that declares it, one team per id, checked by `agentic validate`, epic 3 story 6 plan Q3; the architect's reading of 'which team owns'), so a declaring step's `seat` changed to a seat of another team, a declaring seat's `team` changed, or the declaration moved to a step whose seat is in another team; the classifier compares each table's `team` and each declared document's owning team in the old and new snapshot, reading the whole snapshot, not only the patch, as `ConfigMigration` compares grants"
  - Drop "a declaring workflow's `team` changed", or keep it only if the owner wants it, marked as such.
  - Correct the Open Questions entry (2) and the memlog entry to match.
  - In the "For Sofia" bullet, give 4.17 verify cases for:
    - a table's `team` change;
    - a document step's `seat:` moved to another team's seat;
    - a declaring seat's `team:` change;
    - a declaration moved to another team's seat.
  - Tell 4.17 that this kind needs the old snapshot beside the patch: the ticket's signature is `is_widening(patch)`.

**H2. A `write_document` version has no defined place for its Markdown. The rule cannot be met under the story 3.6 schema and AD-36 without a decision the spine does not make.**

- Spine, AD-29: "It makes no version itself: within one attempt the Markdown of its last successful call, or the final message when the attempt made none, becomes that attempt's version when the attempt succeeds, in the step's finishing transaction with its one Draft item, so a failed attempt adds none, and **the Markdown is stored once (AD-35, AD-43)**."
- Story 3.6 Q6, approved: "The Markdown is not copied. It stays once on the producing attempt's `step_runs.output`." Its `document_versions` row has no text column and is keyed by `step_run_id UNIQUE`. The Draft item's payload is "the producing attempt" (`inbox_items.payload REFERENCES step_runs(id)`, migration 0007).
- AD-36: "A step's **output** is its final message …". 3.6 adds: "A `next` input from a document step is still a step output".
- The brownfield journal keeps no tool-call input. `tool_calls` (0007) has no arguments column, and `prompt_parts` (0009) keeps only what was sent. AD-8 resumes an attempt by re-issuing only the in-flight call. So the Markdown of an earlier successful `write_document` call in that attempt must be committed with that call, or a resume cannot find it.
- Failure scenarios for the new story:
  - (a) It takes the smallest change and sets `step_runs.output` to the last call's Markdown. That breaks AD-36: the output is no longer the final message, and a `next` step receives the document. It also changes what 3.5's output hash and drill-down mean.
  - (b) It keeps the Markdown only in memory until the finish. A crash after the call and before the finish then loses the version, or silently falls back to the final message.
  - (c) It keeps 3.6's Draft payload, which is the attempt. The owner's Draft then shows the final message ("Done, I wrote it") instead of the document.
  - None of these is ruled out by the spine. Yet the memlog's stated aim for this rule is that "3.6's document step and the new story cannot diverge".
- **Fix.** Add to AD-29:
  > "A successful `write_document` call's Markdown has one home, committed with that call's finish (journaled by `(step_run, call_seq)` like a `table_write` after-image, so a resume finds it, AD-8); the version records that call and reads its Markdown from there, and with no call it reads the attempt's output as in story 3.6. The attempt's output stays its final message (AD-36), so a `next` input from the step is never the document; the Draft item, a `document:` hand-off (AD-41) and lineage (AD-43) read the version, not the attempt's output."
  - In the "For Sofia" bullet, add that the new story extends 3.6's `document_versions` with the producing call and changes the Draft payload and the version read path, under 3.6's append-only triggers.

**H3. AD-15 (1) is limited to "a workflow step", and (3) makes "every other Proposal" ordinary. A tainted one-step run's Proposal becomes approvable, contradicting AD-10 (b) and AD-46.**

- Spine, AD-15:
  - "(1) **Tainted-origin**: created by **a workflow step's call** while that step run was tainted (`remember`, `propose_change`, the learning loop reading tainted reasons): never approvable …"
  - "(3) **Every other Proposal is ordinary.** A step's Proposal is (1) or ordinary, an Assistant Proposal (2) or ordinary …; none is both".
- AD-10 (b) says "`remember` and `propose_change` from **a tainted step** create a Proposal marked **tainted-origin**". AD-46 (Memory) says "from a tainted step it is tainted-origin and never approvable". AGENTS.md says "a Proposal from a tainted step is never approvable".
- Seat runs that are not workflow steps exist and can hold `remember` (the Memory capability) or `propose_change`:
  - R53's Ask a seat is "answered by that seat in a one-step run under its caps".
  - AD-46's `ask_seat` is "answered by a one-step run of that seat".
- Failure scenario: an Ask a seat run reads a poisoned row, is tainted, and calls `remember`. That run is not "a workflow step", and it is not the Assistant. Under AD-15's closed partition it is (3), ordinary: approvable with no warning and no typed id. An implementer of 4.10 or 9.18 who checks `runs.kind = workflow` builds exactly that. AD-10 (b) says tainted-origin, so two ADs now disagree.
- **Fix.**
  - In (1), write "created by any step run's call (a workflow step, a learning run, an Ask a seat or `ask_seat` answer run, a review step) while that step run was tainted".
  - In (3), write "Every other Proposal (from a Human command, an import, config alone, or the Assistant or a step run while clean) is ordinary".
  - Change "A step's Proposal" to "A step run's Proposal".
  - Make the same change in the memlog and in the injection-pack wording ("from a tainted step run").

### Medium

**M1. "before any other check" cannot be met as written. It puts the 422 ahead of authentication, the UI-class check and the lookup that tells the server the Proposal is case (2).**

- Spine, AD-15 (2): "the server refuses any apply call whose `confirmItemId` is not exactly that id (422 `confirm-required`), **before any other check**".
- AD-13 authentication and AD-15's UI-class rule (403 `ui-class-only` for the CLI token, as in 3.4's routes) must run first. The server also cannot know an item is case (2) until it has loaded it: an unknown id is 404 `not-found`. Story 3.4 fixes no such order; this is the architect's addition.
- Failure: epic 8 story 5 and epic 4 story 10 write status-code tests with different orders. For example, a CLI-token apply with no `confirmItemId` returns 422 in one and 403 in the other.
- **Fix.** "after authentication, the UI-class check and loading the Proposal, and before the snapshot-hash check, the confirm-token check and any write".

**M2. The one Proposal shape does not carry the creator class that approvability now depends on. Story 4.10, which builds the Proposal entity, is not in the "For Sofia" bullet.**

- Spine, AD-15: "There is **one Proposal shape**: a list of typed config patches, an effect summary, a `privilege_widening` flag, source references (rows, runs, or an import's source, AD-38) and a dry-run estimate when it starts a run." The new text says approvability "is fixed when the engine creates it, from who created it and the taint the engine recorded". AD-34 adds "records that taint and its source rows with their marks".
- Ticket 4.10 builds the entity before any Assistant Proposal exists. It knows only "tainted-origin", and its verify says "a Proposal **citing** tainted rows carries the flag". That wording is citation-based, which the new rule forbids ("never from the model's citations").
- Failure: 4.10 ships a boolean `tainted_origin`, and 8.5 has to migrate the entity to add case (2). Or 4.10 sets the tainted flag from citations.
- **Fix.**
  - Add to the shape: "its creator (step run, Assistant conversation, Human command or import) and its class, tainted-origin, built from untrusted content or ordinary, with the taint the engine recorded, fixed at creation (the store refuses changing them)".
  - Add 4.10 to the "For Sofia" bullet: build the three-valued class and refuse apply of (1) whatever the token, typed id or class. Its "citing tainted rows" verify becomes "whose engine-recorded source rows are tainted".

**M3. "Its source rows" is undefined: engine-recorded reads or the model's citations? The warning names only rows, though a conversation can also be tainted by a document version or a run's tool result.**

- Spine:
  - AD-34: "Every Proposal a tainted conversation builds records that taint and **its source rows** with their marks".
  - AD-15 (2): "name the tainted source rows **it lists**".
  - AD-34 Prevents: "its Proposals' taint depending on what the model chose to cite".
- Approvability is now engine-fixed, but what the card names is not. If "its source rows" means the model's citations, a poisoned row the model leaves uncited never appears on the card. And a conversation tainted only by a document or a tool result names nothing.
- Failure: 8.4 records every row the conversation read; 8.5 shows the model's citations. The card and the warning then disagree.
- **Fix.** In AD-34 write "its untrusted sources: every row, document version and run the engine recorded entering that conversation, with their marks; the model's citations are shown beside them and never shorten that list". In AD-15 (2) write "name the untrusted sources it records".

**M4. The owner's answer changes PRD RULE-8's threshold as written, but nothing routes that change.**

- PRD RULE-8 (`prd-agentic-manager.md`): "Threshold: the injection pack yields zero outbound calls and **zero approvable proposals**." `trust-gate.md` says: "zero approvable proposals (a Proposal from a tainted step is never approvable)".
- Under the amended AD-10 gate, epic 8 story 4's Assistant injection cases now yield approvable Proposals (case (2)), gated only by the typed id. Read literally, the release threshold fails.
- The "For Sofia" bullet reroutes 8.4 but not the PRD threshold or `trust-gate.md`. AGENTS.md says such a change goes to Sofia's REPLAN and is never edited to fit.
- **Fix.** Add to the "For Sofia" bullet: "the PRD RULE-8 threshold and `trust-gate.md`'s injection-pack line carry the owner's words (zero approvable Proposals from a tainted step run; zero Assistant Proposals from untrusted content applied without the typed id), through John or the owner as the PRD requires".

**M5. The UI interaction now bound to `web/` has no place in the UX spine. Epic 8 story 5 must "match" a mockup without it.**

- Spine: AD-15 Binds "`web/` (the typed item id)". Case (2) says "its card and its apply prompt say that it was built from untrusted content … the UI asks for the item's id typed back".
- Ticket 8.5: "Matches key-assistant.html §1 proposal card". EXPERIENCE.md and `key-assistant.html` have no typed-id field and no "built from untrusted content" state for a Proposal card. The 6.7 tainted-Review field from 3.4 is not in the UX spine either.
- Failure: 8.5's design QA (8.11) records the typed-id field as an unexplained difference, or the coder follows the mockup.
- **Fix.** Add to the "For Sofia" bullet a UX update for the Proposal card's untrusted state and typed-id field, shared with 6.7's tainted Review (for Sally, through the usual route). Until then, 8.5's verify names the typed id as a recorded deviation from §1.

**M6. The Open item's alternative reintroduces approvability based on citations. Its default also understates what sets the taint.**

- Spine, Open item: "confirm, or narrow it to **Proposals citing a row** whose taint mark is set."
- If the owner picks the alternative as worded, AD-15 ("never from the model's citations") and AD-34's new Prevents contradict it.
- The item also says only rows trigger the default. AD-34's default also counts "a document version or a run's tool result".
- **Fix.** Reword to: "confirm (any `UntrustedData` in the conversation, a row read whatever its mark, a document version or a run's tool result, asks for the typed id), or narrow it to conversations in which the engine recorded a row whose taint mark is set (never the model's citations)".

### Low

**L1. It is unclear whether a "Run now?" card from a tainted conversation is case (2).** "an Assistant Proposal (2) or ordinary (a "Run now?" card included)" can be read either way. A Run now card is started through the manual-start command (8.7), not through "an apply call". **Fix:** say whether a tainted conversation's Run now? card needs the typed id on its start call, and name 8.7 in the "For Sofia" bullet.

**L2. The conversation's taint is not persisted.** AD-34 says "only ever raised while the conversation lasts", but nothing says the conversation and its taint live in `am-store` and survive a restart. AD-35's table does not list conversations. Failure: a restart reloads a conversation with tainted content still in context but the taint cleared. **Fix:** "persisted with the conversation (AD-35: database) and read back, never recomputed", mirroring AD-10's committed step-run taint.

**L3. `ask_seat` is missing from AD-10's enumeration.** The list "(`remember`, `propose_change`, `draft_message`, `write_file`, `workspace_write`, `write_document`)" omits `ask_seat`, which AD-11 classes as local-write; its clean answer is a separate open question. **Fix:** append "(`ask_seat`'s clean answer stays the open question above)" so nobody reads the list as exhaustive.

**L4. The interim gap and story 4.18 are not stated for the team-ownership kind.** The adjacent `table:`/`document:` clause records "unchecked, an interim risk the owner accepted"; the new kind says nothing, and the memlog alone says it is "unchecked until 4.17". The bullet also names only 4.17, although Reload and startup (4.18) hold it. **Fix:** add "unchecked by hand edit until epic 4 story 17 ships the classifier (owner: 'add it to 4.17')", and add a 4.18 Reload and startup case to the bullet, as the gate-weakening kind has.

**L5. The `write_document` version rule is the architect's design but carries only an owner tag.** AD-29 is tagged "(amended 2026-11-24; owner: built by a story placed before epic 3 story 13)". The rule itself is the architect's: declared document in every step, last successful call is the version, no version per call. It also changes what feeds a version compared with 3.6's owner-approved Q5. **Fix:** tag it "(architect's rule; owner's placement)".

**L6. AD-15 Binds omit `am-api`.** The apply body's `confirmItemId` is a DTO field that goes through ts-rs (`xtask gen-types`), as 3.4's `ResolveBody` did. **Fix:** add "`am-api` (the apply body's `confirmItemId`)" to AD-15 Binds.

**L7. Nothing says what happens when the Assistant re-proposes a tainted-origin Proposal.** If the Assistant can read a tainted-origin Proposal's patches ("read runs"), it can re-propose them as case (2), approvable with the typed id. The owner can already do this through "Start my own edit from this", so this is not a new hole, but the seam should be stated. **Fix:** one sentence in AD-15. Either "an Assistant Proposal built after reading a tainted-origin Proposal is (2), never (1)", accepted as equivalent to Start my own edit, or deny the Assistant that read.

**L8. No story owns the CLI warning.** "the CLI … shows the same warning when it lists the item". 4.10 builds the CLI list before Assistant Proposals exist, and 8.5 is a Playwright story. **Fix:** name the CLI listing case in 8.5's verify in the "For Sofia" bullet.

## Checklist walk

| Check | Result |
|---|---|
| Fixes the real divergence points below: the new `write_document` story against 3.6's document step | **No (H2):** storage, resume and Draft payload are open. Otherwise yes: declared document in every step, one version per succeeded attempt, the step run's committed taint, 3.6 not reopened, and the refusal kept until the new story. |
| Fixes the divergence point: 4.17's classifier | **No (H1):** wrong owner definition; seat-change paths missed; snapshot input not stated. Removing `table:`/`document:` is not widening, owner-confirmed, and matches 4.17's existing verify. |
| Fixes the divergence point: 4.10's Proposal apply | **Partly (M2):** the creator class is missing from the shape, 4.10 is not routed, and its verify is citation-based. |
| Fixes the divergence point: 8.4 and 8.5 | **Mostly:** both routed. Gaps: M1 (check order), M3 (source list), M5 (UX), L1 (Run now?), L8 (CLI). |
| Every changed Rule enforceable and prevents its divergence | **Mostly.** AD-10's deferral is enforceable (status quo). Case (1) is enforceable once worded for every step run (H3); case (2) once the check order is right (M1). AD-34's engine-set taint is enforceable but needs persisting (L2). |
| Nothing deferred lets two units diverge | **Yes** for the Deferred row: tools stay `UntrustedData`. The Open item is fine as a default, but its alternative wording would cause divergence (M6). |
| Every owner answer covered faithfully | 1 yes. 2 recorded, but the definition is unmarked and wrong against 3.6 (H1). 3 yes, with the Deferred row. 4 yes: placement, plus the architect's rule (L5). 5 yes: the reading is stricter and flagged. "Without adding to #35": yes, separate branch. |
| No contradiction with AD-8, AD-10, AD-11, AD-15 "Who may decide", AD-21, AD-28, AD-29, AD-34, AD-35, AD-41, AD-43 | Contradictions found: AD-10 (b) and AD-46 against AD-15 (1)/(3) (H3); AD-21, AD-41 and 3.6 Q3 against the document-owner rule (H1); AD-36 and AD-8 against the `write_document` version home (H2); AD-13 and the UI-class check against "before any other check" (M1). None with AD-11, AD-28 or AD-43. "Who may decide" is consistent: UI class alone applies, the CLI Dismisses or opens, and the class rules stand. |
| The two Proposal rules sit together without gap or overlap | No overlap: the class is fixed at creation, "none is both", and the typed id never lifts (1). One gap: step runs that are not workflow steps (H3). One unstated seam: an Assistant re-proposing a tainted-origin Proposal (L7). |
| Binding constraint "a Proposal from a tainted workflow step is never approvable" kept | **Yes**, word for word, but the wider AD-10 (b) "tainted step" form is weakened in AD-15 (H3). |
