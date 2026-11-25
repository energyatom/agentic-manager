# Agentic Manager task runner. `just check` is the gate: fmt, clippy
# -D warnings, nextest and the offline repo checks (AD-4 dependency direction,
# every crate forbids unsafe, AD-23 vocabulary denylist, AD-32 runtime paths,
# AD-12 fixed hosts and the egress seam guard), am-store's doctests
# (the `compile_fail` proof that a write needs an Actor, story 1.6) and `agentic validate`
# over the valid fixture projects (story 1.4; story 3.3 adds hand-offs/) and
# templates/ (story 1.5).
# `just supply-chain` runs the licence, bans and sources policy (cargo-deny)
# and cargo-audit.

# Build output stays outside the synced project folder; share it across worktrees.
export CARGO_TARGET_DIR := env_var_or_default("CARGO_TARGET_DIR", env_var("HOME") + "/Library/Caches/agentic-manager/target")

default: check

# The story gate: format, lints, every offline test and the repo checks.
check: fmt-check clippy test doctest check-deps check-forbid denylist runtime-paths fixed-hosts seam-guard validate

fmt-check:
    cargo fmt --all -- --check

fmt:
    cargo fmt --all

# Default members: every crate but am-desktop (built only in E11 and CI).
clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo nextest run

# nextest runs no doctests; am-store's include the `compile_fail` proof that
# `Writer::exec` needs an Actor (AD-24, story 1.6) and that only a
# `HumanActor` resolves an Inbox item, never a `WriteCommand` (stories 3.3,
# 3.4); am-domain's, that nothing converts into or out of a TrustedInput
# (AD-10, story 3.3) and that an EngineResult has one constructor and no
# row-input type exists (story 3.6); am-engine's, that a StepPlan has no
# row-input slot (story 3.6).
doctest:
    cargo test --doc -p am-store -p am-domain -p am-engine

# AD-4: crate edges follow the Design Paradigm matrix (cargo metadata, offline);
# story 3.3 (AD-10): only am-engine calls TrustedInput's two constructors.
check-deps:
    cargo xtask check-deps

# Every crate inherits the workspace lints and forbids unsafe code.
check-forbid:
    cargo xtask check-forbid

# AD-23: no department vocabulary in crates/ or web/src.
denylist:
    cargo xtask denylist

# AD-32 (story 1.13): no Rust string literal under crates/ spells a runtime
# path (`.agentic` at its start or after `/`) outside am_domain::RuntimeLayout.
runtime-paths:
    cargo xtask runtime-paths

# AD-12 (story 2.2): every fixed host in shipped source (crates/*/src, tests
# left out) is on the scan's one named list, with its reason: no telemetry.
fixed-hosts:
    cargo xtask fixed-hosts

# Story 2.2, decision 7: shipped code never names am-net's test-only
# constructor NetEgress::with_seams; story 3.2: nor am-server's test-only
# fixture route (router_with_fixture, FixtureRoute); story 3.3: nor
# am-engine's tool seam (run_plan_with_tools).
seam-guard:
    cargo xtask seam-guard

# AD-16, AD-33: `agentic validate --template` on every valid fixture project
# and every shipped template (the Blank department, the role templates and
# the sample company; each must exit 0; story 3.6: the sample and the
# rows-documents fixture with their one fan-out warning each): reference files are checked against
# each folder's runtime-seed/, and a `project_id` is refused (story 1.13). The
# broken fixtures are asserted by the am-config and cli_validate tests instead.
validate:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -q -p am-app --bin agentic
    for project in fixtures/projects/valid*/ fixtures/projects/anatomy/ fixtures/projects/one-step/ fixtures/projects/hand-offs/ fixtures/projects/prompt-parts/ fixtures/projects/rows-documents/ templates/departments/*/ templates/roles/*/ templates/samples/*/; do
        "$CARGO_TARGET_DIR/debug/agentic" validate --template "$project"
    done

# AD-3 licence allow-list plus bans and sources on both graphs (macOS: whole
# workspace; Linux: without am-desktop), then cargo-audit (fetches the advisory
# database; vulnerabilities fail, unmaintained warnings do not). Last, xtask's
# cargo-deny tests (the GPL probe), ignored in `just check`.
supply-chain:
    cargo deny --locked --target aarch64-apple-darwin --workspace check --hide-inclusion-graph licenses bans sources
    cargo deny --locked --target x86_64-unknown-linux-gnu --workspace --exclude am-desktop check --hide-inclusion-graph licenses bans sources
    cargo audit
    cargo nextest run -p xtask --run-ignored only

# Enable the tracked git hooks (.githooks/pre-commit runs `just check`) in this
# clone. The Claude Code hook runs before a command, so a command that edits
# and commits is judged on the pre-edit tree; this pre-commit hook closes that
# gap at commit time.
install-hooks:
    git config core.hooksPath .githooks

# The spike's offline criteria tests (C1-C7).
spike:
    cargo nextest run -p am-e2e spike

# Live spike runs on this Mac (Anthropic key in the Keychain, LM Studio,
# Node.js, AM_SPIKE_DOCS_DIR); each test skips, in words, when its input is absent.
spike-live:
    AM_LIVE=1 cargo nextest run -p am-e2e spike --run-ignored only --no-capture

build:
    cargo build

# The desktop shell (CI and E11 only): lint it and build the ad-hoc-signed .app.
desktop:
    cargo clippy -p am-desktop --all-targets -- -D warnings
    cd crates/am-desktop && npx -y @tauri-apps/cli@2.12.5 build --debug --bundles app
