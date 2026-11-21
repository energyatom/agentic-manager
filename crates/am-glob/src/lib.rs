//! The one glob matcher (architecture Design Paradigm table, Consistency
//! Conventions: Globs). Every path glob in the workspace is compiled and
//! matched here, through one [`Glob`] type over `globset`:
//!
//! - `literal_separator(true)`: `*` and `?` stay inside one path segment,
//!   `**` crosses segments; a glob is anchored at both ends;
//! - pattern and subject are both NFC-normalised, so a name written with a
//!   combining accent (as macOS file systems may report it) matches a
//!   pattern written precomposed, and the reverse;
//! - [`Glob::case_insensitive`] for linked-folder `include`/`exclude` and
//!   fan-out routes (the path relative to the linked root, `/`-separated);
//!   [`Glob::case_sensitive`] for Source URL-path scopes;
//! - [`Glob::regex`] gives the regex form a crawl service is sent, after
//!   which every returned page is re-filtered with the same matcher.
//!
//! No IO; no workspace crate; no `globset` type leaves this crate.
#![forbid(unsafe_code)]

use unicode_normalization::UnicodeNormalization;

/// A compiled glob.
#[derive(Debug, Clone)]
pub struct Glob {
    /// The pattern as written.
    pattern: String,
    matcher: globset::GlobMatcher,
    regex: String,
}

/// Why a pattern does not compile, in words (the pattern is not repeated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobError(String);

impl std::fmt::Display for GlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GlobError {}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}

impl Glob {
    fn build(pattern: &str, case_insensitive: bool) -> Result<Glob, GlobError> {
        let glob = globset::GlobBuilder::new(&nfc(pattern))
            .literal_separator(true)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| GlobError(e.kind().to_string()))?;
        Ok(Glob {
            pattern: pattern.to_string(),
            regex: glob.regex().to_string(),
            matcher: glob.compile_matcher(),
        })
    }

    /// A glob that matches ignoring case (linked-folder patterns).
    pub fn case_insensitive(pattern: &str) -> Result<Glob, GlobError> {
        Glob::build(pattern, true)
    }

    /// A glob that keeps case (Source URL-path scopes).
    pub fn case_sensitive(pattern: &str) -> Result<Glob, GlobError> {
        Glob::build(pattern, false)
    }

    /// True when the whole of `subject` (NFC-normalised) matches.
    pub fn is_match(&self, subject: &str) -> bool {
        self.matcher.is_match(nfc(subject))
    }

    /// The pattern as written.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The regex form of the NFC-normalised pattern (`(?i)` first when it
    /// matches ignoring case).
    pub fn regex(&self) -> &str {
        &self.regex
    }
}
