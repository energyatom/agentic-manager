//! RULE-15 trust test (story 3.2, AD-14): run status over `/api/v1`.
//! `GET /api/v1/runs` lists the open project's runs (`Vec<am_api::RunDto>`,
//! newest first); `GET /api/v1/runs/{id}` returns `am_api::RunDetailDto
//! {run, steps}`; both read through the project store's read-only
//! `Reader`. No project open: `no-project`; an unknown id: `not-found`.
//! Row: "Run status". Written first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use am_api::{RunDetailDto, RunDto};
use support::*;

/// Run status: `/runs` lists a story 3.1 run; `/runs/{id}` holds its
/// steps in order; the session and the CLI token both read them.
#[tokio::test]
async fn runs_lists_the_run_and_run_by_id_holds_its_steps() {
    let rig = Rig::with_project().await;
    let older = rig.create_run(&["x"]);
    let run = rig.create_run(&["a", "b"]);
    let s = rig.sign_in().await;

    for h in [s.auth(), vec![(TOKEN_HEADER, rig.token())]] {
        let r = rig.get(RUNS, &h).await;
        assert_eq!(r.status, 200, "{}", r.text());
        assert!(
            r.header("content-type")
                .join(",")
                .contains("application/json")
        );
        let runs: Vec<RunDto> = serde_json::from_str(&r.body).unwrap();
        let ids: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        assert!(
            ids.contains(&run.as_str()) && ids.contains(&older.as_str()),
            "{ids:?}"
        );
        let mine = runs.iter().find(|r| r.id == run).unwrap();
        assert_eq!(mine.status, "running");
        assert_eq!(mine.workflow, "wf-test");
        assert!(mine.finished_at.is_none());
        assert!(!mine.created_at.is_empty());

        let r = rig.get(&format!("{RUNS}/{run}"), &h).await;
        assert_eq!(r.status, 200, "{}", r.text());
        let detail: RunDetailDto = serde_json::from_str(&r.body).unwrap();
        assert_eq!(detail.run.id, run);
        let keys: Vec<&str> = detail.steps.iter().map(|s| s.step_key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert!(detail.steps.iter().all(|s| s.run_id == run));
        assert!(detail.steps.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    // camelCase JSON (AD-14).
    let r = rig.get(&format!("{RUNS}/{run}"), &s.auth()).await;
    assert!(
        r.body.contains("\"stepKey\"") && r.body.contains("\"createdAt\""),
        "{}",
        r.body
    );
}

/// No project open: both routes answer the envelope `no-project` (409).
#[tokio::test]
async fn with_no_project_open_runs_answer_no_project() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    envelope(&rig.get(RUNS, &s.auth()).await, 409, "no-project");
    envelope(
        &rig.get(&format!("{RUNS}/{}", am_store::new_id()), &s.auth())
            .await,
        409,
        "no-project",
    );
}

/// An unknown run id: the envelope `not-found` (404).
#[tokio::test]
async fn an_unknown_run_id_is_not_found() {
    let rig = Rig::with_project().await;
    rig.create_run(&["a"]);
    let r = rig
        .get(
            &format!("{RUNS}/{}", am_store::new_id()),
            &[(TOKEN_HEADER, rig.token())],
        )
        .await;
    envelope(&r, 404, "not-found");
}

/// The run routes only read: a POST, PUT or DELETE to them is 405 and the
/// run is unchanged.
#[tokio::test]
async fn the_run_routes_only_read() {
    let rig = Rig::with_project().await;
    let run = rig.create_run(&["a"]);
    let s = rig.sign_in().await;
    let mut h = s.auth();
    h.push(("Origin", rig.origin()));
    for m in ["POST", "PUT", "DELETE"] {
        for path in [RUNS.to_string(), format!("{RUNS}/{run}")] {
            envelope(
                &rig.send(m, &path, &h, "{}").await,
                405,
                "method-not-allowed",
            );
        }
    }
    let r = rig.get(&format!("{RUNS}/{run}"), &s.auth()).await;
    let detail: RunDetailDto = serde_json::from_str(&r.body).unwrap();
    assert_eq!(detail.run.status, "running");
}
