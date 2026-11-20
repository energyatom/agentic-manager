#!/bin/sh
# Claude Code PreToolUse hook (story 1.2 build hook, R7): blocks an agent's
# `git commit` while `just check` is red. Registered in .claude/settings.json
# for the Bash and terminal tools; the logic is check_before_commit.py beside
# this script (JSON payload on stdin; exit 2 blocks, exit 0 allows).
#
# It runs before the command, so one command that both edits files and commits
# is judged on the pre-edit tree. The git pre-commit hook (.githooks/pre-commit,
# enabled with `just install-hooks`) closes that gap at commit time.
if ! command -v python3 >/dev/null 2>&1; then
    echo "Blocked: python3 is not available, so this command cannot be checked for a git commit." >&2
    exit 2
fi
exec python3 -I "$(dirname "$0")/check_before_commit.py"
