# Adversarial review: user-level Accounts (2026-11-22)

- Spine: `architecture-agentic-manager.md` as amended 2026-11-22 (owner, global Accounts): AD-7, AD-13 (end), AD-15 (widening kinds; *Interim CLI Account edit* with *Key use*, *Later confirmations*, *Retirement*), AD-16, AD-17, AD-19, AD-31, AD-32, AD-35, AD-48, Structural Seed, Deferred, Open Questions.
- Memlog: last entries of `.memlog.md` (owner constraint, decisions AD-17/AD-15/AD-13/AD-16 et al., direction for Sofia).
- Also read: tickets 2.6, 2.10, 4.11, 4.17, 4.18, 5.5 (`tickets/*/tickets/tickets.toml`); story 2.3's plan and `crates/am-app/src/accounts.rs` for the built `secret_ref` rule.
- Method: for each hole, two units one level down that each obey every AD to the letter yet build incompatibly, a concrete scenario, and the tightening text. The owner's decisions are taken as given (user-level list, ids owner-chosen, keychain naming unchanged, projects reference by id, provider or `base_url` change is widening, same-user forgery out of scope, path comparison and sample confirmation deferred). Same-user forgery scenarios are not raised.

## Verdict

**Revise.** The realisation is close: one definition owner (am-config), one record (user store `account_added`), one check, and the CLI's lock discipline are sound. But the project's approval of an Account is bound to an id only, so a confirmed user-level retarget moves every project's data, including linked-folder files, to a new host with no project-level or folder confirmation; the cross-process meaning of "unchanged since the app wrote it" is torn by the apply order; and `secret_ref` is a free field the spine never pins, so a non-widening edit can point one Account's key at another Account's confirmed endpoint.

| Severity | Count |
|---|---|
| Critical | 1 |
| High | 2 |
| Medium | 6 |
| Low | 5 |

---

## C1. A project's approval names an Account id, not its destination: a user-level retarget moves every project (and its linked-folder files) silently

- **Units:** (a) the new epic 2 story "Accounts belong to this Mac" (`agentic accounts change`, remove-then-add) and later epic 5 story 5's Account card, which confirm the change at user level and record `account_added`; (b) epic 4 stories 17/18's project grant set, where the project-level kind is "naming an Account its approved grant set does not list" (AD-15) and the folder kinds are "linking a folder to a seat whose model or fallback is a cloud Account, or changing such a seat's model or fallback from a local to a cloud Account" (AD-15, AD-30/31; epic 8 story 22).
- **Each obeys the letter:** `change` shows the old and new endpoint and "the references found", takes the typed id, writes `account_added`; Key use passes. The project grant set still lists `my-llm` by id, so nothing is a project-level widening; the seat's `model` field did not change, so the folder kind never fires.
- **Scenario 1 (folder files leave the Mac):** project P has seat `s1` on a keyed loopback Account `my-llm` (`http://127.0.0.1:8080/v1`) with a linked folder (local, so no cloud two-step was ever shown). The owner runs `agentic accounts change my-llm --base-url https://gateway.example/v1` from another project's context; P is on an unmounted volume, so the sentence says "unreachable projects were not checked". Later P opens: its grant set lists `my-llm`, Key use passes, and `s1`'s folder files go to `gateway.example`. AGENTS.md: "never send a file to a cloud model without the two-step folder-link confirmation" — broken by two compliant units.
- **Scenario 2 (remove, then re-add with another meaning):** `remove openai` refuses only for references found in *reachable* projects; P (unreachable) still names it. The owner re-adds `openai` as `openai-compatible` at `https://router.example/v1` (the sentence even says the existing keychain entry is reused). P opens and silently uses the new destination under its old approval.
- **Scenario 3 (missing, then resolved):** a sample or import names `openai`, missing on this Mac; the apply's confirmation lists it as a missing Account. Whether the grant set then records `openai` as approved is unspecified; if it does (the natural implementation: "the names the snapshot carries"), the first `openai` the owner ever adds — for a different project, at any endpoint — is silently in force in this one. This is the AD-15 *Prevents* clause "a project using an Account the owner never approved for it".
- **Tightening (AD-15, project-level kind; AD-16 grant set):**
  > The approved grant set records, for each Account a project names, the destination it was approved with: the provider, scheme, host and port and whether the endpoint is on this Mac (loopback) or not, copied from the latest `account_added` entry at the moment of confirmation, or `unresolved` when the id was missing then. At every startup load, Reload and run start, each named Account's latest `account_added` destination is compared with the recorded one; a difference (another provider or endpoint, `unresolved` now resolving, loopback now remote) is a project-level widening and holds the project as AD-16's hold does, listing the Account, the old and new destination, and, for each seat with linked folders whose Account went from loopback to remote, the folder sentence ("this folder's files leave this Mac for <host>"). A user-level confirmation never updates a project's grant set. Until epic 4 story 18 ships, `agentic accounts change` refuses a change from a loopback to a remote endpoint while any reachable project links a folder to a seat that names that Account, and its sentence names every reachable project that names the Account.

