---
title: 'Runtime folder outside the project'
type: feature
ticket: 13
created: '2026-11-21'
status: done
baseline_revision: '6efd9bc85d0008b7ebc80528fb033cc59876ac86'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Runtime state (`state.db`, lock, backups) lives in `<project>/.agentic/`. The owner's projects sit in cloud-synced folders, which corrupt that state and carry it to other Macs.

**Approach:**
- Build ticket 1.13 as restated by the Sofia REPLAN of 2026-11-21 (`6efd9bc`), with architecture AD-7, AD-15, AD-16, AD-32 and AD-47 as amended that day.
- Runtime state moves to `<runtime base>/<project-id>/`, named only by `am_domain::RuntimeLayout`. One Mac opens a project at a time, through the `.agentic-open` marker.
- The CLI refuses, in words, every open decision the owner can settle. The UI equivalents and the pre-open state are epic 5 stories 16 and 19.

## Boundaries & Constraints

**Always:**

- **The ticket is the spec.** Its description and verify (`uv run --no-cache _bmad/method/scripts/tickets.py --project-root . find 1.13`) and AD-32 bind in full: every clause, tool path, limit and wording. Read both before starting. The points below settle what they leave open. On a conflict the ticket wins, and the implementer stops and reports.

- **Where the code goes:**
  - **`am-domain`:** the `ProjectIdentity` port trait and its types: volume UUID, file id, the path (display only), the Mac name, and an error naming the tool and what it returned. `am-testkit` fakes it, tool failures included.
  - **`am-app/src/identity.rs`:** the adapters (AD-47).
    - The macOS adapter is built by constructor parameter: its tool paths and its time limit. Tests build it over stub scripts. `ProjectIdentity::macos()` fixes the absolute paths and 10 s. No flag or env var selects either one.
    - The Linux branch reads the device and inode from std metadata and the name from `/proc/sys/kernel/hostname`.
    - Any other OS returns "not supported on this platform yet".
  - **`am-app`** alone resolves the runtime base, the app data dir (`<OS app data>/agentic-manager/`) and this Mac's id.
  - **`am-store`** opens the runtime folder it is given. It never computes the base and never reads HOME.
  - **`am-config`:** `ConfigIdentityAssign` lives beside story 1.8's writer and shares its intent, History and `ConfigHistory` port. It is a targeted write, not the canonicalising path (see Mint).

- **Identity (owner decisions 2026-11-21):**
  - **Volume:** `/usr/bin/stat -f %Sd <root>` gives the root's device. `/usr/sbin/diskutil info -plist <device>` then gives its `VolumeUUID`. `diskutil` never runs on `/`.
  - **File id:** the root's inode from std metadata.
  - **Mac name:** `/usr/sbin/scutil --get ComputerName`.
  - **Tool rules:** every tool runs by absolute path with a 10 s limit. When a tool fails, times out or returns output without the expected value, the open is refused naming the tool and what it returned. No value is guessed or filled in.
  - **Network share:** a volume with no `VolumeUUID` is refused in words as not supported in v1.
  - **Lock-record host:** the store's lock-record host is the port's Mac name. `am-store` `host_name()` and its `hostname`, `$HOSTNAME` and "unknown host" fallbacks are deleted.
  - **This Mac's id** is a random UUID minted once into `<app data dir>/mac-id`. The file is created exclusively, mode 0600. An existing file that is unreadable or not a UUID is refused in words and never re-minted. `ioreg` and hardware ids are never used. The marker carries this id.

- **Resolve order of `agentic serve --project`.** It follows AD-32's check order. All checks read before anything is written, and the first that fails decides the words.
  1. **Identity tools**, through the port.
  2. **`company.yaml`.**
     - An empty folder is refused naming the first-run wizard.
     - A folder holding config files but no `company.yaml` is refused in words with no action.
     - An unreadable `company.yaml`, or one whose `schema_version` is newer than the binary, is refused.
     - Nothing is created, the base included.
  3. **Base lock** (`<runtime base>/.lock`, held only while resolving). The identity is re-checked under it.
  4. **Root and id.** `project_id` is read through am-config and the root is looked up in the index; a missing or stale index is rebuilt from the root records. The result is one of:
     - normal;
     - a copy;
     - a move;
     - an unmounted recorded volume;
     - a changed or removed id;
     - a second Mac or a restored project;
     - new.
  5. **Legacy folder.**
     - Refused while its lock is held.
     - Refused when it sits beside a runtime `state.db` that has no unfinished move journal, naming both.
     - With an unfinished journal it resumes (see Legacy move).
  6. **Database version.** The runtime or legacy `state.db` is read through a read-only connection; a newer database is refused.
  7. **Marker.**
     - Another Mac's id is refused naming that Mac.
     - This Mac's id with a live pid is refused naming the holder.
     - This Mac's id with a gone pid is replaced.
  8. **Flags.** `--confirm-move` and `--take-over` are refused, naming the UI, while the config (read through am-config, no store opened) enables a stdio MCP server or a Source with `render: true`. Otherwise they are asked on the terminal (see Confirmations).
  9. **Writes, in this order:**
     - mint, runtime folder (0700, files 0600), root record, then index;
     - or the legacy move under its journal;
     - `open_runtime` (migrations; backups to `backups/pre-migration/`, three newest kept, AD-7 unchanged);
     - `ConfigIdentityAssign` when minted;
     - audits;
     - marker;
     - base lock released.

