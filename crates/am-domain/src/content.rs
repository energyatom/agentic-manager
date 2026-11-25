//! Content parts (AD-5, AD-10, AD-27).
//!
//! Prompt content is one of three types (AD-10, amended 2026-11-23):
//!
//! - an [`Instruction`]: config- or owner-authored text (a seat's goal,
//!   backstory and skills, a step's instruction);
//! - a [`TrustedInput`]: text that is trusted although it is not config,
//!   built only by am-engine through its two named constructors, from the
//!   journaled output of a step run that was untainted when it wrote it, or
//!   from an owner's note written under `Actor::Human`;
//! - [`UntrustedData`]: files, MCP results, web pages, tool results, and
//!   everything a tainted step wrote.
//!
//! There is no conversion from `UntrustedData` to `Instruction` or to
//! `TrustedInput`, none from `TrustedInput` to `Instruction` or back, and
//! none may be added. A step run is [`Taint::Tainted`] once any
//! `UntrustedData` enters its request; its taint only ever rises.
//!
//! Story 3.6 (AD-10 as amended 2026-11-24, owner Q4): a fourth, narrow kind
//! answers a successful `table_write`, the `EngineResult`. ToolGate builds
//! it from engine values only (the row id it minted, the version the write
//! produced, a fixed status word) through its one constructor,
//! `EngineResult::row_written`; it holds no row data, argument or model
//! text, and it does not taint. Every error result stays `UntrustedData`.
//! (Tests first: these doctests were written before the type; the
//! implementer may move them onto `EngineResult`.)
//!
//! The allowed use: built from a `RowWritten`, its label names the tool and
//! its text the row id; the same three values give the same result.
//!
//! ```
//! use am_domain::EngineResult;
//! use am_domain::rows::{RowStatus, RowWritten};
//! let row_id = uuid::Uuid::now_v7();
//! let written = RowWritten { row_id, version: 2, status: RowStatus::Updated };
//! let result = EngineResult::row_written(&written);
//! assert_eq!(result.label(), "tool:table_write");
//! assert!(result.text().contains(&row_id.to_string()));
//! assert_eq!(result, EngineResult::row_written(&written));
//! ```
//!
//! No `Default`:
//!
//! ```compile_fail
//! let _r = am_domain::EngineResult::default();
//! ```
//!
//! no `From` with text:
//!
//! ```compile_fail
//! let _r = am_domain::EngineResult::from(String::from("row 1 inserted"));
//! ```
//!
//! nor with untrusted data:
//!
//! ```compile_fail
//! let _r: am_domain::EngineResult = am_domain::UntrustedData::text("tool:table_write", "x").into();
//! ```
//!
//! no struct literal (its fields are private):
//!
//! ```compile_fail
//! let _r = am_domain::EngineResult { label: "tool:table_write".into(), text: "inserted".into() };
//! ```
//!
//! and no `Deserialize` (the probe holds for plain text, see
//! `TrustedInput`'s docs):
//!
//! ```compile_fail
//! fn deserializable<T: serde::de::DeserializeOwned>() {}
//! deserializable::<am_domain::EngineResult>();
//! ```

use serde::{Deserialize, Serialize};

use crate::Actor;
use crate::llm::Part;

/// Trusted, config- or owner-authored text. Build it only from config files or
/// the owner's own words, never from anything a tool, file or model returned
/// (a clean step's output is a [`TrustedInput`], never an `Instruction`).
///
/// The allowed build, beside the two other kinds of content:
///
/// ```
/// use am_domain::{Instruction, UntrustedData};
/// let goal = Instruction::authored("Keep every line short.");
/// assert_eq!(goal.text(), "Keep every line short.");
/// let owned = Instruction::authored(String::from("a backstory"));
/// assert_eq!(owned.text(), "a backstory");
/// let data = UntrustedData::text("test:page", "a page");
/// assert_eq!(data.label(), "test:page");
/// ```
///
/// There is no conversion from untrusted data to an `Instruction` (story
/// 3.5, AD-10), by `From`:
///
/// ```compile_fail
/// use am_domain::{Instruction, UntrustedData};
/// let _i = Instruction::from(UntrustedData::text("test:page", "a page"));
/// ```
///
/// by handing the data to the constructor:
///
/// ```compile_fail
/// use am_domain::{Instruction, UntrustedData};
/// let _i = Instruction::authored(UntrustedData::text("test:page", "a page"));
/// ```
///
/// or by `Into`:
///
/// ```compile_fail
/// use am_domain::{Instruction, UntrustedData};
/// let _i: Instruction = UntrustedData::text("test:page", "a page").into();
/// ```
///
/// and none the other way:
///
/// ```compile_fail
/// use am_domain::{Instruction, UntrustedData};
/// let _d: UntrustedData = Instruction::authored("a goal").into();
/// ```
///
/// An `Instruction` cannot be deserialized either (story 3.5, review R4:
/// `serde_json::from_value::<Instruction>` would build one from any text in
/// any crate), so the bound is refused where the probe itself is fine:
///
/// ```compile_fail
/// fn deserializable<T: serde::de::DeserializeOwned>() {}
/// deserializable::<am_domain::Instruction>();
/// ```
///
/// while the same probe takes plain text and an `Instruction` still
/// serializes:
///
/// ```
/// fn deserializable<T: serde::de::DeserializeOwned>() {}
/// deserializable::<String>();
/// fn serializable<T: serde::Serialize>() {}
/// serializable::<am_domain::Instruction>();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// Whether a step run read untrusted content (AD-10). A step run is
/// `Tainted` once any `UntrustedData` enters its request; the journal only
/// ever raises it (`Clean < Tainted`), and anything a tainted step run
/// writes is `UntrustedData` wherever it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Taint {
    Clean,
    Tainted,
}

