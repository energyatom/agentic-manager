# Adversarial review: aws-lc-rs for the keyed audit hash (AD-3, AD-32, Stack, THIRD_PARTY.md) (2026-11-24)

- **Reviewed:** the working-tree diff in this worktree on 2026-11-24. Spine pieces: AD-3's native-library parenthetical, AD-32's "Both functions come from `aws-lc-rs`" sentence, the new Stack row `aws-lc-rs` 1.19.0, the Open Questions "Resolved 2026-11-24" and "For Sofia" lines, and the unchanged Deferred row "Backup encryption cipher crate". Also the last three `.memlog.md` entries, `THIRD_PARTY.md`, `deny.toml`, the workspace `Cargo.toml`, `Cargo.lock` and `xtask/src/deps.rs`.
- **Evidence:** `cargo tree --offline --locked` on `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` and `--target all`, and the registry sources of `aws-lc-rs` 1.19.0, `aws-lc-sys` 0.47.0, `rustls` 0.23.46, `rustls-webpki` 0.103.17, `reqwest` 0.13.5 and `objc2-exception-helper` 0.1.1. Nothing was built.
- **Scope:** the owner's crate answer, quoted verbatim in the memlog ("reuse that family and add no new crate"), and the keyed-hash decision are taken as final. The lens: two units, one level down, that each follow every AD to the letter yet do not fit together, or a lawful path that defeats the intent.
- **Verdict:** **accept with changes.** 0 critical, 2 high, 3 medium, 7 low. The crate choice is correct: `aws-lc-rs` is compiled into both CI graphs with exactly `aws-lc-sys,prebuilt-nasm`, `hkdf`, `hmac` and `aead` are not feature-gated in 1.19.0, the licences fit AD-3, and AWS-LC is 5.12.0. Four things need fixing before 9.10 starts:
  - the backup cipher row was left inconsistent with the owner's reuse rule (**owner call**);
  - the trust test cannot independently recompute a keyed hash without breaking "only the key module calls them";
  - nothing re-checks the "already compiled in, rustls's features only" premise after 9.10;
  - AD-3's new count of linked native libraries is again not exhaustive.

| # | Severity | Finding |
|---|---|---|
| 1 | high | The Stack row limits `aws-lc-rs` to HKDF/HMAC, and the Deferred row offers only `age` or RustCrypto `chacha20poly1305`. So story 9.10, following the letter, adds the second crypto family the owner's rule was written to avoid (**owner call**) |
| 2 | high | `audit_hash_*.rs` must "recompute under the subkey", but only the key module may call HKDF/HMAC and no new crate may be added. The only test that fits the letter is a self-check that cannot catch a wrong derivation |
| 3 | medium | "Already compiled in" and "only the features rustls enables" are checked once, by 9.10. A later reqwest or rustls bump can change rustls's provider, its features or its `aws-lc-rs` floor and silently make the premise false, or fail to resolve against `=1.19.0` |
| 4 | medium | "Only the key module calls them" and "am-store only" are not enforced: `xtask check-deps` polices external crates only for `am-domain` and `am-glob` |
| 5 | medium | AD-3's amended "v1 links two" is not exhaustive. The desktop graph compiles `objc2-exception-helper`'s Objective-C shim through `cc` |
| 6 | low | "`Cargo.lock` gains no package and no feature": `Cargo.lock` records no features, and its diff is not empty |
| 7 | low | The 2026-11-24 check ran on the macOS graph only. "ring … not compiled on macOS" understates: it is compiled for no target |
| 8 | low | `THIRD_PARTY.md` pins AWS-LC 5.12.0 and `aws-lc-sys` 0.47.0 in prose, but `aws-lc-sys` is a `^0.47.0` transitive dependency, so a lock-only update makes the record stale |
| 9 | low | `aws_lc_rs::hmac::Key` is `Clone + Send + Sync`, and the keyed-hash handle may outlive the writer. AD-17's "a type private to it" does not say how the key is held |
| 10 | low | No crate note pins the exact `aws-lc-rs` calls and a test vector, although AD-32 says the derivation "never changes" |
| 11 | low | With a hex or base64 backup key, a typed recovery key that decrypts can still give a different subkey (only if owner question (1) is answered yes) |
| 12 | low | v1.x: on x86_64 Windows, `prebuilt-nasm` links prebuilt NASM objects, so "built from source" and "Bundled native binaries: None" stop being true there |

## Findings

