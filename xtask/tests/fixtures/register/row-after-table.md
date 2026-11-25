# Planted register: row after table

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. A blank line ends the table, and another row follows it.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | A planted item that passes. | Planted, 2026-11-25. | 1.2 | yes | open |
| DR-002 | residual | 1.2 | A planted item that passes. | Planted, 2026-11-25. | none | no | accepted |

| DR-003 | deferred | 1.3 | A planted row after the table ended. | Planted, 2026-11-25. | 1.4 | yes | open |
