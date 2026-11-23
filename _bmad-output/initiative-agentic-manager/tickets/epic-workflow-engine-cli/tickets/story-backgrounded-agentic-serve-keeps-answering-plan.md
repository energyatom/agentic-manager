---
title: 'Backgrounded agentic serve keeps answering'
type: bugfix
ticket: 19
created: '2026-11-24'
status: built
baseline_revision: 'd7bcce4714b6719b99d9430e21a368e98f453e70'
route: full
route_source: auto
risk: low
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** story 3.2's Enter reader (`crates/am-app/src/main.rs` ~L333-360) starts whenever stdin is a terminal. Started as `agentic serve &` from an interactive shell, stdin is still the terminal but serve's process group is not the terminal's foreground group, so the pending `read_line` gets SIGTTIN and job control stops the whole server: it stops answering until `fg`.

**Approach:** read stdin for Enter only while serve is the terminal's foreground job, decided without nix's process functions and without unsafe code:
- **Foreground check:** ask the system's `ps` for serve's own process: `/bin/ps -o stat= -p <pid>` (a `+` in the state means "in the foreground process group of its controlling terminal"), run as a child process with a short timeout, through one function in `am_app::serving` (`serve_is_foreground()`), so the Enter reader never calls `read` while serve is in the background. Any failure of the check (no `ps`, timeout, unexpected output) counts as "not foreground": never read.
- **Reader loop:** the reader thread checks before each read; while serve is in the background it waits (re-checking about once a second, or on SIGCONT through tokio's signal stream, whichever the implementer finds simplest without a new crate) and never touches stdin. When serve is brought to the foreground (`fg`), the next check passes and Enter works again, printing a new code that invalidates any unused one (3.2).
- **Residual (as the ticket allows):** a serve moved to the background after starting in the foreground (Ctrl-Z, then `bg`) while a read is already pending is stopped by SIGTTIN when it resumes in the background, because the kernel re-checks the blocked read. The help line keeps naming `agentic serve < /dev/null &` as the safe way to run serve in the background, and the plan records this for the owner.

## Boundaries & Constraints

**Always:**
- **Owner constraints (verbatim):** "no new dependencies, and keep the `nix` crate's `process` feature unchanged (not enabled)". Owner 2026-11-24: "do not call nix's process functions". The workspace nix line stays `features = ["term"]`; no crate's Cargo.toml names nix with another feature; no nix item outside `term` is used (even though Cargo feature unification compiles more of nix); `#![forbid(unsafe_code)]` everywhere; Cargo.toml and Cargo.lock gain no crate.
- **Issuing codes:** a new pairing code is issued only by the Enter reader while serve is the terminal's foreground job, and at start (3.2). While in the background serve issues no new code, whatever arrives on the terminal. No route, CLI command, signal or file issues or returns a code (3.2's rule, re-tested here).
- **Start message and help:** the "press Enter here for a new pairing code" line prints only when stdin is a terminal, and says it works with serve in the foreground; the `agentic serve` help line names `agentic serve < /dev/null &` for background use.
- **3.2 behaviour unchanged:** with stdin not a terminal nothing reads stdin; the startup pairing code is printed as before; every rule_15 test and `serve_project.rs` case stays green.
- **Platform:** macOS only (AD-47); the pty tests compile and skip on Linux; the foreground check returns "not foreground" off macOS.

