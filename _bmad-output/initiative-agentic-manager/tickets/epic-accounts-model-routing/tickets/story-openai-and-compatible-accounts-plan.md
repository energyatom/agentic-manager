---
title: 'OpenAI and compatible Accounts'
type: feature
ticket: 3
created: '2026-11-22'
status: done
baseline_revision: '15649391b4b0ba8fb5e0998299c09601ca2efec2'
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
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** Only an Anthropic Account can be added and tested from the CLI. The OpenAI-chat-compatible adapter is the spike's: no OpenAI cloud path, one tool call proven, no test on a custom endpoint, and reasoning deltas unhandled. R1 needs OpenAI and OpenAI-compatible Accounts with the endpoint shown plainly and pinned.

**Approach:**
- Harden the one OpenAI-chat-compatible adapter behind `LlmProvider`: text, several tool calls, usage, cancel, reasoning deltas kept out of the answer.
- Extend `agentic accounts add|list|test` to the providers `openai` and `openai-compatible`.
- The add path is story 2.1's, unchanged: terminal only, the Account's name typed to confirm, the key read with echo off into the Keychain only, History and an `account_added` audit note. The only differences are the provider and the endpoint.

## Boundaries & Constraints

**Always:**
- **The 2.1 pattern, unchanged (AD-15 interim CLI Account edit, AD-17):**
  - Every check, prompt, refusal, re-check and record of `add` and `remove` stays as story 2.1 built it.
  - A key is always read and stored under the Account id, for every provider. There is no keyless Account in this story.
  - The key is never in a file, an argument, the environment, output, History, audit or a log.
- **Endpoints (AD-12, AD-48):** one function derives an Account's endpoint (scheme, host, port, base URL), used by `add`, `list` and `test`.
  - `anthropic`: `https://api.anthropic.com`, port 443.
  - `openai`: `https://api.openai.com/v1`, port 443. A `--base-url` is refused.
  - `openai-compatible`: `--base-url` is required and must be one of two forms:
    - `https://<host>[/path]` on port 443, where the host is a DNS name. An IP literal, a port other than 443, userinfo, a query or a fragment is refused in words.
    - `http://127.0.0.1:<port>[/path]` with an explicit port, for a server on this Mac. `localhost`, any other address and https to loopback are refused in words.
  - A private, link-local, LAN or metadata address is refused at add, and by am-net at call time when a name resolves to one.
- **Confirmation and record:**
  - The add's sentence names the Account, its provider and the endpoint its calls go to, as typed. For a loopback endpoint it says the calls stay on this Mac, on that port.
  - The `account_added` audit note carries the endpoint's scheme, host and port.
  - `models/accounts.yaml` gains `base_url` for a compatible Account, and `secret_ref: <id>` as before.
- **Test:**
  - `accounts test <id>` picks the adapter by provider.
  - Its egress policy admits only the Account's endpoint: the one host for https, or the one loopback port and no host for a loopback endpoint.
  - OpenAI and compatible Accounts authenticate with `Bearer` from the Keychain entry named by the Account id.
  - `--model` is required for `openai` and `openai-compatible`: no default model id is assumed. Anthropic keeps its default.
- **A key goes only to a confirmed provider and endpoint (AD-15, AD-16, AD-48; owner, 2026-11-22):** for every Account, `accounts test` reads no key and sends nothing unless both of these hold.
  - `models/accounts.yaml` holds exactly the bytes the app last wrote (no outside change).
  - The latest `account_added` audit note for that id names the same provider, scheme, host and port as the file.

  The note carries the provider. An Account with no such note (hand-written and never added) is refused. The check is read-only and takes no project lock, and `accounts test` needs no terminal. Each refusal says what to do.
- **Reused key:** when the Keychain already holds a key for the id, the add's sentence says so, and says that key will be sent to the endpoint, before the id is typed.
- **One rule for a compatible `base_url`:** the two accepted forms are checked by one function in am-config, used by the loader (an error with file, line and fix) and by the CLI.
- **List:** shows each Account's endpoint host (with the port for loopback), and the key masked as before.
- **Adapter** (`OpenAiCompatProvider`):
  - It streams text deltas.
  - It assembles two or more tool calls by index, or by id when a server omits the index, and never merges them.
  - It reports usage (`stream_options.include_usage`).
  - It returns `Cancelled` within 1 s of a cancel, with no tool call emitted.
  - It keeps `reasoning_content` deltas out of the answer text (spike finding 8).
  - It sends the request only to `{base_url}/chat/completions`.
- **Tests:** recorded fixtures under `fixtures/http/`, FakeEgress and FakeSecretStore, with a canary key. No network, no real key.

**Never:**
- No keyless Account, no `--no-key`, and no change to AD-15's add text (a keyless local endpoint is the owner's to decide; story 4's reserved local Accounts need no key).
- No change to `am-net`'s policy rules (story 2), `agentic run`'s providers (story 6), file parts or the Responses API (story 9), local discovery (story 4) or the loaded context window.
- No OpenAI or compatible host added to any seat's `allowed_hosts`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add OpenAI | `add oa --provider openai`; id typed; canary key | Sentence names `https://api.openai.com/v1`; file gains `{id: oa, provider: openai, secret_ref: oa}`; audit note `https`, `api.openai.com`, `443`; key only in the fake store | — |
| Add compatible, remote | `add router --provider openai-compatible --base-url https://openrouter.example/api/v1` | Sentence names the URL; file gains `base_url`; audit note host `openrouter.example`, port 443 | — |
| Add compatible, loopback | `--base-url http://127.0.0.1:8080/v1` | Sentence says calls stay on this Mac, port 8080; audit note `http`, `127.0.0.1`, `8080` | — |
| Refused URLs | No `--base-url`; `http://example.org`; `https://10.0.0.5/v1`; `http://192.168.1.4:8080`; `http://localhost:8080`; `http://127.0.0.1/v1` (no port); `https://x.org:8443`; userinfo; `--base-url` with `openai` or `anthropic` | Each refused in words before any prompt; nothing read, written or stored | — |
| Other providers | `--provider brave`; `--provider nope` | Refused naming the three providers; nothing read | — |
| List | One Account of each kind | Hosts `api.anthropic.com`, `api.openai.com`, `openrouter.example`, `127.0.0.1:8080`; keys masked | — |
| Test OpenAI | FakeEgress replays an OpenAI recording; `--model m` | Streamed text printed; request to `https://api.openai.com/v1/chat/completions`, `AuthRef` Bearer naming the Account; policy admits only that host | Without `--model`: refused in words |
| Test compatible | Remote and loopback Accounts | Request to `{base_url}/chat/completions`; the loopback policy admits only that port and no host; `check_url` refuses any other host or port | — |
| Two tool calls | Adapter on OpenAI and on compatible recordings with two calls and usage | Two `ToolCall` events with their own ids, names and arguments; usage reported | A recording without `index` still gives two calls |
| Cancel | Adapter on a hanging stream | `Cancelled` within 1 s; no tool call | — |
| Reasoning | Recording with `reasoning_content` deltas, then content | The answer text holds only the content | — |
| 2.1 unchanged | Anthropic add, remove, list, test; non-terminal stdin | Every story 2.1 test still passes unchanged, except the one that refused `openai` | — |

