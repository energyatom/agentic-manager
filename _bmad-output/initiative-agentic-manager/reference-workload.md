# Agentic Manager — Reference company: Company A

The v1 reference company: the content of the optional, clearly labelled template "Sample company: Company A (two departments)" the wizard can load, of every fixture and golden end-to-end test, and of the owner-run real-model demo (initiative R22, R26, R73–R77). From the owner's direction of 2026-11-18 and its Amendment 2 (`reference-company-spec-2026-11-18.md`, binding). Company A is fictional and deliberately abstract: departments, seats, documents and tables carry neutral names only, so no domain biases the builders. Every row and document is synthetic. These names live only under `templates/samples/company-a/` and `fixtures/`, never in engine or UI code (RULE-2, arch AD-23). Models are examples; every seat's model is switchable.

The engine adds nothing domain-specific: what each seat does (summarising, cleaning, comparing, advising) is its instructions, skills and tools, which the owner configures. Behaviour patterns follow the open-source CrewAI (crews and flows, hierarchical process) and Paperclip (org-chart company, heads reporting up) projects; their domain content is not used.

## Org chart

| Unit | Seat (id) | Occupant | Example model |
|---|---|---|---|
| Owner | Owner (`owner`) | **Human — You** (hats: CEO, Vision Creator, Reviewer, Developer; restated 2026-11-19) | — |
| Department 1 (D1) | D1-Head (`d1-head`), reports to the Owner | AI | Frontier (Claude Sonnet-class) |
| D1 | D1-E1 (`d1-e1`) | AI | Frontier (Claude Haiku-class) |
| D1 | D1-E2 (`d1-e2`) | AI | Local (LM Studio, gpt-oss-20b-class) |
| D1 | D1-E3 (`d1-e3`) | AI | Frontier (OpenAI-class) |
| Department 2 (D2) | D2-Head (`d2-head`), reports to the Owner | AI | Frontier (OpenAI-class) |
| D2 | D2-E1 (`d2-e1`) | AI | Local (Ollama, small instruct-class) |
| D2 | D2-E2 (`d2-e2`) | AI | Local (LM Studio, gpt-oss-20b-class) |
| D2 | D2-E3 (`d2-e3`) | AI | Frontier (Claude Haiku-class) |
| D2 | D2-E4 (`d2-e4`) | AI | Local (Ollama, small instruct-class) |

D1-E1 to D1-E3 deliberately mix one local (D1-E2, as in every mockup and EXPERIENCE.md; swapped with D1-E1 on 2026-11-18, reconcile) and two frontier models, so the sample shows per-seat routing and per-seat cost (R8, R11).

## Data owned by each department

| Department | Tables and documents | Notes |
|---|---|---|
| D2 | `document-a` (document, versioned), `report-r1` (document to the Owner), `table-t1` (table: `item_id` key, received, text, status `new → processed → handed-off` or `dismissed`, each transition with a reason); default saved view "Open items" (status new or processed) | Table T1 is loaded by a Local Repository Source from a drop folder of synthetic CSV and xlsx files and updated by D2-E1 to D2-E4 |
| D1 | `d1-opinion` (document, one labelled section per branch), `d1-summary` (document), `d1-folder-digest` (document, flow F3; 2026-11-19) | D1's team Knowledge holds synthetic reference notes |

Grant: D2 grants D1 read-only access to `table-t1` (R42).

## Flow F1 — cross-department fan-out, fan-in, review and lineage (workflow `f1`, owned by D2)

Trigger: manual; and the hand-back from F2's D1 workflow (below).

| # | Step | Seat (department) | Kind | Output |
|---|---|---|---|---|
| 1 | Produce Document A | D2-Head (D2), drawing on D2-E1 to D2-E4's work in `table-t1` | step | `document-a` |
| 2 | Work on Document A in parallel | D1-E1, D1-E2, D1-E3 (D1) | **cross-department hand-off** (D2 → D1, R73) into a **fan-out** of 3 branches (R74) | One output per branch, labelled by seat |
| 3 | Consolidate the three outputs | D1-Head (D1) | **fan-in**: waits for all 3 (quorum 3 of 3), timeout 30 min, on a branch failure: ask in the Inbox (R74) | `d1-opinion` |
| 4 | Hand back to D2 | — | **cross-department hand-off** (D1 → D2, R73) | — |
| 5 | Write Report R1 | D2-Head (D2) | step | `report-r1` draft |
| 6 | Review D2's report | D1-Head (D1) | **cross-department review gate** on the D2 → Owner hand-off: approve, return with note (back to step 5, at most 2), reject (R75) | Verdict with note, recorded with actor |
| 7 | Owner review | Owner — You | human gate in the Inbox: approve, edit, return with note, reject (R34) | Approved Report R1 |

Report R1 in the Owner's Inbox shows its **lineage** (R76): Owner ← D2-Head ← (review: D1-Head) ← D1-Head ← D1-E1 / D1-E2 / D1-E3 ← D2-Head ← D2-E1 to D2-E4 (`table-t1` rows) — for each contribution its seat, department, step, model, inputs (labelled, with taint), output version, tokens, time and cost, drillable from the Inbox item and the document, and exportable.

## Flow F2 — workflow triggers workflow, with loop protection

