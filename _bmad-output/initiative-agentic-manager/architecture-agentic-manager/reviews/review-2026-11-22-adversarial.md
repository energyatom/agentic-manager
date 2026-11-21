---
review: adversarial (pairwise incompatibility and security boundary)
target: architecture-agentic-manager.md, AD-30 "Project subfolders" (amended 2026-11-22, owner) and what it touches: AD-16 (ConfigChangedOnDisk, ConfigCreate), AD-32 (runtime folder, template apply, project roots), AD-4 / Design Paradigm (crate dependencies), AD-40 (folder fan-out), Structural Seed (project tree, templates/, fixtures/)
date: 2026-11-22
method: read only. For each clause of the amendment, build two units one level down (crates or stories) that each follow every AD as written and still build incompatibly, or let a seat read config or runtime state through a linked in-project subfolder. The owner decision is final and is not challenged here. Fixes are spine-text only; reference-workload.md, tickets and code are out of scope.
---

# Adversarial review: AD-30 project subfolders (2026-11-22)

## Verdict

The owner's rule is the right one, and flow F3's `files/d1-inputs/` passes it as written. The spine's version still has three holes that are serious enough to block the build:

- **No single owner.** The "one policy function in am-config" cannot be called by `am-tools`, which must re-check at access time. So the rule will be written twice.
- **Checks go by path text.** The checks compare path strings that are checked and then used later. Renames, symlink swaps, case variants and hard links on APFS can all put a config or runtime file under a linked root.
- **"Any project root" is not defined.** So "no write access inside a project" depends on the recent-projects list.

One gap breaks the sample. The spine gives the F3 folder two homes and no way to reach a created project. Both of the sample's homes must change together, or F3 stops being valid.

Severity: **C** critical (breaks a trust rule), **H** high (two correct units cannot be joined, or the boundary leaks), **M** medium, **L** low.

---

## B1 (H): The policy has two owners: `am-tools` cannot call am-config, and the config-entry list is separate from the loader's

**Unit A:** `am-config` (AD-30: "One policy function in am-config decides every allowed root"). It runs the subtree scan and the in-project rules at link time, validate and load.
**Unit B:** `am-tools` (`list_files`, `read_file`). AD-30 says "the folder tools also refuse, and audit, any path whose project-relative path passes through a config entry". The Design Paradigm table lets adapters depend on `am-domain`, `am-glob` and `am-net` only. `am-tools` cannot call `am-config`, so its builder writes a second implementation of "config entry" and "passes through". Both units follow AD-4, and the two lists drift apart.

**Second pair, same root cause:** the loader (am-config) reads config from the folders in the Structural Seed's project tree. The list in AD-30 is a separate copy, made by hand. The owner's words were "and the like". Suppose a later story adds a config folder (a `teams/` or `hats/` file, a `company.yml` spelling, `.gitignore` handling) to the loader and does not add it to AD-30's list. That folder then becomes linkable, and nothing fails.

**Fix (tighten AD-30, one sentence plus a Design Paradigm note):** "The config-entry name set and the root policy are one pure function in `am-domain` (`FolderPolicy`, beside `WorkspacePath` and `RuntimeLayout`). It takes the facts gathered by the caller (canonical paths, file identities, the names found in a subtree scan) and returns a verdict. `am-config` calls it at link, validate and load. `am-tools` calls it on every access. The loader reads config only from entries in that set, and a test fails when the loader opens a path outside it." Then "One policy function in am-config" becomes "One policy function in am-domain, called by am-config and am-tools".

## B2 (H): Check by path string, use by path: renames, symlink swaps, case variants and hard links reach config and runtime state

Each of these units follows AD-30 as written:

