---
title: 'CI baseline and the just check gate'
type: chore
ticket: '2'
created: '2026-11-20'
status: in-progress
baseline_revision: 'e112a3c02015cc96942d16bea8f5251c334e1089'
route: full
route_source: auto
risk: medium
review: ''
review_source: ''
lenses_ran: []
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/epic-foundations-rust-spike.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The spike's CI runs only fmt, clippy and nextest. Nothing yet stops a forbidden crate edge (AD-4), a copyleft licence (AD-3), `unsafe` code, sample-company vocabulary in `crates/` (AD-23) or a red `just check` from landing.

**Approach:** Harden the spike's CI into the platform baseline (epic R7). Add `cargo xtask check-deps` against the AD-4 matrix, a cargo-deny licence policy plus cargo-audit, a check that every crate forbids `unsafe`, and the department-noun denylist scan over `crates/` and `web/src`. All of it runs in SHA-pinned, least-privilege Actions on macOS arm64, which also builds and uploads the `agentic` binary. The Linux portability job stays. A build hook refuses a commit whose `just check` is red. **Tests first (owner rule, 2026-11-20):** every check's failing tests are written and run red before its implementation.

**Decisions (owner, 2026-11-20):**
- **The build hook is both:**
  - a tracked git pre-commit hook (`.githooks/pre-commit` running `just check`, enabled per clone by `just install-hooks` through `core.hooksPath`);
  - a Claude Code PreToolUse hook in the project `.claude/settings.json` that blocks an agent's `git commit` while `just check` is red.
- **Throwaway PRs are allowed:** open 5 throwaway PRs (GPL crate, adapter edge, `unsafe`, sample id, macOS-only API in am-engine) and 1 clean PR on `energyatom/agentic-manager` from throwaway branches. Never merge them. Record each run URL, then close the PR and delete its branch.
- **The full plan is kept**, about 2,000 tokens.

## Boundaries & Constraints

**Always:**
- Tests first: the first tasks write the failing tests and record the red run in Implementation Notes.
- Every check is offline except cargo-audit's advisory database fetch.
- The licence allow-list is exactly MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0 and MPL-2.0. It is checked on the aarch64-apple-darwin graph (whole workspace) and on the x86_64-unknown-linux-gnu graph without `am-desktop`.
- Actions are pinned by commit SHA with `permissions: contents: read`.
- `#![forbid(unsafe_code)]` in every crate.
- Department vocabulary only under `templates/` and `fixtures/`; the denylist file lives outside the scanned trees.

**Never:**
- A Windows job, release or signing jobs (epic 11).
- `agentic validate` in `just check`: that is story 1.4's.
- Allowing a licence outside the list or adding an `exceptions` entry.
- Bumping or adding a crate the checks don't need.
- Weakening an existing clippy ban.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Adapter edge | `am-mcp` depends on `am-providers` | `check-deps` fails | Names both crates and "adapters may depend only on am-domain and am-net (AD-4)" |
| IO in domain | `am-domain` depends on `tokio` | `check-deps` fails | Names the crate and the rule |
| Clean graph | Current workspace | `check-deps` passes | — |
| GPL crate | Fixture crate `license = "GPL-3.0-only"` as a dependency | cargo-deny fails | Message names the crate and the licence |
| Unsafe | A crate without `#![forbid(unsafe_code)]`, or `unsafe` in code | Fails | Names the file |
| Sample id in crates | `"d1-head"` in `crates/x/src/lib.rs` | Denylist scan fails | Names file:line and the term |
| Sample id in fixtures | Same term under `fixtures/` | Passes | — |

</frozen-after-approval>

## Code Map

- `xtask/src/main.rs` -- placeholder that exits 2; becomes subcommands `check-deps`, `denylist`, `check-forbid`. Reuse the workspace `serde_json` (=1.0.154) to parse `cargo metadata --format-version 1`.
- AD-4 matrix (architecture, Design Paradigm table):
  - `am-domain`: std, serde, thiserror, jiff, uuid only.
  - `am-engine`: domain, store.
  - `am-store` and `am-config`: domain.
  - Adapters (`am-net`, `am-providers`, `am-tools`, `am-connectors`, `am-render`, `am-services`, `am-mcp`, `am-extract`, `am-secrets`): domain, plus `am-net` (only `am-net` may be depended on).
  - `am-api`: domain.
  - `am-server`: domain, engine, api, store.
  - `am-app` and `am-desktop`: anything.
  - `am-testkit` and `am-e2e` are test crates and are checked only for no IO in `am-domain`; `xtask` is exempt.
  - Read the real edges from `cargo metadata`; the current graph must pass.
