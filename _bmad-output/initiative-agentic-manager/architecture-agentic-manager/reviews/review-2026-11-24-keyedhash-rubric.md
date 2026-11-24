# Rubric review: keyed audit hashes (AD-17, AD-32, Deferred, Open Questions; 2026-11-24)

Reviewer: rubric walker (good-spine checklist). Scope: the working-tree diff of `architecture-agentic-manager.md` on 2026-11-24 (AD-17 Binds, Prevents and the new *Where each secret is materialised* clause; AD-32 Binds, Prevents and the new *Keyed audit hashes* clause; one Deferred row; two Open Questions entries) and the last six `.memlog.md` entries. Checked against the rest of the spine (AD-6, AD-11, AD-13, AD-15, AD-25, AD-26, AD-33, AD-38, AD-39, AD-42, AD-45, AD-47, RULE-20, the Design Paradigm table, Stack, Deferred) and the built code: `crates/am-store/src/audit.rs`, `crates/am-store/src/lib.rs` (`audit_entry`, `execute`, `resolve_item`), `crates/am-store/src/user.rs`, `crates/am-engine/src/toolgate.rs`, `crates/am-engine/src/run.rs`, `crates/am-domain/src/secrets.rs`, `crates/am-domain/src/audit.rs`, `crates/am-app/src/accounts.rs`, `crates/am-app/src/main.rs`, `crates/am-app/src/serving.rs`, the trust tests under `crates/am-e2e/tests/trust/`, and story 9.10's ticket text. No cargo command was run.

## Verdict

