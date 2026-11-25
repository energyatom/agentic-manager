//! Story 2.10, review round 2, K2 (S2): a provider's error body is cut at
//! 2,000 bytes before it reaches a refusal (`ProviderError::Http`). The
//! cut falls on a character boundary: a body of 1,999 ASCII bytes followed
//! by `é` (two bytes, across the limit) never panics, and what is kept is
//! valid UTF-8 at or under the limit, the whole ASCII run kept. Covered for
//! both adapters, which share the error path (`common.rs`, whose own unit
//! tests live in `src/`; this file reaches it through the public adapters
//! instead, as the tests may not change `src/`). Bodies written by hand on
//! am-testkit's `FakeEgress`; no network, no key. Written before the fix
//! (tester, 2026-11-25).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use am_domain::{
    BoxFuture, CancelSignal, ChatRequest, EgressClient, Instruction, LlmProvider, Message, Part,
    ProviderError, Role,
};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use am_testkit::{CollectEvents, FakeEgress, FakeResponse};

/// The cap on an error body kept for a refusal.
const LIMIT: usize = 2000;

struct NeverCancel;

impl CancelSignal for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
    fn cancelled(&self) -> BoxFuture<()> {
        Box::pin(std::future::pending())
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "m".into(),
        system: Vec::new(),
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(Instruction::authored("Say one line."))],
        }],
        tools: Vec::new(),
        max_output_tokens: 64,
    }
}

/// The adapter named `which`, on an egress that answers HTTP 500 with
/// `body` to every call.
fn adapter(which: &str, body: &str) -> Box<dyn LlmProvider> {
    let egress = FakeEgress::new();
    match which {
        "anthropic" => {
            egress.respond("/v1/messages", FakeResponse::json(500, body));
            Box::new(AnthropicProvider::new(
                egress as Arc<dyn EgressClient>,
                "test-anthropic",
            ))
        }
        _ => {
            egress.respond("/chat/completions", FakeResponse::json(500, body));
            Box::new(OpenAiCompatProvider::new(
                egress as Arc<dyn EgressClient>,
                "test-compatible",
                "http://127.0.0.1:8080/v1",
            ))
        }
    }
}

/// K2: an error body of 1,999 ASCII bytes, then `é` (bytes 1,999 and
/// 2,000), then more text. Neither adapter panics; each refuses with
/// `ProviderError::Http` whose body is at most 2,000 bytes, valid UTF-8,
/// and keeps the 1,999 ASCII bytes.
#[tokio::test]
async fn an_error_body_cut_inside_a_character_never_panics_and_stays_within_the_limit() {
    let ascii = "a".repeat(LIMIT - 1);
    let body = format!("{ascii}é and more after the limit");
    assert!(!body.is_char_boundary(LIMIT), "the limit falls inside `é`");

    for which in ["anthropic", "openai-compatible"] {
        let provider = adapter(which, &body);
        let call = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(10),
                provider.stream(
                    request(),
                    Arc::new(NeverCancel),
                    Arc::new(CollectEvents::default()),
                ),
            )
            .await
        });
        let outcome = match call.await {
            Ok(outcome) => outcome.expect("the call ends within 10 s"),
            Err(e) if e.is_panic() => {
                panic!("{which}: the adapter panicked on an error body cut inside a character")
            }
            Err(e) => panic!("{which}: the call did not finish: {e}"),
        };
        match outcome {
            Err(ProviderError::Http { status, body: kept }) => {
                assert_eq!(status, 500, "{which}");
                assert!(
                    kept.len() <= LIMIT,
                    "{which}: at or under the limit ({} bytes)",
                    kept.len()
                );
                assert!(
                    kept.starts_with(&ascii),
                    "{which}: the ASCII run before the limit is kept"
                );
                assert!(
                    std::str::from_utf8(kept.as_bytes()).is_ok(),
                    "{which}: valid UTF-8"
                );
                assert!(
                    !kept.contains('\u{FFFD}'),
                    "{which}: no replacement character"
                );
            }
            Err(other) => panic!("{which}: expected ProviderError::Http, got {other:?}"),
            Ok(_) => panic!("{which}: expected a refusal of the HTTP 500"),
        }
    }
}
