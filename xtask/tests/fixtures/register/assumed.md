# Planted register: assumed

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. The clean register with one `yes (assumed)` and one `no (assumed)` row.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1, planted plan triage D1 | A planted item owed by the spike. | Planted, 2026-11-25. | 1.1 | yes | open |
| DR-002 | accepted risk | 1.2 planted plan review | A planted accepted risk. | Planted, 2026-11-25: accepted. | none | no | accepted |
| DR-003 | residual | 1.5 (planted plan residual 1) | A planted residual owed by two stories. | Planted, 2026-11-25. | 1.5, 2.1 | yes (assumed) | open |
| DR-004 | owner question | 3.1; planted plan Q1 | A planted question for the owner. | Planted, 2026-11-25. | owner | no | open |
| DR-005 | deferred | 2.1 | A planted item with no story named yet. | Planted, 2026-11-25. | unassigned | no (assumed) | open |
| DR-006 | residual | 1.3, planted plan residual 2 | A planted residual already closed. | Planted, 2026-11-25: closed by a planted PR. | 3.2 | yes | closed |
| DR-007 | deferred | 1.4, planted plan triage D2 | A planted item owed by the last story. | Planted, 2026-11-25. | 2.2 | no | open |
