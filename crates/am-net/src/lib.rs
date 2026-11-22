//! `am-net`: the only HTTP client in the app (AD-12, AD-48).
//!
//! [`NetEgress`] implements the `EgressClient` port. Every request names
//! who it is for ([`Caller`]): a seat, an Account or an MCP server. The
//! client asks one injected [`EgressDirectory`] what that caller may reach
//! and decides from that alone:
//!
//! - a seat: a URL one of its `HostPattern`s admits (`https` on 443, `http`
//!   on 80), never an IP literal, loopback or a private address;
//! - an Account: exactly its pinned endpoint (scheme, host and port); an IP
//!   literal only as `127.0.0.1`, and never the app's own serving port;
//! - an MCP server: exactly its approved URL's endpoint, as an Account.
//!
//! A reserved local Account's port is its own Account's alone, and the
//! app's own port (handed over by `serve`, or recorded in the project's
//! runtime folder and read by the directory) no Account or MCP server's.
//! A key goes only with its own Account: a request whose `AuthRef` names
//! another id than its Account caller (or any key on a seat, MCP or
//! reserved local request) is refused before any Keychain read.
//!
//! Then every DNS answer is checked (private, loopback, link-local, CGNAT,
//! metadata, multicast, unspecified, and their NAT64, 6to4,
//! IPv4-translated and Teredo forms are refused) and the connection is
//! pinned to the checked address; at most 3 redirects are followed, GET
//! only (the port has no HEAD), each re-checked by the same caller's rule,
//! re-resolved and pinned again; a credential, credential-like or session
//! header is dropped for the rest of the chain once a redirect changes
//! scheme, host or port; bodies are capped; a hop whose head does not
//! arrive, or whose body goes idle, past its caller kind's time limit
//! ([`Limits`]: a seat both, an Account idle only, an MCP server neither)
//! is given up; keys
//! are read from the `SecretStore` here and nowhere else, and stay a
//! `SecretString` until the transport builds a sensitive header.
//! Every refusal and every cut is audited as `egress.blocked`, actor
//! `System(am-net)`, with the destination's scheme, host and port, the
//! reason and the caller's id in `args`, never a path, query, userinfo,
//! header or body.
//!
//! The decisions are pure functions ([`check_caller`], [`check_address`],
//! [`url_target`]); the IO loop around them takes a [`Resolver`] and a
//! [`Transport`]. [`NetEgress::new`] always uses the system resolver and
//! reqwest; [`NetEgress::with_seams`] takes others, for tests only (`cargo
//! xtask seam-guard` keeps it out of shipped code).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::time::Duration;

use am_domain::{
    Actor, AuditEvent, AuditSink, AuthRef, AuthScheme, BodyChunks, BoxFuture, Caller, EgressClient,
    EgressDirectory, EgressError, Endpoint, HttpMethod, HttpRequest, HttpResponse, SecretStore,
    SecretString, reserved_local_account,
};
use tokio::sync::{Mutex, mpsc};

/// The most redirects one request follows (AD-12); the next is refused.
pub const MAX_REDIRECTS: usize = 3;

/// Headers that carry a credential: only the client sets one (a caller
/// that does is refused), and none crosses an origin change.
const CREDENTIAL_HEADERS: [&str; 8] = [
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "api-key",
    "x-goog-api-key",
    "x-auth-token",
    "cookie",
    "cookie2",
];

/// Headers that tie a request to a session: allowed from a caller (an MCP
/// client's `mcp-session-id`) but dropped once a redirect changes origin.
const SESSION_HEADERS: [&str; 1] = ["mcp-session-id"];

/// The words of a keyed call whose Account moved around its key read.
fn key_moved_words(account: &str) -> String {
    format!(
        "the Account {account} changed on this Mac while its key was read, so the key was not sent"
    )
}

/// The start of a refusal of an Account its directory does not answer
/// (an internal marker, stripped before anything is shown or audited).
const ACCOUNT_NOT_ANSWERED: &str = "\u{0}account-not-answered:";

/// The fixed audit reason of an Account its directory does not answer.
const ACCOUNT_REFUSED_REASON: &str = "the Account is not answered by this Mac's Accounts (not confirmed, changed or not on this Mac)";

/// The words of a refusal for a key that is not the caller's own.
pub const NOT_THE_CALLERS_CREDENTIAL: &str = "the credential does not belong to this caller";

/// The time limits of one caller kind; `None` is no limit from am-net.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// A hop whose response head has not arrived within this is given up.
    pub head: Option<Duration>,
    /// A body that sends nothing for this long, once its head arrived, is
    /// given up.
    pub idle: Option<Duration>,
}

/// Body size caps and, per caller kind, time limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// A response body past this is cut with `EgressError::TooLarge`.
    pub max_response_bytes: u64,
    /// A request body past this is refused before anything is sent.
    pub max_request_bytes: u64,
    /// A seat's fetch: a web page answers promptly or not at all.
    pub seat: Timeouts,
    /// An Account's call: no head limit (a local model loading, a long
    /// prompt read or a large upload comes before the first byte; the
    /// engine's step timeout and cancel cover it, AD-19), an idle limit
    /// once the reply streams.
    pub account: Timeouts,
    /// An MCP request: neither (a long tool call, and a server-to-client
    /// stream quiet by design; its session and the engine's cancel cover
    /// it).
    pub mcp: Timeouts,
}

