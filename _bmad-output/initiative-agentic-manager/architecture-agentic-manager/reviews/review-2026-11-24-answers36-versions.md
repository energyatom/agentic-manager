---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-24 owner-answers update for story 3.6 (AD-10, AD-15, AD-29, AD-34, Deferred row, Open Questions)
date: 2026-11-24
---

# Review: facts in the 2026-11-24 owner-answers update (story 3.6)

## Verdict

**Revise before merge: two High findings.** The update adds no library, crate or version, so there is nothing to check on the web. The check here is against the existing code, tickets and plans. Most claims are correct:

- the `team` field of a table;
- `write_document` as a local-write tool granted through `tools`;
- the 3.4 typed id with `confirmItemId` and 422 `confirm-required`;
- the CLI never applying a Proposal;
- epic 8 story 4's tainted-flag wording.

Two claims are wrong against the existing project, and both decide something without an owner question:

1. **Which team owns a document.** The spine says it is the declaring workflow's team. Story 3.6's approved plan says it is the declaring seat's team. Company A's F1 gives different owners under the two rules.
2. **The "Checked, no question" claim about write_document.** It says the tool's tainting result changes no Company A step. That is false for the golden run (3.13) as specified: there, F1's first step reads nothing untrusted before it calls `write_document`.

Four Medium and three Low findings follow.

## Method

- **Change:** read the word diff (`spine-answers36.diff`) and pulled each changed span with its context. Read the last 13 `.memlog.md` entries for the rationale.
- **Spine (worktree):** AD-10 (l.192), AD-11 (l.198), AD-15 (l.222), AD-21 (l.258), AD-29 (l.305), AD-33 (l.330), AD-34 (l.334, l.336), AD-35, AD-41 (l.406), AD-46 (l.438), the Deferred row (l.797), Open Questions (l.820–822) and the open item of 2026-11-23.
- **Code (worktree at main 9d2301e):**
  - `crates/am-config/src/entities.rs`: `Seat`, `Workflow`, `Step` and `Table`;
  - `crates/am-config/src/kinds.rs`: fan-out detection, l.2375;
  - `crates/am-domain/src/tool_manifest.rs`, l.94–107;
  - `crates/am-domain/src/inbox.rs`;
  - `crates/am-server/src/inbox.rs` and `envelope.rs`;
  - `crates/am-api` `ResolveBody`;
  - `crates/am-app/src/gate.rs`;
  - `crates/am-e2e/tests/trust/rule_15_inbox.rs`;
  - `docs/config-schema.md`.
- **Plans and tickets:**
  - story 3.4's plan;
  - story 3.6's approved plan, read only in the main checkout;
  - tickets 3.13 and 3.15;
  - tickets 4.9, 4.10, 4.17 and 4.18;
  - tickets 7.2 and 7.3;
  - tickets 8.1 to 8.7.
- **Sample:** `templates/samples/company-a/`: `company.yaml`, the four workflows and the ten seat files.
- **Not done:** no build or test was run, and no file was edited except this review.

## Findings

### High

#### H1. A document's owning team: the spine says the workflow's team; the approved 3.6 plan says the seat's team

**Claim.** The spine says "its team is that of the workflow whose step declares it, one team per id". The classifier's triggers are "a declaring workflow's `team` changed or the declaration moved into another team's workflow". It appears in AD-15 at l.222 and in the Resolved entry at l.820: "a document's team is that of the workflow declaring it". The memlog cites "epic 3 story 6 Q3" as the source.

**Evidence:**

- **3.6 plan, Q3 decision:** "a document id is kebab-case and owned by the declaring seat's team, one team per id" (plan l.471). The table rule matches: "a seat writes only the table its step declares, and only when that table's team is the seat's team" (plan l.59, l.286). `StepPlan` gains `seat_team` (plan l.166, l.658).
  - The plan is not consistent on this point either. Its matrix row 31 (l.125) says "one document id in two teams' workflows", so the 3.6 implementer has to choose one reading.
