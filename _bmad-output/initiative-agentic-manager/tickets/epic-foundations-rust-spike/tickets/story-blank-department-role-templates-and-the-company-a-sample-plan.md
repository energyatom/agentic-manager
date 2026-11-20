---
title: 'Blank department, role templates and the Company A sample'
type: feature
ticket: '5'
created: '2026-11-20'
status: done
baseline_revision: '1975f0f0947c32dbfd17acf6f929793e8701a833'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/initiative-agentic-manager/reference-workload.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/story-seat-anatomy-in-the-config-schema-plan.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `templates/` does not exist. Epic 1 R5 needs:
- the Blank department;
- the eight generic role templates;
- the optional "Sample company: Company A (two departments)", exactly as `reference-workload.md` defines it.

All of them must pass `agentic validate` in CI. Epic 3's golden run, epic 5's wizard and every later fixture load the sample through the generic loader.

**Approach:**
- Write the templates as **config files only**; no engine code changes.
- `just check` runs `agentic validate` over each template.
- The denylist keeps every sample id out of `crates/` and `web/src`.
- **Tests first (owner rule):** the checks that each template validates and that the sample holds its seats, flows, grant, accepts, skills, Sources and folder are written first and recorded red.

**Owner decisions (2026-11-20):**
- **(a) Template shape:** every template is a minimal project, so `agentic validate` checks it as it is.
  - The blank department is a `company.yaml` with one team.
  - Each role template is `templates/roles/<role>/` with a `company.yaml` and `seats/<role>.md`; the Coordinator's also holds its learning workflow.
  - The installer (epic 5) later copies only the team or seat part. No code change.
- **(b) Human seats:** a seat with no `model` is a human seat. That covers the sample's Owner and the Human seat template. No schema key is added.
- **(c) Company cap:** the ₹600 weekly cap for the whole company is left out, because `company.yaml` has no caps key. It is recorded for epic 4's budget stories; the per-workflow caps are written.
- **(d) Learning workflow:** the Coordinator's learning workflow ships with no trigger (manual only), so nothing runs until the owner adds a schedule.
- **(e) The Owner's unit (owner, 2026-11-20, at review):**
  - The Owner sits in a third team of its own, id `owner` (name "Owner"), which holds only the Owner. D1 and D2 are unchanged.
  - The sample has 10 seats: the nine AI seats plus the Owner. Where the matrix says "9 seats", it means the nine AI seats plus the Owner.
- **(f) Models (owner, 2026-11-20, at review; RULE-20):** no sample seat pre-selects a paid provider.
  - The five reference "frontier" seats (D1-Head, D1-E1, D1-E3, D2-Head, D2-E3) run on a local model by default, with their cloud model as the fallback.
  - The reference mix stays visible and is swapped to cloud in Brain.
  - With zero keys, F3's PDF and PNG routes cannot be read until the owner adds a cloud key or a local vision model.

## Boundaries & Constraints

**Always:**
- Every name is abstract and every row and file is synthetic. No domain words; no "Chief of Staff" or "Manager" template.
- Role templates are exactly Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist and Human seat. Each has:
  - instructions (body), a non-empty `goal` and `backstory`;
  - default tools, a default `budget` and a `capabilities` block (Folder access on, plus what the role needs: Researcher web_search and browse; Analyst tables and calculate; Coordinator handoff_ask and memory);
  - an ordered model list (`model` + `fallback`);
  - empty `skills`, `knowledge` and `links` lists.
  
  The Coordinator also carries the learning workflow and is the default seat for Ask a seat.
