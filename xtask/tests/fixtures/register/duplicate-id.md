# Planted register: duplicate id

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. One id on two rows.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | A planted item that passes. | Planted, 2026-11-25. | 1.2 | yes | open |
| DR-002 | deferred | 1.2 | The first planted row with this id. | Planted, 2026-11-25. | 1.3 | yes | open |
| DR-003 | residual | 1.3 | A planted item that passes. | Planted, 2026-11-25. | none | no | accepted |
| DR-002 | deferred | 1.4 | The second planted row with the same id. | Planted, 2026-11-25. | 3.1 | no | open |
| DR-004 | deferred | 2.1 | A planted item that passes. | Planted, 2026-11-25. | 2.2 | no | open |
