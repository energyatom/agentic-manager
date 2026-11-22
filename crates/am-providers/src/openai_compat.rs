//! OpenAI-chat-compatible adapter (`{base_url}/chat/completions`, streaming
//! SSE): OpenAI's cloud, LM Studio (`local-lmstudio`, 127.0.0.1:1234), Ollama
//! (`local-ollama`, 127.0.0.1:11434) and compatible endpoints.
//!
//! The stream (story 2.3):
//! - text is `delta.content` only; a reasoning model's thinking
//!   (`delta.reasoning_content`, `delta.reasoning`) is never read, so it
//!   reaches neither a text delta nor the answer (spike finding 8);
//! - tool calls are assembled by `index`, or by `id` when a server omits the
//!   index, and two calls are never merged into one (a function name on a
//!   call that already has one starts another call);
//! - usage is asked for with `stream_options.include_usage`;
//! - a cancel returns `StopReason::Cancelled` with no tool call: a call the
//!   server had not finished is not a call.
//!
//! File parts (AD-31): local servers read images only, and only with a vision
//! model loaded (LM Studio: JPEG, PNG, WebP; Ollama: base64 images). Neither
//! documents a file or PDF part, so every other type is refused.

use std::sync::{Arc, Mutex};

use am_domain::{
    AuthRef, BoxFuture, Caller, CancelSignal, ChatOutcome, ChatRequest, EgressClient, EventSink,
    FileKind, HttpRequest, LlmProvider, ModelCapabilities, Part, ProviderError, Role, StopReason,
    StreamEvent, ToolCall, UntrustedContent, Usage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::common::{drive_sse, file_label, render_untrusted_text, send_cancellable};

const IMAGE_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];

/// The host of an OpenAI Account's one endpoint (AD-48).
pub const OPENAI_HOST: &str = "api.openai.com";
/// The base URL of an OpenAI Account: its calls go to
/// `{OPENAI_BASE_URL}/chat/completions` and nowhere else.
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiCompatProvider {
    egress: Arc<dyn EgressClient>,
    account_id: String,
    /// e.g. `http://127.0.0.1:1234/v1`
    base_url: String,
    auth: Option<AuthRef>,
    vision: bool,
}

impl OpenAiCompatProvider {
    pub fn new(
        egress: Arc<dyn EgressClient>,
        account_id: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        OpenAiCompatProvider {
            egress,
            account_id: account_id.into(),
            base_url: base_url.into(),
            auth: None,
            vision: false,
        }
    }

    /// LM Studio's reserved local Account.
    pub fn lm_studio(egress: Arc<dyn EgressClient>) -> Self {
        Self::reserved(egress, am_domain::net::LOCAL_LMSTUDIO)
    }

    /// Ollama's reserved local Account.
    pub fn ollama(egress: Arc<dyn EgressClient>) -> Self {
        Self::reserved(egress, am_domain::net::LOCAL_OLLAMA)
    }

    /// A reserved local Account at the endpoint am-domain defines for it,
    /// under its OpenAI-compatible `/v1` path.
    fn reserved(egress: Arc<dyn EgressClient>, id: &str) -> Self {
        let base = am_domain::net::reserved_local_account(id)
            .map(|e| format!("{e}/v1"))
            .unwrap_or_default();
        Self::new(egress, id, base)
    }

    pub fn with_auth(mut self, auth: AuthRef) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Mark the loaded model as vision-capable (its `reads_images` flag).
    pub fn with_vision(mut self, vision: bool) -> Self {
        self.vision = vision;
        self
    }

