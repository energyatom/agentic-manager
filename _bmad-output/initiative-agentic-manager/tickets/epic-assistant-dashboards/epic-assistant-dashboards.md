---
type: epic
title: "Assistant and dashboards"
parent: initiative-agentic-manager
covers: [R43, R44, R45]
sources:
  - _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/party-decisions/decisions.md
companions:
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-assistant.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-tables-dashboards.html
after: []
risk: high
---

# Assistant and dashboards

## Description

The ⌘J Assistant on the jailed read-only connection (authorizer, row and time caps, no egress, no MCP) that shows its query, cites rows and runs, explains runs and says when it is unsure; proposal cards (Apply, Edit, Dismiss) on epic 4's one Proposal shape, extending epic 6's Inbox proposal card, applied only by a real UI event, with privilege-widening cards stating their effect and asking twice, tainted sources flagged and stale cards refused; Run now as a proposal with its estimate; pinned charts, tables and KPIs on dashboards that store their query and refresh on open. It queries only the `v_*` views from epic-tables-grants-views.

## Outcome

The owner asks his company questions in plain words and changes it only by approving what the Assistant proposes.

## Requirements

- R1 (parent R43; arch AD-6, AD-34; D6): The Assistant answers from a dedicated read-only connection whose authorizer denies writes, ATTACH, PRAGMA writes and `load_extension`, with a row cap and a time limit enforced by the reader and a progress handler; its only tools query `v_*` views and read runs, documents and config; it has no egress tool and no MCP; a breach is refused with the reason in words.
- R2 (parent R43; arch AD-29, AD-34; ux Assistant panel): The ⌘J slide-over streams each answer as sanitised Markdown, shows its query collapsed and marked read-only, cites rows and runs as chips that open the source and flags a citation that does not resolve, explains a run from its journal, says so when its confidence is low, and keeps its history per project.
- R3 (parent R43; arch AD-10; prd RULE-8; rhea D6): Untrusted content the Assistant reads (free-text in table rows, documents, tool results in runs; generalised 2026-11-18) reaches its model only as fenced UntrustedData; a Proposal built on tainted rows says so on its card. Threshold: the injection pack run through the Assistant yields zero outbound calls and zero approvable proposals.
- R4 (parent R44; arch AD-15; ux Proposal card): The Assistant's config changes arrive as Proposal cards on the one Proposal shape (typed config patches, effect line, source references, snapshot hash) showing the file diff; Apply runs the Human apply command only from a trusted pointer or key event inside the card and adds a History entry with Undo; Edit opens the affected sheet with the change prefilled; Dismiss discards; a card built on a since-changed file is refused with an offer to re-propose.
- R5 (parent R44; arch AD-15; ux Privilege-widening confirmation; D6): A privilege-widening proposal (new tool, MCP server, bigger budget, new egress host, grant) is flagged "High risk · widens access" with a plain effect sentence, its primary is "Review and apply…", and it applies only after the second confirmation carrying the two-minute confirm token, with "Keep current access" the default.
- R6 (parent R44; D6; ux Estimate line, Cost & Estimate Display): "Run now?" arrives as a Proposal card showing the estimate line with its basis and the remaining budget; the run starts only from Run on the card, with the Human actor.
- R7 (parent R45; arch AD-34, AD-35; ux Dashboard card): Pin on an Assistant chart, table or KPI writes a card with its SQL and layout to `dashboards/<id>.yaml` through the config writer (History, Undo); cards are created only by Pin.
- R8 (parent R45; arch AD-29, AD-34, Deferred dashboard chart library; ux Dashboard card, Stale dashboard query): Dashboards (⌘6) refresh every card on open through the read-only path and show the refresh time; a card offers Query (read-only), Ask about this (which sends the card's query and its already-visible result figures, nothing else, to the Assistant's configured model), Rename and Remove, reorders by drag or ⌥←/⌥→, renders charts with no remote content, and shows "This query failed: … Ask the Assistant to fix it." when its query breaks.

