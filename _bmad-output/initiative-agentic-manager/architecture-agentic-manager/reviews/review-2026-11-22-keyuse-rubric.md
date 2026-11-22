# Rubric review: AD-15 *Key use*, *Later confirmations*, keyless loopback Accounts (2026-11-22)

Reviewer: rubric walker (read-only). Scope: `git diff` of the spine (AD-15 Prevents; AD-15 Rule, Interim CLI Account edit: *Record:* now naming the provider, new *Key use*, *Later confirmations*, *Keyless loopback Account*; last Open Questions bullet), the memlog's last 7 entries, the three owner decisions of 2026-11-22, and the built code (`crates/am-app/src/accounts.rs`, `confirmed()` and `test()`; `crates/am-config/src/account_edit.rs`).

## Verdict

**Pass with fixes.** 0 critical, 2 high, 5 medium, 5 low.

All three owner decisions and all three required amendments landed, and the change is in scope. A word-level diff shows only the Prevents clause, the provider in *Record:*, the three new clauses and the one Open Questions bullet changed. The new text ratifies what story 2.3 built (a byte check against the latest History entry, a provider/scheme/host/port match against the latest `account_added`, a read-only reader, no lock, no terminal) and does not contradict AD-12, AD-17, AD-31 or AD-48.

The two high findings are places where the rule meets AD-16 (Reload and pinned snapshots). As worded, they either lock the owner out after a harmless edit or leave open which bytes a run checks. Neither one opens a path for a key to reach an unconfirmed destination: both fail closed. That is why they are high and not critical.

## Input reconcile (owner decisions)

| Owner decision | Landed? | Where |
|---|---|---|
| (1) Keep the confirmed-endpoint rule | Yes | *Key use*: first two sentences, ratifying 2.3 |
| (1) No terminal for `accounts test`, safe from any stdin, Doctor needs it later | Yes | *Key use*: "takes no project lock and needs no terminal, from any stdin (Doctor uses the same check later)" |
| (2) The check compares the provider too; a mismatch with the latest `account_added` means refuse and send nothing | Yes | *Record:* adds "its provider"; *Key use* compares "provider, scheme, host and port"; "reads no key, sends nothing" |
| (2) "No key ever goes to a provider I didn't confirm" | Yes, and widened by the architect to every use of a key (run, later calls) | *Key use* and Prevents; memlog records it as an assumption |
| (3) Keyless only for `http://127.0.0.1:<port>`; remote always needs a key | Yes | *Keyless loopback Account* |
| (3) A small follow-up story or 2.4, not 2.3 | Yes | "built by a later story, epic 2 story 4 or a small follow-up, not story 3" |

Two deliberate narrowings, both acceptable and worth a line in the memlog:
- The owner wrote `http://127.0.0.1:<port>`; the spine allows `[/path]`. This matches the `base_url` rule story 2.3 already built and AD-12's owner-typed loopback port.
- The owner wrote "Keyless Accounts" in general; the spine allows them only for `openai-compatible`. This fits the owner's llama.cpp context, but a self-hosted crawl-service Account on loopback (AD-12, AD-39) is left undecided (see L4).

Required amendments (a), (b) and (c): all landed. The remaining gaps are in the findings below.

## Findings

### High

**H1. A non-widening Reload or first load of `models/accounts.yaml` leaves the byte check failing forever.**
*Key use* requires the file to hold "exactly the bytes of its latest History entry". *Later confirmations* says only the widening confirmations (story 4.18's hold or Reload "when it lists an Account", 5.5's card, Undo of a remove) write a History entry. It then says "any other edit of the file **through the app** commits its History entry". AD-16, however, applies a non-widening outside edit with "the single Reload", and nowhere says that Reload writes History. Examples are a `git pull` that only renames an Account's display name, or an editor reformatting the file. After such a Reload the loaded bytes are in no History entry, every key use is refused, and the owner has no way out except restoring the old bytes by hand or removing and re-adding every Account. This also conflicts with the built `outside_changes` check, which compares each tracked file with its last History entry.
*Suggested fix* (append to *Later confirmations*): "A Reload or startup load that loads a changed `models/accounts.yaml`, widening or not, commits a History entry of the bytes it loaded (`Actor::Human`, the Reload) in the transaction that accepts them. The `account_added` comparison, not the byte check, is what then refuses a provider or endpoint the owner did not confirm."

