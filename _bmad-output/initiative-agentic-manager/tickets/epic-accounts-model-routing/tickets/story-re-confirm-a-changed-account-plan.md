---
title: 'Re-confirm a changed Account'
type: feature
ticket: 14
created: '2026-11-22'
status: built
baseline_revision: '21894de199b2ec637678bd56da2a88a6013b09a6'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick, adversarial-change]
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

**Problem:** After stories 2.12 and 2.13, the CLI has no way to:
- confirm an Account that `migrate` left unconfirmed;
- re-confirm one whose record a `remove` ended;
- move an Account to a new address or provider.

A hand edit of the user file is an outside change and is refused for every key use. AD-15 *Change* makes `agentic accounts change <id>` the CLI's only confirming path. The owner's words: "confirm by typing the Account name and seeing old vs new provider/address with an explicit yes — don't require typing the full stored key", and "a provider change must require a new key (treated as remove + add) and must never send the old key to the new provider; an address change within the same provider is allowed after confirmation with the warning".

Separately, the Account commands refuse while a reachable project enables a stdio MCP server, but not a loopback-HTTP one. The owner said to "treat stdio and loopback-HTTP servers the same way", and this story owns that change.

**Approach:**
- **`accounts change <id> [--provider P] [--base-url U]`** runs on a real terminal. It shows the old and new values side by side and every reference found, including the projects it could not check. Then it takes the typed id and an explicit `yes`.
- **Address change:** with the same provider, it writes `account_changed` and makes no Keychain call.
- **Provider change, or a first confirmation:** this is a remove plus an add of the same id, under `user.lock`:
  1. it commits `account_removed`;
  2. it deletes the Keychain entry, then sets a new key read at the hidden prompt, never reading the old one;
  3. it writes `account_added` with `confirmation: typed_id_yes_new_key`.
- **Before epic 4 story 18:** a change from a loopback to a remote endpoint, or to another host, is refused while a reachable project names the Account.
- **One shared scan:** am-config's owner-children scan also reports enabled loopback-HTTP MCP servers. `add`, `remove`, `change` and `migrate` refuse on them exactly as on stdio ones, while `agentic run`, `--take-over` and `--confirm-move` keep the stdio-only rule.

## Boundaries & Constraints

**Always:**
- **Terminal and confirmation (AD-15 *Terminal*):** this uses 2.12's one predicate. A non-terminal stdin is refused before anything is read or written.
  - **Sentence:** it shows the Account's old provider, scheme, host and port, and the new ones beside them. The old values come from its latest live confirmation record; for a first confirmation, from the file.
  - **References:** each reachable project that names the Account, with its seats, fallbacks, capabilities and Sources; then each project of the root index not checked, named as not checked.
  - **Confirming:** then the id typed within 2 minutes, then `yes`. A mismatch, a timeout or any other answer writes nothing, and the old record still governs.
  - **Unreachable projects:** they never refuse, so there is no `--force` (owner, 2026-11-22).
  - **Escaping:** every shown name and path goes through 2.12's `user::shown`.
- **Which path (AD-15 *Change*):**
  - **Address change:** the Account has a live confirmation record, and `--provider` is absent or names the same provider. `--base-url` gives the new address. The sentence says the key stored under the id will now go to the new endpoint.
    - The command makes no `SecretStore` call; FakeSecretStore records none.
    - It keeps the key state.
    - It writes the user file through 2.12's user writer with History. In the same user-store transaction it writes `account_changed` with the id, the old and new provider, scheme, host and port, the references, the key state (unchanged) and `confirmation: typed_id_yes`, and never the key.
  - **Provider change, or first confirmation:** `--provider` names another provider, or the Account has no live confirmation record (migrated unconfirmed, removed then still in the file, or a change cut short; flags optional). The sentence says:
    - the key stored under the id will not go to the new provider;
    - a new key is typed now;
    - the old key is deleted and not kept.

    After the typed id and `yes`, the new key is read at the hidden prompt: echo off, echo restored on every exit path, and an empty answer writes nothing. Then, holding `user.lock`:
    1. **Remove:** it commits `account_removed` (`reason: provider_change`, for a first confirmation too, AD-15 *Record*; and `key_for` naming the new provider, scheme, host and port).
    2. **Replace the key:** `SecretStore::delete(id)`, then `set(id, new)`. It never calls `get`, and never updates in place.
    3. **Add:** it writes the user file with History, and `account_added` with the new values, the references, `key: stored`, `replaces` (the old provider, scheme, host and port as `<provider> <scheme>://<host>:<port>`, or `none` when no live record existed; AD-15 *Record*) and `confirmation: typed_id_yes_new_key`, never the key.
