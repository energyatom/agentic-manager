# Accessibility Review — Agentic Manager

Adversarial pass against WCAG 2.2 AA and RULE-19 (keyboard-complete, contrast, screen-reader labels, reduced motion).

- **medium** Single-key shortcuts (EXPERIENCE.md › Interaction Primitives). N, A, E, R, ? and Space could fire while typing or with speech input (WCAG 2.1.4). *Fix:* Resolved in this run: they act only with focus on their surface and can be turned off in Settings.
- **low** Amber status dot is below 3:1 on white (DESIGN.md › colors.warn-dot). #E59A00 on #FFFFFF is about 2.2:1. Every status also has a word, so the dot is not the only signal (1.4.11 is satisfied by the text). *Fix:* Keep the rule that every dot sits beside a status word; do not ship dot-only legends.
- **low** Untrusted hatch under Increased Contrast (DESIGN.md › untrusted-well). The hatch may add visual noise for low-vision users. *Fix:* Already specified: Increased Contrast removes the hatch and keeps the edge and label.
- **low** Toast timing (EXPERIENCE.md › Undo toast). A 6 s toast could be missed (2.2.1). *Fix:* Satisfied: ⌘Z and History provide the same undo without time limit.
