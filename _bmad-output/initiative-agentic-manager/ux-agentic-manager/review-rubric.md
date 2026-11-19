# Spine Pair Review — Agentic Manager

## Overall verdict

The spine pair is ready for architecture and story work. All three PRD journeys (UJ-1 to UJ-3) are Key Flows with a named protagonist, a climax and a failure path. Every DESIGN.md token reference resolves, and colour pairs are contrast-checked in both themes. All thirteen mockups (nine at the time of this rubric, 2026-11-17; thirteen since 2026-11-18) are linked from the sections they illustrate. Gaps found by the mechanical pass were fixed in this run (see Resolved). What remains: one owner decision (accent hue), a frontmatter shape note, and surfaces that will be built from the spine alone.

## 1. Flow coverage — strong

UJ-1, UJ-2 and UJ-3 are present under their PRD names, each with numbered steps, a climax and a failure path. Three added flows (Stop/budget, new seat, xlsx round-trip) also carry failure paths.

## 2. Token completeness — strong

36 references in DESIGN.md and 16 in EXPERIENCE.md all resolve. Every colour has a hex value and a -dark twin. Contrast is stated for load-bearing pairs.

### Findings

- **low** Frontmatter adds `motion` and `elevation` groups beyond the spec's five token groups. Strict DESIGN.md resolvers that read only colors, typography, rounded, spacing and components will ignore these groups; EXPERIENCE.md references `{motion.*}`. (DESIGN.md frontmatter). *Fix:* Keep them (they are referenced). If a resolver rejects unknown groups, move them under `components.motion` and `components.elevation` and update the references.

## 3. Component coverage — adequate

Every component named in EXPERIENCE.md has a behaviour row and either a DESIGN.md visual spec or an explicit place in the Composites paragraph.

### Findings

- **low** Composite components have no dedicated tokens. Team sheet, Schedule picker, MCP server add and similar inherit primitives; visual fidelity depends on builders following the primitives. (DESIGN.md › Components › Composites). *Fix:* Accept for v1; add tokens only if design QA at a milestone demo finds drift.

## 4. State coverage — adequate

Every IA surface has empty, loading and error treatments where they apply. Budget 80%/100%, Stop everything, context overflow, local offline, no internet, outside edit, invalid config, crash resume and server-unreachable are covered. Permission-denied does not apply (single user).

### Findings

- **low** Dashboards and Tables have no explicit very-large-data state. Grid virtualises above 200 rows, but there is no stated behaviour for 500+ rows a year in dashboard queries (slow query). (EXPERIENCE.md › State Patterns). *Fix:* Add a 'Query is taking longer than 3 s' state with Cancel when E8 is refined.

## 5. Visual reference coverage — strong

All nine files in mockups/ and the one import are linked inline, each naming what it illustrates. Spines-win-on-conflict is stated in Foundation and DESIGN.md Brand & Style.

### Findings

- **low** Spine-only surfaces. Team sheet, Runs list, Workflows list, Settings subpages, Schedule picker, MCP add, Goal editor and Role template menu have no mock. Headless run, so the owner could not be asked whether any need one. (EXPERIENCE.md › Requirement Coverage). *Fix:* Owner to confirm at the first UI epic whether the Team sheet and MCP add dialog need mocks; both are safety-relevant.

## 6. Bloat & overspecification — adequate

EXPERIENCE.md is long (about 8,200 words) but tabular, and aimed at coding agents. No pixel specs where tokens cover it.

### Findings

- **low** Cost & Estimate Display and Safety Surfaces partly repeat Component Patterns. Both are indexes across surfaces for two of the owner's five guides. (EXPERIENCE.md). *Fix:* PRESERVE: they are the checklists design QA will run against; rows point back to Component Patterns rather than restating rules.

## 7. Inheritance discipline — adequate

Sources resolve. Journey names are verbatim. Glossary terms (seat, hat, team, workflow, run, table, grant, Account, estimate, meter, proposal, guidance note, memory note, drop folder, Doctor) are used as the PRD defines them. Inbox kinds are mapped to PRD glossary terms.

### Findings

- **medium** Accent hue is an assumption. The owner asked for one accent colour but did not name it. A native blue was assumed; creative tools (theme comparison) were off in headless mode. (DESIGN.md › Colors). *Fix:* Owner picks the accent before E5 mockups are locked; only the accent* tokens change.
- **low** Mocks use an illustrative second team. 'Product Development' is not in any v1 template; it is there to show grant lines. (mockups/key-org.html). *Fix:* Keep; it is captioned as illustrative.
- **low** Picker warning example differs between mock and UJ-2. The mock and UJ-2 name the same seat (D1-E2). Both follow the same rule. (mockups/key-seat-sheet.html §2 vs EXPERIENCE.md UJ-2). *Fix:* None needed; spine wins.

## 8. Shape fit — strong

DESIGN.md sections are in canonical order. EXPERIENCE.md has all required defaults, plus Responsive & Platform (multi-platform), Inspiration & Anti-patterns (import and reference products), and three invented sections (Cost & Estimate Display, Safety Surfaces, Requirement Coverage), each tied to an owner guide.

## Resolved during Finalize

- EXPERIENCE.md had no `{token}` references to DESIGN.md: added 16 (motion, meter, untrusted well, sheet widths, focus ring, targets).
- Component names differed between spines (Seat card (human)/Human seat card, Inbox row/Inbox item, Grid/Table grid, Second confirmation/Privilege-widening confirmation): aligned.
- Org chart and Assistant panel had no DESIGN.md visual spec: added.
- Team sheet, Runs list, Workflows list, Goal editor and Role template menu had no behaviour rows: added.
- No 'No internet' state: added.
- Org density at 30 seats (RULE-18) unspecified: wrap rule and Fit zoom added.
- Flow 5 and Flow 6 lacked failure paths: added.
- Single-key shortcuts vs WCAG 2.1.4 (accessibility lens): scoped to focused surfaces and made switchable.

## Mechanical notes

- 36 token references in DESIGN.md and 16 in EXPERIENCE.md resolve against the frontmatter (checked by script).
- 9 of 9 mockups and 1 of 1 import are linked; no orphans; no missing link targets.
- Mockups contain no network URLs, no scripts, and system fonts only.
- Mermaid: none used.
