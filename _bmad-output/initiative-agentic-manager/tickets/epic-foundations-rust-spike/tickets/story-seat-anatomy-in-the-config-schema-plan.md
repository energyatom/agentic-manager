---
title: 'Seat anatomy in the config schema'
type: feature
ticket: '12'
created: '2026-11-20'
status: built
baseline_revision: '6783fb57ead26ecec81e8802cbe3f74816f60d55'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/story-config-loader-and-agentic-validate-plan.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `am-config` reads a seat's name, role, model, tools and budget only. The owner's seat anatomy (capabilities, linked folders, text fallback, identity, skills, knowledge, links) is rejected as unknown keys, skill folders are not read, and `sources/*.yaml` get only the generic checks (ticket 1.12; AD-30, AD-35, AD-38, AD-39, AD-46). Story 1.5's templates and epics 3, 5, 6, 9 and 12 need these fields.

**Approach:** extend the 1.4 loader, without rewriting it:
- read the new seat keys;
- expand `capabilities` through one fixed table into the seat's built-in grants;
- add a skill-folder loader;
- add a schema for Source files.

Every problem is reported as `file:line` with a fix. **Tests first (owner rule):** fixtures and failing tests come first and their red output is recorded.

**Owner decisions (2026-11-20):**
- **(a) Scope:** kept whole. Seat anatomy and Source validation are one story, and the full plan is kept (about 3,000 tokens).
- **(b) Allow-list:**
  - A seat's host allow-list is `tools.yaml` `egress_hosts`, read through one snapshot function, `allowed_hosts(seat)`.
  - No per-seat field is added; a later per-seat list changes only that function.
- **(c) Account kinds:**
  - New `provider` values: search `brave | tavily | exa | serper | searxng-remote`, and crawl-service `firecrawl` (Firecrawl-compatible).
  - The kind follows from the provider. `anthropic | openai | openai-compatible` are model Accounts.
  - `firecrawl` and `searxng-remote` require `base_url`.
- **(d) Globs:**
  - Pin `globset` exactly at its current release and add `docs/crate-notes/globset.md`. It is dual-licensed MIT or Unlicense, and MIT is allowed.
  - `cargo deny` must pass on its whole dependency graph. If it does not, stop and report rather than adding an exception.
  - Record the pin for the architect in Implementation Notes, as the jsonschema finding was.
- **(e) Source limit caps:**
  - `max_items_per_load` 1–10,000;
  - `max_bytes_per_load` 1 byte to 1 GB;
  - `max_loads_per_day` 1–96;
  - `max_depth` 0–10;
  - `rate_per_host` at least 1 s;
  - `max_spend_per_load` at least 0, as money.

## Boundaries & Constraints

**Always:**
- **Capability table** (AD-46). A missing entry is off, except `folder_access`, which is on. A legacy `documents` key is read as `folder_access`; both keys present is an error.

  | Capability | Grants |
  |---|---|
  | web_search | `web_search` |
  | browse | `read_website`, `read_url`, `arxiv` |
  | crawl | no tool; the seat may own `crawler` Sources |
  | folder_access | `list_files`, `read_file`, `search_knowledge`; `write_file` only when a folder is `read_write` |
  | files | `workspace_read`, `workspace_write` |
  | tables | `table_read`, `table_write` |
  | memory | `remember` |
  | handoff_ask | `ask_seat` |
  | calculate | `calculate`, `stats`, `query_table` |

  `read_skill_file` is added when the seat has a skill attached. The seat's grants are this expansion, plus built-in tools that belong to no capability and are listed in `tools:`, plus the MCP tools listed there.
- **Capability errors:** a listed built-in tool whose capability is off; a `crawler` Source whose owner seat has `crawl` off.
- **Capability providers:** `builtin` or an Account id.
  - `web_search` takes a search Account.
  - `browse` and `crawl` take a crawl-service Account.
  - Every other capability takes `builtin` only.
  - Any other Account is "the wrong kind".
- **Skills give no access** (AD-38): `allowed-tools` is stored as `asks_for`, and the expansion never reads a skill.
- **Folders:**
  - `path` is project-relative or `~/`-relative and is resolved lexically.
  - Refused: an absolute path, `/`, `~`, `~/Library` or anything under it (the app data dir lives there), any path with a `.agentic` component, and a path that escapes to one of these.
  - A missing path is a warning; a path that exists and is not a directory is an error.
  - Folder `id`s are kebab-case and unique within the seat.
