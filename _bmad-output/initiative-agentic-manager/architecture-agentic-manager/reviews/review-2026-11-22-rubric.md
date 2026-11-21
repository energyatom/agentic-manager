# Rubric review: AD-30 "Project subfolders" amendment (2026-11-22)

Reviewer: rubric walker (reviewer gate, read-only)
Spine: `architecture-agentic-manager.md`, AD-30 Rule, the part from "**Project subfolders** (amended 2026-11-22, owner)"; context read: AD-4 table, AD-15 widening kinds, AD-16, AD-32, Structural Seed "Project folder (owner's, git-friendly)", Capability map R83/R84, the last four `.memlog.md` entries.
Owner decision (final, not challenged): "A subfolder inside the project may be linked READ-ONLY if it holds no config files. Still refused: the project root, any folder above it, the config folders (seats/, workflows/, tables/, schemas/, models/, skills/, sources/ and the like), .agentic, runtime-seed/, .agentic-open, and any write access inside a project. Symlinks that leave the linked subfolder are refused (as AD-30 already says). agentic validate must flag a linked project subfolder that later gains a config file. The sample stays as it is."

## Verdict

Pass with fixes. The amendment implements every owner clause in substance, F3's `files/d1-inputs/` is linkable under it, and nothing elsewhere in the spine contradicts it. One clause (runtime-seed/ and .agentic as ancestors) is not enforced at link time as written, and three points leave room for two builders to diverge (where the config-name list lives given AD-4, how the subtree walk behaves, and what "the open project" means outside a served project).

## Clause-by-clause check

| Owner clause | Spine text | Result |
|---|---|---|
| Subfolder inside the project may be linked read-only if it holds no config files | "a subfolder strictly inside the open project's root may be linked with `access: read` only when its subtree holds no config entry" | Implemented |
| Still refused: the project root, any folder above it | Pre-existing "a folder that is or contains the open project's root ... is refused"; "strictly inside" | Implemented |
| Still refused: config folders (seats/, workflows/, tables/, schemas/, models/, skills/, sources/ and the like) | Config-entry list adds `dashboards/`, `company.yaml`, `tools.yaml`; "a folder that is or lies inside a config entry is refused" | Implemented for the root-level loader folders ("and the like" = dashboards/, matches the Structural Seed tree) |
| Still refused: .agentic, runtime-seed/ | Names listed only as entries found "anywhere in the subtree" | Partly: a folder that lies inside `runtime-seed/` or an in-project `.agentic/` is not refused at link time (F1) |
| Still refused: .agentic-open | Listed | Implemented (it is a file, so it can never be a linked root; it only matters in a subtree) |
| Any write access inside a project refused | "`read_write` is refused for any folder inside any project root, and `write_file` refuses any path under a project root" | Implemented; consistent with AD-15 (write switch is widening) and AD-46 (`write_file` only for `read_write`) |
| Symlinks leaving the linked subfolder refused | Pre-existing: `WorkspacePath` canonicalises; "a symlink whose target leaves the root is refused and audited" | Implemented |
| validate flags a linked subfolder that later gains a config file | "The check runs at link time and at every validate and load ... a validation error at the seat file's `folders` line naming the entry, with the fix"; folder tools refuse and audit at access | Implemented (as an error, which satisfies "flag") |
| The sample stays as it is | F3 walked below | Passes |

## F3 walk

`reference-workload.md` F3: `d1-inputs` at `files/d1-inputs/` inside the Company A sample (two PDFs, three csv, two PNGs, one docx), linked read-only to D1-E1, D1-E2, D1-E3 and D1-Head.

- `files/d1-inputs/` is strictly inside the project (and, under `agentic validate --template`, inside the template root); neither `files` nor `d1-inputs` is a config name; its subtree holds no config entry; access is `read`. It passes.
- Linking it to D1-E1 and D1-E3 (cloud Accounts) stays privilege-widening (AD-15), which F3 already says ("appears in the sample's import diff").
- The sample's root-level `runtime-seed/` (templates only) is a sibling, not in the subtree; no effect.

## Contradiction search

Searched the whole spine for "project root", "inside the project", "inside a project", "recent project", "config file", "project's root", "linked folder", "folders:", "under a project", "R83".

