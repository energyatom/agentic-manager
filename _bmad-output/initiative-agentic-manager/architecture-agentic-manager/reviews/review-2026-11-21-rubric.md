# Rubric review: Sofia REPLAN amendments of 2026-11-21

Reviewed: `architecture-agentic-manager.md` (working tree, uncommitted), only the passages marked "amended 2026-11-21". These are AD-15's "Project-open decisions" and `ConfigCreate` classification sentences; AD-16's `ConfigCreate` and baseline sentences; AD-32's Binds, "Creating a project", "Identity comes only from the OS", the copy wording, the Confirm move wording, "Legacy folder" and "Pre-open state"; and the Structural Seed `legacy-agentic-<YYYY-MM-DD>/` line. I judged them against the last seven `.memlog.md` entries. For interaction I read the rest of AD-7, AD-13, AD-15, AD-16, AD-25 and AD-32, plus epic 1 story 13 and epic 5 stories 16 and 19 in the tickets. I also read the built code: `crates/am-store/src/project.rs`, `crates/am-config/src/writer.rs` (story 1.8) and `crates/am-app/src/main.rs`. This is a read-only review; nothing else was edited.

## Verdict

**Revise before story 1.13 is built, and before epic 5 stories 16 and 19 are planned.** The amendments carry all seven owner decisions faithfully, and the tickets restate them. But the rules leave several build orders and failure paths open, so independent stories could diverge:

- One rule contradicts the AD-16 apply order and would silently approve a copied project's grants (C1).
- The order of side effects for a pre-open decision is open, and so is what happens when the decision fails (H1).
- Project creation has no concurrency or rollback rule, so a failed or racing create can poison the root index (H2).
- The legacy move has no crash or resume rule, so a crash can strand a project behind a refusal that offers no action (H3).
- When the original's volume is unmounted, a copy is indistinguishable from a move, and the decision page offers only one action (H4).

The Medium and Low findings can be fixed in the same edit.

## Findings

### Critical

**C1. AD-16 apply order vs the Treat-as-new and System-mint sentences: a `project_id` write would commit an approved grant set and approve every copied grant without the two-step confirmation.**
- Location: AD-16 Rule. The "Apply order" sentence says a `ConfigEdit` "then commits History, the audit entries and the approved grant set in one transaction". The "store persists the approved grant set … each time an apply commits" sentence says the same. Against these, the amended clause says: "a copy given its own id by Treat as new project has none, so its first load holds". The pre-existing AD-32 Lifecycle path writes `project_id` "with a `ConfigEdit` by `Actor::System`" for a project opened for the first time on this Mac. That project also has no baseline.
- Why it matters: Treat as new project is "one `ConfigEdit` by `Actor::Human`" in a fresh store (AD-32 Pre-open state). Built as written, that edit's commit persists the snapshot's grant set as approved. The same happens for story 1.13's System mint. The first load then finds nothing widening, and the H4 control is skipped in exactly the copy and first-open cases it exists for. Epic 4 story 18, which builds the approved grant set, will read the apply order. Epic 5 story 19, whose verify expects the hold, will read the amended clause. The two stories conflict.
- Fix: in AD-16, add: "An apply in a runtime folder with no approved grant set never creates one. Only three things create the first approved grant set: the wizard's `ConfigCreate` of `company.yaml`, a confirmed first-load hold, and a confirmed Reload. The `project_id` writes (the System mint and Treat as new project) commit History and audit only." Also say that an apply updates an existing set only by that apply's own confirmed changes, never by re-reading the whole snapshot.

### High

