---
review: adversarial (pairwise incompatibility)
target: architecture-agentic-manager.md, the 2026-11-21 Sofia REPLAN amendments only (AD-15 project-open decisions and ConfigCreate classification; AD-16 ConfigCreate and the create baseline; AD-32 creating a project, identity from the OS, copy wording, Confirm move, legacy folder kept as legacy-agentic-<date>/, the pre-open state; Structural Seed runtime-folder line)
date: 2026-11-21
method: read only; for each amendment, build two units one level down (stories or crates) that each obey every AD as written and still build incompatibly, open a trust gap or lose data
---

# Adversarial review: the 2026-11-21 amendments

## Verdict

Not yet safe to build story 1.13 or epic 5 story 16 from. Two of the new paths silently approve a project's whole grant set: the mint and Treat-as-new identity writes, and the wizard's create in a folder that already holds config. Both defeat AD-16's first-load hold. Four other problems block building:

- The pre-open state has no port that `am-server` may call under AD-4.
- The pre-open state has no lock that keeps a second process out.
- "Confirm move" has no defined effect for a changed id.
- The order of the legacy move against the mint order refuses every legacy project.

The two tickets that build this (1.13 and 5.16) still describe the superseded behaviour. Story 1.13 as ticketed deletes the legacy folder that the owner decided to keep.

Scope: the passages marked "amended 2026-11-21" and the owner decisions in the last five entries of `.memlog.md`. Other ADs were read only to judge how they interact with the amendments: AD-4, AD-7, AD-13, AD-24, AD-25, AD-35 and AD-47. Built code checked: `crates/am-config/src/writer.rs` (story 1.8: the `ConfigHistory` port, the intent/write/commit order, reconcile and temp cleanup). Tickets checked: epic 1 story 13 and epic 5 stories 12 and 16.

Severity: **C** critical (breaks a trust rule), **H** high (two correct units cannot be joined, or data is lost), **M** medium, **L** low.

---

## A1 (C): Identity writes commit the approved grant set, so the first-load hold never fires for an id-less project or a copy

**Unit A:** `am-config`'s writer (AD-16 apply order, story 1.8 as built). Every `ConfigEdit` commits "History, the audit entries and the approved grant set in one transaction". AD-16 also says "the store persists the approved grant set … each time an apply commits".
**Unit B:** story 1.13's resolver (AD-32). For a `company.yaml` without `project_id` it mints an id and writes it with a `ConfigEdit` by `Actor::System`. For a copy, the UI's Treat as new project writes the new id "with one `ConfigEdit` by `Actor::Human`".
**Both obey:** AD-15, AD-16, AD-32.

**Clash.** In both cases the store is brand new, with no approved grant set. The first write into it is the identity `ConfigEdit`, and per Unit A that write commits the approved grant set of the snapshot it produced. That is every tool, MCP server, host, linked folder and capability in the files. The next load compares the snapshot with that set, finds no widening, and starts runs.

So the two-step hold that AD-16 promises fires for none of these:
- a project "opened from files for the first time on this Mac";
- Winston's Treat-as-new copy;
- the amendment's own wording of the copy fix ("remove `project_id` … and open again").

The hold fires only for a project whose `company.yaml` already carries a `project_id` unknown to this Mac. Anyone can produce the silent path by deleting one line: a cloned repo, a shared template folder, or a stdio MCP child running as the owner.

**Tightening (AD-16, AD-32):**
- The approved grant set is written only in three cases. (a) A confirmed apply, whose widenings were listed. (b) The `ConfigCreate` baseline (A2), from the bytes created. (c) A non-widening edit, and only when a baseline already exists; it never creates one.
- An identity write never creates or replaces the baseline. This covers the System mint, Treat as new project and Confirm move.
- Make the identity write a typed command of its own, `ConfigIdentityAssign`, not a `ConfigEdit`. `is_widening`'s exhaustive match must refuse `project_id` in any other edit (see A7).
- Trust test `widening_first_open_id_less.rs`: an id-less fixture with one tool grant opens with its first load held and lists that grant. The same holds after Treat as new project.

## A2 (C): The wizard's create baselines config files it did not create

**Unit A:** the epic 5 story 16 wizard. It runs "with no `company.yaml` in the served folder" and does a `ConfigCreate` of `company.yaml` at step 1.
**Unit B:** `am-config`/`am-store` per AD-16. The create "commits the baseline in its own transaction", and a snapshot is always the whole project.
**Both obey:** AD-15 ("a ConfigCreate is classified against an empty grant set": `company.yaml` carries no grant, so it is not widening), AD-16 and AD-32 ("a served folder with no `company.yaml` is a new project").

