---
title: 'Accounts belong to this Mac'
type: feature
ticket: 12
created: '2026-11-22'
status: built
baseline_revision: '955050b3f87babd2649349a1e3feb34cc29e3261'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick, adversarial-accounts]
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
warnings: [oversized]
deferred:
  - summary: >-
      The app data dir comes from $HOME (XDG_DATA_HOME off macOS), so a process running as the owner can point `agentic accounts test` at a forged user file and store and have the real Keychain key sent elsewhere.
    evidence: |-
      Story 2.12 adversarial review, finding 4. Places::from_env predates this story. Forging needs a same-user process writing a matching file, History and Human note, which AD-15 places outside the threat model (owner, 2026-11-22). Hardening (resolving the home from the passwd entry, not $HOME) would also move the binary tests' temporary HOME; owner call whether to tighten.
    location: >-
      crates/am-app/src/runtime.rs (Places::from_env)
    severity: medium
  - summary: >-
      `config_note_args` pairs an audit note with the History row whose `at` equals it (ms), not with the intent id.
    evidence: |-
      Story 2.12 adversarial review, finding 6 (second half). Built by story 2.2; with `append_audit` now refusing every `config.*` action, only a direct database write (same-user, outside the threat model) can plant a matching row. Linking notes to the intent id is a store schema change.
    location: >-
      crates/am-store/src/audit.rs (config_note_args)
    severity: low
  - summary: >-
      Every directory answer in `NetEgress::send` runs a full synchronous Key-use check on the async worker (two SQLite readers, two full audit-table scans for confirmation notes, and a possible blocking wait of up to 10 s for an in-flight write).
    evidence: |-
      Story 2.12 follow-up review, finding 4. No run path sends through this client yet; epic 2 story 6 wires the first. Fix there: run the check off the async worker and read confirmation records by Account id with one indexed query instead of scanning the audit log.
    location: >-
      crates/am-net/src/lib.rs (NetEgress::send), crates/am-app/src/key_use.rs (records)
    severity: medium
---

<intent-contract>

## Intent

**Problem:** Stories 2.1 and 2.3 built Accounts per project.
- Each project has its own `models/accounts.yaml`.
- The confirmation records (`account_added`) are in each project's store.
- The add, remove, list and test commands open a project.

The owner decided (2026-11-22, arch AD-17, AD-15, AD-6, AD-7, AD-16) that an Account is user-level:
- one per provider key;
- defined once on this Mac;
- named by id from any project.

Until this story lands, every story that sends a key (2.4, 2.5, 2.6, 2.10, 2.11) would build on the per-project stage.

**Approach:** One user Accounts file and one user store in the app data dir, both named by a new `AppDataLayout`.
- **The file:** `<app data dir>/accounts.yaml`. am-config reads and writes it with AD-16's rules.
- **The user store:** `user.db`. It holds the file's intents, History and hash-chained audit. Its own migration set and `application_id` keep it apart from a project's store. A process writes it only while it holds `user.lock`.
- **The commands:** `add`, `remove`, `list` and `test` move to the user list and open no project. They scan the projects this Mac knows for references.
- **One Key-use check:** a read-only check over the user file and the user store decides whether a key may be sent. `accounts test` and the egress directory both call it, so every key path gets it.
- **Projects:** a project names Accounts by id only.
  - A missing id is a validate warning.
  - An old `models/accounts.yaml` is a validate error, never read to send a key.
- **Samples and fixtures** drop the file, except one legacy fixture kept for story 13's `migrate` tests.

## Boundaries & Constraints

**Always:**
- **Places (AD-17):** `am_domain::AppDataLayout` names every path in the app data dir:
  - the dir;
  - `accounts.yaml`;
  - `user.db`;
  - `user.lock`;
  - `backups/pre-migration/`;
  - the existing `mac-id` and `serving/`.

  `serving_dir` and `MAC_ID_FILE` move behind it. `cargo xtask runtime-paths` treats it like `RuntimeLayout`. am-app's `Places` gains `user: AppDataLayout`, built from the existing `app_data`.
- **Owner-only (AD-17, AD-13):** before opening the user store, am-app checks that the app data dir is 0700 and that `accounts.yaml`, `user.db`, `user.lock` and the WAL files are 0600 (when present).
  - Anything else is refused in words, naming the path and the `chmod` that fixes it.
  - The app creates the dir 0700 and its files 0600.
  - The check also passes on the Linux CI job; mode bits are POSIX.
- **The user store (AD-6, AD-7):** `am_store::user` opens `user.db` from the path am-app gives.
  - **Format:** WAL, `synchronous=FULL`, `foreign_keys=ON`.
  - **Its own migration set:** `0001_user.sql` holds the config intent and History tables and the audit chain, the same shapes as the project store's. It sets `application_id` to a constant distinct from the project store's.
  - **Wrong file:** a database with another non-zero `application_id` is refused in words. So is a newer `user_version`, naming the newer binary.
  - **No long-lived writer:** a write is `UserStore::locked(timeout)`. It takes `user.lock` with a bounded wait: `File::try_lock` polled until a 10 s deadline, std only, no new dependency. After the deadline it is refused in words.
  - **Under the lock:** it opens one read-write connection and migrates it there, backing up first when the database already holds data (`VACUUM INTO` into `backups/pre-migration/user-<version>-<time>.db`, the three newest kept). It settles pending intents, then re-reads the chain tip, the latest History and the pending intents. Each write transaction begins `IMMEDIATE`.
  - **Release:** the guard releases the lock on drop.
  - **Readers:** `UserReader::open` is `SQLITE_OPEN_READ_ONLY` with `query_only=ON` and takes no lock. A missing `user.db` reads as empty.
  - **One executor:** the project writer's command executor (intent, History, audit) is factored into one function over a transaction. The project writer thread and the user guard both call it, so the two stores cannot drift in shape or audit.
- **The user file (AD-16, AD-17):** am-config owns `accounts.yaml`.
  - **Contents:** `schema_version`, then each entry with `id`, `provider`, `name`, `base_url` and `secret_ref`, never a key.
  - **Validation:** ids are kebab-case and unique; `local-lmstudio` and `local-ollama` are refused. `secret_ref` must equal the id, through one predicate (`am_config::key_ref`) that every key reader uses. Story 2.3's provider and `base_url` rules apply unchanged.
  - **Writing:** `ConfigWriter::for_user_accounts(app_data_dir, history)` writes only that one file. It does so with the same atomic write, intent, History, self-write fingerprint and `ConfigCreate` (the first add on a Mac) as a project file, and its `ConfigHistory` is the user-store guard.
  - **Where the loader is called:** `am_config::user_accounts::load(path)` is the only reader. It is called by am-app, and by tests through `fixtures/accounts/<variant>.yaml`, never the real app data dir.
- **Project references (AD-17):** the following keep `account: <id>`:
  - a seat's `model` and `fallback`;
  - a capability's provider (AD-46);
  - a Source (AD-39).
  - **Where the list comes from:** `LoadOptions` gains `user_accounts: Option<UserAccounts>`. Reserved ids always resolve.
  - **No list given:** with `None`, as in `validate --template` and the tests that pass none, an Account reference is checked only for form and gives no missing-id warning.
  - **An id the list lacks:** a Warning naming each line that names it, with the fix: "add that Account on this Mac: `agentic accounts add <id>`".
  - **A known id of the wrong provider kind** where a kind is needed (`AccountOfKind`) stays an Error.
- **Old project files (AD-17):** a project holding `models/accounts.yaml` gets an Error at that file (line 1), with the fix `agentic accounts migrate --project <path>` (story 13).
  - The file is not parsed as Accounts and nothing reads it to send a key.
  - `validate --template` refuses it the same way.
  - `ConfigSnapshot` stops carrying project Accounts.
- **Commands:** `add`, `remove`, `list` and `test` keep story 2.1's and 2.3's rules.
  - **What stays the same:**
    - the one interactive-terminal predicate for `add` and `remove`;
    - the typed id within 2 minutes;
    - the key read with echo off and echo restored on every path;
    - the Keychain service `agentic-manager`, account `<id>`;
    - the fake terminal only in test builds;
    - story 2.3's endpoint rules.
  - **What changes:** AD-15 *Interim CLI Account edit*.
    - **No project:** no `--project` argument, and no project is opened.
    - **References:** each command lists the references from every project in the runtime base's root records whose root is reachable. These are read only through am-config, with the user list passed in. Projects that could not be reached are named as not checked.
    - **Lock and re-read:** the sentence and prompts run holding no lock. Then `user.lock` is taken for a fresh re-read and the write. If the file's bytes, the id's presence or the references changed since the sentence, the command is refused and is to be run again.
    - **Outside change:** the commands are refused while the user file has an outside change (its bytes differ from its latest History after-hash), naming the file.
    - **Children that run as the owner:** the commands are refused while any scanned project enables a stdio MCP server or a Source with `render: true`, naming the project.
  - **Add:**
    - It refuses a reserved id or one already in the list.
    - It reuses an existing Keychain entry, never overwriting it, as built.
    - When the user store holds no confirmation of that id for this provider, scheme, host and port, the owner must also type the stored key at the hidden prompt. It must equal the entry; a mismatch writes nothing. The key is never echoed, logged or stored again.
    - If the write fails, it deletes the Keychain entry only when it created it.
  - **Remove:** it refuses a reserved id or one a found reference still names, naming the references. (Owner, 2026-11-22, at plan approval: "accounts remove refuses (or needs --force) if any unreachable project might reference the Account".) It also refuses while any project in the root records could not be reached, naming each one and saying it might name the Account. `--force` lifts only that unreachable-project refusal, never a found reference, and its sentence names the projects it skips. It writes `account_removed` (id) in the user store with its History, and says the Keychain entry is left in place.
  - **List:** it shows the user list with keys masked. When the file has an outside change, it shows each Account's confirmed values beside the file's.
  - **Test:** it takes no lock and needs no terminal.
