---
title: 'CLI Owner gate'
type: feature
ticket: 4
created: '2026-11-24'
status: approved
baseline_revision: '30d1f4629876f9ce98a81c35d8827442bfe523dd'
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

**Problem:** Story 3.3 pauses a run at a gate with a Review item, and stops there. Nothing moves it on:
- no command resolves an Inbox item (3.3 guards this absence with a `compile_fail` doctest on `Writer::exec` naming `WriteCommand::ResolveInboxItem`);
- `inbox_items.status` has a CHECK of `'open'` only, so a resolved item cannot even be stored;
- AD-8's `waiting_human → running` edge ("Human resolves") is not built, so no step ever leaves `waiting_human`;
- there is no `agentic gate`, no Inbox route on 3.2's server and no `max_returns`;
- the engine has no way to continue a run it left waiting.

The ticket's verify: at a gate, `agentic gate` on a TTY lists the Review; approve resumes the run; return with a note reruns the producing step with the note in its prompt and fails it past `max_returns` with a Failure item; reject ends the run; each resolution's audit entry reads "You (Reviewer)"; the same resolve over `/api/v1` works; piping input to `agentic gate` is refused.

**Approach:**
- **One Human-only command (AD-15, AD-28):** `Writer::resolve_inbox_item(by: HumanActor, item, decision, note)`. Its actor is Human by type, so a non-Human call does not compile; the writer also refuses a non-Human actor. It records the decision and takes AD-8's one Human edge, `waiting_human → running`, in one transaction, audited "You (Reviewer)".
- **The engine applies the decision (AD-8, AD-28, AD-36):** as `System(engine)`, in its own transaction, from the committed resolution: approve finishes the gate; return loops back to the producing step with the note, counted against `max_returns` (default 2); past it, the gate fails with one Failure item; reject fails the gate and the run. The writer refuses any gate transition that does not match the decision.
- **The note (AD-10 (b), AD-43):** stored once, on the item. The producer's new attempt gets a `gate_note` input that refers to it, and the engine builds a `TrustedInput` from it through `from_owner_note` with the committed Human resolver. It never clears taint.
- **`agentic gate` (AD-15 CLI class):** an interactive terminal only (stdin and stdout), no stdio MCP server or `render: true` Source enabled, the project opened under its lock. It lists open items, reads each decision from the terminal, resolves, then continues the run in the foreground (Question 1).
- **`/api/v1` (AD-14, AD-15):** Inbox list and read routes (CLI class) and a resolve route (UI class, Question 4; an owner-approved deviation from the ticket's "CLI class" wording, 2026-11-24) on 3.2's server, calling the same command; each Inbox change becomes one `inbox.updated` event carrying only the item's id and status (owner condition 4).
- **Migration 0008 (AD-7):** rebuilds `inbox_items` with resolved states and resolution fields, behind the pre-migration copy.

## Boundaries & Constraints

**Always:**
- **One command, Human by type:**
  - `Writer::resolve_inbox_item` takes `am_domain::HumanActor` (private fields, built only for a person and a hat; `From<HumanActor> for Actor` one way; no `From<Actor>`, no `Default`, no `Deserialize`).
  - No `WriteCommand` variant resolves an item, so `Writer::exec` cannot resolve with any actor.
  - Inside the writer the resolve is refused, with nothing written, unless the actor is `Actor::Human` (defence in depth; a src unit test reaches it).
  - In shipped code, `resolve_inbox_item` and `HumanActor::new` are named only in `crates/am-server/src/inbox.rs` and `crates/am-app/src/gate.rs`, plus their definitions (`cargo xtask check-deps` call-site rule, as 3.3's for `TrustedInput`). am-engine never names them (AD-15: no engine code path resolves a gate).
- **What the resolve commits (one transaction):**
  - the item: `status = 'resolved'`, `decision`, `note` (return and answer only), `resolved_by` (the actor as JSON), `resolved_at`;
  - for a Review or a Question: the item's step attempt `waiting_human → running` (AD-8's "Human resolves"), and nothing else;
  - one audit entry `inbox.resolved` (AD-25): target the item id; args kind, decision, step run id, taint and the note's sha256; never the note text.
- **What the engine applies (`System(engine)`, its own transaction, decided from committed rows):**
  - approve: the gate attempt `running → succeeded`;
  - return, count within `max_returns`: the gate attempt `running → looped_back`; attempt n+1 of the producing step and of the gate queued; the producer's new attempt's `gate_note` input journaled;
  - return past `max_returns`: the gate attempt `running → failed`, one Failure item, the run `failed`;
  - reject: the gate attempt `running → failed`, the run `failed` (Question 7).
  - The writer re-checks inside the transaction: the item is resolved, its decision matches the transition, and the return count from the journal is within or past the maximum as the engine said. Any other transition of a gate attempt that has a resolved Review (`FinishStep`, `SettleAndLoopBack`, `FailWithItem` against the decision) is refused.
- **Return counting (AD-36):** per gate step, per run: the number of this run's Review items on attempts of that step resolved `return`. Read from the journal, never from memory. `max_returns` is recorded on the Review item when the engine pauses, so the count and its maximum both come from the journal.
- **`agentic gate` checks, in this order (the first that fails is what the owner reads):**
  1. usage (exit 2);
  2. stdin and stdout are both an interactive terminal (the `Terminal` port; `crate::terminal::interactive` in the binary): else refused in words, nothing read, the project not opened;
  3. no stdio MCP server and no Source with `render: true` is enabled in the project's files (`am_config::owner_children`; a file with an error fails closed): else refused, naming it and the app's Inbox (`agentic open --item <id>` arrives with the UI);
  4. the project opens as `agentic run` opens it (lock, migrations, marker): refused in words while `agentic serve` holds it (Question 6);
  5. for approve and return only, before anything is written: `agentic run`'s checks 5 to 9 on the snapshot the run pinned (no outside change, no pending config migration, no config error, no owner child in the pinned snapshot, the current snapshot's hash equal to the run's `config_hash`, a plan and providers for the steps still to run) (Question 2).
- **Decisions are typed:** every decision is read from the terminal; reject is confirmed by typing `reject`; a return asks for its note; Ctrl-C, end of input or a 10-minute idle timeout writes nothing for that item.
- **A tainted Review (owner condition 2):** `agentic gate` says, above the payload and again at the choice, that the output came from untrusted content. Approving it needs the item's full id typed back; anything else (`a` alone, an empty line, a prefix, another id) writes nothing. Return and reject of a tainted Review need no id. The API holds the same rule: a tainted approve without `confirmItemId` equal to the item id is refused 422 `confirm-required` (coordinator's reading of "approving a tainted item requires typing the item's id", so the API is not a way around it; the Inbox UI that types it is 6.7's).
- **API:** `GET /api/v1/inbox` and `GET /api/v1/inbox/{id}` are CLI class (reads); `POST /api/v1/inbox/{id}/resolve` is UI class (session cookie, CSRF value, matching Origin; Question 4, an owner-approved deviation from the ticket's "CLI class" wording, owner 2026-11-24). A GET to the resolve route is 405. Every refusal is AD-14's envelope; no CORS header.
- **Events (owner condition 4):** `inbox.updated` carries only `{id, status}`: never the payload, title, body, decision, note or run.
- **Taint (AD-10):**
  - Resolving never changes an item's or a step's taint (both raise-only, writer and triggers).
  - A return note is trusted only through `TrustedInput::from_owner_note` with the resolver read from `resolved_by` as committed. A resolver that is not `Actor::Human` yields no trusted input, and the attempt does not start (fail closed).
  - The producer's new attempt is tainted by its own inputs, as 3.3 computes it; the note neither adds nor clears taint.
  - The step after an approved gate receives the reviewed payload with its producer's committed taint (a tainted producer arrives fenced and taints the receiver).
- **Layering (AD-4):** am-engine depends only on am-domain and am-store; am-app maps config to the plan. am-server gains a normal edge on am-engine (allowed by the AD-4 matrix) to apply a resolution after an API resolve. No new external crate.
- **Tests first:** every test in the Tests-first list is written and seen red, with its reason logged, before any `src/` change. FakeLlm, FakeClock and FakeTerminal from am-testkit; no network, no real model.

**Never:**
- **No other resolver:** no model output, tool, ToolGate path, hand-off, loop-back, AI review gate, Assistant, event, WebSocket frame, System command or CLI token alone resolves an item or moves a step out of `waiting_human`.
- **No auto-approve (AD-15):** no flag, env var, default answer, `--yes` or "always allow". An empty line is never approve.
- **No laundering:** the note text is stored once (`inbox_items.note`) and never copied into `step_inputs`, an output, the audit log or an event. No conversion between `UntrustedData`, `TrustedInput` and `Instruction` is added. No command lowers taint.
- **No other kinds:** Approval, Draft, BudgetAsk and Proposal items are refused in words until their stories (4.8, 3.6, 4.3, 4.10).
- **Outside this story:** crash resume, a step left `running`, unsettled calls and runs left ready by an API resolve (3.9); `waiting_human → cancelled` and `agentic stop` (3.11); what the engine does with a Question's answer (3.10, 3.14); review gates by a seat and `waiting_review` (3.16); lineage reads (3.17); the Inbox UI and "Edit, then approve" (6.7); `agentic open` (UI epics); ts-rs generation (UI epics, as am-api's doc says); paging of reads (3.2 D2).
- **No HTTP client in am-app** and no new network path (AGENTS.md: all network through am-net).
- **No department vocabulary in `crates/`.** The hat word "Reviewer" is AD-24's product word, defined once in am-domain; the fixture's nouns stay in `fixtures/`.
- **No change to AD-13's three same-user acceptances.**

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | List | `hand-offs` fixture, `gated` run waiting at `approve`; `agentic gate` on an interactive FakeTerminal | Shows the Review: item id, run, workflow, step `approve` attempt 1, the engine words, returns so far `0 of 2`, and step `draft`'s output, control characters neutralised; nothing written until a choice | — |
| 2 | Approve resumes | Row 1, owner types `a` | Item resolved `approve` by You (Reviewer); gate `waiting_human → running → succeeded`; `publish` runs on FakeLlm; run `succeeded`; prints `run <id> succeeded`; exit 0 | — |
| 3 | Return with a note | Row 1, owner types `r`, then `tighten the summary` | Item resolved `return`, note stored on the item; gate attempt 1 `looped_back`; attempts 2 of `draft` and `approve` queued; `draft` attempt 2 has one `gate_note` input (from the item, label `gate-note:approve@1`, taint 0); its request holds the note as `Part::Trusted` after the instruction, outside the fence; the gate waits again with a new Review item, shown in the same session | — |
| 4 | Past `max_returns` | `max_returns: 2`; three returns | Returns 1 and 2 rerun `draft`; the third: gate attempt 3 `failed`, one Failure item (engine words: step, `max_returns` 2, the Review item id; never the note), run `failed`; exit 1 | Failure item |
| 5 | `max_returns: 0` | One return | The first return fails the gate with one Failure item | Failure item |
| 6 | Warning before the last return | A return past the maximum | The CLI says the run will stop with a Failure item and asks to confirm; anything but `y` writes nothing | — |
| 7 | Reject ends the run | Owner types `j`, then `reject` | Item resolved `reject`; gate `failed`; run `failed`, reason "rejected at step `approve` by You (Reviewer)"; `publish` stays queued; no Failure item (Question 7); exit 0 | — |
| 8 | Reject unconfirmed | `j`, then anything but `reject` | Nothing written; item open | — |
| 9 | Skip | `s` | Nothing written; item open; exit 0 | — |
| 10 | Audit "You (Reviewer)" | Any resolution | One `inbox.resolved` entry, actor `{"kind":"human","person":"owner","hat":"Reviewer"}` (`describe()` = "You (Reviewer)"), args kind, decision, step run, taint, note sha256; the follow-up entries are `System (engine)`; the chain verifies | — |
| 11 | Same resolve over `/api/v1` | 3.2 rig with a project; run waits at the gate | `POST /api/v1/inbox/{id}/resolve {"decision":"approve"}` with session, CSRF and Origin: 200, the item resolved by You (Reviewer), the gate `succeeded` (applied by the server as System), `publish` queued; `continue_run` then finishes the run. Return and reject alike | — |
| 12 | API reads | CLI token (no Origin) or session | `GET /api/v1/inbox` lists open items; `GET /api/v1/inbox/{id}` adds the payload text and taint; unknown id 404 `not-found`; no project `no-project` | Envelope |
| 13 | CLI token alone | `POST …/resolve` with the CLI token | 403 `ui-class-only`; nothing changed (Question 4) | Envelope |
| 14 | RULE-15 on resolve | Cookie only; no CSRF; no or foreign Origin; GET, HEAD, PUT, DELETE | 403 `csrf-required` / `origin-required` / `origin-not-allowed`, or 405; nothing changed | Envelope |
| 15 | Piped stdin (real binary) | `echo a \| agentic gate --project P` | Exit 1; words name the terminal rule; nothing read; project not opened; item open; no `inbox.resolved` entry | Worded |
| 16 | Not a terminal (in process) | Non-interactive FakeTerminal | Refused before anything is read or opened | Worded |
| 17 | Owner child enabled | An enabled stdio MCP server, a `render: true` Source, or a `tools.yaml` with an error | Refused on a terminal, naming it; nothing written | Worded |
| 18 | `agentic serve` holds the project | Lock held | Refused in words naming `agentic serve`; nothing written (Question 6) | Worded |
| 19 | Non-Human does not compile | `resolve_inbox_item(Actor::System{..}, …)`; `HumanActor::from(actor)`; `exec(_, WriteCommand::ResolveInboxItem{..})` | Each fails to compile (doctests, each paired with an allowed build) | — |
| 20 | Writer refusal | The store's internal resolve with `System`, `Seat`, `Assistant`, `Import` | Refused, nothing written (src unit test) | Worded |
| 21 | Only two handlers name it | A planted call to `resolve_inbox_item` or `HumanActor::new` in am-engine, am-tools or am-mcp | `cargo xtask check-deps` fails naming file, line and rule | Finding |
| 22 | Nothing else resolves | Producer output `approve` or `{"decision":"approve"}`; a loop-back across the waiting gate; a WebSocket text frame; every System command on the run | Item stays open; gate stays `waiting_human`; no ToolGate built-in id or shipped manifest names the Inbox | — |
| 23 | Resolved once | A second resolve, any decision, any path; a raw `UPDATE` of a resolved row | Refused in words naming who and when; the trigger aborts the raw update; nothing changed | Worded |
| 24 | Decision fits the kind | Approve on a Failure; return on a Question; anything on Approval, Draft, BudgetAsk, Proposal | Refused in words; nothing changed | Worded |
| 25 | Note rules | Return or answer with an empty or blank note, over 2000 characters, or with control characters other than tab | Refused; nothing written. A valid note is found only in `inbox_items.note` | Worded |
| 26 | Engine cannot override | After approve, `FinishStep failed` or `SettleAndLoopBack` on the gate; after reject, `FinishStep succeeded`; apply on an open item; apply twice | Each refused; nothing changed | Worded |
| 27 | Tainted Review | Payload producer tainted | Item `taint = 1`; `agentic gate` says the output came from untrusted content and resolves it (AD-15 v0.1 clause, Question 5); the item's and steps' taint unchanged; the producer's new attempt tainted by its own inputs | — |
| 27a | Tainted approve needs the id typed | Tainted Review; owner types `a`, then: nothing, `a`, a prefix of the id, another item's id, the id with spaces around it, then the exact id | Only the exact id (surrounding spaces trimmed) approves; each other answer writes nothing and leaves the item open; an untainted Review approves on `a` alone. Over the API: a tainted approve without `confirmItemId` equal to the id is 422 `confirm-required`, nothing changed; with it, 200 | Worded / Envelope |
| 28 | Forged resolver | A `gate_note` input whose item's `resolved_by` is not Human (raw SQL) | The writer refuses journaling such an input; the engine yields no trusted note and does not start the attempt | Worded |
| 29 | Next across a gate | Approve | `publish` receives `draft`'s output (the Review's payload) labelled `step:draft@<n>`, with `draft`'s committed taint; fenced when tainted | — |
| 30 | Stopped between resolve and apply | Resolve committed, process stops before the apply | Gate `running`, item resolved; `continue_run` applies it once; no duplicate attempt, input or item | — |
| 31 | Config changed while waiting | Current snapshot hash differs from the run's pin; or an outside change, a pending migration or a config error | Approve and return refused before writing, naming both hashes; item open; reject and dismiss still work | Worded |
| 32 | No provider (real binary) | `a` on a terminal | Refused before writing: "no model provider is wired …"; reject works | Worded |
| 33 | Run left ready | After an API approve | `agentic status` shows the run `running`, the gate `succeeded`, `publish` queued, "ready to continue (resume arrives with story 3.9)" (Question 1) | — |
| 34 | Dismiss a Failure | `d` on a Failure item | Item resolved `dismiss` by You (Reviewer); no step or run change (Question 3) | — |
| 35 | Answer a Question | A Question made by a System `PauseStep` in a test | Typed answer stored once; step `waiting_human → running`; `agentic gate` does not continue the run and says the step continues with its story (Question 3) | — |
| 36 | Interrupt or timeout | Ctrl-C at any prompt; 10 minutes idle | Nothing written for that item; exit 1; lock released | Worded |
| 37 | Migration 0008 | A schema-7 database with an open tainted Review, an open Failure, a `step_inputs` row | Pre-migration copy; rows kept (`open`, resolution fields NULL, `max_returns` 2 on the Review), taint kept, keys and foreign keys hold, triggers present, report 7 → 8, `store.migrated` audited; reopening applies nothing; a forged v7 row with resolution fields fails the step closed at version 7 | Migration error names the backup |
| 37a | No item lost, copy kept (owner condition 5) | A schema-7 database with items of every kind 3.3 could write, on two runs, tainted and clean | After the upgrade: the same number of items and the same ids, every 0007 column equal value for value; the pre-migration copy exists, opens at `user_version` 7 and holds the same items; it is still there after a reopen and after the failed-step case | — |
| 38 | Status | After resolutions | `agentic status` prints each item with its status and, when resolved, its decision and "You (Reviewer)"; never a note | — |
| 39 | Events | Items created and resolved under `agentic serve` | One `inbox.updated` per change whose data has exactly two keys, `id` and `status` (owner condition 4); never the payload, title, body, decision, note or run (canary strings absent from every frame); first look publishes nothing old | — |
| 40 | Two resolves at once | Two API resolves of one item | One 200, one 409 `item-resolved`; one `inbox.resolved` entry | Envelope |
| 41 | `agentic run` at a gate | 3.3 row 9 | Unchanged lines and exit 0, plus one line naming `agentic gate` | — |
| 42 | `max_returns` config | Without `gate:`; outside 0..=5; not a number | `agentic validate` finding with file, line and fix; `agentic run` starts nothing | Worded |
| 43 | No approve flag | `agentic gate --yes`, `--approve` | Usage, exit 2; nothing written | Worded |

