#!/bin/sh
# Claude Code PreToolUse hook (story 1.2 build hook, R7): blocks an agent's
# `git commit` while `just check` is red. Registered for the Bash tool in
# .claude/settings.json. Reads the hook payload (JSON) on stdin; exit 2 blocks
# the tool call and shows stderr to the agent; exit 0 lets it run.
input=$(cat)

# The Bash tool's command string (JSON-escaped; good enough to spot git commit).
cmd=$(printf '%s' "$input" | tr '\n' ' ' |
    sed -nE 's/.*"command"[[:space:]]*:[[:space:]]*"(([^"\\]|\\.)*)".*/\1/p')

# `git [global options] commit`, anywhere in a compound command.
if ! printf '%s\n' "$cmd" | grep -Eq '(^|[^[:alnum:]_.-])git([[:space:]]+-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?)*[[:space:]]+commit([^[:alnum:]_-]|$)'; then
    exit 0
fi

root=${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}
if ! cd "$root"; then
    echo "Blocked: cannot enter the project folder to run 'just check'." >&2
    exit 2
fi
if ! command -v just >/dev/null 2>&1; then
    echo "Blocked: 'just' is not installed, so 'just check' cannot prove this commit green." >&2
    exit 2
fi
if out=$(just check 2>&1); then
    exit 0
fi
printf '%s\n' "$out" | tail -n 40 >&2
echo "Blocked: 'just check' is red. Fix it before committing (story 1.2 build hook)." >&2
exit 2