**Accept with changes.** No critical finding. The update covers owner conditions (1), (2), (3) and (5) in form, keeps the chain keyless (which matches `audit.rs`'s `entry_hash`, whose input includes `args` as text), stays inside the Design Paradigm table (am-store reaches the key through the am-domain `SecretStore` port; HMAC/HKDF are external crates, correctly held as an owner question), and gives a sound no-key rule for AD-47 and RULE-20. Two high findings should be fixed before Sofia's REPLAN for story 9.10 runs (it is held for the crate question anyway): owner condition (5) is enforced only for the free-text events that exist on the day 9.10 is built, and nothing in AD-25 leads a later builder to the rule (H1); and AD-17's new materialisation list contradicts shipped code (`agentic accounts list`) and makes two planned UI features (the Account card, Show recovery key) unbuildable by its letter (H2). The medium findings pin the remaining divergence points between the backup module, the audit module, ToolGate and Doctor.

## Checklist walk

| Check | Result |
|---|---|
| Fixes the real divergence points for the level below, misses none | Mostly. Fixed: construction (HKDF/HMAC-SHA-256), label, which hashes are keyed, arg naming, keyless chain, missing-key behaviour. Missed: how free text added after 9.10 is caught (H1), the ToolGate label's form where no `args` exist (M2), key-id and input-byte construction (M3), what counts as "free text" for hashes that are compared later (M4), how am-store gets the port and when it may create a key (M1) |
| Every rule enforceable and actually prevents its divergence | Partly. The trust test enumerates today's events by hand and never runs the no-key path, so a later bare hash or a bare fallback passes the gate (H1, M5) |
| Nothing under Deferred lets two units diverge | Yes for v1; one guard sentence recommended so no code assumes cross-run equality of keyed hashes (L5) |
| Ratifies, not contradicts, the existing code | No in one place: `accounts list` reads stored keys back for its masked column (`crates/am-app/src/accounts.rs:1177`), which the new AD-17 sentence forbids (H2). Elsewhere it ratifies: the three bare free-text hashes the AD names are exactly `note_sha256` (`lib.rs:2228`), `output_sha256` (`lib.rs:435`, `:518`, `:535`, `:1207`) and ToolGate's `unknown:<sha256>` (`toolgate.rs:84-89`); file and config hashes are computed separately (`fsx.rs`, `config_history.rs`, am-tools) |
| Owner conditions (1), (2), (3), (5) covered by the AD wording | (1) yes. (2) yes in structure; the list is incomplete (H2, M6). (3) yes; boundary of "free text" needs a criterion (M4). (5) yes in form; not enforceable for future events or the no-key path (H1, M5) |
| No contradiction with other ADs | AD-6, AD-13, AD-15, AD-45, AD-47, RULE-20 and the dependency table: consistent. AD-25: compatible but silent (H1). AD-26/AD-38/AD-39: their compared hashes are unclassified (M4). AD-17 vs itself (redaction layer) and vs AD-13 secrets: scope gap (M6). AD-33: pattern list not extended (L1) |

## The AD-25 question

**Could a builder working from AD-25 alone (adding a new audit event that carries free text) fail to find the keyed-hash rule? Yes.**

- AD-25 is the audit AD and says only "Entries hold ids, hashes and redacted arguments, never table row content or secrets, so deleting a row or run really deletes its content." Nothing qualifies "hashes", and AD-25 has no pointer to AD-32. The link is one-way: AD-32 cites AD-25, AD-25 does not cite AD-32.
- The rule lives in AD-32, titled "Runtime lives in a per-project folder outside the project; the desktop shell links the core in-process", inside a very long Rule paragraph, after the backup-encryption sentence. Nobody looking for audit rules reads there.
- The code a builder copies from invites the bare form until 9.10 lands and possibly after: `am_store::audit::sha256_hex` is `pub` with the doc "content stands in the log only as this" (`audit.rs:32-37`), and the generic `AuditEvent.hash` is documented "Content hash when relevant (sha256 hex)" (`am-domain/src/audit.rs:17-18`), which the store writes as the bare arg `sha256` (`lib.rs:588-589`, `user.rs:278`).
- Concrete future events that carry free text and are built from their own ADs plus AD-25: AD-42 review verdicts ("`note: string` (required for return and reject)"), Proposal and Approval resolutions, `ask_seat` questions (AD-46). A builder of any of them reads AD-42/AD-28 and AD-25, not AD-32.
- The release-gate test does not catch them: it "writes each free-text audit event with a canary text", a list fixed by whoever writes `audit_hash_*.rs` in 9.10.

**One cross-reference sentence in AD-25 is warranted** (wording in H1). It is a pointer, not a second home for the rule: the construction stays in AD-32.

## Findings

### H1 — high — Owner condition (5) is enforced only for today's free-text events; AD-25 does not lead a later builder to the rule

**Spine text concerned:**
- AD-25 Rule: "Entries hold ids, hashes and redacted arguments, never table row content or secrets, so deleting a row or run really deletes its content."
- AD-32 Rule: "A trust test in the release gate, `crates/am-e2e/tests/trust/audit_hash_*.rs` (AD-33), writes each free-text audit event with a canary text and finds no bare sha256 of the canary in the audit log, its export or the journal."

**Problem:** The owner asked for "a check to the release trust-gate that a release build has no bare hash of free text". The test as worded covers the events its author enumerates. An event added later (AD-42 verdict notes, Proposal or Approval notes, `ask_seat`), or added between now and 9.10 with today's public `sha256_hex`, ships a bare hash with the gate green, because the builder never read AD-32 (see the AD-25 answer) and the test never drives that event. The memlog's "AD-25 fits as written" is true about compatibility; it does not address discoverability or enforcement. AD-33 already insists that the release gate run "every file ... by directory, never a hand-kept list"; this test reintroduces a hand-kept list one level down.

**Proposed fix (spine wording):**

AD-25 Rule, append after "...really deletes its content.":
> A hash of free text (owner text, model output, a model-chosen name) in a project-store entry is AD-32's keyed audit hash, computed only in `am-store`'s audit module and named `*_hmac` with its key id, never a bare sha256; file-byte, config and chain hashes stay bare sha256 (amended 2026-11-24).

AD-32 Rule, replace the trust-test sentence with:
> Free text reaches an audit entry only as raw text in a typed writer command whose audit builder in `am-store`'s audit module keys it; the generic `hash` field of `AuditEvent` and `WriteCommand::Audit` carries file bytes only, and from the build that ships keyed hashes `am-store` exports no function that hashes text bare. Every audit action the project store can write is listed in one exhaustive match in `am-store`'s audit module that classifies each of its args as id, count, status, file hash, config hash or keyed free text (no wildcard arm, so a new action does not compile until classified, the pattern of AD-15's `is_widening`). A trust test in the release gate, `crates/am-e2e/tests/trust/audit_hash_*.rs` (AD-33), drives every action that list marks as carrying free text with a canary text and finds no bare sha256 of the canary, and no canary text, in the audit log, its export, the journal or `logs/`.

(If the owner prefers no exhaustive list, the minimum is the AD-25 sentence plus "a story that adds an audit event carrying free text adds it to `audit_hash_*.rs`" in the AD-32 sentence.)

### H2 — high — AD-17's materialisation list contradicts `agentic accounts list` and cannot be met by the Account card or Show recovery key

**Spine text concerned (AD-17 Rule):**
- "a secret's value is exposed only inside the one component named here, crosses crates only as a `SecretString` (so no raw key type enters `am-domain` or a port signature), and is never handed to a tool, logged, journaled or written anywhere but the keychain."
- "*Account keys:* ... taken in only by AD-15's Account commands (`am-app`'s hidden prompt until epic 5 story 5, then the Account card), which pass a key straight to the keychain, read a stored one back only for AD-15 *Add*'s stored-key check and show it only masked."
- "*The backup key* ... hands it out once as the recovery key, only to AD-32's Show recovery key after its confirmation."

**Problem:**
1. `agentic accounts list` reads every Account's stored key back from the keychain to show it masked (`crates/am-app/src/accounts.rs:1177`, `Ok(Some(key)) => masked(key.expose())`, with `masked` at `:1093`), and epic 2's requirement says "every read shows it masked" (`epic-accounts-model-routing.md:48`). The new sentence permits a read-back "only for AD-15 *Add*'s stored-key check". Story 9.10, which Sofia's note says "builds ... AD-17's materialisation list", would either remove the list's masked column or be flagged by review for code that is already shipped.
2. The Account card (epic 5 story 5) takes a key from the UI, and Show recovery key (story 9.10: "'Show recovery key…' (UI class, behind a second confirmation)") returns one to the UI. Both carry a raw value through an `/api/v1` JSON body, so `am-api`'s DTO and `am-server`'s handler hold it as text. `SecretString` has no Serialize (and, per its doc, no serde impls at all), and the rule says the value crosses crates only as a `SecretString` and is exposed only in the named component. The two features cannot be built without breaking the letter, and the two builders will improvise differently (one adds serde to `SecretString`, which weakens a global guarantee; another adds a raw-string DTO with no constraint).

**Proposed fix (spine wording, AD-17 Rule):**

Replace the Account-keys taker sentence with:
> taken in only by AD-15's Account commands (`am-app`'s hidden prompt until epic 5 story 5, then the Account card), which pass a key straight to the keychain; a stored key is read back only for AD-15 *Add*'s stored-key check and for the masked form `agentic accounts list` and the Account card show (at most its last four characters, and only for a key of twelve characters or more), and never shown otherwise.

Add after the backup-key sentence:
> **At the API boundary** a secret crosses as text in exactly two places, each one field of one `am-api` DTO that has no Debug and is never logged, put in an event, cached or echoed: the Account card's key on its add or confirm request (deserialised by `am-server` straight into a `SecretString`), and Show recovery key's one response body (`Cache-Control: no-store`). `SecretString` itself stays without Debug, Display and serde.

Add `am-server` and `am-api` to AD-17's Binds for these two fields.

### M1 — medium — When am-store may create the backup key, and how it reaches the keychain, is under-specified

**Spine text concerned (AD-32 Rule):** "`am-store` derives the subkey when it opens the project store's writer, first creating the backup key when the keychain holds none (a new project, a restore, a move to another Mac, a deleted entry)" and "when no key can be had (the keychain refuses, or the platform has no keychain yet, AD-47), the entry holds no hash."

**Problem:**
- The port distinguishes absent from refused (`SecretStore::get -> Result<Option<SecretString>, SecretError>`, `am-domain/src/secrets.rs`), but the AD does not tie creation to `Ok(None)` or forbid overwriting. Creating on an error, or `set` over an existing entry, replaces the key every earlier backup was encrypted under. AD-17's "otherwise never overwritten" covers Account entries only. This is not hypothetical: AD-17 leaves open whether the desktop app can read an entry the `agentic` CLI stored, and the Open Questions mention a data-protection-keychain fix. Under that fix an entry one binary created can be invisible to the other (`Ok(None)`), which would create a second backup key, so the CLI and the app would alternate keys for backups and audit hashes.
- The AD never says how am-store gets the port or the project id. `Store::open(path)` takes only a path today (`lib.rs:837`). The memlog has the answer ("am-store gets the backup key through the am-domain SecretStore port that am-app hands it"), but the AD does not. A builder could instead have am-app read the key and pass a `SecretString`, which contradicts AD-17's "only in `am-store`'s backup module, which creates it". Or a keyless production `Store::open` could survive for tests and quietly ship entries with no hash, which the no-key rule allows.

**Proposed fix (AD-32 Rule, replace the derivation-at-open sentence):**
> `am-app` hands the project store's writer open the project id and the `Arc<dyn SecretStore>` port (am-store links no `am-secrets`); production code has no writer open without them, and tests pass `FakeSecretStore`. At open the backup module reads `backup-key:<project-id>`: on `Ok(Some)` it derives the subkey; on `Ok(None)` it creates the key with `set` on the missing entry, audits the creation by `Actor::System` with the latest key id the audit log holds, and derives; on any error it creates and replaces nothing, and the writer opens with no subkey. A backup-key entry is never overwritten or deleted by the app except by Settings' explicit key actions. Epic 11's signed-build keychain check (AD-17) covers the backup-key entry: one binary must never create a second key because it cannot see another binary's entry.

### M2 — medium — The ToolGate label does not fit the "args hold a key id ... arg name says it is keyed" rule, and its no-key form and hand-off are unstated

**Spine text concerned (AD-32 Rule):** "a model-chosen tool name no manifest holds (ToolGate's `unknown:` label, AD-11, which the journal and Inbox words carry too). Each such entry's args hold a key id beside the keyed hash ... the hash's arg name says it is keyed (for example `note_hmac`, `output_hmac`), never `*_sha256`." AD-17: "other crates, `am-engine`'s ToolGate among them, get only the keyed hash text."

**Problem:** The label does not sit in an arg. It is in the audit entry's `target` (`format!("tool:{tool}{step}")`, `toolgate.rs:327-333`, with `args: Default::default()`), in the journal's `tool_calls.name` column (`run.rs:1294`), in refusal words (`run.rs:274`, `tool_words`), and in Inbox words, none of which has an args map for a key id or a keyed arg name. With no key, "the entry holds no hash" leaves the label's form open (`unknown`? `unknown:`?). `label_of` is a free `pub fn` with no key today (`toolgate.rs:84`), and the AD does not say how am-engine gets keyed text (a handle from the open store, a new am-domain port, or raw names passed into the writer). The 9.10 builder and any later caller of `label_of` can choose differently.

**Proposed fix (AD-32 Rule, after the free-text sentence):**
> ToolGate's label is self-describing wherever it is carried (audit target or words, journal row, Inbox words): `unknown:<key id>:<keyed hash>`, or `unknown` alone when no key can be had. `am-store` gives callers that must label text before a write one opaque, cloneable hasher from the open writer, which keys a text and returns the hash with its key id, or nothing; it exposes no key, derives nothing itself and has no Debug of its state. ToolGate and the engine's words take it at construction; nothing else in `am-engine` hashes text.

### M3 — medium — The construction is named but its bytes are not pinned; "key id derived from the subkey" allows a leaking prefix

**Spine text concerned (AD-32 Rule):** "derived from `backup-key:<project-id>` by HKDF-SHA-256 (no salt) with the fixed context label `audit-hash-v1` as its info ... the label and the derivation never change" and "the id is derived from the subkey, never from the backup key".

**Problem:** "Derived from the subkey" permits `hex(subkey[..4])`, which writes 32 bits of the key into every entry. It also leaves the key id unstable across builds, so later key-id comparisons (the creation entry's "earlier key id", Doctor's "copies made under an earlier key") break. The HKDF input is unpinned: the backup key's format depends on the cipher Deferred to 9.10 (an `age` identity string or 32 raw bytes, possibly hex- or base64-encoded in the keychain), so the stored text and the decoded bytes give different subkeys. A later re-encoding would silently orphan every keyed hash despite "the derivation never change". The HMAC message is unpinned too (trimmed? NFC? the `Option` of an absent note?), which the "can be matched against a text" use depends on.

**Proposed fix (AD-32 Rule, append to the derivation sentence):**
> HKDF's input keying material is the keychain entry's value exactly as stored (its UTF-8 bytes), and its output is 32 bytes. A keyed hash is HMAC-SHA-256 under the subkey over the text's exact UTF-8 bytes (no trimming or normalisation), written as 64 lowercase hex characters; an absent text has no hash. The key id is the first 16 lowercase hex characters of HMAC-SHA-256 under the subkey over the bytes `audit-key-id-v1`, never a prefix or plain hash of any key. These bytes, like the label, never change.

### M4 — medium — "Free text" has no criterion; hashes that other ADs compare later must stay bare, and only AD-30's are named

**Spine text concerned (AD-32 Rule):** "Every hash of free text in the project store's audit log (AD-25) is HMAC-SHA-256 under that subkey" and "File-byte hashes (`folder.read`, `folder.write`, AD-30), config History's before and after hashes (AD-16, checked against stored bytes) and the chain's own hashes stay bare sha256".

**Problem:** Several other ADs hash text that is neither owner text nor model output and compare it later: AD-26 ("Approval stores a hash of the spec and its tool descriptions ... On every connect the descriptions are re-hashed; a mismatch disables the server"), AD-38's skill Proposal ("the full text and sha256 of every file it would write"), AD-39's source-record and dedupe content hashes, and AD-12's archive sha256. If their audit entries carry these hashes, a builder can read MCP descriptions or `SKILL.md` as "free text" and key them. After a key change (restore, move, deleted entry, which AD-32 now makes routine) every comparison against an earlier keyed value fails: false MCP re-approvals, or skill applies refused. Others will leave them bare. The bare list names only AD-30's operations.

**Proposed fix (AD-32 Rule, replace the bare-hash sentence):**
> "Free text" means text an owner or a model wrote: notes, outputs, questions, verdict notes, model-chosen names. A hash the app compares later against bytes it holds stays bare sha256, because a key change must not break the comparison: file bytes (`folder.read`, `folder.write`, AD-30; file records and source-record content, AD-39; skill files and the import archive, AD-12, AD-38), config History's before and after hashes and snapshot hashes (AD-16), MCP spec and description hashes (AD-26), and the chain's own hashes. A new hash is classified by this rule in the audit module's list (H1).

### M5 — medium — The trust test never runs the no-key path or checks the keyed value, so a bare fallback or an omitted hash passes

**Spine text concerned (AD-32 Rule):** "from the build that ships keyed hashes nothing falls back to a bare sha256 of free text: when no key can be had ..., the entry holds no hash" and the trust-test sentence quoted in H1.

**Problem:** The only stated oracle is "no bare sha256 of the canary". An implementation that falls back to bare sha256 when the keychain refuses passes, because the test runs with a key. An implementation that writes no hash even with a key also passes. The label may also reach tracing logs (`logs/`, the trace export), which the test does not scan. Separately, the existing trust tests assert the opposite of the new rule (`crates/am-e2e/tests/trust/rule_16_audit_log.rs:450` asserts `note_sha256 == sha256_hex(NOTE_CANARY)`; `rule_15_inbox.rs:334` reads `note_sha256`), so 9.10 must invert a RULE-16 trust test, and nothing in the spine sanctions that.

**Proposed fix (AD-32 Rule, append to the trust-test sentence):**
> It runs twice: with `FakeSecretStore` holding a backup key, where each free-text arg is present as `*_hmac` with the key id and equals HMAC-SHA-256 under the subkey derived from that key (`audit-hash-v1`); and with a store whose `get` fails, where each such arg is absent, no key is created and nothing is bare. `rule_16_audit_log.rs` and `rule_15_inbox.rs` change with story 9.10 from asserting the bare hash to asserting this.

### M6 — medium — The materialisation list omits Doctor's key check and the redaction layer, and its scope against AD-13's secrets is unclear

**Spine text concerned (AD-17 Rule):** "a secret's value is exposed only inside the one component named here ... and is never ... written anywhere but the keychain", next to "A tracing and audit redaction layer replaces registered secret values and known key patterns." AD-32: "Doctor warns when it is missing"; story 9.10: "the Doctor disk row ... turns red when the backup key is missing from the keychain".

**Problem:**
- Doctor (R48: `am-engine` health checks, `am-server`) can check the key's presence only through `SecretStore::get`, which returns the value. That materialises the backup key outside the backup module.
- The redaction layer must hold registered values to replace them (AD-26 registers MCP env values; AD-45 says the SearXNG environment values, `SEARXNG_SECRET` included, are "each value registered for redaction"). So the Local search secret is in the logging layer too, not "only in `am-services`".
- The clause says "a secret", but the CLI token is written to a file by design (AD-13; `crates/am-app/src/serving.rs:195`), and the session secret and pairing code are AD-13 secrets outside the keychain. Read literally, the clause forbids AD-13.

**Proposed fix (AD-17 Rule):** open the clause with "a keychain secret's value (AD-13's session secret, CLI token and pairing code follow AD-13)". Add to the list:
> *Doctor* learns the backup key's state (present, missing, refused, its key id) only from `am-store`'s backup module, and the Local search secret's only from `am-services`; it never reads an entry. *The redaction layer* holds registered values only to replace them, and they are registered by the component that holds them.

### L1 — low — AD-33's trust-test pattern list lacks `audit_hash_*`

**Spine text (AD-33):** "`rule_*`, `connectors_*`, `local_search_*`, `capabilities_*`, `taint_*`, `widening_*`, `sandbox_*`, `skill_import_*` (AD-12, AD-38; amended 2026-11-22, owner) and the cross-department `engine/*` suites."

The gate runs by directory, so nothing fails, but the list is what AGENTS.md's trust-test line is refreshed from, and `skill_import_*` was added when it was introduced. **Fix:** insert "`audit_hash_*` (AD-32; amended 2026-11-24, owner)" after `skill_import_*`.

### L2 — low — "The backup module derives the subkey" vs "a type private to the audit module"

**Spine text (AD-17):** the backup module "derives the audit-hash subkey from it"; "*The audit-hash subkey* (AD-32): only in `am-store`'s audit module, as a type private to that module".

The backup module cannot build a value of a type private to another module. **Fix:** "...and hands it, as a `SecretString`, to the audit module's one constructor, which derives the subkey" (and drop "derives the audit-hash subkey from it" from the backup module's list).

### L3 — low — Sofia's guidance misses stale ticket text and the tests 9.10 must invert

**Spine text (Open Questions, For Sofia 2026-11-24):** "...the stale doc comment in `crates/am-domain/src/secrets.rs`...".

Also stale: epic 2's requirement text ("is materialised only inside am-net request building", `tickets/epic-accounts-model-routing/epic-accounts-model-routing.md:38`) and story 2.5's description (`tickets/epic-accounts-model-routing/tickets/tickets.toml:101`). Tests asserting the bare form: `crates/am-e2e/tests/trust/rule_16_audit_log.rs:450`, `rule_15_inbox.rs:334`, `crates/am-engine/tests/toolgate.rs:224`, `crates/am-store/tests/inbox_resolve.rs:511`, `:1331`, `crates/am-store/tests/audit.rs:268`. **Fix:** append to the Sofia entry "epic 2's R text and story 2.5's description are restated to AD-17's list; story 9.10's ticket names the trust and unit tests above that change from the bare to the keyed form."

### L4 — low — The Account-key use list reads as closed but omits keyed search providers and the spike commands

**Spine text (AD-17):** "used only in `am-net` request building (model calls, `accounts test`, crawl-service and API Sources, AD-39)".

Keyed web-search providers (AD-45 "Brave, Tavily, Exa, Serper ... are Accounts"; AD-46 provider swap) are missing. `agentic spike c7` exposes a keychain canary in `am-app` (`crates/am-app/src/main.rs:596`), and c1/c4 read `spike-anthropic` (Open Questions 2026-11-22). **Fix:** "(model calls, `accounts test`, keyed capability providers, AD-46, and crawl-service and API Sources, AD-39)", and either list the spike self-tests as an exception that leaves the release binary or point to that Open Question.

### L5 — low — Two statements of fact that should be rules

**Spine text (AD-32):** "the user store's chain holds no hash of free text (AD-17)". **Deferred:** "Per-run salt for keyed audit hashes, deleted with the run ... Owner, later".

The user store has no key, so a later free-text field there must be left out, not hashed. A future salt changes equality across runs. **Fix:** "the user store's chain never holds a hash of free text (it has no key); a free-text field is left out of its entries", and add to the Deferred row: "until decided, no code relies on a keyed hash being equal across runs."

## Checked and consistent

- **AD-6 single writer:** keying happens in the writer transaction for notes and outputs (`resolve_item`, `audit_entry`); derivation at writer open is under the project lock, so two processes cannot both create a key for one project.
- **AD-13:** no helper or child gets the key; the subkey stays in the main process.
- **AD-15:** no new config field, so `is_widening` is unaffected.
- **AD-25:** both chains, append-only, keyless verification. `entry_hash` covers `args` text (`audit.rs:41-58`), so the "verifies with no key" claim is true of the code.
- **AD-45:** the Local search secret's owner and launch-only use match (apart from M6's redaction point).
- **AD-47:** the no-key path covers the Linux job (`SecretError::NotSupported`).
- **RULE-20 and the zero-key golden run:** the app's own backup key is not an Account key; a missing key never blocks a run.
- **Design Paradigm:** am-store → am-domain only, via the `SecretStore` port; the new crates are external, not internal edges.
- **Deferred cipher row:** consistent; the AD wording is independent of the crate choice once M3 pins the input bytes.
- **Owner conditions (1)–(3), (5):** each has a sentence in AD-17 or AD-32. (4) and (6) are in the Sofia entry.