1. **A rename between check and use.** `read_file` canonicalises `files/d1-inputs/a/x.pdf`, and the project-relative path passes the check. Before the open, a sync client renames `files/d1-inputs/a` and puts a symlink to `../../seats` in its place. The sync client could be OneDrive or Dropbox: AD-30 supports projects under CloudStorage, so a collaborator or a compromised cloud account can write into the folder. Inside one project this is a single atomic rename on one volume. The check passed, and the open reads a seat file. The amendment makes this worse: before it, config was never one rename away from a linked root.
2. **A case or normalisation variant.** APFS is case-insensitive and normalisation-insensitive by default. Take a link `path: Seats/notes` or `files/../SEATS/x`, or an entry named `Models/` or `Company.YAML` inside the subtree. Compared as case-sensitive strings, these fail "is or lies inside a config entry" and the name scan, yet they open the real `seats/` and `models/`. The Globs convention makes include and exclude case-insensitive, but AD-30 does not say how the config-entry name check compares names, so the two matchers disagree.
3. **A hard link.** A hard link to `seats/d1-e1.md`, `company.yaml` or the runtime folder's `state.db` is placed in `files/d1-inputs/` as `notes.md`. It lands on the same volume as `~/Library/Application Support`, and an MCP server running as the owner, or the owner by mistake, can create it. The name scan and the symlink rule both pass it, so the seat reads config, or the whole store, which skips AD-22's grants. This also works in reverse for `read_write` links outside the project: `write_file` "creates or replaces" a file. If a builder writes in place (open, truncate, write), a file there that is a hard link to a seat file gets rewritten, and this can happen from a tainted step. The result is a non-widening outside edit that the owner applies with a single Reload, which launders taint into seat instructions (AD-10).
4. **The linked root is re-pointed.** `files/d1-inputs` is itself a symlink, or the folder is replaced after link time, so it now points into `seats/` or into another project. The access-time check AD-30 names is only "passes through a config entry". It does not re-run the full root policy, so a second builder accepts the re-pointed root.

