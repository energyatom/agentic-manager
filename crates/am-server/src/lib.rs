//! `am-server`: the loopback HTTP and WebSocket server (AD-13, AD-14,
//! AD-15).
//!
//! - binds 127.0.0.1 only, with no option to change it;
//! - one guard decides every request ([`guard`](crate::guard)): the Host
//!   must be exactly `127.0.0.1:<port>` or `localhost:<port>` (DNS
//!   rebinding), an Origin, when present, must be the served origin, then
//!   the route table ([`route_table`]) and the route's class ([`RouteClass`]);
//! - a single-use launch code (60 s, URL fragment) or the terminal pairing
//!   code (8 base32 characters, 60 s, single use; [`print_pairing_code`],
//!   [`pairing_terminal`]) is exchanged for an HttpOnly, SameSite=Strict
//!   cookie named for the port (`am_s_<port>`) plus a per-session CSRF value
//!   returned in the body and held in page memory;
//! - UI-class routes need the cookie and the `X-AM-CSRF` header (the
//!   WebSocket upgrade carries the value as the `am.csrf.<v>` subprotocol);
//!   CLI-class routes also take the per-launch [`CliToken`] in
//!   `X-AM-CLI-Token` on a request with no Origin;
//! - every error is the one envelope, `am_api::ApiError`; no CORS headers
//!   are ever sent;
//! - run status from the open project's store, read-only, and the event
//!   stream ([`EventLog`], [`watch_runs`]) with `seq` and `since=`;
//! - story 3.4: the Inbox (`/api/v1/inbox`, reads in CLI class; a resolve
//!   in UI class), which records a person's decision through the store's
//!   one Human-only command and applies it as `System(engine)`; it runs no
//!   model step. Each Inbox change is one `inbox.updated` event carrying
//!   only the item's id and status.
//!
//! Session ids, CSRF values, launch and pairing codes and the CLI token are
//! held in memory only, compared in constant time, and never logged.
#![forbid(unsafe_code)]

mod envelope;
mod events;
mod fixture;
mod guard;
mod inbox;
mod pairing;
mod routes;
mod runs;
mod secret;
mod state;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;

pub use events::{EventLog, watch_runs};
pub use fixture::*;
pub use inbox::PinCheck;
pub use pairing::{PairingCode, PairingLocked, pairing_terminal, print_pairing_code};
pub use routes::{RouteClass, RouteSpec, route_table, router};
pub use secret::CliToken;
pub use state::{AppState, AppStateBuilder};

pub const LAUNCH_CODE_TTL: Duration = Duration::from_secs(60);
pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(60);
/// Counted pairing attempts allowed in any [`PAIRING_RATE_WINDOW`].
pub const PAIRING_RATE_LIMIT: usize = 5;
pub const PAIRING_RATE_WINDOW: Duration = Duration::from_secs(60);
/// Pairing locks after this many failures, until restart.
pub const PAIRING_LOCK_AFTER: u32 = 10;
/// How many events the log holds for `since=` replay.
pub const EVENT_WINDOW: usize = 10_000;
pub const CSRF_HEADER: &str = "x-am-csrf";
pub const CLI_TOKEN_HEADER: &str = "x-am-cli-token";
pub const WS_PROTOCOL_PREFIX: &str = "am.csrf.";

const EVENTS_PATH: &str = "/api/v1/events";

/// Bind 127.0.0.1 (port 0 picks a free port).
pub async fn bind(port: u16) -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Serve the shipped router until the future completes.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_router(listener, router(state), shutdown).await
}

/// Serve `router` until the future completes.
pub async fn serve_router(
    listener: TcpListener,
    router: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}
