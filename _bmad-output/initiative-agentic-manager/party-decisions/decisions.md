---
title: Agentic Manager — Party decision record, round 1
date: 2026-11-17
room: John (PM), Mary (Analyst), Sally (UX), Winston (Architect), Sofia (PO), Amelia (Dev), Rhea (guest, Security)
mandate: The owner delegated these decisions to the room. Tie-breaks were made by the orchestrator against the owner's five guides — 1 user friendly, 2 aesthetic, 3 trustworthy, 4 safe/secure, 5 comprehensive to what we discussed. Guide 5 means anything the owner explicitly asked for stays in v1 unless the room shows it is unsafe; it may be simplified, not dropped.
positions: archived 2026-11-18 (historical; not part of the live plan)
status: binding input for PRD, UX, initiative, architecture and tickets, as amended 2026-11-18 by the owner's reference-company spec (`../reference-company-spec-2026-11-18.md`), which wins where they conflict.
---

# Decisions

## Corrections to the owner's original input (unanimous)
- **C1 Licence is MIT** (owner decision). The earlier "Apache-2.0" line is superseded.
- **C2 Stack is Rust-first** (owner request, see D4). The earlier Python-shaped stack sketch (FastAPI, SQLModel, APScheduler, PyInstaller) is superseded by the architecture document.
- **C3 Dogfood before v1.0.** The earlier milestone "v1.0 tagged; dogfood starts" contradicted its own definition of done. v1.0 is tagged only after 4 clean weeks on a frozen build.
- **C4 Reviewer gate exists from the first runnable milestone** as a CLI approve/return prompt; the UI gate comes later.
- **C5 Add a SQLite schema-migration policy** (long-lived installs).
- **C6 Files are the source of truth for config.** The UI writes files; external edits are detected and reloaded with a visible prompt (no silent two-way sync).

> **Amendment note (2026-11-18, owner REPLAN).** The owner's reference-company spec removes the earlier domain reference department from the live plan and brings cross-department work into v1. Workload-specific wording in this record was neutralised in place, each change marked; the position files and the owner's original input document are archived and no longer cited.

## D1 Cross-department collaboration — hand-off through shared tables and documents (unanimous on scope)
- v1: a table or document is owned by one team; another team's seats can be given an explicit **read-only grant** in config. Grants are visible in the org chart as faint dotted lines between team boxes, labelled with what is shared (hover/selection only; reporting lines stay solid and vertical). Hand-offs needing a human go through the Inbox.
- Granted tables are read as a snapshot at run start (reproducible runs).
- Each department is a trust boundary (security).
- ~~v1.x: routing across teams, inter-team workflow invocation, event triggers — need a cross-team contract nobody has defined yet.~~ *Amended 2026-11-18 (owner REPLAN): cross-department hand-offs, parallel fan-out and fan-in, cross-department review gates, advice lineage and workflow-triggers-workflow come into v1 as generic engine primitives; the contract is arch AD-40 to AD-44 (initiative R73–R77).*
- Genericity proof (amended 2026-11-18): the abstract sample company, Company A (an Owner and departments D1 and D2), proves in CI a read grant and every cross-department primitive through generic config.

## D2 Cost estimate and live meter (unanimous)
- **Enforcement before estimation.** A per-call spend ledger with reserve-then-settle hard caps (call/step/run/seat/week) is built first; the estimate is UX, the ledger is the guarantee. A wrong low estimate never raises a cap.
- **Estimate = a range with its basis**, never a single number: first runs use prompt tokens × expected loop iterations × price, labelled "rough · no history"; after 3+ runs, p50–p90 from history, labelled "based on your last N runs". Shown inline under the Run button (never a blocking modal) and updated as the workflow is edited. Estimate vs actual shown after every run.
- **Live meter** = a calm pill beside the company name: weekly spend vs budget as a thin ring at rest; while running, ticks (tabular figures, ~1 Hz visual change); amber at 80%, stops at 100% with an Inbox ask. Per-seat spend is a secondary line on expanded seat cards. Spend comes from provider usage fields; local models show tokens, ₹0 and a time estimate. Currency INR and USD.
- Unattended (scheduled) runs have a lower default cap than manual runs.