- **AD-41 (l.406):** a step's seat may belong to another department ("into a step of the same run (the step's seat belongs to another department …)").
- **AD-21 (l.258):** table writes are keyed to the seat's team ("Writes come only from that team's seats").
- **Company A, F1:** `workflows/f1.yaml` has `team: d2`. Its `consolidate` step runs on `d1-head`, whose seat file says `team: d1`, and writes `d1-opinion` with `write_document`.
  - The 3.6 plan's Q3 makes `d1-opinion` d1's document. The spine makes it d2's.
  - `company.yaml` l.16 says D2 "Owns Table T1, Document A and Report R1", which does not include `d1-opinion`.

**Why it matters.** Story 3.6's validate and story 4.17's classifier would disagree about who owns a document.

- **Under the seat rule** that 3.6 enforces, moving a declaring seat to another team changes which team owns the document. So does swapping the declaring step's seat for one from another team. The spine's classifier flags neither change.
- **Under the spine's rule,** it flags a workflow `team` edit, which leaves the seat-based owner unchanged.

**Fix.** Pick one rule and write it in both places before 3.6 merges.

- **Recommended: the seat rule.** It matches AD-21, the 3.6 plan's Q3, the plan's `seat_team` and the sample's description. Under it, a document's team is that of the seat whose step declares it. The classifier then flags three things:
  - a declaring step's seat swapped for a seat of another team;
  - a declaring seat's `team` changed;
  - a declaration moved to a step whose seat belongs to another team.

  In each case, compare each declared document's owner in the old and new snapshots.
- **Or keep the workflow rule.** Then send it to Sofia or the owner: 3.6's Q3 text, matrix row 31 and validate must change, and `d1-opinion` becomes D2's.

Either way, the "For Sofia" bullet at l.821 should name the rule that 4.17 uses.

#### H2. "Checked, no question" is false for the golden run: F1's `produce` step reads nothing untrusted before it calls `write_document`

**Claim** (l.822, memlog): "`write_document`'s tainting result changes no step of the Company A sample, since each step that calls it already reads rows, a document payload or folder files, which taint it first." The memlog cites f1 `produce` as a step that reads rows.

**Evidence:**

- **Ticket 3.13** (golden, the first user of the new story) runs "only table_write and write_document called". Until 4.8 and 4.9, the model sees only those two tools ("a test-only tool profile masks the sample's other capabilities"). So `produce` cannot call `table_read`.
- **The 3.6 plan** leaves `row_inputs` empty in the shipped `plan_of`: "3.15 and 7.2 fill it from config" (plan l.195, l.445).
- **3.15** fills `row_inputs` only from `rows:` hand-off payloads, and 7.2 from granted reads; 7.2 builds after 3.13. `produce` gets neither: F1 is started by hand or by a `workflow_completed` trigger, and the trigger is not a hand-off.
- **So in 3.13, `produce` starts clean.** Its `write_document` acknowledgement is the first `UntrustedData` in its context.
  - The step becomes tainted.
  - The rows its `table_write` then moves carry taint 1 at rest. Story 3.6's `EngineResult` exists to stop exactly that.
  - `document-a`'s version is marked tainted, and every later F1 step is tainted through the hand-off.
- **AD-33 (l.330) and 3.13** expect a clean baseline: their golden variants include "a tainted hand-off" and "a tainted row carried across the hand-offs". With a tainted baseline, those variants test nothing different.
- **The stated reason is also not settled.**
  - Ticket 3.15 fences a document hand-off payload only "when tainted".
  - The Open item of 2026-11-23 (2) says a read of an untainted document version "ha[s] no stated type".
  - So "a document payload … taints it first" does not hold for a clean payload.
  - `consolidate` and `gather` read branch outputs, not documents or folder files directly.

