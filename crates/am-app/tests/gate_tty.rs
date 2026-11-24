//! Epic 3 story 4: the rows of `agentic gate` that need the real binary
//! (`CARGO_BIN_EXE_agentic`), with HOME (and XDG_DATA_HOME) in a temporary
//! folder, on a stream copy of `fixtures/projects/hand-offs`. The waiting
//! gate is made in process first, by `am_app::cli::run` on am-testkit's
//! `FakeLlm` with the system identity adapter the binary uses (the binary
//! wires no provider, so it cannot run `draft` itself).
//!
//! - Piped stdin: refused in words naming the terminal; nothing read, the
//!   project not opened, the item open, no `inbox.resolved` entry.
//! - No approve flag: `--yes` and `--approve` are usage errors (exit 2,
//!   `usage: agentic gate`); nothing written.
//! - On a pseudo-terminal (`/usr/bin/script -q /dev/null /bin/sh -c …`, as
//!   3.19's harness; macOS only, AD-47, with a compiling skip elsewhere):
//!   a typed `j` and `reject` end the run, audited "You (Reviewer)"; a typed
//!   `a` is refused before anything is written because no model provider
//!   is wired (and reject still works); an enabled stdio MCP server refuses
//!   the command, naming it and the app's Inbox. The shell prints the
//!   command's exit code after it (`EXIT=<n>`), since `script` may not pass
//!   it on.
//!
//! No network, no real model. Written before the command exists.
#![forbid(unsafe_code)]
#![cfg(unix)]
// Off macOS only the piped and usage rows run; the terminal helpers
// compile there unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use am_app::cli::{RunContext, run};
use am_app::runtime::Places;
use am_config::ModelRef;
use am_domain::runtime::open_marker;
use am_domain::{LlmProvider, RuntimeLayout};
use am_store::Reader;
use am_testkit::{FakeLlm, FakeTerminal, ScriptedTurn};

/// The bound on every wait for the binary.
const WAIT: Duration = Duration::from_secs(60);
const DRAFT_1: &str = "the version to review";
const STDIO_TOOLS: &str = "schema_version: 2\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n";

fn agentic_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_agentic"))
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// What a command printed, and its exit code.
#[derive(Debug, Clone)]
struct Ran {
    code: i32,
    out: String,
    err: String,
}

impl Ran {
    fn all(&self) -> String {
        format!("{}\n{}", self.out, self.err)
    }
}

/// A temporary HOME (and XDG_DATA_HOME) and a stream copy of the
/// `hand-offs` fixture.
struct Env {
    dir: tempfile::TempDir,
    root: PathBuf,
    places: Places,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let support = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        dir.path().join("xdg")
    };
    let places = Places::under(&support, &home);
    let root = dir.path().join("project");
    am_testkit::copy_tree(&am_testkit::fixture("projects/hand-offs"), &root);
    let root = root.canonicalize().unwrap();
    Env { dir, root, places }
}

impl Env {
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn xdg(&self) -> PathBuf {
        self.dir.path().join("xdg")
    }

    fn layout(&self) -> RuntimeLayout {
        let id = am_config::read_company(&self.root)
            .unwrap()
            .project_id
            .expect("the project was opened once");
        self.places.base.project(&id)
    }

    fn reader(&self) -> Reader {
        Reader::open(&self.layout().state_db()).unwrap()
    }

