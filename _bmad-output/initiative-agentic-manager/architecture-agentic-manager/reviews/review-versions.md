---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-20 architect REPLAN amendments only
date: 2026-11-20
---

# Review: versions and facts in the 2026-11-20 amendments

## Verdict

Every crate version, feature name and licence in the amended Stack rows checks out on crates.io and in the repo. The one claim that is only partly true is that reading a cloud-only file makes the provider download it: whether it does depends on the process's materialization policy, so AD-30 and AD-32 need a rule about it. Two smaller macOS facts are missing too: listing can fetch dataless folders, and macOS asks the owner before an app reads File Provider files.

## Method

- **Repository:** read the workspace `Cargo.toml`, `Cargo.lock`, `deny.toml`, `crates/*/Cargo.toml` and `docs/crate-notes/{croner,globset,jsonschema,rmcp,rusqlite}.md`. Also read the rmcp 3.11.0, globset 0.4.21 and process-wrap 10.0.1 sources from the local cargo registry.
- **crates.io:** checked with `cargo info <crate>@<ver>` and the crates.io API (`/versions`, `/<ver>/dependencies`, version features), all on 2026-11-20.
- **Licence graph:** built a throwaway probe crate (scratchpad, outside the repo) that depends on `jsonschema =0.61.2` with `default-features = false`.
  - Listed every licence with `cargo metadata` for the aarch64-apple-darwin and x86_64-unknown-linux-gnu graphs.
  - Ran `cargo deny check licenses` (cargo-deny 0.21.0) against the AD-3 allow-list, once with MIT-0 and once without.
- **macOS:** read the SDK headers (`sys/stat.h`, `sys/resource.h`) and the man pages (`getiopolicy_np(3)`, `launchd.plist(5)`, `read(2)`, `open(2)`). Also:
  - compiled a `#![forbid(unsafe_code)]` Rust probe that reads `std::os::macos::fs::MetadataExt::st_flags`;
  - ran it against one dataless file and one local file on this Mac (macOS 27.2.1);
  - read this process's dataless I/O policy with `getiopolicy_np`.
- **Web:** Apple TN3150, the Apple Mac User Guide, and support pages from Microsoft, Google and Dropbox; Eclectic Light for iCloud Drive (sources at the end).
- **Not edited:** no file other than this review. The probe stat'ed a dataless file but did not read it, and the file was still dataless afterwards.

## Findings

### F1 (High): reading a cloud-only file downloads it only if the process may materialize dataless files

**Where:** AD-30 "Cloud-only files": "then reads it, which makes the provider download it"; AD-32 `agentic service install` LaunchAgent; AD-47 `FileResidency` adapter.

**What I found:**
- `getiopolicy_np(3)` says the process-scope default is the system default, `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`.
- `read(2)` returns `EDEADLK` when the I/O policy disallows materialization. It returns `ETIMEDOUT` when materialization times out.
- `launchd.plist(5)` has a `MaterializeDatalessFiles` key. If the key is not set, "the default system policy for dataless files will be used".
- Apple TN3150 says accessing a dataless file materializes it. It also documents per-thread and per-process opt-out with `setiopolicy_np` and the `EDEADLK` handling that opt-out requires.
- Measured on this Mac: a process started from a GUI app's session reports policy `2` (ON), inherited from its parent ("New processes inherit the policy of their parent process").

**Why it matters:**
- The Tauri app and anything started from Terminal will most likely download on read.
- The headless `agentic serve` registered by `agentic service install` as a LaunchAgent may get the system default (OFF). Every `read_file` on a placeholder would then fail with `EDEADLK` instead of downloading.
- The app cannot set the policy itself without FFI (`setiopolicy_np`), which `#![forbid(unsafe_code)]` rules out unless an exempt dependency wraps it.

**Not verified:** what policy a LaunchAgent without the key actually gets on macOS 27. I did not install one.

**Suggested fix (REPLAN, not an edit here):**
- AD-32: the LaunchAgent plist sets `MaterializeDatalessFiles` to true.
- AD-30: the `FileResidency` adapter maps `EDEADLK` and `ETIMEDOUT` from a placeholder read to the worded "not on this Mac" refusal.
- Add a macOS-gated test for the service mode, or a spike check that the service reads a dataless file.

**Related, Medium: the 120 s download wait cannot interrupt the read.** A blocking `read`/`open` on a dataless file sits in the kernel until materialization finishes or the kernel itself returns `ETIMEDOUT`. The "download wait" therefore has to be enforced by running the read on a blocking thread and abandoning it, not by cancelling it. The download carries on after the step has been refused, and so does the blocked thread. AD-30 should say that the wait abandons the read rather than cancelling the download, and should cap the abandoned blocking threads.

### F2 (Medium): listing can fetch dataless folders; it only skips file contents

