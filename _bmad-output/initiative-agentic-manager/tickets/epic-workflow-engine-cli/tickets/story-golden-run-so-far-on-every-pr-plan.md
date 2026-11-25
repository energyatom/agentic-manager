---
title: 'Golden run so far on every PR'
type: feature
ticket: 24
created: '2026-11-25'
status: built
baseline_revision: '7611520aaa472b6d0f7057e9665dd9663ae669fd'
after: [23]
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [correctness]
review_loop_iteration: 1
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
  - `approve`: a Human seat with no `gate:` (Design Notes, gap G1; register DR-137, target 3.13).
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
  - no real model, and no network or keychain wired on the run's path (structural, review R6).
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
- **Register (quality plan part 2):** this story adds no register row (review R3). Its gaps are rows PR #42 added or restated: G1 is DR-137 and G3 is DR-173 (Design Notes, "Register rows").

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
- **`docs/deferred-register.md`:** no change (review R3: the gaps are DR-137 and DR-173, on main since PR #42).
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
- [ ] **Wrap-up:** the justfile comment; time recorded (row 20). No register row (review R3).
- [ ] **Size (owner):** one squash commit. Stop and report if the code passes about 4,000 lines.

**Acceptance Criteria:**
- **Each matrix row:** given the row, when its test runs, then the stops, words, snapshot, diffs, drift and guard checks, and records match it.
- **The verify:** given `just check`, it runs `golden_so_far` with no feature, ignore or recipe, and:
  - the sample validates with no error;
  - the real F1 is pinned to its refusal;
  - the copy runs on scripted FakeLlm to its stop, with the snapshot of step-run states, rows and history, Inbox items and ledger entries;
  - a planted change to one script's output, or to the copy's first step, fails with a diff naming the step;
  - the denylist passes;
  - no network or keychain is wired on the run's path (structural, review R6).
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

### Gaps found (their register rows: review R3)

- **G1:** F1's `approve` runs on the Human seat `owner` with no `gate:`.
  - `plan_of` (L493-L497) refuses a step that is not a gate on a seat with no model.
  - The reference workload calls step 7 a "human gate in the Inbox".
  - Register DR-137 (target 3.13; restated by PR #42): 3.13 gives `approve` `gate: review`, so `golden.json` tags the removed step `approve` with 3.13.
- **G2 (manager, 2026-11-25):** no F1 run writes rows: its first step produces Document A.
  - Rows reach the golden with F2 into F1 (epic 4 story 13).
  - Until then the `rows` and `history` lines read `(none)`.
  - No register row: Sofia's replan (PR #42, epic Notes 2026-11-25) owns it in story 3.20, which declares `table: table-t1` on `produce`; "not a gap".
- **G3:** from 9.10 a project's first writer open creates its backup key in the keychain (AD-32; DR-170).
  - The golden opens fresh copies on every PR.
  - So 9.10 must make the open's key store a port that the golden fakes, with a counting `FakeSecretStore` and a zero-keychain assertion.
  - Register DR-173 (targets 9.10 and 12.16; added by PR #42).

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

### No network, no keychain (row 16; structural, review R6)

Both claims are structural: they rest on how the run is wired (its only providers, no secret port), not on a count of sockets or keychain calls.

- **Network:** `agentic run`'s only provider source is `RunContext.providers`; the binary wires `unwired_providers` (`run.rs` L128). In the golden, that source is the router or the tripwire. Validate and the open make no request.
- **Keychain:**
  - Validate, the open and `agentic run` take no `SecretStore` at this baseline.
  - The rig builds `RunContext` with an exhaustive literal, so a later secret field fails to compile there.
  - The tripwire carries no `AuthRef`.
  - A zero-count `FakeSecretStore` would be wired to nothing today. It joins with the first secret port on this path (G3, DR-173), and that story asserts zero reads.

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

### Register rows (review R3: none added)

This story adds no row to `docs/deferred-register.md`. Each gap points at the row PR #42 added or restated (read from `origin/main`):

| Gap | Register | Target | Note |
|---|---|---|---|
| G1: F1's `approve` has no `gate:` on a Human seat | DR-137 | 3.13 | Restated by #42 against reference-workload F1 step 7; `golden.json` tags `approve` 3.13 |
| G2: no F1 run writes rows | none | 3.20 | Not a gap (Sofia, #42): 3.20 declares `table: table-t1` on `produce` |
| G3: from 9.10 the first open creates a keychain backup key | DR-173 | 9.10, 12.16 | Added by #42 (owner question) |
| The cut-down copy: the list only shrinks, 3.13 deletes it | none | 3.13 | 3.13's restated ticket and the epic Notes rule (#42); the drift and put-back tests enforce it |

#42's other rows are not this story's: DR-172 (the 3.4 race fix) and DR-174 to DR-176 (3.23's review items).

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
| `no_network_or_keychain_is_wired_on_the_runs_path` (renamed from `it_opens_no_network_and_reads_no_key`, review R6) | same | 16 | Same E0560 |
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
- **Register:** none (review R3).

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

### Tests first (tester, 2026-11-25)

**Written:**
- `crates/am-e2e/tests/golden/golden_so_far.rs`: the ten tests of the Tests-first list (rows 1 to 17).
- `crates/am-e2e/tests/golden/support/mod.rs` (a folder, so not a target): the golden rig (loaders, the router and factory, the drift and fixture-only checks, `pass`, the renderer, the normaliser, the compare and its diff).
- `crates/am-e2e/tests/trust_targets.rs`: `golden_targets_listed` (row 18) and `golden_so_far_runs_in_just_check` (row 14).
- `crates/am-e2e/Cargo.toml`: the `[[test]]` target `golden_so_far`, no `required-features`.
- `crates/am-app/src/run.rs`, inside `#[cfg(test)] mod tests` only: `plan_of_runs_on_the_contexts_clock` (row 19). This is the one `src/` file touched, as the Boundaries allow; no line outside `mod tests` changed.
- The literal sweep: `clock` in every `RunContext` literal of `run_cli.rs` (2, and the doc block), `run_prompt.rs`, `run_rows.rs`, `gate_cli.rs`, `gate_tty.rs` and the engine rig; and in `gate_cli.rs`'s `GateContext` literal and doc block (see below).
- Fixtures, in `fixtures/golden/so-far/`: `workflow.yaml` (the copy), `golden.json` (the removed list, the FakeClock instant, where the stories are listed), `scripts/<seat>.json` (five: two turns for the D2 head, one for the D1 head, none for the three branch seats) and `snapshot.txt` (hand-written).

**Readings and decisions (tester):**
- **The removed list follows Q3 and Sofia's replan (PR #42), not the Code Map's `removed_steps_from`.** `golden.json` holds `removed_keys` (`{step, key, until}`) and `removed_steps` (`{step, until, why}`), each `until` a list of `<epic>.<id>` stories: `produce.handoff` 3.15; `consolidate.join` 3.14; `consolidate.handoff` 3.15; `report.handoff` 3.15, 3.16; step `branches` 3.14; step `approve` 3.13 (Sofia's note: "3.14 for `branches` and `consolidate.join`, 3.15 and 3.16 for the hand-offs and the review inside `report`'s, then 3.13 for `approve`"). So the copy runs three steps (`produce`, `consolidate`, `report`), not one: the run succeeds with three model calls, and the D2 head's script has two turns. The "Today's expected shape" in Design Notes (one turn) is superseded by Q3.
- **`GateContext` gains `clock` too.** The Code Map's `gate.rs` line, `plan_of(…, self.ctx.clock.clone(), KEPT)`, needs it (`self.ctx` is a `GateContext`), and row 19 says "for run and for gate continue"; the sweep list named only `RunContext` literals. The tests pin `GateContext { …, clock }`; `main.rs`'s `GateContext` literal (L212) needs the field too.
- **Prompt lines carry a fingerprint.** Each `prompt` line has `sha256 <first 12 hex digits of the part's text>` (or `-` for a part with no text) besides `bytes`. Found at the by-hand check: an equal-length word change in the copy's first instruction (`the` to `all`) failed the drift test but not a bytes-only snapshot. Both now fail naming the step.
- **A pass copies the sample with `copy_files`, not `am_testkit::copy_tree`.** `copy_tree` syncs each of the sample's 32 files to disk, about 150 ms a copy and five copies a pass; the target took 7.1 s wall. `copy_files` (engine rig, beside `Cli::copy_from`) reads and writes each file (never `std::fs::copy`) and syncs nothing; `copy_of` is unchanged. The target now takes 1.1 s.
- **Words.** No golden source spells a sample id, step ids included; a test (`spelled_ids`, row 15) scans `tests/golden/` for the flow, its steps, the scripted seats and the sample's folder name. The sources say "the real flow" and "the copy", never the flow's name, and avoid the bare step words.
- **Keychain.** No secret port exists on this path at this baseline, so a counting `FakeSecretStore` would be wired to nothing; row 16 is structural (the rig's `run_flow` builds `RunContext` field by field) plus the egress and factory checks, and a tripwire self-check (a fallback model goes to the tripwire, whose egress records one request with no `auth`). A runtime count of keychain calls waits for G3.
- **Row 13:** `agentic validate --template fixtures/golden/so-far/` exits 1 (`company.yaml:1: missing company.yaml`).

### API the tests fix

```text
am_app::cli::RunContext  { places, identity, terminal, providers, clock: Arc<dyn am_domain::Clock> }
am_app::cli::GateContext { places, identity, terminal, providers, clock: Arc<dyn am_domain::Clock> }
am_app::cli::system_clock() -> Arc<dyn am_domain::Clock>   // re-exported from crate::run; reads the system time
crate::run::plan_of(snapshot, workflow, providers, clock: Arc<dyn Clock>, not_done) -> Result<RunPlan, String>
    // the plan's clock is the context's (the same Arc: a later FakeClock::set shows through)
tests/engine/support: pub const CLOCK; Cli::copy_from(&Path) -> PathBuf; pub fn copy_files(from, to)
fixtures/golden/so-far/golden.json: { about, clock, stories: { tickets, epic }, removed_keys: [{ step, key, until: [..] }],
    removed_steps: [{ step, until: [..], why }] }   // unknown keys refused
fixtures/golden/so-far/scripts/<seat>.json: { turns: [{ step, text, usage: [in, out], tool_calls?: [{ name, arguments }] }] }
```

### Red run (2026-11-25)

On `story/3-24` at the plan commit, before any non-test `src/` change. The shared `CARGO_TARGET_DIR`; no clean, no workspace copy.

```text
$ cargo nextest run -p am-e2e --test golden_so_far --test trust_targets
error[E0560]: struct `RunContext` has no field named `clock`
   --> crates/am-e2e/tests/golden/../engine/support/mod.rs:595:13
error[E0560]: struct `RunContext` has no field named `clock`
   --> crates/am-e2e/tests/golden/support/mod.rs:525:9
error: could not compile `am-e2e` (test "golden_so_far") due to 2 previous errors

$ cargo nextest run -p am-e2e --test trust_targets          # green on arrival
PASS golden_targets_listed; PASS golden_so_far_runs_in_just_check; (and the 3 existing) 5 passed

$ cargo nextest run -p am-app                               # stops at the first targets; the full list:
$ cargo build -p am-app --tests --keep-going
error[E0560]: struct `RunContext` has no field named `clock`   gate_tty.rs:135, run_rows.rs:105, run_prompt.rs:129,
                                                               gate_cli.rs:339, run_cli.rs:405, run_cli.rs:1511
error[E0560]: struct `GateContext` has no field named `clock`  gate_cli.rs:370
error[E0061]: this function takes 4 arguments but 5 arguments were supplied   src/run.rs:1083 (plan_of, L399)
error[E0425]: cannot find function `system_clock` in module `crate::cli`     src/run.rs:1094

$ cargo build -p am-e2e --tests --keep-going
25 targets fail (golden_so_far and every target that includes the engine rig), each with E0560 only
```

Every golden test's red reason is the plan's: E0560 (`RunContext` has no field `clock`). The am-app unit test's is E0061 and, because it also pins `system_clock()`, E0425.

**Green on arrival (logged):** `golden_targets_listed`, `golden_so_far_runs_in_just_check`. `cargo xtask denylist`, `runtime-paths`, `seam-guard` and `fixed-hosts` pass; `cargo fmt --all -- --check` passes.

**Rig dry run (test files only, restored):** to check the rig and the hand-written snapshot before the coder's change, the `clock` line was taken out of the two am-e2e literals (`tests/engine/support/mod.rs`, `tests/golden/support/mod.rs`), from backups restored after; `cargo nextest run -p am-e2e --test golden_so_far`: 10 passed. The hand-written snapshot matched at its first compare, and again after the fingerprints were added (no triage needed). `cargo clippy -p am-e2e --all-targets -- -D warnings` was clean in that state. The by-hand checks of Verification were run then and reverted (`cmp` against backups): one character of the D2 head's first turn fails `golden_so_far` at `step produce` (the output line); one word of the copy's `produce` instruction fails the drift test and `golden_so_far`, both at `produce`.

**Time (row 20):** `cargo nextest run -p am-e2e --test golden_so_far` (dry run, owner's Mac, warm cache): 1.1 s wall for the ten tests in parallel (each one-pass test 0.78 to 0.83 s, the two-pass test 1.09 s, the drift test 0.25 s). Before `copy_files` it was 7.1 s wall (one pass alone 1.1 s). No timing assertion.

**Byte counts and fingerprints of `snapshot.txt`'s prompt lines**, from `templates/samples/company-a/` (seat bodies after the second `---` line; goal and backstory from the frontmatter with `## Goal\n\n` and `## Backstory\n\n`; the skill as `## skill:plain-report\n\n` and its SKILL.md body; the step instructions from `workflows/f1.yaml`):

```sh
body() { awk 'n>=2{print} /^---$/{n++}' "$1"; }
body seats/d2-head.md | wc -c                                    # 291; | shasum -a 256 | cut -c1-12: 98227892505b
printf '%s' "## Goal

$(grep '^goal: ' seats/d2-head.md | sed 's/^goal: //')" | wc -c # 112 dcc58d634cc1 (backstory: 123 f2650eb407eb)
# d1-head: role 385 79eb56fd7aa8, goal 143 691bc3829440, backstory 132 fcbedef2101c
{ printf '%s' '## skill:plain-report

'; body skills/plain-report/SKILL.md; } | wc -c                  # 341 d73e32b06c2e
# step instructions (the `instruction:` value): produce 232 f987abcdcefa, consolidate 135 2b2339c5df21, report 107 60da325e3a22
```

**Size so far (tests and fixtures; no shipped code yet):** test lines 2,704 (`golden_so_far.rs` 568, the rig 1,919 after `cargo fmt`, `trust_targets.rs` +128, the engine rig +43, the am-app sweep and unit test +46; and the manifest's target, +9), over the plan's 1,250 to 1,400 (the rig's estimate was 750 to 850; rustfmt's layout and the drift and Q3 checks account for most of it); fixtures 183 lines.

**For the coder:** the am-app change is the plan's, plus `GateContext.clock` and `main.rs`'s `GateContext` literal. Expect every test above green with no fixture change; a difference at the green run is triaged here.

### Implementation (2026-11-25)

**The am-app change (the plan's, plus `GateContext.clock`):**
- `run.rs`: `RunContext` gains `pub clock: Arc<dyn Clock>`; `pub fn system_clock() -> Arc<dyn Clock>` wraps the existing `SystemClock`; `plan_of` takes `clock: Arc<dyn Clock>` and puts it in the `RunPlan` (no clock of its own); `start` passes `ctx.clock.clone()`. Module docs, `RunContext`'s and `plan_of`'s docs name the clock.
- `gate.rs`: `GateContext` gains `pub clock: Arc<dyn Clock>`; the continue's `plan_of` passes `self.ctx.clock.clone()`; its doc names the system clock.
- `cli.rs`: re-exports `system_clock`.
- `main.rs`: the `RunContext` and `GateContext` literals take `clock: am_app::cli::system_clock()`. The scheduler's own `SystemClock` is unchanged.
- `justfile`: one header comment line (`test` runs am-e2e's `golden_so_far`); no recipe changed.

**One test edit (mechanical; every assertion kept):** `plan_of_runs_on_the_contexts_clock` named `am_testkit` (`fixture`, `FakeClock`), which xtask's release guard `am_app_sources_never_name_the_test_kit` refuses anywhere under `crates/am-app/src`, test modules included; the first `just check` failed there. The test now builds the fixture path from `CARGO_MANIFEST_DIR` and uses a local settable clock (`SetClock`, `at` and `set`, as `FakeClock`'s). The red reasons (E0061, E0425) are unchanged.

**Green:** no fixture, snapshot or golden source changed; the hand-written snapshot matched at the first green run (no triage). `git diff --stat templates/` is empty.
- `cargo nextest run -p am-e2e --test golden_so_far --test trust_targets`: 15 passed, 1.13 s wall.
- `cargo nextest run -p am-app`: 384 passed, 1 skipped.
- `cargo test --doc -p am-app`: ok (0 doctests).
- `just check`: `Summary [  56.996s] 1571 tests run: 1571 passed, 8 skipped`, then fmt, clippy, doctest, check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard and validate all pass (exit 0).

**Time (row 20), inside `just check`:** the ten `golden_so_far` tests ran in parallel with the rest of the suite, each 0.014 s (fixture-only) to 2.04 s (the two-pass test); the one-pass tests 1.20 to 1.40 s; the drift test 0.24 s. About 2 s wall for the target under full load, against 1.1 s alone. No timing assertion.

**Size:** code +42 / −11 lines (`run.rs` +25 −5, `gate.rs` +13 −4, `main.rs` +2, `cli.rs` +1 −1, `justfile` +1 −1). Tests: +16 lines in the am-app unit test (the guard fix) on top of the tester's 2,704.

**Left for wrap-up:** the four register rows (Design Notes) are not yet in `docs/deferred-register.md`. (Superseded by review R3: no register row is added.)

### Review triage round 1 (manager, 2026-11-25)

One Opus review covered correctness and the owner's conditions. What it confirmed:
- The drift test is byte-exact and compares every field.
- The restoring stories are right: 3.14, 3.15, 3.16, 3.13.
- The clock is shared.
- No sample id appears in `crates/`, and `templates/` is unchanged.

Every fix below is in test code or the plan. They are written as tests, with the red runs logged.

| Id | From | Finding | Decision |
|----|------|---------|----------|
| R1 | F1 | "Each later story shrinks the list" is not enforced. `run_graph` lists every declared key, so a stale entry (a key that has become runnable) stays green. | Fix: for each entry, install the copy with only that entry put back and assert that `agentic run` refuses at that entry's step and key, before any run row. No model call. A stale entry then fails and names itself. |
| R2 | F2 | The planted cases are hard-wired to today's list (`remove(1)`, both kinds required, the first-step plant), so they panic or mislead once 3.15 lands. | Fix: pick the planted targets from the list's shape. A case that no longer applies is skipped, and the skip says so. |
| R3 | F3 | The plan requires register rows in this story's commit. The sample gaps landed as DR-172 to DR-176 in PR #42. G1 repeats DR-137 with a conflicting target (3.16 here, 3.13 in DR-137 and `golden.json`). | Fix the plan: point G1 at DR-137 (target 3.13) and the other gaps at their DR ids from #42. No new register row. |
| R4 | F4 | The restoring-story check accepts any epic 3 id, including built stories and the sweep. | Fix: the story must come after entry 24 in `tickets.toml`'s table order and not be built. |
| R5 | F5 | The snapshot fingerprints only what the journal stores, and step inputs are journaled with no text (AD-43). A wrong or empty hand-on to the next step would not show. | Fix: the FakeLlm router fingerprints each request's parts per step into the snapshot. |
| R6 | F6 | "No network" rests on how the run is wired, just as "no keychain" does. | Fix the test names and docs to say structural, as the keychain claim already does. |
| R7 | F7 | `golden_so_far_runs_in_just_check` looks only for the text `#[ignore`. | Fix: also refuse `cfg_attr(…, ignore)` and a file-level `#![cfg(…)]` that drops the target on Linux, and check that am-e2e stays a default workspace member. |

**Accepted:**
- **D1 (F8):** the nits. `copy_files` skips symbolic links and disk syncs (test-only). The fixtures-only path check cannot fail by construction. The template byte check matches exact bytes.
- **Note:** the reviewer could not run the Linux job. The PR's Linux CI job is the check.

### Red run round 1 (2026-11-25)

Tester, for the review triage's rows R1 to R7. Test code, fixtures and this plan only; no `src/` change. Each new check was shown red first, on a planted entry, file or stub, then made green. Commands run on `story/3-24` with the shared `CARGO_TARGET_DIR`.

**R4 (restoring stories):** a planted case gave the first key entry `until: ["3.23"]` (built, before 24). `cargo nextest run -p am-e2e --test golden_so_far -E 'test(=the_copy_is_the_real_flow_minus_the_removed_list)'`: red, `an entry naming a built story: no problem found`. Fix: `golden.json`'s `stories` gains `own: 24`; a `StoryBook` reads the table order of `tickets.toml` and the built stories (plans beside it with `status: built` or `done`); a story must be of the epic, after `own` in that order and not built, each worded. New test `a_restoring_story_comes_after_this_one_and_is_not_built` (the repository's book, and a planted book in memory for each message). Note: the sweep (12) is after 24 and unbuilt, so it passes, as R4's rule says.

**R2 (planted cases from the list's shape):** `golden.json` planted with one key entry and no whole step, and `workflow.yaml` with the matching copy (both restored by `cmp`): the drift test was red at its hard-wired assumption (`today the list names keys and whole steps (owner Q3)`), and `remove(1)` would have panicked next. Fix: the cases move into the rig (`planted_drift_cases`), each target picked from the list and the copy (the first key entry, the first whole step, the last kept step, a kept step with a seat), each skip worded. The drift test runs them on the shipped list and on four shapes cut from the real flow in memory (keys only, whole steps only, the first key only, empty). The same plant is now green, printing what it skipped. The row 8 and 10 plants are skipped, in words, when they cannot be made (`plant_skip`); row 2 no longer needs the refusal to name keys (once 3.16 lands it is refused for a seat with no model).

**R5 (request fingerprints):** a new plant, `EmptyHandOn`: the router records every trusted part (a hand-on) as empty. Test `an_empty_hand_on_fails_naming_the_steps_that_receive_it` was red (`an empty hand-on fails the compare: ()`): the snapshot did not see it. Fix: the router records each request part by part (`request_lines`: every system part and message part, its kind, label, bytes and a 12-hex sha256 of its text with ids and times normalised; every part kind matched by name), rendered under each step as `request <seat> turn <n> …`. Rows 8 and 9 now also see the plant at the receiving step's hand-on line and at the sent instruction line; their assertions say so.
- `snapshot.txt` gained 16 `request` lines, taken from a real run, as the triage allowed. **How they were checked:** each system and `user 1.1 instruction` line has the bytes and sha256 already computed by hand for the prompt lines above it (Byte counts, above). The two hand-ons were recomputed from the scripts: `python3 -I -c "import json,sys;print(json.load(open(sys.argv[1]))['turns'][0]['text'])" scripts/d2-head.json | tr -d '\n' | shasum -a 256 | cut -c1-12` gives `5e3d5955913a` (98 bytes, `step:produce@1`); for `scripts/d1-head.json`, `68b5a78abb6d` (86 bytes, `step:consolidate@1`). Both match.

**R1 (the list only shrinks):** test `each_removed_entry_put_back_alone_is_refused_at_its_step` against a stub `put_back` that accepted everything: red (`a stale entry fails: ()`). Fix: `put_back` cuts the real flow by the list without that one entry, installs it in a fresh copy of the sample and runs `agentic run`. It passes only on exit 1, no run row, no model call (the router served nothing, the tripwire recorded nothing), refused at the entry's step and, for a key, naming that key. All six shipped entries pass (the whole step `approve` is refused for its seat with no model; `branches` for `max_parallel`, `seats`). By hand, `golden.json` planted with `report.instruction` (a key `agentic run` honours), restored by `cmp`: red, `the removed entry `report.instruction` is stale: … it exited 0 with 1 run rows and 3 model calls …`. The test pins the same plant in memory.

**R6 (structural wording):** the test is renamed `no_network_or_keychain_is_wired_on_the_runs_path`. Its doc, the golden target's and the rig's module docs, Boundaries, the verify line and the row 16 Design Note now say the network and keychain claims are structural (the wiring, not a count). The keychain pointer is now DR-173.

**R7 (`just check` guards):** test `the_skip_guards_catch_planted_attributes` against stubs: red (`not caught: "#[test]\n#[ignore]\nfn a() {}"`). Fix: `skipping_attributes` refuses `#[ignore]`, any `cfg_attr(…)` that ignores or names a platform, every file-level `#![cfg(…)]` and an item `cfg(…)` naming a platform (attributes parsed with brackets balanced, comment lines left out). `default_member` requires am-e2e in `default-members` (or in `members` when there is no such list). Each is checked on planted text in memory and on the real files and workspace manifest. By hand, a planted `tests/golden/support/planted.rs` with `#[cfg_attr(not(target_os = "macos"), ignore)]`, then with `#![cfg(target_os = "macos")]`: `golden_so_far_runs_in_just_check` was red for each, naming the attribute; the file was removed and the test is green.

**R3 (plan text):** G1 now points at DR-137 (target 3.13) and G3 at DR-173. G2 has no row: it is owned by 3.20, per Sofia. The copy's deletion is owned by 3.13's restated ticket. The "Register rows" table now maps each gap and adds none. Boundaries, the Code Map, the Tasks and Size say so too. `docs/` is unchanged.

**Green:**
- `cargo nextest run -p am-e2e --test golden_so_far --test trust_targets`: 19 passed, 1.85 s wall.
- `just check` (exit 0): `Summary [  61.388s] 1575 tests run: 1575 passed, 8 skipped`. fmt, clippy `-D warnings`, the doctests, check-deps, check-forbid, denylist (280 files clean), runtime-paths, fixed-hosts, seam-guard and validate all pass.
- Inside `just check`, the 13 golden tests ran in 0.02 to 3.19 s each. The longest is the put-back test: six refused runs plus one planted full run.
- `git diff --stat templates/ docs/` is empty.

**Size of round 1 (tests and fixtures only):** the rig +512 lines (2,431), `golden_so_far.rs` +45 (613), `trust_targets.rs` +140 (352). Fixtures: `snapshot.txt` +19, `golden.json` +1. No shipped-code line.

## Auto Run Result

**Status:** built (2026-11-25). Planned, tested, built and reviewed on Opus.

**Summary:** `golden_so_far` runs in `just check` on every PR. There is no feature flag, no ignore and no separate recipe, and a check refuses `cfg_attr` ignores, file-level `cfg` and a non-default am-e2e. It does three things:
- **(1)** Pins the real Company A F1 to its refusal at `produce`/`handoff`: exit 1, no run row, no model call.
- **(2)** Runs a cut-down F1 copy, kept in `fixtures/golden/so-far/` only and never a template, on scripted FakeLlm through three steps. It compares a normalised snapshot of step-run states, Inbox items, ledger entries, and fingerprints of every request part per step.
- **(3)** Proves the copy cannot drift. The copy equals F1 minus exactly the listed keys and whole steps. Each entry names an unbuilt restoring story after 3.24. Each entry, put back alone, is still refused at its step and key, so a stale entry fails and names itself.

Planted changes fail with a diff naming the step; the planted cases pick their targets from the list's shape. No network and no keychain are reachable on the run's path; both claims are marked structural until a port exists (DR-173). The Company A sample is unchanged.

**Code:** `RunContext.clock` and `GateContext.clock` share one clock; `system_clock()` reads the real time.

**Size (owner rule):**
- code: 89 lines;
- tests: 3,370 lines;
- fixtures: 203 lines.

The golden target takes about 2 s.

**Review:** one Opus review (correctness and the owner's conditions). R1–R7 are fixed in test code and the plan, each new check shown red first. D1 (nits) is accepted.

**`just check`:** 1575 passed, 8 skipped.

**For later stories:** each F1 story shrinks the removed list and moves both expected results in the same commit. 3.13 deletes the copy.
