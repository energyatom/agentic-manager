---
title: 'Steps, hand-offs and Inbox items'
type: feature
ticket: 3
created: '2026-11-23'
status: built
baseline_revision: '228202e20f5633f76c37983e871beb4e463d068f'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick, adversarial-security, followup-security]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 3.1 runs a workflow's steps in order, one model turn each. Five things the ticket needs are missing:
- the step is not a bounded agent loop: one turn, no iteration, tool-call or wall-clock cap;
- there are no hand-offs: no `next` that carries work on, no loop-back, no human gate;
- the step state machine has only `queued`, `running`, `succeeded` and `failed` (AD-8 also has `waiting_human`, `looped_back`, `cancelled`), and `StartStep` sets any step `running`, whatever its state;
- there is no Inbox item entity (AD-28), so a gate has nowhere to wait and a passed maximum has nowhere to report;
- nothing carries taint (AD-10) between steps, so a loop-back reason could reach the next prompt as trusted text even when the step that wrote it read untrusted content.

The ticket's verify: a FakeLlm-scripted three-step fixture runs next, loops back from step 3 to step 1 with reasons that appear as a trusted input in the journal, fails with a Failure item when looping past the maximum, pauses at a gate with a Review item, and each loop cap ends its step when hit.

**Approach:**
- **Steps (AD-8):** every step attempt is a step-run row with an `attempt` number. The writer enforces AD-8's transitions. A loop-back closes the producing attempt as `looped_back` and queues new attempts from the target step on. Old attempts stay in the journal unchanged.
- **Bounded loop (AD-11, RULE-11):** each attempt is a loop of model turns. Tool calls go through ToolGate only. Three caps end the attempt when hit: iterations (model turns), tool calls, wall-clock. Every count comes from journal rows, never from memory.
- **Hand-offs (AD-36):**
  - `next`: the following step runs and receives the previous step's output as one input (see Question 4);
  - `loop_back`: a step's JSON output carries a reasons list; a non-empty list sends the run back to the target step, counted against `max` (default 2);
  - human gate: the gate step makes no model call; it moves to `waiting_human` with a Review item, in one transaction.
- **Inbox (AD-28):** one `inbox_items` table holding AD-28's seven kinds. The engine creates items only through the writer, as `System(engine)`. This story creates two kinds: Review (at a gate) and Failure (a passed maximum). Nothing resolves an item in this story; story 3.4 builds the Human-only resolve command.
- **Taint (AD-10, AD-36, AD-27):** a step attempt is tainted once any `UntrustedData` enters its request. Its taint is committed before the call that carries it, never clears, and travels with everything it writes: its output, its loop-back reasons, the Review item that shows its output.

## Boundaries & Constraints

**Always:**
- **Commit before effect (AD-8):**
  - A step attempt's start, with its inputs and its taint, commits before its first model call.
  - Each model call's reservation commits before the call. When the call carries a tool result, the step's taint commits in the same transaction as that reservation.
  - The producing attempt's settlement and its end (`succeeded`, `looped_back` or `failed`) commit together with what follows from it: the new queued attempts of a loop-back; the Failure item and the run's `failed`; or, at a gate, the gate step's `waiting_human` and its Review item.
  - Every command carries an explicit `Actor`. The engine's transitions use `System(engine)`; a tool call uses `Actor::Seat { seat, run, step }` at ToolGate.
