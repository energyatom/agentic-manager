//! RULE-15 trust test (story 3.2, AD-13): the Host header must be exactly
//! `127.0.0.1:<port>` or `localhost:<port>` for this server's port, checked
//! before anything else on every request (DNS rebinding, owner Decision
//! 2026-11-23), and the server listens on 127.0.0.1 only. Rows: "Rebinding
//! Host", "Near-miss Host", "Loopback only". Written first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use support::*;

/// Rebinding Host: `Host: evil.example:<port>` with valid credentials is
/// refused `host-not-allowed` on a read route, the fixture mutation, both
/// exchanges, the WebSocket upgrade, the page and an unknown path; nothing
/// changes, and the refused exchanges consume no code.
#[tokio::test]
async fn a_foreign_host_is_refused_on_every_surface() {
    let rig = Rig::with_project().await;
    let s = rig.sign_in().await;
    let evil_host = format!("evil.example:{}", rig.port);
    let launch = rig.state.issue_launch_code();
    let pairing = rig.state.issue_pairing_code().expect("not locked");

    let mut read = s.auth();
    read.push(("Host", evil_host.clone()));
    envelope(&rig.get(RUNS, &read).await, 403, "host-not-allowed");

    let mut mutate = s.auth();
    mutate.push(("Host", evil_host.clone()));
    mutate.push(("Origin", rig.origin()));
    envelope(
        &rig.send("POST", FIXTURE, &mutate, "{}").await,
        403,
        "host-not-allowed",
    );
    assert_eq!(rig.fixture.count(), 0, "nothing changed");

    for (path, code) in [(SESSION, launch.as_str()), (PAIR, pairing.expose())] {
        let r = rig
            .send(
                "POST",
                path,
                &[("Host", evil_host.clone()), ("Origin", rig.origin())],
                &format!(r#"{{"code":"{code}"}}"#),
            )
            .await;
        envelope(&r, 403, "host-not-allowed");
        assert!(r.header("set-cookie").is_empty());
    }

    let h = ws_headers(
        &evil_host,
        Some(&rig.origin()),
        Some(&s.cookie),
        Some(&s.protocol()),
    );
    match upgrade(rig.port, EVENTS, &h).await {
        Ok(_) => panic!("a rebinding upgrade was accepted"),
        Err(r) => {
            envelope(&r, 403, "host-not-allowed");
        }
    }

    envelope(
        &rig.get("/", &[("Host", evil_host.clone())]).await,
        403,
        "host-not-allowed",
    );
    envelope(
        &rig.get("/api/v1/no-such-route", &[("Host", evil_host.clone())])
            .await,
        403,
        "host-not-allowed",
    );

    // The refusals came before the exchanges: both codes still work.
    assert_eq!(rig.exchange(SESSION, &launch).await.status, 200);
    assert_eq!(rig.exchange(PAIR, pairing.expose()).await.status, 200);
}

/// Near-miss Host: another port, no port, a trailing dot, the IPv6
/// loopback and no Host at all are each refused `host-not-allowed` with a
/// valid session; the two allowed spellings pass.
#[tokio::test]
async fn near_miss_hosts_are_refused() {
    let rig = Rig::with_project().await;
    let s = rig.sign_in().await;
    let other = if rig.port == 65535 { 1 } else { rig.port + 1 };
    let near_misses = [
        format!("127.0.0.1:{other}"),
        format!("localhost:{other}"),
        "127.0.0.1".to_string(),
        "localhost".to_string(),
        format!("localhost.:{}", rig.port),
        format!("127.0.0.1.:{}", rig.port),
        format!("[::1]:{}", rig.port),
        format!("0.0.0.0:{}", rig.port),
        format!("127.0.0.2:{}", rig.port),
        format!("localhost:{}.evil.example", rig.port),
        format!("127.0.0.1:{}0", rig.port),
        format!("127.0.0.1:0{}", rig.port),
        // No Host header at all.
        String::new(),
    ];
    for host in near_misses {
        let mut h = s.auth();
        h.push(("Host", host.clone()));
        let r = rig.get(RUNS, &h).await;
        assert_eq!(
            r.status,
            403,
            "Host {host:?} was not refused:\n{}",
            r.text()
        );
        envelope(&r, 403, "host-not-allowed");
    }
    for host in [
        format!("127.0.0.1:{}", rig.port),
        format!("localhost:{}", rig.port),
    ] {
        let mut h = s.auth();
        h.push(("Host", host.clone()));
        let r = rig.get(RUNS, &h).await;
        assert_eq!(r.status, 200, "Host {host:?}:\n{}", r.text());
    }
}

/// Loopback only: the listener is 127.0.0.1, and a connection to a
/// non-loopback address of this Mac finds nothing listening.
#[tokio::test]
async fn the_server_listens_on_loopback_only() {
    let (listener, port) = am_server::bind(0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert_eq!(addr.ip(), std::net::IpAddr::from([127, 0, 0, 1]));
    let state = am_server::AppState::new(port);
    tokio::spawn(async move {
        let _ = am_server::serve(listener, state, std::future::pending()).await;
    });
    // This Mac's own outward address, found without sending a packet (a
    // UDP connect only picks a route). No route: nothing more to check.
    let Some(ip) = std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|u| u.connect("192.0.2.1:9").ok().map(|_| u))
        .and_then(|u| u.local_addr().ok())
        .map(|a| a.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
    else {
        eprintln!("no non-loopback address on this machine; listener checked only");
        return;
    };
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect((ip, port)),
    )
    .await;
    assert!(
        !matches!(r, Ok(Ok(_))),
        "the server answered on the non-loopback address {ip}"
    );
}

// ------------------------------------------- review patch round 1 (P5)

/// P5: an absolute-form request target naming a foreign authority
/// (`GET http://evil.example/api/v1/health`) is refused
/// `host-not-allowed`, even with the right Host header and a valid CLI
/// token.
#[tokio::test]
async fn an_absolute_form_target_with_a_foreign_authority_is_refused() {
    let rig = Rig::start().await;
    for target in [
        format!("http://evil.example{HEALTH}"),
        format!("http://evil.example:{}{HEALTH}", rig.port),
        format!("http://127.0.0.1:1{HEALTH}"),
    ] {
        let r = rig
            .send("GET", &target, &[(TOKEN_HEADER, rig.token())], "")
            .await;
        assert_eq!(r.status, 403, "{target}:\n{}", r.text());
        envelope(&r, 403, "host-not-allowed");
    }
}
