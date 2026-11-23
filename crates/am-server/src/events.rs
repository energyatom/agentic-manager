//! The event stream (AD-14). [`EventLog`] numbers each event of this launch
//! with a monotonic `seq` from 1 and holds the last N (10 000) for replay;
//! `GET /api/v1/events?since=<seq>` upgrades to a WebSocket, replays every
//! held event after `since`, then streams new ones. A `since` older than the
//! held window (or newer than the last event, as after a restart) first
//! gets one `resync` event: the client re-reads run status. [`watch_runs`]
//! is the first producer: it reads the open project's runs and steps
//! through a read-only connection and publishes `run.updated` and
//! `step.updated` when a row's status or times change, so runs started by
//! `agentic run` in another process show up.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_api::{EVENT_RESYNC, EVENT_RUN_UPDATED, EVENT_STEP_UPDATED, EventDto, RunDto, StepDto};
use axum::extract::ws::{Message, WebSocket};
use tokio::sync::watch;

use crate::runs::{run_dto, step_dto};
use crate::state::AppState;

struct Log {
    events: VecDeque<EventDto>,
    window: usize,
    last: u64,
}

/// This launch's event log; clones share it.
#[derive(Clone)]
pub struct EventLog {
    log: Arc<Mutex<Log>>,
    tx: Arc<watch::Sender<u64>>,
}