**Clash.** The amendment never requires the folder to be empty. A git checkout or a synced folder can hold `seats/*.md` with tools and hosts, `models/accounts.yaml`, MCP specs or `sources/` but no `company.yaml`. Through the wizard such a folder becomes a "created" project, and its baseline is the snapshot's whole grant set. Every stray grant is approved without the two-step confirmation. Deleting `company.yaml` turns "first open from files (held)" into "create (approved)".

**Tightening (AD-32, AD-16):**
- The wizard is offered only when the folder holds no config file at all (`company.yaml` and none of the AD-16 layout's files or folders). Otherwise refuse in words: "This folder holds config files but no `company.yaml`." This is a refusal with no action, or a later "Open as a project from files" path that holds the first load.
- The create baseline is computed from the created file's bytes only. It is never computed from a snapshot that includes files the create did not write.

## A3 (H): Story 1.13 and epic 5 story 16 still specify the superseded behaviour; 1.13 deletes the kept legacy folder

**Unit A:** story 1.13 as ticketed (`tickets/epic-foundations-rust-spike/tickets/tickets.toml`, id 13). Its legacy move has "the old folder removed" and its verify expects "the legacy folder gone". Settling an open happens only through `--take-over` and `--confirm-move` flags. It has no pre-open state, so a refused or empty folder exits. There are no OS identity tools and no refusal when one fails.
**Unit B:** story 5.16 as ticketed. "step 1 writes company.yaml through a config edit", but `ConfigWriter` "never creates … a config file" (writer.rs header). Step 2 adds Accounts, probes local ports and shows Local search status. None of those exist in a pre-open state whose API "answers only status and the wizard or the decision".
**Both obey:** AGENTS.md "the ticket is the spec".

**Clash.** A builder who follows ticket 1.13 deletes the legacy folder. That is the data-loss path the owner decided against on 2026-11-21. Ticket 5.16 cannot be built on the shipped writer. It also leaves open whether steps 2–4 run pre-open or open (A5).

**Tightening:** Sofia re-cuts 1.13 (kept `legacy-agentic-<date>/`, pre-open, identity refusals, decisions as Human commands) and 5.16 (`ConfigCreate`, pre-open entry, the hand-over to open after step 1) before either builds. Add an acceptance line to 1.13: "the legacy folder exists under the runtime folder as `legacy-agentic-<date>/` with every file's sha256 unchanged".

## A4 (H): The mint order opens a store before the legacy move, and a crash mid-move refuses the project with no action

**Unit A:** story 1.13's resolver, in AD-32's order: read `project_id`; "if absent, mint one, create the runtime folder, record the root, open the store, then write `project_id`".
**Unit B:** story 1.13's legacy mover. It runs "on its first open". When "both a legacy folder and a runtime folder holding a `state.db` exist … the open is refused naming both", and the pre-open state lists that refusal as one "the owner cannot settle there".
**Both obey:** AD-7, AD-32.

**Clashes.**
1. Every legacy project predates `project_id`, so Unit A mints and opens a fresh `state.db` before Unit B looks. Unit B then refuses forever. Alternatively, a builder who copies the legacy database over the fresh one loses the mint's History and audit entries and breaks the hash chain (AD-25).
2. Suppose the process crashes after the verified copy and before the legacy folder leaves the project, or during the cross-volume "copy, then remove from the project". Both folders now exist. The project is refused permanently with "no action", and the only way out is for the owner to delete a folder by hand in a hidden `~/Library` path. Deleting the wrong one loses data.

**Tightening (AD-32):**
- The legacy check runs first, before the mint. A legacy project mints its id, creates the runtime folder and places the `VACUUM INTO` copy as its `state.db` before the store is opened. The mint's `ConfigIdentityAssign` (A1) then lands in the migrated store.
- The move keeps a journal, `legacy-move.json` in the runtime folder. It records the source root identity, a sha256 manifest and a phase (copied / kept / removed-from-project). Each phase transition is fsynced. An open that finds a journal resumes the move idempotently instead of refusing.
- The "both exist" refusal applies only when no journal names that legacy folder.

## A5 (H): The pre-open state has no port under AD-4, and its hand-over to open is unspecified

**Unit A:** an `am-server` story serving the pre-open API (wizard, decision). Under the Design Paradigm table it may depend only on `am-domain`, `am-engine`, `am-api` and the `am-store` read handle. It cannot reach `am-config` (ConfigCreate), the resolver in `am-app`, or a writer, and pre-open has no store and no engine.
**Unit B:** story 1.13 in `am-app`. It owns the resolver and builds `am-server`'s state from an open store and engine.
**Both obey:** AD-4, AD-13, AD-14, AD-32.

**Clashes.**
1. Unit A either adds an `am-config` edge, which `cargo xtask check-deps` fails, or invents its own port. Unit B either restarts the server after the decision or swaps state in process.
2. A restart mints a new in-memory session secret (AD-13). The signed-in wizard loses its cookie at step 2, the launch code is gone, and with `--no-browser` the owner sees a dead tab.
3. In-process, nothing says which pieces start at the hand-over: the writer, scheduler, Local search, and the sandbox self-test, whose helper launch the pre-open "no child starts" forbids.

**Tightening (AD-32, AD-4):**
- Name one port in `am-domain`, `ProjectOpener` (status, create, decide). `am-app` implements it, and `am-server` holds it as `Arc<dyn ProjectOpener>` in both states.
- The hand-over to open happens in the same process and keeps the port, session secret and session. The WebSocket reconnects and the UI re-reads status.
- After the hand-over, start in this order: store, writer, reconcile, self-test, then services.
- State that the wizard runs pre-open through step 1's create only. Steps 2–4 run on the open project.

## A6 (H): CLI-class decisions in the pre-open state skip AD-13's narrowing and have two channels

**Unit A:** story 1.13 implements Open here anyway and Confirm move as the startup flags `--take-over` and `--confirm-move` behind `isatty`.
**Unit B:** the pre-open API story. AD-15 now says "both classes may make them", so it accepts the same decisions from the CLI token on its decision endpoint.
**Both obey:** AD-13, AD-15, AD-32.

**Clash.** AD-13 takes all decisions away from the CLI class while the project enables a stdio MCP server or a `render: true` Source, because those children run as the owner. They can read the CLI token and fake a TTY (a pty passes `isatty`). The amendment does not apply that rule to project-open decisions, and pre-open has no store to ask. Unit B also opens a second channel that no TTY guards at all.

A child of another project on this Mac can read the app data dir's CLI token. It can then take over a project open on another Mac, so two Macs run one ledger and double-schedule. It can also Confirm-move a project onto a different runtime folder (A7).

**Tightening (AD-15, AD-13):**
- A project-open decision by the CLI class is only a startup flag. The process checks it itself with `isatty` on stdin and stdout. There is no CLI-token API for decisions.
- The flag is refused, pointing to the UI, when the project's config snapshot (readable pre-open through `am-config`, no store needed) enables a stdio MCP server or a rendering Source, or cannot be loaded.
- Trust test `rule_NN_preopen_cli.rs`.

## A7 (H): "Confirm move" for a changed id has no defined effect and can bind a project to another project's state

**Unit A:** a resolver that reads Confirm move on "a root whose index entry names a different id" as "the file wins". It opens the runtime folder of the id now in `company.yaml` and re-points the index.
**Unit B:** a resolver that reads it as "the index wins". It opens the indexed folder and ignores the file's id, which leaves `company.yaml` and state disagreeing on every later open.
**Both obey:** AD-16 ("IdentityChange that Reload never applies") and AD-32 ("refused … until the owner confirms the move … the owner's runtime decision, not a config edit").

**Clash.** The two units give different outcomes. Unit A has a trust gap. A hand edit, a git pull or a child sets `project_id` to the id of another project on this Mac. If that project's recorded root is not mounted (an external disk), the copy rule does not catch it, and Confirm move binds this project's files to the other project's ledger, Inbox and approved grant set. `is_widening` then compares against the wrong baseline, so grants the other project had approved apply silently. If the id names no runtime folder, Unit A creates an empty one and the real history is orphaned.

**Tightening (AD-32):**
- Confirm move only re-points a root identity to the runtime folder it already belongs to: the volume moved, or the folder moved.
- A changed id is never confirmed as a move. It offers either "Keep this project's state" or Treat as new project, and both are UI class. "Keep this project's state" is a `ConfigIdentityAssign` writing the indexed id back; it does not change the baseline.
- A runtime folder whose recorded root identity differs from this root's is never adopted by Confirm move. Adopting it is only the copy decision.

## A8 (H): Root identity has two owners, and "gone" is inferred from what cannot be read

**Unit A:** an `am-app` story keeps the root index as a JSON file in the runtime base.
**Unit B:** an `am-store` story keeps "each runtime folder records its root's identity" as a table in `state.db`, written by the writer with audit.
**Both obey:** AD-32 (which names both records, but no owner, no format and no rule for when they disagree) and AD-35, which does not place either.

**Clashes.**
1. Confirm move or a recorded move updates one record and crashes before the other. The next open sees the records disagree and refuses again, or treats the project as a copy.
2. A unit that finds the "other root" by its display path, rather than by volume UUID and file id, reports a renamed original as "gone". So does a unit whose read of the other root's `company.yaml` times out because the file is dataless in a cloud folder. Either result is treated as a move: the copy adopts the original's runtime folder. The original is then refused as a copy of its copy, and if Treat as new project is chosen on it, its own history moves to the copy for good.

**Tightening (AD-32, AD-35):**
- The runtime folder's record (a file named by `RuntimeLayout`, not the database, so it is readable before the store opens) is authoritative.
- The root index is a cache that can be rebuilt by scanning runtime folders under the base lock. One `am-app` module writes both, record first, each by temp file and rename.
- Recorded roots are found only by volume UUID and file id. The path is for display only.
- "Gone" requires the volume to be mounted and the file id to be absent. An unreadable or undecided `company.yaml` refuses in words and never infers a move.

## A9 (H): A crash during create leaves the folder un-creatable, and removing the id from an original silently forks it

**Unit A:** the wizard's create, in AD-32's order: mint, create runtime folder, record root, open store, then `ConfigCreate`.
**Unit B:** the resolver. It refuses "a root whose index entry names a different id", but it checks "read `project_id`; if absent, mint" before it consults the index.
**Both obey:** AD-16 and AD-32.

**Clashes.**
1. A crash between "record the root" and the `ConfigCreate` rename leaves the index saying root → Z with no `company.yaml`. The relaunched wizard mints W and Unit B refuses it as a changed id. First run is blocked behind a "Confirm move" the owner cannot understand, and runtime folder Z is orphaned.
2. In the other order, an original project whose `project_id` is deleted (by a hand edit, a pull or a child) silently mints a new id, opens an empty store and resets the week and month ledger. That is a cap raise `is_widening` never sees. History, Inbox and schedules are orphaned. Only A1's fix stops it starting runs.

**Tightening (AD-32):**
- The index lookup comes before the mint.
- An absent id on a root already in the index is refused like a changed id. Its offers are "Keep this project's state" (write the indexed id back, UI class) or Treat as new.
- During create, record the root in the index only after the `ConfigCreate` commits. A runtime folder with a pending create intent and no `company.yaml` is reused by the next wizard run.
- A reconciled `ConfigCreate` intent also writes the baseline. Otherwise a crash after the rename leaves the created project's first load held, which the 2026-11-21 decision forbids.

## A10 (H): Two processes can sit in pre-open on one root, and decisions are not compare-and-swap

**Unit A:** the LaunchAgent (`agentic service`: `serve --no-browser`). It enters pre-open, waits, and holds no lock, because AD-32's project lock lives in the runtime folder and pre-open has none open.
**Unit B:** the desktop app (links `am-app` in process; AD-13 sessions do not cross processes). It enters pre-open for the same folder, or the owner uses a stale browser tab of A.
**Both obey:** AD-13 and AD-32.

**Clash.** The owner chooses Treat as new project in B, and B opens on id Z. The stale decision page in A then mints W and writes it over Z. A `ConfigEdit` intent records only the old hash and does not refuse when the file changed. B's watcher sees an `IdentityChange` that it can never Reload, and the next start orphans Z's state.

The same race happens with two Macs in the wizard of one empty synced folder. Pre-open writes no `.agentic-open` marker, so both create, sync keeps one `company.yaml`, and the other Mac's running process is left with a foreign id.

**Tightening (AD-32):**
- Pre-open takes a lock keyed by root identity in the runtime base (`RuntimeLayout`) and holds it into the open. A second process refuses and names the holder.
- Every decision and every create re-reads the refusal reason and `company.yaml`'s sha256 under the base lock and refuses if either changed.
- Pre-open on a folder without `company.yaml` writes the marker first, and the wizard refuses to create while another Mac's marker is present.

## A11 (M): Identity tools have no test seam and no Linux behaviour that keeps CI green

**Unit A:** the identity adapter story. It follows AGENTS.md/AD-47, so it is `#[cfg(target_os = "macos")]` with a non-macOS branch that returns "not supported on this platform yet". It also follows the amendment: "no id is ever guessed or filled from a fallback".
**Unit B:** story 1.13's verify: "am-app serve tests with HOME set to a temp folder … `just check` is green on macOS and the Linux job". AD-32 says the off-macOS runtime base exists "so the core runs in Linux CI".
**Both obey:** AD-32, AD-33 and AD-47.

**Clash.** Every open is refused on Linux, so 1.13's tests cannot pass there. A builder who adds a Linux source (`/etc/machine-id`, `st_dev`) meets runners that have neither a stable value nor a volume UUID. A test fake reads like the "fallback" the owner banned.

**Tightening:**
- Identity is a port in `am-domain` (`HostIdentity`, `RootIdentity`), with `FakeIdentity` in `am-testkit` for every non-macOS test. A test fake is not a fallback.
- The production non-macOS adapter returns "not supported".
- Name the macOS file-id source (`getattrlist` `ATTR_CMN_FILEID`, or `stat` `st_ino` on APFS) in `docs/crate-notes` or the AD.

## A12 (M): A decision's audit is lost when the open fails after its side effect, and a clean close removes another Mac's marker

**Unit A:** an open that applies Open here anyway by rewriting `.agentic-open`, then opens the store. The open may fail after that: migration error, newer database, legacy refusal.
**Unit B:** the audit rule: "audited … in the store the open then opens, in that open's first transaction". Also AD-7: migrations each commit their own audited transaction first, so "first transaction" cannot hold.
**Both obey:** AD-7, AD-25 and AD-32.

**Clash.** If the open fails after the marker rewrite, the other Mac's marker is gone and no audit exists. Separately, the other Mac's clean close "removes" `.agentic-open` without checking whose it is, which deletes this Mac's marker and lets a third Mac open freely.

**Tightening:**
- The decision is a typed `OpenDecision` passed into the open. Its audit commits immediately after migrations and before any project-folder side effect, and the marker is written last.
- On failure, the previous marker bytes are restored.
- A clean close removes the marker only when the host id, pid and opened-at are its own. Compare markers by host id only; the Mac name is for display, and a renamed Mac is not "another Mac".

## A13 (M): ConfigCreate's "empty grant set" is undefined for caps and conflicts with the shipped-template exception

**Unit A:** an `is_widening` story that treats the empty set's caps as 0. Then any cap in a created `company.yaml` or seat is "raising a cap" and widening. That needs a confirm token, which the server "issues only over the live WebSocket", and the pre-open API serves only "status and the wizard".
**Unit B:** a story that treats absent caps as unlimited, so writing any cap is lowering.
**Both obey:** AD-15 and AD-16.

**Clash.** Under Unit A the wizard's create is either blocked or needs a WebSocket in pre-open. A seat created from a shipped role template through the add-seat popover is also a `ConfigCreate`, which AD-15 classifies against the empty set as widening. AD-15 says that seat is not widening for its template capabilities. Separately, AD-16 says "nothing else creates a project file outside a Proposal apply" but never says whether Proposal applies create files through `ConfigCreate` too. That leaves two creation paths.

**Tightening (AD-15, AD-16):**
- Define the empty set per kind: no tools, hosts, folders, Accounts or MCP; caps absent, which means the shipped defaults; and a created cap at or below the default is not widening.
- State that the shipped-role-template exception applies to `ConfigCreate`.
- State that Proposal applies use `ConfigCreate` for new files, so there is one primitive.
- State that the pre-open API serves the WebSocket only if a widening create is possible. Otherwise the wizard's step-1 file is fixed to carry no widening field.

## A14 (M): The kept legacy folder can be hollowed, mis-moded or pruned by the app

**Unit A:** the legacy mover. It renames on "the same volume", and a File Provider root under `~/Library/CloudStorage/` shares `st_dev` with `~/Library/Application Support`.
**Unit B:** startup hygiene in the same runtime folder:
- AD-13 refuses to start unless the runtime folder is 0700 with files 0600;
- AD-32 "stale ones are removed at start" for plaintext backup temps;
- AD-7 keeps "only the three newest" pre-migration backups.

**Both obey:** AD-7, AD-13 and AD-32.

**Clashes.**
1. Renaming out of a File Provider domain is a delete to the sync client. Evicted (dataless) items may not materialise outside the domain, so the "kept" folder holds stubs. A synced folder's files arrive 0644, so a recursive mode check refuses every later start, while a top-level-only check leaves world-readable state.
2. A recursive temp sweep or retention glob deletes files inside `legacy-agentic-*/`, which breaks "the app never deletes it".
3. Keeping the folder doubles disk use. That is acceptable, but the open should say it.

**Tightening:**
- A root inside `~/Library/CloudStorage/` or `~/Library/Mobile Documents/` (the AD-30 roots) always takes the copy-verify-remove path.
- The kept folder is chmodded 0700 and its files 0600 after the move.
- `RuntimeLayout` exempts `legacy-agentic-*` from every sweep, retention and mode-repair walk; the mode check still covers it.
- The date is the UTC date.

## A15 (M): Wizard progress has no owner

**Unit A:** a UI story that keeps "quitting resumes at the same step" in UI preferences in the app data dir (AD-32 allows "UI preferences" there).
**Unit B:** an engine story that keeps it in the project's store, as runtime state under AD-35.
**Both obey:** AD-32 and AD-35. Neither places this entity.

**Clash.** After step 1 the project exists, so two Macs, or two projects, resume each other's step.

**Tightening:** AD-35 places wizard progress in the store, as a row written by `Actor::Human` per step. Before the create there is nothing to resume.

## A16 (M): A volume with no UUID never opens

AD-32 refuses whenever `diskutil` returns no volume UUID. That is the case for network shares (SMB/AFP) and may be the case for some FAT or exFAT media. Such a project can never open, and the pre-open state offers no action. Either state that this is v1 scope ("projects on network volumes are not supported", worded in the refusal), or name a second identity source, such as the folder's own file id plus a `RuntimeLayout` stamp file, and get an owner decision. Do not substitute a value silently.

## A17 (L): Smaller gaps

- **Base lock held across identity tools.** The base lock is held while `diskutil`, `ioreg` and `scutil` run with their timeouts, which blocks every other project's open on the Mac. Read identity first, then take the lock and re-check.
- **"Open's first transaction".** It conflicts with AD-7's audited migration transactions (A12). Say "first transaction after migrations".
- **Sandbox self-test.** AD-13 runs it "at start" and the pre-open state starts "no … child". Say the self-test runs at the hand-over to open (A5).
- **No-replace rename.** `ConfigCreate`'s no-replace rename needs a named primitive: `renamex_np(RENAME_EXCL)` on macOS, and `renameat2(RENAME_NOREPLACE)` with a `link`+`unlink` fallback where the filesystem refuses it (Linux CI overlayfs). Put it in `docs/crate-notes` or the AD.
- **Marker and `.gitignore`.** The marker is written before the Blank template's `.gitignore` exists (5.16 step 1 comes before 5.12's template), so a git-tracked project can commit `.agentic-open`. The wizard's create should also write the `.gitignore` line, or keep the marker away until the template is applied.
- **AD-15 Binds.** These do not list `am-config`, although `is_widening` and `ConfigCreate` classification live there. Add it.

