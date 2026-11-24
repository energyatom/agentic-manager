---
title: 'Labelled prompt assembly and schema repair'
type: feature
ticket: 5
created: '2026-11-24'
status: approved
baseline_revision: 'a7b5049a12f9f9ef1db7b3034bf927c42ae500d3'
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

**Problem:** A step's request is built in three loose places today, and nothing proves what the model was sent.
- `am-app` puts the seat's body in the request's `system` part and the step's instruction in the first user part (`plan_of`, `run.rs` L335-L391). It sends no goal, backstory, skill, reference-file list or saved-link list.
- `am-engine` adds trusted inputs and the fence after the instruction (`run_step`, `run.rs` L1088-L1090). No code fixes the AD-27 order, and nothing stores the prompt: `model_calls` has no prompt columns and no table holds parts, so drill-down (epic 6) has nothing to read.
- A step's `output_schema` is refused by `agentic run` (`not_honoured`, `am-app/src/run.rs` L278). Nothing validates a final message, so AD-36's "one repair, then fail" is not built.
- `jsonschema` is not in the workspace (`docs/crate-notes/jsonschema.md`).

The ticket's verify: a step whose seat has a goal, a backstory, one attached skill with a reference file, a reference file and a saved link, with a CEO goal, a guidance note, an expired note, a memory note, a gate note and an untrusted input, stores a journal prompt with the parts in order, each labelled. The skill reference and the reference file are named, never read. The expired note is absent. The untrusted part is fenced. Goal, backstory and skill are `Instruction`, and no test can build one from `UntrustedData`. FakeLlm's invalid output is repaired once and marked repaired; a second invalid output fails the step with the validation error in words.

**Approach:**
- **One assembler, one order (AD-27):** a new `am-engine::prompt` module takes typed slots from the plan and the attempt's inputs and emits the request parts in AD-27's order. The plan cannot reorder them. It never cuts text.
- **Typed slots, built from config in am-app:** goal, backstory, skills, reference-file names and saved links come from the seat in the config snapshot (all exist since 1.12). CEO goals, guidance notes and memory notes have no source at the baseline, so their slots are typed inputs on the plan that the shipped binary leaves empty (Question 1).
- **The prompt is journaled (AD-27, AD-35):** migration 0009 adds `prompt_parts`, written in the transaction that starts the attempt: slot, type, label, taint, and the text where it has no other home.
- **Schema check (AD-36, AD-3):** `am-engine::schema` validates a step's final message with `jsonschema =0.61.2`, default features off. One repair turn, journaled and marked. A second failure fails the step with a Failure item in words that never echo the model's text.
- **The type wall:** compile-fail doctests, a writer check on slot and type, and one more `check-deps` source rule keep `UntrustedData` away from `Instruction`.

## Boundaries & Constraints

**Always:**
- **The order (AD-27, as amended 2026-11-18):** role instructions, goal, backstory, each attached skill, CEO goals, unexpired guidance notes, memory notes, reference files, saved links, step instruction, trusted inputs, fenced untrusted data. One function builds it. Slots fill in any order and come out in this one.
- **Types (AD-10):**
  - Instruction: role instructions, goal, backstory, skill bodies, CEO goals, guidance notes, memory notes, the reference-file list, the saved-link list, the step instruction. All config- or owner-authored text.
  - TrustedInput: `step:`, `loop-back:` and `gate-note:` parts, through 3.3's and 3.4's constructors only.
  - UntrustedData: plan inputs, and any producer's output whose committed taint is set.
- **Names, never content:**
  - A skill's reference files and the seat's reference files appear by name. Saved links appear by title and URL.
  - No file is opened, fetched or read for the prompt. A skill reference's text is in the snapshot (`Skill.references`); only its key is used.
- **Nothing truncated:** no cut, no summary, no per-part size limit in assembly. The overflow pause is story 3.10.
- **Stored at the start (AD-8):** the assembled parts are journaled in the same transaction as `StartAttempt`. A gate step assembles nothing.
- **One source label per part (AD-27):** the journal holds each part's slot, type, label and taint. An input part's text stays on its producer's row (AD-43: never stored twice).
- **Repair (AD-36, R4):**
  - Only a final message (a turn with no tool call) of a step with a declared schema is checked.
  - At most one repair per attempt, enforced by the writer from the journal.
  - A repair is an ordinary model call: reserved, settled, counted by the iteration cap and the wall clock. No new reservation path.
- **Schema safety (AD-3, AD-12):**
  - `jsonschema` with `default-features = false`: no `resolve-http`, no `resolve-file`.
  - A schema is compiled before any run row is written.
  - A remote or file `$ref`, or a pattern the linear-time engine refuses, refuses the run in words.
- **Taint never launders (AD-10):** a repair neither adds nor clears taint. Assembly changes no step's taint. Journaled prompt text is marked at rest with the attempt's taint.
- **Audit hashes of free text (owner decision 2026-11-24; story 9.10 swaps in a keyed hash):** this story adds no new free-text hash to any audit arg by any other path. A repair call's settle or finish, and the Failure item's step finish, hash model output only through the one existing helper (`audit::sha256_hex`, am-store `audit.rs` L35, as `output_sha256` at `lib.rs` L435, L518, L535, L1207 does), so 9.10 changes one place. Validation words, prompt text and part text are never hashed into audit args; ids, counts and the purpose are used instead. File and config hashes stay bare.
- **Tests first:** every test in the Tests-first list is written and seen red, with its reason logged, before any `src/` change. FakeLlm, FakeClock and FakeEgress only; no network, no real model.

