//! The terminal pairing code (AD-13; owner Decision 2026-11-23): 8
//! characters of RFC 4648 base32 (40 bits), single use, valid 60 s, at most
//! one outstanding. It is printed only on the `agentic serve` terminal: at
//! start and each time the owner presses Enter ([`pairing_terminal`]); no
//! route, CLI command or file issues or returns one. `POST /api/v1/pair`
//! exchanges it for the same session as the launch code. 5 counted attempts
//! in any 60 s (the 6th is refused unchecked); after 10 failures pairing is
//! locked until restart. Only an attempt that passed the Host, Origin and
//! body checks reaches [`Pairing::attempt`], so a web page cannot lock the
//! owner out. A wrong code leaves the outstanding one valid.

use std::collections::VecDeque;
use std::io::{BufRead, Write};

use jiff::Timestamp;

use crate::secret::{ct_eq, random_bytes};
use crate::state::{AppState, within};
use crate::{PAIRING_CODE_TTL, PAIRING_LOCK_AFTER, PAIRING_RATE_LIMIT, PAIRING_RATE_WINDOW};

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// A pairing code; `Debug` never shows it.
#[derive(Clone)]
pub struct PairingCode(String);

impl PairingCode {
    /// The code, for the serve terminal only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairingCode(<redacted>)")
    }
}

/// Pairing is locked after too many failures, until restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLocked;

impl std::fmt::Display for PairingLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("pairing is locked after too many wrong codes until agentic serve restarts")
    }
}

impl std::error::Error for PairingLocked {}

/// 40 OS-random bits as 8 base32 characters.
fn new_code() -> String {
    let bytes = random_bytes(5);
    let mut bits: u64 = 0;
    for b in bytes {
        bits = (bits << 8) | u64::from(b);
    }
    (0..8)
        .rev()
        .map(|i| char::from(BASE32[((bits >> (i * 5)) & 0x1f) as usize]))
        .collect()
}

/// What a counted attempt came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    Paired,
    Invalid,
    RateLimited,
    Locked,
}

#[derive(Default)]
pub(crate) struct Pairing {
    code: Option<(String, Timestamp)>,
    /// The times of the counted attempts in the rate window.
    attempts: VecDeque<Timestamp>,
    failures: u32,
    locked: bool,
}

impl Pairing {
    pub(crate) fn issue(&mut self, now: Timestamp) -> Result<PairingCode, PairingLocked> {
        if self.locked {
            return Err(PairingLocked);
        }
        let code = new_code();
        self.code = Some((code.clone(), now));
        Ok(PairingCode(code))
    }

    /// One attempt that passed Host, Origin and the body check. The lock
    /// is checked first, then the rate limit (a limited attempt is neither
    /// checked nor counted), then the code.
    pub(crate) fn attempt(&mut self, presented: &str, now: Timestamp) -> Attempt {
        if self.locked {
            return Attempt::Locked;
        }
        while self
            .attempts
            .front()
            .is_some_and(|t| !within(now, *t, PAIRING_RATE_WINDOW))
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= PAIRING_RATE_LIMIT {
            return Attempt::RateLimited;
        }
        self.attempts.push_back(now);
        let fresh = match &self.code {
            Some((code, issued)) if ct_eq(code, presented) => {
                Some(within(now, *issued, PAIRING_CODE_TTL))
            }
            _ => None,
        };
        match fresh {
            Some(true) => {
                self.code = None;
                Attempt::Paired
            }
            Some(false) => {
                // The right code, too late: it is spent.
                self.code = None;
                self.fail()
            }
            None => self.fail(),
        }
    }

    fn fail(&mut self) -> Attempt {
        self.failures += 1;
        if self.failures >= PAIRING_LOCK_AFTER {
            self.locked = true;
            self.code = None;
        }
        Attempt::Invalid
    }
}

/// Issues a pairing code and prints one line holding it; while pairing is
/// locked, prints that it is locked until restart and issues nothing.
pub fn print_pairing_code(state: &AppState, out: &mut dyn Write) -> std::io::Result<()> {
    match state.issue_pairing_code() {
        Ok(code) => writeln!(
            out,
            "agentic: pairing code {} (single use, valid 60 s; press Enter for a new one)",
            code.expose()
        )?,
        Err(PairingLocked) => writeln!(
            out,
            "agentic: pairing is locked after too many wrong codes; restart agentic serve to pair again"
        )?,
    }
    out.flush()
}

/// The serve terminal's loop: each line read (Enter) prints a new pairing
/// code; returns at the end of input.
pub fn pairing_terminal(
    state: &AppState,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        print_pairing_code(state, out)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_eight_base32_characters() {
        for _ in 0..100 {
            let c = new_code();
            assert_eq!(c.len(), 8);
            assert!(c.bytes().all(|b| BASE32.contains(&b)), "{c}");
        }
    }

    #[test]
    fn the_lock_message_holds_no_code_shaped_word() {
        let mut out = Vec::new();
        writeln!(out, "{PairingLocked}").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("locked") && text.contains("restart"));
    }
}
