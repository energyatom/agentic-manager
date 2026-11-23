---
title: 'Loopback server with launch-code sign-in'
type: feature
ticket: 2
created: '2026-11-23'
status: built
baseline_revision: 'a7edbd672e216480e3581e6b14f94ab0c0439b7d'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick, adversarial-security]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** `am-server` is the spike's server (criterion 6). It already binds loopback and checks Host and Origin. It also handles a single-use launch code, an `am_s_<port>` cookie and a CSRF header or subprotocol. Five things are still missing:
- the pairing-code fallback;
- the per-launch CLI token and AD-15's CLI and UI route classes;
- the owner-only permission refusal at start;
- one error envelope;
- run status over `/api/v1`, and a WebSocket event stream with `seq` and `since=`.

RULE-15 needs all of this proven by trust tests (arch AD-13, AD-14, AD-15).

**Approach:** Harden the spike server in place:
- **One guard** decides every request: Host, Origin, then the route's class.
  - **UI class:** needs the session cookie and the CSRF value.
  - **CLI class:** accepts either the session cookie with the CSRF value, or the per-launch CLI token on a request that carries no `Origin`.
  - **Every route** is declared once with its class.
- **Pairing:** an 8-character base32 code, single use and valid 60 s, is printed at start and again on demand when the owner presses Enter in the `agentic serve` terminal. It is exchanged for the same session as the launch code, rate-limited and locked after 10 failures; refusals for Host or Origin never count.
- **State changes:** POST, PUT or DELETE only, never GET; with the browser session they need a matching Origin plus the CSRF value.
- **Errors:** they all use `am_api::ApiError`.
- **Run status:** `/api/v1/runs` and `/api/v1/runs/{id}` serve it from the open project's store, read-only.
- **Events:** an in-memory log numbers each event (`seq`). The stream replays from `since=`, and a run-status watcher publishes changes.

## Boundaries & Constraints

**Always:**
- **Binding and secrets (AD-13):**
  - **Loopback only:** `127.0.0.1` only, with no option to change it.
  - **Generated per launch, in memory only:** the session table, the launch codes, the pairing code and the CLI token value are generated per launch from the OS RNG (256-bit for the session and the token). They are never written to disk, a URL or a log, except two cases:
    - the launch code, in the opened URL's fragment;
    - the CLI token, in its own 0600 file.
  - **Pairing code only on the serve terminal:** each code goes only to the terminal that ran `agentic serve`, never over the API, to a CLI caller or to a log.
- **Launch code (as built in the spike):** single-use and valid 60 s. A failed exchange invalidates every outstanding code.
  - **Exchange:** `POST /api/v1/session {code}` sets an `HttpOnly; SameSite=Strict; Path=/` cookie named `am_s_<port>` and returns `{csrf}` in the body.
  - **No other cookie** is ever set.
- **Pairing code (owner Decision 2026-11-23):** 8 characters of RFC 4648 base32 (40 bits, as AD-13 fixes). `POST /api/v1/pair {code}` exchanges it for the same session as the launch code.
  - **Single use, short-lived:** a code is valid for 60 s and is consumed by its successful exchange. At most one code is outstanding.
  - **On demand:** one is printed at start; pressing Enter in the `agentic serve` terminal prints a fresh one and invalidates any unused one. There is no API, CLI command or file that issues or returns a pairing code, so the CLI token can never become a browser session. While pairing is locked, Enter prints that it is locked until restart and issues nothing.
  - **Rate limit:** 5 attempts in any 60 s window; the 6th is `429` with the envelope.
  - **Lock:** after 10 failures pairing is locked until restart, and even the right code is refused. A wrong code does not invalidate the outstanding one (the lock bounds guessing).
  - **What counts:** only an attempt that passed the Host and Origin checks counts towards the rate limit and the lock. A request refused for its Host, a missing or foreign Origin, or a malformed body never counts, so a web page cannot lock the owner out.
  - **No effect on launch codes:** a pairing failure does not invalidate them.
- **The guard, on every request including the WebSocket upgrade:**
  - **Host (owner Decision 2026-11-23, DNS-rebinding defence):** the `Host` header must be exactly `127.0.0.1:<port>` or `localhost:<port>` for this server's port; anything else (a foreign name, another port, no port, a missing Host, a trailing dot, different case of a non-name) is refused `host-not-allowed` before any other check.
  - **Origin:** must be the served origin (or an added Tauri origin), when present; a foreign Origin is always refused. Origin is required on every non-GET request and every upgrade, except a CLI-token request. A GET with no Origin is accepted only on a read route (browsers omit Origin on same-origin GETs) and still needs the CSRF value.
  - **State changes (owner Decision 2026-11-23):** a route that changes state is POST, PUT or DELETE, never GET or HEAD; a GET or HEAD to it is refused `405 method-not-allowed` with the envelope and changes nothing. Reached with the browser session, it needs a present, matching Origin plus the CSRF value (the session and pair exchanges, which come before any session, need the matching Origin). CLI-class state changes (AD-15: stop, manual run start and the rest, none in this story) authenticate with the CLI token and must carry no Origin.
  - **No CORS headers** ever.
  - **Under `/api/v1/`**, except the session and pair exchanges, the route's class decides:
    - **UI class:** a session cookie and the matching `X-AM-CSRF` (the WebSocket upgrade carries the CSRF value as a subprotocol instead).
    - **CLI class:** that, or a valid `X-AM-CLI-Token` on a request with no `Origin` header. A request carrying `Origin` is never authenticated by the token.
  - **What never authenticates:** a cookie alone, a token on a UI-class route, a token with `Origin`, or the wrong subprotocol.
- **Route classes (AD-15):** every route is declared in one table with its class and whether it changes state:
  - `UiClass`;
  - `CliClass`;
  - `Exchange`, for session and pair, which are pre-auth.

  A unit test fails if a route is served that the table does not hold, or if a state-changing route accepts GET or HEAD. Today's `/api/v1` routes are `health`, `runs`, `runs/{id}` and `events`, all CLI class (reads). Test builds add a UI-class fixture route through a router-builder parameter: it is a constructor parameter, never a feature, env var or flag, and the seam guard covers it.
- **CLI token:**
  - **The file:** at start the server writes 256 random bits, hex-encoded, to `AppDataLayout::cli_token(pid)` = `<app data dir>/serving/<pid>.token` (0600, in the 0700 `serving` folder), beside 2.2's port record. It is removed at a clean exit.
  - **Stale files:** they are ignored, because the server holds its own value in memory and compares in constant time.
  - **What it grants:** only CLI-class routes, and never with an `Origin` header.
- **Owner-only start:**
  - **Refused when loose:** the server refuses to start, in words naming the path and the `chmod`, when the app data dir or the open project's runtime folder is not 0700, or a file in it is not 0600. It reuses 2.12's owner-only check, extended to the runtime folder.
  - **Created owner-only:** the folders and files the server creates are owner-only from the start.
