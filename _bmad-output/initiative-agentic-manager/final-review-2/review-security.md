# Security review (final review 2) — Rhea

**Verdict: CONDITIONAL. Do not start epics 3 and 4 until the 5 High findings are written into the AD text and the stories. The architecture is sound and most rules have a failing-first test. The new cross-department, capability and local-service surface has gaps where text is trusted or confirmation is skipped.**

Counts: Critical 0 · High 5 · Medium 6 · Low 4.

## Traceability (risk → AD → story → test status)

| Risk | AD | Story | Status |
|---|---|---|---|
| Taint across hand-off | 10, 41 | 3.15, 4.9 | Test exists. 3.15's verify needs Source-load taint (12.1) and 4.9 taint, both built later. No test for a tainted AI reviewer (AD-10 marks it only). |
| AI review gate authority | 42 | 3.16 | Partial. Asserts no resolve path. Nothing on human→AI swap or tainted payload (H1, M1). |
| Fan-out budget | 40, 9 | 3.14, 4.2 | 3.14 "budget" test cannot fail first: 3.7 says "no cap is checked (epic 4)". 4.2 has no concurrent-branch test. |
| Trigger chain | 44 | 4.15 | Good (depth, cycle, coalesce, stop). No chain spend, dry-run or ask_seat coverage (M4). |
| Lineage | 43 | 3.17 | OK. |
| SearXNG child | 45 | 12.18, 11.16 | Good except H5 and M3. |
| Rendered Reader | 39, 12, 13 | 12.17 | Gaps in M2. |
| OCR | 46, 30 | 12.20 | Test OK; no fuzz target (M5). |
| Capability toggle | 46, 15 | 12.19, 5.17, 8.6 | Only the API path is tested (H3, H4). |
| Provider swap | 46, 12, 17 | 12.19, 12.6 | OK (no silent fallback, keychain). OCR disclosure gap (L). |
| Calculate and analyse | 46, 6 | 9.17 | No code-execution path found. See M6. |
| Workspace / memory / ask | 46, 10 | 9.18 | No taint test (H2). |
| macOS sign / update | 47, 32 | 11.2, 11.3, 11.16 | Partial (M3, M5). |

## High

**H1. Taint laundering through AI-authored "trusted input".** AD-36 and AD-42 pass a loop-back or return note to the producing step "as a trusted input", yet a tainted AI reviewer wrote it. AD-10 only marks the verdict tainted in lineage. The same gap exists in `ask_seat` (AD-46), where the answer is fenced but the question is not. A tainted asker's question reaches a clean answer step as text, and that step can fetch URLs or run queries carrying data.
*Fix:* a note or question written by a tainted step is `UntrustedData` and taints its recipient. Add to AD-10 and to tests in 3.16, 4.9 and 9.18: an injected payload makes the AI reviewer return a note, and the producer is tainted. Within-run next-step taint propagation is also stated only for hand-offs, so add an explicit test for it.

**H2. Persistent injection through local-write tools.** AD-10 limits a tainted step only on `table_write`. `workspace_write`, `remember`, `write_document` and `propose_change` are not restricted. The workspace is never deleted, and a later clean `workspace_read` is not in AD-10's untrusted list. 4.10 and 10.2 allow tainted memory or instruction Proposals, which contradicts RULE-8's "zero approvable proposals".
*Fix:* in a tainted step these tools write only the declared outputs. Workspace files carry the taint of their writer and a read returns `UntrustedData`. Tainted-origin Proposals always need the second confirmation, with the text shown in full. Reword RULE-8 to match. Failing-first tests in 9.18, 4.9 and 10.2.

**H3. Privilege-widening classifier is incomplete.** AD-15 lists cap, tool, host, MCP and loopback Account, and 8.6 tests "five kinds". Missing: capability on, provider Account, `accepts`, hand-off, event trigger, reviewer changed human→AI, skill import, SearXNG engine list. The 4.11 and 5.11 import diffs name only tools, MCP, schedules and hosts, so an imported seat file with `capabilities: on` is not flagged.
*Fix:* one exhaustive `is_widening(patch)` in `am-config`, with a compile-time match on the schema, enforced in the writer rather than the UI. A property test across Proposal, import, Assistant and API proves each widening field is flagged. Update AD-15's list.

