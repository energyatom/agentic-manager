//! The one error envelope on the wire (AD-14): every error response is JSON
//! `am_api::ApiError` with a fitting status. No secret goes into any field:
//! every message here is fixed words.

use am_api::ApiError;
use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// A JSON response of `value` with `status`.
pub(crate) fn json<T: serde::Serialize>(status: StatusCode, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(bytes) => Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The envelope with `status`, `code` and `message`, and `fix` when given.
pub(crate) fn error(status: StatusCode, code: &str, message: &str, fix: Option<&str>) -> Response {
    let mut e = ApiError::new(code, message);
    if let Some(fix) = fix {
        e = e.with_fix(fix);
    }
    json(status, &e)
}

pub(crate) fn host_not_allowed() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "host-not-allowed",
        "This server answers only requests addressed to 127.0.0.1 or localhost on its own port.",
        Some("Open the address `agentic serve` printed."),
    )
}

pub(crate) fn origin_not_allowed() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "origin-not-allowed",
        "Requests from another site or port are refused.",
        None,
    )
}

pub(crate) fn origin_required() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "origin-required",
        "This request must come from the app's own page, which sends its Origin.",
        None,
    )
}

pub(crate) fn not_found() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "not-found",
        "There is nothing at this address.",
        None,
    )
}

pub(crate) fn method_not_allowed() -> Response {
    error(
        StatusCode::METHOD_NOT_ALLOWED,
        "method-not-allowed",
        "This address does not take this method; a change is only ever a POST, PUT or DELETE.",
        None,
    )
}

pub(crate) fn session_required() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "session-required",
        "Sign in first: open the app from `agentic serve`, or enter its pairing code.",
        Some("Run `agentic serve` and open the address it prints."),
    )
}

pub(crate) fn csrf_required() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "csrf-required",
        "The session's CSRF value is missing or wrong; a cookie alone authenticates nothing.",
        Some("Reload the app's page to sign in again."),
    )
}

pub(crate) fn cli_token_with_origin() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "cli-token-with-origin",
        "The CLI token is never accepted on a request that carries an Origin.",
        None,
    )
}

pub(crate) fn ui_class_only() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "ui-class-only",
        "Only the app's own page may do this; the CLI token cannot.",
        Some("Do it in the app."),
    )
}

pub(crate) fn cli_token_invalid() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "cli-token-invalid",
        "The CLI token is not this server's; each launch has its own.",
        Some("Read the token file of the running `agentic serve`."),
    )
}

pub(crate) fn bad_request(message: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "bad-request", message, None)
}

pub(crate) fn launch_code_invalid() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "launch-code-invalid",
        "This sign-in link has expired or was already used.",
        Some("Run `agentic serve` again, or enter the pairing code it prints."),
    )
}

pub(crate) fn pairing_code_invalid() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "pairing-code-invalid",
        "This pairing code is wrong, expired or already used.",
        Some("Press Enter in the `agentic serve` terminal for a new code."),
    )
}

pub(crate) fn pairing_rate_limited() -> Response {
    error(
        StatusCode::TOO_MANY_REQUESTS,
        "pairing-rate-limited",
        "Too many pairing attempts in the last minute.",
        Some("Wait a minute, then try again."),
    )
}

pub(crate) fn pairing_locked() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "pairing-locked",
        "Pairing is locked after too many wrong codes, until `agentic serve` restarts.",
        Some("Restart `agentic serve`."),
    )
}

pub(crate) fn no_project() -> Response {
    error(
        StatusCode::CONFLICT,
        "no-project",
        "No project is open.",
        Some("Run `agentic serve --project <folder>`."),
    )
}

pub(crate) fn item_resolved() -> Response {
    error(
        StatusCode::CONFLICT,
        "item-resolved",
        "This Inbox item was resolved already; an item is resolved once.",
        Some("Read the item to see who resolved it, and when."),
    )
}

pub(crate) fn not_waiting() -> Response {
    error(
        StatusCode::CONFLICT,
        "not-waiting",
        "This item's step is not waiting for a person, so there is nothing to resolve.",
        Some("Run `agentic status` to see the run's steps."),
    )
}

pub(crate) fn decision_not_allowed(message: &str) -> Response {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "decision-not-allowed",
        message,
        Some(
            "A Review is approved, returned or rejected; a Question is answered; a Failure is dismissed.",
        ),
    )
}

pub(crate) fn note_required() -> Response {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "note-required",
        "A return needs its note, and an answer its text: one line of 1 to 2000 characters.",
        Some("Send `note` with the decision."),
    )
}

pub(crate) fn confirm_required() -> Response {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "confirm-required",
        "This Review's output came from untrusted content, so approving it needs the item's id typed back.",
        Some("Send `confirmItemId` equal to the item's id."),
    )
}

pub(crate) fn config_changed(message: &str) -> Response {
    error(
        StatusCode::CONFLICT,
        "config-changed",
        message,
        Some(
            "Put the project's files back as they were when the run started, or reject the item; reject and dismiss still work.",
        ),
    )
}

pub(crate) fn not_continuable(message: &str) -> Response {
    error(
        StatusCode::CONFLICT,
        "not-continuable",
        message,
        Some("Run `agentic status` to see the run; resume arrives with story 3.9."),
    )
}

pub(crate) fn upgrade_required() -> Response {
    error(
        StatusCode::UPGRADE_REQUIRED,
        "upgrade-required",
        "The event stream is a WebSocket; open it with an upgrade request.",
        None,
    )
}

pub(crate) fn store_unreadable() -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "store-unreadable",
        "The project's store could not be read.",
        Some("Run `agentic status` to see the store's own words."),
    )
}

pub(crate) fn internal() -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "The server could not answer this request.",
        None,
    )
}
