//! Content parts (AD-5, AD-10, AD-27).
//!
//! Prompt content is either an [`Instruction`] (config- or owner-authored) or
//! [`UntrustedData`] (files, MCP results, web pages, ...). There is no conversion
//! from `UntrustedData` to `Instruction`, and none may be added.

use serde::{Deserialize, Serialize};

/// Trusted, config- or owner-authored text. Build it only from config files or
/// the owner's own words, never from anything a tool, file or model returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction(String);

impl Instruction {
    pub fn authored(text: impl Into<String>) -> Self {
        Instruction(text.into())
    }
    pub fn text(&self) -> &str {
        &self.0
    }
}

/// A file attached to a model call, sent to the provider unchanged (AD-30, AD-31).
#[derive(Clone, PartialEq, Eq)]
pub struct FilePart {
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for FilePart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePart")
            .field("name", &self.name)
            .field("media_type", &self.media_type)
            .field("len", &self.bytes.len())
            .finish()
    }
}

impl FilePart {
    pub fn kind(&self) -> FileKind {
        FileKind::of(&self.media_type)
    }
}

/// What a model must be able to read to accept a file (AD-31 file-input flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// Plain text (txt, md, csv, tsv, json): readable by every model.
    Text,
    Pdf,
    Image,
    /// Word, PowerPoint and spreadsheet files.
    Office,
    Audio,
    Other,
}

impl FileKind {
    pub fn of(media_type: &str) -> FileKind {
        let mt = media_type.to_ascii_lowercase();
        let mt = mt.split(';').next().unwrap_or("").trim().to_string();
        match mt.as_str() {
            "text/plain"
            | "text/markdown"
            | "text/csv"
            | "text/tab-separated-values"
            | "application/json" => FileKind::Text,
            "application/pdf" => FileKind::Pdf,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/msword"
            | "application/vnd.ms-excel"
            | "application/vnd.ms-powerpoint" => FileKind::Office,
            _ if mt.starts_with("image/") => FileKind::Image,
            _ if mt.starts_with("audio/") => FileKind::Audio,
            _ => FileKind::Other,
        }
    }
}

/// The payload of untrusted content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UntrustedContent {
    Text(String),
    File(FilePart),
}

/// Untrusted content with its source label (e.g. `file:docs/report.pdf@<sha256>`,
/// `mcp:server/tool`). It renders only inside a labelled fence in the user role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedData {
    label: String,
    content: UntrustedContent,
}

impl UntrustedData {
    pub fn text(label: impl Into<String>, text: impl Into<String>) -> Self {
        UntrustedData {
            label: label.into(),
            content: UntrustedContent::Text(text.into()),
        }
    }
    pub fn file(label: impl Into<String>, file: FilePart) -> Self {
        UntrustedData {
            label: label.into(),
            content: UntrustedContent::File(file),
        }
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn content(&self) -> &UntrustedContent {
        &self.content
    }
    pub fn into_content(self) -> UntrustedContent {
        self.content
    }
}