- **Crash or failure after step 1:** the Account has no live record, so 2.12's Key-use check refuses every key use and `list` says so, until `change` is run again. A denied or locked Keychain fails at step 2, with the same result. When the app-written file already holds the confirmed values (a reconciled write whose record never committed), only the record is written, after the key is stored.
- **Keyless:** a keyless Account cannot exist before story 2.10, which builds keyless Accounts. Until then, `change` refuses any `--provider` or `--base-url` that would make a keyless Account remote. In practice no keyless path is reachable here.
- **Before epic 4 story 18 (AD-15 *Change*):** every kind of change from a loopback to a remote endpoint, or to another host, is refused, with nothing written and naming the projects, while a reachable project names the Account. An unreachable project is only listed. For a first confirmation, the file's values stand as the old destination.
- **Validation:** the new provider and `base_url` pass 2.3's and 2.12's rules: the provider kinds, the accepted endpoints (`https`, or loopback `http`, never the app's own port), and `secret_ref == id`. A refused value is never echoed beyond 2.12's membership hiding.
- **Re-read under the lock (2.12 *Lock*):** under `user.lock` the command re-reads the user file, the Account's latest record and the scan. Anything changed since the sentence refuses it, to be run again. An outside change of the user file refuses it, naming the file.
- **Undo and Restore (AD-15):** in user mode, am-config's writer refuses Undo or Restore of a user-file entry that would change any Account's provider or endpoint, naming `agentic accounts change`. So Undo of a provider change or a first confirmation is refused, because the old key is gone. Undo of a non-widening edit (a name) still works.
- **Loopback-HTTP MCP servers (owner 2026-11-22; AD-13, AD-15 *Open*):** `am_config::owner_children` keeps its stdio and `render: true` lists. It gains a separate list: each enabled `transport: http` MCP server whose `url` host is `127.0.0.1`, `[::1]` or `localhost`, whether `http` or `https`, on any port.
  - **Refuse on it:** `add`, `remove`, `change` and `migrate` refuse on that list exactly as on stdio, naming the project and the server.
  - **Keep stdio-only:** `agentic run`, `serve --take-over` and `--confirm-move` use only the stdio and `render` lists, as today.
  - **Not counted:** an `https` server on a non-loopback host counts for none of them.
- **Key use after a change:** 2.12's check already reads `account_changed` as a confirmation record. After an address change, `accounts test` sends the same key to the new endpoint. After a provider change, it sends the new key to the new provider. A hand edit of the provider or `base_url` with no record stays refused before any Keychain read.
- **Tests first:** written to fail first with FakeSecretStore recording every call, a canary key and a temporary HOME per test. The red run is logged before any `src/` change.

**Never:**
- **The old key:** never a `get` of it, never sent to a new provider, and the new key is never sent to the old one.
- **No key typing for address changes.** No key, key prefix or key hash in output, logs, History or audit.
- **No `--force`, and no refusal for an unreachable project.**
- **Outside this story:**
  - keyless Accounts (2.10);
  - the per-project destination record (epic 4 story 18);
  - the UI Account card (epic 5 story 5).
- **No new dependency.**

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Sentence | `change router --base-url https://new.example/v1`; one reachable project names `router` in a seat; another root is unreachable | Old and new provider and address side by side; the reachable project with its seat; the other named as not checked; then the id prompt | — |
| Wrong answer | Wrong id typed; or id right and `no` | Nothing written; the old record still governs; `test` still reaches the old endpoint | — |
| Address change | Same provider, new host, no project naming it; typed id and `yes` | No Keychain call; History plus `account_changed` (no key); `test` sends the same key to the new endpoint only | — |
| Provider change | `change cloud --provider openai-compatible --base-url https://router.example/v1`; typed id, `yes`, new key | `account_removed` (`reason: provider_change`), Keychain `delete` then `set`, never `get`; History plus `account_added` (`typed_id_yes_new_key`, `replaces`); `test` sends the new key to the new provider; the old canary reaches it at no point | — |
| Crash after remove | Fail point after `account_removed` commits | Every key use refused; `list` says unconfirmed; re-running `change` completes it | — |
| Keychain fails | `set` fails after `delete` | Account refused until `change` is re-run; nothing sent | Worded |
| First confirmation | Account migrated unconfirmed (2.13), `change spare` with no flags | Treated as a first confirmation: new key, records as above; `test` then sends | — |
| Empty key | Empty answer at the key prompt | Nothing written; no Keychain call | — |
| Echo | Ctrl-C at the key prompt | Echo restored; nothing written | — |
| Loopback to remote while named | A loopback Account named by a reachable project; `--base-url https://remote.example` | Refused naming the project; nothing written | Worded |
| Other host while named | A remote Account named by a reachable project; another host | Refused; the same host with another path is allowed | Worded |
| Unreachable only | Only an unreachable project in the root index | Listed as not checked; the change proceeds | — |
| Hand edit | Provider or `base_url` edited by hand, no record | `test` refused before any Keychain read; `change` refused as an outside change, naming the file | — |
| Undo | Undo of the provider change's History entry; Undo of a name edit | The first refused, naming `accounts change`; the second applies | — |
| Loopback-HTTP MCP | A reachable project enables `transport: http` at `http://127.0.0.1:<port>` (and `https://localhost`) | `add`, `remove`, `change` and `migrate` each refused, naming the project and server, nothing written; `agentic run` and `serve --take-over` not refused by it | Worded |
| Remote HTTP MCP | `transport: http` at `https://mcp.example` | No Account command refused | — |
| Not interactive | A pipe on stdin | Refused; nothing read or written | — |
| No key leaked | Every row | The canary is in no file, row, History, audit or output | — |

</intent-contract>

## Code Map

- **`crates/am-app/src/accounts.rs`:**
  - the `change` subcommand, beside `add` (whose hidden prompt, typed id and lock protocol it reuses) and `remove`;
  - the usage text.
- **`crates/am-app/src/user.rs`:** `scan` (L350) and the `owner_children` use: the new loopback-HTTP list is added to the refusal for the Account commands.
- **`crates/am-app/src/accounts/migrate.rs`:** the same refusal picks up the new list.
- **`crates/am-app/src/runtime.rs` and `cli.rs`:** `agentic run`, `serve --take-over` and `--confirm-move`. Check that they read only the stdio and `render` lists.
- **`crates/am-app/src/key_use.rs`:** already reads `account_changed` (L190). Add the `first_confirmation` and `provider_change` reasons to the words `list` shows.
- **`crates/am-config/src/lib.rs`:** `OwnerChildren` and `owner_children` (L859-920) gain a `loopback_http` list.
- **`crates/am-config/src/writer.rs`:** `undo` and `restore` (L686, L719) gain the user-mode refusal for a provider or endpoint change.
- **`crates/am-config/src/account_edit.rs`:** a `change_account_edit` beside `add_account_edit` and `remove_account_edit`.
- **Tests:** new `crates/am-app/tests/accounts_change.rs`, reusing 2.12's harness. A loopback-HTTP case added to the existing refusal tests of `add`, `remove` and `migrate`. An am-config writer test for the Undo refusal.

## Tasks & Acceptance

**Execution:**
- [ ] **Tests first (no `src/` change):** a test for every matrix row. Log the red run.
- [ ] **am-config:** `owner_children`' loopback-HTTP list, `change_account_edit`, the Undo and Restore refusal.
- [ ] **am-app:** `change` (address, provider, first confirmation, the pre-4.18 refusal, the lock re-read); the loopback-HTTP refusal in all four commands.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the outcome, the files, the user store and the Keychain calls match the row.
- Given the workspace, when `just check` runs, then it is green.

## Review Triage Log

### 2026-11-23 — Review pass (quick lens and an adversarial change lens)
- verdicts: 12 findings — high 3, medium 4, low 3, false 1, reject 1 (grouped below)
- routing note: patches are applied tests first, as in 2.12 and 2.13.
- findings:
  - Quick 1 `[high]` `[patch]` (group G-record) On the new-key path the file write carries no note and `account_added` commits later through `record_config_notes`, in a separate transaction. `config_note_args` now accepts any `config.recorded` note as a confirmation. AD-15 *Record* puts a confirmation in the same user-store transaction as its History entry. Fix: `account_added` rides on the file write (`apply_with_audit` notes). The record-only confirmation (file already holding the values) commits a History entry whose before equals its after, carrying the notes, through the writer and the same intent path. `record_config_notes` accepts only non-granting kinds (`account_removed`, `account_key_set`). `config_note_args` reads a `config.recorded` head only for those kinds.
  - Adversarial 2 `[high]` `[patch]` (group G-record) `record_config_notes` lets any holder of `user.lock` mint a live confirmation with no typed id and no check of the file. `Locked.history` is public, and `migrate` already holds it. Fixed by the G-record rule: no granting kind can be written that way.
  - Adversarial 1 `[high]` `[patch]` After an interrupted provider change (the key already replaced for Q, the file still at P), every message says to re-run `change X` with no flags, which defaults to P; pasting K_Q then confirms P with Q's key. Fix: after the Keychain `set`, commit a non-granting `account_key_set` (the `key_for` destination). With no flags, `change` defaults to that pending destination when the latest record is a provider-change removal followed by `account_key_set`, and its sentence says the entry already holds a key typed for Q. Every recovery message names the exact flags (`--provider Q --base-url R`).
  - Quick 2 `[medium]` `[patch]` (group G-keyfor) `last_confirmed` falls back to `account_removed`'s `key_for` even when the Keychain `delete` failed, so the reuse sentence names the wrong destination. Fix: `last_confirmed` uses only `account_key_set`, written after a successful `set`, never `key_for`.
  - Adversarial 4 `[medium]` `[patch]` (group G-keyfor) As Quick 2.
  - Adversarial 3 `[medium]` `[patch]` Two Restores can move an Account without confirmation: one drops it, the next re-adds it elsewhere, leaving the old live record. Fix (AD-15: "Undo of a user-file change goes through the same confirming handler"; "Undoing a remove re-adds an Account, which is widening"): user-mode Undo and Restore are refused whenever they would add, remove or move any Account. Only a non-Account change passes. 2.12's `remove_of_an_unreferenced_account_has_history_and_undo` expectation follows (coordinator decision).
  - Adversarial 5 `[medium]` `[patch]` The loopback-HTTP MCP list misses forms the loader accepts (`0.0.0.0`, `[::]`, `[::ffff:127.0.0.1]`, `127.0.0.2`, `localhost.`, `*.localhost`). Nothing is reachable through am-net today, but the AD-15 *Open* refusal should not rest on it. Fix: use the same "this Mac" predicate as `compatible_base_url` (every loopback and unspecified address, IPv4-mapped included, and `localhost`/`*.localhost` with or without a trailing dot).
  - Adversarial 6 `[low]` `[patch]` The app's own-port refusal is not repeated under the lock. Fix: repeat it in the lock re-read.
  - Quick 3 `[false]` `[reject]` "No green `just check` on record": the coordinator ran `just check` after the test corrections, 950 passed (recorded below).
  - Quick 4 `[low]` `[reject]` The Sentence row's input (`https://new.example/v1` while a reachable project names it) contradicts the pre-4.18 row; the test uses a loopback port change. The fix would edit this build's plan, so it is rejected; the test is right.
  - Quick 5 `[low]` `[patch]` `undo_or_restore_that_moves_an_account_is_refused_naming_change` claims, without testing it, that a project writer is untouched. Fix: assert it.
  - Quick 6 `[low]` `[patch]` A failure after the Keychain was replaced would say "no key was stored". Fix: say the key was replaced and how to finish.
- coordinator verification after the build's test corrections: `just check` green, 950 passed, 8 skipped (2026-11-23).

### 2026-11-23 — Follow-up review pass (adversarial, on the review fixes)
- verdicts: 3 findings — high 0, medium 2, low 1. The G-record, Adversarial 1, Undo/Restore and loopback-list fixes hold.
- findings:
  - `[medium]` `[patch]` A key pasted into the `--base-url` path is recorded for ever in `account_key_set`'s `key_for_base_url` and then echoed by `list`, `test` and `change`. The writer's checks (secret-looking values, added errors) run only at step (3), after the removal and the Keychain replacement. Fix: build the edit and run the writer's checks on its after-bytes before step (1), refusing with nothing recorded and no Keychain call. Never record or print a URL whose text the secret check flags.
  - `[medium]` `[patch]` (low-medium) `live_record` and `latest_record_seq` are read from two snapshots, so a removal committed between them goes unseen. The address path then revives an ended confirmation while the Keychain holds another provider's key. Fix: read both from one reader snapshot, and on the address path re-read the live record under the lock, requiring the same sequence and destination.
  - `[low]` `[patch]` `refuse_account_move` names raw ids from the file before the secret check runs. Fix: run the secret check first; show ids through `valid_ids`/`hide_text`.
  - informational, fixed with the above: the `OwnerChildren.loopback_http` doc now lists the full "this Mac" rule.

## Plan Change Log

- 2026-11-23 (coordinator, after the red run, before any `src/` change): corrected to AD-15 *Record*, which binds over the approved plan: the `account_removed` reason is `provider_change` for a first confirmation too, and `replaces` holds the old provider, scheme, host and port (or `none`), not a sequence number. Tests changed to match; owner told.

- 2026-11-22: approved by the owner as written ("Approve plan").

## Design Notes

**Decisions for the owner's approval:**
1. **One command, three paths:**
   - an address change makes no Keychain call;
   - a provider change asks for a new key;
   - a first confirmation (no live record) always takes the new-key path, whatever flags are given.

   The new-key path is the one AD-15 names for an Account that `migrate` left unconfirmed.
2. **The `account_removed` reason** is `provider_change` for both a provider change and a first confirmation, and `replaces` holds the old destination or `none`, as AD-15 *Record* binds (corrected 2026-11-23 before any code; the approved draft had `first_confirmation` and a sequence number).
3. **The pre-4.18 refusal:** "another host" compares the host only. While a reachable project names the Account, a change of port, path or scheme on the same host is allowed, but loopback to remote, or any host change, is refused.
4. **Undo:** the writer refuses Undo or Restore of any user-file entry that would change an Account's provider or endpoint, not only provider changes. Every such change must go through `change`'s confirmation (AD-15: "Undo of a user-file change goes through the same confirming handler").
5. **The loopback-HTTP list** is kept apart from the stdio list in am-config, so `agentic run` and `--take-over` keep their rule without a flag.

**Size:** medium. One command with three paths, one scan list, one writer guard, about 18 matrix rows.

## Verification

- `cargo nextest run -p am-app --test accounts_change`, plus the am-config tests and the `add`, `remove` and `migrate` refusal tests; then `just check`.

## Implementation Notes

### Red run (2026-11-22, before any `src/` change)

Tests written (no file under any `src/` changed):
- `crates/am-app/tests/accounts_change.rs` (new, 22 tests: every matrix row of `change`, plus the lock re-read, validation, usage, a reconciled write and a record a `remove` ended). The test-data and wording decisions are in its header (prompt order id, `yes`, key; exit codes as `add`; the `account_changed` / `account_removed` / `account_added` arg names; `list` says `unconfirmed`; the crash is the user writer's `BeforeRename` fail point; the Sentence row uses a loopback port change because the matrix's other host is refused by the pre-4.18 row while a reachable project names the Account).
- `crates/am-app/tests/accounts_migrate.rs`: `a_loopback_http_mcp_server_refuses_migrate`, `an_account_migrate_left_unconfirmed_is_confirmed_by_change`.
- `crates/am-app/tests/accounts_tty.rs` (real binary): `change_with_stdin_that_is_not_a_terminal_is_refused`, `pty::typed_ctrl_c_at_the_change_key_prompt_restores_echo_and_writes_nothing`.
- `crates/am-app/tests/run_cli.rs`: `a_loopback_http_mcp_server_does_not_refuse_the_run`; `crates/am-app/tests/runtime_open.rs`: `a_loopback_http_mcp_server_does_not_refuse_a_take_over` (guards of the stdio-only rule; they pass now and must stay green).
- `crates/am-config/tests/account_change.rs` (new: `owner_children(..).loopback_http`, `change_account_edit`); `crates/am-config/tests/user_accounts.rs`: `undo_or_restore_that_moves_an_account_is_refused_naming_change`.

Command 1:
`cargo nextest run -p am-app --no-fail-fast -E 'binary(accounts_change) | test(loopback_http) | test(an_account_migrate_left_unconfirmed_is_confirmed_by_change) | test(/change_with_stdin|change_key_prompt/)'`

```
     Summary [   1.450s] 28 tests run: 2 passed, 26 failed, 261 skipped
        FAIL am-app::accounts_change a_crash_after_the_removal_leaves_the_account_refused_until_changed_again
        FAIL am-app::accounts_change a_failing_keychain_leaves_the_account_refused_until_changed_again
        FAIL am-app::accounts_change an_address_change_makes_no_keychain_call_and_moves_the_same_key
        FAIL am-app::accounts_change an_empty_key_writes_nothing
        FAIL am-app::accounts_change a_provider_change_removes_then_adds_with_a_new_key
        FAIL am-app::accounts_change a_refused_provider_or_address_is_never_echoed
        FAIL am-app::accounts_change a_first_confirmation_takes_a_new_key
        FAIL am-app::accounts_change a_file_changed_while_the_prompt_waits_refuses_the_change
        FAIL am-app::accounts_change a_wrong_answer_writes_nothing_and_the_old_record_governs
        FAIL am-app::accounts_change a_reconciled_write_whose_record_never_committed_gets_only_the_record
        FAIL am-app::accounts_change a_hand_edit_is_refused_by_test_and_by_change
        FAIL am-app::accounts_change change_is_in_the_usage_and_takes_no_force
        FAIL am-app::accounts_change undo_of_a_provider_change_is_refused_and_of_a_name_edit_applies
        FAIL am-app::accounts_change change_from_a_pipe_is_refused_before_anything_is_read
        FAIL am-app::accounts_change ctrl_c_at_the_key_prompt_restores_echo_and_writes_nothing
        FAIL am-app::accounts_change a_remote_http_mcp_server_refuses_no_account_command
        FAIL am-app::accounts_change an_unreachable_project_is_listed_and_never_refuses
        FAIL am-app::accounts_change a_loopback_http_mcp_server_refuses_the_account_commands
        FAIL am-app::accounts_change another_host_is_refused_while_named_but_another_path_is_not
        FAIL am-app::accounts_change an_account_whose_record_a_remove_ended_is_confirmed_by_change
        FAIL am-app::accounts_change loopback_to_remote_is_refused_while_a_reachable_project_names_it
        FAIL am-app::accounts_change the_sentence_shows_old_and_new_and_every_reference
        FAIL am-app::accounts_migrate an_account_migrate_left_unconfirmed_is_confirmed_by_change
        FAIL am-app::accounts_migrate a_loopback_http_mcp_server_refuses_migrate
        FAIL am-app::accounts_tty change_with_stdin_that_is_not_a_terminal_is_refused
        FAIL am-app::accounts_tty pty::typed_ctrl_c_at_the_change_key_prompt_restores_echo_and_writes_nothing
        PASS am-app::run_cli a_loopback_http_mcp_server_does_not_refuse_the_run
        PASS am-app::runtime_open a_loopback_http_mcp_server_does_not_refuse_a_take_over
```

First failure lines (the `change` rows all stop at `change` being an unknown accounts command, exit 2 with the usage):
```
thread 'a_provider_change_removes_then_adds_with_a_new_key' panicked at crates/am-app/tests/accounts_change.rs:802:5:
assertion `left == right` failed:
usage: agentic accounts add <id> --provider anthropic|openai|openai-compatible [--base-url <url>] [--name <text>]
...
agentic: unknown accounts command `change`
  left: 2
 right: 0
thread 'a_first_confirmation_takes_a_new_key' panicked at crates/am-app/tests/accounts_change.rs:1003:5:
spare  anthropic  api.anthropic.com  ••••••••WXYZ        (list does not say unconfirmed)
thread 'an_account_whose_record_a_remove_ended_is_confirmed_by_change' panicked at crates/am-app/tests/accounts_change.rs:1076:5:
agentic: the Account 'cloud' is not called: ... ('cloud' was removed with `agentic accounts remove` after it was last added) ...   (test does not name `agentic accounts change cloud`)
thread 'a_loopback_http_mcp_server_refuses_the_account_commands' panicked at crates/am-app/tests/accounts_change.rs:1501:13:
assertion `left == right` failed: http://127.0.0.1:3001/mcp ["add", "cloud2", "--provider", "anthropic"]: agentic: added the Account 'cloud2' (anthropic, https://api.anthropic.com) ...
thread 'a_loopback_http_mcp_server_refuses_migrate' panicked at crates/am-app/tests/accounts_migrate.rs:2921:9:
assertion `left != right` failed: http://127.0.0.1:3001/mcp: agentic: moved the Accounts of .../projects/p to this Mac's Accounts ...
thread 'pty::typed_ctrl_c_at_the_change_key_prompt_restores_echo_and_writes_nothing' panicked at crates/am-app/tests/accounts_tty.rs:316:21:
exited (exit status: 2) before the id prompt; terminal: usage: agentic accounts add <id> ...
```

Command 2: `cargo nextest run -p am-config --no-fail-fast --test user_accounts -E 'test(undo_or_restore)'`
```
thread 'undo_or_restore_that_moves_an_account_is_refused_naming_change' panicked at crates/am-config/tests/user_accounts.rs:815:42:
called `Result::unwrap_err()` on an `Ok` value: HistoryEntry { ..., kind: Undo, ... }
     Summary [   0.041s] 1 test run: 0 passed, 1 failed, 23 skipped
```

Command 3: `cargo nextest run -p am-config --test account_change`
```
error[E0432]: unresolved import `am_config::change_account_edit`
error[E0609]: no field `loopback_http` on type `OwnerChildren`   (x5)
error: could not compile `am-config` (test "account_change") due to 6 previous errors
```

Note for the build: AD-15 *Record* says `replaces` is "the old provider, scheme, host and port, or none", while this plan says the ended record's sequence number; the tests follow the plan (`replaces` = the `seq` of the ended `account_added`). AD-15 also says `account_removed` carries `reason: provider_change` for a first confirmation too, while the plan's Decision 2 uses `first_confirmation`; the tests follow the plan.

### Tests changed after the red run (2026-11-23, coordinator, before any `src/` change)

The tests now follow AD-15 *Record*, which binds over the approved plan:
- Every expected `reason: first_confirmation` is now `provider_change`, in `accounts_change.rs` (2) and `accounts_migrate.rs` (1).
- The provider-change test expects `replaces: "anthropic https://api.anthropic.com:443"` (the old destination) instead of the ended record's sequence number. The first-confirmation test already accepts `none` or no `replaces`.
- The file header in `accounts_change.rs` says the same.

The assertions are not weakened: each still checks one exact value.

### Build (2026-11-23, after the red run)

Decisions made in the build (owner standing rules: layout, names, wording):
- **Records with no History entry.** AD-15 needs `account_removed` committed on its own before the Keychain is touched, and only the records when the file already holds the confirmed values; the user store could write `config.*` notes only with an intent's History entry. Added `am_store::user::UserStore::record_config_notes` (one transaction, guard-only, refused unless the file's latest History entry holds the bytes the caller saw and no intent of it is pending): a `config.recorded` entry naming that History entry, then the notes; `config_note_args` reads them like a commit's. Outside the Code Map (am-store); no new dependency, no store migration.
- **New-key path order:** `account_removed` (record only), `delete` then `set`, the file written with no note, then `account_added` recorded after the History commit. So a crash between the rename and the History commit reconciles with no record ("a reconciled write whose record never committed"), and the next `change` writes only the record. The address change keeps `account_changed` on the write's intent (one transaction).
- **Scan:** `OwnerChildren::account_words()` (stdio, `render`, broken files and `loopback_http`); `crate::user::scan` and `migrate`'s own-project check use it; `words()` and `decisions_locked` are unchanged. A `transport: http` server whose `url` cannot be parsed counts as loopback (fails closed).
- **Undo/Restore guard:** user mode only; refused when an Account present in both the current file and the target has another provider or `base_url`. Re-adding an Account (Undo of a remove) is not refused, as 2.12's test expects.
- **Words:** `test` and `list` name `agentic accounts change <id>`; `list` adds `unconfirmed: …` to an Account with no live record or one for another destination than the file's. A change that changes nothing is refused (exit 1).

