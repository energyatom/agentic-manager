//! The test-only UI-class fixture route (story 3.2, AD-15): `POST
//! /api/v1/fixture`, a state change that only counts, proves the UI-class
//! rules (the CLI token refused, a matching Origin plus the CSRF value
//! needed, GET and HEAD refused) while no shipped route is UI class yet.
//!
//! It is added only by [`router_with_fixture`], a constructor parameter:
//! never a feature, env var or flag. `cargo xtask seam-guard` fails if any
//! shipped source outside this file names `router_with_fixture` or
//! `FixtureRoute`, so the release binary cannot serve it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::http::StatusCode;

use crate::envelope;
use crate::routes::{Route, RouteClass, RouteSpec, build};
use crate::state::AppState;

/// The fixture route's counter (clones share it).
#[derive(Debug, Clone, Default)]
pub struct FixtureRoute {
    count: Arc<AtomicU64>,
}

impl FixtureRoute {
    pub fn new() -> FixtureRoute {
        FixtureRoute::default()
    }

    /// How many times the route changed state.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }

    /// `POST /api/v1/fixture`, UI class, changes state.
    pub fn spec() -> RouteSpec {
        RouteSpec::new("POST", "/api/v1/fixture", RouteClass::UiClass, true)
    }
}

/// The shipped router plus the fixture route. Tests only.
pub fn router_with_fixture(state: AppState, fixture: FixtureRoute) -> Router {
    let spec = FixtureRoute::spec();
    let count = fixture.count.clone();
    let route = Route::change(&spec.path, spec.class, move || {
        let count = count.clone();
        async move {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            envelope::json(StatusCode::OK, &serde_json::json!({ "count": n }))
        }
    });
    build(state, vec![route])
}
