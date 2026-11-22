---
title: 'Egress client blocks everything unlisted'
type: feature
ticket: 2
created: '2026-11-22'
status: done
baseline_revision: 'afff3b622980c3d01b43a8005419ffc255555bb2'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: [oversized]
deferred:
  - summary: >-
      The egress directory answers seats' allowed_hosts and MCP URLs from config on disk at open; a hand edit that widens them takes effect at the next open with no confirmation.
    evidence: |-
      Story 2.2 adversarial review, finding 3. AD-16's hold of a widening outside edit is epic 4 story 18 (with story 17's classifier); no run path sends seat requests yet.
    location: >-
      crates/am-app/src/egress.rs (ProjectDirectory::open)
    severity: medium
---

<intent-contract>

## Intent

**Problem:** `am-net`'s client is the spike's.
- Its policy belongs to the client, not the request, so the seat and Account a request carries are ignored.
- A remote host is pinned by name only; the port is not checked.
- The two local ports are open to any caller.
- No redirect is followed or re-checked.
- Only one kind of block is audited, and production passes no audit sink at all.
- Nothing proves that no telemetry host is compiled in.

R5 (RULE-9, AD-12, AD-48) needs every request checked against its own seat or Account, every block audited, and zero calls outside configured endpoints, measured on this client.

**Approach:** Each request names who it is for: a seat, an Account, or an MCP server. The client asks one injected directory what that caller may reach, and decides from that alone:
- a seat's `allowed_hosts` patterns;
- an Account's pinned endpoint (scheme, host and port);
- an MCP server's approved URL.

The client then resolves and pins DNS, follows at most 3 redirects with the same check at each hop, caps bodies, attaches credentials itself, and audits every refusal. A source scan fails the build on any fixed host that is not on a named list.

## Boundaries & Constraints

**Always:**
- **Who a request is for (AD-12, AD-48):** `HttpRequest.caller` names exactly one kind.
  - **Seat:** a tool, reader or fetch by a seat.
  - **Account:** a call to that Account's endpoint, with the seat it serves when there is one, for the audit.
  - **MCP server:** a call to that server's approved URL.
  - A request with no caller is refused. The skill-import kind arrives with epic 9 story 16.
- **One directory decides:** an `am-domain` port, `EgressDirectory`, answers three questions from the pinned config snapshot: a seat's host patterns, an Account's endpoint, an MCP server's approved URL.
  - The request carries ids only. No caller can hand the client its own allow-list.
  - `am-app` implements the directory. An unknown id is refused.
- **Seat requests:**
  - The URL must be admitted by one of the seat's `HostPattern`s (`https` on 443 or `http` on 80).
  - There is no loopback or private-address exception, and no IP literal.
- **Account requests:** the URL's scheme, host and port must equal the Account's endpoint exactly.
  - **Key use (AD-15 as amended 2026-11-22):** the directory answers for an Account only while that Account is confirmed: `models/accounts.yaml` holds exactly the bytes of its latest History entry, and the latest `account_added` audit note names the same provider, scheme, host and port. The check moves out of story 2.3's `accounts test` into the directory, so every path that sends a key gets it. `accounts test` keeps its wording.
  - Keyless loopback Accounts arrive with story 2.10. Until then, every Account request carries its key.
  - The reserved local Accounts resolve to `http://127.0.0.1:1234` and `http://127.0.0.1:11434`, defined once.
  - An owner-typed `http://127.0.0.1:<port>` endpoint is admitted for its own Account only.
  - Any loopback endpoint equal to the app's own serving port is refused. `agentic serve` gives the client its port after it binds.
  - The two local ports are no longer open to any other caller.
- **Addresses:**
  - Every DNS answer is checked. If any answer is private, loopback, link-local, CGNAT, metadata, multicast or unspecified, the request is refused.
  - NAT64 and 6to4 forms of those addresses are checked as the IPv4 address they carry.
  - The connection is pinned to the checked address, so a later DNS answer cannot move it.
  - An IP literal in the URL is refused for a seat. For an Account it is allowed only as its loopback endpoint.
- **Redirects:** at most 3 are followed, each one re-checked by the same caller's rule, then re-resolved and pinned again.
  - A 4th redirect is refused.
  - Only GET and HEAD are followed. A redirect answering any other method is refused.
  - An Account or MCP request never leaves its endpoint, so a redirect elsewhere is refused.
  - When a followed redirect changes scheme, host or port, the credential and every credential-like header are dropped for the rest of the chain.
- **Credentials:**
  - Only the client attaches a credential, from the request's `AuthRef` through `SecretStore`.
  - Caller-supplied `authorization`, `proxy-authorization`, `x-api-key`, `cookie` and `host` headers are refused.
- **Bodies:**
  - A response past the cap is cut with a worded error.
  - A request body past the cap is refused before anything is sent.
- **Audit (AD-25):** every refusal and every cut is audited.
  - This covers: no caller, an unknown id, a host or endpoint mismatch, a blocked address, a refused redirect, the 4th redirect, an oversize body, a refused header.
  - The entry is action `egress.blocked`, actor `System(am-net)`.
  - It records the destination's scheme, host and port, the reason, and the seat, Account or server id. It never records a path, a query, userinfo, a header or a body.
  - `am-app` supplies a sink that writes through the store's writer.
- **No telemetry:** `cargo xtask` gains a scan over `crates/*/src` (tests left out).
  - It finds every fixed host, by its URL or its bare name, and fails on any that is not on one named list, kept in the scan with a reason for each.
  - Today's list: the Anthropic and OpenAI provider endpoints, and `127.0.0.1` with the two local ports.
  - `just check` and CI run it.
  - It has no release host yet (epic 11 adds it) and no skill-import hosts (epic 9 story 16 adds them, as AD-12 says).
- **Only am-net builds an HTTP client:**
  - The existing clippy rule also bans `Client::default()` and `ClientBuilder::default()`.
  - A probe test builds a fixture crate that constructs a reqwest client and sees clippy fail.
- **The spike's live commands leave the release binary (owner decision 9, 2026-11-22):** `agentic spike c1` and `c4`, which read the Keychain entry `spike-anthropic` and call a provider with no configured Account, are removed from the release binary. The spike's recorded evidence (`docs/spike-report.md`) and its tests stay. Anything that still needs a binary is built only for development: a separate `[[bin]]` or example under a dev-only path, or a cargo feature that `just check`, CI and the release build never enable. A guard test fails if the release `agentic` binary still dispatches `spike c1` or `c4`.
- **Test seams are constructor parameters, never switches:** the resolver and the per-hop transport are traits passed to a second constructor used by tests.
  - `NetEgress::new` always uses the system resolver and reqwest.
  - No env var, flag or feature changes either.
  - A guard test fails if that second constructor is named anywhere under `crates/*/src`.
- **Tests first:** `crates/am-e2e/tests/trust/rule_09_*.rs` are written to fail first, with no network.

