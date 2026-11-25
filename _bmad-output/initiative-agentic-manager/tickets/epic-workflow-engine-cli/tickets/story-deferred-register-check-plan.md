---
title: 'Deferred register check'
type: feature
ticket: 25
created: '2026-11-25'
status: approved
baseline_revision: 'e0a2115bd3babcdc05a5170b579a0d6e810174c8'
after: [24]
route: full
route_source: pinned
risk: low
review: quick
review_source: pinned
lenses_ran: []
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/docs/deferred-register.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/tickets.toml'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/epic-workflow-engine-cli.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-triggers-budgets-safety-floor/tickets/tickets.toml'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-desktop-release-trust-gate/tickets/tickets.toml'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-golden-run-so-far-on-every-pr-plan.md'
warnings:
  - 'Written from code reading only. No build or test was run while drafting. The counts below (176 rows, 76 assumed, 194 stories in 12 epics, every Target resolving, the release-mode results at 4.13 and 11.8) come from a read-only Python dry run of this plan''s rules over today''s files. The first run of the built check confirms or corrects them before any non-test xtask/src change.'
  - 'The local `main` ref is stale (7611520). `origin/main` is 0e78b56, and `docs/deferred-register.md` on HEAD (ea4b7c7, story/3-24) is identical to origin/main''s copy. The counts come from that file.'
  - 'Wording, for the reader: epic 4 story 13 and epic 11 story 8 call a due entry one whose target is "a story built before this gate". This plan follows 3.25''s own text, "every story it names builds before the gate story", which also matches the epic 3 Notes. Today the two readings differ at 4.13 for three entries (Design Notes, reading R1).'
deferred: []
---

<intent-contract>

## Intent

**Story:** epic 3 entry 25, "Deferred register check", `after = [24]`, `hitl = false`, risk low. It is part 2 of the owner's quality plan of 2026-11-24, split from the smoke test on 2026-11-25 (epic Notes; the smoke test is now story 26). Build order: 6, 23, 24, **25**, 22, 21. The baseline is set when the story starts, on main after 3.24 (PR #43) merges.

**Problem:** `docs/deferred-register.md` is not checked by anything.
- Its Rules (L9-L10) already say that `cargo xtask` checks it in `just check` and that a release mode exists, but neither has been built.
- Several mistakes would go unnoticed today: a mistyped Kind, Must close or Status value; a duplicate id; a row with a missing cell; a Target naming a story that a REPLAN renumbered or dropped.
- 76 of the 176 entries are still marked `(assumed)`: 52 `yes (assumed)` and 24 `no (assumed)`. The owner wants them:
  - listed by id on every `just check`, without failing it;
  - refused at the release gates, 4.13 (v0.1) and 11.8 (every later release).
- Those two gates need a mode that tells entries due by the gate apart from entries carried forward to later stories.

**Approach:**
- **A new xtask task, `register`, in two new modules:**
  - `xtask/src/stories.rs` reads the ticket tree into a story book: which `<epic>.<id>` exist, and each one's build position;
  - `xtask/src/register.rs` parses the register table, checks every row against the book, and runs the release mode.
- **Normal mode** is `cargo xtask register`. A new `register` recipe runs it, and `just check` lists that recipe. It fails on any finding and lists every `(assumed)` entry as a warning, by id, without failing.
- **Release mode** is `cargo xtask register --release --gate <epic>.<id>`. It fails on:
  - every normal finding;
  - every entry still marked `(assumed)`;
  - every `open` must-close entry due by the gate.
  
  It lists the open must-close entries carried forward past the gate without failing. Stories 4.13 (`just release-check`) and 11.8 (the release workflow) call it. This story adds no recipe for it.
- **Inputs:**
  - by default, `<root>/docs/deferred-register.md` and `<root>/_bmad-output/initiative-agentic-manager/tickets/`, where the root is the repository root;
  - `--register <file>` and `--tickets <dir>` replace either one, so tests can use planted files;
  - `--root <dir>` works as it does for the other tasks.
- **No new crate:** the ticket TOML is read by a strict line scan, as the golden rig's `story_order` already does (`crates/am-e2e/tests/golden/support/mod.rs` L263-L280). Anything the scan cannot read exactly is refused with exit 2.
- **Exit codes:** 0 pass, 1 findings, 2 setup (an unreadable input, an unreadable ticket tree, bad options, an unknown gate).

**Today, from the dry run:**
- **Normal mode passes.** All 176 rows (L27-L202) pass every rule below, and every Target story exists among the 194 entries of the 12 epics.
- **It prints 76 warnings,** listed by id in Design Notes.
- **Release mode would fail today** at both gates. This is not pinned by any test:
  - at gate 4.13: 76 assumed entries, 51 due and 13 carried forward;
  - at gate 11.8: 76 assumed entries, 64 due and none carried forward.

## Boundaries & Constraints

**Always:**
- **Tests first (owner standing rule):** the fixtures and `xtask/tests/register.rs` come first. The red run is logged in this plan before any `xtask/src` change.
- **xtask only:**
  - new: `xtask/src/stories.rs` and `xtask/src/register.rs`;
  - changed: `xtask/src/main.rs` (options, usage, dispatch, docs), the description line in `xtask/Cargo.toml`, and the justfile;
  - no crate under `crates/` changes.
