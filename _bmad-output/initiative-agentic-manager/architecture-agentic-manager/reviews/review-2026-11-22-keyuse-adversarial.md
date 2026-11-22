# Adversarial review: AD-15 *Key use*, *Later confirmations*, *Keyless loopback Account* (2026-11-22)

Scope: the uncommitted change to AD-15's **Interim CLI Account edit** (Prevents clause, *Record* naming the provider, new *Key use*, *Later confirmations*, *Keyless loopback Account*) and the new last Open Questions bullet, read against AD-12, AD-13, AD-16, AD-17, AD-25, AD-31, AD-48 and the built check in `crates/am-app/src/accounts.rs` (`confirmed`, `test`, `add`). Owner decisions are taken as fixed: no key to an unconfirmed provider or endpoint; `accounts test` needs no terminal and takes no lock; keyless only for `http://127.0.0.1:<port>`, a remote Account always needs a key.

## Verdict

**Revise before tickets are cut.** The direction is right and the built `test` check matches the letter, but the rule checks *which config was confirmed* and never *which key is sent*. Because AD-17 names keychain entries by Account id and shares them across projects, a key confirmed for one endpoint can reach another endpoint that was confirmed for a different key (critical). Four further gaps let units that each follow the ADs to the letter build incompatibly or lock the owner out of an Account they did confirm: changing an endpoint is not a widening kind; a run's pinned snapshot does not fit "latest History entry"; shipped-template Accounts have no confirmation path; and the check trusts database rows that an owner-uid child can forge. Each gap has suggested wording that fits inside the interim passage.

---

## Critical

### C1. The check proves the config was confirmed, not that the key was. A shared keychain entry lets a key reach an endpoint it was never confirmed for.

*Rule text:* "A key is sent only to a provider and endpoint that an owner-confirmed add recorded". AD-17: entries are `service agentic-manager / account <account-id>`, shared by every project on the Mac. *Add:* "An existing keychain entry for that id is reused, never overwritten."

*Attack (each unit obeys every AD):*
1. Project A: `agentic accounts add cloud-main --provider openai` stores key K1 and records `account_added` (openai, api.openai.com:443).
2. Project B: the owner removes B's own `cloud-main`. The app's remove message tells them to "delete the entry 'cloud-main' … in Keychain Access if no project needs it", and they do. They then add `cloud-main --provider openai-compatible --base-url https://gateway.example/v1` with key K2 (for example an OpenRouter or gateway key). The add is fully confirmed for B.
3. Project A: `agentic accounts test cloud-main` (or, later, a run or Doctor). File bytes match History, and the latest `account_added` matches openai at api.openai.com. The check passes and **K2 goes to api.openai.com**, a provider the owner never confirmed for K2.

The same happens without any manual delete once story 5.5's Account card can "set a key" (`accounts.rs` already promises that "the app's Account card will set a key when it ships"): a card that writes the shared entry in project B retargets project A's key. 5.5 obeys AD-15 and AD-17 to the letter here. It also happens at a first-load hold (story 4.18) of a cloned project whose `accounts.yaml` names an id this Mac already holds: *Later confirmations* requires an `account_added` but no sentence saying that an existing key will be reused. So the owner confirms "Account `anthropic` at api.anthropic.com" without being told it will carry a key stored for another project (only the CLI add says this today, from story 2.3 code, not from the spine).

*Suggested wording* (new clause after *Key use*):
> *Key identity.* Every record that confirms an Account with a key (`account_added`, and every later key set) also records `key: keychain` and `key_mark`: the first 16 hex characters of HMAC-SHA256 over the key, under a per-Mac sealing key the keychain holds (service `agentic-manager`, account `confirm-seal`, created on first use), never the key. Before any request, the caller reads the key, computes its mark and **sends nothing** when the mark differs from the latest record's, saying that the Keychain entry for that id was changed since this project confirmed it (another project, Keychain Access or a key set elsewhere) and that a re-confirmation is needed. Every confirmation that would reuse an existing keychain entry says, in the sentence the owner confirms, that this Mac's Keychain already holds a key for that id, which may have been stored for another project, and that this key will be sent to the named endpoint. No path writes over an existing keychain entry without the same sentence and a new record.

