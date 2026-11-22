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

/// Who a request is for (AD-12, AD-48): exactly one kind. The request
/// carries ids only; the egress client asks one [`EgressDirectory`] what
/// that caller may reach, so no caller can hand the client its own
/// allow-list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// A tool, reader or fetch by a seat: only the seat's `allowed_hosts`.
    Seat { seat: String },
    /// A call to an Account's endpoint, with the seat it serves when there
    /// is one (for the audit).
    Account {
        account: String,
        seat: Option<String>,
    },
    /// A call to an MCP server's approved URL.
    McpServer { server: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub auth: Option<AuthRef>,
    /// Who the request is for. A request with none is refused.
    pub caller: Option<Caller>,
}

impl HttpRequest {
    pub fn post_json(url: impl Into<String>, body: Vec<u8>) -> Self {
        HttpRequest {
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body,
            auth: None,
            caller: None,
        }
    }
    pub fn get(url: impl Into<String>) -> Self {
        HttpRequest {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            auth: None,
            caller: None,
        }
    }
}

/// Where an Account's or an MCP server's calls go: a scheme, a host and a
/// port, compared exactly (AD-12, AD-48). The host is lowercase: a DNS name
/// or, for an endpoint on this Mac, `127.0.0.1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
    }
}

/// The reserved local Accounts (AD-31) and where they are: LM Studio and
/// Ollama on this Mac. Defined here once.
pub const LOCAL_LMSTUDIO: &str = "local-lmstudio";
pub const LOCAL_OLLAMA: &str = "local-ollama";
/// Both reserved local Account ids.
pub const RESERVED_LOCAL_ACCOUNTS: [&str; 2] = [LOCAL_LMSTUDIO, LOCAL_OLLAMA];
const LOCAL_LMSTUDIO_PORT: u16 = 1234;
const LOCAL_OLLAMA_PORT: u16 = 11434;

/// The endpoint of a reserved local Account, `None` for any other id.
pub fn reserved_local_account(account: &str) -> Option<Endpoint> {
    let port = match account {
        LOCAL_LMSTUDIO => LOCAL_LMSTUDIO_PORT,
        LOCAL_OLLAMA => LOCAL_OLLAMA_PORT,
        _ => return None,
    };
    Some(Endpoint {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
    })
}

/// An Account's confirmation as a directory states it (AD-15 *Key use*):
/// the endpoint it is confirmed for and a version naming the Accounts
/// file's hash and the latest confirmation record. Never a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStamp {
    pub endpoint: Endpoint,
    pub version: String,
}

/// The one place the egress client learns what a caller may reach (AD-12,
/// AD-48), answered from the pinned config snapshot. `None` means the id is
/// unknown (or, for an Account, not confirmed), and the request is refused.
pub trait EgressDirectory: Send + Sync {
    /// The host patterns a seat may reach (empty: nothing).
    fn seat_hosts(&self, seat: &str) -> Option<Vec<crate::HostPattern>>;
    /// An Account's pinned endpoint.
    fn account_endpoint(&self, account: &str) -> Option<Endpoint>;
    /// An MCP server's approved URL, as its endpoint.
    fn mcp_endpoint(&self, server: &str) -> Option<Endpoint>;
    /// Why an Account the directory does not answer for is refused, in
    /// words, when it knows (for example: not confirmed). Never a secret.
    fn account_refusal(&self, _account: &str) -> Option<String> {
        None
    }
    /// Where the Account's confirmation stands now (AD-15 *Key use*): the
    /// endpoint it is confirmed for and a version that changes whenever its
    /// confirmation record or the Accounts file changes; `None` when it is
    /// not answered. The client takes it before resolving and again right
    /// after its own key read, and sends the key only when both are present,
    /// equal and name the endpoint the request goes to. A directory that
    /// keeps no confirmations answers with its endpoint and no version.
    fn account_stamp(&self, account: &str) -> Option<AccountStamp> {
        self.account_endpoint(account).map(|endpoint| AccountStamp {
            endpoint,
            version: String::new(),
        })
    }
    /// The ports the app's own servers are listening on (every running
    /// `agentic serve` records its port): refused to every Account and MCP
    /// server (AD-12).
    fn own_ports(&self) -> Vec<u16> {
        Vec::new()
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
