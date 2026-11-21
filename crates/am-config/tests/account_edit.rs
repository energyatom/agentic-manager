//! Story 2.1 (review fix): the interim CLI Account edit is built from a
//! read of `models/accounts.yaml` and names list items by their number, so
//! the writer applies it only while the file still holds the bytes it was
//! built on (`AccountsFile::sha256`, given to
//! `ConfigWriter::apply_with_audit`); a file that changed meanwhile is
//! refused as `ChangedOnDisk` and nothing is written. Written before the
//! check exists.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use am_config::{
    ACCOUNTS_FILE, ConfigWriter, WriteError, accounts_file, add_account_edit, remove_account_edit,
};
use am_domain::{Actor, ConfigHistory};
use am_testkit::InMemoryConfigHistory;
use common::copy_of;
use sha2::{Digest, Sha256};

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

fn ids(root: &std::path::Path) -> Vec<String> {
    accounts_file(root).ids.into_iter().flatten().collect()
}

/// The read carries the sha256 of the bytes it saw (none for no file).
#[test]
fn the_read_names_the_bytes_it_was_made_on() {
    let dir = copy_of("valid");
    let root = dir.path();
    let bytes = std::fs::read(root.join(ACCOUNTS_FILE)).unwrap();
    assert_eq!(
        accounts_file(root).sha256,
        Some(hex::encode(Sha256::digest(&bytes)))
    );
    std::fs::remove_file(root.join(ACCOUNTS_FILE)).unwrap();
    assert_eq!(accounts_file(root).sha256, None);
}

/// An Account added to the file after the read: the edit (an append at the
/// index the read saw) is refused and the file keeps the Account that
/// appeared; an edit built on the file's bytes applies.
#[test]
fn an_edit_built_on_other_bytes_is_refused_and_writes_nothing() {
    let dir = copy_of("valid");
    let root = dir.path();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();

    let seen = accounts_file(root);
    let built_on = seen.sha256.clone().unwrap();
    let appeared = format!(
        "{}  - id: other\n    provider: anthropic\n    secret_ref: other\n",
        std::fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap()
    );
    std::fs::write(root.join(ACCOUNTS_FILE), &appeared).unwrap();

    let add = add_account_edit(&seen, "cloud2", "anthropic", None);
    let err = w
        .apply_with_audit(owner(), add, &built_on, Vec::new())
        .unwrap_err();
    assert!(
        matches!(&err, WriteError::ChangedOnDisk { files } if files == &[ACCOUNTS_FILE.to_string()]),
        "{err:?}"
    );
    let remove = remove_account_edit(&seen, "cloud");
    assert!(remove.is_some());
    assert_eq!(
        std::fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap(),
        appeared
    );
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());

    // Built on what the file holds now, the same add applies.
    let now = accounts_file(root);
    let entry = w
        .apply_with_audit(
            owner(),
            add_account_edit(&now, "cloud2", "anthropic", None),
            now.sha256.as_deref().unwrap(),
            Vec::new(),
        )
        .unwrap();
    assert_eq!(entry.file, ACCOUNTS_FILE);
    assert_eq!(ids(root), ["cloud", "other", "cloud2"]);
}
