---
title: "Move a project's Accounts to this Mac"
type: feature
ticket: 13
created: '2026-11-22'
status: built
baseline_revision: 'ec324e35a1b831e4a90faf4b4c90afd40bdb8931'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick, adversarial-migrate]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: []
deferred:
  - summary: >-
      `ConfigWriter::apply_migration` is public and accepts any `RenameAccount { old, new }` from a Human actor, so a later caller (API, Proposal apply) could re-point a project's references to an existing Account at another destination; the destination rule lives only in am-app's `check_renames`.
    evidence: |-
      Story 2.13 adversarial review, finding 5. Today only `accounts migrate` calls it, after `check_renames`. Epic 4 story 17's `is_widening` must classify a `RenameAccount` by comparing the old and new ids' destinations (AD-15: a rename is not widening only when it keeps each reference's destination).
    location: >-
      crates/am-config/src/writer.rs (apply_migration)
    severity: medium
---

<intent-contract>

## Intent

**Problem:** Since story 2.12, a project that still holds `models/accounts.yaml` (built by stories 2.1 and 2.3) fails `agentic validate`, and its Accounts cannot send. The owner needs one safe way to move those definitions to this Mac's user list without retyping keys, without losing the file, and with a way back (owner, 2026-11-22: "make accounts migrate non-destructive"; "(a) migrate should not ask for stored keys — carry over an existing confirmation for the same address if the project had one; otherwise migrate the Account as unconfirmed so it can't send until confirmed normally").

**Approach:** `agentic accounts migrate --project <path>` is the add path, run once per entry of the old file, plus one project-side `ConfigMigration`. Arch AD-15 *Migrate* and *Record*, and AD-17 *Old project files*.
- **Per entry it decides one of four outcomes:**
  - **Already present:** same id, same provider, scheme, host and port. Nothing is done.
  - **Clash:** same id with another destination. The whole command is refused unless `--as old=new` is given.
  - **New and carried over:** the project store's matching confirmation becomes a user-store `account_added` with `confirmation: migrated`.
  - **New and unconfirmed:** recorded as `account_listed`, so the Account cannot send until `accounts change` confirms it.
- **It makes no Keychain call at all.**
- **The user store's records commit first.** Then it writes a hash-checked backup and removes the project file in one `ConfigMigration`.
- **`--dry-run`** shows all of this and writes nothing.
- **`--rollback`** restores the file byte for byte from the latest backup. It keeps any `--as` renames, because each renamed reference still reaches its original destination.

## Boundaries & Constraints

**Always:**
- **Command shape:** `agentic accounts migrate --project <path> [--as old=new]... [--dry-run | --rollback]`. `--project` is required here, because `migrate` acts on one project; 2.12's "no `--project`" applies only to `add`, `remove`, `list` and `test`. `--dry-run` and `--rollback` together, or `--as` with either, is a usage error (exit 2).
- **Terminal and confirmation (AD-15 *Terminal*):** `migrate` and `--rollback` run only when stdin and stdout are a real interactive terminal, checked by 2.12's one predicate. Anything else is refused before anything is read or written.
  - **New Accounts:** each one shows the add's sentence: id, provider, endpoint, the references found in every reachable project, the projects not checked, and its outcome (carried over, or unconfirmed with the `agentic accounts change <id>` it then needs). The Account's id must be typed within 2 minutes. A wrong id or a timeout ends the command with nothing written.
  - **Already-present entries** show one line and need no typing.
  - **`--dry-run`** only reads. It takes no lock and runs from any stdin.
- **No key, ever:** `migrate`, `--dry-run` and `--rollback` make no `SecretStore` call; FakeSecretStore records none. No key is printed, stored or recorded. A key-looking value in the old file is never echoed. 2.12's membership hiding applies to every message about the old file too.
- **Opening the project (AD-15 *Migrate*, AD-32):** the project is resolved read-only through 2.12's `open_project_unchanged`, which mints nothing, migrates nothing and holds the project's store lock for the whole command.
  - **Refused when:** the project is held by `agentic serve`, `agentic service` or the desktop app (naming the holder); its config enables a stdio MCP server or a Source with `render: true`; or it has an outside change not yet reloaded (AD-16).
  - **The user side** is refused, as for `add`: on an outside change of the user Accounts file, while any reachable project runs a child as the owner, and on any 2.12 user-store or permission refusal.
  - **Lock order:** the project's store lock, then `user.lock`, taken only for the write.
  - **Changed since the sentence:** under `user.lock` the command re-reads the user file, the ids and their records, and the old file's bytes. If anything changed since the sentences were shown, it is refused with nothing written, to be run again (2.12's `Standing::still` rules).
- **Carry-over rule (AD-15 *Migrate*):** an entry is carried over only when all of these hold:
  - the project store is read-only and its audit chain verifies;
  - it holds a latest `account_added` for the id, with no `account_removed` after it;
  - that record names the entry's provider, scheme, host and port (a record naming no provider never matches);
  - the user store holds no record at all for the id;
  - the user store is not marked `recovered`.

  The carried record is a new user-store `account_added` with the same fields, `key: stored`, `confirmation: migrated`, the project id and the source entry's sequence number and hash, and never the key. The sentence says the key stored under the id, if one exists, will be used for that destination. Otherwise the Account is added unconfirmed: History, plus an `account_listed` audit entry (id, provider, scheme, host, port, references, `confirmation: none`), with no confirmation record. 2.12's Key-use check then refuses it before any Keychain read, naming `agentic accounts change <id>`.
- **The `recovered` mark (AD-15 *Migrate*):** a new user migration `0002` adds a one-row marks table. When am-app creates `user.db` while the user Accounts file or `backups/accounts-migrate/` already exists, it marks the store `recovered` in the creating transaction. A recovered store carries nothing over. A store created normally, including every store made before this story, is not marked.
- **Backup (AD-15 *Migrate*, AD-17):**
  - **Where:** `AppDataLayout::accounts_migrate_backups(project_id)` names `<app data dir>/backups/accounts-migrate/<project-id>/`, created 0700. Each run writes `<UTC timestamp>.yaml` (0600), holding the exact bytes it parsed, plus `<UTC timestamp>.manifest.yaml` (0600) listing each `--as` rename (old id, new id, and each file and line rewritten, with before and after hashes). Backups are never pruned.
  - **Hash check:** the copy is fsynced. Then the copy's sha256 and the current file's sha256 are both checked against the parsed bytes, and any difference removes nothing.
