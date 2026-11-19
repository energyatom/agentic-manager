//! MCP client port (AD-26). Results are always `UntrustedData`.

use crate::{BoxFuture, UntrustedData};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    /// The tool's JSON input schema, as JSON text.
    pub input_schema_json: String,
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("could not start or reach the MCP server: {0}")]
    Connect(String),
    #[error("MCP protocol error: {0}")]
    Protocol(String),
    #[error("the MCP tool reported an error: {0}")]
    Tool(String),
}

pub trait McpClient: Send + Sync {
    fn list_tools(&self) -> BoxFuture<Result<Vec<McpToolInfo>, McpError>>;
    /// `arguments_json` is a JSON object as text.
    fn call_tool(
        &self,
        name: String,
        arguments_json: String,
    ) -> BoxFuture<Result<UntrustedData, McpError>>;
    /// Close the session and stop a child process (killing its process group).
    fn shutdown(&self) -> BoxFuture<()>;
}