impl Limits {
    /// The time limits of `caller`'s kind.
    pub fn timeouts(&self, caller: &Caller) -> Timeouts {
        match caller {
            Caller::Seat { .. } => self.seat,
            Caller::Account { .. } => self.account,
            Caller::McpServer { .. } => self.mcp,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_response_bytes: 64 * 1024 * 1024,
            max_request_bytes: 32 * 1024 * 1024,
            seat: Timeouts {
                head: Some(Duration::from_secs(30)),
                idle: Some(Duration::from_secs(120)),
            },
            account: Timeouts {
                head: None,
                idle: Some(Duration::from_secs(300)),
            },
            mcp: Timeouts {
                head: None,
                idle: None,
            },
        }
    }
}

/// The credential of one hop: a key from the Keychain, kept as a
/// [`SecretString`] until the transport builds its header (and marks that
/// header sensitive). It never sits in [`Hop::headers`].
#[derive(Clone)]
pub struct Credential {
    scheme: AuthScheme,
    secret: Arc<SecretString>,
}

impl Credential {
    /// The header the credential goes in (lowercase).
    pub fn header_name(&self) -> &'static str {
        match self.scheme {
            AuthScheme::XApiKey => "x-api-key",
            AuthScheme::Bearer => "authorization",
        }
    }

    /// The header's value, built now: for a transport building its request.
    pub fn header_value(&self) -> SecretString {
        SecretString::new(match self.scheme {
            AuthScheme::XApiKey => self.secret.expose().to_string(),
            AuthScheme::Bearer => format!("Bearer {}", self.secret.expose()),
        })
    }
}

/// Resolves a host name to its addresses (every answer is checked).
pub trait Resolver: Send + Sync {
    fn lookup(&self, host: &str, port: u16) -> BoxFuture<Result<Vec<IpAddr>, EgressError>>;
}

/// One checked hop: the URL, the one address it connects to, the headers
/// to send, and the credential the client attached, apart from them.
pub struct Hop {
    pub method: HttpMethod,
    pub url: String,
    pub addr: SocketAddr,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub credential: Option<Credential>,
}

/// Makes one hop, connected to `hop.addr` only, following no redirect.
pub trait Transport: Send + Sync {
    fn send(&self, hop: Hop) -> BoxFuture<Result<HttpResponse, EgressError>>;
}

/// The test seams of [`NetEgress::with_seams`].
pub struct Seams {
    pub resolver: Arc<dyn Resolver>,
    pub transport: Arc<dyn Transport>,
    pub limits: Limits,
}

/// The one egress client.
pub struct NetEgress {
    inner: Arc<Inner>,
}

struct Inner {
    directory: Arc<dyn EgressDirectory>,
    secrets: Arc<dyn SecretStore>,
    audit: Arc<dyn AuditSink>,
    resolver: Arc<dyn Resolver>,
    transport: Arc<dyn Transport>,
    limits: Limits,
    /// The app's own serving port (0: not serving).
    own_port: AtomicU16,
}

impl NetEgress {
    /// The client on the system resolver and reqwest, with the default caps.
    pub fn new(
        directory: Arc<dyn EgressDirectory>,
        secrets: Arc<dyn SecretStore>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        NetEgress::build(
            directory,
            secrets,
            audit,
            Seams {
                resolver: Arc::new(SystemResolver),
                transport: Arc::new(ReqwestTransport),
                limits: Limits::default(),
            },
        )
    }

    /// Tests only: the client on another resolver, transport and caps.
    /// Shipped code never names this constructor (`cargo xtask seam-guard`).
    pub fn with_seams(
        directory: Arc<dyn EgressDirectory>,
        secrets: Arc<dyn SecretStore>,
        audit: Arc<dyn AuditSink>,
        seams: Seams,
    ) -> Self {
        NetEgress::build(directory, secrets, audit, seams)
    }

    fn build(
        directory: Arc<dyn EgressDirectory>,
        secrets: Arc<dyn SecretStore>,
        audit: Arc<dyn AuditSink>,
        seams: Seams,
    ) -> Self {
        NetEgress {
            inner: Arc::new(Inner {
                directory,
                secrets,
                audit,
                resolver: seams.resolver,
                transport: seams.transport,
                limits: seams.limits,
                own_port: AtomicU16::new(0),
            }),
        }
    }

    /// `agentic serve` hands the client its port once bound: no Account or
    /// MCP endpoint on this Mac may be that port (AD-12).
    pub fn set_own_port(&self, port: u16) {
        self.inner.own_port.store(port, Ordering::SeqCst);
    }
}

impl EgressClient for NetEgress {
    fn send(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, EgressError>> {
        let inner = self.inner.clone();
        Box::pin(async move { inner.send(request).await })
    }
}

// ------------------------------------------------------------ decisions

/// The destination of a URL as the client reads it: scheme, lowercase
/// host (brackets stripped from an IPv6 literal), port (explicit or the
/// scheme's default), and the address when the host is an IP literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub ip: Option<IpAddr>,
    /// The URL carries a user name or a password.
    pub userinfo: bool,
}

impl Target {
    /// The endpoint of this target, when it has a port.
    pub fn endpoint(&self) -> Option<Endpoint> {
        Some(Endpoint {
            scheme: self.scheme.clone(),
            host: self.host.clone(),
            port: self.port?,
        })
    }

    fn same_origin(&self, other: &Target) -> bool {
        self.scheme == other.scheme && self.host == other.host && self.port == other.port
    }
}

