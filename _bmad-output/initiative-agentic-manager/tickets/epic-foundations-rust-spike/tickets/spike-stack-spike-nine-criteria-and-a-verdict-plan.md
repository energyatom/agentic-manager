---
title: 'Stack spike: nine criteria and a verdict'
type: feature
ticket: '1'
created: '2026-11-19'
status: built
baseline_revision: '61e4f78d156e326e338eea562e1b0ffad245a5e2'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/initiative-agentic-manager/stack-spike.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/epic-foundations-rust-spike.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Nothing can be built until we know whether the Rust-first stack holds. The nine stack-spike criteria (arch AD-1) are unproven, and every later story waits on their verdict.

**Approach:** Scaffold the real workspace (every Structural Seed crate, the ports in `am-domain`, a minimal `just check`, minimal CI). Prove each criterion behind the real ports as a failing-first runnable test or a recorded demo. Write `docs/spike-report.md` with a verdict per criterion. Decisions (owner, 2026-11-19): the Anthropic key is the frontier half; work happens on branch `spike/1-1` with checkpoint commits and is squash-merged to `main` as the one story commit; `AM_SPIKE_DOCS_DIR` comes later, so criterion 4 is built on synthetic fixtures first and your recorded run lands before the verdict.

## Boundaries & Constraints

**Always:**
- The architecture's Stack versions, pinned exactly.
- `#![forbid(unsafe_code)]` in every crate.
- No foreign types in `am-domain` or in port signatures; `Arc<dyn Port>`; no lifetimes in public APIs.
- HTTP only through `am-net::EgressClient`.
- Secrets only through `SecretStore`, under service `agentic-manager`. Tests use canary values.
- Files read and MCP results are `UntrustedData`.
- macOS-only code sits behind `#[cfg(target_os = "macos")]`, with a non-macOS branch returning "not supported on this platform yet".
- Tests use no network or real model unless marked live and skipped when their input is absent.
- `CARGO_TARGET_DIR` outside OneDrive.

**Never:**
- genai, rig-core or async-openai.
- A per-format file reader, OCR, or any file conversion.
- Any of your documents in the repo.
- Department vocabulary in `crates/`.
- `Browser::launch` or `Browser::connect`.
- Story 1.2's hardening: xtask check-deps logic, cargo-deny policy, the denylist scan, the build hook.
- Epic 2's Accounts.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| C1 cancel | Cancel token fired mid-stream | Stream ends within 1 s; usage captured up to that point | Settles at the reservation if usage is missing |
| C2 crash | `kill -9` during a step | On restart the run resumes at its last committed transition; only the in-flight call is lost; ledger reconciles | The unsettled call is marked lost and re-issued |
| C3 jump | Clock jumps past ≥1 fire time | Exactly one catch-up fire (`once` policy) | — |
| C4 excluded type | xlsx to Anthropic; pdf to a local model | Worded `Unsupported` refusal | Zero bytes sent |
| C4 confinement | Symlink out of the folder, `../` path, write on a read root | Refused | Refusal audited |
| C4 no folder | `AM_SPIKE_DOCS_DIR` unset | Skipped: "AM_SPIKE_DOCS_DIR not set" | — |
| C6 rebinding / CORS / WS | Bad Host; foreign Origin on fetch; foreign Origin on WS upgrade | Rejected (403) | No CORS headers sent |
| C7 keychain off-mac | Linux build | Compiles; "not supported on this platform yet" | — |

</frozen-after-approval>

## Code Map

Greenfield: there is no code at `61e4f78`. The rules come from the architecture document: AD-1 (criteria), AD-5 (ports, `FilePart { name, media_type, bytes }`, `Unsupported { media_type, model }`), AD-6/7 (writer, WAL, `synchronous=FULL`), AD-8/9 (journal, `settled`, `call_seq`, reserve and settle), AD-11 (ToolGate checks, `max_uses_per_step`), AD-12 (egress; only loopback 1234/11434 allowed), AD-13 (launch code, `am_s_<port>` cookie, `X-AM-CSRF`, Host/Origin), AD-17 (keychain), AD-18 (scheduler, 30 s tick, catch-up `once`), AD-20 (`CancellationToken` tree), AD-24 (Actor), AD-30/31 (folder access, file-input flags), AD-47 (platform rules).

## Tasks & Acceptance

