# Adversarial review: `--as` rule, MCP warn-don't-refuse, loopback-HTTP count, keys after a lost store (2026-11-22)

Target: the uncommitted diff of `architecture-agentic-manager.md` in the `agentic-manager-replan4` worktree (AD-13, AD-15 *Privilege-widening*, *Open* and *Migrate*, AD-17 *Old project files* and *Recovery*, AD-26, the Deferred row removed, three Open items resolved). I did not question the owner decisions themselves, only whether the text records them so that separately built units agree. Code read for buildability: `crates/am-config/src/lib.rs` (`OwnerChildren`, `owner_children`, `decisions_locked`, `mcp_url` in `kinds.rs`), `crates/am-config/src/snapshot.rs` (`ConfigSnapshot::owner_children`), `crates/am-net/src/lib.rs` (`pinned`), their callers in am-app (`user.rs::scan`, `run.rs::no_owner_child` / `no_pinned_owner_child`, `runtime.rs` step 8), and the approved 2.13 plan.

**Verdict:** the decisions are recorded faithfully, but four seams still let two units each follow the letter and disagree. Two are high: the loopback endpoint list does not match what am-config and am-net accept, and an `--as` rename conflicts with AD-16's approved grant set. Two are medium: the scan is ambiguous about whether it is the shared one that other CLI decisions also use, and the crash re-run exception cannot be detected. Fix them in the spine before 2.13 writes tests (`--as` items) and before 2.14 is written (scan items).

---

## F1 (high): the loopback endpoint list is not the set the app can reach, so a scan built to the letter misses the one reachable case

**Spine text (AD-15 *Open*):** "an enabled MCP server at a loopback Streamable HTTP endpoint (`http://127.0.0.1`, `http://[::1]` or `http://localhost`, any port) counts exactly as a stdio one".

**Unit A, am-config `mcp_url` (built, AD-12/AD-26):** it accepts `https://<any host>` or `http://127.0.0.1:<explicit port>`. It **rejects** `http://[::1]` and `http://localhost` as errors, and it **accepts** `https://127.0.0.1:<port>`.

**Unit B, am-net `pinned` (built):** any scheme whose target IP is `127.0.0.1` is `Reach::Loopback`, so `https://127.0.0.1:8443/mcp` is reachable. A host name resolving to loopback (`https://localhost`) is blocked by the resolver.

**Unit C, story 2.14's scan written to the AD's list:** it matches `http://` only. As a result:
- a command-running server at `https://127.0.0.1:8443` (an owner-trusted local cert, for example from mkcert) is enabled and reachable, yet not counted, so `agentic accounts add/change/remove/migrate` proceed. This is the hole the owner asked to close.
- two of the three listed forms can never occur. They already fall into `OwnerChildren.broken` (the file has an error), so a 2.14 test asserting "an `http://localhost` server is refused *as a loopback MCP server*" fails. That pushes the coder to loosen `mcp_url`, which would break AD-12.

**Related surviving text (AD-12):** "The resolver blocks private, link-local, loopback and metadata addresses. The exception is a local Account …". The exception list names local Accounts, owner-added OpenAI-compatible Accounts and crawl-service Accounts, but never an MCP server's approved `127.0.0.1:<port>` URL. Yet AD-13, AD-15 and AD-26 now presume such a server runs, and spike criterion 5 used one.

**Change:**
- In AD-15 *Open*, replace the parenthesis with: "(a `transport: http` server whose `url` host is the IP literal `127.0.0.1`, over http or https, any port: the only loopback form AD-12 and am-config's URL rule accept; a `tools.yaml` that fails that rule already counts as enabling a child, failing closed)".
- Make the same edit in AD-13's sentence "at a loopback Streamable HTTP endpoint" and in the *Migrate* refusal.
- In AD-12, add to the exceptions: "and an MCP server's approved `url` on `127.0.0.1:<port>` (never the app's own port or a reserved local Account's port)".

## F2 (high): an `--as` rename contradicts AD-16's approved grant set, so am-app's migrate and story 18's load-time check disagree

**Spine text, surviving in AD-16 *Outside edits cannot widen silently*:** "nor does AD-15's interim CLI Account edit, which … never touches a project's grant set, its `migrate` aside, whose `ConfigMigration` keeps the same Account ids and destinations". With `--as`, the ids are not the same, so this sentence is now false.

**Unit A, am-app `migrate --as` (2.13, to the letter of *Migrate* (3)):** it commits a `ConfigMigration` that renames `old` to `new` in this project and calls it "not widening". It writes nothing to the project's approved grant set, because AD-16 says migrate needs no grant-set change.

**Unit B, epic 4 story 18's startup, Reload and run-start comparison (to the letter of AD-15 *Privilege-widening*):** "naming an Account its approved grant set does not list" is a widening. The project now names `new`, the grant set lists `old`, so the project is held for the two-step confirmation on its next load.

**Unit C, epic 4 story 17's `ConfigMigration` arm:** AD-15 says a `ConfigMigration` is "classified by comparing the old and new snapshots' grants". Two project snapshots do not carry the rename map or `new`'s user-list destination, so the generic comparison also reports a new Account. The arm can encode "not widening" only if the patch carries the manifest's map and both destinations.

`migrate` lives until epic 5 story 5, which is after 4.17 and 4.18, so every `--as` run in that window holds the project, or story 17 and story 18 diverge.

**Change (AD-16):** "… its `migrate` aside, whose `ConfigMigration` keeps the same Account ids and destinations, or, with `--as <old>=<new>` (AD-15 *Migrate*), renames `old` to `new` in the project's approved grant set in the same project-store transaction, keeping the destination it records (from epic 4 story 18; before it no grant set exists)".

