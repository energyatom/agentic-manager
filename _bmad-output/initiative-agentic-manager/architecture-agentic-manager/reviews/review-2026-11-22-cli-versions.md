# Review 2026-11-22: interim CLI Account edit (AD-15), versions and reality-check lens

- **Spine:** `architecture-agentic-manager.md`, working-tree change on branch `replan/2026-11-22-cli-accounts` (AD-15 Binds, Prevents and the new **Interim CLI Account edit** paragraph; AD-32's one-line pointer; the R6 row of the Capability map).
- **Lens:** every committed decision reality-checked against the existing project (crates, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, the Stack table) and, where a new crate is implied, against crates.io (`cargo info`, 2026-11-22).
- **Reviewer stance:** read-only; nothing in the spine, tickets or code was edited.

## Verdict

**Approve with changes.** No critical finding. The amendment leans on things that mostly exist (TTY detection in std, the store lock that names its holder, `SecretStore` set/delete, `ConfigWriter` with History, `Actor::Human` with the Owner hat, the `decisions_locked` check), but four of its commitments have no carrier in the code or the Stack: no no-echo reader is pinned, the per-widening audit entry cannot ride the existing `ConfigHistory` transaction, the writer cannot create the `models/accounts.yaml` most projects lack, and "real interactive terminal" is stated as a stronger control than `isatty` gives. Each needs a sentence in the spine before epic 2 story 1 builds, or the coder will improvise.

## What was checked and holds

| Assumption in the amendment | Evidence in the project | Result |
|---|---|---|
| Detect a terminal on stdin | `rust-toolchain.toml` pins `1.100.0` (Stack: ≥ 1.93); `std::io::IsTerminal` is stable since 1.70 and already used at `crates/am-app/src/runtime.rs:491` (`TerminalPrompt::interactive`) | Holds; no crate needed |
| Store opened as the one writer, refused naming the holder | `crates/am-store/src/project.rs`: `acquire` takes the OS file lock (`File::try_lock`, std ≥ 1.89) and returns `StoreError::Locked { lock, holder }`; `lock_holder` reads the holder for the words | Holds |
| Key only to the keychain via `SecretStore`, deleted on failure | `crates/am-secrets/src/lib.rs`: `KeychainStore` implements `get`/`set`/`delete` on macOS, non-macOS branch returns "not supported" (AD-47); `am_domain::SecretString` has no Debug/Display/Clone/serde | Holds (Linux CI must use FakeSecretStore) |
| `ConfigEdit` through am-config's writer, History entry, Undo | `crates/am-config/src/writer.rs`: `ConfigEdit { file, ops, body }`, `ConfigWriter::apply/undo/restore`, intent → write → `commit_entry` | Holds for an **existing** file only (H3) |
| `Actor::Human`, the Owner hat | `am_domain::Actor::Human { person, hat }`; `am_app::runtime::owner()` uses `hat: "Owner"` | Holds |
| Refused naming the UI while a stdio MCP server or `render: true` Source is enabled | `am_config::decisions_locked(&root)` already backs the same refusal for `--confirm-move`/`--take-over` (`runtime.rs:805`) | Holds; reuse it (L1) |
| `is_widening` does not exist yet, so an interim path is needed | No `is_widening`, `Widening` type or widening audit anywhere in `crates/` (grep) | Confirmed |
| "No other CLI config edit is allowed" | Built CLI commands: `serve`, `validate`, `audit export/verify`, `spike *`, `--version`. None edits config as `Actor::Human`; `accept_migrations` is not called from `am-app`. The only CLI-launched config write is `serve`'s System `ConfigIdentityAssign` (AD-32) | Holds, wording nit (L4) |
| No `accounts` command already exists | No `accounts` in `crates/am-app/src` | Confirmed |

## Findings

### Critical

None.

### High

**H1. No way to read a key without echo is pinned, and the workspace forbids the unsafe code to hand-roll one.**
Disabling echo needs `tcgetattr`/`tcsetattr`. `[workspace.lints.rust] unsafe_code = "forbid"` rules out calling `libc` directly, and the Stack table names no terminal crate. `Cargo.lock` holds no `rpassword`, `rtoolbox`, `termios`, `crossterm`, `console` or `dialoguer`. Two candidates checked on 2026-11-22:
- `rpassword` **7.5.5**: Apache-2.0 (allow-listed), rust-version 1.85; depends on `rtoolbox` **0.0.6** (Apache-2.0, pre-1.0, "no backwards compatibility guarantees") and `libc` (unix). On Unix its `read_password`/`prompt_password` read from and prompt to **`/dev/tty`**, not stdin (`src/unix.rs: DEFAULT_INPUT_PATH = "/dev/tty"`), and return a plain `String`.
- `nix` **0.31.4**: MIT, already in the lock through `process-wrap` with only its `fs` feature. Its `term` feature gives safe `sys::termios` wrappers on the stdin fd with no new crate in the graph.
AGENTS.md says "never bump or add a crate outside its story" and the Stack must name it; pre-1.0 crates need a `docs/crate-notes/` file.
*Suggested fix (Stack row + one clause in AD-15):* add a Stack row, e.g. "`rpassword` (no-echo key entry for the interim CLI Account edit, AD-15; Apache-2.0; pulls `rtoolbox` 0.0.6 Apache-2.0, pre-1.0, see `docs/crate-notes/rpassword.md`; checked with `cargo info` 2026-11-22) | 7.5.5", **or** "`nix` feature `term` (MIT; already in the graph at 0.31.4 through process-wrap; no-echo key entry, AD-15) | 0.31.4", and in AD-15 replace "read from the terminal without echo" with "read without echo from the same terminal that stdin is (the Stack's no-echo reader), never from arguments, the environment or a pipe".

**H2. The "one audit entry per widening, in the same transaction" has no carrier in the existing History port.**
`am_domain::ConfigHistory::commit_entry(actor, intent_id)` turns the intent into a History entry "with its audit entry" (am-store audits it as `config.edited`). It takes no extra audit records, `NewIntent` has no field for them, and no widening audit path exists yet (the AD-48 `host_added` entry is not in the code either). The amendment's `account_added` entry therefore cannot be committed "in the same transaction" without changing the port in `am-domain` and its `am-store` implementation, and the amended Binds line does not list `am-domain`.
*Suggested fix:* in the amendment, after "in the same transaction", add: "The `ConfigHistory` port carries it: `NewIntent` (or `commit_entry`) takes a list of widening audit records (kind, target, value; never content), which am-store writes in the History entry's transaction; story 2.1 adds this to `am-domain` and `am-store`, and epic 4 story 17's `Widening` list fills the same field later." Add `am-domain` to AD-15's Binds.

**H3. The writer edits existing files only, and most projects have no `models/accounts.yaml`.**
`ConfigWriter::resolve` refuses a missing file ("the file does not exist; the writer edits existing files only"). All eight role templates (`templates/roles/*/`) ship only `company.yaml` and a seat, and fixture `fixtures/projects/hosts-v1` has no `models/accounts.yaml`. A new file must be a `ConfigCreate` (AD-16), which is not in `am-config` and is built only by epic 5 story 16 (first-run wizard). So the amendment's "as the owner's own `ConfigEdit`" fails on the first `accounts add` for any such project.
*Suggested fix (pick one, in AD-15):* (a) "When `models/accounts.yaml` does not exist, `add` creates it with a `ConfigCreate` (AD-16's no-replace atomic write, intent, History and audit), which story 2.1 builds for this one file; the wizard's story 5.16 reuses it." or (b) "When `models/accounts.yaml` does not exist, `add` is refused in words naming the file and the two lines to put in it by hand (`schema_version` and `accounts: []`)". Option (a) avoids teaching owners to hand-edit config; option (b) keeps 2.1 smaller.

**H4. "Stdin is a real interactive terminal" is stated as stopping any agent, script or child process; `isatty` does not do that.**
`IsTerminal` is true for any pseudo-terminal. `script(1)`, `expect`, Python's `pty` module, and agent harnesses or terminal tools that run commands in a pty all pass it, and the typed-name confirmation is just as scriptable through the same pty. What the check does stop is a pipe, a redirected file, and the app's own children with null stdio (stdio MCP servers are already handled by the `decisions_locked` refusal). The "Prevents" line and the closing "so no agent, script or child process can add an Account" overclaim the control.
*Suggested fix:* Prevents: "an agent, script or child process adding an Account through a pipe or non-terminal stdin". Rule: replace "so no agent, script or child process can add an Account and widen its own access" with "so a pipe, a redirected file or one of the app's children cannot add an Account. A process that opens a pseudo-terminal can still pass the check; that residual risk is accepted by the owner until epic 5 story 5's confirm token, which no terminal can present, replaces this path."

### Medium

**M1. How the command finds the project is unspecified, and the read-only resolver that exists does not check the root's identity.**
`am_app::runtime::find_runtime` (used by `agentic audit`) refuses no `project_id`, a legacy folder or no runtime folder, but does not compare the root with the root index or its recorded identity. A copied project folder carries the same `project_id`, so `accounts add` in the copy would write the copy's `models/accounts.yaml` and record History and audit in the original's store. The full resolver (`open_project`) checks identity but also mints, moves and runs store migrations, which the CLI must not do here.
*Suggested fix:* "The command resolves the project read-only as `agentic audit` does and, as `serve` does, refuses in words (never minting, moving or migrating) when the root has no `project_id`, is not this root in the root index or its identity changed (AD-32), still holds a legacy folder, has a config migration pending, or carries an open marker from another Mac."

**M2. The amendment does not say whether the CLI add writes the approved grant set (AD-16).**
AD-16 lets only the wizard's `ConfigCreate`, a confirmed first-load hold or a confirmed Reload create the approved grant set, and an apply update it "when the apply may write it". No grant set exists in code yet (`am-store` has none). If the interim add never touches it, then the first startup after epic 4 story 17 ships will hold every CLI-added Account for the two-step confirmation; if it does, the CLI path becomes a second way to seed the baseline.
*Suggested fix:* add "The interim edit never writes the approved grant set; once 4.17 ships, the first load holds each Account added this way for the two-step widening confirmation (AD-16), so the CLI's typed confirmation is re-confirmed in the UI once."

**M3. Which streams must be terminals, and which one the key comes from, is inconsistent with the code.**
The amendment checks stdin only. The existing `TerminalPrompt::interactive` requires stdin **and** stdout to be terminals. `rpassword` (H1) reads and prompts on `/dev/tty`, a different file from stdin, so a run with stdin a terminal could still read the key from another controlling terminal, and a run that refuses because stdin is piped could otherwise have read `/dev/tty`.
*Suggested fix:* "only when stdin and stdout are both terminals (as for the other CLI decisions); the typed name and the key are read from stdin with echo off for the key, never from `/dev/tty` or another descriptor." If `rpassword` is chosen, require its `read_password_with_config` with stdin as the input, or prefer `nix::sys::termios` on fd 0.

**M4. The owner's 2026-11-22 rule that the release binary has only the real terminal is in ticket 2.1 but not in the spine.**
Epic 2 story 1 says any fake terminal exists only in `cfg(test)` or the dev-only `am-testkit`, and no env var, flag or feature of the release binary enables it. The spine is the binding rule, and the existing `Prompt` trait in `am-app` is public, so a later story could add a non-terminal `Prompt` without contradicting the spine.
*Suggested fix:* add to the amendment: "The only terminal in the release binary is the real one: a fake terminal for tests exists only under `cfg(test)` or in `am-testkit` (dev-dependency), and no env var, flag or Cargo feature of `agentic` enables one (owner, 2026-11-22)."

### Low

**L1. "Enables a stdio MCP server" has no field to test.** `am_config::entities::McpServer` has `transport` but no `enabled` flag. Suggested wording: "while `am_config::decisions_locked` finds a stdio MCP server record or a Source with `render: true` (the same check the decision flags use)".

**L2. The key's in-memory handling.** `rpassword` returns a plain `String` (its internal `SafeString` is not returned). Suggested: "the key is moved into a `SecretString` as soon as it is read; an empty key is refused; the trailing newline is not stored".

**L3. Name the write path.** `ConfigSession::apply` refuses a write while a tracked file differs from accepted bytes and refuses to open when a migration is pending; bare `ConfigWriter::apply` does neither. Suggested: "through am-config's writer, opened as a `ConfigSession` would be (refused while a config migration is pending or the project is invalid)".

**L4. "No other CLI config edit is allowed" vs the System mint.** `serve` writes `project_id` with a System `ConfigIdentityAssign` (AD-32). Suggested: "No other CLI command makes a config edit as `Actor::Human`; AD-32's System mint at open is not a decision."

**L5. Removing has no undo before epic 5.** Undo of `remove` is widening and UI class, and the key is deleted after the commit, so before the UI exists the only recovery is `accounts add` with the key typed again. Suggested: have `remove` print that sentence before it asks to confirm.

**L6. Forward reference.** `agentic service` does not exist yet; the lock refusal will name whichever process holds the lock, so the wording works unchanged once it lands. No change needed.

## Files and evidence

- `rust-toolchain.toml` (channel 1.100.0); `Cargo.toml` (`unsafe_code = "forbid"`, workspace pins); `Cargo.lock` (`nix` 0.31.4 via `process-wrap`; no `rpassword`, `rtoolbox`, `termios`)
- `crates/am-app/src/runtime.rs` (lines 476–503 `Prompt`/`TerminalPrompt`; 805 `decisions_locked`; 985–1019 `find_runtime`); `crates/am-app/src/main.rs` (command dispatch)
- `crates/am-config/src/writer.rs` (`ConfigEdit`, `ConfigWriter::apply`, `resolve` refuses missing files); `crates/am-config/src/session.rs`; `crates/am-config/src/entities.rs` (`Account`, `McpServer`, `Source.render`)
- `crates/am-domain/src/config_history.rs` (`ConfigHistory` port, `NewIntent`); `crates/am-store/src/config_history.rs` (audits `config.edited`); `crates/am-store/src/project.rs` (lock and holder)
- `crates/am-secrets/src/lib.rs`; `crates/am-domain/src/secrets.rs`
- `templates/roles/*/` (no `models/accounts.yaml`); `fixtures/projects/hosts-v1/`
- crates.io via `cargo info`, 2026-11-22: `rpassword` 7.5.5 (Apache-2.0, rust-version 1.85, depends on `rtoolbox` 0.0.x and `libc`, reads `/dev/tty`); `rtoolbox` 0.0.6 (Apache-2.0)