    /// `agentic run gated` in process on a `FakeLlm`, with the system
    /// identity adapter: the run waits at its gate. Its id and its Review
    /// item's.
    fn waiting(&self) -> (String, String) {
        let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::text(DRAFT_1)]));
        let providers: am_app::cli::Providers = Arc::new(move |model: &ModelRef| {
            if model.account == "local-lmstudio" {
                Ok(llm.clone() as Arc<dyn LlmProvider>)
            } else {
                Err(format!("no provider for {}", model.account))
            }
        });
        let ctx = RunContext {
            places: self.places.clone(),
            identity: am_app::identity::system(),
            terminal: Box::new(FakeTerminal::new()),
            providers,
        };
        let argv = vec![
            "gated".to_string(),
            "--project".to_string(),
            self.root.to_string_lossy().into_owned(),
        ];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = block_on(run(&argv, ctx, &mut out, &mut err));
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{out}{}", String::from_utf8_lossy(&err));
        let line = out.lines().find(|l| l.starts_with("run ")).unwrap();
        let words: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(words.get(2), Some(&"waiting_human"), "{line}");
        let run = words[1].to_string();
        let item = self.reader().inbox_items(&run).unwrap()[0].id.clone();
        (run, item)
    }

    /// The real binary with `args`, HOME and XDG_DATA_HOME in the
    /// temporary folder.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(agentic_bin());
        cmd.args(args)
            .env("HOME", self.home())
            .env("XDG_DATA_HOME", self.xdg());
        cmd
    }

    /// `agentic <args>` with `typed` ready on a stdin pipe.
    fn piped(&self, args: &[&str], typed: &str) -> Ran {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The command may end before reading: a closed pipe is fine.
        let _ = child.stdin.take().unwrap().write_all(typed.as_bytes());
        let o = child.wait_with_output().unwrap();
        Ran {
            code: o.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&o.stdout).into_owned(),
            err: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    /// The item `item` of the run `run` as the reader gives it (only 3.3's
    /// fields are read, so this file builds before story 3.4's API).
    fn item_row(&self, run: &str, item: &str) -> am_store::InboxItemRow {
        self.reader()
            .inbox_items(run)
            .unwrap()
            .into_iter()
            .find(|i| i.id == item)
            .expect("the item")
    }

    /// The item is open and unresolved, and no `inbox.resolved` entry
    /// exists.
    fn assert_unresolved(&self, run: &str, item: &str) {
        let row = self.item_row(run, item);
        assert_eq!(row.status, "open");
        assert_eq!((row.resolved_by, row.resolved_at), (None, None));
        assert!(self.resolved_entries().is_empty());
    }

    /// The item was resolved `reject` by the owner as Reviewer: its row and
    /// its one `inbox.resolved` entry.
    fn assert_rejected(&self, run: &str, item: &str) {
        let row = self.item_row(run, item);
        assert_eq!(row.status, "resolved");
        let by = row.resolved_by.expect("resolved_by");
        assert!(
            by.contains("\"kind\":\"human\"") && by.contains("\"hat\":\"Reviewer\""),
            "{by}"
        );
        let entries = self.resolved_entries();
        assert_eq!(entries.len(), 1, "{entries:#?}");
        let entry = &entries[0];
        assert!(
            entry.contains("\"kind\":\"human\"") && entry.contains("\"hat\":\"Reviewer\""),
            "{entry}"
        );
        assert!(entry.contains("\"decision\":\"reject\""), "{entry}");
        assert!(entry.contains(item), "{entry}");
    }

    fn resolved_entries(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.reader().export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .filter(|l| l.contains("\"action\":\"inbox.resolved\""))
            .map(str::to_string)
            .collect()
    }

    fn assert_closed(&self) {
        assert!(!open_marker(&self.root).exists(), "no marker is left");
        assert!(!am_store::lock_is_held(&self.layout()), "no lock is left");
    }
}

/// Row 15: `echo a | agentic gate --project P` exits 1 naming the terminal
/// rule; nothing is read, the project is not opened (the database's bytes
/// unchanged, no marker, no lock), the item stays open and there is no
/// `inbox.resolved` entry.
#[test]
fn piping_input_to_agentic_gate_is_refused() {
    let e = env();
    let (run, item) = e.waiting();
    let root = e.root.to_string_lossy().into_owned();
    let db = e.layout().state_db();
    let bytes = std::fs::read(&db).unwrap();

    let ran = e.piped(&["gate", "--project", &root], "a\nreject\n");

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("terminal"), "{}", ran.all());
    assert!(
        !ran.out.lines().any(|l| l.starts_with("run ")),
        "{}",
        ran.out
    );
    assert_eq!(
        std::fs::read(&db).unwrap(),
        bytes,
        "the store was not opened"
    );
    e.assert_closed();
    e.assert_unresolved(&run, &item);
}