</intent-contract>

## Code Map

- `crates/am-providers/src/openai_compat.rs`:
  - `OpenAiCompatProvider` (24): `new(egress, account_id, base_url)` (34), `with_auth(AuthRef)` (58), `request_body` (69).
  - Text from `delta.content` (221).
  - Tool calls kept by `index` in a `BTreeMap` (227-240), with `unwrap_or(0)` at 229 (the merge risk).
  - Usage (211, 164).
  - POST `{base_url}/chat/completions` (290).
  - `common.rs`: `send_cancellable` (45), `drive_sse` (76).
  - No tests directory yet: add `crates/am-providers/tests/` with am-testkit as a dev-dependency.
- **Fixtures:**
  - Existing: `fixtures/http/openai_compat/c1_tool_use.sse` (one call).
  - New recordings in the provider's SSE format:
    - `openai/two_tools.sse`, `openai/text.sse`;
    - `openai_compat/two_tools.sse`, `openai_compat/two_tools_no_index.sse`, `openai_compat/reasoning.sse`.
- `crates/am-app/src/accounts.rs`:
  - `ANTHROPIC` (86), `USAGE` (88), `parse` (121; flags at 132-136, `--name` at 135), `Command::Add` (106).
  - `add` (417): the provider refusal at 426-428, the endpoint at 450, the sentence at 459-461, the audit note at 567-588 (scheme 575, host 576, port 577), the calls at 593 and 597, the success line at 640.
  - `host_of` (742), `list` (776; the empty hint at 781).
  - `test` (827): the refusal at 845-848, the policy at 870-874, the adapter at 876, the result line at 924.
  - `crates/am-app/src/lib.rs:40` — `DEFAULT_ANTHROPIC_MODEL`.
- `crates/am-config/src/account_edit.rs`:
  - `account_item(id, provider, name)` (203), `add_account_edit` (216) and `new_accounts_file` (257) gain an optional `base_url`.
  - Their tests at `crates/am-config/tests/account_edit.rs:65` and `:87`.
  - Loader: `kinds.rs:433` `accounts()`, `web_url` (279-302), `NEEDS_BASE_URL` (`am-domain/src/capability.rs:141`).
- `crates/am-net/src/lib.rs`:
  - `EgressPolicy { allowed_hosts, loopback_ports, .. }`.
  - `check_url` (83): loopback is admitted by port (111-118), remote is https with an exact host (119-124), and `is_blocked_ip` (143-172). Use as it is; do not change it.
  - `AuthScheme::Bearer` (`am-domain/src/net.rs:15`).
- **Tests:**
  - `crates/am-app/tests/accounts_cli.rs`:
    - the `Kit` at 211-284;
    - the pattern at 742 (`test_streams_a_reply_through_the_accounts_pinned_host`);
    - `add_refuses_a_provider_other_than_anthropic` (585), to rewrite as "a provider that is not a model provider is refused";
    - `list_masks_keys_and_names_missing_ones` (702).
  - `crates/am-e2e/tests/spike/c1_providers.rs:115` — stays as it is.

## Tasks & Acceptance

**Execution:**
- [x] Tests first (no `src/` change): every matrix row, the new recordings, and the rewritten older tests. Log the red run.
- [x] `crates/am-providers/src/openai_compat.rs` — tool calls by index or id, reasoning deltas.
- [x] `crates/am-config/src/account_edit.rs` — `base_url` in the edit.
- [x] `crates/am-app/src/accounts.rs` — the endpoint function, add, list and test for the three providers.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the outcome, wording and side effects match.
- Given the workspace, when `just check` runs, then it is green.

## Implementation Notes

### Red run (2026-11-22, before any `src/` change)

Tests written first, on `story/2-3` at baseline `1564939`; no file under a `src/` folder was created or changed (`git status` shows only test files, fixtures, `crates/am-providers/Cargo.toml` and `Cargo.lock`).

