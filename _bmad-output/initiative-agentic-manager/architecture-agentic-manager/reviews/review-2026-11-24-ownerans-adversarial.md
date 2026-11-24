# Adversarial review: owner answers on the backup cipher, AWS-LC's parts and the restore words (2026-11-24)

- **Reviewed:** the uncommitted diff on branch `replan/keyed-audit-hash` (follow-up to aa8c1f8): AD-3 (Binds, Prevents, *Native-library parts*), AD-32 (*The cipher*, the "model output included" clause, *Restore on another Mac*), the CI/release diagram label, the Deferred cipher row, Open Questions (two Resolved bullets, one For-Sofia bullet), `THIRD_PARTY.md`, `.memlog.md`.
- **Read against:** AD-3, AD-17, AD-25, AD-32 and AD-33 as they now stand; the Stack `aws-lc-rs` row; epic 9 story 10 (`tickets.toml` id 10, not yet updated by Sofia); epic 11 stories 2, 3, 5, 6, 8, 12, 16 and the epic file; `deny.toml`, `justfile` (`supply-chain`), `Cargo.lock`; `docs/owner-standing-rules.md`; `.claude/skills/bmad-build-auto/`. Facts checked from the local cargo registry (aws-lc-sys 0.47.0's `Cargo.toml` and `LICENSE`, licences of every locked package) and docs.rs for `age` 0.12.1's features and required dependencies.
- **Scope:** how the answers are written into the spine. The owner's three answers are taken as final; findings attack the wording and its seams with existing text, not the decisions.
- **Lens:** two units one level down, each obeying every AD to the letter, that still build incompatibly.
- **Verdict:** **amend before Sofia's REPLAN of story 9.10 and before epic 11 takes up the licence review.** 0 critical, 4 high, 9 medium, 5 low. The answers are recorded faithfully and the verbatim quotes match the memlog. The holes are at the edges: no build mechanism enforces the owner's stop on the cipher, the stored form of the backup key is tied to a crate the owner flagged as a maintenance risk and then frozen, `agentic backups decrypt` has no lawful way to reach the key without opening a writer, and the notice duty covers AWS-LC but not the BSD-3-Clause and MPL-2.0 crates already linked today.

| # | Tier | Title | Where |
|---|---|---|---|
| H1 | High | The cipher "plan stop" has no mechanism; the fallback branch triggers no stop at all | AD-32 *The cipher*; Deferred row; 9.10 `hitl = false` |
| H2 | High | The backup key's stored form is tied to the cipher and frozen; replacing the cipher later breaks every keyed hash or reuses a key across protocols | AD-32 *The cipher*, *Keyed audit hashes* |
| H3 | High | `agentic backups decrypt` can reach the key module only through a writer open, which creates a key on the new Mac and loses the key-switch record | AD-17 *The backup key*; AD-32 *Restore on another Mac* |
| H4 | High | Notices are required only for native libraries; BSD-3-Clause and MPL-2.0 crates linked today (and age's dalek crates) need them too | AD-3 *Native-library parts*; `THIRD_PARTY.md` |
| M1 | Medium | "Epic 11's release checklist" does not exist; four stories could own the review; THIRD_PARTY.md and the SBOM are two owners of one list | AD-3; For-Sofia bullet; epic 11 |
| M2 | Medium | Which artefacts carry the notices, and where inside them, is inconsistent across the spine | AD-3; CI diagram; Structural Seed |
| M3 | Medium | STREAM is a construction, not a wire format: the fallback as written forces invented framing | AD-32 *The cipher*; Deferred row |
| M4 | Medium | age's feature set and the check-deps guard are unpinned: `plugin` spawns PATH programs, `ssh` pulls `rsa` | AD-32; Stack (to come) |
| M5 | Medium | A keychain value present but not in the stored form has no defined handling; the trust-test canary may not be one | AD-32 lifecycle; AD-33 suite |
| M6 | Medium | Per-copy key identity has nowhere to live now that no format may be invented | AD-32 Doctor sentence; 9.10 Settings › Backups |
| M7 | Medium | Restore words are tied to surfaces, not to the state that loses verification, and name only notes | AD-32 *Restore on another Mac* |
| M8 | Medium | "Decrypt a backup over `state.db`" leaves `-wal`/`-shm` beside the restored file | AD-32 restore line; 9.10 description |
| M9 | Medium | "No crate-licence exception" leaves `[[licenses.clarify]]` open and blocks a future aws-lc-sys security bump without a rule | AD-3; Stack `aws-lc-rs` row |
| L1 | Low | The owner's "restore screen later" has no Deferred row and no owner | Deferred |
| L2 | Low | AD-25 does not carry the confirmed model-output-in-a-file exception; the release-gate claim overstates | AD-25; AD-32 test sentence |
| L3 | Low | "cargo-deny advisories run" names a check the repo deliberately does not run | AD-32; Deferred row |
| L4 | Low | "chacha20poly1305 is no longer a candidate" vs age's own required dependency on it | Deferred row |
| L5 | Low | "Reveal it once" is now load-bearing: the recovery key is the only way to any copy on another Mac | AD-17; AD-32 |

---

## H1. High: the cipher "plan stop" has no mechanism, and the fallback branch triggers no stop at all

**Two units, each obeying the letter.**
- *Unit A, the manager running `bmad-build-auto`.* AGENTS.md: auto build "takes only `hitl = false` stories whose `after` are built … and stops at any `hitl = true` story". Story 9.10 is `hitl = false`. Its `unknown` asks a question that the spine's Open Questions now marks **Resolved** ("the backup cipher is `age` in principle"). `bmad-build-auto` step 2 halts after planning only when the invocation prompt says "Halt after planning", and halts on an intent gap only when nothing selects between readings; the spine selects (age, else the named fallback). The builder runs `cargo audit` itself; if it judges age to fail (an advisory, a licence it reads as off-list, a maintenance worry), it takes the fallback, which uses `aws-lc-rs`, already in Stack, so `docs/owner-standing-rules.md`'s stop ("any new dependency not in the Stack") does not fire either.
- *Unit B, the owner and the architect,* who expect the build to stop after the plan, with Winston's checks recorded, for the owner's yes on either branch (the For-Sofia bullet says "an owner stop for the cipher (age's checks, or the fallback when age fails them)").

**Clash.** The spine names a stop ("the final choice is made at epic 9 story 10's plan stop"; Deferred: "Owner's final yes at E9 story 10's plan stop") but no rule makes any build stop there. The only enforcement is a For-Sofia note asking Sofia to turn an `unknown` into "its plan stop", and an `unknown` is not a stop in `bmad-build-auto`. "After Winston checks" is also not a step any builder runs: in auto mode no architect is invoked.

**Location.** AD-32 *The cipher*; Deferred row "Backup encryption cipher crate"; Open Questions For-Sofia bullet; `tickets/epic-tools-mcp-search-doctor/tickets/tickets.toml` entry 10 (`hitl = false`).

**Tightening (AD-32, after "the final choice is made at epic 9 story 10's plan stop"):** "Story 9.10 is `hitl = true` until the cipher's Stack row exists, and is built only by `bmad-build`: its plan records the architect's check (the age version, its exact feature list, `cargo tree --locked -e features` of its graph on both CI targets with every licence, and `cargo audit` output) and stops before any code for the owner's yes, on either branch (age, or the named fallback when age fails). An autonomous build that finds no cipher row in Stack and no recorded owner yes halts `blocked`." The Deferred row's "Decided by" then names the same stop for both branches.

---

## H2. High: the backup key's stored form is tied to the cipher and frozen, so replacing the cipher later breaks every keyed hash or reuses a key across protocols

**Two units, each obeying the letter.**
- *Unit A, story 9.10.* AD-32: "The same choice fixes the stored form of the `backup-key:<project-id>` value, from which the audit-hash subkey below derives as stored … it does not change after that story." The plan stop picks age with an X25519 identity, so the keychain value is `AGE-SECRET-KEY-1…` (Bech32), the recovery key is that string, and the subkey is HKDF over its UTF-8 bytes. age 0.12 also offers a post-quantum hybrid identity (required dependency `ml-kem`, `hpke`) and the scrypt passphrase recipient: three different stored forms, none named.
- *Unit B, a later story replacing age.* The owner asked for age's maintenance to be checked precisely because it may lapse; the Stack row rule makes a security fix move its row. If age later fails (an advisory with no fix, abandonment), the owner's own fallback (a published scheme on `aws-lc-rs`) applies. That scheme keys AEAD from raw key bytes, not from an age identity.

**Clash.** Unit B can either (a) store a new form, which changes the subkey and the key id although no key changed, so every keyed hash made before stops matching and Doctor sees an unexplained key switch; or (b) keep the age identity string and use its bytes, or its X25519 scalar, directly as the AEAD key, which is the same secret used under two primitives; or (c) keep age forever as a decrypt-only dependency for old copies, which the fallback was meant to remove. "The label and the derivation never change" and "it does not change after that story" each forbid the clean path, because the spine made the subkey's input the cipher's key format.

**Location.** AD-32 *The cipher* (last two sentences) and *Keyed audit hashes* (the subkey's input).

**Tightening (AD-32, replacing "The same choice fixes the stored form …"):** "The stored form is fixed now and does not depend on the cipher: the keychain value is one text, `amk1-` followed by 64 lowercase hex digits of 32 bytes from the OS random source, and it is also the recovery key (a typed one is accepted after trimming whitespace and lowercasing). The audit-hash subkey derives from that text as stated below. The cipher's key derives from the same text by HKDF-SHA-256 under its own fixed label (`backup-cipher-v1`); if age is chosen, its identity is built deterministically from that output (one recipient kind, named in the cipher's Stack row), never stored. A later cipher takes a new label, and copies made under the old one keep decrypting through the old label." This keeps the owner's "derived from the per-project backup key", keeps the audit derivation unchanged, and makes the cipher swappable without touching keyed hashes.

---

## H3. High: `agentic backups decrypt` can reach the key module only through a writer open, which creates a key on the new Mac and loses the key-switch record

**Two units, each obeying the letter.**
- *Unit A, the decrypt command (9.10).* AD-17: the key module is the only holder of the backup key and is the one "which `am-app` hands the `SecretStore` port and the project id when it opens the project store's writer (no default)"; it "gives the backup code … decryption (`agentic backups decrypt` hands it a typed recovery key)". The only constructor named is the writer open, so the command opens the project's writer to get the module. AD-32: a writer open reads the key and "only the keychain's not-found answer creates a key". On another Mac the keychain says not-found, so **running decrypt creates the new backup key** and writes its creation entry into whatever store that open used: a fresh `state.db` the owner then overwrites with the decrypted copy.
- *Unit B, the first `agentic serve` after the restore.* The key now exists, so nothing is created. AD-32's "the creation is audited with the earlier key id when there was one" never lands in the restored chain, and Doctor's "after a new key, that copies made under an earlier key need its recovery key" has nothing to read.

**Clash.** A builder who instead builds a decrypt-only path without a writer open contradicts AD-17's "when it opens the project store's writer". Either the restore loses its only audit record of the key switch (the event the owner's words exist to explain), or AD-17 is broken. The same writer open also takes the project lock and may write the `.agentic-open` marker on the new Mac before the owner has placed the copy. The decrypt command may also run with no project at all (`<file> --out <path>`), and then there is no project id to hand over.

**Location.** AD-17 *The backup key and the audit-hash subkey*; AD-32 *Keyed audit hashes* (creation rule) and *Restore on another Mac*.

**Tightening (AD-17 and AD-32, one sentence each).** AD-17: "The key module also offers one decrypt-only handle, built from a typed recovery key alone, which opens no store, takes no lock and calls no keychain; with no typed key, `agentic backups decrypt --project <path>` reads that project's `backup-key:` entry and never creates one. Only a writer open creates a key." AD-32: "The first writer open whose key id differs from the last `key_id` the chain records (a key created now, or one already present after a restore of an older copy) appends one key-switch entry naming both ids before any other entry; Doctor's disk row reads it."

---

## H4. High: notices are required only for native libraries, but BSD-3-Clause and MPL-2.0 crates linked today (and age's dalek crates) need them too

**Two units, each obeying the letter.**
- *Unit A, the epic 11 release story.* AD-3 now says: "Each release ships `THIRD_PARTY.md` and AWS-LC's licence notices … inside every artefact that carries the binary", and the licence review checks that "every native library linked is recorded in `THIRD_PARTY.md` at its linked version, and its notices are in the artefacts". The story ships exactly those two files, and the review passes.
- *Unit B, the licences of crates linked into the same binary today* (from `Cargo.lock` and the local registry): `subtle` 2.6.1 (BSD-3-Clause, through rustls), `matchit` 0.8.4 (MIT AND BSD-3-Clause, through axum), `brotli` 9.1.3 (BSD-3-Clause AND MIT), `encoding_rs` 0.8.44 (… AND BSD-3-Clause), `alloc-no-stdlib`/`alloc-stdlib` (BSD-3-Clause); `cssparser`, `selectors`, `dtoa-short`, `option-ext` (MPL-2.0, whose §3.2 requires telling recipients of the executable where the source is); every Apache-2.0 crate (§4(a) a copy of the licence, §4(d) any NOTICE file); the Unicode-3.0 data crates; and the npm packages built into the embedded UI. If age is chosen, `x25519-dalek` (required) and `curve25519-dalek` add more BSD-3-Clause code. `THIRD_PARTY.md` only says these are "linked under their own permissive licences" and carries none of their texts.

**Clash.** AD-3's new Prevents line ("a release that ships a native library without its licence notices") and the review it defines cover the owner's sentence about AWS-LC's BSD notice and nothing else. A release that obeys AD-3 to the letter breaches the BSD-3-Clause binary-redistribution clause for several crates and MPL-2.0 §3.2. A second builder (the SBOM story, or the v1.0 gate reviewer) who generates a full notices file is building something AD-3 does not ask for, in a layout nobody has fixed.

**Location.** AD-3 *Native-library parts* and Prevents; `THIRD_PARTY.md` (new paragraph); CI diagram label.

**Tightening (AD-3, replacing the notice sentence):** "Each release artefact carries one notices file generated at release time from `Cargo.lock` and the UI's npm lockfile for the release target's graph: for every package, its licence text(s), copyright lines and any NOTICE file, and for MPL-2.0 packages where their source is; native libraries' notices (AWS-LC's `LICENSE`, copied from the locked `aws-lc-sys` source) are part of it, and `THIRD_PARTY.md` sits beside it. It is never hand-kept." Choosing the generating tool is a new dependency and a licence matter, so it is an owner call under the standing rules (for example `cargo-about`; the npm side has its own options).

---

## M1. Medium: "epic 11's release checklist" does not exist, four stories could own the review, and THIRD_PARTY.md and the SBOM are two owners of one list

**Two units.** *Story 11.5* ("cargo-deny (licences, …) … fail the release", `hitl = true`) is "the story that owns the release licence checks" in the For-Sofia bullet's words. *Story 11.16* ("Bundle Local search … with its licence notice") already places `THIRD_PARTY.md` and notices in the bundle. *Story 11.8* is where the epic file's own open question is anchored ("before story 8"). *Story 11.6* publishes the SBOM with "version, licence and sha256" for every bundled non-Rust component. No epic 11 file or ticket contains a checklist (`grep -i checklist` finds none).

**Clash.** Each story can read the duty as another's: 11.5 adds a cargo-deny gate and no file placement; 11.16 places SearXNG's notices and not AWS-LC's; 11.6's SBOM lists AWS-LC at the version it finds while `THIRD_PARTY.md` lists the version someone typed. A "review" described as a checklist item is unenforceable between releases: an aws-lc-sys bump (which the Stack row says moves `THIRD_PARTY.md` "together", by hand) drifts unnoticed until a person reads the list at tag time. Using `links` keys to find "native libraries" is also wrong on this graph: `tauri`, `gtk-sys`, `ring`, `wasm-bindgen-shared` and `aws-lc-rs` carry `links` without compiling a C library into the macOS binary.

**Tightening (AD-3, replacing "Epic 11's release checklist includes a licence review: …"):** "The licence review is a CI check built by epic 11 story 6 and run by `just supply-chain` and the release job: an `xtask` lists every package in the `aarch64-apple-darwin` release graph whose build script compiles non-Rust sources into the binary (today `libsqlite3-sys`, `aws-lc-sys`, `objc2-exception-helper`), and fails when `THIRD_PARTY.md` lacks an entry naming its locked crate version and library version, or when that entry disagrees with the SBOM. Epic 11 story 16 places the files (M2). The tag stories (14, 15) have the owner read that output." The For-Sofia bullet then names stories 6 and 16, not "the story that owns the release licence checks".

---

## M2. Medium: which artefacts carry the notices, and where inside them, is inconsistent across the spine

**Two units.** *Story 11.3* (installer) puts `THIRD_PARTY.md` and AWS-LC's `LICENSE` at the DMG's top level beside the `.app`, which is "inside the installer". *Story 11.2* (updater) publishes the Tauri updater payload, an archive of the `.app` alone, which also "carries the binary". An owner who installs v1.0 and updates to v1.1 has no notices.

**Clash.** AD-3 names "the installer's app bundle and the `agentic` archive"; the CI diagram names "installers · `agentic` binaries · updater manifest"; the Structural Seed names "installers and release archives" with "licence texts in `THIRD_PARTY.md`" (texts inline), while AD-3 ships AWS-LC's notices as a separate file. A bare `agentic` binary asset can hold nothing "inside", and the updater payload is not named at all.

**Tightening (AD-3):** "The notices live inside the app at `Contents/Resources/licenses/` (`THIRD_PARTY.md`, the generated notices file of H4), so every artefact that carries the app (installer, updater payload) carries them. A CLI-only download, if v1 publishes one, is an archive holding `agentic` and the same `licenses/` folder, never a bare binary. The release gate unpacks each published artefact and checks those files." Align the CI diagram label and the Structural Seed sentence on that one layout.

---

## M3. Medium: STREAM is a construction, not a wire format, so the fallback as written forces invented framing

**Two units.** *Builder A* follows the example literally ("a STREAM construction implemented from its published specification") and writes a header (magic, version, nonce prefix, segment size, final-segment flag, key wrapping), because the STREAM paper defines none of these. *Builder B* reads "never a format of our own: no backup file format is invented" and refuses A's header.

**Clash.** Both are reading AD-32. The owner's sentence was "a published chunking scheme on aws-lc-rs, never our own"; a construction without a wire format cannot be built without inventing one. The spine also does not say who designs the framing if it comes to that.

**Tightening (AD-32 and the Deferred row, replacing the example):** "The fallback is a published wire format with published test vectors, implemented byte for byte on `aws-lc-rs`: the age v1 format (C2SP's age specification: X25519 recipient, HKDF-SHA-256, HMAC-SHA-256 header MAC, ChaCha20-Poly1305 STREAM in 64 KiB chunks, all available in `aws-lc-rs`), checked against the age project's published test vectors recorded outside the code under test; Tink's AES-GCM-HKDF streaming format is the second choice. No header field, version or chunk size is ours." With H2's derivation, this keeps the files and the recovery key valid if age the crate is dropped later, and keeps copies readable by the upstream `age` tool.

---

## M4. Medium: age's feature set and the check-deps guard are unpinned; `plugin` spawns PATH programs and `ssh` pulls `rsa`

**Two units.** *Builder A* adds `age` with the features that make the recovery key interoperate with the `age` command-line tool: `plugin` (age-core's plugin support, `which`, `wsl`; it starts an `age-plugin-<name>` program found on PATH when an identity names a plugin) and perhaps `cli-common` (`pinentry`, `rpassword`) or `ssh` (`aes`, `bcrypt-pbkdf`, `rsa` 0.9 and more). *Builder B* adds it with no features (age 0.12.1 has 23, none on by default).

**Clash.** "A clean `cargo audit`" depends on the feature set: `rsa` 0.9 carried the unfixed Marvin advisory (RUSTSEC-2023-0071) as of this reviewer's knowledge; verify at the plan stop. `plugin` adds a child process that no AD-13 rule covers. The aws-lc-rs row pins features with a cargo-deny `bans.features` entry; the cipher row has no such rule. The `check-deps` guard polices only the name `aws_lc_rs`, so with age chosen, `age::x25519::Identity` (the backup key in parsed form) can be built in any `am-store` module, and AD-17's "only in one key module" has no mechanical check.

**Tightening (AD-32, *The cipher*):** "The cipher's Stack row names `default-features = false` and its exact features (never `plugin`, `ssh` or `cli-common`); a cargo-deny `bans.features` entry keeps exactly them; `xtask check-deps` extends to the cipher crate as for `aws-lc-rs` (only `am-store` declares it, and only `keys.rs` names it in shipped source); a typed recovery key is parsed as one key string, never as an identity file."

---

## M5. Medium: a keychain value present but not in the stored form has no defined handling, and the trust-test canary may not be one

**Two units.** *The key module (9.10)* receives a value from the keychain that is not in the chosen stored form: edited in Keychain Access, written by a development build, or the readable canary AGENTS.md's "canary values" invite (`FakeSecretStore` holding `canary-backup-key`). Builder A derives the subkey anyway (HKDF accepts any bytes), so keyed hashes are on while every backup fails to encrypt. Builder B parses first and treats the value as unreadable: no subkey, `key_id: none`. *The `audit_hash_*.rs` suite* seeds a canary and expects keyed fields "with a key"; under Builder B it sees `none` and fails, or the test author weakens the parse.

**Clash.** AD-32's lifecycle knows four keychain outcomes (not-found, error, refusal, timeout). A malformed value is none of them, and "the choice fixes the stored form" makes the case real for the first time. Doctor's disk row reads green under A and red under B for the same keychain.

**Tightening (AD-32, after "an error, a refusal or a timeout creates nothing"):** "A value present but not in the stored form is an unreadable key: no subkey, no backup, `key_id: none`, Doctor's disk row red naming it, and it is never replaced or deleted. `am-testkit` holds one canary backup key that is a valid value of the stored form, used by the backup and `audit_hash_*` suites."

---

## M6. Medium: per-copy key identity has nowhere to live now that no format may be invented

**Two units.** *Doctor's disk row* must say "copies made under an earlier key need its recovery key", and *Settings › Backups* labels each copy "Encrypted · key in Keychain". Both need to know which key encrypted each copy. An age X25519 stanza carries no recipient identifier. The keyed-hash gate's fix (a key id "in each backup copy's header", review-2026-11-24-keyedhash-adversarial.md finding 13) is now barred by "no backup file format is invented".

**Clash.** Builder A puts the key id in the file name. Builder B records copies in a table in `state.db`, which a restore rewinds, so it lists copies the restored Mac lacks and misses newer ones. Builder C trial-decrypts each header at every Doctor run, which can say "not this key" but cannot name the earlier key. The three builds give different Doctor and Settings answers for the same folder.

**Tightening (AD-32, Doctor sentence):** "A copy's file name carries the key id of the key that encrypted it (`<UTC timestamp>.<key id>.<the format's extension>`); no header field, sidecar or table records it. Doctor and Settings › Backups read it from the name, and `agentic backups decrypt` names it when the key in hand does not match."

---

## M7. Medium: the restore words are tied to surfaces, not to the state that loses verification, and they name only notes

**Two units.** *`agentic backups decrypt`* prints the words "when it decrypts with a typed recovery key". *docs/restore.md* has them in "its steps for another Mac".

**Clash.** The loss happens whenever a store's entries were hashed under a key the open no longer holds, and that does not line up with either trigger:
- the owner decrypts on the old Mac with its keychain key and carries the plaintext copy over: the CLI says nothing;
- on the same Mac after a deleted entry or a keychain reset, the CLI says "on the new Mac", which is wrong;
- on the same Mac, a copy older than a key change is restored: no key is created, no record is written (H3), and no surface says anything.

The words also say "keyed note hashes", while model-output hashes and unknown-tool labels are lost too: the same model-chosen tool name gets a different `unknown:<key id>:<hex>` label after the restore, so Inbox words and the journal stop matching across it. A CLI test and a docs check pinning "the words" can each pin a different sentence.

**Tightening (AD-32, *Restore on another Mac*):** "One sentence, a constant in `am-store`, states that keyed audit hashes (notes, model output, unknown tool names) made under an earlier key can no longer be matched on this Mac, and that the audit chain still verifies. Doctor's disk row shows it whenever the chain records a key switch (H3); `agentic backups decrypt` prints it when given a typed key; docs/restore.md quotes it verbatim; a later Restore screen shows it."

---

## M8. Medium: "decrypt a backup over `state.db`" leaves `-wal`/`-shm` beside the restored file

**Two units.** *docs/restore.md* follows 9.10's text ("stop the app, decrypt a backup over state.db, start it"); *`agentic backups decrypt --out <path>`* writes exactly the given path.

**Clash.** The project store runs in WAL mode (AD-7). Writing a new `state.db` while the old store's `state.db-wal` and `state.db-shm` remain lets SQLite apply a stale WAL to the restored file at next open: corruption or mixed state, possibly a broken audit chain. AD-17 handles exactly this for the user store's set-aside ("moves `user.db` together with its `-wal`, `-shm` and `-journal` files"). AD-32 says nothing for the project store, and the update now sends the owner through these steps on another Mac too, where a previous open may have left a fresh store and its WAL.

**Tightening (AD-32, restore sentence):** "A restore never writes over a store's files in place: `agentic backups decrypt --out` refuses an existing path; docs/restore.md, with the app stopped and the project lock free, moves `state.db` with its `-wal`, `-shm` and `-journal` files aside before placing the decrypted copy."

---

## M9. Medium: "no crate-licence exception" leaves `[[licenses.clarify]]` open and blocks a future aws-lc-sys security bump without a rule

**Two units.** *Today*, aws-lc-sys 0.47.0 declares `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`: every term is on the allow-list, so cargo-deny passes with no entry (checked). *A security-fix story* (the Stack row: "a security fix moves this row, `aws-lc-sys` and `THIRD_PARTY.md`'s AWS-LC entry together") meets a later aws-lc-sys that declares its CC0 part (`AND CC0-1.0`) or another term cargo-deny refuses.

**Clash.** Builder A adds a `[[licenses.clarify]]` entry rewriting aws-lc-sys's expression to the allowed subset, arguing the native-library rule judges those parts and a clarify is not an "exception". Builder B halts the security fix, because AD-3 says `deny.toml` "gains none for them" and "a further exception is an owner decision". A silently overrides what cargo-deny sees; B leaves a known-vulnerable TLS library in place with no stated path.

**Tightening (AD-3, after "so `deny.toml` gains none for them"):** "`deny.toml` holds no `[[licenses.clarify]]` entry. If a native library's crate ever declares a licence outside the allow-list, CI fails and the bump stops for the owner, who chooses between a per-crate exception at that version (as for `notify`) and another crate; the security fix waits on that answer, and the stopped bump records why."

---

## L1. Low: the owner's "restore screen later" has no Deferred row and no owner

The owner's words are about "the restore screen later"; the spine says "a Restore screen added later says the same", but no Deferred row records a Restore action, so a v1.x builder reading the Deferred table finds nothing to carry the words. **Fix:** add a Deferred row: "Restore action in the UI | v1 restores through docs/restore.md and `agentic backups decrypt` (epic 9 Notes, 2026-11-18); a screen must show AD-32's restore sentence | v1.x".

## L2. Low: AD-25 does not carry the confirmed exception, and the release-gate claim overstates

AD-25 says a hash of "model output" in a project-store entry is keyed, and that "files" stay bare; AD-32 now says model output a seat writes to a file is hashed bare. A builder of the classifier working from AD-25 can classify `folder.write` as text when its bytes came from a model. The workspace file sidecar (AD-10 (c), "keyed by path and content hash") also stores a bare hash of model output in `state.db`. Story 11.8's text ("a release build has no bare hash of free text") is therefore not true, by the owner's own accepted default, and `audit_hash_*.rs` passes only because "the canary is never written as a file". **Fix:** one clause in AD-25 ("model output written to a file is hashed as file bytes, AD-32"), and one in AD-32's test sentence naming the accepted residual (file writes and the workspace sidecar).

## L3. Low: "cargo-deny advisories run" names a check the repo deliberately does not run

`just supply-chain` runs cargo-deny for `licenses bans sources` only, and `deny.toml` says "Advisories are cargo-audit's job". cargo-deny's advisories check with default config also flags unmaintained, unsound and yanked crates, so two architects running "a clean `cargo audit` and cargo-deny advisories" with different tools or configs reach different verdicts on age. **Fix:** "a clean `cargo audit` (as `just supply-chain` runs it) on age's graph, and cargo-deny licenses, bans and sources green on both targets"; unmaintained-crate status is part of the maintenance check, stated in the plan.

## L4. Low: "chacha20poly1305 is no longer a candidate" vs age's own required dependency on it

age 0.12.1 requires `chacha20poly1305`, `hkdf`, `hmac`, `sha2`, `sha3`, `scrypt`, `x25519-dalek`, `hpke` (p256), `ml-kem`, `p256`, `bech32`, `i18n-embed` and `rust-embed`. An architect reading "RustCrypto `chacha20poly1305` is no longer a candidate" as "must not be in the graph" fails age at once. **Fix:** "not a candidate as a direct dependency; age's own graph is judged by age's checks". Note that age's graph is much larger than the earlier "hmac, hkdf 0.12 and sha2 0.10" description suggested.

## L5. Low: "reveal it once" is now load-bearing

AD-17 ("gives the key out once as the recovery key") and AD-32 ("the UI can reveal it once") can be read as one reveal per key lifetime; story 9.10 offers "Show recovery key…" behind a second confirmation with no limit. With the confirmed default (a restore on another Mac gets a new key and decrypt never stores the old one), the recovery key is the only way to any existing copy once the Mac is gone; a builder enforcing a single lifetime reveal makes a lost note unrecoverable. This was raised at the keyed-hash gate (finding 20) and is now sharper. **Fix:** "Show recovery key may be used again, each time behind its confirmation, and each reveal is audited."

---

## Checked and fine

- The owner's verbatim answers in AD-3, AD-32, the Deferred row and Open Questions match the memlog's record word for word; the mapping (owner (1) → earlier (3), (2) → (4), (3) → (1) and (2)) is right.
- aws-lc-sys 0.47.0's declared licence expression is wholly on AD-3's allow-list; `deny.toml` holds only `notify@8.2.2`'s exception, so "no crate-licence exception" is true today with no change to `deny.toml`.
- aws-lc-sys's crate-root `LICENSE` carries the full BSD-3-Clause text, the Jitter Entropy copyright line with Amazon's election over GPL-2.0, and the CC0 note for the Keccak and AES reference code. It differs from `aws-lc/LICENSE` only by trailing whitespace on the last line. The Jitter Entropy library's own `LICENSE`, which it refers to, is not in the packaged crate, so the crate-root file is the right one to ship, as the spine says.
- The keyed-hash derivation, key id, classifier and no-fallback rules are unchanged by this update. "Only not-found creates a key" is consistent with "decrypt never writes the typed key to the keychain" (H3 is about the constructor, not this rule).
- No AD is renumbered or added. AD-3's Binds now names the release pipeline. AD-33's family list already names `audit_hash_*`.
- age 0.12.1 is current on docs.rs, with no default features. Its licence (workspace-inherited) and maintenance are the plan stop's to check, as the spine says.

## Owner calls raised by this review

- H2 changes what the owner approved only in form: the subkey still derives from the per-project backup key, but the stored text is fixed now instead of at the plan stop. Worth a one-line yes.
- H4 needs the owner to pick the tool that generates the notices file (a new dependency and a licence matter, so it is "always ask" under the standing rules).
- M9's "the security fix waits on the owner" is a trade-off the owner may prefer to settle in advance (for example, pre-approving a per-crate CC0-1.0 exception for `aws-lc-sys`, as was done for `notify`).
