//! The Inbox over `/api/v1` (epic 3 story 4; AD-13, AD-14, AD-15, AD-28):
//!
//! - `GET /api/v1/inbox` (CLI class): the open items of the open project;
//! - `GET /api/v1/inbox/{id}` (CLI class): one item, its payload's text and,
//!   once resolved, the note it carried;
//! - `POST /api/v1/inbox/{id}/resolve {decision, note?, confirmItemId?}`
//!   (UI class, Question 4): a person's decision, recorded as You
//!   (Reviewer) through the store's one Human-only command, then applied by
//!   the server as `System(engine)` (`am_engine::gate::apply_resolution`).
//!   It runs no model step: a run it leaves ready goes on with story 3.9's
//!   resume. A tainted Review's approve needs `confirmItemId` equal to the
//!   item's id (owner condition 2), as `agentic gate` needs it typed.
//!
//! Review round 1: an approve or a return is refused before anything is
//! written while the project's config is not the snapshot the run pinned
//! (R1: [`PinCheck`], which am-app hands the server; none fails closed),
//! or while the run could not go on (R2: `am_engine::run::ready_to_go_on`).
//! An apply that fails after the person's decision committed answers 200
//! with `applied: false`, never 500.
//!
//! Every refusal is AD-14's envelope and changes nothing: `not-found`,
//! `no-project`, `item-resolved`, `not-waiting`, `decision-not-allowed`,
//! `note-required`, `confirm-required` (Decision 6), `config-changed` and
//! `not-continuable` (review round 1).

use am_api::{InboxItemDetailDto, InboxItemDto, ResolutionDto, ResolveBody};
use am_domain::{Decision, HumanActor, InboxKind, REVIEWER_HAT, StepStatus};
use am_store::{InboxItemRow, Reader, StoreError};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use std::sync::{Arc, Mutex};

use crate::envelope;
use crate::runs::{read, run_dto};
use crate::state::AppState;

/// Story 3.4, review R1 (AD-16, Question 2): the project's config as a run
/// would go on on it. am-app gives the server one (`agentic gate`'s check
/// 5): the hash of the snapshot the project's files give now, or why no
/// run may go on on them (an outside change, a pending config migration, a
/// config error), in words.
pub trait PinCheck: Send + Sync {
    fn current_snapshot(&self) -> Result<String, String>;
}

/// An item as the API shows it: who resolved it in words, never the note.
pub(crate) fn item_dto(item: InboxItemRow) -> InboxItemDto {
    let resolved_by = item.resolved_by.as_ref().map(|_| {
        item.resolver()
            .map(|actor| actor.describe())
            .unwrap_or_else(|| "an unknown resolver".into())
    });
    InboxItemDto {
        id: item.id,
        kind: item.kind.as_str().to_string(),
        status: item.status,
        run_id: item.run_id,
        step_run_id: item.step_run_id,
        taint: item.taint.is_tainted(),
        title: item.title,
        body: item.body,
        created_at: item.created_at,
        max_returns: item.max_returns,
        decision: item.decision.map(|d| d.as_str().to_string()),
        resolved_by,
        resolved_at: item.resolved_at,
    }
}

/// `GET /api/v1/inbox`: every open item of the open project.
pub(crate) async fn list(State(state): State<AppState>) -> Response {
    match read(&state, |r| r.open_items()).await {
        Ok(items) => {
            let items: Vec<InboxItemDto> = items.into_iter().map(item_dto).collect();
            envelope::json(StatusCode::OK, &items)
        }
        Err(refused) => refused(),
    }
}

/// `GET /api/v1/inbox/{id}`: one item, with its payload's text.
pub(crate) async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let found = read(&state, move |r| {
        let Some(item) = r.inbox_item(&id)? else {
            return Ok(None);
        };
        let payload = match &item.payload {
            Some(step_run) => r
                .steps(&item.run_id)?
                .into_iter()
                .find(|s| &s.id == step_run)
                .and_then(|s| s.output),
            None => None,
        };
        Ok(Some((item, payload)))
    })
    .await;
    match found {
        Ok(Some((item, payload))) => {
            let note = item.note.clone();
            envelope::json(
                StatusCode::OK,
                &InboxItemDetailDto {
                    item: item_dto(item),
                    payload,
                    note,
                },
            )
        }
        Ok(None) => envelope::not_found(),
        Err(refused) => refused(),
    }
}

/// Why a resolve was refused before or by the store.
enum Refusal {
    NotFound,
    Resolved,
    NotWaiting,
    NotAllowed(String),
    NoteRequired,
    ConfirmRequired,
    ConfigChanged(String),
    NotContinuable(String),
    Bad(String),
}

