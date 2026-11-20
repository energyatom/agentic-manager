//! The test kit's own tests (R8, AD-33): one per row of story 1.3's I/O matrix.
//! Nothing here reaches the network, a real model or the keychain; time-dependent
//! tests run on tokio's paused clock.

use std::sync::Arc;
use std::time::Duration;

use am_domain::{
    BoxFuture, CancelSignal, ChatOutcome, ChatRequest, Clock, EgressClient, EgressError,
    LlmProvider, ProviderError, SecretStore, SecretString, StopReason, StreamEvent, ToolCall,
    Usage,
};
use am_domain::{HttpRequest, HttpResponse};
use am_testkit::{
    CollectEvents, FakeClock, FakeEgress, FakeLlm, FakeSecretStore, ScriptedTurn, recorded,
};
use tokio::sync::watch;

// ---------------------------------------------------------------- helpers

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

fn request(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        system: vec![],
        messages: vec![],
        tools: vec![],
        max_output_tokens: 64,
    }
}

async fn run(llm: &FakeLlm, model: &str) -> (Result<ChatOutcome, ProviderError>, Vec<StreamEvent>) {
    let sink = Arc::new(CollectEvents::default());
    let out = llm
        .stream(request(model), Cancel::new(), sink.clone())
        .await;
    (out, sink.events())
}

async fn read_chunks(resp: HttpResponse) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(chunk) = resp.body.next().await {
        out.push(chunk.expect("chunk"));
    }
    out
}

// ---------------------------------------------------------------- FakeLlm

#[tokio::test(start_paused = true)]
async fn streamed_text_arrives_word_by_word_then_usage() {
    let llm = FakeLlm::new(vec![ScriptedTurn::text("alpha beta gamma").usage(10, 3)]);
    let (out, events) = run(&llm, "m").await;
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 3,
    };
    assert_eq!(
        events,
        vec![
            StreamEvent::TextDelta("alpha ".into()),
            StreamEvent::TextDelta("beta ".into()),
            StreamEvent::TextDelta("gamma".into()),
            StreamEvent::Usage(usage),
        ]
    );
    let out = out.expect("outcome");
    assert_eq!(out.text, "alpha beta gamma");
    assert_eq!(out.usage, Some(usage));
    assert_eq!(out.stop, StopReason::EndTurn);
    assert!(out.tool_calls.is_empty());
}