- **Order of writes (AD-15):**
  1. Under `user.lock`, one user-store transaction per Account: the user file write with History, then the `account_added` or `account_listed` entry. These commit first.
  2. Then the backup and its checks.
  3. Then one project `ConfigMigration`. It removes `models/accounts.yaml` and applies the `--as` renames, with a History entry per file and the `config.migrated` audit, through the project writer, committed in the project store.

  A crash between steps 1 and 3 leaves the project file to migrate again. The next run then finds the entries already present, including a renamed id with the same destination, and only removes.
- **Removal in the config writer (AD-16):** am-config's writer gains removal as a step of a `ConfigMigration`.
  - **Recording:** a new `HistoryKind::Removal` records `before` as the removed bytes and `after` as empty.
  - **Intent rule:** the intent is committed before the unlink, and reconcile treats "file absent" as written.
  - **Scope:** removal is reachable only through the migration path, and only for `models/accounts.yaml`. Any other file is refused, and no other caller can remove a file.
  - **Not widening:** a removal never widens (the legacy file grants nothing). The `ConfigMigration`'s renames keep each reference's destination, so it is not widening (AD-15).
- **`--as old=new` under AD-17's "same ids"** (rule decided here, owner 2026-11-22: "a clear rule for --as old=new under 'same ids' (2.13 is the natural home; decide it in the 2.13 plan)"):
  1. **Only on a real clash:** `old` must be an entry of this project's old file that clashes with the user list. Any other `--as` is refused, naming why.
  2. **The new id is the owner's:** `new` is typed by the owner and never minted. It must be kebab-case, not reserved, not already named anywhere in this project, and not on the user list. The one exception is a re-run after a crash: the user list then holds `new` with this entry's destination and it counts as present.
  3. **Renames stay in this project:** every reference to `old` in this project is renamed to `new` in the same `ConfigMigration`: a seat's `model` and `fallback`, a capability's provider and a Source's `account`. Other projects and templates are untouched and keep resolving `old` to the existing user Account.
  4. **The sentence says what changes:** it names each renamed line. It also says that a template, another project or another Mac naming `old` will reach the existing Account at its own destination, not this one, and that this project's exports will carry `new`.
  5. **The new Account starts unconfirmed:** it is added unconfirmed, because the Keychain entry under `old` belongs to the existing Account.
- **`--rollback` (AD-15 *Migrate*):** it has `migrate`'s terminal, typed-confirmation, project-lock and refusal rules. It uses the project's latest backup and manifest. In one project transaction with History it re-creates `models/accounts.yaml` byte for byte through a `ConfigCreate` (AD-16; not widening). It leaves alone:
  - `--as` renames. The project's references keep `new`, which keeps each reference's original destination. Reverting them would point the project at `old`, the other Account's destination.
  - the user list, the user store's records and the Keychain.

  The sentence says all of this (coordinator decision before any code, AD-15 as amended in the same replan).

  **Refused, writing nothing, when:**
  - the backup or manifest is missing or fails its hash;
  - the project's file exists again;
  - an id the restored file names is on the user list with another destination than the backup's entry. The ids the manifest lists as renamed (`old`) are exempt.
- **Tests first:** written to fail first, on `fixtures/projects/legacy-accounts/` (extended as below) with FakeSecretStore, canary keys and a temporary HOME per test. The red run is logged here before any `src/` change.

**Never:**
- **No key:** no Keychain call, key prompt, key comparison or key output.
- **No silent widening:** carry-over only on the exact match above.
- **No deletion beyond this file:** no removal of any file but `models/accounts.yaml`, and no removal path outside `ConfigMigration`.
- **No pruning:** no pruning of backups.
- **Outside this story:**
  - `accounts change` (story 14);
  - the UI path (epic 5 story 5);
  - the per-project destination record (epic 4 story 18).
- **No other changes:** no change to 2.12's `add`, `remove`, `list` and `test` beyond the shared helpers, and no new dependency.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Dry run | Legacy project with `cloud` (project record matches), `spare` (no record), `same` (already on the user list, same destination), `clash` (on the user list, other host); any stdin | Lists each with its outcome (carried over, unconfirmed, present, clash); every project file, the user file and `user.db` byte-identical; no lock taken; no Keychain call | — |
| Carry over | `migrate` without `clash` in the file; typed ids | `cloud`: user-store `account_added` with `confirmation: migrated`, project id, source seq and hash, `key: stored`; `accounts test cloud` then sends to its endpoint | — |
| Unconfirmed | Same run | `spare`: History and `account_listed`, no confirmation record; `accounts test spare` refused before any Keychain read, naming `accounts change spare` | — |
| No-provider or other-host record | Project record for the id names no provider, or another host or provider | Added unconfirmed | — |
| User store already knows the id | The user store holds any record for the id (e.g. an earlier `account_removed`) | Unconfirmed, never carried | — |
| Recovered store | `user.db` deleted while `accounts.yaml` remains; `migrate` creates it | Store marked `recovered`; nothing carried over | — |
| Backup | Any `migrate` | `<project-id>/<ts>.yaml` holds the parsed bytes, 0600 in 0700 folders; hashes checked before the file is removed | — |
| Backup mismatch | The project file changes between parse and removal | Nothing removed; refused, to run again | Worded |
| Order on crash | Fail point after the user-store commits, before removal | Project file still there; a re-run reports every entry present and only removes | — |
| Second run | `migrate` again after success | "Nothing to move"; nothing written | — |
| Clash | `clash` in the file, no `--as` | Whole command refused naming both destinations; nothing written | Worded |
| `--as` | `--as clash=clash-old-host` | `clash-old-host` added unconfirmed; every reference to `clash` in this project renamed in the same `ConfigMigration` with History; manifest lists them; other projects untouched; the sentence names the renamed lines and the template and other-Mac note | — |
| `--as` refused | `--as` for a non-clashing id; a new id reserved, non-kebab, already named in the project, or on the user list with another destination | Refused, naming the rule; nothing written | Worded |
| Rollback | `--rollback` after the `--as` run | File re-created byte for byte; renames kept (references still name `new`, same destination); the user list, `user.db` records and Keychain unchanged; History in the project | — |
| Rollback refused | No backup; backup hash wrong; file exists again; restored id (other than a renamed `old`) on the user list with another destination | Refused, nothing written | Worded |
| Not interactive | `migrate` or `--rollback` with a pipe on stdin | Refused, nothing read or written | — |
| Wrong typed id | A mistyped id at an entry's prompt | Command ends; nothing written | — |
| Project held or risky | `agentic serve` holds the project; or the project enables a stdio MCP server | Refused naming the holder or the project | Worded |
| Changed since sentence | Another process adds `spare` to the user list between the sentence and the lock | Refused, run again; nothing written | — |
| No key anywhere | Any of the above | FakeSecretStore records no call; the canary is in no file, row, History, audit or output | — |
| Removal guard | The writer is asked to remove any file other than `models/accounts.yaml`, or outside a `ConfigMigration` | Refused | — |
| Validate after | `agentic validate` on the migrated project with the user list | No legacy error; missing ids only as warnings | — |