**Where:** AD-30: "listing, glob, type, size and date filters use its metadata only and never download it". AD-40 fan-out "lists without downloading".

**What is confirmed:**
- `stat` on a dataless file did not download it. The probe got `st_flags = 0x40000060` (SF_DATALESS set) and `st_size` = the full logical size. After the probe, `stat -f %Sf` still showed `compressed,dataless`.
- So "decide type from the extension" and "check caps against the placeholder's recorded size" both hold.

**What is not covered:** Apple TN3150 says `stat` and `getattrlist` "trigger the materialization of any intermediate folders in the file's path, if they themselves are dataless". Its example trace shows directory enumeration (`getattrlistbulk`) entering `apfs_materialize_dataless_file_ext`. So listing a root whose subfolders are dataless asks the provider for those folders' contents:
- this is network activity;
- it may be slow;
- it may fail with `ETIMEDOUT` or `EDEADLK` when the Mac is offline (or under F1's policy).

**Suggested fix:** AD-30 says that listing never downloads file contents but may fetch dataless folder listings. It also gives `list_files` the same wait and worded refusal as `read_file` for a folder that cannot be listed.

### F3 (Medium): macOS asks the owner before an app reads files managed by a File Provider

**What I found:** Apple's Mac User Guide ("Allow apps to access files managed by a file provider") says that an app wanting files managed by a file provider, such as iCloud Drive, gets an Allow / Don't Allow dialog. The grant shows under Privacy & Security > Files & Folders.

**Gaps in the spine:** neither AD-30 nor AD-32 mentions this prompt. Three things are open:
- who answers it for the headless LaunchAgent;
- what a "Don't Allow" looks like (the error the adapter sees);
- that the helpers never trigger it. AD-13 helpers get no paths, so this is likely fine.

**Suggested fix:** state in AD-30 that the first link of a cloud-storage folder is done from the app (the UI session) so the prompt can be answered. Map a refusal to a worded "macOS has not allowed Agentic Manager to read <provider> files; allow it in System Settings > Privacy & Security > Files & Folders" message. Add it to the spike or the folder-access story.

### F4 (Low): the refusal wording names a Finder control that does not exist under that name

The AD-30 refusal text says "Make it available offline in Finder". On current macOS, the Finder command for iCloud Drive is **Keep Downloaded** (pinning, added in macOS Sequoia). OneDrive's own command is "Always Keep on This Device". Suggest wording that does not name a control, for example "Download it in Finder (Keep Downloaded or your provider's equivalent)". The UX copy is EXPERIENCE.md's call.

### F5 (Low): the process-wrap feature list understates what is linked

- **What the Stack row says:** process-wrap 10.0.1 with "(tokio1, process-group, kill-on-drop)". The workspace sets `default-features = false` with those three features.
- **What is actually linked:** rmcp 3.11.0 declares `process-wrap = { version = "10.0", features = ["tokio1"] }` with default features on. Cargo's feature unification therefore also turns on process-wrap's defaults: `creation-flags`, `job-object`, `process-session` and `tracing`.
- **Evidence:** `Cargo.lock` lists `tracing` and `windows` among process-wrap's dependencies. The `windows` crate is `cfg(windows)` only, so neither the licences nor the macOS and Linux builds change.
- **Fix:** the row should say "(features as linked through rmcp: defaults plus tokio1)", or just "via rmcp", so no one later believes `default-features = false` is in force.

### F6 (Low): repository files still describe the pre-REPLAN state (follow-up, not a spine error)

These match the spine as it was before the amendment. They are owed to the story that adopts the REPLAN:
- `deny.toml` `allow` has no `MIT-0` yet.
- The `Cargo.toml` comment says jsonschema is "not added" because of MIT-0.
- `docs/crate-notes/jsonschema.md` says "do not add MIT-0 to deny.toml".
- `docs/crate-notes/globset.md` says globset "is not in the architecture's Stack table".
- `docs/crate-notes/rusqlite.md` calls rusqlite_migration a "Stack conflict for the architect".
- The workspace also pins `base64 =0.23.1`, which the amended "Supporting crates" row does not list. Either add it to the row or let the row say "and others".

### Out of scope, noted only

- The jiff 1.x row (not amended) pins `1.0.0`; crates.io has `1.0.1` (2026-11-18).
- The workspace's `rust-version = "1.100"` is far above the Stack's "≥ 1.93".

Neither is part of the 2026-11-20 amendments.

## Claim-by-claim check

