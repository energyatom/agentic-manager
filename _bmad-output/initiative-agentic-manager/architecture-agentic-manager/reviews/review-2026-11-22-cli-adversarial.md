# Adversarial review: AD-15 "Interim CLI Account edit" (2026-11-22)

Lens: build two units one level down that each obey every AD as written and still clash. Scope: the AD-15 amendment (spine line ~222), its aligned lines (AD-32 clause, R6 row), and the restated tickets 2.1, 4.17 and 5.5. The owner decision (memlog, 'interim CLI exception') is final and not re-argued; findings attack how the spine records it.

Evidence read: the spine diff; AD-6, AD-13, AD-14, AD-16, AD-17, AD-24, AD-25, AD-31, AD-32, AD-48; the architecture memlog tail; tickets 2.1, 4.17, 4.18, 4.14, 5.5 and the epic build order (epic 4: ... 10, 15, **17**, 11, 19, **18**, **14**, 12, 13; epic 5 after epic 4); built code `crates/am-secrets/src/lib.rs` (service `agentic-manager`, account `<account-id>`), `crates/am-config/src/session.rs` (accepted bytes held per process), `crates/am-config/src/refs.rs` (an unknown Account reference is a validation error); `templates/departments/blank/` (no `models/accounts.yaml`); `templates/samples/company-a/models/accounts.yaml` (ships `anthropic` and `openai` with `secret_ref`, no key).

## Verdict

**Pass with fixes.** The amendment closes its headline hole (no CLI-token route, so the TTY check cannot be bypassed by a token holder; the store lock keeps children of a running server out). It leaves one critical gap in how the exception retires across 4.17, 4.18 and 5.5, which a v0.1 builder will hit, and three high gaps where two builders split (approved grant set, Mac-wide keychain entries under the new delete rule, laundering outside edits). None needs an owner decision; all are recording fixes.

## Critical

### C1. The retirement window 4.17 → 5.5 has no defined behaviour, and 4.17's ticket contradicts the spine

- **Pair that clashes.** Builder A (4.17, v0.1) reads its ticket: "This story retires epic 2 story 1's interim CLI exception: once the classifier ships, `agentic accounts add` and `remove` go through it and the confirm-token path, or are removed." Builder B (5.5, epic 5, months later) reads: the CLI path "is retired with epic 4 story 17". The spine says the exception lasts "until both 4.17 and 5.5 ship".
- **What breaks.** 4.17 enforces `is_widening` "in the writer on every path" and requires the confirm token for every widening. The CLI write goes through that writer. The confirm token is issued only over a live UI WebSocket and "a token presented ... with the CLI token is refused", so the CLI can never obtain one. In the window between 4.17 and 5.5 (all of v0.1: 4.17, 4.18, 4.14 and the 4.13 release gate) Builder A must choose between (a) deleting the commands per the ticket, leaving v0.1 with no way to add an Account at all (no UI exists yet), (b) letting the writer refuse every CLI add, same effect, or (c) adding an unnamed bypass in the writer, which is the "second permission path" the policy forbids. The spine's sentence "that typed confirmation stands in for the confirm token" does not say how the writer learns of it.
- **The retirement alternative is self-contradictory.** "from then on these commands go through `is_widening` and the confirm-token path, or are removed": the first branch is impossible under AD-15's own token rule, so only removal is consistent.
- **"Shipped" is not checkable in code.** Nothing tells a builder which commit deletes the path.
- **Suggested fix (AD-15, replace the last two sentences of the paragraph):** "The writer takes the confirmation as a typed value: `Confirmation::Token(..)` (AD-15) or `Confirmation::TerminalTypedId { account_id }`, which `am-app`'s `accounts add` alone constructs after its TTY and typed-id checks; from 4.17 the writer accepts `TerminalTypedId` only when every widening `is_widening` finds in the patch is `Widening::AccountAdded` for that same id, and refuses it for any other widening. Epic 5 story 5 retires the exception: it deletes `TerminalTypedId`, `agentic accounts add` and `agentic accounts remove` (the commands then print `agentic open` and the Accounts card), and adds a test that the writer has no confirmation variant but the token. 4.17 does not remove the commands." Ask Sofia to restate 4.17 ("wires the interim stand-in through the classifier; does not retire it") and 5.5 ("retires it; deletes the commands and the variant").

## High

### H1. The interim write and AD-16's approved grant set are unspecified, so 4.18's startup hold and the CLI disagree

