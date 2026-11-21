//! The config names (AD-30 as amended 2026-11-22): the folders the loader
//! reads config from, the files it reads as config, and the runtime and VCS
//! names a seat must never reach by path. One pure policy, called by
//! am-config's folder-root policy and validator and by the folder tools.
//!
//! Names are compared ignoring case (APFS's default). Every name is ASCII,
//! so a component equals one only when its lowercase form does; Unicode
//! normalisation cannot turn non-ASCII text into ASCII letters.

use crate::runtime::{LEGACY_DIR, OPEN_MARKER, RUNTIME_SEED};

/// Every config name, lowercase.
pub const CONFIG_NAMES: &[&str] = &[
    "models",
    "seats",
    "skills",
    "workflows",
    "tables",
    "schemas",
    "dashboards",
    "sources",
    "company.yaml",
    "tools.yaml",
    LEGACY_DIR,
    RUNTIME_SEED,
    OPEN_MARKER,
    ".git",
];

/// The config name `component` bears, ignoring case, if any.
pub fn config_name_of(component: &str) -> Option<&'static str> {
    let lower = component.to_lowercase();
    CONFIG_NAMES.iter().copied().find(|n| *n == lower)
}

/// The first config name among the components of the project-relative
/// path `rel` (written with `/`), if any.
pub fn config_name_in(rel: &str) -> Option<&'static str> {
    rel.split('/').find_map(config_name_of)
}

/// True when any component of the project-relative path `rel` (written
/// with `/`) is a config name, compared ignoring case.
pub fn is_config_bearing(rel: &str) -> bool {
    config_name_in(rel).is_some()
}
