//! Story 2.12 (AD-17, AD-32): `am_domain::runtime::AppDataLayout` names every
//! path in the app data dir, as `RuntimeLayout` does inside a runtime
//! folder: the user Accounts file, the user store, its lock, its
//! pre-migration backups, this Mac's id and the folder of running servers.
//! Nothing else spells them (`serving_dir` and `MAC_ID_FILE` moved behind
//! it). Written before the type exists.
//!
//! Story 2.13 (AD-15 *Migrate*, AD-17): the layout also names each
//! project's `accounts migrate` backups,
//! `<app data dir>/backups/accounts-migrate/<project-id>/`; the old test
//! that every path lies inside the app data dir was rewritten to include
//! it. Written before the method exists.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_domain::runtime::{APP_DATA_NAME, AppDataLayout, ProjectId};

#[test]
fn the_layout_names_every_path_in_the_app_data_dir() {
    let dir = PathBuf::from("/support").join(APP_DATA_NAME);
    let layout = AppDataLayout::new(dir.clone());
    assert_eq!(layout.dir(), dir.as_path());
    assert_eq!(layout.accounts_file(), dir.join("accounts.yaml"));
    assert_eq!(layout.user_db(), dir.join("user.db"));
    assert_eq!(layout.user_lock(), dir.join("user.lock"));
    assert_eq!(
        layout.backups_pre_migration(),
        dir.join("backups").join("pre-migration")
    );
    // Unchanged on disk from the per-project stage: this Mac's id and the
    // folder where every running `agentic serve` records its port.
    assert_eq!(layout.mac_id(), dir.join("mac-id"));
    assert_eq!(layout.serving_dir(), dir.join("serving"));
}

#[test]
fn every_path_lies_inside_the_app_data_dir() {
    let layout = AppDataLayout::new(PathBuf::from("/a/b"));
    for path in [
        layout.accounts_file(),
        layout.user_db(),
        layout.user_lock(),
        layout.backups_pre_migration(),
        layout.mac_id(),
        layout.serving_dir(),
        // Story 2.13.
        layout.accounts_migrate_backups(&ProjectId::mint()),
    ] {
        assert!(path.starts_with(Path::new("/a/b")), "{}", path.display());
        assert_ne!(path, PathBuf::from("/a/b"));
    }
}

#[test]
fn the_layout_is_a_plain_value() {
    let a = AppDataLayout::new(PathBuf::from("/x"));
    let b = a.clone();
    assert_eq!(a, b);
    assert!(format!("{a:?}").contains("/x"));
}

/// Story 2.13: one folder per project for the `accounts migrate` backups,
/// beside the user store's pre-migration backups, named by the project id
/// (never by its folder's path).
#[test]
fn the_layout_names_each_projects_accounts_migrate_backups() {
    let dir = PathBuf::from("/support").join(APP_DATA_NAME);
    let layout = AppDataLayout::new(dir.clone());
    let id = ProjectId::parse("01936d6e-7a3b-7c4d-8e5f-0a1b2c3d4e5f").unwrap();
    assert_eq!(
        layout.accounts_migrate_backups(&id),
        dir.join("backups")
            .join("accounts-migrate")
            .join("01936d6e-7a3b-7c4d-8e5f-0a1b2c3d4e5f")
    );
    let other = ProjectId::mint();
    assert_ne!(
        layout.accounts_migrate_backups(&id),
        layout.accounts_migrate_backups(&other),
        "one folder per project"
    );
    assert_eq!(
        layout.accounts_migrate_backups(&other).parent(),
        layout.accounts_migrate_backups(&id).parent(),
        "every project's folder in one place"
    );
    assert!(
        !layout
            .accounts_migrate_backups(&id)
            .starts_with(layout.backups_pre_migration()),
        "apart from the user store's own backups"
    );
}