**H1. AD-32 Pre-open state: "audited … in that open's first transaction" is ill-defined. The order of the decision's side effects, and the failure path after a decision, are silent.**
- Location: AD-32 Pre-open state, the sentence "Each is a Human command … audited … in the store the open then opens, in that open's first transaction". Interacts with AD-7 (each migration step is its own transaction with its audit entry), AD-16 (a `ConfigEdit` commits its intent row first) and `am-app/src/main.rs` (open, then the writer's reconcile, which writes audit entries).
- Why it matters: when a store is opened, migrations commit first, and the reconcile's dropped or completed intents may commit before the decision. For Treat as new project, the `ConfigEdit` intent row commits before anything else. So "first transaction" cannot be met literally, and each story will choose differently. The order of the decision's side effects is also unstated. Those side effects are: replacing the `.agentic-open` marker, rewriting the root index and the runtime folder's root record, and writing `project_id`. Suppose a decision is applied and the open then fails, for example on a database newer than the binary, a failed migration or a crash. The marker may already have been taken over, or the root index re-pointed, with no audit entry anywhere. Two sources of truth for the root (the base's root index and the runtime folder's own record) can then disagree.
- Fix: state one sequence under the runtime base's lock:
  1. Open and migrate the store.
  2. Commit the decision's audit entry. It is the first transaction after the migrations, before the reconcile, the config load or any other command. For Treat as new project, it goes in the same transaction as the `ConfigEdit` intent row.
  3. Only then apply the side effects: the marker, the runtime folder's root record first, then the root index, then `company.yaml` through the writer.
  4. If any step before the audit commit fails, nothing on disk changes and the pre-open state shows the failure in words.
  5. The runtime folder's own root record is the truth. At startup, the root index is rebuilt from it when they disagree.

**H2. AD-32 Creating a project and Treat as new project: no rule for concurrency or rollback. A failed or racing create leaves an orphan runtime folder and a root-index entry that later refuses the real project.**
- Location: AD-32 "Creating a project": "the same order under the base's lock (mint …, open the store), ending with a `ConfigCreate`". AD-16: the `ConfigCreate` is "refused if the file exists by then". AD-32 Pre-open state: Treat as new project "mints a new `project_id` in the order above".
- Why it matters: in the pre-open state, no store is open, so no project lock is held. Two `agentic serve` processes can both sit in the pre-open state for one empty folder (two browsers, or the LaunchAgent plus a manual serve), and both can submit the wizard. The rule does not say whether the `ConfigCreate` runs inside the base lock. It also does not say whether `company.yaml`'s absence is re-checked under the lock before minting. The no-replace rename catches the second create only after it has minted an id, created a runtime folder and recorded the root. The root index then maps the root to the losing id. On the winner's next open, "a root whose index entry names a different id" is refused until Confirm move. Any other `ConfigCreate` failure has the same effect: invalid input, disk full, or a validation error. The same applies to Treat as new project if its `ConfigEdit` fails.
- Fix: say that the whole create runs inside the base lock. Re-check under that lock that `company.yaml` is absent (for Treat as new project: that the copy's id is unchanged) before minting. Record the root in the index only after the `ConfigCreate` or `ConfigEdit` has renamed the file. On any failure, remove the new runtime folder and leave the index untouched, so the refusal leaves the folder exactly as it was. Also, a pre-open server holds a per-root lock in the runtime base, keyed by the root's identity. A second serve on the same folder, including one with `--take-over` or `--confirm-move`, is then refused, naming the holder.

**H3. AD-32 Legacy folder: no crash or resume rule. A crash or a partial removal leaves "a legacy folder beside runtime state", which is a refusal with no action, permanently.**
- Location: AD-32 "Legacy folder" ("by rename on the same volume, or else by a copy checked by sha256 after which the project's copy is removed"). The same paragraph says "when both a legacy folder and a runtime folder holding a `state.db` exist for one project, the open is refused naming both". The Pre-open state lists "a legacy folder beside runtime state" as a refusal with no action.
- Why it matters: the move has three steps, and each can fail between the others:
  - After `VACUUM INTO` has made the runtime `state.db`, the legacy folder is still in the project.
  - The cross-volume copy into `legacy-agentic-<date>/` can fail before it completes.
  - Removing the project's copy can be partial. A file may be locked by sync, a sync client may re-materialise it, or there may be a permissions error.

  In all three cases the next open sees both folders and refuses forever, and the owner has no action in the product. "No terminal in the product" then breaks on the first interrupted migration.

  Three more gaps:
  - A same-volume `rename(2)` out of a File Provider domain (OneDrive, iCloud) of a folder that holds dataless files can fail or force materialisation. The rule never says that such a failure falls back to the copy path.
  - Files copied from a synced folder may arrive 0644. AD-13 refuses to start unless the runtime folder and its files are 0700 and 0600, so the kept folder can make the server refuse to start for good.
  - The rule never says whether `<YYYY-MM-DD>` is a UTC or a local date.
- Fix: before the first copy, write a move journal in the runtime folder: `legacy-move.json` (named in `RuntimeLayout`), holding the source, the target and a sha256 manifest. On open, an existing journal is resumed rather than refused:
  - If the target is complete and verified against the manifest, finish removing the project's copy. Only files whose hash matches the manifest are removed, and any other file is refused, naming it.
  - Otherwise, discard the partial target and the runtime `state.db` the journal created, then start again.

  "Legacy beside runtime state" stays a refusal only when no journal exists. Treat any rename failure (`EXDEV`, `EPERM`, a File Provider error) as "use the copy path". Set 0700 and 0600 on the kept folder after the move. Use the UTC date. Have `RuntimeLayout` own the legacy source path `<project>/.agentic` too, because of the CI literal check.

**H4. AD-32 copy, move and Confirm move: when the original's volume is not mounted, a copy is indistinguishable from a move, and the page offers only one action.**
- Location: AD-32, the copy sentence ("another root that is mounted and still holds that id"), the Confirm move sentence ("a recorded root on a volume that is not mounted … is refused … until the owner confirms the move"), and Pre-open state ("offers one action").
- Why it matters: a common case is an original on an external drive and a copy made on the laptop. With the drive unplugged, the copy is classified as an unmounted-volume move and offered only Confirm move. The owner confirms, and the root index moves to the copy. When the drive is next mounted, the original is refused as a copy of its own copy and must be given a new id. Story 1.13 (the flags) and story 19 (the page) both implement "one action", and the owner cannot recover the intended identity.
- Fix: for an unmounted recorded volume, offer both **Confirm move** and **Treat as new project**, with the default keeping the project closed. The words should say that the original is on a volume that is not connected. The CLI gets the same choice: `--confirm-move` plus the worded copy fix. Also say what Confirm move does for a changed `project_id` whose new id has no runtime folder: either refuse it, or treat it as the mint path. Today the rule is silent on this.

### Medium

**M1. AD-32 Pre-open state: the API, WebSocket and auth surface is underspecified.**
- "The API answers only status and the wizard or the decision" leaves several things open:
  - The error envelope (AD-14 `code`) and the HTTP status for every other route.
  - Whether the WebSocket stream is served. The AD-15 confirm token "is issued only over the live WebSocket", so a wizard `ConfigCreate` that carries any grant (classified as widening, AD-15) cannot be confirmed without it.
  - Where `seq` comes from with no store, and how a client re-syncs after the open.
  - Whether the transition to an open project happens in-process with sessions kept, as story 16's verify implies, or by a restart.
  - Whether the CLI token and run file are written in the pre-open state.
  - What "the served folder" is when `agentic serve` runs without `--project` (the built `main.rs` opens nothing).
- Fix: list the pre-open routes, with all others answering `project_not_open`. Say the WebSocket is served for status events and confirm tokens, `seq` resets when the store opens, and clients reconnect without `since`. Say the transition is in-process and keeps the session. Define "served folder" as `--project`, or the folder the desktop app picks.

**M2. AD-15 Project-open decisions: the CLI decision path is undefined against the AD-15 and AD-13 owner-uid-child rule.**
- `--take-over` and `--confirm-move` are `serve` flags in a new process, not CLI-token API commands, so the "CLI class" wording does not map onto them. AD-15 moves CLI decisions to the UI "once a stdio MCP server or a Source with `render: true` is enabled". The pre-open state has no snapshot, though. One story will read the config to apply that gate and another will skip it. An owner-uid child can supply a TTY by running its command under `script`. A forced take-over runs schedules on two Macs at once.
- Fix: say whether the gate applies. Recommended: it applies, with the project's config read through am-config without opening the store; when the gate is on, the flags are refused, naming the UI. Also say whether the flags prompt on the TTY with a default of no.

**M3. AD-15 and AD-16 `ConfigCreate`: its scope and classification collide with the shipped-template rule, and "empty grant set" is undefined for caps.**
- "nothing else creates a project file outside a Proposal apply" does not say whether a Proposal apply creates its new files, such as a new seat, by `ConfigCreate`.
  - If it does, "a created file that carries any grant is widening" contradicts AD-15's rule that a seat from a role template shipped inside the signed app is not widening for its template capabilities.
  - If it does not, there is a second file-creation path.
- The `.agentic-open` marker and the Blank template's `.gitignore` are project files created outside am-config, against "Only am-config reads or writes project files".
- "Classified against an empty grant set" does not say whether an absent cap counts as unlimited (so setting one narrows) or zero (so setting one widens).
- Fix: say that every new config file, including those from a Proposal apply, is a `ConfigCreate`, and that the shipped-template exception applies to it. Change "project file" to "config file", and name the marker as the one runtime file am-app writes. Define the empty grant set: every cap absent counts as unlimited, so setting a cap narrows; every list is empty.

**M4. AD-16 `ConfigCreate` "renamed without replacing": no mechanism is enforceable in the pinned stack.**
- `std` has no `renameat2`/`renamex_np`. Every crate has `#![forbid(unsafe_code)]`, and the Stack pins no `rustix`, `nix` or `libc` binding. The story 1.8 writer refuses any file that does not exist ("the writer edits existing files only").
- Fix: name the mechanism: `std::fs::hard_link(temp, target)`, which fails if the target exists, then remove the temp file and fsync the folder. Where the volume refuses hard links, the create is refused in words. Alternatively, add a pinned `rustix` row to the Stack. Also say whether missing parent folders are created.

**M5. AD-32 Identity comes only from the OS: the adapter is underspecified, and the built code conflicts with it.**
- The rule names only the macOS tools. AD-47's pattern (a non-macOS branch returning "not supported") would make every project open fail on the Linux CI job, while story 1.13 requires Linux to be green. Volumes with no volume UUID (SMB, AFP or NFS shares) are refused forever with no action. That is an unstated product consequence. The timeout value and the `scutil` key (`ComputerName` or `LocalHostName`) are open.
- The built `am-store/src/project.rs` `host_name()` falls back to `hostname`, `$HOSTNAME` and then "an unknown host". A story reusing it for the marker would break "no id … filled in from a fallback".
- Fix: make identity a port in `am-domain`, with a fake in am-testkit and named non-macOS sources (`/etc/machine-id`, the mount's filesystem id, `uname -n`). State that a network or UUID-less volume cannot hold a project in v1, with words saying so. Fix the timeout (for example 10 s) and the `scutil` key. Say that the lock record's host is display-only and comes from the same adapter.

**M6. AD-32 Legacy folder vs AD-25 and the Leftovers paragraph: the kept folder holds a plaintext copy of every row forever, and Doctor's removal contradicts "never deletes".**
- `legacy-agentic-<date>/state.db` keeps content that the owner later deletes, so AD-25's "deleting a row or run really deletes its content" no longer holds. The Leftovers paragraph's UI-class removal of a runtime folder deletes the kept folder inside it.
- Fix: the open report and Doctor say that the kept folder still holds data deleted later, and Doctor lists each kept folder with its size. Say whether Doctor's runtime-folder removal includes kept legacy folders; recommended: yes, as an explicit owner action that names them in the confirmation. Reword "never deletes" to "never deletes on its own".

**M7. AD-32 Pre-open state: staging and precedence are silent.**
- The rule says unconditionally that `agentic serve` "stays up instead of exiting". Story 1.13, however, refuses and exits until epic 5 stories 16 and 19.
- Several refusal reasons can apply at once, for example a copy that also carries another Mac's marker and a legacy folder. The rule gives no order.
- A copied `.agentic-open` that names *this* Mac with a live pid (the original still open) is neither "another Mac" nor "stale".
- An unparseable `company.yaml`, or one with a newer `schema_version`, is missing from the list of refusals with no action.
- Fix: add a staging clause in the style of AD-15's v0.1 note. Give the check order: identity tools, then the database or config schema version, then the legacy folder, then copy, move or id, then the marker. Say that a marker naming this Mac with a live pid whose lock is held elsewhere is refused, naming the holder. Add the unreadable or newer `company.yaml` to the refusals with no action.

### Low

- **L1.** AD-16: "commits the baseline in its own transaction" should say "in the `ConfigCreate`'s History transaction". A separate transaction can be lost to a crash and the next startup held for no reason.
- **L2.** AD-32: "The project is left holding only `.agentic-open`" reads as if config files go too. Say "the only runtime file left in the project is `.agentic-open`".
- **L3.** AD-32: with `--no-browser` (the LaunchAgent) the pre-open state waits silently while schedules stop. Say that the desktop app and Doctor show it, or record it as a UX follow-up for EXPERIENCE.md.
- **L4.** AD-15: "creating a project and Treat as new project write `company.yaml`, so they are UI class only". The CLI-launched System mint also writes `company.yaml`. Note that the System mint is not a decision, so the class rule does not apply to it. This keeps a reader from treating it as a contradiction.
- **L5.** Built code, for information: `am_store::open_project(root)` still joins `.agentic` onto the project root and writes a `.gitignore` there. This is expected to change in story 1.13 and is not an amendment defect.

## Checklist summary

| Check | Result |
|---|---|
| Fixes the real divergence points for story 1.13 (CLI open) | Partly. The identity, legacy and copy wording is good. Crash and resume (H3), unmounted copy (H4), identity sources off macOS (M5) and staging (M7) are open. |
| Fixes them for epic 5 (wizard, pre-open UI) | Partly. Side-effect order (H1), create concurrency (H2), API, WebSocket and confirm token (M1) and the template classification (M3) are open. |
| Each Rule enforceable | No-replace rename has no mechanism in the stack (M4); "first transaction" cannot be met literally (H1). |
| Contradictions with other ADs or built code | AD-16 apply order vs Treat as new (C1); AD-15 template rule vs `ConfigCreate` (M3); AD-25 deletion vs kept folder (M6); AD-13 modes vs kept folder (H3); `host_name()` fallback (M5). |
| Silent points two stories could build differently | Audit vs store-open order (H1); pre-open API and auth (M1, M2); cross-volume failure mid-way (H3); Treat as new vs the AD-16 baseline (C1). |