---

## H2. "Unchanged since the app wrote it" is torn across processes: apply order, lock span and intent reconciliation for the user store are undefined

- **Units:** (a) the user-level writer (CLI `add`/`change`/`remove`/`migrate`, later 4.18's Reload and 5.5's card) following AD-16's apply order — intent row, then rename the file, then commit History and the audit entries — holding `user.lock` "for that write only"; (b) every reader of the *Key use* check (`accounts test` "takes no lock", a run's start check, Doctor) and every serving process's `ConfigChangedOnDisk` classifier for the user file (AD-16: a change whose bytes hash to the latest History after-hash "was written by the app, by this or another process").
- **Scenario 1 (stuck outside change):** P1 (`agentic serve` on project A) confirms an Account change. P2 (serving project B) gets the file event between P1's rename and P1's History commit, hashes the file, finds no matching History, and marks the user file as outside-changed: every keyed call in P2 is refused and its banner says "changed outside the app". P1 then commits History, but no further file event reaches P2, so P2 stays refused until restart. A run starting in P2 or a concurrent `accounts test` in that window is refused spuriously.
- **Scenario 2 (crash, then lock-out before 5.5):** the CLI `change` crashes after the rename and before the History/`account_added` commit. AD-16 reconciles intents "at startup" — of a project store; nothing names who reconciles the user store's intents, under which lock. Until someone does, the file "has an outside change", so every CLI Account command is refused "until the owner reverts it (from story 5.5, the UI's Reload)" — no path before 5.5. If a later implementation does reconcile it, AD-15 says a reconciled intent "commits a History entry whose after-hash is the accepted file", which adopts the new `base_url` without an `account_added`: Key use then refuses, and re-running `change` with the same values is a no-op write (story 3.18 "Writer commands that change nothing say so"), so the owner can only re-confirm by changing it to something else and back.
- **Scenario 3 (double confirm):** the owner confirms the same outside change in P1's and P2's UIs; both re-read under `user.lock`; the second must not write a second `account_added` with a different `confirmation` kind, but nothing says it no-ops.
- **Tightening (AD-17 *List*, AD-16 apply order, AD-15 *Key use*):**
  > `user.lock` is an advisory `flock` on `<app data dir>/user.lock` (released when the holder dies; never a create-exclusive file), held by a user-level write across its whole apply: the fresh re-read, the intent row, the file's rename and the History, audit and `account_added`/`account_removed` commit. Every decision that a user-file change "was written by the app" — the *Key use* hash check, a serving process's classification of a file event, the CLI's outside-change refusal — reads the file's bytes and the latest History entry under that lock (a wait of at most 2 s, then a worded refusal; "takes no lock" in *Key use* means no project lock), and a process that classified the user file as outside-changed re-classifies it when the user store's `data_version` advances. A user-level intent row carries the full History and audit payload of its apply (for a confirmation, the `account_added` entry); the next process to take `user.lock` (any user-level write, any serving process's startup) reconciles a pending intent first: if the file has the intent's new hash it commits that payload as recorded, otherwise it drops the intent. `change` re-records an Account whose file entry already holds the requested values but whose latest `account_added` names another destination. A confirmation that finds, under the lock, that the file already matches the latest History and the record already names the destination writes nothing and says so.

---

## H3. `secret_ref` is an unpinned file field: a non-widening edit can send one Account's key to another Account's confirmed endpoint

