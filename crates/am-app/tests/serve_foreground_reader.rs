//! Story 3.19, review round 1, P3: a backgrounded serve must not run
//! `/bin/ps` every second for its whole life. Written first, against the
//! am-app API this patch adds or changes in `am_app::serving`:
//!
//! - `next_recheck(prev: Duration, passed: bool) -> Duration`: the Enter
//!   reader's wait between foreground checks. After a failing check the
//!   wait doubles, from 1 s (`Duration::ZERO` counts as "no wait yet": the
//!   first wait is 1 s) to a cap of 10 s; after a passing check it is back
//!   to 1 s.
//! - `ForegroundInput::new(inner, may_read, still_terminal, sleep)`: the
//!   terminal (`inner`, a `BufRead`), read only while `may_read` (the
//!   foreground check) answers yes. Before every read of `inner` it asks
//!   `still_terminal` (is stdin still a terminal?) and, when that answers
//!   no, ends: the read returns end of input (`Ok(0)`) without touching
//!   `inner` and without waiting, so the reader thread ends. While
//!   `may_read` answers no it calls `sleep(wait)` and asks again, the wait
//!   following `next_recheck` (1 s, 2 s, 4 s, 8 s, 10 s, 10 s...) and back
//!   to 1 s after a passing check. `may_read` and `still_terminal` are
//!   `Box<dyn FnMut() -> bool + Send>`, `sleep` is
//!   `Box<dyn FnMut(Duration) + Send>` (serve passes `serve_is_foreground`,
//!   `stdin_reads_the_owner` and `std::thread::sleep`); the tests record
//!   the waits instead of sleeping.
#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::io::{BufRead, Cursor, Read};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_app::serving::{ForegroundInput, next_recheck};

fn s(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// A terminal stand-in that counts every read it is asked for.
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

fn terminal(text: &str) -> (Counted, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    (
        Counted {
            inner: Cursor::new(text.as_bytes().to_vec()),
            reads: reads.clone(),
        },
        reads,
    )
}

/// A foreground check answering from `script`, then `rest` for ever.
fn scripted(script: &[bool], rest: bool) -> Box<dyn FnMut() -> bool + Send> {
    let mut left: VecDeque<bool> = script.iter().copied().collect();
    Box::new(move || left.pop_front().unwrap_or(rest))
}

/// A "still a terminal" check that answers what `flag` holds.
fn flag_check(flag: &Arc<AtomicBool>) -> Box<dyn FnMut() -> bool + Send> {
    let flag = flag.clone();
    Box::new(move || flag.load(Ordering::SeqCst))
}

/// What `ForegroundInput` calls to wait, and the waits a test recorded.
type Sleep = Box<dyn FnMut(Duration) + Send>;
type Waits = Arc<Mutex<Vec<Duration>>>;

/// A sleep that records the waits instead of sleeping.
fn recorded() -> (Sleep, Waits) {
    let waits = Arc::new(Mutex::new(Vec::new()));
    let w = waits.clone();
    (Box::new(move |d| w.lock().unwrap().push(d)), waits)
}

#[test]
fn the_wait_starts_at_one_second_doubles_to_a_ten_second_cap_and_resets_after_a_pass() {
    assert_eq!(next_recheck(Duration::ZERO, false), s(1));
    let mut wait = s(1);
    let mut waits = vec![wait];
    for _ in 0..6 {
        wait = next_recheck(wait, false);
        waits.push(wait);
    }
    assert_eq!(waits, [1, 2, 4, 8, 10, 10, 10].map(s));
    assert_eq!(next_recheck(s(60), false), s(10), "the cap holds");
    for prev in [Duration::ZERO, s(1), s(4), s(10)] {
        assert_eq!(
            next_recheck(prev, true),
            s(1),
            "reset after a pass from {prev:?}"
        );
    }
}

/// stdin no longer a terminal: the reader ends at once (end of input),
/// whatever the foreground check says, without reading and without waiting.
#[test]
fn the_reader_ends_at_once_when_stdin_is_no_longer_a_terminal() {
    for foreground in [true, false] {
        let (input, reads) = terminal("typed\n");
        let gone = Arc::new(AtomicBool::new(false));
        let (sleep, waits) = recorded();
        let mut r =
            ForegroundInput::new(input, scripted(&[], foreground), flag_check(&gone), sleep);
        let mut line = String::new();
        assert_eq!(
            r.read_line(&mut line).unwrap(),
            0,
            "foreground {foreground}"
        );
        assert!(line.is_empty());
        assert_eq!(reads.load(Ordering::SeqCst), 0, "the terminal was read");
        assert!(
            waits.lock().unwrap().is_empty(),
            "{:?}",
            waits.lock().unwrap()
        );
    }
}

/// In the background the waits back off (1, 2, 4, 8, 10 s) and the reader
/// ends once stdin stops being a terminal; the terminal is never read.
#[test]
fn in_the_background_the_waits_back_off_and_the_reader_ends_when_the_terminal_goes() {
    let (input, reads) = terminal("typed\n");
    let asked = Arc::new(AtomicUsize::new(0));
    let a = asked.clone();
    // A terminal for the first five asks, then gone.
    let still: Box<dyn FnMut() -> bool + Send> =
        Box::new(move || a.fetch_add(1, Ordering::SeqCst) < 5);
    let (sleep, waits) = recorded();
    let mut r = ForegroundInput::new(input, scripted(&[], false), still, sleep);
    let mut line = String::new();
    assert_eq!(r.read_line(&mut line).unwrap(), 0);
    assert_eq!(reads.load(Ordering::SeqCst), 0, "read in the background");
    let waits = waits.lock().unwrap().clone();
    let want = [1, 2, 4, 8, 10, 10].map(s);
    assert!(
        (4..=6).contains(&waits.len()) && waits[..] == want[..waits.len()],
        "the waits back off from 1 s to 10 s: {waits:?}"
    );
}

/// A passing check reads and puts the wait back to 1 s; once stdin is no
/// longer a terminal, the line still waiting on it is not read.
#[test]
fn a_passing_check_reads_and_resets_the_wait_and_a_lost_terminal_ends_the_reader() {
    let (input, reads) = terminal("a\nb\nc\n");
    let still = Arc::new(AtomicBool::new(true));
    let (sleep, waits) = recorded();
    let mut r = ForegroundInput::new(
        input,
        // Six failing checks, a pass; two failing, a pass; then passes.
        scripted(
            &[
                false, false, false, false, false, false, true, false, false, true,
            ],
            true,
        ),
        flag_check(&still),
        sleep,
    );
    let mut line = String::new();
    assert_eq!(r.read_line(&mut line).unwrap(), 2);
    assert_eq!(line, "a\n");
    assert_eq!(
        *waits.lock().unwrap(),
        [1, 2, 4, 8, 10, 10].map(s),
        "backed off while in the background"
    );
    waits.lock().unwrap().clear();

    line.clear();
    assert_eq!(r.read_line(&mut line).unwrap(), 2);
    assert_eq!(line, "b\n");
    assert_eq!(
        *waits.lock().unwrap(),
        [1, 2].map(s),
        "the wait is back to 1 s after a passing check"
    );

    let before = reads.load(Ordering::SeqCst);
    still.store(false, Ordering::SeqCst);
    line.clear();
    assert_eq!(r.read_line(&mut line).unwrap(), 0, "ended: {line:?}");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        before,
        "the terminal was read after it was no longer a terminal"
    );
}