- AD-26 line 288 (MCP cwd "never the project root"): unrelated, no conflict.
- AD-16 line 228 ("missing parent folders inside the project are created"; every new config file is a `ConfigCreate`, a template's `.gitignore` included): no conflict; `.gitignore` is written by am-config but not read by the loader, so it is correctly not a config entry.
- AD-32 line 324 ("a served folder with no config file at all is a new project"): uses "config file" in the loader sense, consistent with the new definition.
- AD-15 line 222 (linking a folder, widening includes, switching write access on): consistent; write access can never be switched on for an in-project folder.
- AD-46 line 438 and Capability map R83 (am-config `folders`, am-tools): consistent, but see F2 on where the shared predicate lives.
- AD-45 line 432 ("never inside a project"): unrelated.

No contradictions found. Gaps follow.

## Findings

### F1 (High): runtime-seed/ and .agentic are not refused as ancestors, and link time and access time read "config entry" differently

The text defines a config entry as a loader folder or file, "or, anywhere in the subtree, an entry bearing one of those names or `.agentic`, `runtime-seed` or `.agentic-open`; a folder that is or lies inside a config entry is refused". The name clause is scoped to the candidate's subtree, and the "lies inside" clause only covers the root-level loader folders. So:

- `runtime-seed/knowledge/team-d1/` inside a project (a user copy of a template) passes at link time: its subtree has no entry named `runtime-seed`, and `runtime-seed` is not a loader folder. The owner lists runtime-seed/ as still refused.
- An in-project `<project>/.agentic/knowledge/` (a legacy folder not yet moved, or one a user creates) passes the same way. The earlier refusal of "any runtime folder (`.agentic/`, AD-32)" does not catch it, because AD-32 says every `.agentic/<x>` in this document means the runtime folder outside the project.
- `files/seats/notes/` passes at link time (its ancestor `files/seats` is not a loader folder and not in its subtree), but the access-time rule refuses "any path whose project-relative path passes through a config entry". A builder reading "config entry" by name refuses every access; one reading it by the definition allows them. The link works and every read fails, or the reverse.

Suggested text (replace from "a config entry is" to "is refused;"):

> a **config name** is `models`, `seats`, `skills`, `workflows`, `tables`, `schemas`, `dashboards`, `sources` (the folders the loader reads config from), `company.yaml`, `tools.yaml` (the files it reads as config), `.agentic`, `runtime-seed` and `.agentic-open`; a project-relative path is **config-bearing** when any of its components is a config name; a folder is refused when its own project-relative path is config-bearing (so `seats/x/`, `runtime-seed/knowledge/` and `files/seats/notes/` are refused) or when any entry in its subtree has a config name;

and make the access-time sentence: "the folder tools also refuse, and audit, any path whose project-relative path is config-bearing."

### F2 (Medium): the access-time check sits in am-tools, which AD-4 forbids from depending on am-config

The rule says "One policy function in am-config decides every allowed root" and "the folder tools also refuse ... any path whose project-relative path passes through a config entry". The folder tools are in `am-tools`, whose allowed dependencies (Design Paradigm table) are `am-domain`, `am-net` and `am-glob`, not `am-config`. One builder duplicates the name list in am-tools (two lists that drift), another imports am-config and `cargo xtask check-deps` fails. The Binds line also omits am-config, which the memlog entry names.

Suggested text (append after the access-time sentence):

> The config names and the predicate `is_config_bearing(project-relative path)` are one pure policy in `am-domain` (AD-4); am-config's root policy function and the am-tools folder tools both call it, and a test asserts the names equal the set of folders and files the loader reads.

and add to Binds: "`am-config` (root policy, validator), `am-domain` (config-name policy)".

### F3 (Medium): the subtree walk is unspecified (symlinks, letter case, cloud or unreadable folders)

The check walks the linked subtree "at link time and at every validate and load". Not stated:

- whether the walk follows symlinked directories (cycles; a symlink named `seats` pointing inside the root);
- whether names compare case-insensitively. macOS volumes are case-insensitive by default and libc `realpath` corrects case (checked: `realpath SEATS/x` returns `seats/x`), but the walk reads names from directory entries and the Linux CI job is case-sensitive, so `Seats/` or `Company.yaml` in a subtree is treated differently by different builders or platforms;
- whether the in-project test uses canonical paths for both the root and the candidate (a project opened through a symlinked path);
- what happens when the walk cannot finish. AD-30 itself says listing a File Provider folder may fetch its entries and fail offline, so a project in `~/Library/CloudStorage/` can fail the walk at load. One builder fails the whole load, one skips the check, one warns.

Suggested text (after "The check runs at link time and at every validate and load"):

> on canonical paths for the root and the folder, comparing names ASCII-case-insensitively; the walk reads directory metadata only, never follows symlinks and never downloads content; a walk that cannot finish (offline cloud folder, access denied) refuses the link at link time, and at validate and load is a warning naming the folder while the folder tools refuse that root in words until a walk completes.

### F4 (Low): "the open project" is undefined for validate, --template and nested recent projects

- `agentic validate` on a path, and `agentic validate --template` over `templates/` and `fixtures/` (run by `just check`), have no open project. A builder could skip the in-project checks there, so the sample's link would never be checked by CI.
- If the open project itself lies inside another recent project's root, every subfolder of it "lies inside any other recent project's root" and is refused, which contradicts the owner's permission for subfolders of the open project.

Suggested text: replace "the open project's root" in the Project subfolders sentence with "the root of the project being opened, loaded or validated (for `agentic validate --template`, the template or fixture root)", and add: "a folder strictly inside that root is judged only by this rule, even when the root itself lies inside another recent project's root."

### F5 (Low): fix wording for a stray folder name, and the "no seat can reach config files" claim

- Matching by name anywhere in the subtree means a data folder whose own subfolder is called `sources/`, `models/` or `schemas/` (common in research or data folders) is refused, and one that gains such a folder later makes the next load fail validation. This is a fail-closed reading of "and the like" and is acceptable, but the stated fix ("move the entry out, or link a narrower subfolder or one outside the project") omits the simplest fix. Suggested: "(rename or move the entry out, or link a narrower subfolder or one outside the project)".
- "so no seat can reach config files" overclaims: a hard link inside the subfolder to `seats/x.md` is not detected by canonicalisation. Config holds no secrets (AD-16 validator), so the impact is low. Suggested: "so no seat can reach config files by path".
- Minor: "A change to an ordinary file inside a linked project subfolder ... raises no `ConfigChangedOnDisk`" reads as if changes in unlinked non-config subfolders might. Suggested: "A change to a file whose project-relative path is not config-bearing is not a config change and raises no `ConfigChangedOnDisk` (AD-16)."

## Not in scope

No change proposed to `reference-workload.md`, tickets, AGENTS.md or code. The Structural Seed project tree could add one line (`files/  # owner's content; any subfolder without a config name may be linked read-only, AD-30`) so the sample's `files/` folder has a home in the layout; optional.
