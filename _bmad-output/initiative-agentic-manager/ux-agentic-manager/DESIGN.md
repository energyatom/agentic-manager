---
name: Agentic Manager
description: Calm, native-feeling desktop and browser UI for running an AI company from an org chart. One accent, the system font, an 8px grid, and cost and safety always in view.
status: final
updated: 2026-11-18
sources:
  - ../reference-company-spec-2026-11-18.md
  - ../prd-agentic-manager/prd-agentic-manager.md
  - ../party-decisions/decisions.md
  - ../party-decisions/sally.md
  - ../../../docs/agentic-manager-brief.md
colors:
  # Light mode. Every token has a -dark twin below.
  bg: '#F5F5F7'
  surface: '#FFFFFF'
  surface-2: '#FAFAFB'
  sidebar: '#ECECEF'
  ink: '#1D1D1F'
  ink-2: '#5E5E64'
  ink-3: '#707075'
  hairline: '#E2E2E7'
  control: '#8E8E93'
  accent: '#0A63E0'
  accent-fill: '#0A63E0'
  on-accent: '#FFFFFF'
  accent-soft: '#E8F0FD'
  ok: '#1A7F45'
  warn: '#8A5200'
  warn-dot: '#E59A00'
  warn-fill: '#FFF3DC'
  err: '#B3261E'
  err-fill: '#FDECEC'
  untrusted: '#FFF8E6'
  untrusted-edge: '#D9B44A'
  scrim: 'rgba(0,0,0,0.18)'
  # Dark mode
  bg-dark: '#161618'
  surface-dark: '#1E1E21'
  surface-2-dark: '#252528'
  sidebar-dark: '#1A1A1C'
  ink-dark: '#F2F2F5'
  ink-2-dark: '#A1A1A8'
  ink-3-dark: '#8A8A91'
  hairline-dark: '#2E2E33'
  control-dark: '#6A6A72'
  accent-dark: '#4C94FF'
  accent-fill-dark: '#2468D6'
  on-accent-dark: '#FFFFFF'
  accent-soft-dark: '#1F2A3D'
  ok-dark: '#4CC27E'
  warn-dark: '#F0B44C'
  warn-dot-dark: '#F0B44C'
  warn-fill-dark: '#3A2C12'
  err-dark: '#FF7B72'
  err-fill-dark: '#3A1D1D'
  untrusted-dark: '#2B2616'
  untrusted-edge-dark: '#8C7330'
  scrim-dark: 'rgba(0,0,0,0.40)'
typography:
  # System stack: SF Pro on macOS (the only v1 platform, 2026-11-19); the stack keeps Segoe UI Variable (Windows) and system-ui (Linux) for v1.x.
  # All figures that show money, counts or durations use tabular numerals.
  display:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI Variable Display", system-ui, sans-serif'
    fontSize: 22px
    fontWeight: '600'
    lineHeight: 28px
  title:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", system-ui, sans-serif'
    fontSize: 17px
    fontWeight: '600'
    lineHeight: 24px
  headline:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", system-ui, sans-serif'
    fontSize: 13px
    fontWeight: '600'
    lineHeight: 20px
  body:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", system-ui, sans-serif'
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 20px
  callout:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", system-ui, sans-serif'
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
  caption:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", system-ui, sans-serif'
    fontSize: 11px
    fontWeight: '500'
    lineHeight: 16px
    letterSpacing: 0.02em
  metric:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI Variable Display", system-ui, sans-serif'
    fontSize: 28px
    fontWeight: '600'
    lineHeight: 32px
    note: 'font-variant-numeric: tabular-nums'
  mono:
    fontFamily: 'ui-monospace, "SF Mono", SFMono-Regular, Menlo, Consolas, monospace'
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 18px
rounded:
  xs: 4px
  sm: 6px
  md: 8px
  lg: 10px
  xl: 12px
  full: 9999px
spacing:
  '1': 4px
  '2': 8px
  '3': 12px
  '4': 16px
  '5': 20px
  '6': 24px
  '8': 32px
  '10': 40px
  sidebar-width: 216px
  toolbar-height: 48px
  sheet-width: 440px
  assistant-width: 420px
  seat-card-width: 192px
  control-height: 28px
  hit-target-min: 24px
motion:
  quick: 150ms
  standard: 180ms
  sheet: 220ms
  easing: 'cubic-bezier(0.2, 0, 0, 1)'
  pulse: '1000ms ease-in-out infinite'