    pub fn request_body(&self, req: &ChatRequest) -> Result<Value, ProviderError> {
        let mut messages = Vec::new();
        if !req.system.is_empty() {
            let text = req
                .system
                .iter()
                .map(|i| i.text())
                .collect::<Vec<_>>()
                .join("\n\n");
            messages.push(json!({"role": "system", "content": text}));
        }
        for m in &req.messages {
            match m.role {
                Role::User => {
                    let mut content = Vec::new();
                    let mut tool_msgs = Vec::new();
                    for p in &m.parts {
                        match p {
                            Part::Instruction(i) => {
                                content.push(json!({"type": "text", "text": i.text()}))
                            }
                            Part::Untrusted(d) => match d.content() {
                                UntrustedContent::Text(t) => content.push(
                                    json!({"type": "text", "text": render_untrusted_text(d.label(), t)}),
                                ),
                                UntrustedContent::File(f) => {
                                    content.push(json!({"type": "text", "text": file_label(d.label(), f)}));
                                    match f.kind() {
                                        FileKind::Image if self.vision && IMAGE_TYPES.contains(&f.media_type.as_str()) => {
                                            let b64 = base64::engine::general_purpose::STANDARD.encode(&f.bytes);
                                            content.push(json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", f.media_type, b64)}}));
                                        }
                                        FileKind::Text => {
                                            let text = String::from_utf8(f.bytes.clone()).map_err(|_| {
                                                ProviderError::unsupported(&f.name, &f.media_type, &req.model)
                                            })?;
                                            content.push(json!({"type": "text", "text": render_untrusted_text(d.label(), &text)}));
                                        }
                                        _ => {
                                            return Err(ProviderError::unsupported(&f.name, &f.media_type, &req.model));
                                        }
                                    }
                                }
                            },
                            Part::ToolResult { call_id, content: d } => {
                                let text = match d.content() {
                                    UntrustedContent::Text(t) => render_untrusted_text(d.label(), t),
                                    UntrustedContent::File(f) => {
                                        return Err(ProviderError::unsupported(&f.name, &f.media_type, &req.model));
                                    }
                                };
                                tool_msgs.push(json!({"role": "tool", "tool_call_id": call_id, "content": text}));
                            }
                            _ => {
                                return Err(ProviderError::Protocol(
                                    "a part was placed in the wrong role".into(),
                                ));
                            }
                        }
                    }
                    messages.extend(tool_msgs);
                    if !content.is_empty() {
                        messages.push(json!({"role": "user", "content": content}));
                    }
                }
                Role::Assistant => {
                    let mut text = String::new();
                    let mut calls = Vec::new();
                    for p in &m.parts {
                        match p {
                            Part::AssistantText(t) => text.push_str(t),
                            Part::ToolCall(c) => calls.push(json!({
                                "id": c.id, "type": "function",
                                "function": {"name": c.name, "arguments": c.arguments_json}
                            })),
                            _ => {
                                return Err(ProviderError::Protocol(
                                    "a part was placed in the wrong role".into(),
                                ));
                            }
                        }
                    }
                    let mut msg = json!({"role": "assistant", "content": text});
                    if !calls.is_empty() {
                        msg["tool_calls"] = json!(calls);
                    }
                    messages.push(msg);
                }
            }
        }
        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_output_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !req.tools.is_empty() {
            let tools: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    let schema: Value = serde_json::from_str(&t.input_schema_json)
                        .unwrap_or_else(|_| json!({"type": "object"}));
                    json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": schema}})
                })
                .collect();
            body["tools"] = json!(tools);
        }
        Ok(body)
    }
}

