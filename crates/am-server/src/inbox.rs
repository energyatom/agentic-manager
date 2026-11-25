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
        InboxKind::BudgetAsk => Some("4.3"),
        InboxKind::Proposal => Some("4.10"),
        // Story 3.21 (owner Q8): a Draft is dismissed, as a Failure is.
        InboxKind::Review | InboxKind::Question | InboxKind::Failure | InboxKind::Draft => None,
    }
}

/// The checks the API makes before it writes, from one committed state (the
/// caller's read transaction, with the item it read there): the item
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
            // Owner (2026-11-25): the item and the step status are read in
            // one transaction, not re-read, so the checks see one committed
            // state; it ends before the write, so a refused write reads back
            // what committed since.
            let prechecked = reader.snapshot(|reader| {
                let Some(item) = reader.inbox_item(&id)? else {
                    return Ok(Err(Refusal::NotFound));
                };
                #[cfg(test)]
                tests::after_item_read(&id);
                let Some(run_before) = reader.run(&item.run_id)? else {
                    return Ok(Err(Refusal::NotFound));
                };
                let decision = match check(reader, &item, &body) {
                    Ok(decision) => decision,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                if item.kind == InboxKind::Review
                    && matches!(decision, Decision::Approve | Decision::Return)
                    && let Err(refusal) = ready_to_go_on(reader, &item, pin.as_deref())?
                {
                    return Ok(Err(refusal));
                }
                Ok(Ok((item, run_before, decision)))
            })?;
            let (item, run_before, decision) = match prechecked {
                Ok(prechecked) => prechecked,
                Err(refusal) => return Ok(Err(refusal)),
            };
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

#[cfg(test)]
mod tests {
    //! Post-merge fix of story 3.4 (owner, 2026-11-25): the second-resolve
    //! race. `resolve` read the item, then the step's status in a separate
    //! read; a first resolve that committed between the two made a second
    //! request answer 409 `not-waiting` instead of `item-resolved`. The
    //! owner's fix: the item and the step status are read in one
    //! transaction, not re-read.
    //!
    //! The seam these tests need, test-only (the fix adds it): `resolve`'s
    //! blocking work calls [`after_item_read`] with the item's id right
    //! after it first reads the item, inside the read transaction its
    //! pre-checks share, under `#[cfg(test)]`:
    //!
    //! ```ignore
    //! #[cfg(test)]
    //! tests::after_item_read(&id);
    //! ```
    //!
    //! A test commits another request's decision there, so the race is
    //! reproduced without timing: the item read as open, the first resolve
    //! committed, then every later read. No store, no network beyond the
    //! system temp folder; the handler is called directly.
    use super::*;
    use am_domain::{Actor, Taint};
    use am_store::{
        AttemptWrites, CallPurpose, NewInboxItem, StepInput, Store, WriteCommand, new_id,
    };
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::Condvar;
    use std::time::Duration;

    type Hook = Box<dyn FnOnce() + Send>;

    /// The hooks a test set, by item id; each runs once, in the order set.
    static HOOKS: Mutex<Vec<(String, Hook)>> = Mutex::new(Vec::new());
    /// Every item id `resolve` reported reading, in order.
    static READS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The seam: `resolve` calls this right after it first reads the item
    /// `id` (inside the read transaction its pre-checks share). It runs the
    /// first hook a test set for `id`, if any.
    pub(super) fn after_item_read(id: &str) {
        lock(&READS).push(id.to_string());
        let hook = {
            let mut hooks = lock(&HOOKS);
            hooks
                .iter()
                .position(|(k, _)| k == id)
                .map(|i| hooks.remove(i).1)
        };
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Runs `hook` once, the next time a resolve of `id` has read the item.
    fn on_item_read(id: &str, hook: impl FnOnce() + Send + 'static) {
        lock(&HOOKS).push((id.to_string(), Box::new(hook)));
    }

    /// How many times a resolve of `id` reported reading the item.
    fn reads_of(id: &str) -> usize {
        lock(&READS).iter().filter(|r| r.as_str() == id).count()
    }

    const SEAM: &str = "the seam is missing: `resolve` must call `tests::after_item_read(&id)` right after it first reads the item (inside the read transaction its pre-checks share)";

    /// The snapshot hash the rig's run pinned, and what its pin check finds.
    const PINNED: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

    struct Pinned;

    impl PinCheck for Pinned {
        fn current_snapshot(&self) -> Result<String, String> {
            Ok(PINNED.into())
        }
    }

    fn engine() -> Actor {
        Actor::System {
            component: "engine".into(),
        }
    }

    fn note_of(decision: Decision) -> Option<String> {
        (decision == Decision::Return).then(|| "tighten it".to_string())
    }

    fn body_of(decision: Decision) -> String {
        match note_of(decision) {
            Some(note) => serde_json::json!({"decision": decision.as_str(), "note": note}),
            None => serde_json::json!({"decision": decision.as_str()}),
        }
        .to_string()
    }

    /// A store with one run of `make`, `check` and `next`: `make` succeeded,
    /// the gate `check` waits with an open, clean Review item on its output
    /// (`max_returns` 2); the run pinned [`PINNED`]. The server's state
    /// holds the store's path, its writer and a pin check finding the pin.
    struct Rig {
        dir: PathBuf,
        db: PathBuf,
        store: Store,
        state: AppState,
        run: String,
        item: String,
        gate: String,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn rig() -> Rig {
        let dir = std::env::temp_dir().join(format!("am-server-inbox-race-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let db = dir.join("state.db");
        let store = Store::open(&db).unwrap();
        let w = store.writer();
        let run = new_id();
        w.exec(
            engine(),
            WriteCommand::CreateRun {
                run_id: run.clone(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["make".into(), "check".into(), "next".into()],
                config_hash: Some(PINNED.into()),
            },
        )
        .unwrap();
        let steps = store.reader().unwrap().steps(&run).unwrap();
        let (make, gate) = (steps[0].id.clone(), steps[1].id.clone());
        w.exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: make.clone(),
                inputs: vec![],
                taint: Taint::Clean,
                prompt: vec![],
                writes: AttemptWrites {
                    seat: "seat".into(),
                    table: None,
                    document: None,
                },
            },
        )
        .unwrap();
        let call = new_id();
        w.exec(
            engine(),
            WriteCommand::ReserveCall {
                step_run_id: make.clone(),
                call_id: call.clone(),
                call_seq: 1,
                price: am_domain::money::CallPrice::local(),
                rate: am_domain::money::Rate::none("EUR"),
                counted_input_tokens: 0,
                max_output_tokens: 0,
                week: "2026-11-23".into(),
                team: "core".into(),
                taint: Taint::Clean,
                purpose: CallPurpose::Turn,
                prompt: vec![],
            },
        )
        .unwrap();
        w.exec(
            engine(),
            WriteCommand::SettleAndFinishStep {
                call_id: call,
                usage: None,
                step_run_id: make.clone(),
                status: "succeeded".into(),
                output: Some("the output to review".into()),
            },
        )
        .unwrap();
        let item = new_id();
        w.exec(
            engine(),
            WriteCommand::PauseStep {
                step_run_id: gate.clone(),
                inputs: vec![StepInput {
                    kind: "step_output".into(),
                    from_step_run: Some(make.clone()),
                    from_item: None,
                    from_row: None,
                    label: "step:make@1".into(),
                    taint: Taint::Clean,
                }],
                taint: Taint::Clean,
                item: NewInboxItem {
                    id: item.clone(),
                    kind: InboxKind::Review,
                    payload: Some(make),
                    taint: Taint::Clean,
                    title: "Review before step check goes on".into(),
                    body: "Step `check` waits for your review of the output of step `make`.".into(),
                    max_returns: Some(2),
                },
            },
        )
        .unwrap();
        let state = AppState::builder(0)
            .project_db(db.clone())
            .project_writer(store.writer())
            .pin_check(Arc::new(Pinned))
            .build();
        Rig {
            dir,
            db,
            store,
            state,
            run,
            item,
            gate,
        }
    }

    /// The resolve route's handler, called directly: its status and its
    /// JSON body.
    async fn post(state: &AppState, id: &str, decision: Decision) -> (u16, Value) {
        let response = resolve(
            State(state.clone()),
            Path(id.to_string()),
            Bytes::from(body_of(decision)),
        )
        .await;
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let body = serde_json::from_slice(&bytes).expect("a JSON body");
        (status, body)
    }

    fn code(body: &Value) -> &str {
        body["code"].as_str().unwrap_or("")
    }

    impl Rig {
        /// Another request's decision, as its handler records it: the one
        /// Human-only command, and then (when `applied`) the engine's apply.
        fn commit(&self, decision: Decision, applied: bool) -> impl FnOnce() + Send + 'static {
            let (w, db, item) = (self.store.writer(), self.db.clone(), self.item.clone());
            move || {
                w.resolve_inbox_item(
                    HumanActor::new("owner", REVIEWER_HAT),
                    &item,
                    decision,
                    note_of(decision),
                )
                .expect("the first request records its decision");
                if applied {
                    am_engine::gate::apply_resolution(&w, &Reader::open(&db).unwrap(), &item)
                        .expect("the first request applies it");
                }
            }
        }

        fn audit(&self) -> Vec<Value> {
            let mut out = Vec::new();
            Reader::open(&self.db)
                .unwrap()
                .export_audit_jsonl(&mut out)
                .unwrap();
            String::from_utf8(out)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect()
        }

        /// Exactly one resolution, `decision`'s: the item holds it, one
        /// `inbox.resolved` entry, the gate attempt moved once (one
        /// `step.finished` or `step.looped_back` entry on it) to what the
        /// decision gives, no other item.
        fn assert_one_resolution(&self, decision: Decision, case: &str) {
            let reader = Reader::open(&self.db).unwrap();
            let item = reader.inbox_item(&self.item).unwrap().unwrap();
            assert_eq!(item.status, "resolved", "{case}");
            assert_eq!(item.decision, Some(decision), "{case}");
            let audit = self.audit();
            let resolved: Vec<&Value> = audit
                .iter()
                .filter(|e| e["action"] == "inbox.resolved" && e["target"] == self.item.as_str())
                .collect();
            assert_eq!(
                resolved.len(),
                1,
                "one inbox.resolved entry ({case}): {resolved:#?}"
            );
            assert_eq!(resolved[0]["args"]["decision"], decision.as_str(), "{case}");
            let moves: Vec<&Value> = audit
                .iter()
                .filter(|e| {
                    e["target"] == self.gate.as_str()
                        && (e["action"] == "step.finished" || e["action"] == "step.looped_back")
                })
                .collect();
            assert_eq!(
                moves.len(),
                1,
                "the gate is applied once ({case}): {moves:#?}"
            );
            let steps = reader.steps(&self.run).unwrap();
            let shape: Vec<(String, i64, String)> = steps
                .iter()
                .map(|s| (s.step_key.clone(), s.attempt, s.status.clone()))
                .collect();
            let want: &[(&str, i64, &str)] = match decision {
                Decision::Approve => &[
                    ("make", 1, "succeeded"),
                    ("check", 1, "succeeded"),
                    ("next", 1, "queued"),
                ],
                Decision::Reject => &[
                    ("make", 1, "succeeded"),
                    ("check", 1, "failed"),
                    ("next", 1, "queued"),
                ],
                _ => &[
                    ("make", 1, "succeeded"),
                    ("make", 2, "queued"),
                    ("check", 1, "looped_back"),
                    ("check", 2, "queued"),
                    ("next", 1, "queued"),
                ],
            };
            let want: Vec<(String, i64, String)> = want
                .iter()
                .map(|(k, a, s)| (k.to_string(), *a, s.to_string()))
                .collect();
            assert_eq!(shape, want, "{case}");
            let run = reader.run(&self.run).unwrap().unwrap();
            let run_status = if decision == Decision::Reject {
                "failed"
            } else {
                "running"
            };
            assert_eq!(run.status, run_status, "{case}");
            assert_eq!(
                reader.inbox_items(&self.run).unwrap().len(),
                1,
                "no other item ({case})"
            );
        }
    }

    /// Who commits first (A) and who read the item while it was open (B).
    const PAIRS: [(Decision, Decision); 5] = [
        (Decision::Approve, Decision::Reject),
        (Decision::Reject, Decision::Approve),
        (Decision::Approve, Decision::Return),
        (Decision::Return, Decision::Approve),
        (Decision::Approve, Decision::Approve),
    ];

    /// The race, deterministic (owner, 2026-11-25): request B reads the
    /// item while it is open; request A's decision commits right then
    /// (recorded only, as A's handler is between its write and its apply;
    /// or recorded and applied); B goes on. B's answer is 409
    /// `item-resolved`, never `not-waiting` (nor `not-continuable` or a
    /// 400 from a read inside a stale snapshot), and A's decision alone is
    /// recorded and applied, once.
    #[tokio::test]
    async fn a_resolve_that_read_the_item_before_another_committed_answers_item_resolved() {
        for (a, b) in PAIRS {
            for a_applied in [false, true] {
                let case = format!(
                    "A {} ({}), then B {}",
                    a.as_str(),
                    if a_applied {
                        "recorded and applied"
                    } else {
                        "recorded"
                    },
                    b.as_str()
                );
                let r = rig();
                on_item_read(&r.item, r.commit(a, a_applied));

                let (status, body) = post(&r.state, &r.item, b).await;

                assert_eq!(reads_of(&r.item), 1, "{SEAM} ({case})");
                assert_ne!(
                    code(&body),
                    "not-waiting",
                    "the stale read ({case}): B read the item open, A committed, then B read the step"
                );
                assert_eq!(
                    (status, code(&body)),
                    (409, "item-resolved"),
                    "{case}: {body}"
                );
                if !a_applied {
                    let reader = Reader::open(&r.db).unwrap();
                    am_engine::gate::apply_resolution(&r.store.writer(), &reader, &r.item)
                        .expect("A's handler applies its decision after B's answer");
                }
                r.assert_one_resolution(a, &case);
            }
        }
    }

    /// A meeting point for `n` requests, with a 5 s limit (a request that
    /// never comes is recorded, not waited for forever).
    struct Meeting {
        n: usize,
        here: Mutex<usize>,
        cv: Condvar,
        late: Mutex<bool>,
    }

    impl Meeting {
        fn new(n: usize) -> Arc<Meeting> {
            Arc::new(Meeting {
                n,
                here: Mutex::new(0),
                cv: Condvar::new(),
                late: Mutex::new(false),
            })
        }

        fn arrive(&self) {
            let mut here = lock(&self.here);
            *here += 1;
            self.cv.notify_all();
            let (_here, waited) = self
                .cv
                .wait_timeout_while(here, Duration::from_secs(5), |h| *h < self.n)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if waited.timed_out() {
                *lock(&self.late) = true;
            }
        }
    }

    /// Both requests read the item while it is open, then both go on at
    /// once (owner: "both resolves' prechecks before either commit"), with
    /// approve vs reject, approve vs return and approve vs approve, many
    /// rounds: exactly one 200, the other 409 `item-resolved`; one decision
    /// recorded, the winner's, applied once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_resolves_that_both_read_the_item_open_record_one_decision() {
        for (a, b) in [
            (Decision::Approve, Decision::Reject),
            (Decision::Approve, Decision::Return),
            (Decision::Approve, Decision::Approve),
        ] {
            for round in 0..10 {
                let case = format!("{} and {}, round {round}", a.as_str(), b.as_str());
                let r = rig();
                let meeting = Meeting::new(2);
                for _ in 0..2 {
                    let m = meeting.clone();
                    on_item_read(&r.item, move || m.arrive());
                }

                let ((sa, ba), (sb, bb)) =
                    tokio::join!(post(&r.state, &r.item, a), post(&r.state, &r.item, b));

                assert_eq!(reads_of(&r.item), 2, "{SEAM} ({case})");
                assert!(!*lock(&meeting.late), "both reached the seam ({case})");
                let mut statuses = [sa, sb];
                statuses.sort_unstable();
                assert_eq!(statuses, [200, 409], "{case}: {ba} / {bb}");
                let (winner, lost) = if sa == 200 { (a, &bb) } else { (b, &ba) };
                assert_eq!(code(lost), "item-resolved", "{case}: {lost}");
                r.assert_one_resolution(winner, &case);
            }
        }
    }
}
