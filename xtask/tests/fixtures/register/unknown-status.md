# Planted register: unknown status

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. One row with Status `done`.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | A planted item that passes. | Planted, 2026-11-25. | 1.2 | no | open |
| DR-002 | residual | 1.2 | A planted row with a Status the Columns do not allow. | Planted, 2026-11-25. | none | no | done |
| DR-003 | accepted risk | 1.3 | A planted item that passes. | Planted, 2026-11-25. | none | no | accepted |
