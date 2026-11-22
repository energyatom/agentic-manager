//! Story 2.2 (AD-12, AD-48): every request a provider adapter sends names
//! its caller, the Account it calls, so the egress client can hold it to
//! that Account's pinned endpoint. The Anthropic adapter and the reserved
//! local Accounts (LM Studio, Ollama) on am-testkit's `FakeEgress`, which
//! records each request; recorded fixtures, no network. (The OpenAI and
//! compatible adapter's own Account is asserted in `openai_compat.rs`.)
//! Written first.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use am_domain::net::Caller;
use am_domain::{
    BoxFuture, CancelSignal, ChatRequest, HttpRequest, Instruction, LlmProvider, Message, Part,
    Role,
};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use am_testkit::{CollectEvents, FakeEgress};

struct NeverCancel;

impl CancelSignal for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
    fn cancelled(&self) -> BoxFuture<()> {
        Box::pin(std::future::pending())
    }
}

fn request(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        system: Vec::new(),
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(Instruction::authored("Count to three."))],
        }],
        tools: Vec::new(),
        max_output_tokens: 64,
    }
}

async fn one_request(provider: &dyn LlmProvider, egress: &FakeEgress) -> HttpRequest {
    let _ = tokio::time::timeout(
        Duration::from_secs(10),
        provider.stream(
            request("m"),
            Arc::new(NeverCancel),
            Arc::new(CollectEvents::default()),
        ),
    )
    .await
    .expect("the stream ends within 10 s");
    let requests = egress.requests();
    assert_eq!(requests.len(), 1, "one request per call");
    requests[0].clone()
}

fn account(id: &str) -> Option<Caller> {
    Some(Caller::Account {
        account: id.into(),
        seat: None,
    })
}

#[tokio::test]
async fn the_anthropic_adapter_names_its_account_as_the_caller() {
    let egress = FakeEgress::new();
    egress.replay("/v1/messages", "anthropic/c1_count_stream.sse");
    let provider = AnthropicProvider::new(egress.clone(), "anthropic-main");
    let req = one_request(&provider, &egress).await;
    assert_eq!(req.url, "https://api.anthropic.com/v1/messages");
    assert_eq!(req.caller, account("anthropic-main"));
}

#[tokio::test]
async fn the_reserved_local_accounts_name_themselves_as_the_caller() {
    for (provider, id, url) in [
        (
            OpenAiCompatProvider::lm_studio as fn(_) -> OpenAiCompatProvider,
            "local-lmstudio",
            "http://127.0.0.1:1234/v1/chat/completions",
        ),
        (
            OpenAiCompatProvider::ollama,
            "local-ollama",
            "http://127.0.0.1:11434/v1/chat/completions",
        ),
    ] {
        let egress = FakeEgress::new();
        egress.replay("/chat/completions", "openai_compat/reasoning.sse");
        let adapter = provider(egress.clone());
        let req = one_request(&adapter, &egress).await;
        assert_eq!(req.url, url, "{id}");
        assert_eq!(req.caller, account(id), "{id}");
        assert_eq!(
            req.auth, None,
            "{id}: a reserved local Account sends no key"
        );
    }
}
