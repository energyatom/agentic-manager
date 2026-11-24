---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-24 owner amendment for keyed audit hashes (AD-17 "Where each secret is materialised", AD-32 "Keyed audit hashes", the Deferred per-run salt row, the Open Question on the HKDF/HMAC crate, the note for Sofia)
date: 2026-11-24
---

# Review: versions and facts in the 2026-11-24 keyed audit hash amendment

## Verdict

**Pass with changes: no critical findings, 1 high, 4 medium, 5 low, 2 nits.**

The version facts in the Open Question are right:

- `hmac` 0.13.0 and `hkdf` 0.13.0 exist, are MIT OR Apache-2.0, and were released on 2026-03-29 and 2026-03-30.
- Both sit on the same `digest` 0.11 line as `sha2` 0.11.1.
- `ring` and `aws-lc-rs` both offer HMAC and HKDF.
- The cryptography is sound for a uniformly random 256-bit key (RFC 5869, RFC 2104).
- The keyless chain claim holds in the code.

The problems are elsewhere:

- **High:** the keyed-hash naming and key-id rule does not fit the one free-text hash that lives outside `args`, ToolGate's `unknown:` label. No path is named for `am-engine` to get keyed text.
- **Medium:** option (b) says `ring` is "already in the graph through rustls". That is not true for the built binary.
- **Medium:** the HKDF inputs that "never change" are not fully pinned.
- **Medium:** AD-17's list, which claims to be complete, leaves out several places where code and spine expose or write a secret.
- **Medium:** reading the keychain at every writer open has a macOS reality the spine has not checked.

## Method

- **Diff:** `git -C W diff` of the spine and `.memlog.md`. Changed spine lines: 10 (`updated`), 232–234 (AD-17), 322–324 (AD-32), 782 (the Deferred row), and 815–816 (the Open Question and the note for Sofia). Read the last six memlog entries.
- **Web, 2026-11-24:**
  - crates.io API: `crates/hmac`, `crates/hkdf`, `crates/sha2`, `crates/digest`, `crates/age`, `crates/age-core`, `crates/chacha20poly1305`, `crates/aws-lc-rs`, `crates/ring`.
  - crates.io dependency lists: `hmac/0.13.0`, `hkdf/0.13.0`, `sha2/0.11.1`, `age/0.12.1`, `age-core/0.12.0`, `chacha20poly1305/0.11.0`, `aead/0.6.0`.
  - OSV (`api.osv.dev/v1/query`) for hmac, hkdf, sha2, aws-lc-rs and ring.
  - docs.rs: `age-core` 0.12.0 `primitives`.
  - RFC 5869 and RFC 2104 at rfc-editor.org.
  - RustSec RUSTSEC-2025-0007 (via search).
  - rust-lang/cargo#10801 (via search).
- **Local, read-only:**
  - `Cargo.lock`, workspace `Cargo.toml`, `deny.toml`, `THIRD_PARTY.md`, `rust-toolchain.toml`, `docs/crate-notes/sha2.md`.
  - The registry sources of `reqwest` 0.13.5, `rustls` 0.23.46, `rustls-webpki` 0.103.17, `quinn` 0.11.12, `quinn-proto` 0.11.19, `tauri` 2.12.6, `ring` 0.17.14, `aws-lc-rs` 1.19.0 and `apple-native-keyring-store` 1.0.2, all under `~/.cargo/registry/src`.
- **Code read:**
  - `crates/am-store/src/audit.rs`, `crates/am-store/src/lib.rs` (lines 435–540, 957–1015, 1190–1210, 2210–2235, 2280–2295), `crates/am-store/src/config_history.rs`.
  - `crates/am-engine/src/toolgate.rs`; `crates/am-engine/src/run.rs` (lines 266–282, 1290–1296).
  - `crates/am-tools/src/lib.rs` (lines 300–400).
  - `crates/am-domain/src/secrets.rs`, `crates/am-domain/src/audit.rs`.
  - `crates/am-secrets/src/lib.rs`.
  - `crates/am-app/src/accounts.rs`, `accounts/change.rs`, `serving.rs`, `main.rs`.
  - `crates/am-server/src/secret.rs`, `pairing.rs`.
  - `crates/am-net/src/lib.rs`.
  - A grep of every `.expose()`, `SecretString::new`, `sha256_hex`, `*_sha256` and `Sha256` site under `crates/`.
- **No cargo command was run.** The "compiled or not" findings below come from `Cargo.lock` plus the crates' feature tables, not from `cargo tree`.

