//! `am-mcp`: the `McpClient` adapter over rmcp (AD-26).
//!
//! - **stdio**: the server runs as a child process with a scrubbed environment
//!   (only the variables its record lists), its own working folder as cwd, in
//!   its own process group (killed with the group on shutdown or drop).
//! - **Streamable HTTP**: rmcp's transport runs over [`EgressHttpClient`], our
//!   implementation of rmcp's `StreamableHttpClient` on `am-net`'s
//!   `EgressClient`, so MCP-over-HTTP obeys the egress allow-lists (AD-12) and
//!   no second HTTP client exists.
//!
//! Tool results are always `UntrustedData`. No rmcp type crosses the port.
#![forbid(unsafe_code)]

mod http_client;

use std::path::PathBuf;
use std::sync::Arc;

use am_domain::{BoxFuture, EgressClient, McpClient, McpError, McpToolInfo, UntrustedData};
use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::service::{Peer, RunningService};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use tokio::sync::Mutex;

pub use http_client::EgressHttpClient;

/// A stdio server record (spike subset of the `tools.yaml` record, AD-26).
#[derive(Debug, Clone)]
pub struct StdioSpec {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    /// The only environment variables the child receives (name, value).
    pub env: Vec<(String, String)>,
    /// The child's working folder (`RuntimeLayout::mcp_workspace`, the
    /// runtime folder's `workspace/mcp/<server-id>/`).
    pub cwd: PathBuf,
}

pub struct RmcpClient {
    id: String,
    peer: Peer<RoleClient>,
    service: Arc<Mutex<Option<RunningService<RoleClient, ()>>>>,
}

impl RmcpClient {
    /// Start a stdio server and complete the MCP handshake.
    pub async fn spawn_stdio(spec: StdioSpec) -> Result<RmcpClient, McpError> {
        std::fs::create_dir_all(&spec.cwd)
            .map_err(|e| McpError::Connect(format!("cannot create {}: {e}", spec.cwd.display())))?;
        let mut cmd = tokio::process::Command::new(&spec.command);
        cmd.args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k.clone(), v.clone())))
            .current_dir(&spec.cwd)
            .kill_on_drop(true);
        let mut wrapped = process_wrap::tokio::CommandWrap::from(cmd);
        #[cfg(unix)]
        wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
        let (transport, _stderr) = TokioChildProcess::builder(wrapped)
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| McpError::Connect(format!("{}: {e}", spec.command)))?;
        let service = ().serve(transport).await.map_err(|e| McpError::Connect(e.to_string()))?;
        Ok(Self::from_service(spec.id, service))
    }

    /// Connect to a Streamable HTTP server through the egress client.
    pub async fn connect_http(
        id: &str,
        url: &str,
        egress: Arc<dyn EgressClient>,
    ) -> Result<RmcpClient, McpError> {
        let config =
            rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
                url.to_string(),
            );
        let transport =
            StreamableHttpClientTransport::with_client(EgressHttpClient::new(egress), config);
        let service = ().serve(transport).await.map_err(|e| McpError::Connect(e.to_string()))?;
        Ok(Self::from_service(id.to_string(), service))
    }

    fn from_service(id: String, service: RunningService<RoleClient, ()>) -> RmcpClient {
        RmcpClient {
            id,
            peer: service.peer().clone(),
            service: Arc::new(Mutex::new(Some(service))),
        }
    }
}

impl McpClient for RmcpClient {
    fn list_tools(&self) -> BoxFuture<Result<Vec<McpToolInfo>, McpError>> {
        let peer = self.peer.clone();
        Box::pin(async move {
            let tools = peer
                .list_all_tools()
                .await
                .map_err(|e| McpError::Protocol(e.to_string()))?;
            Ok(tools
                .into_iter()
                .map(|t| McpToolInfo {
                    name: t.name.to_string(),
                    description: t.description.map(|d| d.to_string()),
                    input_schema_json: serde_json::to_string(&*t.input_schema)
                        .unwrap_or_else(|_| "{}".into()),
                })
                .collect())
        })
    }

    fn call_tool(
        &self,
        name: String,
        arguments_json: String,
    ) -> BoxFuture<Result<UntrustedData, McpError>> {
        let peer = self.peer.clone();
        let label = format!("mcp:{}/{}", self.id, name);
        Box::pin(async move {
            let args: JsonObject = serde_json::from_str(&arguments_json)
                .map_err(|e| McpError::Protocol(format!("arguments are not a JSON object: {e}")))?;
            let result = peer
                .call_tool(CallToolRequestParams::new(name).with_arguments(args))
                .await
                .map_err(|e| McpError::Protocol(e.to_string()))?;
            let text = result
                .content
                .iter()
                .filter_map(|c| c.as_text().map(|t| t.text.clone()))
                .collect::<Vec<_>>()
                .join("\n");
            if result.is_error == Some(true) {
                return Err(McpError::Tool(text));
            }
            Ok(UntrustedData::text(label, text))
        })
    }

    fn shutdown(&self) -> BoxFuture<()> {
        let service = self.service.clone();
        Box::pin(async move {
            if let Some(s) = service.lock().await.take() {
                let _ = s.cancel().await;
            }
        })
    }
}
