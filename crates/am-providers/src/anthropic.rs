//! Anthropic Messages adapter (streaming SSE, tool use, PDF and image file parts).

use std::sync::{Arc, Mutex};

use am_domain::{
    AuthRef, AuthScheme, BoxFuture, Caller, CancelSignal, ChatOutcome, ChatRequest, EgressClient,
    EventSink, FileKind, HttpRequest, LlmProvider, ModelCapabilities, Part, ProviderError, Role,
    StopReason, StreamEvent, ToolCall, UntrustedContent, UntrustedData, Usage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::common::{
    drive_sse, file_label, render_engine_result, render_trusted_input, render_untrusted_text,
    send_cancellable,
};

pub const ANTHROPIC_HOST: &str = "api.anthropic.com";
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Image types the Messages API reads (AD-31).
const IMAGE_TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

pub struct AnthropicProvider {
    egress: Arc<dyn EgressClient>,
    account_id: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(egress: Arc<dyn EgressClient>, account_id: impl Into<String>) -> Self {
        AnthropicProvider {
            egress,
            account_id: account_id.into(),
            base_url: format!("https://{ANTHROPIC_HOST}"),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// The JSON body for a request (public for fixture tests).
    pub fn request_body(&self, req: &ChatRequest) -> Result<Value, ProviderError> {
        let mut messages = Vec::new();
        for m in &req.messages {
            let mut content = Vec::new();
            for p in &m.parts {
                match (m.role, p) {
                    (Role::User, Part::Instruction(i)) => {
                        content.push(json!({"type": "text", "text": i.text()}))
                    }
                    (Role::User, Part::Trusted(t)) => content.push(
                        json!({"type": "text", "text": render_trusted_input(t.label(), t.text())}),
                    ),
                    (Role::User, Part::Untrusted(d)) => {
                        content.extend(untrusted_blocks(d, &req.model)?)
                    }
                    (
                        Role::User,
                        Part::ToolResult {
                            call_id,
                            content: d,
                        },
                    ) => {
                        let text = match d.content() {
                            UntrustedContent::Text(t) => render_untrusted_text(d.label(), t),
                            UntrustedContent::File(f) => {
                                return Err(ProviderError::unsupported(
                                    &f.name,
                                    &f.media_type,
                                    &req.model,
                                ));
                            }
                        };
                        content.push(
                            json!({"type": "tool_result", "tool_use_id": call_id, "content": text}),
                        )
                    }
                    (Role::User, Part::EngineResult { call_id, result }) => content.push(
                        json!({"type": "tool_result", "tool_use_id": call_id, "content": render_engine_result(result)}),
                    ),
                    (Role::Assistant, Part::AssistantText(t)) => {
                        content.push(json!({"type": "text", "text": t}))
                    }
                    (Role::Assistant, Part::ToolCall(c)) => {
                        let input: Value =
                            serde_json::from_str(&c.arguments_json).unwrap_or_else(|_| json!({}));
                        content.push(
                            json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input}),
                        )
                    }
                    // Every variant by name (AD-10): a new one must be placed.
                    (Role::User, Part::AssistantText(_) | Part::ToolCall(_))
                    | (
                        Role::Assistant,
                        Part::Instruction(_)
                        | Part::Trusted(_)
                        | Part::Untrusted(_)
                        | Part::ToolResult { .. }
                        | Part::EngineResult { .. },
                    ) => {
                        return Err(ProviderError::Protocol(
                            "a part was placed in the wrong role".into(),
                        ));
                    }
                }
            }
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            messages.push(json!({"role": role, "content": content}));
        }
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_output_tokens,
            "stream": true,
            "messages": messages,
        });
        if !req.system.is_empty() {
            let text = req
                .system
                .iter()
                .map(|i| i.text())
                .collect::<Vec<_>>()
                .join("\n\n");
            body["system"] = json!(text);
        }
        if !req.tools.is_empty() {
            let tools: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    let schema: Value = serde_json::from_str(&t.input_schema_json)
                        .unwrap_or_else(|_| json!({"type": "object"}));
                    json!({"name": t.name, "description": t.description, "input_schema": schema})
                })
                .collect();
            body["tools"] = json!(tools);
            body["tool_choice"] = json!({"type": "auto"});
        }
        Ok(body)
    }
}

