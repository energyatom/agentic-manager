# Review 2026-11-22: global (user-level) Accounts (versions / reality check)

Scope: the working-tree `git diff` of `architecture-agentic-manager.md` in `/Users/hiteshmamgain/Developer/agentic-manager-replan` (base `afff3b6`): the 2026-11-22 "global Accounts" amendments to AD-7, AD-13 (end), AD-15 (widening list, *Interim CLI Account edit*, *Change*, *Migrate*, *Record*, *Key use*, *Later confirmations*, *Retirement*), AD-16, AD-17 (*User-level Accounts*), AD-19, AD-31, AD-32, AD-35, AD-48, the Structural Seed, Deferred and Open Questions. Lens: is each committed decision checked against (1) the code built on this tree (stories 2.1 and 2.3), (2) the platform facts it relies on (Keychain sharing, SQLite multi-process writes, App Sandbox), and (3) the Stack (no new crate or version)? `_archive/` was not read.

## Verdict

**PASS WITH FINDINGS.** The brownfield statements are accurate: stories 2.1 and 2.3 built the per-project stage exactly as the spine now describes it, am-secrets' naming (`agentic-manager` / `<account-id>`) is unchanged so no Keychain migration is needed, the app data dir path matches the code, and no crate or version is introduced. The gaps are in the new multi-process part. The spine moves one store from "one process holds it under an exclusive lock for its lifetime" to "several processes take turns under `user.lock`", but the code it reuses (am-config's writer reconcile, am-store's open-time migration and deferred transactions, the watcher's in-memory own-write test) was built for the first model. One high finding (say which user-store writes hold the lock and how transactions begin), five medium, five low and two nits. All are fixable with a sentence in the spine or a line in Sofia's guidance for the new story. None reverses an owner decision.

Counts: critical 0, high 1, medium 5, low 5, nit 2.

## 1. Spine against the code on this tree

| Spine claim | Code | Match |
|---|---|---|
| Keys stored under service `agentic-manager`, account `<account-id>`; "the naming is unchanged, so no keychain entry is migrated" | `crates/am-secrets/src/lib.rs:12` (`SERVICE`), `:46-51` (`Store::build(service, account)`); `crates/am-config/src/account_edit.rs:254-264` writes `secret_ref: <id>`; `accounts.rs` reads only the entry named by the Account's own id | Yes |
| `SecretStore` wraps keyring-core + apple-native-keyring-store (`keychain` feature) | `crates/am-secrets/Cargo.toml`; the `keychain` module is the **file-based** keychain (`security_framework::os::macos::keychain::SecKeychain`, `apple-native-keyring-store-1.0.2/src/keychain.rs:47`) | Yes (see F5 for what file-based implies) |
| "am-secrets is unchanged" (Sofia guidance) | Nothing in am-secrets is per-project | Yes |
| Stories 2.1 and 2.3 built the edit per project on `models/accounts.yaml` and the project store | `crates/am-config/src/account_edit.rs:33` (`ACCOUNTS_FILE = "models/accounts.yaml"`); `crates/am-app/src/accounts.rs:1-73, 531-561` (project opened read-only under its store lock), `:1097-1163` (`confirmed`: file sha256 = latest History after-hash, latest `account_added` note names provider, scheme, host, port) | Yes |
| The `account_added` record shape (file, id, provider, scheme, host, port, references, `confirmation: typed_id`, never the key) | `accounts.rs:815-835`: audit action `config.widened`, `args.kind = account_added` | Yes. "Key state" is new: today's notes carry none, which the spine reads as `key: stored`, so it is compatible |
| App data dir is `~/Library/Application Support/agentic-manager/` | `crates/am-domain/src/runtime.rs:31-34` (`APP_DATA_NAME`); `crates/am-app/src/runtime.rs:96-128` (`Places::under` / `from_env`; `$XDG_DATA_HOME/agentic-manager` off macOS) | Yes |
| "`am_domain::RuntimeLayout` names these paths as it names the runtime folder's" | `RuntimeLayout` (`am-domain/src/runtime.rs:146ff`) is the per-runtime-folder type (`at(dir)`, `lock()`, `state_db()`, …). App data dir names are loose consts (`APP_DATA_NAME`, `MAC_ID_FILE`) that am-app's `Places` composes (`runtime.rs:83-128, 213-245`) | **No.** This is new work, and `RuntimeLayout` is the wrong type for it (F3) |
| App data dir "0700, files 0600" (Seed), and the server refuses to start otherwise (AD-13) | am-app creates the dir 0700 only when it is missing (`runtime.rs:155-169`) and never checks an existing one. am-store's `check_private` (`am-store/src/project.rs:176-218`) covers only the runtime folder and its kept legacy folders. The server-start check is story 3.2's (`epic-workflow-engine-cli` story 2, "refuses loose permissions"), not yet built | **Claimed, not built.** The user-level story builds before 3.2, so it needs its own check (F4) |
| User store "WAL, `synchronous=FULL`, forward-only migrations, one writer at a time" | `am-store/src/lib.rs:483-489` (`configure`: WAL, FULL, foreign_keys, `busy_timeout` 5 s); one writer **thread** per process (`:498-546`); each command runs in `conn.transaction()` (`:621`), which is rusqlite's DEFERRED | Partly. Within one process, yes. Across processes there is nothing yet, and deferred transactions are the wrong default (F1) |
| User store pre-migration backups in `<app data dir>/backups/pre-migration/` | `Store::open(path)` already puts backups at `RuntimeLayout::at(parent).backups_pre_migration()` (`lib.rs:498-502`), which is `<app data>/backups/pre-migration/` when `path` is `<app data>/user.db` | Yes (by construction) |
| Lock files | Project lock: `std::fs::File::try_lock` on `<runtime>/lock`, held by the writer thread for the store's life (`project.rs:220-240`, `lib.rs:505-531`) | The mechanism exists. A per-write `user.lock` is new (F1, L3) |
| Templates and fixtures carry no `models/accounts.yaml` | Seven exist today: `templates/samples/company-a/models/accounts.yaml` and `fixtures/projects/{valid,anatomy,broken,broken-anatomy,sources-ok,sources-broken}/models/accounts.yaml` | New work, and Sofia's guidance (1) names it. Fine |

