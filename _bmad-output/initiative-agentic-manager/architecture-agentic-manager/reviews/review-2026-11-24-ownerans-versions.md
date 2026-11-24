---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-24 owner answers (backup cipher, AWS-LC native-library parts, restore on another Mac). Changed: AD-3 Binds/Prevents and "Native-library parts" (line 121), AD-32 "The cipher" and "Restore on another Mac" (line 324), the release diagram (line 662), the Deferred cipher row (line 782), Open Questions (lines 817-819), and THIRD_PARTY.md
date: 2026-11-24
---

# Review: owner answers on the cipher, AWS-LC notices and restore, versions and facts

## Verdict

**Pass with changes: no critical, 2 high, 3 medium, 3 low.**

These facts check out:

- The `age` crate exists and is maintained. It is at 0.12.1 (2026-07-14), licensed `MIT OR Apache-2.0`, with empty default features.
- age's format is the published age v1 spec (C2SP). Its payload is a STREAM variant from ePrint 2015/189.
- `aws-lc-rs` 1.19.0's `aead` module has `CHACHA20_POLY1305` and `AES_256_GCM`. It has no streaming or chunked AEAD.
- `aws-lc-sys` 0.47.0's `LICENSE` contains the Jitter Entropy notice: BSD-3-Clause, "Amazon expressly elects" it over GPL-2.0. It also contains the CC0 Keccak/AES entry.
- The crate's declared licence expression contains only allow-listed identifiers. So the claim "no cargo-deny exception needed" is true in mechanics as well as by the owner's rule.

What needs changing:

- **H1:** the fallback's named example, "a STREAM construction implemented from its published specification", does not meet the owner's "never our own". STREAM is a construction in a paper, not a byte-level format.
- **H2:** the new release licence review covers only native libraries. The same notice duty applies to the Rust crates and npm packages the artefacts carry.

## Method

- **Diff:** the scratchpad `update-ownerans.diff`, plus `git diff --word-diff` of the spine.
- **Context read:** the full AD-3 rule, the AD-32 cipher, keyed-hash and restore paragraphs, Stack row 508, Deferred row 782, Open Questions 816-819, THIRD_PARTY.md, `deny.toml` and the `justfile` `supply-chain` recipe.
- **Local sources:** `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`:
  - `aws-lc-sys-0.47.0/LICENSE`, `Cargo.toml` (licence line 76), `aws-lc/include/openssl/base.h` (`AWSLC_VERSION_NUMBER_STRING "5.12.0"`), `builder/main.rs` and `builder/cc_builder.rs` (jitterentropy compiled, Apple targets included), and every `LICENSE*`/`NOTICE*` in the package.
  - `aws-lc-rs-1.19.0/src/lib.rs`, `src/aead.rs`, `src/cipher.rs`, `src/cipher/streaming.rs`, `src/agreement.rs`.
- **Locked graph:** `cargo metadata --offline --locked` for the licence census of the locked graph.
- **Web (fetched 2026-11-24):** crates.io API, GitHub, RustSec, C2SP, IACR ePrint, Google Tink docs and OSI. URLs are under Sources.
- **Not done:** no cargo build, test, clean or network resolution. `age` is not in the local registry, so its added package count was not resolved.

## Findings

### High

#### H1: The fallback example (STREAM "from its published specification") still leaves a format to invent

**Where:**
- AD-32 "The cipher" (line 324): "for example a STREAM construction implemented from its published specification".
- Deferred row (line 782): "for example a STREAM construction from its published specification".

**Reality:**
- STREAM comes from Hoang, Reyhanitabar, Rogaway and Vizár, "Online Authenticated-Encryption and its Nonce-Reuse Misuse-Resistance". That is IACR ePrint 2015/189, received 2015-03-04 with six revisions, the last on 2018-11-29. The abstract does not even name STREAM.
- The paper defines an abstract construction: a nonce prefix, a segment counter and a last-segment bit over a nonce-based AEAD. It fixes no byte layout.
- Building it means choosing the segment size, the nonce-prefix length and encoding, the key derivation, the header, versioning, and the final-chunk and truncation rules. Choosing all of that is designing a file format, which the owner's "never our own" and "do not invent a file format" forbid.