- `clippy.toml` -- the existing disallowed-methods list (reqwest construction, chromiumoxide launch). Keep it.
- `Cargo.toml` -- workspace lint `unsafe_code = "forbid"`; every crate has `[lints] workspace = true`.
- `.github/workflows/ci.yml` -- macOS `just check` plus binary build; Linux build and test excluding `am-desktop`. Extend it, don't rewrite it.
- `justfile` -- `check` = fmt-check, clippy, test. Add the offline checks (`check-deps`, `check-forbid`, `denylist`) to `check`. A new `supply-chain` recipe runs cargo-deny (both targets) and cargo-audit.
- Licence probe (2026-11-20): strict list passes on the macOS graph. On Linux only `target-lexicon` (`Apache-2.0 WITH LLVM-exception`, via Tauri's gtk stack) fails, and it disappears with `am-desktop` excluded. cargo-deny advisories reports `chrono` unmaintained (warning class); cargo-audit is not installed locally (`brew install cargo-audit`).
- Denylist seed (reference-workload.md ids): `company-a`, `d1-head`, `d2-head`, `d1-e1..d1-e3`, `d2-e1..d2-e4`, `table-t1`, `t1-drop`, `document-a`, `report-r1`, `d1-inputs`, `d1-summary`, `d1-opinion`, `d1-folder-digest`, `d1-notes`, `f2-update`, `f2-d1`, `f3-folder`. `templates/` doesn't exist yet: also add every file stem found under `templates/` and `fixtures/` at scan time.

## Tasks & Acceptance

**Execution:**
- [x] `xtask/tests/` and `xtask/tests/fixtures/` -- FIRST: failing tests for `check-deps` (synthetic metadata JSON: adapter edge, tokio in domain, clean), `denylist` (temp tree: term in `crates/`, in `web/src`, in `fixtures/`), `check-forbid` (crate missing the attribute, `unsafe` block); run them and record the red output -- tests first
- [x] `fixtures/supply-chain/gpl-probe/` (workspace-excluded) and a test that runs cargo-deny on a manifest depending on it -- FIRST, red -- the GPL case offline
- [x] `xtask/src/` -- implement the three subcommands until green -- AD-4, AD-23, unsafe
- [x] `xtask/denylist.txt` -- seed terms plus a comment saying where they come from -- the denylist
- [x] `deny.toml` -- the licence allow-list and graph targets; `bans` and `sources` default-deny unknown registries and git -- AD-3
- [x] `justfile` -- wire the offline checks into `check`; add `supply-chain` -- the gate
- [x] `.github/workflows/ci.yml` -- macOS job: `just check`, `just supply-chain`, build `agentic`, upload it with `actions/upload-artifact` (SHA-pinned). Linux job unchanged in scope -- R7
- [x] `.githooks/pre-commit`, a `just install-hooks` recipe, and a PreToolUse hook in `.claude/settings.json` (a script under `.claude/hooks/` that blocks `git commit` while `just check` fails) -- FIRST, a test that the hook script exits non-zero on a red check -- the build hook
- [x] `docs/crate-notes/` -- none needed unless a new pre-1.0 crate is added (none planned)

**Acceptance Criteria:**
- Given throwaway PRs that add a GPL crate, an adapter-to-adapter edge, `unsafe` code or a sample-company id under `crates/`, when CI runs, then each fails with a named reason.
- Given a throwaway PR in which am-engine calls a macOS-only API without a cfg gate, when CI runs, then the Linux job fails.
- Given a clean PR, when CI runs, then both jobs pass and the macOS `agentic` binary is a downloadable artifact.
- Given a red `just check`, when a commit is attempted, then the hook refuses it.
- Owner step: branch protection on `main` with both CI checks required.

## Implementation Notes

**Red run (tests first, 2026-11-20).** Tests written before any implementation: `xtask/tests/{check_deps,denylist,check_forbid,supply_chain,build_hook}.rs` plus `xtask/tests/fixtures/metadata/*.json` and `fixtures/supply-chain/{gpl-probe,gpl-consumer}`. First run: 24 tests, 23 failed, 1 passed — `supply_chain::gpl_dependency_fails...` passed for the wrong reason (with no `deny.toml`, cargo-deny falls back to a default config that rejects every licence, MIT included). The test was tightened (exactly one rejection, the MIT consumer accepted, no default-config fallback) and an allow-list test added. Second run, recorded red: `cargo nextest run -p xtask` → **25 tests run: 0 passed, 25 failed** (placeholder xtask exits 2; no hooks, settings or deny.toml yet).

**Green.** `cargo nextest run -p xtask` → 28 passed (25 acceptance + 3 lexer/manifest unit tests). `just check` → fmt, clippy, nextest 76 passed / 5 skipped, `check-deps: 20 workspace crates follow the AD-4 matrix`, `check-forbid: 20 crates forbid unsafe code`, `denylist: 74 files clean against 22 seed and 0 derived terms`. `just supply-chain` → `bans ok, licenses ok, sources ok` on both graphs; cargo-audit exit 0 with 3 allowed warnings (chrono and proc-macro-error unmaintained, glib unsound; the last two via Tauri's GTK stack). `cargo clippy -p am-desktop --all-targets -- -D warnings` clean.

**Local rehearsal of the throwaway cases** (edits reverted): am-mcp → am-providers gives `am-mcp -> am-providers: adapters may depend only on am-domain and am-net (AD-4)`; an `unsafe` block in am-engine gives `` crates/am-engine/src/lib.rs:14: `unsafe` code is forbidden in every crate `` (the workspace lint also stops it at compile time); `"D2-Head"` in am-engine gives `` crates/am-engine/src/lib.rs:13: `d2-head` is department vocabulary ... ``; the GPL case is the `supply_chain` test (`gpl-probe`, `GPL-3.0-only`, `rejected`). The PreToolUse hook blocked this session's own Bash call while fmt was red, which is the "red check refuses the commit" criterion seen live.

**Design notes.**
- check-deps reads `cargo metadata --no-deps --offline` (declared edges of workspace members); normal and build edges count, dev-dependencies do not (am-engine and am-tools dev-depend on am-testkit). Every workspace crate must be classified in `xtask/src/deps.rs`, so a new crate fails until it is placed in the matrix.
- check-forbid requires `[lints] workspace = true` in each crate manifest, `#![forbid(unsafe_code)]` in each crate root (`src/lib.rs`, `src/main.rs`, `src/bin/*.rs`, `build.rs`) and no `unsafe` keyword outside comments, strings and char literals. `crates/am-desktop/build.rs` gained the attribute (its only root without it).
- denylist matches case-insensitively on whole tokens (letters, digits and `_` continue a token) over `crates/` and `web/src`; binary files are skipped.
- `.cargo/config.toml` adds the `cargo xtask` alias that AD-4 and `just` use.
- cargo-deny covers licences, bans and sources; advisories are cargo-audit's (unmaintained and unsound are warnings, a vulnerability fails). `multiple-versions = "warn"` because jiff 0.2 and 1.0 coexist by design (crate-notes/croner.md).
- The git pre-commit hook checks the working tree, not only the staged index; `git commit --no-verify` bypasses it, the Claude hook does not. The Claude hook matches `git [global options] commit` anywhere in a Bash command, so a command that merely mentions it (for example in an echo) is also gated while the check is red: it fails closed.

**Outstanding (needs push access and the owner's go-ahead in this session).** Not done here: committing the story, pushing `story/1-2`, the five throwaway PRs (GPL crate, adapter edge, `unsafe`, sample id, macOS-only API in am-engine) and the clean PR on `energyatom/agentic-manager`, recording each run URL here and closing the PRs and deleting their branches; then `gh run list --branch story/1-2`. The Linux-job case (macOS-only API without a cfg gate) can only be shown in CI. Owner step: branch protection on `main` with both CI checks required.

## Plan Change Log

- 2026-11-20 (build): Code Map said the denylist adds "every file stem found under `templates/` and `fixtures/`". Taken literally, the existing spike fixtures (`notes`, `table`, `report`, `c1_tool_use`, ...) would deny ordinary words and fixture names that am-e2e references by path, so the clean tree could never pass. Narrowed to kebab-case stems (containing `-`, hidden files skipped): config ids (seats, documents, tables, workflows, Sources) are kebab-case. Seed list unchanged. Directory names (for example skill folders) are not derived; the seed covers today's ids.
- 2026-11-20 (build): Added `deny_toml_allows_exactly_the_eight_licences_and_no_exceptions` and the "exactly one rejection" assertion after the first red run showed cargo-deny's default config makes the GPL test pass without the policy.

## Review Triage Log

## Verification

**Commands:**
- `cargo nextest run -p xtask` -- expected: red before implementation (recorded), green after
- `just check` -- expected: green, including check-deps, check-forbid and the denylist
- `just supply-chain` -- expected: licences ok on both graphs; cargo-audit reports no vulnerabilities (warnings allowed)
- `gh run list --branch story/1-2` -- expected: both jobs green on the clean head, with the `agentic` artifact

**Manual checks:**
- Each throwaway PR fails with its named reason (the run URL is recorded in Implementation Notes), and then it is closed.
