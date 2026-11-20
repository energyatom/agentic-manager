---
title: 'Config writer, History and Undo'
type: feature
ticket: 8
created: '2026-11-20'
status: done
baseline_revision: '391558ef6dd3f182dc0acfefbaf1d158255adc76'
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [quick]
review_loop_iteration: 0
context:
  - '{project-root}/AGENTS.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** No code writes project config files. R3 and AD-16 need every config edit through the command path to be atomic, canonical and recorded in History (diff, Actor, hat) with its audit entry, and need Undo and Restore. Epic 6 (⌘Z, toast), 1.9 (watcher fingerprints), 1.10 (migrations) and 1.13 (`project_id` write) build on it.

**Approach:** am-config gains a `ConfigWriter` that applies a `ConfigEdit` to an existing file's parsed tree. It emits canonical YAML through serde-saphyr and keeps a Markdown body byte for byte. It writes temp, fsync, rename and fsync dir. It records each step through a new am-domain port, `ConfigHistory`. am-store implements that port with a migration and writer commands, in AD-16's apply order: intent row, then the file, then History and audit in one transaction.

## Boundaries & Constraints

**Always:**
- The AD-16 apply order:
  1. Commit an intent row (file, old and new sha256).
  2. Write and rename the file.
  3. Commit the History entry and its audit entry in one transaction.

  `reconcile()` completes an intent whose file already has the new hash, audited as `config.reconciled`. It drops one whose file does not.
- Every History entry and command carries an explicit `Actor` (no default). History renders it with `Actor::describe()` ("You (Reviewer)").
- The audit `args` hold only the entry id, file path, sha256 values and kind, never file content (AD-25).
- An edit, Undo or Restore that adds an `am_config` error the project did not already have is refused, naming each new error with file, line and fix, and nothing is written. Errors after must be a subset of errors before, compared on (file, message, fix) without the line, so a project broken in several files can be fixed one file at a time (owner renegotiation, 2026-11-21).
- Undo of an entry writes that entry's before-bytes. It is refused in words when the file's current sha256 is not the entry's after-hash (it changed since).
- Restore writes an older entry's after-bytes. Undo and Restore each create a new History entry (kind `undo` or `restore`, naming the entry they act on).
- Self-write fingerprints: the writer remembers (file, after-sha256) of its own writes, exposed for 1.9's watcher.
- Tests are written first and seen failing before implementation.
- Canonical key order (owner decision, 2026-11-21):
  - Top-level keys follow each file kind's schema order, which is its loader allow-list order, with `schema_version` first.
  - Keys in nested mappings are sorted alphabetically.
  - Sequences keep their order.

**Never:**
- Creating, deleting or renaming config files. Only existing files under the project root, by relative path; `..`, absolute paths and paths outside known config folders are refused.
- The approved grant set or `is_widening` (epic 4 stories 17 and 18).
- The watcher or Reload (1.9).
- UI or API endpoints (epic 6).
- Redo as its own command.
- Preserving YAML comments (AD-16).
- Adding a diff crate. The line diff is project code.
- am-config depending on am-store, or the reverse.
- Writing runtime state into the project folder.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Edit seat | `Set ["budget","per_run"]=2` on `seats/alpha.md`, Human(Reviewer) | File replaced; frontmatter keys in canonical order; body bytes identical; History entry with unified diff and "You (Reviewer)"; one audit entry `config.edited` | — |
| Undo latest | Undo that entry | Old bytes back exactly; new entry kind `undo`; audit `config.undone` | — |
| Undo stale | File edited again (or outside) after entry | Nothing written | Refused: "changed since this entry" |
| Restore | Restore entry 1 after entries 2 and 3 | File equals entry 1's after-bytes; new entry `restore`; audit `config.restored` | No-op (already equal) refused in words |
| Invalid result | Set `team` to an unknown id | Nothing written, no History | Refused with file, line, fix |
| Already broken elsewhere | Two files broken by outside edits; fix one through the writer | Allowed while it adds no error; the second fix makes the project valid | An edit adding a new error is refused, naming only the new one |
| Bad path | `../x.yaml`, `/etc/x`, missing file | Nothing written | Refused in words |
| Crash after rename | Intent committed, file renamed, History not committed | `reconcile()` completes it, audited `config.reconciled` | Intent whose file lacks the new hash is dropped |