There are published schemes that pin those bytes:

1. **The age v1 format itself (c2sp.org/age).**
   - Header: an X25519 stanza and an HMAC-SHA-256 header MAC, keyed by HKDF-SHA-256 of the file key with info `header`.
   - Payload: a 16-byte nonce, then a payload key from HKDF-SHA-256 (salt is the nonce, info is `payload`). The data is split into 64 KiB ChaCha20-Poly1305 chunks. Each chunk nonce is an 11-byte big-endian counter plus a last-chunk byte, `0x01` on the last chunk.
   - C2SP CCTV publishes a large set of age test vectors (`C2SP/CCTV/age`) covering header, HMAC, payload and armor failures.
   - Every primitive it needs is in `aws-lc-rs` 1.19.0, already linked: `agreement::X25519`, `PrivateKey::from_private_key` for a static X25519 key from raw bytes, `agree`, `hkdf`, `hmac` and `aead::CHACHA20_POLY1305`.
   - Backups made this way stay readable by the `age`/`rage` CLIs, which helps recovery. Its weak point is a Bech32 encoder, needed only for the `AGE-SECRET-KEY-1…` text form.
2. **Google Tink's AES-GCM-HKDF Streaming.**
   - It documents its header (`len(Header) || Salt || NoncePrefix`) and its segment nonce: NoncePrefix, a 4-byte big-endian counter and a final-segment byte.
   - It is "loosely based on HRRV15" and cites Hoang and Shen (2020) for its security analysis.
   - It runs on `aws-lc-rs`'s `AES_256_GCM` and `hkdf`.
   - Its page does not define the whole wire layout: segment size is a key parameter, not a header field.

**Fix (both places):** replace the example with:

> "for example the age v1 format (c2sp.org/age) implemented on `aws-lc-rs`'s X25519, HKDF-SHA-256, HMAC-SHA-256 and ChaCha20-Poly1305 and checked against C2SP CCTV's age vectors, or Tink's AES-GCM-HKDF Streaming as its documentation specifies it; never STREAM from the paper alone, which fixes no byte layout".

This stays inside the owner's words ("a published chunking scheme on aws-lc-rs").

#### H2: The release licence review covers native libraries only, but crates and npm packages carry the same notice duty

**Where:**
- AD-3 Prevents and "Native-library parts" (line 121): "a release that ships a native library without its licence notices"; "every native library linked is recorded in `THIRD_PARTY.md` … and its notices are in the artefacts".
- THIRD_PARTY.md: "a licence review of this list".

**Reality:**

The locked graph has 575 registry packages (`cargo metadata --offline --locked`):

- 139 declare `MIT` alone. MIT requires "the above copyright notice and this permission notice" in all copies or substantial portions.
- 3 declare `BSD-3-Clause` alone: `subtle` 2.6.1, `alloc-no-stdlib` 3.0.0 and `alloc-stdlib` 0.3.0. BSD-3-Clause condition 2 makes binary redistributions "reproduce the above copyright notice, this list of conditions and the following disclaimer".
- Apache-2.0 §4(a) requires a copy of the licence.

These crates are statically linked into `agentic`, exactly like AWS-LC's BSD part. `age` would add more BSD-3-Clause crates: `x25519-dalek` and `curve25519-dalek`.

The embedded web UI bundle (React and the rest, mostly MIT) has the same duty.

Epic 11 story 6's CycloneDX SBOM names licences but does not reproduce notices. Nothing in the spine or tickets generates a notices file: there is no `cargo-about` or equivalent.

