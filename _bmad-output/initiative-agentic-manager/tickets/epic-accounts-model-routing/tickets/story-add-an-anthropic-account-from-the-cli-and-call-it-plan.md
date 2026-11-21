---
title: 'Add an Anthropic Account from the CLI and call it'
type: feature
ticket: 1
created: '2026-11-22'
status: done
baseline_revision: 'dca40131447d289608a87d8be57edb90bc483f1e'
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
warnings: [oversized]
deferred: []
---

<intent-contract>

## Intent

**Problem:** Epic 2 needs its tracer bullet. There is no way to add an Account and call it. The config writer cannot create a file, no command writes config from the CLI, and nothing reads a key without echo. The owner's decisions of 2026-11-22 and AD-15/AD-16 as amended allow one interim CLI path, with strict limits.

**Approach:** `agentic accounts add|list|test|remove` in am-app:
- `add` and `remove` are AD-15's interim CLI Account edit.
- `add` writes `models/accounts.yaml` through am-config's writer. When the file is missing, it creates it with a new `ConfigCreate`.
- The key goes only to the keychain through `SecretStore`, read with echo off through `nix` termios.
- `list` shows each key masked.
- `test` streams one short reply from the spike's Anthropic adapter, through an `EgressClient` that admits only the Account's pinned host.

## Boundaries & Constraints

**Always:**
- **Terminal (AD-15, owner):**
  - `add` and `remove` run only when stdin and stdout are both a real interactive terminal, checked by one am-app predicate. Any other stdin is refused in words before anything is read, written or stored.
  - The release binary has only the real terminal. A fake terminal exists only in am-testkit (a dev-dependency) or under `cfg(test)`, and no env var, flag or feature enables one.
  - A guard test fails if am-testkit or any fake terminal reaches a non-dev dependency of the `agentic` binary.
- **Open (AD-15):**
  - The command resolves the project read-only, by AD-32's checks. It mints no `project_id`, moves no legacy folder, migrates no store and writes no marker, and refuses in words if any of those would be needed.
  - It holds the store's lock for the whole command, and is refused, naming the holder, while another process holds it.
  - It is refused while config enables a stdio MCP server or a Source with `render: true`, and while an outside change waits for Reload: any tracked file differs from its last History `after` bytes, or the session reports one.
- **Add (AD-15):**
  - Refuse a reserved id, or an id already in the file.
  - Print one sentence naming the Account, its provider, its endpoint (`https://api.anthropic.com`), and every seat, fallback, capability or Source that already names the id.
  - Require the id typed within 2 minutes. A mismatch or a timeout writes and stores nothing.
  - Only then read the key with echo off. Reuse an existing keychain entry for that id and never overwrite it.
  - Write the file. If the write fails, delete the keychain entry, but only one this command created.
  - The file holds `secret_ref: <id>` and never the key. The Account's `provider` is `anthropic`; `name` is optional.
- **Echo (owner):** echo is restored on every exit path: success, error, wrong name, timeout and Ctrl-C (a typed `^C`, and SIGINT). A panic restores it too, through a drop guard.
- **Record (AD-15, AD-16, AD-25):**
  - Each write commits its History entry. In the same store transaction, an add writes one audit entry of kind `account_added`, carrying the file, the Account id, the pinned scheme, host and port, the references it brings into force and `confirmation: typed_id`, and never the key.
  - An add never creates the first approved grant set (no grant-set store exists yet, so there is nothing to update).
- **ConfigCreate (AD-16):**
  - Atomic: a temp file, fsync, a hard link to the final name (failing when the name exists), removal of the temp file, then a folder fsync.
  - Missing parent folders inside the project are created.
  - It carries the same intent, History and audit as an edit, under a new `HistoryKind::Create` (`create`).
  - Undo of a create is refused in words: the writer never deletes a file.
  - A volume that refuses hard links refuses the create in words.
- **Remove (AD-15):**
  - Refuse a reserved id, an unknown id, or an id that a seat, fallback, capability or Source still names, naming them.
  - Otherwise it is a plain edit with Undo. It leaves the keychain entry in place and says so.
- **List and test:** both make no config edit, and both run with any stdin.
  - `list` shows each Account's id, provider, host and key state: masked (dots plus the last 4 characters), or "no key in Keychain".
  - `test <id>` streams one short reply through `NetEgress` with a policy admitting only the Account's host, and no loopback port.
- **Tests:** recorded fixtures, FakeSecretStore and FakeEgress, with a canary key. No real key, no network.

**Never:**
- No key from arguments or the environment, and no key in config, logs, audit, History or output.
- No other CLI config edit, and no API route that adds an Account.
- No `is_widening` (epic 4 story 17), no OpenAI Accounts (story 3) and no redaction layer (story 5).
- Changes to `default_egress` are limited to what the `test` command needs.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add | Terminal; `add cloud2 --provider anthropic`; id typed; canary key | Sentence printed; key in FakeSecretStore under `cloud2`; file gains `{id: cloud2, provider: anthropic, secret_ref: cloud2}`; History entry; `account_added` audit with host `api.anthropic.com`, port 443, no key | — |
| Not a terminal | Binary with piped stdin (real process) | Exit non-zero; nothing read, written or stored | Worded refusal naming the terminal rule |
| Wrong name | Typed name differs | Nothing written or stored; echo restored | Worded refusal |
| Interrupt | Real binary on a pty; `^C` at the key prompt | Exits non-zero; the pty's ECHO flag is set again; nothing written or stored | — |
| Missing file | Project without `models/accounts.yaml` | Add creates it (`ConfigCreate`, History `create`) | A file that appears meanwhile refuses the create |
| Reserved or duplicate | `local-ollama`; an existing id | Refused; nothing changes | Worded |
| Reuse key | Keychain already holds the id | Not overwritten; the add says it reuses it | — |
| Write fails | Writer refuses after the key was stored by this command | Keychain entry deleted; file unchanged | Writer's refusal shown |
| List | Two Accounts, one with a key | Masked `••••…` plus the last 4, and "no key in Keychain" | — |
| Test | Account with a canary key; FakeEgress replays `anthropic/c1_count_stream.sse` | Streamed text printed; the request's `AuthRef` names the Account; pinned policy admits only `api.anthropic.com` | A host outside the pin is refused by `check_url` |
| Remove | Unreferenced id; referenced id | First removed with History and Undo; second refused, naming the seats | — |
| Busy | Store lock held by another process; an outside change pending | Refused, naming the holder or the file | — |
| Guard | `cargo metadata` of the workspace | am-testkit is not a non-dev dependency of am-app or of anything it links | — |