</frozen-after-approval>

## Code Map

- `crates/am-domain/src/actor.rs` -- `Actor`, `describe()`. Reuse; don't change.
- `crates/am-domain/src/` -- new `config_history.rs`: the `ConfigHistory` port (sync, `Send + Sync`, owned types). It covers begin intent, commit entry, drop intent, list pending intents, list entries (optionally per file), and get entry. Its types are `HistoryEntry` (id UUIDv7, seq, at, actor, file, kind, ref_entry, before/after sha256 and bytes, diff) and `HistoryKind`. Export it from `lib.rs`.
- `crates/am-store/src/lib.rs` -- `WriteCommand` (:108-174), `audit_entry()` (:180-310) and `apply()` (:479-606). Add `BeginConfigIntent`, `CommitConfigEntry` (deletes its intent and inserts History in one tx), `DropConfigIntent` and `ReconcileConfigIntent`, plus Reader queries. Audit actions are `config.intent_opened`, `config.edited`/`undone`/`restored`, `config.intent_dropped` and `config.reconciled`.
- `crates/am-store/src/config_history.rs` (new) -- `StoreConfigHistory { writer, reader }` implements the port.
- `crates/am-store/migrations/0004_config_history.sql` -- `config_intents` and `config_history` (bytes as BLOB, content stays out of the audit). Add it to `MIGRATIONS` (migrations.rs:25-29).
- `crates/am-config/src/lib.rs` -- `load` (:84) reads files into a map (:113). Add `load_with(root, overrides)` so the writer validates in memory. `frontmatter()` (:398-414) splits a seat file; reuse it for the body split.
- `crates/am-config/src/node.rs` -- `Node`/`Kind::Map` keep key order and `raw` scalars. The writer edits this tree.
- `crates/am-config/src/kinds.rs` -- each kind's `f.allow(&[...])` list (seat at :1341). Hoist these into shared `const` key-order tables used by both loader and writer.
- `crates/am-config/src/{writer.rs,canon.rs,linediff.rs}` (new) -- `ConfigWriter`, the canonical emitter, and a small LCS unified diff.
- `crates/am-testkit/` -- an `InMemoryConfigHistory` fake for am-config unit tests.
- `crates/am-app/src/main.rs:77-90` -- `serve` opens the store. Build `ConfigWriter` there and call `reconcile()` as `Actor::System{component:"config-writer"}`.
- `crates/am-e2e/tests/config_history.rs` + `Cargo.toml` `[[test]]` and an `am-config` dev-dep -- the ticket's acceptance test on a copy of `fixtures/projects/valid` with a real store.
- `crates/am-config/tests/common/mod.rs` -- `copy_of(name)`; reuse.
- `xtask/src/deps.rs` -- am-config and am-store stay Record → am-domain only. Don't change it.

## Tasks & Acceptance

**Execution:**
- [x] `crates/am-e2e/tests/config_history.rs`, `crates/am-config/tests/writer.rs`, `crates/am-store/tests/config_history.rs` -- write every matrix row and AC as tests first. Add only the stub types needed to compile, run them, and record the failing output in Implementation Notes -- tests-first is mandatory.
- [x] `crates/am-domain/src/config_history.rs` -- port and types -- shared seam.
- [x] `crates/am-store/migrations/0004_config_history.sql`, `src/lib.rs`, `src/config_history.rs` -- tables, commands, audit entries, readers, adapter -- History and audit in one tx.
- [x] `crates/am-config/src/{canon,linediff,writer}.rs`, `lib.rs` (`load_with`), `kinds.rs` (key-order consts) -- the writer -- the R3 core.
- [x] `crates/am-testkit/src/` -- `InMemoryConfigHistory` -- unit-test fake.
- [x] `crates/am-app/src/main.rs` -- reconcile at serve start -- AD-16 startup rule.
- [x] `docs/crate-notes/` -- none needed (serde-saphyr is ≥1.0). Update am-config's crate description.

