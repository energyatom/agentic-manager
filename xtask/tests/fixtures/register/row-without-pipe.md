# Planted register: row without pipe

Test data for story 3.25's register check (xtask/tests/register.rs), checked
against the planted tree in tickets/. GitHub renders a line straight after
the table as another row even without its leading pipe (review R1); this one
has a missing Target story, an unknown Must close and an unknown Status.

## Register

| Id | Kind | Source | Item | Decision | Target | Must close | Status |
|----|------|--------|------|----------|--------|------------|--------|
| DR-001 | accepted risk | 1.1 | A planted item that passes in both modes. | Planted, 2026-11-25: accepted. | none | no | accepted |
| DR-002 | residual | 1.2 | A planted item that passes in both modes. | Planted, 2026-11-25: closed by a planted PR. | 1.2 | yes | closed |
DR-003 | deferred | 1.3 | A planted row with no leading pipe, straight after the table. | Planted, 2026-11-25. | 1.9 | maybe | done |