#[tokio::test(start_paused = true)]
async fn two_tool_calls_stream_in_order_and_stop_for_tool_use() {
    let llm = FakeLlm::new(vec![
        ScriptedTurn::text("")
            .tool_call("search", r#"{"q":"alpha"}"#)
            .tool_call("fetch", r#"{"url":"https://example.invalid/"}"#)
            .usage(20, 7),
    ]);
    let (out, events) = run(&llm, "m").await;
    let calls = vec![
        ToolCall {
            id: "call-1".into(),
            name: "search".into(),
            arguments_json: r#"{"q":"alpha"}"#.into(),
        },
        ToolCall {
            id: "call-2".into(),
            name: "fetch".into(),
            arguments_json: r#"{"url":"https://example.invalid/"}"#.into(),
        },
    ];
    assert_eq!(
        events,
        vec![
            StreamEvent::ToolCall(calls[0].clone()),
            StreamEvent::ToolCall(calls[1].clone()),
            StreamEvent::Usage(Usage {
                input_tokens: 20,
                output_tokens: 7,
            }),
        ]
    );
    let out = out.expect("outcome");
    assert_eq!(out.tool_calls, calls);
    assert_eq!(out.stop, StopReason::ToolUse);
}

#[tokio::test(start_paused = true)]
async fn a_hanging_turn_times_out_at_the_callers_deadline_and_cancel_ends_it() {
    let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::hang()]));
    let sink = Arc::new(CollectEvents::default());

    // The caller's deadline elapses; the hang never answers on its own.
    let started = tokio::time::Instant::now();
    let timed_out = tokio::time::timeout(
        Duration::from_secs(30),
        llm.stream(request("m"), Cancel::new(), sink.clone()),
    )
    .await;
    assert!(timed_out.is_err(), "the hang must not answer by itself");
    assert_eq!(started.elapsed(), Duration::from_secs(30));
    assert_eq!(llm.calls(), 1, "the fake records that the call was made");
    assert_eq!(llm.requests(), vec![request("m")]);
    assert!(sink.events().is_empty());

    // Cancel ends a hang with `Cancelled`.
    let cancel = Cancel::new();
    let call = tokio::spawn({
        let llm = llm.clone();
        let cancel = cancel.clone();
        let sink = sink.clone();
        async move { llm.stream(request("m"), cancel, sink).await }
    });
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!call.is_finished());
    cancel.fire();
    let out = call.await.expect("join").expect("outcome");
    assert_eq!(out.stop, StopReason::Cancelled);
    assert_eq!(out.usage, None);
    assert_eq!(llm.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_fault_returns_the_scripted_error_and_no_text() {
    let overloaded = || ProviderError::Http {
        status: 529,
        body: "overloaded".into(),
    };
    let llm = FakeLlm::new(vec![ScriptedTurn::fail(overloaded())]);
    let (out, events) = run(&llm, "m").await;
    assert_eq!(out, Err(overloaded()));
    assert!(events.is_empty());
    // A fault is a turn like any other: as the last turn it repeats.
    let (again, _) = run(&llm, "m").await;
    assert_eq!(again, Err(overloaded()));
    assert_eq!(llm.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn turns_play_in_order_and_the_last_repeats() {
    let llm = FakeLlm::new(vec![
        ScriptedTurn::text("one"),
        ScriptedTurn::text("two"),
        ScriptedTurn::text("three"),
    ]);
    let mut texts = Vec::new();
    for model in ["m1", "m2", "m3", "m4"] {
        let (out, _) = run(&llm, model).await;
        texts.push(out.expect("outcome").text);
    }
    assert_eq!(texts, vec!["one", "two", "three", "three"]);
    assert_eq!(llm.calls(), 4);
    assert_eq!(
        llm.requests(),
        vec![request("m1"), request("m2"), request("m3"), request("m4")]
    );
}

#[tokio::test]
async fn concurrent_calls_record_each_request_beside_the_turn_it_played() {
    const N: usize = 16;
    let turns = (0..N)
        .map(|i| ScriptedTurn::text(format!("t{i}")))
        .collect();
    let llm = Arc::new(FakeLlm::new(turns));
    // Start every call from its own thread at once; `stream` takes its turn
    // before returning the future.
    let barrier = std::sync::Barrier::new(N);
    let calls: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..N)
            .map(|k| {
                let (llm, barrier) = (llm.clone(), &barrier);
                scope.spawn(move || {
                    let sink = Arc::new(CollectEvents::default());
                    barrier.wait();
                    (
                        k,
                        llm.stream(request(&format!("m{k}")), Cancel::new(), sink),
                    )
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let requests = llm.requests();
    assert_eq!(requests.len(), N);
    assert_eq!(llm.calls(), N);
    for (k, call) in calls {
        let text = call.await.expect("outcome").text;
        let i: usize = text[1..].parse().unwrap();
        assert_eq!(
            requests[i].model,
            format!("m{k}"),
            "requests()[{i}] must be the request that played turn {i}"
        );
    }
}

#[test]
#[should_panic(expected = "ScriptedTurn::fail() streams nothing, so .usage() would be ignored")]
fn usage_on_a_fault_turn_is_refused() {
    let _ = ScriptedTurn::fail(ProviderError::Protocol("x".into())).usage(1, 1);
}

#[test]
#[should_panic(expected = "ScriptedTurn::hang() streams nothing, so .usage() would be ignored")]
fn usage_on_a_hang_turn_is_refused() {
    let _ = ScriptedTurn::hang().usage(1, 1);
}

#[test]
#[should_panic(expected = "ScriptedTurn::fail() streams nothing, so .tool_call() would be ignored")]
fn a_tool_call_on_a_fault_turn_is_refused() {
    let _ = ScriptedTurn::fail(ProviderError::Protocol("x".into())).tool_call("search", "{}");
}

#[test]
#[should_panic(expected = "ScriptedTurn::hang() streams nothing, so .tool_call() would be ignored")]
fn a_tool_call_on_a_hang_turn_is_refused() {
    let _ = ScriptedTurn::hang().tool_call("search", "{}");
}

#[tokio::test(start_paused = true)]
async fn a_delayed_turn_waits_then_answers() {
    let llm = FakeLlm::new(vec![
        ScriptedTurn::text("late").delay(Duration::from_secs(10)),
    ]);
    let started = tokio::time::Instant::now();
    let (out, _) = run(&llm, "m").await;
    assert_eq!(started.elapsed(), Duration::from_secs(10));
    assert_eq!(out.expect("outcome").text, "late");
}

// ---------------------------------------------------------------- clock

#[test]
fn the_clock_jumps_when_advanced() {
    let clock = FakeClock::at("2026-11-20T01:00:00Z");
    clock.advance(jiff::SignedDuration::from_hours(9));
    let expected: jiff::Timestamp = "2026-11-20T10:00:00Z".parse().unwrap();
    assert_eq!(clock.now(), expected);
}

// ---------------------------------------------------------------- secrets

#[test]
fn secrets_set_get_and_delete_under_one_account() {
    const CANARY: &str = "CANARY-am-testkit-7f3e";
    let store = FakeSecretStore::new();
    store
        .set("acct-test", SecretString::new(CANARY.to_string()))
        .unwrap();
    let got = store.get("acct-test").unwrap().expect("stored");
    assert_eq!(got.expose(), CANARY);
    store.delete("acct-test").unwrap();
    assert!(store.get("acct-test").unwrap().is_none());
}

// ---------------------------------------------------------------- recorded HTTP

#[tokio::test(start_paused = true)]
async fn a_recorded_fixture_is_served_byte_for_byte_and_the_request_recorded() {
    let egress = FakeEgress::new();
    egress.replay("/v1/messages", "anthropic/c1_count_stream.sse");
    let url = "https://api.anthropic.com/v1/messages";
    let resp = egress
        .send(HttpRequest::post_json(url, b"{\"probe\":1}".to_vec()))
        .await
        .expect("routed");
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.header("content-type").as_deref(),
        Some("text/event-stream")
    );
    let chunks = read_chunks(resp).await;
    assert!(chunks.len() > 1, "an .sse fixture arrives event by event");
    for c in &chunks {
        assert!(
            c.ends_with(b"\n\n") || c.ends_with(b"\r\n\r\n"),
            "chunk does not end at an event boundary: {:?}",
            String::from_utf8_lossy(c)
        );
    }
    assert_eq!(chunks.concat(), recorded("anthropic/c1_count_stream.sse"));
    let sent = egress.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].url, url);
    assert_eq!(sent[0].body, b"{\"probe\":1}".to_vec());
}

#[tokio::test(start_paused = true)]
async fn a_recorded_json_fixture_is_served_as_json() {
    let egress = FakeEgress::new();
    egress.replay("/v1/probe", "testkit/replay_probe.json");
    let resp = egress
        .send(HttpRequest::get("https://api.example.invalid/v1/probe"))
        .await
        .expect("routed");
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.header("content-type").as_deref(),
        Some("application/json")
    );
    let chunks = read_chunks(resp).await;
    assert_eq!(chunks.concat(), recorded("testkit/replay_probe.json"));
}

#[tokio::test(start_paused = true)]
async fn an_unrouted_url_is_refused_by_name() {
    let egress = FakeEgress::new();
    egress.replay("/v1/messages", "anthropic/c1_count_stream.sse");
    let url = "https://unrouted.example.invalid/v1/other";
    let err = match egress.send(HttpRequest::get(url)).await {
        Ok(_) => panic!("an unrouted URL must be refused"),
        Err(e) => e,
    };
    assert!(matches!(err, EgressError::Blocked(_)));
    assert!(err.to_string().contains(url), "{err}");
    assert_eq!(egress.requests().len(), 1);
}
