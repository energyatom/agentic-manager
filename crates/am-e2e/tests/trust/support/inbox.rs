//! Story 3.4's addition to 3.2's server rig (`support/server.rs`, which the
//! including test declares as `mod server`): a server on an open project
//! whose writer it holds (`AppStateBuilder::project_writer`), so the
//! Inbox's resolve route can record a decision and apply it; the Inbox
//! paths; a resolve with the full browser session; and a reader of the
//! event stream until an event shows. No network beyond 127.0.0.1.
//!
//! Review round 1 (R1): the server refuses an approve or a return unless the
//! project's config is the snapshot the run pinned. It asks am-app through
//! a port in its state, `am_server::PinCheck`, given with
//! `AppStateBuilder::pin_check`; the rig hands it [`FakePin`], whose current
//! snapshot a test sets (a hash, or the words of a config error).
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_api::EventDto;
use am_domain::{Clock, RuntimeLayout};
use am_server::{AppState, FixtureRoute, PinCheck, bind, router_with_fixture, serve_router};
use am_store::Store;
use am_testkit::FakeClock;

use crate::server::{Resp, Rig, Session, T0, Ws, private_dir};

pub const INBOX: &str = "/api/v1/inbox";

/// `GET /api/v1/inbox/{id}`.
pub fn item_path(id: &str) -> String {
    format!("{INBOX}/{id}")
}

/// `POST /api/v1/inbox/{id}/resolve`.
pub fn resolve_path(id: &str) -> String {
    format!("{INBOX}/{id}/resolve")
}

/// The snapshot hash the rig's runs pin (the engine support's `HASH`).
pub const PINNED: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
/// Another snapshot's hash.
pub const OTHER: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

/// The project's config as the server's pin check sees it (R1): the hash
/// of the snapshot its files give now, or why no run may go on on it (an
/// outside change, a pending config migration, a config error) in words.
pub struct FakePin(Mutex<Result<String, String>>);

impl FakePin {
    /// A config whose snapshot hash is `now`.
    pub fn new(now: &str) -> Arc<FakePin> {
        Arc::new(FakePin(Mutex::new(Ok(now.to_string()))))
    }

    pub fn set(&self, now: Result<String, String>) {
        *self.0.lock().unwrap() = now;
    }
}

impl PinCheck for FakePin {
    fn current_snapshot(&self) -> Result<String, String> {
        self.0.lock().unwrap().clone()
    }
}

/// 3.2's rig with an open project (an empty store in a 0700 runtime
/// folder) whose writer the server holds as well as its path, and a pin
/// check that finds the config the rig's runs pinned ([`PINNED`]).
pub async fn with_project_writer() -> Rig {
    with_pin(Some(FakePin::new(PINNED))).await
}

/// [`with_project_writer`] with `pin` as the server's pin check (none: the
/// server has no way to check the config, so approve and return fail
/// closed).
pub async fn with_pin(pin: Option<Arc<FakePin>>) -> Rig {
    with_pin_check(pin.map(|p| p as Arc<dyn PinCheck>)).await
}

/// [`with_pin`] with any pin check (the second-resolve race tests, 2026-11-25,
/// hand one that holds the requests reaching it).
pub async fn with_pin_check(pin: Option<Arc<dyn PinCheck>>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(T0);
    let (listener, port) = bind(0).await.unwrap();
    let rt = dir.path().join("rt");
    private_dir(&rt);
    let layout = RuntimeLayout::at(rt);
    let store = Store::open(&layout.state_db()).unwrap();
    let mut builder = AppState::builder(port)
        .clock(clock.clone() as Arc<dyn Clock>)
        .project_db(layout.state_db())
        .project_writer(store.writer());
    if let Some(pin) = pin {
        builder = builder.pin_check(pin);
    }
    let state = builder.build();
    let fixture = FixtureRoute::new();
    let router = router_with_fixture(state.clone(), fixture.clone());
    tokio::spawn(async move {
        let _ = serve_router(listener, router, std::future::pending()).await;
    });
    Rig {
        state,
        port,
        clock,
        fixture,
        store: Some(store),
        runtime: Some(layout),
        dir,
    }
}

/// `POST /api/v1/inbox/{id}/resolve` with `body`, the session's cookie and
/// CSRF value, and the served Origin (a UI-class request).
pub async fn resolve(rig: &Rig, s: &Session, id: &str, body: &str) -> Resp {
    let mut h = s.auth();
    h.push(("Origin", rig.origin()));
    rig.send("POST", &resolve_path(id), &h, body).await
}

/// Reads events into `seen` until one satisfies `done` (within 10 s);
/// panics, naming what was seen, when none does.
pub async fn events_until(ws: &mut Ws, seen: &mut Vec<EventDto>, done: impl Fn(&EventDto) -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(e) = ws.next_event(left).await else {
            panic!("the awaited event never came; seen: {seen:#?}");
        };
        let hit = done(&e);
        seen.push(e);
        if hit {
            return;
        }
    }
}
