//! Epic 3 story 7, review round 1, M1 (AD-9; owner Q3): an adapter reports
//! a call's usage only when the provider gave both counts as valid
//! non-negative integers; otherwise the outcome's usage is `None`, so the
//! call settles at its reservation instead of at a made-up 0. Covered for
//! the Anthropic adapter (Anthropic's `message_start` input count included)
//! and the OpenAI-chat-compatible one: usage that is empty, partial,
//! negative or a string. A well-formed usage is reported as given (the
//! control). Streams written by hand in each provider's format on
//! am-testkit's `FakeEgress`; no network, no key. Written before the fix
//! (tester, 2026-11-25).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use am_domain::{
    BoxFuture, CancelSignal, ChatOutcome, ChatRequest, Instruction, LlmProvider, Message, Part,
    Role, StopReason, Usage,
};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use am_testkit::{CollectEvents, FakeEgress, FakeResponse};

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

async fn outcome(provider: &dyn LlmProvider) -> ChatOutcome {
    tokio::time::timeout(
        Duration::from_secs(10),
        provider.stream(
            request(),
            Arc::new(NeverCancel),
            Arc::new(CollectEvents::default()),
        ),
    )
    .await
    .expect("the stream ends within 10 s")
    .expect("the stream is read")
}

fn usage(input_tokens: u64, output_tokens: u64) -> Option<Usage> {
    Some(Usage {
        input_tokens,
        output_tokens,
    })
}

// ------------------------------------------------------------------ Anthropic

