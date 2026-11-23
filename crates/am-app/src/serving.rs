//! What `agentic serve` does around the server (story 3.2, AD-13): the
//! owner-only check at start, and the per-launch CLI token's file.
//!
//! - [`owner_only_at_start`]: before it binds, the server refuses to start,
//!   naming the path and the `chmod` that fixes it, when the app data dir
//!   or the open project's runtime folder, or any folder inside either at
//!   any depth, is not owner-only (0700), or a file in one of them can be
//!   read by others (not 0600); a symbolic link anywhere inside refuses it
//!   too, and so does a folder that cannot be looked at (only a missing one
//!   passes). It is 2.12's check ([`crate::user::owner_only`]) extended to
//!   the whole tree and to the runtime folder (review P1).
//! - [`write_cli_token`]: the token's 64 hex digits in
//!   `AppDataLayout::cli_token(pid)` = `<app data dir>/serving/<pid>.token`,
//!   0600 in the 0700 `serving` folder (both created owner-only), beside
//!   the port record; [`clear_cli_token`] removes it at exit. A stale file
//!   from a dead process is harmless: the server compares against the value
//!   it holds in memory.
//! - [`serve_is_foreground`] and [`ForegroundInput`] (story 3.19): the
//!   Enter reader reads the terminal only while serve is its foreground
//!   job, asked of the system's `ps` before each read, so `agentic serve &`
//!   is never stopped by job control and keeps answering.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use am_domain::RuntimeLayout;
use am_server::CliToken;

use crate::runtime::{Places, Refusal, ensure_private_dir, replace_file};

/// Refuses the start when a folder the server keeps state in, or anything
/// inside it at any depth, is not owner-only.
pub fn owner_only_at_start(
    places: &Places,
    runtime: Option<&RuntimeLayout>,
) -> Result<(), Refusal> {
    crate::user::owner_only(&places.user).map_err(Refusal)?;
    owner_only_tree(&places.app_data, "this Mac's app data folder")?;
    if let Some(rt) = runtime {
        owner_only_tree(rt.dir(), "the project's runtime folder")?;
    }
    Ok(())
}

/// The words of a start refused for `path`.
#[cfg(unix)]
fn refused(words: String) -> Refusal {
    Refusal(format!(
        "agentic serve did not start: {words}, then run agentic serve again"
    ))
}

/// The tree at `top`: every folder 0700 and every other entry 0600, at any
/// depth, and no symbolic link anywhere (the top included). A missing `top`
/// passes (it is created owner-only); any other error looking at it
/// refuses. An entry that vanishes during the walk (a temporary file
/// renamed meanwhile) is skipped.
#[cfg(unix)]
fn owner_only_tree(top: &Path, what: &str) -> Result<(), Refusal> {
    match std::fs::symlink_metadata(top) {
        Ok(m) => owner_only_entry(top, &m, what),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(refused(format!(
            "{what} {} cannot be checked ({e}); make sure you own it and it is reachable, run `chmod 700 {}`",
            top.display(),
            top.display()
        ))),
    }
}