| # | Claim (spine) | Repo | crates.io / web | Status |
|---|---|---|---|---|
| 1 | AD-3: MIT-0 added so `jsonschema` links "through `borrow-or-share`"; MIT-0 is MIT without the attribution clause | `deny.toml` not yet updated (F6) | The jsonschema 0.61.2 graph (`default-features = false`) on both the macOS and Linux targets: the only licence outside the allow-list is `borrow-or-share 0.2.4` (MIT-0), via `referencing 0.61.2 → fluent-uri 0.4.1`. With MIT-0 allowed, `cargo deny check licenses` passes; without it, it rejects `borrow-or-share` only. Every `borrow-or-share` version since 0.2.2 is MIT-0 | Confirmed |
| 2 | AD-7: `rusqlite_migration =2.7.0` requires `rusqlite ^0.41`, so no migration crate | Not in lock; own migrator per crate note | 2.7.0 (2026-11-10) is the latest; its normal dep is `rusqlite ^0.41.0`; rusqlite 0.43.0 (2026-11-15, MIT) is the latest | Confirmed |
| 3 | Stack: rusqlite 0.43.0 bundled | `=0.43.0`, `bundled`; lock 0.43.0 | Latest, MIT | Confirmed |
| 4 | AD-18 / Stack: croner 4.0.3, jiff backend, `default-features = false`; backend built on jiff 0.2 | `=4.0.3`, features `jiff`; lock 4.0.3 | 4.0.3 (2026-11-04) is the latest; optional dep `jiff ^0.2.35`; default feature is `chrono`; no croner release uses jiff 1 | Confirmed |
| 5 | Stack: `jiff-croner` = jiff 0.2.46 (package rename) | `jiff-croner = { package = "jiff", version = "=0.2.46" }`; lock holds both jiff 0.2.46 and 1.0.0 | 0.2.46 (2026-11-15) is the newest 0.2.x and satisfies `^0.2.35`; Unlicense OR MIT | Confirmed |
| 6 | AD-26 / Stack: rmcp 3.11.0 with `client`, `transport-child-process`, `transport-streamable-http-client`, no reqwest; `StreamableHttpClient` trait | Workspace pins exactly these, `default-features = false` | 3.11.0 (2026-11-17) is the latest, Apache-2.0. All three features exist. `transport-streamable-http-client = [client-side-sse, transport-worker]` pulls no reqwest; reqwest comes only with `…-reqwest` / `reqwest` / `auth`. Source: `pub trait StreamableHttpClient` with `post_message`, `delete_session`, `get_stream` under that feature | Confirmed |
| 7 | Stack: sse-stream 0.2.6 | `=0.2.6`; lock has a single sse-stream 0.2.6 | rmcp 3.11.0 requires `sse-stream ^0.2.4`. 0.2.6 is the newest 0.2.x. **0.3.0 exists (2026-09-18)** but rmcp does not accept it, and the `Sse` type must be rmcp's, so 0.2.6 is right. MIT OR Apache-2.0 | Confirmed (note: do not "upgrade" to 0.3 independently) |
| 8 | Stack: process-wrap 10.0.1 (tokio1, process-group, kill-on-drop) | `=10.0.1`, those features | 10.0.1 (2026-09-23) is the latest; rmcp requires `^10.0`; the feature names exist; Apache-2.0 OR MIT | Confirmed; feature list understated (F5) |
| 9 | AD-17 / Stack: keyring-core 1.0.1; `keyring` 4 facade not linked | `=1.0.1`, macOS-only target dep in `am-secrets`; `keyring` absent from lock | 1.0.1 (2026-10-13) is the latest, MIT OR Apache-2.0, no dependency on `keyring`. The `keyring` facade is at 4.2.0, so "keyring 4" is accurate | Confirmed |
| 10 | Stack: apple-native-keyring-store 1.0.2, feature `keychain`, macOS only | `=1.0.2`, `keychain`, under `cfg(target_os = "macos")` | 1.0.2 (2026-08-06) is the latest; depends on `keyring-core ^1`; `keychain = [security-framework]` exists (`protected` is the other feature); MIT OR Apache-2.0 | Confirmed |
| 11 | Stack: jsonschema 0.61.2, `default-features = false` drops `resolve-http` and `resolve-file`; MIT | Not yet in the workspace (F6) | 0.61.2 (2026-11-12) is the latest, MIT. Defaults are `resolve-http`, `resolve-file`, `tls-aws-lc-rs`, `idna`; turning them off removes reqwest and rustls (also `idna`, so the `idn-hostname` format is not validated) | Confirmed |
| 12 | Stack / Conventions: globset 0.4.21, Unlicense OR MIT, `default-features = false`; `literal_separator(true)`, case-insensitive for folders | `=0.4.21`, `default-features = false`; lock 0.4.21 | 0.4.21 (2026-10-27) is the latest; licence "Unlicense OR MIT"; the default feature is only `log`. Source has `GlobBuilder::literal_separator` and `case_insensitive` | Confirmed |
| 13 | Conventions: `HostPattern` (AD-48); runtime paths (AD-32) | n/a (design rules, no external technology) | Nothing to check against the web; the public-suffix deferral is stated honestly | n/a |
| 14 | Supporting crates pinned exactly in the workspace `Cargo.toml` | serde, serde_json, uuid, hex, bytes, http, futures-util, getrandom and tempfile are all `=`-pinned | n/a | Confirmed (base64 is also pinned but not named, F6) |
| 15 | File Provider apps (OneDrive, Google Drive, Dropbox) keep files under `~/Library/CloudStorage/<provider>` | On this Mac, `~/Library/CloudStorage/OneDrive-Personal` exists (this repo lives in it) | Microsoft: the OneDrive sync root is in `~/Library/CloudStorage`, and a custom location is a symlink to it. Google: with File Provider (streaming, macOS 12.1+), files are in `~/Library/CloudStorage` and the location cannot be changed. Dropbox on File Provider moves the folder to `~/Library/CloudStorage` | Confirmed. Caveat: Google Drive's mirror mode, Dropbox's opt-out and OneDrive's symlinked custom locations live outside it. Those are not under `~/Library`, so they are linkable anyway, and canonicalising a OneDrive symlink lands in the allowed root |
| 16 | iCloud Drive at `~/Library/Mobile Documents/com~apple~CloudDocs` | Exists on this Mac | Eclectic Light: iCloud Drive appears at that path, and is File Provider based since Sonoma | Confirmed (no Apple doc states the path). Notes: (a) with Desktop & Documents sync on, `CloudDocs/Desktop` and `CloudDocs/Documents` are symlinks to `~/Desktop` and `~/Documents`, so linking the CloudDocs root refuses those entries as root escapes, while `~/Documents` links directly. (b) The Pages, Numbers, Keynote, TextEdit and Preview iCloud folders that Finder shows inside iCloud Drive are separate `Mobile Documents/com~apple~<App>` containers, which AD-30 refuses. That is likely a surprise for owners: decide deliberately |
| 17 | Placeholders detectable from metadata via `SF_DATALESS` in `st_flags`, readable through Rust std `MetadataExt::st_flags` | Probe compiles under `#![forbid(unsafe_code)]` with no warnings | SDK `sys/stat.h`: `#define SF_DATALESS 0x40000000 /* file is dataless object */`. Apple TN3150 prescribes exactly this check. Probe on this Mac: dataless OneDrive file `st_flags = 0x40000060` (bit set), local file `0x40`; `st_size` reports the full size | Confirmed. TN3150 caveat: `stat` materializes dataless parent folders (F2) |
| 18 | Reading such a file triggers its download | n/a | TN3150: accessing materializes. But `getiopolicy_np(3)`: the system default is OFF; `read(2)`: `EDEADLK` when policy disallows it; launchd's `MaterializeDatalessFiles` key | **Partly true:** depends on the process policy (F1) |