/// An Anthropic stream whose `message_start` carries `start` as its usage
/// object and whose `message_delta` carries `delta` (each a JSON text, or
/// empty for no `usage` key at all).
fn anthropic_stream(start: &str, delta: &str) -> Vec<u8> {
    let start_usage = if start.is_empty() {
        String::new()
    } else {
        format!(r#","usage":{start}"#)
    };
    let delta_usage = if delta.is_empty() {
        String::new()
    } else {
        format!(r#","usage":{delta}"#)
    };
    format!(
        concat!(
            "event: message_start\n",
            r#"data: {{"type":"message_start","message":{{"id":"msg_test","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null{}}}}}"#,
            "\n\n",
            "event: content_block_start\n",
            r#"data: {{"type":"content_block_start","index":0,"content_block":{{"type":"text","text":""}}}}"#,
            "\n\n",
            "event: content_block_delta\n",
            r#"data: {{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"one line"}}}}"#,
            "\n\n",
            "event: content_block_stop\n",
            r#"data: {{"type":"content_block_stop","index":0}}"#,
            "\n\n",
            "event: message_delta\n",
            r#"data: {{"type":"message_delta","delta":{{"stop_reason":"end_turn","stop_sequence":null}}{}}}"#,
            "\n\n",
            "event: message_stop\n",
            r#"data: {{"type":"message_stop"}}"#,
            "\n\n",
        ),
        start_usage, delta_usage
    )
    .into_bytes()
}

async fn anthropic_usage(start: &str, delta: &str) -> ChatOutcome {
    let egress = FakeEgress::new();
    egress.respond(
        "/v1/messages",
        FakeResponse::sse(anthropic_stream(start, delta)),
    );
    let provider = AnthropicProvider::new(egress, "test-anthropic");
    outcome(&provider).await
}

/// M1: the Anthropic adapter reports usage only from a valid input count
/// (in `message_start`, or `message_delta`) and a valid output count;
/// `message_start` with no `input_tokens`, an empty or partial usage, a
/// negative count or a count written as a string each give `None`.
#[tokio::test]
async fn anthropic_reports_no_usage_unless_both_counts_are_valid() {
    // The control: both counts given, the output's final count in
    // `message_delta`.
    let fine = anthropic_usage(
        r#"{"input_tokens":31,"output_tokens":1}"#,
        r#"{"output_tokens":9}"#,
    )
    .await;
    assert_eq!(fine.stop, StopReason::EndTurn);
    assert_eq!(fine.text, "one line");
    assert_eq!(fine.usage, usage(31, 9), "a well-formed usage is kept");

    for (case, start, delta) in [
        (
            "message_start with no input_tokens",
            r#"{"output_tokens":1}"#,
            r#"{"output_tokens":9}"#,
        ),
        ("an empty usage at both ends", "{}", "{}"),
        ("no usage at all", "", ""),
        (
            "an input count and no output count",
            r#"{"input_tokens":31}"#,
            "",
        ),
        (
            "an output count in message_delta only",
            "",
            r#"{"output_tokens":9}"#,
        ),
        (
            "a negative input count",
            r#"{"input_tokens":-31,"output_tokens":1}"#,
            r#"{"output_tokens":9}"#,
        ),
        (
            "a negative output count",
            r#"{"input_tokens":31,"output_tokens":1}"#,
            r#"{"output_tokens":-9}"#,
        ),
        (
            "an input count written as a string",
            r#"{"input_tokens":"31","output_tokens":1}"#,
            r#"{"output_tokens":9}"#,
        ),
        (
            "an output count written as a string",
            r#"{"input_tokens":31,"output_tokens":1}"#,
            r#"{"output_tokens":"9"}"#,
        ),
        (
            "a fractional input count",
            r#"{"input_tokens":31.5,"output_tokens":1}"#,
            r#"{"output_tokens":9}"#,
        ),
    ] {
        let out = anthropic_usage(start, delta).await;
        assert_eq!(out.text, "one line", "{case}: the reply is still read");
        assert_eq!(out.usage, None, "{case}: no usage, never a made-up 0");
    }
}

// ------------------------------------------------------------------ OpenAI-compatible

/// A chat-completions stream whose last chunk carries `usage` (a JSON text)
/// before `[DONE]`.
fn openai_stream(usage: &str) -> Vec<u8> {
    format!(
        concat!(
            r#"data: {{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1795000000,"model":"m","choices":[{{"index":0,"delta":{{"role":"assistant","content":"one line"}},"finish_reason":null}}],"usage":null}}"#,
            "\n\n",
            r#"data: {{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1795000000,"model":"m","choices":[{{"index":0,"delta":{{}},"finish_reason":"stop"}}],"usage":null}}"#,
            "\n\n",
            r#"data: {{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1795000000,"model":"m","choices":[],"usage":{}}}"#,
            "\n\n",
            "data: [DONE]\n\n",
        ),
        usage
    )
    .into_bytes()
}

async fn openai_usage(base_url: &str, usage: &str) -> ChatOutcome {
    let egress = FakeEgress::new();
    egress.respond("/chat/completions", FakeResponse::sse(openai_stream(usage)));
    let provider = OpenAiCompatProvider::new(egress, "test-compatible", base_url);
    outcome(&provider).await
}

/// M1: the OpenAI-chat-compatible adapter (OpenAI's endpoint and a server
/// on this Mac alike) reports usage only when `prompt_tokens` and
/// `completion_tokens` are both valid counts; an empty, partial, negative
/// or string usage gives `None`.
#[tokio::test]
async fn openai_compat_reports_no_usage_unless_both_counts_are_valid() {
    for base_url in ["https://api.openai.com/v1", "http://127.0.0.1:8080/v1"] {
        let fine = openai_usage(
            base_url,
            r#"{"prompt_tokens":24,"completion_tokens":6,"total_tokens":30}"#,
        )
        .await;
        assert_eq!(fine.text, "one line", "{base_url}");
        assert_eq!(fine.usage, usage(24, 6), "{base_url}: the control");

        for (case, given) in [
            ("an empty usage", "{}"),
            ("a prompt count only", r#"{"prompt_tokens":24}"#),
            ("a completion count only", r#"{"completion_tokens":6}"#),
            (
                "a negative prompt count",
                r#"{"prompt_tokens":-24,"completion_tokens":6}"#,
            ),
            (
                "a negative completion count",
                r#"{"prompt_tokens":24,"completion_tokens":-6}"#,
            ),
            (
                "a prompt count written as a string",
                r#"{"prompt_tokens":"24","completion_tokens":6}"#,
            ),
            (
                "a completion count written as a string",
                r#"{"prompt_tokens":24,"completion_tokens":"6"}"#,
            ),
            ("a usage that is not an object", r#""24 and 6""#),
        ] {
            let out = openai_usage(base_url, given).await;
            assert_eq!(out.text, "one line", "{base_url}, {case}");
            assert_eq!(out.usage, None, "{base_url}, {case}: never a made-up 0");
        }
    }
}