/// One tool call while its fragments arrive.
#[derive(Default)]
struct PendingCall {
    /// The `index` its fragments carry; `None` when the server sends none.
    index: Option<u64>,
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct StreamState {
    text: String,
    /// The tool calls, in the order each first appeared.
    calls: Vec<PendingCall>,
    usage: Option<Usage>,
    finish: Option<String>,
}

impl StreamState {
    /// The call a tool-call fragment belongs to, started when it is new.
    ///
    /// - With an `index`: the call of that index. A fragment that brings a
    ///   function name while that call already has one starts another
    ///   call, whatever the ids say (a server that numbers every call 0 and
    ///   sends each whole, with or without ids); a fragment without a name
    ///   continues the call (also when it brings another id: a server that
    ///   stamps a fresh id on every fragment).
    /// - Without an `index`: the call of the fragment's `id`. A fragment
    ///   with neither continues the call before it, unless it brings a
    ///   function name and that call already has one.
    ///
    /// Two calls are never merged: no fragment is filed under a default
    /// index, and no call is given a second name.
    fn call_for(&mut self, index: Option<u64>, id: Option<&str>, named: bool) -> &mut PendingCall {
        let another_name = |call: &PendingCall| named && !call.name.is_empty();
        let found = match (index, id) {
            (Some(index), _) => self
                .calls
                .iter()
                .rposition(|c| c.index == Some(index))
                .filter(|at| !another_name(&self.calls[*at])),
            (None, Some(id)) => self.calls.iter().rposition(|c| c.id == id),
            (None, None) => self
                .calls
                .len()
                .checked_sub(1)
                .filter(|at| !another_name(&self.calls[*at])),
        };
        let at = match found {
            Some(at) => at,
            None => {
                self.calls.push(PendingCall {
                    index,
                    ..PendingCall::default()
                });
                self.calls.len() - 1
            }
        };
        &mut self.calls[at]
    }
}

fn on_data(
    st: &mut StreamState,
    sink: &Arc<dyn EventSink>,
    data: &str,
) -> Result<bool, ProviderError> {
    if data.trim() == "[DONE]" {
        return Ok(true);
    }
    let v: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    if let Some(err) = v.get("error") {
        return Err(ProviderError::Protocol(
            err["message"]
                .as_str()
                .unwrap_or("provider error")
                .to_string(),
        ));
    }
    if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
        let usage = Usage {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        };
        st.usage = Some(usage);
        sink.on_event(StreamEvent::Usage(usage));
    }
    if let Some(choice) = v["choices"].get(0) {
        let delta = &choice["delta"];
        // Only `content` is the answer; `reasoning_content` is never read.
        if let Some(t) = delta["content"].as_str()
            && !t.is_empty()
        {
            st.text.push_str(t);
            sink.on_event(StreamEvent::TextDelta(t.to_string()));
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for c in calls {
                let id = c["id"].as_str().filter(|id| !id.is_empty());
                let name = c["function"]["name"].as_str().filter(|n| !n.is_empty());
                let call = st.call_for(c["index"].as_u64(), id, name.is_some());
                // A call keeps the first id it was given.
                if let Some(id) = id
                    && call.id.is_empty()
                {
                    call.id = id.to_string();
                }
                if let Some(name) = name {
                    call.name.push_str(name);
                }
                if let Some(args) = c["function"]["arguments"].as_str() {
                    call.arguments.push_str(args);
                }
            }
        }
        if let Some(f) = choice["finish_reason"].as_str() {
            st.finish = Some(f.to_string());
        }
    }
    Ok(false)
}

