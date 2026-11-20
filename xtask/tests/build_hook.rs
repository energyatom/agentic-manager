//! The build hook refuses a commit while `just check` is red (story 1.2,
//! written first). A stub `just` on PATH plays the red or green check.
#![forbid(unsafe_code)]

mod common;

use common::{repo_root, stderr, write};
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const CLAUDE_HOOK: &str = ".claude/hooks/check-before-commit.sh";
const GIT_HOOK: &str = ".githooks/pre-commit";

/// A temp bin folder whose `just` exits with `code` and records its args
/// (`calls`) and its working directory (`cwd`).
fn stub_just(code: i32) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "just",
        &format!(
            "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/calls\"\npwd -P > \"$(dirname \"$0\")/cwd\"\necho 'stub just: check result {code}' >&2\nexit {code}\n"
        ),
    );
    let status = Command::new("chmod")
        .arg("+x")
        .arg(dir.path().join("just"))
        .status()
        .unwrap();
    assert!(status.success());
    dir
}

fn path_with(bin: &Path) -> String {
    format!("{}:/usr/bin:/bin", bin.display())
}

fn calls(bin: &Path) -> String {
    std::fs::read_to_string(bin.join("calls")).unwrap_or_default()
}

fn run_claude_hook(bin: &Path, command: &str) -> Output {
    let payload = serde_json::json!({
        "session_id": "s",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command, "description": "x" },
    });
    run_claude_hook_raw(bin, &payload.to_string())
}

fn run_claude_hook_raw(bin: &Path, payload: &str) -> Output {
    let mut child = Command::new(repo_root().join(CLAUDE_HOOK))
        .env("PATH", path_with(bin))
        .env("CLAUDE_PROJECT_DIR", repo_root())
        .current_dir(repo_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the Claude hook script exists and is executable");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn claude_hook_blocks_git_commit_while_check_is_red() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git add -A && git commit -m \"story\"");
    assert_eq!(out.status.code(), Some(2), "exit 2 blocks the tool call");
    assert!(
        stderr(&out).contains("just check"),
        "says why: {}",
        stderr(&out)
    );
    assert!(calls(bin.path()).contains("check"), "ran just check");
}

#[test]
fn claude_hook_blocks_git_c_commit_while_check_is_red() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git -C /some/worktree commit -m x");
    assert_eq!(out.status.code(), Some(2), "exit 2 blocks the tool call");
}

#[test]
fn claude_hook_allows_git_commit_when_check_is_green() {
    let bin = stub_just(0);
    let out = run_claude_hook(bin.path(), "git commit -m x");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(calls(bin.path()).contains("check"), "ran just check");
}

#[test]
fn claude_hook_blocks_commit_after_an_escaped_newline() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git add -A\ngit commit -m x");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn claude_hook_blocks_commit_after_a_tab() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git\tcommit -m x");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn claude_hook_blocks_git_c_with_a_quoted_path_with_spaces() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git -C \"/a/[17] b c\" commit -m x");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn claude_hook_blocks_an_unreadable_payload() {
    let bin = stub_just(0);
    let out = run_claude_hook_raw(bin.path(), "not json {");
    assert_eq!(out.status.code(), Some(2), "unreadable payload blocks");
    assert!(
        stderr(&out).contains("not readable JSON"),
        "{}",
        stderr(&out)
    );
    assert!(calls(bin.path()).is_empty(), "did not run just check");
}

#[test]
fn claude_hook_blocks_a_payload_without_tool_input_text() {
    let bin = stub_just(0);
    let out = run_claude_hook_raw(bin.path(), "{\"tool_name\":\"Bash\",\"tool_input\":{}}");
    assert_eq!(out.status.code(), Some(2), "no tool_input text blocks");
    assert!(
        stderr(&out).contains("no tool_input text"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn claude_hook_runs_the_check_in_the_payload_cwd() {
    let bin = stub_just(0);
    let tree = tempfile::tempdir().unwrap();
    let payload = serde_json::json!({
        "cwd": tree.path(),
        "tool_name": "Bash",
        "tool_input": { "command": "git commit -m x" },
    });
    let out = run_claude_hook_raw(bin.path(), &payload.to_string());
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let ran_in = std::fs::read_to_string(bin.path().join("cwd")).unwrap();
    assert_eq!(
        Path::new(ran_in.trim()),
        tree.path().canonicalize().unwrap(),
        "just check ran in the payload's cwd"
    );
}

#[test]
fn claude_hook_ignores_other_commands() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git status && cargo check");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(calls(bin.path()).is_empty(), "did not run just check");
}

#[test]
fn claude_settings_register_the_hook_for_bash_and_the_terminal() {
    let text = std::fs::read_to_string(repo_root().join(".claude/settings.json"))
        .expect(".claude/settings.json exists");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let entries = v["hooks"]["PreToolUse"]
        .as_array()
        .expect("PreToolUse hooks");
    let found = entries.iter().any(|e| {
        e["matcher"] == "Bash|mcp__terminal__run_in_terminal"
            && e["hooks"].as_array().is_some_and(|hs| {
                hs.iter().any(|h| {
                    h["command"]
                        .as_str()
                        .is_some_and(|c| c.contains("check-before-commit.sh"))
                })
            })
    });
    assert!(
        found,
        "the Bash and terminal PreToolUse hook runs {CLAUDE_HOOK}: {text}"
    );
}

fn run_git_hook(bin: &Path) -> Output {
    Command::new(repo_root().join(GIT_HOOK))
        .env("PATH", path_with(bin))
        .current_dir(repo_root())
        .output()
        .expect("the git pre-commit hook exists and is executable")
}

#[test]
fn git_pre_commit_refuses_when_check_is_red() {
    let bin = stub_just(1);
    let out = run_git_hook(bin.path());
    assert!(!out.status.success(), "red check refuses the commit");
    assert!(calls(bin.path()).contains("check"), "ran just check");
}

#[test]
fn git_pre_commit_allows_when_check_is_green() {
    let bin = stub_just(0);
    let out = run_git_hook(bin.path());
    assert!(out.status.success(), "{}", stderr(&out));
}
