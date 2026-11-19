---
title: Reference company — binding input for the 2026-11-18 replan
owner: the owner (product owner)
status: binding
supersedes: every earlier reference workload, department template, fixture, example and sample content
note: wording neutralised 2026-11-18 so the live plan names no removed workload or domain sample; the verbatim owner text is archived
---

# Owner direction (2026-11-18, in his words, gist)

- "We want a simple to use Paperclip-style app that has all the packages built in and can be self hosted, has the right tools for integrating central APIs for local and frontier models, looks aesthetic (Apple-like) and has no distractions."
- The earlier domain-specific reference department must be **removed fully from any mention** so the build does not deviate from that goal. It is not a template, not a sample, not a fixture, not an example, not the acceptance workload.
- Data cleaning, normalisation, enrichment, spreadsheet tolerance and similar are **jobs of the agents the user configures** (their skills, instructions, tools). The engine adds nothing domain-specific; the user decides what their agents do.
- **Cross-department hand-offs and cross-department review come into v1.**
- **Acceptance tests and automated tests become department-neutral.**

# The owner's example (the reference company; abstract names per Amendment 2)

A small company (sample name **Company A**; the owner runs the owner's own company the same way) has an Owner (human, "You"; CEO, Vision Creator, Reviewer and Developer hats, restated 2026-11-19 by owner delegation, final review 2) and two departments, D1 (D1-Head, D1-E1, D1-E2, D1-E3) and D2 (D2-Head, D2-E1 to D2-E4). Its flows F1 (cross-department fan-out, fan-in, review, lineage) and F2 (workflow triggers workflow, loop protection) are defined in Amendment 2 below. Some seats run local models and some frontier models, showing per-seat routing and cost.

# Engine primitives this requires (generic, not domain-specific)

1. **Cross-department hand-off**: a step's output (document or table rows) becomes the input of a step or workflow owned by another department, within grants; taint and provenance travel with it.
2. **Parallel fan-out / fan-in** steps (brings the deferred "parallel branches" into v1): one output to N seats in parallel; a consolidating step waits for all (or a configured quorum/timeout) and receives every branch's output labelled by seat.
3. **Cross-department review gate**: a seat (AI or human) in another department can be a review gate on a hand-off (approve / return with note / reject), recorded with actor and hat.
4. **Advice lineage**: every document/answer reaching a human shows its contribution chain (seats, steps, models, inputs, costs) as a readable trace, drill-down from the Inbox and Documents; exportable.
5. **Workflow-triggers-workflow** across departments (event: "document X updated" or "step Y completed" starts workflow Z), with loop protection.

These replace nothing that is already safe: grants, budgets, reader/actor separation, approvals, egress rules all still apply across departments.

# Tests

- The golden end-to-end test and every fixture use **Company A** with synthetic content (no real people, no real companies' data). FakeLlm scripts for each seat.
- A real-model run (local + frontier mix) of flow F1 is the owner-run acceptance demo.
- Dogfood acceptance: the owner runs **the owner's own** company's departments (any the owner chooses) with at least one cross-department flow, for the clean-weeks period. No department type is prescribed.
- Templates shipped in v1: **Blank department** and the generic role templates (e.g. Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist; see Amendment 3) — plus, optionally, the Company A sample company as "Sample company: Company A (two departments)" the wizard can load to explore, clearly labelled as a sample. No domain department templates in v1.

# AMENDMENT 2 (owner, 2026-11-18 23:04) — supersedes the earlier sample names

The owner does not want any domain-named reference company or department: domain names bias AI builders and add domain constraints. Use **abstract, neutral names only**:

| Unit | Seats |
|---|---|
| Owner | **Owner** — human ("You"); CEO, Vision Creator, Reviewer and Developer hats (restated 2026-11-19) |
| Department 1 (D1) | **D1-Head**, **D1-E1**, **D1-E2**, **D1-E3** |
| Department 2 (D2) | **D2-Head**, **D2-E1**, **D2-E2**, **D2-E3**, **D2-E4** |

Sample company name: **Company A**. Sample documents/tables: "Document A", "Table T1", "Report R1" — no domain words.

Reference flows (abstract):
- **Flow F1 (cross-department, fan-out/fan-in, review, lineage):** D2-Head produces Document A → handed across to D1-E1, D1-E2, D1-E3 in parallel (fan-out) → D1-Head consolidates (fan-in; quorum/timeout) → handed back across to D2-Head (cross-department hand-off; D1-Head can act as a cross-department review gate on D2's output) → D2-Head's Report R1 reaches the Owner in the Inbox with full **lineage** (Owner ← D2-Head ← D1-Head ← D1-E1/E2/E3 ← D2-Head/D2-E*).
- **Flow F2 (workflow triggers workflow, loop protection):** D2-E1…E4 update Table T1 weekly → triggers D1's workflow; D1's result can trigger D2's workflow again, with loop protection.
- Model mix in samples: some seats local, some frontier.

Patterns may reference the open-source CrewAI and Paperclip projects (crews/flows, hierarchical process, org-chart companies with heads reporting up) for behaviour, never for domain content.

Everything else in this spec (engine primitives, tests, templates) stands, with the abstract names above. The optional sample template is "Sample company: Company A (two departments)".

# AMENDMENT 3 (reconcile, 2026-11-18) — role-template names

Two parallel planning sessions used different template names. One generic set ships: **Head, Coordinator, Analyst, Researcher, Reviewer, Writer, Specialist, Human seat**. "Chief of Staff" and "Manager" are not template names. The learning loop and the ad-hoc question feature ship with the **Coordinator** template; the ad-hoc feature is **"Ask a seat"** (an Inbox thread to any seat the owner picks; by default the top Coordinator seat, else the top head seat). Company A's seat names are unchanged.