Alternative, structural: decide the Open Question on per-project keychain names (AD-17 REPLAN with migration) **before any run path or Doctor uses a key**, not merely "before 5.5". Even then, the mark stays useful against Keychain Access edits.

(The *Key use* text "If either fails, it reads no key" then needs one more phrase: "…reads no key; the key mark is compared after the read and before any request".)

---

## High

### H1. Changing an existing Account's provider, endpoint or key state is not a widening kind. A path change passes silently, and a host change locks the owner out with no way back.

AD-15's widening list has "adding an Account" and "choosing or changing a capability's provider Account". It does not list changing an existing Account's `provider`, `base_url` or `secret_ref`. *Later confirmations* says "every confirmation that … changes its provider or endpoint records …", which presupposes such a confirmation exists. Story 4.17 can classify the change as non-widening and still obey the letter.

- **Path hole (key reaches an unconfirmed endpoint):** the record and check compare only provider, scheme, host and port (`accounts.rs` `confirmed`; AD-48 defines the pin the same way). An app edit, or a single Reload of an outside edit, that turns `https://gateway.example/v1` into `https://gateway.example/<other-tenant>/v1` commits History, the bytes check passes, and the key goes to a path the owner never saw. On multi-tenant gateway hosts the path selects the tenant who logs the request. The owner's sentence at add named the full base URL.
- **Host lockout:** the same edit with a new port or host commits History, and the check then refuses for good. Neither the UI (no confirmation for it) nor the CLI (add refuses an existing id, and remove refuses a referenced id) can re-record it.
- Undo of such an edit has the same two outcomes.

*Suggested wording:*
> Add to the widening kinds: "changing an existing Account's provider, any part of its `base_url`, or its key state (`secret_ref` added, removed or renamed), Undo included". In *Record* and *Key use*: "the pinned scheme, host and port **and the full base URL as written**" and "names the same provider, scheme, host, port and base URL as the file".

### H2. A run's pinned snapshot does not fit "the file holds exactly the bytes of its latest History entry". The units are incompatible, and there is a gap between check and send.

AD-16: "A run pins the snapshot hash it started with." *Key use*: before "a run, any later call", the file must hold the bytes of its **latest** History entry. A run path built to the letter can go either of two ways:
- (a) it checks the current file but calls the endpoint from the pinned snapshot. The check then verified bytes other than the bytes used. If the Account was removed and re-added at a new host meanwhile, the old pinned endpoint is called while the check reads the new, confirmed one.
- (b) it checks the pinned snapshot. Then any unrelated edit of `accounts.yaml` (adding another Account) makes "latest History" differ, and every running workflow's next call is refused mid-run.

`test` avoids this only because its code ("the bytes checked are the bytes used") goes further than the spine.

*Suggested wording:*
> The check runs over the same bytes the call's endpoint is derived from: the command's own read, or the run's pinned snapshot (AD-16). For a run, that snapshot's file hash must equal a History entry of the file, and before each call the latest record for the Account must still name the provider, endpoint and key mark that the pinned snapshot holds, with no History entry since that removed the Account; otherwise the call is refused and the run pauses with the reason.

### H3. Shipped-template Accounts have no confirmation path, so the owner is permanently locked out.

`templates/samples/company-a/models/accounts.yaml` ships `anthropic` and `openai` with `secret_ref`, and the sample's seats name them. AD-16: a project the app creates from shipped templates gets its baseline at create, with no hold, so no confirmation writes an `account_added`. Then:
- *Key use* refuses both Accounts.
- `accounts add` refuses because the id is already in the file.
- `accounts remove` refuses because seats name the id.
- *Later confirmations* lists adds, provider or endpoint changes, holds, the 5.5 card "confirmed Account card" and Undo, but not **setting the key of an Account already in the file**.

