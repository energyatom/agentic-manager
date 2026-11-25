---
title: 'Golden run so far on every PR'
type: feature
ticket: 24
created: '2026-11-25'
status: approved
baseline_revision: '7611520aaa472b6d0f7057e9665dd9663ae669fd'
after: [23]
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: []
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/docs/deferred-register.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/reference-workload.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/epic-workflow-engine-cli.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-table-write-through-toolgate-plan.md'
warnings:
  - 'Written from code reading only: no build or test was run while drafting (another PR''s CI was running). The stop point and its words below are confirmed or corrected by this story''s first run, before any non-test src/ change.'
  - 'Reading to confirm (Questions, Q3): besides the owner''s removed keys, the copy also leaves out the steps from `branches` on. With only `produce`''s hand-off removed, `agentic run` would refuse the copy at `branches` and nothing would run.'
deferred: []
---

<intent-contract>

## Intent

**Story:** epic 3 entry 24, "Golden run so far on every PR", `after = [23]`, `hitl = false`, risk medium. It is part 1 of the owner's quality plan of 2026-11-24 (epic Notes, "Quality plan"). Build order: 6, 23, **24**, 25, 22, 21. Its baseline is set when it starts, on main after the ToolGate slice (#41) merges.

**Problem:** nothing runs the Company A sample end to end on a PR.
- `just check` validates the sample with `agentic validate --template` (justfile L78-L84), but no test runs any of its flows.
- Story 3.13's golden run builds last in the epic, so a regression in a flow could go unnoticed for many stories.
- AD-33 names the golden e2e: F1 through the generic loader on FakeLlm, one script per seat, with snapshots. The Structural Seed places it in `crates/am-e2e/tests/golden/`, which does not exist yet.

**How far the real F1 runs on what is built (3.1 to 3.6, 3.19 and 3.23), from code reading:**
- `agentic run f1` on a fresh copy of the sample creates no run.
  - `plan_of` (`crates/am-app/src/run.rs` L399) checks every step for run-graph keys before it asks for any provider (L426-L437).
  - F1's first step `produce` declares `handoff:` (`f1.yaml` L16-L18), which am-config lists in `Step::run_graph` (`kinds.rs` L2102-L2111, L2385).
  - The fan-out table check comes first (L418-L425) and stays silent: no `branches` seat holds `table_write`.
- **Stop point:** step `produce`, before a run row, with exit 1 and these words on stderr:
  `agentic: step `produce` of the workflow `f1` declares `handoff`, which this version of `agentic run` cannot honour yet: it runs each step as one plain model turn and would skip it without saying so. No run was started`
- **Why it stops there:** the cross-department hand-off (D2 → D1, payload `document:document-a`) needs 3.15 and 3.21. The steps after it need more:
  - `branches`: a fan-out with `seats` and `max_parallel`, for 3.14;
  - `consolidate`: `join` for 3.14 and `handoff` for 3.15;
  - `report`: `handoff`, with `review` nested, for 3.15 and 3.16;
  - `approve`: a Human seat with no `gate:` (Design Notes, gap G1).
- Story 3.1's rule refuses the whole run before any row, so the real F1 cannot start until 3.14, 3.15 and 3.16 have all landed.
- **What is left up to there:**
  - **the real F1:** no run, no step runs, no rows or history, no Inbox items, no ledger entries and no model call;
  - **the cut-down copy (owner, 2026-11-25):** one run `succeeded`. `produce` has attempt 1 `succeeded` with taint 0 and one model call (purpose `step`). There is one ledger entry, reserved 0 and settled 0 (`reserved_micro_usd: 0` until 3.7, `am-engine/src/run.rs` L1280). No tool is offered or called, and there is no Inbox item and no row.
- **Rows and history stay empty (manager, 2026-11-25):** F1's first step produces Document A, which is a document, not rows (reference-workload.md, F1 step 1). The sample stays unchanged.

**Approach (owner-approved 2026-11-25):**
- **One new e2e target, `golden_so_far`,** at `crates/am-e2e/tests/golden/golden_so_far.rs`. `cargo nextest run` runs it, so `just check` and both CI jobs run it on every PR. It has no feature, no ignore and no recipe of its own.
- **It drives the shipped commands in-process:**
  - `am_app::cli::validate --template`, as `just validate` does;
  - `am_app::cli::run` on fresh temporary copies of the sample, as the engine rig's `Cli` does (`tests/engine/support/mod.rs` L534-L589).
  - It never builds an engine plan.
- **Two stages per pass (owner: "Both"):**
  - **(1) The real F1:** the sample copied as it is. Its refusal is pinned: the step, the exit code, the words, and no run row.
  - **(2) The cut-down F1 copy:**
    - a hand-written fixture, `fixtures/golden/so-far/workflow.yaml`;
    - it equals the real F1 with only the entries of the removed list in `fixtures/golden/so-far/golden.json` taken out;
    - today those entries are `produce`'s `handoff`, and the steps from `branches` on (Q3);
    - the test installs it over `workflows/<F1>.yaml` in a second temporary copy of the sample, before that copy's first open, then runs it on scripted FakeLlm;
    - its journal is the snapshot.