- Workflow `f2-update` (owned by D2), schedule every Monday 09:00 company time, catch-up once on wake: D2-E1 to D2-E4 update `table-t1` from the week's new rows (read through the step's Source cursor), each status change with a reason.
- Workflow `f2-d1` (owned by D1), event trigger `workflow_completed:f2-update` (R77): D1-Head reads `table-t1` through the grant and writes `d1-summary`.
- `f2-d1` completing triggers D2's `f1` (an event trigger across departments, R77), so D1's result can start D2's work again.

**Loop protection** (R77): the shipped sample has no cycle. A fixture variant (`fixtures/company-a-loop/`) adds "`f1` completed → `f2-update`": `agentic validate` reports the cycle with file, line and fix, and the runtime test (validation bypassed by a direct state write) shows the hop refused at the chain's cycle check and at the maximum depth, each with a Failure Inbox item and an audit entry.

## Flow F3 — many models on one folder (workflow `f3-folder`, owned by D1; added 2026-11-19)

Owner direction: "I can have one folder and multiple LLMs pointed there to read different files, and then it feeds the data to the data-gatherer agent for further processing." Trigger: manual.

| # | Step | Seat | Kind | Output |
|---|---|---|---|---|
| 1 | Read the folder by file type | D1-E1 (`*.pdf`; frontier, reads PDF), D1-E2 (`*.csv`; local text model through the text fallback), D1-E3 (`*.png`, `*.jpg`; frontier, reads images) | **fan-out over a folder** (`over_folder: d1-inputs`, three routes, `files: new_or_changed`; R84) | One output per branch, labelled by seat with the files it read |
| 2 | Gather | D1-Head | **fan-in**: all three, timeout 30 min, on a branch failure: ask; receives the unrouted list (the docx) | `d1-folder-digest` |

A route `*.xlsx → D1-E2` is the sample's refusal example: D1-E2's local text model has no office-files chip, so `agentic validate` warns and Run now is disabled with the reason until the owner gives D1-E2 a model that reads office files (for example an OpenAI Account, whose Responses API reads spreadsheets) or routes the files elsewhere. Linking `d1-inputs` to D1-E1 and D1-E3, whose models are cloud Accounts, is privilege-widening and appears in the sample's import diff ("files in d1-inputs leave this Mac for …").

## Sources, skills and knowledge in the sample

- `t1-drop` — Local Repository Source owned by D2-E1: synthetic CSV and xlsx files → `table-t1`, key `item_id`, files deleted after a committed load (R67). Any cleaning or de-duplication of the text is a seat's instruction, not an engine feature.
- `d1-notes` — team Knowledge for D1: synthetic reference notes, keyword-searched with `search_knowledge` (R68).
- Fixture-only Sources over the loopback fixture site (`fixtures/sites/`): a Crawler, a rendered Crawler, a Feed and an API Source of synthetic pages and items, used by the connector tests (epic 12); not part of the loadable sample.
- `d1-inputs` — a folder of synthetic files shipped with the sample at `files/d1-inputs/` (two PDFs, one with no text layer; three csv files; two PNGs; one docx), linked read-only to D1-E1, D1-E2, D1-E3 and D1-Head (added 2026-11-19, owner folder-access direction). D1-E2 has its text fallback on.
- Skills: `structured-review` (attached to D1-E1, D1-E2, D1-E3: sections findings, risks, required actions, confidence) and `plain-report` (attached to D2-Head) (R60).
- Role-template features: tests of the Coordinator template's learning loop give D2-Head that template's learning workflow, so the sample keeps its abstract names; Ask a seat tests address D2-Head (the head of the department Report R1 came from) and one picked seat, D1-E2 (restated 2026-11-18, reconcile).
- CEO goal: "Deliver Report R1 every week with every finding traceable." Vision Creator note on D1, expiring in 30 days: "State confidence for every finding."

## Caps (placeholders until the owner sets them, prd OQ-8)

₹150 per scheduled run, ₹250 per manual run and ₹600 per week for each sample workflow and for the company; seat caps from the role templates' default budgets. Fan-out branches reserve against the run's caps per branch (arch AD-40).

## Tests and acceptance built on it

- **Golden end-to-end** (FakeLlm, one script per seat; epic 3 closing story and later end-to-end stories): F1 runs from a manual start, F2 runs through to F1, and F3 runs over `d1-inputs` with recorded provider file responses (each route's files reach their seat, the docx is listed as unrouted); snapshots of documents, rows, ledger (per branch), Inbox and lineage are compared.
- **Failing-first engine tests** on the same fixture: fan-in waits for all or the quorum; one branch failing gives the configured behaviour (fail, continue at quorum, or ask); kill -9 mid-fan-out resumes losing at most the in-flight calls; taint from a poisoned `table-t1` row reaches the D1 branches and Report R1; the review gate in the other department blocks until decided; lineage is complete and matches the journal; the trigger loop is stopped.
- **Owner-run real-model demo**: F1 with D1-E2 on a local model and D1-E1 and D1-E3 on frontier models, each step schema-valid after at most one repair.
- **Dogfood acceptance**: the owner runs the owner's own company's departments (any the owner chooses), with at least one cross-department flow, for four consecutive clean weeks on a frozen build (Done when; trust gate).

## Role templates (ship in full)

Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist and Human seat — generic roles only (eight; "Chief of Staff" is not a template name, restated 2026-11-18). The Coordinator template ships the learning loop and is the default seat for Ask a seat. Each carries role instructions, a goal and backstory, default tools, a default budget and a Recommended model resolved from existing Accounts (minimum local model per role: prd OQ-6).

## Not shipped in v1

No domain department templates: v1 ships the Blank department, the role templates and the optional "Sample company: Company A (two departments)" only.