**Execution:**
- [ ] `Cargo.toml`, `rust-toolchain.toml`, `justfile`, `LICENSE`, `THIRD_PARTY.md`, `SECURITY.md`, `clippy.toml`, `.gitignore` -- workspace with `am-desktop` outside the default members; minimal `just check` -- the foundation
- [ ] `crates/am-*/` (all 19) and `xtask/` -- empty crates where unused -- the Structural Seed
- [ ] `crates/am-domain/src/` -- the ports `LlmProvider`, `SecretStore`, `Clock`, `McpClient`, `EgressClient`; `Actor`; content parts; `UntrustedData`; `WorkspacePath` -- the contracts
- [ ] `crates/am-net/` -- `EgressClient` on reqwest with allow-list and loopback exceptions -- the only HTTP client
- [ ] `crates/am-providers/` -- Anthropic Messages and OpenAI-compatible SSE adapters, file parts, capability flags, cancellation -- C1, C4
- [ ] `crates/am-store/` -- writer task, WAL, minimal runs/steps/model_calls/ledger tables, resume -- C2
- [ ] `crates/am-engine/src/scheduler.rs` -- croner + jiff loop over `Clock`, catch-up `once` -- C3
- [ ] `crates/am-tools/` -- `list_files` and `read_file` with confinement -- C4
- [ ] `crates/am-mcp/` -- rmcp client over stdio and Streamable HTTP -- C5
- [ ] `crates/am-server/` -- axum loopback auth and WebSocket -- C6
- [ ] `crates/am-secrets/` -- keyring macOS store, `SecretString` -- C7
- [ ] `crates/am-app/` -- the `agentic` binary (`serve`, the spike run used by C2, a `spike` subcommand for live runs) -- C2, C4
- [ ] `crates/am-desktop/` and `web/` (one minimal page) -- ad-hoc-signed Tauri build -- C8 stand-in
- [ ] `crates/am-engine/src/toolgate.rs` -- built by a subagent: the `max_uses_per_step` rule and its tests -- C9
- [ ] `crates/am-e2e/tests/spike/c{1..7}_*.rs`, `fixtures/folders/spike/`, `fixtures/http/` -- failing-first tests, synthetic files, recorded responses
- [ ] `docs/crate-notes/{rmcp,croner,reqwest,rusqlite,axum}.md` -- one page per pre-1.0 crate used
- [ ] `.github/workflows/ci.yml` -- macOS arm64 `just check` plus the Linux portability build, SHA-pinned
- [ ] `docs/spike-report.md` -- verdict table, REPLAN count, toolchain, OpenAI API choice, loaded-context field, unsafe exceptions

**Acceptance Criteria:**
- Given the workspace, when `just check` runs, then fmt, clippy `-D warnings` and nextest pass, and CI on `spike/1-1` is green on both jobs.
- Given the Anthropic key in the Keychain and LM Studio running, when the live C1 test runs, then both adapters stream text, return a tool call with usage, and stop within 1 s of cancel.
- Given the two npx servers, when the C5 test runs, then the tools list on both, `read_text_file` returns the fixture text and `echo` returns its message.
- Given `AM_SPIKE_DOCS_DIR` (set later by you), when the C4 run runs, then a table per file and model shows accepted or refused, tokens and cost, and every accepted answer names a known word from its file.
- Given all criteria are run, when the report is written, then all nine have a verdict with a link. C8 reads "deferred: Apple Developer account" with epic 11 story 3 as its gate, and the REPLAN count covers 1–7 and 9 only.

## Implementation Notes

- 2026-11-20. Built by one implementation subagent in 6 checkpoint commits (`9b430c7` to `1f0017a`), pushed to `spike/1-1`. CI passed on macOS arm64 and the Linux portability job (run 44588243540). `just check` passed locally: 39 tests passed, 5 live tests skipped.
- Re-run by the caller: `just check` green. `just spike-live` passed C5 live against both npx servers. C1 live (no key, LM Studio off) and C4 live (`AM_SPIKE_DOCS_DIR` unset) skip in words.
- Deviations recorded in `docs/spike-report.md`:
  - croner 4.0.3 needs jiff 0.2, so `jiff-croner` (jiff `=0.2.46`) is used at one conversion point.
  - rmcp uses `transport-streamable-http-client` over our own `EgressClient`, not the `-reqwest` feature, because AD-12 allows only one HTTP client.
  - The Keychain goes through keyring-core 1.0.1 plus apple-native-keyring-store 1.0.2.
  - The "recorded" provider fixtures were written by hand from the published SSE formats, not captured from live calls.
  - The tests were written in the same session as the code, not first. As substitute evidence, the report has a mutation check per criterion.
- Live tests show as PASS in nextest and print `skipped: <reason>` when their input is absent.
- The Keychain account for the live run is `spike-anthropic` (service `agentic-manager`).

## Plan Change Log

## Review Triage Log

