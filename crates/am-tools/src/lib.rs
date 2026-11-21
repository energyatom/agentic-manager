//! Folder access tools (AD-30): `list_files` and `read_file` (read class) and
//! `write_file` (local-write, only on a folder linked with write access).
//!
//! Every path is confined to a linked folder (a grant root): requested paths
//! are checked lexically (no absolute paths, no `..`), then canonicalised with
//! symlinks followed and refused if the target leaves the root. Every list,
//! read, write and refusal is audited with root, path and hash. Files are
//! returned unchanged as `UntrustedData` file parts; nothing is converted.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use am_domain::{
    Access, Actor, AuditEvent, AuditSink, FilePart, GrantRoot, PathError, UntrustedData,
    WorkspacePath,
};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FolderError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("no folder {0} is linked to this seat")]
    UnknownRoot(String),
    #[error("{0} cannot be linked: {1}")]
    Refused(String, String),
    #[error("{0} was not found in the linked folder")]
    NotFound(String),
    #[error("{0} is {1} bytes, over the {2}-byte per-file cap")]
    TooLarge(String, u64, u64),
    #[error("this step already read {0} files, its cap")]
    TooManyFiles(usize),
    #[error("this step already attached {0} bytes, its cap")]
    TotalCap(u64),
    #[error("could not read {0}: {1}")]
    Io(String, String),
}

/// Per-step caps (AD-30 defaults; raising one is privilege-widening).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderCaps {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for FolderCaps {
    fn default() -> Self {
        FolderCaps {
            max_files: 50,
            max_file_bytes: 32 * 1024 * 1024,
            max_total_bytes: 100 * 1024 * 1024,
        }
    }
}