## What checks out

| Claim | Evidence | Result |
|---|---|---|
| `hmac` 0.13.0, MIT OR Apache-2.0, released 2026-03-29 | crates.io: 0.13.0 created 2026-03-29T23:19Z, licence `MIT OR Apache-2.0`, latest stable and newest, `rust_version` 1.85 | Correct |
| `hkdf` 0.13.0, MIT OR Apache-2.0, released 2026-03-30 | crates.io: 0.13.0 created 2026-03-30T14:36Z, same licence, latest stable and newest, `rust_version` 1.85 | Correct |
| "the RustCrypto line `sha2` 0.11.1 belongs to" | `hmac` 0.13.0 depends on `digest ^0.11.2` (feature `mac`). `hkdf` 0.13.0 depends on `hmac ^0.13`. `sha2` 0.11.1 depends on `digest ^0.11`. `Cargo.lock` already resolves `digest` 0.11.3 for `sha2` 0.11.1, which meets `^0.11.2` | Correct: one `digest` 0.11.3 would serve all three |
| Licences allowed by AD-3 and `deny.toml` | `MIT OR Apache-2.0` for both. `ring` is `Apache-2.0 AND ISC`; `aws-lc-rs` is `ISC AND (Apache-2.0 OR ISC)`; `aws-lc-sys` 0.47.0 is ISC/Apache-2.0/MIT/BSD-3-Clause/MIT-0 terms. All are on the allow-list | Correct for every option |
| Toolchain fits | `rust-toolchain.toml` pins 1.100.0, above `hmac`/`hkdf`'s 1.85 and `sha2` 0.11.1's 1.88 | Fits |
| Advisories | OSV: none for `hmac`, `hkdf` or `aws-lc-rs`. The `sha2` ones cover 0.9.7 only. The `ring` ones cover AES-panic and versions before 0.17; RUSTSEC-2025-0007 ("ring unmaintained") was withdrawn after the rustls team took over security-only maintenance | Nothing open against the candidates |
| "No Stack crate provides either" | Stack row `sha2 0.11.1` is a hash only (SHA-224/256/384/512, no MAC or KDF). No other Stack row offers HMAC or HKDF as an API | Correct (see nit N2 on wording) |
| `ring` and `aws-lc-rs` offer HMAC and HKDF, and could serve as the backup cipher | `ring-0.17.14/src/lib.rs:141-142` and `aws-lc-rs-1.19.0/src/lib.rs:256-257` have `pub mod hkdf; pub mod hmac;`; both have `aead` (ChaCha20-Poly1305, AES-GCM) | Correct |
| "The chain verifies with no key, because each entry's hash covers its args as text" | `audit.rs:40-58` `entry_hash` is a bare sha256 over the stored text of prev, at, actor, action, target, outcome and args, NUL-separated. `verify_audit_chain` (`audit.rs:391-404`) and `verify_jsonl` recompute it from stored or exported text only | True. A keyed hash is just a string in `args`, so verification and export stay keyless |
| The free-text hashes today are the three named | `lib.rs:2228` `note_sha256` (`inbox.resolved`). `lib.rs:435/518/535/1207` `output_sha256` (step finish, settle+finish, settle+loop-back). `toolgate.rs:88` and `run.rs:274`: `unknown:<sha256>` in ToolGate's label and its words. The other hashes: `am-tools/src/lib.rs:315,398` (file bytes); `config_history.rs:81-82,122-125` (config bytes, checked against stored bytes); `run.created`'s `config_hash` (config snapshot); `entry_hash` | Inventory is complete; the file and config set is right |
| "the user store's chain holds no hash of free text" | `am-app` and `am-config` hash only file bytes (`sha256_hex(bytes)`). There is no key fingerprint or text hash in user-store notes | True |
| "a missing or unreadable key never blocks…", and "holds none" can be told apart from "refuses" | `am-secrets/src/lib.rs:55-60` maps `keyring_core::Error::NoEntry` to `Ok(None)` and every other error to `Err`. `apple-native-keyring-store` `decode_error` maps only `-25300` (errSecItemNotFound) to `NoEntry`. Denied access and interaction-not-allowed become `PlatformFailure`, so a refusal is never read as "none" and never triggers a second key | The create-only-when-none rule is implementable |
| `am-engine` already reaches `am-store` | `toolgate.rs:88` calls `am_store::audit::sha256_hex` | True |

