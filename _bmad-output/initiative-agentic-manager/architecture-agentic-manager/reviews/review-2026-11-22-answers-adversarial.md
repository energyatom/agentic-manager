# Adversarial review — owner answers amendment (AD-38 skill files and known keys, AD-12 About line, AD-27), 2026-11-22

Reviewer lens: adversary. Two builders one level down who each obey every amended sentence, and a hostile skill author on GitHub.

**Verdict: pass-with-fixes.** The amendment grants nothing, runs nothing and adds no host. Its weak side is that one rule now has four enforcers and several doors, and the text does not yet make them agree. 0 critical, 5 high, 7 medium, 6 low.

No finding is critical: none lets a skill gain a tool, a host, a cap or code execution. The worst outcomes are text the owner did not see reaching a seat's instructions or trusted inputs (H3, H4, H5), bounded by that seat's existing grants, and an import that leaves the project failing `agentic validate` (H1, H2).

## What was read

- Spine `architecture-agentic-manager.md`: the diff of this update; AD-38 (line 368) and AD-12's skill-import part (line 204) in full; AD-27 (line 294); the seed comment (line 591); Open Questions (lines 807 to 810); AD-10, AD-11, AD-13, AD-15 and AD-30 where they bear on skills, imports, taint and archive caps; the Design Paradigm row for adapters (line 40).
- Built code: `crates/am-config/src/skills.rs`, `SKILL_KEYS` and `fn skill` in `kinds.rs`, `Loader::listing` and `Loader::read` in `lib.rs`, `am-domain/src/config_names.rs`, `project_root_above` in `kinds.rs`.
- Tickets, only to see who builds what: 1.12, 9.16 as restated, the prompt-assembly story, the department export and import story, the import dialog story, the Agent card story.
- Not done: no network. Where I say what git or APFS allows, it is from knowledge, not re-checked today.

## Attacks the text already closes

| Attack | Closed by these words |
|---|---|
| Swap the source between review and apply | "The Proposal holds the full text and sha256 of every file it would write; apply writes exactly those bytes and never re-reads the source folder, zip, archive or network." |
| A file the skill import's diff did not show is later listed or served | "whose diff shows SKILL.md and every skill file in full" and "it serves only a file the loader accepted". Open through another door: H5. |
| A second `SKILL.md` by letter case (`skill.md` beside `SKILL.md`) | "a skill folder in which two entries have the same path ignoring case and Unicode normalisation" cannot be imported. Holds for ASCII; beyond ASCII see H3. |
| Replace a skill seats already attach | "an id that already exists in the project or appears twice in one import"; "v1 imports create a skill and never replace one". |
| Scripts, executables, links, `.claude-plugin/`, `.claude/`, `.mcp.json`, `.github/` | *Still not accepted*: "a folder named `scripts` at any depth, with everything in it; a file with an executable bit; a link; any other extension; an entry whose name starts with `.`, with everything in it", and "No file in a skill folder is ever run." |
| Deep nesting | "at most 4 segments (3 folders and the file name)". |
| A `SKILL.md` in a subfolder to carry a second body | "a folder below the root that directly holds a `SKILL.md` (another skill), with everything in it". Spelling and entry kind are open: M4. |
| A file named `scripts.md` | The rule names "a folder named `scripts`", so the file is an ordinary skill file. Consistent. |
| Zip entry whose size lies | "with the size cap checked on what is expanded". |
| Zip entry whose mode hides a script as plain text | It lands as text and "No file in a skill folder is ever run." The mode apply gives the file is unsaid: L4. |
| More than 10,000 entries | "It lists the entry names (at most 10,000; more is refused)". |
| `compatibility` as a grant or in a prompt | "It is information only: nothing reads it to grant or choose a tool, host, cap or model; it is not placed in a prompt; ToolGate, the egress client and the "what this seat can touch" panel never read it; and `am_config::is_widening` classifies it as not widening". |
| `allowed-tools` as a grant | "`allowed-tools` is parsed and shown as "asks for", compared with the seat's grants, and a missing tool is offered only through the normal Tools & connectors path and its privilege-widening confirmation." |
| `metadata` as a grant | "**A skill grants nothing:** ToolGate, the egress client and the "what this seat can touch" panel never read skills". Reaching a prompt is closed only by AD-27's "each SKILL.md body": L1. |
| A new frontmatter key slips through | "Any other key is still refused, so a key the format adds later needs its own amendment." |
| The About line shows other hosts | "The page reads the two names from the core over `/api/v1`, whose one source is `am-net`'s constant, never from a literal in `web/src`", and "The set is not in config, so no setting, patch, import or Reload can add to it." |
| The skill author changes what the confirm step shows as hosts | "The Proposal carries the URL, the commit and the two hosts as its source reference (AD-15); the confirm step, the History entry and the audit entry read them from there." The author controls none of it. Where the Proposal gets the names is unsaid: L2. |

