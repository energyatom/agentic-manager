# Adversarial review: keyed audit hashes, AD-17 materialisation list and AD-32 clause (2026-11-24)

- **Reviewed:** working-tree diff of `architecture-agentic-manager.md` on 2026-11-24 (AD-17 *Where each secret is materialised*; AD-32 *Keyed audit hashes*; the Deferred per-run-salt row; the Open Question on the HKDF/HMAC crate; the For-Sofia line) and the last six `.memlog.md` entries, read against AD-6, AD-7, AD-10, AD-11, AD-15, AD-25, AD-30, AD-33, AD-45, AD-47, the Deferred product-name row, and the built code in `crates/am-store/src/{audit.rs,lib.rs}`, `crates/am-engine/src/{toolgate.rs,run.rs}`, `crates/am-domain/src/{secrets.rs,audit.rs}`, `crates/am-testkit/src/lib.rs` and the trust tests in `crates/am-e2e/tests/trust/`. Tickets: epic 9 stories 10 and 19, epic 11 story 12, epic 12 story 16.
- **Scope:** how the decision is written. The owner decision (HMAC-SHA-256 under an HKDF subkey of the per-project backup key; file and config hashes bare; key created at writer open; no bare fallback; null hash without a key) is taken as final. Every fix stays inside it, mostly inside AD-17 and AD-32; items that would change the decision are marked **owner call**. No new dependency is proposed.
- **Verdict:** **amend before Sofia's REPLAN and before story 9.10's plan.** 1 critical, 9 high, 6 medium, 5 low. The decision is sound; the wording leaves the key's lifecycle (who creates it, when, on which keychain answer), the keyed-hash API the engine calls, and the gate tests under-specified, and one existing release-gate trust test now contradicts the new one.