So the owner's "the BSD notice must ship with releases" would be met for AWS-LC and missed for the crates beside it.

**Fix:** extend the AD-3 sentence:

> "Each release also ships a generated notices file reproducing the licence and copyright notices of every Rust crate compiled into the artefacts and every npm package in the embedded UI bundle (for example `cargo-about`, `MIT OR Apache-2.0`, 0.9.3 on crates.io 2026-10-26, pinned by the epic 11 story that adds it); the licence review checks it against the locked graphs."

Mirror the sentence in THIRD_PARTY.md and in the line 819 note for Sofia.

The tool pin belongs to that epic 11 story's Stack row. Widening the review from the owner's AWS-LC wording is an architect wording fix inside AD-3's new "Binds: the release pipeline". Tell the owner it was widened.

### Medium

#### M1: The project runs no "cargo-deny advisories" check

**Where:** AD-32 "The cipher" and the Deferred row: "a clean `cargo audit` and cargo-deny advisories run".

**Reality:**
- `just supply-chain` runs `cargo deny … check … licenses bans sources` on both graphs, then `cargo audit`.
- `deny.toml` has no `[advisories]` section, and its header says "Advisories are cargo-audit's job in the same recipe".
- CI installs `cargo-audit@0.23.0`.
- The recipe says unmaintained warnings do not fail, so informational advisories in age's graph would pass quietly.

**Fix:** write

> "a clean `just supply-chain` on the graph with `age` (cargo-deny licences, bans and sources on both targets, and `cargo audit`), with its informational warnings for age's graph read and recorded"

instead of "cargo-deny advisories run". Do not add an `[advisories]` section in this update.

#### M2: The `LICENSE` that `aws-lc-sys` carries is not the whole notice it points to

**Where:**
- AD-3 "Native-library parts": "AWS-LC's licence notices (the `LICENSE` file `aws-lc-sys` carries), the BSD-3-Clause notice included".
- The same sentence in THIRD_PARTY.md.

**Reality:**
- `aws-lc-sys-0.47.0/LICENSE` has the Jitter Entropy copyright ("Copyright (C) 2017 - 2025, Stephan Mueller"), `SPDX-License-Identifier: BSD-3-Clause` and a generic BSD 3-Clause text. It says "See third_party/jitterentropy/jitterentropy-library/LICENSE", and likewise "See third_party/s2n-bignum/s2n-bignum-imported/LICENSE".
- The crate package holds only `LICENSE`, `aws-lc/LICENSE` (the same text) and `aws-lc/third_party/fiat/LICENSE`. Neither the jitterentropy licence nor the s2n-bignum licence is in it. The jitterentropy sources say only "License: see LICENSE file in root directory".
- Upstream, AWS-LC's `third_party/jitterentropy/jitterentropy-library/` has `LICENSE`, `LICENSE.bsd` and `LICENSE.gplv2`.
- Jitterentropy's own BSD text differs from the generic one. Its condition 1 also requires "the entire permission notice", its condition 3 reads "The name of the author may not be used…", and it carries the GPL alternative.
- BSD condition 2 requires reproducing "this list of conditions", which means jitterentropy's own list.

**Fix:** write

> "AWS-LC's licence notices: the `LICENSE` file `aws-lc-sys` carries and the third-party licence files it refers to for code compiled in (jitterentropy's BSD text, `LICENSE.bsd`; s2n-bignum's `LICENSE`), taken from AWS-LC's source at the linked version when the crate package omits them".

Make the same change in THIRD_PARTY.md.

#### M3: The aws-lc-rs fallback collides with the scope rules that already bind aws-lc-rs

**Where:** the fallback in AD-32 and the Deferred row, against Stack row 508 and AD-32's keyed-hash paragraph.

