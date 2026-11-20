---
title: 'Test kit fakes'
type: feature
ticket: '3'
created: '2026-11-20'
status: built
baseline_revision: '34d1c658731e8da2c70d0ea2c081782ddf487c54'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/epic-foundations-rust-spike.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Every later story tests through `am-testkit` (epic R8, AD-33), but the spike's kit is thin. `FakeLlm` streams text with usage and honours cancel, but it cannot script tool calls, a hang that times out, or a provider failure. The recorded-HTTP player and the clock and secret fakes have no tests of their own. Nothing proves the kit works without the network.

**Approach:** Finish `am-testkit` behind the existing `am-domain` ports: `LlmProvider`, `Clock`, `SecretStore` and `EgressClient`. Port signatures don't change. **Tests first (owner rule, 2026-11-20):** write `crates/am-testkit/tests/` from the ticket's verify line, run it and record the red run, then implement.

## Boundaries & Constraints

**Always:**
- Tests first, with the red run recorded in Implementation Notes before any implementation.
- No network: a request without a scripted or recorded route fails with an `EgressError` naming its URL, and no test constructs `am_net`'s real client.
- Time-dependent tests use tokio's paused clock, so they run instantly and deterministically.
- `SecretString` stays without Debug, Display or Serialize, and tests use canary values only.
- Stay inside `crates/am-testkit`, except the two `ScriptedTurn` call sites in `crates/am-engine/src/journal.rs` tests, which move to the new constructors.

**Never:**
- Change an `am-domain` port signature or add a `ProviderError` variant. A timeout is the caller's deadline elapsing on a hanging turn.
- Real keychain or network access.
- New dependencies beyond the workspace's (tokio's `test-util` feature for the paused clock is allowed as a dev-dependency feature).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Streamed text | Turn "alpha beta gamma" with usage 10/3 | 3 `TextDelta` events in order, then `Usage`; outcome text equal, `EndTurn` | — |
| Two tool calls | Turn with 2 `ToolCall`s and usage | 2 `ToolCall` events in order, then `Usage`; outcome carries both, `StopReason::ToolUse` | — |
| Timeout | Hanging turn under `tokio::time::timeout(30 s)` with paused time | The deadline elapses; the fake records that the call was made | Cancel ends the hang with `Cancelled` |
| Fault | Turn scripted to fail with `ProviderError::Http { status: 529, .. }` | `stream` returns that error; no text events | — |
| Turns in order | 3 scripted turns, 4 calls | Turns 1, 2, 3, then 3 again; `calls()` = 4; `requests()` holds the 4 requests | — |
| Clock jump | `FakeClock` at 2026-11-20T01:00Z, advanced 9 h | `now()` = 10:00Z | — |
| Secrets | `set`, `get`, `delete` of a canary under one account | get returns the canary; after delete, `None` | — |
| Recorded HTTP | Route serving `fixtures/http/anthropic/<file>` | Body equals the file bytes; the request is recorded | Unrouted URL → `EgressError` naming the URL |

</frozen-after-approval>

## Code Map