/// Reads `url` as the client does. `Err` in words that never repeat the
/// URL (it may hold a key).
pub fn url_target(url: &str) -> Result<Target, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("the URL cannot be read ({e})"))?;
    Ok(target_of(&parsed))
}

/// The endpoint (scheme, host and port) the client reads in `url`.
pub fn url_endpoint(url: &str) -> Result<Endpoint, String> {
    url_target(url)?
        .endpoint()
        .ok_or_else(|| "the URL has no port".to_string())
}

fn target_of(parsed: &reqwest::Url) -> Target {
    let host = parsed
        .host_str()
        .unwrap_or("")
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    let ip = host.parse::<IpAddr>().ok();
    Target {
        scheme: parsed.scheme().to_ascii_lowercase(),
        host,
        port: parsed.port_or_known_default(),
        ip,
        userinfo: !parsed.username().is_empty() || parsed.password().is_some(),
    }
}

/// How a checked target is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// A host name: resolve it, check every answer, pin the first.
    Resolve { host: String, port: u16 },
    /// `127.0.0.1` at an Account's or MCP server's own endpoint: no lookup.
    Loopback(SocketAddr),
}

/// The caller's rule (AD-12, AD-48), from the directory alone: whether the
/// caller may reach `target`, and how. `own_port` is the app's serving
/// port (0: none). `Err` is the reason, in words naming no path or query.
pub fn check_caller(
    directory: &dyn EgressDirectory,
    caller: &Caller,
    target: &Target,
    own_port: u16,
) -> Result<Reach, String> {
    if target.userinfo {
        return Err("a URL with a user name or password is refused".into());
    }
    if target.scheme != "http" && target.scheme != "https" {
        return Err(format!("the scheme {} is not allowed", target.scheme));
    }
    if target.host.is_empty() {
        return Err("the URL has no host".into());
    }
    let Some(port) = target.port else {
        return Err("the URL has no port".into());
    };
    // The app's own ports: handed to the client by `serve`, or recorded by
    // every running server and read by the directory.
    let mut own_ports = directory.own_ports();
    own_ports.push(own_port);
    match caller {
        Caller::Seat { seat } => {
            let Some(patterns) = directory.seat_hosts(seat) else {
                return Err(format!("the seat {seat} is not known"));
            };
            if target.ip.is_some() || target.host == "localhost" {
                return Err(format!(
                    "{} is an address, and a seat reaches host names only",
                    target.host
                ));
            }
            if !patterns
                .iter()
                .any(|p| p.admits(&target.scheme, &target.host, port))
            {
                return Err(format!(
                    "{}://{}:{port} is not on the seat {seat}'s allowed hosts",
                    target.scheme, target.host
                ));
            }
            Ok(Reach::Resolve {
                host: target.host.clone(),
                port,
            })
        }
        Caller::Account { account, .. } => {
            let Some(endpoint) = directory.account_endpoint(account) else {
                return Err(match directory.account_refusal(account) {
                    Some(why) => format!("{ACCOUNT_NOT_ANSWERED}{account} is not called: {why}"),
                    None => format!("the Account {account} is not known"),
                });
            };
            let own_reserved = reserved_local_account(account).as_ref() == Some(&endpoint);
            pinned(
                &endpoint,
                target,
                port,
                own_ports.clone(),
                own_reserved,
                &format!("the Account {account}"),
            )
        }
        Caller::McpServer { server } => {
            let Some(endpoint) = directory.mcp_endpoint(server) else {
                return Err(format!("the MCP server {server} is not approved"));
            };
            pinned(
                &endpoint,
                target,
                port,
                own_ports,
                false,
                &format!("the MCP server {server}"),
            )
        }
    }
}

/// An Account's or MCP server's rule: exactly its endpoint; a non-loopback
/// endpoint is a host name over https; an IP literal only `127.0.0.1`,
/// never the app's own port, and a reserved local Account's port only for
/// that Account (`own_reserved`).
fn pinned(
    endpoint: &Endpoint,
    target: &Target,
    port: u16,
    own_ports: Vec<u16>,
    own_reserved: bool,
    who: &str,
) -> Result<Reach, String> {
    let same = target.scheme == endpoint.scheme.to_ascii_lowercase()
        && target.host == endpoint.host.to_ascii_lowercase()
        && port == endpoint.port;
    if !same {
        return Err(format!(
            "{}://{}:{port} is not {who}'s endpoint",
            target.scheme, target.host
        ));
    }
    match target.ip {
        Some(IpAddr::V4(v4)) if v4 == Ipv4Addr::LOCALHOST => {
            if own_ports.contains(&port) {
                return Err(format!(
                    "the app's own server is listening on port {port}, which {who} never reaches"
                ));
            }
            let reserved = am_domain::net::RESERVED_LOCAL_ACCOUNTS
                .iter()
                .filter_map(|id| reserved_local_account(id).map(|e| (*id, e.port)))
                .find(|(_, p)| *p == port);
            if let Some((id, _)) = reserved
                && !own_reserved
            {
                return Err(format!(
                    "127.0.0.1:{port} is the port of the reserved local Account {id}, which {who} never reaches"
                ));
            }
            Ok(Reach::Loopback(SocketAddr::new(IpAddr::V4(v4), port)))
        }
        Some(ip) => Err(format!(
            "{ip} is an address, and only 127.0.0.1 is an endpoint on this Mac"
        )),
        None => {
            if target.scheme != "https" {
                return Err(format!("{} must be reached over https", target.host));
            }
            Ok(Reach::Resolve {
                host: target.host.clone(),
                port,
            })
        }
    }
}

