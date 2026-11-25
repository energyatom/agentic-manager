//! Pure domain for Agentic Manager: value types, `Actor`, content parts and the
//! port traits the application core drives (architecture AD-4, AD-5).
//!
//! This crate depends only on std, serde, thiserror, jiff and uuid. It has no IO
//! and no async runtime: ports return [`BoxFuture`] built from std types so that
//! adapters choose their own runtime.
//!
//! Story 3.6 (AD-4, AD-5, AD-21): the row-writer port's types in
//! `am_domain::rows` are owned (std, `uuid`, am-domain types), so no store,
//! SQLite or JSON-library type crosses the port; row data crosses as JSON
//! text.
//!
//! ```
//! use am_domain::Taint;
//! use am_domain::rows::{RowOp, RowWrite};
//! let row_id = uuid::Uuid::now_v7();
//! let insert = RowWrite {
//!     tool_call_id: String::from("tool-call-1"),
//!     step_run_id: String::from("step-run-1"),
//!     call_seq: 1,
//!     table_id: String::from("entries"),
//!     op: RowOp::Insert { row_id, data_json: String::from(r#"{"ref":"r-1"}"#) },
//!     taint: Taint::Clean,
//! };
//! let update = RowWrite {
//!     call_seq: 2,
//!     op: RowOp::Update { row_id, expected_version: 1_i64, patch_json: String::from("{}") },
//!     taint: Taint::Tainted,
//!     ..insert.clone()
//! };
//! assert_ne!(insert, update);
//! fn owned<T: Send + Sync + 'static>(_: &T) {}
//! owned(&insert);
//! ```
#![forbid(unsafe_code)]

pub mod actor;
pub mod audit;
pub mod capability;
pub mod clock;
pub mod config_history;
pub mod config_names;
pub mod content;
pub mod host;
pub mod inbox;
pub mod llm;
pub mod mcp;
pub mod net;
pub mod rows;
pub mod runtime;
pub mod secrets;
pub mod terminal;
pub mod text;
pub mod tool_manifest;
pub mod workspace;

use std::future::Future;
use std::pin::Pin;

/// A boxed, sendable, owned future: the return type of every async port method.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A cancellation signal handed to adapters. The engine implements it over its
/// cancellation tree (AD-20); adapters only observe it.
pub trait CancelSignal: Send + Sync {
    /// True once cancellation was requested.
    fn is_cancelled(&self) -> bool;
    /// Resolves when cancellation is requested (immediately if it already was).
    fn cancelled(&self) -> BoxFuture<()>;
}

pub use actor::{Actor, HumanActor, REVIEWER_HAT};
pub use audit::{AuditEvent, AuditSink};
pub use clock::Clock;
pub use config_history::{
    AuditNote, ConfigHistory, HistoryEntry, HistoryError, HistoryKind, NewIntent, PendingIntent,
};
pub use config_names::{CONFIG_NAMES, config_name_in, config_name_of, is_config_bearing};
pub use content::{
    EngineResult, FileKind, FilePart, Instruction, Taint, TrustedInput, UntrustedContent,
    UntrustedData,
};
pub use host::{HostPattern, HostPatternError};
pub use inbox::{Decision, InboxKind, StepStatus};
pub use llm::{
    ChatOutcome, ChatRequest, EventSink, LlmProvider, Message, ModelCapabilities, Part,
    ProviderError, Role, StopReason, StreamEvent, ToolCall, ToolSpec, Usage,
};
pub use mcp::{McpClient, McpError, McpToolInfo};
pub use net::{
    AuthRef, AuthScheme, BodyChunks, Caller, EgressClient, EgressDirectory, EgressError, Endpoint,
    HttpMethod, HttpRequest, HttpResponse, reserved_local_account,
};
pub use runtime::{
    IdentityError, MacId, ProjectId, ProjectIdentity, RootIdentity, RuntimeBase, RuntimeLayout,
    VolumeState,
};
pub use secrets::{SecretError, SecretStore, SecretString};
pub use terminal::{Terminal, TerminalError};
pub use tool_manifest::ToolRunner;
pub use workspace::{Access, GrantRoot, PathError, WorkspacePath};