</intent-contract>

## Code Map

- **`crates/am-domain/src`:**
  - `actor.rs` (`Actor` L13-28, `describe` L32): add `HumanActor { person, hat }` with private fields, `HumanActor::new(person, hat)`, `From<HumanActor> for Actor`, and the hat word `REVIEWER_HAT = "Reviewer"`.
  - `inbox.rs` (`InboxKind` L7, `StepStatus` L64): add `Decision { Approve, Return, Reject, Answer, Dismiss }` with `as_str`, `parse` and `allowed_for(InboxKind)` (Review: approve, return, reject; Question: answer; Failure: dismiss; other kinds: none yet).
  - `content.rs` (`TrustedInput::from_owner_note` L247): unchanged; 3.4 is its first caller.
  - `lib.rs` (exports L42, L53): export both.
- **`crates/am-store/src`:**
  - `lib.rs` `WriteCommand` (L191-379): no resolve variant. Add a System-only apply command (suggested `ApplyResolution { item_id, past_max: Option<NewInboxItem> }`; the name is the implementer's).
  - `Writer::exec` doctests (L641-649): 3.3's `ResolveInboxItem` `compile_fail` is replaced by the pairs in Design Notes. Add `Writer::resolve_inbox_item(&self, by: HumanActor, item_id, decision, note)`. The writer job (L603) carries the resolve through a crate-private command.
  - `execute` (L830; the System-only item check L863-873): the Human check for the resolve; the System check for the apply.
  - `require` (L1322), `run_open_for` (L1479), `no_step_waits` (L1509): the Human edge `waiting_human → running`, only through the resolve.
  - `INPUT_KINDS` (L1380), `journal_inputs` (L1386): kind `gate_note` with `from_item`.
  - `loop_back` (L1562): reused for the return (queue producer through gate).
  - `create_item` (L1638): `max_returns` on a Review.
  - `finish_step` (L1707), `finish_run` (L1690): the decision guard.
  - `NewInboxItem` (L570) gains `max_returns: Option<u32>`. `InboxItemRow` (L1827) gains `decision`, `note`, `max_returns`.
  - `Reader` (L1870): `inbox_item(id)`, `open_items()`; `inbox_items(run)` (L2006) reads the new columns.
  - `migrations.rs` (`MIGRATIONS` L35-43, `SCHEMA_VERSION` L46): add `0008_inbox_resolution.sql`. The migrator (foreign keys off, broken-reference count before and after, L228-252) is unchanged.
  - `testing::database_at_version` (L1747): used by the migration tests.
- **`crates/am-store/migrations/0008_inbox_resolution.sql`** (new): see Design Notes, "The Inbox table migration".
- **`crates/am-engine/src`:**
  - `run.rs` `HandOff::Gate` (L98-106) becomes `Gate { max_returns: u32 }`; `pause_at_gate` (L1249) records it on the Review item.
  - `RunError` (L182): add `ConfigChanged { pinned, now }` and `NotContinuable(String)`.
  - Add `continue_run(store, run_id, plan, cancel) -> Result<RunOutcome, RunError>` beside `run_plan` (L310); it enters `drive` (L620).
  - `drive` (L620-660): a gate attempt `running` with a resolved Review is applied first; any other `running` step stays refused ("resume arrives with story 3.9").
  - `inputs_of` (L674): the `gate_note` input (read item, note, resolver; `from_owner_note`); `next` across a succeeded gate takes the gate's Review payload.
  - New `gate.rs`: `apply_resolution(writer, reader, item_id)`, the engine's words for the past-max Failure item and the reject reason.
- **`crates/am-config/src`:** `entities.rs` `Step` (L319; `gate` L342, `Gate` L365) gains `max_returns: u32` (default 2); `kinds.rs` `step_gate` (L2176) and the allowed step keys parse it; `checks.rs`: only with `gate:`, range 0..=5.
- **`crates/am-api/src/lib.rs`:** `InboxItemDto`, `InboxItemDetailDto` (payload text and taint, note), `ResolveBody { decision, note, confirm_item_id }`, `ResolutionDto { item, run }`, `InboxUpdatedDto { id, status }` and `EVENT_INBOX_UPDATED` beside `EventDto` (L82-96).
- **`crates/am-server/src`:**
  - `routes.rs` `shipped()` (L118-129): the three routes. `RouteClass` (L31) unchanged.
  - New `inbox.rs`: the handlers (list, one, resolve then `am_engine::gate::apply_resolution`).
  - `state.rs` `AppStateBuilder` (L66; `project_db` L89): add `project_writer(Writer)`.
  - `events.rs` `look` (L148), `watch_runs` (L192): items, `inbox.updated`.
  - `Cargo.toml` (L13-24): am-engine.
- **`crates/am-app/src`:**
  - New `gate.rs`: `agentic gate [<item id>] [--project <folder>]`, `GateContext { places, identity, terminal, providers }` (as `RunContext`, `run.rs` L101), re-exported in `cli.rs` (L23).
  - `run.rs`: `plan_of` (L275; gate at L338) maps `max_returns`. Checks 5 to 9 (`no_outside_change` L246, `snapshot_of` L382, `no_pinned_owner_child` L233) are shared with `gate.rs`. `report` (L497) gains the `agentic gate` hint. `status`, `item_line` (L654) and `read_runs` show resolved items.
  - `main.rs`: dispatch (L64-116) `Some("gate")` with `owner_side()` (L125), so the binary still builds one terminal; usage (L112); `serve` (L313-315) passes `project_writer(opened.store.writer())`.
  - `terminal.rs` `interactive` (L27): the one predicate, unchanged.
- **`xtask/src/deps.rs`** (`TRUSTED_CONSTRUCTORS` L28, `INSTRUCTION_SITES` L42): a third call-site rule for `resolve_inbox_item` and `HumanActor::new`.
- **Fixture `fixtures/projects/hand-offs/`:** `gated.yaml` keeps `gate: review` (default `max_returns` 2); new `returned.yaml` with `max_returns: 1` for the past-max rows. Tests write `max_returns: 0` into a temporary copy.
- **Existing tests that must change (supersession, as in 3.3):**
  - `crates/am-store/tests/run_journal.rs` L528 `assert_eq!(SCHEMA_VERSION, BEFORE_ATTEMPTS + 1, "0007 is this story's")`: becomes "the 6 → 7 step is applied" (0008 follows it).
  - `crates/am-server/tests/route_table.rs` `expected()` (L33-44): gains the three routes.
  - `crates/am-e2e/tests/engine/support/mod.rs` L108: `HandOff::Gate` becomes `HandOff::Gate { max_returns: 2 }`.
  - am-store's `Writer::exec` doctest naming `ResolveInboxItem` (src): replaced.

## Tasks & Acceptance

**Execution:**
- [ ] **Tests first (no `src/` change):** every file in the Tests-first list, the fixture change, the `[[test]]` targets and the superseded literals. Log the red run with each test's failure.
- [ ] **am-domain:** `HumanActor`, `Decision`, the hat word.
- [ ] **am-store:** migration 0008, `resolve_inbox_item`, the apply command, the decision guard, `gate_note`, the readers, the doctests and the src unit test.
- [ ] **am-config:** `max_returns` and its checks.
- [ ] **am-engine:** `HandOff::Gate { max_returns }`, `continue_run`, `gate::apply_resolution`, the drive and input changes.
- [ ] **am-api and am-server:** DTOs, routes, handlers, writer handle, `inbox.updated`.
- [ ] **am-app:** `agentic gate`, serve wiring, `status` and `run` lines.
- [ ] **xtask:** the call-site rule.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then rows, items, inputs, requests, audit entries, events and CLI or API output match the row.
- Given the ticket's verify, when the `gated` fixture runs on FakeLlm: `agentic gate` on a TTY lists the Review; approve resumes the run; return with a note reruns `draft` with the note as a trusted part of its prompt; returning past `max_returns` fails with one Failure item; reject ends the run; each `inbox.resolved` entry reads "You (Reviewer)"; the same resolve over `/api/v1` works; and piping input to the real `agentic gate` is refused.
- Given the workspace, when `just check` runs, then it is green, `rule_15_*` and 3.3's `handoff_*` and `taint_*` suites included.

## Plan Change Log

- 2026-11-24, owner approval with five conditions (Design Notes, "Owner answers"): rows 27a and 37a added, row 39 narrowed to `{id, status}`, the API's `confirmItemId` and `confirm-required`, five tests added (`migration_0008_loses_no_item_and_keeps_the_pre_migration_copy`, `an_inbox_event_carries_only_the_id_and_status`, `a_tainted_approve_over_the_api_needs_the_item_id`, `a_tainted_review_says_its_output_came_from_untrusted_content`, `approving_a_tainted_review_needs_its_id_typed`).
- 2026-11-24, owner condition 3, the architect's check (read-only): **FITS AS WRITTEN**; no AD wording change. A Human return note at a gate is AD-10 (b)'s own example; the note stays trusted on a tainted Review, because AD-10's fencing clauses bind text a tainted step authors, and a Human is not a step; taint is not cleared, since approve carries the payload's taint and return passes on only the note. No conflict with AD-27 (trusted parts after the instruction, before the fence), AD-42 ("the note as a trusted input") or AD-43 (one `step_inputs` edge, nothing denormalised). AD-10's "no row ... yields one" means table rows, not the journaled `inbox_items` row. Corrections applied: the trust test file is `taint_trusted_input_gate_note.rs`; "The note alone" says how 3.5 must pass an earlier output.

## Design Notes

### Human-only resolve (owner ask 1)

**The one command.** `Writer::resolve_inbox_item(by: HumanActor, item_id, decision: Decision, note: Option<String>)`.
- **By type:** the parameter is `HumanActor`, not `Actor`. A `System`, `Seat`, `Assistant` or `Import` value does not compile there. There is no `From<Actor>` for `HumanActor`, and no `WriteCommand` variant resolves, so `Writer::exec` cannot resolve either.
- **Doctests replacing 3.3's** (on `Writer::resolve_inbox_item`; each `compile_fail` paired with an allowed build, as 3.3 did, so a missing type cannot make a refusal pass for the wrong reason):
  - allowed (`no_run`): `w.resolve_inbox_item(am_domain::HumanActor::new("owner", "Reviewer"), "i", am_domain::Decision::Approve, None)`;
  - `compile_fail`: the same call with `am_domain::Actor::System { component: "engine".into() }`;
  - `compile_fail`: `let h: am_domain::HumanActor = actor.into();` from an `Actor`;
  - `compile_fail` (kept from 3.3, its meaning restated): `w.exec(owner, am_store::WriteCommand::ResolveInboxItem { item_id: "i".into() })`. No `WriteCommand` resolves; only the typed method does.
- **Writer refusal:** the job crosses the writer channel as an `Actor`. `execute` refuses the resolve unless it is `Actor::Human`, with nothing written. A src unit test sends the crate-private command with each non-Human actor.
- **Call sites:** a `check-deps` rule allows `resolve_inbox_item` and `HumanActor::new` in shipped code only in `crates/am-server/src/inbox.rs` and `crates/am-app/src/gate.rs` (and the two definition lines). This is AD-15's "only API or CLI command handlers carrying `Actor::Human`" made checkable. It also means a later AI review gate (3.16) or the Assistant cannot reach it without failing the build.

**What each kind can do in this story** (AD-15's CLI class, AD-28; Question 3):

| Kind | Decisions here | Step change by the resolve | Then |
|---|---|---|---|
| Review (gate) | approve; return (note required); reject | `waiting_human → running` | The engine applies it (below) |
| Question | answer (text required) | `waiting_human → running` | Its creating story (3.10, 3.14) decides what the engine does |
| Failure | dismiss | none (its run already failed) | Nothing |
| Approval, Draft, BudgetAsk, Proposal | none | — | Refused in words: "arrives with story 4.8 / 3.6 / 4.3 / 4.10" |

**The two paths reach the same command:**
- **`agentic gate`:** checks 1 to 4 of Boundaries, then lists open items by kind urgency (Review, Question, Failure), then age. For each it shows the engine words and, for a Review, the payload (the producing attempt's output) and the returns so far. It reads one choice from the terminal: `a` approve, `r` return (then the note), `j` reject (then `reject` to confirm), `d` dismiss, `s` skip; `answer` for a Question. For approve and return it runs check 5 first. It records the decision with `HumanActor::new("owner", REVIEWER_HAT)`, asking `terminal.interrupted()` just before the write, as `agentic accounts` does. Then it continues the run (Question 1). An item the continuation raises (the gate again after a return) is shown in the same session.
- **`POST /api/v1/inbox/{id}/resolve`:** UI class (Question 4). The handler calls the same command with the same `HumanActor`, then `am_engine::gate::apply_resolution` as `System(engine)`. It never runs a model step. The response is the resolved item and the run's status.

**What cannot resolve an item, and why:**
- **An agent session or a tool:** the binary registers no tool; no ToolGate built-in id or shipped manifest names the Inbox or the local API (AD-15); a tool acts as `Actor::Seat`, which is not a `HumanActor`; am-tools and am-mcp may not name the command (`check-deps`).
- **The CLI token alone:** the resolve route refuses it (`ui-class-only`, Question 4). `agentic gate` never uses the token: it resolves under the project lock, on a terminal.
- **A hand-off or loop-back:** they only create attempts and inputs; 3.3's writer already refuses a loop-back across a waiting step; no hand-off path names the command.
- **An AI review gate (3.16):** am-engine cannot name the command (`check-deps`); AD-42: an AI gate never resolves an Approval, BudgetAsk or Proposal, and its Review verdicts are review step runs, not Inbox resolutions.
- **An event:** the WebSocket stream ignores client frames (`events.rs` L130-132); events only describe changes.
- **The engine:** the apply command needs an item already resolved by a Human, and the writer refuses it on an open item. No System command changes an item's `status`.
- **Model output:** a gate makes no model call; no output is read as a decision (3.3 row 11, kept and extended through `continue_run`).

**"You (Reviewer)":** every resolution is made as `Actor::Human { person: "owner", hat: "Reviewer" }`. Its `inbox.resolved` entry stores that actor; `Actor::describe()` renders "You (Reviewer)" (AD-24), and `agentic status`, `agentic gate` and the API's `resolvedBy` use the same words. The engine's follow-up entries (`step.finished`, `step.looped_back`, `inbox.created`, `run.finished`) are `System (engine)`: the Human decided, the engine applied (AD-15).

**A tainted Review (Question 5):**
- 3.3's Design Notes said that from the first build that serves the Inbox UI, resolving a Review whose payload is tainted needs the UI class (AD-15, amended 2026-11-19).
- This story ships before any Inbox UI. AD-15's v0.1 clause lets the CLI resolve it, because no owner-uid hostile-input child exists yet. Epic 6 story 7 owns the switch: its verify has `agentic gate` print the open-item hint and resolve nothing once it lands.
- So in this story: `agentic gate` resolves a tainted Review. Above the payload it prints "This output came from untrusted content: the step that wrote it read outside content." At the choice it says so again, and approve asks "Type the item id <id> to approve output from untrusted content:"; only the exact id approves (owner condition 2, row 27a). The predicate is one function in `crates/am-app/src/gate.rs`, which 6.7 turns into a refusal. The shipped binary has no taint source yet; tests make one, as in 3.3.
- The API's resolve route applies the same rule with `confirmItemId` (Boundaries); the Inbox UI that asks for it is 6.7's.
- Resolving never changes the item's taint. A return note on a tainted Review is still owner text, so trusted (AD-10 (b)).

### The return note (owner ask 2)

- **Trusted, from a Human only (AD-10 (b)):** the note is owner-authored text for one run's work, so it is a `TrustedInput`, not an `Instruction`. When the producer's new attempt starts, the engine reads the item's `note` and `resolved_by` from the journal and calls `TrustedInput::from_owner_note(label, note, &resolver)`. That returns `None` unless the resolver is `Actor::Human`; then the attempt does not start, and the run stops in words (row 28).
- **In the prompt (AD-27):** its own `Part::Trusted`, labelled `gate-note:<gate key>@<gate attempt>`, in the user role after the step instruction, after the attempt's other trusted inputs, before the fence. am-providers renders it through 3.3's one shared function. Story 3.5 restates the order inside full assembly.
- **The note alone:** as with 3.3's loop-back reasons, the producer receives its usual inputs plus the note. It does not receive its own earlier output (AD-36 names only the note). Story 3.5 may restate this; any earlier output it gives attempt n+1 goes through `TrustedInput::from_step_output` with attempt n's committed taint (architect, 2026-11-24).
- **Taint never clears:**
  - the note's `step_inputs` row has taint 0 and never lowers the attempt's taint;
  - the producer's new attempt is tainted when its own inputs are (its `next` input, plan inputs), exactly as 3.3 computes it;
  - the earlier attempts, the gate attempts and both Review items keep their committed taint;
  - a tainted producer's output still reaches the gate's next Review tainted.
- **Counted against `max_returns` (AD-36):**
  - The count is per gate step, per run: Review items of this run on attempts of that step resolved `return`, read from the journal. `max_returns` (default 2, 0..=5) is written on each Review item at pause, from the run's plan.
  - With `max_returns: 2`, returns 1 and 2 loop back; the third fails the gate attempt with one Failure item. Its body names the step, `max_returns` and its value, and the Review item's id; never the note.
  - The run fails. The third note stays on its item and reaches no prompt.
  - `agentic gate` warns before a return that would pass the maximum (row 6).
- **Stored once (AD-43, AD-25):**
  - the text lives only in `inbox_items.note` of the Review item that carried the return;
  - the producer's new attempt has one `step_inputs` row: kind `gate_note`, `from_item` = that item, `from_step_run` NULL, the label, taint 0. It is written by the apply transaction (as 3.3's P9 writes a loop-back's input in the loop-back's own transaction), so the edge is stored once;
  - the audit entry holds the note's sha256, never its text; events never carry it; no output copies it;
  - lineage (3.17) walks `step_inputs.from_item` to the item, and the item to its payload.
- **The writer checks a `gate_note` input:** it needs `from_item` naming a Review item of the same run resolved `return` by an `Actor::Human`, no `from_step_run`, and taint 0. Anything else is refused.

### The Inbox table migration (owner ask 3)

**Why a rebuild.** 0007's `inbox_items` has `status CHECK (status IN ('open'))`, and SQLite cannot alter a CHECK. So 0008 rebuilds the table with SQLite's own procedure (create, copy, drop, rename), as 0007 did for `step_runs`.

**`0008_inbox_resolution.sql`, in order:**
1. `CREATE TABLE inbox_items_0008` with every 0007 column and constraint kept:
   - `id` primary key; `kind` CHECK (AD-28's seven); `run_id`, `step_run_id`, `payload` foreign keys to `runs` and `step_runs`; `taint` CHECK (0, 1); `title`, `body`, `created_at`; `UNIQUE (step_run_id, kind)`.
   - `status` CHECK becomes `IN ('open', 'resolved')`.
   - New: `max_returns INTEGER CHECK (max_returns BETWEEN 0 AND 5)` (a Review's), `decision TEXT`, `note TEXT`. `resolved_by` (the actor as JSON) and `resolved_at` are kept.
   - A table CHECK: `open` has `decision`, `note`, `resolved_by`, `resolved_at` all NULL; `resolved` has `decision`, `resolved_by`, `resolved_at` set.
   - `decision` has no spelling CHECK: the writer checks it against the kind, so stories 4.3, 4.8 and 4.10 add their decisions without another rebuild (as 3.3 chose for `step_inputs.kind`).
2. `INSERT INTO inbox_items_0008 … SELECT …` from `inbox_items`, every row and column copied as it is, with `decision` and `note` NULL and `max_returns` 2 on a Review (the default its gate had: 3.3 had no `max_returns` key), NULL otherwise. `resolved_by` and `resolved_at` are copied as they are, so a forged v7 row that has them fails the table CHECK and the step fails closed (row 37). No 3.3 command ever wrote them.
3. `DROP TABLE inbox_items; ALTER TABLE inbox_items_0008 RENAME TO inbox_items;`
4. Triggers. Dropping the table dropped 0007's trigger, so it is created again:
   - `inbox_items_taint_only_rises`, as in 0007;
   - new `inbox_items_resolved_never_change`: any `UPDATE` of a row whose old `status` is `resolved` aborts;
   - new `inbox_items_fixed_fields`: an `UPDATE OF` id, kind, run_id, step_run_id, payload, title, body, created_at or max_returns aborts.
5. `CREATE INDEX inbox_items_by_status ON inbox_items (status, created_at);`
6. `ALTER TABLE step_inputs ADD COLUMN from_item TEXT REFERENCES inbox_items(id);` (NULL default, so allowed), after the rename.

**Behind AD-7:**
- The migrator takes the `VACUUM INTO` pre-migration copy first, keeps the three newest, runs the step in its own transaction with foreign keys off, and compares broken references before and after (3.3's P10 rule). The step is reported (CLI line) and audited `store.migrated` from 7 to 8.
- **Data preserved:** every item keeps its id, kind, run, step run, payload, taint, words and time, and stays `open`. Nothing is dropped, rewritten or made up.
- **Idempotent:** `user_version` runs the step once; reopening a version-8 database applies nothing. A failed step rolls back whole: version 7, rows unchanged, and the error names the backup; a later open retries it.
- A binary that knows only version 7 refuses a version-8 database (AD-7, unchanged).

**What 3.3's migration-test pattern requires** (`run_journal.rs`):
- `am_store::testing::database_at_version(&path, 7)`, then raw rows: a run with a waiting gate and its open tainted Review, a failed step's open Failure, a `step_inputs` row;
- open through `open_project`; the report's 7 → 8 step has a backup file;
- the new columns and the index exist; every row is kept as above; `UNIQUE (step_run_id, kind)` and the foreign keys hold (`pragma_foreign_key_check` empty);
- the taint trigger still refuses a lower taint; the two new triggers refuse;
- the new commands work on the migrated database (resolve the old Review, apply it);
- `store.migrated` is audited and the chain verifies;
- reopening applies nothing; a forged row fails closed at version 7.
- 3.3's own 0007 test changes one line: it no longer pins 7 as the last version.

### State machine and resume (owner ask 4)

**Edges (AD-8), and who takes them:**

| Decision | Human resolve (one transaction) | Engine apply as `System(engine)` (one transaction) | Run |
|---|---|---|---|
| approve | item resolved; gate `waiting_human → running` | gate `running → succeeded` | stays `running`; the next step runs |
| return, within `max_returns` | same | gate `running → looped_back`; producer and gate attempt n+1 queued; `gate_note` input | stays `running`; the producer reruns |
| return, past `max_returns` | same | gate `running → failed`; one Failure item | `failed` |
| reject | same | gate `running → failed` | `failed` (Question 7) |
| answer (Question) | item resolved; step `waiting_human → running` | none in this story (3.10, 3.14) | unchanged |
| dismiss (Failure) | item resolved | none | unchanged (already failed) |

- AD-8 has no `waiting_human → succeeded`, `→ looped_back` or `→ failed` edge, so every decision passes through `running`. The Human takes only the edge AD-8 names for them ("Human resolves").
- AD-28 lets only the engine create items. So the past-max Failure item, and with it every follow-up transition, is the engine's.
- AD-15: the engine never resolves; it applies a decision a Human committed.
- `waiting_human → cancelled` (AD-8, Stop) stays 3.11's.

**Who continues the run (Question 1; assumed option (a)):**
- **`agentic gate`** continues the run of the item it resolved, in the foreground, holding the project lock, through `am_engine::run::continue_run`, then reports as `agentic run` does.
- **`continue_run`** accepts only a clean stop:
  - the run exists and is `running`;
  - the plan's `config_hash` equals the run's pin (else `RunError::ConfigChanged`);
  - no step is `running` except a gate whose Review is resolved and not yet applied;
  - no model call is unsettled and no tool call is `issued`.
  - It applies any pending resolution first, then enters 3.3's `drive` loop, which decides from the journal.
- **An API resolve** records the decision, and the server applies its journal step at once (so a reject ends the run). The server runs no model step: `agentic serve` wires no provider and no engine run. Such a run is left ready, and `agentic status` says "ready to continue (resume arrives with story 3.9)".
- **Exit codes:** 0 when every typed decision was recorded and each continued run succeeded, waits again or ended by the owner's reject; 1 on a refusal, an interrupt, a timeout, or a continued run that failed otherwise (past `max_returns` included); 2 for usage.

**If something stops between the resolve and the run going on:**
- **After the resolve, before the apply:** the gate is `running` and its item resolved. The next `continue_run` applies it once; the apply is decided from committed rows, and a second apply is refused, so nothing is duplicated (row 30). In this story only `agentic gate` calls `continue_run`, and only for the item it just resolved. So a run stopped there waits for 3.9's resume, as does a run left ready by an API resolve.
- **After the apply, before the next step starts:** the run is ready (nothing in flight). Same as above.
- **During a continued model step:** the step is `running` with a reserved call. That is 3.9's crash case (3.3 row 28), and `continue_run` refuses it in words.
- **The server stops after an API resolve:** the decision and its apply are committed; nothing is lost.

**What this story builds, and what 3.9 builds:**
- **3.4:** the Human edge; the System apply; `continue_run` for a clean stop, entered only by `agentic gate` for the run it just resolved.
- **3.9:** resume on start for every unfinished run: a step left `running`, unsettled calls (marked lost, re-issued), a resumed attempt's wall clock from its persisted `started_at`, runs left ready by an API resolve or a stopped `agentic gate`. It may reuse `continue_run`'s entry and the apply. Whether a run may continue on a changed snapshot is decided here for 3.4 (Question 2), and 3.9 inherits it unless the owner says otherwise.

### Owner answers (2026-11-24)

The owner accepted every assumed answer to Q1–Q9 below and the drafter's additions (the reviewed payload with its taint for the step after approve, `inbox.updated`, the 10-minute idle timeout, `max_returns` on each Review item, the am-server → am-engine edge), with these conditions:

1. **Q4:** the UI-only resolve route is recorded as an owner-approved deviation from the ticket's "CLI class" wording (epic Notes Decision; the ticket text is not edited).
2. **Q5:** the `agentic gate` prompt states that the output came from untrusted content, and approving a tainted item requires typing the item's id; tests added (row 27a). The coordinator applies the same id rule to the API's resolve route (`confirmItemId`), so the API is not a way around it.
3. **The return note:** Winston (the architect) confirms it fits AD-10's existing `TrustedInput` note before any code; if the AD needs new wording, the build stops for the owner. Winston's verdict is recorded in the Plan Change Log.
4. **`inbox.updated`** carries only the item id and status, never the payload; test added.
5. **Migration 0008:** a test proves an old database upgrades with no item lost and the pre-migration copy kept (row 37a).

Then: commit the plan, log the red run, build tests first on Opus; one story, one squash commit; merge only with both CI jobs green after review triage; stop and report on an overrun.

### Questions for the owner (stop: AD or ticket intent)

Each question is neutral. The plan's current assumption is marked, and it waits on the answer.

**Q1. Who continues a run after approve or return.**
- **Ticket:** "approve resumes the run"; "return … reruns the producing step".
- **Epic Notes assumption:** with no server, `agentic run` prompts the gate inline; with one, the CLI talks to it.
- **3.3 Decision 4 (accepted):** `agentic run` exits 0 at a gate.
- **3.9:** "on start, every unfinished run resumes at its last committed transition".
- **Options:**
  - (a) `agentic gate` continues the run of the item it resolved (`continue_run`, clean stops only). An API resolve records and applies the decision; its model steps wait for 3.9, and `agentic status` says so. **[assumed]**
  - (b) As (a), and `agentic gate` also offers to continue runs left ready (by an API resolve or a stopped `agentic gate`). That is a small part of resume ahead of 3.9.
  - (c) `agentic run` asks at the gate inline and continues in process (epic Notes); `agentic gate` handles items left from an earlier exit. This changes 3.3's Decision 4.
  - (d) Resolve only; 3.9 continues every resolved run. The verify's "approve resumes the run" moves to 3.9.

**Q2. A run continues only on the snapshot it pinned.**
- **AD-16:** a run pins its snapshot hash; only the hash is stored.
- **3.3 row 32:** editing a waiting gate's config never releases it.
- **Effect:** any config edit while a run waits blocks its approve.
- **Options:**
  - (a) Approve and return are refused, before anything is written, while the project's snapshot differs from the pin (plus `agentic run`'s outside-change, migration and error checks). Reject and dismiss still work. **[assumed]**
  - (b) Continue on the current config and re-pin the run (an AD-16 change).
  - (c) Store each run's snapshot in the database, so it continues on its own copy (an AD-16 and AD-35 change; larger).

**Q3. Which items this story resolves.**
- **Ticket:** a Review (approve, return, reject) and a Question (answer).
- **AD-15 CLI class:** also "Dismiss".
- **3.3's accepted Design Notes:** Failure, "Human only (acknowledge)", built by 3.4.
- **Nothing creates a Question until 3.10** (choices Split input / Use fallback) and 3.14.
- **Options:**
  - (a) Review: approve, return, reject. Question: answer (owner text stored once; the AD-8 edge; what the engine does with it comes with 3.10 and 3.14, so `agentic gate` does not continue such a run here). Failure: dismiss (no other change). The other four kinds are refused in words. **[assumed]**
  - (b) Review and Question only; Failure items stay open, listed read-only, until epic 6's "Step failed" fixes.
  - (c) Review only; Question answering moves to 3.10, its first creator (this changes the ticket).

**Q4. The class of `POST /api/v1/inbox/{id}/resolve`.**
- **Ticket:** "(CLI class, arch AD-15: CLI token plus interactive TTY)"; "a non-TTY call is refused".
- **AD-15:** CLI approvals require an interactive TTY; resolving a Review or Question is CLI class "on an interactive TTY".
- **AD-13:** the CLI token is a 0600 file any process of the owner's can read. The server cannot see a terminal.
- **Options:**
  - (a) UI class (session, CSRF, matching Origin); the CLI token gets `ui-class-only`. The list and read routes are CLI class. `agentic gate` resolves directly, under the project lock, on a terminal. Nothing without a terminal or a browser session resolves, so "the CLI token alone" resolves nothing. **[assumed]**
  - (b) CLI class: the token resolves over the API, and the server refuses it while a stdio MCP server or a `render: true` Source is enabled. The TTY rule is then the `agentic gate` binary's alone, so any process holding the token file resolves without a terminal. That residual is not one of AD-13's three accepted ones. This option is needed if Q6 picks (b).

**Q5. A tainted Review on the CLI.**
- **AD-15 (amended 2026-11-19):** from the first build that serves the Inbox UI, it needs the UI class; in v0.1 the CLI may resolve it.
- **Epic 6 story 7** owns the switch.
- **EXPERIENCE.md:** "answered in the app only; the CLI prints where to open it".
- **Options:**
  - (a) The CLI resolves it now, under an untrusted marker; one predicate in `gate.rs` becomes 6.7's refusal. **[assumed]**
  - (b) Refuse it on the CLI already now. With no Inbox UI yet, only a browser session over the API could resolve it.

**Q6. `agentic gate` while `agentic serve` holds the project.**
- **Epic Notes:** the CLI talks to a running server over `/api/v1` with the token.
- **3.1:** `agentic run` is refused while serve holds the project.
- **AGENTS.md:** all network goes through am-net; am-app has no HTTP client.
- **Options:**
  - (a) Refused in words naming `agentic serve`: stop it, then run `agentic gate` (as 3.1's `agentic run`). **[assumed]**
  - (b) Through `/api/v1` with the CLI token. This needs Q4 (b) and a loopback client for the app's own server: a new network path in am-net that AGENTS.md does not name.

**Q7. What reject does.**
- **Ticket:** "reject ends the run".
- **AD-8:** the gate attempt can go `running → failed`, or `waiting_human → cancelled` (Stop).
- **AD-42 (review gates by a seat):** reject fails the hand-off with a Failure item naming the reviewer and note.
- **EXPERIENCE.md:** "the run stops at that step".
- **Options:**
  - (a) Gate attempt `failed`, run `failed`, reason "rejected at step `<key>` by You (Reviewer)", no Failure item: the owner made the decision, and the Review item records it. **[assumed]**
  - (b) As (a), plus a Failure item, as AD-42 gives a seat reviewer.
  - (c) Gate and run `cancelled`, through the Stop edge.

**Q8. Which step fails past `max_returns`.**
- **Ticket:** "reruns the producing step … and fails it past max_returns". "It" may read as the producing step or the gate.
- **AD-36:** "Past either maximum, the step fails with a Failure Inbox item"; 3.3 fails the step whose `loop_back.max` was passed.
- **Options:**
  - (a) The gate step fails (its `max_returns` was passed); the run fails. **[assumed]**
  - (b) The producing step's last attempt is marked failed instead.

**Q9. Size: one commit?**
- **What it touches:** am-domain, am-store (a table rebuild and new commands), am-engine (continue, apply, inputs), am-config (`max_returns`), am-api, am-server (three routes, a writer handle, events), am-app (`agentic gate`, serve wiring, status), xtask (a call-site rule). About 90 tests. Comparable to 3.3, across more crates.
- **Options:**
  - (a) One story, one squash commit; on an overrun, stop and report, never split. **[assumed]**
  - (b) A Sofia REPLAN splitting it into "Human-only resolve, Inbox migration and `agentic gate`" and "Inbox routes and events on the server".

### Decisions for the owner's approval (within the ADs)

1. **Config:** a gate step takes `max_returns` (default 2, 0..=5), refused without `gate:`. Raising it raises a cap, which epic 4 story 17's `is_widening` must classify (recorded here because the function does not exist yet).
   ```yaml
   - id: approve
     seat: owner
     instruction: Approve or return the version.
     gate: review
     max_returns: 2
   ```
2. **Two transactions per decision:** the Human resolve, then the engine's apply (State machine, above).
3. **Gate facts are journaled at pause:** `inbox_items.max_returns`, so a resolve can be applied from the journal alone, by the server or a later resume.
4. **`next` across an approved gate** carries the gate's Review payload (the producing attempt's output, by reference, with its producer's taint). Without this, the step after a gate would receive nothing.
5. **`agentic gate` words:**
   - the payload is printed with control characters other than newline and tab neutralised (model text in a terminal);
   - a return note is one line of at most 2000 characters;
   - every prompt has a 10-minute idle timeout, so a forgotten terminal does not hold the project lock;
   - `agentic gate` reads the terminal like `agentic accounts`, with no foreground check: 3.19's check is for serve's background reader, and a backgrounded `agentic gate` simply stops at its read, as job control does.
6. **API shapes:** `GET /api/v1/inbox`, `GET /api/v1/inbox/{id}`, `POST /api/v1/inbox/{id}/resolve {decision, note?, confirmItemId?}`. Errors: `not-found`, `no-project`, `item-resolved` (409), `not-waiting` (409), `decision-not-allowed`, `note-required` and `confirm-required` (422). Event `inbox.updated` with exactly `{id, status}` (owner condition 4). No paging (3.2 D2).
7. **`agentic status`** prints every item of a run (resolved ones with their decision and "You (Reviewer)"), never a note. **`agentic run`** adds one line naming `agentic gate` after its Review line.
8. **The hat word** "Reviewer" is one am-domain constant (AD-24's product word, not a role template; the denylist does not name it).

### Tests-first list (owner ask 6)

All on FakeLlm, FakeClock and FakeTerminal; no network, no real model. Each file is written before any `src/` change, with its red reason logged. New files in `tests/trust/` and `tests/engine/` get a `[[test]]` target each (`trust_targets` checks this).

**`crates/am-store/tests/inbox_resolve.rs`** (new):
- `a_resolve_records_the_decision_and_only_the_human_edge`: rows 2, 3, 7 (store level), 10. Red: compile, no `HumanActor` or `Writer::resolve_inbox_item`.
- `a_resolved_item_never_changes_again`: row 23, including a raw `UPDATE`. Red: compile.
- `a_decision_must_fit_the_kind`: row 24. Red: compile, no `Decision`.
- `a_return_or_answer_needs_a_note_within_its_rules`: row 25. Red: compile.
- `the_note_is_stored_once`: row 25; scans every table and the audit export for the note text. Red: compile.
- `apply_follows_the_decision_and_nothing_else`: row 26. Red: compile, no apply command.
- `the_return_count_is_checked_inside_the_transaction`: rows 4, 5; an apply whose past-max claim disagrees with the journal is refused. Red: compile.
- `a_gate_note_input_needs_a_human_return`: row 28. Red: compile.
- `no_system_command_changes_an_open_items_status`: row 22. Red: compile (`Reader::inbox_item`).
- `two_resolves_of_one_item_one_wins`: row 40, from two threads. Red: compile.
- `dismiss_changes_nothing_but_the_item`: row 34. Red: compile.
- `an_answer_takes_its_step_out_of_waiting`: row 35. Red: compile.
- `the_store_doctests_pair_each_refusal_with_an_allowed_build`: reads `crates/am-store/src/lib.rs` for the four doctests of Design Notes (the implementer writes doctests, as in 3.3). Red: assertion, they are absent.

**`crates/am-store/tests/run_journal.rs`** (additions; plus the L528 supersession):
- `migration_0008_keeps_every_item_open_with_its_taint`: row 37. Red: assertion, `SCHEMA_VERSION` is 7.
- `migration_0008_keeps_the_taint_trigger_and_adds_its_own`: row 37. Red: assertion.
- `reopening_a_migrated_store_applies_nothing`: row 37. Red: assertion (no 0008 step).
- `a_forged_resolution_fails_0008_closed`: row 37. Red: assertion, the migration succeeds (no 0008).
- `migration_0008_loses_no_item_and_keeps_the_pre_migration_copy`: row 37a (owner condition 5). Red: assertion, no 7 → 8 step and no copy.

**`crates/am-e2e/tests/engine/handoff_gate_resolve.rs`** (new):
- `approve_continues_the_run_and_the_next_step_gets_the_reviewed_output`: rows 2, 29. Red: compile, no `continue_run`.
- `return_queues_the_producer_and_the_gate_with_one_gate_note_input`: row 3. Red: compile.
- `the_note_reaches_the_producers_prompt_as_a_trusted_part`: row 3, through FakeLlm's recorded request. Red: compile.
- `the_return_count_comes_from_the_journal`: row 4, a fresh `Reader`. Red: compile.
- `past_max_returns_the_gate_fails_with_one_failure_item`: rows 4, 5. Red: compile.
- `reject_fails_the_gate_and_the_run`: row 7. Red: compile.
- `a_resolve_stopped_before_its_apply_is_applied_once`: row 30. Red: compile.
- `continue_refuses_another_snapshot`: row 31. Red: compile, no `RunError::ConfigChanged`.
- `continue_refuses_a_step_left_running`: the boundary with 3.9. Red: compile.
- `an_api_resolve_leaves_the_run_ready_and_continue_finishes_it`: rows 11, 33. Red: compile.

**`crates/am-e2e/tests/trust/inbox_resolve_human_only.rs`** (new):
- `model_output_never_resolves_a_gate`: row 22, through `continue_run`. Red: compile.
- `no_tool_or_manifest_names_the_inbox`: row 22. Green on arrival (a guard).
- `a_websocket_frame_resolves_nothing`: row 22. Red: compile (`Reader::inbox_item`).
- `the_cli_token_alone_resolves_nothing`: row 13. Red: assertion, 404 `not-found` (no route).
- `am_engine_never_names_the_resolve`: a structural scan of am-engine's `src/` (comments and literals removed). Green on arrival (a guard; xtask holds the red one).

**`crates/am-e2e/tests/trust/rule_15_inbox.rs`** (new, with 3.2's rig, extended with a project writer):
- `the_inbox_reads_with_the_cli_token_or_a_session`: row 12. Red: assertion, 404.
- `resolve_needs_the_session_csrf_and_a_matching_origin`: row 14. Red: assertion, 404.
- `resolve_is_never_a_get_head_put_or_delete`: row 14. Red: assertion, 404 instead of 405.
- `the_same_resolve_over_the_api_is_audited_you_reviewer`: rows 10, 11. Red: assertion, 404.
- `a_second_resolve_is_refused`: row 40. Red: assertion, 404.
- `inbox_changes_are_published_once_without_words`: row 39. Red: compile (`EVENT_INBOX_UPDATED`).
- `an_inbox_event_carries_only_the_id_and_status`: row 39 (owner condition 4); the data object's keys are exactly `id` and `status`, and canary strings planted in the payload, title, body and note appear in no frame. Red: compile.
- `a_tainted_approve_over_the_api_needs_the_item_id`: row 27a. Red: assertion, 404.
- `the_first_look_publishes_no_old_item`: row 39. Red: compile.

**`crates/am-e2e/tests/trust/taint_trusted_input_gate_note.rs`** (new; named for AD-10's `taint_trusted_input_*.rs` trust tests, which cover each source and each refusal):
- `a_return_note_is_trusted_from_a_human_only`: rows 3, 28. Red: compile.
- `a_note_never_clears_the_producers_taint`: row 27. Red: compile.
- `a_forged_non_human_resolver_starts_no_attempt`: row 28. Red: compile.
- `approval_carries_the_payloads_taint_to_the_next_step`: row 29. Red: compile.
- `resolving_keeps_every_taint`: row 27. Red: compile.

**`crates/am-e2e/tests/trust/rule_16_audit_log.rs`** (addition):
- `a_resolution_is_audited_as_you_reviewer_without_the_note`: row 10. Red: compile.

**`crates/am-app/tests/gate_cli.rs`** (new; in process through `am_app::cli::gate` with FakeTerminal and FakeLlm on the `hand-offs` fixture):
- `it_lists_the_waiting_review_with_its_payload`: row 1. Red: compile, no `am_app::cli::gate`.
- `the_payload_is_printed_with_control_characters_neutralised`: row 1. Red: compile.
- `approve_resumes_the_run`: row 2. Red: compile.
- `return_reruns_the_producer_with_the_note_in_its_prompt`: row 3. Red: compile.
- `returning_past_max_returns_fails_with_one_failure_item`: rows 4, 6. Red: compile.
- `max_returns_zero_fails_on_the_first_return`: row 5. Red: compile.
- `reject_ends_the_run`: row 7. Red: compile.
- `reject_needs_its_confirmation`, `skip_writes_nothing`: rows 8, 9. Red: compile.
- `a_non_interactive_terminal_is_refused_before_anything_is_read`: row 16. Red: compile.
- `an_owner_child_moves_decisions_to_the_app`: row 17. Red: compile.
- `a_project_held_by_serve_is_refused`: row 18. Red: compile.
- `approve_waits_while_the_config_differs_from_the_runs_pin`: row 31. Red: compile.
- `a_tainted_review_shows_the_untrusted_marker_and_resolves`: row 27. Red: compile.
- `a_tainted_review_says_its_output_came_from_untrusted_content`: row 27 (owner condition 2); the words appear above the payload and at the choice, and never for an untainted Review. Red: compile.
- `approving_a_tainted_review_needs_its_id_typed`: row 27a; each wrong answer writes nothing, the exact id approves, an untainted Review approves on `a` alone. Red: compile.
- `dismiss_resolves_a_failure`: row 34. Red: compile.
- `a_question_is_answered_and_not_continued`: row 35. Red: compile.
- `ctrl_c_or_a_timeout_writes_nothing`: row 36. Red: compile.
- `status_shows_each_items_decision_and_who`: rows 33, 38. Red: compile (shared helpers); its assertion fails too.
- `agentic_run_names_agentic_gate_at_a_gate`: row 41. Red: compile (in this file).

**`crates/am-app/tests/gate_tty.rs`** (new; the real binary, HOME in a temporary folder; the waiting gate is prepared through `open_project` and System store commands, since the binary wires no provider; pseudo-terminal rows macOS only through `/usr/bin/script`, as 3.19's harness, with a compiling skip elsewhere, AD-47):
- `piping_input_to_agentic_gate_is_refused`: row 15. Red: assertion, exit 2 (unknown command).
- `on_a_terminal_a_typed_reject_ends_the_run`: rows 7, 10. Red: assertion, exit 2.
- `on_a_terminal_approve_is_refused_while_no_provider_is_wired`: row 32. Red: assertion, exit 2.
- `an_enabled_stdio_mcp_server_refuses_it_on_a_terminal`: row 17. Red: assertion, exit 2.
- `agentic_gate_has_no_approve_flag`: row 43. Red: assertion, the usage line does not name `agentic gate`.

**`crates/am-config/tests/hand_offs.rs`** (additions):
- `max_returns_parses_on_a_gate_with_its_default`: Decision 1. Red: compile, no `Step.max_returns`.
- `max_returns_without_a_gate_or_out_of_range_is_refused`: row 42. Red: compile.

**`crates/am-server/tests/route_table.rs`** (changed `expected()`):
- `the_table_holds_exactly_the_shipped_routes` (existing test, new rows in `expected()`): the three routes with their classes. Red: assertion, the table differs. The existing `no_state_changing_route_accepts_get_or_head` and `every_served_route_is_in_the_table` then cover the resolve route unchanged.

**`xtask/tests/check_deps.rs`** (addition):
- `the_inbox_resolve_is_named_only_by_its_two_handlers`: row 21; a fixture tree with planted calls in am-engine, am-tools and am-mcp, and the allowed sites passing. Red: assertion, the check passes the planted calls.

**Doctests and src unit test (implementer, like 3.3's):** the four doctests on `Writer::resolve_inbox_item` (row 19) and the writer-refusal unit test (row 20). Their presence is checked by `the_store_doctests_pair_each_refusal_with_an_allowed_build`.

**Unchanged and expected green:** `rule_15_*` (but the new file), `handoff_next`, `handoff_loop_back`, `handoff_gate` (`HandOff::Gate` literal aside), `taint_*`, `inbox.rs`, `loop_caps`, `toolgate`, `run_cli`, `serve_*`, xtask `release_deps` (one terminal), `denylist`, `seam_guard`, `paused_time`.

About 90 tests. Most red reasons are "fails to compile"; the route, real-binary, migration and xtask tests fail on assertions.

### Out of scope (by ticket)

- 3.5: full labelled prompt assembly (the note's place is restated there).
- 3.9: resume after a crash or restart, runs left ready, a step left `running`.
- 3.10, 3.14: what an answered Question does.
- 3.11: `agentic stop` and `waiting_human → cancelled`.
- 3.16: review gates by a seat, `waiting_review`, an AI reviewer's notes.
- 3.17: lineage reads over `step_inputs.from_item`.
- 4.3, 4.8, 4.10, 3.6: resolving BudgetAsk, Approval, Proposal and Draft items.
- 4.17: `is_widening` for `max_returns`.
- 6.7: the Inbox UI, "Edit, then approve", the tainted-Review switch.
- UI epics: `agentic open`, ts-rs generation, paging.

**Size:** large (Q9). Eight crates plus xtask, one table rebuild, about 90 tests.

## Verification

- `cargo nextest run -p am-store --test inbox_resolve --test run_journal --test inbox`
- `cargo nextest run -p am-e2e -E 'binary(/handoff_gate/) | binary(/inbox_resolve/) | binary(/rule_15/) | binary(/rule_16/) | binary(/taint_/)'`
- `cargo nextest run -p am-app --test gate_cli --test gate_tty --test run_cli`
- `cargo nextest run -p am-server -p am-config -p am-engine`
- `cargo test --doc -p am-store -p am-domain` and `cargo test -p xtask --test check_deps`
- then `just check`

## Implementation Notes
