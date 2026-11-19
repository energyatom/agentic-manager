//! `am-engine`: the application core. Spike scope: the cancellation tree
//! (AD-20), the journaled run executor with resume (AD-8, AD-9), the
//! scheduler loop (AD-18) and ToolGate's per-step tool limits (AD-11).
#![forbid(unsafe_code)]

pub mod cancel;
pub mod journal;
pub mod scheduler;
pub mod toolgate;

pub use cancel::{CancelTree, TokenSignal};
