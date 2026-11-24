---
title: 'CLI Owner gate'
type: feature
ticket: 4
created: '2026-11-24'
status: built
baseline_revision: '30d1f4629876f9ce98a81c35d8827442bfe523dd'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [correctness, adversarial-security]
review_loop_iteration: 2
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

### API the tests fix (tester, 2026-11-24)

The tests pin this API. Names not listed (internal functions, audit actions other than those named, column names other than those named) are the implementer's. New types derive `Debug, Clone, PartialEq, Eq` (plus `Copy` where marked); the tests compare rows with `assert_eq!`.

**am-domain** (exported at the crate root):
- `pub struct HumanActor` with private fields `person`, `hat`; no `Default`, no `Deserialize`, no `From<Actor>`. `HumanActor::new(person: impl Into<String>, hat: impl Into<String>) -> HumanActor`; `impl From<HumanActor> for Actor` (→ `Actor::Human { person, hat }`).
- `pub const REVIEWER_HAT: &str = "Reviewer";`
- `pub enum Decision { Approve, Return, Reject, Answer, Dismiss }` (`Copy`): `Decision::ALL: [Decision; 5]` in that order; `as_str(self) -> &'static str` (`approve`, `return`, `reject`, `answer`, `dismiss`); `parse(&str) -> Option<Decision>` (exact spelling only); `allowed_for(kind: InboxKind) -> &'static [Decision]` (Review `[Approve, Return, Reject]`, Question `[Answer]`, Failure `[Dismiss]`, the other four `[]`).

**am-store** (exported at the crate root):
- `Writer::resolve_inbox_item(&self, by: HumanActor, item_id: &str, decision: Decision, note: Option<String>) -> Result<(), StoreError>`. Commits: `status = "resolved"`, the decision, the note (return and answer only), `resolved_by` = the serde JSON of `Actor::Human { person: "owner", hat: "Reviewer" }`, `resolved_at` (store timestamp); a Review's or Question's step `waiting_human → running`; nothing else (no attempt added, the run unchanged); one audit entry `inbox.resolved`: target the item id, actor the Human, `args` `kind` (`InboxKind::as_str`), `decision` (`Decision::as_str`), `step_run_id`, `taint` (`0`/`1` or `false`/`true`), `note_sha256` (hex; null or absent without a note); never the note's text.
  - Refusals (any `Err`, nothing written): a second resolve, its words holding `You (Reviewer)` and the committed `resolved_at` string; a decision that does not fit the kind; any decision on Approval, Draft, BudgetAsk, Proposal, its words naming `4.8`, `3.6`, `4.3`, `4.10`; a return or answer without a note, or with an empty or blank one, one over 2000 characters (characters, not bytes: 2000 × `é` passes), or one holding a control character but tab (`\n`, `\r`, ESC, BEL, NUL, DEL); a note on approve, reject or dismiss. Two threads resolving one item: exactly one `Ok`.
- `WriteCommand::ApplyResolution { item_id: String, past_max: Option<NewInboxItem> }`: only `Actor::System` (the four others refused). Refused on an open item and a second time. Decided from committed rows:
  - approve: the gate attempt `running → succeeded` (`finished_at` set; an audit action containing `step.finished`);
  - return, the count within the item's `max_returns`: the gate `looped_back`; attempt n+1 of every step from the producer through the gate queued (not after it); the producer's new attempt gets exactly one `step_inputs` row `StepInput { kind: "gate_note", from_step_run: None, from_item: Some(item id), label: "gate-note:<gate key>@<gate attempt>", taint: Clean }`, the gate's new attempt none; an audit action containing `looped_back`;
  - return past it: the count is this run's Review items on attempts of the gate step resolved `return`, this one included; past when it exceeds the item's `max_returns` (so 0 fails the first). It must carry `past_max: Some(item)`: the gate `failed`, that Failure item created on the gate attempt, the run `failed`. A `past_max` that disagrees with the journal (either way) is refused;
  - reject: the gate `failed`, the run `failed` (`finished_at`), no Failure item, `run.finished` audited.
  - The decision guard: on a gate attempt whose Review is resolved, `FinishStep failed` and `FailWithItem` after approve, `FinishStep succeeded` after reject, `FinishStep` either way after a return within the maximum, and `SettleAndLoopBack` after approve are refused, changing nothing.
- `NewInboxItem` gains `pub max_returns: Option<u32>` (a Review's, from its gate). `StepInput` gains `pub from_item: Option<String>` (a `gate_note` input's item; `None` otherwise). `InboxItemRow` gains `pub decision: Option<Decision>`, `pub note: Option<String>`, `pub max_returns: Option<u32>`; `status` is `open` or `resolved`.
- `Reader::inbox_item(&self, id: &str) -> Result<Option<InboxItemRow>, StoreError>`; `Reader::open_items(&self) -> Result<Vec<InboxItemRow>, StoreError>` (every open item of every run; order not pinned).
- A `gate_note` input sent with `StartAttempt` is refused when its item was resolved by a non-Human, resolved other than `return`, is open, is of another run, is not named, or when the input has a `from_step_run` or is tainted.
- Migration 0008 (`SCHEMA_VERSION` 8) as Design Notes: `inbox_items` columns `max_returns`, `decision`, `note` (and `resolved_by`, `resolved_at` kept); triggers named `inbox_items_taint_only_rises`, `inbox_items_resolved_never_change`, `inbox_items_fixed_fields`; index `inbox_items_by_status`; `step_inputs.from_item`. The table CHECK refuses a status but `open`/`resolved`, an open row with a decision or a resolver and time, a resolved row missing its decision, resolver or time, and `max_returns` outside 0..=5; a raw `UPDATE` raising an open item's taint is allowed. A forged v7 `open` row carrying `resolved_by`/`resolved_at` fails the step: `StoreError::MigrationFailed { from: 7, to: 8, backup: Some(_) }`, its words holding the backup's path, the database left at `user_version` 7 with every row unchanged, the copy at version 7 kept.
- The four doctests (the implementer's): `compile_fail` blocks holding [`resolve_inbox_item(`, `Actor::System`], [`HumanActor`, `.into()`] and [`exec(`, `WriteCommand::ResolveInboxItem`], beside a plain one holding `resolve_inbox_item(`, `HumanActor::new(` and `Decision::`.

**am-engine**:
- `HandOff::Gate { max_returns: u32 }`; the Review item made at the pause carries it.
- `pub async fn am_engine::run::continue_run(store: &Store, run_id: &str, plan: &RunPlan, cancel: CancellationToken) -> Result<RunOutcome, RunError>`.
- `RunError::ConfigChanged { pinned: String, now: String }` (its words hold at least the first 12 characters of both hashes; nothing written); `RunError::NotContinuable(String)` for a step left `running` (words naming `3.9`) and for a finished run; an unknown run is any `Err`.
- `pub fn am_engine::gate::apply_resolution(writer: &am_store::Writer, reader: &am_store::Reader, item_id: &str) -> Result<T, E>` with `E: Debug` (the tests call `expect`, `unwrap` and `is_err`; `Result<(), StoreError>` suggested); a second apply is `Err`.
- Outcomes through `continue_run`: approve `succeeded`; return `waiting_human` with `item` the new Review's id; past the maximum `failed` with a `reason` and `item` the Failure item's id; reject `failed` with a `reason` holding `rejected`, the gate key and `You (Reviewer)`; an unresolved waiting gate `Ok(waiting_human)` (or `Err`) with nothing changed; a `gate_note` whose item's `resolved_by` is not Human (forged by raw SQL): the producer's attempt stays `queued`, no model call, an `Ok` outcome is not `succeeded` and has a `reason`.
- The past-max Failure item: kind Failure on the gate attempt; its body holds the gate key, `max_returns`, the number as a word and the Review item's id; neither title nor body holds a note.
- The producer's new request: the first user part is the step's `Part::Instruction`; one `Part::Trusted` labelled `gate-note:<gate>@<attempt>` with the note's text, before any `Part::Untrusted`; the note in no other part and not in `system`; not the producer's own earlier output. The step after an approved gate gets a `step_output` input `step:<producer>@<attempt>` from the Review's payload with its committed taint (trusted when clean, fenced and tainting when tainted).
- am-engine's code (comments and literals left out) names neither `resolve_inbox_item` nor `HumanActor`.

**am-api**: `pub const EVENT_INBOX_UPDATED: &str = "inbox.updated"`; `InboxUpdatedDto { id: String, status: String }` (`Serialize`, `Deserialize`; serialises to exactly those two keys). camelCase, `Deserialize` for the reads:
- `InboxItemDto { id, kind /* as_str */, status, run_id, step_run_id, taint: bool, title, body, created_at, max_returns: Option<u32>, decision: Option<String>, resolved_by: Option<String> /* "You (Reviewer)" */, resolved_at: Option<String> }` (the tests read `id`, `kind`, `status`, `run_id`, `taint`, `max_returns`, `decision`, `resolved_by`);
- `InboxItemDetailDto { item: InboxItemDto, payload: Option<String> /* the payload's text */, note: Option<String> }`;
- `ResolveBody { decision: String, note: Option<String>, confirm_item_id: Option<String> }` (`Serialize` too; the wire key `confirmItemId`);
- `ResolutionDto { item: InboxItemDto, run: am_api::RunDto }`.

**am-server**: `AppStateBuilder::project_writer(self, writer: am_store::Writer) -> Self`. Routes `GET /api/v1/inbox` (CliClass; open items only), `GET /api/v1/inbox/{id}` (CliClass), `POST /api/v1/inbox/{id}/resolve` (UiClass, changes state). Envelope codes: 404 `not-found` (unknown item, read or resolve), 409 `no-project`, 409 `item-resolved`, 422 `decision-not-allowed` (dismiss or answer on a Review), 422 `note-required` (return with no or an empty note), 422 `confirm-required` (tainted approve without `confirmItemId`, or with an empty one, a prefix, another id, the id in capitals), 403 `ui-class-only` (the CLI token). A 200 resolve returns `ResolutionDto`; the server applies as `System(engine)` and runs no model step. `watch_runs` publishes one `inbox.updated` per item change (made: `open`; resolved: `resolved`), data exactly `{id, status}`; its first look publishes nothing; no second event for one change.

**am-config**: `Step.max_returns: u32`, 2 when a gate omits it. Findings at the key's line, with a fix: on a step without `gate:` (message holds `max_returns` and `gate`); outside 0..=5 or not a whole number (`6`, `-1`, `99`, `two`, `1.5`, `[1]`; message holds `'max_returns'`); never `unknown key 'max_returns'`.

**am-app**:
- `am_app::cli::GateContext { places: Places, identity: Arc<dyn ProjectIdentity>, terminal: Box<dyn Terminal>, providers: am_app::cli::Providers }` and `pub async fn am_app::cli::gate(args: &[String], ctx: GateContext, out: &mut dyn Write, err: &mut dyn Write) -> i32`; args `[<item id>] [--project <folder>]`; `--yes`, `--approve`, `-y` are usage errors (exit 2, `usage: agentic gate` on stderr); `main` dispatches `gate`.
- Streams: everything about an item and every prompt goes to `Terminal::show`; the `run <id> <status>` line to `out`; refusals to `err`.
- Shown for an item: its id, its run's id, the workflow, the step key, `attempt <n>`, its title and body, for a Review `<returns> of <max_returns>` (`0 of 2`) and the payload with every control character but newline and tab neutralised (ESC, BEL, CR, C1 included). A tainted Review shows `came from untrusted content` before the payload and again after it (at the choice); an untainted one never shows `untrusted content`.
- Every read is `read_line(Some(600 s))`. Choices: `a` (a tainted Review then reads the item id; only the id, surrounding spaces trimmed, approves; an untainted one approves on `a` alone, one read); `r` (past the maximum: a warning holding `Failure item`, then `y` confirms, anything else writes nothing; then the note); `j` then `reject` exactly; `d`; `answer` then the answer's text (a Question; not continued; the words name `3.10` or `3.14`); `s`. `interrupted()` is asked before each write.
- Refusals (exit 1): a non-interactive terminal (words `terminal`, zero reads, the project not opened); an enabled stdio MCP server or `render: true` Source (naming its id, and `Inbox`) or a `tools.yaml` with an error (naming `tools.yaml`), zero reads; a held project (words `agentic serve`, zero reads); for `a` and `r`, a snapshot hash other than the run's pin (both hashes' first 12 characters) or a config error; on the binary, `a` refused with `no model provider is wired` (reject works).
- Exit 0 for skip, approve (prints `run <id> succeeded`), a return that waits again, reject (prints or says `rejected`), dismiss, answer; exit 1 past the maximum, on an interrupt and on a timeout.
- `agentic status` prints every item: a resolved one's line holds `resolved`, the decision's `as_str` and `You (Reviewer)`; an open one's `open`; never a note; a run left ready (resolved and applied, not continued) has a line holding `ready to continue`, a failed run none. Item lines hold no run id. `agentic run` at a gate prints its 3.3 lines unchanged plus exactly one stdout line holding `agentic gate`.

