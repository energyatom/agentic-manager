//! Confined file paths (AD-30). Every file tool takes a [`WorkspacePath`],
//! never a bare path. Canonicalisation is IO and happens in the adapter; this
//! module holds the pure checks.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// A linked folder: a grant root. `canonical` must already be canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRoot {
    id: String,
    canonical: PathBuf,
    access: Access,
}

impl GrantRoot {
    pub fn new(id: impl Into<String>, canonical: PathBuf, access: Access) -> Self {
        GrantRoot {
            id: id.into(),
            canonical,
            access,
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn path(&self) -> &Path {
        &self.canonical
    }
    pub fn access(&self) -> Access {
        self.access
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("the path {0} is absolute; give a path inside the linked folder")]
    Absolute(String),
    #[error("the path {0} climbs out of the linked folder with `..`")]
    ParentTraversal(String),
    #[error("the path {0} leaves the linked folder (a link points outside it)")]
    Escapes(String),
    #[error("the path {0} is not valid")]
    Invalid(String),
    #[error("the linked folder {0} is read-only")]
    ReadOnly(String),
}

/// A path proven to lie inside a grant root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePath {
    root_id: String,
    relative: String,
    absolute: PathBuf,
}

impl WorkspacePath {
    /// Lexical check of a requested relative path: no absolute paths, no `..`,
    /// no NUL. Runs before any filesystem access.
    pub fn check_relative(requested: &str) -> Result<(), PathError> {
        if requested.contains('\0') {
            return Err(PathError::Invalid(requested.to_string()));
        }
        let p = Path::new(requested);
        if p.is_absolute() || requested.starts_with('~') {
            return Err(PathError::Absolute(requested.to_string()));
        }
        for c in p.components() {
            match c {
                Component::ParentDir => {
                    return Err(PathError::ParentTraversal(requested.to_string()));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(PathError::Absolute(requested.to_string()));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Build a confined path from the canonical target the adapter resolved
    /// (symlinks followed). Refuses a target outside the root.
    pub fn confine(
        root: &GrantRoot,
        requested: &str,
        canonical_target: PathBuf,
    ) -> Result<WorkspacePath, PathError> {
        Self::check_relative(requested)?;
        if !canonical_target.starts_with(root.path()) {
            return Err(PathError::Escapes(requested.to_string()));
        }
        let relative = canonical_target
            .strip_prefix(root.path())
            .map_err(|_| PathError::Escapes(requested.to_string()))?
            .to_string_lossy()
            .to_string();
        Ok(WorkspacePath {
            root_id: root.id().to_string(),
            relative,
            absolute: canonical_target,
        })
    }

    pub fn root_id(&self) -> &str {
        &self.root_id
    }
    pub fn relative(&self) -> &str {
        &self.relative
    }
    pub fn absolute(&self) -> &Path {
        &self.absolute
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_checks_refuse_escapes() {
        assert!(WorkspacePath::check_relative("a/b.txt").is_ok());
        assert!(matches!(
            WorkspacePath::check_relative("../x"),
            Err(PathError::ParentTraversal(_))
        ));
        assert!(matches!(
            WorkspacePath::check_relative("a/../../x"),
            Err(PathError::ParentTraversal(_))
        ));
        assert!(matches!(
            WorkspacePath::check_relative("/etc/passwd"),
            Err(PathError::Absolute(_))
        ));
    }

    #[test]
    fn confine_refuses_outside_target() {
        let root = GrantRoot::new("r", PathBuf::from("/data/root"), Access::Read);
        assert!(WorkspacePath::confine(&root, "x", PathBuf::from("/data/root/x")).is_ok());
        assert!(matches!(
            WorkspacePath::confine(&root, "link", PathBuf::from("/data/other/secret")),
            Err(PathError::Escapes(_))
        ));
        assert!(matches!(
            WorkspacePath::confine(&root, "x", PathBuf::from("/data/rootx/y")),
            Err(PathError::Escapes(_))
        ));
    }
}