**Acceptance Criteria:**
- Given a seat file whose frontmatter keys are out of order, when any ConfigEdit writes it, then the frontmatter is emitted in canonical order and the file re-loads to the same values with no errors.
- Given every file in `fixtures/projects/valid` and `templates/`, when it is emitted canonically, then a second emit gives identical bytes, and it re-loads to the same snapshot values.
- Given a write is interrupted before the rename (simulated), then the original file is intact and no temp file is left after the next writer start.
- Given a seat file, when it is edited, undone and restored, then `audit verify` passes and the audit log holds one entry per step, with no file content in `args`.

## Implementation Notes

- **Tests first (2026-11-21).** The three acceptance files were written against stub types only (am-domain port and types, `StoreConfigHistory`, `InMemoryConfigHistory` and `ConfigWriter` returning "not implemented"). `cargo nextest run --no-fail-fast -p am-config -p am-store -p am-e2e -E 'test(config_history) | test(writer)'` then gave **21 failed, 2 passed** (the 2 passes are story 1.7's `am-store::audit` tests whose names contain "writer"): all 13 `am-config::writer` tests, all 5 `am-store::config_history` tests and all 3 `am-e2e::config_history` tests failed (e.g. `BadEdit("not implemented")`, `config_files` returned `[]`). After implementation: 27 passed (with the `linediff` unit tests).
- **am-app serve test (redone tests-first, owner decision 2026-11-21).** The serve `ConfigWriter`/`reconcile` wiring was removed (`main.rs` back to its committed state) with `serve_reconciles_interrupted_config_writes_at_start` kept; `cargo nextest run -p am-app -E 'test(serve_reconciles)'` failed at `serve_project.rs:391` with the output `["agentic: project …: database at version 4", "agentic: serving on http://127.0.0.1:…"]` (no "completed the interrupted write" line). The wiring was restored and the test passed.
- **Audit per step.** Every writer command commits its own audit entry (AD-25), so each edit, Undo and Restore leaves two config entries: `config.intent_opened` (step 1 of the apply order) and exactly one completion entry (`config.edited` / `config.undone` / `config.restored`, target = the History entry id). The tests read "one audit entry per step" as one completion entry per step, plus one intent entry each.
- **Intent row holds the bytes and diff.** Besides file and the two hashes, the intent row stores actor, kind, ref_entry, both byte strings and the diff, so `reconcile()` can complete a crashed write into a full History entry; the entry keeps the intent's Actor (who asked), while `config.reconciled` is audited by `System(config-writer)`. The History entry id is the intent id (UUIDv7).
- **Store checks.** `BeginConfigIntent` refuses hashes that are not 64-hex or not the sha256 of their bytes, an unchanged file, an Edit naming a ref_entry, an Undo/Restore without one or naming an unknown entry, and a non-relative path; `config_history` is append-only by triggers (as `audit_log`). `StoreError::NoConfigIntent` maps to `HistoryError::NoIntent`.
- **Validation scope (changed 2026-11-21, owner).** At first an edit, Undo or Restore was refused when the resulting project had any error at all. It is now refused only when it adds an error the project did not already have (multiset of (file, message, fix), so moved lines do not count). Test written first: `writer_allows_fixing_two_broken_files_one_at_a_time` failed with `Invalid` naming both existing team errors on the first harmless edit, then passed after the change.
- **Canonical YAML.** serde-saphyr with `compact_list_indent = false` (list items indented under their key, the house style) and `yaml_12 = true` + `no_lang_directive = true` (the loader reads YAML 1.2, so `on:` is written plain, not `"on":`). Schemas stay JSON (serde_json pretty, `schema_version` first, nested keys sorted). Long text is folded (`>-`), multi-line text is a literal block.
- **Story 1.7 test touched.** `am-store/tests/audit.rs::a_chain_written_by_1_6_binaries_verifies_after_migrating_and_writing` counted exactly one migration step from version 2; it now counts `SCHEMA_VERSION - 2` steps (migration 0004 is the new step).
- **Review pass 1 patches, tests first (2026-11-21).** New tests, run against the unpatched writer (with a stub `exact_name` returning `Ok`): `cargo nextest run --no-fail-fast -p am-config -p am-e2e -E 'test(writer) | test(config_history) | test(exact_name)'` gave 7 failed, 16 passed:
  - #1 `writer_never_follows_a_link_at_the_temp_path`: panicked "a writer start removes a temp-named link" (open() skipped the link).
  - #2 `writer::tests::exact_name_needs_the_listed_spelling` (unit test of the exact-name check, independent of the volume's case rules): `unwrap_err()` on `Ok(())`; `writer_refuses_a_name_that_is_not_the_exact_name_on_disk`: on APFS the edit of `seats/ALPHA.md` setting an unknown team succeeded (`Ok(HistoryEntry { file: "seats/ALPHA.md", … })`).
  - #3 `writer_keeps_the_text_of_untouched_number_written_values`: emitted `metadata:` without `build: 007` / `version: 1.10` (written as `7` / `1.1`).
  - #4 `writer_refuses_while_the_file_holds_a_secret_looking_value`: the edit removing a pasted key succeeded (`Ok(HistoryEntry { … })`), so the key reached History.
  - #5 `writer_settles_a_pending_intent_of_the_file_before_the_next_write` and `am-e2e config_history_pending_intent_is_settled_by_the_system_before_the_next_write`: `pending_intents()` not empty after the next write.
  After the patches: 27 passed (`-p am-config -p am-e2e -p am-testkit`, same filter plus `linediff`).
- **Patches.** #1 `replace` removes whatever stands at `.<name>.am-tmp` and creates the temp with `create_new`; `open()` also removes temp-named links. #2 `resolve` requires each component to equal a `read_dir` name exactly (`exact_name`), naming the on-disk spelling in the refusal. #3 `canon` writes an untouched scalar whose `raw` differs from serde-saphyr's rendering of its typed value as a reserved plain token (`am-raw-scalar-NNNNNN-end`) and puts the raw text back after emission; each token must occur exactly once, so a file holding the reserved word is refused, not rewritten; JSON schemas are unchanged. #4 `step` refuses (`WriteError::Secret`, words name the lines, never the value) when the file's current bytes hold a secret-looking value by `checks::text_secrets` over every line or `checks::secrets` over the tree; nothing is opened. #5 `step` settles pending intents of the same file before `begin_intent` (complete or drop, as `reconcile`, by `System(config-writer)`). #7 `InMemoryConfigHistory`'s doc says exactly what it checks.
- **Test adjusted for #5.** `am-e2e config_history_crash_rows_are_reconciled_at_the_next_start` made its second (before-rename) crash on the same seat; with #5 that write settles the first intent itself. Its second crash now edits `seats/beta.md`, so both rows still reach the next start.
- **Crash simulation.** `ConfigWriter::fail_at(FailPoint::{BeforeRename, AfterRename})` exists only with am-config's `test-support` feature (dev-dependencies of am-config and am-e2e), like am-store's `testing` module.

## Plan Change Log

- 2026-11-21 — **Review pass 1, patched in place (owner decision, 2026-11-21: patch rather than revert and re-derive).** Two Design Notes were wrong and are amended here.
  - **Scalars.** The note said "`1.50` may become `1.5`". That breaks AC 1 for text fields whose value is written as a number: the loader reads those from the raw text, so `version: 1.10` would become `1.1` and `id: 007` would become `7`. Amended: a scalar the edit did not touch keeps its original text whenever that text differs from the typed value's rendering.
  - **Secrets in History.** The note said "config holds no secrets (the validator rejects them)". That holds only for the after-bytes: a key pasted by hand would reach the append-only History through the before-bytes and the diff. Amended: an edit, Undo or Restore is refused in words when the file's current bytes hold a secret-looking value, and the owner removes the key by hand first. No key ever enters History, an intent or the diff.
  - **KEEP:** everything else in the implementation, which passes `just check`.

## Review Triage Log

**Pass 1 (quick lens, 2026-11-21).** Verdicts: 1 high, 3 medium, 2 low, 1 rejected. Every route is a patch (owner decision: patch in place).

| # | Finding | Verdict | Route | Evidence / action |
|---|---------|---------|-------|-------------------|
| 1 | Symlinked `.<name>.am-tmp` makes an edit write outside the project | high | patch | `File::create` follows links. `open()` cleanup skips symlinks because `DirEntry::file_type` doesn't follow them. A project from git or an import can carry the link. Fix: remove any existing entry, create the temp with `create_new`, and have `open()` remove temp links too. |
| 2 | Case-insensitive name (`seats/ALPHA.md`) skips validation | medium | patch | Confirmed: the `load_with` override key must equal the listed name (`lib.rs` `overrides.get(rel)`). APFS is case-insensitive and v1 is macOS only. Fix: `resolve` requires each path component to match a directory entry exactly. |
| 3 | Canonical emit changes untouched number-written text values | medium | patch | Confirmed: `canon.rs:174-175` ignores `raw`, while `fields.rs:105-109` reads text from `raw`. Fix: keep the raw text of an untouched scalar when it differs (Plan Change Log). |
| 4 | Secrets in before-bytes and diff stored forever in History | medium | patch | Confirmed: `step()` validates only the after-bytes, and `config_history` is append-only. Fix: refuse when the current file holds a secret-looking value (Plan Change Log). |
| 5 | Pending intent dropped at next start after later edits | low | patch | Confirmed: needs a `Written::Maybe` or a commit failure while the writer keeps running, then another edit of the same file. Fix: before a write, settle pending intents for that file (complete or drop, as `reconcile`), or refuse while one is pending. |
| 6 | AC 4 "one audit entry per step" reinterpreted | rejected | — | The Code Map names `config.intent_opened`, and AD-6/AD-25 audit every writer command. The only fix would be to edit this plan's AC, which triage rejects. |
| 7 | `InMemoryConfigHistory` doc overstates the rules it enforces | low | patch | Confirmed. Smallest fix: make the doc comment say what the fake checks. |

**Tests-first deviation (owner decision, 2026-11-21):** `serve_reconciles_interrupted_config_writes_at_start` was written after its wiring. To redo it test-first: remove the serve wiring, keep the test, run it and record the failure, then restore the wiring.

## Design Notes

- **Edit shape:**

  ```rust
  ConfigEdit { file: String,
               ops: Vec<EditOp>,         // Set{path: Vec<String>, value: serde_json::Value} | Remove{path}
               body: Option<String> }    // .md only; None keeps the body byte for byte
  ```

  Epic 4's typed patch (`is_widening`) will compile down to these ops.
- **Undo as inverse diff:** History stores before and after bytes, so the inverse is exact. The hash check makes it safe. Config holds no secrets (the validator rejects them), so storing the bytes in the DB is allowed by AD-35 ("config History").
- **Serialisation:** one `Mutex` in `ConfigWriter` runs each intent → write → commit sequence alone.
- **Atomic write:** the temp file `.<name>.am-tmp` sits in the same folder. Keep the file mode, fsync it, rename, then fsync the folder. Stale `*.am-tmp` files are removed at writer start.
- **Scalars:** a scalar the edit didn't touch is emitted from its typed value, so `1.50` may become `1.5`. Canonical form is accepted.

## Verification

**Commands:**
- `cargo nextest run -p am-config -p am-store -p am-e2e -E 'test(config_history) | test(writer)'` -- expected: all pass.
- `cargo run -p am-app -- validate fixtures/projects/valid` -- expected: exit 0.
- `cargo xtask check-deps` -- expected: pass.
- `just check` -- expected: green.
