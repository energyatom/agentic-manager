//! Run status over `/api/v1` (AD-14): `GET /api/v1/runs` lists the open
//! project's runs, newest first; `GET /api/v1/runs/{id}` returns one run
//! with its steps. Both read through the project store's read-only
//! `am_store::Reader` (story 3.1's rows). No project open: `no-project`;
//! an unknown id: `not-found`.

use am_api::{RunDetailDto, RunDto, StepDto};
use am_store::{Reader, RunRow, StepRow};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;

use crate::envelope;
use crate::state::AppState;

pub(crate) fn run_dto(r: RunRow) -> RunDto {
    RunDto {
        id: r.id,
        workflow: r.workflow,
        status: r.status,
        created_at: r.created_at,
        finished_at: r.finished_at,
    }
}

pub(crate) fn step_dto(run_id: &str, s: StepRow) -> StepDto {
    StepDto {
        id: s.id,
        run_id: run_id.to_string(),
        step_key: s.step_key,
        seq: s.seq,
        status: s.status,
        started_at: s.started_at,
        finished_at: s.finished_at,
    }
}

/// Runs `read` on a fresh read-only connection to the open project's
/// store, off the async threads.
pub(crate) async fn read<T, F>(state: &AppState, read: F) -> Result<T, fn() -> Response>
where
    T: Send + 'static,
    F: FnOnce(&Reader) -> Result<T, am_store::StoreError> + Send + 'static,
{
    let Some(db) = state.project_db() else {
        return Err(envelope::no_project);
    };
    match tokio::task::spawn_blocking(move || {
        let reader = Reader::open(&db)?;
        read(&reader)
    })
    .await
    {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "the project's store could not be read");
            Err(envelope::store_unreadable)
        }
        Err(_) => Err(envelope::internal),
    }
}

pub(crate) async fn list(State(state): State<AppState>) -> Response {
    match read(&state, |r| r.runs()).await {
        Ok(rows) => {
            let runs: Vec<RunDto> = rows.into_iter().map(run_dto).collect();
            envelope::json(StatusCode::OK, &runs)
        }
        Err(refused) => refused(),
    }
}

pub(crate) async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let found = read(&state, move |r| {
        let Some(run) = r.run(&id)? else {
            return Ok(None);
        };
        let steps = r.steps(&run.id)?;
        Ok(Some((run, steps)))
    })
    .await;
    match found {
        Ok(Some((run, steps))) => {
            let steps = steps.into_iter().map(|s| step_dto(&run.id, s)).collect();
            envelope::json(
                StatusCode::OK,
                &RunDetailDto {
                    run: run_dto(run),
                    steps,
                },
            )
        }
        Ok(None) => envelope::not_found(),
        Err(refused) => refused(),
    }
}
