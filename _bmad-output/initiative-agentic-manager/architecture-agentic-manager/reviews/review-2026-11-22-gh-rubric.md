# Rubric review: spine update 2026-11-22, skill import from GitHub

Reviewer: rubric walker (independent, read-only). Date: 2026-11-22.
Scope: the uncommitted amendment to `architecture-agentic-manager.md` (AD-12 "App requests" and "No telemetry", AD-15 CLI-class list, AD-38 "Importing" and "From GitHub", two Deferred rows, the R60 map row, the `am-net` seed line, the Cloud node), read against AD-10, AD-11, AD-13, AD-15, AD-23, AD-25, AD-30, AD-32, AD-33, AD-35, AD-48, the Consistency Conventions, the built code (`crates/am-net/src/lib.rs`, `crates/am-domain/src/net.rs`, `crates/am-config/src/skills.rs`, `kinds.rs` `skill()`), AGENTS.md Policy and the restated ticket (epic 9 story 16).

## Verdict

**Pass with fixes.** No critical finding. The amendment covers all seven owner points, keeps AD ids stable, marks its changes, and its facts match the built loader (the `metadata` map takes any text key, so the source record needs no format change). Five high findings must be fixed before story 9.16 is built: each is a place where the Rule states a guarantee without the mechanism, leaves a neighbouring AD stale, or over-reaches.

Counts: 0 critical, 5 high, 9 medium, 8 low.

## Owner decision coverage

| Point | Covered | Where | Note |
|---|---|---|---|
| 1 repo or folder URL; public no key; private v1.x | Yes | AD-38 From GitHub, AD-12 How, Deferred row 2 | The spine accepts only the `https://` form; the owner and the ticket write it without a scheme (M6) |
| 2 several skills listed with descriptions, ticked, each through the same path | Yes | AD-38 Several skills | The skill id and a failing SKILL.md are not settled (H5) |
| 3 download the archive, reuse the import path | Yes | AD-38 Pin, Same path | What is expanded, and one download per command, are not settled (M3, M4) |
| 4 plugin extras left out and named | Yes | AD-38 Plugin extras | "Named" has no granularity; "import summary" is undefined (M4) |
| 5 pin the commit; record URL and SHA in the skill folder | Yes | AD-38 Source record | Which crate edits the frontmatter, and which Proposal field carries the record, are open (M5) |
| 6 fetched by the app, shown, audited, never on a seat's list | Yes | AD-12 App requests | The "never by an agent" guarantee has no mechanism (H1); who writes the audit entry is open (M1) |
| 7 acceptance example | Yes (ticket) | ticket verify | Checked against the loader: the example passes (name and description only in frontmatter; sibling files and `scripts/` are left out and named) |

Nothing was added that the owner did not ask for and that closes no divergence. The broadened "left out and named" sentence ratifies what the built loader already refuses.

---

## High

### H1. The "only a Human command makes an app request" guarantee has no mechanism, and the request is a free URL

Spine (AD-12): "A seat, a tool, a model, the Assistant, a schedule, a trigger or a run never makes it: no tool manifest, ToolGate path or engine run can build a request with an app purpose, and a test proves it."

Problem. The built port is `am_domain::Caller { pub seat: Option<String>, pub account: Option<String> }` with `Default`, on a public `HttpRequest` with a free `url`. Adding the purpose there lets every crate that names `am-domain` set it: `am-tools`, `am-connectors`, `am-mcp`, `am-providers`, and the agent loop, which sits in the same crate (`am-engine`) as the import. A test cannot prove a negative over all code paths unless the rule is structural. The Rule also does not say which crate enforces the *How* limits (method, no query, no credential, 3 redirects, body caps, 8 requests, 1 s spacing). If the import code in `am-engine` enforces them, a second caller of the purpose skips them, and "an app-made request turning into a general fetch path" is not prevented: any `GET` path on `api.github.com` is reachable (a user, a gist, any public file up to the cap).

This is owner point 6, so it should be closed in the Rule, not left to the story.

Proposed replacement for that sentence:

> "No caller passes a URL with an app purpose. The purpose value carries typed fields only (`owner`, `repo`, and either `CommitLookup(ref)` or `Archive(sha)`), and `am-net` builds the two request shapes itself and enforces every limit under *How* for them. The value is built in one place, the import command handler, which takes the command's `Actor` and refuses any actor but `Human`. `cargo xtask` fails the build when the purpose type is named outside `am-domain`, `am-net` and that one `am-engine` module (never in `am-tools`, `am-connectors`, `am-mcp`, `am-providers`, ToolGate or the agent loop). The trust test `crates/am-e2e/tests/trust/rule_09_*` (or a new `skill_import_*.rs`) shows a seat without these hosts on its list making zero requests to them through every tool."

