//! Checks shared by every file kind: schema_version, comments, secret-looking
//! values (decision c), kebab-case ids and "did you mean" suggestions.

use crate::finding::Report;
use crate::node::{Key, Kind, Node};

/// The only config schema version this app reads (AD-7, R4).
pub(crate) const SCHEMA_VERSION: i64 = 1;

pub(crate) const COMMENT_MESSAGE: &str = "comments are not kept when the app saves this file";
const COMMENT_FIX: &str =
    "move the note into a description: field or into the project's docs, then delete the comment";

pub(crate) const SECRET_FIX: &str =
    "keep keys in the Keychain; reference an Account (models/accounts.yaml, secret_ref) instead";

/// Warns at every comment line (decision b).
pub(crate) fn comments(rep: &mut Report, lines: &[u32]) {
    for line in lines {
        rep.warning(*line, COMMENT_MESSAGE, COMMENT_FIX);
    }
}

/// The top-level `schema_version` must be present and be 1. An older file
/// is still an error until its migration is accepted (story 1.10): the fix
/// names the offer when the app can step it, and says it cannot otherwise.
pub(crate) fn schema_version(rep: &mut Report, root: &[(Key, Node)]) {
    match root.iter().find(|(k, _)| k.name == "schema_version") {
        None => rep.error(
            1,
            "missing schema_version",
            "add `schema_version: 1` as the first line",
        ),
        Some((_, v)) => match v.kind {
            Kind::Int(SCHEMA_VERSION) => {}
            Kind::Int(n) if n < SCHEMA_VERSION && crate::migrate::has_path(n) => rep.error(
                v.line,
                format!(
                    "schema_version {n} is older than this app's schema_version {SCHEMA_VERSION}; the file waits for its migration"
                ),
                migration_fix(),
            ),
            Kind::Int(n) if n < SCHEMA_VERSION => rep.error(
                v.line,
                format!(
                    "schema_version {n} is older than this app's schema_version {SCHEMA_VERSION}"
                ),
                format!(
                    "this app cannot migrate a file from schema_version {n}; rewrite the file in the schema_version {SCHEMA_VERSION} format by hand"
                ),
            ),
            Kind::Int(n) => rep.error(
                v.line,
                format!(
                    "unknown schema_version {n}; this app reads schema_version {SCHEMA_VERSION}"
                ),
                "set `schema_version: 1`; if a newer app wrote this file, update the app",
            ),
            _ => rep.error(
                v.line,
                format!(
                    "schema_version must be a whole number, not {}",
                    v.type_name()
                ),
                "set `schema_version: 1`",
            ),
        },
    }
}

/// The fix of an older file the app offers to migrate.
pub(crate) fn migration_fix() -> String {
    format!(
        "review and accept the migration the app offers for this file (it steps the file to schema_version {SCHEMA_VERSION} and keeps a History entry); do not change the number by hand"
    )
}

/// The fix of an older file that is not offered because it holds a
/// secret-looking value (the value would reach the offer's diff).
pub(crate) fn migration_blocked_fix() -> String {
    format!(
        "the file holds a secret-looking value, so its migration is not offered; remove the value by hand first (keys live in the Keychain; reference an Account instead), then the app offers the migration to schema_version {SCHEMA_VERSION}"
    )
}

/// The fix of an older file that is not offered because it, or the folder
/// `link` on its way, is a link.
pub(crate) fn migration_link_fix(file: &str, link: &str) -> String {
    if link == file {
        format!(
            "this file is a link; the app does not write through links. Replace it with the file itself, then the migration to schema_version {SCHEMA_VERSION} is offered"
        )
    } else {
        format!(
            "{link}/ is a link; the app does not write through links. Replace the link with the folder itself, then the migration to schema_version {SCHEMA_VERSION} is offered"
        )
    }
}

/// The fix of an older file that a migration step refuses (`why` is the
/// step's reason, never the file's content).
pub(crate) fn migration_failed_fix(why: &str) -> String {
    format!(
        "the app cannot migrate this file: {why}; fix it by hand, then the migration to schema_version {SCHEMA_VERSION} is offered"
    )
}