elevation:
  flat: 'none'
  raised: '0 1px 2px rgba(0,0,0,0.06), 0 0 0 0.5px rgba(0,0,0,0.06)'
  floating: '0 8px 28px rgba(0,0,0,0.12), 0 0 0 0.5px rgba(0,0,0,0.08)'
  raised-dark: '0 0 0 0.5px rgba(255,255,255,0.08)'
  floating-dark: '0 12px 32px rgba(0,0,0,0.5), 0 0 0 0.5px rgba(255,255,255,0.10)'
components:
  button-primary:
    background: '{colors.accent-fill}'
    foreground: '{colors.on-accent}'
    radius: '{rounded.sm}'
    height: '{spacing.control-height}'
  button-secondary:
    background: '{colors.surface}'
    foreground: '{colors.ink}'
    border: '0.5px {colors.control}'
    radius: '{rounded.sm}'
  button-stop:
    background: '{colors.surface}'
    foreground: '{colors.err}'
    border: '0.5px {colors.control}'
    radius: '{rounded.sm}'
  button-destructive-confirm:
    background: '{colors.err}'
    foreground: '#FFFFFF'
    radius: '{rounded.sm}'
  cost-meter:
    background: '{colors.surface-2}'
    foreground: '{colors.ink-2}'
    figure: '{colors.ink}'
    ring-rest: '{colors.ink-2}'
    ring-running: '{colors.accent}'
    background-amber: '{colors.warn-fill}'
    foreground-amber: '{colors.warn}'
    background-stopped: '{colors.err-fill}'
    foreground-stopped: '{colors.err}'
    radius: '{rounded.full}'
    height: 24px
  seat-card:
    background: '{colors.surface}'
    radius: '{rounded.lg}'
    shadow: '{elevation.raised}'
    width: '{spacing.seat-card-width}'
    selected-ring: '2px {colors.accent}'
  seat-card-human:
    background: '{colors.surface-2}'
    border: '0.5px {colors.control}'
    avatar-ring: '2px {colors.ink}'
    you-badge-background: '{colors.ink}'
    you-badge-foreground: '{colors.surface}'
  team-box:
    background: '{colors.surface}'
    radius: '{rounded.xl}'
    shadow: '{elevation.raised}'
  team-container-expanded:
    background: 'rgba(127,127,140,0.06)'
    border: '0.5px {colors.hairline}'
    radius: '14px'
  reporting-line:
    stroke: '{colors.control}'
    width: 1px
  grant-line:
    stroke: '{colors.accent}'
    width: 1.25px
    dash: '3 4'
  advises-line:
    stroke: '{colors.ink-3}'
    width: 1px
    dash: '2 3'
  handoff-line:
    # Cross-department hand-off on the org chart (added 2026-11-18, replan): dashed ink with an arrowhead.
    # Distinct from reporting (solid control, no arrow) and grant (dotted accent). Hidden at rest.
    stroke: '{colors.ink-2}'
    width: 1.25px
    dash: '7 4'
    arrowhead: '{colors.ink-2}'
    label-background: '{colors.surface}'
    label-border: '0.5px {colors.control}'
    label-foreground: '{colors.ink-2}'
    hover-stroke: '{colors.ink}'
  department-tag:
    # Small uppercase tag naming a department wherever work crosses departments.
    height: 16px
    radius: '{rounded.xs}'
    border: '0.5px {colors.control}'
    foreground: '{colors.ink-2}'
    typeface: '{typography.caption}'
  status-dot:
    size: 8px
    running: '{colors.accent}'
    done: '{colors.ok}'
    waiting: '{colors.warn-dot}'
    failed: '{colors.err}'
    idle: '{colors.control}'
  capability-chip:
    radius: '{rounded.xs}'
    border: '0.5px {colors.control}'
    tested-mark: '{colors.ok}'
    known-mark: '{colors.ink-3}'
    warning-border: '{colors.warn-dot}'
    warning-foreground: '{colors.warn}'
  local-badge:
    background: '{colors.accent-soft}'
    foreground: '{colors.accent}'
    radius: '{rounded.full}'
  side-sheet:
    background: '{colors.surface}'
    width: '{spacing.sheet-width}'
    shadow: '{elevation.floating}'
  avatar-tints:
    # AI seat avatars: white initials on one muted tint, same in light and dark (added 2026-11-18, Agent card Identity).
    # Each passes AA for its white initials (>= 6.0:1). Muted on purpose: none competes with the accent.
    slate: '#5C6273'
    steel: '#4E6478'
    sage: '#566352'
    stone: '#6A6157'
    plum: '#65586E'
    clay: '#73594E'
  agent-card-index:
    # The jump row under the Agent card header (restated 2026-11-19): five group anchors (Identity, Brain, Powers, Context, Limits), current group in accent.
    height: 28px
    foreground: '{colors.ink-2}'
    current: '{colors.accent}'
    divider: '0.5px {colors.hairline}'
  capability-chip-row:
    # Agent card › Capabilities (restated 2026-11-19, final review 2 UX high 1): one wrapping row of on/off chips below Brain.
    chip-height: 28px
    chip-radius: '{rounded.full}'
    chip-on-background: '{colors.accent-soft}'
    chip-on-foreground: '{colors.accent}'
    chip-off-border: '0.5px {colors.control}'
    chip-off-foreground: '{colors.ink-2}'
    chip-pending-border: '1.5px {colors.warn-dot}'
    gap: 6px
    max-lines: 2
  capability-row:
    # Kept for the expand popover's provider line (2026-11-18 row layout retired from the card on 2026-11-19).
    height: 44px
    divider: '0.5px {colors.hairline}'
    switch-on: '{colors.accent-fill}'
    switch-off: '{colors.control}'
    switch-knob: '#FFFFFF'
    provider-builtin: '{colors.ink-2}'
    provider-account: '{colors.ink}'
  skill-card:
    background: '{colors.surface-2}'
    radius: '{rounded.md}'
    border: '0.5px {colors.hairline}'
    asks-for-missing: '{colors.warn}'
  step-row:
    background: '{colors.surface}'
    radius: '{rounded.lg}'
    number-done: '{colors.ok}'
    number-running: '{colors.accent-fill}'
    number-waiting: '{colors.warn-dot}'
  handoff-label:
    next: '{colors.ink-3}'
    loop-back: '{colors.ink-2}'
    gate: '{colors.warn}'
    across: '{colors.ink-2}'
  branch-group:
    # Parallel fan-out inside one step: a surface-2 well holding branch cards side by side.
    background: '{colors.surface-2}'
    border: '0.5px {colors.hairline}'
    radius: '{rounded.lg}'
    branch-background: '{colors.surface}'
    branch-radius: '{rounded.md}'
    branch-selected-ring: '2px {colors.accent}'
    gap: '{spacing.2}'
  consolidating-step:
    waiting-number-ring: '1px {colors.warn-dot}'
    quorum-line: '{typography.caption}'
  trigger-line:
    # "Starts when…" one-liner at the top of a workflow.
    background: '{colors.surface}'
    radius: '{rounded.md}'
    shadow: '{elevation.raised}'
  lineage-trace:
    node-background: '{colors.surface}'
    node-radius: '{rounded.md}'
    rail: '1px {colors.control}'
    node-marker: '1.5px {colors.control}'
    gate-marker: '1.5px {colors.warn-dot}'
    open-ring: '1.5px {colors.accent}'
    indent: 10px
  untrusted-well:
    background: '{colors.untrusted}'
    edge: '3px {colors.untrusted-edge}'
    label: '{colors.warn}'
    typeface: '{typography.mono}'
  payload-well:
    background: '{colors.surface-2}'
    border: '0.5px {colors.hairline}'
    typeface: '{typography.mono}'
    radius: '{rounded.md}'
  proposal-card:
    background: '{colors.surface}'
    border: '0.5px {colors.control}'
    diff-background: '{colors.surface-2}'
    removed: '{colors.err}'
    added: '{colors.ok}'
    radius: '{rounded.lg}'
  proposal-card-risk:
    border: '1.5px {colors.warn-dot}'
    flag-background: '{colors.warn-fill}'
    flag-foreground: '{colors.warn}'
  citation-chip:
    background: '{colors.accent-soft}'
    foreground: '{colors.accent}'
    radius: '{rounded.xs}'
  inbox-row:
    selected-background: '{colors.accent-soft}'
    divider: '{colors.hairline}'
  status-pill:
    radius: '{rounded.full}'
    neutral-background: '{colors.surface-2}'
    accent-background: '{colors.accent-soft}'
    warn-background: '{colors.warn-fill}'
    err-background: '{colors.err-fill}'
  grid:
    header-background: '{colors.surface-2}'
    divider: '{colors.hairline}'
    selected-row: '{colors.accent-soft}'
    typeface: '{typography.callout}'
  kpi-card:
    background: '{colors.surface}'
    figure: '{typography.metric}'
    radius: '{rounded.lg}'
  chart-card:
    measure: '{colors.accent}'
    comparison: '{colors.control}'
    gridline: '{colors.hairline}'
  toast:
    background: '{colors.ink}'
    foreground: '{colors.surface}'
    radius: '{rounded.md}'
  command-palette:
    background: '{colors.surface}'
    width: 560px
    radius: '{rounded.xl}'
    active-row: '{colors.accent-soft}'
  wizard-panel:
    width: 620px
    background: '{colors.surface}'
    radius: '{rounded.xl}'
    progress-on: '{colors.accent}'
    progress-off: '{colors.hairline}'
  account-card:
    background: '{colors.surface-2}'
    border: '0.5px {colors.hairline}'
    radius: '{rounded.lg}'
  doctor-row:
    divider: '{colors.hairline}'
  document-page:
    background: '{colors.surface}'
    width: 620px
    title: '{typography.display}'
  empty-state:
    headline: '{typography.headline}'
    action: '{components.button-secondary}'
  skeleton:
    from: '{colors.hairline}'
    to: '{colors.surface-2}'