**Fix.** Remove "Checked, no question". Put this to the owner as a real, non-blocking question before the new `write_document` story's plan, with two options:

- (a) extend the `EngineResult` to `write_document` now, using the Deferred row's own condition ("Owner, before that tool's story");
- (b) accept a tainted F1 baseline. 3.13's snapshots and its tainted-row variant are then re-specified, and the scripted Owner gate types the id.

Also correct the reason: a clean document payload's type is still the 2026-11-23 open item.

### Medium

#### M1. "Stored once" for a `write_document` version does not fit 3.6's storage

**Claim** (AD-29, l.305): the Markdown of the last successful call becomes the attempt's version, and "the Markdown is stored once (AD-35, AD-43)".

**Evidence:**

- **3.6 Q6** (plan l.480): "a `document_versions` row, with the Markdown kept once on the producing attempt's output".
- **`DocumentVersionRecord`** (plan l.641) holds id, document, version, run, attempt, step run, taint and `bytes`. It has no text column and no pointer to a tool call.
- **With `write_document`,** the version text is the call's input, while the attempt's output is still the final message. 3.6's design has nowhere to keep it once.

**Fix.** In AD-29, or in the "For Sofia" bullet, say which change the new story makes:

- point the version at the call (for example `document_versions.call_seq` via a migration, with the text read from the journaled call input); or
- make the attempt's output the call's Markdown, and state what that does to `step.finished`'s `output_sha256` and to `payload: document:<id>` hand-offs.

Also say that the call's Markdown gets 3.6's checks: not empty after trimming, no NUL, at most 1 MiB (plan l.406).

#### M2. "Before any other check" is not what 3.4 does, and cannot literally hold

**Claim** (AD-15 (2), l.222): the server refuses an apply whose `confirmItemId` is wrong with 422 `confirm-required` "before any other check". The same section calls this "the same … typed-id confirmation as a tainted Review in 3.4".

**Evidence:**

- `crates/am-server/src/inbox.rs` l.166–206 runs its checks in this order:
  1. resolved;
  2. decision parse;
  3. later story;
  4. allowed decision;
  5. note required;
  6. the typed-id check, gated on the item's taint;
  7. then the waiting and pin checks.
- The check needs the item and its taint, so it cannot come before `not-found` or the item lookup.
- `envelope::confirm_required` (envelope.rs l.207–214) only has the Review wording ("This Review's output came from untrusted content …").

**Fix.** State the order: after `not-found`, `no-project` and `item-resolved`, and before the snapshot-hash (re-propose) check and the widening confirm-token check. Say that the Proposal apply route reuses the `confirm-required` code with its own message.

#### M3. Epic 8 story 4 cannot count something story 8.5 builds later

**Claim** (l.821): "epic 8 story 4's injection cases count zero Assistant Proposals applied without the typed id".

**Evidence:**

- In `epic-assistant-dashboards/tickets/tickets.toml`, 8.4 builds before 8.5 (8.5 has `after: [4, '4.10', '6.8']`).
- 8.5 is the story where "The Assistant emits Proposals on epic 4's shape" and that, per the same bullet, "adds the warning, the typed id and the `confirmItemId` check".
- When 8.4 builds, there are no Assistant Proposals and no typed-id check to count.

**Fix.** Put the zero-without-typed-id count in 8.5's verify, as an extension of 8.4's injection cases. Or record that 8.4's count turns on in 8.5.

#### M4. The Assistant conversation's taint must survive a restart

**Claim** (AD-34, l.336): each conversation has one taint, "only ever raised while the conversation lasts".

**Evidence.**

- Ticket 8.3: "Assistant history persists per project … after a restart the conversation is still in the panel."
- If the taint is not stored with that history, a restart resets a tainted conversation to clean, and its next Proposal skips the typed id.
- AD-34 already binds `am-store` but does not say the taint is stored.

**Fix.** Add: "stored with the conversation's persisted history and restored with it; never lowered".

