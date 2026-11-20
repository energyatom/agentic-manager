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

/// A temp bin folder whose `just` exits with `code` and records its args.
fn stub_just(code: i32) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "just",
        &format!(
            "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/calls\"\necho 'stub just: check result {code}' >&2\nexit {code}\n"
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
    let payload = format!(
        "{{\"session_id\":\"s\",\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":{},\"description\":\"x\"}}}}",
        serde_json::Value::String(command.to_string())
    );
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
fn claude_hook_ignores_other_commands() {
    let bin = stub_just(1);
    let out = run_claude_hook(bin.path(), "git log --grep commit && cargo check");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(calls(bin.path()).is_empty(), "did not run just check");
}

#[test]
fn claude_settings_register_the_hook_for_bash() {
    let text = std::fs::read_to_string(repo_root().join(".claude/settings.json"))
        .expect(".claude/settings.json exists");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let entries = v["hooks"]["PreToolUse"]
        .as_array()
        .expect("PreToolUse hooks");
    let found = entries.iter().any(|e| {
        e["matcher"] == "Bash"
            && e["hooks"].as_array().is_some_and(|hs| {
                hs.iter().any(|h| {
                    h["command"]
                        .as_str()
                        .is_some_and(|c| c.contains("check-before-commit.sh"))
                })
            })
    });
    assert!(found, "the Bash PreToolUse hook runs {CLAUDE_HOOK}: {text}");
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