**xtask**: `check-deps` fails on `resolve_inbox_item` or `HumanActor::new` named in shipped code (comments and literals left out) anywhere but `crates/am-server/src/inbox.rs`, `crates/am-app/src/gate.rs`, the definitions (am-store's `fn resolve_inbox_item`, am-domain's `HumanActor`) and `#[cfg(test)]` items; each finding names `<file>:<line>`, the name and `AD-15`, and never a definition's file.

**Test-only support** (folders, not targets): `crates/am-e2e/tests/trust/support/server.rs` gains `Ws::send_text`; new `crates/am-e2e/tests/trust/support/inbox.rs` (3.2's rig with `project_writer`, the Inbox paths, a resolve with the full session, `events_until`).

**Fixture**: `fixtures/projects/hand-offs/workflows/returned.yaml` (`draft` → `approve` with `gate: review`, `max_returns: 1` → `publish`); `gated.yaml` unchanged.

**Changed existing tests**:
- the plan's three supersessions: `run_journal.rs` (the 0007 test no longer pins 7 as the last version), `route_table.rs` `expected()` (the three routes), the engine support's `HandOff::Gate { max_returns: 2 }`;
- beyond the plan, forced by the struct changes: `from_item: None` in the 21 `StepInput` literals (`am-store` `inbox.rs`, `run_journal.rs`; `am-e2e` `handoff_gate`, `handoff_next`, `handoff_loop_back`, `taint_at_rest`, `taint_handoff_chain`) and `max_returns` in the 7 `NewInboxItem` literals (`Some(2)` on a Review, `None` on a Failure: `inbox.rs`, `handoff_gate.rs`, `taint_at_rest.rs`);
- `am-config` `hand_offs::the_fixture_validates` lists `returned` and checks both gates' `max_returns`.

**Ambiguities resolved here:** the apply is the plan's suggested `ApplyResolution { item_id, past_max }`; `StepInput.from_item` (the plan names the column, not the field; a `gate_note` input could not be sent or read without it); a note on approve, reject or dismiss is refused, not dropped; a Question is answered by typing `answer`, then the text; a past-maximum return asks `y` before the note; the trimmed id approves, capitals do not; `#[cfg(test)]` items are not shipped code for the new rule; `continue_run` on a finished run is `NotContinuable`. Added beyond the list: `rule_15_inbox::a_resolve_that_does_not_fit_is_refused_in_the_envelope` (Decision 6's codes).


