---
title: 'Keyless Accounts on this Mac'
type: feature
ticket: 10
created: '2026-11-25'
status: built
baseline_revision: '7364da7'
after: [3, 12, 14]
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [correctness, adversarial-security]
review_loop_iteration: 2
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
| 14 | Keyed, `secret_ref` dropped by an app write | A confirmed keyed Account; an app write (History entry, no confirmation) removes its `secret_ref`, so the hash check passes | `test` and the directory refuse before any Keychain read: confirmed with a key, but the entry has no `secret_ref`; run `agentic accounts change <id>`, which confirms it again with its key and writes `secret_ref: <id>`; no hand-edit advice, because a hand edit is refused as an outside change. Nothing sent. *(Manager's correction, K1, 2026-11-25.)* | Worded refusal |
| 15 | Keyed, `secret_ref` dropped by hand | The ticket's case: a hand edit removes `secret_ref` from a keyed remote Account | Refused before any Keychain read (an outside change of the user file, as 2.12 built). *(guard: ticket verify)* | Worded refusal |
| 16 | Keyless, `secret_ref` added by an app write | Row 1's Account; an app write adds `secret_ref: llama` | Refused before any Keychain read: confirmed with no key, but the entry names a Keychain entry; `agentic accounts change llama` confirms it keyless again and removes the line; to give it a key, remove it (`agentic accounts remove llama`) and add it again without `--no-key` *(manager's correction, K1: the planned words offered a hand edit and said `change` would confirm it with a key, which it does not)* | Worded refusal |
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
- 2026-11-25, manager: rebased onto main `7364da7` (3.7 merged as #48), and the baseline set to it.

## Design Notes

### Owner answers (2026-11-25)

**Owner, verbatim:** "Approve 2.10 with the recommended answers to Q1–Q8. Q1: record the port risk in the register, and show a clear "local model server not answering" error. Q2: 127.0.0.1 only. Q5: yes, with terminal-only and typed-name confirmation. Q6: both directions are widening. Merge #48 on green after I check the price files."

**How the plan takes them:**
- **Q1–Q8:** the recommended answers, as written below, with these additions:
  - **Q1:** the register gets a row for the port-squatting risk. While the local server is stopped, another local process can take its port and receive prompts. When a keyless Account's endpoint refuses the connection, times out or returns nothing, the error says plainly that the local model server is not answering and names the Account and the address. It must never read as a key problem or a cloud error. A test pins the words.
  - **Q2:** `--no-key` accepts only `http://127.0.0.1:<port>`. `localhost`, `[::1]` and other `127/8` addresses are refused in words before any prompt.
  - **Q5:** keyless `change` (an address change stays keyless; `--no-key` on a provider change or a first confirmation; the `secret_ref` fix-up) is built here. It is terminal-only, with typed-name confirmation, the same as `accounts add`. Tests cover the non-terminal refusal and a wrong typed name.
  - **Q6:** a change of key state is widening in both directions, keyed to keyless and keyless to keyed. In this story, the writer refuses an Undo or Restore that flips it. A documents-only replan records it in AD-15's list and 4.17's ticket; that replan merged as #49 (main `1244f3e`, 2026-11-25). The manager's earlier note said it would merge before this story's tests started, but the tests started first. They do not depend on its text, and the manager corrected this note.
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

### API the tests fix

The Code Map's shape, as the tests call it. A choice the Code Map leaves open is marked *(tester, 2026-11-25)*.

**am-config**
- `am_config::KeyState { Stored, None }`, re-exported at the crate root. It derives `Debug, Clone, Copy, PartialEq, Eq` *(tester, 2026-11-25)*.
- `am_config::key_state(&Account) -> KeyState`, re-exported beside `key_ref`. It reads the line's presence: `Stored` for any `secret_ref` line (one naming another entry included; the loader still refuses that entry), `None` for no line.
- `key_ref(&Account)`: `Some(id)` only when `secret_ref` is the id; `None` when the line is absent or names another entry.
- The builders take the key state as their **last** argument *(tester, 2026-11-25: last, as 2.3 added `base_url` last)*. `Stored` writes `secret_ref: <id>` and `None` writes no line; every existing call passes `Stored`.
  - `add_account_edit(file, id, provider, name, base_url, key: KeyState) -> ConfigEdit`;
  - `new_accounts_file(id, provider, name, base_url, key: KeyState) -> Result<Vec<u8>, WriteError>`;
  - `change_account_edit(file, id, provider, base_url, key: KeyState) -> Option<ConfigEdit>`. `Stored` adds `secret_ref: <id>` to an entry that lacks it. `None` removes the line. A state the entry already has changes no line. An edit that changes nothing may give `WriteError::NoChange` or an entry whose before equals its after; the test accepts either.
- The user-mode writer refuses an Undo or Restore that adds or removes any Account's `secret_ref`. The words name the Account (`'<id>'`, when it loaded valid) and `agentic accounts change`. Nothing is written, and no History entry or pending intent is left.
- Loader fix for `secret_ref: <other>`: it holds `secret_ref: <id>` and the word "remove" nowhere.

**am-domain:** `AccountStamp { endpoint, version, keyed: bool }`. The trait's default `account_stamp` answers `keyed: true`.

**am-net:** `NetEgress::send` refuses a request whose `auth` names an Account whose stamp says `keyed: false`:
- with `EgressError::Blocked(_)`, before any `SecretStore` call (`get`, `set` or `delete`), and nothing is sent;
- with one `egress.blocked` carrying `account`, `scheme`, `host` and `port`, and a `reason` that contains "has no key" *(tester, 2026-11-25: the fragment)*.

**am-app**
- `am_app::key_use::Confirmed` gains `pub key: am_config::KeyState`.
- `ProjectDirectory::account_stamp` sets `keyed` from `Confirmed.key`.
- An Account whose file and record key states disagree (rows 14, 16): `account_stamp` gives `None`, and `account_refusal` gives words that hold `secret_ref`.
- `--no-key` is a flag with no value, accepted by `add` and `change` only.

**Words the tests pin** *(tester, 2026-11-25)*. "err" is the refusal printed on stderr; "out" is the command's stdout; "sentence" is the text shown on the terminal.

*Refusals*
- A `--no-key` refusal exits 1, before anything is shown. err holds `--no-key`. This covers a remote endpoint, a fixed provider, a keyed address change, and a remote change or confirmation. A fixed provider's refusal also names `https://api.anthropic.com` or `https://api.openai.com/v1`.
- Another spelling of this Mac exits 1 with 2.3's words (`is not accepted`), before any prompt.
- An add at port 1234 or 11434, or at an own port, exits 1 before any prompt, keyed or keyless. err names `local-lmstudio`, `local-ollama`, or `the app's own server is listening on port <port>`.
- A keyless Account moved to remote: err holds 2.14's `stays on this Mac`.
- A writer refusal of a key-state flip holds `agentic accounts change`.

*Add*
- The keyless add's sentence holds:
  - the id, `openai-compatible` and the base URL;
  - `stay on this Mac, on port <port>`;
  - `no credential`, and "no key" (any case).
- It never holds `already holds`.
- Only the id is read, echo on. out holds `keyless`.
- `account_added` for a keyless add records `key: none`, `confirmation: typed_id`, `scheme: http`, `host: 127.0.0.1` and the port, committed with the History entry.

*List, test, remove*
- `list`:
  - the line starts `<id>  openai-compatible  127.0.0.1:<port>  keyless` and holds `no credential`;
  - it never holds `unconfirmed`;
  - no `get:<id>` is made.
- `test` of a keyless Account:
  - out holds the base URL and `no credential`;
  - there is one POST, with no credential header and no DNS lookup;
  - there are zero Keychain calls.
- Q1, server not answering:
  - err holds `local model server`, `not answering`, the id and `127.0.0.1:<port>`;
  - out and err hold none of the whole words key, keys, keychain, cloud, credential or credentials;
  - there are zero Keychain calls;
  - a keyed remote Account's failure never holds `local model server`.
- Row 14: `test`'s err holds `secret_ref: <id>`. The `list` line holds `unconfirmed` and `secret_ref`.
- Row 16: `test`'s err holds `agentic accounts change <id>`. The `list` line holds `unconfirmed` and `agentic accounts change <id>`, and no `get:<id>` is made.
- `remove` of a keyless Account:
  - out holds "no key" (any case);
  - it never holds `stays in the Keychain` or `Keychain Access`;
  - there are zero Keychain calls.

*Change*
- Keyless address change:
  - the sentence holds both ports and `no credential`, and never `will now be sent`;
  - nothing printed holds `the key the Keychain holds`;
  - it reads the id, then `yes`;
  - `account_changed` records `key: none` and `typed_id_yes`.
- `--no-key` confirmation (a first confirmation, or a provider change):
  - only the id and `yes` are read;
  - the sentence holds `no credential`, and never `type a new key` or `already holds`;
  - `account_removed` records `reason: provider_change` and no `key_for…` field;
  - `account_added` records `key: none`, `confirmation: typed_id_yes_new_key` and `replaces` (`none` or absent for a first confirmation; `<provider> <scheme>://<host>:<port>` for a provider change);
  - both notes commit with the History entry (`Edit`) that removes `secret_ref`;
  - there is no `account_key_set`, and the Keychain entry under the id is left as it is.
- `--no-key` on a keyless Account's address change is accepted, and the Account stays keyless.

### Red run (tester, 2026-11-25)

**Baseline:** `ed5fc56`, before any edit. `cargo nextest run` gave 1741 passed and 8 skipped.

**Stage 1.** Each new test was first written against today's API, so each could be seen red for its own reason. Two things stood in for the new API:
- a local `KeyState`, with the five-argument builders;
- for a `None` entry, an app write that removes `secret_ref`.

The `Confirmed::key` and `AccountStamp::keyed` assertions were left out.

Command: `cargo nextest run -p am-app --test accounts_keyless`. Result: 22 run, 20 failed, 2 passed.
- **Exit 2 (usage).** `--no-key` is an unknown option, so each of these failed at its exit-code assertion:
  - `a_keyless_add_on_this_mac_writes_no_secret_ref_and_records_key_none` (row 1);
  - `a_keyless_add_is_refused_for_a_remote_endpoint_before_any_prompt` (rows 2, 3; expected 1);
  - `a_keyless_add_refuses_every_other_spelling_of_this_mac` (row 4);
  - `a_keyless_add_cancelled_or_mistyped_writes_nothing` (row 6);
  - `a_keyless_add_is_refused_from_a_pipe_and_while_an_owner_child_is_enabled` (rows 7, 8).
- **Row 5.** `an_add_on_a_reserved_local_port_or_the_apps_own_port_is_refused`: today a keyed add at `http://127.0.0.1:1234/v1` succeeds (exit 0).
- **The keyless setup add fails (exit 2)** in:
  - `test_of_a_keyless_account_sends_no_credential_and_reads_no_key` (row 11);
  - `a_keyless_server_that_is_not_answering_is_named_and_never_a_key_or_cloud_problem` (Q1);
  - `the_directory_answers_a_keyless_account_with_no_key` (row 13);
  - `a_keyless_account_given_a_secret_ref_by_an_app_write_is_refused` (row 16);
  - `list_shows_a_keyless_account_as_keyless_and_reads_no_key_for_it` (row 17);
  - `removing_a_keyless_account_makes_no_keychain_call_and_says_so` (row 18);
  - `a_keyless_address_change_stays_keyless_and_makes_no_keychain_call` (row 19);
  - `a_keyless_change_runs_only_on_a_terminal_with_the_typed_name` (Q5);
  - `a_keyless_account_never_moves_to_a_remote_endpoint` (row 20);
  - `no_key_is_refused_on_a_keyed_address_change_and_for_a_remote_endpoint` (row 22).
- **Row 14.** `a_keyed_account_whose_secret_ref_an_app_write_removed_is_refused`: today the check passes, the key is read and the call is sent (exit 0, "The answer is 4.").
- **`change` rejects `--no-key` (exit 2)** in `a_first_confirmation_with_no_key_confirms_a_loopback_account_keyless` (row 21) and `undo_or_restore_that_flips_a_key_state_is_refused` (row 24, Q6).
- **Row 23.** `a_keyed_first_confirmation_writes_the_secret_ref_the_file_lacked`: after the confirmation the file has no `secret_ref` (`left: None, right: Some("spare")`).
- **Guards, green today:**
  - `an_empty_key_never_makes_an_account_keyless` (rows 9, 10);
  - `a_hand_edit_removing_secret_ref_from_a_keyed_remote_account_is_refused` (row 15).

Command: `cargo nextest run -p am-config --test user_accounts --test writer`, filtered to the new tests. All three failed:
- `key_ref_names_the_id_only_when_secret_ref_names_it`: an absent `secret_ref` gives the id (`left: Some("local")`).
- `the_secret_ref_fix_never_tells_a_keyed_account_to_drop_the_line` (row 25): the fix reads "write `secret_ref: cloud`, or remove the line".
- `an_undo_or_restore_that_adds_or_removes_a_secret_ref_is_refused` (row 24): the Undo of the flip is applied, because the guard compares provider and `base_url` only.

Command: `cargo nextest run -p am-e2e --test rule_09_keyless`, without `keyed`.
- `a_credential_for_a_keyless_account_is_refused_before_any_keychain_read` (row 13) failed: "Bearer Post: expected Blocked, got HTTP 200". The canary was read and sent.
- The other two passed, as the plan said: the rules hold today.
  - `a_keyless_accounts_request_carries_no_credential_and_reads_no_key` (row 12, ticket verify);
  - `a_keyless_accounts_redirect_off_its_endpoint_is_refused_and_audited` (row 12).

Q1 harness probe: the same five failing transports on a *keyed* loopback Account each exit 1 today, with the generic "the test call … failed: network refused or failed: …" words. So the harness produces each failure, and the words are what is missing.

**Final state (now).** Each target was compiled with `cargo test -p <crate> --test <target> --no-run`. Every error is one of the pinned API gaps above:
- `am-app/accounts_keyless`: `am_config::KeyState` unresolved; the builders' argument count (E0061 ×2); `Confirmed::key` (E0609 ×2); `AccountStamp::keyed` (E0609 ×2).
- `am-e2e/rule_09_keyless`: `AccountStamp` has no field `keyed` (E0560); `.keyed` (E0609 ×2).
- `am-config/account_edit`, `am-config/user_accounts`: `KeyState` and `key_state` unresolved; the builders' argument count.
- Mechanical supersession, which does not compile until the builders take `KeyState` (E0061, E0433). These targets were green at baseline, with the test counts shown:

  | Target | Builder calls passing `KeyState::Stored` | Baseline tests |
  |---|---|---|
  | `am-app/accounts_change` | 3 | 31 |
  | `am-app/accounts_cli` | 1 | 60 |
  | `am-app/accounts_migrate` | 1 | 44 |
  | `am-app/accounts_tty` | 1 | 6 |
  | `am-app/user_accounts` | 5 | 49 |
  | `am-config/account_change` | 4 | 5 |
  | `am-config/account_edit` | 6 | 7 |
  | `am-config/user_accounts` | 6 | 27 |
  | **Total** | 27 | 229 |

  `accounts_cli.rs` also has its two superseded doc comments (L2348, L2900) reworded; their assertions stand.
- `am-config/writer` compiles. Its new test is red at runtime, as in stage 1.
- No test builds an `AccountStamp` literal, and no existing test writes a confirmed keyed model Account without `secret_ref` through an app write. A grep of am-app and am-e2e tests shows the seeds without the line are hand-written entries in validate and escape tests, never confirmed.
- Row 25's `test` and `change` `SecretRef` fix words (`accounts.rs` L1264, `change.rs` L176) cannot be reached through the CLI: the loader makes `secret_ref: <other>` an entry error, so `Refused::Entry` answers first. No test pins them; the build changes them as the Code Map says.

**Everything else (green).** nextest over every target that compiles:
- the default members other than am-app, am-config and am-e2e: 647 passed, 2 skipped;
- `-p am-app --lib --bins` with the 16 other test targets: 211 passed;
- `-p am-config --lib` with the 18 other test targets: 289 passed, 1 failed (the new writer test above, red as intended);
- `-p am-e2e`, every target but `rule_09_keyless`: 365 passed, 5 skipped.

That is 1,512 green, which is the baseline's 1,741 less the 229 tests in the eight superseded targets.

Also green:
- `cargo test --doc -p am-app -p am-config` (am-e2e has no library);
- `cargo xtask check-deps`;
- `cargo xtask denylist` (308 files clean);
- `cargo xtask fixed-hosts`;
- `cargo fmt --all -- --check`.

Clippy (`-D warnings`) is clean on the new writer test, and on `accounts_keyless.rs` and `rule_09_keyless.rs` built with the stage-1 stand-ins in place of the new API.

**Test lines (no `src/` change):**
- `crates/am-app/tests/accounts_keyless.rs`: 2,122, of which about 830 are the kit, copied from 2.14's harness as Design Notes chose;
- `crates/am-e2e/tests/trust/rule_09_keyless.rs`: 402;
- new tests in `am-config/tests`: `account_edit.rs` +164, `user_accounts.rs` +63, `writer.rs` +113;
- supersession: about 95 lines, mostly rustfmt splitting the longer calls;
- the `[[test]]` target in `am-e2e/Cargo.toml`: +6.

That is about 2,870 test lines, over the plan's 1,400–2,000. Most of the excess is the kit and the owner's additions (Q1, Q5, Q6).

### Build (implementer, 2026-11-25)

The tester's tests were made to pass unchanged: no test file was edited by the build.

**Decisions the plan left open (standing rules: wording, layout, engineering calls):**
- **am-config.** `KeyState::recorded()` gives the record's `key` word (`stored`, `none`). `AccountsFile` keeps each item's `secret_ref` presence, so that `change_account_edit` sets or removes the line only when the entry disagrees. The writer gives a key-state flip its own clause, apart from a move: "add or remove the `secret_ref` of the Account '<id>' (whether its calls carry a key), which is confirmed only with `agentic accounts change <id>`".
- **`key_use`.** `Destination::names` now compares provider, scheme, host and port only, and `Destination::key_state()` compares the key. `add`'s reuse of a held key asks for both. Two new `Refused` kinds, `KeyedWithoutLine` (row 14) and `KeylessWithLine` (row 16), come after the destination check. A keyless pairing at a non-loopback endpoint is also refused, as a guard that the app's own writes cannot reach.
- **am-net.** It refuses a credential for a keyless Account in two places: right after the first stamp, before anything is resolved; and before each Keychain read in the retry loop, in case a stamp taken again has turned keyless.
- **Reserved local Accounts.** `ProjectDirectory` still answers them with `keyed: true`, unchanged (Q8).
- **Q1 words.** These apply to a keyless Account only, and only to an `EgressError::Network` failure (refused, timed out, closed, no reply head, or an idle body):
  - they read "the local model server of the Account '<id>' at 127.0.0.1:<port> is not answering…", with the transport's own words in parentheses;
  - the egress client's timeout block words are not added;
  - an HTTP status, a block or a protocol error keeps 2.3's words;
  - keyed Accounts are unchanged, keyed loopback ones included.
- **`add`.** The refusal at a reserved local port (1234, 11434) or one of the app's own ports covers keyed and keyless loopback adds alike, and comes before any Keychain call. `change` keeps 2.14's own-port refusal only. No reserved-port refusal was added to `change`: am-net refuses those calls, and the plan names only `add`.
- **`change`.**
  - The "keyless stays on this Mac" refusal now needs a live keyless record. A first confirmation of an entry without `secret_ref` takes the keyed path and writes the line (row 23).
  - An address change writes the live record's key state. An entry that disagrees with the record it re-confirms is therefore put back in line.
  - `--no-key` with no other flags never finishes an interrupted keyed provider change: the pending change is read only without `--no-key`.
- **`list` and `remove`.** One predicate decides keyless: a model Account with no `secret_ref`, at `http://127.0.0.1:<port>`, whose live record is not keyed.
  - `list` reads each live record once.
  - It makes no Keychain read for a model entry without the line, or for one whose record is keyless.
  - Search and crawl Accounts are read as before (DR-191).

**Register:** DR-079 is closed. DR-187 to DR-192 are added.

**Size:**
- `src`: about 665 lines added and 145 removed, against the plan's estimate of ~535 (range 450–700).
- Docs: 8 lines.
- Tests: the tester's, about 2,870 lines.

### Build note: a slip (implementer, 2026-11-25)

The build once ran `cargo clippy --workspace --all-targets -D warnings` by mistake. `--workspace` includes `am-desktop`, which is outside the default members, so the shared target cache now holds clippy (check-mode) incremental artifacts for `agentic_desktop`.
- It passed clean, and nothing was cleaned or rebuilt.
- Every later check used the justfile's own commands, which use the default members.

### Review triage, round 1 (manager, 2026-11-25)

Two Opus reviews ran on the built diff: correctness (C) and adversarial security (S). Neither found a blocker. Both confirmed these hold:
- `--no-key` accepts only `http://127.0.0.1:<port>`, refused before any prompt, whatever the spelling (IPv6, other 127/8 addresses, userinfo, percent-encoding, IDNA, trailing dots).
- Redirects stay pinned, and loopback needs no DNS.
- No keyless path makes a Keychain call. am-net refuses before any `SecretStore` call.
- No silent flip between keyed and keyless: the file and the record are compared, and the writer refuses an Undo or Restore that flips the key state, in both directions.
- Terminal-only use with a typed name.
- The Q1 words never mention a key, the cloud or a credential.
- `prices.rs` is untouched, and keyless calls stay local at 0.

Fixes for round 2, tests first. The tester writes each test and logs it red; the implementer then fixes it. Both run on Opus.
- **K1 (C1, C2, S1, important):** the repair words for rows 14 and 16 led to a dead end, and row 16's words even named a command that does the opposite of what they promised.
  - **Row 14** (a keyed record with no `secret_ref` line): name `agentic accounts change <id>` first. It puts the line back. Keep `secret_ref: <id>` in the text, but drop the hand-edit advice, because a hand edit is refused as an outside change.
  - **Row 16** (a keyless record with a `secret_ref` line): `agentic accounts change <id>` confirms the Account keyless again and removes the line. To give it a key, remove it and add it again without `--no-key`.
  - Both refusals, the `list` column and `test` use the same words.
  - Matrix row 16's planned wording is corrected the same way (a manager's correction of the plan's own text).
- **K2 (S2, important; older code, made reachable by keyless calls):**
  - `am-providers/src/common.rs` truncates the error body at byte 2000. That can panic when byte 2000 falls inside a multi-byte character. Truncate on a character boundary instead.
  - `agentic accounts test` prints the streamed reply and the error body raw. Strip control characters, except `\n`, before printing, so escape sequences (OSC 52, cursor moves) cannot spoof or overwrite lines. A local process squatting on the port (DR-187) can now reach this.
  - Tests: a reply of 1,999 ASCII bytes followed by `é` does not panic, and an ESC or OSC sequence in a reply or an error body is printed neutralised.
- **K3 (C3, S3):** `last_confirmed` skips `key: none` records, so a held key never names a keyless confirmation.
- **K4 (C4, S5):** `change` also calls `not_a_taken_port` (1234, 11434, and the app's own ports), and `add` repeats it under the lock.
- **K5 (C5):** `remove`'s words follow the record's key state:
  - a row-16 entry is described as keyless;
  - a store-read error never reads as "It had no key".
- **K8 (S6):** a test for am-net's second keyless refusal, inside the retry loop, using a directory whose stamp turns keyless between takes.

Register edits (the implementer makes them):
- **K6 (C6):** DR-191 notes that the writer's key-state clause names `agentic accounts change`, which refuses non-model Accounts such as search Accounts.
- **K7 (S4):** DR-188 is reworded: a `--no-key` provider change also leaves the live key in the Keychain (AD-15), unlike a keyed provider change, which deletes it.
- **K9 (S6) → new row:** 2.6 may set the reserved local Accounts to `keyed: false`, so they get the same refusal before any Keychain read. Target 2.6, must close no.

### Red run of review round 2 (tester, 2026-11-25)

**State:** `story/2-10` on `7364da7`, with the uncommitted build in place. The tests were added and nothing else was changed: no `src/` change, and no existing test edited.

The tests are in:
- `crates/am-app/tests/accounts_keyless.rs`, in a section headed "review round 2";
- `crates/am-providers/tests/error_body.rs`, which is new. `common.rs`'s own unit tests are in `src/`, so this test reaches its error path through both public adapters;
- `crates/am-e2e/tests/trust/rule_09_keyless.rs`.

**Command:** `cargo nextest run --no-fail-fast` over the default members. Result: 1,780 run, 1,772 passed, 8 failed, 8 skipped. The 8 failures are the 8 new reds below; nothing else fails.

**Red, one line each:**
- **K1, row 14:** `row_14s_words_name_change_first_and_give_no_hand_edit_advice`. The `test` words do not name `agentic accounts change router`; they give hand-edit advice ("put `secret_ref: router` back in its entry").
- **K1, row 16:** `row_16s_words_say_change_confirms_it_keyless_and_how_to_give_it_a_key`. The `test` words never say `keyless`; they say "remove the `secret_ref` line, or confirm it with a key: `agentic accounts change llama`".
- **K2, providers:** `am-providers::error_body an_error_body_cut_inside_a_character_never_panics_and_stays_within_the_limit`. The Anthropic adapter panics in `common.rs:163` (`truncate(2000)` inside `é`).
- **K2, `test` output:** `test_prints_a_reply_and_an_error_body_with_control_characters_neutralised`. A streamed reply is printed with raw ESC, BEL, CR, C1 CSI and BS (`['\u{1b}', '\u{7}', …, '\u{9b}', '\u{8}']`). A probe also showed that the error body (HTTP 400) and the error event both exit 1 with the same raw characters.
- **K3:** `add_never_names_a_keyless_record_as_where_a_held_key_was_last_confirmed`. The sentence says "It was last confirmed for provider openai-compatible at http://127.0.0.1:8080", which is the keyless record.
- **K4, change:** `change_to_a_reserved_local_port_is_refused_before_any_prompt`. `change llama --base-url http://127.0.0.1:1234/v1` succeeds (exit 0, "changed the Account 'llama' to … 127.0.0.1:1234").
- **K4, under the lock:** `an_add_whose_port_the_app_takes_while_it_waits_is_refused_under_the_lock`. An `agentic serve` records port 9090 while the id prompt waits, and the keyless add still succeeds ("added the Account 'llama' (…, http://127.0.0.1:9090/v1)").
- **K5:** `remove_describes_an_entry_by_its_records_key_state`. Removing a row-16 entry says "Its key stays in the Keychain; delete the entry 'llama' … in Keychain Access".

**Green on arrival:**
- **K1:** `change_repairs_both_key_state_mismatches_as_the_words_say`. The fix the new words name already works:
  - in the row-16 state, `agentic accounts change llama` (id, `yes`) leaves the Account keyless, with no `secret_ref`, `account_changed key: none`, no Keychain call, and `test` sending no credential;
  - in the row-14 state, `agentic accounts change router` puts `secret_ref: router` back, with no Keychain call, and `test` sends the key.
- **K8:** `am-e2e::rule_09_keyless a_stamp_that_turns_keyless_between_takes_is_refused_before_any_keychain_read`. The code exists; this guards it.
  - The directory's first stamp is keyed but for port 8081, so no key is read. The stamp taken again inside the loop is keyless at 8080, and is refused there: 2 takes, zero Keychain calls, nothing sent, `egress.blocked` with `account`, the endpoint and "has no key".
  - In the control case the stamp taken again stays keyed. The same pass then reads and sends the key, so the refusal above comes from the loop's check.

**Not reachable without a `src/` seam:**
- **K5, store-read error.** `remove` reads the record's key state under the lock, after `Locked::take`'s owner-only checks. It has no prompt or other hook between the lock and that read. A store that fails there also fails the write that follows, so the success line ("It had no key") cannot be reached with a failed read. The fix's handling of that error is left to review. A unit test inside `src/` would be the implementer's, and the manager decides.
- **K4, reserved ports under the lock.** Ports 1234 and 11434 are fixed, so they cannot become taken while a prompt waits. Only the own-port half of `add`'s check under the lock can be reached, and it is tested above.

**Words pinned (tester, 2026-11-25), on top of the triage:**
- **Row 14:** each of `test`'s err, the `list` line and the directory's refusal:
  - holds `agentic accounts change <id>` before `secret_ref: <id>`;
  - holds none of "put" (as a word), "remove the `secret_ref` line", "remove the line", "back in its entry", "edit the file", "edit its entry", "edit accounts.yaml" or "by hand".
- **Row 16:** the same three forms:
  - hold `agentic accounts change <id>`, `keyless`, `agentic accounts remove <id>` and ``without `--no-key` ``;
  - never hold ``with a key: `agentic accounts change``;
  - hold no hand-edit phrase.
- **K2, providers:** the kept body is at most 2,000 bytes, keeps the 1,999 ASCII bytes, and holds no U+FFFD.
- **K2, `test`:** nothing `test` prints, on stdout or stderr, holds a control character other than `\n`, and the visible text is kept.
- **K3:**
  - with an earlier keyed confirmation, the sentence names it ("last confirmed for provider openai-compatible at https://openrouter.example:443");
  - with none, it keeps the existing words "No confirmation of it is recorded on this Mac".
- **K4:**
  - `change` exits 1 before any prompt, naming `local-lmstudio` or `local-ollama`;
  - under the lock, `add` exits 1 naming "the app's own server is listening on port 9090", with no `set` or `delete` and nothing written.
- **K5:**
  - a row-16 entry: stdout holds "no key", and never "stays in the Keychain" or "Keychain Access";
  - a row-14 entry on this Mac keeps "stays in the Keychain", and never "It had no key".

**Also green:**
- `cargo clippy --all-targets -D warnings`, over the default members;
- `cargo fmt --all -- --check`;
- `cargo xtask check-deps`;
- `cargo xtask denylist`;
- `cargo xtask fixed-hosts`;
- `cargo test --doc -p am-app -p am-providers`.

**Test lines added:**

| File | Lines |
|---|---|
| `accounts_keyless.rs` | +486 |
| `error_body.rs` (new) | 128 |
| `rule_09_keyless.rs` | +151 |
| **Total** | about 765 |

### Build, review round 2 (implementer, 2026-11-25)

The tester's 8 reds now pass, and no test was edited. K8 is still green.

- **K1:** `key_use::keyed_without_line_words` and `keyless_with_line_words` are rewritten. The refusal, `list`'s `unconfirmed:` column and `test` all use them.
  - **Row 14:** "run `agentic accounts change <id>` in a terminal: it confirms it again with its key and writes `secret_ref: <id>` to its entry".
  - **Row 16:** "run `agentic accounts change <id>` in a terminal: it confirms it keyless again and removes its `secret_ref` line. To give it a key instead, remove it with `agentic accounts remove <id>` and add it again without `--no-key`".
  - Matrix row 16 is corrected, marked as the manager's correction, K1.
- **K2:** `am-providers/src/common.rs` now cuts the error body with `floor_char_boundary(2000)`. `agentic accounts test` prints these through a new `printable` function: the streamed reply, the error text (an HTTP body or a stream error event) together with the block words, and the Q1 cause.
  - `printable` is `am_domain::text::escape_hidden`, the same table `agentic gate` uses, with a tab also escaped. Only `\n` stays raw.
  - `am-providers`' `neutralise` is unchanged.
- **K3:** `last_confirmed` skips `account_added` and `account_changed` records with `key: none`.
- **K4:** `change` calls `not_a_taken_port` before its prompts and again under the lock. It replaces 2.14's `not_the_app`, and the own-port words are the same. `add` repeats `not_a_taken_port` under the lock, before any Keychain `set`.
- **K5:** the new `removed_key_words` takes the live record's key state, or the entry's when there is no record. A record that cannot be read never says "It had no key". The in-crate unit test `accounts::tests::removes_words_never_say_no_key_when_the_record_cannot_be_read` covers this.
- **Register:**
  - DR-188 reworded (K7);
  - DR-191 note added (K6);
  - DR-193 added (K9): 2.6 may answer the reserved local Accounts `keyed: false`; Target 2.6, must close no.

**Size of round 2:**
- `src`: +74 / −24. About +47 lines are production code; +27 are the in-crate unit test.
- Docs: three register rows.
- Tests: the tester's, about 765 lines.

### Review triage, round 2 (manager, 2026-11-25)

A focused Opus re-review found round 2 sound: no blocker and nothing important. K1–K5 are correct and complete, and they open no new hole.
- **K2's escaping:** `printable` covers every server-controlled path in `test`. It escapes each character, so a sequence split across chunks cannot get through. It covers C1, ESC, BEL, DEL and the bidi controls.
- **Unchanged:** egress, the allow-lists, `prices.rs`, 3.4's code, taint, `neutralise` and the check-deps rules.

Nits:
- **N1:** DR-193's text claimed a Keychain read that does not happen. `credential_is_callers` already refuses it, and `rule_09_callers` tests that. The row is reworded.
- **N3 → DR-194 (2.8, must close):** `\n` stays raw in `test`'s error paths, so a process squatting on the port could add forged stderr lines.
- **N2 → DR-195 (2.8):** `remove`'s "It had no key" after a `--no-key` provider change.
- **N4 → DR-196 (2.8):** two wording edges.

## Auto Run Result

**Status:** built, 2026-11-25. Security story; planned, tested, built and reviewed on Opus under the owner's rule of 2026-11-25. The owner approved the plan with Q1, Q2, Q5 and Q6 as answered. AD-15 and 4.17 record Q6 (#49).

**Summary.** `agentic accounts add|change --no-key` creates a keyless `openai-compatible` Account at `http://127.0.0.1:<port>` only.
- It is refused for any other endpoint before any prompt, in every spelling.
- It is terminal-only, with typed-name confirmation.
- It stores and reads no key and no placeholder, writes no `secret_ref`, and makes no Keychain call of any kind.
- am-net refuses a credential reference for a keyless Account before any Keychain read, and audits `egress.blocked`.
- The key-use check compares the file with the record. A keyed record with no line (row 14) and a keyless record with a line (row 16) are both refused, and each refusal names `agentic accounts change <id>` as the repair.
- The writer refuses an Undo or Restore that flips the key state, in both directions (Q6).
- When a keyless Account's local server does not answer, the error says so, and never mentions a key, the cloud or a credential (Q1).
- `accounts test` neutralises server text before printing it.
- The error-body truncation is char-safe.
- `add` and `change` refuse the reserved local ports and the app's own ports, again under the lock.
- `list`, `test` and `remove` speak of keyless Accounts.
- 3.7's pricing is unchanged: a keyless Account is local at zero cost.

**Register:**
- DR-079 is closed.
- Added: DR-187 (port-squat risk, accepted), DR-188 (a leftover key is never read), DR-189 (4.17), DR-190 (owner: 12.16 keyless), DR-191 (search and crawl key state), DR-192 (validate), DR-193 (reserved Accounts' words), and DR-194..196 (2.8 sweep).

**Size (code and test lines separately):**
- Production code: about 553 net lines.
- In-crate tests: about 26 lines.
- Docs: about 9 lines.
- Tests: about 3,730 lines.

**Tests.** Written first, with a red run logged for the build and for review round 2. `just check` passed with 1,781 tests and 8 skipped.

**Review (Opus):**
- Round 1 (correctness and adversarial security): no blocker; K1–K9 fixed.
- Round 2 (focused re-review): sound; the nits went to the register.

**Process notes for the owner:**
- The tests started before the Q6 replan (#49) merged, against my earlier note. The tests do not depend on its text, and the note is corrected.
- The tester once built `am-desktop` through `--workspace`, and so did the implementer once with clippy. Both passed. Nothing was cleaned, and no copy was built.
