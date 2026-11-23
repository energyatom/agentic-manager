//! The `LlmProvider` port (AD-5, AD-31).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::content::{FileKind, Instruction, TrustedInput, UntrustedContent, UntrustedData};
use crate::{BoxFuture, CancelSignal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON schema of the input, as JSON text.
    pub input_schema_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments, as JSON text.
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Trusted text (user role only).
    Instruction(Instruction),
    /// A trusted input (AD-10, AD-27): a clean step's output or an owner's
    /// note, with its label (user role only), after the step instruction and
    /// outside the untrusted fence; never in the system position.
    Trusted(TrustedInput),
    /// Fenced untrusted text or a native file part (user role only).
    Untrusted(UntrustedData),
    /// Text the model wrote earlier (assistant role only).
    AssistantText(String),
    /// A tool call the model made earlier (assistant role only).
    ToolCall(ToolCall),
    /// A tool's result (user role only); tool results are untrusted.
    ToolResult {
        call_id: String,
        content: UntrustedData,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    pub model: String,
    pub system: Vec<Instruction>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_output_tokens: u32,
}

impl ChatRequest {
    /// Every file part in the request, in order.
    pub fn file_parts(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for m in &self.messages {
            for p in &m.parts {
                let data = match p {
                    Part::Untrusted(d) => d,
                    Part::ToolResult { content, .. } => content,
                    _ => continue,
                };
                if let UntrustedContent::File(f) = data.content() {
                    out.push((f.name.clone(), f.media_type.clone()));
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Which file types a model reads natively (AD-31). Plain text is readable by every model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub reads_pdf: bool,
    pub reads_images: bool,
    pub reads_office: bool,
    pub reads_audio: bool,
}

impl ModelCapabilities {
    pub fn reads(&self, kind: FileKind) -> bool {
        match kind {
            FileKind::Text => true,
            FileKind::Pdf => self.reads_pdf,
            FileKind::Image => self.reads_images,
            FileKind::Office => self.reads_office,
            FileKind::Audio => self.reads_audio,
            FileKind::Other => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCall(ToolCall),
    /// Usage so far (cumulative), as reported by the provider.
    Usage(Usage),
}

/// Receives stream events as they arrive, so usage up to a cancel is known.
pub trait EventSink: Send + Sync {
    fn on_event(&self, event: StreamEvent);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Cancelled,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatOutcome {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    /// `None` when the provider never reported usage (the ledger then settles at
    /// the reservation, AD-9).
    pub usage: Option<Usage>,
    pub stop: StopReason,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProviderError {
    /// A file part whose type the model's flags exclude; raised before any byte is sent.
    #[error("{message}")]
    Unsupported {
        media_type: String,
        model: String,
        message: String,
    },
    #[error("network refused or failed: {0}")]
    Egress(String),
    #[error("the provider answered HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("unexpected provider stream: {0}")]
    Protocol(String),
}

impl ProviderError {
    /// The worded refusal for a file the model cannot read (AD-30).
    pub fn unsupported(file_name: &str, media_type: &str, model: &str) -> ProviderError {
        ProviderError::Unsupported {
            media_type: media_type.to_string(),
            model: model.to_string(),
            message: format!(
                "The model {model} cannot read {file_name} ({media_type}). Choose a model that reads this file type, or route the file to another seat. Nothing was sent."
            ),
        }
    }
}

pub trait LlmProvider: Send + Sync {
    /// The Account id this adapter serves (e.g. `local-lmstudio`).
    fn account_id(&self) -> String;
    /// File-input flags for a model on this Account.
    fn capabilities(&self, model: &str) -> ModelCapabilities;
    /// Refuse, before any request, every file part the model cannot read natively.
    fn check_file_parts(&self, request: &ChatRequest) -> Result<(), ProviderError>;
    /// Stream one model call. Events go to `sink` as they arrive (partial usage
    /// included); on cancel the stream is aborted and the outcome has
    /// `StopReason::Cancelled`, with `usage` set only if the provider had
    /// reported its final usage, so a cut-off call settles at its reservation.
    /// Implementations call `check_file_parts` first.
    fn stream(
        &self,
        request: ChatRequest,
        cancel: Arc<dyn CancelSignal>,
        sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>>;
}