/// Checks every DNS answer and returns the address to pin: the first, when
/// none is blocked. `Err` names the blocked answer.
pub fn check_address(host: &str, port: u16, answers: &[IpAddr]) -> Result<SocketAddr, String> {
    if let Some(bad) = answers.iter().find(|ip| is_blocked_ip(**ip)) {
        return Err(format!("{host} resolves to the blocked address {bad}"));
    }
    answers
        .first()
        .map(|ip| SocketAddr::new(*ip, port))
        .ok_or_else(|| format!("{host} has no address"))
}

/// Private, loopback, link-local, CGNAT, metadata, multicast, unspecified,
/// reserved and documentation addresses, and the NAT64, 6to4 and
/// IPv4-mapped forms of any of them, are never reached by a resolved name.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v6(v6: Ipv6Addr) -> bool {
    if let Some(v4) = v6.to_ipv4_mapped() {
        return is_blocked_v4(v4);
    }
    let s = v6.segments();
    let embedded =
        |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // IPv4-translated (::ffff:0:0/96): the IPv4 address it carries.
    if s[..6] == [0, 0, 0, 0, 0xffff, 0] {
        return is_blocked_v4(embedded(s[6], s[7]));
    }
    // Teredo (2001::/32): the server's IPv4 address in bits 32..64 and the
    // client's, inverted, in the last 32 bits.
    if s[0] == 0x2001 && s[1] == 0 {
        return is_blocked_v4(embedded(s[2], s[3])) || is_blocked_v4(embedded(!s[6], !s[7]));
    }
    // NAT64 well-known prefix 64:ff9b::/96: the IPv4 address it carries.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_blocked_v4(embedded(s[6], s[7]));
    }
    // 6to4 (2002::/16): the IPv4 address in bits 16..48.
    if s[0] == 0x2002 {
        return is_blocked_v4(embedded(s[1], s[2]));
    }
    // IPv4-compatible (::a.b.c.d, deprecated), `::` and `::1` included.
    if s[..6] == [0, 0, 0, 0, 0, 0] {
        return true;
    }
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
        || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (s[0] & 0xffc0) == 0xfec0 // site-local fec0::/10 (deprecated)
        || (s[0] == 0x64 && s[1] == 0xff9b) // local-use NAT64 64:ff9b:1::/48
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x0100 && s[1..4] == [0, 0, 0]) // discard-only 100::/64
}

fn is_blocked_v4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local() // includes 169.254.169.254 (metadata)
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
        || v4.is_documentation()
        || o[0] == 0
        || o[0] >= 240 // reserved
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 protocol assignments
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // benchmarking
}

/// The caller-supplied header the client refuses, if any: a credential or
/// `host`.
fn forbidden_header(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .map(|(k, _)| k.to_ascii_lowercase())
        .find(|k| k == "host" || CREDENTIAL_HEADERS.contains(&k.as_str()))
}

