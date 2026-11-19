# Reconcile — imports/crewai-studio-canvas-current.png

The owner's earlier CrewAI Studio canvas for a crew of agents. It is used as an anti-reference. The screenshot holds the owner's private names, so it is git-ignored (`*.png`) and stays on the owner's machine; this table is the public record of what it showed (2026-11-19).

| Observed in the import | Kept, changed or dropped | Where in the spines |
|---|---|---|
| Agent cards: name, role, model line with provider glyph | **Kept**, as the Seat card: avatar, name, role, occupant/model line with provider glyph | DESIGN › Components › Seat card; EXPERIENCE › Component Patterns › Seat card |
| Tools listed on each agent card; "Drop tools here" target | **Changed**: tools move to the Seat sheet (drag target plus the A key); the canvas card holds no tools | EXPERIENCE › Tool chip / Tools library |
| Agents in a top row, tasks in a bottom row, curved wires between them | **Dropped**: a top-down org tree (who) plus a separate vertical step list (what, in order) | EXPERIENCE › Inspiration & Anti-patterns; Workflow editor |
| Truncated task instructions on canvas cards | **Changed**: the step row shows a title and seat; the full instruction is in the step editor | EXPERIENCE › Workflow editor |
| "Completed" badges per task | **Kept** as step status (done, running, waiting, failed, queued), plus per-step time and cost | EXPERIENCE › Run view |
| Per-task re-run and run icons | **Kept** as Re-run step / Re-run from step | EXPERIENCE › Run view |
| "Triggers: No triggers configured" side card | **Changed**: the schedule is stated in words at the top of every workflow, with catch-up policy | EXPERIENCE › Schedule picker |
| Edit pencils on every card | **Changed**: Return or double-click opens the sheet; no per-card edit chrome | EXPERIENCE › Interaction Primitives |
| Zoom +/− and fit controls | **Kept**, as a small corner control plus ⌘0/⌘+/⌘− | EXPERIENCE › Org chart |
| No cost or budget anywhere | **Added**: meter, per-seat spend, estimate, estimate vs actual | EXPERIENCE › Cost & Estimate Display |

No qualitative ideas from the import were dropped without a replacement.
