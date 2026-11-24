# Rubric review: keyed-hash crate is aws-lc-rs (AD-3, AD-32, Stack, Open Questions, THIRD_PARTY.md; 2026-11-24)

Reviewer: rubric walker (good-spine checklist). Scope: the last three `.memlog.md` entries (owner keyed-hash crate decision, AD-3 corrected, open question resolved) and what they changed: AD-3's native-library parenthetical, AD-32's "Both functions come from `aws-lc-rs`" sentence, the new Stack row, the resolved Open Questions entry, the `aws-lc-rs` clause of the 2026-11-24 "For Sofia" entry, and `THIRD_PARTY.md`'s new "Native libraries compiled into the binary" section. The rest of the 2026-11-24 keyed-hash wording was reviewed in `reviews/review-2026-11-24-keyedhash-*.md` and is not re-reviewed here. Checked against AD-3, AD-4 (Design Paradigm table, `xtask/src/deps.rs`), AD-5, AD-12, AD-17, AD-47, Deferred, `deny.toml`, `justfile` (`supply-chain`), the workspace and `am-store` manifests, `Cargo.lock`, and the registry sources of `aws-lc-rs` 1.19.0, `aws-lc-sys` 0.47.0, `rustls` 0.23.46, `rustls-webpki` 0.103.17, `libsqlite3-sys` 0.40.0 and `objc2-exception-helper` 0.1.1. Commands run: `cargo tree --offline --locked` (inverted, `-e features`, on `aarch64-apple-darwin` and on `x86_64-unknown-linux-gnu` without `am-desktop`) and `lint_spine.py` (0 findings). No build, test or check was run.

## Verdict

**Accept with changes.** No critical or high finding. Every fact in the update that I checked is true: versions, the dependency path, the feature set, the licence expressions, the AWS-LC version, and that `ring` is not compiled. The decision follows the owner's rule as written: the crate is already compiled in, the direct dependency adds no package, and its feature set matches rustls's exactly, so the shared target directory and the Linux job reuse the same `aws-lc-sys` artefact. No AD is contradicted. No text in the spine still says "v1 links none" or leaves RustCrypto `hmac`/`hkdf` open.

Five medium findings should go in before Sofia's REPLAN writes story 9.10:

- **M1:** the feature check is a one-off and loosely stated, and nothing keeps the feature set after 9.10.
- **M2:** "only the key module calls them" has no check.
- **M3:** the Deferred cipher row still offers only options that add a second crypto family to the same key module. That makes the resolved entry's "no RustCrypto crate is added" false in the same story if either option is picked.
- **M4:** `THIRD_PARTY.md` records AWS-LC's licence more narrowly than AWS-LC's own `LICENSE`, and the gap falls on the two licences AD-3 polices, CC0 and GPL.
- **M5:** the derivation is named only by module, with no known-answer vector, so "the derivation never changes" cannot be tested.

## The two questions asked

**Could 9.10 add `aws-lc-rs` and still change `Cargo.lock` or the feature set?** Yes, in three ways the spine's wording does not close.

