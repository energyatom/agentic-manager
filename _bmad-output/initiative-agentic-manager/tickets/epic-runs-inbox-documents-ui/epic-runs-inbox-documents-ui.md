---
type: epic
title: "Runs, Inbox and Documents UI (v0.2)"
parent: initiative-agentic-manager
covers: [R31, R32, R33, R34, R35, R36, R63, R73, R74, R75, R76, R77]
sources:
  - _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/party-decisions/decisions.md
  - _bmad-output/initiative-agentic-manager/reference-company-spec-2026-11-18.md
  - _bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md
companions:
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-workflow-run.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-inbox.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-states-keyboard.html
after: []
risk: high
---

# Runs, Inbox and Documents UI (v0.2)

## Description

The workflow editor as a vertical step list (seat, instruction, inputs, expected output, hand-off) with the schedule picker and workflow caps; the Runs list and the run view with live status, log, tokens, cost and duration per step and a drill-down showing labelled prompt parts, the response, fenced untrusted tool content, repairs and retries; run controls, with the engine side of pause and re-run from a step; the Inbox with every item kind, Approvals with their exact payload, Proposals with their diff, and the "While you were away" digest; Reviewer resolution in the UI; sanitised document pages versioned per run with Markdown and PDF export and citation links that resolve to rows and runs.

## Outcome

The owner watches, steers and reviews runs that cross departments in the browser — fan-out branches side by side, hand-offs and review gates in place — and reads Report R1 whose every action line cites its source and whose lineage opens from the Inbox and the document.

## Requirements