</intent-contract>

## Code Map

- **`crates/am-app/src/accounts.rs`:** the `migrate` subcommand. It uses 2.12's helpers:
  - `user::{Standing, Locked, scan, shown}`;
  - the add sentence;
  - `key_use` for the dry-run's "present" test, which compares destinations only, never the key.
- **`crates/am-app/src/runtime.rs`:** `open_project_unchanged` (L1123) and `OpenedUnchanged { writer, store, layout, project_id, root }` (L1102). Add a read-only resolve for `--dry-run` that takes no lock: company `project_id` → runtime layout → `Reader::open(state.db)`.
- **`crates/am-app/src/user.rs`:** `Locked::take` marks `recovered` on create when the user file or the `accounts-migrate` backups exist.
- **`crates/am-domain/src/runtime.rs`:** `AppDataLayout::accounts_migrate_backups(&ProjectId)`.
- **`crates/am-domain/src/config_history.rs`:** `HistoryKind::Removal` (L39-66).
- **`crates/am-store`:**
  - `config_history.rs` (L150-163, L286): accept the removal kind, and the `config.migrated` audit for it;
  - `migrations/user/0002_user_marks.sql`;
  - `user.rs`: `mark_recovered`, `UserReader::is_recovered`;
  - `lib.rs`: `Reader::verify_audit_chain` and `config_note_args` for the project's `account_added` records.
- **`crates/am-config`:**
  - `writer.rs`: the migration path (around L780-890), plus a removal step and a rename step;
  - `account_edit.rs`: `account_uses` (L224) for the renames, and the legacy file's parse.
- **`fixtures/projects/legacy-accounts/`:** extend it to carry entries for each matrix case (`cloud`, `spare`, `same`, `clash`) and seat references to each. Project-store records are seeded by the tests.
- **Tests:**
  - new `crates/am-app/tests/accounts_migrate.rs`, reusing 2.12's harness (Kit, Env, FakeTerminal, CountingStore, `assert_no_key_leaked`);
  - am-config writer tests for removal;
  - am-store tests for the marks table and the removal kind.

## Tasks & Acceptance

**Execution:**
- [x] **Tests first (no `src/` change):** a test for every matrix row, plus the writer and store tests. Log the red run.
- [x] **am-domain:** `AppDataLayout::accounts_migrate_backups` and `HistoryKind::Removal`.
- [x] **am-store:** the removal kind, user migration 0002 and the `recovered` mark.
- [x] **am-config:** the removal and rename steps of the migration path, guarded to the legacy file.
- [x] **am-app:** `migrate`, `--dry-run`, `--as`, `--rollback`, the backup and manifest, the order of writes, the `recovered` mark on create.
- [x] **Fixtures:** the legacy project's cases.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the outcome, the files, both stores and the Keychain match the row.
- Given the workspace, when `just check` runs, then it is green.

## Review Triage Log

### 2026-11-22 — Review pass (quick lens and an adversarial migrate lens)
- verdicts: 19 findings — high 4, medium 11, low 3, false 0, maybe-false 0 (one rejected low)
- routing note: patches are applied tests first (red run before any `src/` change), as in story 2.12's review rounds.
- findings:
  - Quick 1 `[medium]` `[patch]` A renamed entry's `account_listed` takes the references to `old`; no reachable project is scanned for `new`; its sentence drops the references and the not-checked projects, and the lock re-read never checks `new`. Fix: scan, show and re-check `new` like any new id.
  - Quick 2 `[medium]` `[patch]` On a crash re-run, a renamed entry whose `new` is present still has its lines renamed with no sentence and no typed id; on a first run an unrelated `new` at the same destination is accepted silently. Fix: a `new` already on the list counts as present only when this project's latest manifest records that rename; the renamed lines are still shown and the id typed.
  - Quick 3 `[low]` `[patch]` Manifest hashes are wrong when two renames touch one file (each previewed on the unmodified disk). Fix: take the manifest from the sequential plan the writer applies.
  - Quick 4 `[high]` `[patch]` (group G-outside) An outside edit of a seat or tool file during the prompts is written as the app's own `migration` entry: `no_outside_change` runs only before the prompts, `apply_migration` re-plans from the current disk. Fix: under the locks re-run the outside-change check and pass each rename file's expected before-hash to `apply_migration`, which refuses on any difference.
  - Quick 5 `[medium]` `[patch]` (group G-dry) `--dry-run` prints raw ids and hosts and `endpoint_of`'s error text before any secret or error check. Fix: run the same secret and error check first; hide by membership; print no value of an entry with an error.
  - Quick 6 `[high]` `[patch]` (group G-removed) Real 2.1/2.3 project stores hold no `account_removed` (the project-era remove wrote no note), so a removed-then-restored id carries its stale confirmation. Fix: carry over only when every project History entry of `models/accounts.yaml` from the record's own entry to the latest one holds the id at the record's destination (an entry without it, a removal or a restore-from-elsewhere breaks continuity), in addition to the note rule.
  - Quick 7 `[medium]` `[patch]` (group G-mark) A crash between user migrations 0001 and 0002 on a store created under the recovery conditions leaves it unmarked. Fix: a fresh store runs every pending user migration and the mark in one transaction.
  - Quick 8 `[medium]` `[patch]` (group G-open) Reading the `recovered` mark turns any error into "not recovered", and project record reads drop errors per query. Fix: fail closed — a mark read error refuses the command; any record read error makes every entry unconfirmed.
  - Quick 9 `[medium]` `[patch]` Undo or Restore of a `removal` History entry writes an empty `models/accounts.yaml` or fails with a bare I/O error. Fix: refuse both in words naming `--rollback`.
  - Adversarial 1 `[medium]` `[patch]` (group G-mark) As Quick 7.
  - Adversarial 2 `[medium]` `[patch]` (group G-open) As Quick 8.
  - Adversarial 3 `[high]` `[patch]` (group G-removed) As Quick 6, plus the wiped-app-data case: a restored file after an earlier migrate's removal carries again. Covered by the History continuity rule (the removal entry breaks it).
  - Adversarial 4 `[high]` `[patch]` (group G-outside) As Quick 4; also `check_renames`' "not named in this project" is not re-checked under the locks. Fix includes re-running `check_renames` under the locks.
  - Adversarial 5 `[medium]` `[defer]` `apply_migration` is public with no destination rule; deferred to epic 4 story 17's classifier (frontmatter).
  - Adversarial 6 `[medium]` `[patch]` "Every step or none" does not hold: renames commit before the removal can fail. Fix: check every step (files, hashes, the removal file resolved and unchanged) before writing any; a re-run completes a partial migration (a rename whose `old` is no longer named is skipped); the doc says so.
  - Adversarial 7 `[low]` `[patch]` `remove_step` does not re-resolve the path before read and unlink. Fix: resolve again (no links) right before the read and the unlink.
  - Adversarial 8 `[medium]` `[patch]` A symlinked `models/accounts.yaml` (or `models/`) passes the reads but fails the removal after the user records commit, stranding every run. Fix: refuse a link before any user write.
  - Adversarial 9 `[medium]` `[patch]` (group G-dry) As Quick 5.
  - Adversarial 10 `[low]` `[patch]` Rollback prints the manifest's renames raw. Fix: show them through `shown()`. (Manifest tampering needs a write to the owner-only app data dir: same-user, outside the threat model; not fixed further.)