Also say where the 8-request count lives: "The command handler creates one request budget per command and passes it with each request; `am-net` counts against it."

### H2. The no-telemetry test sentence is wrong in time, names a test that does not exist, and conflicts with epic 2 story 2

Spine (AD-12): "The no-telemetry test and the egress-configuration test name the set explicitly (amended 2026-11-22, owner): their expected list of fixed hosts in the binary holds `api.github.com` and `codeload.github.com` under `SkillImport`, and they fail on a third host under that purpose, on either host fixed under any other purpose, and on any other fixed host the tests do not name."

Problems.
1. Epic 2 story 2 ("Egress client blocks everything unlisted") is not built and builds next, long before 9.16. Its verify says: "CI fails if any host outside configured Accounts, seat allow-lists and the release host appears in the binary's egress configuration". Its builder reads AD-12 as amended and finds a test that must expect two hosts the binary does not hold yet. Two builders can diverge: one adds the hosts to the expected list now, one does not.
2. "The egress-configuration test" appears nowhere else in the spine or the tickets. The tickets know one test, the no-telemetry test, which reads the binary's egress configuration.
3. The existing fixed host ("the release host" in story 2.2) is not named here, so "any other fixed host the tests do not name" leaves its status open.

Proposed replacement:

> "The no-telemetry test (epic 2 story 2) reads the fixed hosts in the binary's egress configuration and compares them with an exact expected list. Until epic 9 story 16 that list holds no host under an app purpose. Story 16 adds `api.github.com` and `codeload.github.com` under `SkillImport` to the list in the same commit that adds them to `am-net`. The test fails on any fixed host it does not name, on a third host under that purpose, and on either host under any other purpose."

For Sofia (report, not fixed here): story 2.2's verify sentence and story 9.16's "No built story changes" need one line each, since 9.16 edits 2.2's expected list.

### H3. AD-48 and the host-matching convention are left stale; AD-12's own Prevents now contradicts its Rule

Spine (AD-48, not amended): "**Two kinds of destination, never mixed:** tool, reader, `Fetcher`, link-list and Source requests from a seat check that seat's `allowed_hosts` only […]; a request carrying an Account […] goes only to that Account's pinned endpoint […], and an MCP-over-HTTP request only to its server's approved URL".
Spine (AD-48 and Consistency Conventions): "am-config's validator, `is_widening` and `am-net` all use it, and nothing else matches hosts" / "never a hand-written suffix or regex match".
Spine (AD-12 Prevents, amended): "any call outside configured endpoints, and an app-made request turning into a general fetch path".

Problems. There is now another kind of destination, and AD-48 still says two. The amendment does not say how `am-net` compares a request host or a redirect target with the two fixed hosts, so a hand-written string compare is the likely result, against the convention. And AD-12 still promises to prevent "any call outside configured endpoints" while its Rule now allows one to a compiled, not configured, endpoint.

Proposed fixes:
- AD-48, after the "Two kinds" sentence: "A third kind, a request with an app purpose, goes only to AD-12's fixed host set and shares nothing with the other two (amended 2026-11-22, owner)."
- AD-12 Where: "The two hosts are exact `HostPattern`s, and the request host and every redirect target are matched with `HostPattern`, as everywhere (AD-48)."
- AD-12 Prevents: "any call outside configured endpoints and the one fixed app host set".

For Sofia: PRD RULE-5 and the initiative's "Local and private" line say "network only to configured endpoints"; the architect may not edit them.

### H4. AD-15's closed CLI list now names the URL import only; the ticket's folder and zip CLI import is in no class

Spine (AD-15): "[…] a manual run start, resuming a schedule and starting a skill import from a GitHub URL (it makes the app send AD-12's app requests; it creates Proposals and applies none; amended 2026-11-22, owner)".

Problem. The CLI-class list is closed. The ticket (unchanged part) requires `agentic skills import <folder|zip>`, which "prints the diff and the Proposal id". Before the amendment that command was simply missing from the list. Now the list names one of three import sources, so a builder reads the other two as excluded, and whether the folder and zip import are gated by the stdio-MCP rule is open. The trailing "the CLI prints `agentic open --item <id>`" also does not fit an import, which has no Inbox item (see L1).

Proposed replacement:

