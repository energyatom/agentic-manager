# Rubric review: spine update 2026-11-22, owner answers on skill import

Reviewer: rubric walker (independent, read-only). Date: 2026-11-22.

Scope: the uncommitted amendment to `architecture-agentic-manager.md` marked "amended 2026-11-22, owner answers": AD-38 Rule and Prevents (line 367-368), AD-12 Rule (line 204, the About line), AD-27 Rule (line 294), the seed comment (line 591) and Open Questions (lines 807-810). Read against AD-10, AD-11, AD-13, AD-15, AD-27, AD-30, AD-35, AD-46, the Design Paradigm table (line 34-42), the capability map, Deferred, AGENTS.md Policy, the built loader (`crates/am-config/src/skills.rs`, `kinds.rs` `SKILL_KEYS` and `skill()`, `lib.rs` `Loader::read` and `listing`, `entities.rs` `Skill`) and the tickets that consume skills (1.12, 3.5, 4.11, 5.11, 5.17, 9.16). Nothing under `_archive/` was read.

## Verdict

**Pass with fixes.** No critical finding. Every owner answer and every required amendment landed, AD ids are stable, each change is marked and dated, the staging ("until that commit the built rule stands") is honest, and no AGENTS.md policy line is weakened. Three high findings should be fixed before story 9.16 is built. Each is a place where the Rule states the outcome ("a skill that imports also validates", "the same confinement", "changes the loader and validator") without the mechanism that makes two builders, or two crates, arrive at it.

Counts: 0 critical, 3 high, 6 medium, 8 low.

## Part A. Reconcile

### Owner answers

