---
title: 'Keyless Accounts on this Mac'
type: feature
ticket: 10
created: '2026-11-25'
status: approved
baseline_revision: '96028e7'
after: [3, 12, 14]
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
  - '{project-root}/docs/deferred-register.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-accounts-model-routing/epic-accounts-model-routing.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-accounts-model-routing/tickets/story-openai-and-compatible-accounts-plan.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-accounts-model-routing/tickets/story-accounts-belong-to-this-mac-plan.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-accounts-model-routing/tickets/story-re-confirm-a-changed-account-plan.md'
warnings:
  - 'Security story (Accounts, secrets, egress): no test or code is written until the owner answers Q1-Q8.'
  - 'Ticket risk is medium; this plan runs at high (manager instruction). The ticket is not edited.'
deferred: []
---

<intent-contract>

## Intent

**Story:** 2.10, epic 2 entry `id = 10`, "Keyless Accounts on this Mac" (`covers = ["R1"]`, `after = [3, 12, 14]`, all three built). Owner decision 2026-11-22 (epic 2 Notes, "Keyless loopback Accounts"): "allow them only for loopback endpoints (http://127.0.0.1:<port>), so a local llama.cpp-style server needs no fake key; remote endpoints always need a key". AD-15 *Keyless loopback Account* (architecture L222) binds the shape and says it is built by this story. It closes DR-079.

**This is a security stop.** The plan is a draft. Nothing is written (tests or code) until the owner answers Q1–Q8 under Design Notes, "Owner questions (security)". The Approach below is the planner's recommendation, pending those answers.

