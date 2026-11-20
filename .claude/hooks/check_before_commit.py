# Claude Code PreToolUse hook logic (story 1.2 build hook, R7); run by
# check-before-commit.sh with `python3 -I`. Reads the hook payload (JSON) on
# stdin. Exit 2 blocks the tool call and shows stderr to the agent; exit 0 lets
# it run. Over-matching fails closed: any tool_input string holding the words
# `git` and `commit` is gated on a green `just check`.
import json
import os
import re
import shlex
import subprocess
import sys


def block(reason):
    sys.stderr.write("Blocked: " + reason + "\n")
    sys.exit(2)


try:
    payload = json.loads(sys.stdin.read())
except Exception:
    block("the hook payload is not readable JSON, so this command cannot be checked for a git commit.")
if not isinstance(payload, dict):
    block("the hook payload is not a JSON object, so this command cannot be checked for a git commit.")

strings = []


def collect(value):
    if isinstance(value, str):
        strings.append(value)
    elif isinstance(value, dict):
        for v in value.values():
            collect(v)
    elif isinstance(value, list):
        for v in value:
            collect(v)


collect(payload.get("tool_input"))
if not strings:
    block("the hook payload has no tool_input text, so this command cannot be checked for a git commit.")

if not any(re.search(r"\bgit\b", s) and re.search(r"\bcommit\b", s) for s in strings):
    sys.exit(0)

cwd = payload.get("cwd")
if not isinstance(cwd, str) or not cwd:
    cwd = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
target = cwd

# `git -C <path> ...`: check the tree that is being committed.
for s in strings:
    try:
        tokens = shlex.split(s)
    except ValueError:
        tokens = s.split()
    for i, tok in enumerate(tokens):
        if os.path.basename(tok) != "git":
            continue
        j = i + 1
        while j < len(tokens) and tokens[j].startswith("-"):
            if tokens[j] == "-C" and j + 1 < len(tokens):
                path = os.path.join(cwd, tokens[j + 1])
                try:
                    r = subprocess.run(
                        ["git", "-C", path, "rev-parse", "--show-toplevel"],
                        stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL,
                        universal_newlines=True,
                    )
                    if r.returncode == 0 and r.stdout.strip():
                        target = r.stdout.strip()
                except OSError:
                    pass
                j += 2
                continue
            j += 1

if not os.path.isdir(target):
    block("cannot enter " + target + " to run 'just check'.")
try:
    r = subprocess.run(
        ["just", "check"],
        cwd=target,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        universal_newlines=True,
    )
except OSError:
    block("'just' is not installed, so 'just check' cannot prove this commit green.")
if r.returncode == 0:
    sys.exit(0)
sys.stderr.write("\n".join(r.stdout.splitlines()[-40:]) + "\n")
block("'just check' is red in " + target + ". Fix it before committing (story 1.2 build hook).")