**Never:**
- No tainted-step URL set (epic 4 story 9), no `Fetcher`, robots or per-host spacing (AD-39), and no Doctor list of blocked calls (epic 9).
- No change to which hosts any seat or Account may reach. This story only enforces what config already says.
- No way for a request, a setting or a test hook to widen a destination.
- No new dependency.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Seat, listed | Seat with `docs.example.org`; GET `https://docs.example.org/a` | Allowed; pinned to the resolved public address | — |
| Seat, unlisted | Same seat; `https://other.example.net/` | Refused; audited with the seat id, host and port | `Blocked`, worded |
| Seat, wrong port or scheme | `https://docs.example.org:8443/`, `http://docs.example.org:8080/` | Refused; audited | — |
| Seat to loopback or private | `http://127.0.0.1:1234/`, `https://10.0.0.5/`, `http://169.254.169.254/` | Refused even if a pattern were to name it; audited | — |
| Account, its endpoint | Anthropic Account; `https://api.anthropic.com/v1/messages` | Allowed; credential attached by the client | — |
| Account, elsewhere | Same Account; another host, another port, or http | Refused; audited with the Account id | — |
| Local Account | `local-lmstudio` to `http://127.0.0.1:1234/v1/...`; a seat or another Account to that port | First allowed; the others refused | — |
| Owner-typed loopback | Compatible Account at `http://127.0.0.1:8080`; same port as the app's own | Allowed for that Account; refused when it is the app's port | — |
| No caller, unknown id | Request with no caller; a seat or Account the directory does not know | Refused; audited | — |
| DNS answer | A listed name answering a private or metadata address; answers `[public, private]` | Refused; audited | — |
| Rebinding | Name answers public at check; a second lookup would answer private | The connection goes to the checked address only; one lookup per hop | — |
| Redirect, allowed | Seat GET; 2 redirects among listed hosts | Followed; each hop checked and pinned | — |
| Redirect, refused | Redirect to an unlisted host, a private address, loopback or metadata; an Account redirected off its endpoint; a redirect on POST | Refused at that hop; audited | — |
| 4th redirect | A chain of 4 | Refused after the 3rd; audited | — |
| Auth on host change | Seat request with a credential redirected to another listed host | The next hop carries no credential and no credential-like header | — |
| Caller headers | A request setting `authorization` or `cookie` itself | Refused; audited | — |
| Oversize | A response past the cap; a request body past the cap | Response cut with `TooLarge`; request refused; both audited | — |
| Audit content | Any refusal of a URL with a path, query and userinfo | The entry holds scheme, host, port, reason and ids only | — |
| No telemetry | The workspace; a fixture source naming `telemetry.example.com` | The scan passes on the workspace and fails on the fixture, naming the host and file | — |
| Clippy | A fixture crate calling `reqwest::Client::new()` and one calling `Client::default()` | Clippy fails on each | — |
| Seam guard | A source file naming the test constructor | The guard fails | — |
| Existing callers | The Anthropic and OpenAI adapters, `accounts test`, the spike MCP client | Each names its caller, and all their tests pass | — |

</intent-contract>

## Code Map

- `crates/am-domain/src/net.rs` — `HttpRequest` (37-44), `Caller { seat, account }` (30-34), `AuthRef`, `EgressError` (93-102), `EgressClient` (104-106). `Caller` becomes one of three kinds. Add `Endpoint { scheme, host, port }` and the `EgressDirectory` port. The reserved local Accounts' endpoints are defined here once.
- `crates/am-domain/src/host.rs` — `HostPattern::admits(scheme, host, port)` (233).
- `crates/am-domain/src/audit.rs` — `AuditSink::record`, `AuditEvent { actor, action, target, outcome, hash }`. A block needs named arguments (seat, Account, reason): extend the event or add an `args` map.
- `crates/am-net/src/lib.rs` (478 lines):
  - `EgressPolicy` (31-48), `NetEgress::new` (64-74), `check_url` (83-139).
  - `is_blocked_ip` (143-172).
  - `route` (175-205): `tokio::net::lookup_host` at 188, pinned to `addrs[0]`.
  - The client is built per request with `.resolve(..)` and `redirect(Policy::none())` (252-259).
  - Headers are copied as given (266-268); auth is attached at 269-279.
  - The body pump (298-330).
  - `audit_block` (229-241) is called only at 244-249.
  - Local ports: `LOCAL_LMSTUDIO_PORT`, `LOCAL_OLLAMA_PORT` (27-28).
  - Split into pure decision functions (caller rule, address check, next hop) and a thin IO loop.
- `crates/am-store/src/lib.rs` — `WriteCommand::Audit { action, target, outcome, hash }` (218, 466), `Writer::exec`. A block's arguments go into the audit entry's `args`.
- `crates/am-app/src`:
  - `lib.rs:43-49` — `default_egress`.
  - `main.rs:148-151` — the accounts egress factory. `accounts.rs` — the policy in `test`, and story 2.3's endpoint function (to move or share, so one function derives an Account's endpoint).
  - `main.rs:235-242` — where `serve` binds and learns its port.
  - New: the directory over a `ConfigSnapshot`, and the store-backed audit sink.
- `crates/am-config` — `ConfigSnapshot::allowed_hosts(seat)` (`snapshot.rs:95`), `accounts()` (`snapshot.rs:38`), `Account.base_url` (`entities.rs:53-64`).
- **Callers to update:**
  - `crates/am-providers/src/anthropic.rs:330-345`;
  - `crates/am-providers/src/openai_compat.rs` (its request builder);
  - `crates/am-mcp/src/http_client.rs:71-78`;
  - `crates/am-app/src/main.rs:522`;
  - `crates/am-e2e/tests/spike/{live.rs:36, c5_mcp.rs:26-55}`;
  - `crates/am-testkit/tests/fakes.rs:313, 342, 359`;
  - `crates/am-app/tests/accounts_cli.rs` (its policy assertions).
- `clippy.toml` (8-19) and root `Cargo.toml:62-64` — the existing rule.
- `xtask/src/{denylist,runtime_paths}.rs` — the pattern for a source scan. `xtask/tests/supply_chain.rs` with `fixtures/supply-chain/*` — the pattern for a fixture-crate probe.
- **Trust tests:** `crates/am-e2e/tests/trust/rule_16_audit_log.rs` is the pattern. Each new file needs a `[[test]]` in `crates/am-e2e/Cargo.toml` (`tests/trust_targets.rs:27` enforces it). `am-testkit` gains a loopback HTTP server helper, a scripted resolver and a scripted transport.
- `docs/crate-notes/reqwest.md` — update for redirects and the pin.

## Tasks & Acceptance

**Execution:**
- [x] Tests first (no `src/` change): `rule_09_*.rs` for every matrix row, the xtask scan and probe tests, and the updated callers' tests. Log the red run.
- [x] `crates/am-domain` — the caller kinds, `Endpoint`, `EgressDirectory`, audit arguments.
- [x] `crates/am-net` — the decision functions, resolver and transport seams, redirects, caps, audit.
- [x] `crates/am-store`, `crates/am-app` — audit arguments, the directory, the sink, the own-port hand-over, the callers.
- [x] `crates/am-providers`, `crates/am-mcp`, `crates/am-testkit` — callers name their kind; test helpers.
- [x] `xtask`, `clippy.toml`, `justfile`, CI — the scan, the probe, the stricter rule.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the outcome and the audit entry match.
- Given the workspace, when `just check` runs, then it is green, with the new scan in it.

## Implementation Notes

### Red run (2026-11-22, tests first, before any `src/` change)