Verification: `just check` recipes all pass except `test`; `cargo nextest run --no-fail-fast`: 950 run, 948 passed, 2 failed (below), 8 skipped. The PTY and pipe tests of `change` pass on the real binary.

Open (tests disagree with the plan; not changed, for the coordinator):
1. `accounts_change.rs::a_file_changed_while_the_prompt_waits_refuses_the_change` asserts `env.audit() == audit` with `audit` read before the command runs, but the concurrent edit the test makes at the id prompt writes its own `config.intent_opened` and `config.edited` entries, so no implementation that holds no lock at its prompts (AD-15 *Open*) can pass it. The command refuses ("run the command again"), makes no Keychain call and writes no record. Suggested fix: assert no new `account_removed`/`account_added` for `cloud` instead of the whole audit.
2. `accounts_cli.rs::test_refuses_what_the_latest_add_of_the_account_did_not_record` (story 2.12) builds its state by a History Restore that moves an Account's address, which this story's Decision 4 now refuses. Suggested fix: write the same values with an app edit (`change_account_edit` through `user_writer().apply`), keeping the assertions.

### Tests changed after the build (2026-11-23, coordinator decision)

Two tests disagreed with the plan, not with the code:

1. **`a_file_changed_while_the_prompt_waits_refuses_the_change`** compared the whole audit log. The test's own edit at the prompt writes History audit rows, which AD-15 *Open* allows, because no lock is held during the prompts. The test now asserts that the command adds no `account_removed`, `account_added` or `account_changed` record for `cloud`.
2. **`accounts_cli::test_refuses_what_the_latest_add_of_the_account_did_not_record`** (story 2.12) built its case with a History Restore that moves an Account. Decision 4 now refuses that. The helper writes the same values through an app edit (`change_account_edit` with no record), and every assertion is unchanged.

