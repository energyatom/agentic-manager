# Planted register: malformed cells

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. A malformed id, malformed Targets, an empty Item, an empty Decision and a Source with no leading story.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | A planted item that passes. | Planted, 2026-11-25. | 1.2 | yes | open |
| DR-7 | deferred | 1.1 | A planted row whose id is malformed and whose Status is unknown. | Planted, 2026-11-25. | 1.2 | yes | done |
| DR-002 | deferred | 1.2 | A planted row whose Target lacks a comma. | Planted, 2026-11-25. | 1.2 1.3 | yes | open |
| DR-003 | deferred | 1.3 | A planted row whose Target mixes a keyword with a story. | Planted, 2026-11-25. | owner, 1.2 | yes | open |
| DR-004 | deferred | 1.4 | A planted row whose Target ends in an empty element. | Planted, 2026-11-25. | 1.2, | yes | open |
| DR-005 | residual | 2.1 |  | Planted, 2026-11-25. | none | no | accepted |
| DR-006 | residual | 2.2 | A planted row with an empty Decision. |  | none | no | accepted |
| DR-008 | deferred | Sofia, REPLAN | A planted row whose Source names no story first. | Planted, 2026-11-25. | none | no | accepted |