## Constraints

- Propose, then approve (parent; prd RULE-3; arch AD-15): the Assistant has no tool that reaches the local API; nothing it outputs applies, runs or widens anything; no auto-approve.
- AI never-do (parent; prd AI behavior and evaluation): never apply its own change, obey untrusted content, render raw HTML, script or remote images, or give a single-number estimate.
- AI fallback (parent; prd AI behavior and evaluation): a low-confidence Assistant says so.
- Untrusted content (parent; prd RULE-8; arch AD-10): owned for the Assistant path; the injection pack gains Assistant cases.
- Local and private (parent; prd RULE-5; arch AD-12): the Assistant's model calls go only to its configured Account through the egress client.
- Accessibility (parent; prd RULE-19; ux Accessibility Floor): the slide-over traps focus while open, citations and cards are keyboard-reachable, and Return applies only when the card has focus. Definition of Done for every UI story (final review UX M3, 2026-11-18), whether or not its verify repeats it: axe clean in light and dark, its path completed keyboard-only, reduced motion honoured.
- Mockups gate UI (parent): the panel, proposal cards and Run now? card match `key-assistant.html`; dashboards match `key-tables-dashboards.html` §3.

## Done when

- A question such as Table T1 rows by status this month returns an answer with its SQL over the `v_*` views and citations that resolve; a write, ATTACH or a breach of the row or time cap is refused.
- An Assistant config change arrives as a diff card that applies only from a real click or key press in the card; a privilege-widening card needs the second confirmation; a card built on a since-changed file is refused with an offer to re-propose; a Run now? card shows its estimate and starts only from Run.
- A pinned chart lands on a dashboard file that stores its query and refreshes through the read-only path when opened; a broken query shows the stale-query state.
- The injection pack run through the Assistant yields zero outbound calls and zero approvable proposals.
- Design QA against the Assistant and dashboards mockups passes; merged with Playwright e2e and `just check` green.

## Boundaries

- Out: outbound tools for the Assistant (never); dashboard export (v1.x); a hand-built BI editor (non-goal); learning-loop and Developer-hat proposals (epic-polish-dry-run-learning, which reuses the cards built here).
- Out: the Proposal shape, its apply command and confirm token (epic-triggers-budgets-safety-floor); the base proposal card for Inbox items (epic-runs-inbox-documents-ui, extended here); the privilege-widening dialog (epic-org-seats-ui); the estimate line and manual start (epic-org-seats-ui, epic-triggers-budgets-safety-floor); the `v_*` views (epic-tables-grants-views).
- May slip to v1.x without blocking the dogfood: R45 dashboards, in its simplified form (parent Boundaries; prd OQ-5).
- Must not change: Inbox items stay resolvable only by a Human command (arch AD-28).

## References

- parent — _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md, Requirements R43, R44, R45; Constraints (Propose then approve, AI never-do, AI fallback, Untrusted content, Local and private, Accessibility, Mockups gate UI)
- architecture — _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md, AD-6, AD-10, AD-11, AD-15, AD-29, AD-34, AD-35, Deferred (dashboard chart library)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md, Component Patterns (Assistant panel, Proposal card, Privilege-widening confirmation, Dashboard card, Estimate line), State Patterns (Empty Dashboards, Stale dashboard query), Cost & Estimate Display, Accessibility Floor
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md, Components (proposal-card, proposal-card-risk, untrusted-well)
- mockups — _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-assistant.html; key-tables-dashboards.html §3
- party — _bmad-output/initiative-agentic-manager/party-decisions/decisions.md, D6 (Assistant powers) and D11 (red-team pack)

## Notes

