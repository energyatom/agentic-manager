---
title: 'One-step run from the CLI on FakeLlm'
type: feature
ticket: 1
created: '2026-11-22'
status: done
baseline_revision: '02841207dd8e90788a8f069dab9a2d526c9aa1f6'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** Epic 3 needs its tracer bullet. Nothing runs a configured workflow: the spike's `drive_run` takes a hard-coded `LinearWorkflow` on a bare database path, the store records neither the config snapshot a run started with nor when a step ran, and the CLI has no `run` or `status`.

**Approach:**
- **`agentic run <workflow>`:**
  - opens the project and loads the config snapshot;
  - creates the run with the snapshot's hash pinned;
  - for each step in order, commits the step's start and the model call's reservation before calling the model, makes one model turn through `LlmProvider`, then commits the call's settlement and the step's output together.
- **`agentic status`:** lists runs with their steps, snapshot hash and durations, from a read-only reader.
- **Tests:** they drive the command in process with FakeLlm injected through a context, as story 2.1's `accounts` command does.

## Boundaries & Constraints

**Always:**
- **Commit before effect (AD-8, AD-6):**
  - The run row (`running`, with `config_hash`), the step run's `running` state and the model call's `reserved` row are each committed, each with its audit entry in the same transaction, before the provider is called.
  - The call's settlement and the step's finish (with its output) commit in one transaction, then the run's finish.
  - Every command carries an explicit `Actor`: the owner for the manual start, `System(engine)` for the engine's transitions.
- **Pin (AD-16):**
  - The run records `ConfigSnapshot::hash()` of the snapshot it was built from, in its creating transaction. The plan the engine runs is built from that snapshot only.
  - A project whose config has errors, or a pending migration, starts no run and says why.
- **One turn per step:**
  - The request is the seat's instructions as a system `Instruction`, plus the step's instruction as the user `Instruction`, with the seat's model id and `max_output_tokens` from the step (default 4096).
  - A turn that ends `EndTurn` stores its text as the step's output.
  - A provider error, a tool request (no tools exist yet), `MaxTokens` or `Cancelled` fails the step and the run, with the reason in words.
  - A step with no `seat`, or a seat with no model, refuses the run before it is created.
- **Layering (AD-4):** am-engine depends only on am-domain and am-store. am-app loads config, maps it to an engine plan of plain types, resolves each step's provider, and calls the engine.
- **Providers:**
  - `RunContext` carries the terminal and a provider factory (`ModelRef` → `Arc<dyn LlmProvider>`, or a worded refusal).
  - The `agentic` binary's factory wires no provider in this story. It refuses in words before any run is created.
  - Tests pass a FakeLlm factory in process. No env var, flag or feature makes a fake provider reachable from the binary; `xtask/tests/release_deps.rs` stays green.