### Pass 1 (quick lens, 2026-11-20): high 4 · medium 9 · low 3 · false 1 · maybe-false 0

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | `docs/spike-report.md`: C1, C3 and C4 shown as Pass, and "0 failures" counted, while their live and owner runs are still pending | high | patch | Report lines 11, 13 and 14 say "Pass…pending". AD-1 C1 needs the frontier half live, and stack-spike allows "pending" only for C5. The fix is to mark them provisional until the owner runs. |
| 2 | Tests not written failing-first (AGENTS.md policy; plan Approach) | medium | bad_plan → owner decision | Admitted in Implementation Notes and the report, with mutation checks offered as substitute evidence. The loopback would revert about 20k lines. |
| 3 | Stack pins changed in code without REPLAN: keyring → keyring-core plus apple-native-keyring-store; second jiff 0.2.46; rmcp feature | medium | intent_gap → owner decision | Workspace `Cargo.toml` lines 90 and 108–110. croner 4.0.3 needs jiff 0.2, and rmcp's reqwest feature would add a second HTTP client against AD-12. No code change can hold the frozen "Always: pinned exactly", so the Stack table needs a REPLAN through Sofia. |
| 4 | `am-engine/src/journal.rs:64` `create_run` hard-codes `Actor::Human { owner }` | medium | patch | Confirmed. Scheduled runs would be audited as the owner's act, against AD-24's "no default". |
| 5 | Report links CI run 44587037865, not the final head's run | low | patch | Report line 32. The head `1f0017a` ran as 44588243540, which is green. |
| 6 | Out-of-scope `__pycache__` change | false | reject | The owner approved it in the session plan of 2026-11-19 ("Commit the stray deletion there"). |
| 7 | `CARGO_TARGET_DIR` is set only through `just`, so plain `cargo` builds land in OneDrive | low | defer | Real. The fix is per-machine (a shell profile or a README note); AGENTS.md wording belongs to the project-context refresh. |
| 8 | `am-tools` `write_file`: a dangling symlink escapes the root | high | patch | Confirmed at lib.rs:351–359. `canonicalize` fails on the missing target, so the in-root candidate passes `confine`, and `fs::write` follows the link outside. |
| 9 | `am-tools` `list_files`: symlinked folder cycle | medium | patch | Confirmed at lib.rs:233–244. There is no visited set, so a link inside the root recurses and duplicates entries until the 50-file cap. |
| 10 | `am-server` `guard`: an `Upgrade` header skips session and CSRF checks on every `/api/v1/*` route | high | patch | Confirmed at lib.rs:149. Only `events` re-checks the session, so `GET /api/v1/health` with `Upgrade` and an allowed Origin is served unauthenticated. |
| 11 | `render_untrusted_text`: `</untrusted-data>` inside the data closes the fence | medium | patch | Confirmed at common.rs:8–13. The text is inserted raw. |
| 12 | `am-net` body pump: dropping `BodyChunks` does not abort while blocked in `chunk()` | medium | patch | Confirmed at lib.rs:300–318. The drop is noticed only at the next `tx.send`; idle streams hold their connection. |
| 13 | `spike.rs` `c4_passed` passes on refusals of allowed types, read errors, unchecked answers, empty rows, or a single model | high | patch | Confirmed at spike.rs:468–476. `Refused` is always true, `None` is accepted, and an empty slice gives true. |
| 14 | Live harness picks the first `"id"` from LM Studio's `/v1/models` (it could be an embedding model) and forces vision on | medium | patch | Confirmed at main.rs:121–133 and live.rs:28–38. |
| 15 | `drive_run`: a crash between `SettleCall` and `FinishStep` discards a settled call and pays for it twice | medium | patch | Confirmed at journal.rs:167–201. These are two writer transactions, and the output is stored only in `FinishStep`. |
| 16 | `MarkLost` ledger update has no settled guard | low | patch | Confirmed at store lib.rs:286–293. No current caller reaches it on a settled call; the fix is a one-clause guard. |
| 17 | Anthropic cancel settles from `message_start` usage (`output_tokens` 1), under-charging | medium | patch | Confirmed at anthropic.rs:184–251 and 378. `cancelled_outcome` keeps the partial usage, and journal.rs:164 settles from it instead of the reservation (AD-9, AD-20). |
| 18 | Scheduler keeps the old `next_fire` when only the time zone changes | low | patch | Confirmed at scheduler.rs:162–163. Only `cron` is compared. |
| 19 | `web/app.js` tells the user to run `agentic open`, which doesn't exist | low | patch | Confirmed at app.js:9 and 15. The binary has `serve`. |

Owner decision, 2026-11-20 ("Keep code, fix and record"): there is no loopback for findings 2 and 3. Finding 2 stays recorded in the spike report as a deviation, with its mutation evidence. Finding 3 goes to Sofia as a REPLAN of the Stack table when the story closes. The patch findings go back to the implementer.

## Design Notes

C3's sleep/wake and C4's recorded run on your folder are the two owner demos; everything else is automated. C9 is run by a fresh subagent on the finished scaffold, without code edits from me; its transcript summary is the evidence. Ollama isn't installed, so its loaded-context field is recorded from its `/api/ps` documentation, and LM Studio's equivalent is recorded live.

## Verification

**Commands:**
- `just check` -- expected: green
- `cargo nextest run -p am-e2e spike` -- expected: C1–C7 offline tests pass; live tests report skipped when their input is absent
- `AM_LIVE=1 cargo nextest run -p am-e2e spike --run-ignored only` -- expected: live C1, C4 and C5 pass on your machine

**Manual checks:**
- The C3 lid demo log shows one catch-up fire.
- The ad-hoc Tauri app launches and serves the page.
- `docs/spike-report.md` covers all nine criteria.
