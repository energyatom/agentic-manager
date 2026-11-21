---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-22 owner amendment to AD-30 only ("Project subfolders", the Rule part of AD-30)
date: 2026-11-22
---

# Review: versions and facts in the 2026-11-22 AD-30 amendment

## Verdict

**Pass with changes.** The amendment names no third-party technology, crate or version, so there is nothing to check against the web or the Stack. Every name it lists matches what the existing `am-config` loader reads, and the Company A sample's `files/d1-inputs` link passes. One committed guarantee, "so no seat can reach config files", does not hold on this Mac as written. The reason is that the names are compared exactly, while the default APFS volume ignores case. The existing built code and one valid fixture also contradict the amended rule. That belongs in the ticket, not the spine.

## Method

- **Diff:** `git diff --word-diff` of the spine against HEAD. The change is the `updated:` date and the AD-30 Rule sentence on project-root refusal, which was replaced by the "Project subfolders" paragraph.
- **Spine:** read the Structural Seed's "Project folder (owner's, git-friendly)" tree (lines 581–596), AD-16 (line 228) and AD-15 (line 222).
- **Loader code** (read-only):
  - `crates/am-config/src/lib.rs`: `FOLDERS`, `MODEL_FILES`, `read_all`, `has_config`, `tracked`, `load_with`.
  - `crates/am-config/src/canon.rs`: `kind_of`.
  - `crates/am-config/src/session.rs`: `scope_of`, `scan`, `ConfigEvent::ChangedOnDisk`.
  - `crates/am-config/src/kinds.rs`: `seat_folders`, `folder_location`, `refused_location`, `seat_knowledge`, `seat_links`.
  - `crates/am-config/src/writer.rs:855`.
  - `crates/am-domain/src/runtime.rs`: `OPEN_MARKER`, `RUNTIME_SEED`, `is_legacy_component`, `seed_knowledge`.
- **Sample and fixtures:** `templates/samples/company-a/` (seat `folders:` blocks, `files/d1-inputs/`, `.gitignore`, `runtime-seed/`), `fixtures/projects/*/seats/*.md` and `crates/am-config/tests/anatomy.rs`. Also read `reference-workload.md` flow F3 and the "Sources, skills and knowledge" section.
- **Tickets:** read the diffs of `epic-foundations-rust-spike` and `epic-connectors-sources` `tickets.toml`.
- **On this Mac (macOS, APFS, read-only, scratchpad only):**
  - `ls -d …/agentic-manager/Crates` opens `crates/`, so the repo volume ignores case.
  - In the scratchpad, `mkdir Seats; ls seats` lists the file.
  - `realpath` of `…/SEATS` returns `…/SEATS`: canonicalising keeps the case as typed, not the case on disk.

## Reality check results

### 1. Config folder and file names against the spine tree and the loader

| Name in the amendment | Spine project tree | Loader (am-config) | Match |
|---|---|---|---|
| `models/` | `models/accounts.yaml prices.yaml capabilities.yaml` | `MODEL_FILES`, `read_all` reads `models/{accounts,prices,capabilities}.yaml` and warns on any other file in it | yes |
| `seats/` `workflows/` `tables/` `schemas/` `dashboards/` `sources/` | all listed | `FOLDERS` const (lib.rs:108–115): exactly these six, with extensions md/yaml/yaml/json/yaml/yaml | yes |
| `skills/` | `skills/<skill-id>/` | `skills::load_all`; `tracked` also covers `skills/<id>/references/*` | yes (references are inside `skills/`, so covered) |
| `company.yaml`, `tools.yaml` | both listed | `read_all`, `kind_of`, `scan` | yes |
| `.agentic` | runtime folder name | `am_domain::runtime::LEGACY_DIR`, compared **ignoring case** by `is_legacy_component` | yes |
| `runtime-seed` | Structural Seed (templates and fixtures only) | `RUNTIME_SEED`; read only under `LoadOptions::template` | yes |
| `.agentic-open` | listed in the project tree | `OPEN_MARKER` | yes |

