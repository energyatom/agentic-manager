//! `am-net`: the only HTTP client in the app (AD-12).
//!
//! [`NetEgress`] implements the `EgressClient` port on reqwest. Each request is
//! checked against the [`EgressPolicy`] before any connection is made:
//!
//! - remote hosts must be `https` and on the allow-list; every resolved address
//!   must be public (private, loopback, link-local, CGNAT, metadata and
//!   multicast addresses are blocked), and the connection is pinned to the
//!   checked address;
//! - loopback is reachable only on the listed ports (LM Studio 1234 and Ollama
//!   11434 by default; an owner-added port is privilege-widening, AD-15);
//! - redirects are not followed; response bodies are size-capped;
//! - keys are fetched from the `SecretStore` here and nowhere else.
#![forbid(unsafe_code)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use am_domain::{
    AuditEvent, AuditSink, AuthScheme, BodyChunks, BoxFuture, EgressClient, EgressError,
    HttpMethod, HttpRequest, HttpResponse, SecretStore,
};
use tokio::sync::{Mutex, mpsc};

/// Ports of the reserved local Accounts (AD-31): LM Studio and Ollama.
pub const LOCAL_LMSTUDIO_PORT: u16 = 1234;
pub const LOCAL_OLLAMA_PORT: u16 = 11434;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPolicy {
    /// Remote hosts allowed (exact, case-insensitive), reached over https only.
    pub allowed_hosts: Vec<String>,
    /// Loopback ports allowed over http on 127.0.0.1.
    pub loopback_ports: Vec<u16>,
    /// Response bodies larger than this are cut with `EgressError::TooLarge`.
    pub max_body_bytes: u64,
}

