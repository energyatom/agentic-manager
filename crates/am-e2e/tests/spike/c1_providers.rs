//! Criterion 1: the provider port. Anthropic and OpenAI-compatible adapters
//! stream text and tool calls through `LlmProvider`, usage is captured, and a
//! cancel mid-stream stops the stream within 1 s.

use std::sync::Arc;
use std::time::Duration;

use am_app::spike::{C1_STREAM_PROMPT, C1_TOOL_PROMPT, c1_probe};
use am_domain::{AuthScheme, LlmProvider, StopReason, Usage};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use am_testkit::{FakeEgress, FakeResponse, recorded};

use super::live;

/// A cancel that is ignored would hang the probe; fail instead.
async fn within<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), f)
        .await
        .expect("the probe did not finish within 10 s (cancel ignored?)")
}

fn anthropic_fake() -> Arc<FakeEgress> {
    let egress = FakeEgress::new();
    egress.respond_if(
        "/v1/messages",
        "lookup_word",
        FakeResponse::sse(recorded("anthropic/c1_tool_use.sse")),
    );
    egress.respond_if(
        "/v1/messages",
        "Count from 1",
        FakeResponse::sse(recorded("anthropic/c1_count_stream.sse"))
            .with_delay(Duration::from_millis(100))
            .hanging(),
    );
    egress
}

#[tokio::test]
async fn anthropic_streams_tool_call_with_usage_and_cancels_within_1s() {
    let egress = anthropic_fake();
    let provider = Arc::new(AnthropicProvider::new(egress.clone(), "spike-anthropic"));
    let r = within(c1_probe(provider, "claude-opus-5-5", 2048))
        .await
        .unwrap();

    let call = r.tool_call.clone().expect("a tool call");
    assert_eq!(call.name, "lookup_word");
    assert_eq!(call.arguments_json, r#"{"word": "otter"}"#);
    assert_eq!(
        r.tool_usage,
        Some(Usage {
            input_tokens: 412,
            output_tokens: 58
        })
    );
    assert!(
        r.streamed_text_deltas > 0,
        "text streamed before the cancel"
    );
    assert_eq!(r.cancelled_stop, StopReason::Cancelled);
    assert!(
        r.cancel_latency < Duration::from_secs(1),
        "stopped in {:?}",
        r.cancel_latency
    );
    // Usage up to the cancel is captured (message_start carried the input tokens).
    assert_eq!(r.usage_at_cancel.map(|u| u.input_tokens), Some(31));
    assert!(r.passed());
    // The aborted stream's body was dropped (the connection is closed).
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(egress.body_dropped());

    // The key is referenced, never carried: am-net resolves it from the keychain.
    let reqs = egress.requests();
    assert_eq!(reqs.len(), 2);
    for req in &reqs {
        let auth = req.auth.as_ref().expect("auth ref");
        assert_eq!(auth.scheme, AuthScheme::XApiKey);
        assert_eq!(auth.account_id, "spike-anthropic");
        assert!(req.headers.iter().all(|(k, _)| k != "x-api-key"));
        assert!(
            req.headers
                .contains(&("anthropic-version".into(), "2026-10-21".into()))
        );
        assert!(req.url.starts_with("https://api.anthropic.com/"));
    }
    let body = String::from_utf8(reqs[0].body.clone()).unwrap();
    assert!(body.contains(C1_TOOL_PROMPT) && body.contains("\"stream\":true"));
    let body = String::from_utf8(reqs[1].body.clone()).unwrap();
    assert!(body.contains(C1_STREAM_PROMPT));
}

fn openai_fake() -> Arc<FakeEgress> {
    let egress = FakeEgress::new();
    egress.respond_if(
        "/chat/completions",
        "lookup_word",
        FakeResponse::sse(recorded("openai_compat/c1_tool_use.sse")),
    );
    egress.respond_if(
        "/chat/completions",
        "Count from 1",
        FakeResponse::sse(recorded("openai_compat/c1_count_stream.sse"))
            .with_delay(Duration::from_millis(100))
            .hanging(),
    );
    egress
}

#[tokio::test]
async fn openai_compatible_streams_tool_call_with_usage_and_cancels_within_1s() {
    let egress = openai_fake();
    let provider = Arc::new(OpenAiCompatProvider::lm_studio(egress.clone()));
    assert_eq!(provider.account_id(), "local-lmstudio");
    let r = within(c1_probe(provider, "local-tool-model", 2048))
        .await
        .unwrap();
    let call = r.tool_call.clone().expect("a tool call");
    assert_eq!(call.id, "call_fixture01");
    assert_eq!(call.arguments_json, r#"{"word": "otter"}"#);
    assert_eq!(
        r.tool_usage,
        Some(Usage {
            input_tokens: 188,
            output_tokens: 21
        })
    );
    assert!(r.streamed_text_deltas > 0);
    assert_eq!(r.cancelled_stop, StopReason::Cancelled);
    assert!(r.cancel_latency < Duration::from_secs(1));
    // OpenAI-compatible servers report usage only at the end: none at cancel,
    // so the ledger settles at the reservation (AD-9; am-engine journal test).
    assert_eq!(r.usage_at_cancel, None);
    let reqs = egress.requests();
    assert!(
        reqs.iter()
            .all(|r| r.url == "http://127.0.0.1:1234/v1/chat/completions")
    );
    let body = String::from_utf8(reqs[0].body.clone()).unwrap();
    assert!(body.contains("\"include_usage\":true"));
}

#[tokio::test]
async fn http_errors_surface_with_status() {
    let egress = FakeEgress::new();
    egress.respond(
        "/v1/messages",
        FakeResponse::json(401, r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#),
    );
    let provider = Arc::new(AnthropicProvider::new(egress, "spike-anthropic"));
    let err = c1_probe(provider, "claude-opus-5-5", 64).await.unwrap_err();
    assert!(matches!(
        err,
        am_domain::ProviderError::Http { status: 401, .. }
    ));
}

#[tokio::test]
#[ignore = "live: needs AM_LIVE=1 and the Anthropic key in the Keychain"]
async fn live_anthropic_c1() {
    if !live::live_enabled() {
        return live::skip("AM_LIVE is not 1");
    }
    if !live::anthropic_key_present() {
        return live::skip(
            "no Anthropic key in the Keychain (service agentic-manager, account spike-anthropic)",
        );
    }
    let provider = Arc::new(AnthropicProvider::new(
        live::egress(),
        am_app::SPIKE_ANTHROPIC_ACCOUNT,
    ));
    let r = c1_probe(provider, &live::anthropic_model(), 2048)
        .await
        .unwrap();
    eprintln!("{r:#?}");
    assert!(r.passed(), "live Anthropic C1 failed: {r:#?}");
}

#[tokio::test]
#[ignore = "live: needs AM_LIVE=1 and LM Studio with a tool-capable model on 127.0.0.1:1234"]
async fn live_lm_studio_c1() {
    if !live::live_enabled() {
        return live::skip("AM_LIVE is not 1");
    }
    let Some(model) = live::lm_studio_model().await else {
        return live::skip("LM Studio is not answering on 127.0.0.1:1234");
    };
    let provider = Arc::new(OpenAiCompatProvider::lm_studio(live::egress()));
    let r = c1_probe(provider, &model, 2048).await.unwrap();
    eprintln!("{r:#?}");
    assert!(r.passed(), "live LM Studio C1 failed: {r:#?}");
}
