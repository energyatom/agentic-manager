//! Story 2.3: the one OpenAI-chat-compatible adapter behind `LlmProvider`,
//! on recordings under `fixtures/http/` (written by hand in each provider's
//! stream format; no live capture, no key, no network). One test per adapter
//! row of the story's I/O matrix:
//!
//! - text streams as deltas, with usage (`stream_options.include_usage`), and
//!   the one request goes to `{base_url}/chat/completions`;
//! - two tool calls arrive as two `ToolCall` events with their own ids, names
//!   and arguments, on an OpenAI recording and on a compatible one, and on a
//!   recording whose tool calls carry no `index` (assembled by id, never
//!   merged); one call whose every fragment carries a fresh id stays one
//!   call, and two whole calls numbered the same `index` stay two;
//! - a cancel on a hanging stream returns `Cancelled` within 1 s with no
//!   tool call;
//! - `reasoning_content` deltas stay out of the answer text (spike finding 8).
//!
//! Written before the adapter was changed.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use am_domain::{
    AuthRef, AuthScheme, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, HttpMethod,
    Instruction, LlmProvider, Message, Part, Role, StopReason, StreamEvent, ToolCall, ToolSpec,
    Usage,
};
use am_providers::OpenAiCompatProvider;
use am_testkit::{CollectEvents, FakeEgress, FakeResponse, recorded};
use tokio::sync::watch;

const OPENAI: &str = "https://api.openai.com/v1";
const REMOTE: &str = "https://openrouter.example/api/v1";
const LOOPBACK: &str = "http://127.0.0.1:8080/v1";

/// A cancel signal the test fires by hand.
struct Cancel(watch::Sender<bool>);

impl Cancel {
    fn new() -> Arc<Self> {
        Arc::new(Cancel(watch::channel(false).0))
    }
    fn fire(&self) {
        self.0.send_replace(true);
    }
}

impl CancelSignal for Cancel {
    fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    fn cancelled(&self) -> BoxFuture<()> {
        let mut rx = self.0.subscribe();
        Box::pin(async move {
            let _ = rx.wait_for(|c| *c).await;
        })
    }
}

fn tool(name: &str, field: &str) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("The {name} tool."),
        input_schema_json: format!(
            r#"{{"type":"object","properties":{{"{field}":{{"type":"string"}}}},"required":["{field}"]}}"#
        ),
    }
}

/// A request that offers the two tools of the recordings.
fn request(model: &str, prompt: &str) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        system: Vec::new(),
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(Instruction::authored(prompt))],
        }],
        tools: vec![tool("lookup_word", "word"), tool("count_letters", "text")],
        max_output_tokens: 256,
    }
}

/// The adapter for an Account `account` at `base_url`, as `accounts test`
/// builds it: `Bearer` from the Keychain entry named by the Account's id.
fn adapter(egress: &Arc<FakeEgress>, account: &str, base_url: &str) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new(egress.clone(), account, base_url).with_auth(AuthRef {
        scheme: AuthScheme::Bearer,
        account_id: account.into(),
    })
}

/// Streams `fixture` (byte-exact) from `base_url` and returns the outcome
/// with every event the sink saw.
async fn play(base_url: &str, fixture: &str) -> (ChatOutcome, Vec<StreamEvent>, Arc<FakeEgress>) {
    let egress = FakeEgress::new();
    egress.replay("/chat/completions", fixture);
    let sink = Arc::new(CollectEvents::default());
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        adapter(&egress, "acct", base_url).stream(
            request("m", "Look the word up and count its letters."),
            Cancel::new(),
            sink.clone(),
        ),
    )
    .await
    .expect("the stream ends within 10 s")
    .expect("the stream is read");
    (outcome, sink.events(), egress)
}

fn tool_call_events(events: &[StreamEvent]) -> Vec<ToolCall> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ToolCall(c) => Some(c.clone()),
            _ => None,
        })
        .collect()
}

fn text_deltas(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn usage_events(events: &[StreamEvent]) -> Vec<Usage> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Usage(u) => Some(*u),
            _ => None,
        })
        .collect()
}

fn call(id: &str, name: &str, arguments_json: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments_json: arguments_json.into(),
    }
}