Implementer choices accepted:
- **`UserStore::record_config_notes`** writes records alone, under the lock, against the file's latest History entry. It is used for the separate `account_removed` commit and for record-only confirmations.
- **New-key order:** `account_removed`, Keychain `delete` then `set`, the file write, then `account_added`.
- **Loopback-HTTP scan:** an unparseable `transport: http` URL counts as loopback, so the check fails closed.
- **Undo and Restore refusal:** only for an Account present on both sides with a different provider or `base_url`.

### Review fixes: red run (2026-11-23, tests first, before any `src/` change)

Tests for each `[patch]` row of the 2026-11-23 Review Triage Log. No file under any `src/` folder was changed by this round.

- **G-record** (`crates/am-app/tests/accounts_change.rs`):
  - `a_provider_changes_confirmation_commits_with_its_history_entry`: the new `account_added`'s head is the file's History commit (`entry_id` = the last History entry, same `at`), never `config.recorded`.
  - `record_config_notes_refuses_the_granting_kinds`: `account_added` and `account_changed` refused with nothing written; `account_removed` and `account_key_set` accepted.
  - `a_recorded_account_added_is_never_a_confirmation`: a `config.recorded` `account_added` for an unconfirmed Account is not a confirmation; `test` is refused with no Keychain read.
  - Expectations updated to the fix (record-only confirmation commits a History entry whose before equals its after, carrying the notes): `a_first_confirmation_takes_a_new_key` (History +1) and `a_reconciled_write_whose_record_never_committed_gets_only_the_record` (History +2). Both also check the one-transaction head (`assert_committed_with`).