**H4. Outside edits and startup load widen without confirmation.** AD-16 and 1.9 let "Reload" apply an outside edit with a single click. Config is also read at launch. An MCP child's cwd is "the workspace", which is ambiguous. If it is the project, any same-user process can enable capabilities or hosts. RULE-15's "same-user child cannot widen" is tested only against the data dir.
*Fix:* persist the last-approved grant-set fingerprint in the DB. A Reload or startup that raises it requires the widening confirmation, and runs are blocked until then. Define "workspace" in AD-26 as `.agentic/workspace/mcp/` and add the child-edits-seat-file case to 4.3's harness.

**H5. No OS sandbox for the same-user children.** The `__extract`, `__render` and OCR children and SearXNG are "no path to the data dir" by convention only. They parse hostile input and run under the owner's uid, so a pdfium, Leptonica or Chrome bug can read the 0600 CLI token. The CLI class includes Review resolve, schedule resume and run start, and "interactive TTY" is not authentication because a process can open a pty. Story 12.18's "child cannot open a project file" test cannot pass without a sandbox.
*Fix:* a macOS Seatbelt profile per child (deny reads of the data dir and project, deny network except the proxy) behind the `LocalService` port, and use it in AD-47's adapter. Require a UI session to resolve a Review on a cross-department hand-off or on tainted payloads. Reword the About page.

## Medium

**M1. Review gate.** Nothing stops a human gate being swapped for an AI review gate (H3). The verdict's `branch` is model-chosen and must be validated against real branches. AD-42 says the reviewer sees only the payload, while EXPERIENCE.md says it also gets lineage; choose payload only. Tests: a tainted-payload injection tries to approve, and a reviewer branch outside the fan-out is refused.

**M2. Rendered Reader.** Chrome bypasses `--proxy-server` for loopback by default, and the CDP Fetch domain does not see WebSockets. A page can reach local services (Ollama, Firecrawl, MCP HTTP). Add `--proxy-bypass-list=<-loopback>`, `--host-resolver-rules`, `--use-mock-keychain` and `--password-store=basic`. State the CDP port exposure (loopback TCP) and test that a fixture page's `new WebSocket("ws://127.0.0.1:<fixture>")` produces zero connections. The 12.17 tests cover fetch only.

**M3. SearXNG update channel.** The source and signature check of a new tree are unspecified, and a compiled-in manifest would reject any out-of-band tree. No story builds "Update Local search…" (12.18 has only the command). Require a tree manifest signed by the update key and consent from the UI session. Editing the engine list adds proxy hosts and needs confirmation. The disclosure "never your files or tables" is false when an untainted step builds a query from structured row fields. Reword it, and test that SearXNG logs hold no query text.

**M4. Amplification and ordering.** Chains have depth only. Add a company-week chain budget test in 4.2 or 4.15. Dry-run `workflow_completed` events must not start real runs (no ticket covers this). `ask_seat` recursion (A→B→A) is unbounded: the answer run gets no `ask_seat` and charges the asker's scopes. EXPERIENCE.md's "daily limit" is backed by no AD. The epic-3 golden run with a real model (3.13) precedes ToolGate and taint, so it must use no tools. Move the 3.14 budget and 3.15 Source-taint assertions to 4.2 and 4.9, or add stubs.

**M5. Release gate gaps.** 11.8 runs only rule_08..17. Add `trust/local_search_*`, `capabilities_*`, `connectors_*` and `engine/*` (cross-department). 11.7 has no fuzz target for HTML, feeds, images or OCR. 11.6's SBOM omits Python, SearXNG, Tesseract and pdfium. Add per-binary entitlements and a check that the main app has no `disable-library-validation`.

**M6. query_table and calculate.** Add an authorizer that allows only `v_*` views (not `sqlite_master` or `knowledge_fts`), `query_only`, extensions off, and memory and row limits. `calculate` needs digit and exponent limits.

## Low
- L1. Ticket 11.4 (Windows and Linux) is still in `tickets.toml` though its plan file says dropped.
- L2. Clarify whether `accepts` replaces a grant: UX says "within grant", AD-41 says `accepts`.
- L3. Unix socket path may exceed 104 bytes.
- L4. For an OCR Account, the confirmation must say that page images are sent.