**Change (AD-15 *Privilege-widening*, after "never by a second path"):** "the `ConfigMigration` patch carries the manifest's renames and the file entry's destination, and the arm finds no widening only when `new`'s destination equals the one the approved grant set recorded for `old`".

## F3 (medium): "the one shared am-config scan" is the scan `agentic run` and the project-open flags also use

**Spine text (AD-15 *Open*):** "through the one shared am-config scan that every Account command uses … the other CLI-class decisions keep the stdio rule above."

**Code:** `am_config::owner_children(root)` is called by `user.rs::scan` (Account commands), by `run.rs::no_owner_child` (`agentic run`, a CLI-class manual run start), and, through `decisions_locked`, by `runtime.rs` (`--take-over`, `--confirm-move`). `ConfigSnapshot::owner_children` (the run's pinned check) uses the same words.

**Two literal builds:**
- **Story 2.14 extends `owner_children().enabled`.** That is "the one shared scan every Account command uses", so the edit obeys the AD. It also silently starts refusing `agentic run` and the open flags on loopback HTTP, which violates "the other CLI-class decisions keep the stdio rule".
- **Story 2.14 instead adds an Account-only function.** That reads as a second scan, against "the one shared scan".

So the text is buildable without changing non-Account decisions only if `OwnerChildren` gains a separate list that `words()`, `decisions_locked` and the snapshot method ignore.

**Change:** "… counts exactly as a stdio one, through a separate `loopback_http` list that am-config's `owner_children` returns beside its stdio and `render: true` reasons, read only by the Account commands' refusal; `OwnerChildren::words`, `decisions_locked` and `ConfigSnapshot::owner_children` (`agentic run`, `--take-over`, `--confirm-move`) keep the stdio rule above".

## F4 (medium): the crash re-run exception in *Migrate* (2) is undetectable and breaks (5)

**Spine text:** "(2) … not on the user list, except on a re-run after a crash, when the user list holding `new` with this entry's provider and endpoint counts as already present".

Nothing persisted marks a crash re-run:
- the user-store writes commit before the backup and manifest exist;
- `account_listed` records neither a project id nor `old`.

The plan's test table refuses `new` only "on the user list with another destination". In practice, then, any same-destination `new` passes, including one the owner added and confirmed with its own key, or one another project's migrate added. The project's references are then renamed onto a confirmed Account. That contradicts (5), "the new Account starts unconfirmed". A trust test written from (2) ("`new` on the user list is refused") fails against that build.

**Change:** "(2) … except when the user list's `new` was written by an unfinished earlier run of this command for this project: its latest user-store record is that run's `account_listed`, which records the project id and `old` (both added to `account_listed`'s fields in *Record*); any other `new` on the user list is refused".

## F5 (medium): `--rollback` of an `--as` run re-points references at the clashing Account, and its "not widening" covers only the file

**Spine text:** "`--rollback` … re-creates the project's file … (a `ConfigCreate`, AD-16, not widening: the legacy file grants nothing …) and reverts the references its manifest lists".

The reverted references name `old`, which on the user list is, by definition of a clash, the other Account at another destination. Once the owner follows the validate error's fix and removes the legacy file, those seats, capabilities and Sources reach that other destination with no confirmation. Before story 18, nothing holds the project, while *Change* is refused in the same window for the same kind of move.

**Change:** either "`--rollback` re-creates the file and leaves `--as` renames in place (the references keep naming `new`, at the file entry's own destination); the manifest says so", or classify the revert as AD-15's project-level widening (UI class, confirm token), which the CLI cannot do.

## F6 (low): the MCP warning has two scopes and refers to a guarantee retired before epic 9

Two scopes:
- **AD-13** ties the warning to "an MCP server the owner adds, over stdio or at a loopback Streamable HTTP endpoint".
- **AD-26 and AD-15** give it to "a stdio and an HTTP server alike", which includes remote https servers, matching the owner's "any MCP server".

A trust test written from AD-13 (remote: no warning) and epic 9 story 5's dialog written from AD-26 (remote: warning) disagree.

Epic 9 stories 5 and 7 also build after epic 5 story 5. By then AD-15 *Retirement* has removed the CLI `add/change/remove/migrate`, so the "terminal-only guarantee for Account commands" the sentence names no longer exists, and 2.14's loopback count only matters in the window before 5.5.

**Change:** in AD-13, "Adding one, stdio or HTTP, local or remote, is never refused …". Then ask the owner (not decided here) what the sentence names after *Retirement*: for example, the CLI-class decisions, which stay terminal-gated.

## F7 (low): AD-17 *Keys after a lost user store* forbids what am-app must do

**Spine text:** "the keychain entry … is never replaced, read or deleted by recovery, Doctor or am-app".

am-app runs `accounts change`, which replaces the entry, and `add`, which compares the stored key. It also reads keys for `test` and for every run call. A guard test written to the letter forbids all of these. The same paragraph also extends the owner's "at a terminal" to the UI Account card. That extension is consistent with *Retirement*, but it goes beyond the verbatim quote and should say so.

**Change:** "… is never replaced, read or deleted by recovery or Doctor, nor by am-app's set-aside or its creation of a `recovered` store; it is replaced only …" and "(from epic 5 story 5 the Account card's first confirmation, which *Retirement* puts in place of the terminal)".

## Notes

- Story 2.14's ticket (`epic-accounts-model-routing/tickets/tickets.toml`) does not yet carry the loopback-HTTP scan, so it needs a Sofia REPLAN for the AD's ownership to hold.
- *Old project files* (AD-17) and the decision record match the *Migrate* text. The Deferred row's removal is consistent.