- R1 (parent R31; arch AD-36; ux Workflow editor, Hand-off): A workflow is edited as a vertical step list (number, title, seat, output, tools) with hand-offs restated in words between steps; a selected step's editor sets seat, instruction, inputs as named references, expected output or schema, and a hand-off of Next, Loop back (target, maximum default 2, reason required) or Human gate (which human seat); steps reorder by drag or ⌥↑/⌥↓ and are added at the end; each edit writes the workflow file as a config edit with Undo, an invalid edit shows its file:line fix, and the estimate under Run now recalculates within 300 ms.
- R2 (parent R31; ux Schedule picker, Budget field; epic-triggers-budgets-safety-floor R1): The editor reads the trigger back in the core's words and its picker sets daily, weekly-on-a-day or monthly presets, time, timezone (default: the company timezone, which the wizard pre-fills from the Mac's system locale), raw cron behind More and the catch-up policy (once, skip, all); the workflow's caps (scheduled and manual per run, per week, On breach) are edited beside it with 'An estimate never raises a cap.', raising a cap through the same confirmation as the seat sheet.
- R3 (parent R32; arch AD-8, AD-14; ux Run view, Runs list): The Runs list shows runs newest first by day with workflow, trigger (scheduled, manual, catch-up), status, duration and estimate against actual, filtered by team, workflow and status; the Run view's header shows trigger, start, elapsed and estimate with spend so far, and its step list shows each step's live status, time, tokens and cost from the WebSocket, replaying missed events after a reconnect; a seat card's status opens the run at that step; step changes are announced politely.
- R4 (parent R32; arch AD-10, AD-27, AD-29; ux Drill-down, Untrusted well): The drill-down (Log, Calls, Output) shows for each model call the prompt parts in assembly order with their labels, each tool call with secrets redacted, each result from web, file, MCP or free-text in table rows in an untrusted well as plain text (no links, images or HTML; Show all; the consequence line when an external-effect tool was held), the response, schema validation or 'Output repaired once', retries, fallback switches and the resumed-after-restart note.
- R5 (parent R33; arch AD-8, AD-15, AD-20; Decision 2026-11-18): The owner runs now (⌘R, estimate first, Return starts), pauses a run (a pause flag checked at step boundaries holds it before its next step, adding no run state) and resumes it, cancels a run, re-runs a failed step to continue its run, and re-runs from a step as a new run reusing the earlier steps' outputs; each is a Human command recorded in History or audit, and nothing is deleted.
- R6 (parent R35; arch AD-28; ux Inbox item, While you were away, Empty Inbox): The Inbox holds every item kind (Review, Approval, Draft, Question, Budget ask, Failure, Proposal) sorted by kind urgency then age and paginated by week; the selected item's primary button names its outcome and Return activates it only when the detail has focus; Questions take an answer (Split input or Use fallback included), Failures name what happened, what was tried and spent and a fix, Budget asks offer Raise once (two-step) or Leave stopped; a 'While you were away' digest opens after more than 30 minutes away or a restart and lists runs fired (catch-up included), items waiting, spend since last seen and failures; the sidebar count and tab title track open items; empty reads 'Nothing needs you.'
- R7 (parent R34; arch AD-15, AD-28, AD-36; ux Review gate; C4): A Review item shows the producing seat's output as a table or document with per-item include and return toggles; Approve, Edit, Return with note (note required, looped back to the producing seat and counted against max_returns) and Reject (confirmed) use the same Human command as `agentic gate`; the primary counts the outcome ('Approve 5, return 1') with the cost to continue; History and audit read 'You (Reviewer)'.
- R8 (parent R35; parent constraints Actions, Propose then approve; arch AD-10, AD-15; ux Approval payload, Proposal card, Privilege-widening confirmation): An Approval item shows tool, host, exact arguments, size and first record in a payload well, the untrusted consequence line when it applies, and Approve once or Deny with no always-allow; a Proposal item shows its diff, effect line and source references (tainted ones flagged) with Apply, Edit or Dismiss, privilege-widening ones flagged 'High risk · widens access' and applied only through the two-step confirmation; every apply or approve comes from a real pointer or key event and carries the confirm token where required.
- R9 (parent R36; arch AD-29; parent constraint AI never-do; ux Document): Documents render through the sanitising renderer (no raw HTML, scripts or remote images; links show their host) with a version picker per run, an estimate against actual header, am:// citations that open the cited row or run and flag any that does not resolve, Copy draft on each draft with 'Drafts only. Agentic Manager never sends email.', and Markdown and PDF export that works offline.
- R10 (parent constraints Release shape, Mockups gate UI; D10): v0.2 is tagged from main once UJ-1 runs end to end on FakeLlm through the browser and design QA against key-workflow-run and key-inbox passes in light, dark and keyboard-only.
- R11 (parent R63; ux EXPERIENCE Tasks; arch AD-14, AD-35): An AI seat's Agent card shows a read-only Tasks section, with its index entry, listing every workflow step the seat performs: step number and title, workflow, schedule in the core's words or Manual, and last run (status, when, cost) from a core read model over workflows and runs; Return or click opens that step in the workflow editor; a seat with none shows "D1-E1 has no tasks yet. Add it to a workflow step." with Open workflows; reassigning a step's seat in the editor updates both seats' Tasks live (added 2026-11-18, owner statement B).
- R12 (parent R73, R74, R75, R77 UI part; arch AD-40, AD-41, AD-42, AD-44; ux Workflow editor, Run view; mockup `mockups/key-workflow-run.html`; added 2026-11-18, owner REPLAN): The workflow editor shows a fan-out step as one card holding its seats side by side and a fan-in step with its join (all or quorum, timeout, on branch failure) in words; a cross-department hand-off shows as a step-list divider naming the receiving department, its payload and its acceptance, and a review gate as a step card naming the reviewer seat and department; adding a hand-off, an acceptance or a cross-department event trigger opens the two-step privilege-widening confirmation. The run view shows the branches of a fan-out in parallel lanes with per-branch status, tokens, cost and duration, the fan-in's wait ("Waiting for D1-E3 · 2 of 3 done · times out 09:30"), each hand-off crossing with its taint, and the reviewer's verdict and note; a chain started by a hand-off or trigger links to its parent and child runs.
- R13 (parent R76 UI part; arch AD-43; ux Inbox, Document; mockup `mockups/key-inbox.html`; added 2026-11-18): Every Inbox item and document version that carries an output has a Lineage view: the contribution chain as a vertical trace from the output back to its first inputs (seat and department, step, branch, model with local or frontier badge, fallback marked, inputs with untrusted ones marked, tokens, time, cost, review verdicts), each node opening its run drill-down, with totals per department and an Export (JSON, Markdown) that works offline; a removed node shows as "Removed".

## Constraints