## D3 Accounts and model assignment (unanimous)
- **Account** = one connection: provider kind + base URL + credential reference (OS keychain) + friendly name + health + discovered models. Lives in Models & Costs and the wizard, never inside a seat. The owner adds a key once; every seat picks **Account › Model** from one grouped menu (n8n-credential style).
- Local servers are auto-detected on loopback well-known ports only (LM Studio 1234, Ollama 11434; no LAN scans) and appear as already-connected accounts with an "On this Mac" badge — the same card and menu as cloud. Local is never an "advanced" path.
- Capability chips (Tools, Structured output, Context, Cost/"Free (local)") come from a shipped, editable metadata file, overridden by a **Test this model** probe (one tool call + one schema output); each flag is marked known or tested. The *loaded* context window is read from the local server and set explicitly; mismatch warns.
- The picker warns in plain words, never blocks. Each role template has a "Recommended" model resolved from existing accounts; a company default model pre-fills new seats.
- Prices live in an editable file with a "last updated" date. Each Account pins its egress host.
- FR-47 benchmark is folded into "Test this model".

## D4 Stack — Rust core, conditional on a spike (unanimous)
- Rust core in one binary: engine, local HTTP/WebSocket server, scheduler, SQLite. React + TypeScript web UI embedded in the binary. Tauri desktop shell wraps it. Browser mode ships first.
- **Gated by a 2-week spike, the first story of the first epic**, with written pass/fail criteria: (1) provider trait — Anthropic + OpenAI-compatible streaming with tool calls incl. LM Studio, cancellation; (2) kill -9 mid-run then resume on SQLite WAL; (3) cron + timezone + catch-up after macOS sleep/wake; (4) PDF/xlsx/docx extraction on 3 real long, multi-column documents the owner supplies (wording amended 2026-11-18), behind a swappable trait; (5) MCP client (rmcp) stdio + HTTP against 2 real servers; (6) loopback server Host/Origin/token checks incl. WebSocket; (7) OS keychain; (8) signed + notarised Tauri build on a clean Mac; (9) agent-productivity check — Claude Code agents complete a representative story with the test loop. Failing 2+ criteria → the architect proposes a hybrid and the plan is replanned.
- *Amended 2026-11-19 (owner answers; original above kept): criterion 4 reads a mixed folder of the owner's documents (xlsx, pptx, docx, PDF incl. scanned) named by AM_SPIKE_DOCS_DIR; criterion 5 uses two official MCP reference servers the spike launches (stack-spike.md); criterion 8 is deferred until the owner's Apple Developer account exists, is not counted in the two-failure replan rule (criteria 1-7 and 9), and gates the v0.9-rc installer (epic 11 story 3) instead.*
- Pre-1.0 crates (LLM clients, PDF) sit behind our own traits; no foreign types in the domain. Own thin provider adapters preferred.
- Rationale recorded: Rust is chosen for distribution, single signed binary, reliability and maintenance — not raw performance.

## D5 Other humans — owner-only in v1 (unanimous)
- Server binds loopback only; no LAN toggle in v1.
- People outside the app work through **xlsx/CSV export and re-import** of status + reason columns, or the owner edits the grid (amended 2026-11-18: wording made department-neutral).
- The data model already records the **actor** on every status change, comment and approval, so v1.x LAN mode (named tokens, expiry, HTTPS) can be added without migration pain.

## D6 Assistant powers — propose, then approve (unanimous)
- Reads freely through a **read-only SQLite connection** (not a string filter), with row/time limits; answers cite rows/runs.
- Cannot run workflows, change config, add tools/MCP servers or touch keys directly. It emits a structured **proposal** rendered by the app as a diff card (Apply / Edit / Dismiss); the approval must come from a real UI event. "Run now" is a proposal card showing the estimate.
- No outbound tools while it holds table data. Model output is never rendered as raw HTML/JS; markdown sanitised; no remote images.
- Privilege-widening proposals (new tool, MCP server, bigger budget, new egress host) are flagged high-risk with a plain-language effect and a second confirmation. No global auto-approve.
- Assistant-built, pinned dashboards stay in v1 (owner asked explicitly), delivered after saved table views, which are a Must earlier (amended 2026-11-18).

## D7 Templates (unanimous)
- *Amended 2026-11-18 (owner REPLAN, reference-company spec): the earlier domain flagship template is removed from the plan.* Ship the **Blank department** + the full set of generic **role templates** + the optional, clearly labelled **"Sample company: Company A (two departments)"**. No domain department templates ship in v1, and none is shown as a fake "ready" card.
- The engine and UI contain no department vocabulary; templates carry the nouns. CI loads the sample company through the same loader as any user department and fails on department-specific code paths.
- Imported or shared templates are shown as a diff and never auto-run (they can define tools, MCP servers and schedules).

