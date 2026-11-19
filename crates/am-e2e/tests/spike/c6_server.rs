//! Criterion 6: the loopback server's Host, Origin and session checks reject
//! DNS rebinding, a cross-origin fetch and a cross-origin WebSocket upgrade;
//! no CORS headers are sent; the launch code is single-use.

use std::time::Duration;

use am_server::{AppState, bind, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Resp {
    status: u16,
    head: String,
    body: String,
}

async fn raw(port: u16, request: String) -> Resp {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    // Read until the server closes, or briefly for an upgraded connection.
    let _ = tokio::time::timeout(Duration::from_millis(500), s.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Resp {
        status,
        head: head.to_ascii_lowercase(),
        body: body.to_string(),
    }
}

fn req(method: &str, path: &str, headers: &[(&str, String)], body: &str) -> String {
    let mut s = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || method == "POST" {
        s.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    s.push_str("\r\n");
    s.push_str(body);
    s
}

async fn start() -> (AppState, u16) {
    let (listener, port) = bind(0).await.unwrap();
    let state = AppState::new(port);
    let st = state.clone();
    tokio::spawn(async move {
        let _ = serve(listener, st, std::future::pending()).await;
    });
    (state, port)
}

fn assert_no_cors(r: &Resp) {
    assert!(
        !r.head.contains("access-control-"),
        "CORS header sent:\n{}",
        r.head
    );
}

#[tokio::test]
async fn host_origin_csrf_and_websocket_checks() {
    let (state, port) = start().await;
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://127.0.0.1:{port}");
    let evil = "http://evil.example".to_string();

    // DNS rebinding: a foreign Host is refused.
    let r = raw(
        port,
        req("GET", "/", &[("Host", format!("evil.example:{port}"))], ""),
    )
    .await;
    assert_eq!(r.status, 403);
    let r = raw(port, req("GET", "/", &[("Host", "127.0.0.1:1".into())], "")).await;
    assert_eq!(r.status, 403);
    // The page itself loads for the right Host.
    let r = raw(port, req("GET", "/", &[("Host", host.clone())], "")).await;
    assert_eq!(r.status, 200);
    assert!(r.head.contains("content-security-policy"));

    // Cross-origin fetch of the session exchange: refused, no CORS headers,
    // and the failed attempt does not consume a fresh code.
    let code = state.issue_launch_code();
    let body = format!(r#"{{"code":"{code}"}}"#);
    let r = raw(
        port,
        req(
            "POST",
            "/api/v1/session",
            &[("Host", host.clone()), ("Origin", evil.clone())],
            &body,
        ),
    )
    .await;
    assert_eq!(r.status, 403);
    assert_no_cors(&r);
    // POST without any Origin is refused too.
    let r = raw(
        port,
        req("POST", "/api/v1/session", &[("Host", host.clone())], &body),
    )
    .await;
    assert_eq!(r.status, 403);

    // The real exchange.
    let r = raw(
        port,
        req(
            "POST",
            "/api/v1/session",
            &[("Host", host.clone()), ("Origin", origin.clone())],
            &body,
        ),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.head);
    assert_no_cors(&r);
    let cookie_line = r
        .head
        .lines()
        .find(|l| l.starts_with("set-cookie:"))
        .expect("cookie")
        .to_string();
    assert!(cookie_line.contains(&format!("am_s_{port}=")));
    assert!(cookie_line.contains("httponly") && cookie_line.contains("samesite=strict"));
    let sid = cookie_line
        .split_once('=')
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_string();
    let cookie = format!("am_s_{port}={sid}");
    let csrf = r
        .body
        .split("\"csrf\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("csrf in body")
        .to_string();

    // The code is single-use.
    let r = raw(
        port,
        req(
            "POST",
            "/api/v1/session",
            &[("Host", host.clone()), ("Origin", origin.clone())],
            &body,
        ),
    )
    .await;
    assert_eq!(r.status, 401);

    // A cookie alone authenticates nothing; the CSRF header is required.
    let r = raw(
        port,
        req(
            "GET",
            "/api/v1/health",
            &[("Host", host.clone()), ("Cookie", cookie.clone())],
            "",
        ),
    )
    .await;
    assert_eq!(r.status, 403);
    let r = raw(
        port,
        req(
            "GET",
            "/api/v1/health",
            &[
                ("Host", host.clone()),
                ("Cookie", cookie.clone()),
                ("X-AM-CSRF", csrf.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(r.status, 200);
    // ...and a foreign Origin with valid credentials is still refused.
    let r = raw(
        port,
        req(
            "GET",
            "/api/v1/health",
            &[
                ("Host", host.clone()),
                ("Origin", evil.clone()),
                ("Cookie", cookie.clone()),
                ("X-AM-CSRF", csrf.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(r.status, 403);
    assert_no_cors(&r);

    // WebSocket upgrades.
    let ws = |o: &str, protocol: Option<String>| {
        let mut h = vec![
            ("Host", host.clone()),
            ("Origin", o.to_string()),
            ("Cookie", cookie.clone()),
            ("Upgrade", "websocket".to_string()),
            ("Sec-WebSocket-Version", "13".to_string()),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==".to_string()),
        ];
        if let Some(p) = protocol {
            h.push(("Sec-WebSocket-Protocol", p));
        }
        let mut s = req("GET", "/api/v1/events", &h, "");
        s = s.replace("Connection: close", "Connection: Upgrade");
        s
    };
    let good_proto = Some(format!("am.csrf.{csrf}"));
    let r = raw(port, ws(&evil, good_proto.clone())).await;
    assert_eq!(r.status, 403, "cross-origin WebSocket refused");
    assert_no_cors(&r);
    let r = raw(port, ws(&origin, None)).await;
    assert_eq!(
        r.status, 403,
        "upgrade without the CSRF subprotocol refused"
    );
    let r = raw(port, ws(&origin, Some("am.csrf.wrong".into()))).await;
    assert_eq!(r.status, 403);
    let r = raw(port, ws(&origin, good_proto)).await;
    assert_eq!(r.status, 101, "{}", r.head);
    assert!(
        r.head
            .contains(&format!("sec-websocket-protocol: am.csrf.{csrf}"))
    );
}

#[tokio::test]
async fn failed_exchange_invalidates_outstanding_codes() {
    let (state, port) = start().await;
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://127.0.0.1:{port}");
    let code = state.issue_launch_code();
    let r = raw(
        port,
        req(
            "POST",
            "/api/v1/session",
            &[("Host", host.clone()), ("Origin", origin.clone())],
            r#"{"code":"guess"}"#,
        ),
    )
    .await;
    assert_eq!(r.status, 401);
    let r = raw(
        port,
        req(
            "POST",
            "/api/v1/session",
            &[("Host", host), ("Origin", origin)],
            &format!(r#"{{"code":"{code}"}}"#),
        ),
    )
    .await;
    assert_eq!(
        r.status, 401,
        "the real code was invalidated by the failed attempt"
    );
}

/// An `Upgrade` header does not exempt an API route from the session check.
#[tokio::test]
async fn upgrade_header_does_not_bypass_the_session_on_api_routes() {
    let (_state, port) = start().await;
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://127.0.0.1:{port}");
    let mut request = req(
        "GET",
        "/api/v1/health",
        &[
            ("Host", host),
            ("Origin", origin),
            ("Upgrade", "websocket".to_string()),
            ("Sec-WebSocket-Version", "13".to_string()),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==".to_string()),
        ],
        "",
    );
    request = request.replace("Connection: close", "Connection: Upgrade");
    let r = raw(port, request).await;
    assert_eq!(r.status, 401, "{}", r.head);
    assert!(!r.body.contains("\"ok\""));
}

#[tokio::test]
async fn binds_loopback_only() {
    let (listener, _port) = bind(0).await.unwrap();
    assert!(listener.local_addr().unwrap().ip().is_loopback());
}
