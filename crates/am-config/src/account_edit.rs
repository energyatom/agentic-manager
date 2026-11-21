//! What the interim CLI Account edit needs from the project's files (story
//! 2.1, AD-15): only this crate reads them (AD-16).
//!
//! - [`accounts_file`] reads `models/accounts.yaml` as it is now: whether it
//!   exists, each listed Account's id in file order, and the Accounts the
//!   loader accepts.
//! - [`account_uses`] lists every place that names an Account id (a seat's
//!   model or fallback, a capability's provider, a Source's account, the
//!   search provider), whether or not the project is valid: an Account is
//!   often added because seats already name it.
//! - [`add_account_edit`], [`remove_account_edit`] and [`new_accounts_file`]
//!   build the one patch the CLI may write, for the writer to check and
//!   apply ([`crate::ConfigWriter::apply_with_audit`],
//!   [`crate::ConfigWriter::create`]).
//! - [`outside_changes`] names each file whose bytes on disk differ from
//!   what its last History entry wrote: an outside change still waiting for
//!   Reload (AD-16). The CLI writes nothing while there is one.

use std::collections::BTreeMap;
use std::path::Path;

use am_domain::{ConfigHistory, HistoryError};
use serde_json::{Map, Value, json};

use crate::canon::{self, FileKind};
use crate::finding::Report;
use crate::node::{Kind, Node};
use crate::refs::RefKind;
use crate::writer::sha256_hex;
use crate::{Account, ConfigEdit, EditOp, Loader, WriteError, checks, kinds, tracked};

/// The project-relative path of the Accounts file.
pub const ACCOUNTS_FILE: &str = "models/accounts.yaml";

/// The reserved local Account ids (AD-31): always resolvable, never listed
/// in the file, never added or removed.
pub fn reserved_account_ids() -> Vec<String> {
    crate::refs::RESERVED_ACCOUNTS
        .iter()
        .map(|id| id.to_string())
        .collect()
}

/// True when `id` is spelled as a config id (kebab-case: lowercase letters,
/// digits and single hyphens), as the validator requires of an Account's.
pub fn is_config_id(id: &str) -> bool {
    checks::is_kebab(id)
}

/// `models/accounts.yaml` as it is on disk now.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountsFile {
    /// False when there is no such file (an add then creates it).
    pub exists: bool,
    /// The sha256 (lowercase hex) of the bytes this read saw; `None` when
    /// there is no file. An edit built from this read is applied only while
    /// the file still holds these bytes
    /// ([`crate::ConfigWriter::apply_with_audit`]).
    pub sha256: Option<String>,
    /// Why the file cannot be patched, in words (not UTF-8, not YAML, or
    /// its `accounts` is not a list); `None` when it can.
    pub problem: Option<String>,
    /// Each item of `accounts:` in file order: its `id` as written, when it
    /// has one.
    pub ids: Vec<Option<String>>,
    /// The Accounts the loader accepts (a known provider and an id).
    pub accounts: Vec<Account>,
    /// `accounts:` holds a list (an item is appended to it); otherwise the
    /// key is absent or empty and is set whole.
    list: bool,
}

impl AccountsFile {
    /// True when an item of the file has this id.
    pub fn has(&self, id: &str) -> bool {
        self.ids.iter().any(|i| i.as_deref() == Some(id))
    }
}

fn text_of(node: &Node) -> Option<String> {
    match &node.kind {
        Kind::Str(s) => Some(s.clone()),
        Kind::Int(_) | Kind::Float(_) | Kind::Bool(_) => node.raw.clone(),
        _ => None,
    }
}

fn at<'n>(node: &'n Node, key: &str) -> Option<&'n Node> {
    node.as_map()?
        .iter()
        .find(|(k, _)| k.name == key)
        .map(|(_, v)| v)
}

/// Reads `models/accounts.yaml` of the project at `root`.
pub fn accounts_file(root: &Path) -> AccountsFile {
    let mut out = AccountsFile {
        exists: false,
        sha256: None,
        problem: None,
        ids: Vec::new(),
        accounts: Vec::new(),
        list: false,
    };
    let bytes = match std::fs::read(root.join(ACCOUNTS_FILE)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.exists = true;
            out.problem = Some(format!("it cannot be read: {e}"));
            return out;
        }
    };
    out.exists = true;
    out.sha256 = Some(sha256_hex(&bytes));
    let Ok(text) = String::from_utf8(bytes) else {
        out.problem = Some("it is not UTF-8 text".into());
        return out;
    };
    let doc = match canon::parse(FileKind::Accounts, &text) {
        Ok(d) => d,
        Err(reason) => {
            out.problem = Some(reason);
            return out;
        }
    };
    match at(&doc.tree, "accounts").map(|n| &n.kind) {
        None | Some(Kind::Null) => {}
        Some(Kind::Seq(items)) => {
            out.list = true;
            out.ids = items
                .iter()
                .map(|item| at(item, "id").and_then(text_of))
                .collect();
        }
        Some(_) => {
            out.problem = Some("its `accounts` is not a list".into());
            return out;
        }
    }
    let mut rep = Report::new(ACCOUNTS_FILE);
    out.accounts = kinds::accounts(&mut rep, &doc.tree);
    out
}

