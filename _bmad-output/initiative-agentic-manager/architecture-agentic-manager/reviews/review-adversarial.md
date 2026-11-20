---
review: adversarial (pairwise incompatibility)
target: architecture-agentic-manager.md, the 2026-11-20 architect REPLAN amendments only
date: 2026-11-20
method: read only; for each amendment, construct two independently built units (stories or crates) that each obey every AD as written and still build incompatibly
---

# Adversarial review: the 2026-11-20 amendments

## Verdict

The amendments are not yet safe to build from. AD-48's effective allow-list lets every website fetch reach model and loopback Account endpoints. `project_id` has no lifecycle rule, so it can be minted twice or changed. The runtime path layout contradicts itself. The `egress_hosts` migration cannot pass `is_widening` or the approved grant-set fingerprint as both are specified. The single glob helper lives in a crate that none of its consumers may depend on (AD-4).

Scope: the passages marked "amended 2026-11-20, architect REPLAN", AD-48, and the Consistency Conventions rows Runtime paths, Globs and Host patterns. Other ADs (AD-4, AD-5, AD-10, AD-13, AD-15, AD-16, AD-25, AD-35, AD-37, AD-39, AD-40, AD-47) were read only to judge how they interact with the amendments. The code was checked only where it is already built and an amendment depends on it: `crates/am-store/src/project.rs` and `migrations.rs`, `crates/am-config/src/kinds.rs` and `crates/am-net/src/lib.rs`. Two tickets were also checked: epic-triggers story 18 and the epic-tools daily-backup story.

Severity: **C** critical (breaks a trust rule), **H** high (two correct units cannot be joined, or data is lost), **M** medium, **L** low.

---

## P1 (C): The effective allow-list makes Account endpoints, loopback ones included, reachable by every fetch

