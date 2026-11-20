---
title: 'Config loader and agentic validate'
type: feature
ticket: '4'
created: '2026-11-20'
status: done
baseline_revision: 'fecd94a8d5ed514421d8411e259f324b5ac933b9'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/epic-foundations-rust-spike.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Nothing reads a project's configuration yet: `am-config` is an empty crate and `agentic` has no `validate` command (epic R1, AD-16). Every later story loads seats, workflows and tables through this loader, and `just check` must run `agentic validate` over `templates/` and `fixtures/`.

**Approach:** `am-config` loads every file kind of the project layout into an immutable `ConfigSnapshot` keyed by a sha256 content hash:
- YAML files through serde-saphyr;
- `seats/*.md` as YAML frontmatter plus a Markdown body;
- `schemas/*.json` as JSON.

It reports problems as `file:line: message` with a fix, and `agentic validate <project>` prints them and exits non-zero on any error. **Tests first (owner rule, 2026-11-20):** fixture projects and failing tests come first and are recorded red.

**Decisions (owner, 2026-11-20):**
- **(a) Field names:** where the architecture gives none, they are snake_case and taken from its prose (`timezone`, `currency`, `week_start`, `teams`, `hats`, `accepts`, `max_chain_depth`). Unknown keys are errors with a "did you mean" fix.
- **(b) Comment warning:** given for every YAML file and every seat's frontmatter. `templates/` and every fixture project except `fixtures/projects/broken/` contain no YAML comments, so `agentic validate` is silent on a fresh project. Guidance goes in docs or in `description:` fields, never in comments.
- **(c) Secret-looking values:**
  - **Prefixes:** `sk-ant-`, `sk-` followed by 20 or more characters, `ghp_`, `github_pat_`, `xox[abprs]-`, `hf_`, `AKIA` plus 16 characters, `AIza` plus 35 characters, and `-----BEGIN … PRIVATE KEY-----`.
  - **Key names:** any non-empty value whose key name matches case-insensitively, either exactly (`api_key`, `apikey`, `token`, `secret`, `password`, `private_key`) or by suffix (`_api_key`, `_apikey`, `_token`, `_secret`, `_password`, `_private_key`). For example `openai_api_key` and `github_token` match.
  - **Never matched:** keys that only contain these words, such as `max_tokens`, `max_output_tokens`, `token_budget` and `secret_ref`.
- **(d) Tool manifests:** the built-in tool table is data in `am-domain`, read by the validator and later by ToolGate. MCP tools come from `tools.yaml`.

- **(e) jsonschema (owner, 2026-11-20, after the build):** jsonschema is not added in this story. `jsonschema =0.61.2` always pulls in `borrow-or-share 0.2.4`, which is MIT-0 and not on the AD-3 allow-list. `schemas/*.json` are checked as JSON plus a project-owned structural check (`am-config/src/schema_check.rs`). The owner prefers adding MIT-0 to the allowed licences; that replan is queued before epic 3 story 5.
- **(f) Secret prefixes (owner, 2026-11-20, after the build):** the prefixes without a set length (`sk-ant-`, `ghp_`, `github_pat_`, `xox?-`, `hf_`) need at least 8 more characters before a value is flagged, so a bare prefix is not flagged. This narrows decision (c) as accepted.

## Boundaries & Constraints

**Always:**
- Only `am-config` reads project files (AD-16).
- Ids are kebab-case and equal the file stem (Conventions).
- References are checked across files.
- Every file carries `schema_version`; the only version known today is `1`.
- Every finding carries file, line (1-based) and a fix.
- Exit codes: 0 when there are no errors (warnings allowed), 1 on any error, 2 for usage or a missing folder.
- Fixtures are synthetic and department-neutral (no sample-company ids; the denylist scan stays green).
- No YAML comments in `templates/` or in any fixture project except `fixtures/projects/broken/` (decision b).
- croner types stay inside `am-config`'s validation module and never reach a public API.

