> **Historical — superseded 2026-11-18.** This review predates the owner REPLAN of 2026-11-18 (reference-company spec): the earlier domain reference department it names has been removed from the plan and replaced by the abstract Company A; wording that named that department is neutralised below as "the former reference workload". Read the live initiative, PRD, architecture and tickets instead.

# UX final review (Sally, independent) 

**Verdict:** The plan would largely produce the calm, Apple-grade, CrewAI-easy experience on paper, with strong mockups and checkable ACs for org views, meter, estimate and Inbox, but the two headline promises (a seat in 60 seconds, open-source models as easy as paid ones) are under-verified, and the first-run path is built last.

Counts: critical 0, high 3, medium 5, low 3.

## High

1. **"Seat in under 60 s" is only proven by scripted time.** `tickets/epic-org-seats-ui/tickets/tickets.toml` story 10 verify and story 15; PRD FR-24. A Playwright script finishing in 60 "scripted seconds" says nothing about human ease. Story 15's owner step times the wizard but not Flow 5. Fix: add to 5.15 an owner step that times Flow 5 on his Mac (target under 60 s, mouse and keyboard) and records it in the design QA record; make the wizard bound match the PRD (see M1).

2. **Local-model setup is detection only.** EXPERIENCE State Patterns cover just "no server found"; story 5.5 and 5.12. Missing: server running but no model loaded, server up but model not tool-capable at wizard time, cold-load of a 20B model exceeding the "30-second" test, and how a user without LM Studio gets one (a link or one sentence on what to install is enough). The wizard mock shows "+ OpenAI-compatible URL" (vLLM, llama.cpp, OpenRouter), but 5.5's description and verify never mention adding one in the UI. Fix: add states and ACs to 5.5/5.12 for "running, nothing loaded" (names what to do in LM Studio), slow local test (row says "loading model…", never fails on time alone, Skip test stays), and an OpenAI-compatible URL Account add with the plainly shown host. Add a mock state or spine row for each.

3. **First-run wizard is story 12 of 15 in E5, and before E11 launch means a terminal.** `epic-org-seats-ui` 5.12; `epic-desktop-release-trust-gate` 11.1. The single most important experience is verified last, after Accounts, estimate and import. Also the owner runs `agentic serve` from a terminal for v0.2 through the dogfood freeze, which recreates the "babysit a server" friction he left Paperclip over. Fix: pull a thin wizard (steps 1-2, existing Accounts cards) forward right after 5.5, and give the interim a launch-at-login or menu-bar start (or move 11.1 before the dogfood freeze).

## Medium

- **M1. Contradictory time targets.** PRD FR-54: first run under 5 minutes; SM-5 and UJ-2: under 15; 5.15: wizard under 10; 5.12 has no bound. Fix: one table (install-to-wizard-done, wizard-to-first-run) cited by all.
- **M2. Estimate before the wizard test is not an AC.** key-first-run.html shows "about ₹1-2 in total" but 5.12 verify only checks spend after. Fix: AC that the estimate shows before Continue on step 3 and the final figure is within the shown range.
- **M3. Accessibility and dark mode land late and unevenly.** Only 5.4, 5.13, 5.15 (and E7.10, E10.8) state axe; 5.5, 5.6, 5.7, 5.9, 5.12 and runs-UI 6.1/6.6 verify lines omit axe, dark theme and reduced motion. Contrast of the amber dot is waived by rule only. Fix: put "axe clean in light and dark, keyboard-only" in the Definition of Done in each UI epic file (or the shared ticket template) instead of leaving it to rule_19 in E10 story 8; add a token-contrast unit test over DESIGN.md pairs.
- **M4. Spine-only surfaces carry safety or first-impression weight without a mock.** Updates consent (11.2), Settings › Search (9.4), Settings › Tools & MCP, Schedule picker (6.5), Workflows list, Runs list, Goal editor, About (9.11). Validation report asked the owner to confirm at the first UI epic; no ticket records that decision. Fix: add a pre-story gate "owner confirms mock-or-spine" to each; at minimum mock Updates consent and the Schedule picker.
- **M5. Degraded states lack story ACs.** "No internet" pill, "Local model offline" (with fallback cost wording), "Account key revoked", "Update available" and the 3 s slow-query state appear in EXPERIENCE but in no verify line (5.13 covers only skeletons, empty, reconnect). Fix: add them to 5.13 and 5.6/5.5 verify, with the exact sentences.

## Low

- **L1. Mock/spec drift in the wizard.** key-first-run.html says "6 seats incl. you as CEO, Vision Creator and Reviewer" while key-org.html showed a different seat count with the owner in two hats; seat counts will fail design QA. Fix: state the canonical count once in the template fixture and both mocks.
- **L2. Mock accent assumed blue.** Open owner decision (validation report); 5.15 and E10.9 sign-off will bake it in. Fix: owner picks before 5.1 locks tokens.
- **L3. Org mock has the owner's Vision Creator card floating beside the CEO** with an "advises" line to the team; fine, but the collapsed view then has no visible hat summary for CEO and Reviewer. Fix: confirm 5.8's "one card per hat" is tested in collapsed mode.

## What is solid
Accounts masked/leak-scanned (5.5), picker with warnings that never block (5.4), meter and Stop everything with exact thresholds (5.6), estimate honesty and SM-6 band (5.7), org collapsed/expanded with keyboard and ARIA (5.2), Inbox with named-outcome buttons and second confirmation, and design-QA stories in E5, E6, E7, E8 and E10 each name mock sections.
