---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-22 owner answers (a)-(c) at story 2.12 core plan approval — AD-15 Interim CLI Account edit (*Terminal*, *Change*, *Migrate*, *Record*, *Remove*), AD-17's keychain sentence, AD-13's same-user sentence
date: 2026-11-22
---

# Review: reality check of the 2026-11-22 answers amendment

## Verdict

**Pass with changes.** One technical claim is false for the pinned stack as it is wired today. AD-15 *Change* step (2) and AD-17 say the provider change "replaces the keychain entry ... through `SecretStore`, never reading the old one". The repo's `SecretStore::set` on an existing entry decrypts the old key into the process first, and that read can show a macOS access prompt. The guarantee can be met with the same pinned crates (delete, then add), but the spine must say so. That choice also changes the item's access list, which AD-17's epic 11 check should cover. The pseudo-terminal claim in AD-13 and AD-15 is true, and I checked it on this Mac. However, it contradicts an older clause in *Terminal* that was left unchanged. Its practical effect on an address change is larger than the accepted wording says.

## Method

- `git diff` of the spine and `.memlog.md` in `agentic-manager-replan2` (working tree against HEAD).
- Read AD-13 (the same-user paragraph), all of AD-15's Interim CLI Account edit, all of AD-17, and the Stack rows for `keyring-core` `=1.0.1`, `apple-native-keyring-store` `=1.0.2` (feature `keychain`) and `nix` `=0.31.4` (`term`).
- Repo code (read-only): `crates/am-secrets/src/lib.rs` (`KeychainStore`), `crates/am-domain/src/secrets.rs` (`SecretStore`: `get`, `set`, `delete`), `docs/crate-notes/nix.md`, `docs/crate-notes/rusqlite.md`.
- Pinned crate sources in `~/.cargo/registry/src/`: `keyring-core-1.0.1/src/{lib,api}.rs`, `apple-native-keyring-store-1.0.2/src/keychain.rs`, `security-framework-3.7.0/src/os/macos/passwords.rs` and `src/item.rs` (the version in the lock graph), and `nix-0.31.4/src/lib.rs`.
- Apple documentation (fetched 2026-11-22): `SecKeychainFindGenericPassword`, `SecKeychainItemModifyAttributesAndData`, `SecItemDelete`.
- On this Mac, read-only with no keychain writes: ran a `script(1)` pseudo-terminal from a non-TTY parent.

## Call chain verified for "replace the entry"

`KeychainStore::set` → `keyring_core::Entry::set_password` → `CredentialApi::set_password` (default: `set_secret(password.as_bytes())`) → `apple_native_keyring_store::keychain::Cred::set_secret` → `security_framework::os::macos::keychain::SecKeychain::set_generic_password`:

```rust
match self.find_generic_password(service, account) {
    Ok((_, mut item)) => item.set_password(password),   // SecKeychainItemModifyAttributesAndData
    _ => self.add_generic_password(service, account, password), // SecKeychainAddGenericPassword, initialAccess NULL
}
```

`find_generic_password` calls `SecKeychainFindGenericPassword` with non-NULL `passwordLength` and `passwordData` out-pointers. Apple's page for that function says it "decrypts the password before returning it to you". If the calling application is not in the trusted list, the user is prompted. If the access controls do not allow decryption, the call returns `errSecAuthFailed`. The bytes are then freed by `SecKeychainItemFreeContent` on drop, and nothing sends them anywhere.

`Cred::delete_credential` → `ItemSearchOptions::delete` → `SecItemDelete`. Its query carries no return keys, and Apple says the delete query cannot contain them because it "only returns a status". The delete does not decrypt the data.

`Cred::get_credential`, the crate's existence probe, also calls `find_generic_password` with the data pointers, so it decrypts too.

## Findings

### High