## Cryptographic soundness (RFC 5869, RFC 2104)

- **HKDF with no salt.** RFC 5869 §2.2: when no salt is given, it is HashLen zero octets, and Extract still runs.
  - §3.1: the salt is optional. It adds strength when the input keying material (IKM) is weak.
  - §3.3: when the IKM is already a strong key, Extract may even be skipped.
  - For a 256-bit key from the OS RNG, HKDF-SHA-256 with no salt is sound.
- **`info`.** §3.2: `info` binds the output to a context and must not depend on the IKM. The fixed ASCII label `audit-hash-v1` meets both.
- **Output length.** §2.3 limits L to 255·HashLen or less; 32 bytes is fine.
- **Same answer from every library.** RFC 2104 §2, step 1, pads any key shorter than the block size with zero bytes. So an empty salt (`ring`/`aws-lc-rs` `Salt::new(HKDF_SHA256, &[])`) and HashLen zeros (`hkdf` with `None`) give the same PRK. Options (a) and (b) therefore produce the same subkey and key id, provided the IKM bytes, L and `info` encoding are pinned (finding M2). This supports the spine's "the AD wording does not depend on the answer".
- **HMAC-SHA-256 under a 32-byte random subkey.** RFC 2104 §3 recommends a random key of at least L bytes (32 for SHA-256); the subkey meets it.
- **No key reuse with the cipher.** HKDF-Extract uses the IKM as the HMAC message, with the salt as the key. Using the backup key both as a cipher key and as HKDF IKM is therefore not key reuse across primitives.
- **Per-project isolation.** Each project has its own backup key and subkey, so keyed hashes cannot be linked across projects.

## Findings

### Critical

None.

### High

#### H1: The unknown-tool label does not fit the keyed-hash rule, and no seam is named

**Claim (AD-32, line 324):** "Each such entry's args hold a key id beside the keyed hash … and the hash's arg name says it is keyed (for example `note_hmac`, `output_hmac`), never `*_sha256`."

**Claim (AD-17, line 234):** "other crates, `am-engine`'s ToolGate among them, get only the keyed hash text."

**Reality:**

- The model-chosen name's hash is never an arg of its own. `toolgate.rs:320-331` (`refuse`) writes it into the audit entry's `target` (`tool:unknown:<hex> run:… step:…`) and into its `outcome` text (`Tool "unknown:<hex>" was refused…`), with `args: Default::default()`. That entry has no args, so no `key_id`.
- `RecordToolCall` puts the label in `args.tool` (`lib.rs:466-480`).
- The label is also the journal's `tool_calls` name (`run.rs:1293-1295`) and part of the Inbox words (`run.rs:266-282`, `tool_words`).
- ToolGate's per-step counter is keyed on the label (`StepTool.tool`).
- The label is made by free functions, `toolgate::label_of` (`toolgate.rs:84-90`) and `run.rs::tool_words`, which hold no store or writer handle. The only place the subkey can live is `am-store`'s audit module.

A builder of story 9.10 therefore has two bad choices. One is to hand the subkey or the backup key to `am-engine`, which breaks AD-17. The other is to leave the bare sha256 in the label, which breaks AD-32 and fails the `audit_hash_*` trust test. The keyless case ("the entry holds no hash") also leaves the label's form undefined.

**Proposed fix (AD-32, after "never `*_sha256`"):**

> "ToolGate's label for a name no manifest holds is `unknown:hmac:<hex>` (keyed; `unknown` alone when no key can be had), computed only by `am-store`'s audit module through one `am-store` call that takes the name and returns the label text; ToolGate and the run's words take that text and never a key. Every audit entry that holds a keyed hash in any field (args, target or outcome) carries `key_id` in its args; journal rows and Inbox words carry the label only."

**Proposed fix (AD-17, after "get only the keyed hash text"):**

> "(through `am-store`'s keyed-label call on the open project store, never a key or subkey value)".

### Medium

#### M1: Option (b) says `ring` is in the graph through rustls; only `aws-lc-rs` is built, and it is an unrecorded native library

**Claim (line 815):** "(b) `ring` or `aws-lc-rs`, already in the graph through rustls".

**Reality:**

