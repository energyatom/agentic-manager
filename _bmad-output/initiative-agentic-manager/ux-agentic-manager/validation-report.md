# Validation Report — Agentic Manager

- **DESIGN.md:** `/home/claude/am/_bmad-output/initiative-agentic-manager/ux-agentic-manager/DESIGN.md`
- **EXPERIENCE.md:** `/home/claude/am/_bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md`
- **Run at:** 2026-11-18T09:00:00+05:30

## Overall verdict

The spine pair is ready for architecture and story work. All three PRD journeys (UJ-1 to UJ-3) are Key Flows with a named protagonist, a climax and a failure path. Every DESIGN.md token reference resolves, and colour pairs are contrast-checked in both themes. All thirteen mockups (nine at the time of this report; thirteen since 2026-11-18) are linked from the sections they illustrate. Gaps found by the mechanical pass were fixed in this run (see Resolved). What remains: one owner decision (accent hue), a frontmatter shape note, and surfaces that will be built from the spine alone.

The accessibility lens found one medium issue (single-key shortcuts), fixed in this run. The rest are low and already handled by the spine.

## Category verdicts

- Flow coverage — strong
- Token completeness — strong
- Component coverage — adequate
- State coverage — adequate
- Visual reference coverage — strong
- Bloat & overspecification — adequate
- Inheritance discipline — adequate
- Shape fit — strong

## Findings by severity

### Critical (0)

### High (0)

### Medium (2)

**[Inheritance discipline]** — Accent hue is an assumption (§ DESIGN.md › Colors)
The owner asked for one accent colour but did not name it. A native blue was assumed; creative tools (theme comparison) were off in headless mode.
Fix: Owner picks the accent before E5 mockups are locked; only the accent* tokens change.

**[Accessibility review]** — Single-key shortcuts (§ EXPERIENCE.md › Interaction Primitives)
N, A, E, R, ? and Space could fire while typing or with speech input (WCAG 2.1.4).
Fix: Resolved in this run: they act only with focus on their surface and can be turned off in Settings.

### Low (10)

**[Token completeness]** — Frontmatter adds `motion` and `elevation` groups beyond the spec's five token groups (§ DESIGN.md frontmatter)
Strict DESIGN.md resolvers that read only colors, typography, rounded, spacing and components will ignore these groups; EXPERIENCE.md references `{motion.*}`.
Fix: Keep them (they are referenced). If a resolver rejects unknown groups, move them under `components.motion` and `components.elevation` and update the references.

**[Component coverage]** — Composite components have no dedicated tokens (§ DESIGN.md › Components › Composites)
Team sheet, Schedule picker, MCP server add and similar inherit primitives; visual fidelity depends on builders following the primitives.
Fix: Accept for v1; add tokens only if design QA at a milestone demo finds drift.

**[State coverage]** — Dashboards and Tables have no explicit very-large-data state (§ EXPERIENCE.md › State Patterns)
Grid virtualises above 200 rows, but there is no stated behaviour for 500+ rows a year in dashboard queries (slow query).
Fix: Add a 'Query is taking longer than 3 s' state with Cancel when E8 is refined.

**[Visual reference coverage]** — Spine-only surfaces (§ EXPERIENCE.md › Requirement Coverage)
Team sheet, Runs list, Workflows list, Settings subpages, Schedule picker, MCP add, Goal editor and Role template menu have no mock. Headless run, so the owner could not be asked whether any need one.
Fix: Owner to confirm at the first UI epic whether the Team sheet and MCP add dialog need mocks; both are safety-relevant.

**[Bloat & overspecification]** — Cost & Estimate Display and Safety Surfaces partly repeat Component Patterns (§ EXPERIENCE.md)
Both are indexes across surfaces for two of the owner's five guides.
Fix: PRESERVE: they are the checklists design QA will run against; rows point back to Component Patterns rather than restating rules.

**[Inheritance discipline]** — Mocks use an illustrative second team (§ mockups/key-org.html)
'Product Development' is not in any v1 template; it is there to show grant lines.
Fix: Keep; it is captioned as illustrative.

**[Inheritance discipline]** — Picker warning example differs between mock and UJ-2 (§ mockups/key-seat-sheet.html §2 vs EXPERIENCE.md UJ-2)
The mock and UJ-2 name the same seat (D1-E2). Both follow the same rule.
Fix: None needed; spine wins.

**[Accessibility review]** — Amber status dot is below 3:1 on white (§ DESIGN.md › colors.warn-dot)
#E59A00 on #FFFFFF is about 2.2:1. Every status also has a word, so the dot is not the only signal (1.4.11 is satisfied by the text).
Fix: Keep the rule that every dot sits beside a status word; do not ship dot-only legends.

**[Accessibility review]** — Untrusted hatch under Increased Contrast (§ DESIGN.md › untrusted-well)
The hatch may add visual noise for low-vision users.
Fix: Already specified: Increased Contrast removes the hatch and keeps the edge and label.

**[Accessibility review]** — Toast timing (§ EXPERIENCE.md › Undo toast)
A 6 s toast could be missed (2.2.1).
Fix: Satisfied: ⌘Z and History provide the same undo without time limit.

## Resolved during Finalize

- EXPERIENCE.md had no `{token}` references to DESIGN.md: added 16 (motion, meter, untrusted well, sheet widths, focus ring, targets).
- Component names differed between spines (Seat card (human)/Human seat card, Inbox row/Inbox item, Grid/Table grid, Second confirmation/Privilege-widening confirmation): aligned.
- Org chart and Assistant panel had no DESIGN.md visual spec: added.
- Team sheet, Runs list, Workflows list, Goal editor and Role template menu had no behaviour rows: added.
- No 'No internet' state: added.
- Org density at 30 seats (RULE-18) unspecified: wrap rule and Fit zoom added.
- Flow 5 and Flow 6 lacked failure paths: added.
- Single-key shortcuts vs WCAG 2.1.4 (accessibility lens): scoped to focused surfaces and made switchable.

## Reviewer files
- `review-rubric.md`
- `review-accessibility.md`
