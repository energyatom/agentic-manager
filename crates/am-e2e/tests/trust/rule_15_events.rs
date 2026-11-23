//! RULE-15 trust test (story 3.2, AD-14): the event stream. `EventLog`
//! numbers each event of this launch with a monotonic `seq` from 1 and
//! holds the last N (10 000; injected smaller here); `GET
//! /api/v1/events?since=<seq>` upgrades, replays every held event after
//! `since` in order, then streams new ones; a `since` older than the
//! window first gets one `resync`. A run-status watcher publishes
//! `run.updated` / `step.updated` when a run or step row of the open
//! project changes. Rows: "Events", "Reconnect", "Too far back". Written
//! first.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod support;

use std::time::Duration;

use am_store::WriteCommand;
use support::*;

const WAIT: Duration = Duration::from_secs(5);

fn tick(rig: &Rig, n: u64) -> u64 {
    rig.state
        .events()
        .publish("test.tick", serde_json::json!({ "n": n }))
}

/// `seq` starts at 1 and increases by one per event.
#[tokio::test]
async fn seq_starts_at_one_and_increases() {
    let rig = Rig::start().await;
    let seqs: Vec<u64> = (1..=5).map(|n| tick(&rig, n)).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
}

/// Reconnect: a client reads 1..3 and disconnects; 4..6 happen; it
/// reconnects with `since=3` and gets 4, 5, 6 in order, then live events.
#[tokio::test]
async fn a_reconnect_with_since_replays_the_missed_events_then_streams() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    let first: Vec<u64> = (1..=3).map(|n| tick(&rig, n)).collect();
    let got = ws.take(3, WAIT).await;
    assert_eq!(got.iter().map(|e| e.seq).collect::<Vec<_>>(), first);
    assert!(got.iter().all(|e| e.kind == "test.tick"));
    let last = got.last().unwrap().seq;
    ws.close().await;

    let missed: Vec<u64> = (4..=6).map(|n| tick(&rig, n)).collect();
    let mut ws = rig.events_with_session(&s, Some(last)).await;
    let replay = ws.take(3, WAIT).await;
    assert_eq!(
        replay.iter().map(|e| e.seq).collect::<Vec<_>>(),
        missed,
        "{replay:?}"
    );
    assert_eq!(replay[0].data["n"], 4);
    assert_eq!(replay[2].data["n"], 6);

    let live = tick(&rig, 7);
    let e = ws.next_event(WAIT).await.expect("the live event");
    assert_eq!(e.seq, live);
    assert_eq!(e.kind, "test.tick");
    ws.close().await;
}

/// Too far back: with a window of 3 and 10 events, `since=2` first gets one
/// `resync`, then the held events 8, 9, 10; `since=7` (nothing missed
/// beyond the window) gets no `resync`.
#[tokio::test]
async fn a_since_older_than_the_window_gets_one_resync_first() {
    let rig = Rig::start_with(Options {
        project: false,
        event_window: Some(3),
    })
    .await;
    let s = rig.sign_in().await;
    for n in 1..=10 {
        tick(&rig, n);
    }
    let mut ws = rig.events_with_session(&s, Some(2)).await;
    let got = ws.take(4, WAIT).await;
    assert_eq!(got.len(), 4, "{got:?}");
    assert_eq!(got[0].kind, "resync", "{got:?}");
    assert_eq!(
        got[1..].iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![8, 9, 10]
    );
    assert!(got[1..].iter().all(|e| e.kind == "test.tick"));
    ws.close().await;

    let mut ws = rig.events_with_session(&s, Some(7)).await;
    let got = ws.take(3, WAIT).await;
    assert_eq!(
        got.iter()
            .map(|e| (e.seq, e.kind.as_str()))
            .collect::<Vec<_>>(),
        vec![(8, "test.tick"), (9, "test.tick"), (10, "test.tick")]
    );
    ws.close().await;
}

/// Events: a run of the open project (story 3.1's rows) changes status
/// while a client is connected; the watcher publishes `run.updated` with
/// the run's id and new status, and every `seq` the client sees increases.
#[tokio::test]
async fn a_run_changing_status_publishes_run_updated() {
    let rig = Rig::with_project().await;
    let run = rig.create_run(&["a", "b"]);
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    // Let the watcher take its first look, then finish the run.
    tokio::time::sleep(Duration::from_millis(200)).await;
    rig.exec(WriteCommand::FinishRun {
        run_id: run.clone(),
        status: "succeeded".into(),
    });

    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let found = loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(e) = ws.next_event(left).await else {
            break None;
        };
        seen.push(e.seq);
        if e.kind == "run.updated" && e.data["id"] == run && e.data["status"] == "succeeded" {
            break Some(e);
        }
    };
    assert!(
        found.is_some(),
        "no run.updated for the finished run; seqs {seen:?}"
    );
    assert!(
        seen.windows(2).all(|w| w[0] < w[1]),
        "seq increases: {seen:?}"
    );
    ws.close().await;
}

