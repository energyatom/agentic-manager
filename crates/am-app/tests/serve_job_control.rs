//! Story 3.19 (ticket 19; story 3.2's review finding P9, AD-13): a
//! backgrounded `agentic serve` keeps answering. Written first, failing,
//! against the real binary (`CARGO_BIN_EXE_agentic`) with HOME (and
//! XDG_DATA_HOME) in a temporary folder and a stream copy of
//! `fixtures/projects/valid`, as `serve_project.rs` does.
//!
//! Job control needs a shell whose controlling terminal is a
//! pseudo-terminal, that is a session leader: `/usr/bin/script -q /dev/null
//! /bin/sh -i` makes the pty and gives it to the interactive shell as its
//! controlling terminal, so no test calls setsid, nix's process functions
//! or unsafe code. The test types into script's stdin and reads script's
//! output, which is the terminal's: what the shell echoes and what serve
//! prints. `agentic serve ... &` typed there is a real background job: its
//! stdin is the terminal, the terminal is its controlling terminal and its
//! process group is not the terminal's foreground group, as when the owner
//! types it in an interactive shell. serve's pid, port and CLI token come
//! from its records in the app data dir (`serving/<pid>`,
//! `serving/<pid>.token`); its process state from `/bin/ps -o stat=` (`T`
//! stopped, `+` in the terminal's foreground process group).
//!
//! Rows of the plan's I/O matrix here: 1 foreground Enter; 2 background
//! keeps answering (before the fix the background serve is stopped by
//! SIGTTIN: that is the red); 3 background issues no code whatever is
//! typed; 4 `fg` then Enter; 5 no route, signal or file issues a code
//! (foreground and background); 6 stdin not a terminal; 9 the usage and
//! the start line. Row 7 is `serve_foreground_check.rs`, row 8
//! `serve_job_control_constraints.rs`. The pseudo-terminal rows run on
//! macOS only (AD-47); elsewhere they compile out and one test says so.
#![forbid(unsafe_code)]
#![cfg(unix)]
// Off macOS only the usage and piped-stdin rows run; the job-control
// helpers compile there unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use am_domain::runtime::{AppDataLayout, RUNTIME_BASE_NAME};

/// The bound on every wait for serve to start or a line to appear.
const WAIT: Duration = Duration::from_secs(60);
/// The bound on a new code after Enter: the reader may wait about a second
/// between foreground checks before it reads.
const ENTER_WAIT: Duration = Duration::from_secs(15);
/// How long a test leaves serve to react (wrongly) to a key or a signal.
const SETTLE: Duration = Duration::from_millis(1500);
/// How long a background serve must keep answering (the ticket: at least
/// 5 s).
const ANSWERING: Duration = Duration::from_millis(5500);
const HEALTH: &str = "/api/v1/health";
const RUNS: &str = "/api/v1/runs";
const PAIR: &str = "/api/v1/pair";
const TOKEN_HEADER: &str = "X-AM-CLI-Token";
const CSRF_HEADER: &str = "X-AM-CSRF";

fn agentic_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_agentic"))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ------------------------------------------------------------ output

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

/// The complete lines of `text` (a last line with no newline yet is left
/// out), without their carriage returns.
fn complete_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    lines
        .into_iter()
        .map(|l| l.trim_end_matches('\r'))
        .collect()
}

/// The 8-character RFC 4648 base32 words of `text` (pairing codes).
fn base32_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| {
            w.len() == 8
                && w.chars()
                    .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))
        })
        .map(str::to_string)
        .collect()
}

/// The pairing codes serve printed, in order: the code-shaped words of
/// every complete line naming a pairing code.
fn codes(text: &str) -> Vec<String> {
    complete_lines(text)
        .into_iter()
        .filter(|l| l.contains("pairing code"))
        .flat_map(base32_words)
        .collect()
}

/// The start line that says when Enter works: it names Enter and the
/// foreground and carries no code (the code line is another line).
fn enter_lines(text: &str) -> Vec<String> {
    complete_lines(text)
        .into_iter()
        .filter(|l| l.contains("Enter") && l.contains("foreground") && base32_words(l).is_empty())
        .map(str::to_string)
        .collect()
}

// ------------------------------------------------------------ the env