- **Pair.** The 2.1/4.17 builder writes History and the `account_added` audit and stops. The 4.18 builder implements "every startup load compares the new snapshot's grant set with [the approved grant set], and if `is_widening` finds any widening the snapshot is held". AD-16 says the approved grant set is persisted "each time an apply commits" but also "an apply never creates the first approved grant set".
- **What breaks.** (1) If the interim write does not update the approved grant set, every CLI-added Account makes the next `agentic serve` or `agentic service` start hold all runs for a two-step confirmation that is UI class, which v0.1 does not have. (2) If it updates it by rewriting the set from the full new snapshot, it **launders** every other widening that reached disk while the server was stopped (a hand-added MCP server, a raised cap): the CLI process's `ConfigSession` takes its accepted bytes from disk at its own start (session.rs), so those edits are never seen as outside edits. (3) If `remove` does not drop the Account from the set, a later hand edit that re-adds the same id is compared against a set still holding it and is not flagged.
- **Suggested fix (AD-16, after "an apply never creates the first approved grant set"):** "The interim CLI Account edit (AD-15) updates an existing approved grant set by its own patch only (add inserts that Account with its pinned endpoint; remove deletes it), in the same transaction as its History and audit; it never creates the first set and never copies other fields from the snapshot. With no approved grant set it writes none, and the first-load hold later lists the Account like every other grant."

### H2. Keychain entries are Mac-wide, and the amendment adds deletes against them

- **Pair.** AD-17 stores a key under service `agentic-manager`, account `<account-id>` (built that way in am-secrets); the shipped sample's `accounts.yaml` names `secret_ref: anthropic`. The amendment adds: `add` "deletes it again if the write fails", `remove` deletes the entry after commit.
- **What breaks.** Two projects with an Account `anthropic` (the sample and a copy, or any two projects) share one keychain entry. `accounts remove anthropic` in project A deletes project B's key; `accounts add anthropic` in B overwrites A's key; a failed config write in B's `add` rolls back by deleting the entry A was already using. AD-32 explicitly makes copies normal ("copying a company to start another is normal").
- **Suggested fix (AD-17, and the AD-15 paragraph):** "An Account's key is stored under account `<project-id>:<secret_ref>` (a project's Accounts never share an entry with another project's). `accounts add` refuses when an entry already exists under that name, and its rollback deletes only an entry this command created; `remove` deletes only its own project's entry." If the owner prefers Mac-wide keys, say instead that `remove` never deletes a keychain entry (Doctor lists unreferenced entries) and rollback deletes only an entry the command created. Either way state which; today two builders will pick differently.

### H3. An interim add can activate or bless config the owner never confirmed

- **Pair.** The validator (refs.rs) makes a seat naming an unknown Account a validation error; the writer accepts a write that removes errors. The CLI process loads whatever is on disk as accepted.
- **What breaks.** A seat file edited outside the app (or arriving by hand-copy) names `model: x` for a seat with linked folders. The snapshot is invalid until `accounts add x`, which makes it valid: the seat's linked files now go to a cloud host, a widening AD-15 lists explicitly ("linking a folder to a seat whose model ... is a cloud Account") with its own effect sentence, but the interim sentence names only the Account, provider and host, and only `account_added` is audited. The same applies to a capability `provider: x` (choosing a provider Account is a widening).
- **Suggested fix (AD-15 paragraph, after "a mismatch writes nothing"):** "The command refuses, naming the files, when any tracked config file differs from the hash the store last recorded for it (a pending outside edit) or when the loaded snapshot has validation errors. When seats, fallbacks, capabilities or Sources already name the Account id, the sentence lists each and the effect it gains (for a seat with linked folders: its files leave this Mac for that host), and each is audited as its own widening kind in the same transaction."

## Medium

### M1. How the CLI process opens the project is not fixed (two owners of "open")

AD-32's resolver runs identity tools, root index, `project_id` mint, legacy move, database migration and the `.agentic-open` marker in a fixed order, and a pre-open server holds a root-identity lock rather than the store lock. The amendment only says the command "opens the project's store as its one writer". One builder opens the runtime folder straight from `RuntimeLayout` (skipping the marker, so another Mac's open project is written to; skipping the pre-open lock, so it races the wizard); another runs the full resolver including a legacy move and an AD-7 migration from a CLI subcommand.
**Fix:** "The command opens the project through `am-app`'s same resolver as `agentic serve`, in its order, writing and removing the marker by the same rules and taking the pre-open root lock; it never runs a legacy move or a store migration and never makes a project-open decision: any check that would stop `serve` (another Mac's marker, a move, a copy, a changed id, a pending legacy move, a store needing migration) refuses in words naming `agentic serve`. The System mint of a missing `project_id` runs as at any open."

### M2. `remove` semantics split three ways (references, reserved ids, Undo after the key is gone)

