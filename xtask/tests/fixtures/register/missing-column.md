# Planted register: missing column

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. One row with 7 cells and one with 9.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | A planted item that passes. | Planted, 2026-11-25. | 1.2 | yes | open |
| DR-002 | deferred | 1.2 | A planted row missing its Status cell. | Planted, 2026-11-25. | 1.3 | yes |
| DR-003 | residual | 1.3 | A planted row with one cell too many. | Planted, 2026-11-25. | none | no | accepted | extra |
| DR-004 | deferred | 2.1 | A planted item that passes. | Planted, 2026-11-25. | 2.2 | no | open |