The loader reads no other folder or root file:

- `has_config` checks `company.yaml`, `tools.yaml`, `models`, `skills` and `FOLDERS`, and nothing else.
- `knowledge:` and `links:` are seat-frontmatter keys. `knowledge` names files in the runtime folder's `knowledge/<seat-id>/`, never the project. `links` are URLs.
- `.gitignore` is written as a `ConfigCreate` (AD-16) but never read by the loader. It sits at the project root, which can never be linked, so leaving it out of the list is harmless.
- `dashboards/` is read (`FOLDERS`).

The list is therefore complete and exact against today's code.

### 2. Company A `files/d1-inputs/`

- **Contents:** `templates/samples/company-a/files/d1-inputs/` holds `brief.pdf`, `scan.pdf`, `counts.csv`, `items.csv`, `dates.csv`, `chart.png`, `diagram.png` and `memo.docx`. That matches reference-workload F3 ("two PDFs … three csv … two PNGs … one docx"). No entry bears a config name.
- **Links:** `d1-head.md`, `d1-e1.md`, `d1-e2.md` and `d1-e3.md` each link `{ id: d1-inputs, path: files/d1-inputs, access: read }`. The folder is strictly inside the root and read-only.
- **Result:** the link passes the amended rule. `runtime-seed/` and `.gitignore` sit beside `files/`, not under it, so they are not in the linked subtree.

### 3. Named mechanisms

- **`ConfigChangedOnDisk` (AD-16):** exists as `am_config::session::ConfigEvent::ChangedOnDisk` (session.rs:57), with `WriteError::ChangedOnDisk` (writer.rs:124). `scope_of` (session.rs:557) reacts only to the root, the top-level config folders, `skills/<id>[/references]` and tracked files. So the amendment's "A change to an ordinary file inside a linked project subfolder … raises no `ConfigChangedOnDisk`" already holds in code.
- **"One policy function in am-config decides every allowed root":** today this is `folder_location` plus `refused_location` in `kinds.rs` (private). This sentence was not added by the amendment. The new subtree check has to join that function. Story 1.14 owns it, per the ticket diff.
- **`am-config::is_widening`:** not built yet; session.rs says it arrives in epic 4. The amendment does not invoke it, so nothing is asserted about it.

### 4. Third-party technology

None is named. There are no versions or starters to check.

## Findings

### H1. Names compared exactly let a seat link the config folders under another case

- **Fact (checked on this Mac):** the default APFS volume ignores case. `Crates` opens `crates`, and a folder created as `Seats/` lists as `seats/`.
- **Code:** the loader reads `root.join("seats")` (lib.rs `listing`/`entries`). On this volume that reads a folder whose on-disk name is `Seats/`.
- **Canonicalising does not help:** `realpath(…/SEATS)` returns `…/SEATS`, keeping the typed case.
- **Effect:**
  - A seat file with `folders: [{ id: x, path: SEATS }]` passes an exact-name config-entry check, and the seat can then list and read every seat file.
  - The same applies to `Models`, `Skills`, `Company.yaml` and the rest.
  - The amendment's stated guarantee, "so no seat can reach config files", fails.
- **Precedent:** `.agentic` is already compared ignoring case (`is_legacy_component`, "ignoring case (APFS does)").

**Suggested spine text** (insert after the list of names): "Names compare ignoring ASCII case, as `.agentic` does (APFS ignores case by default), on each component of the path as written and as canonicalised."

### M1. Built code and a valid fixture contradict the amended rule

The spine text is fine; the change belongs in the ticket.

- `kinds.rs` `folder_location` accepts any project-relative subfolder with `ups == 0` and any `access`. That includes `seats`, `models` and `read_write`. Today's code is looser than both the old rule (any folder inside the project refused) and the amended one.
- `fixtures/projects/anatomy/seats/keeper.md` links the project subfolder `work` with `access: read_write`. `crates/am-config/tests/anatomy.rs::a_read_write_folder_adds_write_file` asserts that it validates and grants `write_file`.
- Under the amendment ("`read_write` is refused for any folder inside any project root") that fixture fails `agentic validate` over `fixtures/`, and the test breaks.
- The 1.14 ticket diff adds the refusal test but does not name this fixture or test.

