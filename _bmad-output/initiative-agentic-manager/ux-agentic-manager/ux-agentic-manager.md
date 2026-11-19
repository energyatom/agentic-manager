---
title: Agentic Manager — UX
status: final
updated: 2026-11-19
initiative: initiative-agentic-manager
sources:
  - ../prd-agentic-manager/prd-agentic-manager.md
  - ../party-decisions/decisions.md
  - ../reference-company-spec-2026-11-18.md
---

- [`DESIGN.md`](DESIGN.md) — visual identity: tokens (light + dark), type, layout, components. Owns how it looks.
- [`EXPERIENCE.md`](EXPERIENCE.md) — IA, behaviour, states, keyboard, accessibility, key flows. Owns how it works.

Key-screen mocks: [`mockups/`](mockups/) (thirteen static HTML files, offline). All sample content uses the abstract sample company of the 2026-11-18 replan (Company A: the Owner, Department 1, Department 2). Spines win on conflict.

Added in the 2026-11-18 replan: cross-department hand-off lines, parallel branches with a consolidating step, cross-department review gates, document lineage and "Starts when…" triggers with loop protection (EXPERIENCE.md › Component Patterns; DESIGN.md › Components).

Added in the 2026-11-18 reconcile and Open by default change: role templates Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist and Human seat; Ask a seat to any seat; the Agent card's Capabilities section with a Built-in or Account provider per capability (`mockups/key-seat-sheet.html` §1–2); a first-run wizard that needs no key, with Local search (SearXNG) built in (`mockups/key-first-run.html`); Key Flows UJ-1 to UJ-3 mirroring the PRD.

Added 2026-11-19 (owner folder-access direction): Capabilities' Documents & knowledge becomes **Folder access** — linked folders (read only by default), file-type badges marked by what the seat's model reads, a text fallback switch, "Leaves this Mac" for cloud models, two-step linking (`mockups/key-seat-sheet.html` §1–2); Brain's file chips; What this seat can touch gains What leaves this Mac; **Parallel branches › Over a folder** routes files by pattern to seats with different models into a gathering step, with mismatch warnings and a Not routed list (`mockups/key-workflow-run.html` §5, Flow F3); Doctor's OCR check becomes File reading (`mockups/key-models-doctor-history.html`); the wizard lists Folder access (`mockups/key-first-run.html`). The OCR provider menu drawn earlier that day is retired.