> "[…] resuming a schedule and starting a skill import from a folder, a zip or a GitHub URL (it creates Proposals and applies none; from a URL it makes the app send AD-12's app requests; amended 2026-11-22, owner)".

### H5. "Never a reason to refuse the skill" over-reaches; the imported skill's id, a SKILL.md the validator refuses, and an id that already exists are not settled

Spine (AD-38): "Everything the format above does not accept is **left out and named**, never imported, run or read as config, and never a reason to refuse the skill: `scripts/`, executables, links, other extensions, and any file or folder outside `SKILL.md` and `references/`".
Spine (AD-38): "A skill in the archive is a folder at or below the URL's path that directly holds a `SKILL.md` […] When one is found it is imported."

Problems, each checked against the built loader (`kinds.rs` `skill()`, `skills.rs`):
1. The format also does not accept a SKILL.md body over 64 KB, an unknown frontmatter key (`SKILL_KEYS` is closed, and skills written for other tools carry extra keys), a `name` that differs from the folder, or a folder name that is not kebab-case. These cannot be "left out". The sentence as written says they are never a reason to refuse. One builder will refuse; another will strip keys or rename, which rewrites outside text silently.
2. The loader requires `name` to equal the folder id. A repository whose `SKILL.md` sits at its root has no folder name: the archive's top folder is `<owner>-<repo>-<sha>`. The same holds when the URL's path is the skill folder itself. The id is undefined.
3. The 21st reference file and beyond: which 20 are kept is not said.
4. An id that already exists in `skills/` is not covered. The owner will re-import to update until the update check ships, and the Deferred row says the update "goes through the same import Proposal", which assumes a rule that does not exist.
5. Zero skills found under the URL's path has no rule.

Proposed sentences (after "left out and named …"):

> "This holds for files beside SKILL.md. A skill whose SKILL.md the unchanged validator refuses (frontmatter, an unknown key, a body over 64 KB, a `name` that is not a kebab-case id) is not imported: the list shows it with the validator's finding and fix, and it cannot be chosen. The import never rewrites SKILL.md to make it pass; the only text it changes is the two source keys below. The imported skill's id is its `name`, and it is written to `skills/<name>/`; the archive's folder names are not used. Two found skills with the same `name` are both listed with their paths and neither can be chosen. Reference files past the cap of 20 are left out in name order and named. When `skills/<id>/` already exists, the Proposal replaces it and its diff is against the current files, naming the seats that attach it. When no skill is found the import ends in words and writes nothing."

(The architect may prefer "refuse when the id exists" for v1; then the Deferred row must not say "the same import Proposal".)

---

## Medium

### M1. Who writes the audit entry for an app request is open, and a blocked one would be written twice

Spine (AD-12): "every request with an app purpose, allowed or blocked, is an audit entry by that Human actor through the open project's writer, holding the purpose, host, path, outcome and, for the archive, its sha256".

Problem. The built `am-net` already writes `egress.blocked` itself, as `Actor::System { component: "am-net" }`, and it does not know the commanding actor. So either `am-net` writes these entries (then the request must carry the actor) or the import command does (then a blocked request gets two entries, one System and one Human). AD-25's list of what the log covers ("model calls, redacted tool calls, approvals, provenance pointers, config edits, blocked egress and migrations") is also left without allowed app requests.

Proposed: "`am-net` writes the entry, as it does for a blocked call; the app purpose carries the command's `Actor`, so the entry is by that Human and no second entry is written." And in AD-25: add "app requests (AD-12)" to the list.

### M2. "GitHub's name rules" is undefined, and dot segments are not refused

Spine (AD-12): "Owner, repository, ref and path are checked against GitHub's name rules and percent-encoded per path segment before a request is built."

Problem. The builder must guess the rules. This is the boundary that keeps the request inside `/repos/<owner>/<repo>/…`; `reqwest::Url` normalises `..` segments, so a ref of `../../users/x` would reach another endpoint.

Proposed: "Owner is 1 to 39 characters of `A-Z a-z 0-9 -`; repository is 1 to 100 characters of `A-Z a-z 0-9 . _ -` and is not `.` or `..`. Each ref and path segment is non-empty, is not `.` or `..`, and holds no control character, backslash, `?`, `#` or `%`. Every segment is percent-encoded. Anything else is refused in words before any request."

### M3. Pin: the lookup loop has no stop rule, and its budget is not stated

Spine (AD-38): "(`HEAD`, the default branch, when the URL names none; where `<ref>` ends and `<path>` begins is settled by lookups, shortest ref first; the answer must be 40 hex characters)".

