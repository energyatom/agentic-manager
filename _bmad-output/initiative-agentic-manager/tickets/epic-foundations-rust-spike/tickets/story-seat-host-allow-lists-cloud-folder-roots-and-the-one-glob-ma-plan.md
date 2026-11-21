---
title: 'Seat host allow-lists, cloud folder roots and the one glob matcher'
type: feature
ticket: 14
created: '2026-11-22'
status: done
baseline_revision: 'cc1c89bfed30e72b38a66924b5f68fc3d721e970'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: [multiple-goals, oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** Every seat's allow-list is still `tools.yaml` `egress_hosts` (story 12's interim rule), hosts are matched by hand-written exact compares, the folder-root policy refuses all of `~/Library` (so no cloud-synced folder can be linked) and does not apply AD-30's in-project rule (amended 2026-11-22), and globs are compiled ad hoc in am-config. AD-48, AD-30, AD-4 and the Globs convention require one host type, per-seat lists, the cloud roots, and one glob crate.

**Approach:**
- **Hosts:** `am_domain::HostPattern` is the only host parser and matcher. Seat files gain `allowed_hosts`, which becomes the snapshot allow-list and drives the link-host and Source scope-host checks.
- **Migration:** a whole-project config migration from schema_version 1 to 2 (through story 10's registry, now able to see every file) moves `egress_hosts` into the fetching seats.
- **Folders:** the folder-root policy follows the amended AD-30, backed by an `am-domain` config-name policy.
- **Globs:** a new leaf crate, `am-glob`, is the one glob matcher, and am-config validates through it.

## Boundaries & Constraints

**Always:**
- **HostPattern (AD-48):**
  - An exact pattern is an ASCII host name (labels of 1–63 letters, digits or hyphens, no leading or trailing hyphen, 253 characters at most, at least one dot).
  - A wildcard pattern is `*.` plus a domain of at least two labels. It matches any subdomain depth, but not the bare domain.
  - A pattern admits `https:443` and `http:80` only.
  - Refused: a scheme, port, path, IP literal (v4, or anything bracketed or containing `:`), `localhost`, non-ASCII text, and any other `*`.
  - Uppercase ASCII is lowercased (allowed by the ticket's "refused or normalised"; DNS is case-insensitive).
  - `covers(&other)`: an exact pattern covers only itself; `*.d` covers every pattern under `d`, but not `d`.
  - No dependency other than the existing am-domain externals.
- **Seat `allowed_hosts`:** each entry is validated with file, line and fix, and the key sits in `SEAT_KEYS` after `links` in canonical order. `ConfigSnapshot::allowed_hosts(seat)` returns exactly the seat's patterns. Account endpoints and model hosts are never added.
- **Host warnings:** a link host warns when the seat's patterns do not match it. A Source scope host (from `seeds`, `url` or scope `allowed_hosts`) warns when the owner seat's patterns do not cover it. The fix text names the seat file's `allowed_hosts`, never `egress_hosts`.
- **Migration 1→2:**
  - `SCHEMA_VERSION` becomes 2. The 1→2 step sees the whole project and bumps every file.
  - It copies valid `egress_hosts` entries into `allowed_hosts` of each seat that can fetch, and of no other seat. A seat can fetch when `web_search`, `browse` or `crawl` is on, or when it is the `owner_seat` of any Source.
  - It merges with any patterns the seat already has, without duplicates. It drops each entry `HostPattern` refuses and lists it on the offer.
  - It removes `egress_hosts` from `tools.yaml`.
  - Offers stay per file, all or nothing. `check_offer` re-derives the whole project from the current bytes.
  - A v2 `tools.yaml` that still holds `egress_hosts` is an unknown-key error whose fix names the seats' `allowed_hosts`.
- **Folder roots (AD-30 as amended 2026-11-20 and 2026-11-22):**
  - **Allowed:** folders at or below `~/Library/CloudStorage/<provider>/`, and below `~/Library/Mobile Documents/com~apple~CloudDocs/`.
  - **Refused:**
    - `~/Library`, `~/Library/CloudStorage` itself, and every other `Mobile Documents` container;
    - the runtime base (`~/Library/Application Support/agentic-manager-projects`) and the app data dir;
    - `/`, `~`, and the open project's root and every folder above it;
    - a folder that is, contains or lies inside a recent project's root (passed in by the caller).
  - **Inside the open project:** a subfolder may be linked `access: read` only when its path is not config-bearing and no entry in its subtree bears a config name.
    - The subtree walk reads names only, through `lstat`, and never follows links. It refuses in words when it cannot finish: denied, or more than 100,000 entries.
    - The config names are one `am-domain` policy (`CONFIG_NAMES`, `is_config_bearing`), compared ignoring case: `models`, `seats`, `skills`, `workflows`, `tables`, `schemas`, `dashboards`, `sources`, `company.yaml`, `tools.yaml`, `.agentic`, `runtime-seed`, `.agentic-open`, `.git`.
    - A test asserts the names equal the folders and files the loader reads.
  - `read_write` is refused for any folder inside any project root. Here a project root is also any folder holding `company.yaml`, found by walking up from the path.
  - Every refusal is reported at the seat's `folders` line, with the fix.
- **am-glob:** a leaf crate (`globset`, `unicode-normalization =0.1.26`), `#![forbid(unsafe_code)]`, MIT.
  - One `Glob` type: `literal_separator(true)`, pattern and subject NFC-normalised, case-insensitive or case-sensitive by constructor, plus a `regex()` form.
  - am-config compiles linked-folder `include`/`exclude` (case-insensitive) and Source URL-path scopes (case-sensitive) only through it, and no other crate uses globset.
  - `cargo xtask check-deps`:
    - am-glob may depend on no internal crate, and on no external crate other than its two;
    - am-config, am-engine, am-tools and am-connectors may depend on am-glob, and no other crate may;
    - an am-glob → am-domain edge is refused.
- **Tests first:** the red run is logged in Implementation Notes before any `src/` change. Shipped templates and fixtures carry `schema_version: 2`, `allowed_hosts` where they fetch, and no `egress_hosts`. Role templates carry no `allowed_hosts`, and `agentic validate` passes over `templates/` and `fixtures/`.

**Never:**
- No change to `am-net` (its `EgressPolicy` moves to `HostPattern` in the egress story), `am-tools` folder tools, `is_widening` (epic 4), or the UI.
- No `idna` or `url` in am-domain, and no punycode conversion: non-ASCII is refused.
- No hand-written host or suffix match, and no globset outside am-glob.
- No edit to planning documents, `reference-workload.md` or the sample's `files/d1-inputs/`.
- No hardlink-at-access check (the folder tools' job, epic 4/12).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pattern table | `docs.example.org`; `*.example.org`; `*.com`, `10.1.2.3`, `localhost`, `example.org:8080`, `https://x.org/`, `exämple.org`, `a.*.org`; `Example.ORG` | Exact matches itself only. The wildcard matches `a.b.example.org`, not `example.org`. `*.example.org` covers `docs.example.org`, not the reverse. `Example.ORG` becomes `example.org` | Each refused form returns an error naming why |
| Seat hosts | Seat `allowed_hosts: [docs.example.org]`; another with `https://x.org/` | First validates; second fails at its line | Fix: "write the host only, e.g. x.org" |
| Host warnings | Link `https://other.net/`; Source scope host `feeds.x.org`, owner seat lacks a covering pattern | Two warnings naming seat and host | Fix names the seat file's `allowed_hosts` |
| No Account host | Seat whose model or fallback is an anthropic Account | `allowed_hosts(seat)` lacks `api.anthropic.com` | — |
| Migration | v1 project: `egress_hosts: [docs.example.org, 10.1.2.3]`; one web_search seat, one Source owner, one non-fetching seat | Offer per file; disk unchanged; offer notes list `10.1.2.3` dropped. After Human accept: the two fetching seats list `docs.example.org`, the other none; `tools.yaml` has no `egress_hosts`; every file at 2; load clean | Session open returns `MigrationPending` |
| v2 leftover | v2 `tools.yaml` with `egress_hosts` | Error at its line | Fix names seats' `allowed_hosts` |
| Cloud roots | `~/Library/CloudStorage/OneDrive-Fixture/x`, `~/Library/Mobile Documents/com~apple~CloudDocs/x` | Validate (no error) | — |
| Refused roots | `~/Library`, `~/Library/CloudStorage`, `~/Library/Mobile Documents/com~apple~Pages`, project root, `..` | Error at the folder line | Fix per case |
| In-project | Read-only `files/in` (no config names) | Validates | — |
| In-project refused | `files/in` read_write; `seats`; `runtime-seed`; `files/in` after a seat file is copied into it | Error at the folder line naming the reason or entry | Fix: rename or move the entry, link a narrower or outside folder, or use `access: read` |
| Globs | `*.PDF` vs `Report.pdf`; NFD name vs NFC pattern; `a/*` vs `a/b/c` | Folder include matches case-insensitively; URL scope does not; NFD matches; `*` stays in one segment | Bad glob: error at its line |
| Deps | Workspace graph; fixture with am-glob → am-domain | Passes; fixture refused naming the rule | — |
| Shipped | `templates/`, `fixtures/` | `agentic validate` 0 errors; sample `files/d1-inputs` still linked read-only | — |

</intent-contract>

## Code Map

- `crates/am-domain/src/` — add `host.rs` (`HostPattern`, `HostPatternError`) and `config_names.rs` (`CONFIG_NAMES`, `is_config_bearing`), re-exported in `lib.rs` (mods at :9-21, re-exports at :38-59). Externals stay serde, thiserror, jiff, uuid (`xtask/src/deps.rs:13`).
- `crates/am-glob/` (new) — `Cargo.toml` (`[lints] workspace = true`), `src/lib.rs`. Register it in workspace `members` and `default-members` (`Cargo.toml:3-47`) and as a `[workspace.dependencies]` path entry (:67-83), and pin `unicode-normalization = "=0.1.26"` beside `globset` (:120). Update `docs/crate-notes/globset.md` and add `docs/crate-notes/unicode-normalization.md`.
- `xtask/src/deps.rs` — `Layer` (:27), `layer()` (:42-54), `allowed()` (:57-88): add a glob layer and its allowed users, and update the rule texts. Tests in `xtask/tests/check_deps.rs` with metadata fixtures under `xtask/tests/fixtures/metadata/`; add `glob_domain.json`.
- `crates/am-config/src/kinds.rs`:
  - `SEAT_KEYS` (:92-112): add `allowed_hosts`. `TOOLS_KEYS` (:74): drop `egress_hosts`, and give its unknown-key fix.
  - `tools()` (:667): remove the hand-written host check at 672-689.
  - `seat_links` (:1425) and `http_host` (:1414); `source_scope` (:2143), `scope_url` (:2106), `scope_host` (:2119); the scope `allowed_hosts` check (2179-2196) moves to `HostPattern`.
  - `fn glob` (:998) and `globs()` (:1016) move to am-glob.
  - Folder policy: `seat_folders` (:1295), `folder_location` (:1112), `refused_location` (:1077), `FOLDER_FIX` (:969), `Places` (:977).
- `crates/am-config/src/refs.rs` — `Known.egress_hosts` (:152), `Refs::host` (:101), `HostCheck` (:45), and `resolve` (:173-186): switch to per-seat patterns and `covers`/`matches`.
- `crates/am-config/src/snapshot.rs` — `allowed_hosts` (:77) and `seat_allowed_hosts` (:84) now read the seat's patterns. `entities.rs`: `Seat` (:146) gains `allowed_hosts: Vec<HostPattern>`; `ToolsConfig.egress_hosts` (:116) is removed.
- `crates/am-config/src/lib.rs`:
  - `LoadOptions` (:121) gains `home: Option<PathBuf>` (tests inject; the default reads `HOME`; replaces the `env::var_os` calls at :202 and :782) and `recent_roots: Vec<PathBuf>`.
  - The loader's `Known` fill (:597); the `SCHEMA_VERSION` users (:744-748).
- `crates/am-config/src/checks.rs:8` — `SCHEMA_VERSION` = 2.
- `crates/am-config/src/migrate.rs`:
  - `Step` (:43-47) becomes whole-project: `change: fn(&mut ProjectTrees) -> Result<Vec<String>, String>`, with notes per file. The test-only 0→1 `group_to_team` is kept as a project step.
  - Shipped `STEPS` = `[1→2]`. With `test-support`, `[0→1, 1→2]`.
  - `assess` (176), `offer_for` (232), `offers_in` (346) and `check_offer` (393) run on the whole file set.
  - `MigrationOffer` (107-121) gains `notes: Vec<String>`.
- `crates/am-config/src/writer.rs:491-600` (`migrate`) — it already re-checks each file. Keep that, and re-derive through the project form.
- **Templates and fixtures:**
  - All 86 files go from `schema_version: 1` to 2.
  - `tools.yaml` files lose `egress_hosts`. Their values move to these seats:
    - valid: alpha;
    - anatomy: full;
    - sources-ok and sources-broken: owner;
    - broken-anatomy: searcher.
  - Company A: `egress_hosts: []`, so no seat gains hosts.
  - `fixtures/projects/anatomy/seats/keeper.md:8`: `work` stays, read-only.
  - `broken-anatomy/seats/refused.md:8`: `~/Library/x` stays refused.
- **Tests to update (version 1 → 2 as "current"):**
  - `crates/am-config/tests/migrate.rs` (diffs at :152, 224, 271 and 361-364);
  - `crates/am-e2e/tests/config_migrate.rs:59,140`;
  - `tests/validate.rs:270`, where 2 meant unknown: use 3;
  - `tests/identity.rs:224`: use 3;
  - `tests/anatomy.rs:477`: becomes per-seat;
  - `anatomy.rs:110-160` and `507-570` (folder forms; the `keeper` read_write case now fails).
- `docs/config-schema.md:156-172` — the allow-list and folder rules.

## Tasks & Acceptance

**Execution:**
- [x] Tests first, outside `src/`:
  - `crates/am-domain/tests/host_pattern.rs` and `config_names.rs`;
  - `crates/am-glob/Cargo.toml` and `crates/am-glob/tests/glob.rs` only, with no file under `src/` (am-glob's red run is the build error naming the missing library, run with `-p am-glob`; the other crates' red run excludes it if the workspace cannot load);
  - `crates/am-config/tests/{hosts,folder_roots,migrate_hosts}.rs` and the updated existing tests;
  - `xtask/tests/check_deps.rs` cases and fixture;
  - a fixture `fixtures/projects/hosts-v1/` at schema_version 1 for the migration row.

  Run them and log the red run.
- [x] `crates/am-domain/src/{host,config_names,lib}.rs` — the pure policies.
- [x] `crates/am-glob/` and the workspace registration — the matcher.
- [x] `xtask/src/deps.rs` — the matrix.
- [x] `crates/am-config/src/{kinds,refs,snapshot,entities,lib,checks,migrate,writer}.rs` — allow-lists, host checks, folder policy, migration, globs.
- [x] `templates/`, `fixtures/` — version 2, `allowed_hosts`, no `egress_hosts`.
- [x] `docs/config-schema.md`, crate notes — documentation.

**Acceptance Criteria:**
- Given each matrix row, when its test or `agentic validate` runs, then the result, `file:line`, fix and severity match.
- Given the workspace, when `just check` runs (fmt, clippy, nextest, check-deps, check-forbid, denylist, validate, supply chain), then it is green.

## Implementation Notes

### Red run (2026-11-22, before any `src/` change)

Tests written first, no file under any `src/` folder created or changed. To let `-p am-glob` resolve, the root `Cargo.toml` registers `crates/am-glob` in `members`, `default-members` and `[workspace.dependencies]` and pins `unicode-normalization = "=0.1.26"` (manifest only; `Cargo.lock` gains the am-glob entry); the workspace still loads.

Files:
- New: `crates/am-domain/tests/{host_pattern,config_names}.rs`; `crates/am-glob/Cargo.toml`, `crates/am-glob/tests/glob.rs`; `crates/am-config/tests/{hosts,folder_roots,migrate_hosts}.rs`; `fixtures/projects/hosts-v1/` (schema_version 1: `company.yaml`, `tools.yaml` with `egress_hosts: [docs.example.org, 10.1.2.3]`, seats `searcher` (web_search), `owner` (owns `sources/notes.yaml`), `quiet`); `xtask/tests/fixtures/metadata/glob_{ok,domain,external,user}.json`.
- Rewritten: `crates/am-config/tests/migrate.rs` (current version 2, offers 0→2, newer is 3 with `known: 2`, shipped files at 2 except `hosts-v1`), `anatomy.rs` (keeper written by the test; the fixture's `work` read-only; read_write in the project refused; read_write outside through an injected home; per-seat allow-list; new refused forms; `~/Library` fix names both cloud roots), `validate.rs` (missing-version fix says 2, unknown is 3, version-agnostic `id:` insert), `identity.rs` (newer is 3, `known: 2`; version-agnostic insert), `session.rs` (copied workflow diff at 2; `LoadOptions { .., ..Default::default() }`; version-agnostic `project_id` insert), `common/mod.rs` (`COMPANY_V2`, `project_with`, `hosts_of`); `crates/am-e2e/tests/config_migrate.rs` (0→2), `cli_validate.rs` and `crates/am-app/tests/serve_project.rs` (version-agnostic `project_id` insert; both still pass); `xtask/tests/check_deps.rs` (five cases).

Commands and failing output:

1. `cargo nextest run -p am-glob` → build error naming the missing library:
   `error[E0432]: unresolved import am_glob --> crates/am-glob/tests/glob.rs:17:5`
2. `cargo nextest run -p am-domain` →
   `error[E0432]: unresolved imports am_domain::CONFIG_NAMES, am_domain::is_config_bearing --> crates/am-domain/tests/config_names.rs:15:17`;
   `error[E0432]: unresolved imports am_domain::HostPattern, am_domain::HostPatternError --> crates/am-domain/tests/host_pattern.rs:18:17`
3. `cargo nextest run -p am-config --test <t>` for the new and API-changed files →
   - anatomy: `error[E0560]: struct LoadOptions has no field named home --> anatomy.rs:34:9`
   - hosts: `error[E0609]: no field allowed_hosts on type Seat --> hosts.rs:108:10, 124:34`
   - folder_roots: `error[E0425]: cannot find value CONFIG_NAMES in crate am_domain --> folder_roots.rs:425:46`; `error[E0560]: struct LoadOptions has no field named home / recent_roots --> folder_roots.rs:92:13, 93:13`
   - migrate_hosts: `error[E0609]: no field notes on type &MigrationOffer --> migrate_hosts.rs:152:15, 154:15, 322:39, 354:19, 354:59`
4. `cargo nextest run -p am-config --test migrate --test validate --test identity --test session --no-fail-fast` → `81 tests run: 70 passed, 11 failed`:
   identity `the_resolver_reads_company_yaml_in_words` (`TooNew { found: 3, known: 1 }`); migrate `migrate_accept_refuses_an_offer_whose_diff_was_altered`, `migrate_accept_renames_a_v0_group_to_team`, `migrate_keeps_a_seat_body_byte_for_byte`, `migrate_accept_writes_every_file_with_one_migration_entry_each`, `migrate_shipped_templates_and_fixtures_offer_nothing` (`templates/roles/reviewer/company.yaml: schema_version: 1`, left "1" right "2"), `migrate_offer_lists_every_older_file_with_its_diff_and_writes_nothing` (left `(0, 1)` right `(0, 2)`), `migrate_session_open_refuses_with_migration_pending`, `migrate_newer_file_is_not_offered_and_still_refused` (`Err(TooNew { found: 3, known: 1 })`); session `session_created_and_deleted_files_raise_an_event_each_and_reload_applies_both` (diff at version 1); validate `missing_schema_version_is_reported_on_line_one` (fix `add schema_version: 1 as the first line`).
5. `cargo nextest run -p am-e2e --test config_migrate --test cli_validate --no-fail-fast` → `17 tests run: 16 passed, 1 failed`: `config_migrate_database_and_config_one_version_back_on_a_real_store` (config_migrate.rs:143, left `(0, 1)` right `(0, 2)`).
6. `cargo nextest run -p xtask --test check_deps --no-fail-fast` → `10 tests run: 4 passed, 6 failed`: `only_am_glob_depends_on_globset` (left `["am-config", "am-glob"]`), `an_am_glob_to_am_domain_edge_fails_naming_the_rule`, `an_am_glob_external_other_than_its_two_fails`, `a_crate_outside_the_four_users_may_not_depend_on_am_glob`, `am_glob_and_its_four_users_pass` (each: `am-glob: not in the AD-4 matrix; classify it in xtask/src/deps.rs before it lands` / `am-config -> am-glob: am-config may depend only on am-domain (AD-4)`), `current_workspace_passes` (same am-glob finding).

Test-side choices the implementation must meet (recorded under the standing rules): `HostPattern::{parse, as_str, matches, admits(scheme, host, port), covers}` with `Display`; `HostPatternError` words name the cause (`label`, `IP`, `localhost`, `port`, `scheme`, `path`, `ASCII`, `wildcard`); `CONFIG_NAMES` and `is_config_bearing(&str)` re-exported at the crate root; `am_glob::{Glob, GlobError}` with `case_insensitive`, `case_sensitive`, `is_match`, `regex()`; every folder refusal is an error on the folder's `path` line whose message says "not allowed" (subtree entry: names it, fix says move; config-bearing path: names it, fix says outside; read_write in a project: fix says `access: read`; a `~/Library` refusal's fix names `~/Library/CloudStorage` and `com~apple~CloudDocs`; a denied walk says "cannot" and names the folder; the cap says "100,000"); a seat host error's fix for `https://x.org/` is "write the host only, e.g. x.org"; host warnings' fix names `seats/<id>.md` and `allowed_hosts`; the v2 `egress_hosts` error is at its key line; the 1→2 drop list is in the `tools.yaml` offer's `notes`; the new check-deps rule text says "am-glob may depend on no workspace crate" and names `globset` and `unicode-normalization`. The 100,001-file walk test creates its files in a temporary folder (a few seconds).

### Implementation (2026-11-22)

Engineering choices (standing rules):
- `migrate::Step::change` is `fn(&mut ProjectTrees, i64)`: the step changes only files that are stepping at its version (`ProjectFile::steps_at`) and may read every file; a refusal is per file (`ProjectFile::refused`, so one bad seat still blocks only itself) and notes are per file (`ProjectFile::notes` → `MigrationOffer::notes`). `derive` runs the steps over the whole file map; `offers_in`, `blocked`, `no_path`, `left_behind`, `unmigratable`, the loader's `word_blocked` and the writer's `check_offers` all read it. The writer checks every offer's sha256 before re-deriving, so a changed file is `ChangedSince` before any offer mismatch.
- The 1→2 step reads Source owners and seat capabilities from every file whatever its version; a fetching seat that is not stepped (current, linked or with a secret) does not get the hosts, and `tools.yaml`'s notes say so. With no fetching seat, the notes say the hosts reached no seat.
- `HostPattern` also refuses a name whose last label is digits (an IPv4 address or part of one) or `localhost` (`x.localhost`). Duplicate patterns in a seat are an error ("appears twice"). A link or seed is judged with `admits` (scheme and port included); a scope pattern with `covers`. A host check for a Source whose owner seat is unknown is skipped (the unknown seat is already an error).
- Folder policy: a path is judged lexically and, when it exists, canonicalised (home, project and recent roots canonicalised too). `~/Library/Mobile Documents/com~apple~CloudDocs` itself is refused ("below" in AD-30); `~/Library/CloudStorage/<provider>` itself is allowed ("at or below"). `~/Library` component names compare ignoring ASCII case. `read_write` looks for `company.yaml` in the folder and every folder above it (on disk; for a folder not yet created, above where it would be).
- `am-glob` uses no `thiserror` (its only externals are globset and unicode-normalization): `GlobError` implements `Display` and `Error` by hand.
- `check-deps`: composition roots are also refused an `am-glob` edge ("no other crate may"); exempt crates (am-testkit, am-e2e, xtask) stay exempt.

Test-side edits after the red run (behaviour-preserving; listed for review):
- Repo gates refused literals in the red-run tests: `crates/am-domain/tests/config_names.rs` now spells `.agentic` and `.agentic-open` through `am_domain::runtime::{LEGACY_DIR, OPEN_MARKER}` (runtime-paths, AD-32) and uses `files/x1-inputs` for the sample's folder name (denylist, AD-23); `crates/am-config/tests/folder_roots.rs` builds its two runtime-name entries from the same constants; `crates/am-config/tests/hosts.rs` finds the one sample under `templates/samples/` instead of naming it (denylist).
- Existing tests broken only by the plan's own changes: `tests/identity.rs` `COMMENTED` and `tests/writer.rs` `SHUFFLED` write `schema_version: 2` (they assert a clean load / a canonical edit at the current version); `tests/validate.rs` `repeated_grants_tool_settings_and_view_columns_are_errors` expects the repeated tool setting on line 7 (the valid fixture's `tools.yaml` lost its two `egress_hosts` lines).

The `hosts.rs` "first seat validates" contradiction reported on 2026-11-22 was settled by the coordinator (the assertion now checks errors only).

Review fixes (2026-11-22):
- `check-deps` refuses a `globset` edge from any crate but am-glob (exempt test scaffolding aside), naming the rule (`globset_user.json` case); the globset crate note says so.
- `HostPattern` refuses a last label the WHATWG URL parser reads as a number (`127.0x1`, `1.2.3.0x4`) as an IP address; `host_hint` names a host only when it is itself a valid pattern, else the generic fix stands.
- The 1→2 step notes a fetching seat whose `allowed_hosts` is not a list (seat, hosts, why) on its own offer and on `tools.yaml`'s.
- `MigrationOffer` gains `inputs` (sha256 of every config file the derivation read); the accept refuses `ChangedSince` naming any input that changed, was removed or was added since the offer, and checks the whole file set once more right before the first write.
- The retired `egress_hosts` error fires only on a `tools.yaml` at schema_version 2 or later.
- `agentic validate` (not `--template`) passes every recorded project root but its own as `recent_roots` (`am-app` `cli.rs`; test `crates/am-app/tests/validate_recent.rs`).

## Plan Change Log

## Review Triage Log

### 2026-11-22 — Review pass
- verdicts: 9 findings — high 0, medium 4, low 5, false 0, maybe-false 0
- findings:
  - `[medium]` `[patch]` "No globset outside am-glob" is enforced only by a test that reads `[dependencies]` of `crates/*/Cargo.toml`. check-deps lets any non-domain crate add globset (`_ => true`), and `docs/crate-notes/globset.md` claims check-deps enforces it. Fix: check-deps refuses a `globset` edge from any crate other than am-glob, and the doc names what enforces it.
  - `[medium]` `[patch]` `HostPattern::parse` accepts WHATWG-numeric IPv4 forms (`127.0x1`, `1.2.3.0x4`), because only a decimal last label is refused. The `url` crate reads these as IPv4, so the validator and the 1→2 migration accept an IP literal against AD-48. Fix: a last label that is a number in the WHATWG sense (decimal, or `0x` hex) is an IP address; test.
  - `[low]` `[patch]` The scheme, path and port hint can name a host that is itself refused (`https://exämple.org/`, `https://user@x.org/`). Fix: give the hint only when it parses as a `HostPattern` (direct correction).
  - `[low]` `[patch]` `merge_hosts` gives a fetching seat whose existing `allowed_hosts` is not a list nothing, with no note, while the step still removes `egress_hosts`, so hosts are lost silently against the step's own rule. Fix: name the seat and the hosts not given in the offer notes; test.
  - `[medium]` `[patch]` The stepped bytes depend on files that are not offered (other seats' capabilities, every Source's `owner_seat`), but only offered files are re-checked before the writes. Fix: re-check the sha256 of every file the derivation read, right before the first write, refusing `ChangedSince` naming the file.
  - `[low]` `[patch]` The retired `egress_hosts` error fires on a v1 `tools.yaml` waiting for its migration (the plan scopes it to v2), adding a confusing second error beside the offer. Fix: report it only when the file's schema_version is 2 or more (direct correction); test.
  - `[low]` `[reject]` Refusals land on the folder entry's `path:` line, while AD-30 and the plan say the seat's `folders` line. The entry's own line is more precise, and the ticket's verify (file:line and fix) is met; moving it up would make the location less useful. No user harm.
  - `[medium]` `[patch]` `LoadOptions.recent_roots` is never filled in production, so the ticket's "refuse a folder that is, contains or lies inside another recent project's root" never applies (the Design Notes' deferral narrowed the ticket). Fix: `agentic validate` (am-app `cli.rs`, the only production loader) passes the root paths of every runtime-folder record (`runtime::records`) other than the project being validated; test.
  - `[low]` `[reject]` Exempt crates (am-testkit, am-e2e, xtask) may depend on am-glob unchecked. They are exempt from every AD-4 edge rule by design (test and tool crates reach any crate), no shipped code is affected, and the fix adds a special case.

### 2026-11-22 — Follow-up review pass (the seven patches above)
- verdicts: 4 findings — high 0, medium 0, low 4, false 0, maybe-false 0
- findings:
  - `[low]` `[patch]` check-deps skips exempt crates and dev edges, and the backup test reads only `[dependencies]` of `crates/*`, so a `globset` dev or build dependency, or one in xtask or an exempt crate, passes. Fix: the guard test scans every dependency table of every workspace member; its sibling test's doc comment claims only what it asserts.
  - `[low]` `[patch]` `validate_recent.rs` stays green if cli.rs stops excluding the project's own record. Fix applied: `the_validated_project_is_not_its_own_recent_root` (the project recorded, validated through a symlink, links `files/in` read-only, exits 0). The implementer showed that am-config's `refused_location` already skips a recent root equal to the open project, so the cli.rs exclusion is a redundant second guard that no validate run can see. The behaviour that matters (a project never refuses its own folders) is what the test proves.
  - `[low]` `[patch]` A fetching seat whose `allowed_hosts` is not a list is migrated anyway, while `tools.yaml` loses `egress_hosts`, so its hosts survive only in an unkept note. Fix: block that seat as a step refusal with the reason, so the all-or-nothing accept waits for the owner's hand fix. This replaces the previous pass's note-only patch.
  - `[low]` `[reject]` The re-scan right before the first write (writer.rs ~578) has no test: nothing calls out between validation and the first write, so no deterministic seam exists (same as 1.10's re-check). The inputs check before it has its own test.

## Design Notes

Decided under `docs/owner-standing-rules.md` (engineering choices, recorded):
- **Whole-project step:** moving hosts across files cannot be expressed per file, so 1.10's per-file `Step` takes the project's parsed trees. Offers, all-or-nothing accept and re-derive are unchanged in shape.
- **"Old snapshot in force until accept":** this is read as story 10's `MigrationPending`, so no snapshot is built from unaccepted content. The in-memory snapshot of a running session is untouched.
- **Uppercase hosts are lowercased** rather than refused. The ticket allows either.
- **"Owning a Source" means any Source type,** as AD-48 reads it: a Local Repository Source fetches link lists with its owner seat's allow-list (AD-30).
- **Recent project roots** are passed in through `LoadOptions` (am-config has no index). `agentic validate` passes none. Wiring am-app's runtime index is left to the story that opens projects through the session.
- **Version bump to 2** changes 86 shipped files' `schema_version` line. It is reversible in git, and the 1.10 plan anticipated it.

## Verification

**Commands:**
- `cargo nextest run -p am-domain -p am-glob -p am-config -p am-e2e -p xtask` -- expected: all pass.
- `cargo run -q -p xtask -- check-deps` -- expected: ok.
- `just check` -- expected: green.

**Coordinator test fix (2026-11-22, decided under the standing rules: test structure).** The red-run test `a_host_written_with_a_scheme_fails_at_its_line_with_the_fix` asserted that `seats/alpha.md` had no findings at all. Its `ALPHA` text links `https://other.net/`, which the Host warnings row and `a_link_host_and_a_source_scope_host_off_the_seat_list_are_two_warnings` require to warn. The matrix row says "First validates", which means no error, so the assertion now checks that alpha has no error. The beta assertions (error at line 7, with the fix) are unchanged.

## Auto Run Result

Status: built (2026-11-22). The first run halted in planning (AD-30 against the Company A spec). The owner chose "Option 1: amend AD-30", and the replan landed on main as `cc1c89b`. This run restarted from that baseline.

**Summary:**
- **Hosts:** `am_domain::HostPattern` is the one host parser and matcher.
  - Exact or `*.` patterns, admitting `https:443` and `http:80` only.
  - Refuses schemes, ports, paths, IP literals (WHATWG numeric forms included), `localhost`, non-ASCII and other wildcards.
  - Uppercase is lowercased. `covers` is defined.
- **Seat allow-lists:** seat files carry `allowed_hosts`, which is exactly the snapshot allow-list. Link-host and Source scope-host warnings use it, and Account endpoints are never added.
- **Migration 1→2** (whole-project steps, `SCHEMA_VERSION` 2):
  - moves `tools.yaml` `egress_hosts` into the seats that can fetch, and drops and lists refused entries;
  - blocks a fetching seat whose list is malformed;
  - each offer carries `notes`, and the sha256 of every file it read in `inputs`.
- **Folder roots (AD-30 as amended 2026-11-20 and 2026-11-22):**
  - **Allowed:** `~/Library/CloudStorage/<provider>/` and folders below iCloud Drive.
  - **Refused:**
    - the rest of `~/Library`;
    - the runtime base and app data;
    - the project and its ancestors;
    - recent project roots (wired into `agentic validate`).
  - Read-only project subfolders with no config-named entry are allowed (a name-only `lstat` walk, capped at 100,000 entries). `read_write` is refused under any project.
- **am-glob:** the new leaf crate is the one glob matcher (NFC; case-insensitive for folders, case-sensitive for URL scopes; regex form). check-deps enforces its users, and that only it depends on globset.

**Files changed:**
- `crates/am-domain/src/{host,config_names,lib}.rs`: the host and config-name policies.
- `crates/am-glob/` (new): the matcher. Workspace `Cargo.toml` and `Cargo.lock` register it and pin `unicode-normalization =0.1.26`.
- `xtask/src/deps.rs`: the am-glob layer and the globset rule.
- `crates/am-config/src/{kinds,refs,snapshot,entities,lib,checks,fields,migrate,writer}.rs`: allow-lists, host checks, folder policy, the 1→2 migration and globs.
- `crates/am-app/src/cli.rs`: recent roots for validate.
- `templates/`, `fixtures/`: every file at schema_version 2, `allowed_hosts` on fetching seats, no `egress_hosts`. New `fixtures/projects/hosts-v1/`.
- Tests:
  - new: `crates/am-domain/tests/{host_pattern,config_names}.rs`, `crates/am-glob/tests/glob.rs`, `crates/am-config/tests/{hosts,folder_roots,migrate_hosts}.rs`, `crates/am-app/tests/validate_recent.rs`, plus xtask cases and fixtures;
  - updated for version 2: the 1.10 tests and others.
- `docs/config-schema.md`, `docs/crate-notes/{globset,unicode-normalization}.md`.

**Review:** two quick passes; the Review Triage Log has every row.
- **Pass 1, 9 findings:** 7 patched (4 medium, 3 low), 2 low rejected:
  - the refusal's line is the folder entry's `path:` line;
  - exempt test crates are not checked for am-glob edges.
- **Pass 2 (follow-up, because pass 1 patched four mediums), 4 low findings:** 3 patched, 1 rejected (no seam to test the re-scan just before the first write).
- Nothing deferred.
- Follow-up recommended: false. Pass 2 patched no high, so the work has converged.

**Verification:**
- `just check` passes (529 tests, 7 skipped; check-deps, check-forbid, denylist, runtime-paths, validate, supply chain).
- Every matrix row has a named test that ran and passed.
- The red run was logged before any `src/` change (gate checked). One red-run test was aligned with its matrix row ("validates" means no error) and recorded.

**Residual risks:**
- `am-net`'s `EgressPolicy` still matches exact hosts. The egress story moves it onto `HostPattern`.
- The folder tools (epic 4/12) must apply `is_config_bearing` and the hardlink refusal at access.
- `recent_roots` reaches only `agentic validate`. The session and serve paths take them when they load config.
- The subtree walk of a large linked project subfolder runs on every validate. It is capped at 100,000 entries, about 10 s at the cap.
- The re-scan just before the first write has no test of its own.