**Test files:**
- `crates/am-providers/tests/openai_compat.rs` (new; `am-testkit` added as a dev-dependency of am-providers): the adapter rows (text, two tool calls on OpenAI and compatible recordings, no `index`, cancel, reasoning).
- `crates/am-app/tests/accounts_cli.rs`: the story 2.3 section (add OpenAI, add compatible remote and loopback, refused URLs, terminal rule, usage, list, test OpenAI, test compatible, `--model` required, no key, hand-written `base_url`), and the rewritten 2.1 test `add_refuses_a_provider_other_than_anthropic` → `add_refuses_a_provider_that_is_not_a_model_provider`.
- `crates/am-config/tests/account_edit.rs`: the two older calls rewritten to the new signature (`add_account_edit(file, id, provider, name, base_url)`, `new_accounts_file(id, provider, name, base_url)`), and four new tests for `base_url` in the edit, the set-whole-list edit, the new file and the writer's `create`.
- Recordings (written by hand in the provider's stream format, each starting with an SSE comment that says so): `fixtures/http/openai/text.sse`, `openai/two_tools.sse`, `openai_compat/two_tools.sse`, `openai_compat/two_tools_no_index.sse`, `openai_compat/reasoning.sse`.

**Command 1** (the plan's verification command): `cargo nextest run -p am-providers -p am-config -p am-app -p am-e2e` — exit 101, a compile error that names the missing API:

```
error[E0061]: this function takes 4 arguments but 5 arguments were supplied
   --> crates/am-config/tests/account_edit.rs:72:15
    |
 72 |     let add = add_account_edit(&seen, "cloud2", "anthropic", None, None);
    |               ^^^^^^^^^^^^^^^^                                     ---- unexpected argument #5 of type `Option<_>`
note: function defined here
   --> crates/am-config/src/account_edit.rs:216:8
error[E0061]: this function takes 3 arguments but 4 arguments were supplied
   --> crates/am-config/tests/account_edit.rs:258:21
    |
258 |         let bytes = new_accounts_file(id, provider, name, base_url).unwrap();
    |                     ^^^^^^^^^^^^^^^^^                     -------- unexpected argument #4 of type `Option<&str>`
note: function defined here
   --> crates/am-config/src/account_edit.rs:257:8
error: could not compile `am-config` (test "account_edit") due to 6 previous errors
```

**Command 2** (the crates whose tests compile): `cargo nextest run -p am-providers -p am-app -p am-e2e --no-fail-fast` — exit 100:

```
     Summary [  10.392s] 203 tests run: 189 passed, 14 failed, 5 skipped
        FAIL am-app::accounts_cli add_of_an_openai_or_compatible_account_needs_a_terminal
        FAIL am-app::accounts_cli add_a_compatible_account_on_this_mac_says_its_calls_stay_here
        FAIL am-app::accounts_cli add_a_compatible_account_at_a_remote_url_pins_that_host
        FAIL am-app::accounts_cli add_refuses_a_base_url_outside_the_two_accepted_forms
        FAIL am-app::accounts_cli add_an_openai_account_names_its_endpoint_and_records_it
        FAIL am-app::accounts_cli add_refuses_a_provider_that_is_not_a_model_provider
        FAIL am-app::accounts_cli list_shows_the_endpoint_host_of_each_kind_of_account
        FAIL am-app::accounts_cli test_of_a_compatible_account_calls_only_its_base_url
        FAIL am-app::accounts_cli test_of_an_openai_or_compatible_account_needs_a_model
        FAIL am-app::accounts_cli test_of_a_compatible_account_without_a_key_sends_nothing
        FAIL am-app::accounts_cli test_of_an_openai_account_streams_through_its_pinned_host
        FAIL am-app::accounts_cli usage_names_the_three_providers_and_the_base_url
        FAIL am-providers::openai_compat tool_calls_without_an_index_are_kept_apart_by_id
        FAIL am-providers::openai_compat cancel_on_a_hanging_stream_returns_within_1s_with_no_tool_call
error: test run failed
```

First failure lines:

```
add_an_openai_account_names_its_endpoint_and_records_it        accounts_cli.rs:1687  (exit code 1, expected 0)
    agentic: only an Anthropic Account can be added from the command line in this version (`--provider anthropic`), not 'openai'. No Account was added and no key was read or stored
add_a_compatible_account_at_a_remote_url_pins_that_host        accounts_cli.rs:1783  (exit code 2, expected 0)
add_a_compatible_account_on_this_mac_says_its_calls_stay_here  accounts_cli.rs:1870  (exit code 2, expected 0)
add_of_an_openai_or_compatible_account_needs_a_terminal        accounts_cli.rs:2054  (the compatible add: usage error, the terminal rule is not named)
    usage: agentic accounts add <id> --provider anthropic [--name <text>] [--project <folder>] … (`--base-url` is an unknown option)
add_refuses_a_base_url_outside_the_two_accepted_forms          accounts_cli.rs:2004
    openai-compatible None: the refusal names --base-url:
    agentic: only an Anthropic Account can be added from the command line in this version (`--provider anthropic`), not 'openai-compatible'. …
add_refuses_a_provider_that_is_not_a_model_provider            accounts_cli.rs:616
    brave: the refusal names openai:
    agentic: only an Anthropic Account can be added from the command line in this version (`--provider anthropic`), not 'brave'. …
usage_names_the_three_providers_and_the_base_url               accounts_cli.rs:2073
    usage names openai: usage: agentic accounts add <id> --provider anthropic [--name <text>] [--project <folder>]
list_shows_the_endpoint_host_of_each_kind_of_account           accounts_cli.rs:2142
    oa: its endpoint host api.openai.com: ["oa", "openai", "(its", "provider's", "host)", "••••••••OAOA"]
test_of_an_openai_account_streams_through_its_pinned_host      accounts_cli.rs:2182  (exit code 1, expected 0)
    agentic: the Account 'oa' is not a plain Anthropic Account, the only kind this version can test; nothing was sent
test_of_a_compatible_account_calls_only_its_base_url           accounts_cli.rs:2295  (exit code 1, expected 0)
    agentic: the Account 'router' is not a plain Anthropic Account, the only kind this version can test; nothing was sent
test_of_an_openai_or_compatible_account_needs_a_model          accounts_cli.rs:2266
    oa: the refusal names --model:
    agentic: the Account 'oa' is not a plain Anthropic Account, the only kind this version can test; nothing was sent
test_of_a_compatible_account_without_a_key_sends_nothing       accounts_cli.rs:2399  (no "no key in Keychain")
    agentic: the Account 'oa' is not a plain Anthropic Account, the only kind this version can test; nothing was sent
tool_calls_without_an_index_are_kept_apart_by_id               openai_compat.rs:267
    assertion `left == right` failed: two calls, not one merged call
      left: [ToolCall { id: "call_fixture_n2", name: "lookup_wordcount_letters", arguments_json: "{\"word\": \"wren\"}{\"text\": \"wren\"}" }]
     right: [ToolCall { id: "call_fixture_n1", name: "lookup_word", … }, ToolCall { id: "call_fixture_n2", name: "count_letters", … }]
cancel_on_a_hanging_stream_returns_within_1s_with_no_tool_call openai_compat.rs:351
    https://api.openai.com/v1: no tool call is returned: [ToolCall { id: "call_fixture_c1", … }, ToolCall { id: "call_fixture_c2", … }]
```

**Passing already, kept as guards** (the spike's adapter and story 2.1 already behave so): `openai_text_streams_as_deltas_with_usage`, `openai_streams_two_tool_calls_with_usage`, `compatible_streams_two_tool_calls_with_usage` (calls with an `index`), `reasoning_deltas_stay_out_of_the_answer_text` (only `delta.content` is read), `test_never_follows_a_hand_written_base_url_outside_the_endpoint_rules` (refused today as "not a plain Anthropic Account"). Every other story 2.1 test passes unchanged, and `crates/am-e2e/tests/spike/c1_providers.rs` is untouched and passes.

**Choices made in the tests** (owner-standing-rules: test structure and wording are decided here):
- `base_url` is the last argument of `add_account_edit` and `new_accounts_file` (`Option<&str>`).
- Cancel: "no tool call" is read as none emitted to the sink and none returned in the outcome; today the outcome of a cancelled stream still returns the calls streamed so far, which is what makes that test red.
- A recording without `index` sends each call whole in one delta with its own id (how servers that omit the index send them).
- Wording the tests pin: the loopback sentence holds "stay on this Mac" and "port 8080"; each refused `--base-url` names `--base-url` and one reason word from a short list per case (for example "https", "address", "port", "query", "fragment"); a refused provider names `anthropic`, `openai` and `openai-compatible`; a `test` without `--model` names `--model`; a missing key keeps 2.1's "no key in Keychain".
- Beyond the matrix's own list, the refused-URL test also covers what the Boundaries name: a query, a fragment, https to loopback, `[::1]`, a metadata address and a public IP literal.
- The am-config tests were also run against a throwaway stub of the new signature in a scratch worktree outside the repository (since removed) to check they can pass: 6 of 6 passed there.


### Green run (2026-11-22, after the `src/` changes)

No test of the red run was changed, skipped or removed; the test files, recordings and `crates/am-providers/Cargo.toml` are as the red run left them.

- `cargo nextest run -p am-providers -p am-config -p am-app -p am-e2e` — 464 tests run: 464 passed, 5 skipped.
- `just check` — exit 0 (fmt, clippy `-D warnings`, nextest 668 passed and 7 skipped, doctest, check-deps, check-forbid, denylist, runtime-paths, validate).

**What changed in `src/`:**
- `crates/am-providers/src/openai_compat.rs`: tool calls are kept in a list in the order they first appear and found by `index`, or by `id` when a fragment has no index (a fragment with neither continues the call before it); a cancelled stream gives no tool call to the sink or in the outcome; `OPENAI_HOST` and `OPENAI_BASE_URL` are named here, beside the adapter (as `ANTHROPIC_HOST` is beside its adapter). Only `delta.content` is read, as before, so reasoning deltas need no code.
- `crates/am-config/src/account_edit.rs`: `base_url: Option<&str>` as the last argument of `add_account_edit` and `new_accounts_file`, written between `name` and `secret_ref`.
- `crates/am-app/src/accounts.rs`: `endpoint_of(provider, base_url, flag)` is the one function `add`, `list` and `test` derive the endpoint with; `add` takes `--base-url`; `test` picks the adapter by provider, builds its policy from the endpoint and requires `--model` for `openai` and `openai-compatible`. `crates/am-app/src/main.rs`: the doc line of the command only.

**Choices made in the code** (owner-standing-rules: wording and internal layout are decided here):
- The base URL is read by a strict check written here (am-app has no URL crate, and none was added), and then `am_net::check_url` must read `{base_url}/chat/completions` as the same host, port and loopback flag under the endpoint's own policy. So nothing is accepted at add that the egress client would read as another endpoint. `am-net` is used as it is.
- A host is a DNS name only when its last label starts with a letter, so no spelling of an IPv4 address (decimal, hexadecimal, short) passes as a name. `localhost` and `*.localhost` are refused over https as this Mac. An explicit `:443` is accepted; the scheme must be written in lowercase.
- A refusal repeats the URL only when it is plain ASCII with no `@`, `?`, `#`, backslash or backtick, so a key someone put in a URL, or a control character from a hand-written file, is never printed.
- A trailing `/` on the base URL is kept in the file and the sentence (as typed) and dropped only where the adapter appends `/chat/completions`.
- A missing `--base-url` for `openai-compatible` is a refusal (exit 1) after the terminal rule, not bad usage; a `--base-url` with no value is bad usage (exit 2).
- `test` checks in this order: the Account exists, its provider is a model provider, its endpoint, `--model`, `secret_ref`, the key. A model Account whose hand-written `base_url` is outside the rules is refused by `test` and shown by `list` as "(base_url not accepted; …)" with no host. Story 2.1 showed the host of any `base_url` in `list`; that stays only for Accounts that are not model Accounts.
- A tool-call fragment that brings another id on an index that already has one starts a new call (a server that numbers every call 0); calls that all carry an index are returned in index order, as before.
- Unit tests added beside the code for these choices (they were not part of the red run): `accounts::tests` (4) and `openai_compat::tests` (3).

**Left as it is, for the owner or a later story:**
- The adapter still sends `max_tokens`, as the spike's did. Newer OpenAI cloud models are reported to require `max_completion_tokens` and may refuse the request; there is no OpenAI key to check this against the live API, and the plan does not ask for a change to the request body.
- `accounts test` asks for 64 output tokens; a reasoning model may spend them on thinking and answer with no text.
- `am-net` admits any loopback address and `localhost` on an allowed loopback port, over either scheme. The add accepts only `http://127.0.0.1:<port>`, and `am-net`'s rule is story 2's.

### Red run, review pass 1 (2026-11-22, before the `src/` changes of the review fixes)

Tests written or rewritten first for the five review findings and the owner's addition (the confirmed check covers every Account and compares the provider); no file under a `src/` folder was changed between the green run above and this run.

**Test files and recordings:**
- `crates/am-providers/tests/openai_compat.rs`: `a_call_whose_fragments_each_carry_a_fresh_id_is_one_call`, `two_whole_calls_numbered_the_same_index_are_two_calls`; recordings `fixtures/http/openai_compat/one_tool_fresh_ids.sse` and `two_tools_same_index.sse` (written by hand, each saying so in its first line).
- `crates/am-config/tests/validate.rs`: `a_compatible_base_url_takes_only_the_two_accepted_forms`, `a_compatible_account_without_a_base_url_is_told_the_two_forms`; the story 1.4 test `plain_http_is_only_for_this_mac_and_never_with_userinfo` now uses the provider `firecrawl`, because it pins the general rule that `openai-compatible` no longer follows.
- `crates/am-config/tests/account_edit.rs`: `the_read_names_the_errors_of_each_entry` (`AccountsFile::entry_errors`).
- `crates/am-store/tests/config_history.rs`: `a_reader_gives_the_args_of_the_audit_entries_of_one_action_and_target` (`Reader::audit_args`).
- `crates/am-app/tests/accounts_cli.rs`:
  - rewritten to add the Account through the command first: `test_streams_a_reply_through_the_accounts_pinned_host` and `test_without_a_key_or_of_an_unknown_id_sends_nothing` (story 2.1), `test_of_an_openai_account_streams_through_its_pinned_host`, `test_of_an_openai_or_compatible_account_needs_a_model`, `test_of_a_compatible_account_calls_only_its_base_url`, `test_of_a_compatible_account_without_a_key_sends_nothing`;
  - new: `test_refuses_a_base_url_edited_by_hand_after_the_add`, `test_refuses_a_provider_edited_by_hand_after_the_add`, `test_refuses_an_account_that_was_never_added`, `test_refuses_an_account_removed_and_put_back_by_hand`, `test_refuses_what_the_latest_add_of_the_account_did_not_record`, `add_says_before_the_id_is_typed_that_a_held_key_will_go_to_the_endpoint`, `test_and_list_of_an_entry_with_an_error_point_at_validate`.

**Command 1:** `cargo nextest run -p am-providers --test openai_compat -p am-config --test validate -p am-app --test accounts_cli --no-fail-fast` — exit 100:

```
     Summary [   3.959s] 87 tests run: 77 passed, 10 failed, 0 skipped
        FAIL am-providers::openai_compat a_call_whose_fragments_each_carry_a_fresh_id_is_one_call
        FAIL am-config::validate a_compatible_base_url_takes_only_the_two_accepted_forms
        FAIL am-config::validate a_compatible_account_without_a_base_url_is_told_the_two_forms
        FAIL am-app::accounts_cli test_and_list_of_an_entry_with_an_error_point_at_validate
        FAIL am-app::accounts_cli test_refuses_an_account_that_was_never_added
        FAIL am-app::accounts_cli add_says_before_the_id_is_typed_that_a_held_key_will_go_to_the_endpoint
        FAIL am-app::accounts_cli test_refuses_a_provider_edited_by_hand_after_the_add
        FAIL am-app::accounts_cli test_refuses_what_the_latest_add_of_the_account_did_not_record
        FAIL am-app::accounts_cli test_refuses_a_base_url_edited_by_hand_after_the_add
        FAIL am-app::accounts_cli test_refuses_an_account_removed_and_put_back_by_hand
```

First failure lines:

```
a_call_whose_fragments_each_carry_a_fresh_id_is_one_call      openai_compat.rs:297
    one call, not one per fragment
      left: [ToolCall { id: "call_fixture_f1", name: "lookup_word", arguments_json: "{}" }, ToolCall { id: "call_fixture_f2", name: "", arguments_json: "{\"word\":" }, ToolCall { id: "call_fixture_f3", … }, ToolCall { id: "call_fixture_f4", … }]
     right: [ToolCall { id: "call_fixture_f1", name: "lookup_word", arguments_json: "{\"word\": \"finch\"}" }]
a_compatible_base_url_takes_only_the_two_accepted_forms       validate.rs:50
    no finding models/accounts.yaml:9: base_url   (`http://localhost:1234/v1` is accepted today)
a_compatible_account_without_a_base_url_is_told_the_two_forms validate.rs:717
    fix: "add `base_url: https://…` (or http://127.0.0.1:<port> or http://localhost:<port> for a server on this Mac)"
test_refuses_a_base_url_edited_by_hand_after_the_add          accounts_cli.rs:1680  (exit code 0, expected a refusal)
    agentic: the Account 'router' answered from https://elsewhere.example/api/v1 (model m, 18 input and 31 output tokens)
test_refuses_a_provider_edited_by_hand_after_the_add          accounts_cli.rs:1680  (exit code 0)
    agentic: the Account 'cloud' answered from https://api.openai.com/v1 (model m, 18 input and 31 output tokens)
test_refuses_an_account_that_was_never_added                  accounts_cli.rs:1680  (exit code 0)
    agentic: the Account 'cloud' answered from https://api.anthropic.com (model m, no usage reported)
test_refuses_an_account_removed_and_put_back_by_hand          accounts_cli.rs:1680  (exit code 0)
test_refuses_what_the_latest_add_of_the_account_did_not_record accounts_cli.rs:1680  (exit code 0)
    agentic: the Account 'router' answered from https://openrouter.example/api/v1 (model m, 18 input and 31 output tokens)
add_says_before_the_id_is_typed_that_a_held_key_will_go_to_the_endpoint  accounts_cli.rs:2881
    router: before the id is typed, the sentence says "already holds a key for 'router'":
    Adding the Account 'router' (provider openai-compatible): its calls go to https://openrouter.example/api/v1, with a key kept only in this Mac's Keychain. Nothing in the project names it yet.
test_and_list_of_an_entry_with_an_error_point_at_validate     accounts_cli.rs:1680  (exit code 0)
    agentic: the Account 'oa' answered from https://api.openai.com/v1 (model m, 18 input and 31 output tokens)
```

**Command 2:** `cargo nextest run -p am-config --test account_edit` — exit 101, a compile error that names the missing API:

```
error[E0599]: no method named `entry_errors` found for struct `AccountsFile` in the current scope
   --> crates/am-config/tests/account_edit.rs:342:19
error: could not compile `am-config` (test "account_edit") due to 4 previous errors
```

**Command 3:** `cargo nextest run -p am-store --test config_history` — exit 101, a compile error that names the missing API:

```
error[E0599]: no method named `audit_args` found for struct `Reader` in the current scope
   --> crates/am-store/tests/config_history.rs:794:16
error: could not compile `am-store` (test "config_history") due to 3 previous errors
```

**Passing already, kept as guards:** the six rewritten `test` tests (an Account added through the command answers today too), `two_whole_calls_numbered_the_same_index_are_two_calls` (the green run's adapter already starts a new call on another id) and `plain_http_is_only_for_this_mac_and_never_with_userinfo` on `firecrawl`.

**Choices made in the tests:**
- The provider is already in the `account_added` note (`provider`, since story 2.1), so no test changes for it; the confirmed check reads it.
- The refusal of an unconfirmed Account is pinned by three phrases: "not confirmed by `agentic accounts add` on this Mac", "No key was read or sent" and `agentic accounts remove`.
- "Refused before anything is read or sent" is checked as: exit not 0, zero `SecretStore::get` calls, no request and no egress policy asked for.
- Rule (ii) is shown apart from rule (i) by restoring an older History entry through the writer: the file is then as the app wrote it, and only the latest note differs (another host; then the provider alone, with the same scheme, host and port).
- `a_secret_ref_that_names_another_entry_is_never_read_or_sent` and `test_never_follows_a_hand_written_base_url_outside_the_endpoint_rules` write Accounts by hand and expect a refusal; they stay as they are.
- A valid "plain" project (one seat on the reserved local Accounts) is used where several Accounts are added: the usual project refuses an add of any id but `cloud2`, because creating the file brings its seats' unknown `cloud2` into view as a new error.

### Green run, review pass 1 (2026-11-22, after the `src/` changes of the review fixes)

Only the tests that cover the edited files were run, as the review asked; the full gate (`just check`) was not run here.

- `cargo nextest run -p am-app` — 155 tests run: 155 passed.
- `cargo nextest run -p am-providers -p am-config -p am-store` — 343 tests run: 343 passed.
- `cargo clippy -p am-app -p am-config -p am-store -p am-providers --all-targets -- -D warnings` and `cargo fmt --all -- --check` — clean; `cargo xtask denylist`, `runtime-paths`, `check-forbid` and `check-deps` — clean.

**What changed in `src/`:**
- **A key goes only to what was confirmed** (`crates/am-app/src/accounts.rs`, `confirmed`): `accounts test` checks every Account, before any Keychain read, request or egress client. It finds the runtime folder as `status` does, opens a read-only `Reader` (no lock, no migration), and requires that the sha256 of the `models/accounts.yaml` it just read is the `after_sha256` of the file's last History entry, and that the latest `account_added` note of the id names the same provider, scheme, host and port. It still runs from any stdin. `crates/am-store/src/audit.rs` gains `Reader::audit_args(action, target)` for this read.
- **The provider in the note:** `add` has written `provider` in the `account_added` note since story 2.1, so nothing changed there.
- **Reused key:** `add` reads whether the Keychain holds an entry for the id before the sentence (only that it is there) and, when it does, the sentence says so and that the key will be sent to the endpoint. With no entry the sentence is byte for byte what it was. The later reuse and never-overwrite steps are unchanged.
- **Tool calls at an occupied index** (`crates/am-providers/src/openai_compat.rs`): another call starts only when the fragment brings both another id and a function name; otherwise it continues the call, which keeps its first id. This replaces the green run's choice above.
- **One rule for a compatible `base_url`:** the strict check moved from am-app into `crates/am-config/src/base_url.rs` as `compatible_base_url` (with `CompatibleBaseUrl` and `COMPATIBLE_BASE_URL_FORMS`). It now also requires that the `url` crate reads the same scheme, host and port. The loader uses it for `openai-compatible` Accounts only (an error at the `base_url` line, the two forms in the fix, the URL never repeated); the CLI uses it and keeps its `am_net::check_url` cross-check. Other providers keep `web_url`. `docs/config-schema.md` says so.
- **An entry with an error:** `AccountsFile::entry_errors(id)` (am-config) gives the loader's errors on that entry, by its lines. `test` refuses such an Account before anything else, pointing at `agentic validate`; `list` shows it with no host.

**Order of `test`'s checks now:** the Account exists, its entry has no error, its provider is a model provider, its endpoint, `secret_ref`, confirmed, `--model`, the key.

**Fixtures and templates:** none names an `openai-compatible` Account, so none changed. The story 1.4 test `plain_http_is_only_for_this_mac_and_never_with_userinfo` moved to `firecrawl` (see the red run).

**Left as it is, for the owner or a later story:**
- A Keychain entry that appears between the sentence and the typed id is still reused without the sentence having said so (the later steps were to stay unchanged).
- Any hand edit of `models/accounts.yaml` (another Account's line, a name, a `secret_ref`) stops `test` for every Account in it until the file is put back, because the CLI has no Reload. The `secret_ref` refusal still advises an edit of the file, which then needs that.
- Once the app's Reload records an outside change in History, rule 1 holds again and rule 2 alone protects: it compares provider, scheme, host and port, not the path of the base URL.
- A project whose file already holds an `openai-compatible` Account with `http://localhost:<port>`, an IP literal or another https port now fails `agentic validate` on that line.
- `Reader::audit_args` reads the whole audit log to find the entries; fine for one CLI command, not for a hot path.
- The `max_tokens` and 64-token notes of the first green run still stand.

### Red run, review pass 2 (2026-11-22, before the `src/` changes of the second review's fixes)

Tests written or rewritten first; no file under a `src/` folder was changed between the green run of review pass 1 and this run.

**Test files and recording:**
- `crates/am-providers/tests/openai_compat.rs`: `two_whole_calls_at_the_same_index_without_ids_are_two_calls`; recording `fixtures/http/openai_compat/two_tools_same_index_no_ids.sse` (written by hand, saying so in its first line).
- `crates/am-app/tests/accounts_cli.rs`:
  - rewritten: story 2.1's `a_keychain_entry_that_appears_during_the_key_prompt_is_kept` is now `a_keychain_entry_that_changes_while_a_prompt_waits_refuses_the_add` (an entry that appears during the id prompt, one that appears during the key prompt, and a held entry deleted during the id prompt), because the contract's "Reused key" rule now refuses what that test expected to be kept and used;
  - rewritten: `test_never_follows_a_hand_written_base_url_outside_the_endpoint_rules` adds each Account through the command and then edits its `base_url` by hand, and checks the `base_url` reason with zero Keychain reads, no egress policy and no request;
  - tightened: `test_of_an_openai_or_compatible_account_needs_a_model` counts Keychain reads and expects none;
  - new: `test_answers_while_another_process_holds_the_store`.

**Command:** `cargo nextest run -p am-providers --test openai_compat -p am-app --test accounts_cli --no-fail-fast` — exit 100:

```
     Summary [   3.445s] 58 tests run: 56 passed, 2 failed, 0 skipped
        FAIL am-providers::openai_compat two_whole_calls_at_the_same_index_without_ids_are_two_calls
        FAIL am-app::accounts_cli a_keychain_entry_that_changes_while_a_prompt_waits_refuses_the_add
```

First failure lines:

```
two_whole_calls_at_the_same_index_without_ids_are_two_calls        openai_compat.rs:354
    two calls, not one merged call
      left: [ToolCall { id: "", name: "lookup_wordcount_letters", arguments_json: "{\"word\": \"stork\"}{\"text\": \"stork\"}" }]
     right: [ToolCall { id: "", name: "lookup_word", … }, ToolCall { id: "", name: "count_letters", … }]
a_keychain_entry_that_changes_while_a_prompt_waits_refuses_the_add accounts_cli.rs:1404  (exit code 0, expected a refusal)
    during the id prompt: agentic: added the Account 'cloud2' (anthropic, https://api.anthropic.com) to models/accounts.yaml (History entry …); it reuses the key the Keychain already held
    Adding the Account 'cloud2' (provider anthropic): its calls go to https://api.anthropic.com, with a key kept only in this Mac's Keychain. Already naming it, …
```

**Passing already, kept as guards** (they prove what the code already does): the rewritten `test_never_follows_a_hand_written_base_url_outside_the_endpoint_rules`, the tightened `test_of_an_openai_or_compatible_account_needs_a_model`, and `test_answers_while_another_process_holds_the_store`.

**Choices made in the tests:**
- The refusal is pinned by "the Keychain entry for '<id>' changed while you typed" and "run the command again".
- "Changed" also covers a held entry that is gone after the id prompt (the sentence said it would be reused).
- Two whole calls without ids are expected with empty ids, as the server sent them.

### Green run, review pass 2 (2026-11-22, after the `src/` changes of the second review's fixes)

Only the tests that cover the edited files were run; the full gate (`just check`) was not run here.

- `cargo nextest run -p am-providers -p am-app` — 172 tests run: 172 passed.
- `cargo clippy -p am-app -p am-providers --all-targets -- -D warnings` and `cargo fmt --all -- --check` — clean.

**What changed in `src/`:**
- `crates/am-app/src/accounts.rs` (`add`): the Keychain is read again once the id is typed and compared with what the sentence said (held or not held); a difference refuses the add before any key is asked for. Right before `set`, an entry that appeared during the key prompt now refuses the add too (story 2.1 kept and used it): the typed key is discarded, and the entry is neither overwritten, deleted nor used. Both refusals say "the Keychain entry for '<id>' changed while you typed; run the command again".
- `crates/am-providers/src/openai_compat.rs` (`call_for`): at an index that already has a call, a fragment starts another call when it brings a function name and that call already has one, whatever the ids say; a fragment without a name continues the call. This replaces review pass 1's rule (another id and a name). The same rule is applied to a fragment with neither an index nor an id, which would otherwise merge into the call before it in the same way.

**Left as it is, for the owner or a later story:**
- The comparison is of whether an entry is there, not of its value: an entry replaced by another key while a prompt waits is not noticed (the value is never read into this command to compare).
- On the reuse path the Keychain is not read again right before the write; `set` is the only step that had a re-read, and it was the one named.
- A server that splits a function's name over several fragments, or repeats the whole name on every fragment, now gives one call per named fragment (before, the names were run together). No recording of such a server exists; OpenAI sends the name once, whole.

## Plan Change Log

- 2026-11-22 (owner decision during review): "the check must compare the provider too. If accounts.yaml's provider for an Account differs from the provider in its latest account_added audit entry, test refuses and sends nothing. No key ever goes to a provider I didn't confirm."
  - The confirmed check now covers every Account and the provider, and the audit note carries the provider.
  - `accounts test` keeps needing no terminal (owner: "test is safe from any stdin, and Doctor will need it later").
  - Story 2.1's `test` tests add their Account through the command first.
  - Keyless loopback Accounts and the OpenAI output-limit field are follow-up stories by the same decision, not built here.

- 2026-11-22 (review pass 1): the contract gained "A key goes only to a confirmed endpoint", "Reused key" and "One rule for a compatible `base_url`".
  - Story 2.1's `accounts test` could only ever reach one fixed host. With an owner-chosen endpoint, reading it from the file let a hand edit send a Keychain key to a host the owner never confirmed, which AD-15, AD-16 and AD-48 forbid. This is a correction to binding ADs.
  - The tests that wrote a compatible Account straight into the file are rewritten to add it through the command first.
  - KEEP everything else.

## Review Triage Log

### 2026-11-22 — Review pass
- verdicts: 7 findings — high 1, medium 3, low 3, false 0, maybe-false 0
- findings:
  - `[high]` `[bad_plan → corrected in place]` `accounts test` derives the endpoint from whatever the file holds and runs from any stdin, so a hand edit (or any process that can write the project) can make it send an Account's Keychain key to any DNS name. The plan did not carry AD-15, AD-16 and AD-48 into `test`. Fix: the contract rule "A key goes only to a confirmed endpoint"; no revert of the rest (the change is additive and the code is kept).
  - `[medium]` `[patch]` `add` reuses an existing Keychain entry for an endpoint the owner types, and the sentence does not say that key will go there. Fix: the sentence says so before the id is typed.
  - `[low]` `[patch]` A fragment with a new id on an index that already has one starts a new call, which splits one call on servers that stamp an id on every fragment (the plan says by index). Fix: a new call starts only when the fragment brings both a new id and a function name; otherwise it continues the call at that index. Recordings for both shapes.
  - `[medium]` `[patch]` The loader accepts `http://localhost:<port>`, https to an IP literal and any https port for a compatible Account, and its fix text suggests `localhost`, while the CLI refuses them. Fix: one rule in am-config, used by both.
  - `[medium]` `[patch]` A `base_url` the loader rejects reaches the CLI as `None`, so the refusal says the Account "needs `base_url`" when it has one, and an `openai` Account with a rejected `base_url` is called at the provider's host. Fix: `list` and `test` refuse an Account whose entry has a load error, pointing at `agentic validate`.
  - `[low]` `[reject]` "Never the app's own port" is deferred to story 2 only in this plan. The story 2 plan is drafted with it (its design note 5) and goes to the owner for approval before anything is built; recorded under residual risks here.
  - `[low]` `[reject]` Under a loopback endpoint's policy, `am-net` also admits `localhost`, `[::1]`, `127.0.0.2` and https on that port, and a remote policy pins the host only. The adapter calls only `{base_url}/chat/completions`; tightening `am-net` to scheme, host and port is story 2's, by this story's Never list. The matrix row's wording is looser than the code allows; recorded.

### 2026-11-22 — Follow-up review pass (the review fixes and the owner's provider rule)
- verdicts: 4 findings — high 0, medium 0, low 4, false 0, maybe-false 0. The reviewer found no path where a key is read or a request sent for an unconfirmed provider or endpoint, or a changed file.
- findings:
  - `[low]` `[patch]` The held-key state is read once before the sentence; an entry that appears during the id or key prompt is reused although the sentence said none was held. Fix: the add compares the Keychain state again after the id and before `set`, and refuses if it changed.
  - `[low]` `[patch]` Two whole calls at one index without ids are merged. Fix: a fragment bringing a name while the call at that index already has one starts a new call.
  - `[low]` `[patch]` `test_never_follows_a_hand_written_base_url_outside_the_endpoint_rules` now passes vacuously (the confirmed check refuses first), and the `--model` test does not prove "no key was read". Fix: add, then hand-edit, with exit and reason asserted; the counting store in the `--model` test.
  - `[low]` `[patch]` Nothing proves `accounts test` works while another process holds the store. Fix: a test that keeps the store open and expects the reply.

## Design Notes

Decided under `docs/owner-standing-rules.md` and the owner's go-ahead of 2026-11-22 (2.3 runs through if its plan only follows the 2.1 pattern):
- **No keyless Account.** AD-15's add text always reads a key, and the ticket does not ask for keyless. A local server that needs no key is story 4's reserved Accounts. For a compatible server that ignores keys, the owner types any text. Whether a compatible Account may have no key is left to the owner.
- **Remote endpoints on port 443 only.** `am-net` pins a remote endpoint by host, and port pinning is story 2's. Accepting only 443 here keeps the pin exact, as AD-48 asks (scheme, host and port), without touching `am-net`.
- **"Never the app's own port" (AD-12)** cannot be checked at add: `agentic serve` records its port nowhere, and `add` cannot run while `serve` holds the project. It belongs where a serving process makes calls, which is story 2's EgressClient. Carried into that story's plan.
- **`--model` is required for OpenAI and compatible Accounts.** No default model id is assumed for a provider whose model list this app does not hold.
- **Recordings are written in the provider's stream format.** No OpenAI key exists for a live capture, and the spike's local recording has one call only.

## Verification

**Commands:**
- `cargo nextest run -p am-providers -p am-config -p am-app -p am-e2e` -- expected: all pass.
- `just check` -- expected: green.

## Auto Run Result

Status: built (2026-11-22).

**Summary:**
- **Adapter:** the one OpenAI-chat-compatible adapter streams text with usage, assembles tool calls by index (or by id, or a new name) without merging them, returns no tool call when cancelled, and keeps reasoning deltas out of the answer.
- **The three model providers:** `agentic accounts add|list|test` takes `anthropic`, `openai` and `openai-compatible`.
- **Add is story 2.1's, unchanged:** terminal only, the name typed to confirm, the key into the Keychain only, History plus the `account_added` audit note. The note now names the provider, scheme, host and port.
  - A compatible Account's `base_url` must be `https://<DNS name>[/path]` on 443 or `http://127.0.0.1:<port>[/path]`. One am-config rule is shared by the loader and the CLI.
  - The sentence says when an existing Keychain key will be reused, and the add refuses if the Keychain entry changes while it waits.
- **The key goes only where the owner confirmed (owner, 2026-11-22):** `accounts test` reads a key and sends a request only when the accounts file is what the app last wrote, and the latest `account_added` note names the same provider, scheme, host and port. It runs from any stdin without the project lock.

**Files changed:**
- `crates/am-providers/src/openai_compat.rs` and the recordings under `fixtures/http/openai/` and `fixtures/http/openai_compat/`.
- `crates/am-config/src/{base_url,account_edit,kinds,lib}.rs`.
- `crates/am-store/src/audit.rs` (`Reader::audit_args`).
- `crates/am-app/src/accounts.rs`.
- `docs/config-schema.md`.
- Tests: `crates/am-providers/tests/openai_compat.rs`, `crates/am-app/tests/accounts_cli.rs`, `crates/am-config/tests/{account_edit,validate}.rs`, `crates/am-store` (audit args).

**Review:** two quick passes; the Review Triage Log has every row.
- Pass 1, 7 findings: 1 high (a key could go to a hand-edited endpoint; the plan was corrected to AD-15, AD-16 and AD-48, and the owner then extended the check to the provider), 3 medium and 1 low patched, 2 low rejected (the app's own port and `am-net`'s loopback looseness, both in story 2.2's plan).
- Pass 2 (follow-up, because pass 1 patched a high), 4 low findings, all patched. The reviewer found no path that sends a key to an unconfirmed provider or endpoint.
- Nothing deferred.
- Follow-up recommended: false.

**Verification:** `just check` passes (685 tests, 7 skipped). Every matrix row has a named test that ran and passed. Each round's red run was logged before its `src/` change. The login Keychain is untouched.

**Residual risks:**
- **Unconfirmed Accounts cannot be tested.** A hand-written Account (the Company A sample's included) is refused by `accounts test` until a later story records a confirmation (epic 4 story 18 or epic 5 story 5). A hand edit of `models/accounts.yaml` refuses `test` for every Account in it until the file is put back.
- **Keyless loopback Accounts** are a follow-up story (owner, 2026-11-22).
- **`max_tokens`:** the adapter sends it, as the spike did, and newer OpenAI models may want another field. This is a follow-up story (owner, 2026-11-22); the test call asks for only 64 output tokens.
- **Story 2.2's items:** the app's own port, and `am-net` pinning a remote endpoint by host and admitting any loopback spelling on a pinned port.
- **The Keychain comparison is existence only.** An entry whose value is replaced while a prompt waits is not noticed.
- **Not exercised:** nothing ran against a live OpenAI or compatible server.

**CI (2026-11-22):** the branch run (44992122815) did not start: GitHub refused both jobs for a billing block on the account. The story was merged to main (dfd492a) before any CI result, against the owner's merge rule; the slip is recorded and the merge step now reads the CI result first. Once the repository was public, CI re-ran on main for dfd492a (run 44992296291): both jobs, macOS arm64 (`just check`) and Linux portability, passed. 2.3's CI is passed.
