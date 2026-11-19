# UX final review 2 (independent senior product designer)

Scope: DESIGN.md, EXPERIENCE.md, all 13 mockups rendered at 1440 wide and viewed, and the UI tickets (org-seats, runs-inbox-docs, connectors 12.19, polish). The archive was not read.

## Verdict: SHIP WITH CHANGES. 0 critical, 4 high, 6 medium, 4 low.

The system is coherent and calm. It has one accent, one type scale, honest cost and untrusted treatments, and a Stop everything button on every screen. The hand-off, fan-out, gate and lineage additions are well integrated. The weak spots are the length of the Agent card and one overstated promise in the zero-key first run.

### Direct answers

- **Capabilities and the Agent card.** The card is drifting toward a settings dump. Capabilities is nine rows (about 560px) placed before Brain, so the model and instructions fall below the first screen. Ten sections and about 3.5 screens of scroll is a lot for something that must feel like "an agent in 60 seconds".
- **Org chart.** It is readable at rest and with Flows on, but only with two departments. At five or more teams, the dotted grant curves, dashed hand-off arrows, pill labels and a popover will tangle.
- **Zero-key first run.** The promise is real for web search, reading and OCR. It is not yet simple, because a model is still required and the mock does not draw the zero-key path.

## High

1. **Capabilities pushes Brain off-screen** (`key-seat-sheet.html` §1; EXPERIENCE Seat sheet, Capabilities).
   - Nine near-identical rows each repeat a "Built-in · Free · no key" pill. Five of them (Files, Tables, Memory, Hand-off, Calculate) have only a static label and no choice.
   - Rendered rows are 56–70px, while the spec says 44px.
   - Fix:
     - (a) Reorder to Identity, Brain, Capabilities.
     - (b) Collapse the section to a single line, "5 of 9 on: Web search, Browse, Documents…", with Edit expanding it.
     - (c) Show only the four rows that have a real choice (Web search, Browse, Crawl, Documents) and put the five local ones in one "Workspace" row with a single switch.
     - (d) Drop the per-row Provider pill unless a non-default provider is set.
   - Doing (c) and (d) removes most of the noise.
2. **No drawn fast path for "agent in under 60 seconds."**
   - The only add-seat path is `N` plus the role template, and no mock shows it. The ten-section card is an editor, not a creator.
   - Fix: add one small "New seat" popover (name, role template, model defaulting to the company default, Create), with everything else left default. Draw it in `key-seat-sheet.html`, and state that the Agent card is for editing.
3. **Zero-key first run is overstated** (`key-first-run.html` step 2 and step 4; UJ-2 failure line).
   - Step 2 shows a pasted Anthropic key, and step 4 spends ₹0.84 on cloud models, which is the opposite of the headline claim. The real zero-key state (no LM Studio or Ollama) appears only as a small text card in `key-schedule-updates.html` §3, which tells the owner to go install a separate app.
   - Fix:
     - (a) Make the default drawing of step 2 the zero-key state: local servers found or not, and one primary button.
     - (b) Move the Anthropic key to a collapsed "Add a key later" row.
     - (c) When no server is found, offer a one-click bundled small model or Install LM Studio, not prose.
     - (d) Step 4 should say "Cost: ₹0" on the local path.
4. **Terminal text in a "no terminal" product** (`key-states-keyboard.html`, "Server unreachable": "Start it with agentic serve").
   - Fix: for browser mode, give a Reopen the app button and a link to Settings › About. Keep the CLI text only in docs.

## Medium

5. **The org chart will not scale.**
   - Flows-on shows two hand-offs, two grants and a popover, with long swooping curves under the boxes (`key-org.html` §2).
   - Fix:
     - (a) Make Flows a per-selection view only. Drop the global toggle, or cap it at three lines.
     - (b) Show grants only in the Team sheet. The canvas keeps just reporting and hand-off, which removes one of the three line languages.
     - (c) When a grant and a hand-off join the same pair of teams, draw one line labelled "Document A · read-only".
6. **Index row clips** (`key-seat-sheet.html`).
   - "Can to" is cut off at the right edge, and ten anchors do not fit 440px.
   - Fix: drop the row for sticky section headers, or reduce it to five groups (Identity, Brain, Powers = Capabilities + Skills + Tools, Context = Knowledge + Memory, Limits = Budget + Can touch).
7. **Mockup label overlap** (`key-schedule-updates.html` §3).
   - The captions "WIZARD STEP 2 · NO LOCAL SERVER" and "WIZARD STEP 4 · SLOW LOCAL LOAD" overprint their card titles. The "Add OpenAI-compatible URL" card is overlapped the same way.
   - Fix: add 20px top padding to the card or move the caption out of flow.
8. **First-run step 2 footer is cramped** (`key-first-run.html`).
   - "Nothing here needs a key…" wraps into the Back/Continue buttons, and there is no bottom padding below the Anthropic card.
   - Fix: let the panel grow (min-height with padding-bottom 24px) and shorten the footer line, or remove it, since the heading already says this.
9. **Several two-step reviews in a row.** Turning on five capabilities means five dialogs.
   - Fix: let one batch review cover several pending switches ("Review 3 changes…"). A new seat from a role template arrives pre-granted and needs no confirmation.
10. **Cost is invisible on a paid provider.**
    - Choosing an Account provider (Tavily, Exa) shows no price.
    - Fix: add "≈ ₹0.4 per search" in the provider menu row and in the review dialog.

## Low

11. **Sidebar is inconsistent across mockups.** "Connectors" appears in the sidebar only in `key-connectors.html`. Settings sub-nav and Org lack it. Make the sidebar identical in every mock.
12. **State drift in samples.** The Org mock shows Department 1 "Running · 3 branches", while the chart behind the seat sheet shows D1-Head "Done". D1-E2 reads "Done · ₹0 this week" in one place and "Working" in another. Align the sample states to one moment.
13. **Duplicated words** in lineage nodes ("D1-Head head", "D1-E1 branch 1"). Drop the role word when it repeats the name.
14. **Faded half-on switch** for Calculate while pending reads as disabled. Show an off switch with the amber flag instead, since the spec says it does not flip until applied.

## Visual and token check

- **Tokens.** Blue, amber and red are used consistently, 10px card radii are consistent, tabular figures are used, and the dark variants hold up.
- **Domain words.** The mockups, DESIGN.md and EXPERIENCE.md are clean of [removed], [removed], [removed], [removed] and the owner's personal name. The generic text "owner's CrewAI experience" and "he/his" in UJ-2 is acceptable but could be neutral ("the owner").
- **Leftover personal and domain content.** `ux-agentic-manager/.memlog.md` still names the owner personally, plus [removed], [removed]/[removed] and personal staff names (lines around 32 and 64). Delete the file or exclude it from the release and docs bundle.

## Keep as is

- The Stop everything button and the cost meter.
- Hover-only hand-off lines.
- The branch group in `key-workflow-run.html`, which is the best-composed new element.
- The one-line trigger bar.
- The collapsed lineage line with a sheet on request.
- The estimate and actual line.
- The safe-default two-step dialogs.