- **aws-lc-rs is compiled.** The workspace builds `reqwest` with `features = ["rustls", "stream"]` (`Cargo.toml:91`). reqwest 0.13.5's `rustls` feature pulls in `__rustls-aws-lc-rs`, which turns on `rustls?/aws-lc-rs`. The `rustls` 0.23.46 entry in `Cargo.lock` (line 3738) depends on `aws-lc-rs` and not on `ring`.
- **ring is listed in `Cargo.lock` but very likely not compiled.** It appears only as a weak-feature artifact (rust-lang/cargo#10801, "disabled optional weak dependencies end up in Cargo.lock"):
  - `rustls-webpki`'s `alloc = ["ring?/alloc", …]`.
  - `quinn-proto`, which is itself listed only because of reqwest's weak `quinn?/rustls-aws-lc-rs`. `h3` and `h3-quinn` are absent, so `http3` is off. Tauri's reqwest dependency applies only to Android and iOS.
- **aws-lc-rs links a C library.** `aws-lc-rs` builds `aws-lc-sys`, a C library compiled with `cc` and linked into `agentic`. AD-3 says "A native library linked into the binary needs a permissive licence recorded in `THIRD_PARTY.md` (v1 links none…)". `THIRD_PARTY.md` lists SQLite but not AWS-LC. This gap predates the change, but option (b) would make the dependency a deliberate one.
- **ring maintenance.** `ring` 0.17.14 (2025-03-11) has had security-only maintenance by the rustls team since RUSTSEC-2025-0007 was withdrawn.

**Proposed fix (line 815, option (b)):**

> "(b) `aws-lc-rs` 1.19.0 (ISC AND (Apache-2.0 OR ISC); already compiled into `agentic` through reqwest's `rustls` feature; links the AWS-LC C library, which `THIRD_PARTY.md` must then record) or `ring` 0.17.14 (Apache-2.0 AND ISC; listed in `Cargo.lock` only through weak features, so a new compiled dependency; security-only maintenance by the rustls team) as a direct dependency, which could also serve as the backup cipher (checked in `Cargo.lock` and the crates' feature tables 2026-11-24; story 9.10 confirms with `cargo tree -i`)".

Separately, for the next sweep: record AWS-LC in `THIRD_PARTY.md`, and fix AD-3's "v1 links none".

#### M2: The derivation that "never changes" has unpinned inputs

**Claim (line 324):** "derived from `backup-key:<project-id>` by HKDF-SHA-256 (no salt) with the fixed context label `audit-hash-v1` as its info … the label and the derivation never change".

**Reality:**

- **The IKM bytes are not defined.** The keychain holds text: `SecretString(String)`, and `get_password()` returns a `String` (`am-secrets/src/lib.rs:57`). The backup key's text form depends on the cipher crate, which is still Deferred to 9.10. An `age` X25519 identity is Bech32 `AGE-SECRET-KEY-1…`; a raw 256-bit key would be hex or base64. Feeding the text bytes and feeding the decoded bytes give different subkeys and different key ids. Neither choice can then change.
- **The output length L is not stated.**
- **The `info` encoding is not stated.**
- **The key-id function is not stated.** It says only "derived from the subkey".
- **The key's entropy is not stated.** HKDF does no stretching. If the cipher choice ever made the backup key a passphrase (`age`'s scrypt mode), the keyed hashes would only be as hard to guess as that passphrase.

**Proposed fix (AD-32):**

> "The backup key is 32 bytes from the OS RNG, whatever cipher 9.10 picks; the keychain holds it as lowercase hex (or the cipher's own encoding, recorded in 9.10's crate note). IKM = the 32 decoded key bytes; salt absent (HashLen zero bytes, RFC 5869 §2.2); info = the ASCII bytes `audit-hash-v1`; L = 32 bytes. HMAC input = the free text's UTF-8 bytes. The key id is the first 16 hex characters of HMAC-SHA-256(subkey, ASCII `audit-hash-v1 key-id`)."

The key id must not be a bare sha256 of the backup key; the spine already says so. Using a separate label keeps the key id from ever equalling the HMAC of some free text.

Also, "derived from `backup-key:<project-id>`" names the keychain account rather than the key. Say "derived from the backup key (keychain account `backup-key:<project-id>`)".

#### M3: AD-17's "true per-component list" misses exposures the code and the spine already have

**Claim (line 234):** "a secret's value is exposed only inside the one component named here … and is never handed to a tool, logged, journaled or written anywhere but the keychain". The list then enumerates the sites.

**Reality:**

