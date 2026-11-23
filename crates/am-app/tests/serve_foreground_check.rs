//! Story 3.19, row 7 of the plan's I/O matrix: the foreground check fails
//! closed. `agentic serve` reads stdin for Enter only while it is the
//! terminal's foreground job, and asks the system's `ps` for its own
//! process (`/bin/ps -o stat= -p <pid>`: a `+` in the state means "in the
//! foreground process group of its controlling terminal"), as a child
//! process with a short timeout. Any failure (no `ps`, no answer in time,
//! a non-zero exit, unexpected output) counts as "not foreground": never
//! read. Written first, against the am-app API this story adds in
//! `am_app::serving`:
//!
//! - `serve_is_foreground() -> bool`: this process, through `/bin/ps`, on
//!   macOS; `false` on every other platform (AD-47).
//! - `serve_is_foreground_with(ps: &Path, pid: u32, timeout: Duration) ->
//!   bool`: the same check with the `ps` program, the pid and the timeout
//!   given (the seam these tests drive with fake `ps` scripts); a `ps` that
//!   does not answer within `timeout` is ended (killed and reaped) and the
//!   answer is `false`.
//! - `foreground_from_ps_stat(stat: &str) -> bool`: the parse of `ps -o
//!   stat=` output: `true` only for one line holding one word that starts
//!   with an uppercase letter (the process state) and contains `+`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use am_app::serving::{foreground_from_ps_stat, serve_is_foreground, serve_is_foreground_with};

/// A fake `ps`: a `/bin/sh` script with `body`, executable.
fn fake_ps(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

const SHORT: Duration = Duration::from_secs(5);

/// `ps -o stat=` as macOS prints it (padded, newline-ended): a `+` in the
/// state is the foreground.
#[test]
fn a_plus_in_the_process_state_is_the_foreground() {
    for stat in ["S+", "Ss+", "R+", "R+  \n", "  S+\n", "S+\n", "Ss+   \n"] {
        assert!(foreground_from_ps_stat(stat), "{stat:?}");
    }
}

/// No `+`, nothing, or anything that is not one state word: not the
/// foreground.
#[test]
fn no_plus_or_unexpected_output_is_not_the_foreground() {
    for stat in [
        "",
        "\n",
        "   \n",
        "S",
        "Ss",
        "S   \n",
        "T",
        "TN",
        "Ss \n",
        "+",
        "+\n",
        "s+",
        "S+ S+",
        "S+\nS+\n",
        "STAT\nS+\n",
        "ps: illegal option -- o",
        "ps: no such process",
        "garbage",
    ] {
        assert!(!foreground_from_ps_stat(stat), "{stat:?}");
    }
}

/// The check runs the given `ps` with `-o stat= -p <pid>` and reads its
/// answer: the fake answers `S+` only for exactly those arguments.
#[cfg(target_os = "macos")]
#[test]
fn the_check_asks_ps_for_the_state_of_that_pid() {
    let dir = tempfile::tempdir().unwrap();
    let ps = fake_ps(
        dir.path(),
        "ps",
        "if [ \"$*\" = \"-o stat= -p 4242\" ]; then echo 'S+  '; else echo \"unexpected: $*\" >&2; exit 2; fi",
    );
    assert!(serve_is_foreground_with(&ps, 4242, SHORT));
    assert!(!serve_is_foreground_with(&ps, 4243, SHORT));
    let background = fake_ps(dir.path(), "ps-bg", "echo 'S   '");
    assert!(!serve_is_foreground_with(&background, 4242, SHORT));
}

/// No `ps` at that path: not the foreground.
#[test]
fn a_missing_ps_is_not_the_foreground() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!serve_is_foreground_with(
        &dir.path().join("no-such-ps"),
        std::process::id(),
        SHORT
    ));
    assert!(!serve_is_foreground_with(
        Path::new("/nonexistent/bin/ps"),
        std::process::id(),
        SHORT
    ));
}