### High

#### 1. The cipher row and the Stack row's scope push 9.10 into a second crypto family (**owner call**)

**Incompatible pair (both lawful, both in story 9.10):**

- **Unit A, the key module.** It follows AD-32 and the new Stack row: "`aws-lc-rs` (HKDF-SHA-256 and HMAC-SHA-256 for `am-store`'s audit-hash key module **only**…)". So `aws-lc-rs` may serve HKDF and HMAC and nothing else.
- **Unit B, the backup cipher.** It follows the unchanged Deferred row: "Backup encryption cipher crate (`age` or RustCrypto `chacha20poly1305`) … the crate is picked, pinned and added to Stack by the backups story". Ticket 9.10 agrees: "the cipher crate is picked, pinned and added to the Stack table here".

**What goes wrong:**

- `aws-lc-rs` 1.19.0 already ships `aead::CHACHA20_POLY1305` and `aead::AES_256_GCM`, not feature-gated (checked in `src/lib.rs`). The Stack row's "only" forbids using them. Both options the Deferred row allows add a new crate.
- So the letter of the spine produces what the owner's rule was written to prevent: "If aws-lc-rs or ring is already compiled in, *or the backup cipher already uses a crypto family*, reuse that family and add no new crate".
- AD-17 says one key module holds the backup key and the subkey and does both encryption and keyed hashing. That one module would then hold two crypto families.
- With `age`, the graph also gains RustCrypto `hmac`/`hkdf` 0.12 (the versions review notes `age` uses them internally). Those are the very crates the owner rule avoided, at an older line, and they bring `sha2` 0.10 next to the pinned 0.11.1 (`deny.toml` `multiple-versions = "warn"`, so CI does not fail).
- The architect's own memlog option (b) said a direct `aws-lc-rs` dependency "would also settle the Deferred cipher row". The update did not carry that through.

**The cipher choice also fixes the derivation forever.** AD-32 takes the HKDF input as "the entry's value as stored (its UTF-8 bytes)" and says "the derivation never change[s]". An `age` identity is stored as Bech32 `AGE-SECRET-KEY-1…`; a raw AEAD key would be hex or base64. Whatever 9.10 picks becomes part of the frozen subkey derivation.

**Trade-off the owner should see.** `aws-lc-rs` `aead` is one-shot: `seal_in_place` works on a buffer. A multi-GB `VACUUM INTO` copy would need a chunked format that 9.10 designs itself (nonce per chunk, final-chunk flag, key commitment), which is home-made format design. `age` gives a reviewed streaming format at the cost of the second family.

**Minimal fix wording:**

- **Deferred row:** "Backup encryption cipher (owner question 2026-11-24: does the 'reuse that family and add no new crate' rule cover the cipher? If yes: `aws-lc-rs` 1.19.0 `aead` (`CHACHA20_POLY1305` or `AES_256_GCM`), already linked with the same features, in a chunked format that story 9.10 specifies in `docs/crate-notes/aws-lc-rs.md`. If no: `age` or RustCrypto `chacha20poly1305`, each adding a second crypto family) | … | E9 story 10, after the owner's answer".
- **Stack row:** "(HKDF-SHA-256 and HMAC-SHA-256 for `am-store`'s key module, AD-32, and the backup cipher if the owner's answer to the Deferred cipher row picks it; …)".
- **Open Questions:** add the question. In the For-Sofia line, add: "9.10 does not pick the cipher before the owner's answer".

#### 2. The trust test cannot recompute keyed hashes independently without breaking a rule

**Incompatible pair:**

- **Unit A, the restriction.** AD-32 says "only the key module calls them", and the Stack row scopes `aws-lc-rs` to "`am-store`'s audit-hash key module only".
- **Unit B, the test.** AD-32 says `crates/am-e2e/tests/trust/audit_hash_*.rs` "checks that each keyed field recomputes under the subkey with its key id". The keyed-hash memlog entry adds: "Construction named (HKDF/HMAC-SHA-256) so the backup module, the audit module and the trust test agree".

**The test author has three ways to recompute, and each breaks something:**

- **Through `am-store`'s key module.** This needs a test-support export of the subkey or of a recompute function, against AD-17's "a type private to it". It is also a self-check: if the module derived the HMAC key wrongly, say by using the backup key directly (owner condition 1 forbids that) or a different `info`, the test would still pass.
- **With `aws-lc-rs` in `am-e2e`.** This breaks "only the key module calls them".
- **With RustCrypto `hmac`/`hkdf` as dev-dependencies.** This breaks the owner's "add no new crate".