**Never:**
- No `tcgetpgrp`, `getpgrp`, `setpgid`, `signal`/`sigaction` from nix or libc, no unsafe, no new crate, no SIGTTIN handler (a handler with SA_RESTART makes the read retry in a loop).
- No route, CLI flag, env var or file that issues a pairing code.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error Handling |
|---|---|---|---|---|
| 1 | Foreground Enter | serve started in the foreground of a job-control shell on a pseudo-terminal; Enter typed | A new pairing code (8 base32 characters) is printed; the previous unused code is refused at `/api/v1/pair`, the new one pairs | — |
| 2 | Background keeps answering | `agentic serve &` from a job-control shell on a pseudo-terminal | serve is not stopped (`ps` state has no `T`), and answers CLI-token requests on `/api/v1/health` and `/api/v1/runs` for at least 5 s; before the fix the same test sees serve stopped by SIGTTIN | — |
| 3 | Background issues no code | as row 2, then several Enter keystrokes and text typed on the terminal while serve is in the background | No pairing code beyond the one printed at start appears on serve's output; serve's stdin is never read while in the background (its output carries no new-code line) | — |
| 4 | Brought to the foreground | row 2's serve, then `fg`, then Enter | A new pairing code is printed (reader resumes) | — |
| 5 | No other route issues a code | while serve runs (foreground and background): every route in `route_table()`, with the CLI token and with a session, GET and POST; SIGCONT and SIGWINCH sent to serve; files under the app data dir | No response, file or output line carries a new pairing code; the code count on serve's output stays at the start code (plus only Enter-in-foreground codes) | — |
| 6 | Stdin not a terminal | `agentic serve < /dev/null &` and stdin piped | Nothing reads stdin; serve keeps answering (3.2 cases stay green) | — |
| 7 | Check fails closed | `serve_is_foreground` when `ps` is missing, times out or prints something unexpected | Returns false (never read) | — |
| 8 | Constraints | workspace manifests and lockfile; am-app's nix uses | Cargo.toml/Cargo.lock gain no crate; nix line `features = ["term"]` only; no crate names nix with another feature; no `nix::unistd`/`nix::sys::signal` item and no `tcgetpgrp`/`getpgrp` in the code | — |
| 9 | Help and start line | `agentic serve --help` (or the usage text) and the start output on a terminal | Names Enter-in-the-foreground and `agentic serve < /dev/null &` | — |

</intent-contract>

## Code Map