- **No new dependency.** xtask's `[dependencies]` stay `serde_json` only. The ticket TOML is read by a line scan, and the register by plain string handling: no regex crate and no TOML crate.
- **Offline and read-only:** the task reads the register file and the ticket tree, writes nothing and opens no socket.
- **Fail closed:**
  - an input it cannot read, or a ticket tree it cannot read exactly, stops it with exit 2 and words naming the file and line;
  - one run reports every finding, not only the first.
- **Build order is table order:**
  - the epics' order is the order of the `[[epic]]` tables in `tickets/tickets.toml`;
  - each epic's order is the order of the `[[entry]]` tables in its own `tickets.toml`;
  - ids are never sorted or compared.
- **The register stays unchanged and passes:**
  - `docs/deferred-register.md` is not edited by this story;
  - its Rules L9-L10 already describe this check;
  - row 1 pins that today's file passes.
- **Generic:** the task and its fixtures name no department vocabulary. The planted tree uses neutral slugs (`epic-one`, `epic-two`, `epic-three`).
- **One story, one squash commit, on Opus.** Stop and report if the code passes about 4,000 lines.
- **Register:** this story adds no register row unless its review triage defers one.

**Never:**
- **No change to planning documents:** the register's header or rows, any `tickets.toml`, the epic Notes, the tickets of 4.13 and 11.8, and the architecture stay as they are (AGENTS.md policy). Reading R1 is recorded here for Sofia, not changed.
- **Nothing that belongs to 4.13 or 11.8:** no `just release-check`, no release workflow step and no recipe for the release mode.
- **No rule beyond the ticket:**
  - no cross-field rules (Kind against Status, an accepted entry against its Target);
  - no check that a Source story exists;
  - no check that a closed entry names its PR;
  - no id order or gap check;
  - no reading of plans' `status: built` to decide whether a target is done.
  
  Design Notes, "Not checked", lists these.
- **No `--warn-as-error`, environment variable or skip switch.** Normal mode never fails on `(assumed)`, and release mode always does (owner, 2026-11-25).
- **No `cargo clean`,** and no build of a workspace copy against the shared `CARGO_TARGET_DIR`.
- **Never read `_archive/`.**

## I/O & Edge-Case Matrix