/// The one request of a played stream: a POST to `{base_url}/chat/completions`
/// and nowhere else, asking for a stream with usage, carrying a reference to
/// the Account's key and never a key.
fn assert_one_request(egress: &FakeEgress, base_url: &str) {
    let requests = egress.requests();
    assert_eq!(requests.len(), 1, "one request per call");
    let req = &requests[0];
    assert_eq!(req.method, HttpMethod::Post);
    assert_eq!(req.url, format!("{base_url}/chat/completions"));
    assert_eq!(
        req.auth,
        Some(AuthRef {
            scheme: AuthScheme::Bearer,
            account_id: "acct".into()
        })
    );
    assert!(
        req.headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("authorization")),
        "the key is referenced, never carried: {:?}",
        req.headers
    );
    assert_eq!(req.caller.account.as_deref(), Some("acct"));
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("a JSON body");
    assert_eq!(body["stream"], true, "{body}");
    assert_eq!(body["stream_options"]["include_usage"], true, "{body}");
    assert_eq!(body["model"], "m", "{body}");
}

// ------------------------------------------------------------------ text

/// Text streams as deltas in order, the outcome holds their concatenation,
/// usage is reported to the sink and in the outcome, and the request went
/// only to `{base_url}/chat/completions`.
#[tokio::test]
async fn openai_text_streams_as_deltas_with_usage() {
    let (outcome, events, egress) = play(OPENAI, "openai/text.sse").await;

    assert_eq!(outcome.text, "one\ntwo\nthree\n");
    let deltas = text_deltas(&events);
    assert!(deltas.len() > 1, "streamed, not buffered: {deltas:?}");
    assert_eq!(deltas.concat(), outcome.text);
    assert_eq!(outcome.stop, StopReason::EndTurn);
    assert!(outcome.tool_calls.is_empty());
    let usage = Usage {
        input_tokens: 24,
        output_tokens: 6,
    };
    assert_eq!(outcome.usage, Some(usage));
    assert_eq!(usage_events(&events), vec![usage], "{events:?}");
    assert_one_request(&egress, OPENAI);
}

// ------------------------------------------------------------- tool calls

