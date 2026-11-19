# Agentic Manager task runner. `just check` is the gate (spike minimal form:
# fmt, clippy -D warnings, nextest; story 1.2 adds `agentic validate` over
# templates/ and fixtures/ and the other CI checks).

# Build output stays outside the synced project folder; share it across worktrees.
export CARGO_TARGET_DIR := env_var_or_default("CARGO_TARGET_DIR", env_var("HOME") + "/Library/Caches/agentic-manager/target")

default: check

# The story gate: format, lints and every offline test.
check: fmt-check clippy test

fmt-check:
    cargo fmt --all -- --check

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo nextest run --workspace

# The spike's offline criteria tests (C1-C7).
spike:
    cargo nextest run -p am-e2e spike

# Live spike runs on this Mac (Anthropic key in the Keychain, LM Studio,
# Node.js, AM_SPIKE_DOCS_DIR); each test skips, in words, when its input is absent.
spike-live:
    AM_LIVE=1 cargo nextest run -p am-e2e spike --run-ignored only --no-capture

build:
    cargo build --workspace