Planted files live in `xtask/tests/fixtures/register/` and are named in Code Map. "The planted tree" is `xtask/tests/fixtures/register/tickets/`. Its build order is 1.1, 1.2, 1.5, 1.3, 1.4, 3.1, 3.2, 3.3, 2.1, 2.2: epics in table order 1, 3, 2, and epic 1's entries in table order 1, 2, 5, 3, 4.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | **Today's register passes (normal mode)** | `cargo xtask register` at the repository root, with the default paths | Exit 0. stderr holds one `warning: DR-NNN …` line for each entry whose Must close ends ` (assumed)`, in register order, and no other line. The test computes the expected set from the file itself (today 76). stdout: `register: 176 entries checked against 194 stories in 12 epics; 76 still marked (assumed), listed as warnings` | Any finding fails with exit 1 and its words |
| 2 | **Every value confirmed passes with no warning** | `clean.md` against the planted tree. It has each Kind, each Status, Must close `yes` and `no`, and the Targets `owner`, `unassigned`, `none`, one story, several stories (`1.5, 2.1`), and a story of an epic listed out of id order | Exit 0. No `warning:` line. The summary says `0 still marked (assumed)` | — |
| 3 | **An assumed entry is a warning by id** | `assumed.md`: `clean.md` with one row `yes (assumed)` and one row `no (assumed)` | Exit 0. Exactly those two ids are warned, each line naming the id, its Must close value and its Target | — |
| 4 | **A Target naming no story fails** | `no-such-story.md`: Target `1.9` (epic 1 has no entry 9), `4.1` (no epic 4), and `3.1, 2.7` (the second is missing) | Exit 1, with one finding per entry: `<file>:<line>: DR-NNN: Target names 1.9, which is not an entry of epic 1's tickets.toml`; `… names 4.1: there is no epic 4 in tickets.toml`. The other rows pass | Each finding names the id and the story |
| 5 | **A missing or extra column fails** | `missing-column.md`: one row with 7 cells and one with 9 | Exit 1. `DR-NNN: has 7 cells; the header defines 8 (Id, Kind, Source, Item, Decision, Target, Must close, Status)`, and the same for 9 | — |
| 6 | **A duplicate id fails** | `duplicate-id.md`: one id on two rows | Exit 1. `<file>:<line>: DR-NNN: duplicate id, first at line <n>`, once for each later copy | — |
| 7 | **An unknown Status fails** | `unknown-status.md`: Status `done` | Exit 1. `DR-NNN: unknown Status `done`; allowed: open, accepted, closed` | — |
| 8 | **An unknown Kind or Must close fails** | `unknown-values.md`: Kind `risk`, Kind `Deferred` (wrong case), Must close `maybe`, `yes(assumed)` and `assumed` | Exit 1. Each row is named by id, with its value and the allowed list | — |
| 9 | **Malformed cells fail** | `malformed-cells.md`: id `DR-7`; Targets `1.2 1.3`, `owner, 1.2` and `1.2,`; an empty Item; an empty Decision; Source `Sofia, REPLAN` (no leading story) | Exit 1. Each row is named by id. A row whose id is malformed is named by line, with its first cell quoted, and its other cells are still checked | — |
| 10 | **The table must be the one the header defines** | `no-table.md` (no `## Register` heading); `header-renamed.md` (`Must-close`); `row-after-table.md` (a blank line, then another `| DR-…` row) | Exit 1, with `<file>: no "## Register" table`; `<file>:<line>: the register table's header is not | Id | Kind | … | Status |`; `<file>:<line>: a table row after the register table ended at line <n>` | — |
| 11 | **Build order is table order, never id order** | The planted tree, in release mode at gate 3.2 and at gate 1.3 | At gate 3.2, a Target `2.1` builds after the gate (epic 2 is listed after epic 3). At gate 1.3, `1.5` builds before the gate and `1.4` after it. This shows in rows 15 and 16 | — |
| 12 | **The ticket tree is read exactly or not at all** | Temporary copies of the planted tree, each with one change: an epic folder missing; an `[[entry]]` with no `id`; one with two `id` lines; an entry id twice in one epic; an epic id twice; a `"""` string; the top-level `tickets.toml` missing; an `[[epic]]` with no `slug`. The shipped planted tree itself also holds a comment `# id = 9`, inline tables `{ epic = 2, needs = "…" }` in a multi-line `after`, and a `type = "spike"` entry | Each changed copy: exit 2, `register: <file>:<line>: <what>`, and no finding list. The shipped tree reads as the row header says: `1.9` is not a story, and spike `1.1` is | Words name the file and line |
| 13 | **The release interface** | `--release` with no `--gate`; `--gate 3.2` with no `--release`; `--gate 3`; `--gate x.y`; `--gate 3.9` (no such entry) | Exit 2, with words naming the problem, then the usage | — |
| 14 | **Release mode fails on assumed entries** | `release.md --release --gate 3.2`: an open `no (assumed)` row with Target `2.2` (it would be carried forward otherwise), and a closed `yes (assumed)` row | Exit 1. Each is named: `DR-NNN: still marked (assumed) (Must close …, Target …): gate 3.2 needs it confirmed` | — |
| 15 | **Release mode fails on due entries** | `release.md` at gate 3.2, open with Must close `yes`: Target `owner`; `unassigned`; `none`; `1.4` (before the gate); `1.4, 3.1`; `3.2` (the gate itself). Also an open `yes (assumed)` row with Target `3.1`. At gate 1.3: an open `yes` row with Target `1.5` | Exit 1. Each is named with the phrase `due by gate <g>`, its Target and the reason: `a decision is pending` (owner); `no story is named yet` (unassigned); `names no story` (none); `every story it names builds by the gate`. The `yes (assumed)` row is named twice: once as assumed, once as due | — |
| 16 | **Carried forward is listed, never failed** | `release.md` at gate 3.2, open with Must close `yes`: `2.1` (a lower epic id, but later in build order); `1.1, 3.3` (one story after the gate); `3.3` | No finding for these rows. stdout has `carried forward past gate 3.2: DR-NNN (Target …; after the gate: …)`, one line each, in register order. At gate 1.3, the `1.4` row is carried | — |
| 17 | **Release mode passes once each entry is settled** | `release-confirmed.md` at gate 3.2: `release.md` with each failing row confirmed, closed, or set to Must close `no` | Exit 0. The carried lines are on stdout. Summary: `register: release gate 3.2: <n> entries; 0 still marked (assumed); 0 due; 3 carried forward` | — |
| 18 | **What release mode ignores** | In `release.md` and `release-confirmed.md`: a closed `yes`; an accepted `no` with Target `none`; an open `no` with Target `1.1` (before the gate) | None of these is named on stderr or stdout | — |
| 19 | **Release mode keeps the normal findings** | `unknown-status.md --release --gate 3.2` | Exit 1, with row 7's finding | — |
| 20 | **`just check` runs the normal mode** | `justfile`, `.github/workflows/ci.yml` | `check:` lists `register`. The `register` recipe's body is `cargo xtask register`, with no `--release` and no `--gate`. CI's macOS job has `run: just check` | Fails naming the breach |
| 21 | **The real gates resolve** | `cargo xtask register --release --gate 4.13`, and the same with `11.8`, at the repository root | Exit 0 or 1, never 2, and the output names the gate. Pass or fail is not pinned: confirming the assumed values is 4.13's owner step | — |
| 22 | **Offline, read-only, no new crate** | The task and xtask's manifest | It reads only the two inputs and writes nothing, with no network. xtask's `[dependencies]` stay `serde_json` | Checked in review, not by a test |

</intent-contract>

## Code Map

