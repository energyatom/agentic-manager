# Agentic Manager task runner. `just check` is the gate: fmt, clippy
# -D warnings, nextest and the offline repo checks (AD-4 dependency direction,
# every crate forbids unsafe, AD-23 vocabulary denylist). `agentic validate`
# over templates/ and fixtures/ joins it in story 1.4. `just supply-chain` runs
# the licence, bans and sources policy (cargo-deny) and cargo-audit.

# Build output stays outside the synced project folder; share it across worktrees.
export CARGO_TARGET_DIR := env_var_or_default("CARGO_TARGET_DIR", env_var("HOME") + "/Library/Caches/agentic-manager/target")

default: check

# The story gate: format, lints, every offline test and the repo checks.
check: fmt-check clippy test check-deps check-forbid denylist

fmt-check:
    cargo fmt --all -- --check

fmt:
    cargo fmt --all

# Default members: every crate but am-desktop (built only in E11 and CI).
clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo nextest run

# AD-4: crate edges follow the Design Paradigm matrix (cargo metadata, offline).
check-deps:
    cargo xtask check-deps

# Every crate inherits the workspace lints and forbids unsafe code.
check-forbid:
    cargo xtask check-forbid

# AD-23: no department vocabulary in crates/ or web/src.
denylist:
    cargo xtask denylist

# AD-3 licence allow-list plus bans and sources on both graphs (macOS: whole
# workspace; Linux: without am-desktop), then cargo-audit (fetches the advisory
# database; vulnerabilities fail, unmaintained warnings do not).
supply-chain:
    cargo deny --target aarch64-apple-darwin --workspace check --hide-inclusion-graph licenses bans sources
    cargo deny --target x86_64-unknown-linux-gnu --workspace --exclude am-desktop check --hide-inclusion-graph licenses bans sources
    cargo audit

# Enable the tracked git hooks (.githooks/pre-commit runs `just check`) in this clone.
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