/// The credential of a request, when it is the caller's own: the caller is
/// that Account (answered by the directory, which `check_caller` already
/// required) and not a reserved local one, which has no key.
fn credential_is_callers(auth: &AuthRef, caller: &Caller) -> bool {
    match caller {
        Caller::Account { account, .. } => {
            *account == auth.account_id && reserved_local_account(account).is_none()
        }
        Caller::Seat { .. } | Caller::McpServer { .. } => false,
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

// ------------------------------------------------------------- the loop

/// A refusal on its way to the audit log and the caller.
struct Refused {
    target: Option<Target>,
    reason: String,
}

impl Inner {
    /// One `egress.blocked` entry: the destination's scheme, host and port,
    /// the reason and the caller's id; never a path, query or header.
    fn block_event(
        &self,
        caller: Option<&Caller>,
        target: Option<&Target>,
        reason: &str,
    ) -> AuditEvent {
        let mut args = BTreeMap::new();
        let mut put = |k: &str, v: String| {
            if !v.is_empty() {
                args.insert(k.to_string(), v);
            }
        };
        if let Some(t) = target {
            put("scheme", t.scheme.clone());
            put("host", t.host.clone());
            put("port", t.port.map(|p| p.to_string()).unwrap_or_default());
        }
        put("reason", reason.to_string());
        match caller {
            Some(Caller::Seat { seat }) => put("seat", seat.clone()),
            Some(Caller::Account { account, seat }) => {
                put("account", account.clone());
                put("seat", seat.clone().unwrap_or_default());
            }
            Some(Caller::McpServer { server }) => put("server", server.clone()),
            None => {}
        }
        let destination = match target {
            Some(t) => match t.port {
                Some(p) => format!("{}://{}:{p}", t.scheme, t.host),
                None => format!("{}://{}", t.scheme, t.host),
            },
            None => "(no destination)".to_string(),
        };
        AuditEvent {
            actor: Actor::System {
                component: "am-net".into(),
            },
            action: "egress.blocked".into(),
            target: destination,
            outcome: "blocked".into(),
            hash: None,
            args,
        }
    }

    fn refuse(&self, caller: Option<&Caller>, refused: Refused) -> EgressError {
        // An Account the directory refuses is audited with a fixed reason:
        // the directory's words (which may come from the Accounts file) go
        // back to the caller only, never into an audit chain.
        let (audited, returned) = match refused.reason.strip_prefix(ACCOUNT_NOT_ANSWERED) {
            Some(rest) => (
                ACCOUNT_REFUSED_REASON.to_string(),
                format!("the Account {rest}"),
            ),
            None => (refused.reason.clone(), refused.reason),
        };
        self.audit
            .record(self.block_event(caller, refused.target.as_ref(), &audited));
        EgressError::Blocked(returned)
    }

    /// The caller's rule, then the address check: the one address to
    /// connect to for `target`.
    async fn checked_addr(
        &self,
        caller: &Caller,
        target: &Target,
    ) -> Result<Result<SocketAddr, Refused>, EgressError> {
        let own = self.own_port.load(Ordering::SeqCst);
        let reach = match check_caller(self.directory.as_ref(), caller, target, own) {
            Ok(r) => r,
            Err(reason) => {
                return Ok(Err(Refused {
                    target: Some(target.clone()),
                    reason,
                }));
            }
        };
        match reach {
            Reach::Loopback(addr) => Ok(Ok(addr)),
            Reach::Resolve { host, port } => {
                // One lookup per hop; the hop goes to the checked address.
                let answers = self.resolver.lookup(&host, port).await?;
                Ok(
                    check_address(&host, port, &answers).map_err(|reason| Refused {
                        target: Some(target.clone()),
                        reason,
                    }),
                )
            }
        }
    }

    async fn send(self: Arc<Self>, req: HttpRequest) -> Result<HttpResponse, EgressError> {
        let parsed = reqwest::Url::parse(&req.url);
        let mut target = parsed.as_ref().ok().map(target_of);
        let caller = req.caller.clone();
        let Some(caller) = caller else {
            return Err(self.refuse(
                None,
                Refused {
                    target,
                    reason: "the request names no caller (seat, Account or MCP server)".into(),
                },
            ));
        };
        let Ok(mut url) = parsed else {
            return Err(self.refuse(
                Some(&caller),
                Refused {
                    target: None,
                    reason: "the URL cannot be read".into(),
                },
            ));
        };
        if let Some(name) = forbidden_header(&req.headers) {
            return Err(self.refuse(
                Some(&caller),
                Refused {
                    target,
                    reason: format!("the request sets the {name} header itself; only the client attaches a credential"),
                },
            ));
        }
        let max_request = self.limits.max_request_bytes;
        if req.body.len() as u64 > max_request {
            return Err(self.refuse(
                Some(&caller),
                Refused {
                    target,
                    reason: format!(
                        "the request body of {} bytes is over the cap of {max_request} bytes",
                        req.body.len()
                    ),
                },
            ));
        }
        if let Some(auth) = &req.auth
            && !credential_is_callers(auth, &caller)
        {
            return Err(self.refuse(
                Some(&caller),
                Refused {
                    target,
                    reason: NOT_THE_CALLERS_CREDENTIAL.into(),
                },
            ));
        }
        let mut current = target.take().unwrap_or_else(|| target_of(&url));
        // AD-15 *Key use*: a keyed Account's stamp is taken before its host
        // is resolved, and again right after the key read.
        let mut stamp = req
            .auth
            .as_ref()
            .map(|auth| self.directory.account_stamp(&auth.account_id));
        let mut addr = match self.checked_addr(&caller, &current).await? {
            Ok(a) => a,
            Err(refused) => return Err(self.refuse(Some(&caller), refused)),
        };
        // The credential, read only after every check passed; kept as a
        // `SecretString` until the transport builds its header. AD-15 *Key
        // use*: the key is sent only when the stamp taken before resolving
        // and the one taken right after the key read are both present,
        // equal, and confirm the endpoint the request goes to; otherwise the
        // key is dropped unsent and the checks run once more, and a second
        // failure refuses the call.
        let mut credential = None;
        let mut pinned_stamp = None;
        if let Some(auth) = &req.auth {
            let mut attempt = 0;
            loop {
                let before = stamp.take().flatten();
                let goes_to = current.endpoint();
                if let Some(before) = before.filter(|s| Some(&s.endpoint) == goes_to.as_ref()) {
                    let secret = self
                        .secrets
                        .get(&auth.account_id)
                        .map_err(|e| EgressError::Network(e.to_string()))?
                        .ok_or_else(|| EgressError::MissingSecret(auth.account_id.clone()))?;
                    if self.directory.account_stamp(&auth.account_id).as_ref() == Some(&before) {
                        credential = Some(Credential {
                            scheme: auth.scheme,
                            secret: Arc::new(secret),
                        });
                        pinned_stamp = Some(before);
                        break;
                    }
                    drop(secret);
                }
                attempt += 1;
                if attempt > 1 {
                    return Err(self.refuse(
                        Some(&caller),
                        Refused {
                            target: Some(current.clone()),
                            reason: key_moved_words(&auth.account_id),
                        },
                    ));
                }
                stamp = Some(self.directory.account_stamp(&auth.account_id));
                addr = match self.checked_addr(&caller, &current).await? {
                    Ok(a) => a,
                    Err(refused) => return Err(self.refuse(Some(&caller), refused)),
                };
            }
        }
        let mut headers = req.headers.clone();
        let mut redirects = 0usize;
        loop {
            let hop = Hop {
                method: req.method,
                url: url.to_string(),
                addr,
                headers: headers.clone(),
                body: req.body.clone(),
                credential: credential.clone(),
            };
            let sent = self.transport.send(hop);
            let head = match self.limits.timeouts(&caller).head {
                None => Ok(sent.await),
                Some(limit) => tokio::time::timeout(limit, sent).await.map_err(|_| limit),
            };
            let resp = match head {
                Ok(sent) => sent?,
                Err(head_timeout) => {
                    let reason = format!(
                        "no response head arrived within {} s: timed out",
                        head_timeout.as_secs_f64()
                    );
                    self.audit
                        .record(self.block_event(Some(&caller), Some(&current), &reason));
                    return Err(EgressError::Network(format!(
                        "{}:{}: {reason}",
                        current.host,
                        current.port.unwrap_or_default()
                    )));
                }
            };
            let location = resp.header("location");
            let Some(location) = location.filter(|_| is_redirect(resp.status)) else {
                return self.capped(resp, &caller, &current);
            };
            drop(resp);
            let next_url = url.join(&location);
            let next = next_url.as_ref().ok().map(target_of);
            let at = next.clone().unwrap_or_else(|| current.clone());
            let refuse_at = |reason: String| Refused {
                target: Some(at.clone()),
                reason,
            };
            if req.method != HttpMethod::Get {
                return Err(self.refuse(
                    Some(&caller),
                    refuse_at("a redirect is followed only for GET".into()),
                ));
            }
            if redirects == MAX_REDIRECTS {
                return Err(self.refuse(
                    Some(&caller),
                    refuse_at(format!(
                        "no further redirect is followed after {MAX_REDIRECTS} redirects"
                    )),
                ));
            }
            let (Ok(next_url), Some(next)) = (next_url, next) else {
                return Err(self.refuse(
                    Some(&caller),
                    refuse_at("the redirect's location cannot be read".into()),
                ));
            };
            addr = match self.checked_addr(&caller, &next).await? {
                Ok(a) => a,
                Err(refused) => return Err(self.refuse(Some(&caller), refused)),
            };
            if next.same_origin(&current)
                && credential.is_some()
                && let Some(auth) = &req.auth
            {
                // A same-origin hop reuses the key only while the Account's
                // confirmation still stands where it stood at the key read.
                let now = self.directory.account_stamp(&auth.account_id);
                if now.is_none() || now != pinned_stamp {
                    drop(credential.take());
                    return Err(self.refuse(
                        Some(&caller),
                        Refused {
                            target: Some(next.clone()),
                            reason: key_moved_words(&auth.account_id),
                        },
                    ));
                }
            }
            if !next.same_origin(&current) {
                // For the rest of the chain: no credential, no
                // credential-like or session header.
                credential = None;
                headers.retain(|(k, _)| {
                    let k = k.to_ascii_lowercase();
                    !CREDENTIAL_HEADERS.contains(&k.as_str())
                        && !SESSION_HEADERS.contains(&k.as_str())
                });
            }
            redirects += 1;
            url = next_url;
            current = next;
        }
    }

    /// The response with its body capped: past the cap it is cut with
    /// `TooLarge`, and the cut is audited.
    fn capped(
        self: &Arc<Self>,
        resp: HttpResponse,
        caller: &Caller,
        target: &Target,
    ) -> Result<HttpResponse, EgressError> {
        let max = self.limits.max_response_bytes;
        let event = self.block_event(
            Some(caller),
            Some(target),
            &format!("the response body is over the cap of {max} bytes"),
        );
        let declared = resp
            .header("content-length")
            .and_then(|v| v.trim().parse::<u64>().ok());
        if declared.is_some_and(|n| n > max) {
            self.audit.record(event);
            return Err(EgressError::TooLarge(max));
        }
        let idle = self.limits.timeouts(caller).idle;
        let idle_event = self.block_event(
            Some(caller),
            Some(target),
            &format!(
                "the response body sent nothing for {} s: timed out",
                idle.unwrap_or_default().as_secs_f64()
            ),
        );
        Ok(HttpResponse {
            status: resp.status,
            headers: resp.headers,
            body: Box::new(CappedBody {
                inner: resp.body,
                max,
                idle,
                total: Arc::new(AtomicU64::new(0)),
                cut: Arc::new(AtomicBool::new(false)),
                audit: self.audit.clone(),
                event,
                idle_event,
            }),
        })
    }
}

/// A response body cut at the cap, and given up after `idle` with nothing
/// sent; either is audited once.
struct CappedBody {
    inner: Box<dyn BodyChunks>,
    max: u64,
    idle: Option<Duration>,
    total: Arc<AtomicU64>,
    cut: Arc<AtomicBool>,
    audit: Arc<dyn AuditSink>,
    event: AuditEvent,
    idle_event: AuditEvent,
}

impl BodyChunks for CappedBody {
    fn next(&self) -> BoxFuture<Option<Result<Vec<u8>, EgressError>>> {
        if self.cut.load(Ordering::SeqCst) {
            return Box::pin(async { None });
        }
        let next = self.inner.next();
        let (max, idle, total, cut) = (self.max, self.idle, self.total.clone(), self.cut.clone());
        let (audit, event, idle_event) = (
            self.audit.clone(),
            self.event.clone(),
            self.idle_event.clone(),
        );
        Box::pin(async move {
            let waited = match idle {
                None => Ok(next.await),
                Some(limit) => tokio::time::timeout(limit, next).await.map_err(|_| limit),
            };
            let chunk = match waited {
                Ok(chunk) => chunk?,
                Err(idle) => {
                    if !cut.swap(true, Ordering::SeqCst) {
                        audit.record(idle_event);
                    }
                    return Some(Err(EgressError::Network(format!(
                        "the response body sent nothing for {} s: timed out",
                        idle.as_secs_f64()
                    ))));
                }
            };
            let Ok(bytes) = chunk else {
                return Some(chunk);
            };
            let seen = total.fetch_add(bytes.len() as u64, Ordering::SeqCst) + bytes.len() as u64;
            if seen > max {
                if !cut.swap(true, Ordering::SeqCst) {
                    audit.record(event);
                }
                return Some(Err(EgressError::TooLarge(max)));
            }
            Some(Ok(bytes))
        })
    }
}

// ------------------------------------------------------ system adapters

/// The system resolver (`getaddrinfo` through tokio).
struct SystemResolver;

impl Resolver for SystemResolver {
    fn lookup(&self, host: &str, port: u16) -> BoxFuture<Result<Vec<IpAddr>, EgressError>> {
        let host = host.to_string();
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host.as_str(), port))
                .await
                .map_err(|e| EgressError::Network(format!("could not resolve {host}: {e}")))?;
            Ok(addrs.map(|a| a.ip()).collect())
        })
    }
}