- **No drift (owner condition):** a test proves that the copy equals the real F1 minus exactly the removed entries. It fails, naming the difference, if anything else differs.
- **Fixtures only (owner condition):** the copy lives only in `fixtures/golden/so-far/`. It is never under `templates/`, it is never a template, and `agentic validate --template` never takes it as one. A test enforces this.
- **Everything with a Company A id lives in `fixtures/`:**
  - the sample's folder name and F1's id: `fixtures/templates/sample.json` (`sample`, `report_flow.workflow`);
  - in `fixtures/golden/so-far/`: the copy, the removed list, the FakeClock instant, one FakeLlm script per seat that has a model, and the expected snapshot.
  - The test spells none of these ids (AD-23; DR-016).
- **Snapshot:**
  - tagged text lines, with every step's lines tagged `step <key>`;
  - ids, times, the config hash and temporary paths are normalised, and the order comes from the journal, never from time;
  - a mismatch fails with a diff naming the step;
  - the actual snapshot is written under `$CARGO_TARGET_TMPDIR`, and the fixture is never rewritten by the test.
- **No network, no model, no keychain:**
  - providers come only from the golden factory: a seat router over FakeLlm for each F1 seat's model, and for anything else a tripwire on an empty `FakeEgress`;
  - no `SecretStore` exists on the path (Design Notes).
- **FakeClock (ticket):**
  - `RunContext` gains a `clock` port, which `plan_of` puts on the plan;
  - `agentic` passes the system clock, and the test passes `FakeClock`;
  - today `plan_of` hard-wires the system clock (`run.rs` L535);
  - this is the story's only shipped-code change.
- **Extension rule (owner):**
  - each later F1 story shrinks the removed list, edits the copy to match, and moves both expected results (the real F1's stop and the copy's snapshot) in the same commit;
  - 3.13 deletes the copy once the list is empty and the copy matches F1.

## Boundaries & Constraints

**Always:**
- **Tests first (owner standing rule):**
  - the red run is logged in this plan before any non-test `src/` change;
  - the only `src/` lines in the tests-first phase are inside `#[cfg(test)] mod tests` of `crates/am-app/src/run.rs`, as 3.23's doctests were.
- **Shipped path only (AD-33, "through the generic loader"):**
  - no `run_plan` or `run_plan_with_tools` call and no hand-built `StepPlan` in the golden;
  - the copy differs from F1 only as the removed list says.
- **Fakes only (AD-33):**
  - FakeLlm, one script per seat;
  - FakeClock through `RunContext.clock`;
  - `FakeProjectIdentity` and `FakeTerminal`;
  - a temporary HOME per stage;
  - no network, no real model and no keychain.
- **Fixtures own the vocabulary (AD-23):**
  - no seat, team, table, document, workflow or step id of the sample appears in `crates/`, comments included;
  - no "F1" token appears anywhere in `crates/` (DR-016): say "the sample's report flow";
  - `cargo xtask denylist` passes;
  - no file under `fixtures/golden/` has a hyphenated stem that the test spells. The denylist turns every kebab-case file stem under `fixtures/` and `templates/` into a term, so the copy is named `workflow.yaml`.
- **The copy is a fixture, never a template (owner):**
  - it lives only at `fixtures/golden/so-far/workflow.yaml`, and that folder has no `company.yaml`;
  - no `just validate` path names `fixtures/golden/`;
  - it is installed only into temporary copies.
- **The sample stays unchanged (owner):**
  - `templates/samples/company-a/` is read only;
  - the open writes `project_id` into `company.yaml`, so every open is of a temporary copy (`am_testkit::copy_tree`).
- **Expected snapshot written by hand:**
  - from this plan, never pasted from the test's own output;
  - byte counts of journaled prompt parts are computed from the sample's text with a stated command;
  - a difference at the first green run is triaged and recorded in Implementation Notes: was the expectation wrong, or the code?
- **Stable:** two passes in two temporary folders give the same normalised snapshot (row 7).
- **One story, one squash commit, on Opus.** Stop and report if the code passes about 4,000 lines.
- **Register (quality plan part 2):** this story's rows (Design Notes) go into `docs/deferred-register.md` in its commit.

**Never:**
- **No product behaviour change:**
  - `agentic run`'s refuse-before-any-run rule (check 8, `run.rs` L30-L36) and its words stay as they are;
  - no partial-run mode, golden-only seam, env var, flag or feature.