fn untrusted_blocks(d: &UntrustedData, model: &str) -> Result<Vec<Value>, ProviderError> {
    match d.content() {
        UntrustedContent::Text(t) => Ok(vec![
            json!({"type": "text", "text": render_untrusted_text(d.label(), t)}),
        ]),
        UntrustedContent::File(f) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&f.bytes);
            let label = json!({"type": "text", "text": file_label(d.label(), f)});
            match f.kind() {
                FileKind::Pdf => Ok(vec![
                    label,
                    json!({"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": b64}}),
                ]),
                FileKind::Image if IMAGE_TYPES.contains(&f.media_type.as_str()) => Ok(vec![
                    label,
                    json!({"type": "image", "source": {"type": "base64", "media_type": f.media_type, "data": b64}}),
                ]),
                FileKind::Text => {
                    let text = String::from_utf8(f.bytes.clone())
                        .map_err(|_| ProviderError::unsupported(&f.name, &f.media_type, model))?;
                    Ok(vec![
                        label,
                        json!({"type": "text", "text": render_untrusted_text(d.label(), &text)}),
                    ])
                }
                _ => Err(ProviderError::unsupported(&f.name, &f.media_type, model)),
            }
        }
    }
}

#[derive(Default)]
struct StreamState {
    text: String,
    tool_calls: Vec<ToolCall>,
    /// (block index, id, name, json so far)
    open_tool: Option<(u64, String, String, String)>,
    usage: Option<Usage>,
    /// True once `message_delta` reported the final output tokens.
    final_usage: bool,
    stop: Option<StopReason>,
}

fn on_event(
    st: &mut StreamState,
    sink: &Arc<dyn EventSink>,
    event: Option<String>,
    data: &str,
) -> Result<bool, ProviderError> {
    let v: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    let kind = event
        .or_else(|| v["type"].as_str().map(str::to_string))
        .unwrap_or_default();
    match kind.as_str() {
        "message_start" => {
            let u = &v["message"]["usage"];
            let usage = Usage {
                input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
            };
            st.usage = Some(usage);
            sink.on_event(StreamEvent::Usage(usage));
        }
        "content_block_start" => {
            let block = &v["content_block"];
            if block["type"] == "tool_use" {
                st.open_tool = Some((
                    v["index"].as_u64().unwrap_or(0),
                    block["id"].as_str().unwrap_or("").to_string(),
                    block["name"].as_str().unwrap_or("").to_string(),
                    String::new(),
                ));
            } else if block["type"] == "text"
                && let Some(t) = block["text"].as_str()
                && !t.is_empty()
            {
                st.text.push_str(t);
                sink.on_event(StreamEvent::TextDelta(t.to_string()));
            }
        }
        "content_block_delta" => {
            let delta = &v["delta"];
            match delta["type"].as_str().unwrap_or("") {
                "text_delta" => {
                    let t = delta["text"].as_str().unwrap_or("");
                    st.text.push_str(t);
                    sink.on_event(StreamEvent::TextDelta(t.to_string()));
                }
                "input_json_delta" => {
                    if let Some((_, _, _, json)) = st.open_tool.as_mut() {
                        json.push_str(delta["partial_json"].as_str().unwrap_or(""));
                    }
                }
                // thinking_delta, signature_delta and future kinds are ignored.
                _ => {}
            }
        }
        "content_block_stop" => {
            let idx = v["index"].as_u64().unwrap_or(0);
            if let Some((open_idx, id, name, json)) = st.open_tool.take() {
                if open_idx == idx {
                    let arguments_json = if json.trim().is_empty() {
                        "{}".to_string()
                    } else {
                        json
                    };
                    let call = ToolCall {
                        id,
                        name,
                        arguments_json,
                    };
                    sink.on_event(StreamEvent::ToolCall(call.clone()));
                    st.tool_calls.push(call);
                } else {
                    st.open_tool = Some((open_idx, id, name, json));
                }
            }
        }
        "message_delta" => {
            if let Some(out) = v["usage"]["output_tokens"].as_u64() {
                let mut u = st.usage.unwrap_or_default();
                u.output_tokens = out;
                if let Some(inp) = v["usage"]["input_tokens"].as_u64() {
                    u.input_tokens = inp;
                }
                st.usage = Some(u);
                st.final_usage = true;
                sink.on_event(StreamEvent::Usage(u));
            }
            if let Some(reason) = v["delta"]["stop_reason"].as_str() {
                st.stop = Some(match reason {
                    "end_turn" | "stop_sequence" => StopReason::EndTurn,
                    "tool_use" => StopReason::ToolUse,
                    "max_tokens" => StopReason::MaxTokens,
                    _ => StopReason::Other,
                });
            }
        }
        "message_stop" => return Ok(true),
        "error" => {
            return Err(ProviderError::Protocol(
                v["error"]["message"]
                    .as_str()
                    .unwrap_or("provider error")
                    .to_string(),
            ));
        }
        _ => {}
    }
    Ok(false)
}