- An Account still named by a seat's model, fallback, capability provider or Source makes the snapshot invalid (refs.rs), so the writer refuses the remove; 5.5 says removing "names the seats that will fall back". A CLI builder and the validator disagree.
- Nothing says the reserved `local-lmstudio` / `local-ollama` cannot be removed (they are "always resolvable", AD-31).
- "remove is a plain edit with Undo" but the key is deleted after commit, so Undo restores an Account whose `secret_ref` points at nothing.
**Fix:** "`remove` refuses the reserved ids and refuses, naming each seat, fallback, capability and Source file, while anything names the Account (change those first); it requires the typed id like `add`. Undo of a removal restores the config entry only; the Account then shows 'key missing' until the owner adds the key again through the UI class, and that Undo is widening (UI class)." If fallback-on-remove is wanted instead, say the validator treats a removed Account's references as falling back, and change refs.rs's rule in a ticket.

### M3. The first `add` in a project is a `ConfigCreate`, not a `ConfigEdit`

The Blank template ships no `models/accounts.yaml`. AD-16: "Every new config file is a `ConfigCreate`" (create-without-replace by hard link, classified against the empty grant set). The amendment says `ConfigEdit` only.
**Fix:** "as the owner's own `ConfigEdit`, or `ConfigCreate` when `models/accounts.yaml` does not exist (AD-16)".

### M4. `add` for an id already in `accounts.yaml` (the sample's keyless Accounts) is undefined

The Company A sample ships `anthropic` and `openai` with a `secret_ref` and no key; the owner's first step is to give one a key. One builder refuses `add` (the owner must remove and re-add); another treats it as "store the key only", which writes no config, so no History and no audit, outside the typed-confirm rule.
**Fix:** "On an id already in `accounts.yaml` with no key stored, `add` stores only the key, after the same terminal and typed-id checks, and writes one audit entry (`account_key_set`, Account id, pinned endpoint; no key) with no config write; on an id that already has a key it refuses."

### M5. Audit shape drifts from AD-15's general rule and AD-48's `host_added`, and pre/post-retirement entries will not match

AD-15: one entry per widening with "its kind, the seat or file it touches and the value". AD-48: `host_added`, seat id, pattern. The amendment: `account_added`, Account id, "the pinned host". Gaps: no file; "host" drops scheme and port, while AD-48 pins an Account to scheme, host and port (an OpenAI-compatible loopback Account differs only by port); nothing binds 4.17's `Widening::AccountAdded` to serialise to the same kind and args, so audit search across the retirement splits; nothing records which confirmation was used.
**Fix:** "kind `account_added`, file `models/accounts.yaml`, value the Account id and its pinned endpoint (`scheme://host:port`), and `confirmation: terminal_typed_id` (the UI path records `confirm_token`); `Widening::AccountAdded` (4.17) writes exactly this shape." Also: "`remove` writes `account_removed` (Account id; no key), and the keychain delete is recorded in that entry's args as done or failed."

### M6. The TTY guarantee is stated stronger than the check gives

"so no agent, script or child process can add an Account" is not true of a process that allocates a pseudo-terminal (`script`, `expect`, a Python `pty`): `isatty(stdin)` is true and it can type the id. Within the app the real controls are that seats have no shell, the store lock keeps children of a running server out, and the MCP/render refusal. A builder reading the absolute claim will think the TTY check is the control and test only pipes.
**Fix:** "...refused in words before anything is read or written. This keeps out pipes, redirected stdin and the app's own children (which get pipes and, while the server runs, meet the store lock); it does not stop another owner-uid program that allocates a terminal, which is the residual risk stated on the About page until 5.5. The key and the typed id are read from the controlling terminal (`/dev/tty`) with echo off, and the command also refuses when stdout is not a terminal, so the confirmation sentence is seen."

## Low

- **L1. `agentic service` (4.14, v0.1) holds the project permanently under launchd**, so `accounts add` is refused for as long as the LaunchAgent is installed, and no spine command releases it. Fix: the refusal names the release command, and 4.14 gains `agentic service stop` (unloads until next login) or the refusal names `agentic service uninstall`.
- **L2. Crash windows leave orphan keychain entries** (between `add`'s key store and its config commit; between `remove`'s commit and its key delete). AD-16's intent row reconciles files, not keys. Fix: "Doctor lists keychain entries under this project's prefix that no Account names, with Remove."
- **L3. "refused naming the UI" while MCP/render is enabled names a surface that does not exist before epic 5.** Harmless today (MCP and render arrive after 5.5), but if retirement slips the refusal is a dead end. Fix: add "if the exception is still live when epic 9 or 12 ships, those epics' first story retires it" or keep the words and accept it.
- **L4. Hat name.** The amendment uses "the Owner hat"; AD-24 hats come from `company.yaml` `hats`. Say whether "Owner" is a built-in hat that always exists or which hat the CLI uses when the company defines none.

## Not findings (checked)

- A CLI-token holder cannot reach the edit: no API route exists, and the store lock refuses the separate process while a server runs. Consistent with AD-13 and the project-open precedent.
- The test terminal staying out of the release binary is covered by ticket 2.1's guard test.
- The AD-32 clause and the R6 row are aligned with the amendment.
