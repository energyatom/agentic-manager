//! The owner's real terminal (story 2.1, AD-15): the `Terminal` port over
//! stdin, stdout and termios (`nix`, feature `term`).
//!
//! - [`interactive`] is the one predicate of every interactive-TTY rule in
//!   AD-15 and AD-32: stdin and stdout are both a terminal. The interim CLI
//!   Account edit and `agentic serve`'s `--confirm-move` / `--take-over`
//!   prompt both ask it; nothing else decides.
//! - [`OwnerTerminal`] is the only `Terminal` this crate holds, and what the
//!   `agentic` binary is built with. The scripted one lives in am-testkit, a
//!   dev-dependency: no env var, flag or feature swaps it in here.
//! - The key is read with `ECHO`, `ICANON` and `ISIG` off, so a typed `^C`
//!   arrives as a byte and cancels. A drop guard puts the terminal's
//!   settings back on every way out of the read (a line, `^C`, the end of
//!   input, an error, a panic's unwind). A SIGINT, SIGTERM, SIGHUP or
//!   SIGQUIT sent to the process is taken through `tokio::signal`,
//!   installed before echo is turned off, and ends the read as an
//!   interrupt, so the same guard runs; none kills the process with echo
//!   off. One that arrives between reads is kept and reported by
//!   `interrupted()`, which the command asks before it stores or writes.
//! - Bytes are read by one helper thread, one byte per request, so a read
//!   can be given a time limit (the 2 minutes of AD-15's typed id) and can
//!   be interrupted, and nothing typed ahead is taken from the shell.

use std::io::IsTerminal;

/// True only when stdin and stdout are both a real interactive terminal.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

#[cfg(unix)]
pub use unix::OwnerTerminal;

#[cfg(not(unix))]
pub use other::OwnerTerminal;

#[cfg(unix)]
mod unix {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
    use std::time::{Duration, Instant};

    use am_domain::{SecretString, Terminal, TerminalError};
    use nix::sys::termios::{
        LocalFlags, SetArg, SpecialCharacterIndices, Termios, tcgetattr, tcsetattr,
    };

    const CTRL_C: u8 = 0x03;
    const CTRL_D: u8 = 0x04;
    const CTRL_U: u8 = 0x15;
    const BACKSPACE: u8 = 0x08;
    const DELETE: u8 = 0x7f;

    /// What a read waits for.
    enum Event {
        Byte(u8),
        /// The input ended.
        End,
        Failed(String),
        /// SIGINT, SIGTERM, SIGHUP or SIGQUIT.
        Interrupt,
    }

    /// The helper thread and the signal watcher, started at the first read.
    struct Input {
        events: Receiver<Event>,
        /// Asks the helper thread for one more byte.
        want: Sender<()>,
        /// A byte was asked for and has not arrived yet.
        asked: bool,
        /// Events taken off the channel while looking for a pending
        /// interrupt, kept for the next read.
        held: VecDeque<Event>,
    }

    /// Puts the terminal's settings back when dropped.
    struct Restore {
        tty: OwnedFd,
        saved: Termios,
    }

    impl Drop for Restore {
        fn drop(&mut self) {
            // TCSAFLUSH also discards what was typed and not read (the rest
            // of a key after `^C`), so it never reaches the shell.
            if tcsetattr(&self.tty, SetArg::TCSAFLUSH, &self.saved).is_err() {
                let _ = tcsetattr(&self.tty, SetArg::TCSANOW, &self.saved);
            }
        }
    }

    /// The terminal the `agentic` process runs in.
    #[derive(Default)]
    pub struct OwnerTerminal {
        input: Option<Input>,
    }

    impl OwnerTerminal {
        pub fn new() -> OwnerTerminal {
            OwnerTerminal::default()
        }

