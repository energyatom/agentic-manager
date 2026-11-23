---
title: 'Steps, hand-offs and Inbox items'
type: feature
ticket: 3
created: '2026-11-23'
status: in-progress
baseline_revision: '228202e20f5633f76c37983e871beb4e463d068f'
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
  - The allowed edges: `queued → running`; `running → succeeded | failed | looped_back | waiting_human`; `queued → waiting_human` (a gate step starts by waiting).
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
- **No duplicate items on resume:** `UNIQUE (step_run_id, kind)` and insert-or-no-change mean a resumed engine (3.9) or a re-entered gate cannot create a second item for the same attempt (row 12). A new attempt of a gate step, after a loop-back from a later step, is a new step run and gets its own item, as it should.
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