**Precedent.** `rule_16_audit_log.rs` recomputes a bare sha256 with its own `sha2` dev-dependency (`sha256_hex`, line 48). A builder working to the letter would take the first way, which defeats the gate's purpose.

**Minimal fix wording (AD-32, after "only the key module calls them"):** "…in shipped code. `am-e2e` takes `aws-lc-rs` as a dev-dependency with the same pin and features, which adds no `Cargo.lock` package. `audit_hash_*.rs` recomputes the subkey, the key id and every keyed field itself from the canary backup key it seeds in FakeSecretStore, never through `am-store`. It also checks one fixed test vector (a backup-key text, its key id, and the HMAC of a fixed text) that story 9.10 records in `docs/crate-notes/aws-lc-rs.md`, computed once by an independent implementation, so any later crate swap must reproduce it."

`am-e2e` is already `Layer::Exempt` in `xtask/src/deps.rs`, so the dependency edge is allowed. The call-site rule of finding 4 needs the same exemption for `crates/am-e2e/tests/`.

### Medium

#### 3. Nothing keeps the "already compiled in, rustls's features only" premise true after 9.10

**Incompatible pair:**

- **Unit A, story 9.10.** It adds `aws-lc-rs = { version = "=1.19.0", default-features = false, features = ["aws-lc-sys", "prebuilt-nasm"] }`, following the workspace's exact-pin style and the Stack row, and checks `cargo tree -e features` once.
- **Unit B, a later story.** It bumps `reqwest` or `rustls` as a Stack row change in its own story.

**Lawful outcomes:**

- **(i) The premise becomes false silently.** The bump moves TLS to ring (reqwest's `rustls-no-provider` plus a ring provider, or rustls's `ring` feature, a plausible v1.x move for Windows build ease). AWS-LC then stays linked only because of `am-store`, beside ring's own C and assembly. Nothing fails.
- **(ii) Resolution fails, and the fix has no owner.** The new rustls needs `aws-lc-rs ^1.20` (rustls 0.23.46 asks for `"1.18"`). Two semver-compatible versions cannot coexist, so resolution fails against `=1.19.0`. The reqwest story then has to bump a pin the Stack row says "added by epic 9 story 10", and no rule says it moves with reqwest.
- **(iii) The C library under `am-store` changes.** Any crate that turns on `aws-lc-rs/fips` (rustls `fips`, `rustls-webpki` `aws-lc-rs-fips`) swaps `aws-lc-sys` for `aws-lc-fips-sys` through feature unification. `aws-lc-rs`'s own `Salt::none` docs say "Not allowed in FIPS mode".

**What already helps.** Security updates to `aws-lc-rs` are now a Stack change, not a lock-only sweep. That matches the `url` 2.5.9 precedent but is worth stating.

**Minimal fix wording (Stack row, append):** "`aws-lc-rs` moves with `reqwest` in one story. `just supply-chain` fails when, on either CI target, `aws-lc-rs` is not also reached through `rustls`, or when its resolved features differ from `aws-lc-sys, prebuilt-nasm`. A change of rustls's crypto provider re-opens AD-32's crate choice for the owner. The derivation is HKDF/HMAC-SHA-256 over pinned bytes, so its outputs do not depend on the crate."

#### 4. "Only the key module" and "am-store only" are unenforced

`xtask/src/deps.rs` checks external crates only for `am-domain` (`DOMAIN_EXTERNALS`) and `am-glob` (`GLOB_EXTERNALS`). Its call-site rules cover only `TrustedInput`, `Instruction::authored` and the resolve.

**Incompatible pair:**

- **Unit A:** 9.10's key module.
- **Unit B, either of:**
  - a later adapter story (for example an AD-39 API Source that needs HMAC request signing, or a v1.x webhook check) that adds `aws-lc-rs` to `am-net` or `am-connectors` because it is "already compiled in";
  - `am-store`'s backup or `audit.rs` code calling `aws_lc_rs::hmac` directly instead of going through the key module.

Both compile and pass CI. The Stack row's "only" is the sole guard, and it is prose.

**Minimal fix wording:**

