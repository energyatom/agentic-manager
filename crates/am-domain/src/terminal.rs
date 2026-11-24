//! The `Terminal` port (story 2.1, AD-15): the owner's interactive terminal,
//! where the interim CLI Account edit reads the typed Account id and the key.
//!
//! The real implementation lives in am-app (termios through `nix`); the only
//! fake lives in am-testkit, a dev-dependency, so no release binary can be
//! given a scripted terminal (a guard test checks the dependency graph). The
//! port is synchronous and owned-typed.

use std::time::Duration;

use crate::SecretString;

/// Why a read from the terminal returned nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TerminalError {
    /// Ctrl-C: typed at the prompt, or a SIGINT while it waited.
    #[error("interrupted (Ctrl-C)")]
    Interrupted,
    /// Nothing was typed within the time given.
    #[error("nothing was typed in time")]
    TimedOut,
    /// The input ended (Ctrl-D, or the terminal closed).
    #[error("the terminal's input ended")]
    Closed,
    #[error("the terminal cannot be read: {0}")]
    Io(String),
}

/// The owner's terminal.
pub trait Terminal: Send {
    /// True only when stdin and stdout are both a real interactive terminal.
    fn interactive(&self) -> bool;
    /// Writes `text` to the terminal itself (a prompt), as it is.
    fn show(&mut self, text: &str);
    /// Reads one line with echo on, without its line ending; `TimedOut` when
    /// `timeout` is given and passes first. Only a line ended by Return is
    /// a line: a partial line followed by the end of input is `Closed`
    /// (story 3.4, R11), so it never answers a prompt.
    fn read_line(&mut self, timeout: Option<Duration>) -> Result<String, TerminalError>;
    /// Drops whatever was typed ahead and not read yet (story 3.4, R9): a
    /// command that asks something calls it just before it shows the
    /// prompt, so lines typed or pasted earlier never answer it. A pending
    /// interrupt is kept.
    fn discard_typed_ahead(&mut self);
    /// Reads one line with echo off. Echo is on again when this returns,
    /// whatever it returns (a line, an interrupt, the end of input, an
    /// error), and when the implementation is dropped mid-read.
    fn read_secret(&mut self) -> Result<SecretString, TerminalError>;
    /// True when a Ctrl-C (or another signal that ends the command) arrived
    /// that no read has reported yet; asking consumes it. A command checks
    /// this between its last prompt and each thing it then stores or
    /// writes, because an interrupt is otherwise acted on only while a read
    /// waits.
    fn interrupted(&mut self) -> bool;
}
