# Final review 2 — fixes applied (2026-11-19)

Applied headless by the BMad team (Sofia tickets and initiative, Winston architecture, John PRD, Sally UX) under the owner's delegation of 2026-11-19. Every Critical, High and Medium finding is fixed, and so is every Low. Nothing is deferred. The decisions are dated 2026-11-19 in the initiative Notes, and each touched epic's Notes carry a matching Decision line. Each change is also logged in the `.memlog.md` of the workspace it touched.

**Totals:**

| Review | Fixed | Deferred |
|---|---|---|
| Consistency (2 H, 8 M, 7 L) | 17 | 0 |
| Security (5 H, 6 M, 4 L) | 15 | 0 |
| UX (4 H, 6 M, 4 L) | 14 | 0 |
| **All** | **46** | **0** |

**New stories:** these are all in epic 4. Ids are kept, and table order is the build order: 1–6, 16, 7–10, 15, 17, 11, 18, 14, 12, 13.
- 4.16 Parser and OCR children run inside the macOS App Sandbox
- 4.17 One privilege-widening classifier for every config path
- 4.18 Outside edits and startup that widen wait for the two-step confirmation

**Entries:** 170 in all, 169 planned and 1 dropped (11.4).

**ADs:** no new AD. AD-6, 9, 10, 11, 12, 13, 15, 16, 26, 30, 33, 36, 37, 39, 42, 44, 45, 46 and 47 are amended, along with the Stack, Conventions, Deferred and Open Questions sections. The children-isolation decision is recorded under Open Questions.

**New requirements:**
- Epic 4: R16, R17 and R18.
- Epic 5: R23.
- Initiative: the new Child isolation constraint, owned by epic 4.

## Sandbox decision (security H5)

These facts were checked on the web on 2026-11-19:
- Apple, "Embedding a command-line tool in a sandboxed app": the same process works for Developer ID distribution. The helper is signed with `com.apple.security.app-sandbox`, plus `inherit` when its parent is sandboxed.
  https://developer.apple.com/tutorials/data/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app.md
- Apple Developer Forums, "Resolving App Sandbox Inheritance Problems": a child that has `inherit` but a non-sandboxed parent crashes ("Process is not in an inherited sandbox"). The fix is to run the child in its own sandbox by removing `inherit`.
  https://developer.apple.com/forums/thread/706390
- `sandbox-exec`/Seatbelt is deprecated and has no Apple-supported replacement for this use, and its profile language is undocumented. It still works on current macOS, and Chromium, Codex and Claude Code use it.
  https://gastown.dev/docs/research/macos-sandbox-exec ; https://chromium.googlesource.com/chromium/src/+/HEAD/sandbox/mac/seatbelt_sandbox_design.md
- From Chrome 136, `--remote-debugging-port` and `--remote-debugging-pipe` are honoured only with a non-default `--user-data-dir`.
  https://developer.chrome.com/blog/remote-debugging-port
- Chromium bypasses a manual proxy for loopback by default, and `<-loopback>` in the bypass list removes that bypass.
  https://chromium.googlesource.com/chromium/src.git/+/20e8909e694cab857da21d8fe9d5f1da170eaae2/net/docs/proxy.md
- chromiumoxide 0.9.1 speaks CDP only over WebSocket. This was checked in its crate source. `chromiumoxide_cdp` 0.9.1 (MIT OR Apache-2.0) and `command-fds` 0.3.3 (Apache-2.0) exist on crates.io.

**Choice:**
- **App Sandbox** is used for the hostile-input children the app ships:
  - `__extract`: a separately signed copy of `agentic`, with `app-sandbox` only, no network, and I/O over stdio.
  - Tesseract: `app-sandbox` plus `inherit`.
  - Local search's Python: `app-sandbox` plus `network.client`, with its state folder in its sandbox container.
- A sandbox self-test runs at start and in Doctor. If it fails, the feature **fails closed**, unless the owner turns on "Run parsers without OS isolation", which is privilege-widening.
- `sandbox-exec` was not adopted.
- The **rendering browser and stdio MCP servers cannot be sandboxed by the app.** The browser is a third-party binary, and an MCP server is an arbitrary program the owner chooses. Their residual risk is stated on the About page, and these compensating controls apply:
  - Chromium's own renderer sandbox.
  - An empty temporary profile.
  - CDP over a pipe, with no debugging port.
  - Loopback blocked.
  - While either is enabled, every CLI decision moves to the UI.
  - From epic 6 on, a Review of a tainted or cross-department payload is resolved only in the UI.