</intent-contract>

## Code Map

- `crates/am-domain/src/secrets.rs` — `SecretStore` (`get`, `set`, `delete`) and `SecretString` (`new`, `expose`). `net.rs` — `AuthRef`. `llm.rs:176` — `LlmProvider`.
- `crates/am-secrets/src/lib.rs` — `KeychainStore::new()`; service `agentic-manager`, account `<id>`.
- `crates/am-testkit/src/lib.rs` — `FakeSecretStore` (561-603), `FakeEgress::{respond_if, replay, requests}`, `recorded`, `CollectEvents`. Add a `FakeTerminal` here (a scripted line and key input, an interrupt, and echo-state recording), never in am-app.
- `crates/am-providers/src/anthropic.rs` — `AnthropicProvider::new(egress, account_id)` and `ANTHROPIC_HOST`. `am_app::spike::c1_probe` can be reused.
- `crates/am-net/src/lib.rs` — `EgressPolicy{allowed_hosts, loopback_ports, max_body_bytes}`, `NetEgress::new(policy, secrets, audit)`, `check_url`.
- `crates/am-config/src/writer.rs`:
  - `ConfigWriter::apply` and `resolve` (~787 refuses a missing file); `write_step`; `replace`.
  - Add `create(actor, file, bytes)` with hard-link placement.
  - `canon::apply` addresses list items by index: append at `len`, remove by index. When `accounts:` is absent, Set `["accounts"]` to an array.