/// Row 43 (AD-15: no auto-approve): `agentic gate --yes` and `--approve`
/// are usage errors (exit 2, `usage: agentic gate`), and nothing is
/// written.
#[test]
fn agentic_gate_has_no_approve_flag() {
    let e = env();
    let (run, item) = e.waiting();
    let root = e.root.to_string_lossy().into_owned();
    let db = e.layout().state_db();
    let bytes = std::fs::read(&db).unwrap();
    for flag in ["--yes", "--approve", "-y"] {
        let ran = e.piped(&["gate", flag, "--project", &root], "");

        assert_eq!(ran.code, 2, "{flag}: {}", ran.all());
        assert!(
            ran.err.contains("usage: agentic gate"),
            "{flag}: {}",
            ran.all()
        );
        assert_eq!(std::fs::read(&db).unwrap(), bytes, "{flag}");
    }
    e.assert_unresolved(&run, &item);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_terminal_rows_run_on_macos_only() {
    eprintln!(
        "skipped: the pseudo-terminal rows of agentic gate use /usr/bin/script and run on macOS only (AD-47)"
    );
}

#[cfg(target_os = "macos")]
mod terminal {
    use std::io::Read;
    use std::sync::Mutex;
    use std::time::Instant;

    use super::*;

    /// What a child wrote, in order (stdout and stderr together).
    type Seen = Arc<Mutex<Vec<u8>>>;

    fn capture(mut stream: impl Read + Send + 'static, seen: Seen) {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.lock().unwrap().extend_from_slice(&buf[..n]),
                }
            }
        });
    }

    fn text(seen: &Seen) -> String {
        String::from_utf8_lossy(&seen.lock().unwrap()).into_owned()
    }

    /// `agentic gate --project <root>` on a pseudo-terminal made by
    /// `/usr/bin/script`: once `ready` shows (or the command ended), each
    /// of `keys` is typed with Return, half a second apart. The transcript
    /// (what the terminal showed) is `out`; the code is the `EXIT=<n>` the
    /// shell printed after the command.
    pub(super) fn on_a_terminal(e: &Env, ready: &str, keys: &[&str]) -> Ran {
        let lines: Vec<String> = keys.iter().map(|k| format!("{k}\n")).collect();
        let raw: Vec<&str> = lines.iter().map(String::as_str).collect();
        typing(e, ready, &raw)
    }

    /// [`on_a_terminal`], with each of `raw` typed as it is (no Return
    /// added): `"\u{4}"` is Ctrl-D, the terminal's end of input.
    pub(super) fn typing(e: &Env, ready: &str, raw: &[&str]) -> Ran {
        let line = format!(
            "'{}' gate --project '{}'; echo \"EXIT=$?\"",
            agentic_bin().display(),
            e.root.display()
        );
        let mut script = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", "/bin/sh", "-c", &line])
            .env("HOME", e.home())
            .env("XDG_DATA_HOME", e.xdg())
            .env("TERM", "dumb")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("/usr/bin/script runs");
        let seen = Seen::default();
        capture(script.stdout.take().unwrap(), seen.clone());
        capture(script.stderr.take().unwrap(), seen.clone());
        let mut stdin = script.stdin.take().unwrap();
        let deadline = Instant::now() + WAIT;
        while !text(&seen).contains(ready) && !text(&seen).contains("EXIT=") {
            if Instant::now() > deadline {
                let _ = script.kill();
                panic!("`{ready}` never showed:\n{}", text(&seen));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        for key in raw {
            std::thread::sleep(Duration::from_millis(500));
            let _ = stdin.write_all(key.as_bytes());
            let _ = stdin.flush();
        }
        while !text(&seen).contains("EXIT=") {
            if Instant::now() > deadline {
                let _ = script.kill();
                panic!("agentic gate did not end:\n{}", text(&seen));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(stdin);
        let _ = script.wait();
        let out = text(&seen);
        let code = out
            .split("EXIT=")
            .nth(1)
            .and_then(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .unwrap_or(-1);
        Ran {
            code,
            out,
            err: String::new(),
        }
    }
}

/// Rows 7 and 10 on the real binary: on a terminal, a typed `j` and then
/// `reject` resolve the item `reject` as You (Reviewer) (its one
/// `inbox.resolved` entry), the gate and the run fail, and the command
/// exits 0; the project is closed again.
#[cfg(target_os = "macos")]
#[test]
fn on_a_terminal_a_typed_reject_ends_the_run() {
    let e = env();
    let (run, item) = e.waiting();

    let ran = terminal::on_a_terminal(&e, &item, &["j", "reject"]);

    assert_eq!(ran.code, 0, "{}", ran.all());
    e.assert_rejected(&run, &item);
    assert_eq!(e.reader().run(&run).unwrap().unwrap().status, "failed");
    let gate = e
        .reader()
        .steps(&run)
        .unwrap()
        .into_iter()
        .find(|s| s.step_key == "approve")
        .unwrap();
    assert_eq!(gate.status, "failed");
    e.assert_closed();
}

/// Row 32: the binary wires no model provider, so a typed `a` is refused
/// before anything is written, in the words "no model provider is wired";
/// the item stays open. Reject still works.
#[cfg(target_os = "macos")]
#[test]
fn on_a_terminal_approve_is_refused_while_no_provider_is_wired() {
    let e = env();
    let (run, item) = e.waiting();

    let ran = terminal::on_a_terminal(&e, &item, &["a"]);

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.out.contains("no model provider is wired"),
        "{}",
        ran.all()
    );
    e.assert_unresolved(&run, &item);
    e.assert_closed();

    let ran = terminal::on_a_terminal(&e, &item, &["j", "reject"]);
    assert_eq!(ran.code, 0, "{}", ran.all());
    e.assert_rejected(&run, &item);
    assert_eq!(e.reader().run(&run).unwrap().unwrap().status, "failed");
}

/// Row 17 on the real binary: with a stdio MCP server enabled in the
/// project's `tools.yaml`, `agentic gate` on a terminal is refused, naming
/// the server and the app's Inbox; nothing is read or written.
#[cfg(target_os = "macos")]
#[test]
fn an_enabled_stdio_mcp_server_refuses_it_on_a_terminal() {
    let e = env();
    let (run, item) = e.waiting();
    std::fs::write(e.root.join("tools.yaml"), STDIO_TOOLS).unwrap();

    let ran = terminal::on_a_terminal(&e, "EXIT=", &[]);

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.out.contains("notes"), "{}", ran.all());
    assert!(ran.out.contains("Inbox"), "{}", ran.all());
    e.assert_unresolved(&run, &item);
    e.assert_closed();
}

// ------------------------------------------------------- review round 1
//
// Story 3.4 review round 1 (the coordinator's triage, 2026-11-24), R11: on
// the real terminal a partial line followed by the end of input (Ctrl-D
// twice) is no answer. Written before the fix (today the owner's terminal
// returns the partial line as if Return ended it).

/// The open-time prompt of the setup: never asked.
struct NoPrompt;

impl am_app::runtime::Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

/// An open item of `kind` (a Failure or a Question) on a run of `gated`
/// made by System commands, through the system identity adapter the
/// binary uses; its id.
fn item_by_system(e: &Env, kind: am_domain::InboxKind) -> String {
    use am_domain::{Actor, InboxKind, Taint};
    use am_store::{NewInboxItem, WriteCommand, new_id};
    let identity = am_app::identity::system();
    let req = am_app::runtime::OpenRequest {
        root: e.root.clone(),
        confirm_move: false,
        take_over: false,
    };
    let opened =
        am_app::runtime::open_project(&e.places, identity.as_ref(), &req, &mut NoPrompt).unwrap();
    let engine = || Actor::System {
        component: "engine".into(),
    };
    let w = opened.store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "gated".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: None,
        },
    )
    .unwrap();
    let draft = opened.store.reader().unwrap().steps(&run).unwrap()[0]
        .id
        .clone();
    let item = new_id();
    let new_item = NewInboxItem {
        id: item.clone(),
        kind,
        payload: None,
        taint: Taint::Clean,
        title: format!("A {} item", kind.title()),
        body: "Made by the test.".into(),
        max_returns: None,
    };
    if kind == InboxKind::Failure {
        w.exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: draft.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
            },
        )
        .unwrap();
        w.exec(
            engine(),
            WriteCommand::FailWithItem {
                step_run_id: draft,
                settle: None,
                output: None,
                item: new_item,
            },
        )
        .unwrap();
    } else {
        w.exec(
            engine(),
            WriteCommand::PauseStep {
                step_run_id: draft,
                inputs: vec![],
                taint: Taint::Clean,
                item: new_item,
            },
        )
        .unwrap();
    }
    drop(w);
    drop(opened);
    item
}