- **AD-17**, naming the file: "the key module (`crates/am-store/src/keys.rs`)".
- **Stack row:** "`xtask check-deps` fails when a crate other than `am-store` (and `am-e2e` as a dev-dependency) declares `aws-lc-rs`, or when `aws_lc_rs::` is named outside `crates/am-store/src/keys.rs` and `crates/am-e2e/tests/` (a call-site rule beside `TrustedInput`'s)".

#### 5. AD-3's new count is again not exhaustive

**Incompatible pair:**

- **Unit A, the record.** AD-3 now says "v1 links two, both compiled from source into the binary: SQLite … and the AWS-LC C library". `THIRD_PARTY.md`'s "Native libraries compiled into the binary" lists the same two.
- **Unit B, the desktop graph.** Epic 11's Tauri app (`am-desktop`, a v1 deliverable) compiles `objc2-exception-helper` 0.1.1's `src/try_catch.m` with `cc`. `objc2` 0.6.5 has the `exception` feature on through `tao` and `wry`: `cargo tree --target aarch64-apple-darwin --workspace -i cc --depth 1` shows `aws-lc-sys`, `libsqlite3-sys`, `objc2-exception-helper` and `embed-resource`, the last a no-op on macOS. Its licence is `Zlib OR Apache-2.0 OR MIT`, so it is allowed.

This is the same class of untrue exhaustive claim the update set out to fix ("v1 links none").

**Minimal fix wording (AD-3):** "(amended 2026-11-24: native code compiled from source into the binaries is recorded in `THIRD_PARTY.md`: SQLite and the AWS-LC C library in `agentic`, and `objc2-exception-helper`'s Objective-C shim in the desktop app; pdfium was dropped…)". Add the third entry to `THIRD_PARTY.md`. Optionally, `just supply-chain` lists the crates that build native code through `cc` on the whole macOS graph and fails on one `THIRD_PARTY.md` does not name.

### Low

#### 6. The `Cargo.lock` check is mis-worded

The Stack row says "`Cargo.lock` gains no package and no feature, which the story checks with a `Cargo.lock` diff". Two problems:

- `Cargo.lock` records no features.
- The diff is not empty: `am-store`'s `dependencies` list gains `"aws-lc-rs"`.

A builder or reviewer who expects an empty diff will fail the story or wave the check through.

**Fix:** "`Cargo.lock`'s only change is `aws-lc-rs` added to `am-store`'s dependency list (no new `[[package]]`); `cargo tree -e features` on both CI targets shows `aws-lc-rs` with exactly `aws-lc-sys,prebuilt-nasm` and `aws-lc-sys` with `prebuilt-nasm`."

#### 7. The check ran on one target, and the ring claim understates

The memlog records `--target aarch64-apple-darwin` only. On the Linux graph (AD-47) the features are the same (checked in this review). `cargo tree --target all -i ring` prints nothing, so ring is compiled for no target; its `Cargo.lock` entry is a weak-feature artifact from `rustls-webpki` and `quinn-proto`.

**Fix (Open Questions "Resolved" line):** "`ring` 0.17.14 sits in `Cargo.lock` only through weak features and is compiled for no target; checked on both CI targets".

#### 8. `THIRD_PARTY.md` versions can go stale

`aws-lc-rs` 1.19.0 declares `aws-lc-sys` `version = "0.47.0"`, which is a caret range, and AWS-LC's version is a property of `aws-lc-sys`. A lock-only `cargo update -p aws-lc-sys` (for an AWS-LC advisory, for example) moves past 5.12.0 with no Stack change, and the prose record goes stale.

**Fix:** "AWS-LC at the version `aws-lc-sys` in `Cargo.lock` bundles (5.12.0 with 0.47.0 on 2026-11-24); a PR that changes `aws-lc-sys` in `Cargo.lock` updates this line."

#### 9. `hmac::Key` handling is not stated

In `aws-lc-rs` 1.19.0, `hmac::Key`:

- is `#[derive(Clone)]` and `Send + Sync`;
- holds an `HMAC_CTX` that `HMAC_CTX_cleanup` clears on drop;
- when built through `From<hkdf::Okm>`, passes through an internal stack buffer that is not zeroized;
- has a `Debug` that prints only the algorithm, so it does not leak.

`Okm::fill` into a caller buffer would put the raw subkey into `am-store` code. AD-17 requires a private type but says nothing on copies or lifetime. A keyed-hash handle cloned into ToolGate can outlive its writer and keep labelling under an old key id. That cannot happen today, because one writer opens per process, but the desktop app switching projects in-process (AD-32) makes it reachable.