- **Text limits:** `goal` is at most 500 characters and `backstory` at most 2,000, counted as chars.
- **Avatar:** `avatar.tint` is one of slate, steel, sage, stone, plum, clay (DESIGN).
- **Skills:**
  - `skills/<id>/SKILL.md`: `name` (equal to `<id>`) and `description` are required; `license`, `metadata` and `allowed-tools` are optional; the body is at most 64 KB.
  - `references/` holds at most 20 files, each md, txt, csv, json, yaml or yml and at most 256 KB.
  - Errors: any other file or folder, `scripts/`, a file with an executable bit.
  - Skill files are read through `Loader::read`, so they count in the snapshot hash. The secret and comment checks apply to them.
- **Knowledge:** entries are `{ file, note? }`. `file` is relative, with no `..`. A missing `.agentic/knowledge/<seat-id>/<file>` is a warning.
- **Links:** entries are `{ title, url, group? }`. `url` is http or https, to any host. A host outside the seat's allow-list is a warning.
- **Sources** (AD-39):
  - `type` is one of web-search, http-reader, crawler, api, feed, local-repository, knowledge, database, mcp, action.
  - `owner_seat` must exist. An `account` must exist and be the right kind: crawler and http-reader take crawl-service; web-search takes search; api takes any; the other types take none.
  - Scope URLs (`seeds`, `url`) are http or https, and `include`/`exclude` globs compile.
  - `limits` must be within their caps.
  - `output` has exactly one of `table` (+ `key` + `mapping`, all checked against the table's columns), `records: true`, or `knowledge: seat | team`.
  - `trigger` reuses the workflow trigger reader.
  - `render: true` is allowed only on http-reader and crawler.
  - `terms_checked` is a date on website types (http-reader, crawler, feed); an empty or missing one is a warning.
  - A scope host outside the owner seat's allow-list is a warning.
- **Crawl-service Accounts:** `base_url` is required, and must be https or http to a loopback host (the existing `web_url`).
- **Workflow steps** accept `seats`, `join`, `over_folder`, `routes`, `files`, `handoff`, `review` and `max_parallel` as declared but unchecked keys (AD-40..AD-42), as story 1.5's ticket requires of this story; stories 3.14–3.16 and 4.15 check them later.
- **Hygiene:** fixtures are synthetic and department-neutral, with no YAML comments outside `broken*`. The denylist stays green.

**Never:**
- Expose a function from a skill to a grant.
- Run or parse code in a skill.
- Touch the network or the keychain.
- Edit the architecture, the tickets or other stories' scope.
- Write `templates/` (story 1.5) or `is_widening` (4.18).
- Implement tools, connectors or ToolGate.
- Canonicalise through symlinks at validate time.

## I/O & Edge-Case Matrix

| Scenario | Input | Expected | Exit |
|---|---|---|---|
| Default capabilities | Seat with no `capabilities` | Grants = list_files, read_file, search_knowledge | 0 |
| read_write folder | `folders: [{id: work, path: work, access: read_write}]` | + write_file | 0 |
| Refused folder | path `~/Library/x` or `.agentic/x` | `seats/x.md:L` error, fix names allowed forms | 1 |
| Missing folder | path `missing-dir` | warning at L | 0 |
| Legacy key | `documents: {on: true}` | Read as folder_access | 0 |
| Tool while cap off | `tools: [read_url]`, browse off | error at the tool's line, fix "switch on browse" | 1 |
| Wrong-kind provider | web_search provider = crawl-service Account | error | 1 |
| Full seat | goal, backstory, tint, 1 skill, 2 knowledge, 3 links | snapshot holds every field | 0 |
| Broken skills and seat | unknown skill, no description, scripts/run.sh, tool.py, 70 KB body, 2,500-char backstory, ftp:// link | one error each, `file:line` + fix | 1 |
| Soft problems | link host off allow-list; missing knowledge file | two warnings | 0 |
| Skill asks for a tool | skill with `allowed-tools: read_website` | grants identical to the same skill without `allowed-tools` | — |
| Broken crawler Source | unknown output table, bad glob, `ftp://` seed | three errors | 1 |
| Website Source | empty `terms_checked`, host off allow-list | two warnings | 0 |
| Render and base URL | `render: true` on a feed; crawl Account `http://10.0.0.5` | two errors | 1 |

</frozen-after-approval>

## Code Map

- `crates/am-config/src/kinds.rs` -- holds the seat reader at :702 and its allowed keys at :710-724; extend it. `source` at :1247 is a stub; replace it with a full reader. `ACCOUNT_PROVIDERS` is at :24 and `account` at about :264. The trigger reader is at :886; reuse it for Sources. `web_url` is at :112. The workflow step keys are here too.
- `crates/am-config/src/fields.rs` -- the `Fields` helpers (`allow`, `text`, `req_text`, `choice`, `id`, `bool`, `int`, `list`, `texts`, `ids`, `unique`, `absent`); reuse them and add none unless needed.
- `crates/am-config/src/refs.rs` -- `RefKind` (:11) and `Known` (:83). Add `Table` and a `TableColumn(table)` check, and declare table stems and their columns. Owner seats and accounts reuse `Seat` and `Account`.
- `crates/am-config/src/lib.rs` -- `FOLDERS` (:69) and `entries()` (:330) read flat files only. Add a `skills/<id>/` walk through `Loader::read`. Seats are declared at :205; sources are stored at :262.
- `crates/am-config/src/entities.rs` -- `Seat` is at :146; add the anatomy fields, `grants: BTreeSet<String>` and `capabilities`. `SourceFile` is at :303; turn it into a typed `Source`, keeping its raw text if useful. Add `Skill`.
- `crates/am-config/src/snapshot.rs` -- add `skills()`.
- `crates/am-domain/src/tool_manifest.rs` -- tool ids. Put the capability table in a new `am-domain/src/capability.rs` as data, so that ToolGate and the UI can read it later.
- Tests: `crates/am-config/tests/validate.rs` has the helpers `valid_copy`, `write`, `edit`, `at` and `errors`. `crates/am-e2e/tests/cli_validate.rs` covers the CLI.
- Fixtures: `fixtures/projects/valid/seats/alpha.md` lists `web_search` and `table_read` without capabilities. It must gain `capabilities` or it turns red. `valid/sources/feed.yaml` is the existing Source.
- `docs/` has no schema doc; create `docs/config-schema.md`. `docs/README.md` points to it.
- `Cargo.toml` has no glob crate. Pin `globset` per decision (d), in the workspace `[workspace.dependencies]` beside `url`.

## Tasks & Acceptance

**Execution:**
- [x] `fixtures/projects/{valid,anatomy,broken-anatomy,sources-ok,sources-broken}/` -- FIRST. Write the matrix inputs and update `valid/` to the new schema. Use single-word stems, synthetic content and no comments (except in broken sets).
- [x] `crates/am-config/tests/{anatomy,skills,sources}.rs`, plus cases in `crates/am-e2e/tests/cli_validate.rs` -- FIRST. Write one test per matrix row, including the `allowed-tools` grant test. Run `cargo nextest run -p am-config -p am-e2e -E 'package(am-config) | binary(cli_validate)'` and paste the red output into Implementation Notes.
- [x] `crates/am-domain/src/capability.rs` -- the capability ids, the expansion table and the provider kind per capability.
- [x] `crates/am-config/src/{kinds,refs,entities,snapshot,lib}.rs` (a new `skills.rs` if cleaner) -- the seat anatomy, the expansion, the skill loader, the Source schema, the Account kinds and the workflow step keys.
- [x] `Cargo.toml`, `crates/am-config/Cargo.toml`, `docs/crate-notes/globset.md` -- add the globset pin and its crate note. `cargo deny` must pass.
- [x] `docs/config-schema.md`, `docs/README.md` -- document seat, skill, Source and Account fields.
- [x] `just check` -- green, with the validate step and the denylist.

**Acceptance Criteria:**
- Given each matrix row, when `agentic validate` runs on its fixture, then the exit code, `file:line`, fix and severity match.
- Given the valid fixture, when it loads, then `snapshot.seats()["alpha"].grants` equals the table expansion, and `skills()` holds the skill with `asks_for`.
- Given the workspace, when `just check` runs, then it is green.

## Implementation Notes

**Red run (2026-11-20, before any implementation).** Fixtures first: `valid/` updated to the new schema (alpha gains `capabilities` web_search + tables and the skill `brief`; beta gains tables; `sources/feed.yaml` gains `terms_checked`; new `skills/brief/` with `allowed-tools: read_website` and one reference file); `anatomy/` (seats plain, keeper, legacy, scout, full, soft; skill brief; accounts finder = tavily, crawly = firecrawl; `work/` and `.agentic/knowledge/full/` with synthetic files); `broken-anatomy/` (seats refused, reader, searcher, messy; skills nodesc, scripted with `scripts/run.sh` and `tool.py`, bulky with a 74,801-byte body); `sources-ok/` (crawler `docs`, http-reader `site`, web-search `search`, table `pages`); `sources-broken/` (crawler `crawl`, feed `news`, crawl Account on `http://10.0.0.5`). `broken/seats/alpha.md` gained `web_search` on so the 1.4 broken set keeps its own findings. Tests: `crates/am-config/tests/{anatomy,skills,sources}.rs` (shared helpers in `tests/common/mod.rs`) and four CLI cases in `cli_validate.rs`. `cargo nextest run -p am-config -p am-e2e -E 'package(am-config) | binary(cli_validate)'` failed to compile the three new test binaries, all missing API:

```
error[E0432]: unresolved import `am_domain::capability`
error[E0432]: unresolved import `am_config::SourceOutput`
error[E0599]: no method named `skills` found for struct `ConfigSnapshot`   (x5)
error[E0599]: no method named `allowed_hosts` found for struct `ConfigSnapshot`   (x3)
error[E0609]: no field `grants` on type `&am_config::Seat`   (x9, plus x4 on `Seat`)
error[E0609]: no field `capabilities` / `links` / `knowledge` / `folders` / `text_fallback` / `goal` / `backstory` / `avatar` / `skills` on `Seat`
error[E0609]: no field `scope` / `limits` / `output` / `trigger` / `render` / `terms_checked` / `source_type` / `owner_seat` / `account` / `name` on `SourceFile`
error[E0609]: no field `kind` on type `Account`
error: could not compile `am-config` (test "anatomy") due to 42 previous errors
error: could not compile `am-config` (test "skills") due to 9 previous errors
error: could not compile `am-config` (test "sources") due to 21 previous errors
```

The binaries that compile (`--test validate --test cli_validate`): 38 run, 23 passed, 15 failed: every test reading the updated valid fixture (its new `capabilities`, `skills`, `terms_checked` keys are unknown keys to the 1.4 loader) and the four new CLI cases (`broken_anatomy_…`, `anatomy_fixture_…`, `sources_ok_…`, `sources_broken_…`).

**Green run (2026-11-20).**
- `am-domain::capability` (new): `Capability` (9), `AccountKind` (model, search, crawl-service), the table as data (`Row`: id, grants, default on, provider kind), `expand(on, read_write_folder)`, `capability_of_tool`, `account_providers`, `provider_needs_base_url`, and the constants `documents`, `builtin`, `read_skill_file`, `write_file`. 5 unit tests.
- `am-config`: the seat reader reads goal (≤ 500 chars), backstory (≤ 2,000 chars), avatar tint, capabilities (defaults filled, legacy `documents`, both keys an error, `on` required in an entry, provider checked by kind), folders (lexical path rules, missing = warning, file = error, kebab unique ids, globs compiled), text_fallback, skills, knowledge, links; grants = expansion + `read_skill_file` with a skill + listed tools of no capability + listed MCP tools; a listed tool whose capability is off, and `write_file` without a `read_write` folder, are errors at the tool's line. New `skills.rs` walks `skills/<id>/` through `Loader::read` (hash, secrets; SKILL.md frontmatter comments). `sources/*.yaml` get a full reader into a typed `Source` (raw text kept). Accounts gain `kind` and the new providers; `firecrawl` and `searxng-remote` need `base_url`. References gained `AccountOfKind`, `Skill`, `Table`, `TableColumn`, `CrawlOwner` and host warnings resolved through `snapshot::seat_allowed_hosts`, the one function behind `ConfigSnapshot::allowed_hosts(seat)` (decision b). `ConfigSnapshot::skills()` added. Workflow steps accept the eight run-graph keys.
- `globset =0.4.21` (default features off) pinned in `[workspace.dependencies]`; `docs/crate-notes/globset.md`. **For the architect (decision d):** the Stack table has no glob crate; this pin adds `globset 0.4.21` (Unlicense OR MIT, used under MIT) and `bstr 1.14.0` (MIT OR Apache-2.0) to the lock file. `cargo deny --locked … check licenses bans sources` passes on the aarch64-apple-darwin and x86_64-unknown-linux-gnu graphs with no exception.
- `docs/config-schema.md` (seat, capability, folder, skill, Account, Source and step fields) and a pointer in `docs/README.md`.
- `cargo nextest run -p am-config -p am-e2e -E 'package(am-config) | binary(cli_validate)'`: 101 passed. `agentic validate fixtures/projects/broken-anatomy`: 11 errors, each with a fix, exit 1. `just check`: fmt, clippy `-D warnings`, nextest (207 passed, 6 skipped), check-deps, check-forbid, denylist (0 derived terms) and validate green.

**Changes outside the listed files (recorded).**
- `.gitignore` re-includes `fixtures/projects/*/.agentic/`: the global `.agentic/` ignore would otherwise drop the anatomy fixture's reference files, and the full-seat row would warn in CI but not locally.
- Two 1.4 tests follow the new schema: `token_count_and_reference_keys_are_not_secrets` now asserts no "secret-looking" finding (its ad-hoc Source's keys are now unknown keys, and "unknown key 'secret_ref'" contains the word "secret"); `every_secret_pattern_is_caught_in_any_file` expects the seat-body secret on beta.md line 21 (beta gained three lines).
- `Step.seat` is now `Option<String>`: a step with `seats:` or `over_folder:` may leave out `seat` (AD-40); every other step still requires it.

**Interpretations (recorded, not decisions).**
- A capability entry must give `on`; its error is at the capability's key line. `provider` defaults to `builtin`.
- Account kinds are now checked on every Account reference: a seat's `model` and `fallback` need a model Account (the reserved local Accounts count as model), `tools.yaml` `search.provider` a search Account.
- Folder paths: besides the plan's list, the project folder itself (`.`, `notes/..`) is refused because it holds `.agentic/`, and `~user/…` is refused. A project-relative path that climbs out with `..` is judged where it lands, against the absolute project path (lexically, no symlinks); so a project stored under `~/Library` (as this repo is) cannot link `../…` folders, while its own subfolders are fine. With `HOME` unset, a `~/` path that climbs above home is refused.
- A linked folder while `folder_access` is off is a warning.
- Skill folders: hidden entries (e.g. `.DS_Store`) are skipped, as the 1.4 loader skips them in every folder; links are errors; `references/` may not hold folders. Comment warnings apply to the SKILL.md frontmatter only (reference files are not rewritten by the app); the secret check applies to every skill file.
- `allowed-tools` is read as a list or as words separated by spaces or commas; `metadata` as text pairs.
- Source scope fields: `seeds`, `url`, `include`, `exclude`, `allowed_hosts`, `max_depth` (AD-39 puts `max_depth` in scope; decision (e)'s cap 0–10 applies there). `rate_per_host` is seconds (number ≥ 1, ≤ 86,400), stored as ms. 1 GB = 10^9 bytes. `account` on a type other than crawler, http-reader, web-search or api is an error. A non-date `terms_checked` is an error on any type; missing or empty warns on website types only.
- Host warnings compare exact host names, ignoring case, as `am-net`'s allow-list does; scope hosts come from `seeds`, `url` and `allowed_hosts`.

**Review fixes (2026-11-20, pass 1; seven patches, same implementer).**
- **`..` in the project path:** `components()` now drops a component on each `..`, so a project loaded through a path with `..` is judged at its real location.
- **Project folder and its ancestors:** `refused_location` now compares the landing path with the normalised project path. It refuses the project folder itself and any ancestor of it, however the path is written (`..`, `../<name>`, `~/…`).
- **`.agentic` in any case:** `.agentic` is matched in any letter case (`is_agentic`), on the written path and on the resolved location.
- **Skill folders:** only `.DS_Store` is skipped; every other hidden entry gets the same checks as any other entry. `.yaml`, `.yml` and `.json` reference files that parse also get the key-name secret check, and yaml files the comment check.
- **`read_skill_file`:** listing it on a seat with no skill attached is an error.
- **`rate_per_host` message:** it now names the 1 to 86,400 second range.
- **Docs:** `docs/config-schema.md` is updated for each of these.

Seven tests were added. Caller re-check:
- `just check` green: 214 passed, 6 skipped.
- Story tests: 108 passed.
- Probe with a freshly built binary, run from a sibling folder (`agentic validate ../proj`): `path: ../proj` is refused as the project folder itself, and `skills/brief/.hidden-dir` is an error.

**For the owner (not decided here).**
- AD-30 refuses `~/Library`, and macOS keeps OneDrive and iCloud folders under `~/Library/CloudStorage`, so no seat can link a cloud-synced folder. A `../` folder beside a project stored there is refused too.
- The 86,400 s upper bound on `rate_per_host` is beyond decision (e), which set only the lower bound; it is kept as a sanity bound.

## Plan Change Log

## Review Triage Log

### Pass 1 (quick lens, 2026-11-20): high 0 · medium 4 · low 3 · false 1 · maybe-false 0

The lens found every acceptance criterion met. All matrix rows have passing tests (101 in am-config and cli_validate; `just check` 207 passed).

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | A project path given with `..` (`agentic validate ../proj`) gives the wrong landing point for `../` folder paths, so a path climbing into `~/Library` can be accepted | medium | patch | `std::path::absolute` keeps `..` on Unix, and `components()` keeps only `Normal` parts, so the dropped `..` shifts the base. Normalise lexically (pop on `ParentDir`). |
| 2 | The project folder, or a folder above it that contains its `.agentic/`, can be linked by another spelling (`../<name>`, `..`, `~/…`) | medium | patch | `refused_location` never compares the landing path with the project path. Only `ups == 0 && parts.is_empty()` refuses the project. The recorded interpretation and `docs/config-schema.md` both state the refusal. |
| 3 | `.agentic` is matched case-sensitively, but `~/Library` is not, and APFS ignores case | medium | patch | `c == ".agentic"` in `folder_location` and `refused_location`. `.Agentic/x` reaches the same runtime folder on a default macOS volume. |
| 4 | Hidden entries in a skill folder are skipped, so a hidden executable file or a `.scripts/` folder gives no finding | medium | patch | `skills.rs` `listing` filters every `.`-name. The plan makes any other file or folder, and any executable bit, an error. Only `.DS_Store` was justified. |
| 5 | Reference files get only the prefix secret scan: no key-name rule, and no comment warning on yaml | medium | patch | `load_references` calls `checks::text_secrets` only. The plan says the secret and comment checks apply to skill files, and the 1.4 key-name rule applies "in any file". |
| 6 | Listing `read_skill_file` in `tools:` grants it without an attached skill | low | patch | `capability_of_tool(SKILL_TOOL)` is `None`, so it falls into the grant arm. The plan adds it only with a skill, while `docs/config-schema.md` says it may be listed, so authors will meet this. |
| 7 | A fan-out step may name no seat (`seats: []`), and `Step.seat` becoming `Option` widens scope | false | reject | Story 1.5's ticket has this story accept fan-out keys unchecked, and its F1 fan-out step declares `seats:` with no `seat:`. A required `seat` would fail that sample. Checking the seats list is stories 3.14–3.16's job, by design. |
| 8 | The `rate_per_host` message says "at least 1" when the value is over the 86,400 upper bound | low | patch | Direct correction of the message text. The 86,400 s upper bound is the implementer's addition beyond decision (e); it is kept as a sanity bound and reported to the owner. |

## Design Notes

- **Allowed-tools test.** AD-38 always grants `read_skill_file` with an attached skill, so "identical with and without a skill" is tested as: the same skill with and without `allowed-tools: [read_website]` gives identical grants, `read_website` is absent, and the only difference from a seat with no skill is `read_skill_file`.
- **Interpretations:**
  - Knowledge entries are `{file, note?}`.
  - Source mapping is `mapping: {<column>: <source path>}`.
  - Website types are http-reader, crawler and feed.
  - No connector type requires an Account in v1 (RULE-20).
  - A skill folder may hold only `SKILL.md` and `references/`.


## Verification

**Commands:**
- `cargo nextest run -p am-config -p am-e2e -E 'package(am-config) | binary(cli_validate)'` -- expected: red first (recorded), then green.
- `cargo run -q -p am-app --bin agentic -- validate fixtures/projects/broken-anatomy; echo $?` -- expected: every matrix error with its fix; exit 1.
- `just check` -- expected: green.
