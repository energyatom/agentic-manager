# Planted register: release confirmed

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. The release register with each failing row confirmed, closed or set to Must close `no`.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | Confirmed: an open no row whose Target builds after the gate. | Planted, 2026-11-25. | 2.2 | no | open |
| DR-002 | residual | 1.2 | Confirmed: a closed yes row. | Planted, 2026-11-25: closed by a planted PR. | 1.2 | yes | closed |
| DR-003 | owner question | 1.3 | Set to Must close no. | Planted, 2026-11-25. | owner | no | open |
| DR-004 | deferred | 1.4 | Closed. | Planted, 2026-11-25: closed by a planted PR. | unassigned | yes | closed |
| DR-005 | deferred | 1.5 | Set to Must close no and accepted. | Planted, 2026-11-25: accepted. | none | no | accepted |
| DR-006 | deferred | 1.1 | Closed. | Planted, 2026-11-25: closed by a planted PR. | 1.4 | yes | closed |
| DR-007 | deferred | 1.2 | Closed. | Planted, 2026-11-25: closed by a planted PR. | 1.4, 3.1 | yes | closed |
| DR-008 | deferred | 1.3 | Closed. | Planted, 2026-11-25: closed by a planted PR. | 3.2 | yes | closed |
| DR-009 | residual | 1.4 | Confirmed and closed. | Planted, 2026-11-25: closed by a planted PR. | 3.1 | yes | closed |
| DR-010 | deferred | 3.1 | Set to Must close no. | Planted, 2026-11-25. | 1.5 | no | open |
| DR-011 | deferred | 3.2 | Carried: epic 2 builds after epic 3. | Planted, 2026-11-25. | 2.1 | yes | open |
| DR-012 | deferred | 3.3 | Carried: one of its stories builds after the gate. | Planted, 2026-11-25. | 1.1, 3.3 | yes | open |
| DR-013 | deferred | 2.1 | Carried: its story builds after the gate. | Planted, 2026-11-25. | 3.3 | yes | open |
| DR-014 | deferred | 2.2 | Ignored: closed. | Planted, 2026-11-25: closed by a planted PR. | 1.2 | yes | closed |
| DR-015 | accepted risk | 1.1 | Ignored: accepted, nothing owed. | Planted, 2026-11-25: accepted. | none | no | accepted |
| DR-016 | deferred | 1.2 | Ignored: open but not must-close. | Planted, 2026-11-25. | 1.1 | no | open |
