//! Story 1.6: `agentic serve --project <path>` as the owner runs it. Each
//! test runs the real `agentic` binary (built here with `cargo build -p am-app
//! --bin agentic` into this test's target directory) on a temporary project
//! and checks its output, its exit code and the files and modes on disk.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use am_store::SCHEMA_VERSION;

const WAIT: Duration = Duration::from_secs(60);

/// The `agentic` binary, built once per test process with this build's
/// cargo and environment; its path comes from cargo's own artifact report.
fn agentic_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = Command::new(env!("CARGO"))
            .args([
                "build",
                "-q",
                "-p",
                "am-app",
                "--bin",
                "agentic",
                "--message-format=json",
                "--offline",
                "--locked",
            ])
            .current_dir(&root)
            .stderr(Stdio::inherit())
            .output()
            .unwrap();
        assert!(out.status.success(), "building agentic failed");
        let bin = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "agentic")
            .find_map(|m| m["executable"].as_str().map(PathBuf::from))
            .expect("cargo reported no agentic executable");
        assert!(bin.exists(), "{} missing", bin.display());
        bin
    })
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn agentic_dir(root: &Path) -> PathBuf {
    root.join(".agentic")
}

fn existing_agentic(root: &Path) -> PathBuf {
    let dir = agentic_dir(root);
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn serve_cmd(project: &Path) -> Command {
    let mut cmd = Command::new(agentic_bin());
    cmd.args(["serve", "--no-browser", "--port", "0", "--project"])
        .arg(project);
    cmd
}

/// A running `agentic serve`; killed (SIGKILL) when dropped.
struct Serve {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Serve {
    fn start(project: &Path) -> Serve {
        let mut child = serve_cmd(project)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, rx) = channel();
        for stream in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Serve {
            child,
            lines: rx,
            seen: Vec::new(),
        }
    }

    /// Waits for the "serving on" line; returns the port.
    fn wait_serving(&mut self) -> u16 {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let port = line
                        .split("http://127.0.0.1:")
                        .nth(1)
                        .map(|p| p.trim().parse::<u16>().unwrap());
                    self.seen.push(line);
                    if let Some(port) = port {
                        return port;
                    }
                }
                Err(_) => panic!("serve never started; output so far: {:?}", self.seen),
            }
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Runs serve expecting it to exit by itself; returns (status, stdout, stderr).
fn serve_to_exit(project: &Path) -> (ExitStatus, String, String) {
    let mut child = serve_cmd(project)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(_status) = child.try_wait().unwrap() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "serve did not exit: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status,
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn backups(root: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(agentic_dir(root).join("backups")) {
        Ok(rd) => rd.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn serve_on_a_new_project_creates_the_folder_and_prints_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut serve = Serve::start(root);
    serve.wait_serving();
    let created = format!("created at version {SCHEMA_VERSION}");
    assert!(
        serve.seen.iter().any(|l| l.contains(&created)),
        "{:?}",
        serve.seen
    );
    let ag = agentic_dir(root);
    assert_eq!(mode(&ag), 0o700);
    assert_eq!(
        std::fs::read_to_string(ag.join(".gitignore"))
            .unwrap()
            .trim(),
        "*"
    );
    assert_eq!(mode(&ag.join("state.db")), 0o600);
    assert_eq!(mode(&ag.join("lock")), 0o600);
    assert!(backups(root).is_empty());
}

#[test]
fn serve_migrates_an_older_database_after_a_backup_and_prints_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let db = ag.join("state.db");
    let old = SCHEMA_VERSION - 1;
    am_store::testing::database_at_version(&db, old).unwrap();
    am_store::testing::insert_fixture_trigger(&db, "fixture-row").unwrap();

    let mut serve = Serve::start(root);
    serve.wait_serving();
    let found = backups(root);
    assert_eq!(found.len(), 1, "{found:?}");
    let backup = &found[0];
    assert_eq!(mode(backup), 0o600);
    let want = format!(
        "migrated from version {old} to {SCHEMA_VERSION}, backup: {}",
        backup.display()
    );
    assert!(
        serve.seen.iter().any(|l| l.contains(&want)),
        "`{want}` missing from {:?}",
        serve.seen
    );
    serve.kill();

    let reader = am_store::Reader::open(&db).unwrap();
    assert!(reader.trigger("fixture-row").unwrap().is_some());
}

#[test]
fn serve_refuses_a_newer_database_and_exits_non_zero() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = existing_agentic(root);
    let db = ag.join("state.db");
    let newer = SCHEMA_VERSION + 1;
    am_store::testing::database_at_version(&db, newer).unwrap();
    let before = std::fs::read(&db).unwrap();

    let (status, out, err) = serve_to_exit(root);
    assert!(!status.success(), "{out}{err}");
    let want = format!(
        "the database is version {newer}, newer than this app knows ({SCHEMA_VERSION}); update the app"
    );
    assert!(err.contains(&want), "{err}");
    assert!(!out.contains("serving on"), "{out}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert!(backups(root).is_empty());
}

#[test]
fn a_second_serve_on_the_same_project_is_refused_naming_the_holder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut first = Serve::start(root);
    let port = first.wait_serving();
    let lock = std::fs::read_to_string(agentic_dir(root).join("lock")).unwrap();
    let field = |key: &str| {
        lock.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}: ")))
            .unwrap_or_else(|| panic!("no {key} in lock file:\n{lock}"))
            .to_string()
    };

    let (status, out, err) = serve_to_exit(root);
    assert!(!status.success(), "{out}{err}");
    assert!(!out.contains("serving on"), "{out}");
    for part in [
        format!("pid {}", first.pid()),
        field("host"),
        field("command"),
        field("started"),
    ] {
        assert!(err.contains(&part), "`{part}` missing from: {err}");
    }
    assert!(field("command").contains("serve"), "{lock}");

    // The first process is unaffected: still running and still serving.
    assert!(first.child.try_wait().unwrap().is_none());
    std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    assert_eq!(field("pid"), first.pid().to_string());
}