### Low

#### L1. "Every other local-write tool" leaves out `ask_seat`

AD-10 (l.192) and the Deferred row (l.797) list six tools. The manifest also has `("ask_seat", LocalWrite, Local, 3)` (`tool_manifest.rs` l.107), and so does AD-11 (l.198). The 2026-11-23 item on a clean `ask_seat` answer stays open, which explains why it is left out, but "Every other" contradicts that.

**Fix.** Write "Each of these local-write tools' success results (…)", or add "(`ask_seat`'s answer: open, 2026-11-23)".

#### L2. Assistant "Run now?" cards are case (2), but no ticket asks for the typed id there

AD-15 (l.222) puts "a 'Run now?' card included" under Assistant Proposals (2). But Run on that card goes through the manual-start command (ticket 8.7), not 8.5's apply, and 8.7 has no typed-id step.

**Fix.** Name 8.7 in the "For Sofia" bullet, or state that a Run now? card is never case (2).

#### L3. The Reload and startup path of the new widening kind is not named for 4.18

The memlog says the owning-team change is widening "on every path (… Reload and startup) … held by 4.18". The "For Sofia" bullet (l.821) names only 4.17's five paths. For gate weakening, 4.18 has explicit Reload and startup cases.

**Fix.** Add a 4.18 verify case: an outside edit that changes a table's `team`, or a declared document's owner, is held for the two-step confirmation.

## Checked and correct

- **Table owner:** `Table.team` exists ("The one owning team", `entities.rs` l.401–405); its home is `tables/<id>.yaml`.
- **Team fields:** `Workflow.team` (l.282) and `Seat.team` (l.162) both exist.
- **Documents (AD-35):** documents and versions live in the database column, and there is no document config file.
- **`write_document`:** `LocalWrite` in the manifest (l.101). `docs/config-schema.md` l.130 lists it as a built-in tool of no capability, granted through a seat's `tools`. Company A's `d1-head`, `d2-head` and `d1-e1..e3` list it.
- **Join steps:** `consolidate` and `gather` are not fan-out under `kinds.rs` l.2376 (`seats`/`over_folder` only), so 3.6's "no `document:` on a fan-out step" does not block the sample update.
- **Typed id in 3.4:**
  - `confirmItemId` is in the wire body (`am-api` `ResolveBody.confirm_item_id`).
  - 422 `confirm-required` is in `envelope.rs` l.207–214.
  - The rule is applied in `am-server/src/inbox.rs` l.199–206 and tested in `rule_15_inbox.rs` l.651–691 (`a_tainted_approve_over_the_api_needs_the_item_id`).
  - 3.4's plan rows 27 and 27a cover the CLI warning and typed id.
- **CLI and Proposals:** the CLI never applies a Proposal. AD-15's "Who may decide" says only the UI class applies one. Ticket 4.10 says "the CLI lists and dismisses Proposals but never applies one", and `gate.rs` l.378 defers Proposal items to 4.10.
- **`agentic open --item <id>`:** already named in AD-15 and in `gate.rs` l.234 ("arrives with the app's UI"); planned, not built, and correctly cited as such.
- **Epic 8 tickets:**
  - 8.4 says "Proposals built while tainted content was in context carry the tainted flag and their source rows".
  - 8.1 and 8.4 say rows, documents and run tool results reach the Assistant as fenced `UntrustedData`.
  - The Open item correctly records the architect's reading of "built from tainted rows" as the owner's to confirm.
- **Other references:**
  - 4.17's verify lists the five paths named.
  - The 3.13 golden run calls `write_document`, so the new story must come before 3.13.
  - Story 3.6 refuses `write_document` (plan row 10).
  - `InboxKind::Draft` exists, and 3.6 writes one Draft item per version in the finishing transaction.
  - `RowWriter` and `EngineResult` are named as in the 3.6 plan.