1. **`accounts list` reads stored keys back.** `am-app/src/accounts.rs:1177` (in `fn list`, line 1129) reads every stored key to show `masked(key.expose())`: `•` plus the last 4 characters (`accounts.rs:1093-1101`). The tickets require this ("the masked key as `•` plus the last 4 characters"). AD-17 says a stored key is read back "only for AD-15 *Add*'s stored-key check". A strict builder or reviewer would flag `list` or remove its tail.
2. **The recovery key leaves `am-store` to be shown.** "Hands it out once as the recovery key, only to AD-32's Show recovery key" cannot be done without exposing the value outside `am-store`. It must reach the HTTP response body (`am-server`/`am-api`) or the CLI's terminal line (`am-app`). Likewise, the Account card (epic 5 story 5) takes a key in through `am-server`'s request body.
3. **Capability-provider Accounts are missing from the `am-net` list.** The list "(model calls, `accounts test`, crawl-service and API Sources, AD-39)" leaves out AD-46's capability-provider Accounts: search (Brave, Tavily, Exa, Serper, `searxng-remote`) and scrape. AD-48 lists those as requests carrying an Account.
4. **The general clause contradicts AD-13's per-launch values.** The CLI token is written to `<app data dir>/serving/<pid>.token` (`am-app/src/serving.rs:195`; AD-32 lists it in the app data dir). The pairing code is printed to the serve terminal (`am-server/src/pairing.rs:148`). These are their own types (`CliToken`, `PairingCode`), not `SecretString`, but the clause says "a secret".

**Proposed fix (AD-17, replace the first sentence of the paragraph and extend two items):**

> "a keychain secret's value (an Account key, a backup key, the audit-hash subkey, the Local search secret) is exposed only inside the components named here …; AD-13's per-launch values (session secret, launch and pairing codes, CLI token) are not keychain secrets and follow AD-13."

> *Account keys:* "… read a stored one back only for AD-15 *Add*'s stored-key check and for a masked display (`•` and the last four characters: `accounts list`, the Account card); a key typed into the Account card crosses `am-server` only as a `SecretString` straight to the keychain"

> and add "capability provider calls (search and scrape Accounts, AD-46)" to the `am-net` parenthetical.

> *The backup key:* "… hands it out once as the recovery key, exposed only in the one response body (`am-server`) or terminal line (`am-app`) that shows it, never logged, journaled or kept".

#### M4: Reading the keychain at every writer open meets the legacy keychain's per-binary ACL; this is unchecked

**Claim (line 324):** "`am-store` derives the subkey when it opens the project store's writer, first creating the backup key when the keychain holds none".

**Reality:**

- `apple-native-keyring-store`'s `keychain` module (the Stack's `keychain` feature) uses the legacy file-based login keychain (`keychain.rs` module docs). An item there trusts the binary that created it, and another binary prompts or is refused. AD-17 already defers this question to the signed build in epic 11, but only for Account keys ("whether the desktop app reads without a prompt an entry the `agentic` CLI stored").
- The backup key used to be read only at backup time. Now it is read at every writer open, by the CLI, the desktop app and the headless `agentic service` LaunchAgent. In the LaunchAgent a prompt cannot be answered (errSecInteractionNotAllowed, which becomes `PlatformFailure`). The rule then makes every entry hold no hash, silently, for as long as that binary is not trusted.
- The code does tell refusal from absence correctly (see the table above), so no second key is minted. The loss is silent.

**Proposed fix:**

- In AD-17's epic 11 verification sentence, add: "and whether the desktop app and the `agentic service` LaunchAgent read without a prompt a `backup-key:<project-id>` entry another of the app's binaries created".
- In AD-32, after "the entry holds no hash", add: "and Doctor's backup row says that audit hashes are off and why (the keychain refused, naming the process)".

### Low

#### L1: The Open Question's account of `age` and `chacha20poly1305` is loose

**Claim (line 815):** "`age` or RustCrypto `chacha20poly1305`, neither offering HMAC or HKDF for this use".

**Reality:**

- `age-core` 0.12.0 exports a public `primitives::hkdf` (HKDF[salt, label](key, 32)); it has no HMAC.
- `age` 0.12.1 (2026-07-14) depends on `hmac ^0.12`, `hkdf ^0.12`, `sha2 ^0.10` and `chacha20poly1305 ^0.10`, the older `digest` 0.10 line. Choosing `age` with option (a) would put two `hmac`/`hkdf` lines in `agentic`. `deny.toml` only warns on multiple versions.
- `chacha20poly1305` 0.11.0 (2026-06-28, Apache-2.0 OR MIT) is on the new line (`aead ^0.6` → `crypto-common ^0.2`, which `digest` 0.11.3 already uses).