---

## Data-loss paths found

| Path | Finding | Loss |
|---|---|---|
| Ticket 1.13 removes the legacy folder | A3 | Old runtime state deleted, against the owner's decision |
| Crash between legacy copy and move-out, then a hand delete | A4 | Wrong folder deleted by the owner, the only way out of a no-action refusal |
| Copy misread as a move (path lookup or a dataless `company.yaml`) | A8 | Original's history bound to the copy; Treat as new on the original orphans it |
| Stale pre-open decision in a second process | A10 | Live id overwritten; the running project's state orphaned |
| `project_id` deleted from an original | A9 | Silent fork; ledger caps reset; History, Inbox and schedules orphaned |
| Confirm move on a changed id | A7 | Project bound to another project's state, or to an empty folder |
| Rename out of a File Provider domain; recursive sweeps | A14 | "Kept" folder hollowed or pruned |

## Pairs used

| Unit A | Unit B | Findings |
|---|---|---|
| am-config writer (story 1.8, built) | am-app resolver (story 1.13) | A1, A9, A13 |
| epic 5 story 16 wizard (web + am-server) | am-config/am-store create baseline | A2, A3, A13, A15 |
| am-server pre-open API | am-app composition root | A5, A6, A10 |
| am-app resolver | am-store runtime-folder record | A4, A7, A8, A12 |
| am-app identity adapter | Linux CI job and am-testkit | A11, A16 |
| legacy mover | startup hygiene (AD-7, AD-13 checks) | A14 |
