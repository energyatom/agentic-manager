# Final review 2 - consistency, cleanliness, start readiness

Reviewer: independent fresh reader. Scope: live docs only (`_archive/` not read). Method: grep sweeps plus a script over all `tickets.toml` files and the BMad `tickets.py`.

## Verdict

**Ready to build, with 2 high and 8 medium fixes to make first (all small text edits).** No critical issues. The set is internally consistent on the things that usually break: 12 epics, 167 entries (166 planned, 11.4 dropped, confirmed by `tickets.py status`), 64 FRs, 82 Rs, 47 ADs, 13 mockups all match their stated counts; no dangling `after` ids, no cycles, no forward-in-build-order dependencies; `tickets.py next` returns exactly one ready story (1.1); no link or id into `_archive/` other than the two "do not read" notes; zero hits for the owner's name, employer, products or personal context; no domain vocabulary beyond Company A. Stale terms (Chief of Staff, lead seat, Basic no-key search, Firecrawl-required, LAN in v1, Windows in v1) appear only inside dated "renamed/retired" notes, with the exceptions listed below.

Counts: **Critical 0, High 2, Medium 8, Low 7.**

## High

**H1. The spike's owner prerequisites are hidden from the start documents.**
`START-HERE.md` ("Before you start", "Epic 1 story 1 can start now") and `AGENTS.md` (Build team and start) say no owner question blocks the start and mention only the 3 real documents. Story 1.1 (`tickets/epic-foundations-rust-spike/tickets/tickets.toml` entry 1) actually needs: a GitHub repo, Apple Developer ID and notarisation credentials (criterion 8), an Anthropic key, LM Studio and Ollama with a tool-capable model, two real MCP servers, the sleep/wake and clean-Mac demos, and the documents path in "an environment variable" whose name is defined nowhere (`stack-spike.md` criterion 4 too).
Fix: add a "Before the spike (what you provide)" checklist to START-HERE and one line in AGENTS pointing at it; name the variable (for example `AM_SPIKE_DOCS_DIR`) in `stack-spike.md` and story 1.1; state which criteria may be marked "pending owner input" (8, 5) so Claude Code does not stall. Reword "nothing blocks the start" to "no decision blocks; these inputs are needed during the spike".

**H2. Company A's Owner hats disagree, and the tests need more hats than the sample gives.**
Owner is "CEO hat" in `initiative-agentic-manager.md:293` and `reference-company-spec-2026-11-18.md:19`; "CEO, Reviewer" in `reference-workload.md` (Org chart) and spike toml entry 5 text. But the same sample uses a Vision Creator note (`reference-workload.md` "Sources, skills", `prd` UJ-3) and epic 10 tests apply a proposal under "You (Developer hat)" (`epic-polish-dry-run-learning/tickets/tickets.toml:59-60`).
Fix: one line everywhere: Owner hats = CEO, Vision Creator, Reviewer, Developer.

## Medium

**M1. "Chief-of-Staff" survives in a binding spine.** `architecture-agentic-manager.md:216` ("the Chief-of-Staff learning loop"); AGENTS forbids the name. Fix: "the Coordinator template's learning loop".

**M2. AD count stale in two places.** `AGENTS.md:6` "AD-1..AD-46" and `initiative-agentic-manager.md:209` "AD-1 to AD-46" (AD-47 exists and AGENTS itself cites it). Fix: AD-1..AD-47.

**M3. "Nine mockups" after there are thirteen.** `epic-polish-dry-run-learning/epic-polish-dry-run-learning.md:32,48,64`; story 10.x title "Design QA against all nine mockups" (`.../tickets/tickets.toml:105`); `ux-agentic-manager/review-rubric.md:5`; `validation-report.md:9`. Risk: QA skips key-connectors, key-team-sheet, key-mcp-add, key-schedule-updates. Fix: "all thirteen mockups" (or "every file in mockups/").

**M4. "Green" before 1.2 is defined two ways.** `AGENTS.md` (Build team and start): until 1.2, green = spike `cargo test`; story 1.1 verify requires "`just check` is green on the spike workspace" while the Running section says `just check` is created by 1.1 and gated by 1.2. Fix: say 1.1 creates a minimal `just check` (fmt, clippy, nextest) and 1.2 hardens it.