## 2. Platform facts (checked 2026-11-22)

**Keychain: shared by one service and account across processes.** *Partly confirmed.*
- TN3137 (Apple): "The file-based keychain uses access control lists (`SecAccess`)"; "The data protection keychain uses keychain access groups"; data-protection access groups come from code-signing entitlements "authorized by a provisioning profile", which a command-line tool can only carry inside an app-like bundle. The file-based keychain is "on the road to deprecation".
- Apple DTS (developer forums, file-based ACL threads): by default an item's ACL lists one app, the one that created it, by its designated requirement (DR). If an executable has no stable DR, the keychain cannot tell that version N+1 is the same program as version N. Partition lists (10.12+) are a second, independent check.
- Consequence: every process of the **same executable** (`agentic accounts`, `agentic serve`, the `agentic service` LaunchAgent) reads one item with no prompt. A **different executable**, for example the Tauri app, which links am-app in-process (AD-32 "no sidecar"), gets a prompt for an item the CLI created. So does an ad-hoc-signed dev build after each rebuild. *Not confirmed by experiment* (that needs a signed build, spike criterion 8, deferred). This applied to the per-project design too, but the amendment's "one entry … serves every project on this Mac" is true only per executable (F5).

**SQLite multi-process writes with WAL plus a lock file.** *Confirmed (official docs).*
- wal.html: all processes must be on the same host (WAL "does not work over a network filesystem"), and "there can only be one writer at a time". `~/Library/Application Support` is local, so this holds.
- lang_transaction.html: "The default transaction behavior is DEFERRED … Subsequent write statements will upgrade the transaction to a write transaction if possible, or return SQLITE_BUSY."
- rescode.html: `SQLITE_BUSY_SNAPSHOT` "occurs on WAL mode databases when a database connection tries to promote a read transaction into a write transaction but finds that another database connection has already written"; and "To avoid encountering SQLITE_BUSY errors in the middle of a transaction, the application can use BEGIN IMMEDIATE … if it succeeds, then SQLite guarantees that no subsequent operations … through the next COMMIT will return SQLITE_BUSY."
- *Not confirmed from official text:* whether `busy_timeout` retries a `BUSY_SNAPSHOT`. The pages do not say. Design as if it does not, which is what F1 asks for.
- `-wal`/`-shm` take the database file's mode: stated in `docs/crate-notes/rusqlite.md`, not on wal.html. It is checked empirically by `crates/am-app/tests/runtime_open.rs:601-610` (every file in the runtime folder is 0600).
- Lock file: Rust std `File::lock`/`try_lock` (stable 1.89; the pinned toolchain is 1.100.0) "corresponds to the `flock` function on Unix", and the lock is "released when this file (along with any other file descriptors/handles duplicated or inherited from it) is closed". A separate `user.lock` file (not a lock on `user.db` itself) avoids SQLite's own POSIX-lock fd-close hazard. Good choice.