- **`xtask/src/main.rs`:**
  - module docs L1-L11: add story 3.25's `register` (normal mode in `just check`; release mode for 4.13 and 11.8);
  - `mod` list L14-L21: add `mod register;` and `mod stories;`;
  - `USAGE` L26-L33: add `register [--root <dir>] [--register <file>] [--tickets <dir>] [--release --gate <epic>.<id>]`, with a one-line gloss;
  - dispatch L48-L69: add `"register" => register::run(&register::Inputs { … })`;
  - `Options` L95-L100 gains `register: Option<PathBuf>`, `tickets: Option<PathBuf>`, `release: bool` and `gate: Option<String>`;
  - `Options::parse` L102-L122: `--release` takes no value, and `--gate`, `--register` and `--tickets` take one;
  - `Failure` L90-L93 and the `Ok(summary)` printing L71-L73 are unchanged. `register::run` prints its own warnings (stderr) and carried lines (stdout) before it returns.
- **`xtask/src/stories.rs` (new):**
  - `pub struct Book`: the stories in build order, a map from `(epic, id)` to position, and the epic ids;
  - `pub fn load(tickets: &Path) -> Result<Book, String>`: the line scan and every refusal in Design Notes;
  - `Book::position`, `Book::has_epic`;
  - `pub fn parse_story("<e>.<i>") -> Option<(u32, u32)>`, shared with the gate option.
- **`xtask/src/register.rs` (new):**
  - `pub struct Inputs { register, tickets, display_root, release: Option<(u32, u32)> }`;
  - `pub fn run(&Inputs) -> Result<String, Failure>`;
  - `parse_table(text) -> (Vec<Row>, Vec<Finding>)`;
  - `check_row(&Row, &Book) -> Vec<Finding>`;
  - `release(&[Row], &Book, gate) -> (Vec<Finding>, Vec<Carried>)`;
  - the types: `Row { line, cells }`, `Entry { id, kind, target: Target, must_close: MustClose, status: Status }`, `enum Target { Owner, Unassigned, Nothing, Stories(Vec<(u32, u32)>) }`, `MustClose { yes: bool, assumed: bool }`, and `enum Status { Open, Accepted, Closed }`;
  - finding words as in the matrix.
- **`xtask/Cargo.toml` L3:** the description also names `register` (one line).
- **`justfile`:**
  - header comment L1-L9: one line for story 3.25;
  - `check:` L17: add `register` after `seam-guard` and before `validate`;
  - a new recipe after `seam-guard` (L69-L70), with a two-line comment naming the register, the warnings and the release mode's callers (4.13, 11.8): `register:` with the body `cargo xtask register`.
- **`xtask/tests/register.rs` (new):** the tests in the Tests-first list, with these helpers:
  - `run(register, tickets, extra)` over `common::xtask`;
  - `fixture(name)`;
  - `warned_ids(stderr)`;
  - `copy_tree_with(change)`, which uses `tempfile` and `common::write`;
  - a local `recipe()`, the same 10 lines as `gate_runs_egress_scans.rs` L13-L22, so that file is left unchanged.
- **`xtask/tests/fixtures/register/` (new):**
  - **`tickets/tickets.toml`:** three `[[epic]]` tables in the order 1 (`epic-one`), 3 (`epic-three`), 2 (`epic-two`), each with `id`, `slug`, `title`, and on epic 3 a multi-line `after = [ { epic = 1, needs = "…" }, ]`;
  - **`tickets/epic-one/tickets/tickets.toml`:**
    - entries in the order 1 (`type = "spike"`), 2, 5, 3, 4;
    - a comment line `# id = 9 is not an entry`;
    - an entry with `after = [2, "1.1"]`;
  - **`tickets/epic-three/tickets/tickets.toml`:** entries 1, 2, 3;
  - **`tickets/epic-two/tickets/tickets.toml`:** entries 1, 2;
  - **register files,** each with a title, a `## Register` heading and the header and separator copied from the real register (L25-L26):
    - `clean.md`, `assumed.md`, `no-such-story.md`, `missing-column.md`, `duplicate-id.md`, `unknown-status.md`, `unknown-values.md`, `malformed-cells.md`;
    - `no-table.md`, `header-renamed.md`, `row-after-table.md`;
    - `release.md`, `release-confirmed.md`.
- **Read only:**
  - `xtask/tests/common/mod.rs` (`repo_root`, `xtask`, `stderr`, `stdout`, `write`);
  - `xtask/tests/gate_runs_egress_scans.rs` (the justfile-reading pattern);
  - `crates/am-e2e/tests/golden/support/mod.rs` L263-L280 (`story_order`, the precedent for the line scan; xtask does not depend on it);
  - `.cargo/config.toml` (`xtask = "run --quiet --package xtask --"`);
  - `.githooks/pre-commit` (runs `just check`, so from now on the register is checked at every commit in the main checkout).
