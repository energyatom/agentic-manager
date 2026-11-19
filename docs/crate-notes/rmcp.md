# rmcp 3.11.0

Pinned `=3.11.0`, `default-features = false`, features `client`,
`transport-child-process`, `transport-streamable-http-client`. Used only in
`am-mcp`; no rmcp type crosses the `McpClient` port (AD-5, AD-26).

## What we use

- Client: `().serve(transport).await` returns `RunningService<RoleClient, ()>`;
  `service.peer().clone()` is a cheap `Peer<RoleClient>` for calls;
  `service.cancel().await` closes the session.
- `peer.list_all_tools()` (pages through `tools/list`) and
  `peer.call_tool(CallToolRequestParams::new(name).with_arguments(obj))`
  (3.x renamed the param struct to `CallToolRequestParams`; `call_tool` also
  drives SEP-2322 `input_required` rounds). `CallToolResult.content` is a
  `Vec<ContentBlock>`; `ContentBlock::as_text()` gives `TextContent { text }`;
  `is_error: Option<bool>`.
- stdio: `TokioChildProcess::builder(CommandWrap).stderr(..).spawn()`; pass a
  `process_wrap::tokio::CommandWrap` (process-wrap `=10.0.1`, features
  `tokio1`, `process-group`, `kill-on-drop`) to put the server in its own
  process group. Build the `tokio::process::Command` with `env_clear()` and
  only the record's variables, and the server's own cwd.
- Streamable HTTP: `StreamableHttpClientTransport::with_client(client, config)`
  with `StreamableHttpClientTransportConfig::with_uri(url)`.

## Do not

- Do not enable `transport-streamable-http-client-reqwest` or `reqwest`: they
  build rmcp's own reqwest client, a second HTTP client (AD-12). Instead
  `am_mcp::EgressHttpClient` implements `StreamableHttpClient` (methods
  `post_message`, `delete_session`, `get_stream`; return
  `StreamableHttpPostResponse::{Accepted, Json, Sse}`) over our
  `EgressClient`. SSE bodies are decoded with `sse-stream =0.2.6`
  (`SseStream::from_bytes_stream`), the same version rmcp uses.
- Do not use the `auth` feature: keys go through `AuthRef` in `am-net` only.
- `auth_header` arrives as `Option<String>` from rmcp; our client refuses it.