- `crates/am-config/src/kinds.rs:433-517` — the `accounts()` loader. Its keys are `id, provider, name, description, base_url, secret_ref`; top-level keys are `ACCOUNTS_KEYS` (:60). `refs.rs:10` — `RESERVED_ACCOUNTS`.
- `crates/am-domain/src/config_history.rs` — add `HistoryKind::Create` and a way for one commit to carry extra audit entries (for example `NewIntent.audit: Vec<AuditNote>` written by am-store in `commit_entry`'s transaction). am-store implements it in `crates/am-store/src/config_history.rs`, auditing `config.created` for a create.
- `crates/am-app/src`:
  - `main.rs` dispatch (`match argv.first()`): add `accounts`.
  - `cli.rs`: `EXIT_*`; add `pub fn accounts(args, ctx: AccountsContext, out, err) -> i32`. `AccountsContext` holds the terminal, the secret store, the egress factory and the places. `main` builds it with the real terminal, `KeychainStore`, `NetEgress` and `Places::from_env()`.
  - `runtime.rs`: `owner()` (65), `find_runtime`, `open_project` (599; do not use it: it mints and moves), `TerminalPrompt` (467-503).
  - New `terminal.rs`: the `Terminal` trait and the real implementation over `nix::sys::termios`. It holds a drop guard, reads with `ICANON` and `ISIG` off so that `^C` is read as cancel, and handles SIGINT through `tokio::signal` (or a handler) that restores echo.
- `Cargo.toml` — workspace `nix = { version = "=0.31.4", default-features = false, features = ["term"] }`, used by am-app only (and its tests, for `pty`). check-deps: am-app may use `nix`.
- **Tests:**
  - `crates/am-app/tests/accounts_cli.rs` (in-process, with `FakeTerminal`, `FakeSecretStore` and `FakeEgress`);
  - `crates/am-app/tests/accounts_tty.rs` (the real binary: a piped-stdin refusal, and a `^C` on a pty through `nix::pty::openpty`, checking `ECHO` through `tcgetattr`; HOME and XDG set as in `audit_cli.rs`);
  - `crates/am-config/tests/create.rs`;
  - `crates/am-store/tests/config_history.rs` (create, and audit carried with a commit);
  - `xtask/tests/` or `crates/am-app/tests/release_deps.rs` (the guard).

## Tasks & Acceptance

**Execution:**
- [ ] Tests first (no `src/` change): every matrix row, plus `FakeTerminal` in am-testkit. am-testkit's `src/` is a `src/` folder too, so `FakeTerminal` is written after the red run, which then counts the missing type as its compile error. Log the red run.
- [ ] `crates/am-domain`, `crates/am-store` — `HistoryKind::Create`, the audit carried with a commit.
- [ ] `crates/am-config/src/writer.rs` — `create`.
- [ ] `Cargo.toml`, `crates/am-app/Cargo.toml`, `xtask/src/deps.rs` (if needed), `docs/crate-notes/nix.md` — the nix pin.
- [ ] `crates/am-app/src/{terminal,cli,main,runtime}.rs` — the commands.
- [ ] `crates/am-testkit/src/lib.rs` — `FakeTerminal`.
- [ ] `docs/` — `agentic accounts` usage in the CLI docs, if a CLI page exists.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the outcome, wording and side effects match.
- Given the workspace, when `just check` runs, then it is green, including check-deps, check-forbid, the denylist and the supply-chain check for nix.

## Implementation Notes

### Red run (2026-11-22, tests first; no `src/` file created or changed)

Tests written:
- `crates/am-app/tests/accounts_cli.rs` (new, in process, 20 tests): Add (and Missing file), add to an existing file, Not a terminal (in process), Wrong name, the 2-minute timeout, Interrupt (in process), Reserved or duplicate, a non-Anthropic provider, Reuse key, Write fails (the key this add stored is deleted, a reused one stays), List, Test (pinned policy, `AuthRef`, `check_url` outside the pin), Test without a key or of an unknown id, Remove unreferenced (History, Undo through the writer, keychain entry kept), Remove referenced/reserved/unknown, Busy (store lock held, names the holder), Busy (outside change waiting for Reload, names the file), a stdio MCP server enabled, the read-only resolve (no mint, no legacy move, no store migration), bad usage.
- `crates/am-app/tests/accounts_tty.rs` (new, real binary): piped and file-redirected stdin refused naming the terminal rule (unix); typed `^C` and SIGINT at the key prompt on a `nix::pty::openpty` pty, ECHO (and ICANON, ISIG) set again via `tcgetattr`, nothing written or stored (macOS only: the key prompt is reached only where the Keychain exists, AD-47).
- `crates/am-config/tests/create.rs` (new, 11 tests): `ConfigCreate` writes a missing file as a `create` entry, makes missing parent folders, refuses an existing file and one that appears meanwhile (kept), refuses a volume without hard links in words, Undo of a create refused in words ("never deletes"), bad paths and a linked parent, an added error, a secret-looking value, crash after/before placing with `reconcile`.
- `crates/am-store/tests/config_history.rs` (4 new tests): `HistoryKind::Create` (`create`, `config.created`, no ref, empty before), audit notes carried with a commit (written after the `config.*` entry in the commit, none before, none on drop), written on reconcile, empty notes refused.
- `xtask/tests/release_deps.rs` (new, the guard): live `cargo metadata --no-deps --offline`; am-testkit is not reached from am-app through normal, build or optional edges, no linked crate's feature enables it, am-app/src never names `am_testkit`; a self-check on a synthetic graph.
- Rewritten older tests: the `NewIntent` literals in `crates/am-store/tests/config_history.rs` (`intent`, `outside`) and `crates/am-app/tests/serve_project.rs` gain `audit: Vec::new()`.
- `Cargo.toml` pins `nix = "=0.31.4"` (`default-features = false`, `features = ["term"]`, Stack) and `crates/am-app/Cargo.toml` takes it as a dev-dependency for the pty test (`Cargo.lock` gains the edge). The normal dependency, check-deps and `docs/crate-notes/nix.md` remain for the implementation step.

Command: `cargo nextest run -p am-app -p am-config -p am-store -p am-domain -p xtask --no-fail-fast` (exit 101, does not compile):

```
error[E0432]: unresolved import `am_domain::AuditNote`
 --> crates/am-store/tests/config_history.rs:7:24
error[E0560]: struct `NewIntent` has no field named `audit`
  --> crates/am-store/tests/config_history.rs:47:9
error[E0599]: no variant, associated function, or constant named `Create` found for enum `HistoryKind` in the current scope
   --> crates/am-store/tests/config_history.rs:516:28
error[E0609]: no field `audit` on type `NewIntent`
   --> crates/am-store/tests/config_history.rs:632:34
error[E0432]: unresolved import `am_config::ConfigCreate`
  --> crates/am-config/tests/create.rs:22:17
error[E0599]: no method named `create` found for struct `ConfigWriter` in the current scope
   --> crates/am-config/tests/create.rs:100:19
error[E0599]: no variant, associated function, or constant named `NoHardLinks` found for enum `FailPoint` in the current scope
   --> crates/am-config/tests/create.rs:252:31
error: could not compile `am-store` (test "config_history") due to 11 previous errors
error: could not compile `am-config` (test "create") due to 20 previous errors
```

Command: `cargo nextest run -p am-app --test accounts_cli` (exit 101, does not compile; `FakeTerminal` is the missing am-testkit type the plan expects):

```
error[E0432]: unresolved imports `am_app::cli::AccountsContext`, `am_app::cli::accounts`
  --> crates/am-app/tests/accounts_cli.rs:19:19
error[E0432]: unresolved import `am_testkit::FakeTerminal`
  --> crates/am-app/tests/accounts_cli.rs:28:68
error[E0599]: no variant, associated function, or constant named `Create` found for enum `HistoryKind` in the current scope
   --> crates/am-app/tests/accounts_cli.rs:363:46
error: could not compile `am-app` (test "accounts_cli") due to 3 previous errors
```

Command: `cargo nextest run -p am-app --test accounts_tty --no-fail-fast` (exit 100, compiles and fails: the binary has no `accounts` command yet):

```
FAIL am-app::accounts_tty add_and_remove_with_stdin_that_is_not_a_terminal_are_refused
  panicked at crates/am-app/tests/accounts_tty.rs:128:9: ["add", "tty-test-…", "--provider", "anthropic"]: usage: agentic serve [--project <path>] | agentic validate [<project>] | …
FAIL am-app::accounts_tty pty::typed_ctrl_c_at_the_key_prompt_restores_echo_and_stores_nothing
  panicked at crates/am-app/tests/accounts_tty.rs:189:21: exited (exit status: 2) before the name prompt; terminal: usage: agentic serve …
FAIL am-app::accounts_tty pty::sigint_at_the_key_prompt_restores_echo_and_stores_nothing
  panicked at crates/am-app/tests/accounts_tty.rs:189:21: exited (exit status: 2) before the name prompt; terminal: usage: agentic serve …
Summary 3 tests run: 0 passed, 3 failed
```

`cargo nextest run -p xtask --test release_deps`: 3 passed. It is a regression guard: am-testkit is not linked into the binary today; its synthetic-graph self-check shows it fails on a normal, build or optional edge.

Test-design choices the implementation must meet (owner standing rules: field names, test structure, wording):
- **CLI:** `agentic accounts add <id> --provider anthropic [--name <text>] [--project <folder>]`, `remove <id>`, `list`, `test <id> [--model <m>]`; `--project` defaults to the current folder; bad usage exits 2 with `usage: agentic accounts` on stderr; refusals exit non-zero.
- **In process:** `pub async fn am_app::cli::accounts(args: &[String], ctx: AccountsContext, out: &mut dyn Write, err: &mut dyn Write) -> i32` (async because `test` streams); `AccountsContext { places: Places, identity: Arc<dyn ProjectIdentity>, terminal: Box<dyn Terminal>, secrets: Arc<dyn SecretStore>, egress: Arc<dyn Fn(EgressPolicy) -> Arc<dyn EgressClient> + Send + Sync> }`. `identity` is added to the plan's list because AD-32 has the identity adapter supply the store lock's host and the read-only resolve's checks.
- **FakeTerminal** (am-testkit, `Clone` with shared state): `new()` (stdin and stdout a terminal, echo on), `piped()`, `line(text)`, `interrupt()`, `time_out()`, `echo() -> bool`, `echo_reads() -> Vec<bool>`, `timeouts() -> Vec<Option<Duration>>` (the id is read with `Some(120 s)`), `reads() -> usize`, `shown() -> String`; an exhausted script reads as end of input. Because am-testkit cannot depend on am-app (a cycle), the `Terminal` trait must live where am-testkit can implement it (an am-domain port); am-app's `terminal.rs` keeps the real nix implementation.
- **ConfigCreate:** `ConfigWriter::create(actor, ConfigCreate { file, bytes, audit: Vec<AuditNote> }) -> Result<HistoryEntry, WriteError>` (a typed command like `ConfigEdit`, in place of the plan's `create(actor, file, bytes)`, so it can carry the audit notes); refusal words contain "already exists", "hard link" and, for Undo, "never deletes"; test-only `FailPoint::NoHardLinks`; `FailPoint::BeforeRename`/`AfterRename` also stop a create before/after the file is placed.
- **Audit notes:** `am_domain::AuditNote { action: String, target: String, args: BTreeMap<String, String> }` and `NewIntent.audit: Vec<AuditNote>`, kept on the pending intent and written in the commit's (or reconcile's) transaction right after its `config.*` entry; an empty action or target is refused. The add's note is action `config.widened`, target the Account id, args `kind: account_added`, `file`, `account`, `scheme`, `host`, `port` (`443`), `references` (naming the seats) and `confirmation: typed_id`.
- **Words the tests read:** "terminal", "2 minutes", "reus…" (reuse), "Keychain" (remove keeps the entry), "no key in Keychain", the masked key as `•` plus the last 4 characters, "stdio MCP server", "project_id", "old runtime folder", "migrat…", the lock holder's host, the changed file's path, and the seats a reference names.

### Test fixes after the red run (2026-11-22, authorised by the coordinator under the standing rules: test structure, with the cause recorded)

The red run could not show these: `accounts_cli.rs` did not compile then, and the pty tests stopped at the missing command. No assertion changed meaning.

- **`crates/am-app/tests/accounts_cli.rs`, `Env::history`.** Cause: the tests' own setup (`Env::project` → `open_project`) mints `project_id`, which always leaves one `company.yaml` History entry by `System(runtime)` (AD-32), so `history.is_empty()` and `history.len() == 1` could not hold whatever the command did (13 tests, `list` and `test` included). Fix: the helper leaves that setup entry out (`!matches!(&e.actor, Actor::System { component } if component == "runtime")`). No assertion changed.
- **`crates/am-app/tests/accounts_cli.rs`, the marker assertion.** Cause: the literal `".agentic-open"` is refused by `cargo xtask runtime-paths` (AD-32) and failed `xtask::runtime_paths the_repository_is_clean`. Fix: `am_domain::runtime::open_marker(&root)`.
- **`crates/am-app/tests/accounts_tty.rs`, the two `pty::` tests.** Cause: with HOME pointed at a temporary folder (so the binary finds the temporary runtime base) macOS finds no default keychain ("A default keychain could not be found"); the add must read the Keychain before the key prompt (reuse, never overwrite), so the binary rightly refused before echo was turned off. Fix, in the test setup only: the temporary HOME gets its own keychain through `/usr/bin/security` run with that HOME (`create-keychain -p ""`, `default-keychain -s`, `unlock-keychain -p ""` on `<home>/Library/Keychains/test.keychain-db`), apart from the login keychain; a failing step fails the test with `security`'s output. `assert_nothing_stored` now looks in that same keychain (`security find-generic-password -s agentic-manager -a <id>` must fail saying the item could not be found) instead of `KeychainStore::new()` in the test process, which reads the real login keychain. Every ECHO, ICANON, ISIG and exit-code assertion is unchanged.
- **Wording, `crates/am-app/src/accounts.rs`.** The outside-change refusal told the owner to "open the project in the app and choose Reload", but no app UI exists before epic 5. It now says the file changed outside the app since the app last wrote it, that nothing was read, written or stored, to put the file back as the app last wrote it and run the command again, and that Reload arrives with the app's UI. No test pinned the old words (the test reads only the file's path).

