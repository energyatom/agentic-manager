---
title: 'Document versions and Draft items'
type: feature
ticket: 21
created: '2026-11-24'
status: approved
baseline_revision: '2ac755b'
after: [22]
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: []
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-rows-written-with-provenance-through-toolgate-plan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Slice:** 3.6b, epic 3 entry 21, re-sliced from story 3.6 by the owner on 2026-11-24 (Design Notes, Re-slice). Build order (owner, third re-slice): the store slice (entry 6), the ToolGate slice (entry 23), 3.6c (entry 22), then this slice, `after = [22]`.

**Naming (owner, third re-slice):** "3.6a" below means the two slices that replaced it: the store slice (entry 6, "Row store and history": migration 0010, the row command, `AttemptWrites`) and the ToolGate slice (entry 23, "table_write through ToolGate": the port, `table_write`, the engine result, the check-deps rules, `StepPlan.document`).

Earlier wording, kept: It builds after 3.6a (entry 6, "Rows written with provenance through ToolGate", `after = [6]`) and after 3.6c (entry 22, "Rows reaching later steps"), which comes before it in table order (owner, second re-slice), on its own branch; its baseline is set when it starts. Matrix row numbers are kept from the approved 3.6 plan, so the written tests still point at them; rows 18, 22, 28 and 31 here are the document halves of rows 3.6a also carries. Row 41 is new.

**Problem (after 3.6a and 3.6c):** rows have a store, a history, one tool path and one read path, but documents and Draft items have nothing.
- No step declares a document: `document:` is still an unknown key in `workflows/*.yaml` (`crates/am-config/src/kinds.rs` step keys L2355-L2364).
- `StepPlan.document` and `AttemptWrites.document` exist since 3.6a but are refused in words naming 3.6b.
- Nothing stores a document version; there is no `document_versions` table (0010, 3.6a, holds the rows; 0011, 3.6c, the row-input link).
- Nothing raises a Draft item, and none can be resolved: three places name "3.6" for it (`crates/am-store/src/lib.rs` L2398-L2406, `crates/am-app/src/gate.rs` L373-L380, `crates/am-server/src/inbox.rs` L150-L154), and `Decision::allowed_for(Draft)` is empty (`crates/am-domain/src/inbox.rs` L103-L112).

The verify, as re-sliced (Sofia's entry 21 is the spec once merged): a document step leaves one Markdown version per succeeded attempt (the owner's approved deviation from "one Markdown version per run", Q7) and a Draft item in the Inbox, which only a person can dismiss, through 3.4's Human-only path.

**Approach (owner-approved 2026-11-24, carried over unchanged):**
- **What makes a version (Q5):** a step declaring `document: D` stores its validated final message as a version of D when it succeeds.
- **Where (Q6; AD-35):** a `document_versions` row in the database (migration 0012), naming the producing attempt and its taint; the Markdown stays once, on the attempt's `step_runs.output` (AD-43).
- **Numbering (Q7; AD-29 as amended 2026-11-24):** one version per succeeded attempt, numbered per document; a run's version is its latest. The owner's approved deviation from the ticket's "one Markdown version per run".
- **Draft items (Q8; AD-28):** one Draft item per version, in the step's finishing transaction. Dismissed only by a person, through `Writer::resolve_inbox_item(by: HumanActor, …)` (3.4).
- **Taint (AD-10 as amended and restated 2026-11-24):** a version and its Draft item carry the attempt's taint at rest; a version written by a tainted step taints every later reader, by any path.
- **Untrusted at render time (AD-29):** the Markdown is stored byte for byte and rendered only through the sanitising renderer (6.9), which is not in this slice.

## Boundaries & Constraints

**Always:**
- **One version per succeeded attempt (owner Q7; AD-29 as amended 2026-11-24):** never replaced; numbered per document in commit order; the run's version is its latest. Recorded as the owner's approved deviation from the ticket's "one Markdown version per run".
- **Stored once (AD-43, AD-35):** the version row holds no Markdown; the text stays on the producing attempt's `step_runs.output`.
- **One transaction (AD-8):** settle the call, finish the step `succeeded` with its output, add the version and the Draft item, and audit, all together (`SettleAndFinishDocumentStep`).
- **Writer commands with an explicit Actor (AD-6, AD-24):** the document finish and the Draft item are `Actor::System` (engine); only `Actor::Human` resolves an item.
- **Append-only (the 3.5 lesson):** `document_versions` refuses UPDATE, DELETE and any INSERT that meets an existing id or unique key.
- **Draft dismissal (owner Q8; AD-15, AD-28):** only `Writer::resolve_inbox_item(by: HumanActor, item, Decision::Dismiss, None)`, from its two sites (`crates/am-server/src/inbox.rs`, UI class; `crates/am-app/src/gate.rs`). 3.4's check-deps resolve rule stays as it is. A non-Human call cannot dismiss: it does not compile, the writer refuses it, and no `WriteCommand` resolves.
- **Taint (AD-10 as amended and restated 2026-11-24):** a version and its Draft item carry the attempt's taint; a tainted version taints every later reader. A reader of a version arrives with 3.15 (`document:` hand-off payloads) and 7.2 (granted documents); each proves its path.
- **Untrusted at render time (AD-29):** stored byte for byte; returned by the API as a JSON string; printed by `agentic gate` with control characters neutralised; never rendered, fetched or loaded here.
- **Audit (AD-25; AD-32 keyed-hash rule):** `document.versioned` holds ids, the version, the taint and the byte count. Never Markdown, and no new `*_sha256` of free text; the step's own finish keeps 3.5's `output_sha256` through the existing helper.
- **Interim widening risk (owner Q3; AD-15 as amended 2026-11-24):** until 4.17 ships `is_widening`, adding or changing a step's `document:` is not held for the two-step confirmation; an edit picked up at Reload or startup takes effect unchecked. Accepted by the owner; 4.17 classifies it (Sofia's replan, #35).
- **No row path here (AD-21 as amended, "Row-writer path"):** this slice adds no row write, no `RowWriter` caller and no `EngineResult` constructor call; 3.6a's and 3.6c's check-deps rules hold unchanged.
- **New callers of `Instruction::authored` (owner, 2026-11-24):** this slice adds none; if the build needs one, it goes through the same security review as 3.5.
- **Order of work (owner, 2026-11-24):** after 3.6a and 3.6c merge, this plan's baseline is set and it is committed on its own branch; the tester's split tests for this slice are logged red before any `src/` change; one squash commit, on Opus.
- **Tests first:** every test in the Tests-first list is written and seen red, with its reason logged, before any `src/` change. FakeLlm and FakeClock only; no network, no real model.

