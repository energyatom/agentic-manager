---
type: epic
title: "Polish, dry-run, learning loop and design/a11y QA"
parent: initiative-agentic-manager
covers: [R50, R51, R52, R53]
sources:
  - _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/reference-workload.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md
  - _bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/party-decisions/decisions.md
companions:
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/reference-workload.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-workflow-run.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-inbox.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-assistant.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-states-keyboard.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-seat-sheet.html
after: []
risk: medium
---

# Polish, dry-run, learning loop and design/a11y QA

## Description

Dry-run as a run mode whose rows, documents and Inbox items are labelled and kept out of views and history; learning proposals from the Coordinator template's learning loop citing their source rows and flagged when tainted, with owner-editable memory notes; Developer-hat approvals in its subtree; Ask a seat as an Inbox thread to any seat; and the app-wide performance, accessibility and design QA pass against all thirteen mockups. Its fixes land after the frozen dogfood build of epic-tools-mcp-search-doctor and reach the owner in the v0.9-rc build.

## Outcome

The owner rehearses a run for free, lets the department learn from outcomes under the owner's approval, and works in a calm, accessible, fast app.

## Requirements

- R1 (parent R52; arch AD-37; ux Dry-run, Estimate line; D13; Decision 2026-11-18): A dry-run's `mode` is fixed at start; it substitutes every seat's model with the cheap or local model chosen at start (by default the cheapest available local model, or, when no local model exists, the cheapest model across the company's Accounts by the prices file, named with its estimate), shows its own estimate line before start, and spends against the same caps.
- R2 (parent R52; arch AD-37): Rows, row history, documents and Inbox items a dry-run creates carry the dry-run flag and are labelled "Dry run" everywhere; `v_*` views, saved views, dashboards and the estimator's history exclude them unless explicitly asked; a dry-run never raises an external-effect approval and records the call as "would have asked".
- R3 (parent R50; arch AD-15, AD-10; D11 #11): On a schedule the owner sets, a seat built from the Coordinator role template (the learning loop's seat) reads outcome reasons recorded since its last proposal and proposes memory-note or instruction changes as Proposals that cite their sources (gate returns and rejections, status reasons); a Proposal built on tainted rows says so; nothing changes until the owner applies it, and the apply is recorded in History.
- R4 (parent R50; ux Memory notes, Seat sheet): Memory notes stay owner-editable in the seat sheet; a note added by an applied proposal shows the proposal and its source rows, and the owner can edit or delete it like any other note.
- R5 (parent R51; arch AD-15, AD-24; D13; ux Human seat card): A Developer hat on a human seat approves Proposals for tool, model and prompt changes to seats in its subtree; the apply records "You (Developer hat)" in History; a Proposal touching a seat outside the subtree cannot be applied under that hat; the Developer card links to pending proposals in its subtree.
- R6 (parent R53; arch AD-28; ux Ask a seat; D13; restated 2026-11-18, reconcile): A one-line "Ask a seat" field at the foot of the Inbox list creates a Question thread addressed to the seat the owner picks (any seat; by default the top Coordinator seat, else the top head seat, and for a selected item from a department that department's head seat) — formerly the Chief of Staff seat; the seat answers in a one-step run under its caps, its reply arrives in the thread and may cite rows or runs, and the owner can follow up in the same thread; messaging other seats directly is v1.x.
- R7 (parent constraint Performance; prd RULE-18): On a 30-seat fixture, interactions respond under 100 ms, the org chart holds 60 fps, idle memory stays under 300 MB without models and cold start is under 3 s, each measured by a CI test written to fail first.
- R8 (parent constraint Accessibility; prd RULE-19; ux Accessibility Floor): Every surface is keyboard-complete (every drag has a key; single-key shortcuts can be turned off), meets WCAG 2.2 AA contrast in light and dark, carries screen-reader labels (the org chart as an ARIA tree, live regions as specified) and honours reduced motion, each checked by a CI test written to fail first.
- R9 (parent constraints Mockups gate UI, Look and feel; ux Foundation): Design QA of every surface against all thirteen mockups and the spine-only surfaces passes in light and dark, with each difference fixed or recorded as a decision.

## Constraints

- Performance (parent; prd RULE-18): owned here; the `rule_18` tests land in `crates/am-e2e/tests/trust/` and the web performance harness.
- Accessibility (parent; prd RULE-19): audited here as the `rule_19` tests across every UI epic's surfaces. Definition of Done for every UI story (final review UX M3, 2026-11-18), whether or not its verify repeats it: axe clean in light and dark, its path completed keyboard-only, reduced motion honoured.
- Propose, then approve (parent; prd RULE-3; arch AD-15): learning, Developer-hat and Assistant proposals use the one Proposal shape and apply only from a real UI event; no auto-approve.
- Untrusted content (parent; prd RULE-8; arch AD-10): outcome reasons edited by people or imported are untrusted data to the learning seat; a learning proposal is a persistent-injection vector and always shows its source rows.
- Generic engine (parent; prd RULE-2): the learning workflow and its schedule ship with the Coordinator role template; the engine knows only seats, proposals and reasons.
- Mockups gate UI (parent): the dry-run control matches `key-workflow-run.html` §1; Ask a seat matches `key-inbox.html`; learning proposals match `key-assistant.html` proposal cards.

## Done when

- A dry-run substitutes the chosen cheap or local model in every seat, shows its own estimate, labels every row, document and Inbox item "Dry run", raises no approval, and leaves views, saved views, dashboards and estimator history unchanged.
- On its schedule the Coordinator seat's learning loop proposes a memory-note or instruction change citing the return and rejection reasons; applying it is recorded in History; the owner edits a memory note directly.
- A Developer-hat proposal for a tool, model or prompt change in its subtree applies only under that hat and shows "You (Developer hat)" in History; an Ask a seat question to the default seat and one to a picked seat each become an Inbox thread answered by that seat.
- The RULE-18 budgets and the RULE-19 audit pass with failing-first tests, and design QA passes against all thirteen mockups; merged with Playwright e2e and `just check` green.

## Boundaries

- Out: messaging any seat directly, knowledge base, health overlay (v1.x).
- Out: the Proposal shape and apply command (epic-triggers-budgets-safety-floor); proposal cards (epic-assistant-dashboards; reused here); the `v_*` views (epic-tables-grants-views; extended here); the performance budgets' measurement before this epic is informal (arch Deferred).
- May slip to v1.x without blocking the dogfood: R51, R52 and R53, each in its simplified form (parent Boundaries; prd OQ-5).
- Must not change: the frozen dogfood build; this epic's changes reach the owner in the v0.9-rc build (epic-desktop-release-trust-gate).

## References

- parent — _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md, Requirements R50, R51, R52, R53; Constraints (Performance, Accessibility, Propose then approve, Untrusted content, Generic engine, Mockups gate UI, Look and feel)
- architecture — _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md, AD-9 (estimator history), AD-10, AD-15, AD-24, AD-27, AD-28, AD-35, AD-37, Deferred (performance budgets)
- reference workload — _bmad-output/initiative-agentic-manager/reference-workload.md, Parameters (Learning)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md, Component Patterns (Dry-run, Memory notes, Human seat card, Ask a seat, Proposal card), UJ-3, Accessibility Floor, Interaction Primitives, State Patterns
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md (tokens, contrast pairs, motion)
- mockups — _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/ (all nine; key-workflow-run.html §1 for dry-run, key-inbox.html for Ask a seat)

## Notes

- Waits on epic-assistant-dashboards because: proposal cards in the UI.
- Waits on epic-tables-grants-views because: outcome reasons in rows and the `v_*` views the dry-run must stay out of.
- Waits on epic-tools-mcp-search-doctor because: the full tool set the dry-run and the QA pass cover, and the frozen dogfood build this epic's fixes follow.
- Decision: Refined (owner delegated, 2026-11-18; owner decision taken as given): the Developer hat, dry-run and Ask a seat stay in v1 in their simplified forms; settled by the assumptions below; no spike.
- Decision: Tracer bullet is story 1, a dry-run started from the workflow editor with its own estimate, through run mode, model substitution, the run journal and the Runs list label (2026-11-18).
- Decision: Sequencing (2026-11-18): the learning loop comes second as the least certain part (a persistent-injection vector); dry-run data separation third because it extends the store's views; memory notes, the Developer hat and Ask a seat follow; performance, accessibility and design QA run last over the finished surfaces; then the refactor sweep and a closing end-to-end suite.
- Source conflict: R2 — EXPERIENCE.md Dry-run says dry-run outputs "never write to tables except a scratch copy"; arch AD-37 (final) writes them into the same tables flagged dry-run and excludes them from views, saved views, dashboards and the estimator.
- Decision: AD-37 governs R2 (owner delegated, 2026-11-18): dry-run rows are written to the real tables, flagged, and excluded from views, saved views, dashboards and the estimator; Sally aligns EXPERIENCE.md's Dry-run row (initiative Notes); the assumption is confirmed.
- Assumption: The learning loop is a workflow shipped with the Coordinator role template, its schedule paused until the owner sets one; in the Company A fixture tests, D2-Head is given the Coordinator template's learning workflow so the sample keeps abstract names (owner Amendment 2, 2026-11-18).
- Decision: `propose_change` (owner delegated, 2026-11-18) is registered as a local-write tool in epic 4's tool classes (4.8); story 2 builds it: it only raises a Proposal Inbox item and never applies.
- Decision: Injection pack (owner delegated, 2026-11-18): story 2 extends epic 4's injection pack (4.9) with poisoned-reason cases; there is one pack.
- Decision: Proposal card (owner delegated, 2026-11-18): learning and Developer-hat proposals render on epic 6's Inbox proposal card as extended by epic 8.
- Assumption: A Developer hat's subtree is the seats under the human seat's position in the org tree; with no Developer hat in the tree, the owner applies as before.
- Assumption: Ask a seat threads are Question items with replies; each reply is a one-step run with no tools beyond read access to the seat's own tables and grants, attributed to the addressed seat in the ledger.
- Assumption: Performance is measured on the owner's Apple Silicon Mac class in CI runners where possible, with the 60 fps and memory checks run on the macOS runner and recorded per release.
- Decision: Dry-run model (owner delegated, 2026-11-18): when no local model exists, a dry-run uses the cheapest model across the company's Accounts by the prices file, shown with its estimate (R1, story 1); closes the open question.
- Cross-epic: story 3 extends epic-tables-grants-views's `v_*` views and epic-org-seats-ui's estimator to exclude dry-run data; the a11y and design fixes touch every UI epic's surfaces.
- Decision: Final review UX M3 (owner delegated, 2026-11-18): accessibility is a Definition of Done line for every UI story in every UI epic; this epic's rule_19 audit stays as the cross-surface check. Requirements unchanged.
- Decision: Owner REPLAN (owner, 2026-11-18, reference-company spec and Amendment 2; initiative Notes): R3 restated (instruction or memory-note changes from gate returns and status reasons); the learning workflow ships with the Coordinator role template (renamed from Chief of Staff on 2026-11-18, reconcile); every fixture uses Company A, where D2-Head holds that learning workflow in tests; the dry-run and end-to-end stories run flow F1; no entry id or order changes.
- Decision: Reconcile (owner delegated, 2026-11-18; initiative Notes): "Chief of Staff" is not a role template; the learning loop ships with the Coordinator template and the ad-hoc question feature is "Ask a seat", to any seat the owner picks (default the top Coordinator seat, else the top head seat); R3 and R6 restated; story 6 reuses epic 9 story 18's answer run and now waits on it.
- Decision: Mac first (owner, 2026-11-19; initiative Notes): the closing end-to-end suite runs on the macOS arm64 runner only; Windows and Linux runners are v1.x (arch AD-47). No other change.
- Decision: Final review 2 (owner delegated, 2026-11-19): story 2's tainted learning Proposals are never approvable; story 3's dry-run events start no real run; story 9 is 'Design QA against all thirteen mockups' (every file in mockups/).
- Decision: Quality plan (owner, 2026-11-24, final; verbatim in epic 3's Notes): before this epic closes, its refactor sweep (story 10) has the architect run his read-only Validate pass, comparing the architecture spine with the built code. Each drift goes into `docs/deferred-register.md`, the one register of deferred items and accepted risks (seeded 2026-11-24), and anything that would change an AD goes to the owner. The sweep's id, after, covers, hitl, risk and place in build order are kept.
