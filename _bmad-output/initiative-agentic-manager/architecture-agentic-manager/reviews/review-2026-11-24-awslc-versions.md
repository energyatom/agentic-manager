---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-24 owner crate answer for keyed audit hashes (AD-32 names aws-lc-rs for HKDF-SHA-256 and HMAC-SHA-256; the new Stack row for aws-lc-rs 1.19.0; the AD-3 correction "v1 links two"; the Open Questions "Resolved" line; THIRD_PARTY.md's new "Native libraries compiled into the binary" section)
date: 2026-11-24
---

# Review: aws-lc-rs as the keyed-hash provider, versions and facts

## Verdict

**Pass with changes: no critical, no high, 2 medium, 4 low, 2 nits.**

The core claims hold in reality:

- `aws-lc-rs` 1.19.0 is compiled into the `agentic` binary today, on macOS and on the Linux CI target.
- Its enabled features are exactly `aws-lc-sys` and `prebuilt-nasm`, on every target.
- A direct `am-store` dependency with `default-features = false` and those two features adds no package and no feature to the build.
- `hkdf` and `hmac` are public, ungated modules. The API expresses the pinned derivation exactly: no salt, info `audit-hash-v1`, 32-byte output, then HMAC-SHA-256.
- Both crates are the newest on crates.io. No advisory is open against them, nor against the bundled AWS-LC 5.12.0.
- The licence expressions match THIRD_PARTY.md word for word and fit `deny.toml`'s allow-list. `deny.toml` needs no change.

What needs changing:

- **M1:** "built from source and statically linked" (THIRD_PARTY.md, AD-3) depends on the build environment. `aws-lc-sys` 0.47.0 auto-detects a system AWS-LC and links it instead, unless `AWS_LC_SYS_USE_SYSTEM=0` is set. Nothing in the repo sets it.
- **M2:** the Stack row says "`Cargo.lock` gains no package and no feature" and that story 9.10 checks this "with a `Cargo.lock` diff". `Cargo.lock` records no features, and the diff is not empty: it gains exactly one line.

## Method

- **Diff:** `git -C W diff` of the spine, `.memlog.md` and `THIRD_PARTY.md`. Changed spine lines: 10, 121 (AD-3), 232–234 (AD-17), 282 (AD-25), 322–324 (AD-32), 330 (AD-33), 508 (Stack row), 783 (Deferred), 816–818 (Open Questions). Read the last memlog entries: the crate decision, the AD-3 correction, and the "resolved" event.
- **cargo, read-only (`--offline --locked` throughout; no build, check or test):**
  - `cargo tree -p am-app -e features -i aws-lc-rs`, for `--target aarch64-apple-darwin` and for `--target x86_64-unknown-linux-gnu`.
  - The same with `--workspace`, and with `--target all`.
  - `cargo tree --workspace -e features -i aws-lc-sys` on both targets.
  - `cargo tree --workspace --target all -i ring`, with and without `--all-features`.
  - `cargo tree -e features -i untrusted`.
  - `cargo tree -e normal --prefix none` on both targets, filtered for `-sys` crates.
  - `cargo tree -e build,normal -i cc` and `-i cmake` on macOS.
- **Registry sources** (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`):
  - `aws-lc-rs-1.19.0`: `Cargo.toml`, `src/lib.rs`, `src/hkdf.rs`, `src/hmac.rs`.
  - `aws-lc-sys-0.47.0`: `Cargo.toml`, `LICENSE`, `README.md`, `aws-lc/include/openssl/base.h`, `builder/main.rs`, `builder/system_detect.rs`, `builder/system_library.rs`, `builder/cc_builder/universal.rs`.
  - `rustls-0.23.46` and `rustls-webpki-0.103.17` `Cargo.toml` (their feature maps).
  - `objc2-exception-helper-0.1.1` `Cargo.toml`.
  - `libsqlite3-sys-0.40.0/sqlite3/sqlite3.h`.
- **Repo:** workspace `Cargo.toml`, `Cargo.lock`, `deny.toml`, `.cargo/config.toml`, `clippy.toml`, `rust-toolchain.toml`, `crates/am-store/Cargo.toml`, `crates/am-net/Cargo.toml`, `xtask/src/deps.rs`, `docs/crate-notes/` (listing only).
- **Web, 2026-11-24:**
  - crates.io API `crates/aws-lc-rs/versions` and `crates/aws-lc-sys/versions`.
  - OSV `api.osv.dev/v1/query`, ecosystem `crates.io`: by package for `aws-lc-rs` and `aws-lc-sys`, and by exact version for 1.19.0 and 0.47.0.
  - GitHub security advisories of `aws/aws-lc` and `aws/aws-lc-rs`, through `gh api`.

## What checks out

| Claim | Evidence | Result |
|---|---|---|
| aws-lc-rs 1.19.0 is compiled in through rustls (Open Questions line 816; memlog) | `cargo tree -p am-app -e features -i aws-lc-rs`, macOS. The chain is rustls 0.23.46 ← hyper-rustls 0.27.10 feature `aws-lc-rs` ← reqwest 0.13.5 features `__rustls-aws-lc-rs` and `rustls` ← am-net. rustls-webpki 0.103.17 is a second user | True on macOS, on Linux and with `--target all` |
| Enabled features are `aws-lc-sys` and `prebuilt-nasm` only (Stack row) | The same tree, on `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`, for `-p am-app`, for `--workspace` (am-desktop included) and for `--target all`, lists exactly these two. The enablers are rustls's `aws_lc_rs` feature (`dep:aws-lc-rs`, `webpki/aws-lc-rs`, `aws-lc-rs/aws-lc-sys`, `aws-lc-rs/prebuilt-nasm`) and rustls-webpki's `aws-lc-rs` feature (the same two) | Exact |
| The direct dependency as worded adds no feature | aws-lc-rs 1.19.0 feature map: `prebuilt-nasm = ["aws-lc-sys?/prebuilt-nasm"]`; `aws-lc-sys` is an optional dependency with `default-features = false`. `aws-lc-sys` 0.47.0 already has only `prebuilt-nasm` on both targets. `zeroize` is unconditional and already compiled. So the requested set equals the current set, and an `am-store`-only build (`-p am-store`) resolves the same feature set as the workspace, sharing its artifact | No new feature |
| The direct dependency as worded adds no package | aws-lc-rs 1.19.0 and aws-lc-sys 0.47.0 are already in `Cargo.lock`. rustls asks for `aws-lc-rs = "1.18"` (`default-features = false`), so an exact `=1.19.0` resolves to the locked version | No new package (but see M2 on what the lock diff shows) |
| The defaults would add something (memlog) | Defaults are `aws-lc-sys`, `alloc`, `ring-io`, `ring-sig-verify`. `ring-io` and `ring-sig-verify` enable `dep:untrusted` at `^0.7.1`, but `Cargo.lock` holds only `untrusted` 0.9.0, through rustls-webpki. With defaults on, the lock would gain a package (`untrusted` 0.7.x, a duplicate version that `deny.toml`'s `multiple-versions = "warn"` reports) and three features | `default-features = false` is required, as the row says |
| Nothing is lost by dropping `alloc` | `grep 'feature = "alloc"'` over `aws-lc-rs-1.19.0/src` finds nothing: `alloc` is an empty feature that no code reads | True |
| `hkdf` and `hmac` are public and not feature-gated (AD-32, memlog) | `src/lib.rs:256-257` `pub mod hkdf; pub mod hmac;` carry no `cfg`. The only gated public module is `io` (`ring-io`, line 258) | True |
| The API expresses AD-32's derivation | `hkdf::Salt::new(HKDF_SHA256, &[])` and `hkdf::Salt::none(HKDF_SHA256)` both exist (`hkdf.rs:129,165`). `none` is documented as "a string of HashLen zeros" per RFC 5869 §2.2. `Salt::extract(&self, secret: &[u8]) -> Prk` (line 177) takes the backup key's UTF-8 bytes. `Prk::expand(&[&[u8]], L: KeyType)` (line 389) takes `&[b"audit-hash-v1"]`. `HKDF_SHA256`'s `KeyType::len` is the digest length, 32 (line 89), and so is `hmac::HMAC_SHA256`'s (`tag_len`, `hmac.rs:338`). `impl From<hkdf::Okm<'_, hmac::Algorithm>> for hmac::Key` (line 345) turns the 32-byte output straight into an HMAC key. `hmac::sign(&Key, &[u8]) -> Tag` (line 491) has `Tag: AsRef<[u8]>` | Fully expressible |
| HKDF with an empty salt works in AWS-LC | `PrkMode::fill` passes `salt.as_ptr(), salt.len()` (0) to AWS-LC's one-shot `HKDF()` (`hkdf.rs:245-256`). HMAC with an empty key equals HMAC with HashLen zero bytes (RFC 2104 pads to the block length), so `new(.., &[])` and `none` give identical output. Only FIPS mode rejects an empty salt, and FIPS is not enabled | True |
| AWS-LC 5.12.0 is what aws-lc-sys 0.47.0 bundles (THIRD_PARTY) | `aws-lc-sys-0.47.0/aws-lc/include/openssl/base.h:68` `#define AWSLC_VERSION_NUMBER_STRING "5.12.0"` | True (for a source build; see M1) |
| Licences (Stack row, THIRD_PARTY) | aws-lc-rs 1.19.0 `license = "ISC AND (Apache-2.0 OR ISC)"`. aws-lc-sys 0.47.0 `license = "ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)"`. crates.io reports the same strings | Match THIRD_PARTY.md character for character |
| Newest versions | crates.io, 2026-11-24: aws-lc-rs 1.19.0 (2026-10-28) and aws-lc-sys 0.47.0 (2026-10-28) are the newest. Previous releases: 1.18.2 (2026-10-12) and 0.46.0 | Current |
| No open advisory | OSV by exact version: `{}` for aws-lc-rs 1.19.0 and for aws-lc-sys 0.47.0. OSV by package: aws-lc-rs has none; aws-lc-sys has RUSTSEC-2026-0042..0048, all fixed by 0.38.0 or 0.39.0. GitHub: GHSA-xv35-xp83-5wrf (aws-lc-rs) and GHSA-h63c-583w-4qfw (AWS-LC), published 2026-11-23, an out-of-bounds write in multi-call padded-block-cipher decryption. They affect aws-lc-rs below 1.17.4 and 1.18.0, aws-lc-sys below 0.45.0, and AWS-LC below 5.7.0. These are not yet in OSV for crates.io | Nothing open against the pinned versions or the bundled AWS-LC 5.12.0 |
| AD-3 "v1 links two" (SQLite and AWS-LC) for the `agentic` binary | On macOS, `cc` is a build-dependency only of `aws-lc-sys`, `libsqlite3-sys` and `objc2-exception-helper`, and the last comes only through `am-desktop` (Tauri), not `am-app`. `core-foundation-sys` and `security-framework-sys` bind system frameworks and compile nothing. No `openssl-sys` is in `Cargo.lock`, so there is no second libcrypto to clash with | True for the `agentic` binary today (see N2 for E11) |
| ring "not compiled on macOS" (line 816) | `cargo tree --workspace --target all -i ring`, with and without `--all-features`, prints nothing | True, and stronger: compiled on no target and under no feature |
| `deny.toml` needs nothing | Every SPDX term in both expressions (ISC, Apache-2.0, MIT, BSD-3-Clause, MIT-0) is on `[licenses].allow`. Both crates declare an SPDX `license` field, so no `[[licenses.clarify]]` is needed; `confidence-threshold` applies only to licence-file text scanning. aws-lc-sys is already in both graphs `just supply-chain` checks (macOS whole workspace; Linux without am-desktop), and it passes today. A new `am-store → aws-lc-rs` edge changes neither graph | No change |
| AD-5 (thin ports for pre-1.0 or volatile crates) | aws-lc-rs is 1.19.0, semver-stable since 1.0, with a ring-compatible API. AD-32 and AD-17 confine it to one private `am-store` key module, so no aws-lc-rs type reaches `am-domain` or a port signature | No port and no crate note required (see L4) |
| Dependency rules allow it | `xtask/src/deps.rs` has external allow-lists only for `am-domain` (`DOMAIN_EXTERNALS`) and `am-glob` (`GLOB_EXTERNALS`), and `clippy.toml` bans nothing in aws-lc-rs. The API is safe Rust, so `unsafe_code = "forbid"` holds | `check-deps` and clippy pass |
| Linux CI | aws-lc-sys is already built for `x86_64-unknown-linux-gnu` through am-net. The `cc` builder carries `linux_x86_64.rs` and `apple_aarch64.rs` source lists, and `prebuilt-nasm` matters only on Windows | No new toolchain need; an isolated `am-store` build now compiles AWS-LC (slower, same artifact) |

## Findings

### Critical

None.

### High

None.

### Medium

#### M1: "built from source and statically linked" depends on the build environment; aws-lc-sys 0.47.0 auto-adopts a system AWS-LC

**Claims:**

- THIRD_PARTY.md: AWS-LC 5.12.0 is "built from source and statically linked by the `aws-lc-sys` 0.47.0 crate".
- AD-3 (line 121): "v1 links two, both compiled from source into the binary".

**Evidence:**

- `aws-lc-sys-0.47.0/builder/main.rs:737-760` (`get_builder`): when `AWS_LC_SYS_SYSTEM_DIR` is unset and `AWS_LC_SYS_USE_SYSTEM` is not `0`, the build script runs `system_detect::detect_system_awslc`. If that finds a usable install, the build links it instead of compiling the bundled sources.
- Discovery (`system_detect.rs`; README "Discovery order") checks, in order: `OPENSSL_DIR`; `OPENSSL_INCLUDE_DIR` / `OPENSSL_LIB_DIR` (and their target-suffixed forms); then pkg-config modules `openssl`, `aws-lc`, `libcrypto` and `libcrypto-awslc`.
- A candidate is adopted when it:
  - carries `OPENSSL_IS_AWSLC`,
  - is at least version 4.2.0 (`system_library.rs:477`), and
  - ships Rust bindings (`<prefix>/share/rust/aws_lc_bindings.rs`, written by an AWS-LC CMake install with `-DGENERATE_RUST_BINDINGS=ON`, or `AWS_LC_SYS_SYSTEM_BINDINGS`).
- The library form defaults to static but falls back to the dynamic library when only that is on disk (`system_library.rs:696-710`).
- `.cargo/config.toml` holds only the `xtask` alias. No file in the repo sets `AWS_LC_SYS_USE_SYSTEM`.

**Why it matters:**

- On a build machine or CI image that provides AWS-LC this way (a Nix shell, a distro package with bindings, an `OPENSSL_DIR` meant for another project), the release binary would:
  - link a different AWS-LC version, possibly a dylib from outside the app bundle, and
  - break THIRD_PARTY.md's version claim and AD-3's "compiled from source".
- A dylib from outside the bundle also fails the hardened runtime's library validation in a signed build. AD-13 forbids `disable-library-validation`.
- The 4.2.0 floor accepts AWS-LC 5.0–5.6.x, which the 2026-11-23 advisory GHSA-h63c-583w-4qfw marks vulnerable (below 5.7.0).
- This already applies to rustls's TLS today. The update now writes it down as a fact, and from story 9.10 it covers the audit-hash key too.
- On this Mac nothing is auto-detected: Homebrew has `openssl` and `openssl@4` but no AWS-LC, `pkg-config` is not installed, and `OPENSSL_DIR` is unset. Today's builds are therefore source builds.

**Fix wording:**

- **AD-3:** after "both compiled from source into the binary", add: "(`aws-lc-sys` is kept on its bundled source by `AWS_LC_SYS_USE_SYSTEM = "0"` in the repo's `.cargo/config.toml` `[env]`, so a build never adopts a system AWS-LC it auto-detects)".
- **THIRD_PARTY.md:** after "built from source and statically linked by the `aws-lc-sys` 0.47.0 crate", add: "(system auto-detection is off: `AWS_LC_SYS_USE_SYSTEM = "0"` in `.cargo/config.toml`)".
- **For Sofia:** the next sweep (3.12, already carrying owner condition 6), or story 9.10 if Sofia prefers, adds that `[env]` line. E11's release story checks with `otool -L` that the shipped `agentic` binary loads no `libcrypto` or `libssl`.
- Optionally also set `AWS_LC_SYS_STATIC = "1"`, so that a system library given by mistake through `AWS_LC_SYS_SYSTEM_DIR` fails instead of linking dynamically.

#### M2: the Stack row's `Cargo.lock` check is mis-stated: the lock records no features, and its diff is one line, not empty

**Claim (line 508):** "so `Cargo.lock` gains no package and no feature, which the story checks with a `Cargo.lock` diff and `cargo tree -e features`".

**Evidence:**

- `Cargo.lock` lists packages and their dependency edges, never features.
- `am-store`'s lock entry (`Cargo.lock:260-273`) lists its dependencies by name. Adding the dependency inserts exactly one line, `"aws-lc-rs",`, into that list, after `"am-store"`.

**Why it matters:** a builder who reads "gains no package and no feature" as "the lock diff is empty" will think the check failed. A reviewer who reads it loosely may accept a diff that adds `untrusted` 0.7.x (what the defaults would do).

**Fix wording:** replace that clause with: "so the build gains no package and no feature: the story checks that the `Cargo.lock` diff is exactly one added line (`"aws-lc-rs"` in `am-store`'s dependency list) and that `cargo tree --offline --locked -e features -i aws-lc-rs` on `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu` still lists only `aws-lc-sys` and `prebuilt-nasm`".

### Low

#### L1: say where the pin and `default-features = false` go

**Evidence:** the workspace pins every external crate exactly in `[workspace.dependencies]`, with `default-features = false` written there. Precedents are `reqwest`, `croner`, `globset`, `rmcp` and `nix`, and members inherit with `.workspace = true` (for example `crates/am-net/Cargo.toml`). Cargo ignores a member-only `default-features = false` on an inherited dependency unless the workspace entry also sets it: it warns, and edition 2024 (this workspace's edition) rejects it. A builder who writes the row's wording into `crates/am-store/Cargo.toml` alone would pull the defaults back in, the M2 failure.

**Fix wording (Stack row):** change "a direct dependency of `am-store` added by epic 9 story 10 with `default-features = false` and only the features rustls enables (`aws-lc-sys`, `prebuilt-nasm`)" to "pinned `=1.19.0` in `[workspace.dependencies]` with `default-features = false` and only the features rustls enables (`aws-lc-sys`, `prebuilt-nasm`), and inherited by `am-store` alone (`aws-lc-rs.workspace = true`), added by epic 9 story 10".

#### L2: the exact pin now also holds rustls's aws-lc-rs; name the bump path

**Evidence:**

- rustls asks for `aws-lc-rs ^1.18`. The graph can hold only one 1.x version, so `=1.19.0` in `am-store` fixes it for TLS too.
- aws-lc-rs 1.19.0 asks for `aws-lc-sys ^0.47.0`, so a C-library fix in aws-lc-sys 0.48 can arrive only with a new aws-lc-rs.
- Before this change, a RustSec fix could be taken with `cargo update -p aws-lc-rs`. After it, the fix needs a Stack amendment, as `url` 2.5.9 did for RUSTSEC-2026-0592.
- An `aws-lc-sys` 0.47.x patch can still float in the lock, which would leave THIRD_PARTY.md's "0.47.0" and "5.12.0" stale.

**Fix wording (Stack row, end):** "; the pin always equals `Cargo.lock`'s aws-lc-rs, so a RustSec fix in aws-lc-rs or aws-lc-sys moves this row and `THIRD_PARTY.md`'s AWS-LC and aws-lc-sys versions together".

#### L3: THIRD_PARTY.md understates what the AWS-LC `LICENSE` covers, including the one GPL mention a licence reviewer will hit

**Evidence:**

- `aws-lc-sys-0.47.0/LICENSE` lists, besides AWS-LC, BoringSSL and OpenSSL:
  - mlkem-native and mldsa-native (Apache-2.0 OR ISC OR MIT),
  - Fiat Cryptography (MIT),
  - s2n-bignum (Apache-2.0 OR ISC OR MIT-0),
  - Jitter Entropy, googletest (BSD-3-Clause) and Wycheproof test vectors.
- Jitter Entropy is "dual-licensed under a BSD-style license and the GNU General Public License Version 2", and "Amazon expressly elects" the BSD-3-Clause licence.
- Jitter Entropy is compiled in: `builder/cc_builder/universal.rs:256-260` lists five `jitterentropy-*.c` sources.
- AD-3 says GPL fails CI, so a reader of the `LICENSE` file needs the election stated.

**Fix wording (THIRD_PARTY.md):** replace "its `LICENSE` file carries the AWS-LC, BoringSSL and OpenSSL notices" with "its `LICENSE` file carries the AWS-LC, BoringSSL and OpenSSL notices and those of the third-party code compiled with it (Fiat Cryptography, MIT; s2n-bignum; mlkem-native and mldsa-native; the Jitter Entropy RNG, which is dual-licensed BSD-3-Clause or GPL-2.0 and taken under BSD-3-Clause only, by Amazon's stated election)".

#### L4: an optional crate note would pin the exact calls (AD-5 does not require one)

**Evidence:** aws-lc-rs is post-1.0, so AD-5 and AGENTS.md require no `docs/crate-notes/aws-lc-rs.md`. The repo does keep notes for some post-1.0 crates (`notify` 8.2.2). The calls an agent most often gets wrong here:

- `Salt::none` versus `Salt::new(.., &[])`.
- `Salt::extract` returns a `Prk` without computing anything. The one-shot `HKDF()` runs at `fill` or `into`, where `From<Okm> for hmac::Key` unwraps.
- Expanding straight into `hmac::Key` with `hmac::HMAC_SHA256` as the length. This keeps the subkey bytes out of `am-store` code.
- `hmac::Key`'s `Debug` prints only the algorithm, but `Key::construct` leaves an un-zeroed 32-byte stack copy.

**Fix wording (for Sofia, story 9.10):** "write `docs/crate-notes/aws-lc-rs.md` for 1.19.0 with the derivation as code (`hkdf::Salt::none(hkdf::HKDF_SHA256).extract(key_utf8).expand(&[b"audit-hash-v1"], hmac::HMAC_SHA256)` into `hmac::Key`; `hmac::sign`), the no-features note, and that `Prk` and `Key` stay private to the key module". This is optional; keep it if 9.10's plan wants the API pinned.

### Nits

#### N1: THIRD_PARTY.md gives AWS-LC's version but not SQLite's

`libsqlite3-sys` 0.40.0 bundles SQLite 3.54.1 (`sqlite3/sqlite3.h:149`). For symmetry: "SQLite 3.54.1: compiled from source into the binary by `rusqlite` 0.43.0's `bundled` feature (`libsqlite3-sys` 0.40.0)".

#### N2: "v1 links two" is true for the `agentic` binary; E11's desktop shell adds a small native shim

`objc2-exception-helper` 0.1.1 (`Zlib OR Apache-2.0 OR MIT`) compiles its own small Objective-C helper with `cc`. It arrives only through `am-desktop` (tauri → tao, wry, muda → objc2). It is the crate's own code, not a third-party library, so it arguably does not count under AD-3. E11 should still recheck AD-3's count and THIRD_PARTY.md when the Tauri shell is built. No change is needed now.

## Not findings (checked and fine)

- **`non-fips` instead of `aws-lc-sys`.** `non-fips` is an alias that enables `aws-lc-sys`, but it would add a feature name to the set. The row's choice of `aws-lc-sys` is the exact one.
- **Symbol clash.** aws-lc-sys builds with the `aws_lc_0_47_0` symbol prefix (`links = "aws_lc_0_47_0"`), and no OpenSSL is linked.
- **Toolchain.** aws-lc-rs's `rust-version` is 1.71.0, and `rust-toolchain.toml` pins 1.100.0.
- **The 2026-11-23 advisory and AD-32's use.** The advisory concerns streaming padded-block decryption, which neither HKDF nor HMAC uses. In any case it does not affect the pinned versions.
