//! Stories 1.6 and 1.13: `agentic serve --project <path>` as the owner runs
//! it. Each test runs the real `agentic` binary (`CARGO_BIN_EXE_agentic`)
//! with HOME (and XDG_DATA_HOME) set to a temporary folder, so the runtime
//! base is temporary, over the platform's real identity adapter; it starts
//! from a stream copy of `fixtures/projects/valid` or a minimal project, and
//! checks output, exit codes, and the files and modes on disk. A clean stop
//! is SIGTERM to the pid in the project's marker. The rows that need the
//! identity fake or a fail point are in `runtime_open.rs`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use am_app::runtime::Marker;
use am_domain::runtime::{OPEN_MARKER, RUNTIME_BASE_NAME, legacy_dir, open_marker};
use am_domain::{MacId, ProjectId, RuntimeBase, RuntimeLayout};
use am_store::SCHEMA_VERSION;

const WAIT: Duration = Duration::from_secs(60);
const COMPANY: &str = "schema_version: 1\nname: Example\ntimezone: UTC\ncurrency: EUR\n";

fn agentic_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_agentic"))
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// A temporary HOME and a folder for projects.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("home")).unwrap();
        Env { dir }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// The runtime base the binary resolves for this HOME.
    fn base(&self) -> RuntimeBase {
        let support = if cfg!(target_os = "macos") {
            self.home().join("Library/Application Support")
        } else {
            self.dir.path().join("xdg")
        };
        RuntimeBase::new(support.join(RUNTIME_BASE_NAME))
    }

    fn app_data(&self) -> PathBuf {
        self.base().dir().parent().unwrap().join("agentic-manager")
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(agentic_bin());
        c.args(args)
            .env("HOME", self.home())
            .env("XDG_DATA_HOME", self.dir.path().join("xdg"));
        c
    }

    /// A project folder `name` holding `company.yaml` only.
    fn project(&self, name: &str, company: &str) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("company.yaml"), company).unwrap();
        root.canonicalize().unwrap()
    }

    /// A stream copy of `fixtures/projects/valid`.
    fn valid(&self, name: &str) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        am_testkit::copy_tree(&am_testkit::fixture("projects/valid"), &root);
        root.canonicalize().unwrap()
    }

    fn layout(&self, root: &Path) -> RuntimeLayout {
        let id = id_of(root).expect("the project has an id");
        self.base().project(&id)
    }

    fn serve(&self, project: &Path, extra: &[&str]) -> Serve {
        let mut args = vec!["serve", "--no-browser", "--port", "0", "--project"];
        let p = project.to_str().unwrap();
        args.push(p);
        args.extend_from_slice(extra);
        Serve::start(self.cmd(&args))
    }

    /// Runs serve expecting it to exit by itself: (status, stdout, stderr).
    fn serve_to_exit(&self, project: &Path, extra: &[&str]) -> (ExitStatus, String, String) {
        let mut args = vec!["serve", "--no-browser", "--port", "0", "--project"];
        args.push(project.to_str().unwrap());
        args.extend_from_slice(extra);
        let child = self
            .cmd(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_exit(child)
    }

    /// Opens the project once and stops it cleanly; returns its id.
    fn open_once(&self, root: &Path) -> ProjectId {
        let mut s = self.serve(root, &[]);
        s.wait_serving();
        s.stop(root);
        id_of(root).unwrap()
    }
}