- `crates/am-testkit/src/lib.rs` (381 lines; the spike's kit):
  - **Keep:** `FakeEgress` with `respond` and `respond_if`, `requests()`, `body_dropped`, and the unrouted `EgressError::Blocked("FakeEgress has no route for …")` at about line 161. `recorded()` and `fixture()` read `fixtures/http/{anthropic,openai_compat}`.
  - **Extend:** `ScriptedTurn { text, usage, delay }` at line 198 and `FakeLlm` at 206–277. It streams word-by-word `TextDelta`s, then `Usage`, honours cancel during `delay`, and checks file parts against `ModelCapabilities`.
  - **Keep:** `FakeClock` (`at`, `set`, `advance`) at 295–320, `FakeSecretStore` (`new`, `with`) at 323–363 and `CollectEvents` at 279–293.
- `crates/am-domain/src/llm.rs`: `StreamEvent::{TextDelta, ToolCall, Usage}`, `ToolCall { id, name, arguments_json }`, `StopReason::{EndTurn, ToolUse, MaxTokens, Cancelled, Other}`, `ProviderError::{Unsupported, Egress, Http, Protocol}`. Read only; no changes.
- Callers to keep compiling:
  - `crates/am-engine/src/journal.rs`, lines 265 and 289: `ScriptedTurn { .. }` literals.
  - `crates/am-engine/src/{toolgate,scheduler}.rs` and `crates/am-e2e/tests/spike/c{1,3,4,5}_*.rs` use the other fakes, whose APIs don't change.
- `crates/am-testkit/Cargo.toml`: deps are `am-domain`, `am-net`, `tokio` and `jiff`; it has no `tests/` yet.

## Tasks & Acceptance

**Execution:**
- [x] `crates/am-testkit/tests/fakes.rs` -- FIRST: one test per I/O matrix row, written against the API below; run `cargo nextest run -p am-testkit`, see it fail to compile or fail, and record the output in Implementation Notes -- tests first
- [x] `crates/am-testkit/src/lib.rs` -- change `ScriptedTurn` to private fields with constructors: `text(s)`, `.usage(in, out)`, `.tool_call(name, args_json)` (repeatable, ids `call-1`, `call-2`, …), `.delay(d)`, `fail(ProviderError)` and `hang()`. `FakeLlm` streams text, then tool calls, then usage; it returns `ToolUse` when there are tool calls, the scripted error for a fault, and waits until cancelled for a hang. It records each `ChatRequest` (`requests()`). The doc comment at the top lists every fake -- R8
- [x] `crates/am-testkit/src/lib.rs` -- a `FakeEgress::replay(url_part, fixture_rel)` helper (status 200, SSE when the file ends `.sse`), so a recorded fixture is served in one line -- the fixture player
- [x] `crates/am-testkit/Cargo.toml` -- dev-dependency on tokio with `test-util` (paused time) if the workspace tokio lacks it -- deterministic timeouts
- [x] `crates/am-engine/src/journal.rs` -- move the two test call sites to the new constructors -- keep callers compiling

**Acceptance Criteria:**
- Given the kit, when `cargo nextest run -p am-testkit` runs with no network access, then every test passes and none sends a real request.
- Given the whole workspace, when `just check` runs, then it is green and the existing journal, ToolGate, scheduler and spike tests still pass unchanged in meaning.

## Implementation Notes

**Red run (2026-11-20, before any implementation).** `crates/am-testkit/tests/fakes.rs` written from the I/O matrix (one test per row, plus a delayed-turn test), with the tokio `test-util` dev-dependency added. `cargo nextest run -p am-testkit` failed to compile with 13 errors, all missing API:

```
error[E0599]: no associated function or constant named `text` found for struct `ScriptedTurn`   (x7)
error[E0599]: no associated function or constant named `hang` found for struct `ScriptedTurn`
error[E0599]: no associated function or constant named `fail` found for struct `ScriptedTurn`
error[E0599]: no method named `requests` found for struct `FakeLlm` / `Arc<FakeLlm>`             (x2)
error[E0599]: no method named `replay` found for struct `Arc<FakeEgress>`                        (x2)
error[E0599]: no method named `clone` found for enum `ProviderError`
error: could not compile `am-testkit` (test "fakes") due to 13 previous errors
```

The last error was a test bug: `ProviderError` is not `Clone`, and am-domain may not change. The fault test now builds the error with a closure, and the fake copies a scripted fault with an exhaustive match over `ProviderError`'s variants inside am-testkit (a new variant fails to compile there, which is the wanted signal), so a fault turn can repeat like any other.

**Green run (2026-11-20).** `ScriptedTurn` now has private fields and the constructors `text`, `fail`, `hang` with `usage`, `tool_call` (ids `call-1`, `call-2`, … per turn) and `delay`; `FakeLlm` streams text, then tool calls, then usage, returns `ToolUse` when the turn has tool calls, returns the scripted fault with no events, waits for cancel on a hang (cancel also ends a delay), and records every `ChatRequest` (`requests()`), including one its file-part check refuses. `FakeEgress::replay(url_part, fixture_rel)` serves `fixtures/http/<fixture_rel>` with status 200 and the file's bytes exactly (an `.sse` file as `text/event-stream`, one chunk per event, split without normalising; `.json` as JSON). `fixture_rel` is relative to `fixtures/http/`, matching `recorded()`. The crate doc lists every fake. The two `journal.rs` test sites use `ScriptedTurn::text(..).usage(..).delay(..)`; Cargo.lock is unchanged (`test-util` is a feature of the pinned tokio). `cargo nextest run -p am-testkit`: 10 passed. `just check`: fmt, clippy `-D warnings`, nextest (92 passed, 6 skipped, as before), check-deps, check-forbid and denylist green.

**Review fixes (2026-11-20).**
- **Concurrent calls:** `FakeLlm::stream` takes the turn index and records the request under one lock. A 16-thread test checks that each request pairs with its turn.
- **Builder misuse:** `.usage()` and `.tool_call()` panic on a `fail()` or `hang()` turn (4 `should_panic` tests).
- **SSE docs:** they now state that `sse` normalises and `replay` is byte-exact, and which boundaries `replay` splits on.
- **Replay tests:** they now check the per-event chunks and the `text/event-stream` type, and a `.json` replay (`fixtures/http/testkit/replay_probe.json`).
- **Caller verification:** `cargo nextest run -p am-testkit` gives 16 passed. `just check` is green: 98 passed, 6 skipped, check-deps, check-forbid and the denylist clean.

## Plan Change Log

## Review Triage Log

### Pass 1 (quick lens, 2026-11-20): high 0 · medium 1 · low 4 · false 0 · maybe-false 0

The lens found every acceptance criterion and project rule met.

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | `FakeLlm::stream` takes the turn index (`fetch_add`) and pushes the request under separate steps, so with concurrent calls `requests()[i]` may not match the turn played at `i` | medium | patch | Confirmed at lib.rs:426–432. The fan-out tests in epics 3 and 4 call the provider concurrently, and the doc promises "in order". |
| 2 | `fail(..)` and `hang()` accept `.usage()` and `.tool_call()` and then silently drop them | low | patch | `stream` returns early for `Fail` and `Hang` (lib.rs:440–446). A test kit that ignores what a test scripted misleads every later test; the fix is a builder panic. |
| 3 | `FakeResponse::sse` normalises CRLF and drops blank chunks, while `replay` keeps every byte; `replay` splits only on `\n\n` and `\r\n\r\n` | low | patch | The two helpers serve the same file differently. Fix by documenting both behaviours accurately (no behaviour change, which keeps the spike tests stable). |
| 4 | The `replay` test checks only the concatenated body, not one chunk per event or the `.json` content type | low | patch | A regression to a single chunk or a wrong content type would pass. Add assertions. |
| 5 | The `test-util` dev-dependency is unified into every crate's tokio in workspace test runs, so a later crate can use paused time without declaring it and pass `just check` but fail `-p <crate>` | low | defer | Real and caused by cargo's feature unification, not fixable in this diff; it needs a project-context note for later stories. |

## Verification

**Commands:**
- `cargo nextest run -p am-testkit` -- expected: red before implementation (recorded), green after
- `just check` -- expected: green