- **Error envelope (AD-14):** every error response from the server, guard refusals included, is JSON `am_api::ApiError { code, message, fix?, details? }` with a fitting status. `code` is a stable kebab-case string, `message` is plain words, and no secret ever appears in any of them.
- **Run status:** `GET /api/v1/runs` lists the open project's runs. `GET /api/v1/runs/{id}` returns one run with its steps. Both are read through the project store's read-only `Reader` (story 3.1's `runs`, `run` and `steps`). With no project open, the response is an envelope with `code: no-project`; an unknown id gives `code: not-found`. The DTOs live in `am_api`.
- **Events (AD-14):**
  - **The log:** `am_server::EventLog` holds the last 10 000 events of this launch, each with a monotonic `seq` from 1. Each event is `{seq, kind, data}`, an `am_api` type.
  - **The stream:** `GET /api/v1/events?since=<seq>` upgrades to a WebSocket. It first replays every held event after `since`, then streams new ones.
  - **Too far back:** a `since` older than the oldest held event first gets one `{kind: "resync"}` event, telling the client to re-read run status.
  - **Run-status watcher:** while a project is open, a watcher reads its runs and steps through a read-only connection once a second. It publishes `run.updated` and `step.updated` when a row's status or `updated_at` changes. This covers runs started by `agentic run` in another process.
- **`agentic serve` wiring (am-app):**
  - **At start:** it writes the CLI token, checks permissions, and prints the URL with no code (the browser opens with the fragment), the pairing code and where the CLI token lives.
  - **Flags:** `--no-browser` still prints the pairing code.
  - **At exit:** the token file and the port record are removed.
- **Tests first:** RULE-15 trust tests at `crates/am-e2e/tests/trust/rule_15_*.rs` (each a `[[test]]` in am-e2e's Cargo.toml, as `trust_targets.rs` requires), written to fail first. The spike's `c6_server.rs` cases seed them. No network beyond loopback.

**Never:**
- **No other bindings or headers:** no LAN bind and no CORS headers.
- **No secrets in storage:** no session secret on disk, and no CSRF value in a cookie or in browser storage.
- **No token authentication for UI routes or for requests carrying `Origin`.**
- **No test switch in the release binary:** no feature, env var or flag that adds a test route.
- **Outside this story:**
  - `agentic open` (a later story);
  - Tauri bootstrap IPC (epic 11);
  - ts-rs generation (the UI epics);
  - any mutation route.
- **No new dependency** beyond the workspace's pinned ones (axum, tokio, getrandom, hex, serde).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Signed in once | `serve` opens the launch URL; the page exchanges the code | Cookie `am_s_<port>` (HttpOnly, SameSite=Strict); `{csrf}` in the body; `GET /api/v1/runs` with `X-AM-CSRF` → 200 | — |
| Rebinding Host | `Host: evil.example:<port>` with a valid cookie and CSRF, on a read route, the fixture mutation, the exchanges and the upgrade | 403 envelope `host-not-allowed` | — |
| Near-miss Host | `127.0.0.1:<other port>`, `127.0.0.1` with no port, `localhost.:<port>`, `[::1]:<port>`, no Host | 403 `host-not-allowed` | — |
| Cross-origin fetch | `Origin: http://evil.example` on a GET with a valid cookie and CSRF | 403 `origin-not-allowed`; no CORS headers | — |
| Cross-origin upgrade | WebSocket upgrade with a foreign Origin | 403 | — |
| Missing CSRF | A valid cookie, no `X-AM-CSRF`, on a GET and on the fixture mutation (with a matching Origin) | 403 `csrf-required` | — |
| Cookie-only GET | Cookie and no CSRF, as another localhost service's page would send | 403 | — |
| Cookie-only upgrade | Upgrade with the cookie and no subprotocol; with a wrong subprotocol | 403 | — |
| Replayed launch code | The same code exchanged twice | Second refused, `launch-code-invalid` | — |
| Old launch code | Exchanged after 61 s (FakeClock or an injected clock) | Refused | — |
| Failed exchange | A wrong code, then the right one | Both refused (all outstanding codes invalidated) | — |
| Pairing | The printed code at `/api/v1/pair` | Same session as the launch code | — |
| Pairing replay | The same pairing code used twice | Second refused | — |
| Old pairing code | Used after 61 s | Refused | — |
| New pairing code | A new code issued (Enter) while one is unused | Old one refused, new one works | — |
| Pairing rate | 6 attempts within 60 s | 6th is 429 `pairing-rate-limited` | — |
| Pairing lock | 10 wrong, then the right one | Refused `pairing-locked` until restart; a new code is not issued while locked | — |
| Foreign requests don't count | 20 pair attempts with a foreign Host, a foreign Origin or no Origin, then the right code | All 20 refused for Host or Origin; the right code still pairs (no rate limit, no lock) | — |
| GET to a state change | `GET` (and `HEAD`) to the fixture mutation with a valid cookie and CSRF | 405 `method-not-allowed`; nothing changed | — |
| State change without Origin | The fixture mutation with cookie and CSRF but no Origin | 403 `origin-required` | — |
| Exchange without Origin | `POST /session` or `/pair` with no Origin | 403; for pair, not counted | — |
| Cookie name | Any session | Name is `am_s_<port>` | — |
| Loose app data | App data dir 0755, or the token file 0644 at start | Server does not start; worded with the `chmod` | Worded |
| Loose runtime folder | Project runtime folder 0755 | Server does not start | Worded |
| Secret nowhere | After a sign-in, a pairing and a few requests | The session ids, CSRF value, pairing code and token are in no log line, no URL except the launch fragment, and no file under the app data dir or runtime folder except the token file | — |
| CLI token, CLI route | `X-AM-CLI-Token` with no Origin on `/api/v1/runs` | 200 | — |
| CLI token, UI route | The token on the UI-class fixture route | 403 `ui-class-only` | — |
| CLI token with Origin | The token plus `Origin: http://127.0.0.1:<port>` | 403 | — |
| Stale token | A token file from a dead process | Refused | — |
| Route table | A route served but missing from the class table | Unit test fails | — |
| Envelope | Every error above | JSON `{code, message, fix?, details?}`; no secret in any field | — |
| Run status | A project with a 3.1 run | `/runs` lists it; `/runs/{id}` holds its steps; no project → `no-project`; unknown id → `not-found` | — |
| Events | A run changes status while a client is connected | `run.updated` with increasing `seq` | — |
| Reconnect | A client disconnects after seq 3; events 4 to 6 happen; it reconnects with `since=3` | 4, 5 and 6 replayed in order, then live | — |
| Too far back | `since=` older than the held window | One `resync` event first | — |
| Loopback only | Connect via a non-loopback address of the Mac | Refused (not listening) | — |

</intent-contract>

## Code Map

- **`crates/am-server/src/lib.rs`** (315 lines, the spike):
  - `AppState` (L52-120): add the pairing state, the CLI token, the route table and the event log;
  - `guard` (L131-172): the class decision and envelope refusals;
  - `session` (L213): the launch exchange;
  - `events` (L258-286): subprotocol, `since`, replay;
  - `router` (L288): the declared table;
  - `bind` and `serve` (L300-315).
  - It may split into modules (`guard.rs`, `pairing.rs`, `events.rs`, `routes.rs`) inside the crate.
- **`crates/am-api/src/lib.rs`** (empty): `ApiError`, `RunDto`, `StepDto`, `EventDto`, `SessionBody`, `PairBody`.
- **`crates/am-app/src/main.rs`** `serve` (L209-260): permissions, the token file, the printouts, the watcher, the removal at exit.
- **`crates/am-app/src/runtime.rs`** `record_serving` and `clear_serving` (L232-250): the token file beside them.
- **`crates/am-app/src/user.rs`**: 2.12's owner-only check, reused for the runtime folder.
- **`crates/am-domain/src/runtime.rs`**: `AppDataLayout::cli_token(pid)`.
- **`crates/am-store`**: `Reader::runs`, `run` and `steps` (L1033-1070), read-only.
- **`crates/am-app/src/main.rs`** also: the stdin reader that turns Enter into a new pairing code.
- **Tests:**
  - new `crates/am-e2e/tests/trust/rule_15_{signin,csrf,pairing,cli_token,events,permissions}.rs`, seeded from `crates/am-e2e/tests/spike/c6_server.rs`;
  - `[[test]]` entries in `crates/am-e2e/Cargo.toml`;
  - an am-server unit test for the route table;
  - `xtask` seam guard for the fixture-route parameter.

## Tasks & Acceptance

**Execution:**
- [ ] **Tests first (no `src/` change):** the rule_15 files and the route-table test, one test per matrix row. Log the red run.
- [ ] **am-api:** the error and DTO types.
- [ ] **am-server:** guard classes, pairing, CLI token, envelope, run routes, event log and stream.
- [ ] **am-domain and am-app:** `cli_token` path, `serve` wiring, permissions, watcher, cleanup.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the response, headers, files and events match the row.
- Given the workspace, when `just check` runs, then it is green.

## Plan Change Log

- 2026-11-23, owner: approved with changes, recorded as owner Decisions (epic 3 Notes). (1) Pairing code single use, valid 60 s, a new one on demand from the `agentic serve` terminal only (Enter), never over the API or to a CLI caller; Host or Origin refusals never count towards the rate limit or lock; kept at 8 base32 characters as AD-13 fixes (the owner chose this over amending AD-13). (2) Host must be exactly `127.0.0.1:<port>` or `localhost:<port>`, with tests including a foreign Host. (3) State changes are POST, PUT or DELETE only, never GET; with the browser session they need a matching Origin plus the CSRF value, tested on the fixture route, including a GET to it being refused; CLI-class state changes use the CLI token with no Origin (AD-15). Decisions 1, 3, 4 and 5 below stand as written; decision 2 is replaced by (1).

## Design Notes

**Decisions for the owner's approval:**
1. **The CLI token header and file:** `X-AM-CLI-Token`, held in `<app data dir>/serving/<pid>.token` (0600) beside the port record and removed at exit. It never authenticates a request that carries `Origin`, so a web page that somehow learned it still could not use it from a browser.
2. ~~The pairing code is reusable for the whole launch.~~ Replaced by the owner (2026-11-23): single use, 60 s, a new one on demand from the serve terminal; see the Plan Change Log.
3. **Events come from an in-memory log** of the last 10 000 events of this launch, with a `resync` event when `since` is older than that. The first real producer is a once-a-second watcher of the project's run and step rows, so runs started by `agentic run` in another process show up. The engine publishing directly comes when runs start from the server (3.3 onward).
4. **Every existing `/api/v1` route is CLI class** (reads only). The UI-class rule is proven on a test-only fixture route, added through a router-builder parameter that the seam guard keeps out of shipped code.
5. **The permission refusal covers the open project's runtime folder too**, as AD-13 says, reusing 2.12's owner-only check.

**Size:** medium. One crate hardened, one small API crate filled, the `serve` wiring, six trust-test files.

## Verification

- `cargo nextest run -p am-e2e -E 'binary(/rule_15/)'` and the am-server unit tests, then `just check`.

## Implementation Notes

### Phase 1 test API (tester, 2026-11-23)

The tests are written against this API; the implementer matches it (names, signatures, codes, statuses). Nothing under any `src/` was changed by the tester. The tests were type-checked and passed `clippy -D warnings` against a stub of exactly this API in a scratch copy of the repo (not in this tree), and the raw-HTTP and WebSocket test clients were probed against today's spike server there.

**am-api** (adds the workspace's `serde` and `serde_json`; all derive `Debug, Clone, PartialEq, Serialize, Deserialize`):
- `ApiError { code: String, message: String, fix: Option<String>, details: Option<serde_json::Value> }`: `fix` and `details` are left out when `None`; the wire object has no other field.
- `RunDto { id, workflow, status, created_at, finished_at: Option<String> }`, camelCase on the wire.
- `StepDto { id, run_id, step_key, seq: i64, status, started_at: Option<String>, finished_at: Option<String> }`, camelCase.
- `RunDetailDto { run: RunDto, steps: Vec<StepDto> }` (added: the body of `GET /api/v1/runs/{id}`); `GET /api/v1/runs` answers a JSON array of `RunDto`, newest first.
- `EventDto { seq: u64, kind: String, data: serde_json::Value }`.
- `SessionBody { code: String }` and `PairBody { code: String }`: the exchange request bodies. Both exchanges answer a JSON object with exactly one key, `csrf`.

**am-server** (adds `am-domain`, `am-api`, `am-store`, all allowed by `check-deps`):
- `AppState::new(port)` keeps its meaning (system clock, window 10 000, no project), so the spike test still compiles; `AppState::builder(port) -> AppStateBuilder` with `.clock(Arc<dyn am_domain::Clock>)`, `.event_window(usize)`, `.project_db(PathBuf)` (the open project's `state.db`, read only through `am_store::Reader`), `.allow_origin(&str)` and `.build() -> AppState`. `allow_origin`, `port`, `cookie_name`, `issue_launch_code() -> String` and `launch_url() -> String` (`http://127.0.0.1:<port>/#code=<code>`) stay. The launch code, the pairing code and the pairing rate window all read the injected clock.
- `AppState::issue_pairing_code(&self) -> Result<PairingCode, PairingLocked>`: a new code invalidates the unused one; `Err` while locked. `PairingCode::expose(&self) -> &str`; its `Debug` is redacted. `PairingLocked` is a `Debug` error. No HTTP route reaches it.
- `AppState::cli_token(&self) -> CliToken`: one value per `AppState` (256 OS-random bits, 64 hex digits). `CliToken` is `Clone`, `expose(&self) -> &str`, `Debug` redacted.
- `AppState::events(&self) -> EventLog` (a `Clone` handle); `EventLog::publish(&self, kind: &str, data: serde_json::Value) -> u64` returns the `seq`, from 1. The spike's `server.hello` is no longer sent: the stream sends only log events and `resync`.
- `watch_runs(state: &AppState, every: Duration) -> tokio::task::JoinHandle<()>`: polls the project's runs and steps through a read-only `Reader` and publishes `run.updated` (data: the `RunDto` JSON) and `step.updated` (data: the `StepDto` JSON, `runId` included) when a status or time column changes. With no project it publishes nothing.
- `print_pairing_code(state: &AppState, out: &mut dyn std::io::Write) -> std::io::Result<()>`: issues a code and prints one line holding it as an 8-character `[A-Z2-7]` word; while locked, it prints a line with the words "locked" and "restart" and no code. `pairing_terminal(state: &AppState, input: &mut dyn std::io::BufRead, out: &mut dyn std::io::Write) -> std::io::Result<()>` calls it once per line read (Enter) and returns at end of input; `agentic serve` runs it on stdin. Neither prints any other 8-character `[A-Z2-7]` word.
- `RouteClass { Page, Exchange, UiClass, CliClass }` and `RouteSpec { method: String, path: String, class: RouteClass, changes_state: bool }` (both `Debug, Clone, PartialEq, Eq`; `path` in axum syntax). `Page` is added for `/` and `/app.js` (Host and Origin checks only). `route_table() -> Vec<RouteSpec>` is exactly: `GET /` Page, `GET /app.js` Page, `POST /api/v1/session` Exchange (state), `POST /api/v1/pair` Exchange (state), `GET /api/v1/health`, `GET /api/v1/runs`, `GET /api/v1/runs/{id}`, `GET /api/v1/events` (all CliClass, reads). Build the router from this table so the two cannot diverge.
- `FixtureRoute::new()` (and `Default`), `Clone`, `count(&self) -> u64`; `FixtureRoute::spec()` = `POST /api/v1/fixture`, UiClass, changes state; its handler adds one to the counter and answers 2xx. `router_with_fixture(state: AppState, fixture: FixtureRoute) -> Router` is the test-only router builder (the constructor parameter): the xtask seam guard should keep `router_with_fixture` and `FixtureRoute` out of every shipped `src/` except their definitions. `router(state)` is the shipped one (no fixture route: `POST /api/v1/fixture` is 404 there). `serve_router(listener: TcpListener, router: Router, shutdown) -> std::io::Result<()>` is added; `serve` and `bind` are unchanged.

**am-domain:** `AppDataLayout::cli_token(pid: u32) -> PathBuf` = `<app data dir>/serving/<pid>.token`.

**am-app:** a new `pub mod serving`:
- `owner_only_at_start(places: &Places, runtime: Option<&RuntimeLayout>) -> Result<(), Refusal>`: the app data dir and its `serving` folder 0700 and every file in them 0600 (token files included), and the runtime folder 0700 and its files 0600. The words name the path and `chmod 700 <path>` or `chmod 600 <path>`.
- `write_cli_token(places: &Places, pid: u32, token: &CliToken) -> Result<PathBuf, Refusal>`: creates the app data dir and `serving` 0700 if missing, and writes the hex value to a 0600 file (a trailing newline is allowed).
- `clear_cli_token(places: &Places, pid: u32)`.

**Guard order** (the tests depend on it):
1. Host is not exactly `127.0.0.1:<port>` or `localhost:<port>`: 403 `host-not-allowed`, before anything else, 404 included.
2. An Origin is present and foreign: 403 `origin-not-allowed`.
3. Route lookup. A path not in the table: 404 `not-found`. A method not declared for it: 405 `method-not-allowed`. HEAD is accepted where GET is declared on a read route, and never on a state-changing route.
4. The class:
   - **Page:** nothing more.
   - **Exchange:** no Origin gives 403 `origin-required`; a malformed body gives 400 `bad-request`, which is not counted.
   - **`X-AM-CLI-Token` present:** with an Origin, 403 `cli-token-with-origin`; on a UiClass route, 403 `ui-class-only`; a value that is not this launch's, 401 `cli-token-invalid`; otherwise accepted, the upgrade included, with no subprotocol.
   - **Session:**
     - a non-GET/HEAD request, or an upgrade, with no Origin gives 403 `origin-required` (HEAD counts as a GET here);
     - no cookie, or an unknown session, gives 401 `session-required`;
     - a missing or wrong `X-AM-CSRF`, or a missing or wrong `am.csrf.<value>` subprotocol on the upgrade, gives 403 `csrf-required`.
5. Handlers: 409 `no-project`, 404 `not-found` for an unknown run id, 401 `launch-code-invalid`, 401 `pairing-code-invalid`, 429 `pairing-rate-limited`, 403 `pairing-locked`.

**Error codes and statuses.** Codes added by the tester, where the plan left the case unnamed:
- `session-required` 401: no session (the spike's 401 "no session");
- `pairing-code-invalid` 401: a wrong, expired or used pairing code;
- `bad-request` 400: a malformed exchange body;
- `cli-token-with-origin` 403: the token on a request that carries Origin;
- `cli-token-invalid` 401: a stale or made-up token.

Statuses chosen by the tester for the plan's codes:
- `pairing-locked` 403;
- `no-project` 409;
- `not-found` 404;
- `launch-code-invalid` 401 (as in the spike);
- `ui-class-only` 403.

**Pairing counting:**
- An attempt counts only after it passed Host, Origin and the body check.
- The lock is checked before the rate limit. After the 10th failure, every attempt answers `pairing-locked`.
- The rate limit allows 5 counted attempts with times later than now minus 60 s. The 6th answers 429 and its code is not checked.
- A wrong code leaves the outstanding one valid.

**Events:**
- `resync`'s `seq` is not pinned. After it come the held events, oldest first.
- `since` equal to the oldest held seq minus 1 gets no `resync`.
- A connection without `since` is not pinned by the tests.

**Known limits of the tests:**
- The server's `eprintln!` output cannot be captured in-process. The secret check covers `tracing` lines, responses, events, the launch URL and every file under the app data dir and the runtime folder.
- The route-table test cannot list axum's routes, so it pins the table and probes every entry, every method and a list of likely unlisted paths. Building the router from `route_table()` closes the gap.
- The stdin wiring is tested through `pairing_terminal`, not through the binary.

### Red run (tester, 2026-11-23)

Commands:
- `cargo nextest run -p am-e2e -E 'binary(/rule_15/)' --no-fail-fast`: the build failed, as expected. Cargo stops after the first failing binaries, so all ten were then built with `cargo build --keep-going -p am-e2e --test rule_15_<each>`. All 10 binaries fail to compile, with 78 errors in total:
  - signin 5, host 5, csrf 4, state_changes 4, pairing 24, cli_token 10, events 7, runs 5, permissions 3, secrets 11;
  - the errors are `unresolved imports am_api::ApiError, am_api::EventDto, am_api::RunDto, am_api::RunDetailDto`, `am_server::FixtureRoute, router_with_fixture, serve_router, pairing_terminal, print_pairing_code`, `unresolved import am_app::serving`, `no method issue_pairing_code / cli_token / events / builder on AppState`, `no method cli_token on AppDataLayout` and `cannot find function watch_runs`.
- `cargo nextest run -p am-server`: `route_table` fails to compile, with `unresolved imports am_server::FixtureRoute, RouteClass, RouteSpec, route_table, router_with_fixture, serve_router`.
- `cargo test -p am-e2e --test trust_targets` passes: every new trust file has its `[[test]]` target. `cargo xtask denylist`, `runtime-paths` and `check-forbid` are clean.

Tests, by matrix row (61 in all: 57 trust tests and 4 route-table tests):

**`rule_15_signin.rs` (7):**
- `signed_in_once`: Signed in once.
- `a_replayed_launch_code_is_refused`: Replayed launch code.
- `an_old_launch_code_is_refused`: Old launch code.
- `a_failed_exchange_invalidates_every_outstanding_code`: Failed exchange.
- `the_cookie_is_named_for_the_port_and_no_other_is_set`: Cookie name.
- `a_session_exchange_without_a_matching_origin_is_refused`: Exchange without Origin (session).
- `an_api_read_with_no_session_is_refused`: Envelope (`session-required`).

**`rule_15_host.rs` (3):**
- `a_foreign_host_is_refused_on_every_surface`: Rebinding Host.
- `near_miss_hosts_are_refused`: Near-miss Host.
- `the_server_listens_on_loopback_only`: Loopback only.

**`rule_15_csrf.rs` (5):**
- `a_cross_origin_fetch_is_refused_without_cors`: Cross-origin fetch.
- `a_cross_origin_upgrade_is_refused`: Cross-origin upgrade.
- `a_missing_or_wrong_csrf_value_is_refused`: Missing CSRF.
- `a_cookie_alone_authenticates_nothing`: Cookie-only GET.
- `a_cookie_only_upgrade_is_refused`: Cookie-only upgrade.

**`rule_15_state_changes.rs` (3):**
- `get_and_head_to_a_state_change_are_refused_and_change_nothing`: GET to a state change.
- `get_to_an_exchange_is_refused`: GET to a state change (the exchanges).
- `a_state_change_needs_a_matching_origin`: State change without Origin.

**`rule_15_pairing.rs` (14):**
- `the_pairing_code_is_eight_base32_characters` and `pairing_gives_the_same_session_as_the_launch_code`: Pairing.
- `a_wrong_code_leaves_the_outstanding_one_valid`: Pairing; a wrong code does not invalidate the outstanding one.
- `a_replayed_pairing_code_is_refused`: Pairing replay.
- `an_old_pairing_code_is_refused`: Old pairing code.
- `a_new_pairing_code_invalidates_the_unused_one` and `enter_in_the_serve_terminal_prints_a_new_code`: New pairing code.
- `the_sixth_attempt_in_a_minute_is_rate_limited`: Pairing rate.
- `pairing_locks_after_ten_failures`: Pairing lock.
- `attempts_refused_for_host_or_origin_do_not_count`: Foreign requests don't count.
- `malformed_bodies_do_not_count`: Foreign requests don't count (malformed body).
- `a_pair_exchange_without_origin_is_refused_and_not_counted`: Exchange without Origin (pair).
- `pairing_failures_leave_launch_codes_valid`: No effect on launch codes.
- `no_api_route_issues_a_pairing_code`: never over the API.

**`rule_15_cli_token.rs` (7):**
- `the_token_reads_a_cli_route_without_origin`: CLI token, CLI route.
- `the_token_is_refused_on_a_ui_class_route`: CLI token, UI route.
- `the_token_with_any_origin_is_refused`: CLI token with Origin.
- `a_stale_token_from_a_dead_process_is_refused`: Stale token.
- `the_token_file_is_owner_only_beside_the_port_record_and_removed_at_exit`: the token file.
- `the_token_never_opens_a_browser_session`: the token never becomes a session.
- `the_token_opens_the_event_stream_without_origin_only`: CLI class on the upgrade.

**`rule_15_events.rs` (5):**
- `seq_starts_at_one_and_increases`: Events.
- `a_reconnect_with_since_replays_the_missed_events_then_streams`: Reconnect.
- `a_since_older_than_the_window_gets_one_resync_first`: Too far back.
- `a_run_changing_status_publishes_run_updated`: Events.
- `a_step_changing_status_publishes_step_updated`: the watcher.

**`rule_15_runs.rs` (4):**
- `runs_lists_the_run_and_run_by_id_holds_its_steps`, `with_no_project_open_runs_answer_no_project` and `an_unknown_run_id_is_not_found`: Run status.
- `the_run_routes_only_read`: read-only.

**`rule_15_permissions.rs` (6):**
- `owner_only_folders_and_files_start`: the control case.
- `a_loose_app_data_dir_refuses_the_start`, `a_loose_token_file_refuses_the_start` and `a_loose_serving_folder_refuses_the_start`: Loose app data.
- `a_loose_runtime_folder_refuses_the_start` and `a_loose_file_in_the_runtime_folder_refuses_the_start`: Loose runtime folder.

**`rule_15_secrets.rs` (3):**
- `secrets_are_in_no_log_url_or_file`: Secret nowhere.
- `every_error_is_one_envelope_with_no_secret`: Envelope.
- `debug_output_shows_no_secret`: secrets redacted in `Debug`.

**`crates/am-server/tests/route_table.rs` (4):**
- `the_table_holds_exactly_the_shipped_routes`, `no_state_changing_route_accepts_get_or_head`, `every_served_route_is_in_the_table` and `the_shipped_router_has_no_fixture_route`: Route table.

The shared rig is `crates/am-e2e/tests/trust/support/server.rs`. It provides a real server on 127.0.0.1 with a `FakeClock`, the fixture route and an optional project store, raw HTTP/1.1 requests, a minimal WebSocket client and the envelope assertion. Every refusal in every file goes through the envelope assertion, which also checks that there is no CORS header.

### Implementation (implementer, 2026-11-23)

No test file was edited (the rule_15 files, `support/server.rs`, `crates/am-server/tests/route_table.rs`); no test contradicted the plan or an AD.

**What was built, per crate:**
- **am-api:** `ApiError` (`fix`, `details` left out when `None`; `new`, `with_fix`), `RunDto`, `StepDto`, `RunDetailDto`, `EventDto`, `SessionBody`, `PairBody` (camelCase DTOs), and the event kinds `EVENT_RESYNC`, `EVENT_RUN_UPDATED`, `EVENT_STEP_UPDATED`. Deps: the workspace's `serde`, `serde_json`.
- **am-server**, split into modules: `state.rs` (`AppState`, `AppStateBuilder`, the in-memory sign-in table behind one lock, the injected clock), `guard.rs` (the one guard in the plan's order: Host, foreign Origin, route lookup 404/405, class; security headers on every response, never CORS), `routes.rs` (`RouteClass`, `RouteSpec`, `route_table()`; the router is built from the same `(spec, handler)` list, so the two cannot diverge; the session and pair exchanges; the event upgrade), `pairing.rs` (`PairingCode`, `PairingLocked`, the single-use 60 s code, the 5-in-60 s rate limit and the 10-failure lock, `print_pairing_code`, `pairing_terminal`), `events.rs` (`EventLog` with `seq`, `since=` replay and `resync`, the stream, `watch_runs`), `runs.rs` (the run routes over a fresh read-only `am_store::Reader` in `spawn_blocking`), `envelope.rs` (every refusal as `ApiError`, fixed words), `secret.rs` (OS randomness, constant-time comparison, `CliToken` with redacted `Debug`), `fixture.rs` (`FixtureRoute`, `router_with_fixture`). `serve_router` added; `bind`, `serve`, `router` unchanged in shape. Session ids, CSRF values, launch and pairing codes and the token are compared in constant time; the spike's `eprintln!` on session open became a `tracing::info!` with the port only. Deps added: `am-domain`, `am-api`, `am-store` (all allowed by `check-deps`) and the workspace-pinned `jiff` (already in the graph through am-domain; the `Clock` port returns `jiff::Timestamp`).
- **am-domain:** `AppDataLayout::cli_token(pid)` = `serving/<pid>.token`.
- **am-app:** `serving` module (`owner_only_at_start`, `write_cli_token`, `clear_cli_token`); `runtime::replace_file` became `pub(crate)` to write the token by an owner-only temp file and rename. `agentic serve` now: checks owner-only before it binds (app data dir, `serving`, the open project's runtime folder); builds the state with the project's `state.db`; writes the token file after the port records; prints the URL without a code, the token file's place (never its value) and the pairing code; opens the browser unless `--no-browser`; runs `pairing_terminal` on stdin in a thread (Enter prints a new code); starts `watch_runs` every second; at exit aborts the watcher and removes the token file, the port record and the serving record.
- **xtask:** `seam-guard` now holds a list of seams: am-net's `with_seams` as before, plus `router_with_fixture` (allowed only at its definition in `crates/am-server/src/fixture.rs`) and `FixtureRoute` (allowed only in that file). `lib.rs` re-exports the file with `pub use fixture::*;`, so no other shipped source names either. Two new tests in `xtask/tests/seam_guard.rs` (seen failing against the old guard first).
- **web/app.js** (not in the Code Map; small): the stream's events now carry `kind` (was `type`), and with no launch code the page asks for the pairing code and posts it to `/api/v1/pair`, so pairing has a client until the UI epics.

**Deviations from the Phase 1 test API, and choices the tests leave open:**
- Additions only: `AppStateBuilder` is exported; constants `CLI_TOKEN_HEADER`, `PAIRING_CODE_TTL`, `PAIRING_RATE_LIMIT`, `PAIRING_RATE_WINDOW`, `PAIRING_LOCK_AFTER`, `EVENT_WINDOW`; `EventLog::last_seq`; `PairingLocked` also implements `Display` and `Error`.
- Codes added beyond the plan's: `upgrade-required` 426 (a GET to `/api/v1/events` that is not an upgrade, after the guard passed it), `store-unreadable` 500 (the store could not be read; the error goes to a `tracing` warning, not the response), `internal` 500, and `bad-request` 400 also for a `since` that is not a whole number.
- A malformed `POST /api/v1/session` body is now 400 `bad-request` and does not invalidate the outstanding launch codes (the spike treated it as a wrong code). A well-formed wrong code still invalidates them all.
- More than one `Host` header is refused `host-not-allowed`.
- `resync` carries `seq` = the oldest held seq minus 1 and `data: {}`; it is not stored in the log. It is also sent when `since` is past the last event (a cursor from an earlier launch). A connection without `since` streams live events only. A client that falls behind the window while connected also gets a `resync`.
- Watcher: the plan names "status or `updated_at`", but 3.1's rows have no `updated_at`; it compares the whole DTO (status, `createdAt`, `finishedAt`; for steps status, `startedAt`, `finishedAt`). The first look records what is there and publishes nothing; a row that appears later publishes. Steps are re-read only for runs that are unfinished, new or changed.
- `owner_only_at_start` checks each folder and its top-level regular files (symlinks and subfolders are not walked); a folder that does not exist yet is not refused (it is created 0700).
- Known limits, left for later stories: sessions do not expire and the session and launch-code tables are not bounded (codes are issued only by `serve` itself).

**Spike tests:** `crates/am-e2e/tests/spike/c6_server.rs` passes unchanged (5 of 5); no spike edit was needed.

**`just check` (2026-11-23): green.** fmt-check, clippy `-D warnings`, nextest 1033 passed and 8 skipped, am-store's doctests (3 passed), `check-deps` (21 crates follow the AD-4 matrix), `check-forbid`, `denylist`, `runtime-paths`, `fixed-hosts`, `seam-guard` (95 files clean) and `agentic validate` over the fixtures and templates. `cargo nextest run -p am-e2e -E 'binary(/rule_15/)'`: 57 passed; `cargo nextest run -p am-server`: 9 passed (the 4 route-table tests and 5 unit tests).

### Review patch round 1: red run (tester, 2026-11-23)

These are tests only. The tester changed no file under any `src/`; the `src/` changes in the working tree are the implementation's. The new tests are added at the end of the existing files under a "review patch round 1" heading, and of `crates/am-app/tests/serve_project.rs` under a "3.2" heading.

Commands:
- `cargo nextest run -p am-e2e -E 'binary(/rule_15/)' --no-fail-fast`: 73 run, 60 passed, 13 failed. The 57 earlier tests all still pass, and every failure is one of the new tests.
- `cargo nextest run -p am-app -E 'binary(serve_project)' --no-fail-fast`: 17 run, 16 passed, 1 failed (P3).
- Also clean: `cargo clippy -p am-e2e -p am-app --tests -- -D warnings`, `cargo xtask denylist` and `cargo xtask runtime-paths`.

Each finding's tests, and why each red test fails:

**P1, the recursive owner-only walk** (`rule_15_permissions.rs`):
- `a_loose_file_two_levels_deep_in_app_data_refuses_the_start`: red, `owner_only_at_start` returned `Ok`.
- `a_loose_file_two_levels_deep_in_the_runtime_folder_refuses_the_start`: red, `Ok`.
- `a_loose_subfolder_of_app_data_refuses_the_start`: red, `Ok`.
- `a_loose_subfolder_of_the_runtime_folder_refuses_the_start`: red, `Ok`.
- `a_symlink_inside_refuses_the_start`: red, `Ok`. It covers both the app data dir and the runtime folder, and the refusal must name the link.
- `an_unreadable_app_data_dir_refuses_the_start`: red, `Ok`. The parent folder is 0000, so the metadata error is EACCES and is taken for "missing".
- `a_clean_nested_tree_and_missing_folders_start`: green. It is the control case and must stay green.

**P2, the real `agentic serve` wiring** (`serve_project.rs`):
- `serve_writes_the_cli_token_owner_only_and_prints_a_pairing_code_and_its_path` (a): green.
- `sigint_stops_serve_and_removes_the_token_and_the_port_records` (b): green.
- `a_loose_app_data_dir_stops_serve_before_anything_is_written` (c): green.
- These are coverage of wiring that already works, not red tests. All three use bounded waits (60 s), so a hang fails rather than blocks.

**P3, stale token files:**
- `rule_15_cli_token.rs::a_dead_process_token_file_is_pruned_with_its_record`: red. `serving_ports` removes the dead pid's record but not `serving/<dead pid>.token`.
- `serve_project.rs::a_serve_start_prunes_a_dead_process_token_file`: red. `serving/99999999.token` is still there after a serve start; the live pid's token is kept.

**P4, `since` overflow** (`rule_15_events.rs`):
- `a_since_of_u64_max_gets_one_resync_and_the_stream_stays_usable`: red. It panics with an add overflow at `cursor + 1` in `EventLog::after` (events.rs:89) and the client gets no events.
- `a_since_of_u64_max_on_an_empty_log_gets_resync_then_live`: red, the same panic.
- `a_since_past_the_last_seq_gets_one_resync`: green, already handled.

**P5, absolute-form URI** (`rule_15_host.rs`):
- `an_absolute_form_target_with_a_foreign_authority_is_refused`: red. `GET http://evil.example/api/v1/health` with the right Host and the token answered 200, not 403 `host-not-allowed`.

**P6, the clock stepping back.** This is expressible: `FakeClock::advance` takes a negative duration.
- `rule_15_signin.rs::a_launch_code_issued_in_the_future_is_refused`: red. A code issued at T0 is exchanged after the clock goes back 120 s and answers 200, because `within()` accepts a negative age.
- `rule_15_pairing.rs::a_pairing_code_issued_in_the_future_is_refused`: red, the same for the pairing code.
- The second half of each test (back 30 s, then on to T0+61 s, refused) already holds.

**P7, Debug redaction** (`rule_15_secrets.rs`):
- `exchange_bodies_show_no_code_in_debug`: red. The derived `Debug` of `SessionBody` and `PairBody` prints the code.

**P8, a watcher read failure** (`rule_15_events.rs`):
- `a_store_unreadable_for_a_while_does_not_republish_unchanged_rows` is green. The test renames `state.db` away for about 6 watcher ticks, then back, and sees no `run.updated` or `step.updated` for the unchanged rows.
- Through the public API, a failed read makes `look` return `None` and the watcher puts `seen` back. The only path that resets `seen` is the `spawn_blocking` task panicking: `.unwrap_or_default()` on the `JoinError` gives an empty `Seen`, and the next tick republishes every row. No public input makes `look` panic, so that path cannot be made red from outside. The test stays as a regression guard; a unit test inside am-server would be needed to cover the `JoinError` branch.

**P9:** no test, as asked (wiring only).

### Review triage, round 1 (coordinator, 2026-11-23)

Lenses: quick (correctness and spec) and adversarial (security, with live probes of the debug binary). No high finding. Routes: **patch** (tests first), **defer** (named owner), **accept** (residual, told to the owner), **dismiss**.

| Id | Finding | Sev | Verdict | Route |
|---|---|---|---|---|
| P1 | Owner-only check reads top-level files only; AD-13 says 0700 folders and 0600 files; non-NotFound metadata errors pass | medium | real, narrows a binding AD | patch: recursive walk, symlinks refused, only NotFound passes |
| P2 | `agentic serve` wiring untested; "removed at exit" test calls `clear_cli_token` itself | medium | real (test gap) | patch: three real-binary cases in `serve_project.rs` (already green: wiring was right) |
| P3 | `serving/<pid>.token` of a crashed process never pruned (both lenses) | low | real | patch |
| P4 | `since=u64::MAX` overflows `cursor + 1` (debug panic) | low | real, probed | patch: saturating arithmetic |
| P5 | Absolute-form request target with a foreign authority passes the Host check | low | real, probed | patch: refuse when the URI authority differs from the allowed Host |
| P6 | A code whose issue time is in the future (clock stepped back) stays valid | low | real | patch: `within` requires `now >= issued` |
| P7 | `SessionBody` / `PairBody` derived `Debug` prints the code | low | real | patch: redacted `Debug` |
| P8 | Watcher `JoinError` (panicked read) empties `seen`, next tick republishes every row | low | real by reading | patch: keep the previous `seen`; covered by an am-server unit test |
| P9 | stdin Enter thread: a backgrounded `agentic serve` (`&`, Ctrl-Z `bg`) is stopped by SIGTTIN | medium | real (job control) | patch: read stdin only when it is a terminal; document that the Enter key works with serve in the foreground |
| P10 | `web/app.js` posts any fragment to `/session`, so a page that navigates to `/#code=x` burns the outstanding launch codes (DoS only, first seconds) | low | real | patch: app.js posts only a well-formed code (64 hex); the server rule (a failed exchange invalidates every code, AD-13) is unchanged |
| D1 | `seq` restarts at 1 each launch with no launch id | low | real, low impact (a restart also ends the session) | defer: to the UI epic's event client (story 6.1, first consumer of the stream) |
| D2 | Watcher and `GET /runs` read every run with no paging | low | performance | defer: story 3.3 onward (runs started from the server) |
| D3 | Sessions never expire; session table unbounded (grows only through valid codes) | low | by design for v1 | defer: noted for the UI epics |
| A1 | The launch code is briefly in the `open` process's argv and possibly browser history; a same-user process could race the exchange | low | inherent to AD-13's fragment design; same-user rule | accept: residual, told to the owner |
| A2 | A local non-browser process can forge Origin and lock pairing | low | by design (the lock bounds guessing; web pages cannot) | accept |
| A3 | macOS ACLs and folder owner are not checked by the owner-only check | low | beyond AD-13's mode rule | accept: residual, told to the owner |
| — | `web/app.js` edited outside the Code Map (`kind` field; pairing prompt) | — | needed by the new event shape; the pairing prompt is pairing's only browser client until the UI epics | accept (coordinator), told to the owner |

### Review patch round 1 (implementer, 2026-11-23)

No test written by the tester was edited. P2 needed no code (its three cases were already green).

- **P1** (`crates/am-app/src/serving.rs`): `owner_only_at_start` walks the app data dir and the runtime folder recursively, after 2.12's check.
  - Every folder must be 0700 and every other entry 0600, at any depth.
  - A symbolic link anywhere, the top folder included, refuses the start, in words naming it.
  - A missing top folder passes. Any other metadata or read error on the top folder, a folder inside or an entry refuses, worded with the path and the `chmod`.
  - One choice: an entry listed by `read_dir` that is gone by the time it is looked at (NotFound inside the tree, e.g. a temporary file renamed meanwhile) is skipped, not refused. A missing entry is not a permission error, and refusing it would make the start race any writer.
  - The `serving` folder is now covered as part of the app data tree.
- **P3** (`crates/am-app/src/runtime.rs`, `main.rs`): `serving_ports` also removes `serving/<pid>.token` when that pid is not running. `agentic serve` calls it at start, after the owner-only check and before it binds and records its own port.
- **P4** (`crates/am-server/src/events.rs`): `EventLog::after` uses saturating arithmetic for `cursor + 1`, `last + 1` and `oldest - 1`, and so does `publish`'s `last + 1`. A unit case covers `after(u64::MAX)` on a full log and on an empty one.
- **P5** (`state.rs`, `guard.rs`): `host_ok` also takes the request URI. An absolute-form target must be `http` with an authority of exactly `127.0.0.1:<port>` or `localhost:<port>`. A target with a scheme and no authority is refused too.
- **P6** (`state.rs`): `within` requires `now >= issued`. The pairing rate window's prune now relies on it, so the `t > now` clause it had is gone.
- **P7** (`crates/am-api/src/lib.rs`): `SessionBody` and `PairBody` have a hand-written `Debug` that prints `code: <redacted>`.
- **P8** (`events.rs`): the blocking read is factored into `read_off_thread(seen, look)`.
  - The unit test `events::tests::a_failed_look_keeps_what_was_seen` was written first against a refactor that kept the old behaviour, and failed: the look panics, and the `seen` handed back was empty.
  - The fix shares `seen` with the blocking task through an `Arc` and takes it back with `Arc::unwrap_or_clone`, so a `JoinError` leaves it as it was. `Seen` derives `Clone` for the fallback. The test now passes.
- **P9** (`main.rs`, `serving.rs`): the Enter thread starts only when stdin is a terminal (`am_app::serving::stdin_reads_the_owner`, `std::io::IsTerminal`). Only then does serve print "press Enter here for a new pairing code (with agentic serve in the foreground)". The `agentic serve` doc line in `main.rs` says the same.
  - The check lives in `serving.rs` because xtask's terminal guard (`release_deps::the_only_terminal_outside_the_test_kit_is_the_owners_real_one`) refuses any `*Terminal` name in `main.rs`.
  - Residual: `agentic serve &` started from an interactive shell still has a terminal on stdin, so its reader thread is still stopped by SIGTTIN when it reads (job control stops the whole process). Telling foreground from background needs `tcgetpgrp`/`getpgrp`, which are behind nix's `process` feature, not enabled in the workspace. That is an owner decision (a feature change); until then the help words say "in the foreground", and `agentic serve < /dev/null &` is the way to run it in the background.
- **P10** (`web/app.js`): the fragment is exchanged at `/api/v1/session` only when it is exactly 64 lowercase hex digits. Otherwise the fragment is still stripped and the page asks for the pairing code. The server rule is unchanged.

Runs:
- `cargo nextest run -p am-e2e -E 'binary(/rule_15/)'`: 73 passed.
- `cargo nextest run -p am-app -E 'binary(serve_project)'`: 17 passed.
- `cargo nextest run -p am-server`: 10 passed.
- **`just check`: green.** fmt, clippy `-D warnings`, nextest 1054 passed and 8 skipped, am-store doctests (3), `check-deps` (21 crates), `check-forbid`, `denylist`, `runtime-paths`, `fixed-hosts`, `seam-guard` (95 files clean), and `agentic validate` on the 13 fixture and template folders. The first run failed one xtask test, the terminal guard on `IsTerminal` in `main.rs`; the check was moved into `serving.rs`, as above.

## Auto Run Result

**Status:** built (2026-11-23).

**Summary:** `agentic serve` runs the hardened loopback server (AD-13, AD-14, AD-15).
- **One guard** on every request, the upgrade included: an exact Host (`127.0.0.1:<port>` or `localhost:<port>`, absolute-form targets too), then a foreign Origin, then the route table (404, 405), then the route's class. No CORS headers; every refusal is one `ApiError` envelope with no secret in it.
- **Route table:** the router is built from it; state changes are POST/PUT/DELETE only (GET or HEAD gets 405); a state change with the session needs a matching Origin plus the CSRF value; a GET with no Origin is accepted only on read routes and still needs CSRF.
- **Sign-in:** the single-use 60 s launch code (as the spike); an 8-character base32 pairing code, single use and valid 60 s, printed at start and again when Enter is pressed in the serve terminal, never over the API; 5 attempts a minute, locked after 10 failures; Host and Origin refusals never count.
- **CLI token:** 256 bits in `<app data dir>/serving/<pid>.token` (0600), CLI class only, never with an Origin; removed at exit; tokens of dead processes are pruned.
- **Owner-only start:** the app data dir and the open project's runtime folder are walked; any folder not 0700, file not 0600 or symlink refuses the start in words naming the `chmod`.
- **Runs and events:** `GET /api/v1/runs` and `/runs/{id}` read-only; `/api/v1/events` keeps the last 10 000 events with `seq`, replays from `since=`, sends `resync` when too far back; a once-a-second watcher publishes `run.updated` and `step.updated`.
- **Test-only fixture route** only through `router_with_fixture`, kept out of shipped code by the seam guard.

**Files changed (src):** `am-api/src/lib.rs`; `am-server/src/{lib,state,guard,routes,pairing,events,runs,envelope,secret,fixture}.rs`; `am-domain/src/runtime.rs` (`AppDataLayout::cli_token`); `am-app/src/{serving,main,runtime,lib}.rs`; `xtask/src/{seam_guard,main}.rs`, `justfile`; `web/app.js` (`kind` field, pairing prompt, launch code exchanged only when well formed).

**Tests:** 76 RULE-15 trust tests in 10 `rule_15_*.rs` files, 4 route-table tests, 4 real-binary `serve_project.rs` cases, 2 seam-guard tests, am-server unit tests; all written failing first. `just check`: 1054 passed, 8 skipped.

**Review:**

| Pass | Findings | Patched | Deferred | Accepted |
|---|---|---|---|---|
| First (quick lens and adversarial security lens, with live probes) | 16: high 0, medium 3, low 13 | 10 | 3 | 3 |

- No high finding, so no follow-up pass. Every patch ran tests first (red run logged), except P2 (wiring already right; tests added green) and P9 (wiring only).
- **P9 partial:** with stdin not a terminal the Enter reader is not started; `agentic serve &` from an interactive shell can still be stopped by SIGTTIN, because telling foreground from background needs nix's `process` feature, not enabled in the workspace. Documented: run serve in the foreground, or `agentic serve < /dev/null &`.
- **Deferred:** D1 a launch id for `seq` (story 6.1), D2 paging of run reads (3.3 onward), D3 session expiry (UI epics).
- **Accepted residuals:** A1 the launch code briefly in the `open` process's argv (inherent to AD-13's fragment, same-user rule); A2 a local process can lock pairing (by design); A3 macOS ACLs and folder owner not checked.
