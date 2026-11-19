---
type: epic
title: "Tables, grants and saved views"
parent: initiative-agentic-manager
covers: [R37, R39, R40, R41, R42]
sources:
  - _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/reference-workload.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/party-decisions/decisions.md
companions:
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/reference-workload.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-tables-dashboards.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-org.html
after: []
risk: medium
---

# Tables, grants and saved views

## Description

The grid with sort, filter, search, inline edit, comments and row history; template-defined status transitions whose reasons the store command enforces; CSV and xlsx export and import with a diff through the capped import path; saved views, one of which a table may name as its default; per-table read-only `v_*` views regenerated on config apply, which epic-assistant-dashboards queries; read-only grants on tables and documents, drawn as dotted lines and read as of run start; the Team sheet with Add grant as privilege-widening; and the Company A fixture that proves a grant between departments through generic config. Rows, `table_write` and row history with provenance come from epic-workflow-engine-cli; the app shell and the privilege-widening dialog from epic-org-seats-ui; documents from epic-runs-inbox-documents-ui.

## Outcome

The owner, and people he shares spreadsheets with, track every row's status with its reason in the app or a spreadsheet, and departments read each other's data through grants without gaining write access.

## Requirements

- R1 (parent R37; arch AD-21, AD-24; ux Table grid, Provenance): A table opens in a grid with sticky header, sort by header, a filter field, search and inline edit (double-click or Return) that writes with the Human actor shown as "You"; each row's last-changed-by cell shows a seat avatar and run number, "You" or the import source; the grid stays virtualised beyond 200 rows and an empty table shows ghost rows from its schema with Import a file.
- R2 (parent R37; ux Table grid, Provenance): Row detail shows the row's full history (status, actor, run, step, reason, source with untrusted sources marked) and its comments; a comment is stored with its Human actor and time and is not a row change.
- R3 (parent R39; arch AD-21; reference-workload Parameters): A template-defined status field allows only its declared transitions, each with a non-empty reason, refused otherwise by the store command for Seat, Human and Import actors alike with the error in words; the Company A sample's Table T1 declares new → processed → handed-off or dismissed; in the grid the status menu asks for a required one-line reason.
- R4 (parent R40; arch AD-21, AD-17; D5): Export writes the current view as CSV or xlsx, including the status and reason columns and a row id column, after a secret scan that refuses to write a file containing a key.
- R5 (parent R40; arch AD-21, AD-30; ux Import / Export, Flow 6): Import reads a CSV or xlsx file through the capped extract child and WorkspacePath checks, matches rows by row id or the table's declared key column, and shows an Import diff (status changes, reasons added, new rows, rows with a status change but no reason kept and listed, unmatched rows with a column-mapping step); nothing changes until Apply, which writes through the same store command with the Import actor; a CSV or xlsx exported and edited outside round-trips status and reason columns.
- R6 (parent R41; arch AD-16, AD-35; ux Saved view tab): Saved views store filter, sort and columns in the table's config file through the config writer, with History and Undo; tabs above the grid switch views, "+ View" saves the current state, and a table's config may name one saved view as its default.
- R7 (parent R42; arch AD-22; D1): A grant in config gives one team read-only access to another team's table or document; a run records its `start_seq` and reads granted tables as of it and granted documents at the version current at run start; a grant never enables `table_write`; adding a grant is privilege-widening and applies only through the two-step confirmation.
- R8 (parent R42; ux Grant line, Team box, Team sheet, Accessibility Floor): Grant lines are hidden at rest and drawn dotted between the two team boxes on hover or selection, labelled with what is shared and "read-only"; clicking the label opens Team sheet › Grants; the Team sheet shows head seat, seats, workflows with schedule and Run now with its estimate, tables and documents owned, grants, guidance notes, team budget and Export department, and lists grants for screen readers.
- R9 (arch AD-21, AD-6, AD-34; parent R43, R45 hand-off): A read-only SQL view `v_<table-id>` per config table, with the table's fields as columns, is regenerated on every config apply and is the only table surface the Assistant and dashboards query; a write through a view is refused.
- R10 (parent constraint Generic engine; prd RULE-2; arch AD-23; D1; restated 2026-11-18, owner REPLAN): The Company A fixture under `fixtures/` runs in CI on FakeLlm: D1's workflow reads D2's Table T1 through its grant as of its run start and writes its own document; D1's attempted `table_write` to Table T1 is refused; the department-noun denylist scan over `crates/` and `web/src` stays green.

