> **Historical — superseded 2026-11-18.** This review predates the owner REPLAN of 2026-11-18 (reference-company spec): the earlier domain reference department it names has been removed from the plan and replaced by the abstract Company A; wording that named that department is neutralised below as "the former reference workload". Read the live initiative, PRD, architecture and tickets instead.

# Final review, guide 5: comprehensive to what we discussed

Reviewer: independent fresh reader. Report only; no planning file was edited.

## Verdict

PASS WITH FIXES. MISSING items: 0. Every owner ask lives in the set as a requirement, decision or explicit deferral. Three asks are only partly met or weakly deferred (portable skills, build-team roles, "no archive"). Claude Code can start from the set, but the starting point is implied rather than stated, and three stale or open contradictions remain.

## Coverage table

| Owner ask | Where it lives | Status |
|---|---|---|
| Open source, free, any platform; local server in browser first, desktop shell later | PRD Vision, Experience and platform, FR-54; D4, D10; initiative R54, Platforms; AGENTS.md | covered (Windows and Linux follow macOS, still inside v1.0 gate) |
| Like CrewAI (easy setup) but runs like Paperclip (org chart, departments, Chief of Staff) | PRD Vision; initiative Description; EXPERIENCE "Lifted from CrewAI Studio"; reference-workload org | covered |
| Pain points: Paperclip buggy and high-maintenance; CrewAI lacks model-cost control | PRD Vision, SM-4, AI behavior failure modes; D2, D3 | covered |
| Different local or paid models per agent; LM Studio; Claude for Chief of Staff | FR-7, FR-8; D3; the former reference workload's seat models | covered |
| Global account per key/provider, assign models to agents; open-source as easy as paid | FR-6, FR-7; D3 ("Local is never advanced") | covered |
| Ease of use first; Apple-like interface | PRD Experience; DESIGN.md; RULE-1; guide order in decisions.md | covered |
| Cost calculator up front, ticking beside the org; estimate for the task | FR-28, FR-29; D2; RULE-7 | covered |
| In-app dashboards (not Airtable), database, final ChatGPT/Claude-style Assistant | FR-43 to FR-45; D6; original input goal 4; AD-34, AD-35 | covered (Airtable is never named; fine) |
| Studio: org chart, expanded and collapsed views | FR-23; EXPERIENCE Org chart row (collapsed by default, Space, shift-cmd-E) | covered |
| Owner places himself at CEO, Developer, Vision Creator etc. | FR-25, FR-30, FR-51; UJ-3; reference-workload (three human seats) | covered |
| Open-ended departments | Blank template FR-26; D7; RULE-2 | covered |
| Software Product Dev, Scientific Optics, Mechanical/CAD departments | D7, PRD MVP scope, reference-workload "Not shipped" | deferred (v1.x templates; reason: need workspace-write and CAD/optics readers) |
| Collaboration between departments | FR-42 grants (D1) in v1 | covered (grants); cross-department hand-offs were later brought into v1 (2026-11-18) |
| Roles and skills portable to other apps | FR-1, FR-4 (plain files, folder or zip); FR-122 SKILL.md export | deferred (v1.x, no release target or reason in PRD; see F3) |
| The former reference workload's steps | its reference-workload steps; FR-19; epic 1 story 5 | covered (since removed) |
| The former reference workload's weekly volume and schedule | its Parameters; UJ-1, UJ-3; trigger 08:00 IST | covered (since removed) |
| Concise outreach email drafts he sends himself | Step 6; RULE-10 (no send tools); FR-74 deferred | covered |
| People outside the app set statuses with reasons; system learns | FR-39, FR-40 (D5); FR-50 learning loop | covered |
| Per-person drop folders; delete after processing; no re-processing, no token burn | FR-19 (content hash, delete default); reference-workload Inputs; RULE-14 | covered (but see F5) |
| Actionable points, trimmed dashboard | FR-36 document format; FR-41 default saved view | covered |
| MIT with rigorous safety and acceptability checks | C1; AD-3; trust-gate.md; RULE-8 to RULE-19; community review window | covered |
| Native Rust | C2, D4, AD-1; stack-spike.md | covered, but conditional on spike (F6) |
| Multi-agent build in Claude Code (coders, testers, manager), autonomous | original input section 16; AGENTS.md policy; AD-33; spike criterion 9 | partial (F2) |
| Five guides | decisions.md mandate; RULE-1 to RULE-19; EXPERIENCE; trust-gate | covered |

## Starting point for Claude Code

Present: CLAUDE.md imports AGENTS.md; AGENTS.md states policy, layout, gates and conventions; tickets.toml gives build order; epic 1 story 1 (spike, `after = []`) is the only unblocked entry.