- **Unchanged:**
  - `docs/deferred-register.md`, every `tickets.toml`, and `.github/workflows/ci.yml`;
  - `xtask/denylist.txt`. The denylist scans only `crates/` and `web/src`, and takes terms only from the top-level `templates/` and `fixtures/` folders, so `xtask/tests/fixtures/` adds none.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:** #43 (3.24) is merged, this plan's baseline is set to main after it, and this plan is committed on branch `story/3-25`.
- [ ] **Tests first, with no `xtask/src` change:**
  - the planted tree and the thirteen register files;
  - `xtask/tests/register.rs`;
  - log the red run.
- [ ] **xtask:** `stories.rs`, `register.rs`, and `main.rs` (options, usage, dispatch, docs); the manifest description.
- [ ] **justfile:** the `register` recipe, its place in `check:`, and the header line.
- [ ] **Green:**
  - `cargo nextest run -p xtask`;
  - `cargo xtask register` on the repository: record the summary and the warning count;
  - `--release --gate 4.13` and `--gate 11.8`: record the counts in Implementation Notes, and say where they differ from Intent's dry run.
- [ ] **Size (owner):** one squash commit. Stop and report if the code passes about 4,000 lines.

**Acceptance Criteria:**
- **Each matrix row:** given the row, when its test runs, then the exit code, findings, warnings, carried lines and words match it.
- **The verify:**
  - the check passes on the seeded register (row 1);
  - a planted entry naming no story, one missing a column, a duplicate id and an unknown status each fail, naming the entry id (rows 4-7);
  - with every value confirmed it passes with no warning (row 2);
  - an `(assumed)` entry is a warning by id and still passes (row 3);
  - for a planted gate story, release mode fails on an `(assumed)` entry, on an open must-close entry with Target `owner` or `unassigned`, and on one whose target builds before the gate, naming each (rows 14-15);
  - it passes once each is confirmed, closed or not must-close (row 17);
  - it lists, without failing, an open must-close entry whose target builds after the gate (row 16);
  - `just check` runs the normal mode (row 20) and is green.
- **The workspace:** `just check` is green. `git diff --stat docs/ _bmad-output/` is empty apart from this plan. Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-25, manager: approved under the owner's standing rules (no owner-level question). The due rule follows this ticket's "every story it names builds before the gate". 4.13's and 11.8's wording ("a story built before this gate") goes to Sofia's next replan to match. Strict readings kept: an entry targeting the gate story itself is due, and an open must-close entry with target `none` is due.

## Design Notes

### Parsing rules: the register

- **Where the table is:**
  - From the line `## Register` (today L23), the first non-blank line must be the header row. After trimming each cell it is exactly `Id | Kind | Source | Item | Decision | Target | Must close | Status`, in that order.
  - The next line is the separator: 8 cells, each of dashes with an optional `:` at either end.
  - The rows are every following line that starts with `|`, up to the first line that does not.
- **What is a finding:**
  - no `## Register` heading, a second one, a different header or a missing separator;
  - any line after the table's end that starts with `|`, so a row cannot slip past the check behind a blank line.