- **Classification:**
  - **Changed or removed id.** A root in the index whose `project_id` is now absent or different is refused naming both ids. The fixes are given in words: write the indexed id back, or treat it as a new project. Nothing is minted over it and `--confirm-move` never opens it.
    - **Crash during a plain mint.** A crash after the root is recorded and before the id is written leaves exactly this state. The next open refuses it as a removed id, and writing the indexed id back opens it.
    - **Sole exception (AD-32's journal rule):** the indexed runtime folder holds an unfinished legacy move journal for this root. The move then resumes, and the journal ends only after the id is written.
  - **Copy.** The id's runtime folder records another mounted root that still holds that id. It is refused naming both folders and offering, in words, "treat this as a new project" (remove `project_id` and open again). The CLI makes no config edit.
  - **Move.** "Gone" means the recorded volume is mounted and no folder with the recorded file id exists on it.
    - macOS checks this with `diskutil info -plist <VolumeUUID>` for the mount point, then std metadata of `/.vol/<device>/<file id>`.
    - Linux is always mounted and checks the recorded path's inode.
    - A move is recorded and audited `project.moved` by `System(runtime)`.
  - **Unmounted.** Refused until `--confirm-move`, which only re-points that root to its own runtime folder. The re-point is audited `project.moved` by the Human actor.
  - **Second Mac or restored project.** The id is present, this Mac has no runtime folder for it, and the root is not in the index. The folder is created and the root recorded. Nothing is minted, no ConfigEdit is made, and AD-16's first-load hold belongs to epic 4 story 18.

- **Root record and index:** the root record lives in the runtime folder and is authoritative. The index is a cache rebuilt from the records. One am-app module writes both, record first, each by temp file and rename.

- **Mint (owner decision 2026-11-21):**
  - A UUIDv7 is minted and the runtime folder created, then the root recorded and the store opened.
  - Then `ConfigIdentityAssign` by `Actor::System(runtime)` inserts one `project_id: <id>` line directly after `schema_version`. Every other byte of `company.yaml` stays as it was: comments, key order, blank lines and line endings.
  - It commits one History entry and one `config.edited` audit and never an approved grant set (AD-16).
  - It goes through 1.8's intent, so `reconcile` settles a crash after the rename.

- **Confirmations:**
  - `--confirm-move` and `--take-over` count only when stdin and stdout are terminals. Otherwise they are refused, saying a terminal is needed.
  - They then ask a question whose default is No.
  - The resolver takes the answers as plain inputs; there is no test flag or env var.
  - They are audited by the Human actor: `project.moved` for `--confirm-move`, `project.taken_over` for `--take-over`.

- **Legacy move** (AD-32, owner decision: kept, never deleted):
  - **When it runs:** before the mint and before any store opens. With no `project_id`, the id is minted in memory.
  - **Before any byte is copied:** the runtime folder is created, the root recorded and the move journal written. The journal holds the source, the phase and the sha256 manifest of every legacy file.
  - **Lock:** the legacy lock is try-locked without rewriting the file.
  - **Copy into the runtime folder:**
    - `state.db` is copied with `VACUUM INTO` and becomes the runtime store. A legacy folder with no `state.db` gets a fresh store.
    - Every other file is stream-copied and checked against the manifest.
    - Never copied: `lock`, `.gitignore`, `state.db-wal` and `state.db-shm`. Their bytes stay in the kept folder.
  - **The kept folder:**
    - The old folder leaves the project as `legacy-agentic-<YYYY-MM-DD>/` (UTC) in the runtime folder.
    - When that name is taken it gets `-2`, `-3` and so on. The free name is chosen under the base lock and written to the journal before the rename.
    - It leaves by a rename. When the rename fails, or the project lies under AD-30's cloud roots (`~/Library/CloudStorage/`, iCloud Drive at `~/Library/Mobile Documents/com~apple~CloudDocs/`), it leaves by a stream copy checked against the manifest, after which the project's copy is removed. am-app decides the cloud root and passes it to am-store as a plain input.
    - It gets mode 0700 and its files 0600.
    - `RuntimeLayout` marks `legacy-agentic-*` as exempt from every sweep, retention and mode-repair walk; the mode check still covers it.
  - **Journal phases:** copying, copied, moving (with the chosen name), id written, done. The id-written phase applies when an id was minted.
    - An open that finds an unfinished journal resumes from its phase.
    - It re-verifies the manifest before trusting a copy.
  - **Failures:** a failed or unverified copy leaves the old folder untouched and refuses the open in words.
  - **Report.** The open is reported like a migration. The report:
    - names the kept folder;
    - says it still holds data that later deletions do not reach;
    - says the owner may delete it by hand.
  - **Never deleted:** the app never deletes the kept folder.

- **Stream copy only:**
  - Every file copy reads and writes the bytes and never uses `std::fs::copy`, whose first step on macOS is `clonefile(2)` (EPERM on OneDrive files without `com.apple.macl`).
  - The production copy is `am-store` `fsx::stream_copy`, which returns the sha256.
  - The test helpers switch to a stream copy: am-config `tests/common/mod.rs` and `tests/validate.rs`, am-e2e `config_history.rs`, `cli_validate.rs` and `spike/c4_files_to_models.rs` (through `am_testkit::copy_tree`), and xtask `tests/denylist.rs` (its own stream copy).
  - `clippy.toml` bans `std::fs::copy` through `disallowed-methods`.

- **Marker (`.agentic-open`):**
  - **Contents:** the Mac name, this Mac's id, the pid and opened-at; no state.
  - **Comparison:** markers are compared by this Mac's id only.
  - **Removal:** the marker is removed before the store lock is released, on every exit after it was written: a clean close, SIGINT, SIGTERM, SIGHUP or an error. No `process::exit` runs in that span. A close removes it only when its id, pid and opened-at are this process's own.
  - **`.gitignore`:** the Blank template's and the Company A sample's `.gitignore` list it.

- **Read-only resolve.** No lock, no write and no identity tool.
  - **`audit`:** refuses in words when there is no `company.yaml`, no `project_id`, an unmoved legacy folder, or no runtime folder on this Mac. `audit --out` is refused inside the runtime folder.
  - **`validate` on a project:** checks `knowledge` entries against its runtime folder, or skips the existence warning when there is none. It never refuses for a missing id or runtime folder and never writes an id.
  - **`validate --template`:** checks `knowledge` entries against `runtime-seed/` and refuses a `project_id` with file, line and fix.

- **Shipped seed files** (AD-32):
  - `templates/samples/company-a/.agentic/knowledge/team-d1/` moves to `templates/samples/company-a/runtime-seed/knowledge/team-d1/`.
  - `fixtures/projects/anatomy/.agentic/knowledge/full/` moves to `fixtures/projects/anatomy/runtime-seed/knowledge/full/`.
  - The repo `.gitignore` exceptions and `docs/config-schema.md` follow.
  - No `.agentic/` is left in `templates/` or `fixtures/`.

- **CI literal check:** `cargo xtask runtime-paths`, run in `just check`, fails on any Rust string literal under `crates/` outside `crates/am-domain/src/runtime.rs` that holds `.agentic` at its start or right after `/`. `app.agentic-manager` passes.

- **Tests first,** failing, for every matrix row and every rewritten story 1.6, 1.7 and 1.8 test.

**Never:**
- A new crate in the workspace or a new dependency.
- `unsafe`.
- `ioreg`, `/etc/machine-id` or any hardware id.
- `diskutil` on `/`.
- `std::fs::copy`.
- A fallback id, volume or host name.
- am-store computing the base or reading HOME.
- Minting over an indexed root, or `--confirm-move` opening a changed or removed id.
- The app deleting the kept legacy folder.
- A test flag or env var for identity, confirmations or time limits.
- Out of scope (epics 4, 5 and 9): any UI, the pre-open server state, the wizard, `ConfigCreate`, Treat as new project, Keep this project's state, "Add to .gitignore", daily backups, and copying seeds into a runtime folder at apply.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error handling |
|---|---|---|---|
| New project | Fixture copy, no id | One `project_id` line inserted after `schema_version`, every other byte unchanged; one History entry and one `config.edited` audit by System(runtime), no grant set; runtime folder (0700/0600) holds lock, state.db and root record; the project gains only `.agentic-open`, which holds no hardware id and is gone after SIGTERM | — |
| Empty folder | No files | Nothing created, base included | Refused naming the first-run wizard |
| Config-only folder | Config files, no `company.yaml` | Nothing created, base included | Refused in words, no action offered |
| Newer config | `schema_version` above the binary's | Nothing written | Refused in words |
| Copy | Copy opened while the original is present | Nothing written | Refused naming both folders, offering in words to treat it as a new project; after removing the id it opens with a new id and an empty runtime folder |
| Move | Copy to a new folder, delete the original, open the copy | New root recorded, `project.moved` audited by System(runtime) | — |
| Unmounted | Root record names a volume the fake reports unmounted | Opens only after `--confirm-move` on a terminal answered Yes; only that root is re-pointed; `project.moved` audited by Human | Refused; the default No keeps it closed; without a terminal, refused saying one is needed |
| Changed or removed id | Indexed root, id edited or deleted | No mint; `--confirm-move` does not open it | Refused naming both ids and the two worded fixes |
| Mint crash | Fail point after the root record, before the id write | Next open refused as a removed id; writing the indexed id back opens it | — |
| Second Mac or restored | Id present, empty base | Folder created, root recorded, no edit | — |
| Legacy with db | `.agentic/state.db` with rows, no id | Rows present via `VACUUM INTO`; id inserted; no `.agentic/` in the project; `legacy-agentic-<date>/` intact at 0700/0600; report names it, says later deletions do not reach it and that the owner may delete it | Lock held: refused. Beside a runtime state.db with no unfinished journal: refused naming both |
| Legacy, files only | Reference files only | Moved the same way, fresh store | — |
| Legacy name taken | `legacy-agentic-<today>/` exists | Kept as `legacy-agentic-<today>-2/` | — |
| Legacy, cloud root | Project root under the fake HOME's `Library/CloudStorage/` | Copy path taken, verified, project copy removed | — |
| Legacy crash | Fail point after the copy, and after the rename before the id write | Next open resumes from the journal and finishes; no new id is minted | — |
| Legacy unverified | Manifest mismatch on copy | Old folder untouched | Refused in words |
| Clone refused | A fixture file armed so any clone of it fails with EPERM | Copied by stream and verified; zero clone attempts recorded | — |
| Kept folder exempt | Backup pruning and every sweep run | `legacy-agentic-*` untouched | — |
| Backups | Five successive migrations | Exactly 3 files in `backups/pre-migration/` | — |
| Newer database | Runtime or legacy `state.db` newer than the binary | Nothing written | Refused in words |
| Markers | Another Mac's id; this Mac's id with a live pid; this Mac's id with a gone pid; a clean close beside a foreign marker | `--take-over` (terminal, Yes) opens and is audited `project.taken_over`; a gone pid is replaced; the foreign marker is left in place | Refused naming that Mac, or the holder |
| Flags locked | Config enables a stdio MCP server, or a Source with `render: true` | Neither flag opens | Refused naming the UI |
| Check order | Identity tool fails on an empty folder; a changed id plus another Mac's marker | First failing check decides | Names the tool; names both ids, not the marker |
| Identity tool | Stub tools (constructor-built, short limit) that fail, hang past the limit, or print no key; the fake reports a share with no VolumeUUID; production constructor uses 10 s | Nothing written | Refused naming the tool and its output, or "not supported in v1" |
| Mac id | `mac-id` absent, then present, then corrupt | Minted once at 0600, then reused | Corrupt: refused in words, not re-minted |
| Validate | `--template` with an id; a project with and without a runtime folder; templates and fixtures | `--template` refused with file, line and fix; project validate checks the runtime folder or skips the warning; `just validate` green, seeds found under `runtime-seed/`, no `.agentic/` left | — |
| Literal | `"/x/.agentic/y"` planted | `xtask runtime-paths` fails with file:line | — |
| Store host | Store lock record | Host is the port's Mac name; no `hostname` or env read | — |

</frozen-after-approval>

## Code Map

- **`crates/am-domain/src/runtime.rs` (new)**
  - `ProjectId`, `MacId`, `RuntimeBase` and `RuntimeLayout`. The layout covers:
    - store and lock: lock, `state_db`, `backups_pre_migration`, `backups_daily`;
    - folders: logs, inbox, drop, records, `knowledge(seat)`, `knowledge_team`, `workspace(seat)`, `mcp_workspace(server)`;
    - identity and legacy: `root_record`, `legacy_journal`, `legacy_copy(date, n)`, `is_kept_legacy(name)`;
    - the pre-open lock.
  - The consts `LEGACY_DIR`, `OPEN_MARKER`, `LEGACY_COPY_PREFIX` and `RUNTIME_SEED`.
  - The `ProjectIdentity` trait with `RootIdentity`, `VolumeState` (mounted with or without the file id, or unmounted) and `IdentityError { tool, output }`.
  - Export them from `lib.rs`.
- **`crates/am-store`**
  - `src/project.rs`:
    - `open_project` (:139) becomes `open_runtime(&RuntimeLayout, host: String)`;
    - delete `host_name()` (:89-112);
    - keep `prepare_dir` (:148-201) without the `.gitignore`, and keep `acquire`, `read_holder` and `LockHolder`;
    - drop `AGENTIC_DIR` (:29) and update the docs (:1-17, `lib.rs:14`), the re-export (`lib.rs:42`) and `tests/project.rs:5`.
  - `src/fsx.rs`: `stream_copy(src, dst) -> sha256`, with a `test-support` clone-refusal arm that counts clone attempts.
  - `src/legacy.rs` (new): the journaled, resumable move with its phases, `VACUUM INTO`, the manifest, the free-name choice, rename or verified copy, and `test-support` fail points.
  - `src/migrations.rs`:
    - `migrate_with` (:165) and `backup_into` (:203) write `backups/pre-migration/` with unique names and prune to three;
    - `OpenReport` (:46, `lines()` :57) carries the legacy line;
    - `lib.rs:452-468` takes the backups folder as a parameter;
    - the unit-test strings at :286-299 move to `RuntimeLayout`.
- **`crates/am-config`**
  - `ConfigIdentityAssign` in `writer.rs`: a new op beside `apply`, whose bytes are the old file with one line inserted, not `canonical_bytes`. It uses the same `begin_intent`, rename and commit.
  - `COMPANY_KEYS` (`kinds.rs:45-55`) gains `project_id`. `Company.project_id` goes in `entities.rs:13-26`, plus `read_project_id(root)`.
  - `load_in(root, LoadOptions { knowledge_root, template })` (`lib.rs:105-120`). Knowledge is checked at `kinds.rs:1323-1366`; the `.agentic` literals are at `kinds.rs:942,1029-1091`.
  - `uses_stdio_mcp_or_render(&snapshot)` from the MCP `transport` (`kinds.rs:746`) and the Source `render` (`kinds.rs:2407`).
  - The helpers in `tests/common/mod.rs:15` and `tests/validate.rs:16` call `am_testkit::copy_tree`.
- **`crates/am-testkit`**
  - `FakeProjectIdentity`, with scripted identities, volume states and tool failures.
  - `copy_tree`, a stream copy.
- **`crates/am-app`**
  - `src/identity.rs`: the macOS adapter (stat, diskutil and scutil with a plist key extractor and no new crate; `/.vol` lookup) and the Linux adapter.
  - `src/runtime.rs`: the base and app data dir, `mac-id`, the resolver, the index and root record writer, the marker guard and the terminal prompt.
  - `main.rs`:
    - `serve` (:77-134) and the flags;
    - SIGTERM and SIGHUP beside ctrl_c (:128);
    - no `fail()` (:37) after the marker;
    - `config_writer` (:138-161).
  - `cli.rs`: `audit` (:132-136, :199-202) and `validate` (:38, `--template`).
- **Other literals and helpers:**
  - Literals: `am-tools/src/lib.rs:60,81` and `am-mcp/src/lib.rs:36`.
  - am-config and am-e2e tests: `am-config/tests/{writer.rs:412,anatomy.rs,validate.rs}` and `am-e2e/tests/{cli_validate.rs:280,config_history.rs:41,spike/c4_files_to_models.rs:21,trust/rule_16_audit_log.rs}`.
  - am-store, am-app and xtask tests: `am-store/tests/{project.rs,audit.rs}`, `am-app/tests/{serve_project.rs,audit_cli.rs}` and `xtask/tests/denylist.rs:221`.
- **`clippy.toml`:** add `std::fs::copy` to `disallowed-methods`.
- **Serve tests:**
  - Each starts from a stream copy of `fixtures/projects/valid`, with a temp HOME and XDG_DATA_HOME.
  - Database and lock cases go through the runtime folder after a first open. The reconcile test (:352) seeds after a first open.
  - Clean stop is `kill -TERM` to the pid in the marker.
  - Terminal cases run under `script` (unix) and answer the prompt.
- **`xtask/src/runtime_paths.rs` (new):**
  - Wire it into `main.rs` and the `justfile` (`check`, and `validate --template`).
  - Reuse `walk.rs` and the literal scanner in `forbid.rs`.
  - The test follows `xtask/tests/denylist.rs:28-46`.
- **Files moving:**
  - The two seed folders move to `runtime-seed/` (exact paths in Boundaries).
  - The Blank and Company A `.gitignore` files gain `.agentic-open`.
  - The repo `.gitignore:24-31` and `docs/config-schema.md:104,258` change to match.

## Tasks & Acceptance

**Execution:**
- [x] Tests first: every matrix row and every rewritten older test. Run them and record the failures in Implementation Notes.
- [x] Build in this order:
  1. am-domain
  2. am-testkit (fake, `copy_tree`)
  3. am-store: `open_runtime`, `stream_copy`, backups, the legacy move
  4. am-config: `ConfigIdentityAssign`, `load_in`, `project_id`, the flag-lock query
  5. am-app: identity, `mac-id`, resolver, index, marker, signals, CLI
  6. the remaining literals and helpers, and `clippy.toml`
- [x] xtask `runtime-paths`, the `justfile`, the seed and `.gitignore` moves, and the docs.

**Acceptance Criteria:**
- Given the repo after this story, when `cargo xtask runtime-paths` runs, then it passes, and a planted `"/a/.agentic/b"` fails it.
- Given `clippy.toml`, when a `std::fs::copy` call is planted in any crate, then `just check` fails.
- Given every matrix row, when its test runs, then it passes, having first been seen to fail.
- Given the Linux CI job, when it builds and runs nextest, then it is green on the Linux adapter.

## Implementation Notes

- 2026-11-21 (implementer):
  - **Where it landed.** `am-domain/src/runtime.rs` (layout, ids, constants, `ProjectIdentity` port); `am-testkit` (`FakeProjectIdentity`, `copy_tree`); `am-store` (`open_runtime`, `check_database`, `lock_holder`, `fsx::stream_copy`, `legacy.rs`, `backups/pre-migration/` with the three newest kept, `OpenReport::legacy_kept`); `am-config` (`project_id` in `COMPANY_KEYS`, `ConfigIdentityAssign`, `load_in`/`LoadOptions`, `read_company`, `decisions_locked`); `am-app/src/identity.rs` (macOS stub-able adapter, Linux adapter, unsupported branch) and `am-app/src/runtime.rs` (places, `mac-id`, root record and index, marker guard, terminal prompt, the resolver, the read-only resolve); `main.rs` (`serve` returns its exit code, SIGTERM/SIGHUP/SIGINT); `cli.rs` (`validate --template`, `audit export` through the read-only resolve); `xtask runtime-paths`.
  - **Tests first, as run.** am-store: the rewritten `project.rs`/`audit.rs` and the new `legacy.rs` failed first (7 failures: a tempdir is 0755 on macOS, so the runtime folder moved to `rt/`; then 2: the legacy read-only open left `-shm`/`-wal` in the old folder, fixed by opening it `immutable` when it has no WAL). am-config `identity.rs` and the rewritten anatomy/templates tests failed first (compile, then 2 failures on the moved seed path in `fixtures/templates/sample.json` and the anatomy warnings, now read with `--template`). xtask `runtime_paths.rs` failed first (unknown task), then caught three `".agentic-open"` literals in the new am-app tests. am-app `identity_tools.rs` failed first on the hang case (a killed tool's child kept the pipe open; the wait for its output is now bounded). The am-app `runtime_open.rs` and `serve_project.rs` rows were written right after the resolver skeleton and were not run against the old code; they pass now.
  - **Dependencies.** No new crate and no new external dependency. The workspace `uuid` gains its `v4` feature for this Mac's random id (`rng` was already on for v7, so `Cargo.lock` gains no package). am-app gains dev-dependencies on `am-testkit` and on itself with a `test-support` feature (the mint fail point), as am-config already does.
  - **Choices the plan left open.** A `stat` device that is not a disk and looks like a share (`//…`, `host:/…`, `map …`) is the v1 network-share refusal; any other non-disk device names `stat`. An unknown VolumeUUID (`diskutil` "Could not find disk") or an empty MountPoint is "unmounted". A copy is detected by reading `company.yaml` through `/.vol/<device>/<file id>` (macOS) or the recorded path (Linux). An unreadable marker counts as another Mac's. `decisions_locked` fails closed when `tools.yaml` or a Source file has an error. Audits: `project.moved`, `project.taken_over` and `project.legacy_moved` (target: the project id).
  - **Verification.** `just check` green on macOS (fmt, clippy `-D warnings`, 395 nextest tests, doctests, check-deps, check-forbid, denylist, runtime-paths, `validate --template` over fixtures and templates); a planted `std::fs::copy` fails clippy; `git ls-files templates fixtures | grep '/\.agentic/'` is empty. The Linux job was not run here (no Linux target installed); its branches compile only by inspection until CI runs.

- 2026-11-21 (build session, after the implementer):
  - **am-app failing run, after the fact.** The implementer did not run `runtime_open.rs` and `serve_project.rs` before the resolver was written. The build session therefore stubbed the resolver back to a skeleton (`open_project`, `find_runtime`, `read_mac_id` and `mint_mac_id` refuse; `knowledge_root` is `None`) with the tests unchanged. `cargo nextest run -p am-app --features am-app/test-support` then ran 59 tests: 45 failed and 14 passed. Every `runtime_open` matrix test failed except `the_fake_is_a_project_identity`. Every new or rewritten `serve_project` and `audit_cli` test failed. The 14 that passed are the identity-tool adapter tests (already seen failing earlier) and the older tests about usage and a missing folder. The implementation was then restored byte for byte. This was a run against a skeleton made afterwards, not a true tests-first order; the owner decides whether it is accepted.
  - **Verification**, run by the build session: the plan's nextest command passed (376 passed; the 6 skipped are pre-existing live or opt-in tests). `cargo xtask runtime-paths` is clean. `git ls-files templates fixtures | grep '/\.agentic/'` prints nothing. `just check` is green.

- 2026-11-21 (finishing session, owner's instructions of 2026-11-21):
  - **Tests-first exception, accepted by the owner.** The owner accepted the am-app gap as an exception, recorded like story 1.1's ("Accept as an exception, recorded like the 1.1 spike exception, with the stub-run evidence (45/59 failing) in the plan"). The evidence is the build session's stub run above: `cargo nextest run -p am-app --features am-app/test-support` against a resolver skeleton gave 45 failed and 14 passed. Re-run here on the same skeleton (`open_project`, `find_runtime`, `read_mac_id` and `mint_mac_id` refuse; `knowledge_root` returns `None`): `59 tests run: 14 passed, 45 failed`. That is a run made after the code; it is recorded as the exception's evidence, not as a tests-first run. Epic 1 Notes record the exception and the owner's process fix, which logs the red run before any `src/` change from now on.
  - **The 14 tests that passed against the skeleton.**

    | Test | What it checks | Behaviour before 1.13? | Against its own unit's skeleton | Strengthened? |
    |---|---|---|---|---|
    | `audit_cli::verify_names_a_line_that_is_not_json` | `audit verify` names a non-JSON line, exit 1 | Yes (story 1.7; 1.13 only adds the temp HOME) | n/a | No |
    | `audit_cli::bad_usage_exits_two` | `agentic audit` usage errors exit 2 | Yes (story 1.7) | n/a | No |
    | `serve_project::serve_with_a_missing_project_folder_is_a_usage_error` | `serve --project <missing>` is a usage error | Yes (story 1.6) | n/a | No |
    | `serve_project::serve_without_a_project_is_unchanged` | `serve` without `--project` still serves | Yes (story 1.6) | n/a | No |
    | `identity::tests::plist_values_are_read_by_key` | plist key extraction | No (new) | Fails | No |
    | `identity::macos::tests::disk_and_share_devices` | disk and share device names | No (new) | Fails | No |
    | `identity_tools::working_tools_give_the_volume_uuid_the_inode_and_the_name` | stub tools give the identity | No (new) | Fails | No |
    | `identity_tools::a_failing_tool_is_named_with_what_it_returned` | a failing tool is named with its output | No (new) | Fails | No |
    | `identity_tools::a_hanging_tool_is_stopped_at_the_limit_and_named` | the time limit | No (new) | Fails | No |
    | `identity_tools::output_without_the_expected_value_is_refused_naming_the_tool` | output without the key | No (new) | Fails | No |
    | `identity_tools::a_share_is_not_supported_in_v1` | a share device is refused | No (new) | Fails | No |
    | `identity_tools::an_unknown_volume_reads_as_unmounted` | an unknown VolumeUUID is unmounted | No (new) | Fails | No |
    | `identity_tools::the_production_adapter_uses_the_real_tools_and_ten_seconds` | the production constructor | No (new) | Fails | No |
    | `runtime_open::the_fake_is_a_project_identity` | the testkit fake is a `ProjectIdentity` | No (new) | Fails | No |

    - Four check behaviour that existed before 1.13. 1.13 changed only their harness (a temp HOME), so passing against the resolver skeleton is right.
    - The other ten are new in 1.13. They test the identity adapter (`am-app/src/identity.rs`) and the am-testkit fake, which the resolver skeleton does not stub, not the resolver.
    - Each of the ten was therefore run against a skeleton of its own unit: the adapter's `root`, `locate`, `mac_name`, `run_tool`, `plist_string`, `is_disk`, `is_share` and `macos()` stubbed, and the fake's `root`, `locate` and `mac_name` returning a tool error. Command: `cargo nextest run -p am-app --features am-app/test-support --no-fail-fast -E 'binary(identity_tools) | test(/plist_values_are_read_by_key|disk_and_share_devices|the_fake_is_a_project_identity/)'`. Result: `12 tests run: 0 passed, 12 failed` (the ten, plus `a_failing_tool_refuses_the_open_with_nothing_written` and the new `a_disk_volume_with_no_volume_uuid_is_not_supported_in_v1`).
    - None needed strengthening. Both files were restored byte for byte, checked with `cmp`.
  - **Review fixes, each test seen failing first** (by a run against the code without the fix, or with the bug put back):
    - Finding 4: `identity_tools::a_disk_volume_with_no_volume_uuid_is_not_supported_in_v1` failed (1 failed), then passed.
    - Findings 3, 5, 6 and 7: `runtime_open::{a_copy_whose_original_cannot_be_read_is_refused_not_moved, an_indexed_root_whose_record_cannot_be_read_is_refused_not_minted_over, a_renamed_project_folder_updates_its_recorded_path, this_macs_marker_is_refused_only_while_its_process_holds_the_project}`. Run: `4 tests run: 0 passed, 4 failed` ("the open must be refused" ×2; recorded path still `projects/a`; "already open on this Mac (pid … opened 2026-11-20T10:00:00.000Z)").
    - Finding 1: `project::checking_a_database_reads_only` failed against the old plain read-only open (`["state.db", "state.db-shm", "state.db-wal"]`), then passed. Its last step asked `database_at_version` to lower a version, which it never does, so the current-version case now uses a fresh database.
    - Finding 10: `runtime_open::a_failure_after_the_legacy_move_still_reports_and_audits_it_on_the_next_open` failed with the early `Done` put back into `move_legacy`, then passed.
  - **A load flake, fixed in the test.** During the squash commit's `just check`, `identity_tools::a_hanging_tool_is_stopped_at_the_limit_and_named` failed: with a 0.3 s limit, the stub `stat` itself did not answer in time while the whole suite ran in parallel, so the error named `stat`, not the hanging `diskutil`. The test's limit is now 3 s; the stub still hangs for 30 s, and the stop is still checked to come in under 10 s.
  - **Left over from the cut-off build session.** am-store did not compile: `legacy.rs` still called `open_source`, which the finding 1 fix had moved to `lib.rs` as `open_read_only`. The call now goes to `crate::open_read_only`.

## Plan Change Log

- 2026-11-21 — The plan was rewritten after the Sofia REPLAN (`6efd9bc`) restated ticket 1.13. Kept from the first draft's independent review:
  - the legacy files skipped;
  - the lock read without a rewrite;
  - the second-Mac case;
  - marker removal on every exit;
  - the read-only resolve;
  - a literal rule that ignores `app.agentic-manager`;
  - the terminal seam.
- 2026-11-21 — The frozen block was renegotiated with the owner's authorisation and rewritten from the ticket, using the checklist `1-13-plan-vs-ticket-2026-11-21.md`. Items 1–16 are resolved as follows:
  1. A random per-Mac id; no `ioreg` or `/etc/machine-id`.
  2. A one-line `project_id` insert, History and audit only.
  3. A changed or removed id is refused, with `--confirm-move` only for unmounted volumes.
  4. A crash during the mint takes the ticket's refusal and its worded fix; the only resume is the legacy journal's.
  5. AD-32's check order.
  6. stat, then diskutil on the device; a network share is refused.
  7. A 10 s limit.
  8. A resumable journal with its manifest, the cloud-root copy, 0700/0600, the sweep exemption and the report wording.
  9. Stream copy everywhere, the test helpers included, plus a clippy ban.
  10. Validate's knowledge root.
  11. The exact seed paths.
  12. Marker comparison by id, the gone pid, and the Company A `.gitignore`.
  13. The `ProjectIdentity` port, the stub adapter, and the store host from the port.
  14. The flag lock, read without the store.
  15. The config-only folder.
  16. Backups: three newest (AD-7 unchanged).

  The independent review's "marker check before any write" now holds because every check reads before any write; the marker comes last in AD-32's order.
- 2026-11-21 — The owner approved the rewritten plan, including the item 4 rule (a crash during a plain mint is refused as a removed id, with the worded fix). Status set to `ready-for-dev`; the frozen block is locked again.

## Review Triage Log

Quick lens, 2026-11-21 (one context-free reviewer over the full diff; 15 findings).

1. **`check_database` leaves `-wal`/`-shm` in the legacy folder.** Fixed: `open_read_only` opens `immutable` when there is no `-wal`, and the test checks for side files (red then green).
2. **Signals between the marker and the server kill the process and leave the marker.** Fixed: `stop_signal()` installs the SIGINT, SIGTERM and SIGHUP handlers when called, and `serve` calls it before `open()`. No test can reliably hit that window, so this was checked by inspection.
3. **A failed read of the original's `company.yaml` counts as "not a copy", so the copy is treated as a move.** Fixed: refused in words naming the original and the read error (red then green). Kept as decided: a folder that exists with the recorded file id but no longer holds the id (inode reuse, or the id removed from the original) is still a move.
4. **A disk volume with no VolumeUUID is a tool error, not the v1 refusal.** Fixed:
   - a `diskutil` plist that describes a volume (it names its `DeviceIdentifier`) but has no `VolumeUUID` is refused as not supported in v1;
   - output that is not such a plist still names the tool;
   - the message now says "a volume with no VolumeUUID, such as a network share";
   - red then green.
5. **The root index is never read, so an unreadable record lets a root be minted over.** Fixed:
   - an index line whose runtime folder still exists but whose record cannot be read survives index rewrites;
   - a root the index lists for such a folder is refused, naming the record;
   - red then green.
6. **A `Normal` open never refreshes the recorded path.** Fixed: a renamed or moved folder (same file id) rewrites its record (red then green).
7. **A reused pid makes this Mac's left-over marker look live.** Fixed: the marker refuses only while its pid is alive and the runtime folder's store lock is held (`am_store::lock_is_held`, a shared try-lock on a fresh handle that never rewrites the file). A reused pid's marker is replaced (red then green).
8. **The clone-refused counter is dead code, so "zero clone attempts" is always true.** Fixed: the dead `clone_file` and `clone_attempts` are removed. The test still arms the file and asserts that it went through `stream_copy` and verified. No clone path exists, because clippy bans `std::fs::copy`. **Owner accepted (2026-11-21):** the frozen matrix row says "zero clone attempts recorded", and that counter no longer exists; the owner accepted the armed file's stream copy plus the clippy ban as the row's evidence.
9. **Runtime paths spelled outside `RuntimeLayout`.** Fixed: `acquire` uses `RuntimeLayout::lock()`, `legacy.rs` names `state.db` through `RuntimeLayout`, and the resolver's legacy check uses `RuntimeLayout::at(old).state_db()`.
10. **A failure after the move loses its report and audit.** Fixed: `move_legacy` no longer marks the journal done; the resolver marks it after the `project.legacy_moved` audit. Shown by a new fail point `AfterLegacyMove`: red with the early `Done` put back, then green.
11. **The `PROJECT_REFUSED` message still says the project holds `.agentic/`.** Fixed: it now says "which holds its configuration", and the same for the ancestor message.
12. **AC "having first been seen to fail" not met for the am-app resolver rows.** Owner exception (2026-11-21), recorded in Implementation Notes and epic 1 Notes with the 45/59 evidence and the 14-test audit.
13. **The Linux job has not run.** Closed: CI run 44882613883 on `story/1-13` (6ca7954) passed both jobs, macOS arm64 (`just check`) and Linux portability (core build and tests).
14. **The serve tests use the real identity tools on macOS.** Kept as is. These tests run the production binary, which can only take its identity from the OS, since a flag or env var for identity is forbidden. Every matrix row that needs scripted identity runs in `runtime_open.rs` through the am-testkit fake, and the stub-tool adapter is covered in `identity_tools.rs`.
15. **`just validate` skips the moved anatomy seeds.** Fixed: the `validate` loop includes `fixtures/projects/anatomy/` with `--template`.

## Design Notes

- **The ticket wins:** where the ticket and this plan disagree, the ticket wins, and the implementer stops and reports rather than choosing.
- **Human actor:** use the one other CLI commands build. If none exists, use `Actor::Human{person:"owner", hat:"Owner"}`.
- **Crash during a plain mint (checklist item 4):** the plan takes the ticket's rule.
  - After a crash between the root record and the id write, the next open is refused as a removed id. Its worded fix, "write the indexed id back", recovers it.
  - The alternative is to reuse the indexed id when its store holds an unfinished mint intent for this root. That needs Winston (AD-32) and is not built here.
  - The legacy path is covered by AD-32's own journal rule.
- **"Gone" on macOS** uses `/.vol/<device>/<file id>` through std metadata, so there is no `unsafe` and no new crate. It was checked on the build Mac on 2026-11-21. If it fails on a test volume, stop and report; never fall back to the recorded path on macOS. Linux, which only runs CI, uses the recorded path's inode.
- **Cloud-only legacy files:** a plain read materialises them (AD-32's download wait). This story adds no `FileResidency` port; that is AD-30's epic.
- **Clone-refused test:** the stream copy never calls `clonefile`. The `test-support` arm makes any clone of the named file fail and counts attempts, so the test proves the stream path ran (zero attempts) and the copy verified. The clippy ban keeps it so.
- **PR #9 (hook fix) is not merged yet:** commit from the main checkout, never from a worktree, until it is on main.

## Verification

**Commands:**
- `cargo nextest run -p am-domain -p am-testkit -p am-store -p am-config -p am-app -p am-e2e -p am-tools -p am-mcp -p xtask` -- expected: all pass.
- `cargo xtask runtime-paths` -- expected: clean.
- `git ls-files templates fixtures | grep '/\.agentic/'` -- expected: no output.
- `just check` -- expected: green.