Problems. It does not say that the first candidate that answers wins, which answers mean "try a longer ref" and which end the import (a missing or private repository answers "not found" to every candidate, so the loop would spend every request, 1 s apart, before refusing), or that the 8-request cap leaves at most 6 lookups. It also does not say that all chosen skills come from one download.

Proposed: "Candidates are tried shortest first and the first that answers with a SHA wins; the rest of the URL is the path. An answer that says the repository is missing ends the import in words ('not found, or private: private repositories are not supported in this version'); an answer that says the ref is unknown moves to the next candidate. At most 6 candidates are tried (AD-12's 8 requests less the archive and its redirect); past that the URL is refused in words. One command downloads one archive; every skill chosen from it records the same commit." The two answer codes should be checked on the live service, as the other facts were.

### M4. What the extract child expands, and how "named" is counted, are open; "import summary" is undefined

Spine (AD-38): "It is expanded only in the `agentic __extract` child under the AD-30 archive caps, and from there on it is a zip import".
Spine (AD-38): "Hooks, scripts, plugin manifests and harness folders (for example `hooks/`, `.claude-plugin/`) lie outside the skill folders, so they are left out, and the import summary names them."

Problems. A repository archive is not a one-skill zip. If the child expands everything, a large repository with one small skill fails the expansion caps; if it expands only what is needed, the caps mean something else. For naming: a repository holds many files outside the skill folders (README, tests, docs, the unticked skills). One builder names every file, another only known plugin folders. "Import summary" appears nowhere else in the spine; the ticket calls the existing list "Not imported". "Harness folders" and "plugin manifests" are the owner's words and are not defined.

Proposed: "The child first lists the archive's entry names (count-capped), then expands only the `SKILL.md` files for the list and the chosen skills' folders; the expansion caps apply to what is expanded. The import summary is the Proposal's 'Not imported' list. Inside a chosen skill's folder it names each left-out file. Outside the skill folders it names each top-level entry under the URL's path once, a folder with its file count, so hooks, scripts, plugin manifests (a tool's own plugin description file) and harness folders (a folder a coding tool reads for its own setup, such as `.claude-plugin/`) are always named. Skills not chosen are not listed there."

### M5. Source record: which crate edits the frontmatter, what "existing" means, and which Proposal field carries the record

Spine (AD-38): "The import writes two fixed keys into the imported SKILL.md's existing `metadata` map".
Spine (AD-38): "the Proposal's confirm step shows the URL, the commit and the two hosts the app contacted, and its History and audit entries hold the URL and the commit."

Problems. "Existing" reads as "the file already has one"; most files have none, and a `metadata` that is not a map fails the validator. Only `am-config` reads or writes config files, and `am-engine` does not depend on it, so who inserts the keys (and whether the rest of the file stays byte for byte) is open. AD-15's one Proposal shape has "source references (rows, runs)" only; the engine, the confirm dialog built in epic 5 and the History writer need one agreed field for URL, commit and hosts.

Proposed: "`am-config` has the one function that sets these two keys in a SKILL.md text: it adds the format's `metadata` map when the file has none and changes no other byte. The Proposal carries the URL and the commit as a source reference (a third kind beside rows and runs), and the confirm step, the History entry and the audit entry read them from there."

### M6. The URL form is narrower than the owner's and the ticket's

Spine (AD-38): "The owner gives `https://github.com/<owner>/<repo>` or `https://github.com/<owner>/<repo>/tree/<ref>/<path>`; any other host or URL form is refused in words."

Problem. The owner decision and the ticket write `github.com/<owner>/<repo>[/tree/<ref>/<path>]` with no scheme. Under the spine a pasted `github.com/obra/superpowers`, a trailing `/` or a clone URL ending `.git` is refused.

Proposed: "The scheme may be left out and `https` is assumed; one trailing `/` or `.git` is dropped. `http://`, `www.`, a query, a fragment, `/blob/` and every other form or host are refused in words."

### M7. AD-10's list is left stale, and the Proposal's standing under AD-10 (b) is not said

Spine (AD-38): "Until the owner applies the Proposal, its content is `UntrustedData` wherever it is held, shown or read (AD-10)."

Problem. AD-10 defines `UntrustedData` by a list, and AGENTS.md repeats it. Neither names the files of an import not yet applied. The builder of the Assistant (a reader of the store) reads AD-10, not AD-38. And a builder may ask whether a Proposal whose content is `UntrustedData` is tainted-origin, which is never approvable; that would make the feature dead, or lead someone to weaken the check.

