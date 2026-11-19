//! Criterion 5: the rmcp client lists and calls tools on two official
//! reference servers, one over stdio and one over Streamable HTTP (through
//! `am-net`'s egress client). The live test launches them with npx, at the
//! versions pinned in stack-spike.md:
//!
//! - stdio: `@modelcontextprotocol/server-filesystem@2.4.0 --read-only <tmp>`
//! - Streamable HTTP: `@modelcontextprotocol/server-everything@2.3.1 streamableHttp`
//!   on 127.0.0.1:3001/mcp

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use am_domain::{McpClient, UntrustedContent};
use am_mcp::{RmcpClient, StdioSpec};
use am_net::{EgressPolicy, NetEgress};
use am_testkit::FakeSecretStore;

use super::live;

const FS_SERVER: &str = "@modelcontextprotocol/server-filesystem@2.4.0";
const EVERYTHING_SERVER: &str = "@modelcontextprotocol/server-everything@2.3.1";
const FIXTURE_TEXT: &str = "MCP fixture: the code word is HERON.\n";

fn egress_with_port(port: u16) -> Arc<NetEgress> {
    Arc::new(NetEgress::new(
        EgressPolicy {
            loopback_ports: vec![port],
            ..EgressPolicy::default()
        },
        FakeSecretStore::new(),
        None,
    ))
}

/// MCP over HTTP goes through the egress client: a server on a loopback port
/// that no Account allows is refused before any connection.
#[tokio::test]
async fn http_mcp_is_refused_by_egress_policy_off_the_allow_list() {
    let egress = Arc::new(NetEgress::new(
        EgressPolicy::default(),
        FakeSecretStore::new(),
        None,
    ));
    let err = RmcpClient::connect_http("everything", "http://127.0.0.1:3001/mcp", egress)
        .await
        .err()
        .expect("refused");
    assert!(
        err.to_string()
            .contains("not an allowed local Account port"),
        "{err}"
    );
}

fn npx() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("npx"))
        .find(|p| p.exists())
}

fn child_env() -> Vec<(String, String)> {
    // The only variables the child receives (AD-26): PATH for node, HOME for npm's cache.
    ["PATH", "HOME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect()
}

#[tokio::test]
#[ignore = "live: needs AM_LIVE=1 and Node.js (npx downloads the pinned reference servers)"]
async fn live_stdio_filesystem_server() {
    if !live::live_enabled() {
        return live::skip("AM_LIVE is not 1");
    }
    let Some(npx) = npx() else {
        return live::skip("pending owner input: Node.js (npx) is not installed");
    };
    let tmp = tempfile::tempdir().unwrap();
    let served = tmp.path().join("served");
    std::fs::create_dir(&served).unwrap();
    std::fs::write(served.join("fixture.txt"), FIXTURE_TEXT).unwrap();
    let served = served.canonicalize().unwrap();
    let client = RmcpClient::spawn_stdio(StdioSpec {
        id: "filesystem".into(),
        command: npx.display().to_string(),
        args: vec![
            "-y".into(),
            FS_SERVER.into(),
            "--read-only".into(),
            served.display().to_string(),
        ],
        env: child_env(),
        cwd: tmp.path().join("workspace/mcp/filesystem"),
    })
    .await
    .unwrap();
    let tools = client.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    eprintln!("stdio tools: {names:?}");
    assert!(names.contains(&"read_text_file"));
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with("write") || *n == "edit_file"),
        "read-only server"
    );
    let path = served.join("fixture.txt").display().to_string();
    let result = client
        .call_tool(
            "read_text_file".into(),
            format!(r#"{{"path": {}}}"#, serde_json_string(&path)),
        )
        .await
        .unwrap();
    assert_eq!(result.label(), "mcp:filesystem/read_text_file");
    match result.content() {
        UntrustedContent::Text(t) => assert!(t.contains("HERON"), "{t}"),
        other => panic!("{other:?}"),
    }
    client.shutdown().await;
}

fn serde_json_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[tokio::test]
#[ignore = "live: needs AM_LIVE=1 and Node.js (npx downloads the pinned reference servers)"]
async fn live_streamable_http_everything_server() {
    if !live::live_enabled() {
        return live::skip("AM_LIVE is not 1");
    }
    let Some(npx) = npx() else {
        return live::skip("pending owner input: Node.js (npx) is not installed");
    };
    let port: u16 = std::env::var("AM_SPIKE_MCP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);
    let tmp = tempfile::tempdir().unwrap();
    let mut server = tokio::process::Command::new(npx)
        .args(["-y", EVERYTHING_SERVER, "streamableHttp"])
        .env_clear()
        .envs(child_env())
        .env("PORT", port.to_string())
        .current_dir(tmp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0)
        .spawn()
        .unwrap();
    // Wait for the listener (npx may download the package first).
    let start = Instant::now();
    while tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err()
    {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "server did not start"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let url = format!("http://127.0.0.1:{port}/mcp");
    let client = RmcpClient::connect_http("everything", &url, egress_with_port(port))
        .await
        .unwrap();
    let tools = client.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    eprintln!("http tools: {names:?}");
    assert!(names.contains(&"echo"));
    let result = client
        .call_tool("echo".into(), r#"{"message": "spike says hello"}"#.into())
        .await
        .unwrap();
    match result.content() {
        UntrustedContent::Text(t) => assert!(t.contains("spike says hello"), "{t}"),
        other => panic!("{other:?}"),
    }
    client.shutdown().await;
    // Kill the whole process group (npx and the node server it started).
    if let Some(pid) = server.id() {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .status();
    }
    let _ = server.kill().await;
}