**Baseline:** main is `b234d74`. Story 3.7 (PR #48, branch `story/3-7`, `96028e7`) is not merged. It adds `crates/am-app/src/prices.rs` and edits `crates/am-config/src/kinds.rs`, which this story also edits (the `secret_ref` fix text). The baseline is set to main once #48 merges, so the two never rebase over each other. Line numbers below are on `story/3-7`; outside `prices.rs` and `kinds.rs`'s prices section they match main.

**Problem (the code as it stands):**
- **Every Account must have a key.**
  - `add` reads a key for every provider: `crates/am-app/src/accounts.rs` L740-L798. An empty key is refused ("no key was typed", L791-L793).
  - The `account_added` note always records `key: stored` (L901).
  - `account_item` always writes `secret_ref: <id>` (`crates/am-config/src/account_edit.rs` L284-L296).
  - The 2.3 tests say so in words: `crates/am-app/tests/accounts_cli.rs` L2348-L2352 ("there is no keyless Account") and L2903-L2905.
- **A missing `secret_ref` means the id's key today.** `am_config::key_ref` (`crates/am-config/src/user_accounts.rs` L63-L68) returns the id when `secret_ref` is absent. AD-15 makes an absent `secret_ref` mean *keyless* from this story on.
- **The check accepts only a stored key.** `Destination::names` (`crates/am-app/src/key_use.rs` L279-L285) requires `key == "stored"`. So a `key: none` record could never confirm anything. The check never compares the file's key state with the record's (AD-15: "refuse an Account whose file `secret_ref` disagrees with its key state").
- **`test` always reads and sends a key.**
  - `test` reads the Keychain (`accounts.rs` L1342-L1366) and refuses when the key is missing.
  - It builds the compatible adapter with a `Bearer` reference (L1376-L1385).
- **`list` reads the Keychain for every Account** (L1172-L1181). It has no keyless wording.
- **`remove` always says "Its key stays in the Keychain"** (L1070-L1073).
- **`change` has one keyless rule but no keyless path** (`crates/am-app/src/accounts/change.rs`):
  - It already refuses a keyless Account's move to a remote endpoint (L234-L239, built by 2.14).
  - An address change says "the key stored under … will now be sent" (L257-L261).
  - A provider change or first confirmation always reads a new key (L311-L330), records `account_key_set` (about L525-L541) and `account_added key: stored` (about L545-L556).
  - The first-confirmation path assumes `key: "stored"` for the old values (L216-L229).
- **The egress client cannot tell a keyless Account.** `AccountStamp` (`crates/am-domain/src/net.rs` L124-L127) has no key state. `NetEgress::send` (`crates/am-net/src/lib.rs` L762-L818) reads the Keychain for any `AuthRef` whose Account the directory answers. A leftover entry under a keyless id (`remove` leaves the Keychain entry in place, AD-15 *Remove*) would be read and sent by any caller that attaches an `AuthRef`.
- **The writer's Undo/Restore guard ignores key state.** `refuse_account_move` (`crates/am-config/src/writer.rs` L737-L800) compares provider and `base_url` only (L765). An app Restore that adds or drops `secret_ref` passes it.
- **The loader's fix text says "or remove the line".** `crates/am-config/src/kinds.rs` L561-L575: for a `secret_ref` naming another entry, the fix is "write `secret_ref: <id>`, or remove the line". From this story on, removing the line makes the Account keyless. The same words are in `accounts.rs` L1264 and `change.rs` L176.

**What already holds, and stays:**
- **One loopback form.** `am_config::compatible_base_url` (`crates/am-config/src/base_url.rs` L41-L176) accepts `http://127.0.0.1:<port>[/path]` only, with an explicit port. It refuses `localhost`, `[::1]`, every other address spelling, and https to this Mac. `AccountEndpoint::loopback` (`accounts.rs` L313-L328) carries it.
- **No DNS for loopback.** am-net's `pinned` (`lib.rs` L449-L500) reaches `127.0.0.1` by `Reach::Loopback`, with no lookup, never the app's own ports and never a reserved local port for another Account.
- **Redirects.** A redirect is followed only for GET and re-checked by the same caller rule (L853-L913). Model calls are POST, so they are never redirected.
- **Every call passes the key-use check.** The directory answers an Account only while `key_use::check` passes (`crates/am-app/src/egress.rs` L118-L130). Only the reserved local Accounts skip it (AD-15).

**Approach (recommended; each point names its owner question):**
- **Add (Q1, Q2, Q3):** `agentic accounts add <id> --provider openai-compatible --base-url http://127.0.0.1:<port>[/path] --no-key`.
  - Every rule of the add stays: a real terminal, the CLI-class refusals, the typed id within 2 minutes, and the re-read under `user.lock`.
  - `--no-key` is refused in words, before any prompt, for every endpoint but loopback.
  - It is also refused at the reserved local ports and at a port one of the app's servers uses.
  - The sentence says the calls carry no credential and stay on this Mac.
  - No key prompt is shown and no Keychain call of any kind is made.
  - The file entry has no `secret_ref`. The `account_added` note records `key: none`.
- **Key use (Q3, Q4):**
  - The check compares the record's key state with the file's: `secret_ref: <id>` is a stored key, no `secret_ref` is `none`. Any disagreement refuses before any Keychain read.
  - `Confirmed` carries the key state, for `test` now and for 2.6's run path.
  - `AccountStamp` carries it to am-net, which refuses an `AuthRef` for a keyless Account before any Keychain read, and audits it.
- **Test (ticket):** a keyless Account is called with no `AuthRef` and makes no Keychain call. It still passes the check, the own-port refusal and the egress client's pin.
- **List and remove (ticket):**
  - `list` shows the Account as `keyless` and reads no Keychain entry for it.
  - `remove` of a keyless Account says it had no key and makes no Keychain call.
- **Change (Q5):** AD-15's keyless parts of `change`.
  - An address change of a keyless Account stays keyless and makes no Keychain call.
  - A provider change or first confirmation to a loopback endpoint may take `--no-key` instead of a new key. It makes no Keychain call and removes `secret_ref`.
  - A keyed first confirmation writes `secret_ref: <id>` when the file lacks it.
  - A key state changes only on the paths that would replace a key.
- **Writer guard (Q6):** an Undo or Restore that adds or removes an Account's `secret_ref` is refused, naming `agentic accounts change`, as 2.14 refuses a provider or address move.
- **Nothing else moves (Q7, Q8):**
  - 3.7's pricing stays decided by endpoint.
  - The reserved local Accounts stay as they are.
  - No discovery or probe of a keyless Account is added (R2: "no other address is probed").
  - The zero-key golden run (12.16) is not changed here.

## Boundaries & Constraints

**Always:**
- **AD-15's add rules, unchanged, for a keyless add:**
  - *Terminal:* stdin and stdout are both a real terminal.
  - *Open:* the CLI-class refusals, loopback-HTTP MCP servers included, and an outside change of the user file refusing.
  - *References:* reachable projects listed, unreachable ones named as not checked.
  - The typed id within 2 minutes.
  - The re-read under `user.lock` before the write.
  - History and the `account_added` record in one user-store transaction.
- **Keyless only where AD-15 allows it:** an `openai-compatible` Account whose `base_url` passes `am_config::compatible_base_url` with `loopback: true`, chosen explicitly before the sentence (Q2).
- **No Keychain call on a keyless path:** add, test, list (for that Account), remove, and the keyless paths of change. A recording `SecretStore` proves zero `get`, `set` and `delete` in every keyless test (Q3).
- **The key-use check runs for keyless Accounts** (AD-15: "pass the same check as any Account; only AD-31's reserved local Accounts skip it").
- **File and record agree, or nothing is called:** `secret_ref: <id>` with a stored-key record, no `secret_ref` with a `key: none` record. Any other pairing refuses every key use and every call, before any Keychain read (AD-15).
- **One loopback rule:** `am_config::compatible_base_url`. No new host test anywhere (AD-48: one type matches hosts; the endpoint rule is 2.3's).
- **Tests first:** every test in the Tests-first list is written and seen red, with its failure logged, before any `src/` change. Guards that are green today are named as guards.
- **No network and no real Keychain in tests:** FakeSecretStore, a recording store, FakeEgress, and am-e2e's `net_rig` seams. A canary key is used where a keyed Account sits beside a keyless one.
- **One squash commit.** Stop and report if the code nears about 4,000 lines (owner, 2026-11-24).

**Never:**
- **No placeholder.** No placeholder, empty or dummy key in the Keychain, the file, the user store, History or audit (Q3).
- **No keyless remote Account:** not https, not `anthropic` or `openai`, not `localhost`, `[::1]`, another `127.0.0.0/8` address or a LAN address. Not a reserved local port (1234, 11434), and not a port an app server uses (Q1).
- **No keyless by an empty answer** at the key prompt (AD-15).
- **No key-state change** on an address change, an Undo, a Restore, a Reload or a migrate (Q5, Q6).
- **No discovery, health probe or model listing** of a keyless Account (R2).
- **No change to** `crates/am-app/src/prices.rs` or 3.7's local rule, the reserved local Accounts, `migrate`'s records, or any AD, ticket or register row. Sofia and the architect make those after the owner's answers.
- **No new crate or dependency.**
- **No department vocabulary in `crates/`.**

## I/O & Edge-Case Matrix

`LOOP` is `http://127.0.0.1:8080/v1`. "Nothing written" means the user file, `user.db` and the Keychain are byte-identical, and the recording store saw no call.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Keyless add | Terminal; `add llama --provider openai-compatible --base-url LOOP --no-key`; `llama` typed | The sentence names the Account, the provider and `LOOP`, says the calls "stay on this Mac, on port 8080" and carry no credential, and says no key is read or stored. No key prompt. The entry has `id`, `provider`, `base_url` and no `secret_ref`. One History entry. `config.widened` `account_added` with `scheme http`, `host 127.0.0.1`, `port 8080`, `key none`, `confirmation typed_id`. Zero Keychain calls. The output says the Account is keyless. | — |
| 2 | Keyless, remote | `--no-key` with `--base-url https://models.example/v1` | Refused before any prompt: a remote endpoint always needs a key; leave out `--no-key`. Nothing written; nothing shown on the terminal. | Worded refusal |
| 3 | Keyless, fixed provider | `--no-key` with `--provider anthropic` or `openai` | Refused the same way, naming the provider's fixed endpoint | Worded refusal |
| 4 | Keyless, other spellings of this Mac | `--no-key` with `http://localhost:8080`, `http://[::1]:8080`, `http://127.0.0.2:8080`, `http://127.1:8080`, `http://127.0.0.1/v1` (no port), `https://127.0.0.1:8080` | Each is refused by 2.3's endpoint rule, in its existing words, before any prompt. Nothing written. | Worded refusal |
| 5 | Reserved or own port (Q1) | `--base-url http://127.0.0.1:1234/v1` or `:11434`, with or without `--no-key`; or the port an `agentic serve` records | Refused before any prompt, naming the reserved Account (`local-lmstudio`, `local-ollama`) or the app's own server. am-net already refuses these calls; the add now says so first. | Worded refusal |
| 6 | Keyless, cancelled | Row 1 with a wrong id, no answer in 2 minutes, or Ctrl-C at the id | Nothing written; zero Keychain calls; exit 1, or 130 after Ctrl-C | Worded refusal |
| 7 | Keyless, not a terminal | Row 1 from a pipe | Refused by the terminal rule, before anything is read | Worded refusal |
| 8 | Keyless, other add refusals | A reserved or existing id; the user file changed outside the app; a reachable project with a stdio or loopback-HTTP MCP server | Each refused exactly as for a keyed add | Worded refusal |
| 9 | Empty key, keyed add | A loopback add without `--no-key`, and an empty line at the key prompt | Refused ("no key was typed"). The Account is never added keyless. *(guard)* | Worded refusal |
| 10 | Keyed loopback add | A loopback add without `--no-key`, with a key | As 2.3 built it: key stored, `secret_ref`, `key stored`. *(guard)* | — |
| 11 | Test, keyless | `test llama --model m` from any stdin, after row 1 | One POST to `LOOP/chat/completions` with no `AuthRef`. On the wire (real `NetEgress` over `net_rig`) no credential header and only `127.0.0.1:8080`. Zero Keychain calls. The directory pins `http 127.0.0.1 8080`. The output names the endpoint and says no credential was sent. | — |
| 12 | Wire rules, keyless (am-net) | A keyless Account's request: no `AuthRef`; then a GET answered by a redirect to `127.0.0.1:9090` and to `https://elsewhere.example` | The first reaches `127.0.0.1:8080` with no credential and no Keychain read. Each redirect is refused and audited `egress.blocked`. A POST is never redirected. *(The rules exist; the test proves them for a keyless stamp.)* | Refused, audited |
| 13 | Credential for a keyless Account (Q4) | A request for the keyless Account carrying `AuthRef { account: llama }`, with a canary entry under `llama` in the store | Refused before any Keychain read: the Account has no key, so a request for it carries no credential. `egress.blocked` audited with that reason. The canary is on no hop. | Refused, audited |
| 14 | Keyed, `secret_ref` dropped by an app write | A confirmed keyed Account; an app write (History entry, no confirmation) removes its `secret_ref`, so the hash check passes | `test` and the directory refuse before any Keychain read: confirmed with a key, but the entry has no `secret_ref`; put the line back, or remove and add it with `--no-key` (on this Mac only). Nothing sent. | Worded refusal |
| 15 | Keyed, `secret_ref` dropped by hand | The ticket's case: a hand edit removes `secret_ref` from a keyed remote Account | Refused before any Keychain read (an outside change of the user file, as 2.12 built). *(guard: ticket verify)* | Worded refusal |
| 16 | Keyless, `secret_ref` added by an app write | Row 1's Account; an app write adds `secret_ref: llama` | Refused before any Keychain read: confirmed with no key, but the entry names a Keychain entry; remove the line, or confirm it with a key (`agentic accounts change llama`) | Worded refusal |
| 17 | List | Row 1 plus a keyed Account | `llama  openai-compatible  127.0.0.1:8080  keyless (no key; its calls carry no credential)`. No Keychain call for `llama`; the keyed row is masked as today. Rows 14 and 16 show `unconfirmed` with their fix. | — |
| 18 | Remove, keyless | `remove llama` on a terminal | `account_removed`, History, Undo as today. The words say it had no key and no Keychain entry was read or changed. Zero Keychain calls. | — |
| 19 | Change, keyless address (Q5) | `change llama --base-url http://127.0.0.1:9090/v1`; id and `yes` typed | The sentence says it stays keyless and its calls carry no credential. `account_changed` with `key none`, `confirmation typed_id_yes`. Zero Keychain calls. `test` then reaches `:9090` only, with no credential. | — |
| 20 | Change, keyless to remote | `change llama --base-url https://models.example/v1` | Refused before any prompt, as 2.14 built (`change.rs` L234-L239), now reachable. *(guard on a real keyless Account)* | Worded refusal |
| 21 | Change, keyless confirmation (Q5) | An Account with no live record (listed unconfirmed by `migrate`, or a recovered store) at a loopback endpoint; `change <id> --no-key`; id and `yes` | No key prompt and no Keychain call. One transaction: `account_removed` (`reason provider_change`, no `key_for`), then `account_added` (`key none`, `replaces`, `confirmation typed_id_yes_new_key` as AD-15 *Record* reads) with the file write, which removes `secret_ref`. `test` then sends with no credential. | — |
| 22 | Change, `--no-key` refused | `--no-key` on an address change of a keyed Account, or to a remote endpoint | Refused before any prompt. An address change keeps the key state. To drop a key, remove the Account and add it with `--no-key`. | Worded refusal |
| 23 | Change, keyed first confirmation of an entry without `secret_ref` | An unconfirmed Account whose entry has no `secret_ref`; `change <id>` with a new key | After the key is stored, the file write adds `secret_ref: <id>` with `account_added key stored`, so file and record agree. `test` then sends. | — |
| 24 | Undo or Restore of a key state (Q6) | A History entry whose Undo or Restore would add or remove an Account's `secret_ref` | Refused by the writer, naming `agentic accounts change` (as 2.14 refuses a move) | Writer refusal |
| 25 | Loader fix text | `secret_ref: other` on a keyed Account | The validate error's fix says `write secret_ref: <id>`. It no longer offers "or remove the line" (which would make the Account keyless). The `test` and `change` refusals say the same. | Validate error |
| 26 | Migrate and pricing | Any `migrate`; a run on a keyless Account | Unchanged. Migrated entries keep `secret_ref` and `key stored`. A step on a keyless Account prices as local (0) by 3.7's endpoint rule; `prices.rs` is not touched. *(covered by existing tests)* | — |
| 27 | No key anywhere | Rows 1, 11, 18, 19, 21 beside a keyed canary Account | The canary is in no output, file, store row, History or audit entry. No `secret_ref`, key, key prefix or key hash is written for the keyless Account. | — |

</intent-contract>

## Code Map

- **`crates/am-config`:**
  - `src/user_accounts.rs` L63-L68 `key_ref`: returns the id only when `secret_ref` names it. A new `key_state(account) -> KeyState { Stored, None }` reads `secret_ref`'s presence. A `secret_ref` naming another entry stays refused through `key_ref` returning `None`, as today. It stays the one predicate every key reader uses (AD-17).
  - `src/account_edit.rs`:
    - L284-L296 `account_item`, L301 `add_account_edit` and L381 `new_accounts_file` take the key state and write no `secret_ref` for `None`.
    - L347-L378 `change_account_edit` takes the key state to write. It sets `secret_ref: <id>` or removes the key when the file disagrees. The address path passes the current state.
  - `src/kinds.rs` L561-L575: the `secret_ref` fix text (row 25). This collides with 3.7's `kinds.rs` edits, so it waits for #48.
  - `src/writer.rs` L737-L800 `refuse_account_move`: the destination tuple (L765) gains the key state, and a key-state change is refused like a move (row 24).
- **`crates/am-domain`:** `src/net.rs` L124-L127 `AccountStamp` gains `keyed: bool`. The default `account_stamp` (L151-L156) answers `true`, the safe side for a directory that keeps no key state. Four literal sites in all (`net.rs`, `am-app/src/egress.rs`).
- **`crates/am-net`:** `src/lib.rs` `send`. Before the key loop (L762-L818), an `AuthRef` whose Account's stamp says `keyed: false` is refused and audited (`egress.blocked`, reason: the Account has no key, so its requests carry no credential), before any `SecretStore` call (row 13; Q4).
- **`crates/am-app`:**
  - `src/accounts.rs`:
    - `parse` (L155-L244) and `USAGE` (L106) gain `--no-key` for `add` and `change`.
    - `add` (L635-L962):
      - the `--no-key` refusals (rows 2-5) right after `endpoint_of`;
      - the reserved-port and own-port refusal for every loopback add (row 5; Q1);
      - the keyless sentence;
      - no Keychain call;
      - the record's `key none`;
      - the item without `secret_ref`;
      - the keyless output words.
    - `list` (L1129-L1225): no Keychain read and `keyless` words for a keyless entry. The confirmed column compares the key state (row 17).
    - `test` (L1304-L1442): for a keyless `Confirmed`, no Keychain loop and no `with_auth`. The output says no credential was sent.
    - `remove` (L1070-L1073): the keyless words.
    - `refused_words` (L1241-L1282): the key-state refusals and the `secret_ref` fix text.
    - `AccountEndpoint` is unchanged.
  - `src/key_use.rs`:
    - `Destination::names` (L279-L285) takes the key state to compare.
    - `once` (L459-L560):
      - the `secret_ref`-other refusal stays;
      - the record-against-file key-state comparison is added after the hash check;
      - two new `Refused` kinds with words (rows 14, 16).
    - `Confirmed` (L134-L145) gains `key: KeyState`, which 2.6 reads.
  - `src/egress.rs`: `answer` (L118-L130) and `account_stamp` (L144-L152) carry `keyed` from `Confirmed`.
  - `src/accounts/change.rs`:
    - parse `--no-key`;
    - `Old` takes the key state from the live record, or from the file for a first confirmation (L216-L229);
    - the keyless address sentence (L257-L261);
    - the `--no-key` path on remove-plus-add, with no key prompt (L311-L330), no Keychain call, no `account_key_set`, and both notes in the write's transaction;
    - the refusals of row 22;
    - the keyed first confirmation's `secret_ref` (row 23);
    - the `secret_ref` fix text (L176).
  - `src/accounts/migrate.rs`: its `add_account_edit` and `new_accounts_file` calls pass `Stored` (about L1128-L1145). No behaviour change.
- **`crates/am-e2e`:** `tests/trust/rule_09_keyless.rs` (new), and its `[[test]]` target in `Cargo.toml` (`trust_targets.rs` requires one).
- **Docs:** `docs/config-schema.md` L57, the `secret_ref` row: required for a keyed model Account; absent only for a keyless `openai-compatible` Account at `http://127.0.0.1:<port>`.
- **Existing tests that change (supersession, mechanical):**
  - The doc comments of `accounts_cli.rs` L2348-L2352 and L2903-L2905 ("there is no keyless Account"). Their assertions stand: a keyed loopback Account still needs its key.
  - Any test asserting the old "or remove the line" fix text.
  - Any test that builds an `AccountStamp` literal.
  - The build lists every test that writes a confirmed keyed model Account without `secret_ref` through an app write (none found in the sample read: `status_spend.rs` L794, `accounts_migrate.rs`, `user_accounts.rs`). Each one is named in the red run.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:**
  - the owner answers Q1–Q8, recorded verbatim under Design Notes;
  - PR #48 (3.7) merged, and this plan's baseline set to that main;
  - the plan committed on `story/2-10`;
  - any AD or ticket change the answers need (Q5, Q6, Q7(b)) made by the architect or Sofia first, as for 2.13.
- [ ] **Tests first (no `src/` change):** the Tests-first list, the `[[test]]` target and the superseded doc comments. Log the red run with each test's failure.
- [ ] **am-domain, am-net:** `AccountStamp.keyed`, and the keyless `AuthRef` refusal.
- [ ] **am-config:** `key_ref` and `key_state`; the item and edit builders; the writer guard; the fix text.
- [ ] **am-app:**
  - `key_use` (key state, refusals, `Confirmed.key`);
  - `egress`;
  - `add`, `list`, `test` and `remove`;
  - `change`;
  - `migrate`'s call sites.
- [ ] **Docs:** `docs/config-schema.md`.
- [ ] **Register (at triage):** close DR-079; add the rows under "Deferred".
- [ ] **Size:** one squash commit. If the code nears about 4,000 lines, stop and report.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the files, user store, Keychain calls, wire hops, audit entries and words match the row.
- Given the ticket's verify:
  - on a terminal, a compatible Account at `LOOP` added with no key (the owner's explicit choice) writes no `secret_ref`, stores nothing in the Keychain, records `key: none`, and names the endpoint as on this Mac with no credential;
  - `accounts test` sends a request with no `Authorization` header to that port only;
  - the same choice on an `https://` endpoint is refused in words with nothing written;
  - a hand edit that removes `secret_ref` from a keyed remote Account is refused by the confirmed check;
  - `list` shows the Account as keyless.
- Given the workspace, when `just check` runs, then it is green, `rule_09_*` and the golden so-far run included. Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-25, planner: draft for the owner's security stop. No tests or code until Q1–Q8 are answered.
  - Risk is `high` against the ticket's `medium` (manager instruction): it touches the key-use check, the egress client's key path, the writer guard and `change`. The ticket's `risk` field is not edited.
- 2026-11-25, owner: approved with the recommended answers; Q1 adds the register row and the "local model server not answering" error; Q2 is 127.0.0.1 only; Q5 is built with terminal-only typed-name confirmation; Q6 is widening both ways (replan). The baseline is set once #48 (3.7) merges.

## Design Notes

### Owner answers (2026-11-25)

**Owner, verbatim:** "Approve 2.10 with the recommended answers to Q1–Q8. Q1: record the port risk in the register, and show a clear "local model server not answering" error. Q2: 127.0.0.1 only. Q5: yes, with terminal-only and typed-name confirmation. Q6: both directions are widening. Merge #48 on green after I check the price files."

**How the plan takes them:**
- **Q1–Q8:** the recommended answers, as written below, with these additions:
  - **Q1:** the register gets a row for the port-squatting risk. While the local server is stopped, another local process can take its port and receive prompts. When a keyless Account's endpoint refuses the connection, times out or returns nothing, the error says plainly that the local model server is not answering and names the Account and the address. It must never read as a key problem or a cloud error. A test pins the words.
  - **Q2:** `--no-key` accepts only `http://127.0.0.1:<port>`. `localhost`, `[::1]` and other `127/8` addresses are refused in words before any prompt.
  - **Q5:** keyless `change` (an address change stays keyless; `--no-key` on a provider change or a first confirmation; the `secret_ref` fix-up) is built here. It is terminal-only, with typed-name confirmation, the same as `accounts add`. Tests cover the non-terminal refusal and a wrong typed name.
  - **Q6:** a change of key state is widening in both directions, keyed to keyless and keyless to keyed. In this story, the writer refuses an Undo or Restore that flips it. A documents-only replan records it in AD-15's list and 4.17's ticket; that replan merges before this story's tests start.
- **Model:** a security story, so tests, build and reviews run on Opus (owner rule, 2026-11-25).

### Owner questions (security)

Each question gives the options, the planner's pick and why. Anything that changes an AD or a ticket, or adds a dependency, is marked **owner decision**. The planner decided none of them. No question needs a new dependency.

**Q1. What a keyless Account may reach, and how loopback is enforced.**
- **Host forms.**
  - (a) Only 2.3's form, `http://127.0.0.1:<port>[/path]`, read by `am_config::compatible_base_url` (`loopback: true`). Refused: `localhost` and `*.localhost`, `[::1]` and every IPv6 form, every other `127.0.0.0/8` address and spelling (`127.1`, decimal, hex), https to this Mac, and a URL with userinfo, query or fragment.
  - (b) Also `[::1]` and `localhost`. **Owner decision:** AD-12 and AD-15 name `http://127.0.0.1:<port>` only, and `localhost` needs a DNS lookup that a keyless Account otherwise never makes.
  - (c) Also LAN or private addresses. **Owner decision**, against AD-12 ("never a private or LAN range").
- **Ports.** Any explicit port 1–65535, except:
  - (i) 1234 and 11434, the reserved local Accounts' ports, which am-net already refuses to any other Account at every call (`lib.rs` L473-L484);
  - (ii) a port one of the app's servers uses (AD-12: "never the app's own port"), which `test`, `change` and am-net already refuse.

  The pick refuses both at `add` too, in words, before any prompt, for keyed and keyless loopback adds alike: it is one add path, and today an add there creates an Account no call can ever reach. A server started later on such a port is still refused at every call, as today.
- **Facts that hold under (a), with no change:**
  - **Redirects:** model calls are POST and never redirected. A GET redirect is re-checked against the exact pinned endpoint, so it can never leave `127.0.0.1:<port>`.
  - **DNS rebinding:** none is possible. `127.0.0.1` is an IP literal reached by `Reach::Loopback`, with no lookup. A DNS name that resolves to loopback is a remote https endpoint (which needs a key) and is refused by am-net's address check at call time.
- **Residual (to accept):** while the owner's server is stopped, any local process (another macOS user's included) can bind that port and receive the prompts sent to it. A keyless Account sends no credential, so only prompt content is exposed. This is the same residual as the reserved local Accounts and keyed loopback Accounts have today.
- **Pick: (a), with the two port refusals at add, and the residual accepted.** It is AD-12 and AD-15 as written. Every other form needs an AD change and a DNS lookup. The port refusals only move an existing call-time refusal earlier.

**Q2. How the owner chooses "no key".** The ticket says "the owner's explicit choice at the prompt". AD-15 says "chosen explicitly before the add's sentence, never by an empty answer at the key prompt".
- (a) A `--no-key` flag on `add` (and on `change`'s new-key paths, Q5). It is refused before any prompt for any endpoint but loopback.
- (b) A question before the sentence on every `openai-compatible` add: type `key` or `none`. Anything else, an empty line or a timeout writes nothing.
- (c) A word typed at the key prompt. This is against AD-15: it comes after the sentence and is one keystroke from an empty answer.
- **Pick: (a).**
  - The choice is in the command the owner types, at the shell prompt. That is how this plan reads "at the prompt".
  - It is refused before anything is shown, which is the ticket's https case.
  - It leaves the keyed add's prompts byte-for-byte as 2.3, 2.12 and 2.14 built and tested them.
  - (b) adds a question to every compatible add and changes about eight built tests.
- **Owner decision only if** "at the prompt" meant an interactive question. Then (b), with the same refusals.

**Q3. Whether any key or placeholder is ever stored or read.**
- (a) Nothing, anywhere:
  - no Keychain `get`, `set` or `delete` on any keyless path;
  - no `secret_ref` in the file;
  - `key: none` in the record.

  A Keychain entry that already exists under the same id (an earlier keyed Account of that id, removed) is not read, used, deleted or named. The sentence says only that no key is read or stored.
- (b) A placeholder entry (an empty or fixed value), so the keyed code paths run unchanged. This is the fake key the owner asked to be rid of; it would be read and sent like a key.
- (c) A presence check (`get`, value dropped), so the sentence can warn of a leftover entry. A `get` materialises the secret in this process (DR-087 records the same trade for keyed `test`). Nothing is gained, because (Q4) a leftover entry can never be sent for a keyless Account.
- **Pick: (a).** It is AD-15 ("The add reads no key, the file entry has no `secret_ref`") and the ticket ("stores nothing in the Keychain"). It is testable as zero calls on a recording store.

**Q4. Where "no credential" is enforced.**
- (a) At the call sites only: `test` builds the adapter without an `AuthRef`, and 2.6's run path must do the same.
- (b) (a), plus the key-use check comparing file and record key state (AD-15, rows 14 and 16), plus am-net:
  - `AccountStamp` carries `keyed`;
  - `NetEgress::send` refuses an `AuthRef` for a keyless Account before any `SecretStore` call, and audits it.

  This is an am-domain port change (one field, four literal sites) but no AD change.
- **Pick: (b).**
  - `remove` leaves Keychain entries in place, so an id once keyed can be re-added keyless with a real key still under it.
  - Any caller that attaches an `AuthRef` by habit (2.6 will build providers for runs) would read and send that key to a server the owner said takes none.
  - The egress client is AD-12's one choke point, and AD-15 says every call (not only `test`) sends no credential and reads no Keychain entry.

**Q5. `change` for keyless Accounts.** The ticket says nothing of `change`. AD-15 *Change* and *Keyless loopback Account* (L222) assign it to this story: "A keyless Account (from epic 2 story 10) whose new endpoint is loopback may instead be confirmed keyless, chosen explicitly as at its add, and touches no keychain entry".
- (a) Build AD-15's keyless `change` here:
  - **Address change of a keyless Account:** stays keyless, with no Keychain call; the sentence says no credential is sent; `account_changed key none`.
  - **Provider change or first confirmation to a loopback endpoint:** may take `--no-key` instead of a new key. No key prompt, no Keychain call, no `account_key_set`, and `secret_ref` is removed from the file. The two notes go in the write's one transaction, because no Keychain step sits between them.
  - **Keyed first confirmation:** writes `secret_ref: <id>` when the file lacks it (row 23), or file and record would disagree.
  - **`--no-key` on an address change of a keyed Account** is refused: a key state changes only where a key would be replaced. To drop a key, remove the Account and add it with `--no-key`.
- (b) Defer `change` to a follow-up story (**owner decision:** a new ticket). Until then, an Account `migrate` left unconfirmed at a loopback endpoint (a 2.3-era llama.cpp Account with a typed dummy key) can be confirmed only by typing a key again, which is the fake key this story removes. And a keyless address change would say "the key stored … will now be sent".
- **Pick: (a).** The AD binds it to this story, and the ticket is silent rather than contrary. About 130 lines of code. **Owner decision:** whether the ticket's silence stands as scope (b), or Sofia restates entry 10 to name `change` (a).
- **Flag within (a):** AD-15 *Record* keeps `confirmation: typed_id_yes_new_key` with `key: none` for a keyless first confirmation, although no key is typed. The plan follows it literally (the check never reads `confirmation`). A clearer kind such as `typed_id_yes_no_key` is an AD amendment (**owner decision**, not recommended now).

**Q6. Widening classification (AD-15, epic 4 story 17).**
- **No change for the add itself:** a keyless add is "adding an Account (loopback, …) to the user-level list". It is widening, audited `config.widened` `account_added`, as 4.17's "any Account added" already covers.
- **New: a change of key state** (an Account's `secret_ref` added or removed). AD-15's widening list does not name it, and 4.17's exhaustive match must classify the field.
  - (a) Widening in both directions, like a provider or `base_url` change: it needs a confirmation that records the new key state.
    - **Now:** 2.14's writer guard refuses an Undo or Restore that does it (row 24).
    - **Later:** the architect adds "changing an Account's key state" to AD-15's list, and Sofia restates 4.17's table to cover it.
  - (b) Only adding a key is widening; removing one is a narrowing. The key-use check still refuses the mismatch until a confirmation records it, so (b) changes only how a Reload or the UI labels it.
  - (c) Leave it unclassified (4.17 would class it not widening by default).
- **Pick: (a).**
  - Adding a key starts a credential flowing to an endpoint.
  - Removing one breaks agreement with the record, which only a confirmation mends.
  - One rule is simpler for the classifier, the UI (5.5) and Reload (4.18).
  - The writer guard is am-config code this story can add without any AD change.
- **Owner decision:** the AD-15 amendment and the 4.17 restatement.

**Q7. The zero-key golden run and RULE-20 (open by default).**
- **Facts:**
  - A keyless Account is a second key-free model path, beside the reserved `local-lmstudio` and `local-ollama`. It serves a llama.cpp- or vLLM-style server on any other port with an empty Keychain.
  - It does not change 12.16's zero-key golden run, which uses the reserved Accounts.
  - 2.6's ticket already treats "a keyless loopback Account" as not cloud, so such a server is usable before 4.18. A keyed loopback Account is not.
- (a) This story proves its own zero-Keychain claims: rows 1, 11, 18, 19 and 21 assert zero calls on a recording store. No ticket change.
- (b) Also restate 12.16 to run F1 once on a keyless Account. **Owner decision:** a ticket change, made by Sofia.
- **Pick: (a).** (b) is cheap insurance for RULE-20's threshold. Offer it to Sofia if the owner wants the golden run to cover owner-added local servers.

**Q8. The reserved local Accounts, and 3.7's local pricing rule.**
- **Reserved Accounts.** A keyless Account is not a reserved one:
  - it is owner-added and confirmed;
  - it passes the key-use check;
  - it is never discovered, health-probed or listed with models (R2);
  - it cannot use ports 1234 or 11434 (Q1);
  - the reserved ids can never be added.
- **Pricing.** 3.7's `on_this_mac` (`prices.rs` L55-L66, owner Q8 and review M5: "where the server is decides, never the provider's name") already prices any Account whose `base_url` is `http://127.0.0.1:<port>` at 0, keyed or keyless. A keyless Account is always loopback, so it is always local.
  - (a) No change: this story does not touch `prices.rs`. `Confirmed.key` gives 2.6 the key state, and DR-185 already tells 2.6 to decide locality from `endpoint_of`'s loopback flag.
  - (b) Make "keyless" itself mean "local" for pricing. This is against owner Q8: the endpoint decides.
- **Pick: (a).**
- **Flag for 2.6's plan (not decided here):** 2.6's ticket counts a *keyed* loopback Account as cloud for its pre-4.18 refusal, while 3.7 prices it local. Both readings are consistent with their own purposes (a key reaching a server, against money), but 2.6's plan should say so, or Sofia should align the wording.

### Design decisions within the ADs (planner, under the standing rules)

- **`key_ref` semantics:** the id only when `secret_ref` names it.
  - A missing `secret_ref` has no key entry (keyless).
  - Every existing caller keeps its meaning: `other_entry`, `key_use`, and `change`'s other-entry refusal.
  - Both am-config tests (`user_accounts.rs` L183-L184, `templates.rs` L330) stay green.
- **No new validate rule.** `agentic validate` cannot read the user store, so it cannot tell a keyless Account from a keyed one that lost its line. The key-use check refuses with words, and `list` shows it. Only the existing fix text changes (row 25).
- **The keyless first confirmation writes its `account_removed` without `key_for`.** `key_for` says what the Keychain entry was replaced for, and a keyless confirmation replaces nothing. Its two notes commit with the file write in one transaction.
- **`--no-key` on a keyless Account's address change** is accepted and changes nothing. On a keyed one it is refused (row 22).
- **Test file:** a new `crates/am-app/tests/accounts_keyless.rs` with its own small kit (as `accounts_change.rs` has), using am-e2e's `net_rig` for the wire. This keeps the 3,800-line `accounts_cli.rs` untouched except for two doc comments.
- **Words:** each refusal names the fix:
  - put `secret_ref: <id>` back;
  - remove the line;
  - `remove` then `add --no-key`;
  - `agentic accounts change <id>`.

  No refusal repeats a value from the file (`hide_text` rules stand).

### Conflicts and readings found

1. **Ticket against AD-15, the choice.** "at the prompt" against "before the add's sentence, never by an empty answer at the key prompt": Q2.
2. **The ticket is silent on `change`; AD-15 assigns keyless `change` to this story:** Q5.
3. **The ticket's verify item "a hand edit that removes `secret_ref` … is refused" is already refused today,** by 2.12's hash check (an outside change). It cannot be seen red. It stays as a guard (row 15), and its red twin is the app-write case (row 14), which today reads and sends the key.
4. **The ticket says "story 3's confirmed check".** Since 2.12 it is the user-level Key-use check (`key_use.rs`). Wording only.
5. **Risk:** the ticket says `medium`, this plan `high`. The ticket is not edited.
6. **AD-15 *Record*** names `typed_id_yes_new_key` for a keyless confirmation: followed literally (Q5 flag).
7. **`secret_ref` absence changes meaning.** AD-17 and `docs/config-schema.md` call `secret_ref` optional, and `key_ref` reads its absence as the id's entry. From this story on, absence means keyless (AD-15). There is no AD conflict, because AD-17 does not define absence. The loader's "or remove the line" fix becomes wrong and is changed (row 25).
8. **Cross-story:** 2.6's "keyed loopback is cloud" against 3.7's "loopback is local" (Q8 flag).
9. **Later stories:** `fixtures/accounts/sources.yaml` holds a keyless `searxng-remote` Account at `http://127.0.0.1:8888`. AD-15's keyless rule names only `openai-compatible`, and from this story "every call" refuses a `secret_ref` that disagrees with the key state. Search Accounts are not called before epics 9 and 12, whose plans must settle a keyless search Account (register row below).

### Deferred (to add to the register at triage)

- **Residual (Q1):** a local process can bind a keyless Account's port while its server is stopped and receive prompts. Same as the reserved Accounts. Target `none`, accepted if the owner accepts Q1.
- **Leftover Keychain entry (Q3):** a key left under an id that is now keyless is never read, deleted or named. Target `none`.
- **Owner question (Q6):** AD-15's widening list and 4.17's table name "changing an Account's key state". Target 4.17, once the architect has amended AD-15.
- **Owner question (Q7(b)):** 12.16 to cover a keyless Account. Target `owner`.
- **Deferred:** whether a search or crawl Account (`searxng-remote` above) may be keyless, and its key-state rule. Target: the epic 12 story that first calls a search Account (Sofia names it).
- **Deferred:** `agentic validate` does not flag a remote model Account with no `secret_ref`; the key-use check refuses it. Target `none` unless the owner wants it.

### Register entries folded in

- **DR-079** (2.3: "Keyless compatible Accounts are not built", Target 2.10): closed by this story.
- **Read and unaffected, kept as they are:**
  - **DR-105:** `names_this_mac` misses some forms. Keyless rests on `compatible_base_url`'s literal `127.0.0.1`, not on `names_this_mac`.
  - **DR-087 and DR-081:** the presence read, and the existence-only Keychain comparison. A keyless path makes no Keychain call.
  - **DR-091:** the window between the last stamp check and the send. No key travels on a keyless call.
  - **DR-185:** 2.6 decides locality from `endpoint_of`; this story adds `Confirmed.key` for it and does not touch `prices.rs`.
  - **DR-093 and DR-100:** 4.18's destination record is provider, scheme, host and port; key state is not part of it (Q6 covers the classifier).

### Size (owner: code and test lines separately; stop at about 4,000 lines of code)

| Area | Code (src) | Tests |
|---|---|---|
| am-domain `AccountStamp`, am-net keyless refusal | ~45 | ~230 (`rule_09_keyless.rs`) |
| am-config `key_ref`/`key_state`, builders, writer guard, fix text | ~70 | ~150 |
| am-app `key_use`, `egress` | ~85 | (in the files below) |
| am-app `add`, `list`, `test`, `remove`, usage | ~200 | ~900 (`accounts_keyless.rs`, kit included) |
| am-app `change` (Q5(a)) | ~130 | ~400 |
| `migrate` call sites, docs | ~5 (+15 docs) | ~20 supersession |
| **Total** | **~535 (range 450–700)** | **~1,700 (range 1,400–2,000)** |

Without Q5(a): about 400 lines of code and 1,300 of tests. Either way it is well under the 4,000-line stop.

### Tests-first list

Each test is written before any `src/` change and run. Its failure is logged in the red run.

| File | Test | Row | Why red today |
|---|---|---|---|
| `crates/am-app/tests/accounts_keyless.rs` | `a_keyless_add_on_this_mac_writes_no_secret_ref_and_records_key_none` | 1, 27 | `--no-key` is an unknown option (exit 2) |
| same | `a_keyless_add_is_refused_for_a_remote_endpoint_before_any_prompt` | 2, 3 | exit 2 usage, not the worded refusal |
| same | `a_keyless_add_refuses_every_other_spelling_of_this_mac` | 4 | exit 2 usage before the endpoint rule |
| same | `an_add_on_a_reserved_local_port_or_the_apps_own_port_is_refused` | 5 | a keyed add at `:1234` and at a recorded serve port succeeds today |
| same | `a_keyless_add_cancelled_or_mistyped_writes_nothing` | 6 | unknown option |
| same | `a_keyless_add_is_refused_from_a_pipe_and_while_an_owner_child_is_enabled` | 7, 8 | parse fails first (exit 2, not the terminal or child words) |
| same | `an_empty_key_never_makes_an_account_keyless` | 9 | *guard, green today* (kept: AD-15's "never by an empty answer") |
| same | `test_of_a_keyless_account_sends_no_credential_and_reads_no_key` | 11, 27 | the keyless setup add fails |
| same | `a_keyed_account_whose_secret_ref_an_app_write_removed_is_refused` | 14 | today the check passes, the key is read and the call is sent |
| same | `a_hand_edit_removing_secret_ref_from_a_keyed_remote_account_is_refused` | 15 | *guard, green today* (the ticket's verify, refused as an outside change) |
| same | `a_keyless_account_given_a_secret_ref_by_an_app_write_is_refused` | 16 | the keyless setup add fails |
| same | `list_shows_a_keyless_account_as_keyless_and_reads_no_key_for_it` | 17 | the keyless setup add fails |
| same | `removing_a_keyless_account_makes_no_keychain_call_and_says_so` | 18 | the keyless setup add fails |
| same | `the_directory_answers_a_keyless_account_with_no_key` | 13 (am-app side) | does not compile: `AccountStamp::keyed` and `Confirmed::key` do not exist |
| same | `a_keyless_address_change_stays_keyless_and_makes_no_keychain_call` | 19 | the keyless setup add fails |
| same | `a_keyless_account_never_moves_to_a_remote_endpoint` | 20 | the keyless setup add fails (the rule exists) |
| same | `a_first_confirmation_with_no_key_confirms_a_loopback_account_keyless` | 21 | `--no-key` is unknown to `change` |
| same | `no_key_is_refused_on_a_keyed_address_change_and_for_a_remote_endpoint` | 22 | unknown option |
| same | `a_keyed_first_confirmation_writes_the_secret_ref_the_file_lacked` | 23 | today the file keeps no `secret_ref` after the confirmation |
| `crates/am-config/tests/writer.rs` (or `account_edit.rs`) | `an_undo_or_restore_that_adds_or_removes_a_secret_ref_is_refused` | 24 | today the guard compares provider and `base_url` only |
| `crates/am-config/tests/user_accounts.rs` | `key_ref_names_the_id_only_when_secret_ref_names_it` | — | today an absent `secret_ref` gives the id |
| same | `the_secret_ref_fix_never_tells_a_keyed_account_to_drop_the_line` | 25 | the fix says "or remove the line" |
| `crates/am-config/tests/account_edit.rs` | `a_keyless_item_has_no_secret_ref_and_a_change_sets_the_key_state` | 1, 21, 23 | does not compile: the builders take no key state |
| `crates/am-e2e/tests/trust/rule_09_keyless.rs` | `a_keyless_accounts_request_carries_no_credential_and_reads_no_key` | 12 | does not compile (`AccountStamp::keyed`). Its behaviour half holds today for a request with no `AuthRef`, and it is the wire proof of the ticket's no-`Authorization` item. |
| same | `a_credential_for_a_keyless_account_is_refused_before_any_keychain_read` | 13 | today the canary is read and sent on the hop |
| same | `a_keyless_accounts_redirect_off_its_endpoint_is_refused_and_audited` | 12 | does not compile. The rule holds; this proves it for a keyless stamp. |

## Verification

- **Iterate:**
  - `cargo nextest run -p am-app --test accounts_keyless`;
  - `cargo nextest run -p am-app --test accounts_cli --test accounts_change --test accounts_migrate --test user_accounts --test status_spend`;
  - `cargo nextest run -p am-config`;
  - `cargo nextest run -p am-net`;
  - `cargo nextest run -p am-e2e --test rule_09_keyless --test rule_09_callers --test trust_targets`.
- **Gate:** `just check` (fmt, clippy `-D warnings`, nextest, `agentic validate` over `templates/` and `fixtures/`, the register check, the golden so-far run).
- **Leak:** row 27's canary scan over every output, file, store row, History and audit entry of the keyless tests.
- **Not run:** no live model server and no real Keychain. The owner may add a llama.cpp server once, after merge, with `--no-key` and `accounts test` (optional; not a gate).

## Implementation Notes

*(Filled in by the build: the owner's answers verbatim, the red run, the build, the review triage and the Auto Run Result.)*