/// One place in the project's files that names an Account.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountUse {
    /// Project-relative path with forward slashes, e.g. `seats/alpha.md`.
    pub file: String,
    pub line: u32,
    /// What takes the Account there, in words: "a seat's model", "a seat's
    /// fallback model", a capability id, "search", "a crawler Source".
    pub what: String,
}

impl std::fmt::Display for AccountUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.file, self.what)
    }
}

/// Every place in the project at `root` that names the Account `id`, sorted
/// by file and line. Read through the loader without a snapshot, so it
/// answers for a project with errors too (a seat naming an Account that is
/// not declared yet).
pub fn account_uses(root: &Path, id: &str) -> Vec<AccountUse> {
    let mut loader = Loader {
        places: kinds::Places {
            project: root.to_path_buf(),
            home: crate::home_dir(),
            knowledge_root: None,
            recent_roots: Vec::new(),
        },
        ..Loader::default()
    };
    loader.read_all(root);
    let mut uses: Vec<AccountUse> = loader
        .refs
        .pending
        .iter()
        .filter(|r| r.id == id)
        .filter_map(|r| {
            let what = match &r.kind {
                RefKind::Account => "a Source".to_string(),
                RefKind::AccountOfKind { user, .. } => user.clone(),
                _ => return None,
            };
            Some(AccountUse {
                file: r.file.clone(),
                line: r.line,
                what,
            })
        })
        .collect();
    uses.sort();
    uses.dedup();
    uses
}

/// The Account as its item in the file: `id`, `provider`, the optional
/// `name`, and `secret_ref` naming the Keychain entry (never a key).
fn account_item(id: &str, provider: &str, name: Option<&str>) -> Value {
    let mut item = Map::new();
    item.insert("id".into(), json!(id));
    item.insert("provider".into(), json!(provider));
    if let Some(name) = name {
        item.insert("name".into(), json!(name));
    }
    item.insert("secret_ref".into(), json!(id));
    Value::Object(item)
}

/// The edit that appends the Account to the existing file `file`. It names
/// the list by what `file` saw, so it is applied with `file.sha256`.
pub fn add_account_edit(
    file: &AccountsFile,
    id: &str,
    provider: &str,
    name: Option<&str>,
) -> ConfigEdit {
    let item = account_item(id, provider, name);
    let op = if file.list {
        EditOp::Set {
            path: vec!["accounts".into(), file.ids.len().to_string()],
            value: item,
        }
    } else {
        EditOp::Set {
            path: vec!["accounts".into()],
            value: Value::Array(vec![item]),
        }
    };
    ConfigEdit {
        file: ACCOUNTS_FILE.into(),
        ops: vec![op],
        body: None,
    }
}

/// The edit that removes the Account `id` from `file` (by its number in
/// the list `file` saw, so it is applied with `file.sha256`); `None` when no
/// item has that id.
pub fn remove_account_edit(file: &AccountsFile, id: &str) -> Option<ConfigEdit> {
    let index = file.ids.iter().position(|i| i.as_deref() == Some(id))?;
    Some(ConfigEdit {
        file: ACCOUNTS_FILE.into(),
        ops: vec![EditOp::Remove {
            path: vec!["accounts".into(), index.to_string()],
        }],
        body: None,
    })
}

/// The canonical bytes of a new `models/accounts.yaml` holding one Account,
/// for a [`crate::ConfigCreate`].
pub fn new_accounts_file(
    id: &str,
    provider: &str,
    name: Option<&str>,
) -> Result<Vec<u8>, WriteError> {
    let bad = |e: String| WriteError::BadEdit(format!("{ACCOUNTS_FILE}: {e}"));
    let head = format!("schema_version: {}\n", checks::SCHEMA_VERSION);
    let mut doc = canon::parse(FileKind::Accounts, &head).map_err(bad)?;
    let list = Value::Array(vec![account_item(id, provider, name)]);
    canon::apply(&mut doc.tree, &["accounts".to_string()], Some(&list)).map_err(bad)?;
    canon::emit(&doc).map(String::into_bytes).map_err(bad)
}

/// The config files whose bytes on disk differ from what their last History
/// entry wrote (an empty file is the same as no file), sorted: each is a
/// change made outside the app that no Reload has applied yet (AD-16). A
/// file with no History entry is not judged: nothing the app recorded says
/// what it held.
pub fn outside_changes(
    root: &Path,
    history: &dyn ConfigHistory,
) -> Result<Vec<String>, HistoryError> {
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    for entry in history.entries(None)? {
        let after = if entry.after.is_empty() {
            String::new()
        } else {
            entry.after_sha256
        };
        last.insert(entry.file, after);
    }
    Ok(last
        .into_iter()
        .filter(|(file, _)| tracked(file))
        .filter(|(file, after)| {
            let on_disk = std::fs::read(root.join(file))
                .ok()
                .filter(|b| !b.is_empty())
                .map(|b| sha256_hex(&b))
                .unwrap_or_default();
            on_disk != *after
        })
        .map(|(file, _)| file)
        .collect())
}