- Remaining unknown: whether python-build-standalone and Granian run under the App Sandbox. Story 12.18 proves it failing-first. If they cannot, the plan and crate notes record why, the About page states the residual risk, and the isolation case asserts the compensating controls instead. This way 12.18's test can pass either way.

## Consistency review

| Finding | Change |
|---|---|
| H1 spike prerequisites hidden | START-HERE gains a "What you provide for the spike" checklist: repo, Anthropic key, LM Studio and/or Ollama with a tool-capable model, two MCP servers, the documents folder in **`AM_SPIKE_DOCS_DIR`**, the sleep/wake demo, Apple Developer ID and notarisation credentials, and a clean Mac. The notarisation demo may wait for the account. Criteria 5 and 8 may be "pending owner input" and are not fails. Story 1.1, `stack-spike.md` and AGENTS (start line) say the same. "Nothing blocks" now reads "no decision blocks". |
| H2 Owner hats | The Owner holds CEO, Vision Creator, Reviewer and Developer in `reference-workload.md`, the reference-company spec (dated restatement), the initiative, story 1.5 and the org mocks ("CEO · 4 hats"). |
| M1 Chief-of-Staff in AD-15 | Changed to "the Coordinator template's learning loop". |
| M2 AD count | AGENTS and the initiative References now say AD-1..AD-47. |
| M3 nine mockups | Changed to thirteen in epic 10 (file and story 10.9 title), `review-rubric.md` and `validation-report.md`/`.html` (dated notes). |
| M4 "green" before 1.2 | Story 1.1 creates a minimal `just check` (fmt, clippy, nextest) and 1.2 hardens it. Stated in story 1.1, AGENTS (Running section and start line) and START-HERE. |
| M5 stale 9.1/9.3 ids | Epic 4 Notes now cite (12.3, 12.5, 9.6). |
| M6 owner locale in defaults | Currency and timezone default from the Mac's system locale and the wizard confirms them. Applied in arch Conventions and AD-9, the initiative (R11, R17, R22, Platforms), the PRD (FR-11, FR-17, persona), EXPERIENCE (schedule picker, budget field, UJ-2), and epics 3, 4, 5 and 6 (stories 3.7, 4.1, 5.16, 6.5). ₹ remains only in placeholder caps and mock sample data. |
| M7 rep workspace | Renamed to "Team workspace (multi-user, access scoped by group)" in START-HERE, the initiative, arch Deferred and Open Questions, the PRD amendment and the glossary. |
| M8 epic 12 ordering | AGENTS "Where things are" and START-HERE now state that table order is build order and never to sort by id. |
| L1 11.4 text | The description starts "DROPPED 2026-11-19…". The title is kept, because its plan file is named from it. |
| L2 companions | Added key-team-sheet, key-schedule-updates and key-mcp-add. |
| L3 CLI gate naming | Story 3.4 is renamed "CLI Owner gate", and R16 says it is resolved under the Reviewer hat. |
| L4 Table T2 / Site S4 | The key-connectors caption marks them as mock-only examples of the fixture Sources. |
| L5 old counts | The initiative's "11 epics / 134 stories" decisions are flagged as superseded counts. |
| L6 "Owner (owner)" and he/his | The `decisions.md` front matter is fixed. Pronouns for the owner are neutral across the live docs, tickets and mocks. |
| L7 START-HERE pointers | Added an Add-ons (v1.x) line, a `final-review-2/` row, and a note that `_archive/` stays local. |

## Security review

