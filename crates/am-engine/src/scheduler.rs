//! The scheduler (AD-18): a small loop of our own over croner (jiff backend)
//! and jiff time zones, using the `Clock` port. Each trigger persists
//! `next_fire`, `last_fired` and its state; the loop ticks at most every 30 s,
//! detects wall-clock jumps (sleep/wake) and evaluates the catch-up policy
//! (`once` by default, `skip`, `all`) from the persisted state.
//!
//! croner 4.0.3's jiff backend is built on jiff 0.2 while the workspace pins
//! jiff 1.0; conversion happens only in [`next_after`], through Unix
//! milliseconds and the IANA zone name (docs/crate-notes/croner.md).

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use am_domain::{Actor, Clock};
use am_store::{Reader, StoreError, TriggerRow, WriteCommand, Writer, ts};
use jiff::Timestamp;
use tokio_util::sync::CancellationToken;

/// The longest the loop sleeps between ticks (AD-18).
pub const MAX_TICK: Duration = Duration::from_secs(30);
/// A fire this late or less still counts as on time.
pub const ON_TIME_GRACE: jiff::SignedDuration = jiff::SignedDuration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("invalid cron pattern {0}: {1}")]
    Cron(String, String),
    #[error("unknown time zone {0}")]
    Tz(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{0}")]
    Time(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchUp {
    Once,
    Skip,
    All,
}

impl CatchUp {
    pub fn parse(s: &str) -> CatchUp {
        match s {
            "skip" => CatchUp::Skip,
            "all" => CatchUp::All,
            _ => CatchUp::Once,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireKind {
    OnTime,
    CatchUp,
}

impl FireKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FireKind::OnTime => "on_time",
            FireKind::CatchUp => "catch_up",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fire {
    pub trigger_id: String,
    pub scheduled_for: Timestamp,
    pub kind: FireKind,
}

/// The first occurrence strictly after `after`, in the trigger's time zone.
pub fn next_after(cron: &str, tz: &str, after: Timestamp) -> Result<Timestamp, ScheduleError> {
    let pattern = croner::Cron::from_str(cron)
        .map_err(|e| ScheduleError::Cron(cron.to_string(), e.to_string()))?;
    let start = jiff_croner::Timestamp::from_millisecond(after.as_millisecond())
        .map_err(|e| ScheduleError::Time(e.to_string()))?
        .in_tz(tz)
        .map_err(|_| ScheduleError::Tz(tz.to_string()))?;
    let next = pattern
        .find_next_occurrence(&start, false)
        .map_err(|e| ScheduleError::Cron(cron.to_string(), e.to_string()))?;
    Timestamp::from_millisecond(next.timestamp().as_millisecond())
        .map_err(|e| ScheduleError::Time(e.to_string()))
}

/// Pure decision: which fires are due at `now`, and the new `next_fire`.
pub fn due(
    cron: &str,
    tz: &str,
    policy: CatchUp,
    next_fire: Timestamp,
    now: Timestamp,
) -> Result<(Vec<(Timestamp, FireKind)>, Timestamp), ScheduleError> {
    if now < next_fire {
        return Ok((Vec::new(), next_fire));
    }
    // Every occurrence in [next_fire, now] (bounded, so a long sleep cannot spin).
    let mut missed = vec![next_fire];
    let mut cursor = next_fire;
    loop {
        let n = next_after(cron, tz, cursor)?;
        if n > now || missed.len() >= 10_000 {
            break;
        }
        missed.push(n);
        cursor = n;
    }
    let new_next = next_after(cron, tz, now)?;
    let last = *missed.last().unwrap_or(&next_fire);
    let on_time = missed.len() == 1 && now.duration_since(last) <= ON_TIME_GRACE;
    let fires = if on_time {
        vec![(last, FireKind::OnTime)]
    } else {
        match policy {
            CatchUp::Once => vec![(last, FireKind::CatchUp)],
            CatchUp::Skip => Vec::new(),
            CatchUp::All => missed.into_iter().map(|t| (t, FireKind::CatchUp)).collect(),
        }
    };
    Ok((fires, new_next))
}

pub struct Scheduler {
    clock: Arc<dyn Clock>,
    writer: Writer,
    db: PathBuf,
    trigger_ids: Vec<String>,
}

fn sched_actor() -> Actor {
    Actor::System {
        component: "scheduler".into(),
    }
}

impl Scheduler {
    pub fn new(clock: Arc<dyn Clock>, writer: Writer, db: PathBuf) -> Self {
        Scheduler {
            clock,
            writer,
            db,
            trigger_ids: Vec::new(),
        }
    }

    /// Register (or re-register) a trigger. A trigger already persisted keeps
    /// its `next_fire`, so a restart or wake catches up from persisted state.
    pub fn add_trigger(
        &mut self,
        id: &str,
        cron: &str,
        tz: &str,
        catch_up: CatchUp,
    ) -> Result<(), ScheduleError> {
        let existing = Reader::open(&self.db)?.trigger(id)?;
        let next_fire = match existing.as_ref().and_then(|t| t.next_fire.clone()) {
            Some(n) if existing.as_ref().map(|t| t.cron.as_str()) == Some(cron) => n,
            _ => ts(next_after(cron, tz, self.clock.now())?),
        };
        let row = TriggerRow {
            id: id.into(),
            cron: cron.into(),
            tz: tz.into(),
            catch_up: match catch_up {
                CatchUp::Once => "once",
                CatchUp::Skip => "skip",
                CatchUp::All => "all",
            }
            .into(),
            state: existing
                .as_ref()
                .map(|t| t.state.clone())
                .unwrap_or_else(|| "active".into()),
            next_fire: Some(next_fire),
            last_fired: existing.and_then(|t| t.last_fired),
        };
        self.writer
            .exec(sched_actor(), WriteCommand::UpsertTrigger { trigger: row })?;
        if !self.trigger_ids.iter().any(|t| t == id) {
            self.trigger_ids.push(id.to_string());
        }
        Ok(())
    }

    /// One tick: evaluate every active trigger against the clock and persist
    /// each fire with the new `next_fire` before returning it.
    pub fn tick(&self) -> Result<Vec<Fire>, ScheduleError> {
        let reader = Reader::open(&self.db)?;
        let now = self.clock.now();
        let mut out = Vec::new();
        for id in &self.trigger_ids {
            let Some(t) = reader.trigger(id)? else {
                continue;
            };
            if t.state != "active" {
                continue;
            }
            let Some(next) = t.next_fire.as_deref() else {
                continue;
            };
            let next: Timestamp = next
                .parse()
                .map_err(|e: jiff::Error| ScheduleError::Time(e.to_string()))?;
            let (fires, new_next) = due(&t.cron, &t.tz, CatchUp::parse(&t.catch_up), next, now)?;
            if fires.is_empty() && new_next != next {
                // Skipped (policy skip): persist the new next fire only.
                let mut row = t.clone();
                row.next_fire = Some(ts(new_next));
                self.writer
                    .exec(sched_actor(), WriteCommand::UpsertTrigger { trigger: row })?;
            }
            for (at, kind) in fires {
                self.writer.exec(
                    sched_actor(),
                    WriteCommand::RecordFire {
                        trigger_id: id.clone(),
                        scheduled_for: ts(at),
                        kind: kind.as_str().into(),
                        next_fire: ts(new_next),
                    },
                )?;
                out.push(Fire {
                    trigger_id: id.clone(),
                    scheduled_for: at,
                    kind,
                });
            }
        }
        Ok(out)
    }

    /// The earliest persisted `next_fire` among active triggers.
    pub fn earliest_next(&self) -> Result<Option<Timestamp>, ScheduleError> {
        let reader = Reader::open(&self.db)?;
        let mut best: Option<Timestamp> = None;
        for id in &self.trigger_ids {
            if let Some(t) = reader.trigger(id)?
                && t.state == "active"
                && let Some(n) = t.next_fire.and_then(|n| n.parse::<Timestamp>().ok())
            {
                best = Some(best.map_or(n, |b| b.min(n)));
            }
        }
        Ok(best)
    }

    /// The loop: tick, then sleep until the next fire or at most 30 s. A wall
    /// clock that moved much further than the monotonic clock (the Mac slept)
    /// is logged; the catch-up itself comes from the persisted `next_fire`.
    pub async fn run<F>(
        self,
        cancel: CancellationToken,
        mut on_fire: F,
    ) -> Result<(), ScheduleError>
    where
        F: FnMut(&Fire, Option<jiff::SignedDuration>) + Send,
    {
        let mut last_wall = self.clock.now();
        let mut last_mono = Instant::now();
        loop {
            let wall = self.clock.now();
            let mono = Instant::now();
            let wall_delta = wall.duration_since(last_wall);
            let mono_delta = jiff::SignedDuration::try_from(mono - last_mono)
                .unwrap_or(jiff::SignedDuration::ZERO);
            let jump = if wall_delta - mono_delta > jiff::SignedDuration::from_secs(5) {
                tracing::info!(
                    wall_secs = wall_delta.as_secs(),
                    mono_secs = mono_delta.as_secs(),
                    "wall clock jumped (sleep/wake?)"
                );
                Some(wall_delta - mono_delta)
            } else {
                None
            };
            last_wall = wall;
            last_mono = mono;
            for f in self.tick()? {
                on_fire(&f, jump);
            }
            let sleep = match self.earliest_next()? {
                Some(n) => {
                    let until = n.duration_since(self.clock.now());
                    if until <= jiff::SignedDuration::ZERO {
                        Duration::from_millis(10)
                    } else {
                        Duration::try_from(until).unwrap_or(MAX_TICK).min(MAX_TICK)
                    }
                }
                None => MAX_TICK,
            };
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::sleep(sleep) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn next_respects_timezone() {
        // 09:00 in Asia/Kolkata (UTC+05:30) is 03:30Z.
        let n = next_after("0 9 * * *", "Asia/Kolkata", t("2026-11-19T00:00:00Z")).unwrap();
        assert_eq!(n, t("2026-11-19T03:30:00Z"));
        // 09:00 in New York on a winter day is 14:00Z.
        let n = next_after("0 9 * * *", "America/New_York", t("2026-11-19T00:00:00Z")).unwrap();
        assert_eq!(n, t("2026-11-19T14:00:00Z"));
    }

    #[test]
    fn on_time_and_catch_up_policies() {
        let cron = "0 * * * *";
        let next = t("2026-11-19T10:00:00Z");
        let (f, n) = due(cron, "UTC", CatchUp::Once, next, t("2026-11-19T10:00:05Z")).unwrap();
        assert_eq!(f, vec![(next, FireKind::OnTime)]);
        assert_eq!(n, t("2026-11-19T11:00:00Z"));
        // Asleep from 09:59 to 13:30: three fire times missed.
        let now = t("2026-11-19T13:30:00Z");
        let (f, n) = due(cron, "UTC", CatchUp::Once, next, now).unwrap();
        assert_eq!(f, vec![(t("2026-11-19T13:00:00Z"), FireKind::CatchUp)]);
        assert_eq!(n, t("2026-11-19T14:00:00Z"));
        let (f, _) = due(cron, "UTC", CatchUp::Skip, next, now).unwrap();
        assert!(f.is_empty());
        let (f, _) = due(cron, "UTC", CatchUp::All, next, now).unwrap();
        assert_eq!(f.len(), 4);
    }
}
