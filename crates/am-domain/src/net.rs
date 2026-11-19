//! The `EgressClient` port (AD-12). It is the only way any adapter reaches the
//! network. Secrets are referenced, never carried: the client resolves an
//! [`AuthRef`] through the `SecretStore` while building the request.

use crate::BoxFuture;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    /// `x-api-key: <secret>` (Anthropic).
    XApiKey,
    /// `Authorization: Bearer <secret>`.
    Bearer,
}

/// A reference to a key in the keychain; the value never leaves `am-net`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRef {
    pub scheme: AuthScheme,
    pub account_id: String,
}

/// Who is asking (AD-12): every request carries its seat and Account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caller {
    pub seat: Option<String>,
    pub account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub auth: Option<AuthRef>,
    pub caller: Caller,
}

impl HttpRequest {
    pub fn post_json(url: impl Into<String>, body: Vec<u8>) -> Self {
        HttpRequest {
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body,
            auth: None,
            caller: Caller::default(),
        }
    }
    pub fn get(url: impl Into<String>) -> Self {
        HttpRequest {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            auth: None,
            caller: Caller::default(),
        }
    }
}

/// A response body read chunk by chunk. Dropping it aborts the transfer.
pub trait BodyChunks: Send + Sync {
    /// The next chunk; `None` at the end of the body.
    fn next(&self) -> BoxFuture<Option<Result<Vec<u8>, EgressError>>>;
}

pub struct HttpResponse {
    pub status: u16,
    /// Header names are lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn BodyChunks>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<String> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum EgressError {
    #[error("request blocked: {0}")]
    Blocked(String),
    #[error("no key in the keychain for Account {0}")]
    MissingSecret(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("response body exceeded {0} bytes")]
    TooLarge(u64),
}

pub trait EgressClient: Send + Sync {
    fn send(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, EgressError>>;
}
