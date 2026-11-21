# Owner standing rules

## Decide yourself (record the choice and reason in the plan; don't ask)
- Field names, test structure, internal crate/module layout, error and fix wording.
- Small scope trims inside a story, and ranges/limits that stay within the architecture.
- Picking the recommended option when it is a pure engineering call.
- Test timing limits and flaky-test fixes, with the cause recorded.
- Merging a story whose CI is green on BOTH jobs, after review findings are triaged.

## Always stop and ask the owner
- Money: spending, budgets, caps, anything touching the ledger.
- User data: anything that deletes, moves or overwrites user files or project data.
- Licences, and any new dependency not in the Stack.
- Security: egress, permissions, grants, sandbox, secrets, any widening.
- Changing an AD, a ticket's intent or a frozen plan.
- Anything irreversible, or anything that publishes (repo visibility, releases).
- Merging when CI is not green on both jobs.
- A tests-first breach (no logged red run before src/ changes).

## Always (unchanged)
- Tests first: the red run (command + failing output) is logged before any src/ change.
- One story = one squash commit. Planning documents are never edited to fit the code.
- Owner-level deviations are asked, never decided silently. Exceptions so far: 1.1, 1.13.
