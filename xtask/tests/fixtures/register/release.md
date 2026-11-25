# Planted register: release

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. Run in release mode at gate 3.2 (and 1.3): assumed, due, carried and ignored rows.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | Assumed only: an open no (assumed) row whose Target builds after the gate. | Planted, 2026-11-25. | 2.2 | no (assumed) | open |
| DR-002 | residual | 1.2 | Assumed only: a closed yes (assumed) row. | Planted, 2026-11-25: closed by a planted PR. | 1.2 | yes (assumed) | closed |
| DR-003 | owner question | 1.3 | Due: an owner decision is pending. | Planted, 2026-11-25. | owner | yes | open |
| DR-004 | deferred | 1.4 | Due: no story is named yet. | Planted, 2026-11-25. | unassigned | yes | open |
| DR-005 | deferred | 1.5 | Due: an open must-close row that names no story. | Planted, 2026-11-25. | none | yes | open |
| DR-006 | deferred | 1.1 | Due at gate 3.2, carried at gate 1.3. | Planted, 2026-11-25. | 1.4 | yes | open |
| DR-007 | deferred | 1.2 | Due: both stories build before gate 3.2. | Planted, 2026-11-25. | 1.4, 3.1 | yes | open |
| DR-008 | deferred | 1.3 | Due: it names the gate story itself. | Planted, 2026-11-25. | 3.2 | yes | open |
| DR-009 | residual | 1.4 | Assumed and due at gate 3.2. | Planted, 2026-11-25. | 3.1 | yes (assumed) | open |
| DR-010 | deferred | 3.1 | Due at gates 3.2 and 1.3: 1.5 is listed before 1.3. | Planted, 2026-11-25. | 1.5 | yes | open |
| DR-011 | deferred | 3.2 | Carried: epic 2 builds after epic 3. | Planted, 2026-11-25. | 2.1 | yes | open |
| DR-012 | deferred | 3.3 | Carried: one of its stories builds after the gate. | Planted, 2026-11-25. | 1.1, 3.3 | yes | open |
| DR-013 | deferred | 2.1 | Carried: its story builds after the gate. | Planted, 2026-11-25. | 3.3 | yes | open |
| DR-014 | deferred | 2.2 | Ignored: closed. | Planted, 2026-11-25: closed by a planted PR. | 1.2 | yes | closed |
| DR-015 | accepted risk | 1.1 | Ignored: accepted, nothing owed. | Planted, 2026-11-25: accepted. | none | no | accepted |
| DR-016 | deferred | 1.2 | Ignored: open but not must-close. | Planted, 2026-11-25. | 1.1 | no | open |