**Review round 1 additions (tester, 2026-11-24)** (the triage's R1–R13; each pinned by a test logged under `### Red run, round 1`):
- **R1, am-server:** a port in the server state, `pub trait am_server::PinCheck: Send + Sync { fn current_snapshot(&self) -> Result<String, String>; }` (the hash of the snapshot the project's files give now, or why no run may go on on it, in words), given with `AppStateBuilder::pin_check(self, check: Arc<dyn PinCheck>) -> Self`. Approve and return are refused 409 `config-changed` before anything is written when the pin check's hash is not the run's `config_hash` (the envelope's `message` or `fix` holds the first 12 characters of both hashes), when it returns `Err` (a config error, fails closed), and when the state has no pin check. Reject and dismiss need none.
- **R1, am-app:** `pub fn am_app::serving::pin_check(places: &Places, root: &Path, store: &am_store::Store) -> Arc<dyn am_server::PinCheck>`, what `agentic serve` hands the server: `agentic gate`'s check 5. `Ok(hash)` equals `am_config::load`'s snapshot hash, also after a workflow edit; `Err` for a config error and for an outside change of a file the app wrote (`company.yaml`).
- **R2:** before anything is written, an approve or a return whose run `continue_run` would refuse (another step `running`, an unsettled model call, an `issued` tool call) is refused: over the API 409 `not-continuable`; in `agentic gate` exit 1 with words on stderr. `am_api::ResolutionDto` gains `pub applied: bool`: `true` after a normal resolve; when the apply fails after the decision committed, the API answers 200 with the resolved item, the run and `applied: false`, never 500.
- **R3:** `ApplyResolution` is refused, with nothing changed, unless the item's committed `resolved_by` parses to `Actor::Human`; a return so forged journals no `gate_note` row.
- **R4:** `INSERT OR REPLACE` and `REPLACE` over a resolved `inbox_items` row, by its `id` or by its `(step_run_id, kind)`, are aborted (a `BEFORE INSERT` trigger); a plain INSERT of a new open item still works.
- **R5:** a `gate_note` whose resolver is not a person (forged) leaves the run `failed` with exactly one Failure item, the outcome's `item`; its body names the step (`draft`) and the forged item's id, and holds no note.
- **R6:** `continue_run` with a plan whose `workflow` is not the run's is `RunError::NotContinuable`, before anything is applied.
- **R7, the payload display:** every payload line is shown behind the prefix `"  | "` (two spaces, a vertical bar, a space); the payload adds no line without it. Every character in Cc (but newline and tab), Cf, Zl, Zp and the tag block is shown as `\u{XXXX}` (uppercase hex, at least four digits: `\u{202E}`, `\u{E0041}`), never raw, and when there is any, one count line holding `<n> hidden or control characters in this output are shown as \u{XXXX}`; none for a clean payload. A run of blank lines is shown as at most one; newline and tab are kept. The store refuses a note holding a Cf, Zl or Zp character (U+2028, U+2029, U+202E, U+2066, U+200B, U+FEFF, U+E0041 tested).
- **R8:** a note or answer the store refuses ends `agentic gate` with exit 1.
- **R9:** a new `Terminal` port method, `fn discard_typed_ahead(&mut self)` (the owner's terminal: `tcflush(TCIFLUSH)`), called before every prompt. am-testkit's `FakeTerminal` gains `type_ahead(&self, text: &str)` (a line typed with Return before the next prompt is shown: the next read returns it unless the command discarded what was typed ahead first; `discard_typed_ahead` drops every such line at the front of the script) and `discards(&self) -> usize` (how many times it was called; the tests want at least one per read). After a return, `agentic gate` shows the recorded note back.
- **R10:** a Ctrl-C reported by the terminal while `continue_run` works (here `FakeTerminal::interrupt()` called from inside the model call) cancels the token passed to `continue_run`; the step ends `failed` or `cancelled`, the run stops, and `agentic gate` exits 1. The tests use only `interrupt()`; how the command watches for it (polling `interrupted()`, or a new port method) is the implementer's.
- **R11:** `FakeTerminal::unfinished(&self, text: &str)`: the owner types `text` with no Return, then ends the input (Ctrl-D twice); the read is `Err(TerminalError::Closed)`. The port's contract: `read_line` returns only a line ended by Return; the owner's terminal returns `Closed` for a partial line followed by the end of input (the real binary's test drives this on a pseudo-terminal).
- **R12:** `check-deps` matches whole files by token: `HumanActor` and `resolve_inbox_item` may stand in shipped `src/` only in `crates/am-domain/src/` (the definition and its re-export), `crates/am-store/src/lib.rs`, `crates/am-server/src/inbox.rs` and `crates/am-app/src/gate.rs`, and in `#[cfg(test)]` items of am-store only; findings name `<file>:<line>` of the token, `HumanActor` and AD-15.


**Review round 2 additions (tester, 2026-11-24)** (the triage's S1–S7; each pinned by a test logged under `### Red run, round 2`):
- **S1:** an interrupt while a continued run works ends `agentic gate`: it prints that run's `run <id> <status>` line on stdout and exits 1; no later item is shown and nothing more is read.
- **S2:** `INSERT OR REPLACE` and `REPLACE` into `inbox_items` are aborted whenever a row with the same `id` or the same `(step_run_id, kind)` exists, open or resolved; a plain INSERT of a new item works.
- **S3, the wrap rule:** each payload line, after the hidden characters are escaped and each tab is replaced by exactly 4 spaces, is cut into segments of at most 72 columns, shown in order each behind `"  | "`. A character in the common wide ranges (U+1100–115F, U+2E80–A4CF, U+AC00–D7A3, U+F900–FAFF, U+FE30–FE4F, U+FF00–FF60, U+FFE0–FFE6, U+1F300–1F64F, U+1F900–1F9FF, U+20000–3FFFD) counts 2 columns, every other 1. A segment ends before the character that would pass 72 columns (200 `x` are 72, 72, 56; 40 × U+4E00 are 36, 4 characters), and an escape `\u{XXXX}` is never cut between segments. A blank line is one that is empty after `trim`.
- **S4:** the hidden set is Cc (but newline and tab) ∪ Zl ∪ Zp ∪ Unicode's Default_Ignorable_Code_Point (which takes in Cf's invisible ones, the tag block, the variation selectors U+FE00–FE0F and U+E0100–E01EF, U+034F, U+115F, U+1160, U+3164, U+FFA0, U+17B4–17B5, U+180B–180F), in `am_domain::text`, the one place the display and the note check share. The tests use U+FE0F, U+E0100, U+034F, U+115F, U+3164, U+17B4, U+180B, U+E0041 and U+202E.
- **S5:** in am-domain, only `src/actor.rs` (the definition) and `src/lib.rs`'s re-export line may name `HumanActor`. In every allowed file, an `as` alias (`use … HumanActor as X`) or a `pub use` of either token fails the check. A plain `use am_domain::{…, HumanActor, …};` in the two handler files passes. Each finding names `<file>:<line>`, `HumanActor` and AD-15.
- **S6:** once the resolve is recorded, the API answers 200 with `applied: false` whatever fails after it. When the item cannot be read back, the body still holds `item.id` and `item.status: "resolved"` (the tests read the body as JSON; the rest of `item` and `run` is the implementer's).
- **S7:** an item's title and body are shown through the same display as a payload: hidden characters escaped (ESC as `\u{001B}`).

### Red run (2026-11-24)

Shared `CARGO_TARGET_DIR` (`/Users/hiteshmamgain/cargo-target/agentic-manager`), unchanged; no `cargo clean`, no scratch copy of the workspace. No `src/` file changed (`git status --short | grep /src/` is empty). Each target ran alone, since one compile failure aborts a nextest build. Commands:
- `cargo nextest run -p am-store --test inbox_resolve --no-fail-fast` (and `--test run_journal`, `--test inbox`);
- `cargo nextest run -p am-e2e --test <target> --no-fail-fast` for each target below;
- `cargo nextest run -p am-app --test gate_cli --no-fail-fast` (and `--test gate_tty`, `--test run_cli`);
- `cargo nextest run -p am-config --test hand_offs --no-fail-fast`; `cargo nextest run -p am-server --test route_table --no-fail-fast`;
- `cargo test -p xtask --test check_deps`;
- `cargo xtask denylist` (and `runtime-paths`, `fixed-hosts`, `seam-guard`, `check-forbid`): clean;
- `cargo clippy -p am-app --test gate_tty -- -D warnings`, and the same for `am-server --test route_table`, `xtask --test check_deps`, `am-e2e --test rule_15_events --test rule_15_csrf`: clean.

| Target | New/changed tests | Observed red |
|---|---|---|
| am-store `inbox_resolve` (new) | 13 | compile, 30 errors; first: unresolved `am_domain::{Decision, HumanActor, REVIEWER_HAT}`; then `NewInboxItem.max_returns`, `StepInput.from_item`, `WriteCommand::ApplyResolution`, `Writer::resolve_inbox_item`, `Reader::{inbox_item, open_items}`, `InboxItemRow.{decision, note, max_returns}` |
| am-store `run_journal` | +5, 1 line changed | compile, 13 errors; first: unresolved `am_domain::{Decision, HumanActor, REVIEWER_HAT}` (the plan expected assertions; they cannot run while the file does not build) |
| am-store `inbox` (literals) | 0 | compile, 14 errors: `StepInput.from_item`, `NewInboxItem.max_returns` |
| am-e2e `handoff_gate_resolve` (new) | 10 | compile, 24 errors; first: unresolved `am_domain::{Decision, …}`, `am_engine::run::continue_run`, `am_engine::gate`, `RunError::{ConfigChanged, NotContinuable}`, `HandOff::Gate { max_returns }`, `AppStateBuilder::project_writer` |
| am-e2e `inbox_resolve_human_only` (new) | 5 | compile, 5 errors: `continue_run`, `HandOff::Gate { max_returns }`, `project_writer`, `Reader::inbox_item`. The two guards (`no_tool_or_manifest_names_the_inbox`, `am_engine_never_names_the_resolve`) are not observable while the binary does not build; `the_cli_token_alone_resolves_nothing` would see 404 (the probe below) |
| am-e2e `rule_15_inbox` (new) | 10 | compile, 11 errors; first: unresolved `am_api::{EVENT_INBOX_UPDATED, InboxItemDetailDto, InboxItemDto, InboxUpdatedDto, ResolutionDto, ResolveBody}` |
| am-e2e `taint_trusted_input_gate_note` (new) | 5 | compile, 6 errors: `Decision`/`HumanActor`, `continue_run`, `am_engine::gate`, `HandOff::Gate { max_returns }`, `resolve_inbox_item` |
| am-e2e `rule_16_audit_log` | +1 | compile, 5 errors: `Decision`/`HumanActor`, `ApplyResolution`, `resolve_inbox_item`, `StepInput.from_item`, `NewInboxItem.max_returns` |
| am-e2e `handoff_gate`, `handoff_next`, `handoff_loop_back`, `taint_handoff_chain`, `taint_at_rest`, `taint_trusted_input_sources` (literals, support) | 0 | compile, 3/3/3/5/3/1 errors: `HandOff::Gate { max_returns }` (the shared support), `StepInput.from_item`, `NewInboxItem.max_returns` |
| am-e2e `trust_targets`, `rule_15_events`, `rule_15_cli_token`, `cli_validate`, `taint_trusted_input_refusals` | 0 | green (3/3, 9/9, 8/8, 16/16, 3/3): the four new targets are listed, and `Ws::send_text` breaks no `rule_15_*` file |
| am-app `gate_cli` (new) | 21 | compile, 29 errors; first: unresolved `am_app::cli::{GateContext, gate}`, then `Decision`/`HumanActor`, `InboxItemRow.{decision, note, max_returns}`, `am_engine::gate` |
| am-app `gate_tty` (new) | 5 (+1 skip off macOS) | builds (it reads only 3.3's fields); 0/5: each fails in its setup, `agentic run gated` exits 1, "the project's config has 1 error (the first: workflows/returned.yaml:13: unknown key 'max_returns' in step)". Probe on a copy of the fixture without `returned.yaml` (binary only, nothing built): `echo a \| agentic gate --project P` and `agentic gate --yes --project P` both exit 2 with the general usage line, which does not name `agentic gate` — the plan's expected red |
| am-app `run_cli` | 0 | 31/32: `status_lists_attempts_and_a_waiting_step` fails, `agentic run gated` refused for the same unknown key (the fixture change; green once `max_returns` parses) |
| am-config `hand_offs` | +2, 1 changed | compile, 4 errors: no field `max_returns` on `am_config::Step` |
| am-server `route_table` | 0 new (expected() changed) | 3/4; `the_table_holds_exactly_the_shipped_routes` assertion: the table lacks `GET /api/v1/inbox`, `GET /api/v1/inbox/{id}`, `POST /api/v1/inbox/{id}/resolve` |
| xtask `check_deps` | +1 | 18/19; `the_inbox_resolve_is_named_only_by_its_two_handlers` assertion "the engine: must fail" (exit 0; the allowed half passed) |

Probes on today's binary (`$CARGO_TARGET_DIR/debug/agentic`): `agentic validate --template fixtures/projects/hand-offs` exits 1 with `workflows/returned.yaml:13: unknown key 'max_returns' in step`, so `just validate` (and `just check`) is red until am-config parses the key, as the plan intends.

Caveat for the implementer: the new code could only be checked against what exists. rustc reported nothing but missing API in every target, but code typed by a missing item (a `Decision`, the new DTOs, `RunError`'s new variants) is type-checked only once the API exists; a mistake there is the tester's to fix (a test correction), never a reason to bend the API above.

### Implementation (2026-11-24)

Shared `CARGO_TARGET_DIR` (`/Users/hiteshmamgain/cargo-target/agentic-manager`), this checkout only; no `cargo clean`, no scratch copy, no new crate, dependency or feature. The one `Cargo.lock` change is the am-server → am-engine path edge.

**What was built, per crate:**
- **am-domain:** `HumanActor` (private `person`, `hat`; `new`; `From<HumanActor> for Actor` only; no `Default`, `Deserialize` or `From<Actor>`), `REVIEWER_HAT`, `Decision` (`ALL`, `as_str`, `parse`, `allowed_for`, plus `takes_note`), all exported at the root.
- **am-store:**
  - `0008_inbox_resolution.sql` as Design Notes: the rebuild with the table CHECK, the three triggers, `inbox_items_by_status`, `step_inputs.from_item`; `SCHEMA_VERSION` 8.
  - The writer channel carries a crate-private `Command::{Write, Resolve}`; `Writer::resolve_inbox_item(HumanActor, …)` is the only sender of `Resolve`, and `resolve_item` refuses any actor that is not `Actor::Human` before reading anything.
  - `WriteCommand::ApplyResolution { item_id, past_max }`, System only, decided from committed rows (approve, reject, return within or past `max_returns` counted from the journal).
  - The decision guard: `FinishStep`, `SettleAndFinishStep`, `SettleAndLoopBack`, `FailWithItem` and `PauseStep` are refused on an attempt whose Review is resolved.
  - `gate_note` input checks; `StepInput.from_item`; `NewInboxItem.max_returns` (required on a Review, refused on other kinds).
  - Readers: `inbox_item`, `open_items`, the new `InboxItemRow` fields; also `Reader::returns_at` and `InboxItemRow::resolver()`.
  - The four doctests on `resolve_inbox_item`, replacing 3.3's `exec` one, and the src unit test `the_writer_refuses_a_resolve_by_anyone_but_a_person`.
- **am-config:** `Step.max_returns` (default 2, 0..=5, refused without `gate:`), parsed in `kinds.rs` with findings at the key's line, each with a fix.
- **am-engine:**
  - `HandOff::Gate { max_returns }`, journaled on the Review at the pause;
  - `RunError::{ConfigChanged, NotContinuable}`;
  - `run::continue_run`, sharing `drive_to_outcome` (the store-error cleanup) with `run_plan`;
  - `gate::apply_resolution`, returning `gate::Applied { run_id, decision, run_failed, reason, item }`;
  - the `gate_note` part built through `TrustedInput::from_owner_note` from the committed resolver (no person: the attempt does not start, and the run is finished `failed` with a reason);
  - `next` across an approved gate takes the Review's payload.
- **am-api:** `EVENT_INBOX_UPDATED`, `InboxUpdatedDto` (`deny_unknown_fields`), `InboxItemDto`, `InboxItemDetailDto`, `ResolveBody` (`deny_unknown_fields`), `ResolutionDto`.
- **am-server:**
  - `inbox.rs` (list, one, resolve: pre-checks from committed rows, the Human-only command, then `apply_resolution`);
  - the three routes; the envelope codes of Decision 6;
  - `AppStateBuilder::project_writer`; `inbox.updated` `{id, status}` from `watch_runs`;
  - the am-engine edge.
- **am-app:**
  - `gate.rs` (`GateContext`, `gate`), checks 1 to 5 in order, re-exported from `cli`;
  - `main` dispatches `gate` through `owner_side()` and the general usage names it;
  - `serve` passes `project_writer`;
  - `agentic run` adds one `agentic gate` line;
  - `agentic status` prints every item (decision and resolver when resolved) and `ready to continue` for a run left ready;
  - `run.rs`'s checks 5, 6 and 9 take the closing words as a parameter, so `gate` reuses them.
- **xtask:** `check-deps` call-site rule over shipped code. `resolve_inbox_item` and `HumanActor::new` may be named only in `RESOLVE_SITES`, on am-store's `fn resolve_inbox_item` definition line, and in `#[cfg(test)]` items; each finding names `<file>:<line>`, the name and AD-15. The `justfile` doctest comment was updated.

**Choices the plan left open:**
- **Apply refusals:** an apply is refused on an open item and once its gate attempt is no longer `running`, which is how "applied once" is read; the item row itself is never touched by the apply, since a resolved row is frozen by trigger.
- **Decision guard:** it refuses every direct transition of a gate attempt with a resolved Review, not only those against the decision. Only `ApplyResolution` moves it.
- **Unapplied decision:** while a gate is `running` with a resolved, unapplied Review, no other step of the run starts or pauses and the run does not finish. This is defence in depth for 3.9.
- **Past-max Failure item:**
  - payload: the gate attempt, as a cap's Failure item has its own attempt;
  - body: "Step `<gate>` was returned <n> times, past its max_returns of <max>, … Review item <id>";
  - outcome reason: "step `<gate>` failed: it was returned <n> times, past its max_returns of <max>".
- **Reject reason:** "rejected at step `<gate>` by You (Reviewer) (Review item <id>)".
- **Forged gate note:** the attempt stays `queued`, and the run is finished `failed` with the reason "step `<key>` did not start: …".
- **`continue_run` on a run with no pin:** `ConfigChanged { pinned: "no snapshot" }`.
- **`agentic gate`, wrong answers:**
  - An unknown choice or an empty line writes nothing for that item, and the session moves to the next one without asking again.
  - A wrong tainted id, a declined past-max warning or a wrong reject confirmation does the same.
  - Ctrl-C, the end of input and the time limit end the command with exit 1.
- **`agentic gate`, refusals:** a check-5 refusal and a store error end the command with exit 1.
- **`agentic gate`, reject:** it is applied with `gate::apply_resolution`, not `continue_run`, so it works when the config has changed. It prints `run <id> failed` and the reason on stdout.
- **`agentic gate`, return:** check 5 runs before the note is asked for. A note the store refuses (over 2000 characters, a control character) writes nothing and moves on.
- **Held project:** read through `find_runtime` and `lock_is_held` before the open, and again when the open fails, so the words name `agentic serve`.
- **API codes:**
  - An unknown decision spelling, a note on approve, reject or dismiss, or a note the store refuses → 400 `bad-request`, with the store's words.
  - A resolve with no writer in the state → 409 `no-project`.
  - Store or apply errors after the checks → 500 `internal`.
- **Serve shutdown (beyond the plan's list, a small addition):** `AppState::release_project_writer()`, called in `serve` after the server stops. It keeps 3.2's order: the writer handles drop, then the project, so the store's lock goes with the project even while the pairing reader's state clone lives on.
- **`ready to continue`:** the run is `running`, no attempt is `running` or `waiting_human`, and a Review of the run is resolved.

**Mechanical test edits (meaning unchanged):**
- `crates/am-store/tests/run_journal.rs` L533: clippy `assertions_on_constants` (`-D warnings`) on the tester's `assert!(SCHEMA_VERSION > BEFORE_ATTEMPTS, …)`. Wrapped in `const { … }`, clippy's own suggestion: the same assertion, checked at compile time.
- `crates/am-app/tests/gate_cli.rs` L1019: clippy `type_complexity` on `let cases: Vec<(&str, Vec<(&str, &str)>, &str, bool)>`. Factored into a local `type Case<'a> = …;`, with the same tuple.
- `cargo fmt --all` reformatted nothing in the new test files' assertions.

**Test believed mis-specified (stopped on, not changed):**
- `crates/am-e2e/tests/trust/taint_trusted_input_gate_note.rs::resolving_keeps_every_taint`, tainted × Approve, at L318: `before` includes `publish` attempt 1 while it is still `queued` and clean. After approve, `continue_run` runs `publish` on the tainted payload, so its taint rises to 1, as AD-10 and row 29 require. `approval_carries_the_payloads_taint_to_the_next_step` in the same file asserts exactly that (`publish.taint == Tainted`).
- The two tests contradict each other.
- A likely fix: compare only attempts that had started before the resolve (`started_at.is_some()`), or the items and the attempts the resolve itself touches.
- The other five decision/taint combinations pass.

**Deferred / not done:** nothing in the plan's list. `is_widening` for `max_returns` stays epic 4 story 17's (Decision 1; the function does not exist yet).

**Final gate (2026-11-24):** `just check` is red on the one test above, and only on it. Its recipes, run separately after the last change:
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo nextest run --no-fail-fast`: **1281 tests run: 1280 passed, 1 failed (`taint_trusted_input_gate_note::resolving_keeps_every_taint`), 8 skipped**. `just check`'s own nextest is fail-fast, so it stops at that test.
- `just doctest check-deps check-forbid denylist runtime-paths fixed-hosts seam-guard validate`: all green. The am-store doctests are 2 plus 5 (the four on `resolve_inbox_item` with `exec`'s), the am-domain ones 2 plus 3; `check-deps` reports the AD-15 rule.
- The plan's verification set: am-store `inbox_resolve`, `run_journal` and `inbox` 44/44; am-e2e `handoff_gate|inbox_resolve|rule_15|rule_16|taint_` 135/136 (the same test); am-app `gate_cli`, `gate_tty` and `run_cli` 58/58; am-server, am-config and am-engine 362/362; xtask `check_deps` 19/19.

### Test corrections (2026-11-24)

- `taint_trusted_input_gate_note::resolving_keeps_every_taint` (tainted × approve) was mis-specified. It required every row seen at the pause to keep its taint after `continue_run`, but `publish`, queued at the resolve, rightly becomes tainted when it runs on the tainted payload (AD-10, row 29). It now asserts: the resolve changes no taint (every row); after continuing, no taint is lowered (every row), and every item and every attempt that had started by the resolve keeps its committed taint. The final Review-item assertion is unchanged. Green 5/5; `cargo clippy -p am-e2e --tests -- -D warnings` and `cargo fmt --all -- --check` clean.

### Review triage, round 1 (2026-11-24)

Two reviews ran on the working tree after `just check` was green (1281 tests, 8 skipped): a correctness review (C1–C10) and an adversarial security review (A1–A8). Neither reached a non-Human resolve, a taint launder or a bypass of the typed id. Fixes go tests first: the tester adds or corrects tests and logs them red, then the implementer.

| Id | From | Finding | Decision |
|---|---|---|---|
| R1 | C1, A7 | The API's approve and return skip check 5: an approve on a changed config is applied, then no `continue_run` can continue the run (stranded, and reject is no longer possible) | **Fix.** Question 2 (a), owner-accepted, refuses approve and return before writing while the config differs from the pin; it binds every path, not only `agentic gate`. am-app's serve gives am-server the same check `agentic gate` runs (a port in the server state, no new crate edge): pinned hash vs current snapshot, outside change, pending config migration, config error (fails closed). Refusal 409 `config-changed`, naming both hashes. Reject and dismiss still work |
| R2 | C3, A7 | `continue_run`'s refusals (another running step, an unsettled call, an issued tool call) are found only after the resolve committed; an apply error after the commit answers 500 | **Fix.** `agentic gate` and the API run the same read-only checks before writing. If the apply still fails after the commit, the API answers 200 with the resolved item, the run state and `applied: false` (never 500 for a recorded decision), and `agentic gate` exits 1 naming it |
| R3 | C2, A5 | The store's apply never reads `resolved_by`, and the `gate_note` insert skips `gate_note_input`'s checks (row 28 holds on one path only) | **Fix.** The apply is refused unless the item's committed `resolved_by` parses to `Actor::Human`; the `gate_note` row goes through the same checks as `gate_note_input` |
| R4 | C4, A5 | `INSERT OR REPLACE` rewrites a resolved row past `inbox_items_resolved_never_change` (SQLite fires no delete trigger on a REPLACE unless recursive triggers are on) | **Fix.** 0008 adds a `BEFORE INSERT` trigger that aborts when a resolved row has the same `id` or the same `(step_run_id, kind)` |
| R5 | C6 | A forged gate-note resolver finishes the run `failed` with no Failure item; `agentic status` shows no reason | **Fix.** The engine raises one Failure item naming the refusal (engine words, never the note) |
| R6 | C7 | `continue_run` never checks that the plan's workflow is the run's | **Fix.** Refused (`NotContinuable`) before anything is applied |
| R7 | C8, A3 | The payload display neutralises only Cc: model text can print a line that looks like the app's own, push the untrusted banner off screen, or hide or reorder text with bidi, zero-width or tag characters that then reach the next step | **Fix.** Every payload line carries a fixed prefix; characters in Cc (other than newline and tab), Cf, Zl, Zp and the tag block are shown escaped as `\u{XXXX}` with a count line when any; runs of blank lines are collapsed. The note check refuses Cf, Zl and Zp as well (a hand-written table, no new crate) |
| R8 | C9 | A store refusal of a return or an answer leaves `agentic gate`'s exit at 0 | **Fix.** Exit 1 |
| R9 | A1 | Lines typed or pasted ahead (`a⏎a⏎`, or a two-line note) decide the next item before the owner has seen it | **Fix.** Before every prompt `agentic gate` discards input typed ahead (`tcflush(TCIFLUSH)` through a new `Terminal` port method; nix's `term` feature, already used, no new feature); FakeTerminal models type-ahead. After a return the recorded note is printed back, neutralised, so a truncated paste is visible |
| R10 | A2 | Ctrl-C, SIGTERM and SIGHUP cannot stop a continued run (`CancellationToken::new()` is never cancelled); not reachable in the shipped binary until providers are wired | **Fix.** The terminal's interrupt cancels the token passed to `continue_run` |
| R11 | A6 | A partial line followed by end of input (`a`, Ctrl-D twice) is taken as an answer | **Fix.** At every `agentic gate` prompt, end of input after a partial line writes nothing |
| R12 | A4 | The check-deps rule matches `HumanActor::new` line by line: a line break, a `use … as` alias, `type H = HumanActor` or `<HumanActor>::new` pass; the `#[cfg(test)]` exemption applies to every crate | **Fix.** Whole-file token matching: the token `HumanActor` in shipped `src/` only in am-domain (definition), am-store (the resolve), `crates/am-server/src/inbox.rs` and `crates/am-app/src/gate.rs`; `resolve_inbox_item` likewise; the test exemption for am-store only |
| R13 | C10 | Tests that could pass for the wrong reason: an `if let Ok(..)` guard in `taint_trusted_input_gate_note.rs`; no positive control in `a_gate_note_input_needs_a_human_return`; no apply-side forged-resolver test | **Fix** (tests only) |
| D1 | C5 | `inbox.updated` comes from 3.2's once-a-second watcher, so two changes inside one second publish one event | **Accept.** 3.2's watcher design is owner-accepted; an event carries the latest status and a client reads the item. Row 39 reads "one event per observed change" |
| D2 | A5 | No `BEFORE DELETE` trigger on `inbox_items` or `step_inputs` | **Accept.** AD-25: deleting a row or run really deletes its content, so a delete block would conflict with run deletion; a raw-SQL delete by the same user is AD-13's accepted residual; R4 covers the one-statement rewrite |
| D3 | A5 | The apply does not require a matching `inbox.resolved` audit entry | **Accept.** Same-user raw SQL (AD-13 residual); the chain verifier detects a forged audit entry; R3 adds the Human check |
| D4 | A4 | Building the crate-private `Command::Resolve` elsewhere inside am-store is not covered by check-deps | **Accept.** am-store is the writer, the trust boundary itself; the writer's own Human check (row 20) holds for every path |
| Q1 | A8 | `inbox.resolved` stores the note's sha256; a short note can be recovered from an exported audit log by guessing | **Open question for the owner and the architect.** AD-25 prescribes hashes in audit entries, so 3.4 keeps it; whether owner-text hashes need a keyed hash (or none) is an AD-25 question, to settle before the audit export ships |

### Red run, round 1 (2026-11-24)

Shared `CARGO_TARGET_DIR`, this checkout only, no `cargo clean`; no `src/` file changed by the tester. Commands: `cargo nextest run -p am-store --test inbox_resolve --no-fail-fast`; `cargo nextest run -p am-e2e --test <target> --no-fail-fast` for `taint_trusted_input_gate_note`, `rule_15_inbox`, `handoff_gate_resolve`, `inbox_resolve_human_only`; `cargo nextest run -p am-app --test gate_cli --no-fail-fast` and `--test gate_tty`; `cargo test -p xtask --test check_deps`; `cargo clippy -- -D warnings` on each target that builds (clean); `cargo fmt --all -- --check` (clean); `cargo xtask denylist` (clean).

Where a target does not build yet, its new tests were also run once through a temporary probe (a copy of the file with the missing items stubbed or left out, deleted afterwards; also clippy-clean), so each red below is observed, not assumed.

| Row | Test (file) | Red reason |
|---|---|---|
| R1 | `an_api_resolve_on_a_config_other_than_the_pin_is_refused`, `with_no_pin_check_approve_and_return_fail_closed` (`rule_15_inbox`) | compile: no `am_server::PinCheck` (the shared rig, so `handoff_gate_resolve` and `inbox_resolve_human_only` do not build either); probe: an approve on a changed config is `200 OK`, not 409 `config-changed` (both tests) |
| R1 | `the_servers_pin_check_is_agentic_gates_check_5` (`gate_cli`) | compile: no `am_app::serving::pin_check` |
| R2 | `an_api_resolve_whose_run_cannot_go_on_is_refused_before_writing` (`rule_15_inbox`) | probe: `200 OK` for each of the three setups (another step running, an unsettled call, an issued tool call) |
| R2 | `an_apply_that_fails_after_the_resolve_answers_200_unapplied` (`rule_15_inbox`) | compile: no `ResolutionDto.applied`; probe: `500 Internal Server Error` (a raw-SQL trigger aborts the gate's finish after the resolve committed; the honest way found to fail the apply after the commit) |
| R2 | `a_decision_whose_run_cannot_go_on_is_refused_before_writing` (`gate_cli`) | probe: the item is `resolved` (approve recorded, then `continue_run` refuses) |
| R3 | `apply_needs_a_person_as_the_committed_resolver` (`inbox_resolve`) | assertion "Approve resolved by System { component: "engine" }": the apply succeeds |
| R4 | `insert_or_replace_never_rewrites_a_resolved_item` (`inbox_resolve`) | assertion "a trigger aborts INSERT OR REPLACE by its id" |
| R5, R13 | `a_forged_non_human_resolver_starts_no_attempt` (`taint_trusted_input_gate_note`), the `if let Ok` guard replaced by an explicit `failed` outcome | assertion "one Failure item": none (the explicit outcome checks before it pass) |
| R6 | `continue_refuses_a_plan_for_another_workflow` (`handoff_gate_resolve`) | compile (the shared rig, R1); probe: `Ok(RunOutcome { status: "succeeded", .. })` |
| R7 | `every_payload_line_is_shown_behind_a_fixed_prefix`, `invisible_and_bidi_characters_are_shown_escaped_with_a_count`, `blank_runs_collapse_and_newline_and_tab_are_kept` (`gate_cli`) | probe: "`  \| first line` is a line of"; "\u{202E} is never shown raw"; "no line `  \| top`" |
| R7 | `a_note_with_an_invisible_or_separator_character_is_refused` (`inbox_resolve`) | assertion "Return with U+2028 LINE SEPARATOR": accepted |
| R8 | `a_note_the_store_refuses_ends_the_command_with_exit_one` (`gate_cli`) | probe: exit 0 |
| R9 | `lines_typed_ahead_never_answer_a_prompt` (`gate_cli`) | compile: no `FakeTerminal::type_ahead`, `FakeTerminal::discards` |
| R10 | `an_interrupt_while_the_run_goes_on_cancels_it` (`gate_cli`) | probe: "the interrupt stops the run; without it the model call never ends: Elapsed(())" after 30 s |
| R11 | `a_partial_line_then_the_end_of_input_writes_nothing` (`gate_cli`) | compile: no `FakeTerminal::unfinished` |
| R11 | `on_a_terminal_a_partial_line_then_the_end_of_input_writes_nothing` (`gate_tty`, macOS) | assertion: `EXIT=0`; the transcript shows `reject^D^D` then `run … failed`, "rejected at step `approve` by You (Reviewer)": the partial line was taken as an answer |
| R12 | `the_human_actor_token_is_matched_in_whole_files` (`check_deps`) | assertion "a constructor split over lines: must fail" (exit 0; the allowed sites pass). The other four planted forms come after it in the loop and also pass today's line-based rule |
| R13 | `a_gate_note_input_needs_a_human_return` (`inbox_resolve`), positive control added | green on arrival (a control: `StartAttempt` with no forged input starts the attempt with the apply's one `gate_note` row) |

Other targets stay as they were: `inbox_resolve` 13 green, plus 3 red (R3, R4, R7); `gate_tty` 5 green, plus 1 red (R11); `check_deps` 19 green, plus 1 red (R12); `taint_trusted_input_gate_note` 4 green, plus 1 red (R5). The rig change (`PINNED` on the canary test's raw run; `applied` asserted in `the_same_resolve_over_the_api_is_audited_you_reviewer`) passed in the probe apart from `applied`. Not done: D1–D4 and Q1, as triaged.

### Implementation, round 1 (2026-11-24)

Every **Fix** row, R1–R12, is built as the API notes pin it. Shared `CARGO_TARGET_DIR`, this checkout only; no `cargo clean`, no new crate, dependency or feature (nix's `tcflush` is in the `term` feature already enabled; `docs/crate-notes/nix.md` records it). No test file was edited in this round.

- **R1:**
  - `am_server::PinCheck` (defined in `inbox.rs`, exported at the crate root) and `AppStateBuilder::pin_check`.
  - The resolve route refuses an approve or a return of a Review 409 `config-changed`, before anything is written, when the state has no pin check (fails closed), the check returns `Err` (its words), or its hash is not the run's pin (both hashes in full in the message).
  - `am_app::serving::pin_check(places, root, store)` wraps `serving::current_snapshot`: the outside-change check (`run::no_outside_change_in`, which takes any `ConfigHistory`) plus `run::snapshot_of` (pending migration, config error).
  - `agentic gate`'s check 5 now calls the same `current_snapshot`, then its own owner-child and hash checks and `plan_of`.
  - `serve` hands the check to the server; `release_project_writer` drops it with the writer at shutdown (it holds a History handle on the store).
- **R2:**
  - `am_engine::run::ready_to_go_on(reader, run_id) -> Result<Option<String>, RunError>` is the read-only half of `continue_run`, which now calls it.
  - The API (409 `not-continuable`, after the pin check) and `agentic gate` (inside check 5, exit 1) call it before writing an approve or a return.
  - `ResolutionDto.applied`: an apply error after the commit is logged and answers 200 with `applied: false`.
- **R3:** `ApplyResolution` is refused unless `InboxItemRow::resolver()` is `Actor::Human`. The apply's `gate_note` row goes through `gate_note_input` on the new attempt before it is inserted.
- **R4:** `inbox_items_resolved_never_replaced`, a `BEFORE INSERT` trigger, added to the unshipped `0008` in place.
- **R5:** a new System-only command, `WriteCommand::FailBeforeStart { step_run_id, item }`.
  - The attempt must be `queued` and the run open, and stays `queued`. The Failure item is created on it and the run finishes `failed`, audited `inbox.created` and `run.finished`.
  - The engine sends it when a gate note is refused. Title "Step <key> did not start"; the body names the step, the refusal words (with the forged item's id) and the step run, never the note. The outcome's `item` is that item.
- **R6:** `continue_run` refuses `plan.workflow != run.workflow` (`NotContinuable`) before the hash check and before any apply.
- **R7:**
  - `am_domain::text` holds the one hand-written table: `is_hidden` covers Cc but newline and tab, the Cf ranges of Unicode 16, Zl, Zp and the whole tag block. `escape_hidden` writes `\u{XXXX}`, uppercase, at least four digits.
  - The store's note check refuses a newline or any hidden character.
  - `agentic gate`'s `payload_block` prefixes every line with `"  | "`, escapes, collapses runs of blank lines (a line that is empty after `trim`), and adds the count line after the end marker. The echo of an unknown choice and the note shown back use the same escape.
- **R8:** a return note or answer the store refuses writes the store's words to stderr and ends `agentic gate` with exit 1.
- **R9:**
  - `Terminal::discard_typed_ahead` is called in `ask` before every prompt.
  - `OwnerTerminal` drops the bytes its reader thread already took and the bytes not yet read (`tcflush(TCIFLUSH)`; ignored when stdin is no terminal); a pending interrupt is kept.
  - `FakeTerminal` gains `type_ahead`, `discards` and `unfinished`.
  - After a return, the recorded note is shown back behind the prefix.
- **R10:**
  - `agentic gate` runs `continue_run` under `tokio::select!` with a 100 ms poll of `Terminal::interrupted()` (Ctrl-C; the real terminal's SIGTERM, SIGHUP and SIGQUIT arrive the same way).
  - An interrupt cancels the token passed in. The step ends as 3.3's cancellation ends it, the run fails, and the command exits 1 with words on stderr.
- **R11:** the port's contract is now that `read_line` returns only a line ended by Return. `OwnerTerminal::line` returns `Closed` (and wipes the partial line) for a partial line followed by the end of input. `agentic accounts`' one `read_line` (its typed id) gets the same, safer, rule; every `accounts_tty` input ends with Return, so its tests are unaffected.
- **R12:** `check-deps` matches the tokens `HumanActor` and `resolve_inbox_item` word by word over each whole file's code (comments and literals stripped).
  - Allowed: every file of `crates/am-domain/src/`, `crates/am-store/src/lib.rs`, the two handlers, and `#[cfg(test)]` items under `crates/am-store/src/` only.
  - One finding per line names `<file>:<line>`, the token(s), `HumanActor::new` and AD-15, and never an allowed file's path.

**Choices left open:**
- the R2 refusal happens after the R1 pin check, so a changed config reads as `config-changed` first;
- `applied` is `true` for a Question's answer and a Failure's dismiss, which have nothing to apply;
- a blank line is one that is empty after `trim`;
- the count line follows the end-of-output line, unprefixed: it is the app's own line;
- the R5 Failure item has no payload (the queued attempt has no output).

**Mechanical test edits:** none this round.

**Final gate, round 1 (2026-11-24):** `just check` green (exit 0): fmt, clippy `-D warnings`, **`cargo nextest run`: 1303 tests run: 1303 passed, 8 skipped**, doctests (am-store 2 + 5, am-domain 2 + 3), check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard, validate. Verification set: am-store `inbox_resolve`, `run_journal` and `inbox` 47/47; am-e2e `handoff_gate|inbox_resolve|rule_15|rule_16|taint_` 141/141; am-app `gate_cli`, `gate_tty` and `run_cli` 68/68; am-server, am-config and am-engine green; xtask `check_deps` 20/20.

### Review triage, round 2 (2026-11-24)

A follow-up review checked round 1 (`just check` green: 1303 tests, 8 skipped). R1, R3, R5, R6, R8, R9 and R11 landed. The rows below close the holes it found (V1–V6) and are fixed tests first. `FailBeforeStart` and `release_project_writer` were found sound; R11's `read_line` change reaches three `agentic accounts` reads (`accounts.rs`, `change.rs`, `migrate.rs`), not one, and only narrows what they accept.

| Id | From | Finding | Decision |
|---|---|---|---|
| S1 | V1 (R10) | After an interrupt cancels a continued run, `agentic gate` goes on to the next queued item (including a Review the run raised before the cancel) and holds the lock for another prompt | **Fix.** An interrupt ends the command: report the run's outcome, then exit 1 with nothing more shown or read; a two-item test |
| S2 | V2 (R4) | The REPLACE guard fires only over a resolved row; `INSERT OR REPLACE` over an open row fires no UPDATE trigger, so it can lower an open Review's taint and rewrite its words | **Fix.** The `BEFORE INSERT` trigger aborts whenever a row with the same `id` or the same `(step_run_id, kind)` exists, whatever its status |
| S3 | V3 (R7) | Long payload lines soft-wrap; the continuation starts at column 0 without the prefix, so padding can place model text where the app's own lines go | **Fix.** Payload lines are hard-wrapped at 72 columns (tabs expanded to 4; common wide ranges count 2), each segment prefixed. A terminal narrower than 76 columns can still wrap a prefixed segment (residual, recorded) |
| S4 | V4 (R7) | The hidden-character table misses default-ignorable code points that are not Cf (variation selectors, U+034F, the Hangul fillers, U+17B4–17B5, U+180B–180F) | **Fix.** The table is Cc (except newline and tab) ∪ Zl ∪ Zp ∪ Unicode's Default_Ignorable_Code_Point, in the one shared place; display and note check both use it |
| S5 | V5 (R12) | check-deps allows every am-domain file and both handler files whole, so a `pub use … HumanActor as X` re-export there lets another crate call `X::new` | **Fix.** In am-domain only `actor.rs` (the definition) and `lib.rs`'s re-export line; in any allowed file an `as` alias or a `pub use` of either token fails the check |
| S6 | V6 (R2) | After the resolve commits, a read error or a panic in the blocking task still answers 500 | **Fix.** Once the resolve is recorded the API answers 200 with `applied: false` whatever fails after it (the item id and `resolved` when the item cannot be read back) |
| S7 | V (a) | `agentic gate` prints item titles and bodies raw; a Failure body can embed a resolver read from the row | **Fix.** Titles and bodies are shown through the same neutralising display as payloads |
| D5 | V1 | After the interrupt, further signals are not acted on until the provider honours the cancelled token or the step's wall-clock cap passes | **Accept.** The token is cancelled at the first signal and the cap bounds the wait; crash-safe stop is 3.11's and resume 3.9's |

### Red run, round 2 (2026-11-24)

Shared `CARGO_TARGET_DIR`, this checkout only, no `cargo clean`; the tester changed no `src/` file (am-testkit included). Commands: `cargo nextest run -p am-store --test inbox_resolve --no-fail-fast`; `cargo nextest run -p am-app --test gate_cli --no-fail-fast`; `cargo nextest run -p am-e2e --test rule_15_inbox --no-fail-fast`; `cargo test -p xtask --test check_deps`; `cargo clippy -- -D warnings` on those four targets (clean); `cargo fmt --all -- --check` (clean); `cargo xtask denylist` (clean). Every target builds (the round-1 API exists), so each red is an assertion.

| Row | Test (file) | Red reason |
|---|---|---|
| S1 | `an_interrupt_ends_the_command_before_the_next_item` (`gate_cli`) | "the second item is never shown": after the cancelled run the command shows the next Review (the exit 1 and the `run <id> …` line before it pass) |
| S2 | `insert_or_replace_never_rewrites_an_open_item` (`inbox_resolve`) | "a trigger aborts INSERT OR REPLACE by its id": the open, tainted Review is rewritten |
| S3 | `long_payload_lines_are_cut_into_prefixed_segments` (`gate_cli`) | "200 columns cut at 72": the line is shown whole |
| S3 | `blank_runs_collapse_and_newline_and_tab_are_kept` (`gate_cli`), corrected: a tab is now expected as 4 spaces | "no line `  \| middle    column`": the tab is kept |
| S4 | `default_ignorable_characters_are_shown_escaped` (`gate_cli`) | "\u{FE0F} is never shown raw" |
| S4 | `a_note_with_a_default_ignorable_character_is_refused` (`inbox_resolve`) | "Return with U+FE0F VARIATION SELECTOR-16": accepted |
| S5 | `an_alias_or_a_re_export_of_human_actor_fails_even_in_an_allowed_file` (`check_deps`) | "a re-export in another am-domain file: must fail" (exit 0). The allowed half, each handler with its plain import, passes. The other three cases follow in the loop, and each is in a file today's rule allows whole |
| S6 | `a_resolve_whose_item_cannot_be_read_back_answers_200_unapplied` (`rule_15_inbox`) | "never 500 for a recorded decision: HTTP/1.1 500". A probe (a temporary test, deleted) confirmed the decision had committed (`("resolved", Some("not-a-decision"))`) before the 500. The read-back is made to fail honestly: raw SQL drops `inbox_items_resolved_never_change` and plants an `AFTER UPDATE` trigger that rewrites the committed decision to a spelling no reader knows. A panic in the blocking task is not reproduced |
| S7 | `titles_and_bodies_are_shown_escaped` (`gate_cli`) | "\u{001B} is never shown raw": the Failure item's title and body are printed raw |

**Round-1 tests corrected for S3** (meaning kept):
- `every_payload_line_is_shown_behind_a_fixed_prefix`: the forged prompt line is shortened to 61 columns, so it stays one segment.
- `invisible_and_bidi_characters_are_shown_escaped_with_a_count`: the payload is one short line per character.
- `blank_runs_collapse_and_newline_and_tab_are_kept`: see the table.
- `check_deps::the_human_actor_token_is_matched_in_whole_files`: its two handler files now carry their real import line (`HANDLER_FILE`).

The first two and the last stay green.

Totals: `gate_cli` 29 green and 5 red; `inbox_resolve` 16 green and 2 red; `rule_15_inbox` 14 green and 1 red; `check_deps` 20 green and 1 red. D5 is accepted, so it has no test.

### Implementation, round 2 (2026-11-24)

S1–S7 are fixed; D5 is left as accepted. The work stayed in this checkout, with the shared `CARGO_TARGET_DIR`, no `cargo clean`, and no new dependency or feature. No test file was edited in this round.

- **S1:** In `Gate::go_on`, a Ctrl-C reported while the continued run works now ends the command after the run's outcome is reported (the `run <id> <status>` line and its reason). It returns exit 1. A Review the run raised before the cancel is not queued, and nothing more is shown or read. The open project then drops, which removes the marker and releases the lock.
- **S2:** `0008`'s REPLACE guard is now `inbox_items_never_replaced`; it replaces round 1's `inbox_items_resolved_never_replaced`. Its `BEFORE INSERT` aborts whenever a row with the same `id` or the same `(step_run_id, kind)` exists, open or resolved. It is created after the copy and the rename, so the copy step never meets it.
- **S3:** `agentic gate`'s `segmented` turns each payload line into pieces:
  - a hidden character becomes one `\u{XXXX}` piece, whose width is its length;
  - a tab becomes four 1-column spaces;
  - a character in the listed wide ranges counts 2 columns, any other 1.

  Pieces are then packed greedily into segments of at most 72 columns, and a segment ends before the piece that would pass 72. Every segment goes behind `"  | "`. A run of blank lines (empty after `trim`) still collapses to one.

  `payload_block` adds the end line and the count line. The note shown back after a return uses the same segments.

  Residual: a terminal narrower than 76 columns still soft-wraps a segment.
- **S4:** `am_domain::text::is_hidden` now covers Cc (but newline and tab), Zl, Zp and Default_Ignorable_Code_Point. The last is a hand-written range table copied from `DerivedCoreProperties.txt`, Unicode 16.0.0; the source and version are cited in the file, and adjacent ranges are merged.

  The Cf characters that are not default-ignorable (U+0600–0605, U+06DD, U+070F, U+0890–0891, U+08E2, U+110BD, U+110CD, U+13430–1343F, U+FFF9–FFFB) are no longer in the set, as the row defines it. The display and the store's note check share the table.
- **S5:** check-deps' resolve rule is now a per-file pass (`resolve_findings`) over each file's code, with comments and literals stripped.
  - The tokens `HumanActor` and `resolve_inbox_item` are allowed in:
    - am-domain's `actor.rs`;
    - am-domain's `lib.rs`, inside a plain `pub use`;
    - am-store's `lib.rs`;
    - the two handlers;
    - `#[cfg(test)]` items of am-store.
  - In any file, a token followed by `as`, or inside a `pub use …;` statement (`pub(…) use` included), is a finding. The one exception is am-domain `lib.rs`'s plain re-export.
  - Findings come one per line, with `<file>:<line>`, `HumanActor` and AD-15. They never name an allowed file unless the finding is in it.
- **S6:** The resolve handler reads the run before writing. Once the resolve has committed, it stores a fallback `ResolutionDto` in a shared slot: the item as read before, with status `resolved`, the decision and "You (Reviewer)"; the run as read before; and `applied: false`.
  - If the read-back fails, a row is missing, or the blocking task panics (`JoinError`), the API answers 200 with that fallback.
  - If the apply fails but the read-back works, the API answers with the read-back and `applied: false`.
- **S7:** An item's title and body are shown through `one_line`: the payload display's escapes, plus newline and tab as `\u{000A}` and `\u{0009}`, so each stays on one line. They are not prefixed or cut, so row 1's check that the body is shown whole still holds.

**Mechanical test edits:** none this round. **Choices left open:**
- An escape's width is its length.
- In S7, newline and tab are escaped in titles and bodies.
- In S6, the fallback's run is the one read before the resolve.

**Final gate, round 2 (2026-11-24):** `just check` green (exit 0): fmt, clippy `-D warnings`, **`cargo nextest run`: 1312 tests run: 1312 passed, 8 skipped**, doctests (am-store 2 + 5, am-domain 2 + 3), check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard, validate. Verification set: am-store `inbox_resolve`, `run_journal` and `inbox` 49/49; am-e2e `handoff_gate|inbox_resolve|rule_15|rule_16|taint_` 142/142; am-app `gate_cli`, `gate_tty` and `run_cli` 72/72; am-server, am-config and am-engine 362/362; xtask `check_deps` 21/21.

## Auto Run Result

**Status:** built (2026-11-24).

**Summary:** an Inbox item is resolved only by a person, and a run waiting at a gate goes on.
- **Human-only resolve:** `Writer::resolve_inbox_item(by: HumanActor, …)`; a non-Human call does not compile (four doctests, each paired with an allowed build), the writer refuses any other actor (src unit test), no `WriteCommand` resolves, and `cargo xtask check-deps` allows `HumanActor` and `resolve_inbox_item` only at their definitions (am-domain `actor.rs` and `lib.rs`'s plain re-export, am-store `lib.rs`) and in the two handlers (`crates/am-server/src/inbox.rs`, `crates/am-app/src/gate.rs`), with whole-file token matching and no alias or re-export.
- **Two transactions:** the Human records the decision and takes AD-8's `waiting_human → running`, audited "You (Reviewer)"; the engine applies it as `System (engine)` from committed rows (approve → gate `succeeded`; return → producer and gate requeued with a `gate_note` input, past `max_returns` the gate fails with one Failure item; reject → gate and run `failed`). The writer refuses an apply on an open item, a non-Human `resolved_by`, a transition that does not match the decision, and any other move of a resolved gate.
- **Return note:** stored once on the item (the audit holds its sha256); the producer's new attempt receives it as `Part::Trusted` built by `TrustedInput::from_owner_note` from the committed resolver (the architect confirmed it fits AD-10 (b) as written); it never adds or clears taint; a forged resolver starts no attempt and raises a Failure item.
- **`agentic gate`:** interactive terminal only (stdin and stdout), refused while a stdio MCP server or `render: true` Source is enabled or `agentic serve` holds the project; approve and return refused while the config differs from the run's pin; typed decisions, `reject` confirmed by typing it, no approve flag or default; a tainted Review says its output came from untrusted content and approving it needs the item's id typed; payload lines behind a fixed `  | ` prefix, cut at 72 columns, hidden and control characters (Cc, Zl, Zp, Default_Ignorable_Code_Point) shown as `\u{XXXX}`; input typed ahead is discarded before every prompt; Ctrl-C, end of input (also after a partial line) or 10 minutes idle writes nothing; an interrupt during a continued run cancels it and ends the command.
- **API:** `GET /api/v1/inbox`, `GET /api/v1/inbox/{id}` (CLI class) and `POST /api/v1/inbox/{id}/resolve` (UI class, owner-approved deviation from the ticket's "CLI class"); the CLI token alone resolves nothing; a tainted approve needs `confirmItemId`; approve and return are refused on a changed config (409 `config-changed`) or a run that cannot go on (409 `not-continuable`) before writing; a recorded decision is never answered 500 (`applied: false`); `inbox.updated` carries exactly `{id, status}`.
- **Migration 0008:** rebuilds `inbox_items` with `open|resolved`, `decision`, `note`, `max_returns`, resolver and time, behind AD-7's pre-migration copy; no item lost, the copy kept; triggers: taint only rises, a resolved row never changes, an item is never replaced (`INSERT OR REPLACE`), fixed fields never change; `step_inputs.from_item` added.

**Tests:** about 100 new or extended tests across am-store, am-e2e (`handoff_gate_resolve`, `inbox_resolve_human_only`, `rule_15_inbox`, `taint_trusted_input_gate_note`, `rule_16_audit_log`), am-app (`gate_cli` in process, `gate_tty` on a real pseudo-terminal), am-config, am-server and xtask, every one written and seen red first (red runs logged for the build and both patch rounds). `just check`: 1312 passed, 8 skipped.

**Review:** a correctness and an adversarial security review, then a follow-up review of the fixes. None reached a non-Human resolve, a taint launder or a bypass of the typed id. Round 1 fixed R1–R13, round 2 fixed S1–S7, all tests first; accepted D1–D5 with reasons above.

**For the owner:**
- **Decided (Q1, owner 2026-11-24; was an open question, not blocking):** the `inbox.resolved` audit entry holds the return note's sha256, as AD-25 prescribes hashes; a short note can be guessed from an exported audit log. Whether owner text needs a keyed hash (or none) is an AD-25 question to settle before the audit export ships.
  - Decided 2026-11-24 (owner): audit hashes of free text become keyed, HMAC-SHA-256 under an `audit-hash-v1` subkey derived from the project's backup key, so `note_sha256` becomes `note_hmac` with a key id (arch AD-32; epic 3 and epic 9 Notes); epic 9 story 10 builds it, and this story is not reopened.
- **Additions beyond the plan:** the System-only `WriteCommand::FailBeforeStart` (a Failure item on an attempt that never started, for a forged note); the writer also refuses starting, pausing or finishing anything in a run whose decision is recorded but not applied; reject is applied without continuing the run, so it works after a config change; serve releases the store and the pin check when it stops; end of input after a partial line now writes nothing in `agentic accounts`' three reads too (only narrower).
- **Residuals:** a terminal narrower than 76 columns can still soft-wrap a prefixed payload segment; after the first interrupt, further signals wait for the cancelled step to stop (bounded by its wall-clock cap); a run left ready by an API resolve, or stopped between resolve and apply, waits for story 3.9's resume; raw SQL by the same user remains AD-13's accepted residual.
- **Interim risk carried from 3.3:** until epic 4 story 17, a hand edit can weaken a gate unchecked; `max_returns` raising a cap is also 4.17's to classify.

## Post-merge fix: second-resolve race (2026-11-25)

**The bug:** `POST /api/v1/inbox/{id}/resolve` (`crates/am-server/src/inbox.rs`, `resolve`) reads the item, then, in `check`, the run's steps in a separate read. A first resolve that commits between the two makes the second request see the item still open but its step already `running` (or moved on by the apply), and answer 409 `not-waiting` instead of 409 `item-resolved`. `rule_15_inbox::a_second_resolve_is_refused` flaked on this on macOS CI (PR #38 push runs).

**Owner (2026-11-25):** fix it in its own small PR, tests first; read the item and the step status in one transaction rather than re-reading; a deterministic test for the stale read, and a test that under any interleaving exactly one resolution is applied (two applied would be a security bug: stop and report); no change to who can resolve, the typed id or the config check.

### API the tests fix (tester, 2026-11-25)

- **One test-only seam in `crates/am-server/src/inbox.rs`.** It is the only way to put another request's write between the item read and the later pre-check reads without timing. `resolve`'s blocking work calls the test module's hook right after it first reads the item, inside the read transaction its pre-checks share:
  ```rust
  #[cfg(test)]
  tests::after_item_read(&id);
  ```
  The hook, its registry and every test using it are in the file's `#[cfg(test)] mod tests` (written by the tester); the implementer adds only that call, which compiles away outside tests. Until it is added, `cargo clippy -p am-server --lib --tests -- -D warnings` reports `after_item_read` as never used.
- **Outcomes, not mechanism:** the tests pin answers and rows, not how the one read transaction is built. Whatever the fix, a request that read the item open while another request's decision was written answers 409 `item-resolved` (never `not-waiting`, `not-continuable`, or a 400 from a read left inside a stale snapshot after its own write was refused), and exactly one decision is recorded and applied.
- **Unchanged:** who can resolve (`HumanActor`, `resolve_inbox_item`), the typed `confirmItemId`, the pin check, the envelope codes and `a_second_resolve_is_refused`.
- **Test support:** `crates/am-e2e/tests/trust/support/inbox.rs` gains `with_pin_check(Option<Arc<dyn PinCheck>>) -> Rig`; `with_pin` now delegates to it (no behaviour change).

**Tests added** (all test code; no `src/` change outside the listed `#[cfg(test)]` module):

| File | Test | What it pins |
|---|---|---|
| `crates/am-server/src/inbox.rs`, `#[cfg(test)] mod tests` (test code inside a src file) | `a_resolve_that_read_the_item_before_another_committed_answers_item_resolved` | The stale read, deterministic: B reads the item open; at the seam, A's decision is written (recorded only, or recorded and applied); B goes on. B is 409 `item-resolved`, never `not-waiting`; A's decision alone is recorded and applied once. Pairs (A, B): approve/reject, reject/approve, approve/return, return/approve, approve/approve; 10 cases. |
| same | `two_resolves_that_both_read_the_item_open_record_one_decision` | Both requests read the item open (they meet at the seam), then go on at once: one 200, one 409 `item-resolved`; one decision, the winner's, applied once. approve/reject, approve/return, approve/approve; 10 rounds each. |
| `crates/am-store/tests/inbox_resolve.rs` | `two_resolves_in_any_order_record_and_apply_one_decision` | At the writer, every order of two sides' resolve and apply (6 orders, each side's resolve before its apply; every side tries its apply, even after losing) for 6 decision pairs (36 cases): one resolve succeeds, one `inbox.resolved` entry, one apply succeeds, the gate moved once (one `step.finished` or `step.looped_back` on it) to the winner's result, no other item, one gate note for a return. |
| `crates/am-e2e/tests/trust/rule_15_inbox.rs` | `a_resolve_held_after_its_checks_loses_to_one_that_committed` | Over HTTP: request H passes every check before the pin check and is held there (`HeldPin`, on the existing `PinCheck` port); request O resolves fully; H goes on: O 200, H 409 `item-resolved`, O's decision recorded and applied once. approve held vs reject, return, approve; return held vs approve. |
| same | `two_resolves_held_after_their_checks_record_one_decision` | Both requests held at the pin check, released together: one 200, one 409 `item-resolved`, one decision applied once. approve/approve, approve/return, return/return; 5 rounds each. |
| same | `concurrent_resolves_over_http_record_one_decision_every_round` | Stress: 200 rounds of two resolves sent at once (`tokio::join!`, multi-threaded runtime), approve vs reject, return and approve in turn. For every round, first the security property (one `inbox.resolved`, one gate move, the 200's decision; findings marked SECURITY), then the answers (one 200, one 409 `item-resolved`); every failing round is listed. |

### Red run

Shared `CARGO_TARGET_DIR` (`/Users/hiteshmamgain/cargo-target/agentic-manager`); no `cargo clean`, no copy of the workspace, no new dependency. `git diff` of `crates/am-server/src/inbox.rs` is one hunk after line 379, the `#[cfg(test)]` module; no other `src/` file changed. `cargo fmt --all` run.

Commands:
- `cargo nextest run -p am-server --lib --no-fail-fast`
- `cargo nextest run -p am-store --test inbox_resolve --no-fail-fast`
- `cargo nextest run -p am-e2e --test rule_15_inbox --no-fail-fast`
- under load: the `rule_15_inbox` test binary run 8 times at once, `<binary> concurrent_resolves_over_http_record_one_decision_every_round --exact --test-threads 1`; and 8 × 3 runs at once of `<binary> held_after_their_checks held_after_its_checks a_second_resolve --test-threads 4`
- `cargo clippy -p am-e2e --test rule_15_inbox -- -D warnings`, `cargo clippy -p am-store --test inbox_resolve -- -D warnings`, `cargo clippy -p am-server --lib --tests -- -D warnings`
- `cargo xtask denylist`, `check-deps`, `check-forbid`, `seam-guard`, `runtime-paths`, `fixed-hosts`: all clean.

| Test | Observed |
|---|---|
| am-server `a_resolve_that_read_the_item_before_another_committed_answers_item_resolved` | **Red**, assertion: `the seam is missing: resolve must call tests::after_item_read(&id) …` (left 0, right 1; first case "A approve (recorded), then B reject"). The hook never runs, so B resolves unopposed. |
| am-server `two_resolves_that_both_read_the_item_open_record_one_decision` | **Red**, the same seam assertion (left 0, right 2; "approve and reject, round 0"). |
| am-server, clippy `-D warnings` | **Red**: `function after_item_read is never used` (the seam's call is missing). |
| Probe (a temporary test in the same module, run once and removed): today's `check`, given the item row read before A's write, after A's write | **409 `not-waiting`** in all 10 cases (each pair above, A recorded or recorded and applied): the bug, reproduced without timing. A second temporary probe sent B after A with no hook: 409 `item-resolved` and the one-resolution checks held in all 10 cases, so the rig and the checks are right. |
| am-store `two_resolves_in_any_order_record_and_apply_one_decision` | Green, 36 of 36: the writer refuses the second resolve inside its own transaction, and the second apply once the gate has left `running`. Guard. |
| am-e2e `a_resolve_held_after_its_checks_loses_to_one_that_committed` | Green: a write landing after the pre-checks is already answered `item-resolved` (the writer refuses, the handler reads the item back). Guard. Green in all 24 runs under load. |
| am-e2e `two_resolves_held_after_their_checks_record_one_decision` | Green, for the same reason. Guard. Green in all 24 runs under load. |
| am-e2e `concurrent_resolves_over_http_record_one_decision_every_round` | Green alone (200 of 200; the race depends on timing). **Red under load in 6 of 8 parallel runs:** 10 of 1,600 rounds answered `409 not-waiting` (8 approve vs reject, 2 approve vs return; the loser was the approve 9 times and the return once), e.g. `round 84 (approve vs reject): 409 not-waiting / 200`. No SECURITY finding in any run. |
| am-e2e `a_second_resolve_is_refused` (unchanged) | Under the same load: red in 2 of 24 runs, `left: "not-waiting"`, `right: "item-resolved"`: the CI flake, reproduced locally. |

**Two resolutions applied?** No ordering observed did. Each item had exactly one `inbox.resolved` entry, one decision and one gate move:
- the 36 writer orders;
- the 4 + 15 cases held at the pin check;
- all 1,800 stress rounds (200 alone, 1,600 under load), the 10 `not-waiting` rounds included.

In the 10 probe cases the stale request was refused before it wrote. The writer serialises resolves and refuses the second in its own transaction, so the race changes only the loser's answer. Not a security bug. The two seam tests are still to be seen green or red past the seam assertion, once the seam's call exists.

### Implementation (2026-11-25)

**Lines changed** (no test file edited; no new dependency; shared `CARGO_TARGET_DIR`, no `cargo clean`):
- `crates/am-store/src/lib.rs` (+23, `impl Reader`, after `open`): `Reader::snapshot<T>(&self, f: impl FnOnce(&Reader) -> Result<T, StoreError>) -> Result<T, StoreError>`. It runs `f` inside one deferred read transaction on the reader's own connection (`Transaction::new_unchecked`, `Deferred`), so every read `f` makes sees the WAL snapshot its first read took. The transaction ends (rolled back; it wrote nothing) when `f` returns, whatever `f` returns. A failure to end it is returned as an error, because a reader left inside it would read stale rows. No raw SQL outside am-store.
- `crates/am-server/src/inbox.rs`, `resolve` (old lines 287-302, now 288-314): the pre-checks run in `reader.snapshot(..)`. These are the item read, the run read (`run_before`), `check` (the step status) and, for a Review's approve or return, `ready_to_go_on` (pin check and `am_engine::run::ready_to_go_on`). The tester's seam `#[cfg(test)] tests::after_item_read(&id);` sits right after the item read, inside that transaction. The transaction ends before `Writer::resolve_inbox_item`. The read-back after a refused write, `apply_resolution` and the final reads all see what committed since. A request that read the item open while another committed therefore gets the writer's refusal and reads the item back `resolved`: 409 `item-resolved`.
- `check`'s doc comment (line 158) now says it checks one committed state.
- Unchanged: `HumanActor` and the writer's resolve, which still refuses a resolved item inside its write transaction; `confirmItemId`; the pin check; the apply; every error code; the order of the refusals.

**Two resolutions applied?** None found. The writer's `Immediate` transaction refuses a second resolve. The tester's 36 writer orders still pass.

**Verification:**
- The two seam tests pass. With the snapshot disabled as a temporary probe (reverted), `a_resolve_that_read_the_item_before_another_committed_answers_item_resolved` fails `not-waiting` again.
- `just check`: green. fmt; clippy `-D warnings`; nextest 1,470 passed, 8 skipped; doctests; check-deps, check-forbid, denylist, runtime-paths, fixed-hosts and seam-guard clean; `agentic validate` 0 errors.
- `cargo test --doc -p am-store -p am-server`: ok.
- Stress, every run with zero `not-waiting` answers and no SECURITY finding:
  - `cargo nextest run -p am-e2e --test rule_15_inbox --stress-count 20`: 20/20.
  - Four of those (`--stress-count 10`) in parallel: 40/40.
  - The tester's load pattern, three times: 8 × `concurrent_resolves_over_http_record_one_decision_every_round` and 8 × the held and second-resolve tests at once. 48/48 runs ok, 4,800 concurrent rounds.
  - `cargo nextest run -p am-server --lib inbox --stress-count 20`: 20/20.