impl LlmProvider for OpenAiCompatProvider {
    fn account_id(&self) -> String {
        self.account_id.clone()
    }

    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities {
            reads_pdf: false,
            reads_images: self.vision,
            reads_office: false,
            reads_audio: false,
        }
    }

    fn check_file_parts(&self, request: &ChatRequest) -> Result<(), ProviderError> {
        let caps = self.capabilities(&request.model);
        for (name, media_type) in request.file_parts() {
            let kind = FileKind::of(&media_type);
            let ok = caps.reads(kind)
                && (kind != FileKind::Image || IMAGE_TYPES.contains(&media_type.as_str()));
            if !ok {
                return Err(ProviderError::unsupported(
                    &name,
                    &media_type,
                    &request.model,
                ));
            }
        }
        Ok(())
    }

    fn stream(
        &self,
        request: ChatRequest,
        cancel: Arc<dyn CancelSignal>,
        sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let checked = self
            .check_file_parts(&request)
            .and_then(|_| self.request_body(&request));
        let egress = self.egress.clone();
        let url = format!("{}/chat/completions", self.base_url);
        let auth = self.auth.clone();
        let account_id = self.account_id.clone();
        Box::pin(async move {
            let body = checked?;
            let req = HttpRequest {
                headers: vec![
                    ("content-type".into(), "application/json".into()),
                    ("accept".into(), "text/event-stream".into()),
                ],
                auth,
                caller: Some(Caller::Account {
                    account: account_id,
                    seat: None,
                }),
                ..HttpRequest::post_json(url, body.to_string().into_bytes())
            };
            let Some(resp) = send_cancellable(&egress, req, &cancel).await? else {
                return Ok(finish(StreamState::default(), &sink, true));
            };
            let state = Arc::new(Mutex::new(StreamState::default()));
            let st2 = state.clone();
            let sink2 = sink.clone();
            let cancelled = drive_sse(resp, &cancel, move |ev| {
                let mut st = st2
                    .lock()
                    .map_err(|_| ProviderError::Protocol("lock".into()))?;
                on_data(&mut st, &sink2, &ev.data)
            })
            .await?;
            let st = std::mem::take(
                &mut *state
                    .lock()
                    .map_err(|_| ProviderError::Protocol("lock".into()))?,
            );
            Ok(finish(st, &sink, cancelled))
        })
    }
}