**Unit A:** the `am-net` story that implements AD-48's effective allow-list. It builds a seat's list as `allowed_hosts` ∪ {the pinned host of the seat's model, fallback and capability-provider Accounts}, then checks every request carrying that seat against it (AD-12: "The host must be in that seat's effective allow-list").
**Unit B:** two connector stories: the Rendered Reader's subresource interception (AD-39: "GET, HEAD or POST with a capped body … through `Fetcher` with … the owner seat's allow-list") and the drop-folder link-list loader (AD-30: "fetches each URL … with the owner seat's allow-list").
**Both obey:** AD-12, AD-30, AD-39, AD-46, AD-48. AD-48 says outright: "Every 'seat's allow-list' in AD-12, AD-30, AD-39 and AD-46 (egress, link-host warnings, Source scope hosts, link lists in drop folders) means the effective allow-list."

**Clash.** The two units together let any seat that uses `local-lmstudio` (`127.0.0.1:1234`) reach it from untrusted content. A hostile page's script, or a `links.txt` line in a drop folder, can send a GET or a capped-body POST to the local model server's management API. AD-12's resolver exception for "a local Account … on 127.0.0.1 at its well-known port" admits it, because the host is now on the seat's list. The same holds for an owner-typed loopback OpenAI-compatible or self-hosted Firecrawl Account, which has no authentication. Cloud Account hosts (`api.openai.com`, a search Account's API host) also become legal targets for `read_url`, `read_website` and Source scope hosts. A Source can then name `api.anthropic.com` as a scope host and pass AD-39's "scope hosts must be on the owner seat's allow-list" check without any `allowed_hosts` entry.

Two side effects follow. Changing a seat's model from one cloud Account to another is not widening under AD-15, yet it now widens every tool's reach. A dry run (AD-37) substitutes a model Account at run start. One unit computes the effective list from config and refuses the dry run's model calls. Another computes it from the run and silently adds a host for every tool.

**Tightening (amend AD-48 and AD-12):**
- An Account's pinned endpoint is an `(account_id, host, port)` triple. It admits only requests whose `account` field is that Account and which that Account's own adapter built (provider call, search call, crawl-service call).
- Tools, `Fetcher`, Source scope checks, link-host warnings and link lists check `allowed_hosts` alone.
- The loopback exception never applies to a `Fetcher` request.
- "What this seat can touch" shows the two lists separately.
- Add a trust test: a rendered fixture page that POSTs to `127.0.0.1:1234`, and a `links.txt` that names it, both make zero requests.

---

## P2 (C): `project_id` has no lifecycle, so it can be minted twice, changed by a pull, or stamped into templates

**Unit A:** the `am-app` open path (AD-32). It reads `company.yaml`; if there is no `project_id`, it mints a UUIDv7, derives the runtime folder, takes the lock there, then stamps the id ("a `ConfigEdit` by `Actor::System`, not widening").
**Unit B:** `am-config`'s ConfigEdit, Reload and `is_widening` (AD-15, AD-16). Every schema field must be classified, and a non-widening outside edit applies with a single Reload.
**Both obey:** AD-15, AD-16, AD-32, AD-35.

**Clashes.**
1. **Double mint.** The lock lives in the runtime folder, which is named by the id, so nothing excludes two processes before the id exists. For example, the login LaunchAgent (`agentic service`) and the desktop app open the same id-less project. Each mints a different id, creates and locks its own runtime folder, and stamps `company.yaml`; the last rename wins. Two servers then schedule the same workflows with separate ledgers.
2. **Id change by edit or `git pull`.** `project_id` sits in a git-tracked definitions file (AD-35). AD-32 classifies the first write as not widening, so Unit B's exhaustive match most naturally marks the field non-widening. A pulled commit from a clone that "gave the copy its own id", or a hand edit, then applies with one Reload. The next start opens a new, empty runtime folder. Ledger week and month caps reset, which is a cap raise that `is_widening` never sees. The approved grant-set fingerprint is absent, and AD-16 does not say what happens with no baseline. History, Inbox, knowledge files and drop folders are orphaned. No reverse index from root to runtime folder exists, so the change cannot even be detected.
3. **Templates and CI.** `just check` runs `agentic validate` over `templates/` and `fixtures/`. If "first opens" includes validate, CI writes an id into `templates/samples/company-a/company.yaml`, and every owner who creates the sample on one Mac gets the same id. Their second sample project is then refused as a copy. If validate requires the field instead, every template fails.
4. **Keychain coupling.** The backup key is `backup-key:<project-id>` (AD-17), so an id change also strands every encrypted backup.

**Tightening (amend AD-32, AD-15 and AD-16):**
- Only `agentic open` / create mints `project_id`. It does so under a lock that does not depend on the id (for example `agentic-manager-projects/.mint.lock`), re-reads `company.yaml` after taking it, and records `root → id` in a reverse index there.
- `agentic validate` never writes. `templates/` and `fixtures/` must not carry `project_id` (a validate error there), and instantiating a template always mints a fresh id.
- `is_widening` classifies a `project_id` change as `IdentityChange`. No Reload or ConfigEdit applies it. At open, an id that differs from the reverse index entry for this root is refused with two choices: "Use this project's existing state", or "Start as a new project". The second is a Human command that runs the first-open confirmation.
- AD-16: a runtime folder with no stored approved grant set treats the whole grant set as widening, like an import.

---

## P3 (H): The runtime layout is drawn two ways

**Unit A:** `am-store` today joins `AGENTIC_DIR = ".agentic"` onto the root it is given (`crates/am-store/src/project.rs:29`). Once the caller passes the runtime folder, it follows the Structural Seed, which draws `~/Library/Application Support/agentic-manager-projects/<project-id>/` with `.agentic/` nested inside it. State therefore lands at `<rf>/.agentic/state.db`.
**Unit B:** the `am-app` legacy mover follows the Runtime paths convention ("`.agentic/<x>` means `<runtime folder>/<x>`"). It copies `<project>/.agentic/*` to `<rf>/*`, so `state.db` lands at `<rf>/state.db`.
**Both obey:** AD-32 and the Consistency Conventions (the Seed and the convention row each read as authoritative).

**Clash.** After a "verified" move, the store opens a fresh empty database at `<rf>/.agentic/state.db`, and the legacy folder has already been removed. The same split hits AD-13's 0700/0600 check, the MCP cwd `.agentic/workspace/mcp/<id>/` (AD-26) and the drop-folder watcher. One crate builds `<rf>/workspace/...` and another builds `<rf>/.agentic/workspace/...`.

**Tightening:**
- Redraw the Seed with the contents placed directly under the runtime folder.
- Add one pure `am_domain::RuntimeLayout` (root plus named joins: `state_db()`, `lock()`, `backups_daily()`, `drop(source)`, `workspace(seat)`, `mcp_cwd(server)`, …) that every crate uses.
- A CI grep fails on a literal `".agentic"` in `crates/` outside that type.

---

## P4 (H): The legacy move races the old lock and SQLite's WAL

**Unit A:** story 1.6's store, already built. It holds an exclusive lock in `<project>/.agentic/` and writes `state.db` in WAL mode. A still-installed older binary keeps doing this, for example a LaunchAgent or a second Mac on the same synced project.
**Unit B:** the new mover (AD-32): "copied into the runtime folder, the copy verified (`PRAGMA integrity_check` and a sha256 per file), then the old folder removed".
**Both obey:** AD-7 and AD-32.

**Clash.** The mover copies `state.db` file by file while the old process holds the legacy lock. Committed frames still in `state.db-wal` are lost or torn. The copy of the main file passes `integrity_check`, and per-file hashes match a moving target, so a silent loss is "verified". Two more problems:
- In a cloud-synced project (the case the amendment exists for), legacy files may be dataless. The copy downloads them with no wait or cap, and FileResidency covers only linked folders.
- Removing the old folder propagates through sync. Another Mac running an older binary loses its live runtime. If that Mac runs a newer binary, it finds nothing to migrate and starts empty, so caps and the fingerprint baseline reset.

**Tightening (amend AD-32):**
- The mover first takes the legacy lock with am-store's own lock primitive and refuses, naming the holder, if the lock is held.
- It opens the legacy database through am-store and produces the new `state.db` with `VACUUM INTO` (or the online backup API), then runs `integrity_check` on the result. sha256 applies only to the other files.
- It reads legacy files through the FileResidency wait (P8).
- The removal is reported with a sentence that other Macs syncing this project lose their in-project state.
- v1 states one Mac per project (see Open decisions).

---

## P5 (H): The `egress_hosts` migration cannot pass `is_widening` or the fingerprint as specified

**Unit A:** the AD-48 config migration in `am-config`. It copies `tools.yaml` `egress_hosts` into every seat's `allowed_hosts` and removes the key, "so it is not widening", and is written on owner accept (AD-7).
**Unit B:** story 17's exhaustive `is_widening(patch)` and story 18's approved grant-set fingerprint (epic-triggers: "a hash of every field story 17 classifies", compared at Reload and every startup). The writer enforces both "on every path".
**Both obey:** AD-7, AD-15, AD-16, AD-48.

**Clashes.**
1. `is_widening` sees a patch that adds N patterns to each of M seats, so it returns N×M widenings, which the "not widening" rule contradicts. The new schema has no `egress_hosts` field, so the exhaustive match cannot see the removal that offsets the additions. Unit A must either bypass the writer's check, which breaks "every path", or put the owner through an N×M two-step confirmation.
2. The fingerprint is stored as a hash, but AD-16 runs `is_widening` against it, which needs the stored grant set itself, not a hash. Its shape also changes with the migration: a company-wide list before, per-seat lists after. Either every startup after the migration is held, or the comparator discards the old record and nothing is held.
3. The interim validator (`kinds.rs:540`) accepts any `[A-Za-z0-9.-]` string containing a dot. That includes `10.1.2.3`, which `crates/am-net/src/lib.rs` uses in a test, and uppercase or trailing-dot names. `HostPattern` refuses IP literals and uppercase, so a faithful copy makes every migrated seat fail `agentic validate`.
4. Nothing says what a binary does while the migration is pending or declined: read the retired key, ignore it (every seat narrows to its Accounts), or refuse to load. Nothing says what an older binary does with a newer config `schema_version`, which is likely once project files sync between Macs. AD-7 refuses newer databases only.

**Tightening (amend AD-7, AD-15, AD-16, AD-48):**
- A config migration is its own writer command, `ConfigMigration`. It is classified by `is_widening_between(old_snapshot, new_snapshot)`, which compares host coverage using `HostPattern::covers` (P7), not patches.
- The approved grant set is stored as a versioned, structured record (the hash is only its index). The same commit migrates it.
- Each entry that `HostPattern` refuses is shown in the diff and dropped with its reason (narrowing).
- Until the migration is accepted, the snapshot is held exactly like a widening hold.
- A config file whose `schema_version` is newer than the binary knows is refused, worded like the database rule.

---

## P6 (H): The single glob helper sits where its consumers may not depend

**Unit A:** `am-tools` (`list_files` filters, linked-folder `include`/`exclude`, AD-30) and `am-engine` (fan-out `over_folder` routes, AD-40).
**Unit B:** `am-connectors` (crawler URL-path scope, and the CrawlService adapter that sends "`includePaths`, `excludePaths` (from the globs)", AD-39).
**Both obey:** the Globs row says "`globset` only, through am-config's one glob helper". AD-4 and the Design Paradigm table say `am-tools` and `am-connectors` depend on `am-domain` and `am-net` only, and `am-engine` on `am-domain` and `am-store`. `am-domain` may link "std, serde, thiserror, jiff, uuid only".

**Clash.** No consumer can call the helper, so each links `globset` itself ("globset only" is satisfied) with its own builder options and its own idea of the match subject. The validator says a pattern compiles and matches; the folder tools disagree. That is the exact divergence the memlog says this row prevents. Concrete splits:
- **Case and normalisation.** Case-insensitive matching versus none. APFS returns names in the form they were created, often NFD from Finder, while YAML holds NFC, so `résumé*.pdf` matches in one crate and not the other.
- **Match subject.** Root-relative versus absolute; URL path with or without the query (AD-39: "drops query strings unless a glob names them"); percent-decoded or not.
- **Firecrawl translation.** Firecrawl's `includePaths`/`excludePaths` are regular expressions, not globs, so the adapter's hand translation differs from globset's semantics. The app re-checks robots on returned pages but not scope.
- **Widening.** AD-15's "widening a linked folder's include patterns" needs glob-set containment, which no unit can decide in general.

**Tightening (amend the Design Paradigm, the Globs row, AD-30 and AD-39):**
- Give the helper a reachable home: a leaf crate `am-glob` (depends on `globset` only, allowed for `am-config`, `am-tools`, `am-connectors` and `am-engine` in the matrix and in `xtask check-deps`), or explicitly allow `globset` in `am-domain`.
- It exposes `PathGlob::{folder, url_path}` constructors that fix case and separators and define the match subject. For folders: root-relative, `/`-separated, NFC-normalised path. For URLs: the path, plus `?query` only when the pattern contains `?`.
- It also exposes `regex()` for the CrawlService adapter, which must re-filter every returned page through the same matcher and count misses as out-of-scope skips.
- `is_widening` for globs is syntactic: any added `include` or removed `exclude` is widening.

---

## P7 (M): `HostPattern` cannot compare two patterns, which "broader", Source scope hosts and the migration all need

**Unit A:** `am-config` `is_widening`. Per AD-15 and AD-48, "replacing one with a broader one" is widening.
**Unit B:** the Source validator. AD-39's `scope.allowed_hosts` must be "on that seat's allow-list" (AD-12). This second field, also called `allowed_hosts`, is validated today with the interim host check (`kinds.rs:2110`).
**Both obey:** AD-48 ("one pure type … parses and matches patterns; nothing else matches hosts") and the Host patterns row.

**Clash.** AD-48 defines `matches(host)` only. Both rules compare a pattern with a pattern (`*.example.org` versus `www.example.org`). One unit feeds a pattern string to `matches`, which never matches. Another writes a suffix comparison, which the Host patterns row forbids. The two give different answers for "is `*.a.example.org` covered by `*.example.org`". It is also unstated whether a Source scope may hold `*.` patterns. Request hosts with a trailing dot (`example.org.`) or uppercase letters need normalising before `matches`: `am-net` today uses `eq_ignore_ascii_case`, while `HostPattern` is lowercase-only.

**Tightening (amend AD-48):**
- `HostPattern` exposes `parse`, `matches(&Host)` and `covers(&HostPattern)`.
- A normalised `am_domain::Host` (lowercase, punycode, trailing dot stripped, not an IP literal) is the only argument to `matches`.
- Source `scope.allowed_hosts` are `HostPattern`s checked with `covers` against the seat's `allowed_hosts`.
- Widening means some new pattern is not covered by the old set.

---

## P8 (H): Cloud-only listing cannot satisfy the folder cursor

**Unit A:** `am-engine` fan-out over a folder (AD-40): `files: new_or_changed`, "tracked by the step's folder cursor of path and sha256". AD-35 likewise stores "per-step folder cursors (path and sha256 last read)".
**Unit B:** `am-tools` `list_files` with FileResidency (AD-30): listing "use[s] its metadata only and never download[s] it". "`sha256` on request downloads the file and counts as a read", and "A fan-out over a folder … lists without downloading".
**Both obey:** AD-30, AD-35, AD-40.

**Clash.** To decide new-or-changed, Unit A asks for sha256, which downloads every cloud-only file in the folder at list time and charges the reads to whom? Unit B forbids that download. Otherwise Unit A treats every cloud-only file as changed on every run, so each run re-reads and re-spends on them. AD-30 also lacks three things:
- **Trait home.** FileResidency is not in AD-5's port list, and its trait has no home crate that `am-tools` and `am-engine` can both reach.
- **Platform branch.** Its non-macOS branch "reads as local", while AD-47 requires such a branch to return "not supported on this platform yet".
- **Blocking read.** A read of a dataless file blocks in the kernel, so neither AD-20's cancellation tree nor the 120 s download wait can stop it unless the adapter starts the download and polls residency instead of blocking on `read`.

**Tightening (amend AD-30, AD-35, AD-40, AD-5, AD-47):**
- The folder cursor is `(path, size, mtime, provider content version when available)`. sha256 is recorded only after a read, and changed metadata counts as changed without a download.
- `FileResidency` is listed in AD-5 with its trait in `am-domain`, and AD-47 names its non-macOS `Local` answer as an exception.
- The adapter requests materialisation and polls under the step's cancellation token; it never blocks a runtime thread on a dataless `read`.
- After the download, the actual size is re-checked against the caps before any byte goes to a model.

---

## P9 (H): Copy and move detection compares path strings

**Unit A:** the `am-app` opener records "the project's canonical root" from the `--project` argument, or from the desktop app's recent-projects entry.
**Unit B:** `agentic service install` registers `agentic serve --project <path>` (AD-32) with whatever spelling the owner typed. Examples: `~/OneDrive/...` (a symlink to `~/Library/CloudStorage/OneDrive-Personal/...`), a different letter case on case-insensitive APFS, or a path on `/Volumes/X`.
**Both obey:** AD-32 ("same id at another root that still exists = copy; recorded root gone = move").

**Clash.** The same folder reached by two spellings is refused as a copy of itself. In the other direction, a project on an unmounted external or network volume ("no longer exists") is re-recorded as moved to a real copy. When the volume returns, the original is refused as a copy of its copy, and the owner's only way out is a new id, which means empty state (P2). The record also lacks a home in AD-35: a runtime-folder file, or a database row so it can be audited. The MCP cwd in `tools.yaml` (AD-26) bakes a runtime path into a git-tracked file. A copy given a new id either keeps pointing into the original's runtime folder, or changes the spec hash and forces every MCP server to be re-approved.

**Tightening (amend AD-32, AD-35, AD-26):**
- Root identity is `(volume UUID, directory file id)`, with the canonical path read back from the file system (`F_GETPATH`). Copy and move compare identity, not strings.
- "Move" applies only when the old root's volume is mounted and the root is absent. In every other case the opener asks.
- The recorded root is a row in `state.db` (AD-35: database column), written by an audited writer command.
- The MCP cwd is not a config field. `RuntimeLayout::mcp_cwd(server_id)` derives it, and the spec hash covers only its symbolic form.

---

## P10 (M): "Audited in the apply's own transaction" spans a file and a database

**Unit A:** the `am-config` ConfigEdit writer (AD-16: temp file, fsync, rename).
**Unit B:** the `am-store` writer. It commits the per-widening audit entries (AD-15, amended: "in the apply's own transaction"), the History entry and the new approved grant set (AD-16).
**Both obey:** AD-6, AD-15, AD-16, AD-25.

**Clash.** No transaction covers both a rename and an SQLite commit, so each unit picks its own order. If the file is renamed first and the process dies, a widening is on disk with no audit entry; startup holds it, but the audit chain is missing the event. If the database commits first and the rename fails, the stored approved set already contains the new host. A later outside edit that adds exactly that host, for example from a synced folder or a hostile process, is then not seen as widening. This silently pre-approves a widening.

**Tightening (amend AD-16):**
- Order the steps as: temp file plus fsync, then a database commit that also records the intended post-content hash, then the rename.
- At startup and Reload, an apply whose post-hash does not match the file on disk is marked incomplete, and its approved set is not trusted. The comparison falls back to the previous approved set.

---

## P11 (M): Pre-migration backups and daily encrypted backups share `backups/`

**Unit A:** am-store's migrator, already built (`migrations.rs:202`). It writes plaintext `backups/state-v{n}-{stamp}.db` and, per amended AD-7, keeps the three newest pre-migration copies.
**Unit B:** the epic-tools daily-backup story (AD-32). It encrypts copies into `.agentic/backups/`, removes "stale" plaintext temporary files at start, and keeps 14. Its verify requires "no plaintext copy remains in `.agentic/`" and "a row deleted before the oldest kept copy exists in no backup".
**Both obey:** AD-7, AD-25, AD-32.

**Clash.**
- **Overlapping pruning.** Unit B's stale sweep or 14-copy rotation deletes the AD-7 recovery copies, or Unit A's keep-three count includes daily copies.
- **Contradictory verify.** Unit B's verify fails on Unit A's plaintext files.
- **Deleted content survives.** Three pre-migration copies keep deleted row content indefinitely, against AD-25's "deleting a row or run really deletes its content".

**Tightening (amend AD-7, AD-32):**
- Use separate subfolders, `backups/pre-migration/`, `backups/daily/` and `backups/tmp/`. Each pruner touches only its own, and the stale sweep runs only in `tmp/`.
- Pre-migration copies are encrypted under the same key, or the spine names them as a stated plaintext exception with a bounded age (for example, deleted after 30 days or after the next migration), and the About page says so.

---

## P12 (M): Cloud classification by path prefix disagrees with what actually syncs

**Unit A:** the `am-config` link validator. Following AD-30, it treats a folder as a "cloud-storage root" (listed under "What leaves this Mac" when write is on) by prefix: `~/Library/CloudStorage/<provider>/` or `~/Library/Mobile Documents/com~apple~CloudDocs/`.
**Unit B:** the FileResidency adapter. It finds dataless placeholders anywhere, including `~/Documents` and `~/Desktop` under iCloud "Desktop & Documents". The memlog notes that iCloud syncs `~/Documents` in place.
**Both obey:** AD-30, AD-47.

**Clash.**
- **Missing disclosure.** A `read_write` folder under `~/Documents` syncs to iCloud but is never disclosed.
- **Refusal checks disagree.** The refuse list (`~/Library` except the two roots, the home folder, the root) is checked twice: at link time in `am-config` and on every access through `WorkspacePath`. The two checks can disagree on case (`~/library/...` on case-insensitive APFS, if the canonical path keeps the typed case) and on the iCloud container spelling.

**Tightening (amend AD-30):**
- Decide "this folder syncs to <provider>" through the FileResidency port (File Provider domain or ubiquitous-item query) at link time and on each listing. Path prefixes are used only for the refuse list.
- One `am_domain` `FolderRootPolicy::check(canonical_path)` with case-folded comparison serves both the validator and `WorkspacePath`.

---

## P13 (M): A linked folder can now contain the project's config files

**Unit A:** the AD-30 link validator. Since the 2026-11-20 move, no runtime folder lies inside a project, so nothing refuses linking the project root or an ancestor of it. The amendment explicitly allows a whole provider root, `~/Library/CloudStorage/<provider folder>/`, which is where this very project lives.
**Unit B:** AD-16's watcher and Reload. A non-widening outside edit, such as a seat's instructions body or goal, applies with a single Reload, and it becomes `Instruction` (AD-10).
**Both obey:** AD-10 (b), AD-16, AD-30.

**Clash.** A seat with that folder linked `read_write` uses `write_file` to rewrite `seats/<other>.md`'s body or `workflows/*.yaml`. An untainted step may write anywhere under the root. A tainted step may write its declared paths, and the declared output path can be a config file. The owner sees "a file changed outside the app", clicks Reload, and model-authored text (possibly a laundered injection) becomes `Instruction`.

**Tightening (amend AD-30):**
- Refuse linking a folder that is, contains or lies inside the open project's root (and any recent project's root).
- At minimum, `write_file` refuses any path under a project root.
- A ConfigChangedOnDisk caused by the app's own `write_file` is applied only as a tainted-origin Proposal.

---

## Open decisions to hand to the owner

1. **Two Macs on one synced project.** With the runtime outside the project, each Mac has its own `state.db`, scheduler, ledger and caps, and no cross-Mac lock exists. Both Macs fire the same schedules and double the spend. The rule "nothing runtime is written into the project folder" rules out a presence marker. The spine should state that v1 supports one Mac per project (Doctor cannot detect two), or allow one small marker file as a named exception.
2. **The migration copies every host to every seat.** That keeps behaviour, but it reproduces the company-wide reach AD-48 exists to prevent until the owner prunes. Consider having the migration diff offer a per-seat assignment step, or a Doctor nudge.
3. **Shipped role templates that carry `allowed_hosts`.** AD-15 exempts their capabilities, not their hosts. Say whether template hosts count as widening at Create.

## Summary table

| # | Sev | Unit A | Unit B | Closing AD change |
|---|---|---|---|---|
| P1 | C | am-net effective list | Rendered Reader / link lists / reader tools | Account endpoints bound to (account, host, port) for that Account's own adapter calls; tools and Fetcher check `allowed_hosts` only; no loopback for Fetcher |
| P2 | C | am-app id mint and open | am-config Reload / `is_widening` | Mint under an id-independent lock; reverse root index; `IdentityChange` never Reload-applied; no ids in templates; no baseline means whole set is widening |
| P3 | H | am-store paths | am-app legacy mover | Fix the Seed; `am_domain::RuntimeLayout`; CI grep for `".agentic"` |
| P4 | H | legacy lock and WAL (story 1.6) | legacy mover | Take the legacy lock; `VACUUM INTO` copy; residency-aware copy; one Mac per project |
| P5 | H | egress_hosts migration | `is_widening` and fingerprint | `ConfigMigration` with snapshot-to-snapshot coverage; versioned stored grant set; refuse newer config |
| P6 | H | am-tools / am-engine globs | am-connectors globs | `am-glob` leaf crate (or globset in am-domain); defined match subjects; regex for CrawlService plus re-filter |
| P7 | M | `is_widening` "broader" | Source scope host check | `HostPattern::covers`; normalised `Host` |
| P8 | H | AD-40 sha256 cursor | AD-30 metadata-only listing | Metadata cursor; FileResidency in AD-5 and am-domain; async materialise; re-check size |
| P9 | H | opener root record | service install / recent projects | Identity by volume UUID and file id; ask on an unmounted volume; DB-recorded root; derived MCP cwd |
| P10 | M | config file writer | store audit and fingerprint | Ordered two-phase apply with a post-hash reconcile |
| P11 | M | pre-migration backups | daily encrypted backups | Separate subfolders; encrypt or bound pre-migration copies |
| P12 | M | prefix-based cloud check | FileResidency | Sync decided by the port; one folder-root policy check |
| P13 | M | folder link validator | outside-edit Reload | Refuse linking at, inside or above a project root; no `write_file` under a root |
