# Stack spike report (E1 story 1)

Branch `spike/1-1`, 2026-11-19. The criteria are those of
`_bmad-output/initiative-agentic-manager/stack-spike.md` and architecture AD-1.
Each verdict links its runnable test or recorded demo.

## Verdicts

| # | Criterion | Verdict | Evidence |
|---|---|---|---|
| 1 | Provider port (Anthropic + OpenAI-compatible, streaming, tool calls, usage, cancel within 1 s) | **Pass (live).** Frontier half on the owner's Anthropic key (`claude-opus-5-5`). LM Studio half on `google/gemma-4-e4b`. A second local run on the reasoning model `qwen/qwen3.5-9b` streamed no answer text before the cancel; that is a finding for epic 2 (finding 8), not a failure of the criterion. | [`c1_providers.rs`](../crates/am-e2e/tests/spike/c1_providers.rs): recorded-fixture tests for both adapters, and the live tests `live_anthropic_c1` and `live_lm_studio_c1`. [Live run log below](#c1-live-run-2026-11-20). |
| 2 | `kill -9` mid-run, resume on SQLite WAL | **Pass** | [`c2_crash_resume.rs`](../crates/am-e2e/tests/spike/c2_crash_resume.rs): SIGKILL lands while step 3's call is in flight. On restart the run resumes at step 3. Exactly one call is marked lost, settled at its reservation and re-issued. Every step completes exactly once, the ledger reconciles to the micro-dollar and the audit chain verifies. |
| 3 | Cron + timezone + catch-up after sleep/wake | **Provisional: sleep demo inconclusive (not yet a verdict).** The automated tests pass. Attempt 1 (2026-11-20, forced sleep) crossed the 20:05Z fire time by only about 8 s, inside the 60 s on-time grace, so the missed fire ran once on wake but was classed `on_time` and the catch-up path was not exercised. It needs a sleep that wakes more than 60 s after a fire time. | [`c3_scheduler.rs`](../crates/am-e2e/tests/spike/c3_scheduler.rs): 09:30 Asia/Kolkata fires at 04:00Z. A three-day clock jump gives exactly one catch-up (`once`); the same holds across a restart from persisted `next_fire`. The `skip` and `all` policies are covered. Demo: `agentic spike c3`; [attempt 1 log below](#c3-sleep-demo-attempt-1-2026-11-20). |
| 4 | Files to models | **Pass (live, verified answers; owner decision 2026-11-20).** Two runs on the owner's folder (`AM_SPIKE_DOCS_DIR`, 10 files of 7 types) with `claude-opus-5-5` and `google/gemma-4-e4b`. Every type the model's flags exclude was refused in words before any request (13 of 20 pairs per run, zero bytes sent). Every accepted answer (14 of 14 over both runs) is a word that appears in its file, checked by hand. The automated word match failed 2 rows per run only because the open question lets the model pick any distinctive word (finding 9). Confinement is covered by the offline tests. | [`c4_files_to_models.rs`](../crates/am-e2e/tests/spike/c4_files_to_models.rs) (`live_files_to_models_on_owner_folder`), [owner-folder runs below](#c4-live-runs-on-the-owners-folder-2026-11-20) and [synthetic-fixture table](#c4-on-synthetic-fixtures). The confinement cases pass offline: a symlink file and a symlinked folder leaving the root, `../`, an absolute path, a dangling outward symlink, a folder-link loop and a write on a read-only root are all refused and audited. |
| 5 | MCP (rmcp) over stdio and Streamable HTTP | **Pass (live)** | [`c5_mcp.rs`](../crates/am-e2e/tests/spike/c5_mcp.rs), [run log below](#c5-live-run-2026-11-19). `server-filesystem@2.4.0 --read-only` lists only read tools and `read_text_file` returns the fixture text. `server-everything@2.3.1 streamableHttp` returns `echo`. The HTTP transport runs over `am-net`, and an unlisted port is refused. |
| 6 | Loopback server Host, Origin, token, WebSocket | **Pass** | [`c6_server.rs`](../crates/am-e2e/tests/spike/c6_server.rs). Refused with 403: a bad Host, a foreign Origin on fetch, a foreign Origin on the WebSocket upgrade, a missing CSRF header or subprotocol, and a POST with no Origin. No CORS headers are sent. The launch code is single-use, and a failed exchange invalidates every outstanding code. |
| 7 | macOS Keychain | **Pass** | [`c7_keychain.rs`](../crates/am-e2e/tests/spike/c7_keychain.rs): store, read, overwrite and delete with a canary under service `agentic-manager`, here and on the CI macOS runner. The Linux job builds the store and it answers "the OS keychain is not supported on this platform yet". |
| 8 | Signed + notarised Tauri build on a clean Mac | **Deferred: Apple Developer account.** The gate is epic 11 story 3, before v0.9-rc. Not counted. | Stand-in: [`am-desktop`](../crates/am-desktop/src/main.rs), an ad-hoc-signed `.app` (`Signature=adhoc`, hardened runtime flag). It links the core in-process, launches, serves the UI, and the webview exchanged its launch code ([log](#c8-stand-in-2026-11-19)). |
| 9 | Agent productivity | **Pass** | A fresh Claude Code subagent built the ToolGate `max_uses_per_step` rule and its 7 tests ([`toolgate.rs`](../crates/am-engine/src/toolgate.rs)) with no human code edits. It wrote the tests failing first and passed `just check` on its second run. Summary [below](#c9-subagent-summary). |

**REPLAN count (criteria 1–7 and 9): provisional.** Of the eight counted
criteria, seven have a verdict (1, 2, 4, 5, 6, 7 and 9: all pass, 0 failures)
and one is provisional until the owner's run lands:

- criterion 3: a sleep demo that wakes more than 60 s after a fire time (attempt 1 was inconclusive). The owner closed story 1 on 2026-11-20 with this demo owed on 2026-11-21; its result is appended here.

stack-spike.md allows "pending" only for criterion 5, so the count is not final
and no replan decision can be taken until this run reports. Even a failure of
criterion 3 would leave the count at one, below the threshold of two, so the
stack holds either way; a single failure would get a local workaround here.

CI on `spike/1-1`: run
[44597696551](https://github.com/energyatom/agentic-manager/actions/runs/44597696551) (head `ec1c16f`, after the review fixes).
Both jobs passed: macOS arm64 `just check`, and Linux portability (build and
tests).

## Recorded facts the epic asks for

- **Toolchain:** Rust 1.100.0, edition 2024, pinned in `rust-toolchain.toml`
  (the architecture requires 1.93 or later). cargo-nextest 0.9.152 and just.
  The build output lives outside OneDrive, in
  `~/Library/Caches/agentic-manager/target`; the justfile sets it.
- **OpenAI cloud API choice:** file parts go through the **Responses API**
  (`input_file`), because Chat Completions takes only PDFs as files (AD-31).
  The spike's frontier half ran on Anthropic, so no OpenAI cloud adapter was
  built. The OpenAI-compatible adapter (Chat Completions) serves local servers
  only, which read images and nothing else.
- **Loaded-context field:**
  - **Ollama:** `GET /api/ps` → `models[].context_length` (integer). Source:
    `ProcessModelResponse.ContextLength` in `api/types.go` on main, release
    v0.40.16. The older `docs/api.md` example omits the field. Ollama is not
    installed here, so this comes from the documentation and source, as
    planned.
  - **LM Studio:** `GET /api/v1/models` →
    `models[].loaded_instances[].config.context_length` (integer; the model's
    ceiling is `models[].max_context_length`). Read live on 2026-11-20 with
    `agentic spike local-context`: `qwen/qwen3.5-9b` and `google/gemma-4-e4b`
    both loaded at 32768 (ceiling 262144 for qwen3.5-9b). The same response
    reports `capabilities.vision`, `capabilities.trained_for_tool_use` and
    `capabilities.reasoning` (`allowed_options`, `default`).
- **Unsafe-code exceptions:** none. Every crate has `#![forbid(unsafe_code)]`
  and the workspace lint `unsafe_code = "forbid"`. Unsafe code inside
  dependencies (SQLite, rustls/aws-lc, security-framework, tauri) stays inside
  those crates.

## Findings that change later stories

1. **croner's jiff backend is jiff 0.2, but the Stack pins jiff 1.0.0.**
   croner 4.0.3 implements `CronDateTime` only for jiff 0.2 types. The
   scheduler adds `jiff-croner = { package = "jiff", version = "=0.2.46" }` and
   converts at one function, `scheduler::next_after`: the instant crosses as
   Unix milliseconds and the zone as its IANA name. This is a local workaround,
   not a failure. See `docs/crate-notes/croner.md`.
2. **rmcp without its reqwest client.** The Stack names rmcp's
   `transport-streamable-http-client-reqwest` feature. That feature builds a
   second HTTP client, which AD-12 forbids. `am_mcp::EgressHttpClient`
   implements rmcp's `StreamableHttpClient` over `EgressClient` (about 250
   lines), and MCP-over-HTTP obeys the allow-lists. Story notes should name the
   `transport-streamable-http-client` feature instead.
3. **keyring 4 is used as keyring-core 1.0.1 plus apple-native-keyring-store
   1.0.2.** These are the crates the `keyring` 4 facade wraps, and keyring's own
   documentation recommends linking them directly. Only the Apple store is
   linked (AD-47). The `keyring` crate itself is not a dependency.
4. **Egress pins DNS per request** by building one reqwest client per request
   with `resolve()`. Redirects are not followed in the spike; AD-12's
   re-checked redirects (at most 3) land with the egress story.
5. **Cancellation lives in the stream loop.** Both adapters race the body
   against the cancel signal, and dropping the body ends the connection.
   - Partial usage (Anthropic's `message_start` input tokens) reaches the
     event sink, but a stream cancelled before its final usage reports usage
     as missing, so the call settles at its reservation (AD-9, AD-20; the C1
     cancel test and the `am-engine` journal test). OpenAI-compatible servers
     report usage only at the end, with the same result.
6. **Anthropic API version `2026-10-21`.** It forbids `temperature`, `top_p`
   and `top_k`; the adapter sends none.
   - It ignores `thinking` blocks: `claude-opus-5-5` always thinks.
   - It uses `tool_choice: auto`: forced tool choice returns a 400 on current
     models.
7. **Defence in depth showed up under mutation.**
   - Removing `check_file_parts` alone did not break C4, because the request
     builder refuses the same types.
   - Removing the prefix check in `WorkspacePath::confine` alone did not break
     C4 either, because `strip_prefix` refuses.

   Both C4 tests fail only when both layers are removed (see the mutation table).
8. **Reasoning models stream their thinking in a separate field.** On the
   live C1 run, LM Studio served `qwen/qwen3.5-9b` (reasoning on by default,
   per `capabilities.reasoning`) as `delta.reasoning_content` chunks before any
   `delta.content`. The OpenAI-compatible adapter reads only `content`, so the
   probe saw no answer text within its 20 s wait and cancelled with zero text
   deltas. The tool call, usage and cancel (0.3 ms) all worked. The epic 2
   provider stories should either surface `reasoning_content` as its own stream
   event or switch reasoning off per request where the server allows it, and
   should choose which from the model's `capabilities.reasoning`. No code was
   changed in the spike.

9. **An open question makes the known-word check nondeterministic.** The C4
   question asks the model to "name one distinctive word" in the file, and
   Opus picked a different real word on each run ("Soccer", then "Gearbox";
   "Lithography-free", then "Antibunching"). A fixed `.known-words` list
   therefore misses correct answers at random. Later file-reading tests should
   ask a closed question (which of these words, some of them decoys, appears
   in the file) or check the answer against the file's own text where one
   exists. The spike's verdict rests on the hand check recorded below.

## Test-first evidence (mutation runs)

The tests were written in the same session as the code, not before it. To
show that each criterion's tests fail when its behaviour is removed, the
behaviour was mutated and the tests were re-run, with
`cargo nextest run -p am-e2e <cN_>`:

| Mutation | Result |
|---|---|
| C1: stream loop ignores cancel | 2 of 3 tests fail (10 s deadline) |
| C2: resume skips mark-lost | 1 of 2 tests fails |
| C3: `once` fires every missed time | 2 of 3 tests fail |
| C4: provider file check and request-builder refusal removed | 1 test fails |
| C4: both confinement checks removed | 1 test fails |
| C6: Origin check removed | 1 of 3 tests fails |
| C7: keychain stores an empty value | 1 test fails |

The C9 subagent wrote its tests first against an always-allow stub (7 of 7
failed) and then implemented the rule.

## C4 on synthetic fixtures

The folder is `fixtures/folders/spike/`, with known words in `.known-words`.
The provider responses are recorded fixtures: hand-written in each provider's
published streaming format, served by `FakeEgress`.

| File | Type | Frontier (Anthropic) | Local vision (LM Studio) |
|---|---|---|---|
| notes.txt | text/plain | accepted, "PELICAN" | accepted, "Pelican" |
| table.csv | text/csv | accepted, "MARIGOLD" | accepted, "marigold" |
| report.pdf | application/pdf | accepted natively (`document` block), "ZEPHYR", 1650 in / 7 out tokens, $0.006740 | refused before sending |
| swatch.png | image/png | accepted (`image` block), "Red" | accepted (`image_url` data URL), "red" |
| sheet.xlsx | spreadsheet | refused before sending | refused before sending |
| memo.docx | Word | refused before sending | refused before sending |

The worded refusal reads: "The model claude-opus-5-5 cannot read sheet.xlsx
(application/vnd.openxmlformats-officedocument.spreadsheetml.sheet). Choose a
model that reads this file type, or route the file to another seat. Nothing
was sent."

## C1 live run (2026-11-20)

The owner stored the Anthropic key in the Keychain (service `agentic-manager`,
account `spike-anthropic`) and loaded `qwen/qwen3.5-9b` and
`google/gemma-4-e4b` in LM Studio (32768 context each). The command, run once
per local model:
`AM_LIVE=1 AM_SPIKE_LOCAL_MODEL=<model> cargo nextest run -p am-e2e spike::c1_providers::live --run-ignored only --no-capture`.

| Provider / model | Tool call | Tool-turn usage | Text deltas before cancel | Stop after cancel | Usage after cancel | Result |
|---|---|---|---|---|---|---|
| Anthropic `claude-opus-5-5` (run 1) | `lookup_word {"word": "otter"}` | 412 in / 50 out | 6 | 1.86 ms, `Cancelled` | none, so it settles at the reservation (partial seen: 41 in / 9 out) | PASS |
| Anthropic `claude-opus-5-5` (run 2) | `lookup_word {"word": "otter"}` | 412 in / 50 out | 5 | 0.58 ms, `Cancelled` | none (partial seen: 41 in / 9 out) | PASS |
| LM Studio `google/gemma-4-e4b` | `lookup_word {"word":"otter"}` | 92 in / 77 out | 12 | 0.33 ms, `Cancelled` | none | PASS |
| LM Studio `qwen/qwen3.5-9b` | `lookup_word {"word":"otter"}` | 301 in / 66 out | 0 | 0.28 ms, `Cancelled` | none | FAIL: no `content` deltas within 20 s (finding 8) |

## C4 live runs on the owner's folder (2026-11-20)

`AM_LIVE=1 AM_SPIKE_DOCS_DIR=~/AM-spike-docs AM_SPIKE_LOCAL_MODEL=google/gemma-4-e4b cargo nextest run -p am-e2e spike::c4_files_to_models::live --run-ignored only`.
The folder stays outside the repo. Files are described here by type, not by
name, and personal names are left out, because the repository is to be
published.

**Refused before any request (both runs, identical): 13 of 20 pairs, zero bytes sent.**

| File type | `claude-opus-5-5` | `google/gemma-4-e4b` |
|---|---|---|
| PDF (3 files, all with a text layer) | accepted | refused |
| JPEG (2 scanned card images) | accepted | accepted |
| xlsx (2 files) | refused | refused |
| pptx | refused | refused |
| doc (Word 97) | refused | refused |
| 3mf (3D model) | refused | refused |

Each refusal reads, for example: "The model claude-opus-5-5 cannot read
<file> (application/msword). Choose a model that reads this file type, or
route the file to another seat. Nothing was sent."

**Accepted answers.** "In file" is the hand check: the word appears in the
file's text layer (PDFs) or is printed on the image (JPEGs).

| File | Model | Run 1 answer | Run 2 answer | In file | Tokens in/out (run 1) | Cost USD (run 1) |
|---|---|---|---|---|---|---|
| assembly guide, PDF, 82 pages | Opus | Soccer | Gearbox | yes, both (gearbox ×3) | 130811 / 6 | 0.523364 |
| photon-counter datasheet, PDF, 15 pages | Opus | Excelitas | Excelitas | yes | 34173 / 7 | 0.136832 |
| research paper, PDF, 7 pages | Opus | Lithography-free | Antibunching | yes, both (×4, ×5) | 23253 / 8 | 0.093172 |
| card image, front | Opus | Bombay | Bombay | yes | 2144 / 6 | 0.008696 |
| card image, front | gemma | Bombay | Bombay | yes | 172 / 327 | 0 (local) |
| card image, back | Opus | Physics | Physics | yes | 2143 / 5 | 0.008672 |
| card image, back | gemma | (a surname printed on the card) | Physics | yes, both | 169 / 475 | 0 (local) |

Each run cost about USD 0.77, all on Anthropic. The automated check failed 2
rows per run (run 1: "Lithography-free" and the surname; run 2: "Gearbox" and
"Antibunching") because those words were not on the owner's `.known-words`
list, not because a model failed to read a file (finding 9). gemma's output
token counts are high for a one-word answer because the server counts its
thinking. The owner decided on 2026-11-20 to settle criterion 4 on these
verified answers rather than rewrite the check and run again.

There was no PDF without a text layer in the folder, so that case rests on the
two scanned JPEGs, which both models read.

## C3 sleep demo, attempt 1 (2026-11-20)

The owner's Mac runs in clamshell mode with external monitors, so it was put to
sleep with Apple menu → Sleep instead of the lid; criterion 3 names macOS sleep,
not the lid as such. `agentic spike c3 --db /tmp/c3.db --cron "*/5 * * * *"`
(Asia/Kolkata):

```text
2026-11-19T19:42:02.224418Z scheduler started: cron `*/5 * * * *` in Asia/Kolkata
2026-11-19T19:45:00.00618Z  FIRE scheduled_for=19:45:00Z kind=on_time
2026-11-19T19:50:00.005436Z FIRE scheduled_for=19:50:00Z kind=on_time
2026-11-19T19:55:00.008254Z FIRE scheduled_for=19:55:00Z kind=on_time
2026-11-19T20:00:00.007557Z FIRE scheduled_for=20:00:00Z kind=on_time
2026-11-19T20:05:08.755451Z FIRE scheduled_for=20:05:00Z kind=on_time (wall clock jumped 248s: sleep/wake)
```

`pmset -g log` for the same window (local time, +05:30):

```text
01:27:42 Sleep     Entering Sleep state due to 'Software Sleep'
01:27:55 DarkWake  DarkWake from Deep Idle … 180 secs
01:30:55 Sleep     Entering Sleep state due to 'Maintenance Sleep' … 261 secs
01:35:16 Wake      Wake from Deep Idle
```

Reading: the Mac slept at 19:57:42Z, but macOS woke it 13 s later into a
DarkWake (a maintenance wake with the screen off) until 20:00:55Z. The process
ran during the DarkWake and fired 20:00 on time. The real sleep, 20:00:55Z to
about 20:05:08Z, crossed one fire time (20:05:00Z) by about 8 s. The scheduler
detected the 248 s jump and fired that slot exactly once on wake: no miss and
no duplicate. Because the delay was inside `ON_TIME_GRACE` (60 s,
`crates/am-engine/src/scheduler.rs`), the fire was classed `on_time`, so the
`catch_up` path that criterion 3 names was not exercised. Verdict:
inconclusive; repeat with a sleep that wakes more than 60 s after a fire time.

## C5 live run (2026-11-19)

Both servers were started with `npx`. The run used Node from `/usr/local/bin`,
`AM_LIVE=1 cargo nextest run -p am-e2e spike --run-ignored only --no-capture`:

```text
stdio tools: ["read_file", "read_text_file", "read_media_file", "read_multiple_files", "list_directory", "list_directory_with_sizes", "directory_tree", "search_files", "get_file_info", "list_allowed_directories"]
PASS [1.924s] spike::c5_mcp::live_stdio_filesystem_server
http tools: ["echo", "get-annotated-message", "get-env", "get-header-params", "get-resource-links", "get-resource-reference", "get-structured-content", "get-sum", "get-tiny-image", "gzip-file-as-resource", "trigger-list-changed", "trigger-log-messages", "trigger-long-running-operation", "trigger-resource-update", "toggle-simulated-logging", "toggle-subscriber-updates"]
PASS [3.849s] spike::c5_mcp::live_streamable_http_everything_server
```

The stdio child runs with a scrubbed environment (only `PATH` and `HOME`), its
own cwd and its own process group. No server process was left after the run.

## C8 stand-in (2026-11-19)

```text
$ just desktop   # tauri build --debug --bundles app, signingIdentity "-"
Bundling Agentic Manager.app … Signing with identity "-"
Warn skipping app notarization, no APPLE_ID … found
$ codesign -dv "Agentic Manager.app"
Identifier=app.agentic-manager.desktop
CodeDirectory … flags=0x10002(adhoc,runtime)
Signature=adhoc
$ "Agentic Manager.app/Contents/MacOS/agentic-desktop"
agentic-desktop: serving on http://127.0.0.1:55207
am-server: session opened on port 55207      # the webview exchanged its launch code
GET / with Host 127.0.0.1:<port> -> 200; with Host evil.example:<port> -> 403
```

## C9 subagent summary

The subagent got a brief with the story and six acceptance criteria. It
created `crates/am-engine/src/toolgate.rs`:

- an `ActionClass` enum, `ToolManifest`, and `GateDecision::{Allow, Refuse}`;
- `ToolGate::check(&Actor, &ToolCall)`, with counts per run, step and tool;
- seat overrides;
- refusals audited as `toolgate.refused`.

It added 7 tests and `uuid` as a dev-dependency (to build `Actor::Seat`).

- **Test-first:** all 7 failed against an always-allow stub.
- **`just check`:** the first run failed only on rustfmt. After
  `cargo fmt --all` the second run passed (39 tests).
- **Scaffold friction it reported:**
  - `am-domain` does not re-export `Uuid`, so tests need the `uuid` crate. A
    testkit helper that builds a seat actor would remove that.
  - The ticket did not say how to treat a caller that is not a seat. It chose
    to refuse and audit such a call.

The code was committed unchanged (commit `7ead7ff`).

## Owner runs still to do

1. **Anthropic key: done 2026-11-20** (account `spike-anthropic`; criterion 1
   passed live). Criterion 4's frontier half uses the same key. The model
   defaults to `claude-opus-5-5`; `AM_SPIKE_ANTHROPIC_MODEL` changes it.
2. **LM Studio: done 2026-11-20** for criterion 1. For criterion 4 set
   `AM_SPIKE_LOCAL_MODEL=google/gemma-4-e4b`, the vision-capable model that
   passed criterion 1. Do not use a reasoning model such as `qwen/qwen3.5-9b`
   until finding 8 is handled.
3. **Live tests (criterion 4: done 2026-11-20, two runs):**
   `AM_SPIKE_DOCS_DIR=<folder> AM_SPIKE_C4_OUT=<file.md> just spike-live`.
   Each live test prints `skipped: <reason>` when its input is absent. Or use
   the commands:
   - `agentic spike c1 --provider anthropic|lmstudio [--model <id>]`
   - `agentic spike c4 --local-model <id> --out c4.md`
   - `agentic spike local-context`

   Criterion 4 needs known words: put a `.known-words` file in the folder with
   one line per file, `relative/path: word, other word`, naming words that
   appear in that file. Without it the run fails and says so. The run also
   fails when only one of the frontier and local models ran, when a file
   cannot be read, or when a type the model's flags allow is refused.
4. **Sleep demo (criterion 3; attempt 1 inconclusive).** Run `agentic spike c3 --db /tmp/c3.db --cron "*/5 * * * *"`,
   close the lid across a fire time, then reopen. The log shows one
   `kind=catch_up` fire, with "wall clock jumped …s: sleep/wake".