**Proposed fix:**

> "(`age` 0.12.1, whose `age-core` exposes an HKDF helper but no HMAC and which stays on the older RustCrypto line `hmac`/`hkdf` 0.12 and `sha2` 0.10; or RustCrypto `chacha20poly1305` 0.11.0, on the same line as `sha2` 0.11.1, with neither MAC nor KDF)".

The cipher choice and option (a) are then visibly coupled for 9.10.

#### L2: The backup module derives the subkey, yet the subkey's type is private to the audit module

**Claim (line 234):** the backup module "derives the audit-hash subkey from it", and the subkey lives "only in `am-store`'s audit module, as a type private to that module".

**Reality:** whichever module runs HKDF holds both the backup key's bytes and the subkey's bytes. If the type is private to `audit`, `backup` cannot construct it. If `audit` derives it, `audit` calls `.expose()` on the backup key. Module privacy inside one crate is also not something `xtask check-deps` enforces.

**Proposed fix:**

> "the backup module exposes the backup key only to call the audit module's one `pub(crate)` derive function, which returns the subkey as the audit module's private type".

#### L3: `is_sha256_hex` cannot tell keyed from bare, so the generic `hash` field must stay file-bytes only

**Reality:**

- An HMAC-SHA-256 hex value is also 64 lowercase hex characters, so it passes `is_sha256_hex` (`lib.rs:957-959`). That check guards only the shape of `WriteCommand::Audit`'s `hash` and `run.created`'s `config_hash` (`lib.rs:998-1015`). It is about keeping content out of `args`, not about keying, so for safety it does not matter.
- However, `AuditEvent.hash` is stored as the arg `sha256` (`lib.rs:588-590`) and is documented as "Content hash when relevant (sha256 hex)" (`am-domain/src/audit.rs:17-18`). The format check would let a keyed hash in under a bare name, or a bare hash of free text in through any crate's `AuditSink`. The trust test is the only guard.

**Proposed fix (AD-32):**

> "The generic audit `hash` field (stored as `sha256`) carries only file-byte hashes; a keyed hash is written only by `am-store` under a `*_hmac` name with `key_id`, never passed in through `AuditSink`".

The doc comment in `am-domain/src/audit.rs` follows in 9.10.

#### L4: Option (a) adds two pre-1.0 crates, which the project convention says need crate notes

**Reality:** AGENTS.md requires `docs/crate-notes/<crate>.md` before a pre-1.0 crate is used, and says its types never enter `am-domain` or a port signature. `hmac` 0.13.0 and `hkdf` 0.13.0 are pre-1.0, as is `ring`. `docs/crate-notes/` has `sha2.md` but nothing for these.

**Proposed fix (line 815, option (a)):**

> "… as two Stack rows that story 9.10 adds and pins, with `docs/crate-notes/hmac.md` and `hkdf.md` (their types stay inside `am-store`'s audit module)".

#### L5: The trust test must keep the canary out of `folder.write`

**Reality:** `folder.write` audits a bare sha256 of the bytes written (`am-tools/src/lib.rs:393-399`), and those bytes are often model output. The owner decided that file hashes stay bare, so this is accepted. But the `audit_hash_*` test ("finds no bare sha256 of the canary in the audit log, its export or the journal") would fail if its model-output canary were also written through `folder.write`.

**Proposed fix (AD-32's trust-test sentence):** add "(the canary is not written to a file; file-byte `sha256` args are out of scope)".

### Nits

- **N1.** "Each entry's hash covers its args as text" is true but narrower than the code: `entry_hash` covers prev, at, actor, action, target, outcome and args. "Covers every field as stored text" is more exact, and matters once a keyed hash lives in `target` (H1).
- **N2.** "`sha2` 0.11.1 gives SHA-256 only": it gives the SHA-2 family and no MAC or KDF. Suggest "`sha2` 0.11.1 is a hash only (no MAC or KDF)".

## Not checked

- Whether `ring` or `aws-lc-rs` is actually compiled into `agentic`: no cargo command was allowed, so this is inferred from `Cargo.lock` and the feature tables (M1). Story 9.10 or the manager can confirm with `cargo tree -i ring -e normal --target aarch64-apple-darwin`.
- The macOS keychain ACL behaviour across the CLI, desktop and LaunchAgent binaries (M4). This needs the signed build (epic 11).
- The ticket text of story 9.10 and the release trust-gate story. The Sofia REPLAN is held, so they were not changed.
