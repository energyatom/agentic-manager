# Rubric review: owner answers to the keyed-hash and aws-lc-rs questions (AD-3, AD-32, Deferred, Open Questions, release diagram, THIRD_PARTY.md; 2026-11-24)

Reviewer: rubric walker (good-spine checklist). Scope: the uncommitted diff (`git diff`, the same as `scratchpad/update-ownerans.diff`). It covers AD-3 (Binds, Prevents, the new **Native-library parts** passage, the removed "the owner confirms this reading before epic 11" parenthetical), AD-32 (the new **The cipher** passage, the new **Restore on another Mac** passage, and "model output included" in the bare file-hash list), the Deferred row "Backup encryption cipher crate", the Open Questions (two Open bullets replaced by one Resolved bullet and one For Sofia bullet), the release mermaid diagram's GitHub Releases node, `THIRD_PARTY.md`, and the five new `.memlog.md` entries.

Checked against AD-17 (key module, recovery key, materialisation list), AD-25, AD-30 (folder tools' audit), AD-32's other passages (key creation on not-found, Doctor's disk row, Leftovers, keyed audit hashes, `check-deps` rule), AD-33, the Stack `aws-lc-rs` row, the earlier 2026-11-24 Resolved and For Sofia bullets, `deny.toml`, `docs/owner-standing-rules.md`, story 9.10 (`tickets/epic-tools-mcp-search-doctor/tickets/tickets.toml`, id 10) with its epic's Notes, epic 11's `tickets.toml` (stories 3, 5, 6, 8, 14, 16), and the registry sources of `aws-lc-sys` 0.47.0 (`Cargo.toml` licence field, `LICENSE`, `aws-lc/LICENSE`). Commands run: `git diff`, `lint_spine.py --workspace …/architecture-agentic-manager` (0 findings), and read-only greps. No cargo command was run. `_archive/` was not read.

## Verdict

**Accept with changes.** There is no critical or high finding. Every owner answer is recorded, verbatim and in substance, and nothing the owner decided was dropped:

- **Answer (1), backup cipher:** `age` is approved in principle, the final yes comes at 9.10's plan stop after the architect's checks, the fallback is a published scheme on `aws-lc-rs`, and no format of our own is allowed.
- **Answer (2), native-library parts:** AWS-LC's CC0 and BSD-3 parts count as native-library parts, with no crate exception. The BSD notice ships with releases, and the licence review goes on epic 11's checklist.
- **Answer (3), both defaults:** a restore gets a new key, model output written to a file stays bare, and a restore surface says that old note hashes cannot be verified.

Other checks:

- AD ids are unchanged.
- No binding text still points at the closed questions: "confirms this reading" and "(Open Questions)" are gone from AD-3 and the Deferred row.
- `lint_spine.py` is clean.
- `THIRD_PARTY.md`'s claim about AWS-LC's `LICENSE` is true. `aws-lc-sys` 0.47.0's `LICENSE` carries the Jitter Entropy notice and Amazon's BSD-3 election (lines 118–131), the CC0 Keccak and AES note (133–136), and the full BSD-3-Clause text (283 onward).
- `aws-lc-sys`'s declared licence expression has no `CC0-1.0`, so `deny.toml` indeed needs no exception.

Four medium findings should be fixed before Sofia rewrites 9.10 and the epic 11 story. Two of them are about enforcement:

- **M1:** the owner's "final yes at the plan stop" depends on ticket metadata that the For Sofia bullet leaves to inference.
- **M2:** "Epic 11's release checklist" exists nowhere, so the notice rule has no owning story and no check that fails.

The other two are about wording that undercuts the rule beside it:

- **M3:** the fallback example, STREAM, does not fix a file format.
- **M4:** AD-32's Prevents still claims to stop a bare hash of model output, while the Rule now keeps one on purpose.

## Owner answers against the text

| Owner said | Where recorded | Faithful? |
|---|---|---|
| (1) "do not invent a file format" | AD-32 *The cipher*, Deferred row, Resolved | Yes. But the fallback's example invites an invented framing (M3) |
| (1) "Approve age in principle" | AD-32, Deferred, Resolved | Yes |
| (1) "the final yes is at the 9.10 plan stop after Winston checks maintenance, licence, exact pin and cargo audit" | AD-32 (checks), Deferred (third column "Owner's final yes"), For Sofia ("an owner stop") | Yes. The added checks are readings of "licence" and "cargo audit" under AD-3 and the CI baseline: the whole dependency graph, and cargo-deny advisories. AD-32 does not name the owner as the one who says yes (L3). The board does not yet enforce the stop (M1) |
| (1) "Fallback if it fails: a published chunking scheme on aws-lc-rs, never our own" | AD-32, Deferred, Resolved | Yes in words; the example is weak (M3) |
| (2) CC0 and BSD-3 parts are native-library parts, no crate-licence exception | AD-3 *Native-library parts*, `THIRD_PARTY.md`, Resolved | Yes; checked against `deny.toml` and the declared licence of `aws-lc-sys` |
| (2) "the BSD notice must ship with releases" | AD-3 Rule, Prevents and Binds; `THIRD_PARTY.md`; diagram | Yes. It is extended to all of AWS-LC's notices and `THIRD_PARTY.md` itself, which is sound because they are one `LICENSE` file |
| (2) "add a licence review to the epic 11 checklist" | AD-3, `THIRD_PARTY.md`, Resolved, For Sofia | Recorded, but the "checklist" has no home in epic 11 (M2). The review's scope is narrowed to native libraries (L8) |
| (3) "Both defaults stand" | AD-32 *Restore on another Mac* (default 1) and the file-hash list (default 2), Resolved | Yes. AD-32's Prevents now contradicts default 2 (M4) |
| (3) "the restore screen later must say old note hashes cannot be verified on the new Mac" | AD-32 *Restore on another Mac*, For Sofia | Yes for the later screen. The v1 surfaces (`agentic backups decrypt`, docs/restore.md) are the architect's reading of "later" and are not marked as such (L2) |

## Checklist walk

| Check | Result |
|---|---|
| Records each owner answer faithfully and completely | Yes (table above); L2 and L8 are attribution and scope nits |
| Every amended Rule is enforceable and prevents its stated divergence | Partly. The cipher stop rests on 9.10's `hitl`/`unknown`, which the For Sofia bullet does not set (M1). The notice-shipping rule has no failing check and no owning story (M2). The fallback example leaves the framing open (M3). AD-32's Prevents overclaims (M4) |
| Nothing under Deferred lets two units diverge | Yes. Every consumer of the cipher choice is in 9.10: daily and pre-migration encryption, `backups decrypt`, Show recovery key, the stored form of `backup-key:<project-id>` and so the audit-hash subkey's input. Epic 11 story 12's keychain move does not depend on the form. 9.10's own verify contradicts an `age` pick (M1(b)) |
| Consistent with AD-17 | Yes. AD-17 gives the key module the backup code's encryption and decryption and has `backups decrypt` hand it a typed recovery key. *Restore on another Mac* ("never writes it to the keychain") matches. So does *The cipher* ("fixes the stored form", which the key module creates). With the fallback, the AEAD calls must sit in `keys.rs` under the `check-deps` rule; that already follows, but is unstated (L5) |
| Consistent with AD-32's key creation on not-found and the Doctor row | Yes. "A restore on another Mac gets a new backup key" is the existing not-found case. Doctor's "copies made under an earlier key need its recovery key" fits. A pre-existing edge where no creation is audited is noted as L9 |
| Consistent with AD-25 | Yes ("hashes kept to compare bytes (files, …) stay bare"). AD-32's own Prevents is the outlier (M4) |
| Consistent with AD-33 | Yes. Nothing in the update touches the trust or engine directories. The licence review is a release-job matter (M2) |
| Consistent with the Stack `aws-lc-rs` row | Yes for the keyed hashes. With the fallback, the existing row is amended rather than a new row added (L5). With `age`, 9.10's Cargo.lock verify no longer holds (M1(b)) |
| Consistent with the earlier Resolved and For Sofia bullets | Mostly. The keyed-hash For Sofia bullet still says "no cipher picked before the owner's answer" (L7). The earlier reuse-rule quote reads awkwardly beside an approved second crypto family (L4) |
| AD ids unchanged | Yes |
| No stale reference to the closed questions in binding text | Yes. AD-3's parenthetical and the Deferred row no longer cite Open Questions. The only stale clause is in a For Sofia bullet (L7) |

## Findings

### Critical

None.

### High

None.

### Medium

**M1. For Sofia (new bullet) leaves out the ticket mechanics that make "the final yes is at the 9.10 plan stop" real, and the 9.10 verify line that an `age` pick breaks.**

- *Location:* Open Questions, "For Sofia (2026-11-24, the owner answers above)". AD-32 *The cipher*. Deferred row, third column ("Owner's final yes at E9 story 10's plan stop").
- *Problem (a), the stop:*
  - Story 9.10 is `hitl = false`. Its only gate today is its `unknown`. Under `board.md`, settling an `unknown` removes it, and the story becomes ready for `bmad-build-auto`, which stops only at `hitl = true` stories.
  - `docs/owner-standing-rules.md` forces a stop for "any new dependency not in the Stack", so picking `age` would stop anyway. The fallback, however, uses `aws-lc-rs`, which is already in the Stack. An auto build that judges `age` to have failed a check could therefore pick and build the fallback with no owner yes, against the Deferred row.
  - The bullet's "becomes its plan stop, an owner stop" does not say how.
- *Problem (b), the verify:*
  - 9.10's verify says "the `Cargo.lock` diff adds only the `aws-lc-rs` dependency lines". Its description still lists the three old candidates and the "once the owner has answered whether the keyed-hash crate rule … also covers the cipher" condition.
  - If `age` is chosen in the same story, the lock diff adds `age`'s whole graph and that verify fails.
  - Epic 9's Notes also still hold the matching "Open question" (line 140).
- *Fix:* extend the For Sofia bullet:
  - Make 9.10 `hitl = true`, or keep an `unknown` reworded to "owner's final yes on the backup cipher at the plan stop, `age` or the published fallback", so the board stops for it.
  - Replace the description's candidate list with AD-32's wording.
  - Scope the `Cargo.lock` verify line to the `aws-lc-rs` addition. When `age` is chosen, the plan lists its expected packages and the verify checks that list.
  - If `age` is chosen, the plan writes `docs/crate-notes/age.md` (AD-5, pre-1.0).
  - Close the epic Notes open question with a dated Decision.
  - Add verify lines for the restore words: `backups decrypt` with a typed recovery key prints them, and docs/restore.md's other-Mac steps carry them.

**M2. "Epic 11's release checklist" is not a thing in epic 11. The notice-shipping rule has no owning story and no check that fails.**

- *Location:* AD-3 *Native-library parts* ("Epic 11's release checklist includes a licence review: …"). `THIRD_PARTY.md` (last paragraph of the AWS-LC section). The Resolved bullet. For Sofia ("the epic 11 story that owns the release licence checks").
- *Problem:*
  - Epic 11 has no checklist artefact. Licence work is spread over several stories:
    - 11.5 fails the release on cargo-deny licences, advisories and bans.
    - 11.6 writes the SBOM.
    - 11.16 ships SearXNG's notice.
    - 11.3 builds the artefacts.
    - 11.8, 11.14 and 11.15 are the gates.
  - The For Sofia bullet does not name which story takes the review. AD-3's new Prevents ("a release that ships a native library without its licence notices") therefore has no failing check behind it. Whether the notices are in the artefacts is mechanically checkable, so leaving it to a human "review" is weaker than the rule's own wording.
- *Fix:*
  - In the For Sofia bullet (or AD-3), name the owning story. 11.5 is the natural one: it already fails the release on licence policy and has a seeded-failure verify.
  - State the check as a release-job assertion. Each artefact that carries the binary (the app bundle, the `agentic` archive) contains `THIRD_PARTY.md` and `aws-lc-sys`'s `LICENSE`, byte-identical to the pinned crate's. `THIRD_PARTY.md` names the `Cargo.lock` versions of `aws-lc-sys` and `libsqlite3-sys`. A seeded artefact missing the notice stops publishing.
  - Keep the human licence review of `THIRD_PARTY.md` as a tag-time step (11.14/11.15).
  - Use one name for it in AD-3 and `THIRD_PARTY.md`, for example "the release job's licence check and the tag-time licence review", rather than "checklist".

**M3. The fallback's example (STREAM) does not fix a file format, so following it means inventing the header and framing, which is what the owner forbade.**

- *Location:* AD-32 *The cipher* ("for example a STREAM construction implemented from its published specification"). The Deferred row (same example).
- *Problem:*
  - STREAM is an online-AEAD construction. It fixes how nonces and segments are built.
  - It does not fix magic bytes, version, header layout, salt or key derivation, segment size, key id or the final-segment encoding. A builder who follows the example still has to design those, and that design is a backup file format of our own.
  - The rule says "no backup file format is invented" and the example contradicts it.
- *Fix:* require a published scheme whose specification fixes the whole on-disk form, and give an example that does. For instance: "a published streaming-AEAD scheme whose specification fixes the header, key derivation, segment size and nonce rules, implemented on `aws-lc-rs`'s AEAD and HKDF (for example Tink's AES-GCM-HKDF streaming AEAD and its documented wire format); a bare construction such as STREAM is not enough on its own." Alternatively, drop the example and leave the pick to the plan stop under that criterion.

**M4. AD-32's Prevents still claims to prevent a bare hash of model output in the audit log. The Rule now keeps one on purpose, for model output a seat writes to a file.**

- *Location:* AD-32 Prevents ("a bare hash in the audit log against which a short owner note, model output or model-chosen name can be guessed (amended 2026-11-24)"). AD-32 Rule ("file bytes (`folder.read`, `folder.write`, file records, AD-30), even bytes a seat wrote, model output included").
- *Problem:*
  - AD-30 says "Every list, read and write is audited with root, path and hash".
  - A seat's `write_file` of a short model output therefore leaves a bare sha256 of that output in the audit log, by the owner's confirmed default (answer 3).
  - The Prevents line now states a guarantee the Rule explicitly does not give. A builder or reviewer of 9.10's classifier could cite the Prevents line to key `folder.write`, which would reopen the owner's decision.
  - AD-25's "hashes kept to compare bytes (files, config, the chain) stay bare" is right but does not say "even model output".
- *Fix:*
  - Qualify the Prevents line: "… against which a short owner note, model output or a model-chosen name recorded as text can be guessed (file bytes a seat writes, model output included, stay a bare file hash by the owner's rule, confirmed 2026-11-24)".
  - Optionally add "even model output a seat writes to a file, AD-32" to AD-25's parenthetical.

### Low

**L1. The *Restore on another Mac* passage sits in the middle of *Keyed audit hashes*.** AD-32 Rule. It is inserted after "…can no longer be matched against a text." The rest of the keyed-hash rules now read under the restore label:

- Doctor's disk row.
- "A missing or unreadable key never blocks…".
- No bare fallback, and `unknown:none`.
- The keychain rename.
- Pre-9.10 entries.
- The `audit_hash_*` suite.
- The Deferred residual.

Fix: move the passage to after "…keyed hashes (Deferred)." (before "`state.db` itself is not encrypted in v1"), so each bold label scopes its own text.

**L2. The v1 restore surfaces are the architect's reading, not the owner's words, and the required words are narrower than the fact.** AD-32 *Restore on another Mac*, For Sofia, and the memlog's "(decision by owner)" entry.

- The owner said "the restore screen later". Making `agentic backups decrypt` and docs/restore.md carry the words in v1 is a sensible reading, but it adds 9.10 scope under the owner's name.
- The words say "keyed note hashes", but model-output hashes and unknown-tool labels made before the restore cannot be matched either.

Fix:

- Mark the v1 surfaces "(architect's reading of 'later': v1 has no Restore screen; the owner can confirm at 9.10's plan stop)".
- Phrase the required words as "keyed audit hashes made before the restore (owner notes, model output, unknown tool names) cannot be verified on this Mac; the audit chain still verifies".

**L3. AD-32 *The cipher* does not say who gives the final yes.** It reads "the final choice is made at epic 9 story 10's plan stop, after the architect checks …". The Deferred row and For Sofia say it is the owner's. Fix: "the owner gives the final yes at epic 9 story 10's plan stop, after the architect checks …; the fallback, if `age` fails, is chosen at the same stop".

**L4. "RustCrypto `chacha20poly1305` is no longer a candidate" reads oddly beside `age`, and the earlier reuse-rule quote is left unreconciled.** Deferred row title; the first 2026-11-24 Resolved bullet and AD-32's keyed-hash quote ("or the backup cipher already uses a crypto family, reuse that family and add no new crate").

- `age`'s own graph uses RustCrypto `chacha20poly1305` for its payload, and also `hkdf`, `hmac` and `sha2`.
- A check like "no `chacha20poly1305` in the graph" would therefore wrongly fail an `age` pick.
- Nothing says that picking `age` leaves the keyed hashes on `aws-lc-rs` in `keys.rs`.

Fix:

- Write "is no longer a candidate as a standalone choice (it stays in `age`'s own graph)".
- Add to the Resolved bullet: "whichever cipher is chosen, the audit-hash subkey and HMAC stay on `aws-lc-rs` in the key module (AD-32); `age`'s RustCrypto crates are used only through `age`".

**L5. The fallback path's Stack and module consequences are unstated.** AD-32 *The cipher* ("its Stack row is added only then") and the Deferred row ("The chosen crate is pinned and added to Stack"). With the fallback:

- No crate is added.
- The existing `aws-lc-rs` row's purpose ("HKDF-SHA-256 and HMAC-SHA-256 for `am-store`'s key module") is amended to name the AEAD and the published scheme.
- The AEAD calls live in `crates/am-store/src/keys.rs`. AD-17 already gives the key module encryption and decryption, and `check-deps` refuses `aws_lc_rs` elsewhere.

Fix: add one clause: "with the fallback no crate is added: the `aws-lc-rs` row's purpose gains the AEAD and the scheme, and its calls stay in the key module".

**L6. The release diagram's node overclaims and uses another noun than AD-3.** GitHub Releases node: "installers · `agentic` binaries · updater manifest (minisign)<br/>THIRD_PARTY.md + licence notices in each artefact (AD-3)".

- The updater manifest, the SBOM and the checksums do not carry the binary or the notices.
- AD-3 says "the `agentic` archive" (a bare binary cannot carry a licence file), and the Bundled built-ins paragraph says "release archives".

Fix: "installers · `agentic` archives · updater manifest (minisign)<br/>THIRD_PARTY.md + licence notices in each artefact carrying the binary (AD-3)".

**L7. A stale clause remains in the earlier For Sofia bullet.** "For Sofia (2026-11-24, keyed audit hashes; …)": "… and no cipher picked before the owner's answer". The answer has arrived and moved the pick to 9.10's plan stop. Fix: append "(superseded by the owner answers of 2026-11-24: the pick is at 9.10's plan stop, next bullet)".

**L8. The licence review's scope is narrower than the owner's words, and a related gap stays implicit.** AD-3 defines the review as native libraries only: each is recorded at its linked version and its notices are in the artefacts. The owner said only "add a licence review". The MIT, BSD and Apache crates and npm packages linked into the shipped binary and UI also ask for their notices in binary redistributions, and nothing in the spine ships those (11.5 checks which licences are allowed; 11.6's SBOM lists names and licences, not notice texts). This is pre-existing and not the update's to decide. Fix: either state that the review covers native libraries only and that crate and npm attribution is a separate owner question (add an Open bullet), or have the review also confirm where crate and npm notices ship. Do not decide it in the spine without the owner.

**L9 (pre-existing, adjacent). Restoring onto a Mac where the project was already opened writes no key-change audit entry.** AD-32 keyed audit hashes and *Restore on another Mac*; docs/restore.md (9.10).

- If the owner opens the project on the new Mac before restoring, the first open finds no key and creates K2, auditing it in the fresh store.
- The restored `state.db` then replaces that store.
- The next writer open finds K2, creates nothing and audits nothing.
- The restored entries carry K1's `key_id`, but no entry records the change.
- Doctor's "after a new key …" sentence, if it is driven by the creation entry, stays silent.

The CLI words still print, so the owner is told once. Fix (9.10 plan, not this update): either docs/restore.md says to restore before the first open on the new Mac, or the writer open audits a key-id mismatch between the store's last `key_id` and the keychain's key, and Doctor reads that.

## What was checked and holds

- AD-3: the old "(the owner confirms this reading before epic 11, Open Questions)" is removed, and the parenthetical still closes correctly. Binds and Prevents are amended with dates.
- AD-3 and `deny.toml`: no `[[licenses.exceptions]]` for AWS-LC is needed. `aws-lc-sys` declares `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`, all on the allow-list. A later version that declares CC0 falls under "A further exception is an owner decision".
- `THIRD_PARTY.md`: "that `LICENSE` file" has its antecedent ("Its `LICENSE` file carries …"). The crate's root `LICENSE` and `aws-lc/LICENSE` differ only by trailing whitespace, so "the `LICENSE` file `aws-lc-sys` carries" is unambiguous in substance.
- AD-32 *The cipher* and keyed-hash derivation: "the entry's value as stored (its UTF-8 bytes)" stays valid for either pick, because the stored form is fixed at the same plan stop, before the first keyed hash.
- AD-17: the key module creates the key, encrypts and decrypts, and never writes a typed recovery key back. This matches *Restore on another Mac*.
- Open Questions: the Resolved bullet maps owner answers (1)→(3), (2)→(4) and (3)→(1)+(2) correctly, cites both review sets, and leaves no 2026-11-24 owner question open.
- The memlog's five entries match the spine text.
- AD headings: none changed. `lint_spine.py`: 0 findings.