**Suggested fix (Sofia, ticket only):** 1.14's description and verify should say that the anatomy fixture's `work` link becomes `access: read`, and that the `read_write` row of `anatomy.rs` uses a folder outside the project root (a sibling temp folder reached by `../`).

### M2. "Anywhere in the subtree … at every validate and load" has no cost bound and may trigger cloud enumeration

- **Every edit loads:** `load_with` runs on every `ConfigEdit` check (writer.rs:855). Each edit, Reload and startup would walk the whole tree of every linked project subfolder.
- **Projects in cloud storage:** the owner's projects may live in a File Provider folder. AD-30 itself allows `~/Library/CloudStorage/…`, and the project memory records OneDrive projects. There, walking a folder the provider has not listed "may fetch that folder's entries" (AD-30, Cloud-only files), so the walk inherits the download wait and the offline refusal.
- **Today's precedent:** the loader lists only top-level folders.

**Suggested spine text:** "The subtree check reads entry names and types only, never file content. It is capped (for example 10,000 entries or 2 s per linked folder). A scan that does not finish fails closed: the link is a validation error in words naming the cap. At access time the folder tools' per-path refusal is the enforcement."

### L1. "Config entry" mixes root paths and names at any depth

- The definition first lists folders and files "the loader reads", which exist only at the project root (`kind_of`). It then adds "anywhere in the subtree, an entry bearing one of those names".
- "Passes through a config entry" is therefore ambiguous for `files/models/x.pdf`: is a nested `models` a config entry?
- The loader would never read it, so refusing it is a defensive superset. That is fine, but it should be explicit.
- It also refuses common owner folder names (`sources/`, `models/`, `tables/`, `schemas/` in a research folder). The fix wording covers this, but the owner should expect it.

**Suggested spine text:** "A name match counts at any depth of the project-relative path, although the loader reads these names only at the project root; an owner folder named, for example, `sources/` must be renamed or left unlinked."

### L2. The spine's project tree omits the owner data folder the sample relies on

- The "Project folder (owner's, git-friendly)" tree lists only config. The Structural Seed's `samples/company-a/` line lists `.gitignore` and `runtime-seed/` but not `files/d1-inputs/`.
- The spine does not say how a non-config sample folder like `files/d1-inputs/` reaches a project created from the sample. `ConfigCreate` covers config files, and `runtime-seed/` goes to the runtime folder.
- In a project created from the sample, `files/d1-inputs` would warn "does not exist yet" (`seat_folders`).
- The gap predates this amendment, but project subfolders are now the sample's F3 mechanism.

**Suggested spine text:**

- Add to the project tree: `files/ # the owner's own files (any name that is not a config entry); linkable read-only (AD-30)`.
- Add `files/d1-inputs/` to the sample line.
- State that applying the sample copies `files/` into the project as a `ConfigCreate` per file (or as data with sha256 check). Which one is an architect call.

### I1. A config entry that appears later raises no event (informational)

A config-named entry appearing inside a linked subfolder is not tracked, so it raises no `ChangedOnDisk`. The "validation error at the seat file's `folders` line" therefore appears only at the next load, Reload or edit. The per-path tool refusal covers the gap, so this is consistent.

**Optional spine text:** "between loads the folder tools' per-path refusal applies; no `ConfigChangedOnDisk` is raised for it."

## Summary of checks

- No new third-party technology, crate or version: nothing to research online.
- Names against the loader: exact match; no missing folder or root file.
- Company A `files/d1-inputs`: passes.
- `ConfigChangedOnDisk`: exists (`ConfigEvent::ChangedOnDisk`), and its scope already matches the amendment.
- am-config policy function: exists in substance (`folder_location`/`refused_location`); story 1.14 extends it.
- Blocking before adoption: H1 (case). M1 needs a ticket note; M2, L1 and L2 are spine wording.