---

## Brand & Style

Agentic Manager should feel like an app that shipped with the Mac: quiet, exact, and slightly reserved. The owner runs a business; the app runs the owner's AI departments. It should never feel like a dashboard competing for attention. The scene to design for: 08:45 on a Tuesday, a calm window, one seat pulsing softly, a small number ticking beside the company name, a single card in the Inbox.

The posture is **native calm with visible guardrails**. Calm comes from restraint: one accent, the system font, generous whitespace on an 8px grid, Things/Linear density, motion only to show cause. Guardrails come from always-visible facts: cost beside the company name, Stop everything in the toolbar, untrusted content fenced, approvals that show the exact payload. A calm surface that hides its guardrails would not be trusted, so the guardrails are part of the style, not exceptions to it.

This is an org chart, not a node graph. The CrewAI canvas the owner used before (`imports/crewai-studio-canvas-current.png`) puts agents in one row and tasks in another, wired with curves, and reads poorly at five agents. Here, people and seats sit in a top-down tree, and work reads top to bottom in a step list.

→ Visual references: all thirteen key screens in [`mockups/`](mockups/). Spines win on conflict with any mock.

## Colors

One accent and a set of neutrals carry the whole UI. Status colours appear only as small dots and pills.

- **Accent (`#0A63E0` light / `#4C94FF` dark text, `#2468D6` dark fill)** marks what is selected, current or actionable: the primary button, the selected seat ring, the active sidebar item, the running step, grant lines, citations and the On this Mac badge. It is never decoration. [ASSUMPTION] The hue is a native-feeling blue; the owner asked for one accent but did not name it (open question).
- **Neutrals.** `bg` is the window, `surface` holds cards and sheets, `surface-2` holds wells, human seats and table headers, and `sidebar` sits slightly darker. `ink`, `ink-2` and `ink-3` are the three text levels; `ink-3` is the lowest level that still passes AA for small text. `hairline` draws dividers; `control` draws input borders and reporting lines (3:1 non-text).
- **Status.** `ok` (done), `warn-dot` (waiting on you, 80% budget), `err` (failed, stopped). These appear as 8px dots, pill text and pill fills (`warn-fill`, `err-fill`), never as large surfaces. Running uses the accent, so the eye goes to live work.
- **Untrusted (`untrusted`, `untrusted-edge`).** A pale ochre well with a left edge and a faint hatch. It means only one thing: this text came from outside (a web page, a file, an MCP server, or free text in a table row) and is shown as data.
- **Money is never coloured to persuade.** The meter stays neutral until 80% of budget is spent, then turns amber; at 100% it shows the stopped red.