- **Open:** `agentic run` opens the project as `agentic serve` does (AD-32: lock, store, migration, marker) and holds it for the whole run. It is refused, naming the holder, while another process holds the lock. It asks no question: a move or take-over is refused in words, pointing at `agentic serve`.
- **Who may start a run (AD-15):** a manual run start is a CLI-class decision.
  - `agentic run` runs only when stdin and stdout are a real interactive terminal, checked through the `Terminal` port of story 2.1 (am-app's one predicate). Any other stdin is refused in words before the project is opened.
  - It is refused, naming the reason, while the project's config enables a stdio MCP server or a Source with `render: true`.
  - The fake terminal stays in am-testkit; the release guard is unchanged.
  - `agentic status` is read-only and has no such rule.
- **Status:**
  - `agentic status [--project <folder>]` reads without the lock (`find_runtime`, `Reader::open`), so it works while a run or `serve` holds the project.
  - It prints each run newest first: id, workflow, status, snapshot hash, duration (finished minus created, or "running").
  - Under each run it prints each step: key, status, duration.
  - A database older than this schema, or no runtime folder, is said in words.
- **Store:** migration `0006` adds `runs.config_hash`, `runs.finished_at`, `step_runs.started_at` and `step_runs.finished_at`. Existing commands keep their audit actions. `CreateRun` gains the hash, which is audited as an argument.
- **Ledger:** `ReserveCall` and settlement are used as they are, with a zero reservation. No ledger behaviour, schema or amount logic changes here (cost settlement is story 7).
- **Output and exit codes:**
  - `agentic run` prints `run <id> <status>`, and exits 0 on `succeeded`, 1 otherwise.
  - Usage errors exit 2.
- **Tests first:** no network, no real model.

**Never:**
- No prompt assembly beyond the two parts above (story 5), and no tools or ToolGate calls (story 3).
- No retries or fallback (story 8), no resume of unfinished runs (story 9), no queueing behind an active run (story 8), no cancel on Ctrl-C (story 11).
- No real provider in the binary, and no change to the spike's `drive_run` tests beyond what `CreateRun`'s new field needs.
- No department vocabulary in `crates/`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Run | One-step fixture; FakeLlm scripted "done" with usage | Prints `run <uuid> succeeded`, exit 0; step output "done"; run `config_hash` equals the snapshot's hash | — |
| Journal order | A provider that reads the database when called | At call time: run `running`, step `running` with `started_at`, one model call `reserved` (unsettled); audit holds `run.created`, `step.started`, `model_call.reserved` in that order | — |
| After | Same run finished | Call settled with the usage; step `succeeded` with `finished_at`; run `succeeded` with `finished_at`; audit ends `model_call.settled+step.finished`, `run.finished`; audit chain verifies | — |
| Two steps | A two-step project written by the test (the valid fixture enables a stdio MCP server, so it cannot be run from the CLI) | Both steps run in order, each with its own seat's instructions and model; two calls | — |
| Status | After one succeeded and one failed run | Both runs newest first with workflow, status, hash and duration, and their steps | No runs: says so, exit 0 |
| Provider fails | FakeLlm `fail` | Step and run `failed`; `run <id> failed`, exit 1; the reason is printed | Call settled as the store's rules say for a failed call |
| Tool request | FakeLlm turn with a tool call | Step fails: tools are not available in this version | — |
| Unknown workflow | `run nope` | Exit 1; names the workflows that exist; no run row | — |
| Invalid config | Fixture with a config error | Exit 1; says to run `agentic validate`; no run row | — |
| No model | Step's seat has no `model` | Exit 1, worded; no run row | — |
| Not a terminal | The real `agentic run` with piped stdin; in process with a piped fake terminal | Exit 1, refused naming the terminal rule; the project is not opened; no run row | — |
| MCP or render enabled | Config enables a stdio MCP server; a Source with `render: true` | Refused naming it; no run row | — |
| Binary's providers | The binary's own provider factory, called in process with an interactive fake terminal | Exit 1: no model provider is wired into `agentic run` yet; no run row | — |
| Busy | Store lock held | Refused, naming the holder | — |
| Old database | `status` on a schema-5 database | Says the project must be opened once by this version | — |

</intent-contract>

## Code Map

- `crates/am-engine/src/journal.rs`:
  - The spike driver is `create_run` (:62) and `drive_run` (:88). Its sequence is MarkLost, StartStep, ReserveCall, `provider.stream` with a `NullSink`, then SettleAndFinishStep and FinishRun. Reuse the sequence. `LinearWorkflow`, `Price` and the hard-coded prompt are spike-only: keep them working for `crates/am-e2e/tests/spike/c2_crash_resume.rs`.
  - Add a new module (e.g. `run.rs`) with plain types: `RunPlan { workflow, config_hash, steps: Vec<StepPlan> }` and `StepPlan { key, system: Vec<Instruction>, instruction, model, max_output_tokens, provider: Arc<dyn LlmProvider> }`.
  - `cancel.rs`: `TokenSignal::signal`.
- `crates/am-store/src/lib.rs`:
  - `WriteCommand` (:147), `audit_entry()` (:243), `apply` (:584-737).
  - `CreateRun { run_id, workflow, origin, steps }` (:606-615) mints the step-run ids; read them back through `Reader::steps`.
  - `Reader` (:863): `run`, `steps`, `calls_for_run`. Add a list of runs, and the new columns to `RunRow` and `StepRow`.
  - `new_id()` (:141), `ts()` (:133).
  - Migrations: `migrations/0001_initial.sql` (runs, step_runs, model_calls) and `migrations.rs:27-37`. Add `0006_run_pin_and_times.sql`.
  - `CreateRun` users to update: `crates/am-app/tests/audit_cli.rs`, `crates/am-e2e/tests/trust/rule_16_audit_log.rs`, `crates/am-store/tests/audit.rs`, `crates/am-app/src/spike.rs`.
- `crates/am-domain/src/llm.rs` — `ChatRequest` (:57), `ChatOutcome` (:137), `StopReason` (:128), `LlmProvider` (:176).
- `crates/am-testkit/src/lib.rs` — `ScriptedTurn::{text, fail}` with `.usage`, `.tool_call`; `FakeLlm::new`, `requests()`.
- `crates/am-config`:
  - `load_in(root, LoadOptions::project(knowledge))` returns `Loaded { findings, snapshot }`.
  - `ConfigSnapshot::{hash, workflows, seats}`.
  - `Step { id, seat, instruction, max_tokens }` (`entities.rs:319`); `Seat.model: Option<ModelRef>`, `Seat.instructions`; `migration_offers`.
- `crates/am-app/src`:
  - `main.rs:49-89` — dispatch and usage.
  - `cli.rs` — `EXIT_*`; the pattern of `accounts.rs:200` (`pub async fn accounts(args, ctx, out, err) -> i32` with `AccountsContext`).
  - `runtime.rs` — `open_project` (:598), `OpenRequest`, `Prompt`, `find_runtime` (:991), `owner()` (:65), `Places::from_env` (:112).
  - `identity::system()`.
  - `cli.rs:173-211` (`audit_export`) — the lock-free read pattern.
- **Fixture:** a new `fixtures/projects/one-step/`, with `company.yaml`, one seat whose model is `local-lmstudio`, and one workflow with one step. It must pass `agentic validate` (`just validate` covers fixtures). Use `fixtures/projects/valid/workflows/digest.yaml` for the two-step row.
- **Tests:**
  - `crates/am-app/tests/run_cli.rs` (in process, the `Env` pattern of `accounts_cli.rs:86-146`) and one real-binary case;
  - `crates/am-engine/tests/run.rs`;
  - `crates/am-store/tests/` (the new command fields, the reader, migration 0006).

## Tasks & Acceptance

**Execution:**
- [x] Tests first (no `src/` change): every matrix row, plus the older tests' `CreateRun` literals. Log the red run.
- [x] `crates/am-store` — migration 0006, the `CreateRun` hash, start and finish times, the reader.
- [x] `crates/am-engine/src/run.rs` — the plan types and the driver.
- [x] `crates/am-app/src/{run,cli,main}.rs` — `run` and `status`, `RunContext`.
- [x] `fixtures/projects/one-step/` — the fixture.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the output, exit code, journal rows and audit entries match.
- Given the workspace, when `just check` runs, then it is green.

## Implementation Notes

### Red run (2026-11-22, tests only, no `src/` file touched)

**Test files written** (all outside `src/`):
- `crates/am-store/tests/run_journal.rs` (new, 5 tests): the pin on `CreateRun` and its audit argument, a run with no snapshot, the step and run times, `Reader::runs()` newest first, migration 0006 on a schema-5 database.
- `crates/am-engine/tests/run.rs` (new, 6 tests): Run, After, Journal order, Two steps, Provider fails, Tool request, `MaxTokens` and `Cancelled`.
- `crates/am-app/tests/run_cli.rs` (new, 21 tests): every matrix row in process, plus the real-binary row.
- `fixtures/projects/one-step/` (new: `company.yaml`, `seats/alpha.md`, `workflows/hello.yaml`); `agentic validate --template fixtures/projects/one-step` exits 0.
- Rewritten older tests (the `CreateRun` literal): `crates/am-store/tests/audit.rs` (also the `run.created` arguments, now with `config_hash`), `crates/am-app/tests/audit_cli.rs`, `crates/am-e2e/tests/trust/rule_16_audit_log.rs`.

**Command:** `cargo nextest run --no-fail-fast -p am-store -p am-engine -p am-app -p am-e2e` — exit 101. The build stops at the first crate that does not compile:

```
error[E0559]: variant `WriteCommand::CreateRun` has no field named `config_hash`
  --> crates/am-store/tests/audit.rs:59:13
error[E0559]: variant `WriteCommand::CreateRun` has no field named `config_hash`
  --> crates/am-store/tests/run_journal.rs:88:17
error[E0609]: no field `config_hash` on type `RunRow`
   --> crates/am-store/tests/run_journal.rs:146:20
error[E0599]: no method named `runs` found for struct `Reader` in the current scope
error: could not compile `am-store` (test "audit") due to 1 previous error
error: could not compile `am-store` (test "run_journal") due to 28 previous errors
```

**Per target** (`cargo nextest run -p <crate> --test <target>`, each exit 101):

| Target | First failure lines |
|--------|---------------------|
| `-p am-store --test audit` | `error[E0559]: variant WriteCommand::CreateRun has no field named config_hash` (audit.rs:59) |
| `-p am-store --test run_journal` | 28 errors: `CreateRun` has no `config_hash`; no field `config_hash`, `created_at`, `finished_at` on `RunRow`; no field `started_at`, `finished_at` on `StepRow`; `no method named runs found for struct Reader` |
| `-p am-engine --test run` | `error[E0432]: unresolved import am_engine::run` (run.rs:42: `could not find run in am_engine`), then the same missing `RunRow`, `StepRow` and `Reader::runs` (20 errors) |
| `-p am-app --test run_cli` | `error[E0432]: unresolved imports am_app::cli::RunContext, am_app::cli::run, am_app::cli::status` (run_cli.rs:42), then the same missing store fields and `Reader::runs` (16 errors) |
| `-p am-app --test audit_cli` | `error[E0559]: variant WriteCommand::CreateRun has no field named config_hash` (audit_cli.rs:83) |
| `-p am-e2e --test rule_16_audit_log` | `error[E0559]: variant WriteCommand::CreateRun has no field named config_hash` (rule_16_audit_log.rs:63) |

Failing tests (none compiles yet):
- am-store `run_journal`: `create_run_pins_the_config_hash_and_audits_it`, `a_run_without_a_snapshot_has_no_hash`, `the_writer_stamps_step_and_run_times`, `runs_are_listed_newest_first_with_hash_and_times`, `migration_0006_adds_the_pin_and_the_times_to_an_older_database`; am-store `audit` (all, through the `CreateRun` literal; `args_are_redacted_ids_hashes_counts_and_statuses` holds the new `run.created` arguments).
- am-engine `run`: `one_step_succeeds_and_the_journal_holds_it`, `every_transition_is_committed_before_the_provider_is_called`, `two_steps_run_in_order_each_with_its_own_request`, `a_provider_error_fails_the_step_and_the_run`, `a_tool_request_fails_the_step_because_no_tools_exist_yet`, `a_cut_off_or_cancelled_turn_fails_the_step`.
- am-app `run_cli`: `the_one_step_fixture_is_a_valid_project`, `run_one_step_succeeds_prints_the_run_and_pins_the_snapshot`, `the_steps_max_tokens_is_the_output_limit`, `at_the_model_call_the_journal_is_committed_and_status_reads_it`, `two_steps_run_in_order_each_with_its_own_seat_and_model`, `status_lists_runs_newest_first_with_steps_hash_and_duration`, `status_with_no_runs_says_so`, `status_reads_while_the_project_is_held`, `status_on_an_older_database_says_to_open_the_project_once`, `status_without_a_runtime_folder_says_so`, `a_provider_failure_fails_the_run_and_prints_the_reason`, `a_tool_request_fails_the_run_because_no_tools_exist_yet`, `an_unknown_workflow_names_the_ones_that_exist`, `a_config_error_starts_no_run_and_points_at_validate`, `a_pending_config_migration_starts_no_run_and_says_why`, `a_seat_without_a_model_refuses_before_the_run_is_created`, `a_refusing_provider_factory_starts_no_run`, `while_the_store_is_held_the_run_is_refused_naming_the_holder`, `another_macs_marker_refuses_the_run_pointing_at_serve`, `bad_usage_exits_two`, `the_binary_wires_no_provider_and_starts_no_run`; am-app `audit_cli` and am-e2e `rule_16_audit_log` (all, through the `CreateRun` literal).

Still green with the new test files and fixture in place: `cargo fmt --all -- --check`, `cargo xtask denylist`, `cargo xtask runtime-paths`, `cargo xtask check-forbid`, `cargo nextest run -p am-config -p xtask` (305 passed).

**What the tests pin** (names chosen by the tests under `docs/owner-standing-rules.md`: field names and test structure; each test file's header repeats its part):
- am-store:
  - `WriteCommand::CreateRun { .., config_hash: Option<String> }`. `None` is only for a run with no snapshot (the spike's hard-coded workflow, the older tests); the column is NULL for it and for rows from before 0006.
  - The `run.created` audit arguments are `{config_hash, origin, steps, workflow}` (`config_hash` is `null` when there is none).
  - `RunRow` gains `config_hash: Option<String>`, `created_at: String`, `finished_at: Option<String>`. `StepRow` gains `started_at: Option<String>`, `finished_at: Option<String>`.
  - `Reader::runs() -> Result<Vec<RunRow>, StoreError>`, newest first.
  - `StartStep` stamps `started_at`; `FinishStep` and `SettleAndFinishStep` stamp the step's `finished_at`; `FinishRun` stamps the run's.
- am-engine: `am_engine::run::{RunPlan, StepPlan, run_plan}`.
  - `RunPlan { workflow: String, config_hash: String, steps: Vec<StepPlan> }`.
  - `StepPlan { key: String, system: Vec<Instruction>, instruction: Instruction, model: String, max_output_tokens: u32, provider: Arc<dyn LlmProvider> }`.
  - `run_plan(store: &Store, started_by: Actor, origin: &str, plan: &RunPlan, cancel: CancellationToken).await` returns `Ok` with `run_id: String`, `status: String`, `reason: Option<String>`.
  - A failed run is an `Ok` outcome with `status == "failed"` and the reason, so the caller still has the run id; `MaxTokens` and `Cancelled` end as `failed` too, as the plan says. Later steps stay `queued`.
  - `run.created` is by `started_by`; every other entry is by `System { component: "engine" }`. `call_seq` is 1; the reservation is 0.
- am-app: `am_app::cli::{RunContext, run, status}`.
  - `RunContext { places: Places, identity: Arc<dyn ProjectIdentity>, providers: Arc<dyn Fn(&am_config::ModelRef) -> Result<Arc<dyn LlmProvider>, String> + Send + Sync> }`.
  - `run(args: &[String], ctx: RunContext, out: &mut dyn Write, err: &mut dyn Write).await -> i32`, with `args` = `<workflow> [--project <folder>]`.
  - `status(args: &[String], places: &Places, out: &mut dyn Write, err: &mut dyn Write) -> i32`, with `args` = `[--project <folder>]`.
  - `run` prints `run <uuid-v7> <status>` on `out`; refusals go to `err`; usage errors print `usage: agentic run` or `usage: agentic status` and exit 2.
  - `status` prints one line per run (id, workflow, status, at least the first 12 characters of the hash, and a duration written as a number followed by `s` or `ms`, or the word `running`), and under it one line per step (key, status, duration). With no run it says `no runs`; an older database or a missing runtime folder exits 1.
  - Words the tests look for: `agentic validate` (config error), `migrat` (pending migration), the seat id and `model` (no model), the unknown name and the existing workflow ids, the holder's name (busy), `agentic serve` (another Mac's marker), `tool` and `not available`, `once` (older database), `no model provider` (the binary).

**Checked that the tests can pass:** in a throwaway copy of the repo outside the project (deleted afterwards), a rough prototype of the store, engine and CLI changes made all 259 tests of the four crates pass and `cargo clippy --all-targets -- -D warnings` clean. Nothing of it is in this repo.

**For the build to look at:**
- **AD-15, CLI manual run start.** AD-15 lists "a manual run start" among the CLI-class decisions that need an interactive TTY and no enabled stdio MCP server. This plan's `agentic run` has no terminal in its context, and its Two steps row runs the valid fixture, whose `tools.yaml` enables a stdio MCP server. The tests follow the plan as written; whether `agentic run` must apply AD-15's rule is a security question for the owner or Sofia, not decided here.
- **`just validate`** loops over `fixtures/projects/valid*/` and `anatomy/` only, so it does not cover `fixtures/projects/one-step/`. `run_cli.rs` checks the fixture loads with no finding; adding the folder to the justfile loop is left to the build.
- **A step with no `seat`** is already a validation error today (`step is missing 'seat'`), so through the CLI it is the Invalid config row; no separate CLI test exists for it.

### Red run 2 (2026-11-22, after the AD-15 amendment; tests only, no `src/` file touched)

The plan gained "Who may start a run" and the rows Not a terminal, MCP or render enabled and Binary's providers; the Two steps row no longer uses the valid fixture. This supersedes the AD-15 item of the first entry above.

**Changed file:** `crates/am-app/tests/run_cli.rs` only (now 24 tests). The store and engine tests, the rewritten older tests and the fixture are as in the first entry.

- Every in-process `run` passes a terminal: `RunContext { .., terminal: Box<dyn Terminal>, .. }`, an interactive `am_testkit::FakeTerminal::new()` unless the test is about the refusal. The Run row also checks that nothing is read from it.
- New `without_an_interactive_terminal_the_run_is_refused_before_the_project_is_opened` (Not a terminal, in process, `FakeTerminal::piped()`):
  - exit 1, the words `interactive terminal` and `agentic run`;
  - on a folder never opened: no id minted, no runtime folder, no marker, no read from the terminal, no provider asked for;
  - on a project whose database is at schema 5: the database's bytes are unchanged (no migration, no run row) and no lock is left held.
- New `the_binary_refuses_run_with_piped_stdin_and_status_needs_no_terminal` (the real binary; it replaces `the_binary_wires_no_provider_and_starts_no_run`):
  - `agentic run hello` with piped stdin exits 1 naming the terminal; the database's bytes are unchanged, there is no marker, no lock and no run row;
  - `agentic status` with piped stdin exits 0 and says there are no runs.
- New `an_enabled_stdio_mcp_server_or_rendering_source_refuses_the_run` (MCP or render enabled), on three projects whose config is valid:
  - the one-step fixture with a `tools.yaml` that enables the stdio server `notes`;
  - the valid fixture as it is;
  - the `sources-ok` fixture (its Source `docs` has `render: true`) given the one-step seat and workflow.
  - Each exits 1 naming what is enabled (`MCP` and `notes`, or `render` and `docs`), with no run row and no provider asked for.
- New `the_binarys_own_provider_factory_wires_no_provider` (Binary's providers): `am_app::cli::unwired_providers()` passed to `run` in process with an interactive fake exits 1 saying `no model provider is wired` and `agentic run`; no run row.
- Two steps and Status now run on a project the test writes (`Env::two_step`): seats `alpha` (`local-lmstudio`, `example-local`) and `beta` (`local-ollama`, `example-small`), workflow `digest` with `gather` and `summarise` (`max_tokens: 256`). The Two steps row now also proves each step's own model id and output limit.
- `a_refusing_provider_factory_starts_no_run` runs on the one-step fixture.

**Newly pinned:** `RunContext.terminal: Box<dyn Terminal>`, and `am_app::cli::unwired_providers()` returning the factory type of `RunContext.providers` (the one `main` gives the binary).

**Command:** `cargo nextest run --no-fail-fast -p am-store -p am-engine -p am-app -p am-e2e` — exit 101; the store, engine and older-test failures are the first entry's (`CreateRun` has no `config_hash`; no `config_hash`, `created_at`, `finished_at` on `RunRow`; no `started_at`, `finished_at` on `StepRow`; no `Reader::runs`; `unresolved import am_engine::run`).

**`cargo nextest run -p am-app --test run_cli`** — exit 101:

```
error[E0432]: unresolved imports `am_app::cli::RunContext`, `am_app::cli::run`, `am_app::cli::status`, `am_app::cli::unwired_providers`
  --> crates/am-app/tests/run_cli.rs:53:19
   |
53 | use am_app::cli::{RunContext, run, status, unwired_providers};
   |                   ^^^^^^^^^^  ^^^  ^^^^^^  ^^^^^^^^^^^^^^^^^ no `unwired_providers` in `cli`
error[E0609]: no field `config_hash` on type `RunRow`
error[E0599]: no method named `runs` found for struct `Reader` in the current scope
error: could not compile `am-app` (test "run_cli") due to 16 previous errors
```

Failing tests of `run_cli` (none compiles yet): the first entry's list without `the_binary_wires_no_provider_and_starts_no_run`, plus `without_an_interactive_terminal_the_run_is_refused_before_the_project_is_opened`, `an_enabled_stdio_mcp_server_or_rendering_source_refuses_the_run`, `the_binarys_own_provider_factory_wires_no_provider` and `the_binary_refuses_run_with_piped_stdin_and_status_needs_no_terminal`.

Still green: `cargo fmt --all -- --check`, `cargo xtask denylist`, `cargo xtask runtime-paths`.

**Checked that the tests can pass:** again in a throwaway copy outside the project (deleted afterwards), a rough prototype made all 262 tests of the four crates pass with clippy `-D warnings` clean. It checked the terminal first, then `am_config::decisions_locked`, then opened the project.

**Left open by the tests:**
- For MCP or render enabled they do not say whether the project is opened before the refusal; only that no run row exists and the project is closed afterwards.
- The usage rows run with an interactive terminal, so they do not say whether a usage error or the terminal rule comes first.

### Build (2026-11-22, after both red runs; no test of the red runs changed)

**Changed:**
- `crates/am-store`: `migrations/0006_run_pin_and_times.sql` (the four columns); `CreateRun.config_hash` (stored, audited as an argument); `StartStep` stamps `started_at`, `FinishStep` / `SettleAndFinishStep` the step's `finished_at`, `FinishRun` the run's; `RunRow` and `StepRow` gain the fields; `Reader::runs()` and `Reader::schema_version()`.
- `crates/am-engine/src/run.rs` (new): `RunPlan`, `StepPlan`, `RunOutcome`, `RunError`, `run_plan`. `journal.rs`: `config_hash: None` on the spike's `CreateRun`, nothing else.
- `crates/am-app/src/run.rs` (new): `RunContext`, `Providers`, `unwired_providers`, `run`, `status`. `cli.rs`: the re-exports, `OwnerSide`, `project_load_options` (taken out of `validate`, unchanged). `main.rs`: `run` and `status` dispatch and usage; `owner_side()`.
- `justfile`: `fixtures/projects/one-step/` added to the `validate` loop.

**Decided under `docs/owner-standing-rules.md` (engineering choices):**
- **Check order of `agentic run`:** usage, terminal, `am_config::decisions_locked`, the open, pending migration, config errors, the workflow, `decisions_locked` again on what the files hold now, then seats, models and providers. The project is not opened for the terminal or the MCP/render refusal.
- **A failed step has no output.** The reason is in the outcome and printed (`agentic: run <id> failed: step … failed: …`); the store has no column for it, so `agentic status` shows `failed` without the reason.
- **`config_hash` must be a sha256 hex string** (64 lowercase hex characters) or the command is refused: it is copied into the audit arguments (AD-25).
- **`started_at` is kept** if a step is started twice (`COALESCE`), so the spike's resume never moves it.
- **A plan with no steps is refused** by the engine before the run is created (`RunError::NoSteps`); the config check already makes a workflow without steps an error.
- **A seat with no instructions sends no system part,** never an empty one.
- **Runs are listed by `created_at` descending, then insertion order.**
- **`status` prints** `<id>  <workflow>  <status>  snapshot <first 12 of the hash>  <duration>` and, indented under it, `<step>  <status>  <duration>`. Durations are seconds with milliseconds (`1.042s`); `running` while started and not finished; `-` for a step that never started or a row from before the times were kept.
- **The open's report lines are printed** on stdout as `serve` prints them (`agentic: project <p>: <line>`).

**The release guard (`xtask/tests/release_deps.rs`, unchanged):** it allows exactly one `terminal:` field in `main.rs`, the real terminal. A second context built there with its own `OwnerTerminal::new()` fails it (`found 2 terminal: fields`; the prototype of the red runs did not run xtask). So `main.rs` now builds the owner's side once (`owner_side()` returns `am_app::cli::OwnerSide { places, identity, terminal }`), and `accounts` and `run` both take their terminal from it. `accounts` gets the same values as before. This touches story 2.1's wiring: for the reviewer to confirm.

**Extra tests (written with the code, for behaviour the red runs do not pin):** am-store `a_config_hash_that_is_not_a_sha256_is_refused`; am-engine `a_plan_with_no_steps_creates_no_run`; am-app `run::tests` (durations, the `--project` flag).

**Verified:**
- `cargo nextest run -p am-store -p am-engine -p am-app -p am-e2e`: 267 passed, 5 skipped (the 262 of the red runs' prototype count plus the 5 above).
- `just check`: green (fmt, clippy `-D warnings`, 629 tests, am-store doctests, check-deps, check-forbid, denylist, runtime-paths, validate with the one-step fixture).
- The real binary on a pseudo-terminal, with HOME in a temporary folder: `agentic run hello` opens the project and exits 1 with "no model provider is wired into `agentic run` yet … No run was started"; `agentic status` then says there are no runs; with piped stdin `run` is refused before the open.

**For the review:**
- With the real binary in a terminal, `agentic run` on a folder never opened mints the project id and makes the runtime folder (the full open) before it refuses for the missing provider. The plan orders it so; the refusal can only come once a seat's model is known.
- The run's snapshot is read from the files on disk after the open (`load_in`, as the plan says), not from a config session's accepted bytes: an outside edit that no Reload has taken in is run as it is.
- A run killed mid-call (Ctrl-C) stays `running` in `status`; cancel and resume are stories 11 and 9.

### Red run 3 (2026-11-22, after the review of the build; tests only, logged before the fixes)

The review asked for six fixes. Their tests were written first; no `src/` logic changed before this entry (the am-app unit test below lives in `src/run.rs`'s test module and names code that does not exist yet).

**Tests written:**
- `crates/am-app/tests/run_cli.rs` (4 new, now 28):
  - `a_step_this_version_cannot_honour_refuses_the_run_by_name`: a step with `review:`, `handoff:`, `output_schema:` or a `seats:` fan-out, each on a valid config, is refused naming the step (`` `check` ``) and the key, with `agentic run` and `cannot honour`; no provider asked, no run row.
  - `a_file_changed_outside_the_app_refuses_the_run`: after a run (the open wrote the id to company.yaml), a hand edit of company.yaml refuses the next run with `company.yaml`, `changed on disk outside the app` and `Reload`; no new run row; put back, the run starts.
  - `a_broken_tools_file_refuses_the_run_pointing_at_validate`: `tools.yaml` with an error says `has an error` and `agentic validate`, never `is enabled` or `remove or disable`; the project is not opened.
  - `a_server_the_pinned_snapshot_enables_refuses_the_run`: an identity adapter enables a stdio server during the open (after the check before it); the run is refused naming it. Coverage of the second child check; it passes on the old code too, which re-read the disk (the difference, a change between the snapshot's load and the re-read, cannot be made to happen in a test).
- `crates/am-engine/tests/run.rs` (1 new): `a_store_error_after_the_run_was_created_fails_the_run_and_names_it`. The seam is at the engine level on the real store: a provider that settles its own call through the store's writer, so the engine's `SettleAndFinishStep` is refused (`call … is not open`). Pins `RunError::Stopped { run_id, status, source }`.
- `crates/am-app/src/run.rs` test module (1 new): `a_run_the_store_stopped_is_named_with_its_state`, for the CLI's line (`run <id> failed|running`, the store's error, `agentic status`). At the CLI level no seam reaches a failing store (the command holds the store's lock and am-app has no direct database access), so the printing is tested on a constructed engine error. Pins `Stop::Engine(RunError)` and `report(..)`.
- `xtask/tests/release_deps.rs` self-test extended: one construction shared by shorthand is clean; a second `OwnerTerminal::new()` (in main or elsewhere in am-app), `OwnerTerminal::default()` outside its file, a shorthand `terminal` bound from another constructor, another `Terminal` type and `dyn Terminal` in main are each found; a comment naming them is not.

**Red output:**
- `cargo nextest run -p am-app --test run_cli --no-fail-fast`: 28 run, 25 passed, 3 failed:
  - `a_step_this_version_cannot_honour_refuses_the_run_by_name`: `assertion left == right failed: review:` (exit 0: the run was created and succeeded);
  - `a_file_changed_outside_the_app_refuses_the_run`: `assertion left == right failed` (exit 0: the run started on the edited file);
  - `a_broken_tools_file_refuses_the_run_pointing_at_validate`: panicked at the `agentic validate` assertion on ``agentic: `agentic run` cannot start a run from the command line while tools.yaml has an error, so it is read as enabling a child that runs as you: … remove or disable it first``.
- `cargo nextest run -p am-engine --test run`: `error[E0599]: no variant named Stopped found for enum am_engine::run::RunError`.
- `cargo nextest run -p am-app --lib`: `error[E0433]: cannot find type Stop in this scope`, `error[E0599]: no variant named Stopped`, `error[E0425]: cannot find function report in this scope`.
- `cargo nextest run -p xtask --test release_deps`: `the_terminal_guard_finds_a_fake_a_feature_gate_and_another_main` panicked at release_deps.rs:472 with `[]` (the second `OwnerTerminal::new()` is not found by the old `terminal:` count).

### Review fixes (2026-11-22, after red run 3)

This entry supersedes, in the Build entry above: the check order, "`decisions_locked` again", "the release guard (unchanged)", and the review note that an outside edit is run as it is.

**Changed:**
- **Steps the run cannot honour.** `am_config::Step` gains `run_graph: Vec<String>`: the run-graph keys the step declares (`seats`, `join`, `over_folder`, `routes`, `files`, `handoff`, `review`, `max_parallel`), names only, sorted; a key written with no value counts. `agentic run` refuses a workflow with a step that declares one, or an `output_schema`, before any provider is asked for and before the run is created, naming the step and the keys. The no-seat wording no longer says to add `seat:`.
- **Outside change (AD-16).** After the open and before the snapshot is loaded, `am_config::outside_changes` on the store's History refuses the run when a file changed outside the app, in story 2.1's words.
- **The second child check reads the pinned snapshot.** New `ConfigSnapshot::owner_children()` (the same reasons, from the snapshot's tools and Sources) replaces the second disk read.
- **A file error is not an enabled child.** New `am_config::owner_children(root) -> OwnerChildren { enabled, broken }`; `decisions_locked` is built on it and gives the same words as before. With nothing enabled and a `tools.yaml` or Source file in error, `agentic run` says the file has an error and points at `agentic validate`.
- **Release guard (`xtask/tests/release_deps.rs`).** The `terminal:` text count is replaced by `construction_offences`: outside `terminal.rs`, am-app's code names `OwnerTerminal` only as the one `OwnerTerminal::new()`, exactly once, in `main.rs`; `main.rs` names no other type ending in `Terminal` nor the trait; every `terminal:` field and `terminal =` binding there is given exactly `Box::new(am_app::terminal::OwnerTerminal::new())`. Whole-line comments are not read.
- **A store error after the run was created.** `run_plan` tries once to finish the open step (`FinishStep`, `failed`) and the run (`FinishRun`, `failed`) and returns `RunError::Stopped { run_id, status, source }` (`status` is `failed`, or `running` when the store refused that too; `source` is boxed for clippy's `result_large_err`, so the am-app unit test of red run 3 wraps its constructed error in `Box::new`). `RunError::Journal` is gone (a step-count mismatch is a `Stopped` run). The CLI prints `run <id> <status>` and, on stderr, the store's error and that `agentic status` shows the run.

**Check order of `agentic run` now:** usage; terminal; `owner_children` on the files (enabled, or a file error); the open; outside changes; pending migration; config errors; the pinned snapshot's children; the workflow; steps it cannot honour; seats, models and providers.

**Left as it is:** a call the store could not settle stays as the journal has it (the cleanup finishes the step and the run only; marking it lost would settle a ledger row, which is not this story's to decide). If the last `FinishRun(succeeded)` is the command that fails, the cleanup records the run as `failed`.

**Verified (only what covers the edited files):** `cargo nextest run -p am-config -p am-engine -p am-app -p xtask`: 456 passed, 2 skipped; `cargo clippy -p am-config -p am-engine -p am-app -p xtask --all-targets -- -D warnings` clean; `cargo fmt --all -- --check`, `cargo check --all-targets`, `cargo xtask denylist|check-deps|check-forbid|runtime-paths` clean. `just check` was not run again here.

### Red run 4 (2026-11-22, after the second review; tests only, logged before the fixes)

**The seam.** The real store gives no way to make one chosen command fail and the next succeed (no fail point; a `FinishRun` fails only on a database error). So the driver gets a private entry, `run_on`, that takes the function executing a write command; `run_plan` passes the store's writer. The tests below live in `crates/am-engine/src/run.rs`'s test module (they need the private entry) and run the driver on a real store whose writer refuses the commands a test picks.

**Tests written:**
- `crates/am-engine/src/run.rs` test module (3 new):
  - `a_refused_last_finish_is_retried_with_the_status_the_run_earned`: the first `FinishRun` is refused after every step succeeded; the error's `status` and the journal say `succeeded`.
  - `a_store_that_refuses_every_finish_leaves_the_run_running_and_says_so`: `status` is `running`, as the journal holds.
  - `a_store_error_after_a_failed_step_keeps_the_steps_reason`: a provider error (HTTP 500), then the settlement refused, or the run's first finish refused: `Stopped.reason` holds the step's reason, `status` is `failed`, the step and the run are journaled `failed`.
- `crates/am-engine/tests/run.rs`: the `Stopped` pattern of `a_store_error_after_the_run_was_created_fails_the_run_and_names_it` gains `reason` (asserted `None`).
- `crates/am-app/src/run.rs` test module: `a_stopped_run_prints_the_steps_reason_and_exits_by_its_recorded_status` (the step's reason beside the store's error; exit 0 only when the journal holds `succeeded`); the older `a_run_the_store_stopped_is_named_with_its_state` gains `reason: None` in its constructed error.

**Pinned:** `RunError::Stopped { run_id, status, reason: Option<String>, source }`; the private `run_on(store, exec, started_by, origin, plan, cancel)`.

**Red output:**
- `cargo nextest run -p am-engine`: `error[E0425]: cannot find function run_on in this scope`; `error[E0026]: variant run::RunError::Stopped does not have a field named reason` (twice); `could not compile am-engine (lib test)`.
- `cargo nextest run -p am-app --lib`: `error[E0559]: variant am_engine::run::RunError::Stopped has no field named reason` (twice).

### Second review fixes (2026-11-22, after red run 4)

This entry supersedes, in "Review fixes" above: "If the last `FinishRun(succeeded)` is the command that fails, the cleanup records the run as `failed`", and the shape of `RunError::Stopped`.

**Changed:**
- **The cleanup records what the run earned.** The driver keeps its progress (the open step, a failed step's reason, the status it is about to record). After a store error the cleanup records that status when every started step had finished (`succeeded` after the last step, `failed` after a failed one), and `failed`, with the open step, only when a step did not finish. `RunError::Stopped.status` is what was recorded, or `running` when the store refused that too.
- **The step's reason is kept.** `RunError::Stopped` gains `reason: Option<String>`, set from the moment a turn's failure is known. The CLI prints it on its own line beside the store's error.
- **Exit code of a stopped run:** it follows the status the journal holds, so 0 only when the run is recorded `succeeded` (the store's error is still printed on stderr). Decided here as an engineering choice: the command's exit code says how the run ended.
- **The seam:** `run_plan` calls the private `run_on`, which takes the function that executes a write command.
- **Check 5 says what it does.** The module doc and `no_outside_change`'s doc in `crates/am-app/src/run.rs` now say that only files the app has written (those with a History entry) are judged, and that a hand edit of a workflow or seat file the app never wrote is not refused. The test is renamed `a_file_the_app_wrote_that_was_changed_outside_it_refuses_the_run` (its assertions are unchanged; Red run 3 names it by its old name).

**No seam:** none needed beyond `run_on`. At the CLI level a failing store is still not reachable, so the CLI's lines are tested on constructed engine errors, as before.

**Verified (only what covers the edited files):** `cargo nextest run -p am-engine -p am-app`: 155 passed; `cargo clippy -p am-engine -p am-app --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.

## Plan Change Log

- 2026-11-22 (before any `src/` change; found by the tests-first pass): the intent contract left out AD-15's rule that a manual run start is a CLI-class decision. Added "Who may start a run" (interactive terminal through the `Terminal` port; refused while a stdio MCP server or a rendering Source is enabled), three matrix rows in place of the Binary row, and the Two steps row no longer uses the valid fixture. AD-15 has one reading, so this is a correction, not a choice. KEEP everything else, including the tests already written.
- 2026-11-22: accepted from the tests-first pass: a failed turn's partial text is not stored as the step's output; `MaxTokens` and `Cancelled` end the step `failed`; the API names its tests pin.

## Review Triage Log

### 2026-11-22 — Review pass
- verdicts: 8 findings — high 1, medium 3, low 3, false 1, maybe-false 0
- findings:
  - `[high]` `[patch]` A step's run-graph keys (`seats`, `join`, `over_folder`, `routes`, `files`, `handoff`, `review`, `max_parallel`) and its `output_schema` are dropped: the step runs as one plain turn and the run reports `succeeded`, so a declared review gate or hand-off is skipped without a word. Fix: the config `Step` carries which run-graph keys it declares. `agentic run` refuses, before the run is created, a workflow with any step this driver cannot honour, naming the step and the key. The no-seat wording is corrected for a `seats:` step.
  - `[medium]` `[patch]` The run is built from whatever is on disk with no outside-change check, while AD-16 says nothing applies until Reload and story 2.1's `accounts` refuses in that state. Fix: `agentic run` refuses when `am_config::outside_changes` names a file, with 2.1's wording. The widening hold needs `is_widening` and the approved grant set (epic 4 stories 17 and 18), and is recorded under residual risks.
  - `[low]` `[patch]` The second child check reads the disk again and not the pinned snapshot. Fix: it checks the snapshot's tools and Sources.
  - `[low]` `[patch]` An error in `tools.yaml` or `sources/*` gets the "cannot start a run while … enables" wording and not the `agentic validate` pointer. Fix: that case says the file has an error and to run `agentic validate`; test.
  - `[medium]` `[patch]` The release guard counts the text `terminal:` in `main.rs`, and field shorthand now hides both contexts from it. Fix: the guard asserts that `OwnerTerminal::new()` is the only terminal construction in am-app's sources, appears exactly once, and that `main.rs` names no other `Terminal`; its self-test covers a second construction and a shorthand context fed from something else.
  - `[medium]` `[patch]` A store error after the run row is committed leaves the run `running` for good, and the CLI cannot name it. Fix: the engine tries once to finish the run as `failed` and returns the run id with the error. The CLI prints `run <id>` and what state it is in.
  - `[low]` `[reject]` In a terminal, every `agentic run` does the full project open (mint, runtime folder, migration) before a later refusal. Opening is what a run command does, as `serve` does; the snapshot hash must be taken after the mint; no run row is left. Reordering would mean loading the config twice.
  - `[false]` `[reject]` The intent contract gained AD-15's run-start rule without a recorded owner decision.
    - The ticket's intent did not change.
    - The plan was this run's own draft, corrected before any `src/` change to an AD that already binds the story and has one reading.
    - The owner standing rules ask for the owner on changing an AD or a ticket's intent, not on following one.
    - The correction was reported to the owner in the session when it was made.

### 2026-11-22 — Follow-up review pass (the six patches above)
- verdicts: 4 findings — high 0, medium 0, low 4, false 0, maybe-false 0
- findings:
  - `[low]` `[patch]` The outside-change check judges only files the app has written (those with a History entry), so a hand edit of a workflow or seat file the app never wrote is not refused, while the module doc and the test name say "a file changed outside the app". Fix: the doc and the test name say what is covered. The full rule (every file, and the widening hold) needs the approved grant set of epic 4 story 18; recorded under residual risks.
  - `[low]` `[reject]` The outside-change check and the snapshot come from two disk reads, so a file rewritten between them is pinned unchecked. This needs an editor save within the same milliseconds; binding the check to the pinned bytes needs per-file hashes on `ConfigSnapshot`, a new API for a narrow window.
  - `[low]` `[reject]` Another workflow's event trigger on this workflow's completion does not fire and nothing says so. The epic's Boundaries put event triggers and trigger chains in epic 4 story 15, so the intent excludes it; recorded under residual risks.
  - `[low]` `[patch]` The engine's cleanup after a store error always records `failed`, even when the failing command was the final `FinishRun("succeeded")`, and a step's provider reason is dropped when a store error follows. Fix: the cleanup records the status the engine was about to record, and `Stopped` carries the step's reason.

## Design Notes

Decided under `docs/owner-standing-rules.md` (engineering choices, recorded):
- **"On FakeLlm" means in process.** The fake lives in am-testkit and may never reach the binary (the release guard). The command takes its provider factory through a context, and tests inject FakeLlm.
- **The binary wires no real provider yet.**
  - A real call would spend with no cost ledger (story 7) and before the secrets hardening (epic 2 story 5). Both are owner stop rules: money, secrets.
  - The story that resolves a seat's model and fallback (epic 2 story 6) wires them.
- **Full open, not the read-only one.** A run is a project open like `serve`: it migrates the store and holds the lock.
- **Durations come from the store's own timestamps.** The writer stamps time itself, and passing a clock through commands is not this story's job.
- **A zero reservation** keeps `ReserveCall` unchanged. Amounts arrive with story 7.

## Verification

**Commands:**
- `cargo nextest run -p am-store -p am-engine -p am-app -p am-e2e` -- expected: all pass.
- `just check` -- expected: green.

## Auto Run Result

Status: built (2026-11-22).

**Summary:**
- **`agentic run <workflow>`:**
  - It is a CLI-class decision (AD-15): it runs only on a real interactive terminal.
  - It refuses while an stdio MCP server or a rendering Source is enabled, and while a file the app wrote was changed outside it.
  - It opens the project as `serve` does, loads the config and pins the snapshot's hash on the run.
  - It refuses, by step and key, a workflow whose steps declare what this driver cannot honour yet (review, hand-off, fan-out, routing, output schema).
  - For each step it commits the step's start and the call's reservation before the model is called, makes one turn, and commits the settlement with the step's output.
  - The binary wires no model provider yet and says so.
- **`agentic status`:** lists runs newest first with workflow, status, snapshot hash and duration, and their steps. It reads without the lock, from any stdin.

**Files changed:**
- `crates/am-store`: migration `0006` (`runs.config_hash`, `runs.finished_at`, `step_runs.started_at`, `step_runs.finished_at`); `CreateRun.config_hash` (audited); `Reader::runs` and `Reader::schema_version`.
- `crates/am-engine/src/run.rs` (new): `RunPlan`, `StepPlan`, `run_plan`, and the cleanup after a store error.
- `crates/am-config`: `Step.run_graph`; `owner_children` on the snapshot and on a project root.
- `crates/am-app/src/{run,cli,main}.rs`: `run`, `status`, `RunContext`, `unwired_providers`, and one shared owner terminal for `accounts` and `run`.
- `fixtures/projects/one-step/`; `justfile` (the fixture is validated).
- Tests: `crates/am-app/tests/run_cli.rs`, `crates/am-engine/tests/run.rs`, `crates/am-store/tests/run_journal.rs`, `xtask/tests/release_deps.rs` (the construction guard), and the older tests' `CreateRun` literals.

**Review:** two quick passes; the Review Triage Log has every row.
- Pass 1, 8 findings: 6 patched (1 high, 3 medium, 2 low), 1 low rejected (the full open before a later refusal), 1 false (the AD-15 correction of this run's own draft plan, which the owner has since confirmed is the right way).
- Pass 2 (follow-up, because pass 1 patched a high), 4 low findings: 2 patched, 2 rejected (two disk reads; event triggers are epic 4 story 15's).
- Nothing deferred.
- Follow-up recommended: false. Pass 2 patched no high.

**Verification:**
- `just check` passes (639 tests, 7 skipped).
- Every matrix row has a named test that ran and passed.
- Red runs 1 and 2 were logged before any `src/` change, and red runs 3 and 4 before each round of review fixes.

**Residual risks:**
- **Hand edits to unwritten files:** the outside-change check judges only files the app has written. A hand edit of a workflow or seat file is run as it is, and the widening hold on such edits needs the approved grant set (epic 4 stories 17 and 18).
- **No provider in the binary:** no model provider is wired, so nothing can be run from a terminal yet. Epic 2 story 6 wires them; cost settlement is story 7.
- **Killed or stopped runs:** a killed run stays `running` (resume is story 9, cancel story 11). After a store error, an unsettled call is left as it is, because settling it writes the ledger.
- **Missing from the journal:** a failed step's reason is printed by `run` but not stored, so `status` shows `failed` without why. Prompts and responses are not journaled yet (story 5).
- **Event triggers:** another workflow's trigger on this workflow's completion does not fire (epic 4 story 15).
- **Coverage gate:** the 80% gate on am-engine and am-store (R11) has no CI job yet.
