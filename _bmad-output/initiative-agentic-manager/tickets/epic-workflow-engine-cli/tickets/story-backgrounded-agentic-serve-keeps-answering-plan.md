---
title: 'Backgrounded agentic serve keeps answering'
type: bugfix
ticket: 19
created: '2026-11-24'
status: in-progress
baseline_revision: 'd7bcce4714b6719b99d9430e21a368e98f453e70'
route: full
route_source: auto
risk: low
review: quick
review_source: pinned
lenses_ran: []
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