Proposed: in AD-10's list add "the files of a skill, template or department import not yet applied (AD-38)". In AD-38 add: "The import Proposal is created by the Human command, not by a step, so it is not tainted-origin (AD-10 (b)); the owner's review of the full text is the control."

### M8. Ticket and parent requirement gaps (for Sofia; not fixed here)

- The ticket's verify has no test for: "no seat, tool or run can make an app request"; the update of the no-telemetry test's expected list; the 8-request cap, the 1 s spacing and the 3 redirects; the rate-limit wording; removal of the temporary folder; the source keys replacing values the downloaded file carried; names and descriptions shown as plain text; the CLI being refused while a stdio MCP server is enabled. The spine promises each.
- Initiative R60 still says "imported from a folder or zip"; the spine's map row says "folder, zip or GitHub URL". Only the epic's R13 was restated.
- PRD RULE-5 wording (see H3) and story 2.2's verify (see H2).
- The ticket's CLI rule (`--skill <name>`, none given lists and imports nothing) is not in the spine. It does not conflict, but with H5's id rule the name is the id, which makes it exact.

### M9. Trust-test home is not named

Spine (AD-12): "and a test proves it".

Problem. AD-33 gives every safety rule a failing-first test at a named path, and the gate runs the folder. The amendment names no file; the ticket puts everything in `rule_10_skills_grant_nothing.rs`, which is about grants, not egress.

Proposed: name the file in AD-12 (for example `crates/am-e2e/tests/trust/skill_import_*.rs`) and add the prefix to AD-33's list. AGENTS.md's trust-test line then needs the same word at its next refresh.

---

## Low

- **L1.** AD-15: "these decisions move to the UI and the CLI prints `agentic open --item <id>`" does not fit an import, which has no item. Add: "for an import the CLI names Import skill… in the UI".
- **L2.** AD-38 Before apply: say the extract child gets the archive on stdin and is never given the temporary folder's path (AD-13, AGENTS.md hostile-input policy), and that the import is refused before any request when the sandbox self-test has failed.
- **L3.** Binds and map: AD-12 Binds and the R60 row lack `am-domain` (the port and the purpose type, `RuntimeLayout`'s new path), `am-app` (the CLI command) and `am-store` (audit entries). AD-23 and AGENTS.md ban "named source sites and hosts" in `crates/`; add one clause in AD-12 that the two hosts are app infrastructure, like a provider's endpoint, and not department vocabulary.
- **L4.** AGENTS.md's network line ("All network goes through `am-net::EgressClient` and its allow-lists …") and its skills line need a refresh to mention app requests and the import; flag for the next `bmad-project-context` run.
- **L5.** Wording. "attach offer" and "touch panel" are used without definition in the new text; "lookup answer" is fine. The Pin sentence holds three rules in one bracket; split it. "It is a claim in a file, not proof of origin" is good; add that the UI words it as "says it came from" for a folder, zip or department import.
- **L6.** Source record details: the URL for a skill at the repository root (empty path) is not shown; a ref that holds `/` makes the stored URL ambiguous on a later read, so "the check needs no migration" in the Deferred row is a little strong. A third key is not needed; say the update check re-runs the Pin lookups.
- **L7.** "removed when the import ends" does not say when a UI import ends if the owner leaves the list open. Add a lifetime (for example 15 minutes after the list is shown, and on server stop).
- **L8.** Optional, beyond the owner's ask: every other unusual network use is "stated on the About page". One line there ("Importing a skill from GitHub contacts api.github.com and codeload.github.com; GitHub sees the repository name and this computer's IP address") would keep the About page true.

## Checked and found sound

- The skill format and the built loader are not changed: `kinds.rs` reads every `metadata` key as text, the two key names do not match the secret-key rules, and a 40-hex value does not match a provider-key prefix.
- The acceptance repository's two named skills pass the unchanged validator; their sibling files and `scripts/` are left out and named.
- The two Deferred rows cannot make two v1 units diverge: v1 behaviour for both (no credential; no code path fetches from the record) is fixed in the Rules.
- The fixed set is not config, so `is_widening` needs no new field; "importing a skill" was already a widening kind.
- The download writes nothing under the project root and uses a `RuntimeLayout` path, as AD-32 and AGENTS.md require; no child gets a project or data path.
- No new sentence weakens taint (AD-10), ToolGate (AD-11), the secrets rule, RULE-20 (public repositories need no key) or the "skill adds instructions only" policy.