**Never:**
- Seat anatomy (capabilities expansion, folders, text_fallback, skills loader, knowledge, links, goal and backstory caps): story 1.12.
- Validating `sources/*.yaml`: story 1.12. They are loaded and hashed only.
- Templates or the sample company (story 1.5); the writer, History or Undo (1.8); the watcher or Reload (1.9); migrations (1.10); `is_widening`.
- Running any check that needs the network or the keychain.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Valid project | `fixtures/projects/valid/` holding every file kind | Exit 0; the snapshot has every entity; the same files give the same hash | — |
| Unknown seat id | A workflow step names `ghost-seat` | `workflows/x.yaml:L: unknown seat 'ghost-seat'`, with a fix listing the known seats | Exit 1 |
| Bad cron | Schedule trigger `cron: "61 * * * *"` | `file:L: invalid cron …`, with a fix showing the 5-field format | Exit 1 |
| Secret-looking value | `sk-ant-…`, `hf_…`, `api_key: abc…`, `openai_api_key: abc…` or `github_token: abc…` in any file | `file:L: secret-looking value`, with the fix "keep keys in the Keychain; reference an Account" | Exit 1 |
| Missing schema_version | A file without it | `file:1: missing schema_version`, with the fix `schema_version: 1` | Exit 1 |
| Token-count key | `max_tokens: 4096` (and `max_output_tokens: 8192`) in a model or workflow file | Not flagged; the valid fixture includes both | — |
| Unknown tool id | A seat lists `tools: [teleport]` | `seats/x.md:L: unknown tool 'teleport'`, with a fix listing the built-in ids | Exit 1 |
| Commented YAML | A `# note` line in any YAML or frontmatter | A warning at that line ("comments are not kept when the app saves this file") | Exit 0 if there are no errors |
| Many problems | A broken fixture with all of the above | Every finding is printed, sorted by file and line | Exit 1 |

</frozen-after-approval>

## Code Map

- `crates/am-config/` -- empty (2-line lib.rs). Allowed deps: `am-domain` plus external crates (AD-4; `cargo xtask check-deps` enforces this).
- `crates/am-domain/` -- pure; holds ports and `Actor`. A new `tool_manifest.rs` holds the built-in tool table as data: id, action class (`read`, `local-write`, `external-effect`), network needs and `max_uses_per_step`. Ids from AD-11:
  - read: `web_search`, `read_url`, `read_website`, `read_file`, `arxiv`, `read_skill_file`, `search_knowledge`, `table_read`, `list_files`, `workspace_read`, `calculate`, `stats`, `query_table`;
  - local-write: `table_write`, `write_document`, `draft_message`, `propose_change`, `write_file`, `workspace_write`, `remember`, `ask_seat`.
- `crates/am-engine/src/toolgate.rs` -- has its own spike `ToolManifest`. Leave it; the engine adopts the domain table later.
- `crates/am-app/src/main.rs` -- the `agentic` CLI (`serve`, `__spike-run`, `spike …`); add `validate`.
- `docs/crate-notes/croner.md` -- says croner is used "only in am-engine/src/scheduler.rs"; update it to add am-config's syntax check.
- Workspace pins to add (Stack): `serde-saphyr = "=2.1.1"`, `jsonschema = "=0.61.2"` (pre-1.0, so it needs `docs/crate-notes/jsonschema.md`). `sha2 =0.11.1` is already pinned; add `docs/crate-notes/sha2.md`. `schemars` is not needed here.
- `justfile` `check` -- add a `validate` step running `agentic validate` on each project under `fixtures/projects/valid*/`, and on `templates/` once it exists. The broken fixtures are asserted by tests instead.

## Tasks & Acceptance

**Execution:**
- [x] `fixtures/projects/valid/`, `fixtures/projects/broken/` -- FIRST: synthetic projects. The valid one has `company.yaml`, the three `models/*.yaml` files, `tools.yaml`, two seats, a workflow with a schedule trigger, a table, a schema, a dashboard and a source. The broken one has one file per matrix error row plus a commented file -- the test inputs
- [x] `crates/am-config/tests/validate.rs` and `crates/am-e2e/tests/cli_validate.rs` -- FIRST: one test per matrix row (exit code, `file:line`, fix text, warning vs error); run them and record the red output -- tests first
- [x] `crates/am-domain/src/tool_manifest.rs` -- the built-in tool table and a lookup by id -- the manifests the validator reads
- [x] `crates/am-config/src/` -- a loader per file kind with serde-saphyr; a `ConfigSnapshot` (entities by id plus a sha256 over the sorted (path, bytes) pairs); `Finding { file, line, severity, message, fix }`; checks for schema_version, id = stem, cross-file references, cron syntax, secret patterns, unknown tools and comments -- R1
- [x] `crates/am-app/src/main.rs` -- `agentic validate [<project>]`, which prints the findings and exits with the plan's codes -- the CLI
- [x] `Cargo.toml`, `docs/crate-notes/{jsonschema,sha2}.md`, `docs/crate-notes/croner.md`, `justfile` -- pins, notes and the validate step -- Stack and the gate