/// A step changing status publishes `step.updated` with the step's id,
/// its run's id and its new status.
#[tokio::test]
async fn a_step_changing_status_publishes_step_updated() {
    let rig = Rig::with_project().await;
    let run = rig.create_run(&["a"]);
    let step = am_store::Reader::open(&rig.runtime.as_ref().unwrap().state_db())
        .unwrap()
        .steps(&run)
        .unwrap()[0]
        .id
        .clone();
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    rig.exec(WriteCommand::StartStep {
        step_run_id: step.clone(),
    });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let found = loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Some(e) = ws.next_event(left).await else {
            break None;
        };
        if e.kind == "step.updated" && e.data["id"] == step && e.data["status"] == "running" {
            break Some(e);
        }
    };
    let e = found.expect("a step.updated for the started step");
    assert_eq!(e.data["runId"], run);
    ws.close().await;
}

// ------------------------------------------- review patch round 1 (P4, P8)

/// P4: `since=18446744073709551615` (u64::MAX) gets one `resync`, then the
/// held events oldest first, and the stream stays usable for live events
/// (no overflow, no dropped connection).
#[tokio::test]
async fn a_since_of_u64_max_gets_one_resync_and_the_stream_stays_usable() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let held: Vec<u64> = (1..=3).map(|n| tick(&rig, n)).collect();
    let mut ws = rig.events_with_session(&s, Some(u64::MAX)).await;
    let got = ws.take(4, WAIT).await;
    assert_eq!(got.len(), 4, "{got:?}");
    assert_eq!(got[0].kind, "resync", "{got:?}");
    assert_eq!(got[1..].iter().map(|e| e.seq).collect::<Vec<_>>(), held);
    let live = tick(&rig, 4);
    let e = ws
        .next_event(WAIT)
        .await
        .expect("the stream is still usable");
    assert_eq!(e.seq, live);
    ws.close().await;
    // The server is still up.
    assert_eq!(rig.get(HEALTH, &s.auth()).await.status, 200);
}

/// P4: `since=u64::MAX` before any event is published: one `resync`, then
/// live events.
#[tokio::test]
async fn a_since_of_u64_max_on_an_empty_log_gets_resync_then_live() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(u64::MAX)).await;
    let first = ws.next_event(WAIT).await.expect("a resync");
    assert_eq!(first.kind, "resync");
    let live = tick(&rig, 1);
    let e = ws.next_event(WAIT).await.expect("the live event");
    assert_eq!((e.seq, e.kind.as_str()), (live, "test.tick"));
    ws.close().await;
}

/// P4: a `since` greater than the last seq (another launch's cursor) gets
/// one `resync` first, then the held events.
#[tokio::test]
async fn a_since_past_the_last_seq_gets_one_resync() {
    let rig = Rig::start().await;
    let s = rig.sign_in().await;
    let held: Vec<u64> = (1..=2).map(|n| tick(&rig, n)).collect();
    let mut ws = rig.events_with_session(&s, Some(held[1] + 5)).await;
    let got = ws.take(3, WAIT).await;
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got[0].kind, "resync");
    assert_eq!(got[1..].iter().map(|e| e.seq).collect::<Vec<_>>(), held);
    ws.close().await;
}

/// P8: the project's store unreadable for a few watcher ticks (renamed
/// away), then back: rows that did not change are not published again (no
/// `run.updated` / `step.updated` burst).
#[tokio::test]
async fn a_store_unreadable_for_a_while_does_not_republish_unchanged_rows() {
    let rig = Rig::with_project().await;
    rig.create_run(&["a", "b"]);
    rig.create_run(&["c"]);
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    let s = rig.sign_in().await;
    // Let the watcher take its first looks.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut ws = rig.events_with_session(&s, None).await;

    let db = rig.runtime.as_ref().unwrap().state_db();
    let away = db.with_extension("away");
    std::fs::rename(&db, &away).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    std::fs::rename(&away, &db).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let burst = ws.take(10, Duration::from_millis(500)).await;
    let republished: Vec<_> = burst
        .iter()
        .filter(|e| e.kind == "run.updated" || e.kind == "step.updated")
        .collect();
    assert!(
        republished.is_empty(),
        "unchanged rows published again after a failed read: {republished:?}"
    );
    ws.close().await;
}