/// A `ps` that does not answer within the timeout: not the foreground, the
/// check returns soon after the timeout, and that `ps` is ended (killed
/// and reaped), not left running or as a zombie.
#[test]
fn a_ps_that_does_not_answer_in_time_is_not_the_foreground_and_is_ended() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("ps.pid");
    let ps = fake_ps(
        dir.path(),
        "ps",
        &format!("echo $$ > '{}'\nexec /bin/sleep 30", pid_file.display()),
    );
    let began = Instant::now();
    assert!(!serve_is_foreground_with(
        &ps,
        std::process::id(),
        Duration::from_millis(300)
    ));
    let took = began.elapsed();
    assert!(took < Duration::from_secs(5), "the check took {took:?}");

    // Where the fake ran (it wrote its pid), it is gone within 3 s.
    if let Ok(pid) = std::fs::read_to_string(&pid_file) {
        let pid = pid.trim().to_string();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let out = Command::new("/bin/ps")
                .args(["-o", "stat=", "-p", &pid])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if stat.is_empty() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the timed-out ps (pid {pid}) is still there (state {stat:?})"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A `ps` that exits non-zero (even after printing a `+`), prints a
/// header, several lines or something else: not the foreground.
#[test]
fn a_ps_that_fails_or_prints_something_unexpected_is_not_the_foreground() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("fails", "echo 'S+'\nexit 1"),
        ("silent", "exit 0"),
        ("words", "echo 'hello world'"),
        ("header", "echo 'STAT'\necho 'S+'"),
        ("twice", "echo 'S+'\necho 'S+'"),
        ("error", "echo 'ps: illegal option -- o' >&2\nexit 1"),
    ] {
        let ps = fake_ps(dir.path(), name, body);
        assert!(
            !serve_is_foreground_with(&ps, std::process::id(), SHORT),
            "{name}: {body:?}"
        );
    }
}

/// `serve_is_foreground()` reports this process's own state as `/bin/ps`
/// shows it, on macOS; elsewhere it is never the foreground.
#[test]
fn serve_is_foreground_reports_this_process_s_own_state() {
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &std::process::id().to_string()])
            .output()
            .unwrap();
        let want = foreground_from_ps_stat(&String::from_utf8_lossy(&out.stdout));
        assert_eq!(serve_is_foreground(), want);
    }
    #[cfg(not(target_os = "macos"))]
    assert!(!serve_is_foreground());
}

/// Review round 1, P5: `ps` runs with an empty environment and `/` as its
/// working folder, so nothing of serve's environment (HOME, PATH, the
/// project) or working folder reaches it. The fake `ps` (a `/bin/sh`
/// script) records both and answers `S+`. Seen on macOS with `env -i
/// /bin/sh` from `/`: the only variables are the three the shell itself
/// sets for its children, `PWD=/`, `SHLVL=1` and `_=/usr/bin/env`.
#[cfg(target_os = "macos")]
#[test]
fn ps_runs_with_an_empty_environment_in_the_root_folder() {
    let dir = tempfile::tempdir().unwrap();
    let env_file = dir.path().join("env.txt");
    let cwd_file = dir.path().join("cwd.txt");
    let ps = fake_ps(
        dir.path(),
        "ps",
        &format!(
            "/usr/bin/env > '{}'\npwd -P > '{}'\necho 'S+'",
            env_file.display(),
            cwd_file.display()
        ),
    );
    assert!(serve_is_foreground_with(&ps, std::process::id(), SHORT));
    let env = std::fs::read_to_string(&env_file).unwrap();
    let names: Vec<&str> = env
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k))
        .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .collect();
    let added_by_the_shell = ["PWD", "SHLVL", "_"];
    let inherited: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| !added_by_the_shell.contains(n))
        .collect();
    let cwd = std::fs::read_to_string(&cwd_file)
        .unwrap()
        .trim()
        .to_string();
    // Names only in the messages: the values may hold secrets.
    assert!(
        inherited.is_empty() && cwd == "/",
        "ps ran in {cwd:?} (want /) and inherited {inherited:?} (want nothing)"
    );
    assert!(
        env.lines().any(|l| l == "PWD=/"),
        "the shell's PWD is not /; variables: {names:?}"
    );
}