**Acceptance Criteria:**
- Given the valid fixture, when `agentic validate fixtures/projects/valid` runs, then it exits 0 and prints no errors.
- Given the broken fixture, when it runs, then every matrix error appears as `file:line` with a fix, the comment appears as a warning, and it exits 1.
- Given the workspace, when `just check` runs, then it is green, including the validate step.

## Implementation Notes

**Red run (2026-11-20, before any implementation).** Fixtures first: `fixtures/projects/valid/` (company, the three models files, tools, seats `alpha` and `beta`, workflow `digest` with a schedule trigger, table `items`, schema `summary`, dashboard `overview`, source `feed`; `max_tokens` on a step, `max_output_tokens` in capabilities, `secret_ref` on the account; no comments) and `fixtures/projects/broken/` (one file per matrix error row: `workflows/ghost.yaml`, `workflows/nightly.yaml`, `models/accounts.yaml`, `tables/bare.yaml`, `seats/teleporter.md`, plus the commented `dashboards/overview.yaml`). File stems are single words so the denylist's derived terms stay empty. Then `crates/am-config/tests/validate.rs` (one test per matrix row plus variants built in a temp copy of the valid fixture) and `crates/am-e2e/tests/cli_validate.rs`. `cargo nextest run -p am-config -p am-e2e cli_validate` failed to compile, all missing API:

```
error[E0432]: unresolved imports `am_config::Finding`, `am_config::LoadError`, `am_config::Severity`, `am_config::load`
error[E0433]: cannot find `tool_manifest` in `am_domain`   (x6)
error[E0433]: cannot find `cli` in `am_app`
error: could not compile `am-e2e` (test "cli_validate") due to 1 previous error
error: could not compile `am-config` (test "validate") due to 7 previous errors
```