- **Adversarial 1**: `an_interrupted_provider_change_finishes_at_its_new_destination`. The refusal, `list` and `test` name `--provider openai-compatible --base-url https://router.example/v1`. `change cloud` with no flags then defaults to router.example, its sentence says the entry already holds a key typed for openai-compatible, and anthropic is never confirmed again.
- **G-keyfor**: `a_failed_delete_leaves_the_held_key_named_for_its_old_destination`. With the Keychain `delete` failing, a later `remove` then `add` says the held key was last confirmed for provider anthropic, not router.example.
- **Adversarial 3**:
  - `crates/am-config/tests/user_accounts.rs::undo_or_restore_that_adds_or_drops_an_account_is_refused`: a Restore that drops an Account and a Restore that re-adds it elsewhere are both refused; an Undo of a remove is refused naming `agentic accounts add`; a name-only Undo applies.
  - 2.12's `accounts_cli.rs::remove_of_an_unreferenced_account_has_history_and_undo` now expects the Undo of the remove refused, naming `agentic accounts add`, with nothing written (coordinator decision).
- **Adversarial 5**:
  - `accounts_change.rs::every_form_of_a_loopback_http_mcp_server_refuses_add`: `https://0.0.0.0/`, `https://[::]/`, `https://[::ffff:127.0.0.1]/`, `https://127.0.0.2/`, `https://localhost./`, `https://x.localhost/`. Each makes `add` refuse, naming the server and project.
  - `crates/am-config/tests/account_change.rs::every_form_that_reaches_this_mac_is_in_the_loopback_http_list`: the same forms plus `x.localhost.`; `localhost.example` is not listed.