**M5. Stale story ids in epic 4 notes.** `epic-triggers-budgets-safety-floor.md:102` cites "epic 9 ... (9.1, 9.3, 9.6)"; 9.1 and 9.3 moved to 12.3 and 12.5, and epic 9 now has ids 5-13, 16-18. Fix: "(12.3, 12.5, 9.6)" after confirming 9.6.

**M6. Owner locale baked into generic defaults.** Company timezone default "Asia/Kolkata" (`architecture` Consistency Conventions; also EXPERIENCE, epics 4, 5, 6) and ₹ caps in the sample and START-HERE. For an open-source, generic product the default should come from the system (timezone, currency), with ₹/INR only as the owner's sample values. Fix: default = system timezone/locale, wizard confirms; keep ₹ only in placeholder caps and mockup data.

**M7. "Rep workspace ... per rep and per state" is sales-flavoured wording.** `START-HERE.md`, `initiative:182,184,288`, `architecture:757,763`, `prd`. Contradicts the "no domain specifics" direction though the owner used "rep". Fix: "team-member workspace scoped per member and per scope" with one parenthetical noting the owner's term.

**M8. Epic 12 vs epic 9 ordering note is easy to misread.** START-HERE says epic 12 "builds between epics 8 and 9" but story ids 12.x are listed after 11 in `tickets.toml`; correct by build order, but START-HERE should say "table order is build order" once so Claude Code does not sort by id. Fix: add that sentence to AGENTS "Where things are".

## Low

- L1. `epic-desktop-release-trust-gate/tickets/tickets.toml:54-63` (11.4) still carries the full Windows/Linux description, `hitl = true`, an `unknown`. `tickets.py` treats it as dropped via the plan file, so harmless; fix: prefix the title "DROPPED:" or trim.
- L2. `initiative-agentic-manager.md` frontmatter `companions:` lists 10 of 13 mockups (missing key-team-sheet, key-schedule-updates, key-mcp-add); add them.
- L3. Epic 3 description and `tickets/tickets.toml` say "CLI Owner gate"; story 3.4 and decisions say "CLI Reviewer gate". Pick one ("Owner gate" matches Company A).
- L4. `key-connectors.html` uses "Table T2 / Site S4", not defined in Company A (only `table-t1`). Rename to T1 or note as mock-only.
- L5. `initiative` Notes lines ~244-248 keep "11 epics / 134 stories" decisions; fine as history but add "(superseded count)" to avoid miscounting.
- L6. `party-decisions/decisions.md` front matter "Owner (owner)" is a scrubbing artefact; and "his/he" for the owner in `trust-gate.md`, `reference-workload.md`, `prd` (use "the owner").
- L7. START-HERE does not mention the Add-ons framework (v1.x), and its `final-review/` row will need updating to point at `final-review-2/` once that review is accepted.

## Checks with no finding

Role template names (8) and "Ask a seat" consistent in AGENTS, START-HERE, glossary, PRD, EXPERIENCE, epics. Platform: macOS Apple Silicon only, Linux portability job, Windows/Linux v1.x, consistent (AD-47, trust-gate, PRD, epics). Loopback-only owner access in v1, LAN only with the v1.1 Team release: consistent. Crawl service is optional/self-hosted, never required. Local search is the only search default; "Basic search" appears only as a retired assumption and a "do not say" row. Capability defaults (Documents on, others off, templates/sample switch on what they need) are consistent across PRD FR-63, initiative R81, AD-46, glossary; that is compatible with Open by default because the zero-key golden run uses the sample's switched-on capabilities. All 13 mockups exist; all AD/FR/RULE/R ids referenced resolve. Owner's direction items (generic engine, Company A, F1/F2, built-in capabilities per agent with Account swap, generic connectors, Agent card sections, accounts then models, meter/estimate/caps, human hats, Assistant propose-then-approve, Team release v1.1, Add-ons v1.x, Tauri spike-gated) are all present; no gaps found beyond H1 and M6/M7.