**H1. "never reading the old one" is false for `SecretStore::set` on an existing entry (AD-15 *Change* step (2); AD-17 "which never reads the old key").**
Today's `KeychainStore::set` decrypts the stored key into process memory whenever an entry exists. On an entry that another binary created, the read can raise the macOS "wants to use your confidential information" dialog. If the owner clicks Deny, `set` fails with `errSecAuthFailed`. That failure lands after step (1), so the Account is left refused. The key is not sent to the new provider, so the "never sent" half of the owner's answer (b) holds. The "never read" half that the spine commits to does not. A port-level test on `FakeSecretStore` (asserting no `get`) would pass while the real adapter reads. The behaviour sits below the port.

The pinned API can meet the claim. `SecretStore::delete`, followed by `SecretStore::set` on the now-absent entry, decrypts nothing. Its find returns `errSecItemNotFound` and `SecKeychainAddGenericPassword` runs. The keychain is never in a mixed state that matters, because the delete happens after step (1) has already made the Account refused.

*Proposed fix (AD-15 *Change*, step (2)):* "(2) deletes the keychain entry (service `agentic-manager`, account `<id>`, AD-17's naming unchanged) through `SecretStore::delete`, a missing entry counting as deleted, and then stores the new key with `SecretStore::set` on the now-absent entry. It never calls `get` and never updates an existing item in place, because with the pinned `apple-native-keyring-store` an in-place set first decrypts the old key (`SecKeychainFindGenericPassword` with password data). This is the one write that replaces an existing entry."

*Proposed fix (AD-17):* "the entry is replaced only by AD-15 *Change*'s provider change or first confirmation, which deletes it and stores a new one and never reads or updates the old key in place (amended 2026-11-22, owner)".

*Proposed fix (Stack or `docs/crate-notes/`):* add `docs/crate-notes/apple-native-keyring-store.md` for `=1.0.2` (on security-framework 3.7.0). It records three facts. `set_secret` on an existing item and `get_credential` both decrypt the item and may prompt. `delete_credential` (`SecItemDelete`) does not decrypt. A new item gets the creating binary's default access list. The story's test should assert the order on `FakeSecretStore`: `delete` then `set`, with no `get`. A code-review check should confirm that no `get` call or existence probe sits on this path.

### Medium

**M1. Delete and re-add resets the item's access list, and the delete itself is unverified on an entry another binary created (AD-17's epic 11 note).**
`SecKeychainAddGenericPassword` with `initialAccess` NULL gives the new item a default access list that trusts only the creating binary, the `agentic` CLI at that build. An in-place modify would have kept the old access list, but it decrypts (H1), so delete and add is the only route that meets the claim. Every provider change or first confirmation therefore drops any "Always Allow" the owner gave the desktop app or an earlier CLI build. The next read from the desktop app will prompt again.

I also could not confirm, from Apple's docs or the crate, whether `SecItemDelete` prompts or fails when the legacy item was created by a different binary. Examples are the desktop app, or an earlier unsigned CLI build whose code hash differs. I did not test it, because that would write to the owner's login keychain.

*Proposed fix (AD-17 parenthesis):* "...whether the desktop app reads without a prompt an entry the `agentic` CLI stored, given the keychain item's access list, whether AD-15 *Change*'s delete of an entry another binary created prompts or fails, and that the entry it re-creates trusts only the CLI (so an earlier Always Allow is gone), is verified on the signed build in epic 11". In *Change*, add: "a delete or store the keychain refuses (a denied prompt, a locked keychain in a non-GUI session) is a failure after (1), with the Account refused and the refusal worded".

**M2. *Terminal* still says "so no agent, script or child process can add or change an Account". Today's sentence contradicts that, and the consequence for an address change is not stated.**
I verified (b) on this Mac. From this agent's non-TTY shell (stdin `/dev/null`, stdout a pipe), `script -q /dev/null sh -c '[ -t 0 ] && [ -t 1 ]'` reports both as TTYs (`/dev/ttys000`). A plain pipe fails the same test. macOS ships `/usr/bin/script` and `/usr/bin/expect`, and Python's stdlib has `pty`. The repo's own `crates/am-app/tests/accounts_tty.rs` drives the real binary through `nix::pty::openpty`. `termios` echo-off also works on a pty slave, so the hidden prompt gives no extra protection.