**Never:**
- **No conversion:** no `From`, `TryFrom` or `Into<String>` from `UntrustedData` or `TrustedInput` to `Instruction`, or the other way. The existing three-way wall is kept; none is added.
- **No engine-authored `Instruction`:** am-engine never calls `Instruction::authored`. The repair instruction is built in am-app from the schema file, which is config.
- **No model words in a prompt as instructions:** the validation error's text is for the Failure item and the run's reason. It is never put into any prompt.
- **No content of a reference file, a skill reference, a saved link's page or a linked folder** in a prompt, a journal row, an audit entry or an event.
- **No new config field and no new store table for CEO goals, guidance notes or memory notes.** Another story owns each (Question 1).
- **No second HTTP client and no file read for a schema:** `jsonschema`'s retrievers stay off; nothing in the validator can fetch.
- **No reading of `read_skill_file`'s open question into this story (AD-38):** 3.5 builds no tool result and no part labelled `skill:<id>/<file>`. It neither relies on nor settles that question, which must be settled before 9.16.
- **Outside this story:** the overflow pause (3.10), fan-in and hand-off parts `branch:` and `handoff:` (3.14, 3.15), file parts and the folder fence labels (epic 12), per-call prompt text of later turns and tool results (6.x reads the tool-call rows), cost amounts (3.7), the fallback chain (3.8), resume of a half-assembled prompt (3.9), the drill-down UI (epic 6), the config fields and editors for CEO goals, guidance and memory notes (5.8, 5.3, 10.2, 10.4).
- **No department vocabulary in `crates/`.** Labels such as `Goal` and `Backstory` are product words. Fixtures keep the nouns.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Full order | A step whose seat has role text, goal, backstory, one skill (with a reference), a CEO goal, a guidance note, a memory note, a reference file, a saved link; the step has a previous output, loop-back reasons, a gate note and a plan input | The request's parts, in order: role, Goal, Backstory, `skill:<id>`, CEO goal, Guidance, Memory, Reference files, Saved links (system); step instruction, `step:`, `loop-back:`, `gate-note:` (user, trusted); fenced plan input (user) | — |
| 2 | Journaled prompt | Row 1, then read the journal | `prompt_parts` rows for the attempt, `ord` 1..n, one per request part, each with slot, kind (`instruction`, `trusted`, `untrusted`), label, taint; kinds match row 1's types | — |
| 3 | Slots filled out of order | A plan whose slots are filled in reverse | The request and the journal hold the AD-27 order | — |
| 4 | Empty slots | A seat with no goal, no skills, no links; no notes | No `Goal`, `Skills`, `Reference files` or other empty part, no empty heading | — |
| 5 | Skill part | Attached skill with SKILL.md body and two reference files | One `skill:<id>` part: the body, then the reference file names (`references/<name>`); the references' text (canary) is in no request part, no journal row, no audit entry | — |
| 6 | Reference files and links | A seat with two reference files (`file`, with a `note`) and two saved links (with a `group`) | One `Reference files` part of file names only (the `note` and the files' text are absent); one `Saved links` part of `title — url` per link (the `group` absent); no file opened, no fetch | — |
| 7 | Hidden text in a name | A link title with a bidi override, a zero-width space and a newline; a file name with a tab | Hidden characters written as `\u{XXXX}` (3.4's table), newlines and tabs become one space; every entry stays on one line | — |
| 8 | Guidance expiry | FakeClock at T; notes expiring at T-1 s, T, T+1 s and never | Only the T+1 s and never notes are present; a later attempt after the clock passes T+1 s drops the second | — |
| 9 | Trusted inputs | A clean producer, a clean loop-back producer, a gate note (3.4) | Each its own `Part::Trusted` after the step instruction and before the fence, labelled `step:<key>@<n>`, `loop-back:<key>@<n>`, `gate-note:<key>@<n>`, in that order; the journal rows have kind `trusted` and no text (the text stays on the producer's row or the item) | — |
| 10 | Tainted producer | A tainted previous output | The part is `Part::Untrusted` with the same label, kind `untrusted`, taint 1, inside the fence; the attempt is tainted | — |
| 11 | The fence | A plan input whose text holds `</untrusted-data>`, `</UNTRUSTED-DATA>`, `<untrusted-data source="x">` and a fake label line; its label holds a quote, a newline and a bidi override | Every adapter renders it inside one labelled fence of its own; no marker in the text or label closes or opens a fence; the label is one line with hidden characters escaped | — |
| 12 | Nothing cut | A 64 KB skill body, a 2,000-character backstory, a 200 KB plan input | Every request part equals its source, byte for byte | — |
| 13 | A gate | A gate step | No model call, no `prompt_parts` row for it | — |
| 14 | Writer refuses disorder | `StartAttempt` with parts out of AD-27 order, an `untrusted` part in an instruction slot, an `instruction` part with taint 1, an `untrusted` part with taint 0 | Refused in words; nothing written | Worded |
| 15 | Writer raises taint | `StartAttempt` with an `untrusted` part and attempt taint clean | The attempt's taint is raised to 1 in the same transaction (taint only rises) | — |
| 16 | Valid first time | Step with a schema; FakeLlm's final message matches | One call; output stored as written; step `succeeded`; no repair row | — |
| 17 | Repaired once | First message invalid, second valid | Two calls; the second is `purpose = 'repair'`; the step succeeded with the second message as its output; the journal marks the repair; the repair call's parts are journaled | — |
| 18 | The repair prompt | Row 17 | FakeLlm's second request: the same system and user parts as the first, then the invalid message as `Part::AssistantText`, then the schema's repair `Instruction` (user role). No fence, no new `UntrustedData`. The step's taint is unchanged | — |
| 19 | Invalid twice | Both messages invalid | Exactly two calls, no third; step `failed`; run `failed`; one Failure item whose words name the step, the schema id and each failing place and rule; the invalid texts are not stored as the step's output | Failure item |
| 20 | Not JSON | A final message that is prose or a fenced code block | Fails the check with "its final message is not JSON" (no repair of fences by the engine); then as row 17 or 19 | Failure item |
| 21 | Words never echo | Invalid messages with canary values: a string value, an unexpected key name, a value outside an enum | The Failure item, the run's reason, the audit entries and the repair `Instruction` hold none of the canaries | — |
| 22 | Cap and clock | `limits.iterations: 1`; the first message is invalid | No repair call; the step fails with a Failure item that says the schema failed and the iterations cap left no turn to repair it. Same for a passed wall clock | Failure item |
| 23 | Repair is a call | Row 17, ledger | Two `model_calls` rows, two `ledger` reservations, each settled; no other reservation path; the repair call's audit entry names `purpose: repair` | — |
| 24 | Tainted step | Row 17 on a tainted attempt | Repair parts journaled with taint 1; the attempt and its output stay tainted; a clean step stays clean | — |
| 25 | With a loop-back | A step with a schema and a `loop_back` | Schema check first; the reasons are read only from a message that passed it | — |
| 26 | Remote or file `$ref` | A schema file with `"$ref": "https://…"` or a file path (past the validator, e.g. hand-built in a test) | `agentic run` and the engine refuse the run before any row is written, naming the step and the schema id; nothing fetched, no file read | Worded |
| 27 | Schema that cannot compile | A pattern with look-around; an unknown `type` | Refused as row 26 | Worded |
| 28 | Repair only once at the writer | A second `ReserveCall` with `purpose = repair` for one attempt; a repair with no settled first turn | Refused in words; nothing written | Worded |
| 29 | Plan from config | `fixtures/projects/prompt-parts` through `plan_of` and a FakeLlm provider | The request holds Role, Goal, Backstory, `skill:brief`, Reference files, Saved links in order; no CEO, guidance or memory part (no source); `output_schema` is honoured, not refused | — |
| 30 | Migration 0009 | A schema-8 database with runs, attempts, calls, inputs and Inbox items | Pre-migration copy; every row kept value for value; `prompt_parts` and `model_calls.purpose` exist (existing calls `turn`); report 8 → 9; `store.migrated` audited; reopening applies nothing | Migration error names the backup |
| 31 | Journal is append-only | `UPDATE` or `DELETE` on `prompt_parts` | Aborted by trigger | — |
| 32 | Content kept out of audit | Any run | `step.started` and `model_call.reserved` audit args hold counts and the purpose, never a part's text | — |

</intent-contract>

## Code Map

- **`crates/am-domain/src`:**
  - `content.rs` (`Instruction` L28, `authored` L31, `UntrustedData` L111, `TrustedInput` L221): add doctests on `Instruction`: an allowed build, and `compile_fail` for `Instruction::from(UntrustedData)`, `Instruction::authored(UntrustedData::text(..))`, `let i: Instruction = untrusted.into()` and the reverse (`UntrustedData` from an `Instruction`). No type change.
  - `llm.rs` (`Part` L34, `ChatRequest` L61): unchanged. A repair turn uses `Part::AssistantText` and `Part::Instruction`.
  - `text.rs` (`is_hidden`, `escape_hidden` L55): used for names and labels, unchanged.
- **`crates/am-store`:**
  - `migrations/0009_prompt_parts.sql` (new) and `migrations.rs` (`MIGRATIONS` L35-L44): see Design Notes, "Where the prompt lives".
  - `lib.rs`: `WriteCommand::StartAttempt` (L235) gains `prompt: Vec<NewPromptPart>`; `ReserveCall` (L244) gains `purpose: CallPurpose` and `prompt: Vec<NewPromptPart>` (a repair call's parts, empty for a plain turn); `start_attempt` (L1851) and the `ReserveCall` arm (L1102) write the rows and check slot order, type and taint; `CallRow` (L2517) gains `purpose`; `Reader` gains `prompt_parts(step_run_id)`; `calls_for_step` (L2755) reads `purpose`.
  - `am-store/src/lib.rs` audit arms for `step.started` and `model_call.reserved`: add `prompt_parts` count and `purpose`.
- **`crates/am-engine`:**
  - `Cargo.toml`: `jsonschema.workspace = true`; `sha2` moves from `[dev-dependencies]` to `[dependencies]` (already pinned; a file part is journaled by hash).
  - `src/prompt.rs` (new): the slots, the labels, `heading(label)`, `assemble(...)`, and the journal records.
  - `src/schema.rs` (new): `compile(schema_id, &Value) -> Result<Compiled, SchemaError>`, `check(&Compiled, &str) -> Result<(), Vec<Finding>>`, and `words(&[Finding]) -> String`. The only file that names `jsonschema`.
  - `src/run.rs`:
    - `StepPlan` (L148) gains `prompt: StepPrompt` and `output_schema: Option<OutputSchema>`; `RunPlan` (L172) gains `clock: Arc<dyn Clock>`.
    - `inputs_of` (L865) is unchanged in what it computes; `run_step` (L1025, request at L1088-L1090 and L1129) builds the request through `prompt::assemble` and passes the records to `StartAttempt`.
    - `end_turn` (L1392) validates before the loop-back branch and before `succeed` (L1452).
    - A new repair step beside `fail_with_item` (L1514); `Cap` (L1615) gains `Schema { .. }`.
    - `run_on` (L387) compiles every step's schema before `CreateRun`; `RunError` (L197) gains `Schema { step, why }`.
- **`crates/am-app/src/run.rs`:** `not_honoured` (L278) stops naming `output_schema`; `plan_of` (L291) builds the slots from the seat and its skills (goal, backstory, skill bodies, reference-file names, links), the schema's value and repair `Instruction` from `snapshot.schemas()`, and a system clock; the gate branch (L348) builds none.
- **`crates/am-providers/src/common.rs`:** `render_untrusted_text` (L10) and `render_trusted_input` (L25): the label goes through `am_domain::text::escape_hidden` and loses newlines. Fence text is unchanged.
- **`xtask/src/deps.rs`:** one more call-site rule beside `INSTRUCTION_SITES` (L42): a file allowed to call `Instruction::authored` may not name `UntrustedData`, `UntrustedContent` or `into_content` in code. The rule's own tests are in `xtask/tests/check_deps.rs`.
- **`Cargo.toml` (workspace):** `jsonschema = { version = "=0.61.2", default-features = false }`, replacing the comment at L116-L118. `docs/crate-notes/jsonschema.md`: as built.
- **Fixture:** `fixtures/projects/prompt-parts/` (a copy of `anatomy`'s seat, skill and runtime seed, plus a workflow with `output_schema` and `schemas/`).
- **Existing tests that must change (supersession, as in 3.3 and 3.4):**
  - `crates/am-app/tests/run_cli.rs` L1583-L1600: `output_schema` leaves the list of keys a step is refused for.
  - `crates/am-store/tests/run_journal.rs` L1097-L1101: `SCHEMA_VERSION == BEFORE_RESOLUTION + 1` ("0008 is this story's") becomes "the 7 to 8 step is applied".
  - Every literal `StepPlan { .. }` and `RunPlan { .. }` gains the two new fields: `am-app/src/run.rs` (L349, L380), `am-engine/src/run.rs` (unit tests), `am-engine/tests/{run,loop_caps,toolgate}.rs`, and in `am-e2e`: `tests/engine/support/mod.rs` (L70, L123), `tests/engine/handoff_gate_resolve.rs`, `tests/trust/{inbox_resolve_human_only,taint_trusted_input_gate_note,rule_15_inbox}.rs`.
  - Every `WriteCommand::StartAttempt` and `ReserveCall` literal (`journal.rs` L117-L150, `am-store` unit and integration tests, `am-engine` tests) gains its new fields.

## Tasks & Acceptance

**Execution:**
- [ ] **Tests first (no `src/` change):** every file in the Tests-first list, the fixture, the `[[test]]` targets and the superseded literals. Log the red run with each test's failure.
- [ ] **Dependency:** the workspace line, `am-engine`'s dependency, `sha2` out of dev-dependencies, `cargo deny` over the new graph (the manager runs it; Verification).
- [ ] **am-domain:** the doctests.
- [ ] **am-store:** migration 0009, `NewPromptPart`, `CallPurpose`, the writer checks, the readers, the audit args.
- [ ] **am-engine:** `prompt.rs`, `schema.rs`, the plan types, the run changes.
- [ ] **am-providers:** label hardening.
- [ ] **am-app:** slots, schema, clock, the refusal removed.
- [ ] **xtask:** the source rule and its tests.
- [ ] **Docs:** `docs/crate-notes/jsonschema.md`; one sentence in `docs/config-schema.md` (around L315) if it says `agentic run` refuses `output_schema`.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then requests, journal rows, audit entries, Failure items and outputs match the row.
- Given the ticket's verify, when the full-parts step runs on FakeLlm: the journal prompt holds the parts in order with their labels; the skill reference and the reference file are named and their content is absent; the expired note is absent; the untrusted part is fenced; goal, backstory and skill are `Instruction` and the doctests refuse a build from `UntrustedData`; an invalid message is repaired once and marked; a second invalid message fails the step with the error in words.
- Given the workspace, when `just check` runs, then it is green, 3.3's `taint_*` and `handoff_*` suites and 3.4's `rule_15_*` and gate suites included.

## Plan Change Log

- 2026-11-24, manager: Q1–Q3 decided under the owner's standing rules (each option (a), reasons in Questions for the owner); no owner-level question remains. Owner direction (2026-11-24): 3.5 runs on Sonnet. Audit hashes of free text follow the owner's keyed-hash decision (Boundaries; built in 9.10).

## Design Notes

### Assembly order and labels (ask 1)

**The parts, in AD-27's order.** Labels are product words, written once in `prompt.rs`. A part's heading in the system text is `heading(label)`, built by am-app into the `Instruction` it hands over (am-engine cannot call `authored`).

| # | Slot | Label | Type | Role in the request | Text comes from |
|---|------|-------|------|---------------------|-----------------|
| 1 | role | `Role instructions` | Instruction | system | seat file body (`Seat.instructions`) |
| 2 | goal | `Goal` | Instruction | system | `Seat.goal` |
| 3 | backstory | `Backstory` | Instruction | system | `Seat.backstory` |
| 4 | skill (seat's `skills` order) | `skill:<id>` | Instruction | system | `Skill.body`, then the keys of `Skill.references` |
| 5 | ceo_goal | `CEO goal:<id>` | Instruction | system | plan slot (Question 1) |
| 6 | guidance | `Guidance:<id>` | Instruction | system | plan slot, dropped when expired (Question 1) |
| 7 | memory | `Memory:<id>` | Instruction | system | plan slot (Question 1) |
| 8 | reference_files | `Reference files` | Instruction | system | `Seat.knowledge[].file` only |
| 9 | saved_links | `Saved links` | Instruction | system | `Seat.links[]` title and url only |
| 10 | step | `Step instruction` | Instruction | user, first | `Step.instruction` |
| 11 | trusted | `step:<key>@<n>`, `loop-back:<key>@<n>`, `gate-note:<key>@<n>` | TrustedInput | user | journal, through 3.3's and 3.4's constructors |
| 12 | untrusted | the input's own label | UntrustedData | user, own fence each | plan inputs; a tainted producer's output (same label) |

- **Slots 5 to 7 can repeat.** Each goal or note is its own part, in the plan's order (which the filler decides; the goals' tree inheritance is the filler's).
- **Slot 11's order** is today's: the previous output, the loop-back reasons, the gate note (3.4: "after the other trusted inputs"). Slot 12 keeps declared order. Later stories add `branch:` and `handoff:` to these two slots; the writer's slot list is the one place to extend.
- **Why system for slots 1 to 9.** They precede the step instruction in AD-27, and role text is in `system` today. They are `Instruction`, which AD-10 allows there. Trusted inputs and the fence stay in the user role, as AD-10 requires.
- **Empty slots** produce nothing.

**How each part's label is stored.** Migration 0009 (Question 3; the baseline has no prompt table, and `model_calls` has no prompt column, so no existing table can hold it). Reasons for a new table, not a column:
- there are many parts per attempt, in order, each with its own slot, type and taint;
- it is append-only, like `step_inputs` and the audit log;
- the existing `step_inputs` stores edges (AD-43), not request parts, and has no goal or skill.

**Nothing is truncated.** `assemble` copies each source string whole. No size limit is added; AD-38's 64 KB, 500 and 2,000 character caps are config's. A test (row 12) pins byte equality.

**Reference-file and skill-reference content never enters.** Reference files are `Seat.knowledge[].file` names; their text lives in the runtime folder and is not read by assembly. A skill's references are in `Skill.references` as text (the loader read them for hashing and secret checks): am-app takes only the keys. Until 9.16, the list is `references/<name>` as the loader gives it; AD-38 says 3.5 must switch to the loader's one list of skill files when 9.16 builds it, and the plan keeps the names in one `Vec<String>` so that is a one-line change. Saved links carry title and URL; the `group` and any page content stay out. Canary text in a reference file, a skill reference and a link's page is absent from the request, the journal and the audit (rows 5, 6, 32).

**Names and titles are config, but may hide characters.** A link title can come from a department import the owner reviewed. Each name, title and URL goes through `escape_hidden` (3.4's table, `am-domain/src/text.rs`); newlines and tabs become one space, so an entry cannot span lines or forge a heading. Row 7.

**Expired guidance notes are absent.** Each note carries `expires: Option<jiff::Timestamp>`. `assemble` compares it with `RunPlan.clock` (`am_domain::Clock`; FakeClock in tests) at each attempt's start; `expires <= now` drops it. A loop-back attempt days later re-evaluates. The note stays in History (5.8), not in the journal prompt. Goals carry no expiry here (the ticket names guidance only; 5.8's end date is its own).

**Untrusted data and its fence.**
- Each `UntrustedData` is its own labelled fence in the user role, rendered by `am-providers`' one function (`render_untrusted_text`, `common.rs` L10), after the trusted inputs. Native file parts keep their fence label (`file_label`). This is unchanged and shared by both adapters.
- **Closing or spoofing by content:** `neutralise` escapes every `</untrusted-` in text, any letter case, and the trusted block also escapes `<untrusted-`. Row 11 proves both with hostile text.
- **By label:** the label is engine- or source-built, but a source may put a quote, newline or bidi mark in it. `render_*` already replaces `"`. This story adds `escape_hidden` and drops newlines from the label in both render functions (a small change in `am-providers`, with its tests). 3.4's `text.rs` table is reused; no second table.
- **Content is not escaped:** the data keeps its characters (AD-27: never cut or rewrite). Only the fence tag is made safe, as AD-10 and AD-27 say.

### The type wall (ask 2)

- **Types by part:** goal, backstory and every skill body are `Instruction`, as are the role text, the CEO goals, the notes, and the two name lists (AD-10 (b): "owner text that is config, such as goals, guidance and memory notes, stays `Instruction`"). The step instruction is `Instruction` already.
- **Where each part's text comes from:**
  - config: role, goal, backstory, skill, reference names, links, step instruction. Built in `am-app/src/run.rs` (the one existing `Instruction::authored` site for config);
  - plan slots with no source yet: CEO goals, guidance, memory (Question 1);
  - the store: trusted inputs, read from the journal by 3.3's and 3.4's code and unchanged here;
  - a tainted step: only as `UntrustedData` (the tainted producer's output, a tainted reason or note), by AD-10 (a).
- **By type (existing):** `Instruction` has a private field, and `UntrustedData` has no `Into<String>` and no `From`. So `Instruction::authored(untrusted)` does not compile. Doctests on `Instruction` pin four refusals, each paired with an allowed build so a missing item cannot pass for the wrong reason.
- **By source rule (new):** a string can still be pulled out of `UntrustedContent::Text(s)`. So `check-deps` adds: a shipped file allowed to call `Instruction::authored` (`INSTRUCTION_SITES`) may not name `UntrustedData`, `UntrustedContent` or `into_content`. Today none does (one comment in `journal.rs` names the type; comments are not code to the scanner). This only narrows; it weakens nothing.
- **By the journal:** the writer checks each part's slot against its kind. An `instruction` row with taint set, an `untrusted` row in an instruction slot, or a row out of order is refused (rows 14, 15). The journal cannot say a part was an instruction when it was not.
- **Can a config file be tainted?**
  - A seat file, a skill's SKILL.md and a skill folder's text files are config once applied (AD-10, AD-38). The only way tainted text reaches config is a Proposal, and a tainted-origin Proposal is never approvable (AD-10 (b)), so applied config carries no taint. The assembler has no taint input for config parts.
  - A skill import not yet applied is `UntrustedData` and is not in the snapshot (AD-38), so assembly never sees it.
  - The seat's reference files are `UntrustedData` content (AD-10), but only their names are config; this story never reads their content.
  - A skill reference file's trust class is the open question in AD-38 and the architecture's Open Questions (item 1, 2026-11-23): `read_skill_file`'s "trusted input". 3.5 does not read a skill file's content into a prompt or a tool result, so it needs no answer and does not touch the question. It must be settled before 9.16.

### Parts whose sources do not exist yet (ask 3)

| Part | At `a7b5049` | Owner |
|------|--------------|-------|
| Goal, backstory | `Seat.goal`, `Seat.backstory` (`am-config/src/entities.rs` L171-L173) | 1.12, built |
| Skills (body, reference names) | `Seat.skills`, `Skill.body`, `Skill.references` | 1.12, built; the one list of skill files is 9.16's |
| Reference files | `Seat.knowledge[].file` | 1.12, built |
| Saved links | `Seat.links` | 1.12, built |
| Gate note | `gate_note` input and `from_owner_note` | 3.4, built |
| CEO goals | none: no field, no table; `Team` and `Hat` have none | 5.8 "Human seats, CEO goals and guidance notes" |
| Guidance notes (expiry) | none | 5.8 |
| Memory notes | none; the seat file schema has no field (`capabilities.memory` only) | 5.3 (seat sheet) and 10.2, 10.4 (learning loop) |

Smallest honest way (Question 1, assumed): assembly takes CEO goals, guidance notes and memory notes as typed slots on the plan, and the shipped `plan_of` fills them empty with a comment naming 5.8 and 5.3. The tests build the plan with values, which meets the ticket's verify at the engine, where the order and the journal are decided. Each slot is shaped so the owning story fills it without changing the engine: an id, an `Instruction`, and for guidance an `expires`. The writer's slot list already holds them.
- This adds no config field, no table and no `is_widening` classification (AD-15's exhaustive match would force one for any new config field, which belongs with the story that defines the field).
- It does not meet the verify through `agentic run` from files for these three. That gap closes when 5.8 and 5.3 fill the slots; 5.8's own verify already requires "a FakeLlm run's journal prompt holds both as labelled parts", and 10.4's "notes keep reaching prompts in the AD-27 order".
- If the owner wants the verify met from files, that means building the config for 5.8 and 5.3 first. That is another story's feature (Question 1, option b).

### Where the prompt lives (migration 0009) (ask 1, journal)

**`0009_prompt_parts.sql`**, additive, forward-only (AD-7):
1. `CREATE TABLE prompt_parts`: `id` autoincrement; `step_run_id` NOT NULL, foreign key; `call_seq` INTEGER NOT NULL (0 is the attempt's assembled prompt, n is a repair call's own parts); `ord`; `slot` CHECK in the slot list; `kind` CHECK in (`instruction`, `trusted`, `untrusted`, `assistant`); `label`; `taint` CHECK (0, 1); `text` NULL; `bytes`; `sha256`; `created_at`; `UNIQUE (step_run_id, call_seq, ord)`.
2. Content by kind of source:
   - config and plan text: `text` stored (the only copy of what the model read from config, since a run pins only a hash, AD-16);
   - a trusted or fenced input from a step output, a loop-back or a gate note: no `text`; the part's label equals its `step_inputs.label`, and the text stays on the producer's row or the Inbox item (AD-43: never stored twice; `step_inputs` already says "text is never copied here"). Drill-down joins them;
   - a plan input (no other home): `text` stored, marked tainted at rest;
   - a native file part: label, size and sha256 only, never bytes.
3. `ALTER TABLE model_calls ADD COLUMN purpose TEXT NOT NULL DEFAULT 'turn' CHECK (purpose IN ('turn', 'repair'))`.
4. Triggers: `prompt_parts_no_update` and `prompt_parts_no_delete`, as `audit_log` has.
5. Nothing is rewritten; existing attempts simply have no parts (the reader returns none and says "not recorded").

**Why no earlier table is enough.** `step_inputs` holds edges and taints, not parts (it has no goal, skill or list). `model_calls` and `ledger` hold tokens and money. The audit log must hold no content (AD-25). A column of one JSON blob on `step_runs` would hide the order and the types from the writer's checks.

**Behind AD-7:** the pre-migration copy, report 8 → 9, `store.migrated` audited, a failed step rolls back whole and names the backup. The migration test follows 3.4's pattern (`database_at_version(&path, 8)`, raw rows, every row equal after, reopening applies nothing, a forged row fails closed).

**Writer rules** (`start_attempt` and the repair `ReserveCall`): slot order is AD-27's (non-decreasing, repeats allowed only in slots 4-7, 11, 12); kind matches slot (1-10 `instruction`, 11 `trusted`, 12 `untrusted`, repair parts `assistant` and `instruction`); taint matches kind (`untrusted` is 1; `instruction` and `trusted` are 0; an `assistant` part carries the attempt's taint); an `untrusted` part raises the attempt's taint in the same transaction; an input part's label must match a journaled `step_inputs` row of the attempt. Audited: `step.started` gains the part count; `model_call.reserved` gains `purpose`.

### Schema validation and repair (ask 4)

**Where the schema comes from.** `Step.output_schema` names `schemas/<id>.json`; the snapshot holds it as a JSON value (`ConfigSnapshot::schemas`, `snapshot.rs` L56). `plan_of` copies the value into `StepPlan.output_schema` with its id. A missing id is already a validate error (1.14). The file's own `schema_version` key is an unknown keyword to the validator and is ignored.

**`jsonschema` use (`schema.rs`, the only file naming it).**
- `jsonschema::options()` with `PatternOptions::regex()` (the linear-time engine, with size limits) and the draft the file's `$schema` names, else 2020-12. Formats are not asserted (the draft default); `idna` is off with the defaults.
- **Why the linear-time engine:** the subject of a `pattern` is the model's text, which may be hostile. fancy-regex can backtrack exponentially. A schema that needs look-around or backreferences is refused in words at compile time (row 27). No schema uses one yet.
- **Compile before any row:** `run_on` compiles each step's schema before `CreateRun`. A failure is `RunError::Schema { step, why }`, nothing written, no provider asked for.
- **`$ref`:** `am-config` already refuses any `$ref` not starting with `#`. With `resolve-http` and `resolve-file` off, the validator's default retriever has no way to fetch or read; a remote or file `$ref` makes the build fail. A test builds such a schema past the config check and proves the refusal and that nothing was fetched (FakeEgress sees no request; no file path is read). The crate source shows this behaviour; the red run confirms it.
- Its types stay in `am-engine`: `StepPlan` holds a `serde_json::Value`, never a `Validator`.

**The repair loop** (`end_turn` L1392, in `run.rs`):
1. A turn ends with no tool call. If the step has a schema, parse `out.text.trim()` as JSON. The engine does not strip code fences or fix quotes (no silent conversion); that is what the repair is for.
2. Valid: go on to the loop-back branch, then `succeed`, as today.
3. Invalid, no repair yet, a turn and time remain: settle that call, then reserve a repair call (`purpose = repair`) whose request is the same system and messages as before plus two messages:
   - the invalid message, as `Part::AssistantText` in the assistant role (what the tool loop already does with the model's own words);
   - one user message holding the step's repair `Instruction`: a fixed sentence and the schema text, built in am-app from the schema file.
4. The repair call's parts are journaled (`call_seq` = the repair call's): an `assistant` row labelled `reply:<key>@<n>` with the invalid text (marked at rest with the attempt's taint), and an `instruction` row labelled `repair`.
5. The repair's message is checked the same way. Valid: the step succeeds with it as its output. Invalid: `fail_with_item`, one Failure item, the run `failed`.

**What types the invalid output has (Question 2).**
- The model's reply is its own words from this very attempt. The existing loop already replays such words as `Part::AssistantText`. Doing the same needs no new type and does not raise taint: if the attempt read untrusted content it is already tainted and its journal rows say so; if it did not, nothing untrusted entered.
- Wrapping it as fenced `UntrustedData` is the stricter reading and would taint every step that needed a repair, and everything downstream of it (AD-10: a step is tainted once any `UntrustedData` enters its context). AD-10 does not list a model's own reply among `UntrustedData`'s sources. AD-27 is silent on a repair turn. This is Question 2.

**How "repaired" is stored.** The repair call's `model_calls.purpose = 'repair'`, and its journaled parts. There is no separate flag on the step: whether a step was repaired is a fact of the journal (a repair call exists), so it is not stored twice. The writer allows at most one repair call per attempt and none before a settled first turn (row 28). `Reader::calls_for_step` returns the purpose. `agentic status` does not show it in this story; drill-down (epic 6) will.

**The failure, in words, on AD-8's edge.** `running → failed` through `fail_with_item`, as the loop-back past its maximum does: the step fails, one Failure item (payload the step run, taint the attempt's), the run `failed`, the reason "step `<key>` failed: its final message does not match the output schema `<id>`: …". The step's `output` stays empty: a message that failed validation is not an output (AD-36: the output is the validated final message). The second invalid text is not stored (as a failed turn's text is not, `fail`, L1478). The first stays only in the repair call's journaled parts.

**The words never echo the model's text.**
- Each finding is a rule and a place, built from the schema, not from the message: the failing keyword (`jsonschema`'s error kind `keyword()`) and the location in the schema (`schema_path`), turned into words such as "field `count`: must be of type integer". Property names come from the schema's `properties` and `required`.
- Never used: the error's `Display` (it quotes the instance), the instance path's names (they come from the message), any value, any unexpected key's name. An extra field is "a field the schema does not allow", unnamed. Array positions are numbers.
- At most 5 findings, 200 characters each, hidden characters escaped; the rest are counted.
- These words go only to the Failure item and the run's reason. They are never put in any prompt (so the repair needs no engine-authored instruction), never in an event and never in the audit log.

**Budgets and caps.** The repair is a model call: `ReserveCall` (zero reserved until 3.7; priced like any call after it; the caps of epic 4 check it with every reservation), `SettleCall`, its own ledger row. It counts against `limits.iterations` (the cap counts `model_calls` rows) and against the wall clock (`call_within`). It never gets an exemption: with no turn left the step fails with a Failure item saying so (row 22). A cancel stops it as any call.

**Interaction with a loop-back step.** The schema check runs first. `reasons_in` (L662) reads only a message that passed it, so a step cannot loop back on a message the schema refused.

**Audit hashes (owner decision 2026-11-24).** The audit args this story adds are `prompt_parts` (a count) and `purpose`; neither is a hash. `prompt_parts.sha256` is a journal column for native file parts only (a file hash, kept bare), not an audit arg. A repair call's output reaches audit only through the existing `output_sha256` helper path (`audit::sha256_hex`). No `Reader` or engine code computes its own hash of owner text, model output or validation text for audit. Tests: `audit_args_hold_counts_and_purpose_never_text` also asserts every `*_sha256` arg of a repaired run came from the helper (equal to `audit::sha256_hex` of the stored text) and that no new `*_sha256` key appears. The decision is recorded elsewhere (AD-17, AD-32, 9.10); no doc is edited here.

### Dependency (ask 5)

- **Add:** workspace `jsonschema = { version = "=0.61.2", default-features = false }`; used by `am-engine` only. The Stack row (architecture L496) names it, linked from epic 3 story 5. AD-3 allows MIT-0 since 2026-11-20 and `deny.toml` lists `MIT-0`; no exception is needed.
- **No HTTP stack:** with the defaults off, `reqwest` and `rustls` are not in the resolved graph. (`idna`, `tokio` and `async-trait` are optional and off.) `cargo xtask seam-guard` and the no-second-client rule are unaffected.
- **New packages in `Cargo.lock`.** Resolved offline in a scratch package outside the repo, with `jsonschema = { version = "=0.61.2", default-features = false }`; the workspace's own lock may pick slightly different versions of shared crates. All are MIT, MIT OR Apache-2.0 or MIT-0, so inside the allow-list:

  | Package | Licence |
  |---------|---------|
  | jsonschema 0.61.2, jsonschema-regex 0.61.2, jsonschema-value 0.61.2, referencing 0.61.2 | MIT |
  | fluent-uri 0.4.1, fancy-regex 0.20.2, micromap 0.3.0, outref 0.5.2, uuid-simd 0.8.0, vsimd 0.8.0 | MIT |
  | borrow-or-share 0.2.4 | MIT-0 |
  | ahash 0.8.12, allocator-api2 0.2.21, bytecount 0.6.9, email_address 0.2.10, fraction 0.17.0, num 0.4.3, num-bigint 0.4.8, num-complex 0.4.6, num-integer 0.1.47, num-iter 0.1.46, num-rational 0.4.2, num-cmp 0.1.0 | MIT OR Apache-2.0 (bytecount and num-cmp as `Apache-2.0/MIT`, `MIT/Apache-2.0`) |

  - 23 packages. Already in the lock and reused: `serde`, `serde_json`, `regex`, `regex-automata`, `regex-syntax`, `percent-encoding`, `data-encoding`, `itoa`, `num-traits`, `strum`, `unicode-general-category`, `once_cell`, `parking_lot` and others.
  - `fluent-uri` pulls `borrow-or-share` (the one MIT-0), as the crate note said.
- **`cargo deny`:** `multiple-versions = "warn"` only. The manager runs `just supply-chain` (licences, bans, sources) on the real lock; this plan did not build. A licence outside the allow-list or a crate outside the Stack would be an owner stop; none is expected.
- **Crate note:** `docs/crate-notes/jsonschema.md` is updated to as built: features, the pattern engine, the `$ref` behaviour, the types kept in `schema.rs`.

### Size and commit

Estimated 3,000 to 4,000 changed lines with tests: a migration, the assembler, the validator, the plan types (about 10 test files change for the literals), a provider tweak, an xtask rule and the fixture. 3.3 and 3.4 carried the same `oversized` warning. The owner's standing decision for 3.3 stands: one story, one squash commit; on an overrun, stop and report, never split. Not a question.

### Questions for the owner (stop: AD or ticket intent)

None. The drafter raised three questions. The manager decided each one under the owner's standing rules (docs/owner-standing-rules.md: small scope trims inside a story, and the recommended option when it is a pure engineering call). None of them amends an AD, changes the ticket's intent or touches an always-stop area. The owner said on 2026-11-24 to stop only for owner-level questions.

**Q1. CEO goals, guidance notes and memory notes have no source at the baseline. Decided: option (a).**
- The assembler has typed slots for the three. The shipped `plan_of` leaves them empty, and the tests supply values through the plan.
- The order, the types, the expiry rule and the journal are built and proven here. Filling the slots from files is 5.8's and 5.3's work, and their own verify already requires it.
- Why: the ticket's verify is a step "with a CEO goal, a guidance note, …" that stores the labelled parts. The engine-level test meets that without inventing a config field or `is_widening` class that another story owns. Option (b) would build another story's feature. Option (c) would trim the verify further than needed.

**Q2. The type of a model's invalid reply inside the repair turn. Decided: option (a).**
- The reply is replayed as `Part::AssistantText` in the assistant role. The repair instruction is config text built in am-app. The step's taint is unchanged.
- Why: AD-10's lists of `UntrustedData` and `TrustedInput` sources do not include a model's own reply in its own attempt. AGENTS.md's list of untrusted content does not include it either.
- The agent loop already replays the model's own words this way. If the attempt read untrusted content, it is already tainted and stays tainted.
- Option (b) would taint clean steps that read nothing untrusted. Option (c) would give the model no way to see what it got wrong.
- The validation words never reach any prompt.

**Q3. Migration 0009 and what the journal keeps. Decided: option (a).**
- Additive migration: `prompt_parts` plus `model_calls.purpose`.
- Config text and plan-input text are stored, plan inputs marked tainted at rest. Step-output and gate-note text is referenced through `step_inputs`. File bytes are never stored.
- Why: AD-25 and AD-35 already put full prompts and responses in the run journal, deleted with the run, and AD-43 says never stored twice.
- The migration deletes, moves and overwrites nothing. It keeps the pre-migration copy (AD-7) and is tested as the owner required for 0008 (row 30: every row kept value for value, the pre-migration copy present).

**Asked and answered by the plan, not questions:** the dependency (in the Stack, licences inside the allow-list, none outside); the size (one commit, owner rule); `read_skill_file` (untouched); the new `check-deps` rule (it only narrows); the failure being a Failure item, the repair counting against `limits.iterations`, the linear-time pattern engine (engineering, with reasons above).

### Tests-first list

All FakeLlm and FakeClock; no network; no real model. Red reasons are what the first run shows before any `src/` change; most are compile errors for names that do not exist yet.

**am-domain (doctests, `crates/am-domain/src/content.rs`):**
- `Instruction` doctests: an allowed build (`Instruction::authored("goal")`); `compile_fail`: `Instruction::from(UntrustedData::text(..))`; `Instruction::authored(UntrustedData::text(..))`; `let _: Instruction = UntrustedData::text(..).into();` the reverse (`UntrustedData` from an `Instruction`). Rows 14, type wall. Red: the doctest fails to exist (no failure to compile is not a red run; the red is the missing file text, so the doctests are added in the test phase and are green by the missing impls; recorded in the log as "doctest added; passes on the existing types", and the writer and xtask tests carry the red).

**am-store (`crates/am-store/tests/prompt_journal.rs`, new `[[test]]`):**
- `migration_0009_keeps_every_row_and_the_pre_migration_copy` (row 30). Red: `SCHEMA_VERSION` is 8, no step 8 to 9.
- `migration_0009_marks_old_calls_as_turns_and_leaves_no_parts` (row 30). Red: no `purpose` column.
- `reopening_a_version_9_database_applies_nothing` (row 30). Red: as above.
- `prompt_parts_cannot_be_updated_or_deleted` (row 31). Red: no table.
- `start_attempt_journals_the_parts_in_order` (row 2). Red: no `prompt` field on `StartAttempt`.
- `the_writer_refuses_parts_out_of_order_or_of_the_wrong_kind` (row 14, four cases). Red: as above.
- `an_untrusted_part_raises_the_attempts_taint` (row 15). Red: as above.
- `an_input_part_must_match_a_journaled_input` (row 9). Red: as above.
- `a_second_repair_call_is_refused` and `a_repair_needs_a_settled_first_turn` (row 28). Red: no `purpose` on `ReserveCall`.
- `audit_args_hold_counts_and_purpose_never_text` (row 32). Red: args lack them.
- **Superseded:** `run_journal.rs` L1097-L1101 (see Code Map).

**am-engine (`crates/am-engine/tests/`):**
- `prompt_assembly.rs` (new `[[test]]`): `parts_come_out_in_the_ad_27_order_whatever_order_they_are_given` (rows 1, 3); `empty_slots_make_no_part` (row 4); `nothing_is_truncated` (row 12); `expired_guidance_is_absent_and_unexpired_present` (row 8, FakeClock at T and the boundary); `a_later_attempt_drops_a_note_that_has_since_expired` (row 8); `names_and_titles_cannot_hide_or_span_lines` (row 7). Red: no `am_engine::prompt`.
- `schema_check.rs` (new `[[test]]`): `a_matching_message_passes`; `a_mismatch_is_worded_from_the_schema_not_the_message` (row 21); `not_json_is_worded` (row 20); `findings_are_capped_at_five` (words); `a_remote_ref_is_refused_and_nothing_is_fetched` (row 26); `a_file_ref_is_refused_and_no_file_is_read` (row 26); `a_lookaround_pattern_is_refused` (row 27); `the_file_schema_version_key_is_ignored`. Red: no `am_engine::schema`.
- `loop_caps.rs`/`run.rs`: **superseded literals** only.

**am-e2e engine (`crates/am-e2e/tests/engine/`, each with a `[[test]]` target, as `trust_targets_listed` requires):**
- `prompt_assembly_journal.rs`: `a_full_step_stores_its_prompt_in_order_with_labels` (rows 1, 2, 9, the ticket's verify, with every part including a gate note; the skill reference and the reference file named, content absent; the expired note absent; the untrusted part fenced; kinds as the types); `a_gate_assembles_nothing` (row 13); `the_audit_never_holds_part_text` (row 32); `a_tainted_producers_output_is_fenced_and_taints` (row 10). Red: no new fields on `StepPlan`.
- `schema_repair.rs`: `a_matching_message_is_not_repaired` (row 16); `an_invalid_message_is_repaired_once_and_marked` (rows 17, 23); `the_repair_prompt_replays_the_reply_and_adds_the_schema_instruction` (row 18); `a_second_invalid_message_fails_with_a_failure_item_in_words` (row 19); `no_third_call_is_made` (row 19); `no_turn_left_means_no_repair` and `a_passed_wall_clock_means_no_repair` (row 22); `the_schema_is_checked_before_the_loop_back` (row 25); `a_repair_on_a_tainted_step_stays_tainted_and_a_clean_one_stays_clean` (row 24); `a_run_with_a_bad_schema_writes_no_row` (rows 26, 27). Red: no `output_schema` on `StepPlan`.

**am-e2e trust (`crates/am-e2e/tests/trust/`, each with a `[[test]]` target):**
- `taint_prompt_type_wall.rs`: `the_goal_backstory_and_skill_parts_are_instructions_in_the_journal` (kinds, rows 2, 14); `no_instruction_row_is_ever_tainted_and_no_untrusted_row_is_clean` (writer, row 14); `a_tainted_producers_note_or_reason_is_never_an_instruction` (rows 9, 10, with 3.3's and 3.4's constructors); `plan_inputs_never_become_system_text` (the canary of a plan input is in no `system` part). Red: no parts journaled.
- `taint_prompt_fence.rs`: `hostile_text_cannot_close_or_open_a_fence` (row 11, both adapters' bodies from `am-providers` against a FakeEgress); `a_label_cannot_span_lines_or_hide_characters` (row 11). Red: labels are not hardened.
- `taint_prompt_names_only.rs`: `canaries_in_reference_files_skill_references_and_links_reach_no_request_journal_or_audit` (rows 5, 6, 32). Red: no assembly of these parts, so the test asserts their names and fails on the missing parts.
- `taint_prompt_repair.rs`: `the_invalid_reply_is_replayed_as_assistant_text_and_taints_nothing` (rows 18, 24, Q2 option a); `validation_words_hold_no_canary_from_the_message` (row 21, in the item, the reason, the audit and the repair instruction); `a_repair_never_clears_taint` (row 24). Red: no repair.

**am-providers (`crates/am-providers/tests/`, existing file or new `labels.rs`):** `a_label_is_one_line_with_hidden_characters_escaped` (rows 11, 32; unit-level). Red: the label is only quote-replaced.

**am-app (`crates/am-app/tests/`):**
- `run_prompt.rs` (new): `plan_of_builds_the_seat_parts_from_the_prompt_parts_fixture` (row 29); `plan_of_leaves_ceo_guidance_and_memory_empty` (Q1); `output_schema_is_honoured_by_agentic_run` (row 29). Red: no fixture, no slots.
- **Superseded:** `run_cli.rs` L1583-L1600.

**xtask (`xtask/tests/check_deps.rs`):** `an_instruction_site_naming_untrusted_data_fails` and `the_shipped_instruction_sites_name_none` (the type wall's source rule; fixtures in `xtask/tests/fixtures`). Red: no such rule.

**Fixture:** `fixtures/projects/prompt-parts/` with `company.yaml`, one seat (goal, backstory, `skills: [brief]`, two `knowledge` files, two `links`), `skills/brief/` with a reference file, `runtime-seed/knowledge/<seat>/`, a workflow with `output_schema: line`, `schemas/line.json`. The canary strings are neutral (no department vocabulary: the denylist scans `crates/`).

## Verification

- `cargo nextest run` per crate, filters first: `am-store` `prompt_journal`, `am-engine` `prompt_assembly` and `schema_check`, `am-e2e` the new `engine` and `trust` targets, `am-app` `run_prompt` and `run_cli`, `am-providers`, xtask `check_deps`.
- `cargo test --doc -p am-domain`.
- `just check` at the end (fmt, clippy `-D warnings`, nextest, doctest, check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard, `agentic validate` over templates and fixtures).
- `cargo xtask check-deps`: the new source rule green on shipped code, and red on its planted-file test.
- `just supply-chain` (cargo-deny licences, bans, sources) on the real `Cargo.lock`, run by the manager session; the new packages are the list in "Dependency".
- `cargo tree -p am-engine -e normal -i reqwest` must find nothing new reached through `jsonschema`.
- Both CI jobs green before merge (owner standing rule).

## Implementation Notes
