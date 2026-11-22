//! This Mac's user Accounts file (story 2.12; AD-15, AD-16, AD-17):
//! `<app data dir>/accounts.yaml`, the one list of Accounts every project on
//! this Mac names by id. Only this crate reads and writes it.
//!
//! - [`load`] is its only reader: the findings (each naming the file it was
//!   loaded from, with its line) and the [`UserAccounts`] list. Entries are
//!   `id`, `provider`, `name`, `description`, `base_url` and `secret_ref`,
//!   never a key; ids are kebab-case and unique, the reserved local ids are
//!   refused, and `secret_ref`, when present, must equal the id. Story 2.3's
//!   provider and `base_url` rules apply unchanged. A missing file is an
//!   empty list with no finding (the first add on a Mac).
//! - [`key_ref`] is the one predicate every key reader uses: the Keychain
//!   entry an Account's key is read from, which is only ever its own id.
//! - The writer of the file is [`crate::ConfigWriter::for_user_accounts`].

use std::path::Path;

use crate::finding::{Finding, Report};
use crate::{Account, checks, kinds};

/// The user Accounts file's name in the app data dir.
pub const USER_ACCOUNTS_FILE: &str = "accounts.yaml";

/// The per-project Accounts file of stories 2.1 and 2.3: an Error in a
/// project, never read as Accounts (AD-17).
pub const LEGACY_ACCOUNTS_FILE: &str = "models/accounts.yaml";

/// The Accounts of this Mac, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserAccounts {
    accounts: Vec<Account>,
}

impl UserAccounts {
    /// The list holding `accounts`.
    pub fn new(accounts: Vec<Account>) -> UserAccounts {
        UserAccounts { accounts }
    }

    /// Every Account's id, in file order.
    pub fn ids(&self) -> Vec<String> {
        self.accounts.iter().map(|a| a.id.clone()).collect()
    }

    /// The Account with this id (the first, should the file hold two).
    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// Every Account, in file order.
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }
}

/// The Keychain entry the key of `account` is read from: its own id, and
/// only that. An Account whose `secret_ref` names anything else has none,
/// so no key reader ever reads another entry (AD-17).
pub fn key_ref(account: &Account) -> Option<&str> {
    match &account.secret_ref {
        Some(entry) if *entry != account.id => None,
        _ => Some(account.id.as_str()),
    }
}

/// What [`load`] read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadedAccounts {
    /// Every problem, sorted by line; each names the file as it was given.
    pub findings: Vec<Finding>,
    /// The Accounts the file declares with an id and a known provider, also
    /// those whose entry has an error (the caller checks the entry).
    pub accounts: UserAccounts,
}

/// Reads and checks the user Accounts file at `path`. A missing file is an
/// empty list with no finding.
pub fn load(path: &Path) -> LoadedAccounts {
    let label = path.display().to_string();
    match std::fs::read(path) {
        Ok(bytes) => load_bytes(&label, &bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LoadedAccounts::default(),
        Err(e) => {
            let mut rep = Report::new(&label);
            rep.error(
                1,
                format!("cannot read the file: {e}"),
                "check that the file exists and can be read",
            );
            LoadedAccounts {
                findings: rep.findings,
                accounts: UserAccounts::default(),
            }
        }
    }
}

/// [`load`] on the file's bytes; `label` names the file in the findings.
pub(crate) fn load_bytes(label: &str, bytes: &[u8]) -> LoadedAccounts {
    let mut rep = Report::new(label);
    let mut accounts = Vec::new();
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            if let Some(node) = kinds::yaml(&mut rep, text, 0) {
                accounts = kinds::accounts(&mut rep, &node);
            }
        }
        Err(_) => rep.error(1, "the file is not UTF-8 text", "save the file as UTF-8"),
    }
    let mut findings = rep.findings;
    if let Ok(text) = std::str::from_utf8(bytes) {
        hide_values(&mut findings, text, &valid_ids(&accounts));
    }
    findings
        .sort_by(|a, b| (a.line, a.severity, &a.message).cmp(&(b.line, b.severity, &b.message)));
    findings.dedup();
    LoadedAccounts {
        findings,
        accounts: UserAccounts::new(accounts),
    }
}

/// The one guard on every word the app shows about the user Accounts file
/// (the loader's findings, the entry errors and the `problem` of
/// [`crate::accounts_file`], AD-17): a value from the file is never
/// repeated, so a key pasted anywhere in it (quoted or not, in a key, a
/// host, a block scalar, a parse error) is never printed or audited. Every
/// maximal fragment of 6 or more characters (split on whitespace, quotes
/// and punctuation other than `-` and `_`) that occurs in the file's text,
/// compared ignoring case, is replaced by a fixed placeholder, unless it is
/// a schema field name, a known provider name, a reserved id, a fixed word
/// of the app's own messages, or the id of an entry that loaded as a valid
/// Account (`valid_ids`, parsed, never read from raw lines).
pub(crate) fn hide_text(words: &str, text: &str, valid_ids: &[String]) -> String {
    const HIDDEN: &str = "<value hidden>";
    let file = text.to_lowercase();
    let kept = |fragment: &str| -> bool {
        let lower = fragment.to_lowercase();
        SCHEMA_FIELDS.contains(&lower.as_str())
            || FIXED_WORDS.contains(&lower.as_str())
            || am_domain::capability::account_providers().contains(&lower)
            || crate::refs::RESERVED_ACCOUNTS.contains(&lower.as_str())
            || valid_ids.iter().any(|id| *id == fragment)
    };
    let part = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    let mut out = String::with_capacity(words.len());
    let mut fragment = String::new();
    let flush = |fragment: &mut String, out: &mut String| {
        if fragment.chars().count() >= 6
            && file.contains(&fragment.to_lowercase())
            && !kept(fragment)
        {
            out.push_str(HIDDEN);
        } else {
            out.push_str(fragment);
        }
        fragment.clear();
    };
    for c in words.chars() {
        if part(c) {
            fragment.push(c);
        } else {
            flush(&mut fragment, &mut out);
            out.push(c);
        }
    }
    flush(&mut fragment, &mut out);
    out
}

/// [`hide_text`] over every finding's message and fix.
pub(crate) fn hide_values(findings: &mut [Finding], text: &str, valid_ids: &[String]) {
    for f in findings.iter_mut() {
        f.message = hide_text(&f.message, text, valid_ids);
        f.fix = hide_text(&f.fix, text, valid_ids);
    }
}

/// The ids of the parsed entries that are valid Accounts: kebab-case, not
/// shaped like a key.
pub(crate) fn valid_ids(accounts: &[Account]) -> Vec<String> {
    accounts
        .iter()
        .map(|a| a.id.clone())
        .filter(|id| checks::is_kebab(id) && !checks::has_secret_prefix(id))
        .collect()
}

/// The field names of the user Accounts file.
const SCHEMA_FIELDS: &[&str] = &[
    "schema_version",
    "accounts",
    "account",
    "id",
    "provider",
    "name",
    "description",
    "base_url",
    "secret_ref",
];

/// Words of the app's own messages that a file may also hold.
const FIXED_WORDS: &[&str] = &["localhost"];