fn wait_exit(mut child: Child) -> (ExitStatus, String, String) {
    let deadline = Instant::now() + WAIT;
    while child.try_wait().unwrap().is_none() {
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

fn id_of(root: &Path) -> Option<ProjectId> {
    am_config::read_company(root).unwrap().project_id
}

fn marker(root: &Path) -> Marker {
    Marker::parse(&std::fs::read_to_string(open_marker(root)).unwrap()).unwrap()
}

fn kill(pid: u32, signal: &str) {
    let ok = Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok, "kill {signal} {pid}");
}

/// A running `agentic serve`; killed (SIGKILL) when dropped.
struct Serve {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Serve {
    fn start(mut cmd: Command) -> Serve {
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Serve::reading(child)
    }

    fn reading(mut child: Child) -> Serve {
        let (tx, rx) = channel();
        let streams: Vec<Box<dyn Read + Send>> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .collect();
        for stream in streams {
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

    /// A clean stop: SIGTERM to the pid in the project's marker; exits 0.
    fn stop(&mut self, root: &Path) {
        let pid = marker(root).pid;
        assert_eq!(pid, self.pid());
        kill(pid, "-TERM");
        let deadline = Instant::now() + WAIT;
        let status = loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                break s;
            }
            assert!(Instant::now() < deadline, "serve did not stop");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.success(), "{status:?}");
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

fn backups(layout: &RuntimeLayout) -> Vec<PathBuf> {
    match std::fs::read_dir(layout.backups_pre_migration()) {
        Ok(rd) => rd.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

// ------------------------------------------------------------ 1.13

#[test]
fn serve_on_a_new_project_gives_it_an_id_and_a_runtime_folder_outside_it() {
    let e = Env::new();
    let root = e.valid("p");
    let before = std::fs::read_to_string(root.join("company.yaml")).unwrap();
    let files_before = names(&root);
    let mut serve = e.serve(&root, &[]);
    serve.wait_serving();
    let created = format!("created at version {SCHEMA_VERSION}");
    assert!(
        serve.seen.iter().any(|l| l.contains(&created)),
        "{:?}",
        serve.seen
    );

    let id = id_of(&root).expect("an id was written");
    // After the schema_version line, whatever version the fixture is at.
    let version = before
        .lines()
        .find(|l| l.starts_with("schema_version:"))
        .unwrap()
        .to_string();
    assert_eq!(
        std::fs::read_to_string(root.join("company.yaml")).unwrap(),
        before.replacen(
            &format!("{version}\n"),
            &format!("{version}\nproject_id: {id}\n"),
            1
        ),
        "one line inserted, every other byte unchanged"
    );
    let layout = e.base().project(&id);
    assert_eq!(mode(layout.dir()), 0o700);
    for f in [layout.lock(), layout.state_db(), layout.root_record()] {
        assert_eq!(mode(&f), 0o600, "{}", f.display());
    }
    assert!(backups(&layout).is_empty());
    let mut want = files_before.clone();
    want.push(OPEN_MARKER.into());
    want.sort();
    assert_eq!(names(&root), want, "the project gains only the marker");
    let m = marker(&root);
    let mac = std::fs::read_to_string(e.app_data().join("mac-id")).unwrap();
    assert_eq!(m.mac_id, mac.trim(), "this Mac's random id, no hardware id");
    assert!(MacId::parse(&m.mac_id).is_some());
    assert_eq!(m.pid, serve.pid());

    // Story 2.2 review: the bound port is recorded, owner-only, in the
    // runtime folder (every egress client of the project refuses it), and
    // removed at a clean stop.
    let serving = serve
        .seen
        .iter()
        .find_map(|l| l.split("serving on http://127.0.0.1:").nth(1))
        .expect("the serving line")
        .trim()
        .to_string();
    assert_eq!(
        std::fs::read_to_string(layout.serve_port()).unwrap().trim(),
        serving
    );
    assert_eq!(mode(&layout.serve_port()), 0o600);

    let running = am_domain::runtime::serving_dir(&e.app_data()).join(serve.pid().to_string());
    assert_eq!(
        std::fs::read_to_string(&running).unwrap().trim(),
        serving,
        "the user-level record of running servers"
    );
    assert_eq!(mode(&running), 0o600);

    serve.stop(&root);
    assert!(
        !layout.serve_port().exists(),
        "the port file is gone after a clean stop"
    );
    assert!(
        !running.exists(),
        "the user-level record is gone after a clean stop"
    );
    assert_eq!(
        names(&root),
        files_before,
        "the marker is gone after a clean stop"
    );
    // History and audit landed in the new store, by System(runtime).
    let reader = am_store::Reader::open(&layout.state_db()).unwrap();
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    let log = String::from_utf8(out).unwrap();
    let edited: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("\"action\":\"config.edited\""))
        .collect();
    assert_eq!(edited.len(), 1, "{log}");
    assert!(
        edited[0].contains(r#""actor":{"component":"runtime","kind":"system"}"#),
        "{}",
        edited[0]
    );
}

#[test]
fn an_empty_folder_is_refused_naming_the_wizard_and_nothing_is_created() {
    let e = Env::new();
    let root = e.dir.path().join("empty");
    std::fs::create_dir(&root).unwrap();
    let (status, out, err) = e.serve_to_exit(&root, &[]);
    assert!(!status.success(), "{out}{err}");
    assert!(err.contains("first-run wizard"), "{err}");
    assert!(!out.contains("serving on"), "{out}");
    assert!(!e.base().dir().exists(), "the base is not created");
    assert!(names(&root).is_empty());
}

#[test]
fn a_copy_is_refused_naming_both_folders_and_a_move_opens() {
    let e = Env::new();
    let a = e.project("a", COMPANY);
    let id = e.open_once(&a);
    let b = e.dir.path().join("projects/b");
    am_testkit::copy_tree(&a, &b);
    let b = b.canonicalize().unwrap();

    let (status, _, err) = e.serve_to_exit(&b, &[]);
    assert!(!status.success(), "{err}");
    assert!(err.contains(&a.display().to_string()), "{err}");
    assert!(err.contains(&b.display().to_string()), "{err}");
    assert!(err.contains("new project"), "{err}");
    assert!(!open_marker(&b).exists());

    // The original goes: the copy is now the project (a move).
    std::fs::remove_dir_all(&a).unwrap();
    let mut serve = e.serve(&b, &[]);
    serve.wait_serving();
    assert!(
        serve.seen.iter().any(|l| l.contains("moved")),
        "{:?}",
        serve.seen
    );
    assert_eq!(id_of(&b), Some(id));
    serve.stop(&b);
}

#[test]
fn serve_migrates_an_older_database_after_a_backup_and_prints_the_line() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    e.open_once(&root);
    let layout = e.layout(&root);
    let db = layout.state_db();
    std::fs::remove_file(&db).unwrap();
    let old = SCHEMA_VERSION - 1;
    am_store::testing::database_at_version(&db, old).unwrap();
    am_store::testing::insert_fixture_trigger(&db, "fixture-row").unwrap();

    let mut serve = e.serve(&root, &[]);
    serve.wait_serving();
    let found = backups(&layout);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(mode(&found[0]), 0o600);
    let want = format!(
        "migrated from version {old} to {SCHEMA_VERSION}, backup: {}",
        found[0].display()
    );
    assert!(
        serve.seen.iter().any(|l| l.contains(&want)),
        "`{want}` missing from {:?}",
        serve.seen
    );
    serve.stop(&root);
    let reader = am_store::Reader::open(&db).unwrap();
    assert!(reader.trigger("fixture-row").unwrap().is_some());
}

#[test]
fn serve_refuses_a_newer_database_and_exits_non_zero() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    e.open_once(&root);
    let db = e.layout(&root).state_db();
    std::fs::remove_file(&db).unwrap();
    let newer = SCHEMA_VERSION + 1;
    am_store::testing::database_at_version(&db, newer).unwrap();
    let before = std::fs::read(&db).unwrap();
    let (status, out, err) = e.serve_to_exit(&root, &[]);
    assert!(!status.success(), "{out}{err}");
    let want = format!(
        "the database is version {newer}, newer than this app knows ({SCHEMA_VERSION}); update the app"
    );
    assert!(err.contains(&want), "{err}");
    assert!(!out.contains("serving on"), "{out}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert!(!open_marker(&root).exists());
}

#[test]
fn a_second_serve_on_the_same_project_is_refused_naming_the_holder() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    let mut first = e.serve(&root, &[]);
    let port = first.wait_serving();
    let lock = std::fs::read_to_string(e.layout(&root).lock()).unwrap();
    let field = |key: &str| {
        lock.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}: ")))
            .unwrap_or_else(|| panic!("no {key} in lock file:\n{lock}"))
            .to_string()
    };
    let (status, out, err) = e.serve_to_exit(&root, &[]);
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
    assert_eq!(
        field("host"),
        marker(&root).mac_name,
        "the host is the port's Mac name"
    );
    assert!(first.child.try_wait().unwrap().is_none());
    std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    first.stop(&root);
}

#[test]
fn a_stale_lock_and_marker_left_by_a_killed_serve_do_not_block() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    let mut first = e.serve(&root, &[]);
    first.wait_serving();
    let killed = first.pid();
    first.kill();
    assert_eq!(marker(&root).pid, killed, "SIGKILL leaves the marker");
    let mut second = e.serve(&root, &[]);
    second.wait_serving();
    let lock = std::fs::read_to_string(e.layout(&root).lock()).unwrap();
    assert!(lock.contains(&format!("pid: {}", second.pid())), "{lock}");
    assert_eq!(
        marker(&root).pid,
        second.pid(),
        "a gone pid's marker is replaced"
    );
    second.stop(&root);
}

#[test]
fn serve_refuses_a_runtime_folder_with_loose_permissions() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    e.open_once(&root);
    let dir = e.layout(&root).dir().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (status, out, err) = e.serve_to_exit(&root, &[]);
    assert!(!status.success(), "{out}{err}");
    assert!(
        err.contains(&format!("chmod 700 {}", dir.display())),
        "{err}"
    );
    assert!(!open_marker(&root).exists());
}

#[test]
fn a_legacy_folder_with_rows_moves_out_and_the_report_names_the_kept_folder() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    let old = legacy_dir(&root);
    std::fs::create_dir(&old).unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
    let db = old.join("state.db");
    am_store::testing::database_at_version(&db, SCHEMA_VERSION - 1).unwrap();
    am_store::testing::insert_fixture_trigger(&db, "legacy-row").unwrap();
    std::fs::write(old.join(".gitignore"), "*\n").unwrap();

    let mut serve = e.serve(&root, &[]);
    serve.wait_serving();
    let layout = e.layout(&root);
    assert!(!old.exists(), "no legacy folder left in the project");
    let line = serve
        .seen
        .iter()
        .find(|l| l.contains("legacy-agentic-"))
        .unwrap_or_else(|| panic!("{:?}", serve.seen))
        .clone();
    assert!(line.contains(&layout.dir().display().to_string()), "{line}");
    assert!(line.contains("delete it by hand"), "{line}");
    serve.stop(&root);
    let reader = am_store::Reader::open(&layout.state_db()).unwrap();
    assert!(reader.trigger("legacy-row").unwrap().is_some());
}