fn finish(st: StreamState, sink: &Arc<dyn EventSink>, cancelled: bool) -> ChatOutcome {
    // A cancelled stream gives no tool call, to the sink or in the outcome:
    // the server had not finished them.
    let mut calls = if cancelled { Vec::new() } else { st.calls };
    // Calls that all carry an index are given in index order (the sort is
    // stable, so calls that share an index keep the order they came in).
    if calls.iter().all(|c| c.index.is_some()) {
        calls.sort_by_key(|c| c.index);
    }
    let tool_calls: Vec<ToolCall> = calls
        .into_iter()
        .map(|c| ToolCall {
            id: c.id,
            name: c.name,
            arguments_json: if c.arguments.trim().is_empty() {
                "{}".into()
            } else {
                c.arguments
            },
        })
        .collect();
    for c in &tool_calls {
        sink.on_event(StreamEvent::ToolCall(c.clone()));
    }
    let stop = if cancelled {
        StopReason::Cancelled
    } else {
        match st.finish.as_deref() {
            Some("stop") => StopReason::EndTurn,
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ if !tool_calls.is_empty() => StopReason::ToolUse,
            _ => StopReason::Other,
        }
    };
    ChatOutcome {
        text: st.text,
        tool_calls,
        usage: st.usage,
        stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSink;

    impl EventSink for NoSink {
        fn on_event(&self, _event: StreamEvent) {}
    }

    /// One tool-call fragment, as a stream chunk through `on_data`.
    fn fragment(
        st: &mut StreamState,
        index: Option<u64>,
        id: Option<&str>,
        name: Option<&str>,
        arguments: &str,
    ) {
        let mut call = json!({"function": {"arguments": arguments}});
        if let Some(index) = index {
            call["index"] = json!(index);
        }
        if let Some(id) = id {
            call["id"] = json!(id);
        }
        if let Some(name) = name {
            call["function"]["name"] = json!(name);
        }
        let chunk = json!({"choices": [{"index": 0, "delta": {"tool_calls": [call]}}]});
        let sink: Arc<dyn EventSink> = Arc::new(NoSink);
        let done = on_data(st, &sink, &chunk.to_string()).unwrap();
        assert!(!done);
    }

    fn calls(st: &StreamState) -> Vec<(&str, &str, &str)> {
        st.calls
            .iter()
            .map(|c| (c.id.as_str(), c.name.as_str(), c.arguments.as_str()))
            .collect()
    }

    /// Fragments by index: the id and the name come once, the rest follow
    /// the index.
    #[test]
    fn fragments_with_an_index_join_the_call_of_that_index() {
        let mut st = StreamState::default();
        fragment(&mut st, Some(0), Some("a"), Some("one"), "{\"x\":");
        fragment(&mut st, Some(1), Some("b"), Some("two"), "{\"y\":");
        fragment(&mut st, Some(0), None, None, "1}");
        fragment(&mut st, Some(1), Some(""), Some(""), "2}");
        assert_eq!(
            calls(&st),
            [("a", "one", "{\"x\":1}"), ("b", "two", "{\"y\":2}")]
        );
    }

    /// A server that numbers every call 0: another id with a function name
    /// is another call, and what follows without a name continues it.
    #[test]
    fn another_id_with_a_name_on_the_same_index_is_another_call() {
        let mut st = StreamState::default();
        fragment(&mut st, Some(0), Some("a"), Some("one"), "{}");
        fragment(&mut st, Some(0), Some("b"), Some("two"), "{\"y\":");
        fragment(&mut st, Some(0), None, None, "2}");
        assert_eq!(calls(&st), [("a", "one", "{}"), ("b", "two", "{\"y\":2}")]);
    }

    /// A server that stamps a fresh id on every fragment: another id with no
    /// function name continues the call at that index, which keeps its
    /// first id.
    #[test]
    fn another_id_without_a_name_continues_the_call_at_that_index() {
        let mut st = StreamState::default();
        fragment(&mut st, Some(0), Some("a"), Some("one"), "{\"x\":");
        fragment(&mut st, Some(0), Some("b"), None, "1");
        fragment(&mut st, Some(0), Some("c"), Some(""), "}");
        assert_eq!(calls(&st), [("a", "one", "{\"x\":1}")]);
    }

    /// Two whole calls at the same index with no ids, and two with neither
    /// an index nor an id: a second function name is another call.
    #[test]
    fn a_second_function_name_is_another_call_whatever_the_ids_say() {
        let mut st = StreamState::default();
        fragment(&mut st, Some(0), None, Some("one"), "{\"x\":1}");
        fragment(&mut st, Some(0), None, Some("two"), "{\"y\":");
        fragment(&mut st, Some(0), None, None, "2}");
        assert_eq!(
            calls(&st),
            [("", "one", "{\"x\":1}"), ("", "two", "{\"y\":2}")]
        );

        let mut st = StreamState::default();
        fragment(&mut st, None, None, Some("one"), "{\"x\":");
        fragment(&mut st, None, None, None, "1}");
        fragment(&mut st, None, None, Some("two"), "{}");
        assert_eq!(calls(&st), [("", "one", "{\"x\":1}"), ("", "two", "{}")]);

        // The same id with the name again: another call at that index.
        let mut st = StreamState::default();
        fragment(&mut st, Some(0), Some("a"), Some("one"), "{}");
        fragment(&mut st, Some(0), Some("a"), Some("two"), "{}");
        assert_eq!(calls(&st), [("a", "one", "{}"), ("a", "two", "{}")]);

        // A name that arrives after the id fills the call in.
        let mut st = StreamState::default();
        fragment(&mut st, Some(0), Some("a"), None, "");
        fragment(&mut st, Some(0), Some("b"), Some("one"), "{}");
        assert_eq!(calls(&st), [("a", "one", "{}")]);
    }

    /// No index: calls are told apart by id, and a fragment with neither
    /// continues the call before it.
    #[test]
    fn fragments_without_an_index_follow_their_id() {
        let mut st = StreamState::default();
        fragment(&mut st, None, Some("a"), Some("one"), "{\"x\":");
        fragment(&mut st, None, None, None, "1}");
        fragment(&mut st, None, Some("b"), Some("two"), "{\"y\":");
        fragment(&mut st, None, Some("b"), None, "2}");
        assert_eq!(
            calls(&st),
            [("a", "one", "{\"x\":1}"), ("b", "two", "{\"y\":2}")]
        );
    }
}
