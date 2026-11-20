//! Pure domain for Agentic Manager: value types, `Actor`, content parts and the
//! port traits the application core drives (architecture AD-4, AD-5).
//!
//! This crate depends only on std, serde, thiserror, jiff and uuid. It has no IO
//! and no async runtime: ports return [`BoxFuture`] built from std types so that
//! adapters choose their own runtime.
#![forbid(unsafe_code)]

pub mod actor;
pub mod audit;
pub mod clock;
pub mod content;
pub mod llm;
pub mod mcp;
pub mod net;
pub mod secrets;
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

pub use actor::Actor;
pub use audit::{AuditEvent, AuditSink};
pub use clock::Clock;
pub use content::{FileKind, FilePart, Instruction, UntrustedContent, UntrustedData};
pub use llm::{
    ChatOutcome, ChatRequest, EventSink, LlmProvider, Message, ModelCapabilities, Part,
    ProviderError, Role, StopReason, StreamEvent, ToolCall, ToolSpec, Usage,
};
pub use mcp::{McpClient, McpError, McpToolInfo};
pub use net::{
    AuthRef, AuthScheme, BodyChunks, Caller, EgressClient, EgressError, HttpMethod, HttpRequest,
    HttpResponse,
};
pub use secrets::{SecretError, SecretStore, SecretString};
pub use workspace::{Access, GrantRoot, PathError, WorkspacePath};