/// A temporary HOME and a stream copy of `fixtures/projects/valid`.
struct Env {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("home")).unwrap();
        let root = dir.path().join("projects").join("p");
        am_testkit::copy_tree(&am_testkit::fixture("projects/valid"), &root);
        let root = root.canonicalize().unwrap();
        Env { dir, root }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn xdg(&self) -> PathBuf {
        self.dir.path().join("xdg")
    }

    fn support(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home().join("Library/Application Support")
        } else {
            self.xdg()
        }
    }

    fn app_data(&self) -> PathBuf {
        self.support().join("agentic-manager")
    }

    fn runtime_base(&self) -> PathBuf {
        self.support().join(RUNTIME_BASE_NAME)
    }

    fn layout(&self) -> AppDataLayout {
        AppDataLayout::new(self.app_data())
    }

    /// `agentic serve` on the project, as typed in the job-control shell
    /// (whose environment holds the binary and the project as
    /// `SERVE_BIN` and `SERVE_PROJECT`, so the typed line stays short);
    /// `tail` is what follows it (` &`, ` < /dev/null &` or nothing).
    fn serve_line(&self, tail: &str) -> String {
        format!("\"$SERVE_BIN\" serve --no-browser --port 0 --project \"$SERVE_PROJECT\"{tail}\n")
    }

    /// The running serve, once it has written its port record and CLI
    /// token (exactly one serve per test).
    fn served(&self) -> Option<Served> {
        let layout = self.layout();
        let dir = layout.serving_dir();
        let pids: Vec<u32> = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(|en| {
                let name = en.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".token")?.parse().ok()
            })
            .collect();
        let &[pid] = pids.as_slice() else {
            return None;
        };
        let token = std::fs::read_to_string(layout.cli_token(pid))
            .ok()?
            .trim()
            .to_string();
        let port = std::fs::read_to_string(dir.join(pid.to_string()))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        (token.len() == 64).then_some(Served { pid, port, token })
    }
}

/// A running serve's pid, port and CLI token (read from its records).
struct Served {
    pid: u32,
    port: u16,
    token: String,
}

impl Served {
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// A CLI request: the token, no Origin.
    fn with_token(&self, method: &str, path: &str) -> Result<Resp, String> {
        let body = if method == "POST" { "{}" } else { "" };
        http(
            self.port,
            method,
            path,
            &[(TOKEN_HEADER, self.token.clone())],
            body,
        )
    }

