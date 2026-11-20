---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-21 Sofia REPLAN and owner amendments only (AD-15 project-open decisions, AD-16 ConfigCreate, AD-32 creating a project, identity, legacy folder, pre-open state, Structural Seed)
date: 2026-11-21
---

# Review: versions and facts in the 2026-11-21 amendments

## Verdict

**Pass with changes.** The amendments name no crate and no version, so nothing falls outside the Stack. The three macOS tools exist on this Mac and return the stated values: `diskutil` gives a volume UUID, `ioreg` gives `IOPlatformUUID` and `scutil` gives the Mac's name. `rename(2)` does fail across file systems with `EXDEV`, and macOS has a no-replace rename.

Three committed facts are not yet buildable as written:

- **`diskutil` needs a mount point or device, not the project folder.** The obvious wrong fix, asking about `/`, returns the sealed system snapshot's UUID, which changes with macOS updates.
- **`ConfigCreate`'s "renamed without replacing" names no mechanism.** The Rust standard library has no such rename. The crate that offers one, `tempfile`, is only a dev-dependency of `am-config`, and `rustix` is not in the shipped graph.
- **AD-47 does not list the new identity adapter as a port.** Under "no fallback", no project could open in the Linux CI job.

## Method

- **Diff:** `git diff` of `architecture-agentic-manager.md` (6 changed lines) and `.memlog.md`.
- **Repository:** read `rust-toolchain.toml` (channel 1.100.0) and the workspace `Cargo.toml`, which has `unsafe_code = "forbid"` and exact pins. Also read `Cargo.lock`, `crates/am-config/Cargo.toml`, `crates/am-app/Cargo.toml` and `crates/am-config/src/writer.rs`.
- **Dependency graph:** ran `cargo tree --offline -e normal -i rustix` and `-i tempfile` for aarch64-apple-darwin. Both print nothing, so neither crate is in the shipped graph today.
- **Crate sources (local cargo registry):** read tempfile 3.27.0 `src/file/imp/unix.rs` (`persist`) and rustix 1.1.6 `src/backend/libc/fs/{syscalls,types}.rs` (`renameat_with` on Apple).
- **macOS 27.2.1 (arm64), all read-only:**
  - ran `diskutil info -plist` on every mounted file system, on the project folder, on `/Users`, on `/` and on device nodes;
  - ran `ioreg -rd1 [-a] -c IOPlatformExpertDevice`, `scutil --get ComputerName` and `stat -f %Sd`;
  - read `man 2 rename`.
  
  UUIDs are not reproduced here; they are reported only as present or absent.
- **Probes (scratchpad, outside the repo):**
  - a Python `ctypes` call of `renamex_np(..., RENAME_EXCL)`, `link(2)` onto an existing name, and a directory renamed onto an empty directory;
  - a `#![forbid(unsafe_code)]` Rust program compiled with the pinned 1.100.0 toolchain. It uses `io::ErrorKind::CrossesDevices`, `std::os::macos::fs::MetadataExt::{st_dev, st_ino, st_flags}` and `std::fs::hard_link` onto an existing name.
- **Not edited:** no file other than this review. Nothing was written inside the project or the OneDrive tree.

## Facts checked

| Claim in the amendment | Result |
|---|---|
| Names no crate or version outside the Stack | **Holds.** The only technical names are `diskutil`, `ioreg`, `scutil` (OS tools), `VACUUM INTO` and sha256, which were already present. sha2 0.11.1 is in the Stack, and the bundled SQLite (libsqlite3-sys 0.40.0) is far past 3.27, which added `VACUUM INTO`. |
| Volume UUID via `diskutil` | **Holds on disk-backed volumes, with a caveat.** `/usr/sbin/diskutil` is present. `diskutil info -plist /System/Volumes/Data` has `VolumeUUID` present (about 11 ms). `diskutil info -plist <project folder>` exits 1 ("Could not find disk"); so does `/Users`, which is a firmlink, not a mount point. `diskutil info -plist /` returns the **sealed snapshot** (`disk3s1s1`, `APFSSnapshot = true`), whose UUID differs from the Data volume's. See F1. |
| Host id via `ioreg` | **Holds.** `/usr/sbin/ioreg` is present. `ioreg -rd1 -c IOPlatformExpertDevice` has `IOPlatformUUID` present (about 10 ms), and `ioreg -a` gives the same as a plist. |
| Mac's name via `scutil` | **Holds.** `/usr/sbin/scutil` is present. `scutil --get ComputerName` exits 0 with a non-empty name (about 10 ms). |
| "rename on the same volume, else copy" | **Holds.** `man 2 rename` lists `[EXDEV]`: the two names are "on different logical devices (file systems)". Rust maps that error to `io::ErrorKind::CrossesDevices`, which compiles on 1.100.0. |
| A no-replace rename exists on macOS | **Holds.** `renamex_np`/`renameatx_np` with `RENAME_EXCL` work on volumes that advertise `VOL_CAP_INT_RENAME_EXCL`; other volumes get `ENOTSUP`. Probe on APFS: the call succeeded when the target was absent and returned `EEXIST` when it existed, leaving the target unchanged. |
| …and is reachable from this workspace | **Not as written.** Rust std has no flagged rename, and the crates forbid unsafe code. tempfile 3.27.0's `persist_noclobber` calls rustix's `renameat_with(..., NOREPLACE)`, which maps to `renameatx_np` + `RENAME_EXCL` on macOS. But tempfile is a **dev-dependency only** of `am-config`, and neither tempfile nor rustix 1.1.6 is in the normal graph. See F2. |
| Identity "only from the OS", refused on failure | **Holds on macOS** (failures are detectable: `diskutil -plist` exits 1 and prints an error plist). Off macOS it is **unspecified**. See F3. |