- Stack row 508 scopes `aws-lc-rs` to "HKDF-SHA-256 and HMAC-SHA-256 for `am-store`'s key module, `crates/am-store/src/keys.rs`".
- AD-32 says `xtask check-deps` fails when "shipped source outside that file names `aws_lc_rs`".
- A backup cipher on `aws-lc-rs`'s AEAD, and its X25519 if the age format is used (H1), would live in a backup module, not `keys.rs`, and would trip the rule.
- The features need no change: `aead`, `agreement`, `hkdf` and `hmac` are ungated modules in 1.19.0, so `bans.features` stays as it is.

**Fix:** add to the fallback sentence:

> "if the fallback is taken, story 9.10 widens Stack row `aws-lc-rs`'s use and the `check-deps` file rule to the backup cipher module in the same story (no new feature)".

### Low

#### L1: Facts the 9.10 plan-stop check will meet on `age` (heads-up only; the binding check stays at 9.10)

**Version and maintenance:**
- 0.12.1 was published 2026-07-14 by str4d: `MIT OR Apache-2.0`, MSRV 1.74, default features empty.
- 0.11.5 came out the same day. Before that, 0.11.3 was published 2026-04-22.
- The last commits on rage `main` are dated 2026-07-14. There are 55 open issues, 21 open PRs and one main maintainer.
- The README's crate line reads "[BETA] A simple, secure, and modern encryption library", and the crate is still pre-1.0. AGENTS.md's crate-notes rule therefore applies: 9.10 writes `docs/crate-notes/age.md`.
- 0.12.0 changed public signatures: plugin constructors now return `Result`, and error enums became non-exhaustive.
- I found no published third-party audit of the Rust crate. An openSUSE "AUDIT-0" packaging review exists.

**Its dependency lines lag the current releases:**

| Dependency | age 0.12.1 requires | Current or in this graph |
|---|---|---|
| `hmac`, `hkdf` | 0.12 | — |
| `sha2` | 0.10 | 0.11.1 is pinned here |
| `chacha20poly1305` | 0.10 | — |
| `rand` | 0.8 | 0.9.5 and 0.10.4 are locked here |
| `x25519-dalek` | ^2 | 3.0.0 (2026-07-06) |
| `hpke` | ^0.12 | 0.15.1 (2026-10-28) |

**What the graph brings with default features:**
- The localisation stack: `i18n-embed` 0.16 (MIT), `i18n-embed-fl` 0.10.1 (MIT, proc-macro), `rust-embed` 8 and fluent. Through `fluent-bundle` it includes `self_cell` `Apache-2.0 OR GPL-2.0-only`, which passes because of the OR. The graph already has two `… OR LGPL-2.1-or-later` crates.
- Post-quantum and P-256 parts: `ml-kem` 0.2, `p256` 0.13, and `hpke` 0.12 (`MIT/Apache-2.0`, slash form).
- BSD-3-Clause `x25519-dalek` and `curve25519-dalek` (see H2).
- `rsa` (the Marvin advisory) only comes in with the `ssh` feature, so it stays out with default features.

**RustSec:**
- `age` has only RUSTSEC-2024-0433 (plugin names, GHSA-4fg7-vxc8-qx5w). It is patched in ≥0.11.1, so 0.12.1 is unaffected, and the `plugin` feature is off by default.
- `rand` has RUSTSEC-2026-0097 (unsound, informational). It is patched for the 0.8 line in ≥0.8.6, so resolution must pick ≥0.8.6.
- `curve25519-dalek` has RUSTSEC-2024-0344, patched in ≥4.1.3 (the newest non-yanked 4.x).
- No RustSec page exists for `hpke` or `ml-kem`.

**Fix (optional):** add to the Deferred row "pre-1.0 and self-described [BETA]; default features only; 9.10 writes `docs/crate-notes/age.md`".

#### L2: "RustCrypto `chacha20poly1305` is no longer a candidate" reads as if it leaves the graph

**Where:** the Deferred row (line 782).

