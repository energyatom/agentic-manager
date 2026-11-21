//! Story 1.14 acceptance tests (Consistency Conventions: Globs): `am-glob`
//! is the one glob matcher. One `Glob` type over globset with
//! `literal_separator(true)` (`*` and `?` stay in one path segment, `**`
//! crosses them), pattern and subject both NFC-normalised, case-insensitive
//! (linked-folder `include`/`exclude`) or case-sensitive (Source URL-path
//! scopes) by constructor, plus a `regex()` form for a crawl service. Matrix
//! row "Globs". Written before the crate has a `src/` folder.
//!
//! The API these tests expect:
//! - `Glob::case_insensitive(&str) -> Result<Glob, GlobError>`;
//! - `Glob::case_sensitive(&str) -> Result<Glob, GlobError>`;
//! - `is_match(&self, subject: &str) -> bool`;
//! - `regex(&self)`, the regex form as text (`&str` or `String`);
//! - `GlobError: Display`, the reason in words.
#![forbid(unsafe_code)]

use am_glob::{Glob, GlobError};

fn folder(p: &str) -> Glob {
    Glob::case_insensitive(p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn url(p: &str) -> Glob {
    Glob::case_sensitive(p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// `café` with a precomposed é (NFC) and with e + combining acute (NFD).
const CAFE_NFC: &str = "caf\u{e9}";
const CAFE_NFD: &str = "cafe\u{301}";

// ------------------------------------------------------------------ case

#[test]
fn a_folder_glob_matches_ignoring_case() {
    let g = folder("*.PDF");
    assert!(g.is_match("Report.pdf"));
    assert!(g.is_match("report.PDF"));
    assert!(!g.is_match("Report.pdfx"));
    assert!(folder("Reports/**").is_match("reports/2026/a.txt"));
}

#[test]
fn a_url_scope_glob_keeps_case() {
    let g = url("*.PDF");
    assert!(!g.is_match("Report.pdf"));
    assert!(g.is_match("Report.PDF"));
    let docs = url("/docs/**");
    assert!(docs.is_match("/docs/a/b.html"));
    assert!(!docs.is_match("/Docs/a/b.html"));
}

// ------------------------------------------------------------------ NFC

#[test]
fn an_nfd_subject_matches_an_nfc_pattern_and_the_reverse() {
    assert_ne!(CAFE_NFC, CAFE_NFD, "two spellings of one name");
    for make in [folder, url] {
        let nfc = make(&format!("{CAFE_NFC}/*.txt"));
        assert!(nfc.is_match(&format!("{CAFE_NFD}/notes.txt")));
        assert!(nfc.is_match(&format!("{CAFE_NFC}/notes.txt")));
        let nfd = make(&format!("{CAFE_NFD}/*.txt"));
        assert!(nfd.is_match(&format!("{CAFE_NFC}/notes.txt")));
        assert!(nfd.is_match(&format!("{CAFE_NFD}/notes.txt")));
        // The subject alone in NFD against a literal NFC name.
        let name = make(&format!("{CAFE_NFC}.txt"));
        assert!(name.is_match(&format!("{CAFE_NFD}.txt")));
    }
}

#[test]
fn the_regex_form_is_the_normalised_pattern() {
    let nfc = folder(&format!("{CAFE_NFC}/*.txt")).regex().to_string();
    let nfd = folder(&format!("{CAFE_NFD}/*.txt")).regex().to_string();
    assert_eq!(nfc, nfd);
    let scope = url("/docs/**").regex().to_string();
    assert!(scope.contains("docs"), "{scope}");
    assert!(!scope.contains("(?i"), "a URL scope keeps case: {scope}");
    let any_case = folder("*.PDF").regex().to_string();
    assert!(any_case.contains("(?i"), "{any_case}");
}

// ------------------------------------------------------------------ segments

#[test]
fn a_star_stays_in_one_path_segment() {
    for make in [folder, url] {
        let one = make("a/*");
        assert!(one.is_match("a/b"));
        assert!(!one.is_match("a/b/c"));
        let any = make("a/**");
        assert!(any.is_match("a/b/c"));
        assert!(any.is_match("a/b"));
        let q = make("a?b");
        assert!(q.is_match("axb"));
        assert!(!q.is_match("a/b"));
        let deep = make("**/*.txt");
        assert!(deep.is_match("x/y/z.txt"));
        assert!(deep.is_match("z.txt"));
        assert!(!make("*.txt").is_match("x/z.txt"), "anchored at the start");
    }
}

// ------------------------------------------------------------------ errors

#[test]
fn a_bad_glob_is_refused_in_words() {
    for p in ["[x", "a/[b", "{a,b"] {
        let e: GlobError = match Glob::case_insensitive(p) {
            Ok(_) => panic!("{p} compiles"),
            Err(e) => e,
        };
        assert!(!e.to_string().is_empty(), "{p}");
        assert!(Glob::case_sensitive(p).is_err(), "{p}");
    }
}