## Constraints

- Generic engine (parent; prd RULE-2; arch AD-23): completed here by R10; table, status and view behaviour comes only from table config, never from a department-specific code path.
- Propose, then approve (parent; prd RULE-3; arch AD-15, AD-22): an import applies only on the owner's Apply; Add grant goes through the privilege-widening confirmation.
- Files (parent; prd RULE-14; arch AD-30): CSV and xlsx imports go through the same size and time caps and WorkspacePath checks as drop files.
- Secrets (parent; prd RULE-12; arch AD-17): table exports are secret-scanned.
- Personal data (parent; prd Data and compliance): exports are files the owner sends himself; the app sends nothing.
- Accessibility (parent; prd RULE-19): the grid, status menu, saved-view tabs, import diff and Team sheet are keyboard-complete with screen-reader labels; grant lines are announced in the Team sheet's Grants list. Definition of Done for every UI story (final review UX M3, 2026-11-18), whether or not its verify repeats it: axe clean in light and dark, its path completed keyboard-only, reduced motion honoured.
- Mockups gate UI (parent; ux EXPERIENCE Foundation; Decision 2026-11-18): the grid, row detail, saved views, import diff and grant pill match `key-tables-dashboards.html` §1–2; the grant line matches `key-org.html`; the Team sheet matches `key-team-sheet.html`.

## Done when

- A table opens in its default saved view (Table T1 on the Company A fixture); sort, filter, search, inline edit and comments work, and row history shows run, step and seat, "You" or the import source.
- A status change without a reason, or to an undeclared transition, is refused by the store command for seats, people and imports alike.
- An exported xlsx with edited status and reason columns re-imports through a diff and applies only on Apply; CSV round-trips the same columns; an export containing a key is refused.
- In CI, D1 reads Table T1 through its grant as of its run start and writes its own document; a write through the grant is refused; the denylist scan stays green.
- `v_*` views regenerate on config apply and refuse writes; adding a grant from the Team sheet needs the second confirmation, and its dotted line appears on hover.
- Design QA against the tables mockup passes; merged with Playwright e2e and `just check` green.

## Boundaries

- Out: dashboards and the Assistant (epic-assistant-dashboards, which queries the views built here); excluding dry-run rows from views (epic-polish-dry-run-learning extends the views built here); cross-department hand-offs, fan-out and fan-in, review gates and event triggers (epic-workflow-engine-cli and epic-triggers-budgets-safety-floor; a grant here is read access only); a LAN or shared-grid mode for other people (v1.1 Team release, D5).
- Out: row provenance written at `table_write` (epic-workflow-engine-cli, R38); document rendering and versions (epic-runs-inbox-documents-ui); department export and import as a Proposal (epic-triggers-budgets-safety-floor; the Team sheet only calls it).
- Out: the privilege-widening dialog and the org-chart canvas (epic-org-seats-ui; this epic adds grant lines to it).
- Must not change: `table_write` stays the only seat write path and stays bound to the step's declared table (epic-workflow-engine-cli R8).

## References

- parent — _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md, Requirements R37, R39, R40, R41, R42; Constraints (Generic engine, Files, Secrets, Personal data, Accessibility, Mockups gate UI)
- architecture — _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md, AD-6, AD-15, AD-16, AD-17, AD-21, AD-22, AD-23, AD-24, AD-30, AD-35, AD-36 (granted inputs as of `start_seq`)
- reference company — _bmad-output/initiative-agentic-manager/reference-workload.md, Company A (Table T1, its statuses, the D2 → D1 grant)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md, Component Patterns (Table grid, Saved view tab, Provenance, Import / Export, Team box, Team sheet, Grant line, Privilege-widening confirmation), State Patterns (Empty table, No search results), Flow 6, Accessibility Floor
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md, Components (grid, grant-line, side-sheet)
- mockups — _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-tables-dashboards.html §1–2; key-org.html (grant line); key-team-sheet.html (Team sheet)

## Notes

