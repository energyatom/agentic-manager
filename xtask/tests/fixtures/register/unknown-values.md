# Planted register: unknown values

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. Kinds and Must close values the Columns do not allow; every match is exact and case-sensitive.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | risk | 1.1 | A planted row with an unknown Kind. | Planted, 2026-11-25. | 1.2 | no | open |
| DR-002 | Deferred | 1.2 | A planted row with a Kind in the wrong case. | Planted, 2026-11-25. | 1.3 | no | open |
| DR-003 | deferred | 1.3 | A planted row with an unknown Must close. | Planted, 2026-11-25. | 1.4 | maybe | open |
| DR-004 | deferred | 1.4 | A planted row whose assumed mark has no space. | Planted, 2026-11-25. | 3.1 | yes(assumed) | open |
| DR-005 | residual | 2.1 | A planted row whose Must close is the mark alone. | Planted, 2026-11-25. | 2.2 | assumed | open |
| DR-006 | deferred | 2.2 | A planted item that passes. | Planted, 2026-11-25. | none | no | accepted |
