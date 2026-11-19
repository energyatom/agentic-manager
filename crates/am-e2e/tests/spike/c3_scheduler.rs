//! Criterion 3: croner + jiff fire a cron with a time zone, and a clock that
//! jumps past one or more fire times (the Mac slept) triggers exactly one
//! catch-up (`once` policy), from persisted state, also across a restart.
//! The real sleep/wake run is the owner's demo: `agentic spike c3`.

use std::sync::Arc;

use am_engine::scheduler::{CatchUp, FireKind, Scheduler};
use am_store::{Reader, Store};
use am_testkit::FakeClock;
use jiff::Timestamp;

fn t(s: &str) -> Timestamp {
    s.parse().unwrap()
}

#[test]
fn fires_on_time_in_zone_and_catches_up_once_after_a_jump() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    // 09:30 every day in Asia/Kolkata (UTC+05:30) is 04:00Z.
    let clock = FakeClock::at("2026-11-19T03:59:00Z");
    let mut s = Scheduler::new(clock.clone(), store.writer(), db.clone());
    s.add_trigger("daily", "30 9 * * *", "Asia/Kolkata", CatchUp::Once)
        .unwrap();
    let r = Reader::open(&db).unwrap();
    assert_eq!(
        r.trigger("daily").unwrap().unwrap().next_fire.as_deref(),
        Some("2026-11-19T04:00:00.000Z")
    );

    assert!(s.tick().unwrap().is_empty());
    clock.set("2026-11-19T04:00:05Z");
    let fires = s.tick().unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].kind, FireKind::OnTime);
    assert_eq!(fires[0].scheduled_for, t("2026-11-19T04:00:00Z"));
    assert!(s.tick().unwrap().is_empty(), "no double fire");

    // The Mac sleeps for three days across three fire times.
    clock.set("2026-11-22T10:00:00Z");
    let fires = s.tick().unwrap();
    assert_eq!(fires.len(), 1, "exactly one catch-up");
    assert_eq!(fires[0].kind, FireKind::CatchUp);
    assert_eq!(fires[0].scheduled_for, t("2026-11-22T04:00:00Z"));
    assert!(s.tick().unwrap().is_empty());
    assert_eq!(
        r.trigger("daily").unwrap().unwrap().next_fire.as_deref(),
        Some("2026-11-23T04:00:00.000Z")
    );
    assert_eq!(r.fires("daily").unwrap().len(), 2);
}

#[test]
fn catch_up_comes_from_persisted_state_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let clock = FakeClock::at("2026-11-19T09:59:00Z");
    {
        let store = Store::open(&db).unwrap();
        let mut s = Scheduler::new(clock.clone(), store.writer(), db.clone());
        s.add_trigger("hourly", "0 * * * *", "Europe/Berlin", CatchUp::Once)
            .unwrap();
        clock.set("2026-11-19T10:00:01Z");
        assert_eq!(s.tick().unwrap().len(), 1);
    } // the app stops

    clock.set("2026-11-19T15:20:00Z"); // five hourly fires missed
    let store = Store::open(&db).unwrap();
    let mut s = Scheduler::new(clock.clone(), store.writer(), db.clone());
    s.add_trigger("hourly", "0 * * * *", "Europe/Berlin", CatchUp::Once)
        .unwrap();
    let fires = s.tick().unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].kind, FireKind::CatchUp);
    assert_eq!(fires[0].scheduled_for, t("2026-11-19T15:00:00Z"));
}

#[test]
fn skip_and_all_policies() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    let clock = FakeClock::at("2026-11-19T09:59:00Z");
    let mut s = Scheduler::new(Arc::clone(&clock) as _, store.writer(), db.clone());
    s.add_trigger("skip", "0 * * * *", "UTC", CatchUp::Skip)
        .unwrap();
    s.add_trigger("all", "0 * * * *", "UTC", CatchUp::All)
        .unwrap();
    clock.set("2026-11-19T12:30:00Z");
    let fires = s.tick().unwrap();
    assert!(fires.iter().all(|f| f.trigger_id == "all"));
    assert_eq!(fires.len(), 3);
    let r = Reader::open(&db).unwrap();
    assert_eq!(
        r.trigger("skip").unwrap().unwrap().next_fire.as_deref(),
        Some("2026-11-19T13:00:00.000Z")
    );
}