- Untrusted content (parent; prd RULE-8; arch AD-10): untrusted text renders only as plain text in its well, never as instructions, links or markup.
- Propose, then approve (parent; prd RULE-3; arch AD-15, AD-28): every resolution, apply and run control is a Human command from a real UI event; no auto-approve or always-allow; Esc never cancels a run.
- AI never-do (parent): never render raw HTML, script or remote images from model output; never a single-number estimate.
- Accessibility, Look and feel, Mockups gate UI (parent; prd RULE-19; DESIGN.md; EXPERIENCE.md): keyboard-complete, WCAG 2.2 AA in both themes, polite live announcements, reduced motion; DESIGN tokens only; each UI story names the mockup it matches and the spines win on conflict. Definition of Done for every UI story (final review UX M3, 2026-11-18), whether or not its verify repeats it: axe clean in light and dark, its path completed keyboard-only, reduced motion honoured.
- Performance (parent; prd RULE-18): interactions under 100 ms are measured in this epic's end-to-end suite; owned by epic-polish-dry-run-learning.
- Generic engine (parent; prd RULE-2): product chrome says seat, team, workflow, run, table, document, Inbox; document, table and seat names come from template data only.
- Release shape (parent; D10; owner decision 2026-11-18): v0.2 is tagged here; the official four-week dogfood clock starts only after epic 9 on a frozen build, and v1.0 additionally requires at least the final 2 clean weeks on the v0.9-rc build.

## Done when

- On the Company A fixture, the workflow editor and run view show F1's fan-out lanes, fan-in wait, hand-offs and D1-Head's review as in `mockups/key-workflow-run.html`, and Report R1's Lineage opens from its Inbox item and the document as in `mockups/key-inbox.html`, matches the journal and exports.
- D1-E1's Agent card lists its workflow steps with schedule in words and last run, and Return opens the step in the workflow editor (owner statement B, 2026-11-18).
- Editing a step in the list writes the workflow file with Undo and the estimate updates; the schedule picker reads back in the core's words and caps edit beside it.
- A live Company A flow F1 run streams status, tokens and cost per step in the Run view and the Runs list; drill-down shows labelled prompt parts and fenced untrusted content; run now, pause, cancel, re-run step and re-run from a step work.
- The Reviewer gate appears in the Inbox; approve, edit, return with a note (looped back and counted against its maximum) and reject each record "You (Reviewer)" in History and audit; the digest opens after an absence and names catch-up runs; Approvals show their exact payload and Proposals their diff, with the two-step confirmation for widening ones.
- Report R1 renders sanitised, with no raw HTML, scripts or remote images; each action line's citation resolves, and the page exports to Markdown and PDF offline.
- Design QA against key-workflow-run and key-inbox passes in light, dark and keyboard-only; v0.2 is tagged from main with Playwright e2e and axe green.

## Boundaries