#[test]
fn a_stale_lock_left_by_a_killed_serve_does_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let lock = agentic_dir(root).join("lock");
    let mut first = Serve::start(root);
    first.wait_serving();
    let killed = first.pid();
    first.kill();
    assert!(
        std::fs::read_to_string(&lock)
            .unwrap()
            .contains(&format!("pid: {killed}"))
    );

    let mut second = Serve::start(root);
    second.wait_serving();
    let text = std::fs::read_to_string(&lock).unwrap();
    assert!(text.contains(&format!("pid: {}", second.pid())), "{text}");
}

#[test]
fn serve_refuses_a_folder_with_loose_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let ag = agentic_dir(root);
    std::fs::create_dir(&ag).unwrap();
    std::fs::set_permissions(&ag, std::fs::Permissions::from_mode(0o755)).unwrap();

    let (status, out, err) = serve_to_exit(root);
    assert!(!status.success(), "{out}{err}");
    assert!(
        err.contains(&format!("chmod 700 {}", ag.display())),
        "{err}"
    );
    assert_eq!(std::fs::read_dir(&ag).unwrap().count(), 0);
}

#[test]
fn serve_without_a_project_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(agentic_bin())
        .args(["serve", "--no-browser", "--port", "0"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _ = child.kill();
    let _ = child.wait();
    assert!(line.contains("serving on http://127.0.0.1:"), "{line}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn serve_with_a_missing_project_folder_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["serve", "--no-browser", "--port", "0", "--project"][..],
        &["serve", "--project", "--no-browser", "--port", "0"][..],
    ] {
        let out = Command::new(agentic_bin())
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {err}");
        assert!(err.contains("--project needs the project folder"), "{err}");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("serving on"),
            "{args:?}"
        );
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