**Tests written (no file under any `src/` touched):**
- Trust tests, one target each in `crates/am-e2e/Cargo.toml`: `crates/am-e2e/tests/trust/rule_09_callers.rs` (seat listed, unlisted, wrong port or scheme, loopback or private; Account endpoint with key attached, elsewhere, private or LAN endpoint; reserved local Accounts defined once; owner-typed loopback on a real socket and the app's own port; MCP; no caller, unknown id, empty directory), `rule_09_addresses.rs` (DNS answers incl. NAT64, 6to4, mixed; rebinding: one lookup per hop, pinned; re-resolved per redirect hop), `rule_09_redirects.rs` (2 followed and pinned; 3 followed, 4th refused; seat, Account and MCP redirects refused; POST redirect refused; credential dropped on origin change for the rest of the chain), `rule_09_requests.rs` (caller headers, response cut, request body cap, audit content), `rule_09_audit_store.rs` (am-app's store-backed sink writes `egress.blocked` with args; chain verifies). Shared fakes in `tests/trust/support/egress.rs` (hand-built directory, scripted resolver, scripted transport, loopback server); they can move to am-testkit in the build.
- xtask: `xtask/tests/fixed_hosts.rs` (`cargo xtask fixed-hosts`), `seam_guard.rs` (`cargo xtask seam-guard`), `egress_clippy.rs` with `fixtures/egress-probe/` (real reqwest at the pin, the repo's clippy.toml, offline; control case passes), `gate_runs_egress_scans.rs` (`just check` runs both scans).
- Release guard: `crates/am-app/tests/spike_release.rs` (decision 9).
- Callers: `crates/am-providers/tests/callers.rs` (new: Anthropic, LM Studio, Ollama name their Account), `openai_compat.rs` (caller assertion), `crates/am-e2e/tests/spike/c5_mcp.rs` (MCP directory, refusal audited with the server id, requests name `McpServer`), `spike/live.rs` (own spike directory and `SPIKE_ANTHROPIC_ACCOUNT`; `c1_providers.rs`, `c4_files_to_models.rs` use it), `crates/am-app/tests/accounts_cli.rs` (factory takes the directory and the sink; the four `check_url` policy checks become directory checks; new `test_prints_a_block_and_says_it_is_not_in_the_audit_log`), `crates/am-store/tests/audit.rs` (new `an_audit_entrys_named_args_are_stored_and_exported`) and `args: Default::default()` added to every `WriteCommand::Audit` literal in `am-store/tests/{audit,project}.rs`, `am-app/tests/audit_cli.rs`, `am-e2e/tests/trust/rule_16_audit_log.rs`.

**API the tests pin (names decided here, standing rules: field names and test structure):**
- `am_domain::net`: `enum Caller { Seat { seat }, Account { account, seat: Option<String> }, McpServer { server } }`; `HttpRequest.caller: Option<Caller>` (`HttpRequest::get`/`post_json` keep `None`, so am-testkit's `fakes.rs` needs no change); `struct Endpoint { scheme, host, port }`; `trait EgressDirectory { seat_hosts(&str) -> Option<Vec<HostPattern>>; account_endpoint(&str) -> Option<Endpoint>; mcp_endpoint(&str) -> Option<Endpoint> }`; `fn reserved_local_account(&str) -> Option<Endpoint>`.
- `AuditEvent.args: BTreeMap<String, String>` (keys `scheme`, `host`, `port`, `reason`, and `seat`/`account`/`server`); `WriteCommand::Audit { .., args: BTreeMap<String, String> }`, kept next to `sha256`.
- `am_net`: `NetEgress::new(Arc<dyn EgressDirectory>, Arc<dyn SecretStore>, Arc<dyn AuditSink>)`; `NetEgress::with_seams(directory, secrets, audit, Seams { resolver, transport, limits })`; `NetEgress::set_own_port(&self, u16)`; `trait Resolver { lookup(&self, host, port) -> BoxFuture<Result<Vec<IpAddr>, EgressError>> }`; `trait Transport { send(&self, Hop) -> BoxFuture<Result<HttpResponse, EgressError>> }`; `struct Hop { method, url, addr: SocketAddr, headers, body }` (headers include the attached credential); `struct Limits { max_response_bytes, max_request_bytes }`.
- `am_app::egress::ProjectDirectory::open(root: &Path, places: &am_app::runtime::Places) -> Result<ProjectDirectory, String>`, implementing `EgressDirectory` over the project's config and its store read-only (no lock, no edit); it answers an Account only while confirmed (decision 8, AD-15 Key use) and the reserved local Accounts always.
- `am_app::egress::StoreAuditSink::new(am_store::Writer)`; `AccountsContext.egress: Arc<dyn Fn(Arc<dyn EgressDirectory>, Arc<dyn AuditSink>) -> Arc<dyn EgressClient> + Send + Sync>`.
- xtask tasks `fixed-hosts [--root]` and `seam-guard [--root]`, each a `just check` recipe.
- The test code was type-checked and clippy-clean against throwaway stubs of this API in a scratch copy outside the repo (no repo `src/` edited).

**Command 1** (compile red, every target that names the new API):
`cargo build --tests --keep-going -p am-domain -p am-net -p am-store -p am-app -p am-providers -p am-mcp -p am-e2e -p xtask --message-format short` exit 101:

```
crates/am-e2e/tests/trust/support/egress.rs: error[E0432]: unresolved imports `am_net::Hop`, `am_net::Limits`, `am_net::Resolver`, `am_net::Seams`, `am_net::Transport`
crates/am-e2e/tests/trust/support/egress.rs: error[E0432]: unresolved imports `am_domain::net::EgressDirectory`, `am_domain::net::Endpoint`, `am_domain::net::reserved_local_account`
crates/am-e2e/tests/trust/rule_09_callers.rs: error[E0432]: unresolved imports `am_domain::net::EgressDirectory`, `am_domain::net::reserved_local_account`
crates/am-e2e/tests/trust/rule_09_requests.rs: error[E0432]: unresolved import `am_net::Limits`
crates/am-e2e/tests/trust/rule_09_audit_store.rs: error[E0432]: unresolved import `am_app::egress`: could not find `egress` in `am_app`
crates/am-e2e/tests/spike/live.rs, spike/c5_mcp.rs, crates/am-app/tests/accounts_cli.rs: error[E0432]: unresolved imports `am_domain::net::EgressDirectory`, `am_domain::net::Endpoint` (and `reserved_local_account`)
crates/am-store/tests/audit.rs (7), project.rs (3), crates/am-app/tests/audit_cli.rs, crates/am-e2e/tests/trust/rule_16_audit_log.rs: error[E0559]: variant `WriteCommand::Audit` has no field named `args`
crates/am-providers/tests/callers.rs: error[E0308]: mismatched types: expected `Caller`, found `Option<Caller>`; error[E0223]: ambiguous associated type
crates/am-providers/tests/openai_compat.rs: error[E0223]: ambiguous associated type
error: could not compile `am-e2e` (tests "rule_09_callers", "rule_09_addresses", "rule_09_redirects", "rule_09_requests", "rule_09_audit_store", "spike", "rule_16_audit_log")
error: could not compile `am-app` (tests "accounts_cli", "audit_cli"); `am-store` (tests "audit", "project"); `am-providers` (tests "callers", "openai_compat")
```

**Command 2** (runtime red, the targets that compile today):
`cargo nextest run -p xtask -p am-app --test egress_clippy --test fixed_hosts --test seam_guard --test gate_runs_egress_scans --test spike_release --no-fail-fast` exit 100, 12 run: 1 passed (`egress_clippy::the_control_case_passes_clippy`), 11 failed:

```
FAIL xtask::gate_runs_egress_scans just_check_runs_the_fixed_host_scan_and_the_seam_guard
  `just check` runs fixed-hosts: check: fmt-check clippy test doctest check-deps check-forbid denylist runtime-paths validate
FAIL xtask::fixed_hosts the_repository_names_only_listed_hosts / a_fixed_host_not_on_the_list_fails_naming_host_and_file / hosts_not_yet_listed_fail / listed_hosts_comments_and_tests_pass
  xtask: unknown task `fixed-hosts`
FAIL xtask::seam_guard the_repository_never_names_the_test_constructor_in_shipped_code / a_source_file_naming_the_test_constructor_fails / the_definition_in_am_net_and_tests_folders_pass
  xtask: unknown task `seam-guard`
FAIL xtask::egress_clippy clippy_fails_on_each_way_to_build_a_reqwest_client
  client_default: clippy must fail:     Checking egress-probe-client-default v0.1.0 (...)   [client_new already fails on the rule]
FAIL am-app::spike_release the_release_binary_does_not_dispatch_spike_c1_or_c4
  ["spike", "c1", "--provider", "no-such-provider"] is still dispatched by the release binary: agentic: unknown provider no-such-provider
FAIL am-app::spike_release the_binarys_usage_no_longer_lists_spike_c1_or_c4
  the usage lists a removed spike command: usage: ... | agentic spike c1|c3|c4|c7|local-context
```

**Command 3** (decision 8, added at the coordinator's request: the confirmed check at the directory level). Five tests appended to `crates/am-app/tests/accounts_cli.rs` (they reuse its add/remove helpers): `the_project_directory_answers_an_account_added_through_the_command`, `the_project_directory_refuses_every_account_of_a_hand_edited_file` (base_url host, base_url port, provider; every Account of the file refused, reserved locals still answered; file put back byte for byte answers again), `the_project_directory_refuses_an_account_never_added`, `the_project_directory_refuses_an_account_removed_and_put_back_by_hand`, `an_unconfirmed_accounts_request_reads_no_key_and_is_audited` (real `NetEgress::new` over the directory, counting secret store: zero Keychain reads, `egress.blocked` with `args.account`).
`cargo build --tests -p am-app --message-format short` exit 101:

```
crates/am-app/tests/accounts_cli.rs: error[E0433]: cannot find `egress` in `am_app`: could not find `egress` in `am_app` (2)
crates/am-app/tests/accounts_cli.rs: error[E0432]: unresolved imports `am_domain::net::EgressDirectory`, `am_domain::net::Endpoint`, `am_domain::net::reserved_local_account`
crates/am-app/tests/audit_cli.rs: error[E0559]: variant `WriteCommand::Audit` has no field named `args`
error: could not compile `am-app` (test "accounts_cli") due to 3 previous errors
error: could not compile `am-app` (test "audit_cli") due to 1 previous error
```

`cargo nextest run -p am-e2e --test trust_targets` passes (every `rule_09_*.rs` has its target); `cargo xtask denylist`, `runtime-paths` and `check-forbid` stay clean with the new test files.

### Test changed after the red run (2026-11-22, coordinator decision: option 1)

- **What the pinned clippy cannot do:** on toolchain 1.100, `disallowed-methods` cannot ban `reqwest::Client::default()` or `reqwest::ClientBuilder::default()`. The paths `reqwest::Client::default` and `reqwest::ClientBuilder::default` only warn "does not refer to a reachable function", and clippy still passes. A `<reqwest::Client as Default>::default` path is silently ignored. The call resolves to `core::default::Default::default`, which can be banned only for every type at once.
- **The choice:** `clippy.toml` gains `disallowed-types` entries for `reqwest::Client` and `reqwest::ClientBuilder` (reason "use am_net::EgressClient (AD-12)"). The existing `disallowed-methods` entries stay. am-net allows both lints on the one statement that builds its client (`#[allow(clippy::disallowed_methods, clippy::disallowed_types)]` in `ReqwestTransport::send`), never crate-wide.
- **The test:** in `xtask/tests/egress_clippy.rs`, the `client_default` and `builder_default` cases accept "disallowed type" as the reason, and `client_new` keeps asserting "disallowed method". A new case, `fixtures/egress-probe/cases/names_type.rs`, has a function that takes `&reqwest::Client`; it is refused too. The control case still passes.
- **Why stricter, not weaker:** the type ban refuses every way to build a client outside am-net, and also any code that holds or passes one. Before, only four named constructors were refused.

### Build (2026-11-22)

**Changed:**
- `am-domain`:
  - `Caller` is an enum (`Seat`, `Account { account, seat }`, `McpServer`), and `HttpRequest.caller` is `Option<Caller>`.
  - New `Endpoint`, `EgressDirectory` and `reserved_local_account`. The reserved ids and ports are defined once.
  - `EgressDirectory` gains one provided method, `account_refusal(&self, id) -> Option<String>` (default `None`), so a refusal can say why.
  - `AuditEvent.args`. The literals in am-tools and am-engine are updated.
- `am-store`: `WriteCommand::Audit { .., args }` is stored as the entry's `args`, next to `sha256`.
- `am-net`, rewritten:
  - Pure decisions: `check_caller`, `check_address`, `url_target` and `url_endpoint`.
  - Seams: `Resolver`, `Transport`, `Hop`, `Limits` and `Seams`. `NetEgress::new` always uses the system resolver and reqwest; `with_seams` is for tests only.
  - `set_own_port`.
  - Address checks cover NAT64, 6to4, IPv4-mapped and IPv4-compatible forms, and the connection is pinned to the checked address.
  - Redirects: up to 3, each re-checked by the caller's rule, re-resolved and pinned. The credential is dropped for the rest of the chain on any change of scheme, host or port.
  - Caller-set credential and `host` headers are refused. Request and response bodies are capped.
  - Every refusal and cut is audited as `egress.blocked`.
  - `EgressPolicy`, `check_url` and `route` are removed.
- `am-providers` and `am-mcp`: every request names its caller. `EgressHttpClient::new(egress, server)`.
- `am-app`, new module `egress.rs`:
  - `ProjectDirectory`, with the confirmed check moved here from `accounts test`.
  - `StoreAuditSink`.
  - `LocalAccountsOnly`, used by `spike local-context`.
- `am-app`, the commands:
  - `accounts test` keeps its wording. It builds the client over the project directory, and prints a block, saying it is not in the audit log. New `pub type EgressFactory`.
  - `serve` hands its bound port to the project's egress client, which writes to the store-backed sink.
  - `spike c1` and `c4` are removed from the binary; the spike usage exits 2.
  - `default_egress` and `SPIKE_ANTHROPIC_ACCOUNT` are removed.
- `xtask`, `justfile` and docs:
  - New `xtask` tasks `fixed-hosts` and `seam-guard`, with `src/shipped.rs`, which skips `#[cfg(test)]` code. Both are `just check` recipes.
  - `clippy.toml`, as above.
  - `docs/crate-notes/reqwest.md`.

**Choices (standing rules: engineering calls, reported):**
- **Two extra entries on the fixed-host list,** each with its reason in `xtask/src/fixed_hosts.rs`:
  - `localhost`, only with a placeholder port (`{port}`, `<port>`): `agentic serve`'s Host, Origin and CSP checks and the base-URL help text name it. The egress client never reaches it: no host pattern admits it, and no endpoint is it.
  - `json-schema.org` (443): the JSON Schema reference named in the words of am-config's schema fix message. It is never contacted.
- **Bare-name rule:** a bare name counts as a host only when the whole string literal is that name, with an optional `:port`. URLs count anywhere in a literal. Without this rule, example names in refusal messages (`example.org`) would be flagged. A host named inside a sentence is not caught.
- **Account and MCP endpoint rules:**
  - The scheme, host and port must match the endpoint exactly.
  - An IP literal is allowed only as exactly `127.0.0.1`, and never on the app's own port. The own-port refusal applies to MCP servers too.
  - A non-loopback endpoint must be https.
- **GET-only redirects:** `HttpMethod` has no `Head` variant, so only GET is followed.
- **`MissingSecret` is not audited:** a key missing from the Keychain is not a block.
- **Shared fakes:** the shared test fakes stay in `crates/am-e2e/tests/trust/support/egress.rs`; they were not moved to am-testkit.

### Review fixes: red run (2026-11-22, tests first, before any `src/` change)

**Tests written or extended** (no file under any `src/` touched):
- `crates/am-e2e/tests/trust/support/egress.rs`:
  - The directory can name the app's own port (`EgressDirectory::own_port`).
  - The Rig's keychain counts reads (`rig.secrets.reads()`).
  - Scripted replies can arrive late (`Reply::late`) or stall (`Reply::stalling`).
  - The scripted transport renders the hop's credential, which now travels apart from the headers (`Hop.credential`).
  - `roomy()` keeps the default timeouts.
- `rule_09_callers.rs`:
  - `a_credential_goes_only_with_its_own_account`: a seat, an MCP server, another Account and a reserved local Account, each carrying a key reference. Each is refused with reason "the credential does not belong to this caller" and audited, with 0 Keychain reads.
  - `the_reserved_local_ports_are_refused_to_every_other_caller`.
  - `the_own_port_the_directory_names_is_refused`.
- `rule_09_redirects.rs`:
  - `the_credential_is_dropped_…` is rewritten. An Account's key stays within its endpoint, and the hop off it (another host, port or scheme) is never made. A seat's `mcp-session-id` header is stripped after an origin change.
  - The 4th-redirect refusal must say "after 3 redirects".
- `rule_09_requests.rs`:
  - Four more refused headers: `api-key`, `x-goog-api-key`, `x-auth-token`, `cookie2`.
  - `the_default_timeouts_are_bounded` (30 s head, 120 s idle).
  - Two timeout tests, each refused and audited: `a_response_head_that_never_arrives_…` and `a_body_that_goes_idle_…`.
  - The redirect audit-content case no longer puts a key on a seat request.
- `rule_09_addresses.rs`: IPv4-translated `::ffff:0:a00:5`, and Teredo with a private client or server, are refused; a public Teredo address passes.
- `rule_09_audit_store.rs`: `the_store_sink_writes_only_egress_blocks`.
- `crates/am-store/tests/config_history.rs`: `only_a_config_intent_writes_a_config_note`.
  - A `WriteCommand::Audit` with a `config.*` action is refused.
  - `Reader::config_note_args` returns only a committed intent's notes. A raw-SQL note is not among them.
- `crates/am-app/tests/accounts_cli.rs`: `test_of_an_account_on_the_apps_own_port_is_refused_with_no_key_read`. The port file names 8080. The directory refuses the Account at `http://127.0.0.1:8080` and says why; `accounts test` is refused with 0 Keychain reads, and no client is built.
- `crates/am-app/tests/serve_project.rs`: `serve` writes `RuntimeLayout::serve_port()` (mode 0600, holding the bound port) and removes it at a clean stop.
- `crates/am-config/tests/validate.rs`: `an_http_mcp_server_on_this_mac_is_named_by_127_0_0_1_only`.
- `xtask/tests/fixed_hosts.rs`:
  - `more_top_level_domains_and_ipv6_literals_fail`: `collector.vercel.app`, `t.ly`, `[2001:db8::1]`, `2001:db8::2`, `sink.example.de:8443`.
  - `a_cfg_test_on_a_field_variant_or_arm_excludes_only_that_item`.
- `xtask/tests/seam_guard.rs`: `shipped_am_net_code_calling_the_test_constructor_fails`.
- `xtask/tests/egress_clippy.rs`: a new case, `default_in_body`.

**Command 1** (compile red):
`cargo build --tests --keep-going -p am-e2e -p am-app -p am-store -p am-config -p xtask --message-format short` exits 101:

```
crates/am-app/tests/accounts_cli.rs: error[E0599]: no method named `serve_port` found for struct `RuntimeLayout`; no method named `own_port` found for struct `ProjectDirectory`
crates/am-app/tests/serve_project.rs: error[E0599]: no method named `serve_port` found for struct `RuntimeLayout` (3)
crates/am-e2e/tests/trust/rule_09_requests.rs: error[E0560]: struct `Limits` has no field named `head_timeout` / `idle_timeout`
crates/am-e2e/tests/trust/support/egress.rs: error[E0407]: method `own_port` is not a member of trait `EgressDirectory`; error[E0609]: no field `credential` on type `Hop`
crates/am-store/tests/config_history.rs: error[E0599]: no method named `config_note_args` found for struct `Reader` (2)
error: could not compile `am-app` (tests "accounts_cli", "serve_project"), `am-e2e` (tests "rule_09_addresses", "rule_09_audit_store", "rule_09_callers", "rule_09_redirects", "rule_09_requests"), `am-store` (test "config_history")
```

**Command 2** (runtime red):
`cargo nextest run -p am-config --test validate an_http_mcp_server` and `cargo nextest run -p xtask --test fixed_hosts --test seam_guard --test egress_clippy --no-fail-fast`:

```
FAIL am-config::validate an_http_mcp_server_on_this_mac_is_named_by_127_0_0_1_only   (left: 0, right: 2: localhost and a port-less URL are accepted)
FAIL xtask::fixed_hosts a_cfg_test_on_a_field_variant_or_arm_excludes_only_that_item
FAIL xtask::fixed_hosts more_top_level_domains_and_ipv6_literals_fail
FAIL xtask::seam_guard shipped_am_net_code_calling_the_test_constructor_fails
12 xtask tests run: 9 passed, 3 failed
```

`egress_clippy`'s new `default_in_body` case passes on arrival: the `disallowed-types` ban already refuses the call. It is kept as a proof, not a behaviour change.

No test is written for the reserved local Accounts' single definition (am-providers, am-config, am-app). It is a refactor, and the existing tests cover it (`callers.rs`, `rule_09_callers` reserved tests, the am-config reserved-id tests).

### Review fixes: build (2026-11-22)

- **Credential ownership (high):** in `am-net`, a request whose `auth` is set is refused before any Keychain read, lookup or hop, and audited with reason "the credential does not belong to this caller". The only exception is a caller that is `Account { account }` with `account == auth.account_id` and is not a reserved local Account. The directory's answer for the Account is checked as before.
- **Config notes only from an intent (medium):**
  - `am-store` refuses a `WriteCommand::Audit` whose action starts with `config.`.
  - New `Reader::config_note_args(action, target)` returns only the notes written right after a config History entry (`config.edited`, `created`, …) in the same transaction (the same `at`).
  - am-app's confirmed check reads only those notes.
  - `StoreAuditSink` writes only `egress.blocked` and drops any other event with a warning.
- **The credential stays a `SecretString` (medium):** a new `am_net::Credential` carries the key apart from the headers, in the new field `Hop.credential`. `ReqwestTransport` builds the header from it and calls `set_sensitive(true)`. The scripted test transport renders it as the header it becomes.
- **Own port (medium):**
  - New `RuntimeLayout::serve_port()` (`serve.port`). `agentic serve` writes the bound port there, owner-only, by a temp file and a rename (`runtime::record_serve_port`), and removes it at exit and on the early-exit path.
  - `ProjectDirectory::open` reads the file. It answers no Account whose endpoint is `127.0.0.1:<that port>` and says why.
  - New provided trait method `EgressDirectory::own_port()`. The client refuses that port to every Account and MCP server, as well as the port `set_own_port` hands it.
  - `accounts test` checks the port before its Keychain read, in its own words.
- **Timeouts (medium):** `Limits` gains `head_timeout` (30 s) and `idle_timeout` (120 s). A hop whose response head does not arrive in time, and a body that sends nothing for that long, each give `EgressError::Network("… timed out")` and are audited.
  - The review triage asked for a connect timeout per hop. The coordinator's message asked for a head timeout and an idle timeout; this follows the message. Every reqwest hop also keeps its 10 s connect timeout.
- **Reserved local ports (low):** `127.0.0.1:1234` and `:11434` are refused to every caller except their own reserved Account.
- **MCP `url` rule (low):** am-config's validator checks an MCP `url` with its own rule, `mcp_url`: https, or `http://127.0.0.1:<port>` with an explicit port. It refuses `localhost` and a port-less `http://127.0.0.1`. The fix names the 127.0.0.1 form only.
  - Account `base_url`s keep `web_url`. Its test still expects `localhost` there.
  - No fixture used `localhost` for an MCP server.
- **Headers (low):**
  - Refused when a caller sets them, and stripped on an origin change: `authorization`, `proxy-authorization`, `x-api-key`, `api-key`, `x-goog-api-key`, `x-auth-token`, `cookie`, `cookie2`. `host` is also refused.
  - `mcp-session-id` is allowed from any caller and stripped on an origin change.
- **Addresses (low):** IPv4-translated `::ffff:0:a.b.c.d` is checked as the IPv4 address it carries. Teredo `2001::/32` is refused when either its server address or its inverted client address is blocked.
- **One definition of the reserved local Accounts (low):** am-domain adds `RESERVED_LOCAL_ACCOUNTS`. am-config's `RESERVED_ACCOUNTS`, am-providers' `lm_studio` and `ollama`, and am-app's `spike local-context` all read am-domain's ids and endpoints.
- **`fixed-hosts` (low):**
  - A `#[cfg(test)]` excludes only its own item, ending at a `;` or `,` at its own depth, at the `}` of a body it opened, or before the bracket that closes the enclosing item.
  - The TLD list now holds the generic ones (`app` included) and every two-letter country code but `rs`, `md`, `py` and `sh`. A whole literal such as `Agentic.app` therefore now counts as a host. No such literal exists in shipped code today.
  - Bracketed and bare IPv6 literals are found. `::` alone is not an address.
- **Clippy probe (low):** new case `default_in_body`.
- **Wording (low):** the 4th redirect is refused with "no further redirect is followed after 3 redirects".
- **`seam-guard` (low):** inside `crates/am-net/src` only the definition line (`fn with_seams`) is skipped, so a call fails there too.
- **Tests that cover the edited files pass:**
  - `am-domain`, `am-net` and `am-mcp`;
  - in `am-e2e`, every `rule_09_*` target and `spike`;
  - `am-store`, `am-config` and `am-providers`;
  - in `am-app`, `accounts_cli`, `serve_project` and `spike_release`;
  - `xtask`.
- **Gate:** fmt and clippy `-D warnings` are clean.

### Review fixes, round 2: red run (2026-11-22, tests first, before any `src/` change)

**Tests written or changed:**
- `rule_09_requests.rs`: the timeouts now depend on the caller kind.
  - `Limits { seat, account, mcp: Timeouts { head, idle } }`.
  - `the_default_timeouts_depend_on_the_caller_kind`: seat 30 s / 120 s; Account no head timeout / 300 s idle; MCP neither.
  - Seat head and idle timeouts are refused and audited.
  - `an_accounts_call_has_no_head_timeout_but_an_idle_timeout`.
  - `an_mcp_request_has_no_head_or_idle_timeout`.
- `support/egress.rs`: `EgressDirectory::own_ports()` replaces `own_port()`.
- `crates/am-store/tests/config_history.rs`: `config_note_args` returns each note's actor and the file of its History entry. Checked for an owner note on the Accounts file, an owner note on `company.yaml`, and a System actor's note.
- `crates/am-app/tests/accounts_cli.rs`:
  - `a_note_by_a_system_actor_or_with_another_files_entry_does_not_confirm`: the control (the owner, on the Accounts file's intent) confirms.
  - `every_running_servers_port_is_refused_and_a_stale_record_removed`: two live records (8080, 3001) are refused; a stale record (pid 99999999) is ignored and removed.
  - The own-port wording is now "the app's own server is listening on port 8080", without "this project".
- `crates/am-app/tests/serve_project.rs`: `serve`, with or without a project, records `<app data>/serving/<pid>` (0600, holding the port), and removes it at a clean stop.
- `xtask/tests/fixed_hosts.rs`: `a_placeholder_scheme_still_names_the_host`.

**Command 1** (compile red):
`cargo build --tests --keep-going -p am-e2e -p am-app -p am-store --message-format short`:

```
crates/am-app/tests/accounts_cli.rs: error[E0599]: no method named `own_ports` found for struct `ProjectDirectory`; error[E0425]: cannot find function `serving_dir` in module `am_domain::runtime`
crates/am-app/tests/serve_project.rs: error[E0425]: cannot find function `serving_dir` in module `am_domain::runtime` (2)
crates/am-e2e/tests/trust/rule_09_requests.rs: error[E0432]: unresolved import `am_net::Timeouts`; error[E0560]: struct `Limits` has no field named `seat` / `account` / `mcp`
crates/am-e2e/tests/trust/support/egress.rs: error[E0407]: method `own_ports` is not a member of trait `EgressDirectory`
crates/am-store/tests/config_history.rs: error[E0609]: no field `args` / `actor` / `file` on type `BTreeMap<String, String>` (6)
```

**Command 2** (runtime red):
`cargo nextest run -p xtask --test fixed_hosts`: 7 tests run, 6 passed, 1 failed (`a_placeholder_scheme_still_names_the_host`).

### Review fixes, round 2: build (2026-11-22)

- **Timeouts per caller kind:**
  - `am_net::Limits` now holds `seat`, `account` and `mcp` `Timeouts { head, idle }` (each `Option<Duration>`) and `Limits::timeouts(&Caller)`.
  - Defaults:

    | Caller | Head | Idle |
    |---|---|---|
    | Seat | 30 s | 120 s |
    | Account | none | 300 s |
    | MCP | none | none |

  - The engine's step timeout and cancel cover an Account's call (AD-19); its session and cancel cover an MCP request.
  - The head and idle waits apply only where a limit is set.
- **Only the owner's note on the Accounts file confirms:**
  - `Reader::config_note_args` returns `am_store::ConfigNote { actor, file, args }`, where `file` is the `file` arg of the History entry the note follows.
  - `Records::check` accepts only a note whose actor is `Actor::Human` and whose file is `models/accounts.yaml`.
- **Every running server's port:**
  - New `am_domain::runtime::serving_dir(app_data)` (`<app data>/serving/`).
  - `agentic serve`, with or without a project, records `<pid>` holding its port, owner-only (`runtime::record_serving`, temp file and rename), after bind. It removes the record at exit and on every early exit after it (`clear_serving`). The per-project `serve.port` stays.
  - `runtime::serving_ports` reads the folder, ignores and removes a record whose process is not running (`pid_alive`), and returns the rest.
  - `ProjectDirectory::open` refuses every recorded port, and the project's own, to its Accounts.
  - The trait's provided method is now `EgressDirectory::own_ports() -> Vec<u16>`. The client refuses every listed port to Accounts and MCP servers.
  - Wording: "the app's own server is listening on port N", in the directory, in `accounts test` and in am-net.
- **`fixed-hosts`:** a URL whose scheme is a format placeholder (`{scheme}://`, `{}://`) still records its host, with no default port.
- **Tests run** (the files edited): `am-domain`, `am-net`, `am-store`; in am-e2e the `rule_09_*` targets and `spike`; in am-app the lib, `accounts_cli`, `serve_project` and `spike_release`; `xtask`. All pass, and fmt and clippy `-D warnings` are clean.

## Plan Change Log

## Review Triage Log

### 2026-11-22 — Review pass (quick lens and an adversarial egress lens)
- verdicts: 18 findings (7 adversarial, 11 quick; one duplicate pair) — high 1, medium 5, low 11, false 0, maybe-false 0. Both lenses found URL parsing, DNS pinning, redirect re-checks, body caps, audit content, Account confirmation and the test-seam guard sound.
- findings:
  - `[high]` `[patch]` (both lenses) The client attaches `req.auth`'s key whatever the caller is. A seat or MCP request, or an Account request naming another Account's key, sends a key the directory never answered for, which defeats decision 8 and AD-48. Fix: refuse, and audit, any credential unless the caller is `Account { account }` with `account == auth.account_id`, not a reserved local id, and answered by the directory. Tests for each mismatch.
  - `[medium]` `[patch]` (quick) `WriteCommand::Audit` gained free-form args and `StoreAuditSink` writes any event, so in-app code holding a writer or sink can write a `config.widened`/`account_added` note that confirms an Account. Fix: the store refuses `WriteCommand::Audit` with a `config.*` action (those come only with a config intent), `StoreAuditSink` writes only `egress.blocked`, and the confirmed check accepts only notes written with a config History entry.
  - `[medium]` `[patch]` (quick) The key left `SecretString` for the whole request as plain `(String, String)` header pairs, and the sensitive flag `bearer_auth` set is lost. Fix: hold the credential as `SecretString` until each hop builds its header, and mark it sensitive.
  - `[medium]` `[patch]` (both) The app's own port is known only to `serve`'s client, which nothing uses yet, so `accounts test` can send a key to the app's own port while `serve` runs. Fix: `serve` records its port in the runtime folder (a `RuntimeLayout` path, owner-only, removed at exit), and every client am-app builds for the project reads it through the directory and refuses it. Test.
  - `[medium]` `[defer]` (adversarial) The directory answers seats and MCP servers from config on disk at open, with no confirmation of an outside edit, so a hand-added host or MCP server takes effect at the next open. AD-16's hold of a widening outside edit is epic 4 story 18's (with story 17's classifier), and no run path sends seat requests yet. Recorded in `deferred`.
  - `[medium]` `[patch]` (adversarial) No read or total timeout, so a server that accepts and never answers holds a request forever. Fix: an idle timeout between bytes and a connect timeout per hop, refused and audited.
  - `[low]` `[patch]` (adversarial) An MCP server may use 127.0.0.1:1234 or :11434. Fix: the reserved local ports are refused to every caller but their own Account.
  - `[low]` `[patch]` (both) The validator accepts `http://localhost:<port>` for an MCP server, which the client always refuses. Fix: the validator's MCP rule names `http://127.0.0.1:<port>` only.
  - `[low]` `[patch]` (adversarial) The credential-like header list is narrow. Fix: refuse and strip on an origin change `api-key`, `x-goog-api-key`, `x-auth-token` and `cookie2` too (`mcp-session-id` stays allowed on MCP requests and is stripped on any origin change).
  - `[low]` `[patch]` (adversarial) `::ffff:0:a.b.c.d` and Teredo `2001::/32` are not checked as the IPv4 they carry. Fix: add both. A network-specific NAT64 prefix cannot be known in general; recorded.
  - `[low]` `[patch]` (quick) The reserved local Accounts are not defined once (am-providers, am-config and am-app repeat ids or URLs). Fix: all read am-domain's definitions.
  - `[low]` `[patch]` (quick) `fixed-hosts`' test-range rule fails open on a `#[cfg(test)]` on a field, variant or arm. Fix: such an attribute excludes only its own item.
  - `[low]` `[patch]` (quick) Bare names are found only on a TLD subset, and bare IPv6 literals never. Fix: a broader TLD list (every two-letter country code that is not a common file extension, plus the common generic ones), and bracketed or `::` IPv6 literals.
  - `[low]` `[patch]` (quick) The clippy probe names the type in each case's signature, so it does not prove the `default()` call itself is refused. Fix: a case that calls `reqwest::Client::default()` in a body returning `()`.
  - `[low]` `[patch]` (quick) The redirect refusal reads "past the 3th". Fix: the wording.
  - `[low]` `[reject]` (quick) `StoreAuditSink::record` blocks a tokio worker for a durable write. A block is rare, its audit entry must be durable before the refusal returns (AD-25), and the write is milliseconds.
  - `[low]` `[patch]` (quick) `seam-guard` skips all of `crates/am-net/src`, not only the definition. Fix: it skips only the `with_seams` definition and the seam module.

### 2026-11-22 — Follow-up review pass (the review fixes)
- verdicts: 4 findings — high 0, medium 1, low 3, false 0, maybe-false 0. The reviewer found the credential rule sound: no path reads or sends a key except for that same confirmed, non-reserved Account, and the key reaches no Debug, log, audit or error text.
- findings:
  - `[medium]` `[patch]` The 30 s head timeout covers upload and the server's work before its first byte, so a local model loading or reading a long prompt, a large upload or a long MCP tool call fails (RULE-20's zero-key path), and a quiet MCP SSE stream is cut every 120 s. Fix: timeouts by caller kind (seat: head 30 s, idle 120 s; Account: no head timeout, idle 300 s; MCP: none; the engine's step timeout and cancel cover model and tool calls, AD-19).
  - `[low]` `[patch]` A `config.widened` note under any actor, with any History entry, still confirms an Account through the intent path. Fix: only a Human-actor note written with a `models/accounts.yaml` History entry confirms.
  - `[low]` `[patch]` The own port is recorded per project, so another project's Account on the same loopback port is not refused, and `serve` without `--project` records nothing. Fix: every `serve` also records its port in one user-level folder (per process, stale files ignored), and every directory refuses those ports.
  - `[low]` `[patch]` `fixed-hosts` misses a URL whose scheme is a format placeholder. Fix: `{…}://` counts as a scheme.

## Design Notes

**Approved by the owner, 2026-11-22:** "Approve 2.2 as written, with decision 9 = remove spike c1/c4 from the release binary (keep evidence/tests; dev-only build if needed). Decision 8 approved." On decision 10, the owner chose global, user-level Accounts with globally unique ids that projects reference. That is a separate REPLAN (AD-17), which must land before a run path or Doctor sends a key. It does not change this story: the directory hides where Accounts come from.

**The decisions as approved:**

1. **Requests carry ids; one directory decides.** The alternative is that each request carries its own allow-list, which any caller could widen. Recommended: the directory.
2. **Redirects.**
   - Only GET and HEAD are followed.
   - An Account or MCP request never leaves its endpoint.
   - Credentials are dropped on any change of scheme, host or port.

   This is stricter than the ticket's "auth stripped on a host change".
3. **`agentic accounts test` and the audit (decided by the owner, 2026-11-22).** That command opens no store, so that it works while `serve` holds the project. A block there cannot be written to the audit log. It prints the block and says it is not in the audit log.
4. **No-telemetry as a source scan with a named list.** Every fixed host in shipped code must be on the list, with its reason. The list today is the two provider endpoints and the two local ports.
5. **The app's own port.** `serve` hands its port to the client, which refuses any Account endpoint on it (AD-12). This closes the item story 3 could not.
6. **Scope.**
   - The MCP caller kind is included, because the spike's MCP client must keep working once callerless requests are refused.
   - The tainted-step URL set waits for epic 4 story 9.
7. **Test seams.** They are constructor parameters only, with a guard that shipped code never names the test constructor. This is the same rule as the fake terminal.
8. **One confirmed check for every key (AD-15 Key use).** The rule 2.3 put in `accounts test` moves into the directory, so a run, Doctor or any later caller cannot send a key without it.
9. **The spike's live commands.** `agentic spike c1` and `c4` in the release binary read the Keychain entry `spike-anthropic` and send it with no check, because that id is not a configured Account. Once requests need a known caller, they are refused anyway. Recommended: remove them from the release binary, keeping the spike's recorded evidence and tests. Alternative: keep them and reserve the `spike-` prefix.
10. **Not decided here (yours):** AD-17's Keychain entries are named by Account id only, so two projects with an Account of the same id share one key. The architect lists this as needing an AD-17 change before a run path or Doctor sends keys. It does not block this story, which only decides where a request may go.

## Verification

**Commands:**
- `cargo nextest run -p am-domain -p am-net -p am-store -p am-app -p am-providers -p am-mcp -p am-e2e -p xtask` -- expected: all pass.
- `just check` -- expected: green.

**Coordinator decision before phase 2 (2026-11-22, standing rules: the safe, reversible engineering choice):** `ProjectDirectory::open` on a project never opened on this Mac (no project id, or no runtime folder) returns a directory that refuses every Account (no confirmation record can exist), still answers the reserved local Accounts and the seats from the project config, and says why in each refusal. It fails closed and never errors the whole command.

## Auto Run Result

Status: built (2026-11-22). The owner approved the plan before any test (decisions 1–10; decision 9 = remove `spike c1`/`c4` from the release binary).

**Summary:**
- **Requests:** every request names one caller (a seat, an Account or an MCP server), and the client checks it only against what one `EgressDirectory` answers:
  - a seat's `HostPattern`s, over `https` 443 or `http` 80, with no loopback, private address or IP literal;
  - an Account's exact endpoint, while it is confirmed (AD-15 Key use, now in the directory: a Human note written with a `models/accounts.yaml` History entry);
  - an MCP server's URL.
- **Local and own ports:** the reserved local ports are reachable only by their own Account. The ports of every running `agentic serve` are refused.
- **Credentials:** a credential goes only with its own confirmed Account, is held as a `Credential`, and is marked sensitive. Caller-set credential headers are refused.
- **DNS and redirects:** every DNS answer is checked (NAT64, 6to4, IPv4-mapped, IPv4-translated and Teredo included) and the connection pinned to it. At most 3 GET redirects, each re-checked; the credential and credential-like headers are dropped after any change of origin.
- **Limits:** bodies are capped both ways. Timeouts depend on the caller kind.
- **Audit:** every refusal is written as `egress.blocked`, with scheme, host, port, reason and ids only, through `StoreAuditSink`. `accounts test` prints a block it cannot audit (owner).
- **Build gates:** `fixed-hosts` fails the build on an unlisted fixed host; `seam-guard` on shipped code naming the test seams; clippy on any reqwest client outside am-net (method and type bans).
- **Spike:** `spike c1` and `c4` are gone from the release binary.

**Files changed:**
- `crates/am-domain/src/{net,audit,runtime,lib}.rs`
- `crates/am-net/src/lib.rs` (rewritten)
- `crates/am-store/src/{lib,audit}.rs`
- `crates/am-config` (`mcp_url`, reserved ids)
- `crates/am-providers/src/{anthropic,openai_compat}.rs`
- `crates/am-mcp/src/{http_client,lib}.rs`
- `crates/am-app/src/{egress,accounts,main,lib,runtime}.rs`
- `xtask/src/{fixed_hosts,seam_guard,shipped,main,forbid}.rs`
- `clippy.toml`, `justfile`, `docs/crate-notes/reqwest.md`
- Tests:
  - `crates/am-e2e/tests/trust/rule_09_{callers,addresses,redirects,requests,audit_store}.rs` and `support/egress.rs`;
  - `xtask/tests/{fixed_hosts,seam_guard,egress_clippy,gate_runs_egress_scans}.rs` and `fixtures/egress-probe/`;
  - `crates/am-app/tests/{spike_release,accounts_cli,serve_project}.rs`;
  - `crates/am-providers/tests/callers.rs`;
  - the store and spike tests.

**Review:** a quick and an adversarial lens, then one follow-up pass. The Review Triage Log has every row.
- **Pass 1:** 18 findings. 15 patched (1 high: a credential not tied to its caller; 4 medium; 10 low), 1 deferred (medium: seats' and MCP servers' config is read from disk at open, which is epic 4 story 18's hold), 1 low rejected (the audit write's durable wait).
- **Pass 2:** 4 findings (1 medium: timeouts that would break local models and MCP; 3 low), all patched. The reviewer found the credential rule sound.
- **Follow-up recommended:** false. Pass 2 patched no high.

**Verification:** `just check` passes (762 tests, 7 skipped), with fixed-hosts and seam-guard in the gate. Each round's red run was logged before its `src/` change, and every matrix row has a named test that ran and passed. The login Keychain is untouched, and no server-port record was left behind.

**Residual risks:**
- **Hand-edited seat and MCP config:** a hand edit that widens seats' `allowed_hosts` or MCP URLs takes effect at the next open (deferred to epic 4 story 18).
- **NAT64:** a network-specific NAT64 prefix cannot be known in general.
- **Fixed-host scan:** a host buried inside a sentence is not found, and a whole literal `Agentic.app` would now count as a host.
- **Engine timeouts:** model and MCP calls rely on the engine's step timeout and cancel, which come with epic 3's later stories.
- **CI:** the merge waits on the `url` 2.5.8 advisory (RUSTSEC-2026-0592), for which the owner's decision is pending.
