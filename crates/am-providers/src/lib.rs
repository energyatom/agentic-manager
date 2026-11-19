//! Own thin `LlmProvider` adapters (AD-5): Anthropic Messages and
//! OpenAI-chat-compatible (LM Studio, Ollama, any compatible endpoint), both
//! streaming SSE over `am-net`'s `EgressClient`.
//!
//! File parts (AD-31): each adapter maps a `FilePart` to its provider's native
//! input or refuses it with `ProviderError::Unsupported` before any byte is
//! sent. No adapter converts a file.
#![forbid(unsafe_code)]

pub mod anthropic;
mod common;
pub mod openai_compat;
pub mod sse;

pub use anthropic::AnthropicProvider;
pub use common::render_untrusted_text;
pub use openai_compat::OpenAiCompatProvider;