#[cfg(unix)]
fn owner_only_entry(path: &Path, m: &std::fs::Metadata, what: &str) -> Result<(), Refusal> {
    use std::os::unix::fs::PermissionsExt;
    let mode = m.permissions().mode() & 0o777;
    let ft = m.file_type();
    if ft.is_symlink() {
        return Err(refused(format!(
            "{} is a symbolic link inside {what}, which must hold only its own files and folders; remove it (`rm {}`)",
            path.display(),
            path.display()
        )));
    }
    if !ft.is_dir() {
        if mode & 0o077 != 0 {
            return Err(refused(format!(
                "{} can be read by other users (mode {mode:o}); run `chmod 600 {}`",
                path.display(),
                path.display()
            )));
        }
        return Ok(());
    }
    if mode & 0o077 != 0 {
        return Err(refused(format!(
            "{} in {what} can be read or entered by other users (mode {mode:o}); run `chmod 700 {}`",
            path.display(),
            path.display()
        )));
    }
    let cannot_read = |e: std::io::Error| {
        refused(format!(
            "{} in {what} cannot be read ({e}); run `chmod 700 {}`",
            path.display(),
            path.display()
        ))
    };
    for entry in std::fs::read_dir(path).map_err(cannot_read)? {
        let entry = entry.map_err(cannot_read)?;
        let child = entry.path();
        match std::fs::symlink_metadata(&child) {
            Ok(m) => owner_only_entry(&child, &m, what)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(refused(format!(
                    "{} in {what} cannot be checked ({e}); run `chmod 600 {}` (a file) or `chmod 700 {}` (a folder)",
                    child.display(),
                    child.display(),
                    child.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn owner_only_tree(_top: &Path, _what: &str) -> Result<(), Refusal> {
    Ok(())
}

/// Writes this launch's CLI token, owner-only, to
/// `<app data dir>/serving/<pid>.token` (the folders created 0700 when
/// missing); returns the file's path.
pub fn write_cli_token(places: &Places, pid: u32, token: &CliToken) -> Result<PathBuf, Refusal> {
    ensure_private_dir(&places.app_data)?;
    ensure_private_dir(&places.user.serving_dir())?;
    let path = places.user.cli_token(pid);
    replace_file(&path, format!("{}\n", token.expose()).as_bytes())?;
    Ok(path)
}

/// Removes the CLI token file of process `pid` (at `serve`'s exit).
pub fn clear_cli_token(places: &Places, pid: u32) {
    let _ = std::fs::remove_file(places.user.cli_token(pid));
}

/// Whether `agentic serve`'s stdin is a terminal, so pressing Enter there
/// can print a new pairing code (review P9). With stdin redirected (a
/// service, `< /dev/null`, a test) nothing reads it.
pub fn stdin_reads_the_owner() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal()
}

/// How long [`serve_is_foreground`] waits for `ps` before it counts the
/// check as failed (not the foreground).
#[cfg(target_os = "macos")]
const PS_TIMEOUT: Duration = Duration::from_secs(2);
/// The most of `ps`'s output the check reads; more is unexpected output.
const PS_OUTPUT_LIMIT: u64 = 4096;

/// Whether this `agentic serve` is its terminal's foreground job (story
/// 3.19), so the Enter reader may read stdin: a read of the terminal from a
/// background job gets SIGTTIN and job control stops the whole server. It
/// asks the system's `/bin/ps` for this process's state, which carries a
/// `+` when the process is in the foreground process group of its
/// controlling terminal (the kernel's own record), through
/// [`serve_is_foreground_with`]; any failure is "not the foreground". Off
/// macOS it is never the foreground (AD-47): nothing reads stdin there.
#[cfg(target_os = "macos")]
pub fn serve_is_foreground() -> bool {
    serve_is_foreground_with(Path::new("/bin/ps"), std::process::id(), PS_TIMEOUT)
}

/// Off macOS the foreground check is not supported yet (AD-47): never the
/// foreground, so the Enter reader never reads stdin.
#[cfg(not(target_os = "macos"))]
pub fn serve_is_foreground() -> bool {
    false
}

/// The foreground check with the `ps` program, the pid and the timeout
/// given: runs `<ps> -o stat= -p <pid>` with an empty environment, in `/`,
/// stdin from `/dev/null` and stderr dropped, and reads its answer through
/// [`foreground_from_ps_stat`]. `false` when the program (or the thread
/// that reads its answer) cannot be started, when it does not answer and
/// exit within `timeout` (it is then killed and reaped), when it exits
/// non-zero, or when its output is not one state word with a `+`.
pub fn serve_is_foreground_with(ps: &Path, pid: u32, timeout: Duration) -> bool {
    use std::process::{Command, Stdio};

    let deadline = Instant::now() + timeout;
    let Ok(mut child) = Command::new(ps)
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    // The answer is read on its own thread so that a `ps` that never
    // closes its output cannot hold the check past the deadline. No thread,
    // no answer: not the foreground.
    let answer = child.stdout.take().and_then(|stdout| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("agentic-ps-answer".into())
            .spawn(move || {
                let mut out = Vec::new();
                let read = stdout.take(PS_OUTPUT_LIMIT + 1).read_to_end(&mut out);
                let _ = tx.send(read.map(|_| out));
            })
            .ok()?;
        Some(rx)
    });
    let Some(answer) = answer else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    let output = answer
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
        .and_then(Result::ok);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => break None,
        }
    };
    let (Some(status), Some(output)) = (status, output) else {
        // No answer in time (or no way to read it): end it, leave no zombie.
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    status.success()
        && output.len() as u64 <= PS_OUTPUT_LIMIT
        && std::str::from_utf8(&output).is_ok_and(foreground_from_ps_stat)
}

/// The parse of `ps -o stat=` output: `true` only when, trimmed, it is one
/// word on one line that starts with an ASCII uppercase letter (the process
/// state) and holds a `+` (in the foreground process group of its
/// controlling terminal), as in `S+` or `Ss+`; `false` for anything else (no
/// `+`, nothing, a header, several lines or words, an error message).
pub fn foreground_from_ps_stat(stat: &str) -> bool {
    let mut words = stat.split_whitespace();
    match (words.next(), words.next()) {
        (Some(word), None) => {
            word.starts_with(|c: char| c.is_ascii_uppercase()) && word.contains('+')
        }
        _ => false,
    }
}

/// The shortest and longest wait between the Enter reader's foreground
/// checks while serve is in the background.
const RECHECK_FIRST: Duration = Duration::from_secs(1);
const RECHECK_CAP: Duration = Duration::from_secs(10);

/// The Enter reader's wait before its next foreground check (story 3.19,
/// review P3), given the previous wait and whether the last check passed:
/// after a failing check the wait doubles from 1 s (`Duration::ZERO`, no
/// wait yet, gives 1 s) to a cap of 10 s; after a passing check it is back
/// to 1 s. A serve left in the background runs `ps` at most every 10 s.
pub fn next_recheck(prev: Duration, passed: bool) -> Duration {
    if passed || prev.is_zero() {
        RECHECK_FIRST
    } else {
        prev.saturating_mul(2).min(RECHECK_CAP)
    }
}

/// The Enter reader's input (story 3.19): the terminal, read only while
/// `may_read` says serve is the terminal's foreground job. Before each read
/// (each [`BufRead::fill_buf`], buffered or not) it first asks
/// `still_terminal`: when stdin is no longer a terminal the read is the end
/// of input at once (nothing read, no wait), so the reader thread ends.
/// Then it asks `may_read`; while the answer is no it calls `sleep` with a
/// wait that backs off by [`next_recheck`] (1 s doubling to 10 s, back to
/// 1 s after a passing check) and asks both again, never touching the
/// terminal, so a background serve is never stopped by SIGTTIN and issues
/// no code whatever is typed. Brought to the foreground (`fg`), a check
/// passes within the current wait and reading resumes.
///
/// A read already waiting when the owner presses Ctrl-Z was seen (macOS,
/// 2026-11-24) to end interrupted; `read_line` retries it through
/// `fill_buf`, so it comes back to this check and `bg` leaves serve
/// running. What it cannot rule out: serve stopped and sent to the
/// background in the instant between a passing check and the read it
/// allows, which job control then stops until `fg`. `agentic serve <
/// /dev/null &` never reads the terminal.
pub struct ForegroundInput<R> {
    inner: R,
    may_read: Box<dyn FnMut() -> bool + Send>,
    still_terminal: Box<dyn FnMut() -> bool + Send>,
    sleep: Box<dyn FnMut(Duration) + Send>,
    /// The wait before the next check after a failing one.
    wait: Duration,
}

impl<R: BufRead> ForegroundInput<R> {
    /// `inner` read only while `still_terminal` and `may_read` answer yes;
    /// `sleep` waits between checks (serve passes `std::thread::sleep`).
    pub fn new(
        inner: R,
        may_read: Box<dyn FnMut() -> bool + Send>,
        still_terminal: Box<dyn FnMut() -> bool + Send>,
        sleep: Box<dyn FnMut(Duration) + Send>,
    ) -> Self {
        ForegroundInput {
            inner,
            may_read,
            still_terminal,
            sleep,
            wait: next_recheck(Duration::ZERO, false),
        }
    }
}

impl<R: BufRead> Read for ForegroundInput<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for ForegroundInput<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        loop {
            if !(self.still_terminal)() {
                return Ok(&[]);
            }
            if (self.may_read)() {
                self.wait = next_recheck(self.wait, true);
                return self.inner.fill_buf();
            }
            (self.sleep)(self.wait);
            self.wait = next_recheck(self.wait, false);
        }
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Cursor, Read};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::ForegroundInput;

    /// A terminal stand-in that counts the reads it was asked for.
    struct Counted {
        inner: Cursor<Vec<u8>>,
        reads: Arc<AtomicUsize>,
    }

    impl Read for Counted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.read(buf)
        }
    }

    impl BufRead for Counted {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.fill_buf()
        }
        fn consume(&mut self, amt: usize) {
            self.inner.consume(amt);
        }
    }

    /// While the check says "background", the input is not touched; once it
    /// says "foreground" each line is read, and every read is asked first.
    #[test]
    fn the_input_is_read_only_after_the_check_passes_and_asked_before_each_read() {
        let reads = Arc::new(AtomicUsize::new(0));
        let asked = Arc::new(AtomicUsize::new(0));
        let waits = Arc::new(Mutex::new(Vec::new()));
        let (r, a, w) = (reads.clone(), asked.clone(), waits.clone());
        let mut input = ForegroundInput::new(
            Counted {
                inner: Cursor::new(b"\n\n".to_vec()),
                reads: reads.clone(),
            },
            // Background for the first three checks, then the foreground.
            Box::new(move || {
                let n = a.fetch_add(1, Ordering::SeqCst);
                if n < 3 {
                    assert_eq!(r.load(Ordering::SeqCst), 0, "read in the background");
                }
                n >= 3
            }),
            Box::new(|| true),
            Box::new(move |d| w.lock().unwrap().push(d)),
        );
        let mut line = String::new();
        assert_eq!(input.read_line(&mut line).unwrap(), 1);
        assert_eq!(asked.load(Ordering::SeqCst), 4);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            *waits.lock().unwrap(),
            [1, 2, 4].map(Duration::from_secs),
            "one wait after each failing check"
        );
        line.clear();
        assert_eq!(input.read_line(&mut line).unwrap(), 1);
        line.clear();
        assert_eq!(input.read_line(&mut line).unwrap(), 0);
        // One check per read of the input.
        assert_eq!(
            asked.load(Ordering::SeqCst),
            3 + reads.load(Ordering::SeqCst)
        );
    }
}