**H2. The spine does not say which bytes a run checks, and AD-16 pins a run to a snapshot.**
*Key use* says "`models/accounts.yaml` holds exactly the bytes of its latest History entry … names the same provider, scheme, host and port **as the file**". For `test`, the built code checks the same read it derives the endpoint from: "the bytes checked are the bytes used". For a run under AD-16 ("A run pins the snapshot hash it started with") the sentence has two readings, and both go wrong:
- Checking the disk file stops a running run after any later app edit of the file.
- Checking the snapshot means the "latest History entry" compares bytes the call does not use, so the check and the call can disagree (time-of-check against time-of-use).
*Suggested fix* (replace the second sentence of *Key use*'s check): "the check reads the same bytes the call uses: for `test`, the one read of the file it derives the endpoint from; for a run, the accounts file of the snapshot it pinned. Those bytes must be the bytes of a History entry of that file (for `test`, the latest one; for a run, the one its snapshot was loaded from), and the latest `account_added` entry for that Account id must name the provider, scheme, host and port the call goes to."

### Medium

**M1. *Later confirmations* assumes a confirmation for "changes its provider or endpoint", but the AD-15 widening list has no such kind.**
The list classifies "adding an Account" as widening, not retargeting one. Story 4.17's exhaustive `is_widening` could therefore classify a `provider` or `base_url` change as non-widening. Story 4.18 would then never hold it, so no confirmation ever writes the `account_added` the check needs. The check still fails closed, but the Account stays refused until it is removed and added again. This touches AD-15's widening list, which is outside the scoped change, so it should be an owner or architect decision.
*Suggested fix* (in *Later confirmations*, or as an Open Questions item): "`is_widening` classifies a change of an Account's `provider`, `base_url` or key mode as adding that Account, so it is held, confirmed and recorded as above."

**M2. Undo or History restore that changes an Account's provider or endpoint is not covered.**
Only "the UI Undo of a remove" is listed. AD-16's "Undo applies the inverse diff as a new History entry". An Undo of a confirmed endpoint change therefore restores the old endpoint while the latest `account_added` still names the new one, so the Account is refused.
*Suggested fix*: replace "and the UI Undo of a remove" with "and any Undo or History restore that adds an Account or changes its provider or endpoint (an Undo of a remove included)".

**M3. Nothing says how long *Key use* lasts once the interim edit retires.**
*Key use* sits inside the **Interim CLI Account edit** passage, which *Retirement* ends at story 5.5. The clause says it "stays after" stories 17 and 18, but says nothing about 5.5. *Later confirmations* implies it stays, so a reader could still conclude the check retires along with the interim edit.
*Suggested fix*: change "and it stays after them" to "and it stays after them and after the *Retirement* below: it is the permanent rule for every use of an Account's key, not part of the interim exception." Optionally also move *Key use* and *Later confirmations* out of the interim passage into their own bold-labelled paragraph in AD-15.

**M4. The marker for whether an Account has a key is not defined for keyed Accounts.**
Only the keyless add records `key: none`. *Record:* does not give keyed adds a `key` field, and story 2.3's notes have none. Two cases are left open:
- a loopback Account added with a key whose `secret_ref` an app edit later removes: should calls send a key read by id, as 2.3 does, or none?
- the refusal "remote endpoint and no `secret_ref`" contradicts 2.3's behaviour, which reads the key from the entry named by the id when `secret_ref` is absent. This is acceptable only because the keyless story will change it, and the spine does not say so.
*Suggested fix* (in *Keyless loopback Account*): "The latest `account_added` entry decides whether a call carries a key: an entry with no `key` field (every add before this story) or with `key: stored` carries the key from the entry named by the Account's id, and the file's `secret_ref` must name that id; an entry with `key: none` carries none, and the file must have no `secret_ref`. Any other combination, or a remote endpoint with `key: none`, is refused before any request. From this story on a keyed add records `key: stored`."

**M5. The clauses do not say whether a keyless Account's calls pass the confirmed check.**
*Key use* is phrased as "before any use of an Account's key", and a keyless Account has no key. A hand-written keyless loopback entry could therefore be read as callable without any `account_added`, which contradicts AD-15/AD-12 ("adding it is privilege-widening"). The reserved ids `local-lmstudio` and `local-ollama` (AD-31) must stay exempt.
*Suggested fix* (in *Keyless loopback Account*): "Every call of a keyless Account passes the same check as a key use (bytes and the latest `account_added` entry); only the reserved local Accounts of AD-31 are called without one."

### Low

**L1. AD-16 History stores a diff, not bytes.** "holds exactly the bytes of its latest History entry" and "a History entry holding the file's confirmed bytes" read as if History stores file contents. The built check compares the file's sha256 with the entry's `after_sha256`.
*Fix*: "the file's sha256 equals the after-hash of its latest History entry" and "a History entry whose after-hash is the confirmed file's".

**L2. `test` from any stdin while a stdio MCP server or a `render: true` Source is enabled.** The interim passage's "As for every CLI decision, it is refused while …" applies to add and remove. `test` is not a decision, and the owner accepted any stdin. The spine still does not say outright that `test` runs in those states, or that it makes one paid call outside any run's budget.
*Fix*: add "`test` is not a CLI-class decision: it runs in every state, makes one short fixed-prompt call, and is not reserved against a run's budget."

**L3. Keyless add: what counts as an explicit choice.** The add sentence, printed before the typed id, must already say "no credential", so the keyless choice has to be known before the prompt. Ticket 2.10 says "the owner's explicit choice at the prompt".
*Fix*: "Keyless is the owner's explicit choice, made before the add sentence is printed; an empty answer at the key prompt is never read as keyless and stores and writes nothing."

**L4. The Open Questions bullet for Sofia is incomplete.** It omits Doctor's ticket, although owner decision (1) names Doctor. It does not ask whether a loopback crawl-service Account (AD-12) may also be keyless. It still says "epic 2 story 4 or a small follow-up" now that ticket 2.10 "Keyless Accounts on this Mac" exists in the working tree.
*Fix*: add "the Doctor ticket that reuses the check" to the bullet, and one line: "(owner) whether a self-hosted loopback crawl-service Account may also be keyless; today only `openai-compatible` may."

**L5. The Prevents clause does not name the keyless divergences.**
*Fix*: add "a remote endpoint called without a key, or a fake key needed for a server on this Mac" after "the owner never confirmed".

## Consistency checks (no finding)

- **AD-12:** keyless loopback reuses the owner-typed `127.0.0.1:<port>` pin, which is already widening and "never the app's own port". Consistent; it is inherited, not restated.
- **AD-16:** apart from H1 and H2, consistent. *Later confirmations* writes History and `account_added` in the confirmation's own transaction, alongside the approved grant set, and does not create a first grant set.
- **AD-17:** a key is read only from the entry named by the Account's id. Keyless reads no entry. Consistent with the built refusal of a `secret_ref` that names another entry.
- **AD-25:** `account_added` args are a provider, scheme, host and port, with no content and no key. Consistent.
- **AD-31:** the reserved local ids stay keyless and always resolvable (but see M5 for stating the exemption).
- **AD-48:** "a request carrying an Account goes only to that Account's pinned endpoint". *Key use* adds a check of the owner's confirmation on top and does not conflict.
- **Brownfield:** story 2.1 already recorded `provider` in `account_added` (commit 1e6b847), so Accounts added under 2.1 pass the provider comparison without being added again.
- **Scope:** the word diff touches only the Prevents clause, *Record:*, the three new clauses and the Open Questions bullet. Nothing else in AD-15 or elsewhere changed.