- **Units:** (a) the run path of epic 2 story 6 / am-providers (and Doctor), which reads the key the Account entry references — AD-17 lists `secret_ref` as an entry field and says keys are stored under `<account-id>`, but no AD says the key is *read* only by the Account's own id; (b) epic 4 story 17's classifier and the *Key use* comparison, where the widening kinds for the user file are "adding an Account, changing its provider or `base_url`" and the record compares "provider, scheme, host, port and key state" — not `secret_ref`.
- **Scenario:** Account `cheap-gw` (`https://gateway.example/v1`, keyed) and `anthropic` are both confirmed. A sync tool or hand edit sets `cheap-gw`'s `secret_ref: anthropic`. From 5.5 this is an outside change that `is_widening` does not flag, so the single non-widening Reload applies it and commits History (AD-15 *Later confirmations*: "a non-widening Reload commits a History entry … so the hash check keeps passing"). Key state is still "stored", provider and endpoint unchanged, so Key use passes; a run path that follows `secret_ref` sends the Anthropic key to `gateway.example`. Story 2.3's `accounts test` already refuses this (`crates/am-app/src/accounts.rs`, "a key is only ever read from the entry named by the Account's own id"), but that rule lives in am-app's CLI module, not the spine, and the run path is a different crate.
- **Tightening (AD-17 *List*; AD-15 *Key use*):**
  > A key is read only from the keychain entry named by the Account's own id (service `agentic-manager`, account `<account-id>`); `secret_ref`, when present, must equal the id, and `agentic validate` reports any other value as an error at its line. One am-config predicate answers "the keychain entry this Account may use" and every key reader (`accounts test`, runs, Doctor, the capability and Source adapters) calls it; no code reads a key by the `secret_ref` value. A change of `secret_ref` is classified as a change of the Account's key state (widening when it goes from none to a value).

---

## M4. The user store is shared by live processes of possibly different binaries, but AD-7's migrator assumes exclusive use

- **Units:** (a) AD-7's migrator applied to `user.db` "under the same rules" (VACUUM INTO backup, numbered steps, `user_version`); (b) AD-32's `agentic service` LaunchAgent and a second `agentic serve`, each holding `user.db` open, one of them still the previous binary after an update (a running process keeps its old code).
- **Scenario:** the updated binary starts for project B, migrates `user.db` to v2 (new column on audit rows). The old process serving project A already has the store open — the "refuses to open a database whose version is newer" check ran at its open, not now — and later appends an `account_added` with the v1 shape, or reads the v2 audit chain with v1 code and computes a different chain hash. Two writers, two schemas, one hash chain.
- **Tightening (AD-7, AD-17):**
  > Each process that opens the user store holds a shared `flock` on `<app data dir>/user.open` for as long as it is open; migrating the user store takes that lock exclusively (refused, naming the holders from the run files, while another process holds it), and its pre-migration backup and steps run under `user.lock` too. Before each user-store write, and in the *Key use* read, a process re-reads `user_version`; if it is newer than the binary knows, the process stops writing the user store and refuses every keyed call in words ("this Mac's Accounts were upgraded by a newer version; restart the app").

---

## M5. A run pins the user file's hash but not the destination: resume after a confirmed change, and removal mid-run, are ambiguous