So even after 5.5, a card that only sets the key on `anthropic` writes no record, and the Account stays refused. The same applies to a confirmed first-load hold on a Mac with no key: the record exists, but before 5.5 there is no way to set the key.

*Suggested wording* (in *Later confirmations*):
> …and every confirmation that sets or replaces the key of an Account already in the file (the 5.5 card's Set key; an Account a shipped template or a confirmed hold wrote), whose sentence names the provider, the full endpoint and whether an existing keychain entry is reused. A project created from shipped templates carries no `account_added`: its Accounts are refused until such a confirmation.

For the interim, one of two (an owner or Sofia call):
- (a) "`agentic accounts add <id>` of an id already in the file, with no `account_added` on this Mac and with the same provider and base URL as typed, confirms it in place under every *Terminal*, *Open* and *Add* rule: same sentence, typed id, key read or reused, History entry of the unchanged bytes, `confirmation: typed_id`; it writes no file change"; or
- (b) state that until 5.5 a project created from a shipped sample cannot use its sample Accounts, and the wizard or sample story must say so.

### H4. An owner-uid child can forge everything the check reads.

AD-13 concedes that stdio MCP servers and the render child run as the owner and "could read any file the owner can". They can also write the project's `accounts.yaml` and the runtime `state.db` (0600, owner). AD-25's hash chain is unkeyed, so it can be recomputed. A child can write `base_url: https://evil.example/v1` and insert a matching `config_history` row and `account_added` audit row with a recomputed chain. `test` (any stdin, no lock) or the next run then passes the check and sends the key. The keychain is the only store such a child cannot read silently, because the item's ACL is tied to the signed `agentic` binary. So the CLI class stopping its decisions while such a child is enabled does not protect key use.

*Suggested wording:*
> Each `account_added` (and key-set record) carries `seal`: HMAC-SHA256, under the same per-Mac keychain sealing key as C1, over the project_id, Account id, provider, full base URL, key state, key mark and the sha256 of the History entry it confirms. *Key use* verifies the seal before reading the key, and a missing or wrong seal is "not confirmed".

If the owner prefers not to seal, add one sentence that names the residual risk: "while a stdio MCP server or a Source with `render: true` is enabled, a child running as the owner can forge the records this check reads; the About page says so". That makes the gap a decision instead of an oversight.

---

## Medium

### M1. Paths that rewrite the file through the app must keep the record and History in step, or a confirmed Account locks.

"Any other edit of the file through the app commits its History entry as usual" does not name every path:
- **Non-widening Reload:** AD-16 says it "still applies with the single Reload", but not that it commits a History entry with the reloaded bytes. If 4.18 builds it without one, every Account in the project is refused after any harmless outside edit, such as a `name:` change or a sync tool's rewrite.
- **Intent reconciliation:** AD-16 says the intent is "completed and audited as reconciled". The code carries the intent's notes, but the spine does not require that the `account_added` the intent carried is committed. A crash between rename and commit then leaves the bytes matching and no record.
- **ConfigMigration (AD-7):** a migration that renames a provider string or the base-URL field changes no grants and is non-widening, but the literal comparison with the old record then locks every Account after an upgrade.

*Suggested wording:*
> Every app path that changes the file's bytes (Reload of a non-widening outside edit, Undo, a reconciled intent, a `ConfigMigration`) commits a History entry of the new bytes in its own transaction. A reconciled intent commits the audit entries it carried, an `account_added` included. A `ConfigMigration` that rewrites how an Account's provider or endpoint is written commits, for each Account whose old record matched the old file, a carried-forward record of the new form (`confirmation: migrated`); it never creates a record for an unconfirmed Account.

### M2. The keyless rule does not say what decides "keyless", and older records have no `key` value.

Records from stories 2.1 and 2.3 have no `key` field. The built `test` reads the entry named by the id whenever `secret_ref` is absent or equals the id (`other_entry`). The refusal text even advises "or remove the line". A keyless story built to the letter therefore has to choose between two readings:
- (a) "no `secret_ref` means keyless": this breaks or reinterprets existing keyed Accounts whose line is removed by an app edit;
- (b) keep the 2.3 path: a keyless loopback Account then **reads and sends the shared keychain entry of the same id** (C1) to the loopback port, against "read no keychain entry".

*Suggested wording:*
> Whether a call carries a key is decided by the latest record and the file together: a key is read only when the record says `key: keychain` (a record with no `key` value, from stories 2.1 and 2.3, reads as `key: keychain`) and the file's `secret_ref` equals the Account id. `key: none` with no `secret_ref` on a `http://127.0.0.1:<port>` endpoint sends no credential and reads no entry. Every other combination is refused. A keyless add neither reads nor mentions an existing keychain entry of that id.

### M3. A change of key state is not a "later confirmation".

The keyless clause refuses `secret_ref` while the record says `key: none`. *Later confirmations* covers adds and provider or endpoint changes only. So once 5.5 lets the owner give a keyless loopback Account a key, or remove one, that Account is refused for good, or a build skips the record.

*Suggested wording:* "…that adds an Account, changes its provider or endpoint, **or changes its key state (none to keychain, keychain to none, or a replaced key)** records…".

### M4. A keyed loopback Account sends its key to whatever process holds the port.

Story 2.3 already allows a key on `http://127.0.0.1:<port>`. The owner confirmed a port, not a process. If the intended server is down, any local process can bind that port and receive the key, an owner-uid child (AD-13) included. Without relitigating the decision that keyless means loopback only:

*Suggested wording* (in *Add* or *Keyless*):
> A key added to a `http://127.0.0.1:<port>` Account is sent in clear to whatever program on this Mac listens on that port; the add sentence says so, and suggests adding it keyless when the server needs no key.

### M5. "The latest `account_added` for that id" survives a removal.

After a remove, the stale record still matches if the same id and endpoint come back through any path that is not a confirmation. Today none exists, but M1's paths and a reconciled intent make it plausible.

*Suggested wording:* "the latest `account_added` for that Account id **committed after the last History entry that removed that id**".

---

## Low

- **L1.** `accounts test` from any stdin, with no lock, lets any process make paid model calls that are neither reserved in the ledger (AD-9) nor audited as model calls (AD-25), because no writer is open. The calls are small, but the passage should say that `test` is exempt and why, or cap it (for example one call per Account per 10 s).
- **L2.** The read-only check cannot tell a pending intent (crash after rename) from an outside edit, and the built text says "changed on disk outside the app". Suggested addition: "when a pending intent names the file's hash, the refusal says to open the project once to finish the write".
- **L3.** The built refusal advises "Set `secret_ref: {id}` (or remove the line)", which conflicts with the keyless rule that refuses a remote Account with no `secret_ref`. Change it when the keyless story lands.
- **L4.** The Open Questions bullet for Sofia lists the keyless story, the first run path, 4.18 and 5.5. Add: the first-run wizard or sample create (H3), non-widening Reload and Undo History (M1), `ConfigMigration` carry-forward (M1), 4.17's widening kind for endpoint, provider and key-state changes (H1), and the key mark and seal (C1, H4). Also move the AD-17 per-project naming decision to "before any run path or Doctor uses a key".

## Checked and holding

- The interactive-terminal predicate and the add flow are unchanged and sound for the CLI path.
- A remote Account cannot run keyless under the rule as written, since the endpoint and `secret_ref` checks both refuse.
- `secret_ref` naming another entry is refused and never followed (`other_entry`).
- In `test`, the check is fail-closed against concurrent writers: an unreadable DB, a pending intent or an older schema all mean "not confirmed".
- Redirects strip auth on a host change (AD-12), so a confirmed endpoint cannot bounce the key elsewhere.