## Sources

- crates.io API and `cargo info`: rusqlite, rusqlite_migration, croner, jiff, rmcp, sse-stream, process-wrap, keyring-core, apple-native-keyring-store, keyring, jsonschema, borrow-or-share, globset (all checked 2026-11-20).
- [Apple TN3150: Getting ready for dataless files](https://developer.apple.com/documentation/technotes/tn3150-getting-ready-for-data-less-files)
- [Apple Mac User Guide: Allow apps to access files managed by a file provider](https://support.apple.com/en-kz/guide/mac-help/apps-access-files-managed-file-provider-mchl4206cab8/mac)
- [Microsoft: Inside the new Files On-Demand experience on macOS](https://techcommunity.microsoft.com/blog/onedriveblog/inside-the-new-files-on-demand-experience-on-macos/3058922)
- [Microsoft: Missing files and autosave issues with OneDrive on macOS](https://support.microsoft.com/office/missing-files-and-autosave-issues-with-onedrive-on-macos-7fc55219-7fb5-4885-b117-1c8cb53d2db5)
- [Google: Use Drive for desktop on macOS](https://support.google.com/drive/answer/12178485?hl=en)
- [Dropbox: Expected changes with Dropbox on File Provider](https://help.dropbox.com/installs/macos-support-for-expected-changes)
- [Eclectic Light: How iCloud Drive works in macOS Sonoma](https://eclecticlight.co/2024/03/18/how-icloud-drive-works-in-macos-sonoma/)
- [Eclectic Light: Explainer: File Provider and cloud services](https://eclecticlight.co/2026/05/09/explainer-file-provider-and-cloud-services/)
- [Eclectic Light: Sequoia introduces pinning to iCloud Drive](https://eclecticlight.co/2024/09/16/sequoia-introduces-pinning-to-icloud-drive/)
- Local: the MacOSX SDK `usr/include/sys/stat.h` and `sys/resource.h`; man pages `getiopolicy_np(3)`, `launchd.plist(5)`, `read(2)`, `open(2)` on macOS 27.2.1.