- Out: the table grid (epic-tables-grants-views); the Assistant and its "Run now?" card (epic-assistant-dashboards, reusing this epic's proposal card); Ask a seat threads (epic-polish-dry-run-learning); dry-run (epic-polish-dry-run-learning); notifications and digests by mail (v1.x).
- Out: schedules, caps, approvals and proposals as engine rules (epic-triggers-budgets-safety-floor); this epic renders and resolves them through epic 4's Human commands and adds only pause and re-run from a step to the engine.
- Touch point: the owner's mail client — receives drafts copied from the Inbox; the app never sends.
- Must not change: the Human-only resolve command shared with `agentic gate`; the shell, meter, estimate line and privilege-widening dialog of epic-org-seats-ui, which this epic reuses.

## References

- parent — _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md, Requirements R31, R32, R33, R34, R35, R36; Constraints
- architecture — _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md, AD-8, AD-10, AD-14, AD-15, AD-20, AD-27, AD-28, AD-29, AD-36, AD-37, Deferred (PDF export)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md (Step row, Hand-off, Untrusted well, Payload well, Proposal card, Inbox item, Document); _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md (Run view, Drill-down, Runs list, Workflow editor, Schedule picker, Inbox item, While you were away, Review gate, Approval payload, Proposal card, Document, State Patterns, UJ-1)
- mockups — _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-workflow-run.html §1–3, key-inbox.html §1–3, key-states-keyboard.html §1 (empty Inbox and Runs, step failed, budget stop, context overflow); key-assistant.html for the proposal card visuals
- party — _bmad-output/initiative-agentic-manager/party-decisions/decisions.md, D2 (estimate vs actual), D6, D13 (While you were away, Apple-like calm)
- reference company — _bmad-output/initiative-agentic-manager/reference-workload.md, flows F1 and F2

## Notes

- Waits on epic-org-seats-ui because: the app shell, sidebar, toolbar, ⌘K, design tokens, estimate line and privilege-widening dialog.
- Waits on epic-workflow-engine-cli because: the Human resolve command, journal, stored documents, cancellation and WebSocket events (1 on 3.2, 3 on 3.9 and 3.11, 7 on 3.4, 9 on 3.6).
- Waits on epic-triggers-budgets-safety-floor because: the schedule read-back, Budget asks, Approval and Proposal items and the confirm token (5 on 4.5, 6 on 4.3, 8 on 4.8 and 4.10).
- Decision: v0.2 is tagged at the end of this epic, but the official dogfood clock starts only after epic 9 on a frozen build, and v1.0 additionally requires at least the final 2 clean weeks on the v0.9-rc build (owner, 2026-11-18); the owner shadow-runs from v0.1.
- Decision: Refined (owner delegated, 2026-11-18): settled by the assumptions below; pause representation is an open question for the architect while the hold is assumed; PDF export is decided in story 9; no spike.
- Decision: Tracer bullet is story 1, the Runs list and live Run view fed by the WebSocket from a real FakeLlm run, opened from a seat card's status (2026-11-18).
- Decision: Sequencing (2026-11-18): drill-down second as the least certain part (untrusted rendering is a safety surface); run controls third because they add engine commands; the Inbox lane (list and digest, Review gate, Approvals and Proposals, documents) follows the run controls because a Failure's fix button uses Re-run step (found by the Dependencies check); the editor and schedule picker form a lane beside them; the refactor sweep, then the v0.2 end-to-end and design-QA suite.
- Decision: Pause (owner delegated, 2026-11-18): Pause holds the run at the next safe point through a pause flag checked at step boundaries; the running step finishes, no next step starts until Resume, and no state is added to the AD-8 machine; the architect confirms it in the next architecture update (initiative Notes); closes the pause open question and replaces the per-call hold assumed before.
- Assumption: Re-run step continues a run failed at that step in place (UJ-1 failure path); Re-run from step starts a new manual run linked to its source, reusing the source's outputs for earlier steps and pinning the current config snapshot.
- Decision: Proposal card (owner delegated, 2026-11-18): this epic builds the proposal card for Inbox Proposal items (story 8); epic 8 extends it, including the Assistant's 'Run now?' card (8.5 to 8.7), rather than a second component; the Proposal data shape and confirm token are epic 4's (4.10, 4.3) and the privilege-widening dialog is epic 5's (5.9).
- Assumption: PDF export is chosen inside story 9 between a server-side renderer and the webview's print, as arch Deferred allows, under the rule offline and sanitised; the choice is recorded as a Decision there.
- Assumption: The Ask a seat field at the Inbox foot is not built here (epic-polish-dry-run-learning); MCP re-approval items (epic 9) render through the Proposal and Approval details built here.
- Assumption: v0.2's UJ-1 failure path uses a FakeLlm-scripted tool failure in place of a blocked web search, because search arrives with epic 9.
- Decision: Dependencies (owner delegated, 2026-11-18): the initiative now declares epics 3 and 4 before this epic, and the entries pin the stories they need; closes the dependency open question.
- Decision: REPLAN consistency pass (2026-11-18): epics 7, 8 and 9 now pin this epic's stories 6.1, 6.2, 6.8 and 6.9 instead of the whole epic.
- Decision: Final review UX M3 and M4 (owner delegated, 2026-11-18): accessibility is a Definition of Done line for every story here; the schedule picker (story 5) matches the new mock ux-agentic-manager/mockups/key-schedule-updates.html §1. Requirements unchanged.
- Decision: Seat anatomy (owner statement B, 2026-11-18; initiative Notes): R11 added (parent R63); new story 12, "Seat Tasks on the Agent card", after story 4 (workflow editor) and epic 5 story 17 (Agent card layout); story 10 also waits on story 12. Matches key-seat-sheet.html §1 (Tasks).
- Decision: Generic engine principle (owner, 2026-11-18): R4 and story 2 speak of free-text in table rows; no scope change.
- Decision: Owner REPLAN (owner, 2026-11-18, reference-company spec and Amendment 2; initiative Notes): R12 and R13 added (parent R73 to R77 UI part); new stories 13 "Fan-out, hand-offs and review gates in the editor and run view" (`mockups/key-workflow-run.html`) and 14 "Lineage from the Inbox and Documents" (`mockups/key-inbox.html`) build before the sweep; Sally updates both mockups under the same filenames; story 5 gains event triggers; story 9 is retitled "Documents with citations and export"; UJ-1 in the closing story is the Company A journey; every fixture seat is a Company A seat.
- Decision: Final review 2 (owner delegated, 2026-11-19; initiative Notes): story 7 resolves Reviews of tainted or cross-department payloads only in the UI (arch AD-15); story 13's human→AI reviewer swap opens the widening confirmation; schedule picker and Workflows default to the company timezone from the system locale.