- fixes applied (2026-11-22, implementer re-engaged; tests first, red run logged):
  - **Quick 1:** the target id is scanned, shown, recorded and re-checked.
  - **Quick 2:** a `new` already on the list counts as present only when this project's latest manifest records the rename; the renamed lines are still shown and the id typed.
  - **Quick 3:** the manifest comes from the writer's sequential plan.
  - **G-outside:** a `MigrationStep::Holds` expected hash per file; the outside-change and `--as` checks run again under the locks.
  - **G-dry:** the dry run lists clean entries only; others are named by line, or by a valid non-key id.
  - **G-removed:** the History continuity rule, using `ConfigNote::entry_id`.
  - **G-mark:** a fresh marked store migrates in one transaction.
  - **G-open:** errors in the mark or the records fail closed.
  - **Quick 9:** Undo and Restore of a removal are refused, naming `--rollback`.
  - **Adversarial 6 and 7:** the old file is resolved again before the intent and before the unlink.
  - **Adversarial 8:** links are refused right after the project opens.
  - **Adversarial 10:** the rollback sentence goes through `shown()`.

  Coordinator accepted: an entry with an error may still be named by its id when that id is valid and not key-like. `just check` green, 912 tests.

### 2026-11-22 — Follow-up review pass (adversarial, on the review fixes)
- verdicts: 4 findings — high 0, medium 2, low 2. History continuity, the single-transaction fresh store, fail-closed reads and link refusals hold.
- findings:
  - `[medium]` `[patch]` An empty `Holds` set disables the outside-edit guard: with nothing naming `old` at prompt time, a file given `account: old` from outside before the apply is written as the app's own migration entry. Fix: every `RenameAccount` carries the exact expected set of files, even an empty one; `apply_migration` refuses a rename that would write a file outside it.
  - `[medium]` `[patch]` A failure or crash after the user records commit and before the manifest is written strands `--as` entries: the re-run finds no manifest and refuses `new`. Fix: accept as present a `new` whose user-store owner `account_listed` note names this project id and the entry's provider, scheme, host and port (the manifest stays the evidence for the renamed lines).
  - `[low]` `[patch]` `--dry-run` hides unclean entries by id, so a duplicate id's second entry prints its values (and `endpoint_of`'s refusal repeats a plain `base_url`). Fix: decide cleanliness per entry; an id appearing on any unclean entry shows no values.
  - `[low]` `[patch]` `remove_step` leaves a pending removal intent when the re-read fails. Fix: drop the intent on a read error too.
- follow-up fixes applied (tests first, red run logged):
  - `MigrationStep::Expects` carries the exact set of files with their hashes (an empty set counts), and `migrate` always sends it.
  - `listed_renames` accepts a `new` whose `account_listed` names this project and its destination.
  - The dry run judges cleanliness per entry and never passes endpoint refusal text through.
  - `remove_step` drops its intent when the re-read fails.

  The coordinator accepted `Expects` as one step per migration, not a field on each rename: same guard, and the existing `RenameAccount` shape is kept. `just check` passes, 917 tests.

## Plan Change Log

- 2026-11-22 (coordinator, before any code): `--rollback` keeps `--as` renames instead of reverting them. Reverting would point this project at `old`, which is on the user list at the other Account's destination, and AD-15's rollback refusal would refuse every such rollback anyway. The refusal now exempts the manifest's renamed ids. Recorded in AD-15 and ticket 13 in the same replan; the owner is told.

- 2026-11-22: approved by the owner as written ("Approve the 2.13 plan as written; go ahead (architect records the --as rule in AD-15 first, then tests first)."). Owner, the same day: Account commands treat an enabled loopback-HTTP MCP server like a stdio one; that consistency belongs to the story the architect names (not this one unless recorded here).

## Design Notes