    /// `POST /api/v1/pair {code}` with the served Origin.
    fn pair(&self, code: &str) -> Resp {
        http(
            self.port,
            "POST",
            PAIR,
            &[("Origin", self.origin())],
            &format!(r#"{{"code":"{code}"}}"#),
        )
        .unwrap_or_else(|e| panic!("POST {PAIR}: {e}"))
    }
}

/// `/bin/ps -o <field> -p <pid>`, trimmed; empty when the process is gone.
fn ps(pid: u32, field: &str) -> String {
    let out = Command::new("/bin/ps")
        .args(["-o", field, "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .output()
        .expect("/bin/ps runs");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn signal(pid: u32, name: &str) {
    let ok = Command::new("/bin/kill")
        .args([name, &pid.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok, "kill {name} {pid}");
}

// ------------------------------------------------------------ raw HTTP

/// One answer: the status line and headers, and the body.
struct Resp {
    status: u16,
    head: String,
    body: String,
}

impl Resp {
    fn header(&self, name: &str) -> Vec<String> {
        self.head
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_string())
            .collect()
    }

    fn text(&self) -> String {
        format!("{}\r\n\r\n{}", self.head, self.body)
    }
}

/// A raw HTTP/1.1 request to 127.0.0.1:`port` (Host added, the connection
/// closed after the answer); an error when there is no answer within 3 s.
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &str,
) -> Result<Resp, String> {
    let limit = Duration::from_secs(3);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, limit)
        .map_err(|e| format!("{method} {path}: no connection ({e})"))?;
    s.set_read_timeout(Some(limit)).unwrap();
    s.set_write_timeout(Some(limit)).unwrap();
    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if method == "POST" {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes())
        .map_err(|e| format!("{method} {path}: not sent ({e})"))?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf)
        .map_err(|e| format!("{method} {path}: no answer within 3 s ({e})"))?;
    if buf.is_empty() {
        return Err(format!("{method} {path}: closed without an answer"));
    }
    let all = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = all.split_once("\r\n\r\n").unwrap_or((&all, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(Resp {
        status,
        head: head.to_string(),
        body: body.to_string(),
    })
}

/// A browser session (from a pairing): the `am_s_<port>` cookie and the
/// CSRF value.
struct Session {
    cookie: String,
    csrf: String,
}

fn session_of(r: &Resp, port: u16) -> Session {
    assert_eq!(r.status, 200, "{}", r.text());
    let cookies = r.header("set-cookie");
    assert_eq!(cookies.len(), 1, "one cookie: {}", r.text());
    let pair = cookies[0].split(';').next().unwrap().trim().to_string();
    assert!(pair.starts_with(&format!("am_s_{port}=")), "{pair}");
    let csrf = r
        .body
        .split(r#""csrf":""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("csrf in the body: {}", r.body))
        .to_string();
    Session { cookie: pair, csrf }
}

/// Row 5: every route of `am_server::route_table()`, GET and POST, with
/// the CLI token (a CLI request: no Origin) and, when given, with the
/// session (cookie, CSRF, the served Origin on a POST). `{name}` segments
/// get a run-id-shaped value. Every answer, or the first request that got
/// none (and why).
fn sweep(s: &Served, session: Option<&Session>) -> Result<Vec<(String, Resp)>, (String, String)> {
    let mut out = Vec::new();
    for spec in am_server::route_table() {
        let path: String = spec
            .path
            .split('/')
            .map(|seg| {
                if seg.starts_with('{') && seg.ends_with('}') {
                    "019ab000-0000-7000-8000-000000000000"
                } else {
                    seg
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        for method in ["GET", "POST"] {
            let what = format!("{method} {path} with the CLI token");
            match s.with_token(method, &path) {
                Ok(r) => out.push((what, r)),
                Err(e) => return Err((what, e)),
            }
            if let Some(sess) = session {
                let mut h = vec![
                    ("Cookie", sess.cookie.clone()),
                    (CSRF_HEADER, sess.csrf.clone()),
                ];
                let body = if method == "POST" {
                    h.push(("Origin", s.origin()));
                    "{}"
                } else {
                    ""
                };
                let what = format!("{method} {path} with a session");
                match http(s.port, method, &path, &h, body) {
                    Ok(r) => out.push((what, r)),
                    Err(e) => return Err((what, e)),
                }
            }
        }
    }
    Ok(out)
}

/// No answer carries a printed code, and none carries a code-shaped word
/// except inside a shipped page served byte for byte (`/`, `/app.js`).
fn assert_no_code_in_answers(answers: &[(String, Resp)], printed: &[String]) {
    let pages: Vec<String> = ["web/index.html", "web/app.js"]
        .iter()
        .map(|p| std::fs::read_to_string(repo_root().join(p)).unwrap())
        .collect();
    assert!(!answers.is_empty());
    for (what, r) in answers {
        assert_ne!(r.status, 0, "{what}: not an HTTP answer:\n{}", r.text());
        for c in printed {
            assert!(
                !r.text().contains(c.as_str()),
                "{what} carries the pairing code {c}:\n{}",
                r.text()
            );
        }
        let body = if pages.contains(&r.body) {
            ""
        } else {
            r.body.as_str()
        };
        let words = base32_words(&format!("{}\n{body}", r.head));
        assert!(
            words.is_empty(),
            "{what} carries code-shaped words {words:?}:\n{}",
            r.text()
        );
    }
}

/// No file under the app data dir, the runtime base or the project holds
/// a printed code.
fn assert_no_code_in_files(e: &Env, printed: &[String]) {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for en in rd.flatten() {
            let p = en.path();
            match en.file_type() {
                Ok(t) if t.is_dir() => walk(&p, files),
                Ok(t) if t.is_file() => files.push(p),
                _ => {}
            }
        }
    }
    let mut files = Vec::new();
    for top in [e.app_data(), e.runtime_base(), e.root.clone()] {
        walk(&top, &mut files);
    }
    assert!(!files.is_empty());
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        for c in printed {
            assert!(
                !bytes.windows(c.len()).any(|w| w == c.as_bytes()),
                "{} holds the pairing code {c}",
                f.display()
            );
        }
    }
}

// ------------------------------------------------------------ all Unix

/// Row 9: the `agentic serve` usage (printed on a usage error) says Enter
/// works with serve in the foreground and names `agentic serve < /dev/null
/// &` for background use.
#[test]
fn the_serve_usage_names_enter_in_the_foreground_and_dev_null_for_the_background() {
    let e = Env::new();
    let out = Command::new(agentic_bin())
        .args(["serve", "--no-browser", "--port", "0", "--project"])
        .env("HOME", e.home())
        .env("XDG_DATA_HOME", e.xdg())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("usage: agentic serve"), "{err}");
    assert!(
        err.contains("agentic serve < /dev/null &"),
        "the usage names `agentic serve < /dev/null &` for background use: {err}"
    );
    assert!(
        err.contains("Enter") && err.contains("foreground"),
        "the usage says Enter works with serve in the foreground: {err}"
    );
}

/// Row 6 (stdin piped): nothing reads stdin. Enter written to the pipe
/// prints no new code, no Enter line is printed, and serve answers.
#[test]
fn with_stdin_piped_nothing_reads_it_and_no_code_follows_enter() {
    let e = Env::new();
    let mut child = Command::new(agentic_bin())
        .args(["serve", "--no-browser", "--port", "0", "--project"])
        .arg(&e.root)
        .env("HOME", e.home())
        .env("XDG_DATA_HOME", e.xdg())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let seen = Seen::default();
    capture(child.stdout.take().unwrap(), seen.clone());
    capture(child.stderr.take().unwrap(), seen.clone());
    let mut stdin = child.stdin.take().unwrap();
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Kill(child);

    let deadline = Instant::now() + WAIT;
    let s = loop {
        if let Some(s) = e.served() {
            break s;
        }
        if let Ok(Some(status)) = child.0.try_wait() {
            panic!("serve ended ({status}): {}", text(&seen));
        }
        assert!(
            Instant::now() < deadline,
            "serve never started: {}",
            text(&seen)
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let deadline = Instant::now() + WAIT;
    while codes(&text(&seen)).is_empty() {
        assert!(Instant::now() < deadline, "no start code: {}", text(&seen));
        std::thread::sleep(Duration::from_millis(20));
    }
    let start = codes(&text(&seen));
    assert_eq!(start.len(), 1, "{}", text(&seen));

    stdin.write_all(b"\n\n").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(SETTLE);
    let all = text(&seen);
    assert_eq!(codes(&all), start, "Enter on a pipe printed a code: {all}");
    assert!(
        enter_lines(&all).is_empty(),
        "the Enter line printed with stdin not a terminal: {all}"
    );
    let r = s.with_token("GET", HEALTH).unwrap();
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(child.0.try_wait().unwrap().is_none(), "serve still runs");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_job_control_rows_run_on_macos_only() {
    eprintln!(
        "skipped: the pseudo-terminal job-control rows of story 3.19 run on macOS only (AD-47)"
    );
}

// ------------------------------------------------------------ macOS: job control

#[cfg(target_os = "macos")]
mod job_control {
    use std::process::{Child, ChildStdin};

    use super::*;

    const PROMPT: &str = "jc$ ";

    /// An interactive `/bin/sh` with job control on a pseudo-terminal made
    /// by `/usr/bin/script`; serve (once started from it) and script are
    /// killed when it is dropped.
    struct Shell {
        script: Child,
        keys: ChildStdin,
        seen: Seen,
        serve: Option<u32>,
    }

    impl Shell {
        fn start(e: &Env) -> Shell {
            let mut script = Command::new("/usr/bin/script")
                .args(["-q", "/dev/null", "/bin/sh", "-i"])
                .env("HOME", e.home())
                .env("XDG_DATA_HOME", e.xdg())
                .env("PS1", PROMPT)
                .env("SERVE_BIN", agentic_bin())
                .env("SERVE_PROJECT", &e.root)
                .env("TERM", "dumb")
                .env("BASH_SILENCE_DEPRECATION_WARNING", "1")
                .env_remove("ENV")
                .env_remove("BASH_ENV")
                .env_remove("PROMPT_COMMAND")
                .current_dir(e.dir.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("/usr/bin/script runs");
            let seen = Seen::default();
            capture(script.stdout.take().unwrap(), seen.clone());
            capture(script.stderr.take().unwrap(), seen.clone());
            let keys = script.stdin.take().unwrap();
            let mut sh = Shell {
                script,
                keys,
                seen,
                serve: None,
            };
            sh.wait_for("the shell's prompt", WAIT, |t| t.contains(PROMPT));
            sh
        }

        fn transcript(&self) -> String {
            text(&self.seen)
        }

        fn mark(&self) -> usize {
            self.seen.lock().unwrap().len()
        }

        fn since(&self, mark: usize) -> String {
            String::from_utf8_lossy(&self.seen.lock().unwrap()[mark..]).into_owned()
        }

        /// Types `keys` on the terminal.
        fn typed(&mut self, keys: &str) {
            self.keys.write_all(keys.as_bytes()).unwrap();
            self.keys.flush().unwrap();
        }

        fn wait_for(&mut self, what: &str, limit: Duration, done: impl Fn(&str) -> bool) {
            let deadline = Instant::now() + limit;
            loop {
                let t = self.transcript();
                if done(&t) {
                    return;
                }
                if let Ok(Some(status)) = self.script.try_wait() {
                    panic!("the shell ended ({status}) before {what}; terminal:\n{t}");
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {what}; terminal:\n{t}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn codes(&self) -> Vec<String> {
            codes(&self.transcript())
        }

        fn wait_codes(&mut self, n: usize, what: &str, limit: Duration) -> Vec<String> {
            self.wait_for(what, limit, |t| codes(t).len() >= n);
            self.codes()
        }

        /// serve, started from this shell, once its records are written.
        fn wait_served(&mut self, e: &Env) -> Served {
            let deadline = Instant::now() + WAIT;
            loop {
                if let Some(s) = e.served() {
                    self.serve = Some(s.pid);
                    return s;
                }
                if let Ok(Some(status)) = self.script.try_wait() {
                    panic!(
                        "the shell ended ({status}) before serve started; terminal:\n{}",
                        self.transcript()
                    );
                }
                assert!(
                    Instant::now() < deadline,
                    "serve never wrote its records; terminal:\n{}",
                    self.transcript()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        /// serve runs: not stopped by job control (no `T` in its state)
        /// and not gone. A stopped serve fails with the shell's own job
        /// report (`jobs -l`), which names the signal (SIGTTIN: "tty
        /// input").
        fn assert_not_stopped(&mut self, pid: u32, when: &str) {
            let stat = ps(pid, "stat=");
            if stat.contains('T') {
                let mark = self.mark();
                self.typed("jobs -l\n");
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline && !self.since(mark).contains("Stopped") {
                    std::thread::sleep(Duration::from_millis(20));
                }
                panic!(
                    "{when}: serve (pid {pid}) was stopped by job control (ps state {stat:?}) \
                     instead of serving; the shell's job report:\n{}\nterminal:\n{}",
                    self.since(mark),
                    self.transcript()
                );
            }
            assert!(
                !stat.is_empty(),
                "{when}: serve (pid {pid}) is gone; terminal:\n{}",
                self.transcript()
            );
        }

        /// Fails (with the job report when serve is stopped) unless `r` is
        /// an answer.
        fn answered(&mut self, pid: u32, what: &str, r: Result<Resp, String>) -> Resp {
            match r {
                Ok(r) => r,
                Err(err) => {
                    self.assert_not_stopped(pid, what);
                    panic!(
                        "{what}: serve did not answer ({err}); terminal:\n{}",
                        self.transcript()
                    );
                }
            }
        }

        /// A sweep's answers; when a request got none, fails (with the job
        /// report when serve is stopped).
        fn swept(
            &mut self,
            pid: u32,
            sweep: Result<Vec<(String, Resp)>, (String, String)>,
        ) -> Vec<(String, Resp)> {
            match sweep {
                Ok(answers) => answers,
                Err((what, err)) => {
                    self.assert_not_stopped(pid, &what);
                    panic!(
                        "{what}: serve did not answer ({err}); terminal:\n{}",
                        self.transcript()
                    );
                }
            }
        }

        /// Waits (bounded) until serve is in the terminal's foreground
        /// process group (`+` in its state).
        fn wait_foreground(&mut self, pid: u32, what: &str) {
            let deadline = Instant::now() + ENTER_WAIT;
            loop {
                let stat = ps(pid, "stat=");
                if stat.contains('+') {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "{what}: serve (pid {pid}) never became the foreground job (ps state \
                     {stat:?}); terminal:\n{}",
                    self.transcript()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        /// The background job's preconditions: the pseudo-terminal is
        /// serve's controlling terminal and its stdin (the Enter line is
        /// printed only for a terminal stdin), and serve's process group is
        /// not the foreground one.
        fn assert_background_job_on_the_terminal(&mut self, pid: u32) {
            self.wait_for("the Enter line (stdin is the terminal)", WAIT, |t| {
                !enter_lines(t).is_empty()
            });
            let tty = ps(pid, "tty=");
            assert!(
                !tty.is_empty() && !tty.starts_with('?'),
                "serve has the pseudo-terminal as its controlling terminal: {tty:?}"
            );
            let stat = ps(pid, "stat=");
            assert!(
                !stat.contains('+'),
                "serve is a background job (ps state {stat:?}); terminal:\n{}",
                self.transcript()
            );
        }

        /// Types lines the shell answers, and waits for the answer: proof
        /// that the shell, not serve, read what was typed.
        fn typed_and_read_by_the_shell(&mut self, keys: &str) {
            self.typed(keys);
            self.typed("echo typed-$((40+2))\n");
            self.wait_for("the shell's answer to the typed line", WAIT, |t| {
                t.contains("typed-42")
            });
        }
    }

    impl Drop for Shell {
        fn drop(&mut self) {
            if let Some(pid) = self.serve {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &pid.to_string()])
                    .status();
            }
            let _ = self.script.kill();
            let _ = self.script.wait();
        }
    }

    /// Starts serve from a new job-control shell: in the background when
    /// `tail` ends in `&`. Returns the shell, serve and its start code.
    fn start(e: &Env, tail: &str) -> (Shell, Served, String) {
        let mut sh = Shell::start(e);
        sh.typed(&e.serve_line(tail));
        let s = sh.wait_served(e);
        let start = sh.wait_codes(1, "the start pairing code", WAIT);
        assert_eq!(start.len(), 1, "one code at start: {}", sh.transcript());
        let c0 = start[0].clone();
        (sh, s, c0)
    }

    /// Row 1, and row 9's start line: serve started in the foreground of a
    /// job-control shell says Enter works in the foreground; Enter prints a
    /// new code; the previous unused one is refused at `/api/v1/pair` and
    /// the new one pairs. Ctrl-C then ends serve cleanly (its token file is
    /// removed), as in story 3.2.
    #[test]
    fn in_the_foreground_enter_prints_a_new_code_and_only_the_new_one_pairs() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, "");
        sh.wait_foreground(s.pid, "serve started in the foreground");
        sh.wait_for("the Enter line", WAIT, |t| !enter_lines(t).is_empty());

        sh.typed("\n");
        let codes = sh.wait_codes(2, "a new pairing code after Enter", ENTER_WAIT);
        assert_eq!(codes.len(), 2, "{}", sh.transcript());
        let c1 = codes[1].clone();
        assert_ne!(c1, c0, "a new code");
        let old = s.pair(&c0);
        assert_eq!(old.status, 401, "the replaced code: {}", old.text());
        assert!(old.body.contains("pairing-code-invalid"), "{}", old.text());
        let new = s.pair(&c1);
        session_of(&new, s.port);

        sh.typed("\u{3}");
        let token_file = e.layout().cli_token(s.pid);
        let deadline = Instant::now() + WAIT;
        while token_file.exists() {
            assert!(
                Instant::now() < deadline,
                "Ctrl-C did not end serve cleanly; terminal:\n{}",
                sh.transcript()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Row 2 (the ticket's red): `agentic serve &` from an interactive
    /// job-control shell on a pseudo-terminal is not stopped and answers
    /// CLI-token requests on `/api/v1/health` and `/api/v1/runs` for at
    /// least 5 s, printing no code beyond the start one. Before the fix its
    /// Enter reader's read gets SIGTTIN and job control stops the server.
    #[test]
    fn a_backgrounded_serve_is_not_stopped_and_keeps_answering_cli_token_requests() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, " &");
        sh.assert_background_job_on_the_terminal(s.pid);

        let began = Instant::now();
        let mut rounds = 0;
        while began.elapsed() < ANSWERING {
            sh.assert_not_stopped(s.pid, "in the background");
            for path in [HEALTH, RUNS] {
                let what = format!("GET {path} with the CLI token in the background");
                let r = s.with_token("GET", path);
                let r = sh.answered(s.pid, &what, r);
                assert_eq!(r.status, 200, "{what}: {}", r.text());
            }
            rounds += 1;
            std::thread::sleep(Duration::from_millis(250));
        }
        sh.assert_not_stopped(s.pid, "after 5 s in the background");
        assert!(rounds >= 10, "{rounds} rounds");
        assert_eq!(sh.codes(), vec![c0], "{}", sh.transcript());
    }

    /// Row 3: with serve in the background, Enter (twice), a line of text
    /// and a line the shell answers are typed on the terminal; the shell
    /// reads them (its answer appears), serve prints no code beyond the
    /// start one and is never stopped (a read of the terminal from the
    /// background would stop it with SIGTTIN), and it still answers.
    #[test]
    fn a_backgrounded_serve_issues_no_new_code_when_enter_and_text_are_typed() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, " &");
        sh.assert_background_job_on_the_terminal(s.pid);

        sh.typed("\n");
        std::thread::sleep(Duration::from_millis(300));
        sh.typed("\n");
        std::thread::sleep(Duration::from_millis(300));
        sh.typed_and_read_by_the_shell(": some words typed while serve runs in the background\n");
        std::thread::sleep(SETTLE);

        assert_eq!(
            sh.codes(),
            vec![c0],
            "a code was printed in the background: {}",
            sh.transcript()
        );
        sh.assert_not_stopped(s.pid, "after Enter and text were typed in the background");
        let r = s.with_token("GET", HEALTH);
        let r = sh.answered(s.pid, "GET /api/v1/health after typing", r);
        assert_eq!(r.status, 200, "{}", r.text());
    }

    /// Row 4: serve started in the background, then `fg`, then Enter: a
    /// new code is printed (the reader resumes); the start code is refused
    /// and the new one pairs.
    #[test]
    fn brought_to_the_foreground_enter_prints_a_new_code_again() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, " &");
        sh.assert_background_job_on_the_terminal(s.pid);
        std::thread::sleep(SETTLE);

        sh.typed("fg\n");
        sh.wait_foreground(s.pid, "after fg");
        sh.typed("\n");
        let codes = sh.wait_codes(2, "a new pairing code after fg and Enter", ENTER_WAIT);
        assert_eq!(codes.len(), 2, "{}", sh.transcript());
        let c1 = codes[1].clone();
        assert_ne!(c1, c0);
        let old = s.pair(&c0);
        assert_eq!(old.status, 401, "the replaced code: {}", old.text());
        let new = s.pair(&c1);
        session_of(&new, s.port);
    }

    /// Row 5 in the foreground: with serve in the foreground (codes C0 at
    /// start, C1 and C2 from Enter; C1 paired for a session, C2 left
    /// outstanding), every route in `route_table()` is called GET and POST
    /// with the CLI token and with the session, and SIGCONT and SIGWINCH
    /// are sent to serve. No new code line appears, no answer or file
    /// carries a code, and C2 still pairs: nothing issued a newer code.
    #[test]
    fn in_the_foreground_no_route_signal_or_file_issues_a_pairing_code() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, "");
        sh.wait_foreground(s.pid, "serve started in the foreground");
        sh.typed("\n");
        let c1 = sh.wait_codes(2, "C1 after Enter", ENTER_WAIT)[1].clone();
        let session = session_of(&s.pair(&c1), s.port);
        sh.typed("\n");
        let printed = sh.wait_codes(3, "C2 after Enter", ENTER_WAIT);
        assert_eq!(printed.len(), 3, "{}", sh.transcript());
        assert_eq!(printed[..2], [c0, c1]);
        let c2 = printed[2].clone();

        let answers = sweep(&s, Some(&session));
        let answers = sh.swept(s.pid, answers);
        signal(s.pid, "-CONT");
        signal(s.pid, "-WINCH");
        std::thread::sleep(SETTLE);

        assert_eq!(sh.codes(), printed, "{}", sh.transcript());
        assert_no_code_in_answers(&answers, &printed);
        assert_no_code_in_files(&e, &printed);
        let r = s.pair(&c2);
        assert_eq!(
            r.status,
            200,
            "C2 was replaced, so something issued a code: {}",
            r.text()
        );
    }

    /// Row 5 in the background: with serve a background job and its start
    /// code C0 outstanding, every route is called GET and POST with the CLI
    /// token, SIGCONT and SIGWINCH are sent, and Enter and a line are typed;
    /// C0 still pairs (nothing issued a newer code), and with that session
    /// every route is called again. No new code line appears, serve is
    /// never stopped, and no answer or file carries a code.
    #[test]
    fn in_the_background_no_route_signal_typed_line_or_file_issues_a_pairing_code() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, " &");
        sh.assert_background_job_on_the_terminal(s.pid);

        let by_token = sweep(&s, None);
        let mut answers = sh.swept(s.pid, by_token);
        signal(s.pid, "-CONT");
        signal(s.pid, "-WINCH");
        sh.typed_and_read_by_the_shell("\n");
        std::thread::sleep(SETTLE);
        sh.assert_not_stopped(s.pid, "after the token sweep, the signals and Enter");
        assert_eq!(sh.codes(), vec![c0.clone()], "{}", sh.transcript());

        let session = session_of(&s.pair(&c0), s.port);
        let by_session = sweep(&s, Some(&session));
        answers.extend(sh.swept(s.pid, by_session));
        signal(s.pid, "-CONT");
        std::thread::sleep(SETTLE);
        sh.assert_not_stopped(s.pid, "after the session sweep");
        let printed = vec![c0];
        assert_eq!(sh.codes(), printed, "{}", sh.transcript());
        assert_no_code_in_answers(&answers, &printed);
        assert_no_code_in_files(&e, &printed);
    }

    /// Row 6 (`agentic serve < /dev/null &`): stdin is not a terminal, so
    /// no Enter line is printed and nothing reads it; serve keeps answering
    /// for at least 5 s and lines typed on the terminal go to the shell.
    #[test]
    fn with_stdin_from_dev_null_a_backgrounded_serve_reads_nothing_and_keeps_answering() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, " < /dev/null &");
        let began = Instant::now();
        while began.elapsed() < ANSWERING {
            sh.assert_not_stopped(s.pid, "with stdin from /dev/null in the background");
            let r = s.with_token("GET", HEALTH);
            let r = sh.answered(s.pid, "GET /api/v1/health", r);
            assert_eq!(r.status, 200, "{}", r.text());
            std::thread::sleep(Duration::from_millis(250));
        }
        sh.typed_and_read_by_the_shell("\n\n");
        std::thread::sleep(SETTLE);
        sh.assert_not_stopped(s.pid, "after typing");
        let t = sh.transcript();
        assert!(
            enter_lines(&t).is_empty(),
            "the Enter line printed with stdin not a terminal: {t}"
        );
        assert_eq!(sh.codes(), vec![c0], "{t}");
    }

    /// Review round 1, P4 (the plan's residual, Ctrl-Z then `bg`): serve
    /// started in the foreground, Enter gives C1 (the reader then waits in
    /// a read); Ctrl-Z stops serve and `bg` resumes it in the background.
    /// Enter and text typed there go to the shell; serve is not stopped,
    /// keeps answering CLI-token requests for at least 5 s and prints no
    /// new code. `fg` then Enter gives C2; C1 is refused and C2 pairs.
    #[test]
    fn after_ctrl_z_and_bg_serve_keeps_answering_and_issues_no_code_until_fg() {
        let e = Env::new();
        let (mut sh, s, c0) = start(&e, "");
        sh.wait_foreground(s.pid, "serve started in the foreground");
        sh.typed("\n");
        let c1 = sh.wait_codes(2, "C1 after Enter", ENTER_WAIT)[1].clone();
        assert_ne!(c1, c0);

        // Ctrl-Z: job control stops serve and the shell prompts again.
        let mark = sh.mark();
        sh.typed("\u{1a}");
        let deadline = Instant::now() + WAIT;
        while !ps(s.pid, "stat=").contains('T') {
            assert!(
                Instant::now() < deadline,
                "Ctrl-Z did not stop serve; terminal:\n{}",
                sh.transcript()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        sh.wait_for("the prompt after Ctrl-Z", WAIT, |t| {
            t.get(mark..).is_some_and(|rest| rest.contains(PROMPT))
        });

        // `bg`: serve resumes as a background job.
        let mark = sh.mark();
        sh.typed("bg\n");
        sh.wait_for("the prompt after bg", WAIT, |t| {
            t.get(mark..).is_some_and(|rest| rest.contains(PROMPT))
        });
        std::thread::sleep(SETTLE);
        sh.assert_not_stopped(s.pid, "after Ctrl-Z and bg");
        let stat = ps(s.pid, "stat=");
        assert!(!stat.contains('+'), "serve is in the background: {stat:?}");

        sh.typed("\n");
        std::thread::sleep(Duration::from_millis(300));
        sh.typed("\n");
        std::thread::sleep(Duration::from_millis(300));
        sh.typed_and_read_by_the_shell(": some words typed after bg\n");
        std::thread::sleep(SETTLE);
        sh.assert_not_stopped(s.pid, "after Enter and text were typed after bg");
        assert_eq!(
            sh.codes(),
            vec![c0.clone(), c1.clone()],
            "a code was printed in the background: {}",
            sh.transcript()
        );

        let began = Instant::now();
        while began.elapsed() < ANSWERING {
            sh.assert_not_stopped(s.pid, "in the background after bg");
            for path in [HEALTH, RUNS] {
                let what = format!("GET {path} with the CLI token after bg");
                let r = s.with_token("GET", path);
                let r = sh.answered(s.pid, &what, r);
                assert_eq!(r.status, 200, "{what}: {}", r.text());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        assert_eq!(sh.codes(), vec![c0.clone(), c1.clone()]);

        // `fg`, then Enter: C2; C1 is refused and C2 pairs.
        sh.typed("fg\n");
        sh.wait_foreground(s.pid, "after fg");
        sh.typed("\n");
        let codes = sh.wait_codes(3, "C2 after fg and Enter", ENTER_WAIT);
        assert_eq!(codes.len(), 3, "{}", sh.transcript());
        assert_eq!(codes[..2], [c0, c1.clone()]);
        let c2 = codes[2].clone();
        let old = s.pair(&c1);
        assert_eq!(old.status, 401, "the replaced C1: {}", old.text());
        assert!(old.body.contains("pairing-code-invalid"), "{}", old.text());
        session_of(&s.pair(&c2), s.port);
    }
}