**Never:**
- **No `write_document` or `draft_message`** (Sofia places them before 3.13); they stay refused by class.
- **No delete, move or replace of a version.**
- **No document rendering, export or citation resolution** (6.9). No PDF, no HTML conversion.
- **No other decision on a Draft** than dismiss: approve, return, reject and answer are refused.
- **No new crate, no new prompt content type.**
- **No department vocabulary in `crates/`.**
- **Outside this slice:** rows and ToolGate (3.6a), document reads and hand-offs (3.15, 7.2), lineage reads (3.17), the Inbox UI and "Copy draft" (6.6, 6.9), dry-run marks (AD-37), the `is_widening` classification (4.17).

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 18 | Append-only (versions) | `UPDATE`/`DELETE` on `document_versions`; `INSERT OR REPLACE` on it by id or unique key | Aborted by trigger (the row half is 3.6a's) | Trigger |
| 22 | Audit (Markdown) | A document step whose Markdown holds a canary | `document.versioned` holds the document id, version, run, step run, taint and bytes; no canary and no hash of it in the audit export; the chain verifies (the row half is 3.6a's) | — |
| 23 | Document step | A step declaring `document: D`; FakeLlm's final message is Markdown | Step `succeeded`, output = the Markdown; one `document_versions` row (D, version 1, run, attempt, step run, taint, bytes); one Draft item (payload = that attempt, taint = the version's, title and body in the engine's words); all in the step's finishing transaction; audit `document.versioned` and `inbox.created` | — |
| 24 | Next run | A second run of the same workflow | Version 2 of D; version 1 unchanged | — |
| 25 | Same run again (Q7, owner-approved deviation) | The document step's attempt 2 in the same run (loop-back or gate return) | Version n+1 with its own Draft item; earlier versions and items unchanged; the run's version is its latest | — |
| 26 | Tainted document | A tainted document step | Version taint 1; Draft item taint 1 | — |
| 27 | Empty document | Final message empty, only whitespace, or holding NUL; over 1 MiB | Step fails with a Failure item; no version, no Draft | Failure item |
| 28 | Mixed declarations (documents) | `document:` with `output_schema:`, `loop_back:` or `gate:`; `document:` on a gate or fan-out step | `agentic validate` error; `run_on` refuses a hand-built plan before any row (the `table:` half is 3.6a's) | Worded |
| 29 | Hostile Markdown | Script tags, raw HTML, a remote image, a link, control characters | Stored byte for byte; the read route returns it as a JSON string; `agentic gate` prints it with control characters neutralised (3.4); no egress action is audited and the image host is in no audit entry | — |
| 30 | Draft dismissal (Q8) | `agentic gate` on a TTY, a Draft item | Shown with document id, version, run and taint marker; `d` dismisses through `resolve_inbox_item(HumanActor, …, Dismiss, None)` as You (Reviewer); approve, return, reject and answer refused; the run is unaffected | Refused in words |
| 31 | Validate (documents) | `document:` not kebab-case; one document id in two teams' workflows | Each an error with file, line and fix (the table half is 3.6a's) | Validate error |
| 39 | Non-Human dismissal (Q8) | Dismissing a Draft as `Actor::System` or `Actor::Seat` | Does not compile at `Writer::resolve_inbox_item` (doctest); the writer's own check refuses it with nothing written (a store unit test); no `WriteCommand` resolves (3.4's doctest stands); the API with the CLI token alone resolves nothing (3.4) | Refused |
| 41 | Migration 0012 | A version-11 database with rows, history, row-input edges, runs, attempts, calls, tool calls, Inbox items, prompt parts | Pre-migration copy; every row kept value for value; `document_versions` empty; `step_runs.writes_document` NULL on old rows; report 11 → 12; `store.migrated` audited; reopening applies nothing; a failing 0012 names the backup and leaves version 11 | Migration error |

</intent-contract>

## Code Map

- **`crates/am-store`:**
  - `migrations/0012_document_versions.sql` (new): `document_versions`, `step_runs.writes_document`, their triggers; `src/migrations.rs` `MIGRATIONS` gains it; `SCHEMA_VERSION` becomes 12.
  - `src/lib.rs`:
    - `start_attempt`: journals `AttemptWrites.document` (3.6a refused it) on `step_runs.writes_document`; a trigger refuses changing it once set.
    - New `WriteCommand::SettleAndFinishDocumentStep` beside `SettleAndFinishStep` (3.6a's L293): settle, finish, version, Draft item, one transaction; the writer checks the attempt's `writes_document` equals the document id and numbers the version.
    - `create_item` (L2270 at the 3.6 baseline): unchanged rules; a Draft's payload is its own attempt, its taint at least the version's (L2276-L2292 already raise it).
    - `resolved_from` (L2398-L2406): Draft leaves it. The resolve's non-Human refusal (3.4) is kept and reached by a unit test for a Draft.
    - `Reader`: `document_versions(document_id)`; `attempt_writes` returns the document.
  - `src/documents.rs` (new): the version and Draft insert.
- **`crates/am-domain/src/inbox.rs`** `Decision::allowed_for` L103-L112: `Draft => [Dismiss]`.
- **`crates/am-config`:** `src/entities.rs` `Step` gains `document: Option<String>`; `src/kinds.rs` step key `document` and its checks beside `hand_off_checks` (kebab-case; one team per id, the declaring seat's; no `output_schema`, `loop_back` or `gate` with it; none on a gate or fan-out step).
- **`crates/am-engine/src/run.rs`:** `run_on` lifts 3.6a's "document steps arrive with 3.6b" refusal and checks a document's mixes instead; `StartAttempt` carries the document; `succeed` (L1702 at the 3.6 baseline) finishes a document step through `SettleAndFinishDocumentStep`; an empty document fails with a Failure item.
- **`crates/am-app`:** `src/run.rs` `plan_of` maps the step's document; `src/gate.rs` `later_story` (L373-L380) and `decide` (L589-L593): a Draft is shown and `d` dismisses it through the existing `dismiss` (L780).
- **`crates/am-server/src/inbox.rs`** L150-L154: Draft leaves the later-story list; `allowed_for` (L176) then admits dismiss.
- **`justfile`**, **docs:** nothing new in `validate` (3.6a added the fixture); `docs/config-schema.md` gains the `document` key.
- **Fixture:** `fixtures/projects/rows-documents/` gains the `summarise` step (`document: summary-note`) in workflow `record`.
- **Existing tests that must change (supersession):**
  - `crates/am-store/tests/inbox_resolve.rs` L575-L611 and L651-L655: Draft leaves the "takes none" and "3.6" lists.
  - 3.6c's migration test, if it pins `SCHEMA_VERSION == 11` ("0011 is this story's"), becomes "the 10 to 11 step is applied" (as 3.5, 3.6a and 3.6c did for theirs).
  - 3.6a's `a_document_value_is_refused_until_3_6b` and the document refusal in `run_on`'s test are superseded: a document is now journaled and versioned.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:** 3.6a and 3.6c merged; this plan's baseline set to main after 3.6c; this plan committed on its own branch.
- [ ] **Tests first (no `src/` change), on Opus:** this slice's files in the Tests-first list, the fixture step and the superseded tests. Log this slice's red run with each test's failure.
- [ ] **am-store:** migration 0012, `SettleAndFinishDocumentStep`, `documents.rs`, the readers, the audit args, Draft out of `resolved_from`.
- [ ] **am-domain:** `Decision::allowed_for(Draft)`.
- [ ] **am-config:** the `document` key and its checks.
- [ ] **am-engine:** the document finish; `run_on`'s document checks.
- [ ] **am-app:** `plan_of`'s document; `agentic gate`'s Draft.
- [ ] **am-server:** the Draft later-story line.
- [ ] **Docs:** `docs/config-schema.md`.
- [ ] **Size (owner Q9, re-slice):** one squash commit; if it overruns, stop and report.

**Acceptance Criteria:**
- Given each matrix row of this slice, when its test runs, then versions, Inbox items, step finishes, taint, audit entries and CLI output match the row.
- Given this slice's verify, when the fixture's document step runs on FakeLlm: it leaves one Markdown version per succeeded attempt (the owner's approved deviation) and a Draft item that only a person can dismiss.
- Given the workspace, when `just check` runs, then it is green, 3.6a's suites and 3.4's gate and `rule_15_*` suites included. Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-24, owner (re-slice): split from the approved 3.6 plan (`story-rows-and-documents-written-with-provenance-plan.md`, now 3.6a's `story-rows-written-with-provenance-through-toolgate-plan.md`). Approved answers carried over unchanged. Carries rows 23–27, 29, 30, 39, the document halves of 18, 22, 28, 31, Q5–Q8's build, and `document_versions` with `step_runs.writes_document` as migration 0011 (new row 41; renumbered 0012 by the second re-slice). Baseline set when it starts.
- 2026-11-24, owner (second re-slice): the row-read side moved from 3.6a to a new 3.6c (entry 22, "Rows reaching later steps"), built after 3.6a and before this slice. This slice's scope is unchanged; its migration is renumbered 0011 → 0012 so the order is 0010 (3.6a), 0011 (3.6c), 0012 (3.6b); its baseline is set after 3.6c (`TBD-after-3.6c`); Q4's reader condition is 3.6c's.
- 2026-11-24, owner (third re-slice): 3.6a split into the store slice (entry 6) and the ToolGate slice (entry 23); this slice now builds after 3.6c (`after = [22]`, baseline `TBD-after-3.6c`). Its scope and migration number (0012) are unchanged; "3.6a" in this plan names the two (Intent, Naming). Size reported as code and test lines.
- 2026-11-25, manager: baseline set to `2ac755b`, 3.6c's head (PR #45). Main after #45's squash merge has the same tree, and this branch is rebased onto it.

## Design Notes

### Owner answers (2026-11-24)

**Owner, verbatim:** "Approved: the 3.6 plan with the recommended answers to Q1–Q9 and these conditions. Q1: a check-deps rule so only ToolGate calls the row-writer. Q2: tests that a stale-version update is refused and the old version stays readable. Q3: record the interim risk, and add "changing table: or document:" to 4.17 as widening. Q4: Winston merges the narrow AD-10 amendment first (results built solely from engine values by ToolGate, enforced by check-deps, error results excluded and still untrusted, planted-string test), and a row written by a tainted step carries that taint to any later reader, with a test. Q7: record as my approved deviation from "one version per run". Q8: Draft dismissal uses the 3.4 Human-only path, with a test that a non-human call cannot dismiss. Q9: one squash commit; stop and report if it overruns. Merge #34 on green on every run. Then commit the plan, log the red run, and build tests first on Opus. Merge only with both CI jobs green after review triage."

**Answers and conditions, as the plan builds them:**
- **Q1:** `table_write` in am-tools, writing through an am-domain row-writer port (owned types); am-engine implements it over the store's writer; only ToolGate calls it; `agentic run` offers it only to a step that declares a table. *Condition:* a check-deps rule so only ToolGate calls the port's method (Code Map, xtask; matrix row 33).
- **Q2:** insert, or update by row id with an expected version; no delete; every after-image kept. *Condition:* tests that a stale update is refused in words, nothing changes, and the old version stays readable (matrix row 4).
- **Q3:** step keys `table:` and `document:`; a seat writes only its own team's table; one team per document id. *Condition:* the interim risk is recorded (Boundaries); Sofia adds "changing `table:` or `document:`" to 4.17 as widening in a separate replan.
- **Q4:** a trusted result built by ToolGate solely from engine values (row id, version, a fixed status word). *Conditions:* Winston's narrow AD-10 amendment merges first; check-deps keeps its constructor in ToolGate; error results stay untrusted; a planted-string test; a row written by a tainted step taints a later reader, with a test (matrix rows 35 to 38).
- **Q5:** a version comes from the document step's final message.
- **Q6:** a `document_versions` row, with the Markdown kept once.
- **Q7:** one version per succeeded attempt. *Condition:* recorded as the owner's approved deviation from the ticket's "one Markdown version per run" (Intent, matrix row 25).
- **Q8:** one Draft item per version, dismissable here. *Condition:* dismissal uses 3.4's Human-only path, with a test that a non-Human call cannot dismiss (matrix row 39).
- **Q9:** one squash commit. *Condition:* stop and report on an overrun.
- **Sequence:** #34 merges green on every run; then the plan is committed, the red run logged, and tests built first on Opus; merge only with both CI jobs green after review triage.

**In this slice:** Q3's `document:` half, Q5 to Q8, and Q9 for this slice. Q1, Q2, Q4 and Q3's `table:` half are 3.6a's, unchanged, except Q4's reader condition, which the owner moved to 3.6c on 2026-11-24 (second re-slice), unchanged.

### Re-slice (owner, 2026-11-24)

**Owner, verbatim:** "Don't accept a single 12,000-line commit. Sofia re-slices 3.6 into 3.6a (row store, history, row-writer interface, table_write via ToolGate only, engine-built result, tainted-row rules) and 3.6b (document versions, Draft items with Human-only dismissal). My approved answers carry over unchanged. Split the written tests by slice, log a red run for each, one squash commit per slice, on Opus. Fan-out: accept that the Company A f2-update step fails until 3.14, but agentic validate must name the reason, and 3.14's ticket must include allowing table: on fan-out steps."

**Third, owner verbatim:** "Split 3.6a by layer: a store slice (migration 0010, row and history write commands, append-only triggers, taint on write, stale-version refusal, store-level tests) and a ToolGate slice (row-writer interface, table_write via ToolGate only, engine result and journaling, idempotency, check-deps rules, no-read-path proof, fan-out warning). Order: store, ToolGate, then 3.6c, then 3.6b. Condition: nothing in shipped code calls the row-write commands until the ToolGate slice lands, enforced by check-deps. Accept Winston's wording "on its tool call's journal row (AD-8)" in the same re-slice PR. From now on, plan estimates report code and test lines separately, and the overrun stop is about 4,000 lines of code."

**Second, owner verbatim:** "Split again: move the row-read side (row-input slot, from_row link, reader taint, fenced row text) into a new 3.6c, "Rows reaching later steps", built right after 3.6a and before 3.6b. Condition: 3.6a contains no way for a step to read rows, with no from_row and no row-input slot, even test-only, plus a test or check-deps rule proving no engine path passes row content to a model call. 3.6a still stores each row's own taint on write. My Q4 condition moves to 3.6c unchanged. Add the AD-10 journaling of the engine result to 3.6a."

**What this means here:**
- Build order: 3.6a, 3.6c, then this slice. Its migration is 0012, after 3.6a's 0010 and 3.6c's 0011.
- This slice: the `document:` key, `document_versions` (migration 0012), the document finish, Draft items and their Human-only dismissal.
- The fan-out warning is 3.6a's; this slice adds the `document:` case to its fan-out refusal as a validate error (row 28), since no fan-out step writes a document before 3.14 either.
- `StepPlan.document` and `AttemptWrites.document` arrive with 3.6a, inert and refused; this slice turns them on, so no literal changes again.

### Documents (owner Q5, Q6, Q7)

- **What makes a version (Q5):** the document step's validated final message, as the ticket and R10 say. A step declaring `document: D` stores that Markdown as a version when it succeeds. "Validated" means: not empty after trimming, no NUL, at most 1 MiB. Otherwise the step fails with a Failure item and no version.
- **Where it lives (Q6; AD-35: documents and versions are database rows):** `document_versions`, migration 0012 (additive, forward-only, AD-7; 0010, 3.6a, holds the rows; 0011, 3.6c, the row-input link):

| Column | Why |
|--------|-----|
| `id TEXT PRIMARY KEY` | UUIDv7 the engine mints |
| `document_id TEXT NOT NULL` | The step's declared document |
| `version INTEGER NOT NULL ≥ 1` | Numbered per document by the writer (MAX + 1 in the transaction); `UNIQUE (document_id, version)` |
| `run_id`, `attempt` | "Versioned per run" (AD-29 as amended); the run's latest version is its version |
| `step_run_id TEXT NOT NULL UNIQUE` | The producing attempt (AD-43); one version per attempt |
| `taint INTEGER NOT NULL` | The attempt's taint at finish, marked at rest (AD-10 (c)) |
| `bytes INTEGER NOT NULL` | Size, for lists without reading text |
| `created_at` | When |

  - `step_runs.writes_document TEXT` (NULL on old rows; set once, a trigger refuses a change): the writer checks the finish against it.
  - The Markdown is not copied. It stays once on the producing attempt's `step_runs.output` (AD-43's "never stored twice", as `step_inputs` and `prompt_parts` do). The existing read route already serves an item's payload text (`crates/am-server/src/inbox.rs` L83-L110).
  - Triggers: no update, no delete, never replaced.
- **Version numbering (Q7, the owner's approved deviation; AD-29 as amended 2026-11-24):** per document, 1, 2, 3 in commit order across runs. A second attempt in the same run (a loop-back or a gate return) adds a version and never replaces one. The ticket says "one Markdown version per run"; the owner approved one version per succeeded attempt instead. A run with one attempt still leaves exactly one.
- **The finishing transaction (`SettleAndFinishDocumentStep`):** settle the call; the step `succeeded` with its output; the version; one Draft item; audits `model_call.settled`, `step.finished` (3.5's `output_sha256` via the helper), `document.versioned` (document id, version, run, step run, taint, bytes) and `inbox.created`.
- **Validate (Q3's `document:` half):** a document id is kebab-case and owned by the declaring seat's team, one team per id; no `output_schema`, `loop_back` or `gate` with it; none on a gate or a fan-out step (errors, rows 28 and 31). Adding or changing `document:` is widening for 4.17 (AD-15 as amended).

### Draft items (owner Q8)

- Kind `draft`; payload the producing attempt; taint at least the version's (`create_item` already raises it).
- One per version. 0008's `UNIQUE (step_run_id, kind)` already allows exactly that, so no Inbox table rebuild.
- Title and body are the engine's words: "Draft: `<document-id>` version `<n>`" and "Step `<key>` wrote version `<n>` of `<document-id>` in run `<run>`. Drafts only: Agentic Manager never sends it." No model text.
- It never blocks the run: the step succeeded and the run goes on.
- **Dismissal uses 3.4's Human-only path:** `Writer::resolve_inbox_item(by: HumanActor, item, Decision::Dismiss, None)`, from its two sites (`am-server/src/inbox.rs`, UI class; `am-app/src/gate.rs`). 3.4's check-deps resolve rule (`RESOLVE_TOKENS`, `RESOLVE_SITES`) is unchanged.
- **A non-Human call cannot dismiss (owner condition):** `HumanActor` cannot be built from an `Actor` (compile_fail doctests); the writer refuses any non-Human actor in its own check (3.4's defence in depth, reached by a unit test for a Draft); no `WriteCommand` resolves; the CLI token alone resolves nothing over the API (3.4).
- 3.4's rules hold: a resolved item never changes; `inbox.updated` carries only id and status; dismiss carries no note; a tainted item's dismiss needs no typed id (only approve does).
- `agentic gate` shows a Draft's document id, version, run and taint marker, and the Markdown through 3.4's neutraliser (`gate.rs` L258, L341).

### Taint and provenance (AD-10 as amended and restated 2026-11-24, AD-43)

- A tainted document step's version: `document_versions.taint = 1`, and its Draft item's taint is 1 (row 26).
- A version written by a tainted step carries that taint at rest to every later reader, by any path. No reader of a version exists in this slice: a `next` input from a document step is still a step output (3.3's path, with the producer's committed taint); reading a version by id comes with 3.15 and 7.2, and each of those proves its path. No version yields a `TrustedInput`.
- **Untrusted at render time (AD-29):** the Markdown is model output. It is stored byte for byte, returned by the API as a JSON string, and printed by the CLI with control characters neutralised. It is rendered only through the sanitising renderer (6.9). Nothing here fetches a link or loads an image (row 29).
- **Lineage (AD-43):** each version names its producing step run; 3.17 walks `document_versions` with `step_inputs` and `row_history`. Nothing is stored twice.

### Owner decisions in this slice

- **Q5:** the document step's final message. `write_document` stays refused; Sofia places it before 3.13.
- **Q6:** a `document_versions` row, the Markdown kept once on the producing attempt's output.
- **Q7:** one version per succeeded attempt; the owner's approved deviation, recorded here and in AD-29.
- **Q8:** one Draft item per version, dismissable here through 3.4's Human-only path, with the non-Human tests.
- **Q3 (`document:` half):** the key, its checks, and the interim widening risk until 4.17.
- **Q9:** one squash commit for this slice; stop and report if the code passes about 4,000 lines (owner, third re-slice).

### Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code:** about 800 to 1,100 lines: migration 0012 (~60), am-store's `SettleAndFinishDocumentStep`, `documents.rs`, readers and the Draft decision (~400 to 500), am-config's `document` key and checks (~150), am-engine's document finish and `run_on` checks (~150 to 200), am-app's `plan_of` and `agentic gate` Draft (~100 to 150), am-server and am-domain lines, docs and fixture (~50). **Well under the ~4,000-line stop.**
- **Tests:** about 1,300 to 1,500 lines, already written and moved from 3.6: `documents_versions.rs` (~280), `taint_documents_untrusted_markdown.rs` (~200), the document and Draft store tests (~300), `gate_cli.rs`'s Draft tests (~140), `inbox_resolve_human_only.rs`'s test (~70), the document halves of `step_writes.rs`, `run_rows.rs` and `rule_16_row_provenance.rs` (~150), plus the new migration 0012 tests (~150) and the moved tainted-document test (~60).

### Tests-first list

All on FakeLlm and FakeClock; no network; no real model. These are the tests written for 3.6 that move here in the tester's split; the tester logs this slice's red run after 3.6a and 3.6c merge. Each am-e2e file keeps its `[[test]]` target.

**am-store (`crates/am-store/tests/documents.rs`, split from 3.6's `rows.rs`):**
- `migration_0012_keeps_every_row_and_the_pre_migration_copy`, `migration_0012_adds_document_versions_and_a_null_column`, `reopening_a_version_12_database_applies_nothing`, `a_failing_0012_names_the_backup_and_leaves_version_11` (row 41). New, in 3.6a's 0010 pattern. Red: `SCHEMA_VERSION` is 11.
- `a_document_version_and_its_draft_commit_with_the_step` (row 23). Red: no `SettleAndFinishDocumentStep`.
- `document_versions_number_per_document` (rows 24, 25). Red: as above.
- `document_versions_are_append_only` (row 18). Red: no table.
- `a_document_audit_holds_ids_never_markdown` (row 22). Red: no `document.versioned`.
- `a_draft_is_dismissed_only_by_a_person` (row 30). Red: `allowed_for(Draft)` is empty.
- The guard for the writer unit test (`the_writer_unit_test_for_a_non_human_draft_is_there`) (row 39). Red: the unit test does not exist.
- Unit test in `crates/am-store/src/lib.rs` (`#[cfg(test)]`), written by the implementer (the tester may not change `src/`): `a_non_human_resolve_of_a_draft_is_refused_in_the_writer` (row 39). Its intended body is in Implementation Notes.
- Doctest on `Writer::resolve_inbox_item`: `compile_fail` dismissing with `Actor::Seat { .. }` (row 39), beside 3.4's `Actor::System` one (green on arrival, as logged).
- **Superseded:** `inbox_resolve.rs` L575-L611, L651-L655; 3.6c's `SCHEMA_VERSION == 11` pin if it has one; 3.6a's document refusals.

**am-config (`crates/am-config/tests/step_writes.rs`, the document tests):** the document half of `a_step_declares_its_table_and_document`; `a_document_step_takes_no_schema_loop_back_or_gate` (row 28); `one_document_belongs_to_one_team` (row 31); `a_fan_out_step_declares_no_document` (row 28). Red: unknown key `document`.

**am-e2e engine (`crates/am-e2e/tests/engine/documents_versions.rs`, all five):** `a_document_step_leaves_one_version_and_one_draft` (row 23, the verify); `a_second_run_adds_version_two` (row 24); `a_returned_document_step_adds_a_version_in_the_same_run` (row 25, the owner's approved deviation, Q7); `an_empty_document_fails_with_a_failure_item` (row 27); `a_mixed_declaration_is_refused_before_any_row` (row 28). Red: documents refused since 3.6a.

**am-e2e trust (`crates/am-e2e/tests/trust/`):**
- `taint_documents_untrusted_markdown.rs` (both): `hostile_markdown_is_stored_as_is_and_nothing_is_fetched` (row 29); `the_cli_prints_a_draft_neutralised` (rows 29, 30).
- `taint_documents_at_rest.rs` (new file name for the moved test): `a_tainted_document_version_and_its_draft_carry_taint` (row 26), moved from `taint_rows_at_rest.rs`, since `taint_rows_*` names the row suite of AD-10's restatement.
- `rule_16_row_provenance.rs`: the document halves of `audit_holds_ids_never_row_content` and `the_chain_verifies_after_row_and_document_writes` (row 22).
- `inbox_resolve_human_only.rs` (3.4's; a test added, none changed): `a_draft_cannot_be_dismissed_by_a_non_human` (row 39: through the store and through the API with the CLI token alone).
- Red: documents refused since 3.6a; Draft refused for everyone.

**am-app (`crates/am-app/tests/`):**
- `run_rows.rs`: the document half of `plan_of_maps_grants_team_table_document_and_the_row_tool`.
- `gate_cli.rs`: `a_draft_is_listed_and_dismissed` and `a_draft_takes_no_approve` (row 30). Red: Draft refused as "3.6".

**Fixture:** `fixtures/projects/rows-documents/` workflow `record` gains `summarise` (`document: summary-note`).

## Verification

- `cargo nextest run` per crate, filters first: am-store `documents`, `inbox_resolve`; am-config `step_writes`; am-engine `run`; am-e2e `documents_versions`, `taint_documents_untrusted_markdown`, `taint_documents_at_rest`, `rule_16_row_provenance`, `inbox_resolve_human_only`; am-app `run_rows`, `gate_cli`, `gate_tty`.
- `cargo test --doc -p am-store -p am-domain`.
- `cargo xtask check-deps`: 3.4's resolve rule and the rules of 3.6a and 3.6c unchanged and green.
- `agentic validate` over `templates/` and `fixtures/`: green, the fixture's document step included; the sample's one fan-out warning (3.6a) unchanged.
- `just check` at the end.
- No `cargo clean`, and no build of a workspace copy against the shared target (AGENTS.md, owner 2026-11-23).
- Merge only with both CI jobs green after review triage (owner, 2026-11-24).

## Implementation Notes

### API the tests fix (tester, 2026-11-24): the document and Draft items

*Moved word for word from 3.6's plan in the re-slice (2026-11-24); 3.6a's plan keeps the rest.*

- `Decision::allowed_for(InboxKind::Draft) == &[Decision::Dismiss]`.
- `WriteCommand::SettleAndFinishDocumentStep { call_id: String, usage: Option<Usage>, cost_micro_usd: Option<i64>, step_run_id: String, output: String, version_id: String, document_id: String, item: NewInboxItem }` *(tester's choice of `version_id` and `document_id` fields: the engine mints the version's UUIDv7, the writer numbers it)*. The item is `InboxKind::Draft` with `payload: Some(step_run_id)`.
- `pub struct DocumentVersionRecord { pub id: String, pub document_id: String, pub version: i64, pub run_id: String, pub attempt: i64, pub step_run_id: String, pub taint: Taint, pub bytes: i64, pub created_at: String }`.
- `Reader`: `document_versions(&self, document_id: &str) -> Result<Vec<DocumentVersionRecord>, StoreError>` (version order).
- Audit: `document.versioned` with args `version`, `taint`, `bytes` (and the document id somewhere in the entry). No Markdown, no new `*_sha256` key.
- `am_engine::run`: a plan whose step declares a document with `output_schema`, a loop-back or a gate, or a document on a gate, is refused by `run_plan` with an `Err` naming the step key and no run row (the variant is the implementer's).
- A document step's success writes one version and one Draft (`taint` the attempt's; title and body name the document id and version, never the Markdown); an empty, whitespace-only, NUL-holding or over-1 MiB final message fails the step with a Failure item and no version.
- **am-config**: `Step` gains `pub document: Option<String>` (key `document:`).
- **Fixture**: `summarise` (`document: summary-note`) in workflow `record`.
- *(Migration, re-slice: `document_versions` and `step_runs.writes_document` are migration 0012 (renumbered from 0011 by the second re-slice), `SCHEMA_VERSION` 12, report line `from version 11 to 12`.)*

**Not written, and why (tester, 2026-11-24):**
- The plan's `#[cfg(test)]` unit test in `crates/am-store/src/lib.rs`, `a_non_human_resolve_of_a_draft_is_refused_in_the_writer` (row 39): the tester may change only doc-comment lines in `src/`. `rows.rs`'s guard `the_writer_unit_test_for_a_non_human_draft_is_there` stays red until it exists. Intended body, for the implementer: make a Draft (`StartAttempt { writes: AttemptWrites { seat, table: None, document: Some("summary-note") }, .. }`, `ReserveCall`, `SettleAndFinishDocumentStep { item: Draft, payload: the attempt }`); then for `Actor::System { component: "engine" }` and an `Actor::Seat` of the run, `w.send(actor, Command::Resolve(Resolve { item_id, decision: Decision::Dismiss, note: None }))` is `Err(StoreError::Invalid(_))` with the item open, no decision, `resolved_by` None and the audit count unchanged; the same as `Actor::Human { person: "owner", hat: REVIEWER_HAT }` resolves it. *(re-slice: the guard moves with the store tests to `documents.rs`.)*

**Ambiguities resolved (tester, 2026-11-24), this slice's:**
- Row 29's "FakeEgress sees no request": a run has no egress client at all; the test asserts no `egress*` audit action and that the hostile Markdown's image host is in no audit entry, and reads the Draft's payload over the API as a JSON string.
- Row 30's TTY tests are in `gate_cli.rs` (am-app's in-process `agentic gate` on a `FakeTerminal`); `gate_tty.rs` only gains a literal (3.6a's).
- `documents_versions.rs` row 28 covers only the document and gate cases.

### Reconciliation with the built API (tester, 2026-11-25)

The 2026-11-24 patch was applied on `story/3-21` (stacked on 3.6c, `story/3-22`) and checked against the API as built by the store slice, the ToolGate slice and 3.6c. Every planned signature fits as written: `WriteCommand::SettleAndFinishDocumentStep` and its fields, `DocumentVersionRecord`, `Reader::document_versions`, `Decision::allowed_for(Draft) == [Dismiss]`, `Step.document`, `StepPlan.document` and `AttemptWrites.document` (both already there, refused), the `document.versioned` audit, migration 0012 and `SCHEMA_VERSION` 12. None is changed. The choices below place helpers or extend tests; none changes the API.

- **`versions_of` is not in `tests/engine/support/mod.rs`.** That module is included by 29 am-e2e targets. Importing `DocumentVersionRecord` there stopped all of them from compiling, golden run included. The helper (`Reader::document_versions`, unchanged) is now a local function in the three new files: `documents_versions.rs`, `taint_documents_at_rest.rs` and `taint_documents_untrusted_markdown.rs`. The implementer may move it back into support once the reader exists. `DOCUMENT` and `documenting` stay in support, because they compile today.
- **`rule_16_row_provenance.rs` keeps 3.6a's rows-only run.** The patch had moved all four tests onto a document run. Now `every_row_change_has_an_after_image_naming_run_step_and_seat` stays on `rows_and_a_note`, as at HEAD, and stays green. `audit_holds_ids_never_row_content`, `the_chain_verifies_after_row_and_document_writes` and `no_new_sha256_arg_of_free_text` use `rows_and_a_document`. `no_new_sha256_arg_of_free_text` was moved by the patch and is not in the Tests-first list. It is the document half of row 22's "no new `*_sha256`", so it is counted in this slice. The byte check compares `document.versioned`'s `bytes` with the Markdown's length rather than with `versions_of`, so the file compiles and 3.6a's test in it runs.
- **Row 28 gains a control.** `a_mixed_declaration_is_refused_before_any_row` would pass as soon as it compiled, because 3.6a's `check_writes` refuses every document. It now ends with a control: `first`, then the document step with none of the mixes, is `Ok` and `succeeded`. This shows the refusal is the mix's.
- **3.24's `GateContext.clock`** is set (`FakeClock::at(CLOCK)`) in `the_cli_prints_a_draft_neutralised`.
- **The fixture's third step, in the earlier slices' tests.** Two existing tests now script a fourth model turn for `summarise` (the Markdown) so that they pass once the build lands: 3.6a's `plan_of_maps_grants_team_table_and_the_row_tool` and 3.6c's `agentic_run_leaves_the_row_input_slot_empty`. The 3.6c test now expects four requests and also checks that `summarise`'s request holds no row text.
- **Migration 0012 tests (row 41).** They are new in `documents.rs`, in 3.6c's 0011 pattern.
  - The version-11 fixture holds runs, attempts with journaled writes, calls with their ledger rows, tool calls of each result kind, rows with after-images, a `row` input edge (`step_inputs.from_row`), a gate note, Inbox items, prompt parts, a trigger and a fire. It was checked by hand against a version-11 database before it was committed to the file.
  - `a_failing_0012_names_the_backup_and_leaves_version_11` forges `step_runs.writes_document` by hand, as 0011's test forged `from_row`.
  - The column test also asserts that `document_versions` has no `text`, `markdown` or `output` column (AD-43: the Markdown stays on the attempt's output).
  - The 11 → 12 step is found by its `from`, not assumed last, as 3.6a and 3.6c did.
- **Supersession.**
  - `inbox_resolve.rs`: Draft left the "takes none" list and the later-story list (the patch did this; checked).
  - 3.6c's 0011 tests and 3.6a's 0010 tests: none pins `SCHEMA_VERSION == 11` or calls a migration "this story's". They already find their step by its `from` (`row_inputs.rs` "3.6b adds 0012 after it"), and their "exactly at version 10/11: no `document_versions`" checks stay true. Nothing changed.
  - 3.6a's `a_document_value_is_refused_until_3_6b` (store): removed by the patch. `a_document_version_and_its_draft_commit_with_the_step` covers it, because it journals `writes.document` and reads it back.
  - No test pins `run_on`'s "documents are versioned from story 3.6b on" refusal. It lives only in `am-engine/src/run.rs` `check_writes`, which is the implementer's. Row 28's test and its control now fix the new expectation.
- **Golden rule (3.24).** This slice adds no F1 capability. F1's `document:` mentions in `templates/samples/…/workflows/f1.yaml` are hand-off payloads (`handoff: payload: document:…`), removed from the golden copy until 3.15. F1's steps name `write_document`, which stays refused. No F1 step has a step-level `document:` key. `fixtures/golden/so-far/` is unchanged.

### Red run of 3.6b

*Tester, 2026-11-25, on Opus, in the real working tree with the shared cache. No `cargo clean`, no copy, no git change. The only `src/` change is doc-comment lines: the `compile_fail` doctest of a seat dismissing a Draft in `crates/am-store/src/lib.rs`, from the patch, whose wording now says 3.6b.*

**Commands:**
```
cargo fmt --all
cargo build --tests --keep-going   (see the note under "Everything else")
cargo nextest run -p am-store --test inbox_resolve --test rows -p am-app --test gate_cli -p am-e2e --test inbox_resolve_human_only --test rule_16_row_provenance --test capabilities_tables_grant --no-fail-fast
cargo nextest run --workspace --exclude am-desktop --exclude am-store --exclude am-e2e --exclude am-app --exclude am-config --no-fail-fast
cargo nextest run -p <am-config | am-store | am-app | am-e2e> --lib --bins --test <every target but the six below> --no-fail-fast
cargo test --doc -p am-store -p am-domain
cargo xtask check-deps; cargo xtask denylist
cargo clippy -p am-e2e --test rule_16_row_provenance --test inbox_resolve_human_only --test capabilities_tables_grant -p am-app --test gate_cli -p am-store --test inbox_resolve --test rows -- -D warnings
agentic validate --template fixtures/projects/rows-documents
```

**Does not compile: six targets, each because a test in it pins this slice's API.**

| Target | Tests | Red because |
|--------|-------|-------------|
| am-store `documents` (10) | `a_document_version_and_its_draft_commit_with_the_step`, `a_document_audit_holds_ids_never_markdown`, `document_versions_number_per_document`, `document_versions_are_append_only`, `a_draft_is_dismissed_only_by_a_person`, `the_writer_unit_test_for_a_non_human_draft_is_there`, `migration_0012_keeps_every_row_and_the_pre_migration_copy`, `migration_0012_adds_document_versions_and_a_null_column`, `reopening_a_version_12_database_applies_nothing`, `a_failing_0012_names_the_backup_and_leaves_version_11` | no `am_store::DocumentVersionRecord` (E0432); no variant `WriteCommand::SettleAndFinishDocumentStep` (E0599, 2); no `Reader::document_versions` (E0599, 4); the `const` assert `SCHEMA_VERSION > 11` fails at build (E0080, `SCHEMA_VERSION` is 11). |
| am-e2e `documents_versions` (5) | `a_document_step_leaves_one_version_and_one_draft`, `a_second_run_adds_version_two`, `a_returned_document_step_adds_a_version_in_the_same_run`, `an_empty_document_fails_with_a_failure_item`, `a_mixed_declaration_is_refused_before_any_row` | no `am_store::DocumentVersionRecord` (E0425), no `Reader::document_versions` (E0599) |
| am-e2e `taint_documents_at_rest` (1) | `a_tainted_document_version_and_its_draft_carry_taint` | as above |
| am-e2e `taint_documents_untrusted_markdown` (2) | `hostile_markdown_is_stored_as_is_and_nothing_is_fetched`, `the_cli_prints_a_draft_neutralised` | as above |
| am-config `step_writes` (13) | this slice's `a_step_declares_its_document`, `a_document_step_takes_no_schema_loop_back_or_gate`, `a_fan_out_step_declares_no_document`, `one_document_belongs_to_one_team`; and 3.6a's 9 table tests | no field `document` on `am_config::Step` (E0609, 2). With `a_step_declares_its_document` switched off by hand for one run, then restored: 3.6a's 9 pass. Two of this slice's tests fail at `assert_keys_known` ("unknown key 'document'"), and `one_document_belongs_to_one_team` finds no error naming `Summary_Note`. |
| am-app `run_rows` (7) | this slice's `plan_of_maps_the_document_step`; 3.6a's and 3.6c's 6 | no `Reader::document_versions` (E0599). With that test switched off by hand for one run, then restored: `a_step_declaring_another_teams_table_is_refused_before_any_row` passes. The other 5 fail: "workflows/record.yaml:16: unknown key 'document' in step", from the fixture step. |

**Compiles and fails at run time, for this slice's reason:**

| Test (target) | Row | Red because |
|---------------|-----|-------------|
| `a_decision_must_fit_the_kind` (am-store `inbox_resolve`, superseded) | 30 | `Decision::allowed_for(Draft)` is `[]`, not `[Dismiss]` |
| `a_draft_cannot_be_dismissed_by_a_non_human` (am-e2e `inbox_resolve_human_only`) | 39 | `run_plan` is `Err(Writes { step: "summarise", why: "… documents are versioned from story 3.6b on …" })` |
| `audit_holds_ids_never_row_content`, `the_chain_verifies_after_row_and_document_writes`, `no_new_sha256_arg_of_free_text` (am-e2e `rule_16_row_provenance`) | 22 | the same `Writes` refusal |
| `a_draft_is_listed_and_dismissed`, `a_draft_takes_no_approve` (am-app `gate_cli`) | 30 | `agentic run record` exits 1: "unknown key 'document' in step" (the fixture step) |
| `every_tool_request_goes_through_toolgate` (am-e2e `capabilities_tables_grant`; 3.6a's test, plus the patch's one line about `summarise`) | 34 | no `run <id> <status>` line: the fixture's `document:` is an unknown key |
| `agentic validate --template fixtures/projects/rows-documents` (`just validate`) | — | exit 1: "workflows/record.yaml:16: unknown key 'document' in step" |

**Not in a test binary:** the plan's `#[cfg(test)]` unit test `a_non_human_resolve_of_a_draft_is_refused_in_the_writer` is the implementer's. Its guard `the_writer_unit_test_for_a_non_human_draft_is_there` is in `documents.rs` and stays red.

**Everything else is green.**
- **nextest, 1,628 tests run, 8 failed** (the 8 run-time reds above):
  - every default member except am-store, am-e2e, am-app and am-config: 359 passed, 2 skipped;
  - am-config without `step_writes`: 309 passed;
  - am-store without `documents`: 240 of 241 passed;
  - am-app without `run_rows`: 379 of 381 passed, 1 skipped;
  - am-e2e without its three new targets: 333 of 338 passed, 5 skipped.
- **Doctests** (`cargo test --doc -p am-store -p am-domain`): all pass, the new `compile_fail` included (8, 14, 3, 7).
- **`cargo xtask check-deps`** and **`cargo xtask denylist`**: pass.
- **fmt and clippy:** fmt is clean. Clippy `-D warnings` is clean on the changed targets that compile.
- **Note:** a plain `cargo nextest run` stops at the first target that does not compile (`step_writes`). That is why the run went package by package.
- **Note:** an early `cargo build --workspace --tests` in this round also built `am-desktop`, against AGENTS.md. No later command did.

**Test lines (against HEAD 2ac755b, staged and unstaged):**
- About 2,450 added and 85 removed in `crates/*/tests` and the am-e2e manifest.
- The applied patch brought about 1,806 added and 81 removed.
- The tester's own lines this round are about 674 added and 36 removed. Most of them are `documents.rs` +573 (migration 0012: about 330 lines of fixture and helpers, about 240 of the four tests). The rest: `run_rows.rs` +15, `documents_versions.rs` +29, `rule_16_row_provenance.rs` +30 −13, the three local `versions_of` helpers, and `support/mod.rs` −10 +2.
- Fixture: +8 −4. `src/`: 10 doc-comment lines, from the patch.