- **State machine (AD-8), enforced in the writer:**
  - The step states in this story: `queued`, `running`, `waiting_human`, `looped_back`, `succeeded`, `failed`, `cancelled`. (`cancelled` is in the table; no command reaches it until story 3.11.)
  - The allowed edges: `queued → running`; `running → succeeded | failed | looped_back | waiting_human`. A gate step goes `queued → running → waiting_human` in the one transaction of its pause, its `started_at` stamped (AD-8 has no `queued → waiting_human` edge; corrected 2026-11-23, review P3, owner told).
  - **Leaving `waiting_human`:** AD-8's two edges, to `running` (Human resolves, story 3.4) and to `cancelled` (stop, story 3.11), are not built here. In this story no command moves a step out of `waiting_human`.
  - `StartStep` on a `waiting_human` or finished step is refused in words and changes nothing. (`StartStep` on an unknown id stays as it is; that is story 3.18's.)
  - `looped_back` is final for its attempt. The re-run is a new attempt row.
  - The run stays `running` while a step waits. It ends `succeeded` or `failed` as in 3.1.
- **Bounded loop (AD-11):**
  - One iteration is one model turn. A turn that asks for tools goes on to the next iteration once ToolGate has decided each call and each allowed call's result is appended.
  - **Caps per attempt (defaults pending Question 7):** `iterations` 8, `tool_calls` 20, `wall_clock` 10 min. They are set per step under `limits:`.
  - **When a cap is hit:** the attempt ends `failed`. The reason names the cap and its value. An in-flight call is cancelled through its child token and settles by the ledger rule (usage so far, else its reservation).
  - **From the journal:** iterations are counted from `model_calls` rows, tool calls from `tool_calls` rows (AD-8: tool calls are journal rows), and the wall-clock from the attempt's persisted `started_at`.
  - **Tools in the binary:** none. ToolGate holds no manifest, so a tool request fails the step in 3.1's words. Tests exercise the loop with am-testkit's fixture tool (Question 5).
- **Hand-offs (AD-36):**
  - `next` is the default between consecutive steps.
  - A `loop_back` fires when the producing attempt's final message parses as a JSON object whose named field (default `reasons`) is a non-empty list of strings. An empty list means `next`. A message that is not such an object fails the step in words: "fail closed" until story 3.5 validates schemas.
  - The loop-back count is the number of the producing step's attempts in this run that ended `looped_back` by this hand-off. AD-8's `count ≤ max` holds: with `max: 2`, the third request fails the step with a Failure item.
  - A gate step makes no model call. Its payload is the previous step's committed output, by reference.
- **Inbox (AD-28, AD-15):**
  - One table, `inbox_items`, with AD-28's kinds: `review`, `approval`, `draft`, `question`, `budget_ask`, `failure`, `proposal` (a CHECK constraint).
  - Each item has: id (UUIDv7), kind, status (`open` only in this story), run, step run, a payload reference (the step-run id whose output it carries, if any), `taint`, a title and a body in engine words, the creation time, and resolution columns left NULL (story 3.4 fills them).
  - **Created only by the engine:** through writer commands whose actor must be `Actor::System`. Any other actor is refused and writes nothing.
  - **Insert-only, no duplicates:** `UNIQUE (step_run_id, kind)`; creating an item that exists already changes nothing.
  - **Words only from the engine:** an item's body never holds model text. A Failure names the step, the maximum and its value, and points at the producing step run by id; the reasons stay in that step's stored output.
  - **Audit (AD-25):** each creation is audited `inbox.created` with the kind, item id, step-run id and taint, never the body.
- **Taint (AD-10, AD-27, AD-36; see Design Notes, "Taint through hand-offs"):**
  - **When a step is tainted:** an attempt is tainted when any `Part::Untrusted` or `Part::ToolResult` enters its request. That happens through an untrusted input at its start, a fenced input from a tainted producer, or a tool result.
  - **At rest:** `step_runs.taint` marks the attempt and its stored output. The writer only ever raises it, from 0 to 1.
  - **Into the next prompt:** the output and loop-back reasons of a tainted attempt reach the next prompt only as `UntrustedData`, fenced and labelled, and taint the receiver. From an untainted attempt they arrive as a trusted input (Question 3: its type).
  - A Review item copies its payload's taint.
  - **Every input is journaled:** as a `step_inputs` row (kind, ref, label, taint) in the transaction that starts the attempt (AD-43's rule; Decision 3).
- **Config (AD-35; Decision 1):** the new step keys are `loop_back`, `gate` and `limits`. `agentic validate` checks them with file, line and fix. AD-40..AD-42's run-graph keys (`handoff`, `review`, `seats`, `join`, and the rest) stay refused by `agentic run` as in 3.1.
- **Layering (AD-4):** am-engine depends only on am-domain and am-store. am-app maps config to the plan. The engine reads no config.
- **Tests first:** no network and no real model. FakeLlm and FakeClock from am-testkit, plus tokio paused time for the wall-clock cap. Each crate that pauses time enables tokio's `test-util` in its own dev-dependencies, as `xtask/tests/paused_time.rs` requires.

**Never:**
- **No resolving:** no command, route, tool or CLI path resolves, dismisses or changes an Inbox item after it is created, and no path takes a step out of `waiting_human`. That is story 3.4 (resolve) and 3.11 (cancel).
- **No laundering:** no conversion from `UntrustedData` to `Instruction` or to the trusted-input type, and none may be added. A tainted attempt's text never enters a later prompt outside the fence. No command clears `step_runs.taint` or an item's `taint`.
- **No model decisions:** model output never decides a gate. A gate step's state does not depend on any model text.
- **No tools in the binary:** no tool is registered, and the fixture tool stays in am-testkit, a dev-dependency (`xtask/tests/release_deps.rs` stays green).
- **Outside this story:** fan-out and fan-in (3.14); cross-department hand-offs, `handoff_deliveries` and `accepts` (3.15); review gates by a seat and `waiting_review` (3.16); lineage queries (3.17); event triggers and chains (4.15); labelled prompt assembly and schema repair (3.5); ToolGate grants and `table_write` (3.6); cost amounts (3.7); retries, fallback, step timeouts and Failure items for provider errors (3.8); resume after a crash (3.9); `agentic stop` (3.11); real taint sources, the tainted-read rule and the injection pack (4.9); `is_widening` (4.17); Inbox routes and UI (3.4, epic 6).
- **No am-server change** (Question 11) and no new event kind.
- **No new dependency.**
- **No department vocabulary in `crates/`:** the fixture's nouns stay in `fixtures/`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Next | Three-step fixture; FakeLlm scripted plain outputs | Steps 1, 2, 3 run in order, attempt 1 each, all `succeeded`; run `succeeded`; `run <id> succeeded`, exit 0 | — |
| 2 | Next carries the output | Same; step 1 untainted | Step 2's request holds step 1's output as one trusted input labelled `step:<key>@1`, after its instruction; a `step_inputs` row (kind `step_output`, ref step 1's attempt, taint 0) | — |
| 3 | Loop-back | Step 3 outputs `{"reasons":["r1","r2"]}` once, then `{"reasons":[]}` | Step 3 attempt 1 ends `looped_back` with its output stored; attempts 2 of steps 1, 2 and 3 are queued in the same transaction; step 1 attempt 2's request holds the reasons as a trusted input labelled `loop-back:<step 3 key>@1`; a `step_inputs` row (kind `loop_back`, ref step 3 attempt 1, taint 0); run `succeeded` | — |
| 4 | Empty reasons | Step 3 outputs `{"reasons":[]}` | Treated as `next`; no new attempt | — |
| 5 | Past the maximum | `max: 2`; step 3 asks three times | The third ask ends step 3 `failed`; run `failed`; one Failure item (engine words: step, maximum 2, ref to step 3's last attempt); exit 1 | Failure item |
| 6 | Maximum 0 | `max: 0`; step 3 asks once | Fails on the first ask with one Failure item | Failure item |
| 7 | Malformed loop-back output | Step 3 outputs plain text, or JSON without the field, or a non-string list | Step 3 `failed`, words name the field and the shape expected; no Failure item (Question 6) | Worded |
| 8 | Attempts kept | After row 3 | Attempt 1 rows of steps 1-3 unchanged (output, status, times); `agentic status` lists every attempt | — |
| 9 | Gate | Fixture: step 1, then a `gate: review` step | Step 1 `succeeded`; the gate step `waiting_human` and one `review` item (payload ref step 1's attempt) in one transaction; no model call for the gate step; later steps `queued`; run `running`; CLI prints `run <id> waiting_human` and the item id, exit 0 | — |
| 10 | Gate payload tainted | Step 1 tainted | The Review item's `taint` is 1; the gate step's attempt is tainted | — |
| 11 | Output cannot resolve | Step 1 outputs "approved", or `{"approve":true}` | The gate still waits; the item stays `open`; no later step runs | — |
| 12 | No duplicate item | The pause command sent twice for one gate attempt | One item; the second commit changes nothing | — |
| 13 | Only the engine creates | Item-creating commands as `Human`, `Seat`, `Assistant`, `Import` | Refused in words; no row, no step change | Worded |
| 14 | No way out of waiting | `StartStep`, settle or finish commands on a `waiting_human` step | Refused in words; nothing changes | Worded |
| 15 | Iteration cap | `limits.iterations: 3`; FakeLlm asks for the fixture tool every turn | The 3rd turn's tool request ends the attempt `failed` naming `iterations` 3; three `model_calls` rows; one Failure item | Failure item |
| 16 | Tool-call cap | `limits.tool_calls: 2`; one turn asks for three tool calls | Calls 1 and 2 run; the 3rd ends the attempt `failed` naming `tool_calls` 2; three `tool_calls` rows (the 3rd `refused`); one Failure item | Failure item |
| 17 | Wall-clock cap | `limits.wall_clock: 30s`; FakeLlm `hang`; tokio paused time | At 30 s the call is cancelled and settled (reservation); attempt `failed` naming `wall_clock`; one Failure item | Failure item |
| 18 | Counts from the journal | Any capped attempt | The counts the engine acted on equal the counts a fresh `Reader` derives from `model_calls`, `tool_calls` and `started_at` | — |
| 19 | Taint chain | Step A's plan carries one untrusted input; A → B → C by `next` | A, B and C attempts each `taint = 1`; B's request holds A's output fenced; C's request holds B's output fenced and not A's output | — |
| 20 | Taint before the call | A provider that reads the database when called | At A's call, A's attempt already has `taint = 1` and its `step_inputs` row | — |
| 21 | Tainted loop-back | C tainted; C outputs reasons | Step 1 attempt 2 starts with `taint = 1`; the reasons are fenced, labelled `loop-back:<C key>@1`, never a trusted part; its `step_inputs` row has taint 1; attempts 2 of every later step are tainted | — |
| 22 | Untainted loop-back (control) | Nothing untrusted anywhere | The reasons are a trusted part; no attempt tainted | — |
| 23 | Taint never clears | A command that would set `taint = 0` on a tainted attempt or item | Refused or no change; still 1 | — |
| 24 | Tool result taints | The fixture tool returns a result | That attempt's taint commits with the next call's reservation; its next request carries the result fenced | — |
| 25 | No tools in the binary | `agentic run`; FakeLlm asks for any tool | The step fails in 3.1's words ("tools are not available in this version"); no `tool_calls` row allowed | Worded |
| 26 | Config errors | `loop_back.to` unknown or not earlier; `max` out of range; a gate as the first step; `gate` with `loop_back` or `limits`; a cap out of range; an unknown key | `agentic validate` fails naming file, line and fix; `agentic run` starts nothing | Worded |
| 27 | Stopped while waiting | Row 9, then the process exits; later `agentic status` and `agentic serve` | Step still `waiting_human`, the item still `open`, nothing duplicated; status shows both; serve's watcher publishes nothing for them on its first look | — |
| 28 | Killed mid-loop | Process killed during a turn | Rows stay as committed (`running`, a reserved call); nothing resumes in this story (3.9); a fresh `Reader` derives the same counts | — |
| 29 | Old database | A schema-6 project database | Migration 0007 keeps every row: `attempt = 1`, `taint = 0`, new tables empty; a pre-migration copy as AD-7 | — |
| 30 | Run-graph keys | A step declaring `handoff`, `review`, `seats` or `join` | Refused as in 3.1, before any run row | Worded |
| 31 | Server unchanged | `rule_15_*` and the route-table tests | All green and unchanged; a new attempt row is published once as `step.updated` | — |
| 32 | Editing a waiting gate | A run waits at a gate; the config is then edited to remove `gate:` from that step, or to move the step to a seat with a model; `agentic run` is invoked again | The waiting step stays `waiting_human`, its Review item stays open and unchanged, no later step starts and no model call is made; only a Human resolve (3.4) moves it on | — |
| 33 | Agents cannot edit config | An am-config write of a `limits:` raise (and any edit) whose actor is `Seat` or `Assistant` | Refused before any file write, worded; the same edit as `Human` applies | Worded |

</intent-contract>

## Code Map

- **`crates/am-engine/src/run.rs`** (581 lines, story 3.1):
  - `StepPlan` (L45-58), `RunPlan` (L60-69), `RunOutcome` (L71-80): add the step's seat id, `inputs`, its hand-off (`Next`, `LoopBack { to, field, max }`, `Gate`) and its `Limits`, plus a tool registry (empty in the binary). `RunOutcome.status` gains `waiting_human`.
  - `request_of` (L117-129): the user parts in AD-27's order, as far as this story needs it: instruction, trusted inputs, then fenced untrusted inputs. Story 3.5 labels and assembles fully.
  - `failure_of` (L131-149): tool requests go to ToolGate instead of failing outright when a tool registry is given.
  - `run_on` and `drive` (L215-372): become a loop that decides the next transition from committed rows, never an in-memory index, so stories 3.4 and 3.9 can enter it later. Gate, loop-back and cap paths are added here.
  - The tests module (L374-581) keeps its store-error seam (`run_on` with `exec`).
  - A new module (e.g. `loop.rs` or `handoff.rs`) may hold the loop and the hand-off rules.
- **`crates/am-engine/src/toolgate.rs`** (`ToolGate` L36, `check` L70): it is the only caller of the tool runner. Its in-memory per-step counts stay for `max_uses_per_step`; the step's tool-call cap is counted from the journal.
- **`crates/am-engine/src/cancel.rs`** (L27-43): a child token per call, cancelled by the wall-clock cap.
- **`crates/am-store/src/lib.rs`:**
  - `WriteCommand` (L179-283): add `StartStep { inputs, taint }`, a `tainted` field on `ReserveCall`, `RecordToolCall`, `SettleAndLoopBack`, `PauseForReview`, `FailStepWithItem` (names are the implementer's).
  - `audit_entry` (L285 on): new actions `step.looped_back`, `step.waiting`, `inbox.created`, `tool_call.recorded`; args are ids, counts, statuses and taint only.
  - `apply` (L663 on; `StartStep` L732): the transition table, the System-actor check on item creation, and raise-only taint.
  - `StepRow` (L963): `attempt`, `taint`. `Reader` (L1002-1104): `steps` gains attempts; add `inbox_items(run)`, `step_inputs(step_run)`, `tool_calls(step_run)`.
- **`crates/am-store/src/migrations.rs`** (`MIGRATIONS` L28-35) and a new **`migrations/0007_steps_inputs_inbox.sql`**:
  - rebuild `step_runs`, because `0001_initial.sql` L10-19 has `UNIQUE (run_id, step_key)`, to `UNIQUE (run_id, step_key, attempt)` with `attempt` and `taint`;
  - add `step_inputs`, `tool_calls` and `inbox_items` (CHECK on kind and status).
- **`crates/am-domain/src`:**
  - `content.rs`: `Instruction` (L9-21), `UntrustedData` (L94-122); add the trusted-input type (Question 3) with no `From` either way.
  - `llm.rs`: `Part` (L35-48) gains its variant.
  - New: `InboxKind` (the seven kinds) and the step states as enums, so the store and the engine share one spelling.
  - A `ToolRunner` port, called only after ToolGate allows (Question 5).
- **`crates/am-providers/src`:** `anthropic.rs` (L47-55) and `openai_compat.rs` (L112-138) render the trusted-input part as plain labelled user text. It is not fenced, and it is not an `Instruction`.
- **`crates/am-config/src`:**
  - `entities.rs` `Step` (L319): `loop_back`, `gate`, `limits`.
  - `kinds.rs`: the allowed step keys (L2163-2171), next to `RUN_GRAPH_KEYS` (L2103); parsing beside `max_tokens`.
  - `fields.rs` (`int` L203): a duration reader for `wall_clock`.
  - `checks.rs`: target exists and is earlier, a gate is not first, ranges.
- **`crates/am-app/src/run.rs`:**
  - `not_honoured` (L217): unchanged for the run-graph keys.
  - `plan_of` (L230-301): maps the new keys; a gate step needs no model.
  - `report` (L418-466): prints `waiting_human` with the item id, exit 0; a Failure prints its words, exit 1.
  - `step_line` (L541): shows the attempt and `waiting_human`.
- **`crates/am-testkit/src/lib.rs`:** `FakeLlm` (L411) and `ScriptedTurn` (`tool_call` L341, `hang` L319) unchanged; `FakeClock` (L538); a new `FixtureTool` (read-class, returns a fixed `UntrustedData`).
- **`crates/am-server`:** no change. `events.rs` `look` (L148) and `watch_runs` (L192) key steps by id, so new attempts publish as new rows. The `routes.rs` `route_table` (L128) stays pinned by `crates/am-server/tests/route_table.rs`.
- **Fixture:** a new `fixtures/projects/hand-offs/`, with three seats, an `owner`-style human seat, and three workflows: `three-steps.yaml` (loop-back 3 → 1), `gated.yaml` (a step, then a gate), `capped.yaml` (`limits`). It must pass `agentic validate`.
- **Tests:** see "Tests-first list" in Design Notes. `crates/am-e2e/Cargo.toml` gains one `[[test]]` per new file; `crates/am-e2e/tests/trust_targets.rs` also checks `tests/engine/`.

## Tasks & Acceptance

**Execution:**
- [ ] **Tests first (no `src/` change):** every file in the Tests-first list, the fixture, the `[[test]]` targets, and the `trust_targets` extension. Log the red run with each test's failure.
- [ ] **am-domain:** the trusted-input type and `Part` variant, `InboxKind`, the step states, the `ToolRunner` port; compile-fail doctests for "no conversion".
- [ ] **am-store:** migration 0007, the commands, the transition table, the System-only item creation, raise-only taint, the reader.
- [ ] **am-config:** `loop_back`, `gate`, `limits`, the checks, the duration reader.
- [ ] **am-engine:** the bounded loop, the caps, the hand-offs, taint, the gate pause, the Failure items.
- [ ] **am-providers:** render the trusted-input part.
- [ ] **am-app:** plan mapping, `run` report, `status` lines.
- [ ] **am-testkit:** `FixtureTool`.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the rows, items, requests, audit entries and CLI output match the row.
- Given the ticket's verify, when the three-step fixture runs on FakeLlm, then it runs next, loops back from step 3 to step 1 with the reasons as a trusted `step_inputs` row and a trusted request part, fails past the maximum with one Failure item, pauses at the gate with one Review item, and each of the three caps ends its step.
- Given the workspace, when `just check` runs, then it is green, the `rule_15_*` suite included and unchanged.

## Plan Change Log

- 2026-11-23, owner: all assumed answers to Q1–Q11 accepted, with conditions (epic 3 Notes Decision):
  1. **Q3:** the architect records `TrustedInput` (trusted only from a clean step) in AD-10, and it merges to main before any code of this story.
  2. **Q5:** the minimal tool path lives in the test kit only. The shipped binary carries no tool and no runner. The engine takes a runner only through a constructor parameter that shipped code never passes, kept out of shipped code by the seam guard (as 3.2's fixture route); `FixtureTool` and its runner are in am-testkit.
  3. **Q7:** the cap defaults (iterations 8, tool calls 20, wall clock 10 min, loop-back max 2) are placeholders, configurable per step under `limits:` and `loop_back.max`, and never raisable by an agent. In this story an agent has no config path at all (no tool, no route, no Proposal); the am-config writer also refuses any config edit whose actor is `Seat` or `Assistant` (AD-15: only Human command handlers apply), with a test.
  4. **Q8:** removing a gate, or moving a gate step to a seat that has a model, is a privilege-widening change (AD-15, AD-42) and needs the two-step confirmation. The classifier and the confirmation are built by epic 4 story 17 and the outside-edits story, which Sofia restates with these two cases and the line "must close the interim gap left by 3.3" (owner, 2026-11-23, option (a)). This story adds a passing test: editing config while a gate is waiting never releases or skips the waiting Review item; only a Human resolve (3.4) moves it on. **Accepted interim risk** (to be written into the Auto Run Result): until 4.17, a hand edit can weaken a gate unchecked.
  5. **Q10:** one story, one squash commit. If it overruns, stop and report; never split.
  6. Story 3.19 tests both the foreground and the background case (its verify already names both; checked 2026-11-23).
- 2026-11-23, coordinator, aligning the plan to the amended AD-10 (architect, PR #25; binding, so corrected before any code and told to the owner):
  - `TrustedInput { label, text }` in am-domain has private fields, no `Deserialize`, no `From`/`TryFrom` with `Instruction` or `UntrustedData`, and two named constructors: one from a step run's journaled output taking the producer's committed taint (a tainted producer yields `UntrustedData` with the same label), one from an owner note taking `Actor::Human` (used from 3.4). It is its own `Part` variant; am-providers renders it through one shared function, in the user role after the step instruction and outside the fence, neutralising fence markers.
  - Only am-engine calls the constructors: `cargo xtask check-deps` gains a call-site rule for them, with an xtask test.
  - The producer's taint is read from the journal (`step_runs.taint`), never recomputed.
  - The spike's `crates/am-engine/src/journal.rs` (L131-155) wraps earlier steps' journaled outputs in `Instruction::authored`, the conversion AD-10 forbids: this story replaces it (TrustedInput or UntrustedData by the producer's taint), and the two-type doc comment in `crates/am-domain/src/content.rs` is updated.
  - Trust tests `crates/am-e2e/tests/trust/taint_trusted_input_*.rs` cover each source and each refusal (added to the tests-first list).

## Design Notes

### Taint through hand-offs (owner ask 1)

Rules followed: AD-10 (a) to (c), AD-27, AD-36, AD-41, AGENTS.md "Taint never launders". Taint never clears on crossing a hand-off, a review, a gate or a trigger.

**Where taint starts.** In the shipped binary of this story, nothing can taint a step: there are no tools, no rows, no documents and no web. Tests start taint in two ways:
- an engine plan built in the test with one `UntrustedData` input on a step (the "poisoned input");
- the fixture tool, whose result is `UntrustedData`, as every tool result is.

The rules below are what stories 3.6, 4.9 and 3.15 inherit when real sources arrive.

**The rule.** An attempt is tainted once any `UntrustedData` part enters its request, at its start or mid-loop:
- **at its start:** the engine computes the attempt's taint from its inputs and commits it with `StartStep`, in the same transaction as its `step_inputs` rows, before the first call;
- **mid-loop:** a tool result makes the attempt tainted; that taint commits in the same transaction as the next call's reservation, before the model sees the result;
- **raise-only:** the writer only raises taint; a 0 never overwrites a 1.

**What a tainted attempt writes stays untrusted (AD-10 (a), (c)):**
- Its output is stored with the attempt's `taint = 1`. That is the mark at rest.
- Whenever the engine reads that output as an input (a `next` hand-off, a loop-back's reasons, a gate's payload), it reads the producer's taint from the journal row, not from memory. When the producer is tainted, it builds `UntrustedData` (fenced and labelled), and the receiving attempt is tainted.
- There is no path from a tainted producer to a trusted part, because the trusted-input type has no constructor that takes `UntrustedData` or a tainted row.

**Worked example: A reads untrusted content, A → B → C by `next`, C loops back to A.**

| Moment | A | B | C | What the journal records |
|---|---|---|---|---|
| A starts | `taint = 1` (its poisoned input) | queued | queued | A's `step_runs.taint = 1`; a `step_inputs` row for the poisoned input (kind `untrusted`, label, taint 1); `step.started` audited with `taint: 1` |
| A's call | the poisoned text fenced in A's request | — | — | `model_calls` reserved; A's output stored under A's tainted row |
| B starts | — | `taint = 1` | queued | `step_inputs` row: kind `step_output`, ref A attempt 1, taint 1; B's request holds A's output inside the fence, labelled `step:A@1` |
| C starts | — | — | `taint = 1` | `step_inputs` row: kind `step_output`, ref B attempt 1, taint 1. C sees B's output, fenced. C does not see A's output, because `next` carries only the previous step's output (Question 4). C is tainted through B. |
| C loops back | attempt 2 queued | attempt 2 queued | attempt 1 `looped_back` | One transaction: C's output (with its reasons) stored under C's tainted row, C `looped_back`, attempts 2 of A, B and C queued |
| A attempt 2 starts | `taint = 1` | — | — | `step_inputs` rows: kind `loop_back`, ref C attempt 1, taint 1, plus the poisoned input again. The reasons enter A's prompt fenced, labelled `loop-back:C@1`, never as a trusted part. |
| B and C attempt 2 | — | `taint = 1` | `taint = 1` | As before. Taint never clears across attempts. |

**Loop-back reasons:**
- From an untainted producer, the reasons are a trusted input (AD-36). This is the ticket's verify case.
- From a tainted producer, they are `UntrustedData` and taint the target (AD-10 (a), AD-36 amended 2026-11-19). See Question 1.
- Either way, the reasons text is stored once, in the producer's output. The `step_inputs` row refers to it; nothing is copied (AD-43's "never stored twice").

**A gate:**
- The Review item copies its payload producer's taint into `inbox_items.taint`, and the gate attempt is tainted too.
- This is what story 3.4 and epic 6 need for AD-15's rule. From the first build that serves the Inbox UI, resolving a Review whose payload is tainted needs the UI class.
- An owner's return note (story 3.4) is owner-authored, so trusted. It does not clear the producer's taint: the producer re-runs tainted when its own inputs are tainted.

**What this story does not do:**
- The tainted-read rule (AD-11) and the injection pack (4.9).
- Taint at rest on rows (3.6) and documents (3.6 and 4.9).
- Cross-department payloads (3.15).
- Fan-in taint (3.14) and AI reviewer taint (3.16).

### Inbox items and who may resolve them (owner ask 2)

| Kind (AD-28) | Created in this story? | By | Who may resolve (AD-15, AD-28, AD-42) | Built by |
|---|---|---|---|---|
| Review | yes, at a `gate: review` step | `System(engine)` | `Actor::Human` only, through the one resolve command (API or `agentic gate` on a TTY) | 3.4 |
| Failure | yes, past a loop-back `max` or a loop cap | `System(engine)` | `Actor::Human` only (acknowledge) | 3.4 |
| Question | no (3.10 context overflow, 3.14 `ask`) | engine | Human only | 3.4, 3.10 |
| Approval | no (4.8 external effects) | engine | Human only; never an AI review gate (AD-42) | 4.8 |
| Draft | no (3.6 documents) | engine | Human only | 3.6, epic 6 |
| BudgetAsk | no (4.3) | engine | Human only; never an AI review gate | 4.3 |
| Proposal | no (4.10) | engine | Human only (UI class for apply); a tainted-origin Proposal is never approvable | 4.10 |

**What exists after this story that keeps a non-Human from resolving an item:**
- **No resolve command:** no `WriteCommand` variant changes an item after it is created. Items are insert-only; a second create is a no-change. A `compile_fail` doctest names `WriteCommand::ResolveInboxItem` and shows that it does not exist; story 3.4 replaces it with its Human-only command.
- **Only `System` creates:** the writer refuses item creation from `Human`, `Seat`, `Assistant` and `Import` (row 13). A seat's tool call carries `Actor::Seat`, so even a future tool could not create one.
- **No way out of waiting:** the writer's transition table has no edge out of `waiting_human` in this story. `StartStep`, settle and finish commands on a waiting step are refused (row 14). AD-8's `waiting_human → running` edge arrives only with 3.4's Human resolve.
- **No model decision:** the gate step makes no model call, and no step output is read as a decision (row 11).
- **No tool:** the binary registers no tool. ToolGate refuses any tool without a manifest. No manifest anywhere names the Inbox or the local API (AD-15: agents have no tool that reaches it).
- **No route:** `am_server::route_table()` holds no Inbox route, and `crates/am-server/tests/route_table.rs` pins the table exactly, so adding one breaks a test. The CLI token grants only CLI-class routes (3.2), and none of them writes.
- **No CLI path:** there is no `agentic gate` until 3.4. `agentic run` only reports the waiting item.
- **No hand-off path:** `next` and `loop_back` only create attempts and inputs. A loop-back target must be an earlier step, and a step after a gate runs only after the gate is resolved, so no hand-off can pass a waiting gate.

**What story 3.4 builds, not this story:**
- the resolve command, whose actor is Human by type, so a non-Human call does not compile, plus a writer refusal;
- the `waiting_human → running` edge, approve, return (counted against `max_returns`, default 2) and reject;
- the Inbox routes on the 3.2 server;
- `agentic gate` with its TTY and CLI-token rules;
- the "You (Reviewer)" audit.

### Stopped or restarted with work unresolved (owner ask 3)

What story 3.1 does today: `run_plan` never resumes. An unfinished run stays as the journal holds it. Only the spike's `journal::drive_run` resumes (crash tests, `crates/am-e2e/tests/spike/c2_crash_resume.rs`). Resume is story 3.9's.

After this story:
- **A `waiting_human` step and its Review item:** they commit in one transaction, so neither exists without the other. Then `agentic run` prints the item and exits 0, releasing the project lock. Both persist in `state.db` (AD-35: Inbox items are database state) across any stop or restart. Nothing in memory holds them. `agentic status` shows the step `waiting_human` and the run `running`. Only story 3.4's resolve moves them on.
- **A step that was `running` mid-loop when the process died:** it stays `running`, with its last call reserved and unsettled. Nothing resumes it in this story. Story 3.9 marks the call lost and re-issues it.
  - To make that exact, every count the loop acts on is derived from committed rows: iterations from `model_calls`, tool calls from `tool_calls`, loop-backs from `looped_back` attempts. Row 28 proves that a fresh `Reader` gets the same numbers.
- **The wall-clock cap:** the engine measures it from the attempt's persisted `started_at`. Within this story the clock only runs while the process runs. Whether time spent stopped counts against a resumed attempt's cap is story 3.9's call; this plan records the deadline's source so 3.9 can decide either way.
- **No duplicate items on resume:** `UNIQUE (step_run_id, kind)` and the writer's transition table (a second pause or failure of the same attempt is refused, the attempt no longer being `queued` or `running`; a duplicate insert would fail and roll the whole command back) mean a resumed engine (3.9) or a re-entered gate cannot create a second item for the same attempt: the second command changes nothing (row 12; corrected 2026-11-23, review P12: it is refused, not an insert-or-no-change). A new attempt of a gate step, after a loop-back from a later step, is a new step run and gets its own item, as it should.
- **The 3.2 event watcher:** on a new `agentic serve`, its first look records what is there and publishes nothing. So a waiting step is not re-announced, and a client reconnecting with an old `since=` gets `resync` and re-reads run status. Later changes publish once each. Inbox items have no event kind in this story (3.4 adds the Inbox routes).
- **Locks:** `agentic run` is refused while `agentic serve` holds the project (3.1), so in this story a gate is never reached under a running server.

### Questions for the owner (stop: AD or ticket intent)

Each question is presented neutrally. The plan's current assumption is marked, and it waits on the answer.

**Q1. Loop-back reasons "passed as a trusted input".**
- **Ticket:** "loop-back (target, maximum default 2, reasons passed as a trusted input)"; the verify says "reasons that appear as a trusted input in the journal".
- **AD-36 (amended 2026-11-19) and AD-10 (a):** the reasons are trusted only when the step that wrote them was untainted; otherwise they go inside the untrusted fence and taint the target. AGENTS.md says the same.
- **Conflict:** not a real one for an untainted producer. The ticket's words predate the amendment and cover only that case, and the verify's fixture is untainted. The difference is the tainted producer, which this story's binary cannot create but tests can.
- **Options:**
  - (a) Follow AD-36: trusted when the producer is untainted, fenced and tainting when it is tainted. **[assumed]**
  - (b) The ticket literally: always trusted. This conflicts with AD-10 (a) once any taint source exists.
  - (c) Always fenced. Stricter than AD-36; the verify's "trusted input" would not hold.

**Q2. Who owns taint carrying inside a run.**
- **Ticket:** silent on taint.
- **Epic 4 story 9:** owns "within one run any step that takes an input from a tainted step is tainted; a loop-back reason … written by a tainted step enters the recipient's prompt fenced". Story 3.15 owns payload taint across departments.
- **Conflict:** this story builds the channels (next, loop-back, gate) that taint must cross. Building them trusted-only would ship a laundering path until 4.9.
- **Options:**
  - (a) This story builds the minimal carrying for its own three hand-off kinds: the taint column, raise-only, propagation, fenced reasons, item taint. 4.9 proves it with real sources and the injection pack. **[assumed]**
  - (b) No taint here; 4.9 retrofits it.
  - (c) Only the `taint` column here, written 0, with propagation in 4.9.

**Q3. The type of a "trusted input".**
- **AD-10:** content is either `Instruction` (config- or owner-authored) or `UntrustedData`.
- **AD-27 and AD-36:** they name "trusted inputs" as their own part: untainted step outputs, loop-back reasons, gate notes.
- **am-domain's `Instruction`:** it must never be built "from anything a tool, file or model returned".
- **Conflict:** the output of an untainted model step is neither config-authored nor owner-authored.
- **Options:**
  - (a) A third am-domain type, `TrustedInput { label, text }`, with its own `Part` variant. The engine builds it only from an untainted journaled output or an owner's note; there is no conversion from `UntrustedData` and none to `Instruction`. It touches am-providers, and it may need an architect note on AD-10. **[assumed]**
  - (b) Carry it as an `Instruction`. That widens `Instruction`'s meaning to model text.
  - (c) Carry every model-written input as `UntrustedData`. Stricter than AD-27 and AD-36; the verify's "trusted input" would not hold.

**Q4. Does `next` carry the previous step's output?**
- **AD-36:** "A step's inputs are named references: an earlier step's output, …". The config has no `inputs:` key.
- **Ticket:** names `next` with no detail. The owner's A → B → C case needs data to flow.
- **Options:**
  - (a) `next` delivers the immediately previous step's output as one input. A step sees no older outputs until named inputs exist. **[assumed]**
  - (b) Add a step `inputs:` key now: a new config field and a wider story.
  - (c) Nothing flows by `next`. Steps see only their instruction, and taint cannot cross `next` in this story.

**Q5. A tool-call cap with no tools (tools arrive in story 3.6).**
- **Ticket:** "each loop cap ends its step when hit". Today a tool request fails the step at once (3.1), so neither the iteration cap nor the tool-call cap can be reached.
- **AGENTS.md:** "only ToolGate calls tools".
- **Options:**
  - (a) A minimal `ToolRunner` port in am-domain, called by am-engine only after `ToolGate::check` allows. The binary registers no tool. am-testkit's `FixtureTool` (read-class, an `UntrustedData` result) drives the cap and taint tests, and tool calls become journal rows (AD-8). Story 3.6 builds on the port. **[assumed]**
  - (b) No tool runs. ToolGate's refusal goes back to the model as the call's result and counts towards the caps. That needs a ruling on the refusal text's type: engine words, but delivered as a tool result, which is `UntrustedData` and would taint the step.
  - (c) Leave the tool-call and iteration cap tests to story 3.6. That changes the verify.

**Q6. Which failures raise a Failure item in this story.**
- **Ticket:** "a Failure item when a maximum is passed". AD-36: past the loop-back maximum.
- **EXPERIENCE.md:** "Step failed" shows in the Inbox as a Failure.
- **Story 3.8:** adds the Failure for a failed fallback, "naming the next action".
- **Options:**
  - (a) The loop-back maximum and the three caps. Provider errors and malformed output fail the step without an item, as in 3.1, until 3.8. **[assumed]**
  - (b) The loop-back maximum only.
  - (c) Every step failure, which takes over part of 3.8.

**Q7. Cap defaults, ceilings, and the overlap with 3.8's step timeout.**
- **ADs:** no AD fixes the iteration, tool-call or wall-clock values. RULE-11 sits under spend caps, which the standing rules send to the owner.
- **Assumed values:**
  - `iterations` 8 (1 to 50);
  - `tool_calls` 20 (0 to 200);
  - `wall_clock` 10 min (10 s to 2 h);
  - `loop_back.max` default 2 (the ticket), at most 5.
- **The overlap:** story 3.8 says "every step has a timeout".
- **Options:**
  - (a) This story's `wall_clock` is the step attempt's limit, and 3.8 adds per-call timeouts and retries inside it. **[assumed]**
  - (b) 3.8 owns all step timing, and this story only counts iterations and tool calls (the verify changes).

**Q8. How a human gate is declared.**
- **Ticket and AD-36:** a "gate" hand-off. The shipped F1 template models the Owner's approval as a plain step on a seat whose `role: Human seat`, with no gate key.
- **Constraint:** the CI denylist forbids template names such as that role in `crates/`, so the engine cannot recognise a gate by its role text.
- **Options:**
  - (a) An explicit step key, `gate: review`. The step names its (human) seat, makes no model call and cannot be first. F1 gains the key when story 3.13 needs it. **[assumed]**
  - (b) A typed seat field (e.g. `kind: human`), where every step on such a seat is a gate. A seat schema change.
  - (c) A seat with no model is a gate. Implicit; a missing model would silently become a gate.
- Per AD-42, removing a human gate is privilege-widening, so the answer also fixes what 4.17 classifies.

**Q9. "All kinds" of Inbox item.**
- **Ticket:** "the one Inbox item entity (all kinds)".
- **AD-28:** seven kinds. EXPERIENCE.md also lists "Re-approval" and "Returned at review" (informational).
- **Options:**
  - (a) AD-28's seven in the CHECK constraint; the other two are added by their stories (epic 9 MCP, 3.16). **[assumed]**
  - (b) All nine now.

**Q10. Size: one commit?**
- **What it touches:** am-domain, am-store (a table rebuild plus three tables), am-config, am-engine, am-providers, am-app and am-testkit, with about 70 tests. It is larger than 3.1 and 3.2 and spans more crates than "inside one crate boundary where possible".
- **Options:**
  - (a) One story, one commit, as planned. **[assumed]**
  - (b) A Sofia REPLAN splitting it into "Inbox items, next, loop-back and gate with taint carrying" and "Bounded agent loop: caps and the tool path". The plan does not split it on its own.

**Q11. Must this story touch the server?**
- **Ticket:** silent.
- **3.2 deferred D2** (paging of run reads) to "3.3 onward", for when runs start from the server.
- **Options:**
  - (a) No server or am-api change. This story starts no run from the server, the watcher already publishes new attempts and states, D2 stays with the first story that starts runs from the server, and Inbox events come with 3.4's routes. **[assumed]**
  - (b) Add `attempt` and `taint` to `StepDto` and an `inbox.created` event now.

### Decisions for the owner's approval (within the ADs)

1. **Config keys** (field names are the plan's to choose; Q8 decides the gate's meaning):
   ```yaml
   - id: check
     seat: s3
     instruction: …
     loop_back: { to: draft, reasons: reasons, max: 2 }   # `reasons` and `max` default as shown
     limits: { iterations: 8, tool_calls: 20, wall_clock: 10m }
   - id: approve
     seat: owner
     instruction: Approve or return the draft.
     gate: review
   ```
   - **For story 4.17's `is_widening`** (recorded here because the function does not exist yet; AGENTS.md says a new field must be classified there):
     - raising `loop_back.max` or any `limits` value, and adding a `loop_back`, count as raising a cap;
     - removing `gate:`, or moving the gate step to a seat with a model, removes a human gate (AD-42).
2. **Attempts are rows.** A loop-back queues new step-run rows (`attempt` n+1) and never rewrites an old one, so every output, reason and time stays in the journal. This needs a `step_runs` table rebuild in 0007 (SQLite cannot drop a UNIQUE constraint), behind AD-7's pre-migration copy.
3. **Inputs are journaled as AD-43's `step_inputs`** (kind, ref, label, taint), written in the transaction that starts the attempt. This story uses the kinds `step_output`, `loop_back` and `untrusted`. Story 3.17 adds the rest and the lineage walk. The rows are what makes "reasons appear as a trusted input in the journal" checkable before story 3.5 journals whole prompts.
4. **Waiting is not failing.** `agentic run` prints `run <id> waiting_human` and `Review item <id> waits for you in the Inbox`, then exits 0. The run row stays `running`.
5. **Loop-back output** is read as JSON only for a step that declares `loop_back`. Anything else in that step fails it in words, until story 3.5's schema check.
6. **Labels** for this story are `step:<key>@<attempt>` and `loop-back:<key>@<attempt>`. Story 3.5 may restate them inside AD-27's labelled assembly.

### Tests-first list

All on FakeLlm, FakeClock and tokio paused time. No network, no real model. Each file is written before any `src/` change, with its red reason logged.

**`crates/am-e2e/tests/engine/handoff_next.rs`** (new `tests/engine/` folder; a `[[test]]` each):
- `steps_run_in_order_each_as_attempt_one`: row 1. Red: fails to compile, no `HandOff` or `inputs` on `am_engine::run::StepPlan`.
- `the_previous_output_arrives_as_one_trusted_input`: row 2. Red: compile, no `Reader::step_inputs` and no `Part::Trusted…`.
- `the_cli_runs_the_three_step_fixture`: rows 1 and 2 through `agentic run` in process. Red: assertion, `agentic validate` reports `loop_back` as an unknown key, exit 1.

**`crates/am-e2e/tests/engine/handoff_loop_back.rs`:**
- `step_three_loops_back_to_step_one_with_its_reasons_as_a_trusted_input`: row 3, the ticket's verify. Red: compile, no `HandOff::LoopBack`.
- `an_empty_reasons_list_moves_on`: row 4. Red: compile.
- `looping_past_the_maximum_fails_with_one_failure_item`: row 5. Red: compile, no `Reader::inbox_items`.
- `a_maximum_of_zero_fails_on_the_first_ask`: row 6. Red: compile.
- `a_malformed_loop_back_output_fails_the_step_in_words`: row 7. Red: compile.
- `earlier_attempts_stay_in_the_journal`: row 8. Red: compile, no `StepRow::attempt`.
- `the_loop_back_count_comes_from_the_journal`: row 18 (loop-backs). Red: compile.

**`crates/am-e2e/tests/engine/handoff_gate.rs`:**
- `a_gate_waits_with_one_review_item_and_no_model_call`: row 9. Red: compile, no `HandOff::Gate`.
- `the_cli_reports_the_waiting_item_and_exits_zero`: row 9 through `agentic run`. Red: assertion, `gate` is an unknown key, exit 1.
- `a_tainted_payload_makes_a_tainted_review_item`: row 10. Red: compile.
- `no_model_output_resolves_a_gate`: row 11. Red: compile.
- `a_waiting_gate_survives_a_restart_without_a_duplicate`: rows 12 and 27; reopens the store and re-runs the pause command. Red: compile.
- `the_server_publishes_nothing_old_on_its_first_look`: row 27, with the 3.2 rig. Red: compile (fixture run needs `HandOff::Gate`).
- `editing_config_never_releases_a_waiting_gate`: row 32 (owner condition 4); removes `gate:`, then moves the gate step to a seat with a model, re-running `agentic run` after each. Red: compile, no `HandOff::Gate` (it then passes once the gate exists, and must stay green).

**`crates/am-e2e/tests/trust/taint_handoff_chain.rs`:**
- `a_tainted_step_taints_every_step_after_it_by_next`: row 19, A → B → C. Red: compile, no `StepRow::taint`.
- `c_sees_b_fenced_and_never_a`: row 19. Red: compile.
- `taint_commits_before_the_call_that_carries_it`: row 20, a provider that reads the database. Red: compile.
- `reasons_from_a_tainted_step_are_fenced_and_taint_the_target`: row 21. Red: compile.
- `reasons_from_an_untainted_step_stay_trusted`: row 22. Red: compile.
- `every_later_attempt_stays_tainted`: row 21. Red: compile.
- `a_tool_result_taints_its_step_before_the_next_call`: row 24. Red: compile, no `am_testkit::FixtureTool`.

**`crates/am-e2e/tests/trust/taint_at_rest.rs`:**
- `no_command_clears_a_step_or_item_taint`: row 23. Red: compile, no taint field on `StartStep` or `ReserveCall`.

**`crates/am-engine/tests/loop_caps.rs`:**
- `the_iteration_cap_ends_the_step_with_a_failure_item`: rows 15 and 18. Red: compile, no `Limits`.
- `the_tool_call_cap_ends_the_step_with_a_failure_item`: rows 16 and 18. Red: compile.
- `the_wall_clock_cap_cancels_and_settles_the_call`: row 17, `start_paused`. Red: compile.
- `counts_a_fresh_reader_derives_match_the_engines`: row 28. Red: compile.
- `with_no_tools_a_tool_request_fails_as_before`: row 25. Red: compile, `StepPlan` has no tool registry. The words are already right.

**`crates/am-store/tests/inbox.rs`:**
- `only_a_system_actor_creates_an_item`: row 13. Red: compile, no item-creating command.
- `an_item_kind_is_one_of_ad_28s_seven`: Q9 (a), a raw insert of another kind fails the CHECK. Red: assertion, no table.
- `creating_an_item_twice_changes_nothing`: row 12. Red: compile.
- `the_pause_and_its_item_commit_together_or_not_at_all`: a refused item insert leaves the step `running`. Red: compile.
- `nothing_leaves_waiting_human`: row 14. Red: assertion, today's `StartStep` sets a step `running` whatever its state.
- A `compile_fail` doctest on `Writer::exec` naming `WriteCommand::ResolveInboxItem`: green on arrival (it guards an absence); 3.4 replaces it.

**`crates/am-store/tests/run_journal.rs`** (additions):
- `migration_0007_keeps_every_row_as_attempt_one_untainted`: row 29. Red: assertion, schema version is 6.
- `a_loop_back_keeps_the_old_attempt_and_queues_new_ones`: row 8. Red: compile.

**`crates/am-config/tests/hand_offs.rs`:**
- `loop_back_gate_and_limits_parse_with_defaults`: Decision 1. Red: assertion, unknown keys reported.
- `a_loop_back_to_an_unknown_or_later_step_is_refused`, `a_maximum_out_of_range_is_refused`, `a_gate_cannot_be_first_or_loop_back`, `a_limit_out_of_range_is_refused`, `the_fixture_validates`: row 26. Red: assertion, the findings are "unknown key", not the expected words.

**`crates/am-domain`** doctests (`content.rs`):
- `compile_fail`: no `From<UntrustedData>` for the trusted-input type, and none to `Instruction`.
- Caveat for the red run: while the type is absent, each `compile_fail` doctest already fails to compile, for the wrong reason, so it would pass before any code exists. The tester pairs each one with a plain doctest that builds the type the allowed way. That doctest is red (the type does not exist) and turns green with the type, so the pair proves the conversion is what fails to compile.

**`crates/am-app/tests/run_cli.rs`** (additions):
- `status_lists_attempts_and_a_waiting_step`: rows 8 and 27. Red: assertion, the fixture is refused by validate.
- `run_graph_keys_are_still_refused`: row 30. Green on arrival (a regression guard).

**`crates/am-e2e/tests/trust_targets.rs`:**
- `engine_targets_listed`: every `tests/engine/*.rs` has a `[[test]]` target. Green on arrival once the targets are added (a guard, as AGENTS.md's release gate runs `tests/engine/` by directory).

**Unchanged and expected green:** `rule_15_*`, `crates/am-server/tests/route_table.rs` (row 31), `crates/am-engine/tests/run.rs`, the spike's `c2_crash_resume.rs`, `xtask` `release_deps`, `denylist`, `paused_time`.

About 70 tests in all. Expected red reasons are mostly "fails to compile". The CLI and config ones fail on assertions, because today's validator reports the new keys as unknown.

**`crates/am-e2e/tests/trust/taint_trusted_input_sources.rs`** (AD-10 amended 2026-11-23):
- `a_clean_step_output_becomes_a_trusted_input_part`: Red: compile, no `am_domain::TrustedInput`.
- `a_tainted_step_output_becomes_untrusted_data_with_the_same_label`: Red: compile.
- `the_producers_taint_is_read_from_the_journal_not_recomputed`: a producer row marked tainted in the store while its in-memory result looks clean still yields `UntrustedData`. Red: compile.
- `the_spike_resume_path_no_longer_wraps_outputs_as_instruction`: `journal::drive_run` on a tainted earlier step puts its output inside the fence. Red: assertion, the output arrives as an `Instruction` part.
- `a_trusted_input_renders_outside_the_fence_after_the_instruction_with_fence_markers_neutralised`: through FakeLlm's recorded request. Red: compile, no `Part::Trusted`.

**`crates/am-e2e/tests/trust/taint_trusted_input_refusals.rs`:**
- `no_conversion_compiles`: `compile_fail` doctests (paired with a positive one) for `TrustedInput::from(UntrustedData)`, `Instruction::from(TrustedInput)`, and `serde_json::from_str::<TrustedInput>`. Red: the positive doctest fails to compile (no type).

**`xtask/tests/check_deps.rs`** (additions):
- `trusted_input_constructors_are_called_only_from_am_engine`: a fixture crate calling `TrustedInput::from_step_output` outside am-engine fails the check. Red: assertion, the check passes it.

**`crates/am-config/tests/writer_actor.rs`** (new):
- `a_seat_or_assistant_actor_cannot_edit_config`: row 33 (owner condition 3). Red: assertion, the writer applies the edit for any actor.

### Out of scope (by ticket)

- 3.4: resolution, `max_returns`, `agentic gate`, Inbox routes.
- 3.5: full labelled prompt assembly and schema repair.
- 3.6: ToolGate grants, `table_write`, Draft items.
- 3.7: amounts.
- 3.8: retries, fallback, provider Failure items, per-call timeouts.
- 3.9: resume.
- 3.11: stop and `cancelled`.
- 3.14: fan-out and fan-in, `waiting_branches`.
- 3.15: cross-department hand-offs.
- 3.16: review gates by a seat, `waiting_review`.
- 3.17: lineage.
- 4.15: trigger chains.
- 4.9: real taint sources and the injection pack.
- 4.17: `is_widening`.

**Size:** large (see Q10). Seven crates, one migration with a table rebuild, a new fixture, about 70 tests.

## Verification

- `cargo nextest run -p am-e2e -E 'binary(/handoff_/) | binary(/taint_/)'`
- `cargo nextest run -p am-engine -p am-store -p am-config -p am-app`
- `cargo test -p am-domain --doc` and `cargo test -p am-store --doc`
- `cargo nextest run -p am-e2e -E 'binary(/rule_15/)'` and `-p am-server`, unchanged and green
- then `just check`

## Implementation Notes

### Phase 1 test API (tester, 2026-11-23)

The tests pin this API. Names not listed here (internal functions, audit actions other than the three named, column names other than `step_runs.attempt`/`taint` and `inbox_items.id`/`kind`) are the implementer's. Every type below derives `Debug, Clone, PartialEq, Eq` (plus `Copy` where marked) unless said otherwise; the tests compare rows with `assert_eq!`.

**am-domain** (exported at the crate root):
- `enum Taint { Clean, Tainted }` (`Copy`). No `Default` is needed by the tests.
- `struct TrustedInput` with private fields `label`, `text`; no `Default`, no `Deserialize`, no `From`/`TryFrom` with `Instruction`, `UntrustedData`, `String` or `&str` in either direction (probed in `taint_trusted_input_refusals.rs`, which also probes that `UntrustedData` still never becomes an `Instruction`).
  - `TrustedInput::from_step_output(label: impl Into<String>, text: impl Into<String>, producer: Taint) -> Part`: `Part::Trusted(..)` for `Taint::Clean`, `Part::Untrusted(UntrustedData::text(label, text))` (same label) for `Taint::Tainted`.
  - `TrustedInput::from_owner_note(label: impl Into<String>, text: impl Into<String>, author: &Actor) -> Option<TrustedInput>`: `Some` only for `Actor::Human`.
  - `label(&self) -> &str`, `text(&self) -> &str`.
- `Part::Trusted(TrustedInput)` (user role only).
- `enum InboxKind { Review, Approval, Draft, Question, BudgetAsk, Failure, Proposal }` (`Copy`), `InboxKind::ALL: [InboxKind; 7]` in that order, `InboxKind::as_str(self) -> &'static str`: `review`, `approval`, `draft`, `question`, `budget_ask`, `failure`, `proposal`.
- `trait ToolRunner: Send + Sync` (the port; no shipped implementation). Suggested shape (the tests do not call it directly): `fn manifests(&self) -> Vec<tool_manifest::ToolManifest>` and `fn run(&self, call: ToolCall, cancel: Arc<dyn CancelSignal>) -> BoxFuture<Result<UntrustedData, String>>`.
- Doctests in `crates/am-domain/src/content.rs` (the plan's compile_fail pairs; tests cannot hold doctests, so the implementer writes them; `taint_trusted_input_refusals::the_domain_doctests_pair_each_refusal_with_an_allowed_build` checks they are there). Validated against a stub of this API: each `compile_fail` failed for the conversion, the plain ones passed. am-domain has no serde_json, so the `Deserialize` refusal bounds on serde's trait (a `serde_json::from_str` doctest would fail for the missing crate, the wrong reason):
  ```rust
  /// ```
  /// use am_domain::{Part, Taint, TrustedInput};
  /// let p = TrustedInput::from_step_output("step:a@1", "text", Taint::Clean);
  /// assert!(matches!(p, Part::Trusted(_)));
  /// ```
  /// ```compile_fail
  /// use am_domain::{TrustedInput, UntrustedData};
  /// let _t = TrustedInput::from(UntrustedData::text("x", "y"));
  /// ```
  /// ```compile_fail
  /// use am_domain::{Instruction, Part, Taint, TrustedInput};
  /// let Part::Trusted(t) = TrustedInput::from_step_output("step:a@1", "t", Taint::Clean) else { panic!() };
  /// let _i = Instruction::from(t);
  /// ```
  /// ```compile_fail
  /// fn deserializable<T: serde::de::DeserializeOwned>() {}
  /// deserializable::<am_domain::TrustedInput>();
  /// ```
  /// ```
  /// fn deserializable<T: serde::de::DeserializeOwned>() {}
  /// deserializable::<am_domain::Instruction>();
  /// ```
  ```
  `just doctest` runs only `cargo test --doc -p am-store`; it needs `-p am-domain` too.

**am-store** (exported at the crate root):
- `StepRow` gains `attempt: i64` (1 for every pre-0007 row) and `taint: Taint`. `Reader::steps(run)` returns every attempt ordered by `(seq, attempt)`.
- `struct StepInput { kind: String, from_step_run: Option<String>, label: String, taint: Taint }`: one type for the command and the read. Kinds: `step_output`, `loop_back`, `untrusted`. `from_step_run` is the producing attempt (`None` for a plan input).
- `struct NewInboxItem { id: String, kind: InboxKind, payload: Option<String>, taint: Taint, title: String, body: String }` (`payload`: the step-run id whose output it carries).
- `struct InboxItemRow { id, kind: InboxKind, status: String /* "open" */, run_id, step_run_id, payload: Option<String>, taint: Taint, title, body, created_at: String, resolved_at: Option<String>, resolved_by: Option<String> }`.
- `struct ToolCallRow { id: String, step_run_id: String, call_seq: i64 /* increasing in read order */, tool: String, status: String /* "refused" for a refused call */ }`.
- `struct CallSettlement { call_id: String, usage: Option<Usage>, cost_micro_usd: Option<i64> }`.
- `Reader::inbox_items(run_id) -> Result<Vec<InboxItemRow>, StoreError>`, `Reader::step_inputs(step_run_id) -> Result<Vec<StepInput>, _>` (insertion order), `Reader::tool_calls(step_run_id) -> Result<Vec<ToolCallRow>, _>`.
- `WriteCommand`:
  - `StartStep { step_run_id }` **unchanged** (deviation: the plan put `inputs`/`taint` on it, but `rule_15_events.rs` builds `StartStep { step_run_id }` and row 31 keeps `rule_15_*` unchanged). It starts an attempt with no inputs and no raise.
  - new `StartAttempt { step_run_id, inputs: Vec<StepInput>, taint: Taint }`: `queued → running`, its `step_inputs` rows and raise-only taint in one transaction; audited `step.started` with a `taint` argument (`1`/`true` or `0`/`false`; the tests accept either) and never an input's text.
  - `ReserveCall` gains `taint: Taint` (raise-only; the plan's `tainted`). The three existing literals got `taint: Taint::Clean` (`am-store/tests/audit.rs`, `am-store/tests/run_journal.rs`, `am-e2e/tests/trust/rule_16_audit_log.rs`).
  - new `SettleAndLoopBack { call_id, usage, cost_micro_usd, step_run_id, output: String, to: String }`: settles the call, ends the producing attempt `looped_back` with `output` stored, queues attempt n+1 of every step from `to` through the producer (not after it), one transaction; an unknown `to` or one after the producer is refused with nothing changed. Audited once with an action containing `step.looped_back`, no output text.
  - new `PauseStep { step_run_id, inputs: Vec<StepInput>, taint: Taint, item: NewInboxItem }`: `queued`/`running → waiting_human` with the item (and, for a queued gate, its inputs and taint) in one transaction. A second pause of the same attempt (same or new item id) changes nothing (Ok or Err; one item, one `inbox.created` entry). A payload that is no step run of the run is refused with nothing written. An item never carries less taint than its payload's producer (refused, or created tainted).
  - new `FailWithItem { step_run_id, settle: Option<CallSettlement>, output: Option<String>, item: NewInboxItem }`: settles the call if given, ends the step `failed`, creates the item and finishes the run `failed`, one transaction.
  - Item creation (`PauseStep`, `FailWithItem`) is refused for any actor but `Actor::System`, with words containing `actor.describe()`, writing nothing (no row, no audit entry).
  - Audit `inbox.created`: target = the item id; `args` hold `kind` (its `as_str`), `step_run_id`, `taint`; never the title or body.
  - Transitions (AD-8): `StartStep`/`StartAttempt` only from `queued`; finishing only from `running`; nothing leaves `waiting_human` (the pure transition commands are refused in words containing `waiting_human`; the settling ones may be refused for their call first, but change nothing). Note for the implementer: the spike's `journal::drive_run` finishes a cancelled step `cancelled` from `running` (its src test `cancel_settles_at_reservation_when_usage_missing`); AD-8 has that edge but the plan's list for this story does not, so keep `running → cancelled` or that src test fails.
- Migration 0007: `step_runs` rebuilt with `attempt` (default 1) and `taint` (0/1, default 0) under `UNIQUE (run_id, step_key, attempt)`; tables `step_inputs`, `tool_calls`, `inbox_items` (CHECK on kind, the seven spellings). The CHECK test copies a row by column name (`id` and `kind` replaced) and expects SQLite's "CHECK" error. `SCHEMA_VERSION` becomes 7. One trust test (`the_producers_taint_is_read_from_the_journal_not_recomputed`) writes `UPDATE step_runs SET taint = 1 WHERE status = 'running'` raw, so the column is an integer named `taint`.
- To add as src doctest (plan): a `compile_fail` on `Writer::exec` naming `WriteCommand::ResolveInboxItem`.

**am-engine** (`am_engine::run`):
- `StepPlan` gains `seat: String` (a tool call's `Actor::Seat`), `inputs: Vec<UntrustedData>` (plan inputs; journaled `untrusted`, taint 1), `hand_off: HandOff`, `limits: Limits`. `provider` stays required: a gate step's provider is never called (am-app needs a stand-in for a gate seat with no model, never asked of the factory).
- `enum HandOff { Next, LoopBack { to: String, field: String, max: u32 }, Gate }`.
- `struct Limits { iterations: u32, tool_calls: u32, wall_clock: std::time::Duration }` (`Copy`), `Default` = 8, 20, 600 s.
- `run_plan(store, started_by, origin, plan, cancel)` unchanged (no tools: a tool request fails in 3.1's words "tools are not available in this version", no `tool_calls` row, no item). New `run_plan_with_tools(store, started_by, origin, plan, cancel, tools: Arc<dyn ToolRunner>)`: the test-only constructor parameter (owner condition 2), kept out of shipped code by `cargo xtask seam-guard` (`xtask/tests/seam_guard.rs::shipped_code_naming_the_engines_tool_seam_fails`: naming `run_plan_with_tools` in any `crates/*/src` but its `fn` line fails).
- `RunOutcome.status`: `succeeded`, `failed` or `waiting_human` (reason `None`). Failure reasons name the step and the number (loop-back max), the cap and its value (`iterations`, `tool_calls`, `wall_clock`), or the field and `list of strings` (malformed loop-back output). Numbers are matched as words, so a UUID never satisfies them.
- Requests: one user message, parts `[Instruction(step), Trusted.., Untrusted..]`; labels `step:<key>@<attempt>` (next) and `loop-back:<key>@<attempt>`; the trusted reasons' text contains each reason. Tool results travel as `Part::ToolResult`. The wall-clock cap uses tokio timers (paused-time test) and cancels only the call's child token.
- Failure item body: names the step or cap and the value as a word, and (loop-back max) the producing attempt's id; never model text.

**am-config** (`am_config::*`): `Step` gains `loop_back: Option<LoopBack>`, `gate: Option<Gate>`, `limits: StepLimits` (defaults applied at parse); `struct LoopBack { to: String, reasons: String /* default "reasons" */, max: u32 /* default 2 */ }`; `enum Gate { Review }` (`Copy`); `struct StepLimits { iterations: u32, tool_calls: u32, wall_clock: Duration }` (`Copy`), `Default` = 8, 20, 600 s. Durations `<n>s|m|h`. Findings (file, line of the key's value, a fix): `loop_back` unknown target (message names `loop_back` and the id), not earlier or itself (`earlier`), missing `to` (the existing "missing 'to'"), `'max'` out of 0..=5 or not a number, a gate first (`gate`, `first`), gate with `loop_back` or `limits` (names both), `gate:` other than `review` (names `review`), `'iterations'` 1..=50, `'tool_calls'` 0..=200, `'wall_clock'` 10 s..=2 h, unknown keys under both maps with "did you mean". The writer (`apply`, `create`, `undo`, `restore`) refuses `Actor::Seat` and `Actor::Assistant` first (words contain "refused" and `actor.describe()`), before any intent or write.

**am-app**: `agentic run` prints `run <id> waiting_human` and then exactly `Review item <id> waits for you in the Inbox` on stdout, exit 0. `agentic status` prints one line per attempt holding the key as a word and `attempt <n>`, and the status (`looped_back`, `waiting_human`).

**am-testkit**: `FixtureTool` (the implementer writes it; src): `FixtureTool::NAME: &str` (e.g. `fixture_read`), `FixtureTool::RESULT: &str` (its fixed result text), `FixtureTool::new() -> Arc<FixtureTool>`, `calls(&self) -> usize` (runs so far); implements `ToolRunner` with one read-class manifest whose `max_uses_per_step` is high (100) so only the story's caps refuse, returning `UntrustedData::text(<a label>, RESULT)`.

**xtask**: `check-deps` also scans `crates/*/src` under `--root` (default the repo root; comments and literals left out) and fails, naming the file, line, `TrustedInput` and `am-engine`, on `from_step_output` / `from_owner_note` named anywhere but `crates/am-engine/src/` and am-domain's definition lines. `seam-guard` gains `run_plan_with_tools`.

**Fixture** `fixtures/projects/hand-offs/`: seats `first`, `second`, `third` (all `local-lmstudio`/`example-local`) and `owner` (`role: Human seat`, no model); workflows `relay` (draft → edit → check, loop-back to draft, max 2), `gated` (draft → approve `gate: review` on owner → publish on second), `capped` (one step `work`, `limits: { iterations: 3, tool_calls: 2, wall_clock: 30s }`). Workflow files are `relay`/`gated`/`capped`, not the plan's `three-steps`: the denylist turns every hyphenated fixture file stem into a forbidden term in `crates/`, where the tests name the workflow. `just validate` should list `fixtures/projects/hand-offs/` (justfile, implementer).

**Changed existing tests** (row 2 and AD-8 supersede 3.1):
- `crates/am-engine/tests/run.rs`: the `step()` helper fills the new fields; `two_steps_run_in_order_each_with_its_own_request` now expects the second request to end with the trusted `step:gather@1` part (3.1 asserted it held nothing of the first output; the plan lists this file as unchanged, which row 2 contradicts).
- `crates/am-app/tests/run_cli.rs`: `two_steps_run_in_order_each_with_its_own_seat_and_model` likewise for its second request.
- `crates/am-store/tests/audit.rs`: step b went from `queued` straight to a `skipped` finish, an edge AD-8 lacks; it now starts, then finishes `failed` (and the expected args say `failed`). Still green today.

**Tests added beyond the list:** `taint_at_rest::a_review_item_is_never_cleaner_than_its_payload`, `taint_trusted_input_sources::an_owner_note_is_trusted_only_from_a_human`, `taint_trusted_input_refusals::the_domain_doctests_pair_each_refusal_with_an_allowed_build`, `loop_caps::the_default_caps_are_the_placeholders`, `inbox::the_writer_refuses_an_edge_ad_8_does_not_have`, `writer_actor::a_seat_or_assistant_actor_cannot_create_undo_or_restore`, `xtask/tests/seam_guard.rs::shipped_code_naming_the_engines_tool_seam_fails` (owner condition 2), `trust_targets::engine_targets_listed`. Shared rig: `crates/am-e2e/tests/engine/support/mod.rs` (a folder, not a target); `handoff_gate.rs` also includes `tests/trust/support/server.rs`.

**Dev-dependencies** (workspace-pinned only): am-engine `tokio` with `test-util` (paused time; `xtask/tests/paused_time.rs`); am-e2e `serde` (probes) and `uuid`; am-config `uuid`. Cargo.lock gained only those edges.

### Red run (tester, 2026-11-23)

Shared `CARGO_TARGET_DIR` (`~/cargo-target/agentic-manager`), unchanged. Each target ran alone (`cargo nextest run -p <crate> --test <target> --no-fail-fast`), since one compile failure aborts a whole nextest build; plus `cargo test -p xtask --test check_deps`. No `src/` file changed (`git status --short | grep /src/` is empty).

| Target | New/changed tests | Observed | Plan's expected red |
|---|---|---|---|
| am-e2e `handoff_next` | 3 | compile: no `StepPlan.{seat,inputs,hand_off,limits}`, `HandOff`, `Part::Trusted`, `StepRow.{attempt,taint}`, `Reader::{step_inputs,inbox_items,tool_calls}`, `StepInput`, `Taint` (25 errors) | compile (×2); `the_cli_runs_the_three_step_fixture`: assertion, validate reports the key unknown → see probe below |
| am-e2e `handoff_loop_back` | 7 | compile (30 errors, same kinds + `InboxKind`) | compile |
| am-e2e `handoff_gate` | 7 | compile (26 errors, + `PauseStep`, `NewInboxItem`) | compile; the CLI one: assertion (probe) |
| am-e2e `taint_handoff_chain` | 7 | compile (37 errors, + `run_plan_with_tools`, `FixtureTool`) | compile |
| am-e2e `taint_at_rest` | 2 | compile (12 errors: `StartAttempt`, `ReserveCall.taint`, `PauseStep`, `StepRow.taint`) | compile |
| am-e2e `taint_trusted_input_sources` | 6 | compile (27 errors: `TrustedInput`, `Part::Trusted`, `Taint`, `StartAttempt`) | compile; the spike-resume one: assertion (probe) |
| am-e2e `taint_trusted_input_refusals` | 2 | compile (1: no `am_domain::TrustedInput`) | compile (positive doctest missing) |
| am-e2e `rule_16_audit_log` (changed) | 0 new | compile (`ReserveCall.taint`, `Taint`) | — (literal updated) |
| am-e2e `trust_targets` | +1 | green, 3/3 (`engine_targets_listed` included) | green on arrival ✓ |
| am-e2e `rule_15_events` (unchanged) | 0 | green, 9/9 | green ✓ |
| am-engine `loop_caps` | 6 | compile (13 errors: `Limits`, `HandOff`, `run_plan_with_tools`, `FixtureTool`, `InboxKind`, `InboxItemRow`, `ToolCallRow`) | compile ✓ |
| am-engine `run` (changed) | 0 new | compile (6: new `StepPlan` fields, `HandOff`, `Limits`, `Taint`, `TrustedInput`) | — (helper updated) |
| am-store `inbox` | 6 | compile (20 errors: the commands, `Reader` methods, row types) | compile; CHECK one and `nothing_leaves_waiting_human`: assertion (probe) |
| am-store `run_journal` | +2 | compile (18 errors) | migration one: assertion, version 6 (probe); loop-back one: compile |
| am-store `audit` (changed) | 0 new | compile (`ReserveCall.taint`, `Taint`) | — |
| am-config `hand_offs` | 6 | compile (15 errors: `Step.{loop_back,gate,limits}`, `LoopBack`, `Gate`, `StepLimits`) | assertion "unknown key" → see validate below |
| am-config `writer_actor` | 2 | **assertion**, 0/2: `a_seat_or_assistant_actor_cannot_edit_config` panicked at `expect_err("an agent never writes config")` with the Seat's `HistoryEntry` (the edit applied); `…create_undo_or_restore` panicked at `expect_err("create")` (the Seat's create applied) | assertion, the writer applies the edit for any actor ✓ |
| am-app `run_cli` | +2 (+1 changed) | compile (1: no `am_domain::{Taint, TrustedInput}`) | `status_lists…`: assertion, fixture refused (probe); `run_graph_keys…`: green on arrival — not observable while the binary does not compile |
| xtask `check_deps` | +1 | 12/13; `trusted_input_constructors_are_called_only_from_am_engine` **assertion** "an adapter: must fail" (exit 0; the allowed half passed) | assertion, the check passes it ✓ |
| xtask `seam_guard` | +1 | 6/7; new test **assertion** "agentic run: must fail" | (not in list) |
| xtask `paused_time`, `denylist`, `release_deps` | 0 | green 2/2, 8/8, 5/5 | green ✓ |

Compile reds are the expected ones: every error names an API this plan adds; none is a mistake in a test. Before the run, the tests were type-checked (and clippy-clean for `--tests`) against a throwaway stub of the API above in a scratch copy of the workspace (never in this repo); the refusal probes passed on the stub and failed when a `From<UntrustedData> for TrustedInput` was planted, and the doctest pattern above was run there.

Assertion reasons the plan names, for tests whose binary is compile-red, observed on today's code with a temporary probe test (deleted after) and the CLI:
- `agentic validate --template fixtures/projects/hand-offs`: exit 1, exactly three findings, `unknown key 'limits'` (capped.yaml:9), `'gate'` (gated.yaml:12), `'loop_back'` (relay.yaml:15); so `agentic run relay` and `agentic run gated` exit 1 with "the project's config has 3 errors … No run was started" (matches the CLI rows and the `hand_offs` finding rows; the first error `agentic run` names is `limits`, from another file of the fixture, the same cause).
- `journal::drive_run` puts the earlier step's output inside an `Instruction` part ("… Earlier steps said: A-COMMITTED-OUTPUT …"), as the plan says.
- Today's `StartStep` on a `succeeded` step sets it `running` (no transition table).
- `SCHEMA_VERSION` is 6 and there is no `inbox_items` table.

Housekeeping: the scratch type-check shared the target dir, and Cargo's workspace-relative path hashing let its stub artifacts shadow this repo's crates (one run reported the stub's fields). `cargo clean -p` for every workspace crate removed those artifacts (about 61 GiB of build cache, rebuilt by the next build); the red run above came after that.

### Implementation (implementer, 2026-11-23)

Built on branch `story/3-3`, in this checkout, against the shared `CARGO_TARGET_DIR`; no commit. No test file written by the tester was changed (their mtimes all predate the first `src/` edit). Every target of the red run is green.

**Per crate:**
- **am-domain:** `Taint { Clean, Tainted }` (`Ord`, `raise`, `of_flag`), `TrustedInput` (private `label`/`text`, `Debug, Clone, PartialEq, Eq` only; `from_step_output(label, text, producer) -> Part`, `from_owner_note(label, text, &Actor) -> Option<_>`, `label()`, `text()`) and the five doctests of the notes (two allowed builds, three `compile_fail`) in `content.rs`, whose module doc now names the three types; `Part::Trusted`; new `inbox.rs` with `InboxKind` (`ALL`, `as_str`, `parse`, `title`) and `StepStatus` (AD-8's seven states, `as_str`, `parse`, `is_final`); the `ToolRunner` port in `tool_manifest.rs`. All exported at the root.
- **am-store:** migration `0007_steps_inputs_inbox.sql` (`step_runs` rebuilt with `attempt`, `taint`, `loop_back_from` under `UNIQUE (run_id, step_key, attempt)`; `step_inputs`, `tool_calls`, `inbox_items` with the kind and status CHECKs and `UNIQUE (step_run_id, kind)`; triggers that refuse lowering `step_runs.taint` or `inbox_items.taint`). The migrator runs every step with foreign keys off and checks `pragma_foreign_key_check` inside the step's transaction (SQLite's table-rebuild procedure), behind AD-7's unchanged pre-migration copy. Commands `StartAttempt`, `SettleAndLoopBack`, `PauseStep`, `FailWithItem`, `RecordToolCall`, `FinishToolCall`; `ReserveCall.taint`. The writer's transition table (`require`): start only from `queued`; finish, settle-and-finish, loop back, fail, reserve a call and record a tool call only from `running`; pause from `queued` or `running`; nothing leaves `waiting_human` (refusals name the state). Item creation refused for any actor but `Actor::System`, before anything. Taint is raise-only (writer and triggers); an input or item carries at least its producer's or payload's committed taint. A command may now write several audit entries (`step.waiting` + `inbox.created`; settlement or `step.finished` + `inbox.created` + `run.finished`). Readers: `steps` (every attempt, by `(seq, attempt)`), `calls_for_step`, `step_inputs`, `tool_calls`, `inbox_items`. The `compile_fail` doctest naming `WriteCommand::ResolveInboxItem` is on `Writer::exec`.
- **am-config:** `Step.loop_back`, `Step.gate`, `Step.limits` with `LoopBack`, `Gate`, `StepLimits` (defaults 8 / 20 / 600 s); `Fields::duration` (`<n>s|m|h`); the per-step parsing (`step_loop_back`, `step_gate`, `step_limits`) and the cross-step checks (`hand_off_checks`: target unknown, not earlier or itself; a gate first; a gate with `loop_back` or `limits`), each a finding with file, line and fix. The writer refuses `Actor::Seat` and `Actor::Assistant` first in `apply`, `apply_with_audit`, `create`, `undo`, `restore`, `assign_identity`, `apply_unchanged_with_audit` and the shared `step`/`write_step` (new `WriteError::AgentWrite`, words "refused: <actor> cannot change the project's config …").
- **am-engine:** `run.rs` rewritten around a drive loop that reads the run's attempts from the journal each time and runs the first step whose latest attempt is `queued`. `StepPlan` gains `seat`, `inputs`, `hand_off`, `limits`; `HandOff`, `Limits`; `run_plan` unchanged in signature (no tools); `run_plan_with_tools` (the test-only seam). A step attempt: inputs (previous step's output by `next`, the loop-back reasons when the row names `loop_back_from`, the plan's untrusted inputs) each through `TrustedInput::from_step_output` with the producer's committed taint; `StartAttempt` with inputs and taint before the first call; a loop of turns (reserve with the carried taint, call within the wall-clock deadline on a child token, then: provider failure / cut-off / cancel fail as in 3.1; tool requests go through the iteration cap, `SettleCall`, then per call the tool-call cap, ToolGate (refusals audited through the store), `RecordToolCall issued`, the run, `FinishToolCall`; results appended as `Part::ToolResult` and the attempt tainted with the next reservation; an end turn takes the step's hand-off). Loop-back: JSON object with the named field as a list of strings (anything else fails in words naming the field and "list of strings"); empty is `next`; the count is the producer's `looped_back` attempts in the journal; past `max`, `FailWithItem` with the output stored. Gate: `PauseStep` with one Review item (payload the previous attempt, taint copied), no model call. `journal.rs` (spike) no longer wraps earlier outputs in `Instruction::authored`: each is a `TrustedInput` or fenced `UntrustedData` by the producer's committed taint, journaled with `StartAttempt`.
- **am-providers:** `render_trusted_input` in `common.rs`, the one renderer both adapters use for `Part::Trusted` (labelled, own user-role block, outside the fence, opening and closing fence markers neutralised); the wrong-role arms of both adapters now name every `Part` variant (AD-10: "every adapter matches each Part variant by name").
- **am-testkit:** `FixtureTool` (`NAME = "fixture_read"`, `RESULT`, `new()`, `calls()`), a read-class `ToolRunner` with `max_uses_per_step` 100 returning `UntrustedData::text("tool:fixture_read", RESULT)`.
- **am-app:** `plan_of` maps `loop_back`, `gate` and `limits`; a gate step gets the stand-in provider `NoModelAtGate` (never called, never asked of the factory, so its seat needs no model); `report` prints `run <id> waiting_human` and `Review item <id> waits for you in the Inbox`, exit 0, and for a failed run with a Failure item adds `agentic: run <id>: Failure item <id> is in the Inbox` on stderr; `status` prints one line per attempt (`<key>  attempt <n>  <status>  <duration>`).
- **xtask:** `check-deps` takes `--root` and adds the call-site rule (`from_step_output` / `from_owner_note` named in shipped code only under `crates/am-engine/src/` and on am-domain's `fn` definition lines); `seam-guard` gains `run_plan_with_tools` (home `crates/am-engine/src/run.rs`).
- **justfile:** `doctest` runs `cargo test --doc -p am-store -p am-domain`; `validate` includes `fixtures/projects/hand-offs/`.

**Deviations from the Phase 1 test API (all additive; no test changed):**
1. `RunOutcome` gains `item: Option<String>`: the Review item a waiting run waits on, or the Failure item a failed run raised. am-app prints the Review line from it.
2. `StepRow` gains `loop_back_from: Option<String>` (column `step_runs.loop_back_from`): set by `SettleAndLoopBack` on the target's new attempt, so the engine finds the reasons that attempt receives in the journal, never in memory (decision from committed rows).
3. Extra store API: `WriteCommand::RecordToolCall` / `FinishToolCall` (tool calls `issued` before the tool runs, then `succeeded`/`failed`, or `refused`; audited `tool_call.recorded` / `tool_call.finished`) and `Reader::calls_for_step`.
4. `StartStep` keeps 3.1's audit entry (`step.started`, args `{}`) and, on an unknown id, 3.1's no-change behaviour (the plan: story 3.18's); `StartAttempt`'s entry carries `{"inputs": n, "taint": 0|1}`.
5. Wall-clock deadline: the attempt's persisted `started_at` plus the cap, less the whole seconds elapsed since then, on tokio's clock (a fresh attempt gets the full cap). Whole seconds, because a sub-second real-time offset would make the paused-time test wait under 30 s; story 3.9 inherits the persisted source.
6. `ReserveCall` now requires a `running` step and `FinishStep` / `SettleAndFinishStep` accept only `succeeded`, `failed`, `cancelled` (AD-8; `running → cancelled` kept for the spike's resume test). am-store's own three src unit tests now start their step before reserving.
7. A ToolGate refusal (unknown tool, a per-step maximum) when a runner is given fails the step in words, with no Failure item (not one of the story's caps); a tool's own error comes back to the model as an `UntrustedData` result.
8. `step_inputs.kind` has no CHECK (the writer checks the three kinds), so story 3.17 adds kinds without a table rebuild.

**Points for the coordinator (none blocks the story):**
- *"No new dependency":* no crate entered the graph, but am-engine gained two normal edges on workspace-pinned crates already locked: `serde_json` (reading a loop-back step's JSON output, AD-36) and `uuid` (moved from dev to normal: a tool call's `Actor::Seat.run` is a `Uuid`). Cargo.lock's only extra change is `serde_json` in am-engine's list. Writing a JSON reader by hand was the alternative.
- *`inbox_items.status` CHECK (`'open'`)*, as the plan asks: story 3.4 needs a migration that rebuilds `inbox_items` to add its resolved states.
- *For epic 4 story 17's `is_widening`* (no code here; Decision 1 already records it): the new config fields are `loop_back` (`to`, `reasons`, `max`), `gate` and `limits` (`iterations`, `tool_calls`, `wall_clock`); raising `max` or a limit, adding `loop_back`, removing `gate:` or moving a gate step to a seat with a model widen. Accepted interim risk (owner condition 4): until 4.17 a hand edit can weaken a gate for a new run; a waiting gate is never released (row 32 green).

**Overrun:** none. One story; no scope beyond the plan, no test cut.

**`just check`** (2026-11-23, after the last change): exit 0. fmt clean; clippy `-D warnings` clean; nextest 1118 passed, 8 skipped (the `#[ignore]`d ones, all from before this story: five live spike tests, two cargo-deny supply-chain probes, am-app's `lock_holder_child` helper); doctests am-domain 2 passed + 3 `compile_fail`, am-store 1 passed + 3 `compile_fail`; `check-deps` (21 crates follow AD-4; only am-engine builds a TrustedInput), `check-forbid`, `denylist`, `runtime-paths`, `fixed-hosts`, `seam-guard` clean; `agentic validate` 0 errors on every valid fixture (`hand-offs/` included) and template.

### Review triage, round 1 (coordinator, 2026-11-23)

Lenses: quick (correctness and spec) and adversarial (security, with static-guard probes). No high finding. Routes: **patch** (tests first), **defer** (named owner), **accept**.

| Id | Finding | Sev | Route |
|---|---|---|---|
| P1 | Wall-clock deadline cut by up to 1 s (difference of truncated seconds); `loop_caps` wall-clock test flaky | medium | patch: a fresh attempt's deadline from tokio's clock right after `StartAttempt`; elapsed time rounded down only for a resumed attempt |
| P2 | The tool runner is called by run.rs, not by ToolGate; ToolGate ignores `ActionClass` | medium | patch: ToolGate checks then runs (run.rs never holds the runner); any class other than `Read` refused on this path until 3.6/4.8 |
| P3 | `PauseStep` takes `queued → waiting_human`, an edge AD-8 does not have (a plan error against a binding AD) | medium | patch: pause goes `queued → running → waiting_human` in one transaction, stamping `started_at`; plan Boundaries corrected; owner told |
| P4 | Raw C-string literals (`cr#"…"#`) desynchronise xtask's source stripper, hiding code from check-deps, seam-guard, fixed-hosts and the other scans | medium | patch: every Rust 2024 literal prefix; fail closed on an unterminated literal |
| P5 | Call-site guards scan only `src/**`; `#[path]`, `include!`, build.rs output and exported macros escape | medium | patch: scan every `.rs` under a crate except `tests/`, `benches/`, `examples/`, plus `build.rs`; refuse `#[path` and `include!` of `.rs` and `macro_rules!` in shipped source; seam-guard bans `ToolRunner` impls and uses outside am-domain and am-engine |
| P6 | `journal::drive_run` fresh run no longer passes earlier outputs (resume and fresh prompts differ) | low-med | patch: push each settled step's journal row onto `earlier` |
| P7 | Wall clock not checked before a turn or tool; `select!` not biased | low | patch |
| P8 | ToolGate per-step counts shared across attempts | low | patch: key by step run |
| P9 | `step_runs.loop_back_from` stores the loop-back edge twice (AD-43: never stored twice) | low | patch: remove the column; the loop-back `step_inputs` row for the target's new attempt is written in the loop-back transaction |
| P10 | Migration foreign-key check fails on any pre-existing broken reference | low | patch: compare with the count before the step |
| P11 | Matrix test gaps: row 31 (attempt row published once), row 27 (`agentic status` shows the waiting item), row 26 (`agentic run` starts nothing) | low | patch: tests (and `agentic status` prints the open item) |
| P12 | Doc comments now wrong (run.rs module doc, `StepPlan` "none for a gate", toolgate "single choke point" after P2, "insert-or-no-change" in the Design Notes) | low | patch |
| P13 | `Uuid::parse_str(..).unwrap_or_default()` gives a nil seat-run id | low | patch: error in words |
| P14 | `Instruction::authored` has no call-site rule; `render_trusted_input` is `pub` | low | patch: check-deps call-site rule for `Instruction::authored` (allowed sites enumerated); `render_trusted_input` `pub(crate)` |
| P15 | Model-chosen tool names reach the journal, audit and terminal unfiltered | low | patch: unknown names stored as `unknown` plus a sha256; length cap and control characters stripped before storing or printing |
| P16 | Store-level guards the engine alone enforces: `FinishRun` while a step waits; item taint not raised from the gate attempt; inputs from an unfinished producer; loop-back past a waiting step; no taint in `model_call.reserved` audit | low | patch all five in the writer |
| D1 | Row 18's count tests cannot tell journal-derived from in-memory counting | low | defer: story 3.9 (resume), where a fresh engine reads an existing journal |
| D2 | Blocking `Reader::open` per turn on the async executor | low | defer: one reader per run, story 3.9 (pre-existing pattern) |
| D3 | Loop-back max and caps checked before the write, not inside the transaction | low | defer: 3.9 and 3.14 (second driver) |
| D4 | `StartStep` remains a second start path | low | defer: story 3.18, as planned |
| D5 | The untrusted fence and the trusted header have no per-request random value; near-miss closing tags are not neutralised | low (plausible) | defer: epic 4 story 9 (injection pack); fence format is AD-27's, told to the owner |
| D6 | `TrustedInput::from_step_output` takes a bare `Taint` rather than a journal-row proof | low | defer: with D1 in 3.9 |
| A1 | am-engine gains normal edges on `serde_json` and `uuid` (both pinned and already in the graph; no new crate) | — | accept (coordinator), told to the owner |

### Review patch round 1: red run (tester, 2026-11-23)

Tests for P1–P16 (no test for P12, which is doc comments only), written before the patches. The tester changed no `src/` file: the newest `src/` mtime, 13:12, is before this round started; `git status --short | grep /src/` still lists the implementer's 23 files. The workspace was not copied anywhere; everything ran in this checkout on the shared `CARGO_TARGET_DIR`. The new tests are appended to existing files, so there is no new target (`trust_targets` and `engine_targets_listed` stay green). New dev-dependencies, all workspace-pinned: am-engine `sha2`, `hex` (P15) and `rusqlite` (P1); Cargo.lock gained only those edges. `cargo clippy --all-targets -D warnings` is clean on am-engine, am-store, am-e2e, am-app and xtask; fmt, `denylist`, `seam-guard`, `check-deps` and `fixed-hosts` pass on the repository.

**Commands.** `cargo nextest run -p <crate> --test <target> --no-fail-fast`, one target at a time, and `cargo test -p xtask --test check_deps`.

**Results per target** (old tests all green):

| Target | Run | Passed | Failed |
|---|---|---|---|
| am-engine `loop_caps` | 16 | 7 | 9 |
| am-store `inbox` | 12 | 6 | 6 |
| am-store `run_journal` | 9 | 7 | 2 |
| am-e2e `handoff_gate` | 9 | 7 | 2 |
| am-e2e `taint_trusted_input_sources` | 7 | 6 | 1 |
| am-e2e `taint_trusted_input_refusals` | 3 | 2 | 1 |
| am-app `run_cli` | 32 | 31 | 1 |
| xtask `check_deps` | 17 | 13 | 4 |
| xtask `seam_guard` | 11 | 7 | 4 |
| xtask `fixed_hosts` | 8 | 7 | 1 |

These targets are green: `handoff_next` (4), `handoff_loop_back` (8), `taint_handoff_chain` (7), `taint_at_rest` (2), `trust_targets` (3), `rule_15_events` (9), am-engine `run` (7), am-store `audit` (20), xtask `paused_time`, `denylist` and `release_deps`.

| Id | Test (file) | Observed |
|---|---|---|
| P1 | `a_fresh_attempt_gets_its_whole_wall_clock_cap` (`am-engine/tests/loop_caps.rs`) | red: waited 20 s, not 30 s. The step before moves the queued attempt's `started_at` 10 s back (raw SQL; `StartAttempt` keeps a `started_at` already set), and the deadline subtracts real time from a fresh attempt. |
| P1 | `a_fresh_attempt_started_just_before_a_second_boundary_waits_the_full_cap` (same) | red: waited 29 s. `started_at` is set to the last whole second minus 1 ms, which is deterministic: "now" is always past that boundary, the flake's case. The existing `the_wall_clock_cap_cancels_and_settles_the_call` is unchanged and green. |
| P2 | `a_tool_whose_class_is_not_read_is_refused_and_never_runs` (same) | red: "LocalWrite: the runner never ran it". A test-local `ToolRunner` with a `LocalWrite` (then `ExternalEffect`) manifest was run. |
| P2 | `only_toolgate_runs_a_tool` (same) | red: "run.rs runs a tool past ToolGate (`runner.run(`)". A heuristic structural scan of am-engine's `src/`, comments removed and whitespace dropped: outside `toolgate.rs`, no `runner.run(`, `.runner.` or `ToolRunner::run(`. |
| P7 | `a_reply_at_the_deadline_loses_to_the_deadline` (same) | red at trial 2 of 16: a tool call was recorded. A reply delayed exactly to the deadline won the unbiased `select!`. |
| P7 | `nothing_starts_after_the_deadline` (same) | red at trial 0: the second tool (`fast_read`) was recorded after the first tool ended exactly at the deadline. 16 trials. |
| P8 | `a_new_attempt_starts_its_tool_uses_at_zero` (same) | red: run `failed`, "Tool \"once_read\" was refused: this step already used it 1 times". |
| P13 | `a_tool_call_acts_as_the_seat_of_this_run` (same) | green on arrival, a guard: the refusal is audited as `Actor::Seat` of this run's id, not nil. |
| P13 | `an_unparsable_run_id_is_never_a_nil_uuid` (same) | red: "run.rs: … `parse_str(&run_id).unwrap_or_default()`". A behavioural test cannot reach a malformed run id, because `run_plan` mints it; this structural scan forbids `unwrap_or_default()`/`unwrap_or(` after `parse_str(` and any `Uuid::nil(` in am-engine's `src/`. |
| P15 | `an_unknown_tool_name_is_kept_as_unknown_and_its_hash` (same) | red: "unknown: exfiltrate_every_key". The `tool_calls.tool` holds the raw name. Pins for each of the three names (plain unknown, control characters, 10 KB):<br>- the row's `tool` starts with `unknown`, contains the lowercase sha256 hex of the raw name's UTF-8 bytes, has no control character, and is at most 256 characters;<br>- the audit export never holds the raw name, `\u001b` or `\u0007`, and does hold the sha256;<br>- the outcome's reason is printable, under 2000 bytes, and holds no raw name. |
| P15 | `a_tool_name_is_never_printed_raw` (`am-app/tests/run_cli.rs`) | red: "control characters: the raw name is printed". `agentic run` output has no raw name, no control character but `\n`, is under 4096 bytes, and still says "tool". |
| P3 | `a_paused_gate_passes_through_running` (`am-store/tests/inbox.rs`) | red: the waiting gate's row has `started_at: None`. Pins: `started_at` set, then audit `step.started` before `step.waiting` on the gate. |
| P3 | `a_gate_attempt_starts_before_it_waits` (`am-e2e/tests/engine/handoff_gate.rs`) | red: the same, through the engine. |
| P16 | `a_run_does_not_finish_while_a_step_waits` (`inbox.rs`) | red: `FinishRun succeeded` was accepted with a step `waiting_human`. |
| P16 | `an_item_is_never_cleaner_than_its_attempt` (same) | red: "payload false: the item carries its attempt's taint". A gate attempt tainted by its own untrusted input made a clean item, with no payload and with a clean one. |
| P16 | `an_input_from_an_unfinished_producer_is_refused` (same) | red: "from a queued producer" was accepted. Also pins: a running producer is refused, a pause's input too, and a succeeded producer is taken. |
| P16 | `a_loop_back_never_crosses_a_waiting_step` (same) | red: "back to draft, across the waiting gate" was accepted. |
| P16 | `the_reservation_audit_carries_the_taint` (same) | red: `model_call.reserved` args have no `taint`. Pinned: the attempt's taint after the reservation (a clean command on a tainted attempt says tainted). |
| P9 | `the_loop_back_input_is_journaled_by_the_loop_back` (`am-store/tests/run_journal.rs`) | red: "the edge is not stored twice (AD-43)": the `loop_back_from` column exists. Also pins: right after `SettleAndLoopBack`, the target's still-queued new attempt has exactly one input (kind `loop_back`, from the producing attempt, label `loop-back:check@1`, producer's taint) and the other new attempts have none. The engine's existing `step_three_loops_back…` test already pins that the start adds no second `loop_back` row. |
| P10 | `a_reference_broken_before_the_migration_does_not_fail_it` (same) | red: `MigrationFailed { from: 6, to: 7, … "the migration would leave 1 broken reference(s)" }`. The schema-6 database held one orphan `model_calls` row written with foreign keys off. |
| P6 | `a_fresh_spike_run_passes_each_earlier_output_as_a_resumed_one_does` (`taint_trusted_input_sources.rs`) | red: "Clean: step a's output reaches b". `found.len()` was 0: on a fresh `drive_run`, b's request carries nothing of a. |
| P14 | `the_trusted_renderer_is_the_providers_own` (`taint_trusted_input_refusals.rs`) | red: "lib.rs: `pub use common::{render_trusted_input, …}` re-exports it". Also requires the definition not be `pub fn`. |
| P14 | `instruction_authored_is_called_only_at_its_allowed_sites` (`xtask/tests/check_deps.rs`) | red: "an adapter: must fail". Allowed in the fixture: am-domain's definition, `am-app/src/run.rs`, `am-engine/src/journal.rs`, tests, comments and literals. Refused, naming the file plus `authored` and `Instruction`: am-tools, am-providers, a second site in am-domain (`llm.rs`), and a renamed import in am-server. The real allowed list is the implementer's: today's sites are am-app `run.rs`, `spike.rs`, `accounts.rs`, am-engine `journal.rs`, and am-engine `run.rs`'s `cfg(test)`. |
| P4 | `a_literal_prefix_never_hides_a_trusted_input_call` (`check_deps.rs`) | red on its first case, `cr#"\"#` (exit 0, the call hidden). A probe of the built xtask showed `cr"\"` hides too, while `c"a\"b"`, `br##"a"#"##` and `r#"say "hi""#` are already found (guards). |
| P4 | `an_unterminated_literal_fails_check_deps_closed` (same) | red: exit 0 on `"never closed;`. The case list also has `r#"…` and `cr#"…` left open. |
| P5 | `a_trusted_input_call_outside_src_is_found` (same) | red: "build.rs: must fail". Other cases: a `#[path]` module's file, an `include!`d file, and any other `.rs` under the crate are each named. Calls in `tests/`, `benches/` and `examples/` pass. |
| P5 | `a_tool_runner_outside_the_domain_and_engine_fails` (`xtask/tests/seam_guard.rs`) | red: "an adapter's implementation: must fail". Allowed: am-domain (definition), am-engine, am-testkit, and any `tests/`. am-testkit is allowed beyond the triage's "am-domain and am-engine" wording, because `FixtureTool` lives in its `src/` (owner condition 2) and the repository check must stay green. Refused, naming the file and `ToolRunner`: an impl in am-tools, an import in am-app, a parameter in am-server. |
| P4/P5 | `the_engines_tool_seam_is_found_after_any_literal_and_outside_src` (same) | red: "after a raw C string: must fail". Also `build.rs` and `extra/start.rs`. |
| P4 | `an_unterminated_literal_fails_the_guard_closed` (same) | red: exit 0. |
| P5 | `path_include_and_macro_rules_in_shipped_source_are_findings` (same) | red: "a #[path] module: a finding naming the file and `#[path`: []". Either `seam-guard` or `check-deps` may report it, naming the file and the construct text (`#[path`, `include!`, `macro_rules!`). `include_str!` and `include_bytes!` pass both. |
| P4 | `a_host_after_or_inside_a_c_string_is_found` (`xtask/tests/fixed_hosts.rs`) | red: "after a raw C string ending in a backslash: must fail". Other cases: a host inside a raw C string that holds quotes, a host in `c"…"`, and an unterminated literal (named file). |
| P11 | `each_new_attempt_row_is_published_once` (`handoff_loop_back.rs`, with the 3.2 server rig) | row 31, green on arrival (gap coverage). All six attempt rows are published as `step.updated`, each ends in its final status, and no state is published twice. |
| P11 | `the_status_shows_the_open_review_item` (`handoff_gate.rs`) | row 27, red: `agentic status` names no item. Pinned: a line holding the item id and "Review". |
| P11 | `a_hand_off_config_error_or_a_run_graph_key_starts_no_run` (`handoff_next.rs`) | rows 26 and 30, green on arrival (gap coverage). Cases: a loop-back to itself, `max: 9`, `iterations: 500`, a gate first, and a `handoff:` key. Each exits 1, points at `agentic validate` or says "cannot honour", calls no model and writes zero run rows. |

### Review patch round 1 (implementer, 2026-11-23)

Every red test of the tester's round-1 run is green; no test file was changed (their mtimes all predate this round's first edit). Same checkout and shared `CARGO_TARGET_DIR`; no commit. No scope beyond the triage rows.

**Per row:**
- **P1:** a fresh attempt's deadline is tokio's clock right after its `StartAttempt` commits plus the cap (`run.rs`); the persisted `started_at` is no longer read for it (story 3.9's resume will count a resumed attempt's, rounded down).
- **P2:** ToolGate is the only caller of a `ToolRunner`. `ToolGate::over(runner, audit)` builds the gate from the runner's manifests; `admit(actor, step_run, call) -> Result<Permit, String>` decides; `run(permit, cancel)` runs. `Permit` has a private field, so only `admit` makes one, and `run.rs` never holds the runner. Every class but `Read` is refused in words (and journaled `refused`) until stories 3.6 and 4.8. `check` stays for the existing unit tests. The toolgate module doc now says it checks, then runs.
- **P3:** `PauseStep` on a `queued` step starts it in the same transaction (`running`, inputs and taint, `started_at` stamped, audited `step.started`), then sets `waiting_human` (audited `step.waiting`, then `inbox.created`). A `running` step pauses directly. The plan's Boundaries line is corrected. In `agentic status`, a waiting step's duration column now reads `waiting`.
- **P4:** the stripper (`xtask/src/forbid.rs`) reads `r`, `br` and `cr` raw prefixes with any number of hashes; `b"…"` and `c"…"` were already normal strings. `unterminated()` and `unterminated_finding()` are new, and check-deps, seam-guard, fixed-hosts, runtime-paths and check-forbid fail closed with a finding naming the file and line. See conflict 1 for the fixed-hosts case.
- **P5:** `shipped::sources` now reads every `.rs` of a crate but its `tests/`, `benches/` and `examples/` (so `build.rs`, `#[path]` targets and `include!`d files too). seam-guard reports `#[path`, `include!` and `macro_rules!` in shipped source as findings naming the file and construct; `include_str!` and `include_bytes!` pass. It also allows the name `ToolRunner` only under `crates/am-domain/src/`, `crates/am-engine/src/` and `crates/am-testkit/src/`. The repository itself had no `macro_rules!`, `#[path]` or `include!` in shipped code. Its only `include_*!` uses are `include_str!` in `am-store/src/migrations.rs` and `user.rs` and in `am-server/src/routes.rs`; the one non-`src` `.rs` is `am-desktop/build.rs`. All pass.
- **P6:** `journal::drive_run` pushes each step it settles, re-read from the journal (output and committed taint), onto the earlier outputs. A fresh run therefore passes them as a resumed one does.
- **P7:** both `select!`s (model call, tool run) are `biased` with the deadline first. Before every reservation and every tool call, a passed deadline ends the attempt with its Failure item.
- **P8:** ToolGate's uses are counted per (run, step run, tool) through `admit`, so a new attempt starts at zero.
- **P9:** `step_runs.loop_back_from` is gone (0007 edited in place; not shipped). `SettleAndLoopBack` writes the target's new attempt's `loop_back` input in its own transaction: from the producer, labelled `loop-back:<key>@<attempt>`, with the producer's taint. The engine reads that row at the attempt's start to build the reasons part and does not journal it again. `StartAttempt` raises the attempt's taint from inputs already journaled for it.
- **P10:** a migration step counts broken references before and after its SQL, in its transaction, and fails only when the count grew.
- **P11:** `agentic status` prints each open Inbox item under its run as `<Kind> item <id>  open` (kind and id only, never its words). Rows 26, 30 and 31 were green on arrival.
- **P12:** `run.rs` module doc (wall clock, journaled loop-back input, ToolGate runs tools, tool labels), the `StepPlan` doc ("a gate's provider is never called"), the `Inputs` doc, the toolgate module doc, and the plan's Design Notes line that said "insert-or-no-change": a second pause is refused and changes nothing.
- **P13:** the new run id is parsed to its UUID before `CreateRun`; a failure is a `RunError::Store` in words. There is no `unwrap_or_default`.
- **P14:** check-deps gains the `Instruction::authored` call-site rule. In shipped code the name `authored` is allowed only at the sites below. `render_trusted_input` is `pub(crate)` and am-providers re-exports only `render_untrusted_text`.
- **P15:** a model-chosen tool name is journaled, audited (`tool_call.recorded`, `toolgate.refused`) and put into the run's words only as `tool_label`. A name a manifest holds (the runner's, or a built-in tool id) stays as itself; any other becomes `unknown:<sha256 hex of its UTF-8 bytes>` (72 printable characters). This applies on the tools path and on the no-tools path (3.1's words), so `agentic run` prints no raw name. The hash comes from `am_store::audit::sha256_hex`, now `pub`, so am-engine gained no new crate edge.
- **P16:** all five guards are in the writer:
  - `FinishRun` is refused while a step of the run is `waiting_human`.
  - An item's taint is at least its attempt's as committed, in `PauseStep` and `FailWithItem`.
  - An input's producer must be `succeeded` or `looped_back`; a queued, running or waiting producer is refused.
  - A loop-back is refused when any step from the target through the producer is `waiting_human`.
  - `model_call.reserved`'s args carry `taint`, the attempt's after the raise. The entry is now built in `execute`.

**Allowed `Instruction::authored` sites** (`xtask/src/deps.rs`, `INSTRUCTION_SITES` and `INSTRUCTION_TEST_SITES`):
1. `crates/am-domain/src/content.rs`, on its definition line only (`fn authored`), as for `TrustedInput`'s constructors. No other am-domain line may name it.
2. `crates/am-app/src/run.rs`: the plan built from the config snapshot (a seat's instructions, a step's instruction).
3. `crates/am-app/src/spike.rs`: the spike harness's fixed prompts.
4. `crates/am-app/src/accounts.rs`: the Account test's fixed prompt (`TEST_PROMPT`).
5. `crates/am-engine/src/journal.rs`: the spike driver's fixed prompt and system line.
6. `crates/am-engine/src/run.rs`, inside its `#[cfg(test)]` items only: the driver's unit-test plans.

Nothing else in shipped code may name it. Tests, comments and literals are not shipped code.

**Conflicts and choices for the coordinator:**
1. *`fixed_hosts::a_host_after_or_inside_a_c_string_is_found`, its unterminated case:* the source `"never closed;⏎… = "https://telemetry.example.com";` is not lexically unterminated. Its second quote closes the first literal, and `https://…` then lexes as `https:` followed by a `//` comment that swallows the third quote, so a correct lexer finds no open literal. I left the test unchanged and made the scan fail closed on the symptom instead: a `//` comment that opens right after `https:` or `http:` in code position, which no valid code holds (a URL belongs in a literal). That is `url_scheme_before` in `forbid.rs`, with unit tests showing a URL in a comment or a literal still passes. The repository stays clean under it.
2. *`include!`* is refused whatever file it names (it always pulls Rust source), not only a `.rs` one. `include_str!` and `include_bytes!` pass, as the test pins.
3. *P5's `ToolRunner` homes* include am-testkit (`FixtureTool`), per the tester's note.
4. *Fail closed beyond the tests:* check-forbid and runtime-paths report an unterminated literal too, so every scan that reads literals behaves alike.

**`just check`** (after the last change): exit 0.
- fmt and clippy `-D warnings` are clean.
- nextest: 1154 passed, 8 skipped (the same eight `#[ignore]`d tests as before).
- Doctests: am-domain 2 passed plus 3 `compile_fail`; am-store 1 passed plus 3 `compile_fail`.
- check-deps (21 crates follow AD-4; `TrustedInput` and `Instruction::authored` rules) and check-forbid are clean; denylist reads 223 files; runtime-paths 188 files; fixed-hosts 97 files; seam-guard 97 files (it now reads `am-desktop/build.rs` too).
- `agentic validate` reports 0 errors on every valid fixture and template (14 projects).

### Review patch round 1: test correction (tester, 2026-11-23)

This correction follows a coordinator decision. In `xtask/tests/fixed_hosts.rs::a_host_after_or_inside_a_c_string_is_found`, the "unterminated" case was not unterminated. Its second `"` closed the first literal, which left `https:` followed by a `//` comment. The case is now two literals that run to the end of the file:
- a raw C string `cr#"never closed;`, with no closing `"#`, followed by a line holding a quoted host;
- a C string `c"never closed; see https://telemetry.example.com/v1`, with no closing quote and no other quote in the file.

Each must fail the scan and name the file. The rest of the test is unchanged.

`cargo test -p xtask --test fixed_hosts` is **green** against the scanner the implementer has in place now: 8 of 8 pass. The scanner still holds `url_scheme_before` (`xtask/src/forbid.rs`), so the implementer may remove that workaround and re-run this test.

The tester changed no `src/` file. The newest `src/` change is at 13:56:39, before this edit at 14:00:24.

### Review triage, round 2 (coordinator, 2026-11-23)

A focused follow-up (adversarial security) on round 1's patches, with probes against the built xtask and rustc. No high finding.

| Id | Finding | Sev | Route |
|---|---|---|---|
| R1 | A shebang first line (`#!/x "`) desynchronises every literal-stripping scan; rustc discards it | medium | patch: apply rustc's shebang rule (a leading `#!` not followed by `[` blanks line 1), with tests |
| R2 | `#[path` / `include!` / `macro_rules!` refusals match spelling per line: `cfg_attr(.., path = ..)`, `#[r#path`, an attribute split over lines, `use core::include as x`, `include`/`macro_rules` then `!` on the next line | medium | patch: match over the whole flattened file with `r#` stripped; refuse `path` as an attribute name anywhere inside `#[...]` (`cfg_attr` included); refuse importing `include` or `macro_rules`. A full `mod`-tree walk from each crate root is deferred (D7) |
| R3 | Directories named `dist`, `target`, `node_modules` or hidden are skipped at every depth; symlinked directories are not scanned; manifest `[lib]`/`[[bin]]` `path` and `build` keys are not read; unreadable `.rs` files are skipped | medium | patch: skip those names only at a crate's top level; any symlink under `crates/` is a finding; a crate manifest's target `path` or `build` key outside the crate's scanned set is a finding; an unreadable `.rs` is a finding |
| R4 | `FailWithItem` finishes the run without the waiting-step check; start or pause accepted on a finished run or while another step waits | low | patch: the check inside `finish_run`; start and pause refused on a finished run or while another step of the run waits |
| R5 | An item's payload may be an unfinished step run, whose taint can still rise | low | patch: the payload must be finished (`succeeded`, `looped_back`, `failed`) or the attempt itself |
| R6 | Input kind and taint pairing trusted from the caller | low | patch: `untrusted` forces `Tainted` and forbids a producer; `step_output` and `loop_back` require one |
| R7 | "Only ToolGate runs a tool" by convention: `run_on` holds the runner; `Permit` not tied to its gate; duplicate manifest ids kept silently | low | patch: `run_plan_with_tools` builds the gate and passes the gate, not the runner; seam-guard allows `ToolRunner` in am-engine only in `toolgate.rs`; `Permit` carries its gate's id, checked by `run`; duplicate manifest ids refused |
| R8 | `url_scheme_before` protects nothing and false-positives on valid code | low | patch: remove it (the fixture was corrected by the tester) |
| R9 | ToolGate's `admit`/`refuse` write whatever name they get; labelling happens in run.rs | low | patch: labelling inside ToolGate |
| D7 | Full `mod`-tree walk from each crate root so no reachable file escapes the scanned set | low | defer: epic 3 story 12 (the sweep) |
| D8 | Raw provider error text (`ProviderError::Protocol`) reaches the step reason and the terminal (pre-existing) | low | defer: story 3.8 (retries, rate limits and the fallback chain), which owns provider errors |

### Review patch round 2: red run (tester, 2026-11-23)

These tests for R1–R7, R8 and R9 were written before the patches. The tester changed no `src/` file: the newest `src/` change is at 13:56:39, before this round started, and `git status --short | grep /src/` still lists the implementer's 28 files. Everything ran in this checkout on the shared `CARGO_TARGET_DIR`, with no copy. There is one new target, `crates/am-engine/tests/toolgate.rs`, which Cargo auto-discovers. There are no new dependencies and no Cargo.lock change. `cargo clippy --all-targets -D warnings` is clean on am-engine, am-store and xtask; fmt and `denylist` are clean.

The follow-up reviewer's probe trees (`scratchpad/sg1`–`sg7`) became xtask fixture tests. Each fixture writes its tree in a temporary folder, which is not a git work tree, so the scans walk the folder themselves. Each case was also probed alone against the current xtask binary, to tell new reds from regression guards. The seam_guard tests accept a finding from either `seam-guard` or `check-deps` (helper `failures`) when it names the file (and the word).

Commands: `cargo nextest run -p <crate> --test <target> --no-fail-fast` for each target, and `cargo test -p xtask --test fixed_hosts`.

| Target | Run | Passed | Failed |
|---|---|---|---|
| am-engine `toolgate` (new) | 4 | 1 | 3 |
| am-store `inbox` | 17 | 12 | 5 |
| xtask `seam_guard` | 18 | 11 | 7 |
| xtask `check_deps` | 18 | 17 | 1 |
| xtask `fixed_hosts` | 10 | 8 | 2 |

Still green: am-engine `loop_caps` (16) and `run` (7); am-store `run_journal` (9); am-e2e `taint_at_rest`, `handoff_gate`, `handoff_loop_back` and `taint_handoff_chain`.

| Id | Test (file) | Observed |
|---|---|---|
| R1 | `a_shebang_line_hides_nothing` (`xtask/tests/seam_guard.rs`) | red: "after a shebang: … found []". sg1's `a_shebang.rs` hides `use am_domain::ToolRunner`. Guards: an inner attribute on line 1 (`#![…]`, `#! […]`) is code, and today's scan already finds the line after it. |
| R1 | `a_shebang_line_hides_no_call` (`xtask/tests/check_deps.rs`) | red: "after a shebang: must fail". sg4's `hide.rs` hides `from_step_output` and `authored`. |
| R1 | `a_shebang_hides_no_host` (`xtask/tests/fixed_hosts.rs`) | red: "after a shebang: must fail". sg3's host `evil.example.com` is missed. Guard: a host after `#! [forbid(unsafe_code)]` is found. |
| R2 | `path_include_and_macro_rules_are_found_however_spelled` (`seam_guard.rs`) | red on its first case, `cfg_attr(all(), path = …)` ("found []"). Probed alone, these are missed today: `cfg_attr` path; `#[⏎path`; `#[r#path`; `use core::include as grab`; `use std::include`; `include⏎!`; `macro_rules⏎!`. Already found today (guards): a plain `#[path]`, `macro_rules !`, `r#macro_rules!`. Also asserts that ordinary code using the words (a `path` field, `include_all`, `macro_rules_count`, `#[doc = "…path…"]`) is no finding. |
| R3 | `only_top_level_build_folders_are_skipped` (same) | red on `src/dist/mod.rs`. Probed alone, `src/target/`, `src/node_modules/` and `src/.h/` are missed too. Guard: the crate's top-level `target/`, `dist/`, `node_modules/` and `.cache/` stay skipped. |
| R3 | `a_symlink_under_crates_is_a_finding` (same, unix) | red: the symlinked folder `src/linkdir` gives no finding. Probed alone, the symlinked file `src/linkfile.rs` is missed too outside a git work tree (sg2 caught it only because git listed it). |
| R3 | `a_manifest_target_outside_the_scanned_files_is_a_finding` (same) | red: `[lib] path = "tests/hid/lib.rs"` gives no finding naming `Cargo.toml` and `path`. Also covers `[[bin]] path` in `examples/` and `build = "../../tools/build.rs"`. Guard: `build.rs`, `src/lib.rs` and `src/main.rs` pass. |
| R3 | `an_unreadable_source_file_is_a_finding` (same, unix) | red: a mode-000 `src/secret.rs` is skipped silently. The test returns early when run as a user who can read it (root). |
| R7 | `am_engine_names_tool_runner_only_in_toolgate` (same) | red: "run.rs:359: names ToolRunner outside ToolGate: `tools: Option<Arc<dyn ToolRunner>>,`". The scan runs on the repository's am-engine `src/`, comments removed. Outside `toolgate.rs`, `ToolRunner` may appear only in one `use` and once in `fn run_plan_with_tools`'s signature, from `fn` to its `{`. That is the one signature allowed, because the test-only seam takes the runner and builds the gate. |
| R7 | `a_permit_from_one_gate_is_refused_by_another` (`am-engine/tests/toolgate.rs`) | red: gate B ran gate A's permit (`Ok(UntrustedData …)`). Control `a_permit_runs_on_its_own_gate` is green. |
| R7 | `duplicate_manifest_ids_are_refused` (same) | red: the run succeeded and the tool ran. The `twice_named` id was declared external-effect, then read, and the later manifest won. Pinned: the runner never runs it, and either `run_plan_with_tools` errs naming the id, or the run fails naming it with only `refused` tool calls. |
| R9 | `toolgate_labels_the_names_it_audits` (same) | red: "an unknown name: the words: Tool \"exfiltrate_every_key\" was refused". `ToolGate::admit` given a raw name words and audits it raw. Pinned for an unknown name, one with control characters and a 10 KB one:<br>- the refusal has no part of the raw name and no control character;<br>- the audit entry's target contains `unknown:<sha256>`, and none of target, outcome or args holds the raw name or a control character;<br>- a manifest's name (`lookup`) and a built-in id (`web_search`) stay as themselves. |
| R4 | `a_failure_never_finishes_a_run_while_another_step_waits` (`am-store/tests/inbox.rs`) | red: `FailWithItem` on the running `publish` was accepted while `approve` waits. |
| R4 | `nothing_starts_or_pauses_on_a_finished_run` (same) | red: "StartStep on a finished run" was accepted. `StartAttempt` and `PauseStep` are covered too. |
| R4 | `nothing_starts_or_pauses_while_another_step_waits` (same) | red: "StartStep while another step waits" was accepted. `StartAttempt` and `PauseStep` are covered too. |
| R5 | `an_items_payload_is_a_finished_step_run_or_its_own_attempt` (same) | red: "a queued payload" was accepted for a Review. A Failure whose payload is another running step is refused; one whose payload is its own attempt is taken. |
| R6 | `an_inputs_kind_decides_its_producer_and_taint` (same) | red: "an untrusted input is journaled tainted": it was journaled `Clean`. Also refused: `untrusted` with a producer, and `step_output` or `loop_back` without one. |
| R8 | `a_scheme_like_field_before_a_comment_is_no_finding` (`fixed_hosts.rs`) | red: "valid code is not a finding: crates/am-app/src/nospace.rs:2: a string literal or block comment does not end …". This is `url_scheme_before`'s false positive on sg5's `Bar { https:// note⏎ 443 }` and `let http:// typed⏎ u8`. `space.rs` (`https: // note`) passes today. The corrected unterminated cases of `a_host_after_or_inside_a_c_string_is_found` stay green and keep failing closed once the workaround is removed. |

**Adjusted round-1 tests (R4 changes their setup, not what they assert).** In `inbox.rs`, `nothing_leaves_waiting_human` and `a_loop_back_never_crosses_a_waiting_step` now start `publish` and reserve its call *before* the gate pauses. Under R4 nothing starts while another step waits, which their old setup did. Both stay green today.

### Review patch round 2 (implementer, 2026-11-23)

Every red test of the tester's round-2 run is green. No test file was changed: the tester's last test edit is at 14:17:21, and this round's first source edit is at 14:21:26. Same checkout and shared `CARGO_TARGET_DIR`; no commit. No scope beyond R1–R9.

**Per row:**
- **R1:** the literal stripper (`xtask/src/forbid.rs`, `shebang_len`) applies rustc's rule. A file that starts with `#!` not followed, after whitespace, by `[` has its first line blanked. `#![…]` and `#! […]` stay code. Every scan that strips literals inherits this: check-deps, seam-guard, fixed-hosts, runtime-paths and check-forbid.
- **R2:** seam-guard matches constructs over tokens of the whole file (comments and literals blanked; `r#ident` read as `ident`), not per line. The findings are:
  - `path` followed by `=` anywhere inside `#[…]` or `#![…]`, `cfg_attr` included, split lines included;
  - `include` or `macro_rules` followed by `!` across any whitespace;
  - a `use` that names `include` or `macro_rules`, under any alias.
  
  The seam names (`with_seams`, `router_with_fixture`, `FixtureRoute`, `run_plan_with_tools`, `ToolRunner`) are matched as tokens too. A definition is the name right after `fn`. Ordinary uses of the words (a `path` field, `include_all`, `macro_rules_count`, words in a doc literal) are not findings. The D7 `mod`-tree walk stays deferred.
- **R3:** `xtask/src/shipped.rs` now walks `crates/` itself, with no git listing and no symlink followed:
  - `dist`, `target`, `node_modules` and hidden names are skipped only at a crate's top level;
  - any symlink under `crates/`, and any folder or `.rs` that cannot be read, is a finding;
  - a crate manifest's `[lib]` or `[[bin]]` `path`, or `[package]` `build`, that names a file outside the scanned set (outside the crate, under `tests/`, `benches/`, `examples/` or a top-level skipped folder, or not `.rs`) is a finding naming `Cargo.toml` and the key. `[[test]]` targets are not checked: they are not shipped.
  
  seam-guard reports these tree findings. The repository was clean before the change: no symlink, no deep build-named or hidden folder, no git-ignored `.rs`, and only test targets point into `tests/`. The scans read the same 97 shipped files as before.
- **R4:** the waiting-step check moved into the store's `finish_run`, so `FailWithItem` (and `FinishRun`) is refused while another step of the run waits. `StartStep` on a known step, `StartAttempt` and `PauseStep` are refused on a finished run or while another step of the run waits (`run_open_for`). The step's own state is checked first, so a waiting step is still refused in words naming `waiting_human`. `StartStep` on an unknown id still changes nothing (story 3.18's).
- **R5:** an item's payload must be a step run of the same run that is `succeeded`, `looped_back` or `failed`, or the item's own attempt.
- **R6:** an `untrusted` input is journaled `Tainted` and taints its attempt; one that names a producer is refused. A `step_output` or `loop_back` input without a producer is refused.
- **R7:**
  - `run_plan_with_tools` builds the ToolGate (`ToolGate::over`) and hands `run_on` a `Tooling` that holds the gate, never the runner. `Tooling::new` takes the gate.
  - A runner that declares one id twice is refused before any run is created, with the new `RunError::Tools` naming the id. The gate also drops a duplicated id from its manifests and refuses it if asked (`ToolGate::duplicates`).
  - A `Permit` carries its gate's id (from a process-wide counter), and `ToolGate::run` refuses another gate's permit, running nothing.
  - seam-guard allows `ToolRunner` in am-engine only in `toolgate.rs`, plus `run.rs`'s `use` statements and `run_plan_with_tools`' signature. That one exception matches the tester's scan.
- **R8:** `url_scheme_before` is removed. Valid code such as `Bar { https:// note⏎ 443 }` is no finding, and the tester's corrected unterminated cases still fail closed.
- **R9:** labelling now lives in ToolGate (`toolgate::label_of`, `builtin_names`, `ToolGate::label`). `check` and `admit` label the name before they decide, word a refusal or audit it. A manifest's id, a duplicated id and a built-in id stay as themselves; anything else is `unknown:<sha256>`. `run.rs` uses `gate.label` for the journal row and `label_of` with the built-in ids for 3.1's no-tools words. One am-engine src unit test (`unknown_tool_is_refused_and_audited`) now expects the label instead of the raw name. That test is mine, not the tester's, and R9 changes the behaviour it pinned.

**Conflicts:** none. One reading of R7: the triage wording "only in `toolgate.rs`" and the tester's scan, which also allows one `use` and `run_plan_with_tools`' signature in `run.rs`, are reconciled by seam-guard's single exception for that file. The test-only seam has to take the runner it hands to the gate.

**`just check`** (after the last change): exit 0.
- fmt and clippy `-D warnings` are clean.
- nextest: 1174 passed, 8 skipped (the same eight `#[ignore]`d tests as before).
- Doctests: am-domain 2 passed plus 3 `compile_fail`; am-store 1 passed plus 3 `compile_fail`.
- check-deps (21 crates; the `TrustedInput` and `Instruction::authored` rules) and check-forbid are clean; denylist reads 224 files; runtime-paths 189 files; fixed-hosts 97 files; seam-guard 97 files plus the tree checks.
- `agentic validate` reports 0 errors on all 14 valid fixtures and templates.

## Auto Run Result

**Status:** built (2026-11-23). One story, one squash commit (owner condition 5); not split, no overrun beyond the triaged review rows.

**Summary:** every step of a workflow runs in order through AD-8's state machine, each attempt a bounded agent loop.
- **Hand-offs:** `next` passes the previous step's output; `loop_back: {to, reasons, max}` (default max 2, ceiling 5) re-queues the target and every step after it as new attempt rows, with the reasons journaled as the target's `loop_back` input in the same transaction; `gate: review` waits `waiting_human` with one Review item and no model call (`queued → running → waiting_human` in one transaction).
- **Caps:** `limits: {iterations, tool_calls, wall_clock}` per step, defaults 8, 20 and 10 min (owner: placeholders, configurable, never raisable by an agent; the config writer refuses `Seat` and `Assistant` actors on every path). Each cap, and passing a loop-back `max`, fails the step with one Failure item in one transaction. Counts come from committed rows.
- **Inbox:** one `inbox_items` entity with AD-28's seven kinds; created only by `System` through the writer, insert-only, `UNIQUE (step_run_id, kind)`; no command, route, CLI, tool or hand-off resolves one or leaves `waiting_human` (3.4 builds resolution). `agentic run` prints the waiting item and exits 0; `agentic status` shows attempts and open items.
- **Taint (AD-10, amended 2026-11-23):** `step_runs.taint` and `inbox_items.taint` are raise-only (writer and triggers); an attempt's taint commits before the call that carries it; a clean producer's output or reasons reach the next prompt as a `TrustedInput` part, a tainted one's as fenced `UntrustedData` that taints the receiver; A → B → C carries taint through every step and attempt. `TrustedInput` has private fields and two named constructors only am-engine calls (`check-deps`); `Instruction::authored` has an enumerated call-site list. The spike's resume path no longer wraps outputs as `Instruction`.
- **Tools (owner condition 2):** the binary has no tool. `run_plan_with_tools` (test seam, seam-guard) builds a ToolGate that alone runs a `Permit`ted `Read`-class call; `FixtureTool` lives in am-testkit.
- **Journal:** migration 0007 rebuilds `step_runs` (attempt, taint) and adds `step_inputs`, `tool_calls` and `inbox_items`, behind AD-7's pre-migration copy.
- **Static guards hardened** (review): the xtask source stripper handles every Rust 2024 literal prefix and a shebang and fails closed on an unterminated literal; scans cover every `.rs` of a crate (build.rs included), refuse symlinks, `#[path`, `include!` and `macro_rules!` in shipped code, and manifest target paths outside the scanned set.

**Tests:** about 120 new tests (engine `handoff_*`, trust `taint_*`, `loop_caps`, `toolgate`, `inbox`, `hand_offs`, `writer_actor`, xtask guards, doctests), all written failing first with red runs logged. `just check`: 1174 passed, 8 skipped.

**Review:**

| Pass | Findings | Patched | Deferred | Accepted |
|---|---|---|---|---|
| First (quick and adversarial security lenses) | 22: high 0, medium 5, low 17 | 16 | 6 | 1 |
| Follow-up (security, on round 1's patches) | 11: high 0, medium 3, low 8 | 9 | 2 | — |

- **Deferred:** D1 and D6 journal-derived counts and a journal-row proof for `TrustedInput` (3.9); D2 one reader per run (3.9); D3 caps checked inside the transaction (3.9, 3.14); D4 `StartStep` as a second start path (3.18); D5 a per-request random value in the untrusted fence and trusted header (4.9; the fence format is AD-27's); D7 a full `mod`-tree walk for the scans (3.12); D8 raw provider error text in the terminal (3.8).
- **Coordinator decisions, told to the owner:** a gate pauses through `running` (the plan's `queued → waiting_human` edge was not in AD-8); am-engine gains normal edges on the pinned `serde_json` and `uuid` (no new crate); one test fixture corrected (an "unterminated" literal that was terminated).

**Accepted interim risk (owner, 2026-11-23, condition 4, option (a)):** until epic 4 story 17 ships `is_widening`, a hand edit can weaken a gate unchecked: removing `gate:` from a step, or moving a gate step to a seat that has a model, lets the next new run pass straight through with no confirmation. A waiting gate is never released by such an edit (`editing_config_never_releases_a_waiting_gate`). 4.17 and 4.18 carry the cases and "must close the interim gap left by 3.3"; AD-15 and AD-42 record it.