`age` 0.12.1 and `age-core` 0.12.0 both depend on `chacha20poly1305 ^0.10` (`default-features = false`, `alloc`).

**Fix:** "is no longer a direct candidate (`age` brings its 0.10 line transitively)".

#### L3: "Approved in principle" leaves the age recipient type, and so the backup key's stored form and entropy, unnamed

**Where:** AD-32 "The cipher": "The same choice fixes the stored form of the `backup-key:<project-id>` value".

- The age v1 spec now defines X25519, scrypt (passphrase), hybrid MLKEM768-X25519, and tagged P-256 types.
- An X25519 identity's stored form is the Bech32 `AGE-SECRET-KEY-1…` text, which AD-32's HKDF takes as UTF-8 bytes.
- A scrypt passphrase would make the audit-hash subkey only as strong as the passphrase.

**Fix:** add to the architect's check list "the recipient type (a random X25519 identity, never a scrypt passphrase)".

## Claims checked

| Claim (location) | Evidence | Result |
|---|---|---|
| `age` exists, is maintained, and fits as the cipher (AD-32, Deferred) | crates.io API: 0.12.1, 2026-07-14, `MIT OR Apache-2.0`, 8.6 M downloads. rage releases in Apr and Jul 2026. Last commits 2026-07-14 | Holds. Pre-1.0 and "[BETA]" (L1) |
| age is a published streaming format (removed "reviewed" wording; implied by "do not invent a file format") | c2sp.org/age: the payload is a "STREAM" variant citing ePrint 2015/189, with 64 KiB ChaCha20-Poly1305 chunks. CCTV vectors exist. No formal status line on the C2SP page | Holds. No audit found |
| `aws-lc-rs` AEAD offers ChaCha20-Poly1305 and AES-256-GCM (Deferred, AD-32 fallback) | `src/aead.rs:202-203` re-exports `AES_256_GCM` and `CHACHA20_POLY1305`. `lib.rs:250` `pub mod aead;` is ungated | Holds |
| `aws-lc-rs` has a streaming/chunked AEAD | No. `SealingKey<N: NonceSequence>`/`LessSafeKey` seal one message per call. `cipher::StreamingEncryptingKey` is unauthenticated (CBC, CTR, CFB128, ECB) and must not be used for backups. No scrypt (PBKDF2 only) | One-shot only, so the fallback needs a published scheme (H1) |
| STREAM is a published specification (AD-32, Deferred) | ePrint 2015/189, authors as named. It is a construction, not a byte format | Partly (H1) |
| `aws-lc-sys` 0.47.0 `LICENSE` carries the Jitter BSD-3 notice and the CC0 parts (THIRD_PARTY.md, AD-3) | `LICENSE` lines 118-136: "Amazon expressly elects to distribute the package under the 3-Clause BSD License and NOT under GNU General Public License Version 2". "Keccak / AES Reference Implementations: Public domain (CC0) contributions." Full BSD-3 text at lines 283+. No NOTICE file | Holds. Upstream jitterentropy licence file missing from the package (M2) |
| Jitter Entropy is compiled into the macOS binary | `builder/main.rs:1083` ("On Apple targets, jitterentropy requires this header"); `cc_builder.rs:365-373` | Holds |
| No cargo-deny crate-licence exception is needed (AD-3, THIRD_PARTY.md) | `aws-lc-sys` declares `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`. Every identifier is in `deny.toml`'s allow-list, and no CC0 is declared | Holds |
| What BSD-3-Clause requires of a binary release | OSI text, condition 2: reproduce the copyright notice, the conditions and the disclaimer "in the documentation and/or other materials provided with the distribution" | Covered by AD-3 for AWS-LC, with M2. Not for crates (H2) |
| AWS-LC 5.12.0 (THIRD_PARTY.md, "at its linked version") | `aws-lc/include/openssl/base.h:68` | Holds |
| "a clean `cargo audit` and cargo-deny advisories run" | `justfile` `supply-chain`, `deny.toml` header, `ci.yml:41` | Partly (M1) |