### Implementation decisions (2026-11-22, engineering choices under `docs/owner-standing-rules.md`)

- **Store schema 5.** `config_intents` gains `audit_notes` (migration `0005_config_intent_audit.sql`): an intent's audit notes must survive a crash to be written when it is reconciled, so they are kept on the intent row; History rows hold none.
- **Reconcile at open.** `agentic accounts add` and `remove` settle config writes a crash interrupted when they open the store as the one writer, as `agentic serve` does (History and audit in the store only); otherwise an interrupted add would later read as an outside change. The open itself (`runtime::open_project_unchanged`, `am_store::open_runtime_unchanged`) mints, records, moves, creates, backs up and migrates nothing.
- **`list` and `test` take no lock** and open no store: they read `models/accounts.yaml` through am-config and the Keychain only, so they run while `agentic serve` holds the project.
- **Keychain entry name = `secret_ref`**, falling back to the Account id (`list`, `test`); `add` writes `secret_ref: <id>` and stores under the id, so the two are equal for every Account the CLI adds (AD-17). *Superseded by the review fixes below: the entry is always the Account id.*
- **Exit 130** after Ctrl-C at a prompt (typed `^C` or SIGINT); 2 for bad usage; 1 for every other refusal or failure.
- **Default model of `test`:** the repo's existing spike default (`am_app::DEFAULT_ANTHROPIC_MODEL`, also used by `agentic spike c1`); `--model` overrides it. One short request, 64 output tokens.
- **The key is kept when the file was placed.** "If the write fails, delete the keychain entry this command created" is applied when `models/accounts.yaml` does not hold the Account afterwards. When the writer failed after placing the file (the Account is in the file, its History entry pending for the next start's reconcile), the key stays and the refusal says so.
- **Check order of `add` and `remove`:** usage, the terminal, the provider (`add`), no stdio MCP server or rendering Source, the read-only open under the store's lock, no outside change, then the id; nothing is read from the terminal, written or stored before all passed.
- **Layout:** the command is `crates/am-app/src/accounts.rs` (re-exported from `cli`); the `Terminal` port is `am_domain::terminal`; the real one is `am_app::terminal::OwnerTerminal`, whose `interactive()` is the one TTY predicate (`TerminalPrompt` uses it too); am-config's reads and patches for the edit are in `account_edit.rs` (`accounts_file`, `account_uses`, `add_account_edit`, `remove_account_edit`, `new_accounts_file`, `outside_changes`); `ConfigWriter::apply_with_audit` carries the notes for an edit, `ConfigCreate.audit` for a create. nix is a `cfg(unix)` dependency of am-app.
- **SIGINT** is taken through `tokio::signal` (nix's `sigaction` is unsafe), installed before echo is turned off; it ends a read in flight as an interrupt, so the drop guard restores the terminal. Once installed, a SIGINT outside a read (the short store-and-write window after the key) is not acted on. *Superseded by the review fixes below: SIGTERM, SIGHUP and SIGQUIT are watched too, and a pending interrupt is checked before the key is stored and before the file is written.*
- **Docs:** no CLI page exists under `docs/`, so none was written; `docs/crate-notes/nix.md` is new.

### Review fixes: red run (2026-11-22, tests first; no `src/` file changed yet)

Tests written or extended for the coordinator's review findings:
- `crates/am-store/tests/config_history.rs`: `config_history_reconcile_writes_the_intents_audit_notes` now asserts the note's actor is the intent's (`Human owner/Reviewer`) while the `config.reconciled` entry is the system's.
- `crates/am-config/tests/account_edit.rs` (new, 2 tests): `AccountsFile::sha256` names the bytes the read was made on; `apply_with_audit(actor, edit, before_sha256, audit)` refuses an edit built on other bytes as `ChangedOnDisk`, writing nothing, and applies one built on the file's bytes.
- `crates/am-app/tests/accounts_cli.rs` (5 new tests, 2 extended; `Kit::run_with` takes a counting `SecretStore`): the file changing while the add waits (list present, and `accounts:` absent) is never overwritten and no key is stored; a seat that starts naming the id while the add waits refuses the add; a `secret_ref` naming another entry is never read (`list`) or sent (`test`: no Keychain read, no request); a refused add still prints the reconcile report and no refusal claims that nothing was written; Ctrl-C after the id on the key-reuse path exits 130 and adds nothing; `test`'s missing-key advice and `remove`'s History words say what is true today.
- `crates/am-app/tests/accounts_tty.rs`: `pty::sigterm_at_the_key_prompt_restores_echo_and_stores_nothing`.
- `xtask/tests/release_deps.rs` (2 new tests): a source scan for `Terminal` implementations outside am-testkit (only `OwnerTerminal`'s in `crates/am-app/src/terminal.rs`, under no cargo feature, and `main.rs` building the context with `OwnerTerminal::new()`), with a self-check on a synthetic tree.
- `crates/am-config/src/writer.rs` `place` (fsync of the parent of each folder a create makes): durability after a power loss cannot be observed by a test here; `create_makes_missing_parent_folders_inside_the_project` keeps covering the behaviour. No new test.

Command: `cargo nextest run -p am-store --test config_history reconcile_writes` (exit 100):

```
FAIL am-store::config_history config_history_reconcile_writes_the_intents_audit_notes
  panicked at crates/am-store/tests/config_history.rs:712:5:
  assertion `left == right` failed: the note keeps the Actor of the intent
    left: Object {"component": String("config-writer"), "kind": String("system")}
   right: Object {"hat": String("Reviewer"), "kind": String("human"), "person": String("owner")}
```

Command: `cargo nextest run -p am-config --test account_edit` (exit 101, does not compile):

```
error[E0609]: no field `sha256` on type `AccountsFile`
  --> crates/am-config/tests/account_edit.rs:40:29
error[E0061]: this method takes 3 arguments but 4 arguments were supplied
   --> crates/am-config/tests/account_edit.rs:67:10
 67 |         .apply_with_audit(owner(), add, &built_on, Vec::new())
```

Command: `cargo nextest run -p am-app --test accounts_cli` (exit 101, does not compile; `FakeTerminal::line_and` is the missing am-testkit method, written next as the first red run did for `FakeTerminal`):

```
error[E0599]: no method named `line_and` found for struct `FakeTerminal` in the current scope
    --> crates/am-app/tests/accounts_cli.rs:1136:18
error: could not compile `am-app` (test "accounts_cli") due to 2 previous errors
```

Command: `cargo nextest run -p am-app --test accounts_tty sigterm` (exit 100):

```
FAIL am-app::accounts_tty pty::sigterm_at_the_key_prompt_restores_echo_and_stores_nothing
  panicked at crates/am-app/tests/accounts_tty.rs:380:9:
  ECHO is set again; terminal: … Key for 'tty-test-27503-1795303067136' (not shown as you type or paste it):
```

`cargo nextest run -p xtask --test release_deps`: 5 passed. The two new tests are a regression guard: the sources are clean today; the synthetic-tree self-check shows the scan fails on a second `Terminal` implementation (in am-app or another crate, plain or generic), on a feature-gated real one, on a gated `mod terminal` and on a `main.rs` built with another terminal.

With `FakeTerminal::line_and` added to am-testkit (the test kit only; no am-app, am-config, am-store or am-domain `src/` change yet): `cargo nextest run -p am-app --test accounts_cli --no-fail-fast` (exit 100, 25 run: 18 passed, 7 failed):

```
FAIL a_secret_ref_that_names_another_entry_is_never_read_or_sent
  panicked at accounts_cli.rs:1215:5: cloud  anthropic  api.anthropic.com  ••••••••QRST
FAIL a_seat_that_starts_naming_the_account_while_add_waits_refuses_the_add
  panicked at accounts_cli.rs:1183:5: assertion `left != right` failed: agentic: added the Account 'cloud2' …
FAIL an_account_added_to_the_file_while_add_waits_is_never_overwritten
  panicked at accounts_cli.rs:1145:9: assertion `left != right` failed: agentic: added the Account 'cloud2' …
FAIL a_refused_add_still_reports_the_interrupted_write_it_completed
  panicked at accounts_cli.rs:1290:9: the refusal claims `othing was written`: … 'local-ollama' is a reserved local Account: … nothing was written or stored
FAIL ctrl_c_after_the_id_on_the_reuse_path_adds_nothing
  panicked at accounts_cli.rs:1314:5: assertion `left == right` failed: agentic: added the Account 'cloud2' … it reuses the key the Keychain already held
FAIL test_without_a_key_or_of_an_unknown_id_sends_nothing
  panicked at accounts_cli.rs:811:5: advice that cannot be followed: … `agentic accounts remove cloud2` then `agentic accounts add cloud2 --provider anthropic` stores one
FAIL remove_of_an_unreferenced_account_has_history_and_undo
  panicked at accounts_cli.rs:850:5: … (History entry …; Undo is in the app's History). …
```

### Review fixes: what changed (2026-11-22, after the red run above)

- **Stale read (add, remove).** `AccountsFile.sha256` names the bytes a read saw. Once the last prompt has returned and before the key is stored, `add` runs its checks again on a fresh read (no owner-uid child enabled, no outside change, the file's bytes and the id, what names the id); a difference refuses with nothing stored and says to run the command again. `ConfigWriter::apply_with_audit(actor, edit, before_sha256, audit)` applies an edit only while the file holds the bytes it was built on (`ChangedOnDisk` otherwise); `add` and `remove` both use it. A file that appears where none was is still refused by the create's hard link.
- **Keychain entry = the Account id, always** (AD-17). A `secret_ref` naming another entry is never followed: `list` prints "secret_ref names another entry; not used" and reads no key for that Account; `test` refuses before any Keychain read or request.
- **Audit notes keep the intent's Actor** on commit and on reconcile (am-store): an interrupted add's `account_added` entry names the owner who confirmed it, while `config.reconciled` stays the system's.
- **Reconcile report and refusal words.** The open's report lines are printed inside the open, before any later refusal. Every refusal of `add`/`remove` says what the command did not do ("No Account was added and no key was read or stored", "… no key was stored" once the key was typed, "No Account was removed"), never that nothing was written.
- **Guard by source** (`xtask/tests/release_deps.rs`): outside am-testkit the only `Terminal` implementations are `OwnerTerminal`'s in `crates/am-app/src/terminal.rs`, under no cargo feature, its module under no cfg, and `main.rs` builds the context once with `OwnerTerminal::new()`.
- **Pending interrupt.** `Terminal::interrupted()` reports and consumes an interrupt no read has reported. `add` asks it before the key is stored and again before the file is written (a key this command just stored is deleted); `remove` asks it before the write. Exit 130.
- **Signals.** `OwnerTerminal` watches SIGINT, SIGTERM, SIGHUP and SIGQUIT, all installed before echo is turned off; each ends the read, so the drop guard restores the terminal before the process exits non-zero.
- **Words true today.** `test`'s missing-key message no longer offers remove-then-add as if it always worked (it says re-adding needs the Account unreferenced first, or the app's Account card when it ships); `remove` says the change is recorded in History and can be undone from the app's History when the UI ships.
- **Durability.** A create fsyncs the parent of each folder it makes.

Tests run after the fixes (only the crates edited): `cargo nextest run -p am-app --test accounts_cli --test accounts_tty` 29 passed; `-p am-config -p am-store -p am-domain -p am-testkit` 359 passed; `-p xtask --test release_deps` 5 passed; fmt and clippy `-D warnings` on those crates clean. The full `just check` is the coordinator's.

### Second review: red run (2026-11-22, tests first)

Tests written or changed:
- `crates/am-app/tests/accounts_cli.rs` (4 new tests): a Keychain entry that appears while the key prompt waits is kept, used and never deleted (also when the write then fails); the same id added to the file from outside between the last check and the write is not taken for this command's write (no pending History entry claimed, the key this command stored deleted); Ctrl-C between the store and the write leaves no key and no Account, exit 130; a remove whose write fails after the file was replaced says the Account was removed from the file and that its History entry is recorded at the next open.
- `crates/am-store/tests/config_history.rs`: `config_history_commit_writes_the_intents_audit_notes_with_the_entry` now begins with the Reviewer and commits with another Actor (the Owner hat), asserts the `config.created` entry names the commit's Actor and the note the intent's, and its comment says so. This is a test-only change: the behaviour was fixed in the first review round, so the test passes as written (`1 passed`); before that round's fix it would have failed, which the reconcile test's red run above showed for the same code path.

Command: `cargo nextest run -p am-app --test accounts_cli` (exit 101, does not compile; both missing items are test scaffolding, written next: `FakeTerminal::interrupt_on_check` in am-testkit and `runtime::testing::fail_writer_at` behind am-app's `test-support` feature):

```
error[E0425]: cannot find function `fail_writer_at` in module `am_app::runtime::testing`
    --> crates/am-app/tests/accounts_cli.rs:1513:31
error[E0599]: no method named `interrupt_on_check` found for struct `FakeTerminal` in the current scope
    --> crates/am-app/tests/accounts_cli.rs:1485:14
error: could not compile `am-app` (test "accounts_cli") due to 3 previous errors
```

With that scaffolding in place and `crates/am-app/src/accounts.rs` unchanged: `cargo nextest run -p am-app --test accounts_cli --no-fail-fast` (exit 100, 29 run: 26 passed, 3 failed):

```
FAIL a_remove_that_fails_after_the_file_was_written_says_it_was_removed
  panicked at accounts_cli.rs:1522:5: it is gone from the file: agentic: the Account 'spare' was not removed: the write stopped: simulated crash after the rename
FAIL a_keychain_entry_that_appears_during_the_key_prompt_is_kept
  panicked at accounts_cli.rs:1380:5: assertion `left == right` failed: the entry that appeared is not overwritten
    left: Some("sk-ant-canary-0a1b2c3d4e5f-WXYZ")   right: Some("sk-ant-stored-by-another-process-7777")
FAIL the_same_id_added_from_outside_before_the_write_is_not_taken_for_this_write
  panicked at accounts_cli.rs:1462:5: no pending History entry is claimed: agentic: the Account 'cloud2' is in models/accounts.yaml, but the write did not finish: … Its History entry is recorded the next time the project opens; its key is in the Keychain
```

`ctrl_c_between_the_store_and_the_write_leaves_no_key_and_no_account` passes once the fake can script it (`interrupt_on_check(2)`): the second check and its key deletion were already in `add`; the test now covers them.

### Second review: what changed (2026-11-22, after the red run above)

- **Keychain read again right before the store** (`add`): the key prompt has no time limit, so an entry another process stored meanwhile is found, kept and used; the typed key is discarded, the add says so, and since this command did not create the entry it is never deleted afterwards.
- **"Written, History pending" is decided by bytes** (`add`, `remove`): after a writer error the command compares the file's sha256 with the after-sha256 of its own pending intent in the store (`Editing::written_pending`). Only on a match does it say the Account was written (or removed) and that its History entry is recorded at the next open. Otherwise it is a refusal, and a key this command created is deleted; that the id is, or is not, in the file decides nothing. When the store cannot be read to tell, the command says so and keeps the key.
- **`remove` after a late failure** says the Account was removed from the file and that its History entry is recorded the next time the project opens.
- **`ChangedOnDisk` from the writer** is worded for this command (the file changed after the last check; run the command again) instead of the writer's Reload words.
- **Test scaffolding:** `FakeTerminal::interrupt_on_check(n)` (am-testkit) scripts a Ctrl-C for the n-th `interrupted()` call; `am_app::runtime::testing::fail_writer_at(root, point)` (am-app's `test-support` feature only, keyed by project root) stops the writer that `open_project_unchanged` builds, as a crash would.
- **am-store test only:** the commit-path note test commits with another Actor than the intent's and asserts both entries' actors.

Tests run after the fixes (only the crates edited): `cargo nextest run -p am-app -p am-testkit` and `-p am-store --test config_history`, all passed; fmt and clippy `-D warnings` on those crates clean. The full `just check` is the coordinator's.

## Plan Change Log

## Review Triage Log

### 2026-11-22 — Review pass
- verdicts: 10 findings — high 2, medium 4, low 4, false 0, maybe-false 0 (plus one coordinator finding, listed last)
- findings:
  - `[high]` `[patch]` The add's edit is built from a read made before the prompts (2 minutes for the id; no limit for the key), then applied by list index with no re-check. An Account added to the file meanwhile is overwritten (or the whole list is, when `accounts:` was absent), and the uses and outside-change checks are stale. Fix: after the last prompt and before the key is stored, every check runs again on a fresh read, and a file that changed refuses with nothing stored; the writer applies the edit only if the file still holds the bytes it was built on.
  - `[high]` `[patch]` `list` and `test` take the Keychain entry name from `secret_ref`, so a hand-edited `models/accounts.yaml` can make `test` send any `agentic-manager` Keychain entry to the provider, and `list` show its last 4 characters. AD-17 names the entry by the Account id. Fix: the entry is always the Account id; an Account whose `secret_ref` differs from its id is reported as such, and no key is read or sent for it. Test with a differing `secret_ref`.
  - `[medium]` `[patch]` When an interrupted add is reconciled, its `account_added` audit entry is written as System (the reconciler), losing who confirmed the widening. Fix: notes carried by an intent are audited with the intent's own actor on commit and on reconcile; the reconcile test asserts the actor.
  - `[medium]` `[patch]` The open reconciles pending writes (History and audit rows) and removes temp files before the remaining checks, and a later refusal says nothing was written; on a refusal the reconcile report is not printed. Reconciling first is AD-16's startup rule and stays (a crashed add would otherwise read as an outside change). Fix: the reconcile report is always printed, and each refusal says what this command did not do (no Account added or removed, no key read or stored) and no longer claims that nothing was written.
  - `[medium]` `[patch]` The release guard knows only the name `am-testkit`; a second `impl Terminal` in a linked crate, or one behind a feature, would pass. Fix: a guard test asserts that the only `Terminal` implementations under `crates/*/src` outside am-testkit are `OwnerTerminal`'s in `crates/am-app/src/terminal.rs`, and that none sits behind a `cfg(feature …)`.
  - `[low]` `[reject]` `test` builds its client with no audit sink and no ledger entry. The ticket asks for "a first EgressClient that admits only the Account's pinned host"; auditing every blocked call is story 2.2's (R5) and the ledger is epic 3's, and `test` opens no store so that it works while `serve` holds the lock. Recorded under residual risks.
  - `[medium]` `[patch]` After the id is typed, a Ctrl-C (or SIGINT) is acted on only during a read, so on the key-reuse path the Account is added although the owner pressed Ctrl-C, and a Keychain dialog cannot be cancelled. Fix: the port reports a pending interrupt, and the add checks it before the key is stored and before the file is written.
  - `[low]` `[patch]` Two messages give advice that cannot be followed: `test` says to remove and re-add an Account that `remove` refuses while a seat names it, and `remove` points to History in an app that does not exist yet. Fix: both reworded to what is true today.
  - `[low]` `[patch]` When a create makes a parent folder, that folder's own entry is not made durable (its parent is not fsynced). Fix: fsync the parent of each folder the create makes.
  - `[low]` `[reject]` The `Terminal` port takes `&mut self` and is held as `Box<dyn Terminal>`, not `Arc<dyn Port>`. A terminal has one owner and is never shared between tasks, the red-run tests fix this shape, and no caller is harmed; recorded here as a deviation from the AGENTS.md convention.
  - `[medium]` `[patch]` (coordinator, from reading `terminal.rs`) SIGTERM and SIGHUP during the key prompt kill the process with echo off; the owner's rule is every exit path. Fix: the watcher takes SIGTERM, SIGHUP and SIGQUIT as it takes SIGINT, so the guard restores the terminal before the process exits; a pty test sends SIGTERM at the key prompt.

### 2026-11-22 — Follow-up review pass (the nine patches above)
- verdicts: 6 findings — high 0, medium 1, low 4, false 1, maybe-false 0
- findings:
  - `[medium]` `[patch]` The Keychain is read once before the key prompt (no time limit) and never again; an entry stored meanwhile by another process (the same id in another project) is overwritten by `set`, and deleted if this add's write then fails. Fix: the entry is read again right before `set`; one that appeared is kept and reused, never overwritten or deleted.
  - `[low]` `[patch]` `remove` prints "was not removed" for every writer error, though a failure after the rename leaves the Account already gone with its intent pending. Fix: it re-reads the file as `add` does and says which happened.
  - `[low]` `[patch]` After a failed write, `add` takes "the id is in the file" as proof that its own write landed; an outside edit adding the same id gives the same answer, so the message is false and the stored key is kept. Fix: the file's sha256 is compared with the bytes the command meant to write.
  - `[low]` `[patch]` The second interrupt check in `add` (which deletes a key just stored) had no test, because the fake hands a scripted interrupt to the first check. Fix: the fake can script an interrupt for a later check, and one test covers it.
    - The fake's echo flag cannot prove real echo; the pty tests do. Left as it is.
    - `remove`'s check never fires on a real terminal, because `remove` reads nothing; a signal there ends the process with the terminal untouched.
  - `[low]` `[patch]` The commit-path audit-note test used one actor for begin and commit. Fix: different actors, and the doc comment corrected.
  - (coordinator, from the reviewer's "checked" notes) The source guard would not catch an environment read inside the terminal check. Fix applied: `xtask/tests/release_deps.rs` refuses any `env::` or `env!(` in the real terminal's code, with a self-test.
  - `[false]` `[reject]` `crates/am-config/tests/account_edit.rs` is untracked while other new files are staged. Nothing is staged for commit selectively: the story's commit adds every file (`git add -A`), and the finalize step checks the tree is clean.

## Design Notes

Decided under `docs/owner-standing-rules.md` (engineering choices, recorded):
- **^C as a byte:** reading the key with `ICANON` and `ISIG` off makes a typed `^C` an ordinary byte that cancels. No signal is needed for the common case, and a SIGINT handler still covers `kill -INT`.
- **Interrupt test on a pty:** the test drives the real binary on a pseudo-terminal (`nix::pty`, feature `term`), so it proves the terminal's real `ECHO` flag, not a fake's.
- **Grant set:** "Updates an existing approved grant set": no grant-set store exists before epic 4 story 18, so an add has nothing to update. This is recorded, not built.
- **Mask:** dots plus the last 4 characters, from the draft first-run mockup. EXPERIENCE.md says only "masked".

## Verification

**Commands:**
- `cargo nextest run -p am-app -p am-config -p am-store -p am-domain -p xtask` -- expected: all pass.
- `just check` -- expected: green.

## Auto Run Result

Status: built (2026-11-22). The first run halted in planning (the ticket against AD-15). The owner chose the interim CLI exception with conditions and accepted `nix`; the replan landed on main as `dca4013`. This run restarted from that baseline.

**Summary:** `agentic accounts add|list|test|remove` in am-app.
- **Add and remove (AD-15's interim CLI Account edit):**
  - They run only when stdin and stdout are a real terminal, and refuse otherwise before anything is read.
  - They resolve the project read-only, hold the store lock, and refuse while an MCP server or rendering Source is enabled, or an outside change is pending.
- **Add:**
  - It names the Account, its provider, its endpoint and what already names it, then requires the id typed within 2 minutes.
  - It re-checks everything on a fresh read, then reads the key with echo off (`nix` termios).
  - It stores the key only in the Keychain, under the Account id. An existing entry is reused and never overwritten.
  - It writes `models/accounts.yaml`, creating it with the new `ConfigCreate` when it is missing.
  - The History entry and an `account_added` audit note are written in one transaction, with the owner as actor (after a reconcile too).
- **Echo:** restored on every way out: Return, a typed Ctrl-C, SIGINT, SIGTERM, SIGHUP, SIGQUIT, an error or a panic.
- **List and test:**
  - `list` masks keys.
  - `test` streams a reply through a client that admits only the Account's host.
  - Both read only the Keychain entry named by the Account id.

**Files changed:**
- `crates/am-domain`: `HistoryKind::Create`, `AuditNote`, `NewIntent.audit`, the `Terminal` port.
- `crates/am-store`: migration `0005` (audit notes on intents), `config.created`, notes audited with the intent's actor, `open_runtime_unchanged`.
- `crates/am-config`: `ConfigCreate` and `ConfigWriter::create` (hard-link placement), `apply_with_audit` with an expected before-sha256, `account_edit.rs`.
- `crates/am-app`: `accounts.rs`, `terminal.rs` (the real terminal and the one TTY predicate), `runtime::open_project_unchanged`, `main.rs` dispatch.
- `crates/am-testkit`: `FakeTerminal` (tests only).
- `Cargo.toml`, `Cargo.lock`, `docs/crate-notes/nix.md`: nix =0.31.4, feature `term`.
- Tests:
  - `crates/am-app/tests/accounts_cli.rs` (29);
  - `accounts_tty.rs` (the real binary: non-terminal refusals; on a pty with a temporary keychain, Ctrl-C, SIGINT and SIGTERM);
  - `crates/am-config/tests/{create,account_edit}.rs`;
  - `crates/am-store/tests/config_history.rs`;
  - `xtask/tests/release_deps.rs`.

**Review:** two quick passes; the Review Triage Log has every row.
- Pass 1, 10 findings plus one coordinator finding: 9 patched (2 high, 5 medium, 2 low), 2 low rejected:
  - `test` has no audit sink or ledger, which belongs to story 2.2 and epic 3;
  - the port is held as `Box<dyn Terminal>`.
- Pass 2 (follow-up, because pass 1 patched two highs), 6 findings: 5 patched (1 medium, 4 low), 1 false. One coordinator addition: the guard refuses environment reads in the terminal code.
- Nothing deferred.
- Follow-up recommended: false. Pass 2 patched no high.

**Verification:**
- `just check` passes (589 tests, 7 skipped).
- Every matrix row has a named test that ran and passed.
- The red run was logged before any `src/` change, and each review fix has its own red run where behaviour changed.
- Three red-run test defects were fixed, with causes recorded: the setup's own History entry, a path literal, and no Keychain under a temporary home.
- The login Keychain is untouched by the tests.

**Residual risks:**
- **Not exercised:** a live call to Anthropic, and an add against the real login Keychain. Both wait for story 5's redaction layer and leak scan, as the ticket says.
- **`accounts test`** writes no audit entry and no ledger line (story 2.2; epic 3).
- **Hand edits can lock the owner out:** a hand edit of a file the app has written (`company.yaml` after its mint, `models/accounts.yaml` after an add) refuses add and remove until the file is put back. No Reload exists before the UI (AD-15 as amended).
- **No key on this Mac:** a referenced Account whose key is missing on this Mac has no CLI way to get one.
- **Keychain entries are shared:** they are named by Account id only, across projects (the architect's open question, due before 5.5).
- **Ignored interrupt:** an interrupt during the final file write is not acted on.
- **macOS-only pty tests:** the Linux job runs the non-terminal refusals only.