Missing: nothing says "start here". The unblocked story is `hitl = true` with a long owner-step list (GitHub repo, Apple Developer ID, keys, LM Studio, two MCP servers, 3 real spike documents, sleep/wake demo), and two owner open questions precede it (OQ-3 manual advice note; fixture-vs-local spike documents). Autonomous gating exists only implicitly: `hitl` flags in the TOML, "owner reviews between milestones" in the original input, and a hook that refuses a story without green `just check` (which does not exist until story 1.2). AGENTS.md still carries a TODO for the `just check` line.

## Findings, ranked

**F1 (medium). No explicit kickoff and autonomy-gate statement.**
Fix: add a short "Start here" block to AGENTS.md (outside or beside the managed markers so refresh keeps it): (1) owner clears OQ-3 and the real document question; (2) run `bmad-build` (not `-auto`) on epic 1 story 1 with the owner doing the listed steps; (3) `bmad-build-auto` allowed only for `hitl = false` stories whose `after` are all built, within one milestone, stopping at each milestone demo; (4) a story with `hitl = true` or a spike REPLAN stops autonomous mode. Also state that until story 1.2 lands, "green" means the spike workspace's `cargo test`.

**F2 (medium). Build-team shape (coders, testers, manager) is not specified.**
The owner asked for a multi-agent build with coders, testers and a manager. Only the PO REPLAN path, `bmad-build`/`bmad-build-auto`, and original input section 16 (review, QA and retrospective skills, model tiering) exist; nothing in AGENTS.md or the initiative names the roles, the tester-writes-tests-first split, or the manager that dispatches stories and enforces milestone stops.
Fix: add a 10-line "Build team" section to AGENTS.md or the initiative Notes: manager (dispatch, board, REPLAN escalation), coder (implement one story), tester (acceptance tests from the ticket, failing first), reviewer (`bmad-code-review`); model tiering from original input section 16.

**F3 (low-medium). Portability of roles and skills deferred without target or reason.**
FR-122 (SKILL.md export) is "v1.x" only; the owner wanted roles and skills portable to other apps. FR-1/FR-4 give folder portability but only for this app.
Fix: in PRD/initiative Boundaries, state the v1.x target (for example v1.1) and the reason (needs a stable role schema first), and note that v1 role files are plain Markdown with front-matter so export stays cheap.

**F4 (medium). Stale dogfood-clock statements.**
decisions.md D10 and the PRD (rollout diagram "v0.2: dogfood may start", Decisions line "dogfood from v0.2") still say the clock starts at v0.2. The initiative decision of 2026-11-18 (clock after epic 9, final two weeks on v0.9-rc) and the corrected architecture line supersede them, but the two upstream documents are not annotated. An agent reading the PRD or decisions first gets the wrong release shape.
Fix: add a one-line "superseded by initiative Decision 2026-11-18 (Clean weeks)" note at those three places (through the REPLAN route, since PRD and decisions are locked), or add a precedence line to AGENTS.md: initiative Notes decisions beat PRD and decisions.md.

**F5 (low). "Files deleted, no archive" is looser in the product than the owner said.**
FR-19 and EXPERIENCE offer delete (default), archive or keep.
Fix: record the choice as an explicit owner-approved deviation (default delete matches his ask; archive/keep are opt-in), or confirm it with him at the OQ round.

**F6 (low). "Native Rust" is conditional.**
D4 gates Rust on the spike and allows a hybrid fallback. This is disclosed and sensible, but the owner asked for native Rust without conditions.
Fix: one sentence in the initiative Notes that the owner is informed the Rust choice is spike-gated and a hybrid keeps Rust for every criterion that passes (AD-2).

**F7 (low-medium). Default cap values disagree.**
Initiative Assumption: ₹40 per run and ₹600 per month. Initiative open question OQ-8 and the mockups: ₹150 scheduled, ₹250 manual per run, ₹600 per week. UJ-1 shows ₹96 of ₹600 this week. Epic 1 story 5 writes the template "caps from the parent assumption", so the shipped template inherits the weaker figure.
Fix: pick one set (suggest the mock set, weekly ₹600) as the working default in the Assumption, mark OQ-8 as the owner's override, and keep story 1.5 and story 4.13 consistent.

**F8 (low). Epic-local R-numbers collide with initiative R-numbers.**
In epic 1 tickets, `covers = ["R7"]` means the epic's R7 (CI baseline), while initiative R7 is LM Studio autodetect. A reader or script mapping `covers` to the initiative will mislink.
Fix: prefix epic-local ids (for example E1-R7) or add a note in tickets.toml that `covers` in story files are epic-local.

## Cross-document contradictions checked

- Licence: MIT everywhere; Apache-2.0 appears only as a superseded original-input line and as an allowed dependency licence. Clean.
- Stack: Rust, one binary, React embedded, Tauri later; Python references appear only as superseded. Clean.
- Release shape: stale in PRD and decisions.md (F4); initiative and architecture agree.
- Dry-run: EXPERIENCE row 108 now matches AD-37 (real tables, flagged). Clean.
- Caps: F7.
- Raising a cap is privilege-widening everywhere after the 2026-11-18 decision.
