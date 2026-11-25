# Planted register: no such story

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. Targets naming a missing entry (1.9), a missing epic (4.1) and one missing story among two (2.7).

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | deferred | 1.1 | A planted item that passes. | Planted, 2026-11-25. | 1.2 | yes | open |
| DR-002 | deferred | 1.2 | A planted item naming an entry epic 1 does not have. | Planted, 2026-11-25. | 1.9 | yes | open |
| DR-003 | residual | 1.3 | A planted item naming an epic the tree does not have. | Planted, 2026-11-25. | 4.1 | no | open |
| DR-004 | deferred | 1.4 | A planted item naming one real and one missing story. | Planted, 2026-11-25. | 3.1, 2.7 | yes | open |
| DR-005 | deferred | 2.1 | A planted item that passes. | Planted, 2026-11-25. | 2.2 | no | open |