/// OpenAI cloud: two tool calls streamed by `index`, each in fragments.
#[tokio::test]
async fn openai_streams_two_tool_calls_with_usage() {
    let (outcome, events, egress) = play(OPENAI, "openai/two_tools.sse").await;

    let expected = vec![
        call("call_fixture_oa_1", "lookup_word", r#"{"word": "otter"}"#),
        call("call_fixture_oa_2", "count_letters", r#"{"text": "otter"}"#),
    ];
    assert_eq!(tool_call_events(&events), expected, "{events:?}");
    assert_eq!(outcome.tool_calls, expected);
    assert_eq!(outcome.stop, StopReason::ToolUse);
    assert_eq!(outcome.text, "");
    let usage = Usage {
        input_tokens: 96,
        output_tokens: 41,
    };
    assert_eq!(outcome.usage, Some(usage));
    assert_eq!(usage_events(&events), vec![usage], "{events:?}");
    assert_one_request(&egress, OPENAI);
}

/// A compatible endpoint, remote and on this Mac: a short text, then two
/// tool calls (the first in fragments, the second whole in one delta).
#[tokio::test]
async fn compatible_streams_two_tool_calls_with_usage() {
    for base_url in [REMOTE, LOOPBACK] {
        let (outcome, events, egress) = play(base_url, "openai_compat/two_tools.sse").await;

        let expected = vec![
            call("call_fixture_c1", "lookup_word", r#"{"word": "heron"}"#),
            call("call_fixture_c2", "count_letters", r#"{"text": "heron"}"#),
        ];
        assert_eq!(
            tool_call_events(&events),
            expected,
            "{base_url}: {events:?}"
        );
        assert_eq!(outcome.tool_calls, expected, "{base_url}");
        assert_eq!(outcome.stop, StopReason::ToolUse, "{base_url}");
        assert_eq!(outcome.text, "Looking both up.", "{base_url}");
        let usage = Usage {
            input_tokens: 203,
            output_tokens: 37,
        };
        assert_eq!(outcome.usage, Some(usage), "{base_url}");
        assert_eq!(usage_events(&events), vec![usage], "{base_url}: {events:?}");
        assert_one_request(&egress, base_url);
    }
}

/// A server that omits `index` on its tool calls: the two calls are told
/// apart by their ids and never merged into one.
#[tokio::test]
async fn tool_calls_without_an_index_are_kept_apart_by_id() {
    let (outcome, events, egress) = play(REMOTE, "openai_compat/two_tools_no_index.sse").await;

    let expected = vec![
        call("call_fixture_n1", "lookup_word", r#"{"word": "wren"}"#),
        call("call_fixture_n2", "count_letters", r#"{"text": "wren"}"#),
    ];
    assert_eq!(
        tool_call_events(&events),
        expected,
        "two calls, not one merged call: {events:?}"
    );
    assert_eq!(outcome.tool_calls, expected);
    assert_eq!(outcome.stop, StopReason::ToolUse);
    let usage = Usage {
        input_tokens: 150,
        output_tokens: 29,
    };
    assert_eq!(outcome.usage, Some(usage));
    assert_eq!(usage_events(&events), vec![usage], "{events:?}");
    assert_one_request(&egress, REMOTE);
}

/// A server that stamps a fresh `id` on every fragment of one tool call
/// (review of story 2.3): the fragments share an `index` and only the first
/// brings the function's name, so they are one call, with the first id and
/// its arguments whole, and never one call per fragment.
#[tokio::test]
async fn a_call_whose_fragments_each_carry_a_fresh_id_is_one_call() {
    let (outcome, events, egress) = play(LOOPBACK, "openai_compat/one_tool_fresh_ids.sse").await;

    let expected = vec![call(
        "call_fixture_f1",
        "lookup_word",
        r#"{"word": "finch"}"#,
    )];
    assert_eq!(
        tool_call_events(&events),
        expected,
        "one call, not one per fragment: {events:?}"
    );
    assert_eq!(outcome.tool_calls, expected);
    assert_eq!(outcome.stop, StopReason::ToolUse);
    let usage = Usage {
        input_tokens: 88,
        output_tokens: 17,
    };
    assert_eq!(outcome.usage, Some(usage));
    assert_eq!(usage_events(&events), vec![usage], "{events:?}");
    assert_one_request(&egress, LOOPBACK);
}

/// A server that numbers every tool call `index` 0 and sends each whole,
/// with its own id and name: a fragment that brings another id and a
/// function name starts another call, so the two are never merged.
#[tokio::test]
async fn two_whole_calls_numbered_the_same_index_are_two_calls() {
    let (outcome, events, egress) = play(REMOTE, "openai_compat/two_tools_same_index.sse").await;

    let expected = vec![
        call("call_fixture_s1", "lookup_word", r#"{"word": "crane"}"#),
        call("call_fixture_s2", "count_letters", r#"{"text": "crane"}"#),
    ];
    assert_eq!(
        tool_call_events(&events),
        expected,
        "two calls, not one merged call: {events:?}"
    );
    assert_eq!(outcome.tool_calls, expected);
    assert_eq!(outcome.stop, StopReason::ToolUse);
    let usage = Usage {
        input_tokens: 120,
        output_tokens: 33,
    };
    assert_eq!(outcome.usage, Some(usage));
    assert_eq!(usage_events(&events), vec![usage], "{events:?}");
    assert_one_request(&egress, REMOTE);
}

/// A server that numbers every tool call `index` 0 and sends each whole
/// with its name but no `id` (second review of story 2.3): a fragment that
/// brings a function name while the call at that index already has one is
/// another call, whatever the ids say, so the two are never merged into one
/// call with their names run together.
#[tokio::test]
async fn two_whole_calls_at_the_same_index_without_ids_are_two_calls() {
    let (outcome, events, egress) =
        play(LOOPBACK, "openai_compat/two_tools_same_index_no_ids.sse").await;

    let expected = vec![
        call("", "lookup_word", r#"{"word": "stork"}"#),
        call("", "count_letters", r#"{"text": "stork"}"#),
    ];
    assert_eq!(
        tool_call_events(&events),
        expected,
        "two calls, not one merged call: {events:?}"
    );
    assert_eq!(outcome.tool_calls, expected);
    assert_eq!(outcome.stop, StopReason::ToolUse);
    let usage = Usage {
        input_tokens: 111,
        output_tokens: 27,
    };
    assert_eq!(outcome.usage, Some(usage));
    assert_eq!(usage_events(&events), vec![usage], "{events:?}");
    assert_one_request(&egress, LOOPBACK);
}

// ---------------------------------------------------------------- cancel

/// The stream stops after both tool calls were streamed but before the
/// server said it had finished, and then hangs. A cancel returns `Cancelled`
/// within 1 s; no tool call is emitted to the sink or returned in the
/// outcome (a call the server never finished is not a call), and the body
/// is dropped.
#[tokio::test]
async fn cancel_on_a_hanging_stream_returns_within_1s_with_no_tool_call() {
    for base_url in [OPENAI, LOOPBACK] {
        let mut hanging = FakeResponse::sse(recorded("openai_compat/two_tools.sse"));
        let finished = hanging
            .chunks
            .iter()
            .position(|c| String::from_utf8_lossy(c).contains(r#""finish_reason":"tool_calls""#))
            .expect("the recording has a finish chunk");
        hanging.chunks.truncate(finished);
        assert!(
            hanging
                .chunks
                .iter()
                .any(|c| String::from_utf8_lossy(c).contains("call_fixture_c2")),
            "both calls are streamed before the hang"
        );
        let egress = FakeEgress::new();
        egress.respond("/chat/completions", hanging.hanging());

        let cancel = Cancel::new();
        let sink = Arc::new(CollectEvents::default());
        let provider = adapter(&egress, "acct", base_url);
        let stream = tokio::spawn(provider.stream(
            request("m", "Look the word up and count its letters."),
            cancel.clone(),
            sink.clone(),
        ));

        // The stream is being read (its text arrived); then everything the
        // server sent has time to arrive before the cancel.
        let waited = Instant::now();
        while text_deltas(&sink.events()).is_empty() {
            assert!(
                waited.elapsed() < Duration::from_secs(10),
                "{base_url}: no text within 10 s"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!stream.is_finished(), "{base_url}: the stream hangs");

        let fired = Instant::now();
        cancel.fire();
        let outcome = tokio::time::timeout(Duration::from_secs(10), stream)
            .await
            .expect("the cancel is not ignored")
            .expect("the stream task ends")
            .expect("a cancel is an outcome, not an error");
        let latency = fired.elapsed();

        assert_eq!(outcome.stop, StopReason::Cancelled, "{base_url}");
        assert!(
            latency < Duration::from_secs(1),
            "{base_url}: stopped in {latency:?}"
        );
        assert!(
            tool_call_events(&sink.events()).is_empty(),
            "{base_url}: no tool call is emitted: {:?}",
            sink.events()
        );
        assert!(
            outcome.tool_calls.is_empty(),
            "{base_url}: no tool call is returned: {:?}",
            outcome.tool_calls
        );
        assert_eq!(outcome.text, "Looking both up.", "{base_url}");
        // The server never reported usage, so the call settles at its
        // reservation (AD-9).
        assert_eq!(outcome.usage, None, "{base_url}");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(egress.body_dropped(), "{base_url}: the body is dropped");
        assert_eq!(egress.requests().len(), 1, "{base_url}");
    }
}

// ------------------------------------------------------------- reasoning

/// A reasoning model streams its thinking as `reasoning_content` deltas
/// before any `content` (spike finding 8): the answer text, and every text
/// delta, hold only the content.
#[tokio::test]
async fn reasoning_deltas_stay_out_of_the_answer_text() {
    let (outcome, events, egress) = play(LOOPBACK, "openai_compat/reasoning.sse").await;

    assert_eq!(outcome.text, "The answer is 4.");
    assert_eq!(text_deltas(&events).concat(), "The answer is 4.");
    for thought in ["The question asks", "two plus two", "so I will say"] {
        assert!(
            !format!("{events:?}{outcome:?}").contains(thought),
            "the thinking reached the answer: {events:?}"
        );
    }
    assert_eq!(outcome.stop, StopReason::EndTurn);
    assert!(outcome.tool_calls.is_empty());
    assert_eq!(
        outcome.usage,
        Some(Usage {
            input_tokens: 18,
            output_tokens: 31
        })
    );
    assert_one_request(&egress, LOOPBACK);
}