- **Units:** (a) the run path (story 2.6 onwards) with AD-8's resume, which pins "the user file's hash beside its project snapshot" and before each call checks "that the latest `account_added` entry still names the destination"; (b) the user-level `change`/`remove` in another process.
- **Scenario 1:** run R in P1 calls `openai`; P1 is killed; the owner runs a confirmed `change openai --base-url …`; P1 restarts and resumes R (AD-8 re-issues the in-flight call). The pinned hash names bytes no longer on disk. Implementer A re-reads the current file (hash differs from the pin, so refuses — the run fails); implementer B takes the endpoint from the current file, whose record matches, and the rest of the run goes to the new host. Both obey the text.
- **Scenario 2:** the owner `remove`s `openai` (no reachable reference — R's project is reachable but R is a fan-out branch in a process that read the references before the seat edit). The per-call check is worded "the latest `account_added` entry still names the destination" — without the "no `account_removed` after it" clause of the start check — so a literal implementation keeps sending the key for an Account the owner removed (the keychain entry stays, by design).
- **Tightening (AD-15 *Key use*, AD-8):**
  > At run start the run journals, for each Account it may call, the resolved destination (provider, scheme, host, port, key state) beside the pinned hash. Every call, including a call re-issued on resume, goes only to the journaled destination and only while the latest `account_added` entry for that id, with no `account_removed` after it, names that same destination; otherwise the call is not made, the step fails with the reason in words and its fallback applies as for any refusal. A resumed run never re-resolves an Account from the current file.

---

## M6. Re-confirming between 4.18 and 5.5, and after a lost user store or a deleted file, has no defined path

- **Units:** (a) AD-16: "from epic 5 story 5 Reload applies" an outside change of the user file, and the CLI refuses "until the owner reverts it (from story 5.5, the UI's Reload)"; (b) AD-15 *Later confirmations* and Sofia guidance (5), which name "epic 4 story 18's confirmed Reload of an outside change of the user file"; and AD-17's "a lost user store leaves … every Account to be re-confirmed" plus the Deferred row "losing it means re-confirming each Account".
- **Scenario 1:** Sofia writes 4.18 to hold and confirm the user file (per guidance and *Later confirmations*); the 5.5 builder reads AD-16 and builds a second user-file Reload confirmation. Two confirmation paths for one entity, or 4.18 ships none and the gap persists until epic 5.
- **Scenario 2:** `user.db` is lost (or a new Mac restores the app data dir without it). The file lists `openai`; no History exists, so the file "has an outside change": every CLI Account command is refused; `add openai` would be refused anyway ("an id already in the user list"); `change` with the same values writes nothing. Before 5.5 the only way out is deleting `accounts.yaml` by hand and re-adding every Account. Conversely, a hand-deleted `accounts.yaml` with History present: is the first `add` a `ConfigCreate` (no file) or refused (outside change)? Both readings fit.
- **Tightening (AD-15 interim passage, AD-16):**
  > The user file's outside-change hold and its confirmation are built by epic 4 story 18 (UI class, confirm token, through the API) and shown by epic 5 story 5's card; AD-16's "from epic 5 story 5" reads "from epic 4 story 18". Until then, `agentic accounts confirm` re-confirms the user file as it stands on disk: for each entry it shows the add's sentence and takes the typed id (as *Add*, reusing the keychain entry and naming it), and when every entry is confirmed it commits one History entry for the file's current bytes and one `account_added` per entry; it retires with the others at 5.5. A missing user file while History exists is an outside change (no key sent); `confirm` on it, or the first `add`, records the absence as a History entry before the create.

---

## M7. Proposals and Undo can touch the user file but write no confirmation record

- **Units:** (a) AD-15's one Proposal shape ("a list of typed config patches", used by the Assistant, the learning loop, imports) and AD-16's Undo ("applies the inverse diff as a new History entry"); (b) *Later confirmations*, which lists only `change`, `migrate`, 4.18's Reload, 5.5's card and "the UI Undo of a remove" as writers of `account_added`.
- **Scenario:** 4.17 types a patch over the user file's schema (it must, to classify it). The Assistant proposes "point `openai` at the company gateway"; the owner approves with the confirm token; the apply commits History but no `account_added` — so Key use refuses every call and the owner is locked out (or a builder adds a second record writer inside the Proposal apply). Likewise Undo of a `change` restores the old `base_url` — a base_url change, hence widening — but is not in the list; Undo of an add removes the Account with no `account_removed`.
- **Tightening (AD-15):**
  > No Proposal carries a patch to the user Accounts file: the typed patch for that file is constructed only by the user-level Account handlers (the interim CLI commands, story 4.18's confirmation, story 5.5's card), and a Proposal apply that would touch it is refused. Undo of a user-level Account write is itself one of those handlers: undoing an add writes `account_removed`, undoing a remove or a provider or `base_url` change is the two-step confirmation and writes `account_added`.

---

## M8. A project that still holds `models/accounts.yaml`: does it open, and how does the grant set map old definitions to new names?

- **Units:** (a) AD-17: the file is "an `agentic validate` error"; AD-16: validation errors name file and line (and by implementation block a load); (b) 5.5, which "offers the move of a project's old file" in the UI — which needs the project open — and 4.18's grant-set comparison for the move's `ConfigMigration` ("classified by comparing the old and new snapshots' grants").
- **Scenario:** after 5.5 retires CLI `migrate`, a project with the old file is served: if the validate error blocks load, there is no UI session for that project from which to offer the move (the pre-open state's decisions do not include it), so the project can never be migrated. If it loads, 4.18 compares an old grant set whose Account entries are "Accounts defined" with a new snapshot whose entries are "Accounts named", and a builder who keys them differently holds the whole project as widening after a non-widening move.
- **Tightening (AD-17 *Old project files*):**
  > A project that holds `models/accounts.yaml` opens: the file is ignored (never read for a key or a definition), its references resolve against the user list, and the open report, the banner and Doctor offer the move (the CLI's `migrate` until 5.5, then the UI). In the grant-set comparison an Account defined in the old file and named by the project maps to the same Account named, so the move's `ConfigMigration` is not widening; an Account defined there but not named is dropped without a widening.

---

## M9. Confirmations that list the project-level Account kind show only ids, which mean different things on different Macs

- **Units:** (a) department import (4.11/5.11), the sample apply (5.12/5.16) and the first-load hold (4.18), which list "present ones as the project-level widening" by id (AD-17); (b) the user list, where ids are owner-chosen per Mac — `gpt` may be OpenAI on the exporter's Mac and a gateway or a local server on the importer's.
- **Scenario:** a department exported with seats on `gpt` (OpenAI) is imported on a Mac whose `gpt` is `https://router.example`; the confirmation says "names Account gpt", the owner approves, and the department's work goes to a host the confirmation never named.
- **Tightening (AD-15, AD-17 *Export, import and backup*):**
  > Every confirmation that lists the project-level Account kind names, for each Account, its provider and endpoint host on this Mac and whether its calls leave this Mac; a missing Account is listed as missing and recorded `unresolved` in the grant set (C1).

---

## L10. Lock order between project locks and `user.lock` is unstated

- **Units:** `migrate --project` (holds the project's store lock for the whole command, then `user.lock` per add) vs 5.5's move in a serving process (holds the project, then `user.lock`) vs any future handler that holds `user.lock` while resolving a project.
- **Tightening (AD-17):** "Lock order: a project's store lock or the runtime base's lock before `user.lock`, never the reverse; `user.lock` is never held across a terminal prompt, a UI dialog or a network call."

## L11. Tests and `validate --template` must not read the developer's real Accounts

- **Units:** `just check` running `agentic validate --template` over `templates/` and `fixtures/` (missing-Account warnings resolve against the user list) vs fixtures "handed to the loader by path".
- **Scenario:** validate output and test results differ between the CI runner and the developer's Mac because the real `~/Library/Application Support/agentic-manager/accounts.yaml` is read; or a builder adds `--accounts-file`/an env var to make tests deterministic and it ships in the release binary.
- **Tightening (AD-17, AD-33):** "`agentic validate --template` resolves no Account. Tests build `RuntimeLayout` over a temporary app data dir; no flag, env var or feature selects the user Accounts file or user store in the release binary (a guard test, as for the fake terminal)."

## L12. `migrate` reuses a keychain entry whose provenance may be another project's endpoint

- **Scenario:** in the per-project stage, projects A (`openai` → api.openai.com) and B (`openai` → a gateway) shared the entry `openai` (AD-17's former C1). Migrating B first puts `openai` → gateway in the user list with A's OpenAI key; the sentence says only "the existing keychain entry is reused".
- **Tightening (AD-15 *Migrate*):** "When the id's keychain entry is reused, the sentence names every reachable project whose old file defines that id, with its endpoint, and says the key may have been stored for another of them."

## L13. Two `agentic serve` processes: a loopback Account's port can become another process's API port

- **Units:** 5.5's "never the app's own port" check (per process) vs a second `agentic serve` choosing its port later.
- **Tightening (AD-13, AD-31):** "`agentic serve` never binds a loopback port that a user-level Account names, and a call to a loopback Account is refused when the port belongs to a running agentic server (its run file)." (Related to open question (f).)

## L14. "Renaming references is a project config edit for the UI" has no path before epic 5

- **Scenario:** `migrate` refuses an id collision until the owner renames one; before epic 5 there is no UI.
- **Tightening (AD-15 *Migrate*):** "… until the owner renames one: in the UI, or before epic 5 by editing the project's files and choosing Reload."

---

## Out of scope / not raised

- Same-user processes writing the file, the user store and its audit rows together (accepted by the owner, AD-13).
- Cross-process rate limits and daily backup of the user store (Deferred by the spine).
- `base_url` path comparison and CLI confirmation of sample Accounts (Deferred by the owner).
- Pre-existing, not from this change: Local search's per-user state sits under the app data dir that AD-13's sandbox self-test requires the helpers be denied; worth a separate check by whoever owns AD-45.