- Waits on epic-workflow-engine-cli because: the generic row store, `table_write`, row history with provenance, run journal and stored document versions.
- Waits on epic-org-seats-ui because: the shell routes and ⌘K the grid registers in (story 1 on 5.13), the estimate line and Run now (story 10 on 5.7), the privilege-widening dialog (story 10 on 5.9) and the org chart the grant lines draw on (story 11 on 5.2).
- Waits on epic-runs-inbox-documents-ui because: the run view that row-history entries open (story 7 on 6.1) and the document pages the Team sheet opens (story 10 on 6.9).
- Decision: Refined (owner delegated, 2026-11-18): settled by the assumptions below; no spike, as AD-21 and AD-22 fix the storage and snapshot rules; the column-mapping step's layout and the Team sheet's section order within EXPERIENCE.md are left to the stories.
- Decision: Tracer bullet is story 1, a table's grid opening over `/api/v1` with its provenance cell, through store reader, API, shell and grid (2026-11-18).
- Decision: Sequencing (2026-11-18): grants read as of run start come second as the least certain part (AD-22 snapshot reads on the shared journal), then the Company A fixture that proves a grant between departments; the store lane (status enforcement, `v_*` views) follows because those stories share am-store migrations; the grid lane (editing, row detail, saved views and export, import) follows; Team sheet and grant lines close the feature work; then the refactor sweep and a closing end-to-end suite with design QA.
- Assumption: 13 entries stay in one epic; a split into a tables epic and a grants epic was considered and not taken because the fixture proof needs both under one Done when.
- Assumption: Exports carry a row id column and import matches by it first, then by the table's declared key column (Company A's Table T1: item_id); unmatched rows are listed and become new rows only when the owner ticks "Add as new rows" in the diff.
- Assumption: Comments live in the database with their Human actor (arch AD-35), are audited, appear in row detail and are not exported.
- Assumption: A granted document is a named step input (arch AD-36) read at the version current at run start; documents have no `v_*` view.
- Assumption: The Team sheet's Workflows, Team budget, guidance-note and Export department sections call the commands epics 4, 5 and 6 build and add no behaviour of their own.
- Decision: Team sheet mockup (owner delegated, 2026-11-18): Sally is producing ux-agentic-manager/mockups/key-team-sheet.html; story 10 matches it, with EXPERIENCE.md winning on conflict; closes the Team sheet mockup open question.
- Assumption: `v_<table-id>` views expose current rows only; row history stays out of the views in v1.
- Decision: Dependencies (owner delegated, 2026-11-18): the initiative now declares epic 5 before this epic; story 1's whole-epic reference to epic-runs-inbox-documents-ui is repointed to the stories it needs (5.13; 6.1 and 6.9 on stories 7 and 10).
- Decision: Raising a cap and adding a grant are privilege-widening and use the two-step confirmation (owner delegated, 2026-11-18; initiative Notes); the Team sheet's team budget follows it.
- Cross-epic: Run now on the Team sheet reuses epic-runs-inbox-documents-ui's run start and epic-org-seats-ui's estimate line; epic-polish-dry-run-learning extends the `v_*` views built here to exclude dry-run rows; epic-assistant-dashboards story 1 waits on story 5 here.
- Decision: Final review UX M3 (owner delegated, 2026-11-18): accessibility is a Definition of Done line for every story here. Requirements unchanged.
- Decision: Generic engine principle (owner, 2026-11-18: what a department does is fed to its agents, not the engine's job): the epic is titled "Tables, grants and saved views" and stories 1 and 8 "A table opens in the grid with its provenance" and "Saved views, a table's default view and export"; Personal data replaces the earlier data constraint.
- Decision: Owner REPLAN (owner, 2026-11-18, reference-company spec and Amendment 2; initiative Notes): the slug and folder are renamed `epic-tables-grants-views` and every `after` link is updated; R3, R6 and R10 restated; story 3's fixture becomes the Company A grant proof (D1 reads D2's Table T1); every test table and status is Company A's; no requirement id, entry id or order changes.
- Decision: Mac first (owner, 2026-11-19; initiative Notes): the closing end-to-end suite runs on the macOS arm64 runner only; Windows and Linux runners are v1.x (arch AD-47). No other change.
- Decision: Final review 2 UX M5 (owner delegated, 2026-11-19): grants live in the Team sheet; story 11 draws a grant line only on hover of a team (selection shows hand-offs), merging with a hand-off line between the same pair.