**Fix (AD-17, key module):** "derives the subkey straight into `hmac::Key::from(okm)` (never `Okm::fill` into a buffer it keeps), holds it in a private type that is not `Clone`, and the keyed-hash handle it gives `am-engine` carries the key id with the key and lives no longer than the writer it came from."

#### 10. No crate note

AD-5 requires notes only for pre-1.0 crates, and `aws-lc-rs` is 1.x. AD-32 nonetheless freezes a derivation that is built from exact calls:

- `hkdf::Salt::none(HKDF_SHA256)`, which is the same as `Salt::new(…, &[])` and RFC 5869's HashLen zeros;
- `extract(ikm)`;
- `expand(&[b"audit-hash-v1"], hmac::HMAC_SHA256)`;
- `hmac::Key::from(okm)`;
- `hmac::sign`.

The `sha2.md` precedent also carries "Do not let `sha2` types into `am-domain` or a port signature".

**Fix:** For-Sofia line: "9.10 writes `docs/crate-notes/aws-lc-rs.md` with these calls, the no-foreign-type rule and the test vector of finding 2".

#### 11. A recovery key that decrypts can still change the subkey

This matters only if owner question (1) in the Open Questions is answered yes, so that `agentic backups decrypt` may put a typed recovery key back into the keychain. With a hex or base64 key (the natural form for an `aws-lc-rs` AEAD key, finding 1), a decoder accepts case and whitespace variants. Backups would still decrypt, but AD-32's HKDF input is "the entry's value as stored (its UTF-8 bytes)", so the subkey would change silently and earlier keyed hashes would stop matching.

**Fix (attach to that open question):** "if yes, the key module stores a typed recovery key only in the exact canonical text it generates."

#### 12. v1.x Windows note

With `prebuilt-nasm`, `aws-lc-sys` links prebuilt NASM objects on x86_64 Windows. `THIRD_PARTY.md`'s "built from source" and "Bundled native binaries: None" would then be untrue there. This is a v1.x item only (AD-47). Note it in the Deferred Windows row or in `THIRD_PARTY.md`.

## Checked and fine

- **Graph and features.** `aws-lc-rs` 1.19.0 is reached through rustls 0.23.46, hyper-rustls 0.27.10 and reqwest 0.13.5 (`rustls`, then `__rustls-aws-lc-rs`), from `am-net`, on both CI targets. Its resolved features are `aws-lc-sys,prebuilt-nasm` on both; `aws-lc-sys` has `prebuilt-nasm`. rustls's `aws_lc_rs` feature turns on exactly `aws-lc-rs/aws-lc-sys` and `aws-lc-rs/prebuilt-nasm`.
- **Defaults.** `aws-lc-rs`'s defaults are `aws-lc-sys`, `alloc`, `ring-io` and `ring-sig-verify`. The last two would add `untrusted` and turn on the `io` module and ring signature verification, so `default-features = false` is right. `alloc` gates nothing in 1.19.0.
- **Module gating.** `hkdf`, `hmac` and `aead` are not feature-gated; only `fips` and `ring-*` gate code.
- **Licences.** `aws-lc-rs` is `ISC AND (Apache-2.0 OR ISC)`. `aws-lc-sys`'s expression is within `deny.toml`'s allow-list. Its `LICENSE` says OpenSSL-derived code is Apache-2.0 and that Amazon elects BSD-3-Clause, not GPL-2.0, for jitterentropy.
- **Version.** `AWSLC_VERSION_NUMBER_STRING` is "5.12.0" in `aws-lc-sys` 0.47.0.
- **No salt.** An empty salt in `aws-lc-rs` equals RFC 5869's HashLen zeros (`Salt::none` docs), so another library reproduces the subkey from the same bytes.
- **AD-47.** `aws-lc-sys` already builds in the Linux job for `am-net`. `am-store` gains no OS-specific code and no new platform risk.
- **AD-4 and AD-5.** Adding an external crate to `am-store` adds no workspace edge. `aws-lc-rs` is 1.x, so no port is required; AD-17 already keeps the key type private and gives other crates only keyed hash text.

## Owner calls

- **Finding 1:** whether the owner's "reuse that family and add no new crate" rule also governs the backup cipher (`aws-lc-rs` `aead` with a self-specified chunked format, against `age`'s reviewed format and a second crypto family).
- **Finding 3 (recorded, not asked now):** a future change of rustls's crypto provider re-opens the AD-32 crate choice.
