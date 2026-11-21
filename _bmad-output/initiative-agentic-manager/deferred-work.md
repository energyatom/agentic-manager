- source_plan: `_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/spike-stack-spike-nine-criteria-and-a-verdict-plan.md`
  summary: Builds run outside `just` (plain `cargo check`/`cargo nextest`) write `target/` inside the OneDrive-synced project folder, because `CARGO_TARGET_DIR` is exported only by the justfile.
  evidence: The justfile exports `CARGO_TARGET_DIR` (default ~/Library/Caches/agentic-manager/target); there is no shell or cargo config doing the same, and AGENTS.md tells agents to iterate with plain `cargo check`. The fix is per-machine (shell profile) or a project-context note.
  resolved: 2026-11-20, owner. `.claude/settings.local.json` (per-machine, git-ignored) sets `CARGO_TARGET_DIR=/Users/hiteshmamgain/cargo-target/agentic-manager` for every Claude Code session in this project, so plain `cargo` runs build outside OneDrive. The justfile's `env_var_or_default` honours the same value.
- source_plan: `_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/story-test-kit-fakes-plan.md`
  summary: Tokio's `test-util` feature, enabled as an am-testkit dev-dependency, is unified into every crate during workspace test runs, so a crate that uses paused time without declaring `test-util` itself passes `just check` but fails `cargo nextest run -p <crate>`.
  evidence: Story 1.3 review finding 5. Cargo resolver 3 unifies features across a workspace build. A project-context note ("declare tokio test-util in any crate whose tests pause time") would catch it; it was not fixable in the story's diff.
  resolved: 2026-11-22, epic 1 story 11 (refactor sweep): `xtask/tests/paused_time.rs` fails `just check` when a crate's code or tests pause tokio time without enabling `test-util` in its own Cargo.toml.
- source_plan: `_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/story-blank-department-role-templates-and-the-company-a-sample-plan.md`
  summary: The Company A sample ships no synthetic drop files or rows for table-t1, so a freshly installed sample has an empty T1 until a load runs.
  evidence: The reference says T1 is loaded from a drop folder of synthetic CSV and xlsx files, but `.agentic/drop/` is runtime and the story 1.5 ticket does not list drop files. The golden-run fixtures (epic 3) or the Local Repository story (4.7) should supply them.
  resolved: 2026-11-22, named in new story 4.19 (epic 1 story 11 sweep scoping).
- source_plan: `_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/story-blank-department-role-templates-and-the-company-a-sample-plan.md`
  summary: company.yaml has no company-wide caps key, so the Company A sample's ₹600-a-week company cap is not written; epic 4's budget stories should add the key and the sample's value.
  evidence: Owner decision (c) of story 1.5 (2026-11-20). reference-workload.md sets ₹600 a week for the company; the sample's description names the gap.
  resolved: 2026-11-22, owned by story 4.2, whose company week cap needs the key (epic 1 story 11 sweep scoping).
- source_plan: `_bmad-output/initiative-agentic-manager/tickets/epic-foundations-rust-spike/tickets/story-hash-chained-audit-log-plan.md`
  summary: Writer commands that change no row (MarkLost on an already settled call, StartStep on an unknown step id) still commit and audit outcome `ok`; they should refuse or audit a "no change" outcome.
  evidence: Spike behaviour predating story 1.7 (`apply` in crates/am-store/src/lib.rs: MarkLost's `WHERE ... AND settled = 0` and StartStep's UPDATE return Ok when 0 rows change); found by the story 1.7 quick review. The workflow-engine stories that own these commands should decide refuse vs. "no change".
  resolved: 2026-11-22, named in new story 3.18 (epic 1 story 11 sweep scoping).