| Finding | Change (failing-first tests) |
|---|---|
| H1 taint laundering via AI-written text | AD-10 (a): notes and `ask_seat` questions from tainted steps are `UntrustedData`, and any step taking input from a tainted step is tainted. AD-36 and AD-42: notes are fenced when the writer was tainted. Tests: 4.9 (next step, AI reviewer note, document at rest), 3.16 (tainted reviewer note), 9.18 (`ask_seat`). |
| H2 persistent injection via local writes | AD-10 (b) and (c): in a tainted step, local-write tools write only declared outputs; documents and workspace files carry taint at rest; Proposals from tainted steps are tainted-origin and **never approvable** (Dismiss, or "Start my own edit from this"). RULE-8 wording is in the Untrusted content constraint and trust-gate. Tests: 4.9, 4.10, 9.18, 10.2. |
| H3 incomplete widening classifier | AD-15: one exhaustive `is_widening` with a compile-time match, enforced in the writer on every path. It covers capability on, provider Account, `accepts`, hand-off, event trigger, grant, skill import, imported seats, the engine list, the isolation override, and human→AI reviewer swap or gate removal. New story **4.17** has a property test, a trybuild test and a table-driven test. 4.11, 5.11, 8.6 and 12.19 now use it. |
| H4 outside edits and startup widen | AD-16: an approved grant-set fingerprint means a widening Reload or startup is held behind the two-step confirmation, and nothing runs meanwhile. AD-26: MCP cwd is `.agentic/workspace/mcp/<server-id>/`. New story **4.18**, which has the child-edits-seat-file case. Story 1.9 has a note. 9.6 has a test. EXPERIENCE gains a state, and key-states-keyboard gains its card. |
| H5 no OS sandbox | See the sandbox decision above. AD-13, 45, 46 and 47. New story **4.16**. Changes to 12.18, 12.20, 11.16 (per-helper entitlements), 11.8 (entitlement check) and 11.9 (About page). CLI-class narrowing in AD-15, 6.7, 9.6 and 12.17. |
| M1 review gate | AD-42: the AI reviewer sees the payload only, and the branch is validated. Human→AI swap is widening. EXPERIENCE now says payload only. Tests in 3.16 and 6.13. |
| M2 Rendered Reader | AD-12 and AD-39: `--remote-debugging-pipe` with its own transport on `chromiumoxide_cdp` and `command-fds`, plus `<-loopback>`, `--host-resolver-rules`, `--use-mock-keychain` and `--password-store=basic`. `Browser::launch` and `Browser::connect` are banned. The Stack table is updated. Story 12.17 tests that WebSocket and fetch to loopback make zero connections and that no debugging port is open. |
| M3 SearXNG updates | AD-45 and the initiative: updates come **only by app release**, with no separate command. Engine-list edits are widening. No query text goes in logs. The disclosure is reworded. Stories 12.18 and 12.6 are changed. |
| M4 amplification and ordering | Chain-level budget (`max_chain_spend`) in AD-9 and AD-44, tested in 4.15. Dry-run events start nothing: AD-37, tested in 10.3. `ask_seat` answer runs have no `ask_seat` and also charge the asker: AD-46, tested in 9.18. EXPERIENCE's unbacked "3 starts a day" is replaced by the chain cap. Test ordering: the 3.14 cap test moved to 4.2, the 3.15 Source-taint case moved to 12.1 (3.15 uses a fixture-tainted row), and the 3.13 golden runs offer only table_write and write_document until epic 4. |
| M5 release gate gaps | AD-33, trust-gate and 11.8: every trust and engine suite runs by directory. Entitlements are checked per binary. 11.7 fuzzes HTML, feeds, sitemaps, images and OCR input, link lists and CDP framing. 11.6's SBOM lists Python, SearXNG with its locked packages, Granian, Tesseract, Leptonica, the model and pdfium. |
| M6 query_table and calculate | AD-6 and AD-46: an allow-list authorizer (`v_*` only; `sqlite_master` and `knowledge_fts` denied) with `query_only`, extensions off, SQLite limits and a memory cap. `calculate` has character, nesting, digit, exponent and time limits. Tests in 8.2 and 9.17. |
| L1 11.4 | As consistency L1. |
| L2 accepts vs grant | AD-42: `accepts` never adds a grant. EXPERIENCE hand-off text now says "accepted" rather than "within grant". |
| L3 socket path | AD-45: the path is checked against the 104-byte `sun_path` limit and named `run/s.sock`. Test in 12.18. |
| L4 OCR Account | AD-46 and 12.20: the confirmation says page images are sent. |

## UX review