/// One hop on reqwest, pinned to the hop's address, following no redirect,
/// through no proxy.
struct ReqwestTransport;

impl Transport for ReqwestTransport {
    fn send(&self, hop: Hop) -> BoxFuture<Result<HttpResponse, EgressError>> {
        Box::pin(async move {
            let url = reqwest::Url::parse(&hop.url)
                .map_err(|e| EgressError::Network(format!("the URL cannot be read ({e})")))?;
            let host = url.host_str().unwrap_or("").to_string();
            // The only reqwest client construction in the workspace (AD-12).
            #[allow(clippy::disallowed_methods, clippy::disallowed_types)]
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(Duration::from_secs(10))
                .resolve(&host, hop.addr)
                .build()
                .map_err(|e| EgressError::Network(e.to_string()))?;
            let method = match hop.method {
                HttpMethod::Get => reqwest::Method::GET,
                HttpMethod::Post => reqwest::Method::POST,
                HttpMethod::Delete => reqwest::Method::DELETE,
            };
            let mut builder = client.request(method, url);
            for (k, v) in &hop.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            if let Some(credential) = &hop.credential {
                // Built here, from the `SecretString`, and marked sensitive
                // (hidden from Debug, kept out of HPACK tables).
                let mut value =
                    reqwest::header::HeaderValue::from_str(credential.header_value().expose())
                        .map_err(|_| {
                            EgressError::Network("the key is not a valid header value".into())
                        })?;
                value.set_sensitive(true);
                builder = builder.header(credential.header_name(), value);
            }
            if !hop.body.is_empty() {
                builder = builder.body(hop.body);
            }
            let mut resp = builder
                .send()
                .await
                .map_err(|e| EgressError::Network(e.without_url().to_string()))?;
            let status = resp.status().as_u16();
            let headers = resp
                .headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_ascii_lowercase(),
                        String::from_utf8_lossy(v.as_bytes()).to_string(),
                    )
                })
                .collect();
            let (tx, rx) = mpsc::channel(32);
            tokio::spawn(async move {
                loop {
                    // Race the next chunk against the reader going away, so an
                    // idle stream is dropped as soon as its body is (cancellation).
                    let next = tokio::select! {
                        biased;
                        _ = tx.closed() => return,
                        c = resp.chunk() => c,
                    };
                    match next {
                        Ok(Some(bytes)) => {
                            if tx.send(Ok(bytes.to_vec())).await.is_err() {
                                return;
                            }
                        }
                        Ok(None) => return,
                        Err(e) => {
                            let _ = tx
                                .send(Err(EgressError::Network(e.without_url().to_string())))
                                .await;
                            return;
                        }
                    }
                }
            });
            Ok(HttpResponse {
                status,
                headers,
                body: channel_body(rx),
            })
        })
    }
}