**Decisions for the owner's approval:**
1. **The `--as` rule** (owner asked it be decided here; ticket 13 as restated in PR #18: once approved, an architect REPLAN records it in AD-15 *Migrate* before any test or code) is the five points under `--as` above. In short: only for a real clash; the new id is the owner's, never minted, and fresh in this project and on this Mac; renames stay inside this project; the sentence says other projects, templates and Macs keep reaching the existing Account under `old`; the new Account starts unconfirmed.
2. **Removal is a `ConfigMigration` step and nothing else.** A new `HistoryKind::Removal` records the removed bytes as `before`. It is limited to `models/accounts.yaml`, so the writer still cannot delete any other file.
3. **The `recovered` mark lives in a new user migration**, `0002` (a one-row marks table). This also exercises the user store's pre-migration backup, which 2.12 could not test. Stores made before this story count as not recovered.
4. **Each new Account's id is typed, one by one.** The AD gives each new entry the add's sentence and typed id, so a file of N new entries asks N times. Entries already present need nothing.
5. **`--dry-run` takes no lock and runs from any stdin.** It reads the project store and the user store read-only, so it can be run by a script or an agent without risk.

**Size:** medium to large. Three new paths (`migrate`, `--rollback`, `--dry-run`), one writer step, one store migration, about 25 matrix rows.

## Verification

- `cargo nextest run -p am-app --test accounts_migrate` and the am-config and am-store tests, then `just check`.
- The canary scans are the tests' own (`assert_no_key_leaked` over the temporary HOME, both stores and the output).

## Implementation Notes

### Red run (2026-11-22, tests first, no `src/` change)

**Tests written** (outside every `src/` folder):
- `crates/am-app/tests/accounts_migrate.rs` (new, 26 tests): one test per matrix row — dry run (outcomes, nothing written, no lock), carry over, unconfirmed, no-provider / other-host / other-provider / removed record, user store already knows the id, recovered store (lost `user.db`, fresh Mac, backups present), backup (bytes, 0600 in 0700, manifest), backup mismatch, order on crash, second run, clash, `--as`, `--as` refused, usage (exit 2), rollback (byte for byte, renames kept), backups never pruned, rollback refused (no backup, changed backup, missing manifest, file back, restored id elsewhere), not interactive (in process and the real binary with piped stdin; `--dry-run` from a pipe runs), wrong or late typed id, project held or risky (holder named, stdio MCP in this or another project, outside change of a project file or the user file, never-opened folder), changed since the sentence, no key anywhere, key-looking value in the old file, validate after.
- `crates/am-config/tests/accounts_migration.rs` (new, 9 tests): removal as a `ConfigMigration` step (`removal` entry, before = bytes, after empty), changed file refused, the removal guard (no other path or file, owner only, the user writer removes nothing, an edit never removes), intent before the unlink and reconcile ("file absent" is written), rename of model / fallback / capability provider / Source `account` (bodies byte for byte), all or nothing.
- `crates/am-store/tests/accounts_migrate.rs` (new, 7 tests): user migration 0002 (version 2), `recovered` mark set and read, a store created normally not marked, a version-1 store backed up before migrating and not marked, the `removal` kind recorded and audited `config.migrated`, non-removals refused, reconcile of a removal intent.
- `crates/am-domain/tests/history_kind.rs` (new): `HistoryKind::Removal` stored as `removal`; `crates/am-domain/tests/app_data_layout.rs` (older test rewritten: the "every path inside the app data dir" list now includes `accounts_migrate_backups`; one new test for its path).
- Fixture: `fixtures/projects/legacy-accounts/` extended — the old file now holds `cloud`, `spare`, `same` (openai-compatible at `same.example`) and `clash` (openai-compatible at `old-host.example`); new seats `beta.md` (model `clash`, fallback `spare`) and `gamma.md` (model `same`, fallback `clash`); `alpha.md` unchanged so the older tests' line numbers hold. The older tests that read this fixture (`am-config` `user_accounts`, `am-e2e` `cli_validate`, `am-app` `user_accounts`) were run after the change: 88 passed.

**API and wording the tests fix** (engineering choices under the owner's standing rules; the coordinator may revisit before code):
- am-config: `ConfigMigration { steps: Vec<MigrationStep> }`, `MigrationStep::Remove { file, before_sha256 }`, `MigrationStep::RenameAccount { old, new }`, `ConfigWriter::apply_migration(Actor, ConfigMigration) -> Result<Vec<HistoryEntry>, WriteError>` (owner only, all or none); removal reuses `FailPoint::BeforeRename` / `AfterRename` as before / after the unlink.
- am-store: `UserStore::mark_recovered()`, `UserReader::is_recovered()`; a removal audits `config.migrated` with `kind: removal`.
- am-app: `am_app::runtime::testing::before_backup_check(root, meanwhile)` (test-support only, beside `fail_writer_at`) for the backup-mismatch row; the carried `account_added` names its source with `project`, `source_seq`, `source_hash`; outcome words `carried over`, `unconfirmed`, `present`, `clash`, and `nothing to move`; new Accounts are asked in file order (the renamed one by its new id), echoed; **`--rollback`'s typed confirmation is the word `rollback`** (the plan says "migrate's typed-confirmation rule" but rollback adds no Account, so there is no id to type — flagged for the coordinator); the manifest records the backup's own sha256 so a changed backup fails its hash.
- The setup helpers were checked once against the current code with temporary stubs for the missing API (a scratch test file, deleted): every setup step runs, and each test then fails at the `migrate` command's outcome, not in its setup.

**Command:**

```
cargo nextest run -p am-domain -p am-store -p am-config -p am-app --test app_data_layout --test history_kind --test accounts_migrate --test accounts_migration
```

**Output (exit 101; the build of every new or rewritten test target fails on the missing API):**

```
error[E0599]: no variant, associated function, or constant named `Removal` found for enum `HistoryKind` in the current scope
  --> crates/am-domain/tests/history_kind.rs:13:29
error[E0599]: no method named `accounts_migrate_backups` found for struct `AppDataLayout` in the current scope
  --> crates/am-domain/tests/app_data_layout.rs:48:16
error: could not compile `am-domain` (test "history_kind") due to 3 previous errors
error: could not compile `am-domain` (test "app_data_layout") due to 7 previous errors
error[E0599]: no method named `is_recovered` found for struct `UserReader` in the current scope
   --> crates/am-store/tests/accounts_migrate.rs:106:49
error[E0599]: no method named `mark_recovered` found for struct `UserStore` in the current scope
   --> crates/am-store/tests/accounts_migrate.rs:119:11
error[E0599]: no variant, associated function, or constant named `Removal` found for enum `HistoryKind` in the current scope
   --> crates/am-store/tests/accounts_migrate.rs:196:28
error: could not compile `am-store` (test "accounts_migrate") due to 11 previous errors
error[E0432]: unresolved imports `am_config::ConfigMigration`, `am_config::MigrationStep`
  --> crates/am-config/tests/accounts_migration.rs:39:5
error[E0599]: no method named `apply_migration` found for struct `ConfigWriter` in the current scope
   --> crates/am-config/tests/accounts_migration.rs:130:10
error: could not compile `am-config` (test "accounts_migration") due to 16 previous errors
error[E0425]: cannot find function `before_backup_check` in module `am_app::runtime::testing`
    --> crates/am-app/tests/accounts_migrate.rs:1172:31
error[E0599]: no method named `accounts_migrate_backups` found for struct `AppDataLayout` in the current scope
error[E0599]: no method named `is_recovered` found for struct `UserReader` in the current scope
error: could not compile `am-app` (test "accounts_migrate") due to 7 previous errors
error: command `cargo test --no-run --package=am-domain --package=am-store --package=am-config --package=am-app --test=app_data_layout --test=history_kind --test=accounts_migrate --test=accounts_migration` exited with code 101
```

Failing tests (all, by compile error): am-domain `history_kind` (2), `app_data_layout` (4); am-store `accounts_migrate` (7); am-config `accounts_migration` (9); am-app `accounts_migrate` (26). With the temporary stubs, the am-app tests failed at the command (`accounts migrate` is not a command yet: exit 2 where 0 or a worded refusal is expected), e.g. `dry_run_lists_every_outcome_and_writes_nothing` at `assert_eq!(ran.code, 0, …)`; `a_held_or_risky_project_is_refused_naming_why` at `assert!(ran.all().contains("Holder Mac"))`; `migrate_usage_errors_exit_two` at the usage naming `agentic accounts migrate --project`.

### Choices fixed by the tests (2026-11-22, coordinator accepted at the gate)

- **`--rollback`'s typed confirmation is the word `rollback`.** A rollback adds no Account, so there is no id to type; it still needs the real terminal and the typed word within 2 minutes.
- **The am-config API:**
  - `ConfigMigration { steps }`, with `MigrationStep::Remove { file, before_sha256 }` and `MigrationStep::RenameAccount { old, new }`;
  - applied by `ConfigWriter::apply_migration(Actor, ConfigMigration)`, which writes every step or none;
  - removal reuses `FailPoint::BeforeRename` and `AfterRename` as the points before and after the unlink.
- **The test hook `am_app::runtime::testing::before_backup_check`** is test-support only, like `fail_writer_at`.
- **Record fields:** the carried `account_added` has `project`, `source_seq` and `source_hash`. `account_listed` uses `account_added`'s keys with `confirmation: none`.
- **Outcome words:** `carried over`, `unconfirmed`, `present`, `clash` and `nothing to move`.
- **The manifest** records the backup's sha256.
- **Not covered by a test:** a broken project audit chain blocking carry-over. am-app's tests cannot tamper with the chain without a new dev-dependency. The implementer reuses `Reader::verify_audit_chain`, and the review checks it.

### Build (2026-11-22, after the red run)

Every test of the red run passes unchanged; `just check` is green (see the note on the target folder below).

- **am-domain:** `HistoryKind::Removal` (`removal`); `AppDataLayout::accounts_migrate_backups(&ProjectId)` and `accounts_migrate_backups_root()` (the folder whose presence marks a new user store `recovered`).
- **am-store:** user migration `0002_user_marks.sql` (one row, `recovered` never cleared, triggers refuse a delete or an unmark); `UserStore::locked_marking_new(layout, wait, recovered_if_new)` commits the mark in the creating (last migration) transaction through a new `on_create` hook of the migrator; `UserStore::mark_recovered` / `is_recovered`, `UserReader::is_recovered` / `is_created`; setting the mark is audited `store.marked_recovered` (System, no args). The `removal` kind is accepted only with `before` non-empty, `after` empty and no `ref_entry`, audited `config.migrated`. `ConfigNote` gains `entry_hash` (the carried record's `source_hash`).
- **am-config:** new `account_migration` module: `ConfigMigration`, `MigrationStep`, `RenamedFile`, `account_renames` (preview, used for the sentence and the manifest), `legacy_accounts` (the old file read by the user-file loader, values hidden, secret lines). `ConfigWriter::apply_migration` (owner only, never in user mode, renames first as `migration` entries and the removal last, so a crash never leaves the file gone while references still name `old`; whole-project check before any write); `remove_step` (intent, re-read, unlink, folder fsync; `FailPoint::BeforeRename`/`AfterRename` around the unlink); reconcile treats an absent file as written for a `removal` intent. `ConfigWriter::restore_legacy_accounts` is `--rollback`'s `ConfigCreate`: exact bytes (not emitted canonically, so byte for byte), placed by hard link, owner only, only `models/accounts.yaml`, no project check (the restored file is the legacy error by design). `sha256_hex` is public (am-app has no sha2 dependency; no dependency was added).
- **am-app:** `accounts/migrate.rs` (a child module of `accounts.rs`); `user::Locked::take` marks a store it creates `recovered` when the user file or `backups/accounts-migrate/` exists; `user::recovered` predicts it read-only, so the sentence already says `unconfirmed`. `key_use::Refused::Unconfirmed`: an Account whose latest record is `account_listed` is refused before any Keychain read naming `agentic accounts change <id>`. Test hook `runtime::testing::before_backup_check`.
- **Choices made in the build (owner standing rules: engineering calls):**
  - A rename also renames the search provider in `tools.yaml` when it names `old` (`account_uses` lists it; leaving it would point it at the other Account's destination).
  - `migrate` refuses an old file holding a secret-looking value (or any loader error) before anything is asked or written, naming only line numbers: removing it would copy the key into the project's History (`removal` before-bytes) and the backup. `--dry-run` lists the entries anyway and says migrate would refuse. A rollback refuses a backup holding one.
  - `account_listed` is written under the action `config.widened` (it is the add's widening entry, AD-15 *Record*), with the `project` id; the Key-use reader ignores it as a confirmation.
  - The project's `account_added` must have `key` absent or `stored` to carry over.
  - A backup whose hash check fails stays on disk (backups are never pruned); the next run writes a new one.
- **Target folder note:** in this session `just check` passes with `CARGO_TARGET_DIR=~/Library/Caches/agentic-manager/target` (893 tests). With the shell's own `CARGO_TARGET_DIR=~/cargo-target/agentic-manager`, `just test` fails only in `xtask::build_hook` (and once `check_deps`/`denylist`) with ENOENT when spawning `.claude/hooks/check-before-commit.sh` or the xtask binary; the same tests pass alone, through a scratch justfile, and in plain `cargo nextest run`. Not caused by a source change of this story as far as could be found; CI is the arbiter.


### Coordinator verification (2026-11-22)

The implementer's `xtask::build_hook` failure in `~/cargo-target/agentic-manager` came from stale xtask test binaries. Its temporary worktree had baked in another repo root. After `cargo clean -p xtask`, `just check` passes in that target folder: 893 tests passed, 8 skipped.

Implementer choices accepted:
- `--as` also renames the search provider in `tools.yaml`.
- `migrate` refuses, before any prompt, an old file holding a secret-looking value or a loader error, and names line numbers only.
- `store.marked_recovered` is audited.
- `account_listed` is recorded under `config.widened`.
- A backup that fails its check stays on disk.

### Review fixes: red run (2026-11-22, tests first, before any `src/` change)

New tests, appended to the story's three test files (no `src/` file touched by this round; the fixture is unchanged):

- `crates/am-app/tests/accounts_migrate.rs`:
  - Quick 1: `a_renamed_entry_shows_and_records_the_references_to_its_new_id` (the sentence names a project that names `new` and a project not checked; `account_listed.references` holds `new`'s references, not `old`'s); `a_project_naming_the_new_id_meanwhile_refuses_the_migrate` (re-checked under the lock).
  - Quick 2: `a_crash_re_run_of_a_rename_shows_the_renamed_lines_and_asks_the_id`; `an_unrelated_new_id_at_the_same_destination_is_refused`.
  - Quick 3: `the_manifest_hashes_match_two_renames_in_one_file` (beta names `clash` and `spare`, both renamed).
  - G-outside: `a_seat_edited_during_the_prompts_is_never_taken_in` (the seat has a History entry; edited at the first prompt; refused, nothing written, the next run names it as an outside change); `the_new_id_named_in_the_project_during_the_prompts_refuses`.
  - G-dry: `a_dry_run_never_prints_a_key_looking_or_erroneous_value` (key-looking id, key-looking host label, an entry with an error).
  - G-removed: `a_record_broken_by_a_later_write_without_the_id_carries_nothing`; `a_record_before_an_earlier_removal_carries_nothing`.
  - Adversarial 7/8: `a_linked_old_file_or_folder_is_refused_before_any_user_write`.
  - Adversarial 10: `the_rollback_sentence_escapes_the_manifests_renames`.
- `crates/am-config/tests/accounts_migration.rs`:
  - Quick 9: `undo_and_restore_of_a_removal_are_refused_naming_rollback`.
  - Adversarial 7: `a_removal_resolves_the_old_file_again_before_the_unlink` (a test `ConfigHistory` swaps the old file for a link to an identical copy after the first rename commits).
  - Adversarial 6 and 8, already green (kept as guards): `a_removal_that_would_fail_writes_no_rename`, `a_re_run_completes_a_partial_migration`, `a_linked_old_file_or_folder_is_refused_before_any_write`.
- `crates/am-store/tests/accounts_migrate.rs`:
  - G-open: `a_mark_that_cannot_be_read_is_an_error_not_unmarked` (the one marks row deleted past its trigger); `a_missing_marks_table_at_version_two_is_an_error`.

Not testable from outside `src/`, and why:
- **G-mark (Quick 7, Adversarial 1):** a crash between user migrations 0001 and 0002 cannot be injected. No hook exists. A version-0 store holding tables is refused as foreign, so 0002 cannot be made to fail. The existing tests cover it only indirectly: `a_store_created_while_accounts_remain_is_marked_recovered` and the am-store mark tests show a store created under the recovery conditions at version 2 and marked.
- **G-open, the project-record half:** a record read error in am-app needs a corrupt project audit row. Writing one needs rusqlite, and am-app has no rusqlite dev-dependency (no new dependency). Only the mark half is tested, in am-store.

**Command:**

```
cargo nextest run --no-fail-fast -p am-store --test accounts_migrate -p am-config --test accounts_migration -p am-app --test accounts_migrate
```

**Output (exit 100): 61 tests run, 45 passed, 16 failed.** The 16 that fail (every older test of the story still passes, and so do the 3 guards):

```
FAIL am-app::accounts_migrate a_renamed_entry_shows_and_records_the_references_to_its_new_id
  names the project that names the new id: 'clash' as 'clash-old-host' (provider openai-compatible): unconfirmed. …
FAIL am-app::accounts_migrate a_project_naming_the_new_id_meanwhile_refuses_the_migrate
  assertion `left != right` failed: agentic: moved the Accounts of …/projects/p … added cloud (carried over), spare (unconfirmed), clash-old-host (unconfirmed) …
FAIL am-app::accounts_migrate a_crash_re_run_of_a_rename_shows_the_renamed_lines_and_asks_the_id
  assertion `left == right` failed: the new id is typed again (left: 0, right: 1)
FAIL am-app::accounts_migrate an_unrelated_new_id_at_the_same_destination_is_refused
  assertion `left != right` failed: agentic: moved the Accounts … added cloud (carried over), spare (unconfirmed) …
FAIL am-app::accounts_migrate the_manifest_hashes_match_two_renames_in_one_file
  the bytes written: schema_version: 1 … (no hash of beta.md as written)
FAIL am-app::accounts_migrate a_seat_edited_during_the_prompts_is_never_taken_in
  assertion `left != right` failed: agentic: moved the Accounts of …/projects/p …
FAIL am-app::accounts_migrate the_new_id_named_in_the_project_during_the_prompts_refuses
  assertion `left != right` failed: agentic: moved the Accounts of …/projects/p …
FAIL am-app::accounts_migrate a_dry_run_never_prints_a_key_looking_or_erroneous_value
  a key-looking id: the value was printed: … sk-ant-api03-dryidcanary-0123456789abcdef  anthropic  api.anthropic.com  unconfirmed …
FAIL am-app::accounts_migrate a_record_broken_by_a_later_write_without_the_id_carries_nothing
  not carried: … (an account_added for cloud with confirmation: migrated)
FAIL am-app::accounts_migrate a_record_before_an_earlier_removal_carries_nothing
  not carried: … (an account_added for cloud with confirmation: migrated)
FAIL am-app::accounts_migrate a_linked_old_file_or_folder_is_refused_before_any_user_write
  assertion `left == right` failed: file: refused before any prompt (left: 2, right: 0)
FAIL am-app::accounts_migrate the_rollback_sentence_escapes_the_manifests_renames
  a raw control character reached the terminal: "… Rolling back the Accounts move of …
FAIL am-config::accounts_migration undo_and_restore_of_a_removal_are_refused_naming_rollback
  undo: names the way back: refused: models/accounts.yaml: the file does not exist; the writer edits existing files only
FAIL am-config::accounts_migration a_removal_resolves_the_old_file_again_before_the_unlink
  a link is never removed: Ok(["seats/beta.md", "seats/gamma.md", "models/accounts.yaml"])
FAIL am-store::accounts_migrate a_mark_that_cannot_be_read_is_an_error_not_unmarked
  the reader (is_recovered() returned Ok(false))
FAIL am-store::accounts_migrate a_missing_marks_table_at_version_two_is_an_error
  assertion failed: UserReader::open(&layout).unwrap().is_recovered().is_err()
```

`cargo clippy -p am-store -p am-config -p am-app --tests -- -D warnings` is clean on the new tests.

### Follow-up fixes: red run (2026-11-22, tests first, before any `src/` change)

New tests, appended to the story's test files (no `src/` file touched by this round; the fixture is unchanged):

- `crates/am-app/tests/accounts_migrate.rs`:
  - Follow-up 1, the window the review names: `a_reference_to_old_added_from_outside_before_the_apply_is_never_taken_in`.
    - Setup: `--as clash=clash-old-host` while beta and gamma name `clash` nowhere. Through the existing `before_backup_check` hook, which runs after the lock's re-read and before the apply, `seats/beta.md` (no History) gains `account: clash`.
    - Expected: refused, naming `seats/beta.md`; the file is as the outside program wrote it; no project write; the old file not removed.
  - Follow-up 1, during a prompt: `a_reference_to_old_added_from_outside_during_a_prompt_is_never_taken_in`. **It already passes and is kept as a guard**: the lock's re-scan of `clash`'s references catches a change made at a prompt.
  - Follow-up 2: `a_re_run_after_a_failure_before_the_manifest_completes_the_rename`.
    - No test hook stops a run between the user commits and the manifest. Instead, a plain file stands where this project's backup folder goes, so the backup and its manifest cannot be written after the user records committed.
    - Expected: after removing that file, a re-run with the same `--as` completes (renamed, removed, one `account_listed` for `clash-old-host`).
  - Follow-up 3: `a_dry_run_shows_no_value_of_a_duplicated_id_with_an_unclean_copy`.
    - Setup: `id: cloud` twice, the second with a key-looking label in `base_url`, or a plain `base_url` the endpoint rules refuse.
    - Expected: neither marker is printed, and no value of the first `cloud` is printed either.
- `crates/am-config/tests/accounts_migration.rs`:
  - Follow-up 4: `a_removal_whose_re_read_fails_leaves_no_pending_intent`. A test `ConfigHistory` deletes the old file right after the removal's intent commits; the removal must be refused and leave no pending intent.

**Command:**

```
cargo nextest run --no-fail-fast -p am-config --test accounts_migration -p am-app --test accounts_migrate
```

**Output (exit 100): 57 tests run, 53 passed, 4 failed.** The earlier tests and the guard pass; these 4 fail:

```
FAIL am-app::accounts_migrate a_reference_to_old_added_from_outside_before_the_apply_is_never_taken_in
  assertion `left != right` failed: agentic: moved the Accounts of …/projects/p to this Mac's Accounts …
FAIL am-app::accounts_migrate a_re_run_after_a_failure_before_the_manifest_completes_the_rename
  assertion `left == right` failed (left: 1, right: 0):
  agentic: `--as clash=clash-old-host` is refused: this Mac's Accounts already hold 'clash-old-host' (and no earlier run of this migrate added it there); choose an id this Mac does not have. …
FAIL am-app::accounts_migrate a_dry_run_shows_no_value_of_a_duplicated_id_with_an_unclean_copy
  a key-looking base_url: printed: … cloud  openai-compatible  sk-ant-api03-dupcanary-0123456789abcdef.example  unconfirmed …
FAIL am-config::accounts_migration a_removal_whose_re_read_fails_leaves_no_pending_intent
  no pending intent: [PendingIntent { … intent: NewIntent { file: "models/accounts.yaml", kind: Removal, … } }]
```

`cargo clippy -p am-config -p am-app --tests -- -D warnings` is clean.

## Auto Run Result

**Status:** built (2026-11-22).

**Summary:** `agentic accounts migrate --project <path>` moves a project's old `models/accounts.yaml` to this Mac's Account list.
- **Per entry:** an entry already on the list is present. A clash is refused unless `--as old=new` is given. Otherwise the project's confirmation is carried over, as `account_added` with `confirmation: migrated`, only on an exact match: a verified chain, the latest record, no removal after it, History continuity, and the store not marked `recovered`. Every other entry is added unconfirmed, as `account_listed`, and refused for key use until `accounts change`.
- **No Keychain call at all.**
- **Order of writes:** the user records commit first. Then a hash-checked backup and manifest are written. Then one project `ConfigMigration` removes the file through a new `HistoryKind::Removal` step and applies the `--as` renames, guarded by `Expects`.
- **`--dry-run`** only reads, from any stdin, and shows no unclean value.
- **`--rollback`** re-creates the file byte for byte and keeps the renames.
- **The `recovered` mark** comes from user migration 0002. A fresh marked store is created in one transaction.

**Files changed (src):**
- **am-domain:** `HistoryKind::Removal`; `AppDataLayout::accounts_migrate_backups`.
- **am-store:**
  - `migrations.rs`: the `on_create` mark; a fresh store migrates in one transaction;
  - `user.rs`: the marks, fail-closed reads, `is_created`;
  - `migrations/user/0002_user_marks.sql`;
  - `config_history.rs`: the removal kind;
  - `ConfigNote`: `entry_hash` and `entry_id`.
- **am-config:**
  - `account_migration.rs`: `ConfigMigration`, `MigrationStep::{Remove, RenameAccount, Expects}`, `legacy_accounts`, `account_renames`;
  - `writer.rs`: `apply_migration`, `remove_step`, `restore_legacy_accounts`; Undo and Restore of a removal refused.
- **am-app:**
  - `accounts/migrate.rs`: `migrate`, `--dry-run`, `--as`, `--rollback`;
  - `user.rs`: marks a recovered store on create;
  - `key_use.rs`: `Refused::Unconfirmed`;
  - `runtime.rs`: the `before_backup_check` test hook;
  - CLI usage.

**Review:**

| Pass | Findings | Patched | Deferred | Rejected |
|---|---|---|---|---|
| First (quick lens and adversarial migrate lens) | 19: high 4, medium 11, low 3 | 18 | 1 | — |
| Follow-up | 4: medium 2, low 2 | 4 | — | — |

- One first-pass low (manifest tampering) was patched only in part: the rollback sentence now escapes the manifest's renames, and tampering itself needs a same-user write, which is outside the threat model.
- **Deferred:** the destination rule of the public `apply_migration`, to epic 4 story 17's classifier (frontmatter).
- **Patched by verdict:** high 4, medium 13, low 5. Every patch round ran tests first.

**Follow-up review:** not recommended. The last pass patched no high finding.

**Verification:** `just check` passes, 917 tests passed and 8 skipped. The canary and no-Keychain checks are the tests' own.

**Residual risks:**
- A crash between user migrations on a store not created under the recovery conditions still migrates step by step, as before.
- A corrupt project audit row is not covered by a test; it fails closed by code.
- `apply_migration` without `Expects` is unguarded for other callers (deferred to 4.17).