**`~/Library/Application Support/agentic-manager/` denied to an App Sandbox helper with only `com.apple.security.app-sandbox`.** *Confirmed in general terms, not for this path.*
- Apple, "Accessing files from the macOS App Sandbox": macOS gives a sandboxed app "unrestricted access to limited parts of the Mac's file system". It "has full read and write access to its sandbox container" (`~/Library/Containers`). Wider access needs user selection, bookmarks, app groups, standard-folder capabilities or Full Disk Access. The page does not name `~/Library/Application Support/<other>` explicitly.
- The spine already guards this with the right empirical check: the sandbox self-test opens a canary in the app data dir and fails closed (AD-13), and epic 7's verify asserts EPERM. No new claim is needed. The user store and Accounts file sit in that same directory, so they are covered by the same canary.

## 3. Versions

No crate, version or feature is added or changed by the amendments. What the new text needs already exists in the Stack: rusqlite `=0.43.0` (`Connection::transaction_with_behavior(TransactionBehavior::Immediate)` for F1), std file locks (toolchain 1.100.0 ≥ 1.89), keyring-core `=1.0.1`, and apple-native-keyring-store `=1.0.2` with feature `keychain`. Watch-outs if F5 is acted on: moving to the data protection keychain is the same pinned crate's `protected` feature, which needs a Stack row change and an entitlement or provisioning-profile decision (AD-13 helper entitlements stay untouched). Editing a file-based item's ACL is not exposed by keyring-core 1.0.1, and doing it would make `security-framework` (today transitive) a direct dependency to pin.

## Findings

### H1 (high): multi-process writes to the user store: the lock scope and transaction mode are under-specified against code built for one owning process
The spine says "a command writing it holds `<app data dir>/user.lock` for that write only, so several processes … take turns". The code this will reuse assumes one process owns the store for its whole life:
- `am-config` `ConfigWriter::reconcile` (`crates/am-config/src/writer.rs:843-858`, run on every open) **drops** each pending intent whose file does not yet hold its new bytes. If process B opens the user store while process A sits between `begin_intent` and its file rename, B drops A's intent, and A's `commit_entry` then fails or leaves a file with no History (an "outside change" that refuses every key).
- `Store::open_reporting` migrates at open (`am-store/src/lib.rs:508-520`), so two processes starting after an update race on the migration.
- Every command runs in a DEFERRED transaction (`lib.rs:621`), and audit appends read the chain head before inserting. Any user-store write path that skips the lock gets `SQLITE_BUSY`/`BUSY_SNAPSHOT` mid-transaction, and the busy timeout is not documented to help.

**Fix (spine, AD-17 user store sentence):** "Every write to `user.db` holds `user.lock`: the open-time migration, the settling of pending intents, and each intent, History and audit commit. User-store transactions begin IMMEDIATE. A process never settles or drops a user-store intent without the lock." Add the same line to Sofia's guidance (1) and to AD-6 (see M2).

### M1 (medium): *Key use* refuses during another process's in-flight write, and after a crash until someone settles
The write protocol is intent commit, then file rename, then History commit (`ConfigHistory`, `crates/am-domain/src/config_history.rs:159-181`). Between the rename and the History commit, any other process's `accounts test`, Doctor or run start sees bytes that match no History entry, and refuses with "changed outside the app". The watcher's own-write test is in-memory per process (`writer.rs:888-892`, `session.rs:629`), so another process's watcher announces the same window as an outside change. After a crash in that window, keys stay refused until some process settles the intent, and the spine does not say which process settles or when.
**Fix:** "A user-file hash that matches a pending intent's after-hash, or that is seen while `user.lock` is held, is in flight: the reader waits for the lock (bounded) and checks again before calling it an outside change. Any process that opens the user store settles its pending intents under `user.lock`."

### M2 (medium): AD-6 still says "One writer task owns the only read-write connection"
With the user store, each process has its own read-write connection to `user.db`. AD-6 was not amended, so its rule text and AD-17 now disagree.
**Fix:** add to AD-6: "The user store (AD-17) has one writer task per process; across processes, writes are serialized by `user.lock` and IMMEDIATE transactions."

### M3 (medium): "`am_domain::RuntimeLayout` names these paths" does not match the type
`RuntimeLayout` is one runtime folder's layout. `RuntimeLayout::at(app_data).lock()` would name `<app data>/lock` and `state_db()` would name `state.db`, not `user.lock`/`user.db`. Today the app data dir's names are loose consts in `am_domain::runtime`, which am-app's `Places` composes. The Seed and Sofia's guidance send the story to put the names into `RuntimeLayout`.
**Fix:** "`am_domain::runtime` names them in an app-data layout beside `RuntimeLayout` (`accounts_file`, `user_db`, `user_lock`, `backups_pre_migration`, `mac_id`), and am-app's `Places` resolves the directory." Apply the same wording in the Seed and in Sofia's guidance (1).