## Findings

### F1 (Medium): `diskutil` cannot be pointed at the project folder, and pointing it at `/` gives an id that changes on macOS updates

**Where:** AD-32 "Identity comes only from the OS".

**Evidence:**
- `diskutil info -plist <path>` accepts only a mount point or a device. On this Mac it fails for the project folder and for `/Users`.
- User folders reach the Data volume through firmlinks. Walking up a folder's parents and comparing `st_dev` (the std-only route) never finds a boundary at `/Users`, because `stat -f %Sd` reports `disk3s5` for the folder, for `/Users` and for `/`. The walk ends at `/`.
- `diskutil info /` describes the sealed system snapshot. Its UUID is not the Data volume's, and a new snapshot is sealed with each macOS update.

An adapter that ends up at `/` would therefore record a volume id that changes after an OS update. With "no fallback", every project would then be refused as "unmounted volume or changed id" after each update.

**Working route (verified):** run `/usr/bin/stat -f %Sd <root>` to get `disk3s5`, then `diskutil info -plist disk3s5`. It has `VolumeUUID` present, and its `MountPoint` is `/System/Volumes/Data`. `df -P` also gives the mount point, but its text is fragile to parse when paths contain spaces.

**Fix:** In AD-32, say that the volume is the one holding the root, found from the root's device. Name the fourth tool (`stat -f %Sd`, or `df`) next to `diskutil`, `ioreg` and `scutil`. Forbid calling `diskutil` on `/` or on a folder path. Call each tool by absolute path (`/usr/sbin/diskutil`, `/usr/sbin/ioreg`, `/usr/sbin/scutil`, `/usr/bin/stat`) and use machine output (`-plist`, `ioreg -a`). Add a test that fails if the recorded volume is an APFS snapshot.

### F2 (Medium): `ConfigCreate`'s "renamed without replacing" has no mechanism the workspace can use as pinned

**Where:** AD-16 `ConfigCreate`, and AD-32 "Creating a project".

**Evidence:**
- `std::fs::rename` replaces an existing file. Rust std (1.100.0) has no flag for a no-replace rename, and `unsafe_code = "forbid"` rules out calling `renamex_np` directly.
- tempfile's `NamedTempFile::persist_noclobber` does call `renameatx_np` + `RENAME_EXCL` (through rustix). It falls back to `link` + `unlink` only on `ENOSYS`/`EINVAL`, so on a volume without `RENAME_EXCL` it returns `ENOTSUP`.
- tempfile is a dev-dependency of `am-config`. Making it a normal dependency adds tempfile 3.27.0, rustix 1.1.6 and fastrand to the shipped graph. Both crates have allowed licences ("MIT OR Apache-2.0" and "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"), and the Stack's supporting-crates row covers tempfile. rustix is not named in the Stack, and the story would have to own that change.

**Std-only alternative (verified):** `std::fs::hard_link(tmp, file)` then `remove_file(tmp)`. `link(2)` fails atomically with `AlreadyExists` when the name exists, as the Rust probe on 1.100.0 showed. A crash between the two calls leaves only `.<name>.am-tmp`, which the writer already removes when it opens (`writer.rs`). The cost: file systems without hard links refuse it. That includes exFAT and FAT and some SMB shares. Whether hard links work in File Provider domains such as OneDrive was **not verified**.

**Fix:** Choose one mechanism in AD-16 and name it:

- **Recommended:** `hard_link` + `remove_file`, std only and no new crate. A refusal names the file system.
- **Or:** `tempfile::persist_noclobber`, with tempfile promoted to a normal dependency of `am-config` and rustix 1.1.6 recorded in the Stack.

