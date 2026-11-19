# Agentic Manager — Start here

This folder holds the complete plan for **Agentic Manager**, an open-source (MIT), local-first app for running a company of AI agents. Departments are drawn as an org chart, and you can take seats in it yourself as CEO, Vision Creator, Reviewer or Developer. Departments hand work to each other, fan it out to several agents in parallel, consolidate it, review each other's output, and every result that reaches you shows its lineage: who contributed what, from which inputs, with which model and at what cost. Each seat has its own model (local or paid) and its own budget, and a live cost meter and an estimate before each run show what you're spending.

The plan was produced by the BMad team in party mode against your five guides: **user friendly · aesthetic · trustworthy · safe/secure · comprehensive**.

## What's in the folder

| Read in this order | File | What it is |
|---|---|---|
| 1 | `_bmad-output/initiative-agentic-manager/reference-company-spec-2026-11-18.md` | Your direction of 2026-11-18 (binding): cross-department work in v1, department-neutral tests, and the abstract reference company. |
| 2 | `_bmad-output/initiative-agentic-manager/party-decisions/decisions.md` | The party's binding decisions (C1–C6, D1–D13), plus dated amendments. |
| 3 | `_bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md` | The PRD: vision, journeys, 64 capabilities (FR-56–FR-60: cross-department hand-off, fan-out and fan-in, cross-department review gate, advice lineage, workflow triggers workflow; FR-61–FR-64: built-in Local search, built-in capabilities, per-agent capability switches, swapping a built-in for an Account), rules including RULE-20 Open by default, MVP scope, metrics. |
| 4 | `_bmad-output/initiative-agentic-manager/ux-agentic-manager/` | `DESIGN.md` (design system), `EXPERIENCE.md` (behaviour), **`mockups/`** (13 offline HTML screens; open them in a browser). |
| 5 | `_bmad-output/initiative-agentic-manager/initiative-agentic-manager.md` | **The spec Claude Code builds from**: 84 requirements (R83–R84, added 2026-11-19 from your folder-access direction: an agent links folders, read-only unless you switch write on, and its model reads the files natively, falls back to plain text for text files if you allow it, or says plainly that it cannot; and one workflow step can fan a folder out by file type to several agents with different models, gathered by one; R78–R82, added 2026-11-18 for Open by default: everything works with no key, web search through a Local search (SearXNG) bundled in the install, reading, crawling and folder access built in, a Capabilities section on every Agent card with one switch per capability, and an optional Account per capability instead of the built-in; R73–R77, added 2026-11-18 by your replan, bring cross-department hand-offs, parallel fan-out and fan-in, review gates from another department, lineage and workflow-triggers-workflow into v1; R58–R63, added 2026-11-18, make each agent a full Agent card: identity and backstory, model, skills, tools and connectors, knowledge and inbox with links, tasks; R64–R71, added the same day, make connectors generic building blocks: web search, HTTP reader, crawler that can render pages built by scripts in a browser already on your computer, plus an optional crawl service you run yourself; R72 added the same day; API and feed, a local repository that cleans itself after each load, searchable knowledge), constraints, decisions, and the open questions only you can answer; your answers of 2026-11-19 are recorded as Decisions: concrete spike inputs, criterion 8 deferred, PowerPoint (pptx) supported, and OCR swappable to a Local OCR service such as PaddleOCR; the last two are superseded by your folder-access direction of 2026-11-19 (formats are what each agent's model reads; OCR is a vision or OCR model you assign). Beside it: `glossary.md`, `reference-workload.md` (**Company A**, the abstract reference company every test and the optional sample use: you as Owner, Department 1 and Department 2, flows F1, F2 and F3, F3 being many models reading one folder), `trust-gate.md`, `stack-spike.md`. |
| 6 | `_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md` | Architecture: Rust core, embedded React UI, Tauri, 47 decisions (amended 2026-11-19 by your answers: spike criteria 1, 4, 5 and 8, the pptx reader in AD-30 and the Local OCR service in AD-46, both retired later that day by your folder-access direction (AD-30 now: linked folders and file records; AD-31: which file types each model reads; AD-40: fan-out over a folder); and by final review 2: taint through AI-written text, one privilege-widening classifier, App Sandbox for the parser and Local search; AD-47: v1 ships on macOS Apple Silicon only, with the core kept portable; AD-45: the bundled, supervised Local search; AD-46: capabilities as the friendly face of tool permissions, provider swap; AD-38: skills are reviewed instructions that never widen a seat; AD-39: connectors and Sources; AD-40–AD-44: the run graph with fan-out and fan-in, the cross-department hand-off contract, review gates, lineage from the journal, and trigger-chain loop protection), security boundaries, versions checked against crates.io and npm. |
| 7 | `_bmad-output/initiative-agentic-manager/tickets/` | **12 epics and 171 planned stories** (174 entries; story 11.4, signed Windows and Linux builds, dropped 2026-11-19; epic 12 stories 20 and 21 (bundled OCR and a Local OCR service) dropped and stories 2.9, 12.22 and 12.23 (models take files; folder access; many models on one folder) added 2026-11-19 from your folder-access direction; epic 4 stories 16–18 added 2026-11-19 by final review 2; epic 12 story 21, a Local OCR service such as PaddleOCR as the OCR provider, added 2026-11-19 from your answers and dropped the same day), each with failing-first acceptance criteria. **Table order in each `tickets.toml` is the build order, not the id**: epic 12 (Connectors and Sources) is listed after epic 11 but builds between epics 8 and 9, and epic 4's new stories 16–18 sit where they build. |
| — | `AGENTS.md` + `CLAUDE.md` | The rulebook Claude Code loads automatically: build policy, security invariants, coding conventions, build roles. |
| — | `_bmad-output/initiative-agentic-manager/final-review-2/` | The second final review (consistency, security, UX) and `fixes-applied.md`, mapping every finding to its change (2026-11-19). The first round in `final-review/` is historical. |
| — | `_archive/` | Superseded planning inputs (your original brief, the party position files). **Stays local**: `.gitignore` excludes it (and rendered PNGs), so it never enters the public repository; coding agents never read it. |

## Release path

**Platform:** v1 (v0.1 through v1.0) is **macOS on Apple Silicon only**: the Mac app and browser mode served by it on your Mac. Windows 11 and Linux builds, installers and signing come in a later release (v1.x); the code stays portable, and CI builds and unit-tests the core on Linux to keep it so (your decision, 2026-11-19).


- **v0.1** (end of epic 4): run your own departments from the terminal, alongside Paperclip; the golden test runs Company A's cross-department flow.
- **v0.2** (end of epic 6): web UI with Org view, Runs and Inbox.
- **End of epic 9:** a frozen build, and the official 4-week dogfood clock starts: your own departments, with at least one cross-department flow.
- **v0.9-rc** (epic 11): the signed, notarised Mac installer, with the trust gate green. Epic 11 story 3 opens with the spike's deferred criterion 8 (a signed, notarised build on a clean Mac), which needs your Apple Developer account by then.
- **v1.0 public:** 4 clean weeks, the last 2 of them on v0.9-rc.

## Before you start: what you provide for the spike

No decision blocks the start; these **inputs** are needed during the spike (epic 1 story 1). Have them ready, or tell Claude Code which are pending (your answers of 2026-11-19 are folded in):

- [ ] **An empty GitHub repository** with push access (you provide it; later, branch protection for story 1.2).
- [ ] **An Anthropic API key** (criterion 1: proves a frontier model works end to end, streaming and tool calls). An OpenAI key, or any other OpenAI-compatible cloud key, is an accepted alternative.
- [ ] **LM Studio and/or Ollama** on this Mac with **one tool-capable model** loaded (criteria 1 and 9).
- [ ] **Node.js and uv installed**, nothing else, for criterion 5 (MCP). Claude Code launches two official MCP reference servers itself during the spike; they are free and need no account:
  - stdio: `npx -y @modelcontextprotocol/server-filesystem@2.4.0 --read-only <throwaway test folder>` (a temporary folder the spike creates with one test file; alternative: `uvx mcp-server-fetch@2026.11.18`)
  - Streamable HTTP: `npx -y @modelcontextprotocol/server-everything@2.3.1 streamableHttp`, reached at `http://127.0.0.1:3001/mcp`
- [ ] **A folder of a few of your files, of mixed types,** outside the repo: for example a PDF, a scanned PDF, an image, a spreadsheet, a Word or PowerPoint file and a csv. Set its path in the environment variable **`AM_SPIKE_DOCS_DIR`**. Criterion 4 ("Files to models") links it read-only and hands each file to a frontier model (your Anthropic or OpenAI key) and to a local model that can see images, and records which types each model reads natively and which it refuses; nothing is converted. CI skips this test and uses synthetic files.
- [ ] **One vision-capable model loaded in LM Studio or Ollama** (for criterion 4's local half; any model that accepts images).
- [ ] **The sleep/wake demo**: close the lid across a scheduled fire time and reopen (criterion 3).
- **Later, not for the spike:** your **Apple Developer ID and notarisation credentials** (an app-specific password or an App Store Connect API key) and **a clean Mac** (or a fresh macOS user account). Criterion 8 (a signed, notarised build on a clean Mac) is **deferred** until your account exists: the spike keeps an ad-hoc-signed build that launches as stand-in evidence, criterion 8 does not count towards the spike's 2-failure replan threshold, and the signed, notarised demo becomes the gate that opens epic 11 story 3, before v0.9-rc.

The replan threshold counts criteria 1–7 and 9. Only criterion 5 may be recorded *pending owner input*, and only while Node.js or uv is missing; a pending criterion is not a fail. Before story 1.2, "green" means the spike's minimal `just check` (fmt, clippy, nextest), which story 1.2 hardens into the full gate.

**Optional, any time after epic 12 story 22:** to read scanned pages and images on this Mac, load a vision or OCR model in LM Studio or Ollama and choose it as that agent's model; the agent's Folder access shows which file types its model reads. Files given to a cloud model leave the Mac, and the app asks twice before linking a folder to such an agent.

## Your open questions

They're listed in the initiative file under *Open questions*. None blocks the start: the pre-build advice test (OQ-3) was dropped on 2026-11-19, because the app is a generic agent manager, what departments produce is your choice, and your own use of Paperclip and CrewAI already establishes the need.

Also settled on 2026-11-18: no paid crawl account is needed. Pages built by scripts are read by rendering them in Chrome, Chromium or Edge already installed on your computer (with a fresh, empty profile); a crawl service is optional and, if ever needed, one you run yourself. A Team workspace (multi-user, access scoped by group) and approve-then-send come later, in a v1.1 Team release. Add-ons (one-click install and supervision of open-source services such as a self-hosted crawler or more MCP servers) come in v1.x; Local search is the one built-in service in v1.

Settled on 2026-11-19: **Mac first**. The Windows questions (code-signing route; Local search on Windows) wait for the Windows release in v1.x.

Also settled on 2026-11-18: **open by default**. Web search, reading and crawling pages, and documents and knowledge are built into the install and need no account or key; web search runs through a Local search (SearXNG) the app installs and looks after itself. On each agent you switch each capability on or off and can swap a built-in for an outside Account if you want one. The role templates are Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist and Human seat, and "Ask a seat" sends a question to any agent from the Inbox.

These can wait until the story that needs them:

| Question | Needed before |
|---|---|
| Default caps (placeholders in the sample: ₹150 per scheduled run, ₹250 per manual run, ₹600 per week; your currency and timezone default from your Mac's settings) | v0.1 (story 4.13) |
| Recommended local model per role | story 5.4 |
| Baseline numbers: your weekly maintenance time and gate approval rate | the dogfood clock (story 9.13) |
| Which keyed search provider, if any, to recommend as an optional upgrade over Local search | after the comparison in story 12.4 |
| Split epic 12 (20 stories) into two epics? (Sofia's pick: keep one) | before epic 12 starts |
| Data-protection duties | v0.9-rc |
| Product name (Staffroom, Seatwork, Deskwork, Orgline, Cadre) | story 11.12 |

## How to start in Claude Code

```bash
cd "<this folder>"
git init && git add -A && git commit -m "Planning set: Agentic Manager"   # BMad commits per story; it needs git
claude
```

Then, in Claude Code:

1. `bmad status`: confirms the BMad install, and `bmad` shows the active initiative.
2. `Sofia, what's ready?`: should report **epic 1, story 1 (the Rust stack spike)** as the only ready story.
3. `bmad-build epic 1 story 1`: you take part in this one (see *What you provide for the spike*). It is a 2-week spike against 9 checks. Check 8 (signed, notarised build) is deferred until your Apple Developer account exists and is not counted; if 2 or more of the other 8 fail (a *pending owner input* check is not a fail), Sofia replans around a hybrid stack before anything else is built.
4. After the spike passes: `bmad-build-auto` builds stories that have `hitl = false` and whose dependencies are built. It stays inside one milestone and stops at any story that needs you, any milestone demo, and any replan.
5. At each milestone: `bmad-walkthrough` for your 10-minute demo, then `bmad-retrospective`, then Sofia closes the epic.

**Who does what during the build:** the main Claude Code session is the manager (`bmad-build-auto` working from Sofia's board). Amelia (`bmad-agent-dev` / `bmad-build`) codes. Testing comes from `bmad-qa-generate-e2e-tests` plus each ticket's failing-first criteria. `bmad-code-review` reviews.

**Rules that keep autonomous building on track** (all in `AGENTS.md`):
- the ticket is the spec;
- tests are written first;
- `just check` must be green before a story counts as done (before story 1.2, the spike's minimal `just check`);
- one commit per story;
- planning documents change only through Sofia's replan;
- UI must match `DESIGN.md` and the mockups;
- the security invariants are enforced as tests.

## If something changes

Tell Sofia ("this changed, what now"). She replans: she updates the tickets and records the decision, and only the affected work is redone. To re-run a debate, call `bmad-party-mode`. The room remembers this session.