### M4 (medium): the app data dir's 0700/0600 rule is claimed but not built, and the user-level story builds first
The only check that exists covers the runtime folder (`am-store/src/project.rs:176-218`). The app data dir is created 0700 when it is missing and never checked when it already exists (`am-app/src/runtime.rs:155-169`). The server-start check is story 3.2 (not built), and the CLI `accounts add|change|remove|migrate` never starts the server.
**Fix (Sofia guidance (1)):** "am-store opens `user.db` only when the app data dir has no group or other permission bits (refused in words, as `prepare_dir` does for the runtime folder), and `accounts.yaml`, `user.db` and `user.lock` are created 0600."

### M5 (medium): "one entry serves every project on this Mac" holds per executable, not per Mac (file-based Keychain ACL)
am-secrets uses the file-based keychain, whose items are ACL-protected and by default trust only the creating executable's DR (§2). The CLI (`agentic`) is the only add path until story 5.5. The Tauri app is a different executable and will prompt on first read of each key, and adding it to the trusted list needs the login keychain password. Ad-hoc-signed dev builds prompt after each rebuild.
**Fix:** record it as a risk under AD-17 or Deferred, owned by E11 story 3 or the signed-build check (criterion 8): verify on the signed build that the desktop app reads a key the CLI stored. Name the options as a later owner decision: the data protection keychain (`protected` feature; entitlement or profile), or an ACL listing both executables at creation. Do not decide it now.

### L1 (low): the Seed lists `services/searxng/` under the app data dir
That contradicts AD-13 (sandboxed helpers are denied the app data dir) and AD-45 (on macOS the state folder is the helper's sandbox container). AD-32's "holds only" list omits it.
**Fix:** drop it from the app data dir line, or annotate it "(off macOS; on macOS the helper's container, AD-45)".

### L2 (low): `user.db`'s schema is unstated
`Store::open` today applies the project migration set (`SCHEMA_VERSION`).
**Fix:** "The user store has its own numbered migration set and its own `application_id`, so neither database opens as the other."

### L3 (low): lock order and waiting
`migrate` holds a project lock, then `user.lock`, and a served project holds its lock while it briefly takes `user.lock`. The order is consistent, but the spine does not say so. `File::lock` blocks without limit.
**Fix:** "Take a project lock before `user.lock`, never the reverse. `user.lock` is taken with a bounded wait, then refused naming the holder."

### L4 (low): one shared `user.db` makes version skew global
A newer binary that migrates `user.db` makes every older running process, for example a LaunchAgent `agentic serve` started before an update, refuse to read it (AD-7). Keyed calls then stop for every project.
**Fix:** "An older process that finds a newer user store refuses key use in words ('restart the app or service')."

### L5 (low): `migrate` may reuse a key that belongs to another provider
Under 2.1 and 2.3, two projects could share one id with different providers and one Keychain entry (the C1 case). After a rename, `migrate` reuses the entry "stored by the per-project stage", which may hold the other provider's key.
**Fix:** when the runtime base's reachable projects show that id with more than one endpoint, `migrate` offers to replace the key before reusing it.

### N1 (nit)
`remove`'s message still says the key stays "because another project may use the same id" (`accounts.rs:959`). The spine dropped that reason, so the new story should reword the message.

### N2 (nit)
The spine names the remove record `account_removed` but not its audit action. Today `account_added` is `args.kind` under action `config.widened`, and a remove is not a widening.
**Fix:** name the action, for example `config.account_removed` or `config.edited` with `kind: account_removed`.

## Sources
- Apple, TN3137 "On Mac keychain APIs and implementations": https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
- Apple Developer Forums (DTS), file-based keychain ACL and partition lists: https://developer.apple.com/forums/thread/845980 , https://developer.apple.com/forums/thread/746931
- Apple, "Accessing files from the macOS App Sandbox": https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox
- SQLite, Write-Ahead Logging: https://www.sqlite.org/wal.html
- SQLite, BEGIN TRANSACTION: https://www.sqlite.org/lang_transaction.html
- SQLite, result codes (SQLITE_BUSY, SQLITE_BUSY_SNAPSHOT): https://www.sqlite.org/rescode.html
- Rust std `File::lock` / `try_lock`: https://doc.rust-lang.org/std/fs/struct.File.html