## High

### H1 — The skill-file rule has four enforcers in crates that cannot call each other, and no single home

- **Spine (line 368):** "*Skill files.* … text files of the same types (md, txt, csv, json, yaml; `.yml` is read as yaml, as built) anywhere in the folder"; "expands only the `SKILL.md` and skill-file entries … (the skill-file rule above, judged from each entry's name, type and mode, with the size cap checked on what is expanded)"; "it serves only a file the loader accepted".
- **Fact.** The rule's constants are private to `am-config/src/skills.rs`. `am-extract` and `am-tools` are adapters, and adapters depend on `am-domain` only (line 40). So the child that picks zip entries cannot call the validator. The rule will be written at least twice: in the child and in the validator, and again in the import's "left out and named" sorter.
- **Two builders, both to the letter.**
  - Extension case. The built loader lowercases the extension, so `NOTES.MD` is accepted. The spine says "as built" only for `.yml`. A child that matches `.md` exactly leaves `NOTES.MD` out as "any other extension". The reverse build writes a file the other refuses.
  - The size. The loader's 256 KB is 262,144 bytes. A child using 256,000 leaves out a file the validator accepts.
  - `Scripts/notes.md`, `sub/skill.md`: see M4.
- **Result.** Either a skill imports and then fails `agentic validate` after apply (the amendment's own Prevents line: "an imported skill the validator then refuses"), or a file the validator accepts is silently named "not imported".
- **Precedent in the spine and the code.** AD-30 already does this for config names: "one pure policy in `am-domain` (AD-4), called by am-config's root policy and validator and by the `am-tools` folder tools, and a test asserts the names equal what the loader reads" (`am-domain/src/config_names.rs`).
- **Proposed text (AD-38, after *Still not accepted*):** "*One rule, one place.* The skill-file rule (the accepted types, the caps and *Still not accepted*) is one pure function in `am-domain`, beside the config-name policy, over a skill folder's entries, each given as its path below the skill folder, its kind (file, folder, link, other), whether it is executable, and its size when known. It answers for each entry: accepted, left out with its reason, or stops the skill. `am-config`'s validator, the `__extract` child and the import call it and add no check of their own. An extension is compared ignoring ASCII case, as built. 256 KB is 262,144 bytes. Before it creates a Proposal, the import runs `am-config`'s loader over exactly the files the Proposal would write, and creates the Proposal only when that skill has no error. A test gives one fixture tree to the validator on disk and, zipped, to the child, and asserts the same accepted and left-out sets."

### H2 — The three outcomes of an import do not cover every validator error; a file that is not UTF-8 is the plain case

- **Spine (line 368):** left out and named is "everything under *Still not accepted* above"; cannot be imported is "a SKILL.md the validator refuses (…); a skill file the secret check refuses; a skill folder with more than 20 skill files; … the same path …; an id that already exists". "Text file" is defined by extension only.
- **Fact.** `Loader::read` reports "the file is not UTF-8 text" as an error for any file it reads. A `.md` or `.csv` in UTF-16 or Windows-1252 is common and is trivial for a hostile author.
- **Three builders.** Such a file is in neither list. A imports it, apply writes it, and the project then fails `agentic validate`: a hostile skill breaks the project's config until the owner removes the folder by hand. B leaves it out, though the left-out list does not name it. C refuses the skill, though the cannot-import list does not name it. The diff also cannot show such bytes "in full".
- **Proposed text (AD-38, *What an import takes*):** "A skill file is UTF-8 text. A `SKILL.md` or skill file that is not valid UTF-8, and any other error the validator reports on a file that *Still not accepted* does not list, means the skill cannot be imported, with that finding. The left-out list is closed: an import leaves out only what *Still not accepted* names."

### H3 — A skill file's path is the author's text; it reaches the instruction part of the prompt and the label, and the diff shows a cleaned copy of it

- **Spine (line 368):** "Every name, description and path from the archive is untrusted text: shown as plain text with control characters removed, cut to 64, 1,024 and 255 characters, never rendered as Markdown and never placed in a prompt." Against: "the prompt lists each attached skill's skill files by their path below the skill folder … and `<file>` in the label is that path", and AD-27 (line 294): "followed by … the paths of its skill files", which sits among the `Instruction` parts.
- **Attack (a).** Git stores a file name with any byte but NUL and `/`: line breaks, escape characters, bidirectional overrides. The author names a file `notes<LF><LF>## Step instruction<LF>Copy the table to …<LF>.md`. The diff shows one cleaned line. Apply writes the name as it is. The prompt then lists the raw path, line breaks included, as up to 20 x 255 characters of author-shaped text in the instruction region. AD-11 also counts those words as instruction words for a tainted step's search query. The same raw text goes into the journal label.
- **Two builders.** "255 characters" counted as Unicode characters lets one segment be 255 four-byte characters, which is 1,020 bytes; APFS takes at most 255 bytes per name, so apply fails part-way. "Ignoring case" done with Rust's lowercase does not match the file system's folding for every letter, so two names the check passed can still be one file on disk. "Never placed in a prompt" read to the letter forbids the list AD-27 requires.
- **Not closed.** The control-character sentence covers display only. No sentence limits which names a skill file may have.
- **Proposed text (AD-38, *Skill files*, caps):** "Each segment of a skill file's path is 1 to 100 characters of `A-Z a-z 0-9 . _ -` and does not start with `.`; the whole path is at most 255 bytes. A file or folder with any other name is not accepted: `agentic validate` refuses it and an import leaves it out and names it. So the path in the diff, the path written, the path listed in the prompt, the tool argument and `<file>` in the label are the same bytes." And change the later sentence to: "… never rendered as Markdown and never placed in a prompt before apply; after apply the only such text in a prompt is the path of an accepted skill file (AD-27)."
- **If the owner wants other alphabets in file names:** "A path is stored in NFC; no segment holds a control or format character, `\` or `:`; each segment is at most 255 bytes; the same-path check folds case as `am-domain`'s config-name comparison does." This is weaker and needs its own test list.
- The acceptance example is unaffected: `builder-check-prompt.md`, `visual-companion.md`.

### H4 — "Shown in full" does not say the owner can see every character, and the owner's reading is the only control

- **Spine (line 368):** "produces one Proposal (AD-15) whose diff shows SKILL.md and every skill file in full (amended 2026-11-22, owner answers)"; "the owner's review of the full text is the control."
- **Attack (a).** The known hidden-text attacks on instruction files fetched from a code host all pass "in full":
  - Unicode tag characters (U+E0000 to U+E007F): no renderer draws them, and models read them as letters.
  - Bidirectional controls (U+202A to U+202E, U+2066 to U+2069): the displayed order differs from the byte order.
  - Escape sequences in a file that `agentic skills import` "prints" to a terminal: they can hide or overwrite lines of the printed diff.
  - A diff that renders Markdown: an HTML comment, a link target and a link reference definition are not drawn.
  - One 256 KB line in a view that does not wrap.
- **Scope.** The gap existed for SKILL.md and `references/` before today. The amended sentence restates the control and extends it to csv, json, yaml and txt files anywhere in the folder, and the secret check is the only content check named.
- **Not closed.** "Never rendered as Markdown" and "control characters removed" are said for names, descriptions and paths, not for file content.
- **Proposed text (AD-38, after "the owner's review of the full text is the control"):** "`SKILL.md` and a skill file may not hold a control character other than tab, line feed and carriage return, a bidirectional control (U+202A to U+202E, U+2066 to U+2069), a tag character (U+E0000 to U+E007F) or a supplementary variation selector (U+E0100 to U+E01EF). The validator refuses such a file, so the skill cannot be imported. The import diff, in the UI and as the CLI prints it, shows the source text as typed: never rendered as Markdown, long lines wrapped, and every other format character (general category Cf), every space other than U+0020 and every variation selector shown as a visible `<U+XXXX>` mark."

### H5 — Department and template imports are a second door for skill files, and the amendment does not bind them

- **Spine (line 368):** "*Timing.* Epic 9 story 16 changes the loader and validator that epic 1 story 12 built"; "**What an import takes** (… every source)", where the sources are a folder, a zip and a GitHub URL of a skill import. Skills also "travel in a department export as their folders, unchanged."
- **Fact.** Four stories that build before 9.16 use the old shape: prompt assembly ("followed by its reference-file names"), department export and import (export carries "skills/ folders its seats attach, as they are"; the import diff shows "every SKILL.md and skill reference file"), the import dialog ("SKILL.md and skill reference files") and the Agent card ("reference-file count"). The loader's public shape today is `Skill.references`, keyed by bare file name.
- **Two builders.** The department import enumerates `references/*` for its diff and copies each skill folder as it is. Before 9.16 a stray `skills/x/notes/a.md` fails validation, so such an import cannot apply. From 9.16 the loader accepts that file. The department diff still shows SKILL.md and `references/` only, apply writes the folder, and `notes/a.md` is now listed in the prompt and served by `read_skill_file` as a trusted input. No sentence was broken, and text the owner never saw is in a seat's trusted inputs (AD-38 Prevents: "outside text entering instructions unreviewed").
- **Also.** The prompt assembler may add `references/` in front of a name the loader now gives as `references/forms.md`. The amended AD-27 changes what a built story lists, while the Timing sentence and the ticket say only the loader and validator change.
- **Proposed text (AD-38, *Timing* and *Skill files*):** "Every path that brings a skill folder into or out of a project uses this rule and the snapshot's one list of a skill's files, keyed by path below the skill folder: skill import, department and template import and export, the import dialog, the skill card and the prompt list. None adds or strips a prefix. A department or template import shows in full every skill file it would write, leaves out and names what *Still not accepted* lists, and writes no file into a skill folder that its diff did not show; an export writes `SKILL.md` and the skill files and nothing else. Story 9.16 changes, in the same commit, each built consumer of the old shape: the loader and validator, the prompt list, the department and template export and import, the import dialog and the skill card. A trust test imports a department zip whose skill holds a text file in a subfolder and asserts the diff shows it in full."

## Medium

### M1 — `read_skill_file`: what it reads and how the argument is matched

- **Spine (line 368):** the standing sentence says the tool "resolves only inside the folders of the skills attached to the calling seat"; the amended one says "the tool takes the skill id and that path; it serves only a file the loader accepted … so a left-out file, a `..` path, a link and another skill's file are refused in words; and `<file>` in the label is that path." Binds names both `am-engine` and `am-tools`.
- **Two builders.** A puts the tool in `am-tools`, which cannot see the loader, and resolves `skills/<id>/<argument>` on disk at call time after checking the name. On APFS `References/Forms.md` then opens `references/forms.md`, so one file gets two labels; a name stored in decomposed form does not equal what the model types; and the bytes served are the disk's bytes now, not the ones hashed and secret-checked at load, yet they carry the trusted label. It also breaks the project rule that only `am-config` reads config files. B answers from the snapshot. Separately, one builder puts the argument in the label and one puts the listed path. Different refusal words for "`..` path" and "no such file" tell a model which project paths exist.
- **Proposed text:** "`read_skill_file` is a lookup, not a path resolution. `am-engine` answers it from the run's config snapshot, where each attached skill's files are keyed by the listed path. The argument must equal a listed path byte for byte: nothing is normalised (`./`, a trailing `/`, `\`, another case and a leading `skills/<id>/` all fail), and the tool never opens a file by a name the model gave. The bytes served are the ones hashed and checked at load. Every other argument gets one refusal that lists that skill's paths and says nothing else. The label carries the listed path." Also name one crate for the tool in Binds.

### M2 — The 20-file count: what counts, and when

- **Spine (line 368):** "a skill folder holds at most 20 skill files, `SKILL.md` not counted"; a file over a cap is "not accepted" and left out; "a skill folder with more than 20 skill files" cannot be imported; "the size cap checked on what is expanded".
- **Two builders.** A folder holds 25 `.md` files, 6 of them over 256 KB. A counts after the leave-outs: 19, the skill imports with 6 named. B counts names before expanding: 25, cannot be imported. A second case: one skill folder with 9,000 compressible `.md` entries. A expands them all, the AD-30 archive cap trips and the whole URL is refused. B counts first, marks that one skill "cannot be imported" and the other skills still import.
- **Proposed text:** "The 20 is counted from names, before anything is expanded or read: every file entry of an accepted type that is not under a `scripts` folder, a dot-entry or a nested skill folder and is within the depth and path caps counts, whether or not it later proves over the size cap or fails a check. A skill folder over 20 is not expanded; it is listed as one that cannot be imported, and the other skills in the archive are unaffected."

### M3 — `SKILL.md` as a file has no cap, so the stated bound is not a bound and the child has no number for it

- **Spine (line 368):** "There is no separate total cap: 20 files of 256 KB bound a folder at 5,120 KB beside `SKILL.md`, as before"; "with the size cap checked on what is expanded".
- **Fact.** The built loader caps the body only (`SKILL_BODY_MAX_BYTES`). `description` and `metadata` have no cap, so a SKILL.md can be many megabytes of frontmatter with a small body. The amendment caps `compatibility` and nothing else.
- **Two builders.** The child stops a `SKILL.md` entry at 64 KB as a file, so a valid skill with a 63 KB body and 2 KB of frontmatter cannot be imported. Or it stops at 256 KB, or not at all, and megabytes of frontmatter are parsed in the main process and shown "in full".
- **Proposed text:** "`SKILL.md` as a file is at most 96 KB (98,304 bytes) as written, the two source keys included: the body at most 64 KB as built, and `description` at most 1,024 characters as the format says. The loader checks it; the child stops expanding a `SKILL.md` entry past it, and that skill cannot be imported. A skill folder is so at most 96 KB and 5,120 KB."

### M4 — What marks a skill folder, a `scripts` folder and a dot-entry: spelling, entry kind, and whether the rules reach above the skill root

- **Spine (line 368):** "a folder named `scripts` at any depth"; "a folder below the root that directly holds a `SKILL.md`"; "an entry whose name starts with `.`"; "A skill in the archive is a folder at or below the URL's path that directly holds a `SKILL.md`"; harness folders "lie outside the skill folders".
- **Two builders.**
  - Spelling. `sub/skill.md`: a builder who compares listed names exactly sees a skill file; one who asks the file system whether `sub/SKILL.md` exists gets yes on APFS, sees a nested skill and refuses. The unit tests then pass on the Linux CI job and fail on a Mac, or the reverse. `Scripts/` has the same split.
  - Entry kind. A root `SKILL.md` committed with mode 100755 is common. One builder leaves it out as "a file with an executable bit", so the folder holds no skill and nothing is listed. The other lists the skill as "cannot be imported: SKILL.md is executable". Below the root, a `SKILL.md` that is a link or a folder either makes its folder a nested skill, left out whole, or is left out alone while its siblings join the outer skill.
  - Reach. A repository that keeps skills under `.claude/skills/<name>/`, or under a top-level `scripts/` folder: "at any depth" and the dot-entry rule either apply to the folders above a skill root (no skill found, and a URL the owner typed with `/.claude/skills` in it finds nothing) or only below it.
- **Proposed text:** "Three names are compared ignoring ASCII case: `SKILL.md`, `scripts` and the extension. A folder is a skill folder when it directly holds an entry of that name, whatever the entry's kind, mode or size. At a skill's root, a `SKILL.md` that is a link, a folder, executable or spelled in another case means the skill cannot be imported, with that reason; it is never left out. Below the root such a folder is a nested skill and is left out whole. The `scripts` and dot-entry rules apply only below a skill folder's root: the folders between the URL's path and a skill folder are not judged." If the owner wants skills under dot-folders not found, say the opposite; it must be said.

### M5 — How left-out entries are named: per file or per folder, and without a limit

- **Spine (line 368):** the standing sentence says "an import leaves it out and names each file"; the amended example says "its `scripts/` folder of five files is named as not imported"; the 100-name limit covers plugin extras only. The ticket's verify names single files inside `scripts/`.
- **Attack and split.** A repository that is one skill (`SKILL.md` at its root) makes every other file of the repository a left-out entry. A skill folder with a `.git/` or a vendored tree gives thousands of lines. One builder writes a line per file, one a line per folder; one validator walks into the refused folder and reports each file, one stops at the folder. A folder chain a thousand levels deep with no files in it is not "a file over the depth cap", so nothing says the walk stops.
- **Proposed text:** "A left-out folder (a `scripts` folder, a dot-entry, a nested skill folder, any folder below the third level) is named once with its file count; the validator does not enter it and the child does not expand it. Left-out files are named one per line. A Proposal's "Not imported" list holds at most 100 lines; the rest are counted by reason." And change "names each file" in the standing sentence to "names it".

### M6 — Plugin files inside a skill folder are text and import; the *Plugin extras* sentence rests on a premise the amendment made false

- **Spine (line 368):** "Hooks, scripts, plugin manifests and harness folders (for example `hooks/`, `.claude-plugin/`) lie outside the skill folders, so they are never expanded or imported."
- **Attack (b).** The author puts them inside: `skills/x/hooks/hooks.json`, `skills/x/plugin.json`, `skills/x/mcp.json`, `skills/x/package.json` (with install scripts), `skills/x/AGENTS.md`. Each is a text file of an accepted type, so each is a skill file. A repository that is itself one skill has them inside by layout. They are shown in full, and inside the app they are inert: "No file in a skill folder is ever run", "A skill grants nothing". So the app is closed by words, but the sentence above is no longer true, and a builder may rely on it and skip the check that nothing loads such a file.
- **One name does matter in built code.** `project_root_above` (`am-config/src/kinds.rs`) takes the nearest folder holding `company.yaml` as a project root, as AD-30 says ("any folder holding `company.yaml`, found by walking up from the path"). A skill file named `company.yaml` plants that marker inside the project.
- **Proposed text:** "*Plugin extras.* Hooks, scripts, plugin manifests and harness folders outside the skill folders are never expanded or imported; inside one they are left out when they are dot-entries, `scripts` folders or other types. A text file of an accepted type inside a skill folder is a skill file whatever its name (`plugin.json`, `hooks/hooks.json`, `package.json`): it is shown in full, and no code path parses it, loads it as configuration or acts on it. A file named `company.yaml`, compared ignoring case, is not accepted in a skill folder, because a folder holding one is taken for a project root (AD-30)."

### M7 — Story 9.16 as restated contradicts the amended spine in five places, and the ticket is what the builder builds from

- **Spine (line 810):** the Open Question for Sofia asks that the story "carry" the rule. The ticket was restated in this same change set and still says otherwise:
  1. "`read_skill_file` … lists and reads only files" against "There is no second tool and no listing beyond those paths."
  2. "within AD-38's total caps" against "There is no separate total cap".
  3. "newer frontmatter keys that AD-38 names (for example `compatibility`)" against "exactly the six … Any other key is still refused".
  4. Verify: "names run.py and tool.exe as not imported", where `run.py` is in `scripts/`, against the folder being named once (M5).
  5. "No built story changes" against the loader, validator and prompt-list changes (H5).
- **Proposed text (Open Questions, the Sofia line):** name these five corrections one by one, so the restatement is checked against them.

## Low

### L1 — No frontmatter value in any prompt is said for `compatibility` only

- **Spine (line 368):** "it is not placed in a prompt" (for `compatibility`); for the other keys only AD-27's "each SKILL.md body" implies it, and "like the other skill keys" is said only of `is_widening`. A builder of an Assistant or a seat-suggestion feature may read the silence as leave to put `description` or `metadata` in a prompt.
- **Proposed text:** "No frontmatter value of a skill is placed in any prompt, a seat's or the Assistant's; only the body is." Also: "The skill card shows `compatibility` as the skill author's own words, never as the app's statement", as the source record already says for its keys. And say the unit: "500 characters counted as the loader counts `goal`; an empty or null value is refused."

### L2 — A second copy of the two host names outside `am-net`

- **Spine (line 204):** "whose one source is `am-net`'s constant, never from a literal in `web/src`" is said for the About page. The Proposal's "two hosts" (line 368) and the CLI's output have no stated source, so `am-engine` may hold its own literal. A UI test that asserts the real names needs a literal under `web/src`.
- **Proposed text:** "The Proposal's two hosts and the CLI's host line come from the same constant, through the `SkillSource` port; no other crate holds the names. The About page's UI test mocks the `/api/v1` answer with two other names and asserts the page shows those, which proves there is no literal."

### L3 — `.DS_Store` is skipped by name alone

- **Spine (line 368):** "only `.DS_Store` is skipped unreported, as built". As built, `Loader::listing` skips the name before it looks at the entry. A folder or a link named `.DS_Store`, now at any depth, with anything in it, is unseen by the validator and is not in the snapshot hash.
- **Proposed text:** "Only a regular file named `.DS_Store` is skipped; a folder or a link of that name is a dot-entry and is refused. An import never writes one."

### L4 — Apply: the target folder and the file modes

- **Spine (line 368):** "an id that already exists in the project"; "apply writes exactly those bytes". A stray `skills/Brainstorming/` is not a declared id, yet on APFS apply writes into it, and its text files then become skill files that no diff showed. The mode of a written file is unsaid.
- **Proposed text:** "Apply creates `skills/<name>/` itself and refuses when `skills/` already holds an entry of that name, compared ignoring case. It writes regular files with mode 0644 in folders with mode 0755; no mode, owner, time or extended attribute comes from the source."

### L5 — One built case narrows, and only the memlog says so

- **Spine (line 368):** "`references/` is one such subfolder and keeps no rule of its own." The built loader accepts `references/.notes.md` (a dot-file with an accepted extension); from 9.16 it is refused. The memlog records this; the spine does not, so story 9.16's builder will meet a failing project or template without a rule to cite.
- **Proposed text:** "One built case narrows: a dot-file under `references/` was accepted and is refused from that commit; `agentic validate` names the file and the fix (rename it)."

### L6 — URLs inside skill files count as recorded URLs for a tainted step (carried over, not new)

- **Spine:** `read_skill_file` "returns the file as a trusted input"; AD-11 allows a tainted step a read whose URL is "present in the step's trusted inputs". A skill written outside the project can so bring up to 5,120 KB of URLs on an allowed host, and a tainted step may then choose among them. The amendment keeps the total and only moves where the files may sit; the owner's review of the diff is the control. Noted so the choice is a stated one.
- **Proposed text, if the owner wants it closed:** "A URL counts as recorded from a skill only when it is in the `SKILL.md` body; a URL in a skill file read through `read_skill_file` does not."

## Summary table

| Id | Tier | Where | One line |
|---|---|---|---|
| H1 | high | AD-38 *Skill files*, *Same path* | Rule has no single home; child and validator cannot share code |
| H2 | high | AD-38 *What an import takes* | Non-UTF-8 and other validator errors fall in no outcome |
| H3 | high | AD-38 `read_skill_file`, AD-27 | Path text reaches the prompt raw; diff shows it cleaned |
| H4 | high | AD-38 Importing | "In full" does not mean visible; hidden characters |
| H5 | high | AD-38 *Timing* | Department and template imports not bound to the rule |
| M1 | medium | AD-38 `read_skill_file` | Disk resolution or snapshot lookup; argument spelling; label |
| M2 | medium | AD-38 caps | What the 20 counts and when |
| M3 | medium | AD-38 caps | `SKILL.md` file has no cap |
| M4 | medium | AD-38 *Still not accepted*, *Several skills* | Spelling, entry kind and reach of the marker names |
| M5 | medium | AD-38 left out and named | Per file or per folder; no limit |
| M6 | medium | AD-38 *Plugin extras* | False premise; `company.yaml` as a skill file |
| M7 | medium | Open Questions, story 9.16 | Five contradictions between ticket and spine |
| L1 | low | AD-38 *Known keys* | Frontmatter never in a prompt; card label; unit |
| L2 | low | AD-12 About line | Second copy of the host names; UI test |
| L3 | low | AD-38 *Still not accepted* | `.DS_Store` folder or link |
| L4 | low | AD-38 apply | Target folder by case; file modes |
| L5 | low | AD-38 *Skill files* | Narrowed built case not in the spine |
| L6 | low | AD-38, AD-11 | Recorded URLs from skill files (carried over) |