impl Default for EgressPolicy {
    fn default() -> Self {
        EgressPolicy {
            allowed_hosts: Vec::new(),
            loopback_ports: vec![LOCAL_LMSTUDIO_PORT, LOCAL_OLLAMA_PORT],
            max_body_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Where a checked request may connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub host: String,
    pub addr: SocketAddr,
}

pub struct NetEgress {
    policy: EgressPolicy,
    secrets: Arc<dyn SecretStore>,
    audit: Option<Arc<dyn AuditSink>>,
}

impl NetEgress {
    pub fn new(
        policy: EgressPolicy,
        secrets: Arc<dyn SecretStore>,
        audit: Option<Arc<dyn AuditSink>>,
    ) -> Self {
        NetEgress {
            policy,
            secrets,
            audit,
        }
    }

    pub fn policy(&self) -> &EgressPolicy {
        &self.policy
    }
}

/// Parse and check the URL against the policy, without DNS. Returns the host,
/// the port and whether it is a loopback target.
pub fn check_url(policy: &EgressPolicy, url: &str) -> Result<(String, u16, bool), EgressError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| EgressError::Blocked(format!("bad URL: {e}")))?;
    let scheme = parsed.scheme().to_string();
    if scheme != "http" && scheme != "https" {
        return Err(EgressError::Blocked(format!(
            "scheme {scheme} is not allowed"
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(EgressError::Blocked(
            "credentials in URLs are not allowed".into(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| EgressError::Blocked("URL has no host".into()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| EgressError::Blocked("URL has no port".into()))?;
    let loopback = host == "localhost"
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    if loopback {
        if !policy.loopback_ports.contains(&port) {
            return Err(EgressError::Blocked(format!(
                "loopback port {port} is not an allowed local Account port"
            )));
        }
        return Ok((host, port, true));
    }
    if scheme != "https" {
        return Err(EgressError::Blocked(format!(
            "{host} must be reached over https"
        )));
    }
    if !policy
        .allowed_hosts
        .iter()
        .any(|h| h.eq_ignore_ascii_case(&host))
    {
        return Err(EgressError::Blocked(format!(
            "{host} is not on the allow-list"
        )));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_blocked_ip(ip)
    {
        return Err(EgressError::Blocked(format!("{ip} is a blocked address")));
    }
    Ok((host, port, false))
}

/// Private, loopback, link-local, CGNAT, metadata, multicast and unspecified
/// addresses are never reached by a remote-host request.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_v4(v4);
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xfe00) == 0xfc00 // unique local
                || (seg0 & 0xffc0) == 0xfe80 // link-local
        }
    }
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
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // benchmarking
}

/// Resolve and pin the route for a checked URL.
pub async fn route(policy: &EgressPolicy, url: &str) -> Result<Route, EgressError> {
    let (host, port, loopback) = check_url(policy, url)?;
    if loopback {
        let ip = if host.contains(':') {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        return Ok(Route {
            host,
            addr: SocketAddr::new(ip, port),
        });
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| EgressError::Network(format!("could not resolve {host}: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(EgressError::Network(format!("{host} has no address")));
    }
    if let Some(bad) = addrs.iter().find(|a| is_blocked_ip(a.ip())) {
        return Err(EgressError::Blocked(format!(
            "{host} resolves to the blocked address {}",
            bad.ip()
        )));
    }
    Ok(Route {
        host,
        addr: addrs[0],
    })
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

impl NetEgress {
    fn audit_block(&self, req: &HttpRequest, reason: &str) {
        if let Some(audit) = &self.audit {
            audit.record(AuditEvent {
                actor: am_domain::Actor::System {
                    component: "am-net".into(),
                },
                action: "egress.blocked".into(),
                target: req.url.split('?').next().unwrap_or("").to_string(),
                outcome: reason.to_string(),
                hash: None,
            });
        }
    }

    async fn send_checked(&self, req: HttpRequest) -> Result<HttpResponse, EgressError> {
        let route = match route(&self.policy, &req.url).await {
            Ok(r) => r,
            Err(e) => {
                self.audit_block(&req, &e.to_string());
                return Err(e);
            }
        };
        // The only reqwest client construction in the workspace (AD-12).
        #[allow(clippy::disallowed_methods)]
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .resolve(&route.host, route.addr)
            .build()
            .map_err(|e| EgressError::Network(e.to_string()))?;
        let method = match req.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let mut builder = client.request(method, &req.url);
        for (k, v) in &req.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if let Some(auth) = &req.auth {
            let secret = self
                .secrets
                .get(&auth.account_id)
                .map_err(|e| EgressError::Network(e.to_string()))?
                .ok_or_else(|| EgressError::MissingSecret(auth.account_id.clone()))?;
            builder = match auth.scheme {
                AuthScheme::XApiKey => builder.header("x-api-key", secret.expose()),
                AuthScheme::Bearer => builder.bearer_auth(secret.expose()),
            };
        }
        if !req.body.is_empty() {
            builder = builder.body(req.body.clone());
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
        let max = self.policy.max_body_bytes;
        tokio::spawn(async move {
            let mut total: u64 = 0;
            loop {
                match resp.chunk().await {
                    Ok(Some(bytes)) => {
                        total += bytes.len() as u64;
                        if total > max {
                            let _ = tx.send(Err(EgressError::TooLarge(max))).await;
                            return;
                        }
                        if tx.send(Ok(bytes.to_vec())).await.is_err() {
                            // Receiver dropped: the caller cancelled; drop the connection.
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
    }
}

impl EgressClient for NetEgress {
    fn send(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, EgressError>> {
        let this = NetEgress {
            policy: self.policy.clone(),
            secrets: self.secrets.clone(),
            audit: self.audit.clone(),
        };
        Box::pin(async move { this.send_checked(request).await })
    }
}

/// Read a whole body (capped by the egress policy already).
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

    fn policy() -> EgressPolicy {
        EgressPolicy {
            allowed_hosts: vec!["api.anthropic.com".into()],
            ..EgressPolicy::default()
        }
    }

    #[test]
    fn allow_list_and_scheme() {
        let p = policy();
        assert!(check_url(&p, "https://api.anthropic.com/v1/messages").is_ok());
        assert!(check_url(&p, "http://api.anthropic.com/v1/messages").is_err());
        assert!(check_url(&p, "https://evil.example/").is_err());
        assert!(check_url(&p, "file:///etc/passwd").is_err());
        assert!(check_url(&p, "https://user:pw@api.anthropic.com/").is_err());
    }

    #[test]
    fn loopback_only_on_local_account_ports() {
        let p = policy();
        assert!(check_url(&p, "http://127.0.0.1:1234/v1/models").is_ok());
        assert!(check_url(&p, "http://localhost:11434/api/ps").is_ok());
        assert!(check_url(&p, "http://127.0.0.1:8080/").is_err());
        assert!(check_url(&p, "http://[::1]:22/").is_err());
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
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_blocked_ip(ip.parse().unwrap()), "{ip} should be blocked");
        }
        assert!(!is_blocked_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn allow_listed_private_literal_is_still_blocked() {
        let p = EgressPolicy {
            allowed_hosts: vec!["10.1.2.3".into()],
            ..EgressPolicy::default()
        };
        assert!(check_url(&p, "https://10.1.2.3/").is_err());
    }
}