impl Taint {
    /// `Tainted` when `flag` is set (a journal column's 0 or 1).
    pub fn of_flag(flag: bool) -> Taint {
        if flag { Taint::Tainted } else { Taint::Clean }
    }

    pub fn is_tainted(self) -> bool {
        self == Taint::Tainted
    }

    /// The higher of the two: taint only ever rises, so a clean value never
    /// lowers a tainted one.
    pub fn raise(self, other: Taint) -> Taint {
        self.max(other)
    }
}

/// Text that is trusted although it is not config (AD-10, amended
/// 2026-11-23; AD-27, AD-36): a clean step run's journaled output (a `next`
/// input, loop-back reasons) or an owner's note for one run's work. It is
/// its own [`Part`] (`Part::Trusted`), rendered with its label in the user
/// role, after the step instruction and outside the untrusted fence.
///
/// Its fields are private, it has no `Default` and no `Deserialize`, and no
/// `From` or `TryFrom` joins it to [`Instruction`], [`UntrustedData`] or
/// plain text, either way. Its only constructors are the two named ones,
/// and only am-engine calls them (`cargo xtask check-deps`). A tainted
/// producer's output never becomes one: the step-output constructor gives
/// `UntrustedData` with the same label instead.
///
/// The allowed build:
///
/// ```
/// use am_domain::{Part, Taint, TrustedInput};
/// let p = TrustedInput::from_step_output("step:a@1", "text", Taint::Clean);
/// assert!(matches!(p, Part::Trusted(_)));
/// let q = TrustedInput::from_step_output("step:a@1", "text", Taint::Tainted);
/// assert!(matches!(q, Part::Untrusted(_)));
/// ```
///
/// No conversion from untrusted data:
///
/// ```compile_fail
/// use am_domain::{TrustedInput, UntrustedData};
/// let _t = TrustedInput::from(UntrustedData::text("x", "y"));
/// ```
///
/// No conversion into an instruction:
///
/// ```compile_fail
/// use am_domain::{Instruction, Part, Taint, TrustedInput};
/// let Part::Trusted(t) = TrustedInput::from_step_output("step:a@1", "t", Taint::Clean) else { panic!() };
/// let _i = Instruction::from(t);
/// ```
///
/// No `Deserialize` (bounded on serde's trait; am-domain has no
/// serde_json):
///
/// ```compile_fail
/// fn deserializable<T: serde::de::DeserializeOwned>() {}
/// deserializable::<am_domain::TrustedInput>();
/// ```
///
/// while the same bound holds for plain text, so the refusal above is the
/// missing impl, not the probe (an `Instruction` has none either, see its
/// docs):
///
/// ```
/// fn deserializable<T: serde::de::DeserializeOwned>() {}
/// deserializable::<String>();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedInput {
    label: String,
    text: String,
}

impl TrustedInput {
    /// A step run's journaled output (or what the engine reads from it, such
    /// as loop-back reasons) as an input to a later step, with `producer`
    /// the producing step run's taint as committed in the journal: a
    /// `Part::Trusted` when it is clean, `Part::Untrusted` with the same
    /// label when it is tainted (AD-10 (a)). Only am-engine calls it.
    pub fn from_step_output(
        label: impl Into<String>,
        text: impl Into<String>,
        producer: Taint,
    ) -> Part {
        let (label, text) = (label.into(), text.into());
        match producer {
            Taint::Clean => Part::Trusted(TrustedInput { label, text }),
            Taint::Tainted => Part::Untrusted(UntrustedData::text(label, text)),
        }
    }

    /// An owner-authored note for one run's work (a Human return note at a
    /// gate, story 3.4): `Some` only when `author` is `Actor::Human`. Owner
    /// text that is config stays an [`Instruction`]. Only am-engine calls it.
    pub fn from_owner_note(
        label: impl Into<String>,
        text: impl Into<String>,
        author: &Actor,
    ) -> Option<TrustedInput> {
        match author {
            Actor::Human { .. } => Some(TrustedInput {
                label: label.into(),
                text: text.into(),
            }),
            _ => None,
        }
    }

    /// Where it came from, e.g. `step:<key>@<attempt>` or
    /// `loop-back:<key>@<attempt>`.
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The result of a successful `table_write` (AD-10 as amended 2026-11-24,
/// owner Q4): the fourth content kind, built by ToolGate alone through its
/// one constructor, [`EngineResult::row_written`], from engine values only
/// (the row id the engine minted, the version the write produced and one
/// fixed status word). It holds no row data, no argument and no model
/// text, so it does not taint (and never clears taint). Every error result
/// stays [`UntrustedData`]. Its fields are private, and it has no
/// `Default`, `From`, `TryFrom` or `Deserialize` (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineResult {
    label: String,
    text: String,
}

/// The label of every engine result: the tool it answers.
const ENGINE_RESULT_LABEL: &str = "tool:table_write";

impl EngineResult {
    /// The engine result of a committed row write. The text is built from
    /// `written` alone: the same row id, version and status give the same
    /// result, so a rebuild from the journal equals what the model received.
    pub fn row_written(written: &crate::rows::RowWritten) -> EngineResult {
        use crate::rows::RowStatus;
        let (id, v) = (written.row_id, written.version);
        let text = match written.status {
            RowStatus::Inserted => format!("Row {id} was inserted at version {v}."),
            RowStatus::Updated => format!("Row {id} was updated to version {v}."),
        };
        EngineResult {
            label: ENGINE_RESULT_LABEL.to_string(),
            text,
        }
    }

    /// The tool it answers: `tool:table_write`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The engine's words: the row id, the version and the status word.
    pub fn text(&self) -> &str {
        &self.text
    }
}
