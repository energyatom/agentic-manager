//! Clock port (AD-5, AD-18). The scheduler and journal read time only here, so
//! tests drive time with a fake clock.

pub trait Clock: Send + Sync {
    /// Current wall-clock time (UTC instant).
    fn now(&self) -> jiff::Timestamp;
}
