# Review 2026-11-22: AD-15 *Change* and *Migrate* amendment, versions and reality lens

Scope: AD-15 Rule (spine line 222), the `*Change*` and `*Migrate*` paragraphs and the `*Terminal*`, `*Open*` and `*Add*` text they lean on, the last Open Questions bullet, and the Stack table. Lens: does every committed decision that names a technology, crate, OS facility or behaviour hold up, and does the amendment bring in any technology the Stack does not pin? The spine was not edited.

## Verdict

Mostly sound. The amendment adds no new crate or feature. The hidden prompt, the TTY predicate and the time limit are already built and checked in code. Two committed behaviours are stated more strongly than reality supports: the backup location, and the claim that the TTY check stops automation. Neither needs a new technology. Both need a sentence each.

## What was checked and holds

- **`nix` 0.31.4, feature `term` only.** The Stack pins it, the workspace `Cargo.toml` has `=0.31.4`, `default-features = false, features = ["term"]`, and `Cargo.lock` has 0.31.4 pulled in through process-wrap 10.0.1. `nix::sys::termios` sits behind `term`. The hidden prompt that *Change* and `migrate --as` fall back on is `crates/am-app/src/terminal.rs`. It clears ECHO, ICANON and ISIG, restores the saved settings through a drop guard with TCSAFLUSH and a TCSANOW fallback, and ends the read on signals through `tokio::signal`. It needs no `signal` or `poll` feature. `docs/crate-notes/nix.md` records all of this. Nothing in the amendment needs another nix feature.
- **The 2-minute limit on the typed id and the `yes`.** The limit is a helper thread with `recv_timeout` (`terminal.rs`), not `poll`, so it is consistent with the `term`-only pin.
- **Interactive-TTY detection.** The predicate is `std::io::IsTerminal` on stdin and stdout (std, stable since 1.70). No crate is involved.
- **The hash for the backup check.** `sha2` 0.11.1 is pinned. The spine's own precedent for a checked copy is sha256 (the project-move rule near line 324). The amendment names no algorithm, so it adds nothing new.
- **`AppDataLayout`, `user.lock` (`flock`) and `ConfigMigration`.** All three are defined elsewhere in the spine (AD-6, AD-7, AD-16, AD-17). The lock order (project store lock, then `user.lock`) does not conflict with any other holder named in the spine: `add`, `change` and `remove` take `user.lock` alone and read projects without a lock.
- **Keychain.** `change` never reads the keychain, and `--as` stores a key under a new id through `SecretStore`. Neither needs anything beyond keyring-core and apple-native-keyring-store.

## Findings

### M1 (medium): the *Migrate* backup goes into a folder that another rule prunes

*Migrate* writes `<app data dir>/backups/pre-migration/<project-id>/`. AD-7 (line 149) already uses `<app data dir>/backups/pre-migration/` for the user store's `VACUUM INTO` schema backups, under the rule that "only the three newest are kept, older ones deleted once a migration commits". A pruner written to that rule, keeping the three newest entries in the folder, would count each `<project-id>/` subfolder as an entry. Once a user-store schema migration ran, it could delete the only source that `--rollback` restores from.

The meanings also clash: one folder is for schema migration, the other for the Accounts move. AD-32's exhaustive list ("The app data dir holds only … its pre-migration backups", line 234) names only the user store's backups, so the new folder is not covered by it.

A second run after `--rollback` would also overwrite the first backup at the same path, and the spine does not say whether it should.

**Fix:** use a separate path, for example `<app data dir>/backups/accounts-migrate/<project-id>/<UTC timestamp>.yaml`, which no pruner touches and which `--rollback` restores from (the newest copy). Name it in `AppDataLayout` and in AD-32's "holds only" list.

### M2 (medium): "no agent, script or child process can add or change an Account" overclaims what the TTY check does

*Terminal* (the gate that *Change* reuses) checks `is_terminal()` on stdin and stdout. Any same-user process passes that check by giving the command a pseudo-terminal: `script(1)`, `expect`, `openpty`, or the many agent harnesses that run commands under a pty. The repo's own `crates/am-app/tests/accounts_tty.rs` drives the real binary that way. Such a process can then type the id and `yes`. The check refuses non-interactive callers. It does not prove that a human is at the keyboard.

This sits inside the same-user residual that AD-13 already accepts, and the refusal while a stdio MCP server or `render: true` is enabled narrows it. But the sentence as written is a security claim that is false.

**Fix:** reword it to "any other stdin is refused, so a pipe, a redirected file or a child that inherits no terminal cannot …; a same-user process that allocates a pseudo-terminal can still drive the prompts, the residual AD-13 accepts". Name that residual in the AD-13 or About statement that story 9.11 writes.

### L1 (low): the backup check does not cover durability or a file that changes mid-command

The copy is checked by hash against "the file", and then the original is removed. Two gaps:

- **Durability.** Nothing requires the backup to be fsynced, file and folder, before the removal commits. The spine's own atomic-write rule (line 228) does require this for other files.
- **A file that changes mid-command.** Sync tools take no lock. If OneDrive or iCloud rewrites the file between parse and backup, the hash check passes on the new bytes while the user list got the old entries.

Project memory adds a third point: `std::fs::copy` fails with EPERM in File Provider folders (OneDrive), where owners' projects may live. So the backup should be a read followed by a write, not a clone or copy call.

**Fix:** hash the bytes parsed at the start (sha256, matching line 324). Write those same bytes to the backup and fsync the file and its folder. Before the removal, require the current file and the backup to both match that hash.

### Note (outside the lens, logic only)

After a crash between the two commits of a `--as` run, the re-run must pass `--as` again. *Add* refuses an id already in the user list, so the new id that the first run added must count as "already present" rather than a clash. Otherwise the re-run is refused and the project file is stuck. One clause in *Migrate* would settle this.

## No new technology

The amendment introduces no crate, feature or OS facility that is missing from the Stack. It names no hash algorithm, and the Open Questions bullet makes no tech claim.