- **Not read:** the text before `## Register` (the title, Rules and Columns).
- **Splitting a row:**
  - trim the line, drop one leading and one trailing `|`, then split at every `|` that is not preceded by `\`;
  - `\|` is a literal pipe, and each cell is trimmed;
  - code spans get no special treatment: in GFM a pipe inside a code span still splits a cell unless it is escaped;
  - today no cell holds a pipe, and every row splits into 8 cells.
- **The columns.** Every match is exact and case-sensitive, so `Yes`, `Open`, `Deferred`, `yes(assumed)` and `yes  (assumed)` are unknown values.

| Column | Allowed | Checked |
|---|---|---|
| Id | `DR-` and three digits (`DR-001` to `DR-999`), or more digits with no leading zero (`DR-1000`) | Shape, and unique in the table (the first line is named) |
| Kind | `deferred`, `accepted risk`, `residual`, `owner question` | Value |
| Source | Not empty. It starts with `<epic>.<id>` (positive integers, no leading zero), followed by the end, a space, `,`, `;` or `(` | Shape only: whether that story exists is not checked (the ticket names only the Target) |
| Item, Decision | Not empty | Not empty |
| Target | Exactly `owner`, `unassigned` or `none`; or `<epic>.<id>` stories separated by commas, with optional spaces | Grammar: no empty element, and a keyword never mixed with stories. Each story must exist (next section) |
| Must close | `yes`, `no`, `yes (assumed)`, `no (assumed)` | Value |
| Status | `open`, `accepted`, `closed` | Value |

- **The `(assumed)` marker:**
  - it is recognised only as the Must close suffix ` (assumed)`, which is where the register's Columns put it (L20);
  - in Kind, Target or Status the text is an unknown value;
  - in Source, Item or Decision it is prose and is ignored (today it appears in no cell but Must close).
- **One run lists every problem:**
  - a row whose Id is malformed is named by line, with its first cell quoted, and its other cells are still checked;
  - a row with the wrong cell count is named by its first cell if that is an id, or by line otherwise, and its cells are not checked further.
- **Today:** the header is at L25, the separator at L26 and the rows at L27-L202. All 176 rows pass:
  - Kinds: 89 accepted risk, 58 deferred, 21 residual, 8 owner question;
  - Must close: 87 `no`, 13 `yes`, 52 `yes (assumed)`, 24 `no (assumed)`;
  - Status: 89 open, 86 accepted, 1 closed (DR-171);
  - Targets: 86 `none`, 15 `unassigned`, 4 `owner`, and 71 story lists, which all resolve.

### A story that exists, and build order

- **The book:**
  - The top-level `tickets/tickets.toml` gives each `[[epic]]` table's `id` and `slug`, in table order.
  - Each epic's `<tickets>/<slug>/tickets/tickets.toml` gives each `[[entry]]` table's `id`, in table order.
  - Every entry counts, whatever its `type` (`story` or `spike`): the register names entries by `<epic>.<id>`, and ids are permanent.
- **A Target story `<e>.<i>` exists** when `e` is an `[[epic]]` id and `i` is an `[[entry]]` id in that epic's file. The finding words say which of the two is missing.
- **Build position:**
  - a story's position is (the index of its epic's `[[epic]]` table in the top-level file, the index of its `[[entry]]` table in the epic's file), compared lexicographically;
  - this is the concatenation of every epic's entries in epic table order;
  - ids are never compared. AGENTS.md says so too: "Table order in each `tickets.toml` is the build order, not the id".
- **Today's order:**
  - the epics run 1, 2, 3, 4, 5, 6, 7, 8, 12, 9, 10, 11, with 194 entries;
  - in epic 3, 23, 24, 25 come before 22, 21;
  - in epic 4, 16 comes after 6; 15 and 17 come after 10; 19 and 18 come after 11; 13 is last;
  - epic 9 starts at entry 5;
  - in epic 11, 16 comes after 2, 17 comes after 7, and both come before 8.
- **The line scan** (no TOML crate; the golden rig's precedent, made stricter):
  - each line is trimmed;
  - a line starting with `#` is a comment;
  - a table header is a line of the form `[[name]]` or `[name]`, where the name has bare-key characters and dots, spaces are allowed inside the brackets, and a trailing `#` comment is allowed;
  - any header other than `[[epic]]` (top-level file) or `[[entry]]` (epic file) ends the current table;
  - inside the current table, a line whose key (the text before the first `=`, trimmed) is exactly `id` gives the id: a decimal integer, optionally followed by a comment;
  - `slug` gives a basic string;
  - lines inside multi-line arrays, such as `{ epic = 1, needs = "…" }` and `"1.1",`, never have the key `id`.
- **Refusals (exit 2, `<file>:<line>: <what>`):**
  - any `"""` or `'''`: the scan cannot read multi-line strings, and none of today's 13 files has one;
  - a table with no `id`, or with two;
  - an `[[epic]]` with no `slug`, or with two;
  - an id that is not a positive integer;
  - a duplicate epic id, or a duplicate entry id within one epic;
  - a missing top-level file, epic folder or epic file.
  
  A quoted or dotted key, such as `"id" = 3`, is not read, so that table has no id and is refused. Nothing is guessed.

### The release mode

- **Interface:** `cargo xtask register --release --gate <epic>.<id> [--root <dir>] [--register <file>] [--tickets <dir>]`.
  - `--release` needs `--gate`, and `--gate` needs `--release`.
  - The gate is two positive integers joined by a dot, and it must exist in the book.
  - Anything else exits 2 with words, then the usage.
  - Any existing story may be the gate (the verify plants one). 4.13 and 11.8 are the callers today.
- **Its findings,** in register order:
  1. every normal finding;
  2. every entry whose Must close carries `(assumed)`, whatever its Status ("fails while any entry is still marked `(assumed)`");
  3. every `open` entry whose Must close is `yes` or `yes (assumed)` (an assumed yes counts as yes, per the epic Notes) and that is due by the gate.
  
  An entry that is both assumed and due gets two findings.
- **Due by the gate:**
  - the Target is `owner` or `unassigned`;
  - or the Target is `none`, which names no story, so "every story it names builds before the gate" holds vacuously. An open must-close entry that owes nothing contradicts itself, so the check fails closed. Today no row has it;
  - or no story the Target names builds after the gate. The gate story itself counts as due: it is not later, and it runs this check at its own end (reading R2).
- **Carried forward:** an `open` must-close entry whose Target names at least one story that builds after the gate.
  - It is never a finding.
  - stdout lists it in register order: `carried forward past gate <g>: DR-NNN (Target <t>; after the gate: <stories>)`.
  - The lines are printed whether the run passes or fails.
- **Not counted:**
  - `accepted` and `closed` entries;
  - Must close `no`;
  - Must close `no (assumed)`, which fails only as assumed.
- **Words:** each finding names the id, the Target and the gate, with one of the phrases `still marked (assumed)` or `due by gate <g>`.

### Output and exit codes

- **Findings:** stderr, one line each, `<register path>:<line>: DR-NNN: <words>`.
  - The path is shown relative to the root when it lies under it.
  - The existing `main` then prints `register: FAILED with <n> finding(s)` and exits 1.
- **Warnings (normal mode only):** stderr, one line per assumed entry, in register order: `warning: DR-NNN: Must close is still marked (assumed): <value>; Target <t>`.
- **Summary:** stdout, through `main`'s `Ok(summary)`.
  - Normal: `register: <n> entries checked against <m> stories in <k> epics; <a> still marked (assumed), listed as warnings`.
  - Release, passing: `register: release gate <g>: <n> entries; 0 still marked (assumed); 0 due; <c> carried forward`.
- **Exit codes:** 0, 1 or 2, as in Intent. Setup words go through `Failure::Setup`, as in the other tasks.
- The exact words beyond the pinned phrases are the coder's call (owner standing rule: "error and fix wording").

### `just check` (row 20)

- **The recipe:** `register:` with the body `cargo xtask register`. It sits in `check:` after `seam-guard` and before `validate`. It is the normal mode only.
- **Today it passes with these 76 warnings:**
  - **`yes (assumed)`, 52:** DR-001, 002, 010, 013, 014, 017, 022, 023, 025, 036, 039, 052, 053, 057, 058, 067, 075, 076, 077, 078, 081, 086, 091, 092, 094, 095, 107, 108, 109, 110, 111, 112, 124, 126, 127, 128, 129, 130, 131, 134, 135, 137, 138, 146, 147, 151, 156, 157, 158, 159, 168, 173.
  - **`no (assumed)`, 24:** DR-003, 007, 015, 018, 019, 020, 033, 034, 035, 079, 080, 083, 085, 115, 116, 123, 125, 136, 139, 154, 160, 163, 169, 170.
- **Row 1's test does not hard-code these.** It computes the set from the file, so it stays green as the owner confirms entries.
- **What changes for the build:**
  - CI's macOS job runs `just check`, so every PR runs the check;
  - the Linux job runs `cargo nextest run --workspace --exclude am-desktop`, and xtask is a default member, so row 1's test also runs there;
  - the pre-commit hook runs `just check`, so a documents-only commit in the main checkout that breaks the register is refused;
  - documents-only worktree commits use `--no-verify` (AGENTS.md), and CI still checks them.

### Today's release mode, dry run (for 4.13's and 11.8's planners; not pinned)

| Gate | Assumed | Due | Of which `owner` / `unassigned` | Carried forward |
|---|---|---|---|---|
| 4.13 | 76 | 51 | 3 (DR-017, 075, 135) / 10 (DR-039, 052, 058, 076, 081, 091, 109, 146, 151, 168) | 13: DR-005 (11.3), 010 (11.5), 022 (9.10), 036, 057, 121 (11.17), 138, 144 (9.10), 149 (6.7), 156, 157 (9.16), 158 (9.16), 173 |
| 11.8 | 76 | 64 | Same | 0: 11.3, 11.5 and 11.17 all precede 11.8 in epic 11's table |

### Readings (decided within the ticket; owner standing rule "decide yourself")

- **R1, "every" over "a story".**
  - 3.25's description, its verify ("one whose target builds before the gate", "whose target builds after the gate") and the epic 3 Notes all say due means "every story it names builds before the gate story". The 4.13 and 11.8 tickets say "a story built before this gate".
  - This plan follows 3.25, the ticket being built.
  - Today three entries differ at gate 4.13. Each names one story before 4.13 and one or more after it, so here they are carried forward, not due:
    - DR-036 (4.11 before; 5.16, 5.19, 9.8 and 9.10 after);
    - DR-057 (4.6 before; 12.22 after);
    - DR-138 (3.16 before; 9.7 after).
  - Sofia may align the 4.13 and 11.8 wording. Nothing here changes them.
- **R2, the gate story as a Target is due.** "Before the gate" and "after the gate" leave the gate itself undecided. It is due because it is not a later story ("carried forward to later stories"), and because otherwise an entry aimed at the gate would pass that very gate while still open. No row targets 4.13 or 11.8 today.
- **R3, an open must-close `none` is due** (above). No row has it today.
- **R4, every entry type counts** as a story.
- **R5, Source:** its shape is checked, as Columns L16 defines it, but its existence is not.
- **R6, warnings:** one line per assumed entry, as the owner asked ("lists every entry … as a warning, by id"). Today that is 76 lines in `just check`.
- **R7, release mode checks `(assumed)` on every entry,** whatever its Status (ticket: "any entry"). Today every assumed entry is open.

Each reading except R1 makes the gate stricter, never looser. R1 is the ticket's own rule.

### Not checked (outside the ticket)

- Cross-field consistency. For example, DR-084 is an `accepted risk` that is `open` with Target 9.11, which the Columns allow.
- Whether a Source story exists.
- Whether a `closed` entry names its PR (Rules L8).
- Id order and gaps.
- Whether a Target story is already built (plans' `status`), or whether an open entry's story was built without closing it.

Each could be a later story's rule if the owner asks for one. None is a register row.

### Register rows

None. This story builds the check and defers nothing at planning time.

## Questions for the owner

None. Every choice above is an engineering reading inside the ticket's words (Readings R1-R7). Each one makes the gate stricter, except R1, which is the ticket's own rule. R1's wording difference from 4.13 and 11.8 is recorded for Sofia; it is not an owner question.

## Tests-first list

All tests run the built `xtask` binary (`CARGO_BIN_EXE_xtask`), on planted files under `xtask/tests/fixtures/register/` or on temporary copies of the planted tree. There is no network and nothing is written outside a tempdir.

Every test asserts words, not only an exit code. So tests that expect exit 2 are red too: they get xtask's own refusal words, not theirs.

| Test | Matrix rows | Expected red reason |
|---|---|---|
| `the_repository_register_passes_and_warns_each_assumed_entry` | 1 | Exit 2: `xtask: unknown task `register`` |
| `a_register_with_every_value_confirmed_passes_with_no_warning` | 2 | Exit 2: `xtask register: unknown option `--register`` (`Options::parse` runs before the dispatch) |
| `an_assumed_entry_is_a_warning_by_id_and_still_passes` | 3 | Same unknown option |
| `a_target_naming_no_story_fails_with_the_entry_id` | 4 | Same; exit 2, not 1, and no id in the words |
| `a_row_missing_or_adding_a_column_fails_with_the_entry_id` | 5 | Same |
| `a_duplicate_id_fails_naming_it_and_the_first_line` | 6 | Same |
| `an_unknown_status_fails_with_the_entry_id` | 7 | Same |
| `an_unknown_kind_or_must_close_fails_with_the_entry_id` | 8 | Same |
| `malformed_cells_fail_with_the_entry_id_or_line` | 9 | Same |
| `the_table_must_be_the_one_the_header_defines` | 10 | Same |
| `build_order_is_table_order_never_id_order` | 11, 15, 16 (gates 3.2 and 1.3) | Same |
| `an_unreadable_ticket_tree_stops_with_file_and_line` | 12 | Exit 2, but with xtask's unknown-option words, not `<file>:<line>` |
| `release_mode_needs_a_known_gate` | 13 | Exit 2, but with `unknown option `--release``, not the gate words |
| `release_mode_fails_on_each_assumed_entry` | 14 | Unknown option |
| `release_mode_fails_on_each_open_must_close_entry_due_by_the_gate` | 15 | Unknown option |
| `release_mode_lists_entries_carried_forward_without_failing` | 16 | Unknown option |
| `release_mode_passes_once_each_entry_is_settled` | 17, 18 | Unknown option |
| `release_mode_keeps_the_normal_findings` | 19 | Unknown option |
| `just_check_runs_the_register_check_in_normal_mode` | 20 | `just check` does not list `register`, and there is no `register` recipe |
| `the_real_release_gates_resolve` | 21 | Exit 2: unknown option `--release` |

- **Red-run command:** `cargo nextest run -p xtask --test register`. It is logged here, with each test's failing words, before any `xtask/src` change.
- **Green on arrival:** none expected.
- **Fixtures:** they are test data, written by hand from this plan and never generated by the check. Each register file's rows use the real register's header and the planted tree's stories.

## Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code:** about 520-650 lines, far below the stop.
  - `xtask/src/register.rs`: 330-420 lines (table parse about 110, the row checks about 110, the release mode about 70, output and `run` about 60, and docs).
  - `xtask/src/stories.rs`: 130-170 lines (the scan, its refusals, `Book`, `parse_story`).
  - `xtask/src/main.rs`: +35-45 lines (four options, usage, dispatch, docs).
  - `justfile`: +6-8 lines. `xtask/Cargo.toml`: 1 line.
- **Tests:** about 520-640 lines.
  - `xtask/tests/register.rs`: 20 tests at 15-25 lines each, plus about 80 lines of helpers.
- **Fixtures (test data):** about 230-280 lines.
  - The planted tree: 4 files, about 70 lines.
  - The thirteen register files: 10-20 lines each.
- **Register:** none.

## Verification

- **Tests:**
  - `cargo nextest run -p xtask --test register`, then `cargo nextest run -p xtask`;
  - `cargo clippy -p xtask --all-targets -- -D warnings` and `cargo fmt --all -- --check`.
- **On the repository** (record each in Implementation Notes):
  - `cargo xtask register`: exit 0, the summary line, and 76 warnings (or today's count, with the difference explained);
  - `cargo xtask register --release --gate 4.13`: exit 1; the assumed, due and carried counts, against the dry run (76, 51, 13);
  - `cargo xtask register --release --gate 11.8`: exit 1 (76, 64, 0).
- **By hand, once each, on a temporary copy** passed with `--register`, never by editing `docs/deferred-register.md`:
  - set one Status to `done`: the check fails, naming that id;
  - set one Target to `3.99`: the check fails, naming that id and `3.99`;
  - remove that row's last cell: the check fails, naming the id.
- **Unchanged:** `git diff --stat docs/ _bmad-output/` shows nothing but this plan.
- **The gate:** `just check` is green, its output shows the `register` recipe's warnings, and the run exits 0.
- **No `cargo clean`,** and no build of a workspace copy against the shared target.
- **Merge** only with both CI jobs green after review triage.

## Implementation Notes
