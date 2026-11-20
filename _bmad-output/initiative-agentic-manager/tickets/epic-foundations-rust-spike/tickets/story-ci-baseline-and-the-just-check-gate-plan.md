---
title: 'CI baseline and the just check gate'
type: chore
ticket: '2'
created: '2026-11-20'
status: built
baseline_revision: 'e112a3c02015cc96942d16bea8f5251c334e1089'
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

**Review fixes (2026-11-20).** Commit `d130943`. The fixes:
- **Claude hook:** now a python3 wrapper that parses the payload as JSON. It gates when any `tool_input` string holds the words `git` and `commit`, and blocks an unreadable payload. It runs `just check` in the payload's `cwd`, or in the toplevel of a `git -C` path. The matcher is `Bash|mcp__terminal__run_in_terminal`. This replaces the sed-based matcher described above.
- **Scans:** `denylist` and `check-forbid` list files through `git ls-files --cached --others --exclude-standard`, so git-ignored files are never scanned.
- **Denylist seed:** adds `structured-review` and `plain-report`.
- **Supply chain:** `deny.toml` targets aarch64-apple-darwin; `supply-chain` passes `--locked` and runs the cargo-deny-dependent test, which is now `#[ignore]` in `just check`.
- **Caller verification:** `just check` passed (82 tests, 6 skipped). `just supply-chain` passed: licences ok on both graphs, 3 allowed audit warnings, and the GPL test passed.

**CI evidence (2026-11-20, caller).** Throwaway PRs from branches off `story/1-2` at `43f5461`, closed unmerged afterwards:

| PR | Case | macOS job | Linux job | Named reason |
|---|---|---|---|---|
| [#1](https://github.com/energyatom/agentic-manager/pull/1) ([run 44684622595](https://github.com/energyatom/agentic-manager/actions/runs/44684622595)) | adapter edge am-mcp → am-providers | fail | fail | `am-mcp -> am-providers: adapters may depend only on am-domain and am-net (AD-4)` |
| [#2](https://github.com/energyatom/agentic-manager/pull/2) ([run 44684681606](https://github.com/energyatom/agentic-manager/actions/runs/44684681606)) | GPL dependency (am-app → gpl-probe) | fail in `just supply-chain` | pass (the licence check runs on macOS) | `license = "GPL-3.0-only"` … `rejected: license is not explicitly allowed` |
| [#3](https://github.com/energyatom/agentic-manager/pull/3) ([run 44684755515](https://github.com/energyatom/agentic-manager/actions/runs/44684755515)) | `std::os::macos` in am-engine, no cfg gate | pass (on re-run; the first attempt hit a runner DNS failure fetching the toolchain) | fail | `error[E0433]: cannot find macos in os` |
| [#5](https://github.com/energyatom/agentic-manager/pull/5) ([run 44685823633](https://github.com/energyatom/agentic-manager/actions/runs/44685823633)) | `unsafe` block in am-engine (committed by the owner) | fail (rustfmt reached first) | fail | `error: usage of an unsafe block` under `#![forbid(unsafe_code)]` |
| [#6](https://github.com/energyatom/agentic-manager/pull/6) ([run 44685851685](https://github.com/energyatom/agentic-manager/actions/runs/44685851685)) | `"d1-head"` in am-engine (committed by the owner) | fail | fail | `` crates/am-engine/src/lib.rs:14: `d1-head` is department vocabulary; it belongs only under templates/ or fixtures/ (AD-23) `` |
| [#4](https://github.com/energyatom/agentic-manager/pull/4) ([run 44684761439](https://github.com/energyatom/agentic-manager/actions/runs/44684761439)) | clean story PR | pass | pass | Artifact `agentic-macos-arm64` (8,576,866 bytes) uploaded |

- **Build hook, refusing:** in the caller's session the Claude Code hook refused `git commit --allow-empty` on a red tree ("Blocked: 'just check' is red").
- **Build hook gap:** a single Bash command that edits a file and commits is checked before the edit runs, so that commit passed. This is how the adapter-edge commit was made; it goes to the review.
- **Why the owner committed #5 and #6:** the hook blocked the caller from committing those two red cases (owner decision, 2026-11-20).

## Plan Change Log

- 2026-11-20 (build): Code Map said the denylist adds "every file stem found under `templates/` and `fixtures/`". Taken literally, the existing spike fixtures (`notes`, `table`, `report`, `c1_tool_use`, ...) would deny ordinary words and fixture names that am-e2e references by path, so the clean tree could never pass. Narrowed to kebab-case stems (containing `-`, hidden files skipped): config ids (seats, documents, tables, workflows, Sources) are kebab-case. Seed list unchanged. Directory names (for example skill folders) are not derived; the seed covers today's ids.
- 2026-11-20 (build): Added `deny_toml_allows_exactly_the_eight_licences_and_no_exceptions` and the "exactly one rejection" assertion after the first red run showed cargo-deny's default config makes the GPL test pass without the policy.

## Review Triage Log

### Pass 1 (quick lens plus one caller finding, 2026-11-20): high 3 · medium 6 · low 5 · false 3 · maybe-false 0

| # | Finding | Verdict | Route | Evidence |
|---|---|---|---|---|
| 1 | CI acceptance evidence missing (throwaway PRs, clean PR, artifact, run list) | false | reject | The diff was staged before the caller ran CI. The six runs and the artifact are now recorded under "CI evidence". Branch protection is the owner step still to do. |
| 2 | Claude hook misses `git commit` after a newline or tab (`\n` in the JSON payload puts `n` before `git`) | high | patch | Reproduced: payload `git add -A\ngit commit -m x` → MISSED. |
| 3 | Claude hook misses `git -C "<path with spaces>" commit` | high | patch | Reproduced: `git -C "/a/[17] b c" commit` → MISSED. This repo's own path has spaces. |
| 4 | Claude hook runs `just check` in the project dir, not the tree being committed (worktrees) | medium | patch | Script line `root=${CLAUDE_PROJECT_DIR:-...}`. The payload carries `cwd`, which it ignores. |
| 5 | Claude hook exits 0 when the payload has no readable command | medium | patch | Reproduced: a payload without `"command"` → MISSED, exit 0. It should fail closed. |
| 6 | Hook matcher is `Bash` only; another shell tool (the terminal tool) can commit | medium | patch | `.claude/settings.json` matcher `"Bash"`. |
| 7 | The hook blocks the plan's own throwaway-PR step | false | reject | That is the hook working as intended. The owner committed the two red cases by hand (decision, 2026-11-20). |
| 8 | Denylist seed misses the sample skill ids `structured-review` and `plain-report` | low | patch | Both are named in reference-workload.md and missing from `xtask/denylist.txt`. Two lines to add. |
| 9 | `denylist` and `check-forbid` walk git-ignored local files (for example `crates/am-e2e/c4*.md`, which holds the owner's file names) | medium | patch | `walk.rs::skipped` ignores `.gitignore`. A local run can go red, or quote private files, where CI never would. |
| 10 | Directory-style binary roots (`src/bin/<name>/main.rs`) and custom `path` roots are not checked for the attribute | low | reject | Real, but no such crate exists, the workspace lint still forbids `unsafe` there, and the fix adds branches. |
| 11 | `r#unsafe` raw identifier is a false positive | low | reject | Real but unlikely in this codebase; fixing it would add lexer complexity. |
| 12 | Licence-check graph targets live only in the justfile, not in `deny.toml` (the plan task said deny.toml) | low | patch | A bare `cargo deny check licenses` fails on the Linux `am-desktop` graph. Setting `targets = ["aarch64-apple-darwin"]` in deny.toml makes the bare run match the macOS gate. |
| 13 | `just supply-chain` doesn't pass `--locked` to cargo-deny | low | patch | Its internal `cargo metadata` may rewrite the lockfile. The test already passes `--frozen`. |
| 14 | `just check`, and so both commit hooks, now need cargo-deny installed, because `supply_chain.rs` runs inside nextest | medium | patch | Without cargo-deny every commit is refused. The plan keeps cargo-deny in the separate supply-chain recipe. |
| 15 | A shared `CARGO_TARGET_DIR` could run an xtask built from another worktree (`env!("CARGO_MANIFEST_DIR")`) | false | reject | `cargo xtask` is `cargo run -p xtask`, and cargo fingerprints by package path, so each worktree rebuilds its own binary before running. nextest builds per invocation the same way. |
| 16 | (caller) PreToolUse runs before the command, so one command that edits a file and commits is judged on the pre-edit tree | high | patch | Observed: the adapter-edge commit went through. This is inherent to a pre-tool check. The git pre-commit hook runs at commit time and closes the gap once `just install-hooks` is run in the clone. |

## Verification

**Commands:**
- `cargo nextest run -p xtask` -- expected: red before implementation (recorded), green after
- `just check` -- expected: green, including check-deps, check-forbid and the denylist
- `just supply-chain` -- expected: licences ok on both graphs; cargo-audit reports no vulnerabilities (warnings allowed)
- `gh run list --branch story/1-2` -- expected: both jobs green on the clean head, with the `agentic` artifact

**Manual checks:**
- Each throwaway PR fails with its named reason (the run URL is recorded in Implementation Notes), and then it is closed.