| Finding | Change (mockups re-rendered in headless Chrome and inspected) |
|---|---|
| 1 Capabilities pushes Brain off-screen | The order is now Identity, Brain, Capabilities. Capabilities is one compact chip row: on/off chips, with ▾ expand for the four that have a provider choice, a provider label only when not built in, and no per-row "Free" pill. Rarely used sections collapse. Applied in EXPERIENCE, DESIGN (`capability-chip-row`), R58, R81, epic 5 R18/R21, story 5.17 and key-seat-sheet §1–2. |
| 2 no 60-second creation path | Add-seat popover (Name, Role template with its capabilities, Model, Create). Added in the EXPERIENCE component row, Flow 5, the DESIGN popover, initiative R24, epic 5 R23, story 5.10 (owner-timed under-60-seconds acceptance) and **key-seat-sheet §5** (light and dark). |
| 3 zero-key first run overstated | key-first-run is redrawn on the zero-key path: locale-prefilled step 1, collapsed "Add a key later", step 4 "Cost ₹0" in the system currency, and a new no-server step 2 (Get LM Studio / Get Ollama, then Detect again, never a terminal). Updated R22, EXPERIENCE states and UJ-2, stories 5.12 and 5.16, and the key-schedule-updates card. |
| 4 terminal text | "The app is not running. Open Agentic Manager to reconnect." with Reopen the app. Updated EXPERIENCE, epic 5 R16, story 5.13 and key-states-keyboard. |
| 5 org chart will not scale | There is no global Flows toggle. Hand-off lines are drawn only for the selected department. Grant lines appear only on hover and in the Team sheet, and a grant merges into a hand-off between the same pair. Updated EXPERIENCE, DESIGN, the F key, stories 5.18 and 7.11, key-org §1–2 and the key-team-sheet text. |
| 6 index row clips | Replaced by a five-group jump row (⌥1–⌥5) with sticky section headers. |
| 7 label overlap | `.state` cards get top padding and auto height. Fixed in key-schedule-updates and key-states-keyboard. |
| 8 wizard footer cramped | The wizard panel uses min-height with 24 px bottom padding and a shorter footer line. |
| 9 several two-step reviews | One "Review 3 changes…" dialog applies with one confirm token (AD-15). A shipped role template needs no confirmation. Updated in story 5.9 and key-seat-sheet §2. |
| 10 cost invisible on paid provider | A price per search shows in the provider menu and in the dialog. Updated in EXPERIENCE, stories 5.9 and 12.19, and the mock. |
| 11 sidebar inconsistent | The shared generator sidebar now includes Connectors in every mock, and its Department 1 dot follows the run state. |
| 12 sample-state drift | Every at-rest mock is one moment: F1 done, Report R1 waiting. Only the running mock shows branches. |
| 13 duplicated lineage words | The role word is dropped when it repeats the name, and branches read "parallel N of 3". |
| 14 faded half-on switch | A pending chip is drawn off with an amber outline and dot (DESIGN and EXPERIENCE). |

## Privacy (open-source repository)

- Every `.memlog.md` under `_bmad-output/**` and the party-mode memory was scrubbed of the owner's personal name, company and product names, staff names and earlier domain content. These were replaced with "the owner" and "[removed]", and each log ends with a privacy-scrub entry.
- The final-review files were scrubbed the same way.
- Owner pronouns are neutral across the live docs.
- `/home/claude/am/.gitignore` excludes `_archive/`, `target/`, `node_modules/`, `.agentic/` and `*.png`. This covers the owner's private CrewAI screenshot, which the reconcile file now notes stays local.
- START-HERE says `_archive/` stays local.
- The required case-insensitive grep over the tree, excluding `_archive` and `.claude`, is empty. It covered the owner's first and last name, the company, distributor and product names, the old domain terms, and the staff names listed in the task.

## Validation

- `tickets.py status`: store repo; 170 entries (169 planned, 1 dropped). `unpinned_after`, `undeclared_after` and `order_conflict` are all empty. Epic 4 now also declares its dependency on epic 1. `tickets.py next`: 1.1 is the only story ready to start.
- `lint_spine.py` on the architecture workspace: ok, 0 findings.
- Every `tickets.toml` parses as TOML.
- The AGENTS.md managed block was refreshed headless. It covers the AD-1..AD-47 count, taint, the classifier, App Sandbox helpers, CDP over a pipe, SearXNG updates by release, table order as build order, locale defaults and the trust-test families. Its start lines name the spike inputs and the minimal `just check`.
