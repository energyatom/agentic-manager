# Adversarial review — owner answers 2 amendment (2026-11-22)

Scope: today's uncommitted diff of `architecture-agentic-manager.md` (AD-13 condition and `$HOME` residual, AD-15 *Terminal*, *References*, *Change*, AD-17 *Recovery*, AD-38, Deferred rows, Open Questions "Resolved"). The owner's five answers are taken as final; findings attack only the wording added today and what it does to the rest of the spine. Ticket 9.19 and the current `am-app` code were read only as evidence of how a builder reads the text.

**Verdict: revise before commit.** Four high findings (one is a build-order contradiction, one a data-integrity bug in "Set aside", one a factual claim that does not hold in v1, one a deferral that points at a function that does not exist yet). No critical finding.

---

## High

### H1 — *Recovery* offers a CLI command that *Retirement* has already removed

Spine (AD-17 *Recovery*): "offers each Account's normal confirmation (AD-15: `change <id>`'s first confirmation in the CLI, … and the Account card from epic 5 story 5)".
Spine (AD-15 *Retirement*): "when story 5.5 ships, … `add`, `change`, `remove` and `migrate` are removed while `list` and `test` stay."

Problem: *Recovery* is built by epic 9 story 19, and the board builds epic 5 before epic 9 (9.19's `after` includes "5.5"). When 9.19 ships, `agentic accounts change` does not exist. 9.19's ticket copies the mistake ("the row also names `agentic accounts change <id>`"). Two units that follow the spine to the letter disagree: 5.5 deletes the command, 9.19 tests a Doctor row that names it.

Fix wording (AD-17 *Recovery*): "…and offers each Account's normal confirmation: the UI Account card's first confirmation (epic 5 story 5; AD-15 *Change*'s rule, a new key typed, the entry replaced by delete then set). The CLI `change` is retired by then (AD-15 *Retirement*) and is not named." Sofia restates 9.19's description and verify the same way.

### H2 — "Set aside" moves only `user.db`; with WAL and live connections it is neither byte for byte nor safe

Spine (AD-17 *Recovery*): "a store that is unreadable or not a user store is only moved aside, byte for byte, to an `AppDataLayout` path after the owner's confirmation, so am-app can create a store marked `recovered`".
Spine (AD-17 *List*): user store is WAL; "a command writing it holds `<app data dir>/user.lock` for that write only, so several processes … take turns."

Problems:
1. The user store is a file set (`user.db`, `user.db-wal`, `user.db-shm`). Moving `user.db` alone and letting am-app create a new `user.db` beside the old `-wal` is SQLite's documented "mispaired hot journal" case: the old WAL's frames can be replayed into the new store, resurrecting the old store's rows (confirmation records included) inside a store marked `recovered`, or corrupting it. Leaving the sidecars in place also breaks "deletes nothing / byte for byte" for the set-aside copy, which then lacks its committed WAL frames.
2. `user.lock` is held only for writes, so another process (a second `agentic serve`, the desktop app) may still hold an open connection. After a rename that process keeps reading and writing the moved inode, so the set-aside copy changes after its sha256 was checked, and the two processes now use two different stores.
3. Who moves it is unstated (Doctor runs in the server; only am-app creates the store; `AppDataLayout` has no named set-aside path), and so is the decision class (a CLI-token route? UI only?).

Fix wording: "Set aside… (UI class, the confirm token) is done by am-app through `AppDataLayout::set_aside_dir()` (`<app data dir>/set-aside/<UTC timestamp>/`, 0700): holding `user.lock` and an exclusive `user.open.lock` (every process that opens the user store holds it shared for as long as the store is open, and Set aside refuses, naming the holder, while one does), it moves `user.db` and any `-wal`, `-shm` or `-journal` beside it together by rename, checks each moved file's sha256 against the one shown in the confirmation, and only then lets am-app create the new store marked `recovered`. A set-aside folder, like the user Accounts file, makes am-app mark a store it creates `recovered`."

### H3 — The AD-13 condition is stated as true in v1, but an owner-added MCP server lets agents start processes, and forgery needs no terminal

Spine (AD-13): "no seat, step, tool, skill, connector, trigger or the Assistant can start a process or open a terminal. In v1 that holds: there is no shell or process tool, … a stdio MCP server is the owner's own program (AD-26), and every `agentic accounts` command is refused while any reachable project enables one (AD-15 *Open*)."

Problems:
1. A tool assigned from an MCP server is a "tool" in this sentence. An owner who adds a stdio server whose tools run commands (a common kind) gives seats, driven by tainted prompts, a way to start processes. So the claim "In v1 that holds" is false whenever such a server is assigned. The mitigation named (the CLI refusal) covers only the pseudo-terminal route. The same agent can **write the user Accounts file, `user.db` and its audit rows directly** (the forgery AD-13 already accepts for "an owner-uid child (a stdio MCP server…)"), and no refusal of `agentic accounts` touches that.
2. AD-26 also allows streamable HTTP MCP servers, and an owner may run one on loopback that executes commands. It is not a child, so neither the CLI-class narrowing nor the *Open* refusal ("enables a stdio MCP server or a Source with `render: true`") triggers.
3. "every `agentic accounts` command is refused" is wrong: *Key use* says `test` "is not a CLI-class decision, so the stdio MCP and `render: true` refusal does not apply to it", `list` is the same, and *Open* refuses `migrate` only over its own project's config ("refused while that project's config enables a stdio MCP server").
4. Nothing enforces the condition. A later story that adds a tool calling `std::process::Command` (or `process-wrap`) in am-tools obeys every AD as written and silently voids the residual.

Fix wording (AD-13): "**That acceptance rests on one condition**: no built-in seat, step, tool, skill, connector, trigger or the Assistant can start a process or open a terminal, and only am-mcp, am-extract's launcher, am-render, am-services and am-app start processes (`cargo xtask check-deps` refuses `std::process::Command`, `process-wrap` and `nix::pty` elsewhere). An MCP server the owner adds is the owner's own program: when its tools run programs, the seats it is assigned to can act as the owner on this Mac (forge an Account confirmation or drive `agentic`), which is inside the accepted residual and is said in the MCP server's add and approval sentence (AD-26). While any reachable project enables a stdio MCP server, a streamable-HTTP MCP server on a loopback address, or a Source with `render: true`, `agentic accounts add`, `change`, `remove` and `migrate` are refused (AD-15 *Open*); `list` and `test` decide nothing and stay." Extend AD-15 *Open* and the CLI-class narrowing to loopback HTTP MCP servers and to every reachable project for `migrate`.

### H4 — The `--as` deferral points 2.13 at `is_widening`, which epic 4 story 17 builds, and AD-15 already decides the answer

Spine (Deferred, new row): "The plan decides it inside `am_config::is_widening` and AD-15, never by a second path, and records it here | E2 story 13 plan".
Spine (AD-15 *Migrate*, unchanged): "the new id carries the file entry's own provider and endpoint, so the rename keeps each reference's destination and is not widening".
Spine (AD-15): "From story 17 the writer classifies the patch with `is_widening`".

Problems:
1. Build order: 2.13 comes before epic 4 story 17. The 2.13 plan cannot decide anything "inside `am_config::is_widening`"; the function does not exist yet.
2. AD-15 *Migrate* still states the answer ("is not widening") that the new row says is undecided, and the row's own text leans the other way ("so the ids are not the same"). A 2.13 builder can cite either sentence.
3. Once 4.18 exists, "naming an Account its approved grant set does not list" holds a project whose grant set records the old id and whose seats now name the new id. That conflicts with "not widening" unless the rule says how 4.18 treats a `--as` rename (`migrate` stays usable from 4.18 until 5.5).
4. "records it here" tells a story plan to edit the spine, which AGENTS.md forbids ("Never edit … architecture … to fit the code; a needed change goes to Sofia").

Fix wording: (AD-15 *Migrate*) replace "and is not widening" with "and its classification is decided by epic 2 story 13's plan (Deferred)". (Deferred row) "The 2.13 plan proposes the rule in words with its test, including how epic 4 story 18's grant-set check treats a reference renamed by `--as`. The architect records it in AD-15 *Migrate* through a REPLAN before 2.13's code lands. Epic 4 story 17 encodes it in `is_widening`'s `ConfigMigration` classification, never by a second path."

---

## Medium

### M1 — A recovered store has no History baseline; *Open* and *Key use* read "outside change" differently

Spine (AD-15 *Open*): "They are refused while the user Accounts file has an outside change (its bytes differ from the after-hash of its latest History entry in the user store)". *Key use*: the file's bytes must "hash to the after-hash of its latest History entry".

Problem: after *Recovery*, the new store holds no History entry for an existing file. One builder reads that as an outside change, which refuses the very confirmation *Recovery* offers until a Reload. Another reads it as "never app-written, not judged", as `am-app` does today (`run_cli.rs`: "one it never wrote has no History entry, and a hand edit of it is not refused"). The 5.5 card and 9.19's verify (a) then disagree on whether Confirm… works.

Fix wording (AD-17 *Recovery*): "A store am-app creates marked `recovered` holds no History entry for the user Accounts file. The file is then not an outside change for AD-15 *Open*, and every key use is refused by the missing confirmation record. The first confirmation writes the file's first History entry."

### M2 — Doctor cannot run when the user store cannot be opened

Problem: am-store refuses an unreadable, foreign or newer user store. The spine never says whether `agentic serve` or the desktop app starts without one, yet *Recovery*'s fixes live in a Doctor row in the UI. If startup refuses, the row is unreachable. If it does not, the spine has to say what runs.
Fix wording: "With the user store missing a usable file, am-app serves with no user store reader: every key use is refused before any keychain read, every Account edit is refused naming Doctor, and Doctor's user-store row is available."

### M3 — "while the keychain stays the owner's" is contradicted by the repo's own tests

Spine (AD-13): "points it at an app data dir that process wrote, with its own Accounts file and user store, while the keychain stays the owner's".
Evidence: `crates/am-app/tests/accounts_tty.rs` says "With HOME in a temporary folder macOS finds no default keychain", so the setup creates one. The legacy keychain's default and search list resolve under `HOME`.
Fix wording: "…and the keychain it sees is the one macOS resolves for that `HOME` (the owner's only if that process arranged it). Epic 11 verifies this on the signed build with AD-17's keychain-access checks. Spawned-binary tests rely on a temporary `HOME` having no default keychain and create their own."

### M4 — The `$HOME` text contradicts itself and leaves out cloud roots

Spine: "derives the app data dir … from `HOME` (`XDG_DATA_HOME` off macOS) … still no env var, flag or setting that names another app data dir".
Problems: `HOME` and `XDG_DATA_HOME` are env vars that name it, so a guard test written from the sentence fails. "still" points at a rule that exists nowhere else in the spine. `HOME` also sets the cloud folder roots (`~/Library/CloudStorage`, iCloud; `Places::under`), which the residual does not list.
Fix wording: "…and no env var other than `HOME` (`XDG_DATA_HOME` off macOS), and no flag or setting, names another app data dir, runtime base or cloud root. The residual covers all three."

---

## Low

- **L1 — *Open* re-read scope.** "anything that changed since the sentence was shown (the file's bytes, the id, the references) refuses". Say "the references, including the set of projects named as not checked", so a project that drops out of reach between the sentence and the lock refuses the command and does not silently shrink what the owner saw.
- **L2 — The record does not match the sentence.** *Record* `account_changed` and the provider-change `account_added` keep only "the references found". Add "and the projects named as not checked", so the audit shows what the owner was told. A later hold of those projects can also read it.
- **L3 — Loopback-to-remote change with projects not checked.** Before epic 4 story 18, a project not checked that names the Account follows a loopback-to-remote change with no hold. That is the owner's call. The sentence should say it in words: "projects not checked that name it will send to the new endpoint when next run, without a hold, until epic 4 story 18".
- **L4 — Scope of "deletes nothing".** *Recovery*'s "It deletes and overwrites nothing (no file, store or keychain entry)" sits next to a remedy that deletes and then sets the keychain entry. Make it "Doctor itself deletes nothing … the confirmation it offers replaces the entry, so the old key is deleted, not kept (*Change*)". It also contradicts the earlier "a lost user store leaves the keys in the keychain" unless that is limited to "until each Account is confirmed".
- **L5 — Naming "the newer binary".** `user_version` gives only a schema number. To name a binary, the user store must record the app version that last migrated it. Say so, or change it to "names the schema version and says a newer app wrote it".
- **L6 — Indirect process start.** The condition reads as direct starts only. An agent with a linked folder's write switch on (epic 12) can write files that the OS or the owner later runs (for example `~/Library/LaunchAgents`). Either say the condition covers direct starts and that indirect ones through a write-on folder are the owner's AD-30 grant within the residual, or refuse a write switch on folders that hold launch locations.
- **L7 — The answer-5 rule lives only in Open Questions.** "reading confirmation records by Account id with one indexed query" binds AD-15 *Key use* and AD-25's audit schema, but appears only in the Resolved bullet. Add a clause to *Key use* citing epic 2 story 6.
- **L8 — *References* says nothing about `add`.** "(*Remove* refuses on them; *Change* does not)". State `add` too: "*Add* and *Change* do not".