/// The receiving half of a response-body channel.
pub type ChunkReceiver = mpsc::Receiver<Result<Vec<u8>, EgressError>>;

struct ChannelBody {
    rx: Arc<Mutex<ChunkReceiver>>,
}

impl BodyChunks for ChannelBody {
    fn next(&self) -> BoxFuture<Option<Result<Vec<u8>, EgressError>>> {
        let rx = self.rx.clone();
        Box::pin(async move { rx.lock().await.recv().await })
    }
}

/// Wrap a chunk channel as a response body (used by adapters' test doubles too).
pub fn channel_body(rx: ChunkReceiver) -> Box<dyn BodyChunks> {
    Box::new(ChannelBody {
        rx: Arc::new(Mutex::new(rx)),
    })
}

/// Read a whole body (capped by the egress client already).
pub async fn read_all(body: &dyn BodyChunks) -> Result<Vec<u8>, EgressError> {
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use am_domain::HostPattern;

    /// A seat `s` with `docs.example.org`, an Account `a` at Anthropic's
    /// endpoint and `l` at `127.0.0.1:<port>`.
    struct Dir {
        loopback_port: u16,
    }

    impl EgressDirectory for Dir {
        fn seat_hosts(&self, seat: &str) -> Option<Vec<HostPattern>> {
            (seat == "s").then(|| vec![HostPattern::parse("docs.example.org").unwrap()])
        }
        fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
            match account {
                "a" => Some(Endpoint {
                    scheme: "https".into(),
                    host: "api.anthropic.com".into(),
                    port: 443,
                }),
                "l" => Some(Endpoint {
                    scheme: "http".into(),
                    host: "127.0.0.1".into(),
                    port: self.loopback_port,
                }),
                other => am_domain::reserved_local_account(other),
            }
        }
        fn mcp_endpoint(&self, _server: &str) -> Option<Endpoint> {
            None
        }
    }

    fn check(caller: Caller, url: &str) -> Result<Reach, String> {
        check_caller(
            &Dir {
                loopback_port: 8080,
            },
            &caller,
            &url_target(url).unwrap(),
            0,
        )
    }

    fn seat() -> Caller {
        Caller::Seat { seat: "s".into() }
    }

    fn account(id: &str) -> Caller {
        Caller::Account {
            account: id.into(),
            seat: None,
        }
    }

    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: &str) -> Result<Option<am_domain::SecretString>, am_domain::SecretError> {
            Ok(None)
        }
        fn set(&self, _: &str, _: am_domain::SecretString) -> Result<(), am_domain::SecretError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), am_domain::SecretError> {
            Ok(())
        }
    }

    struct NoAudit;
    impl AuditSink for NoAudit {
        fn record(&self, _: AuditEvent) {}
    }

    /// Dropping the body of an idle stream closes the connection at once.
    #[tokio::test]
    async fn dropping_an_idle_body_closes_the_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await.unwrap();
            sock.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n")
                .await
                .unwrap();
            // Idle: send nothing more. Report how long until the client hangs up.
            let start = std::time::Instant::now();
            let n =
                tokio::time::timeout(std::time::Duration::from_secs(5), sock.read(&mut buf)).await;
            (n.map(|r| r.unwrap_or(0)).ok(), start.elapsed())
        });
        let egress = NetEgress::new(
            Arc::new(Dir {
                loopback_port: port,
            }),
            Arc::new(NoSecrets),
            Arc::new(NoAudit),
        );
        let resp = egress
            .send(HttpRequest {
                caller: Some(account("l")),
                ..HttpRequest::get(format!("http://127.0.0.1:{port}/"))
            })
            .await
            .unwrap();
        assert_eq!(resp.body.next().await.unwrap().unwrap(), b"hello".to_vec());
        drop(resp);
        let (read, waited) = server.await.unwrap();
        assert_eq!(read, Some(0), "the client closed the connection");
        assert!(
            waited < std::time::Duration::from_secs(1),
            "closed after {waited:?}"
        );
    }

    #[test]
    fn a_seats_rule_admits_its_hosts_only() {
        assert!(check(seat(), "https://docs.example.org/a").is_ok());
        assert!(check(seat(), "http://docs.example.org/a").is_ok());
        for url in [
            "https://evil.example/",
            "https://docs.example.org:8443/",
            "file:///etc/passwd",
            "https://user:pw@docs.example.org/",
            "https://10.1.2.3/",
            "http://localhost/",
        ] {
            assert!(check(seat(), url).is_err(), "{url}");
        }
        assert!(
            check(
                Caller::Seat { seat: "x".into() },
                "https://docs.example.org/"
            )
            .is_err()
        );
    }

    #[test]
    fn an_accounts_rule_is_its_endpoint_exactly() {
        assert!(matches!(
            check(account("a"), "https://api.anthropic.com/v1/messages"),
            Ok(Reach::Resolve { .. })
        ));
        assert_eq!(
            check(account("l"), "http://127.0.0.1:8080/v1"),
            Ok(Reach::Loopback("127.0.0.1:8080".parse().unwrap()))
        );
        assert_eq!(
            check(account("local-ollama"), "http://127.0.0.1:11434/api/ps"),
            Ok(Reach::Loopback("127.0.0.1:11434".parse().unwrap()))
        );
        for (id, url) in [
            ("a", "http://api.anthropic.com/v1/messages"),
            ("a", "https://api.anthropic.com:8443/v1/messages"),
            ("l", "http://127.0.0.1:8081/v1"),
            ("l", "http://localhost:8080/v1"),
            ("local-lmstudio", "http://127.0.0.1:11434/"),
            ("nobody", "https://api.anthropic.com/"),
        ] {
            assert!(check(account(id), url).is_err(), "{id} {url}");
        }
        let own = check_caller(
            &Dir {
                loopback_port: 8080,
            },
            &account("l"),
            &url_target("http://127.0.0.1:8080/v1").unwrap(),
            8080,
        );
        assert!(own.is_err(), "the app's own port");
    }

    #[test]
    fn blocked_addresses() {
        for ip in [
            "10.0.0.1",
            "172.16.3.4",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "240.0.0.1",
            "::",
            "::1",
            "fd00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:5",
            "64:ff9b::a9fe:a9fe",
            "2002:a00:5::1",
            "2002:7f00:1::1",
        ] {
            assert!(is_blocked_ip(ip.parse().unwrap()), "{ip} should be blocked");
        }
        for ip in [
            "8.8.8.8",
            "2606:4700::1111",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(!is_blocked_ip(ip.parse().unwrap()), "{ip} is public");
        }
        assert!(
            check_address(
                "x",
                443,
                &["8.8.8.8".parse().unwrap(), "10.0.0.1".parse().unwrap()]
            )
            .is_err()
        );
        assert_eq!(
            check_address("x", 443, &["8.8.8.8".parse().unwrap()]),
            Ok("8.8.8.8:443".parse().unwrap())
        );
    }
}
