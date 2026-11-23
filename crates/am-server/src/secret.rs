//! The per-launch secrets' plumbing (AD-13): OS randomness, constant-time
//! comparison and the CLI token, whose `Debug` never shows its value.

use std::sync::Arc;

/// `bytes` OS-random bytes, hex-encoded.
pub(crate) fn random_hex(bytes: usize) -> String {
    hex::encode(random_bytes(bytes))
}

/// `n` bytes from the OS RNG. The OS RNG failing is unrecoverable: no
/// secret may be made from anything weaker.
pub(crate) fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).expect("the OS random number generator failed");
    buf
}

/// Equal in time independent of where the first difference is (the
/// lengths of the values compared here are public).
pub(crate) fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

/// The per-launch CLI token (AD-13, AD-15): 256 OS-random bits as 64 hex
/// digits, held in memory and written only to its own owner-only file by
/// `agentic serve`. It authenticates only CLI-class routes, and never a
/// request that carries `Origin`.
#[derive(Clone)]
pub struct CliToken(Arc<str>);

impl CliToken {
    pub(crate) fn generate() -> CliToken {
        CliToken(Arc::from(random_hex(32)))
    }

    /// The value, for its file and for the constant-time check only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub(crate) fn matches(&self, presented: &str) -> bool {
        ct_eq(&self.0, presented)
    }
}

impl std::fmt::Debug for CliToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CliToken(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_compares_whole_values() {
        assert!(ct_eq("abcd", "abcd"));
        assert!(!ct_eq("abcd", "abce"));
        assert!(!ct_eq("abcd", "abc"));
        assert!(ct_eq("", ""));
    }

    #[test]
    fn the_token_is_256_bits_of_hex_and_debug_is_redacted() {
        let t = CliToken::generate();
        assert_eq!(t.expose().len(), 64);
        assert!(t.expose().chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!format!("{t:?}").contains(t.expose()));
        assert_ne!(t.expose(), CliToken::generate().expose());
    }
}