So the old clause is now false, and its own next sentence contradicts it. There is also a larger consequence. An **address change within one provider** asks only for the typed id and `yes`, and neither is secret. A same-user program, such as a coding agent the owner runs or an `expect` script, can therefore point a stored key at a new host of the same provider without knowing any secret. That sends the key off the confirmed destination. A provider change needs a new key, so it does not leak the old one, but it can still swap in an attacker's key. AD-13's accepted wording ("passes that check") is correct, but it does not name this result, so the owner's acceptance in (c) does not say it covers it.

*Proposed fix (AD-15 *Terminal*):* "...any other stdin (a pipe, a redirected file, one of the app's children) is refused in words before anything is read or written, so an agent, script or child process cannot add or change an Account through a pipe or by inheriting the app's stdio; the typed name and the key are read from that terminal. A same-user process that drives a pseudo-terminal (`script`, `expect`, `openpty`) passes this check. This is accepted under AD-13's same-user rule (owner, 2026-11-22), and it includes an address change within one provider, which needs no secret and so lets such a process send a stored key to another host of the same provider."

*Proposed fix (AD-13):* add after "passes that check": "including an address change within one provider, which needs only the id and `yes`, so such a process can send a stored key to another endpoint of that provider". If the owner did not mean to accept that, it is a REPLAN item for the owner, not a wording fix.

### Low

**L1. `migrate` "never reads ... a key" holds only if `migrate` never probes the keychain at all.**
With the pinned crate, the only existence check (`get_credential`, or `get`) decrypts the item (see the call chain). In the carried-over case, `migrate`'s sentence says "the key stored under the id will be used for that destination", which invites an existence check. *Proposed fix (Migrate):* "`migrate` makes no keychain call (no read, no existence check); its sentence says the key stored under the id, if one exists, will be used for that destination". The same fact affects *Add*'s existing "an existing keychain entry for that id is reused ... the sentence says so", which needs a decrypting probe today. That text predates this change and is outside today's scope, but the crate note from H1 should state it.

**L2. First confirmation with no entry under the id.**
This is consistent. `KeychainStore::delete` maps `keyring_core::Error::NoEntry` to `Ok(())`, so the delete-then-set route of H1 works for a migrated Account that has no entry. No change is needed. Wording H1's fix as "a missing entry counting as deleted" makes the behaviour explicit.

**L3. A locked keychain or a non-GUI session.**
Over SSH the login keychain can be locked and no unlock dialog can appear, so step (2) fails after step (1). The spine's "a failure or crash after (1) leaves the Account ... refused" already covers this. The M1 fix wording names it so the refusal message is specific. No other change.

## Claims checked and found true

- **(b) Pseudo-terminal passes an isatty check on stdin and stdout.** True. I verified it on this Mac (M2). AD-13's sentence is accurate.
- **Address change "never reads, replaces or deletes the keychain entry".** True as specified: the path calls no `SecretStore` method.
- **"The old key deleted from the keychain and not kept" / "no Undo".** True for delete and re-add. The app keeps no copy.
- **`migrate` reads the project store "read-only".** Consistent with `docs/crate-notes/rusqlite.md`: readers already open with `SQLITE_OPEN_READ_ONLY` on WAL databases, and the 0700 directory and `-shm` mode rules apply.
- **Hidden prompt via `nix` termios on a pty.** True: the repo's `accounts_tty.rs` reads `ECHO` through `tcgetattr` on an `openpty` slave. `nix::pty` is gated on the already-allowed `term` feature (`nix-0.31.4/src/lib.rs`), so no Stack change is needed.
- **Stack pins.** `keyring-core =1.0.1` and `apple-native-keyring-store =1.0.2` (`keychain`) match `Cargo.toml:115,129`. Both are post-1.0, so the pre-1.0 crate-note rule does not require a note. H1 recommends one anyway, because the behaviour is load-bearing.

## Not verified (flag)

- Whether `SecItemDelete` prompts or fails on a legacy-keychain generic password created by another binary (M1). This needs a test on a Mac with two differently signed binaries, in epic 11 or in story 2.14's manual check.
- Whether the desktop app's first read after a CLI re-create prompts once and then honours "Always Allow". This is already AD-17's epic 11 item, which M1 widens.