/// Another Mac's marker refuses; `--take-over` without a terminal is
/// refused saying one is needed; on a terminal (under `script`) answered
/// Yes it opens.
#[test]
fn another_macs_marker_opens_only_after_take_over_on_a_terminal() {
    let e = Env::new();
    let root = e.project("p", COMPANY);
    e.open_once(&root);
    let other = format!(
        "mac_name: Studio Mac\nmac_id: {}\npid: 4242\nopened_at: 2026-11-20T10:00:00.000Z\n",
        MacId::mint()
    );
    std::fs::write(open_marker(&root), &other).unwrap();

    let (status, _, err) = e.serve_to_exit(&root, &[]);
    assert!(!status.success());
    assert!(err.contains("Studio Mac"), "{err}");
    let (status, _, err) = e.serve_to_exit(&root, &["--take-over"]);
    assert!(!status.success());
    assert!(err.contains("need a terminal"), "{err}");
    assert_eq!(std::fs::read_to_string(open_marker(&root)).unwrap(), other);

    // On a terminal: `script` gives the child a pty; the answer is typed.
    let line = format!(
        "{} serve --no-browser --port 0 --project {} --take-over",
        agentic_bin().display(),
        root.display()
    );
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/script");
        c.args(["-q", "/dev/null", "/bin/sh", "-c", &line]);
        c
    } else {
        let mut c = Command::new("script");
        c.args(["-qec", &line, "/dev/null"]);
        c
    };
    cmd.env("HOME", e.home())
        .env("XDG_DATA_HOME", e.dir.path().join("xdg"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut serve = Serve::reading(child);
    std::thread::sleep(Duration::from_millis(500));
    stdin.write_all(b"y\n").unwrap();
    stdin.flush().unwrap();
    serve.wait_serving();
    assert!(
        serve.seen.iter().any(|l| l.contains("Open it here anyway")),
        "{:?}",
        serve.seen
    );
    let m = marker(&root);
    assert_eq!(m.mac_name, marker_name_of_this_mac(&e));
    kill(m.pid, "-TERM");
    drop(stdin);
    let deadline = Instant::now() + WAIT;
    while open_marker(&root).exists() {
        assert!(Instant::now() < deadline, "the marker stayed");
        std::thread::sleep(Duration::from_millis(20));
    }
    let reader = am_store::Reader::open(&e.layout(&root).state_db()).unwrap();
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    let log = String::from_utf8(out).unwrap();
    assert!(log.contains("\"action\":\"project.taken_over\""), "{log}");
}

/// The Mac name this Mac's marker shows (from a fresh open's lock record).
fn marker_name_of_this_mac(e: &Env) -> String {
    let probe = e.project("probe", COMPANY);
    let mut s = e.serve(&probe, &[]);
    s.wait_serving();
    let name = marker(&probe).mac_name;
    s.stop(&probe);
    name
}

// ------------------------------------------------------------ 1.6, 1.8

#[test]
fn serve_without_a_project_is_unchanged() {
    let e = Env::new();
    let dir = e.dir.path().join("cwd");
    std::fs::create_dir(&dir).unwrap();
    let mut child = e
        .cmd(&["serve", "--no-browser", "--port", "0"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    // Story 2.2 review: even with no project, the bound port is recorded
    // in the user-level folder of running servers, under the process id.
    let port = line
        .split("serving on http://127.0.0.1:")
        .nth(1)
        .unwrap_or_default()
        .trim()
        .to_string();
    let recorded = am_domain::runtime::serving_dir(&e.app_data()).join(child.id().to_string());
    let seen = std::fs::read_to_string(&recorded);
    let recorded_mode = mode(&recorded);
    let _ = child.kill();
    let _ = child.wait();
    assert!(line.contains("serving on http://127.0.0.1:"), "{line}");
    assert_eq!(
        seen.unwrap_or_default().trim(),
        port,
        "{}",
        recorded.display()
    );
    assert_eq!(recorded_mode, 0o600);
    assert!(names(&dir).is_empty());
    assert!(!e.base().dir().exists());
}

#[test]
fn serve_with_a_missing_project_folder_is_a_usage_error() {
    let e = Env::new();
    for args in [
        &["serve", "--no-browser", "--port", "0", "--project"][..],
        &["serve", "--project", "--no-browser", "--port", "0"][..],
        &["serve", "--no-browser", "--take-over"][..],
    ] {
        let out = e.cmd(args).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {err}");
        assert!(err.contains("usage: agentic serve"), "{err}");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("serving on"),
            "{args:?}"
        );
    }
}

/// Story 1.8 (AD-16 startup rule): serve reconciles the config writer's
/// pending intents before it serves: one whose file holds its new bytes is
/// completed, one whose file does not is dropped, both audited by the system.
#[test]
fn serve_reconciles_interrupted_config_writes_at_start() {
    use am_domain::{Actor, ConfigHistory, HistoryKind, NewIntent};
    use sha2::{Digest, Sha256};

    let e = Env::new();
    let root = e.project("p", COMPANY);
    let id = e.open_once(&root);
    let company = std::fs::read(root.join("company.yaml")).unwrap();
    let layout = e.base().project(&id);
    let sha = |b: &[u8]| hex::encode(Sha256::digest(b));
    let new = [company.clone(), b"description: New\n".to_vec()].concat();
    // The file already holds the new bytes (a crash after the rename).
    std::fs::write(root.join("company.yaml"), &new).unwrap();
    let (done, dropped) = {
        let (store, _) = am_store::open_runtime(&layout, "seed".into()).unwrap();
        let history = am_store::StoreConfigHistory::new(&store).unwrap();
        let intent = |before: &[u8], after: &[u8]| NewIntent {
            file: "company.yaml".into(),
            kind: HistoryKind::Edit,
            ref_entry: None,
            before_sha256: sha(before),
            after_sha256: sha(after),
            before: before.to_vec(),
            after: after.to_vec(),
            diff: String::new(),
            audit: Vec::new(),
        };
        let owner = Actor::Human {
            person: "owner".into(),
            hat: "Owner".into(),
        };
        let done = history
            .begin_intent(owner.clone(), intent(&company, &new))
            .unwrap();
        let dropped = history
            .begin_intent(owner, intent(&new, b"schema_version: 1\n"))
            .unwrap();
        (done, dropped)
    };

    let mut serve = e.serve(&root, &[]);
    serve.wait_serving();
    assert!(
        serve.seen.iter().any(
            |l| l.contains("completed the interrupted write of company.yaml") && l.contains(&done)
        ),
        "{:?}",
        serve.seen
    );
    assert!(
        serve
            .seen
            .iter()
            .any(|l| l.contains("dropped the unwritten config change") && l.contains(&dropped)),
        "{:?}",
        serve.seen
    );
    serve.stop(&root);

    let (store, _) = am_store::open_runtime(&layout, "check".into()).unwrap();
    let history = am_store::StoreConfigHistory::new(&store).unwrap();
    assert!(history.pending_intents().unwrap().is_empty());
    let entries = history.entries(None).unwrap();
    assert!(entries.iter().any(|en| en.id == done), "{entries:?}");
    assert_eq!(std::fs::read(root.join("company.yaml")).unwrap(), new);
}
