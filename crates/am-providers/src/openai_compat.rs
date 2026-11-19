//! OpenAI-chat-compatible adapter (`/v1/chat/completions`, streaming SSE):
//! LM Studio (`local-lmstudio`, 127.0.0.1:1234), Ollama (`local-ollama`,
//! 127.0.0.1:11434) and compatible endpoints.
//!
//! File parts (AD-31): local servers read images only, and only with a vision
//! model loaded (LM Studio: JPEG, PNG, WebP; Ollama: base64 images). Neither
//! documents a file or PDF part, so every other type is refused.

use std::collections::BTreeMap;
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
        Self::new(egress, "local-lmstudio", "http://127.0.0.1:1234/v1")
    }

    /// Ollama's reserved local Account.
    pub fn ollama(egress: Arc<dyn EgressClient>) -> Self {
        Self::new(egress, "local-ollama", "http://127.0.0.1:11434/v1")
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

#[derive(Default)]
struct StreamState {
    text: String,
    /// index -> (id, name, arguments)
    calls: BTreeMap<u64, (String, String, String)>,
    usage: Option<Usage>,
    finish: Option<String>,
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
        if let Some(t) = delta["content"].as_str()
            && !t.is_empty()
        {
            st.text.push_str(t);
            sink.on_event(StreamEvent::TextDelta(t.to_string()));
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for c in calls {
                let idx = c["index"].as_u64().unwrap_or(0);
                let entry = st.calls.entry(idx).or_default();
                if let Some(id) = c["id"].as_str() {
                    entry.0 = id.to_string();
                }
                if let Some(name) = c["function"]["name"].as_str() {
                    entry.1.push_str(name);
                }
                if let Some(args) = c["function"]["arguments"].as_str() {
                    entry.2.push_str(args);
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
                caller: Caller {
                    seat: None,
                    account: Some(account_id),
                },
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
    let tool_calls: Vec<ToolCall> = st
        .calls
        .into_values()
        .map(|(id, name, args)| ToolCall {
            id,
            name,
            arguments_json: if args.trim().is_empty() {
                "{}".into()
            } else {
                args
            },
        })
        .collect();
    if !cancelled {
        for c in &tool_calls {
            sink.on_event(StreamEvent::ToolCall(c.clone()));
        }
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
