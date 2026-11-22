//! Story 2.12 (AD-17, AD-32): `am_domain::runtime::AppDataLayout` names every
//! path in the app data dir, as `RuntimeLayout` does inside a runtime
//! folder: the user Accounts file, the user store, its lock, its
//! pre-migration backups, this Mac's id and the folder of running servers.
//! Nothing else spells them (`serving_dir` and `MAC_ID_FILE` moved behind
//! it). Written before the type exists.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_domain::runtime::{APP_DATA_NAME, AppDataLayout};

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