/// A config id: kebab-case, lowercase letters and digits in hyphen-separated
/// words (Conventions).
pub(crate) fn is_kebab(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// The id a non-kebab name would have, for the fix text.
pub(crate) fn kebab_of(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The closest candidate within a small edit distance, if any.
pub(crate) fn did_you_mean(word: &str, candidates: &[&str]) -> Option<String> {
    let limit = (word.chars().count() / 3).max(2);
    candidates
        .iter()
        .map(|c| (edit_distance(word, c), *c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c.to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

// Secret-looking values (owner decision c, 2026-11-20).

const EXACT_SECRET_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "token",
    "secret",
    "password",
    "private_key",
];
const SUFFIX_SECRET_KEYS: &[&str] = &[
    "_api_key",
    "_apikey",
    "_token",
    "_secret",
    "_password",
    "_private_key",
];

/// True for a key name that holds a secret by its name: an exact match or a
/// suffix match, ignoring case. `max_tokens`, `token_budget` and `secret_ref`
/// only contain the words and never match.
pub(crate) fn is_secret_key(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    EXACT_SECRET_KEYS.contains(&lower.as_str())
        || SUFFIX_SECRET_KEYS.iter().any(|s| lower.ends_with(s))
}

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// How many token characters follow position `from` in `chars`.
fn run_len(chars: &[char], from: usize, allowed: fn(char) -> bool) -> usize {
    chars[from..].iter().take_while(|c| allowed(**c)).count()
}

/// True when `text` holds a value shaped like a known provider key or a PEM
/// private key. Open-ended prefixes need at least 8 more key characters so
/// that a bare word such as `hf_` or `sk-short` is not flagged.
pub(crate) fn has_secret_prefix(text: &str) -> bool {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        if start > 0 && chars[start - 1].is_ascii_alphanumeric() {
            continue; // not at a token boundary
        }
        let rest: String = chars[start..].iter().take(12).collect();
        let after = |prefix: &str| start + prefix.chars().count();
        let open = |prefix: &str, min: usize| {
            rest.starts_with(prefix)
                && after(prefix) <= chars.len()
                && run_len(&chars, after(prefix), token_char) >= min
        };
        if open("sk-ant-", 8)
            || open("sk-", 20)
            || open("ghp_", 8)
            || open("github_pat_", 8)
            || open("hf_", 8)
        {
            return true;
        }
        // xox[abprs]-
        if chars.len() >= start + 5
            && rest.starts_with("xox")
            && "abprs".contains(chars[start + 3])
            && chars[start + 4] == '-'
            && run_len(&chars, start + 5, token_char) >= 8
        {
            return true;
        }
        if rest.starts_with("AKIA")
            && run_len(&chars, start + 4, |c| {
                c.is_ascii_uppercase() || c.is_ascii_digit()
            }) >= 16
        {
            return true;
        }
        if rest.starts_with("AIza") && run_len(&chars, start + 4, token_char) >= 35 {
            return true;
        }
    }
    false
}

/// Reports every secret-looking value in the tree, at most once per line;
/// a finding names the key, never the value.
pub(crate) fn secrets(rep: &mut Report, node: &Node) {
    let mut seen = Vec::new();
    walk_secrets(rep, node, None, &mut seen);
}

fn flag(rep: &mut Report, seen: &mut Vec<u32>, line: u32, key: Option<&str>) {
    if seen.contains(&line) {
        return;
    }
    seen.push(line);
    let message = match key {
        Some(k) => format!("secret-looking value in '{k}'"),
        None => "secret-looking value".to_string(),
    };
    rep.error(line, message, SECRET_FIX);
}

fn walk_secrets(rep: &mut Report, node: &Node, key: Option<&str>, seen: &mut Vec<u32>) {
    match &node.kind {
        Kind::Str(s) => {
            for (i, text) in s.lines().enumerate() {
                if has_secret_prefix(text) {
                    let line = node.line + u32::try_from(i).unwrap_or(0);
                    flag(rep, seen, line, key);
                }
            }
        }
        Kind::Seq(items) => {
            for item in items {
                walk_secrets(rep, item, key, seen);
            }
        }
        Kind::Map(entries) => {
            for (k, v) in entries {
                if is_secret_key(&k.name) && v.is_non_empty() {
                    flag(rep, seen, k.line, Some(&k.name));
                }
                walk_secrets(rep, v, Some(&k.name), seen);
            }
        }
        _ => {}
    }
}

/// Secret prefixes in free text (a seat's Markdown body); `first_line` is the
/// file line of the text's first line.
pub(crate) fn text_secrets(rep: &mut Report, text: &str, first_line: u32) {
    for (i, line) in text.lines().enumerate() {
        if has_secret_prefix(line) {
            rep.error(
                first_line + u32::try_from(i).unwrap_or(0),
                "secret-looking value",
                SECRET_FIX,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_names() {
        for k in [
            "api_key",
            "APIKEY",
            "token",
            "Secret",
            "password",
            "private_key",
            "openai_api_key",
            "github_token",
            "db_password",
            "my_apikey",
            "ssh_private_key",
            "client_secret",
        ] {
            assert!(is_secret_key(k), "{k}");
        }
        for k in [
            "max_tokens",
            "max_output_tokens",
            "token_budget",
            "secret_ref",
            "tokens",
            "tokenizer",
            "secrets",
        ] {
            assert!(!is_secret_key(k), "{k}");
        }
    }

    #[test]
    fn secret_prefixes() {
        for v in [
            "sk-ant-abcdefgh",
            "key: sk-abcdefghijklmnopqrstu",
            "ghp_abcdefgh",
            "github_pat_abcdefgh",
            "xoxb-abcdefgh",
            "xoxs-abcdefgh",
            "hf_abcdefgh",
            "AKIAABCDEFGHIJKLMNOP",
            "AIzaabcdefghijabcdefghijabcdefghijabcde",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        ] {
            assert!(has_secret_prefix(v), "{v}");
        }
        for v in [
            "sk-short",
            "task-ant-abcdefgh",
            "hf_",
            "the hf_model",
            "xoxc-abcdefgh",
            "AKIA-not",
            "AKIAABCDEF",
            "-----BEGIN CERTIFICATE-----",
            "skill",
        ] {
            assert!(!has_secret_prefix(v), "{v}");
        }
    }

    #[test]
    fn kebab_ids() {
        assert!(is_kebab("d-1"));
        assert!(is_kebab("alpha"));
        for bad in ["", "Alpha", "a_b", "-a", "a-", "a--b", "a b"] {
            assert!(!is_kebab(bad), "{bad}");
        }
        assert_eq!(kebab_of("Gamma_Seat"), "gamma-seat");
    }

    #[test]
    fn suggestions() {
        let keys = ["timezone", "currency", "teams"];
        assert_eq!(did_you_mean("timezon", &keys).as_deref(), Some("timezone"));
        assert_eq!(did_you_mean("team", &keys).as_deref(), Some("teams"));
        assert_eq!(did_you_mean("zzzzzz", &keys), None);
    }
}