1. `aws-lc-rs`'s default `alloc` feature is empty in 1.19.0 and changes nothing in `Cargo.lock`. Only `cargo tree -e features` sees it, and the spine gives neither the command nor the expected output.
2. The defaults `ring-io` and `ring-sig-verify` would add `untrusted` 0.7.1, since `Cargo.lock` holds only 0.9.0, through `ring`. A lock diff catches that. But the spine says the lock gains "no package and no feature" when it does change by one line (`am-store`'s dependency list), so a reviewer reading the diff has no exact expectation to compare against.
3. After 9.10, any story can change `am-store`'s entry (for example `fips`, `bindgen`, `unstable`, `legacy-des`, or the defaults), and neither `just check` nor `just supply-chain` fails. See M1.

**Does any text still say "v1 links none" or offer RustCrypto as open?**

- "links none": no hits.
- "RustCrypto": two hits. The resolved Open Questions entry quotes the owner and closes the question. The Deferred row "Backup encryption cipher crate (`age` or RustCrypto `chacha20poly1305`)" is a different, still-open choice, but it now conflicts with the reuse rule and with the resolved entry (M3).
- The keyed-hash update's "the crate is an owner question" is gone, and no hits remain for `hmac 0.13`, `owner question` or `no Stack crate`.

## Checklist walk

| Check | Result |
|---|---|
| Fixes the real divergence points for the level below, misses none | Mostly. Fixed: which crate, which modules, direct dependency in `am-store` only, features matched to rustls, licence recorded. Missed: a lasting feature guard (M1), confinement of the crate to the key module (M2), the cipher choice made in the same story (M3), the exact calls and a known-answer vector (M5) |
| Every rule enforceable and actually prevents its divergence | Partly. The feature rule is checked once, by a loosely described check (M1). The "only the key module" rule has no check (M2). "The derivation never changes" has no vector (M5) |
| Nothing under Deferred lets two units diverge | No: the cipher row lets 9.10 put a second HMAC/HKDF implementation (`age`'s) beside `aws-lc-rs` in one key module (M3) |
| Ratifies, not contradicts, the existing code | Yes. `aws-lc-rs` 1.19.0 is in the graph on both CI targets with exactly `aws-lc-sys` and `prebuilt-nasm`; `am-store` depends today on `am-domain` plus external crates (`crates/am-store/Cargo.toml`); `check-deps` checks only workspace edges |
| No contradiction with other ADs | AD-4, AD-5, AD-12, AD-17 and AD-47: consistent (see the last section). AD-3: consistent, but the parenthetical's count is scoped to the `agentic` binary without saying so (L1), and its native-library rule does not say how a native library's public-domain or CC0 parts are judged (M4) |
| Stack table conventions | Exact version given; notes follow the style of the nix row (licence, why, already in the graph). The "checked … date" phrase used by the nix row and others is missing, and "rustls enables" is really rustls plus rustls-webpki (L3) |
| `THIRD_PARTY.md` style and truth | Style fits the file. Every stated fact is true; the licence record is incomplete (M4); version facts can drift (L5) |

## Findings

### M1 — medium — The 9.10 feature check is stated loosely, is one-off, and nothing keeps the feature set after 9.10

**Spine text (Stack row):** "a direct dependency of `am-store` added by epic 9 story 10 with `default-features = false` and only the features rustls enables (`aws-lc-sys`, `prebuilt-nasm`), so `Cargo.lock` gains no package and no feature, which the story checks with a `Cargo.lock` diff and `cargo tree -e features`"

**Problem:**

- `Cargo.lock` records no features, and it does change: `am-store`'s `dependencies` gains `"aws-lc-rs",`.
- The default `alloc` feature (`alloc = []` in 1.19.0's manifest) changes no lock line, so only `cargo tree` catches it.
- No command, target or expected output is given.
- After 9.10, a later story that flips `am-store`'s entry (enabling `fips`, `bindgen`, `unstable`, `legacy-des`, `asan`, or dropping `default-features = false`) passes `just check` and `just supply-chain`.

That loses the shared-cache property the matched feature set buys: building `-p am-store` alone today compiles the same `aws-lc-rs`/`aws-lc-sys` artefact as the workspace build. Once the sets differ, it builds a second one, and `fips` would add `aws-lc-fips-sys` and its cmake/Go build.

**Proposed fix (Stack row):** replace "so `Cargo.lock` gains no package and no feature, which the story checks with a `Cargo.lock` diff and `cargo tree -e features`" with:

> pinned `=1.19.0` in `[workspace.dependencies]`; the story checks that the `Cargo.lock` diff is the one line `"aws-lc-rs",` in `am-store`'s dependencies, with no `[[package]]` added or changed, and that `cargo tree --locked -e features -i aws-lc-rs` on the `aarch64-apple-darwin` graph and on the `x86_64-unknown-linux-gnu` graph without `am-desktop` shows exactly the features `aws-lc-sys` and `prebuilt-nasm` (no `default`, `alloc`, `ring-io`, `ring-sig-verify` or `fips`). It also adds a `[[bans.features]]` entry to `deny.toml` (`crate = "aws-lc-rs"`, `allow = ["aws-lc-sys", "prebuilt-nasm"]`, `exact = true`), so `just supply-chain` fails on any later change to that set.

(Confirm the `bans.features` key names against cargo-deny 0.21.0, the installed version, when writing it. If they do not fit, use an `xtask` check of equal force over `cargo metadata`'s resolved features.)

### M2 — medium — "Only the key module calls them" has no check, and any crate can add `aws-lc-rs` without changing `Cargo.lock`

**Spine text:**
- AD-32: "Both functions come from `aws-lc-rs` (…; Stack), which rustls already compiles into the binary, so no new crate is added (…); only the key module calls them."
- Stack: "for `am-store`'s audit-hash key module only".

**Problem:**

- As a direct `am-store` dependency, `aws_lc_rs` is open to every `am-store` module, including `aws_lc_rs::digest`, which computes a bare SHA-256 of anything.
- Any other crate can add `aws-lc-rs.workspace = true` with no new lock package and a clean cargo-deny run. For example, `am-engine` could compute its own unknown-tool label instead of using AD-32's keyed-hash handle.
- `cargo xtask check-deps` checks only workspace-crate edges ("Only normal and build edges count", `xtask/src/deps.rs`), so neither case fails.
- Stories 3.3 and 3.4 already added call-site token rules to `check-deps` for the same kind of "only X may call Y" rule.
- The trust test's "recomputes under the subkey" needs HKDF/HMAC in test code, and as written that breaches the "only" rule.

**Proposed fix (AD-32, after "only the key module calls them"):**

> ; `cargo xtask check-deps` enforces it (story 9.10): no workspace crate but `am-store` has a normal or build edge to `aws-lc-rs`, and the token `aws_lc_rs` appears in shipped source only in the key module's file (tests, `#[cfg(test)]` items and `am-e2e`'s dev-dependency excepted, as for the story 3.3 and 3.4 call-site rules); the trust test recomputes through `aws-lc-rs` directly, so its check does not reuse the code it tests

Add `xtask` to AD-32's **Binds**.

### M3 — medium — The Deferred cipher row offers only a second crypto family, contradicting the resolved entry in the same story

**Spine text:**
- Deferred: "| Backup encryption cipher crate (`age` or RustCrypto `chacha20poly1305`) | Encryption at rest is decided (AD-32, 2026-11-18); the crate is picked, pinned and added to Stack by the backups story | E9 story 10 |"
- Open Questions, resolved entry: "…; no RustCrypto crate is added."

**Problem:**

- Story 9.10 both adds `aws-lc-rs` to `am-store` and picks the backup cipher, which goes in the same key module (AD-17: one key module holding the backup key and the subkey).
- `aws-lc-rs` 1.19.0's `aead` module (CHACHA20_POLY1305, AES_256_GCM) is compiled in and not feature-gated (`pub mod aead;` with no `cfg`).
- The row offers only options that add RustCrypto crates. `chacha20poly1305` does so directly. `age`, per the 2026-11-24 versions review recorded in the memlog, also brings RustCrypto `hmac` and `hkdf` 0.12, a second HMAC/HKDF beside `aws-lc-rs` in one module.
- Either pick makes the resolved entry's unscoped "no RustCrypto crate is added" false in the same story, and it is the outcome the owner's rule ("reuse that family and add no new crate") was written to avoid.
- The architect's own earlier note ("(b) ring or aws-lc-rs … would also settle the Deferred cipher row") was not carried into the row.

**Proposed fix:**

Deferred row, first cell:
> Backup encryption cipher crate (`aws-lc-rs`'s `aead`, already a direct `am-store` dependency from the same story, AD-32; `age`; or RustCrypto `chacha20poly1305`)

Deferred row, second cell, append:
> `age` and `chacha20poly1305` add RustCrypto crates beside `aws-lc-rs` (`age` also brings `hmac` and `hkdf` 0.12); `aws-lc-rs`'s `aead` seals one buffer at a time, so a backup file needs a chunked format the story defines. Whether the owner's 2026-11-24 reuse rule for the keyed-hash crates also governs the cipher is asked before 9.10 picks.

Open Questions, resolved entry: change "no RustCrypto crate is added." to:
> no RustCrypto crate is added for the keyed hashes (the backup cipher crate is still open, Deferred).

### M4 — medium — `THIRD_PARTY.md` records AWS-LC's licence more narrowly than AWS-LC's own `LICENSE`, on the two licences AD-3 polices

**Text (`THIRD_PARTY.md`):** "Licence, as `aws-lc-sys` declares it: `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`; its `LICENSE` file carries the AWS-LC, BoringSSL and OpenSSL notices."

**Problem:** both statements are true, but the record is incomplete.

- `aws-lc-sys` 0.47.0's `LICENSE` also has mlkem-native and mldsa-native sections.
- Its "Third-Party Libraries (compiled into libcrypto/libssl)" section lists:
  - Fiat Cryptography (MIT)
  - s2n-bignum (Apache-2.0 OR ISC OR MIT-0)
  - the Jitter Entropy RNG: dual BSD-style or GPL-2.0, and Amazon "expressly elects" BSD-3-Clause and not GPL-2.0
  - "Keccak / AES Reference Implementations — Public domain (CC0) contributions"
- The cc builder compiles jitterentropy (`builder/cc_builder/universal.rs:256-260`).

AD-3 limits CC0-1.0 to `notify` ("any other CC0-1.0 crate fails CI") and fails GPL. Neither is breached:

- The CC0 code is native code, which falls under AD-3's native-library rule ("needs a permissive licence recorded"), as SQLite's public domain already does.
- The GPL option is not taken.

But neither the spine nor `THIRD_PARTY.md` says so. A release reviewer who opens the `LICENSE` meets an apparent conflict with AD-3 that the record does not answer.

**Proposed fix:**

`THIRD_PARTY.md`: replace "its `LICENSE` file carries the AWS-LC, BoringSSL and OpenSSL notices." with:
> Its `LICENSE` file carries the AWS-LC, BoringSSL and OpenSSL notices and those of the third-party code compiled into the library: mlkem-native and mldsa-native (Apache-2.0 OR ISC OR MIT), Fiat Cryptography (MIT), s2n-bignum (Apache-2.0 OR ISC OR MIT-0), the Jitter Entropy RNG (offered under BSD-3-Clause or GPL-2.0; AWS-LC distributes it under BSD-3-Clause only) and public-domain (CC0) Keccak and AES reference code.

AD-3 parenthetical, append before "pdfium was dropped":
> a native library's licence is judged by this rule (permissive, recorded), not by the crate allow-list: SQLite's public domain and the CC0 reference code inside AWS-LC are recorded in `THIRD_PARTY.md`, and AWS-LC's Jitter Entropy RNG is taken under BSD-3-Clause, never GPL-2.0;

If the owner reads the notify-only CC0 limit as covering native code too, this becomes an owner question before epic 11's release gate, not a 9.10 blocker: AWS-LC is already linked today through rustls.

### M5 — medium — The derivation is named by module only; "the derivation never changes" has no known-answer vector

**Spine text (AD-32):** "Both functions come from `aws-lc-rs` (`aws_lc_rs::hkdf` with `HKDF_SHA256`, `aws_lc_rs::hmac` with `HMAC_SHA256`; Stack)". Earlier in the same rule: "the label and the derivation never change".

**Problem:**

- `aws-lc-rs` 1.19.0 offers more than one way to get a "HKDF key":
  - `hkdf::Salt::none` and `Salt::new(HKDF_SHA256, &[])`, which are equal and correct;
  - `Salt::new` with any non-empty value, which gives different bytes;
  - `hkdf::Prk::new_less_safe`, which skips Extract and also gives different bytes.
- A builder who picks a wrong one still passes the trust test as worded, because the backup module, the audit module and the recompute share the code.
- The error surfaces only when the derivation is reimplemented, which "never change" forbids but cannot detect: after a crate swap, or when the cipher choice (M3) moves the module to another family.
- AD-5 names this hazard: "agents writing against hallucinated crate APIs".
- `aws-lc-rs` is 1.x, so AD-5 does not require a crate note.

**Proposed fix (AD-32, after "…`HMAC_SHA256`; Stack)"):**

> : `hkdf::Salt::none(HKDF_SHA256).extract(<value bytes>).expand(&[b"audit-hash-v1"], hmac::HMAC_SHA256)` into `hmac::Key::from`, then `hmac::sign`; a unit test in `am-store` pins RFC 5869 test case 3 (SHA-256, no salt, empty info) and one recorded vector (a canary backup-key value, its key id and the keyed hash of a fixed text) computed outside the code under test, so a change to the derivation fails a test

### L1 — low — "v1 links two" is an absolute count, scoped to the `agentic` binary without saying so

**Spine text (AD-3):** "v1 links two, both compiled from source into the binary: SQLite, … and the AWS-LC C library …"

**Problem:**

- The count is true of `am-app`'s graph (`cc`'s only build dependents there are `aws-lc-sys` and `libsqlite3-sys`).
- It is not true of the shipped Tauri app, which links `am-app` in-process (AD-32). In `am-desktop`'s graph, tao/objc2 bring in `objc2-exception-helper` 0.1.1, whose build script compiles `src/try_catch.m` with cc (crate licence Zlib OR Apache-2.0 OR MIT, already checked by cargo-deny).
- Nothing slips through licensing, but this update exists to correct an untrue absolute, so it should not leave another.

**Proposed fix:** "v1 links two" becomes:

> the `agentic` core links two (a crate's own small C or Objective-C glue compiled under that crate's licence, such as `objc2-exception-helper` in the desktop shell, is checked by cargo-deny as part of the crate and is not a native library here), both compiled from source into the binary:

### L2 — low — The "reused" premise has no tripwire

**Spine text (Stack row):** "owner, 2026-11-24: reused from rustls's graph, already compiled in, no new crate".

**Problem:**

- The owner's choice rests on reuse.
- If a later reqwest or rustls bump stops compiling `aws-lc-rs`, `am-store`'s exact pin keeps AWS-LC in the binary for HKDF/HMAC alone, against the reason it was chosen.
- If the bump needs a newer 1.x, resolution fails on the `=1.19.0` pin. That failure is acceptable, but the spine does not say what happens next.

**Proposed fix (Stack row, append):**

> ; a Stack bump that takes `aws-lc-rs` out of rustls's graph sends the keyed-hash crate back to the owner, and one that needs a newer `aws-lc-rs` bumps this row in the same story

### L3 — low — The Stack row's notes miss two conventions

**Spine text:** "only the features rustls enables (`aws-lc-sys`, `prebuilt-nasm`)".

**Problem:**

- `rustls-webpki` 0.103.17 also requests `aws-lc-rs` through its `aws-lc-rs` feature, with the same two features.
- Rows for crates reused from the graph name their check (nix: "already in the graph through process-wrap; checked in Cargo.lock and on crates.io 2026-11-22").

**Proposed fix:**

> only the features rustls and rustls-webpki enable (`aws-lc-sys`, `prebuilt-nasm`; checked with `cargo tree -e features` on both CI graphs, 2026-11-24)

### L4 — low — The resolved entry's `ring` fact is narrower than what was checked

**Spine text:** "`ring` 0.17.14 sits in `Cargo.lock` but is not compiled on macOS".

**Problem:** It is not compiled on the Linux CI graph either. `cargo tree --workspace --all-features -i ring --target all` prints nothing: `ring` is in the lock only as an optional dependency of `quinn-proto` and `rustls-webpki`. The narrower wording invites a reader to think Linux links two TLS crypto families.

**Proposed fix:** "is not compiled on macOS" becomes:
> is not compiled on any target (an optional dependency of `quinn-proto` and `rustls-webpki` only)

### L5 — low — `THIRD_PARTY.md` version facts can drift silently; the SQLite entry lacks the detail the AWS-LC entry gives

**Text:**
- "AWS-LC 5.12.0 … by the `aws-lc-sys` 0.47.0 crate"
- "SQLite: compiled from source into the binary by `rusqlite`'s `bundled` feature"
- "built from source and statically linked"

**Problem:**

- `aws-lc-rs` 1.19.0 requires `aws-lc-sys` `^0.47.0`, so a `cargo update` can move `aws-lc-sys` (and the AWS-LC version) without any Stack change, and the file goes stale.
- The SQLite entry names neither the `-sys` crate nor the SQLite version, though the AWS-LC entry names both (`libsqlite3-sys` 0.40.0, MIT, bundles SQLite 3.54.1).
- "built from source and statically linked" holds only while no `aws-lc-sys` build switch is set: its builder honours `AWS_LC_SYS_SYSTEM_DIR` (link a system AWS-LC) and `AWS_LC_SYS_STATIC`.

**Proposed fix (`THIRD_PARTY.md`):**

SQLite bullet:
> SQLite 3.54.1: compiled from source and statically linked by the `libsqlite3-sys` 0.40.0 crate (MIT) through `rusqlite`'s `bundled` feature; SQLite is in the public domain.

Add after the list:
> A change to `aws-lc-sys` or `libsqlite3-sys` in `Cargo.lock` updates this section in the same commit; release builds set no `AWS_LC_SYS_*` or `LIBSQLITE3_SYS_*` variable that links a system library.

(The release-job assertion belongs to epic 11; Sofia can note it there.)

### Nits

- **N1:** AGENTS.md's licence line ("A bundled native binary is recorded in `THIRD_PARTY.md`") and the epic 1 epic file ("bundled native binaries recorded in THIRD_PARTY.md") predate the new section. On the next `bmad-project-context` refresh, or by Sofia, add "and each native library compiled into the binary". This is not an edit for this update.
- **N2:** The memlog's latest status event still reads "Status partial: owner crate question (HMAC/HKDF dependency) open". After this gate, append the reviewer-gate event and a closing "spine updated/finalized" event, as the 2026-11-23 update did.

## Checked and consistent

- **Facts in the update:** all true.
  - `Cargo.lock` holds `aws-lc-rs` 1.19.0, `aws-lc-sys` 0.47.0, `rustls` 0.23.46, `hyper-rustls` 0.27.10, `reqwest` 0.13.5 and `ring` 0.17.14.
  - The feature path is `am-net` → reqwest `rustls` → `__rustls-aws-lc-rs` → hyper-rustls `aws-lc-rs` → rustls `aws_lc_rs` (= `aws-lc-rs/aws-lc-sys`, `aws-lc-rs/prebuilt-nasm`).
  - Resolved `aws-lc-rs` features on both CI graphs: exactly `aws-lc-sys` and `prebuilt-nasm`, with no `default`.
  - `aws-lc-rs`'s defaults are `aws-lc-sys`, `alloc`, `ring-io` and `ring-sig-verify`. The `ring-*` features pull in `untrusted` 0.7.1, which is not in the lock.
  - `hkdf` and `hmac` are not feature-gated (`src/lib.rs:256-257`; only `io` is gated, on `ring-io`). `HKDF_SHA256` (`hkdf.rs:72`), `HMAC_SHA256` (`hmac.rs:152`), `Salt::none` and `impl From<hkdf::Okm<hmac::Algorithm>> for hmac::Key` (`hmac.rs:345`) exist, so a 32-byte subkey is direct.
  - Licence fields match: `aws-lc-rs` is `ISC AND (Apache-2.0 OR ISC)`, and `aws-lc-sys` carries the stated expression. Every term is on the `deny.toml` allow-list, and cargo-deny already checks both crates today.
  - `AWSLC_VERSION_NUMBER_STRING "5.12.0"` is at `aws-lc/include/openssl/base.h:68`.
- **AD-4:** the Design Paradigm table and `check-deps` cover only workspace edges. `am-store` → external crate needs no table change.
- **AD-5:** `aws-lc-rs` is 1.x, so it needs no own port and no crate note. Its types stay private to the key module (AD-17), and it exposes a safe API, so `#![forbid(unsafe_code)]` holds.
- **AD-12:** the crate does no network I/O, and reqwest stays in `am-net` (the only path to rustls in the graph).
- **AD-17:** consistent. The subkey is a type private to `am-store`'s key module, `aws_lc_rs::hmac::Key`'s `Debug` prints no key, and `Prk` keeps its input in a zeroising buffer.
- **AD-47:** `aws-lc-sys` is already in the Linux portability job's graph through `am-net`. With identical features, `am-store`'s tests reuse that artefact. `am-store` uses no macOS-only API.
- **AD-3 for the `agentic` binary:** exactly two `cc`-built native libraries, `aws-lc-sys` and `libsqlite3-sys`.
- **Spine mechanics:** `lint_spine.py` reports 0 findings, and the frontmatter `updated` is 2026-11-24.
- **Owner decision:** quoted verbatim, and the same in the memlog, AD-32 and Open Questions.