**Fix (tighten AD-30's WorkspacePath sentence and the access-time sentence):** "Folder tools resolve a path by handle. They open the linked root once by its pinned identity (volume and file id, recorded at load; a root whose identity changed is refused until Reload). They walk each component with `openat` and `O_NOFOLLOW`. A symlink is resolved only when its target stays under the root, and it is re-checked as a fresh path. They read from the opened handle, never by re-opening the path. On every access the same `FolderPolicy` as at link time runs on the canonical path and on the opened file's identity. Config entries and the project root are matched by file identity, and names are compared case-insensitively and normalisation-insensitively on case-insensitive volumes. Under a project root, a regular file with a link count above 1 is refused and audited. `write_file` writes a temp file and renames it into place, never opens an existing file for writing, and refuses to replace a file with a link count above 1."

## B3 (H): "Any project root" has no definition, so nested or unknown projects keep write access

**Unit A** reads "`read_write` is refused for any folder inside any project root" as "inside any root on the recent-projects list", because AD-30's earlier clause speaks of "any other recent project's root". **Unit B** reads it as "inside any folder that holds `company.yaml`". Both are valid readings.

Here is the attack. Project B is nested inside project A, at `A/files/b/`, or A is a cloned repo that was never opened on this Mac. B links `../../seats` (canonicalised, so it is outside B) as `read_write`. Unit A finds A on no recent list and allows the link. A seat in B can now rewrite A's seat instructions. When A opens, this shows up as a non-widening outside edit that needs one Reload. That breaks the owner's "any write access inside a project". The same gap lets a `read` link reach A's `models/` and `seats/`.

**Fix (tighten AD-30):** "A project root, for every AD-30 check, is the open project's root, every recent project's root, and every folder holding `company.yaml` or `.agentic-open`. The checker finds the last kind by walking up from the candidate to its volume root at link time, validate, load and access. A candidate inside such a root follows the project-subfolder rules relative to the nearest enclosing root, and a candidate whose subtree holds `company.yaml` is refused as containing a project." This keeps F3 valid: Company A's only enclosing root is its own.

## B4 (H): The F3 folder has two homes and no defined way into a created project

- The Structural Seed puts the F3 folder at `fixtures/folders/` ("synthetic mixed-type folder for flow F3"). Its `templates/samples/company-a/` line does not mention `files/`. reference-workload.md puts it at `files/d1-inputs/` inside the sample, linked as a project subfolder.
- Builder A puts it under `fixtures/folders/` and links it through `../folders/...`, which counts as an outside folder. Builder B puts it at `files/d1-inputs/`. Their seat files and golden snapshots differ.
- The project tree in "Project folder (owner's, git-friendly)" has no entry for owner data folders, so `files/` looks like something unknown.
- When the wizard creates a project from the sample, AD-16 says "Every new config file is a `ConfigCreate`" and "Only am-config ... writes project files". A PDF in `d1-inputs` is not a config file, so it is unclear who writes it:
  - A raw copy in `am-app` breaks AD-16.
  - A `ConfigCreate` puts binary data into History diffs.
  - Skipping it leaves the four seats' `folders:` lines invalid ("must be an existing directory"). That breaks the sample, which the owner said stays as it is.

**Fix (Structural Seed, AD-16 and AD-32, one clause each):**
- **Seed:** `samples/company-a/` gains "`files/d1-inputs/` (the F3 folder, linked in place read-only as a project subfolder, AD-30)". `fixtures/folders/` becomes "variants of the F3 folder for engine tests". The project tree gains "any other folder (for example `files/`): owner data, never read by the loader; linkable read-only (AD-30)".
- **AD-16:** "A template's non-config files are written by the same apply as `ConfigCreate`s (create without replace, sha256-checked). History records them by name and hash only, with no diff. They are written before the seat files that link them."

## B5 (M): A later config entry is flagged, but the watcher, the size of the error and the listing are undefined

- **Watcher.** AD-30 says an ordinary file change in a linked subfolder "raises no ConfigChangedOnDisk", and that a config entry appearing there "makes the next load fail validation". Neither AD says what triggers that next load:
  - Builder A's notify watcher ignores the subtree completely, so the owner learns nothing until a restart.
  - Builder B raises ConfigChangedOnDisk on any change, which floods the banner with noise.
- **Size of the error.** One builder fails the whole snapshot: the next startup refuses the project because someone dropped a data folder named `models/` or `sources/` into `d1-inputs`. Another builder only disables the link.
- **Listing.** `list_files` and AD-40's folder fan-out over a root that now holds `seats/` could fail the whole listing, leave the entries out, or list their names and so leak config file names.
- **The scan.** The scan can walk symlinks: `docs -> /` gives a huge or looping walk. It can make a cloud provider list a dataless folder, which brings back the download wait and the offline refusal. It can also apply or ignore `include` and `exclude`.

**Fix (one AD-30 sentence):**
- **Watcher:** the config watcher raises ConfigChangedOnDisk only for config entries. Under a linked project subfolder, that means only an entry whose name is in the config-entry set.
- **Size of the error:** the resulting validation error disables that seat's link: its folder tools refuse, and steps over it fail in words. It does not fail the rest of the snapshot.
- **Listing:** `list_files` and folder fan-out leave out every entry at or under a config entry, and report one audited count.
- **The scan:**
  - It uses `lstat` and never follows a symlink.
  - It ignores `include` and `exclude`.
  - It never materialises a dataless folder. An unlisted cloud folder counts as unscanned, and so does a scan past 100,000 entries. An unscanned folder refuses the link in words.

## B6 (L): `.git` is linkable and holds config history and, often, credentials

`.git/` passes the name scan. `.git/config` often carries a remote URL with a token embedded in it, and `.git/objects` holds every past version of every config file. Linking `.git` read-only, or a subfolder whose subtree holds a `.git` (a nested checkout), sends both to the seat's model, possibly a cloud model. The owner's "and the like" covers this.

**Fix:** add `.git` to the names in AD-30's config-entry definition: "or, anywhere in the subtree, an entry bearing one of those names or `.agentic`, `.git`, `runtime-seed` or `.agentic-open`". F3's `d1-inputs` has no `.git`, so it stays valid.

---

## Checked and sound

- F3's `files/d1-inputs/` (PDFs, csv, PNGs, one docx) holds no config entry, so it can be linked read-only to D1-E1, D1-E2, D1-E3 and D1-Head under the amended text.
- Linking the root, a folder above it, or a folder inside `seats/`, `models/` and the other config folders is refused as written, provided names and identities are compared as B2 says.
- A symlink that leaves the linked root is refused, as before. B2 only adds resolution by handle.
- The rule that a change to an ordinary file in a linked subfolder raises no ConfigChangedOnDisk is consistent with AD-16, once B5 states the positive rule.
- `write_file` refusing anything under a project root agrees with the AD-32 rule that `.agentic-open` is the only runtime file in a project. No other engine writer targets a project subfolder: drop, knowledge, records and workspace folders are all in the runtime folder.