- **No change to the sample, the reference workload, the PRD, the architecture or the ticket.**
- **No escape from `just check`:** no `#[ignore]`, no `required-features`, no nextest filter and no separate recipe for `golden_so_far`.
- **No update mode** that writes `fixtures/` from a test.
- **No new crate:** the diff and the normaliser are hand-written. `sha2`, `hex`, `serde_json` and `tempfile` are already dev-dependencies of am-e2e.
- **No `cargo clean`,** and no build of a workspace copy against the shared `CARGO_TARGET_DIR`.
- **Outside this story:**
  - `just golden`, its variants and the Owner's gate through `agentic gate` (3.13);
  - lineage (3.17);
  - the real-model golden (epic 4);
  - F2 into F1, which first brings rows (epic 4 story 13);
  - the register check (3.25).

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Sample validates | `am_app::cli::validate(["--template", <copy of the sample>])`, before the copy is opened | Exit 0, `0 errors`. The `validate:` lines hold each finding (today the one fan-out warning on `f2-update`'s `update`, and its `fix:` line) and the summary. The folder and the snapshot hash are normalised | An error fails, with the finding in the diff |
| 2 | **(a) Real F1 pinned** | `agentic run <F1>` on an unchanged copy of the sample, with the golden factory | Exit 1. No run row (`runs 0`, read from that copy's journal), and the factory is never asked. The `stop:` lines hold the step (today `produce`), `exit 1` and stderr's words verbatim. Open-time lines are left out (row 5) | Diff names the step |
| 3 | **(b) Copy runs to its stop** | A second copy of the sample, with `workflows/<F1>.yaml` replaced by `fixtures/golden/so-far/workflow.yaml` before its first open; `agentic run <F1>` on the scripts | Today: `run <id-1> succeeded`, exit 0. `produce` attempt 1 succeeded with taint 0. One model call, purpose step, from D2-Head's script. One ledger entry. No tool offered or called, no Inbox item, no row. The `copy:` lines name the removed entries | A failing step shows in the snapshot with its words |
| 4 | Snapshot content | After both stages | In order: `validate`, `stop`, `copy`, then the run. Per step: attempts, inputs, prompt parts, calls, ledger, tool calls, output, items, history and script turns. Then `rows <table>` for each sample table, `inbox`, and `script <seat>`. Audit is left out | — |
| 5 | Normalised | Any run | UUIDs become `<id-N>`, numbered by first appearance in render order. RFC 3339 instants become `<time>`. The config hash and validate's snapshot prefix become `<config>`. The copies' roots become `<sample>`. `agentic: project …` open lines are dropped (they name the database version and a new id). Order follows step `seq`, `attempt`, `call_seq`, history `seq` and table id, never a timestamp | — |
| 6 | Compare | Actual vs `fixtures/golden/so-far/snapshot.txt` | Equal passes. A difference fails with every differing tag named (`step <key>`, or the section), the expected-only lines marked `-` and the actual-only lines `+`, and the path of `$CARGO_TARGET_TMPDIR/golden_so_far.actual.txt` | Panic with the diff; the fixture is untouched |
| 7 | Stable | Two full passes, each with its own temporary HOMEs and copies | Identical snapshots | Diff of the two |
| 8 | **(e) Planted script change** | The first served turn of the first scripted seat gets other text, in memory | Compare fails at `step <first step of the copy>` (read from the loaded copy), showing both output lines | — |
| 9 | **(e) Planted change to the copy's first step** | The installed copy's first step gets a suffix on `instruction:`, in the temporary project only | Compare fails at `step <first step>` (its prompt part's byte count) | — |
| 10 | Planted change to the real F1's first step | The unchanged copy's first step loses `handoff:` in its temporary copy | Compare fails at `stop`: the expected line names the first step, and the actual line names the next refused step | — |
| 11 | **(c) No drift** | The real F1 (`templates/…/workflows/<F1>.yaml`), the copy and `golden.json`'s removed list | **Text:** the real file with each removed key's block (its line, and the lines indented under it within that step) and each removed step's block taken out equals the copy byte for byte. **Structure:** loaded with `am_config::load_in(…, LoadOptions::template(…))`, each kept step equals the real `Step` with exactly its removed keys gone from `run_graph`, and every workflow field is equal. Planted cases in temporary files: an extra byte in a kept step, a changed `seat:`, a removed key missing from the list, and a listed key still present each fail | Fails naming the step, the field or key, and the first differing line of each file |
| 12 | Removed list tracks the product | Stage 1's stop and the removed list | Each removed key is in its real step's `run_graph`. No step before the real F1's stop step has a removed key. The stop step's removed keys equal the keys its words name. The first removed step is after the stop step or equals it | Fails naming the step and key, so a story that honours a key must shrink the list |
| 13 | **(d) Fixture only, never a template** | The repo tree | The copy is at `fixtures/golden/so-far/workflow.yaml`. No file under `templates/` has its bytes. Its folder holds no `company.yaml`. The justfile's `validate` list names no `fixtures/golden` path, and none of its globs match it. `agentic validate --template fixtures/golden/so-far/` does not exit 0 (not a project; the code is recorded at the first run) | Fails naming the breach |
| 14 | Runs in `just check` | The manifest, the test file, the justfile and the nextest config | Target `golden_so_far` at `tests/golden/golden_so_far.rs`, with no `required-features`. No `#[ignore` in the file. `check` lists `test`, and `test` is `cargo nextest run` with no filter. There is no nextest default filter. CI: macOS runs `just check`; Linux runs `cargo nextest run --workspace --exclude am-desktop` | Fails naming the breach |
| 15 | Ids only in fixtures | `crates/am-e2e/tests/golden/**` | Ids are read from `fixtures/templates/sample.json` and `fixtures/golden/so-far/`, and `cargo xtask denylist` passes | Denylist fails CI |
| 16 | No network, no model, no keychain | The golden factory | Each F1 seat's primary `ModelRef` gets the seat router. Any other gets the tripwire: am-providers' OpenAI-compatible adapter, with no `AuthRef`, on an empty `FakeEgress`. After a pass, the egress holds no request, and the factory was asked only for primary models of F1 seats. `RunContext` is built by an exhaustive literal (places, identity, terminal, providers, clock), so no secret port exists | Fails naming the model or request |
| 17 | One script per seat | `fixtures/golden/so-far/scripts/<seat>.json` | One file per F1 seat that has a model: five, and none for the Human seat. The file set equals those seats. The router picks the seat whose instructions are a system part of the request. It refuses (`ProviderError`, naming the seat) when no seat or two seats match, or the script is exhausted. Every scripted turn is served | The step fails visibly; the diff names it |
| 18 | Targets listed | `tests/golden/*.rs` | Each file has a `[[test]]` target named after it | Fails naming the file |
| 19 | Clock port | `RunContext { clock, .. }` | `plan_of` puts `ctx.clock` on the `RunPlan` for run and for gate continue. `agentic` passes the system clock (`main.rs` L188-L193); tests pass `FakeClock` | — |
| 20 | Time | `cargo nextest run -p am-e2e --test golden_so_far` | Recorded in Implementation Notes. Estimate: under 2 s wall on the owner's Mac, 2–4 s on the macOS runner. Budget 10 s; over it, stop and report | No timing assertion |

</intent-contract>

## Code Map

- **`crates/am-app/src/run.rs`:**
  - module docs L1-L67: check 8 (L30-L36) is unchanged; add a line naming `RunContext.clock`;
  - `NOT_STARTED` L107 is part of today's stop words;
  - `RunContext` L115-L123 gains `pub clock: Arc<dyn Clock>`;
  - `not_honoured` L288-L292 is unchanged: its words are today's stop;
  - `SystemClock` L296-L302 is exposed as `pub fn system_clock() -> Arc<dyn Clock>`;
  - `plan_of` L399-L539 takes `clock: Arc<dyn Clock>` and uses it at L535. L418-L425 and L426-L437 are read only;
  - `start` L644-L699: L686 passes `ctx.clock.clone()`;
  - `report` L701-L770 prints the stop lines;
  - `mod tests` L977: new `plan_of_runs_on_the_contexts_clock`.
- **Other am-app files:**
  - `gate.rs` L639: `plan_of(…, self.ctx.clock.clone(), KEPT)`;
  - `cli.rs` L25: re-export `system_clock`;
  - `main.rs` L188-L193: `clock: am_app::cli::system_clock()`. The scheduler's own `SystemClock` (L531) is unchanged.
- **`crates/am-config`**, read only:
  - `kinds.rs` L2102-L2111 and L2385-L2390 (`RUN_GRAPH_KEYS`, `run_graph`);
  - `entities.rs` L318-L319 (`Step: PartialEq`).
- **`crates/am-e2e/Cargo.toml`:** add `[[test]] name = "golden_so_far"`, `path = "tests/golden/golden_so_far.rs"`, after L240 and before `[dev-dependencies]` L242. No new dependency.
- **`crates/am-e2e/tests/golden/golden_so_far.rs` (new):** the tests in the Tests-first list. It includes:
  - the engine rig, with `#[path = "../engine/support/mod.rs"] mod support;`, as the trust tests do (`taint_rows_no_read_path.rs` L30-L31);
  - the golden rig, with `#[path = "support/mod.rs"] mod rig;`.
- **`crates/am-e2e/tests/golden/support/mod.rs` (new; in a folder, so not a target):**
  - loaders: `sample.json` (`sample`, `report_flow.workflow`), `golden.json` and the scripts; each refuses unknown keys;
  - the sample copy: `am_testkit::repo_root()` + `templates/samples/<sample>`, with `copy_tree`;
  - the copy's install;
  - the drift check: block removal over text, then the am-config comparison;
  - the fixture-only check;
  - the seat router and the factory with its tripwire;
  - `pass(plant) -> Pass`;
  - the renderer, the normaliser, and the compare with its diff.
- **`crates/am-e2e/tests/engine/support/mod.rs`:**
  - the `RunContext` literal at L575 gains `clock: am_testkit::FakeClock::at(CLOCK)`;
  - `pub const CLOCK` sits beside `HASH` (L38);
  - `Cli::copy_from(&Path)` sits beside `copy_of` (L545).
- **`crates/am-e2e/tests/trust_targets.rs`:** add `golden_targets_listed` beside L54-L72, and `golden_so_far_runs_in_just_check`.
- **`RunContext` literal sweep:** each of these gains `clock`:
  - `crates/am-app/tests/run_cli.rs` L16 (doc), L394 and L1495;
  - `run_prompt.rs` L124;
  - `run_rows.rs` L100;
  - `gate_cli.rs` L333;
  - `gate_tty.rs` L130.
- **`crates/am-testkit/src/lib.rs`:** no change. Used: `repo_root` L42, `FakeEgress` L153-L230, `ScriptedTurn` L292-L370, `FakeLlm` L413-L437 (its last turn repeats, so the router counts turns), `FakeClock` L599-L615 and `copy_tree` L835.
- **Fixtures (new), `fixtures/golden/so-far/`:**
  - **`workflow.yaml`:** the cut-down F1 copy. Today it is F1's header (L1-L12 unchanged: name, team, description, triggers, caps) and `produce` (L13-L15, without its `handoff:` block at L16-L18);
  - **`golden.json`:**
    - `about`;
    - `clock` (RFC 3339);
    - `removed_keys`: a list of `{step, key, until}`. Today: `produce`, `handoff`, `3.15`;
    - `removed_steps_from`: `{step, until, why}`. Today: `branches`, `3.14, 3.15, 3.16`, "fan-out, fan-in, hand-offs, review gate, and an Owner step with no gate (G1)";
  - **`scripts/<seat>.json`:**
    - one per F1 seat with a model: `d2-head`, `d1-e1`, `d1-e2`, `d1-e3` and `d1-head`;
    - each holds `{turns: [{step, text, usage: [in, out], tool_calls?: [{name, arguments}]}]}`;
    - today only `d2-head` holds a turn, for `produce`;
  - **`snapshot.txt`.**
- **Read only:**
  - `fixtures/templates/sample.json`;
  - `templates/samples/company-a/`, including `workflows/f1.yaml` L13-L18 `produce`, L19-L25 `branches`, L26-L37 `consolidate`, L38-L46 `report` and L47-L49 `approve`.
- **`justfile`:**
  - recipes unchanged: `check` L17 → `test` L29-L30 (`cargo nextest run`) runs the target;
  - one header comment line;
  - the `validate` list L82 stays free of `fixtures/golden/`.
- **`docs/deferred-register.md`:** new rows.
- **`.github/workflows/ci.yml`:** no change.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:** #41 merged; this plan's baseline set to main after it; this plan committed on its own branch.
- [ ] **Tests first (on Opus), with no non-test `src/` change:**
  - the fixtures: the copy, `golden.json`, five scripts, and a hand-written `snapshot.txt`;
  - the golden target and its rig;
  - the two `trust_targets.rs` tests and the `[[test]]` target;
  - the `RunContext` literal sweep;
  - the `run.rs` unit test.
  - Log the red run.
- [ ] **am-app:** `RunContext.clock`, `system_clock()`, the `plan_of` parameter, the run and gate call sites, `main.rs`, and the module docs.
- [ ] **Green:** triage any snapshot difference, and record it.
- [ ] **Wrap-up:** register rows; the justfile comment; time recorded (row 20).
- [ ] **Size (owner):** one squash commit. Stop and report if the code passes about 4,000 lines.

**Acceptance Criteria:**
- **Each matrix row:** given the row, when its test runs, then the stops, words, snapshot, diffs, drift and guard checks, and records match it.
- **The verify:** given `just check`, it runs `golden_so_far` with no feature, ignore or recipe, and:
  - the sample validates with no error;
  - the real F1 is pinned to its refusal;
  - the copy runs on scripted FakeLlm to its stop, with the snapshot of step-run states, rows and history, Inbox items and ledger entries;
  - a planted change to one script's output, or to the copy's first step, fails with a diff naming the step;
  - the denylist passes;
  - no network or keychain is touched.
- **The owner's conditions:** rows 11–13 pass, and the sample is unchanged (`git diff --stat templates/` is empty).
- **The workspace:** `just check` is green. Merge only with both CI jobs green after review triage.

## Plan Change Log

- **2026-11-25, owner (Q1) and manager (Q2):**
  - The real F1's refusal is pinned, and the cut-down copy runs.
  - The copy is a stored fixture, not cut at test time, with a drift test and a fixture-only check (new rows 11 and 13). The removed list sits beside it.
  - Planted step changes go to the copy's first step (row 9).
  - The sample is unchanged, and rows and history stay empty.
  - Rows renumbered; Q3 (removed steps) added as a reading to confirm.

## Design Notes

### Owner answers (2026-11-25)

**Owner, verbatim (Q1):** "Both: pin the real F1's refusal and run the cut-down F1 copy. Conditions: a test proves the copy equals the real F1 with only the listed keys removed (today, produce's hand-off), so it can't drift; the copy lives in test fixtures only and is never a template; each later story shrinks the removed-keys list, and 3.13 deletes the copy once it matches F1. The sample stays unchanged."

**Manager (Q2), 2026-11-25:** the sample stays as it is. In `reference-workload.md`, F1's first step produces Document A, which is a document, not rows, so adding `table:` would change the reference company to fit the code. Rows and history stay empty for now.

**As this plan builds them:**
- **Real F1 pinned:** row 2.
- **Copy runs:** row 3.
- **Drift test:** rows 11 and 12.
- **Fixtures only:** row 13.
- **Planted changes:** rows 8 and 9.
- **Shrinking list and 3.13's deletion:** the extension rule below.
- **Sample unchanged:** Boundaries.
- **Removed steps:** Q3.

### How far F1 runs, story by story

| Story (build order) | The real F1 (stage 1) | The copy (stage 2) |
|---|---|---|
| 3.24 (this) | Refused at `produce` (`handoff`), no run | `produce` without `handoff`; one turn; the run succeeds |
| 3.22, 3.21, 3.7, 3.8, 3.9, 3.18, 3.20, 3.10, 3.11 | Unchanged: the run-graph check comes first | Same list. Each story extends the snapshot where it shows: ledger amounts (3.7); a fallback hitting the tripwire (3.8); a document, if the sample declares one (3.21, 3.20) |
| 3.14 | Still `produce` (`handoff`) | Removed steps start after `report`. `handoff` stays removed on `produce`, `consolidate` and `report` |
| 3.15 | Moves to `report` (`review`, if 3.15 leaves it to 3.16) | `handoff` leaves the list |
| 3.16 | Moves to `approve` ("has no model"), unless G1 is fixed | Only `approve` stays removed |
| 3.17 | — | The snapshot gains lineage |
| 3.13 | Runs to the Owner's gate; `agentic gate` answers it | The list is empty and the copy equals F1, so the copy is deleted; `just golden` |

### Gaps found (register rows below)

- **G1:** F1's `approve` runs on the Human seat `owner` with no `gate:`.
  - `plan_of` (L493-L497) refuses a step that is not a gate on a seat with no model.
  - The reference workload calls step 7 a "human gate in the Inbox".
- **G2 (manager, 2026-11-25):** no F1 run writes rows: its first step produces Document A.
  - Rows reach the golden with F2 into F1 (epic 4 story 13).
  - Until then the `rows` and `history` lines read `(none)`.
- **G3:** from 9.10 a project's first writer open creates its backup key in the keychain (AD-32; DR-170).
  - The golden opens fresh copies on every PR.
  - So 9.10 must make the open's key store a port that the golden fakes, with a counting `FakeSecretStore` and a zero-keychain assertion.

### The copy, the removed list and the drift check (owner conditions)

- **The copy:** the builder writes `fixtures/golden/so-far/workflow.yaml` by hand from `f1.yaml`.
  - It deletes each removed key's line, and the lines indented under it within its step.
  - It deletes every step from `removed_steps_from.step` to the end of `steps:`.
  - Every other byte is kept.
- **Install:** each pass copies the sample to a temporary folder, then writes the copy's bytes over `workflows/<F1>.yaml` before the first open. The app has never written that file, so check 5 does not judge it. The run's workflow id is F1's.
- **Drift, text:** the rig applies the same deletions to the real `f1.yaml` and compares the result with the copy, byte for byte.
  - A difference names the first differing line of each file, and the step whose block holds it.
  - So the copy cannot gain, lose or change anything the list does not name.
- **Drift, structure:** both are loaded through am-config, in template mode, from temporary copies of the sample.
  - Each kept step equals the real `Step` with exactly its removed keys gone from `run_graph`.
  - The workflow's other fields are equal.
  - This catches a deletion that the text check would accept but that changes meaning.
- **The list tracks the product (row 12):** stage 1 names the first refused step and its keys. The list must not hold a key on any earlier step, and the stop step's listed keys must equal the keys named. So the story that honours `handoff` (3.15) fails here until it shrinks the list and edits the copy.
- **Fixture only (row 13):** a test checks the copy's location, that `templates/` holds no file with the copy's bytes, that its folder has no `company.yaml`, and that `just validate`'s list and globs never reach `fixtures/golden/`. `agentic validate --template` on that folder does not succeed.

### The seat router and the factory (rows 16, 17)

- **The router:**
  - The sample gives its seats the same primary model (`local-lmstudio` / `gpt-oss-20b`), so one router serves every F1 seat's primary `ModelRef`.
  - The router holds each scripted seat's instructions body (`Seat.instructions`, which `plan_of` sends as a system part, `run.rs` L453-L457) and a `FakeLlm`.
  - It picks the one seat whose body equals a system part's text (`Instruction::text`).
  - It refuses, naming the seat, when no seat or two seats match, or the script is exhausted.
  - It records the seat, the turn and the names of the tools offered. Each turn's `step` tag puts its line under `step <key>`.
- **The tripwire:**
  - Any other `ModelRef`, such as a fallback Account once 3.8 asks for one, gets `OpenAiCompatProvider::new(egress, "golden-tripwire", "http://127.0.0.1:1/v1")`.
  - It has no `AuthRef` and runs on a `FakeEgress` with no route.
  - Row 16 asserts that the egress recorded no request.

### No network, no keychain (row 16)

- **Network:** `agentic run`'s only provider source is `RunContext.providers`; the binary wires `unwired_providers` (`run.rs` L128). In the golden, that source is the router or the tripwire. Validate and the open make no request.
- **Keychain:**
  - Validate, the open and `agentic run` take no `SecretStore` at this baseline.
  - The rig builds `RunContext` with an exhaustive literal, so a later secret field fails to compile there.
  - The tripwire carries no `AuthRef`.
  - A zero-count `FakeSecretStore` would be wired to nothing today. It joins with the first secret port on this path (G3), and that story asserts zero reads.

### Snapshot format (rows 4–6)

- **Lines:** each line is `<tag>: <fact>`, and `#` lines are comments. The tags, in order:
  - `validate:`: exit code, findings and summary;
  - `stop:`: `step <key>`, `exit <n>`, `runs <n>`, the stderr lines and stdout's `run ` lines;
  - `copy:`: each removed key with its story, `removed from <step>`, the exit code and the `run` line;
  - `run <id-N>:`: workflow, status and `<config>`;
  - `step <key>:`: `attempt`, `input`, `prompt call … bytes <b>`, `model call … tokens <in>/<out>`, `ledger call … reserved … settled …`, `tool call …`, `output <text>`, `item …`, `row <table> <id-N> v<n> <op> taint <t> by seat <seat>`, and `script <seat> turn <n> served, offered [...]`;
  - `rows <table>:`;
  - `inbox:`;
  - `script <seat>: served <n> of <m>`.
- **Diff:** about 60 hand-written lines.
  - Lines are grouped by tag, in the order the tags first appear.
  - Each differing tag is printed (`step `<key>`` for a step), followed by its `-` and `+` lines.
- **Byte counts:** the builder computes each one with `printf '%s' "<text>" | wc -c`. The input is the sample's text together with `prompt::heading`'s fixed heading (`crates/am-engine/src/prompt.rs` L74). The commands are recorded in Implementation Notes.
- **Today's expected shape:**
  - `validate:` exit 0, with the one warning and its fix;
  - `stop:` `step produce`, `exit 1`, `runs 0`, and the words in Intent;
  - `copy:` `removed produce handoff until 3.15`, `removed from branches until 3.14, 3.15, 3.16`, `exit 0`, and `run <id-1> succeeded`;
  - `step produce:` one succeeded attempt with taint 0; no input; its prompt parts; one model call; ledger 0/0 `settled`; no tool call; its output; no item; no row; script turn 1 served;
  - `rows table-t1: (none)`;
  - `inbox: (none)`;
  - `script`: `d2-head` 1 of 1, and the others 0 of 0.

### The extension rule (owner; binds 3.22, 3.21, 3.7, 3.8, 3.9, 3.18, 3.20, 3.10, 3.11, 3.14, 3.15, 3.16, 3.17)

- **In the same commit, a story that changes what F1 can run:**
  - shrinks `golden.json`'s removed list (`removed_keys`, `removed_steps_from`);
  - edits `workflow.yaml` to match;
  - adds script turns for newly reached steps;
  - moves both expected results in `snapshot.txt`: the real F1's `stop:` lines and the copy's journal;
  - adds a rig section if the snapshot gains one (document versions in 3.21, lineage in 3.17).
  - Rows 11 and 12 fail a story that does only part of this.
- **A story that changes nothing in F1** leaves the fixtures alone.
- **Each snapshot change is hand-checked** against that story's matrix, never regenerated blind.
- **A story that adds a secret port** to validate, open or run wires a counting `FakeSecretStore` here (G3).
- **3.13 deletes the copy, its list and stage 2** once the list is empty and the drift check shows the copy byte-equal to F1, and completes `just golden`: F1 to the Owner's gate answered by `agentic gate`, then AD-33's variants.

### Time (row 20)

- **One full pass:** about 0.3–0.6 s.
  - Validate takes 50–100 ms.
  - Two copies of 128 KB take a few ms.
  - Two opens with migrations take 50–200 ms each.
  - One FakeLlm turn takes 20–50 ms.
- **The target:** eleven tests, with the drift and guard tests needing no run. That is about seven passes in parallel nextest processes:
  - under 2 s wall on the owner's Mac;
  - 2–4 s on CI.
- **Build:** one more test binary, roughly 5–15 s more on a warm cache.
- **Budget:** 10 s.

### Decided within the ADs (accepted with the plan)

1. The shipped commands run in-process, so the golden follows `plan_of` (AD-33).
2. The fixtures live at `fixtures/golden/so-far/` (the Structural Seed's "golden recordings"), with ids taken from fixtures (AD-23).
3. The snapshot is tagged text, written by hand, with no update mode.
4. A hand-written diff; no new crate.
5. One script per seat, behind a router keyed by the seat's instructions.
6. `RunContext.clock` is a port, like `providers`, and meets the ticket's FakeClock. The engine reads it only for guidance-note expiry (`am-engine/src/run.rs` L1206). Store times are normalised.
7. Open-time lines and audit entries are left out of the snapshot.
8. There is no timing assertion.

### Register rows (added in this story's commit; ids from DR-172)

| Kind | Item | Target | Must close |
|---|---|---|---|
| deferred | G1: F1's `approve` has no `gate:` on a Human seat. `agentic run` will refuse it once F1's run-graph keys are honoured | 3.16 (assumed) | yes (assumed) |
| deferred | G2: no F1 run writes rows, so the golden's rows and history stay empty until F2 runs into F1 (manager, 2026-11-25) | 4.13 (assumed) | no (assumed) |
| deferred | G3: from 9.10 the first open creates a keychain backup key. The golden's fresh copies need a faked key store and a zero-keychain assertion | 9.10 | yes |
| deferred | The cut-down F1 copy in `fixtures/golden/so-far/`: each F1 story shrinks its removed list, and 3.13 deletes it once the copy matches F1 (owner, 2026-11-25) | 3.13 | yes |

## Questions for the owner

**Q1. How should `golden_so_far` run F1 "as far as the built stories support"?**
Decided (owner, 2026-11-25): both. Pin the real F1's refusal, and run the cut-down copy, under the owner's conditions (Design Notes, verbatim).

**Q2. Should the golden show rows now?**
Decided (manager, 2026-11-25): no. The sample stays unchanged, and rows and history stay empty until F2 runs into F1 (G2).

**Q3. Decided by the owner, 2026-11-25: option (a), verbatim:** "Yes: the removed list may name whole steps as well as keys, each tagged with the story that restores it. The drift test checks the copy equals F1 minus exactly the listed keys and steps. An entry without a restoring story fails. The fixture-only, never-a-template check stays. Real F1 stays pinned to its refusal."

The list follows Sofia's replan (PR #42):
- keys: `produce.handoff`, `consolidate.join`, `consolidate.handoff`, `report.handoff`;
- whole steps: `branches`, `approve`.

The drift test also fails on any entry whose restoring story is missing or names no story in `tickets.toml`.

The question as it was asked, kept for the record:
The owner's condition reads "only the listed keys removed (today, produce's hand-off)". If only `produce`'s `handoff` is removed, the copy still holds `branches`, a fan-out. `agentic run` checks every step first, so it would refuse the copy at `branches` (`max_parallel`, `seats`) and no step would run. Rows 3, 8 and 9 could then not hold. A fan-out step cannot be made runnable by removing keys either: without `seats:` it has no seat.
- **(a) The list holds removed keys and removed trailing steps.** ← **assumed**
  - Each entry is listed with the story that brings it back: today `produce.handoff` (3.15), and the steps from `branches` (3.14, 3.15, 3.16).
  - The drift test checks both kinds.
  - The list shrinks to empty at 3.13.
- **(b) Keys only.**
  - The copy keeps every step, and stage 2 is refused at `branches` with no run.
  - The planted-script check waits until the copy first runs a step.

## Tests-first list

All FakeLlm and FakeClock; no network, no real model, no keychain. Each red reason is what the run shows before the am-app change. Tests in `trust_targets.rs` are green on arrival and logged.

| Test | File | Matrix rows | Expected red reason |
|---|---|---|---|
| `golden_so_far` | `crates/am-e2e/tests/golden/golden_so_far.rs` | 1, 3–6, 15, 17 | Does not compile: E0560, `RunContext` has no field `clock` (engine and golden rigs) |
| `the_real_flow_is_refused_before_any_run` | same | 2 | Same E0560 |
| `the_snapshot_is_the_same_on_a_second_run` | same | 7 | Same E0560 |
| `a_planted_script_change_fails_naming_the_step` | same | 8 | Same E0560 |
| `a_planted_change_to_the_copys_first_step_fails_naming_it` | same | 9 | Same E0560 |
| `a_planted_change_to_the_real_flows_first_step_fails_naming_it` | same | 10 | Same E0560 |
| `the_copy_is_the_real_flow_minus_the_removed_list` | same | 11 (with its planted drift cases) | Same E0560 |
| `the_removed_list_is_what_the_real_flow_refuses` | same | 12 | Same E0560 |
| `the_copy_is_a_fixture_never_a_template` | same | 13 | Same E0560 |
| `it_opens_no_network_and_reads_no_key` | same | 16 | Same E0560 |
| `golden_targets_listed` | `crates/am-e2e/tests/trust_targets.rs` | 18 | Green on arrival; logged |
| `golden_so_far_runs_in_just_check` | same | 14 | Green on arrival; logged |
| `plan_of_runs_on_the_contexts_clock` | `crates/am-app/src/run.rs` `mod tests` (test-only lines) | 19 | Does not compile: E0061, `plan_of` takes 4 arguments but 5 were supplied |
| The literal sweep (`run_cli`, `run_prompt`, `run_rows`, `gate_cli`, `gate_tty`, and every am-e2e target that includes the engine rig) | am-app tests and `tests/engine/support/mod.rs` | 19 | E0560 until the field exists |

Red-run commands: `cargo nextest run -p am-e2e --test golden_so_far --test trust_targets` and `cargo nextest run -p am-app`, both logged here before the am-app change.

## Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code:** about 30–50 lines.
  - am-app's `RunContext.clock` and its docs, `system_clock()`, the `plan_of` parameter, two call sites, `main.rs` and the re-export: 25–40.
  - The justfile comment: 2.
  - This is far below the stop.
- **Tests:** about 1,250–1,400 lines.
  - `golden_so_far.rs`: about 380, for eleven tests including the planted drift cases.
  - The golden rig: about 750–850.
    - copy and install: about 60;
    - drift check (text and am-config): about 140;
    - fixture-only check: about 40;
    - loaders: about 120;
    - router and factory: about 130;
    - renderer: about 220;
    - normaliser: about 80;
    - compare and diff: about 70.
  - `trust_targets.rs`: about 60.
  - The am-app unit test: about 30.
  - The sweep and the engine rig: about 20.
  - The target: 5.
- **Fixtures (test data):** about 130–160 lines.
  - `workflow.yaml`: about 15.
  - `golden.json`: about 25.
  - Five scripts: about 35.
  - `snapshot.txt`: about 55–75.
- **Register:** 4 rows.

## Verification

- `cargo nextest run -p am-e2e --test golden_so_far` (record its wall time), `--test trust_targets`, and every engine and trust target, for the sweep.
- `cargo nextest run -p am-app`.
- `cargo xtask denylist`, `runtime-paths`, `seam-guard` and `check-deps`.
- By hand, once, then revert each and confirm `git status` is clean:
  - change one character of the first turn in `fixtures/golden/so-far/scripts/d2-head.json`: the test fails naming `step produce`;
  - change one word of `produce`'s `instruction:` in `fixtures/golden/so-far/workflow.yaml`: the drift test and the snapshot both fail naming `produce`.
- `git diff --stat templates/` is empty: the sample is unchanged.
- `just check` is green. The Linux job's `cargo nextest run --workspace --exclude am-desktop` runs the target too.
- No `cargo clean`, and no build of a workspace copy against the shared target.
- Merge only with both CI jobs green after review triage.

## Implementation Notes