/// Link a folder: canonicalise it and refuse the filesystem root, the home
/// folder itself, `~/Library` and any legacy in-project runtime folder
/// (`am_domain::runtime::LEGACY_DIR`).
pub fn link_folder(id: &str, path: &Path, access: Access) -> Result<GrantRoot, FolderError> {
    let shown = path.display().to_string();
    let canonical = path
        .canonicalize()
        .map_err(|e| FolderError::Refused(shown.clone(), e.to_string()))?;
    if !canonical.is_dir() {
        return Err(FolderError::Refused(shown, "it is not a folder".into()));
    }
    if canonical.parent().is_none() {
        return Err(FolderError::Refused(shown, "the filesystem root".into()));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(home) = home.canonicalize()
        && (canonical == home || canonical.starts_with(home.join("Library")))
    {
        return Err(FolderError::Refused(
            shown,
            "the home folder itself and ~/Library cannot be linked".into(),
        ));
    }
    if canonical
        .components()
        .any(|c| c.as_os_str() == am_domain::runtime::LEGACY_DIR)
    {
        return Err(FolderError::Refused(
            shown,
            "app runtime folders cannot be linked".into(),
        ));
    }
    Ok(GrantRoot::new(id, canonical, access))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub relative_path: String,
    pub size: u64,
    pub media_type: String,
}

/// Media type from the extension, confirmed by the leading bytes where a
/// signature exists.
pub fn media_type(path: &Path, head: &[u8]) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let sniffed = if head.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF8") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        Some("audio/wav")
    } else {
        None
    };
    let by_ext = match ext.as_str() {
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "ppt" => "application/vnd.ms-powerpoint",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    };
    match sniffed {
        // Office files are zip containers: trust the extension for them.
        Some(s) => s.to_string(),
        None => by_ext.to_string(),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub struct FolderAccess {
    roots: Vec<GrantRoot>,
    audit: Arc<dyn AuditSink>,
    actor: Actor,
    caps: FolderCaps,
    files_read: AtomicUsize,
    bytes_read: AtomicU64,
}

impl FolderAccess {
    pub fn new(roots: Vec<GrantRoot>, audit: Arc<dyn AuditSink>, actor: Actor) -> Self {
        FolderAccess {
            roots,
            audit,
            actor,
            caps: FolderCaps::default(),
            files_read: AtomicUsize::new(0),
            bytes_read: AtomicU64::new(0),
        }
    }

    pub fn with_caps(mut self, caps: FolderCaps) -> Self {
        self.caps = caps;
        self
    }

    fn root(&self, id: &str) -> Result<&GrantRoot, FolderError> {
        self.roots
            .iter()
            .find(|r| r.id() == id)
            .ok_or_else(|| FolderError::UnknownRoot(id.to_string()))
    }

    fn audit(&self, action: &str, root: &str, rel: &str, outcome: &str, hash: Option<String>) {
        self.audit.record(AuditEvent {
            actor: self.actor.clone(),
            action: action.into(),
            target: format!("{root}/{rel}"),
            outcome: outcome.into(),
            hash,
        });
    }

    fn refuse<T>(&self, root: &str, rel: &str, e: FolderError) -> Result<T, FolderError> {
        self.audit("folder.refused", root, rel, &e.to_string(), None);
        Err(e)
    }

    /// Resolve an existing path inside a root (symlinks followed, then confined).
    fn resolve_existing(&self, root: &GrantRoot, rel: &str) -> Result<WorkspacePath, FolderError> {
        WorkspacePath::check_relative(rel)?;
        let joined = root.path().join(rel);
        let canonical = joined
            .canonicalize()
            .map_err(|_| FolderError::NotFound(rel.to_string()))?;
        Ok(WorkspacePath::confine(root, rel, canonical)?)
    }

    /// List every file under a root (recursively). Entries whose symlink
    /// leaves the root are skipped and audited.
    pub fn list_files(&self, root_id: &str) -> Result<Vec<FileEntry>, FolderError> {
        let root = self.root(root_id)?.clone();
        let mut out = Vec::new();
        let mut stack = vec![String::new()];
        // Canonical folders already listed: a symlinked folder that leads back
        // to one (a link loop, or a second path to the same folder) is skipped.
        let mut visited = std::collections::HashSet::new();
        visited.insert(root.path().to_path_buf());
        while let Some(dir_rel) = stack.pop() {
            let dir = root.path().join(&dir_rel);
            let entries = std::fs::read_dir(&dir)
                .map_err(|e| FolderError::Io(dir_rel.clone(), e.to_string()))?;
            let mut names: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            names.sort_by_key(|e| e.file_name());
            for entry in names {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let rel = if dir_rel.is_empty() {
                    name.clone()
                } else {
                    format!("{dir_rel}/{name}")
                };
                let wp = match self.resolve_existing(&root, &rel) {
                    Ok(wp) => wp,
                    Err(e) => {
                        self.audit("folder.refused", root.id(), &rel, &e.to_string(), None);
                        continue;
                    }
                };
                let meta = std::fs::metadata(wp.absolute())
                    .map_err(|e| FolderError::Io(rel.clone(), e.to_string()))?;
                if meta.is_dir() {
                    if visited.insert(wp.absolute().to_path_buf()) {
                        stack.push(rel);
                    } else {
                        self.audit(
                            "folder.refused",
                            root.id(),
                            &rel,
                            "folder already listed (link loop or second path); skipped",
                            None,
                        );
                    }
                    continue;
                }
                let head = read_head(wp.absolute());
                out.push(FileEntry {
                    name,
                    relative_path: rel,
                    size: meta.len(),
                    media_type: media_type(wp.absolute(), &head),
                });
            }
        }
        out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.audit("folder.list", root.id(), "", "ok", None);
        Ok(out)
    }

    /// Read one file as an untrusted native file part, under the per-step caps.
    pub fn read_file(&self, root_id: &str, rel: &str) -> Result<UntrustedData, FolderError> {
        let root = self.root(root_id)?.clone();
        let wp = match self.resolve_existing(&root, rel) {
            Ok(wp) => wp,
            Err(e) => return self.refuse(root.id(), rel, e),
        };
        if self.files_read.load(Ordering::SeqCst) >= self.caps.max_files {
            return self.refuse(
                root.id(),
                rel,
                FolderError::TooManyFiles(self.caps.max_files),
            );
        }
        let size = std::fs::metadata(wp.absolute())
            .map_err(|e| FolderError::Io(rel.to_string(), e.to_string()))?
            .len();
        if size > self.caps.max_file_bytes {
            return self.refuse(
                root.id(),
                rel,
                FolderError::TooLarge(rel.to_string(), size, self.caps.max_file_bytes),
            );
        }
        if self.bytes_read.load(Ordering::SeqCst) + size > self.caps.max_total_bytes {
            return self.refuse(
                root.id(),
                rel,
                FolderError::TotalCap(self.caps.max_total_bytes),
            );
        }
        let bytes = std::fs::read(wp.absolute())
            .map_err(|e| FolderError::Io(rel.to_string(), e.to_string()))?;
        self.files_read.fetch_add(1, Ordering::SeqCst);
        self.bytes_read
            .fetch_add(bytes.len() as u64, Ordering::SeqCst);
        let hash = sha256_hex(&bytes);
        self.audit(
            "folder.read",
            root.id(),
            wp.relative(),
            "ok",
            Some(hash.clone()),
        );
        let name = wp
            .absolute()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mt = media_type(wp.absolute(), &bytes[..bytes.len().min(16)]);
        Ok(UntrustedData::file(
            format!("file:{}/{}@{}", root.id(), wp.relative(), hash),
            FilePart {
                name,
                media_type: mt,
                bytes,
            },
        ))
    }

    /// Create or replace a file; only on a folder linked with write access.
    /// Never deletes, moves or renames.
    pub fn write_file(
        &self,
        root_id: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<WorkspacePath, FolderError> {
        let root = self.root(root_id)?.clone();
        if root.access() != Access::ReadWrite {
            return self.refuse(
                root.id(),
                rel,
                FolderError::Path(PathError::ReadOnly(root.id().to_string())),
            );
        }
        if let Err(e) = WorkspacePath::check_relative(rel) {
            return self.refuse(root.id(), rel, e.into());
        }
        let target = root.path().join(rel);
        let parent = target
            .parent()
            .ok_or_else(|| FolderError::Path(PathError::Invalid(rel.to_string())))?;
        let parent_c = match parent.canonicalize() {
            Ok(p) => p,
            Err(_) => return self.refuse(root.id(), rel, FolderError::NotFound(rel.to_string())),
        };
        let file_name = target
            .file_name()
            .ok_or_else(|| FolderError::Path(PathError::Invalid(rel.to_string())))?;
        let mut candidate = parent_c.join(file_name);
        // An existing symlink at the target is resolved and must stay inside;
        // one that does not resolve (dangling) is refused, because writing
        // through it would create its target wherever it points.
        let is_link = std::fs::symlink_metadata(&candidate)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        match candidate.canonicalize() {
            Ok(c) => candidate = c,
            Err(_) if is_link => {
                return self.refuse(
                    root.id(),
                    rel,
                    FolderError::Path(PathError::Escapes(rel.to_string())),
                );
            }
            Err(_) => {}
        }
        let wp = match WorkspacePath::confine(&root, rel, candidate) {
            Ok(wp) => wp,
            Err(e) => return self.refuse(root.id(), rel, e.into()),
        };
        std::fs::write(wp.absolute(), bytes)
            .map_err(|e| FolderError::Io(rel.to_string(), e.to_string()))?;
        self.audit(
            "folder.write",
            root.id(),
            wp.relative(),
            "ok",
            Some(sha256_hex(bytes)),
        );
        Ok(wp)
    }
}

fn read_head(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut buf = [0u8; 16];
    match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => buf[..n].to_vec(),
        Err(_) => Vec::new(),
    }
}