/// R11 on the real binary: on a terminal, a partial line followed by the
/// end of input (Ctrl-D twice) answers nothing at the reject confirmation
/// (`j`, then `reject` with no Return), at a Failure's choice (`d` with no
/// Return) and at a Question's answer (`answer`, then the text with no
/// Return): each writes nothing, and the command exits 1.
#[cfg(target_os = "macos")]
#[test]
fn on_a_terminal_a_partial_line_then_the_end_of_input_writes_nothing() {
    let e = env();
    let (run, item) = e.waiting();
    let ran = terminal::typing(&e, &item, &["j\n", "reject\u{4}\u{4}"]);
    assert_eq!(ran.code, 1, "{}", ran.all());
    e.assert_unresolved(&run, &item);
    e.assert_closed();

    for (kind, raw) in [
        (am_domain::InboxKind::Failure, vec!["d\u{4}\u{4}"]),
        (
            am_domain::InboxKind::Question,
            vec!["answer\n", "an answer\u{4}\u{4}"],
        ),
    ] {
        let e = env();
        let item = item_by_system(&e, kind);

        let ran = terminal::typing(&e, &item, &raw);

        assert_eq!(ran.code, 1, "{kind:?}: {}", ran.all());
        let row = e.reader().inbox_item(&item).unwrap().unwrap();
        assert_eq!(row.status, "open", "{kind:?}: {}", ran.all());
        assert!(e.resolved_entries().is_empty(), "{kind:?}");
        e.assert_closed();
    }
}