- **Record (AD-15):** each write commits its History entry and, in the same user-store transaction, one audit entry.
  - **An add writes `account_added`** with:
    - the file;
    - the id;
    - the provider;
    - the pinned scheme, host and port;
    - the references found;
    - the key state (`stored`);
    - `confirmation: typed_id`.

    It never records the key.
  - **A remove writes `account_removed`.**
  - **Project stores:** their old `account_added` entries are never read or copied.
- **Key use (AD-15):** one am-app module, `key_use`, is the only check before any key read. `accounts test` and `egress::ProjectDirectory` both call it. The check is read-only, over the exact bytes the endpoint is taken from:
  - those bytes hash to the user file's latest History after-hash;
  - the Account's latest confirmation record (`account_added` here; story 14 adds `account_changed`, which the reader already accepts), with no `account_removed` after it, names the same provider, scheme, host, port and key state;
  - `secret_ref` equals the id.

  **In flight:** a hash mismatch while another process holds `user.lock`, or one equal to a pending intent's new hash, waits for the lock (bounded) and checks once more.

  **Re-check after the read** (AD-15 *Key use*, tightened 2026-11-22 after the reviewer gate, merged in PR #16 before any `src/` change): after the key is read and before it is sent, the caller reads the id's latest user-store sequence again; if it moved since the check, the key is dropped unsent and the check runs again.

  **Refusal:** any failure reads no key, sends nothing, and says in words what to do. The refusal is checked before the Keychain is touched, which FakeSecretStore's read counter shows.
- **Egress directory:** `ProjectDirectory` resolves an Account id from the user list through `key_use`. The reserved local Accounts resolve as today. An id missing on this Mac is refused like an unknown id, audited as story 2.2 built it.
- **Templates and fixtures (AD-17):**
  - **Files removed:** `models/accounts.yaml` is removed from `templates/samples/company-a` and from `fixtures/projects/{valid, broken, anatomy, broken-anatomy, sources-ok, sources-broken}`.
  - **Accounts files added:** `fixtures/accounts/<variant>.yaml` carries the Accounts those fixtures need (Company A: `anthropic` and `openai`).
  - **The legacy fixture:** `fixtures/projects/legacy-accounts/` is a small valid project plus the old-format file. It is the only one, and it is for story 13.
  - **The broken fixtures' Account findings** move to a broken `fixtures/accounts/broken.yaml`.
  - **No department vocabulary** is added in `crates/` or `web/src`.
- **Tests first:** tests are written to fail first, with no network and no real Keychain (FakeSecretStore with canary keys, a temporary HOME per test). The red run is logged here before any `src/` change.

**Never:**
- **Out of scope here:**
  - `accounts migrate` (story 13) and `accounts change` (story 14);
  - keyless Accounts (story 2.10);
  - a run's pin of the user file's hash (epic 2 story 6, the first run that sends a key);
  - the per-project record of where each Account sends calls, and the approved grant set's Account destinations (epic 4 stories 17 and 18);
  - any UI.
- **No reads to send a key:** never read a project's `models/accounts.yaml` to send a key, and never copy a project store's records to the user store.
- **No path override:** no env var, flag or setting that points the app at another Accounts file or user store; tests use a temporary HOME. No CLI-token API route that touches Accounts.
- **Keys:** no key in the user file, `user.db`, History, audit, logs or output.
- **No new dependency.**

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Shared Account | `add anthropic` (terminal, typed id, canary key); then `list` and `test` with the cwd in project P1 and again in P2 | Both list and test it; one Keychain entry; no second add | — |
| Where it lives | After the add | The user file and `user.db` hold it; no file under P1 or P2 changed; the canary is in no file, row, log or output | — |
| First add on a Mac | No app data dir | Dir created 0700, the file by `ConfigCreate`, `user.db` migrated; files 0600 | — |
| Loose permissions | App data dir 0755, or `user.db` 0644 | Refused before the store opens, naming the path and the `chmod` | Worded |
| Wrong database | `user.db` holding a project store, or a newer `user_version` | Refused, naming what it is or the newer binary | Worded |
| `secret_ref` ≠ id | User file entry `secret_ref: other` | Validate error; `test` refused before any Keychain read | — |
| Hand edit | Change any byte of the user file | `test`, the egress directory and `add`/`remove` refused before any Keychain read, naming the file; putting the bytes back restores them | — |
| Record mismatch | A user store whose latest `account_added` for the id names another host than the file (the file's hash still matches History, as after a non-widening accepted edit) | Refused before any Keychain read, naming the confirmed and the file's host | — |
| Moved while reading | The record or file changes (another process) between the check and the Keychain read | The key is dropped unsent; the check runs again and refuses, naming the file's new host | — |
| Remove | `remove openai`, no references | `account_removed` and History written; Keychain entry left; a later `test openai` refused | — |
| Remove in use | `remove anthropic` while P1's seat names it | Refused, naming P1 and the seat line | Worded |
| Unreachable project | A root record whose path is gone | `add` says it was not checked; `remove` is refused, naming it, until `--force`, which removes and names the skipped project; `--force` never overrides a found reference | Worded |
| Child of the owner | A reachable project enables a stdio MCP server | `add` and `remove` refused naming the project; `test` still runs | Worded |
| Existing Keychain entry | Entry `openai` exists, no confirmation for it | The sentence names the reuse; the stored key must be typed (hidden) and match; a mismatch writes nothing | — |
| Changed since sentence | Another process adds the same id between sentence and lock | Refused, run again; nothing written | — |
| Concurrency | Two adds of different ids at once (two threads, separate lock fds) | Both succeed, one after the other; the audit chain verifies | — |
| Lock held | A child process holds `user.lock` past the deadline | Refused in words after the bounded wait; nothing written | — |
| In flight | `test` while a write holds the lock with the new file already renamed | Waits, then passes against the new History entry | — |
| Old project file | Project with `models/accounts.yaml` | `agentic validate` error at that file with the migrate fix; no command reads it for a key | — |
| Missing id | A seat names `anthropic`; the user list lacks it | Validate warning at the seat line with the add fix; a key path refuses it in words | — |
| Templates | `validate --template templates/samples/company-a`; fixtures with their `fixtures/accounts` variant | Pass; the legacy fixture fails as above | — |
| Not interactive | `add`/`remove` with a pipe on stdin | Refused, nothing read or written (unchanged) | — |

</intent-contract>

## Code Map

- **`crates/am-domain/src/runtime.rs`:**
  - `APP_DATA_NAME`, `MAC_ID_FILE` and `serving_dir` (L30-45).
  - Add `AppDataLayout` beside `RuntimeBase` (L119).
- **`crates/am-store`:**
  - **Factor the executor:** `lib.rs` `WriteCommand` (L151), `Writer::exec` (L480) and the job loop (L534). The intent, History and audit execution moves into one `fn execute(tx, actor, cmd)`.
  - **Generalise the migrator:** `migrations.rs` `migrate_with` (L187) is already generic. Add a parameter for `application_id` and the backup name prefix (`state-*` today, `user-*` for the user store).
  - **New module `user.rs`:** `UserStore::locked` (the guard implementing `ConfigHistory` and an audit append), `UserReader` and `USER_MIGRATIONS`.
  - **Patterns:** `project.rs` `open_runtime` (L119) and `fsx.rs` for the 0600 writes.
- **`crates/am-config`:**
  - **Move:** `kinds.rs` `kinds::accounts` (L481-577) becomes `user_accounts.rs`, the user-file kind and loader. The project kind becomes the legacy-file error.
  - **Retarget:** `account_edit.rs` (`AccountsFile` L52, `accounts_file` L132, `account_uses` L217, `add_account_edit` L271, `remove_account_edit` L300, `new_accounts_file` L314, `outside_changes` L333) targets the user file.
  - **References:** `refs.rs` `RESERVED_ACCOUNTS` (L11), `RefKind::Account` (L308-320) and `AccountOfKind` (L395-445) resolve against `LoadOptions.user_accounts`.
  - **The places that resolve Accounts:** `kinds.rs` `model_ref` (L931-955), `tools` (L758-775), `seat_capabilities` (L1630-1640) and `source` (L2840-2870).
  - **Writer:** `writer.rs` `ConfigWriter::open` (L332) gains its `for_user_accounts` sibling.
  - **Snapshot:** `snapshot.rs` `accounts()` (L38) leaves the project snapshot.
- **`crates/am-app/src`:**
  - **`runtime.rs`:** `Places` (L83-130) gains `user`. `records(base)` (L375) and `RootRecord.root.path` drive the reference scan.
  - **`accounts.rs`** (1413 lines):
    - `open_for_edit` (L534) is replaced by the user-side open and lock;
    - `add` (L639-898);
    - `remove` (L899-974), which gains `account_removed`;
    - `list` (L1036);
    - `test` (L1088).
  - **`egress.rs`:** `ProjectDirectory::open` (L53), `Records::open` (L214) and `Records::check` (L251) move into the new `key_use.rs`.
  - **`main.rs`:** the `accounts` subcommands drop `--project`, and `validate` passes the user list.
- **`xtask/src/runtime_paths.rs`:** `AppDataLayout` joins the allowed spellers.
- **Fixtures:**
  - `templates/samples/company-a/models/accounts.yaml`;
  - `fixtures/projects/*/models/accounts.yaml` (6 projects);
  - new `fixtures/accounts/` and `fixtures/projects/legacy-accounts/`.
- **Tests:**
  - **`crates/am-app/tests/accounts_cli.rs`** (58 tests, 3577 lines): the harness at L94-110 already gives a temporary HOME. Its project-store assertions move to user-store ones.
  - **New:**
    - `crates/am-store/tests/user_store.rs`;
    - `crates/am-config/tests/user_accounts.rs`;
    - `crates/am-app/tests/user_accounts.rs` (sharing, lock, permissions, in flight, scan);
    - validate tests in the existing am-config and am-app suites.

## Tasks & Acceptance

**Execution:**
- [ ] **Tests first (no `src/` change):** a test for every matrix row, plus the updated `accounts_cli.rs`, the fixture changes and the validate expectations. Log the red run.
- [ ] **`crates/am-domain`:** `AppDataLayout`.
- [ ] **`crates/am-store`:** the shared executor, the migrator parameters, the user store and its reader.
- [ ] **`crates/am-config`:** the user-file kind, loader, writer mode and `key_ref` predicate; references against the user list; the legacy-file error.
- [ ] **`crates/am-app`:** the places, the permission check, `key_use`, the commands on the user list with the reference scan and the lock protocol, the directory, and `validate`.
- [ ] **`xtask`, templates and fixtures:** the runtime-paths scan, and the files moved.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then the outcome, the files, the user store and the Keychain match the row.
- Given the workspace, when `just check` runs, then it is green, `agentic validate` included over `templates/` and `fixtures/`.

## Review Triage Log

### 2026-11-22 — Review pass (quick lens and an adversarial keys-and-Accounts lens)
- verdicts: 19 findings — high 3, medium 9, low 5, false 0, maybe-false 0 (one adversarial finding, 5, is low on its uncompared items; its main claim is the accepted reading in "Implementer choices accepted")
- routing note: the fixes below add one port method (am-domain `EgressDirectory`) and route as patch by the coordinator, following story 2.2's review-fix rounds (tests first, red run logged before any `src/` change), not a revert and re-derivation of the whole story.
- findings:
  - Quick 1 `[high]` `[patch]` (group G1) The Key-use re-check guards a throwaway key read in `test`; the key actually sent is read by am-net's `NetEgress::send` (crates/am-net/src/lib.rs:748-753) with no check or re-check, and `ProjectDirectory` answers Accounts from a map filled once at open (egress.rs:148-152), so `serve` keeps answering an Account removed or hand-edited after startup — verified at both lines. Fix: the directory runs `key_use` on every Account answer (no cache) and gives a stamp; am-net reads the stamp again right after its key read and drops the key unsent if it moved; `test` loses its throwaway read.
  - Quick 2 `[medium]` `[patch]` The add sentence for a reused Keychain entry does not name the provider and host that entry was last confirmed for (AD-15 *Add*); the last confirmation before a removal is lost. Fix: name it from the latest `account_added` for the id even when a removal follows.
  - Quick 3 `[medium]` `[patch]` (group G3) `UserReader::open` goes through `open_read_only`, which opens `immutable=1` when no WAL exists, while other processes write `user.db`. Fix: the user reader opens read-only with `query_only=ON`, never immutable.
  - Quick 4 `[medium]` `[patch]` A crash between the first user migration's commit and the separate `application_id` write leaves version 1 with id 0, refused for ever. Fix: stamp `application_id` inside the first migration's transaction.
  - Quick 5 `[medium]` `[patch]` (group G5) `record_serving` creates the app data dir with the default mode (0755 under a normal umask) via `ensure_private_dir`'s `create_dir_all(parent)`, after which every Accounts command is refused. Fix: create every app-data folder 0700.
  - Quick 6 `[medium]` `[patch]` (group G6) `UserStore::append_audit` appends directly, refusing only two kinds, so a `config.*` entry can be written with no intent, unlike the shared executor. Fix: refuse every `config.*` action there.
  - Quick 7 `[low]` `[patch]` A crash-interrupted write (file holds a pending intent's bytes, no lock holder) is refused as an outside edit with advice to undo it. Fix: its own words, naming that the next `accounts add` or `remove` settles it.
  - Quick 8 `[low]` `[patch]` `SECRET_FIX` and doc comments still send the owner to `models/accounts.yaml`. Fix: point to the user Accounts file and `agentic accounts add`.
  - Adversarial 1 `[high]` `[patch]` (group G1) The serve directory checks once at startup and never again; any run path on that client inherits a one-time check.
  - Adversarial 2 `[high]` `[patch]` (group G1) `accounts test` re-checks after a throwaway read; the key sent is read later after no check, and the directory is built before the re-check loop.
  - Adversarial 3 `[high]` `[patch]` A key pasted into `secret_ref:` is quoted by the loader error (kinds.rs:568), printed by `validate` and `test`, and through the egress block reason written into a project's audit chain — verified. Fix: loader messages never quote a value the secret check flags (and `secret_ref` is never quoted), and an egress Account refusal records a fixed reason code, not loader words.
  - Adversarial 4 `[medium]` `[defer]` `$HOME` chooses the app data dir, so a same-user process can point `test` at a forged store. Same-user forging is outside the threat model (AD-15, owner 2026-11-22); deferred for the owner (frontmatter).
  - Adversarial 5 `[low]` `[patch]` Under the lock, `add` does not compare the sentence's not-checked list or the `confirmed` state that decided the stored-key prompt. Fix: compare both. (Accepting app writes of other ids is the accepted reading, not a defect.)
  - Adversarial 6 `[medium]` `[patch]` (group G6) `append_audit` can write `config.*` rows that the note reader pairs by timestamp. Fixed with Quick 6; the timestamp pairing itself predates the story (2.2) and is deferred (frontmatter).
  - Adversarial 7 `[medium]` `[patch]` (group G3) The immutable reader, as Quick 3.
  - Adversarial 8a `[medium]` `[patch]` (group G5) The 0755 app data dir, as Quick 5.
  - Adversarial 8b `[low]` `[patch]` A non-empty foreign SQLite file with `application_id` 0 and `user_version` 0 is migrated into and stamped. Fix: refuse version 0 when the schema is not empty.
  - Adversarial 8c `[medium]` `[patch]` `for_accounts` checks every Account, each up to 10 s, and each check loads every History row with its blobs. Fix: with G1 the directory checks only the Account asked for; the check reads only the latest History entry.
  - Adversarial 8d `[medium]` `[patch]` Control characters in a project's file names reach the terminal in the add and remove sentences and can hide where the key goes. Fix: escape control characters in every name and path shown in those sentences.

- fixes applied (2026-11-22, implementer re-engaged; tests first, red run logged): G1: one `EgressDirectory::account_stamp` port method; `ProjectDirectory` checks the one Account asked on every answer, no cache and no check at open; `NetEgress::send` re-reads the stamp after its own key read and drops the key unsent if it moved. 8c: one-row `latest_config_entry`. Quick 2: `key_use::last_confirmed` named in the sentence. Adversarial 5: not-checked list and confirmed state compared under the lock. 8d: `user::shown()` escapes control characters. Quick 7: `Refused::Interrupted` words. Adversarial 3: `secret_ref` never quoted unless a plain kebab id, flagged values stripped from findings, fixed audit reason for directory refusals. Quick 8: fix text and doc comments. G3: read-only `query_only` user reader. Quick 4: `application_id` in the first migration's transaction. 8b: non-empty version-0 database refused. G6: `append_audit` refuses `config.*`. G5: app data dir created 0700. `just check` green, 834 tests.

### 2026-11-22 — Follow-up review pass (adversarial lens on the review fixes)
- verdicts: 11 findings — high 3, medium 3, low 3, false 1, maybe-false 0 (one finding grouped)
- findings:
  - `[high]` `[patch]` (group FU1) The endpoint the client sends to comes from one Key-use check (`checked_addr`, lib.rs:757) and the stamps from two later ones (lib.rs:770, 776); the stamp holds no endpoint, so a remove and re-add to another host during DNS leaves equal stamps and the key goes to the old address. Fix: the stamp names the file hash, the latest record's sequence and the confirmed endpoint; the client takes it before resolving and again after its key read, requires both present and equal, and sends only to an endpoint equal to the stamp's.
  - `[high]` `[patch]` (group FU1) `account_stamp` returns `None` on any refusal and `None == None` passes in `NetEgress::send`, so a hand edit or remove during DNS still sends the key. Fix: a non-reserved Account needs a present stamp both times.
  - `[false]` `[reject]` `test` keeps its earlier Keychain read — the coordinator's logged decision ("Review fixes: test changed after the red run"); that read is a presence check whose key is never sent.
  - `[medium]` `[defer]` Directory answers in the async send path are synchronous full checks with full audit scans and a possible 10 s blocking wait. No run path uses this client before epic 2 story 6; deferred there (frontmatter).
  - `[high]` `[patch]` (group FU5) Hiding flagged values in user-file findings matches raw line text, so a quoted value (`provider: "sk-…"`), a trailing comment, a lowercased `base_url` host or a lowercase-hex key in `secret_ref` (quoted when kebab-shaped) is still printed by `validate` and `test`. Fix: user Accounts findings, wherever produced (the loader, `account_edit`'s entry errors, `key_use` words), quote no field value except the entry's own valid id and a known provider name; any other quoted segment is replaced by a fixed placeholder; `secret_ref` is never quoted (the broken-fixture test's expectation follows).
  - `[medium]` `[patch]` (group FU5) `base_url` errors quote the lowercased host, which the hiding misses.
  - `[medium]` `[patch]` (group FU5) The Key-use path builds entry errors through `kinds::accounts` directly (account_edit.rs:181-190) with no hiding, reaching `test` output and every egress caller's error.
  - `[high]` `[patch]` (group FU5) `secret_ref` is quoted for a lowercase-hex key (kinds.rs:567).
  - `[medium]` `[patch]` `user::shown` escapes only Cc; bidi overrides and isolates, zero-width characters, U+2028/2029 and U+FEFF pass, and `list` prints an invalid entry's raw id. Fix: escape those too, and in `list`.
  - `[low]` `[patch]` 8b's emptiness test uses `NOT LIKE 'sqlite_%'`, where `_` is a wildcard. Fix: match the literal prefix.
  - `[low]` `[reject]` am-net's public `check_caller` returns a NUL marker only `refuse` strips; no other caller exists and the fix adds surface for an unreachable case.

### 2026-11-22 — Second follow-up review pass (narrow: send-time stamp and value hiding)
- verdicts: 5 findings — high 0, medium 4, low 1, false 0, maybe-false 0. Send-time key use: no findings (stamp present and equal before resolution and after the key read, endpoint equal to the connection's target, redirects and retries covered; the default `account_stamp` is used by no production directory for a non-reserved Account).
- findings:
  - `[medium]` `[patch]` (group FU6) A YAML parse error message carries file text unquoted (e.g. a duplicate mapping key that is a key); `validate` prints it, and on the Key-use path `accounts_file`'s `problem` is never hidden (reaches `test`, `add`, `remove`, `list` and egress callers' words).
  - `[medium]` `[patch]` (group FU6) A `'` inside a value breaks `hide_values`' quote pairing, so part of the value is copied verbatim.
  - `[medium]` `[patch]` (group FU6) The keep-list takes every raw `id:` line (block scalars included, invalid ids included), so a non-prefixed key placed as `id:` prints.
  - `[medium]` `[patch]` (group FU6) `list` prints an invalid entry's raw id. Fix for the group: hiding by membership, not by quotes — any fragment of 6 or more characters in a message or fix that occurs in the user file's text (case-insensitively) is replaced by the placeholder unless it is a field name, a known provider name, a reserved id, or the parsed id of an entry that loaded as a valid Account; applied to every user-Accounts finding, entry error and `problem` text; `list` names an invalid entry by its line, not its id.
  - `[low]` `[reject]` Key-use words quote the confirmed file's host and a typed `base_url`: reachable only for a file the app wrote (its hash matches History), values the owner typed at `add`; `list` shows the same by design.

## Plan Change Log

- 2026-11-22 (coordinator, before any `src/` change): added the Key-use re-check after the key read and its matrix row, from AD-15 as amended by PR #16 (owner answers replan). Owner additions at approval (remove refuses unreachable projects until `--force`) were folded in before phase 1.

## Design Notes

**Decisions for the owner's approval:**
1. **No `--project` on `add`, `remove`, `list` and `test`.** References come from the runtime base's root records, as AD-15 says, so the cwd does not matter.
2. **The lock wait is 10 s, polled, std only.** `File::try_lock` is stable on the pinned toolchain, so no crate is added. The same deadline serves the Key-use in-flight wait.
3. **The user store shares the project store's table shapes** (one executor) but has its own migration set and `application_id`. A user store never opens as a project store, and a project store never opens as a user store.
4. **`LoadOptions.user_accounts: None` gives no missing-id warning.** `validate --template` and the library tests check Account references for form only, so a shipped template never warns about ids that exist only on the owner's Mac. `agentic validate` on a project always passes the user list, an empty one when the Mac has none.
5. **The add's stored-key check is built here** (AD-15 *Add*, tightened 2026-11-22). When the Keychain already holds an entry for the id and the user store has no confirmation of it for this destination, the owner types the stored key at the hidden prompt. This is not story 14's `change`, which the owner ruled must not ask for the key.
6. **The broken fixtures' Account findings move to `fixtures/accounts/broken.yaml`**, so `validate` over fixtures keeps covering them.

**Size:** this is the largest Accounts story yet. It touches 3 crates' sources, about 1.5k lines of tests and 8 fixture trees. It stays one commit, as one story. Story 13 (`migrate`) and story 14 (`change`) follow it at once, back to back.

## Verification

- `cargo nextest run -p am-store -p am-config -p am-app` and the new tests, then `just check`.
- The canary scan finds no canary in the temporary HOME's files, `user.db`, logs or output.

## Implementation Notes

### Red run (2026-11-22, tests first; no `src/` file created or changed)

**Test files written or rewritten** (all outside `src/`):

- New: `crates/am-domain/tests/app_data_layout.rs`, `crates/am-store/tests/user_store.rs`, `crates/am-config/tests/user_accounts.rs`, `crates/am-app/tests/user_accounts.rs`.
- Rewritten for user-level Accounts: `crates/am-app/tests/accounts_cli.rs` (harness: no `--project`, the user file and `user.db` instead of the project file and store, hand seeds written to the user file 0600; tests now covering the stored-key check, `account_removed`, opening no project), `crates/am-app/tests/accounts_tty.rs`, `crates/am-app/tests/serve_project.rs` (`AppDataLayout::serving_dir`), `crates/am-config/tests/{account_edit,create,validate,anatomy,sources,templates,writer,hosts}.rs` and `common/mod.rs`, `crates/am-e2e/tests/cli_validate.rs` (fixture rows in `--template` mode; the project-mode rows moved to the binary tests in `crates/am-app/tests/user_accounts.rs` so no test reads the real HOME), `xtask/tests/runtime_paths.rs`.
- Fixtures: `models/accounts.yaml` removed from `templates/samples/company-a` and `fixtures/projects/{valid,broken,anatomy,broken-anatomy,sources-ok,sources-broken}`; added `fixtures/accounts/{sample,valid,anatomy,sources,broken}.yaml` (the sample's variant is named `sample` to keep its id out of `crates/`, AD-23) and `fixtures/projects/legacy-accounts/` (company, one seat naming `cloud`, one workflow, the old file).

**Matrix row → test:** Shared Account `one_account_serves_every_project_on_this_mac`; Where it lives `the_account_lives_in_the_app_data_dir_and_nowhere_else` (+ `assert_no_key_leaked` scanning every file under the temp HOME); First add `the_first_add_on_a_mac_creates_the_app_data_dir_owner_only` (+ store side `a_new_user_store_is_owner_only_wal_and_carries_its_own_application_id`); Loose permissions `loose_permissions_are_refused_naming_the_path_and_the_chmod`; Wrong database `a_project_store_at_user_db_is_refused` (am-app), `a_project_store_at_user_db_is_refused_naming_what_it_is`, `a_newer_user_store_is_refused_naming_the_newer_binary`, `a_user_store_never_opens_as_a_project_store` (am-store); secret_ref ≠ id `a_secret_ref_must_equal_the_id`, `validate_reports_the_user_files_own_errors`, `a_secret_ref_that_names_another_entry_is_never_read_or_sent`; Hand edit `any_hand_edit_of_the_user_file_refuses_every_key_path` (+ the 2.3 hand-edit tests in accounts_cli); Record mismatch `a_file_that_disagrees_with_the_confirmation_record_is_refused`; Remove `a_removed_account_is_recorded_and_then_refused`, `remove_of_an_unreferenced_account_has_history_and_undo`; Remove in use `remove_refuses_a_referenced_reserved_or_unknown_id`; Unreachable project `an_unreachable_project_is_named_and_only_force_lifts_its_refusal`; Child of the owner `a_project_with_an_owner_child_refuses_add_and_remove_but_not_test`, `add_is_refused_while_a_stdio_mcp_server_is_enabled`; Existing Keychain entry `add_reuses_an_existing_keychain_entry_and_never_overwrites_it`, `add_reusing_an_entry_whose_stored_key_is_mistyped_writes_nothing`, `add_reusing_an_entry_confirmed_for_another_host_asks_for_the_stored_key`; Changed since sentence `the_same_id_added_by_another_process_meanwhile_refuses_the_add`; Concurrency `two_adds_at_once_take_turns_and_the_chain_verifies` (+ store `two_writers_take_turns_and_the_chain_verifies`); Lock held `an_add_waits_its_bounded_time_for_a_held_lock_then_refuses` (child = this test binary running the ignored `lock_holder_child`; takes ≥ 10 s by design) (+ store `a_held_lock_refuses_after_the_bounded_wait`); In flight `a_test_during_a_write_waits_for_it_and_passes`; Old project file `an_old_project_accounts_file_is_an_error_with_the_migrate_fix`, `an_old_project_accounts_file_is_never_parsed_as_accounts`, `an_old_project_file_fails_validate_with_the_migrate_fix`, `the_legacy_accounts_fixture_fails_with_the_migrate_fix`, `test_refuses_an_account_that_was_never_added`; Missing id `an_id_missing_on_this_mac_is_a_warning_at_each_line_naming_it`, `a_missing_id_is_a_validate_warning_and_a_key_path_refusal`; Templates `templates_and_fixtures_validate_with_their_accounts_variant` (am-config and am-app), `only_the_legacy_fixture_holds_an_old_accounts_file`; Not interactive `add_and_remove_refuse_when_stdin_is_not_a_terminal` and `accounts_tty.rs` (unchanged rule).

**Names the tests fix (decided here as field/test-structure choices):** `am_domain::runtime::AppDataLayout::{new, dir, accounts_file, user_db, user_lock, backups_pre_migration, mac_id, serving_dir}` (Clone, Debug, PartialEq); `Places::user`; `am_store::APPLICATION_ID` and `am_store::user::{UserStore, UserReader, USER_APPLICATION_ID, USER_SCHEMA_VERSION}` with `UserStore::locked(&AppDataLayout, Duration)` (implements `ConfigHistory`, plus `append_audit(AuditEvent)`), `UserReader::open(&AppDataLayout)` with `config_history`, `config_intents`, `config_note_args`, `export_audit_jsonl`, `verify_audit_chain`; `am_config::{USER_ACCOUNTS_FILE = "accounts.yaml", LEGACY_ACCOUNTS_FILE = "models/accounts.yaml", UserAccounts (Default, ids(), get()), key_ref(&Account) -> Option<&str>, user_accounts::load(&Path) -> {findings, accounts}}`, `LoadOptions::user_accounts`, `ConfigWriter::for_user_accounts(dir, history)`, `accounts_file(<app data dir>)`; History entries of the user file have `file == "accounts.yaml"`; the user writer's test fail point is `am_app::runtime::testing::fail_writer_at(<app data dir>, …)`; `agentic accounts remove … --force`; usage error (exit 2) for `--project` on every accounts command; the missing-id fix text `add that Account on this Mac: \`agentic accounts add <id>\``; xtask `runtime-paths` also flags `user.db`/`user.lock` literals outside `runtime.rs`, naming `AppDataLayout`. Not covered by a test: the pre-migration backup of a user store with data (only one user migration exists).

**Command 1** (the four crates whose tests name the new API):

```
cargo nextest run -p am-domain -p am-store -p am-config -p am-app --no-fail-fast
```

```
error[E0432]: unresolved import `am_domain::runtime::AppDataLayout`
  --> crates/am-domain/tests/app_data_layout.rs:11:41
error[E0432]: unresolved import `am_store::user`
  --> crates/am-store/tests/user_store.rs:33:15
error[E0425]: cannot find value `APPLICATION_ID` in crate `am_store`
   --> crates/am-store/tests/user_store.rs:169:26
error[E0432]: unresolved imports `am_config::LEGACY_ACCOUNTS_FILE`, `am_config::USER_ACCOUNTS_FILE`, `am_config::UserAccounts`, `am_config::key_ref`, `am_config::user_accounts`
  --> crates/am-config/tests/user_accounts.rs:37:50
error[E0560]: struct `LoadOptions` has no field named `user_accounts`
  --> crates/am-config/tests/common/mod.rs:64:9
error[E0599]: no associated function or constant named `for_user_accounts` found for struct `ConfigWriter` in the current scope
   --> crates/am-config/tests/account_edit.rs:83:27
error[E0609]: no field `user` on type `Places`
   --> crates/am-app/tests/user_accounts.rs:134:21
error: could not compile `am-domain` (test "app_data_layout") due to 1 previous error
error: could not compile `am-store` (test "user_store") due to 5 previous errors
error: could not compile `am-config` (test "user_accounts") due to 10 previous errors
error: could not compile `am-config` (test "account_edit") due to 5 previous errors
error: could not compile `am-app` (test "user_accounts") due to 38 previous errors
```

(also `am-app` tests `accounts_cli` 19 errors, `accounts_tty` 1, `serve_project` 2, and every am-config test that uses `common`: the same missing names; the only other errors, E0277 on two `for` loops in `crates/am-app/tests/user_accounts.rs`, are inference cascades of the missing `Places::user` and disappear once it exists, checked by substituting plain paths.)

**Command 2** (the suites that still compile: the fixtures moved, the scan rule is new):

```
cargo nextest run -p am-e2e -p xtask --no-fail-fast
```

```
FAIL am-e2e::cli_validate the_legacy_accounts_fixture_fails_with_the_migrate_fix
  assertion `left == right` failed: agentic validate: …/fixtures/projects/legacy-accounts: 0 errors, 0 warnings
FAIL am-e2e::cli_validate valid_fixture_exits_zero_with_no_errors
  seats/beta.md:11: unknown account 'cloud'
FAIL am-e2e::cli_validate sources_ok_exits_zero_with_two_warnings
  sources/docs.yaml:5: unknown account 'crawly'
FAIL am-e2e::cli_validate anatomy_fixture_exits_zero_with_its_warnings
  seats/full.md:13: unknown account 'finder'
FAIL am-e2e::cli_validate broken_anatomy_prints_every_error_with_a_fix_and_exits_one
FAIL am-e2e::cli_validate sample_company_template_exits_zero
  seats/d1-e1.md:15: unknown account 'anthropic'
FAIL am-e2e::cli_validate every_template_folder_exits_zero
FAIL am-e2e::cli_validate warnings_alone_exit_zero
FAIL am-e2e::config_reload config_reload_outside_edit_waits_for_reload_on_a_real_store
  the session opens: Invalid { findings: [Finding { file: "seats/beta.md", line: 11, severity: Error, message: "unknown account 'cloud'", … }] }
FAIL am-e2e::config_reload config_reload_empty_file_created_outside_is_not_announced_or_applied
FAIL am-e2e::config_migrate config_migrate_database_and_config_one_version_back_on_a_real_store
FAIL xtask::runtime_paths a_planted_user_store_path_literal_fails_and_the_layout_file_passes
  panicked at xtask/tests/runtime_paths.rs:73:5: a planted literal must fail
Summary [4.575s] 169 tests run: 157 passed, 12 failed, 7 skipped
```

The `config_reload`/`config_migrate` e2e failures are existing tests turned red by the fixture move (the old loader still requires a project-declared Account); they pass once a missing id is checked for form only (`LoadOptions::user_accounts: None`). `just validate` is red for the same reason until then.

### Red run addendum (2026-11-22, coordinator, before any `src/` change)

AD-15 *Key use* gained the re-check after the key read (PR #16, merged after phase 1). One test added for it, `a_record_that_moves_while_the_key_is_read_drops_the_key_unsent` in `crates/am-app/tests/user_accounts.rs`, and one matrix row with the Key-use clause (Plan Change Log).

```
cargo nextest run -p am-app --test user_accounts a_record_that_moves
error[E0432]: unresolved import `am_store::user`
error[E0433]: cannot find `user_accounts` in `am_config`
error[E0609]: no field `user` on type `Places`
```


### Tests changed after the red run (2026-11-22, coordinator decision)

The implementer stopped on four red-run tests that could not compile at the red run and contradict the plan or themselves; corrected by the coordinator, no expectation weakened against the plan:

1. `am-store/tests/user_store.rs` `a_config_write_is_recorded_alike_in_both_stores`: also drops `target` before comparing (it is the intent id, a fresh UUIDv7 per write; `config_history.rs` requires `target == id`).
2. `am-config/tests/user_accounts.rs` `an_id_missing_on_this_mac_is_a_warning_at_each_line_naming_it`: the `account:` lines of its own `seat()` text are 7 and 10, not 8 and 11.
3. `am-config/tests/anatomy.rs` `broken_skills_and_seat_give_one_error_each`: 10 errors, not 11; with no user list the wrong-kind Account is found only by `a_provider_account_of_the_wrong_kind_is_an_error` (with the list), as `cli_validate.rs` already says.
4. `am-config/tests/writer.rs` `writer_allows_fixing_two_broken_files_one_at_a_time`: the refused edit now sets a malformed id `No_Such_Account` (a form error), since a missing id is a Warning under the plan.

Plus mechanical clippy fixes in `user_store.rs` (`.err().expect()` to `let Err(..) else`, a const assert).

Implementer choices accepted: user-store pre-migration backups named `user-<stamp>-v<from>.db` (the existing pattern); the `runtime-paths` rule skips test files; settling a pending user-store intent is `ConfigWriter::reconcile`, called by am-app under the lock.
5. `am-app/tests/user_accounts.rs` `a_missing_id_is_a_validate_warning_and_a_key_path_refusal`: the warning is on line 10 (its `seat()` text's `- account:` line), not 11; same miscount as item 2.

Implementer choices accepted (am-app round):
- **Two adds at once:** under the lock, bytes changed since the sentence are accepted only when an app write made them (a newer History entry holds them) and the id is still absent and the references unchanged; a hand change, or the same id appearing, still refuses. This is the reading of AD-15's "anything that changed since the sentence was shown refuses" under which the ticket's "two processes adding Accounts at once both succeed" holds: what the sentence stated (the id's absence, its references, an app-written file) is re-checked, not unrelated entries.
- `account_removed` is recorded with action `config.narrowed`.
- `test` checks the app's own ports across every running server and runtime folder, since it opens no project.
- `ConfigNote` gained `seq` for ordering additions and removals.

### Review fixes: red run (2026-11-22, tests first, before any `src/` change)

Tests for the review findings routed `patch` (Review Triage Log, 2026-11-22), written in the crates' `tests/` folders only; no `src/` file created or changed. The am-app tests drive a real `am_net::NetEgress` on am-e2e's RULE-9 fakes (scripted resolver and per-hop transport, `crates/am-e2e/tests/trust/support/egress.rs`, included by `#[path]`; no network).

**Item → test:**

- **G1 (Quick 1, Adversarial 1, 2), send-time key use** (`crates/am-app/tests/user_accounts.rs`): `an_account_removed_after_the_client_was_built_is_refused_unsent` (client built while confirmed, one call sent, then `accounts remove`; the next call is refused, the transport gets nothing more and the Keychain is not read again); `a_hand_edit_after_the_client_was_built_is_refused_unsent`; `a_change_committed_while_the_client_reads_the_key_is_never_sent` (the existing `ChangingStore` commits a re-point during the client's own key read: no hop, a refusal naming the new host or the change); `accounts_test_reads_the_key_once_inside_the_egress_client` (`accounts test` with a real `NetEgress` from the command's `EgressFactory`: one Keychain read, one hop).
- **Quick 2:** `add_names_where_a_reused_key_was_last_confirmed_even_after_a_removal` (add `router` openai-compatible at openrouter.example, remove it, add again as anthropic: the sentence names openai-compatible and openrouter.example).
- **G3 (Quick 3, Adversarial 7)** (`crates/am-store/tests/user_store.rs`): `a_reader_opened_without_a_wal_sees_a_later_commit`.
- **Quick 4:** no failing test is possible from outside `src/` (the crash falls between two statements of `migrate_set`; nothing outside can stop it there). Covered only indirectly by the existing `a_new_user_store_is_owner_only_wal_and_carries_its_own_application_id` (both stamps set after migration).
- **G5 (Quick 5, Adversarial 8a):** `a_serving_record_on_a_fresh_mac_leaves_the_app_data_dir_owner_only` (`am_app::runtime::record_serving` on a fresh HOME, then `accounts add`). Under a umask of 077 it would pass vacuously; the run's umask is 022.
- **G6 (Quick 6, Adversarial 6):** `the_guard_refuses_every_config_audit_entry` (`config.narrowed` with `kind: account_removed`, `config.edited`, `config.widened`, `config.created`). The existing `the_guard_appends_an_audit_entry_to_the_chain` appended `config.widened` (`account_removed`), which G6 forbids; it now appends `accounts.tested` (`kind: account_tested`), same assertion shape.
- **Quick 7:** `an_interrupted_write_is_named_as_one_and_not_as_an_outside_edit` (a write stopped after its rename, its writer dropped: words say interrupted, name `agentic accounts add` and `remove`, never "outside the app" or "by hand").
- **Quick 8** (`crates/am-config/tests/user_accounts.rs`): `the_secret_value_fix_names_this_macs_accounts_not_the_old_project_file` (a seat body and a user-file `name:` holding a key).
- **Adversarial 3:** `accounts_test_never_prints_a_key_pasted_as_secret_ref`, `an_egress_refusal_never_returns_or_audits_a_key_pasted_as_secret_ref` (project store via `StoreAuditSink`; the audit export is checked first, then the returned words), `validate_never_prints_a_key_pasted_as_secret_ref`.
- **Adversarial 5:** `a_project_unreachable_since_the_sentence_refuses_the_add` (a reachable project removed while the add waits at the id prompt); `a_confirmation_recorded_since_the_sentence_refuses_the_add` (Keychain holds `cloud`, no confirmation, so the stored key is asked; meanwhile another process creates the file with an `account_added` of `cloud` for this provider and endpoint, then an app write drops the entry with no record, so the id is absent again but the confirmed state flipped).
- **Adversarial 8b:** `a_foreign_database_at_user_db_is_refused_not_migrated`.
- **Adversarial 8c:** `building_the_egress_directory_waits_on_no_account` (two Accounts, `user.lock` held and the file not as last recorded: `ProjectDirectory::open` must return in under 3 s). The literal case "`accounts test x` does not wait on y" cannot fail on its own: a write in flight changes the one file's bytes, so x's own check rightly waits (AD-15 *In flight*), and today's `test` already stops after x's single bounded wait. The per-Account wait is in building the directory (`for_accounts`), which this test pins.
- **Adversarial 8d:** `control_characters_in_a_project_path_never_reach_the_terminal` (a project folder named `p<ESC>[2Khidden<CR>shown` naming `cloud`; the add sentence and the remove refusal must hold no raw control character).

No new API name is fixed by these tests: they use `ProjectDirectory::open`, `NetEgress::with_seams`, `am_app::accounts::EgressFactory`, `AccountsContext`, `am_app::runtime::{record_serving, clear_serving}`, `UserStore::{locked, append_audit}`, `UserReader::open`, `am_store::user::user_lock_is_held` and am-config's writer, all existing. The G1 port method (am-domain `EgressDirectory`, routing note) is left to the implementer; the tests observe only transport hops, Keychain read counts, exit codes and words.

**Command:**

```
cargo nextest run -p am-app --test user_accounts -p am-store --test user_store -p am-config --test user_accounts --no-fail-fast
```

```
FAIL am-app::user_accounts an_account_removed_after_the_client_was_built_is_refused_unsent
  assertion `left == right` failed: nothing more reaches the transport after the removal  left: 2  right: 1
FAIL am-app::user_accounts a_hand_edit_after_the_client_was_built_is_refused_unsent
  assertion `left == right` failed: nothing is sent  left: 1  right: 0
FAIL am-app::user_accounts a_change_committed_while_the_client_reads_the_key_is_never_sent
  assertion `left == right` failed: the key read while the Account moved is never sent  left: 1  right: 0
FAIL am-app::user_accounts accounts_test_reads_the_key_once_inside_the_egress_client
  assertion `left == right` failed: one Keychain read, the client's own  left: 2  right: 1
FAIL am-app::user_accounts add_names_where_a_reused_key_was_last_confirmed_even_after_a_removal
  the sentence names the provider the key was last confirmed for: Adding the Account 'router' (provider anthropic) … This Mac has no confirmation of that key for this provider and endpoint …
FAIL am-app::user_accounts a_serving_record_on_a_fresh_mac_leaves_the_app_data_dir_owner_only
  assertion `left == right` failed: the app data dir is owner-only  left: Some(493)  right: Some(448)
FAIL am-app::user_accounts an_interrupted_write_is_named_as_one_and_not_as_an_outside_edit
  agentic: the Account 'cloud' is not called: … (accounts.yaml was changed on disk outside the app since the app last wrote it) …
FAIL am-app::user_accounts accounts_test_never_prints_a_key_pasted_as_secret_ref
  `test` prints the pasted key: … (line 5: secret_ref '<canary>' names another Keychain entry …
FAIL am-app::user_accounts an_egress_refusal_never_returns_or_audits_a_key_pasted_as_secret_ref
  the project's audit chain holds the pasted key: {"action":"store.migrated", … (the egress.blocked entry's reason quotes it)
FAIL am-app::user_accounts validate_never_prints_a_key_pasted_as_secret_ref
  validate prints the pasted key: …/accounts.yaml:5: secret_ref '<canary>' names another Keychain entry …
FAIL am-app::user_accounts a_project_unreachable_since_the_sentence_refuses_the_add
  assertion `left != right` failed: agentic: added the Account 'cloud' (anthropic, https://api.anthropic.com) to this Mac's Accounts …
FAIL am-app::user_accounts a_confirmation_recorded_since_the_sentence_refuses_the_add
  assertion `left != right` failed: agentic: added the Account 'cloud' (anthropic, https://api.anthropic.com) to this Mac's Accounts …
FAIL am-app::user_accounts building_the_egress_directory_waits_on_no_account   [20.6 s]
  the directory waited on the Accounts it was not asked for: 20.043048834s
FAIL am-app::user_accounts control_characters_in_a_project_path_never_reach_the_terminal
  assertion `left == right` failed: the add sentence: …  left: ['\u{1b}', '\r']  right: []
FAIL am-store::user_store the_guard_refuses_every_config_audit_entry
  assertion `left == right` failed: written without an intent  left: ["config.narrowed", "config.edited", "config.widened", "config.created"]
FAIL am-store::user_store a_reader_opened_without_a_wal_sees_a_later_commit
  assertion `left == right` failed: the reader sees the commit made after it was opened  left: 1  right: 2
FAIL am-store::user_store a_foreign_database_at_user_db_is_refused_not_migrated
  a foreign database was opened as the user store (application_id 1095587155, user_version 1, tables ["audit_log", "config_history", "config_intents", "migration_reports", "notes"])
FAIL am-config::user_accounts the_secret_value_fix_names_this_macs_accounts_not_the_old_project_file
  a project file: the fix names the old project file: Finding { file: "seats/writer.md", line: 14, … fix: "keep keys in the Keychain; reference an Account (models/accounts.yaml, secret_ref) instead" }
Summary [21.226s] 73 tests run: 55 passed, 18 failed, 1 skipped
```

The 18 failures are exactly the new tests; every earlier test in the three files passes (the one changed, `the_guard_appends_an_audit_entry_to_the_chain`, included). `cargo clippy -p am-app -p am-store -p am-config --tests -- -D warnings` is clean and the three files are rustfmt-clean.

### Review fixes: test changed after the red run (2026-11-22, coordinator decision)

`accounts_test_reads_the_key_once_inside_the_egress_client` now expects two Keychain reads, not one. Removing `test`'s own read broke four older tests that run `test` on `FakeEgress` (which never reads the Keychain) and rely on it for the "no key in Keychain" words. That read is a presence check whose key is never sent. The finding's harm, a sent key read with no re-check, is fixed in `NetEgress::send`: it reads the stamp before and after its own key read and drops the key unsent if it moved. `a_change_committed_while_the_client_reads_the_key_is_never_sent` proves that. The test's name is kept; its doc and assertion say two reads, never more.

### Follow-up fixes: red run (2026-11-22, tests first, before any `src/` change)

Tests for the `[patch]` rows of the follow-up review pass (Review Triage Log), in the crates' `tests/` folders only; no `src/` file created or changed, nothing committed. The F1 tests drive the real `NetEgress` over am-app's `ProjectDirectory` on the RULE-9 fakes, with a test resolver (`HookResolver`, implementing `am_net::Resolver`) that runs another process's write once, on the first lookup of the Account's host: after the client took the endpoint, before it reads the key.

**Item → test** (`crates/am-app/tests/user_accounts.rs` unless named):

- **F1, endpoint vs stamp:** `an_account_re_pointed_while_its_host_resolves_is_never_sent_to_the_old_host` (during the lookup of openrouter.example: `router` removed with `account_removed`, then added again at elsewhere.example with an `account_added` naming it, reusing the Keychain entry; nothing may reach the transport, the call is refused).
- **F1, no stamp:** `a_hand_edit_while_the_host_resolves_is_never_sent` (one byte appended to the user file during the lookup) and `an_account_removed_while_its_host_resolves_is_never_sent` (`account_removed` recorded during the lookup).
- **F5:** `validate_never_prints_a_key_in_the_user_file_however_placed`, `accounts_test_never_prints_a_key_in_the_user_file_however_placed`, `an_egress_refusal_never_returns_or_audits_a_key_in_the_user_file_however_placed`, each over five placements: `provider: "<key>"`, `id: '<key>'`, `provider: <key> # mine`, `base_url: https://<Mixed_Case_Key>.example/v1` (an underscore makes the host not a DNS name, so the error quotes it lowercased; matched case-insensitively), and a 64-character lowercase-hex `secret_ref`. `test` and the egress call name `cloud`, so the `id` placement is checked by `validate` only. Changed per the coordinator's decision (`secret_ref` is never quoted): `crates/am-config/tests/user_accounts.rs` `the_broken_accounts_fixture_reports_each_error_at_its_line` now expects `someone-else` in neither the message nor the fix.
- **8d:** `bidi_and_invisible_characters_in_a_project_path_are_escaped` (a project folder holding U+202E, U+2066, U+200B and U+2028 that names `cloud`; the add sentence and the remove refusal), and `accounts_list_escapes_an_invalid_entrys_raw_id` (user-file ids `"esc\e[2Kid"` and `"rtl‮id"`). Escaped means no control character (line feed and tab aside), no U+200B–U+200F, U+202A–U+202E, U+2066–U+2069, U+2028, U+2029 or U+FEFF in the output.
- **8b** (`crates/am-store/tests/user_store.rs`): `a_foreign_database_whose_only_table_is_sqlitex_is_refused`.

No new API name is fixed: the tests use `am_net::{NetEgress::with_seams, Resolver, Seams}`, `ProjectDirectory::open`, `StoreAuditSink`, am-config's user writer (`apply_with_audit`, `add_account_edit`, `remove_account_edit`) and `UserStore::locked`.

**Command:**

```
cargo nextest run -p am-app --test user_accounts -p am-store --test user_store -p am-config --test user_accounts --no-fail-fast
```

```
FAIL am-app::user_accounts an_account_re_pointed_while_its_host_resolves_is_never_sent_to_the_old_host
  the key went to the old host: ["https://openrouter.example/api/v1/chat/completions"]
FAIL am-app::user_accounts a_hand_edit_while_the_host_resolves_is_never_sent
  assertion `left == right` failed: nothing is sent  left: 1  right: 0
FAIL am-app::user_accounts an_account_removed_while_its_host_resolves_is_never_sent
  assertion `left == right` failed: nothing is sent  left: 1  right: 0
FAIL am-app::user_accounts validate_never_prints_a_key_in_the_user_file_however_placed
  validate printed a key:
  provider quoted: …/accounts.yaml:4: '<key>' is not a valid provider
  value with a trailing comment: …/accounts.yaml:4: '<key>' is not a valid provider
  key in the base_url host: …/accounts.yaml:5: 'base_url' of an openai-compatible account is not accepted: its host '<key, lowercased>.example' is not a DNS name …
  lowercase-hex key as secret_ref: …/accounts.yaml:5: secret_ref '<hex key>' names another Keychain entry than the Account's own id 'cloud' …
FAIL am-app::user_accounts accounts_test_never_prints_a_key_in_the_user_file_however_placed
  `test` printed a key: key in the base_url host: … its host '<key, lowercased>.example' …; lowercase-hex key as secret_ref: … secret_ref '<hex key>' …
FAIL am-app::user_accounts an_egress_refusal_never_returns_or_audits_a_key_in_the_user_file_however_placed
  a key leaked: key in the base_url host (refusal): … its host '<key, lowercased>.example' …; lowercase-hex key as secret_ref (refusal): … secret_ref '<hex key>' …
FAIL am-config::user_accounts the_broken_accounts_fixture_reports_each_error_at_its_line
  Finding { … line: 13, … message: "secret_ref 'someone-else' names another Keychain entry than the Account's own id 'other'; …" }
FAIL am-app::user_accounts bidi_and_invisible_characters_in_a_project_path_are_escaped
  assertion `left == right` failed: the add sentence: …  left: ['\u{202e}', '\u{2066}', '\u{200b}', '\u{2028}']  right: []
FAIL am-app::user_accounts accounts_list_escapes_an_invalid_entrys_raw_id
  assertion `left == right` failed: "esc\u{1b}[2Kid  (not a valid Account entry; …)\nrtl\u{202e}id  (not a valid Account entry; …)…"  left: ['\u{1b}', '\u{202e}']  right: []
FAIL am-store::user_store a_foreign_database_whose_only_table_is_sqlitex_is_refused
  a foreign database was opened as the user store (application_id 1095587155, user_version 1, tables ["audit_log", "config_history", "config_intents", "migration_reports", "sqliteX", "sqlite_sequence"])
Summary [10.789s] 82 tests run: 72 passed, 10 failed, 1 skipped
```

The 10 failures are exactly the new and changed tests; every other test in the three files passes, the first review round's tests included. Placements that do not leak today are kept as guards: `id: '<key>'` in `validate`; the egress audit export, which holds no key for any placement (only the returned refusal words leak); and the quoted-provider and trailing-comment placements in `test` and egress. `cargo clippy -p am-app -p am-store -p am-config --tests -- -D warnings` is clean and the three files are rustfmt-clean.

### Second follow-up fixes: red run (2026-11-22, tests first, before any `src/` change)

Tests for the `[patch]` rows of the second follow-up review pass (group FU6, hiding by membership), in `crates/am-app/tests/user_accounts.rs` only; no `src/` file created or changed, nothing committed. Each test loops over these placements of a canary key in the user Accounts file:

- **(a)** the same prefixed key twice as a mapping key inside `cloud`'s item (a YAML duplicate-key parse error);
- **(b)** `provider: "openai' <key> '"`;
- **(c)** `id: <key>` for a 40-character key with no known prefix, mixed case with underscores;
- **(d)** `cloud` with `provider: <unprefixed key>` and a `description: |` block scalar holding the line `id: <same key>`. This is constructible: the provider error quotes the key, and the block-scalar `id:` line puts it on the keep-list.

`test` and the egress call name `cloud`, so they cover (a), (b) and (d); `validate` and `list` cover all four.

**Tests:** `fu6_validate_never_prints_a_key_in_the_user_file`, `fu6_accounts_test_never_prints_a_key_in_the_user_file`, `fu6_accounts_list_never_prints_a_key_in_the_user_file`, `fu6_an_egress_refusal_never_returns_or_audits_a_key_in_the_user_file` (returned words and the project's audit export), and **(e)** `fu6_list_names_an_invalid_entry_by_its_line_not_its_id`. In (e), an invalid entry whose id is a key (`- id: <prefixed key>` with no provider, line 6) must be named by `line 6`, never by its id. No new API name is fixed.

**Command:**

```
cargo nextest run -p am-app --test user_accounts -p am-store --test user_store -p am-config --test user_accounts --no-fail-fast
```

```
FAIL am-app::user_accounts fu6_validate_never_prints_a_key_in_the_user_file
  validate printed a key:
  (a) …/accounts.yaml:7: not valid YAML: duplicate mapping key: <key>, set DuplicateKeyPolicy in …
  (b) …/accounts.yaml:4: 'openai' <key> '<value hidden>' is not a valid provider
  (c) …/accounts.yaml:3: '<unprefixed key>' in 'id' is not a kebab-case id
  (d) …/accounts.yaml:4: '<unprefixed key>' is not a valid provider
FAIL am-app::user_accounts fu6_accounts_test_never_prints_a_key_in_the_user_file
  `test` printed a key: (a) agentic: accounts.yaml (…) cannot be read as Accounts because it is not valid YAML at line 7: duplicate mapping key: <key>, …
FAIL am-app::user_accounts fu6_accounts_list_never_prints_a_key_in_the_user_file
  `list` printed a key: (a) … duplicate mapping key: <key> …; (c) <unprefixed key>  (not a valid Account entry; `agentic validate` names the line)
FAIL am-app::user_accounts fu6_an_egress_refusal_never_returns_or_audits_a_key_in_the_user_file
  a key leaked: (a) (refusal): the Account cloud is not called: accounts.yaml (…) cannot be read as Accounts because it is not valid YAML … duplicate mapping key: <key> …
FAIL am-app::user_accounts fu6_list_names_an_invalid_entry_by_its_line_not_its_id
  `list` named the entry by its id: … <key>  (not a valid Account entry; `agentic validate` names the line)
Summary [10.739s] 87 tests run: 82 passed, 5 failed, 1 skipped
```

The 5 failures are exactly the new tests; every earlier test in the three files passes (all guards included). Placements that do not leak today are kept as guards: (b) and (d) in `test` and in the egress refusal, and every placement in the project's audit export. `cargo clippy -p am-app --tests -- -D warnings` is clean and the file is rustfmt-clean.

### Second follow-up fixes: tests changed after the red run (2026-11-22, coordinator decision)

Hiding by membership hides more than quote pairing did, so three earlier expectations conflict with it. Each now follows the rule: the finding is located by its line and field, and the value is not shown.
- `accounts_list_escapes_an_invalid_entrys_raw_id` expects "the entry at line 3" and "line 5", and still no hidden characters.
- `a_newer_file_and_a_bad_id_are_errors` expects the `'id'` error at line 3 without `Bad_Id`. A non-kebab id has the same shape as a key with no prefix.
- `the_broken_accounts_fixture_reports_each_error_at_its_line` expects "unknown key" at line 6 without `api_key`.

Implementer addition accepted: `localhost` is kept as one of the app's own message words.

## Auto Run Result

**Status:** built (2026-11-22).

**Summary:** Accounts are user-level. One file, `<app data dir>/accounts.yaml`, lives beside a user store, `user.db`, which has its own migration set and `application_id` and is written only under `user.lock` (bounded 10 s wait).
- **Commands:** `accounts add`, `remove`, `list` and `test` work on the user list.
  - They open no project and take no `--project`.
  - They scan the projects this Mac knows for references, and are refused while a reachable project runs a child as the owner.
  - `remove` refuses while a project could not be reached, until `--force`.
  - `add` asks for the stored key at the hidden prompt when it reuses an unconfirmed Keychain entry.
- **Key use:** one read-only Key-use check (`key_use`) runs on every Account answer of the egress directory. `NetEgress::send` takes the Account's stamp (file hash, record sequence, confirmed endpoint) before resolving and again after its own key read. The key is sent only when the two are present and equal and the endpoint matches.
- **Validate:** an old `models/accounts.yaml` is an error with the migrate fix and is never read; an id missing on this Mac is a warning.
- **Templates and fixtures:** they carry no `models/accounts.yaml`, except `fixtures/projects/legacy-accounts/`, and Accounts variants live in `fixtures/accounts/`.
- **Messages:** user-Accounts messages never repeat a file value other than a valid id, a provider name or a field name.

**Files changed (src):**
- **am-domain:**
  - `runtime.rs`: `AppDataLayout`.
  - `net.rs`: `AccountStamp` and `EgressDirectory::account_stamp`.
- **am-store:**
  - `lib.rs`: the shared executor and `latest_config_entry`.
  - `migrations.rs`: migration sets, with `application_id` set inside the first migration.
  - `user.rs` and `migrations/user/0001_user.sql`: the user store and its reader.
  - `project.rs`: the project store's `application_id`.
- **am-config:**
  - `user_accounts.rs`: the loader, `key_ref` and membership hiding.
  - `account_edit.rs` and `writer.rs`: the user-file writer mode.
  - `kinds.rs` and `refs.rs`: references against the user list, and the legacy-file error.
  - `snapshot.rs`, `entities.rs`, `checks.rs`, `base_url.rs` and `lib.rs`.
- **am-net:**
  - `lib.rs`: stamp checks around the key read, on retries and on same-origin redirects, and a fixed audit reason for refused Accounts.
- **am-app:**
  - `user.rs`: permissions, the lock protocol, the reference scan and escaping.
  - `key_use.rs`: the Key-use check and the stamp.
  - `accounts.rs`: the commands on the user list.
  - `egress.rs`: `ProjectDirectory` with no Account cache.
  - `cli.rs`: validate with this Mac's list.
  - `runtime.rs`: places, and the app data dir created 0700.
  - `main.rs`.
- **xtask:**
  - `runtime_paths.rs`: `user.db` and `user.lock` literals are allowed only in `runtime.rs`.

**Review:** three passes, recorded in the Review Triage Log.

| Pass | Findings | Patched | Deferred | Rejected |
|---|---|---|---|---|
| First (quick lens and adversarial lens) | 19: high 3, medium 9, low 5 | 17 | 1 | 0 |
| Follow-up (adversarial) | 11 | 7 | 1 | 3 |
| Second follow-up (narrow) | 5 | 4 | 0 | 1 |

- **First pass deferred:** the app data dir comes from `$HOME`; same-user, owner call.
- **Follow-up deferred:** synchronous checks in the async send path, until epic 2 story 6.
- **Rejected:**
  - `test`'s presence-only Keychain read: a coordinator decision.
  - am-net's public `check_caller` NUL marker: no caller.
  - `account_stamp` `None == None`: superseded, fixed under FU1.
  - Key-use words quoting a host the owner typed: reachable only for an app-written file.
- **Deferred items:** three, in the frontmatter. They also include the 2.2 note pairing by timestamp.
- **Every patch round ran tests first**, with a red run logged before any `src/` change.

**Follow-up review:** not recommended. The last pass patched four mediums and no high. Patched by verdict over the run: high 6, medium 15, low 7.

**Verification:** `just check` is green, 848 tests passed and 8 skipped. It covers fmt, clippy `-D warnings`, the doctests, check-deps, check-forbid, the denylist, runtime-paths, fixed-hosts, seam-guard, and `agentic validate` over templates and fixtures. The canary scans are the tests' own (`assert_no_key_leaked` and the `..._however_placed` and `fu6_*` loops).

**Residual risks:**
- There is a small window between the final stamp comparison and the transport's send, which only a lock could close.
- The deferred items above.