**Green run (2026-11-20).**
- `am-domain::tool_manifest`: the 21 built-in tools of AD-11 and AD-46 as a data table (id, `ActionClass`, `NetworkNeed`, default `max_uses_per_step`) with `builtin_tools()`, `builtin_tool(id)`, `builtin_tool_ids()`. The defaults are first values (read_file 50 as AD-30's per-step cap); ToolGate's spike manifest is untouched.
- `am-config`: `load(root) -> Result<Loaded, LoadError>`; `Loaded { findings, snapshot }` where the snapshot exists only without errors. YAML is read by serde-saphyr into a line-aware node tree (`Spanned`); comments come from the same parser's event stream (granit-parser, re-exported by serde-saphyr), so a `#` in a quoted value or block scalar is not a comment. Schema JSON is read by serde_json and, for line numbers, by the same YAML reader. Per-kind readers check unknown keys with "did you mean", required fields, types and ranges, kebab-case ids = file stem (an optional `id:` must equal the stem), inner-id uniqueness, cron (exactly 5 fields, then croner, inside `cron.rs` only), IANA zones (jiff), currency codes, and record references that are resolved after every file is read (team, seat, account incl. the reserved `local-lmstudio`/`local-ollama`, schema, workflow, step, tool, `accepts` as `<team>/<workflow>` of that team). Findings are sorted by file, line. `ConfigSnapshot` exposes every entity by id and `hash()`, a sha256 over length-framed (path, bytes) pairs sorted by project-relative path.
- `agentic validate [<project>]` (`am_app::cli::validate`, called by `main`): prints `file:line: message` (warnings as `file:line: warning: message`), an indented `fix:` line each, then `N errors, M warnings`; exit 0, 1 or 2.
- `justfile`: `validate` joins `check` and runs `agentic validate` on each `fixtures/projects/valid*/`; `templates/` joins when story 1.5 creates it.
- `cargo nextest run -p am-config -p am-e2e -p am-domain -E 'package(am-config) | binary(cli_validate) | package(am-domain)'`: 44 passed. `just check`: fmt, clippy `-D warnings`, nextest (140 passed, 6 skipped), check-deps, check-forbid, denylist (0 derived terms) and validate green. `cargo deny … check licenses bans sources` on both graphs: ok.

**Review fixes (2026-11-20).**
- **Empty required values:** a required key written with no value (`provider:`, a column's `type:`, amounts, `transport:`, `base_url`/`url` where required) is now reported the same way as a missing key (`Fields::absent`).
- **No cascades:** references now resolve against ids as they are declared: the ids under `teams:` and `accounts:`, MCP `<server>/<tool>` names from tools.yaml, and the file stems of seats, workflows and schemas (with a workflow's team and step ids when readable). Before this, an id whose entity failed another check counted as unknown; now one bad `timezone` gives exactly one error.
- **Scalars keep their spelling:** YAML is parsed with `strict_booleans`, so `No` stays text. Booleans and numbers read as text keep their source spelling, so `1.10` stays `1.10`.
- **Block scalars:** serde-saphyr already places `|` and `>` content on its first content line. A test pins this (secret on line 23, nothing on the indicator line).
- **URLs:** `web_url` parses with `url =2.5.8`. It allows https, or plain http only to host `127.0.0.1` or `localhost`, and never a user name or password.
- **Loops:** loops in `reports_to` and in team `parent` are errors, reported at the reference that closes the loop.
- **Duplicates:** repeated built-in tool settings, grants and view columns are errors.
- **Broken-fixture test:** it now asserts every matrix row and the warning.
- **Tests:** 47 passed for am-config and cli_validate.

**Interpretations (recorded, not decisions).**
- Field names beyond decision (a): accounts `id, provider, name, description, base_url, secret_ref`; prices `updated, models[provider, model, input_usd_per_mtok, output_usd_per_mtok], services[id, unit, usd_per_unit]`; capabilities `models[provider, model, context_window, max_output_tokens, tools, reads_pdf, reads_images, reads_office, reads_audio]`; tools `egress_hosts, search.provider, builtin[id, max_uses_per_step], mcp_servers[id, description, transport, command, args, env, url, version, network, tools[name, class, description]]`, an MCP tool referenced from a seat as `<server-id>/<tool-name>`; seat `name, role, team, description, reports_to, model{account, model}, fallback, tools, budget{per_run, per_week}`; workflow `name, team, description, triggers[schedule{cron, timezone, catch_up} | event{on, only_if}], caps{per_run, scheduled_per_run, per_week, per_month}, steps[id, seat, instruction, description, output_schema, max_tokens]`; table `name, team, description, key, columns[id (snake_case), type, required, description, values, transitions[from, to]], views[id, name, columns], grants`; dashboard `name, team, description, cards[id, title, query (SELECT/WITH), layout{x, y, w, h}]`. Seat anatomy keys (goal, backstory, capabilities, …) are unknown keys until story 1.12 adds them.
- `company.yaml` is required; every other file is optional. `sources/*.yaml` get only the checks every file gets (parse, schema_version, comments, secrets, `id` = stem), no schema, per Boundaries.
- Secret prefixes that the decision leaves open-ended (`sk-ant-`, `ghp_`, `github_pat_`, `xox[abprs]-`, `hf_`) need at least 8 more key characters and a token boundary before them, so a bare `hf_` or `sk-short` is not flagged; a finding names the key, never the value; seat bodies are scanned for prefixes too.
- An empty file reports `missing schema_version (the file is empty)` at line 1.

**Caller re-check after the review fixes (2026-11-20).** `just check` is green: 148 passed, 6 skipped, and the validate step reports the valid fixture clean. Probes on copies of the valid fixture, using a freshly built binary:
- an empty `provider:` gives `models/accounts.yaml:3: account is missing 'provider'`;
- `timezone: Mars/Base` gives exactly one error, `company.yaml:4`;
- `name: No` validates and is kept as "No";
- `base_url: http://127.0.0.1:80@remote.example/` is rejected at `models/accounts.yaml:10`.

## Plan Change Log

- 2026-11-20 — **jsonschema not added (licence conflict).** `jsonschema =0.61.2` (Stack, Code Map) depends, with or without default features, on `referencing` → `fluent-uri` → `borrow-or-share 0.2.4`, licensed MIT-0, which is outside the AD-3 allow-list; `cargo deny check licenses` failed and `deny.toml` forbids adding a licence or an exception. Instead `am-config/src/schema_check.rs` checks schema files structurally (type names, `required`, schema-valued keywords, local `$ref` only), not against the meta-schema. `docs/crate-notes/jsonschema.md` records the finding. **Open for REPLAN (architect/owner):** the engine's step-output validation (AD-36) needs a validator: allow MIT-0 in AD-3, choose another crate, or keep a project-owned subset validator. The architecture's Stack row is unchanged here.

## Review Triage Log

### Pass 1 (quick lens, 2026-11-20): high 2 · medium 5 · low 1 · false 1 · maybe-false 0

The lens found every acceptance criterion and AGENTS.md rule met.

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | A required field present but empty (`provider:`, price amounts, MCP `transport`, column `type`) drops the entry with no finding | high | patch | Probe: an empty `provider:` on account `cloud` gives no finding at the account, only "seats/beta.md:11: unknown account 'cloud'". `choice()` and `micros()` return `None` on Null without a finding. |
| 2 | Cross-file references check only entities that parsed completely, so one local error cascades into wrong "unknown …" errors in other files | medium | patch | Probe: `timezone: Mars/Base` gives 9 errors, among them "tables/items.yaml:3: unknown team 'core'", which point at the wrong file. |
| 3 | YAML 1.1 booleans (`yes`, `no`, `on`, `off`) are parsed as bools and turned into "true" or "false" | medium | patch | Probe: `name: No` validates as "false" with 0 errors. `node.rs` uses serde-saphyr's default options (`strict_booleans: false`), and `fields.rs:103` stringifies the bool. |
| 4 | Text fields lose a float's written form (`1.10` → "1.1", `1.0` → "1") | medium | patch | `fields.rs:102` `Kind::Float(f) => f.to_string()`, against its own doc comment. |
| 5 | A secret inside a `\|` or `>` block scalar is reported one line early | false | reject | Checked by the implementer before any change: serde-saphyr 2.1.1 places a block scalar's value on its first content line, not on the indicator line. The test `a_secret_in_a_block_scalar_is_reported_at_its_own_line` pins this; a comment was added, and no code changed. |
| 6 | The loopback URL check passes `http://127.0.0.1:80@remote.example/` (userinfo bypass) | medium | patch | `kinds.rs:114` uses `starts_with` on the raw string, so the real host is never parsed. AD-12 enforces at runtime too, but the validator must not approve a remote plain-http host. |
| 7 | Cycles in `reports_to` and team `parent` are accepted | medium | patch | `refs.rs` checks existence only. The org tree must be a tree (Structural Seed). |
| 8 | Duplicates are unchecked in `tools.yaml` `builtin`, table `grants` and view `columns` | low | patch | Every other list goes through `unique()`; one call each. |
| 9 | The many-problems test asserts only `errors >= 5` and sort order | low | patch | Real; `cli_validate.rs` covers the matrix, but the unit test should name each expected error. |

## Verification

**Commands:**
- `cargo nextest run -p am-config -p am-e2e cli_validate` -- expected: red first (recorded), then green. Note: nextest's positional filter matches test names, so this runs no tests once it compiles; the equivalent is `cargo nextest run -p am-config -p am-e2e -E 'package(am-config) | binary(cli_validate)'`
- `cargo run -q -p am-app --bin agentic -- validate fixtures/projects/valid; echo $?` -- expected: no errors, `0`
- `cargo run -q -p am-app --bin agentic -- validate fixtures/projects/broken; echo $?` -- expected: every error as `file:line` with a fix, `1`
- `just check` -- expected: green, including validate and the denylist