        /// Starts the helper thread and the SIGINT watcher once. The signal
        /// is installed here, before any read and so before echo is turned
        /// off; without a tokio runtime to carry it, nothing is read.
        fn input(&mut self) -> Result<&mut Input, TerminalError> {
            if self.input.is_none() {
                let io = |what: &str, e: &dyn std::fmt::Display| {
                    TerminalError::Io(format!("{what}: {e}"))
                };
                let runtime = tokio::runtime::Handle::try_current()
                    .map_err(|e| io("Ctrl-C cannot be watched", &e))?;
                let (events_tx, events) = channel::<Event>();
                let (want, wanted) = channel::<()>();
                // Every signal whose default action would end the process
                // (and so leave echo off): each ends the read instead.
                use tokio::signal::unix::{SignalKind, signal};
                for kind in [
                    SignalKind::interrupt(),
                    SignalKind::terminate(),
                    SignalKind::hangup(),
                    SignalKind::quit(),
                ] {
                    let mut signals = {
                        let _in_runtime = runtime.enter();
                        signal(kind).map_err(|e| io("signals cannot be watched", &e))?
                    };
                    let on_signal = events_tx.clone();
                    runtime.spawn(async move {
                        while signals.recv().await.is_some() {
                            if on_signal.send(Event::Interrupt).is_err() {
                                break;
                            }
                        }
                    });
                }
                let stdin = std::io::stdin()
                    .as_fd()
                    .try_clone_to_owned()
                    .map_err(|e| io("stdin cannot be opened", &e))?;
                let mut stdin = std::fs::File::from(stdin);
                std::thread::Builder::new()
                    .name("agentic-terminal".into())
                    .spawn(move || {
                        let mut byte = [0u8; 1];
                        while wanted.recv().is_ok() {
                            let event = match stdin.read(&mut byte) {
                                Ok(0) => Event::End,
                                Ok(_) => Event::Byte(byte[0]),
                                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                                    Event::Interrupt
                                }
                                Err(e) => Event::Failed(e.to_string()),
                            };
                            if events_tx.send(event).is_err() {
                                break;
                            }
                        }
                    })
                    .map_err(|e| io("the terminal reader cannot start", &e))?;
                self.input = Some(Input {
                    events,
                    want,
                    asked: false,
                    held: VecDeque::new(),
                });
            }
            self.input
                .as_mut()
                .ok_or_else(|| TerminalError::Io("the terminal reader is gone".into()))
        }

        /// The next byte typed, or why there is none; `deadline` bounds the
        /// wait.
        fn next(&mut self, deadline: Option<Instant>) -> Result<u8, TerminalError> {
            let input = self.input()?;
            if !input.asked && input.held.is_empty() {
                input
                    .want
                    .send(())
                    .map_err(|_| TerminalError::Io("the terminal reader stopped".into()))?;
                input.asked = true;
            }
            let event = match (input.held.pop_front(), deadline) {
                (Some(held), _) => held,
                (None, None) => input
                    .events
                    .recv()
                    .map_err(|_| TerminalError::Io("the terminal reader stopped".into()))?,
                (None, Some(at)) => {
                    let left = at.saturating_duration_since(Instant::now());
                    match input.events.recv_timeout(left) {
                        Ok(e) => e,
                        Err(RecvTimeoutError::Timeout) => return Err(TerminalError::TimedOut),
                        Err(RecvTimeoutError::Disconnected) => {
                            return Err(TerminalError::Io("the terminal reader stopped".into()));
                        }
                    }
                }
            };
            match event {
                Event::Byte(b) => {
                    input.asked = false;
                    Ok(b)
                }
                Event::End => {
                    input.asked = false;
                    Err(TerminalError::Closed)
                }
                Event::Failed(e) => {
                    input.asked = false;
                    Err(TerminalError::Io(e))
                }
                // The asked-for byte is still owed: `asked` stays.
                Event::Interrupt => Err(TerminalError::Interrupted),
            }
        }

        /// One line typed with the terminal's settings as they are.
        fn line(&mut self, timeout: Option<Duration>) -> Result<Vec<u8>, TerminalError> {
            let deadline = timeout.map(|t| Instant::now() + t);
            let mut line = Vec::new();
            loop {
                match self.next(deadline) {
                    Ok(b'\n') => return Ok(line),
                    Ok(b) => line.push(b),
                    Err(TerminalError::Closed) if !line.is_empty() => return Ok(line),
                    Err(e) => return Err(e),
                }
            }
        }

