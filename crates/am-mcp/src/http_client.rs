//! rmcp's `StreamableHttpClient` implemented on our `EgressClient` (AD-12).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use am_domain::{BodyChunks, EgressClient, EgressError, HttpMethod, HttpRequest, HttpResponse};
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use http::{HeaderName, HeaderValue};
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::streamable_http_client::{
    SseError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::{Sse, SseStream};

const SESSION_HEADER: &str = "mcp-session-id";
const LAST_EVENT_HEADER: &str = "last-event-id";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";
const MAX_JSON: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct EgressHttpError(#[from] EgressError);

type HttpErr = StreamableHttpError<EgressHttpError>;

#[derive(Clone)]
pub struct EgressHttpClient {
    egress: Arc<dyn EgressClient>,
}

impl EgressHttpClient {
    pub fn new(egress: Arc<dyn EgressClient>) -> Self {
        EgressHttpClient { egress }
    }

    async fn send(&self, req: HttpRequest) -> Result<HttpResponse, HttpErr> {
        self.egress
            .send(req)
            .await
            .map_err(|e| StreamableHttpError::Client(EgressHttpError(e)))
    }
}

fn base_request(
    method: HttpMethod,
    uri: &str,
    session_id: Option<&str>,
    auth_header: Option<String>,
    custom: HashMap<HeaderName, HeaderValue>,
) -> Result<HttpRequest, HttpErr> {
    if auth_header.is_some() {
        // Keys never travel as plain strings outside am-net (AD-17).
        return Err(StreamableHttpError::UnexpectedServerResponse(Cow::from(
            "MCP auth headers are not supported in the spike",
        )));
    }
    let mut headers = vec![("accept".to_string(), format!("{SSE}, {JSON}"))];
    if let Some(s) = session_id {
        headers.push((SESSION_HEADER.into(), s.to_string()));
    }
    for (k, v) in custom {
        headers.push((
            k.as_str().to_string(),
            String::from_utf8_lossy(v.as_bytes()).to_string(),
        ));
    }
    Ok(HttpRequest {
        method,
        url: uri.to_string(),
        headers,
        body: Vec::new(),
        auth: None,
        caller: Default::default(),
    })
}

fn body_stream(body: Box<dyn BodyChunks>) -> BoxStream<'static, Result<Bytes, EgressHttpError>> {
    futures_util::stream::unfold(body, |b| async move {
        let next = b.next().await?;
        Some((next.map(Bytes::from).map_err(EgressHttpError), b))
    })
    .boxed()
}

fn sse_stream(body: Box<dyn BodyChunks>) -> BoxStream<'static, Result<Sse, SseError>> {
    SseStream::from_bytes_stream(body_stream(body)).boxed()
}

async fn read_body(body: &dyn BodyChunks) -> Result<Vec<u8>, HttpErr> {
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| StreamableHttpError::Client(EgressHttpError(e)))?;
        out.extend_from_slice(&chunk);
        if out.len() > MAX_JSON {
            return Err(StreamableHttpError::ResponseBodyTooLarge { max_size: MAX_JSON });
        }
    }
    Ok(out)
}

fn is(content_type: Option<&str>, essence: &str) -> bool {
    content_type
        .map(|c| {
            c.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case(essence)
        })
        .unwrap_or(false)
}

impl StreamableHttpClient for EgressHttpClient {
    type Error = EgressHttpError;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, HttpErr> {
        let mut req = base_request(
            HttpMethod::Post,
            &uri,
            session_id.as_deref(),
            auth_header,
            custom_headers,
        )?;
        req.headers.push(("content-type".into(), JSON.into()));
        req.body = serde_json::to_vec(&message)?;
        let awaits_no_response = matches!(
            message,
            ClientJsonRpcMessage::Notification(_)
                | ClientJsonRpcMessage::Response(_)
                | ClientJsonRpcMessage::Error(_)
        );
        let resp = self.send(req).await?;
        let status = resp.status;
        if status == 202 || status == 204 {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == 404 && session_id.is_some() {
            return Err(StreamableHttpError::SessionExpired);
        }
        let content_type = resp.header("content-type");
        let new_session = resp.header(SESSION_HEADER);
        if !(200..300).contains(&status) {
            let body = read_body(resp.body.as_ref()).await.unwrap_or_default();
            if let Ok(msg) = serde_json::from_slice::<ServerJsonRpcMessage>(&body) {
                return Ok(StreamableHttpPostResponse::Json(msg, new_session));
            }
            let mut text = String::from_utf8_lossy(&body).to_string();
            text.truncate(500);
            return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                format!("HTTP {status}: {text}"),
            )));
        }
        if is(content_type.as_deref(), SSE) {
            return Ok(StreamableHttpPostResponse::Sse(
                sse_stream(resp.body),
                new_session,
            ));
        }
        if is(content_type.as_deref(), JSON) {
            let body = read_body(resp.body.as_ref()).await?;
            return match serde_json::from_slice::<ServerJsonRpcMessage>(&body) {
                Ok(msg) => Ok(StreamableHttpPostResponse::Json(msg, new_session)),
                Err(_) if awaits_no_response => Ok(StreamableHttpPostResponse::Accepted),
                Err(e) => Err(StreamableHttpError::Deserialize(e)),
            };
        }
        if awaits_no_response {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        Err(StreamableHttpError::UnexpectedContentType(content_type))
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), HttpErr> {
        let req = base_request(
            HttpMethod::Delete,
            &uri,
            Some(&session_id),
            auth_header,
            custom_headers,
        )?;
        let resp = self.send(req).await?;
        if resp.status == 405 || (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
            format!("HTTP {} on delete", resp.status),
        )))
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, HttpErr> {
        let mut req = base_request(
            HttpMethod::Get,
            &uri,
            session_id.as_deref(),
            auth_header,
            custom_headers,
        )?;
        if let Some(id) = last_event_id {
            req.headers.push((LAST_EVENT_HEADER.into(), id));
        }
        let resp = self.send(req).await?;
        if resp.status == 405 {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        if !(200..300).contains(&resp.status) {
            return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                format!("HTTP {} on GET", resp.status),
            )));
        }
        let ct = resp.header("content-type");
        if !is(ct.as_deref(), SSE) && !is(ct.as_deref(), JSON) {
            return Err(StreamableHttpError::UnexpectedContentType(ct));
        }
        Ok(sse_stream(resp.body))
    }
}