- `crates/am-app/src/main.rs` (~L333-360): the Enter thread; gate each read on `serving::serve_is_foreground()`; help text.
- `crates/am-app/src/serving.rs` (~L142-150): `stdin_reads_the_owner()` stays; add `serve_is_foreground()` (spawns `/bin/ps -o stat= -p <pid>` with a timeout; macOS only, false elsewhere).
- `crates/am-server/src/pairing.rs` (~L140-175): `pairing_terminal` may take a "may read" check so the loop never reads in the background; the start line text.
- Tests: `crates/am-app/tests/serve_project.rs` or a sibling `serve_job_control.rs` (pty + `/bin/sh -i` or `zsh -i` job control, like `accounts_tty.rs`'s pty module); an am-app unit test for the fail-closed check; a constraints test (manifests, lockfile, nix uses), possibly in xtask's existing release_deps style.

## Tasks & Acceptance

**Execution:**
- [ ] Tests first (no `src/` change): rows 1-9; log the red run.
- [ ] am-app: `serve_is_foreground`, the gated reader, help and start text.
- [ ] `just check` green.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the output, process state and responses match the row.
- Given the workspace, when `just check` runs, then it is green.

## Plan Change Log

- 2026-11-24, owner: build now, tests first; no new dependencies; do not call nix's process functions; tests must cover foreground Enter still printing a new pairing code, `agentic serve &` keeping answering and issuing no code, and no other route issuing a code. Coordinator reading: "issues no code" means no new code while in the background (the startup code of 3.2 is still printed once at start); recorded for the owner.

## Design Notes

- **Why `ps`:** `/bin/ps` reports the `+` flag from the kernel's own record of the terminal's foreground group; it needs no new crate, no nix process item and no unsafe. Its cost (one short child process per check, at most about once a second while in the background) is negligible for an owner terminal.
- **Residual:** Ctrl-Z then `bg` with a read already pending (see Intent). Recorded in the Auto Run Result for the owner.

## Verification

- `cargo nextest run -p am-app -E 'binary(serve_project) | binary(serve_job_control)'`, the rule_15 suite, then `just check`.

## Implementation Notes

### Phase 1 test API (tester, 2026-11-24)

The am-app API the tests are written against (`am_app::serving`, new in this story):

- `serve_is_foreground() -> bool`: this process, through `/bin/ps -o stat= -p <own pid>` with a short timeout, on macOS; `false` on every other platform (AD-47). The Enter thread in `main.rs` asks it before each read.
- `serve_is_foreground_with(ps: &Path, pid: u32, timeout: Duration) -> bool`: the same check with the `ps` program, pid and timeout given (the seam the row 7 tests drive with fake `ps` scripts). It runs `<ps> -o stat= -p <pid>` and returns `false` when the program is missing, when it does not answer within `timeout` (then it is killed and reaped, so no zombie is left; the test checks the fake is gone within 3 s), when it exits non-zero (even after printing a `+`), or when `foreground_from_ps_stat` rejects its output. Off macOS it may simply return `false`: the only test expecting `true` from it is macOS-gated.
- `foreground_from_ps_stat(stat: &str) -> bool`: the parse. `true` only for output that, trimmed, is one line holding one word that starts with an ASCII uppercase letter (the process state) and contains `+` (`"S+"`, `"Ss+"`, `"R+  \n"`); `false` for no `+`, empty output, `"+"`, `"s+"`, two words, two lines, a `STAT` header line or an error message.
- Row 9 text: the `agentic serve` usage (printed on a usage error, exit 2) contains `agentic serve < /dev/null &`, `Enter` and `foreground`. The start line on a terminal keeps naming `Enter` and `foreground` on a line with no code-shaped word. Tester's reading of row 9 against the Boundaries: the start line is not required to name `< /dev/null &` (the Boundaries put that on the help line), and `agentic serve --help` is not required (row 9 allows "or the usage text"); the usage is read through the `--project`-without-a-folder usage error.
- Constraints scan (row 8): in `crates/am-app/src` and `crates/am-server/src`, code with line comments left out may not contain `nix::unistd`, `nix::sys::signal`, `tcgetpgrp`, `getpgrp`, `setpgid`, `sigaction`, `libc::`, `SIGTTIN`, `SIGTTOU`, `from_raw(21)` or `from_raw(22)`, and every `nix::` path must start with `sys::termios`, `pty`, `errno`, `Error` or `Result` (so no grouped `use nix::{...}`). A SIGCONT stream through `SignalKind::from_raw` is allowed.
- Timing the tests allow: a new code within 15 s of Enter in the foreground (the reader may wait about a second between checks); 5.5 s of answering in the background.

Test files (no `src/` change, no Cargo.toml change: every crate used was already a dependency or dev-dependency of am-app):

- `crates/am-app/tests/serve_job_control.rs`: rows 1-6 and 9 against the real binary. Job control needs a shell whose controlling terminal is a pseudo-terminal (a session leader), which `nix::pty::openpty` alone cannot give without `setsid` (a nix process item, or `pre_exec`, which is unsafe); `/usr/bin/script -q /dev/null /bin/sh -i` makes the pty and hands it to the interactive shell, so `"$SERVE_BIN" serve ... &` typed there is a real background job (the pty is its stdin and controlling terminal, its process group not the foreground one; both checked with `ps -o tty=,stat=` and the Enter line). serve's pid, port and CLI token come from `serving/<pid>` and `serving/<pid>.token`; a stopped serve fails with the shell's own `jobs -l` report. The pty rows are `cfg(target_os = "macos")`; off macOS one test says they are skipped, and the usage and piped-stdin rows run on every Unix. Every wait is bounded; serve and script are killed on drop.
- `crates/am-app/tests/serve_foreground_check.rs`: row 7 (the API above).
- `crates/am-app/tests/serve_job_control_constraints.rs`: row 8. The comparison of Cargo.toml and Cargo.lock with the baseline revision (`git show d7bcce4...:Cargo.lock`; no new package, no new dependency edge of a workspace crate, no new `[workspace.dependencies]` key) is `#[ignore]`d: it needs git and the full history (CI checks out a single revision) and pins this story's baseline, so it is run for this story with `--run-ignored all`.
- Row 5's route sweep is in am-app, not am-e2e: it needs the real binary's terminal output (the printed codes), which the 3.2 in-process rig does not have.

### Red run (tester, 2026-11-24)

Commands (shared `CARGO_TARGET_DIR`, this checkout, nothing cleaned):

1. `cargo nextest run -p am-app --test serve_job_control --test serve_job_control_constraints --no-fail-fast --run-ignored all`: 13 tests, 9 passed, 4 failed (run 4 times, same result each time; no serve or script process left afterwards).
2. `cargo nextest run -p am-app --test serve_foreground_check --no-fail-fast`: build fails, E0432 unresolved imports `am_app::serving::foreground_from_ps_stat`, `serve_is_foreground`, `serve_is_foreground_with` (7 tests, none run).
3. `cargo nextest run -p am-app --test serve_project`: 17 passed (story 3.2's cases, unchanged).
4. `cargo clippy -p am-app --test serve_job_control --test serve_job_control_constraints -- -D warnings`: clean; `rustfmt --edition 2024` on the three files. `just check` is red until the API exists (clippy `--all-targets` stops on row 7's E0432), as expected for a failing-first test.

Red, with the observed reason:

- Row 2 `a_backgrounded_serve_is_not_stopped_and_keeps_answering_cli_token_requests`: serve stopped at the first check (ps state `T`); the shell reports `[1]+ <pid> Stopped (tty input): 21 "$SERVE_BIN" serve ...`, i.e. SIGTTIN from the Enter reader's read. This is the ticket's expected red.
- Row 3 `a_backgrounded_serve_issues_no_new_code_when_enter_and_text_are_typed`: no new code was printed and the shell read the typed lines (`typed-42`), but serve was stopped: `Stopped(SIGTTIN)` then `Stopped (tty input): 21`.
- Row 5, background `in_the_background_no_route_signal_typed_line_or_file_issues_a_pairing_code`: the first sweep request (`GET /` with the CLI token) got no answer within 3 s; serve stopped (`T`, `Stopped (tty input): 21`).
- Row 9 `the_serve_usage_names_enter_in_the_foreground_and_dev_null_for_the_background`: the usage is `usage: agentic serve [--project <path> [--confirm-move] [--take-over]] [--port N] [--no-browser]`, without `agentic serve < /dev/null &`, Enter or foreground.
- Row 7, all 7 tests of `serve_foreground_check`: compile red (the API does not exist yet).

Green today, and must stay green after the fix (regression guards):

- Row 1 `in_the_foreground_enter_prints_a_new_code_and_only_the_new_one_pairs` (also row 9's start line, and Ctrl-C ending serve cleanly with its token file removed).
- Row 4 `brought_to_the_foreground_enter_prints_a_new_code_again`: green today because the stopped serve resumes its pending read after `fg`; with the gated reader it must still print a code within 15 s of Enter, the start code refused and the new one pairing.
- Row 5, foreground `in_the_foreground_no_route_signal_or_file_issues_a_pairing_code`: 8 routes x GET/POST x token/session, SIGCONT and SIGWINCH; the code list stays C0, C1, C2, no answer or file carries a code, and the outstanding C2 still pairs.
- Row 6 `with_stdin_from_dev_null_a_backgrounded_serve_reads_nothing_and_keeps_answering` and `with_stdin_piped_nothing_reads_it_and_no_code_follows_enter`.
- Row 8: the three standing checks and the ignored baseline comparison pass on the baseline; they guard the fix's diff.

Harness check: a scratch copy of the pty file (deleted at once, never added to git) running rows 2, 3 and 5 (background) with `< /dev/null &`, so that serve does not read stdin, passed 4 of 4: the background rows turn green once serve stops reading the terminal in the background. Not checked here: a Linux build of the three files (no Linux target installed); the pty rows compile out there.

### Implementation (implementer, 2026-11-24)

What changed (am-app only; no test file edited; no Cargo.toml or Cargo.lock change; am-server untouched):

- `crates/am-app/src/serving.rs`: `serve_is_foreground()` (macOS: `/bin/ps`, this pid, 2 s timeout; elsewhere `false`), `serve_is_foreground_with(ps, pid, timeout)` and `foreground_from_ps_stat(stat)` as the Phase 1 API says. The check spawns `<ps> -o stat= -p <pid>` with stdin from `/dev/null` and stderr dropped, reads at most 4 KiB of its output on a helper thread, and waits for both the output and the exit until the deadline. On timeout it kills and reaps the child. It returns `false` on a spawn failure, a timeout, a non-zero exit, oversized or non-UTF-8 output, or output the parse rejects. The parse: trimmed, exactly one whitespace-separated word, starting with an ASCII uppercase letter and containing `+`. Also new there: `ForegroundInput<R: BufRead>`, a `BufRead` wrapper whose `fill_buf` asks a `may_read` check before every read, buffered or not. While the check says no, it sleeps `recheck` and asks again without touching the inner reader. Plus one unit test (`serving::tests::the_input_is_read_only_after_the_check_passes_and_asked_before_each_read`).
- `crates/am-app/src/main.rs`: the Enter thread hands `am_server::pairing_terminal` a `ForegroundInput` over the locked stdin, with `serve_is_foreground` as the check and a 1 s recheck. `SERVE_USAGE` gained two lines. One says Enter gives a new pairing code with agentic serve in the foreground of a terminal. The other says to start serve as `agentic serve < /dev/null &` to run it in the background (it then never reads the terminal). The start line is unchanged (it already names Enter and the foreground); the binary's doc comment notes 3.19.

Deviations:

- `pairing_terminal` keeps its signature. The plan allowed a may-read check there; the gate lives in am-app's wrapper instead, so the story stays inside one crate and the 3.2 rule_15 callers are untouched.
- The SIGCONT stream was not used; the once-a-second recheck is the simpler of the two the plan offered.

Ctrl-Z/`bg` residual, as observed. This was a scratch driver in the session scratchpad, not added to the repo, on the same `script -q /dev/null <shell> -i` pty as the tests, on Darwin 27.2.0, under `/bin/sh -i` (2 runs) and `/bin/zsh -f -i` (1 run). The sequence: serve started in the foreground (`S+`), Enter gave a code, Ctrl-Z stopped it (`T`, `Stopped(SIGTSTP)`), then `bg`. After `bg` serve was **not** stopped: `S`, `jobs -l` reports `Running`. Then came 3 rounds of: Enter, a text line and an `echo` typed while serve was in the background (the shell read them; serve printed no code and stayed `S`), `fg` plus Enter (one new code, `S+`), and Ctrl-Z plus `bg` again (`S`). The plan's predicted residual did not reproduce. The likely cause: the stop interrupts the waiting terminal read, and `read_line` retries an interrupted read through `fill_buf`, so the retry comes back to the foreground check. That cause is inferred, not traced. A narrow window is still possible: a stop that lands between a passing check and the read it allows. The usage still names `agentic serve < /dev/null &` as the way that never reads the terminal.

Runs (shared `CARGO_TARGET_DIR`, this checkout, nothing cleaned):

- `cargo nextest run -p am-app --test serve_foreground_check --test serve_job_control --test serve_job_control_constraints --no-fail-fast --run-ignored all`: 20 of 20 passed, 4 runs in a row. That includes the ignored baseline comparison: Cargo.toml and Cargo.lock gained no crate. No serve or script process was left afterwards.
- `serve_project` plus am-app's lib tests: 29 of 29 passed. The rule_15 suite (`cargo nextest run -p am-e2e -E 'binary(/^rule_15/)'`): 73 of 73 passed across 10 binaries.
- `just check`: green. fmt, clippy `-D warnings`, nextest 1194 passed and 9 skipped, doctests 9 passed, then check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard and validate.

Not checked: a Linux build (no Linux target here). Off macOS, `serve_is_foreground()` is a plain `false`, and `serve_is_foreground_with` uses only `std::process`.

### Review triage, round 1 (coordinator, 2026-11-24)

One quick lens (correctness, security, spec). No high or medium finding.

| Id | Finding | Sev | Route |
|---|---|---|---|
| P1 | The `#[ignore]`d baseline-comparison test is pinned to d7bcce4, panics on a new workspace crate, and is run by nothing; "no new crate" was this story's constraint, not a standing rule | low | patch: remove it; its one-time result stays recorded in the red-run notes and `git diff d7bcce4 -- '*Cargo.toml' Cargo.lock` is empty |
| P2 | `PS_TIMEOUT` unused off macOS (warning); off macOS the "press Enter" line prints though Enter never works | low | patch: gate the constant; off macOS start no Enter thread and print no Enter line |
| P3 | A backgrounded serve runs `/bin/ps` every second for its whole life, even after its terminal is gone | low | patch: back off the re-check (1 s doubling to 10 s, reset after a passing check); end the reader when stdin is no longer a terminal |
| P4 | Ctrl-Z then `bg` is verified only by a scratch driver | low | patch: a macOS pty test (foreground Enter gives C1; Ctrl-Z, `bg`; Enter and text: not stopped, no new code; `fg`, Enter gives C2) |
| P5 | Nits: stale comment ("the thread ends with stdin"), redundant import, `thread::spawn` panics instead of failing closed, `ps` inherits serve's environment and working folder | low | patch: fix the comment and import; `thread::Builder::spawn` failure counts as "not foreground"; `ps` runs with `env_clear()` and `current_dir("/")` |
| I1 | Row 5 (no route issues a code) lives in am-app's tests, not `am-e2e/tests/trust/`, so the release gate (by directory) does not run it; it needs the real binary | info | record for the owner |

### Review patch round 1: test API (tester, 2026-11-24)

- **P1 (coordinator decision):** the `#[ignore]`d baseline comparison `cargo_toml_and_cargo_lock_gain_no_crate_since_the_baseline` and its git and lockfile helpers are removed from `serve_job_control_constraints.rs`. "No new crate" was this story's constraint, not a standing rule. Its one-time result stays recorded above (the red run and the implementer's run both passed it). Re-checked now: `git diff d7bcce4 --stat -- '*Cargo.toml' Cargo.lock` is empty. The three standing checks (the nix line, no other nix feature in any manifest, no nix process or signal item in am-app or am-server) remain.
- **P3 API** (`am_app::serving`), tested in the new `crates/am-app/tests/serve_foreground_reader.rs`:
  - `next_recheck(prev: Duration, passed: bool) -> Duration` is the wait between foreground checks.
    - After a failing check it is `min(2 × prev, 10 s)`, and `Duration::ZERO` gives 1 s, so the sequence is 1, 2, 4, 8, 10, 10 s.
    - After a passing check it is 1 s, whatever `prev` was.
  - `ForegroundInput::new(inner, may_read, still_terminal, sleep)` replaces the `recheck: Duration` parameter with two parameters:
    - `still_terminal: Box<dyn FnMut() -> bool + Send>`: serve passes `stdin_reads_the_owner`.
    - `sleep: Box<dyn FnMut(Duration) + Send>`: serve passes `std::thread::sleep`; the tests record the waits instead of sleeping.
  - Before every read of `inner`, `ForegroundInput` asks `still_terminal`. When that answers no, the read returns end of input (`Ok(0)`) at once, without reading `inner` and without waiting, whatever `may_read` says, so the reader thread ends.
  - While `may_read` answers no, it calls `sleep(wait)`; the waits follow `next_recheck` and go back to 1 s after a passing check. The implementer's existing unit test in `serving.rs` uses the old three-argument `new`, so it changes with the API.
- **P4:** `after_ctrl_z_and_bg_serve_keeps_answering_and_issues_no_code_until_fg` in `serve_job_control.rs`, reusing `Shell`. The steps:
  - Serve starts in the foreground and Enter gives C1. Ctrl-Z stops serve (the test waits for state `T` and the prompt), then `bg`.
  - After 1.5 s serve is not `T` and not `+`. Enter (twice), a text line and an `echo` are typed; the shell answers them.
  - Serve prints no code beyond C0 and C1, and answers `/api/v1/health` and `/api/v1/runs` with the CLI token for 5.5 s.
  - Then `fg` and Enter give C2; C1 gets 401 `pairing-code-invalid` and C2 pairs.
- **P5:** `ps_runs_with_an_empty_environment_in_the_root_folder` in `serve_foreground_check.rs` (macOS). A fake `ps` writes `/usr/bin/env` and `pwd -P` to files and answers `S+`.
  - The test requires the working folder to be `/` and no variable other than `PWD`, `SHLVL` and `_`.
  - Those three are what I saw `/bin/sh` itself set for its children when started with an empty environment from `/` (`env -i /bin/sh -c /usr/bin/env` prints exactly `PWD=/`, `SHLVL=1`, `_=/usr/bin/env`). The test also requires `PWD=/`.
  - Failure messages name variables, never their values.
- **P2:** no test here (it is a Linux-only behaviour and there is no Linux target on this Mac).

### Review patch round 1: red run (tester, 2026-11-24)

Shared `CARGO_TARGET_DIR`, this checkout, nothing cleaned, no `src/` file touched (the implementer's round-0 changes to `main.rs` and `serving.rs` are in the tree).

1. `cargo nextest run -p am-app --test serve_job_control --test serve_job_control_constraints --test serve_foreground_check --no-fail-fast`: 21 tests, 20 passed, 1 failed.
   - **Red:** P5 `ps_runs_with_an_empty_environment_in_the_root_folder`. The message: `ps ran in ".../crates/am-app" (want /) and inherited [...]`, a list of about 120 variable names from the test process (HOME, PATH, TMPDIR, CARGO_*, NEXTEST_* and others). Today `ps` inherits serve's environment and working folder.
   - **Green on arrival:** P4 `after_ctrl_z_and_bg_serve_keeps_answering_and_issues_no_code_until_fg` (12.4 s). The implementer's `ForegroundInput` already survives Ctrl-Z then `bg`, as their scratch driver saw. `serve_job_control` passed 10 of 10 in 4 runs, and no serve or script process was left afterwards.
   - **Now green after round 0:** row 9's usage test and rows 2, 3 and 5 (background). The 3 constraint tests also pass.
2. `cargo nextest run -p am-app --test serve_foreground_reader --no-fail-fast`: the build fails, which is the expected red for P3's 4 tests.
   - E0432: `am_app::serving::next_recheck` does not exist.
   - E0061: `ForegroundInput::new` takes 3 arguments, but 4 were supplied.
3. `cargo clippy -p am-app --test serve_job_control --test serve_job_control_constraints --test serve_foreground_check -- -D warnings`: clean. `rustfmt --edition 2024` was run on the four test files.

### Review patch round 1 (implementer, 2026-11-24)

What changed (am-app `src/` only; no Cargo.toml or Cargo.lock change; am-server untouched):

- **P2:** `PS_TIMEOUT` is now `#[cfg(target_os = "macos")]`, so it has the same gate as its only user, `serve_is_foreground()`. In `main.rs`, `enter_for_codes = cfg!(target_os = "macos") && stdin_reads_the_owner()`, so off macOS serve starts no Enter thread and prints no Enter line (AD-47).
- **P3:**
  - `next_recheck(prev, passed)`: 1 s after a pass or from `Duration::ZERO`, otherwise `min(2 × prev, 10 s)` (`saturating_mul`).
  - `ForegroundInput::new(inner, may_read, still_terminal, sleep)`. Its `fill_buf` loops as follows. If `still_terminal` answers no, it returns end of input (`Ok(&[])`) at once. If `may_read` answers yes, the wait goes back to 1 s and it reads `inner`. Otherwise it calls `sleep(wait)` and doubles the wait. The first wait is `next_recheck(ZERO, false)` = 1 s.
  - Serve passes `serve_is_foreground`, `stdin_reads_the_owner` and `std::thread::sleep`.
  - The unit test in `serving.rs` uses the 4-argument API and also checks the waits (1, 2, 4 s).
- **P5:**
  - The stale comment ("the thread ends with stdin") now says the thread ends at the end of input or once stdin is no longer a terminal.
  - The redundant inner `use std::io::Read as _;` is gone.
  - The answer thread is started with `std::thread::Builder::spawn`. If that fails, `ps` is killed and reaped and the answer is `false`.
  - `ps` runs with `env_clear()` and `current_dir("/")`.

Deviation, in a test file: `crates/am-app/tests/serve_foreground_reader.rs`, as the tester wrote it, failed `cargo clippy --all-targets -D warnings` on two points once it compiled:

- `clippy::doc_lazy_continuation`: a module-doc line began with `+ Send>`, which markdown reads as a list item.
- `clippy::type_complexity`: the return type of `recorded()`.

I made a lint-only edit, after the 4 tests had passed unedited:

- Rewrapped those doc lines (same words).
- Added `type Sleep = Box<dyn FnMut(Duration) + Send>;` and `type Waits = Arc<Mutex<Vec<Duration>>>;` for `recorded()`.

No assertion, input or step changed. For the tester or coordinator to confirm or redo.

Runs (shared `CARGO_TARGET_DIR`, this checkout, nothing cleaned):

- am-app targets, run 3 times in a row with `--run-ignored all`: 54 of 54 passed each time, and no serve or script process was left afterwards.
  - `serve_job_control`: 10, including P4's `after_ctrl_z_and_bg_serve_keeps_answering_and_issues_no_code_until_fg`.
  - `serve_foreground_check`: 8, including P5's `ps_runs_with_an_empty_environment_in_the_root_folder`.
  - `serve_foreground_reader`: 4.
  - `serve_job_control_constraints`: 3.
  - `serve_project`: 17.
  - am-app lib: 12.
- The rule_15 suite (`cargo nextest run -p am-e2e -E 'binary(/^rule_15/)'`): 73 of 73 passed.
- `just check`: green. fmt, clippy `-D warnings`, nextest 1200 passed and 8 skipped, doctests 9 passed, then check-deps, check-forbid, denylist, runtime-paths, fixed-hosts, seam-guard and validate.

Not checked: a Linux build (no Linux target here). By reading the code: off macOS, `PS_TIMEOUT` is compiled out together with its only user, `RECHECK_FIRST` and `RECHECK_CAP` are used by `next_recheck` on every platform, and `main.rs` needs no other cfg.

## Auto Run Result

**Status:** built (2026-11-24).

**Summary:** `agentic serve`'s Enter reader now reads stdin only while serve is the terminal's foreground job.
- **Foreground check:** `am_app::serving::serve_is_foreground()` runs `/bin/ps -o stat= -p <pid>` (absolute path, no shell, empty environment, working folder `/`, output capped, killed and reaped on a 2 s timeout); any failure means "not foreground". macOS only; elsewhere serve starts no Enter thread and prints no Enter line.
- **Reader:** `ForegroundInput` asks before every read; in the background it never touches stdin and re-checks after 1, 2, 4, 8, then every 10 s (back to 1 s after a pass); it ends when stdin is no longer a terminal.
- **Results:** `agentic serve &` keeps answering and issues no new code whatever is typed; `fg` then Enter gives a new code that invalidates the unused one; no route, signal or file issues a code; Ctrl-Z then `bg` also keeps serving (observed and now tested; the plan had expected a residual). The usage text names Enter in the foreground and `agentic serve < /dev/null &`.
- **Owner constraints held:** no new crate (Cargo.toml and Cargo.lock unchanged), no nix process or signal item, no libc, no unsafe, no SIGTTIN handler; standing checks keep the nix line at `term` only and forbid those items in am-app and am-server.

**Tests:** 25 new (`serve_job_control.rs` 10 on a real pty with a job-control shell, `serve_foreground_check.rs` 8, `serve_foreground_reader.rs` 4, `serve_job_control_constraints.rs` 3), written failing first; the background rows were red by SIGTTIN stopping serve. `just check`: 1200 passed, 8 skipped.

**Review:** one quick lens; no high or medium finding; 5 low patched tests-first (P1 the one-time baseline-comparison test removed, its result kept in the notes; P2 Linux; P3 back-off and end on a lost terminal; P4 the Ctrl-Z/`bg` test; P5 hygiene). The implementer rewrapped one doc comment and added two type aliases in a tester's file for clippy, with no assertion changed (checked by diff).

**For the owner:**
- **Reading taken:** "issues no code" means no new code while in the background; the startup code (story 2) is still printed once at start.
- **Residual:** a stop that lands between a passing check and the read could still catch a pending read; `agentic serve < /dev/null &` never reads the terminal.
- **Note:** row 5 (no route issues a code) runs in am-app's tests because it needs the real binary, so the release gate's `tests/trust/` sweep does not include it.