## D8 Web search (majority)
- A **no-key "Basic" search** ships so first run needs no signup; labelled Basic, flagged as limited in Doctor. *(Amended 2026-11-18, owner principle Open by default: replaced by the bundled, supervised Local search (SearXNG), the default provider; keyed providers are optional; see initiative Notes and arch AD-45.)*
- The wizard recommends a free-tier keyed provider as the quality upgrade (Brave, Tavily or Exa — a spike story compares them on the owner's real queries before the default is fixed (wording amended 2026-11-18)).
- Search results and fetched pages are untrusted content. SearXNG is bring-your-own only (hosting conflicts with zero-maintenance). *(Amended 2026-11-18: superseded; the app bundles and supervises SearXNG itself, so the owner maintains nothing.)*

## D9 CrewAI / Paperclip import — cut from v1 (6 of 7; Mary dissented for CrewAI YAML)
- Documented how-to instead (the format is plain files). CrewAI YAML import is a v1.x candidate after an outside adopter asks.

## D10 Release shape (synthesis of John, Sofia, Winston)
- No calendar deadline (owner has none). Milestones of about two weeks, each ending in a 10-minute demo and a design-QA check against the mockups.
- Private tags: **v0.1** = engine + CLI + schedule + drop folder + budgets + CLI gate (owner shadow-runs beside Paperclip). **v0.2** = minimal web UI (Org view, Run view, Inbox, table grid) — **the 4-week dogfood clock can start here** on a frozen build. **v0.3+** = Assistant, dashboards, MCP, Doctor, polish. **v0.9-rc** = signed desktop installers + trust gate green.
- **v1.0 public** = 4 clean weeks with no manual repair (a manual repair resets the clock) + trust gate (D11-B) green + one-click install + docs.
- *Amendment 2026-11-18 (initiative Decision "Clean weeks", owner delegated; the record above is kept as decided):* the official 4-week dogfood clock starts after E9 (web search, MCP, Doctor) on a frozen build, not at v0.2; v1.0 also needs at least the final 2 clean weeks on the v0.9-rc build; moving to v0.9-rc does not reset the clock, only a manual repair does. From v0.1 the owner shadow-runs beside Paperclip.
- Recommended to the owner (his call): before M0, produce one manual advice note with Claude Code, its lineage written out, to test the riskiest assumption — that the owner acts on it; and record baseline numbers (current weekly maintenance time, gate approval rate) now (wording amended 2026-11-18).
- *Amendment 2026-11-19 (owner, binding; the recommendation above is kept as recorded):* the manual advice note before M0 is dropped (prd OQ-3 closed), because the app is a generic agent manager; what departments produce is the user's choice; the owner's own use of Paperclip and CrewAI already establishes the need; recording the baseline numbers stays (prd OQ-2).

## D11 Trust and safety package (unanimous; Rhea's split adopted)
**Product requirements (each with a failing-first CI test):**
1. Untrusted-content handling: all web/document/MCP/table free-text content is tagged untrusted in the engine and never concatenated into instructions. **Reader/actor separation:** a step that has read untrusted content cannot, in the same step, call a tool with external effect without approval; it can write only to its declared table.
2. Egress control: per-seat/per-account host allow-list; fetch blocks private, link-local and metadata IPs (SSRF), pins DNS, caps redirects and size, strips credentials.
3. Action classes: every tool is read / local-write / external-effect; external-effect needs approval by default, and the approval shows the exact payload. **v1 ships no send tools** (email and posts are drafts only).
4. Budget and kill switch: hard caps (reserve-then-settle), max iterations / tool calls / loop-backs, wall-clock timeouts, circuit breaker; a visible **Stop everything** button in the toolbar plus `agentic stop`; schedules pause on breach.
5. Secrets: keychain only; never in prompts, logs, traces, exports or config; redaction pass on logs; exports secret-scanned; Doctor warns on keys found in files.
6. MCP allow-listing: nothing available until added and assigned per seat; the add screen shows exact command, args, env and network behaviour; stdio servers run with a scrubbed environment and workspace cwd, pinned version; tool descriptions hashed at approval and re-approval required on change.
7. File safety: workspace-confined, canonicalised, symlink-escape rejected; parsers with size/page/zip-bomb limits; drop-folder files deleted only after successful processing.
8. Local server: loopback bind, random per-launch token (never in URL or logs), Host allow-list, Origin check incl. WebSocket upgrade, no CORS wildcard; data dir and token files 0600, refuse to start if world-readable.
9. Audit trail: append-only run log of model calls, tool calls (redacted), approvals and row provenance; trace export.
10. Privacy: no telemetry; calls outside configured endpoints blocked and shown in Doctor; deletion really deletes (incl. backups per policy); a per-seat **"What this seat can touch / what leaves this machine"** panel.
11. Learning-loop proposals (FR-111) are approval-gated and show their source rows (persistent-injection defence).
12. Desktop hardening: minimal Tauri capabilities, strict CSP, no remote content in the webview, narrow IPC, updater with pinned public key, no downgrade, never auto-install without consent.

**Release and process rules (gates for v0.9-rc / v1.0):** macOS signing + notarisation + hardened runtime, Windows signing, Linux signed checksums; separate update-signing key with rotation plan; cargo-deny, cargo-audit, npm audit; locked dependencies; Actions pinned by SHA; least-privilege CI tokens; branch protection + required review; SBOM (CycloneDX/SPDX) and build provenance per release; OpenSSF Scorecard badge; SECURITY.md with disclosure channel and response target; published threat model; security tests as release gates (rebinding/CSRF/WebSocket, path traversal, prompt-injection regression pack, budget/kill-switch, parser fuzzing, secret-leak scan); community review window before v1.0; an honest "what this app protects you from, and what it does not" page.
- *Amendment 2026-11-19 (owner: "Priority is the Mac build. If it's OK, the entire Windows and Linux build can wait for later."; the rules above are kept as decided):* v1 releases only the macOS Apple Silicon build, so v0.9-rc and v1.0 are gated by macOS signing, notarisation and hardened runtime; Windows signing and Linux signed checksums move with those platforms to v1.x (trust-gate.md, Deferred to v1.x). D4's stack is unchanged and the code stays portable.

## D12 Name (owner decides later)
- Working name stays **Agentic Manager**. Shortlist for the open-source release: **Staffroom** (Mary's and Sofia's pick), **Seatwork**, **Deskwork**, **Orgline**, **Cadre**. Avoid "Agent", "Crew", "Paperclip" derivatives. Check trademark, GitHub, crates.io, npm and domain before choosing.

## D13 Scope trims and additions
**Kept because the owner asked (guide 5), simplified:**
- **Developer hat (FR-34)** — kept as a human hat that approves config/tool/model changes in its subtree using the same proposal-and-approval mechanism, with version history. No new concept.
- **Assistant-built dashboards (FR-92/93)** — kept (D6).
- **Dry-run (FR-24)** — kept as a trust feature: run with a cheap/local model substituted, before paying.
- **Ad-hoc question to a seat via the Chief of Staff (FR-35)** — kept minimal: a "Ask Chief of Staff" box that becomes an Inbox thread; direct messaging to any seat is v1.x. *(Amended 2026-11-18, reconcile: renamed "Ask a seat", addressed to any seat the owner picks, by default the top Coordinator or head seat; "Chief of Staff" is not a role-template name.)*

**Added (from owner input or the room):**
- Accounts (D3), live meter + estimate (D2), table grants (D1), Stop everything (D11), reader/actor separation, egress allow-list, "what this seat can touch" panel.
- Undo/version history for every config edit, surfaced generally.
- "While you were away" digest at the top of the Inbox on open.
- Saved table views promoted to Must.
- Actor field on all human actions (D5).
- SQLite migration policy (C5).

**Deferred to v1.x:** FR-5 import, FR-15 health overlay, ~~FR-25 parallel branches~~ (in v1 from 2026-11-18), FR-46 caching, FR-57 concurrency beyond a per-provider limit, FR-64 file-drop trigger, FR-65 webhooks, FR-74 email-draft via MCP, FR-96 dashboard export, FR-102/103 notifications, FR-112 knowledge base, FR-122 SKILL.md export, LAN mode, ~~cross-team routing~~ (cross-department hand-offs in v1 from 2026-11-18), domain department templates, mini-map, 100-seat performance target (v1 target: 30 seats smooth).

## Epic order (Sofia's slice, adjusted)
E1 Foundations + Rust spike · E2 Accounts and model routing · E3 Workflow engine + CLI (+CLI gate) · E4 Triggers, inputs, budgets and safety floor (→ v0.1) · E5 Org and Seats UI with live cost meter + first-run wizard · E6 Runs, Inbox and Documents UI (→ v0.2, dogfood may start) · E7 Tables, grants and saved views · E8 Assistant and dashboards · E9 Tools, MCP, search and Doctor · E10 Polish, dry-run, learning loop, design/a11y QA · E11 Desktop, release and trust gate (→ v0.9-rc; v1.0 after 4 clean weeks).