- **Sample people and seats.** The Owner is human, with hats CEO, Vision Creator, Reviewer and Developer. D1 is D1-Head and D1-E1..E3; D2 is D2-Head and D2-E1..E4. Each seat has a goal, backstory, avatar tint, the reference model mix (D1-E2 is D1's local seat) and only Built-in capability providers:
  - D1-E1..E3: web_search and browse;
  - D2 seats: tables.
- **Sample data:**
  - `table-t1`: key `item_id`; status transitions new → processed → handed-off, and dismissed.
  - Saved view "Open items".
  - The grant D2 → D1 (read) on `table-t1`.
  - Each department's `accepts`.
- **Flows:**
  - **F1:** fan-out to D1-E1..E3; fan-in at D1-Head, quorum 3 of 3, timeout 30 min, `on_branch_failure: ask`; hand-offs D2 → D1 and D1 → D2; D1-Head's review gate; the Owner's gate.
  - **F2:** `f2-update` runs weekly (Monday 09:00, catch-up once); `f2-d1` runs on `workflow_completed:f2-update`; `f1` runs on `workflow_completed:f2-d1`.
  - **F3:** `f3-folder` fans out over `d1-inputs`: `*.pdf` → D1-E1, `*.csv` → D1-E2 (`text_fallback` on), `*.png` and `*.jpg` → D1-E3. D1-Head gathers into `d1-folder-digest`. The folder is linked read-only to D1-E1..E3 and D1-Head.
- **Sample folder and files:** `files/d1-inputs/` holds two PDFs (one with no text layer), three csv files, two PNGs and one docx.
- **Skills:** `structured-review` (D1-E1..E3) and `plain-report` (D2-Head), with no scripts.
- **Sources:** `t1-drop` (Local Repository, owned by D2-E1 → `table-t1`, key `item_id`, delete is the default) and `d1-notes` (D1's team Knowledge).
- **Caps** (₹; parent placeholders): ₹150 per scheduled run, ₹250 per manual run, ₹600 per week for each sample workflow. Seat budgets come from the role templates.
- Fan-out, fan-in, hand-off and review-gate fields are written in the AD-40..AD-42 shapes under the keys 1.12 accepts unchecked.
- The denylist covers every sample id; YAML has no comments; `just check` and CI are green.

**Never:**
- Change engine, server or UI code.
- Add a domain department template.
- Put a sample name in `crates/` or `web/src`.
- Commit a real document or key.
- Add a per-format reader.
- Edit the reference spec, the architecture or tickets.

## I/O & Edge-Case Matrix

| Scenario | Input | Expected |
|---|---|---|
| Blank department | `agentic validate templates/departments/blank` | exit 0, no errors |
| Each role template | `agentic validate` on each of the 8 | exit 0; the seat has a non-empty goal and backstory and empty skills, knowledge and links |
| Sample | `agentic validate templates/samples/company-a` | exit 0. The snapshot holds the nine AI seats plus the Owner (decision e), both teams and their accepts, the hats, `table-t1` (key, transitions, view, grant), workflows `f1`, `f2-update`, `f2-d1` and `f3-folder` with their triggers and caps, both skills, both Sources, and `d1-inputs` linked to 4 seats |
| Grants by capability | the sample's D1-E1 / D2-E1 / D1-E2 | web_search and read_website / table_write / read_file plus text_fallback, all on the builtin provider |
| Denylist | the sample ids | every seat, table, document, workflow, Source, skill, folder and template id is a seed or derived term; the scan of `crates/` and `web/src` stays clean |
| Gate | `just check` | runs validate over every template and the valid fixtures; green |

</frozen-after-approval>

## Code Map

- **`justfile`** (:39-48), `validate` recipe. Today it loops over `fixtures/projects/valid*/`; the comment at :42 says templates join here. Add a loop over `templates/departments/*/`, `templates/roles/*/` and `templates/samples/*/`.
- **`crates/am-config`** -- **do not change.**
  - `company.yaml` is required, with `name`, `timezone`, and a three-letter `currency`. Company keys: `name, description, timezone, currency, week_start, max_chain_depth, teams[id,name,description,parent,accepts], hats[id,name,description]`. There is no company `caps` key.
  - Workflow keys: `name, team, description, triggers, caps{per_run, scheduled_per_run, per_week, per_month}, steps`.
  - Step keys: `id, seat, instruction, description, output_schema, max_tokens`, plus the unchecked keys `seats, join, over_folder, routes, files, handoff, review, max_parallel`. A step with `seats` or `over_folder` may leave out `seat`.
  - Triggers are `schedule{cron, timezone, catch_up}` or `event{on, only_if}`; no trigger means manual.
  - Tables: `key, columns[id snake_case, type, required, values, transitions[from,to]], views[id,name,columns], grants`.
  - Sources follow 1.12's reader: `local-repository` takes no account; output is `table`+`key`+`mapping` or `knowledge: team`.
  - Seat keys follow the 1.12 plan.
  - **Money:** written as plain numbers in the company currency (`per_run: 1.5`).
  - **Documents:** there is no document file kind. Documents are database-owned (AD-35); the sample names them in hand-off payloads (`document:<id>`), in `document_updated:` triggers and in step instructions that use `write_document`.
- **`xtask/denylist.txt`** (:12-46) already seeds every Company A id: `company-a`, the seat ids, `table-t1`, the documents, `t1-drop`, `d1-notes`, `d1-inputs`, both skills and the workflows.
  - Derived terms come from hyphenated file stems under `templates/` and `fixtures/` (`xtask/src/denylist.rs:26-32`); folder names and `SKILL` are not derived.
  - So role-template file stems must not be hyphenated, or a generic word like `human-seat` would become a banned term in `crates/`. Generated sample file names in `files/` should be single words too.
- **`.gitignore`** ignores `*.png` except under `fixtures/`, and ignores `.agentic/` except `fixtures/projects/*/.agentic/`. Re-include `templates/**/*.png`, and `templates/samples/*/.agentic/knowledge/` if team Knowledge notes ship.
- **Tests:**
  - `crates/am-e2e/tests/cli_validate.rs` has the `validate(args)` and `fixture` helpers; add a templates test.
  - `crates/am-config/tests/` has the helpers in `common/mod.rs`; add `templates.rs` for the snapshot assertions.
  - `xtask/tests/denylist.rs` gets a test that every sample id is denied.

## Tasks & Acceptance

**Execution:**
- [x] `crates/am-config/tests/templates.rs`, `crates/am-e2e/tests/cli_validate.rs`, `xtask/tests/denylist.rs` -- FIRST. Write one test per matrix row, plus a test that walks every role template for its fields. Run them and paste the red output into Implementation Notes.
- [x] `templates/departments/blank/` -- the blank department.
- [x] `templates/roles/<role>/` -- the eight role templates; the Coordinator also holds the learning workflow.
- [x] `templates/samples/company-a/` -- `company.yaml` (teams `d1` and `d2` with their accepts, the hats), `models/accounts.yaml`, `tools.yaml`, the nine seats, `tables/table-t1.yaml`, workflows `f1`, `f2-update`, `f2-d1` and `f3-folder`, `schemas/` as the steps need, both skills, both Sources, `files/d1-inputs/` and the team Knowledge notes.
- [x] `files/d1-inputs/` content -- generate the synthetic PDFs, PNGs, docx and csv with a throwaway script, not committed. Each file holds a known word.
- [x] `justfile`, `.gitignore`, `xtask/denylist.txt` -- the validate loop, the re-includes, and any sample id not yet seeded.
- [x] `docs/config-schema.md` -- a short "Templates" section saying where each template lives and what installing one copies.

**Acceptance Criteria:**
- Given the sample and `reference-workload.md`, when a reviewer compares them, then every seat, department, document id, Table T1, the grant, the accepts lists, flows F1–F3, the sample folder and its links, the skills, the Sources and the caps are found.
- Given the workspace, when `just check` runs, then it is green, and CI on `story/1-5` is green.

## Implementation Notes

**Red run (2026-11-20, before any template existed).** Tests first: `crates/am-config/tests/templates.rs` (13 tests: blank validates as one team; blank is the only department template; the eight role folders; every role template's fields; the sample's people and teams; Table T1; workflow triggers and caps; F1's fan-out, fan-in, hand-offs and both gates; F2 and F3; the folder links; the synthetic files; grants by capability; skills, Sources and Knowledge), four cases in `crates/am-e2e/tests/cli_validate.rs` (blank, each role, the sample, every template folder: exit 0 with `0 errors, 0 warnings`) and `every_sample_id_is_denied` in `xtask/tests/denylist.rs`. `cargo nextest run --no-fail-fast -p am-config -p am-e2e -p xtask -E 'binary(templates) | binary(cli_validate) | binary(denylist)'`:

```
Summary [0.469s] 34 tests run: 16 passed, 18 failed, 0 skipped
  templates/departments/blank does not exist
  templates/roles/head does not exist
  templates/samples/company-a does not exist                       (x6)
  templates/samples/company-a/workflows/f1.yaml: No such file or directory
  templates/samples/company-a/workflows/f2-update.yaml: No such file or directory
  templates/departments: No such file or directory                  (cli_validate)
  templates/departments exists: NotFound / templates/roles exists: NotFound
  the sample folder exists: NotFound
  the sample ships at …/templates/samples/company-a                 (xtask denylist)
```

The 16 that passed are the existing tests of the three binaries.

**Green run (2026-11-20).** Same command: 34 passed. `agentic validate` on each of the 10 template folders: `0 errors, 0 warnings`. `just check` green: fmt, clippy `-D warnings`, nextest (232 passed, 6 skipped), check-deps, check-forbid, denylist (`96 files clean against 25 seed and 0 derived terms`: every derived stem is already seeded) and validate over `fixtures/projects/valid*/` and every template. A mutation probe (removing `f1` from `xtask/denylist.txt`) turns `every_sample_id_is_denied` red with "`f1` is not denied". CI on `story/1-5` not yet run: nothing is committed.

**What was written.**
- `templates/departments/blank/company.yaml`: one team, `department`.
- `templates/roles/{head,coordinator,analyst,researcher,reviewer,writer,specialist,human}/`: a wrapper `company.yaml` (team `team`, UTC, USD) and `seats/<role>.md`. AI roles: `local-lmstudio` › `gpt-oss-20b`, fallback `local-ollama` › `qwen2.5:7b-instruct`. Capabilities: Folder access on in every role; Researcher web_search and browse; Analyst tables and calculate; Coordinator handoff_ask and memory. `skills: []`, `knowledge: []`, `links: []` are written out. The Coordinator's description names it the default seat for Ask a seat, and `workflows/learning.yaml` (two steps, review then propose with `propose_change`, no trigger, caps 30 per run and 60 per week) is its learning loop.
- `templates/samples/company-a/`: `company.yaml` (INR, Asia/Kolkata, teams `d1` accepting `d2/f1` and `d2` accepting `d1/f2-d1`, hats CEO, Vision Creator, Reviewer, Developer), `models/accounts.yaml` (`anthropic`, `openai`, `secret_ref` only), `tools.yaml` (Local search, no egress hosts), ten seats, `tables/table-t1.yaml`, workflows `f1`, `f2-update`, `f2-d1`, `f3-folder`, skills `structured-review` and `plain-report` (no scripts), Sources `t1-drop` and `d1-notes`, `files/d1-inputs/` (`brief.pdf` with a text layer, `scan.pdf` drawn only, three csv, two png, `memo.docx`; known words AMBER … HAZEL) and `.agentic/knowledge/team-d1/{notes,glossary}.md`. No `schemas/` were needed.
- The synthetic files were made with a throwaway stdlib Python script in the session scratchpad, not committed.
- `justfile` validate loop, `.gitignore` re-includes (`!templates/**/*.png`, `!templates/samples/*/.agentic/`), `f1` added to `xtask/denylist.txt`, and a "Templates" section in `docs/config-schema.md`.

**Changes outside the listed files (recorded).**
- `fixtures/templates/sample.json` (new): the sample's expected ids and values. The plan put the snapshot tests in `crates/`, which the AD-23 scan covers, so the first draft named `d1-head`, `table-t1` and the rest literally and turned `current_repo_passes` red with 104 findings. The tests now read every sample id from this file under `fixtures/` and stay generic; the xtask test, outside the scan, names the ids literally.

**Interpretations (recorded, not decisions).**
- **Seat count.** The matrix says "9 seats" and the task "the nine seats", but the Owner is also a seat (`owner`, named by `review: { seat: owner }` and by both heads' `reports_to`). The sample has the nine AI seats plus the Owner, ten in all, and the tests assert that.
- **Owner's team.** A seat needs a team and the sample has only `d1` and `d2`; the Owner sits in `d2`, the department whose Report R1 the Owner approves.
- **`owner` is not denied.** It is the one sample seat id left off the denylist: `owner` is ordinary product vocabulary (`owner_seat`, `Actor::Human`). Team ids `d1` and `d2` are not denied either; the plan's list (seat, table, document, workflow, Source, skill, folder and template ids) does not name teams.
- **D1-Head has `tables`.** F2's `f2-d1` has D1-Head read `table-t1` through the grant, which needs `table_read`. Every other sample capability is as the plan lists: D1-E1..E3 web_search and browse, D2 seats tables, all Built-in.
- **Sample roles.** D1-Head and D2-Head are Heads, D1-E1..E3 Researchers, D2-E1..E4 Specialists with tables on (the Analyst template would add calculate, which the plan does not list for D2).
- **Owner's gate.** The last F1 step, `approve`, has `seat: owner` and `review: { seat: owner }`, so the Owner's work there is the Inbox review only. D1-Head's gate is on the `report` step's hand-off to `approve`.
- **Hand-off targets.** Into a step of the same run: `d1/f1/branches` (D2 → D1) and `d2/f1/report` (D1 → D2), in AD-41's `<department>/<workflow>/<step>` form. F1's fan-in is `wait: quorum, quorum: 3` ("quorum 3 of 3"); F3's gather is `wait: all`. Durations are written `30m`.
- **Budgets.** Role-template budgets are company-currency placeholders sized to the sample's ₹ caps: Head 60 per run and 240 per week, other AI roles 30 and 120, Human seat 0 and 0 (it makes no model calls). F1's fan-out (3 × 30) plus D1-Head (60) fits the ₹150 scheduled-run cap. In a USD company the same numbers are generous; the owner may want smaller defaults.
- **"Open items" view.** Its filter (status new or processed) is in the reference but not written: a view has only `id`, `name` and `columns` today.
- **Wrapper company settings.** Blank and role templates use UTC and USD only so they validate; the installer copies only the team or seat part.

**For later stories (not done here).**
- The ₹600 company-wide weekly cap (decision c) waits for a company caps key (epic 4's budget stories); it is noted in the sample's `company.yaml` description.
- The `*.xlsx → D1-E2` refusal route, the CEO goal and the Vision Creator note stay out, as the Design Notes say.

**Review fixes (2026-11-20, pass 1; same implementer).** These replace the interpretations above where they differ.
- **Owner's unit (decision e):** a third team `owner` holds only the Owner; `owner.md` has `team: owner`. Tests assert three teams and the Owner's team.
- **Local models first (decision f):** D1-Head, D1-E1, D1-E3, D2-Head and D2-E3 run on `local-lmstudio` › `gpt-oss-20b`, each with its former cloud model as the only fallback. D1-E2 is unchanged. Tests assert every AI seat's model is local and those five fall back to cloud.
- **Accepts:** `d2` now has `accepts: []`; `d1` keeps `d2/f1`.
- **F1:** `produce` hands off to `d2/f1/branches` and, after writing document-a, moves each row it used from processed to handed-off with `table_write`, giving the reason (D2-Head lists `table_write`). `approve` has no step-level `review`: the Owner's human step is the Inbox gate.
- **F2 shares:** D2-E1 to D2-E4 each work only on their share of rows, by the last digit of `item_id` (0–2, 3–4, 5–7, 8–9), which their seat instructions state; the shared step instruction says to work only on that share.
- **Sources:** `t1-drop` maps `status` (the drop files carry `new`) and loads Mondays at 08:00 company time; `d1-notes` loads daily at 07:00 with limits of 100 items, 10 MB and 1 load a day.
- **`.gitignore`:** only `templates/samples/*/.agentic/knowledge/` is re-included. `git check-ignore -v` shows a `state.db` there ignored by `templates/samples/*/.agentic/*` and the notes tracked.
- **Denylist:** `d1` and `d2` are seeded. The scan of `crates/` and `web/src` has no real hit; the only hit was a message in the new test, now reworded.
- **Label:** the sample's `company.yaml` `description` starts with "Sample company: Company A (two departments)" and is quoted, because of its colon.
- **Run:** `cargo nextest run --no-fail-fast -p am-config -p am-e2e -p xtask -E 'binary(templates) | binary(cli_validate) | binary(denylist)'` gives 34 passed. `agentic validate templates/samples/company-a` gives 0 errors and 0 warnings. fmt and clippy are clean on am-config's tests.

**Caller re-check (2026-11-20):**
- `just check` green: 232 passed, 6 skipped; denylist clean against 27 seed terms; every template validates with 0 errors and 0 warnings.
- `git check-ignore`: a `state.db` in the sample's `.agentic/` stays ignored, and the knowledge notes are tracked.
- The Anthropic fallback examples were updated to the current model ids (`claude-sonnet-5-5`, `claude-haiku-5-5`).

## Plan Change Log

## Review Triage Log

### Pass 1 (quick lens plus the caller's own check, 2026-11-20): high 0 · medium 6 · low 6 · false 0 · maybe-false 0 · owner-decided 2

The lens found every template validating, and the matrix rows covered by passing tests (`just check`: 232 passed).

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | The Owner sits in team `d2`, and the sample has 10 seats against the matrix's "9" | medium | intent_gap → owner decided (e); patch | Reference org chart: the Owner is its own unit. `owner.md` has `team: d2` only because every seat needs a team. Decision (e) adds the `owner` team; the matrix meant the nine AI seats plus the Owner. |
| 2 | Five seats pre-select paid cloud models (RULE-20) | medium | intent_gap → owner decided (f); patch | `d1-head`, `d1-e1`, `d1-e3`, `d2-head`, `d2-e3` have an `anthropic` or `openai` `model`. Decision (f): local first, cloud as fallback. |
| 3 | `d2` accepts `d1/f2-d1`, which admits no hand-off; every `accepts` entry is privilege-widening | medium | patch | `f2-d1.yaml` declares no `handoff`; its link to `f1` is an AD-44 event trigger. F1's hand-back goes into a step of D2's own workflow `f1`, so no D2 entry is needed. Fix: `d2` `accepts: []`. The plan's Design Notes were wrong here. |
| 4 | `produce.handoff.to: d1/f1/branches` names `f1` under team `d1` | low | patch | `f1` is `team: d2`, and the other targets use `d2/f1/...`. Fix: `d2/f1/branches`. |
| 5 | The Owner's gate is a step-level `review` outside any hand-off (AD-42) | low | patch | Step `approve` has both `seat: owner` and `review: {seat: owner}`. The Owner's human step is the Inbox gate (reference row 7). Fix: drop the step-level `review`. |
| 6 | F2's four branches all process every new row, and can race on transitions | medium | patch | One shared instruction with no split. Fix: each D2-E seat's instructions name its share of rows by the last digit of `item_id`, and the shared instruction says to work only on that share. |
| 7 | No step moves a row to `handed-off`, so the same rows feed every F1 run | medium | patch | `produce` reads the processed rows and never moves them. Fix: `produce` moves each row it used from processed to handed-off with `table_write`, giving a reason. |
| 8 | `t1-drop` leaves the required `status` column unmapped | low | patch | The mapping covers `item_id`, `received` and `text`. Fix: map `status` too; the drop files carry `new`, as the Source description says. |
| 9 | The Sources have no schedule, and `d1-notes` has no limits (epic R5) | low | patch | Epic R5 lists a schedule and limits for each Source. Fix: `t1-drop` on Mondays at 08:00 company time (before F2 at 09:00); `d1-notes` daily, with limits. |
| 10 | `.gitignore` re-includes each sample's whole `.agentic/`, runtime files included | medium | patch | The Code Map asked for `.agentic/knowledge/` only. A run against the sample in place would leave `state.db` and logs untracked and committable. Fix: re-include only `.agentic/knowledge/`. |
| 11 | Team ids `d1` and `d2` are not on the denylist | low | patch | AGENTS.md bans department names in `crates/` and `web/src`. Fix: seed both, if the scan stays clean; report a real hit instead of working around it. |
| 12 | The `f1` seed blocks the token "F1" anywhere in `crates/` and `web/src` | low | reject | By design: AGENTS.md bans sample ids, and the frozen matrix requires every workflow id denied. Later stories read sample ids from fixtures, as `fixtures/templates/sample.json` does now. Noted for the golden-run stories. |
| 13 | The sample ships no drop files or rows for T1 | low | defer | `.agentic/drop/` is runtime, and the ticket does not list drop files. The golden run's fixtures (epic 3) supply rows. Recorded in deferred-work. |
| 14 | The label "Sample company: Company A (two departments)" appears nowhere | low | patch | No label key exists. Fix: start `company.yaml` `description` with the label text. |

## Design Notes

- **Accepts:** `d1` accepts `d2/f1`, because the fan-out and the review come into D1. `d2` accepts nothing: F1's hand-back goes into D2's own workflow, and `f2-d1` → `f1` is an event trigger, not a hand-off (amended at review, finding 3).
- **Gates:**
  - D1-Head's review gate sits on F1's report hand-off: `review: { seat: d1-head, max_returns: 2 }`.
  - The Owner's gate is a human review gate on the last step: `review: { seat: owner }`.
  - These shapes follow AD-41 and AD-42. Stories 3.14–3.16 and 4.15 may rename them when they add the checks.
- **Sample settings:** `currency: INR`, because the reference caps are ₹, and `timezone: Asia/Kolkata`. These are template data, not code; the wizard can replace them.
- **Models:**
  - Local seats use the reserved `local-lmstudio` or `local-ollama` Account.
  - Frontier seats name sample Accounts in `models/accounts.yaml` with a `secret_ref` only, never a key.
  - Model names are examples.
- **Left out on purpose:**
  - The `*.xlsx → D1-E2` refusal route from the reference spec is not written. The ticket lists only three routes, and the refusal warning belongs to a later story.
  - The CEO goal and the Vision Creator guidance note are not written, because `company.yaml` has no goals or guidance keys yet and the ticket does not list them.

## Verification

**Commands:**
- `cargo nextest run -p am-config -p am-e2e -p xtask -E 'binary(templates) | binary(cli_validate) | binary(denylist)'` -- expected: red first (recorded), then green.
- `for d in templates/departments/*/ templates/roles/*/ templates/samples/*/; do cargo run -q -p am-app --bin agentic -- validate "$d" || echo FAIL $d; done` -- expected: no FAIL.
- `just check` -- expected: green.