Either way, the story's acceptance tests should cover:

- an APFS folder;
- a File Provider folder (manual, on the owner's Mac);
- the "file appeared meanwhile" refusal.

`ConfigCreate` also has no existing file to copy the mode from, as `ConfigEdit` does at `writer.rs` `replace`. AD-16 should state the mode it uses.

### F3 (Medium): the identity adapter is not an AD-47 port, and off macOS it would block every project from opening

**Where:** AD-32 "Identity comes only from the OS", and AD-47 Portability.

**Evidence:**
- AD-47 lists the OS adapters that sit behind ports with a compiling non-macOS branch (`SecretStore`, `LocalService`, file-permission checks, `FileResidency`, the runtime base, `agentic service`). The new identity reader is not on that list.
- The `.memlog.md` decision says "off macOS its own sources". The spine says nothing about what happens off macOS.
- Suppose the non-macOS branch returns AD-47's worded "not supported on this platform yet" error. Under "no id is ever guessed or filled in from a fallback", every open then fails. That breaks any `am-app` or `am-e2e` test that opens a project in the Linux portability job, unless those tests are gated or use a fake.

**Fix:** Add the identity reader to AD-47's port list. It could be a port such as `HostIdentity` (volume id for a path, file id, host id, Mac name), with an `am-testkit` fake. Say which Linux branch applies. It can be real sources: `/etc/machine-id`, the hostname, the filesystem UUID from `/dev/disk/by-uuid` or `statx`, and `st_ino`. Or it can be the worded error, with Linux tests using the fake. Either way the Linux job stays green without a fallback in the product.

### F4 (Low): projects on volumes with no volume UUID can never open

**Evidence:** On this Mac, mounts that are not backed by a disk have no `VolumeUUID` from `diskutil`. That covers `devfs`, `autofs` (`/System/Volumes/Data/home`) and a `nullfs` mount. Every disk-backed volume, including a mounted HFS disk image, has one. No SMB, NFS or AFP share was mounted, so network shares were **not tested**, but they are expected to behave like the other non-disk mounts.

Under AD-32, a failing identity tool is a refusal with no action in the pre-open state. A project on a NAS share would therefore never open.

**Fix:** Say this outright in AD-32: projects on network shares (volumes with no volume UUID) are not supported in v1. The refusal should say so in words rather than only naming the tool output. If the owner wants NAS support, that is a new decision.

### F5 (Low): two `rename(2)` details for the legacy move

**Evidence:**
- *A directory rename replaces an existing empty directory.* `rename(2)` fails with `ENOTEMPTY` only when the target is a non-empty directory. The probe confirmed that a directory renamed onto an empty `emptydir` silently replaced it. So the "`-2`, `-3`… when the name is taken" rule cannot rely on `rename` failing. It must check under the runtime base lock, or use `RENAME_EXCL` (which has the same dependency question as F2).
- *Same volume does not guarantee that the rename succeeds.* Moving a folder out of a File Provider domain (for example `~/Library/CloudStorage/...` into `~/Library/Application Support/...`) is a same-volume rename on APFS. With dataless items inside, it may fail, depending on the process's materialization policy (see review-versions.md F1). The effect on the moved items' readability was **not verified**.

**Fix:**
- Try the rename first, and take the copy path on **any** rename error, not only on a volume mismatch found in advance. The ordinary cross-volume case is `ErrorKind::CrossesDevices`.
- Check the `legacy-agentic-<date>` name under the lock.
- Have the open report say that the kept folder may hold cloud-only placeholders. The verified copy is already in place, so nothing is lost.

## Not findings (checked and fine)

- The amendment's new CLI flags (`--take-over`, `--confirm-move`) and the UI actions name no crate.
- `VACUUM INTO`, sha256 and serde-saphyr are already in the Stack or were already in the text.
- Each identity tool returns in about 10 ms. Any timeout of a second or more is far above normal, so "times out → refuse" will not fire in ordinary use.
- `std::os::macos::fs::MetadataExt::st_ino` (the folder's file id) and `st_flags` (`SF_DATALESS`) are available with `unsafe` forbidden on 1.100.0.

## Sources

- `man 2 rename` on macOS 27.2.1 (the `RENAME_EXCL`, `EEXIST`, `ENOTSUP`, `ENOTEMPTY` and `EXDEV` entries).
- tempfile 3.27.0 `src/file/imp/unix.rs` `persist`; rustix 1.1.6 `src/backend/libc/fs/syscalls.rs` (`renameatx_np`) and `types.rs` (`NOREPLACE = RENAME_EXCL`).
- Repository: `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`, `crates/am-config/Cargo.toml`, `crates/am-config/src/writer.rs`.