impl Refusal {
    fn response(self) -> Response {
        match self {
            Refusal::NotFound => envelope::not_found(),
            Refusal::Resolved => envelope::item_resolved(),
            Refusal::NotWaiting => envelope::not_waiting(),
            Refusal::NotAllowed(words) => envelope::decision_not_allowed(&words),
            Refusal::NoteRequired => envelope::note_required(),
            Refusal::ConfirmRequired => envelope::confirm_required(),
            Refusal::ConfigChanged(words) => envelope::config_changed(&words),
            Refusal::NotContinuable(words) => envelope::not_continuable(&words),
            Refusal::Bad(words) => envelope::bad_request(&words),
        }
    }
}

/// The story a kind's decisions arrive with, for a kind this version does
/// not resolve (Question 3).
fn later_story(kind: InboxKind) -> Option<&'static str> {
    match kind {
        InboxKind::Approval => Some("4.8"),
        InboxKind::Draft => Some("3.6"),
        InboxKind::BudgetAsk => Some("4.3"),
        InboxKind::Proposal => Some("4.10"),
        InboxKind::Review | InboxKind::Question | InboxKind::Failure => None,
    }
}

/// The checks the API makes before it writes, from committed rows: the item
/// exists and is open, the decision fits its kind, a return or answer has
/// its text, a tainted approve has the id typed back, and the step waits.
fn check(reader: &Reader, item: &InboxItemRow, body: &ResolveBody) -> Result<Decision, Refusal> {
    if item.status == "resolved" {
        return Err(Refusal::Resolved);
    }
    let Some(decision) = Decision::parse(&body.decision) else {
        return Err(Refusal::Bad(
            "`decision` is one of approve, return, reject, answer or dismiss.".into(),
        ));
    };
    if let Some(story) = later_story(item.kind) {
        return Err(Refusal::NotAllowed(format!(
            "A {} item is resolved from story {story} on, not in this version.",
            item.kind.title()
        )));
    }
    if !Decision::allowed_for(item.kind).contains(&decision) {
        let names: Vec<&str> = Decision::allowed_for(item.kind)
            .iter()
            .map(|d| d.as_str())
            .collect();
        return Err(Refusal::NotAllowed(format!(
            "A {} item takes {}, not `{}`.",
            item.kind.title(),
            names.join(", "),
            decision.as_str()
        )));
    }
    match (decision.takes_note(), body.note.as_deref()) {
        (true, None) => return Err(Refusal::NoteRequired),
        (true, Some(note)) if note.trim().is_empty() => return Err(Refusal::NoteRequired),
        (false, Some(_)) => {
            return Err(Refusal::Bad(format!(
                "A decision `{}` carries no note.",
                decision.as_str()
            )));
        }
        _ => {}
    }
    // Owner condition 2: the API is no way around the typed id.
    if item.kind == InboxKind::Review
        && decision == Decision::Approve
        && item.taint.is_tainted()
        && body.confirm_item_id.as_deref() != Some(item.id.as_str())
    {
        return Err(Refusal::ConfirmRequired);
    }
    if matches!(item.kind, InboxKind::Review | InboxKind::Question) {
        let waiting = reader
            .steps(&item.run_id)
            .map_err(|_| Refusal::NotWaiting)?
            .into_iter()
            .any(|s| s.id == item.step_run_id && s.status == StepStatus::WaitingHuman.as_str());
        if !waiting {
            return Err(Refusal::NotWaiting);
        }
    }
    Ok(decision)
}

/// R1 and R2, for an approve or a return of a Review, before anything is
/// written: the project's config is still the snapshot the run pinned, and
/// the run could go on.
fn ready_to_go_on(
    reader: &Reader,
    item: &InboxItemRow,
    pin: Option<&dyn PinCheck>,
) -> Result<Result<(), Refusal>, StoreError> {
    let Some(run) = reader.run(&item.run_id)? else {
        return Ok(Err(Refusal::NotFound));
    };
    let pinned = run.config_hash.as_deref().unwrap_or("no snapshot");
    let Some(pin) = pin else {
        return Ok(Err(Refusal::ConfigChanged(
            "This server cannot check the project's config against the snapshot the run pinned, so approve and return are refused (fail closed). Nothing was written.".into(),
        )));
    };
    match pin.current_snapshot() {
        Err(why) => {
            return Ok(Err(Refusal::ConfigChanged(format!(
                "The project's config cannot be used to go on with run {}: {why}. Nothing was written.",
                run.id
            ))));
        }
        Ok(now) if Some(now.as_str()) != run.config_hash.as_deref() => {
            return Ok(Err(Refusal::ConfigChanged(format!(
                "The project's config is not the snapshot run {} pinned: the run pinned {pinned}, and the project's files now give {now}. A run goes on only on the config it started with (AD-16). Nothing was written.",
                run.id
            ))));
        }
        Ok(_) => {}
    }
    match am_engine::run::ready_to_go_on(reader, &item.run_id) {
        Ok(_) => Ok(Ok(())),
        Err(am_engine::run::RunError::NotContinuable(words)) => Ok(Err(Refusal::NotContinuable(
            format!("Run {}: {words}.", run.id),
        ))),
        Err(am_engine::run::RunError::Store(e)) => Err(e),
        Err(e) => Err(StoreError::Invalid(e.to_string())),
    }
}

