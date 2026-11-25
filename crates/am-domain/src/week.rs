//! The budget week (story 3.7, owner Q11): the company-local week an instant
//! falls in, keyed by the date it starts.
//!
//! The ledger journals the key at reserve, from the run's clock in the
//! pinned snapshot's timezone and `week_start`, so a later timezone edit
//! moves no past spend. A week starting on a Monday also has its ISO label.
//!
//! ```
//! use am_domain::week::budget_week;
//! use jiff::civil::Weekday;
//! use jiff::tz::TimeZone;
//! let week = budget_week("2026-11-25T12:00:00Z".parse().unwrap(), &TimeZone::UTC, Weekday::Monday);
//! assert_eq!(week.key(), "2026-11-23");
//! assert_eq!(week.iso_label().as_deref(), Some("2026-W48"));
//! ```

use jiff::Timestamp;
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;

/// A company-local budget week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetWeek {
    /// The company-local date the week starts on.
    pub start: Date,
}

impl BudgetWeek {
    /// The ledger's key: the start date as `YYYY-MM-DD`.
    pub fn key(&self) -> String {
        self.start.to_string()
    }

    /// The ISO week label (`2026-W48`), only for a week starting on a
    /// Monday.
    pub fn iso_label(&self) -> Option<String> {
        if self.start.weekday() != Weekday::Monday {
            return None;
        }
        let iso = self.start.iso_week_date();
        Some(format!("{:04}-W{:02}", iso.year(), iso.week()))
    }
}

/// The week `at` falls in, in `tz`, for weeks starting on `week_start`.
pub fn budget_week(at: Timestamp, tz: &TimeZone, week_start: Weekday) -> BudgetWeek {
    let local = at.to_zoned_datetime(tz.clone()).date();
    let back = i64::from(local.weekday().since(week_start));
    // At most six days back from a date jiff produced; the earliest civil
    // date is the only place this can fail.
    let start = local
        .checked_sub(jiff::Span::new().days(back))
        .unwrap_or(local);
    BudgetWeek { start }
}
