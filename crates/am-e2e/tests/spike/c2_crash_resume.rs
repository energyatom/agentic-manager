//! Criterion 2: `kill -9` during a step on SQLite WAL, restart, and the run
//! resumes at its last committed transition, losing only the in-flight model
//! call (marked lost, settled at its reservation, re-issued); ledger totals
//! reconcile.
//!
//! The child process is this test binary re-executed into [`child_entry`],
//! which runs `am_app::spike::spike_run` (the same code as
//! `agentic __spike-run`), so the test does not depend on the `agentic`
//! binary having been built first.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use am_store::Reader;

const CHILD_ENV: &str = "AM_E2E_C2_CHILD";
const STEPS: usize = 5;

/// Runs only inside the re-executed child; a no-op in a normal test run.
#[test]
fn child_entry() {
    let Ok(db) = std::env::var(CHILD_ENV) else {
        return;
    };
    let resume = std::env::var("AM_E2E_C2_RESUME").is_ok();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (run, status) = rt
        .block_on(am_app::spike::spike_run(
            Path::new(&db),
            STEPS,
            Duration::from_millis(600),
            resume,
        ))
        .unwrap();
    println!("run {run} {status}");
}

fn spawn_child(db: &Path, resume: bool) -> Child {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(exe);
    cmd.args([
        "--exact",
        "spike::c2_crash_resume::child_entry",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(CHILD_ENV, db);
    if resume {
        cmd.env("AM_E2E_C2_RESUME", "1");
    }
    cmd.spawn().unwrap()
}

#[test]
fn kill_9_mid_call_then_resume_loses_only_the_in_flight_call() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let mut child = spawn_child(&db, false);

    // Wait until step 3's model call is journaled and in flight.
    let start = Instant::now();
    let run_id = loop {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "child never reached step 3"
        );
        std::thread::sleep(Duration::from_millis(25));
        let Ok(reader) = Reader::open(&db) else {
            continue;
        };
        let Ok(runs) = reader.unfinished_runs() else {
            continue;
        };
        let Some(run) = runs.first() else { continue };
        let calls = reader.calls_for_run(&run.id).unwrap();
        if calls.len() == 3 && !calls[2].settled {
            break run.id.clone();
        }
    };
    // SIGKILL: no chance to clean up.
    child.kill().unwrap();
    child.wait().unwrap();

    let reader = Reader::open(&db).unwrap();
    let steps = reader.steps(&run_id).unwrap();
    let states: Vec<&str> = steps.iter().map(|s| s.status.as_str()).collect();
    assert_eq!(
        states,
        ["succeeded", "succeeded", "running", "queued", "queued"]
    );
    let calls = reader.calls_for_run(&run_id).unwrap();
    assert_eq!(calls.iter().filter(|c| !c.settled).count(), 1);
    drop(reader);

    // Restart: resume the unfinished run.
    let status = spawn_child(&db, true).wait().unwrap();
    assert!(status.success());

    let reader = Reader::open(&db).unwrap();
    assert_eq!(reader.run(&run_id).unwrap().unwrap().status, "succeeded");
    let steps = reader.steps(&run_id).unwrap();
    assert!(steps.iter().all(|s| s.status == "succeeded"));
    let calls = reader.calls_for_run(&run_id).unwrap();
    // One call per step, plus the one lost in flight and re-issued.
    assert_eq!(calls.len(), STEPS + 1);
    let lost: Vec<_> = calls.iter().filter(|c| c.status == "lost").collect();
    assert_eq!(lost.len(), 1);
    assert_eq!(lost[0].step_run_id, steps[2].id);
    assert_eq!(lost[0].call_seq, 1);
    let reissued = calls
        .iter()
        .find(|c| c.step_run_id == steps[2].id && c.call_seq == 2)
        .expect("re-issued call");
    assert_eq!(reissued.status, "settled");
    for s in &steps {
        let n = calls
            .iter()
            .filter(|c| c.step_run_id == s.id && c.status == "settled")
            .count();
        assert_eq!(n, 1, "step {} completed its call exactly once", s.step_key);
    }

    // The ledger reconciles: every entry settled; the lost call at its
    // reservation; the others from usage (100 in, 20 out at 3 and 15 USD/MTok).
    let ledger = reader.ledger_for_run(&run_id).unwrap();
    assert_eq!(ledger.len(), calls.len());
    assert!(ledger.iter().all(|l| l.state == "settled"));
    let wf = am_app::spike::spike_workflow(STEPS);
    // Story 3.7: am-domain's one money function prices the spike's calls.
    let per_call = am_domain::money::cost(&wf.price, &wf.rate, 100, 20)
        .unwrap()
        .usd_micro;
    let mut expected_total = 0;
    for l in &ledger {
        let call = calls.iter().find(|c| c.id == l.model_call_id).unwrap();
        let expected = if call.status == "lost" {
            l.reserved_micro_usd
        } else {
            per_call
        };
        assert_eq!(l.settled_micro_usd, Some(expected));
        expected_total += expected;
    }
    let total: i64 = ledger.iter().map(|l| l.settled_micro_usd.unwrap()).sum();
    assert_eq!(total, expected_total);
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}