- Waits on epic-tables-grants-views because: the per-table read-only `v_*` views (story 7.5).
- Waits on epic-triggers-budgets-safety-floor because: the one Proposal shape and its Human apply command with the confirm token (stories 4.3, 4.10), the Assistant spend scope (4.2) and the injection pack (4.9).
- Waits on epic-org-seats-ui because: the privilege-widening dialog story 6 opens (5.9).
- Waits on epic-runs-inbox-documents-ui because: the Inbox proposal card component stories 5 to 7 extend (6.8).
- Decision: Refined (owner delegated, 2026-11-18; owner decision taken as given): the Assistant is propose-then-approve on a read-only database handle; dashboards stay in v1. Settled by the assumptions below; no spike, since AD-6 fixes the jail; the chart library (arch Deferred) is chosen in story 8.
- Decision: Tracer bullet is story 1, a question answered with its query through the jailed reader, the Assistant module, the event stream and the ⌘J panel (2026-11-18).
- Decision: Sequencing (2026-11-18): the jail's refusals come second as the least certain and highest-risk part; citations and run explanations, then the injection cases, before any card can apply; the card lane (cards, privilege-widening, Run now?) shares the card component; dashboards close the feature work; then the refactor sweep and a closing end-to-end suite with design QA.
- Assumption: The Assistant uses the company default model unless Settings names another.
- Decision: Assistant spend (owner delegated, 2026-11-18): the Assistant's calls are reserved and settled in the ledger as Assistant spend and count against the company weekly cap, through epic 4's scope (4.2).
- Assumption: The Assistant's four read tools pass through ToolGate under a fixed read-only grant set with no egress, so the tool choke point stays single (arch AD-11).
- Assumption: Row cap 1,000 rows and time limit 5 s per query by default, editable in `company.yaml`.
- Assumption: A trusted UI event is a pointer or key event the browser marks trusted, inside the card, after the card has rendered for at least 500 ms; synthetic or model-originated events never apply.
- Decision: Proposal card (owner delegated, 2026-11-18): epic 6 builds the card for Inbox Proposal items (6.8); this epic extends it with the Assistant's cards, privilege-widening flagging and Run now? (stories 5 to 7); the data shape is epic 4's and the dialog epic 5's, carrying epic 4's confirm token.
- Assumption: Dashboard cards are charts (bar, line), tables and single-figure KPIs only; the chart library is chosen in story 8 against no remote content and the DESIGN tokens.
- Decision: Ask about this (owner delegated, 2026-11-18): it sends the query and the card's already-visible result figures to the Assistant's configured model, nothing else (R8, story 9); closes the open question.
- Decision: Injection pack (owner delegated, 2026-11-18): story 4 extends epic 4's injection pack (4.9) with Assistant cases; there is one pack.
- Decision: Dependencies (2026-11-18): the initiative now declares epics 5 and 6 before this epic, following the proposal-card decision; stories 5 and 6 pin 6.8 and 5.9.
- Decision: Final review F7b (owner delegated, 2026-11-18): story 1 writes the first jail (write refusals, no raw tables) and fence tests to fail first, so the Assistant never reads live rows unguarded; stories 2 and 4 complete the authorizer and the injection cases. Requirements unchanged.
- Decision: Generic engine principle (owner, 2026-11-18): R3 speaks of free-text in table rows; no scope change.
- Decision: Owner REPLAN (owner, 2026-11-18, reference-company spec and Amendment 2; initiative Notes): every test question, proposal, dashboard and seat in this epic uses the Company A fixture; Assistant answers may cite lineage nodes (arch AD-43) as they cite runs; no requirement, entry or order changes.
- Decision: Mac first (owner, 2026-11-19; initiative Notes): the closing end-to-end suite runs on the macOS arm64 runner only; Windows and Linux runners are v1.x (arch AD-47). No other change.
- Decision: Final review 2 security M6 and H3 (owner delegated, 2026-11-19): story 2's jail is an allow-list authorizer with SQLite limits (sqlite_master and knowledge_fts denied); story 6 flags every widening kind of epic 4 story 17's classifier.