- **Adversarial 6**: `a_server_on_the_new_port_since_the_sentence_refuses_the_change`. An `agentic serve` record for port 9090 appears at the id prompt; the change is refused under the lock naming 9090, nothing written, no Keychain call.
- **Quick 5**: `user_accounts.rs::a_project_writers_undo_of_a_seat_edit_is_unaffected`. A guard: it passes now and must stay green.
- **Quick 6**: not constructible from outside `src/`. The only "no key was stored" message reachable after the Keychain `set` is `change.rs`'s `edit()` returning `None`. That can happen only if the file read under the lock lacks the id, but the lock re-read has just confirmed the same bytes as before the prompts, which held it. The Adversarial 1 test asserts as far as is reachable: after the fail point, the refusal does not say "no key was stored".

Command 1: `cargo nextest run -p am-app --no-fail-fast -E 'test(a_provider_changes_confirmation_commits_with_its_history_entry) | test(record_config_notes_refuses_the_granting_kinds) | test(a_recorded_account_added_is_never_a_confirmation) | test(an_interrupted_provider_change_finishes_at_its_new_destination) | test(a_failed_delete_leaves_the_held_key_named_for_its_old_destination) | test(every_form_of_a_loopback_http_mcp_server_refuses_add) | test(a_server_on_the_new_port_since_the_sentence_refuses_the_change) | test(a_first_confirmation_takes_a_new_key) | test(a_reconciled_write_whose_record_never_committed_gets_only_the_record) | test(remove_of_an_unreferenced_account_has_history_and_undo)'`
```
     Summary [   0.281s] 10 tests run: 0 passed, 10 failed, 286 skipped
thread 'record_config_notes_refuses_the_granting_kinds' panicked at crates/am-app/tests/accounts_change.rs:1799:9:
account_added is refused
thread 'an_interrupted_provider_change_finishes_at_its_new_destination' panicked at crates/am-app/tests/accounts_change.rs:1857:5:
the exact flags: agentic: the new values of 'cloud' were not written: the write stopped: simulated crash before the rename. This command did not change accounts.yaml; the new key is in the Keychain. Its old confirmation ...
thread 'a_recorded_account_added_is_never_a_confirmation' panicked at crates/am-app/tests/accounts_change.rs:1831:5:
assertion `left == right` failed: one two three ...   (test sent: the forged record was read as a confirmation)
thread 'a_server_on_the_new_port_since_the_sentence_refuses_the_change' panicked at crates/am-app/tests/accounts_change.rs:2013:5:
assertion `left == right` failed: agentic: changed the Account 'router' to provider openai-compatible at http://127.0.0.1:9090 ...
thread 'a_first_confirmation_takes_a_new_key' panicked at crates/am-app/tests/accounts_change.rs:1094:5:
assertion `left == right` failed  left: 1 ...   (no History entry for the record-only confirmation)
thread 'a_provider_changes_confirmation_commits_with_its_history_entry' panicked at crates/am-app/tests/accounts_change.rs:590:5:
assertion `left != right` failed: a confirmation never rides on a record-only head: {"action":"config.recorded",...}
thread 'a_reconciled_write_whose_record_never_committed_gets_only_the_record' panicked at crates/am-app/tests/accounts_change.rs:1698:5:
assertion `left == right` failed: the reconciled entry, then the confirmation's entry  left: 2 ...
thread 'a_failed_delete_leaves_the_held_key_named_for_its_old_destination' panicked at crates/am-app/tests/accounts_change.rs:1934:5:
... It was last confirmed for provider openai-compatible at https://router.example:443 ...
thread 'every_form_of_a_loopback_http_mcp_server_refuses_add' panicked at crates/am-app/tests/accounts_change.rs:1969:9:
assertion `left == right` failed: https://0.0.0.0/mcp: agentic: added the Account 'cloud' (anthropic, https://api.anthropic.com) ...
thread 'remove_of_an_unreferenced_account_has_history_and_undo' panicked at crates/am-app/tests/accounts_cli.rs:1222:52:
called `Result::unwrap_err()` on an `Ok` value: HistoryEntry { ... }
```