**Contrast (WCAG 2.2 AA, computed):**

| Pair | Light | Dark |
|---|---|---|
| `ink` on `surface` | 16.8 | 14.9 |
| `ink-2` on `surface` | 6.4 | 6.5 |
| `ink-3` on `surface` | 4.9 | 4.9 |
| `accent` text on `surface` | 5.4 | 5.5 |
| `on-accent` on `accent-fill` | 5.4 | 5.2 |
| `warn` on `warn-fill` | 5.8 | 7.3 |
| `err` on `err-fill` | 5.7 | 6.1 |
| `ink-2` on `untrusted` | 6.1 | 5.9 |
| `control` border on `surface` (non-text) | 3.3 | — |
| white initials on each `avatar-tints` tint (lowest: slate) | 6.1 | 6.1 |

Avoid: a second brand colour, gradients on surfaces, coloured section backgrounds, red for anything but failure and budget stops, and green for money.

## Typography

The system font is the spec: SF Pro on macOS, Segoe UI Variable on Windows, system-ui on Linux. v1 ships on macOS only (amended 2026-11-19, owner Mac first); the stack stays complete so Windows and Linux in v1.x need no token change. No web fonts are downloaded, because the app loads no remote content.

| Role | Use |
|---|---|
| `display` 22/28 semibold | Wizard titles, document titles ("Report R1: change two sections…") |
| `title` 17/24 semibold | Surface headings (Accounts, Run #7, Table T1) |
| `headline` 13/20 semibold | Card titles, seat names, step titles, empty-state sentence |
| `body` 13/20 | Default text |
| `callout` 12/16 | Secondary lines, grid cells, occupant/model lines |
| `caption` 11/16 medium, +0.02em | Section labels in sheets (INSTRUCTIONS, MODEL), chip text |
| `metric` 28/32 semibold, tabular | KPI cards, estimate and actual figures |
| `mono` 12/18 | Prompts, payloads, diffs, file paths, queries |

All money, token counts, durations and counts use tabular numerals so a ticking meter never jitters. Body is 13px, never smaller than 11px. No all-caps except the 11px section labels in sheets and dialogs.

## Layout & Spacing

The grid is 8px with a 4px half-step. Window anatomy: a 216px sidebar on the left, a 48px toolbar across the main column, and the content below it. Side sheets (440px) and the Assistant (420px) slide in from the right over content; they never push it. The designed size is 1440×900; the minimum window is 1024×700.

- **Org canvas.** Seat cards are 192px wide; siblings are 28px apart and generations 40px apart. Collapsed teams show as single boxes. An expanded team becomes a soft container with its seats inside.
- **List-detail surfaces** (Inbox, Run, Tables) use a fixed left column (360–470px) and a flexible detail.
- **Density** is Things-like: one line of status per card, secondary detail on hover or in the sheet. Table rows are 32px.
- **Gutters** are 24px around surface content and 20px inside sheets. Sections inside sheets are separated by a hairline with 14px vertical padding.

## Elevation & Depth

There are three levels: `flat` (the canvas and the window background), `raised` (cards, seats, steps: a hairline-like shadow), and `floating` (sheets, menus, the palette, popovers, toasts). In dark mode, depth comes from lighter surfaces and a 0.5px light edge rather than shadow. A dimmed backdrop (opacity about 50%) is used only when a sheet takes focus over the org chart, so the seat keeps its place. A scrim is never stacked twice.

## Shapes

Corners follow macOS: 4px for chips, 6px for buttons and fields, 8px for wells, 10px for cards, seats and steps, and 12px for team boxes, the wizard, the palette and the window. Pills (`full`) are reserved for status, the cost meter, the You badge and the On this Mac badge. Avatars are circles; human avatars carry a 2px ink ring so a human seat is recognisable at a glance in either theme.

## Components

Visual specs. Behaviour lives in `EXPERIENCE.md` § Component Patterns under the same names.

- **Toolbar.** Company name (`headline`), then the **Cost meter**, then a breadcrumb in `ink-2`. Right side: Search ⌘K (ghost), Assistant ⌘J (secondary), **Stop everything** (`button-stop`: red text with a square glyph on a neutral button, always visible, never hidden in an overflow).
- **Cost meter.** A 24px pill with a 16px ring showing the share of the weekly budget spent, then "₹128 of ₹600 this week". States: rest (neutral ring), running (accent ring), amber at 80% (`warn-fill`), stopped (`err-fill`, "Stopped at budget"). → `mockups/key-org.html` §4.
- **Seat card.** 192px wide. Avatar and name (`headline`), role (`caption`, `ink-2`), occupant line (provider glyph + model + Account, `callout`), status line (status dot + words, with this week's spend right-aligned in tabular figures). Selected: 2px accent ring. Nothing else on the card.
- **Human seat card.** `surface-2` fill, `control` border, ringed avatar, the **You badge** (ink pill) and a **hat chip** (CEO, Vision Creator, Reviewer, Developer, or custom). Shows the hat's live fact ("Goal G1 · this quarter", "1 guidance note", "Nothing to review yet").
- **Team box (collapsed).** Name, weekly cost right-aligned, status dot and sentence, an avatar strip with the seat count ("5 seats · 1 is you"), and an active guidance note in a `surface-2` strip.
- **Team container (expanded).** A soft tinted area with the team label, status, seat count, cost and guidance pill at its top-left.
- **Reporting line / Grant line / Advises line.** Solid `control` 1px orthogonal lines for reporting. Grant lines are a dotted accent curve between team boxes with an accent-outlined label ("Department 1 reads Table T1 · read-only"), drawn only on hover or selection. Advises lines are a fine dash in `ink-3` with a small "advises" label.
- **Hand-off line** (`handoff-line`, added 2026-11-18). A cross-department hand-off: a dashed `ink-2` line (7 4) with a small arrowhead, routed between team boxes (curved around boxes, never through them), with a pill label in `surface` with a `control` hairline naming what moves ("**Document A**", bold name, optional `ink-2` qualifier such as "· back" or "· starts Flow F2"). Under the pointer the line and its label border go to `ink`. Hidden at rest; drawn only for the selected or hovered department (restated 2026-11-19: no global Flows toggle). A small legend (reports to · grant · hand-off) appears bottom-left only while lines are visible. Three line languages, never mixed: solid = reporting, dotted accent = grant, dashed ink with arrow = hand-off. → `mockups/key-org.html` §2.
- **Department tag** (`department-tag`). A 16px uppercase `caption` tag with a hairline ("DEPARTMENT 1", "DEPT 2") used wherever work crosses departments: hand-off lines in the step list, branch and lineage nodes, review-gate headers, trigger lines, popovers. Never coloured per department; department identity comes from avatar tints only.
- **Org chart.** A dotted canvas (16px dot grid in `hairline`) holding Team boxes, Seat cards and lines; a small Fit/zoom control floats bottom-right in `floating` elevation.
- **Assistant panel.** A 420px side sheet (`spacing.assistant-width`) with a composer pinned to the foot; owner messages are `accent-fill` bubbles, answers are plain `body` text with Citation chips and a collapsed mono query well.
- **Side sheet** (Seat sheet, Team sheet). Header: avatar, name (`title`), role, status pill, Esc. Body sections with `caption` labels. → `mockups/key-seat-sheet.html`, `mockups/key-team-sheet.html`.
- **Agent card** (the AI Seat sheet, added 2026-11-18; restated 2026-11-19, final review 2 UX). The Side sheet with a **jump row** (`agent-card-index`) of five groups under the header: Identity · Brain · Powers · Context · Limits, in `caption`, the group in view in accent; each section has a sticky `caption` header. Section order: Identity, Brain, Capabilities, Skills, Tools & connectors, Knowledge & inbox, Tasks, Memory notes, Budget, What this seat can touch; Skills, Knowledge & inbox, Memory notes and Budget collapse to a one-line `callout` summary with a chevron when empty or at template defaults. Sections keep the `caption` label style; no section gets a coloured background. **Identity** shows the avatar large (36px, initials on an `avatar-tints` tint), goal as one `body` line and backstory as a quiet `callout` paragraph in `ink-2`. **Capability chips** (`capability-chip-row`, restated 2026-11-19): nine 28px pill chips in one wrapping row (two lines at most) under a `callout` summary "5 of 9 on"; on = `accent-soft` fill, `accent` text and a ✓; off = transparent with a `control` hairline and `ink-2` text; a chip waiting for its widening review stays drawn off with a 1.5px `warn-dot` outline and a 6px `warn-dot` dot (never a faded half-on state); a non-built-in provider appends "· Tavily" in `ink`; a ▾ affordance opens the expand popover (summary line, and for Web search, Browse & read pages and Crawl the Provider menu with prices; for Folder access, restated 2026-11-19, the linked-folder list: each folder row with its access pill, patterns and a line of **file-type badges** (caption type, `rounded.full`; a type the seat's model reads natively in `colors.ok` with ✓, one served by the text fallback in `colors.ink-2` reading "text", one it cannot read in `colors.warn` on `colors.warn-fill` with ✗ and its reason on hover or focus), and, for a cloud model, a "Leaves this Mac → <host>" line in `colors.warn` with an arrow-out glyph). The section caption says once "Built in and free. Add an Account only if you want a different provider."; there is no per-chip "Free · no key" pill. **Add-seat popover** (added 2026-11-19): a 320px `popover` with Name, Role template (each with its capabilities in `caption`), Model and a primary Create; it never opens the Agent card. **Skill card** (`skill-card`): a `surface-2` well with the skill name (`headline`), description (`callout`, `ink-2`), reference-file count, and "asks for" chips: a granted tool as a plain chip, a missing one with the `warn-dot` border and "not granted" — never accent, because a skill grants nothing. **Link row**: title, host in `ink-3` (never a favicon or remote image), group label, and a `warn` "host not allowed" line when it is outside the allow-list. **Reference file row**: name, size, a neutral "Kept" pill. **Task row**: a small Step row (number, workflow and step title, schedule in words, last-run dot and cost). → `mockups/key-seat-sheet.html` §1–3.
- **Account › Model picker.** A floating menu grouped by Account (logo, name, local badge). Rows: model name, capability chips, an optional "Recommended for <role>" accent pill, and a warning line in `warn` text. The current row is marked "✓ current".
- **Capability chip.** 20px with a 4px radius. Tested: ✓ in `ok` on `surface-2`. Known: hollow ○ in `ink-3`. Missing or failed: "!" with a `warn-dot` border.
- **Local badge** ("On this Mac"). An accent-soft pill. It is the only visual difference between local and cloud Accounts.
- **Step row.** A numbered circle (outline at rest; `ok` when done; accent fill with a soft halo when running; `warn-dot` when waiting), a title, a seat avatar and name with the output in `callout`, and tools or metrics right-aligned.
- **Hand-off** (`handoff-label`). Between steps: a 1px tick and words. "then" in `ink-3`; "↺ loop back…" in `ink-2`; "◆ waits for you…" and "◆ review gate · <seat> <DEPARTMENT>…" in `warn`; across departments, an `ink-2` tick and "⇢ hands across to <DEPARTMENT> · <item> · within grant, read-only".
- **Branch group** (`branch-group`, added 2026-11-18). A parallel fan-out step: one `surface-2` well holding a header (step number, title, a neutral "⫘ in parallel · 3 branches" pill, a right-aligned "1 of 3 done") and N branch cards side by side in equal columns (2–4 fit the editor; 5+ wrap to a second row). Each branch card: seat avatar and name (`headline`), model in `caption` `ink-2`, then status dot + words and time · cost right-aligned in tabular figures. Selected branch: 2px accent ring. Under the cards, one `caption` line. → `mockups/key-workflow-run.html` §1–2.
- **Consolidating step** (`consolidating-step`). A Step row whose number carries a `warn-dot` ring while it waits, with "waiting for 2 of 3" in `ink` semibold and a `caption` line "Quorum 3 of 3 · timeout in 1h 52m, then continues with what arrived". Its editor uses a segmented control (All · Quorum), two small fields (count, timeout) and an input chip per branch labelled by seat.
- **Trigger line** (`trigger-line`). A raised one-line card at the top of a workflow: "⚡ **Starts when** Document A is updated <DEPARTMENT> · or Run now", with "Loop-protected" in `ink-3` and Edit in accent at the right. Edit opens the **Starts when…** card: event field, what it hands over, a fixed "LOOP PROTECTION · ALWAYS ON" checklist (✓ lines, never switches) and the last starts, including any start that loop protection skipped.
- **Lineage trace** (`lineage-trace`). In a Document, one raised summary line under the title ("↳ How this report was made · 8 contributions · 2 departments · 3 models · ₹64 · 1h 12m" with Show lineage, L). Opened, a Side sheet titled Lineage: a vertical rail in `control` with indented node cards, newest contribution first; a parallel group is indented one level. Node: avatar, seat (`headline`), role and department tag, then model · output in `caption`, cost · time right-aligned. A review-gate node uses a diamond marker in `warn-dot`. One node at a time expands in place (accent ring) to inputs, skill, untrusted pages read (in `warn`), output and calls, with "Open step in Run" and "Show prompt" links. The sheet foot carries the total and Export (Markdown, JSON). → `mockups/key-inbox.html` §2.
- **Estimate line.** `callout` under the Run button: "About **₹80–120** · based on your last 4 runs". The range is in `ink` semibold, the basis in `ink-2`.
- **Estimate vs actual.** Two `caption` labels over `title`-size tabular figures, side by side, with a one-line verdict.
- **Untrusted well.** `untrusted` background, 3px `untrusted-edge` on the left, faint 135° hatch, a label "⚠ Untrusted · from <source>" in `warn` `caption`, content in `mono` `ink-2`, and an optional consequence line in `warn`.
- **Payload well.** A `surface-2` mono block that shows tool, host, args and size exactly as they will be sent.
- **Proposal card.** A header pill ("Proposal", "Run now?"), title, mono diff (removed in `err`, added in `ok`), an effect line, and a footer with Dismiss (ghost), Edit (secondary) and Apply (primary). **High-risk variant:** a 1.5px `warn-dot` border, a "⚠ High risk · widens access" flag, a bold effect sentence, and a primary labelled "Review and apply…".
- **Privilege-widening confirmation.** A floating dialog (the second step after a high-risk proposal card). The safe action is primary and the default; the widening action is secondary and named by its effect.
- **Inbox item** (`inbox-row`). A kind glyph in a 22px pill, title (`headline` when selected), and a source line in `caption`. Selected rows use `accent-soft`. The **While you were away** digest is a raised card above the list.
- **Table grid** (`grid`). Sticky `surface-2` header in `caption`, 32px rows, hairline dividers, status pills, and a provenance cell (seat avatar + run, or ringed human avatar + "You").
- **KPI card / Chart card.** `metric` figure with a `caption` label and a context line. Charts use the accent for the measure that matters and `control` grey for comparison, with hairline gridlines and no chart junk.
- **Account card, Doctor row, History entry, Undo toast (`toast`), Command palette, Wizard step (`wizard-panel`), Document (`document-page`), Empty state, Skeleton** follow the tokens in frontmatter.
- **Composites.** These `EXPERIENCE.md` components add no new visual parts; they combine the primitives above. Team sheet, Schedule picker, Budget field, Drop folder (with Paste links), Reference files, Saved links, Skill import, Memory notes, Tools library, MCP server add and Ask a seat use Side sheet, fields, `button-secondary` and wells. Review gate and Import / Export (import diff) use Grid, `status-pill` and `proposal-card` diff colours. Drill-down uses `payload-well` and `untrusted-well`. Provenance uses avatars and `callout` text. Saved view tab uses the segmented tab style. Test this model and Dry-run use `button-secondary` with an inline result line. Guidance note uses a `surface-2` strip on team boxes and a neutral pill in expanded containers. Estimate vs actual uses `typography.metric` at title size. Team sheet → `mockups/key-team-sheet.html`; MCP server add and its Inbox re-approval → `mockups/key-mcp-add.html`. Connectors (added 2026-11-18): the Sources list uses the list-detail layout with 22px type icons in `surface-2` tiles and status dots; Connector catalogue cards are `card` surfaces with a 28px `accent-soft` icon tile and a `status-pill`, and a later type is a muted `surface-2` card with a hairline and no button; Add Source is the dialog shell of MCP server add with a segmented step line (accent for done and current); Test connector is a checklist (✓ `ok`, ! `warn`) beside an `untrusted-well` sample; the Ingest report uses four `surface-2` count tiles in tabular figures, `caption` lines and a `grid` of rejected rows → `mockups/key-connectors.html`. → `mockups/key-models-doctor-history.html`, `mockups/key-states-keyboard.html`, `mockups/key-first-run.html`, `mockups/key-inbox.html`.

## Do's and Don'ts

| Do | Don't |
|---|---|
| One accent, for selection, current work and the primary action | A second brand colour, or accent used for decoration |
| Status as 8px dots and small pills | Coloured card fills or banners for routine status |
| Tabular figures for every number that changes | Proportional digits in the meter, estimates or grids |
| Keep Stop everything and the cost meter visible on every surface | Move either into a menu, or hide them in narrow windows |
| Fence untrusted content in its well, as plain text | Render fetched HTML, links or remote images inside a run |
| Show the exact payload before any approval | Summarise what a tool "will do" in place of the payload |
| Seat cards with name, role, model, status and spend only | Tools, budgets or instructions on the org canvas |
| A top-down tree with solid reporting lines | Node-graph wiring, an agents row above a tasks row, or crossing curves |
| Hand-off lines only for the selected department and grant lines only on hover and in the Team sheet (restated 2026-11-19); parallel branches and lineage collapsed until asked for | Always-on flow wiring on the org chart, or a lineage graph open by default |
| System font, 13px body | Web fonts, or body text under 11px |
| Light and dark built from the same token names | Dark mode by inverting light colours |