## Could not verify

- The C2SP age spec's formal status or version tag. The page shows an editor's copy with no status line. The v1 format id `age-encryption.org/v1` has been stable since 2019.
- How many packages `age` 0.12.1 adds to the locked graph. Resolving it needs network or a registry fetch, which this review did not do.
- Whether the Rust `age` 0.12.1 decrypts the hybrid MLKEM768-X25519 type. Its `ml-kem` dependency suggests partial support. This is not needed for v1.
- The restore behaviour (`agentic backups decrypt` never writes the typed key; the restore wording). These are design statements with no external fact to check.

## Sources (all fetched 2026-11-24)

- crates.io API, `age`: https://crates.io/api/v1/crates/age, https://crates.io/api/v1/crates/age/0.12.1, https://crates.io/api/v1/crates/age/0.12.1/dependencies
- crates.io API, `age-core` 0.12.0 dependencies: https://crates.io/api/v1/crates/age-core/0.12.0/dependencies
- crates.io API, the i18n crates: https://crates.io/api/v1/crates/i18n-embed, https://crates.io/api/v1/crates/i18n-embed/0.16.0/dependencies, https://crates.io/api/v1/crates/i18n-embed-fl, https://crates.io/api/v1/crates/i18n-embed-fl/0.10.1/dependencies
- crates.io API, fluent and its dependencies: https://crates.io/api/v1/crates/fluent-bundle/0.16.0/dependencies, https://crates.io/api/v1/crates/self_cell, https://crates.io/api/v1/crates/proc-macro-error3
- crates.io API, crypto crates: https://crates.io/api/v1/crates/hpke, https://crates.io/api/v1/crates/x25519-dalek, https://crates.io/api/v1/crates/curve25519-dalek
- crates.io API, `cargo-about`: https://crates.io/api/v1/crates/cargo-about
- rage on GitHub: https://github.com/str4d/rage, https://github.com/str4d/rage/releases, https://github.com/str4d/rage/releases/tag/v0.12.0, https://github.com/str4d/rage/releases/tag/v0.12.1, https://github.com/str4d/rage/commits/main
- age README on docs.rs: https://docs.rs/crate/age/0.12.1/source/README.md
- RustSec: https://rustsec.org/packages/age.html, https://rustsec.org/advisories/RUSTSEC-2024-0433.html, https://rustsec.org/packages/rand.html, https://rustsec.org/advisories/RUSTSEC-2026-0097.html, https://rustsec.org/packages/curve25519-dalek.html (the `ml-kem` and `hpke` package pages return 404, so there are no advisories)
- GitHub advisory for the Go age implementation: https://github.com/advisories/GHSA-32gq-x56h-299c
- age v1 spec: https://c2sp.org/age and https://github.com/C2SP/C2SP/blob/main/age.md
- age test vectors: https://github.com/C2SP/CCTV/tree/main/age
- STREAM paper: https://eprint.iacr.org/2015/189
- Tink: https://developers.google.com/tink/streaming-aead, https://developers.google.com/tink/streaming-aead/aes_gcm_hkdf_streaming
- jitterentropy licence: https://raw.githubusercontent.com/smuellerDD/jitterentropy-library/master/LICENSE and https://github.com/aws/aws-lc/tree/main/third_party/jitterentropy/jitterentropy-library
- BSD-3-Clause text: https://opensource.org/license/bsd-3-clause
- openSUSE packaging review of rage: https://lists.opensuse.org/archives/list/bugs@lists.opensuse.org/thread/I6R5LFAVQPZ4GFMBRA3KFVTU64GXYV4S
- Local: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/aws-lc-sys-0.47.0/` and `…/aws-lc-rs-1.19.0/`, `deny.toml`, `justfile`, `.github/workflows/ci.yml`, `cargo metadata --offline --locked`