| # | Severity | Finding |
|---|---|---|
| 1 | critical | Epic 11 story 12's keychain-service rename and "create the backup key at writer open" together delete the only key every backup was made under |
| 2 | high | "The keychain holds none" versus "the keychain refuses" is undefined, so a locked or access-denied keychain mints and writes a second key; an access prompt blocks writer open |
| 3 | high | Doctor's "missing key" row, 9.10's verify and a mid-session deletion contradict the key created at open; backups and hashing can run under two keys |
| 4 | high | Two owners of the key material: the backup module "derives" a subkey whose type is private to the audit module; how am-store gets the key and project id is in the memlog only |
| 5 | high | The engine's `unknown:` label has no API, no format and no no-key form; a second labeller and the public `sha256_hex` stay bare |
| 6 | high | A generic `hash` slot and free-form `args` let any later story write a bare hash of free text; the store cannot tell |
| 7 | high | Existing release-gate trust tests (RULE-16, RULE-15 Inbox) assert the bare hash is present; `audit_hash_*.rs` asserts it is absent |
| 8 | high | `audit_hash_*.rs` passes when no hash is written at all, scans only three sinks, and has two owners |
| 9 | high | 9.19's and 12.16's verifies (and epic 2's Key-use tests) forbid the keychain traffic writer open now does |
| 10 | high | Show recovery key has no lawful path over `/api/v1`; redacting the key by value or by a hex pattern breaks AD-17 or the keyless export check |
| 11 | medium | A model's output written to a file is hashed bare as a "file hash" (**owner call**) |
| 12 | medium | One subkey, no domain separation: the model-chosen tool-name label is a chosen-input oracle for note and output hashes |
| 13 | medium | The key id has no format and is defined only for audit entries; backups need one, inviting a fingerprint of the backup key |
| 14 | medium | HKDF's input is not pinned to bytes: stored text, decoded bytes and a typed recovery key can differ |
| 15 | medium | Writer-open order against AD-7's pre-migration copy, and no record when a restored chain's last key differs from the keychain's |
| 16 | medium | A null keyed hash means both "no note" and "no key" |
| 17 | low | A random key per test run makes golden snapshots that hold an `unknown:` label nondeterministic |
| 18 | low | A restore on another Mac always rotates; no path takes the owner's recovery key in (**owner call**) |
| 19 | low | Stale words outside AD-17/AD-32 (AD-33's suite list, AGENTS.md, three code doc comments) |
| 20 | low | "Reveal it once" and orphaned `backup-key:*` entries are unspecified |
| 21 | low | The leak scan omits the subkey; the user store's "no free-text hash" is descriptive, not a rule |

Security-intent probes, in short: bare sha256 of free text still lands in the audit log through finding 11 (model output written by a file tool), finding 6 (any caller of `WriteCommand::Audit { hash }` or `AuditEvent.hash`), finding 5 (`run.rs`'s `tool_words` labels with the public bare `label_of`), and the RULE-16 test keeps asserting it (finding 7). The backup key itself is pushed towards hashing by finding 13 (no shared key id for backup copies) and finding 10 (redaction by value). The subkey or backup key reaches a raw type through findings 4 (`[u8; 32]` between modules, or a `SecretString` read in am-app) and 10 (an am-api DTO field).

---

## 1. Critical: the keychain rename deletes the key every backup was made under

**Two units, each obeying the letter.**
- *Unit A, epic 11 story 12 "Public name applied everywhere"* (and the Deferred row "Product name in binary, bundle id, keychain service ... renaming the keychain service ... needs a one-time migration"): "a one-time migration that moves stored keys, backup keys included, from the agentic-manager service and leaves none behind". Verify: "first launch moves them ... and the old entries are gone".
- *Unit B, AD-32 as amended:* "`am-store` derives the subkey when it opens the project store's writer, first creating the backup key when the keychain holds none (a new project, a restore, a move to another Mac, a deleted entry)".

**Clash.** The renamed binary reads `backup-key:<project-id>` under the new service. If any writer opens before the move has run for that entry (the LaunchAgent `agentic serve` registered by AD-32 starts at login; a lazy per-project move; `agentic gate` or `agentic run` run first), Unit B finds no entry and mints key B. Unit A then either overwrites B with A (entries hashed since open are now under a key nobody holds) or keeps B and, "leaving none behind", deletes A. In the second case every daily and pre-migration copy is unrecoverable unless the owner once revealed A's recovery key, and nothing warns. Separately, AD-17's new list lets only `am-store`'s backup module touch the backup key, so Unit A cannot be built in `am-app` or `am-secrets` to the letter at all.

**Fix (AD-17, under *The backup key*):** "A keychain service rename (Deferred: product name) moves every entry, backup keys included, inside `am-secrets` as opaque values that no code exposes (the one exception to this list), before any project store opens in that process. It writes an entry under the new service only where none exists, reads it back equal, and only then deletes the old entry; an entry under the new service that differs from the old one stops the move for that entry, deletes neither, and Doctor names both. Writer open never creates a backup key while an entry for that project exists under the old service." Sofia carries the ordering into epic 11 story 12's verify (a LaunchAgent-started serve before the move leaves both keys and every copy decryptable).

## 2. High: "holds none" versus "refuses" is undefined; creation can replace a key, and a prompt blocks open

**Two units.**
- *Unit A, `am-secrets` (AD-17: `keyring-core` with `apple-native-keyring-store`):* the keychain answers item-not-found, interaction-not-allowed, auth-failed, user-cancelled, or holds the call on an access prompt. The port (`crates/am-domain/src/secrets.rs`) has only `Ok(None)` and `Err(SecretError::Store(String))`; nothing says which answer is which.
- *Unit B, AD-32:* create "when the keychain holds none"; hold no hash "when no key can be had (the keychain refuses ...)"; "a missing or unreadable key never blocks a resolution, a model call or a refusal".

**Clash.** A builder who maps any read failure to "none" (or meets AD-17's own open question: the desktop app reading an item the `agentic` CLI created, whose access list may refuse it) creates key B and `set`s it. AD-15 already records that the pinned crate's in-place update decrypts and replaces the old item, so key A is replaced: every copy is lost. Conversely, an access prompt with no time limit holds writer open, which blocks every resolution and model call, against "never blocks".

**Fix (AD-32, after "first creating the backup key"):** "The backup key is created only after the keychain answers that no entry exists (`SecretStore::get` returning `Ok(None)`); `am-secrets` returns `None` for item-not-found only and an error for every other answer (locked, interaction not allowed, cancelled, denied, any failure). Creation is a `set` on a missing entry, never an update, and never replaces or deletes an entry. The read at writer open has a 10-second limit (as AD-32's identity tools); a timeout or error means no key can be had for that session (no keyed hash, no backup, Doctor red), never a creation." Add to AD-17's epic-11 signed-build check: "and the same for `backup-key:*` entries created by the CLI and read by the desktop app and the LaunchAgent".

## 3. High: Doctor's row, 9.10's verify and a mid-session deletion contradict creation at open

**Two units.**
- *Unit A, AD-32's unchanged backup sentence* "Doctor warns when it is missing", and story 9.10: "the Doctor disk row ... turns red when the backup key is missing from the keychain"; verify "deleting the keychain key makes Doctor's disk row red with its sentence".
- *Unit B, AD-32 as amended:* the key is created at writer open when the keychain holds none, "a deleted entry" included; the subkey is derived once at open; "Doctor's backup row says that copies made under an earlier key need its recovery key". AD-17 lists "creates it" among the backup module's jobs without saying when.

**Clash.** After a restart the key always exists, so Unit A's row can only be red between a deletion and the next open, and then turns green although every kept copy needs the lost key. Mid-session, one builder's backup module re-reads the keychain per daily backup and, finding none, follows AD-17's "creates it": the backups move to key B while the audit module keeps hashing under A's subkey until restart (two keys in one session). Another builder caches the key: daily copies keep being encrypted under A after A has left the keychain, and after the next open Show recovery key reveals B, so those copies can never be opened.

**Fix (AD-32, replacing "Doctor warns when it is missing" and extending the new clause):** "The backup key is read or created once, at writer open, and held for that session; no other path (a backup, Doctor, `agentic backups decrypt`) creates one. Each backup first checks by key id that the keychain still holds the session's key and, when it does not, takes no copy. Doctor's backup row is green when the key is held and every kept copy was made under it; amber when some kept copies were made under an earlier key id (naming them and saying they need that key's recovery key); red when no key can be had or the keychain no longer holds the session's key ('reopening creates a new key; copies made under the earlier key need its recovery key')." Sofia rewords 9.10's verify: "deleting the key while the app runs turns the row red and stops backups; after reopening the row is amber naming the earlier copies".

## 4. High: two owners of the key material, and how am-store gets it is in the memlog only

**Two units.**
- *Unit A, AD-17's backup-key item:* the backup module "derives the audit-hash subkey from it".
- *Unit B, AD-17's subkey item:* the subkey lives "only in `am-store`'s audit module, as a type private to that module".

**Clash.** Neither module can construct the other's private type. The backup-module builder returns `[u8; 32]` or `Vec<u8>` to the audit module (a raw key crossing a module boundary, which the list exists to prevent); the audit-module builder derives the subkey itself and so reads the backup key outside the backup module. Further, the spine never says how `am-store` obtains the key or the project id; the memlog's "through the am-domain SecretStore port that am-app hands it" is not in the AD. A builder may as well have `am-app` read `backup-key:<id>` and pass a `SecretString` into `Store::open`, materialising the key in `am-app`, and 9.10's Doctor row and `agentic backups decrypt` ("key from the keychain") will each call `SecretStore::get("backup-key:...")` from `am-app`.

**Fix (AD-17, replacing the two items with one):** "*The backup key and the audit-hash subkey* (AD-32): only in one private `am-store` key module, which alone calls `SecretStore` for `backup-key:*` entries, creates the key, derives the subkey and holds both for the open writer. The backup module asks it to encrypt or decrypt, the audit module to hash; neither receives key bytes. `am-app` passes `Arc<dyn SecretStore>` and the project id to the writer open, with no default (as `Actor`), so there is no keyless writer. Nothing outside `am-store` reads a `backup-key:*` entry: Doctor gets the key's state (held with its key id, none, refused) and `agentic backups decrypt` gets decryption only through `am-store`, the latter handing in a typed recovery key as a `SecretString`."

## 5. High: the engine's `unknown:` label has no API, format or no-key form

**Two units.**
- *Unit A, `am-engine` ToolGate (AD-11, built):* `ToolGate::label` is synchronous and runs inside `check`/`admit` before any write; a refusal goes through the `AuditSink` port with `target: tool:<label>`, `outcome: <words>` and empty `args`; the label is journaled by `RecordToolCall { tool }` and words the step failure. `run.rs`'s `tool_words` labels a second time through the public free function `label_of`, which calls the public `am_store::audit::sha256_hex`.
- *Unit B, AD-32/AD-17:* other crates "get only the keyed hash text"; "each such entry's args hold a key id beside the keyed hash"; nothing more.

**Clash.** With text only, the gate cannot put a key id in args (Unit B's own rule). If the store instead keys the raw name at execute time, the gate does not know the label it must word in the refusal and journal. Nothing says where the key id goes in the journal or Inbox words, which have no args, or what the label is with no key (empty, `unknown:`, or a fallback to the bare hash that AD-32 forbids). Old `unknown:<sha256>` and new `unknown:<hmac>` both end in 64 lowercase hex (the shape the store's `is_sha256_hex` checks), so neither a reader nor a test can tell them apart by form. And unless the spine names it, `run.rs`'s second labeller keeps writing bare hashes into failure words.

**Fix (AD-32, after the tool-name sentence):** "The open writer hands out one cloneable keyed-hash handle; `am-engine` labels a model-chosen name only through it, in ToolGate and the run's words alike (the bare `label_of` and `sha256_hex` stop being public). It returns the key id and the hex HMAC, or nothing when no key can be had. The label is `unknown:<key id>:<hex HMAC>`, and `unknown:unkeyed` when there is no key; an audit entry carrying the label also holds `key_id` in its args."

## 6. High: a generic hash slot lets any later story write a bare hash of free text

**Two units.**
- *Unit A, the built audit API used by every story:* `AuditEvent.hash` ("Content hash when relevant (sha256 hex)") and `WriteCommand::Audit { hash, args: BTreeMap<String, String> }` store whatever hash the caller computed as `sha256`, and free-form args.
- *Unit B, AD-32:* "Every hash of free text in the project store's audit log ... is HMAC-SHA-256", classified by meaning, with examples.

**Clash.** Stories built before 9.10 that add free-text events (loop-back and review notes, AD-36/AD-42; `ask_seat` questions, AD-46; Question answers; Proposal and Budget reject notes) follow the visible `*_sha256` pattern, and 9.10 must find them by hand. After 9.10, a story can still pass `sha256_hex(text)` through `hash` or `args`; the store sees a valid 64-hex value and cannot know it hashed text. The rule exists only in prose.

**Fix (AD-32, one sentence):** "An audit argument that hashes content is typed by what it hashes: bytes (a file's or a config file's bytes: bare sha256, computed from the bytes by `am-store` or the file adapter) or text (owner or model text: keyed, computed only by `am-store`'s key module from the text it is given). Every audit action declares its hashed arguments in one exhaustive `am-store` table, as `is_widening` classifies config fields, so a new action does not compile until classified, and `audit_hash_*.rs` iterates that table."

## 7. High: existing release-gate trust tests require the bare hash the new test forbids

**Two units, both run by the release gate by directory (AD-33).**
- *Unit A, `crates/am-e2e/tests/trust/rule_16_audit_log.rs` (RULE-16, AD-25 "Entries hold ids, hashes"):* asserts the export contains `sha256_hex(CONTENT_CANARY)` (a step output) and `sha256_hex(SECRET_CANARY)` (a secret written as step output), and that `inbox.resolved` holds `note_sha256 == sha256_hex(NOTE_CANARY)`. `rule_15_inbox.rs` reads `note_sha256`; `crates/am-engine/tests/toolgate.rs` asserts `unknown:<sha256>`; `crates/am-store/tests/{audit,inbox_resolve}.rs` assert `output_sha256` and `note_sha256`.
- *Unit B, `audit_hash_*.rs` (AD-32):* "finds no bare sha256 of the canary in the audit log, its export or the journal".

**Clash.** Both obey their AD; they cannot both pass. Project policy forbids editing a trust test to make code pass unless the spec says so, and the For-Sofia line names only a doc comment in `secrets.rs`. A side tension: AD-15 says a key's "hash" is never recorded, while RULE-16 asserts the hash of a model-echoed secret is; a keyed hash of it is still a hash of it.

**Fix (For-Sofia line, and one AD-32 sentence):** "Story 9.10 rewrites the hash assertions of the RULE-16 and RULE-15 Inbox trust tests to the keyed form (the export holds the HMAC of the canary under the fixed test key's id and no bare sha256 of it), with the am-store and am-engine unit tests of `note_sha256`, `output_sha256` and `unknown:`." The architect states whether a step output is redacted (AD-17 redaction layer) before it is hashed, so AD-15's "no hash of a key" and RULE-16 agree.

## 8. High: the release-gate test passes when nothing is hashed, looks in three places, and has two owners

**Two units.**
- *Unit A, AD-32:* the test "writes each free-text audit event with a canary text and finds no bare sha256 of the canary in the audit log, its export or the journal"; "when no key can be had ... the entry holds no hash"; "built by epic 9 story 10".
- *Unit B, the For-Sofia line:* "the release trust gate story carries `audit_hash_*.rs`" (epic 11).

**Clash.** A build whose key read always fails (finding 2's mapping, a writer opened without a `SecretStore`, `am-secrets` returning "not supported" on the Linux job) writes no hashes and passes the test, so the owner's condition 5 ("a release build has no bare hash of free text") is not tested. The label also lands in Inbox items, `logs/`, other store tables, the trace, lineage and company exports, none of which are scanned. And two stories own the file: 9.10 must write it failing-first (tests-first policy), and an epic 11 story cannot write it failing-first once 9.10 has shipped.

**Fix (AD-32, replacing the test sentence):** "Story 9.10 writes `crates/am-e2e/tests/trust/audit_hash_*.rs` failing-first, and the release gate runs it by directory. With a fixed test backup key in `FakeSecretStore`, it writes each action the hash table (finding 6) marks as text with a canary and asserts the argument equals the expected HMAC under that key id; with a refusing store, that the argument is absent and Doctor's row is red; and it finds neither the canary nor its bare sha256 in any table of `state.db`, `logs/`, or the audit, trace, lineage and company exports. The signed-build check (epic 11) repeats one Inbox resolution against the real keychain and finds a keyed hash."

## 9. High: existing verifies forbid the keychain traffic writer open now does

**Two units.**
- *Unit A, ticket verifies written to AD-17's Account rules:* epic 9 story 19 (a) "FakeSecretStore records no call", "in every case ... FakeSecretStore records no delete or set before an Account's own confirmation", while "a run refuse[s] each Account before any Keychain read"; epic 12 story 16 "the zero-key golden run passes with zero keychain reads that return a key" (its kill-and-resume variants reopen the writer); epic 2's Key-use tests ("refused before any Keychain read").
- *Unit B, AD-32:* every writer open reads `backup-key:<project-id>` and, on a fresh project, sets it.

**Clash.** The run in 9.19 (a) records a get and a set; the zero-key run's second open reads a key back. Each builder either breaks the other's test or "fixes" it by skipping the key in run paths, which brings back null hashes.

**Fix (AD-17, after the materialisation list):** "Statements about keychain reads and writes for Accounts (Key use's 'before any Keychain read', Doctor's 'reads no key', RULE-20's zero-key run) count only entries named by an Account id. Writer open's read or creation of `backup-key:<project-id>` is expected: the zero-key run starts with no entry at all and ends with exactly one backup key and no Account entry." Sofia tightens 9.19's and 12.16's verifies to "no call naming an Account id".

## 10. High: Show recovery key has no lawful path, and redaction breaks either AD-17 or the keyless export check

**Two units.**
- *Unit A, story 9.10's Settings › Backups "Show recovery key…"* (UI class), which talks to the core only over `/api/v1` with ts-rs DTOs (Conventions), and AD-17's "A tracing and audit redaction layer replaces registered secret values and known key patterns".
- *Unit B, AD-17 as amended:* a secret "crosses crates only as a `SecretString`"; `SecretString` has no `Serialize`; the backup key lives only in `am-store`.

**Clash.** The builder adds a `String` field to an `am-api` DTO (a raw key in a generated type, possibly traced by the HTTP layer) or sends it on the WebSocket. Registering the backup key by value puts it in `am-app`'s logging layer, outside `am-store`. Registering a pattern instead, when the key's text is 64 hex, also redacts every sha256 and HMAC argument in an export, and `agentic audit export`'s keyless chain check (which hashes the args as text) then fails.

**Fix (AD-17, under the backup key):** "The recovery key leaves `am-store` only as a `SecretString` to one `am-server` handler, which writes it straight into that response body after AD-32's confirmation: no `am-api` DTO or ts-rs type, never on the WebSocket, `Cache-Control: no-store`, not traced. The key's one text form starts with a fixed prefix (for example `am-backup-key-1-`), which the redaction layer's key patterns match, so it is redacted without being registered by value; no hash or key-id format matches that prefix."

## 11. Medium (owner call): model output written to a file is hashed bare as a "file hash"

**Two units.**
- *Unit A, AD-30 and AD-46:* `write_file` (local-write) and the Files workspace; "every list, read and write is audited with root, path and hash"; AD-10's workspace sidecar "keyed by path and content hash".
- *Unit B, AD-32:* model output is keyed; "File-byte hashes (`folder.read`, `folder.write`) ... stay bare".

**Clash.** A seat that writes its answer to a file puts the bare sha256 of model output into the audit log, where it stays after the file and the run are deleted; owner condition 3 ("key ... model output") and "file hashes stay bare" overlap here. The canary test passes only because it never routes the canary through a write.

**Owner call.** (a) Key the hash of bytes a write tool takes from a model (`bytes_hmac`), keeping reads and config bare (reviewer's lean); or (b) accept it as a residual, say so in AD-32, and have `audit_hash_*.rs` name the exclusion.

## 12. Medium: one subkey and no domain separation make the tool-name label an oracle

**Two units.** *Unit A:* the `unknown:` label, whose input the model chooses (or content steering the model) and whose output is shown in Inbox words, `agentic gate`'s terminal, step-failure words and the journal. *Unit B:* `note_hmac` and `output_hmac` under the same subkey and the same construction.

**Clash.** A model can obtain HMAC(subkey, x) for any x it names, which is exactly the value to compare with an Inbox note's hash in an exported log; equal texts also link across kinds (an output equal to a note).

**Fix (AD-32, one clause, derivation unchanged):** "The HMAC input is the kind's fixed name (`inbox-note`, `model-output`, `tool-name`, one per text row of the hash table), a NUL byte, then the text's UTF-8 bytes."

## 13. Medium: the key id has no format and no use outside audit entries

**Two units.** *Unit A, the backup module (9.10):* needs a per-copy key identity so Doctor can say which copies "were made under an earlier key" and `agentic backups decrypt` can say which key a copy needs. *Unit B, AD-32:* defines a key id only for audit entries ("derived from the subkey, never from the backup key"), with no format.

**Clash.** The natural per-copy identity is a fingerprint of the backup key, which makes the backup key hash something (forbidden); a second, unrelated id cannot be matched to the key-change audit entry.

**Fix (AD-32):** "One key id names a backup key everywhere (audit args, the key-change entry, each backup copy's header, Doctor and decrypt's words): `k1-` followed by the first 16 lowercase hex characters of HMAC-SHA-256(subkey, `key-id`)."

## 14. Medium: HKDF's input is not pinned to bytes

**Two units.** *Unit A:* `SecretStore` holds text (`SecretString(String)`), the cipher crate is still Deferred (an `age` identity string or a raw 32-byte `chacha20poly1305` key in some encoding), and the recovery key is typed by hand. *Unit B:* "derived from `backup-key:<project-id>` by HKDF-SHA-256"; "the derivation never change[s]".

**Clash.** One builder feeds HKDF the stored text's bytes, another the decoded key bytes; finding 1's move, a later cipher change or a re-encoding then changes the subkey silently, so later entries stop matching earlier ones although the derivation "never changes".

**Fix (AD-32):** "HKDF's input keying material is the backup key's 32 random bytes (OS CSPRNG), decoded from its one text form (finding 10's prefix); the keychain value and the recovery key are that text, and a typed recovery key is accepted only when it canonicalises to it. The cipher keys from the same 32 bytes or from an HKDF output under another label, so the cipher choice changes neither the text form nor the subkey."

## 15. Medium: writer-open order against AD-7, and no record of a key switch on restore

**Two units.** *Unit A, AD-7:* the pre-migration `VACUUM INTO` copy is taken before migrations and is "encrypted the same way once the backup key exists". *Unit B, AD-32:* the key is created "when it opens the project store's writer" and "the creation is audited with the earlier key id".

**Clash.** If creation follows migrations, the first open of a restored or moved project leaves a plaintext pre-migration copy; if it precedes them, its audit entry has no table to land in yet. And restoring, on the same Mac, a copy older than a key change: the keychain holds B, the chain's last key is A, nothing is created, so no entry records the switch and Doctor's "earlier key" sentence has no source. "The earlier key id" itself can come only from the chain.

**Fix (AD-32):** "Writer open reads or creates the key before AD-7's pre-migration copy, so that copy is encrypted, and after migrations commit it appends one `audit_key.in_use` entry whenever the key id in use differs from the last one the chain records, naming both and whether the key was created now."

## 16. Medium: a null keyed hash is ambiguous

`note_sha256: null` means "no note" today (`crates/am-store/src/lib.rs`), and AD-32's no-key path also writes no hash. Doctor, readers and tests (finding 8) cannot tell a dropped key from an empty note. **Fix (AD-32):** "An entry with a text but no key carries `key_id: "none"` and no hash; an absent text carries neither."

## 17. Low: random keys make golden snapshots nondeterministic

Writer open creates a random key in every test run, so any golden snapshot holding an `unknown:` label (AD-33 snapshots Inbox; the injection variants) changes each run. **Fix (AD-33 or AD-32):** "Golden and trust fixtures pre-seed `backup-key:<project-id>` in `FakeSecretStore` with a fixed test key (`FakeSecretStore::with`); the zero-key run, which cannot, excludes key-dependent fields from its snapshot."

## 18. Low (owner call): a restore on another Mac always rotates

A restore or move always mints a new key, so the app there cannot decrypt the older copies or match the older entries, although the owner holds the recovery key. **Owner call:** keep as decided, or add a UI-class "Use recovery key…" that writes a typed recovery key as the backup key only when the keychain holds none for that project.

## 19. Low: stale words outside the two ADs

AD-33's list of trust-suite prefixes and AGENTS.md's trust-test list omit `audit_hash_*` (the gate runs by directory, so only the words are stale). The For-Sofia line names `crates/am-domain/src/secrets.rs` only; `crates/am-store/src/audit.rs` (`sha256_hex`, "Public since story 3.3"), `crates/am-engine/src/toolgate.rs` (module docs and `label_of`) and `crates/am-engine/src/run.rs` (module docs and `tool_words`) also describe `unknown:<sha256>`.

## 20. Low: "once" and orphaned entries

"Reveal it once" (AD-32) and "hands it out once" (AD-17) do not say whether once per key or once ever; with rotation on restore it must be per key (or per confirmation). `backup-key:*` entries left by a failed wizard create, Treat as new project or a Leftovers removal are never deleted; say that this is accepted.

## 21. Low: leak scan and the user store

9.10's leak scan looks for the backup key and the recovery key; add the subkey (hex and base64). "The user store's chain holds no hash of free text" describes today's code; make it a rule ("never; a story that needs one asks the owner, since the user store has no key").