Command 2: `cargo nextest run -p am-config --no-fail-fast -E 'test(undo_or_restore_that_adds_or_drops_an_account_is_refused) | test(a_project_writers_undo_of_a_seat_edit_is_unaffected) | test(every_form_that_reaches_this_mac_is_in_the_loopback_http_list)'`
```
     Summary [   0.187s] 3 tests run: 1 passed, 2 failed, 294 skipped
        FAIL am-config::account_change every_form_that_reaches_this_mac_is_in_the_loopback_http_list
        FAIL am-config::user_accounts undo_or_restore_that_adds_or_drops_an_account_is_refused
        PASS am-config::user_accounts a_project_writers_undo_of_a_seat_edit_is_unaffected   (guard, Quick 5)
thread 'every_form_that_reaches_this_mac_is_in_the_loopback_http_list' panicked at crates/am-config/tests/account_change.rs:211:9:
https://0.0.0.0/mcp: OwnerChildren { enabled: [], broken: [], loopback_http: [] }
thread 'undo_or_restore_that_adds_or_drops_an_account_is_refused' panicked at crates/am-config/tests/user_accounts.rs:934:26:
a Restore that drops router: HistoryEntry { ..., kind: Restore, ... }
```

`cargo clippy -p am-app -p am-config --tests -- -D warnings` and `cargo fmt --all -- --check` are clean.

### Review fixes: build (2026-11-23)

- **G-record:** `account_added` and `account_changed` ride on the write's intent (`apply_with_audit`). When the file already holds the values, the new `ConfigWriter::apply_unchanged_with_audit` (user mode only, at least one note) commits an Edit whose before equals its after. am-store's intent check accepts before == after only for an Edit that carries notes. `UserStore::record_config_notes` accepts only `am_store::audit::RECORDED_KINDS` (`account_removed`, `account_key_set`), and `config_note_args` skips every other kind after a `config.recorded` head.
- **Lock re-read with a settled write:** `Locked.settled` lists the History entries that the lock's reconcile completed. A note written since the sentence is a change unless its History entry is one of those. Without this, the reconcile's own `account_added` would refuse the re-run that the reconciled-write row expects. `replaces` is read again under the lock.
- **Adversarial 1, G-keyfor:** after a successful `set`, `account_key_set` (`key_for_…`, plus `key_for_base_url` for a compatible Account) is recorded. `key_use::pending_change` and `change_command` drive the flags in the refusal, `list`, `test`/NotConfirmed and the no-flag default of `change`. `last_confirmed` reads only confirmation records and `account_key_set`.
- **Adversarial 3:** the writer refuses a user-mode Undo or Restore that adds, removes or moves any Account, naming `add`, `remove` or `change`.
- **Adversarial 5:** `base_url::names_this_mac` is the one "this Mac" rule; `compatible_base_url` and the loopback-HTTP list both use it.
- **Adversarial 6:** the own-port check runs again under the lock.
- **Quick 6:** the edit is built before any Keychain call, and every refusal after `set` says the entry was replaced and names the exact `change` command.
- Ran: `cargo nextest run -p am-store` (98 passed), `cargo nextest run -p am-config -p am-app` (592 passed, 1 skipped); clippy `-D warnings` on the three crates and fmt are clean.

### Follow-up fixes: red run (2026-11-23, tests first, before any `src/` change)

Tests for the three `[patch]` rows of the 2026-11-23 follow-up review pass. No file under any `src/` folder was changed by this round.

- **Key in the `--base-url` path** (`crates/am-app/tests/accounts_change.rs::a_key_pasted_into_the_base_url_is_refused_before_anything_is_recorded`):
  - The command: `change cloud --provider openai-compatible --base-url https://router.example/<canary>/v1`, with the id, `yes` and a new key.
  - Expected: refused (exit 1) with no Keychain call, no `account_removed`, no `account_key_set`, and nothing written.
  - The canary must be in no output (the sentence included), file, History or audit entry, and not in `list` or `test` afterwards.
  - The old confirmation still governs: `test` sends.
- **Two-snapshot race**: not constructible from outside `src/`. The two reads (`key_use::live_record`, then `key_use::latest_record_seq`) both run before the first prompt, with no test hook between them. Instead, `an_address_change_is_refused_when_a_removal_commits_while_it_waits`:
  - While the id prompt of an address change waits (`FakeTerminal::line_and`), another process commits a provider-change `account_removed` for the id through `UserStore::record_config_notes`, with no `account_key_set`.
  - Expected: refused under the lock ("run the command again"), no `account_changed`, no Keychain call, the file unchanged, and `test` refused with no key read.
  - It passes now: the lock re-read (`notes_after`) already catches a removal committed during the prompt. It is a guard and must stay green.
- **Key-shaped id in a Restore** (`crates/am-config/tests/user_accounts.rs::a_restore_that_drops_a_key_shaped_id_never_echoes_it`):
  - The current file holds a hand-added kebab-case Account `sk-ant-api03-idcanary-…` with an error (`secret_ref: other`).
  - A user-mode Restore of the create, or of a name edit, would drop it.
  - Expected: each is refused with the secret refusal ("secret-looking"), never echoing the id (Display or Debug), and nothing is written.