| Answer | Status | Where | Note |
|---|---|---|---|
| a. Commit-id and PR URLs refused: accept | Landed | AD-38 *Pin* (line 368, "confirmed by the owner 2026-11-22"); line 807 (1) "Confirmed" | The reason (forks and pull requests) stays in AD-38 and was dropped only from the resolved question. Fine. |
| b. Text files anywhere in the skill folder; same types and size caps; scripts and executables left out and named; done in 9.16, 1.12 not reopened | Landed | AD-38 *Timing*, *Skill files*, *Still not accepted* (line 368); AD-27 (line 294); seed (line 591); line 807 (2) | Types and the 256 KB cap match the built constants. The count, depth and path caps and five other choices are architect defaults that the spine does not flag as such (M6). |
| c. Newer open-format keys are known keys, shown in the import diff, never granting; same timing | Landed | AD-38 *Known keys* (line 368); line 807 (4) | The six keys are named; `compatibility` is the one the built `SKILL_KEYS` lacks (checked: five keys built). "Any other key is still refused" keeps the built behaviour. |
| d. About page line naming the two GitHub hosts | Landed | AD-12 *Shown and recorded* (line 204); line 807 (5) | The route the names take to `/api/v1` is not buildable as written (M4). |
| e. Sally's UX pass queued before 9.16's UI | Landed | Line 809 (owed, `bmad-ux`); line 810 (Sofia: dependency of 9.16's UI) | Extended by the architect to the skill card's `compatibility` line and the "Not imported" wording (L6). |
| Acceptance example kept: brainstorming and writing-plans import complete | Landed | AD-38 *Acceptance example* (line 368); line 807 | Matches the given facts: three text files and a five-file `scripts/` folder for brainstorming, `SKILL.md` only for writing-plans; sizes are far below the caps. "Complete" is worded "complete in their text files", which is what answer b allows. |

### Required amendments

| # | Requirement | Status | Where | Note |
|---|---|---|---|---|
| 1 | AD-38 skill folder rule for every skill: text files anywhere, subfolders too, same per-file cap; total caps for count, depth, bytes; `read_skill_file` reach, confinement and labels; scripts, executables, links, other extensions left out and named; timing | Landed, with gaps | Line 368 | Count 20, depth 4 segments, 255 characters, bytes bounded at 5,120 KB by derivation (no separate cap, stated). Gaps: where the one rule lives (H1), the tool's mechanism (H2), what else 9.16 changes (H3), how the 20 is counted on import (M1), a non-UTF-8 file (M2), path characters (M3). |
| 2 | AD-38 frontmatter: the open format's keys named, parsed, shown on the skill card and in the import diff, information only; unknown key refused; same timing | Landed | Line 368 *Known keys* | Also classified in `is_widening` as not widening, which AD-15's exhaustive match needs for a new field. |
| 3 | About line naming `api.github.com` and `codeload.github.com`, contacted only on a GitHub skill import | Landed | Line 204 | See M4 and L4. |
| 4 | Answered questions moved to decided with the date; Sofia items kept; Sally's pass owed before 9.16's UI | Landed | Lines 807-810 | Question (3), one confirmation per ticked skill, was not among the answers and is kept open with the rule standing (line 808). Correct handling. |
| 5 | Answer (a) marked confirmed | Landed | Line 368 *Pin*; line 807 (1) | |

Marking and stability: all AD ids unchanged; every amended sentence carries "amended 2026-11-22, owner answers"; no built story is changed by the text; the spine diff is 9 insertions and 7 deletions on the five named places only.

### Scope beyond the answers

No large scope creep. These additions go beyond the owner's words. All are defensible consequences of answer b, but none is flagged in the spine as an architect default (the memlog lists them as assumptions; the spine does not). See M6.

- A skill folder with more than 20 skill files cannot be imported at all (the earlier text did not say what happens over the count on import).
- A skill file the secret check refuses stops the whole import (earlier only `SKILL.md` did).
- Depth cap of 4 segments and path cap of 255 characters (asked for by the update brief, values chosen by the architect).
- Dot-entries are refused at any depth; this narrows one built case (L1).
- A nested folder holding its own `SKILL.md` is refused by `agentic validate` too, not only left out by an import. Needed so the previous amendment's *Several skills* rule and the validator agree.
- Sally's pass also covers the skill card's `compatibility` line and the "Not imported" wording (L6).

### Stale or contradicting text elsewhere in the spine

Searched for `references/`, "reference file", `read_skill_file`, "frontmatter key", `SKILL.md`, "skill" and "About page" across the spine.

- AD-10 (line 192), AD-11 (line 198, `read_skill_file` in the read class), AD-15 (line 222, importing a skill is widening), AD-35 (line 346, `skills/<id>/` on the files side), AD-46 (line 438, the tool is always granted with an attached skill), AD-48 (line 452), Deferred (lines 780, 787-789): consistent, no change needed.
- AD-27 (line 294) and the seed (line 591): amended, consistent with AD-38.
- "Reference files" in AD-30 and AD-32 (lines 312, 324) are a seat's Knowledge files (`UntrustedData`), a different thing from a skill's files (trusted after the reviewed apply). The new term "skill files" removes that overlap for the amended text. Good.
- Capability map R60 row (line 735) still says "references". Cosmetic (L8).
- AD-12 Binds (line 202) does not name the About page or the delivery crates the new line binds (L4).
- The contradiction that matters is with other stories, not other ADs: see H3.

## Part B. Findings

### Critical

None.

### High

**H1. One rule, three implementers, and no single place for it (line 368, *Skill files*, *Still not accepted*, *Same path*).**

Problem. The amendment's own Prevents line is "an imported skill the validator then refuses". The Rule gets there by saying the same sentence applies to the loader, the import and the archive child ("the skill-file rule above, judged from each entry's name, type and mode"). Three pieces of code must then hold the same extension list, caps, `scripts` rule, dot rule and nested-skill rule: `am-config` (loader and validator), `am-extract` (the `agentic __extract` child, which decides what to expand) and `am-engine` (the import that builds the Proposal and its "Not imported" list). The Design Paradigm table (line 40) lets `am-extract` depend on `am-domain` only, and `am-engine` on `am-domain` and `am-store`, so neither can call the loader. Today the constants live privately in `am-config/src/skills.rs`. As written, 9.16 copies the rule into two more crates and nothing but a fixture test keeps them equal. That is exactly the divergence the amendment says it prevents.

Proposed wording, added after *Still not accepted*: "*One rule.* `am-domain` owns the one pure function that judges an entry of a skill folder from its path below the skill folder, its kind (file, folder, link), its mode and its size, and answers `SKILL.md`, skill file, or not accepted with the reason (as `is_config_bearing` does for config names, AD-30). The loader, the import and the extract child call it; no other code holds the extension list, the caps, the `scripts` rule, the dot rule or the nested-skill rule. A trust test (`skill_import_*`) runs `agentic validate` on what every import fixture's apply wrote and expects no error."

**H2. `read_skill_file` states its refusals but not its mechanism; the older sentence suggests a file-system lookup (line 368, "Epic 9 story 16 builds the tool...").**

Problem. The amended sentence lists outcomes: it "serves only a file the loader accepted", and a left-out file, a `..` path, a link and another skill's file are "refused in words". The sentence before it, unchanged, says the tool "resolves only inside the folders of the skills attached to the calling seat". A builder can meet both by joining the path onto `skills/<id>/` and reading the disk with checks. That build (a) is a second copy of the accept rule in `am-engine` or `am-tools`, (b) breaks the convention that only `am-config` reads project config files, (c) breaks AD-15's "a run pins the snapshot hash it started with": a skill file edited mid-run would reach the model as trusted input though it is not in the pinned snapshot, and (d) reopens link and race questions that the loader already settled. The amendment widened the argument from a flat file name to a path of up to four segments, so the mechanism now matters more. The built snapshot already holds each reference text (`Skill.references`, name to text), so the safe mechanism ratifies built code.

Proposed wording, replacing "it serves only a file the loader accepted ... are refused in words": "the tool never touches the file system: it answers from the run's pinned config snapshot, which holds each accepted skill file's text keyed by its path below the skill folder. The call names a skill id attached to the calling seat and a path; the path must equal one listed path exactly (after NFC, no other normalisation, no resolution). Anything else, such as a left-out file, a `..` path, a link, `SKILL.md` itself or another skill's file, matches nothing and is refused in words, with the listed paths of that skill."

**H3. *Timing* names only the loader and validator; four stories built before 9.16 consume the old list, and nothing makes them follow (line 368 *Timing*; line 294; line 810).**

Problem. The Rule says "Epic 9 story 16 changes the loader and validator that epic 1 story 12 built". But the rule change also alters what these unbuilt stories will have built by then, in other crates:

- epic 3 story 5 (prompt assembly, `am-engine`): lists "reference-file names"; AD-27 as amended turns that into paths, so `forms.md` becomes `references/forms.md` and recorded prompts change;
- epic 5 story 17 (Agent card, `web/`): the skill card's "reference-file count", and now the `compatibility` line;
- epic 5 story 11 (import dialog) and epic 4 story 11 (department export and import): the diff shows "every SKILL.md and skill reference file" in full.

Story 9.16 lists 4.11, 5.11 and 5.17 in its `after`, so all are built first. If the department import walks `references/` itself, then after 9.16 a department that carries a skill with a file beside `SKILL.md` is shown incomplete, or the file is dropped without being named. The spine gives no rule that these consumers take a skill's files from one list, and line 810's addition for Sofia does not name these stories. "No built story changes" is true today and will not be true when 9.16 builds.

Proposed wording, added to *Timing*: "Every consumer of a skill's files (the AD-27 list, the skill card, the department and template export and import diff of AD-15, `read_skill_file`) takes them from the loader's one list of skill files, keyed by path below the skill folder, and never walks a skill folder itself; so the commit that changes the loader changes them all, and a department import shows every skill file in full. From epic 3 story 5 on, the AD-27 list shows that path (`references/forms.md`), so its format does not change at story 9.16." And in line 810, for Sofia: "epic 3 story 5, epic 4 story 11 and epic 5 stories 11 and 17 should say they read a skill's files from the loader's list by path; story 9.16's verify should show a department import with a file beside `SKILL.md` in full."

### Medium

**M1. How the 20 is counted on import is open (line 368, "a skill folder holds at most 20 skill files" against "a file over the size, depth or path cap" being left out).**

Problem. A folder with 21 text files of which one is over 256 KB: builder A counts candidates by name and type before expanding (21, cannot be imported); builder B leaves the oversize file out first (20, imports). The same question arises for files past the depth cap. In `agentic validate` it does not matter (any refused file is already an error; the built loader counts every regular file in `references/`). On import it decides between "imports" and "cannot be imported", and it decides how much the extract child expands before it may stop.

Proposed wording: "The 20 is counted over the entries the rule accepts by name, kind, mode, depth and path, before anything is expanded; a file later left out for its size still counts. Over 20, the child expands none of that skill's files." (The other reading is also workable; the spine must pick one.)

**M2. A skill file that is not UTF-8 text is refused by the validator and unclassified on import (line 368, *Still not accepted* and "These cannot be imported").**

Problem. `Loader::read` reports "the file is not UTF-8 text" as an error for any file it reads, so a Latin-1 `.csv` or a UTF-16 `.txt` fails `agentic validate`. The import lists neither it nor an unreadable file under "left out and named" or under "cannot be imported", and the Proposal "holds the full text" of every file, so the builder must guess. One guess writes a skill the validator then refuses.

Proposed wording, added to *Still not accepted*: "a file that is not valid UTF-8 text (validate refuses it; an import leaves it out and names it as 'not UTF-8 text')."

**M3. The path rule lacks a unit, a normal form and a character rule, and these paths now enter the prompt and the source label (line 368, "at most 4 segments ... and 255 characters"; line 294).**

Problem. (a) "255 characters" can be bytes, Unicode scalar values or UTF-16 units. (b) Archives made on macOS often carry NFD names; the model will most likely type NFC, so an exact match fails, and the count differs by form. (c) Nothing limits the characters of an accepted path. The archive rule strips control characters for display only. An accepted path with a newline or other control character would be written to disk, listed in the Instruction region of the prompt (AD-27) and used inside the journal label `skill:<id>/<file>`, where a line break can imitate another label. A file name is outside text of up to 255 characters; the reviewed diff shows it, but it should not be able to break the list or the label.

Proposed wording: "A skill-file path is held, listed, labelled and compared in NFC and counted in Unicode scalar values. A path with a control or format character, a backslash or a colon in any segment, or a segment with leading or trailing space, is not accepted (validate refuses it; an import leaves it out and names it)."

**M4. The About line's "reads the two names from the core over `/api/v1`, whose one source is `am-net`'s constant" has no buildable route (line 204).**

Problem. `am-server` and `am-api` may depend on `am-domain`, `am-engine`, `am-api` and the store read handle, not on `am-net` (line 41). The same Rule says `am-app` hands the `SkillSource` port only to the skill-import command handler and a trust test proves nothing else holds it. So the About handler can neither import the constant nor hold the port. A builder then either copies the literal into `am-server` (a second source, and a named host in a second crate), or gives the About handler the port (fails the trust test), or adds a dependency `check-deps` refuses.

Proposed wording: "`am-app` reads the two names from `am-net`'s constant at start and gives them to `am-server` as plain values for the About read; the About handler never holds the `SkillSource` port, `am-server` gains no dependency on `am-net`, and no second literal exists in `crates/` or `web/src`."

**M5. "Left out and named" has no grain, reason or limit, and two sentences differ (line 368: sentence 2 "names each file"; *Still not accepted* "leaves each out and names it"; *Acceptance example* "its `scripts/` folder of five files is named").**

Problem. One builder lists `scripts/ (5 files)`, another lists five paths. A local folder import with a `.git/` folder shows why the grain matters. Nothing says a reason is shown. That matters most for a text file left out for its size or depth: the owner expects the skill to be complete and needs to see why it is not. The *Plugin extras* list is capped at 100 names; this list has no cap.

Proposed wording: "In the 'Not imported' list a left-out folder (`scripts`, a dot-folder, a nested skill) is one line with its path and file count; a left-out file is one line with its path; each line gives its reason in words (script folder, executable, link, file type, hidden, another skill, over 256 KB, too deep, path too long, not UTF-8 text); at most 100 lines per skill, the rest counted." And change sentence 2's "names each file" to "names it".

**M6. Architect defaults are not marked as defaults in the spine, and one has a hard consequence for the owner (line 368; Open Questions).**

Problem. The memlog records as assumptions: the depth and path caps; no total-bytes cap; over 20 skill files stops the import; a secret finding in a skill file stops the import; dot-entries refused; a nested skill left out whole; `compatibility` not placed in a prompt. The spine states them as settled and Open Questions does not list them, so the owner cannot tell them from the answers. The count rule is the one with bite: a GitHub skill with 21 text files cannot be imported at all, and the owner has no remedy short of forking it. The memlog notes that one skill in the acceptance repository already holds 19.

Proposed wording, a new Open Questions line: "Architect defaults 2026-11-22 in AD-38's skill-file rule (not owner words; they stand unless the owner says otherwise): at most 20 skill files in a folder, and a folder with more cannot be imported (the import never chooses which to keep; one skill of the acceptance repository holds 19); at most 4 path segments and 255 characters; no total-bytes cap beyond 20 times 256 KB; a secret finding in a skill file stops that import; dot-entries and nested skill folders are not accepted; `compatibility` is not placed in a prompt."

### Low

**L1. The dot-entry rule narrows a built case and the text reads as if it were built (line 368, "an entry whose name starts with `.` ... (only `.DS_Store` is skipped unreported, as built)").** The built loader accepts `references/.notes.md` today (hidden entries get the same checks as any entry, and the extension is `md`). Proposed: add "This narrows one built case: a dot-file under `references/` validates until that commit and is refused from it."

**L2. Units and small edges the level below would guess (line 368).** Proposed additions: "KB is 1,024 bytes, as built"; "the extension is compared ignoring ASCII case, as built; a file with no extension is 'any other extension'"; "`compatibility` is counted in Unicode scalar values; an empty, null or longer value is refused by the validator".

**L3. Tool surface (line 368; line 810).** The arguments are described ("the skill id and that path") but not named; whether `SKILL.md` can be read through the tool is open (H2's wording says no); and ticket 9.16 still says the tool "lists and reads" while the spine says "no listing beyond those paths". Proposed: name the two arguments (`skill`, `path`) or say the ticket names them, and add to line 810: "story 9.16's text should drop 'lists' from `read_skill_file`".

**L4. AD-12 Binds and the word "GitHub" in the UI (lines 202, 204, 810).** The new About line binds the About page and the delivery crates, which AD-12's Binds does not list. The About sentence and the URL field put the word "GitHub" into `web/src` strings while AGENTS.md bans named source sites there. Line 810 already asks Sofia to restate the named-hosts line. Proposed: add "the About page (`web/`, `am-server`)" to Binds, and to line 810 "the restated denylist line should allow the word 'GitHub' in the import and About strings, and nothing else".

**L5. Warnings on imported yaml (line 368, "then pass `agentic validate`").** The built loader warns on every comment line of a yaml reference. A third-party yaml skill file with comments imports and then validates with warnings. Proposed: "pass `agentic validate` with no error (a warning, such as a comment in a yaml skill file, does not stop an import and is shown in the diff view)".

**L6. Line 809 goes slightly past answer e.** The skill card's `compatibility` line and the "Not imported" wording are the architect's addition to Sally's pass. Reasonable, since answers b and c create both. Proposed: mark it, "(the last two added by the architect, as answers b and c create them)".

**L7. The acceptance example's "pass `agentic validate`" is asserted, not shown checked (line 368, *Acceptance example*).** File names, sizes, modes and frontmatter keys were checked at commit `22f0b46`. The secret-prefix scan over the three text bodies is not recorded as checked, and the owner's live run imports HEAD, which can drift from that commit. Proposed: "the recorded fixture is pinned to that commit and is the gate; the owner's live run is a demonstration, and a later upstream change that the rule refuses is reported in words, not a defect".

**L8. Text that goes stale by design (lines 368, 735).** After story 9.16 lands, the Rule's first two sentences and the "until that commit" clause describe a rule that no longer exists, and the R60 map row still says "references". Proposed: add to line 810 "when story 9.16 lands, the spine's AD-38 opening, AD-27 and the seed are collapsed to the one rule (architect, at that story's review)".

## Good-spine checklist on the amended text

| Check | Result |
|---|---|
| Each amended Rule is enforceable | Mostly. Known keys, the caps and the About line are testable as written. The import and validate parity (H1) and the tool's confinement (H2) are stated as outcomes without the mechanism. |
| It prevents its stated divergence | "A frontmatter key read as a grant": yes (information only, `is_widening` classification, rule_10 test). "An imported skill the validator then refuses": not yet (H1, M1, M2). |
| Two builders could not build incompatibly | Not yet for: the count on import (M1), the path unit and form (M3), the "Not imported" grain (M5), the About route (M4), the consumers of the list (H3). |
| Nothing left for the level below to guess | See M2, M3, L2, L3. |
| Ratifies, not contradicts, the built code | Yes on types (`md, txt, csv, json, yaml, yml`), 256 KB, 20, links, the executable bit, `.DS_Store`, five built keys, `name` equal to the folder. It says where 9.16 changes the built loader. Unstated difference: a dot-file under `references/` (L1). Unstated match worth using: the snapshot already holds reference text (H2). |
| Does not weaken another AD or an AGENTS.md policy | Holds. A skill still adds instructions only: no file is run, and no key or file reaches ToolGate, the egress client or the touch panel. Outside text still becomes `Instruction` only through the reviewed apply, and the diff shows every skill file in full. No host literal is added to `web/src` (but see M4 for `crates/`). Widening is still decided only by `is_widening`. The one soft spot is path text entering the prompt and the label (M3). |
| AD ids stable, changes marked, scope limited | Yes. Scope additions are listed in Part A and in M6. |