impl EventLog {
    pub(crate) fn new(window: usize) -> EventLog {
        let (tx, _) = watch::channel(0);
        EventLog {
            log: Arc::new(Mutex::new(Log {
                events: VecDeque::new(),
                window,
                last: 0,
            })),
            tx: Arc::new(tx),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Publishes an event; returns its `seq` (from 1).
    pub fn publish(&self, kind: &str, data: serde_json::Value) -> u64 {
        let seq = {
            let mut log = self.lock();
            log.last = log.last.saturating_add(1);
            let seq = log.last;
            log.events.push_back(EventDto {
                seq,
                kind: kind.to_string(),
                data,
            });
            while log.events.len() > log.window {
                log.events.pop_front();
            }
            seq
        };
        self.tx.send_replace(seq);
        seq
    }

    /// The last `seq` published (0 before the first).
    pub fn last_seq(&self) -> u64 {
        self.lock().last
    }

    /// What a client that has seen up to `cursor` gets next: a `resync`
    /// first when events after `cursor` are no longer held (or `cursor` is
    /// past the last event), then the held events after `cursor`, oldest
    /// first.
    fn after(&self, cursor: u64) -> Vec<EventDto> {
        let log = self.lock();
        // Saturating throughout: `cursor` comes from the client (`since`).
        let oldest = log
            .events
            .front()
            .map_or(log.last.saturating_add(1), |e| e.seq);
        let mut out = Vec::new();
        let from = if cursor.saturating_add(1) < oldest || cursor > log.last {
            let before = oldest.saturating_sub(1);
            out.push(EventDto {
                seq: before,
                kind: EVENT_RESYNC.to_string(),
                data: serde_json::json!({}),
            });
            before
        } else {
            cursor
        };
        out.extend(log.events.iter().filter(|e| e.seq > from).cloned());
        out
    }
}

/// Serves one connected client from `since` (none: live from now).
pub(crate) async fn stream(mut socket: WebSocket, log: EventLog, since: Option<u64>) {
    // Subscribed before the replay, so nothing published meanwhile is lost.
    let mut rx = log.tx.subscribe();
    let mut cursor = since.unwrap_or_else(|| log.last_seq());
    loop {
        for e in log.after(cursor) {
            cursor = e.seq;
            let Ok(text) = serde_json::to_string(&e) else {
                continue;
            };
            if socket.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            msg = socket.recv() => match msg {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

/// The rows the watcher last saw.
#[derive(Default, Clone)]
struct Seen {
    runs: HashMap<String, RunDto>,
    steps: HashMap<String, StepDto>,
}

/// One look at the project's runs and steps through a read-only
/// connection: the runs, and the steps of every run that is unfinished,
/// new or changed (a finished run's steps no longer change).
fn look(db: &Path, seen: &Seen) -> Option<(Vec<RunDto>, Vec<StepDto>)> {
    let reader = am_store::Reader::open(db).ok()?;
    let runs: Vec<RunDto> = reader.runs().ok()?.into_iter().map(run_dto).collect();
    let mut steps = Vec::new();
    for run in &runs {
        let unchanged_and_done = seen
            .runs
            .get(&run.id)
            .is_some_and(|before| before == run && run.finished_at.is_some());
        if unchanged_and_done {
            continue;
        }
        let rows = reader.steps(&run.id).ok()?;
        steps.extend(rows.into_iter().map(|s| step_dto(&run.id, s)));
    }
    Some((runs, steps))
}

/// What one look found: the runs and the steps it read.
type Looked = (Vec<RunDto>, Vec<StepDto>);

/// Runs `look` on a blocking thread with what was seen; gives back what
/// was seen and what the look found (`None` when it found nothing or
/// failed). A look that panics (a `JoinError`) leaves what was seen as it
/// was, so the next look does not republish every unchanged row.
async fn read_off_thread<F>(seen: Seen, look: F) -> (Seen, Option<Looked>)
where
    F: FnOnce(&Seen) -> Option<Looked> + Send + 'static,
{
    let seen = Arc::new(seen);
    let shared = Arc::clone(&seen);
    let looked = tokio::task::spawn_blocking(move || look(&shared))
        .await
        .ok()
        .flatten();
    // The blocking task has ended (or unwound) and dropped its handle, so
    // this is normally the only one; a clone otherwise.
    (Arc::unwrap_or_clone(seen), looked)
}

/// Watches the open project's runs and steps every `every` and publishes
/// `run.updated` (the run's DTO) and `step.updated` (the step's DTO, with
/// its `runId`) when a row is new or its status or times changed. The first
/// look only records what is there. With no project it publishes nothing.
pub fn watch_runs(state: &AppState, every: Duration) -> tokio::task::JoinHandle<()> {
    let db = state.project_db();
    let log = state.events();
    tokio::spawn(async move {
        let Some(db) = db else {
            return;
        };
        let mut seen = Seen::default();
        let mut first = true;
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let path = db.clone();
            let looked;
            (seen, looked) = read_off_thread(seen, move |before| look(&path, before)).await;
            let Some((runs, steps)) = looked else {
                continue;
            };
            for run in runs {
                if !first && seen.runs.get(&run.id) != Some(&run) {
                    log.publish(
                        EVENT_RUN_UPDATED,
                        serde_json::to_value(&run).unwrap_or_default(),
                    );
                }
                seen.runs.insert(run.id.clone(), run);
            }
            for step in steps {
                if !first && seen.steps.get(&step.id) != Some(&step) {
                    log.publish(
                        EVENT_STEP_UPDATED,
                        serde_json::to_value(&step).unwrap_or_default(),
                    );
                }
                seen.steps.insert(step.id.clone(), step);
            }
            first = false;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str) -> RunDto {
        RunDto {
            id: id.into(),
            workflow: "w".into(),
            status: "running".into(),
            created_at: "2026-11-23T10:00:00.000Z".into(),
            finished_at: None,
        }
    }

    /// Review P8: a look that panics on its blocking thread (a `JoinError`)
    /// leaves what was seen as it was, so the next look does not republish
    /// every unchanged row.
    #[tokio::test]
    async fn a_failed_look_keeps_what_was_seen() {
        let mut seen = Seen::default();
        seen.runs.insert("r1".into(), run("r1"));
        let (seen, looked) = read_off_thread(seen, |_| -> Option<Looked> {
            panic!("a read that panics (test)")
        })
        .await;
        assert!(looked.is_none());
        assert_eq!(
            seen.runs.get("r1"),
            Some(&run("r1")),
            "what was seen is kept"
        );
    }

    #[test]
    fn replay_and_resync() {
        let log = EventLog::new(3);
        for n in 1..=10 {
            assert_eq!(log.publish("t", serde_json::json!(n)), n);
        }
        let got = log.after(2);
        assert_eq!(got[0].kind, EVENT_RESYNC);
        assert_eq!(
            got[1..].iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![8, 9, 10]
        );
        let got = log.after(7);
        assert_eq!(
            got.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![8, 9, 10]
        );
        assert!(log.after(10).is_empty());
        // Past the last event (another launch's cursor): resync.
        assert_eq!(log.after(11)[0].kind, EVENT_RESYNC);
        // Nothing published yet, from 0: nothing.
        assert!(EventLog::new(3).after(0).is_empty());
        // Review P4: the largest cursor does not overflow.
        assert_eq!(log.after(u64::MAX)[0].kind, EVENT_RESYNC);
        assert_eq!(EventLog::new(3).after(u64::MAX)[0].seq, 0);
    }
}
