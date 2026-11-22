# Review 2026-11-22: owner answers on skill import (AD-38 Skill files and known keys, AD-12 About line, AD-27, seed, Open Questions), versions and reality-check lens

- **Spine:** `architecture-agentic-manager.md`, working-tree change on branch `replan/2026-11-22-skill-answers` (every place marked "amended 2026-11-22, owner answers": AD-38's *Timing*, *Skill files*, *Still not accepted*, *Known keys*, *Acceptance example*, the `read_skill_file` passage and "What an import takes"; AD-12's About line; AD-27's skill part; the Structural Seed comment; four Open Questions entries).
- **Lens:** every fact the amended text commits to, checked today against the live source or the existing project. Nothing below is from memory; where I could not check something I say so.
- **Date of every check:** 2026-11-22, 04:46 to 04:52 UTC, unauthenticated, `User-Agent: curl`, fewer than 10 GitHub API calls in all.
- **Reviewer stance:** read-only. Nothing in the spine, tickets, code or docs was edited. Fetched files and three throwaway copies of `fixtures/projects/valid` lived only in the session scratchpad. No archive was downloaded.
- **Tools:** `curl`; the built `agentic` binary (debug, built 2026-11-22 10:10 local at HEAD `0284120`; `crates/` has no uncommitted change) for `agentic validate` on the scratch copies; `grep` and reading over `crates/`.

## Verdict

**Approve with changes.** No fact in the amended text is false against the live sources: the six keys, the `compatibility` limit, the acceptance example's files, the pinned commit, the built rule and the redirect all check out. Eight sentences need a correction or an added fact. Two matter: "Still not accepted" is wrong for one case the built loader accepts today (C1), and the 20-file cap, now counted over the whole folder, refuses real skills in the format's own example repository and sits one file above a skill in the acceptance repository (C2).

| # | Claim | Verdict |
|---|---|---|
| 1 | Six frontmatter keys; `compatibility` 1 to 500 characters | Confirmed; two wording corrections (C3, C4) |
| 2 | Acceptance example at `22f0b46` | Confirmed in every detail; one added fact (C5); cap fit against real skills (C2) |
| 3 | The built code the text ratifies | Seven claims true, three imprecise (C1, C6, C7) |
| 4 | `api.github.com` answers 302 to `codeload.github.com` | Confirmed |

---

## Claim 1. The open format's frontmatter keys

**What the spine says.** "The frontmatter keys are exactly the six of the open format (checked 2026-11-22 on agentskills.io/specification and its reference validator): `name`, `description`, `license`, `compatibility`, `metadata` and `allowed-tools`. `compatibility` is the one the built loader lacks: optional plain text of 1 to 500 characters."

**What I saw.**

| Source | Fetched | Answer |
|---|---|---|
| `https://agentskills.io/specification.md` | 04:46 UTC, 200, 7,685 bytes | Frontmatter table has exactly six rows: `name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools`. `compatibility`: "Max 500 characters" in the table, "Must be 1-500 characters if provided" in its section. |
| `https://agentskills.io/specification` (HTML) | 04:50 UTC, 200 | Same page. |
| `agentskills/agentskills` `docs/specification.mdx` at `83021e2` (repo HEAD, committed 2026-11-04) | 04:50 UTC, 200 | Same six rows and the same limits as the site copy. |
| `skills-ref/src/skills_ref/validator.py` at the same commit (file last changed 2025-12-18, commit `547831f`) | 04:46 UTC, 200, 5,154 bytes | `ALLOWED_FIELDS = {"name", "description", "license", "allowed-tools", "metadata", "compatibility"}`; `MAX_SKILL_NAME_LENGTH = 64`, `MAX_DESCRIPTION_LENGTH = 1024`, `MAX_COMPATIBILITY_LENGTH = 500`. Any other key is an error ("Unexpected fields in frontmatter"). |

Differences between the specification and its validator, and between both and the spine:

- **`compatibility` lower bound.** The specification says 1 to 500. The validator checks only that the value is a string and `len(...) <= 500`; an empty string passes it. The length is Python `len`, so Unicode characters, not bytes. The spine's "1 to 500" follows the specification, which is the stricter and the safer reading. See C3.
- **`name`.** Specification: 1 to 64 characters, `a-z`, `0-9` and hyphens, no leading, trailing or doubled hyphen, equal to the parent folder name. Validator: the same, but any Unicode lowercase letter or digit is accepted (NFKC-normalised, `isalnum`). The built `checks::is_kebab` is ASCII only, refuses `--` and a leading or trailing hyphen, and has **no length cap**: a 70-character name validated with 0 errors today. See C4.
- **`description`.** Specification and validator: 1 to 1,024 characters. The built loader has **no cap**: a 1,500-character description validated with 0 errors today. See C4.
- **`allowed-tools`.** Specification: one space-separated string, experimental. The built loader also takes a list, or commas. Laxer than the format, not in the amended text, no correction asked.
- **`metadata`.** A map of string keys to string values, in both. The spine's two source-record keys fit it.

**What the format says about files and folders** (bearing on *Skill files*):

- "A skill directory may contain any files and directories beyond the required `SKILL.md`." The named folders (`scripts/`, `references/`, `assets/`) are "recommendations". So text files anywhere in the folder are within the format, and the spine's rule is a narrower subset of it (text types only; no scripts; nothing from `assets/` but text).
- `assets/` is described as templates, images and data files. The spine does not name it. Under the rule its text files are skill files and its images, fonts and PDFs are left out as "any other extension". That is consistent; a builder should not treat `assets/` as special.
- "Keep file references one level deep from `SKILL.md`. Avoid deeply nested reference chains." This is about one file pointing to another, not folder depth. The format sets no folder-depth limit, so the spine's 4 segments is the project's own cap. Deepest real case I saw: 4 segments, in `anthropics/skills` `claude-api`; in obra/superpowers, 2.
- The reference parser also accepts a lowercase `skill.md` ("Prefers SKILL.md (uppercase) but accepts skill.md"). The specification text names only `SKILL.md`, as the spine does. No change asked; a zip that holds only `skill.md` will be reported as holding no skill.

**Verdict.** Confirmed. Six keys, 500 characters, and "any other key is refused" all match the reference validator. Corrections C3 and C4.

---

## Claim 2. The acceptance example

**What the spine says.** "(checked 2026-11-22 at obra/superpowers commit `22f0b46`): `brainstorming` imports `SKILL.md`, `builder-check-prompt.md` and `visual-companion.md`, and its `scripts/` folder of five files is named as not imported; `writing-plans` holds `SKILL.md` only. Both import complete in their text files and then pass `agentic validate`."

**What I saw** (`api.github.com/repos/obra/superpowers/commits?per_page=1` and `.../git/trees/HEAD?recursive=1`, 04:47 UTC, both 200, tree not truncated, 308 entries).

- HEAD of the default branch is still `22f0b463e7a5e579904dd6211a520321035526f7`, committed 2026-11-03T22:17:07Z, "Release v7.0.2".

| Path under `skills/` | Mode | Bytes |
|---|---|---|
| `brainstorming/SKILL.md` | 100644 | 15,190 |
| `brainstorming/builder-check-prompt.md` | 100644 | 881 |
| `brainstorming/visual-companion.md` | 100644 | 13,413 |
| `brainstorming/scripts/frame-template.html` | 100644 | 8,094 |
| `brainstorming/scripts/helper.js` | 100644 | 5,615 |
| `brainstorming/scripts/server.cjs` | 100644 | 25,699 |
| `brainstorming/scripts/start-server.sh` | **100755** | 7,315 |
| `brainstorming/scripts/stop-server.sh` | **100755** | 3,263 |
| `writing-plans/SKILL.md` | 100644 | 10,335 |

- Frontmatter (raw files at the full SHA, 04:48 UTC): both hold exactly `name` and `description`. `name: brainstorming`, `name: writing-plans`; descriptions of 213 and 84 characters. All 15 skills in the repository hold only those two keys (13 more raw fetches).
- SKILL.md bodies: 14,933 and 10,209 bytes; the cap is 65,536. The largest body in the repository is 19,599 bytes.

**Judged against the new rule.**

| Rule | brainstorming | writing-plans |
|---|---|---|
| Keys within the six | yes (2) | yes (2) |
| `name` a kebab-case id, equal to the folder | yes | yes |
| Body at most 64 KB | 14,933 B | 10,209 B |
| Skill files at most 20 | 2 | 0 |
| Each at most 256 KB | 881 B, 13,413 B | none |
| Path at most 4 segments and 255 characters | 1 segment each | none |
| Extension, executable bit, dot-name, link | both `.md`, mode 100644, no dot-name, no link | none |
| Left out | `scripts/`, five files (two executable) | nothing |

No text file in either skill falls foul of any rule. Only `scripts/` is left out.

**Checked with the built validator too.** I put the four real text files into scratch copies of `fixtures/projects/valid` and ran the built `agentic validate`:

- `writing-plans` as it is upstream: 0 errors today.
- `brainstorming` with its two `.md` files beside `SKILL.md`: 2 errors today, both "file '…' is not accepted in a skill folder". That is exactly the placement rule the amendment changes, and nothing else.
- `brainstorming` with the same two files moved under `references/`: 0 errors. So the real files already pass the built frontmatter parser, the key list, the name check, the body cap and the secret checks.

**Two facts the example does not state.**

- `visual-companion.md`, which is imported, tells its reader to run `bash scripts/start-server.sh` (six places) and `bash scripts/stop-server.sh`, and points at `scripts/frame-template.html` and `scripts/helper.js`. None of those is imported. "Complete in their text files" is true as written, but one imported file's instructions rest on files that are left out. This is the same concern the old Open Question (2) raised, moved from text files to scripts. See C5.
- Both SKILL.md bodies name other skills as `superpowers:<name>` (`writing-plans` names three that the example does not import). Information only.

**Verdict.** Confirmed in every detail: names, the count of five, "SKILL.md only", and both skills import complete in their text files. Corrections C5 (pin the full SHA, state the scripts dependency) and C2 (the cap against other real skills).

**The cap against other real skills** (the same tree, and `anthropics/skills` at `e4bb5d9`, 04:51 UTC; this is the repository the format's own site points to for examples).

| Skill | Skill files under the rule | Result under the rule |
|---|---|---|
| obra/superpowers `diagnosing-superpowers` | 19 (in `prompts/`, `references/`, `templates/`) | imports; one more file and it cannot |
| obra/superpowers `subagent-driven-development` | 10, plus `scripts/` of 4 | imports |
| anthropics/skills `canvas-design` | 28 (27 font licence `.txt` files and `LICENSE.txt`) | **cannot be imported** (over 20) |
| anthropics/skills `claude-api` | 91 (90 `.md`, 1 `.txt`); one file is 346,255 bytes | **cannot be imported** (over 20); the large file alone would be left out |
| anthropics/skills, the other 17 | 0 to 11 | import |

All 20 SKILL.md files in `anthropics/skills` hold only `name`, `description` and, in 17 of them, `license`. No key outside the six was seen in either repository.

---

## Claim 3. The built code the text ratifies

Read: `crates/am-config/src/skills.rs` (all 237 lines), `SKILL_KEYS` and `kinds::skill` in `crates/am-config/src/kinds.rs`, `Loader::listing`, `Loader::read` and `tracked` in `crates/am-config/src/lib.rs`, `checks::is_kebab`; `grep` over `crates/` and `web/src`; story 9.16 in `tickets/epic-tools-mcp-search-doctor/tickets/tickets.toml`. Probed with the built binary on a scratch copy.

| # | The spine says | What the code does | Verdict |
|---|---|---|---|
| a | The built rule is `SKILL.md` plus a flat `references/`, five keys | `load_one` refuses every folder but `references` and every file but `SKILL.md`; `load_references` refuses a folder inside; `SKILL_KEYS = [name, description, license, metadata, allowed-tools]` | **True** |
| b | `.yml` is read as yaml, as built | `REFERENCE_EXTENSIONS` holds `yml`; `structured_checks` treats `yaml` and `yml` alike. Probe: `references/x.yml` accepted. The extension is matched without regard to case (`UP.MD` accepted). | **True**; the case rule is unstated |
| c | Only `.DS_Store` is skipped unreported, as built | `Loader::listing` skips exactly that name; `tracked` excludes it. Probe: no finding at the skill root or in `references/`. | **True** |
| d | A dot-entry is "still" not accepted | At the skill root, yes (`.gitkeep` refused as any other file). In `references/`, a dot-named folder is refused, but a dot-named file with a text extension is **accepted**: probe `references/.hidden.md`, no finding. | **Imprecise** (C1) |
| e | Reference files count in the snapshot hash and get the secret checks | `Loader::read` puts every file it reads into `self.files`, and the hash is `content_hash(&self.files)`; each reference gets `text_secrets`, and yaml, yml and json also get the key-name and comment checks. A refused file (extension, size, link, executable) is not read and is not in the hash. | **True** |
| f | `compatibility` is the one key the built loader lacks | Probe: "unknown key 'compatibility' in SKILL.md … keys allowed here: name, description, license, metadata, allowed-tools" | **True** |
| g | No total cap, "as before"; 20 × 256 KB = 5,120 KB | No total cap in the code. The arithmetic is right. The built count is of every regular file in `references/`, accepted or not; the new count is of skill files. | **True** |
| h | Epic 1 story 12 built the loader and validator | Commit `d31b7ea` "Story 1.12: seat anatomy in the config schema"; the ticket text says so. | **True** |
| i | `read_skill_file` is not built; "Epic 9 story 16 builds the tool" | No executor exists: nothing in `am-tools`, `am-engine`, `am-server` or `am-app`. But the name and the grant are built: `am_domain::capability::SKILL_TOOL = "read_skill_file"`, and `am-config` grants it to every seat with an attached skill and refuses it listed without one (`kinds.rs` 1933 to 1947; tests in `crates/am-config/tests/anatomy.rs` and `skills.rs`). | **Imprecise** (C7) |
| j | The import is not built; story 9.16 builds it | No `SkillSource`, no `api.github.com` or `codeload.github.com`, no `skill_import_*` test, no `skills import` command, no About page in `crates/` or `web/src`. Story 9.16's text names all of them. | **True** |
| k | "`am_config::is_widening` classifies it as not widening, like the other skill keys" | `is_widening` does not exist in `crates/` (zero hits). AD-15 itself says "From story 17 the writer classifies the patch with `is_widening`". The sentence is a rule for a function not yet written, phrased as present fact. | **Imprecise** (C6) |

**For the builder of 9.16, from the same reading** (no spine change asked):

- `tracked()` in `lib.rs` matches only `skills/<id>/references/<name>`; the session will not see a skill file elsewhere until it changes.
- `docs/config-schema.md`, section "Skills", restates the built rule and the five keys; it changes with the loader.
- No fixture or template skill in the repository holds a dot-named entry, a link or a `compatibility` key today, so the new rule breaks none of them.

**Story 9.16's text against the spine** (two wording mismatches, for Sofia):

- The story says "the open Agent Skills format's newer frontmatter keys that AD-38 names (for example `compatibility`)" and its verify says "each newer known frontmatter key". The format has one such key. The spine says so: six keys, `compatibility` the one added.
- The story says "within AD-38's total caps". The spine says "There is no separate total cap". The caps are the per-file 256 KB, the 20 files, and the 4 segments and 255 characters, none of which the story names.

---

## Claim 4. The two hosts in the About line

**What the spine says.** "Skill import contacts api.github.com and codeload.github.com, and only when you import a skill from a GitHub URL."

**What I saw** (04:49 UTC, `curl -sI -H "User-Agent: curl"`, so `HEAD` requests; nothing downloaded).

| Request | Answer |
|---|---|
| `https://api.github.com/repos/obra/superpowers/zipball/22f0b463e7a5e579904dd6211a520321035526f7` | `HTTP/2 302`, `content-length: 0`, `location: https://codeload.github.com/obra/superpowers/legacy.zip/22f0b463e7a5e579904dd6211a520321035526f7` |
| that `location` | `HTTP/2 200`, `content-type: application/zip`, `content-disposition: attachment; filename=obra-superpowers-v7.0.2-0-g22f0b46.zip`; no further redirect |

**Verdict.** Confirmed. Two hosts, one hop, no third host. Not checked: that the zip entries carry the Unix mode the rule judges an executable by, because that needs the archive itself; the recorded fixture should assert it (`start-server.sh` and `stop-server.sh` are 100755 in the tree).

---

## Corrections needed

### C1. "Still not accepted" is wrong for a dot-named text file in `references/` (imprecise; changes behaviour for existing projects)

The built loader reads and accepts `references/.hidden.md` with no finding. The amended rule refuses it, under a heading that says nothing changes. A project that validates today could fail after story 9.16 with no line in the spine saying why.

*Proposed rewording:* "*Not accepted* (the first four as built; the rest from the same commit): a folder named `scripts` at any depth, with everything in it; a file with an executable bit; a link; any other extension; an entry whose name starts with `.`, with everything in it (new for a dot-named text file inside `references/`, which the built loader reads; only `.DS_Store` is skipped unreported, as built); …"

### C2. The 20-file cap now refuses real skills, and the text presents it as unchanged (flag for the owner; add the checked fact)

"The reference cap, now counted over the whole folder" and "as before" are true of the number, not of its reach. Checked today: `canvas-design` (28 skill files) and `claude-api` (91) in `anthropics/skills` cannot be imported at all, because the import "never chooses which to keep"; `diagnosing-superpowers` in the acceptance repository holds 19, one under the cap. The owner's tick list for `github.com/obra/superpowers` will show a skill as not importable the day it gains two files.

*Proposed rewording, if the cap stays:* add after the caps sentence: "Checked 2026-11-22: of the 15 skills in obra/superpowers the largest holds 19 skill files; in anthropics/skills two of 19 hold more than 20 (28 and 91) and cannot be imported in v1. The cap is a v1 limit, not a property of the format." And add an Open Questions line for the owner: "whether 20 skill files per skill is enough, given the count is now over the whole folder." Raising the number is the owner's and the architect's call; I only report that the present one is already exceeded in the wild.

### C3. `compatibility`: say what the two sources each enforce, and what a character is (imprecise)

The text cites "agentskills.io/specification and its reference validator" for "1 to 500 characters". Only the specification sets the lower bound; the validator enforces the upper bound alone, and counts Unicode characters.

*Proposed rewording:* "optional plain text of 1 to 500 characters, counted as Unicode characters as `goal` is (the specification sets both bounds; its reference validator enforces only the upper one, and the app enforces both)."

### C4. The format's two other limits are neither adopted nor ruled out (gap; bears on "a private format that cannot travel")

The amendment takes the format's limit for `compatibility`. The built loader enforces neither `name` at most 64 characters nor `description` at most 1,024 (both probed today: 70 and 1,500 characters validate). A skill written in the app can therefore fail the format's own validator. The spine already cuts listed names and descriptions to 64 and 1,024, so it knows the numbers.

*Proposed rewording (architect to rule which):* add to *Known keys* either "From the same commit the validator also enforces the format's lengths: `name` at most 64 characters and `description` 1 to 1,024." or "The format's length limits on `name` (64) and `description` (1,024) are not enforced in v1; a longer one validates here and fails the format's reference validator."

### C5. Acceptance example: pin the full SHA, and state that an imported file points at the left-out scripts (added fact)

`22f0b46` is seven characters; the rest of AD-38 demands 40. `visual-companion.md` instructs its reader to run the left-out scripts.

*Proposed rewording:* "*Acceptance example* (checked 2026-11-22 at obra/superpowers commit `22f0b463e7a5e579904dd6211a520321035526f7`, then the default branch's HEAD): `brainstorming` imports `SKILL.md`, `builder-check-prompt.md` and `visual-companion.md`, and its `scripts/` folder of five files is named as not imported; `visual-companion.md` refers to those scripts, so that part of the skill cannot work in v1, and the "Not imported" list is the owner's notice of it. `writing-plans` holds `SKILL.md` only. …" The owner's one live run reads whatever HEAD is that day; the recorded fixture is what stays true.

### C6. `is_widening` is not built; "like the other skill keys" cannot be read off any code (imprecise)

*Proposed rewording:* "and `am_config::is_widening`, when story 17 builds it (AD-15), classifies it as not widening, as it must every skill key."

### C7. "Epic 9 story 16 builds the tool": the tool's name and grant are already built (imprecise)

*Proposed rewording:* "Epic 9 story 16 builds the tool itself (its name and its grant to every seat with an attached skill are built, epic 1 story 12), and the same tool reaches every skill file …"

### C8. One left-out `scripts/` folder: named once, or file by file? (two sentences disagree)

The unamended sentence says an import "leaves it out and names each file". The amended ones say a `scripts` folder is left out "with everything in it" and, in the example, "its `scripts/` folder of five files is named as not imported". The fixture and the "Not imported" list need one answer.

*Proposed rewording:* in *Still not accepted*, after "an import leaves each out and names it (below)": "A left-out folder is named once with its file count; a left-out file is named by its path." And change the earlier "names each file" to "names it".

---

## Evidence kept

Scratchpad only, not in the repository: the specification copy (sha256 `bf4137a7…d19738`), `validator.py` (sha256 `b5ee3d85…645f6c5`), `parser.py`, `models.py`, the two repository trees as JSON, the 15 superpowers SKILL.md files and three other text files at the pinned commit, and the three scratch projects the built validator ran on.