Command 1: `cargo nextest run -p am-app --no-fail-fast -E 'test(a_key_pasted_into_the_base_url_is_refused_before_anything_is_recorded) | test(an_address_change_is_refused_when_a_removal_commits_while_it_waits)'`
```
        PASS [   0.060s] (1/2) am-app::accounts_change an_address_change_is_refused_when_a_removal_commits_while_it_waits
        FAIL [   0.059s] (2/2) am-app::accounts_change a_key_pasted_into_the_base_url_is_refused_before_anything_is_recorded
     Summary [   0.062s] 2 tests run: 1 passed, 1 failed, 296 skipped
thread 'a_key_pasted_into_the_base_url_is_refused_before_anything_is_recorded' panicked at crates/am-app/tests/accounts_change.rs:2059:5:
no Keychain call: ["delete:cloud", "set:cloud"]
```

Command 2: `cargo nextest run -p am-config --no-fail-fast -E 'test(a_restore_that_drops_a_key_shaped_id_never_echoes_it)'`
```
        FAIL [   0.041s] (1/1) am-config::user_accounts a_restore_that_drops_a_key_shaped_id_never_echoes_it
     Summary [   0.041s] 1 test run: 0 passed, 1 failed, 297 skipped
thread 'a_restore_that_drops_a_key_shaped_id_never_echoes_it' panicked at crates/am-config/tests/user_accounts.rs:1063:9:
refused: accounts.yaml: the restore of entry 00000000-0000-7000-8000-000000000001 would remove the Account '<id>', which only `agentic accounts remove` does on this Mac; nothing was written
```
(The test prints the id as `<id>`; the real message holds the key-shaped id itself.)

`cargo clippy -p am-app -p am-config --tests -- -D warnings` and `cargo fmt --all -- --check` are clean.

### Follow-up fixes: build (2026-11-23)

- **A key in `--base-url`:**
  - New `am_config::looks_like_a_secret`, the writer's secret rule. `change` refuses a flagged `--base-url` first, before anything is shown, recorded or touched.
  - New `ConfigWriter::check_edit` runs the writer's checks with no write: the edit's base bytes, a secret-looking value before or after, and added errors. `change` calls it under the lock before step (1). `step` now shares those checks through `step_checks`.
  - `key_for_base_url` is never recorded for a flagged URL. `pending_change` offers no flags for one.
- **One snapshot:** `key_use::record_state` derives the live record and the latest sequence from one read of the notes. On the address path, `change` reads it again under the lock and needs the same sequence and destination. `replaces` uses that read.
- **`refuse_account_move`:** the secret check runs on the current and target bytes first. Only ids in `valid_ids` are named; any other entry is "(an entry whose id is not shown)".
- **`OwnerChildren.loopback_http` doc:** it now states the full "this Mac" rule.
- **Ran:** `cargo nextest run -p am-config -p am-app` (595 passed, 1 skipped). Clippy `-D warnings` and fmt are clean.

## Auto Run Result

**Status:** built (2026-11-23).

**Summary:** `agentic accounts change <id> [--provider] [--base-url]` runs on a real terminal. Its sentence shows the old and new values side by side, every reference found and the projects not checked (never refused for an unreachable one). Then it takes the typed id and an explicit `yes`.
- **Address change:** with the same provider, it writes `account_changed` in the same transaction as the file write and makes no Keychain call.
- **Provider change, or first confirmation**, with a new key at the hidden prompt:
  1. `account_removed` (`reason: provider_change`);
  2. Keychain `delete` then `set`, never `get`;
  3. a non-granting `account_key_set`;
  4. the file write with `account_added` (`replaces`, `typed_id_yes_new_key`) in one transaction.
- **Checks before step 1:** every writer check runs, so nothing is recorded or deleted for a value the write would refuse.
- **Interrupted change:** every message names the exact flags, and `change <id>` with no flags defaults to the pending destination.
- **Before epic 4 story 18:** a change from loopback to remote, or to another host, is refused while a reachable project names the Account.
- **Undo and Restore:** in user mode they are refused whenever they would add, remove or move an Account.
- **Loopback-HTTP MCP servers** (one "this Mac" rule) make `add`, `remove`, `change` and `migrate` refuse. `agentic run`, `--take-over` and `--confirm-move` are unchanged.

**Files changed (src):**
- **am-app:**
  - `accounts/change.rs` (new);
  - `accounts.rs`: parsing, `list` and `test` words;
  - `key_use.rs`: `record_state`, `pending_change`, `last_confirmed`;
  - `user.rs`: the loopback-HTTP refusal and `settled`;
  - `accounts/migrate.rs`.
- **am-config:**
  - `lib.rs`: `loopback_http`, `account_words`, `looks_like_a_secret`;
  - `base_url.rs`: `names_this_mac`;
  - `account_edit.rs`: `change_account_edit`;
  - `writer.rs`: `apply_unchanged_with_audit`, `check_edit`, the user-mode Undo and Restore refusal.
- **am-store:**
  - `user.rs`: `record_config_notes`, non-granting kinds only;
  - `audit.rs`: `RECORDED_KINDS` and the `config.recorded` reading;
  - `config_history.rs`: an Edit whose before equals its after is accepted only with notes.

**Review:**

| Pass | Findings | Patched | Rejected |
|---|---|---|---|
| First (quick lens and adversarial change lens) | 12: high 3, medium 4, low 3, false 1, plus 1 rejected | 10 | 2 |
| Follow-up | 3: medium 2, low 1 | 3 | — |

- **Rejected:** "no green check on record" (false), and the Sentence-row wording (it would edit the plan).
- **The two-snapshot race fix** has no failing test, because it cannot be built from outside `src/`; a guard covers the nearest case.
- **Patched by verdict:** high 3, medium 6, low 4. Every patch round ran tests first.

**Follow-up review:** not recommended. The last pass patched no high finding.

**Verification:** `just check` passes, 963 tests passed and 8 skipped.

**Residual risks:**
- If `account_key_set` fails after the `set`, `add` names the old destination in its words; it still requires the held key to be typed.
- An unchanged-write intent left pending after a failed `drop_intent` is completed by reconcile.
- `names_this_mac` does not cover IPv4-compatible or NAT64 forms or DNS names that resolve to loopback. am-net refuses all of these.