impl LlmProvider for AnthropicProvider {
    fn account_id(&self) -> String {
        self.account_id.clone()
    }

    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        // Shipped defaults (AD-31): PDF and images natively; no office files, no audio.
        ModelCapabilities {
            reads_pdf: true,
            reads_images: true,
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
        let url = format!("{}/v1/messages", self.base_url);
        let account_id = self.account_id.clone();
        Box::pin(async move {
            let body = checked?;
            let req = HttpRequest {
                headers: vec![
                    ("content-type".into(), "application/json".into()),
                    ("accept".into(), "text/event-stream".into()),
                    ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
                ],
                auth: Some(AuthRef {
                    scheme: AuthScheme::XApiKey,
                    account_id: account_id.clone(),
                }),
                caller: Some(Caller::Account {
                    account: account_id,
                    seat: None,
                }),
                ..HttpRequest::post_json(url, body.to_string().into_bytes())
            };
            let Some(resp) = send_cancellable(&egress, req, &cancel).await? else {
                return Ok(cancelled_outcome(StreamState::default()));
            };
            let state = Arc::new(Mutex::new(StreamState::default()));
            let st2 = state.clone();
            let cancelled = drive_sse(resp, &cancel, move |ev| {
                let mut st = st2
                    .lock()
                    .map_err(|_| ProviderError::Protocol("lock".into()))?;
                on_event(&mut st, &sink, ev.event, &ev.data)
            })
            .await?;
            let st = std::mem::take(
                &mut *state
                    .lock()
                    .map_err(|_| ProviderError::Protocol("lock".into()))?,
            );
            if cancelled {
                return Ok(cancelled_outcome(st));
            }
            let stop = st.stop.unwrap_or(if st.tool_calls.is_empty() {
                StopReason::Other
            } else {
                StopReason::ToolUse
            });
            Ok(ChatOutcome {
                text: st.text,
                tool_calls: st.tool_calls,
                usage: st.usage,
                stop,
            })
        })
    }
}

/// A cancelled stream that never reported its final usage reports usage as
/// missing, so the call settles at its reservation (AD-9, AD-20); the partial
/// usage still reached the sink.
fn cancelled_outcome(st: StreamState) -> ChatOutcome {
    ChatOutcome {
        text: st.text,
        tool_calls: st.tool_calls,
        usage: if st.final_usage { st.usage } else { None },
        stop: StopReason::Cancelled,
    }
}