        /// One line typed with echo, canonical mode and signals off: `^C`
        /// cancels, Backspace and `^U` edit, Return ends it.
        fn hidden_line(&mut self) -> Result<Vec<u8>, TerminalError> {
            let mut key = Vec::new();
            loop {
                match self.next(None) {
                    Ok(b'\n' | b'\r') => return Ok(key),
                    Ok(CTRL_C) => {
                        wipe(&mut key);
                        return Err(TerminalError::Interrupted);
                    }
                    Ok(CTRL_D) if key.is_empty() => return Err(TerminalError::Closed),
                    Ok(CTRL_D) => return Ok(key),
                    Ok(BACKSPACE | DELETE) => {
                        // One character: its continuation bytes, then its lead.
                        while key.pop().is_some_and(|b| b & 0xc0 == 0x80) {}
                    }
                    Ok(CTRL_U) => wipe(&mut key),
                    Ok(b) if b < 0x20 => {}
                    Ok(b) => key.push(b),
                    Err(TerminalError::Closed) if !key.is_empty() => return Ok(key),
                    Err(e) => {
                        wipe(&mut key);
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Overwrites and empties a buffer that held (part of) a key.
    fn wipe(bytes: &mut Vec<u8>) {
        bytes.iter_mut().for_each(|b| *b = 0);
        bytes.clear();
    }

    fn text(bytes: Vec<u8>) -> Result<String, TerminalError> {
        String::from_utf8(bytes).map_err(|e| {
            let mut bytes = e.into_bytes();
            wipe(&mut bytes);
            TerminalError::Io("what was typed is not UTF-8 text".into())
        })
    }

    impl Terminal for OwnerTerminal {
        fn interactive(&self) -> bool {
            super::interactive()
        }

        fn show(&mut self, text: &str) {
            let mut out = std::io::stdout();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }

        fn read_line(&mut self, timeout: Option<Duration>) -> Result<String, TerminalError> {
            let mut line = text(self.line(timeout)?)?;
            if line.ends_with('\r') {
                line.pop();
            }
            Ok(line)
        }

        fn read_secret(&mut self) -> Result<SecretString, TerminalError> {
            // The SIGINT watcher first: from here a signal ends the read
            // instead of the process.
            self.input()?;
            let io =
                |what: &str, e: &dyn std::fmt::Display| TerminalError::Io(format!("{what}: {e}"));
            let tty = std::io::stdin()
                .as_fd()
                .try_clone_to_owned()
                .map_err(|e| io("stdin cannot be opened", &e))?;
            let saved =
                tcgetattr(&tty).map_err(|e| io("the terminal's settings cannot be read", &e))?;
            let mut quiet = saved.clone();
            quiet
                .local_flags
                .remove(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::ISIG);
            quiet.control_chars[SpecialCharacterIndices::VMIN as usize] = 1;
            quiet.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
            // The guard exists before the settings change, so every way out
            // from here (a panic's unwind too) puts them back.
            let restore = Restore { tty, saved };
            tcsetattr(&restore.tty, SetArg::TCSAFLUSH, &quiet)
                .map_err(|e| io("echo cannot be turned off, so no key is read", &e))?;
            let typed = self.hidden_line();
            drop(restore);
            // Return was not echoed: end the prompt's line.
            self.show("\n");
            Ok(SecretString::new(text(typed?)?))
        }

        fn interrupted(&mut self) -> bool {
            // Before the first read no signal is watched: its default
            // action ends the process, with the terminal as it was.
            let Some(input) = self.input.as_mut() else {
                return false;
            };
            let mut found = false;
            while let Ok(event) = input.events.try_recv() {
                match event {
                    Event::Interrupt => found = true,
                    other => input.held.push_back(other),
                }
            }
            found
        }
    }
}

#[cfg(not(unix))]
mod other {
    use std::time::Duration;

    use am_domain::{SecretString, Terminal, TerminalError};

    /// Not supported on this platform yet (AD-47): never interactive, so
    /// nothing is read.
    #[derive(Default)]
    pub struct OwnerTerminal;

    impl OwnerTerminal {
        pub fn new() -> OwnerTerminal {
            OwnerTerminal
        }
    }

    fn unsupported() -> TerminalError {
        TerminalError::Io("reading from the terminal is not supported on this platform yet".into())
    }

    impl Terminal for OwnerTerminal {
        fn interactive(&self) -> bool {
            false
        }

        fn show(&mut self, text: &str) {
            print!("{text}");
        }

        fn read_line(&mut self, _timeout: Option<Duration>) -> Result<String, TerminalError> {
            Err(unsupported())
        }

        fn read_secret(&mut self) -> Result<SecretString, TerminalError> {
            Err(unsupported())
        }

        fn interrupted(&mut self) -> bool {
            false
        }
    }
}