/// `POST /api/v1/inbox/{id}/resolve`: the guard has checked the session,
/// its CSRF value and the Origin (UI class).
pub(crate) async fn resolve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(body) = serde_json::from_slice::<ResolveBody>(&body) else {
        return envelope::bad_request(
            "The body must be JSON {\"decision\": \"<decision>\", \"note\": \"<text>\"?, \"confirmItemId\": \"<id>\"?}.",
        );
    };
    let (Some(db), Some(writer)) = (state.project_db(), state.project_writer()) else {
        return envelope::no_project();
    };
    let pin = state.pin_check();
    // S6: once the decision is recorded, what the API answers if anything
    // after it fails (a read-back error, a missing row, a panic in the
    // task): 200, the item as read before with its new status, the run as
    // read before, `applied: false`.
    let recorded: Arc<Mutex<Option<ResolutionDto>>> = Arc::new(Mutex::new(None));
    let after_commit = recorded.clone();
    let done = tokio::task::spawn_blocking(
        move || -> Result<Result<ResolutionDto, Refusal>, StoreError> {
            let reader = Reader::open(&db)?;
            let Some(item) = reader.inbox_item(&id)? else {
                return Ok(Err(Refusal::NotFound));
            };
            let Some(run_before) = reader.run(&item.run_id)? else {
                return Ok(Err(Refusal::NotFound));
            };
            let decision = match check(&reader, &item, &body) {
                Ok(decision) => decision,
                Err(refusal) => return Ok(Err(refusal)),
            };
            if item.kind == InboxKind::Review
                && matches!(decision, Decision::Approve | Decision::Return)
                && let Err(refusal) = ready_to_go_on(&reader, &item, pin.as_deref())?
            {
                return Ok(Err(refusal));
            }
            // The one Human-only command (AD-15): the person is the owner as
            // Reviewer, whatever the request says.
            let recorded = writer.resolve_inbox_item(
                HumanActor::new("owner", REVIEWER_HAT),
                &id,
                decision,
                body.note.clone(),
            );
            if let Err(e) = recorded {
                // Decided from what committed: another resolve won, or the
                // store refused the note in its own words.
                let now = reader.inbox_item(&id)?;
                return match (now, e) {
                    (Some(now), _) if now.status == "resolved" => Ok(Err(Refusal::Resolved)),
                    (_, StoreError::Invalid(words)) => Ok(Err(Refusal::Bad(words))),
                    (_, e) => Err(e),
                };
            }
            // Recorded: from here the answer is 200 whatever fails (S6).
            let mut fallback = item_dto(item.clone());
            fallback.status = "resolved".into();
            fallback.decision = Some(decision.as_str().to_string());
            fallback.resolved_by = Some("You (Reviewer)".into());
            *after_commit
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ResolutionDto {
                item: fallback,
                run: run_dto(run_before),
                applied: false,
            });
            // The engine applies the decision as System(engine); the server
            // runs no model step (Question 1). The decision is recorded
            // whatever the apply does: a failed apply is `applied: false`
            // (R2), and the run waits for story 3.9's resume.
            let mut applied = true;
            if item.kind == InboxKind::Review
                && let Err(e) = am_engine::gate::apply_resolution(&writer, &reader, &id)
            {
                tracing::warn!(error = %e, "a recorded decision could not be applied");
                applied = false;
            }
            let item = reader
                .inbox_item(&id)?
                .ok_or_else(|| StoreError::Invalid(format!("Inbox item {id} is gone")))?;
            let run = reader
                .run(&item.run_id)?
                .ok_or_else(|| StoreError::Invalid(format!("run {} is gone", item.run_id)))?;
            Ok(Ok(ResolutionDto {
                item: item_dto(item),
                run: run_dto(run),
                applied,
            }))
        },
    )
    .await;
    let unapplied = || {
        recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    };
    match done {
        Ok(Ok(Ok(resolution))) => envelope::json(StatusCode::OK, &resolution),
        Ok(Ok(Err(refusal))) => refusal.response(),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "an Inbox resolve could not be completed");
            match unapplied() {
                Some(resolution) => envelope::json(StatusCode::OK, &resolution),
                None => envelope::internal(),
            }
        }
        Err(_) => match unapplied() {
            Some(resolution) => envelope::json(StatusCode::OK, &resolution),
            None => envelope::internal(),
        },
    }
}
